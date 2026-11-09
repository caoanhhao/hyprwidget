use std::path::{Path, PathBuf};
use std::rc::Rc;

use hw_core::config::ConfigDir;
use serde_json::{Value, json};

use crate::runtime::{Core, Runtime};

#[test]
fn links_lua_55() {
    assert_eq!(super::lua_version().unwrap(), "Lua 5.5");
}

/// Writes `files` into a fresh config dir.
fn config_dir(name: &str, files: &[(&str, &str)]) -> ConfigDir {
    let root: PathBuf = std::env::temp_dir().join(format!("hw-lua-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    ConfigDir { root }
}

fn load(dir: ConfigDir, hypr: Option<Value>) -> (std::rc::Rc<Core>, Runtime) {
    let core = Core::new(dir, None, crate::runtime::HyprOptions::Ignore).unwrap();
    if let Some(h) = hypr {
        core.options.set_hypr(h);
    }
    let rt = Runtime::new(Rc::clone(&core)).unwrap();
    rt.load().unwrap();
    (core, rt)
}

const ENTRY: &str = r#"
require_all({ "general", "decoration" })
require_custom_dir()
"#;

#[test]
fn hyprland_style_config_merges_in_load_order() {
    let dir = config_dir(
        "merge",
        &[
            ("hyprwidget.lua", ENTRY),
            (
                "config.json",
                r#"{ "decoration:rounding": 2, "general": { "border_size": 3 } }"#,
            ),
            (
                "config/general.lua",
                r#"hw.config({ general = {
                    gaps_out = "inherit",
                    gaps_in = function(h) return h.general.gaps_out * 2 end,
                    col = { active_border = "rgb(ff0000) rgb(0000ff) 45deg" },
                } })"#,
            ),
            (
                "config/decoration.lua",
                r"hw.config({ decoration = { rounding = 8, blur = { size = 3 } } })",
            ),
            // custom/ overrides win (loaded last), flat key form.
            (
                "config/custom/decoration:rounding.lua",
                r#"hw.config({ ["decoration:rounding"] = 12 })"#,
            ),
        ],
    );
    let (core, rt) = load(
        dir,
        Some(
            json!({ "general": { "gaps_out": 6, "border_size": 1 }, "decoration": { "rounding": 16 } }),
        ),
    );
    let o = &core.options;
    assert!(
        rt.st.diagnostics.borrow().is_empty(),
        "{:?}",
        rt.st.diagnostics.borrow()
    );
    assert_eq!(o.get("decoration:rounding"), json!(12));
    assert_eq!(o.get("decoration:blur:size"), json!(3));
    assert_eq!(o.get("general:gaps_out"), json!(6));
    assert_eq!(o.get("general:gaps_in"), json!(12));
    assert_eq!(o.get("general:border_size"), json!(3));
    assert_eq!(
        o.get("general:col.active_border"),
        json!("linear-gradient(135deg, rgba(255,0,0,1), rgba(0,0,255,1))")
    );
    // Resolvers follow Hyprland.
    o.set_hypr(json!({ "general": { "gaps_out": 10 } }));
    assert_eq!(o.get("general:gaps_in"), json!(20));
}

#[test]
fn config_errors_have_file_and_line() {
    let dir = config_dir(
        "errors",
        &[
            ("hyprwidget.lua", "require_all({ \"general\" })\n"),
            (
                "config/general.lua",
                "\n\nhw.config({ general = { gaps_out = \"wide\" } })\n",
            ),
        ],
    );
    let (_core, rt) = load(dir, None);
    let d = rt.st.diagnostics.borrow();
    assert_eq!(d.len(), 1);
    assert!(d[0].starts_with("config/general.lua:3: "), "{}", d[0]);
}

#[test]
fn lua_reactive_api() {
    let dir = config_dir("reactive", &[("hyprwidget.lua", "")]);
    let (_core, rt) = load(dir, None);
    let out = rt
        .eval(
            r#"(function()
                local a = hw.state(1)
                local b = hw.state(2)
                local sum = hw.computed(function() return a() + b:get() end)
                local label = sum:map(function(v) return "sum=" .. v end)
                local seen = {}
                local unsub = label:subscribe(function(v) seen[#seen + 1] = v end)
                a:set(10)
                b:set(5)
                unsub()
                a:set(0)
                return label:get() .. " " .. table.concat(seen, ",")
            end)()"#,
        )
        .unwrap();
    assert_eq!(out, "sum=5 sum=12,sum=15");
}

#[test]
fn widget_tables_and_defines() {
    let dir = config_dir("widgets", &[("hyprwidget.lua", "")]);
    let (_core, rt) = load(dir, None);
    let out = rt
        .eval(
            r#"(function()
                local W = hw.widget
                hw.define("Card", function(p) return W.Box({ class = "card", W.Label(p.title) }) end)
                local c = W.Card({ title = "t" })
                return c.type .. " " .. c.class .. " " .. c[1].type .. " " .. c[1].label
            end)()"#,
        )
        .unwrap();
    assert_eq!(out, "Box card Label t");
    assert!(rt.eval("hw.widget.Nope({})").is_err());
}

#[test]
fn option_signals_and_opt_proxy() {
    let dir = config_dir(
        "opt",
        &[(
            "hyprwidget.lua",
            "hw.config({ decoration = { rounding = 7 } })",
        )],
    );
    let (core, rt) = load(dir, None);
    assert_eq!(rt.eval("hw.opt.decoration.rounding:get()").unwrap(), "7");
    assert_eq!(rt.eval("hw.option('decoration:rounding')()").unwrap(), "7");
    assert_eq!(rt.eval("hw.opt.decoration().rounding").unwrap(), "7");
    let sig = rt.eval("_G.r = hw.opt.decoration.rounding:map(function(v) return v + 1 end)");
    assert!(sig.is_ok());
    core.options.set_hypr(json!({}));
    assert_eq!(rt.eval("r()").unwrap(), "8");
}

#[test]
fn demo_config_loads_headless() {
    let demo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo");
    let dir = ConfigDir::resolve(Some(demo)).unwrap();
    let (core, rt) = load(
        dir,
        Some(json!({ "general": { "gaps_in": 2, "gaps_out": 6 } })),
    );
    assert!(
        rt.st.diagnostics.borrow().is_empty(),
        "{:?}",
        rt.st.diagnostics.borrow()
    );
    assert_eq!(core.options.get("general:gaps_in"), json!(4));
}

#[test]
fn scss_partials_are_importable() {
    let dir = config_dir(
        "scss",
        &[
            (
                "hyprwidget.lua",
                r#"hw.scss("settings", "$OPACITY: 0.5;")
                   assert(not pcall(hw.scss, "../x", ""))"#,
            ),
            (
                "style/main.scss",
                "@import \"settings\";\n.a { opacity: $OPACITY; }",
            ),
        ],
    );
    let problems = crate::app::check(dir.clone()).unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    let (core, _rt) = load(dir, None);
    assert_eq!(
        core.scss_partials
            .borrow()
            .get("settings")
            .map(String::as_str),
        Some("$OPACITY: 0.5;")
    );
}

#[test]
fn json_roundtrip() {
    let dir = config_dir(
        "json",
        &[(
            "hyprwidget.lua",
            r#"local t = hw.json.decode('{"a":[1,2,{"b":"c"}],"n":null}')
               assert(t.a[3].b == "c" and t.a[2] == 2)
               assert(hw.json.decode(hw.json.encode({ x = 1.5 })).x == 1.5)
               assert(not pcall(hw.json.decode, "{"))"#,
        )],
    );
    load(dir, None);
}

#[test]
fn json_helpers() {
    let dir = config_dir("json-helpers", &[("hyprwidget.lua", "")]);
    let (_core, rt) = load(dir, None);
    let out = rt
        .eval(
            r#"(function()
                local t = hw.json.decode('{"a": [1, 2, {"b": "x"}], "n": null}')
                return t.a[3].b .. #t.a .. tostring(t.n) .. hw.json.encode({ k = "v" })
            end)()"#,
        )
        .unwrap();
    assert_eq!(out, r#"x3nil{"k":"v"}"#);
    assert!(rt.eval("hw.json.decode('{')").is_err());
}

#[test]
fn set_option_writes_widget_custom_file() {
    let dir = config_dir("set-option", &[("hyprwidget.lua", "require_custom_dir()")]);
    let root = dir.root.clone();
    let (core, rt) = load(dir.clone(), None);
    let out = rt
        .eval(r#"hw.set_option("decoration:rounding", 7, "widget")"#)
        .unwrap();
    assert_eq!(out, "widget");
    assert_eq!(core.options.get("decoration:rounding"), json!(7));
    assert_eq!(
        std::fs::read_to_string(root.join("config/custom/decoration:rounding.lua")).unwrap(),
        "hw.config({ decoration = { rounding = 7 } })\n"
    );
    assert!(rt.eval(r#"hw.set_option("../x", 1, "widget")"#).is_err());
    assert!(rt.eval(r#"hw.set_option("a:b", 1, "nope")"#).is_err());
    // The file is picked up by require_custom_dir() on the next load.
    drop(rt);
    let (core, _rt) = load(dir, None);
    assert_eq!(core.options.get("decoration:rounding"), json!(7));
}

#[test]
fn theme_palette_and_partial() {
    let dir = config_dir("theme", &[("hyprwidget.lua", "")]);
    let (core, rt) = load(dir, None);
    let out = rt
        .eval(
            r##"(function()
                hw.theme:set_source("#cc3333", "light")
                local c = hw.theme.colors()
                return hw.theme.mode() .. " " .. tostring(c.primary:match("^#%x+$") ~= nil)
            end)()"##,
        )
        .unwrap();
    assert_eq!(out, "light true");
    let partials = core.scss_partials.borrow();
    let m = partials.get("material").unwrap();
    assert!(
        m.contains("$md-mode: light;") && m.contains("$md-source: #cc3333;"),
        "{m}"
    );
}
