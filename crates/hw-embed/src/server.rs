//! In-process Wayland server (smithay), dispatched from the GLib main loop.
//!
//! Every embedded client is connected through its own socketpair, so the
//! server knows which [`EmbedView`] a surface belongs to without any socket
//! file. Only the toplevel's root surface is shown; subsurfaces, popups and
//! real input handling are M5.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use gtk4::{cairo, gdk, glib, prelude::*};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::{Buffer as _, Format, Fourcc, Modifier};
use smithay::backend::drm::DrmDeviceFd;
use smithay::input::keyboard::{FilterResult, KeyboardHandle, Keycode, XkbConfig};
use smithay::input::pointer::{ButtonEvent, MotionEvent, PointerHandle};
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::output::{Mode as OutputMode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_callback::WlCallback;
use smithay::reexports::wayland_server::protocol::wl_seat::WlSeat;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Client, Display, DisplayHandle, Resource};
use smithay::utils::{DeviceFd, Point, Serial, SERIAL_COUNTER};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{
    BufferAssignment, CompositorClientState, CompositorHandler, CompositorState, Damage,
    SurfaceAttributes, with_states,
};
use smithay::wayland::dmabuf::{
    DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier, get_dmabuf,
};
use smithay::wayland::drm_syncobj::{
    DrmSyncPoint, DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState,
    supports_syncobj_eventfd,
};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::selection::data_device::{
    ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::shell::xdg::decoration::{XdgDecorationHandler, XdgDecorationState};
use smithay::wayland::shell::xdg::{
    PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
};
use smithay::wayland::shm::{ShmHandler, ShmState, with_buffer_contents};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_drm_syncobj, delegate_output, delegate_seat,
    delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
};

use crate::view::EmbedView;

type ViewId = u64;

/// Handle to the nested compositor. Cheap to clone; lives on the GTK thread.
#[derive(Clone)]
pub struct EmbedServer(Rc<Inner>);

struct Inner {
    display: RefCell<Display<State>>,
    state: RefCell<State>,
    next_view: std::cell::Cell<ViewId>,
}

thread_local! {
    /// Lets texture release functions (which must be `Send`) flush the server.
    static SERVER: RefCell<Weak<Inner>> = const { RefCell::new(Weak::new()) };
}

impl EmbedServer {
    /// Creates the server and attaches its fd to the default GLib main context.
    /// Must be called after GTK is initialised (dmabuf formats come from GDK).
    pub fn new() -> anyhow::Result<Self> {
        let display: Display<State> = Display::new()?;
        let state = State::new(display.handle())?;
        let inner = Rc::new(Inner {
            display: RefCell::new(display),
            state: RefCell::new(state),
            next_view: std::cell::Cell::new(1),
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
        Ok(Self(inner))
    }

    /// Runs `argv` as a client of this server, shown inside `view`.
    pub fn spawn(&self, view: &EmbedView, argv: &[String]) -> anyhow::Result<u32> {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("empty command"))?;
        let id = self.0.next_view.get();
        self.0.next_view.set(id + 1);

        let (server_end, client_end) = UnixStream::pair()?;
        self.0.display.borrow_mut().handle().insert_client(
            server_end,
            Arc::new(ClientState {
                view: id,
                compositor: CompositorClientState::default(),
            }),
        )?;
        self.0.state.borrow_mut().views.insert(
            id,
            ViewSlot {
                view: view.downgrade(),
                toplevel: None,
                frame_callbacks: Vec::new(),
                texture: None,
            },
        );
        view.attach(self.clone(), id);

        let fd = client_end.as_raw_fd();
        let mut cmd = Command::new(program);
        cmd.args(args)
            .env("WAYLAND_SOCKET", fd.to_string())
            .env("GDK_BACKEND", "wayland")
            .env("QT_QPA_PLATFORM", "wayland")
            .env("SDL_VIDEODRIVER", "wayland")
            .env("XDG_SESSION_TYPE", "wayland")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY");
        // SAFETY: only async-signal-safe fcntl between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;
        drop(client_end);
        let pid = child.id();
        let program = program.clone();
        glib::child_watch_add_local(glib::Pid(pid as i32), move |_, status| {
            tracing::info!(%program, pid, status, "embedded client exited");
        });
        tracing::info!(pid, ?argv, "spawned embedded client");
        self.flush();
        Ok(pid)
    }

    pub(crate) fn resize(&self, id: ViewId, width: i32, height: i32) {
        if let Ok(state) = self.0.state.try_borrow()
            && let Some(toplevel) = state.views.get(&id).and_then(|v| v.toplevel.as_ref())
        {
            configure_toplevel(toplevel, width, height);
        }
        self.flush();
    }

    /// Sends `wl_surface.frame` done for everything the client queued.
    /// Returns whether the client is still waiting for a frame.
    pub(crate) fn frame_done(&self, id: ViewId, time_ms: u32) {
        if let Ok(mut state) = self.0.state.try_borrow_mut()
            && let Some(slot) = state.views.get_mut(&id)
        {
            for cb in slot.frame_callbacks.drain(..) {
                cb.done(time_ms);
            }
        }
        self.flush();
    }

    pub(crate) fn has_pending_frames(&self, id: ViewId) -> bool {
        self.0
            .state
            .try_borrow()
            .ok()
            .and_then(|s| s.views.get(&id).map(|v| !v.frame_callbacks.is_empty()))
            .unwrap_or(false)
    }

    /// Basic pointer forwarding (M0.5 stub; full input is M5).
    pub(crate) fn pointer_motion(&self, id: ViewId, x: f64, y: f64, time: u32) {
        let Ok(mut state) = self.0.state.try_borrow_mut() else {
            return;
        };
        let Some(surface) = state.root_surface(id) else {
            return;
        };
        let pointer = state.pointer.clone();
        pointer.motion(
            &mut state,
            Some((surface, Point::from((0.0, 0.0)))),
            &MotionEvent {
                location: (x, y).into(),
                serial: SERIAL_COUNTER.next_serial(),
                time,
            },
        );
        pointer.frame(&mut state);
        drop(state);
        self.flush();
    }

    pub(crate) fn pointer_button(&self, id: ViewId, button: u32, pressed: bool, time: u32) {
        let Ok(mut state) = self.0.state.try_borrow_mut() else {
            return;
        };
        if state.root_surface(id).is_none() {
            return;
        }
        let pointer = state.pointer.clone();
        pointer.button(
            &mut state,
            &ButtonEvent {
                serial: SERIAL_COUNTER.next_serial(),
                time,
                button,
                state: if pressed {
                    smithay::backend::input::ButtonState::Pressed
                } else {
                    smithay::backend::input::ButtonState::Released
                },
            },
        );
        pointer.frame(&mut state);
        drop(state);
        self.flush();
    }

    /// Basic keyboard forwarding; `keycode` is the XKB keycode GDK reports.
    pub(crate) fn key(&self, id: ViewId, keycode: u32, pressed: bool, time: u32) {
        let Ok(mut state) = self.0.state.try_borrow_mut() else {
            return;
        };
        let Some(surface) = state.root_surface(id) else {
            return;
        };
        let keyboard = state.keyboard.clone();
        let serial = SERIAL_COUNTER.next_serial();
        if keyboard.current_focus().as_ref() != Some(&surface) {
            keyboard.set_focus(&mut state, Some(surface), serial);
        }
        keyboard.input::<(), _>(
            &mut state,
            Keycode::new(keycode),
            if pressed {
                smithay::backend::input::KeyState::Pressed
            } else {
                smithay::backend::input::KeyState::Released
            },
            serial,
            time,
            |_, _, _| FilterResult::Forward,
        );
        drop(state);
        self.flush();
    }

    /// Closes the client of a view that is going away.
    pub(crate) fn detach(&self, id: ViewId) {
        if let Ok(mut state) = self.0.state.try_borrow_mut()
            && let Some(slot) = state.views.remove(&id)
            && let Some(toplevel) = slot.toplevel
        {
            toplevel.send_close();
        }
        self.flush();
    }

    fn flush(&self) {
        self.0.flush();
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

    fn flush(&self) {
        if let Ok(mut display) = self.display.try_borrow_mut() {
            let _ = display.flush_clients();
        }
    }
}

/// Flushes the server from a `Send` context (texture release functions).
fn flush_later() {
    glib::idle_add_once(|| {
        SERVER.with(|s| {
            if let Some(inner) = s.borrow().upgrade() {
                inner.flush();
            }
        });
    });
}

fn configure_toplevel(toplevel: &ToplevelSurface, width: i32, height: i32) {
    toplevel.with_pending_state(|s| {
        s.size = (width > 0 && height > 0).then(|| (width, height).into());
        for st in [
            xdg_toplevel::State::Maximized,
            xdg_toplevel::State::Activated,
            xdg_toplevel::State::TiledLeft,
            xdg_toplevel::State::TiledRight,
            xdg_toplevel::State::TiledTop,
            xdg_toplevel::State::TiledBottom,
        ] {
            s.states.set(st);
        }
        s.decoration_mode = Some(DecorationMode::ServerSide);
    });
    if toplevel.is_initial_configure_sent() {
        toplevel.send_pending_configure();
    }
}

struct ClientState {
    view: ViewId,
    compositor: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        tracing::debug!(view = self.view, ?reason, "embedded client disconnected");
    }
}

struct ViewSlot {
    view: glib::WeakRef<EmbedView>,
    toplevel: Option<ToplevelSurface>,
    frame_callbacks: Vec<WlCallback>,
    texture: Option<gdk::Texture>,
}

pub(crate) struct State {
    gdk_display: gdk::Display,
    compositor: CompositorState,
    xdg_shell: XdgShellState,
    _xdg_decoration: XdgDecorationState,
    shm: ShmState,
    data_device: DataDeviceState,
    seat_state: SeatState<State>,
    _seat: Seat<State>,
    pointer: PointerHandle<State>,
    keyboard: KeyboardHandle<State>,
    _output: Output,
    dmabuf: DmabufState,
    _dmabuf_global: Option<DmabufGlobal>,
    syncobj: Option<DrmSyncobjState>,
    views: HashMap<ViewId, ViewSlot>,
}

impl State {
    fn new(dh: DisplayHandle) -> anyhow::Result<Self> {
        let gdk_display =
            gdk::Display::default().ok_or_else(|| anyhow::anyhow!("no GDK display"))?;
        let compositor = CompositorState::new::<Self>(&dh);
        let xdg_shell = XdgShellState::new::<Self>(&dh);
        let xdg_decoration = XdgDecorationState::new::<Self>(&dh);
        let shm = ShmState::new::<Self>(&dh, []);
        // Clipboard is M5; the global exists because foot refuses to start without it.
        let data_device = DataDeviceState::new::<Self>(&dh);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "hw-embed");
        let keyboard = seat
            .add_keyboard(XkbConfig::default(), 600, 25)
            .map_err(|e| anyhow::anyhow!("keyboard: {e:?}"))?;
        let pointer = seat.add_pointer();

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
                if supports_syncobj_eventfd(&drm) {
                    syncobj = Some(DrmSyncobjState::new::<Self>(&dh, drm));
                } else {
                    tracing::warn!("DRM device lacks syncobj eventfd; linux-drm-syncobj disabled");
                }
            }
            None => tracing::warn!("no DRM render node; dmabuf disabled"),
        }

        Ok(Self {
            gdk_display,
            compositor,
            xdg_shell,
            _xdg_decoration: xdg_decoration,
            shm,
            data_device,
            seat_state,
            _seat: seat,
            pointer,
            keyboard,
            _output: output,
            dmabuf,
            _dmabuf_global: dmabuf_global,
            syncobj,
            views: HashMap::new(),
        })
    }

    fn view_of(&self, surface: &WlSurface) -> Option<ViewId> {
        let client = surface.client()?;
        client.get_data::<ClientState>().map(|c| c.view)
    }

    fn root_surface(&self, id: ViewId) -> Option<WlSurface> {
        self.views
            .get(&id)?
            .toplevel
            .as_ref()
            .map(|t| t.wl_surface().clone())
    }

    fn handle_root_commit(&mut self, id: ViewId, surface: &WlSurface) {
        let (buffer, damage, callbacks, acquire, release) = with_states(surface, |states| {
            let mut attrs = states.cached_state.get::<SurfaceAttributes>();
            let attrs = attrs.current();
            let buffer = attrs.buffer.take();
            let damage = std::mem::take(&mut attrs.damage);
            let callbacks = std::mem::take(&mut attrs.frame_callbacks);
            let mut sync = states.cached_state.get::<DrmSyncobjCachedState>();
            let sync = sync.current();
            (
                buffer,
                damage,
                callbacks,
                sync.acquire_point.take(),
                sync.release_point.take(),
            )
        });
        let display = self.gdk_display.clone();
        let Some(slot) = self.views.get_mut(&id) else {
            return;
        };
        slot.frame_callbacks.extend(callbacks);
        let Some(view) = slot.view.upgrade() else {
            return;
        };

        match buffer {
            Some(BufferAssignment::NewBuffer(buffer)) => {
                if let Ok(dmabuf) = get_dmabuf(&buffer) {
                    let dmabuf = dmabuf.clone();
                    match dmabuf_texture(&display, &dmabuf, buffer, release) {
                        Ok(texture) => match acquire {
                            Some(point) => wait_acquire(&view, point, texture),
                            None => view.set_texture(Some(&texture)),
                        },
                        Err(e) => tracing::warn!("dmabuf import failed: {e}"),
                    }
                    slot.texture = None;
                } else {
                    match shm_texture(&buffer, &damage, slot.texture.as_ref()) {
                        Ok(texture) => {
                            view.set_texture(Some(&texture));
                            slot.texture = Some(texture);
                        }
                        Err(e) => tracing::warn!("shm buffer: {e}"),
                    }
                    buffer.release();
                }
            }
            Some(BufferAssignment::Removed) => {
                slot.texture = None;
                view.set_texture(None);
            }
            None => {}
        }
        if !slot.frame_callbacks.is_empty() {
            view.request_frame();
        }
    }
}

/// Shows `texture` once the client's GPU work behind `point` has finished.
fn wait_acquire(view: &EmbedView, point: DrmSyncPoint, texture: gdk::Texture) {
    let fd = match point.eventfd() {
        Ok(fd) => fd,
        Err(e) => {
            tracing::warn!("syncobj eventfd: {e}; showing unsynchronised");
            view.set_texture(Some(&texture));
            return;
        }
    };
    let view = view.downgrade();
    let raw = fd.as_raw_fd();
    let mut pending = Some((fd, texture));
    glib_unix::unix_fd_add_local(raw, glib::IOCondition::IN, move |_, _| {
        if let Some((_fd, texture)) = pending.take()
            && let Some(view) = view.upgrade()
        {
            view.set_texture(Some(&texture));
        }
        glib::ControlFlow::Break
    });
}

fn dmabuf_texture(
    display: &gdk::Display,
    dmabuf: &Dmabuf,
    buffer: WlBuffer,
    release: Option<DrmSyncPoint>,
) -> Result<gdk::Texture, glib::Error> {
    let format = dmabuf.format();
    let mut builder = gdk::DmabufTextureBuilder::new()
        .set_display(display)
        .set_width(dmabuf.width())
        .set_height(dmabuf.height())
        .set_fourcc(format.code as u32)
        .set_modifier(format.modifier.into())
        .set_premultiplied(true)
        .set_n_planes(dmabuf.num_planes() as u32);
    for (i, ((fd, offset), stride)) in dmabuf
        .handles()
        .zip(dmabuf.offsets())
        .zip(dmabuf.strides())
        .enumerate()
    {
        // SAFETY: the fd stays open while the `Dmabuf` (moved into the release
        // func below) is alive, i.e. until GDK is done with the texture.
        builder = unsafe { builder.set_fd(i as u32, fd.as_raw_fd()) }
            .set_offset(i as u32, offset)
            .set_stride(i as u32, stride);
    }
    let keep = dmabuf.clone();
    // SAFETY: see above; GDK calls the release func exactly once.
    unsafe {
        builder.build_with_release_func(move || {
            drop(keep);
            if let Some(point) = release
                && let Err(e) = point.signal()
            {
                tracing::warn!("syncobj release: {e}");
            }
            if buffer.is_alive() {
                buffer.release();
            }
            flush_later();
        })
    }
}

/// Copies an shm buffer into a `GdkMemoryTexture`; tells GSK which region
/// changed so only the damage gets re-uploaded.
fn shm_texture(
    buffer: &WlBuffer,
    damage: &[Damage],
    previous: Option<&gdk::Texture>,
) -> anyhow::Result<gdk::Texture> {
    use smithay::reexports::wayland_server::protocol::wl_shm::Format as ShmFormat;

    with_buffer_contents(buffer, |ptr, len, data| {
        let format = match data.format {
            ShmFormat::Argb8888 => gdk::MemoryFormat::B8g8r8a8Premultiplied,
            ShmFormat::Xrgb8888 => gdk::MemoryFormat::B8g8r8x8,
            ShmFormat::Abgr8888 => gdk::MemoryFormat::R8g8b8a8Premultiplied,
            ShmFormat::Xbgr8888 => gdk::MemoryFormat::R8g8b8x8,
            other => anyhow::bail!("unsupported shm format {other:?}"),
        };
        let start = data.offset as usize;
        let size = data.stride as usize * data.height as usize;
        anyhow::ensure!(start + size <= len, "shm buffer out of bounds");
        // SAFETY: smithay mapped `len` bytes at `ptr`; range checked above.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.add(start), size) };
        let mut builder = gdk::MemoryTextureBuilder::new()
            .set_bytes(Some(&glib::Bytes::from(bytes)))
            .set_width(data.width)
            .set_height(data.height)
            .set_stride(data.stride as usize)
            .set_format(format);
        if let Some(prev) = previous
            .filter(|p| p.width() == data.width && p.height() == data.height && !damage.is_empty())
        {
            let region = cairo::Region::create();
            for d in damage {
                let (loc, size) = match d {
                    Damage::Surface(r) => (r.loc.into(), r.size.into()),
                    Damage::Buffer(r) => ((r.loc.x, r.loc.y), (r.size.w, r.size.h)),
                };
                let _ =
                    region.union_rectangle(&cairo::RectangleInt::new(loc.0, loc.1, size.0, size.1));
            }
            builder = builder
                .set_update_texture(Some(prev))
                .set_update_region(Some(&region));
        }
        Ok(builder.build())
    })?
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

    fn commit(&mut self, surface: &WlSurface) {
        let Some(id) = self.view_of(surface) else {
            return;
        };
        let Some(toplevel) = self.views.get(&id).and_then(|v| v.toplevel.clone()) else {
            return;
        };
        if toplevel.wl_surface() != surface {
            return;
        }
        if !toplevel.is_initial_configure_sent() {
            toplevel.send_configure();
            return;
        }
        self.handle_root_commit(id, surface);
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
        _dmabuf: Dmabuf,
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
        let Some(slot) = self.views.get_mut(&id) else {
            return;
        };
        if slot.toplevel.is_some() {
            tracing::warn!(view = id, "second toplevel ignored (M5)");
            return;
        }
        let (w, h) = slot
            .view
            .upgrade()
            .map(|v| (v.width(), v.height()))
            .unwrap_or((0, 0));
        configure_toplevel(&surface, w, h);
        slot.toplevel = Some(surface);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        for slot in self.views.values_mut() {
            if slot.toplevel.as_ref() == Some(&surface) {
                slot.toplevel = None;
                slot.texture = None;
                slot.frame_callbacks.clear();
                if let Some(view) = slot.view.upgrade() {
                    view.set_texture(None);
                }
            }
        }
    }

    fn new_popup(&mut self, _surface: PopupSurface, _positioner: PositionerState) {
        tracing::debug!("xdg_popup not supported yet (M5)");
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
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
        self.request_mode(toplevel, DecorationMode::ServerSide);
    }
}

impl SeatHandler for State {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }
}

impl OutputHandler for State {}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device
    }
}

impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {}

delegate_compositor!(State);
delegate_shm!(State);
delegate_dmabuf!(State);
delegate_drm_syncobj!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_seat!(State);
delegate_output!(State);
delegate_data_device!(State);
