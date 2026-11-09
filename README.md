# hyprwidget

Widget framework for Wayland (wlr-layer-shell) written in Rust — like AGS, but
without a JS engine: **GTK4** for UI, **Lua 5.5** for layout/logic, **JSON** for
config/settings, **SCSS** for style. Built for Hyprland. See [`PLAN.md`](PLAN.md)
for the design and roadmap, [`docs/`](docs/README.md) for the Lua API.

Status: milestones M0–M6 done (PLAN.md §7); `examples/demo` is a full port of an
AGS config.

## Features

- **Lua DSL** (`W.Box{…}`, `hw.state/computed/bind/poll`, `hw.With/For`,
  `hw.define`) over a native GTK4 widget builder; every prop accepts a value or a
  signal. JSON layouts with `{"$bind": "settings.x"}`.
- **Looks like your Hyprland**: gaps, borders, colors, rounding, opacity, shadow,
  blur rules and animation curves are read from the running Hyprland and follow
  it live; override them with the same option names (`hw.config{…}`,
  `config/custom/<option>.lua`), use them as SCSS variables (`$decoration-rounding`).
- **Config laid out like `~/.config/hypr`**, hot reload (Lua rebuilds windows,
  SCSS reloads in place), `hyprwidget check` with `file:line` errors.
- **Native services**: Hyprland IPC, notification daemon, MPRIS, system tray
  (SNI + DBusMenu), PipeWire audio, battery/UPower, power profiles, brightness
  (backlight/DDC), sysinfo, apps launcher, hyprsunset, Material You palette from
  the wallpaper, keystrokes.
- **Plugins** (sandboxed `_ENV`, `exec`/`network`/`fs` permissions) and **themes**.
- **Apps inside widgets**: `W.Terminal` (VTE4) for TUIs, `W.App` runs any Wayland
  app in a nested compositor (dmabuf zero-copy, syncobj, text-input-v3 for IMEs).
- AGS-compatible `settings.json`, per-monitor windows with hot-plug, CLI/IPC
  (`hyprwidget request|toggle|eval|…`), LuaLS type stubs in [`types/`](types).

## Build

Rust ≥ 1.88 and, on Arch:

```sh
pacman -S gtk4 gtk4-layer-shell lua vte4 libxkbcommon
cargo build --release
```

| Library | Used for |
|---|---|
| `gtk4` (≥ 4.18) | UI |
| `gtk4-layer-shell` (≥ 1.3) | layer surfaces |
| `lua` 5.5 (`liblua.so.5.5`) | config / plugins (same Lua as Hyprland 0.57) |
| `vte4` (`vte-2.91-gtk4`) | `W.Terminal` |
| `libxkbcommon` | `hw.service.keystrokes` |

Optional runtime tools (a service reports `available = false` without them):
`pipewire` + `wireplumber` (`pw-dump`, `wpctl`) for audio, `brightnessctl` /
`ddcutil` for brightness, `nvidia-smi` for GPU stats, `hyprsunset`, `curl`
(plugins with the `network` permission), `notify-send` (plugin error
notifications).

## Quick start

```sh
hyprwidget run -c examples/demo        # the AGS port
hyprwidget check -c examples/demo      # validate without opening windows
```

A minimal `~/.config/hyprwidget/hyprwidget.lua`:

```lua
local W = hw.widget
return function(monitor)
  return W.Window {
    name = "bar-" .. monitor.name,
    monitor = monitor,
    anchor = { "top", "left", "right" },
    exclusivity = "exclusive",
    W.CenterBox {
      start = W.Label { label = hw.service.hyprland.focused_client:map(function(c) return c and c.title or "" end) },
      ["end"] = W.Label { label = hw.poll(1000, "date +%H:%M") },
    },
  }
end
```

Config dir: `-c <dir>` > `$HYPRWIDGET_CONFIG` > `$XDG_CONFIG_HOME/hyprwidget`
(default `~/.config/hyprwidget`). More in
[docs/getting-started.md](docs/getting-started.md).

Other commands talk to the daemon over
`$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock` ([docs/cli.md](docs/cli.md)):

| Command | |
|---|---|
| `hyprwidget request <name> [args…]` | call a `hw.on_request` handler |
| `hyprwidget toggle <window>` | show/hide a window |
| `hyprwidget reload [css\|lua]` | reload styles and/or Lua |
| `hyprwidget eval '<lua>'` | run Lua in the daemon |
| `hyprwidget list` | windows and request handlers |
| `hyprwidget option <name>` | merged option value (works without a daemon) |
| `hyprwidget theme [name\|none]` | switch / list themes |
| `hyprwidget check` | validate the config, errors as `file:line` |
| `hyprwidget inspect` | open the GTK Inspector |
| `hyprwidget quit` | stop the daemon |

## Documentation

| | |
|---|---|
| [docs/README.md](docs/README.md) | index (Vietnamese) |
| [docs/getting-started.md](docs/getting-started.md) | install, config layout, hot reload, LuaLS |
| [docs/reactive.md](docs/reactive.md), [docs/widgets.md](docs/widgets.md) | Lua API |
| [docs/options.md](docs/options.md), [docs/settings.md](docs/settings.md) | Hyprland options, settings |
| [docs/services.md](docs/services.md), [docs/plugins.md](docs/plugins.md) | services, plugins and themes |
| [docs/memory.md](docs/memory.md) | memory budget measurements |

## Layout

| Path | Role |
|---|---|
| `crates/hw-core` | reactive core, config/settings JSON, IPC, Hyprland options/colors |
| `crates/hw-ui` | GTK4 + layer-shell widgets, windows, monitors, SCSS/CSS, file watch, `W.Terminal`, `W.App` |
| `crates/hw-services` | native services (Hyprland, notifd, mpris, tray, audio…) |
| `crates/hw-lua` | mlua runtime, `hw.*` API, Lua/JSON widget builder, plugins, themes, daemon |
| `crates/hw-embed` | nested compositor to embed apps as widgets |
| `src/main.rs` | `hyprwidget` binary (daemon + CLI) |
| `types/` | LuaLS stubs (`---@meta`) |
| `examples/demo` | example config (port of `~/.config/ags`) |
