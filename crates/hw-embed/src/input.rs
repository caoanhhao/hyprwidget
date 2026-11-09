//! GTK input → `wl_pointer` / `wl_keyboard` / `wl_touch`.
//!
//! Coordinates arrive in surface-tree coordinates of a root (see
//! [`SurfaceView::widget_to_tree`](crate::surface::SurfaceView)); the
//! target surface is found by hit-testing the tree.

use std::process::Command;

use smithay::backend::input::{Axis, AxisSource, ButtonState, KeyState};
use smithay::input::keyboard::{FilterResult, Keycode};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent};
use smithay::input::touch::{DownEvent, MotionEvent as TouchMotion, UpEvent};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::SERIAL_COUNTER;

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
            get(name).and_then(|v| v.get("int").and_then(|i| i.as_i64()).map(|i| i as i32))
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

impl EmbedServer {
    pub(crate) fn pointer_motion(&self, root: WlSurface, x: f64, y: f64, time: u32) {
        self.run(move |st| {
            let focus = surface_under(&root, (x, y))
                .map(|(s, (lx, ly))| (s, (lx as f64, ly as f64).into()));
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

    pub(crate) fn pointer_button(&self, _root: WlSurface, button: u32, pressed: bool, time: u32) {
        self.run(move |st| {
            let ptr = st.pointer.clone();
            ptr.button(
                st,
                &ButtonEvent {
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                    button,
                    state: if pressed {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                },
            );
            ptr.frame(st);
        });
    }

    /// `wheel`: `dx`/`dy` are notches (discrete + v120); otherwise smooth
    /// touchpad deltas. `stop` ends a kinetic touchpad scroll.
    pub(crate) fn pointer_axis(
        &self,
        _root: WlSurface,
        dx: f64,
        dy: f64,
        wheel: bool,
        stop: bool,
        time: u32,
    ) {
        self.run(move |st| {
            let mut frame = AxisFrame::new(time).source(if wheel {
                AxisSource::Wheel
            } else {
                AxisSource::Finger
            });
            if stop {
                frame = frame.stop(Axis::Horizontal).stop(Axis::Vertical);
            } else {
                for (axis, d) in [(Axis::Horizontal, dx), (Axis::Vertical, dy)] {
                    if d == 0.0 {
                        continue;
                    }
                    if wheel {
                        // One notch = 15 (libinput's degrees) / 120 v120 units.
                        frame = frame.value(axis, d * 15.0).v120(axis, (d * 120.0) as i32);
                    } else {
                        frame = frame.value(axis, d);
                    }
                }
            }
            let ptr = st.pointer.clone();
            ptr.axis(st, frame);
            ptr.frame(st);
        });
    }

    /// `keycode` is the XKB keycode GDK reports (evdev + 8).
    pub(crate) fn key(&self, root: WlSurface, keycode: u32, pressed: bool, time: u32) {
        self.run(move |st| {
            let kb = st.keyboard.clone();
            let serial = SERIAL_COUNTER.next_serial();
            if pressed && kb.current_focus().as_ref() != Some(&root) {
                kb.set_focus(st, Some(root), serial);
            }
            kb.input::<(), _>(
                st,
                Keycode::new(keycode),
                if pressed {
                    KeyState::Pressed
                } else {
                    KeyState::Released
                },
                serial,
                time,
                |_, _, _| FilterResult::Forward,
            );
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
                kb.set_focus(st, None, SERIAL_COUNTER.next_serial());
            }
        });
    }

    /// `pos = Some` with `down` → touch down, `Some` → motion, `None` → up.
    pub(crate) fn touch(
        &self,
        root: WlSurface,
        slot: i32,
        pos: Option<(f64, f64)>,
        down: bool,
        time: u32,
    ) {
        self.run(move |st| {
            let touch = st.touch.clone();
            let slot = Some(slot as u32).into();
            match pos {
                Some((x, y)) => {
                    let focus = surface_under(&root, (x, y))
                        .map(|(s, (lx, ly))| (s, (lx as f64, ly as f64).into()));
                    if down {
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
                None => touch.up(
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
