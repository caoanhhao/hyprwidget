-- Port of widgets/bar/components/Information.tsx: media players, weather,
-- clock (click cycles the format, hover reveals the date), bandwidth, and
-- the favorite crypto symbol.
local W = hw.widget
local common = require("config.bar.common")
local weather = require("config.bar.weather")
local Box = common.Box

-- constants/date.constants.ts
local DATE_FORMATS = { "%H:%M", "%I:%M %p" }

local date_format = hw.settings["dateFormat"]
local tick = hw.poll(5000, function()
  return os.time() // 60
end)
local date_less = hw.computed(function()
  tick()
  return os.date(date_format() or DATE_FORMATS[1])
end)
local date_more = tick:map(function()
  return os.date(" %A ·%e %b %Y ")
end)

local function Clock()
  return common.custom_revealer({
    class = "clock",
    transition = "slide_right",
    trigger = W.Label({ class = "clock", label = date_less }),
    child = W.Label({ class = "revealer", label = date_more }),
    on_click = function()
      local cur = date_format:peek()
      local next_i = 1
      for i, f in ipairs(DATE_FORMATS) do
        if f == cur then
          next_i = i % #DATE_FORMATS + 1
        end
      end
      hw.settings:set("dateFormat", DATE_FORMATS[next_i])
    end,
  })
end

local function Mpris(on_active)
  local mpris = hw.service.mpris
  local function player_button(p)
    local bus = p.bus_name
    local player = mpris.players:map(function(list)
      for _, q in ipairs(list or {}) do
        if q.bus_name == bus then
          return q
        end
      end
      return p
    end)
    local playing = player:map(function(q)
      return q.playback_status == "playing"
    end)
    local title = player:map(function(q)
      return q.title or ""
    end)
    -- Status icon slides in for 5 s after play/pause.
    local reveal = hw.state(false)
    local timer
    playing:subscribe(function()
      reveal:set(true)
      if timer then
        timer:cancel()
      end
      timer = hw.timeout(5000, function()
        timer = nil
        reveal:set(false)
      end)
    end)
    local icon = common.app_icon(p.desktop_entry ~= "" and p.desktop_entry or p.identity)
    return W.MenuButton({
      W.Overlay({
        css = hw.computed(function()
          local width = math.min((utf8.len(title()) or #title()) * 10 + 50, 200)
          return string.format("min-width: %dpx;", playing() and width + 25 or width)
        end),
        child = W.Box({
          class = "cover-art",
          css = player:map(function(q)
            if not q.cover_art or q.cover_art == "" then
              return ""
            end
            return string.format(
              "background-image: url('file://%s'); background-size: cover; background-position: center;",
              q.cover_art
            )
          end),
        }),
        Box({
          class = "content",
          spacing = 3,
          icon and W.Image({
            icon = icon,
            class = playing:map(function(on)
              return on and "mpris-icon playing" or "mpris-icon paused"
            end),
          }),
          W.Revealer({
            reveal = reveal,
            transition = "slide_left",
            duration = common.TRANSITION,
            W.Label({
              class = "playback-status-icon icon",
              label = playing:map(function(on)
                return on and "" or ""
              end),
            }),
          }),
          W.Label({ label = title, ellipsize = "end", max_width_chars = 25 }),
        }),
      }),
      popover = common.popover(
        Box({
          class = "player",
          vertical = true,
          spacing = 5,
          width_request = 300,
          W.Label({ class = "title", label = title, ellipsize = "end", max_width_chars = 30 }),
          W.Label({
            class = "artist",
            label = player:map(function(q)
              return q.artist or ""
            end),
            ellipsize = "end",
          }),
          Box({
            halign = "center",
            spacing = 5,
            W.Button({
              label = "󰒮",
              on_click = function()
                mpris:previous(bus)
              end,
            }),
            W.Button({
              label = playing:map(function(on)
                return on and "" or ""
              end),
              on_click = function()
                mpris:play_pause(bus)
              end,
            }),
            W.Button({
              label = "󰒭",
              on_click = function()
                mpris:next(bus)
              end,
            }),
          }),
        }),
        on_active
      ),
    })
  end
  return Box({
    class = "mpris",
    spacing = 5,
    visible = mpris.players:map(function(list)
      return #(list or {}) > 0
    end),
    hw.For(mpris.players, player_button, function(p)
      return p.bus_name
    end),
  })
end

local function Bandwidth(on_active)
  local net = hw.service.sysinfo.network
  -- AGS bandwidth-loop-ags: [tx KB/s, rx KB/s, tx today KB, rx today KB]
  local function kb(field)
    return net:map(function(n)
      return ((n or {})[field] or 0) / 1024
    end)
  end
  local up, down = kb("tx_speed"), kb("rx_speed")
  local function packet(class, sig, arrow)
    return Box({
      W.Label({
        class = "packet " .. class,
        label = sig:map(function(v)
          return string.format("%.0f", v)
        end),
        css = sig:map(function(v)
          return string.format("min-width: %dpx;", #string.format("%.0f", v) * 10)
        end),
      }),
      W.Label({ class = "secondary", label = arrow }),
    })
  end
  local function format_kb(k)
    if k >= 1024 * 1024 then
      return string.format("%.2f GB", k / 1024 / 1024)
    elseif k >= 1024 then
      return string.format("%.2f MB", k / 1024)
    end
    return string.format("%.2f KB", k)
  end
  local function section(title, speed, total)
    local function item(name, value)
      return Box({
        class = "bandwidth-item",
        vertical = true,
        spacing = 2,
        hexpand = true,
        W.Label({ class = "bandwidth-label", label = name }),
        W.Label({ class = "bandwidth-value", label = value }),
      })
    end
    return Box({
      class = "bandwidth-section",
      vertical = true,
      spacing = 8,
      W.Label({ class = "bandwidth-subheading", label = title }),
      Box({
        class = "bandwidth-detail",
        spacing = 8,
        item(
          "Packets",
          speed:map(function(v)
            return string.format("%.2f", v)
          end)
        ),
        item("Data", kb(total):map(format_kb)),
      }),
    })
  end
  return W.MenuButton({
    class = "bandwidth",
    Box({
      class = "bandwidth-button",
      tooltip = "click to open",
      spacing = 5,
      packet("upload", up, ""),
      packet("download", down, ""),
    }),
    popover = common.popover(
      Box({
        class = "bandwidth-popover",
        spacing = 12,
        vertical = true,
        W.Label({ class = "bandwidth-heading", label = "Network Statistics" }),
        Box({
          spacing = 12,
          section("Upload", up, "tx_today"),
          section("Download", down, "rx_today"),
        }),
      }),
      on_active
    ),
  })
end

-- settings crypto.favorite: shown when a symbol is pinned (the price graph
-- itself is the Crypto widget, ported with the panels).
local function Crypto()
  local fav = hw.settings["crypto.favorite"]
  return Box({
    visible = fav:map(function(f)
      return f ~= nil and (f.symbol or "") ~= ""
    end),
    tooltip = "click to remove",
    on_click = function()
      hw.settings:set("crypto.favorite", { symbol = "", timeframe = "" })
    end,
    W.Label({
      class = "crypto",
      label = fav:map(function(f)
        return f and f.symbol or ""
      end),
    }),
  })
end

return function(_, opts)
  return Box({
    class = "information",
    spacing = 5,
    halign = opts.halign,
    Mpris(opts.on_active),
    weather.button(opts.on_active),
    Clock(),
    Bandwidth(opts.on_active),
    Crypto(),
  })
end
