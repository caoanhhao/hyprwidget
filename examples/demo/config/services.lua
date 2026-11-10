-- M2 services demo: a bottom bar per monitor showing live data from
-- hw.service.{hyprland,sysinfo,audio,battery,brightness}, and a request
-- dumping every service:
--
--   hyprwidget request services            -- JSON snapshot of all services
--   hyprwidget request apps fire           -- fuzzy app search
--   hyprwidget eval 'return hw.service.hyprland.focused_client()'
--
-- Services start lazily: nothing runs until a prop or method is touched.
local W = hw.widget

local function gib(bytes)
  return string.format("%.1fG", (bytes or 0) / 1073741824)
end

local function kib(bytes)
  return string.format("%.0fK", (bytes or 0) / 1024)
end

hw.on_request("services", function()
  local out = {}
  for _, name in ipairs(hw.service.names) do
    local svc = hw.service[name]
    local t = {}
    for _, p in ipairs(svc.props) do
      t[p] = svc[p]:get()
    end
    -- Hyprland's lists are long: keep the counts.
    if name == "hyprland" then
      t.clients, t.monitors, t.event = #(t.clients or {}), #(t.monitors or {}), nil
      t.workspaces = #(t.workspaces or {})
    elseif name == "apps" then
      t.list, t.history = #(t.list or {}), #(t.history or {})
    end
    out[name] = t
  end
  return out
end)

hw.on_request("apps", function(args)
  local names = {}
  for _, a in ipairs(hw.service.apps:query(table.concat(args, " "), 10)) do
    table.insert(names, a.name .. " (" .. a.id .. ")")
  end
  return names
end)

-- `hyprwidget request brightness +10` (or -10, or 50 to set it): for the
-- brightness keys in Hyprland's binds. Through hw.service.brightness, a held
-- key only queues the latest value instead of starting a ddcutil per repeat.
-- Returns the new brightness in percent.
hw.on_request("brightness", function(args)
  local b = hw.service.brightness
  local arg = args[1] or ""
  local n = tonumber(arg)
  if not n then
    error("usage: brightness +N | -N | N (percent)")
  end
  local current = b.brightness:peek()
  if current == nil then
    error("no display with adjustable brightness")
  end
  local v = arg:match("^[+-]") and current + n / 100 or n / 100
  v = math.max(0, math.min(1, v))
  b:set(v)
  return math.floor(v * 100 + 0.5)
end)

hw.per_monitor(function(monitor)
  local hypr = hw.service.hyprland
  local sys = hw.service.sysinfo
  local audio = hw.service.audio
  local battery = hw.service.battery

  -- Workspaces of this monitor; the focused one is highlighted.
  local workspaces = hw.computed(function()
    local focused = (hypr.focused_workspace() or {}).id
    local parts = {}
    for _, ws in ipairs(hypr.workspaces() or {}) do
      if ws.monitor == monitor.name and (ws.id or 0) > 0 then
        local label = tostring(ws.id)
        if ws.id == focused then
          label = "[" .. label .. "]"
        end
        table.insert(parts, label)
      end
    end
    local special = (hypr.special() or {})[monitor.name]
    if special then
      table.insert(parts, "★")
    end
    return table.concat(parts, " ")
  end)

  local title = hypr.focused_client:map(function(c)
    if not c then
      return "—"
    end
    local t = c.class .. ": " .. c.title
    -- Truncate on a UTF-8 boundary.
    local cut = utf8.offset(t, 61)
    return cut and (t:sub(1, cut - 1) .. "…") or t
  end)

  local stats = hw.computed(function()
    local cpu, mem, gpu, net = sys.cpu() or {}, sys.memory() or {}, sys.gpu() or {}, sys.network() or {}
    return string.format(
      "CPU %s%% %s°  RAM %s/%s  GPU %s%% %s°  ↓%s ↑%s",
      cpu.usage or "?",
      cpu.temp or "?",
      gib(mem.used),
      gib(mem.total),
      gpu.load or "?",
      gpu.temp or "?",
      kib(net.rx_speed),
      kib(net.tx_speed)
    )
  end)

  local volume = audio.speaker:map(function(s)
    if not s then
      return 0
    end
    return s.volume
  end)

  return W.Window({
    name = "services-" .. monitor.name,
    namespace = "hyprwidget-services",
    monitor = monitor,
    anchor = { "bottom", "left", "right" },
    margin = hw.opt.general.gaps_out,
    -- Hidden by default now that the real bar exists:
    --   hyprwidget toggle services-DP-2
    visible = false,

    W.CenterBox({
      class = "bar hw-panel",
      start = W.Box({
        spacing = hw.opt.general.gaps_in,
        W.Label({ label = workspaces, class = "workspaces" }),
        W.Label({ label = title, class = "title" }),
      }),
      center = W.Label({ label = stats, class = "stats" }),
      ["end"] = W.Box({
        spacing = hw.opt.general.gaps_in,
        W.Button({
          label = audio.speaker:map(function(s)
            if not s then
              return "no audio"
            end
            return (s.mute and "🔇 " or "🔊 ") .. math.floor(s.volume * 100 + 0.5) .. "%"
          end),
          on_click = function()
            audio:toggle_mute("speaker")
          end,
        }),
        W.Slider({
          value = volume,
          min = 0,
          max = 1,
          hexpand = false,
          width_request = 120,
          on_change = function(_, v)
            if math.abs(v - volume:peek()) > 0.005 then
              audio:set_volume("speaker", v)
            end
          end,
        }),
        W.Label({
          visible = battery.available,
          label = battery.percentage:map(function(p)
            return "🔋 " .. tostring(p or "?") .. "%"
          end),
        }),
      }),
    }),
  })
end)
