//! The `org.freedesktop.Notifications` object. Runs on the zbus executor
//! thread and forwards everything to the main thread.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

use super::hints::{Hints, parse_actions, parse_hints};

pub const PATH: &str = "/org/freedesktop/Notifications";
pub const NAME: &str = "org.freedesktop.Notifications";

pub enum Event {
    Notify(Box<Incoming>),
    Close(u32),
}

pub struct Incoming {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub hints: Hints,
    pub expire_timeout: i32,
}

pub struct Server {
    pub tx: async_channel::Sender<Event>,
    pub next_id: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec![
            "actions",
            "body",
            "body-hyperlinks",
            "body-markup",
            "icon-static",
            "persistence",
            "sound",
        ]
    }

    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            self.next_id.fetch_max(replaces_id + 1, Ordering::Relaxed);
            replaces_id
        } else {
            // Skip 0 on wrap-around: it means "no id" in the protocol.
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            if id == 0 {
                self.next_id.fetch_add(1, Ordering::Relaxed)
            } else {
                id
            }
        };
        let incoming = Incoming {
            id,
            app_name,
            app_icon,
            summary,
            body,
            actions: parse_actions(&actions),
            hints: parse_hints(&hints),
            expire_timeout,
        };
        let _ = self.tx.try_send(Event::Notify(Box::new(incoming)));
        id
    }

    fn close_notification(&self, id: u32) {
        let _ = self.tx.try_send(Event::Close(id));
    }

    #[zbus(out_args("name", "vendor", "version", "spec_version"))]
    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("hyprwidget", "hyprwidget", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[zbus(signal)]
    pub async fn notification_closed(
        emitter: &SignalEmitter<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn action_invoked(
        emitter: &SignalEmitter<'_>,
        id: u32,
        action_key: &str,
    ) -> zbus::Result<()>;
}
