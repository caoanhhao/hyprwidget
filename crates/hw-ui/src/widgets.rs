//! Widget kinds, property setters, children slots and events.
//!
//! Language-agnostic: the Lua (and JSON) builder in `hw-lua` walks its
//! tables and calls into this module with [`Prop`] values.

use std::collections::BTreeMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::num::{non_negative, saturating_i32};

/// A property value coming from Lua/JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum Prop {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    List(Vec<Prop>),
    Map(BTreeMap<String, Prop>),
    Monitor(gdk::Monitor),
}

impl Prop {
    pub fn truthy(&self) -> bool {
        !matches!(self, Prop::Nil | Prop::Bool(false))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Prop::Num(n) => Some(*n),
            Prop::Str(s) => s.trim().trim_end_matches("px").parse().ok(),
            Prop::Bool(b) => Some(f64::from(u8::from(*b))),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        self.as_f64().map(|f| saturating_i32(f.round()))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Prop::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Display text for labels: numbers without a trailing `.0`.
    pub fn to_text(&self) -> String {
        match self {
            Prop::Nil => String::new(),
            Prop::Bool(b) => b.to_string(),
            Prop::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => {
                format!("{}", crate::num::saturating_i64(*n))
            }
            Prop::Num(n) => n.to_string(),
            Prop::Str(s) => s.clone(),
            Prop::List(l) => l.iter().map(Prop::to_text).collect::<Vec<_>>().join(" "),
            Prop::Map(_) => "[table]".into(),
            Prop::Monitor(m) => m.connector().map(|c| c.to_string()).unwrap_or_default(),
        }
    }

    /// Strings from a list or a whitespace/comma separated string.
    pub fn words(&self) -> Vec<String> {
        match self {
            Prop::Str(s) => s
                .split([' ', ',', '|'])
                .filter(|w| !w.is_empty())
                .map(str::to_owned)
                .collect(),
            Prop::List(l) => l.iter().flat_map(Prop::words).collect(),
            _ => Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Prop> {
        match self {
            Prop::Map(m) => m.get(key),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Window,
    Box,
    CenterBox,
    Label,
    Button,
    Image,
    Icon,
    Revealer,
    Stack,
    Scrolled,
    Entry,
    Slider,
    Switch,
    Progress,
    Overlay,
    Grid,
    FlowBox,
    MenuButton,
    Popover,
    DrawingArea,
    Calendar,
    Terminal,
    App,
    Picture,
}

impl Kind {
    pub const ALL: &[Kind] = &[
        Kind::Window,
        Kind::Box,
        Kind::CenterBox,
        Kind::Label,
        Kind::Button,
        Kind::Image,
        Kind::Icon,
        Kind::Revealer,
        Kind::Stack,
        Kind::Scrolled,
        Kind::Entry,
        Kind::Slider,
        Kind::Switch,
        Kind::Progress,
        Kind::Overlay,
        Kind::Grid,
        Kind::FlowBox,
        Kind::MenuButton,
        Kind::Popover,
        Kind::DrawingArea,
        Kind::Calendar,
        Kind::Terminal,
        Kind::App,
        Kind::Picture,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Window => "Window",
            Kind::Box => "Box",
            Kind::CenterBox => "CenterBox",
            Kind::Label => "Label",
            Kind::Button => "Button",
            Kind::Image => "Image",
            Kind::Icon => "Icon",
            Kind::Revealer => "Revealer",
            Kind::Stack => "Stack",
            Kind::Scrolled => "Scrolled",
            Kind::Entry => "Entry",
            Kind::Slider => "Slider",
            Kind::Switch => "Switch",
            Kind::Progress => "Progress",
            Kind::Overlay => "Overlay",
            Kind::Grid => "Grid",
            Kind::FlowBox => "FlowBox",
            Kind::MenuButton => "MenuButton",
            Kind::Popover => "Popover",
            Kind::DrawingArea => "DrawingArea",
            Kind::Calendar => "Calendar",
            Kind::Terminal => "Terminal",
            Kind::App => "App",
            Kind::Picture => "Picture",
        }
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Kind::ALL.iter().copied().find(|k| k.name() == s)
    }

    /// Whether positional children / `children = {}` are accepted.
    pub fn takes_children(self) -> bool {
        matches!(
            self,
            Kind::Box | Kind::Stack | Kind::Overlay | Kind::Grid | Kind::FlowBox
        )
    }

    /// Named single-child slots.
    pub fn slots(self) -> &'static [&'static str] {
        match self {
            Kind::Window | Kind::Button | Kind::Revealer | Kind::Scrolled | Kind::Popover => {
                &["child"]
            }
            Kind::CenterBox => &["start", "center", "end"],
            Kind::MenuButton => &["child", "popover"],
            Kind::Overlay => &["child"],
            _ => &[],
        }
    }
}

/// CSS class added to every widget of a kind (`hw-box`, `hw-label`…).
pub fn kind_class(kind: Kind) -> String {
    format!("hw-{}", kind.name().to_ascii_lowercase())
}

/// Creates a (non-window) widget.
pub fn create(kind: Kind) -> gtk4::Widget {
    use gtk4::Orientation::{Horizontal, Vertical};
    let w: gtk4::Widget = match kind {
        Kind::Window => panic!("use create_window for windows"),
        Kind::Box => gtk4::Box::new(Horizontal, 0).upcast(),
        Kind::CenterBox => gtk4::CenterBox::new().upcast(),
        Kind::Label => gtk4::Label::new(None).upcast(),
        Kind::Button => gtk4::Button::new().upcast(),
        Kind::Image | Kind::Icon => gtk4::Image::new().upcast(),
        Kind::Revealer => gtk4::Revealer::new().upcast(),
        Kind::Stack => gtk4::Stack::new().upcast(),
        Kind::Scrolled => gtk4::ScrolledWindow::new().upcast(),
        Kind::Entry => gtk4::Entry::new().upcast(),
        Kind::Slider => gtk4::Scale::with_range(Horizontal, 0.0, 1.0, 0.01).upcast(),
        Kind::Switch => gtk4::Switch::new().upcast(),
        Kind::Progress => gtk4::ProgressBar::new().upcast(),
        Kind::Overlay => gtk4::Overlay::new().upcast(),
        Kind::Grid => gtk4::Grid::new().upcast(),
        Kind::FlowBox => {
            let f = gtk4::FlowBox::new();
            f.set_selection_mode(gtk4::SelectionMode::None);
            f.upcast()
        }
        Kind::MenuButton => gtk4::MenuButton::new().upcast(),
        Kind::Popover => gtk4::Popover::new().upcast(),
        Kind::DrawingArea => gtk4::DrawingArea::new().upcast(),
        Kind::Calendar => gtk4::Calendar::new().upcast(),
        Kind::Terminal => crate::terminal::create(),
        Kind::App => crate::app::create(),
        Kind::Picture => gtk4::Picture::new().upcast(),
    };
    if kind == Kind::Box {
        // Orientation defaults; `vertical = true` flips it.
        cast::<gtk4::Box>(&w).set_orientation(Horizontal);
    }
    let _ = Vertical;
    // Without a theme nothing sizes these; keep them at natural height
    // instead of stretching to the row (override with `valign`).
    if matches!(kind, Kind::Switch | Kind::Progress) {
        w.set_valign(gtk4::Align::Center);
    }
    w.add_css_class(&kind_class(kind));
    w
}

/// Creates a layer-shell window (hidden until presented).
pub fn create_window(app: &gtk4::Application) -> gtk4::ApplicationWindow {
    let w = gtk4::ApplicationWindow::new(app);
    w.init_layer_shell();
    w.set_layer(Layer::Top);
    w.add_css_class("hw-window");
    w
}

/// Downcasts `w` to the GTK type of its [`Kind`].
fn cast<T: IsA<gtk4::Widget>>(w: &gtk4::Widget) -> &T {
    w.downcast_ref::<T>()
        .expect("widget kind matches its GTK type")
}

fn err(kind: Kind, name: &str, v: &Prop) -> String {
    format!("{}.{name}: invalid value {v:?}", kind.name())
}

fn align(s: &str) -> Option<gtk4::Align> {
    Some(match s {
        "start" | "left" | "top" => gtk4::Align::Start,
        "end" | "right" | "bottom" => gtk4::Align::End,
        "center" => gtk4::Align::Center,
        "fill" => gtk4::Align::Fill,
        "baseline" => gtk4::Align::Baseline,
        _ => return None,
    })
}

fn orientation(v: &Prop) -> gtk4::Orientation {
    if v.truthy() {
        gtk4::Orientation::Vertical
    } else {
        gtk4::Orientation::Horizontal
    }
}

fn policy(s: &str) -> Option<gtk4::PolicyType> {
    Some(match s {
        "never" => gtk4::PolicyType::Never,
        "auto" | "automatic" => gtk4::PolicyType::Automatic,
        "always" => gtk4::PolicyType::Always,
        "external" => gtk4::PolicyType::External,
        _ => return None,
    })
}

fn revealer_transition(s: &str) -> Option<gtk4::RevealerTransitionType> {
    use gtk4::RevealerTransitionType as T;
    Some(match s {
        "none" => T::None,
        "crossfade" | "fade" => T::Crossfade,
        "slide_right" => T::SlideRight,
        "slide_left" => T::SlideLeft,
        "slide_up" => T::SlideUp,
        "slide_down" => T::SlideDown,
        "swing_right" => T::SwingRight,
        "swing_left" => T::SwingLeft,
        "swing_up" => T::SwingUp,
        "swing_down" => T::SwingDown,
        _ => return None,
    })
}

fn stack_transition(s: &str) -> Option<gtk4::StackTransitionType> {
    use gtk4::StackTransitionType as T;
    Some(match s {
        "none" => T::None,
        "crossfade" | "fade" => T::Crossfade,
        "slide_right" => T::SlideRight,
        "slide_left" => T::SlideLeft,
        "slide_up" => T::SlideUp,
        "slide_down" => T::SlideDown,
        "slide_left_right" => T::SlideLeftRight,
        "slide_up_down" => T::SlideUpDown,
        "over_up" => T::OverUp,
        "over_down" => T::OverDown,
        "over_left" => T::OverLeft,
        "over_right" => T::OverRight,
        "rotate_left" => T::RotateLeft,
        "rotate_right" => T::RotateRight,
        _ => return None,
    })
}

/// Classes set through `class` (kept apart from GTK's own and `hw-*`).
const USER_CLASSES: &str = "hw-user-classes";

fn set_classes(w: &gtk4::Widget, v: &Prop) {
    // SAFETY: the key is only ever used with `Vec<String>`.
    let old = unsafe { w.steal_data::<Vec<String>>(USER_CLASSES) };
    for c in old.into_iter().flatten() {
        w.remove_css_class(&c);
    }
    let new = v.words();
    for c in &new {
        w.add_css_class(c);
    }
    // SAFETY: as above.
    unsafe { w.set_data(USER_CLASSES, new) };
}

/// The scroll controllers `scroll = false` took off a widget, kept to put
/// back on `scroll = true`.
const BUILTIN_SCROLL: &str = "hw-builtin-scroll";
/// Name of the controller `on_scroll` adds, which `scroll` leaves alone.
const ON_SCROLL: &str = "hw-on-scroll";

/// `scroll`: whether the wheel is handled by the widget itself.
fn set_builtin_scroll(w: &gtk4::Widget, v: &Prop) {
    if v.truthy() {
        restore_builtin_scroll(w);
    } else {
        remove_builtin_scroll(w);
    }
}

/// Puts back the scroll controllers [`remove_builtin_scroll`] took off.
fn restore_builtin_scroll(w: &gtk4::Widget) {
    // SAFETY: the key is only ever used with `Vec<gtk4::EventController>`.
    let kept = unsafe { w.steal_data::<Vec<gtk4::EventController>>(BUILTIN_SCROLL) };
    for c in kept.into_iter().flatten() {
        w.add_controller(c);
    }
}

/// Turns a widget's own wheel handling (a Scale changing its value) off, so
/// the wheel goes on to the parent, e.g. a `Scrolled`.
fn remove_builtin_scroll(w: &gtk4::Widget) {
    let list = w.observe_controllers();
    let own: Vec<gtk4::EventController> = (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gtk4::EventControllerScroll>())
        .filter(|c| c.name().as_deref() != Some(ON_SCROLL))
        .map(Cast::upcast)
        .collect();
    if own.is_empty() {
        return;
    }
    for c in &own {
        w.remove_controller(c);
    }
    // SAFETY: as above.
    let mut kept =
        unsafe { w.steal_data::<Vec<gtk4::EventController>>(BUILTIN_SCROLL) }.unwrap_or_default();
    kept.extend(own);
    // SAFETY: as above.
    unsafe { w.set_data(BUILTIN_SCROLL, kept) };
}

/// Sets `name` on a widget of `kind`. Unknown names fall back to a `GObject`
/// property of the same name (with `_` → `-`).
pub fn set_prop(w: &gtk4::Widget, kind: Kind, name: &str, v: &Prop) -> Result<(), String> {
    let bad = || err(kind, name, v);
    let num = || v.as_f64().ok_or_else(bad);
    let int = || v.as_i32().ok_or_else(bad);
    let text = || v.as_str().ok_or_else(bad);

    // Common to all widgets.
    match name {
        "class" | "class_name" => set_classes(w, v),
        "visible" if kind == Kind::Window => {
            let win = cast::<gtk4::Window>(w);
            if v.truthy() {
                crate::windows::present(win);
            } else {
                win.set_visible(false);
            }
        }
        "visible" => w.set_visible(v.truthy()),
        "tooltip" | "tooltip_text" => {
            w.set_tooltip_text(Some(v.to_text().as_str()).filter(|t| !t.is_empty()));
        }
        "tooltip_markup" => w.set_tooltip_markup(Some(&v.to_text())),
        "sensitive" => w.set_sensitive(v.truthy()),
        "hexpand" => w.set_hexpand(v.truthy()),
        "vexpand" => w.set_vexpand(v.truthy()),
        "halign" => w.set_halign(align(text()?).ok_or_else(bad)?),
        "valign" => w.set_valign(align(text()?).ok_or_else(bad)?),
        "width" | "width_request" if kind != Kind::DrawingArea => w.set_width_request(int()?),
        "height" | "height_request" if kind != Kind::DrawingArea => w.set_height_request(int()?),
        "opacity" => w.set_opacity(num()?),
        "focusable" | "can_focus" => w.set_focusable(v.truthy()),
        "can_target" => w.set_can_target(v.truthy()),
        "cursor" => w.set_cursor_from_name(v.as_str()),
        "widget_name" => w.set_widget_name(text()?),
        "margin" if kind != Kind::Window => {
            let [top, right, bottom, left] = sides(v).ok_or_else(bad)?;
            w.set_margin_top(top);
            w.set_margin_end(right);
            w.set_margin_bottom(bottom);
            w.set_margin_start(left);
        }
        _ => return set_kind_prop(w, kind, name, v),
    }
    Ok(())
}

/// Kind-specific part of [`set_prop`] for windows, layout and display widgets.
fn set_kind_prop(w: &gtk4::Widget, kind: Kind, name: &str, v: &Prop) -> Result<(), String> {
    let bad = || err(kind, name, v);
    let num = || v.as_f64().ok_or_else(bad);
    let int = || v.as_i32().ok_or_else(bad);
    let text = || v.as_str().ok_or_else(bad);

    match (kind, name) {
        (Kind::Window, _) => {
            return set_window_prop(cast(w), name, v).map_err(|()| bad());
        }
        (Kind::App, _) => return crate::app::set_prop(w, name, v),

        (Kind::Box, "vertical") => cast::<gtk4::Box>(w).set_orientation(orientation(v)),
        (Kind::Box, "spacing") => cast::<gtk4::Box>(w).set_spacing(int()?),
        (Kind::Box, "homogeneous") => cast::<gtk4::Box>(w).set_homogeneous(v.truthy()),
        (Kind::CenterBox, "vertical") => cast::<gtk4::CenterBox>(w).set_orientation(orientation(v)),

        (Kind::Label, "label" | "text") => cast::<gtk4::Label>(w).set_label(&v.to_text()),
        (Kind::Label, "markup") => cast::<gtk4::Label>(w).set_markup(&v.to_text()),
        (Kind::Label, "use_markup") => cast::<gtk4::Label>(w).set_use_markup(v.truthy()),
        (Kind::Label, "wrap") => cast::<gtk4::Label>(w).set_wrap(v.truthy()),
        (Kind::Label, "xalign") => cast::<gtk4::Label>(w).set_xalign(crate::num::to_f32(num()?)),
        (Kind::Label, "yalign") => cast::<gtk4::Label>(w).set_yalign(crate::num::to_f32(num()?)),
        (Kind::Label, "max_width_chars") => cast::<gtk4::Label>(w).set_max_width_chars(int()?),
        (Kind::Label, "width_chars") => cast::<gtk4::Label>(w).set_width_chars(int()?),
        (Kind::Label, "selectable") => cast::<gtk4::Label>(w).set_selectable(v.truthy()),
        (Kind::Label, "ellipsize") => {
            let mode = match v {
                Prop::Bool(true) => pango::EllipsizeMode::End,
                Prop::Bool(false) | Prop::Nil => pango::EllipsizeMode::None,
                Prop::Str(s) => match s.as_str() {
                    "start" => pango::EllipsizeMode::Start,
                    "middle" => pango::EllipsizeMode::Middle,
                    "end" => pango::EllipsizeMode::End,
                    "none" => pango::EllipsizeMode::None,
                    _ => return Err(bad()),
                },
                _ => return Err(bad()),
            };
            cast::<gtk4::Label>(w).set_ellipsize(mode);
        }
        (Kind::Label, "justify") => {
            let j = match text()? {
                "left" => gtk4::Justification::Left,
                "right" => gtk4::Justification::Right,
                "center" => gtk4::Justification::Center,
                "fill" => gtk4::Justification::Fill,
                _ => return Err(bad()),
            };
            cast::<gtk4::Label>(w).set_justify(j);
        }

        (Kind::Button, "label") => cast::<gtk4::Button>(w).set_label(&v.to_text()),
        (Kind::Button, "icon") => cast::<gtk4::Button>(w).set_icon_name(text()?),
        (Kind::Button, "has_frame") => cast::<gtk4::Button>(w).set_has_frame(v.truthy()),

        (Kind::Image | Kind::Icon, "icon" | "icon_name") => {
            cast::<gtk4::Image>(w).set_icon_name(v.as_str());
        }
        (Kind::Image, "file") => cast::<gtk4::Image>(w).set_from_file(v.as_str()),
        // Scaled image; `content_fit` ("cover", "contain"…) and `can_shrink`
        // go through the GObject fallback.
        (Kind::Picture, "file") => {
            cast::<gtk4::Picture>(w).set_filename(v.as_str().filter(|s| !s.is_empty()));
        }
        (Kind::Image | Kind::Icon, "pixel_size" | "size") => {
            cast::<gtk4::Image>(w).set_pixel_size(int()?);
        }

        _ => return set_container_prop(w, kind, name, v),
    }
    Ok(())
}

/// Kind-specific part of [`set_prop`] for containers.
fn set_container_prop(w: &gtk4::Widget, kind: Kind, name: &str, v: &Prop) -> Result<(), String> {
    let bad = || err(kind, name, v);
    let int = || v.as_i32().ok_or_else(bad);
    let text = || v.as_str().ok_or_else(bad);

    match (kind, name) {
        (Kind::Revealer, "reveal" | "reveal_child") => {
            cast::<gtk4::Revealer>(w).set_reveal_child(v.truthy());
        }
        (Kind::Revealer, "transition") => cast::<gtk4::Revealer>(w)
            .set_transition_type(revealer_transition(text()?).ok_or_else(bad)?),
        (Kind::Revealer, "duration" | "transition_duration") => {
            cast::<gtk4::Revealer>(w).set_transition_duration(non_negative(int()?));
        }

        (Kind::Stack, "shown" | "visible_child_name") => {
            let st = cast::<gtk4::Stack>(w);
            if st.child_by_name(text()?).is_some() {
                st.set_visible_child_name(text()?);
            }
        }
        (Kind::Stack, "transition") => {
            cast::<gtk4::Stack>(w).set_transition_type(stack_transition(text()?).ok_or_else(bad)?);
        }
        (Kind::Stack, "duration" | "transition_duration") => {
            cast::<gtk4::Stack>(w).set_transition_duration(non_negative(int()?));
        }
        (Kind::Stack, "hhomogeneous") => cast::<gtk4::Stack>(w).set_hhomogeneous(v.truthy()),
        (Kind::Stack, "vhomogeneous") => cast::<gtk4::Stack>(w).set_vhomogeneous(v.truthy()),

        (Kind::Scrolled, "hscroll" | "vscroll") => {
            let sw = cast::<gtk4::ScrolledWindow>(w);
            let p = policy(text()?).ok_or_else(bad)?;
            let (h, vp) = sw.policy();
            if name == "hscroll" {
                sw.set_policy(p, vp);
            } else {
                sw.set_policy(h, p);
            }
        }
        (Kind::Scrolled, "propagate_natural_width") => {
            cast::<gtk4::ScrolledWindow>(w).set_propagate_natural_width(v.truthy());
        }
        (Kind::Scrolled, "propagate_natural_height") => {
            cast::<gtk4::ScrolledWindow>(w).set_propagate_natural_height(v.truthy());
        }

        (Kind::Grid, "row_spacing") => cast::<gtk4::Grid>(w).set_row_spacing(non_negative(int()?)),
        (Kind::Grid, "column_spacing" | "spacing") => {
            let g = cast::<gtk4::Grid>(w);
            g.set_column_spacing(non_negative(int()?));
            if name == "spacing" {
                g.set_row_spacing(non_negative(int()?));
            }
        }
        (Kind::Grid, "homogeneous") => {
            let g = cast::<gtk4::Grid>(w);
            g.set_row_homogeneous(v.truthy());
            g.set_column_homogeneous(v.truthy());
        }
        (Kind::Grid, "columns") => {} // used by set_children

        (Kind::FlowBox, "max_per_line" | "max_children_per_line") => {
            cast::<gtk4::FlowBox>(w).set_max_children_per_line(non_negative(int()?.max(1)));
        }
        (Kind::FlowBox, "min_per_line" | "min_children_per_line") => {
            cast::<gtk4::FlowBox>(w).set_min_children_per_line(non_negative(int()?.max(1)));
        }
        (Kind::FlowBox, "homogeneous") => cast::<gtk4::FlowBox>(w).set_homogeneous(v.truthy()),
        (Kind::FlowBox, "spacing") => {
            let f = cast::<gtk4::FlowBox>(w);
            f.set_row_spacing(non_negative(int()?));
            f.set_column_spacing(non_negative(int()?));
        }
        _ => return set_control_prop(w, kind, name, v),
    }
    Ok(())
}

/// Kind-specific part of [`set_prop`] for inputs and the rest.
fn set_control_prop(w: &gtk4::Widget, kind: Kind, name: &str, v: &Prop) -> Result<(), String> {
    let bad = || err(kind, name, v);
    let num = || v.as_f64().ok_or_else(bad);
    let int = || v.as_i32().ok_or_else(bad);
    let text = || v.as_str().ok_or_else(bad);

    match (kind, name) {
        (Kind::Entry, "text") => {
            let e = cast::<gtk4::Entry>(w);
            let t = v.to_text();
            if e.text() != t {
                e.set_text(&t);
            }
        }
        // Cursor position; -1 = end of text.
        (Kind::Entry, "position" | "cursor_position") => {
            cast::<gtk4::Entry>(w).set_position(int()?);
        }
        (Kind::Entry, "placeholder" | "placeholder_text") => {
            cast::<gtk4::Entry>(w).set_placeholder_text(v.as_str());
        }
        (Kind::Entry, "password") => cast::<gtk4::Entry>(w).set_visibility(!v.truthy()),

        (Kind::Slider, "value") => {
            let sc = cast::<gtk4::Scale>(w);
            if (sc.value() - num()?).abs() > f64::EPSILON {
                sc.set_value(num()?);
            }
        }
        (Kind::Slider, "min") => {
            let sc = cast::<gtk4::Scale>(w);
            sc.set_range(num()?, sc.adjustment().upper());
        }
        (Kind::Slider, "max") => {
            let sc = cast::<gtk4::Scale>(w);
            sc.set_range(sc.adjustment().lower(), num()?);
        }
        (Kind::Slider, "step") => cast::<gtk4::Scale>(w).set_increments(num()?, num()? * 10.0),
        (Kind::Slider, "draw_value") => cast::<gtk4::Scale>(w).set_draw_value(v.truthy()),
        (Kind::Slider, "digits") => cast::<gtk4::Scale>(w).set_digits(int()?),
        (Kind::Slider, "vertical") => cast::<gtk4::Scale>(w).set_orientation(orientation(v)),
        (Kind::Slider, "inverted") => cast::<gtk4::Scale>(w).set_inverted(v.truthy()),
        (Kind::Slider, "scroll") => set_builtin_scroll(w, v),

        (Kind::Switch, "active") => {
            let sw = cast::<gtk4::Switch>(w);
            if sw.is_active() != v.truthy() {
                sw.set_active(v.truthy());
            }
        }

        (Kind::Progress, "value" | "fraction") => {
            cast::<gtk4::ProgressBar>(w).set_fraction(num()?.clamp(0.0, 1.0));
        }
        (Kind::Progress, "vertical") => {
            cast::<gtk4::ProgressBar>(w).set_orientation(orientation(v));
        }
        (Kind::Progress, "inverted") => cast::<gtk4::ProgressBar>(w).set_inverted(v.truthy()),
        (Kind::Progress, "text") => {
            let p = cast::<gtk4::ProgressBar>(w);
            p.set_text(Some(&v.to_text()));
            p.set_show_text(true);
        }

        (Kind::MenuButton, "label") => cast::<gtk4::MenuButton>(w).set_label(&v.to_text()),
        (Kind::MenuButton, "icon") => cast::<gtk4::MenuButton>(w).set_icon_name(text()?),
        (Kind::MenuButton, "always_show_arrow") => {
            cast::<gtk4::MenuButton>(w).set_always_show_arrow(v.truthy());
        }
        (Kind::MenuButton, "active") => cast::<gtk4::MenuButton>(w).set_active(v.truthy()),

        (Kind::Popover, "has_arrow") => cast::<gtk4::Popover>(w).set_has_arrow(v.truthy()),
        (Kind::Popover, "autohide") => cast::<gtk4::Popover>(w).set_autohide(v.truthy()),
        (Kind::Popover, "open") => {
            let p = cast::<gtk4::Popover>(w);
            if v.truthy() { p.popup() } else { p.popdown() }
        }

        (Kind::DrawingArea, "width" | "content_width") => {
            cast::<gtk4::DrawingArea>(w).set_content_width(int()?);
        }
        (Kind::DrawingArea, "height" | "content_height") => {
            cast::<gtk4::DrawingArea>(w).set_content_height(int()?);
        }
        (Kind::DrawingArea, "redraw") => w.queue_draw(),

        (Kind::Calendar, "date") => {
            let parts: Vec<i32> = text()?.split('-').filter_map(|p| p.parse().ok()).collect();
            let [year, month, day] = parts[..] else {
                return Err(bad());
            };
            let dt = glib::DateTime::from_local(year, month, day, 0, 0, 0.0).map_err(|_| bad())?;
            cast::<gtk4::Calendar>(w).select_day(&dt);
        }
        (Kind::Calendar, "show_heading") => cast::<gtk4::Calendar>(w).set_show_heading(v.truthy()),
        (Kind::Calendar, "show_day_names") => {
            cast::<gtk4::Calendar>(w).set_show_day_names(v.truthy());
        }
        (Kind::Calendar, "show_week_numbers") => {
            cast::<gtk4::Calendar>(w).set_show_week_numbers(v.truthy());
        }

        (Kind::Terminal, _) if crate::terminal::set_prop(w, name, v)? => {}

        _ => return set_gobject_prop(w.upcast_ref(), kind, name, v),
    }
    Ok(())
}

/// Fallback: set a `GObject` property by name.
fn set_gobject_prop(obj: &glib::Object, kind: Kind, name: &str, v: &Prop) -> Result<(), String> {
    let pname = name.replace('_', "-");
    let Some(pspec) = obj.find_property(&pname) else {
        return Err(format!("{}: unknown property '{name}'", kind.name()));
    };
    let value = prop_to_value(v, pspec.value_type()).ok_or_else(|| err(kind, name, v))?;
    obj.set_property_from_value(&pname, &value);
    Ok(())
}

/// Converts a [`Prop`] to a `glib::Value` of type `ty` (basic types only).
pub fn prop_to_value(v: &Prop, ty: glib::Type) -> Option<glib::Value> {
    use glib::prelude::ToValue;
    Some(match ty {
        t if t == glib::Type::BOOL => v.truthy().to_value(),
        t if t == glib::Type::I32 => v.as_i32()?.to_value(),
        t if t == glib::Type::U32 => non_negative(v.as_i32()?).to_value(),
        t if t == glib::Type::I64 => crate::num::saturating_i64(v.as_f64()?).to_value(),
        t if t == glib::Type::U64 => crate::num::saturating_u64(v.as_f64()?.max(0.0)).to_value(),
        t if t == glib::Type::F64 => v.as_f64()?.to_value(),
        t if t == glib::Type::F32 => crate::num::to_f32(v.as_f64()?).to_value(),
        t if t == glib::Type::STRING => v.to_text().to_value(),
        t if t.is_a(glib::Type::ENUM) => {
            let class = glib::EnumClass::with_type(t)?;
            let ev = match v {
                Prop::Str(s) => class
                    .value_by_nick(&s.replace('_', "-"))
                    .or_else(|| class.value_by_name(s))?,
                _ => class.value(v.as_i32()?)?,
            };
            ev.to_value(&class)
        }
        _ => return None,
    })
}

/// Converts a basic `glib::Value` back to a [`Prop`].
pub fn value_to_prop(v: &glib::Value) -> Prop {
    if let Ok(b) = v.get::<bool>() {
        return Prop::Bool(b);
    }
    if let Ok(s) = v.get::<Option<String>>() {
        return s.map_or(Prop::Nil, Prop::Str);
    }
    macro_rules! num {
        ($($t:ty => $to_f64:path),*) => {$(
            if let Ok(n) = v.get::<$t>() {
                return Prop::Num($to_f64(n));
            }
        )*};
    }
    num!(
        i32 => f64::from,
        u32 => f64::from,
        i64 => crate::num::i64_to_f64,
        u64 => crate::num::u64_to_f64,
        f64 => f64::from,
        f32 => f64::from
    );
    if let Some(ev) = glib::EnumValue::from_value(v) {
        return Prop::Str(ev.1.nick().replace('-', "_"));
    }
    Prop::Nil
}

/// `n`, `{top,right,bottom,left}` (missing = 0), or a CSS-style list.
pub fn sides(v: &Prop) -> Option<[i32; 4]> {
    match v {
        Prop::Num(_) | Prop::Str(_) => {
            let n: Vec<i32> = match v {
                Prop::Num(n) => vec![saturating_i32(n.round())],
                Prop::Str(s) => s
                    .split_whitespace()
                    .map(|p| p.trim_end_matches("px").parse().ok())
                    .collect::<Option<_>>()?,
                _ => unreachable!(),
            };
            from_list(&n)
        }
        Prop::List(l) => from_list(&l.iter().map(Prop::as_i32).collect::<Option<Vec<_>>>()?),
        Prop::Map(m) => {
            let g = |k: &str| m.get(k).and_then(Prop::as_i32).unwrap_or(0);
            Some([g("top"), g("right"), g("bottom"), g("left")])
        }
        _ => None,
    }
}

fn from_list(list: &[i32]) -> Option<[i32; 4]> {
    Some(match *list {
        [a] => [a; 4],
        [a, b] => [a, b, a, b],
        [a, b, c] => [a, b, c, b],
        [a, b, c, d] => [a, b, c, d],
        _ => return None,
    })
}

fn set_window_prop(w: &gtk4::Window, name: &str, v: &Prop) -> Result<(), ()> {
    match name {
        "namespace" => w.set_namespace(v.as_str()),
        "layer" => w.set_layer(match v.as_str().ok_or(())? {
            "background" => Layer::Background,
            "bottom" => Layer::Bottom,
            "top" => Layer::Top,
            "overlay" => Layer::Overlay,
            _ => return Err(()),
        }),
        "anchor" => {
            let words = v.words();
            for (edge, key) in [
                (Edge::Top, "top"),
                (Edge::Bottom, "bottom"),
                (Edge::Left, "left"),
                (Edge::Right, "right"),
            ] {
                w.set_anchor(edge, words.iter().any(|x| x == key));
            }
        }
        "exclusivity" => match v.as_str().ok_or(())? {
            "exclusive" => w.auto_exclusive_zone_enable(),
            "normal" => w.set_exclusive_zone(0),
            "ignore" => w.set_exclusive_zone(-1),
            _ => return Err(()),
        },
        "exclusive_zone" => w.set_exclusive_zone(v.as_i32().ok_or(())?),
        "margin" => {
            let [top, right, bottom, left] = sides(v).ok_or(())?;
            w.set_margin(Edge::Top, top);
            w.set_margin(Edge::Right, right);
            w.set_margin(Edge::Bottom, bottom);
            w.set_margin(Edge::Left, left);
        }
        "keymode" | "keyboard_mode" => w.set_keyboard_mode(match v.as_str().ok_or(())? {
            "none" => KeyboardMode::None,
            "exclusive" => KeyboardMode::Exclusive,
            "on_demand" | "on-demand" => KeyboardMode::OnDemand,
            _ => return Err(()),
        }),
        "monitor" => match v {
            Prop::Monitor(m) => w.set_monitor(Some(m)),
            Prop::Nil => w.set_monitor(None),
            Prop::Str(c) => w.set_monitor(find_monitor(c).as_ref()),
            Prop::Map(m) => w.set_monitor(
                m.get("connector")
                    .and_then(Prop::as_str)
                    .and_then(find_monitor)
                    .as_ref(),
            ),
            _ => return Err(()),
        },
        "default_width" => w.set_default_size(v.as_i32().ok_or(())?, w.default_height()),
        "default_height" => w.set_default_size(w.default_width(), v.as_i32().ok_or(())?),
        "title" => w.set_title(v.as_str()),
        // Registry name; handled by the caller.
        "name" => {}
        _ => return set_gobject_prop(w.upcast_ref(), Kind::Window, name, v).map_err(|_| ()),
    }
    Ok(())
}

/// Finds a connected monitor by connector name (`DP-2`).
pub fn find_monitor(connector: &str) -> Option<gdk::Monitor> {
    let display = gdk::Display::default()?;
    let list = display.monitors();
    (0..list.n_items())
        .filter_map(|i| list.item(i).and_downcast::<gdk::Monitor>())
        .find(|m| m.connector().is_some_and(|c| c == connector))
}

/// Puts `child` into the named slot of `w`.
pub fn set_slot(
    w: &gtk4::Widget,
    kind: Kind,
    slot: &str,
    child: Option<&gtk4::Widget>,
) -> Result<(), String> {
    match (kind, slot) {
        (Kind::Window, "child") => cast::<gtk4::Window>(w).set_child(child),
        (Kind::Button, "child") => cast::<gtk4::Button>(w).set_child(child),
        (Kind::Revealer, "child") => cast::<gtk4::Revealer>(w).set_child(child),
        (Kind::Scrolled, "child") => cast::<gtk4::ScrolledWindow>(w).set_child(child),
        (Kind::Popover, "child") => cast::<gtk4::Popover>(w).set_child(child),
        (Kind::Overlay, "child") => cast::<gtk4::Overlay>(w).set_child(child),
        (Kind::MenuButton, "child") => cast::<gtk4::MenuButton>(w).set_child(child),
        (Kind::MenuButton, "popover") => {
            let p = child
                .map(|c| c.clone().downcast::<gtk4::Popover>())
                .transpose()
                .map_err(|_| "MenuButton.popover must be a W.Popover".to_string())?;
            cast::<gtk4::MenuButton>(w).set_popover(p.as_ref());
        }
        (Kind::CenterBox, "start") => cast::<gtk4::CenterBox>(w).set_start_widget(child),
        (Kind::CenterBox, "center") => cast::<gtk4::CenterBox>(w).set_center_widget(child),
        (Kind::CenterBox, "end") => cast::<gtk4::CenterBox>(w).set_end_widget(child),
        _ => return Err(format!("{} has no '{slot}' slot", kind.name())),
    }
    Ok(())
}

/// A child of a multi-child container with its layout metadata.
#[derive(Debug)]
pub struct Child {
    pub widget: gtk4::Widget,
    /// Stack page name.
    pub name: Option<String>,
    /// Grid cell `(col, row, width, height)`.
    pub cell: Option<(i32, i32, i32, i32)>,
}

/// Removes all children of a multi-child container.
pub fn clear_children(w: &gtk4::Widget, kind: Kind) {
    let mut c = w.first_child();
    while let Some(child) = c {
        c = child.next_sibling();
        match kind {
            Kind::Box => cast::<gtk4::Box>(w).remove(&child),
            Kind::Grid => cast::<gtk4::Grid>(w).remove(&child),
            Kind::FlowBox => cast::<gtk4::FlowBox>(w).remove(&child),
            Kind::Overlay => {
                let o = cast::<gtk4::Overlay>(w);
                if o.child().as_ref() != Some(&child) {
                    o.remove_overlay(&child);
                }
            }
            // Stack's internal children are pages; removed below.
            _ => {}
        }
    }
    if kind == Kind::Stack {
        let st = cast::<gtk4::Stack>(w);
        while let Some(c) = st.first_child() {
            st.remove(&c);
        }
    }
}

/// Appends children to a multi-child container.
pub fn add_children(
    w: &gtk4::Widget,
    kind: Kind,
    children: Vec<Child>,
    columns: Option<i32>,
) -> Result<(), String> {
    for (i, ch) in children.into_iter().enumerate() {
        match kind {
            Kind::Box => cast::<gtk4::Box>(w).append(&ch.widget),
            Kind::FlowBox => cast::<gtk4::FlowBox>(w).append(&ch.widget),
            Kind::Overlay => cast::<gtk4::Overlay>(w).add_overlay(&ch.widget),
            Kind::Stack => {
                let st = cast::<gtk4::Stack>(w);
                let name = ch.name.unwrap_or_else(|| i.to_string());
                st.add_named(&ch.widget, Some(&name));
            }
            Kind::Grid => {
                let cols = columns.unwrap_or(1).max(1);
                let i = i32::try_from(i).map_err(|_| "Grid: too many children".to_string())?;
                let (c, r, cw, rh) = ch.cell.unwrap_or((i % cols, i / cols, 1, 1));
                cast::<gtk4::Grid>(w).attach(&ch.widget, c, r, cw, rh);
            }
            _ => return Err(format!("{} does not take children", kind.name())),
        }
    }
    Ok(())
}

/// An input event delivered to a Lua handler.
#[derive(Debug, Clone)]
pub enum Event {
    Click {
        button: u32,
    },
    Scroll {
        dx: f64,
        dy: f64,
    },
    Hover {
        x: f64,
        y: f64,
    },
    Motion {
        x: f64,
        y: f64,
    },
    Leave,
    Key {
        name: String,
        code: u32,
        mods: Vec<&'static str>,
    },
    /// `on_change` / `on_activate` with the widget's new value.
    Value(Prop),
    Destroy,
}

/// Handler: returns `true` to stop propagation (scroll/key).
pub type Handler = Rc<dyn Fn(Event) -> bool>;

fn mods(state: gdk::ModifierType) -> Vec<&'static str> {
    let mut m = Vec::new();
    for (bit, name) in [
        (gdk::ModifierType::SHIFT_MASK, "shift"),
        (gdk::ModifierType::CONTROL_MASK, "ctrl"),
        (gdk::ModifierType::ALT_MASK, "alt"),
        (gdk::ModifierType::SUPER_MASK, "super"),
    ] {
        if state.contains(bit) {
            m.push(name);
        }
    }
    m
}

/// Runs `h`; its result decides whether the event propagates.
fn dispatch(h: &Handler, e: Event) -> glib::Propagation {
    if h(e) {
        glib::Propagation::Stop
    } else {
        glib::Propagation::Proceed
    }
}

/// Event names accepted by [`connect_event`].
pub const EVENTS: &[&str] = &[
    "on_click",
    "on_scroll",
    "on_hover",
    "on_leave",
    "on_motion",
    "on_key",
    "on_change",
    "on_activate",
    "on_show",
    "on_hide",
    "on_destroy",
    "on_title",
    "on_exit",
];

/// Connects `on_*` handlers.
pub fn connect_event(w: &gtk4::Widget, kind: Kind, name: &str, h: Handler) -> Result<(), String> {
    match name {
        "on_click" => connect_click(w, h),
        "on_scroll" => {
            let c = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::BOTH_AXES);
            c.set_name(Some(ON_SCROLL));
            c.connect_scroll(move |_, dx, dy| dispatch(&h, Event::Scroll { dx, dy }));
            w.add_controller(c);
        }
        "on_hover" | "on_leave" | "on_motion" => {
            let c = gtk4::EventControllerMotion::new();
            match name {
                "on_hover" => {
                    c.connect_enter(move |_, x, y| {
                        h(Event::Hover { x, y });
                    });
                }
                "on_leave" => {
                    c.connect_leave(move |_| {
                        h(Event::Leave);
                    });
                }
                _ => {
                    c.connect_motion(move |_, x, y| {
                        h(Event::Motion { x, y });
                    });
                }
            }
            w.add_controller(c);
        }
        "on_key" => {
            let c = gtk4::EventControllerKey::new();
            c.connect_key_pressed(move |_, key, code, state| {
                let name = key.name().map(|n| n.to_string()).unwrap_or_default();
                dispatch(
                    &h,
                    Event::Key {
                        name,
                        code,
                        mods: mods(state),
                    },
                )
            });
            w.add_controller(c);
        }
        "on_change" => connect_change(w, kind, h)?,
        "on_activate" => match kind {
            Kind::Entry => {
                cast::<gtk4::Entry>(w).connect_activate(move |e| {
                    h(Event::Value(Prop::Str(e.text().into())));
                });
            }
            _ => return Err(format!("{} has no on_activate", kind.name())),
        },
        // Mapped / unmapped (a window shown or hidden, a revealed child…).
        "on_show" => {
            w.connect_map(move |_| {
                h(Event::Value(Prop::Bool(true)));
            });
        }
        "on_hide" => {
            w.connect_unmap(move |_| {
                h(Event::Value(Prop::Bool(false)));
            });
        }
        "on_destroy" => {
            w.connect_destroy(move |_| {
                h(Event::Destroy);
            });
        }
        "on_exit" if w.is::<vte4::Terminal>() => {
            crate::terminal::connect_exit(
                w,
                Rc::new(move |code| {
                    h(Event::Value(Prop::Num(code.into())));
                }),
            )?;
        }
        "on_title" | "on_exit" => return crate::app::connect_event(w, name, h),
        _ => return Err(format!("unknown event '{name}'")),
    }
    Ok(())
}

/// `on_click`: every mouse button.
fn connect_click(w: &gtk4::Widget, h: Handler) {
    if let Some(b) = w.downcast_ref::<gtk4::Button>() {
        let h2 = Rc::clone(&h);
        b.connect_clicked(move |_| {
            h2(Event::Click { button: 1 });
        });
        // Secondary/middle clicks on buttons.
        let g = gtk4::GestureClick::builder().button(0).build();
        g.connect_released(move |g, _, _, _| {
            let button = g.current_button();
            if button != 1 {
                h(Event::Click { button });
            }
        });
        w.add_controller(g);
    } else {
        let g = gtk4::GestureClick::builder().button(0).build();
        g.connect_released(move |g, _, _, _| {
            h(Event::Click {
                button: g.current_button(),
            });
        });
        w.add_controller(g);
    }
}

/// `on_change` with the widget's new value.
fn connect_change(w: &gtk4::Widget, kind: Kind, h: Handler) -> Result<(), String> {
    match kind {
        Kind::Entry => {
            cast::<gtk4::Entry>(w).connect_changed(move |e| {
                h(Event::Value(Prop::Str(e.text().into())));
            });
        }
        Kind::Slider => {
            cast::<gtk4::Scale>(w).connect_value_changed(move |s| {
                h(Event::Value(Prop::Num(s.value())));
            });
        }
        Kind::Switch => {
            cast::<gtk4::Switch>(w).connect_active_notify(move |s| {
                h(Event::Value(Prop::Bool(s.is_active())));
            });
        }
        Kind::Stack => {
            cast::<gtk4::Stack>(w).connect_visible_child_name_notify(move |s| {
                h(Event::Value(
                    s.visible_child_name()
                        .map_or(Prop::Nil, |n| Prop::Str(n.into())),
                ));
            });
        }
        Kind::Revealer => {
            cast::<gtk4::Revealer>(w).connect_child_revealed_notify(move |r| {
                h(Event::Value(Prop::Bool(r.is_child_revealed())));
            });
        }
        Kind::Calendar => {
            cast::<gtk4::Calendar>(w).connect_day_selected(move |c| {
                let d = c.date();
                let s = format!("{:04}-{:02}-{:02}", d.year(), d.month(), d.day_of_month());
                h(Event::Value(Prop::Str(s)));
            });
        }
        Kind::Popover => {
            w.connect_visible_notify(move |p| {
                h(Event::Value(Prop::Bool(p.is_visible())));
            });
        }
        Kind::MenuButton => {
            cast::<gtk4::MenuButton>(w).connect_active_notify(move |m| {
                h(Event::Value(Prop::Bool(m.is_active())));
            });
        }
        _ => return Err(format!("{} has no on_change", kind.name())),
    }
    Ok(())
}

/// `draw(cr, width, height)`.
pub type DrawFn = Rc<dyn Fn(&gtk4::cairo::Context, i32, i32)>;

/// Cairo drawing callback for `DrawingArea` (`draw = function(cr, w, h)`).
pub fn set_draw_func(w: &gtk4::Widget, f: DrawFn) -> Result<(), String> {
    let area = w
        .downcast_ref::<gtk4::DrawingArea>()
        .ok_or("draw is only valid on DrawingArea")?;
    area.set_draw_func(move |area, cr, width, height| {
        // Start from the CSS `color` so drawings follow the stylesheet.
        let fg = area.color();
        cr.set_source_rgba(
            fg.red().into(),
            fg.green().into(),
            fg.blue().into(),
            fg.alpha().into(),
        );
        f(cr, width, height);
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sides_forms() {
        assert_eq!(sides(&Prop::Num(4.0)), Some([4; 4]));
        assert_eq!(sides(&Prop::Str("1 2".into())), Some([1, 2, 1, 2]));
        let mut m = BTreeMap::new();
        m.insert("top".to_string(), Prop::Num(3.0));
        assert_eq!(sides(&Prop::Map(m)), Some([3, 0, 0, 0]));
    }

    #[test]
    fn words() {
        assert_eq!(
            Prop::Str("top left, right".into()).words(),
            ["top", "left", "right"]
        );
    }

    #[test]
    fn kind_names_roundtrip() {
        for k in Kind::ALL {
            assert_eq!(Kind::from_name(k.name()), Some(*k));
        }
    }
}
