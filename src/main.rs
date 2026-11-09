use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use hw_core::config::ConfigDir;
use hw_core::ipc::{self, Request, Response};

/// Wayland layer-shell widget framework: GTK4 + Lua.
#[derive(Parser)]
#[command(name = "hyprwidget", version, about)]
struct Cli {
    /// Config directory (default: $HYPRWIDGET_CONFIG, then
    /// $XDG_CONFIG_HOME/hyprwidget). Used by `run`, `check` and offline
    /// `option`.
    #[arg(short, long, value_name = "DIR", global = true)]
    config: Option<PathBuf>,
    /// Instance name; the socket is $XDG_RUNTIME_DIR/hyprwidget/<instance>.sock.
    #[arg(
        short,
        long,
        env = "HYPRWIDGET_INSTANCE",
        default_value = "default",
        global = true
    )]
    instance: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the daemon.
    Run,
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
    Eval { code: Vec<String> },
    /// List windows and request handlers.
    List,
    /// Validate the config (errors as file:line).
    Check,
    /// Switch theme (`none` = no theme), or list themes (current marked `*`).
    Theme { name: Option<String> },
    /// Print the merged value of an option, e.g. `decoration:rounding`.
    Option { name: String },
    /// Stop the daemon.
    Quit,
}

/// Renderer used when `GSK_RENDERER` is unset; see `docs/renderer.md`.
const DEFAULT_GSK_RENDERER: &str = "vulkan";

fn main() -> ExitCode {
    if std::env::var_os("GSK_RENDERER").is_none() {
        // SAFETY: still single-threaded; nothing has read the environment yet.
        unsafe { std::env::set_var("GSK_RENDERER", DEFAULT_GSK_RENDERER) };
    }
    let cli = Cli::parse();
    // The daemon logs at info; CLI commands only warn so their output stays clean.
    let default_level = if matches!(cli.command, Command::Run) {
        "info"
    } else {
        "warn"
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| default_level.into()),
        )
        .init();

    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("hyprwidget: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    let ipc = |cmd: &str, args: Vec<String>| -> anyhow::Result<ExitCode> {
        let resp = ipc::send(
            &cli.instance,
            &Request {
                cmd: cmd.into(),
                args,
            },
        )?;
        print_response(&resp);
        Ok(if resp.ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    };
    match &cli.command {
        Command::Run => {
            let config = ConfigDir::resolve(cli.config.clone())?;
            tracing::info!(dir = %config.root.display(), lua = %hw_lua::lua_version().map_err(|e| anyhow::anyhow!("{e}"))?, "config");
            let code = hw_lua::app::run(config, cli.instance.clone())?;
            Ok(ExitCode::from(code.clamp(0, 255) as u8))
        }
        Command::Request { name, args } => {
            let mut a = vec![name.clone()];
            a.extend(args.iter().cloned());
            ipc("request", a)
        }
        Command::Reload { what } => ipc("reload", what.iter().cloned().collect()),
        Command::Toggle { window } => ipc("toggle", vec![window.clone()]),
        Command::Inspect => ipc("inspect", vec![]),
        Command::Eval { code } => ipc("eval", vec![code.join(" ")]),
        Command::List => ipc("list", vec![]),
        Command::Quit => ipc("quit", vec![]),
        Command::Theme { name } => ipc("theme", name.iter().cloned().collect()),
        Command::Option { name } => {
            // Ask the daemon; without one, compute the merged value locally.
            let req = Request {
                cmd: "option".into(),
                args: vec![name.clone()],
            };
            match ipc::send(&cli.instance, &req) {
                Ok(resp) => {
                    print_response(&resp);
                    Ok(if resp.ok {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    })
                }
                Err(_) => {
                    let config = ConfigDir::resolve(cli.config.clone())?;
                    let v = hw_lua::app::option_offline(config, name)?;
                    println!("{}", hw_lua::app::format_option(&v));
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
        Command::Check => {
            let config = ConfigDir::resolve(cli.config.clone())?;
            let problems = hw_lua::app::check(config)?;
            if problems.is_empty() {
                println!("config ok");
                return Ok(ExitCode::SUCCESS);
            }
            for p in &problems {
                eprintln!("{p}");
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

fn print_response(resp: &Response) {
    if resp.output.is_empty() {
        return;
    }
    if resp.ok {
        println!("{}", resp.output);
    } else {
        eprintln!("{}", resp.output);
    }
}
