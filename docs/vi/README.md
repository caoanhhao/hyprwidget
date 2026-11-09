# Tài liệu hyprwidget

[English](../README.md)

hyprwidget là framework widget cho Wayland (layer-shell) viết bằng Rust: giao
diện GTK4, logic/layout bằng **Lua 5.5**, config/settings bằng **JSON**, style
bằng **SCSS**. Thiết kế và lộ trình: [`../../PLAN.md`](../../PLAN.md).

## API Lua

| Trang | Nội dung |
|---|---|
| [getting-started.md](getting-started.md) | cài đặt, `hyprwidget run -c`, thư mục config, bố cục `hyprwidget.lua` kiểu Hyprland, hot-reload |
| [reactive.md](reactive.md) | `hw.state` / `computed` / `bind` / `poll`, `:map`, `hw.With` / `hw.For`, timer, process, coroutine |
| [widgets.md](widgets.md) | `W.*` (mọi widget dựng sẵn, props, events, `setup`, `css`/`theme`), `W.Terminal`, `W.App`, `hw.define`, window & per-monitor, layout JSON `$bind` |
| [options.md](options.md) | option kế thừa từ Hyprland (bảng option → tác dụng), `hw.config`, `hw.opt` / `hw.option`, biến `_hypr.scss`, `config/custom/`, `hw.set_option`, `config.json` |
| [settings.md](settings.md) | `hw.settings` (tương thích `settings.json` của AGS) |
| [services.md](services.md) | `hw.service.*`: props và methods của từng service |
| [plugins.md](plugins.md) | plugin (`plugin.json`, quyền, sandbox), theme, ví dụ plugin `sysmon` |
| [cli.md](cli.md) | lệnh `hyprwidget …` và IPC socket |
| [`../../types/`](../../types) | type stub LuaLS (`---@meta`) cho autocomplete — cách bật ở [getting-started.md](getting-started.md#autocomplete-luals) |

## Ghi chú kỹ thuật

| Trang | Nội dung |
|---|---|
| [memory.md](memory.md) | đo bộ nhớ (ngân sách < 80 MB, không tăng theo thời gian) |
| [renderer.md](../renderer.md) | (tiếng Anh) renderer GSK (`vulkan` / `gl`) trên NVIDIA |
| [services-dbus.md](../services-dbus.md) | (tiếng Anh) chi tiết các service D-Bus (notifd, mpris, tray, upower, power_profiles) |
| [system-widgets.md](../system-widgets.md) | (tiếng Anh) UserPanel, wallpaper palette, AlwaysOn, KeyStroke của demo |
| [embed.md](../embed.md), [embed-spike.md](../embed-spike.md) | (tiếng Anh) `hw-embed`: compositor lồng cho `W.App` |
