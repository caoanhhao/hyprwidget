//! `hw.service.<name>`: services from `hw-services`, started lazily on first
//! access and kept in [`Core`](crate::runtime::Core) so a Lua reload reuses
//! them.
//!
//! ```lua
//! local hypr = hw.service.hyprland
//! hypr.focused_client               -- Signal (JSON → Lua tables)
//! hypr:dispatch("hl.dsp.focus({ workspace = 3 })")
//! hw.bind(hypr, "workspaces")       -- same as hypr.workspaces
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hw_services::Service;
use mlua::{
    Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataMethods, Value, Variadic,
};

use crate::convert::{json_to_lua, lua_to_json};
use crate::runtime::RtState;
use crate::signal::LuaSignal;

/// Started services, by name (lives in `Core`).
#[derive(Default)]
pub struct Services(RefCell<HashMap<String, Rc<Service>>>);

impl Services {
    pub fn get(&self, name: &str) -> Result<Option<Rc<Service>>> {
        if let Some(s) = self.0.borrow().get(name) {
            return Ok(Some(s.clone()));
        }
        let Some(started) = hw_services::start(name) else {
            return Ok(None);
        };
        let svc = started
            .map_err(|e| mlua::Error::runtime(format!("starting service '{name}': {e:#}")))?;
        tracing::info!(service = name, "service started");
        self.0.borrow_mut().insert(name.to_owned(), svc.clone());
        Ok(Some(svc))
    }
}

/// A service seen from Lua: props are signals, methods are `svc:method()`.
#[derive(Clone)]
pub struct LuaService(pub Rc<Service>);

impl LuaService {
    fn index(&self, lua: &Lua, key: &str) -> Result<Value> {
        if let Some(sig) = self.0.prop(key) {
            return lua.pack(LuaSignal::json(sig.clone(), None));
        }
        if self.0.has_method(key) {
            let svc = self.0.clone();
            let name = key.to_owned();
            let f = lua.create_function(move |lua, args: Variadic<Value>| {
                // Called as `svc:method(...)`: drop `self`.
                let mut it = args.into_iter().peekable();
                if matches!(it.peek(), Some(Value::UserData(ud)) if ud.is::<LuaService>()) {
                    it.next();
                }
                let args = it
                    .map(|v| lua_to_json(lua, v))
                    .collect::<Result<Vec<_>>>()?;
                match svc.call(&name, args) {
                    Some(Ok(v)) => json_to_lua(lua, &v),
                    Some(Err(e)) => {
                        Err(mlua::Error::runtime(format!("{}:{name}: {e:#}", svc.name)))
                    }
                    None => Ok(Value::Nil),
                }
            })?;
            return Ok(Value::Function(f));
        }
        match key {
            "name" => lua.pack(self.0.name),
            "props" => lua.pack(self.0.prop_names().collect::<Vec<_>>()),
            "methods" => lua.pack(self.0.method_names().collect::<Vec<_>>()),
            _ => Ok(Value::Nil),
        }
    }
}

impl UserData for LuaService {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.index(lua, &key)
        });
        m.add_meta_method(MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Service({})", this.0.name))
        });
    }
}

/// Installs `hw.service` and `hw.bind`.
pub fn install(lua: &Lua, hw: &Table) -> Result<()> {
    let service = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(|lua, (_, name): (Table, String)| {
            let st = RtState::get(lua)?;
            Ok(st.core.services.get(&name)?.map(LuaService))
        })?,
    )?;
    service.set_metatable(Some(meta))?;
    service.set("names", hw_services::NAMES.to_vec())?;
    hw.set("service", service)?;

    // `hw.bind(obj, "prop")`: AGS' createBinding; works on services and on
    // any table/userdata field.
    hw.set(
        "bind",
        lua.create_function(|lua, (obj, prop): (Value, String)| match &obj {
            Value::UserData(ud) if ud.is::<LuaService>() => {
                ud.borrow::<LuaService>()?.index(lua, &prop)
            }
            Value::Table(t) => t.get::<Value>(prop),
            Value::UserData(ud) => ud.get::<Value>(prop),
            _ => Err(mlua::Error::runtime("hw.bind: expected a service or table")),
        })?,
    )?;
    Ok(())
}
