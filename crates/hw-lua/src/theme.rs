//! Themes (PLAN.md §3.5): `themes/<name>/{theme.json, _vars.scss, init.lua?}`,
//! selected by `"theme": "<name>"` in `config.json` and switchable at
//! runtime (`hw.theme.set(name)`, `hyprwidget theme <name>`).
//!
//! ```json
//! { "name": "nord", "version": "1.0.0", "variant": "dark",
//!   "colors": { "background": "#2e3440", "primary": "#88c0d0" },
//!   "source": "wallpaper", "permissions": ["fs"] }
//! ```
//!
//! The colors of the theme (`hw.theme.colors`, reactive) are, later wins:
//! 1. `colors` of theme.json,
//! 2. `colors` returned by the theme's `init.lua` (a table or a signal; the
//!    file runs sandboxed like a plugin, with theme.json `permissions`),
//! 3. the **color source** named by `"source"`, when one is registered.
//!
//! A color source is a reactive palette provided by Rust or Lua — e.g. the
//! wallpaper palette: [`Theme::set_source`]`("wallpaper", signal)` from Rust,
//! `hw.theme.add_source("wallpaper", signal_or_table)` from Lua. Its value
//! is a JSON object `{ "<color name>": "<css color>" }`.
//!
//! Without a theme, `hw.theme.colors` is the `wallpaper` source alone.
//!
//! The selected theme is exported to SCSS as the generated partial
//! `_hw-theme.scss` (`@import "hw-theme";`): `$theme-name`, a `!default`
//! fallback palette, `$<color>: …;` per theme color, `$theme-colors` (a
//! map), then the theme's `_vars.scss`. It is rewritten (and the CSS
//! recompiled) whenever the theme or its colors change.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hw_core::reactive::{Signal, computed, untracked};
use mlua::{Lua, MetaMethod, Result, Table, UserData, UserDataMethods, Value};
use serde_json::{Value as J, json};

use crate::convert::{json_to_lua, lua_to_json};
use crate::plugin::{self, Diags, Perms};
use crate::runtime::RtState;
use crate::signal::{JsonSetter, LuaSignal, as_signal};

pub const THEMES_DIR: &str = "themes";
pub const MANIFEST: &str = "theme.json";
/// Name of the generated partial (`@import "hw-theme";`).
pub const PARTIAL: &str = "hw-theme";
/// Color source used when no theme is selected: the wallpaper palette of
/// the `theme` service (`hw.service.theme.colors`, Material roles).
pub const WALLPAPER_SOURCE: &str = "wallpaper";

/// Used when neither the stylesheet nor the theme defines them.
const FALLBACK: &[(&str, &str)] = &[
    ("background", "#1e1e2e"),
    ("foreground", "#cdd6f4"),
    ("surface", "#313244"),
    ("primary", "#89b4fa"),
    ("secondary", "#a6adc8"),
    ("accent", "#f5c2e7"),
    ("urgent", "#f38ba8"),
];

/// A parsed `theme.json`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub variant: Option<String>,
    pub colors: BTreeMap<String, String>,
    pub source: Option<String>,
    pub permissions: Perms,
}

/// Accepts CSS colors (`#rgb[a]`, `#rrggbb[aa]`, `rgb()/rgba()/hsl()`,
/// names) and Hyprland colors (`rgb(rrggbb)`, `0xAARRGGBB`).
pub fn valid_color(s: &str) -> bool {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    if hw_core::color::parse_color(s).is_ok() {
        return true;
    }
    let lower = s.to_ascii_lowercase();
    if ["rgb(", "rgba(", "hsl(", "hsla("]
        .iter()
        .any(|p| lower.starts_with(p))
        && lower.ends_with(')')
    {
        return true;
    }
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic())
}

/// Color as CSS: Hyprland forms are converted, the rest kept.
pub fn css_color(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('#') {
        return s.to_owned();
    }
    hw_core::color::parse_color(s)
        .map(|c| c.to_string())
        .unwrap_or_else(|_| s.to_owned())
}

fn valid_color_name(k: &str) -> bool {
    !k.is_empty()
        && !k.starts_with(|c: char| c.is_ascii_digit() || c == '-')
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parses and validates `theme.json`.
pub fn parse_manifest(
    file: &str,
    text: &str,
    dir_name: &str,
) -> std::result::Result<Manifest, Vec<String>> {
    let mut d = Diags::new(file, text);
    let Some(o) = d.object() else {
        return Err(d.out);
    };
    d.unknown_keys(
        &o,
        &[
            "name",
            "version",
            "description",
            "author",
            "license",
            "variant",
            "colors",
            "source",
            "permissions",
        ],
    );
    let name = d.string(&o, "name").unwrap_or_else(|| dir_name.to_owned());
    if name != dir_name {
        d.at(
            "name",
            format!("name '{name}' does not match the directory '{dir_name}'"),
        );
    }
    let version = d.string(&o, "version");
    if let Some(v) = &version
        && plugin::parse_version(v).is_none()
    {
        d.at(
            "version",
            format!("invalid version '{v}' (expected e.g. 1.0.0)"),
        );
    }
    let description = d.string(&o, "description");
    let _ = d.string(&o, "author");
    let _ = d.string(&o, "license");
    let variant = d.string(&o, "variant");
    if let Some(v) = &variant
        && v != "dark"
        && v != "light"
    {
        d.at(
            "variant",
            format!("'variant' must be \"dark\" or \"light\", got '{v}'"),
        );
    }
    let mut colors = BTreeMap::new();
    match o.get("colors") {
        None => {}
        Some(J::Object(m)) => {
            for (k, v) in m {
                match v.as_str() {
                    _ if !valid_color_name(k) => d.at(k, format!("invalid color name '{k}'")),
                    Some(c) if valid_color(c) => {
                        colors.insert(k.clone(), c.to_owned());
                    }
                    _ => d.at(k, format!("colors.{k}: invalid color {v}")),
                }
            }
        }
        Some(other) => d.at("colors", format!("'colors' must be an object, got {other}")),
    }
    let source = d.string(&o, "source");
    if let Some(s) = &source
        && !plugin::valid_name(s)
    {
        d.at("source", format!("invalid color source name '{s}'"));
    }
    let permissions = d.perms(&o);
    if !d.out.is_empty() {
        return Err(d.out);
    }
    Ok(Manifest {
        name,
        version,
        description,
        variant,
        colors,
        source,
        permissions,
    })
}

/// Theme names found in `themes/` (sorted).
pub fn list(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root.join(THEMES_DIR))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().join(MANIFEST).is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Reads and validates `themes/<name>/theme.json`.
pub fn read(root: &Path, name: &str) -> std::result::Result<(Manifest, PathBuf), Vec<String>> {
    let file = format!("{THEMES_DIR}/{name}/{MANIFEST}");
    if !plugin::valid_name(name) {
        return Err(vec![format!("config.json: invalid theme name '{name}'")]);
    }
    let dir = root.join(THEMES_DIR).join(name);
    let text = std::fs::read_to_string(dir.join(MANIFEST)).map_err(|e| {
        vec![if e.kind() == std::io::ErrorKind::NotFound {
            format!("theme '{name}' not found ({file})")
        } else {
            format!("{file}: {e}")
        }]
    })?;
    parse_manifest(&file, &text, name).map(|m| (m, dir))
}

/// Validates every theme in `themes/` (for `hyprwidget check`).
pub fn check_all(root: &Path) -> Vec<String> {
    list(root)
        .iter()
        .filter_map(|n| read(root, n).err())
        .flatten()
        .collect()
}

/// `config.json` `"theme"`: a name, `null`, or `{ "name": … }`.
pub fn configured(v: Option<&J>) -> std::result::Result<Option<String>, String> {
    match v {
        None | Some(J::Null) => Ok(None),
        Some(J::String(s)) if s.is_empty() || s == "none" => Ok(None),
        Some(J::String(s)) => Ok(Some(s.clone())),
        Some(J::Object(o)) => match o.get("name") {
            Some(J::String(s)) => Ok(Some(s.clone())),
            None | Some(J::Null) => Ok(None),
            Some(other) => Err(format!(
                "config.json: 'theme.name' must be a string, got {other}"
            )),
        },
        Some(other) => Err(format!(
            "config.json: 'theme' must be a theme name, got {other}"
        )),
    }
}

/// Theme state; lives in `Core` so it survives Lua reloads.
pub struct Theme {
    /// Selected theme name (string) or `null`.
    pub name: Signal<J>,
    /// Merged colors (object of CSS color strings).
    pub colors: Signal<J>,
    file_colors: Signal<J>,
    lua_colors: Signal<J>,
    /// Color source the theme follows (string) or `null`; without a theme,
    /// [`WALLPAPER_SOURCE`].
    source: Signal<J>,
    sources: Rc<RefCell<HashMap<String, Signal<J>>>>,
    sources_rev: Signal<u64>,
    vars: RefCell<Option<PathBuf>>,
    /// `(config.json value, theme)` of a runtime switch: kept across Lua
    /// reloads until `config.json` names another theme.
    runtime: RefCell<Option<(Option<String>, Option<String>)>>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new()
    }
}

/// Names stylesheets/terminals expect, filled from Material roles.
const ALIASES: &[(&str, &str)] = &[("foreground", "on_background"), ("urgent", "error")];

fn merge_into(out: &mut serde_json::Map<String, J>, v: &J) {
    if let J::Object(m) = v {
        for (k, v) in m {
            if let Some(s) = v.as_str()
                && valid_color_name(k)
                && valid_color(s)
            {
                out.insert(k.clone(), J::String(css_color(s)));
            }
        }
    }
}

impl Theme {
    pub fn new() -> Self {
        let file_colors = Signal::new(json!({}));
        let lua_colors = Signal::new(json!({}));
        let source = Signal::new(J::String(WALLPAPER_SOURCE.into()));
        let sources: Rc<RefCell<HashMap<String, Signal<J>>>> = Rc::default();
        let sources_rev = Signal::new(0u64);
        let colors = {
            let (f, l, s, map, rev) = (
                file_colors.clone(),
                lua_colors.clone(),
                source.clone(),
                sources.clone(),
                sources_rev.clone(),
            );
            computed(move || {
                let mut out = serde_json::Map::new();
                f.with(|v| merge_into(&mut out, v));
                l.with(|v| merge_into(&mut out, v));
                rev.get();
                if let Some(name) = s.get().as_str() {
                    let sig = map.borrow().get(name).cloned();
                    if let Some(sig) = sig {
                        sig.with(|v| merge_into(&mut out, v));
                    }
                }
                for (alias, role) in ALIASES {
                    if !out.contains_key(*alias)
                        && let Some(v) = out.get(*role).cloned()
                    {
                        out.insert((*alias).to_owned(), v);
                    }
                }
                J::Object(out)
            })
        };
        Self {
            name: Signal::new(J::Null),
            colors,
            file_colors,
            lua_colors,
            source,
            sources,
            sources_rev,
            vars: RefCell::default(),
            runtime: RefCell::default(),
        }
    }

    /// Registers (or replaces) the color source `name`. Themes with
    /// `"source": "<name>"` take their colors from it (over their own).
    /// The value is a JSON object `{ "primary": "#88c0d0", … }`.
    pub fn set_source(&self, name: &str, colors: Signal<J>) {
        self.sources.borrow_mut().insert(name.to_owned(), colors);
        self.sources_rev.update(|r| *r += 1);
    }

    pub fn remove_source(&self, name: &str) {
        if self.sources.borrow_mut().remove(name).is_some() {
            self.sources_rev.update(|r| *r += 1);
        }
    }

    pub fn source_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.sources.borrow().keys().cloned().collect();
        v.sort();
        v
    }

    /// Contents of `_hw-theme.scss` for the current theme.
    pub fn scss(&self) -> String {
        let name = self.name.peek();
        let colors = self.colors.peek();
        let mut out =
            String::from("// Generated by hyprwidget from the selected theme; do not edit.\n");
        out.push_str(&match name.as_str() {
            Some(n) => format!("$theme-name: \"{n}\";\n"),
            None => "$theme-name: null;\n".into(),
        });
        for (k, v) in FALLBACK {
            out.push_str(&format!("${k}: {v} !default;\n"));
        }
        let mut map = Vec::new();
        // Without a theme the stylesheet keeps its own palette (the colors
        // still reach Lua as `hw.theme.colors`).
        if let (J::Object(m), false) = (&colors, name.is_null()) {
            for (k, v) in m {
                let v = v.as_str().unwrap_or_default();
                out.push_str(&format!("${k}: {v};\n"));
                map.push(format!("\"{k}\": {v}"));
            }
        }
        out.push_str(&format!("$theme-colors: ({});\n", map.join(", ")));
        if let Some(vars) = self.vars.borrow().as_ref() {
            // `_vars.scss` → `@import "<dir>/vars"`.
            let dir = vars.parent().unwrap_or(Path::new("."));
            out.push_str(&format!("@import \"{}/vars\";\n", dir.display()));
        }
        out
    }

    fn clear(&self) {
        *self.vars.borrow_mut() = None;
        self.file_colors.set(json!({}));
        self.lua_colors.set(json!({}));
        self.source.set(J::String(WALLPAPER_SOURCE.into()));
        self.name.set(J::Null);
    }

    /// Theme to use for a `config.json` value, honoring a runtime switch.
    fn effective(&self, configured: Option<String>) -> Option<String> {
        let mut rt = self.runtime.borrow_mut();
        match rt.as_ref() {
            Some((cfg, chosen)) if *cfg == configured => chosen.clone(),
            Some(_) => {
                *rt = None;
                configured
            }
            None => configured,
        }
    }
}

/// Per-Lua-state theme data: things owned by the current theme's init.lua.
#[derive(Default)]
struct ThemeRt {
    kept: RefCell<Vec<u64>>,
}

fn drop_theme_owned(lua: &Lua) -> Result<()> {
    let st = RtState::get(lua)?;
    let ids: Vec<u64> = lua
        .app_data_ref::<ThemeRt>()
        .map(|t| std::mem::take(&mut *t.kept.borrow_mut()))
        .unwrap_or_default();
    for id in ids {
        st.unkeep(id);
    }
    Ok(())
}

fn configured_name(lua: &Lua) -> std::result::Result<Option<String>, String> {
    let st = RtState::get(lua).map_err(|e| e.to_string())?;
    let json = st.core.dir.load_json().map_err(|e| format!("{e:#}"))?;
    configured(json.get("theme"))
}

/// Applies the configured theme (called before plugins and the config).
pub fn load(lua: &Lua) -> Result<()> {
    let st = RtState::get(lua)?;
    let name = match configured_name(lua) {
        Ok(n) => n,
        Err(e) => {
            plugin::report(lua, "theme", &e);
            None
        }
    };
    let name = st.core.theme.effective(name);
    apply(lua, name.as_deref())
}

/// Switches to `name` (`None` = no theme) at runtime.
pub fn switch(lua: &Lua, name: Option<&str>) -> Result<()> {
    let st = RtState::get(lua)?;
    if let Some(n) = name {
        read(&st.core.dir.root, n).map_err(|e| mlua::Error::runtime(e.join("\n")))?;
    }
    let cfg = configured_name(lua).ok().flatten();
    *st.core.theme.runtime.borrow_mut() = Some((cfg, name.map(str::to_owned)));
    apply(lua, name)?;
    tracing::info!(theme = ?name, "theme switched");
    Ok(())
}

fn apply(lua: &Lua, name: Option<&str>) -> Result<()> {
    let st = RtState::get(lua)?;
    let theme = &st.core.theme;
    drop_theme_owned(lua)?;
    let Some(name) = name else {
        theme.clear();
        return Ok(());
    };
    let (m, dir) = match read(&st.core.dir.root, name) {
        Ok(r) => r,
        Err(e) => {
            plugin::report(lua, &format!("theme '{name}'"), &e.join("\n"));
            theme.clear();
            return Ok(());
        }
    };
    let vars = dir.join("_vars.scss");
    *theme.vars.borrow_mut() = vars.is_file().then_some(vars);
    theme
        .file_colors
        .set(serde_json::to_value(&m.colors).unwrap_or_default());
    theme.lua_colors.set(json!({}));
    theme
        .source
        .set(m.source.clone().map_or(J::Null, J::String));
    let init = dir.join("init.lua");
    if init.is_file()
        && let Err(e) = run_init(lua, &m, &dir, &init)
    {
        plugin::report(lua, &format!("theme '{name}'"), &e.to_string());
    }
    theme.name.set(J::String(name.to_owned()));
    Ok(())
}

/// Runs `init.lua` sandboxed; it may return `{ colors = table|signal }`.
fn run_init(lua: &Lua, m: &Manifest, dir: &Path, init: &Path) -> Result<()> {
    let spec = plugin::spec(
        lua,
        &format!("theme:{}", m.name),
        m.version.as_deref().unwrap_or("0"),
        dir,
        "init.lua",
        m.permissions,
        &[],
    )?;
    let ret = plugin::run_sandboxed(lua, &spec, init)?;
    let t = match ret {
        Value::Nil => return Ok(()),
        Value::Table(t) => t,
        other => {
            return Err(mlua::Error::runtime(format!(
                "init.lua must return a table, got {}",
                other.type_name()
            )));
        }
    };
    for k in t.pairs::<Value, Value>() {
        let (k, _) = k?;
        if !matches!(&k, Value::String(s) if s.to_str()? == "colors") {
            return Err(mlua::Error::runtime(format!(
                "init.lua: unknown key '{}' in the returned table (colors)",
                crate::convert::display(lua, k)
            )));
        }
    }
    let colors: Value = t.get("colors")?;
    let st = RtState::get(lua)?;
    let target = st.core.theme.lua_colors.clone();
    let id = bridge(lua, &colors, target)?;
    if let (Some(id), Some(rt)) = (id, lua.app_data_ref::<ThemeRt>()) {
        rt.kept.borrow_mut().push(id);
    }
    Ok(())
}

/// Copies a Lua table or signal into `target` (following the signal);
/// returns the id of the kept subscription.
fn bridge(lua: &Lua, v: &Value, target: Signal<J>) -> Result<Option<u64>> {
    match as_signal(v) {
        Some(sig) => {
            let weak = lua.weak();
            let get = {
                let sig = sig.clone();
                move || -> Option<J> {
                    let lua = weak.try_upgrade()?;
                    untracked(|| sig.0.get(&lua))
                        .and_then(|v| lua_to_json(&lua, v))
                        .ok()
                }
            };
            target.set(get().unwrap_or_else(|| json!({})));
            let sub = sig.0.subscribe(Box::new(move || {
                if let Some(v) = get() {
                    target.set(v);
                }
            }));
            Ok(Some(RtState::get(lua)?.keep(Box::new(sub))))
        }
        None => {
            target.set(match v {
                Value::Nil => json!({}),
                v => lua_to_json(lua, v.clone())?,
            });
            Ok(None)
        }
    }
}

/// Removes a Lua color source when the Lua state goes away.
struct SourceGuard {
    core: std::rc::Weak<crate::runtime::Core>,
    name: String,
}

impl Drop for SourceGuard {
    fn drop(&mut self) {
        if let Some(core) = self.core.upgrade() {
            core.theme.remove_source(&self.name);
        }
    }
}

/// `hw.theme.colors`: `.primary` → signal, `()` → table.
struct ColorsProxy;

impl UserData for ColorsProxy {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_meta_method(MetaMethod::Index, |lua, _, key: String| {
            let st = RtState::get(lua)?;
            let k = key.clone();
            let sig = st
                .core
                .theme
                .colors
                .map(move |c| c.get(&k).cloned().unwrap_or(J::Null));
            Ok(LuaSignal::json(sig, None))
        });
        m.add_meta_method(MetaMethod::Call, |lua, _, ()| {
            json_to_lua(lua, &RtState::get(lua)?.core.theme.colors.get())
        });
        m.add_meta_method(MetaMethod::ToString, |lua, _, ()| {
            let c = RtState::get(lua)?.core.theme.colors.peek();
            Ok(format!("ThemeColors({c})"))
        });
    }
}

fn name_arg(v: Value) -> Result<Option<String>> {
    match v {
        Value::Nil | Value::Boolean(false) => Ok(None),
        Value::String(s) if s.to_str()?.is_empty() || s.to_str()? == "none" => Ok(None),
        Value::String(s) => Ok(Some(s.to_str()?.to_string())),
        other => Err(mlua::Error::runtime(format!(
            "theme name must be a string or nil, got {}",
            other.type_name()
        ))),
    }
}

/// Installs `hw.theme`.
pub fn install(lua: &Lua, hw: &Table) -> Result<()> {
    lua.set_app_data(ThemeRt::default());
    let st = RtState::get(lua)?;
    let t = lua.create_table()?;
    let weak = lua.weak();
    let setter: JsonSetter = Rc::new(move |v| {
        let lua = weak
            .try_upgrade()
            .ok_or_else(|| anyhow::anyhow!("Lua state gone"))?;
        let name = v.as_str().filter(|s| !s.is_empty() && *s != "none");
        switch(&lua, name).map_err(|e| anyhow::anyhow!("{e}"))
    });
    t.set(
        "name",
        LuaSignal::json(st.core.theme.name.clone(), Some(setter)),
    )?;
    t.set("colors", ColorsProxy)?;
    t.set(
        "palette",
        LuaSignal::json(st.core.theme.colors.clone(), None),
    )?;
    t.set(
        "set",
        lua.create_function(|lua, v: Value| switch(lua, name_arg(v)?.as_deref()))?,
    )?;
    t.set(
        "list",
        lua.create_function(|lua, ()| Ok(list(&RtState::get(lua)?.core.dir.root)))?,
    )?;
    t.set(
        "get",
        lua.create_function(|lua, ()| {
            let st = RtState::get(lua)?;
            let r = lua.create_table()?;
            r.set("name", json_to_lua(lua, &st.core.theme.name.peek())?)?;
            r.set("colors", json_to_lua(lua, &st.core.theme.colors.peek())?)?;
            r.set("source", json_to_lua(lua, &st.core.theme.source.peek())?)?;
            Ok(r)
        })?,
    )?;
    t.set(
        "sources",
        lua.create_function(|lua, ()| Ok(RtState::get(lua)?.core.theme.source_names()))?,
    )?;
    // hw.theme.add_source(name, table_or_signal): a color source for
    // themes with `"source": "<name>"`; removed on Lua reload.
    t.set(
        "add_source",
        lua.create_function(|lua, (name, v): (String, Value)| {
            if !plugin::valid_name(&name) {
                return Err(mlua::Error::runtime(format!(
                    "invalid color source name '{name}'"
                )));
            }
            let st = RtState::get(lua)?;
            let target = Signal::new(json!({}));
            bridge(lua, &v, target.clone())?;
            st.core.theme.set_source(&name, target);
            st.keep(Box::new(SourceGuard {
                core: Rc::downgrade(&st.core),
                name,
            }));
            Ok(())
        })?,
    )?;
    attach_service(lua, &st, &t)?;
    hw.set("theme", t)?;
    Ok(())
}

/// Ties `hw.theme` to the `theme` service (wallpaper palette) when it
/// exists: its `colors` become the `wallpaper` color source, and keys not
/// defined here (`mode`, `wallpaper`, `set_wallpaper`…) are forwarded, so
/// `hw.theme:set_wallpaper(path)` works.
fn attach_service(lua: &Lua, st: &RtState, t: &Table) -> Result<()> {
    if !hw_services::NAMES.contains(&"theme") {
        return Ok(());
    }
    let svc = match st.core.services.get("theme") {
        Ok(Some(svc)) => svc,
        Ok(None) => return Ok(()),
        Err(e) => {
            tracing::warn!("{e}");
            return Ok(());
        }
    };
    if let Some(colors) = svc.prop("colors") {
        st.core.theme.set_source(WALLPAPER_SOURCE, colors.clone());
    }
    let ud = lua.create_userdata(crate::service::LuaService(svc))?;
    let mt = lua.create_table()?;
    let this = t.clone();
    mt.set(
        "__index",
        lua.create_function(move |lua, (_, k): (Value, Value)| {
            use mlua::ObjectLike;
            let v: Value = ud.get(k)?;
            let Value::Function(f) = v else { return Ok(v) };
            // `hw.theme:method(…)`: drop our table as `self`.
            let this = this.clone();
            lua.create_function(move |_, args: mlua::Variadic<Value>| {
                let mut it = args.into_iter().peekable();
                if matches!(it.peek(), Some(Value::Table(t)) if *t == this) {
                    it.next();
                }
                f.call::<mlua::MultiValue>(mlua::MultiValue::from_iter(it))
            })
            .map(Value::Function)
        })?,
    )?;
    t.set_metatable(Some(mt))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert!(valid_color("#88c0d0"));
        assert!(valid_color("rgba(1, 2, 3, 0.5)"));
        assert!(valid_color("rgb(88c0d0)"));
        assert!(valid_color("transparent"));
        assert!(!valid_color("#12"));
        assert!(!valid_color("not a color"));
        assert_eq!(
            css_color("0xff112233"),
            hw_core::color::parse_color("0xff112233")
                .unwrap()
                .to_string()
        );
        assert_eq!(css_color("#abc"), "#abc");
    }

    #[test]
    fn manifest_errors_have_lines() {
        let text = "{\n  \"name\": \"nord\",\n  \"colors\": {\n    \"primary\": \"bogus color\"\n  },\n  \"permissions\": [\"root\"]\n}";
        let e = parse_manifest("themes/nord/theme.json", text, "nord").unwrap_err();
        assert!(
            e.iter()
                .any(|l| l.starts_with("themes/nord/theme.json:4: ")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|l| l.starts_with("themes/nord/theme.json:6: ")),
            "{e:?}"
        );
        let e = parse_manifest("t.json", "{\n \"name\": 1,", "x").unwrap_err();
        assert!(e[0].starts_with("t.json:2:"), "{e:?}");
    }

    #[test]
    fn source_overrides_theme_colors() {
        let t = Theme::new();
        t.file_colors
            .set(json!({"primary": "#111111", "background": "#000000"}));
        t.source.set(json!("wallpaper"));
        assert_eq!(t.colors.peek()["primary"], "#111111");
        let wp = Signal::new(json!({"primary": "#abcdef"}));
        t.set_source("wallpaper", wp.clone());
        assert_eq!(t.colors.peek()["primary"], "#abcdef");
        assert_eq!(t.colors.peek()["background"], "#000000");
        wp.set(json!({"primary": "#fedcba"}));
        assert_eq!(t.colors.peek()["primary"], "#fedcba");
        t.remove_source("wallpaper");
        assert_eq!(t.colors.peek()["primary"], "#111111");
        assert!(
            !t.scss().contains("$primary: #111111;"),
            "no theme: palette kept"
        );
        t.name.set(json!("x"));
        let scss = t.scss();
        assert!(scss.contains("$primary: #111111;"), "{scss}");
        assert!(scss.contains("$urgent: #f38ba8 !default;"), "{scss}");
    }
}
