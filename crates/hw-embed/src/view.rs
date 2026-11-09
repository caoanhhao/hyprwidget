//! `EmbedView`: the widget behind `W.App`. Runs a command as a client of
//! the nested compositor and shows its toplevel(s); handles restart,
//! keep-alive, close and frame pacing.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk4::subclass::prelude::*;
use gtk4::{glib, graphene, gsk, prelude::*};

use crate::cast;
use crate::process::{self, Connection, SocketGuard};
use crate::server::{EmbedServer, ToplevelKind, ViewId};
use crate::surface::SurfaceView;

/// How the app's size follows the widget.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// The app is told the widget size and redraws at that size.
    #[default]
    Resize,
    /// The app keeps its own size; the picture is scaled to fit.
    Scale,
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "resize" => Ok(Self::Resize),
            "scale" => Ok(Self::Scale),
            _ => Err(format!("mode must be \"resize\" or \"scale\", got {s:?}")),
        }
    }
}

/// What to do when the app exits by itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Restart {
    #[default]
    Never,
    OnExit,
    OnFailure,
}

impl std::str::FromStr for Restart {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "never" | "no" => Ok(Self::Never),
            "on-exit" | "on_exit" | "always" => Ok(Self::OnExit),
            "on-failure" | "on_failure" => Ok(Self::OnFailure),
            _ => Err(format!(
                "restart must be \"never\", \"on-exit\" or \"on-failure\", got {s:?}"
            )),
        }
    }
}

/// Grace period before a hidden app without `keep_alive` is closed.
const HIDE_GRACE: Duration = Duration::from_secs(2);
/// `xdg_toplevel.close` → SIGTERM → SIGKILL.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);
/// Restart backoff: a run shorter than this counts as a crash.
const QUICK_EXIT: Duration = Duration::from_secs(3);
const MAX_QUICK_RESTARTS: u32 = 5;

#[derive(Debug, Clone, Default)]
struct Config {
    command: Vec<String>,
    env: Vec<(String, String)>,
    mode: Mode,
    keep_alive: bool,
    restart: Restart,
    frame_when_hidden: bool,
    socket: bool,
    size: Option<(i32, i32)>,
    rounding: f64,
}

struct Session {
    id: ViewId,
    pid: u32,
    alive: Rc<Cell<bool>>,
    exit: Option<i32>,
    clients: usize,
    socket: Option<SocketGuard>,
    started: Instant,
    /// We asked it to close: no restart.
    stopping: bool,
}

type StrCallback = Rc<dyn Fn(&EmbedView, &str)>;
type ExitCallback = Rc<dyn Fn(&EmbedView, i32)>;
type PidCallback = Rc<dyn Fn(&EmbedView, Option<u32>)>;

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

    /// Command line; restarts a running app when it changes.
    pub fn set_command(&self, argv: Vec<String>) {
        let changed = self.imp().config.borrow().command != argv;
        self.imp().config.borrow_mut().command = argv;
        if changed && self.imp().session.borrow().is_some() {
            self.restart();
        }
    }

    pub fn set_env(&self, env: Vec<(String, String)>) {
        self.imp().config.borrow_mut().env = env;
    }

    pub fn set_mode(&self, mode: Mode) {
        self.imp().config.borrow_mut().mode = mode;
        for sv in self.imp().toplevels.borrow().iter() {
            sv.set_scale_mode(mode);
        }
        self.queue_resize();
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "public setter used by hw-ui (W.App property); signature kept stable"
    )]
    pub fn set_keep_alive(&self, on: bool) {
        self.imp().config.borrow_mut().keep_alive = on;
    }

    pub fn set_restart(&self, policy: Restart) {
        self.imp().config.borrow_mut().restart = policy;
    }

    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "public setter used by hw-ui (W.App property); signature kept stable"
    )]
    pub fn set_frame_when_hidden(&self, on: bool) {
        self.imp().config.borrow_mut().frame_when_hidden = on;
    }

    /// Socket mode (`WAYLAND_DISPLAY=hyprwidget-embed-N`) for apps that open
    /// several connections or fork; takes effect on the next start.
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "public setter used by hw-ui (W.App property); signature kept stable"
    )]
    pub fn set_socket(&self, on: bool) {
        self.imp().config.borrow_mut().socket = on;
    }

    /// Natural size; `None` follows the app's own size.
    pub fn set_size(&self, size: Option<(i32, i32)>) {
        self.imp().config.borrow_mut().size = size;
        self.queue_resize();
    }

    /// Corner radius in pixels; 0 keeps zero-copy offload possible.
    pub fn set_rounding(&self, radius: f64) {
        self.imp().config.borrow_mut().rounding = radius.max(0.0);
        self.queue_draw();
    }

    pub fn pid(&self) -> Option<u32> {
        self.imp().session.borrow().as_ref().map(|s| s.pid)
    }

    pub fn title(&self) -> String {
        self.imp().title.borrow().clone()
    }

    pub fn app_id(&self) -> String {
        self.imp().app_id.borrow().clone()
    }

    pub fn is_running(&self) -> bool {
        self.imp().session.borrow().is_some()
    }

    pub fn connect_title_changed(&self, f: impl Fn(&Self, &str) + 'static) {
        self.imp().on_title.borrow_mut().push(Rc::new(f));
    }

    pub fn connect_app_id_changed(&self, f: impl Fn(&Self, &str) + 'static) {
        self.imp().on_app_id.borrow_mut().push(Rc::new(f));
    }

    /// Called with the exit code (128 + signal when killed).
    pub fn connect_exited(&self, f: impl Fn(&Self, i32) + 'static) {
        self.imp().on_exit.borrow_mut().push(Rc::new(f));
    }

    pub fn connect_pid_changed(&self, f: impl Fn(&Self, Option<u32>) + 'static) {
        self.imp().on_pid.borrow_mut().push(Rc::new(f));
    }

    /// Starts the app if it is not running.
    pub fn start(&self) {
        let imp = self.imp();
        if imp.session.borrow().is_some() {
            return;
        }
        let cfg = imp.config.borrow().clone();
        if cfg.command.is_empty() {
            return;
        }
        let server = match EmbedServer::shared() {
            Ok(s) => s,
            Err(e) => {
                self.set_placeholder(&format!("embedding unavailable: {e:#}"));
                return;
            }
        };
        let id = server.open_session(self);
        let connection = if cfg.socket {
            Connection::Listener
        } else {
            Connection::Inherited
        };
        let spawned = match process::spawn(&server, id, &cfg.command, &cfg.env, connection) {
            Ok(s) => s,
            Err(e) => {
                server.drop_session(id);
                tracing::warn!(command = ?cfg.command, "W.App: {e:#}");
                self.set_placeholder(&format!("{}: {e}", cfg.command[0]));
                return;
            }
        };
        let alive = Rc::new(Cell::new(true));
        let (weak, a, srv) = (self.downgrade(), Rc::clone(&alive), server.clone());
        glib::child_watch_add_local(glib::Pid(spawned.pid.cast_signed()), move |_, status| {
            a.set(false);
            let code = process::exit_code(status);
            match weak.upgrade() {
                Some(view) => view.process_exited(id, code),
                None => srv.drop_session(id),
            }
        });
        imp.session.replace(Some(Session {
            id,
            pid: spawned.pid,
            alive,
            exit: None,
            clients: 0,
            socket: spawned.socket,
            started: Instant::now(),
            stopping: false,
        }));
        imp.server.replace(Some(server.clone()));
        imp.auto_start.set(true);
        let prog = cfg.command[0].clone();
        self.set_placeholder(&format!("Starting {prog}…"));
        server.set_scale(id, self.scale());
        self.emit_pid();
    }

    /// `xdg_toplevel.close`, then SIGTERM / SIGKILL if it does not exit.
    pub fn send_close(&self) {
        self.imp().auto_start.set(false);
        self.stop();
    }

    /// Closes the app (if running) and starts it again.
    pub fn restart(&self) {
        let imp = self.imp();
        if imp.session.borrow().is_some() {
            imp.restart_pending.set(true);
            self.stop();
        } else {
            imp.quick_exits.set(0);
            self.start();
        }
    }

    /// Gives keyboard focus to the app.
    pub fn focus(&self) -> bool {
        self.imp()
            .toplevels
            .borrow()
            .last()
            .is_some_and(WidgetExt::grab_focus)
    }

    fn stop(&self) {
        let imp = self.imp();
        let Some((id, pid, alive)) = imp.session.borrow_mut().as_mut().map(|s| {
            s.stopping = true;
            (s.id, s.pid, Rc::clone(&s.alive))
        }) else {
            return;
        };
        let asked = imp
            .server
            .borrow()
            .as_ref()
            .is_some_and(|s| s.close_session(id));
        // In socket mode the session can outlive the reaped child; its pid may be reused.
        if !asked && alive.get() {
            process::signal(pid, libc::SIGTERM);
        }
        let a = alive;
        glib::timeout_add_local_once(CLOSE_TIMEOUT, move || {
            if a.get() {
                tracing::info!(pid, "embedded app did not close; SIGTERM");
                process::signal(pid, libc::SIGTERM);
                glib::timeout_add_local_once(CLOSE_TIMEOUT, move || {
                    if a.get() {
                        process::signal(pid, libc::SIGKILL);
                    }
                });
            }
        });
    }

    fn process_exited(&self, id: ViewId, code: i32) {
        let done = {
            let mut s = self.imp().session.borrow_mut();
            match s.as_mut().filter(|s| s.id == id) {
                Some(s) => {
                    s.exit = Some(code);
                    s.socket.is_none() || s.clients == 0
                }
                None => return,
            }
        };
        if done {
            self.end_session(code);
        }
    }

    /// Socket mode: the session also lasts while forked clients are connected.
    pub(crate) fn clients_changed(&self, id: ViewId, left: usize) {
        let code = {
            let mut s = self.imp().session.borrow_mut();
            match s.as_mut().filter(|s| s.id == id) {
                Some(s) => {
                    s.clients = left;
                    s.exit.filter(|_| left == 0)
                }
                None => return,
            }
        };
        if let Some(code) = code {
            self.end_session(code);
        }
    }

    fn end_session(&self, code: i32) {
        let imp = self.imp();
        let Some(session) = imp.session.take() else {
            return;
        };
        if let Some(server) = imp.server.borrow().as_ref() {
            server.drop_session(session.id);
        }
        for sv in imp.toplevels.take() {
            sv.unparent();
        }
        tracing::info!(pid = session.pid, code, "embedded app exited");
        self.emit_pid();
        let callbacks = imp.on_exit.borrow().clone();
        for f in callbacks {
            f(self, code);
        }

        let prog = imp
            .config
            .borrow()
            .command
            .first()
            .cloned()
            .unwrap_or_default();
        if imp.restart_pending.take() {
            self.start();
            return;
        }
        let policy = imp.config.borrow().restart;
        let wanted = !session.stopping
            && match policy {
                Restart::Never => false,
                Restart::OnExit => true,
                Restart::OnFailure => code != 0,
            };
        if !session.stopping {
            imp.auto_start.set(false);
        }
        if !wanted {
            self.set_placeholder(&format!("{prog} exited ({code}) — click to start"));
            return;
        }
        let quick = session.started.elapsed() < QUICK_EXIT;
        let n = if quick { imp.quick_exits.get() + 1 } else { 0 };
        imp.quick_exits.set(n);
        if n > MAX_QUICK_RESTARTS {
            self.set_placeholder(&format!("{prog} keeps exiting ({code}) — click to retry"));
            return;
        }
        self.set_placeholder(&format!("{prog} exited ({code}), restarting…"));
        let delay = Duration::from_millis(250 << n.min(5));
        glib::timeout_add_local_once(
            delay,
            glib::clone!(
                #[weak(rename_to = view)]
                self,
                move || view.start()
            ),
        );
    }

    fn emit_pid(&self) {
        let pid = self.pid();
        let callbacks = self.imp().on_pid.borrow().clone();
        for f in callbacks {
            f(self, pid);
        }
    }

    fn set_placeholder(&self, text: &str) {
        self.imp().placeholder.set_label(text);
        self.content_changed();
    }

    /// Widget scale (fractional scale preferred to clients).
    pub(crate) fn scale(&self) -> f64 {
        self.native()
            .and_then(|n| n.surface())
            .map_or_else(|| f64::from(self.scale_factor()), |s| s.scale())
    }

    /// Size to configure the main toplevel with.
    pub(crate) fn configure_size(&self) -> Option<(i32, i32)> {
        let cfg = self.imp().config.borrow();
        match cfg.mode {
            Mode::Scale => cfg.size,
            Mode::Resize => {
                let (w, h) = (self.width(), self.height());
                if w > 0 && h > 0 {
                    Some((w, h))
                } else {
                    cfg.size
                }
            }
        }
    }

    pub(crate) fn add_toplevel(&self, sv: &SurfaceView, kind: ToplevelKind) {
        if kind == ToplevelKind::Dialog {
            sv.set_scale_mode(Mode::Resize);
            sv.center();
        } else {
            sv.set_scale_mode(self.imp().config.borrow().mode);
        }
        sv.set_parent(self);
        self.imp().toplevels.borrow_mut().push(sv.clone());
        self.content_changed();
        if self.has_focus() || self.focus_child().is_some() {
            sv.grab_focus();
        }
    }

    pub(crate) fn remove_toplevel(&self, sv: &SurfaceView) {
        self.imp().toplevels.borrow_mut().retain(|t| t != sv);
        sv.unparent();
        self.content_changed();
    }

    pub(crate) fn set_identity(&self, title: &str, app_id: &str) {
        let imp = self.imp();
        if *imp.title.borrow() != title {
            imp.title.replace(title.to_owned());
            let callbacks = imp.on_title.borrow().clone();
            for f in callbacks {
                f(self, title);
            }
        }
        if *imp.app_id.borrow() != app_id {
            imp.app_id.replace(app_id.to_owned());
            let callbacks = imp.on_app_id.borrow().clone();
            for f in callbacks {
                f(self, app_id);
            }
        }
    }

    /// The placeholder shows while no toplevel has content.
    pub(crate) fn content_changed(&self) {
        let shown = self
            .imp()
            .toplevels
            .borrow()
            .iter()
            .any(SurfaceView::has_content);
        self.imp().placeholder.set_visible(!shown);
    }

    /// The client queued `wl_surface.frame`; answer on the next frame clock
    /// tick. Nothing is scheduled while unmapped unless `frame_when_hidden`.
    pub(crate) fn request_frame(&self) {
        let imp = self.imp();
        if imp.tick.borrow().is_some() || imp.timer.borrow().is_some() {
            return;
        }
        if self.is_mapped() {
            let id = self.add_tick_callback(|view, clock| {
                view.imp().tick.take();
                view.frame_done(cast::time_ms(clock.frame_time()));
                glib::ControlFlow::Break
            });
            imp.tick.replace(Some(id));
        } else if imp.config.borrow().frame_when_hidden {
            let id = glib::timeout_add_local_once(
                Duration::from_millis(16),
                glib::clone!(
                    #[weak(rename_to = view)]
                    self,
                    move || {
                        view.imp().timer.take();
                        view.frame_done(cast::time_ms(glib::monotonic_time()));
                    }
                ),
            );
            imp.timer.replace(Some(id));
        }
    }

    fn frame_done(&self, time: u32) {
        let imp = self.imp();
        if let (Some(server), Some(id)) = (
            imp.server.borrow().as_ref(),
            imp.session.borrow().as_ref().map(|s| s.id),
        ) {
            server.frame_done(id, time);
        }
    }

    fn session_id(&self) -> Option<ViewId> {
        self.imp().session.borrow().as_ref().map(|s| s.id)
    }

    fn with_server(&self, f: impl FnOnce(&EmbedServer, ViewId)) {
        if let (Some(server), Some(id)) = (self.imp().server.borrow().clone(), self.session_id()) {
            f(&server, id);
        }
    }
}

mod imp {
    use gtk4::prelude::*;
    use gtk4::subclass::prelude::*;

    use super::{
        Cell, Config, EmbedServer, ExitCallback, HIDE_GRACE, PidCallback, RefCell, Session,
        StrCallback, SurfaceView, cast, glib, graphene, gsk,
    };

    #[derive(Default)]
    pub struct EmbedView {
        pub(super) config: RefCell<Config>,
        pub(super) session: RefCell<Option<Session>>,
        pub(super) server: RefCell<Option<EmbedServer>>,
        pub(super) toplevels: RefCell<Vec<SurfaceView>>,
        pub(super) placeholder: gtk4::Label,
        pub(super) title: RefCell<String>,
        pub(super) app_id: RefCell<String>,
        pub(super) tick: RefCell<Option<gtk4::TickCallbackId>>,
        pub(super) timer: RefCell<Option<glib::SourceId>>,
        pub(super) hide_timer: RefCell<Option<glib::SourceId>>,
        pub(super) scale_handler: RefCell<Option<(gtk4::gdk::Surface, glib::SignalHandlerId)>>,
        /// Start when mapped (cleared after the app exits on its own).
        pub(super) auto_start: Cell<bool>,
        pub(super) restart_pending: Cell<bool>,
        pub(super) quick_exits: Cell<u32>,
        pub(super) on_title: RefCell<Vec<StrCallback>>,
        pub(super) on_app_id: RefCell<Vec<StrCallback>>,
        pub(super) on_exit: RefCell<Vec<ExitCallback>>,
        pub(super) on_pid: RefCell<Vec<PidCallback>>,
    }

    impl std::fmt::Debug for EmbedView {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("EmbedView")
                .field("config", &self.config)
                .field("title", &self.title)
                .field("app_id", &self.app_id)
                .finish_non_exhaustive()
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EmbedView {
        const NAME: &'static str = "HwEmbedView";
        type Type = super::EmbedView;
        type ParentType = gtk4::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("embedview");
        }
    }

    impl ObjectImpl for EmbedView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_overflow(gtk4::Overflow::Hidden);
            self.auto_start.set(true);
            self.placeholder.add_css_class("hw-app-placeholder");
            self.placeholder.add_css_class("dim-label");
            self.placeholder.set_wrap(true);
            self.placeholder.set_parent(&*obj);
            let click = gtk4::GestureClick::new();
            click.connect_released(glib::clone!(
                #[weak]
                obj,
                move |_, _, _, _| {
                    if !obj.is_running() {
                        obj.imp().quick_exits.set(0);
                        obj.start();
                    }
                }
            ));
            self.placeholder.add_controller(click);
            // After the builder applied every property: keep_alive apps
            // start right away, the rest when first shown.
            glib::idle_add_local_once(glib::clone!(
                #[weak]
                obj,
                move || {
                    if obj.imp().config.borrow().keep_alive || obj.is_mapped() {
                        obj.start();
                    }
                }
            ));
        }

        fn dispose(&self) {
            if let Some(id) = self.hide_timer.take() {
                id.remove();
            }
            if let Some(id) = self.timer.take() {
                id.remove();
            }
            if let Some((surface, h)) = self.scale_handler.take() {
                surface.disconnect(h);
            }
            self.obj().stop();
            for sv in self.toplevels.take() {
                sv.unparent();
            }
            self.placeholder.unparent();
        }
    }

    impl WidgetImpl for EmbedView {
        fn measure(&self, orientation: gtk4::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let horizontal = orientation == gtk4::Orientation::Horizontal;
            let (pmin, pnat, _, _) = self.placeholder.measure(orientation, for_size);
            if let Some((w, h)) = self.config.borrow().size {
                let v = if horizontal { w } else { h };
                return (pmin.min(v), v, -1, -1);
            }
            // Otherwise follow the app's own size.
            let app = self.toplevels.borrow().first().map_or(0, |sv| {
                let g = sv.geometry();
                if horizontal { g.w } else { g.h }
            });
            (pmin, app.max(pnat), -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.placeholder.allocate(width, height, baseline, None);
            for sv in self.toplevels.borrow().iter() {
                sv.allocate(width, height, baseline, None);
            }
            let obj = self.obj();
            let size = obj.configure_size();
            obj.with_server(|s, id| s.configure(id, size));
        }

        fn snapshot(&self, snapshot: &gtk4::Snapshot) {
            let obj = self.obj();
            let radius = cast::gsk(self.config.borrow().rounding);
            let clip = radius > 0.0;
            if clip {
                let (w, h) = (f64::from(obj.width()), f64::from(obj.height()));
                let rect = graphene::Rect::new(0.0, 0.0, cast::gsk(w), cast::gsk(h));
                snapshot.push_rounded_clip(&gsk::RoundedRect::from_rect(rect, radius));
            }
            let mut child = obj.first_child();
            while let Some(c) = child {
                obj.snapshot_child(&c, snapshot);
                child = c.next_sibling();
            }
            if clip {
                snapshot.pop();
            }
        }

        fn realize(&self) {
            self.parent_realize();
            let obj = self.obj();
            if let Some(surface) = obj.native().and_then(|n| n.surface()) {
                let h = surface.connect_scale_notify(glib::clone!(
                    #[weak]
                    obj,
                    move |s| {
                        let scale = s.scale();
                        obj.with_server(|srv, id| srv.set_scale(id, scale));
                    }
                ));
                self.scale_handler.replace(Some((surface, h)));
            }
        }

        fn unrealize(&self) {
            if let Some((surface, h)) = self.scale_handler.take() {
                surface.disconnect(h);
            }
            self.parent_unrealize();
        }

        fn map(&self) {
            self.parent_map();
            let obj = self.obj();
            if let Some(id) = self.hide_timer.take() {
                id.remove();
            }
            if let Some(id) = self.timer.take() {
                id.remove();
            }
            if self.session.borrow().is_none() {
                if self.auto_start.get() {
                    obj.start();
                }
                return;
            }
            let server = self.server.borrow().clone();
            if server
                .zip(obj.session_id())
                .is_some_and(|(s, id)| s.has_pending_frames(id))
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
            let obj = self.obj();
            if !self.config.borrow().keep_alive && self.session.borrow().is_some() {
                let id = glib::timeout_add_local_once(
                    HIDE_GRACE,
                    glib::clone!(
                        #[weak]
                        obj,
                        move || {
                            obj.imp().hide_timer.take();
                            if !obj.is_mapped() {
                                obj.stop();
                            }
                        }
                    ),
                );
                if let Some(old) = self.hide_timer.replace(Some(id)) {
                    old.remove();
                }
            } else if self.config.borrow().frame_when_hidden {
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    obj,
                    move || obj.request_frame()
                ));
            }
        }
    }
}
