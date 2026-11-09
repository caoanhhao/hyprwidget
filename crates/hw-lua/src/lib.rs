//! Lua 5.5 runtime (mlua): the `hw.*` API, Signal ↔ Lua bridge, the widget
//! builder, Hyprland-style config loading and the daemon.

pub mod app;
pub mod builder;
pub mod cairo;
pub mod convert;
pub mod monitor;
pub mod runtime;
pub mod service;
pub mod signal;

/// Version string reported by the embedded Lua state (`_VERSION`).
pub fn lua_version() -> mlua::Result<String> {
    mlua::Lua::new().globals().get("_VERSION")
}

#[cfg(test)]
mod tests;
