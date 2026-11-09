//! Signal ↔ Lua bridge: the `Signal` userdata returned by `hw.state`,
//! `hw.computed`, `hw.poll`, `hw.option`, `hw.settings`, `hw.monitors`…

use std::rc::Rc;

use hw_core::reactive::{Signal, Subscription, computed};
use hw_ui::monitors::MonitorInfo;
use hw_ui::widgets::Prop;
use mlua::{Function, Lua, MetaMethod, Result, UserData, UserDataMethods, Value, WeakLua};

use crate::convert::{json_to_lua, json_to_prop, lua_to_json, lua_to_prop};
use crate::monitor::LuaMonitor;
use crate::runtime::{RtState, log_lua_error};

/// A reactive value seen from Lua.
pub trait Source {
    /// Current value (registers a dependency inside computeds).
    fn get(&self, lua: &Lua) -> Result<Value>;
    /// Current value as a [`Prop`] (tracked).
    fn get_prop(&self) -> Prop;
    fn set(&self, _lua: &Lua, _v: Value) -> Result<()> {
        Err(mlua::Error::runtime("this signal is read-only"))
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription;
}

/// Signal holding Lua values (`hw.state`, `hw.computed`, `s:map`).
#[derive(Debug)]
pub struct LuaValueSource(pub Signal<Value>);

impl Source for LuaValueSource {
    fn get(&self, _: &Lua) -> Result<Value> {
        Ok(self.0.get())
    }
    fn get_prop(&self) -> Prop {
        self.0.with(lua_to_prop)
    }
    fn set(&self, _: &Lua, v: Value) -> Result<()> {
        self.0.set(v);
        Ok(())
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription {
        self.0.subscribe(move |_| f())
    }
}

/// Setter for JSON-backed signals (settings).
pub type JsonSetter = Rc<dyn Fn(serde_json::Value) -> anyhow::Result<()>>;

/// Signal holding JSON (options, settings).
pub struct JsonSource(pub Signal<serde_json::Value>, pub Option<JsonSetter>);

impl std::fmt::Debug for JsonSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonSource")
            .field("signal", &self.0)
            .field("writable", &self.1.is_some())
            .finish()
    }
}

impl Source for JsonSource {
    fn get(&self, lua: &Lua) -> Result<Value> {
        self.0.with(|v| json_to_lua(lua, v))
    }
    fn get_prop(&self) -> Prop {
        self.0.with(json_to_prop)
    }
    fn set(&self, lua: &Lua, v: Value) -> Result<()> {
        let Some(set) = &self.1 else {
            return Err(mlua::Error::runtime("this signal is read-only"));
        };
        set(lua_to_json(lua, v)?).map_err(|e| mlua::Error::runtime(format!("{e:#}")))
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription {
        self.0.subscribe(move |_| f())
    }
}

/// `hw.monitors`.
#[derive(Debug)]
pub struct MonitorsSource(pub Signal<Vec<MonitorInfo>>);

impl Source for MonitorsSource {
    fn get(&self, lua: &Lua) -> Result<Value> {
        let t = lua.create_table()?;
        for m in self.0.get() {
            t.raw_push(LuaMonitor(m))?;
        }
        Ok(Value::Table(t))
    }
    fn get_prop(&self) -> Prop {
        Prop::List(
            self.0
                .get()
                .into_iter()
                .map(|m| Prop::Monitor(m.monitor))
                .collect(),
        )
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription {
        self.0.subscribe(move |_| f())
    }
}

/// The userdata. Methods: `get`, `set`, `peek`, `map`, `subscribe`,
/// `toggle`; calling it (`s()`) is `get`.
#[derive(Clone)]
pub struct LuaSignal(pub Rc<dyn Source>);

impl std::fmt::Debug for LuaSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("LuaSignal").finish_non_exhaustive()
    }
}

impl LuaSignal {
    pub fn state(v: Value) -> Self {
        Self(Rc::new(LuaValueSource(Signal::new(v))))
    }

    pub fn json(s: Signal<serde_json::Value>, setter: Option<JsonSetter>) -> Self {
        Self(Rc::new(JsonSource(s, setter)))
    }

    /// `hw.computed(fn)`.
    pub fn computed(lua: &Lua, f: Function) -> Self {
        let weak = lua.weak();
        Self(Rc::new(LuaValueSource(computed(move || {
            call_value(&weak, &f, ())
        }))))
    }

    /// `s:map(fn)`.
    #[must_use]
    pub fn map(&self, lua: &Lua, f: Function) -> Self {
        let weak = lua.weak();
        let src = Rc::clone(&self.0);
        Self(Rc::new(LuaValueSource(computed(move || {
            let Some(lua) = weak.try_upgrade() else {
                return Value::Nil;
            };
            match src.get(&lua) {
                Ok(v) => call_value(&weak, &f, v),
                Err(e) => {
                    log_lua_error(&lua, "signal", &e);
                    Value::Nil
                }
            }
        }))))
    }
}

/// Calls `f` if its Lua state is still alive, logging errors as `nil`.
pub fn call_value(weak: &WeakLua, f: &Function, args: impl mlua::IntoLuaMulti) -> Value {
    let Some(lua) = weak.try_upgrade() else {
        return Value::Nil;
    };
    match f.call::<Value>(args) {
        Ok(v) => v,
        Err(e) => {
            log_lua_error(&lua, "computed", &e);
            Value::Nil
        }
    }
}

impl UserData for LuaSignal {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("get", |lua, this, ()| this.0.get(lua));
        m.add_method("peek", |lua, this, ()| {
            hw_core::reactive::untracked(|| this.0.get(lua))
        });
        m.add_method("set", |lua, this, v: Value| this.0.set(lua, v));
        m.add_method("toggle", |lua, this, ()| {
            let v = hw_core::reactive::untracked(|| this.0.get(lua))?;
            let new = !matches!(v, Value::Nil | Value::Boolean(false));
            this.0.set(lua, Value::Boolean(!new))
        });
        m.add_method("map", |lua, this, f: Function| Ok(this.map(lua, f)));
        m.add_method("subscribe", |lua, this, f: Function| {
            let weak = lua.weak();
            let src = Rc::clone(&this.0);
            let sub = this.0.subscribe(Box::new(move || {
                let Some(lua) = weak.try_upgrade() else {
                    return;
                };
                let v = hw_core::reactive::untracked(|| src.get(&lua));
                match v {
                    Ok(v) => {
                        RtState::spawn(&lua, &f, v);
                    }
                    Err(e) => log_lua_error(&lua, "subscribe", &e),
                }
            }));
            // Owned by the runtime so a Lua reload drops it.
            let id = RtState::get(lua)?.keep(Box::new(sub));
            let st = Rc::downgrade(&RtState::get(lua)?);
            lua.create_function(move |_, ()| {
                if let Some(st) = st.upgrade() {
                    st.unkeep(id);
                }
                Ok(())
            })
        });
        m.add_meta_method(MetaMethod::Call, |lua, this, ()| this.0.get(lua));
        m.add_meta_method(MetaMethod::ToString, |lua, this, ()| {
            let v = hw_core::reactive::untracked(|| this.0.get(lua))?;
            Ok(format!("Signal({})", crate::convert::display(lua, &v)))
        });
    }
}

/// Returns the signal inside `v`, if it is one.
pub fn as_signal(v: &Value) -> Option<LuaSignal> {
    match v {
        Value::UserData(ud) => ud.borrow::<LuaSignal>().ok().map(|s| s.clone()),
        _ => None,
    }
}
