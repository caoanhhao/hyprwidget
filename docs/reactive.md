# Reactive API

Everything that changes over time (state, options, settings, services) is a
**Signal**. Assign a Signal to a widget prop and the widget updates itself; no
callbacks needed. Equivalents in gnim/AGS:

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
count:get()          -- 0 (read, tracks the dependency)
count()              -- same as :get()
count:peek()         -- read WITHOUT tracking
count:set(1)         -- notifies subscribers if the value changed
count:toggle()       -- set(not value)
local label = count:map(function(n) return "count: " .. n end)  -- new Signal
local unsub = count:subscribe(function(v) print(v) end)          -- returns an unsubscribe function
unsub()
tostring(count)      -- "Signal(1)"
hw.is_signal(count)  -- true
```

- `set` only notifies when the value differs from the old one. Tables compare by
  **reference**: mutating a table and `set`ting the same table again does not
  notify — create a new table.
- Option and service Signals are **read-only** (`set` raises an error). Signals
  from `hw.settings` are writable (they write to `settings.json`).
- Subscriptions made from Lua (`subscribe`, widget binds) are dropped on Lua
  reload, so subscribers do not leak across config edits.

### `hw.computed(fn)`

`fn` runs immediately and again whenever a Signal it **reads** (`s()` / `s:get()`)
changes. Dependencies are collected again after every run, so `if` branches work
too. Use `:peek()` to read without depending.

```lua
local hypr = hw.service.hyprland
local title = hw.computed(function()
  local c = hypr.focused_client()
  return c and c.title or hw.settings["bar.placeholder"]()
end)
```

### `hw.bind(obj [, key])`

- `hw.bind(signal)` → that signal.
- `hw.bind(hw.settings, "bar.lock")` → the settings signal (writable).
- `hw.bind(table, key)` → `table[key]` if it is a signal (services: `hw.bind(hw.service.hyprland, "workspaces")`
  = `hw.service.hyprland.workspaces`); if the table has a `bind` function, calls
  `table:bind(key)`; otherwise wraps the static value in a `hw.state`.

### `hw.poll(ms, cmd_or_fn [, transform])`

```lua
local time = hw.poll(1000, "date +%H:%M")             -- stdout, trailing \n stripped
local up = hw.poll(60000, { "uptime", "-p" })          -- argv as a list
local load = hw.poll(2000, function() return hw.exec("cat /proc/loadavg") end)
local n = hw.poll(5000, "ls /tmp", function(out, prev) return select(2, out:gsub("\n", "")) + 1 end)
```

With a command: runs asynchronously every `ms` (the first run is immediate), errors
are logged; the initial value is `nil` until the first command finishes. With a
function: called synchronously, the value is available at once. Plugins without
the `exec` permission can only use the function form.

## Dynamic widgets: `hw.With`, `hw.For`, signals as children

```lua
-- One child rebuilt from fn(value) whenever the signal changes (nil/false = nothing).
hw.With(hw.service.mpris.players, function(players)
  if #players == 0 then return W.Label("no player") end
  return W.Label(players[1].title)
end)

-- Keyed list: widgets of items whose key is unchanged are kept, not rebuilt.
hw.For(hw.service.hyprland.workspaces, function(ws, i)
  return W.Button { label = tostring(ws.id), on_click = function() … end }
end, function(ws) return ws.id end)

-- There is no 4th argument; to change the wrapping Box, use the table form:
{ type = "For", each = sig, fn = fn, key = key_fn, vertical = true, spacing = 4 }
```

- Both return a `Box` (class `hw-with` / `hw-for`), horizontal by default. `W.With` /
  `W.For` are aliases.
- Without `key`, string/number/boolean items use their own value as key, and table
  items use their position.
- Putting a signal directly where a child goes (`W.Box { sig }`, slot `child = sig`)
  is the same as `hw.With(sig)`: the value is built into a widget (string/number →
  `Label`).
- `children = sig` (a signal of a list) on Box/Grid/FlowBox/Stack/Overlay: rebuilds
  all children on every change (no keys).

## Timers

```lua
local t = hw.timeout(500, function() … end)   -- once
local i = hw.interval(1000, function() … end) -- runs once immediately, then every 1000 ms
t:cancel(); i:cancel()
hw.now()                                      -- seconds (float), monotonic clock
```

Timers, processes and file watches belong to the current Lua state: they are
cancelled on reload.

## Processes

```lua
hw.exec("hyprctl -j monitors")             -- synchronous: stdout, raises if exit != 0 (avoid in UI code)
hw.exec({ "notify-send", "hi" })           -- argv as a list, no shell

hw.exec_async("curl -s wttr.in?format=3", function(out, err) … end)
local out = hw.exec_async("sleep 1; echo done")   -- yields: only inside a coroutine (see below)

local p = hw.subprocess({ "cava", "-p", conf }, function(line) … end, function(err_line) … end)
p:write("q\n"); p:signal(15); p:kill(); p:pid()
```

String commands are split with shell syntax (`glib::shell_parse_argv`) but are
**not** run through a shell; for pipes/redirects use `{ "sh", "-c", "a | b" }`.

### Coroutines

Event handlers, `setup`, request handlers, timer/subscribe callbacks and
`hw.async(fn, ...)` run inside a coroutine, so `hw.exec_async(cmd)` without a
callback can "wait" there without blocking the UI:

```lua
W.Button {
  label = "IP",
  on_click = function(self)
    local ip = hw.exec_async("curl -s ifconfig.me")   -- the UI keeps running
    self:set("label", ip)
  end,
}
hw.async(function() print(hw.exec_async("uname -r")) end)
```

At the top level of a config file (not a coroutine) you must pass a callback.

## Misc

```lua
hw.json.decode('{"a": 1}')          -- table
hw.json.encode({ a = 1 }, true)     -- string (pretty = true)
hw.watch_file(path, function() … end)  -- create/modify/delete/rename; returns a handle with :cancel()
hw.monitors                         -- Signal: list of monitors (debounced 300 ms) — see widgets.md
```
