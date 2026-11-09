-- Left panel widgets (widgets/leftPanel/components/*.tsx): UserProfile,
-- KeyBinds, CustomScripts and Settings. BooruViewer is in booru.lua.
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
  return common.file_exists(face) and face or (CONFIG_DIR .. "/assets/userpanel/archeclipse_default_pfp.jpg")
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
  hw.exec_async({ "bash", SCRIPTS .. "get-keybinds.sh" }, function(out, err)
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
      cmd = 'bash -c "$HOME/.config/hypr/scripts/screenshot.sh --area"',
    },
    {
      name = "Record Screen",
      icon = "\u{eba7}",
      description = "Record entire screen",
      keybind = { "SUPER", "SHIFT", "R" },
      app = "wf-recorder",
      cmd = 'bash -c "$HOME/.config/hypr/scripts/screenrecord.sh --now"',
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

-- Settings ------------------------------------------------------------------

local HYPR_CUSTOM_DIR = HOME .. "/.config/hypr/config/custom"
local HYPR_THEME_CONF = HOME .. "/.config/hypr/theme/theme.conf"

local FILE_MANAGERS = {
  { id = "nautilus", name = "Nautilus (GNOME)", command = "nautilus" },
  { id = "thunar", name = "Thunar (XFCE)", command = "thunar" },
  { id = "dolphin", name = "Dolphin (KDE)", command = "dolphin" },
  { id = "nemo", name = "Nemo (Cinnamon)", command = "nemo" },
  { id = "pcmanfm", name = "PCManFM", command = "pcmanfm" },
  { id = "ranger", name = "Ranger (Terminal)", command = "kitty ranger" },
}

local function set_theme_flag(flag, on)
  local v = on and "true" or "false"
  common.run({
    "bash",
    "-c",
    string.format(
      [=[f=%s; if [[ -f "$f" ]]; then sed -i 's/^%s=.*/%s=%s/' "$f"; grep -q '^%s=' "$f" || echo '%s=%s' >> "$f"; else mkdir -p "$(dirname "$f")"; echo '%s=%s' > "$f"; fi]=],
      q(HYPR_THEME_CONF), flag, flag, v, flag, flag, v, flag, v
    ),
  }, "Error")
end

local function lua_value(v)
  if type(v) == "boolean" or type(v) == "number" then
    return tostring(v)
  elseif type(v) == "table" then
    local items = {}
    for _, x in ipairs(v) do
      table.insert(items, lua_value(x))
    end
    return "{ " .. table.concat(items, ", ") .. " }"
  end
  return string.format("%q", tostring(v))
end

-- Writes ~/.config/hypr/config/custom/<key>.lua and applies it live (AGS
-- applyHyprlandSetting). `key` uses ':' ("decoration:blur:size").
local function apply_hypr(key, value)
  local expr = lua_value(value)
  local parts = {}
  for p in key:gmatch("[^:]+") do
    table.insert(parts, p)
  end
  for i = #parts, 1, -1 do
    local k = parts[i]:match("^[%a_][%w_]*$") and parts[i] or string.format("[%q]", parts[i])
    expr = "{ " .. k .. " = " .. expr .. " }"
  end
  local path = HYPR_CUSTOM_DIR .. "/" .. key .. ".lua"
  hw.exec_async({
    "bash", "-c",
    string.format("mkdir -p %s && printf '%%s\\n' %s > %s && hyprctl keyword %s %s",
      q(HYPR_CUSTOM_DIR), q("hl.config(" .. expr .. ")"), q(path), q(key), q(tostring(value))),
  }, function(_, err)
    if err then
      common.notify("Error", err)
    end
  end)
end

local function fmt_value(meta, v)
  if meta.type == "int" then
    return tostring(math.floor((v or 0) + 0.5))
  end
  return string.format("%.2f", v or 0)
end

-- One AGSSetting row; the control depends on its `type`.
local function Setting(key, opts)
  opts = opts or {}
  local meta = hw.settings:meta(key) or {}
  local value = hw.settings[key]
  local title = W.Label({ hexpand = true, xalign = 0, label = meta.name or key })
  local control
  if meta.type == "int" or meta.type == "float" then
    local timer
    control = Box({
      spacing = 5,
      title,
      Box({
        hexpand = true,
        halign = "end",
        spacing = 5,
        W.Slider({
          class = "slider",
          draw_value = false,
          min = meta.min or 0,
          max = meta.max or 1,
          step = meta.type == "int" and 1 or 0.01,
          width_request = hw.settings["leftPanel.width"]:map(function(w)
            return math.floor((w or 400) / 2)
          end),
          value = value,
          on_change = function(_, v)
            v = meta.type == "int" and math.floor(v + 0.5) or tonumber(string.format("%.2f", v))
            if v == value:peek() then
              return
            end
            if timer then
              timer:cancel()
            end
            timer = hw.timeout(150, function()
              timer = nil
              hw.settings:set(key, v)
              if opts.on_change then
                opts.on_change(v)
              end
            end)
          end,
        }),
        W.Label({
          hexpand = true,
          label = value:map(function(v)
            return fmt_value(meta, v)
          end),
        }),
      }),
    })
  elseif meta.type == "bool" then
    control = Box({
      spacing = 5,
      title,
      Box({
        hexpand = true,
        halign = "end",
        spacing = 5,
        W.Switch({
          active = value,
          valign = "center",
          on_change = function(_, on)
            if on ~= value:peek() then
              hw.settings:set(key, on)
              if opts.on_change then
                opts.on_change(on)
              end
            end
          end,
        }),
        W.Label({
          hexpand = true,
          label = value:map(function(v)
            return v and "On" or "Off"
          end),
        }),
      }),
    })
  elseif meta.type == "select" then
    local row = { spacing = 5 }
    for _, choice in ipairs(opts.choices or {}) do
      table.insert(
        row,
        common.toggle({
          hexpand = true,
          label = choice.label,
          active = value:map(function(cur)
            return hw.json.encode(cur) == hw.json.encode(choice.value)
          end),
          on_toggle = function()
            hw.settings:set(key, choice.value)
            if opts.on_change then
              opts.on_change(choice.value)
            end
          end,
        })
      )
    end
    control = Box({ vertical = true, spacing = 10, title, Box(row) })
  elseif meta.type == "string" then
    control = Box({
      vertical = true,
      spacing = 10,
      title,
      W.Entry({
        hexpand = true,
        text = tostring(value:peek() or ""),
        placeholder = "Enter " .. (meta.name or key),
        on_change = function(self, text)
          local dirty = text ~= tostring(value:peek() or "")
          self:toggle_class("unsaved", dirty)
          self:set("tooltip_markup", dirty and "<b>Unsaved Changes</b>\nPress Enter to save changes" or "")
        end,
        on_activate = function(self, text)
          if text ~= value:peek() then
            hw.settings:set(key, text)
            common.notify(meta.name or key, "Changed to " .. text)
            if opts.on_change then
              opts.on_change(text)
            end
          end
          self:toggle_class("unsaved", false)
          self:set("tooltip_markup", "")
        end,
      }),
    })
  else
    control = W.Label({ halign = "end", label = "Unsupported setting type" })
  end
  return Box({ class = "setting", spacing = 5, tooltip_markup = meta.tooltip or "", control })
end

local function category(title, children)
  local box = { class = "category", vertical = true, spacing = 16, W.Label({ label = title, halign = "start" }) }
  for _, c in ipairs(children) do
    table.insert(box, c)
  end
  return Box(box)
end

local function sub(children)
  children.class = "sub-category"
  children.spacing = 5
  return Box(children)
end

local function BarLayout()
  local layout = hw.settings["bar.layout"]
  return Box({
    vertical = true,
    spacing = 5,
    W.Label({ class = "subcategory-label", label = "bar Layout", halign = "start" }),
    Box({
      class = "setting",
      spacing = 10,
      children = layout:map(function(list)
        local out = {}
        for i, item in ipairs(list or {}) do
          table.insert(
            out,
            W.Button({
              hexpand = true,
              class = "widget drag" .. (item.enabled and " checked" or ""),
              label = item.name,
              tooltip_markup = "<b>Scroll To Move</b>\n" .. item.name,
              on_click = function()
                local new = {}
                for j, it in ipairs(hw.settings:get("bar.layout") or {}) do
                  local copy = { name = it.name, icon = it.icon, enabled = it.enabled }
                  if j == i then
                    copy.enabled = not copy.enabled
                  end
                  table.insert(new, copy)
                end
                hw.settings:set("bar.layout", new)
              end,
              -- AGS reorders by drag & drop; scrolling moves the item here.
              on_scroll = function(_, _, dy)
                local new = hw.settings:get("bar.layout") or {}
                local j = i + (dy > 0 and 1 or -1)
                if j >= 1 and j <= #new then
                  new[i], new[j] = new[j], new[i]
                  hw.settings:set("bar.layout", new)
                end
                return true
              end,
            })
          )
        end
        return out
      end),
    }),
  })
end

local function FileManagers()
  local installed = hw.state({})
  local found = {}
  local pending = #FILE_MANAGERS
  for _, fm in ipairs(FILE_MANAGERS) do
    local bin = fm.command:match("^(%S+)")
    hw.exec_async({ "bash", "-c", "command -v " .. q(bin) .. " >/dev/null && echo yes || echo no" }, function(out)
      if (out or ""):match("yes") then
        found[fm.id] = true
      end
      pending = pending - 1
      if pending == 0 then
        local list = {}
        for _, f in ipairs(FILE_MANAGERS) do
          if found[f.id] then
            table.insert(list, f)
          end
        end
        installed:set(list)
      end
    end)
  end
  local current = hw.settings["fileManager"]
  return Box({
    vertical = true,
    spacing = 5,
    W.Label({ class = "subcategory-label", label = "File Manager", halign = "start" }),
    Box({
      class = "setting",
      spacing = 10,
      children = installed:map(function(list)
        local out = {}
        for _, fm in ipairs(list) do
          table.insert(
            out,
            common.toggle({
              hexpand = true,
              class = "widget",
              label = fm.id,
              tooltip_markup = "<b>" .. fm.name .. "</b>\nCommand: " .. fm.command,
              active = current:map(function(c)
                return c == fm.id
              end),
              on_toggle = function()
                hw.settings:set("fileManager", fm.id)
                common.notify("File Manager", "Changed to " .. fm.name)
              end,
            })
          )
        end
        return out
      end),
    }),
  })
end

-- Every leaf under settings `hyprland`, as "general.gaps_in" keys.
local function hypr_keys()
  local keys = {}
  local function walk(prefix, t)
    if type(t) ~= "table" then
      return
    end
    local names = {}
    for k in pairs(t) do
      table.insert(names, k)
    end
    table.sort(names)
    for _, k in ipairs(names) do
      local full = prefix == "" and k or (prefix .. "." .. k)
      if hw.settings:meta("hyprland." .. full) then
        table.insert(keys, full)
      else
        walk(full, t[k])
      end
    end
  end
  walk("", hw.settings:get("hyprland"))
  return keys
end

hw.define("Settings", function(props)
  local hypr_rows = {}
  for _, key in ipairs(hypr_keys()) do
    table.insert(
      hypr_rows,
      Setting("hyprland." .. key, {
        on_change = function(v)
          apply_hypr((key:gsub("%.", ":")), v)
        end,
      })
    )
  end
  return Box({
    class = common.class("settings", props.class),
    vertical = true,
    spacing = 5,
    W.Scrolled({
      vexpand = true,
      hscroll = "never",
      Box({
        vertical = true,
        spacing = 16,
        category("AGS -- Global", {
          BarLayout(),
          Setting("bar.orientation"),
          Setting("alwaysOnWidget.visibility"),
          -- config/theme.lua rebuilds the CSS when ui.* change.
          Setting("ui.opacity"),
          Setting("ui.scale"),
          Setting("ui.fontSize"),
        }),
        category("AGS -- Api Keys", {
          sub({ Setting("apiKeys.danbooru.user"), Setting("apiKeys.danbooru.key") }),
          sub({ Setting("apiKeys.safebooru.user"), Setting("apiKeys.safebooru.key") }),
          sub({ Setting("apiKeys.wallhaven.key") }),
        }),
        category("Notifications", {
          Setting("notifications.sound"),
          sub({ Setting("notifications.soundPath") }),
        }),
        category("AGS -- KeyStrokeVisualizer", {
          Setting("keyStrokeVisualizer.visibility"),
          Setting("keyStrokeVisualizer.anchor", {
            choices = {
              { label = "Bottom Left", value = { "bottom", "left" } },
              { label = "Bottom", value = { "bottom" } },
              { label = "Bottom Right", value = { "bottom", "right" } },
            },
          }),
        }),
        category("Hyprland", hypr_rows),
        category("Custom", {
          Setting("dynamicThemeColors", {
            on_change = function(on)
              set_theme_flag("autocolor", on)
            end,
          }),
          Setting("dynamicThemeVariants", {
            on_change = function(on)
              set_theme_flag("autovariant", on)
            end,
          }),
          FileManagers(),
        }),
        category("Widget Settings", {
          sub({ Setting("waifuWidget.random_interval") }),
        }),
      }),
    }),
    Box({
      spacing = 5,
      halign = "end",
      W.Button({
        class = "apply-button",
        label = "Apply Hyprland Settings",
        halign = "end",
        on_click = function()
          exec_cmd("hyprctl reload")
        end,
      }),
      W.Button({
        class = "reset-button",
        label = "Reset to Default",
        halign = "end",
        on_click = function()
          local defaults = require("config.panels.defaults").defaults()
          for key, v in pairs(defaults) do
            if type(v) == "table" and v.value ~= nil and hw.settings:meta(key) then
              hw.settings:set(key, v.value)
            end
          end
        end,
      }),
    }),
  })
end)
