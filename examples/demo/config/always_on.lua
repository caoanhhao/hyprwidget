-- Port of widgets/AlwaysOnWidget.tsx: the weather card on the desktop
-- (bottom layer, bottom-left), hidden while a window is fullscreen or when
-- settings `alwaysOnWidget.visibility` is off.
local W = hw.widget
local weather = require("config.bar.weather")

local visibility = hw.settings["alwaysOnWidget.visibility"]
local fullscreen = hw.service.hyprland.focused_client:map(function(c)
  return c ~= nil and c.fullscreen == 2
end)

hw.per_monitor(function(monitor)
  return {
    W.Window({
      name = "always-on-widget-" .. monitor.name,
      namespace = "always-on-widget",
      monitor = monitor,
      layer = "bottom",
      keymode = "on_demand",
      exclusivity = "ignore",
      anchor = { "left", "bottom" },
      margin = hw.opt.general.gaps_out,
      visible = hw.computed(function()
        return visibility() ~= false and not fullscreen()
      end),
      weather.details(),
    }),
  }
end)
