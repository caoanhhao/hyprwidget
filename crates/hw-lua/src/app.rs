//! The daemon (`hyprwidget run`) and offline commands (`check`, `option`).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use anyhow::{Context, Result};
use hw_core::config::ConfigDir;
use hw_core::hyprland;
use hw_core::ipc::{self, Reply, Request, Response};
use hw_ui::gtk4::{self, gio, glib, prelude::*};
use hw_ui::style::Styles;
use hw_ui::watch::{self, Changes};

use crate::convert::LuaResultExt;
use crate::runtime::{Core, Runtime, Ui};

pub struct App {
    core: Rc<Core>,
    runtime: RefCell<Option<Runtime>>,
    keep: RefCell<Vec<Box<dyn std::any::Any>>>,
    /// A Lua (re)load is running; the CSS is compiled once it is done.
    loading: std::cell::Cell<bool>,
    /// A generated partial changed while loading.
    css_dirty: std::cell::Cell<bool>,
}

/// Writes `_hw-theme.scss`; returns whether it changed.
fn write_theme(core: &Core) -> bool {
    let Some(ui) = &core.ui else { return false };
    match ui
        .styles
        .write_partial(crate::theme::PARTIAL, &core.theme.scss())
    {
        Ok(changed) => changed,
        Err(e) => {
            tracing::error!("theme partial: {e:#}");
            false
        }
    }
}

fn gen_dir(instance: &str) -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    Ok(base.join("hyprwidget").join(instance))
}

/// Runs the GTK main loop. Returns the exit code.
pub fn run(dir: ConfigDir, instance: String) -> Result<i32> {
    hw_ui::init()?;
    let gtk_app = gtk4::Application::builder()
        .application_id(hw_ui::APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let state: Rc<RefCell<Option<Rc<App>>>> = Rc::default();
    let startup_err: Rc<RefCell<Option<anyhow::Error>>> = Rc::default();
    {
        let (state, startup_err) = (state.clone(), startup_err.clone());
        gtk_app.connect_activate(move |gtk_app| {
            match App::start(gtk_app, dir.clone(), &instance) {
                Ok(app) => {
                    *state.borrow_mut() = Some(app);
                }
                Err(e) => {
                    *startup_err.borrow_mut() = Some(e);
                    gtk_app.quit();
                }
            }
        });
    }
    let code = gtk_app.run_with_args::<&str>(&[]);
    if let Some(app) = state.borrow_mut().take() {
        app.shutdown();
    }
    if let Some(e) = startup_err.borrow_mut().take() {
        return Err(e);
    }
    Ok(code.get() as i32)
}

impl App {
    fn start(gtk_app: &gtk4::Application, dir: ConfigDir, instance: &str) -> Result<Rc<Self>> {
        let app_cfg = dir.load_app()?;
        let styles = Styles::new(gen_dir(instance)?, app_cfg.style.clone())?;
        let ui = Ui {
            app: gtk_app.clone(),
            styles,
            windows: Default::default(),
            monitors: hw_ui::monitors::watch(),
        };
        let core = Core::new(dir, Some(ui), true)?;
        let app = Rc::new(Self {
            core: core.clone(),
            runtime: RefCell::new(None),
            keep: RefCell::new(Vec::new()),
            loading: Default::default(),
            css_dirty: Default::default(),
        });
        // Keep running without windows (e.g. while the Lua config is broken).
        app.keep(gtk_app.hold());

        // Options → `_hypr.scss` + default stylesheet, live.
        let styles = core.ui().anyhow()?.styles.clone();
        let regen = move |tree: &serde_json::Value| {
            if let Err(e) = styles.set_options(tree) {
                tracing::error!("default stylesheet: {e:#}");
            }
        };
        regen(&core.options.tree().peek());
        app.keep(core.options.tree().subscribe(regen));

        // Theme → `_hw-theme.scss`, recompiled when it changes (the initial
        // compile happens after the first Lua load).
        let ui = core.ui().anyhow()?;
        ui.styles.set_extra_error_hook(Rc::new(|key, msg| {
            crate::plugin::notify(&format!("hyprwidget: {key} stylesheet failed"), msg)
        }));
        write_theme(&core);
        for sig in [core.theme.name.clone(), core.theme.colors.clone()] {
            let weak = Rc::downgrade(&app);
            app.keep(sig.subscribe(move |_| {
                if let Some(app) = weak.upgrade()
                    && write_theme(&app.core)
                {
                    if app.loading.get() {
                        app.css_dirty.set(true);
                    } else {
                        app.reload_css();
                    }
                }
            }));
        }

        app.reload_lua();
        app.reload_css();

        let weak = Rc::downgrade(&app);
        let server = ipc::Server::start(instance, move |req, reply| match weak.upgrade() {
            Some(app) => app.handle(req, reply),
            None => reply.send(Response::err("shutting down")),
        })?;
        tracing::info!(socket = %server.path().display(), "listening");
        app.keep(server);

        if hyprland::available() {
            let weak = Rc::downgrade(&app);
            hyprland::listen_events(move |ev, _| {
                if ev == "configreloaded"
                    && let Some(app) = weak.upgrade()
                {
                    tracing::info!("Hyprland config reloaded: refreshing options");
                    app.core.refresh_hypr();
                    app.core.reapply_layer_rules();
                }
            })?;
        } else {
            tracing::warn!("not running under Hyprland: using built-in option defaults");
        }

        let weak: Weak<App> = Rc::downgrade(&app);
        let cache = app
            .core
            .app_cfg
            .borrow()
            .settings_file
            .parent()
            .map(PathBuf::from);
        // Plugins are watched one by one (crate::plugin).
        let skip: Vec<PathBuf> = cache
            .into_iter()
            .chain([app.core.dir.root.join(crate::plugin::PLUGINS_DIR)])
            .collect();
        let watcher = watch::watch(&app.core.dir.root, &skip, move |c: Changes| {
            let Some(app) = weak.upgrade() else { return };
            if c.lua {
                tracing::info!("config changed: reloading Lua");
                app.reload_lua();
            }
            if c.css || c.lua {
                app.reload_css();
            }
        });
        app.keep(watcher);
        Ok(app)
    }

    fn keep(&self, thing: impl std::any::Any) {
        self.keep.borrow_mut().push(Box::new(thing));
    }

    fn shutdown(&self) {
        self.runtime.borrow_mut().take();
        self.keep.borrow_mut().clear();
        if let Err(e) = self.core.settings.flush() {
            tracing::error!("saving settings: {e:#}");
        }
    }

    /// Tears down the Lua state and its windows and loads the config again.
    /// Services/options/settings live in [`Core`] and are kept.
    pub fn reload_lua(&self) {
        // Drop the old runtime first: closes windows and stops timers.
        self.runtime.borrow_mut().take();
        self.loading.set(true);
        let _done = scopeguard(|| self.loading.set(false));
        match self.core.dir.load_app() {
            Ok(cfg) => {
                if let Some(ui) = &self.core.ui {
                    ui.styles.set_entry(cfg.style.clone());
                }
                *self.core.app_cfg.borrow_mut() = cfg;
            }
            Err(e) => tracing::error!("{e:#}"),
        }
        self.core
            .options
            .set_user(serde_json::json!({}), Vec::new());
        let rt = match Runtime::new(self.core.clone()) {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("{e:#}");
                return;
            }
        };
        match rt.load() {
            Ok(()) => {
                for d in rt.st.diagnostics.borrow().iter() {
                    tracing::warn!("{d}");
                }
                tracing::info!(windows = ?self.core.ui.as_ref().map(|u| u.windows.names()), "Lua config loaded");
            }
            Err(e) => tracing::error!("loading {}: {e}", self.core.dir.entry().display()),
        }
        *self.runtime.borrow_mut() = Some(rt);
    }

    pub fn reload_css(&self) {
        self.css_dirty.set(false);
        if let Some(ui) = &self.core.ui
            && let Err(e) = ui.styles.reload_user()
        {
            tracing::error!("stylesheet: {e:#}");
        }
    }

    fn handle(self: &Rc<Self>, req: Request, reply: Reply) {
        let arg = |i: usize| req.args.get(i).cloned().unwrap_or_default();
        let resp = match req.cmd.as_str() {
            "request" => {
                let rt = self.runtime.borrow();
                let Some(rt) = rt.as_ref() else {
                    return reply.send(Response::err("Lua config is not loaded"));
                };
                let reply = RefCell::new(Some(reply));
                rt.request(
                    &arg(0),
                    &req.args[1.min(req.args.len())..],
                    move |ok, out| {
                        if let Some(r) = reply.borrow_mut().take() {
                            r.send(Response { ok, output: out });
                        }
                    },
                );
                return;
            }
            "reload" => {
                // Reply first: reloading may take a while and runs Lua.
                let what = arg(0);
                reply.send(Response::ok(""));
                let app = self.clone();
                glib::idle_add_local_once(move || match what.as_str() {
                    "css" => app.reload_css(),
                    "lua" => {
                        app.reload_lua();
                        if app.css_dirty.get() {
                            app.reload_css();
                        }
                    }
                    _ => {
                        app.reload_lua();
                        app.reload_css();
                    }
                });
                return;
            }
            "toggle" => match self
                .core
                .ui
                .as_ref()
                .and_then(|u| u.windows.toggle(&arg(0)))
            {
                Some(v) => Response::ok(if v { "shown" } else { "hidden" }),
                None => Response::err(format!("no window '{}'", arg(0))),
            },
            "eval" => match self.runtime.borrow().as_ref() {
                Some(rt) => match rt.eval(&req.args.join(" ")) {
                    Ok(s) => Response::ok(s),
                    Err(e) => Response::err(format!("{e:#}")),
                },
                None => Response::err("Lua config is not loaded"),
            },
            "list" => {
                let mut out = String::new();
                if let Some(ui) = &self.core.ui {
                    for (name, visible) in ui.windows.list() {
                        out.push_str(&format!(
                            "{name}\t{}\n",
                            if visible { "visible" } else { "hidden" }
                        ));
                    }
                }
                if let Some(rt) = self.runtime.borrow().as_ref() {
                    for r in rt.request_names() {
                        out.push_str(&format!("request:{r}\n"));
                    }
                }
                Response::ok(out.trim_end())
            }
            "option" => Response::ok(format_option(&self.core.options.get(&arg(0)))),
            "theme" => self.theme_cmd(req.args.first().map(String::as_str)),
            "inspect" => {
                gtk4::Window::set_interactive_debugging(true);
                Response::ok("")
            }
            "quit" => {
                if let Some(ui) = &self.core.ui {
                    ui.app.quit();
                }
                Response::ok("")
            }
            other => Response::err(format!("unknown command '{other}'")),
        };
        reply.send(resp);
    }
}

impl App {
    /// `hyprwidget theme [name|none]`: switch, or show the current theme.
    fn theme_cmd(&self, name: Option<&str>) -> Response {
        let rt = self.runtime.borrow();
        let Some(rt) = rt.as_ref() else {
            return Response::err("Lua config is not loaded");
        };
        if let Some(n) = name {
            let n = Some(n).filter(|n| *n != "none" && !n.is_empty());
            return match crate::theme::switch(&rt.lua, n) {
                Ok(()) => Response::ok(""),
                Err(e) => Response::err(crate::runtime::compact_error(&e.to_string())),
            };
        }
        let cur = self.core.theme.name.peek();
        let mut out = String::new();
        for t in crate::theme::list(&self.core.dir.root) {
            let mark = if cur.as_str() == Some(t.as_str()) {
                "*"
            } else {
                " "
            };
            out.push_str(&format!("{mark} {t}\n"));
        }
        if cur.is_null() {
            out.push_str("* none\n");
        }
        Response::ok(out.trim_end())
    }
}

/// Runs `f` on drop.
fn scopeguard(f: impl FnOnce()) -> impl Drop {
    struct G<F: FnOnce()>(Option<F>);
    impl<F: FnOnce()> Drop for G<F> {
        fn drop(&mut self) {
            if let Some(f) = self.0.take() {
                f()
            }
        }
    }
    G(Some(f))
}

/// Prints an option value: strings raw, everything else as JSON.
pub fn format_option(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(_) | serde_json::Value::Array(_) => {
            serde_json::to_string_pretty(v).unwrap_or_default()
        }
        other => other.to_string(),
    }
}

/// Loads the config without GTK (for `check` and offline `option`).
fn headless(dir: ConfigDir) -> Result<(Rc<Core>, Runtime, Option<String>)> {
    let core = Core::new(dir, None, true)?;
    let rt = Runtime::new(core.clone())?;
    let err = rt.load().err().map(|e| format!("{e:#}"));
    Ok((core, rt, err))
}

/// Merged value of an option without a running daemon.
pub fn option_offline(dir: ConfigDir, name: &str) -> Result<serde_json::Value> {
    let (core, _rt, err) = headless(dir)?;
    if let Some(e) = err {
        anyhow::bail!("{e}");
    }
    Ok(core.options.get(name))
}

/// Validates the config; returns one message per problem (`file:line: …`).
pub fn check(dir: ConfigDir) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    if let Err(e) = dir.load_app() {
        problems.push(format!("{e:#}"));
        return Ok(problems);
    }
    let (core, rt, err) = headless(dir.clone())?;
    if let Some(e) = err {
        problems.push(crate::runtime::compact_error(&e));
    }
    problems.extend(rt.st.diagnostics.borrow().iter().cloned());

    // Options under Hyprland sections must exist in Hyprland.
    let locs = rt.st.option_locs.borrow().clone();
    let unknown: Vec<&(String, String)> = locs
        .iter()
        .filter(|(name, _)| {
            let path = hw_core::options::name_to_path(name);
            path.first()
                .is_some_and(|s| hw_core::options::HYPR_SECTIONS.contains(&s.as_str()))
                && path.first().map(String::as_str) != Some("animations")
                && hw_core::options::inherited(name).is_none()
        })
        .collect();
    if !unknown.is_empty() && hyprland::available() {
        let names: Vec<&str> = unknown.iter().map(|(n, _)| n.as_str()).collect();
        match hyprland::get_options(&names) {
            Ok(replies) => {
                for ((name, loc), reply) in unknown.iter().zip(replies) {
                    if reply.is_none() {
                        problems.push(format!("{loc}: unknown Hyprland option '{name}'"));
                    }
                }
            }
            Err(e) => tracing::warn!("cannot ask Hyprland about options: {e:#}"),
        }
    }

    // Stylesheet.
    let cfg = core.app_cfg.borrow().clone();
    let gen_tmp = std::env::temp_dir().join(format!("hyprwidget-check-{}", std::process::id()));
    std::fs::create_dir_all(&gen_tmp).context("creating temp dir")?;
    std::fs::write(
        gen_tmp.join("_hypr.scss"),
        hw_core::options::to_scss(&core.options.tree().peek()),
    )?;
    for (name, text) in core.scss_partials.borrow().iter() {
        hw_ui::style::write_partial(&gen_tmp, name, text)?;
    }
    hw_ui::style::write_partial(&gen_tmp, crate::theme::PARTIAL, &core.theme.scss())?;
    let style_dir = cfg.style.parent().map(PathBuf::from).unwrap_or_default();
    if let Err(e) = hw_ui::style::compile_file(&cfg.style, &[&style_dir, &gen_tmp]) {
        problems.push(format!("{e:#}"));
    }

    // Every plugin.json / theme.json (also the disabled ones) and the
    // stylesheets of the loaded plugins.
    for f in crate::plugin::discover(&dir.root) {
        if let Err(e) = f.manifest {
            problems.extend(e);
        } else if let Ok(m) = f.manifest.as_ref()
            && crate::plugin::loaded_ok(&rt.lua).contains(&m.name)
        {
            let style = f.dir.join(&m.style);
            if style.is_file()
                && let Err(e) = hw_ui::style::compile_extra(&style, &gen_tmp)
            {
                problems.push(format!("{e:#}"));
            }
        }
    }
    problems.extend(crate::theme::check_all(&dir.root));
    let mut seen = std::collections::HashSet::new();
    problems.retain(|p| seen.insert(p.clone()));
    let _ = std::fs::remove_dir_all(&gen_tmp);
    Ok(problems)
}
