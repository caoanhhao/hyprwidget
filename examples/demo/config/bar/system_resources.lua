-- Port of widgets/rightPanel/components/SystemResources.tsx (CPU / RAM / GPU
-- columns), used by the bar's resource monitor popover; data from
-- hw.service.sysinfo instead of the system-resources-loop-ags helper.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local GB = 1e9

local function opt(v, suffix, fmt)
  if v == nil then
    return "N/A"
  end
  return string.format(fmt or "%s", v) .. suffix
end

local function column(class, title, lines)
  local children = {
    class = "resource-column " .. class,
    vertical = true,
    spacing = 6,
    hexpand = true,
    W.Label({ class = "column-title", label = title, xalign = 0 }),
  }
  for _, l in ipairs(lines) do
    table.insert(children, W.Label({ class = "metric", label = l, xalign = 0 }))
  end
  return Box(children)
end

return function(opts)
  opts = opts or {}
  local sys = hw.service.sysinfo
  local updated = hw.computed(function()
    sys.cpu()
    return os.date("%H:%M:%S")
  end)
  local function f(sig, fn)
    return sig:map(function(d)
      return fn(d or {})
    end)
  end
  return Box({
    class = "system-resources " .. (opts.class or ""),
    vertical = true,
    spacing = 10,
    Box({
      class = "header",
      spacing = 8,
      W.Label({ class = "title", label = "System Resources", hexpand = true, halign = "start" }),
      W.Label({
        class = "updated-at",
        xalign = 1,
        label = updated:map(function(t)
          return "Updated: " .. t
        end),
      }),
    }),
    Box({
      class = "resource-columns",
      spacing = 10,
      vertical = opts.vertical or false,
      column("cpu", "CPU", {
        f(sys.cpu, function(c)
          return opt(c.usage, "%", "%.1f")
        end):map(function(s)
          return "Load: " .. s
        end),
        f(sys.cpu, function(c)
          return "Clock: " .. opt(c.clock_ghz, " GHz", "%.2f")
        end),
        f(sys.cpu, function(c)
          return "Temp: " .. opt(c.temp, "°C")
        end),
      }),
      column("ram", "RAM", {
        f(sys.memory, function(m)
          return "Total: " .. opt(m.total and m.total / GB, " GB", "%.2f")
        end),
        f(sys.memory, function(m)
          return "Used: " .. opt(m.used and m.used / GB, " GB", "%.2f")
        end),
        f(sys.memory, function(m)
          return "Free: " .. opt(m.available and m.available / GB, " GB", "%.2f")
        end),
      }),
      column("gpu", sys.gpu:map(function(g)
        return (g or {}).label or "GPU"
      end), {
        f(sys.gpu, function(g)
          return "Load: " .. opt(g.load, "%")
        end),
        f(sys.gpu, function(g)
          return "Memory: " .. opt(g.memory_used and g.memory_used / GB, " GB", "%.2f")
        end),
        f(sys.gpu, function(g)
          return "Temp: " .. opt(g.temp, "°C")
        end),
      }),
    }),
  })
end
