use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hw_core::config::ConfigDir;

/// Wayland layer-shell widget framework: GTK4 + Lua.
#[derive(Parser)]
#[command(name = "hyprwidget", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the daemon.
    Run {
        /// Config directory (default: $HYPRWIDGET_CONFIG, then
        /// $XDG_CONFIG_HOME/hyprwidget).
        #[arg(short, long, value_name = "DIR")]
        config: Option<PathBuf>,
    },
    /// Call a request handler registered with `hw.on_request`.
    Request { name: String, args: Vec<String> },
    /// Reload styles, Lua, or both.
    Reload {
        #[arg(value_parser = ["css", "lua"])]
        what: Option<String>,
    },
    /// Toggle a window's visibility.
    Toggle { window: String },
    /// Open the GTK Inspector.
    Inspect,
    /// Evaluate Lua code in the running instance.
    Eval { code: String },
    /// List windows.
    List,
    /// Validate the config.
    Check,
    /// Print the merged value of an option, e.g. `decoration:rounding`.
    Option { name: String },
}

/// Renderer used when `GSK_RENDERER` is unset; see `docs/renderer.md`.
const DEFAULT_GSK_RENDERER: &str = "vulkan";

fn main() -> ExitCode {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: still single-threaded; nothing has read the environment yet.
        unsafe { std::env::set_var("GSK_RENDERER", DEFAULT_GSK_RENDERER) };
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    match run(Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hyprwidget: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Run { config } => {
            let config = ConfigDir::resolve(config)?;
            tracing::info!(dir = %config.root.display(), lua = %hw_lua::lua_version().map_err(|e| anyhow::anyhow!("{e}"))?, "config");
            let code = hw_ui::run(config)?;
            Ok(ExitCode::from(code.get()))
        }
        _ => {
            eprintln!("hyprwidget: not implemented yet");
            Ok(ExitCode::FAILURE)
        }
    }
}
