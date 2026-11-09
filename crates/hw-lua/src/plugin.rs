//! Plugins (PLAN.md §3.5): `plugins/<name>/{plugin.json, init.lua,
//! style.scss}` in the config dir.
//!
//! ```json
//! { "name": "sysmon", "version": "1.0.0", "entry": "init.lua",
//!   "permissions": ["exec"], "dependencies": { "other": ">=1.0" },
//!   "widgets": ["Sysmon"] }
//! ```
//!
//! `init.lua` runs in its own sandboxed `_ENV` (see `plugin.lua`) and
//! returns `{ widgets = {...}, services = {...}, requests = {...}, setup =
//! fn }`. Widgets become `W.<Name>` and JSON `{"type": "<Name>"}`, services
//! `hw.service.<name>`, requests `hyprwidget request <name>`. `style.scss`
//! is compiled before the user's stylesheet with `hypr` and `hw-theme`
//! imported. Errors are logged, reported by `hyprwidget check`, shown as a
//! notification, and never stop other plugins or the config.
//!
//! `config.json` selects plugins: `"plugins": ["a", "b"]` (only these),
//! or `{ "enabled": [...], "disabled": [...] }`; default = all found.
//! Each plugin directory is watched on its own: editing a plugin reloads
//! just that plugin and rebuilds the `per_monitor` windows.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use hw_ui::gtk4::{gio, glib};
use mlua::{Function, Lua, Result, Table, Value};
use serde_json::Value as J;

use crate::runtime::{RtState, compact_error};

const LOADER: &str = include_str!("plugin.lua");
pub const PLUGINS_DIR: &str = "plugins";
pub const MANIFEST: &str = "plugin.json";

/// Capabilities a plugin (or theme `init.lua`) may be granted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Perms {
    pub exec: bool,
    pub network: bool,
    pub fs: bool,
}

pub const PERMISSIONS: &[&str] = &["exec", "network", "fs"];

impl Perms {
    fn to_lua(self, lua: &Lua) -> Result<Table> {
        let t = lua.create_table()?;
        t.set("exec", self.exec)?;
        t.set("network", self.network)?;
        t.set("fs", self.fs)?;
        Ok(t)
    }
}

/// A parsed `plugin.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub entry: String,
    pub style: String,
    pub permissions: Perms,
    /// `(name, version requirement)`; `"*"` when given as a list.
    pub dependencies: Vec<(String, String)>,
    /// Widgets that get error placeholders even if `init.lua` fails.
    pub widgets: Vec<String>,
}

/// 1-based line of the first `"key"` in `text` (for `file:line`).
pub fn line_of(text: &str, key: &str) -> usize {
    let needle = format!("\"{key}\"");
    text.lines()
        .position(|l| l.contains(&needle))
        .map_or(1, |i| i + 1)
}

/// Collects `file:line: message` diagnostics for a JSON manifest.
pub struct Diags<'a> {
    pub file: &'a str,
    pub text: &'a str,
    pub out: Vec<String>,
}

impl<'a> Diags<'a> {
    pub fn new(file: &'a str, text: &'a str) -> Self {
        Self {
            file,
            text,
            out: Vec::new(),
        }
    }

    pub fn at(&mut self, key: &str, msg: impl std::fmt::Display) {
        let line = line_of(self.text, key);
        self.out.push(format!("{}:{line}: {msg}", self.file));
    }

    /// Parses the file as a JSON object (syntax errors with line/column).
    pub fn object(&mut self) -> Option<serde_json::Map<String, J>> {
        match serde_json::from_str::<J>(self.text) {
            Ok(J::Object(o)) => Some(o),
            Ok(_) => {
                self.out
                    .push(format!("{}:1: top level must be an object", self.file));
                None
            }
            Err(e) => {
                self.out
                    .push(format!("{}:{}:{}: {e}", self.file, e.line(), e.column()));
                None
            }
        }
    }

    pub fn string(&mut self, o: &serde_json::Map<String, J>, key: &str) -> Option<String> {
        match o.get(key)? {
            J::String(s) => Some(s.clone()),
            other => {
                self.at(key, format!("'{key}' must be a string, got {other}"));
                None
            }
        }
    }

    pub fn strings(&mut self, o: &serde_json::Map<String, J>, key: &str) -> Vec<String> {
        match o.get(key) {
            None => Vec::new(),
            Some(J::Array(a)) if a.iter().all(J::is_string) => a
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect(),
            Some(other) => {
                self.at(
                    key,
                    format!("'{key}' must be a list of strings, got {other}"),
                );
                Vec::new()
            }
        }
    }

    pub fn unknown_keys(&mut self, o: &serde_json::Map<String, J>, known: &[&str]) {
        for k in o.keys() {
            if !known.contains(&k.as_str()) {
                self.at(
                    k,
                    format!("unknown key '{k}' (expected one of: {})", known.join(", ")),
                );
            }
        }
    }

    /// `permissions`: a list of [`PERMISSIONS`].
    pub fn perms(&mut self, o: &serde_json::Map<String, J>) -> Perms {
        let mut p = Perms::default();
        for name in self.strings(o, "permissions") {
            match name.as_str() {
                "exec" => p.exec = true,
                "network" => p.network = true,
                "fs" => p.fs = true,
                other => self.at(
                    "permissions",
                    format!(
                        "unknown permission '{other}' (expected {})",
                        PERMISSIONS.join(", ")
                    ),
                ),
            }
        }
        p
    }
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn valid_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.split('/').any(|p| p == "..")
}

/// `1.2.3` → `[1, 2, 3]` (missing parts = 0); `None` if not numeric.
pub fn parse_version(v: &str) -> Option<[u64; 3]> {
    let core = v.trim().split(['-', '+']).next()?;
    let mut out = [0; 3];
    let mut n = 0;
    for (i, p) in core.split('.').enumerate() {
        if i >= 3 {
            return None;
        }
        out[i] = p.parse().ok()?;
        n += 1;
    }
    (n > 0).then_some(out)
}

/// `*`, `1.2`, `=1.2`, `>=1.2`, `>1.2`, `<2`, `<=2`, `^1.2` (same major),
/// `~1.2` (same major.minor).
pub fn version_matches(req: &str, version: &str) -> Option<bool> {
    let req = req.trim();
    if req.is_empty() || req == "*" {
        return Some(true);
    }
    let v = parse_version(version)?;
    let (op, rest) = ["^", "~", ">=", "<=", ">", "<", "="]
        .iter()
        .find_map(|op| req.strip_prefix(op).map(|r| (*op, r)))
        .unwrap_or(("=", req));
    let r = parse_version(rest)?;
    Some(match op {
        "^" => v[0] == r[0] && v >= r,
        "~" => v[0] == r[0] && v[1] == r[1] && v >= r,
        ">=" => v >= r,
        "<=" => v <= r,
        ">" => v > r,
        "<" => v < r,
        _ => {
            // `1.2` matches 1.2.x.
            let parts = rest.trim().split('.').count().min(3);
            v[..parts] == r[..parts]
        }
    })
}

/// Parses and validates `plugin.json`. `file` is the path shown in
/// diagnostics; `dir` the plugin directory (to check `entry`).
pub fn parse_manifest(
    file: &str,
    text: &str,
    dir: &Path,
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
            "entry",
            "style",
            "permissions",
            "dependencies",
            "widgets",
        ],
    );
    let name = d.string(&o, "name");
    match &name {
        None if !o.contains_key("name") => d.out.push(format!("{file}:1: missing 'name'")),
        Some(n) if !valid_name(n) => d.at(
            "name",
            format!("invalid plugin name '{n}' (letters, digits, '_' and '-')"),
        ),
        _ => {}
    }
    let version = d.string(&o, "version");
    match &version {
        None if !o.contains_key("version") => d.out.push(format!("{file}:1: missing 'version'")),
        Some(v) if parse_version(v).is_none() => d.at(
            "version",
            format!("invalid version '{v}' (expected e.g. 1.0.0)"),
        ),
        _ => {}
    }
    let description = d.string(&o, "description");
    let _ = d.string(&o, "author");
    let _ = d.string(&o, "license");
    let entry = d.string(&o, "entry").unwrap_or_else(|| "init.lua".into());
    if !valid_rel_path(&entry) {
        d.at(
            "entry",
            format!("'entry' must be a relative path inside the plugin, got '{entry}'"),
        );
    } else if !dir.join(&entry).is_file() {
        let key = if o.contains_key("entry") {
            "entry"
        } else {
            "name"
        };
        d.at(key, format!("entry file '{entry}' not found"));
    }
    let style = d.string(&o, "style").unwrap_or_else(|| "style.scss".into());
    if !valid_rel_path(&style) {
        d.at(
            "style",
            format!("'style' must be a relative path inside the plugin, got '{style}'"),
        );
    } else if o.contains_key("style") && !dir.join(&style).is_file() {
        d.at("style", format!("style file '{style}' not found"));
    }
    let permissions = d.perms(&o);
    let mut dependencies = Vec::new();
    match o.get("dependencies") {
        None => {}
        Some(J::Array(_)) => {
            for n in d.strings(&o, "dependencies") {
                dependencies.push((n, "*".to_owned()));
            }
        }
        Some(J::Object(m)) => {
            for (n, req) in m {
                match req.as_str() {
                    Some(r) if version_matches(r, "0.0.0").is_some() => {
                        dependencies.push((n.clone(), r.to_owned()))
                    }
                    _ => d.at(
                        n,
                        format!("dependency '{n}': invalid version requirement {req}"),
                    ),
                }
            }
        }
        Some(other) => d.at(
            "dependencies",
            format!("'dependencies' must be a list or an object, got {other}"),
        ),
    }
    let widgets = d.strings(&o, "widgets");
    for w in &widgets {
        if w.is_empty()
            || w.starts_with(|c: char| c.is_ascii_digit())
            || !w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            || hw_ui::widgets::Kind::from_name(w).is_some()
        {
            d.at("widgets", format!("invalid widget name '{w}'"));
        }
    }
    if !d.out.is_empty() {
        return Err(d.out);
    }
    Ok(Manifest {
        name: name.unwrap_or_default(),
        version: version.unwrap_or_default(),
        description,
        entry,
        style,
        permissions,
        dependencies,
        widgets,
    })
}

/// Which plugins `config.json` enables.
#[derive(Debug, Clone, Default)]
pub struct Selection {
    /// `None` = every plugin found.
    pub enabled: Option<Vec<String>>,
    pub disabled: Vec<String>,
}

impl Selection {
    pub fn from_config(v: Option<&J>) -> std::result::Result<Self, String> {
        let names = |v: &J, what: &str| -> std::result::Result<Vec<String>, String> {
            v.as_array()
                .filter(|a| a.iter().all(J::is_string))
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_owned))
                        .collect()
                })
                .ok_or_else(|| format!("config.json: '{what}' must be a list of plugin names"))
        };
        match v {
            None | Some(J::Null) => Ok(Self::default()),
            Some(a @ J::Array(_)) => Ok(Self {
                enabled: Some(names(a, "plugins")?),
                disabled: Vec::new(),
            }),
            Some(J::Object(o)) => {
                if let Some(k) = o.keys().find(|k| *k != "enabled" && *k != "disabled") {
                    return Err(format!(
                        "config.json: unknown key 'plugins.{k}' (expected enabled, disabled)"
                    ));
                }
                Ok(Self {
                    enabled: o
                        .get("enabled")
                        .map(|v| names(v, "plugins.enabled"))
                        .transpose()?,
                    disabled: o
                        .get("disabled")
                        .map(|v| names(v, "plugins.disabled"))
                        .transpose()?
                        .unwrap_or_default(),
                })
            }
            Some(other) => Err(format!(
                "config.json: 'plugins' must be a list or {{\"enabled\", \"disabled\"}}, got {other}"
            )),
        }
    }

    fn wants(&self, name: &str) -> bool {
        !self.disabled.iter().any(|d| d == name)
            && self
                .enabled
                .as_ref()
                .is_none_or(|e| e.iter().any(|n| n == name))
    }
}

/// A plugin found on disk.
#[derive(Debug, Clone)]
pub struct Found {
    /// Directory name.
    pub id: String,
    pub dir: PathBuf,
    /// Manifest path relative to the config dir.
    pub file: String,
    pub manifest: std::result::Result<Manifest, Vec<String>>,
}

/// Reads every `plugins/*/plugin.json` (sorted by directory name).
pub fn discover(root: &Path) -> Vec<Found> {
    let Ok(rd) = std::fs::read_dir(root.join(PLUGINS_DIR)) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        })
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| {
            let id = dir.file_name().unwrap().to_string_lossy().into_owned();
            let file = format!("{PLUGINS_DIR}/{id}/{MANIFEST}");
            let manifest = match std::fs::read_to_string(dir.join(MANIFEST)) {
                Ok(text) => parse_manifest(&file, &text, &dir).and_then(|m| {
                    if m.name == id {
                        Ok(m)
                    } else {
                        Err(vec![format!(
                            "{file}:{}: name '{}' does not match the directory '{id}'",
                            line_of(&text, "name"),
                            m.name
                        )])
                    }
                }),
                Err(e) => Err(vec![format!("{file}: {e}")]),
            };
            Found {
                id,
                dir,
                file,
                manifest,
            }
        })
        .collect()
}

/// Selected plugins in load order (dependencies first) and the problems
/// found (`file:line: …`, one per skipped plugin).
/// Plugins to load (manifest, directory) and `(plugin, error)` pairs.
pub type Plan = (Vec<(Manifest, PathBuf)>, Vec<(String, String)>);

pub fn plan(found: &[Found], sel: &Selection) -> Plan {
    let mut errors: Vec<(String, String)> = Vec::new();
    let mut ok: BTreeMap<String, (&Found, &Manifest)> = BTreeMap::new();
    for f in found.iter().filter(|f| sel.wants(&f.id)) {
        match &f.manifest {
            Ok(m) => {
                ok.insert(m.name.clone(), (f, m));
            }
            Err(e) => errors.push((f.id.clone(), e.join("\n"))),
        }
    }
    if let Some(en) = &sel.enabled {
        for n in en {
            if !found.iter().any(|f| &f.id == n) {
                errors.push((
                    n.clone(),
                    format!("config.json: plugin '{n}' not found in {PLUGINS_DIR}/"),
                ));
            }
        }
    }
    // Drop plugins whose dependencies are missing or mismatched, until stable.
    loop {
        let mut bad = Vec::new();
        for (name, (f, m)) in &ok {
            for (dep, req) in &m.dependencies {
                let msg = match ok.get(dep) {
                    None => Some(format!("dependency '{dep}' is not available")),
                    Some((_, dm)) if version_matches(req, &dm.version) != Some(true) => Some(
                        format!("dependency '{dep}' {} does not match '{req}'", dm.version),
                    ),
                    _ => None,
                };
                if let Some(msg) = msg {
                    let text = std::fs::read_to_string(f.dir.join(MANIFEST)).unwrap_or_default();
                    bad.push((
                        name.clone(),
                        format!("{}:{}: {msg}", f.file, line_of(&text, dep)),
                    ));
                    break;
                }
            }
        }
        if bad.is_empty() {
            break;
        }
        for (name, msg) in bad {
            ok.remove(&name);
            errors.push((name, msg));
        }
    }
    // Topological order (alphabetical among independent plugins).
    let mut order = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    let mut visiting: HashSet<String> = HashSet::new();
    fn visit(
        n: &str,
        ok: &BTreeMap<String, (&Found, &Manifest)>,
        done: &mut HashSet<String>,
        visiting: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> bool {
        if done.contains(n) {
            return true;
        }
        if !visiting.insert(n.to_owned()) {
            return false;
        }
        let (_, m) = ok[n];
        for (dep, _) in &m.dependencies {
            if !visit(dep, ok, done, visiting, order) {
                return false;
            }
        }
        visiting.remove(n);
        done.insert(n.to_owned());
        order.push(n.to_owned());
        true
    }
    let names: Vec<String> = ok.keys().cloned().collect();
    for n in names {
        visiting.clear();
        if !visit(&n, &ok, &mut done, &mut visiting, &mut order) {
            let (f, _) = ok[&n];
            errors.push((n.clone(), format!("{}:1: dependency cycle", f.file)));
            done.insert(n);
        }
    }
    let cyclic: HashSet<&String> = errors.iter().map(|(n, _)| n).collect();
    let list = order
        .iter()
        .filter(|n| !cyclic.contains(n))
        .map(|n| {
            let (f, m) = ok[n];
            (m.clone(), f.dir.clone())
        })
        .collect();
    (list, errors)
}

/// Per-Lua-state plugin data (Lua app data).
#[derive(Default)]
struct PluginRt {
    /// Loaded plugins: name → (manifest, dir).
    loaded: RefCell<BTreeMap<String, (Manifest, PathBuf)>>,
    /// Plugins that failed (name → error).
    failed: RefCell<BTreeMap<String, String>>,
    /// Service name → owning plugin.
    services: RefCell<HashMap<String, String>>,
    /// Errors already notified (no notification storms).
    notified: RefCell<HashSet<String>>,
}

fn prt(lua: &Lua) -> Result<mlua::AppDataRef<'_, PluginRt>> {
    lua.app_data_ref::<PluginRt>()
        .ok_or_else(|| mlua::Error::runtime("plugin runtime missing"))
}

/// Shows a desktop notification (UI mode only) through `notify-send`, so it
/// works whichever process is the notification daemon (us included).
pub fn notify(summary: &str, body: &str) {
    let argv = [
        "notify-send",
        "-a",
        "hyprwidget",
        "-u",
        "critical",
        "-i",
        "dialog-error",
        summary,
        body,
    ];
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    match gio::Subprocess::newv(&argv, gio::SubprocessFlags::NONE) {
        Ok(p) => p.wait_async(gio::Cancellable::NONE, |_| {}),
        Err(e) => tracing::warn!("notify-send: {e}"),
    }
}

/// Logs a plugin (or theme) error, records it for `hyprwidget check` and
/// notifies the user once per distinct message.
pub fn report(lua: &Lua, who: &str, msg: &str) {
    let msg = compact_error(msg);
    let Ok(st) = RtState::get(lua) else { return };
    // `check` prints the diagnostics itself.
    if st.core.ui.is_some() {
        tracing::error!(target: "plugin", "{who}: {msg}");
    }
    st.diagnostics.borrow_mut().push(
        if msg.starts_with(PLUGINS_DIR) || msg.starts_with("themes/") {
            msg.clone()
        } else {
            format!("{who}: {msg}")
        },
    );
    if st.core.ui.is_none() {
        return;
    }
    if let Ok(p) = prt(lua)
        && p.notified.borrow_mut().insert(format!("{who}\n{msg}"))
    {
        let body: String = msg.lines().take(4).collect::<Vec<_>>().join("\n");
        notify(&format!("hyprwidget: {who} failed"), &body);
    }
}

/// `{ name, version, dir, entry, entry_name, permissions, widgets }` for
/// `plugin.lua`.
pub fn spec(
    lua: &Lua,
    name: &str,
    version: &str,
    dir: &Path,
    entry: &str,
    perms: Perms,
    widgets: &[String],
) -> Result<Table> {
    let t = lua.create_table()?;
    t.set("name", name)?;
    t.set("version", version)?;
    t.set("dir", dir.display().to_string())?;
    t.set("entry", dir.join(entry).display().to_string())?;
    t.set("entry_name", entry)?;
    t.set("permissions", perms.to_lua(lua)?)?;
    t.set("widgets", widgets.to_vec())?;
    Ok(t)
}

/// `@<path relative to the config dir>`, so errors read `plugins/x/init.lua:3`.
fn chunk_name(lua: &Lua, path: &Path) -> String {
    let root = RtState::get(lua).map(|st| st.core.dir.root.clone()).ok();
    let shown = root
        .as_deref()
        .and_then(|r| path.strip_prefix(r).ok())
        .unwrap_or(path);
    format!("@{}", shown.display())
}

/// Installs the loader (`hw.plugins`, private helpers).
pub fn install(lua: &Lua, hw: &Table) -> Result<()> {
    lua.set_app_data(PluginRt::default());
    let p = lua.create_table()?;
    p.set(
        "load_file",
        lua.create_function(|lua, (path, env): (String, Table)| {
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok((None, Some("not found".to_string())));
                }
                Err(e) => return Ok((None, Some(format!("{path}: {e}")))),
            };
            match lua
                .load(text)
                .set_name(chunk_name(lua, Path::new(&path)))
                .set_mode(mlua::chunk::ChunkMode::Text)
                .set_environment(env)
                .into_function()
            {
                Ok(f) => Ok((Some(f), None)),
                Err(e) => Ok((None, Some(compact_error(&e.to_string())))),
            }
        })?,
    )?;
    p.set(
        "define",
        lua.create_function(|lua, (name, f): (String, Function)| {
            RtState::get(lua)?.defined.borrow_mut().insert(name, f);
            Ok(())
        })?,
    )?;
    p.set(
        "report",
        lua.create_function(|lua, (who, msg): (String, String)| {
            report(lua, &format!("plugin '{who}'"), &msg);
            Ok(())
        })?,
    )?;
    p.set(
        "register_service",
        lua.create_function(|lua, (plugin, name, v): (String, Value, Value)| {
            let Value::String(name) = name else {
                return Ok(Some("services must be keyed by name".to_string()));
            };
            let name = name.to_str()?.to_string();
            let owner = prt(lua)?.services.borrow().get(&name).cloned();
            let ctors: Table = lua.named_registry_value("hw.service_ctors")?;
            let builtin = hw_services::NAMES.contains(&name.as_str())
                || (owner.is_none() && ctors.contains_key(name.as_str())?);
            if builtin || owner.as_ref().is_some_and(|o| o != &plugin) {
                return Ok(Some(format!(
                    "service '{name}' is already provided by {}",
                    owner.map_or("hyprwidget".into(), |o| format!("plugin '{o}'"))
                )));
            }
            let ctor = match v {
                Value::Function(f) => f,
                v => lua.create_function(move |_, ()| Ok(v.clone()))?,
            };
            crate::dbus_services::register(lua, &name, ctor)?;
            // Drop a value cached by an earlier load of the plugin.
            let hw: Table = lua.globals().get("hw")?;
            hw.get::<Table>("service")?
                .raw_set(name.as_str(), Value::Nil)?;
            prt(lua)?.services.borrow_mut().insert(name, plugin);
            Ok(None)
        })?,
    )?;
    let (make_env, load_plugin): (Function, Function) = lua
        .load(LOADER)
        .set_name("=hw.plugin")
        .call((hw.clone(), p))?;
    lua.set_named_registry_value("hw.plugin_env", make_env)?;
    lua.set_named_registry_value("hw.plugin_load", load_plugin)?;

    // hw.plugins.list() / hw.plugins.get(name) / hw.plugins.errors()
    let api = lua.create_table()?;
    api.set(
        "list",
        lua.create_function(|lua, ()| {
            Ok(prt(lua)?
                .loaded
                .borrow()
                .keys()
                .cloned()
                .collect::<Vec<_>>())
        })?,
    )?;
    api.set(
        "get",
        lua.create_function(|lua, name: String| {
            let p = prt(lua)?;
            let loaded = p.loaded.borrow();
            let Some((m, dir)) = loaded.get(&name) else {
                return Ok(None);
            };
            let t = spec(
                lua,
                &m.name,
                &m.version,
                dir,
                &m.entry,
                m.permissions,
                &m.widgets,
            )?;
            t.set("description", m.description.clone())?;
            t.set("error", p.failed.borrow().get(&name).cloned())?;
            Ok(Some(t))
        })?,
    )?;
    api.set(
        "errors",
        lua.create_function(|lua, ()| {
            Ok(prt(lua)?
                .failed
                .borrow()
                .clone()
                .into_iter()
                .collect::<HashMap<_, _>>())
        })?,
    )?;
    hw.set("plugins", api)?;
    Ok(())
}

/// Plugins loaded without error in this Lua state.
pub fn loaded_ok(lua: &Lua) -> Vec<String> {
    let Ok(p) = prt(lua) else { return Vec::new() };
    let failed = p.failed.borrow();
    p.loaded
        .borrow()
        .keys()
        .filter(|n| !failed.contains_key(*n))
        .cloned()
        .collect()
}

/// Runs a sandboxed chunk (theme `init.lua`): `make_env(spec)`, load, call.
pub fn run_sandboxed(lua: &Lua, spec: &Table, path: &Path) -> Result<Value> {
    let make_env: Function = lua.named_registry_value("hw.plugin_env")?;
    let env: Table = make_env.call(spec.clone())?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| mlua::Error::runtime(format!("{}: {e}", path.display())))?;
    lua.load(text)
        .set_name(chunk_name(lua, path))
        .set_mode(mlua::chunk::ChunkMode::Text)
        .set_environment(env)
        .call(())
}

fn selection(lua: &Lua) -> std::result::Result<Selection, String> {
    let st = RtState::get(lua).map_err(|e| e.to_string())?;
    let json = st.core.dir.load_json().map_err(|e| format!("{e:#}"))?;
    Selection::from_config(json.get("plugins"))
}

/// Loads every enabled plugin (before the user's config). Plugin problems
/// are reported, never returned.
pub fn load_all(lua: &Lua) -> Result<()> {
    let st = RtState::get(lua)?;
    let sel = match selection(lua) {
        Ok(s) => s,
        Err(e) => {
            report(lua, "plugins", &e);
            return Ok(());
        }
    };
    let found = discover(&st.core.dir.root);
    let (list, errors) = plan(&found, &sel);
    for (name, msg) in errors {
        prt(lua)?
            .failed
            .borrow_mut()
            .insert(name.clone(), msg.clone());
        report(lua, &format!("plugin '{name}'"), &msg);
    }
    for (m, dir) in list {
        load_one(lua, &m, &dir)?;
        watch(lua, &m.name, &dir)?;
    }
    Ok(())
}

/// Loads (or reloads) one plugin; registers its stylesheet.
fn load_one(lua: &Lua, m: &Manifest, dir: &Path) -> Result<bool> {
    let load: Function = lua.named_registry_value("hw.plugin_load")?;
    let spec = spec(
        lua,
        &m.name,
        &m.version,
        dir,
        &m.entry,
        m.permissions,
        &m.widgets,
    )?;
    let (ok, err): (bool, Option<String>) = load.call(spec)?;
    let p = prt(lua)?;
    p.loaded
        .borrow_mut()
        .insert(m.name.clone(), (m.clone(), dir.to_path_buf()));
    if ok {
        p.failed.borrow_mut().remove(&m.name);
        tracing::info!(plugin = %m.name, version = %m.version, "plugin loaded");
    } else {
        let e = err.unwrap_or_default();
        p.failed.borrow_mut().insert(m.name.clone(), e.clone());
        drop(p);
        report(lua, &format!("plugin '{}'", m.name), &e);
    }
    let st = RtState::get(lua)?;
    if let Some(ui) = &st.core.ui {
        let style = dir.join(&m.style);
        ui.styles.set_extra(
            &format!("plugin:{}", m.name),
            (ok && style.is_file()).then_some(style),
        );
    }
    Ok(ok)
}

/// Watches a plugin's directory: Lua/JSON changes reload the plugin and
/// rebuild the `per_monitor` windows, style changes recompile the CSS.
fn watch(lua: &Lua, name: &str, dir: &Path) -> Result<()> {
    let st = RtState::get(lua)?;
    if st.core.ui.is_none() {
        return Ok(());
    }
    let weak = lua.weak();
    let (name, dir2) = (name.to_owned(), dir.to_path_buf());
    let w = hw_ui::watch::watch(dir, &[], move |c| {
        let Some(lua) = weak.try_upgrade() else {
            return;
        };
        // Let the current callback finish before touching windows.
        let (name, dir) = (name.clone(), dir2.clone());
        glib::idle_add_local_once(move || {
            if c.lua
                && let Err(e) = reload(&lua, &name, &dir)
            {
                report(&lua, &format!("plugin '{name}'"), &e.to_string());
            }
            if let Ok(st) = RtState::get(&lua)
                && let Some(ui) = &st.core.ui
                && let Err(e) = ui.styles.reload_user()
            {
                tracing::error!("stylesheet: {e:#}");
            }
        });
    });
    st.keep(Box::new(w));
    Ok(())
}

/// Hot reload of one plugin.
pub fn reload(lua: &Lua, name: &str, dir: &Path) -> Result<()> {
    tracing::info!(plugin = name, "plugin changed: reloading it");
    let file = format!("{PLUGINS_DIR}/{name}/{MANIFEST}");
    let text = std::fs::read_to_string(dir.join(MANIFEST))
        .map_err(|e| mlua::Error::runtime(format!("{file}: {e}")))?;
    let m = match parse_manifest(&file, &text, dir) {
        Ok(m) => m,
        Err(errs) => {
            let e = errs.join("\n");
            prt(lua)?
                .failed
                .borrow_mut()
                .insert(name.to_owned(), e.clone());
            report(lua, &format!("plugin '{name}'"), &e);
            return Ok(());
        }
    };
    // A new error should notify again.
    prt(lua)?
        .notified
        .borrow_mut()
        .retain(|k| !k.starts_with(&format!("plugin '{name}'\n")));
    load_one(lua, &m, dir)?;
    crate::runtime::rebuild_monitor_windows(lua)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_version("2"), Some([2, 0, 0]));
        assert_eq!(parse_version("1.x"), None);
        assert_eq!(version_matches(">=1.2", "1.10.0"), Some(true));
        assert_eq!(version_matches("^1.2", "2.0.0"), Some(false));
        assert_eq!(version_matches("~1.2", "1.2.9"), Some(true));
        assert_eq!(version_matches("1.2", "1.2.7"), Some(true));
        assert_eq!(version_matches("1.2", "1.3.0"), Some(false));
        assert_eq!(version_matches("*", "whatever"), Some(true));
        assert_eq!(version_matches("<2", "1.9.9"), Some(true));
    }

    #[test]
    fn selection_forms() {
        let s = Selection::from_config(Some(&serde_json::json!(["a"]))).unwrap();
        assert!(s.wants("a") && !s.wants("b"));
        let s = Selection::from_config(Some(&serde_json::json!({"disabled": ["b"]}))).unwrap();
        assert!(s.wants("a") && !s.wants("b"));
        assert!(Selection::from_config(Some(&serde_json::json!({"enable": []}))).is_err());
        assert!(Selection::from_config(None).unwrap().wants("x"));
    }
}
