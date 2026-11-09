-- Port of widgets/bar/Bar.tsx and BarHover.tsx.
--
-- Bar: modules laid out from settings `bar.layout` (1 → center, 2 → start/end,
-- 3 → start/center/end), top or bottom from `bar.orientation`, hidden while
-- the focused window is fullscreen. When `bar.lock` is off the bar auto-hides
-- 100 ms after the pointer leaves it (unless a popover is open) and the thin
-- BarHover strip on the same edge brings it back.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box
local hypr = hw.service.hyprland

local MODULES = {
  workspaces = require("config.bar.workspaces"),
  information = require("config.bar.information"),
  utilities = require("config.bar.utilities"),
}

local lock = hw.settings["bar.lock"]
local top = hw.settings["bar.orientation"]
local layout = hw.settings["bar.layout"]
-- AGS globalMargin = general:gaps_out (first value).
local gaps_out = hw.opt.general.gaps_out:map(function(g)
  return type(g) == "table" and (g.top or 0) or (tonumber(g) or 0)
end)

local function anchor(on_top)
  return on_top and { "top", "left", "right" } or { "bottom", "left", "right" }
end

-- AGS fullscreenClient: the focused window is fullscreen (mode 2).
local fullscreen = hypr.focused_client:map(function(c)
  return c ~= nil and c.fullscreen == 2
end)

-- Left/RightPanelVisibility: toggles the side panel of this monitor (M4);
-- revealed while the panel is pinned.
local function panel_trigger(monitor, side, closed, opened, tooltip)
  local active = hw.state(false)
  return W.Revealer({
    reveal = hw.settings[side .. "Panel.lock"],
    transition = side == "left" and "slide_right" or "slide_left",
    duration = common.TRANSITION,
    W.Button({
      class = active:map(function(a)
        return "panel-trigger icon" .. (a and " checked" or "")
      end),
      tooltip = tooltip,
      label = active:map(function(a)
        return a and opened or closed
      end),
      on_click = function()
        local on = not active:peek()
        active:set(on)
        local win = hw.window.get(side .. "-panel-" .. monitor.name)
        if win then
          if on then
            win:show()
          else
            win:hide()
          end
        end
      end,
    }),
  })
end

local function Bar(monitor)
  local name = "bar-" .. monitor.name
  -- Shown by the hover strip while unlocked.
  local shown = hw.state(lock:peek() == true)
  -- AGS Window.popupIsOpen: popovers currently open from this bar.
  local popups = 0
  local function on_active(open)
    popups = math.max(0, popups + (open and 1 or -1))
  end
  local hide_timer

  local centerbox = hw.With(layout, function(list)
    local enabled = {}
    for _, item in ipairs(list or {}) do
      if item.enabled then
        table.insert(enabled, item)
      end
    end
    -- AGS picks the slots from the full list's length.
    local n = #(list or {})
    local slots = n == 1 and { "center" } or n == 2 and { "start", "end" } or { "start", "center", "end" }
    local props = { hexpand = true }
    for i, item in ipairs(enabled) do
      local slot = slots[i]
      local module = MODULES[item.name]
      if slot then
        props[slot] = module and module(monitor, { halign = slot, on_active = on_active }) or Box({})
      end
    end
    return W.CenterBox(props)
  end)

  return W.Window({
    name = name,
    namespace = "bar",
    class = "Bar",
    monitor = monitor,
    layer = "top",
    exclusivity = "exclusive",
    anchor = top:map(anchor),
    margin = hw.computed(function()
      local m, on_top = gaps_out(), top()
      return { top = on_top and m or 0, bottom = on_top and 0 or m, left = m, right = m }
    end),
    visible = hw.computed(function()
      return not fullscreen() and (lock() or shown())
    end),
    setup = function()
      -- Pinning the bar keeps it shown after unpinning.
      lock:subscribe(function(l)
        if l then
          shown:set(true)
        end
      end)
    end,
    on_hover = function()
      if hide_timer then
        hide_timer:cancel()
        hide_timer = nil
      end
    end,
    on_leave = function()
      if lock:peek() then
        return
      end
      hide_timer = hw.timeout(100, function()
        hide_timer = nil
        if not lock:peek() and popups == 0 then
          shown:set(false)
        end
      end)
    end,

    Box({
      spacing = 5,
      class = hypr.focused_client:map(function(c)
        return c and "bar full" or "bar empty"
      end),
      panel_trigger(monitor, "left", "", "", "SUPER + L"),
      Box({ class = "bar-center", hexpand = true, centerbox }),
      panel_trigger(monitor, "right", "", "", "SUPER + R"),
    }),
  }),
    shown
end

local function BarHover(monitor, shown)
  return W.Window({
    name = "bar-hover-" .. monitor.name,
    -- AGS left the namespace unset (gtk4-layer-shell's default).
    namespace = "bar-hover",
    monitor = monitor,
    layer = "top",
    exclusivity = "ignore",
    margin = 0,
    anchor = top:map(anchor),
    visible = lock:map(function(l)
      return not l
    end),
    on_hover = function()
      shown:set(true)
    end,
    Box({ css = "min-height: 5px; background-color: rgba(0,0,0,0.01);" }),
  })
end

return function(monitor)
  local bar, shown = Bar(monitor)
  return { bar, BarHover(monitor, shown) }
end
