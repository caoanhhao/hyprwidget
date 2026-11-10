-- Building blocks shared by the side panels and their widgets (AGS
-- widgets/Picture.tsx, Progress.tsx, KeyBind.tsx, Custom/Eventbox,
-- utils/window.tsx).
local W = hw.widget
local bar_common = require("config.bar.common")
local util = require("lib.util")

local M = {}

M.Box = bar_common.Box
M.TRANSITION = bar_common.TRANSITION
M.toggle = bar_common.toggle
M.notify = util.notify
M.run = util.run
-- hw.For with props for its box (`vertical`, `spacing`, …).
M.For = util.For

local Box = M.Box

-- `<side>Panel.width` is the whole panel layer, like `panel_width_default` in
-- the Hyprland main layout: the visible panel spans gaps_out .. width from the
-- screen edge, the same box as the window borders of the layout column below.
-- Width of the `.panel-actions` column (buttons + padding).
M.PANEL_ACTIONS_WIDTH = 35

-- general:gaps_out on the panel's screen edge ("left" or "right").
function M.panel_gap(side)
  return hw.opt.general.gaps_out:map(function(g)
    if type(g) == "table" then
      return tonumber(g[side]) or 0
    end
    return tonumber(g) or 0
  end)
end

-- Width left for the panel's widgets: layer width minus gap and actions.
function M.panel_content_width(side)
  local width = hw.settings[side .. "Panel.width"]
  local gap = M.panel_gap(side)
  return hw.computed(function()
    return math.max(0, (width() or 400) - gap() - M.PANEL_ACTIONS_WIDTH)
  end)
end

-- AGS ~/.config/ags/cache, here $XDG_CACHE_HOME/hyprwidget.
M.CACHE_DIR = util.CACHE_DIR

function M.sh_quote(s)
  return "'" .. tostring(s):gsub("'", "'\\''") .. "'"
end

function M.file_exists(path)
  local f = path and io.open(path, "r")
  if f then
    f:close()
    return true
  end
  return false
end

-- Merges `extra` into a copy of `props` (props passed to hw.define'd
-- widgets are reused by callers, so never mutate them).
function M.merge(props, extra)
  local t = {}
  for k, v in pairs(props or {}) do
    t[k] = v
  end
  for k, v in pairs(extra or {}) do
    t[k] = v
  end
  return t
end

-- `class` prop that may be a string or a signal, prefixed with `base`.
function M.class(base, extra)
  if extra == nil or extra == "" then
    return base
  end
  if hw.is_signal(extra) then
    return extra:map(function(c)
      return base .. " " .. (c or "")
    end)
  end
  return base .. " " .. extra
end

-- A signal that only notifies when `fn(value)` changes (hw.With rebuilds on
-- every emission; mpris `players` changes on every position update).
function M.distinct(sig, fn)
  local out = hw.state(fn(sig:peek()))
  sig:subscribe(function(v)
    local n = fn(v)
    if n ~= out:peek() then
      out:set(n)
    end
  end)
  return out
end

-- AGS Picture: a scaled image (cover by default) with optional info badges
-- in the bottom-right corner. Like AGS the picture is an overlay child, so
-- the image's own size never grows the layout: the size comes from
-- `width` / `height` (or the space the parent gives).
function M.Picture(opts)
  local overlay = {
    class = "image",
    hexpand = opts.hexpand,
    vexpand = opts.vexpand,
    child = Box({ width_request = opts.width, height_request = opts.height }),
    W.Picture({
      class = M.class("image", opts.class),
      file = opts.file,
      content_fit = opts.content_fit or "cover",
      can_shrink = true,
    }),
  }
  if opts.info and #opts.info > 0 then
    local info = { class = "image-info", halign = "end", valign = "end", spacing = 5 }
    for _, i in ipairs(opts.info) do
      table.insert(info, W.Label({ class = "image-info-item", label = i }))
    end
    table.insert(overlay, Box(info))
  end
  return W.Overlay(overlay)
end

-- AGS Progress: a revealer showing loading (spinner-less "…"), success (for
-- 3 s, with the latency) or error. `status` is a state of "idle" | "loading"
-- | "success" | "error"; `text` an optional signal.
function M.Progress(opts)
  local status = opts.status
  local reveal = hw.state(opts.show_when_idle == true)
  local latency = hw.state("")
  local started, timer
  status:subscribe(function(s)
    if timer then
      timer:cancel()
      timer = nil
    end
    if s == "loading" then
      started = hw.now()
      reveal:set(true)
    elseif s == "success" then
      if started then
        latency:set(string.format("%.2fs", (hw.now()) - started))
        started = nil
      end
      reveal:set(true)
      timer = hw.timeout(3000, function()
        timer = nil
        reveal:set(false)
      end)
    elseif s == "error" then
      reveal:set(true)
    else
      reveal:set(opts.show_when_idle == true)
    end
  end)
  return W.Revealer({
    class = "progress-widget " .. (opts.class or ""),
    reveal = reveal,
    duration = M.TRANSITION,
    transition = opts.transition or "slide_down",
    Box({
      class = status:map(function(s)
        return "progress " .. tostring(s)
      end),
      spacing = 5,
      hexpand = true,
      Box({
        class = "progress-icon",
        W.Label({
          label = status:map(function(s)
            return s == "error" and "❌" or s == "success" and "✅" or s == "loading" and "󰔟" or ""
          end),
        }),
      }),
      Box({ class = "progress-content", W.Label({ class = "progress-text", label = opts.text or status }) }),
      Box({
        class = "progress-latency",
        halign = "end",
        hexpand = true,
        W.Label({
          label = latency,
          visible = status:map(function(s)
            return s == "success" or s == "error"
          end),
        }),
      }),
    }),
  })
end

-- AGS KeyBind: one button per key of the combination.
function M.KeyBind(bindings, props)
  props = props or {}
  local box = { class = "keybind " .. (props.class or ""), spacing = 5, halign = props.halign }
  for _, b in ipairs(bindings or {}) do
    table.insert(box, W.Button({ class = "binding", W.Label({ label = b }) }))
  end
  return Box(box)
end

-- AGS Eventbox + hover revealer: `actions` slides in while hovering `child`.
function M.hover_actions(opts)
  local hovered = hw.state(false)
  return Box({
    class = opts.class,
    on_hover = function()
      hovered:set(true)
    end,
    on_leave = function()
      hovered:set(false)
    end,
    opts.build(W.Revealer({
      reveal = hovered,
      transition = "slide_left",
      duration = M.TRANSITION,
      opts.actions,
    })),
  })
end

-- Runs `cmd` asynchronously; `cb(out)` on success, notification on error.
function M.exec(cmd, cb, title)
  hw.exec_async(cmd, function(out, err)
    if err then
      if title then
        M.notify(title, err)
      else
        print(tostring(cmd) .. ": " .. tostring(err))
      end
      return
    end
    if cb then
      cb(out)
    end
  end)
end

-- Popovers opened from a panel keep it from auto-hiding (AGS
-- Window.popupIsOpen); widgets receive `on_active` through their props.
function M.popup_counter()
  local n = 0
  return function(open)
    n = math.max(0, n + (open and 1 or -1))
  end, function()
    return n > 0
  end
end

return M
