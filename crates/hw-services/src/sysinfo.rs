//! `hw.service.sysinfo`: CPU, memory, GPU, temperatures, disks, network
//! bandwidth, uptime and load, polled on a worker thread. Replaces AGS'
//! `system-resources-loop-ags.c` and `bandwidth-loop-ags.c` (same fields,
//! sizes in bytes instead of GB).
//!
//! The interval (default 2000 ms) is set with `:set_interval(ms)`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{Value, json};

use crate::service::{Pusher, Service, arg_f64, cache_dir};

pub const PROPS: &[&str] = &[
    "cpu",
    "memory",
    "gpu",
    "temperatures",
    "disks",
    "network",
    "uptime",
    "load",
    "interval",
];

const DEFAULT_INTERVAL_MS: u64 = 2000;
/// How often today's bandwidth totals are written to disk.
const SAVE_EVERY: Duration = Duration::from_secs(30);

pub fn start() -> Result<Rc<Service>> {
    let interval = Arc::new(AtomicU64::new(DEFAULT_INTERVAL_MS));
    let mut svc = Service::new("sysinfo", PROPS);
    svc.set("interval", json!(DEFAULT_INTERVAL_MS));
    let iv = interval.clone();
    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let weak = weak.clone();
        svc.method("set_interval", move |args| {
            let ms = arg_f64(&args, 0, "milliseconds")?.max(250.0) as u64;
            iv.store(ms, Ordering::Relaxed);
            if let Some(s) = weak.upgrade() {
                s.set("interval", json!(ms));
            }
            Ok(Value::Null)
        });
        svc
    });
    let pusher = svc.pusher();
    std::thread::Builder::new()
        .name("hw-sysinfo".into())
        .spawn(move || worker(interval, pusher))?;
    Ok(svc)
}

fn worker(interval: Arc<AtomicU64>, pusher: Pusher) {
    let mut s = Sampler::new();
    // First CPU delta needs two samples.
    std::thread::sleep(Duration::from_millis(500));
    loop {
        for (prop, v) in s.sample() {
            if !pusher.set(prop, v) {
                s.net.save();
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(interval.load(Ordering::Relaxed)));
    }
}

struct Sampler {
    cpu_prev: Vec<CpuTimes>,
    gpu: Gpu,
    gpu_at: Option<Instant>,
    cpu_temp: Option<PathBuf>,
    components: Option<sysinfo::Components>,
    net: Net,
}

impl Sampler {
    fn new() -> Self {
        Self {
            cpu_prev: read("/proc/stat")
                .map(|s| parse_cpu_stat(&s))
                .unwrap_or_default(),
            gpu: Gpu::detect(),
            gpu_at: None,
            cpu_temp: find_cpu_temp(),
            components: sysinfo::Components::new_with_refreshed_list().ok(),
            net: Net::new(),
        }
    }

    fn sample(&mut self) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();

        let now = read("/proc/stat")
            .map(|s| parse_cpu_stat(&s))
            .unwrap_or_default();
        let usage: Vec<f64> = now
            .iter()
            .zip(&self.cpu_prev)
            .map(|(n, o)| n.usage_since(o))
            .collect();
        self.cpu_prev = now;
        let temp = self
            .cpu_temp
            .as_deref()
            .and_then(read)
            .and_then(|s| s.trim().parse::<f64>().ok())
            .map(|m| round1(m / 1000.0));
        out.push((
            "cpu",
            json!({
                "usage": usage.first().map(|u| round1(*u)).unwrap_or(0.0),
                "cores": usage.iter().skip(1).map(|u| round1(*u)).collect::<Vec<_>>(),
                "clock_ghz": read("/proc/cpuinfo").map(|s| cpu_clock_ghz(&s)).unwrap_or(0.0),
                "threads": usage.len().saturating_sub(1),
                "temp": temp,
            }),
        ));

        if let Some(m) = read("/proc/meminfo").map(|s| parse_meminfo(&s)) {
            out.push(("memory", memory_json(&m)));
        }

        let slow = matches!(self.gpu, Gpu::Nvidia(_));
        if !slow || self.gpu_at.is_none_or(|t| t.elapsed() >= NVIDIA_EVERY) {
            self.gpu_at = Some(Instant::now());
            out.push(("gpu", self.gpu.sample()));
        }

        if let Some(c) = &mut self.components {
            c.refresh(false);
            let temps: Vec<Value> = c
                .iter()
                .filter_map(|c| {
                    Some(json!({
                        "label": c.label(),
                        "temp": round1(c.temperature()? as f64),
                        "critical": c.critical().map(|t| round1(t as f64)),
                    }))
                })
                .collect();
            out.push(("temperatures", Value::Array(temps)));
        }

        out.push(("disks", disks()));
        out.push(("network", self.net.sample()));

        if let Some(up) =
            read("/proc/uptime").and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        {
            out.push(("uptime", json!(up.floor())));
        }
        if let Some(l) = read("/proc/loadavg") {
            let l: Vec<f64> = l
                .split_whitespace()
                .take(3)
                .filter_map(|v| v.parse().ok())
                .collect();
            out.push(("load", json!(l)));
        }
        out
    }
}

fn read(p: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

// ---------------------------------------------------------------- CPU

#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct CpuTimes {
    total: u64,
    idle: u64,
}

impl CpuTimes {
    fn usage_since(&self, old: &CpuTimes) -> f64 {
        let dt = self.total.saturating_sub(old.total);
        let di = self.idle.saturating_sub(old.idle);
        if dt == 0 {
            return 0.0;
        }
        100.0 * (1.0 - di as f64 / dt as f64)
    }
}

/// `cpu` (aggregate) first, then `cpuN`. idle = idle + iowait, total =
/// user..steal (same as the C helper).
fn parse_cpu_stat(s: &str) -> Vec<CpuTimes> {
    s.lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|l| {
            let f: Vec<u64> = l
                .split_whitespace()
                .skip(1)
                .take(8)
                .filter_map(|v| v.parse().ok())
                .collect();
            let g = |i: usize| f.get(i).copied().unwrap_or(0);
            CpuTimes {
                total: f.iter().sum(),
                idle: g(3) + g(4),
            }
        })
        .collect()
}

/// Average `cpu MHz` over all cores, in GHz.
fn cpu_clock_ghz(cpuinfo: &str) -> f64 {
    let mhz: Vec<f64> = cpuinfo
        .lines()
        .filter(|l| l.starts_with("cpu MHz"))
        .filter_map(|l| l.split(':').nth(1)?.trim().parse().ok())
        .collect();
    if mhz.is_empty() {
        return 0.0;
    }
    (mhz.iter().sum::<f64>() / mhz.len() as f64 / 10.0).round() / 100.0
}

/// hwmon `k10temp`/`coretemp`/`cpu_thermal`, else a CPU thermal zone.
fn find_cpu_temp() -> Option<PathBuf> {
    let mut hwmons: Vec<_> = std::fs::read_dir("/sys/class/hwmon")
        .ok()?
        .flatten()
        .collect();
    hwmons.sort_by_key(|e| e.file_name());
    for e in hwmons {
        let name = read(e.path().join("name")).unwrap_or_default();
        if ["k10temp", "coretemp", "cpu_thermal", "zenpower"]
            .iter()
            .any(|n| name.contains(n))
        {
            let p = e.path().join("temp1_input");
            if p.exists() {
                return Some(p);
            }
        }
    }
    for zone in 0..32 {
        let base = PathBuf::from(format!("/sys/class/thermal/thermal_zone{zone}"));
        let ty = read(base.join("type")).unwrap_or_default();
        if ["x86_pkg_temp", "coretemp", "cpu_thermal"]
            .iter()
            .any(|n| ty.contains(n))
        {
            return Some(base.join("temp"));
        }
    }
    None
}

// ---------------------------------------------------------------- memory

/// `/proc/meminfo` values in bytes.
fn parse_meminfo(s: &str) -> HashMap<String, u64> {
    s.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let kb: u64 = v.split_whitespace().next()?.parse().ok()?;
            Some((k.to_owned(), kb * 1024))
        })
        .collect()
}

fn memory_json(m: &HashMap<String, u64>) -> Value {
    let g = |k: &str| m.get(k).copied().unwrap_or(0);
    let (total, available) = (g("MemTotal"), g("MemAvailable"));
    let (swap_total, swap_free) = (g("SwapTotal"), g("SwapFree"));
    let pct = |used: u64, total: u64| {
        if total == 0 {
            0.0
        } else {
            round1(100.0 * used as f64 / total as f64)
        }
    };
    json!({
        "total": total,
        "available": available,
        "used": total.saturating_sub(available),
        "usage": pct(total.saturating_sub(available), total),
        "swap_total": swap_total,
        "swap_used": swap_total.saturating_sub(swap_free),
        "swap_usage": pct(swap_total.saturating_sub(swap_free), swap_total),
    })
}

// ---------------------------------------------------------------- GPU

enum Gpu {
    /// `nvidia-smi` (like the AGS helper). NVML would avoid the spawn
    /// (~20 ms) but keeps ~30 MB of driver libraries resident.
    Nvidia(PathBuf),
    Sysfs(PathBuf),
    None,
}

/// Minimum time between `nvidia-smi` runs.
const NVIDIA_EVERY: Duration = Duration::from_secs(5);

impl Gpu {
    fn detect() -> Self {
        if let Some(smi) = ["/usr/bin/nvidia-smi", "/bin/nvidia-smi"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
        {
            return Gpu::Nvidia(smi);
        }
        for card in 0..16 {
            let dev = PathBuf::from(format!("/sys/class/drm/card{card}/device"));
            if dev.join("gpu_busy_percent").exists() {
                return Gpu::Sysfs(dev);
            }
        }
        Gpu::None
    }

    fn sample(&self) -> Value {
        match self {
            Gpu::Nvidia(smi) => std::process::Command::new(smi)
                .args([
                    "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
                    "--format=csv,noheader,nounits",
                ])
                .stderr(std::process::Stdio::null())
                .output()
                .ok()
                .and_then(|o| parse_nvidia_smi(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or(Value::Null),
            Gpu::Sysfs(dev) => {
                let num = |f: &str| read(dev.join(f)).and_then(|s| s.trim().parse::<f64>().ok());
                let temp = std::fs::read_dir(dev.join("hwmon"))
                    .ok()
                    .into_iter()
                    .flatten()
                    .flatten()
                    .find_map(|h| read(h.path().join("temp1_input")))
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .map(|m| round1(m / 1000.0));
                json!({
                    "label": "GPU",
                    "load": num("gpu_busy_percent"),
                    "memory_used": num("mem_info_vram_used"),
                    "memory_total": num("mem_info_vram_total"),
                    "temp": temp,
                })
            }
            Gpu::None => Value::Null,
        }
    }
}

/// First line of `nvidia-smi --query-gpu=name,utilization.gpu,memory.used,
/// memory.total,temperature.gpu --format=csv,noheader,nounits` (MiB → bytes).
fn parse_nvidia_smi(out: &str) -> Option<Value> {
    let f: Vec<&str> = out.lines().next()?.split(',').map(str::trim).collect();
    let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok());
    let mib = |i: usize| num(i).map(|m| (m * 1024.0 * 1024.0) as u64);
    Some(json!({
        "label": f.first()?,
        "load": num(1),
        "memory_used": mib(2),
        "memory_total": mib(3),
        "temp": num(4),
    }))
}

// ---------------------------------------------------------------- disks

/// Real filesystems, one entry per device (btrfs subvolumes share one).
fn disks() -> Value {
    let Ok(disks) = sysinfo::Disks::new_with_refreshed_list() else {
        return Value::Array(Vec::new());
    };
    let mut seen: HashMap<String, Value> = HashMap::new();
    for d in disks.iter() {
        let dev = d.name().to_string_lossy().into_owned();
        let mount = d.mount_point().to_string_lossy().into_owned();
        if !dev.starts_with("/dev/") {
            continue;
        }
        let keep = seen
            .get(&dev)
            .is_none_or(|v| mount.len() < v["mount"].as_str().unwrap_or("").len());
        if keep {
            let (total, avail) = (d.total_space(), d.available_space());
            seen.insert(
                dev.clone(),
                json!({
                    "device": dev,
                    "mount": mount,
                    "fs": d.file_system().to_string_lossy(),
                    "total": total,
                    "available": avail,
                    "used": total.saturating_sub(avail),
                    "usage": if total == 0 { 0.0 } else {
                        round1(100.0 * total.saturating_sub(avail) as f64 / total as f64)
                    },
                    "removable": d.is_removable(),
                }),
            );
        }
    }
    let mut v: Vec<Value> = seen.into_values().collect();
    v.sort_by(|a, b| a["mount"].as_str().cmp(&b["mount"].as_str()));
    Value::Array(v)
}

// ---------------------------------------------------------------- network

/// Interface of the default route (`/proc/net/route`).
fn default_iface(route: &str) -> Option<String> {
    route.lines().skip(1).find_map(|l| {
        let mut f = l.split_whitespace();
        let iface = f.next()?;
        (f.next()? == "00000000").then(|| iface.to_owned())
    })
}

/// `(rx, tx)` bytes of `iface` in `/proc/net/dev`.
fn iface_bytes(dev: &str, iface: &str) -> Option<(u64, u64)> {
    dev.lines().skip(2).find_map(|l| {
        let (name, rest) = l.split_once(':')?;
        if name.trim() != iface {
            return None;
        }
        let f: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        Some((*f.first()?, *f.get(8)?))
    })
}

/// Today's local date, `YYYY-MM-DD`.
fn today() -> String {
    glib::DateTime::now_local()
        .and_then(|d| d.format("%F"))
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Bandwidth speed + per-day totals, persisted in the cache dir as
/// `bandwidth.log` lines `YYYY-MM-DD tx rx` (format of the AGS helper).
struct Net {
    log: PathBuf,
    iface: Option<String>,
    prev: Option<(u64, u64, Instant)>,
    date: String,
    today: (u64, u64),
    last_save: Instant,
}

impl Net {
    fn new() -> Self {
        let log = cache_dir().join("bandwidth.log");
        let date = today();
        let today = read(&log)
            .and_then(|s| parse_log_day(&s, &date))
            .unwrap_or((0, 0));
        Self {
            log,
            iface: None,
            prev: None,
            date,
            today,
            last_save: Instant::now(),
        }
    }

    fn sample(&mut self) -> Value {
        let iface = read("/proc/net/route").and_then(|r| default_iface(&r));
        if iface != self.iface {
            self.iface = iface;
            self.prev = None;
        }
        let Some(iface) = self.iface.clone() else {
            return json!({ "interface": null });
        };
        let Some((rx, tx)) = read("/proc/net/dev").and_then(|d| iface_bytes(&d, &iface)) else {
            return json!({ "interface": iface });
        };
        let now = Instant::now();
        let date = today();
        if date != self.date {
            self.save();
            self.date = date;
            self.today = (0, 0);
        }
        let (mut rx_speed, mut tx_speed) = (0.0, 0.0);
        if let Some((prx, ptx, t)) = self.prev {
            let dt = now.duration_since(t).as_secs_f64().max(0.001);
            let (drx, dtx) = (rx.saturating_sub(prx), tx.saturating_sub(ptx));
            rx_speed = (drx as f64 / dt).round();
            tx_speed = (dtx as f64 / dt).round();
            self.today.0 += drx;
            self.today.1 += dtx;
        }
        self.prev = Some((rx, tx, now));
        if self.last_save.elapsed() >= SAVE_EVERY {
            self.save();
        }
        json!({
            "interface": iface,
            "rx_speed": rx_speed,
            "tx_speed": tx_speed,
            "rx_today": self.today.0,
            "tx_today": self.today.1,
            "rx_total": rx,
            "tx_total": tx,
        })
    }

    fn save(&mut self) {
        self.last_save = Instant::now();
        let old = read(&self.log).unwrap_or_default();
        let text = update_log(&old, &self.date, self.today.1, self.today.0);
        if let Some(dir) = self.log.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = self.log.with_extension("log.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &self.log);
        }
    }
}

/// `(rx, tx)` for `date` from the log (`date tx rx` lines).
fn parse_log_day(log: &str, date: &str) -> Option<(u64, u64)> {
    log.lines().find_map(|l| {
        let mut f = l.split_whitespace();
        if f.next()? != date {
            return None;
        }
        let tx = f.next()?.parse().ok()?;
        let rx = f.next()?.parse().ok()?;
        Some((rx, tx))
    })
}

/// Replaces (or appends) the line for `date`.
fn update_log(old: &str, date: &str, tx: u64, rx: u64) -> String {
    let mut out: Vec<String> = old
        .lines()
        .filter(|l| !l.trim().is_empty() && l.split_whitespace().next() != Some(date))
        .map(str::to_owned)
        .collect();
    out.push(format!("{date} {tx} {rx}"));
    out.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_usage_from_stat() {
        let a =
            parse_cpu_stat("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 50 0 50 350 50 0 0 0\nintr 1");
        let b = parse_cpu_stat("cpu  200 0 200 1300 200 0 0 0 0 0\ncpu0 100 0 100 650 100 0 0 0");
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[0],
            CpuTimes {
                total: 1000,
                idle: 800
            }
        );
        // Δtotal 900, Δidle 700 → 22.2 %
        assert_eq!(round1(b[0].usage_since(&a[0])), 22.2);
        assert_eq!(b[0].usage_since(&b[0]), 0.0);
    }

    #[test]
    fn meminfo() {
        let m =
            parse_meminfo("MemTotal:       1000 kB\nMemAvailable:    250 kB\nSwapTotal: 0 kB\n");
        let j = memory_json(&m);
        assert_eq!(j["total"], json!(1024000));
        assert_eq!(j["used"], json!(750 * 1024));
        assert_eq!(j["usage"], json!(75.0));
        assert_eq!(j["swap_usage"], json!(0.0));
    }

    #[test]
    fn clock() {
        assert_eq!(
            cpu_clock_ghz("cpu MHz\t\t: 3000.000\ncpu MHz\t\t: 4000.000\n"),
            3.5
        );
        assert_eq!(cpu_clock_ghz(""), 0.0);
    }

    #[test]
    fn network_parsing() {
        let route =
            "Iface\tDestination\tGateway\nwlan0\t0001A8C0\t00000000\nenp4s0\t00000000\t0101A8C0\n";
        assert_eq!(default_iface(route).as_deref(), Some("enp4s0"));
        let dev = "Inter-|   Receive\n face |bytes packets\n    lo: 5 1 0 0 0 0 0 0 5 1 0 0 0 0 0 0\nenp4s0: 1000 10 0 0 0 0 0 0 2000 20 0 0 0 0 0 0\n";
        assert_eq!(iface_bytes(dev, "enp4s0"), Some((1000, 2000)));
        assert_eq!(iface_bytes(dev, "wlan0"), None);
    }

    #[test]
    fn nvidia_smi() {
        let v = parse_nvidia_smi("NVIDIA GeForce RTX 4070, 3, 1445, 12282, 40\n").unwrap();
        assert_eq!(v["label"], json!("NVIDIA GeForce RTX 4070"));
        assert_eq!(v["load"], json!(3.0));
        assert_eq!(v["memory_used"], json!(1445u64 * 1048576));
        assert_eq!(v["temp"], json!(40.0));
        assert_eq!(parse_nvidia_smi(""), None);
    }

    #[test]
    fn bandwidth_log() {
        let log = "2026-11-08 10 20\n2026-11-09 30 40\n";
        assert_eq!(parse_log_day(log, "2026-11-09"), Some((40, 30)));
        assert_eq!(parse_log_day(log, "2026-11-10"), None);
        assert_eq!(
            update_log(log, "2026-11-09", 31, 41),
            "2026-11-08 10 20\n2026-11-09 31 41\n"
        );
        assert_eq!(update_log("", "2026-11-10", 1, 2), "2026-11-10 1 2\n");
    }
}
