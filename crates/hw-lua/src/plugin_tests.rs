//! Plugins (sandbox, loading, errors) and themes (selection, switching).

use std::path::PathBuf;
use std::rc::Rc;

use hw_core::config::ConfigDir;

use crate::runtime::{Core, Runtime};

fn config_dir(name: &str, files: &[(&str, &str)]) -> ConfigDir {
    let root: PathBuf =
        std::env::temp_dir().join(format!("hw-plugin-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    ConfigDir { root }
}

fn load(dir: ConfigDir) -> (Rc<Core>, Runtime) {
    let core = Core::new(dir, None, false).unwrap();
    let rt = Runtime::new(core.clone()).unwrap();
    rt.load().unwrap();
    (core, rt)
}

fn manifest(name: &str, perms: &str, extra: &str) -> String {
    format!(r#"{{ "name": "{name}", "version": "1.0.0", "permissions": [{perms}]{extra} }}"#)
}

const PROBE: &str = r#"
local r = {}
r.exec = pcall(function() return hw.exec end)
r.exec_async = pcall(function() return hw.exec_async end)
r.subprocess = pcall(function() return hw.subprocess end)
r.poll_cmd = pcall(hw.poll, 1000, "true")
r.watch_file = pcall(function() return hw.watch_file end)
r.fetch = pcall(function() return hw.fetch end) and type(hw.fetch) == "function"
r.quit = pcall(function() return hw.quit end)
r.config = pcall(function() return hw.config end)
r.io = io ~= nil
r.popen = io ~= nil and io.popen ~= nil
r.os_execute = os.execute ~= nil
r.os_remove = os.remove ~= nil
r.string_mt = getmetatable("") ~= nil
r.state = pcall(hw.state, 1)
leaked_global = true
string.leaked = true
hw.leaked = true
return { services = { ["probe_" .. hw.plugin.name] = r } }
"#;

fn probe(rt: &Runtime, plugin: &str, key: &str) -> String {
    rt.eval(&format!("hw.service.probe_{plugin}.{key}"))
        .unwrap()
}

#[test]
fn sandbox_grants_only_declared_permissions() {
    let dir = config_dir(
        "sandbox",
        &[
            ("hyprwidget.lua", ""),
            ("plugins/none/plugin.json", &manifest("none", "", "")),
            ("plugins/none/init.lua", PROBE),
            (
                "plugins/all/plugin.json",
                &manifest("all", r#""exec", "network", "fs""#, ""),
            ),
            ("plugins/all/init.lua", PROBE),
            ("plugins/fs/plugin.json", &manifest("fs", r#""fs""#, "")),
            ("plugins/fs/init.lua", PROBE),
        ],
    );
    let (core, rt) = load(dir);
    assert!(
        core.ui.is_none() && rt.st.diagnostics.borrow().is_empty(),
        "{:?}",
        rt.st.diagnostics.borrow()
    );
    for key in [
        "exec",
        "exec_async",
        "subprocess",
        "poll_cmd",
        "watch_file",
        "fetch",
        "io",
        "popen",
        "os_execute",
        "os_remove",
    ] {
        assert_eq!(probe(&rt, "none", key), "false", "none: {key}");
        assert_eq!(probe(&rt, "all", key), "true", "all: {key}");
    }
    for key in ["io", "watch_file", "os_remove"] {
        assert_eq!(probe(&rt, "fs", key), "true", "fs: {key}");
    }
    for key in ["exec", "popen", "os_execute", "fetch"] {
        assert_eq!(probe(&rt, "fs", key), "false", "fs: {key}");
    }
    for p in ["none", "all"] {
        assert_eq!(probe(&rt, p, "quit"), "false");
        assert_eq!(probe(&rt, p, "config"), "false");
        assert_eq!(probe(&rt, p, "string_mt"), "false");
        assert_eq!(probe(&rt, p, "state"), "true");
    }
    // Nothing leaks into the config's globals, `string` or `hw`.
    assert_eq!(rt.eval("leaked_global").unwrap(), "nil");
    assert_eq!(rt.eval("string.leaked").unwrap(), "nil");
    assert_eq!(rt.eval("hw.leaked").unwrap(), "nil");
    // The error message names the missing permission.
    let dir = config_dir(
        "sandbox-msg",
        &[
            ("hyprwidget.lua", ""),
            ("plugins/p/plugin.json", &manifest("p", "", "")),
            ("plugins/p/init.lua", "hw.exec('true')"),
        ],
    );
    let (_c, rt) = load(dir);
    let d = rt.st.diagnostics.borrow().join("\n");
    assert!(d.contains("hw.exec needs the 'exec' permission (p)"), "{d}");
    assert!(d.contains("plugins/p/init.lua:1"), "{d}");
}

#[test]
fn plugin_widgets_services_requests_and_require() {
    let dir = config_dir(
        "load",
        &[
            (
                "hyprwidget.lua",
                r#"local W = hw.widget
                   local w = W.Clock{ fmt = "x" }
                   assert(w.type == "Label" and w.label == "clock:x", tostring(w.label))
                   assert(hw.service.clock_svc.ticks == 3)
                   assert(hw.service.lazy_svc.n == 7)
                   SETUP_SEEN = hw.service.clock_svc.setup_ran"#,
            ),
            ("plugins/clock/plugin.json", &manifest("clock", "", "")),
            (
                "plugins/clock/init.lua",
                r#"local util = require("lib.util")
                   local svc = { ticks = 3 }
                   return {
                     widgets = { Clock = function(p) return W.Label{ label = util.prefix .. p.fmt } end },
                     services = { clock_svc = svc, lazy_svc = function() return { n = 7 } end },
                     requests = { clock_now = function(args) return "now:" .. (args[1] or "") end },
                     setup = function(info) svc.setup_ran = info.name end,
                   }"#,
            ),
            (
                "plugins/clock/lib/util.lua",
                r#"return { prefix = "clock:" }"#,
            ),
        ],
    );
    let (_core, rt) = load(dir);
    assert!(
        rt.st.diagnostics.borrow().is_empty(),
        "{:?}",
        rt.st.diagnostics.borrow()
    );
    assert_eq!(rt.eval("SETUP_SEEN").unwrap(), "clock"); // setup ran before the config
    assert_eq!(rt.eval("hw.service.clock_svc.setup_ran").unwrap(), "clock");
    assert!(rt.request_names().contains(&"clock_now".to_string()));
    let out = Rc::new(std::cell::RefCell::new(None));
    let o = out.clone();
    rt.request("clock_now", &["a".into()], move |ok, s| {
        *o.borrow_mut() = Some((ok, s))
    });
    assert_eq!(*out.borrow(), Some((true, "now:a".to_string())));
    assert_eq!(rt.eval("hw.plugins.list()[1]").unwrap(), "clock");
    // JSON layouts resolve plugin widgets through the same registry.
    assert_eq!(rt.eval("hw._defined('Clock') ~= nil").unwrap(), "true");
}

#[test]
fn broken_plugins_are_reported_and_isolated() {
    let dir = config_dir(
        "broken",
        &[
            (
                "hyprwidget.lua",
                r#"local W = hw.widget
                   local w = W.Broken{}
                   assert(w.class == "plugin-error", "placeholder expected")
                   assert(W.Good{}.label == "good")
                   local t = W.Throws{}
                   assert(t.class == "plugin-error")"#,
            ),
            (
                "plugins/broken/plugin.json",
                &manifest("broken", "", r#", "widgets": ["Broken"]"#),
            ),
            ("plugins/broken/init.lua", "return {{ syntax error"),
            ("plugins/good/plugin.json", &manifest("good", "", "")),
            (
                "plugins/good/init.lua",
                r#"return { widgets = {
                     Good = function() return W.Label{ label = "good" } end,
                     Throws = function() error("kaboom") end,
                   } }"#,
            ),
            ("plugins/badret/plugin.json", &manifest("badret", "", "")),
            ("plugins/badret/init.lua", "return 42"),
            ("plugins/badkey/plugin.json", &manifest("badkey", "", "")),
            ("plugins/badkey/init.lua", "return { widget = {} }"),
            (
                "plugins/setupfail/plugin.json",
                &manifest("setupfail", "", ""),
            ),
            (
                "plugins/setupfail/init.lua",
                "return { setup = function() error('nope') end }",
            ),
            (
                "plugins/badjson/plugin.json",
                "{\n  \"name\": \"badjson\",\n  \"version\": \"1.0.0\",\n  \"permissions\": [\"root\"]\n}",
            ),
            ("plugins/badjson/init.lua", "return {}"),
            (
                "plugins/needs/plugin.json",
                &manifest("needs", "", r#", "dependencies": { "missing": ">=1" }"#),
            ),
            ("plugins/needs/init.lua", "return {}"),
            ("plugins/shadow/plugin.json", &manifest("shadow", "", "")),
            (
                "plugins/shadow/init.lua",
                "return { widgets = { Box = function() end } }",
            ),
        ],
    );
    let (_core, rt) = load(dir);
    let d = rt.st.diagnostics.borrow().join("\n");
    for needle in [
        "plugins/broken/init.lua:1:",
        "plugin 'good': W.Throws: plugins/good/init.lua:3: kaboom",
        "init.lua must return a table, got number",
        "unknown key 'widget'",
        "setup: plugins/setupfail/init.lua:1: nope",
        "plugins/badjson/plugin.json:4: unknown permission 'root'",
        "plugins/needs/plugin.json:1: dependency 'missing' is not available",
        "widget 'Box' shadows a built-in widget",
    ] {
        assert!(d.contains(needle), "missing {needle:?} in:\n{d}");
    }
    assert_eq!(rt.eval("hw.plugins.errors().good").unwrap(), "nil");
}

#[test]
fn dependencies_load_first_and_selection_applies() {
    let dir = config_dir(
        "deps",
        &[
            ("hyprwidget.lua", ""),
            ("config.json", r#"{ "plugins": { "disabled": ["off"] } }"#),
            (
                "plugins/a/plugin.json",
                &manifest("a", "", r#", "dependencies": ["b"]"#),
            ),
            (
                "plugins/a/init.lua",
                "assert(hw.service.from_b == 'b'); return {}",
            ),
            (
                "plugins/b/plugin.json",
                r#"{ "name": "b", "version": "2.1.0" }"#,
            ),
            (
                "plugins/b/init.lua",
                "return { services = { from_b = 'b' } }",
            ),
            ("plugins/off/plugin.json", &manifest("off", "", "")),
            ("plugins/off/init.lua", "error('should not load')"),
            (
                "plugins/old/plugin.json",
                &manifest("old", "", r#", "dependencies": { "b": "^1.0" }"#),
            ),
            ("plugins/old/init.lua", "return {}"),
        ],
    );
    let (_core, rt) = load(dir);
    let d = rt.st.diagnostics.borrow().join("\n");
    assert!(
        d.contains("dependency 'b' 2.1.0 does not match '^1.0'"),
        "{d}"
    );
    assert!(!d.contains("should not load"), "{d}");
    assert_eq!(
        rt.eval("table.concat(hw.plugins.list(), ',')").unwrap(),
        "a,b"
    );
    assert_eq!(rt.eval("hw.plugins.errors().a").unwrap(), "nil");
}

#[test]
fn manifest_validation_reports_lines() {
    let dir = config_dir("manifest", &[("plugins/x/init.lua", "")]);
    let text = "{\n  \"name\": \"x\",\n  \"version\": \"one\",\n  \"entry\": \"../evil.lua\",\n  \"colour\": 1\n}";
    let e =
        crate::plugin::parse_manifest("plugins/x/plugin.json", text, &dir.root.join("plugins/x"))
            .unwrap_err();
    let e = e.join("\n");
    assert!(
        e.contains("plugins/x/plugin.json:3: invalid version 'one'"),
        "{e}"
    );
    assert!(
        e.contains("plugins/x/plugin.json:4: 'entry' must be a relative path"),
        "{e}"
    );
    assert!(
        e.contains("plugins/x/plugin.json:5: unknown key 'colour'"),
        "{e}"
    );
    let e =
        crate::plugin::parse_manifest("p.json", "{\n\"name\": \"x\"\n,,}", &dir.root).unwrap_err();
    assert!(e[0].starts_with("p.json:3:"), "{e:?}");
}

const THEMES: &[(&str, &str)] = &[
    (
        "themes/nord/theme.json",
        r##"{ "name": "nord", "version": "1.0.0", "colors": { "background": "#2e3440", "primary": "#88c0d0" } }"##,
    ),
    ("themes/nord/_vars.scss", "$nord-extra: 1px;"),
    (
        "themes/wall/theme.json",
        r##"{ "name": "wall", "source": "wallpaper", "permissions": [], "colors": { "primary": "#000001" } }"##,
    ),
    (
        "themes/wall/init.lua",
        r##"local s = hw.state({ background = "#101010" })
            hw.on_request("wall_bg", function(args) s:set({ background = args[1] }) end)
            return { colors = s }"##,
    ),
];

#[test]
fn theme_selection_switching_and_sources() {
    let mut files = THEMES.to_vec();
    files.push(("hyprwidget.lua", "PRIMARY = hw.theme.colors.primary"));
    files.push(("config.json", r#"{ "theme": "nord" }"#));
    let dir = config_dir("theme", &files);
    let (core, rt) = load(dir.clone());
    assert!(
        rt.st.diagnostics.borrow().is_empty(),
        "{:?}",
        rt.st.diagnostics.borrow()
    );
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "nord");
    assert_eq!(rt.eval("PRIMARY:get()").unwrap(), "#88c0d0");
    let scss = core.theme.scss();
    assert!(
        scss.contains("$theme-name: \"nord\";") && scss.contains("$primary: #88c0d0;"),
        "{scss}"
    );
    assert!(scss.contains("themes/nord/vars\";"), "{scss}");
    // The generated partial compiles, including the theme's _vars.scss.
    let gen_dir = dir.root.join("gen");
    std::fs::create_dir_all(&gen_dir).unwrap();
    hw_ui::style::write_partial(&gen_dir, crate::theme::PARTIAL, &scss).unwrap();
    let css = hw_ui::style::compile_str(
        "@import \"hw-theme\"; .x { color: $primary; margin: $nord-extra; background: $urgent; }",
        &[&gen_dir],
    )
    .unwrap();
    assert!(
        css.contains("#88c0d0") && css.contains("1px") && css.contains("#f38ba8"),
        "{css}"
    );

    // Runtime switch: signals follow; init.lua colors (a signal) apply.
    rt.eval("hw.theme.set('wall')").unwrap();
    assert_eq!(rt.eval("PRIMARY:get()").unwrap(), "#000001");
    assert_eq!(
        rt.eval("hw.theme.colors.background:get()").unwrap(),
        "#101010"
    );
    rt.request("wall_bg", &["#202020".into()], |ok, e| assert!(ok, "{e}"));
    assert_eq!(rt.eval("hw.theme.colors().background").unwrap(), "#202020");
    // A registered color source wins over the theme's own colors.
    rt.eval("hw.theme.add_source('wallpaper', { primary = 'rgb(ff0000)' })")
        .unwrap();
    assert_eq!(
        rt.eval("PRIMARY:get()").unwrap(),
        "rgba(255, 0, 0, 1)".replace(", ", ",")
    );
    assert!(rt.eval("hw.theme.set('missing')").is_err());
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "wall");

    // The runtime choice survives a Lua reload (same Core)…
    drop(rt);
    assert!(
        core.theme.source_names().is_empty(),
        "Lua source removed with its state"
    );
    let rt = Runtime::new(core.clone()).unwrap();
    rt.load().unwrap();
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "wall");
    rt.eval("hw.theme.set(nil)").unwrap();
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "nil");
    assert_eq!(rt.eval("hw.theme.colors.primary:get()").unwrap(), "nil");
    // …until config.json names another theme.
    rt.eval("hw.theme.set('wall')").unwrap();
    drop(rt);
    std::fs::write(dir.root.join("config.json"), r#"{ "theme": "nord" }"#).unwrap();
    let rt = Runtime::new(core.clone()).unwrap();
    rt.load().unwrap();
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "wall");
    drop(rt);
    std::fs::write(dir.root.join("config.json"), r#"{ "theme": null }"#).unwrap();
    let rt = Runtime::new(core.clone()).unwrap();
    rt.load().unwrap();
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "nil");
}

#[test]
fn rust_color_source_plugs_into_themes() {
    let mut files = THEMES.to_vec();
    files.push(("hyprwidget.lua", ""));
    files.push(("config.json", r#"{ "theme": "wall" }"#));
    let (core, rt) = load(config_dir("theme-src", &files));
    let palette = hw_core::reactive::Signal::new(serde_json::json!({ "primary": "#abcdef" }));
    core.theme.set_source("wallpaper", palette.clone());
    assert_eq!(rt.eval("hw.theme.colors.primary:get()").unwrap(), "#abcdef");
    palette.set(serde_json::json!({ "primary": "#123456" }));
    assert_eq!(rt.eval("hw.theme.colors.primary:get()").unwrap(), "#123456");
}

#[test]
fn bad_theme_is_reported() {
    let dir = config_dir(
        "theme-bad",
        &[
            ("hyprwidget.lua", ""),
            ("config.json", r#"{ "theme": "oops" }"#),
            (
                "themes/oops/theme.json",
                "{\n  \"name\": \"oops\",\n  \"colors\": { \"primary\": \"nope nope\" }\n}",
            ),
            ("themes/gone/README", ""),
        ],
    );
    let (_core, rt) = load(dir.clone());
    let d = rt.st.diagnostics.borrow().join("\n");
    assert!(
        d.contains("themes/oops/theme.json:3: colors.primary: invalid color"),
        "{d}"
    );
    assert_eq!(rt.eval("hw.theme.name:get()").unwrap(), "nil");
    assert_eq!(crate::theme::check_all(&dir.root).len(), 1);
}
