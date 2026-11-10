//! `hw.service.hyprsunset`: blue-light filter through hyprsunset's IPC
//! socket (`$XDG_RUNTIME_DIR/hypr/$SIG/.hyprsunset.sock`, what
//! `hyprctl hyprsunset …` talks to). Polled every 10 s because hyprsunset's
//! own profiles change the values over the day.
//!
//! ```lua
//! local s = hw.service.hyprsunset
//! s.temperature()          -- Kelvin, nil if hyprsunset isn't running
//! s:set_temperature(4500)  -- also leaves identity mode
//! s:set_gamma(90)          -- percent
//! s:set_identity()         -- no filter (s.identity() tracks it)
//! ```

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use hw_core::hyprland;
use serde_json::{Value, json};

use crate::service::{Pusher, Service, arg_f64};

pub const PROPS: &[&str] = &["available", "temperature", "gamma", "identity"];
const POLL: Duration = Duration::from_secs(10);

pub fn start() -> Result<Rc<Service>> {
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("hyprsunset", PROPS);
        let w = std::rc::Weak::clone(weak);
        svc.method("set_temperature", move |args| {
            let k = arg_f64(&args, 0, "temperature")?
                .clamp(1000.0, 20000.0)
                .round();
            request(&format!("temperature {k}"))?;
            set_prop(&w, "identity", json!(false));
            refresh(&w);
            Ok(Value::Null)
        });
        let w = std::rc::Weak::clone(weak);
        svc.method("set_gamma", move |args| {
            let g = arg_f64(&args, 0, "gamma")?.clamp(0.0, 200.0).round();
            request(&format!("gamma {g}"))?;
            refresh(&w);
            Ok(Value::Null)
        });
        let w = std::rc::Weak::clone(weak);
        svc.method("set_identity", move |_| {
            request("identity")?;
            set_prop(&w, "identity", json!(true));
            refresh(&w);
            Ok(Value::Null)
        });
        let w = std::rc::Weak::clone(weak);
        svc.method("refresh", move |_| {
            refresh(&w);
            Ok(Value::Null)
        });
        svc
    });
    for (p, v) in read() {
        svc.set(p, v);
    }
    // hyprsunset can't be asked whether identity mode is on; start from the
    // active profile and track our own changes.
    let identity = request("profile").ok().and_then(|p| parse_identity(&p));
    svc.set("identity", json!(identity));
    let pusher = svc.pusher();
    std::thread::Builder::new()
        .name("hw-hyprsunset".into())
        .spawn(move || worker(&pusher))?;
    Ok(svc)
}

fn set_prop(w: &std::rc::Weak<Service>, prop: &str, v: Value) {
    if let Some(svc) = w.upgrade() {
        svc.set(prop, v);
    }
}

fn refresh(w: &std::rc::Weak<Service>) {
    if let Some(svc) = w.upgrade() {
        for (p, v) in read() {
            svc.set(p, v);
        }
    }
}

fn worker(pusher: &Pusher) {
    loop {
        std::thread::sleep(POLL);
        for (p, v) in read() {
            if !pusher.set(p, v) {
                return;
            }
        }
    }
}

fn socket() -> Result<PathBuf> {
    Ok(hyprland::socket_dir()
        .context("not running under Hyprland")?
        .join(".hyprsunset.sock"))
}

/// One request (hyprsunset answers and closes).
pub(crate) fn request(cmd: &str) -> Result<String> {
    let path = socket()?;
    let mut s = UnixStream::connect(&path).context("hyprsunset is not running")?;
    s.set_read_timeout(Some(Duration::from_secs(1)))?;
    s.write_all(cmd.as_bytes())?;
    s.shutdown(std::net::Shutdown::Write)?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    let out = out.trim().to_owned();
    if out.starts_with("err") || out.starts_with("invalid") {
        bail!("hyprsunset: {out}");
    }
    Ok(out)
}

fn read() -> Vec<(&'static str, Value)> {
    let num = |cmd| request(cmd).ok().and_then(|r| r.parse::<f64>().ok());
    match (num("temperature"), num("gamma")) {
        (Some(t), Some(g)) => vec![
            ("available", json!(true)),
            ("temperature", json!(t)),
            ("gamma", json!(g)),
        ],
        _ => vec![
            ("available", json!(false)),
            ("temperature", Value::Null),
            ("gamma", Value::Null),
        ],
    }
}

fn parse_identity(profile: &str) -> Option<bool> {
    profile
        .lines()
        .find_map(|l| l.strip_prefix("Identity:"))
        .map(|v| v.trim() == "true")
}

#[cfg(test)]
mod tests {
    #[test]
    fn identity_from_profile() {
        let p = "Time: 07:30\nTemperature: 6000\nGamma: 1\nIdentity: true";
        assert_eq!(super::parse_identity(p), Some(true));
        assert_eq!(super::parse_identity("Identity: false"), Some(false));
        assert_eq!(super::parse_identity("x"), None);
    }
}
