-- "hello" bar: one per monitor, spacing/margins/rounding follow Hyprland.
local W = hw.widget

local clock = hw.poll(1000, function()
  return os.date("%H:%M:%S")
end)
local clicks = hw.state(0)

-- hyprwidget request hello DP-2
hw.on_request("hello", function(args)
  return "hello " .. (args[1] or "world")
end)

hw.per_monitor(function(monitor)
  return W.Window({
    name = "bar-" .. monitor.name,
    namespace = "hyprwidget-bar",
    monitor = monitor,
    anchor = { "top", "left", "right" },
    exclusivity = "exclusive",
    margin = hw.opt.general.gaps_out,

    W.CenterBox({
      class = "bar hw-panel",
      start = W.Box({
        W.Label({ label = "hello, " .. monitor.name, class = "title" }),
      }),
      center = W.Label({ label = clock, class = "clock" }),
      ["end"] = W.Box({
        spacing = hw.opt.general.gaps_in,
        W.Label({
          label = hw.opt.decoration.rounding:map(function(r)
            return "rounding " .. tostring(r)
          end),
        }),
        W.Button({
          label = clicks:map(function(n)
            return "clicked " .. n .. "×"
          end),
          on_click = function()
            clicks:set(clicks:get() + 1)
          end,
        }),
      }),
    }),
  })
end)
