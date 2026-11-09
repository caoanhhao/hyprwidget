-- Port of widgets/ControlPanel.tsx (popover of the bar's 󱗼 button): volume
-- and brightness sliders, light/dark theme, pin bar, do-not-disturb, and
-- buttons toggling the user panel / app launcher / wallpaper switcher of
-- the bar's monitor (when those windows exist).
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local THEME_SCRIPT = HOME .. "/.config/hypr/theme/scripts/system-theme.sh"

-- AGS globalTheme: true = light.
local light = hw.state(false)
hw.exec_async({ "bash", "-c", THEME_SCRIPT .. " get" }, function(out)
  if out then
    light:set(out:find("light") ~= nil)
  end
end)

local function slider(icon, value, on_change, visible)
  return Box({
    class = "control-panel-slider",
    spacing = 5,
    visible = visible,
    icon,
    W.Slider({
      class = "slider",
      hexpand = true,
      draw_value = false,
      value = value:map(function(v)
        return math.max(0, math.min(1, tonumber(v) or 0))
      end),
      on_change = function(_, v)
        if math.abs(v - (tonumber(value:peek()) or 0)) > 0.005 then
          on_change(v)
        end
      end,
    }),
  })
end

local function window_button(class, label, tooltip, prefix)
  return W.Button({
    class = class,
    label = label,
    tooltip_markup = tooltip,
    on_click = function(self)
      local root = self:window()
      local mon = root and root:widget().name:match("^bar%-(.+)$")
      local win = mon and hw.window.get(prefix .. mon)
      if win then
        win:toggle()
      end
    end,
  })
end

return function()
  local audio = hw.service.audio
  local brightness = hw.service.brightness
  local volume = audio.speaker:map(function(s)
    return s and s.volume or 0
  end)
  local dnd = hw.settings["notifications.dnd"]
  local lock = hw.settings["bar.lock"]

  return Box({
    class = "control-panel",
    spacing = 15,
    vertical = true,
    Box({
      class = "sliders",
      spacing = 10,
      vertical = true,
      slider(
        W.Image({ class = "icon", pixel_size = 15, icon = audio.speaker:map(common.volume_icon) }),
        volume,
        function(v)
          audio:set_volume("speaker", v)
        end
      ),
      slider(
        W.Label({ class = "icon", label = brightness.brightness:map(common.brightness_icon) }),
        brightness.brightness,
        function(v)
          brightness:set(v)
        end,
        brightness.available
      ),
    }),
    Box({
      spacing = 5,
      halign = "center",
      common.toggle({
        class = "theme icon",
        active = light,
        label = light:map(function(l)
          return l and "" or ""
        end),
        tooltip_markup = light:map(function(l)
          return l and "Switch to Dark Theme" or "Switch to Light Theme"
        end),
        on_toggle = function(on)
          hw.exec_async({ "bash", "-c", THEME_SCRIPT .. " switch " .. (on and "light" or "dark") }, function(_, err)
            if not err then
              light:set(on)
            end
          end)
        end,
      }),
      common.toggle({
        class = "panel-lock icon",
        active = lock,
        label = lock:map(function(l)
          return l and "" or ""
        end),
        tooltip_markup = lock:map(function(l)
          return l and "Unlock Bar" or "Lock Bar"
        end),
        on_toggle = function(on)
          hw.settings:set("bar.lock", on)
        end,
      }),
      common.toggle({
        class = "dnd-toggle",
        active = dnd,
        label = dnd:map(function(d)
          return d and "" or ""
        end),
        tooltip_markup = dnd:map(function(d)
          return d and "Disable Do Not Disturb" or "Enable Do Not Disturb"
        end),
        on_toggle = function(on)
          hw.settings:set("notifications.dnd", on)
        end,
      }),
      window_button("user-panel", "", "User Panel\n<b>SUPER + ESC</b>", "user-panel-"),
      window_button(nil, "", "App Launcher\n<b>SUPER</b>", "app-launcher-"),
      window_button("wallpaper-switcher", "󰸉", "Wallpaper Switcher\n<b>SUPER + W</b>", "wallpaper-switcher-"),
    }),
  })
end
