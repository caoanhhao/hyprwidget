-- Port of widgets/KeyStrokeVisualizer.tsx: the last keys pressed, shown at the
-- bottom of the screen (settings `keyStrokeVisualizer.visibility` / `.anchor`).
-- Keys come from hw.service.keystrokes (evdev, replaces keystroke-loop-ags.c),
-- which needs the user in the `input` group; it reads nothing while hidden.
local W = hw.widget
local common = require("config.bar.common")

local MAX_KEYS = 5
local HIDE_DELAY = 2000
local ANIMATION = 300

local ks = hw.service.keystrokes
local visibility = hw.settings["keyStrokeVisualizer.visibility"]
local anchor = hw.settings["keyStrokeVisualizer.anchor"]
local fullscreen = hw.service.hyprland.focused_client:map(function(c)
  return c ~= nil and c.fullscreen == 2
end)

-- { id, label, shown = state } (shown drives each key's revealer).
local keys = hw.state({})
local counter = 0
local hide_timer

local function remove_later(ids)
  hw.timeout(ANIMATION, function()
    local keep = {}
    for _, k in ipairs(keys:peek()) do
      if not ids[k.id] then
        table.insert(keep, k)
      end
    end
    keys:set(keep)
  end)
end

local function push(label)
  counter = counter + 1
  local key = { id = counter, label = label, shown = hw.state(false), leaving = false }
  local list = {}
  for _, k in ipairs(keys:peek()) do
    table.insert(list, k)
  end
  table.insert(list, key)
  keys:set(list)
  -- Animate in once the revealer exists.
  hw.timeout(10, function()
    key.shown:set(true)
  end)
  -- Animate the oldest out past MAX_KEYS.
  local live = {}
  for _, k in ipairs(list) do
    if not k.leaving then
      table.insert(live, k)
    end
  end
  if #live > MAX_KEYS then
    local old = live[1]
    old.leaving = true
    old.shown:set(false)
    remove_later({ [old.id] = true })
  end
  if hide_timer then
    hide_timer:cancel()
  end
  hide_timer = hw.timeout(HIDE_DELAY, function()
    hide_timer = nil
    local ids = {}
    for _, k in ipairs(keys:peek()) do
      k.shown:set(false)
      ids[k.id] = true
    end
    remove_later(ids)
  end)
end

ks.key:subscribe(function(k)
  if k and k.label then
    push(k.label)
  end
end)

local function sync()
  pcall(ks.enable, ks, visibility:peek() == true)
end
sync()
visibility:subscribe(sync)

hw.per_monitor(function(monitor)
  return {
    W.Window({
      name = "keystroke-visualizer-" .. monitor.name,
      namespace = "keystroke-visualizer",
      class = "keystroke-visualizer",
      monitor = monitor,
      layer = "overlay",
      exclusivity = "normal",
      keymode = "none",
      margin = hw.opt.general.gaps_out,
      anchor = anchor:map(function(a)
        return type(a) == "table" and #a > 0 and a or { "bottom" }
      end),
      visible = hw.computed(function()
        return visibility() == true and not fullscreen() and #keys() > 0
      end),
      common.Box({
        hexpand = true,
        vexpand = true,
        height_request = 28,
        spacing = 5,
        hw.For(keys, function(k)
          return W.Revealer({
            transition = "swing_right",
            duration = ANIMATION,
            reveal = k.shown,
            W.Button({ class = "keystroke", label = k.label, focusable = false }),
          })
        end, function(k)
          return k.id
        end),
      }),
    }),
  }
end)
