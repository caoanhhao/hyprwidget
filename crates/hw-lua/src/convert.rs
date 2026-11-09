//! Conversions between Lua values, JSON and [`Prop`].

use std::collections::BTreeMap;

use hw_ui::widgets::Prop;
use mlua::serde::{DeserializeOptions, SerializeOptions};
use mlua::{Lua, LuaSerdeExt, Result, Table, Value};

use crate::monitor::LuaMonitor;

/// Error type for `?` into `anyhow` (mlua errors aren't `Send + Sync`).
pub trait LuaResultExt<T> {
    fn anyhow(self) -> anyhow::Result<T>;
}

impl<T> LuaResultExt<T> for Result<T> {
    fn anyhow(self) -> anyhow::Result<T> {
        self.map_err(|e| anyhow::anyhow!("{e}"))
    }
}

pub fn json_to_lua(lua: &Lua, v: &serde_json::Value) -> Result<Value> {
    let opts = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false)
        .set_array_metatable(false);
    lua.to_value_with(v, opts)
}

pub fn lua_to_json(lua: &Lua, v: Value) -> Result<serde_json::Value> {
    let opts = DeserializeOptions::new()
        .deny_unsupported_types(false)
        .deny_recursive_tables(false);
    lua.from_value_with(v, opts)
}

pub fn json_to_prop(v: &serde_json::Value) -> Prop {
    use serde_json::Value as J;
    match v {
        J::Null => Prop::Nil,
        J::Bool(b) => Prop::Bool(*b),
        J::Number(n) => Prop::Num(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Prop::Str(s.clone()),
        J::Array(a) => Prop::List(a.iter().map(json_to_prop).collect()),
        J::Object(o) => Prop::Map(
            o.iter()
                .map(|(k, v)| (k.clone(), json_to_prop(v)))
                .collect(),
        ),
    }
}

/// Lua value → [`Prop`]. Tables with a sequence part become lists.
pub fn lua_to_prop(v: &Value) -> Prop {
    fn go(v: &Value, depth: usize) -> Prop {
        if depth > 16 {
            return Prop::Nil;
        }
        match v {
            Value::Nil => Prop::Nil,
            Value::Boolean(b) => Prop::Bool(*b),
            Value::Integer(i) => Prop::Num(*i as f64),
            Value::Number(n) => Prop::Num(*n),
            Value::String(s) => Prop::Str(s.to_string_lossy()),
            Value::Table(t) => table_to_prop(t, depth),
            Value::UserData(ud) => match ud.borrow::<LuaMonitor>() {
                Ok(m) => Prop::Monitor(m.0.monitor.clone()),
                Err(_) => Prop::Nil,
            },
            _ => Prop::Nil,
        }
    }
    fn table_to_prop(t: &Table, depth: usize) -> Prop {
        if t.raw_len() > 0 {
            return Prop::List(
                t.sequence_values::<Value>()
                    .filter_map(|v| v.ok())
                    .map(|v| go(&v, depth + 1))
                    .collect(),
            );
        }
        let mut m = BTreeMap::new();
        for (k, v) in t.pairs::<Value, Value>().flatten() {
            if let Value::String(k) = k {
                m.insert(k.to_string_lossy(), go(&v, depth + 1));
            }
        }
        Prop::Map(m)
    }
    go(v, 0)
}

pub fn prop_to_lua(lua: &Lua, p: &Prop) -> Result<Value> {
    Ok(match p {
        Prop::Nil => Value::Nil,
        Prop::Bool(b) => Value::Boolean(*b),
        Prop::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => Value::Integer(*n as i64),
        Prop::Num(n) => Value::Number(*n),
        Prop::Str(s) => Value::String(lua.create_string(s)?),
        Prop::List(l) => {
            let t = lua.create_table()?;
            for v in l {
                t.raw_push(prop_to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
        Prop::Map(m) => {
            let t = lua.create_table()?;
            for (k, v) in m {
                t.raw_set(k.as_str(), prop_to_lua(lua, v)?)?;
            }
            Value::Table(t)
        }
        Prop::Monitor(m) => match hw_ui::monitors::current()
            .into_iter()
            .find(|i| &i.monitor == m)
        {
            Some(info) => Value::UserData(lua.create_userdata(LuaMonitor(info))?),
            None => Value::Nil,
        },
    })
}

/// `tostring`-like rendering for `eval`/`option` output: JSON for tables.
pub fn display(lua: &Lua, v: Value) -> String {
    match &v {
        Value::Nil => "nil".into(),
        Value::String(s) => s.to_string_lossy(),
        Value::Table(_) => match lua_to_json(lua, v.clone()) {
            Ok(j) => serde_json::to_string_pretty(&j).unwrap_or_default(),
            Err(_) => v.to_string().unwrap_or_default(),
        },
        _ => v.to_string().unwrap_or_else(|_| format!("{v:?}")),
    }
}
