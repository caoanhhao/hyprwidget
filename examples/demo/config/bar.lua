-- Bar + BarHover on every monitor (AGS app.tsx perMonitorDisplay). The
-- modules live in config/bar/; settings use AGS' settings.json format.
require("config.bar.defaults").ensure()
local bar = require("config.bar.bar")

-- hyprwidget request hello DP-2
hw.on_request("hello", function(args)
  return "hello " .. (args[1] or "world")
end)

hw.per_monitor(bar)
