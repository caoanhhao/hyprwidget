//! `com.canonical.dbusmenu` client → `gio::MenuModel` + action group (use
//! with a GTK `MenuButton`/`PopoverMenu` and the `dbusmenu` action prefix).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use futures_util::StreamExt;
use gio::prelude::*;
use zbus::Connection;
use zbus::zvariant::{OwnedValue, Value};

use crate::dbus::{self, unwrap_variant, v_bool, v_i64, v_str};

const IFACE: &str = "com.canonical.dbusmenu";
/// Prefix to pass to `insert_action_group`.
pub const ACTION_PREFIX: &str = "dbusmenu";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Toggle {
    #[default]
    None,
    Checkmark,
    Radio,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MenuNode {
    pub id: i32,
    pub label: String,
    pub separator: bool,
    pub enabled: bool,
    pub visible: bool,
    pub icon_name: String,
    pub toggle: Toggle,
    /// `Some(true)` checked, `Some(false)` unchecked, `None` indeterminate.
    pub toggle_state: Option<bool>,
    pub submenu: bool,
    pub children: Vec<MenuNode>,
}

impl MenuNode {
    fn from_props(id: i32, props: &HashMap<String, &Value>) -> Self {
        let s = |k: &str| props.get(k).and_then(|v| v_str(v)).unwrap_or_default();
        let b = |k: &str, d: bool| props.get(k).and_then(|v| v_bool(v)).unwrap_or(d);
        Self {
            id,
            label: s("label"),
            separator: s("type") == "separator",
            enabled: b("enabled", true),
            visible: b("visible", true),
            icon_name: s("icon-name"),
            toggle: match s("toggle-type").as_str() {
                "checkmark" => Toggle::Checkmark,
                "radio" => Toggle::Radio,
                _ => Toggle::None,
            },
            toggle_state: match props.get("toggle-state").and_then(|v| v_i64(v)) {
                Some(0) => Some(false),
                Some(1) => Some(true),
                _ => None,
            },
            submenu: s("children-display") == "submenu",
            children: Vec::new(),
        }
    }
}

fn props_of<'a, 'v>(v: &'a Value<'v>) -> HashMap<String, &'a Value<'v>> {
    crate::dbus::v_dict(v)
}

/// Parses one `(ia{sv}av)` node (possibly wrapped in a variant).
pub fn parse_node(v: &Value) -> Option<MenuNode> {
    let Value::Structure(s) = unwrap_variant(v) else {
        return None;
    };
    let f = s.fields();
    if f.len() != 3 {
        return None;
    }
    let id = v_i64(&f[0])? as i32;
    let mut node = MenuNode::from_props(id, &props_of(&f[1]));
    if let Value::Array(a) = unwrap_variant(&f[2]) {
        node.children = a.iter().filter_map(parse_node).collect();
    }
    Some(node)
}

/// Parses the `layout` returned by `GetLayout`.
pub fn parse_layout(
    id: i32,
    props: &HashMap<String, OwnedValue>,
    children: &[OwnedValue],
) -> MenuNode {
    let p: HashMap<String, &Value> = props
        .iter()
        .map(|(k, v)| (k.clone(), unwrap_variant(v)))
        .collect();
    let mut node = MenuNode::from_props(id, &p);
    node.children = children.iter().filter_map(|c| parse_node(c)).collect();
    node
}

/// Applies an `ItemsPropertiesUpdated` change in place.
pub fn apply_props(
    root: &mut MenuNode,
    id: i32,
    changed: &HashMap<String, OwnedValue>,
    removed: &[String],
) {
    fn find(n: &mut MenuNode, id: i32) -> Option<&mut MenuNode> {
        if n.id == id {
            return Some(n);
        }
        n.children.iter_mut().find_map(|c| find(c, id))
    }
    let Some(n) = find(root, id) else { return };
    let children = std::mem::take(&mut n.children);
    let mut p: HashMap<String, &Value> = HashMap::new();
    // Rebuild from the current values, then overlay the change.
    let current = node_props(n);
    for (k, v) in &current {
        p.insert(k.clone(), v);
    }
    for k in removed {
        p.remove(k);
    }
    for (k, v) in changed {
        p.insert(k.clone(), unwrap_variant(v));
    }
    *n = MenuNode::from_props(id, &p);
    n.children = children;
}

fn node_props(n: &MenuNode) -> Vec<(String, Value<'static>)> {
    let mut v = vec![
        ("label".to_owned(), Value::from(n.label.clone())),
        ("enabled".to_owned(), Value::Bool(n.enabled)),
        ("visible".to_owned(), Value::Bool(n.visible)),
    ];
    if n.separator {
        v.push(("type".into(), Value::from("separator")));
    }
    if !n.icon_name.is_empty() {
        v.push(("icon-name".into(), Value::from(n.icon_name.clone())));
    }
    match n.toggle {
        Toggle::Checkmark => v.push(("toggle-type".into(), Value::from("checkmark"))),
        Toggle::Radio => v.push(("toggle-type".into(), Value::from("radio"))),
        Toggle::None => {}
    }
    if let Some(s) = n.toggle_state {
        v.push(("toggle-state".into(), Value::I32(s as i32)));
    }
    if n.submenu {
        v.push(("children-display".into(), Value::from("submenu")));
    }
    v
}

fn action_name(id: i32) -> String {
    format!("item-{id}")
}

/// Fills `menu` with `nodes`; separators split sections.
fn fill(
    menu: &gio::Menu,
    nodes: &[MenuNode],
    group: &gio::SimpleActionGroup,
    owner: &Weak<DbusMenu>,
) {
    let mut section = gio::Menu::new();
    let flush = |section: &mut gio::Menu| {
        if section.n_items() > 0 {
            menu.append_section(None, section);
            *section = gio::Menu::new();
        }
    };
    for n in nodes.iter().filter(|n| n.visible) {
        if n.separator {
            flush(&mut section);
            continue;
        }
        let item = gio::MenuItem::new(Some(&n.label), None);
        if !n.icon_name.is_empty() {
            item.set_icon(&gio::ThemedIcon::new(&n.icon_name));
        }
        if !n.children.is_empty() || n.submenu {
            let sub = gio::Menu::new();
            fill(&sub, &n.children, group, owner);
            item.set_submenu(Some(&sub));
        } else {
            let name = action_name(n.id);
            let action = match n.toggle {
                Toggle::None => gio::SimpleAction::new(&name, None),
                _ => gio::SimpleAction::new_stateful(
                    &name,
                    None,
                    &n.toggle_state.unwrap_or(false).to_variant(),
                ),
            };
            action.set_enabled(n.enabled);
            let (owner, id) = (owner.clone(), n.id);
            action.connect_activate(move |_, _| {
                if let Some(m) = owner.upgrade() {
                    m.click(id);
                }
            });
            group.add_action(&action);
            item.set_detailed_action(&format!("{ACTION_PREFIX}.{name}"));
        }
        section.append_item(&item);
    }
    flush(&mut section);
}

/// A live DBusMenu: `model` and `actions` are updated in place, so widgets
/// keep working across layout changes.
pub struct DbusMenu {
    pub model: gio::Menu,
    pub actions: gio::SimpleActionGroup,
    pub root: RefCell<Option<MenuNode>>,
    conn: Connection,
    bus: String,
    path: String,
    refresh_pending: Cell<bool>,
    task: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Drop for DbusMenu {
    fn drop(&mut self) {
        if let Some(t) = self.task.take() {
            t.abort();
        }
    }
}

impl DbusMenu {
    pub fn new(conn: Connection, bus: &str, path: &str) -> Rc<Self> {
        let this = Rc::new(Self {
            model: gio::Menu::new(),
            actions: gio::SimpleActionGroup::new(),
            root: RefCell::new(None),
            conn,
            bus: bus.to_owned(),
            path: path.to_owned(),
            refresh_pending: Cell::new(false),
            task: RefCell::new(None),
        });
        let weak = Rc::downgrade(&this);
        let t = glib::spawn_future_local(async move {
            if let Err(e) = Self::run(weak).await {
                tracing::debug!("dbusmenu: {e:#}");
            }
        });
        *this.task.borrow_mut() = Some(t);
        this
    }

    async fn run(weak: Weak<Self>) -> anyhow::Result<()> {
        let Some(this) = weak.upgrade() else {
            return Ok(());
        };
        let proxy =
            zbus::Proxy::new(&this.conn, this.bus.clone(), this.path.clone(), IFACE).await?;
        let mut signals = proxy.receive_all_signals().await?;
        drop(this);
        Self::refresh(weak.clone()).await;
        while let Some(msg) = signals.next().await {
            let Some(this) = weak.upgrade() else { break };
            let member = msg
                .header()
                .member()
                .map(|m| m.to_string())
                .unwrap_or_default();
            match member.as_str() {
                "ItemsPropertiesUpdated" => {
                    type Updated = Vec<(i32, HashMap<String, OwnedValue>)>;
                    type Removed = Vec<(i32, Vec<String>)>;
                    match msg.body().deserialize::<(Updated, Removed)>() {
                        Ok((upd, rem)) if this.root.borrow().is_some() => {
                            {
                                let mut root = this.root.borrow_mut();
                                let root = root.as_mut().expect("checked");
                                for (id, props) in &upd {
                                    apply_props(root, *id, props, &[]);
                                }
                                for (id, keys) in &rem {
                                    apply_props(root, *id, &HashMap::new(), keys);
                                }
                            }
                            this.rebuild();
                        }
                        _ => this.schedule_refresh(),
                    }
                }
                "LayoutUpdated" | "ItemActivationRequested" => this.schedule_refresh(),
                _ => {}
            }
        }
        Ok(())
    }

    fn schedule_refresh(self: &Rc<Self>) {
        if self.refresh_pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            // Coalesce bursts of LayoutUpdated.
            glib::timeout_future(std::time::Duration::from_millis(50)).await;
            if let Some(this) = weak.upgrade() {
                this.refresh_pending.set(false);
            }
            Self::refresh(weak).await;
        });
    }

    async fn refresh(weak: Weak<Self>) {
        let Some(this) = weak.upgrade() else { return };
        let (conn, bus, path) = (this.conn.clone(), this.bus.clone(), this.path.clone());
        drop(this);
        type Layout = (u32, (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>));
        let res: anyhow::Result<Layout> = async {
            let reply = conn
                .call_method(
                    Some(bus.as_str()),
                    path.as_str(),
                    Some(IFACE),
                    "GetLayout",
                    &(0i32, -1i32, Vec::<String>::new()),
                )
                .await?;
            Ok(reply.body().deserialize::<Layout>()?)
        }
        .await;
        let Some(this) = weak.upgrade() else { return };
        match res {
            Ok((_rev, (id, props, children))) => {
                *this.root.borrow_mut() = Some(parse_layout(id, &props, &children));
                this.rebuild();
            }
            Err(e) => tracing::debug!("dbusmenu {bus}{path}: {e:#}"),
        }
    }

    fn rebuild(self: &Rc<Self>) {
        for a in self.actions.list_actions() {
            self.actions.remove_action(&a);
        }
        self.model.remove_all();
        if let Some(root) = &*self.root.borrow() {
            fill(
                &self.model,
                &root.children,
                &self.actions,
                &Rc::downgrade(self),
            );
        }
    }

    fn click(&self, id: i32) {
        let (conn, bus, path) = (self.conn.clone(), self.bus.clone(), self.path.clone());
        dbus::spawn("dbusmenu event", async move {
            let ts = (glib::monotonic_time() / 1000) as u32;
            conn.call_method(
                Some(bus.as_str()),
                path.as_str(),
                Some(IFACE),
                "Event",
                &(id, "clicked", Value::I32(0), ts),
            )
            .await?;
            Ok(())
        });
    }

    /// Tells the app the root menu is about to open (lazy menus populate
    /// here); refreshes the layout if the app asks for it.
    pub fn about_to_show(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let (conn, bus, path) = (self.conn.clone(), self.bus.clone(), self.path.clone());
        dbus::spawn("dbusmenu about-to-show", async move {
            let reply = conn
                .call_method(
                    Some(bus.as_str()),
                    path.as_str(),
                    Some(IFACE),
                    "AboutToShow",
                    &(0i32,),
                )
                .await?;
            if reply.body().deserialize::<bool>().unwrap_or(false)
                && let Some(this) = weak.upgrade()
            {
                this.schedule_refresh();
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::StructureBuilder;

    fn props(pairs: &[(&str, Value<'static>)]) -> HashMap<String, Value<'static>> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn node(
        id: i32,
        p: HashMap<String, Value<'static>>,
        children: Vec<Value<'static>>,
    ) -> Value<'static> {
        let children: Vec<Value<'static>> = children
            .into_iter()
            .map(|c| Value::Value(Box::new(c)))
            .collect();
        Value::Structure(
            StructureBuilder::new()
                .add_field(id)
                .add_field(p)
                .append_field(Value::from(children))
                .build()
                .unwrap(),
        )
    }

    fn owned(p: HashMap<String, Value<'static>>) -> HashMap<String, OwnedValue> {
        p.into_iter()
            .map(|(k, v)| (k, v.try_to_owned().unwrap()))
            .collect()
    }

    fn sample() -> MenuNode {
        let open = node(
            1,
            props(&[
                ("label", Value::from("_Open")),
                ("icon-name", Value::from("document-open")),
            ]),
            vec![],
        );
        let sep = node(2, props(&[("type", Value::from("separator"))]), vec![]);
        let check = node(
            3,
            props(&[
                ("label", Value::from("Mute")),
                ("toggle-type", Value::from("checkmark")),
                ("toggle-state", Value::I32(1)),
                ("enabled", Value::Bool(false)),
            ]),
            vec![],
        );
        let hidden = node(
            4,
            props(&[
                ("label", Value::from("Hidden")),
                ("visible", Value::Bool(false)),
            ]),
            vec![],
        );
        let leaf = node(6, props(&[("label", Value::from("Leaf"))]), vec![]);
        let sub = node(
            5,
            props(&[
                ("label", Value::from("More")),
                ("children-display", Value::from("submenu")),
            ]),
            vec![leaf],
        );
        let children: Vec<OwnedValue> = [open, sep, check, hidden, sub]
            .into_iter()
            .map(|c| Value::Value(Box::new(c)).try_to_owned().unwrap())
            .collect();
        parse_layout(
            0,
            &owned(props(&[("children-display", Value::from("submenu"))])),
            &children,
        )
    }

    #[test]
    fn parses_layout_tree() {
        let root = sample();
        assert_eq!(root.id, 0);
        assert!(root.submenu);
        let ids: Vec<i32> = root.children.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
        let open = &root.children[0];
        assert_eq!(open.label, "_Open");
        assert_eq!(open.icon_name, "document-open");
        assert!(open.enabled && open.visible && !open.separator);
        assert!(root.children[1].separator);
        let check = &root.children[2];
        assert_eq!(check.toggle, Toggle::Checkmark);
        assert_eq!(check.toggle_state, Some(true));
        assert!(!check.enabled);
        assert!(!root.children[3].visible);
        let sub = &root.children[4];
        assert!(sub.submenu);
        assert_eq!(sub.children.len(), 1);
        assert_eq!(sub.children[0].label, "Leaf");
    }

    #[test]
    fn rejects_malformed_nodes() {
        assert_eq!(parse_node(&Value::I32(3)), None);
        let short = Value::Structure(StructureBuilder::new().add_field(1i32).build().unwrap());
        assert_eq!(parse_node(&short), None);
    }

    #[test]
    fn applies_property_updates() {
        let mut root = sample();
        apply_props(
            &mut root,
            3,
            &owned(props(&[
                ("toggle-state", Value::I32(0)),
                ("label", Value::from("Unmute")),
            ])),
            &["enabled".to_string()],
        );
        let check = &root.children[2];
        assert_eq!(check.label, "Unmute");
        assert_eq!(check.toggle_state, Some(false));
        assert!(check.enabled, "removed property falls back to its default");
        assert_eq!(check.toggle, Toggle::Checkmark);
        // Children survive updates of their parent.
        apply_props(
            &mut root,
            5,
            &owned(props(&[("label", Value::from("Extra"))])),
            &[],
        );
        assert_eq!(root.children[4].children.len(), 1);
    }

    #[test]
    fn builds_menu_model() {
        let root = sample();
        let model = gio::Menu::new();
        let group = gio::SimpleActionGroup::new();
        fill(&model, &root.children, &group, &Weak::new());
        // Separator splits into two sections; the hidden item is skipped.
        assert_eq!(model.n_items(), 2);
        let first = model.item_link(0, "section").unwrap();
        assert_eq!(first.n_items(), 1);
        let second = model.item_link(1, "section").unwrap();
        assert_eq!(second.n_items(), 2);
        let action = group.lookup_action("item-3").unwrap();
        assert!(!action.is_enabled());
        assert_eq!(action.state().and_then(|s| s.get::<bool>()), Some(true));
        assert!(
            group.lookup_action("item-6").is_some(),
            "submenu actions share the group"
        );
        assert!(group.lookup_action("item-4").is_none());
        let label: Option<String> = second
            .item_attribute_value(1, "label", None)
            .and_then(|v| v.get());
        assert_eq!(label.as_deref(), Some("More"));
        assert!(second.item_link(1, "submenu").is_some());
    }
}
