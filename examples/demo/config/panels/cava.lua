-- Port of widgets/Cava.tsx: an audio visualizer drawn with cairo. AGS used
-- AstalCava and a label of block characters; here one shared `cava` process
-- (raw ASCII output through hw.subprocess) feeds every visible W.Cava.
--
--   W.Cava{ bars = 50, height = 40, transition = "slide_up" }
--
-- cava only runs while at least one W.Cava is mapped and a player is
-- playing; the revealer hides after 2 s of silence (AGS hysteresis).
local W = hw.widget
local common = require("config.panels.common")

local BARS = 50
local FRAMERATE = 30
local HIDE_DELAY = 2000

local values = {}
local consumers = {} -- id -> { area = handle, on_frame = fn }
local n_consumers = 0
local proc
local frames = 0

local function config_path()
  local dir = common.CACHE_DIR .. "/cava"
  local path = dir .. "/config"
  os.execute("mkdir -p " .. common.sh_quote(dir))
  local f = io.open(path, "w")
  if f then
    f:write(string.format(
      [[
[general]
framerate = %d
bars = %d
autosens = 1

[input]
method = pipewire
source = %s

[output]
method = raw
raw_target = /dev/stdout
data_format = ascii
ascii_max_range = 1000
bar_delimiter = 59
frame_delimiter = 10

[smoothing]
noise_reduction = 77
]],
      FRAMERATE,
      BARS,
      -- HW_CAVA_SOURCE: capture another node than the default output.
      os.getenv("HW_CAVA_SOURCE") or "auto"
    ))
    f:close()
  end
  return path
end

local function on_line(line)
  local i = 0
  for n in line:gmatch("%d+") do
    i = i + 1
    values[i] = tonumber(n) / 1000
  end
  frames = frames + 1
  for _, c in pairs(consumers) do
    c.on_frame()
  end
end

local playing = hw.service.mpris.players:map(function(list)
  for _, p in ipairs(list or {}) do
    if p.playback_status == "playing" then
      return true
    end
  end
  return false
end)

local function update_process()
  local want = n_consumers > 0 and playing:peek()
  if want and not proc then
    proc = hw.subprocess({ "cava", "-p", config_path() }, on_line, function(err)
      print("cava: " .. err)
    end)
  elseif not want and proc then
    proc:kill()
    proc = nil
    for i = 1, #values do
      values[i] = 0
    end
    for _, c in pairs(consumers) do
      c.on_frame()
    end
  end
end
playing:subscribe(update_process)

-- Frames received since start (for measuring the frame rate).
hw.on_request("cava-frames", function()
  return tostring(frames)
end)

local next_id = 0

hw.define("Cava", function(props)
  local bars = props.bars or BARS
  local reveal = hw.state(false)
  local hide_timer
  next_id = next_id + 1
  local id = next_id
  local area

  local function is_empty()
    for i = 1, bars do
      local v = values[math.floor((i - 1) * BARS / bars) + 1]
      if v and v > 0.01 then
        return false
      end
    end
    return true
  end

  local consumer = {
    on_frame = function()
      if is_empty() then
        if reveal:peek() and not hide_timer then
          hide_timer = hw.timeout(HIDE_DELAY, function()
            hide_timer = nil
            reveal:set(false)
          end)
        end
      else
        if hide_timer then
          hide_timer:cancel()
          hide_timer = nil
        end
        if not reveal:peek() then
          reveal:set(true)
        end
      end
      if area and reveal:peek() then
        area:queue_draw()
      end
    end,
  }

  local function attach(on)
    if on and not consumers[id] then
      consumers[id] = consumer
      n_consumers = n_consumers + 1
    elseif not on and consumers[id] then
      consumers[id] = nil
      n_consumers = n_consumers - 1
    end
    update_process()
  end

  return W.Revealer({
    reveal = reveal,
    transition = props.transition or "slide_up",
    duration = common.TRANSITION,
    -- The revealer stays mapped while its child is hidden.
    on_show = function()
      attach(true)
    end,
    on_hide = function()
      attach(false)
    end,
    on_destroy = function()
      area = nil
      attach(false)
    end,
    W.DrawingArea({
      class = common.class("cava", props.class),
      height = props.height or 40,
      hexpand = true,
      setup = function(self)
        area = self
      end,
      draw = function(cr, w, h)
        local gap = 2
        local bw = math.max(1, (w - gap * (bars - 1)) / bars)
        for i = 1, bars do
          local v = values[math.floor((i - 1) * BARS / bars) + 1] or 0
          local bh = math.max(2, math.min(1, v) * h)
          cr:rounded_rectangle((i - 1) * (bw + gap), h - bh, bw, bh, math.min(bw / 2, 3))
        end
        cr:fill()
      end,
    }),
  })
end)
