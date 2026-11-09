-- Weather data (AGS variables.ts updateWeather / setWeatherCity) and the bar
-- button with its popover (AGS widgets/Weather.tsx WeatherButton). Open-Meteo,
-- location from settings `weather.lat/lon`, else from the IP; every 10 min.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local M = {}

local CODES = {
  [0] = { "Clear sky", "#0F4C81" },
  [1] = { "Mainly clear", "#1F5D8A" },
  [2] = { "Partly cloudy", "#2E5984" },
  [3] = { "Overcast", "#4A5568" },
  [45] = { "Foggy", "#4B5563" },
  [48] = { "Depositing rime fog", "#4B5563" },
  [51] = { "Light drizzle", "#0C4A6E" },
  [53] = { "Moderate drizzle", "#0B3A67" },
  [55] = { "Dense drizzle", "#1E3A8A" },
  [56] = { "Light freezing drizzle", "#0C4A6E" },
  [57] = { "Dense freezing drizzle", "#1E3A8A" },
  [61] = { "Slight rain", "#0C4A6E" },
  [63] = { "Moderate rain", "#0B3A67" },
  [65] = { "Heavy rain", "#1E3A8A" },
  [66] = { "Light freezing rain", "#0C4A6E" },
  [67] = { "Heavy freezing rain", "#1E3A8A" },
  [71] = { "Slight snow fall", "#334155" },
  [73] = { "Moderate snow fall", "#1E40AF" },
  [75] = { "Heavy snow fall", "#1E3A8A" },
  [77] = { "Snow grains", "#334155" },
  [80] = { "Slight rain showers", "#0C4A6E" },
  [81] = { "Moderate rain showers", "#0B3A67" },
  [82] = { "Violent rain showers", "#1E3A8A" },
  [85] = { "Slight snow showers", "#334155" },
  [86] = { "Heavy snow showers", "#1E3A8A" },
  [95] = { "Thunderstorm", "#9A3412" },
  [96] = { "Thunderstorm with slight hail", "#7C2D12" },
  [99] = { "Thunderstorm with heavy hail", "#7F1D1D" },
}

local DIRECTIONS = {
  "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE",
  "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW",
}

local function describe(code)
  return (CODES[code] or { "Unknown" })[1]
end

local function background(code)
  return (CODES[code] or { nil, "#000000" })[2]
end

local function icon(code)
  if code == nil then
    return ""
  elseif code == 0 then
    return "󰖙"
  elseif code <= 2 then
    return "󰖐"
  elseif code == 3 then
    return "󰖑"
  elseif code >= 45 and code <= 48 then
    return "󰼰"
  elseif code >= 56 and code <= 57 then
    return "󰖒"
  elseif code >= 65 and code <= 67 then
    return "󰖖"
  elseif (code >= 51 and code <= 64) or (code >= 80 and code <= 82) then
    return "󰖗"
  elseif code >= 71 and code <= 86 then
    return "󰖘"
  elseif code >= 95 then
    return "󰖓"
  end
  return "󰖙"
end

-- Module-level: one fetch loop shared by every monitor's bar.
M.data = hw.state(nil)

local function fetch()
  hw.async(function()
    local lat, lon = hw.settings:get("weather.lat"), hw.settings:get("weather.lon")
    if not lat or not lon then
      local ok, loc = pcall(hw.exec_async, {
        "bash",
        "-c",
        "curl -fsSL https://ipapi.co/latlong || curl -fsSL https://ifconfig.co/coordinates || curl -fsSL https://ipinfo.io/loc",
      })
      if ok then
        lat, lon = loc:match("^%s*([%-%d%.]+)%s*,%s*([%-%d%.]+)")
      end
    end
    if not lat or not lon then
      return
    end
    local url = string.format(
      "https://api.open-meteo.com/v1/forecast?latitude=%s&longitude=%s"
        .. "&current=temperature_2m,relative_humidity_2m,wind_speed_10m,wind_direction_10m,apparent_temperature,is_day,precipitation,weather_code"
        .. "&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,precipitation_sum,precipitation_hours,wind_speed_10m_max"
        .. "&timezone=auto&forecast_days=2",
      lat,
      lon
    )
    local ok, out = pcall(hw.exec_async, { "curl", "-fsSL", url })
    if not ok then
      print("weather: " .. tostring(out))
      return
    end
    local ok2, p = pcall(hw.json.decode, out)
    if not ok2 or not p.current then
      print("weather: bad reply")
      return
    end
    M.data:set({
      temp = p.current.temperature_2m,
      temp_unit = p.current_units.temperature_2m,
      humidity = p.current.relative_humidity_2m,
      wind_speed = p.current.wind_speed_10m,
      wind_unit = p.current_units.wind_speed_10m,
      wind_direction = p.current.wind_direction_10m,
      apparent_temp = p.current.apparent_temperature,
      precipitation = p.current.precipitation,
      code = p.current.weather_code,
      daily = p.daily,
    })
  end)
end

-- AGS setWeatherCity: geocode a city name ("" = back to IP location).
function M.set_city(name)
  hw.async(function()
    name = (name or ""):match("^%s*(.-)%s*$")
    if name == "" then
      hw.settings:set("weather.city", "")
      hw.settings:set("weather.lat", nil)
      hw.settings:set("weather.lon", nil)
      fetch()
      return
    end
    local ok, out = pcall(hw.exec_async, {
      "curl",
      "-fsSL",
      "https://geocoding-api.open-meteo.com/v1/search?count=1&format=json&name=" .. name:gsub(
        "[^%w%-_.~]",
        function(c)
          return string.format("%%%02X", c:byte())
        end
      ),
    })
    local res = ok and hw.json.decode(out).results
    if res and res[1] then
      hw.settings:set("weather.city", res[1].name)
      hw.settings:set("weather.lat", res[1].latitude)
      hw.settings:set("weather.lon", res[1].longitude)
      fetch()
    else
      hw.exec_async({ "notify-send", "Weather", "City '" .. name .. "' not found" }, function() end)
    end
  end)
end

hw.interval(600000, fetch)

local function hhmm(iso)
  return iso and iso:match("T(%d%d:%d%d)") or "N/A"
end

local function details()
  return hw.With(M.data, function(w)
    if not w then
      return W.Label({ label = "Weather data unavailable" })
    end
    local d = w.daily or {}
    local function first(t)
      return t and t[1]
    end
    local function detail(ic, text)
      return Box({
        class = "weather-detail",
        spacing = 5,
        hexpand = true,
        W.Label({ class = "icon", label = ic }),
        W.Label({ label = text }),
      })
    end
    local entry
    return Box({
      class = "weather",
      vertical = true,
      spacing = 12,
      Box({
        class = "weather-section",
        css = "background-color: " .. background(w.code) .. "; color: white;",
        vertical = true,
        spacing = 10,
        Box({
          spacing = 25,
          Box({
            class = "main",
            vertical = true,
            halign = "center",
            spacing = 4,
            W.Label({ class = "weather-icon-large", label = icon(w.code) }),
            W.Label({ class = "weather-temp-large", label = tostring(w.temp) .. w.temp_unit }),
            W.Label({ class = "weather-description", label = describe(w.code) }),
            W.Label({
              class = "weather-feels-like",
              label = "Feels like: " .. tostring(w.apparent_temp) .. w.temp_unit,
            }),
          }),
          Box({
            vertical = true,
            halign = "center",
            spacing = 5,
            Box({
              class = "weather-detail sun-detail",
              valign = "center",
              spacing = 5,
              vexpand = true,
              Box({ vertical = true, hexpand = true, W.Label({ class = "icon", label = "" }), W.Label(hhmm(first(d.sunrise))) }),
              W.Label({ class = "sun", label = "", hexpand = true }),
              Box({ vertical = true, hexpand = true, W.Label({ class = "icon", label = "" }), W.Label(hhmm(first(d.sunset))) }),
            }),
            Box({
              spacing = 5,
              vexpand = true,
              detail("", tostring(w.humidity) .. "%"),
              detail("", tostring(w.precipitation) .. " mm"),
            }),
            detail(
              "",
              string.format(
                "%s %s %s",
                w.wind_speed,
                w.wind_unit,
                DIRECTIONS[math.floor(((w.wind_direction or 0) % 360) / 22.5 + 0.5) % 16 + 1]
              )
            ),
          }),
        }),
        Box({
          spacing = 8,
          halign = "center",
          W.Entry({
            class = "weather-city-input",
            placeholder = "Enter city...",
            text = hw.settings:get("weather.city") or "",
            hexpand = true,
            setup = function(self)
              entry = self
            end,
            on_activate = function(_, text)
              M.set_city(text)
            end,
          }),
          W.Button({
            class = "weather-city-apply",
            tooltip = "Search city",
            label = "✔",
            on_click = function()
              if entry then
                M.set_city(entry:get("text"))
              end
            end,
          }),
          W.Button({
            class = "weather-city-clear",
            tooltip = "Auto (IP)",
            label = "󰦛",
            on_click = function()
              M.set_city("")
            end,
          }),
        }),
      }),
      Box({
        class = "weather-section",
        vertical = true,
        spacing = 12,
        W.Label({ class = "weather-subheading", label = "Today's Forecast" }),
        Box({
          class = "daily-forecast",
          spacing = 8,
          table.unpack((function()
            local items = {}
            for _, it in ipairs({
              { "Max", tostring(first(d.temperature_2m_max)) .. w.temp_unit },
              { "Min", tostring(first(d.temperature_2m_min)) .. w.temp_unit },
              { "Rain", tostring(first(d.precipitation_sum)) .. " mm" },
              { "Wind", tostring(first(d.wind_speed_10m_max)) .. " " .. w.wind_unit },
            }) do
              table.insert(
                items,
                Box({
                  class = "forecast-item",
                  vertical = true,
                  spacing = 2,
                  hexpand = true,
                  W.Label({ class = "forecast-label", label = it[1] }),
                  W.Label({ class = "forecast-value", label = it[2] }),
                })
              )
            end
            return items
          end)()),
        }),
      }),
    })
  end)
end

function M.button(on_active)
  local open = hw.state(false)
  return W.Button({
    class = "weather-button",
    tooltip = "click to open",
    css = M.data:map(function(w)
      if not w then
        return ""
      end
      return "background-color: " .. background(w.code) .. "; color: white;"
    end),
    on_click = function()
      open:set(not open:peek())
    end,
    popover = W.Popover({
      has_arrow = true,
      autohide = true,
      position = "bottom",
      open = open,
      on_change = function(self, visible)
        self:toggle_class("popover-open", visible)
        if not visible then
          open:set(false)
        end
        if on_active then
          on_active(visible)
        end
      end,
      details(),
    }),
    Box({
      class = "weather-button",
      spacing = 5,
      W.Label({
        label = M.data:map(function(w)
          return icon(w and w.code)
        end),
      }),
      W.Label({
        ellipsize = "end",
        label = M.data:map(function(w)
          if not w then
            return "Weather N/A"
          end
          return tostring(w.temp) .. w.temp_unit .. " " .. describe(w.code)
        end),
      }),
    }),
  })
end

-- Full weather card (AGS <Weather />), also used by the always-on widget.
M.details = details

return M
