# Plugin và theme

## Plugin

Một plugin là thư mục `plugins/<name>/` trong thư mục config:

```
plugins/sysmon/
├── plugin.json      -- manifest (bắt buộc)
├── init.lua         -- entry, chạy trong sandbox
├── lib/format.lua   -- module riêng: require("lib.format")
└── style.scss       -- (tuỳ chọn) biên dịch trước stylesheet của người dùng
```

### `plugin.json`

```json
{
  "name": "sysmon",
  "version": "1.0.0",
  "description": "CPU / RAM / GPU meters",
  "author": "…", "license": "MIT",
  "entry": "init.lua",
  "style": "style.scss",
  "permissions": ["exec", "network", "fs"],
  "dependencies": { "other": ">=1.0" },
  "widgets": ["Sysmon", "SysmonMeter"]
}
```

| Key | | |
|---|---|---|
| `name` | bắt buộc | trùng tên thư mục; chữ, số, `_`, `-` |
| `version` | bắt buộc | `x.y.z` |
| `description`, `author`, `license` | | thông tin |
| `entry` | `"init.lua"` | đường dẫn tương đối trong plugin |
| `style` | `"style.scss"` (nếu có) | |
| `permissions` | `[]` | `exec`, `network`, `fs` (bảng dưới) |
| `dependencies` | | `{ "tên": "yêu cầu version" }` hoặc `["tên", …]`; yêu cầu: `*`, `1.2`, `=1.2`, `>=1.2`, `>1.2`, `<2`, `<=2`, `^1.2` (cùng major), `~1.2` (cùng minor). Plugin được nạp sau dependency; thiếu/sai version/vòng lặp → plugin lỗi |
| `widgets` | | widget plugin cung cấp: được giữ chỗ ngay cả khi `init.lua` lỗi (hiện nhãn `⚠ <plugin>`, class `plugin-error`) để layout không vỡ |

Key lạ, sai kiểu, file không tồn tại… được `hyprwidget check` báo dạng
`plugins/x/plugin.json:<dòng>: …`.

Chọn plugin trong `config.json`: `"plugins": ["a", "b"]` (chỉ các plugin này) hoặc
`"plugins": { "enabled": […], "disabled": […] }`; không khai báo = mọi plugin tìm
thấy.

### `init.lua`

`init.lua` trả về một table, mọi key đều tuỳ chọn:

```lua
return {
  widgets = {                       -- W.<Name> và JSON {"type": "<Name>"}
    Sysmon = function(props) return W.Box { … } end,
  },
  services = {                      -- hw.service.<name>: table, hoặc hàm trả về table (gọi lazy)
    sysmon = { summary = summary_signal },
  },
  requests = {                      -- hyprwidget request <name> [args…]
    sysmon = function(args) return "cpu 12%" end,
  },
  setup = function(info)            -- chạy sau khi nạp (coroutine; info = hw.plugin)
    print("sysmon " .. info.version)
  end,
}
```

- Tên widget không được trùng widget dựng sẵn, `hw.define` hay plugin khác; tên
  service không được trùng service có sẵn.
- Lỗi của plugin (cú pháp, runtime, trong widget) được log, báo bởi `hyprwidget check`,
  hiện thông báo desktop (`notify-send`) và **không** làm dừng plugin khác hay config.
- Sửa file trong `plugins/<name>/` chỉ nạp lại plugin đó rồi dựng lại các window
  `per_monitor` (window top-level trả về từ entry không được dựng lại).

### Sandbox và quyền

Mỗi plugin chạy trong `_ENV` riêng:

- có: hàm cơ bản (`assert`, `error`, `ipairs`, `pairs`, `pcall`, `select`,
  `tonumber`, `tostring`, `type`, `xpcall`, `setmetatable`, `getmetatable` (không trả
  metatable của string), `rawget/rawset/rawequal/rawlen`, `next`, `load` (chỉ text)),
  bản sao `string`, `table`, `math`, `utf8`, `coroutine`, `os.time/date/clock/difftime/getenv`,
  `HOME`, `CONFIG_DIR`, `PLUGIN_DIR`, `W`, `print` (thêm tiền tố `[name]`);
- `require` chỉ nạp module trong thư mục plugin;
- `hw` là proxy của `hw` thật, thêm `hw.plugin = { name, version, dir, permissions }`.

| Quyền | Mở khoá |
|---|---|
| *(không)* | mọi `hw.*` còn lại: widget, signal, service, settings, option, timer, `hw.poll` với **hàm** |
| `exec` | `hw.exec`, `hw.exec_async`, `hw.subprocess`, `hw.poll` với lệnh, `io.popen`, `os.execute` |
| `network` | `hw.fetch(url [, cb])` — HTTP(S) GET qua `curl` (yield như `exec_async`) |
| `fs` | `io.*` (trừ `popen`), `os.remove`, `os.rename`, `os.tmpname`, `hw.watch_file` |

Không bao giờ có trong plugin: `hw.quit`, `hw.config`, `hw.layer_rule`, `hw.define`,
`hw.require_all`, `hw.require_custom_dir` (plugin không đổi config toàn cục; widget
khai báo qua `widgets = {}`).

Sandbox chặn lỗi vô ý và giới hạn quyền mặc định; nó không phải ranh giới bảo mật
chống code cố tình độc hại (plugin vẫn chạy trong cùng process). Chỉ cài plugin bạn tin.

### Lua API cho plugin

```lua
hw.plugins.list()        -- tên plugin đã nạp
hw.plugins.get("sysmon") -- { name, version, description, dir, entry, permissions, widgets, error }
hw.plugins.errors()      -- { [name] = message }
```

### Ví dụ: plugin `sysmon`

`examples/demo/plugins/sysmon` là plugin mẫu đầy đủ, **không cần quyền nào**: đồng
hồ CPU/RAM/GPU dựng trên `hw.service.sysinfo`.

- `plugin.json` khai báo `widgets: ["Sysmon", "SysmonMeter"]`, `permissions: []`.
- `init.lua`:
  - `require("lib.format")` — module riêng của plugin;
  - mỗi meter là `sys.cpu:map(...)` → Signal %, gán vào `W.Label`/`W.Progress` (không
    callback);
  - `summary = hw.computed(...)` đọc ba signal, dùng cho cả service
    `hw.service.sysmon.summary` và request `hyprwidget request sysmon`;
  - `setup(info)` in version khi nạp.
- `style.scss` dùng biến theme (`$background`, `$primary`) và option Hyprland
  (`$decoration-rounding`, `$general-gaps_in`) — có sẵn vì style plugin được biên dịch
  với `@import "hypr"; @import "hw-theme";`.

Dùng trong demo: `config/sysmon.lua` tạo window `sysmon-<monitor>` (ẩn) từ layout JSON
`layouts/sysmon.json` (`Sysmon` + một `Terminal` chạy btop):

```sh
hyprwidget -c examples/demo check          # validate cả manifest
hyprwidget toggle sysmon-DP-2
hyprwidget request sysmon                  # → "cpu 7% mem 41% gpu 3%"
hyprwidget eval 'return hw.service.sysmon.summary()'
```

Viết plugin mới: copy thư mục `sysmon`, đổi `name` trong `plugin.json` và tên thư mục,
bật trong `config.json` (`"plugins": {"enabled": ["sysmon", "mine"]}`), chạy
`hyprwidget check`.

## Theme

Một theme là thư mục `themes/<name>/`:

```
themes/nord/
├── theme.json      -- tên, màu
├── _vars.scss      -- (tuỳ chọn) biến SCSS thêm, nạp sau màu
└── init.lua        -- (tuỳ chọn) trả về { colors = table_or_signal }, sandbox như plugin
```

```json
{ "name": "nord", "version": "1.0.0", "description": "…", "variant": "dark",
  "colors": { "background": "#2e3440", "foreground": "#d8dee9", "primary": "#88c0d0" },
  "source": "wallpaper", "permissions": ["fs"] }
```

Màu của theme (`hw.theme.colors`, reactive), sau thắng trước:

1. `colors` của `theme.json`;
2. `colors` trả về từ `init.lua` (table hoặc Signal; chạy với `permissions` của theme);
3. **nguồn màu** tên `"source"` nếu đã đăng ký — ví dụ `"wallpaper"` = palette
   Material từ `hw.service.theme`, hoặc nguồn tự thêm bằng `hw.theme.add_source`.

Không chọn theme (`"theme": null`): `hw.theme.colors` là nguồn `wallpaper`.

Chọn theme: `"theme": "nord"` trong `config.json`, hoặc lúc chạy
`hyprwidget theme nord` / `hyprwidget theme none` / `hyprwidget theme` (liệt kê, `*` =
đang dùng). Đổi lúc chạy không ghi vào `config.json`.

### SCSS

Partial `_hw-theme.scss` (`@import "hw-theme";`), tự viết lại khi theme hay màu đổi:
`$theme-name`, palette dự phòng `!default` (`$background`, `$foreground`, `$surface`,
`$primary`, `$secondary`, `$accent`, `$urgent`), `$<màu>` cho từng màu của theme,
`$theme-colors` (map), rồi `_vars.scss` của theme. Style của plugin luôn được biên
dịch với partial này.

### Lua

```lua
hw.theme.name()                 -- Signal tên theme (set được: hw.theme.name:set("nord"))
hw.theme.colors.primary         -- Signal của một màu
hw.theme.colors()               -- table mọi màu
hw.theme.palette                -- Signal của cả table màu
hw.theme.set("nord")            -- nil/"none" = bỏ theme
hw.theme.list()                 -- tên theme trong themes/
hw.theme.get()                  -- { name, colors, source }
hw.theme.sources()              -- tên nguồn màu đã đăng ký
hw.theme.add_source("mine", { primary = "#ff8800" })   -- table hoặc Signal; mất khi reload Lua
hw.theme:set_wallpaper(path)    -- chuyển tiếp sang hw.service.theme
```

`W.Terminal` mặc định lấy màu từ `hw.theme.colors`.
