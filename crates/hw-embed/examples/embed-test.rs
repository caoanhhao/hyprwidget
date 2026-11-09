//! Dev harness for the M0.5 spike: a layer-shell window with one `EmbedView`.
//!
//! `cargo run -p hw-embed --example embed-test -- foot`
//! Size with `HW_EMBED_SIZE=WxH` (default 900x600). `kill -USR1` toggles the
//! view's visibility (frame callbacks must stop while it is hidden).

use gtk4::{glib, prelude::*};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use hw_embed::{EmbedServer, EmbedView};

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
        vec!["foot".to_string()]
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
        let server = match EmbedServer::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("embed-test: {e:#}");
                app.quit();
                return;
            }
        };
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
        let view = EmbedView::new();
        view.set_hexpand(true);
        view.set_vexpand(true);
        frame.append(&label);
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
        if let Err(e) = server.spawn(&view, &argv) {
            eprintln!("embed-test: spawn: {e:#}");
        }
        // Keep the server alive as long as the window.
        unsafe { window.set_data("hw-embed-server", server) };
        window.present();
    });
    app.run_with_args::<&str>(&[])
}
