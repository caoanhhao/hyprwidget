//! Lua side of `W.App` (PLAN.md §6.3): `self.pid`, `self.title`,
//! `self.app_id` and `self:send_close()`, `self:restart()`, `self:focus()`.
//! On other widgets the fields are nil and the methods do nothing.

use hw_ui::app;
use mlua::{UserDataFields, UserDataMethods};

use crate::builder::LuaWidget;
use crate::signal::LuaSignal;

pub(crate) fn add_fields<F: UserDataFields<LuaWidget>>(f: &mut F) {
    f.add_field_method_get("pid", |_, w| Ok(app::pid(&w.widget)));
    f.add_field_method_get("title", |_, w| {
        Ok(app::signals(&w.widget).map(|s| LuaSignal::json(s.title, None)))
    });
    f.add_field_method_get("app_id", |_, w| {
        Ok(app::signals(&w.widget).map(|s| LuaSignal::json(s.app_id, None)))
    });
}

pub(crate) fn add_methods<M: UserDataMethods<LuaWidget>>(m: &mut M) {
    m.add_method("send_close", |_, w, ()| {
        app::send_close(&w.widget);
        Ok(())
    });
    m.add_method("restart", |_, w, ()| {
        app::restart(&w.widget);
        Ok(())
    });
    m.add_method("focus", |_, w, ()| Ok(app::focus(&w.widget)));
}
