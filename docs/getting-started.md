# Bắt đầu

## Cài đặt

Phụ thuộc hệ thống (Arch): `gtk4`, `gtk4-layer-shell` (≥ 1.3), `lua` (5.5, link
`liblua.so.5.5`), `vte4` (cho `W.Terminal`), `libxkbcommon`. Lúc chạy, một số
service gọi công cụ ngoài: `pipewire` + `wireplumber` (`pw-dump`, `wpctl`),
`brightnessctl` / `ddcutil` (brightness), `nvidia-smi` (GPU), `curl` (plugin có
quyền `network`). Thiếu công cụ nào thì service đó báo `available = false`, phần
còn lại vẫn chạy.

```sh
pacman -S gtk4 gtk4-layer-shell lua vte4 libxkbcommon pipewire wireplumber
cargo build --release
```

## Chạy

```sh
hyprwidget run                       # thư mục config mặc định
hyprwidget run -c examples/demo      # config khác (khi dev)
hyprwidget -i test run -c ~/x        # instance thứ hai, socket riêng
```

Thư mục config, theo thứ tự ưu tiên:

1. `-c <dir>` / `--config <dir>`
2. `$HYPRWIDGET_CONFIG`
3. `$XDG_CONFIG_HOME/hyprwidget`
4. `~/.config/hyprwidget`

Mỗi instance (`-i <name>`, mặc định `default`, hoặc `$HYPRWIDGET_INSTANCE`) có
socket IPC riêng `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock` và thư mục partial
SCSS sinh tự động `$XDG_RUNTIME_DIR/hyprwidget/<instance>/`. Các lệnh khác
(`toggle`, `request`, `eval`…) nói chuyện với daemon qua socket đó — xem
[cli.md](cli.md).

Kiểm tra config mà không mở cửa sổ nào (lỗi in dạng `file:dòng: thông báo`, như
lỗi config của Hyprland):

```sh
hyprwidget check -c examples/demo
```

## Cấu trúc thư mục config

Giống `~/.config/hypr`: một file entry gọi các file con theo thứ tự, thư mục
`config/custom/` để ghi đè từng option.

```
~/.config/hyprwidget/
├── hyprwidget.lua          -- entry
├── config.json             -- (tuỳ chọn) settings file, style, theme, plugin, option dạng JSON
├── config/
│   ├── general.lua         -- hw.config({ general = { … } })
│   ├── decoration.lua
│   ├── bar.lua  launcher.lua  …   -- widget
│   └── custom/             -- "decoration:rounding.lua"… (ghi bởi hw.set_option)
├── layouts/*.json          -- (tuỳ chọn) layout JSON, nạp bằng hw.layout()
├── style/main.scss         -- stylesheet của bạn
├── plugins/<name>/{plugin.json, init.lua, style.scss}
├── themes/<name>/{theme.json, _vars.scss, init.lua?}
└── cache/settings/settings.json   -- hw.settings (đường dẫn đổi được trong config.json)
```

`package.path` có sẵn thư mục config, nên `require("lib.util")` nạp
`<config>/lib/util.lua`.

### `hyprwidget.lua`

```lua
-- require_all nạp config/<tên>.lua theo thứ tự (prefix mặc định "config"),
-- require_custom_dir nạp mọi config/custom/*.lua (sắp theo tên).
require_all({
  "general",
  "decoration",
  "bar",
  "launcher",
})
require_custom_dir()
```

File entry cũng có thể `return` một window, một danh sách window, hoặc một
`function(monitor)` (viết tắt của `hw.per_monitor`):

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

Biến toàn cục có sẵn: `hw` (cũng là `require("hw")`), `HOME`, `CONFIG_DIR`,
`require_all`, `require_custom_dir`. `print(...)` ghi vào log của daemon (target
`lua`). Thư viện `debug` không được nạp (Lua chạy ở safe mode); dùng
`hw._traceback` với `xpcall` nếu cần stack trace.

### `config.json`

Mọi key đều tuỳ chọn:

| Key | Mặc định | |
|---|---|---|
| `settings_file` | `"cache/settings/settings.json"` | file của `hw.settings` |
| `style` | `"style/main.scss"` | stylesheet chính (SCSS hoặc CSS) |
| `theme` | không có | tên theme trong `themes/`; `null` = không theme ([plugins.md](plugins.md#theme)) |
| `plugins` | mọi plugin tìm thấy | `["a", "b"]` (chỉ các plugin này) hoặc `{ "enabled": […], "disabled": […] }` |
| `layer_rules` | `true` | tự đăng ký layer rule blur cho namespace của hyprwidget |
| `hypr_custom_dir` | `"~/.config/hypr/config/custom"` | nơi `hw.set_option` ghi option của Hyprland |
| *mọi key khác* | | option, cùng cấu trúc với `hw.config` (`{"decoration": {"rounding": 16}}` hoặc key phẳng `{"decoration:rounding": 16}`) — xem [options.md](options.md) |

## Hot-reload

Daemon theo dõi thư mục config:

- sửa `.lua` / `.json` → chạy lại Lua và dựng lại toàn bộ window (service, settings,
  option, theme vẫn giữ nguyên);
- sửa `.scss` / `.css` → biên dịch lại stylesheet tại chỗ, không dựng lại widget;
- sửa file trong `plugins/<name>/` → chỉ nạp lại plugin đó và dựng lại các window
  `per_monitor`.

Lệnh tay: `hyprwidget reload` (cả hai), `hyprwidget reload css`, `hyprwidget reload lua`.

Lỗi Lua lúc reload được log kèm `file:dòng`; window cũ đã bị huỷ, sửa file là
config được nạp lại.

## Stylesheet

`style/main.scss` được biên dịch bằng [grass](https://github.com/connorskees/grass)
(SCSS thuần Rust, không cần `sass`). Có thể `@import` các partial sinh tự động:

| Partial | Nội dung |
|---|---|
| `@import "hypr";` | option Hyprland: `$decoration-rounding`, `$general-gaps_out`… ([options.md](options.md#scss--css)) |
| `@import "hw-theme";` | màu của theme đang chọn: `$background`, `$primary`… ([plugins.md](plugins.md#theme)) |
| `@import "material";` | palette Material You từ hình nền (`hw.service.theme`) |
| `@import "<name>";` | partial tự viết từ Lua bằng `hw.scss(name, text)` |

hyprwidget không dùng GTK theme của desktop: stylesheet mặc định reset mọi widget
(`* { all: unset; }`) rồi style lại từ option của Hyprland. Màu không có option
Hyprland tương ứng là biến CSS `--hw-fg`, `--hw-bg`, `--hw-surface`,
`--hw-surface-hover`, `--hw-accent` (= `general:col.active_border`), có thể ghi
đè trong stylesheet của bạn. Mỗi widget có class `hw-<kind>` (`hw-box`,
`hw-label`…), window có `hw-window`.

GTK Inspector: `hyprwidget inspect`.

## Autocomplete (LuaLS)

Thư mục [`types/`](../types) chứa stub `---@meta` cho
[lua-language-server](https://luals.github.io/): `hw.*`, mọi `W.*` với props và
event, các service và option (tên giống wiki Hyprland). Đặt `.luarc.json` trong
thư mục config (xem `examples/demo/.luarc.json`):

```json
{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime.version": "Lua 5.5",
  "workspace.library": ["/đường/dẫn/tới/hyprwidget/types"],
  "diagnostics.globals": ["hw", "W", "HOME", "CONFIG_DIR", "PLUGIN_DIR", "require_all", "require_custom_dir"]
}
```

`"Lua 5.5"` cần LuaLS ≥ 3.19 (bản cũ hơn: `"Lua 5.4"`). Kiểm tra cả thư mục
(với `examples/demo`, LuaLS 3.19.2 chỉ còn vài cảnh báo kiểu trong code demo, không
có lỗi nào từ stub):

```sh
lua-language-server --check examples/demo --checklevel=Warning
```
