//! MPRIS media players (`org.mpris.MediaPlayer2.*` on the session bus).
//!
//! Players don't announce `Position` changes, so positions are interpolated
//! from the last known value, the playback rate and the time it was read;
//! [`Mpris::position`] ticks once a second while something plays.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

use futures_util::StreamExt;
use gio::prelude::*;
use hw_core::reactive::{Signal, SourceGuard, computed};
use serde::Serialize;
use zbus::Connection;
use zbus::fdo::DBusProxy;
use zbus::zvariant::{ObjectPath, Value};

use crate::dbus::{self, Props, p_bool, p_f64, p_str, v_dict, v_i64, v_str, v_strs};
use crate::num::{f64_to_i64, i64_to_f64};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_IFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors MPRIS properties, serialized to Lua as flat booleans"
)]
pub struct Player {
    /// `org.mpris.MediaPlayer2.spotify` — the id used by control methods.
    pub bus_name: String,
    /// `spotify`.
    pub name: String,
    pub identity: String,
    pub desktop_entry: String,
    /// `playing`, `paused`, `stopped`.
    pub playback_status: String,
    pub playing: bool,
    pub track_id: String,
    pub title: String,
    /// Artists joined with ", ".
    pub artist: String,
    pub artists: Vec<String>,
    pub album: String,
    pub album_artist: String,
    pub url: String,
    pub art_url: String,
    /// Local file for the cover (downloaded for http(s) art), or empty.
    pub cover_art: String,
    /// Seconds (0 when unknown).
    pub length: f64,
    /// Seconds, as of the last update (see [`Mpris::position`]).
    pub position: f64,
    pub rate: f64,
    pub volume: f64,
    pub shuffle: bool,
    /// `None`, `Track`, `Playlist`.
    pub loop_status: String,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_seek: bool,
    pub can_control: bool,
    pub can_raise: bool,
    pub can_quit: bool,
    /// `glib::monotonic_time()` when `position` was read.
    #[serde(skip)]
    pos_at: i64,
}

impl Player {
    /// Interpolated position in seconds.
    pub fn position_now(&self) -> f64 {
        let mut p = self.position;
        if self.playing {
            let dt = i64_to_f64(glib::monotonic_time() - self.pos_at) / 1e6;
            p += dt * if self.rate > 0.0 { self.rate } else { 1.0 };
        }
        if self.length > 0.0 {
            p = p.min(self.length);
        }
        p.max(0.0)
    }

    fn set_position_us(&mut self, us: i64) {
        self.position = i64_to_f64(us) / 1e6;
        self.pos_at = glib::monotonic_time();
    }

    fn apply_player_props(&mut self, p: &Props) {
        let status = p_str(p, "PlaybackStatus").to_lowercase();
        let rate = p
            .get("Rate")
            .and_then(|v| crate::dbus::v_f64(v))
            .unwrap_or(1.0);
        #[expect(clippy::float_cmp, reason = "exact change detection, not arithmetic")]
        let changed = status != self.playback_status || rate != self.rate;
        if changed {
            // Re-anchor the interpolation before the rate/status change.
            self.position = self.position_now();
            self.pos_at = glib::monotonic_time();
        }
        self.playback_status = status;
        self.playing = self.playback_status == "playing";
        self.rate = rate;
        self.volume = p_f64(p, "Volume");
        self.shuffle = p_bool(p, "Shuffle");
        self.loop_status = p_str(p, "LoopStatus");
        self.can_play = p_bool(p, "CanPlay");
        self.can_pause = p_bool(p, "CanPause");
        self.can_go_next = p_bool(p, "CanGoNext");
        self.can_go_previous = p_bool(p, "CanGoPrevious");
        self.can_seek = p_bool(p, "CanSeek");
        self.can_control = p_bool(p, "CanControl");
        if let Some(md) = p.get("Metadata") {
            self.apply_metadata(md);
        }
    }

    fn apply_metadata(&mut self, md: &Value<'_>) {
        let d = v_dict(md);
        let get = |k: &str| d.get(k).copied();
        let s = |k: &str| get(k).and_then(v_str).unwrap_or_default();
        self.track_id = s("mpris:trackid");
        self.title = s("xesam:title");
        self.artists = get("xesam:artist").map(v_strs).unwrap_or_default();
        self.artist = self.artists.join(", ");
        self.album = s("xesam:album");
        self.album_artist = get("xesam:albumArtist")
            .map(|v| v_strs(v).join(", "))
            .unwrap_or_default();
        self.url = s("xesam:url");
        let art = s("mpris:artUrl");
        if art != self.art_url {
            self.cover_art = match art.strip_prefix("file://") {
                Some(_) => glib::filename_from_uri(&art)
                    .map(|(p, _)| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                None if art.starts_with('/') => art.clone(),
                // http(s): filled in by the download in `Mpris::fetch_art`.
                None => String::new(),
            };
            self.art_url = art;
        }
        self.length = i64_to_f64(get("mpris:length").and_then(v_i64).unwrap_or(0)) / 1e6;
    }
}

impl std::fmt::Debug for Mpris {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mpris")
            .field("owners", &self.owners)
            .finish_non_exhaustive()
    }
}

pub struct Mpris {
    pub players: Signal<Vec<Player>>,
    /// Bumped every second while a player is playing.
    tick: Signal<u64>,
    ticker: RefCell<Option<SourceGuard>>,
    tasks: RefCell<HashMap<String, Vec<glib::JoinHandle<()>>>>,
    /// Every `org.mpris.MediaPlayer2.*` name → owning process. Players with
    /// several names (VLC: `vlc` and `vlc.instance<pid>`, on two
    /// connections) are listed once.
    owners: RefCell<HashMap<String, String>>,
    conn: RefCell<Option<Connection>>,
}

thread_local! {
    static INSTANCE: OnceCell<Rc<Mpris>> = const { OnceCell::new() };
}

/// Started on first use (main thread only).
pub fn get() -> Rc<Mpris> {
    INSTANCE.with(|i| {
        Rc::clone(i.get_or_init(|| {
            let this = Rc::new(Mpris {
                players: Signal::new(Vec::new()),
                tick: Signal::new(0),
                ticker: RefCell::new(None),
                tasks: RefCell::default(),
                owners: RefCell::default(),
                conn: RefCell::new(None),
            });
            let weak = Rc::downgrade(&this);
            dbus::spawn("mpris", async move { Mpris::discover(weak).await });
            this
        }))
    })
}

/// Identifies the process behind a connection (its unique name if the bus
/// won't tell).
async fn process_key(proxy: &DBusProxy<'_>, unique: &str) -> String {
    match zbus::names::BusName::try_from(unique) {
        Ok(name) => match proxy.get_connection_unix_process_id(name).await {
            Ok(pid) => format!("pid:{pid}"),
            Err(_) => unique.to_owned(),
        },
        Err(_) => unique.to_owned(),
    }
}

impl Mpris {
    async fn discover(weak: Weak<Self>) -> anyhow::Result<()> {
        let conn = dbus::session().await?;
        if let Some(this) = weak.upgrade() {
            *this.conn.borrow_mut() = Some(conn.clone());
        }
        let proxy = DBusProxy::new(&conn).await?;
        let mut owners = proxy.receive_name_owner_changed().await?;
        let mut names = proxy.list_names().await?;
        // Prefer `vlc` over `vlc.instance123`.
        names.sort_by_key(|n| n.len());
        for name in names {
            if !name.starts_with(PREFIX) {
                continue;
            }
            let Ok(owner) = proxy.get_name_owner(name.as_ref()).await else {
                continue;
            };
            let key = process_key(&proxy, owner.as_str()).await;
            if let Some(this) = weak.upgrade() {
                this.add(&conn, name.as_str(), &key);
            }
        }
        while let Some(sig) = owners.next().await {
            let Ok(a) = sig.args() else { continue };
            if !a.name.starts_with(PREFIX) {
                continue;
            }
            let key = match &*a.new_owner {
                Some(owner) => Some(process_key(&proxy, owner.as_str()).await),
                None => None,
            };
            let Some(this) = weak.upgrade() else { break };
            this.remove(&conn, a.name.as_str());
            if let Some(key) = key {
                this.add(&conn, a.name.as_str(), &key);
            }
        }
        Ok(())
    }

    fn add(self: &Rc<Self>, conn: &Connection, bus: &str, owner: &str) {
        self.owners
            .borrow_mut()
            .insert(bus.to_owned(), owner.to_owned());
        let shown = {
            let (tasks, owners) = (self.tasks.borrow(), self.owners.borrow());
            tasks.contains_key(bus)
                || tasks
                    .keys()
                    .any(|b| owners.get(b).map(String::as_str) == Some(owner))
        };
        if shown {
            return;
        }
        tracing::debug!("mpris: + {bus}");
        self.players.update(|ps| {
            ps.push(Player {
                bus_name: bus.to_owned(),
                name: bus.trim_start_matches(PREFIX).to_owned(),
                rate: 1.0,
                ..Default::default()
            });
        });
        let mut tasks = Vec::new();

        let (weak, c, b) = (Rc::downgrade(self), conn.clone(), bus.to_owned());
        tasks.push(glib::spawn_future_local(async move {
            let res = dbus::watch_props(&c, &b, PATH, PLAYER_IFACE, |p| {
                let Some(this) = weak.upgrade() else { return };
                let mut fetch_art = None;
                let mut refresh_pos = false;
                this.modify(&b, |pl| {
                    let (status, track, art) = (
                        pl.playback_status.clone(),
                        pl.track_id.clone(),
                        pl.art_url.clone(),
                    );
                    let first = pl.pos_at == 0;
                    pl.apply_player_props(p);
                    if first && let Some(pos) = p.get("Position").and_then(|v| v_i64(v)) {
                        pl.set_position_us(pos);
                    }
                    refresh_pos = !first && (status != pl.playback_status || track != pl.track_id);
                    if pl.art_url != art && pl.cover_art.is_empty() && !pl.art_url.is_empty() {
                        fetch_art = Some(pl.art_url.clone());
                    }
                });
                if refresh_pos {
                    this.refresh_position(&b);
                }
                if let Some(url) = fetch_art {
                    this.fetch_art(&b, url);
                }
            })
            .await;
            if let Err(e) = res {
                tracing::debug!("mpris {b}: {e:#}");
            }
        }));

        let (weak, c, b) = (Rc::downgrade(self), conn.clone(), bus.to_owned());
        tasks.push(glib::spawn_future_local(async move {
            let res = dbus::watch_props(&c, &b, PATH, ROOT_IFACE, |p| {
                if let Some(this) = weak.upgrade() {
                    this.modify(&b, |pl| {
                        pl.identity = p_str(p, "Identity");
                        pl.desktop_entry = p_str(p, "DesktopEntry");
                        pl.can_raise = p_bool(p, "CanRaise");
                        pl.can_quit = p_bool(p, "CanQuit");
                    });
                }
            })
            .await;
            if let Err(e) = res {
                tracing::debug!("mpris {b}: {e:#}");
            }
        }));

        let (weak, c, b) = (Rc::downgrade(self), conn.clone(), bus.to_owned());
        tasks.push(glib::spawn_future_local(async move {
            let res: anyhow::Result<()> = async {
                let proxy = zbus::Proxy::new(&c, b.clone(), PATH, PLAYER_IFACE).await?;
                let mut seeked = proxy.receive_signal("Seeked").await?;
                while let Some(msg) = seeked.next().await {
                    let Ok(us) = msg.body().deserialize::<i64>() else {
                        continue;
                    };
                    let Some(this) = weak.upgrade() else { break };
                    this.modify(&b, |pl| pl.set_position_us(us));
                }
                Ok(())
            }
            .await;
            if let Err(e) = res {
                tracing::debug!("mpris {b}: {e:#}");
            }
        }));

        self.tasks.borrow_mut().insert(bus.to_owned(), tasks);
    }

    fn remove(self: &Rc<Self>, conn: &Connection, bus: &str) {
        let owner = self.owners.borrow_mut().remove(bus);
        let Some(tasks) = self.tasks.borrow_mut().remove(bus) else {
            return;
        };
        tracing::debug!("mpris: - {bus}");
        for t in tasks {
            t.abort();
        }
        self.players.update(|ps| ps.retain(|p| p.bus_name != bus));
        self.update_ticker();
        // Show another name of the same player, if any.
        let alias = owner.and_then(|o| {
            self.owners
                .borrow()
                .iter()
                .find(|(_, v)| **v == o)
                .map(|(k, v)| (k.clone(), v.clone()))
        });
        if let Some((name, owner)) = alias {
            self.add(conn, &name, &owner);
        }
    }

    fn modify(&self, bus: &str, f: impl FnOnce(&mut Player)) {
        let mut ps = self.players.peek();
        let Some(p) = ps.iter_mut().find(|p| p.bus_name == bus) else {
            return;
        };
        f(p);
        self.players.set(ps);
        self.update_ticker();
    }

    /// Runs the 1 s position tick only while something is playing.
    fn update_ticker(&self) {
        let playing = self.players.peek().iter().any(|p| p.playing);
        let mut t = self.ticker.borrow_mut();
        if playing && t.is_none() {
            let tick = self.tick.downgrade();
            let id =
                glib::timeout_add_local(Duration::from_secs(1), move || match tick.upgrade() {
                    Some(s) => {
                        s.update(|n| *n += 1);
                        glib::ControlFlow::Continue
                    }
                    None => glib::ControlFlow::Break,
                });
            *t = Some(SourceGuard(Some(id)));
        } else if !playing {
            *t = None;
        }
    }

    /// Interpolated position (seconds) of player `bus`, updated every
    /// second while it plays and on seeks.
    pub fn position(&self, bus: &str) -> Signal<f64> {
        let (players, tick, bus) = (self.players.clone(), self.tick.clone(), bus.to_owned());
        computed(move || {
            tick.with(|_| ());
            players.with(|ps| {
                ps.iter()
                    .find(|p| p.bus_name == bus)
                    .map_or(0.0, |p| (p.position_now() * 10.0).round() / 10.0)
            })
        })
    }

    fn refresh_position(self: &Rc<Self>, bus: &str) {
        let (weak, bus) = (Rc::downgrade(self), bus.to_owned());
        dbus::spawn("mpris position", async move {
            let conn = dbus::session().await?;
            let proxy = dbus::props_proxy(&conn, &bus, PATH).await?;
            let v = proxy.get(PLAYER_IFACE.try_into()?, "Position").await?;
            if let (Some(us), Some(this)) = (v_i64(&v), weak.upgrade()) {
                this.modify(&bus, |p| p.set_position_us(us));
            }
            Ok(())
        });
    }

    /// Downloads http(s) cover art through GIO into the cache.
    fn fetch_art(self: &Rc<Self>, bus: &str, url: String) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut h);
        let path = dbus::cache_dir("mpris-art").join(format!("{:016x}", h.finish()));
        let (weak, bus, art) = (Rc::downgrade(self), bus.to_owned(), url.clone());
        let set = move |path: &std::path::Path| {
            if let Some(this) = weak.upgrade() {
                let p = path.to_string_lossy().into_owned();
                this.modify(&bus, |pl| {
                    if pl.art_url == art {
                        pl.cover_art = p;
                    }
                });
            }
        };
        if path.exists() {
            set(&path);
            return;
        }
        dbus::spawn("mpris art", async move {
            let (bytes, _) = gio::File::for_uri(&url)
                .load_contents_future()
                .await
                .map_err(|e| anyhow::anyhow!("{url}: {e}"))?;
            std::fs::write(&path, &bytes)?;
            set(&path);
            Ok(())
        });
    }
}

/// Player controls. `bus` is [`Player::bus_name`].
impl Mpris {
    fn call(
        bus: &str,
        method: &'static str,
        args: impl serde::Serialize + zbus::zvariant::DynamicType + 'static,
    ) {
        let bus = bus.to_owned();
        dbus::spawn("mpris call", async move {
            let conn = dbus::session().await?;
            conn.call_method(Some(bus.as_str()), PATH, Some(PLAYER_IFACE), method, &args)
                .await?;
            Ok(())
        });
    }

    fn set_prop(bus: &str, iface: &'static str, prop: &'static str, v: Value<'static>) {
        let bus = bus.to_owned();
        dbus::spawn("mpris set", async move {
            let conn = dbus::session().await?;
            let proxy = dbus::props_proxy(&conn, &bus, PATH).await?;
            proxy.set(iface.try_into()?, prop, v).await?;
            Ok(())
        });
    }

    pub fn play_pause(&self, bus: &str) {
        Self::call(bus, "PlayPause", ());
    }
    pub fn play(&self, bus: &str) {
        Self::call(bus, "Play", ());
    }
    pub fn pause(&self, bus: &str) {
        Self::call(bus, "Pause", ());
    }
    pub fn stop(&self, bus: &str) {
        Self::call(bus, "Stop", ());
    }
    pub fn next(&self, bus: &str) {
        Self::call(bus, "Next", ());
    }
    pub fn previous(&self, bus: &str) {
        Self::call(bus, "Previous", ());
    }
    /// Relative seek in seconds.
    pub fn seek(&self, bus: &str, offset: f64) {
        Self::call(bus, "Seek", (f64_to_i64(offset * 1e6),));
    }
    /// Absolute position in seconds.
    pub fn set_position(&self, bus: &str, secs: f64) {
        let Some(p) = self.players.peek().into_iter().find(|p| p.bus_name == bus) else {
            return;
        };
        match ObjectPath::try_from(p.track_id.clone()) {
            Ok(track) if !p.track_id.is_empty() => {
                Self::call(bus, "SetPosition", (track, f64_to_i64(secs * 1e6)));
            }
            _ => self.seek(bus, secs - p.position_now()),
        }
    }
    pub fn set_volume(&self, bus: &str, v: f64) {
        Self::set_prop(bus, PLAYER_IFACE, "Volume", Value::F64(v));
    }
    #[expect(
        clippy::fn_params_excessive_bools,
        reason = "public API called from hw-lua with a Lua boolean"
    )]
    pub fn set_shuffle(&self, bus: &str, on: bool) {
        Self::set_prop(bus, PLAYER_IFACE, "Shuffle", Value::Bool(on));
    }
    /// `None`, `Track` or `Playlist`.
    pub fn set_loop_status(&self, bus: &str, s: &str) {
        Self::set_prop(bus, PLAYER_IFACE, "LoopStatus", Value::from(s.to_owned()));
    }
    pub fn raise(&self, bus: &str) {
        let bus = bus.to_owned();
        dbus::spawn("mpris raise", async move {
            let conn = dbus::session().await?;
            conn.call_method(Some(bus.as_str()), PATH, Some(ROOT_IFACE), "Raise", &())
                .await?;
            Ok(())
        });
    }
}
