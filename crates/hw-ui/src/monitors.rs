//! Monitor list as a signal (PLAN.md §4): debounced 300 ms like AGS'
//! `app.tsx`, skipping Hyprland's headless `FALLBACK` output.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{gdk, glib};
use hw_core::Signal;
use hw_core::reactive::remove_source;

pub const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub monitor: gdk::Monitor,
    pub connector: String,
    pub description: String,
    pub manufacturer: String,
    pub model: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub refresh_rate: i32,
}

impl MonitorInfo {
    fn new(m: gdk::Monitor) -> Option<Self> {
        let connector = m.connector()?.to_string();
        if connector == "FALLBACK" || connector.starts_with("HEADLESS") {
            return None;
        }
        let g = m.geometry();
        Some(Self {
            connector,
            description: m.description().map(String::from).unwrap_or_default(),
            manufacturer: m.manufacturer().map(String::from).unwrap_or_default(),
            model: m.model().map(String::from).unwrap_or_default(),
            x: g.x(),
            y: g.y(),
            width: g.width(),
            height: g.height(),
            scale: m.scale(),
            refresh_rate: m.refresh_rate(),
            monitor: m,
        })
    }
}

/// Currently connected monitors, sorted by position.
pub fn current() -> Vec<MonitorInfo> {
    let Some(display) = gdk::Display::default() else {
        return Vec::new();
    };
    let list = display.monitors();
    let mut v: Vec<MonitorInfo> = (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
        .filter(|m| m.is_valid())
        .filter_map(MonitorInfo::new)
        .collect();
    v.sort_by_key(|m| (m.x, m.y));
    v
}

/// Signal of [`current`] monitors, updated 300 ms after the last change.
pub fn watch() -> Signal<Vec<MonitorInfo>> {
    let sig = Signal::new(current());
    let Some(display) = gdk::Display::default() else {
        return sig;
    };
    let pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let weak = sig.downgrade();
    let handler = display.monitors().connect_items_changed(move |_, _, _, _| {
        if let Some(id) = pending.borrow_mut().take() {
            remove_source(id);
        }
        let weak = weak.clone();
        let p2 = pending.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            p2.borrow_mut().take();
            if let Some(s) = weak.upgrade() {
                s.set(current());
            }
        });
        *pending.borrow_mut() = Some(id);
    });
    let list = display.monitors();
    sig.keep_alive(DisconnectOnDrop(list.upcast(), Some(handler)));
    sig
}

struct DisconnectOnDrop(glib::Object, Option<glib::SignalHandlerId>);

impl Drop for DisconnectOnDrop {
    fn drop(&mut self) {
        if let Some(h) = self.1.take() {
            self.0.disconnect(h);
        }
    }
}
