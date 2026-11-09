# Option kế thừa từ Hyprland

Border, màu, gaps, bo góc, opacity, shadow, blur, animation của widget mặc định
**lấy đúng giá trị Hyprland đang dùng**, và ghi đè được bằng **đúng tên option của
Hyprland** (như trên [wiki Hyprland](https://wiki.hypr.land/Configuring/Variables/)).

Thứ tự ưu tiên (sau thắng trước):

1. mặc định có sẵn (giá trị mặc định của Hyprland — dùng khi không chạy dưới Hyprland);
2. **Hyprland đang chạy**: đọc qua socket IPC (`getoption`, `animations`) lúc khởi
   động, đọc lại khi Hyprland báo `configreloaded` và sau mỗi `hw.set_option`. Vì đọc
   giá trị thật, các file `config/custom/*.lua` của Hyprland cũng được tính;
3. config của hyprwidget: `config.json`, rồi các lời gọi `hw.config{}` theo thứ tự nạp;
4. theo từng widget: prop `theme = {…}` hoặc CSS.

Mọi option là Signal: đổi `rounding` trong Hyprland thì widget bo góc lại ngay.

## Bảng option → tác dụng

Đây là danh sách `INHERITED` trong `crates/hw-core/src/options.rs` (test
`docs_cover_api` kiểm tra bảng này đủ). Cột "Mặc định" là giá trị khi không có Hyprland.

| Option | Kiểu | Mặc định | Tác dụng trong hyprwidget |
|---|---|---|---|
| `general:gaps_in` | gaps | `5` | `spacing` mặc định của mọi `W.Box` |
| `general:gaps_out` | gaps | `20` | `margin` mặc định của mọi `W.Window` so với mép màn hình |
| `general:border_size` | int (px) | `1` | độ dày border của `.hw-panel`, tooltip, popover, entry… |
| `general:col.active_border` | màu/gradient | `0xffffffff` | border `.hw-panel:hover`, màu nhấn `--hw-accent` (selection, focus, slider…) |
| `general:col.inactive_border` | màu/gradient | `0xff444444` | border thường (`--hw-border`) |
| `decoration:rounding` | int (px) | `0` | `border-radius` của panel, card, tooltip (một nửa), popover; bo góc `W.App` |
| `decoration:rounding_power` | float | `2.0` | chỉ xuất ra SCSS |
| `decoration:active_opacity` | float | `1.0` | opacity `.hw-panel:hover` |
| `decoration:inactive_opacity` | float | `1.0` | opacity `.hw-panel` |
| `decoration:dim_inactive` | bool | `false` | chỉ xuất ra SCSS |
| `decoration:dim_strength` | float | `0.5` | chỉ xuất ra SCSS |
| `decoration:shadow:enabled` | bool | `true` | bật `box-shadow` của `.hw-panel` |
| `decoration:shadow:range` | int (px) | `4` | bán kính shadow |
| `decoration:shadow:render_power` | int | `3` | chỉ xuất ra SCSS |
| `decoration:shadow:color` | màu | `0xee1a1a1a` | màu shadow |
| `decoration:shadow:offset` | vec2 (px) | `[0, 0]` | chỉ xuất ra SCSS (`0px 0px`) |
| `decoration:blur:enabled` | bool | `true` | tự đăng ký layer rule `blur` cho mọi namespace của hyprwidget |
| `decoration:blur:size` | int (px) | `8` | chỉ xuất ra SCSS (Hyprland tự blur) |
| `decoration:blur:passes` | int | `1` | chỉ xuất ra SCSS |
| `decoration:blur:ignore_opacity` | bool | `true` | chỉ xuất ra SCSS |
| `decoration:blur:popups` | bool | `false` | thêm layer rule `blur_popups` |
| `decoration:blur:popups_ignorealpha` | float | `0.2` | chỉ xuất ra SCSS (layer dùng `blur:ignore_alpha` bên dưới) |
| `animations:enabled` | bool | `true` | chỉ xuất ra SCSS |
| `misc:font_family` | string | `"Sans"` | `font-family` mặc định |

Ngoài ra **cả cây `animations`** của Hyprland được đọc (`hyprctl animations`):
`animations:bezier:<tên>` = `cubic-bezier(…)`, và mỗi animation
`animations:animation:<tên>:{enabled, speed, bezier, style, duration, easing}` —
`duration` (ms) = `speed` × 100 ms, `easing` = `cubic-bezier()` của bezier. Animation
không khai báo kế thừa từ cha như Hyprland (`layersIn` ← `layers` ← `global`).
`Revealer`/`Stack` mặc định `duration = animations:animation:layersIn:duration`;
transition CSS mặc định dùng `fadeLayersIn`.

Option **riêng của widget** (section không thuộc Hyprland):

| Option | Mặc định | |
|---|---|---|
| `blur:ignore_alpha` | `0.1` | `ignore_alpha` của layer rule blur (`false` = không đặt). Không dùng `popups_ignorealpha` của Hyprland vì giá trị đó (thường ~0.97) làm panel trong suốt mất blur |

Section của Hyprland (`general`, `decoration`, `animations`, `input`, `gestures`,
`group`, `misc`, `binds`, `xwayland`, `opengl`, `render`, `cursor`, `ecosystem`,
`experimental`, `debug`, `dwindle`, `master`): `hyprwidget check` báo lỗi nếu option
sai kiểu. Key top-level khác là option riêng của widget, tự đặt tên tuỳ ý (đọc bằng
`hw.option`).

Định dạng màu của Hyprland (`rgb(…)`, `rgba(…)`, `0xAARRGGBB`, gradient nhiều màu +
góc `"rgba(33ccffee) rgba(00ff99ee) 45deg"`) được chuyển thành màu CSS; gradient
thành `linear-gradient(…)`.

## `hw.config`

```lua
hw.config({
  general = {
    gaps_out = 10,                       -- hằng số
    gaps_in = "inherit",                 -- (mặc định) theo Hyprland; huỷ một hằng số trước đó
    col = { active_border = "rgb(808080)" },
  },
  decoration = {
    rounding = function(h) return h.decoration.rounding * 2 end,   -- tính từ giá trị Hyprland
    blur = { enabled = true },
  },
  ["decoration:shadow:range"] = 8,       -- key phẳng cũng được
  blur = { ignore_alpha = 0.3 },         -- option riêng của widget
})
```

- Gọi nhiều lần được; các table merge sâu theo đúng thứ tự nạp (như `hl.config` của
  Hyprland).
- Hàm `function(h)` nhận cây option của lớp 1+2 (`h.general.gaps_in`…) và được tính
  lại khi Hyprland đổi.
- Giá trị sai kiểu được báo kèm `file:dòng` (`hyprwidget check`).
- `config.json` nhận cùng cấu trúc: `{"general": {"gaps_out": 10}}` hoặc
  `{"decoration:rounding": 16}`.

## Đọc trong Lua

```lua
hw.option("decoration:rounding")        -- Signal<number>
hw.option("general:col.active_border")  -- Signal<string> (CSS)
hw.opt.general.gaps_out                 -- cùng giá trị, truy cập theo path (lá là Signal)
hw.opt.decoration()                     -- cả nhánh hiện tại dạng table
W.Box { spacing = hw.opt.general.gaps_in, … }
```

Gaps khác nhau theo từng cạnh là table `{top, right, bottom, left}`. Lệnh
`hyprwidget option decoration:rounding` in giá trị đã gộp (chạy được cả khi không có
daemon).

## SCSS / CSS

hyprwidget sinh partial `_hypr.scss` (`@import "hypr";`) và cập nhật nóng. Tên biến
giữ nguyên tên option, `:` và `.` thay bằng `-`:

```scss
// tự sinh
$general-gaps_in: 4px;
$general-col-active_border: rgba(128,128,128,0.6);     // màu hoặc linear-gradient(...)
$general-col-active_border-color: rgba(128,128,128,0.6); // màu đầu tiên (dùng được cho border-color)
$decoration-rounding: 16px;
$animations-animation-layersIn-duration: 400ms;
$animations-animation-layersIn-easing: cubic-bezier(0, 1, 0, 1);
:root { --decoration-rounding: 16px; --general-gaps_out: 6px; … }

// stylesheet của bạn
@import "hypr";
.panel { border-radius: $decoration-rounding; border: $general-border_size solid $general-col-inactive_border-color; }
```

Prop `theme` của widget sinh CSS custom property cho riêng widget đó:
`theme = { decoration = { rounding = 0 } }` → `--decoration-rounding: 0px`.

## `config/custom/` và `hw.set_option`

`hw.set_option(name, value [, target])` là thứ ControlPanel/UserPanel gọi khi người
dùng đổi một option; giống quy ước AGS ghi vào `hypr/config/custom/`:

| target | Ghi vào | Áp dụng |
|---|---|---|
| `"hypr"` | `<hypr_custom_dir>/<name>.lua`: `hl.config({ decoration = { rounding = 16 } })` | `hyprctl eval` cùng dòng đó (Hyprland 0.57 không còn `keyword`), rồi đọc lại option |
| `"widget"` | `<config>/config/custom/<name>.lua`: `hw.config({ … })` | merge ngay vào lớp option của hyprwidget |
| `"auto"` (mặc định) | `"hypr"` nếu Hyprland biết option đó, ngược lại `"widget"` | |

Trả về target đã dùng. `value`: boolean, number, string hoặc list. `hypr_custom_dir`
mặc định `~/.config/hypr/config/custom` (đổi trong `config.json`); `require_custom_dir()`
trong `hyprwidget.lua` nạp lại các file `config/custom/*.lua` của widget lúc khởi động.

```lua
hw.set_option("decoration:rounding", 12)            -- → Hyprland
hw.set_option("blur:ignore_alpha", 0.2)             -- → widget
```

Gọi thẳng Hyprland: `hw.service.hyprland:dispatch(...)`, `:message(...)` — xem
[services.md](services.md#hyprland).
