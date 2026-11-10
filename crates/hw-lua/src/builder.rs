//! Lua widget tables (and JSON node trees) → GTK widgets.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hw_ui::gtk4::{self, prelude::*};
use hw_ui::widgets::{self, Child, Event, Kind, Prop};
use mlua::{
    Function, Lua, MetaMethod, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};

use crate::cairo::Cairo;
use crate::convert::{json_to_lua, lua_to_json, lua_to_prop, prop_to_lua};
use crate::runtime::{RtState, log_lua_error, peek_prop};
use crate::signal::{LuaSignal, as_signal};

/// Things whose lifetime is the widget's (subscriptions, inline CSS).
#[derive(Clone, Default)]
struct Keep(Rc<RefCell<Vec<Box<dyn Any>>>>);

impl Keep {
    fn for_widget(w: &gtk4::Widget) -> Self {
        let k = Keep::default();
        let k2 = k.clone();
        w.connect_destroy(move |_| k2.0.borrow_mut().clear());
        k
    }

    fn push(&self, thing: impl Any) {
        self.0.borrow_mut().push(Box::new(thing));
    }
}

fn rt_err(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(msg.into())
}

/// `self` in `setup`/handlers.
#[derive(Debug, Clone)]
pub struct LuaWidget {
    pub widget: gtk4::Widget,
    pub kind: Option<Kind>,
}

impl UserData for LuaWidget {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("kind", |_, w| Ok(w.kind.map(Kind::name)));
        f.add_field_method_get("name", |_, w| Ok(w.widget.widget_name().to_string()));
        crate::app_widget::add_fields(f);
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("add_class", |_, w, c: String| {
            w.widget.add_css_class(&c);
            Ok(())
        });
        m.add_method("remove_class", |_, w, c: String| {
            w.widget.remove_css_class(&c);
            Ok(())
        });
        m.add_method("toggle_class", |_, w, (c, on): (String, Option<bool>)| {
            let on = on.unwrap_or(!w.widget.has_css_class(&c));
            if on {
                w.widget.add_css_class(&c);
            } else {
                w.widget.remove_css_class(&c);
            }
            Ok(on)
        });
        m.add_method("has_class", |_, w, c: String| {
            Ok(w.widget.has_css_class(&c))
        });
        m.add_method("show", |_, w, ()| {
            w.widget.set_visible(true);
            Ok(())
        });
        m.add_method("hide", |_, w, ()| {
            w.widget.set_visible(false);
            Ok(())
        });
        m.add_method("is_visible", |_, w, ()| Ok(w.widget.is_visible()));
        m.add_method("queue_draw", |_, w, ()| {
            w.widget.queue_draw();
            Ok(())
        });
        m.add_method("grab_focus", |_, w, ()| Ok(w.widget.grab_focus()));
        m.add_method("set", |_, w, (name, v): (String, Value)| {
            let kind = w.kind.ok_or_else(|| rt_err("unknown widget kind"))?;
            widgets::set_prop(&w.widget, kind, &name, &lua_to_prop(&v)).map_err(rt_err)
        });
        // GObject property by name (`_` or `-`).
        m.add_method("get", |lua, w, name: String| {
            let pname = name.replace('_', "-");
            if w.widget.find_property(&pname).is_none() {
                return Ok(Value::Nil);
            }
            prop_to_lua(
                lua,
                &widgets::value_to_prop(&w.widget.property_value(&pname)),
            )
        });
        m.add_method("on", |lua, w, (event, f): (String, Function)| {
            let kind = w.kind.ok_or_else(|| rt_err("unknown widget kind"))?;
            connect(lua, &w.widget, kind, &event, f)
        });
        m.add_method("parent", |_, w, ()| {
            Ok(w.widget.parent().map(|p| LuaWidget {
                widget: p,
                kind: None,
            }))
        });
        m.add_method("window", |_, w, ()| {
            Ok(w.widget
                .root()
                .and_downcast::<gtk4::Window>()
                .map(LuaWindow))
        });
        m.add_meta_method(MetaMethod::ToString, |_, w, ()| {
            Ok(format!("Widget({})", w.kind.map_or("?", Kind::name)))
        });
        crate::app_widget::add_methods(m);
    }
}

/// `hw.window.get(name)`.
#[derive(Debug, Clone)]
pub struct LuaWindow(pub gtk4::Window);

impl UserData for LuaWindow {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("visible", |_, w| Ok(w.0.is_visible()));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("show", |_, w, ()| {
            hw_ui::windows::present(&w.0);
            Ok(())
        });
        m.add_method("hide", |_, w, ()| {
            w.0.set_visible(false);
            Ok(())
        });
        m.add_method("toggle", |_, w, ()| {
            let show = !w.0.is_visible();
            if show {
                hw_ui::windows::present(&w.0);
            } else {
                w.0.set_visible(false);
            }
            Ok(show)
        });
        m.add_method("is_visible", |_, w, ()| Ok(w.0.is_visible()));
        m.add_method("set", |_, w, (name, v): (String, Value)| {
            widgets::set_prop(w.0.upcast_ref(), Kind::Window, &name, &lua_to_prop(&v))
                .map_err(rt_err)
        });
        m.add_method("widget", |_, w, ()| {
            Ok(LuaWidget {
                widget: w.0.clone().upcast(),
                kind: Some(Kind::Window),
            })
        });
        m.add_method("destroy", |lua, w, ()| {
            match RtState::get(lua)?.core.ui.as_ref() {
                Some(ui) => ui.windows.remove_window(&w.0),
                None => w.0.destroy(),
            }
            Ok(())
        });
    }
}

/// Lua arguments for an event (after `self`).
fn event_args(lua: &Lua, ev: Event) -> Result<mlua::MultiValue> {
    use mlua::IntoLuaMulti;
    match ev {
        Event::Click { button } => button.into_lua_multi(lua),
        Event::Scroll { dx, dy } => (dx, dy).into_lua_multi(lua),
        Event::Hover { x, y } | Event::Motion { x, y } => (x, y).into_lua_multi(lua),
        Event::Leave | Event::Destroy => ().into_lua_multi(lua),
        Event::Key { name, code, mods } => (name, code, mods).into_lua_multi(lua),
        Event::Value(p) => prop_to_lua(lua, &p)?.into_lua_multi(lua),
    }
}

fn connect(lua: &Lua, w: &gtk4::Widget, kind: Kind, event: &str, f: Function) -> Result<()> {
    let weak_lua = lua.weak();
    let weak_w = w.downgrade();
    let handler: widgets::Handler = Rc::new(move |ev| {
        let Some(lua) = weak_lua.try_upgrade() else {
            return false;
        };
        // GLib clears weak refs before disposing an object whose last
        // reference drops (a child removed by `hw.For`), so `on_destroy`
        // still runs then, with a nil `self`.
        let w = weak_w.upgrade();
        if w.is_none() && !matches!(ev, Event::Destroy) {
            return false;
        }
        let args = match event_args(&lua, ev) {
            Ok(a) => a,
            Err(e) => {
                log_lua_error(&lua, "event", &e);
                return false;
            }
        };
        let mut all = mlua::MultiValue::new();
        match w
            .map(|widget| {
                lua.create_userdata(LuaWidget {
                    widget,
                    kind: Some(kind),
                })
            })
            .transpose()
        {
            Ok(ud) => all.push_back(ud.map_or(Value::Nil, Value::UserData)),
            Err(e) => {
                log_lua_error(&lua, "event", &e);
                return false;
            }
        }
        all.extend(args);
        !matches!(
            RtState::spawn(&lua, &f, all),
            Value::Nil | Value::Boolean(false)
        )
    });
    widgets::connect_event(w, kind, event, handler).map_err(rt_err)
}

/// Applies a static or reactive property.
fn bind_prop(w: &gtk4::Widget, kind: Kind, name: &str, v: &Value, keep: &Keep) -> Result<()> {
    let Some(sig) = as_signal(v) else {
        return widgets::set_prop(w, kind, name, &lua_to_prop(v)).map_err(rt_err);
    };
    widgets::set_prop(w, kind, name, &peek_prop(&sig)).map_err(rt_err)?;
    let weak = w.downgrade();
    let (name, src) = (name.to_owned(), sig.clone());
    keep.push(sig.0.subscribe(Box::new(move || {
        if let Some(w) = weak.upgrade()
            && let Err(e) = widgets::set_prop(&w, kind, &name, &peek_prop(&src))
        {
            tracing::warn!("{e}");
        }
    })));
    Ok(())
}

/// Binds an option signal as a default property value.
fn bind_option(
    lua: &Lua,
    w: &gtk4::Widget,
    kind: Kind,
    prop: &str,
    option: &str,
    keep: &Keep,
) -> Result<()> {
    let st = RtState::get(lua)?;
    let sig = lua.create_userdata(LuaSignal::json(st.core.options.signal(option), None))?;
    bind_prop(w, kind, prop, &Value::UserData(sig), keep)
}

/// Builds any widget value: widget table, string/number (→ Label), signal
/// (→ dynamic child), `LuaWidget`, or nil/false (→ nothing).
pub fn build(lua: &Lua, v: &Value) -> Result<Option<gtk4::Widget>> {
    match v {
        Value::Nil | Value::Boolean(false) => Ok(None),
        Value::String(_) | Value::Integer(_) | Value::Number(_) => {
            let w = widgets::create(Kind::Label);
            widgets::set_prop(&w, Kind::Label, "label", &lua_to_prop(v)).map_err(rt_err)?;
            Ok(Some(w))
        }
        Value::UserData(ud) => {
            if let Ok(w) = ud.borrow::<LuaWidget>() {
                return Ok(Some(w.widget.clone()));
            }
            if let Some(sig) = as_signal(v) {
                let t = lua.create_table()?;
                t.set("type", "With")?;
                t.set("signal", sig)?;
                return build_with(lua, &t).map(Some);
            }
            Err(rt_err("cannot build a widget from this userdata"))
        }
        Value::Table(t) => build_table(lua, t).map(Some),
        other => Err(rt_err(format!(
            "cannot build a widget from a {}",
            other.type_name()
        ))),
    }
}

fn build_table(lua: &Lua, t: &Table) -> Result<gtk4::Widget> {
    let ty: Option<String> = t.get("type")?;
    let Some(ty) = ty else {
        return Err(rt_err("widget table without `type` (use W.Box{…} etc.)"));
    };
    match ty.as_str() {
        "With" => return build_with(lua, t),
        "For" => return build_for(lua, t),
        "Window" => return Err(rt_err("W.Window can only be a top-level window")),
        _ => {}
    }
    if let Some(f) = RtState::get(lua)?.defined.borrow().get(&ty).cloned() {
        let v: Value = f.call(t.clone())?;
        return build(lua, &v)?.ok_or_else(|| rt_err(format!("W.{ty} returned nothing")));
    }
    let kind = Kind::from_name(&ty).ok_or_else(|| rt_err(format!("unknown widget type '{ty}'")))?;
    let w = widgets::create(kind);
    configure(lua, &w, kind, t)?;
    Ok(w)
}

/// Applies everything in `t` to a freshly created widget.
#[expect(
    clippy::too_many_lines,
    reason = "one pass over the widget table; each key kind is a short, independent step"
)]
fn configure(lua: &Lua, w: &gtk4::Widget, kind: Kind, t: &Table) -> Result<()> {
    let keep = Keep::for_widget(w);
    let st = RtState::get(lua)?;

    let mut positional: Vec<(i64, Value)> = Vec::new();
    let mut children: Option<Value> = None;
    let mut slots: Vec<(String, Value)> = Vec::new();
    let mut events: Vec<(String, Function)> = Vec::new();
    let mut props: Vec<(String, Value)> = Vec::new();
    let (mut setup, mut draw, mut css, mut theme, mut visible) = (None, None, None, None, None);
    let mut popover = None;

    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        match k {
            Value::Integer(idx) => positional.push((idx, v)),
            Value::String(k) => {
                let k = k.to_str()?.to_string();
                match k.as_str() {
                    "type" | "col" | "row" | "col_span" | "row_span" => {}
                    "children" => children = Some(v),
                    "setup" => setup = Some(Function::from_lua_value(v)?),
                    "draw" => draw = Some(Function::from_lua_value(v)?),
                    "css" => css = Some(v),
                    "theme" => theme = Some(v),
                    "visible" => visible = Some(v),
                    // Any widget may own a popover (MenuButton has a slot).
                    "popover" if kind != Kind::MenuButton => popover = Some(v),
                    "name" if kind != Kind::Window => {
                        if let Value::String(name) = &v {
                            w.set_widget_name(&name.to_str()?);
                        }
                    }
                    name if kind.slots().contains(&name) => slots.push((k, v)),
                    name if widgets::EVENTS.contains(&name) => {
                        events.push((k, Function::from_lua_value(v)?));
                    }
                    _ => props.push((k, v)),
                }
            }
            _ => {}
        }
    }

    // Defaults inherited from Hyprland (PLAN.md §3.6).
    let has = |n: &str| props.iter().any(|(k, _)| k == n);
    match kind {
        Kind::Box if !has("spacing") => {
            bind_option(lua, w, kind, "spacing", "general:gaps_in", &keep)?;
        }
        Kind::Revealer | Kind::Stack if !has("duration") && !has("transition_duration") => {
            bind_option(
                lua,
                w,
                kind,
                "duration",
                "animations:animation:layersIn:duration",
                &keep,
            )?;
        }
        Kind::Window if !has("margin") => {
            bind_option(lua, w, kind, "margin", "general:gaps_out", &keep)?;
        }
        // Terminal colors follow the theme (`hw.theme.colors`).
        Kind::Terminal if !has("colors") => {
            let sig = lua.create_userdata(LuaSignal::json(st.core.theme.colors.clone(), None))?;
            bind_prop(w, kind, "colors", &Value::UserData(sig), &keep)?;
        }
        Kind::App if !has("rounding") => {
            bind_option(lua, w, kind, "rounding", "decoration:rounding", &keep)?;
        }
        _ => {}
    }

    // Children: positional entries, then `children`.
    positional.sort_by_key(|(i, _)| *i);
    if kind == Kind::Window || (!kind.takes_children() && !kind.slots().is_empty()) {
        // `W.Window{ W.Box{} }`, `W.Button{ "label" }`: first positional is the child.
        if let Some((_, v)) = positional.first()
            && !slots.iter().any(|(s, _)| s == "child")
        {
            slots.push(("child".into(), v.clone()));
        }
    } else if !positional.is_empty() || children.is_some() {
        if !kind.takes_children() {
            return Err(rt_err(format!("{} does not take children", kind.name())));
        }
        let mut list: Vec<Value> = positional.into_iter().map(|(_, v)| v).collect();
        match children {
            Some(Value::Table(ct)) => {
                list.extend(ct.sequence_values::<Value>().collect::<Result<Vec<_>>>()?);
            }
            Some(v) => {
                if let Some(sig) = as_signal(&v) {
                    bind_children(lua, w, kind, t, list, &sig, &keep)?;
                    list = Vec::new();
                }
            }
            None => {}
        }
        if !list.is_empty() {
            let built = build_children(lua, &list)?;
            widgets::add_children(w, kind, built, columns(t)?).map_err(rt_err)?;
        }
    }

    // A window that starts hidden builds its content when first shown.
    let hidden = kind == Kind::Window
        && match &visible {
            Some(Value::Boolean(false)) => true,
            Some(v) => as_signal(v).is_some_and(|s| !peek_prop(&s).truthy()),
            None => false,
        };
    for (slot, v) in slots {
        if hidden && slot == "child" {
            let weak = lua.weak();
            let keep = keep.clone();
            let win = w
                .downcast_ref()
                .expect("only a Kind::Window can start hidden");
            hw_ui::windows::defer_content(win, move |win| {
                let Some(lua) = weak.try_upgrade() else {
                    return;
                };
                // May run inside a signal notification (`visible` turning true).
                let res = hw_core::reactive::untracked(|| {
                    bind_slot(&lua, win.upcast_ref(), kind, "child", &v, &keep)
                });
                if let Err(e) = res {
                    log_lua_error(&lua, &format!("window '{}'", win.widget_name()), &e);
                }
            });
            continue;
        }
        bind_slot(lua, w, kind, &slot, &v, &keep)?;
    }
    if let Some(v) = popover {
        attach_popover(lua, w, &v)?;
    }
    // After children so e.g. Stack `shown` can find its page.
    for (k, v) in &props {
        bind_prop(w, kind, k, v, &keep)?;
    }
    for (name, f) in events {
        connect(lua, w, kind, &name, f)?;
    }
    if let Some(f) = draw {
        let weak = lua.weak();
        widgets::set_draw_func(
            w,
            Rc::new(move |cr, width, height| {
                let Some(lua) = weak.try_upgrade() else {
                    return;
                };
                if let Err(e) = f.call::<()>((Cairo(cr.clone()), width, height)) {
                    log_lua_error(&lua, "draw", &e);
                }
            }),
        )
        .map_err(rt_err)?;
    }
    if let Some(v) = css {
        bind_css(lua, w, &v, &keep, |v| Ok(lua_to_prop(v).to_text()))?;
    }
    if let Some(v) = theme {
        let weak = lua.weak();
        bind_css(lua, w, &v, &keep, move |v| {
            let lua = weak.try_upgrade().ok_or_else(|| rt_err("lua gone"))?;
            let j = hw_core::options::expand_keys(&lua_to_json(&lua, v.clone())?);
            Ok(hw_core::options::css_decls(&j))
        })?;
    }
    if let Some(f) = setup {
        RtState::spawn(
            lua,
            &f,
            LuaWidget {
                widget: w.clone(),
                kind: Some(kind),
            },
        );
    }
    if let Some(v) = visible {
        bind_prop(w, kind, "visible", &v, &keep)?;
    } else if kind == Kind::Window {
        w.downcast_ref::<gtk4::Window>()
            .expect("a Kind::Window widget is a gtk4::Window")
            .present();
    }
    let _ = st;
    Ok(())
}

fn columns(t: &Table) -> Result<Option<i32>> {
    t.get::<Option<i32>>("columns")
}

fn build_children(lua: &Lua, list: &[Value]) -> Result<Vec<Child>> {
    let mut out = Vec::new();
    for v in list {
        let Some(widget) = build(lua, v)? else {
            continue;
        };
        let (mut name, mut cell) = (None, None);
        if let Value::Table(ct) = v {
            name = ct.get::<Option<String>>("name")?;
            if let (Some(c), Some(r)) =
                (ct.get::<Option<i32>>("col")?, ct.get::<Option<i32>>("row")?)
            {
                let cs = ct.get::<Option<i32>>("col_span")?.unwrap_or(1);
                let rs = ct.get::<Option<i32>>("row_span")?.unwrap_or(1);
                cell = Some((c, r, cs, rs));
            }
        }
        out.push(Child { widget, name, cell });
    }
    Ok(out)
}

/// `children = signal_of_list`: rebuilds all children on change.
fn bind_children(
    lua: &Lua,
    w: &gtk4::Widget,
    kind: Kind,
    t: &Table,
    fixed: Vec<Value>,
    sig: &LuaSignal,
    keep: &Keep,
) -> Result<()> {
    let cols = columns(t)?;
    let weak_lua = lua.weak();
    let weak_w = w.downgrade();
    let src = sig.clone();
    let rebuild = Rc::new(move || -> Result<()> {
        let (Some(lua), Some(w)) = (weak_lua.try_upgrade(), weak_w.upgrade()) else {
            return Ok(());
        };
        let mut list = fixed.clone();
        if let Value::Table(lt) = hw_core::reactive::untracked(|| src.0.get(&lua))? {
            list.extend(lt.sequence_values::<Value>().collect::<Result<Vec<_>>>()?);
        }
        widgets::clear_children(&w, kind);
        let built = build_children(&lua, &list)?;
        widgets::add_children(&w, kind, built, cols).map_err(rt_err)
    });
    rebuild()?;
    keep.push(sig.0.subscribe(Box::new(move || {
        if let Err(e) = rebuild() {
            tracing::error!(target: "lua", "children: {e}");
        }
    })));
    Ok(())
}

fn bind_slot(
    lua: &Lua,
    w: &gtk4::Widget,
    kind: Kind,
    slot: &str,
    v: &Value,
    keep: &Keep,
) -> Result<()> {
    if let Some(sig) = as_signal(v) {
        // A signal in a slot: rebuild the slot's child on change.
        let t = lua.create_table()?;
        t.set("type", "With")?;
        t.set("signal", sig)?;
        let child = build_with(lua, &t)?;
        return widgets::set_slot(w, kind, slot, Some(&child)).map_err(rt_err);
    }
    let child = build(lua, v)?;
    widgets::set_slot(w, kind, slot, child.as_ref()).map_err(rt_err)?;
    let _ = keep;
    Ok(())
}

/// `popover = W.Popover{…}` on a widget other than `MenuButton`: the popover
/// is parented to the widget (open it with its `open` prop) and unparented
/// with it.
fn attach_popover(lua: &Lua, w: &gtk4::Widget, v: &Value) -> Result<()> {
    let Some(child) = build(lua, v)? else {
        return Ok(());
    };
    let p = child
        .downcast::<gtk4::Popover>()
        .map_err(|_| rt_err("`popover` must be a W.Popover"))?;
    p.set_parent(w);
    w.connect_destroy(move |_| p.unparent());
    Ok(())
}

fn bind_css(
    lua: &Lua,
    w: &gtk4::Widget,
    v: &Value,
    keep: &Keep,
    to_decls: impl Fn(&Value) -> Result<String> + 'static,
) -> Result<()> {
    let st = RtState::get(lua)?;
    let Some(ui) = &st.core.ui else { return Ok(()) };
    let styles = Rc::clone(&ui.styles);
    let holder: Rc<RefCell<Option<hw_ui::style::InlineCss>>> = Rc::default();
    keep.push(Rc::clone(&holder));
    let apply = {
        let (styles, holder, weak) = (styles, holder, w.downgrade());
        move |v: &Value| -> Result<()> {
            let Some(w) = weak.upgrade() else {
                return Ok(());
            };
            let decls = to_decls(v)?;
            // Drop the old rule first so classes don't accumulate.
            holder.borrow_mut().take();
            *holder.borrow_mut() = Some(styles.set_inline(&w, &decls));
            Ok(())
        }
    };
    match as_signal(v) {
        Some(sig) => {
            let weak = lua.weak();
            let src = sig.clone();
            let get = move || -> Result<Value> {
                let lua = weak.try_upgrade().ok_or_else(|| rt_err("lua gone"))?;
                hw_core::reactive::untracked(|| src.0.get(&lua))
            };
            apply(&get()?)?;
            let apply = Rc::new(apply);
            keep.push(sig.0.subscribe(Box::new(move || {
                if let Err(e) = get().and_then(|v| apply(&v)) {
                    tracing::warn!("css: {e}");
                }
            })));
        }
        None => apply(v)?,
    }
    Ok(())
}

/// `hw.With(signal, fn)`: a box whose single child is rebuilt from
/// `fn(value)` (or the value itself) on change.
fn build_with(lua: &Lua, t: &Table) -> Result<gtk4::Widget> {
    let sig = as_signal(&t.get::<Value>("signal")?)
        .ok_or_else(|| rt_err("hw.With: first argument must be a signal"))?;
    let f: Option<Function> = t.get("fn")?;
    let w = widgets::create(Kind::Box);
    w.add_css_class("hw-with");
    let keep = Keep::for_widget(&w);
    let weak_lua = lua.weak();
    let weak_w = w.downgrade();
    let src = sig.clone();
    let rebuild = move || -> Result<()> {
        let (Some(lua), Some(w)) = (weak_lua.try_upgrade(), weak_w.upgrade()) else {
            return Ok(());
        };
        let v = hw_core::reactive::untracked(|| src.0.get(&lua))?;
        let v = match &f {
            Some(f) => f.call::<Value>(v)?,
            None => v,
        };
        widgets::clear_children(&w, Kind::Box);
        if let Some(child) = build(&lua, &v)? {
            widgets::add_children(
                &w,
                Kind::Box,
                vec![Child {
                    widget: child,
                    name: None,
                    cell: None,
                }],
                None,
            )
            .map_err(rt_err)?;
        }
        Ok(())
    };
    rebuild()?;
    keep.push(sig.0.subscribe(Box::new(move || {
        if let Err(e) = rebuild() {
            tracing::error!(target: "lua", "hw.With: {e}");
        }
    })));
    Ok(w)
}

/// `hw.For(list_signal, fn, key_fn)`: keyed list; widgets of items whose
/// key survives are reused.
fn build_for(lua: &Lua, t: &Table) -> Result<gtk4::Widget> {
    let sig = as_signal(&t.get::<Value>("each")?)
        .ok_or_else(|| rt_err("hw.For: first argument must be a signal"))?;
    let f: Function = t.get("fn")?;
    let key_fn: Option<Function> = t.get("key")?;
    let w = widgets::create(Kind::Box);
    w.add_css_class("hw-for");
    let keep = Keep::for_widget(&w);
    for (k, v) in t.pairs::<String, Value>().flatten() {
        if !matches!(k.as_str(), "type" | "each" | "fn" | "key") {
            bind_prop(&w, Kind::Box, &k, &v, &keep)?;
        }
    }
    let cache: Rc<RefCell<HashMap<String, gtk4::Widget>>> = Rc::default();
    let weak_lua = lua.weak();
    let weak_w = w.downgrade();
    let src = sig.clone();
    let rebuild = move || -> Result<()> {
        let (Some(lua), Some(w)) = (weak_lua.try_upgrade(), weak_w.upgrade()) else {
            return Ok(());
        };
        let Value::Table(items) = hw_core::reactive::untracked(|| src.0.get(&lua))? else {
            widgets::clear_children(&w, Kind::Box);
            cache.borrow_mut().clear();
            return Ok(());
        };
        let mut next = HashMap::new();
        let mut order = Vec::new();
        for (i, item) in items.sequence_values::<Value>().enumerate() {
            let item = item?;
            let key = match &key_fn {
                Some(kf) => crate::convert::display(&lua, &kf.call::<Value>(item.clone())?),
                None => match &item {
                    Value::String(_) | Value::Integer(_) | Value::Number(_) | Value::Boolean(_) => {
                        crate::convert::display(&lua, &item)
                    }
                    _ => format!("#{i}"),
                },
            };
            let widget = match cache.borrow_mut().remove(&key) {
                Some(old) => old,
                None => match build(
                    &lua,
                    &f.call::<Value>((item, crate::convert::lua_index(i)?))?,
                )? {
                    Some(nw) => nw,
                    None => continue,
                },
            };
            order.push(widget.clone());
            next.insert(key, widget);
        }
        widgets::clear_children(&w, Kind::Box);
        let children = order
            .into_iter()
            .map(|widget| Child {
                widget,
                name: None,
                cell: None,
            })
            .collect();
        widgets::add_children(&w, Kind::Box, children, None).map_err(rt_err)?;
        *cache.borrow_mut() = next;
        Ok(())
    };
    rebuild()?;
    keep.push(sig.0.subscribe(Box::new(move || {
        if let Err(e) = rebuild() {
            tracing::error!(target: "lua", "hw.For: {e}");
        }
    })));
    Ok(w)
}

/// Builds a top-level `W.Window{}` and registers it. Returns its name.
pub fn build_window(lua: &Lua, t: &Table) -> Result<String> {
    let st = RtState::get(lua)?;
    let ui = st.core.ui()?;
    let ty: Option<String> = t.get("type")?;
    if ty.as_deref() != Some("Window") {
        return Err(rt_err(format!(
            "expected W.Window, got {}",
            ty.as_deref().unwrap_or("a plain table")
        )));
    }
    let name: String = match t.get::<Option<String>>("name")? {
        Some(n) => n,
        None => t
            .get::<Option<String>>("namespace")?
            .unwrap_or_else(|| st.next_window_name()),
    };
    let namespace: String = t
        .get::<Option<String>>("namespace")?
        .unwrap_or_else(|| name.clone());
    let win = widgets::create_window(&ui.app);
    let w: gtk4::Widget = win.clone().upcast();
    w.add_css_class(&widgets::kind_class(Kind::Window));
    w.set_widget_name(&name);
    widgets::set_prop(&w, Kind::Window, "namespace", &Prop::Str(namespace.clone()))
        .map_err(rt_err)?;
    ui.windows.add(&name, win.upcast_ref());
    if let Err(e) = configure(lua, &w, Kind::Window, t) {
        ui.windows.remove(&name);
        return Err(e);
    }
    st.core.ensure_layer_rules(&namespace);
    Ok(name)
}

/// `hw.layout(path)`: loads a JSON node tree as widget tables. Values of
/// the form `{"$bind": "settings.<key>"}` / `{"$bind": "option.<name>"}`
/// become signals.
pub fn load_layout(lua: &Lua, path: &str) -> Result<Value> {
    let st = RtState::get(lua)?;
    let p = st.core.dir.path(path);
    let text = std::fs::read_to_string(&p).map_err(|e| rt_err(format!("{}: {e}", p.display())))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| rt_err(format!("{}: {e}", p.display())))?;
    json_layout_to_lua(lua, &json)
}

fn bind_source(lua: &Lua, spec: &str) -> Result<Value> {
    let st = RtState::get(lua)?;
    let sig = if let Some(key) = spec.strip_prefix("settings.") {
        let s = Rc::clone(&st.core.settings);
        let k = key.to_owned();
        let setter: crate::signal::JsonSetter = Rc::new(move |v| s.set(&k, v).map(|_| ()));
        LuaSignal::json(st.core.settings.signal(key), Some(setter))
    } else if let Some(name) = spec
        .strip_prefix("option.")
        .or_else(|| spec.strip_prefix("opt."))
    {
        LuaSignal::json(st.core.options.signal(name), None)
    } else {
        return Err(rt_err(format!(
            "$bind: unknown source '{spec}' (use settings.<key> or option.<name>)"
        )));
    };
    Ok(Value::UserData(lua.create_userdata(sig)?))
}

pub fn json_layout_to_lua(lua: &Lua, v: &serde_json::Value) -> Result<Value> {
    use serde_json::Value as J;
    match v {
        J::Object(o) if o.len() == 1 && o.contains_key("$bind") => match &o["$bind"] {
            J::String(s) => bind_source(lua, s),
            other => Err(rt_err(format!("$bind must be a string, got {other}"))),
        },
        J::Object(o) => {
            let t = lua.create_table()?;
            for (k, v) in o {
                if k == "$bind" {
                    // Node-level form: {"$bind": {"label": "settings.x"}}
                    if let J::Object(binds) = v {
                        for (prop, spec) in binds {
                            let spec = spec
                                .as_str()
                                .ok_or_else(|| rt_err("$bind values must be strings"))?;
                            t.set(prop.as_str(), bind_source(lua, spec)?)?;
                        }
                    }
                    continue;
                }
                t.set(k.as_str(), json_layout_to_lua(lua, v)?)?;
            }
            Ok(Value::Table(t))
        }
        J::Array(a) => {
            let t = lua.create_table()?;
            for v in a {
                t.raw_push(json_layout_to_lua(lua, v)?)?;
            }
            Ok(Value::Table(t))
        }
        other => json_to_lua(lua, other),
    }
}

trait FromLuaValue: Sized {
    fn from_lua_value(v: Value) -> Result<Self>;
}

impl FromLuaValue for Function {
    fn from_lua_value(v: Value) -> Result<Self> {
        match v {
            Value::Function(f) => Ok(f),
            other => Err(rt_err(format!(
                "expected a function, got {}",
                other.type_name()
            ))),
        }
    }
}
