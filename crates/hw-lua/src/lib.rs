//! Lua 5.5 runtime (mlua): the `hw.*` API, Signal ↔ Lua bridge, per-plugin
//! sandboxed environments and hot-reload of `hyprwidget.lua`.
//!
//! M0 only proves that the system `liblua.so.5.5` links.

/// Version string reported by the embedded Lua state (`_VERSION`).
pub fn lua_version() -> mlua::Result<String> {
    mlua::Lua::new().globals().get("_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn links_lua_55() {
        assert_eq!(super::lua_version().unwrap(), "Lua 5.5");
    }
}
