# Services (`hw.service.*`)

Service native bằng Rust, **lazy**: chỉ khởi động lần đầu Lua truy cập
`hw.service.<name>`, rồi sống qua mọi lần reload Lua. Tên có sẵn:
`hw.service.names` = hyprland, sysinfo, apps, audio, battery, brightness,
hyprsunset, theme, keystrokes; cộng các service D-Bus notifd, mpris, tray, upower,
power_profiles; cộng service do plugin đăng ký.

Quy ước chung:

- **Prop là Signal** (chỉ đọc): `svc.prop()` để đọc, `svc.prop:map(fn)`,
  `svc.prop:subscribe(fn)`, `hw.bind(svc, "prop")`, hoặc gán thẳng vào prop widget.
  Danh sách/object là table snapshot (dữ liệu JSON) — truyền `id` cho method.
- **Method** gọi bằng `:` (`svc:method(args…)`), đồng bộ, lỗi thì raise. Với
  service native: `svc.props` / `svc.methods` liệt kê tên, `svc.name` là tên service.
- Prop chưa có dữ liệu là `nil` (ví dụ máy không có pin).

Danh sách prop/method dưới đây khớp với code (test `docs_cover_api` kiểm tra mọi tên
service đều có trong trang này và trong `types/`). Lấy lại trực tiếp từ daemon:

```sh
hyprwidget eval 'local s = hw.service.audio return table.concat(s.props, ", ") .. " | " .. table.concat(s.methods, ", ")'
```

## hyprland

State Hyprland qua socket IPC của nó (`.socket.sock` cho request, `.socket2.sock`
cho event); burst event được gộp (12 ms) và chỉ phần bị ảnh hưởng được tải lại.

| Prop | Giá trị |
|---|---|
| `available` | đang chạy dưới Hyprland |
| `workspaces` | list như `hyprctl -j workspaces`: `{id, name, monitor, monitorID, windows, hasfullscreen, lastwindow, lastwindowtitle, ispersistent, …}` |
| `monitors` | list như `hyprctl -j monitors` (`name`, `id`, `width`, `height`, `scale`, `focused`, `activeWorkspace`, `specialWorkspace`, `reserved`…) |
| `clients` | list như `hyprctl -j clients` (`address`, `class`, `title`, `workspace = {id, name}`, `monitor`, `pid`, `at`, `size`, `floating`, `fullscreen`, `focusHistoryID`…) |
| `focused_client` | client đang focus (`activewindow`), hoặc rỗng |
| `focused_workspace` | workspace đang active |
| `focused_monitor` | tên monitor đang focus (`"DP-2"`) |
| `fullscreen` | `{ [monitor] = mode }`: mode fullscreen cao nhất trên workspace hiện tại của từng monitor (0 = không) |
| `special` | `{ [monitor] = tên special workspace đang mở }` |
| `event` | event thô cuối cùng `{name, data, args}` (mỗi event đều báo, kể cả trùng) |

| Method | |
|---|---|
| `:dispatch(...)` | `dispatch <args nối bằng dấu cách>`; Hyprland 0.57 nhận Lua: `hypr:dispatch('hl.dsp.focus({ workspace = 3 })')` |
| `:keyword(option, value...)` | `keyword` (Hyprland 0.57 đã bỏ — dùng `hw.set_option`) |
| `:message(...)` | request thô; bắt đầu bằng `j/` thì trả về table JSON |
| `:refresh()` | tải lại toàn bộ |

## sysinfo

CPU, RAM, GPU, nhiệt độ, ổ đĩa, mạng; worker thread poll mỗi `interval` ms (mặc định
2000). Kích thước tính bằng byte.

| Prop | Giá trị |
|---|---|
| `cpu` | `{usage (%), cores = {%…}, threads, clock_ghz, temp}` |
| `memory` | `{total, used, available, usage (%), swap_total, swap_used, swap_usage}` |
| `gpu` | `{label, load (%), memory_total, memory_used, temp}` (NVIDIA qua `nvidia-smi`, ≥ 5 s/lần) |
| `temperatures` | list `{label, temp, critical}` |
| `disks` | list `{device, mount, fs, total, used, available, usage, removable}` |
| `network` | `{interface, rx_speed, tx_speed (B/s), rx_today, tx_today, rx_total, tx_total}`; tổng theo ngày lưu ở `~/.cache/hyprwidget/bandwidth.log` (định dạng AGS) |
| `uptime` | giây |
| `load` | `{1m, 5m, 15m}` |
| `interval` | ms |

Method: `:set_interval(ms)` (tối thiểu 250).

## apps

Desktop entry, tìm fuzzy (`nucleo`) xếp theo tần suất dùng, launch qua Hyprland
(`hl.dsp.exec_cmd`, nên app là con của Hyprland và theo rule của nó), lịch sử ở
`$XDG_CACHE_HOME/hyprwidget/apps.json`.

| Prop | Giá trị |
|---|---|
| `list` | mọi app hiển thị: `{id, name, generic_name, description, icon, exec, terminal, categories, keywords, wm_class, path, frequency}` |
| `history` | app launch gần đây (tối đa 20) |

| Method | |
|---|---|
| `:query(text [, limit])` | fuzzy, tốt nhất trước (text rỗng = theo tần suất) |
| `:exact_query(text [, limit])` | khớp chuỗi con |
| `:get(id)` | app theo id (tên file `.desktop` bỏ đuôi) hoặc nil |
| `:launch(id_or_app)` | chạy app, ghi lịch sử |
| `:reload()` | quét lại desktop entry |

## audio

PipeWire/WirePlumber: đọc bằng `pw-dump --monitor`, ghi bằng `wpctl`. Volume theo
thang "cubic" của WirePlumber (1.0 = 100 %, như `wpctl get-volume`).

| Prop | Giá trị |
|---|---|
| `available` | `pw-dump` chạy được |
| `speaker`, `microphone` | node mặc định: `{id, name, description, icon, kind, media_class, volume, volumes, mute, state, is_default}` |
| `sinks`, `sources` | list node như trên |
| `streams`, `recorders` | stream phát / ghi của app (thêm `app_name`, `media_name`) |

| Method | |
|---|---|
| `:set_volume(target, v)` | `v` 0..1.5 |
| `:set_mute(target [, on])` | `on` = true/false/`"toggle"` (mặc định toggle) |
| `:toggle_mute(target)` | |
| `:set_default(target)` | |

`target`: `nil`/`"speaker"`/`"sink"` (sink mặc định), `"microphone"`/`"source"`, id
node, hoặc table node có `id`.

## battery

Pin và AC từ `/sys/class/power_supply`, poll 10 s (không cần daemon; bản UPower là
`upower`).

| Prop | Giá trị |
|---|---|
| `available` | có pin |
| `percentage` | 0..100 (gộp các pin) |
| `state` | `"charging"`, `"discharging"`, `"full"`, … |
| `charging`, `ac_online` | bool |
| `time_to_empty`, `time_to_full` | giây |
| `batteries` | list `{name, percentage, state, now, full, rate, model}` |

## brightness

Màn laptop qua `/sys/class/backlight` (ghi bằng `brightnessctl`) và màn ngoài qua
DDC/CI (`ddcutil`, VCP 0x10; poll 30 s, cập nhật lạc quan khi set).

| Prop | Giá trị |
|---|---|
| `available` | có ít nhất một màn điều khiển được |
| `brightness` | 0..1 của màn đầu tiên (nil nếu không có) |
| `displays` | list `{id ("backlight:<dev>" / "ddc:<n>"), kind, name, brightness (0..1), value, max}` |

Method: `:set(v [, display_id])` — `v` 0..1, mọi màn hoặc một màn.

## hyprsunset

Lọc ánh sáng xanh qua socket IPC của hyprsunset (`.hyprsunset.sock`); poll 10 s.

| Prop | Giá trị |
|---|---|
| `available` | hyprsunset đang chạy |
| `temperature` | Kelvin |
| `gamma` | % |
| `identity` | đang ở chế độ không lọc |

Method: `:set_temperature(k)` (1000..20000, thoát identity), `:set_gamma(pct)`
(0..200), `:set_identity()`, `:refresh()`.

## theme

Palette Material You tính từ hình nền (crate `material-colors`, thay
`get-image-color.sh`). Cũng truy cập được qua `hw.theme` (key không phải của
`hw.theme` được chuyển tiếp: `hw.theme:set_wallpaper(p)`). Palette là nguồn màu
`wallpaper` của hệ thống theme ([plugins.md](plugins.md#theme)) và được ghi ra partial
`_material.scss` (`@import "material";` → `$md-primary`, `$md-on-primary`…, `--md-*`).

| Prop | Giá trị |
|---|---|
| `colors` | mọi role Material: `primary`, `on_primary`, `primary_container`, `secondary`, `tertiary`, `error`, `background`, `surface`, `surface_variant`, `outline`, `inverse_*`, `*_fixed`… (`"#rrggbb"`) |
| `mode` | `"dark"` / `"light"` |
| `source` | màu nguồn |
| `dominant` | màu chủ đạo của ảnh |
| `wallpaper` | đường dẫn ảnh |
| `scss` | nội dung partial |
| `busy` | đang tính |

| Method | |
|---|---|
| `:set_wallpaper(path [, mode])` | `mode` `"auto"` (sáng nếu ảnh sáng), `"dark"`, `"light"` |
| `:set_mode(mode)` | giữ màu nguồn |
| `:set_source(hex [, mode])` | |
| `:from_image(path)` | → `{source, dominant, brightness, mode}` (không đổi theme) |

## keystrokes

Phím bấm đọc từ `/dev/input/event*` (thay `keystroke-loop-ags.c`), keycode → keysym
bằng xkbcommon theo `input:kb_layout`/`kb_variant` của Hyprland. Cần quyền đọc
thiết bị (`sudo usermod -aG input $USER`, đăng nhập lại). Không đọc gì cho tới khi
`:enable(true)`.

| Prop | Giá trị |
|---|---|
| `enabled`, `available` | bool |
| `error` | lý do không đọc được |
| `devices` | thiết bị bàn phím đang mở |
| `key` | phím cuối `{label, keysym}` (mỗi lần bấm đều báo) |

Method: `:enable([on = true])`.

## Service D-Bus

Chi tiết đầy đủ: [services-dbus.md](../services-dbus.md). Tóm tắt:

| Service | Field (Signal) | Method |
|---|---|---|
| `notifd` | `notifications`, `dnd` (ghi được), `active` | `:on_notified(fn(n, replaced))`, `:on_resolved(fn(id, reason))` (trả về hàm huỷ), `:dismiss(id)`, `:close(id)`, `:clear()`, `:invoke(id, action)`, `:get(id)`, `:set_dnd(b)`, `:set_ignore_timeout(b)` |
| `mpris` | `players` | `:position(bus)` (Signal), `:play_pause/play/pause/stop/next/previous/raise(bus)`, `:seek(bus, s)`, `:set_position(bus, s)`, `:set_volume(bus, v)`, `:set_shuffle(bus, b)`, `:set_loop_status(bus, s)` |
| `tray` | `items`, `watcher_active` | `:activate/secondary_activate/context_menu(id, x, y)`, `:scroll(id, delta, orientation)`, `:attach_menu(id, menubutton)`, `:popup_menu(id, widget)`, `:menu_layout(id)`, `:about_to_show(id)` |
| `upower` | `battery`, `available`, `is_present`, `percentage`, `state`, `charging`, `time_to_empty`, `time_to_full`, `icon_name`, `energy_rate`, `energy`, `energy_full`, `warning_level` | |
| `power_profiles` | `state`, `available`, `profiles`, `performance_degraded`, `active_profile` (ghi được) | `:set_active_profile(name)` |

## Service của plugin

Plugin trả về `services = { name = table_or_fn }` → `hw.service.name` (xem
[plugins.md](plugins.md)). Ví dụ `hw.service.sysmon.summary` của plugin `sysmon`.
