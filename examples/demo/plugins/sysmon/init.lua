-- Example plugin: system meters built on hw.service.sysinfo.
--
-- Runs sandboxed with no permissions (no hw.exec, io or os.execute);
-- everything comes from the sysinfo service. Use from Lua or JSON:
--   W.Sysmon{ items = { "cpu", "mem", "gpu" } }
--   { "type": "Sysmon", "items": ["cpu", "mem"] }
--   hyprwidget request sysmon        -- one-line summary
local fmt = require("lib.format")

local sys = hw.service.sysinfo

-- name -> { label, signal of a 0..100 percentage, detail signal }
local METERS = {
  cpu = {
    icon = "󰻠",
    pct = sys.cpu:map(function(c) return c and c.usage or 0 end),
    detail = sys.cpu:map(function(c)
      return c and fmt.temp(c.temp) or ""
    end),
  },
  mem = {
    icon = "󰍛",
    pct = sys.memory:map(function(m) return m and m.usage or 0 end),
    detail = sys.memory:map(function(m)
      return m and (fmt.gib(m.used) .. "/" .. fmt.gib(m.total)) or ""
    end),
  },
  gpu = {
    icon = "󰢮",
    pct = sys.gpu:map(function(g) return g and g.load or 0 end),
    detail = sys.gpu:map(function(g) return g and fmt.temp(g.temp) or "" end),
  },
}

local function meter(props)
  local m = METERS[props.name]
  if not m then error("unknown meter '" .. tostring(props.name) .. "'") end
  return W.Box({
    class = "sysmon-meter sysmon-" .. props.name,
    vertical = true,
    W.Box({
      W.Label({ label = m.icon, class = "sysmon-icon" }),
      W.Label({ label = m.pct:map(fmt.pct), class = "sysmon-value", hexpand = true, xalign = 0 }),
      W.Label({ label = m.detail, class = "sysmon-detail" }),
    }),
    W.Progress({ value = m.pct:map(function(p) return (p or 0) / 100 end) }),
  })
end

local summary = hw.computed(function()
  local c, m, g = sys.cpu() or {}, sys.memory() or {}, sys.gpu() or {}
  return string.format("cpu %s mem %s gpu %s", fmt.pct(c.usage), fmt.pct(m.usage), fmt.pct(g.load))
end)

return {
  widgets = {
    SysmonMeter = meter,
    Sysmon = function(props)
      local items = props.items or { "cpu", "mem", "gpu" }
      local children = {}
      for i, name in ipairs(items) do
        children[i] = meter({ name = name })
      end
      return W.Box({
        class = "sysmon " .. (props.class or ""),
        vertical = props.vertical ~= false,
        children = children,
      })
    end,
  },
  services = {
    sysmon = { summary = summary },
  },
  requests = {
    sysmon = function() return summary:get() end,
  },
  setup = function(info)
    print("sysmon " .. info.version .. " ready")
  end,
}
