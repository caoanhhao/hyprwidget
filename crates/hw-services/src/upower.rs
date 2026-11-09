//! Battery via UPower's `DisplayDevice` (system bus).

use std::cell::OnceCell;
use std::rc::Rc;

use hw_core::reactive::Signal;
use serde::Serialize;

use crate::dbus::{self, p_bool, p_f64, p_i64, p_str};

const DEST: &str = "org.freedesktop.UPower";
const ROOT: &str = "/org/freedesktop/UPower";
const DISPLAY: &str = "/org/freedesktop/UPower/devices/DisplayDevice";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Battery {
    /// UPower answered (false when the daemon is missing).
    pub available: bool,
    pub is_present: bool,
    /// 0–100.
    pub percentage: f64,
    /// `unknown`, `charging`, `discharging`, `empty`, `fully-charged`,
    /// `pending-charge`, `pending-discharge`.
    pub state: &'static str,
    pub charging: bool,
    /// Seconds (0 when unknown).
    pub time_to_empty: i64,
    pub time_to_full: i64,
    pub on_battery: bool,
    pub icon_name: String,
    /// Watts.
    pub energy_rate: f64,
    /// Wh.
    pub energy: f64,
    pub energy_full: f64,
    /// `none`, `discharging`, `low`, `critical`, `action`.
    pub warning_level: &'static str,
}

fn state_name(n: i64) -> &'static str {
    match n {
        1 => "charging",
        2 => "discharging",
        3 => "empty",
        4 => "fully-charged",
        5 => "pending-charge",
        6 => "pending-discharge",
        _ => "unknown",
    }
}

fn warning_name(n: i64) -> &'static str {
    match n {
        1 => "none",
        2 => "discharging",
        3 => "low",
        4 => "critical",
        5 => "action",
        _ => "unknown",
    }
}

pub struct UPower {
    pub battery: Signal<Battery>,
}

thread_local! {
    static INSTANCE: OnceCell<Rc<UPower>> = const { OnceCell::new() };
}

/// Started on first use (main thread only).
pub fn get() -> Rc<UPower> {
    INSTANCE.with(|i| {
        i.get_or_init(|| {
            let this = Rc::new(UPower {
                battery: Signal::new(Battery {
                    state: "unknown",
                    warning_level: "unknown",
                    ..Default::default()
                }),
            });
            start(&this);
            this
        })
        .clone()
    })
}

fn start(this: &Rc<UPower>) {
    let weak = Rc::downgrade(this);
    dbus::spawn("upower device", async move {
        let conn = dbus::system().await?;
        dbus::watch_props(&conn, DEST, DISPLAY, "org.freedesktop.UPower.Device", |p| {
            let Some(this) = weak.upgrade() else { return };
            let state = p_i64(p, "State");
            this.battery.update(|b| {
                b.available = true;
                b.is_present = p_bool(p, "IsPresent");
                b.percentage = p_f64(p, "Percentage");
                b.state = state_name(state);
                b.charging = matches!(state, 1 | 5);
                b.time_to_empty = p_i64(p, "TimeToEmpty");
                b.time_to_full = p_i64(p, "TimeToFull");
                b.icon_name = p_str(p, "IconName");
                b.energy_rate = p_f64(p, "EnergyRate");
                b.energy = p_f64(p, "Energy");
                b.energy_full = p_f64(p, "EnergyFull");
                b.warning_level = warning_name(p_i64(p, "WarningLevel"));
            });
        })
        .await
    });
    let weak = Rc::downgrade(this);
    dbus::spawn("upower", async move {
        let conn = dbus::system().await?;
        dbus::watch_props(&conn, DEST, ROOT, "org.freedesktop.UPower", |p| {
            if let Some(this) = weak.upgrade() {
                let on = p_bool(p, "OnBattery");
                if this.battery.peek().on_battery != on {
                    this.battery.update(|b| b.on_battery = on);
                }
            }
        })
        .await
    });
}
