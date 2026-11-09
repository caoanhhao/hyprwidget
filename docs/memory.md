# Bộ nhớ (M6)

Mục tiêu trong PLAN §2: **< 80 MB RSS sau 24h, không tăng theo thời gian** (AGS/gjs:
2.8 GB RSS sau 28h). Đo ngày 2026-11-09 trên máy dev: RTX 4070 (driver NVIDIA),
Hyprland 0.57, GTK 4.24.1, renderer `vulkan` (mặc định), bản **release** của commit
M6, config `examples/demo` (port đầy đủ AGS: bar, panel trái/phải, launcher,
notification, user panel, wallpaper switcher, sysmon + btop trong `W.Terminal`…) trên
một màn hình (DP-2).

## Kết luận

- **Không thấy rò bộ nhớ không giới hạn.** RssAnon tăng theo bậc khi từng window được
  mở lần đầu (texture, glyph, icon, CSS image, pipeline Vulkan, VTE + btop…) rồi
  **phẳng**: 4 phút cuối của lần đo 2 và 5 phút idle không đổi (±8 KB); mở/đóng lại
  cùng một window 10–30 lần không còn tăng (0–180 KB / 10 lần).
- **Ngân sách < 80 MB chưa đạt** với demo đầy đủ: ~73 MB anon ngay khi khởi động,
  **105–150 MB** sau khi đã mở mọi window. Framework rỗng (1 window, 1 label) là 41 MB.
- `VmRSS` (~330–380 MB) không phải thước đo phù hợp trên NVIDIA: ~150–180 MB là thư
  viện driver map từ file (dùng chung giữa các process). Dùng **RssAnon** (bộ nhớ riêng
  của process) và **PSS**.
- Phần lớn là bộ nhớ đang dùng thật, không phải phân mảnh: `malloc_stats` cho arena
  chính 83 MB "in use" / 87 MB "system"; `malloc_trim(0)` chỉ trả lại 4 MB. Heap Lua chỉ
  2–3 MB (`collectgarbage("count")`).

## Cách đo

Script (trong scratchpad của phiên M6, nội dung tóm tắt ở đây):

1. `cargo build --release`, chạy `hyprwidget run -c examples/demo -i memtest`.
2. Mỗi phút đọc `/proc/<pid>/status` (`VmRSS`, `RssAnon`, `RssFile`, `RssShmem`) và
   `/proc/<pid>/smaps_rollup` (`Pss`, `Pss_Anon`, `Pss_File`), số fd, số thread.
3. Mỗi 30 s mở một window bằng `hyprwidget toggle <window>` ~4 s rồi đóng, xoay vòng
   right-panel, left-panel, user-panel, notification-history, wallpaper-switcher,
   sysmon; mỗi 2 phút mở/đóng launcher; mỗi vòng `hyprwidget request sysmon`.
4. Lần đo 2 chạy dưới `gdb` (do `ptrace_scope = 1`, gdb phải là tiến trình cha) để gọi
   `malloc_stats()` / `malloc_trim(0)` khi nhận `SIGUSR2`, và đo từng window riêng.

## Kết quả

### Lần 1 — 15 phút từ lúc khởi động (RssAnon, MB)

| phút | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| RssAnon | 80.2 | 100.1 | 111.3 | 122.7 | 128.1 | 134.4 | 136.3 | 138.9 | 146.8 | 146.8 | 148.5 | 148.4 | 148.8 | 148.9 | 148.9 | 152.1 |
| PSS | 123.6 | 146.6 | 171.2 | 193.4 | 195.7 | 192.9 | 196.9 | 201.2 | 211.8 | 211.9 | 206.8 | 220.0 | 225.0 | 225.1 | 225.1 | 210.0 |

Vòng xoay 6 window mất 3 phút, nên 3–4 phút đầu là chi phí mở lần đầu. Trong lần này
còn có các thao tác ngoài kịch bản (`hyprwidget eval` dump mọi prop của mọi service,
khởi động cả keystrokes/brightness; 8 lần đổi input method fcitx5 để kiểm tra menu
tray), nên mức cao hơn lần 2. fd dao động 128–256 (GTK/VTE/subprocess), thread 27–34.

### Lần 2 — từng window (RssAnon / `[heap]`, MB)

| Bước | Lần mở 1 | Lần mở 2 |
|---|---|---|
| khởi động | 73.0 / 49.5 | |
| right-panel | +3.1 | +0.2 |
| left-panel | +3.2 | +0.2 |
| user-panel | +6.4 | 0 |
| notification-history | 0 | +0.7 |
| wallpaper-switcher | +8.4 | +6.6 (lần 3–7: +0.02) |
| sysmon (VTE + btop) | +3.9 | 0 |
| app-launcher | +1.0 | −0.2 |
| **tổng** | **99.0** / 74.6 | **106.6** / 82.1 |

Sau đó cùng tiến trình chạy tiếp kịch bản 15 phút: 102.9 → 107.5 (10 phút đầu,
~0.45 MB/phút) → bậc +6.3 ở phút 11 → **113.7 phẳng** 4 phút cuối. Idle 6 phút không
thao tác: 113.75 → 113.76, bậc +4 MB ở phút thứ 6 (poll định kỳ của demo: thời tiết,
crypto…). Rồi mở/đóng mỗi window 10 lần: user-panel +9.3 MB, left-panel +2.5 MB, các
window khác ±0.2 MB; user-panel thêm 3 × 10 lần: +0.18, +0.01, 0 MB → bão hoà.
50 × `request sysmon`: 0.

### Phân bổ

| | RssAnon |
|---|---|
| Framework rỗng, renderer `vulkan` / `gl` / `cairo` | 40.6 / 41.3 / 27.3 MB |
| Demo đầy đủ lúc khởi động, `vulkan` / `cairo` | 73.0 / 58.7 MB |
| Demo sau khi mở mọi window 2 lần, `vulkan` / `cairo` | 106.6 / 85.3 MB |

- **Driver NVIDIA (Vulkan)**: ~14 MB lúc khởi động, ~21 MB sau khi mở mọi window
  (chênh lệch vulkan − cairo).
- **Dựng sẵn mọi window**: ~32 MB (73 − 41). Mọi window `per_monitor` của demo (15
  window, đa số ẩn) được dựng toàn bộ cây widget + style ngay lúc khởi động.
- **Cache sau lần mở đầu**: ~26–40 MB (texture ảnh/thumbnail, glyph, icon theme,
  pipeline GPU, VTE).
- Trong anon: `[heap]` của main thread chiếm ~75% (82 / 107 MB); arena của các thread worker cộng lại < 12 MB.

## Lỗi đã sửa trong M6

**`Gtk-WARNING: duplicate child name in GtkStack: Charset`** (và rò widget kèm theo):
`GtkPopoverMenu` thêm một trang stack cho mỗi submenu nhưng **không gỡ** trang đó khi
*section* chứa submenu bị xoá khỏi model (GTK 4.24). Menu DBusMenu của tray được cập
nhật tại chỗ (`remove_all` + dựng lại) mỗi khi app đổi layout — fcitx5 đổi menu theo
input method của cửa sổ đang focus (Lotus có submenu "Charset") — nên mỗi lần đổi focus
rò một trang và in cảnh báo. Tái hiện bằng script PyGObject: số widget của popover tăng
37 → 47 → 56 → 66 qua mỗi lần dựng lại. Sửa (`crates/hw-services/src/tray/menu.rs`):
mỗi lần layout đổi tạo **model mới** và gán lại cho `MenuButton`/`PopoverMenu` đang gắn
(`DbusMenu::connect_model`), GTK dựng popover mới và giải phóng cái cũ. Kiểm tra: 8 lần
đổi input method khi bar đang chạy → 0 cảnh báo.

Cảnh báo còn lại trong log thuộc GTK, không do hyprwidget:
`gdk_frame_timings_throttling_hint: runtime check failed` (vài lần/phút khi
window hiện/ẩn).

## Chưa làm / hướng giảm

- Chưa chạy đủ **24h**; kết luận "không tăng" dựa trên ~35 phút (2 lần đo) với
  phẳng ở cuối mỗi lần.
- Để xuống dưới 80 MB với demo đầy đủ:
  1. **Dựng window lười**: window `visible = false` chỉ dựng cây widget khi hiện lần
     đầu (tiết kiệm phần lớn ~32 MB lúc khởi động); tuỳ chọn huỷ cây khi ẩn lâu.
  2. Wallpaper switcher (+15 MB qua hai lần mở đầu): mỗi lần `on_show` nó dựng lại các
     `Picture` dùng CSS `background-image` của file thumbnail; nên đo lại với
     `W.Picture` (texture giải phóng cùng widget) thay cho ảnh nền CSS.
  3. `GSK_RENDERER=cairo` cho cấu hình ít animation: −15 đến −21 MB anon, đổi lại
     vẽ bằng CPU.
- Lần đo 1 cao hơn lần 2 ~35–40 MB vì các thao tác ngoài kịch bản (khởi động thêm
  service, dump dữ liệu qua `eval`); chưa tách riêng từng nguồn.
