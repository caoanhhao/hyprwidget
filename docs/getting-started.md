# Getting started

## Install

System dependencies (Arch): `gtk4`, `gtk4-layer-shell` (≥ 1.3), `lua` (5.5, links
`liblua.so.5.5`), `vte4` (for `W.Terminal`), `libxkbcommon`. At runtime some
services call external tools: `pipewire` + `wireplumber` (`pw-dump`, `wpctl`),
`brightnessctl` / `ddcutil` (brightness), `nvidia-smi` (GPU), `curl` (plugins with
the `network` permission). When a tool is missing, that service reports
`available = false` and everything else keeps running.

```sh
pacman -S gtk4 gtk4-layer-shell lua vte4 libxkbcommon pipewire wireplumber
cargo install --path . --locked     # installs the binary into ~/.cargo/bin
```

`cargo build --release` only builds `target/release/hyprwidget`; it does not put
it in `PATH`. Use `cargo install` (with `~/.cargo/bin` in `PATH`) to get the
`hyprwidget` command, and re-run it after every code update.

## Run

```sh
hyprwidget run                       # default config directory
hyprwidget run -c examples/demo      # another config (while developing)
hyprwidget -i test run -c ~/x        # a second instance with its own socket
```

Config directory, in order of precedence:

1. `-c <dir>` / `--config <dir>`
2. `$HYPRWIDGET_CONFIG`
3. `$XDG_CONFIG_HOME/hyprwidget`
4. `~/.config/hyprwidget`

Each instance (`-i <name>`, default `default`, or `$HYPRWIDGET_INSTANCE`) has its
own IPC socket `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock` and its own directory
of generated SCSS partials `$XDG_RUNTIME_DIR/hyprwidget/<instance>/`. The other
commands (`toggle`, `request`, `eval`…) talk to the daemon through that socket —
see [cli.md](cli.md).

Check a config without opening any window (errors are printed as
`file:line: message`, like Hyprland's config errors):

```sh
hyprwidget check -c examples/demo
```

## Config directory layout

Like `~/.config/hypr`: one entry file loads the other files in order, and a
`config/custom/` directory overrides individual options.

```
~/.config/hyprwidget/
├── hyprwidget.lua          -- entry
├── config.json             -- (optional) settings file, style, theme, plugins, options as JSON
├── config/
│   ├── general.lua         -- hw.config({ general = { … } })
│   ├── decoration.lua
│   ├── bar.lua  launcher.lua  …   -- widgets
│   └── custom/             -- "decoration:rounding.lua"… (written by hw.set_option)
├── layouts/*.json          -- (optional) JSON layouts, loaded with hw.layout()
├── style/main.scss         -- your stylesheet
├── plugins/<name>/{plugin.json, init.lua, style.scss}
├── themes/<name>/{theme.json, _vars.scss, init.lua?}
└── cache/settings/settings.json   -- hw.settings (path configurable in config.json)
```

`package.path` includes the config directory, so `require("lib.util")` loads
`<config>/lib/util.lua`.

### `hyprwidget.lua`

```lua
-- require_all loads config/<name>.lua in order (default prefix "config"),
-- require_custom_dir loads every config/custom/*.lua (sorted by name).
require_all({
  "general",
  "decoration",
  "bar",
  "launcher",
})
require_custom_dir()
```

The entry file can also `return` a window, a list of windows, or a
`function(monitor)` (shorthand for `hw.per_monitor`):

```lua
local W = hw.widget
return function(monitor)
  return W.Window {
    name = "bar-" .. monitor.name,
    monitor = monitor,
    anchor = { "top", "left", "right" },
    exclusivity = "exclusive",
    W.CenterBox {
      start = W.Label("hyprwidget"),
      ["end"] = W.Label { label = hw.poll(1000, "date +%H:%M") },
    },
  }
end
```

Predefined globals: `hw` (also `require("hw")`), `HOME`, `CONFIG_DIR`,
`require_all`, `require_custom_dir`. `print(...)` writes to the daemon log (target
`lua`). The `debug` library is not loaded (Lua runs in safe mode); use
`hw._traceback` with `xpcall` when you need a stack trace.

### `config.json`

Every key is optional:

| Key | Default | |
|---|---|---|
| `settings_file` | `"cache/settings/settings.json"` | file backing `hw.settings` |
| `style` | `"style/main.scss"` | main stylesheet (SCSS or CSS) |
| `theme` | none | theme name in `themes/`; `null` = no theme ([plugins.md](plugins.md#theme)) |
| `plugins` | every plugin found | `["a", "b"]` (only these) or `{ "enabled": […], "disabled": […] }` |
| `layer_rules` | `true` | register blur layer rules for hyprwidget's namespaces automatically |
| `hypr_custom_dir` | `"~/.config/hypr/config/custom"` | where `hw.set_option` writes Hyprland options |
| *any other key* | | options, same structure as `hw.config` (`{"decoration": {"rounding": 16}}` or flat keys `{"decoration:rounding": 16}`) — see [options.md](options.md) |

## Hot reload

The daemon watches the config directory:

- editing `.lua` / `.json` → re-runs Lua and rebuilds every window (services,
  settings, options and the theme are kept);
- editing `.scss` / `.css` → recompiles the stylesheet in place, without
  rebuilding widgets;
- editing a file in `plugins/<name>/` → reloads only that plugin and rebuilds the
  `per_monitor` windows.

Manual commands: `hyprwidget reload` (both), `hyprwidget reload css`, `hyprwidget reload lua`.

Lua errors during a reload are logged with `file:line`; the old windows are
already destroyed, and fixing the file reloads the config.

## Stylesheet

`style/main.scss` is compiled with [grass](https://github.com/connorskees/grass)
(pure-Rust SCSS, no `sass` needed). It can `@import` generated partials:

| Partial | Contents |
|---|---|
| `@import "hypr";` | Hyprland options: `$decoration-rounding`, `$general-gaps_out`… ([options.md](options.md#scss--css)) |
| `@import "hw-theme";` | colors of the active theme: `$background`, `$primary`… ([plugins.md](plugins.md#theme)) |
| `@import "material";` | Material You palette from the wallpaper (`hw.service.theme`) |
| `@import "<name>";` | a partial written from Lua with `hw.scss(name, text)` |

hyprwidget does not use the desktop's GTK theme: the default stylesheet resets
every widget (`* { all: unset; }`) and restyles it from Hyprland's options. Colors
without a matching Hyprland option are the CSS variables `--hw-fg`, `--hw-bg`,
`--hw-surface`, `--hw-surface-hover`, `--hw-accent` (= `general:col.active_border`),
which your stylesheet can override. Every widget has a `hw-<kind>` class
(`hw-box`, `hw-label`…), and windows have `hw-window`.

GTK Inspector: `hyprwidget inspect`.

## Autocomplete (LuaLS)

The [`types/`](../types) directory holds `---@meta` stubs for
[lua-language-server](https://luals.github.io/): `hw.*`, every `W.*` with its
props and events, services and options (named as in the Hyprland wiki). Put a
`.luarc.json` in the config directory (see `examples/demo/.luarc.json`):

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.5",
  "workspace.library": ["/path/to/hyprwidget/types"],
  "diagnostics.globals": ["hw", "W", "HOME", "CONFIG_DIR", "PLUGIN_DIR", "require_all", "require_custom_dir"]
}
```

`"Lua 5.5"` needs LuaLS ≥ 3.19 (older versions: `"Lua 5.4"`). To check a whole
directory (for `examples/demo`, LuaLS 3.19.2 reports only a few type warnings in
the demo code and no errors from the stubs):

```sh
lua-language-server --check examples/demo --checklevel=Warning
```
