//! Hot reload: watch the config tree with `gio::FileMonitor` and report
//! what kind of file changed, debounced.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use gtk4::{gio, glib, prelude::*};
use hw_core::reactive::remove_source;

pub const DEBOUNCE: Duration = Duration::from_millis(150);

/// What needs reloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Changes {
    pub css: bool,
    pub lua: bool,
}

/// Classifies a changed path.
pub fn classify(path: &Path) -> Changes {
    match path.extension().and_then(|e| e.to_str()) {
        Some("scss" | "sass" | "css") => Changes {
            css: true,
            lua: false,
        },
        Some("lua" | "json") => Changes {
            css: false,
            lua: true,
        },
        _ => Changes::default(),
    }
}

/// Keeps the monitors alive.
pub struct Watcher {
    _monitors: Vec<gio::FileMonitor>,
}

/// Watches `root` and its subdirectories (except `skip` and hidden dirs)
/// and calls `on_change` once per burst of changes.
pub fn watch(root: &Path, skip: &[PathBuf], on_change: impl Fn(Changes) + 'static) -> Watcher {
    let pending: Rc<RefCell<(Changes, Option<glib::SourceId>)>> = Rc::default();
    let on_change = Rc::new(on_change);
    let mut monitors = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                let hidden = e.file_name().to_string_lossy().starts_with('.');
                if p.is_dir() && !hidden && !skip.iter().any(|s| p.starts_with(s)) {
                    dirs.push(p);
                }
            }
        }
        let file = gio::File::for_path(&dir);
        let mon = match file
            .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("cannot watch {}: {e}", dir.display());
                continue;
            }
        };
        let pending = pending.clone();
        let on_change = on_change.clone();
        mon.connect_changed(move |_, f, other, ev| {
            use gio::FileMonitorEvent as E;
            if !matches!(
                ev,
                E::ChangesDoneHint
                    | E::Created
                    | E::Deleted
                    | E::Renamed
                    | E::MovedIn
                    | E::MovedOut
            ) {
                return;
            }
            let mut c = Changes::default();
            for p in [Some(f.clone()), other.cloned()]
                .into_iter()
                .flatten()
                .filter_map(|f| f.path())
            {
                let k = classify(&p);
                c.css |= k.css;
                c.lua |= k.lua;
            }
            if c == Changes::default() {
                return;
            }
            let mut st = pending.borrow_mut();
            st.0.css |= c.css;
            st.0.lua |= c.lua;
            if let Some(id) = st.1.take() {
                remove_source(id);
            }
            let p2 = pending.clone();
            let cb = on_change.clone();
            st.1 = Some(glib::timeout_add_local_once(DEBOUNCE, move || {
                let changes = {
                    let mut st = p2.borrow_mut();
                    st.1 = None;
                    std::mem::take(&mut st.0)
                };
                cb(changes);
            }));
        });
        monitors.push(mon);
    }
    Watcher {
        _monitors: monitors,
    }
}
