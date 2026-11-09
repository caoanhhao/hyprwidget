//! Lua 5.5 runtime (mlua): the `hw.*` API, Signal ↔ Lua bridge, the widget
//! builder, Hyprland-style config loading and the daemon.

pub mod app;
mod app_widget;
pub mod builder;
pub mod cairo;
pub mod convert;
pub mod dbus_services;
pub mod monitor;
pub mod plugin;
pub mod runtime;
pub mod service;
pub mod signal;
pub mod theme;

/// Version string reported by the embedded Lua state (`_VERSION`).
pub fn lua_version() -> mlua::Result<String> {
    mlua::Lua::new().globals().get("_VERSION")
}

#[cfg(test)]
mod plugin_tests;
#[cfg(test)]
mod tests;
