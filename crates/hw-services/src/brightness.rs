//! `hw.service.brightness`: laptop panels from `/sys/class/backlight`
//! (set through `brightnessctl`, which goes via logind) and external
//! monitors over DDC/CI with `ddcutil` (VCP 0x10), e.g. DP-2 on this desktop.
//! DDC is slow (~100 ms per call), so it is polled every 30 s and updated
//! optimistically when set. Writes go through one thread, one `ddcutil` at a
//! time, and only the latest value per display is written: a slider being
//! dragged would otherwise start dozens of `ddcutil` processes fighting over
//! the I2C bus.
//!
//! A monitor's DDC brightness 0 is often still bright, so while hyprsunset
//! runs the bottom [`SOFT_RANGE`] of a DDC display's 0..1 is software
//! dimming: DDC stays at 0 and hyprsunset's gamma goes down to
//! [`SOFT_FLOOR`]. hyprsunset's gamma covers every output, and one of its
//! profiles that sets gamma overrides the dimming.
//!
//! ```lua
//! local b = hw.service.brightness
//! b.brightness()               -- first display, 0..1 (nil if none)
//! b:set(0.6)                   -- every display
//! b:set(0.6, "ddc:1")          -- one display (see b.displays())
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::service::{Pusher, Service, arg_f64, arg_str};

pub const PROPS: &[&str] = &["available", "brightness", "displays"];
const BACKLIGHT: &str = "/sys/class/backlight";
const BACKLIGHT_EVERY: Duration = Duration::from_secs(2);
/// DDC reads happen every this many backlight ticks (30 s).
const DDC_EVERY: u32 = 15;
/// Part of a DDC display's 0..1 below its hardware minimum, done with gamma.
const SOFT_RANGE: f64 = 0.2;
/// hyprsunset gamma (fraction) at brightness 0: black, like the screen is off.
const SOFT_FLOOR: f64 = 0.0;

#[derive(Debug, Clone, PartialEq)]
struct Display {
    /// `backlight:<device>` or `ddc:<display number>`.
    id: String,
    kind: &'static str,
    name: String,
    value: f64,
    max: f64,
    /// hyprsunset's gamma (1 = none) for DDC displays; `None` without
    /// hyprsunset, which leaves no software dimming.
    gamma: Option<f64>,
}

impl Display {
    /// 0..1 as the slider shows it, software dimming included.
    fn level(&self) -> f64 {
        if self.max <= 0.0 {
            return 0.0;
        }
        let hw = self.value / self.max;
        match self.gamma {
            None => hw,
            Some(_) if self.value > 0.0 => SOFT_RANGE + (1.0 - SOFT_RANGE) * hw,
            Some(g) => SOFT_RANGE * ((g - SOFT_FLOOR) / (1.0 - SOFT_FLOOR)).clamp(0.0, 1.0),
        }
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id,
            "kind": self.kind,
            "name": self.name,
            "brightness": (self.level() * 1000.0).round() / 1000.0,
            "value": self.value,
            "max": self.max,
        })
    }
}

type Shared = Arc<Mutex<Vec<Display>>>;

/// Held for every `ddcutil` call: concurrent calls on one bus fail to lock it.
static DDC_BUS: Mutex<()> = Mutex::new(());

/// Writes brightness to DDC displays from its own thread, one at a time.
#[derive(Debug, Default)]
struct DdcWriter {
    queue: Mutex<DdcQueue>,
    wake: Condvar,
}

#[derive(Debug, Default)]
struct DdcQueue {
    /// Display number → raw value not written yet (newer values replace it).
    pending: BTreeMap<String, f64>,
    /// A write is running.
    writing: bool,
}

impl DdcWriter {
    fn queue(&self, display: &str, raw: f64) {
        let mut q = self.queue.lock().expect("ddc queue lock poisoned");
        q.pending.insert(display.to_owned(), raw);
        self.wake.notify_one();
    }

    /// Writes are queued or running, so a read now would return a stale value.
    fn busy(&self) -> bool {
        let q = self.queue.lock().expect("ddc queue lock poisoned");
        q.writing || !q.pending.is_empty()
    }

    fn run(&self) {
        loop {
            let (display, raw) = {
                let mut q = self.queue.lock().expect("ddc queue lock poisoned");
                q.writing = false;
                while q.pending.is_empty() {
                    q = self.wake.wait(q).expect("ddc queue lock poisoned");
                }
                q.writing = true;
                q.pending.pop_first().expect("pending is not empty")
            };
            let _bus = DDC_BUS.lock().expect("ddc bus lock poisoned");
            let _ = run(
                "ddcutil",
                &["--display", &display, "setvcp", "10", &format!("{raw}")],
            );
        }
    }
}

pub fn start() -> Result<Rc<Service>> {
    let displays: Shared = Arc::new(Mutex::new(Vec::new()));
    let ddc = Arc::new(DdcWriter::default());
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("brightness", PROPS);
        let (d, w) = (Arc::clone(&displays), std::rc::Weak::clone(weak));
        let ddc = Arc::clone(&ddc);
        svc.method("set", move |args| {
            let v = arg_f64(&args, 0, "brightness")?.clamp(0.0, 1.0);
            let only = arg_str(&args, 1, "display").ok();
            let mut list = d.lock().expect("brightness displays lock poisoned");
            let mut hit = false;
            let mut gamma = None;
            for disp in list
                .iter_mut()
                .filter(|x| only.as_ref().is_none_or(|o| *o == x.id))
            {
                gamma = set_display(disp, v, &ddc)?.or(gamma);
                hit = true;
            }
            if !hit {
                bail!("no such display");
            }
            if let Some(g) = gamma {
                // Gamma is global: every DDC display sees the new value. If
                // hyprsunset is gone, they fall back to hardware only.
                let sent = crate::hyprsunset::request(&format!("gamma {}", (g * 100.0).round()));
                let g = sent.is_ok().then_some(g);
                for disp in list.iter_mut().filter(|x| x.gamma.is_some()) {
                    disp.gamma = g;
                }
            }
            if let Some(svc) = w.upgrade() {
                for (p, v) in props(&list) {
                    svc.set(p, v);
                }
            }
            Ok(Value::Null)
        });
        svc
    });
    let pusher = svc.pusher();
    let writer = Arc::clone(&ddc);
    std::thread::Builder::new()
        .name("hw-ddc-write".into())
        .spawn(move || writer.run())?;
    std::thread::Builder::new()
        .name("hw-brightness".into())
        .spawn(move || worker(&displays, &ddc, &pusher))?;
    Ok(svc)
}

fn props(list: &[Display]) -> Vec<(&'static str, Value)> {
    vec![
        ("available", json!(!list.is_empty())),
        (
            "brightness",
            list.first()
                .map_or(Value::Null, |d| d.json()["brightness"].clone()),
        ),
        (
            "displays",
            Value::Array(list.iter().map(Display::json).collect()),
        ),
    ]
}

fn worker(shared: &Shared, ddc: &DdcWriter, pusher: &Pusher) {
    let has_ddc = which("ddcutil");
    let mut tick = 0u32;
    loop {
        // Skipped while a set is still being written: the read would undo it.
        let fresh_ddc = (has_ddc && tick.is_multiple_of(DDC_EVERY) && !ddc.busy()).then(read_ddc);
        tick = tick.wrapping_add(1);
        let mut list = read_backlights(Path::new(BACKLIGHT));
        {
            let mut s = shared.lock().expect("brightness displays lock poisoned");
            // Between DDC reads, keep the last known (or last set) values.
            let ddc = fresh_ddc
                .unwrap_or_else(|| s.iter().filter(|d| d.kind == "ddc").cloned().collect());
            list.extend(ddc);
            s.clone_from(&list);
        }
        for (p, v) in props(&list) {
            if !pusher.set(p, v) {
                return;
            }
        }
        std::thread::sleep(BACKLIGHT_EVERY);
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

fn read_backlights(root: &Path) -> Vec<Display> {
    let mut out: Vec<Display> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let dir: PathBuf = e.path();
            let num = |f: &str| -> Option<f64> {
                std::fs::read_to_string(dir.join(f))
                    .ok()?
                    .trim()
                    .parse()
                    .ok()
            };
            let name = e.file_name().to_string_lossy().into_owned();
            Some(Display {
                id: format!("backlight:{name}"),
                kind: "backlight",
                value: num("actual_brightness").or_else(|| num("brightness"))?,
                max: num("max_brightness")?,
                name,
                gamma: None,
            })
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// `ddcutil detect --brief` + `getvcp 10 --brief` per display, and
/// hyprsunset's gamma (percent) for the software dimming.
fn read_ddc() -> Vec<Display> {
    let gamma = crate::hyprsunset::request("gamma")
        .ok()
        .and_then(|g| g.parse::<f64>().ok())
        .map(|g| g / 100.0);
    let _bus = DDC_BUS.lock().expect("ddc bus lock poisoned");
    let Some(detect) = run("ddcutil", &["detect", "--brief"]) else {
        return Vec::new();
    };
    parse_detect(&detect)
        .into_iter()
        .filter_map(|(n, conn)| {
            let out = run("ddcutil", &["--display", &n, "getvcp", "10", "--brief"])?;
            let (value, max) = parse_vcp(&out)?;
            Some(Display {
                id: format!("ddc:{n}"),
                kind: "ddc",
                name: conn.unwrap_or_else(|| format!("display {n}")),
                value,
                max,
                gamma,
            })
        })
        .collect()
}

fn run(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `(display number, DRM connector)` from `ddcutil detect --brief`.
fn parse_detect(s: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    for line in s.lines() {
        let t = line.trim();
        if let Some(n) = t.strip_prefix("Display ") {
            out.push((n.trim().to_owned(), None));
        } else if let (Some(conn), Some(last)) = (t.strip_prefix("DRM connector:"), out.last_mut())
        {
            // `card1-DP-2` → `DP-2`
            let c = conn.trim();
            let c = c.split_once('-').map_or(
                c,
                |(card, rest)| {
                    if card.starts_with("card") { rest } else { c }
                },
            );
            last.1 = Some(c.to_owned());
        }
    }
    out
}

/// `VCP 10 C 50 100` → (50, 100).
fn parse_vcp(s: &str) -> Option<(f64, f64)> {
    let f: Vec<&str> = s.split_whitespace().collect();
    if f.len() >= 5 && f[0] == "VCP" && f[2] == "C" {
        return Some((f[3].parse().ok()?, f[4].parse().ok()?));
    }
    None
}

/// Returns the hyprsunset gamma to set, if this display needs one.
fn set_display(d: &mut Display, v: f64, ddc: &DdcWriter) -> Result<Option<f64>> {
    let (raw, gamma) = split_level(d, v);
    if d.kind == "backlight" {
        let dev = d.id.trim_start_matches("backlight:").to_owned();
        spawn_detached("brightnessctl", &["-d", &dev, "set", &format!("{raw}")])?;
    } else {
        ddc.queue(d.id.trim_start_matches("ddc:"), raw);
    }
    d.value = raw;
    Ok(gamma)
}

/// Level `v` (0..1) → (raw hardware value, hyprsunset gamma to set).
fn split_level(d: &Display, v: f64) -> (f64, Option<f64>) {
    let Some(g) = d.gamma else {
        return ((v * d.max).round(), None);
    };
    if v < SOFT_RANGE {
        let gamma = SOFT_FLOOR + (1.0 - SOFT_FLOOR) * v / SOFT_RANGE;
        return (0.0, Some((gamma * 100.0).round() / 100.0));
    }
    let raw = ((v - SOFT_RANGE) / (1.0 - SOFT_RANGE) * d.max).round();
    // Leaving the dimmed range restores full gamma; above it, leave
    // whatever gamma hyprsunset has (its profiles may set one).
    let undim = d.value <= 0.0 && g < 1.0;
    (raw, undim.then_some(1.0))
}

fn spawn_detached(bin: &str, args: &[&str]) -> Result<()> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("running {bin}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddcutil_output() {
        let detect = "Display 1\n   I2C bus:  /dev/i2c-5\n   DRM connector:    card1-DP-2\n   Monitor:   GSM:LG ULTRAGEAR\n\nInvalid display\n   I2C bus: /dev/i2c-7\n";
        assert_eq!(
            parse_detect(detect),
            [("1".to_owned(), Some("DP-2".to_owned()))]
        );
        assert_eq!(parse_vcp("VCP 10 C 50 100\n"), Some((50.0, 100.0)));
        assert_eq!(parse_vcp("VCP 10 ERR"), None);
    }

    #[test]
    fn soft_dimming_below_the_hardware_minimum() {
        let mut d = Display {
            id: "ddc:1".into(),
            kind: "ddc",
            name: "DP-2".into(),
            value: 50.0,
            max: 100.0,
            gamma: Some(1.0),
        };
        assert_eq!(d.json()["brightness"], json!(0.6));
        // Into the dimmed range: DDC 0, gamma down to the floor at 0.
        assert_eq!(split_level(&d, 0.0), (0.0, Some(SOFT_FLOOR)));
        assert_eq!(split_level(&d, 0.04), (0.0, Some(0.2)));
        (d.value, d.gamma) = (0.0, Some(0.2));
        assert_eq!(d.json()["brightness"], json!(0.04));
        // Back out of it restores gamma once; above it gamma is left alone.
        assert_eq!(split_level(&d, 1.0), (100.0, Some(1.0)));
        (d.value, d.gamma) = (100.0, Some(0.93));
        assert_eq!(split_level(&d, 0.6), (50.0, None));
        // Without hyprsunset the range is plain hardware.
        d.gamma = None;
        assert_eq!(split_level(&d, 0.1), (10.0, None));
    }

    #[test]
    fn ddc_writes_keep_the_latest_value() {
        let ddc = DdcWriter::default();
        assert!(!ddc.busy());
        for raw in [10.0, 40.0, 75.0] {
            ddc.queue("1", raw);
        }
        ddc.queue("2", 5.0);
        let q = ddc.queue.lock().unwrap();
        assert_eq!(
            q.pending,
            BTreeMap::from([("1".to_owned(), 75.0), ("2".to_owned(), 5.0)])
        );
        drop(q);
        assert!(ddc.busy());
    }

    #[test]
    fn backlight_tree() {
        let root = std::env::temp_dir().join(format!("hw-bl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let d = root.join("intel_backlight");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("actual_brightness"), "300\n").unwrap();
        std::fs::write(d.join("max_brightness"), "1200\n").unwrap();
        let list = read_backlights(&root);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].json()["brightness"], json!(0.25));
        let p: std::collections::HashMap<_, _> = props(&list).into_iter().collect();
        assert_eq!(p["brightness"], json!(0.25));
        assert_eq!(props(&[])[1].1, Value::Null);
        let _ = std::fs::remove_dir_all(&root);
    }
}
