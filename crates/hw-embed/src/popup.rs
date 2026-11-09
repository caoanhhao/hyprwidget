//! `xdg_popup` (menus, combo boxes, tooltips) shown in a `GtkPopover`, so
//! it is not clipped by the widget: on Hyprland the popover is itself an
//! `xdg_popup` of hyprwidget's layer surface.

use gtk4::{gdk, glib, prelude::*};
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState};

use crate::server::{EmbedServer, State, ViewId};
use crate::surface::SurfaceView;

pub(crate) struct Popup {
    pub(crate) surface: PopupSurface,
    pub(crate) view: ViewId,
    sv: SurfaceView,
    popover: gtk4::Popover,
    parent: SurfaceView,
    geometry: Rectangle<i32, Logical>,
    grab: bool,
    mapped: bool,
}

const CSS: &str = "
popover.hw-embed-popup, popover.hw-embed-popup > contents {
  padding: 0; margin: 0; border: none; border-radius: 0;
  box-shadow: none; background: none; min-width: 0; min-height: 0;
}
popover.hw-embed-popup > arrow { min-width: 0; min-height: 0; }
";

fn ensure_css() {
    thread_local!(static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) });
    if DONE.with(|d| d.replace(true)) {
        return;
    }
    if let Some(display) = gdk::Display::default() {
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(CSS);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION + 10,
        );
    }
}

impl Popup {
    /// Points the popover at the popup's position inside the parent widget.
    fn place(&self) {
        let pg = self.parent.geometry();
        let (x, y) = self.parent.tree_to_widget(
            (pg.x + self.geometry.loc.x) as f64,
            (pg.y + self.geometry.loc.y) as f64,
        );
        // Bottom + halign start: the popover's top-left sits at the
        // rectangle's bottom-left corner.
        self.popover.set_pointing_to(Some(&gdk::Rectangle::new(
            x.round() as i32,
            y.round() as i32 - 1,
            1,
            1,
        )));
    }

    pub(crate) fn set_grab(&mut self) {
        self.grab = true;
        self.popover.set_autohide(true);
    }

    /// First buffer → show; buffer removed → hide (from an idle: popping up
    /// moves focus, which calls back into the server).
    pub(crate) fn set_mapped(&mut self, mapped: bool) {
        if self.mapped == mapped {
            return;
        }
        self.mapped = mapped;
        let (popover, sv, grab) = (self.popover.clone(), self.sv.clone(), self.grab);
        glib::idle_add_local_once(move || {
            if !mapped {
                popover.popdown();
                return;
            }
            popover.popup();
            if grab {
                sv.grab_focus();
            }
        });
    }
}

impl State {
    pub(crate) fn popup_new(&mut self, surface: PopupSurface, positioner: PositionerState) {
        let Some(server) = EmbedServer::current() else {
            return;
        };
        let (Some(view), Some(parent)) = (
            self.view_of(surface.wl_surface()),
            surface.get_parent_surface().and_then(|p| self.sv_of(&p)),
        ) else {
            tracing::debug!("xdg_popup without a known parent");
            return;
        };
        ensure_css();
        let geometry = positioner.get_geometry();
        surface.with_pending_state(|s| {
            s.geometry = geometry;
            s.positioner = positioner;
        });

        let sv = SurfaceView::new(&server, surface.wl_surface());
        let popover = gtk4::Popover::new();
        popover.add_css_class("hw-embed-popup");
        popover.set_has_arrow(false);
        popover.set_autohide(false);
        popover.set_cascade_popdown(true);
        popover.set_position(gtk4::PositionType::Bottom);
        popover.set_halign(gtk4::Align::Start);
        popover.set_child(Some(&sv));
        popover.set_parent(&parent);
        let wl = surface.wl_surface().clone();
        popover.connect_closed(move |_| {
            if let Some(s) = EmbedServer::current() {
                let wl = wl.clone();
                s.run(move |st| st.popup_dismissed(&wl));
            }
        });

        self.add_root(surface.wl_surface(), &sv);
        let popup = Popup {
            surface: surface.clone(),
            view,
            sv,
            popover,
            parent,
            geometry,
            grab: false,
            mapped: false,
        };
        popup.place();
        self.popups.insert(surface.wl_surface().id(), popup);
    }

    pub(crate) fn popup_reposition(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        let geometry = positioner.get_geometry();
        surface.with_pending_state(|s| {
            s.geometry = geometry;
            s.positioner = positioner;
        });
        surface.send_repositioned(token);
        let _ = surface.send_configure();
        if let Some(p) = self.popups.get_mut(&surface.wl_surface().id()) {
            p.geometry = geometry;
            p.place();
        }
    }

    /// Client destroyed the popup.
    pub(crate) fn popup_destroyed_(&mut self, surface: &PopupSurface) {
        let wl = surface.wl_surface();
        self.remove_root(wl);
        if let Some(p) = self.popups.remove(&wl.id()) {
            p.sv.clear();
            let popover = p.popover;
            glib::idle_add_local_once(move || {
                popover.popdown();
                if popover.parent().is_some() {
                    popover.unparent();
                }
            });
        }
    }

    /// The popover closed (click outside, Escape handled by GTK, parent
    /// closed): tell the client.
    fn popup_dismissed(&mut self, wl: &WlSurface) {
        if let Some(p) = self.popups.get_mut(&wl.id())
            && p.mapped
        {
            p.mapped = false;
            if p.surface.alive() {
                p.surface.send_popup_done();
            }
        }
    }
}
