-- M4 demo: the "sysmon" plugin (plugins/sysmon) and W.Terminal, laid out
-- from JSON (layouts/sysmon.json). Hidden by default:
--   hyprwidget toggle sysmon-DP-2
--   hyprwidget theme nord | wallpaper | none
local W = hw.widget

hw.per_monitor(function(monitor)
  return W.Window({
    name = "sysmon-" .. monitor.name,
    namespace = "hyprwidget-sysmon",
    monitor = monitor,
    anchor = { "top", "right" },
    layer = "overlay",
    keymode = "on_demand", -- type into btop
    visible = false,
    hw.layout("layouts/sysmon.json"),
  })
end)
