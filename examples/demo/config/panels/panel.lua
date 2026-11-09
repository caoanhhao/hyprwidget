-- Port of widgets/leftPanel/{LeftPanel,LeftPanelHover}.tsx,
-- widgets/rightPanel/{RightPanel,RightPanelHover}.tsx and the
-- WindowActions of utils/window.tsx.
--
-- Both panels pick their widgets from settings (AGS keys): the left panel
-- shows one at a time (`leftPanel.widget`), the right panel stacks every
-- enabled one in order (`rightPanel.widgets`). Each selectable widget is a
-- hw.define'd W.<Name> (see M.SELECTORS), so a plugin can provide one
-- without touching this file. Unlocked panels hide when the pointer leaves
-- them and come back through a 5 px hover strip on the screen edge.
local W = hw.widget
local common = require("config.panels.common")
local defaults = require("config.panels.defaults")
local Box = common.Box

local M = {}

-- settings selector name -> hw.define'd widget.
M.SELECTORS = {
  Waifu = "Waifu",
  Media = "MediaWidget",
  NotificationHistory = "NotificationHistory",
  ScriptTimer = "ScriptTimer",
  Crypto = "CryptoViewer",
  Calendar = "CalendarWidget",
  SystemResources = "SystemResources",
  VPN = "VPN",
  Docker = "Docker",
  WayDroid = "WayDroid",
  UserProfile = "UserProfile",
  BooruViewer = "BooruViewer",
  Settings = "Settings",
  CustomScripts = "CustomScripts",
  KeyBinds = "KeyBinds",
}

local function selector_widget(name, props)
  local widget = M.SELECTORS[name]
  local ok, w = pcall(function()
    return W[widget or name](props)
  end)
  if not ok then
    print("panel: widget " .. tostring(name) .. ": " .. tostring(w))
    return Box({})
  end
  return w
end

-- AGS globalMargin = general:gaps_out (first value).
local gaps_out = hw.opt.general.gaps_out:map(function(g)
  return type(g) == "table" and (g.top or 0) or (tonumber(g) or 0)
end)

local function copy_list(list)
  local out = {}
  for _, w in ipairs(list or {}) do
    local c = {}
    for k, v in pairs(w) do
      c[k] = v
    end
    table.insert(out, c)
  end
  return out
end

-- Layer-shell surfaces keep their size until remapped (AGS queueResize).
local function remap(name)
  local win = hw.window.get(name)
  if win and win:is_visible() then
    win:hide()
    hw.timeout(1, function()
      win:show()
    end)
  end
end

local function WindowActions(key, window_name, min_width, max_width)
  local width = hw.settings[key .. ".width"]
  local exclusive = hw.settings[key .. ".exclusivity"]
  local lock = hw.settings[key .. ".lock"]
  return Box({
    class = "window-actions",
    vexpand = true,
    halign = "end",
    valign = "end",
    vertical = true,
    spacing = 5,
    W.Button({
      class = "expand-window",
      label = "\u{f067}",
      on_click = function()
        hw.settings:set(key .. ".width", math.min(max_width, (width:peek() or 0) + 50))
        remap(window_name)
      end,
    }),
    W.Button({
      class = "shrink-window",
      label = "\u{f068}",
      on_click = function()
        hw.settings:set(key .. ".width", math.max(min_width, (width:peek() or 0) - 50))
        remap(window_name)
      end,
    }),
    common.toggle({
      class = "exclusivity",
      label = "\u{f2d2}",
      active = exclusive:map(function(e)
        return not e
      end),
      on_toggle = function(on)
        hw.settings:set(key .. ".exclusivity", not on)
      end,
    }),
    common.toggle({
      class = "lock",
      label = lock:map(function(l)
        return l and "\u{f023}" or "\u{f2fc}"
      end),
      active = lock,
      on_toggle = function(on)
        hw.settings:set(key .. ".lock", on)
      end,
    }),
    W.Button({
      class = "close",
      label = "\u{f00d}",
      on_click = function()
        local win = hw.window.get(window_name)
        if win then
          win:hide()
        end
      end,
    }),
  })
end

-- The window shared by both panels. `side` is "left" or "right".
local function PanelWindow(monitor, side, content, popup_open)
  local key = side .. "Panel"
  local name = side .. "-panel-" .. monitor.name
  local lock = hw.settings[key .. ".lock"]
  local exclusive = hw.settings[key .. ".exclusivity"]
  local hide_timer
  local function hide()
    local win = hw.window.get(name)
    if win then
      win:hide()
    end
  end
  return W.Window({
    name = name,
    namespace = side .. "-panel",
    monitor = monitor,
    class = exclusive:map(function(e)
      return side .. "-panel " .. (e and "exclusive" or "normal")
    end),
    layer = "top",
    keymode = "on_demand",
    anchor = { "top", side, "bottom" },
    exclusivity = exclusive:map(function(e)
      return e and "exclusive" or "normal"
    end),
    margin = gaps_out:map(function(m)
      return { top = m, bottom = m }
    end),
    visible = false,
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
        if not lock:peek() and not popup_open() then
          hide()
        end
      end)
    end,
    on_key = function(_, key_name)
      if key_name == "Escape" then
        hide()
        return true
      end
      return false
    end,
    content,
  })
end

local function Hover(monitor, side)
  local lock = hw.settings[side .. "Panel.lock"]
  return W.Window({
    name = side .. "-panel-hover-" .. monitor.name,
    namespace = side .. "-panel-hover",
    monitor = monitor,
    layer = "top",
    exclusivity = "ignore",
    margin = 0,
    anchor = { side, "top", "bottom" },
    visible = lock:map(function(l)
      return not l
    end),
    on_hover = function()
      local win = hw.window.get(side .. "-panel-" .. monitor.name)
      if win then
        win:show()
      end
    end,
    Box({ css = "min-width: 5px; background-color: rgba(0,0,0,0.01);" }),
  })
end

-- Left panel ----------------------------------------------------------------

local function LeftPanel(monitor)
  local on_active, popup_open = common.popup_counter()
  local current = hw.settings["leftPanel.widget"]:map(function(w)
    return (w and w.name) or defaults.LEFT_WIDGETS[1].name
  end)
  local name = "left-panel-" .. monitor.name

  local actions = { class = "widget-actions", vertical = true, spacing = 10 }
  for _, sel in ipairs(defaults.LEFT_WIDGETS) do
    table.insert(
      actions,
      common.toggle({
        class = sel.name,
        label = sel.icon,
        tooltip_markup = "Click to open\n<b>" .. sel.name .. "</b>",
        active = current:map(function(c)
          return c == sel.name
        end),
        on_toggle = function()
          hw.settings:set("leftPanel.widget", { name = sel.name, icon = sel.icon, enabled = sel.enabled })
        end,
      })
    )
  end

  local stack = {
    hexpand = true,
    vexpand = true,
    transition = "slide_left_right",
    duration = common.TRANSITION,
    shown = current,
  }
  for _, sel in ipairs(defaults.LEFT_WIDGETS) do
    table.insert(
      stack,
      Box({
        name = sel.name,
        hexpand = true,
        vexpand = true,
        selector_widget(sel.name, { monitor = monitor.name, on_active = on_active }),
      })
    )
  end

  return PanelWindow(
    monitor,
    "left",
    Box({
      Box({
        class = "panel-actions",
        halign = "start",
        vertical = true,
        Box(actions),
        WindowActions("leftPanel", name, 400, 1500),
      }),
      Box({
        class = "main-content",
        hexpand = true,
        vertical = true,
        spacing = 10,
        width_request = hw.settings["leftPanel.width"],
        W.Stack(stack),
      }),
    }),
    popup_open
  )
end

-- Right panel ---------------------------------------------------------------

local widgets = hw.settings["rightPanel.widgets"]
-- Name of the widget enabled last (gets the `new-widget` fade-in).
local newest = nil

local function set_widgets(list)
  hw.settings:set("rightPanel.widgets", list)
end

-- AGS rightPanelWidgetSelectors[NotificationHistory].onRightClick.
local RIGHT_CLICK = {
  NotificationHistory = function()
    local on = not hw.settings:get("notifications.sound")
    hw.settings:set("notifications.sound", on)
    local list = copy_list(widgets:peek())
    for _, w in ipairs(list) do
      if w.name == "NotificationHistory" then
        w.icon = on and "\u{eb9a}" or "\u{ec08}"
      end
    end
    set_widgets(list)
    common.notify("Notification Sound", on and "Enabled" or "Disabled")
  end,
}

local function RightPanel(monitor)
  local on_active, popup_open = common.popup_counter()
  local name = "right-panel-" .. monitor.name
  local width = hw.settings["rightPanel.width"]
  local height = width:map(function(w)
    return (w or 250) - 20
  end)

  local selectors = common.For(widgets, function(w, i)
    return W.Button({
      class = "widget-selector drag" .. (w.enabled and " checked" or ""),
      label = w.icon or "?",
      tooltip_markup = "<b>Scroll To Move</b>\n" .. w.name .. (w.tooltip and ("\n" .. w.tooltip) or ""),
      on_click = function(_, button)
        if button == 3 then
          if RIGHT_CLICK[w.name] then
            RIGHT_CLICK[w.name]()
          end
          return
        end
        if button ~= 1 then
          return
        end
        local list = copy_list(widgets:peek())
        for _, x in ipairs(list) do
          if x.name == w.name then
            x.enabled = not x.enabled
            if x.enabled then
              newest = x.name
            end
          end
        end
        set_widgets(list)
      end,
      -- AGS reorders by drag & drop; scrolling moves the selector here.
      on_scroll = function(_, _, dy)
        local list = copy_list(widgets:peek())
        for k, x in ipairs(list) do
          if x.name == w.name then
            i = k
          end
        end
        local j = i + (dy > 0 and 1 or -1)
        if j >= 1 and j <= #list then
          list[i], list[j] = list[j], list[i]
          set_widgets(list)
        end
        return true
      end,
    })
  end, function(w)
    return table.concat({ w.name, tostring(w.enabled), w.icon or "" }, "|")
  end, { vertical = true, spacing = 5 })

  local enabled = widgets:map(function(list)
    local out = {}
    for _, w in ipairs(list or {}) do
      if w.enabled then
        table.insert(out, w.name)
      end
    end
    return out
  end)

  return PanelWindow(
    monitor,
    "right",
    Box({
      Box({
        class = "main-content",
        hexpand = true,
        vertical = true,
        width_request = width,
        common.For(enabled, function(widget_name)
          local cls = widget_name == newest and "new-widget" or ""
          return selector_widget(widget_name, {
            class = cls,
            height = height,
            monitor = monitor.name,
            on_active = on_active,
          })
        end, nil, { vertical = true }),
      }),
      Box({
        class = "panel-actions",
        halign = "end",
        vertical = true,
        Box({ class = "widget-actions", vertical = true, spacing = 5, selectors }),
        WindowActions("rightPanel", name, 250, 1500),
      }),
    }),
    popup_open
  )
end

function M.per_monitor(monitor)
  return {
    LeftPanel(monitor),
    Hover(monitor, "left"),
    RightPanel(monitor),
    Hover(monitor, "right"),
  }
end

return M
