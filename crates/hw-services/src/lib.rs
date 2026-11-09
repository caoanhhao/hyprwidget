//! Native Rust services exposed to widgets: Hyprland IPC, notifd, mpris, tray,
//! audio, battery, network, apps, sysinfo, cava…
//!
//! Each service runs lazily (only when Lua actually uses it) and pushes data to
//! the GTK main thread as `hw-core` signals. Empty until M2.

pub mod dbus;
pub mod mpris;
pub mod notifd;
pub mod power_profiles;
pub mod tray;
pub mod upower;
