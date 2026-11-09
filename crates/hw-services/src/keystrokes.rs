//! `hw.service.keystrokes`: key presses read from `/dev/input/event*`
//! (replaces AGS' `keystroke-loop-ags.c`, which piped `xkbcli
//! interactive-evdev`). Keycodes are turned into keysyms with xkbcommon using
//! Hyprland's `input:kb_layout`/`kb_variant`, then into the same labels the C
//! helper printed (`ENTER`, `SUPER`, `󱁐`…).
//!
//! Needs read access to the keyboard devices: they are `root:input 0660`, so
//! the user must be in the `input` group (`sudo usermod -aG input $USER`, then
//! log in again). `available` is false and `error` explains it otherwise.
//! Nothing is read until `enable(true)`.
//!
//! ```lua
//! local ks = hw.service.keystrokes
//! ks:enable(true)
//! ks.key:subscribe(function(k) print(k.label, k.keysym) end)
//! ```

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use hw_core::hyprland;
use serde_json::{Value, json};
use xkbcommon::xkb;

use crate::service::{Pusher, Service};

const PROPS: &[&str] = &["enabled", "available", "error", "devices", "key"];
const KEY_A: usize = 30;
const KEY_VOLUMEUP: usize = 115;
const EV_KEY: u16 = 1;
const RESCAN: Duration = Duration::from_secs(5);
const POLL_MS: i32 = 500;

pub fn start() -> Result<Rc<Service>> {
    let enabled = Arc::new(AtomicBool::new(false));
    let running = Arc::new(AtomicBool::new(false));
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("keystrokes", PROPS);
        let (en, run, w) = (enabled.clone(), running.clone(), weak.clone());
        svc.method("enable", move |args| {
            let on = args.first().and_then(Value::as_bool).unwrap_or(true);
            en.store(on, Ordering::SeqCst);
            let Some(svc) = w.upgrade() else {
                return Ok(Value::Null);
            };
            svc.set("enabled", json!(on));
            // One reader thread at a time; it exits soon after disabling.
            if on && !run.swap(true, Ordering::SeqCst) {
                let (en, run, pusher) = (en.clone(), run.clone(), svc.pusher());
                std::thread::Builder::new()
                    .name("hw-keystrokes".into())
                    .spawn(move || {
                        reader(&en, &pusher);
                        run.store(false, Ordering::SeqCst);
                    })?;
            }
            Ok(Value::Null)
        });
        svc
    });
    svc.set("enabled", json!(false));
    let (devs, err) = scan();
    svc.set("available", json!(!devs.is_empty()));
    svc.set("error", json!(err));
    svc.set(
        "devices",
        json!(devs.iter().map(|d| &d.name).collect::<Vec<_>>()),
    );
    Ok(svc)
}

#[derive(Debug, Clone, PartialEq)]
struct Device {
    name: String,
    path: PathBuf,
}

/// Readable keyboards, and why some (or all) can't be read.
fn scan() -> (Vec<Device>, Option<String>) {
    let text = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    let mut ok = Vec::new();
    let mut denied = Vec::new();
    for d in keyboards(&text) {
        match File::open(&d.path) {
            Ok(_) => ok.push(d),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied.push(d),
            Err(_) => {}
        }
    }
    let err = if ok.is_empty() && !denied.is_empty() {
        Some(format!(
            "no permission to read {} (add yourself to the 'input' group: sudo usermod -aG input $USER, then log in again)",
            denied[0].path.display()
        ))
    } else if ok.is_empty() {
        Some("no keyboard found in /proc/bus/input/devices".into())
    } else {
        None
    };
    (ok, err)
}

/// Devices with letter keys (keyboards) or volume keys (media remotes).
fn keyboards(proc_devices: &str) -> Vec<Device> {
    let mut out = Vec::new();
    for block in proc_devices.split("\n\n") {
        let mut name = String::new();
        let mut event = None;
        let mut keys = None;
        for line in block.lines() {
            if let Some(n) = line.strip_prefix("N: Name=") {
                name = n.trim_matches('"').to_owned();
            } else if let Some(h) = line.strip_prefix("H: Handlers=") {
                event = h.split_whitespace().find(|w| w.starts_with("event"));
            } else if let Some(k) = line.strip_prefix("B: KEY=") {
                keys = Some(k.trim());
            }
        }
        let (Some(ev), Some(keys)) = (event, keys) else {
            continue;
        };
        if has_bit(keys, KEY_A) || has_bit(keys, KEY_VOLUMEUP) {
            out.push(Device {
                name,
                path: PathBuf::from("/dev/input").join(ev),
            });
        }
    }
    out
}

/// `B: KEY=` is a list of 64-bit hex words, most significant first.
fn has_bit(bitmap: &str, bit: usize) -> bool {
    let words: Vec<&str> = bitmap.split_whitespace().collect();
    let Some(w) = words.len().checked_sub(1 + bit / 64).map(|i| words[i]) else {
        return false;
    };
    u64::from_str_radix(w, 16).is_ok_and(|w| w >> (bit % 64) & 1 == 1)
}

fn layout() -> (String, String) {
    let get = |r: Option<Value>| {
        r.and_then(|v| v["str"].as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    match hyprland::get_options(&["input:kb_layout", "input:kb_variant"]) {
        Ok(mut v) if v.len() == 2 => {
            let variant = get(v.pop().flatten());
            let layout = get(v.pop().flatten());
            let first = |s: String| s.split(',').next().unwrap_or("").trim().to_owned();
            let l = first(layout);
            (if l.is_empty() { "us".into() } else { l }, first(variant))
        }
        _ => ("us".into(), String::new()),
    }
}

struct Open {
    dev: Device,
    file: File,
}

fn reader(enabled: &AtomicBool, pusher: &Pusher) {
    let (l, v) = layout();
    let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
    let Some(keymap) = xkb::Keymap::new_from_names(
        &ctx,
        "evdev",
        "pc105",
        &l,
        &v,
        None,
        xkb::KEYMAP_COMPILE_NO_FLAGS,
    ) else {
        pusher.set("error", json!(format!("xkb keymap {l}/{v} failed")));
        return;
    };
    let mut decoder = Decoder(xkb::State::new(&keymap));
    let mut open: Vec<Open> = Vec::new();
    let mut last_scan: Option<Instant> = None;
    let mut buf = [0u8; 24 * 64];
    while enabled.load(Ordering::SeqCst) {
        if last_scan.is_none_or(|t| t.elapsed() >= RESCAN) {
            last_scan = Some(Instant::now());
            let (devs, err) = scan();
            open.retain(|o| devs.contains(&o.dev));
            let new: Vec<Device> = devs
                .iter()
                .filter(|d| !open.iter().any(|o| o.dev == **d))
                .cloned()
                .collect();
            for d in new {
                if let Ok(file) = std::fs::OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&d.path)
                {
                    open.push(Open { dev: d, file });
                }
            }
            if !pusher.set("available", json!(!devs.is_empty()))
                || !pusher.set("error", json!(err))
                || !pusher.set(
                    "devices",
                    json!(devs.iter().map(|d| &d.name).collect::<Vec<_>>()),
                )
            {
                return;
            }
        }
        if open.is_empty() {
            std::thread::sleep(Duration::from_millis(POLL_MS as u64));
            continue;
        }
        let mut fds: Vec<libc::pollfd> = open
            .iter()
            .map(|o| libc::pollfd {
                fd: o.file.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        // SAFETY: `fds` is a valid array of `fds.len()` pollfds.
        let n = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, POLL_MS) };
        if n <= 0 {
            continue;
        }
        let mut gone = Vec::new();
        for (i, fd) in fds.iter().enumerate() {
            if fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                gone.push(i);
                continue;
            }
            if fd.revents & libc::POLLIN == 0 {
                continue;
            }
            let Ok(len) = open[i].file.read(&mut buf) else {
                continue;
            };
            for name in decoder.feed(&buf[..len]) {
                if enabled.load(Ordering::SeqCst)
                    && !pusher.emit("key", json!({ "label": label(&name), "keysym": name }))
                {
                    return;
                }
            }
        }
        for i in gone.into_iter().rev() {
            open.remove(i);
        }
    }
}

/// Turns raw `struct input_event`s (24 bytes on 64-bit) into the keysym
/// names of key presses, tracking modifiers.
struct Decoder(xkb::State);

impl Decoder {
    fn feed(&mut self, buf: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for ev in buf.as_chunks::<24>().0 {
            let ty = u16::from_ne_bytes([ev[16], ev[17]]);
            let code = u16::from_ne_bytes([ev[18], ev[19]]);
            let value = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
            // value: 1 press, 0 release, 2 autorepeat (ignored).
            if ty != EV_KEY || value == 2 {
                continue;
            }
            let kc = xkb::Keycode::new(u32::from(code) + 8);
            if value == 1 {
                let name = xkb::keysym_get_name(self.0.key_get_one_sym(kc));
                if !name.is_empty() && name != "NoSymbol" {
                    out.push(name);
                }
            }
            let dir = if value == 1 {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            };
            self.0.update_key(kc, dir);
        }
        out
    }
}

/// `map_key` of `keystroke-loop-ags.c`.
pub fn label(keysym: &str) -> String {
    match keysym {
        "Return" => "ENTER",
        "Escape" => "󱊷",
        "BackSpace" => "󰌍",
        "Tab" => "󰌒",
        "Shift_L" | "Shift_R" => "󰘶",
        "Control_L" | "Control_R" => "CTRL",
        "Alt_L" | "Alt_R" => "ALT",
        "Super_L" | "Super_R" => "SUPER",
        "Up" => "UP",
        "Down" => "DOWN",
        "Left" => "LEFT",
        "Right" => "RIGHT",
        "space" => "󱁐",
        "XF86AudioRaiseVolume" => "󰝝",
        "XF86AudioLowerVolume" => "󰝞",
        "XF86AudioMute" => "󰝟",
        other => other,
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICES: &str = "I: Bus=0019\nN: Name=\"Power Button\"\nH: Handlers=kbd event1 \nB: EV=3\nB: KEY=8000 10000000000000 0\n\n\
I: Bus=0003\nN: Name=\"Compx VGN Mouse 2.4G Receiver\"\nH: Handlers=sysrq kbd leds event7 \nB: EV=120013\nB: KEY=1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe\n\n\
I: Bus=0003\nN: Name=\"Mouse\"\nH: Handlers=event13 mouse0 \nB: EV=17\n";

    #[test]
    fn finds_keyboards() {
        let k = keyboards(DEVICES);
        assert_eq!(k.len(), 1);
        assert_eq!(k[0].path, PathBuf::from("/dev/input/event7"));
        assert!(!has_bit("8000 10000000000000 0", KEY_A));
        assert!(has_bit("8000 10000000000000 0", 116)); // KEY_POWER
    }

    fn ev(ty: u16, code: u16, value: i32) -> Vec<u8> {
        let mut b = vec![0u8; 16];
        b.extend(ty.to_ne_bytes());
        b.extend(code.to_ne_bytes());
        b.extend(value.to_ne_bytes());
        b
    }

    #[test]
    fn decodes_presses_with_modifiers() {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let Some(km) = xkb::Keymap::new_from_names(
            &ctx,
            "evdev",
            "pc105",
            "us",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ) else {
            return; // no xkb data on this machine
        };
        let mut d = Decoder(xkb::State::new(&km));
        let mut buf = Vec::new();
        for (c, v) in [
            (30, 1),
            (30, 0),
            (42, 1),
            (30, 1),
            (30, 2),
            (30, 0),
            (42, 0),
            (28, 1),
        ] {
            buf.extend(ev(EV_KEY, c, v));
            buf.extend(ev(0, 0, 0)); // SYN_REPORT
        }
        assert_eq!(d.feed(&buf), ["a", "Shift_L", "A", "Return"]);
    }

    #[test]
    fn labels() {
        assert_eq!(label("Return"), "ENTER");
        assert_eq!(label("Super_L"), "SUPER");
        assert_eq!(label("a"), "a");
    }
}
