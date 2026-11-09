-- Port of widgets/Player.tsx and widgets/MediaWidget.tsx.
--
--   W.Player{ bus = "org.mpris.MediaPlayer2.spotify", width = 240, height = 240 }
--   W.MediaWidget{ height = 240 }   -- the playing player, else the first one
--
-- Cover art fills the widget; Cava and the controls sit at the bottom. The
-- controls slide left/right when the track changes (two copies in a Stack).
local W = hw.widget
local common = require("config.panels.common")
local bar_common = require("config.bar.common")
local Box = common.Box
local mpris = hw.service.mpris

local DEFAULT_COVER = CONFIG_DIR .. "/assets/player/player_default.png"

local function length_str(secs)
  secs = math.max(0, math.floor(secs or 0))
  return string.format("%d:%02d", secs // 60, secs % 60)
end

local function find(list, bus)
  for _, p in ipairs(list or {}) do
    if p.bus_name == bus then
      return p
    end
  end
  return nil
end

hw.define("Player", function(props)
  local bus = props.bus
  local player = mpris.players:map(function(list)
    return find(list, bus) or {}
  end)
  local function field(name, fallback)
    return player:map(function(p)
      local v = p[name]
      if v == nil or v == "" then
        return fallback
      end
      return v
    end)
  end
  local position = mpris:position(bus)
  local length = field("length", 0)
  local has_length = length:map(function(l)
    return l > 0
  end)
  local status = field("playback_status", "stopped")
  local cover = field("cover_art", DEFAULT_COVER)
  local direction = "next"

  local function bottom_bar()
    local icon = bar_common.app_icon((player:peek().desktop_entry ~= "" and player:peek().desktop_entry) or player:peek().identity)
    return Box({
      class = "main",
      spacing = 5,
      vertical = true,
      hexpand = true,
      Box({
        class = "top-row",
        spacing = 10,
        common.Picture({
          class = status:map(function(s)
            return s == "playing" and "cover-art-spinner playing" or "cover-art-spinner paused"
          end),
          width = 40,
          height = 40,
          file = cover,
        }),
        Box({
          class = "info",
          vertical = true,
          W.Label({ class = "title", ellipsize = "end", halign = "start", label = field("title", "Unknown Track") }),
          W.Label({
            class = "artist",
            max_width_chars = 20,
            halign = "start",
            ellipsize = "end",
            label = field("artist", "Unknown Artist"),
          }),
        }),
        Box({
          hexpand = true,
          halign = "end",
          valign = "start",
          W.Image({ class = "icon", tooltip = field("identity", ""), icon = icon or "audio-x-generic" }),
        }),
      }),
      Box({ class = "separator", vexpand = true }),
      W.CenterBox({
        start = Box({
          W.Label({ class = "position time", halign = "start", label = position:map(length_str), visible = has_length }),
        }),
        center = Box({
          spacing = 5,
          W.Button({
            visible = field("can_go_previous", false),
            on_click = function()
              direction = "prev"
              mpris:previous(bus)
            end,
            W.Label("󰒮"),
          }),
          W.Button({
            class = "play-pause",
            visible = field("can_play", false),
            on_click = function()
              mpris:play_pause(bus)
            end,
            W.Label({
              label = status:map(function(s)
                return s == "playing" and "\u{f04c}" or "\u{f04b}"
              end),
            }),
          }),
          W.Button({
            visible = field("can_go_next", false),
            on_click = function()
              direction = "next"
              mpris:next(bus)
            end,
            W.Label("󰒭"),
          }),
        }),
        ["end"] = Box({
          W.Label({ class = "length time", halign = "end", label = length:map(length_str), visible = has_length }),
        }),
      }),
      W.Slider({
        class = "slider",
        draw_value = false,
        visible = has_length,
        value = hw.computed(function()
          local l = length()
          return l > 0 and math.min(1, position() / l) or 0
        end),
        -- Programmatic updates report the value we just set; a user drag
        -- differs from it.
        on_change = function(_, v)
          local l = length:peek()
          if l > 0 and math.abs(v * l - position:peek()) > 1.5 then
            mpris:set_position(bus, v * l)
          end
        end,
      }),
    })
  end

  -- AGS switches between two identical bars to animate track changes.
  local shown = hw.state("bar1")
  local transition = hw.state("slide_left")
  local last_title = player:peek().title
  local unsubscribe = player:subscribe(function(p)
    if p.title ~= last_title then
      last_title = p.title
      transition:set(direction == "next" and "slide_left" or "slide_right")
      shown:set(shown:peek() == "bar1" and "bar2" or "bar1")
      direction = "next"
    end
  end)

  return W.Overlay({
    class = common.class("player", props.class),
    hexpand = true,
    visible = field("can_play", false),
    on_destroy = function()
      unsubscribe()
    end,
    child = common.Picture({ class = "img", width = props.width, height = props.height, file = cover }),
    Box({
      vertical = true,
      valign = "end",
      W.Cava({ bars = 50, height = 40, transition = "slide_up" }),
      W.Stack({
        transition = transition,
        duration = 250,
        hexpand = true,
        vexpand = true,
        shown = shown,
        Box({ name = "bar1", bottom_bar() }),
        Box({ name = "bar2", bottom_bar() }),
      }),
    }),
  })
end)

-- The player shown by MediaWidget: the playing one, else the first.
local current = common.distinct(mpris.players, function(list)
  list = list or {}
  for _, p in ipairs(list) do
    if p.playback_status == "playing" then
      return p.bus_name
    end
  end
  return list[1] and list[1].bus_name or false
end)

hw.define("MediaWidget", function(props)
  return Box({
    class = props.class,
    css = "border-radius: 10px;",
    visible = props.visible,
    hw.With(current, function(bus)
      if not bus then
        return Box({})
      end
      return W.Player({ bus = bus, width = props.width, height = props.height })
    end),
  })
end)
