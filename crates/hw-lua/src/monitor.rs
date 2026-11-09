//! Monitor userdata (`hw.monitors` items, `per_monitor` argument).

use hw_ui::monitors::MonitorInfo;
use mlua::{MetaMethod, UserData, UserDataFields, UserDataMethods};

#[derive(Debug, Clone)]
pub struct LuaMonitor(pub MonitorInfo);

impl UserData for LuaMonitor {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        // `name` matches AGS' `monitor.connector` usage in window names.
        f.add_field_method_get("name", |_, m| Ok(m.0.connector.clone()));
        f.add_field_method_get("connector", |_, m| Ok(m.0.connector.clone()));
        f.add_field_method_get("description", |_, m| Ok(m.0.description.clone()));
        f.add_field_method_get("manufacturer", |_, m| Ok(m.0.manufacturer.clone()));
        f.add_field_method_get("model", |_, m| Ok(m.0.model.clone()));
        f.add_field_method_get("x", |_, m| Ok(m.0.x));
        f.add_field_method_get("y", |_, m| Ok(m.0.y));
        f.add_field_method_get("width", |_, m| Ok(m.0.width));
        f.add_field_method_get("height", |_, m| Ok(m.0.height));
        f.add_field_method_get("scale", |_, m| Ok(m.0.scale));
        f.add_field_method_get("refresh_rate", |_, m| Ok(m.0.refresh_rate));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::ToString, |_, m, ()| {
            Ok(format!("Monitor({})", m.0.connector))
        });
        m.add_meta_method(MetaMethod::Eq, |_, a, b: mlua::UserDataRef<LuaMonitor>| {
            Ok(a.0.monitor == b.0.monitor)
        });
    }
}
