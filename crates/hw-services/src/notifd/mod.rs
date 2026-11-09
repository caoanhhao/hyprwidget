//! Notification daemon: a full `org.freedesktop.Notifications` server
//! (spec 1.2) with persisted history and a do-not-disturb flag.
//!
//! If another daemon owns the name (e.g. AGS), we queue for it: history is
//! still available, `active` stays `false`, and we take over automatically
//! once the other daemon exits.

pub mod hints;
mod server;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use hw_core::reactive::{Signal, remove_source};
use serde::{Deserialize, Serialize};
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::object_server::SignalEmitter;

use crate::dbus;
pub use hints::Urgency;
use hints::{ImageHint, uri_to_path};
use server::{Event, Incoming, NAME, PATH, Server};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "spec hints persisted and serialized to Lua as flat booleans"
)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<Action>,
    pub urgency: Urgency,
    /// Milliseconds as sent: -1 = server default, 0 = never.
    pub expire_timeout: i32,
    /// Unix seconds.
    pub time: i64,
    /// `image-data` saved as a PNG, an `image-path` file, or an icon name.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub desktop_entry: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub resident: bool,
    #[serde(default)]
    pub transient: bool,
    #[serde(default)]
    pub action_icons: bool,
    #[serde(default)]
    pub sound_file: Option<String>,
    #[serde(default)]
    pub sound_name: Option<String>,
    #[serde(default)]
    pub suppress_sound: bool,
    #[serde(default)]
    pub value: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    Closed = 3,
    Undefined = 4,
}

impl CloseReason {
    /// The reason's number in the `NotificationClosed` signal.
    pub fn code(self) -> u32 {
        match self {
            Self::Expired => 1,
            Self::Dismissed => 2,
            Self::Closed => 3,
            Self::Undefined => 4,
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    dnd: bool,
    #[serde(default)]
    next_id: u32,
    #[serde(default)]
    notifications: Vec<Notification>,
}

/// Upper bound on kept notifications, oldest dropped first.
const MAX_HISTORY: usize = 500;

pub struct Notifd {
    /// Unresolved notifications, newest first (persisted, minus transient).
    pub notifications: Signal<Vec<Notification>>,
    /// Do-not-disturb (persisted). Notifications are still recorded; popups
    /// are the UI's business.
    pub dnd: Signal<bool>,
    /// Whether we own `org.freedesktop.Notifications`.
    pub active: Signal<bool>,
    /// Last `(id, replaced)` received; notifies on every notification.
    pub notified: Signal<(u32, bool)>,
    /// Last `(id, reason)` resolved; notifies on every close.
    pub resolved: Signal<(u32, CloseReason)>,
    /// Ignore senders' expire timeouts (keep everything until dismissed).
    pub ignore_timeout: Cell<bool>,
    conn: RefCell<Option<zbus::Connection>>,
    timers: RefCell<HashMap<u32, glib::SourceId>>,
    save_pending: Cell<bool>,
    history_file: PathBuf,
    images_dir: PathBuf,
    next_id: Arc<AtomicU32>,
}

impl std::fmt::Debug for Notifd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Notifd")
            .field("ignore_timeout", &self.ignore_timeout)
            .field("history_file", &self.history_file)
            .field("images_dir", &self.images_dir)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

thread_local! {
    static INSTANCE: OnceCell<Rc<Notifd>> = const { OnceCell::new() };
}

/// The daemon, started on first use (main thread only).
pub fn get() -> Rc<Notifd> {
    INSTANCE.with(|i| {
        Rc::clone(i.get_or_init(|| {
            let n = Notifd::load();
            let weak = Rc::downgrade(&n);
            dbus::spawn("notifd", async move { Notifd::serve(weak).await });
            n
        }))
    })
}

impl Notifd {
    fn load() -> Rc<Self> {
        let dir = dbus::cache_dir("notifd");
        let history_file = dir.join("history.json");
        let images_dir = dir.join("images");
        let _ = std::fs::create_dir_all(&images_dir);
        let saved: Saved = std::fs::read(&history_file)
            .ok()
            .and_then(|b| match serde_json::from_slice(&b) {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("{}: {e}", history_file.display());
                    None
                }
            })
            .unwrap_or_default();
        let max_id = saved.notifications.iter().map(|n| n.id).max().unwrap_or(0);
        let next = saved.next_id.max(max_id.saturating_add(1)).max(1);
        Rc::new(Self {
            notifications: Signal::new(saved.notifications),
            dnd: Signal::new(saved.dnd),
            active: Signal::new(false),
            notified: Signal::new((0, false)),
            resolved: Signal::new((0, CloseReason::Undefined)),
            ignore_timeout: Cell::new(false),
            conn: RefCell::new(None),
            timers: RefCell::default(),
            save_pending: Cell::new(false),
            history_file,
            images_dir,
            next_id: Arc::new(AtomicU32::new(next)),
        })
    }

    async fn serve(weak: std::rc::Weak<Self>) -> anyhow::Result<()> {
        let conn = dbus::session().await?;
        let Some(this) = weak.upgrade() else {
            return Ok(());
        };
        let (tx, rx) = async_channel::unbounded();
        conn.object_server()
            .at(
                PATH,
                Server {
                    tx,
                    next_id: Arc::clone(&this.next_id),
                },
            )
            .await?;
        *this.conn.borrow_mut() = Some(conn.clone());
        drop(this);

        let proxy = DBusProxy::new(&conn).await?;
        let mut acquired = proxy.receive_name_acquired().await?;
        let mut lost = proxy.receive_name_lost().await?;
        // No DoNotQueue: if another daemon owns the name we wait in line and
        // take over when it exits, without disturbing it.
        let reply = conn
            .request_name_with_flags(NAME, RequestNameFlags::AllowReplacement.into())
            .await?;
        match reply {
            RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {
                tracing::info!("notifd: owning {NAME}");
                if let Some(this) = weak.upgrade() {
                    this.active.set(true);
                }
            }
            RequestNameReply::InQueue | RequestNameReply::Exists => {
                let owner = proxy
                    .get_name_owner(NAME.try_into()?)
                    .await
                    .map(|o| o.to_string())
                    .unwrap_or_default();
                tracing::warn!(
                    "notifd: {NAME} is owned by {owner}; queued, will take over when it exits"
                );
            }
        }

        let w = std::rc::Weak::clone(&weak);
        glib::spawn_future_local(async move {
            while let Some(sig) = acquired.next().await {
                if let (Ok(a), Some(this)) = (sig.args(), w.upgrade())
                    && a.name == NAME
                {
                    tracing::info!("notifd: acquired {NAME}");
                    this.active.set(true);
                }
            }
        });
        let w = std::rc::Weak::clone(&weak);
        glib::spawn_future_local(async move {
            while let Some(sig) = lost.next().await {
                if let (Ok(a), Some(this)) = (sig.args(), w.upgrade())
                    && a.name == NAME
                {
                    tracing::warn!("notifd: lost {NAME}");
                    this.active.set(false);
                }
            }
        });

        while let Ok(ev) = rx.recv().await {
            let Some(this) = weak.upgrade() else { break };
            match ev {
                Event::Notify(inc) => this.add(*inc),
                Event::Close(id) => this.close(id, CloseReason::Closed),
            }
        }
        Ok(())
    }

    fn add(self: &Rc<Self>, inc: Incoming) {
        let h = inc.hints;
        let image = match h.image {
            Some(ImageHint::Data(img)) => dbus::save_png(
                &self.images_dir,
                img.data,
                if img.has_alpha {
                    dbus::Channels::Rgba
                } else {
                    dbus::Channels::Rgb
                },
                img.width,
                img.height,
                img.rowstride,
            )
            .map(|p| p.to_string_lossy().into_owned()),
            Some(ImageHint::Path(p)) => Some(p),
            None => None,
        };
        let n = Notification {
            id: inc.id,
            app_name: inc.app_name,
            app_icon: uri_to_path(&inc.app_icon),
            summary: inc.summary,
            body: inc.body,
            actions: inc
                .actions
                .into_iter()
                .map(|(id, label)| Action { id, label })
                .collect(),
            urgency: h.urgency,
            expire_timeout: inc.expire_timeout,
            time: dbus::unix_now(),
            image,
            desktop_entry: h.desktop_entry,
            category: h.category,
            resident: h.resident,
            transient: h.transient,
            action_icons: h.action_icons,
            sound_file: h.sound_file,
            sound_name: h.sound_name,
            suppress_sound: h.suppress_sound,
            value: h.value,
        };
        let id = n.id;
        let timeout = n.expire_timeout;
        let critical = n.urgency == Urgency::Critical;
        let mut replaced = false;
        let mut dropped = Vec::new();
        self.notifications.update(|list| {
            if let Some(pos) = list.iter().position(|o| o.id == id) {
                dropped.push(list.remove(pos));
                replaced = true;
            }
            list.insert(0, n);
            while list.len() > MAX_HISTORY {
                dropped.extend(list.pop());
            }
        });
        self.cancel_timer(id);
        for old in dropped {
            if old.id != id {
                self.cancel_timer(old.id);
            }
            self.forget_image(old.image.as_deref());
        }
        if timeout > 0 && !critical && !self.ignore_timeout.get() {
            let weak = Rc::downgrade(self);
            let src = glib::timeout_add_local_once(
                Duration::from_millis(u64::from(timeout.unsigned_abs())),
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.timers.borrow_mut().remove(&id);
                        this.close(id, CloseReason::Expired);
                    }
                },
            );
            self.timers.borrow_mut().insert(id, src);
        }
        self.notified.replace((id, replaced));
        self.schedule_save();
    }

    fn cancel_timer(&self, id: u32) {
        if let Some(src) = self.timers.borrow_mut().remove(&id) {
            remove_source(src);
        }
    }

    /// Removes notification `id`, emitting `NotificationClosed`.
    pub fn close(self: &Rc<Self>, id: u32, reason: CloseReason) {
        let mut removed = None;
        self.notifications.update(|list| {
            if let Some(pos) = list.iter().position(|n| n.id == id) {
                removed = Some(list.remove(pos));
            }
        });
        let Some(n) = removed else {
            return;
        };
        self.cancel_timer(id);
        self.forget_image(n.image.as_deref());
        self.emit(
            move |em| async move { Server::notification_closed(&em, id, reason.code()).await },
        );
        self.resolved.replace((id, reason));
        self.schedule_save();
    }

    /// Closes as dismissed by the user.
    pub fn dismiss(self: &Rc<Self>, id: u32) {
        self.close(id, CloseReason::Dismissed);
    }

    /// Dismisses everything.
    pub fn clear(self: &Rc<Self>) {
        let ids: Vec<u32> = self.notifications.peek().iter().map(|n| n.id).collect();
        for id in ids {
            self.dismiss(id);
        }
    }

    /// Sends `ActionInvoked`; non-resident notifications are then dismissed.
    pub fn invoke(self: &Rc<Self>, id: u32, action: &str) {
        let Some(resident) = self
            .notifications
            .peek()
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.resident)
        else {
            return;
        };
        let key = action.to_owned();
        self.emit(move |em| async move { Server::action_invoked(&em, id, &key).await });
        if !resident {
            self.dismiss(id);
        }
    }

    pub fn get(&self, id: u32) -> Option<Notification> {
        self.notifications.peek().into_iter().find(|n| n.id == id)
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "public API called from hw-lua with a Lua boolean"
    )]
    pub fn set_dnd(self: &Rc<Self>, on: bool) {
        self.dnd.set(on);
        self.schedule_save();
    }

    /// Emits a signal on our object, only while we own the name (ids of a
    /// queued instance would collide with the real daemon's).
    fn emit<F, Fut>(&self, f: F)
    where
        F: FnOnce(SignalEmitter<'static>) -> Fut + 'static,
        Fut: Future<Output = zbus::Result<()>> + 'static,
    {
        if !self.active.peek() {
            return;
        }
        let Some(conn) = self.conn.borrow().clone() else {
            return;
        };
        dbus::spawn("notifd signal", async move {
            let em = SignalEmitter::new(&conn, PATH)?.into_owned();
            f(em).await?;
            Ok(())
        });
    }

    /// Deletes an `image-data` PNG once nothing references it.
    fn forget_image(&self, image: Option<&str>) {
        let Some(path) = image else { return };
        if !std::path::Path::new(path).starts_with(&self.images_dir) {
            return;
        }
        let used = self
            .notifications
            .peek()
            .iter()
            .any(|n| n.image.as_deref() == Some(path));
        if !used {
            let _ = std::fs::remove_file(path);
        }
    }

    fn schedule_save(self: &Rc<Self>) {
        if self.save_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(500), move || {
            if let Some(this) = weak.upgrade() {
                this.save_pending.set(false);
                this.save();
            }
        });
    }

    fn save(&self) {
        let saved = Saved {
            dnd: self.dnd.peek(),
            next_id: self.next_id.load(Ordering::Relaxed),
            notifications: self
                .notifications
                .peek()
                .into_iter()
                .filter(|n| !n.transient)
                .collect(),
        };
        let tmp = self.history_file.with_extension("json.tmp");
        let res = serde_json::to_vec(&saved)
            .map_err(std::io::Error::other)
            .and_then(|b| std::fs::write(&tmp, b))
            .and_then(|()| std::fs::rename(&tmp, &self.history_file));
        if let Err(e) = res {
            tracing::warn!("saving {}: {e}", self.history_file.display());
        }
    }
}
