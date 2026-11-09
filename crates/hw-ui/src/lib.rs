//! GTK4 + layer-shell front end: windows, widget registry, builder from the
//! Lua/JSON node tree, CSS/SCSS loading.
//!
//! M0: a single hard-coded layer-shell window to prove the stack works.

use gtk4::prelude::*;
use gtk4::{gio, glib};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use hw_core::config::ConfigDir;

/// GApplication id; also the D-Bus name used for single-instance handling.
pub const APP_ID: &str = "io.github.caoanhhao.hyprwidget";
/// Layer-shell namespace of the M0 window (matchable in Hyprland layer rules).
pub const NAMESPACE: &str = "hyprwidget";

/// Runs the GTK main loop with one layer-shell window anchored to the top
/// edge. Returns the application's exit code.
pub fn run(config: ConfigDir) -> anyhow::Result<glib::ExitCode> {
    gtk4::init()?;
    anyhow::ensure!(
        gtk4_layer_shell::is_supported(),
        "compositor does not support wlr-layer-shell (not a Wayland session?)"
    );

    let app = gtk4::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| build_window(app, &config));
    // Arguments were already parsed by clap; don't let GTK see them.
    Ok(app.run_with_args::<&str>(&[]))
}

fn build_window(app: &gtk4::Application, config: &ConfigDir) {
    let window = gtk4::ApplicationWindow::new(app);
    window.init_layer_shell();
    window.set_namespace(Some(NAMESPACE));
    window.set_layer(Layer::Top);
    window.set_anchor(Edge::Top, true);
    window.set_margin(Edge::Top, 8);

    let label = gtk4::Label::new(Some(&format!("hyprwidget — {}", config.root.display())));
    label.set_margin_top(8);
    label.set_margin_bottom(8);
    label.set_margin_start(16);
    label.set_margin_end(16);
    window.set_child(Some(&label));

    window.connect_realize(|w| {
        let renderer = w.renderer().map(|r| r.type_().name().to_string());
        tracing::info!(
            renderer = renderer.as_deref().unwrap_or("?"),
            "layer-shell window realized"
        );
    });
    window.present();
}
