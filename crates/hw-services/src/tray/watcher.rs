//! `org.kde.StatusNotifierWatcher`, served when nobody else owns it.

use std::sync::{Arc, Mutex};

use zbus::message::Header;
use zbus::object_server::SignalEmitter;

pub const NAME: &str = "org.kde.StatusNotifierWatcher";
pub const PATH: &str = "/StatusNotifierWatcher";

#[derive(Default)]
pub struct Registry {
    pub items: Vec<String>,
    pub hosts: Vec<String>,
}

pub struct Watcher {
    pub reg: Arc<Mutex<Registry>>,
}

/// Normalizes what an item registered into `bus/object/path`.
pub fn item_id(service: &str, sender: &str) -> String {
    if service.starts_with('/') {
        format!("{sender}{service}")
    } else if let Some((bus, path)) = service.split_once('/') {
        format!("{bus}/{path}")
    } else {
        format!("{service}/StatusNotifierItem")
    }
}

/// Splits an item id into bus name and object path.
pub fn split_id(id: &str) -> (&str, String) {
    match id.split_once('/') {
        Some((bus, path)) => (bus, format!("/{path}")),
        None => (id, "/StatusNotifierItem".to_owned()),
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    async fn register_status_notifier_item(
        &self,
        service: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) {
        let sender = hdr.sender().map(|s| s.to_string()).unwrap_or_default();
        let id = item_id(service, &sender);
        let added = {
            let mut reg = self.reg.lock().unwrap();
            if reg.items.contains(&id) {
                false
            } else {
                reg.items.push(id.clone());
                true
            }
        };
        if added {
            tracing::debug!("tray watcher: + {id}");
            let _ = Self::status_notifier_item_registered(&em, &id).await;
            let _ = self.registered_status_notifier_items_changed(&em).await;
        }
    }

    async fn register_status_notifier_host(
        &self,
        service: &str,
        #[zbus(signal_emitter)] em: SignalEmitter<'_>,
    ) {
        {
            let mut reg = self.reg.lock().unwrap();
            if !reg.hosts.iter().any(|h| h == service) {
                reg.hosts.push(service.to_owned());
            }
        }
        let _ = Self::status_notifier_host_registered(&em).await;
        let _ = self.is_status_notifier_host_registered_changed(&em).await;
    }

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.reg.lock().unwrap().items.clone()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        !self.reg.lock().unwrap().hosts.is_empty()
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    pub async fn status_notifier_item_registered(
        em: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn status_notifier_item_unregistered(
        em: &SignalEmitter<'_>,
        service: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_registered(em: &SignalEmitter<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn status_notifier_host_unregistered(em: &SignalEmitter<'_>) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ids() {
        assert_eq!(
            item_id("/org/ayatana/NotificationItem/x", ":1.5"),
            ":1.5/org/ayatana/NotificationItem/x"
        );
        assert_eq!(
            item_id("org.kde.StatusNotifierItem-1-1", ":1.5"),
            "org.kde.StatusNotifierItem-1-1/StatusNotifierItem"
        );
        assert_eq!(
            item_id(":1.9/StatusNotifierItem", ":1.5"),
            ":1.9/StatusNotifierItem"
        );
        assert_eq!(split_id(":1.5/org/a/b"), (":1.5", "/org/a/b".to_owned()));
        assert_eq!(split_id(":1.5"), (":1.5", "/StatusNotifierItem".to_owned()));
    }
}
