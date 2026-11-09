//! One Lua state running `hyprwidget.lua` and the `hw.*` API.
//!
//! [`Core`] survives Lua reloads (options, settings, styles, windows
//! registry, monitors); [`Runtime`] is recreated on every reload and owns
//! everything Lua started (timers, subscriptions, processes, windows).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use hw_core::config::{AppConfig, ConfigDir};
use hw_core::options::{self, Options};
use hw_core::reactive::{Signal, SourceGuard, poll, untracked};
use hw_core::settings::Settings;
use hw_core::{Subscription, hyprland};
use hw_ui::gtk4::{self, gio, glib, prelude::*};
use hw_ui::monitors::MonitorInfo;
use hw_ui::style::Styles;
use hw_ui::widgets::Kind;
use hw_ui::windows::Windows;
use mlua::{
    Function, Lua, MetaMethod, MultiValue, Result, Table, UserData, UserDataMethods, Value,
    Variadic,
};
use serde_json::json;

use crate::builder;
use crate::convert::{display, json_to_lua, lua_to_json};
use crate::monitor::LuaMonitor;
use crate::signal::{JsonSetter, LuaSignal, MonitorsSource, as_signal, call_value};

const PRELUDE: &str = include_str!("prelude.lua");

/// Widget option for the `ignore_alpha` layer rule of our namespaces
/// (`hw.config({ blur = { ignore_alpha = 0.3 } })`, `false` = no rule).
const IGNORE_ALPHA_OPTION: &str = "blur:ignore_alpha";
/// Same value `~/.config/hypr/config/layerrule.lua` used for the AGS panels:
/// low enough that translucent backgrounds still get blurred.
const DEFAULT_IGNORE_ALPHA: f64 = 0.1;

/// GTK side; absent in headless mode (`check`, offline `option`).
pub struct Ui {
    pub app: gtk4::Application,
    pub styles: Rc<Styles>,
    pub windows: Windows,
    pub monitors: Signal<Vec<MonitorInfo>>,
}

/// State that survives Lua reloads.
pub struct Core {
    pub dir: ConfigDir,
    pub app_cfg: RefCell<AppConfig>,
    pub options: Options,
    pub settings: Rc<Settings>,
    pub ui: Option<Ui>,
    /// Namespaces whose Hyprland layer rules are registered.
    layer_ns: RefCell<HashSet<String>>,
    /// Every namespace we created a window with.
    all_ns: RefCell<HashSet<String>>,
    /// `hw.layer_rule{}` calls of the current Lua config (as Lua source).
    pub extra_rules: RefCell<Vec<String>>,
    /// `hw.service.*`, started on first use.
    pub services: crate::service::Services,
    /// Generated SCSS partials from `hw.scss(name, text)`.
    pub scss_partials: RefCell<std::collections::BTreeMap<String, String>>,
    /// Selected theme and its colors (`hw.theme`).
    pub theme: crate::theme::Theme,
}

impl Core {
    /// `inherit_hypr`: read the running Hyprland's options (off in tests).
    pub fn new(dir: ConfigDir, ui: Option<Ui>, inherit_hypr: bool) -> anyhow::Result<Rc<Self>> {
        let app_cfg = dir.load_app()?;
        let settings = Settings::load(app_cfg.settings_file.clone())?;
        let options = Options::new();
        if inherit_hypr && hyprland::available() {
            match hyprland::fetch_tree() {
                Ok(t) => options.set_hypr(t),
                Err(e) => tracing::warn!("reading Hyprland options: {e:#}"),
            }
        }
        Ok(Rc::new(Self {
            dir,
            app_cfg: RefCell::new(app_cfg),
            options,
            settings,
            ui,
            layer_ns: RefCell::default(),
            all_ns: RefCell::default(),
            extra_rules: RefCell::default(),
            services: Default::default(),
            scss_partials: Default::default(),
            theme: Default::default(),
        }))
    }

    pub fn ui(&self) -> Result<&Ui> {
        self.ui
            .as_ref()
            .ok_or_else(|| mlua::Error::runtime("not available in headless mode"))
    }

    /// Re-reads Hyprland's options (after `configreloaded`).
    pub fn refresh_hypr(&self) {
        match hyprland::fetch_tree() {
            Ok(t) => self.options.set_hypr(t),
            Err(e) => tracing::warn!("reading Hyprland options: {e:#}"),
        }
    }

    /// Registers blur layer rules for `ns` once per Hyprland config load
    /// (PLAN.md §3.6), following `decoration:blur:*`.
    pub fn ensure_layer_rules(&self, ns: &str) {
        self.all_ns.borrow_mut().insert(ns.to_owned());
        if self.ui.is_none()
            || !self.app_cfg.borrow().layer_rules
            || !hyprland::available()
            || self.layer_ns.borrow().contains(ns)
        {
            return;
        }
        if self.options.get("decoration:blur:enabled") != json!(true) {
            return;
        }
        let popups = self.options.get("decoration:blur:popups") == json!(true);
        // `ignore_alpha` on the layer itself (Hyprland's
        // `popups_ignorealpha` only covers popups and is usually close to
        // 1, which would leave translucent panels unblurred).
        let alpha = match self.options.get(IGNORE_ALPHA_OPTION) {
            serde_json::Value::Bool(false) => None,
            v => Some(v.as_f64().unwrap_or(DEFAULT_IGNORE_ALPHA)),
        };
        for rule in hyprland::blur_rules(ns, popups, alpha) {
            if let Err(e) = hyprland::eval(&rule) {
                tracing::warn!("layer rule for '{ns}': {e:#}");
                return;
            }
        }
        tracing::debug!(ns, "layer rules registered");
        self.layer_ns.borrow_mut().insert(ns.to_owned());
    }

    /// Hyprland dropped dynamic rules (config reload): register them again.
    pub fn reapply_layer_rules(&self) {
        self.layer_ns.borrow_mut().clear();
        let all: Vec<String> = self.all_ns.borrow().iter().cloned().collect();
        for ns in all {
            self.ensure_layer_rules(&ns);
        }
        for rule in self.extra_rules.borrow().iter() {
            if let Err(e) = hyprland::eval(rule) {
                tracing::warn!("hw.layer_rule: {e:#}");
            }
        }
    }
}

/// Windows created by a `per_monitor` function for one monitor.
type MonitorWindows = (gtk4::gdk::Monitor, Vec<String>);

/// Per-Lua-state data, reachable from Lua callbacks through app data.
pub struct RtState {
    pub core: Rc<Core>,
    pub(crate) user_opts: RefCell<serde_json::Value>,
    resolvers: RefCell<Vec<(Vec<String>, Function)>>,
    /// `file:line: message` found while loading (`hyprwidget check`).
    pub diagnostics: RefCell<Vec<String>>,
    /// Option names set by the config with their location.
    pub option_locs: RefCell<Vec<(String, String)>>,
    requests: RefCell<HashMap<String, Function>>,
    per_monitor: RefCell<Vec<Function>>,
    pm_windows: RefCell<HashMap<(usize, String), MonitorWindows>>,
    kept: RefCell<HashMap<u64, Box<dyn Any>>>,
    next_id: Cell<u64>,
    procs: RefCell<Vec<gio::Subprocess>>,
    pub defined: RefCell<HashMap<String, Function>>,
    loading: Cell<bool>,
    window_seq: Cell<u64>,
}

impl RtState {
    pub fn get(lua: &Lua) -> Result<Rc<RtState>> {
        lua.app_data_ref::<Rc<RtState>>()
            .map(|r| r.clone())
            .ok_or_else(|| mlua::Error::runtime("hyprwidget runtime missing"))
    }

    /// Stores `thing` until the runtime is dropped or [`Self::unkeep`].
    pub fn keep(&self, thing: Box<dyn Any>) -> u64 {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.kept.borrow_mut().insert(id, thing);
        id
    }

    pub fn unkeep(&self, id: u64) {
        let thing = self.kept.borrow_mut().remove(&id);
        drop(thing);
    }

    /// Runs `f(args)` in a coroutine (so `hw.exec_async` may yield) and
    /// returns its result if it finished synchronously. Errors are logged.
    pub fn spawn(lua: &Lua, f: &Function, args: impl mlua::IntoLuaMulti) -> Value {
        let run = || -> Result<Value> {
            let spawn: Function = lua.named_registry_value("hw.spawn")?;
            let mut a = args.into_lua_multi(lua)?;
            a.push_front(Value::Function(f.clone()));
            spawn.call::<Value>(a)
        };
        run().unwrap_or_else(|e| {
            log_lua_error(lua, "callback", &e);
            Value::Nil
        })
    }

    pub fn next_window_name(&self) -> String {
        let n = self.window_seq.get();
        self.window_seq.set(n + 1);
        format!("window-{n}")
    }
}

pub fn log_lua_error(_lua: &Lua, ctx: &str, e: &mlua::Error) {
    tracing::error!(target: "lua", "{ctx}: {e}");
}

/// Location of the Lua code calling the current Rust function, as
/// `path:line` with the path relative to the config dir.
fn caller(lua: &Lua) -> String {
    let root = RtState::get(lua).map(|st| st.core.dir.root.clone()).ok();
    lua.inspect_stack(1, |d| {
        let src = d.source();
        let path = match src.source.as_deref().and_then(|s| s.strip_prefix('@')) {
            Some(p) => {
                let p = std::path::Path::new(p);
                root.as_deref()
                    .and_then(|r| p.strip_prefix(r).ok())
                    .unwrap_or(p)
                    .display()
                    .to_string()
            }
            None => src.short_src.map(|s| s.to_string()).unwrap_or_default(),
        };
        match d.current_line() {
            Some(l) => format!("{path}:{l}"),
            None => path,
        }
    })
    .unwrap_or_else(|| "?".into())
}

/// Drops `stack traceback:` sections from an mlua error message.
pub fn compact_error(msg: &str) -> String {
    let mut out = Vec::new();
    let mut in_tb = false;
    for line in msg.lines() {
        if line.trim_start().starts_with("stack traceback:") {
            in_tb = true;
            continue;
        }
        if in_tb && (line.starts_with('\t') || line.starts_with("  ")) {
            continue;
        }
        in_tb = false;
        if !line.trim().is_empty() {
            out.push(line.trim_end());
        }
    }
    out.join("\n")
}

/// A loaded `hyprwidget.lua`.
pub struct Runtime {
    pub lua: Lua,
    pub st: Rc<RtState>,
}

impl Runtime {
    pub fn new(core: Rc<Core>) -> anyhow::Result<Self> {
        let lua = Lua::new();
        let st = Rc::new(RtState {
            core,
            user_opts: RefCell::new(json!({})),
            resolvers: RefCell::default(),
            diagnostics: RefCell::default(),
            option_locs: RefCell::default(),
            requests: RefCell::default(),
            per_monitor: RefCell::default(),
            pm_windows: RefCell::default(),
            kept: RefCell::default(),
            next_id: Cell::new(0),
            procs: RefCell::default(),
            defined: RefCell::default(),
            loading: Cell::new(true),
            window_seq: Cell::new(0),
        });
        lua.set_app_data(st.clone());
        install(&lua, &st).map_err(|e| anyhow::anyhow!("installing hw API: {e}"))?;
        Ok(Self { lua, st })
    }

    /// Runs the entry file, applies options and creates windows.
    pub fn load(&self) -> anyhow::Result<()> {
        let entry = self.st.core.dir.entry();
        let res = (|| -> Result<()> {
            // Theme first (plugin styles use its colors), then plugins so
            // their widgets exist for the config.
            crate::theme::load(&self.lua)?;
            crate::plugin::load_all(&self.lua)?;
            let chunk = self
                .lua
                .load(std::fs::read_to_string(&entry).map_err(mlua::Error::external)?)
                .set_name(format!("@{}", entry.display()));
            let ret: Value = chunk.call(())?;
            self.st.loading.set(false);
            self.apply_options()?;
            self.handle_entry_return(ret)?;
            self.start_per_monitor()?;
            Ok(())
        })();
        self.st.loading.set(false);
        res.map_err(|e| anyhow::anyhow!("{e}"))
    }

    /// The entry may `return` a window, a list of windows, or a
    /// `function(monitor)` (shorthand for `hw.per_monitor`).
    fn handle_entry_return(&self, ret: Value) -> Result<()> {
        match ret {
            Value::Function(f) => self.st.per_monitor.borrow_mut().push(f),
            Value::Table(t) if t.contains_key("type")? => {
                if self.st.core.ui.is_some() {
                    builder::build_window(&self.lua, &t)?;
                }
            }
            Value::Table(t) => {
                for v in t.sequence_values::<Value>() {
                    self.handle_entry_return(v?)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Pushes the user option layer (config.json, then `hw.config` calls)
    /// into the option store.
    pub fn apply_options(&self) -> Result<()> {
        apply_options(&self.lua, &self.st)
    }

    fn start_per_monitor(&self) -> Result<()> {
        let Some(ui) = &self.st.core.ui else {
            return Ok(());
        };
        sync_monitors(&self.lua)?;
        let weak = self.lua.weak();
        let sub = ui.monitors.subscribe(move |_| {
            if let Some(lua) = weak.try_upgrade()
                && let Err(e) = sync_monitors(&lua)
            {
                log_lua_error(&lua, "per_monitor", &e);
            }
        });
        self.st.keep(Box::new(sub));
        Ok(())
    }

    /// `hyprwidget eval`.
    pub fn eval(&self, code: &str) -> anyhow::Result<String> {
        let chunk = self.lua.load(code).set_name("=eval");
        // Expressions first (`1 + 1`), then statements.
        let res: Result<MultiValue> = match self
            .lua
            .load(format!("return {code}"))
            .set_name("=eval")
            .into_function()
        {
            Ok(f) => f.call(()),
            Err(_) => chunk.call(()),
        };
        let vals = res.map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(vals
            .into_iter()
            .map(|v| display(&self.lua, v))
            .collect::<Vec<_>>()
            .join("\t"))
    }

    /// Calls a `hw.on_request` handler; `reply(ok, text)` may come later.
    pub fn request(&self, name: &str, args: &[String], reply: impl FnOnce(bool, String) + 'static) {
        let Some(f) = self.st.requests.borrow().get(name).cloned() else {
            reply(false, format!("no request handler '{name}'"));
            return;
        };
        let reply = RefCell::new(Some(reply));
        let res = (|| -> Result<()> {
            let lua = &self.lua;
            let reply_fn = lua.create_function(move |lua, (ok, v): (bool, Value)| {
                if let Some(r) = reply.borrow_mut().take() {
                    let text = match v {
                        Value::Nil => String::new(),
                        v => display(lua, v),
                    };
                    r(ok, text);
                }
                Ok(())
            })?;
            let dispatch: Function = lua.named_registry_value("hw.request")?;
            let args_t = lua.create_sequence_from(args.iter().map(String::as_str))?;
            dispatch.call::<()>((f, args_t, reply_fn))
        })();
        if let Err(e) = res {
            log_lua_error(&self.lua, "request", &e);
        }
    }

    pub fn request_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.st.requests.borrow().keys().cloned().collect();
        v.sort();
        v
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        for p in self.st.procs.borrow_mut().drain(..) {
            p.force_exit();
        }
        self.st.kept.borrow_mut().clear();
        if let Some(ui) = &self.st.core.ui {
            ui.windows.clear();
        }
        self.st.core.extra_rules.borrow_mut().clear();
    }
}

/// Pushes the user option layer (config.json, then `hw.config` calls) into
/// the option store.
pub(crate) fn apply_options(lua: &Lua, st: &RtState) -> Result<()> {
    let json_opts = st.core.app_cfg.borrow().options.clone();
    let mut user = normalize_tree(
        &json_opts,
        &mut Vec::new(),
        "config.json",
        &mut st.diagnostics.borrow_mut(),
    );
    options::deep_merge(&mut user, &st.user_opts.borrow());
    let weak = lua.weak();
    let resolvers = st
        .resolvers
        .borrow()
        .iter()
        .map(|(path, f)| {
            let (weak, f) = (weak.clone(), f.clone());
            let r: options::Resolver = Box::new(move |base| {
                let lua = weak.try_upgrade()?;
                let h = json_to_lua(&lua, base).ok()?;
                let v = call_value(&weak, &f, h);
                lua_to_json(&lua, v).ok().filter(|v| !v.is_null())
            });
            (path.clone(), r)
        })
        .collect();
    st.core.options.set_user(user, resolvers);
    Ok(())
}

/// Stores and writes the generated partial `_<name>.scss`, recompiling the
/// stylesheet when it changed (after loading).
pub(crate) fn set_partial(lua: &Lua, name: &str, text: &str) -> Result<()> {
    let st = RtState::get(lua)?;
    st.core
        .scss_partials
        .borrow_mut()
        .insert(name.to_owned(), text.to_owned());
    if let Some(ui) = &st.core.ui {
        let changed = ui
            .styles
            .write_partial(name, text)
            .map_err(|e| mlua::Error::runtime(format!("{e:#}")))?;
        // While loading, the stylesheet is compiled right after.
        if changed
            && !st.loading.get()
            && let Err(e) = ui.styles.reload_user()
        {
            tracing::error!("stylesheet: {e:#}");
        }
    }
    Ok(())
}

/// Normalizes known options of a JSON tree (colors → CSS…), recording
/// errors as diagnostics at `loc`.
fn normalize_tree(
    v: &serde_json::Value,
    path: &mut Vec<String>,
    loc: &str,
    diags: &mut Vec<String>,
) -> serde_json::Value {
    match v {
        serde_json::Value::Object(o)
            if options::inherited(&options::path_to_name(path)).is_none() =>
        {
            let mut out = serde_json::Map::new();
            for (k, v) in o {
                path.push(k.clone());
                out.insert(k.clone(), normalize_tree(v, path, loc, diags));
                path.pop();
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::String(s) if s == "inherit" => v.clone(),
        _ => match options::normalize(&options::path_to_name(path), v) {
            Ok(n) => n,
            Err(e) => {
                diags.push(format!("{loc}: {e}"));
                json!("inherit")
            }
        },
    }
}

/// Creates/destroys `per_monitor` windows to match the monitor list.
fn sync_monitors(lua: &Lua) -> Result<()> {
    let st = RtState::get(lua)?;
    let ui = st.core.ui()?;
    let mons = ui.monitors.peek();
    let fns: Vec<Function> = st.per_monitor.borrow().clone();
    for (idx, f) in fns.iter().enumerate() {
        // Drop windows of monitors that went away or were replaced.
        let stale: Vec<(usize, String)> = st
            .pm_windows
            .borrow()
            .iter()
            .filter(|((i, conn), (mon, _))| {
                *i == idx
                    && !mons
                        .iter()
                        .any(|m| &m.connector == conn && &m.monitor == mon)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for key in stale {
            if let Some((_, names)) = st.pm_windows.borrow_mut().remove(&key) {
                for n in names {
                    ui.windows.remove(&n);
                }
            }
        }
        for m in &mons {
            let key = (idx, m.connector.clone());
            if st.pm_windows.borrow().contains_key(&key) {
                continue;
            }
            let ret: Value = f.call(LuaMonitor(m.clone()))?;
            let mut names = Vec::new();
            collect_windows(lua, ret, &mut names)?;
            tracing::info!(monitor = %m.connector, windows = ?names, "per_monitor");
            st.pm_windows
                .borrow_mut()
                .insert(key, (m.monitor.clone(), names));
        }
    }
    Ok(())
}

/// Destroys and rebuilds the `per_monitor` windows (after a plugin was
/// reloaded) without re-running the config.
pub fn rebuild_monitor_windows(lua: &Lua) -> Result<()> {
    let st = RtState::get(lua)?;
    let Some(ui) = &st.core.ui else {
        return Ok(());
    };
    let old: Vec<MonitorWindows> = st.pm_windows.borrow_mut().drain().map(|(_, v)| v).collect();
    for (_, names) in old {
        for n in names {
            ui.windows.remove(&n);
        }
    }
    sync_monitors(lua)
}

fn collect_windows(lua: &Lua, v: Value, names: &mut Vec<String>) -> Result<()> {
    match v {
        Value::Table(t) if t.contains_key("type")? => names.push(builder::build_window(lua, &t)?),
        Value::Table(t) => {
            for v in t.sequence_values::<Value>() {
                collect_windows(lua, v?, names)?;
            }
        }
        Value::Nil => {}
        other => {
            return Err(mlua::Error::runtime(format!(
                "per_monitor must return windows, got {}",
                other.type_name()
            )));
        }
    }
    Ok(())
}

/// `hw.opt.general.gaps_out`: path proxy; leaves are signals.
#[derive(Clone)]
struct OptProxy(Vec<String>);

impl UserData for OptProxy {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            let mut path = this.0.clone();
            path.extend(options::name_to_path(&key));
            let st = RtState::get(lua)?;
            let name = options::path_to_name(&path);
            if st.core.options.get(&name).is_object() {
                return lua.create_userdata(OptProxy(path)).map(Value::UserData);
            }
            lua.create_userdata(LuaSignal::json(st.core.options.signal(&name), None))
                .map(Value::UserData)
        });
        // `hw.opt.decoration()` → current subtree as a table.
        m.add_meta_method(MetaMethod::Call, |lua, this, ()| {
            let st = RtState::get(lua)?;
            json_to_lua(lua, &st.core.options.get(&options::path_to_name(&this.0)))
        });
    }
}

/// `hw.settings`.
struct SettingsUd;

fn settings_signal(lua: &Lua, key: &str) -> Result<LuaSignal> {
    let st = RtState::get(lua)?;
    let s = st.core.settings.clone();
    let k = key.to_owned();
    let setter: JsonSetter = Rc::new(move |v| s.set(&k, v).map(|_| ()));
    Ok(LuaSignal::json(st.core.settings.signal(key), Some(setter)))
}

impl UserData for SettingsUd {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("get", |lua, _, key: String| {
            json_to_lua(lua, &RtState::get(lua)?.core.settings.get(&key))
        });
        m.add_method("set", |lua, _, (key, v): (String, Value)| {
            let st = RtState::get(lua)?;
            let stored = st
                .core
                .settings
                .set(&key, lua_to_json(lua, v)?)
                .map_err(|e| mlua::Error::runtime(format!("{e:#}")))?;
            json_to_lua(lua, &stored)
        });
        m.add_method("toggle", |lua, _, key: String| {
            let st = RtState::get(lua)?;
            let cur = st.core.settings.get(&key).as_bool().unwrap_or(false);
            st.core
                .settings
                .set(&key, json!(!cur))
                .map_err(|e| mlua::Error::runtime(format!("{e:#}")))?;
            Ok(!cur)
        });
        m.add_method("meta", |lua, _, key: String| {
            match RtState::get(lua)?.core.settings.meta(&key) {
                Some(v) => json_to_lua(lua, &v),
                None => Ok(Value::Nil),
            }
        });
        m.add_method("bind", |lua, _, key: String| settings_signal(lua, &key));
        m.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            settings_signal(lua, &key)
        });
    }
}

/// `hw.timeout` / `hw.interval` handle.
struct TimerHandle(u64);

impl UserData for TimerHandle {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("cancel", |lua, this, ()| {
            RtState::get(lua)?.unkeep(this.0);
            Ok(())
        });
    }
}

/// `hw.subprocess` handle.
struct ProcHandle(gio::Subprocess);

impl UserData for ProcHandle {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("kill", |_, this, ()| {
            this.0.force_exit();
            Ok(())
        });
        m.add_method("signal", |_, this, sig: i32| {
            this.0.send_signal(sig);
            Ok(())
        });
        m.add_method("write", |_, this, text: String| {
            if let Some(stdin) = this.0.stdin_pipe() {
                stdin
                    .write_all(text.as_bytes(), gio::Cancellable::NONE)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            }
            Ok(())
        });
        m.add_method("pid", |_, this, ()| {
            Ok(this.0.identifier().map(|s| s.to_string()))
        });
    }
}

fn argv(lua: &Lua, cmd: Value) -> Result<Vec<std::ffi::OsString>> {
    match cmd {
        Value::String(s) => glib::shell_parse_argv(s.to_str()?.as_ref())
            .map_err(|e| mlua::Error::runtime(format!("parsing command: {e}"))),
        Value::Table(t) => t
            .sequence_values::<String>()
            .map(|s| s.map(Into::into))
            .collect(),
        other => {
            let _ = lua;
            Err(mlua::Error::runtime(format!(
                "command must be a string or list, got {}",
                other.type_name()
            )))
        }
    }
}

fn spawn_process(
    argv: &[std::ffi::OsString],
    flags: gio::SubprocessFlags,
) -> Result<gio::Subprocess> {
    let refs: Vec<&std::ffi::OsStr> = argv.iter().map(|s| s.as_os_str()).collect();
    gio::Subprocess::newv(&refs, flags)
        .map_err(|e| mlua::Error::runtime(format!("{:?}: {e}", argv.first())))
}

/// `hw._exec(cmd, cb)`: runs `cmd` async, then `cb(stdout)` or
/// `cb(nil, error)`.
fn exec_cb(lua: &Lua, (cmd, cb): (Value, Function)) -> Result<()> {
    let argv = argv(lua, cmd)?;
    let proc = spawn_process(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE,
    )?;
    let weak = lua.weak();
    glib::spawn_future_local(async move {
        let res = proc.communicate_utf8_future(None).await;
        let Some(lua) = weak.try_upgrade() else {
            return;
        };
        let (out, err) = match res {
            Ok((out, err)) => {
                let out = out
                    .map(|s| s.trim_end_matches('\n').to_string())
                    .unwrap_or_default();
                let err = err.map(|s| s.trim().to_string()).unwrap_or_default();
                if proc.is_successful() {
                    (Some(out), None)
                } else {
                    let msg = if err.is_empty() {
                        format!("exit status {}", proc.exit_status())
                    } else {
                        err
                    };
                    (None, Some(msg))
                }
            }
            Err(e) => (None, Some(e.to_string())),
        };
        RtState::spawn(&lua, &cb, (out, err));
    });
    Ok(())
}

/// Installs `hw`, `W`-style helpers and globals.
fn install(lua: &Lua, st: &Rc<RtState>) -> Result<()> {
    let g = lua.globals();
    let root = st.core.dir.root.clone();
    let package: Table = g.get("package")?;
    package.set(
        "path",
        format!(
            "{0}/?.lua;{0}/?/init.lua;{1}",
            root.display(),
            package.get::<String>("path")?
        ),
    )?;
    g.set("HOME", std::env::var("HOME").unwrap_or_default())?;
    g.set("CONFIG_DIR", root.display().to_string())?;

    let hw = lua.create_table()?;

    // print → log
    g.set(
        "print",
        lua.create_function(|lua, args: Variadic<Value>| {
            let s: Vec<String> = args.into_iter().map(|v| display(lua, v)).collect();
            tracing::info!(target: "lua", "{}", s.join("\t"));
            Ok(())
        })?,
    )?;
    // `debug` isn't loaded (safe mode); minimal `debug.traceback` for xpcall.
    hw.set(
        "_traceback",
        lua.create_function(|lua, msg: Value| {
            let mut out = display(lua, msg);
            out.push_str("\nstack traceback:");
            for level in 1..16 {
                let Some(frame) = lua.inspect_stack(level, |d| {
                    let src = d
                        .source()
                        .short_src
                        .map(|s| s.to_string())
                        .unwrap_or_default();
                    (src, d.current_line())
                }) else {
                    break;
                };
                if frame.0 == "[C]" || frame.0.starts_with("hw.prelude") {
                    continue;
                }
                match frame.1 {
                    Some(l) => out.push_str(&format!("\n\t{}:{l}", frame.0)),
                    None => out.push_str(&format!("\n\t{}", frame.0)),
                }
            }
            Ok(out)
        })?,
    )?;
    hw.set(
        "_error",
        lua.create_function(|_, msg: String| {
            tracing::error!(target: "lua", "{msg}");
            Ok(())
        })?,
    )?;

    // Reactive
    hw.set(
        "state",
        lua.create_function(|_, v: Value| Ok(LuaSignal::state(v)))?,
    )?;
    hw.set(
        "computed",
        lua.create_function(|lua, f: Function| Ok(LuaSignal::computed(lua, f)))?,
    )?;
    hw.set(
        "_poll_fn",
        lua.create_function(|lua, (ms, f): (u64, Function)| {
            let weak = lua.weak();
            let sig = poll(Duration::from_millis(ms.max(1)), move || {
                call_value(&weak, &f, ())
            });
            Ok(LuaSignal(Rc::new(crate::signal::LuaValueSource(sig))))
        })?,
    )?;
    hw.set(
        "is_signal",
        lua.create_function(|_, v: Value| Ok(as_signal(&v).is_some()))?,
    )?;
    hw.set(
        "bind",
        lua.create_function(
            |lua, (obj, key): (Value, Option<String>)| -> Result<Value> {
                if let Some(s) = as_signal(&obj) {
                    return Ok(Value::UserData(lua.create_userdata(s)?));
                }
                if let Value::UserData(ud) = &obj
                    && ud.is::<SettingsUd>()
                {
                    let key = key.ok_or_else(|| {
                        mlua::Error::runtime("hw.bind(hw.settings, key): key missing")
                    })?;
                    return Ok(Value::UserData(
                        lua.create_userdata(settings_signal(lua, &key)?)?,
                    ));
                }
                if let (Value::Table(t), Some(k)) = (&obj, &key) {
                    let v: Value = t.get(k.as_str())?;
                    if as_signal(&v).is_some() {
                        return Ok(v);
                    }
                    if let Ok(Value::Function(b)) = t.get::<Value>("bind") {
                        return b.call((obj.clone(), k.as_str()));
                    }
                    return Ok(Value::UserData(lua.create_userdata(LuaSignal::state(v))?));
                }
                Err(mlua::Error::runtime(
                    "hw.bind: expected a signal, hw.settings or a table",
                ))
            },
        )?,
    )?;

    // Timers
    // hw.now() -> monotonic seconds (float), for durations / frame timing.
    hw.set(
        "now",
        lua.create_function(|_, ()| Ok(glib::monotonic_time() as f64 / 1e6))?,
    )?;
    hw.set(
        "timeout",
        lua.create_function(|lua, (ms, f): (u64, Function)| {
            let st = RtState::get(lua)?;
            let weak = lua.weak();
            let id_cell = Rc::new(Cell::new(None::<u64>));
            let idc = id_cell.clone();
            let src = glib::timeout_add_local_once(Duration::from_millis(ms), move || {
                let Some(lua) = weak.try_upgrade() else {
                    return;
                };
                if let (Ok(st), Some(id)) = (RtState::get(&lua), idc.get()) {
                    // The source is finished; dropping the guard is a no-op.
                    st.unkeep(id);
                }
                RtState::spawn(&lua, &f, ());
            });
            let id = st.keep(Box::new(SourceGuard(Some(src))));
            id_cell.set(Some(id));
            Ok(TimerHandle(id))
        })?,
    )?;
    hw.set(
        "interval",
        lua.create_function(|lua, (ms, f): (u64, Function)| {
            let st = RtState::get(lua)?;
            RtState::spawn(lua, &f, ());
            let weak = lua.weak();
            let src = glib::timeout_add_local(Duration::from_millis(ms.max(1)), move || {
                let Some(lua) = weak.try_upgrade() else {
                    return glib::ControlFlow::Break;
                };
                RtState::spawn(&lua, &f, ());
                glib::ControlFlow::Continue
            });
            Ok(TimerHandle(st.keep(Box::new(SourceGuard(Some(src))))))
        })?,
    )?;

    // Processes
    hw.set("_exec", lua.create_function(exec_cb)?)?;
    hw.set(
        "exec",
        lua.create_function(|lua, cmd: Value| {
            let argv = argv(lua, cmd)?;
            let (prog, rest) = argv
                .split_first()
                .ok_or_else(|| mlua::Error::runtime("empty command"))?;
            let out = std::process::Command::new(prog)
                .args(rest)
                .output()
                .map_err(|e| mlua::Error::runtime(format!("{prog:?}: {e}")))?;
            if !out.status.success() {
                return Err(mlua::Error::runtime(
                    String::from_utf8_lossy(&out.stderr).trim().to_string(),
                ));
            }
            Ok(String::from_utf8_lossy(&out.stdout)
                .trim_end_matches('\n')
                .to_string())
        })?,
    )?;
    hw.set(
        "subprocess",
        lua.create_function(
            |lua, (cmd, on_line, on_err): (Value, Option<Function>, Option<Function>)| {
                let st = RtState::get(lua)?;
                let argv = argv(lua, cmd)?;
                let proc = spawn_process(
                    &argv,
                    gio::SubprocessFlags::STDOUT_PIPE
                        | gio::SubprocessFlags::STDERR_PIPE
                        | gio::SubprocessFlags::STDIN_PIPE,
                )?;
                for (pipe, cb) in [(proc.stdout_pipe(), on_line), (proc.stderr_pipe(), on_err)] {
                    let (Some(pipe), Some(cb)) = (pipe, cb) else {
                        continue;
                    };
                    let weak = lua.weak();
                    let stream = gio::DataInputStream::new(&pipe);
                    glib::spawn_future_local(async move {
                        while let Ok(Some(line)) =
                            stream.read_line_utf8_future(glib::Priority::DEFAULT).await
                        {
                            let Some(lua) = weak.try_upgrade() else {
                                return;
                            };
                            RtState::spawn(&lua, &cb, line.to_string());
                        }
                    });
                }
                st.procs.borrow_mut().push(proc.clone());
                Ok(ProcHandle(proc))
            },
        )?,
    )?;

    // Options & settings
    hw.set(
        "option",
        lua.create_function(|lua, name: String| {
            let st = RtState::get(lua)?;
            Ok(LuaSignal::json(st.core.options.signal(&name), None))
        })?,
    )?;
    hw.set("opt", OptProxy(Vec::new()))?;
    hw.set("settings", SettingsUd)?;

    // hw.json.decode(text) / hw.json.encode(value [, pretty])
    let json = lua.create_table()?;
    json.set(
        "decode",
        lua.create_function(|lua, text: mlua::LuaString| {
            let v: serde_json::Value = serde_json::from_slice(&text.as_bytes())
                .map_err(|e| mlua::Error::runtime(format!("hw.json.decode: {e}")))?;
            json_to_lua(lua, &v)
        })?,
    )?;
    json.set(
        "encode",
        lua.create_function(|lua, (v, pretty): (Value, Option<bool>)| {
            let v = lua_to_json(lua, v)?;
            let out = if pretty.unwrap_or(false) {
                serde_json::to_string_pretty(&v)
            } else {
                serde_json::to_string(&v)
            };
            out.map_err(|e| mlua::Error::runtime(format!("hw.json.encode: {e}")))
        })?,
    )?;
    hw.set("json", json)?;
    hw.set("config", lua.create_function(hw_config)?)?;
    crate::custom::install(lua, &hw)?;
    hw.set(
        "layer_rule",
        lua.create_function(|lua, t: Table| {
            let st = RtState::get(lua)?;
            let code = format!("hl.layer_rule({})", lua_literal(&Value::Table(t), 0));
            if st.core.ui.is_some() && hyprland::available() {
                if let Err(e) = hyprland::eval(&code) {
                    st.diagnostics
                        .borrow_mut()
                        .push(format!("{}: {e:#}", caller(lua)));
                    tracing::warn!("hw.layer_rule: {e:#}");
                }
                st.core.extra_rules.borrow_mut().push(code);
            }
            Ok(())
        })?,
    )?;

    // Styles
    hw.set(
        "scss",
        lua.create_function(|lua, (name, text): (String, String)| {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                || name == "hypr"
            {
                return Err(mlua::Error::runtime(format!(
                    "hw.scss: invalid partial name '{name}'"
                )));
            }
            set_partial(lua, &name, &text)
        })?,
    )?;
    hw.set(
        "watch_file",
        lua.create_function(|lua, (path, f): (String, Function)| {
            let st = RtState::get(lua)?;
            let file = gio::File::for_path(&path);
            let mon = file
                .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
                .map_err(|e| mlua::Error::runtime(format!("hw.watch_file {path}: {e}")))?;
            let weak = lua.weak();
            mon.connect_changed(move |_, _, _, ev| {
                use gio::FileMonitorEvent as E;
                if !matches!(
                    ev,
                    E::ChangesDoneHint | E::Created | E::Deleted | E::MovedIn | E::Renamed
                ) {
                    return;
                }
                if let Some(lua) = weak.try_upgrade() {
                    RtState::spawn(&lua, &f, ());
                }
            });
            Ok(TimerHandle(st.keep(Box::new(mon))))
        })?,
    )?;

    // Requests
    hw.set(
        "on_request",
        lua.create_function(|lua, (name, f): (String, Function)| {
            RtState::get(lua)?.requests.borrow_mut().insert(name, f);
            Ok(())
        })?,
    )?;

    // Monitors & windows
    let monitors = match &st.core.ui {
        Some(ui) => LuaSignal(Rc::new(MonitorsSource(ui.monitors.clone()))),
        None => LuaSignal(Rc::new(MonitorsSource(Signal::new(Vec::new())))),
    };
    hw.set("monitors", monitors)?;
    crate::service::install(lua, &hw)?;
    crate::service::install_theme(lua, &hw)?;
    hw.set(
        "per_monitor",
        lua.create_function(|lua, f: Function| {
            let st = RtState::get(lua)?;
            st.per_monitor.borrow_mut().push(f);
            if !st.loading.get() && st.core.ui.is_some() {
                sync_monitors(lua)?;
            }
            Ok(())
        })?,
    )?;
    let window = lua.create_table()?;
    window.set(
        "get",
        lua.create_function(|lua, name: String| {
            let st = RtState::get(lua)?;
            Ok(st
                .core
                .ui
                .as_ref()
                .and_then(|ui| ui.windows.get(&name))
                .map(builder::LuaWindow))
        })?,
    )?;
    window.set(
        "new",
        lua.create_function(|lua, t: Table| {
            let st = RtState::get(lua)?;
            if st.core.ui.is_none() {
                return Ok(None);
            }
            let name = builder::build_window(lua, &t)?;
            Ok(st.core.ui()?.windows.get(&name).map(builder::LuaWindow))
        })?,
    )?;
    window.set(
        "list",
        lua.create_function(|lua, ()| {
            let st = RtState::get(lua)?;
            Ok(st
                .core
                .ui
                .as_ref()
                .map(|ui| ui.windows.names())
                .unwrap_or_default())
        })?,
    )?;
    window.set(
        "toggle",
        lua.create_function(|lua, name: String| {
            let st = RtState::get(lua)?;
            Ok(st.core.ui.as_ref().and_then(|ui| ui.windows.toggle(&name)))
        })?,
    )?;
    hw.set("window", window)?;

    // Widgets
    hw.set(
        "define",
        lua.create_function(|lua, (name, f): (String, Function)| {
            if Kind::from_name(&name).is_some() {
                return Err(mlua::Error::runtime(format!(
                    "'{name}' is a built-in widget"
                )));
            }
            RtState::get(lua)?.defined.borrow_mut().insert(name, f);
            Ok(())
        })?,
    )?;
    hw.set(
        "_defined",
        lua.create_function(|lua, name: String| {
            Ok(RtState::get(lua)?.defined.borrow().get(&name).cloned())
        })?,
    )?;
    hw.set(
        "_is_kind",
        lua.create_function(|_, name: String| Ok(Kind::from_name(&name).is_some()))?,
    )?;
    hw.set("layout", lua.create_function(builder::load_layout)?)?;
    hw.set(
        "quit",
        lua.create_function(|lua, ()| {
            if let Some(ui) = &RtState::get(lua)?.core.ui {
                ui.app.quit();
            }
            Ok(())
        })?,
    )?;
    hw.set("config_dir", root.display().to_string())?;
    hw.set("headless", st.core.ui.is_none())?;

    // Hyprland-style config helpers.
    let require_custom_dir = lua.create_function(|lua, dir: Option<String>| {
        let st = RtState::get(lua)?;
        let rel = dir.unwrap_or_else(|| "config/custom".into());
        let path: PathBuf = st.core.dir.root.join(&rel);
        let mut files: Vec<String> = match std::fs::read_dir(&path) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| {
                    e.file_name()
                        .to_str()?
                        .strip_suffix(".lua")
                        .map(str::to_owned)
                })
                .collect(),
            Err(_) => return Ok(()),
        };
        files.sort();
        let require: Function = lua.globals().get("require")?;
        let prefix = rel.trim_matches('/').replace('/', ".");
        for m in files {
            require.call::<Value>(format!("{prefix}.{m}"))?;
        }
        Ok(())
    })?;
    let require_all =
        lua.create_function(|lua, (names, prefix): (Vec<String>, Option<String>)| {
            let require: Function = lua.globals().get("require")?;
            let prefix = prefix.unwrap_or_else(|| "config".into());
            for n in names {
                require.call::<Value>(format!("{prefix}.{n}"))?;
            }
            Ok(())
        })?;
    hw.set("require_all", require_all.clone())?;
    hw.set("require_custom_dir", require_custom_dir.clone())?;
    g.set("require_all", require_all)?;
    g.set("require_custom_dir", require_custom_dir)?;

    crate::dbus_services::install(lua, &hw)?;
    crate::theme::install(lua, &hw)?;
    crate::plugin::install(lua, &hw)?;

    let loaded: Table = package.get("loaded")?;
    loaded.set("hw", hw.clone())?;
    g.set("hw", hw.clone())?;

    // Lua part of the API (coroutines, W, With/For, exec_async, poll).
    let (spawn, request): (Function, Function) =
        lua.load(PRELUDE).set_name("=hw.prelude").call(hw)?;
    lua.set_named_registry_value("hw.spawn", spawn)?;
    lua.set_named_registry_value("hw.request", request)?;
    Ok(())
}

/// `hw.config({...})`: deep-merges into the user option layer in load
/// order. Values: constants, `"inherit"`, or `function(h)`.
fn hw_config(lua: &Lua, t: Table) -> Result<()> {
    let st = RtState::get(lua)?;
    let loc = caller(lua);
    let mut diags = Vec::new();
    walk_config(lua, &st, &t, &mut Vec::new(), &loc, &mut diags)?;
    st.diagnostics.borrow_mut().extend(diags);
    Ok(())
}

fn walk_config(
    lua: &Lua,
    st: &RtState,
    t: &Table,
    path: &mut Vec<String>,
    loc: &str,
    diags: &mut Vec<String>,
) -> Result<()> {
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        let key = match k {
            Value::String(s) => s.to_str()?.to_string(),
            other => {
                diags.push(format!(
                    "{loc}: option keys must be strings, got {}",
                    other.type_name()
                ));
                continue;
            }
        };
        let depth = path.len();
        path.extend(options::name_to_path(&key));
        let name = options::path_to_name(path);
        match v {
            Value::Table(sub) if sub.raw_len() == 0 && options::inherited(&name).is_none() => {
                walk_config(lua, st, &sub, path, loc, diags)?;
            }
            Value::Function(f) => {
                st.option_locs
                    .borrow_mut()
                    .push((name.clone(), loc.to_owned()));
                st.resolvers.borrow_mut().push((path.clone(), f));
                options::insert(&mut st.user_opts.borrow_mut(), path, json!("inherit"));
            }
            v => {
                st.option_locs
                    .borrow_mut()
                    .push((name.clone(), loc.to_owned()));
                let j = lua_to_json(lua, v)?;
                let j = if j.as_str() == Some("inherit") {
                    j
                } else {
                    match options::normalize(&name, &j) {
                        Ok(n) => n,
                        Err(e) => {
                            diags.push(format!("{loc}: {e}"));
                            path.truncate(depth);
                            continue;
                        }
                    }
                };
                st.resolvers.borrow_mut().retain(|(p, _)| p != path);
                let mut nested = json!({});
                options::insert(&mut nested, path, j);
                options::deep_merge(&mut st.user_opts.borrow_mut(), &nested);
                if nested_is_inherit(&nested, path) {
                    // `"inherit"` resets an earlier constant.
                    remove_path(&mut st.user_opts.borrow_mut(), path);
                }
            }
        }
        path.truncate(depth);
    }
    Ok(())
}

fn nested_is_inherit(v: &serde_json::Value, path: &[String]) -> bool {
    options::lookup(v, path).and_then(|v| v.as_str()) == Some("inherit")
}

fn remove_path(v: &mut serde_json::Value, path: &[String]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut cur = v;
    for k in parents {
        match cur.get_mut(k) {
            Some(n) => cur = n,
            None => return,
        }
    }
    if let Some(o) = cur.as_object_mut() {
        o.remove(last);
    }
}

/// Serializes a Lua value as Lua source (for `hyprctl eval`).
fn lua_literal(v: &Value, depth: usize) -> String {
    if depth > 8 {
        return "nil".into();
    }
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("{:?}", s.to_string_lossy()),
        Value::Table(t) => {
            let mut parts = Vec::new();
            for (k, v) in t.pairs::<Value, Value>().flatten() {
                let key = match &k {
                    Value::String(s) => {
                        let s = s.to_string_lossy();
                        if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                            && !s.starts_with(|c: char| c.is_ascii_digit())
                        {
                            s
                        } else {
                            format!("[{s:?}]")
                        }
                    }
                    other => format!("[{}]", lua_literal(other, depth + 1)),
                };
                parts.push(format!("{key} = {}", lua_literal(&v, depth + 1)));
            }
            format!("{{ {} }}", parts.join(", "))
        }
        _ => "nil".into(),
    }
}

/// Untracked helper used by the builder.
pub fn peek_prop(sig: &LuaSignal) -> hw_ui::widgets::Prop {
    untracked(|| sig.0.get_prop())
}

/// Subscription owned by a widget (dropped with it).
pub type Sub = Subscription;
