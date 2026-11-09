-- Port of widgets/bar/components/Utilities.tsx: brightness and volume
-- (slider revealed on hover/change), battery + power profiles, system tray
-- (3 icons + overflow popover), resource rings with a hover popover, and the
-- control-panel button.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local function Brightness()
  local b = hw.service.brightness
  local screen = b.brightness
  return common.slider_revealer({
    visible = b.available,
    value = screen,
    tooltip = screen:map(function(v)
      return string.format("Brightness: %d%%", math.floor((v or 0) * 100 + 0.5))
    end),
    icon = W.Label({ label = screen:map(common.brightness_icon) }),
    label = W.Label({
      label = screen:map(function(v)
        return string.format("%d%%", math.floor((v or 0) * 100 + 0.5))
      end),
    }),
    on_change = function(v)
      b:set(v)
    end,
  })
end

local function Volume()
  local audio = hw.service.audio
  local volume = audio.speaker:map(function(s)
    return s and s.volume or 0
  end)
  return common.slider_revealer({
    value = volume,
    tooltip = volume:map(function(v)
      return string.format("Volume: %d%%\nClick to open Volume Mixer", math.floor(v * 100 + 0.5))
    end),
    icon = W.Image({ pixel_size = 11, icon = audio.speaker:map(common.volume_icon) }),
    label = W.Label({
      label = volume:map(function(v)
        return string.format("%d%%", math.floor(v * 100 + 0.5))
      end),
    }),
    on_change = function(v)
      audio:set_volume("speaker", v)
    end,
    on_click = function(_, button)
      if button == 1 then
        common.spawn("pavucontrol")
      end
    end,
  })
end

local function Battery()
  local upower = hw.service.upower
  local pp = hw.service.power_profiles
  local percent = upower.percentage:map(function(p)
    return string.format("%d%%", math.floor(p or 0))
  end)
  return W.MenuButton({
    visible = upower.is_present,
    tooltip_markup = hw.computed(function()
      return string.format("Battery: %s \nProfile: %s", percent(), pp.active_profile() or "?")
    end),
    Box({
      class = "battery",
      spacing = 5,
      W.Image({ icon = upower.icon_name }),
      W.Label({ label = percent }),
    }),
    popover = common.popover(
      Box({
        vertical = true,
        hw.For(pp.profiles, function(p)
          return W.Button({
            on_click = function()
              pp:set_active_profile(p.profile)
            end,
            W.Label({ label = p.profile, xalign = 0 }),
          })
        end, function(p)
          return p.profile
        end),
      })
    ),
  })
end

local function Tray(on_active)
  local tray = hw.service.tray
  local MAX_VISIBLE = 3
  local function slice(from, to)
    return tray.items:map(function(items)
      local out = {}
      for i = from, math.min(to, #(items or {})) do
        table.insert(out, items[i])
      end
      return out
    end)
  end
  local function item_button(item, with_label)
    local image = W.Image(common.icon_props(item.icon, 11))
    return W.MenuButton({
      class = "tray-icon",
      tooltip = item.tooltip_text ~= "" and item.tooltip_text or item.title,
      setup = function(self)
        tray:attach_menu(item.id, self)
      end,
      on_change = function(_, active)
        if on_active then
          on_active(active)
        end
      end,
      with_label and Box({
        spacing = 8,
        image,
        W.Label({ label = item.tooltip_text ~= "" and item.tooltip_text or item.title, xalign = 0 }),
      }) or image,
    })
  end
  local key = function(item)
    return item.id .. "|" .. tostring(item.icon) .. "|" .. tostring(item.tooltip_text)
  end
  local hidden = slice(MAX_VISIBLE + 1, math.huge)
  return Box({
    class = "system-tray",
    Box({
      spacing = 2,
      hw.For(slice(1, MAX_VISIBLE), function(item)
        return item_button(item, false)
      end, key),
    }),
    Box({
      spacing = 2,
      visible = hidden:map(function(l)
        return #l > 0
      end),
      W.MenuButton({
        class = "tray-icon tray-overflow",
        tooltip = "More icons",
        W.Image({ pixel_size = 11, icon = "view-more-symbolic" }),
        popover = common.popover(
          Box({
            class = "tray-popover",
            vertical = true,
            spacing = 5,
            hw.For(hidden, function(item)
              return item_button(item, true)
            end, key),
          }),
          on_active
        ),
      }),
    }),
  })
end

local function ResourceMonitor()
  local sys = hw.service.sysinfo
  local function ring(class, icon, sig, field, name)
    local value = sig:map(function(d)
      return d and d[field]
    end)
    return common.circular_progress({
      class = class,
      icon = icon,
      visible = value:map(function(v)
        return v ~= nil
      end),
      tooltip = value:map(function(v)
        return string.format("%s Usage %s%%", name, v and string.format("%.0f", v) or "?")
      end),
      value = value:map(function(v)
        return (v or 0) / 100
      end),
    })
  end
  local details = require("config.bar.system_resources")({ class = "resource-monitor-popover" })
  local props = common.hover_popover(details, { delay = 80 })
  props.class = "resource-monitor"
  props.on_click = function()
    common.dispatch("hl.dsp.focus({workspace = 5})")
  end
  props[1] = Box({
    spacing = 10,
    ring("cpu-monitor", "", sys.cpu, "usage", "CPU"),
    ring("ram-monitor", "", sys.memory, "usage", "RAM"),
    ring("gpu-monitor", "󱤟", sys.gpu, "load", "GPU"),
  })
  return Box(props)
end

local function ControlPanelButton(on_active)
  return W.MenuButton({
    W.Label({ label = "󱗼" }),
    popover = common.popover(require("config.bar.control_panel")(), on_active),
  })
end

return function(_, opts)
  return Box({
    class = "utilities",
    spacing = 5,
    halign = opts.halign,
    hexpand = true,
    Battery(),
    Brightness(),
    Volume(),
    Tray(opts.on_active),
    ResourceMonitor(),
    ControlPanelButton(opts.on_active),
  })
end
