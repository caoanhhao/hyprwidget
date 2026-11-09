//! `hw.service.hyprland`: Hyprland state over its own IPC sockets
//! (`.socket.sock` for `j/` requests, `.socket2.sock` for events).
//!
//! A worker thread reads events, coalesces bursts (refetching only the parts
//! an event invalidates, once the socket has been quiet for a few ms) and
//! pushes the JSON replies to the main thread.

pub mod events;

use std::io::{BufRead, BufReader, ErrorKind};
use std::os::unix::net::UnixStream;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hw_core::hyprland;
use serde_json::{Map, Value, json};

use crate::service::{Pusher, Service, arg_str, join_args};
use events::dirty;

/// Props, in the order [`fetch`] fills them.
pub const PROPS: &[&str] = &[
    "available",
    "workspaces",
    "monitors",
    "clients",
    "focused_client",
    "focused_workspace",
    "focused_monitor",
    "fullscreen",
    "special",
    "event",
];

/// Quiet time after an event before refetching.
const COALESCE: Duration = Duration::from_millis(12);

pub fn start() -> Result<Rc<Service>> {
    let available = hyprland::available();
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("hyprland", PROPS);
        svc.set("available", json!(available));
        svc.method("dispatch", |args| {
            // 0.57 (Lua config): `dispatch hl.dsp.focus({ workspace = 3 })`;
            // older: `dispatch workspace 3`. Either way the args are joined.
            checked(&hyprland::request(&format!(
                "dispatch {}",
                join_args(&args)
            ))?)
        });
        svc.method("keyword", |args| {
            let name = arg_str(&args, 0, "option")?;
            let value = join_args(args.get(1..).unwrap_or_default());
            checked(&hyprland::request(&format!("keyword {name} {value}"))?)
        });
        svc.method("message", |args| {
            let cmd = join_args(&args);
            let reply = hyprland::request(&cmd)?;
            Ok(if cmd.starts_with("j/") {
                serde_json::from_str(&reply).unwrap_or(Value::String(reply))
            } else {
                Value::String(reply)
            })
        });
        let weak = std::rc::Weak::clone(weak);
        svc.method("refresh", move |_| {
            let mut st = State::default();
            st.fetch(dirty::ALL)?;
            if let Some(svc) = weak.upgrade() {
                st.apply_now(&svc);
            }
            Ok(Value::Null)
        });
        svc
    });
    if !available {
        tracing::warn!("hw.service.hyprland: not running under Hyprland");
        return Ok(svc);
    }

    let mut state = State::default();
    match state.fetch(dirty::ALL) {
        Ok(()) => state.apply_now(&svc),
        Err(e) => tracing::warn!("hw.service.hyprland: initial fetch: {e:#}"),
    }

    let pusher = svc.pusher();
    let path = hyprland::socket_dir()
        .context("not running under Hyprland")?
        .join(".socket2.sock");
    std::thread::Builder::new()
        .name("hw-hyprland".into())
        .spawn(move || worker(&path, state, &pusher))?;
    Ok(svc)
}

/// Turns Hyprland's `error …` replies into errors.
fn checked(reply: &str) -> Result<Value> {
    let t = reply.trim();
    if t.starts_with("error") || t.starts_with("Invalid") || t.starts_with("unknown") {
        bail!("{t}");
    }
    Ok(Value::String(t.to_owned()))
}

/// Last fetched replies, kept by the worker to compute derived props.
#[derive(Default)]
struct State {
    workspaces: Value,
    monitors: Value,
    clients: Value,
    active_window: Value,
    active_workspace: Value,
}

impl State {
    fn fetch(&mut self, what: u8) -> Result<()> {
        let parts: [(u8, &str); 5] = [
            (dirty::WORKSPACES, "j/workspaces"),
            (dirty::MONITORS, "j/monitors"),
            (dirty::CLIENTS, "j/clients"),
            (dirty::ACTIVE_WINDOW, "j/activewindow"),
            (dirty::ACTIVE_WORKSPACE, "j/activeworkspace"),
        ];
        let wanted: Vec<_> = parts.iter().filter(|(f, _)| what & f != 0).collect();
        let cmds: Vec<String> = wanted.iter().map(|(_, c)| c.to_string()).collect();
        let replies = hyprland::batch(&cmds)?;
        for ((flag, cmd), reply) in wanted.iter().zip(replies) {
            let v: Value = serde_json::from_str(reply.trim())
                .with_context(|| format!("parsing {cmd} reply"))?;
            match *flag {
                dirty::WORKSPACES => self.workspaces = sort_workspaces(v),
                dirty::MONITORS => self.monitors = v,
                dirty::CLIENTS => self.clients = v,
                dirty::ACTIVE_WINDOW => self.active_window = non_empty(v),
                _ => self.active_workspace = non_empty(v),
            }
        }
        Ok(())
    }

    /// Values for each prop affected by `what`.
    fn values(&self, what: u8) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        if what & dirty::WORKSPACES != 0 {
            out.push(("workspaces", self.workspaces.clone()));
        }
        if what & dirty::MONITORS != 0 {
            out.push(("monitors", self.monitors.clone()));
            out.push(("focused_monitor", focused_monitor(&self.monitors)));
            out.push(("special", special(&self.monitors)));
        }
        if what & dirty::CLIENTS != 0 {
            out.push(("clients", self.clients.clone()));
        }
        if what & dirty::ACTIVE_WINDOW != 0 {
            out.push(("focused_client", self.active_window.clone()));
        }
        if what & dirty::ACTIVE_WORKSPACE != 0 {
            out.push(("focused_workspace", self.active_workspace.clone()));
        }
        if what & (dirty::MONITORS | dirty::CLIENTS | dirty::WORKSPACES) != 0 {
            out.push(("fullscreen", fullscreen(&self.monitors, &self.clients)));
        }
        out
    }

    fn apply_now(&self, svc: &Service) {
        for (p, v) in self.values(dirty::ALL) {
            svc.set(p, v);
        }
    }

    fn push(&self, pusher: &Pusher, what: u8) -> bool {
        self.values(what).into_iter().all(|(p, v)| pusher.set(p, v))
    }
}

/// `{}` (no focused window/workspace) → `null`.
fn non_empty(v: Value) -> Value {
    match &v {
        Value::Object(m) if m.is_empty() || m.get("address") == Some(&json!("")) => Value::Null,
        _ => v,
    }
}

fn sort_workspaces(mut v: Value) -> Value {
    if let Value::Array(a) = &mut v {
        a.sort_by_key(|w| w["id"].as_i64().unwrap_or(i64::MIN));
    }
    v
}

fn focused_monitor(monitors: &Value) -> Value {
    monitors
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["focused"] == json!(true))
        .map_or(Value::Null, |m| m["name"].clone())
}

/// `{ [monitor] = "special:name" }` for monitors showing a special
/// workspace.
fn special(monitors: &Value) -> Value {
    let mut out = Map::new();
    for m in monitors.as_array().into_iter().flatten() {
        let name = m["specialWorkspace"]["name"].as_str().unwrap_or("");
        if let (Some(mon), false) = (m["name"].as_str(), name.is_empty()) {
            out.insert(mon.to_owned(), json!(name));
        }
    }
    Value::Object(out)
}

/// `{ [monitor] = mode }`: highest client fullscreen mode (0 none,
/// 1 maximized, 2 fullscreen) on the monitor's visible workspace(s).
fn fullscreen(monitors: &Value, clients: &Value) -> Value {
    let mut out = Map::new();
    for m in monitors.as_array().into_iter().flatten() {
        let Some(mon) = m["name"].as_str() else {
            continue;
        };
        let visible = [
            &m["activeWorkspace"]["name"],
            &m["specialWorkspace"]["name"],
        ];
        let mode = clients
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| {
                let ws = &c["workspace"]["name"];
                visible
                    .iter()
                    .any(|v| v.as_str().is_some_and(|s| !s.is_empty()) && *v == ws)
            })
            .filter_map(|c| c["fullscreen"].as_i64())
            .max()
            .unwrap_or(0);
        out.insert(mon.to_owned(), json!(mode));
    }
    Value::Object(out)
}

fn worker(path: &std::path::Path, mut state: State, pusher: &Pusher) {
    let mut first = true;
    loop {
        if pusher.is_closed() {
            return;
        }
        let Ok(stream) = UnixStream::connect(path) else {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        if !first {
            // Hyprland restarted (or we lost events): resync everything.
            if state.fetch(dirty::ALL).is_ok() && !state.push(pusher, dirty::ALL) {
                return;
            }
        }
        first = false;
        if !read_events(stream, &mut state, pusher) {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

/// Reads events until the socket closes. Returns `false` when the service is
/// gone.
fn read_events(stream: UnixStream, state: &mut State, pusher: &Pusher) -> bool {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    let mut pending: u8 = 0;
    loop {
        // Block while idle; poll briefly while a refetch is pending.
        let timeout = (pending != 0).then_some(COALESCE);
        if reader.get_ref().set_read_timeout(timeout).is_err() {
            return true;
        }
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => return true,
            Ok(_) if buf.ends_with(b"\n") => {
                let line = String::from_utf8_lossy(&buf).into_owned();
                buf.clear();
                let ev = events::parse(&line);
                pending |= events::affects(ev.name);
                let raw = json!({ "name": ev.name, "data": ev.data, "args": ev.args });
                if !pusher.emit("event", raw) {
                    return false;
                }
            }
            Ok(_) => {}
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                match state.fetch(pending) {
                    Ok(()) => {
                        if !state.push(pusher, pending) {
                            return false;
                        }
                    }
                    Err(e) => tracing::warn!("hw.service.hyprland: refetch: {e:#}"),
                }
                pending = 0;
            }
            Err(_) => return true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors() -> Value {
        json!([
            { "name": "DP-2", "focused": true,
              "activeWorkspace": { "id": 1, "name": "1" },
              "specialWorkspace": { "name": "" } },
            { "name": "HDMI-A-1", "focused": false,
              "activeWorkspace": { "id": 4, "name": "4" },
              "specialWorkspace": { "name": "special:special" } },
        ])
    }

    #[test]
    fn derived_props() {
        let clients = json!([
            { "workspace": { "id": 1, "name": "1" }, "fullscreen": 1 },
            { "workspace": { "id": 2, "name": "2" }, "fullscreen": 2 },
            { "workspace": { "name": "special:special" }, "fullscreen": 2 },
        ]);
        assert_eq!(focused_monitor(&monitors()), json!("DP-2"));
        assert_eq!(
            special(&monitors()),
            json!({ "HDMI-A-1": "special:special" })
        );
        assert_eq!(
            fullscreen(&monitors(), &clients),
            json!({ "DP-2": 1, "HDMI-A-1": 2 })
        );
    }

    #[test]
    fn empty_replies_become_null() {
        assert_eq!(non_empty(json!({})), Value::Null);
        assert_eq!(non_empty(json!({ "address": "" })), Value::Null);
        assert_eq!(
            non_empty(json!({ "address": "0x1" })),
            json!({ "address": "0x1" })
        );
    }

    #[test]
    fn workspaces_sorted_by_id() {
        let v = sort_workspaces(json!([{ "id": 3 }, { "id": -98 }, { "id": 1 }, { "name": "s" }]));
        let ids: Vec<_> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["id"].clone())
            .collect();
        assert_eq!(ids, [Value::Null, json!(-98), json!(1), json!(3)]);
    }

    #[test]
    fn checked_replies() {
        assert!(checked("ok").is_ok());
        assert!(checked("error: 3 bad").is_err());
    }
}
