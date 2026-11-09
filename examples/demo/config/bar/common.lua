-- Small building blocks shared by the bar modules (AGS: widgets/CustomRevealer,
-- widgets/Custom/Eventbox, widgets/CircularProgress, variables.ts).
local W = hw.widget
local M = {}

-- GTK boxes with AGS' default spacing (0) instead of general:gaps_in.
function M.Box(props)
  if props.spacing == nil then
    props.spacing = 0
  end
  return W.Box(props)
end
local Box = M.Box

-- AGS `globalTransition` (ms).
M.TRANSITION = 300

function M.dispatch(lua_code)
  hw.service.hyprland:dispatch(lua_code)
end

-- Fire-and-forget command (AGS execAsync(...).catch(notify)).
function M.spawn(cmd)
  hw.exec_async(cmd, function(_, err)
    if err then
      print("bar: " .. tostring(cmd) .. ": " .. err)
    end
  end)
end

-- Hover popover: opens while the pointer is on `owner` or on the popover, with
-- a grace delay to move between them (AGS Workspaces/ResourceMonitor). Returns
-- the props to merge into the owner widget. `on_active(bool)` mirrors AGS'
-- onActivate (keeps the auto-hiding bar up while a popover is open).
function M.hover_popover(child, opts)
  opts = opts or {}
  local open = hw.state(false)
  local timer
  local function cancel()
    if timer then
      timer:cancel()
      timer = nil
    end
  end
  local function set(v)
    if open:peek() ~= v then
      open:set(v)
      if opts.on_active then
        opts.on_active(v)
      end
    end
  end
  return {
    on_hover = function()
      cancel()
      set(true)
    end,
    on_leave = function()
      cancel()
      timer = hw.timeout(opts.delay or 200, function()
        timer = nil
        set(false)
      end)
    end,
    popover = W.Popover({
      class = opts.class,
      has_arrow = true,
      autohide = false,
      position = "bottom",
      open = open,
      on_hover = cancel,
      on_leave = function()
        set(false)
      end,
      child,
    }),
  }
end

-- Popover whose open state is reported to `on_active` and that gets the
-- `popover-open` class while visible (fade-in animation in lib.scss).
function M.popover(child, on_active, props)
  props = props or {}
  props[1] = child
  props.on_change = function(self, visible)
    self:toggle_class("popover-open", visible)
    if on_active then
      on_active(visible)
    end
  end
  return W.Popover(props)
end

-- AGS CustomRevealer: `trigger` always shown, `child` slides in on hover.
function M.custom_revealer(opts)
  local reveal = hw.state(false)
  return Box({
    class = "custom-revealer " .. (opts.class or ""),
    tooltip = opts.tooltip,
    visible = opts.visible,
    on_hover = function()
      reveal:set(true)
    end,
    on_leave = function()
      reveal:set(false)
    end,
    on_click = opts.on_click,
    Box({
      class = "content",
      opts.trigger,
      W.Revealer({
        reveal = reveal,
        duration = M.TRANSITION,
        transition = opts.transition or "swing_left",
        opts.child,
      }),
    }),
  })
end

-- AGS Volume/Brightness: icon + percentage, and a slider revealed on hover
-- or for 2 s after the value changes.
function M.slider_revealer(opts)
  local reveal = hw.state(false)
  local hovering = false
  local timer
  local function hide_later()
    if timer then
      timer:cancel()
    end
    timer = hw.timeout(2000, function()
      timer = nil
      if not hovering then
        reveal:set(false)
      end
    end)
  end
  local last = opts.value:peek()
  opts.value:subscribe(function(v)
    if v == nil or v == last then
      return
    end
    last = v
    reveal:set(true)
    hide_later()
  end)
  return Box({
    class = "custom-revealer",
    tooltip = opts.tooltip,
    visible = opts.visible,
    on_hover = function()
      hovering = true
      if timer then
        timer:cancel()
        timer = nil
      end
      reveal:set(true)
    end,
    on_leave = function()
      hovering = false
      hide_later()
    end,
    on_click = opts.on_click,
    Box({
      class = "content",
      Box({ class = "trigger", spacing = 5, opts.icon, opts.label }),
      W.Revealer({
        reveal = reveal,
        duration = M.TRANSITION,
        transition = "swing_left",
        W.Slider({
          class = "slider",
          width_request = 100,
          draw_value = false,
          value = opts.value:map(function(v)
            v = tonumber(v) or 0
            return math.max(0, math.min(1, v))
          end),
          on_change = function(_, v)
            if math.abs(v - (tonumber(opts.value:peek()) or 0)) > 0.005 then
              opts.on_change(v)
            end
          end,
        }),
      }),
    }),
  })
end

-- AGS CircularProgress: a ring (track + value arc in the CSS color) with an
-- icon in the middle. `value` is a 0..1 signal.
function M.circular_progress(opts)
  local size = opts.size or 20
  return Box({
    class = "circular-progress " .. (opts.class or ""),
    tooltip = opts.tooltip,
    visible = opts.visible,
    W.Overlay({
      child = W.DrawingArea({
        width = size,
        height = size,
        redraw = opts.value,
        draw = function(cr, w, h)
          local cx, cy = w / 2, h / 2
          local r = math.min(w, h) / 2 - 1
          cr:set_line_width(3)
          cr:save()
          cr:set_source_rgba(0.5, 0.5, 0.5, 0.3)
          cr:arc(cx, cy, r, 0, 2 * math.pi)
          cr:stroke()
          cr:restore()
          local v = math.max(0, math.min(1, opts.value:peek() or 0))
          cr:new_path()
          cr:arc(cx, cy, r, -math.pi / 2, -math.pi / 2 + v * 2 * math.pi)
          cr:stroke()
        end,
      }),
      Box({
        halign = "center",
        valign = "center",
        hexpand = true,
        vexpand = true,
        W.Label({ class = "circular-progress-icon", label = opts.icon }),
      }),
    }),
  })
end

-- AstalWp's volume icon names.
function M.volume_icon(s)
  if not s then
    return "audio-volume-muted-symbolic"
  end
  local v = s.volume or 0
  if s.mute or v <= 0 then
    return "audio-volume-muted-symbolic"
  elseif v < 0.33 then
    return "audio-volume-low-symbolic"
  elseif v < 0.66 then
    return "audio-volume-medium-symbolic"
  elseif v <= 1 then
    return "audio-volume-high-symbolic"
  end
  return "audio-volume-overamplified-symbolic"
end

function M.brightness_icon(v)
  v = v or 0
  if v > 0.75 then
    return "󰃠"
  elseif v > 0.5 then
    return "󰃟"
  end
  return "󰃞"
end

-- Toggle button look (lib.scss styles `.checked` like GTK's :checked).
function M.toggle(props)
  local active = props.active
  props.active = nil
  local cls = props.class or ""
  props.class = active:map(function(on)
    return cls .. (on and " checked" or "")
  end)
  local on_toggle = props.on_toggle
  props.on_toggle = nil
  props.on_click = function()
    on_toggle(not active:peek())
  end
  return W.Button(props)
end

-- Image from a tray/app icon: absolute paths are files, the rest icon names.
function M.icon_props(icon, size)
  local props = { pixel_size = size }
  if type(icon) == "string" and icon:sub(1, 1) == "/" then
    props.file = icon
  else
    props.icon = icon
  end
  return props
end

-- Icon of the desktop entry matching a window class / player name.
local icon_cache = {}
function M.app_icon(name)
  if not name or name == "" then
    return nil
  end
  if icon_cache[name] == nil then
    local app = hw.service.apps:exact_query(name, 1)[1]
    icon_cache[name] = app and app.icon or false
  end
  return icon_cache[name] or nil
end

return M
