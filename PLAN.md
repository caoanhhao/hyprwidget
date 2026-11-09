# hyprwidget — Plan

Widget framework cho Wayland (wlr-layer-shell) viết bằng Rust, tương tự AGS nhưng cấu hình/giao diện bằng **Lua**, config dữ liệu bằng **JSON**, style bằng **SCSS/CSS**, có hệ thống **plugin/theme** và khả năng **nhúng app khác** bên trong widget.

Example đầu tiên: port lại cấu hình hiện có ở `~/.config/ags`.

---

## 1. Hiện trạng cấu hình AGS (đầu vào cần port)

| Thành phần | File AGS | Phụ thuộc |
|---|---|---|
| Bar (workspaces / information / utilities, layout cấu hình được, ẩn khi fullscreen, tự ẩn khi rời chuột) | `widgets/bar/*` | AstalHyprland, Tray, Battery, Wp, PowerProfiles |
| BarHover / LeftPanelHover / RightPanelHover (dải nhỏ ở mép màn hình để hiện panel) | `widgets/*Hover.tsx` | – |
| LeftPanel / RightPanel (widget selector động: Player, Weather, Crypto, Cava, Calendar, Notification history, Picture, Booru…) | `widgets/leftPanel`, `widgets/rightPanel` | Mpris, Cava, Notifd, script Python |
| AppLauncher (app, lịch sử, quick apps, prefix: `translate`, `cb`, `emoji`, `note`) | `widgets/applauncher/*` | AstalApps, script |
| NotificationPopups | `widgets/NotificationPopups.tsx` | AstalNotifd |
| UserPanel, WallpaperSwitcher, AlwaysOnWidget, KeyStrokeVisualizer | `widgets/*.tsx` | helper C (`scripts/*-loop-ags.c`) |
| Settings toàn cục (đọc/ghi `cache/settings/settings.json`, dạng `AGSSetting{name,value,type,min,max}`) | `variables.ts`, `utils/settings.ts` | – |
| Per-monitor (tạo toàn bộ widget cho từng monitor, debounce khi hot-plug) | `app.tsx` | – |
| Request handler (`ags request translate DP-2`…) | `app.tsx` | – |
| Style SCSS + màu động theo wallpaper | `scss/*` | sass |

---

## 2. Lựa chọn công nghệ

**Mục tiêu chính: bỏ JS engine.** AGS chạy trên GJS (SpiderMonkey, engine JS của Firefox). Đo thực tế: tiến trình `gjs` của AGS chiếm ~2.8 GB RSS (2.6 GB anon heap) sau 28 giờ chạy. GTK4 là toolkit native C, không chứa JS/web engine; dùng từ Rust thì chỉ còn GTK + Lua (vài MB). Không đưa WebKit vào project.

Ngân sách mục tiêu: < 80 MB RSS sau 24h, không tăng theo thời gian. Trên NVIDIA kiểm thử cả renderer `GSK_RENDERER=ngl` và `vulkan`, chọn cái ổn định/nhẹ hơn làm mặc định.

Phương án thay GTK4 đã cân nhắc: `iced` + `iced_layershell` (thuần Rust, nhưng không có CSS → phải tự viết hệ thống style, không tái dùng SCSS, ít widget, IME/tray menu yếu); `smithay-client-toolkit` + renderer tự viết (nhẹ nhất nhưng phải tự làm layout, text, input — khối lượng lớn). → Giữ GTK4.

| Lĩnh vực | Lựa chọn | Lý do |
|---|---|---|
| UI toolkit | **GTK4** (`gtk4-rs`) | Giống AGS → giữ lại được SCSS/CSS, cây widget, animation (Revealer…) |
| Layer shell | **`gtk4-layer-shell`** crate (lib 1.3.0 đã cài) | anchor/margin/exclusive/keyboard-mode/namespace |
| Lua | **`mlua` 0.12** + **Lua 5.5** (feature `lua55`, `async`, `serialize`; link `liblua.so.5.5` hệ thống, cùng thư viện Hyprland 0.57 đang dùng) | Cùng phiên bản và cú pháp với config Lua của Hyprland → chép code qua lại được; hỗ trợ async, map serde ↔ Lua table |
| JSON | `serde` + `serde_json` | config, settings, cache |
| SCSS | **`grass`** (sass compiler thuần Rust) | Biên dịch lại `scss/` hiện có mà không cần `sass` |
| File watch / hot-reload | `gio::FileMonitor` (hoặc `notify`) | Reload Lua/SCSS/JSON khi lưu |
| Async | GLib main loop + `glib::spawn_future_local`; `tokio` chỉ cho service nặng nếu cần | Một event loop duy nhất, tránh lock |
| D-Bus | `zbus` | Notifd, Mpris, Tray (SNI), UPower, PowerProfiles |
| Hyprland | client IPC tự viết trên socket `.socket.sock` / `.socket2.sock` | Event stream nhẹ, không phụ thuộc thư viện ngoài |
| Audio | WirePlumber qua `pipewire-rs` (giai đoạn đầu: `wpctl` + subscribe) | |
| Apps | `freedesktop-desktop-entry` + fuzzy (`nucleo`) | |
| Hệ thống | `sysinfo` + đọc `/proc`, `/sys` trực tiếp | Thay thế các helper C `*-loop-ags.c` |
| Nhúng app | Xem mục 6 | |

> **Phương án thay thế cho services:** sinh binding Rust từ `.gir` của Astal (`libastal-4-git` đã cài) bằng `gir`. Port nhanh hơn, nhưng phụ thuộc Astal/GObject. **Đề xuất: viết native Rust** (ít phụ thuộc, dễ expose ra Lua); có thể dùng Astal gir tạm thời cho service nào khó (ví dụ Cava).

---

## 3. Kiến trúc

```
hyprwidget/                      (cargo workspace)
├── crates/
│   ├── hw-core/        reactive (Signal/Computed/Poll), config JSON, settings store, IPC socket, logging
│   ├── hw-ui/          GTK4 + layer-shell: Window, widget registry, builder từ node-tree, CSS/SCSS loader
│   ├── hw-services/    hyprland, notifd, mpris, tray, audio, battery, network, apps, sysinfo, cava…
│   ├── hw-lua/         mlua runtime: API `hw.*`, bridge Signal ↔ Lua, sandbox plugin, hot-reload
│   └── hw-embed/       compositor lồng (smithay): chạy app khác làm widget
├── src/main.rs         binary `hyprwidget`: daemon + CLI (`run`, `request`, `reload`, `toggle`, `inspect`)
└── examples/
    └── demo/           Example 1 (port ~/.config/ags) — config dùng khi dev
```

### 3.1 Luồng chạy

1. `hyprwidget run [-c dir]` → thư mục config: `-c` > `$HYPRWIDGET_CONFIG` > `$XDG_CONFIG_HOME/hyprwidget` (mặc định `~/.config/hyprwidget`); đọc `config.json` (đường dẫn, theme, plugin bật/tắt, settings).
2. Đọc option Hyprland (`hyprctl getoption`, `hyprctl animations`) → gộp với config riêng (mục 3.6). Khởi tạo services lazy (chỉ chạy khi Lua thực sự dùng).
3. Load `hyprwidget.lua` → Lua khai báo window/widget, đăng ký request handler, load plugin.
4. Biên dịch SCSS (theme + user) → `GtkCssProvider`.
5. Watch file → reload CSS nóng; reload Lua = dựng lại toàn bộ window (giữ state services).

### 3.2 Mô hình reactive (tương đương gnim)

| AGS / gnim | hyprwidget (Lua) |
|---|---|
| `createState(v)` | `local s = hw.state(v)` → `s:get()`, `s:set(v)`, `s()` |
| `createComputed(fn)` | `hw.computed(function() return a() + b() end)` (tự track dependency) |
| `createBinding(obj, "prop")` | `hw.bind(hw.service.hyprland, "focused_client")` |
| `createPoll(ms, cmd)` | `hw.poll(1000, "date +%H:%M")` hoặc `hw.poll(1000, fn)` |
| `accessor(fn)` | `s:map(fn)` |
| `<With value=…>` / `<For each=…>` | `hw.With(sig, fn)`, `hw.For(list_sig, fn, key_fn)` |
| `execAsync` / `subprocess` | `hw.exec_async(cmd)` (coroutine), `hw.subprocess(cmd, on_line)` |
| `timeout` / `interval` | `hw.timeout(ms, fn)`, `hw.interval(ms, fn)` |

Phía Rust: `Signal<T>` dùng `Rc<RefCell>` + danh sách subscriber, chạy trên main thread GTK; service ở thread khác đẩy dữ liệu về bằng `glib::MainContext::channel`.

### 3.3 Giao diện bằng Lua (DSL)

```lua
local hw  = require("hw")
local W   = hw.widget
local hypr = hw.service.hyprland

return function(monitor)
  return W.Window {
    name = "bar-" .. monitor.name, namespace = "bar", monitor = monitor,
    anchor = { "top", "left", "right" }, exclusivity = "exclusive",
    margin = { top = hw.settings.gaps_out, left = hw.settings.gaps_out, right = hw.settings.gaps_out },
    visible = hypr.fullscreen:map(function(f) return not f end),
    W.CenterBox {
      class = "bar",
      start  = require("bar.workspaces")(monitor),
      center = require("bar.information")(),
      ["end"] = require("bar.utilities")(),
    },
  }
end
```

- Mỗi widget là một Lua table → Rust dựng `gtk::Widget`. Thuộc tính nhận giá trị tĩnh **hoặc** Signal (tự bind).
- `setup = function(self) ... end` để truy cập GTK object (tương đương `$=`); expose các method phổ biến + `self:call("method", ...)` dùng GObject introspection động cho phần còn lại.
- Event: `on_click`, `on_scroll`, `on_hover`, `on_leave`, `on_key`, controllers (`motion`, `drag`, `gesture`).
- Widget có sẵn: Window, Box, CenterBox, Label, Button, Image, Icon, Revealer, Stack, Scrolled, Entry, Slider, Switch, Progress, CircularProgress, Overlay, Grid, FlowBox, MenuButton/Popover, DrawingArea (vẽ bằng cairo từ Lua — cho Cava), Picture/Video, Calendar, **Embed** (mục 6).
- **Custom widget** từ Lua: `hw.define("Weather", function(props) ... end)` → dùng như `W.Weather{}`.

### 3.4 JSON

Hai vai trò:

1. **`config.json`** — cấu hình app: thư mục, theme, danh sách plugin, monitor muốn dùng, keybind request…
2. **Settings store** — tương thích `settings.json` của AGS (`AGSSetting{value,type,min,max}`), đọc/ghi qua `hw.settings` (reactive, `hw.settings:set("bar.lock", true)` tự ghi file, có debounce).
3. **(Tùy chọn) layout khai báo bằng JSON**: một node-tree JSON (`{"type":"Box","class":"bar","children":[...]}`) dùng chung builder với Lua — cho phép người dùng sửa layout mà không viết Lua; `"$bind": "settings.bar.lock"` để bind dữ liệu.

### 3.5 Plugin & Theme

Thư mục `plugins/` và `themes/` nằm trong cấu trúc config ở mục 3.7.

- `plugin.json`: tên, version, entry, quyền (`exec`, `network`, `fs`), dependencies.
- Plugin trả về table `{ widgets = {...}, services = {...}, requests = {...}, setup = fn }`; được đăng ký vào registry, dùng được bằng tên trong layout Lua/JSON (ví dụ widget selector của LeftPanel).
- Theme: biến SCSS + palette Lua (`hw.theme.colors.primary` reactive). Màu động theo wallpaper (thay `get-image-color.sh`) → tính palette trong Rust (crate `material-colors`), ghi ra biến SCSS, recompile CSS nóng.
- Mỗi plugin chạy trong Lua env riêng (sandbox `_ENV`), lỗi của plugin không làm sập app (bắt lỗi, log, thông báo).

### 3.6 Giao diện kế thừa từ Hyprland (cùng tên biến)

Border, màu, gaps, bo góc, opacity, shadow, blur, animation của widget **mặc định lấy đúng giá trị Hyprland đang dùng**, và **ghi đè được bằng đúng tên option của Hyprland**.

**Thứ tự ưu tiên** (sau ghi đè trước):

1. Giá trị mặc định có sẵn trong hyprwidget.
2. **Giá trị Hyprland đang chạy**: đọc bằng `hyprctl getoption <tên> -j` và `hyprctl animations -j` lúc khởi động. Dùng giá trị thật của Hyprland nên các file `config/custom/*.lua` của hypr cũng được tính. Lấy lại khi socket2 báo `configreloaded` và sau mỗi lần hyprwidget tự đổi option.
3. **Config riêng của hyprwidget**, cùng tên và cùng cấu trúc với Hyprland (mục 3.7).
4. Ghi đè theo từng window/widget: `theme = { decoration = { rounding = 0 } }` trong Lua, hoặc bằng CSS.

Mọi giá trị là Signal reactive: đổi `rounding` trong Hyprland thì widget bo góc lại ngay.

**Các option được kế thừa** (giữ nguyên tên Hyprland):

| Option Hyprland | Áp vào widget |
|---|---|
| `general:gaps_out` | margin của layer window so với mép màn hình (giống AGS `globalMargin` hiện tại) |
| `general:gaps_in` | spacing mặc định giữa các widget trong Box/panel |
| `general:border_size` | độ dày border của window/panel |
| `general:col.active_border` / `general:col.inactive_border` | màu border khi window đang focus/hover hoặc không. Gradient (`"99808080 45deg"`) chuyển thành `linear-gradient` |
| `decoration:rounding` (+ `rounding_power`) | `border-radius` của window, panel, card, app nhúng |
| `decoration:active_opacity` / `inactive_opacity` | opacity khi window có/không có focus |
| `decoration:dim_inactive`, `dim_strength` | làm tối panel không focus (tuỳ chọn) |
| `decoration:shadow:enabled/range/render_power/color/offset` | `box-shadow` |
| `decoration:blur:enabled` (+ `size`, `passes`, `ignore_opacity`, `popups`) | hyprwidget tự đăng ký layer rule `blur`/`blur_popups` cho namespace của nó (thay cho danh sách viết tay trong `layerrule.lua` hiện tại); `ignore_alpha` của layer lấy từ option riêng `blur:ignore_alpha` (mặc định 0.1 như `layerrule.lua`; `false` = không đặt), vì `blur:popups_ignorealpha` của Hyprland chỉ dành cho popup và thường ~0.97 → panel trong suốt không được blur |
| `animations:enabled`, `bezier`, `animation = layersIn/layersOut/fadeLayers…` | thời gian và easing của Revealer/transition CSS (speed của Hyprland tính bằng 100ms, bezier đổi thành `cubic-bezier()`) |
| `misc:font_family` (nếu có) | font mặc định |

Định dạng màu của Hyprland (`rgb(…)`, `rgba(…)`, `0xAARRGGBB`, gradient nhiều màu + góc) được parse trong Rust thành màu CSS.

**Truy cập trong Lua** (giữ nguyên tên):

```lua
hw.option("general:col.active_border")       -- Signal<Gradient>
hw.option("decoration:rounding")             -- Signal<number>
hw.opt.general.gaps_out                      -- cùng giá trị, dạng table
W.Box { spacing = hw.opt.general.gaps_in, ... }
```

**Truy cập trong SCSS/CSS:** hyprwidget sinh file `_hypr.scss` và CSS custom property. Tên giữ nguyên, chỉ thay `:` và `.` (không hợp lệ trong tên biến) bằng `-`:

```scss
// tự sinh, cập nhật nóng
$general-gaps_in: 7px;
$general-col-active_border: linear-gradient(45deg, rgba(128,128,128,.6), …);
$decoration-rounding: 16px;
:root { --decoration-rounding: 16px; --general-gaps_out: 6px; … }

.panel { border-radius: $decoration-rounding; border: $general-border_size solid; }
```

Stylesheet mặc định của hyprwidget đã dùng sẵn các biến này, nên chưa viết dòng CSS nào widget đã khớp với Hyprland.

**App nhúng (`W.App`):** bo góc theo `decoration:rounding`. Lưu ý: khi bị cắt bo góc, GTK không đẩy thẳng buffer lên Hyprland được (mất zero-copy, phải vẽ qua GPU của hyprwidget). Cho phép tắt (`rounding = 0`) với app nặng như mpv.

### 3.7 Tổ chức config giống Hyprland

Cấu trúc và cách viết giống hệt `~/.config/hypr` hiện tại: Lua `hw.config({...})` tương đương `hl.config({...})`, tách file theo nhóm, thư mục `custom/` để ghi đè từng option.

```
~/.config/hyprwidget/
├── hyprwidget.lua            -- entry: require_all({...}) + require_custom_dir(), giống hyprland.lua
├── config/
│   ├── first/                -- constant, lib, variable (giống hypr/config/first)
│   ├── general.lua           -- hw.config({ general = { gaps_out = 10, col = { active_border = "rgb(808080)" } } })
│   ├── decoration.lua        -- hw.config({ decoration = { rounding = "inherit", blur = { enabled = true } } })
│   ├── animations.lua        -- bezier/animation riêng cho widget (mặc định kế thừa)
│   ├── layerrule.lua         -- hw.layer_rule({ match = { namespace = "bar" }, blur = true })
│   ├── bar.lua  panels.lua  launcher.lua  notifications.lua …   -- widget
│   └── custom/               -- ghi đè từng option, tên file theo option: "decoration:rounding.lua"
├── config.json               -- (tuỳ chọn) cùng dữ liệu dạng JSON
├── style/                    -- *.scss của người dùng
├── plugins/<name>/{plugin.json, init.lua, style.scss}
└── themes/<name>/{theme.json, _vars.scss, init.lua?}
```

- `hw.config()` gọi nhiều lần được, các table được merge sâu theo đúng thứ tự load (như Hyprland).
- Giá trị đặc biệt: `"inherit"` (lấy theo Hyprland; mặc định khi không khai báo), hằng số, hoặc hàm tính từ giá trị Hyprland: `gaps_out = function(h) return h.general.gaps_out * 2 end`.
- **JSON** dùng cùng cấu trúc: `{"general": {"gaps_out": 10}, "decoration": {"rounding": "inherit"}}`; cũng chấp nhận key phẳng: `{"decoration:rounding": 16}`.
- `config/custom/<option>.lua` được ghi tự động khi đổi trong ControlPanel/UserPanel. Đây là cùng quy ước AGS đang dùng để ghi vào `hypr/config/custom/`. Có hai đích: option của **Hyprland** vẫn ghi vào `~/.config/hypr/config/custom/` (+ `hyprctl keyword`); option **riêng của widget** ghi vào `~/.config/hyprwidget/config/custom/`.
- Kèm file type stub cho LuaLS mô tả đầy đủ các option (tên giống wiki Hyprland) để có autocomplete và kiểm tra kiểu.
- Kiểm tra config: `hyprwidget check`. Option sai tên hoặc sai kiểu sẽ báo đúng file và dòng, giống lỗi config của Hyprland.

### 3.8 IPC / CLI

- Socket `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`.
- `hyprwidget request translate DP-2` → gọi handler đăng ký bằng `hw.on_request("translate", function(args) ... end)` (tương thích các lệnh hiện có trong keybind Hyprland).
- Lệnh tích hợp: `toggle <window>`, `reload [css|lua]`, `check`, `option <tên>` (in giá trị đã gộp, ví dụ `hyprwidget option decoration:rounding`), `inspect` (mở GTK Inspector), `eval '<lua>'`, `list`.

---

## 4. Per-monitor & đời sống window

- `hw.monitors` là Signal danh sách monitor (debounce 300ms, bỏ connector `FALLBACK`, giống `app.tsx`).
- `hw.per_monitor(function(monitor) return { Bar(monitor), Launcher(monitor), ... } end)` → tự tạo/huỷ window khi cắm/rút màn hình.
- Window registry theo tên → `hw.window.get("app-launcher-DP-2"):show()`.

---

## 5. Example 1 — `examples/demo`

Port theo thứ tự (mỗi bước dùng được ngay):

1. **Bar** + BarHover: workspaces (Hyprland), information (đồng hồ, focused client), utilities (tray, audio, battery, power profile); orientation top/bottom; auto-hide; ẩn khi fullscreen.
2. **NotificationPopups** (daemon Notifd native) + notification history + DND.
3. **AppLauncher**: app search, history, quick apps, prefix `translate` / `cb` / `emoji` / `note` (giữ các script `.sh/.py` hiện có qua `hw.exec_async`).
4. **RightPanel / LeftPanel** + hover: widget selector đọc từ settings; Player (Mpris), Calendar, Weather, Crypto, Cava (DrawingArea), Notification history.
5. **UserPanel / ControlPanel**: settings Hyprland (gaps, rounding, blur… ghi qua `hyprctl keyword`), hyprsunset, brightness.
6. **WallpaperSwitcher**, AlwaysOnWidget, KeyStrokeVisualizer (thay helper C bằng service Rust đọc `/dev/input` hoặc `libinput`).
7. SCSS: copy `scss/` từ AGS, chỉnh selector cho khớp class mới; dùng chung `settings.json` format.

Tiêu chí xong: chạy `hyprwidget run -c examples/demo` cho giao diện và hành vi tương đương AGS hiện tại trên Hyprland của bạn (DP-2), có thể thay `ags run` trong autostart.

---

## 6. App làm widget (nhúng app Linux vào layer của hyprwidget)

**Mục tiêu:** app Linux bất kỳ (foot, btop trong terminal, pavucontrol, nautilus, mpv, Qt app…) chạy **bên trong** surface layer-shell của hyprwidget như một widget bình thường: nằm trong Box/Revealer, theo CSS (bo góc, opacity, margin), ẩn/hiện cùng panel. App **không** tạo cửa sổ riêng trên Hyprland và **không** mở thành layer-shell surface riêng.

Wayland không có XEmbed, nên cách làm duy nhất là: **hyprwidget tự làm một Wayland compositor mini cho app đó** (nested compositor). App nói chuyện với hyprwidget, không nói chuyện với Hyprland.

### 6.1 Sơ đồ

```
 Hyprland (compositor thật)
   └─ layer surface "hyprwidget" (gtk4-layer-shell)
        └─ GTK widget tree:  Box › Revealer › W.App{command="pavucontrol"}
                                                   │  frame (dmabuf) ▲   │ input ▼
                                                   ▼                     │
 hw-embed (Wayland server trong process hyprwidget, dùng smithay)
   └─ client: pavucontrol  (WAYLAND_SOCKET=fd riêng, DISPLAY không set)
```

### 6.2 Cách hoạt động

1. **Khởi chạy:** `hw-embed` tạo `socketpair`, gắn một đầu vào Wayland server (`Display::insert_client`), chạy app với `WAYLAND_SOCKET=<fd>` + `GDK_BACKEND=wayland`, `QT_QPA_PLATFORM=wayland`, `SDL_VIDEODRIVER=wayland`, bỏ `DISPLAY`. → Biết chính xác client nào thuộc widget nào, không cần socket file. Có thêm chế độ socket (`WAYLAND_DISPLAY=hyprwidget-embed-N`) cho app tự fork tiến trình con.
2. **Event loop:** fd của Wayland server được gắn vào GLib main loop (`glib::unix_fd_add_local`) → chạy chung thread GTK, không cần thread/lock.
3. **Kích thước:** widget `W.App` đo kích thước GTK cấp cho nó → gửi `xdg_toplevel.configure(w, h, [maximized, tiled_*, activated])` để app tự vẽ đúng kích thước, không viền/bóng CSD. `xdg-decoration` ép `server_side` (thực ra là không vẽ decoration). `fractional-scale-v1` + `viewporter` để nét trên màn hình scale lẻ.
4. **Hình ảnh (zero-copy):**
   - Buffer **dmabuf** từ app → `GdkDmabufTextureBuilder` → đặt trong `GtkGraphicsOffload` → GTK chuyển thẳng buffer đó thành **subsurface** của layer surface gửi lên Hyprland. Không copy, không qua GPU của hyprwidget.
   - Buffer **shm** (app vẽ CPU) → `GdkMemoryTexture` (copy 1 lần, chỉ vùng damage).
   - Định dạng/modifier dmabuf quảng bá cho app lấy từ `gdk::Display::dmabuf_formats()` để chắc chắn GTK/Hyprland import được.
5. **Đồng bộ trên NVIDIA (rủi ro chính):** NVIDIA không có implicit sync tốt → hiện thực `linux-drm-syncobj-v1` phía server: chờ acquire point (`drmSyncobjEventfd` gắn vào GLib) rồi mới đưa texture cho GTK; signal release point khi GTK giải phóng texture. Nếu không làm, sẽ bị nhòe/rách hình (giống lỗi 10-bit DP-2 từng gặp).
6. **Frame pacing:** `wl_surface.frame` callback được trả theo `GdkFrameClock` của widget → app chạy đúng tần số màn hình; widget bị ẩn (Revealer đóng, panel ẩn) thì **ngừng gửi frame callback** → app không tốn GPU/CPU khi không nhìn thấy.
7. **Input:** event controller GTK trên widget → chuyển thành `wl_pointer` (motion/button/axis, toạ độ nội bộ), `wl_keyboard` (keymap xkb lấy từ GTK/Hyprland, modifiers, repeat), `wl_touch`. Focus bàn phím: widget nhận focus → `wl_keyboard.enter`; layer surface cần `keyboard-mode = on_demand` khi có `W.App`.
8. **Gõ tiếng Việt (fcitx5/lotus):** server hiện thực `text-input-v3`, cầu nối sang `GtkIMContext` của widget → preedit/commit từ fcitx5 đi qua GTK vào app. Đây là điểm bắt buộc, không làm thì app nhúng không gõ được tiếng Việt.
9. **Popup/menu:** `xdg_popup` (menu chuột phải, combobox) vẽ trong một `GtkPopover`/surface phụ không bị cắt bởi kích thước widget; grab + đóng khi click ra ngoài.
10. **Con trỏ:** `cursor-shape-v1` + cursor surface → đổi `gdk::Cursor` của widget.
11. **Clipboard:** `wl_data_device` + `primary-selection` ↔ `GdkClipboard` (copy/paste hai chiều giữa app nhúng và hệ thống). Drag & drop: giai đoạn sau.
12. **Vòng đời:** app thoát → widget hiện placeholder/tự restart (`restart = "on-exit"`); widget bị huỷ (đổi layout, rút màn hình) → đóng client (`xdg_toplevel.close`, timeout rồi `SIGTERM`); tuỳ chọn `keep_alive` giữ app chạy khi panel ẩn.
13. **App X11:** chạy qua `xwayland-satellite` trỏ vào server nhúng (tuỳ chọn, sau).

### 6.3 API Lua

```lua
W.App {
  command   = { "pavucontrol" },        -- hoặc chuỗi shell
  env       = { GTK_THEME = "Adwaita:dark" },
  class     = "embedded-mixer",          -- CSS: bo góc, border… áp lên khung chứa
  size      = { 420, 320 },              -- nếu không đặt: theo layout GTK
  mode      = "resize",                  -- "resize" (app tự vẽ theo kích thước) | "scale" (giữ kích thước gốc, co giãn hình)
  keep_alive = true,                     -- giữ app chạy khi widget ẩn
  restart   = "on-exit",                 -- "never" | "on-exit" | "on-failure"
  frame_when_hidden = false,             -- dừng render khi không thấy
  on_title  = function(t) title:set(t) end,
  on_exit   = function(code) end,
}
-- điều khiển: self:send_close(), self:restart(), self:focus(), self.pid, self.title (Signal), self.app_id (Signal)
```

JSON tương đương: `{"type":"App","command":["foot","-e","btop"],"keep_alive":true}`.

### 6.4 Giới hạn đã biết

- **App single-instance** (Firefox, Chromium, app GNOME dùng D-Bus activation như nautilus, gnome-calculator) nếu đã có một instance đang chạy trên Hyprland sẽ mở cửa sổ ở instance cũ chứ không vào widget. Cách xử lý: profile riêng (`firefox --new-instance -P widget`, `chromium --user-data-dir=…`), hoặc chạy trước instance trong widget. Ghi rõ trong docs.
- Không thể "giật" một cửa sổ **đang chạy** trên Hyprland vào widget (giao thức Wayland không cho phép). Có thể bổ sung **chế độ mirror chỉ-xem** dùng `hyprland-toplevel-export-v1` (xem trực tiếp hình cửa sổ có sẵn, ví dụ preview workspace) — tuỳ chọn.
- Vulkan/game fullscreen, protocol hiếm (layer-shell lồng, screencopy từ app nhúng) không hỗ trợ.

### 6.5 Các cách khác (phụ)

| Cách | Dùng khi |
|---|---|
| `W.Terminal{}` (VTE4, native, nhẹ) | Chỉ cần TUI (btop, cava, yazi) — không cần compositor lồng |
| Hyprland overlay (`float/pin`, bám theo vị trí widget qua `hyprctl`) | Fallback cho app không chạy được trong compositor lồng |

### 6.6 Thứ tự làm

1. **Spike (ngay sau M0):** server tối thiểu (`wl_compositor`, `wl_shm`, `xdg_shell`), chạy `foot`, hiện hình bằng shm → kiểm chứng ý tưởng.
2. dmabuf + `GtkGraphicsOffload` + `linux-drm-syncobj` trên RTX 4070 (rủi ro lớn nhất, kiểm sớm).
3. Input: pointer, keyboard, `text-input-v3` (gõ tiếng Việt), cursor.
4. Popup, clipboard, fractional scale, decoration, vòng đời, `W.App` Lua/JSON.
5. Kiểm thử ma trận app: foot, pavucontrol (GTK3), nautilus (GTK4), Qt6 (qalculate-qt/dolphin), mpv, firefox (`--new-instance`).

---

## 7. Lộ trình (milestones)

| # | Nội dung | Kết quả kiểm chứng |
|---|---|---|
| **M0** | Workspace cargo (repo `github.com/caoanhhao/hyprwidget`), CI `cargo clippy/test`, binary `hyprwidget run` mở 1 layer-shell window; đo renderer ngl/vulkan | Thấy window trên Hyprland |
| **M0.5** | Spike `hw-embed` (mục 6.6 bước 1–2): `foot` hiện trong layer surface, dmabuf + syncobj trên NVIDIA | Thấy foot chạy trong window hyprwidget, không nhòe |
| **M1** | `hw-core` reactive + `hw-ui` builder + `hw-lua` DSL cơ bản + SCSS (grass) + hot-reload + JSON settings + IPC + option kế thừa Hyprland + cấu trúc config kiểu Hyprland (3.6, 3.7) | Bar "hello" viết bằng Lua, sửa file → cập nhật nóng |
| **M2** | Services: Hyprland, Notifd, Mpris, Tray, Audio, Battery, PowerProfiles, Apps, sysinfo | Lua đọc được dữ liệu reactive |
| **M3** | Example 1 bước 1–3 (Bar, Notifications, Launcher) | Thay được AGS cho phần chính |
| **M4** | Example 1 bước 4–7, plugin/theme system, `W.Terminal` | Port đầy đủ AGS |
| **M5** | `hw-embed` hoàn chỉnh (mục 6.6 bước 3–5), `W.App` trong Lua/JSON | Ma trận app chạy được, gõ tiếng Việt được trong app nhúng |
| **M6** | Tài liệu API Lua (+ file type stub cho LuaLS để có autocomplete), ví dụ plugin | |

---

## 8. Quyết định đã chốt

| # | Vấn đề | Quyết định |
|---|---|---|
| 1 | Services | **Native Rust**: zbus, socket IPC Hyprland, sysinfo, pipewire. Không phụ thuộc Astal. |
| 2 | Lua | **Lua 5.5**, giống Hyprland 0.57 (`mlua` feature `lua55`). Không dùng LuaJIT/Luau. |
| 3 | App làm widget | **Spike ở M0.5** (foot trong widget, dmabuf + syncobj trên NVIDIA), **hoàn chỉnh ở M5**. |
| 4 | Repo | Repo riêng tại `~/projects/hyprwidget`, remote `https://github.com/caoanhhao/hyprwidget` (`origin`). |
| 5 | Vị trí config | **Chính thức:** `~/.config/hyprwidget` (đường dẫn mặc định, theo `$XDG_CONFIG_HOME`). **Khi dev:** `examples/demo` trong repo, chạy bằng `hyprwidget run -c examples/demo` (hoặc `HYPRWIDGET_CONFIG=examples/demo`). Example 1 (port AGS) nằm ở `examples/demo`. |
