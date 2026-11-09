# CLI và IPC

```
hyprwidget [-c DIR] [-i INSTANCE] <command>
```

| Option | |
|---|---|
| `-c, --config DIR` | thư mục config (`run`, `check`, `option` khi không có daemon); mặc định `$HYPRWIDGET_CONFIG`, rồi `$XDG_CONFIG_HOME/hyprwidget` |
| `-i, --instance NAME` | tên instance (mặc định `default`, hoặc `$HYPRWIDGET_INSTANCE`) |

| Lệnh | |
|---|---|
| `run` | chạy daemon |
| `request <name> [args…]` | gọi handler `hw.on_request(name, fn)` hoặc `requests` của plugin; in kết quả |
| `toggle <window>` | hiện/ẩn window theo tên registry; in `shown`/`hidden` |
| `reload [css\|lua]` | nạp lại stylesheet, Lua, hoặc cả hai |
| `eval '<lua>'` | chạy Lua trong daemon; thử như biểu thức trước (`eval 1+1` → `2`), rồi như câu lệnh; in giá trị trả về |
| `list` | window (`tên<TAB>visible\|hidden`) và `request:<tên>` |
| `option <name>` | giá trị đã gộp của một option (`decoration:rounding`); không có daemon thì tự tính từ config |
| `theme [name\|none]` | đổi theme; không tham số = liệt kê (`*` = đang dùng) |
| `check` | kiểm tra config (Lua, option, manifest plugin/theme, SCSS) không mở window; lỗi in `file:dòng: …`, exit 1 |
| `inspect` | mở GTK Inspector |
| `quit` | dừng daemon |

Daemon log ra stderr (mức `info`, đổi bằng `RUST_LOG`, ví dụ
`RUST_LOG=hw_lua=debug,info`). Các lệnh khác chỉ in cảnh báo. Renderer GSK mặc định
`vulkan` nếu `GSK_RENDERER` chưa đặt ([renderer.md](renderer.md)).

## Request handler

```lua
-- hyprwidget request translate DP-2 "xin chào"
hw.on_request("translate", function(args)        -- args = { "DP-2", "xin chào" }
  local out = hw.exec_async({ "trans", "-b", args[2] })   -- được phép yield
  return out                                      -- in ra stdout của lệnh request
end)
```

Lỗi trong handler → lệnh `request` in lỗi ra stderr và exit 1. Đặt lại cùng tên sẽ
thay handler cũ (handler bị xoá khi reload Lua). Keybind Hyprland gọi
`hyprwidget request <name> …` thay cho `ags request <name> …` như trước.

## Giao thức IPC

Socket Unix `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`. Mỗi kết nối: client gửi
**một dòng JSON** `{"cmd": "<lệnh>", "args": ["…"]}`, daemon trả **một dòng JSON**
`{"ok": true|false, "output": "…"}` rồi đóng. `cmd` là tên lệnh ở bảng trên
(`request` nhận tên handler là `args[0]`).

```sh
printf '%s\n' '{"cmd":"toggle","args":["bar-DP-2"]}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprwidget/default.sock
```
