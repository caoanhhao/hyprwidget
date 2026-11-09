-- Port of widgets/UserPanel.tsx: full-screen overlay (`user-panel-<monitor>`,
-- SUPER + ESC / the control panel's  button) with logout / shutdown /
-- sleep / reboot around the profile picture. Escape closes it.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local FALLBACK_PFP = HOME .. "/.config/ags/assets/userpanel/archeclipse_default_pfp.jpg"

local function exists(path)
  local f = io.open(path, "r")
  if f then
    f:close()
  end
  return f ~= nil
end

-- AGS profilePicturePath: ~/.face.icon, else the bundled default.
local pfp = hw.state(exists(HOME .. "/.face.icon") and HOME .. "/.face.icon" or FALLBACK_PFP)
hw.watch_file(HOME .. "/.face.icon", function()
  pfp:set(exists(HOME .. "/.face.icon") and HOME .. "/.face.icon" or FALLBACK_PFP)
end)

local uptime = hw.poll(600000, "uptime -p")

local function css_url(path)
  return 'background-image: url("file://' .. path:gsub('"', '\\"') .. '");'
end

local function action(class, label, tooltip, run)
  return W.Button({
    hexpand = true,
    class = class .. " system-action",
    label = label,
    tooltip = tooltip,
    width_request = 350,
    height_request = 350,
    on_click = run,
  })
end

local function UserPanel(monitor)
  local name = "user-panel-" .. monitor.name
  local function hide()
    local w = hw.window.get(name)
    if w then
      w:hide()
    end
  end
  return W.Window({
    name = name,
    namespace = "user-panel",
    class = "user-panel",
    monitor = monitor,
    layer = "overlay",
    keymode = "on_demand",
    exclusivity = "ignore",
    anchor = { "top", "right", "left", "bottom" },
    visible = false,
    on_key = function(_, key)
      if key == "Escape" then
        hide()
        return true
      end
    end,
    Box({
      class = "display",
      vertical = true,
      spacing = 10,
      W.Overlay({
        child = Box({
          class = "display",
          halign = "center",
          valign = "center",
          hexpand = true,
          vexpand = true,
          W.Grid({
            class = "user-grid",
            halign = "center",
            valign = "center",
            columns = 2,
            row_spacing = 10,
            column_spacing = 10,
            action("logout", "󰍃", "logout from Hyprland", function()
              common.dispatch("hl.dsp.exit()")
            end),
            action("shutdown", "\u{f011}", "shutdown immediately", function()
              common.spawn({ "shutdown", "now" })
            end),
            action("sleep", "󰤄", "put system to sleep", function()
              hide()
              common.spawn({ "bash", "-c", "$HOME/.config/hypr/scripts/hyprlock.sh suspend" })
            end),
            action("reboot", "󰜉", "reboot immediately", function()
              common.spawn({ "reboot" })
            end),
          }),
        }),
        -- AGS UserProfile(minimal): the picture in the middle.
        Box({
          halign = "center",
          valign = "center",
          class = "user-profile",
          vertical = true,
          Box({
            class = "main",
            vertical = true,
            spacing = 10,
            Box({
              class = "profile-picture",
              halign = "center",
              width_request = 200,
              height_request = 200,
              css = pfp:map(function(p)
                return css_url(p) .. " background-size: cover; background-position: center; border-radius: 100px;"
              end),
            }),
            W.Label({
              class = "user-name",
              label = os.getenv("USER") or "",
            }),
            W.Label({
              class = "uptime",
              label = uptime:map(function(u)
                return u or ""
              end),
            }),
          }),
        }),
      }),
    }),
  })
end

hw.per_monitor(function(monitor)
  return { UserPanel(monitor) }
end)

