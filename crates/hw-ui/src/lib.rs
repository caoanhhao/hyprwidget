//! GTK4 + layer-shell front end: widget kinds and property setters,
//! window registry, monitors, SCSS/CSS loading and file watching.
//!
//! Language-agnostic: `hw-lua` walks Lua/JSON widget trees and drives this
//! crate through [`widgets::Prop`] values.

pub mod app;
pub mod monitors;
pub mod style;
pub mod watch;
pub mod widgets;
pub mod windows;

pub use gtk4;
pub use gtk4_layer_shell;

/// GApplication id.
pub const APP_ID: &str = "io.github.caoanhhao.hyprwidget";

/// Initializes GTK and checks for layer-shell support.
pub fn init() -> anyhow::Result<()> {
    gtk4::init()?;
    anyhow::ensure!(
        gtk4_layer_shell::is_supported(),
        "compositor does not support wlr-layer-shell (not a Wayland session?)"
    );
    Ok(())
}
