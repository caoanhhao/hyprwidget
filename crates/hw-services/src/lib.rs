//! Native Rust services exposed to widgets: Hyprland IPC, notifd, mpris, tray,
//! audio, battery, network, apps, sysinfo, cava…
//!
//! Each service runs lazily (only when Lua actually uses it) and pushes data to
//! the GTK main thread as `hw-core` signals. See [`service::Service`] for the
//! shape Lua sees (`hw.service.<name>.<prop>` signals, `:<method>(...)`).

use std::rc::Rc;

mod num;
pub mod service;

pub use service::Service;

// Services (one module each; keep this list append-only).
pub mod apps;
pub mod audio;
pub mod battery;
pub mod brightness;
pub mod hyprland;
pub mod hyprsunset;
pub mod keystrokes;
pub mod sysinfo;
pub mod theme;

// D-Bus services (own Lua bridge in `hw-lua::dbus_services`).
pub mod dbus;
pub mod mpris;
pub mod notifd;
pub mod power_profiles;
pub mod tray;
pub mod upower;

/// Starts service `name`; `None` if there is no such service. Called once per
/// name, on the main thread, the first time Lua touches it.
pub fn start(name: &str) -> Option<anyhow::Result<Rc<Service>>> {
    Some(match name {
        "hyprland" => hyprland::start(),
        "sysinfo" => sysinfo::start(),
        "apps" => apps::start(),
        "audio" => audio::start(),
        "battery" => battery::start(),
        "brightness" => brightness::start(),
        "hyprsunset" => hyprsunset::start(),
        "theme" => theme::start(),
        "keystrokes" => keystrokes::start(),
        _ => return None,
    })
}

/// Prop names of service `name` without starting it (docs/stub checks).
pub fn props(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "hyprland" => hyprland::PROPS,
        "sysinfo" => sysinfo::PROPS,
        "apps" => apps::PROPS,
        "audio" => audio::PROPS,
        "battery" => battery::PROPS,
        "brightness" => brightness::PROPS,
        "hyprsunset" => hyprsunset::PROPS,
        "theme" => theme::PROPS,
        "keystrokes" => keystrokes::PROPS,
        _ => return None,
    })
}

/// Names accepted by [`start`].
pub const NAMES: &[&str] = &[
    "hyprland",
    "sysinfo",
    "apps",
    "audio",
    "battery",
    "brightness",
    "hyprsunset",
    "theme",
    "keystrokes",
];
