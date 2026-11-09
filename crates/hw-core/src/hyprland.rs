//! Minimal Hyprland IPC client: request socket (`.socket.sock`) for
//! options/animations/eval and event socket (`.socket2.sock`).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use crate::options::{self, INHERITED};

/// `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE`, if running under
/// Hyprland.
pub fn socket_dir() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(PathBuf::from(runtime).join("hypr").join(sig))
}

pub fn available() -> bool {
    socket_dir().is_some_and(|d| d.join(".socket.sock").exists())
}

/// Sends one request and returns the raw reply.
pub fn request(cmd: &str) -> Result<String> {
    let path = socket_dir()
        .context("not running under Hyprland")?
        .join(".socket.sock");
    let mut s =
        UnixStream::connect(&path).with_context(|| format!("connecting {}", path.display()))?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    s.write_all(cmd.as_bytes())?;
    let mut out = String::new();
    s.read_to_string(&mut out)?;
    Ok(out)
}

/// Sends several requests in one `[[BATCH]]`; replies are in order.
pub fn batch(cmds: &[String]) -> Result<Vec<String>> {
    if cmds.is_empty() {
        return Ok(Vec::new());
    }
    let reply = request(&format!("[[BATCH]]{}", cmds.join(";")))?;
    let parts: Vec<String> = reply.split("\n\n\n").map(str::to_owned).collect();
    if parts.len() != cmds.len() {
        bail!(
            "batch: expected {} replies, got {}",
            cmds.len(),
            parts.len()
        );
    }
    Ok(parts)
}

/// `getoption` for each name; `None` for options Hyprland doesn't know.
pub fn get_options(names: &[&str]) -> Result<Vec<Option<Value>>> {
    let cmds: Vec<String> = names.iter().map(|n| format!("j/getoption {n}")).collect();
    Ok(batch(&cmds)?
        .iter()
        .map(|r| serde_json::from_str::<Value>(r).ok())
        .collect())
}

/// Reads all inherited options plus animations into an options tree.
pub fn fetch_tree() -> Result<Value> {
    let names: Vec<&str> = INHERITED.iter().map(|(n, ..)| *n).collect();
    let mut tree = json!({});
    for (name, reply) in names.iter().zip(get_options(&names)?) {
        if let Some(v) = reply.as_ref().and_then(options::from_getoption) {
            options::insert(&mut tree, &options::name_to_path(name), v);
        }
    }
    let anims: Value =
        serde_json::from_str(&request("j/animations")?).context("parsing animations")?;
    let anims = options::animations_tree(&anims);
    options::deep_merge(&mut tree, &json!({ "animations": anims }));
    Ok(tree)
}

/// Runs Lua in Hyprland (`hyprctl eval`). Errors if Hyprland reports one.
pub fn eval(code: &str) -> Result<String> {
    let r = request(&format!("eval {code}"))?;
    if r.starts_with("error") {
        bail!("{r}");
    }
    Ok(r)
}

/// Spawns a thread reading `.socket2.sock`; each `event>>data` line is
/// delivered to `on_event` on the calling thread's GLib main context.
/// Reconnects if Hyprland restarts.
pub fn listen_events(on_event: impl Fn(&str, &str) + 'static) -> Result<()> {
    let path = socket_dir()
        .context("not running under Hyprland")?
        .join(".socket2.sock");
    let (tx, rx) = async_channel::unbounded::<String>();
    std::thread::Builder::new()
        .name("hypr-events".into())
        .spawn(move || {
            loop {
                if let Ok(s) = UnixStream::connect(&path) {
                    for line in BufReader::new(s).lines() {
                        let Ok(line) = line else { break };
                        if tx.send_blocking(line).is_err() {
                            return;
                        }
                    }
                }
                if tx.is_closed() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })?;
    glib::spawn_future_local(async move {
        while let Ok(line) = rx.recv().await {
            let (ev, data) = line.split_once(">>").unwrap_or((&line, ""));
            on_event(ev, data);
        }
    });
    Ok(())
}

/// Lua for Hyprland's `hl.layer_rule` making a namespace blurred the way
/// `decoration:blur:*` says (PLAN.md §3.6).
pub fn blur_rules(namespace: &str, popups: bool, ignore_alpha: Option<f64>) -> Vec<String> {
    let ns = format!("^({})$", namespace.replace('"', ""));
    let mut rules = vec![format!(
        "hl.layer_rule({{ match = {{ namespace = \"{ns}\" }}, blur = true }})"
    )];
    if popups {
        rules.push(format!(
            "hl.layer_rule({{ match = {{ namespace = \"{ns}\" }}, blur_popups = true }})"
        ));
    }
    if let Some(a) = ignore_alpha {
        rules.push(format!(
            "hl.layer_rule({{ match = {{ namespace = \"{ns}\" }}, ignore_alpha = {a} }})"
        ));
    }
    rules
}
