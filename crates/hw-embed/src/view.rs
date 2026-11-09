//! `EmbedView`: GTK widget showing one embedded client's toplevel.
//!
//! Layout: `EmbedView › GtkGraphicsOffload › GtkPicture`. dmabuf textures can
//! then be handed to Hyprland as a subsurface; shm textures render normally.

use gtk4::subclass::prelude::*;
use gtk4::{gdk, glib, prelude::*};

use crate::server::EmbedServer;

glib::wrapper! {
    pub struct EmbedView(ObjectSubclass<imp::EmbedView>)
        @extends gtk4::Widget,
        @implements gtk4::Accessible, gtk4::Buildable, gtk4::ConstraintTarget;
}

impl Default for EmbedView {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbedView {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn attach(&self, server: EmbedServer, id: u64) {
        self.imp().link.replace(Some((server, id)));
    }

    pub(crate) fn set_texture(&self, texture: Option<&gdk::Texture>) {
        self.imp().picture.set_paintable(texture);
    }

    /// The client queued `wl_surface.frame`; answer on the next frame clock
    /// tick. Nothing is scheduled while unmapped, which throttles the client.
    pub(crate) fn request_frame(&self) {
        let imp = self.imp();
        if !self.is_mapped() || imp.tick.borrow().is_some() {
            return;
        }
        let id = self.add_tick_callback(|view, clock| {
            let imp = view.imp();
            let time = (clock.frame_time() / 1000) as u32;
            if let Some((server, id)) = imp.link.borrow().as_ref() {
                server.frame_done(*id, time);
            }
            imp.tick.take();
            glib::ControlFlow::Break
        });
        imp.tick.replace(Some(id));
    }

    fn with_link(&self, f: impl FnOnce(&EmbedServer, u64)) {
        if let Some((server, id)) = self.imp().link.borrow().as_ref() {
            f(server, *id);
        }
    }

    fn setup_input(&self) {
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |c, x, y| {
                let time = c.current_event_time();
                view.with_link(|s, id| s.pointer_motion(id, x, y, time));
            }
        ));
        self.add_controller(motion);

        let click = gtk4::GestureClick::builder().button(0).build();
        let button = |pressed: bool| {
            move |g: &gtk4::GestureClick, _n: i32, _x: f64, _y: f64| {
                let Some(view) = g.widget().and_downcast::<EmbedView>() else {
                    return;
                };
                if pressed {
                    view.grab_focus();
                }
                // GDK buttons 1/2/3 → evdev BTN_LEFT/MIDDLE/RIGHT.
                let code = match g.current_button() {
                    1 => 0x110,
                    2 => 0x112,
                    3 => 0x111,
                    b => 0x110 + b,
                };
                let time = g.current_event_time();
                view.with_link(|s, id| s.pointer_button(id, code, pressed, time));
            }
        };
        click.connect_pressed(button(true));
        click.connect_released(button(false));
        self.add_controller(click);

        let key = gtk4::EventControllerKey::new();
        key.connect_key_pressed(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |c, _keyval, keycode, _mods| {
                let time = c.current_event_time();
                view.with_link(|s, id| s.key(id, keycode, true, time));
                glib::Propagation::Stop
            }
        ));
        key.connect_key_released(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |c, _keyval, keycode, _mods| {
                let time = c.current_event_time();
                view.with_link(|s, id| s.key(id, keycode, false, time));
            }
        ));
        self.add_controller(key);
    }
}

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    pub struct EmbedView {
        pub(super) picture: gtk4::Picture,
        pub(super) offload: RefCell<Option<gtk4::GraphicsOffload>>,
        pub(super) link: RefCell<Option<(EmbedServer, u64)>>,
        pub(super) tick: RefCell<Option<gtk4::TickCallbackId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EmbedView {
        const NAME: &'static str = "HwEmbedView";
        type Type = super::EmbedView;
        type ParentType = gtk4::Widget;
    }

    impl ObjectImpl for EmbedView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_focusable(true);
            obj.set_overflow(gtk4::Overflow::Hidden);
            self.picture.set_content_fit(gtk4::ContentFit::Fill);
            self.picture.set_can_shrink(true);
            let offload = gtk4::GraphicsOffload::new(Some(&self.picture));
            offload.set_parent(&*obj);
            self.offload.replace(Some(offload));
            obj.setup_input();
        }

        fn dispose(&self) {
            if let Some(offload) = self.offload.take() {
                offload.unparent();
            }
            if let Some((server, id)) = self.link.take() {
                server.detach(id);
            }
        }
    }

    impl WidgetImpl for EmbedView {
        fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            match self.offload.borrow().as_ref() {
                Some(child) => {
                    let (_, nat, _, _) = child.measure(orientation, for_size);
                    (0, nat, -1, -1)
                }
                None => (0, 0, -1, -1),
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            if let Some(child) = self.offload.borrow().as_ref() {
                child.allocate(width, height, baseline, None);
            }
            if let Some((server, id)) = self.link.borrow().as_ref() {
                server.resize(*id, width, height);
            }
        }

        fn map(&self) {
            self.parent_map();
            let obj = self.obj();
            if let Some((server, id)) = self.link.borrow().as_ref()
                && server.has_pending_frames(*id)
            {
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    obj,
                    move || obj.request_frame()
                ));
            }
        }

        fn unmap(&self) {
            if let Some(id) = self.tick.take() {
                id.remove();
            }
            self.parent_unmap();
        }
    }
}
