//! In-process Wayland server (smithay), dispatched from the GLib main loop.
//!
//! Every app session (one [`EmbedView`] run) has a [`ViewId`]; its clients
//! carry it in their [`ClientState`], so the server knows which widget a
//! surface belongs to without any socket file. Each `xdg_toplevel` and
//! `xdg_popup` root is shown by its own [`SurfaceView`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gtk4::{gdk, glib, prelude::*};
use smithay::backend::allocator::{Format, Fourcc, Modifier};
use smithay::backend::drm::DrmDeviceFd;
use smithay::input::keyboard::{KeyboardHandle, XkbConfig};
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData, PointerHandle};
use smithay::input::touch::TouchHandle;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration::{
    Mode as KdeMode, OrgKdeKwinServerDecoration,
};
use smithay::reexports::wayland_protocols_misc::server_decoration::server::org_kde_kwin_server_decoration_manager::Mode as KdeDefaultMode;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason, ObjectId};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_callback::WlCallback;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{DeviceFd, Logical, Point, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    self, BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, Damage,
    SubsurfaceCachedState, SurfaceAttributes, SurfaceData, TraversalAction, with_states,
    with_surface_tree_downward, with_surface_tree_upward,
};
use smithay::wayland::cursor_shape::CursorShapeManagerState;
use smithay::wayland::dmabuf::{
    DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier,
};
use smithay::wayland::drm_syncobj::{
    DrmSyncPoint, DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState,
    supports_syncobj_eventfd,
};
use smithay::wayland::fractional_scale::{
    FractionalScaleHandler, FractionalScaleManagerState, with_fractional_scale,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
    set_data_device_focus,
};
use smithay::wayland::selection::primary_selection::{
    PrimarySelectionHandler, PrimarySelectionState, set_primary_focus,
};
use smithay::wayland::shell::kde::decoration::{KdeDecorationHandler, KdeDecorationState};
use smithay::wayland::shell::xdg::decoration::{XdgDecorationHandler, XdgDecorationState};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, SurfaceCachedState, ToplevelSurface, XdgShellHandler,
    XdgShellState, XdgToplevelSurfaceData,
};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::wayland::single_pixel_buffer::SinglePixelBufferState;
use smithay::wayland::viewporter::{ViewportCachedState, ViewporterState};
use smithay::{
    delegate_compositor, delegate_cursor_shape, delegate_data_device, delegate_dmabuf,
    delegate_drm_syncobj, delegate_fractional_scale, delegate_kde_decoration, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_single_pixel_buffer,
    delegate_viewporter, delegate_xdg_decoration, delegate_xdg_shell,
};

use crate::buffer;
use crate::clipboard::Clipboards;
use crate::input::KeyboardConfig;
use crate::popup::Popup;
use crate::surface::{Placed, Rect, SurfaceView};
use crate::text_input::TextInputState;
use crate::view::EmbedView;

/// One app session (one spawn of an [`EmbedView`]'s command).
pub(crate) type ViewId = u64;

/// Handle to the nested compositor. Cheap to clone; lives on the GTK thread.
#[derive(Clone)]
pub struct EmbedServer(Rc<Inner>);

struct Inner {
    display: RefCell<Display<State>>,
    state: RefCell<State>,
    next_view: Cell<ViewId>,
}

thread_local! {
    /// Lets `Send` callbacks (texture release, client disconnect) reach the server.
    static SERVER: RefCell<Weak<Inner>> = const { RefCell::new(Weak::new()) };
    /// The server `W.App` widgets share, created on first use.
    static SHARED: RefCell<Option<EmbedServer>> = const { RefCell::new(None) };
}

impl EmbedServer {
    /// Creates a server and attaches its fd to the default GLib main context.
    /// Must be called after GTK is initialised (dmabuf formats come from GDK).
    pub fn new() -> anyhow::Result<Self> {
        let display: Display<State> = Display::new()?;
        let state = State::new(display.handle())?;
        let inner = Rc::new(Inner {
            display: RefCell::new(display),
            state: RefCell::new(state),
            next_view: Cell::new(1),
        });
        SERVER.with(|s| *s.borrow_mut() = Rc::downgrade(&inner));

        let fd = inner.display.borrow_mut().backend().poll_fd().as_raw_fd();
        let weak = Rc::downgrade(&inner);
        glib_unix::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
            let Some(inner) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            inner.dispatch();
            glib::ControlFlow::Continue
        });
        let server = Self(inner);
        server.run(|st| st.clipboards.sync_all(&st.dh, &st.seat));
        Ok(server)
    }

    /// The process-wide server, created on first use.
    pub fn shared() -> anyhow::Result<Self> {
        if let Some(s) = SHARED.with(|s| s.borrow().clone()) {
            return Ok(s);
        }
        let s = Self::new()?;
        SHARED.with(|slot| *slot.borrow_mut() = Some(s.clone()));
        Ok(s)
    }

    pub(crate) fn current() -> Option<Self> {
        SERVER.with(|s| s.borrow().upgrade()).map(Self)
    }

    /// Runs `f` on the state now, or from an idle if the state is busy
    /// (a GTK callback fired while the server was dispatching).
    pub(crate) fn run(&self, f: impl FnOnce(&mut State) + 'static) {
        match self.0.state.try_borrow_mut() {
            Ok(mut st) => {
                f(&mut st);
                drop(st);
                self.flush();
            }
            Err(_) => {
                let s = self.clone();
                glib::idle_add_local_once(move || s.run(f));
            }
        }
    }

    /// Runs `f` on the state if it is not busy.
    pub(crate) fn with_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> Option<R> {
        let mut st = self.0.state.try_borrow_mut().ok()?;
        let r = f(&mut st);
        drop(st);
        self.flush();
        Some(r)
    }

    pub(crate) fn flush(&self) {
        if let Ok(mut display) = self.0.display.try_borrow_mut() {
            let _ = display.flush_clients();
        }
    }

    /// Starts a session shown in `view`.
    pub(crate) fn open_session(&self, view: &EmbedView) -> ViewId {
        let id = self.0.next_view.get();
        self.0.next_view.set(id + 1);
        let slot = ViewSlot {
            view: view.downgrade(),
            toplevels: Vec::new(),
            frame_callbacks: Vec::new(),
            scale: view.scale(),
            clients: Arc::new(AtomicUsize::new(0)),
        };
        self.0.state.borrow_mut().views.insert(id, slot);
        id
    }

    /// Number of live client connections of a session.
    pub(crate) fn client_counter(&self, id: ViewId) -> Arc<AtomicUsize> {
        self.0
            .state
            .borrow()
            .views
            .get(&id)
            .map(|v| v.clients.clone())
            .unwrap_or_default()
    }

    /// Adds a client connection to session `id`.
    pub(crate) fn connect(&self, id: ViewId, stream: UnixStream) -> anyhow::Result<()> {
        let clients = self.client_counter(id);
        clients.fetch_add(1, Ordering::SeqCst);
        let r = self.0.display.borrow_mut().handle().insert_client(
            stream,
            Arc::new(ClientState {
                view: id,
                compositor: CompositorClientState::default(),
                clients: clients.clone(),
            }),
        );
        if r.is_err() {
            clients.fetch_sub(1, Ordering::SeqCst);
        }
        r?;
        Ok(())
    }

    /// Asks the session's toplevels to close. Returns whether there were any.
    pub(crate) fn close_session(&self, id: ViewId) -> bool {
        self.with_state(|st| {
            let tops = st.views.get(&id).map(|v| v.toplevels.clone()).unwrap_or_default();
            for (t, _) in &tops {
                t.send_close();
            }
            !tops.is_empty()
        })
        .unwrap_or(false)
    }

    /// Forgets a session (its process is gone): drops its views and kicks
    /// any client still connected.
    pub(crate) fn drop_session(&self, id: ViewId) {
        self.run(move |st| {
            if let Some(slot) = st.views.remove(&id) {
                for (t, sv) in slot.toplevels {
                    st.roots.remove(&t.wl_surface().id());
                    sv.clear();
                }
            }
            st.popups.retain(|_, p| p.view != id);
            let clients: Vec<ClientId> = st
                .clients
                .iter()
                .filter(|(_, v)| **v == id)
                .map(|(c, _)| c.clone())
                .collect();
            for c in clients {
                st.dh.backend_handle().kill_client(c, DisconnectReason::ConnectionClosed);
            }
        });
    }

    /// New widget size: `Some((w, h))` = fill, `None` = let the app choose.
    pub(crate) fn configure(&self, id: ViewId, size: Option<(i32, i32)>) {
        self.run(move |st| {
            if let Some(slot) = st.views.get(&id) {
                for (t, sv) in &slot.toplevels {
                    if t.parent().is_none() {
                        configure_toplevel(t, size, true);
                    } else {
                        let _ = sv;
                    }
                }
            }
        });
    }

    /// Sends `wl_surface.frame` done for everything the session queued.
    pub(crate) fn frame_done(&self, id: ViewId, time_ms: u32) {
        self.run(move |st| {
            if let Some(slot) = st.views.get_mut(&id) {
                for cb in slot.frame_callbacks.drain(..) {
                    cb.done(time_ms);
                }
            }
        });
    }

    pub(crate) fn has_pending_frames(&self, id: ViewId) -> bool {
        self.0
            .state
            .try_borrow()
            .ok()
            .and_then(|s| s.views.get(&id).map(|v| !v.frame_callbacks.is_empty()))
            .unwrap_or(false)
    }

    /// The widget's (fractional) scale changed.
    pub(crate) fn set_scale(&self, id: ViewId, scale: f64) {
        self.run(move |st| {
            let Some(slot) = st.views.get_mut(&id) else {
                return;
            };
            slot.scale = scale;
            let roots: Vec<WlSurface> = st
                .roots
                .keys()
                .filter_map(|k| st.surface_by_id(k))
                .filter(|s| st.view_of(s) == Some(id))
                .collect();
            for root in roots {
                with_surface_tree_downward(
                    &root,
                    (),
                    |_, _, _| TraversalAction::DoChildren(()),
                    |_, states, _| {
                        with_fractional_scale(states, |fs| fs.set_preferred_scale(scale))
                    },
                    |_, _, _| true,
                );
            }
            st.output.change_current_state(
                None,
                None,
                Some(Scale::Fractional(scale)),
                None,
            );
        });
    }
}

impl Inner {
    fn dispatch(&self) {
        let (Ok(mut display), Ok(mut state)) =
            (self.display.try_borrow_mut(), self.state.try_borrow_mut())
        else {
            return;
        };
        if let Err(e) = display.dispatch_clients(&mut state) {
            tracing::warn!("wayland dispatch: {e}");
        }
        let _ = display.flush_clients();
    }
}

/// Flushes the server from a `Send` context (texture release functions).
pub(crate) fn flush_later() {
    glib::idle_add_once(|| {
        if let Some(s) = EmbedServer::current() {
            s.flush();
        }
    });
}

/// `size = None` lets the client pick its own size (dialogs, `scale` mode).
fn configure_toplevel(toplevel: &ToplevelSurface, size: Option<(i32, i32)>, fill: bool) {
    toplevel.with_pending_state(|s| {
        s.size = size
            .filter(|(w, h)| *w > 0 && *h > 0)
            .map(|(w, h)| (w, h).into());
        s.states.set(xdg_toplevel::State::Activated);
        for st in [
            xdg_toplevel::State::Maximized,
            xdg_toplevel::State::TiledLeft,
            xdg_toplevel::State::TiledRight,
            xdg_toplevel::State::TiledTop,
            xdg_toplevel::State::TiledBottom,
        ] {
            if fill {
                s.states.set(st);
            } else {
                s.states.unset(st);
            }
        }
        s.decoration_mode = Some(DecorationMode::ServerSide);
    });
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}

pub(crate) struct ClientState {
    pub(crate) view: ViewId,
    compositor: CompositorClientState,
    clients: Arc<AtomicUsize>,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        tracing::debug!(view = self.view, ?reason, "embedded client disconnected");
        let left = self.clients.fetch_sub(1, Ordering::SeqCst).saturating_sub(1);
        let id = self.view;
        glib::idle_add_once(move || {
            if let Some(s) = EmbedServer::current() {
                s.client_gone(id, left);
            }
        });
    }
}

impl EmbedServer {
    fn client_gone(&self, id: ViewId, left: usize) {
        let view = self
            .0
            .state
            .try_borrow()
            .ok()
            .and_then(|st| st.views.get(&id).and_then(|v| v.view.upgrade()));
        if let Some(view) = view {
            view.clients_changed(id, left);
        }
    }
}

pub(crate) struct ViewSlot {
    pub(crate) view: glib::WeakRef<EmbedView>,
    /// Toplevels in creation order, each with its widget.
    pub(crate) toplevels: Vec<(ToplevelSurface, SurfaceView)>,
    pub(crate) frame_callbacks: Vec<WlCallback>,
    pub(crate) scale: f64,
    pub(crate) clients: Arc<AtomicUsize>,
}

/// Per-surface data kept in smithay's `data_map`.
#[derive(Default)]
pub(crate) struct SurfaceInfo {
    /// Logical size while a buffer is attached.
    pub(crate) size: Option<(i32, i32)>,
}

pub(crate) fn surface_info<R>(states: &SurfaceData, f: impl FnOnce(&mut SurfaceInfo) -> R) -> R {
    states
        .data_map
        .insert_if_missing(|| RefCell::new(SurfaceInfo::default()));
    f(&mut states.data_map.get::<RefCell<SurfaceInfo>>().unwrap().borrow_mut())
}

fn sub_loc(states: &SurfaceData) -> Point<i32, Logical> {
    states
        .cached_state
        .get::<SubsurfaceCachedState>()
        .current()
        .location
}

pub(crate) fn root_of(surface: &WlSurface) -> WlSurface {
    let mut s = surface.clone();
    while let Some(p) = compositor::get_parent(&s) {
        s = p;
    }
    s
}

/// Location of `surface` inside its tree (sum of subsurface offsets).
pub(crate) fn tree_offset(surface: &WlSurface) -> (i32, i32) {
    let mut s = surface.clone();
    let mut loc = Point::<i32, Logical>::from((0, 0));
    while let Some(p) = compositor::get_parent(&s) {
        loc += with_states(&s, sub_loc);
        s = p;
    }
    (loc.x, loc.y)
}

/// Topmost surface of `root`'s tree under `p` (tree coordinates), with
/// its origin.
pub(crate) fn surface_under(root: &WlSurface, p: (f64, f64)) -> Option<(WlSurface, (i32, i32))> {
    let found = RefCell::new(None);
    with_surface_tree_downward(
        root,
        Point::<i32, Logical>::from((0, 0)),
        |_, states, loc| TraversalAction::DoChildren(*loc + sub_loc(states)),
        |s, states, loc| {
            if found.borrow().is_some() {
                return;
            }
            let loc = *loc + sub_loc(states);
            let Some((w, h)) = surface_info(states, |i| i.size) else {
                return;
            };
            let (lx, ly) = (p.0 - loc.x as f64, p.1 - loc.y as f64);
            if lx < 0.0 || ly < 0.0 || lx >= w as f64 || ly >= h as f64 {
                return;
            }
            let inside = states
                .cached_state
                .get::<SurfaceAttributes>()
                .current()
                .input_region
                .as_ref()
                .is_none_or(|r| r.contains((lx as i32, ly as i32)));
            if inside {
                *found.borrow_mut() = Some((s.clone(), (loc.x, loc.y)));
            }
        },
        |_, _, _| found.borrow().is_none(),
    );
    found.into_inner()
}

pub(crate) struct State {
    pub(crate) dh: DisplayHandle,
    gdk_display: gdk::Display,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    _xdg_decoration: XdgDecorationState,
    kde_decoration: KdeDecorationState,
    shm: ShmState,
    _viewporter: ViewporterState,
    _single_pixel: SinglePixelBufferState,
    _fractional: FractionalScaleManagerState,
    _cursor_shape: CursorShapeManagerState,
    data_device: DataDeviceState,
    primary: PrimarySelectionState,
    seat_state: SeatState<State>,
    pub(crate) seat: Seat<State>,
    pub(crate) pointer: PointerHandle<State>,
    pub(crate) keyboard: KeyboardHandle<State>,
    pub(crate) touch: TouchHandle<State>,
    output: Output,
    dmabuf: DmabufState,
    _dmabuf_global: Option<DmabufGlobal>,
    syncobj: Option<DrmSyncobjState>,
    pub(crate) text_input: TextInputState,
    pub(crate) clipboards: Clipboards,
    pub(crate) views: HashMap<ViewId, ViewSlot>,
    /// Client → session (to kick clients when a session ends).
    clients: HashMap<ClientId, ViewId>,
    /// Root surface (toplevel or popup) → the widget showing it.
    pub(crate) roots: HashMap<ObjectId, glib::WeakRef<SurfaceView>>,
    root_surfaces: HashMap<ObjectId, WlSurface>,
    pub(crate) popups: HashMap<ObjectId, Popup>,
    /// Root whose widget the pointer is over (receives cursor changes).
    pub(crate) pointer_root: Option<WlSurface>,
    cursor_surface: Option<WlSurface>,
    /// Surface with keyboard focus, for `focus_changed`.
    pub(crate) kb_focus: Option<WlSurface>,
}

impl State {
    fn new(dh: DisplayHandle) -> anyhow::Result<Self> {
        let gdk_display =
            gdk::Display::default().ok_or_else(|| anyhow::anyhow!("no GDK display"))?;
        let compositor = CompositorState::new::<Self>(&dh);
        let xdg_shell = XdgShellState::new::<Self>(&dh);
        let xdg_decoration = XdgDecorationState::new::<Self>(&dh);
        // GTK3 and Qt drop their client-side titlebar when asked.
        let kde_decoration = KdeDecorationState::new::<Self>(&dh, KdeDefaultMode::Server);
        let shm = ShmState::new::<Self>(&dh, []);
        let viewporter = ViewporterState::new::<Self>(&dh);
        let single_pixel = SinglePixelBufferState::new::<Self>(&dh);
        let fractional = FractionalScaleManagerState::new::<Self>(&dh);
        let cursor_shape = CursorShapeManagerState::new::<Self>(&dh);
        let data_device = DataDeviceState::new::<Self>(&dh);
        let primary = PrimarySelectionState::new::<Self>(&dh);
        let text_input = TextInputState::new(&dh);

        let kb = KeyboardConfig::from_hyprland();
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "hw-embed");
        let keyboard = seat
            .add_keyboard(
                XkbConfig {
                    rules: &kb.rules,
                    model: &kb.model,
                    layout: &kb.layout,
                    variant: &kb.variant,
                    options: kb.options.clone(),
                },
                kb.repeat_delay,
                kb.repeat_rate,
            )
            .or_else(|e| {
                tracing::warn!("keymap {kb:?}: {e:?}; using the default");
                seat.add_keyboard(XkbConfig::default(), kb.repeat_delay, kb.repeat_rate)
            })
            .map_err(|e| anyhow::anyhow!("keyboard: {e:?}"))?;
        let pointer = seat.add_pointer();
        let touch = seat.add_touch();

        let output = Output::new(
            "HW-EMBED-1".into(),
            PhysicalProperties {
                size: (0, 0).into(),
                subpixel: Subpixel::Unknown,
                make: "hyprwidget".into(),
                model: "embed".into(),
            },
        );
        let _ = output.create_global::<Self>(&dh);
        let mode = OutputMode {
            size: (1920, 1080).into(),
            refresh: 60_000,
        };
        output.change_current_state(
            Some(mode),
            None,
            Some(Scale::Integer(1)),
            Some((0, 0).into()),
        );
        output.set_preferred(mode);

        let mut dmabuf = DmabufState::new();
        let mut dmabuf_global = None;
        let mut syncobj = None;
        match render_node() {
            Some(node) => {
                let formats = gdk_dmabuf_formats(&gdk_display);
                tracing::info!(node = %node.display(), formats = formats.len(), "dmabuf");
                let dev = std::fs::metadata(&node)?.rdev();
                if !formats.is_empty() {
                    let feedback = DmabufFeedbackBuilder::new(dev, formats).build()?;
                    dmabuf_global =
                        Some(dmabuf.create_global_with_default_feedback::<Self>(&dh, &feedback));
                }
                let fd: OwnedFd = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&node)?
                    .into();
                let drm = DrmDeviceFd::new(DeviceFd::from(fd));
                if std::env::var_os("HW_EMBED_NO_SYNCOBJ").is_some() {
                    tracing::warn!("HW_EMBED_NO_SYNCOBJ set; linux-drm-syncobj disabled");
                } else if supports_syncobj_eventfd(&drm) {
                    syncobj = Some(DrmSyncobjState::new::<Self>(&dh, drm));
                } else {
                    tracing::warn!("DRM device lacks syncobj eventfd; linux-drm-syncobj disabled");
                }
            }
            None => tracing::warn!("no DRM render node; dmabuf disabled"),
        }

        let clipboards = Clipboards::new(&gdk_display);
        Ok(Self {
            dh,
            gdk_display,
            compositor,
            xdg_shell,
            _xdg_decoration: xdg_decoration,
            kde_decoration,
            shm,
            _viewporter: viewporter,
            _single_pixel: single_pixel,
            _fractional: fractional,
            _cursor_shape: cursor_shape,
            data_device,
            primary,
            seat_state,
            seat,
            pointer,
            keyboard,
            touch,
            output,
            dmabuf,
            _dmabuf_global: dmabuf_global,
            syncobj,
            text_input,
            clipboards,
            views: HashMap::new(),
            clients: HashMap::new(),
            roots: HashMap::new(),
            root_surfaces: HashMap::new(),
            popups: HashMap::new(),
            pointer_root: None,
            cursor_surface: None,
            kb_focus: None,
        })
    }

    pub(crate) fn view_of(&self, surface: &WlSurface) -> Option<ViewId> {
        let client = surface.client()?;
        client.get_data::<ClientState>().map(|c| c.view)
    }

    pub(crate) fn view_widget(&self, id: ViewId) -> Option<EmbedView> {
        self.views.get(&id)?.view.upgrade()
    }

    fn surface_by_id(&self, id: &ObjectId) -> Option<WlSurface> {
        self.root_surfaces.get(id).cloned()
    }

    /// Widget showing the tree `surface` belongs to.
    pub(crate) fn sv_of(&self, surface: &WlSurface) -> Option<SurfaceView> {
        self.roots.get(&root_of(surface).id())?.upgrade()
    }

    pub(crate) fn add_root(&mut self, surface: &WlSurface, sv: &SurfaceView) {
        self.roots.insert(surface.id(), sv.downgrade());
        self.root_surfaces.insert(surface.id(), surface.clone());
    }

    pub(crate) fn remove_root(&mut self, surface: &WlSurface) {
        self.roots.remove(&surface.id());
        self.root_surfaces.remove(&surface.id());
    }

    /// Applies a committed surface tree to its widget.
    fn render_tree(&mut self, id: ViewId, root: &WlSurface) {
        struct Item {
            surface: WlSurface,
            loc: (i32, i32),
            buffer: Option<BufferAssignment>,
            damage: Vec<Damage>,
            acquire: Option<DrmSyncPoint>,
            release: Option<DrmSyncPoint>,
            size: Option<(i32, i32)>,
        }
        let mut items: Vec<Item> = Vec::new();
        let mut callbacks = Vec::new();
        with_surface_tree_upward(
            root,
            Point::<i32, Logical>::from((0, 0)),
            |_, states, loc| TraversalAction::DoChildren(*loc + sub_loc(states)),
            |s, states, loc| {
                let loc = *loc + sub_loc(states);
                let mut attrs = states.cached_state.get::<SurfaceAttributes>();
                let attrs = attrs.current();
                let buffer = attrs.buffer.take();
                let damage = std::mem::take(&mut attrs.damage);
                callbacks.append(&mut attrs.frame_callbacks);
                let (scale, transform) = (attrs.buffer_scale, attrs.buffer_transform);
                let mut sync = states.cached_state.get::<DrmSyncobjCachedState>();
                let sync = sync.current();
                let (acquire, release) = (sync.acquire_point.take(), sync.release_point.take());
                let mut vp = states.cached_state.get::<ViewportCachedState>();
                let vp = vp.current();
                let size = surface_info(states, |info| {
                    match &buffer {
                        Some(BufferAssignment::NewBuffer(b)) => {
                            info.size = buffer::buffer_size(b).map(|bs| {
                                let s = buffer::logical_size(
                                    bs,
                                    scale,
                                    transform,
                                    vp.dst,
                                    vp.src.map(|r| r.size),
                                );
                                (s.w, s.h)
                            });
                        }
                        Some(BufferAssignment::Removed) => info.size = None,
                        None => {}
                    }
                    info.size
                });
                items.push(Item {
                    surface: s.clone(),
                    loc: (loc.x, loc.y),
                    buffer,
                    damage,
                    acquire,
                    release,
                    size,
                });
            },
            |_, _, _| true,
        );

        let sv = self.sv_of(root);
        if let Some(slot) = self.views.get_mut(&id) {
            slot.frame_callbacks.extend(callbacks);
            if !slot.frame_callbacks.is_empty()
                && let Some(view) = slot.view.upgrade()
            {
                view.request_frame();
            }
        } else {
            for cb in callbacks {
                cb.done(0);
            }
        }
        let Some(sv) = sv else {
            for it in items {
                if let Some(BufferAssignment::NewBuffer(b)) = it.buffer {
                    buffer::discard(b, it.release);
                }
            }
            return;
        };

        let mut order = Vec::new();
        let mut bbox: Option<Rect> = None;
        for it in items {
            let sid = it.surface.id();
            match it.buffer {
                Some(BufferAssignment::NewBuffer(b)) => {
                    let prev = sv.texture_of(&sid);
                    match buffer::import(
                        &self.gdk_display,
                        b,
                        &it.damage,
                        prev.as_ref(),
                        it.release,
                        flush_later,
                    ) {
                        Ok((tex, _dmabuf)) => match it.acquire {
                            Some(point) => wait_acquire(&sv, sid.clone(), point, tex),
                            None => sv.set_texture(&sid, Some(&tex)),
                        },
                        Err(e) => tracing::warn!("buffer import failed: {e:#}"),
                    }
                }
                Some(BufferAssignment::Removed) => sv.set_texture(&sid, None),
                None => {}
            }
            if let Some(size) = it.size {
                let r = Rect {
                    x: it.loc.0,
                    y: it.loc.1,
                    w: size.0,
                    h: size.1,
                };
                bbox = Some(match bbox {
                    None => r,
                    Some(b) => union(b, r),
                });
                order.push(Placed {
                    id: sid,
                    loc: it.loc,
                    size,
                });
            }
        }
        let geometry = with_states(root, |s| {
            s.cached_state.get::<SurfaceCachedState>().current().geometry
        })
        .map(|g| Rect {
            x: g.loc.x,
            y: g.loc.y,
            w: g.size.w,
            h: g.size.h,
        })
        .or(bbox)
        .unwrap_or_default();
        let mapped = !order.is_empty();
        sv.set_layout(order, geometry);

        if let Some(popup) = self.popups.get_mut(&root.id()) {
            popup.set_mapped(mapped);
        } else if let Some(view) = self.view_widget(id) {
            view.content_changed();
        }
    }

    fn cursor_commit(&mut self, surface: &WlSurface) {
        let (buffer, release, callbacks, hotspot) = with_states(surface, |states| {
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            let attrs = attrs.current();
            let hotspot = states
                .data_map
                .get::<CursorImageSurfaceData>()
                .map(|d| d.lock().unwrap().hotspot)
                .unwrap_or_default();
            let mut sync = states.cached_state.get::<DrmSyncobjCachedState>();
            (
                attrs.buffer.take(),
                sync.current().release_point.take(),
                std::mem::take(&mut attrs.frame_callbacks),
                hotspot,
            )
        });
        let now = glib::monotonic_time() / 1000;
        for cb in callbacks {
            cb.done(now as u32);
        }
        let Some(BufferAssignment::NewBuffer(b)) = buffer else {
            return;
        };
        if self.cursor_surface.as_ref() != Some(surface) {
            buffer::discard(b, release);
            return;
        }
        match buffer::import(&self.gdk_display, b, &[], None, release, flush_later) {
            Ok((tex, _)) => {
                let cursor = gdk::Cursor::from_texture(&tex, hotspot.x, hotspot.y, None);
                self.set_cursor(Some(cursor));
            }
            Err(e) => tracing::debug!("cursor buffer: {e:#}"),
        }
    }

    fn set_cursor(&self, cursor: Option<gdk::Cursor>) {
        if let Some(sv) = self.pointer_root.as_ref().and_then(|r| self.sv_of(r)) {
            sv.set_cursor(cursor.as_ref());
        }
    }

    fn toplevel_slot(&self, surface: &ToplevelSurface) -> Option<(ViewId, usize)> {
        self.views.iter().find_map(|(id, v)| {
            v.toplevels
                .iter()
                .position(|(t, _)| t == surface)
                .map(|i| (*id, i))
        })
    }

    fn notify_title(&self, surface: &ToplevelSurface) {
        let Some((id, idx)) = self.toplevel_slot(surface) else {
            return;
        };
        if idx != 0 {
            return;
        }
        let (title, app_id) = with_states(surface.wl_surface(), |s| {
            let d = s.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap();
            (d.title.clone().unwrap_or_default(), d.app_id.clone().unwrap_or_default())
        });
        if let Some(view) = self.view_widget(id) {
            view.set_identity(&title, &app_id);
        }
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        w: (a.x + a.w).max(b.x + b.w) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}

/// Shows `texture` once the client's GPU work behind `point` has finished.
fn wait_acquire(sv: &SurfaceView, id: ObjectId, point: DrmSyncPoint, texture: gdk::Texture) {
    let fd = match point.eventfd() {
        Ok(fd) => fd,
        Err(e) => {
            tracing::warn!("syncobj eventfd: {e}; showing unsynchronised");
            sv.set_texture(&id, Some(&texture));
            return;
        }
    };
    let sv = sv.downgrade();
    let raw = fd.as_raw_fd();
    let mut pending = Some((fd, texture));
    glib_unix::unix_fd_add_local(raw, glib::IOCondition::IN, move |_, _| {
        if let Some((_fd, texture)) = pending.take()
            && let Some(sv) = sv.upgrade()
        {
            sv.set_texture(&id, Some(&texture));
        }
        glib::ControlFlow::Break
    });
}

fn gdk_dmabuf_formats(display: &gdk::Display) -> Vec<Format> {
    let formats = display.dmabuf_formats();
    (0..formats.n_formats())
        .filter_map(|i| {
            let (code, modifier) = formats.format(i);
            Some(Format {
                code: Fourcc::try_from(code).ok()?,
                modifier: Modifier::from(modifier),
            })
        })
        .collect()
}

/// Render node of the GPU Hyprland renders on (first of `AQ_DRM_DEVICES`),
/// overridable with `HW_EMBED_DRM_NODE`.
fn render_node() -> Option<PathBuf> {
    if let Some(node) = std::env::var_os("HW_EMBED_DRM_NODE") {
        return Some(node.into());
    }
    let card = std::env::var("AQ_DRM_DEVICES")
        .ok()
        .and_then(|v| v.split(':').next().map(PathBuf::from))
        .and_then(|p| std::fs::canonicalize(p).ok());
    if let Some(name) = card.as_deref().and_then(Path::file_name) {
        let dir = Path::new("/sys/class/drm").join(name).join("device/drm");
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let n = e.file_name();
                if n.to_string_lossy().starts_with("renderD") {
                    return Some(Path::new("/dev/dri").join(n));
                }
            }
        }
    }
    let fallback = PathBuf::from("/dev/dri/renderD128");
    fallback.exists().then_some(fallback)
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor
    }

    fn new_surface(&mut self, surface: &WlSurface) {
        if let (Some(client), Some(id)) = (surface.client(), self.view_of(surface)) {
            self.clients.insert(client.id(), id);
        }
    }

    fn commit(&mut self, surface: &WlSurface) {
        if compositor::get_role(surface) == Some("cursor_image") {
            self.cursor_commit(surface);
            return;
        }
        if compositor::is_sync_subsurface(surface) {
            return;
        }
        let root = root_of(surface);
        let Some(id) = self.view_of(&root) else {
            return;
        };
        let top = self.views.get(&id).and_then(|v| {
            v.toplevels
                .iter()
                .find(|(t, _)| t.wl_surface() == &root)
                .map(|(t, _)| t.clone())
        });
        if let Some(top) = top
            && !top.is_initial_configure_sent()
        {
            top.send_configure();
            return;
        }
        if let Some(popup) = self.popups.get(&root.id()).map(|p| p.surface.clone())
            && !popup.is_initial_configure_sent()
        {
            let _ = popup.send_configure();
            return;
        }
        self.render_tree(id, &root);
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm
    }
}

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        _dmabuf: smithay::backend::allocator::dmabuf::Dmabuf,
        notifier: ImportNotifier,
    ) {
        // Formats were taken from GDK, so it can import them; a real failure
        // shows up when the texture is built at commit time.
        let _ = notifier.successful::<State>();
    }
}

impl DrmSyncobjHandler for State {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj.as_mut()
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let Some(id) = self.view_of(surface.wl_surface()) else {
            return;
        };
        let (Some(server), Some(view)) = (EmbedServer::current(), self.view_widget(id)) else {
            return;
        };
        let sv = SurfaceView::new(&server, surface.wl_surface());
        // Dialogs keep their own size and are centred over the main window.
        let dialog = surface.parent().is_some();
        if dialog {
            configure_toplevel(&surface, None, false);
        } else {
            configure_toplevel(&surface, view.configure_size(), true);
        }
        self.add_root(surface.wl_surface(), &sv);
        if let Some(slot) = self.views.get_mut(&id) {
            slot.toplevels.push((surface, sv.clone()));
        }
        view.add_toplevel(&sv, dialog);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let Some((id, idx)) = self.toplevel_slot(&surface) else {
            return;
        };
        let (_, sv) = self.views.get_mut(&id).unwrap().toplevels.remove(idx);
        self.remove_root(surface.wl_surface());
        sv.clear();
        if let Some(view) = self.view_widget(id) {
            view.remove_toplevel(&sv);
        }
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        self.notify_title(&surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        self.notify_title(&surface);
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        self.popup_new(surface, positioner);
    }

    fn grab(&mut self, surface: PopupSurface, _seat: WlSeat, _serial: Serial) {
        if let Some(p) = self.popups.get_mut(&surface.wl_surface().id()) {
            p.set_grab();
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        self.popup_reposition(surface, positioner, token);
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        self.popup_destroyed_(&surface);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        // Already maximized; answer so the client is not left waiting.
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        if surface.is_initial_configure_sent() {
            surface.send_configure();
        }
    }
}

impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        XdgDecorationHandler::request_mode(self, toplevel, DecorationMode::ServerSide);
    }
}

impl KdeDecorationHandler for State {
    fn kde_decoration_state(&self) -> &KdeDecorationState {
        &self.kde_decoration
    }

    fn request_mode(
        &mut self,
        _surface: &WlSurface,
        decoration: &OrgKdeKwinServerDecoration,
        _mode: smithay::reexports::wayland_server::WEnum<KdeMode>,
    ) {
        decoration.mode(KdeMode::Server);
    }
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| self.dh.get_client(s.id()).ok());
        set_data_device_focus(&self.dh, seat, client.clone());
        set_primary_focus(&self.dh, seat, client);
        let old = std::mem::replace(&mut self.kb_focus, focused.cloned());
        self.text_input_focus(old.as_ref(), focused);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_surface = None;
        match image {
            CursorImageStatus::Hidden => self.set_cursor(gdk::Cursor::from_name("none", None)),
            CursorImageStatus::Named(icon) => {
                self.set_cursor(gdk::Cursor::from_name(icon.name(), None))
            }
            CursorImageStatus::Surface(s) => self.cursor_surface = Some(s),
        }
    }
}

impl OutputHandler for State {}

impl FractionalScaleHandler for State {
    fn new_fractional_scale(&mut self, surface: WlSurface) {
        let scale = self
            .view_of(&surface)
            .and_then(|id| self.views.get(&id))
            .map(|v| v.scale)
            .unwrap_or(1.0);
        with_states(&surface, |s| {
            with_fractional_scale(s, |fs| fs.set_preferred_scale(scale))
        });
    }
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary
    }
}

impl ClientDndGrabHandler for State {}
impl smithay::wayland::tablet_manager::TabletSeatHandler for State {}
impl ServerDndGrabHandler for State {}

delegate_compositor!(State);
delegate_shm!(State);
delegate_dmabuf!(State);
delegate_drm_syncobj!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_kde_decoration!(State);
delegate_seat!(State);
delegate_output!(State);
delegate_viewporter!(State);
delegate_single_pixel_buffer!(State);
delegate_fractional_scale!(State);
delegate_cursor_shape!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
