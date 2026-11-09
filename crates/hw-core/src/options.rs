//! Options inherited from Hyprland (PLAN.md §3.6).
//!
//! Layers, later wins:
//! 1. built-in defaults ([`defaults`]),
//! 2. the running Hyprland (`hyprctl getoption`/`animations`),
//! 3. hyprwidget's own config (`hw.config{}` / `config.json`), where
//!    `"inherit"` means "keep the lower layer" and resolvers compute a value
//!    from layers 1+2,
//! 4. per-window/widget overrides, handled by the UI.
//!
//! Options are stored as a JSON tree using Hyprland's names split on `:` and
//! `.`: `general:col.active_border` ↔ `general.col.active_border`. Colors are
//! normalized to CSS strings, `css` gaps to a number (or a
//! `{top,right,bottom,left}` table when the sides differ).

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;

use serde_json::{Map, Value, json};

use crate::color;
use crate::reactive::Signal;

/// Value type of a known option; used for normalization and `check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptType {
    Int,
    Float,
    Bool,
    Str,
    /// Hyprland `css` gaps: one to four ints.
    Gaps,
    /// Color or gradient.
    Gradient,
    Vec2,
}

impl OptType {
    pub fn name(self) -> &'static str {
        match self {
            OptType::Int => "int",
            OptType::Float => "float",
            OptType::Bool => "bool",
            OptType::Str => "string",
            OptType::Gaps => "gaps (int or \"t r b l\")",
            OptType::Gradient => "color/gradient",
            OptType::Vec2 => "vec2 [x, y]",
        }
    }
}

/// CSS unit appended when exporting a numeric option to SCSS/CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    None,
    Px,
    Ms,
}

/// Options fetched from Hyprland, with their type, CSS unit and built-in
/// default (Hyprland's own default, used when Hyprland isn't reachable).
pub const INHERITED: &[(&str, OptType, Unit, &str)] = &[
    ("general:gaps_in", OptType::Gaps, Unit::Px, "5"),
    ("general:gaps_out", OptType::Gaps, Unit::Px, "20"),
    ("general:border_size", OptType::Int, Unit::Px, "1"),
    (
        "general:col.active_border",
        OptType::Gradient,
        Unit::None,
        "\"0xffffffff\"",
    ),
    (
        "general:col.inactive_border",
        OptType::Gradient,
        Unit::None,
        "\"0xff444444\"",
    ),
    ("decoration:rounding", OptType::Int, Unit::Px, "0"),
    (
        "decoration:rounding_power",
        OptType::Float,
        Unit::None,
        "2.0",
    ),
    (
        "decoration:active_opacity",
        OptType::Float,
        Unit::None,
        "1.0",
    ),
    (
        "decoration:inactive_opacity",
        OptType::Float,
        Unit::None,
        "1.0",
    ),
    (
        "decoration:dim_inactive",
        OptType::Bool,
        Unit::None,
        "false",
    ),
    ("decoration:dim_strength", OptType::Float, Unit::None, "0.5"),
    (
        "decoration:shadow:enabled",
        OptType::Bool,
        Unit::None,
        "true",
    ),
    ("decoration:shadow:range", OptType::Int, Unit::Px, "4"),
    (
        "decoration:shadow:render_power",
        OptType::Int,
        Unit::None,
        "3",
    ),
    (
        "decoration:shadow:color",
        OptType::Gradient,
        Unit::None,
        "\"0xee1a1a1a\"",
    ),
    (
        "decoration:shadow:offset",
        OptType::Vec2,
        Unit::Px,
        "[0, 0]",
    ),
    ("decoration:blur:enabled", OptType::Bool, Unit::None, "true"),
    ("decoration:blur:size", OptType::Int, Unit::Px, "8"),
    ("decoration:blur:passes", OptType::Int, Unit::None, "1"),
    (
        "decoration:blur:ignore_opacity",
        OptType::Bool,
        Unit::None,
        "true",
    ),
    ("decoration:blur:popups", OptType::Bool, Unit::None, "false"),
    (
        "decoration:blur:popups_ignorealpha",
        OptType::Float,
        Unit::None,
        "0.2",
    ),
    ("animations:enabled", OptType::Bool, Unit::None, "true"),
    ("misc:font_family", OptType::Str, Unit::None, "\"Sans\""),
];

/// Top-level sections that belong to Hyprland; unknown keys under them are
/// errors in `check`. Other top-level keys are widget options.
pub const HYPR_SECTIONS: &[&str] = &[
    "general",
    "decoration",
    "animations",
    "input",
    "gestures",
    "group",
    "misc",
    "binds",
    "xwayland",
    "opengl",
    "render",
    "cursor",
    "ecosystem",
    "experimental",
    "debug",
    "dwindle",
    "master",
];

pub fn inherited(name: &str) -> Option<(OptType, Unit)> {
    INHERITED
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|(_, t, u, _)| (*t, *u))
}

/// `general:col.active_border` → `["general", "col", "active_border"]`.
pub fn name_to_path(name: &str) -> Vec<String> {
    name.split([':', '.'])
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Inverse of [`name_to_path`], following Hyprland's naming where `col` is
/// joined to its child with a dot.
pub fn path_to_name<S: AsRef<str>>(path: &[S]) -> String {
    let mut out = String::new();
    for (i, seg) in path.iter().enumerate() {
        if i > 0 {
            out.push(if path[i - 1].as_ref() == "col" {
                '.'
            } else {
                ':'
            });
        }
        out.push_str(seg.as_ref());
    }
    out
}

/// SCSS/CSS variable name: `:` and `.` replaced by `-`.
pub fn var_name(name: &str) -> String {
    name.replace([':', '.'], "-")
}

pub fn lookup<'a, S: AsRef<str>>(tree: &'a Value, path: &[S]) -> Option<&'a Value> {
    path.iter().try_fold(tree, |v, k| v.get(k.as_ref()))
}

/// Inserts `value` at `path`, creating objects as needed.
pub fn insert<S: AsRef<str>>(tree: &mut Value, path: &[S], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        *tree = value;
        return;
    };
    let mut cur = tree;
    for k in parents {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        cur = cur
            .as_object_mut()
            .unwrap()
            .entry(k.as_ref())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    if !cur.is_object() {
        *cur = Value::Object(Map::new());
    }
    cur.as_object_mut()
        .unwrap()
        .insert(last.as_ref().to_owned(), value);
}

/// Deep-merges `over` into `base`: objects merge recursively, anything else
/// replaces. The string `"inherit"` leaves `base` untouched.
pub fn deep_merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (_, Value::String(s)) if s == "inherit" => {}
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(bv) => deep_merge(bv, v),
                    None if v.as_str() == Some("inherit") => {}
                    None => {
                        let mut nv = if v.is_object() {
                            json!({})
                        } else {
                            Value::Null
                        };
                        deep_merge(&mut nv, v);
                        b.insert(k.clone(), nv);
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Expands flat keys (`"decoration:rounding"`, `"general.col.active_border"`)
/// into nested objects, so JSON may use either form.
pub fn expand_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = json!({});
            for (k, v) in m {
                let path = name_to_path(k);
                let mut nested = json!({});
                insert(&mut nested, &path, expand_keys(v));
                deep_merge(&mut out, &nested);
            }
            out
        }
        other => other.clone(),
    }
}

/// Converts one `hyprctl getoption -j` reply to a tree value.
pub fn from_getoption(reply: &Value) -> Option<Value> {
    let o = reply.as_object()?;
    if let Some(v) = o.get("int") {
        return Some(v.clone());
    }
    if let Some(f) = o.get("float").and_then(Value::as_f64) {
        return Some(json!((f * 1000.0).round() / 1000.0));
    }
    if let Some(v) = o.get("bool") {
        return Some(v.clone());
    }
    if let Some(s) = o.get("str").and_then(Value::as_str) {
        return Some(json!(if s == "[[EMPTY]]" { "" } else { s }));
    }
    if let Some(s) = o.get("css").and_then(Value::as_str) {
        return parse_gaps(s);
    }
    if let Some(s) = o.get("gradient").and_then(Value::as_str) {
        return color::to_css(s).ok().map(Value::String);
    }
    if let Some(v) = o.get("vec2") {
        return Some(v.clone());
    }
    o.get("custom").cloned()
}

/// `"6"`, `"6 8"`, `"1 2 3"`, `"1 2 3 4"` (CSS shorthand) → number or
/// `{top,right,bottom,left}`.
pub fn parse_gaps(s: &str) -> Option<Value> {
    let n: Vec<i64> = s
        .split_whitespace()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [t, r, b, l] = match n.as_slice() {
        [a] => [*a; 4],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    };
    if t == r && r == b && b == l {
        Some(json!(t))
    } else {
        Some(json!({ "top": t, "right": r, "bottom": b, "left": l }))
    }
}

/// Normalizes a user-provided value for a known option, or explains why it
/// has the wrong type.
pub fn normalize(name: &str, v: &Value) -> Result<Value, String> {
    let Some((ty, _)) = inherited(name) else {
        return Ok(v.clone());
    };
    let bad = || Err(format!("option '{name}' expects {}, got {v}", ty.name()));
    match ty {
        OptType::Int if v.is_i64() || v.as_f64().is_some_and(|f| f.fract() == 0.0) => {
            Ok(json!(v.as_f64().unwrap() as i64))
        }
        OptType::Float if v.is_number() => Ok(v.clone()),
        OptType::Bool => match v {
            Value::Bool(_) => Ok(v.clone()),
            Value::Number(n) if n.as_i64() == Some(0) || n.as_i64() == Some(1) => {
                Ok(json!(n.as_i64() == Some(1)))
            }
            _ => bad(),
        },
        OptType::Str if v.is_string() => Ok(v.clone()),
        OptType::Gaps => match v {
            Value::Number(_) => Ok(v.clone()),
            Value::String(s) => parse_gaps(s).map_or_else(bad, Ok),
            Value::Object(_) => Ok(v.clone()),
            _ => bad(),
        },
        OptType::Gradient => match v.as_str().map(color::to_css) {
            Some(Ok(css)) => Ok(json!(css)),
            Some(Err(e)) => Err(format!("option '{name}': {e}")),
            None => bad(),
        },
        OptType::Vec2 => match v.as_array().map(Vec::as_slice) {
            Some([a, b]) if a.is_number() && b.is_number() => Ok(v.clone()),
            _ => bad(),
        },
        _ => bad(),
    }
}

/// Built-in defaults tree.
pub fn defaults() -> Value {
    let mut t = json!({});
    for (name, ty, _, def) in INHERITED {
        let v: Value = serde_json::from_str(def).expect("valid default");
        let v = if *ty == OptType::Gradient {
            normalize(name, &v).unwrap()
        } else {
            v
        };
        insert(&mut t, &name_to_path(name), v);
    }
    let anims = animations_tree(&json!([[], []]));
    deep_merge(&mut t, &json!({ "animations": anims }));
    t
}

/// Parent of each Hyprland animation in the inheritance tree.
const ANIM_PARENTS: &[(&str, &str)] = &[
    ("windows", "global"),
    ("windowsIn", "windows"),
    ("windowsOut", "windows"),
    ("windowsMove", "windows"),
    ("layers", "global"),
    ("layersIn", "layers"),
    ("layersOut", "layers"),
    ("fade", "global"),
    ("fadeIn", "fade"),
    ("fadeOut", "fade"),
    ("fadeSwitch", "fade"),
    ("fadeShadow", "fade"),
    ("fadeDim", "fade"),
    ("fadeLayers", "fade"),
    ("fadeLayersIn", "fadeLayers"),
    ("fadeLayersOut", "fadeLayers"),
    ("fadePopups", "fade"),
    ("fadePopupsIn", "fadePopups"),
    ("fadePopupsOut", "fadePopups"),
    ("border", "global"),
    ("borderangle", "border"),
    ("workspaces", "global"),
    ("workspacesIn", "workspaces"),
    ("workspacesOut", "workspaces"),
    ("specialWorkspace", "workspaces"),
    ("specialWorkspaceIn", "specialWorkspace"),
    ("specialWorkspaceOut", "specialWorkspace"),
    ("zoomFactor", "global"),
    ("monitorAdded", "global"),
];

/// Hyprland's built-in `default` curve.
const DEFAULT_BEZIER: [f64; 4] = [0.0, 0.75, 0.15, 1.0];

/// Converts `hyprctl animations -j` (`[[animations], [beziers]]`) into
/// `{ bezier = {name = "cubic-bezier(..)"}, animation = {name = {...}} }`,
/// resolving non-overridden animations through their parents.
pub fn animations_tree(reply: &Value) -> Value {
    let anims = reply
        .get(0)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let beziers = reply
        .get(1)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut bez = Map::new();
    bez.insert("default".into(), json!(cubic(DEFAULT_BEZIER)));
    for b in &beziers {
        let (Some(name), Some(x0), Some(y0), Some(x1), Some(y1)) = (
            b["name"].as_str(),
            b["X0"].as_f64(),
            b["Y0"].as_f64(),
            b["X1"].as_f64(),
            b["Y1"].as_f64(),
        ) else {
            continue;
        };
        bez.insert(name.into(), json!(cubic([x0, y0, x1, y1])));
    }

    let by_name: HashMap<&str, &Value> = anims
        .iter()
        .filter_map(|a| Some((a["name"].as_str()?, a)))
        .collect();
    let mut names: Vec<&str> = by_name.keys().copied().collect();
    names.extend(ANIM_PARENTS.iter().map(|(n, _)| *n));
    names.push("global");
    names.sort_unstable();
    names.dedup();

    let mut out = Map::new();
    for name in names {
        // Walk up until an overridden ancestor (or global).
        let mut cur = name;
        let entry = loop {
            match by_name.get(cur) {
                Some(a) if a["overridden"].as_bool() == Some(true) => break Some(*a),
                _ => {}
            }
            match ANIM_PARENTS.iter().find(|(n, _)| *n == cur) {
                Some((_, p)) => cur = p,
                None => break by_name.get("global").copied(),
            }
        };
        let (enabled, speed, bezier, style) = match entry {
            Some(a) => (
                a["enabled"].as_bool().unwrap_or(true),
                a["speed"].as_f64().unwrap_or(8.0),
                a["bezier"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("default"),
                a["style"].as_str().unwrap_or(""),
            ),
            None => (true, 8.0, "default", ""),
        };
        out.insert(
            name.into(),
            json!({ "enabled": enabled, "speed": speed, "bezier": bezier, "style": style }),
        );
    }
    let mut tree = json!({ "bezier": bez, "animation": out });
    finish_animations(&mut tree, true);
    tree
}

fn cubic([x0, y0, x1, y1]: [f64; 4]) -> String {
    format!("cubic-bezier({x0}, {y0}, {x1}, {y1})")
}

/// Fills `duration` (ms, Hyprland speed is in 100 ms units) and `easing`
/// (CSS `cubic-bezier`) for every animation of an `animations` subtree.
fn finish_animations(anims: &mut Value, globally_enabled: bool) {
    let beziers = anims.get("bezier").cloned().unwrap_or(json!({}));
    let Some(list) = anims.get_mut("animation").and_then(Value::as_object_mut) else {
        return;
    };
    for a in list.values_mut() {
        let enabled = globally_enabled && a["enabled"].as_bool().unwrap_or(true);
        let speed = a["speed"].as_f64().unwrap_or(8.0);
        let bezier = a["bezier"].as_str().unwrap_or("default");
        let easing = beziers
            .get(bezier)
            .cloned()
            .unwrap_or_else(|| json!(cubic(DEFAULT_BEZIER)));
        let duration = if enabled {
            (speed * 100.0).round()
        } else {
            0.0
        };
        if let Some(o) = a.as_object_mut() {
            o.insert("duration".into(), json!(duration as i64));
            o.insert("easing".into(), easing);
        }
    }
}

/// Resolver for a user value given as a function of the Hyprland layer.
pub type Resolver = Box<dyn Fn(&Value) -> Option<Value>>;

/// Layered option store; see the module docs.
pub struct Options {
    defaults: Value,
    hypr: RefCell<Value>,
    user: RefCell<Value>,
    resolvers: RefCell<Vec<(Vec<String>, Resolver)>>,
    tree: Signal<Value>,
    signals: RefCell<HashMap<String, Signal<Value>>>,
}

impl Default for Options {
    fn default() -> Self {
        Self::new()
    }
}

impl Options {
    pub fn new() -> Self {
        let defaults = defaults();
        Self {
            tree: Signal::new(defaults.clone()),
            defaults,
            hypr: RefCell::new(json!({})),
            user: RefCell::new(json!({})),
            resolvers: RefCell::new(Vec::new()),
            signals: RefCell::new(HashMap::new()),
        }
    }

    /// Replaces the Hyprland layer and recomputes.
    pub fn set_hypr(&self, tree: Value) {
        *self.hypr.borrow_mut() = tree;
        self.recompute();
    }

    /// Replaces the user layer and its resolvers and recomputes.
    pub fn set_user(&self, tree: Value, resolvers: Vec<(Vec<String>, Resolver)>) {
        *self.user.borrow_mut() = tree;
        *self.resolvers.borrow_mut() = resolvers;
        self.recompute();
    }

    /// Defaults + Hyprland: what `"inherit"` and resolvers see.
    pub fn base(&self) -> Value {
        let mut t = self.defaults.clone();
        deep_merge(&mut t, &self.hypr.borrow());
        t
    }

    fn compute(&self) -> Value {
        let base = self.base();
        let mut t = base.clone();
        deep_merge(&mut t, &self.user.borrow());
        for (path, f) in self.resolvers.borrow().iter() {
            if let Some(v) = f(&base) {
                let v = normalize(&path_to_name(path), &v).unwrap_or(v);
                insert(&mut t, path, v);
            }
        }
        let enabled =
            lookup(&t, &["animations", "enabled"]).and_then(Value::as_bool) != Some(false);
        if let Some(a) = t.get_mut("animations") {
            finish_animations(a, enabled);
        }
        t
    }

    pub fn recompute(&self) {
        let t = self.compute();
        for (name, sig) in self.signals.borrow().iter() {
            sig.set(
                lookup(&t, &name_to_path(name))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
        }
        self.tree.set(t);
    }

    /// Merged tree as a signal.
    pub fn tree(&self) -> Signal<Value> {
        self.tree.clone()
    }

    /// Current merged value of `name` (`Null` if unknown).
    pub fn get(&self, name: &str) -> Value {
        self.tree
            .with(|t| lookup(t, &name_to_path(name)).cloned())
            .unwrap_or(Value::Null)
    }

    /// Reactive merged value of `name`.
    pub fn signal(&self, name: &str) -> Signal<Value> {
        let key = path_to_name(&name_to_path(name));
        self.signals
            .borrow_mut()
            .entry(key.clone())
            .or_insert_with(|| {
                Signal::new(self.tree.with(|t| {
                    lookup(t, &name_to_path(&key))
                        .cloned()
                        .unwrap_or(Value::Null)
                }))
            })
            .clone()
    }
}

fn unit_of(path: &[String]) -> Unit {
    if path.last().is_some_and(|l| l == "duration") {
        return Unit::Ms;
    }
    inherited(&path_to_name(path)).map_or(Unit::None, |(_, u)| u)
}

fn css_value(v: &Value, unit: Unit) -> Option<String> {
    let suffix = match unit {
        Unit::None => "",
        Unit::Px => "px",
        Unit::Ms => "ms",
    };
    Some(match v {
        Value::Number(n) => format!("{n}{suffix}"),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => {
            const RAW: &[&str] = &["rgb", "linear-gradient(", "cubic-bezier(", "#"];
            if RAW.iter().any(|p| s.starts_with(p)) {
                s.clone()
            } else {
                format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
            }
        }
        Value::Object(o) if o.contains_key("top") => ["top", "right", "bottom", "left"]
            .iter()
            .map(|k| format!("{}{suffix}", o.get(*k).unwrap_or(&json!(0))))
            .collect::<Vec<_>>()
            .join(" "),
        Value::Array(a) => a
            .iter()
            .map(|x| format!("{x}{suffix}"))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    })
}

/// Visits every exportable leaf as (variable name, CSS value).
fn for_each_var(tree: &Value, mut f: impl FnMut(&str, &str)) {
    fn walk(v: &Value, path: &mut Vec<String>, f: &mut dyn FnMut(&str, &str)) {
        match v {
            Value::Object(o) if !o.contains_key("top") => {
                for (k, v) in o {
                    path.push(k.clone());
                    walk(v, path, f);
                    path.pop();
                }
            }
            _ if !path.is_empty() => {
                if let Some(css) = css_value(v, unit_of(path)) {
                    let name = var_name(&path_to_name(path));
                    f(&name, &css);
                    // Solid fallback for properties that can't take a
                    // gradient (border-color, box-shadow…): the first stop.
                    if let Some(c) = v.as_str().and_then(first_color) {
                        f(&format!("{name}-color"), c);
                    }
                }
            }
            _ => {}
        }
    }
    walk(tree, &mut Vec::new(), &mut f);
}

/// First `rgba(…)` of a CSS color or gradient.
fn first_color(s: &str) -> Option<&str> {
    let start = s.find("rgba(")?;
    let end = start + s[start..].find(')')?;
    Some(&s[start..=end])
}

/// Contents of the generated `_hypr.scss`.
pub fn to_scss(tree: &Value) -> String {
    let mut s = String::from("// Generated by hyprwidget from Hyprland options. Do not edit.\n");
    for_each_var(tree, |name, val| {
        let _ = writeln!(s, "${name}: {val};");
    });
    s
}

/// `:root { --name: value; … }` with the same variables.
pub fn to_css_vars(tree: &Value) -> String {
    format!(":root {{\n{}}}\n", css_decls(tree))
}

/// `--name: value;` declarations for every exportable leaf of `tree`.
pub fn css_decls(tree: &Value) -> String {
    let mut s = String::new();
    for_each_var(tree, |name, val| {
        let _ = writeln!(s, "  --{name}: {val};");
    });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_paths() {
        assert_eq!(
            name_to_path("general:col.active_border"),
            ["general", "col", "active_border"]
        );
        assert_eq!(
            path_to_name(&["general", "col", "active_border"]),
            "general:col.active_border"
        );
        assert_eq!(
            path_to_name(&["decoration", "blur", "size"]),
            "decoration:blur:size"
        );
        assert_eq!(
            var_name("general:col.active_border"),
            "general-col-active_border"
        );
    }

    #[test]
    fn merge_inherit_and_nesting() {
        let mut base = json!({"decoration": {"rounding": 16, "blur": {"size": 4, "passes": 2}}});
        deep_merge(
            &mut base,
            &json!({"decoration": {"rounding": "inherit", "blur": {"size": 8}}}),
        );
        assert_eq!(
            base,
            json!({"decoration": {"rounding": 16, "blur": {"size": 8, "passes": 2}}})
        );
    }

    #[test]
    fn flat_keys_expand() {
        let v = expand_keys(&json!({
            "decoration:rounding": 4,
            "general": {"col.active_border": "rgb(ff0000)"},
            "decoration": {"blur": {"size": 3}}
        }));
        assert_eq!(
            v,
            json!({
                "decoration": {"rounding": 4, "blur": {"size": 3}},
                "general": {"col": {"active_border": "rgb(ff0000)"}}
            })
        );
    }

    #[test]
    fn getoption_conversion() {
        let g = |s: &str| from_getoption(&serde_json::from_str(s).unwrap()).unwrap();
        assert_eq!(g(r#"{"option":"a","int":16,"set":true}"#), json!(16));
        assert_eq!(g(r#"{"option":"a","css":"6 6 6 6","set":true}"#), json!(6));
        assert_eq!(
            g(r#"{"option":"a","css":"1 2","set":true}"#),
            json!({"top":1,"right":2,"bottom":1,"left":2})
        );
        assert_eq!(
            g(r#"{"option":"a","float":0.970000,"set":true}"#),
            json!(0.97)
        );
        assert_eq!(
            g(r#"{"option":"a","gradient":"99808080 0deg","set":true}"#),
            json!("rgba(128,128,128,0.6)")
        );
    }

    #[test]
    fn priority_defaults_hypr_user_resolver() {
        let o = Options::new();
        assert_eq!(o.get("decoration:rounding"), json!(0));
        o.set_hypr(json!({"decoration": {"rounding": 16}, "general": {"gaps_out": 6}}));
        assert_eq!(o.get("decoration:rounding"), json!(16));
        let sig = o.signal("decoration:rounding");
        o.set_user(
            json!({"decoration": {"rounding": 4}, "general": {"gaps_out": "inherit"}}),
            vec![(
                name_to_path("general:gaps_in"),
                Box::new(|h: &Value| Some(json!(h["general"]["gaps_out"].as_i64()? * 2))),
            )],
        );
        assert_eq!(sig.get(), json!(4));
        assert_eq!(o.get("general:gaps_out"), json!(6));
        assert_eq!(o.get("general:gaps_in"), json!(12));
        // Hyprland changes propagate through resolvers.
        o.set_hypr(json!({"general": {"gaps_out": 10}}));
        assert_eq!(o.get("general:gaps_in"), json!(20));
        assert_eq!(sig.get(), json!(4));
    }

    #[test]
    fn normalize_types() {
        assert_eq!(
            normalize("general:col.active_border", &json!("rgb(808080)")).unwrap(),
            json!("rgba(128,128,128,1)")
        );
        assert!(normalize("decoration:rounding", &json!("big")).is_err());
        assert_eq!(
            normalize("general:gaps_out", &json!("4 8")).unwrap()["right"],
            json!(8)
        );
        assert_eq!(normalize("bar:anything", &json!("x")).unwrap(), json!("x"));
    }

    #[test]
    fn animations_inherit_parents() {
        let reply = json!([
            [
                {"name":"global","overridden":true,"bezier":"default","enabled":true,"speed":8.0,"style":""},
                {"name":"layers","overridden":true,"bezier":"ease","enabled":true,"speed":1.62,"style":"slide"},
                {"name":"layersIn","overridden":false,"bezier":"","enabled":true,"speed":0.0,"style":""}
            ],
            [{"name":"ease","X0":0.0,"Y0":1.0,"X1":0.62,"Y1":1.0}]
        ]);
        let t = animations_tree(&reply);
        let li = &t["animation"]["layersIn"];
        assert_eq!(li["bezier"], "ease");
        assert_eq!(li["duration"], 162);
        assert_eq!(li["easing"], "cubic-bezier(0, 1, 0.62, 1)");
        assert_eq!(t["animation"]["fadeLayersIn"]["duration"], 800);
    }

    #[test]
    fn scss_export() {
        let mut t = defaults();
        deep_merge(
            &mut t,
            &json!({"decoration": {"rounding": 16}, "misc": {"font_family": "JetBrains Mono"}}),
        );
        let scss = to_scss(&t);
        assert!(scss.contains("$decoration-rounding: 16px;"));
        assert!(scss.contains("$general-col-active_border: rgba(255,255,255,1);"));
        assert!(scss.contains("$general-col-active_border-color: rgba(255,255,255,1);"));
        assert!(scss.contains("$misc-font_family: \"JetBrains Mono\";"));
        assert!(scss.contains("$animations-animation-layersIn-duration: 800ms;"));
        assert!(to_css_vars(&t).contains("--decoration-rounding: 16px;"));
    }
}
