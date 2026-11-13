# Widgets

```lua
local W = hw.widget   -- also hw.W
W.Box {
  class = "bar",            -- prop
  spacing = 8,
  on_click = function(self, button) end,   -- event
  setup = function(self) end,              -- runs once after building
  W.Label("hello"),         -- positional child
  W.Label { label = hw.poll(1000, "date +%T") },   -- a Signal prop → updates itself
}
```

A widget is a Lua table `{ type = "Box", ... }`; `W.Box{…}` only sets `type`.
`W.X("text")` (or `W.X(signal)`) is shorthand for `W.X{ label = … }`. Anywhere a
widget is accepted you can also pass: a string/number (→ `Label`), a Signal (→ a
child rebuilt on change, see
[reactive.md](reactive.md#dynamic-widgets-hwwith-hwfor-signals-as-children)),
`nil`/`false` (→ skipped), or an already-built widget (`self`).

Every prop takes a static value **or** a Signal. Props not in the tables below are
forwarded to the **GObject property of the same name** (`_` → `-`), e.g.
`W.Picture { content_fit = "cover" }`, `W.Label { lines = 2 }`; enums take their
nick (`"cover"`), and a wrong name is an error.

## Common props (every widget)

| Prop | Type | |
|---|---|---|
| `class` / `class_name` | `"a b"` or `{"a","b"}` | CSS classes (added to the built-in `hw-<kind>`); changing classes through a Signal replaces exactly the old set |
| `name` | string | widget name (CSS `#name`; for a Window, its registry name) |
| `visible` | bool | |
| `tooltip` / `tooltip_text`, `tooltip_markup` | string | |
| `sensitive`, `hexpand`, `vexpand`, `focusable` / `can_focus`, `can_target` | bool | |
| `halign`, `valign` | `"start"`/`"left"`/`"top"`, `"end"`/`"right"`/`"bottom"`, `"center"`, `"fill"`, `"baseline"` | |
| `width` / `width_request`, `height` / `height_request` | int | minimum size (DrawingArea: content size) |
| `margin` | `n`, `"t r b l"`, `{t, r, b, l}`, `{top=…, right=…}` | like the CSS shorthand |
| `opacity` | number | |
| `cursor` | string | CSS cursor name (`"pointer"`) |
| `widget_name` | string | |
| `css` | string or Signal | inline CSS for this widget only: `css = "padding: 4px;"` |
| `theme` | table or Signal | overrides Hyprland options for the widget: `theme = { decoration = { rounding = 0 } }` → the widget's `--decoration-rounding` variable |
| `popover` | `W.Popover{…}` | popover attached to any widget; open/close it with the popover's `open` prop |
| `setup` | `function(self)` | runs after building (inside a coroutine) |

Children of a Grid also take `col`, `row`, `col_span`, `row_span`; in a Stack, a
child's `name` is the page name.

## Events

Handlers receive `self` (the widget) and then the arguments. Return `true` to stop
the event from propagating (`on_scroll`, `on_key`). Handlers run inside a
coroutine (they can call `hw.exec_async(cmd)`).

| Event | Arguments | Widgets |
|---|---|---|
| `on_click` | `button` (1 left, 2 middle, 3 right) | every widget (Button: normal click + other buttons) |
| `on_scroll` | `dx, dy` | every widget |
| `on_hover` | `x, y` | every widget (pointer enters) |
| `on_leave` | | every widget |
| `on_motion` | `x, y` | every widget |
| `on_key` | `name, code, mods` (`mods` = list of `"shift"`,`"ctrl"`,`"alt"`,`"super"`) | focused widget |
| `on_change` | new value | Entry (text), Slider (number), Switch (bool), Stack (page name), Revealer (finished revealing), Calendar (`"YYYY-MM-DD"`), Popover (open), MenuButton (open) |
| `on_activate` | text | Entry (Enter) |
| `on_show` | `true` | every widget (map: window shown, revealer opened…) |
| `on_hide` | `false` | every widget (unmap) |
| `on_destroy` | | every widget (`self` is `nil` once the widget is already disposed, e.g. removed by `hw.For`) |
| `on_title` | title | App |
| `on_exit` | exit code | App, Terminal |

Registering after building: `self:on("on_click", fn)`.

## `self` (the widget in `setup` / handlers)

| | |
|---|---|
| `self.kind`, `self.name` | `"Box"`, the widget name |
| `self:set(prop, value)` | set a prop as in the table |
| `self:get(prop)` | read a GObject property (`self:get("label")`) |
| `self:add_class(c)`, `remove_class(c)`, `toggle_class(c [, on])`, `has_class(c)` | |
| `self:show()`, `hide()`, `is_visible()`, `queue_draw()`, `grab_focus()` | |
| `self:on(event, fn)` | |
| `self:parent()` | parent widget (without `kind`) |
| `self:window()` | the window containing the widget (see [Window](#window)) |
| `self.pid`, `self.title`, `self.app_id`, `self:send_close()`, `self:restart()`, `self:focus()` | `W.App` only |

## Built-in widgets

Children: **n** = many positional children / `children = {…}` (or a Signal of a
list); a slot name = one child (the first positional child fills `child`).

| Widget | GTK | Children | Own props |
|---|---|---|---|
| `Window` | ApplicationWindow + layer-shell | `child` | see [Window](#window) |
| `Box` | Box | n | `vertical`, `spacing` (default `general:gaps_in`), `homogeneous` |
| `CenterBox` | CenterBox | `start`, `center`, `end` | `vertical` |
| `Label` | Label | | `label`/`text`, `markup`, `use_markup`, `wrap`, `xalign`, `yalign`, `max_width_chars`, `width_chars`, `selectable`, `ellipsize` (`true`, `"start"`/`"middle"`/`"end"`/`"none"`), `justify` (`"left"`/`"right"`/`"center"`/`"fill"`) |
| `Button` | Button | `child` | `label`, `icon` (icon name), `has_frame` |
| `Image` | Image | | `icon`/`icon_name`, `file`, `pixel_size`/`size` |
| `Icon` | Image | | `icon`/`icon_name`, `pixel_size`/`size` |
| `Picture` | Picture | | `file`; `content_fit` (`"fill"`/`"contain"`/`"cover"`/`"scale_down"`), `can_shrink` via GObject |
| `Revealer` | Revealer | `child` | `reveal`/`reveal_child`, `transition` (`none`, `crossfade`/`fade`, `slide_{left,right,up,down}`, `swing_{left,right,up,down}`), `duration`/`transition_duration` (ms, defaults to `animations:animation:layersIn`) |
| `Stack` | Stack | n (child `name` = page name) | `shown`/`visible_child_name`, `transition` (also `slide_left_right`, `slide_up_down`, `over_*`, `rotate_left/right`), `duration`, `hhomogeneous`, `vhomogeneous` |
| `Scrolled` | ScrolledWindow | `child` | `hscroll`, `vscroll` (`"never"`/`"auto"`/`"always"`/`"external"`), `propagate_natural_width`, `propagate_natural_height` |
| `Entry` | Entry | | `text`, `placeholder`/`placeholder_text`, `password`, `position`/`cursor_position` (-1 = end) |
| `Slider` | Scale | | `value`, `min`, `max`, `step`, `draw_value`, `digits`, `vertical`, `inverted`, `scroll` (false: the mouse wheel scrolls the parent instead of changing the value) |
| `Switch` | Switch | | `active` |
| `Progress` | ProgressBar | | `value`/`fraction` (0..1), `vertical`, `inverted`, `text` |
| `Overlay` | Overlay | `child` (background, decides the size) + n overlays | |
| `Grid` | Grid | n (`col`,`row`,`col_span`,`row_span`; or auto-placed by `columns`) | `columns`, `spacing`, `row_spacing`, `column_spacing`, `homogeneous` |
| `FlowBox` | FlowBox | n | `max_per_line`, `min_per_line`, `homogeneous`, `spacing` |
| `MenuButton` | MenuButton | `child`, `popover` | `label`, `icon`, `always_show_arrow`, `active` |
| `Popover` | Popover | `child` | `has_arrow`, `autohide`, `open` |
| `DrawingArea` | DrawingArea | | `width`/`content_width`, `height`/`content_height`, `redraw` (value change → redraw), `draw = function(cr, w, h)` |
| `Calendar` | Calendar | | `date` (`"YYYY-MM-DD"`), `show_heading`, `show_day_names`, `show_week_numbers` |
| `Terminal` | VTE4 Terminal | | see [W.Terminal](#wterminal) |
| `App` | `hw-embed` EmbedView | | see [W.App](#wapp) |

`Switch` and `Progress` default to `valign = "center"` (there is no GTK theme, so
nothing stretches them). `Overlay`: positional children are **overlays** and do not
grow the widget — set the background with `child = …`.

### `DrawingArea` and cairo

```lua
W.DrawingArea {
  width = 120, height = 32,
  redraw = hw.service.sysinfo.cpu,          -- redraw when the CPU changes
  draw = function(cr, w, h)                 -- initial source color = CSS `color`
    cr:set_line_width(2)
    cr:rounded_rectangle(0, 0, w, h, 6)     -- (x, y, w, h, radius)
    cr:stroke()
  end,
}
```

Methods of `cr`: `set_source_rgba(r,g,b,a)`, `set_source_rgb`, `set_source_color(css)`,
`set_line_width`, `set_line_cap("butt"|"round"|"square")`, `move_to`, `line_to`,
`rel_line_to`, `curve_to`, `rectangle`, `rounded_rectangle(x, y, w, h, r)`,
`arc(xc, yc, r, a1, a2)`, `arc_negative`, `new_path`, `close_path`, `fill`,
`fill_preserve`, `stroke`, `stroke_preserve`, `paint([alpha])`, `clip`, `save`,
`restore`, `translate`, `scale`, `rotate`, `select_font_face(family [, bold])`,
`set_font_size`, `show_text`,
`text_extents(text)` → `{width, height, x_bearing, y_bearing, x_advance}`.

### `W.Terminal`

A lightweight VTE4 terminal that runs TUIs (btop, cava, yazi, a shell) inside a
widget — no nested compositor needed.

```lua
W.Terminal {
  command = { "btop" },            -- list or shell string; defaults to $SHELL
  cwd = "~", env = { TERM_PROGRAM = "hyprwidget" },
  font = "JetBrainsMono Nerd Font 10", font_scale = 1.0,
  size = { cols = 110, rows = 30 }, -- or { 110, 30 }
  colors = { foreground = "#ccc", background = "#111", cursor = "#fff", palette = { … 16 colors } },
  transparent = false, scrollback = 10000, cursor_blink = true,
  input = true, bold_is_bright = false,
  restart = "never",               -- "on-exit" | "on-failure"
  on_exit = function(self, status) end,
}
```

- The child process starts when the widget is first realized: a hidden window =
  not started yet.
- `colors` default to `hw.theme.colors` (switching themes recolors the terminal);
  `palette` is a list of 16 colors or the keys `color0`..`color15`.
- `self:set("send", "q")` writes to the program's stdin; `self:set("restart_now", true)`
  starts it again after it exited.
- The window holding a terminal needs `keymode = "on_demand"` for typing.

### `W.App`

Runs a Linux app **inside** a widget through a nested Wayland compositor
(`hw-embed`); technical details, the app matrix and limits: [embed.md](embed.md).

```lua
W.App {
  command = { "pavucontrol" },     -- list, or a string (run with sh -c)
  env = { GTK_THEME = "Adwaita:dark" },
  size = { 420, 320 },             -- or { width =, height = }; nil = follow GTK layout
  mode = "resize",                 -- "resize": the app draws at the widget's size | "scale"
  keep_alive = true,               -- keep the app while the widget is hidden
  restart = "on-exit",             -- "never" | "on-exit" | "on-failure" | true/false
  frame_when_hidden = false,       -- stop frame callbacks while not visible
  socket = false,                  -- true: its own WAYLAND_DISPLAY (apps that fork: mpv, firefox)
  rounding = 12,                   -- defaults to decoration:rounding; 0/false = no clipping (zero-copy)
  on_title = function(self, title) end,
  on_exit = function(self, code) end,
}
```

In `setup`/handlers: `self.pid`, `self.title` / `self.app_id` (Signals),
`self:send_close()`, `self:restart()`, `self:focus()`. The layer surface switches to
`keymode = "on_demand"` by itself so the app gets the keyboard. Single-instance apps
(Firefox, nautilus…) need their own profile/instance.

## Window

```lua
W.Window {
  name = "bar-" .. monitor.name,    -- registry name (hyprwidget toggle <name>)
  namespace = "bar",                -- layer namespace (default = name); used by layer rules
  monitor = monitor,                -- a Monitor, a connector "DP-2", or nil (compositor picks)
  layer = "top",                    -- "background" | "bottom" | "top" | "overlay"
  anchor = { "top", "left", "right" },   -- or "top left right"
  exclusivity = "exclusive",        -- "exclusive" | "normal" | "ignore"; or exclusive_zone = n
  margin = 10,                      -- like CSS; defaults to general:gaps_out
  keymode = "on_demand",            -- "none" | "exclusive" | "on_demand" (keyboard_mode)
  visible = true,                   -- shown by default; can be a Signal
  default_width = 400, default_height = 300, title = "…",
  W.CenterBox { … },                -- child
}
```

- `W.Window` is only used at the top level: returned from `hyprwidget.lua`, from a
  `per_monitor` function, or created with `hw.window.new{…}`.
- A window that starts hidden (`visible = false` or a Signal that is `false`) only
  builds its children the first time it is shown (`hyprwidget toggle`, `:show()`,
  the `visible` Signal turning `true`); until then `:widget()` has no children, and
  errors building them are logged when it is shown (`docs/memory.md`).
- Every namespace gets a blur layer rule following `decoration:blur:*`
  automatically (disable with `"layer_rules": false` in `config.json`); add your own
  rules with `hw.layer_rule{ match = { namespace = "bar" }, … }` (sends
  `hl.layer_rule(...)` to Hyprland 0.57).

| API | |
|---|---|
| `hw.per_monitor(function(monitor) return { win1, win2 } end)` | creates windows per monitor; creates/destroys them when screens are plugged/unplugged (debounced 300 ms, skips the `FALLBACK` connector) |
| `hw.monitors` | Signal list of Monitors: `name`/`connector`, `description`, `manufacturer`, `model`, `x`, `y`, `width`, `height`, `scale`, `refresh_rate` |
| `hw.window.get(name)` | a Window or nil: `:show()`, `:hide()`, `:toggle()` → visible, `:is_visible()`, `.visible`, `:set(prop, v)`, `:widget()`, `:destroy()` |
| `hw.window.new{…}` | creates a window at runtime (nil when headless) |
| `hw.window.list()` | window names |
| `hw.window.toggle(name)` | → the new visibility, nil if there is no such window |

## `hw.define`: custom widgets

```lua
hw.define("Weather", function(props)
  return W.Box {
    class = "weather " .. (props.class or ""),
    W.Label { label = hw.poll(600000, "curl -s 'wttr.in?format=%t'") },
    W.Label(props.city or ""),
  }
end)

W.Weather { city = "Hanoi" }              -- Lua
{ "type": "Weather", "city": "Hanoi" }   -- JSON layout
```

The function receives the whole props table (positional children included) and
returns a widget. The name must not clash with a built-in widget. Plugins declare
widgets differently (see [plugins.md](plugins.md)).

## Layout JSON

The same node tree as Lua, so users can edit layouts without writing Lua:

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
W.Window { name = "panel", hw.layout("layouts/panel.json") }   -- path relative to the config directory
```

- `{"$bind": "settings.<key>"}` → a settings Signal (writable); `"option.<name>"` /
  `"opt.<name>"` → an option Signal (read-only).
- Grouped per node: `"$bind": { "<prop>": "<source>", … }`.
- `type` is a built-in widget, a `hw.define` widget or a plugin widget. Events and
  functions (`setup`, `draw`) can only be written in Lua: wrap the JSON layout in a
  `hw.define` widget when you need them.
