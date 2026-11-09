# Reactive API

Mọi dữ liệu thay đổi theo thời gian (state, option, settings, service) là
**Signal**. Gán một Signal vào prop của widget thì widget tự cập nhật; không cần
viết callback. Tương đương gnim/AGS:

| AGS / gnim | hyprwidget |
|---|---|
| `createState(v)` | `hw.state(v)` |
| `createComputed(fn)` | `hw.computed(fn)` |
| `createBinding(obj, "prop")` | `hw.bind(obj, "prop")` |
| `createPoll(ms, cmd)` | `hw.poll(ms, cmd)` |
| `accessor.as(fn)` | `s:map(fn)` |
| `<With value=…>` / `<For each=…>` | `hw.With(sig, fn)` / `hw.For(sig, fn, key)` |
| `execAsync` / `subprocess` | `hw.exec_async(cmd)` / `hw.subprocess(cmd, on_line)` |
| `timeout` / `interval` | `hw.timeout(ms, fn)` / `hw.interval(ms, fn)` |

## Signal

```lua
local count = hw.state(0)
count:get()          -- 0 (đọc, có track dependency)
count()              -- như :get()
count:peek()         -- đọc, KHÔNG track
count:set(1)         -- báo subscriber nếu giá trị đổi
count:toggle()       -- set(not value)
local label = count:map(function(n) return "count: " .. n end)  -- Signal mới
local unsub = count:subscribe(function(v) print(v) end)          -- trả về hàm huỷ
unsub()
tostring(count)      -- "Signal(1)"
hw.is_signal(count)  -- true
```

- `set` chỉ báo khi giá trị khác cũ. Table so sánh theo **tham chiếu**: sửa một
  table rồi `set` lại chính nó sẽ không báo — hãy tạo table mới.
- Signal của option và của service là **chỉ đọc** (`set` báo lỗi). Signal từ
  `hw.settings` thì ghi được (ghi vào `settings.json`).
- Subscription tạo trong Lua (`subscribe`, widget bind) bị huỷ khi Lua reload,
  nên không rò subscriber qua các lần sửa config.

### `hw.computed(fn)`

`fn` chạy ngay và chạy lại mỗi khi một Signal nó **đọc** (`s()` / `s:get()`)
thay đổi. Dependency được thu thập lại sau mỗi lần chạy, nên nhánh `if` cũng đúng.
Dùng `:peek()` để đọc mà không phụ thuộc.

```lua
local hypr = hw.service.hyprland
local title = hw.computed(function()
  local c = hypr.focused_client()
  return c and c.title or hw.settings["bar.placeholder"]()
end)
```

### `hw.bind(obj [, key])`

- `hw.bind(signal)` → chính signal đó.
- `hw.bind(hw.settings, "bar.lock")` → signal của settings (ghi được).
- `hw.bind(table, key)` → `table[key]` nếu là signal (service: `hw.bind(hw.service.hyprland, "workspaces")`
  = `hw.service.hyprland.workspaces`); nếu table có hàm `bind` thì gọi
  `table:bind(key)`; nếu không, bọc giá trị tĩnh thành `hw.state`.

### `hw.poll(ms, cmd_or_fn [, transform])`

```lua
local time = hw.poll(1000, "date +%H:%M")             -- stdout, bỏ \n cuối
local up = hw.poll(60000, { "uptime", "-p" })          -- argv dạng list
local load = hw.poll(2000, function() return hw.exec("cat /proc/loadavg") end)
local n = hw.poll(5000, "ls /tmp", function(out, prev) return select(2, out:gsub("\n", "")) + 1 end)
```

Với lệnh: chạy async mỗi `ms` (lần đầu chạy ngay), lỗi được log; giá trị ban đầu là
`nil` cho tới khi lệnh đầu tiên xong. Với hàm: gọi đồng bộ, giá trị có ngay. Plugin
không có quyền `exec` chỉ dùng được dạng hàm.

## Widget động: `hw.With`, `hw.For`, signal làm child

```lua
-- Một child dựng lại từ fn(value) mỗi khi signal đổi (nil/false = không có gì).
hw.With(hw.service.mpris.players, function(players)
  if #players == 0 then return W.Label("không có player") end
  return W.Label(players[1].title)
end)

-- Danh sách có key: widget của item còn key cũ được giữ lại, không dựng lại.
hw.For(hw.service.hyprland.workspaces, function(ws, i)
  return W.Button { label = tostring(ws.id), on_click = function() … end }
end, function(ws) return ws.id end)

-- Tham số thứ 4 trở đi không có; muốn đổi Box bọc ngoài thì dùng dạng table:
{ type = "For", each = sig, fn = fn, key = key_fn, vertical = true, spacing = 4 }
```

- Cả hai trả về một `Box` (class `hw-with` / `hw-for`), mặc định ngang. `W.With` /
  `W.For` là alias.
- Không có `key` thì item kiểu string/number/boolean dùng chính giá trị làm key,
  item kiểu table dùng vị trí.
- Gán thẳng signal vào chỗ của một child (`W.Box { sig }`, slot `child = sig`)
  tương đương `hw.With(sig)`: giá trị được dựng thành widget (string/number →
  `Label`).
- `children = sig` (signal của list) trên Box/Grid/FlowBox/Stack/Overlay: dựng lại
  toàn bộ children mỗi lần đổi (không có key).

## Timer

```lua
local t = hw.timeout(500, function() … end)   -- một lần
local i = hw.interval(1000, function() … end) -- chạy ngay một lần, rồi mỗi 1000 ms
t:cancel(); i:cancel()
hw.now()                                      -- giây (float), đồng hồ monotonic
```

Timer, process và file watch thuộc về Lua state hiện tại: bị huỷ khi reload.

## Process

```lua
hw.exec("hyprctl -j monitors")             -- đồng bộ: stdout, raise nếu exit != 0 (tránh dùng trong UI)
hw.exec({ "notify-send", "hi" })           -- argv dạng list, không qua shell

hw.exec_async("curl -s wttr.in?format=3", function(out, err) … end)
local out = hw.exec_async("sleep 1; echo xong")   -- yield: chỉ trong coroutine (xem dưới)

local p = hw.subprocess({ "cava", "-p", conf }, function(line) … end, function(err_line) … end)
p:write("q\n"); p:signal(15); p:kill(); p:pid()
```

Lệnh dạng string được tách theo cú pháp shell (`glib::shell_parse_argv`) nhưng
**không** chạy qua shell; cần pipe/redirect thì dùng `{ "sh", "-c", "a | b" }`.

### Coroutine

Event handler, `setup`, request handler, callback của timer/subscribe và
`hw.async(fn, ...)` chạy trong coroutine, nên `hw.exec_async(cmd)` không callback
"chờ" được ở đó mà không chặn UI:

```lua
W.Button {
  label = "IP",
  on_click = function(self)
    local ip = hw.exec_async("curl -s ifconfig.me")   -- UI vẫn chạy
    self:set("label", ip)
  end,
}
hw.async(function() print(hw.exec_async("uname -r")) end)
```

Gọi ở top-level của file config (không phải coroutine) thì phải truyền callback.

## Khác

```lua
hw.json.decode('{"a": 1}')          -- table
hw.json.encode({ a = 1 }, true)     -- string (pretty = true)
hw.watch_file(path, function() … end)  -- tạo/sửa/xoá/đổi tên; trả về handle có :cancel()
hw.monitors                         -- Signal: list monitor (debounce 300 ms) — xem widgets.md
```
