//! Dev harness: a layer-shell window with one `EmbedView` and an entry.
//!
//! `cargo run -p hw-embed --example embed-test -- kitty`
//! Size with `HW_EMBED_SIZE=WxH` (default 900x600). `kill -USR1` toggles the
//! view's visibility (frame callbacks must stop while it is hidden).
//! `HW_EMBED_SOCKET=1` uses socket mode, `HW_EMBED_MODE=scale` scale mode,
//! `HW_EMBED_ROUNDING=16` rounded corners, `HW_EMBED_RESTART=on-exit`.

use gtk4::{glib, prelude::*};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use hw_embed::EmbedView;

fn main() -> glib::ExitCode {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: single-threaded, before GTK starts.
        unsafe { std::env::set_var("GSK_RENDERER", "vulkan") };
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let argv = if argv.is_empty() {
        vec!["kitty".to_string()]
    } else {
        argv
    };
    let (w, h) = std::env::var("HW_EMBED_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((900, 600));

    let app = gtk4::Application::builder()
        .application_id("dev.hyprwidget.EmbedTest")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let window = gtk4::ApplicationWindow::new(app);
        window.init_layer_shell();
        window.set_namespace(Some("hyprwidget-embed-test"));
        window.set_layer(Layer::Top);
        window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
        for edge in [Edge::Top, Edge::Right] {
            window.set_anchor(edge, true);
            window.set_margin(edge, 40);
        }
        window.set_default_size(w, h);

        let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        frame.add_css_class("embed-frame");
        let label = gtk4::Label::new(Some(&format!("hw-embed: {}", argv.join(" "))));
        // A plain GTK entry to compare typing / clipboard with.
        let entry = gtk4::Entry::new();
        entry.set_placeholder_text(Some("hyprwidget's own entry"));
        let view = EmbedView::new();
        let env = |k: &str| std::env::var(k).ok();
        view.set_command(argv.clone());
        view.set_keep_alive(true);
        view.set_socket(env("HW_EMBED_SOCKET").is_some());
        if let Some(m) = env("HW_EMBED_MODE").and_then(|m| m.parse().ok()) {
            view.set_mode(m);
        }
        if let Some(r) = env("HW_EMBED_RESTART").and_then(|m| m.parse().ok()) {
            view.set_restart(r);
        }
        if let Some(r) = env("HW_EMBED_ROUNDING").and_then(|m| m.parse().ok()) {
            view.set_rounding(r);
        }
        view.connect_title_changed(|_, t| tracing::info!(title = t, "title"));
        view.connect_app_id_changed(|_, t| tracing::info!(app_id = t, "app_id"));
        view.connect_exited(|_, c| tracing::info!(code = c, "exited"));
        view.set_hexpand(true);
        view.set_vexpand(true);
        frame.append(&label);
        frame.append(&entry);
        frame.append(&view);
        window.set_child(Some(&frame));

        glib_unix::unix_signal_add_local(
            libc::SIGUSR1,
            glib::clone!(
                #[weak]
                view,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    view.set_visible(!view.is_visible());
                    tracing::info!(visible = view.is_visible(), "toggled view");
                    glib::ControlFlow::Continue
                }
            ),
        );
        window.present();
    });
    app.run_with_args::<&str>(&[])
}
