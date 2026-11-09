//! The shape every service has as seen from Lua: named JSON [`Signal`]s
//! (`hw.service.<name>.<prop>`) plus methods taking and returning JSON
//! (`hw.service.<name>:<method>(...)`).
//!
//! Services live on the GTK main thread. Worker threads push new values
//! through a [`Pusher`], drained by the GLib main loop.

use std::any::Any;
use std::cell::RefCell;
use std::path::PathBuf;

use anyhow::Result;
use hw_core::reactive::{Signal, WeakSignal};
use serde_json::Value;

/// A service method: JSON arguments in, JSON result out.
pub type Method = Box<dyn Fn(Vec<Value>) -> Result<Value>>;

pub struct Service {
    pub name: &'static str,
    props: Vec<(&'static str, Signal<Value>)>,
    methods: Vec<(&'static str, Method)>,
    keep: RefCell<Vec<Box<dyn Any>>>,
}

impl Service {
    /// A service whose props all start as `null`.
    pub fn new(name: &'static str, props: &[&'static str]) -> Self {
        Self {
            name,
            props: props
                .iter()
                .map(|p| (*p, Signal::new(Value::Null)))
                .collect(),
            methods: Vec::new(),
            keep: RefCell::default(),
        }
    }

    pub fn prop(&self, name: &str) -> Option<&Signal<Value>> {
        self.props.iter().find(|(n, _)| *n == name).map(|(_, s)| s)
    }

    pub fn prop_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.props.iter().map(|(n, _)| *n)
    }

    pub fn method_names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.methods.iter().map(|(n, _)| *n)
    }

    pub fn has_method(&self, name: &str) -> bool {
        self.methods.iter().any(|(n, _)| *n == name)
    }

    /// Adds a method.
    pub fn method(
        &mut self,
        name: &'static str,
        f: impl Fn(Vec<Value>) -> Result<Value> + 'static,
    ) {
        self.methods.push((name, Box::new(f)));
    }

    /// Calls a method; `None` if there is no such method.
    pub fn call(&self, name: &str, args: Vec<Value>) -> Option<Result<Value>> {
        self.methods
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, f)| f(args))
    }

    /// Sets a prop from the main thread.
    pub fn set(&self, prop: &str, v: Value) {
        if let Some(s) = self.prop(prop) {
            s.set(v);
        }
    }

    /// Returns a `Send` handle that updates props from any thread. Must be
    /// called on the main thread (needs its GLib main context).
    pub fn pusher(&self) -> Pusher {
        let (tx, rx) = async_channel::unbounded::<Update>();
        let props: Vec<(&'static str, WeakSignal<Value>)> = self
            .props
            .iter()
            .map(|(n, s)| (*n, s.downgrade()))
            .collect();
        glib::spawn_future_local(async move {
            while let Ok(u) = rx.recv().await {
                let (name, v, always) = match u {
                    Update::Set(n, v) => (n, v, false),
                    Update::Emit(n, v) => (n, v, true),
                };
                let Some(sig) = props
                    .iter()
                    .find(|(n, _)| *n == name)
                    .and_then(|(_, w)| w.upgrade())
                else {
                    if props.iter().all(|(_, w)| w.upgrade().is_none()) {
                        break;
                    }
                    continue;
                };
                if always {
                    sig.replace(v);
                } else {
                    sig.set(v);
                }
            }
        });
        Pusher(tx)
    }

    /// Ties `thing` (worker handles, guards…) to the service's lifetime.
    pub fn keep_alive(&self, thing: impl Any) {
        self.keep.borrow_mut().push(Box::new(thing));
    }
}

enum Update {
    Set(&'static str, Value),
    Emit(&'static str, Value),
}

/// Thread-safe prop updater created by [`Service::pusher`].
#[derive(Clone)]
pub struct Pusher(async_channel::Sender<Update>);

impl Pusher {
    /// Sets `prop` (subscribers run only if the value changed). Returns
    /// `false` once the service is gone: worker threads should stop then.
    pub fn set(&self, prop: &'static str, v: Value) -> bool {
        self.0.send_blocking(Update::Set(prop, v)).is_ok()
    }

    /// Sets `prop` and notifies even if the value is unchanged (event
    /// streams).
    pub fn emit(&self, prop: &'static str, v: Value) -> bool {
        self.0.send_blocking(Update::Emit(prop, v)).is_ok()
    }

    pub fn is_closed(&self) -> bool {
        self.0.is_closed()
    }
}

/// `$XDG_CACHE_HOME/hyprwidget` (created on demand).
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("hyprwidget")
}

/// Argument helpers for [`Method`]s.
pub fn arg_str(args: &[Value], i: usize, what: &str) -> Result<String> {
    match args.get(i) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => anyhow::bail!("argument {} ({what}) must be a string", i + 1),
    }
}

pub fn arg_f64(args: &[Value], i: usize, what: &str) -> Result<f64> {
    match args.get(i) {
        Some(Value::Number(n)) => Ok(n.as_f64().unwrap_or(0.0)),
        Some(Value::String(s)) => s
            .parse()
            .map_err(|_| anyhow::anyhow!("argument {} ({what}) must be a number", i + 1)),
        _ => anyhow::bail!("argument {} ({what}) must be a number", i + 1),
    }
}

/// Joins all arguments with spaces (`dispatch("workspace", 3)`).
pub fn join_args(args: &[Value]) -> String {
    args.iter()
        .map(|a| match a {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}
