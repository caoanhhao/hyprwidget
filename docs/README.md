# hyprwidget documentation

[Tiếng Việt](vi/README.md)

hyprwidget is a Wayland (layer-shell) widget framework written in Rust: GTK4
UI, logic/layout in **Lua 5.5**, config/settings in **JSON**, styling in
**SCSS**. Design and roadmap: [`../PLAN.md`](../PLAN.md).

## Lua API

| Page | Contents |
|---|---|
| [getting-started.md](getting-started.md) | install, `hyprwidget run -c`, config directory, Hyprland-style `hyprwidget.lua` layout, hot reload |
| [reactive.md](reactive.md) | `hw.state` / `computed` / `bind` / `poll`, `:map`, `hw.With` / `hw.For`, timers, processes, coroutines |
| [widgets.md](widgets.md) | `W.*` (every built-in widget, props, events, `setup`, `css`/`theme`), `W.Terminal`, `W.App`, `hw.define`, windows & per-monitor, JSON layouts with `$bind` |
| [options.md](options.md) | options inherited from Hyprland (option → effect table), `hw.config`, `hw.opt` / `hw.option`, `_hypr.scss` variables, `config/custom/`, `hw.set_option`, `config.json` |
| [settings.md](settings.md) | `hw.settings` (compatible with AGS' `settings.json`) |
| [services.md](services.md) | `hw.service.*`: props and methods of every service |
| [plugins.md](plugins.md) | plugins (`plugin.json`, permissions, sandbox), themes, the `sysmon` example plugin |
| [cli.md](cli.md) | `hyprwidget …` commands and the IPC socket |
| [`../types/`](../types) | LuaLS type stubs (`---@meta`) for autocomplete — how to enable them in [getting-started.md](getting-started.md#autocomplete-luals) |

## Technical notes

| Page | Contents |
|---|---|
| [memory.md](memory.md) | memory measurements (budget < 80 MB, no growth over time) |
| [renderer.md](renderer.md) | GSK renderer (`vulkan` / `gl`) on NVIDIA |
| [services-dbus.md](services-dbus.md) | details of the D-Bus services (notifd, mpris, tray, upower, power_profiles) |
| [system-widgets.md](system-widgets.md) | the demo's UserPanel, wallpaper palette, AlwaysOn, KeyStroke |
| [embed.md](embed.md), [embed-spike.md](embed-spike.md) | `hw-embed`: the nested compositor behind `W.App` |
