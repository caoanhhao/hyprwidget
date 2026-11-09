# Widget

```lua
local W = hw.widget   -- cũng là hw.W
W.Box {
  class = "bar",            -- prop
  spacing = 8,
  on_click = function(self, button) end,   -- event
  setup = function(self) end,              -- chạy một lần sau khi dựng
  W.Label("xin chào"),      -- child theo vị trí
  W.Label { label = hw.poll(1000, "date +%T") },   -- prop là Signal → tự cập nhật
}
```

Một widget là một Lua table `{ type = "Box", ... }`; `W.Box{…}` chỉ gắn `type`.
`W.X("text")` (hoặc `W.X(signal)`) là viết tắt của `W.X{ label = … }`. Chỗ nào nhận
widget cũng nhận: string/number (→ `Label`), Signal (→ child dựng lại khi đổi, xem
[reactive.md](reactive.md#widget-động-hwwith-hwfor-signal-làm-child)), `nil`/`false`
(→ bỏ qua), widget đã dựng (`self`).

Mọi prop nhận giá trị tĩnh **hoặc** Signal. Prop không có trong bảng dưới được chuyển
sang **GObject property cùng tên** (`_` → `-`), ví dụ `W.Picture { content_fit = "cover" }`,
`W.Label { lines = 2 }`; enum nhận nick (`"cover"`), sai tên thì báo lỗi.

## Prop chung (mọi widget)

| Prop | Kiểu | |
|---|---|---|
| `class` / `class_name` | `"a b"` hoặc `{"a","b"}` | CSS class (thêm vào `hw-<kind>` có sẵn); đổi class qua Signal thay đúng bộ class cũ |
| `name` | string | tên widget (CSS `#name`; với Window là tên registry) |
| `visible` | bool | |
| `tooltip` / `tooltip_text`, `tooltip_markup` | string | |
| `sensitive`, `hexpand`, `vexpand`, `focusable` / `can_focus`, `can_target` | bool | |
| `halign`, `valign` | `"start"`/`"left"`/`"top"`, `"end"`/`"right"`/`"bottom"`, `"center"`, `"fill"`, `"baseline"` | |
| `width` / `width_request`, `height` / `height_request` | int | kích thước tối thiểu (DrawingArea: kích thước nội dung) |
| `margin` | `n`, `"t r b l"`, `{t, r, b, l}`, `{top=…, right=…}` | như CSS shorthand |
| `opacity` | number | |
| `cursor` | string | tên cursor CSS (`"pointer"`) |
| `widget_name` | string | |
| `css` | string hoặc Signal | CSS inline cho riêng widget này: `css = "padding: 4px;"` |
| `theme` | table hoặc Signal | ghi đè option Hyprland cho widget: `theme = { decoration = { rounding = 0 } }` → biến `--decoration-rounding` của widget |
| `popover` | `W.Popover{…}` | popover gắn vào widget bất kỳ; mở/đóng bằng prop `open` của popover |
| `setup` | `function(self)` | chạy sau khi dựng (trong coroutine) |

Các widget con trong Grid nhận thêm `col`, `row`, `col_span`, `row_span`; trong
Stack, `name` của child là tên trang.

## Event

Handler nhận `self` (widget) rồi các tham số. Trả về `true` để chặn event lan tiếp
(`on_scroll`, `on_key`). Handler chạy trong coroutine (gọi được
`hw.exec_async(cmd)`).

| Event | Tham số | Widget |
|---|---|---|
| `on_click` | `button` (1 trái, 2 giữa, 3 phải) | mọi widget (Button: click thường + nút khác) |
| `on_scroll` | `dx, dy` | mọi widget |
| `on_hover` | `x, y` | mọi widget (con trỏ vào) |
| `on_leave` | | mọi widget |
| `on_motion` | `x, y` | mọi widget |
| `on_key` | `name, code, mods` (`mods` = list `"shift"`,`"ctrl"`,`"alt"`,`"super"`) | widget có focus |
| `on_change` | giá trị mới | Entry (text), Slider (number), Switch (bool), Stack (tên trang), Revealer (đã hiện xong), Calendar (`"YYYY-MM-DD"`), Popover (đang mở), MenuButton (đang mở) |
| `on_activate` | text | Entry (Enter) |
| `on_show` | `true` | mọi widget (map: window hiện, revealer mở…) |
| `on_hide` | `false` | mọi widget (unmap) |
| `on_destroy` | | mọi widget |
| `on_title` | title | App |
| `on_exit` | exit code | App, Terminal |

Đăng ký sau khi dựng: `self:on("on_click", fn)`.

## `self` (widget trong `setup` / handler)

| | |
|---|---|
| `self.kind`, `self.name` | `"Box"`, tên widget |
| `self:set(prop, value)` | đặt prop như trong table |
| `self:get(prop)` | đọc GObject property (`self:get("label")`) |
| `self:add_class(c)`, `remove_class(c)`, `toggle_class(c [, on])`, `has_class(c)` | |
| `self:show()`, `hide()`, `is_visible()`, `queue_draw()`, `grab_focus()` | |
| `self:on(event, fn)` | |
| `self:parent()` | widget cha (không có `kind`) |
| `self:window()` | window chứa widget (xem [Window](#window)) |
| `self.pid`, `self.title`, `self.app_id`, `self:send_close()`, `self:restart()`, `self:focus()` | chỉ với `W.App` |

## Danh sách widget dựng sẵn

Children: **n** = nhiều child theo vị trí / `children = {…}` (hoặc Signal của list);
tên slot = một child (child đầu tiên theo vị trí điền vào `child`).

| Widget | GTK | Children | Prop riêng |
|---|---|---|---|
| `Window` | ApplicationWindow + layer-shell | `child` | xem [Window](#window) |
| `Box` | Box | n | `vertical`, `spacing` (mặc định `general:gaps_in`), `homogeneous` |
| `CenterBox` | CenterBox | `start`, `center`, `end` | `vertical` |
| `Label` | Label | | `label`/`text`, `markup`, `use_markup`, `wrap`, `xalign`, `yalign`, `max_width_chars`, `width_chars`, `selectable`, `ellipsize` (`true`, `"start"`/`"middle"`/`"end"`/`"none"`), `justify` (`"left"`/`"right"`/`"center"`/`"fill"`) |
| `Button` | Button | `child` | `label`, `icon` (tên icon), `has_frame` |
| `Image` | Image | | `icon`/`icon_name`, `file`, `pixel_size`/`size` |
| `Icon` | Image | | `icon`/`icon_name`, `pixel_size`/`size` |
| `Picture` | Picture | | `file`; `content_fit` (`"fill"`/`"contain"`/`"cover"`/`"scale_down"`), `can_shrink` qua GObject |
| `Revealer` | Revealer | `child` | `reveal`/`reveal_child`, `transition` (`none`, `crossfade`/`fade`, `slide_{left,right,up,down}`, `swing_{left,right,up,down}`), `duration`/`transition_duration` (ms, mặc định theo `animations:animation:layersIn`) |
| `Stack` | Stack | n (child `name` = tên trang) | `shown`/`visible_child_name`, `transition` (thêm `slide_left_right`, `slide_up_down`, `over_*`, `rotate_left/right`), `duration`, `hhomogeneous`, `vhomogeneous` |
| `Scrolled` | ScrolledWindow | `child` | `hscroll`, `vscroll` (`"never"`/`"auto"`/`"always"`/`"external"`), `propagate_natural_width`, `propagate_natural_height` |
| `Entry` | Entry | | `text`, `placeholder`/`placeholder_text`, `password`, `position`/`cursor_position` (-1 = cuối) |
| `Slider` | Scale | | `value`, `min`, `max`, `step`, `draw_value`, `digits`, `vertical`, `inverted` |
| `Switch` | Switch | | `active` |
| `Progress` | ProgressBar | | `value`/`fraction` (0..1), `vertical`, `inverted`, `text` |
| `Overlay` | Overlay | `child` (nền, quyết định kích thước) + n overlay | |
| `Grid` | Grid | n (`col`,`row`,`col_span`,`row_span`; hoặc tự xếp theo `columns`) | `columns`, `spacing`, `row_spacing`, `column_spacing`, `homogeneous` |
| `FlowBox` | FlowBox | n | `max_per_line`, `min_per_line`, `homogeneous`, `spacing` |
| `MenuButton` | MenuButton | `child`, `popover` | `label`, `icon`, `always_show_arrow`, `active` |
| `Popover` | Popover | `child` | `has_arrow`, `autohide`, `open` |
| `DrawingArea` | DrawingArea | | `width`/`content_width`, `height`/`content_height`, `redraw` (đổi giá trị → vẽ lại), `draw = function(cr, w, h)` |
| `Calendar` | Calendar | | `date` (`"YYYY-MM-DD"`), `show_heading`, `show_day_names`, `show_week_numbers` |
| `Terminal` | VTE4 Terminal | | xem [W.Terminal](#wterminal) |
| `App` | `hw-embed` EmbedView | | xem [W.App](#wapp) |

`Switch` và `Progress` mặc định `valign = "center"` (không có theme GTK nên không gì
kéo giãn chúng). `Overlay`: child theo vị trí là **overlay** và không làm to widget —
đặt nền bằng `child = …`.

### `DrawingArea` và cairo

```lua
W.DrawingArea {
  width = 120, height = 32,
  redraw = hw.service.sysinfo.cpu,          -- vẽ lại khi CPU đổi
  draw = function(cr, w, h)                 -- màu nguồn ban đầu = CSS `color`
    cr:set_line_width(2)
    cr:rounded_rectangle(0, 0, w, h, 6)     -- (x, y, w, h, radius)
    cr:stroke()
  end,
}
```

Method của `cr`: `set_source_rgba(r,g,b,a)`, `set_source_rgb`, `set_source_color(css)`,
`set_line_width`, `set_line_cap("butt"|"round"|"square")`, `move_to`, `line_to`,
`rel_line_to`, `curve_to`, `rectangle`, `rounded_rectangle(x, y, w, h, r)`,
`arc(xc, yc, r, a1, a2)`, `arc_negative`, `new_path`, `close_path`, `fill`,
`fill_preserve`, `stroke`, `stroke_preserve`, `paint([alpha])`, `clip`, `save`,
`restore`, `translate`, `scale`, `rotate`, `select_font_face(family [, bold])`,
`set_font_size`, `show_text`,
`text_extents(text)` → `{width, height, x_bearing, y_bearing, x_advance}`.

### `W.Terminal`

Terminal VTE4 nhẹ, chạy TUI (btop, cava, yazi, shell) bên trong widget — không cần
compositor lồng.

```lua
W.Terminal {
  command = { "btop" },            -- list hoặc chuỗi shell; mặc định $SHELL
  cwd = "~", env = { TERM_PROGRAM = "hyprwidget" },
  font = "JetBrainsMono Nerd Font 10", font_scale = 1.0,
  size = { cols = 110, rows = 30 }, -- hoặc { 110, 30 }
  colors = { foreground = "#ccc", background = "#111", cursor = "#fff", palette = { … 16 màu } },
  transparent = false, scrollback = 10000, cursor_blink = true,
  input = true, bold_is_bright = false,
  restart = "never",               -- "on-exit" | "on-failure"
  on_exit = function(self, status) end,
}
```

- Tiến trình con chạy khi widget được realize lần đầu: window ẩn = chưa chạy.
- `colors` mặc định theo `hw.theme.colors` (đổi theme → terminal đổi màu);
  `palette` là list 16 màu hoặc các key `color0`..`color15`.
- `self:set("send", "q")` ghi vào stdin của chương trình; `self:set("restart_now", true)`
  chạy lại khi đã thoát.
- Window chứa terminal cần `keymode = "on_demand"` để gõ được.

### `W.App`

Chạy một app Linux **bên trong** widget qua compositor Wayland lồng (`hw-embed`);
chi tiết kỹ thuật, ma trận app và giới hạn: [embed.md](../embed.md).

```lua
W.App {
  command = { "pavucontrol" },     -- list, hoặc chuỗi (chạy bằng sh -c)
  env = { GTK_THEME = "Adwaita:dark" },
  size = { 420, 320 },             -- hoặc { width =, height = }; nil = theo layout GTK
  mode = "resize",                 -- "resize": app vẽ theo kích thước widget | "scale"
  keep_alive = true,               -- giữ app khi widget ẩn
  restart = "on-exit",             -- "never" | "on-exit" | "on-failure" | true/false
  frame_when_hidden = false,       -- dừng frame callback khi không thấy
  socket = false,                  -- true: WAYLAND_DISPLAY riêng (app tự fork: mpv, firefox)
  rounding = 12,                   -- mặc định decoration:rounding; 0/false = không cắt (zero-copy)
  on_title = function(self, title) end,
  on_exit = function(self, code) end,
}
```

Trong `setup`/handler: `self.pid`, `self.title` / `self.app_id` (Signal),
`self:send_close()`, `self:restart()`, `self:focus()`. Layer surface tự chuyển sang
`keymode = "on_demand"` để app nhận bàn phím. App single-instance (Firefox,
nautilus…) cần profile/instance riêng.

## Window

```lua
W.Window {
  name = "bar-" .. monitor.name,    -- tên registry (hyprwidget toggle <name>)
  namespace = "bar",                -- layer namespace (mặc định = name); dùng cho layer rule
  monitor = monitor,                -- Monitor, connector "DP-2", hoặc nil (compositor chọn)
  layer = "top",                    -- "background" | "bottom" | "top" | "overlay"
  anchor = { "top", "left", "right" },   -- hoặc "top left right"
  exclusivity = "exclusive",        -- "exclusive" | "normal" | "ignore"; hoặc exclusive_zone = n
  margin = 10,                      -- như CSS; mặc định general:gaps_out
  keymode = "on_demand",            -- "none" | "exclusive" | "on_demand" (keyboard_mode)
  visible = true,                   -- mặc định hiện; Signal được
  default_width = 400, default_height = 300, title = "…",
  W.CenterBox { … },                -- child
}
```

- `W.Window` chỉ dùng ở top-level: trả về từ `hyprwidget.lua`, từ hàm `per_monitor`,
  hoặc tạo bằng `hw.window.new{…}`.
- Window bắt đầu ẩn (`visible = false` hoặc Signal đang `false`) chỉ dựng cây con lần
  đầu được hiện (`hyprwidget toggle`, `:show()`, Signal `visible` thành `true`); trước
  đó `:widget()` chưa có con, lỗi dựng cây con được ghi log lúc hiện (`docs/memory.md`).
- Mỗi namespace được tự đăng ký layer rule blur theo `decoration:blur:*` (tắt bằng
  `"layer_rules": false` trong `config.json`); thêm rule riêng bằng
  `hw.layer_rule{ match = { namespace = "bar" }, … }` (gửi `hl.layer_rule(...)` cho
  Hyprland 0.57).

| API | |
|---|---|
| `hw.per_monitor(function(monitor) return { win1, win2 } end)` | tạo window cho từng monitor; tự tạo/huỷ khi cắm/rút màn hình (debounce 300 ms, bỏ connector `FALLBACK`) |
| `hw.monitors` | Signal list Monitor: `name`/`connector`, `description`, `manufacturer`, `model`, `x`, `y`, `width`, `height`, `scale`, `refresh_rate` |
| `hw.window.get(name)` | Window hoặc nil: `:show()`, `:hide()`, `:toggle()` → visible, `:is_visible()`, `.visible`, `:set(prop, v)`, `:widget()`, `:destroy()` |
| `hw.window.new{…}` | tạo window lúc chạy (nil khi headless) |
| `hw.window.list()` | tên các window |
| `hw.window.toggle(name)` | → visible mới, nil nếu không có |

## `hw.define`: widget tự viết

```lua
hw.define("Weather", function(props)
  return W.Box {
    class = "weather " .. (props.class or ""),
    W.Label { label = hw.poll(600000, "curl -s 'wttr.in?format=%t'") },
    W.Label(props.city or ""),
  }
end)

W.Weather { city = "Hà Nội" }              -- Lua
{ "type": "Weather", "city": "Hà Nội" }   -- JSON layout
```

Hàm nhận nguyên table props (kể cả children theo vị trí) và trả về một widget. Không
được trùng tên widget dựng sẵn. Plugin khai báo widget theo cách khác (xem
[plugins.md](plugins.md)).

## Layout JSON

Cùng cây node với Lua, cho người dùng sửa layout mà không viết Lua:

```json
{
  "type": "Box", "class": "panel", "vertical": true,
  "children": [
    { "type": "Label", "label": { "$bind": "settings.bar.title" } },
    { "type": "Revealer", "reveal": { "$bind": "settings.bar.expanded" },
      "child": { "type": "Sysmon", "items": ["cpu", "mem"] } },
    { "type": "Label", "$bind": { "label": "option.decoration:rounding" } },
    { "type": "App", "command": ["pavucontrol"], "keep_alive": true }
  ]
}
```

```lua
W.Window { name = "panel", hw.layout("layouts/panel.json") }   -- đường dẫn tương đối thư mục config
```

- `{"$bind": "settings.<key>"}` → Signal của settings (ghi được); `"option.<tên>"` /
  `"opt.<tên>"` → Signal của option (chỉ đọc).
- Dạng gom theo node: `"$bind": { "<prop>": "<nguồn>", … }`.
- `type` là widget dựng sẵn, widget của `hw.define` hoặc plugin. Event và hàm
  (`setup`, `draw`) chỉ viết được bằng Lua: bọc layout JSON trong một widget
  `hw.define` nếu cần.
