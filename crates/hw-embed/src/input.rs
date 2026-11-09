//! GTK input → `wl_pointer` / `wl_keyboard` / `wl_touch`.
//!
//! Coordinates arrive in surface-tree coordinates of a root (see
//! [`SurfaceView::widget_to_tree`](crate::surface::SurfaceView)); the
//! target surface is found by hit-testing the tree.

use std::collections::HashMap;
use std::process::Command;

use smithay::backend::input::{Axis, AxisSource, ButtonState, KeyState};
use smithay::input::keyboard::{FilterResult, Keycode, xkb};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::touch::{DownEvent, MotionEvent as TouchMotion, UpEvent};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::SERIAL_COUNTER;

use crate::cast;
use crate::server::{EmbedServer, State, surface_under};

/// Keymap and repeat settings, read from Hyprland's `input:*` options so
/// embedded apps type with the same layout as everything else.
#[derive(Debug, Clone)]
pub(crate) struct KeyboardConfig {
    pub rules: String,
    pub model: String,
    pub layout: String,
    pub variant: String,
    pub options: Option<String>,
    /// Keys per second.
    pub repeat_rate: i32,
    /// Milliseconds.
    pub repeat_delay: i32,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            rules: String::new(),
            model: String::new(),
            layout: String::new(),
            variant: String::new(),
            options: None,
            repeat_rate: 25,
            repeat_delay: 600,
        }
    }
}

impl KeyboardConfig {
    pub(crate) fn from_hyprland() -> Self {
        let mut c = Self::default();
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return c;
        }
        let get = |name: &str| -> Option<serde_json::Value> {
            let out = Command::new("hyprctl")
                .args(["-j", "getoption", name])
                .output()
                .ok()?;
            serde_json::from_slice(&out.stdout).ok()
        };
        let text = |name: &str| {
            get(name)
                .and_then(|v| v.get("str").and_then(|s| s.as_str()).map(str::to_owned))
                .filter(|s| !s.is_empty() && s != "[[EMPTY]]")
        };
        let int = |name: &str| {
            get(name).and_then(|v| {
                v.get("int")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|i| i32::try_from(i).ok())
            })
        };
        c.rules = text("input:kb_rules").unwrap_or_default();
        c.model = text("input:kb_model").unwrap_or_default();
        c.layout = text("input:kb_layout").unwrap_or_default();
        c.variant = text("input:kb_variant").unwrap_or_default();
        c.options = text("input:kb_options");
        if let Some(r) = int("input:repeat_rate") {
            c.repeat_rate = r;
        }
        if let Some(d) = int("input:repeat_delay") {
            c.repeat_delay = d;
        }
        c
    }
}

/// Keysym ↔ keycode tables of the server keymap. GDK reports keycodes of
/// the device's own keymap; a device with a different one (a virtual
/// keyboard such as `wtype`, a per-device layout) is remapped by keysym.
#[derive(Default)]
pub(crate) struct KeyLookup {
    by_sym: HashMap<u32, u32>,
    syms: HashMap<u32, Vec<u32>>,
}

/// A touch point event, in tree coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Touch {
    Down(f64, f64),
    Motion(f64, f64),
    Up,
}

/// Modifiers GDK says are held (ctrl, shift, alt, logo).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent modifier flags, mirroring smithay's ModifiersState"
)]
pub(crate) struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub logo: bool,
}

impl KeyLookup {
    pub(crate) fn new(c: &KeyboardConfig) -> Self {
        let ctx = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let Some(keymap) = xkb::Keymap::new_from_names(
            &ctx,
            &c.rules,
            &c.model,
            &c.layout,
            &c.variant,
            c.options.clone(),
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        ) else {
            return Self::default();
        };
        let mut this = Self::default();
        let (min, max) = (keymap.min_keycode().raw(), keymap.max_keycode().raw());
        for kc in min..=max {
            let code = xkb::Keycode::new(kc);
            for level in 0..keymap.num_levels_for_key(code, 0) {
                for sym in keymap.key_get_syms_by_level(code, 0, level) {
                    this.by_sym.entry(sym.raw()).or_insert(kc);
                    this.syms.entry(kc).or_default().push(sym.raw());
                }
            }
        }
        this
    }

    /// `keycode` if the server keymap gives it `keyval` (or its lowercase),
    /// else the server keycode for that keysym.
    pub(crate) fn remap(&self, keycode: u32, keyval: u32, lower: u32) -> u32 {
        if keyval == 0
            || self
                .syms
                .get(&keycode)
                .is_some_and(|s| s.contains(&keyval) || s.contains(&lower))
        {
            return keycode;
        }
        self.by_sym
            .get(&keyval)
            .or_else(|| self.by_sym.get(&lower))
            .copied()
            .unwrap_or(keycode)
    }
}

impl State {
    /// Presses/releases modifier keys so the client sees `want`. Needed when
    /// the modifier was pressed outside the widget or only sent as state
    /// (virtual keyboards).
    fn sync_mods(&mut self, want: Mods, time: u32) {
        use smithay::input::keyboard::keysyms as k;
        let kb = self.keyboard.clone();
        let have = kb.modifier_state();
        for (w, h, sym) in [
            (want.ctrl, have.ctrl, k::KEY_Control_L),
            (want.shift, have.shift, k::KEY_Shift_L),
            (want.alt, have.alt, k::KEY_Alt_L),
            (want.logo, have.logo, k::KEY_Super_L),
        ] {
            if w == h {
                continue;
            }
            let Some(code) = self.key_lookup.by_sym.get(&sym).copied() else {
                continue;
            };
            let state = if w {
                KeyState::Pressed
            } else {
                KeyState::Released
            };
            kb.input::<(), _>(
                self,
                Keycode::new(code),
                state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        }
    }
}

impl EmbedServer {
    pub(crate) fn pointer_motion(&self, root: WlSurface, x: f64, y: f64, time: u32) {
        self.run(move |st| {
            let focus = surface_under(&root, (x, y))
                .map(|(s, (lx, ly))| (s, (f64::from(lx), f64::from(ly)).into()));
            st.pointer_root = Some(root);
            let ptr = st.pointer.clone();
            ptr.motion(
                st,
                focus,
                &MotionEvent {
                    location: (x, y).into(),
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            ptr.frame(st);
        });
    }

    pub(crate) fn pointer_leave(&self, root: WlSurface, time: u32) {
        self.run(move |st| {
            if st.pointer_root.as_ref() != Some(&root) || st.pointer.is_grabbed() {
                return;
            }
            st.pointer_root = None;
            let ptr = st.pointer.clone();
            let location = ptr.current_location();
            ptr.motion(
                st,
                None,
                &MotionEvent {
                    location,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            ptr.frame(st);
        });
    }

    pub(crate) fn pointer_button(
        &self,
        _root: WlSurface,
        button: u32,
        state: ButtonState,
        time: u32,
    ) {
        self.run(move |st| {
            let ptr = st.pointer.clone();
            ptr.button(
                st,
                &ButtonEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                    button,
                    state,
                },
            );
            ptr.frame(st);
        });
    }

    /// `AxisSource::Wheel`: `dx`/`dy` are notches (discrete + v120);
    /// otherwise smooth touchpad deltas.
    pub(crate) fn pointer_axis(
        &self,
        _root: WlSurface,
        dx: f64,
        dy: f64,
        source: AxisSource,
        time: u32,
    ) {
        self.run(move |st| {
            let mut frame = AxisFrame::new(time).source(source);
            for (axis, d) in [(Axis::Horizontal, dx), (Axis::Vertical, dy)] {
                if d == 0.0 {
                    continue;
                }
                if source == AxisSource::Wheel {
                    // One notch = 15 (libinput's degrees) / 120 v120 units.
                    frame = frame.value(axis, d * 15.0).v120(axis, cast::px(d * 120.0));
                } else {
                    frame = frame.value(axis, d);
                }
            }
            let ptr = st.pointer.clone();
            ptr.axis(st, frame);
            ptr.frame(st);
        });
    }

    /// Ends a kinetic touchpad scroll.
    pub(crate) fn pointer_axis_stop(&self, _root: WlSurface, time: u32) {
        self.run(move |st| {
            let frame = AxisFrame::new(time)
                .source(AxisSource::Finger)
                .stop(Axis::Horizontal)
                .stop(Axis::Vertical);
            let ptr = st.pointer.clone();
            ptr.axis(st, frame);
            ptr.frame(st);
        });
    }

    /// `keycode` is the XKB keycode GDK reports (evdev + 8); `keyval` and
    /// its lowercase form fix it up for devices with another keymap.
    pub(crate) fn key(
        &self,
        root: WlSurface,
        keycode: u32,
        keyval: (u32, u32),
        mods: Option<Mods>,
        state: KeyState,
        time: u32,
    ) {
        self.run(move |st| {
            let keycode = st.key_lookup.remap(keycode, keyval.0, keyval.1);
            let kb = st.keyboard.clone();
            if state == KeyState::Pressed && kb.current_focus().as_ref() != Some(&root) {
                kb.set_focus(st, Some(root), SERIAL_COUNTER.next_serial());
            }
            if let Some(m) = mods {
                st.sync_mods(m, time);
            }
            let serial = SERIAL_COUNTER.next_serial();
            kb.input::<(), _>(st, Keycode::new(keycode), state, serial, time, |_, _, _| {
                FilterResult::Forward
            });
        });
    }

    pub(crate) fn keyboard_enter(&self, root: WlSurface) {
        self.run(move |st| {
            let kb = st.keyboard.clone();
            if kb.current_focus().as_ref() != Some(&root) {
                kb.set_focus(st, Some(root), SERIAL_COUNTER.next_serial());
            }
        });
    }

    pub(crate) fn keyboard_leave(&self, root: WlSurface) {
        self.run(move |st| {
            let kb = st.keyboard.clone();
            if kb.current_focus().as_ref() == Some(&root) {
                st.sync_mods(Mods::default(), 0);
                kb.set_focus(st, None, SERIAL_COUNTER.next_serial());
            }
        });
    }

    pub(crate) fn touch(&self, root: WlSurface, slot: u32, event: Touch, time: u32) {
        self.run(move |st| {
            let touch = st.touch.clone();
            let slot = Some(slot).into();
            match event {
                Touch::Down(x, y) | Touch::Motion(x, y) => {
                    let focus = surface_under(&root, (x, y))
                        .map(|(s, (lx, ly))| (s, (f64::from(lx), f64::from(ly)).into()));
                    if matches!(event, Touch::Down(..)) {
                        touch.down(
                            st,
                            focus,
                            &DownEvent {
                                slot,
                                location: (x, y).into(),
                                serial: SERIAL_COUNTER.next_serial(),
                                time,
                            },
                        );
                    } else {
                        touch.motion(
                            st,
                            focus,
                            &TouchMotion {
                                slot,
                                location: (x, y).into(),
                                time,
                            },
                        );
                    }
                }
                Touch::Up => touch.up(
                    st,
                    &UpEvent {
                        slot,
                        serial: SERIAL_COUNTER.next_serial(),
                        time,
                    },
                ),
            }
            touch.frame(st);
        });
    }

    pub(crate) fn touch_cancel(&self) {
        self.run(|st: &mut State| {
            let touch = st.touch.clone();
            touch.cancel(st);
        });
    }
}
