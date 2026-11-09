//! Settings store compatible with AGS' `settings.json`.
//!
//! The file is a nested JSON object. A leaf is either a plain value or an
//! `AGSSetting` object `{ name, value, type, min?, max? }`; keys are dotted
//! paths (`"hyprland.decoration.rounding"`) and always address the *value*.
//! Writes are debounced and atomic.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::reactive::{Signal, remove_source};

/// Delay between the last `set` and the write to disk.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

pub struct Settings {
    path: PathBuf,
    data: RefCell<Value>,
    signals: RefCell<HashMap<String, Signal<Value>>>,
    pending: RefCell<Option<glib::SourceId>>,
    this: Weak<Settings>,
}

fn is_setting(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|o| o.contains_key("value") && o.contains_key("type"))
}

fn split(key: &str) -> Vec<&str> {
    key.split('.').filter(|s| !s.is_empty()).collect()
}

fn node<'a>(root: &'a Value, key: &str) -> Option<&'a Value> {
    split(key).iter().try_fold(root, |v, k| v.get(k))
}

fn unwrap_setting(v: &Value) -> Value {
    if is_setting(v) {
        v["value"].clone()
    } else {
        v.clone()
    }
}

/// `v` as an object, replacing it with `{}` if it is anything else.
fn as_object(v: &mut Value) -> &mut Map<String, Value> {
    if !v.is_object() {
        *v = json!({});
    }
    match v {
        Value::Object(m) => m,
        _ => unreachable!(),
    }
}

/// Checks `value` against an `AGSSetting`'s `type`/`min`/`max`; numbers are
/// clamped.
pub fn validate(meta: &Value, value: Value) -> Result<Value> {
    let ty = meta["type"].as_str().unwrap_or("");
    let lo = meta["min"].as_f64().unwrap_or(f64::NEG_INFINITY);
    let hi = meta["max"].as_f64().unwrap_or(f64::INFINITY);
    if lo > hi {
        bail!("invalid bounds: min {lo} > max {hi}");
    }
    let clamp = |x: f64| x.clamp(lo, hi);
    Ok(match ty {
        "int" => match value.as_f64().map(|x| clamp(x.round())) {
            // `i64::MAX as f64` rounds up to 2^63, hence the strict bound.
            Some(x) if x >= i64::MIN as f64 && x < i64::MAX as f64 => json!(x as i64),
            _ => bail!("expected int, got {value}"),
        },
        "float" => match value.as_f64() {
            Some(x) => json!(clamp(x)),
            None => bail!("expected float, got {value}"),
        },
        "bool" | "boolean" if !value.is_boolean() => bail!("expected bool, got {value}"),
        "string" | "str" if !value.is_string() => bail!("expected string, got {value}"),
        _ => value,
    })
}

impl Settings {
    /// Loads `path`; a missing file starts empty.
    pub fn load(path: impl Into<PathBuf>) -> Result<Rc<Self>> {
        let path = path.into();
        let data = match std::fs::read_to_string(&path) {
            Ok(s) if s.trim().is_empty() => json!({}),
            Ok(s) => {
                serde_json::from_str(&s).with_context(|| format!("parsing {}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Rc::new_cyclic(|this| Self {
            path,
            data: RefCell::new(data),
            signals: RefCell::new(HashMap::new()),
            pending: RefCell::new(None),
            this: this.clone(),
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Value at `key` (`Null` if missing).
    pub fn get(&self, key: &str) -> Value {
        node(&self.data.borrow(), key)
            .map(unwrap_setting)
            .unwrap_or(Value::Null)
    }

    /// The `AGSSetting` object at `key`, if the leaf is one.
    pub fn meta(&self, key: &str) -> Option<Value> {
        node(&self.data.borrow(), key)
            .filter(|v| is_setting(v))
            .cloned()
    }

    /// Whole document (raw, with `AGSSetting` objects).
    pub fn raw(&self) -> Value {
        self.data.borrow().clone()
    }

    /// Sets `key`, validating against its `AGSSetting` if any, updates
    /// signals and schedules a save. Returns the stored value.
    pub fn set(&self, key: &str, value: Value) -> Result<Value> {
        let parts = split(key);
        let Some((last, parents)) = parts.split_last() else {
            bail!("empty settings key");
        };
        let stored = {
            let mut data = self.data.borrow_mut();
            let mut cur = &mut *data;
            for k in parents {
                if is_setting(cur) {
                    *cur = json!({});
                }
                cur = as_object(cur).entry(*k).or_insert_with(|| json!({}));
            }
            let obj = as_object(cur);
            match obj.get_mut(*last) {
                Some(leaf) if is_setting(leaf) => {
                    let v = validate(leaf, value).with_context(|| format!("setting '{key}'"))?;
                    leaf["value"] = v.clone();
                    v
                }
                _ => {
                    obj.insert((*last).to_owned(), value.clone());
                    value
                }
            }
        };
        self.refresh_signals();
        self.schedule_save();
        Ok(stored)
    }

    /// Reactive value at `key`.
    pub fn signal(&self, key: &str) -> Signal<Value> {
        self.signals
            .borrow_mut()
            .entry(key.to_owned())
            .or_insert_with(|| Signal::new(self.get(key)))
            .clone()
    }

    fn refresh_signals(&self) {
        let sigs: Vec<(String, Signal<Value>)> = self
            .signals
            .borrow()
            .iter()
            .map(|(k, s)| (k.clone(), s.clone()))
            .collect();
        for (k, s) in sigs {
            s.set(self.get(&k));
        }
    }

    fn schedule_save(&self) {
        if self.pending.borrow().is_some() {
            return;
        }
        let this = self.this.clone();
        let id = glib::timeout_add_local_once(SAVE_DEBOUNCE, move || {
            if let Some(s) = this.upgrade() {
                s.pending.borrow_mut().take();
                if let Err(e) = s.save() {
                    tracing::error!("saving settings: {e:#}");
                }
            }
        });
        *self.pending.borrow_mut() = Some(id);
    }

    /// Writes pending changes now.
    pub fn flush(&self) -> Result<()> {
        let pending = self.pending.borrow_mut().take();
        if let Some(id) = pending {
            remove_source(id);
            self.save()?;
        }
        Ok(())
    }

    /// Writes the file atomically with AGS' 4-space indentation.
    pub fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut buf = Vec::new();
        let fmt = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut ser = serde_json::Serializer::with_formatter(&mut buf, fmt);
        self.data.borrow().serialize(&mut ser)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &buf)?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("writing {}", self.path.display()))?;
        Ok(())
    }
}

impl Drop for Settings {
    fn drop(&mut self) {
        if let Some(id) = self.pending.get_mut().take() {
            remove_source(id);
            if let Err(e) = self.save() {
                tracing::error!("saving settings: {e:#}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hw-settings-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn ags_format_roundtrip() {
        let p = tmp("roundtrip");
        std::fs::write(
            &p,
            r#"{"dateFormat":"%H:%M","hyprland":{"decoration":{"rounding":
               {"name":"Rounding","value":16,"min":0,"max":50,"type":"int"}}}}"#,
        )
        .unwrap();
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let s = Settings::load(&p).unwrap();
            assert_eq!(s.get("dateFormat"), json!("%H:%M"));
            assert_eq!(s.get("hyprland.decoration.rounding"), json!(16));
            let sig = s.signal("hyprland.decoration.rounding");
            assert_eq!(
                s.set("hyprland.decoration.rounding", json!(99)).unwrap(),
                json!(50)
            );
            assert_eq!(sig.get(), json!(50));
            assert!(s.set("hyprland.decoration.rounding", json!("x")).is_err());
            s.set("bar.lock", json!(true)).unwrap();
            s.flush().unwrap();
        })
        .unwrap();
        let back: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(
            back["hyprland"]["decoration"]["rounding"]["value"],
            json!(50)
        );
        assert_eq!(
            back["hyprland"]["decoration"]["rounding"]["name"],
            json!("Rounding")
        );
        assert_eq!(back["bar"]["lock"], json!(true));
    }

    #[test]
    fn validate_rejects_bad_bounds_and_range() {
        let inverted = json!({"type":"int","min":10,"max":0,"value":5});
        assert!(validate(&inverted, json!(5)).is_err());
        let int = json!({"type":"int","value":0});
        assert!(validate(&int, json!(1e300)).is_err());
        assert_eq!(validate(&int, json!(-3.6)).unwrap(), json!(-4));
    }

    #[test]
    fn set_with_inverted_bounds_errors() {
        let p = tmp("inverted");
        std::fs::write(&p, r#"{"x":{"value":5,"min":10,"max":0,"type":"int"}}"#).unwrap();
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let s = Settings::load(&p).unwrap();
            assert!(s.set("x", json!(3)).is_err());
            assert_eq!(s.get("x"), json!(5));
        })
        .unwrap();
    }

    #[test]
    fn missing_file_is_empty() {
        let s = Settings::load(tmp("missing").with_file_name("nope.json")).unwrap();
        assert!(s.get("a.b").is_null());
    }
}
