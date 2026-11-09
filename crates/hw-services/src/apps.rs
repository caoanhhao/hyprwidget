//! `hw.service.apps`: desktop entries (`freedesktop-desktop-entry`), fuzzy
//! search (`nucleo-matcher`) ranked with launch frequency, launching through
//! Hyprland (`hl.dsp.exec_cmd`, so apps are Hyprland's children and get its
//! rules) and a launch history in `$XDG_CACHE_HOME/hyprwidget/apps.json`.
//!
//! ```lua
//! local apps = hw.service.apps
//! apps.list()                 -- every visible app
//! apps:query("fire", 10)      -- fuzzy, best first
//! apps:launch("firefox")      -- by id (desktop file name without .desktop)
//! ```

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use freedesktop_desktop_entry as fde;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::service::{Service, arg_f64, arg_str, cache_dir};

pub const PROPS: &[&str] = &["list", "history"];
/// Entries kept in `history`.
const HISTORY_LEN: usize = 20;

#[derive(Debug, Clone, Serialize)]
pub struct App {
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub exec: Vec<String>,
    pub terminal: bool,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub wm_class: Option<String>,
    pub path: PathBuf,
    pub frequency: u32,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Usage {
    count: u32,
    last: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    usage: HashMap<String, Usage>,
    #[serde(default)]
    history: Vec<String>,
}

struct State {
    apps: Vec<App>,
    store: Store,
    store_path: PathBuf,
    matcher: Matcher,
}

pub fn start() -> Result<Rc<Service>> {
    let store_path = cache_dir().join("apps.json");
    let store: Store = std::fs::read_to_string(&store_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let st = Rc::new(RefCell::new(State {
        apps: Vec::new(),
        store,
        store_path,
        matcher: Matcher::new(Config::DEFAULT),
    }));

    let svc = Rc::new_cyclic(|weak: &std::rc::Weak<Service>| {
        let mut svc = Service::new("apps", PROPS);
        let (s, w) = (Rc::clone(&st), std::rc::Weak::clone(weak));
        svc.method("reload", move |_| {
            reload(&s);
            if let Some(svc) = w.upgrade() {
                publish(&svc, &s.borrow());
            }
            Ok(Value::Null)
        });
        let s = Rc::clone(&st);
        svc.method("query", move |args| {
            let text = arg_str(&args, 0, "text").unwrap_or_default();
            let limit = arg_f64(&args, 1, "limit")
                .map(crate::num::f64_to_usize)
                .ok();
            Ok(serde_json::to_value(query(
                &mut s.borrow_mut(),
                &text,
                limit,
                Match::Fuzzy,
            ))?)
        });
        let s = Rc::clone(&st);
        svc.method("exact_query", move |args| {
            let text = arg_str(&args, 0, "text").unwrap_or_default();
            let limit = arg_f64(&args, 1, "limit")
                .map(crate::num::f64_to_usize)
                .ok();
            Ok(serde_json::to_value(query(
                &mut s.borrow_mut(),
                &text,
                limit,
                Match::Exact,
            ))?)
        });
        let s = Rc::clone(&st);
        svc.method("get", move |args| {
            let id = arg_str(&args, 0, "id")?;
            let st = s.borrow();
            Ok(find(&st.apps, &id)
                .map(serde_json::to_value)
                .transpose()?
                .unwrap_or(Value::Null))
        });
        let (s, w) = (Rc::clone(&st), std::rc::Weak::clone(weak));
        svc.method("launch", move |args| {
            let id = match args.first() {
                Some(Value::Object(o)) => o
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                _ => arg_str(&args, 0, "id")?,
            };
            let app = find(&s.borrow().apps, &id)
                .cloned()
                .with_context(|| format!("no app '{id}'"))?;
            launch(&app)?;
            record_launch(&mut s.borrow_mut(), &app.id);
            if let Some(svc) = w.upgrade() {
                publish(&svc, &s.borrow());
            }
            Ok(Value::Null)
        });
        svc
    });
    reload(&st);
    publish(&svc, &st.borrow());
    Ok(svc)
}

fn find<'a>(apps: &'a [App], id: &str) -> Option<&'a App> {
    let id = id.strip_suffix(".desktop").unwrap_or(id);
    apps.iter()
        .find(|a| a.id == id)
        .or_else(|| apps.iter().find(|a| a.id.eq_ignore_ascii_case(id)))
}

fn publish(svc: &Service, st: &State) {
    svc.set("list", serde_json::to_value(&st.apps).unwrap_or_default());
    let history: Vec<Value> = st
        .store
        .history
        .iter()
        .filter_map(|id| find(&st.apps, id))
        .filter_map(|a| serde_json::to_value(a).ok())
        .collect();
    svc.set("history", Value::Array(history));
}

/// Reads all desktop entries (first one wins per id, like XDG lookup).
fn reload(st: &RefCell<State>) {
    let locales = fde::get_languages_from_env();
    let desktops: Vec<String> = fde::current_desktop()
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let mut apps: Vec<App> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for path in fde::Iter::new(fde::default_paths()) {
        let Ok(e) = fde::DesktopEntry::from_path(path, Some(&locales)) else {
            continue;
        };
        if !seen.insert(e.id().to_owned()) {
            continue;
        }
        if e.type_() != Some("Application") || e.no_display() || e.hidden() || e.exec().is_none() {
            continue;
        }
        let shown = |list: Option<Vec<&str>>| {
            list.map(|l| l.iter().any(|d| desktops.contains(&d.to_ascii_lowercase())))
        };
        if shown(e.only_show_in()) == Some(false) || shown(e.not_show_in()) == Some(true) {
            continue;
        }
        let Some(name) = e.name(&locales) else {
            continue;
        };
        let st_b = st.borrow();
        let frequency = st_b.store.usage.get(e.id()).map_or(0, |u| u.count);
        apps.push(App {
            id: e.id().to_owned(),
            name: name.into_owned(),
            generic_name: e.generic_name(&locales).map(std::borrow::Cow::into_owned),
            description: e.comment(&locales).map(std::borrow::Cow::into_owned),
            icon: e.icon().map(str::to_owned),
            exec: e.parse_exec().unwrap_or_default(),
            terminal: e.terminal(),
            categories: e
                .categories()
                .unwrap_or_default()
                .into_iter()
                .filter(|c| !c.is_empty())
                .map(str::to_owned)
                .collect(),
            keywords: e
                .keywords(&locales)
                .unwrap_or_default()
                .into_iter()
                .filter(|k| !k.is_empty())
                .map(std::borrow::Cow::into_owned)
                .collect(),
            wm_class: e.startup_wm_class().map(str::to_owned),
            path: e.path.clone(),
            frequency,
        });
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    st.borrow_mut().apps = apps;
}

/// How [`query`] matches its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Match {
    Fuzzy,
    /// Case-insensitive substring.
    Exact,
}

/// Fuzzy (or substring with [`Match::Exact`]) search over name, generic name,
/// keywords and id; ties and near-ties favour often-launched apps. Empty
/// text lists apps by frequency.
fn query(st: &mut State, text: &str, limit: Option<usize>, mode: Match) -> Vec<App> {
    let text = text.trim();
    let mut scored: Vec<(u32, &App)> = if text.is_empty() {
        st.apps.iter().map(|a| (0, a)).collect()
    } else {
        let pat = if mode == Match::Exact {
            Pattern::new(
                text,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Substring,
            )
        } else {
            Pattern::parse(text, CaseMatching::Ignore, Normalization::Smart)
        };
        let mut buf = Vec::new();
        let matcher = &mut st.matcher;
        st.apps
            .iter()
            .filter_map(|a| {
                let fields = std::iter::once(a.name.as_str())
                    .chain(a.generic_name.as_deref())
                    .chain(a.keywords.iter().map(String::as_str))
                    .chain(std::iter::once(a.id.as_str()));
                let best = fields
                    .enumerate()
                    .filter_map(|(i, f)| {
                        let s = pat.score(Utf32Str::new(f, &mut buf), matcher)?;
                        // The name counts more than other fields.
                        Some(if i == 0 { s + s / 4 } else { s })
                    })
                    .max()?;
                Some((best, a))
            })
            .collect()
    };
    let bonus = |a: &App| i64::from(a.frequency.min(50) * 4);
    scored.sort_by(|(sa, a), (sb, b)| {
        (i64::from(*sb) + bonus(b))
            .cmp(&(i64::from(*sa) + bonus(a)))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    scored
        .into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .map(|(_, a)| a.clone())
        .collect()
}

fn record_launch(st: &mut State, id: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let u = st.store.usage.entry(id.to_owned()).or_default();
    u.count += 1;
    u.last = now;
    let count = u.count;
    st.store.history.retain(|h| h != id);
    st.store.history.insert(0, id.to_owned());
    st.store.history.truncate(HISTORY_LEN);
    if let Some(a) = st.apps.iter_mut().find(|a| a.id == id) {
        a.frequency = count;
    }
    if let Some(dir) = st.store_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string_pretty(&st.store)
        && let Err(e) = std::fs::write(&st.store_path, text)
    {
        tracing::warn!("saving {}: {e}", st.store_path.display());
    }
}

/// Single-quotes a word for `sh`.
fn sh_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Lua string literal (for `hl.dsp.exec_cmd("…")`).
fn lua_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Shell command line for `app` (terminal apps run in `$TERMINAL`, default
/// kitty).
pub fn command_line(app: &App) -> Option<String> {
    if app.exec.is_empty() {
        return None;
    }
    let mut words: Vec<String> = Vec::new();
    if app.terminal {
        let term = std::env::var("TERMINAL").unwrap_or_else(|_| "kitty".into());
        words.extend(term.split_whitespace().map(str::to_owned));
        words.push("-e".into());
    }
    words.extend(app.exec.iter().cloned());
    Some(
        words
            .iter()
            .map(|w| sh_quote(w))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn launch(app: &App) -> Result<()> {
    let Some(cmd) = command_line(app) else {
        bail!("'{}' has no Exec", app.id);
    };
    if hw_core::hyprland::available() {
        let reply =
            hw_core::hyprland::request(&format!("dispatch hl.dsp.exec_cmd({})", lua_quote(&cmd)))?;
        if reply.trim() == "ok" {
            return Ok(());
        }
        tracing::warn!("launching {} via Hyprland: {}", app.id, reply.trim());
    }
    // Outside Hyprland (or old dispatch syntax): detach through `setsid`.
    std::process::Command::new("setsid")
        .args(["-f", "sh", "-c", &cmd])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("launching {}", app.id))?
        .wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, freq: u32) -> App {
        App {
            id: id.into(),
            name: name.into(),
            generic_name: None,
            description: None,
            icon: None,
            exec: vec![id.into()],
            terminal: false,
            categories: vec![],
            keywords: vec![],
            wm_class: None,
            path: PathBuf::new(),
            frequency: freq,
        }
    }

    fn state(apps: Vec<App>) -> State {
        State {
            apps,
            store: Store::default(),
            store_path: std::env::temp_dir()
                .join(format!("hw-apps-test-{}.json", std::process::id())),
            matcher: Matcher::new(Config::DEFAULT),
        }
    }

    #[test]
    fn fuzzy_and_frequency() {
        let mut st = state(vec![
            app("firefox", "Firefox", 0),
            app("foot", "Foot", 0),
            app("kitty", "kitty", 0),
        ]);
        let ids = |v: Vec<App>| v.into_iter().map(|a| a.id).collect::<Vec<_>>();
        assert_eq!(ids(query(&mut st, "ffx", None, Match::Fuzzy)), ["firefox"]);
        assert_eq!(ids(query(&mut st, "kit", None, Match::Exact)), ["kitty"]);
        assert!(query(&mut st, "zzz", None, Match::Fuzzy).is_empty());
        // Empty text: everything, most used first.
        record_launch(&mut st, "kitty");
        assert_eq!(ids(query(&mut st, "", Some(1), Match::Fuzzy)), ["kitty"]);
        assert_eq!(st.store.history, ["kitty"]);
        let _ = std::fs::remove_file(&st.store_path);
    }

    #[test]
    fn quoting() {
        let mut a = app("x", "X", 0);
        a.exec = vec!["sh".into(), "-c".into(), "echo 'hi' \"there\"".into()];
        assert_eq!(
            command_line(&a).unwrap(),
            r#"sh -c 'echo '\''hi'\'' "there"'"#
        );
        assert_eq!(lua_quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }
}
