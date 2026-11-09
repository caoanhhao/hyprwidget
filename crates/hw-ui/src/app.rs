//! `W.App`: an embedded Linux app (`hw_embed::EmbedView`), PLAN.md §6.3.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4_layer_shell::{KeyboardMode, LayerShell};
use hw_core::reactive::Signal;
use hw_embed::{EmbedView, Mode, Restart};
use serde_json::Value as Json;

use crate::widgets::{Event, Handler, Prop};

/// `self.title` / `self.app_id` signals of a `W.App`.
#[derive(Clone, Debug)]
pub struct AppSignals {
    pub title: Signal<Json>,
    pub app_id: Signal<Json>,
}

const SIGNALS: &str = "hw-app-signals";

pub fn create() -> gtk4::Widget {
    let view = EmbedView::new();
    view.set_hexpand(true);
    view.set_vexpand(true);
    let signals = AppSignals {
        title: Signal::new(Json::String(String::new())),
        app_id: Signal::new(Json::String(String::new())),
    };
    let s = signals.title.clone();
    view.connect_title_changed(move |_, t| s.set(Json::String(t.to_owned())));
    let s = signals.app_id.clone();
    view.connect_app_id_changed(move |_, t| s.set(Json::String(t.to_owned())));
    // SAFETY: the key is only ever used with `AppSignals`.
    unsafe { view.set_data(SIGNALS, signals) };
    // Keyboard input needs a focusable layer surface.
    view.connect_realize(|v| {
        if let Some(win) = v.root().and_downcast::<gtk4::Window>()
            && win.is_layer_window()
            && win.keyboard_mode() == KeyboardMode::None
        {
            win.set_keyboard_mode(KeyboardMode::OnDemand);
        }
    });
    view.upcast()
}

fn view(w: &gtk4::Widget) -> Option<&EmbedView> {
    w.downcast_ref::<EmbedView>()
}

pub fn signals(w: &gtk4::Widget) -> Option<AppSignals> {
    view(w)?;
    // SAFETY: set in `create` with this type.
    let p = unsafe { w.data::<AppSignals>(SIGNALS) }?;
    // SAFETY: the data lives as long as `w`, which outlives this borrow.
    Some(unsafe { p.as_ref() }.clone())
}

pub fn pid(w: &gtk4::Widget) -> Option<u32> {
    view(w)?.pid()
}

pub fn send_close(w: &gtk4::Widget) {
    if let Some(v) = view(w) {
        v.send_close();
    }
}

pub fn restart(w: &gtk4::Widget) {
    if let Some(v) = view(w) {
        v.restart();
    }
}

pub fn focus(w: &gtk4::Widget) -> bool {
    view(w).is_some_and(EmbedView::focus)
}

/// `command = {"foot", "-e", "btop"}` or a shell string.
fn command(v: &Prop) -> Option<Vec<String>> {
    match v {
        Prop::Str(s) => Some(vec!["sh".into(), "-c".into(), s.clone()]),
        Prop::List(l) => Some(l.iter().map(Prop::to_text).collect()),
        Prop::Nil => Some(Vec::new()),
        _ => None,
    }
}

/// `nil` (natural size), `{w, h}` or `{width = w, height = h}`.
fn size(v: &Prop) -> Result<Option<(i32, i32)>, ()> {
    let pair = |w: Option<i32>, h: Option<i32>| w.zip(h).map(Some).ok_or(());
    match v {
        Prop::Nil => Ok(None),
        Prop::List(l) if l.len() == 2 => pair(l[0].as_i32(), l[1].as_i32()),
        Prop::Map(_) => pair(
            v.get("width").or_else(|| v.get("w")).and_then(Prop::as_i32),
            v.get("height")
                .or_else(|| v.get("h"))
                .and_then(Prop::as_i32),
        ),
        _ => Err(()),
    }
}

pub fn set_prop(w: &gtk4::Widget, name: &str, v: &Prop) -> Result<(), String> {
    let bad = || format!("App.{name}: invalid value {v:?}");
    let view = view(w).ok_or_else(bad)?;
    match name {
        "command" => view.set_command(command(v).ok_or_else(bad)?),
        "env" => {
            let Prop::Map(m) = v else {
                return Err(bad());
            };
            view.set_env(m.iter().map(|(k, v)| (k.clone(), v.to_text())).collect());
        }
        "size" => view.set_size(size(v).map_err(|()| bad())?),
        "mode" => view.set_mode(v.as_str().ok_or_else(bad)?.parse::<Mode>()?),
        "restart" => view.set_restart(match v {
            Prop::Bool(true) => Restart::OnExit,
            Prop::Bool(false) | Prop::Nil => Restart::Never,
            _ => v.as_str().ok_or_else(bad)?.parse()?,
        }),
        "keep_alive" => view.set_keep_alive(v.truthy()),
        "frame_when_hidden" => view.set_frame_when_hidden(v.truthy()),
        "socket" => view.set_socket(v.truthy()),
        "rounding" => view.set_rounding(match v {
            Prop::Nil | Prop::Bool(false) => 0.0,
            _ => v.as_f64().ok_or_else(bad)?,
        }),
        _ => return Err(format!("App: unknown property '{name}'")),
    }
    Ok(())
}

/// `on_title(title)`, `on_exit(code)`.
pub fn connect_event(w: &gtk4::Widget, name: &str, h: Handler) -> Result<(), String> {
    let view = view(w).ok_or_else(|| format!("'{name}' is only for W.App"))?;
    match name {
        "on_title" => view.connect_title_changed(move |_, t| {
            h(Event::Value(Prop::Str(t.to_owned())));
        }),
        "on_exit" => {
            let h = Rc::clone(&h);
            view.connect_exited(move |_, code| {
                h(Event::Value(Prop::Num(f64::from(code))));
            });
        }
        _ => return Err(format!("unknown event '{name}'")),
    }
    Ok(())
}
