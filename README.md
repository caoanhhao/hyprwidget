# hyprwidget

Widget framework for Wayland (wlr-layer-shell) written in Rust — like AGS, but
without a JS engine: **GTK4** for UI, **Lua 5.5** for layout/logic, **JSON** for
config/settings, **SCSS** for style. Built for Hyprland. See [`PLAN.md`](PLAN.md)
for the design and roadmap.

Status: **M0** — workspace skeleton, `hyprwidget run` opens one layer-shell window.

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

## Layout

| Path | Role |
|---|---|
| `crates/hw-core` | config dir/JSON; later reactive core, settings, IPC |
| `crates/hw-ui` | GTK4 + layer-shell windows, widget builder, CSS |
| `crates/hw-services` | native services (Hyprland, notifd, mpris, tray…) |
| `crates/hw-lua` | mlua runtime, `hw.*` API |
| `crates/hw-embed` | nested compositor to embed apps as widgets |
| `src/main.rs` | `hyprwidget` binary (daemon + CLI) |
| `examples/demo` | example config (port of `~/.config/ags`) |
