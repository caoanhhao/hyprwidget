//! System tray: StatusNotifierWatcher + StatusNotifierHost (SNI) with
//! DBusMenu menus exposed as `gio::MenuModel`.
//!
//! We serve the watcher when nobody else does; otherwise we queue for its
//! name and act as a host of the existing watcher (e.g. AGS), taking over
//! automatically when it exits.

pub mod menu;
pub mod watcher;

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use hw_core::reactive::Signal;
use serde::Serialize;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::message::Type as MsgType;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::Value;
use zbus::{Connection, MatchRule, MessageStream};

use crate::dbus::{self, Props, p_bool, p_str, unwrap_variant, v_bytes, v_i64, v_str};
pub use menu::{ACTION_PREFIX, DbusMenu};
use watcher::{Registry, Watcher, split_id};

const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TrayItem {
    /// `bus/object/path` — the id used by the control methods.
    pub id: String,
    pub bus_name: String,
    pub object_path: String,
    /// The app's own `Id` property.
    pub item_id: String,
    pub title: String,
    /// `ApplicationStatus`, `Communications`, `SystemServices`, `Hardware`.
    pub category: String,
    /// `Passive`, `Active`, `NeedsAttention`.
    pub status: String,
    /// What to show: an icon file (pixmap or themed path) or an icon name.
    pub icon: String,
    pub icon_name: String,
    pub icon_theme_path: String,
    pub attention_icon_name: String,
    pub overlay_icon_name: String,
    pub tooltip_title: String,
    pub tooltip_body: String,
    /// `title` + newline + `body`, or the item title.
    pub tooltip_text: String,
    /// The item only supports a menu (no `Activate`).
    pub item_is_menu: bool,
    pub menu_path: String,
    pub has_menu: bool,
}

pub struct Tray {
    pub items: Signal<Vec<TrayItem>>,
    /// Whether we serve `org.kde.StatusNotifierWatcher` ourselves.
    pub watcher_active: Signal<bool>,
    tasks: RefCell<HashMap<String, glib::JoinHandle<()>>>,
    menus: RefCell<HashMap<String, Rc<DbusMenu>>>,
    conn: RefCell<Option<Connection>>,
}

thread_local! {
    static INSTANCE: OnceCell<Rc<Tray>> = const { OnceCell::new() };
}

/// Started on first use (main thread only).
pub fn get() -> Rc<Tray> {
    INSTANCE.with(|i| {
        i.get_or_init(|| {
            let this = Rc::new(Tray {
                items: Signal::new(Vec::new()),
                watcher_active: Signal::new(false),
                tasks: RefCell::default(),
                menus: RefCell::default(),
                conn: RefCell::new(None),
            });
            let weak = Rc::downgrade(&this);
            dbus::spawn("tray", async move { Tray::run(weak).await });
            this
        })
        .clone()
    })
}

impl Tray {
    async fn run(weak: Weak<Self>) -> anyhow::Result<()> {
        let conn = dbus::session().await?;
        if let Some(this) = weak.upgrade() {
            *this.conn.borrow_mut() = Some(conn.clone());
        }
        let reg = Arc::new(Mutex::new(Registry::default()));
        conn.object_server()
            .at(watcher::PATH, Watcher { reg: reg.clone() })
            .await?;

        let proxy = DBusProxy::new(&conn).await?;
        let mut owners = proxy.receive_name_owner_changed().await?;
        let mut acquired = proxy.receive_name_acquired().await?;
        let mut lost = proxy.receive_name_lost().await?;
        let rule = MatchRule::builder()
            .msg_type(MsgType::Signal)
            .interface(watcher::NAME)?
            .path(watcher::PATH)?
            .build();
        let mut watcher_signals = MessageStream::for_match_rule(rule, &conn, None).await?;

        match conn
            .request_name_with_flags(watcher::NAME, RequestNameFlags::AllowReplacement.into())
            .await?
        {
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
                tracing::info!("tray: owning {}", watcher::NAME);
                if let Some(this) = weak.upgrade() {
                    this.watcher_active.set(true);
                }
            }
            _ => tracing::info!("tray: {} is owned elsewhere; acting as host", watcher::NAME),
        }
        let host = format!("org.kde.StatusNotifierHost-{}", std::process::id());
        conn.request_name(host.as_str()).await?;
        Self::sync(&weak, &conn, &host).await;

        let w = weak.clone();
        glib::spawn_future_local(async move {
            while let Some(s) = acquired.next().await {
                if let (Ok(a), Some(this)) = (s.args(), w.upgrade())
                    && a.name == watcher::NAME
                {
                    tracing::info!("tray: acquired {}", watcher::NAME);
                    this.watcher_active.set(true);
                }
            }
        });
        let w = weak.clone();
        glib::spawn_future_local(async move {
            while let Some(s) = lost.next().await {
                if let (Ok(a), Some(this)) = (s.args(), w.upgrade())
                    && a.name == watcher::NAME
                {
                    this.watcher_active.set(false);
                }
            }
        });

        let w = weak.clone();
        let c = conn.clone();
        glib::spawn_future_local(async move {
            while let Some(Ok(msg)) = watcher_signals.next().await {
                let Some(this) = w.upgrade() else { break };
                let member = msg
                    .header()
                    .member()
                    .map(|m| m.to_string())
                    .unwrap_or_default();
                let Ok(id) = msg.body().deserialize::<String>() else {
                    continue;
                };
                match member.as_str() {
                    "StatusNotifierItemRegistered" => this.add(&c, &id),
                    "StatusNotifierItemUnregistered" => this.remove(&id),
                    _ => {}
                }
            }
        });

        while let Some(sig) = owners.next().await {
            let Ok(a) = sig.args() else { continue };
            let name = a.name.to_string();
            if name == watcher::NAME {
                if a.new_owner.is_some() {
                    // A new watcher: register again and resync.
                    Self::sync(&weak, &conn, &host).await;
                }
                continue;
            }
            if a.new_owner.is_some() {
                continue;
            }
            // `name` vanished: drop its items from our watcher and host.
            let gone: Vec<String> = {
                let mut r = reg.lock().unwrap();
                let gone = r
                    .items
                    .iter()
                    .filter(|i| split_id(i).0 == name)
                    .cloned()
                    .collect();
                r.items.retain(|i| split_id(i).0 != name);
                r.hosts.retain(|h| *h != name);
                gone
            };
            if !gone.is_empty()
                && let Ok(em) = SignalEmitter::new(&conn, watcher::PATH)
            {
                for id in &gone {
                    let _ = Watcher::status_notifier_item_unregistered(&em, id).await;
                }
            }
            let Some(this) = weak.upgrade() else { break };
            let ids: Vec<String> = this
                .items
                .peek()
                .iter()
                .filter(|i| i.bus_name == name)
                .map(|i| i.id.clone())
                .collect();
            for id in ids {
                this.remove(&id);
            }
        }
        Ok(())
    }

    /// Registers as host with the current watcher and loads its items.
    async fn sync(weak: &Weak<Self>, conn: &Connection, host: &str) {
        let res: anyhow::Result<Vec<String>> = async {
            let proxy = zbus::Proxy::new(conn, watcher::NAME, watcher::PATH, watcher::NAME).await?;
            proxy
                .call_method("RegisterStatusNotifierHost", &(host,))
                .await?;
            Ok(proxy
                .get_property::<Vec<String>>("RegisteredStatusNotifierItems")
                .await?)
        }
        .await;
        let ids = match res {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("tray: talking to the watcher: {e:#}");
                return;
            }
        };
        let Some(this) = weak.upgrade() else { return };
        let stale: Vec<String> = this
            .items
            .peek()
            .iter()
            .filter(|i| !ids.contains(&i.id))
            .map(|i| i.id.clone())
            .collect();
        for id in stale {
            this.remove(&id);
        }
        for id in ids {
            this.add(conn, &id);
        }
    }

    fn add(self: &Rc<Self>, conn: &Connection, id: &str) {
        if self.tasks.borrow().contains_key(id) {
            return;
        }
        let (bus, path) = split_id(id);
        tracing::debug!("tray: + {id}");
        self.items.update(|items| {
            items.push(TrayItem {
                id: id.to_owned(),
                bus_name: bus.to_owned(),
                object_path: path.clone(),
                ..Default::default()
            })
        });
        let (weak, conn, id2, bus) = (
            Rc::downgrade(self),
            conn.clone(),
            id.to_owned(),
            bus.to_owned(),
        );
        let task = glib::spawn_future_local(async move {
            let res: anyhow::Result<()> = async {
                let proxy = zbus::Proxy::new(&conn, bus.clone(), path.clone(), ITEM_IFACE).await?;
                let mut signals = proxy.receive_all_signals().await?;
                let props = dbus::props_proxy(&conn, &bus, &path).await?;
                loop {
                    let p = props.get_all(ITEM_IFACE.try_into()?).await?;
                    let Some(this) = weak.upgrade() else { break };
                    this.apply(&id2, &p);
                    drop(this);
                    // Wait for a `New*` signal, then coalesce bursts.
                    loop {
                        let Some(msg) = signals.next().await else {
                            return Ok(());
                        };
                        let m = msg
                            .header()
                            .member()
                            .map(|m| m.to_string())
                            .unwrap_or_default();
                        if m.starts_with("New") {
                            break;
                        }
                    }
                    glib::timeout_future(Duration::from_millis(100)).await;
                }
                Ok(())
            }
            .await;
            if let Err(e) = res {
                tracing::debug!("tray item {id2}: {e:#}");
            }
        });
        self.tasks.borrow_mut().insert(id.to_owned(), task);
    }

    fn remove(&self, id: &str) {
        let Some(task) = self.tasks.borrow_mut().remove(id) else {
            return;
        };
        tracing::debug!("tray: - {id}");
        task.abort();
        self.menus.borrow_mut().remove(id);
        self.items.update(|items| items.retain(|i| i.id != id));
    }

    fn apply(&self, id: &str, p: &Props) {
        let mut items = self.items.peek();
        let Some(it) = items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        it.item_id = p_str(p, "Id");
        it.title = p_str(p, "Title");
        it.category = p_str(p, "Category");
        it.status = p_str(p, "Status");
        it.icon_name = p_str(p, "IconName");
        it.icon_theme_path = p_str(p, "IconThemePath");
        it.attention_icon_name = p_str(p, "AttentionIconName");
        it.overlay_icon_name = p_str(p, "OverlayIconName");
        it.item_is_menu = p_bool(p, "ItemIsMenu");
        let menu = p_str(p, "Menu");
        it.has_menu = !menu.is_empty() && menu != "/";
        if it.menu_path != menu {
            self.menus.borrow_mut().remove(id);
        }
        it.menu_path = menu;
        let (tt, tb) = p.get("ToolTip").map(|v| tooltip(v)).unwrap_or_default();
        it.tooltip_title = tt;
        it.tooltip_body = tb;
        it.tooltip_text = match (it.tooltip_title.is_empty(), it.tooltip_body.is_empty()) {
            (false, false) => format!("{}\n{}", it.tooltip_title, it.tooltip_body),
            (false, true) => it.tooltip_title.clone(),
            (true, false) => it.tooltip_body.clone(),
            (true, true) => it.title.clone(),
        };
        let attention = it.status == "NeedsAttention";
        let (name, pixmap) = if attention
            && (!it.attention_icon_name.is_empty() || p.contains_key("AttentionIconPixmap"))
        {
            (it.attention_icon_name.clone(), p.get("AttentionIconPixmap"))
        } else {
            (it.icon_name.clone(), p.get("IconPixmap"))
        };
        it.icon = resolve_icon(&name, &it.icon_theme_path)
            .or_else(|| pixmap.and_then(|v| pixmap_file(v)))
            .unwrap_or_default();
        self.items.set(items);
    }

    fn call(
        &self,
        id: &str,
        method: &'static str,
        args: impl serde::Serialize + zbus::zvariant::DynamicType + 'static,
    ) {
        let (bus, path) = split_id(id);
        let bus = bus.to_owned();
        dbus::spawn("tray call", async move {
            let conn = dbus::session().await?;
            conn.call_method(
                Some(bus.as_str()),
                path.as_str(),
                Some(ITEM_IFACE),
                method,
                &args,
            )
            .await?;
            Ok(())
        });
    }

    /// Primary click at screen position `x`, `y`.
    pub fn activate(&self, id: &str, x: i32, y: i32) {
        self.call(id, "Activate", (x, y));
    }

    /// Middle click.
    pub fn secondary_activate(&self, id: &str, x: i32, y: i32) {
        self.call(id, "SecondaryActivate", (x, y));
    }

    /// Asks the app to show its own context menu (items without DBusMenu).
    pub fn context_menu(&self, id: &str, x: i32, y: i32) {
        self.call(id, "ContextMenu", (x, y));
    }

    /// `orientation`: `vertical` or `horizontal`.
    pub fn scroll(&self, id: &str, delta: i32, orientation: &str) {
        self.call(id, "Scroll", (delta, orientation.to_owned()));
    }

    /// The item's menu (created on first use, kept in sync).
    pub fn menu(&self, id: &str) -> Option<Rc<DbusMenu>> {
        if let Some(m) = self.menus.borrow().get(id) {
            return Some(m.clone());
        }
        let item = self.items.peek().into_iter().find(|i| i.id == id)?;
        if !item.has_menu {
            return None;
        }
        let conn = self.conn.borrow().clone()?;
        let m = DbusMenu::new(conn, &item.bus_name, &item.menu_path);
        self.menus.borrow_mut().insert(id.to_owned(), m.clone());
        Some(m)
    }
}

/// `(sa(iiay)ss)` → (title, body).
fn tooltip(v: &Value) -> (String, String) {
    let Value::Structure(s) = unwrap_variant(v) else {
        return Default::default();
    };
    let f = s.fields();
    if f.len() < 4 {
        return Default::default();
    }
    (
        v_str(&f[2]).unwrap_or_default(),
        v_str(&f[3]).unwrap_or_default(),
    )
}

/// An icon name, an absolute file, or a file inside the item's theme path.
fn resolve_icon(name: &str, theme_path: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Path::new(name).exists().then(|| name.to_owned());
    }
    if !theme_path.is_empty()
        && let Some(f) = find_in_theme_path(Path::new(theme_path), name, 0)
    {
        return Some(f.to_string_lossy().into_owned());
    }
    Some(name.to_owned())
}

fn find_in_theme_path(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    for ext in ["png", "svg", "xpm"] {
        let f = dir.join(format!("{name}.{ext}"));
        if f.is_file() {
            return Some(f);
        }
    }
    if depth >= 4 {
        return None;
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    // Prefer bigger sizes (`64x64` over `16x16`), `scalable` last.
    dirs.sort_by_key(|p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        std::cmp::Reverse(
            n.split('x')
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0),
        )
    });
    dirs.iter()
        .find_map(|d| find_in_theme_path(d, name, depth + 1))
}

/// Largest `a(iiay)` ARGB32 pixmap → PNG in the runtime dir.
fn pixmap_file(v: &Value) -> Option<String> {
    let Value::Array(a) = unwrap_variant(v) else {
        return None;
    };
    let (w, h, argb) = a
        .iter()
        .filter_map(|p| {
            let Value::Structure(s) = unwrap_variant(p) else {
                return None;
            };
            let f = s.fields();
            let (w, h) = (v_i64(f.first()?)? as i32, v_i64(f.get(1)?)? as i32);
            let data = v_bytes(f.get(2)?)?;
            (w > 0 && h > 0 && data.len() >= (w * h * 4) as usize).then_some((w, h, data))
        })
        .max_by_key(|(w, h, _)| w * h)?;
    let rgba = argb_to_rgba(&argb[..(w * h * 4) as usize]);
    dbus::save_png(&dbus::runtime_dir("tray"), rgba, true, w, h, w * 4)
        .map(|p| p.to_string_lossy().into_owned())
}

/// SNI pixmaps are ARGB32 in network byte order.
fn argb_to_rgba(argb: &[u8]) -> Vec<u8> {
    argb.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|[a, r, g, b]| [*r, *g, *b, *a])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argb_conversion() {
        assert_eq!(
            argb_to_rgba(&[0xff, 1, 2, 3, 0x80, 4, 5, 6]),
            vec![1, 2, 3, 0xff, 4, 5, 6, 0x80]
        );
    }

    #[test]
    fn icon_resolution() {
        assert_eq!(resolve_icon("", ""), None);
        assert_eq!(resolve_icon("firefox", ""), Some("firefox".into()));
        let dir = std::env::temp_dir().join(format!("hw-tray-test-{}", std::process::id()));
        let sub = dir.join("hicolor/22x22/apps");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("myapp.png"), b"x").unwrap();
        assert_eq!(
            resolve_icon("myapp", dir.to_str().unwrap()),
            Some(sub.join("myapp.png").to_string_lossy().into_owned())
        );
        assert_eq!(
            resolve_icon("other", dir.to_str().unwrap()),
            Some("other".into())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
