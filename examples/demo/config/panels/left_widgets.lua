-- Left panel widgets (widgets/leftPanel/components/*.tsx): UserProfile,
-- KeyBinds and CustomScripts. BooruViewer is in booru.lua; Settings is
-- W.SettingsWidget (config/settings_widget.lua, M4b).
local W = hw.widget
local common = require("config.panels.common")
local bar_common = require("config.bar.common")
local booru = require("config.panels.booru")
local Box = common.Box
local q = common.sh_quote

local SCRIPTS = CONFIG_DIR .. "/scripts/"

-- ~/.face.icon, else the bundled default (AGS variables.ts).
local function profile_picture()
  local face = HOME .. "/.face.icon"
  return common.file_exists(face) and face or (CONFIG_DIR .. "/assets/userpanel/default_pfp.jpg")
end

-- UserProfile ---------------------------------------------------------------

hw.define("UserProfile", function(props)
  local status = hw.state("idle")
  local text = hw.state("Pick a profile picture")
  local size = hw.settings["leftPanel.width"]:map(function(w)
    return math.floor((w or 400) / 2)
  end)
  return Box({
    class = common.class("user-profile", props.class),
    vertical = true,
    spacing = 10,
    Box({
      class = "main",
      vertical = true,
      spacing = 10,
      Box({
        halign = "center",
        W.Button({
          class = "profile-picture",
          tooltip_markup = "Click to set up profile picture",
          on_click = function()
            hw.exec_async({
              "zenity", "--file-selection", "--title=Select Profile Picture",
              "--file-filter=Images (png, jpg, webp) | *.png *.jpg *.jpeg *.webp",
            }, function(out)
              local path = (out or ""):match("^%s*(.-)%s*$")
              if path ~= "" then
                booru.set_profile_avatar(path, nil, function(s, t)
                  status:set(s)
                  text:set(t)
                end)
              end
            end)
          end,
          hw.With(booru.avatar_changed, function()
            return common.Picture({ width = size, height = size, file = profile_picture() })
          end),
        }),
      }),
    }),
    props.minimal ~= true
        and common.Progress({ status = status, text = text, class = "profile-progress", show_when_idle = true })
      or nil,
  })
end)

-- KeyBinds ------------------------------------------------------------------

hw.define("KeyBinds", function(props)
  local binds = hw.state({})
  hw.exec_async({ "bash", SCRIPTS .. "get-keybinds.sh", require("lib.paths").keybinds }, function(out, err)
    local ok, parsed = pcall(hw.json.decode, out or "")
    if err or not ok or type(parsed) ~= "table" then
      print("KeyBinds: " .. tostring(err or parsed))
      return
    end
    binds:set(parsed)
  end)
  return W.Scrolled({
    class = props.class,
    hexpand = true,
    vexpand = true,
    hscroll = "never",
    hw.With(binds, function(all)
      local categories = {}
      for name in pairs(all or {}) do
        table.insert(categories, name)
      end
      table.sort(categories)
      local box = { class = "keybinds", vertical = true, hexpand = true, spacing = 10 }
      for _, category in ipairs(categories) do
        local cat = { class = "keybind-category", vertical = true, hexpand = true, spacing = 5 }
        table.insert(cat, W.Label({ class = "keybind-category-title", label = category, xalign = 0 }))
        for _, bind in ipairs(all[category]) do
          table.insert(
            cat,
            Box({
              class = "keybind-box",
              spacing = 5,
              W.Label({ class = "keybind-description", label = bind.description or "", wrap = true, hexpand = true, xalign = 0 }),
              common.KeyBind(bind.keys),
            })
          )
        end
        table.insert(box, Box(cat))
      end
      return Box(box)
    end),
  })
end)

-- CustomScripts -------------------------------------------------------------

local function exec_cmd(cmd)
  bar_common.dispatch(string.format("hl.dsp.exec_cmd(%q)", cmd))
end

-- constants/customScript.constant.ts (the entries enabled there).
local function custom_scripts()
  local fm = hw.settings:get("fileManager") or "nautilus"
  return {
    {
      name = "Update Packages",
      icon = "󰏗",
      description = "Update Packages (pacman)",
      run = function(done)
        hw.exec_async({ "kitty", "--class=package-update-script", "-e", SCRIPTS .. "installupdates.sh" }, done)
      end,
      status = function(cb)
        hw.exec_async({ "bash", SCRIPTS .. "updates.sh" }, function(out)
          out = (out or ""):match("^%s*(.-)%s*$")
          cb((out ~= "" and out ~= "0") and (out .. " \u{f1b2}") or "")
        end)
      end,
    },
    { name = "Visual Studio Code", icon = "󰨞", description = "Code Editor", app = "code", package = "visual-studio-code-bin", cmd = "code" },
    { name = "Orca", icon = "󱢴", description = "Orca IDE", app = "orca-ide", package = "stably-orca-bin", cmd = "orca-ide open" },
    { name = "Steam", icon = "\u{f1b6}", description = "Game Launcher", app = "steam", cmd = "steam" },
    { name = "Telegram", icon = "\u{e217}", description = "Telegram", app = "Telegram", cmd = "Telegram" },
    { name = "Discord", icon = "\u{f1ff}", description = "Discord", app = "discord", cmd = "discord" },
    { name = fm .. " File Manager", icon = "󰉋", description = "Open " .. fm, keybind = { "HYPER", "E" }, cmd = fm },
    {
      name = "System Monitor",
      icon = "󰍛",
      description = "Open system monitor",
      app = "btop",
      keybind = { "SUPER", "P" },
      dispatch = "hl.dsp.exec_cmd('kitty -e btop', { workspace = 5, float = false })",
    },
    {
      name = "Screenshot Area",
      icon = "\u{eb4c}",
      description = "Select area to screenshot",
      keybind = { "SUPER", "CTRL", "SHIFT", "S" },
      app = "grimblast",
      package = "grimblast-git",
      cmd = require("lib.paths").screenshot_area or 'grimblast copysave area',
    },
    {
      name = "Record Screen",
      icon = "\u{eba7}",
      description = "Record entire screen",
      keybind = { "SUPER", "SHIFT", "R" },
      app = "wf-recorder",
      cmd = require("lib.paths").screenrecord or 'wf-recorder -f "$HOME/Videos/recording-$(date +%s).mp4"',
    },
    { name = "Volume Control", icon = "\u{f057e}", description = "Adjust volume", app = "pavucontrol", cmd = "pavucontrol" },
    {
      name = "Clear Clipboard",
      icon = "\u{f00e2}",
      description = "Clear clipboard history",
      app = "wl-copy",
      package = "wl-clipboard",
      cmd = "wl-copy --clear",
    },
    { name = "HyprPicker", icon = "\u{f1fb}", description = "Color Picker for Hyprland", app = "hyprpicker", cmd = "hyprpicker" },
    {
      name = "Refresh Hyprland",
      icon = "\u{f0453}",
      description = "Refresh Hyprland (reload config)",
      keybind = { "SHIFT", "HYPER", "R" },
      cmd = "hyprctl reload",
    },
  }
end

local function ScriptRow(script)
  local badge = hw.state("")
  local installed = hw.state(true)
  local function update_badge()
    if script.status then
      script.status(function(b)
        badge:set(b)
      end)
    end
  end
  update_badge()
  if script.app then
    hw.exec_async({ "bash", "-c", "command -v " .. q(script.app) .. " >/dev/null 2>&1 && echo true || echo false" }, function(out)
      installed:set((out or ""):match("true") ~= nil)
    end)
  end
  return Box({
    spacing = 5,
    W.Button({
      class = "script",
      hexpand = true,
      sensitive = installed,
      tooltip = installed:map(function(i)
        return i and script.description or (script.description .. " (Requires installation)")
      end),
      on_click = function()
        if script.run then
          script.run(function(_, err)
            if err then
              common.notify("Error", "Failed to run " .. script.name .. ".")
            end
            update_badge()
          end)
        elseif script.dispatch then
          bar_common.dispatch(script.dispatch)
        else
          exec_cmd(script.cmd)
        end
      end,
      Box({
        spacing = 10,
        W.Label({ class = "icon", halign = "start", label = script.icon }),
        W.Label({ class = "name", halign = "start", wrap = true, hexpand = true, label = script.name }),
        script.keybind and common.KeyBind(script.keybind) or nil,
      }),
    }),
    W.Button({
      class = "install-button",
      label = "\u{f019}",
      tooltip = "Install " .. tostring(script.app),
      visible = installed:map(function(i)
        return not i
      end),
      on_click = function()
        local pkg = script.package or script.app
        local cmd = "if command -v yay >/dev/null 2>&1; then yay -S "
          .. pkg
          .. "; elif command -v paru >/dev/null 2>&1; then paru -S "
          .. pkg
          .. "; else pacman -S "
          .. pkg
          .. "; fi"
        common.run({ "kitty", "-e", "bash", "-c", cmd }, "Error")
      end,
    }),
    W.Label({
      class = "status-badge",
      label = badge,
      visible = badge:map(function(b)
        return b ~= ""
      end),
    }),
  })
end

hw.define("CustomScripts", function(props)
  return W.Scrolled({
    class = props.class,
    hexpand = true,
    vexpand = true,
    hscroll = "never",
    hw.With(hw.settings["fileManager"], function()
      local box = { class = "custom-scripts", vertical = true, hexpand = true, spacing = 10 }
      for _, s in ipairs(custom_scripts()) do
        table.insert(box, ScriptRow(s))
      end
      return Box(box)
    end),
  })
end)
