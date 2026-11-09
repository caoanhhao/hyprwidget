# hyprwidget

Widget framework for Wayland (wlr-layer-shell) written in Rust — like AGS, but
without a JS engine: **GTK4** for UI, **Lua 5.5** for layout/logic, **JSON** for
config/settings, **SCSS** for style. Built for Hyprland. See [`PLAN.md`](PLAN.md)
for the design and roadmap.

Status: **M1** — reactive core, Lua DSL + widget builder, SCSS with hot reload,
AGS-compatible settings, IPC/CLI, options inherited from Hyprland.

## Build

Needs `gtk4`, `gtk4-layer-shell` (≥ 1.3) and `lua` 5.5 (system `liblua.so.5.5`).
On Arch:

```sh
pacman -S gtk4 gtk4-layer-shell lua
cargo build --release
```

## Run

```sh
hyprwidget run -c examples/demo
```

Config dir: `-c <dir>` > `$HYPRWIDGET_CONFIG` > `$XDG_CONFIG_HOME/hyprwidget`
(default `~/.config/hyprwidget`). Renderer notes: [`docs/renderer.md`](docs/renderer.md).

Editing any `.lua`/`.json` in the config dir rebuilds the windows; `.scss`/`.css`
reloads styles in place. Other commands talk to the daemon over
`$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`:

| Command | |
|---|---|
| `hyprwidget request <name> [args…]` | call a `hw.on_request` handler |
| `hyprwidget reload [css\|lua]` | reload styles and/or Lua |
| `hyprwidget toggle <window>` | show/hide a window |
| `hyprwidget eval '<lua>'` | run Lua in the daemon |
| `hyprwidget list` | windows and request handlers |
| `hyprwidget option <name>` | merged option value (works without a daemon) |
| `hyprwidget check` | validate the config, errors as `file:line` |
| `hyprwidget inspect` | open the GTK Inspector |

## Config

Same structure as `~/.config/hypr` (PLAN.md §3.7): `hyprwidget.lua` calls
`require_all({...})` and `require_custom_dir()`; `hw.config({...})` takes
Hyprland option names and is deep-merged in load order. Values default to the
running Hyprland's (`"inherit"`), can be constants, or `function(h)` of the
Hyprland values. The merged options are available as `hw.option("decoration:rounding")`,
`hw.opt.general.gaps_out` (signals) and as SCSS variables from the generated
`hypr` partial (`@import "hypr";` → `$decoration-rounding`, `$general-gaps_out`, …,
plus `--decoration-rounding` CSS custom properties).

## Layout

| Path | Role |
|---|---|
| `crates/hw-core` | reactive core, config/settings JSON, IPC, Hyprland options/colors |
| `crates/hw-ui` | GTK4 + layer-shell widgets, windows, monitors, SCSS/CSS, file watch |
| `crates/hw-services` | native services (Hyprland, notifd, mpris, tray…) |
| `crates/hw-lua` | mlua runtime, `hw.*` API, Lua/JSON widget builder, daemon |
| `crates/hw-embed` | nested compositor to embed apps as widgets |
| `src/main.rs` | `hyprwidget` binary (daemon + CLI) |
| `examples/demo` | example config (port of `~/.config/ags`) |
