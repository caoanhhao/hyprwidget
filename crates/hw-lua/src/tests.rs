use std::path::{Path, PathBuf};

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
    let core = Core::new(dir, None, false).unwrap();
    if let Some(h) = hypr {
        core.options.set_hypr(h);
    }
    let rt = Runtime::new(core.clone()).unwrap();
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
                r#"hw.config({ decoration = { rounding = 8, blur = { size = 3 } } })"#,
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
