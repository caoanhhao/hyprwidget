//! Registry of windows by name (`hw.window.get`, `hyprwidget toggle`).

use std::cell::RefCell;
use std::collections::BTreeMap;

use gtk4::prelude::*;

#[derive(Default)]
pub struct Windows {
    map: RefCell<BTreeMap<String, gtk4::Window>>,
}

impl Windows {
    /// Registers `w` under `name`, destroying a previous window of that name.
    pub fn add(&self, name: &str, w: &gtk4::Window) {
        let old = self.map.borrow_mut().insert(name.to_owned(), w.clone());
        if let Some(old) = old.filter(|o| o != w) {
            tracing::warn!("window '{name}' replaced");
            old.destroy();
        }
    }

    pub fn get(&self, name: &str) -> Option<gtk4::Window> {
        self.map.borrow().get(name).cloned()
    }

    /// Unregisters and destroys `name`.
    pub fn remove(&self, name: &str) {
        let w = self.map.borrow_mut().remove(name);
        if let Some(w) = w {
            w.destroy();
        }
    }

    /// Unregisters and destroys `w`, whatever its name.
    pub fn remove_window(&self, w: &gtk4::Window) {
        self.map.borrow_mut().retain(|_, v| v != w);
        w.destroy();
    }

    /// Toggles visibility; returns the new state.
    pub fn toggle(&self, name: &str) -> Option<bool> {
        let w = self.get(name)?;
        let show = !w.is_visible();
        if show {
            w.present()
        } else {
            w.set_visible(false)
        }
        Some(show)
    }

    /// `(name, visible)` of every window.
    pub fn list(&self) -> Vec<(String, bool)> {
        self.map
            .borrow()
            .iter()
            .map(|(n, w)| (n.clone(), w.is_visible()))
            .collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.map.borrow().keys().cloned().collect()
    }

    /// Destroys every window (Lua reload).
    pub fn clear(&self) {
        let all = std::mem::take(&mut *self.map.borrow_mut());
        for w in all.into_values() {
            w.destroy();
        }
    }
}
