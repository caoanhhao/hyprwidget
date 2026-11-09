//! `hw.service.battery`: batteries and AC from `/sys/class/power_supply`,
//! polled every 10 s. (UPower over D-Bus is the `upower` service; this one
//! needs no daemon.)

use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::service::{Pusher, Service};

pub const PROPS: &[&str] = &[
    "available",
    "percentage",
    "state",
    "charging",
    "ac_online",
    "time_to_empty",
    "time_to_full",
    "batteries",
];

const ROOT: &str = "/sys/class/power_supply";
const INTERVAL: Duration = Duration::from_secs(10);

pub fn start() -> Result<Rc<Service>> {
    let svc = Rc::new(Service::new("battery", PROPS));
    for (p, v) in sample(Path::new(ROOT)) {
        svc.set(p, v);
    }
    let pusher = svc.pusher();
    std::thread::Builder::new()
        .name("hw-battery".into())
        .spawn(move || worker(pusher))?;
    Ok(svc)
}

fn worker(pusher: Pusher) {
    loop {
        std::thread::sleep(INTERVAL);
        for (p, v) in sample(Path::new(ROOT)) {
            if !pusher.set(p, v) {
                return;
            }
        }
    }
}

fn read(dir: &Path, f: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(f))
        .ok()
        .map(|s| s.trim().to_owned())
}

fn num(dir: &Path, f: &str) -> Option<f64> {
    read(dir, f)?.parse().ok()
}

fn sample(root: &Path) -> Vec<(&'static str, Value)> {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    entries.sort();
    let mut bats = Vec::new();
    let mut ac_online = None;
    for dir in &entries {
        match read(dir, "type").as_deref() {
            Some("Mains") => {
                ac_online = Some(ac_online.unwrap_or(false) || num(dir, "online") == Some(1.0));
            }
            Some("Battery") if read(dir, "scope").as_deref() != Some("Device") => {
                // energy_* (µWh) or charge_* (µAh); power_now/current_now.
                let (now, full, rate) = match num(dir, "energy_now") {
                    Some(e) => (Some(e), num(dir, "energy_full"), num(dir, "power_now")),
                    None => (
                        num(dir, "charge_now"),
                        num(dir, "charge_full"),
                        num(dir, "current_now"),
                    ),
                };
                let pct = num(dir, "capacity").or_else(|| Some(100.0 * now? / full?));
                let status = read(dir, "status").unwrap_or_else(|| "Unknown".into());
                bats.push(json!({
                    "name": dir.file_name().map(|n| n.to_string_lossy().into_owned()),
                    "percentage": pct,
                    "state": status.to_lowercase(),
                    "now": now,
                    "full": full,
                    "rate": rate,
                    "model": read(dir, "model_name"),
                }));
            }
            _ => {}
        }
    }
    summarize(&bats, ac_online)
}

/// Combined props for all system batteries.
fn summarize(bats: &[Value], ac_online: Option<bool>) -> Vec<(&'static str, Value)> {
    let sum = |k: &str| bats.iter().filter_map(|b| b[k].as_f64()).sum::<f64>();
    let (now, full, rate) = (sum("now"), sum("full"), sum("rate"));
    let pct = if full > 0.0 {
        Some((1000.0 * now / full).round() / 10.0)
    } else {
        bats.first().and_then(|b| b["percentage"].as_f64())
    };
    let states: Vec<&str> = bats.iter().filter_map(|b| b["state"].as_str()).collect();
    let state = if states.contains(&"charging") {
        "charging"
    } else if states.contains(&"discharging") {
        "discharging"
    } else if !states.is_empty() && states.iter().all(|s| *s == "full") {
        "full"
    } else {
        states.first().copied().unwrap_or("unknown")
    };
    let hours = |energy: f64| (rate > 0.0).then(|| (energy / rate * 3600.0).round());
    vec![
        ("available", json!(!bats.is_empty())),
        ("percentage", json!(pct)),
        ("state", json!(state)),
        ("charging", json!(state == "charging")),
        ("ac_online", json!(ac_online)),
        (
            "time_to_empty",
            json!(if state == "discharging" {
                hours(now)
            } else {
                None
            }),
        ),
        (
            "time_to_full",
            json!(if state == "charging" {
                hours(full - now)
            } else {
                None
            }),
        ),
        ("batteries", Value::Array(bats.to_vec())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sysfs_tree() {
        let root = std::env::temp_dir().join(format!("hw-bat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let w = |dir: &str, f: &str, v: &str| {
            let d = root.join(dir);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join(f), v).unwrap();
        };
        w("AC", "type", "Mains\n");
        w("AC", "online", "0\n");
        w("BAT0", "type", "Battery\n");
        w("BAT0", "status", "Discharging\n");
        w("BAT0", "energy_now", "25000000\n");
        w("BAT0", "energy_full", "50000000\n");
        w("BAT0", "power_now", "10000000\n");
        w("BAT0", "capacity", "50\n");
        w("hidpp_battery_0", "type", "Battery\n");
        w("hidpp_battery_0", "scope", "Device\n");
        let s: std::collections::HashMap<_, _> = sample(&root).into_iter().collect();
        assert_eq!(s["available"], json!(true));
        assert_eq!(s["percentage"], json!(50.0));
        assert_eq!(s["state"], json!("discharging"));
        assert_eq!(s["ac_online"], json!(false));
        // 25 Wh at 10 W → 2.5 h
        assert_eq!(s["time_to_empty"], json!(9000.0));
        assert_eq!(s["time_to_full"], Value::Null);
        assert_eq!(s["batteries"].as_array().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_battery() {
        let s: std::collections::HashMap<_, _> = summarize(&[], Some(true)).into_iter().collect();
        assert_eq!(s["available"], json!(false));
        assert_eq!(s["percentage"], Value::Null);
        assert_eq!(s["state"], json!("unknown"));
    }
}
