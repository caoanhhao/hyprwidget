//! `SurfaceView`: GTK widget showing one client surface tree (an
//! `xdg_toplevel` or `xdg_popup` root plus its subsurfaces) and forwarding
//! GTK input to it.
//!
//! Every surface with a buffer gets its own `GtkGraphicsOffload ›
//! GtkPicture`, positioned at its subsurface offset, so dmabuf surfaces can
//! still be handed to Hyprland as subsurfaces.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, graphene, gsk, prelude::*};
use smithay::reexports::wayland_server::backend::ObjectId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

use crate::server::EmbedServer;

/// Rectangle in surface-tree coordinates (logical pixels of the client).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// One entry of the paint order (bottom to top).
pub(crate) struct Placed {
    pub id: ObjectId,
    pub loc: (i32, i32),
    pub size: (i32, i32),
}

/// What a client's enabled `zwp_text_input_v3` currently says.
#[derive(Debug, Clone)]
pub(crate) struct TextInputInfo {
    /// Cursor rectangle in tree coordinates.
    pub rect: Option<Rect>,
    /// Surrounding text, cursor and anchor (byte offsets).
    pub surrounding: Option<(String, i32, i32)>,
    pub purpose: gtk4::InputPurpose,
    pub hints: gtk4::InputHints,
}

impl Default for TextInputInfo {
    fn default() -> Self {
        Self {
            rect: None,
            surrounding: None,
            purpose: gtk4::InputPurpose::FreeForm,
            hints: gtk4::InputHints::NONE,
        }
    }
}

/// IM output waiting for the next `done`.
#[derive(Debug, Default)]
pub(crate) struct ImBatch {
    /// Bytes before / after the cursor.
    pub delete: Option<(u32, u32)>,
    pub commit: Option<String>,
    /// Current preedit text and cursor (byte offsets).
    pub preedit: (String, i32, i32),
}

/// evdev button code for a GDK button number.
fn evdev_button(b: u32) -> u32 {
    match b {
        1 => 0x110, // BTN_LEFT
        2 => 0x112, // BTN_MIDDLE
        3 => 0x111, // BTN_RIGHT
        8 => 0x113, // BTN_SIDE
        9 => 0x114, // BTN_EXTRA
        b => 0x110 + b,
    }
}

glib::wrapper! {
    pub(crate) struct SurfaceView(ObjectSubclass<imp::SurfaceView>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl SurfaceView {
    pub(crate) fn new(server: &EmbedServer, root: &WlSurface) -> Self {
        let sv: Self = glib::Object::new();
        let imp = sv.imp();
        imp.server.replace(Some(server.clone()));
        imp.root.replace(Some(root.clone()));
        sv
    }

    /// `scale` mode: the tree keeps its own size and is scaled to fit.
    pub(crate) fn set_scale_mode(&self, on: bool) {
        self.imp().scale_mode.set(on);
        self.queue_allocate();
    }

    /// Dialogs (secondary toplevels) are centred instead of filling.
    pub(crate) fn set_centered(&self, on: bool) {
        self.imp().centered.set(on);
        self.queue_allocate();
    }

    pub(crate) fn has_content(&self) -> bool {
        !self.imp().order.borrow().is_empty()
    }

    pub(crate) fn geometry(&self) -> Rect {
        self.imp().geometry.get()
    }

    pub(crate) fn texture_of(&self, id: &ObjectId) -> Option<gdk::Texture> {
        self.imp().kids.borrow().get(id).and_then(|k| k.texture.clone())
    }

    pub(crate) fn set_texture(&self, id: &ObjectId, texture: Option<&gdk::Texture>) {
        let imp = self.imp();
        let mut kids = imp.kids.borrow_mut();
        let kid = kids.entry(id.clone()).or_insert_with(|| {
            let picture = gtk4::Picture::new();
            picture.set_content_fit(gtk4::ContentFit::Fill);
            picture.set_can_shrink(true);
            let offload = gtk4::GraphicsOffload::new(Some(&picture));
            offload.set_parent(self);
            Kid {
                offload,
                picture,
                texture: None,
            }
        });
        kid.picture.set_paintable(texture);
        kid.texture = texture.cloned();
    }

    /// Paint order and window geometry after a commit.
    pub(crate) fn set_layout(&self, order: Vec<Placed>, geometry: Rect) {
        let imp = self.imp();
        {
            let mut kids = imp.kids.borrow_mut();
            let keep: HashSet<&ObjectId> = order.iter().map(|p| &p.id).collect();
            kids.retain(|id, kid| {
                let alive = keep.contains(id);
                if !alive {
                    kid.offload.unparent();
                }
                alive
            });
            for p in &order {
                if let Some(kid) = kids.get(&p.id) {
                    kid.offload.insert_before(self, None::<&gtk4::Widget>);
                }
            }
        }
        let resized = imp.geometry.replace(geometry) != geometry;
        imp.order.replace(order);
        if resized {
            self.queue_resize();
        } else {
            self.queue_allocate();
        }
    }

    /// Forgets everything (surface destroyed).
    pub(crate) fn clear(&self) {
        let imp = self.imp();
        for (_, kid) in imp.kids.borrow_mut().drain() {
            kid.offload.unparent();
        }
        imp.order.borrow_mut().clear();
        imp.root.replace(None);
        self.set_text_input(None);
        self.queue_resize();
    }

    /// `(scale, offset)`: widget = (tree - geometry.loc) * scale + offset.
    fn mapping(&self) -> (f64, (f64, f64)) {
        let imp = self.imp();
        let g = imp.geometry.get();
        let (w, h) = (self.width() as f64, self.height() as f64);
        let (gw, gh) = (g.w.max(1) as f64, g.h.max(1) as f64);
        let s = if imp.scale_mode.get() && g.w > 0 && g.h > 0 {
            (w / gw).min(h / gh)
        } else {
            1.0
        };
        let off = if imp.scale_mode.get() || imp.centered.get() {
            (((w - gw * s) / 2.0).max(0.0), ((h - gh * s) / 2.0).max(0.0))
        } else {
            (0.0, 0.0)
        };
        (s, off)
    }

    pub(crate) fn widget_to_tree(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, (ox, oy)) = self.mapping();
        let g = self.imp().geometry.get();
        ((x - ox) / s + g.x as f64, (y - oy) / s + g.y as f64)
    }

    pub(crate) fn tree_to_widget(&self, x: f64, y: f64) -> (f64, f64) {
        let (s, (ox, oy)) = self.mapping();
        let g = self.imp().geometry.get();
        ((x - g.x as f64) * s + ox, (y - g.y as f64) * s + oy)
    }

    fn with_root(&self, f: impl FnOnce(&EmbedServer, WlSurface)) {
        let imp = self.imp();
        if let (Some(server), Some(root)) = (imp.server.borrow().as_ref(), imp.root.borrow().clone())
        {
            f(server, root);
        }
    }

    /// The client enabled (`Some`) or disabled (`None`) text input on this
    /// tree while it has keyboard focus.
    pub(crate) fn set_text_input(&self, info: Option<TextInputInfo>) {
        let imp = self.imp();
        let Some(info) = info else {
            if imp.ti_enabled.replace(false) {
                imp.im.focus_out();
                imp.im.reset();
                imp.key.set_im_context(None::<&gtk4::IMContext>);
                imp.batch.borrow_mut().preedit = Default::default();
            }
            return;
        };
        if let Some(r) = info.rect {
            let (x, y) = self.tree_to_widget(r.x as f64, r.y as f64);
            let (s, _) = self.mapping();
            imp.im.set_cursor_location(&gdk::Rectangle::new(
                x as i32,
                y as i32,
                ((r.w as f64 * s) as i32).max(1),
                ((r.h as f64 * s) as i32).max(1),
            ));
        }
        imp.im.set_input_purpose(info.purpose);
        imp.im.set_input_hints(info.hints);
        imp.surrounding.replace(info.surrounding);
        if !imp.ti_enabled.replace(true) {
            imp.key.set_im_context(Some(&imp.im));
            if self.has_focus() {
                imp.im.focus_in();
            }
        }
    }

    /// Queues IM output and sends it with one `done` from an idle.
    fn queue_im(&self, f: impl FnOnce(&mut ImBatch)) {
        let imp = self.imp();
        f(&mut imp.batch.borrow_mut());
        if !imp.flush_queued.replace(true) {
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = sv)]
                self,
                move || sv.flush_im()
            ));
        }
    }

    fn flush_im(&self) {
        let imp = self.imp();
        imp.flush_queued.set(false);
        let batch = {
            let mut b = imp.batch.borrow_mut();
            ImBatch {
                delete: b.delete.take(),
                commit: b.commit.take(),
                preedit: b.preedit.clone(),
            }
        };
        if !imp.ti_enabled.get() {
            return;
        }
        self.with_root(|s, root| s.text_input_send(root, batch));
    }

    fn setup_im(&self) {
        let im = &self.imp().im;
        im.set_client_widget(Some(self));
        im.connect_commit(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |_, text| {
                sv.queue_im(|b| match &mut b.commit {
                    Some(c) => c.push_str(text),
                    None => b.commit = Some(text.to_owned()),
                });
            }
        ));
        im.connect_preedit_changed(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |im| {
                let (text, attrs, cursor) = im.preedit_string();
                let _ = attrs;
                let byte = text
                    .char_indices()
                    .nth(cursor.max(0) as usize)
                    .map(|(i, _)| i)
                    .unwrap_or(text.len()) as i32;
                sv.queue_im(|b| b.preedit = (text.to_string(), byte, byte));
            }
        ));
        im.connect_retrieve_surrounding(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            #[upgrade_or]
            false,
            move |im| match sv.imp().surrounding.borrow().as_ref() {
                Some((text, cursor, anchor)) => {
                    im.set_surrounding_with_selection(text, *cursor, *anchor);
                    true
                }
                None => false,
            }
        ));
        im.connect_delete_surrounding(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            #[upgrade_or]
            false,
            move |_, offset, n_chars| {
                let Some(range) = sv
                    .imp()
                    .surrounding
                    .borrow()
                    .as_ref()
                    .and_then(|(t, c, _)| delete_range(t, *c, offset, n_chars))
                else {
                    return false;
                };
                // A delete must not be reordered before an earlier commit.
                if sv.imp().batch.borrow().commit.is_some() {
                    sv.flush_im();
                }
                sv.queue_im(|b| b.delete = Some(range));
                true
            }
        ));
    }

    fn setup_input(&self) {
        let motion = gtk4::EventControllerMotion::new();
        let moved = glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |c: &gtk4::EventControllerMotion, x: f64, y: f64| {
                let time = c.current_event_time();
                let (tx, ty) = sv.widget_to_tree(x, y);
                sv.with_root(|s, root| s.pointer_motion(root, tx, ty, time));
            }
        );
        motion.connect_enter(moved.clone());
        motion.connect_motion(moved);
        motion.connect_leave(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |c| {
                let time = c.current_event_time();
                sv.with_root(|s, root| s.pointer_leave(root, time));
            }
        ));
        self.add_controller(motion);

        // Buttons and touch: raw events (no gesture state machine).
        let legacy = gtk4::EventControllerLegacy::new();
        legacy.connect_event(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, ev| sv.legacy_event(ev)
        ));
        self.add_controller(legacy);

        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |c, dx, dy| {
                let time = c.current_event_time();
                let wheel = c.unit() == gdk::ScrollUnit::Wheel;
                sv.with_root(|s, root| s.pointer_axis(root, dx, dy, wheel, false, time));
                glib::Propagation::Stop
            }
        ));
        scroll.connect_scroll_end(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |c| {
                let time = c.current_event_time();
                sv.with_root(|s, root| s.pointer_axis(root, 0.0, 0.0, false, true, time));
            }
        ));
        self.add_controller(scroll);

        let key = &self.imp().key;
        key.connect_key_pressed(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |c, _keyval, keycode, _mods| {
                // GDK repeats held keys itself; the client repeats on its own.
                if sv.imp().pressed.borrow_mut().insert(keycode) {
                    let time = c.current_event_time();
                    sv.with_root(|s, root| s.key(root, keycode, true, time));
                }
                glib::Propagation::Stop
            }
        ));
        key.connect_key_released(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |c, _keyval, keycode, _mods| {
                if sv.imp().pressed.borrow_mut().remove(&keycode) {
                    let time = c.current_event_time();
                    sv.with_root(|s, root| s.key(root, keycode, false, time));
                }
            }
        ));
        self.add_controller(key.clone());

        let focus = gtk4::EventControllerFocus::new();
        focus.connect_enter(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |_| {
                if sv.imp().ti_enabled.get() {
                    sv.imp().im.focus_in();
                }
                sv.with_root(|s, root| s.keyboard_enter(root));
            }
        ));
        focus.connect_leave(glib::clone!(
            #[weak(rename_to = sv)]
            self,
            move |_| sv.release_keyboard()
        ));
        self.add_controller(focus);
    }

    /// Releases held keys and drops keyboard focus (GTK focus left).
    fn release_keyboard(&self) {
        let imp = self.imp();
        let keys: Vec<u32> = imp.pressed.borrow_mut().drain().collect();
        if imp.ti_enabled.get() {
            imp.im.focus_out();
        }
        self.with_root(|s, root| {
            for k in keys {
                s.key(root.clone(), k, false, 0);
            }
            s.keyboard_leave(root);
        });
    }

    fn legacy_event(&self, ev: &gdk::Event) -> glib::Propagation {
        use gdk::EventType as T;
        let time = ev.time();
        match ev.event_type() {
            T::ButtonPress | T::ButtonRelease => {
                let Some(b) = ev.downcast_ref::<gdk::ButtonEvent>() else {
                    return glib::Propagation::Proceed;
                };
                let pressed = ev.event_type() == T::ButtonPress;
                if pressed && self.is_focusable() {
                    self.grab_focus();
                }
                let code = evdev_button(b.button());
                self.with_root(|s, root| s.pointer_button(root, code, pressed, time));
                glib::Propagation::Stop
            }
            T::TouchBegin | T::TouchUpdate | T::TouchEnd | T::TouchCancel => {
                let seq = ev.event_sequence().as_ptr() as usize;
                let imp = self.imp();
                let slot = {
                    let mut slots = imp.touch_slots.borrow_mut();
                    let next = slots.len() as i32;
                    *slots.entry(seq).or_insert(next)
                };
                let pos = ev.position().and_then(|(x, y)| self.surface_to_widget(x, y));
                match ev.event_type() {
                    T::TouchBegin | T::TouchUpdate => {
                        if ev.event_type() == T::TouchBegin && self.is_focusable() {
                            self.grab_focus();
                        }
                        let Some((x, y)) = pos else {
                            return glib::Propagation::Proceed;
                        };
                        let (tx, ty) = self.widget_to_tree(x, y);
                        let down = ev.event_type() == T::TouchBegin;
                        self.with_root(|s, root| s.touch(root, slot, Some((tx, ty)), down, time));
                    }
                    T::TouchEnd => {
                        imp.touch_slots.borrow_mut().remove(&seq);
                        self.with_root(|s, root| s.touch(root, slot, None, false, time));
                    }
                    _ => {
                        imp.touch_slots.borrow_mut().clear();
                        self.with_root(|s, _| s.touch_cancel());
                    }
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    /// Native-surface coordinates (legacy events) → widget coordinates.
    fn surface_to_widget(&self, x: f64, y: f64) -> Option<(f64, f64)> {
        let native = self.native()?;
        let (sx, sy) = native.surface_transform();
        let p = native.upcast_ref::<gtk4::Widget>().compute_point(
            self,
            &graphene::Point::new((x - sx) as f32, (y - sy) as f32),
        )?;
        Some((p.x() as f64, p.y() as f64))
    }
}

/// `GtkIMContext::delete-surrounding` (chars around the cursor) →
/// `delete_surrounding_text` (bytes before / after the cursor).
fn delete_range(text: &str, cursor: i32, offset: i32, n_chars: i32) -> Option<(u32, u32)> {
    let cursor = usize::try_from(cursor).ok().filter(|c| *c <= text.len())?;
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let cursor_char = text[..cursor].chars().count() as i64;
    let start = (cursor_char + offset as i64).max(0) as usize;
    let end = start + n_chars.max(0) as usize;
    let byte = |c: usize| {
        text.char_indices()
            .nth(c)
            .map(|(i, _)| i)
            .unwrap_or(text.len())
    };
    let (start, end) = (byte(start), byte(end));
    let before = cursor.saturating_sub(start.min(cursor));
    let after = end.max(cursor) - cursor;
    Some((before as u32, after as u32))
}

struct Kid {
    offload: gtk4::GraphicsOffload,
    picture: gtk4::Picture,
    texture: Option<gdk::Texture>,
}

mod imp {
    use super::*;

    pub(crate) struct SurfaceView {
        pub(super) server: RefCell<Option<EmbedServer>>,
        pub(super) root: RefCell<Option<WlSurface>>,
        pub(super) kids: RefCell<HashMap<ObjectId, Kid>>,
        pub(super) order: RefCell<Vec<Placed>>,
        pub(super) geometry: Cell<Rect>,
        pub(super) scale_mode: Cell<bool>,
        pub(super) centered: Cell<bool>,
        pub(super) im: gtk4::IMMulticontext,
        pub(super) key: gtk4::EventControllerKey,
        pub(super) ti_enabled: Cell<bool>,
        pub(super) surrounding: RefCell<Option<(String, i32, i32)>>,
        pub(super) batch: RefCell<ImBatch>,
        pub(super) flush_queued: Cell<bool>,
        pub(super) pressed: RefCell<HashSet<u32>>,
        pub(super) touch_slots: RefCell<HashMap<usize, i32>>,
    }

    impl Default for SurfaceView {
        fn default() -> Self {
            Self {
                server: RefCell::default(),
                root: RefCell::default(),
                kids: RefCell::default(),
                order: RefCell::default(),
                geometry: Cell::default(),
                scale_mode: Cell::default(),
                centered: Cell::default(),
                im: gtk4::IMMulticontext::new(),
                key: gtk4::EventControllerKey::new(),
                ti_enabled: Cell::default(),
                surrounding: RefCell::default(),
                batch: RefCell::default(),
                flush_queued: Cell::default(),
                pressed: RefCell::default(),
                touch_slots: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SurfaceView {
        const NAME: &'static str = "HwEmbedSurface";
        type Type = super::SurfaceView;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for SurfaceView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk4::Overflow::Hidden);
            obj.add_css_class("hw-embed-surface");
            obj.setup_input();
            obj.setup_im();
        }

        fn dispose(&self) {
            for (_, kid) in self.kids.borrow_mut().drain() {
                kid.offload.unparent();
            }
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for SurfaceView {
        fn measure(&self, orientation: gtk4::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let g = self.geometry.get();
            let nat = match orientation {
                gtk4::Orientation::Horizontal => g.w,
                _ => g.h,
            };
            (0, nat.max(0), -1, -1)
        }

        fn size_allocate(&self, _width: i32, _height: i32, _baseline: i32) {
            let obj = self.obj();
            let (s, (ox, oy)) = obj.mapping();
            let g = self.geometry.get();
            let kids = self.kids.borrow();
            for p in self.order.borrow().iter() {
                let Some(kid) = kids.get(&p.id) else {
                    continue;
                };
                let x = (p.loc.0 - g.x) as f64 * s + ox;
                let y = (p.loc.1 - g.y) as f64 * s + oy;
                let mut t = gsk::Transform::new().translate(&graphene::Point::new(x as f32, y as f32));
                if (s - 1.0).abs() > f64::EPSILON {
                    t = t.scale(s as f32, s as f32);
                }
                kid.offload.allocate(p.size.0.max(0), p.size.1.max(0), -1, Some(t));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::delete_range;

    #[test]
    fn delete_range_ascii() {
        // "hello|" delete 2 chars before.
        assert_eq!(delete_range("hello", 5, -2, 2), Some((2, 0)));
        // "he|llo" delete 1 after.
        assert_eq!(delete_range("hello", 2, 0, 1), Some((0, 1)));
    }

    #[test]
    fn delete_range_vietnamese() {
        // "việt|": 'ệ' is 3 bytes; delete the last 2 chars ("ệt").
        let t = "việt";
        assert_eq!(delete_range(t, t.len() as i32, -2, 2), Some((4, 0)));
        assert_eq!(delete_range(t, 3, -1, 1), None); // not a char boundary
    }
}
