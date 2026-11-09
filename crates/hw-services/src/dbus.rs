//! Shared plumbing for the zbus-based services (notifd, mpris, tray, upower,
//! power-profiles).
//!
//! All service logic runs as futures on the GTK main thread
//! (`glib::spawn_future_local`); zbus drives the socket on its own executor
//! thread and wakes those futures, so results land on the main thread without
//! extra channels. Object-server handlers (which run on the zbus thread) talk
//! to the main thread through `async_channel`.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;

use futures_util::FutureExt;
use futures_util::future::{LocalBoxFuture, Shared};
use zbus::Connection;
use zbus::zvariant::Value;

type ConnFuture = Shared<LocalBoxFuture<'static, Result<Connection, String>>>;

thread_local! {
    static SESSION: RefCell<Option<ConnFuture>> = const { RefCell::new(None) };
    static SYSTEM: RefCell<Option<ConnFuture>> = const { RefCell::new(None) };
}

fn shared(
    slot: &'static std::thread::LocalKey<RefCell<Option<ConnFuture>>>,
    make: fn() -> LocalBoxFuture<'static, zbus::Result<Connection>>,
) -> ConnFuture {
    slot.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(|| {
                make()
                    .map(|r| r.map_err(|e| e.to_string()))
                    .boxed_local()
                    .shared()
            })
            .clone()
    })
}

/// The shared session bus connection (main thread only).
pub async fn session() -> anyhow::Result<Connection> {
    shared(&SESSION, || Connection::session().boxed_local())
        .await
        .map_err(|e| anyhow::anyhow!("session bus: {e}"))
}

/// The shared system bus connection (main thread only).
pub async fn system() -> anyhow::Result<Connection> {
    shared(&SYSTEM, || Connection::system().boxed_local())
        .await
        .map_err(|e| anyhow::anyhow!("system bus: {e}"))
}

/// Spawns `fut` on the main loop, logging its error under `what`.
pub fn spawn(what: &'static str, fut: impl Future<Output = anyhow::Result<()>> + 'static) {
    glib::spawn_future_local(async move {
        if let Err(e) = fut.await {
            tracing::warn!("{what}: {e:#}");
        }
    });
}

/// `$XDG_CACHE_HOME/hyprwidget/<sub>` (created).
pub fn cache_dir(sub: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| glib::home_dir().join(".cache"));
    let dir = base.join("hyprwidget").join(sub);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!("creating {}: {e}", dir.display());
    }
    dir
}

/// `$XDG_RUNTIME_DIR/hyprwidget/<sub>` (created), falling back to the cache.
pub fn runtime_dir(sub: &str) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        Some(base) if base.is_absolute() => {
            let dir = base.join("hyprwidget").join(sub);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("creating {}: {e}", dir.display());
            }
            dir
        }
        _ => cache_dir(sub),
    }
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> i64 {
    glib::real_time() / 1_000_000
}

/// Strips `Value::Value` wrappers.
pub fn unwrap_variant<'a, 'v>(v: &'a Value<'v>) -> &'a Value<'v> {
    match v {
        Value::Value(inner) => unwrap_variant(inner),
        v => v,
    }
}

pub fn v_str(v: &Value) -> Option<String> {
    match unwrap_variant(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        Value::Signature(s) => Some(s.to_string()),
        _ => None,
    }
}

pub fn v_i64(v: &Value) -> Option<i64> {
    Some(match unwrap_variant(v) {
        Value::U8(n) => *n as i64,
        Value::Bool(b) => *b as i64,
        Value::I16(n) => *n as i64,
        Value::U16(n) => *n as i64,
        Value::I32(n) => *n as i64,
        Value::U32(n) => *n as i64,
        Value::I64(n) => *n,
        Value::U64(n) => *n as i64,
        Value::F64(n) => *n as i64,
        _ => return None,
    })
}

pub fn v_f64(v: &Value) -> Option<f64> {
    match unwrap_variant(v) {
        Value::F64(n) => Some(*n),
        v => v_i64(v).map(|n| n as f64),
    }
}

pub fn v_bool(v: &Value) -> Option<bool> {
    match unwrap_variant(v) {
        Value::Bool(b) => Some(*b),
        v => v_i64(v).map(|n| n != 0),
    }
}

/// Strings of an `as` (or a single `s`, as some players send for artists).
pub fn v_strs(v: &Value) -> Vec<String> {
    match unwrap_variant(v) {
        Value::Array(a) => a.iter().filter_map(v_str).collect(),
        v => v_str(v).into_iter().collect(),
    }
}

/// Bytes of an `ay`.
pub fn v_bytes(v: &Value) -> Option<Vec<u8>> {
    match unwrap_variant(v) {
        Value::Array(a) => a
            .iter()
            .map(|b| match b {
                Value::U8(b) => Some(*b),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// Writes an RGBA buffer as PNG into `dir`, named by its content hash so
/// identical images share one file. Returns the path.
pub fn save_png(
    dir: &std::path::Path,
    rgba: Vec<u8>,
    has_alpha: bool,
    width: i32,
    height: i32,
    rowstride: i32,
) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    if width <= 0 || height <= 0 || rowstride <= 0 {
        return None;
    }
    let channels = if has_alpha { 4 } else { 3 };
    // Values come off the bus: reject rows narrower than a pixel row and
    // sizes that overflow instead of handing GdkPixbuf inconsistent geometry.
    let row = (width as usize).checked_mul(channels)?;
    let min_len = (rowstride as usize)
        .checked_mul(height as usize - 1)?
        .checked_add(row)?;
    if (rowstride as usize) < row || rgba.len() < min_len {
        return None;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (width, height, rowstride, has_alpha).hash(&mut h);
    rgba.hash(&mut h);
    let path = dir.join(format!("{:016x}.png", h.finish()));
    if path.exists() {
        return Some(path);
    }
    let pb = gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(rgba),
        gdk_pixbuf::Colorspace::Rgb,
        has_alpha,
        8,
        width,
        height,
        rowstride,
    );
    match pb.savev(&path, "png", &[]) {
        Ok(()) => Some(path),
        Err(e) => {
            tracing::warn!("saving {}: {e}", path.display());
            None
        }
    }
}

pub type Props = std::collections::HashMap<String, zbus::zvariant::OwnedValue>;

/// Properties proxy for `dest` at `path`.
pub async fn props_proxy(
    conn: &Connection,
    dest: &str,
    path: &str,
) -> anyhow::Result<zbus::fdo::PropertiesProxy<'static>> {
    Ok(zbus::fdo::PropertiesProxy::builder(conn)
        .destination(dest.to_owned())?
        .path(path.to_owned())?
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await?)
}

/// Calls `on_change` with all properties of `iface`, then again after every
/// `PropertiesChanged` (invalidated properties are re-read). Returns when
/// the signal stream ends.
pub async fn watch_props(
    conn: &Connection,
    dest: &str,
    path: &str,
    iface: &str,
    mut on_change: impl FnMut(&Props),
) -> anyhow::Result<()> {
    use futures_util::StreamExt;
    let proxy = props_proxy(conn, dest, path).await?;
    let iface_name = zbus::names::InterfaceName::try_from(iface.to_owned())?;
    // Subscribe before reading so no change is lost in between.
    let mut changes = proxy.receive_properties_changed().await?;
    let mut props = proxy.get_all(iface_name.clone()).await?;
    on_change(&props);
    while let Some(sig) = changes.next().await {
        let Ok(args) = sig.args() else { continue };
        if args.interface_name.as_str() != iface {
            continue;
        }
        for (k, v) in args.changed_properties.iter() {
            if let Ok(v) = v.try_to_owned() {
                props.insert((*k).to_owned(), v);
            }
        }
        for k in args.invalidated_properties.iter() {
            match proxy.get(iface_name.clone(), k).await {
                Ok(v) => {
                    props.insert((*k).to_owned(), v);
                }
                Err(_) => {
                    props.remove(*k);
                }
            }
        }
        on_change(&props);
    }
    Ok(())
}

pub fn p_str(p: &Props, k: &str) -> String {
    p.get(k).and_then(|v| v_str(v)).unwrap_or_default()
}

pub fn p_f64(p: &Props, k: &str) -> f64 {
    p.get(k).and_then(|v| v_f64(v)).unwrap_or_default()
}

pub fn p_i64(p: &Props, k: &str) -> i64 {
    p.get(k).and_then(|v| v_i64(v)).unwrap_or_default()
}

pub fn p_bool(p: &Props, k: &str) -> bool {
    p.get(k).and_then(|v| v_bool(v)).unwrap_or_default()
}

/// String-keyed entries of an `a{sv}` (or `a{ss}`…) dict.
pub fn v_dict<'a, 'v>(v: &'a Value<'v>) -> std::collections::HashMap<String, &'a Value<'v>> {
    match unwrap_variant(v) {
        Value::Dict(d) => d
            .iter()
            .filter_map(|(k, v)| Some((v_str(k)?, unwrap_variant(v))))
            .collect(),
        _ => Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_png_rejects_bad_geometry() {
        let dir = std::env::temp_dir();
        // Rowstride narrower than a row of pixels.
        assert_eq!(save_png(&dir, vec![0; 64], true, 4, 2, 1), None);
        assert_eq!(save_png(&dir, vec![0; 64], false, 4, 2, 11), None);
        // Huge width with a tiny rowstride must not pass the length check.
        assert_eq!(save_png(&dir, vec![0; 64], true, i32::MAX, 1, 1), None);
        // Too short for the claimed size.
        assert_eq!(save_png(&dir, vec![0; 31], true, 4, 2, 16), None);
        assert_eq!(save_png(&dir, vec![0; 8], true, 0, 2, 16), None);
    }
}
