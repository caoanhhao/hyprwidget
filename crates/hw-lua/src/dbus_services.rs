//! `hw.service.{notifd,mpris,tray,upower,power_profiles}`: the D-Bus
//! services of `hw-services` seen from Lua.
//!
//! `hw.service` builds each service table on first access (which starts the
//! service), so configs only pay for what they use. Services themselves are
//! per-process singletons and survive Lua reloads; only the tables and
//! subscriptions are per Lua state. Other modules can add lazy services with
//! [`register`].
//!
//! Service tables hold signals (usable with `hw.bind(service, "field")`) and
//! methods called with `:`.

use std::rc::Rc;

use hw_core::reactive::{Signal, Subscription, computed};
use hw_services::{mpris, notifd, power_profiles, tray, upower};
use hw_ui::gtk4::{self, prelude::*};
use hw_ui::widgets::Prop;
use mlua::serde::SerializeOptions;
use mlua::{Function, Lua, LuaSerdeExt, Result, Table, Value};
use serde::Serialize;

use crate::builder::LuaWidget;
use crate::convert::json_to_prop;
use crate::runtime::RtState;
use crate::signal::{LuaSignal, Source};

const CTORS: &str = "hw.service_ctors";

/// Read-only signal of any serializable value.
struct SerdeSource<T>(Signal<T>);

fn to_lua<T: Serialize>(lua: &Lua, v: &T) -> Result<Value> {
    let opts = SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false)
        .set_array_metatable(false);
    lua.to_value_with(v, opts)
}

impl<T: Serialize + 'static> Source for SerdeSource<T> {
    fn get(&self, lua: &Lua) -> Result<Value> {
        self.0.with(|v| to_lua(lua, v))
    }
    fn get_prop(&self) -> Prop {
        self.0
            .with(|v| json_to_prop(&serde_json::to_value(v).unwrap_or_default()))
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription {
        self.0.subscribe(move |_| f())
    }
}

type Setter = Box<dyn Fn(&Lua, Value) -> Result<()>>;

/// [`SerdeSource`] whose `set` calls a setter.
struct WritableSource<T> {
    sig: Signal<T>,
    set: Setter,
}

impl<T: Serialize + 'static> Source for WritableSource<T> {
    fn get(&self, lua: &Lua) -> Result<Value> {
        self.sig.with(|v| to_lua(lua, v))
    }
    fn get_prop(&self) -> Prop {
        self.sig
            .with(|v| json_to_prop(&serde_json::to_value(v).unwrap_or_default()))
    }
    fn set(&self, lua: &Lua, v: Value) -> Result<()> {
        (self.set)(lua, v)
    }
    fn subscribe(&self, f: Box<dyn Fn()>) -> Subscription {
        self.sig.subscribe(move |_| f())
    }
}

fn sig<T: Serialize + 'static>(s: Signal<T>) -> LuaSignal {
    LuaSignal(Rc::new(SerdeSource(s)))
}

/// Signal of one field of a struct signal.
fn field<T: Serialize + Clone + 'static>(s: &Signal<T>, name: &'static str) -> LuaSignal {
    let s = s.clone();
    let f = computed(move || {
        s.with(|v| {
            serde_json::to_value(v)
                .ok()
                .and_then(|j| j.get(name).cloned())
                .unwrap_or_default()
        })
    });
    LuaSignal::json(f, None)
}

/// Keeps `sub` alive for the Lua state; returns an unsubscribe function.
fn keep_sub(lua: &Lua, sub: Subscription) -> Result<Function> {
    let st = RtState::get(lua)?;
    let id = st.keep(Box::new(sub));
    let st = Rc::downgrade(&st);
    lua.create_function(move |_, ()| {
        if let Some(st) = st.upgrade() {
            st.unkeep(id);
        }
        Ok(())
    })
}

/// Adds a lazily built `hw.service.<name>`.
pub fn register(lua: &Lua, name: &str, ctor: Function) -> Result<()> {
    let ctors: Table = lua.named_registry_value(CTORS)?;
    ctors.set(name, ctor)
}

/// Installs `hw.service` (keeping an existing table) and our services.
pub fn install(lua: &Lua, hw: &Table) -> Result<()> {
    let service = match hw.get::<Value>("service")? {
        Value::Table(t) => t,
        _ => {
            let t = lua.create_table()?;
            hw.set("service", &t)?;
            t
        }
    };
    let ctors = lua.create_table()?;
    lua.set_named_registry_value(CTORS, &ctors)?;
    let mt = lua.create_table()?;
    mt.set(
        "__index",
        lua.create_function(|lua, (t, k): (Table, Value)| {
            let ctors: Table = lua.named_registry_value(CTORS)?;
            let Value::Function(ctor) = ctors.get::<Value>(&k)? else {
                return Ok(Value::Nil);
            };
            let v: Value = ctor.call(())?;
            t.raw_set(k, &v)?;
            Ok(v)
        })?,
    )?;
    service.set_metatable(Some(mt))?;

    register(
        lua,
        "notifd",
        lua.create_function(|lua, ()| notifd_table(lua))?,
    )?;
    register(
        lua,
        "mpris",
        lua.create_function(|lua, ()| mpris_table(lua))?,
    )?;
    register(lua, "tray", lua.create_function(|lua, ()| tray_table(lua))?)?;
    register(
        lua,
        "upower",
        lua.create_function(|lua, ()| upower_table(lua))?,
    )?;
    register(
        lua,
        "power_profiles",
        lua.create_function(|lua, ()| power_profiles_table(lua))?,
    )?;
    Ok(())
}

/// `t.name = function(self, ...)`.
macro_rules! method {
    ($lua:expr, $t:expr, $name:literal, |$l:tt, $args:tt : $ty:ty| $body:expr) => {
        $t.set(
            $name,
            $lua.create_function(move |$l, (_self, a): (Value, $ty)| {
                let $args = a;
                $body
            })?,
        )?
    };
}

fn notifd_table(lua: &Lua) -> Result<Table> {
    let n = notifd::get();
    let t = lua.create_table()?;
    t.set("notifications", sig(n.notifications.clone()))?;
    let n2 = n.clone();
    t.set(
        "dnd",
        LuaSignal(Rc::new(WritableSource {
            sig: n.dnd.clone(),
            set: Box::new(move |_, v| {
                n2.set_dnd(!matches!(v, Value::Nil | Value::Boolean(false)));
                Ok(())
            }),
        })),
    )?;
    t.set("active", sig(n.active.clone()))?;

    let n2 = n.clone();
    method!(lua, t, "dismiss", |_, id: u32| {
        n2.dismiss(id);
        Ok(())
    });
    let n2 = n.clone();
    method!(lua, t, "close", |_, id: u32| {
        n2.close(id, notifd::CloseReason::Closed);
        Ok(())
    });
    let n2 = n.clone();
    method!(lua, t, "invoke", |_, (id, action): (u32, String)| {
        n2.invoke(id, &action);
        Ok(())
    });
    let n2 = n.clone();
    t.set(
        "clear",
        lua.create_function(move |_, _self: Value| {
            n2.clear();
            Ok(())
        })?,
    )?;
    let n2 = n.clone();
    method!(lua, t, "get", |lua, id: u32| match n2.get(id) {
        Some(n) => to_lua(lua, &n),
        None => Ok(Value::Nil),
    });
    let n2 = n.clone();
    method!(lua, t, "set_dnd", |_, on: bool| {
        n2.set_dnd(on);
        Ok(())
    });
    let n2 = n.clone();
    method!(lua, t, "set_ignore_timeout", |_, on: bool| {
        n2.ignore_timeout.set(on);
        Ok(())
    });
    // `notifd:on_notified(function(n, replaced) end)` → unsubscribe fn.
    let n2 = n.clone();
    method!(lua, t, "on_notified", |lua, f: Function| {
        let (weak, n3) = (lua.weak(), n2.clone());
        let sub = n2.notified.subscribe(move |&(id, replaced)| {
            let (Some(lua), Some(n)) = (weak.try_upgrade(), n3.get(id)) else {
                return;
            };
            if let Ok(v) = to_lua(&lua, &n) {
                RtState::spawn(&lua, &f, (v, replaced));
            }
        });
        keep_sub(lua, sub)
    });
    // `notifd:on_resolved(function(id, reason) end)`; reason is
    // `expired`, `dismissed`, `closed` or `undefined`.
    let n2 = n.clone();
    method!(lua, t, "on_resolved", |lua, f: Function| {
        let weak = lua.weak();
        let sub = n2.resolved.subscribe(move |(id, reason)| {
            let Some(lua) = weak.try_upgrade() else {
                return;
            };
            if let Ok(r) = to_lua(&lua, reason) {
                RtState::spawn(&lua, &f, (*id, r));
            }
        });
        keep_sub(lua, sub)
    });
    Ok(t)
}

fn mpris_table(lua: &Lua) -> Result<Table> {
    let m = mpris::get();
    let t = lua.create_table()?;
    t.set("players", sig(m.players.clone()))?;
    let m2 = m.clone();
    method!(lua, t, "position", |_, bus: String| Ok(sig(
        m2.position(&bus)
    )));
    macro_rules! ctl {
        ($($name:literal => $f:ident),*) => {$(
            let m2 = m.clone();
            method!(lua, t, $name, |_, bus: String| {
                m2.$f(&bus);
                Ok(())
            });
        )*};
    }
    ctl!(
        "play_pause" => play_pause, "play" => play, "pause" => pause, "stop" => stop,
        "next" => next, "previous" => previous, "raise" => raise
    );
    let m2 = m.clone();
    method!(lua, t, "seek", |_, (bus, secs): (String, f64)| {
        m2.seek(&bus, secs);
        Ok(())
    });
    let m2 = m.clone();
    method!(lua, t, "set_position", |_, (bus, secs): (String, f64)| {
        m2.set_position(&bus, secs);
        Ok(())
    });
    let m2 = m.clone();
    method!(lua, t, "set_volume", |_, (bus, v): (String, f64)| {
        m2.set_volume(&bus, v);
        Ok(())
    });
    let m2 = m.clone();
    method!(lua, t, "set_shuffle", |_, (bus, on): (String, bool)| {
        m2.set_shuffle(&bus, on);
        Ok(())
    });
    let m2 = m.clone();
    method!(
        lua,
        t,
        "set_loop_status",
        |_, (bus, s): (String, String)| {
            m2.set_loop_status(&bus, &s);
            Ok(())
        }
    );
    Ok(t)
}

fn tray_table(lua: &Lua) -> Result<Table> {
    let tr = tray::get();
    let t = lua.create_table()?;
    t.set("items", sig(tr.items.clone()))?;
    t.set("watcher_active", sig(tr.watcher_active.clone()))?;
    let tr2 = tr.clone();
    method!(lua, t, "activate", |_,
                                 (id, x, y): (
        String,
        Option<i32>,
        Option<i32>
    )| {
        tr2.activate(&id, x.unwrap_or(0), y.unwrap_or(0));
        Ok(())
    });
    let tr2 = tr.clone();
    method!(lua, t, "secondary_activate", |_,
                                           (id, x, y): (
        String,
        Option<i32>,
        Option<i32>
    )| {
        tr2.secondary_activate(&id, x.unwrap_or(0), y.unwrap_or(0));
        Ok(())
    });
    let tr2 = tr.clone();
    method!(lua, t, "context_menu", |_,
                                     (id, x, y): (
        String,
        Option<i32>,
        Option<i32>
    )| {
        tr2.context_menu(&id, x.unwrap_or(0), y.unwrap_or(0));
        Ok(())
    });
    let tr2 = tr.clone();
    method!(lua, t, "scroll", |_,
                               (id, delta, o): (
        String,
        i32,
        Option<String>
    )| {
        tr2.scroll(&id, delta, o.as_deref().unwrap_or("vertical"));
        Ok(())
    });
    let tr2 = tr.clone();
    method!(lua, t, "about_to_show", |_, id: String| {
        if let Some(m) = tr2.menu(&id) {
            m.about_to_show();
        }
        Ok(())
    });
    // `tray:menu_layout(item.id)`: the menu tree as tables (nil until
    // loaded), for menus rendered in Lua.
    let tr2 = tr.clone();
    method!(lua, t, "menu_layout", |lua, id: String| {
        match tr2.menu(&id).and_then(|m| m.root.borrow().clone()) {
            Some(root) => to_lua(lua, &root),
            None => Ok(Value::Nil),
        }
    });
    // `tray:attach_menu(item.id, self)` in a MenuButton's `setup`: sets its
    // menu model and action group (kept in sync with the app).
    let tr2 = tr.clone();
    method!(lua, t, "attach_menu", |_,
                                    (id, w): (
        String,
        mlua::UserDataRef<LuaWidget>
    )| {
        let Some(menu) = tr2.menu(&id) else {
            return Ok(false);
        };
        w.widget
            .insert_action_group(tray::ACTION_PREFIX, Some(&menu.actions));
        if let Some(btn) = w.widget.downcast_ref::<gtk4::MenuButton>() {
            btn.set_menu_model(Some(&menu.model));
            let weak = Rc::downgrade(&menu);
            btn.connect_active_notify(move |b| {
                if b.is_active()
                    && let Some(m) = weak.upgrade()
                {
                    m.about_to_show();
                }
            });
        }
        Ok(true)
    });
    // `tray:popup_menu(item.id, widget)`: shows the menu under any widget.
    let tr2 = tr.clone();
    method!(lua, t, "popup_menu", |_,
                                   (id, w): (
        String,
        mlua::UserDataRef<LuaWidget>
    )| {
        let Some(menu) = tr2.menu(&id) else {
            return Ok(false);
        };
        menu.about_to_show();
        w.widget
            .insert_action_group(tray::ACTION_PREFIX, Some(&menu.actions));
        let pop = gtk4::PopoverMenu::from_model(Some(&menu.model));
        pop.set_parent(&w.widget);
        pop.connect_closed(|p| {
            let p = p.clone();
            // Unparent after the action (if any) was activated.
            glib_idle(move || p.unparent());
        });
        pop.popup();
        Ok(true)
    });
    Ok(t)
}

fn glib_idle(f: impl FnOnce() + 'static) {
    gtk4::glib::idle_add_local_once(f);
}

fn upower_table(lua: &Lua) -> Result<Table> {
    let u = upower::get();
    let t = lua.create_table()?;
    t.set("battery", sig(u.battery.clone()))?;
    for name in [
        "available",
        "is_present",
        "percentage",
        "state",
        "charging",
        "time_to_empty",
        "time_to_full",
        "on_battery",
        "icon_name",
        "energy_rate",
        "energy",
        "energy_full",
        "warning_level",
    ] {
        t.set(name, field(&u.battery, name))?;
    }
    Ok(t)
}

fn power_profiles_table(lua: &Lua) -> Result<Table> {
    let p = power_profiles::get();
    let t = lua.create_table()?;
    t.set("state", sig(p.state.clone()))?;
    for name in ["available", "profiles", "performance_degraded"] {
        t.set(name, field(&p.state, name))?;
    }
    let p2 = p.clone();
    let active = p.state.map(|s| s.active_profile.clone());
    t.set(
        "active_profile",
        LuaSignal(Rc::new(WritableSource {
            sig: active,
            set: Box::new(move |_, v| match v {
                Value::String(s) => {
                    p2.set_active(&s.to_str()?);
                    Ok(())
                }
                _ => Err(mlua::Error::runtime("active_profile: expected a string")),
            }),
        })),
    )?;
    let p2 = p.clone();
    method!(lua, t, "set_active_profile", |_, profile: String| {
        p2.set_active(&profile);
        Ok(())
    });
    Ok(t)
}
