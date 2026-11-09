//! `W.Terminal`: a VTE4 terminal running a command (btop, cava, a shell…)
//! inside a widget (PLAN.md §6.5).
//!
//! ```lua
//! W.Terminal {
//!   command = { "btop" },          -- list or shell string; default $SHELL
//!   cwd = "~", env = { TERM_PROGRAM = "hyprwidget" },
//!   font = "JetBrainsMono Nerd Font 11",
//!   colors = { foreground = "#ccc", background = "#111", palette = {…} },
//!   restart = "never",             -- | "on-exit" | "on-failure"
//!   on_exit = function(self, status) end,
//! }
//! ```
//!
//! The child is spawned when the widget is first realized, so `command`,
//! `cwd` and `env` must be set by then (the builder sets every prop right
//! after creation). `colors` defaults to `hw.theme.colors` (set by the Lua
//! builder).

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use vte4::prelude::*;

use crate::widgets::Prop;

/// Spawn parameters, colors and state, stored on the widget.
#[derive(Default)]
struct Term {
    argv: Vec<String>,
    cwd: Option<String>,
    env: Vec<String>,
    restart: Restart,
    /// The child is running (or being spawned).
    running: bool,
    on_exit: Vec<Rc<dyn Fn(i32)>>,
    /// Last `colors` / `transparent`, re-applied when either changes.
    colors: Option<Prop>,
    transparent: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Restart {
    #[default]
    Never,
    OnExit,
    OnFailure,
}

type State = Rc<RefCell<Term>>;

const DATA_KEY: &str = "hw-terminal";

fn state(t: &vte4::Terminal) -> State {
    // SAFETY: the key is only ever used with `State`.
    unsafe {
        if let Some(p) = t.data::<State>(DATA_KEY) {
            return p.as_ref().clone();
        }
        let s = State::default();
        t.set_data(DATA_KEY, s.clone());
        s
    }
}

/// Creates the terminal widget; the child starts on first realize.
pub fn create() -> gtk4::Widget {
    let t = vte4::Terminal::new();
    t.set_hexpand(true);
    t.set_vexpand(true);
    t.set_scrollback_lines(1000);
    t.set_audible_bell(false);
    let st = state(&t);
    t.connect_realize(|t| {
        if !state(t).borrow().running {
            spawn(t);
        }
    });
    t.connect_child_exited(move |t, status| {
        let (restart, handlers) = {
            let mut s = st.borrow_mut();
            s.running = false;
            (s.restart, s.on_exit.clone())
        };
        let code = exit_code(status);
        for h in handlers {
            h(code);
        }
        let again = match restart {
            Restart::Never => false,
            Restart::OnExit => true,
            Restart::OnFailure => code != 0,
        };
        if again {
            // Don't spin on a command that dies immediately.
            let weak = t.downgrade();
            glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                if let Some(t) = weak.upgrade()
                    && t.is_realized()
                {
                    spawn(&t);
                }
            });
        }
    });
    t.upcast()
}

/// `waitpid` status → exit code (128 + signal when killed).
fn exit_code(status: i32) -> i32 {
    if status & 0x7f == 0 {
        (status >> 8) & 0xff
    } else {
        128 + (status & 0x7f)
    }
}

fn spawn(t: &vte4::Terminal) {
    let st = state(t);
    let (argv, cwd, env) = {
        let mut s = st.borrow_mut();
        s.running = true;
        let argv = if s.argv.is_empty() {
            vec![std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())]
        } else {
            s.argv.clone()
        };
        (argv, s.cwd.clone(), s.env.clone())
    };
    let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();
    let env_ref: Vec<&str> = env.iter().map(String::as_str).collect();
    let weak = t.downgrade();
    let cmd = argv.join(" ");
    t.spawn_async(
        vte4::PtyFlags::DEFAULT,
        cwd.as_deref(),
        &argv_ref,
        &env_ref,
        glib::SpawnFlags::SEARCH_PATH,
        || {},
        -1,
        gtk4::gio::Cancellable::NONE,
        move |res| match res {
            Ok(pid) => tracing::debug!(%cmd, pid = pid.0, "terminal child started"),
            Err(e) => {
                tracing::error!("W.Terminal {cmd}: {e}");
                if let Some(t) = weak.upgrade() {
                    let st = state(&t);
                    st.borrow_mut().running = false;
                    let handlers = st.borrow().on_exit.clone();
                    for h in handlers {
                        h(127);
                    }
                }
            }
        },
    );
}

/// `on_exit(self, status)`: called with the child's exit code.
pub fn connect_exit(w: &gtk4::Widget, f: Rc<dyn Fn(i32)>) -> Result<(), String> {
    let t = w
        .downcast_ref::<vte4::Terminal>()
        .ok_or("on_exit is only valid on Terminal")?;
    state(t).borrow_mut().on_exit.push(f);
    Ok(())
}

fn expand_home(p: &str) -> String {
    match p.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", std::env::var("HOME").unwrap_or_default())
        }
        _ => p.to_owned(),
    }
}

fn rgba(v: &Prop) -> Option<gdk::RGBA> {
    let s = v.as_str()?;
    if let Ok(c) = gdk::RGBA::parse(s) {
        return Some(c);
    }
    // Hyprland formats (`rgb(…)`, `0xAARRGGBB`) via hw-core.
    let css = hw_core::color::to_css(s).ok()?;
    gdk::RGBA::parse(css.as_str()).ok()
}

/// Applies `colors = { foreground, background, cursor, palette = {…} }`;
/// `color0`..`color15` work too (the shape of `hw.theme.colors`).
fn set_colors(t: &vte4::Terminal, v: &Prop, transparent: bool) -> Result<(), String> {
    let Prop::Map(m) = v else {
        return match v {
            Prop::Nil => Ok(()),
            _ => Err("Terminal: colors must be a table".into()),
        };
    };
    let fg = m.get("foreground").and_then(rgba);
    let mut bg = m.get("background").and_then(rgba);
    if transparent && let Some(b) = &mut bg {
        b.set_alpha(0.0);
    }
    let mut palette: Vec<gdk::RGBA> = match m.get("palette") {
        Some(Prop::List(l)) => l.iter().filter_map(rgba).collect(),
        _ => Vec::new(),
    };
    if palette.is_empty() {
        palette = (0..16)
            .map_while(|i| m.get(&format!("color{i}")).and_then(rgba))
            .collect();
    }
    // VTE accepts 0, 8, 16, 232 or 256 entries.
    palette.truncate(if palette.len() >= 16 {
        16
    } else if palette.len() >= 8 {
        8
    } else {
        0
    });
    let refs: Vec<&gdk::RGBA> = palette.iter().collect();
    t.set_colors(fg.as_ref(), bg.as_ref(), &refs);
    if let Some(c) = m.get("cursor").and_then(rgba) {
        t.set_color_cursor(Some(&c));
    }
    Ok(())
}

fn argv(v: &Prop) -> Result<Vec<String>, String> {
    match v {
        Prop::Nil => Ok(Vec::new()),
        Prop::Str(s) => glib::shell_parse_argv(s)
            .map(|a| {
                a.into_iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect()
            })
            .map_err(|e| format!("Terminal: command: {e}")),
        Prop::List(l) => Ok(l.iter().map(Prop::to_text).collect()),
        _ => Err("Terminal: command must be a list or a string".into()),
    }
}

/// Terminal-specific properties; `Ok(false)` = not handled here.
pub fn set_prop(w: &gtk4::Widget, name: &str, v: &Prop) -> Result<bool, String> {
    let t = w.downcast_ref::<vte4::Terminal>().ok_or("not a Terminal")?;
    let st = state(t);
    match name {
        "command" | "argv" => st.borrow_mut().argv = argv(v)?,
        "cwd" => st.borrow_mut().cwd = v.as_str().map(expand_home),
        "env" => {
            st.borrow_mut().env = match v {
                Prop::Map(m) => m
                    .iter()
                    .map(|(k, v)| format!("{k}={}", v.to_text()))
                    .collect(),
                Prop::List(l) => l.iter().map(Prop::to_text).collect(),
                Prop::Nil => Vec::new(),
                _ => return Err("Terminal: env must be a table".into()),
            }
        }
        "restart" => {
            st.borrow_mut().restart = match v.as_str().unwrap_or("never") {
                "never" => Restart::Never,
                "on-exit" | "on_exit" | "always" => Restart::OnExit,
                "on-failure" | "on_failure" => Restart::OnFailure,
                other => return Err(format!("Terminal: unknown restart mode '{other}'")),
            }
        }
        "font" => {
            let desc = v.as_str().map(pango::FontDescription::from_string);
            t.set_font(desc.as_ref());
        }
        "font_scale" => {
            t.set_font_scale(v.as_f64().ok_or("Terminal: font_scale must be a number")?)
        }
        "colors" | "transparent" => {
            let (colors, transparent) = {
                let mut s = st.borrow_mut();
                if name == "colors" {
                    s.colors = Some(v.clone());
                } else {
                    s.transparent = v.truthy();
                }
                (s.colors.clone(), s.transparent)
            };
            if let Some(c) = colors {
                set_colors(t, &c, transparent)?;
            }
        }
        "scrollback" => {
            t.set_scrollback_lines(v.as_f64().ok_or("Terminal: scrollback must be a number")? as _)
        }
        "size" => {
            let (c, r) = match v {
                Prop::List(l) if l.len() == 2 => (l[0].as_f64(), l[1].as_f64()),
                Prop::Map(m) => (
                    m.get("cols").and_then(Prop::as_f64),
                    m.get("rows").and_then(Prop::as_f64),
                ),
                _ => (None, None),
            };
            let (Some(c), Some(r)) = (c, r) else {
                return Err("Terminal: size must be { cols, rows }".into());
            };
            t.set_size(c as _, r as _);
        }
        "cursor_blink" => t.set_cursor_blink_mode(if v.truthy() {
            vte4::CursorBlinkMode::On
        } else {
            vte4::CursorBlinkMode::Off
        }),
        "input" => t.set_input_enabled(v.truthy()),
        "bold_is_bright" => t.set_bold_is_bright(v.truthy()),
        // `self:set("send", "q")`: write to the child's stdin.
        "send" | "feed_child" => t.feed_child(v.to_text().as_bytes()),
        "restart_now" => {
            if v.truthy() && !st.borrow().running {
                spawn(t);
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(exit_code(0), 0);
        assert_eq!(exit_code(3 << 8), 3);
        assert_eq!(exit_code(9), 137);
    }

    #[test]
    fn argv_forms() {
        assert_eq!(
            argv(&Prop::Str("btop --utf-force".into())).unwrap(),
            ["btop", "--utf-force"]
        );
        assert_eq!(
            argv(&Prop::List(vec![Prop::Str("cava".into())])).unwrap(),
            ["cava"]
        );
        assert!(argv(&Prop::Num(1.0)).is_err());
    }
}
