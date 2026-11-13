-- Port of widgets/leftPanel/components/SettingsWidget.tsx: `W.SettingsWidget{}`
-- (shown by the left panel's widget selector) with AGS' global settings and
-- the Hyprland options.
--
-- Hyprland options are written like AGS did, to
-- ~/.config/hypr/config/custom/<option>.lua as `hl.config({ … })`, and applied
-- live (`hw.set_option`, through `hyprctl eval`). The "Widgets" category holds
-- options of hyprwidget itself, written to <config>/config/custom/<option>.lua.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local M = {}

local HYPR_THEME_CONF = require("lib.paths").theme_conf

local function setting(name, value, type_, min, max, tooltip)
  return { name = name, value = value, type = type_, min = min, max = max, tooltip = tooltip }
end

-- AGS constants/settings.constants.ts (the parts this widget edits).
M.hyprland = {
  { "general:border_size", setting("Border Size", 0, "int", 0, 10) },
  { "general:gaps_in", setting("Gaps In", 7, "int", 0, 20) },
  { "general:gaps_out", setting("Gaps Out", 10, "int", 0, 40) },
  { "decoration:rounding", setting("Rounding", 16, "int", 0, 50) },
  { "decoration:active_opacity", setting("Active Opacity", 0.9, "float", 0, 1) },
  { "decoration:inactive_opacity", setting("Inactive Opacity", 0.8, "float", 0, 1) },
  { "decoration:blur:enabled", setting("Blur Enabled", true, "bool", 0, 1) },
  { "decoration:blur:size", setting("Blur Size", 4, "int", 0, 10) },
  { "decoration:blur:passes", setting("Blur Passes", 4, "int", 0, 10) },
  { "decoration:blur:xray", setting("Blur Xray", false, "bool", 0, 1) },
  { "decoration:shadow:enabled", setting("Shadow Enabled", true, "bool", 0, 1) },
  { "decoration:shadow:range", setting("Shadow Range", 15, "int", 0, 20) },
  { "decoration:shadow:render_power", setting("Shadow Render Power", 3, "int", 0, 20) },
}

M.defaults = {
  ["alwaysOnWidget.visibility"] = setting("Always On Widget Visibility", true, "bool", 0, 1),
  ["keyStrokeVisualizer.visibility"] = setting("Key Stroke Visualizer Visibility", false, "bool", 0, 1),
  ["keyStrokeVisualizer.anchor"] = setting("Key Stroke Visualizer Anchor", { "bottom" }, "select", 0, 0),
  ["notifications.sound"] = setting("Notification Sound", true, "bool", 0, 1, "Play a sound when a notification arrives"),
  ["notifications.soundPath"] = setting(
    "Notification Sound Path",
    "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
    "string",
    1,
    512,
    "Path to the audio file played on notification"
  ),
  ["dynamicThemeColors"] = setting("Dynamic Theme Colors", true, "bool", 0, 1, "Enable dynamic theme colors based on the current wallpaper"),
  ["dynamicThemeVariants"] = setting(
    "Dynamic Theme Variants",
    true,
    "bool",
    0,
    1,
    "Enable dynamic theme variants (light/dark) based on the current wallpaper"
  ),
  ["hyprsunset.kelvin"] = 6500,
  ["fileManager"] = "nautilus",
  ["wallpaperSwitcher.category"] = "defaults/sfw",
  ["apiKeys.wallhaven.key"] = setting("Wallhaven API Key", "", "string", 1, 256),
}
for _, h in ipairs(M.hyprland) do
  M.defaults["hyprland." .. h[1]:gsub(":", ".")] = h[2]
end

function M.ensure()
  for key, value in pairs(M.defaults) do
    if hw.settings:get(key) == nil then
      hw.settings:set(key, value)
    end
  end
end
M.ensure()

local function notify(summary, body)
  hw.exec_async({ "notify-send", "-a", "hyprwidget", summary, body or "" }, function() end)
end

-- paths.theme_conf flags read by the theme scripts (AGS setThemeFlagInConf).
local function set_theme_flag(flag, on)
  if not HYPR_THEME_CONF then
    return
  end
  local v = on and "true" or "false"
  local script = string.format(
    [[f=%q; if [ -f "$f" ]; then sed -i 's/^%s=.*/%s=%s/' "$f"; grep -q '^%s=' "$f" || echo '%s=%s' >> "$f"; else echo '%s=%s' > "$f"; fi]],
    HYPR_THEME_CONF, flag, flag, v, flag, flag, v, flag, v
  )
  hw.exec_async({ "bash", "-c", script }, function(_, err)
    if err then
      notify("Error", err)
    end
  end)
end

local function fmt(meta, v)
  v = tonumber(v) or 0
  if meta.type == "int" then
    return tostring(math.floor(v + 0.5))
  end
  return string.format("%.2f", v)
end

-- Debounces `fn(value)` per key while a slider is dragged.
local pending = {}
local function debounced(key, ms, fn)
  return function(v)
    if pending[key] then
      pending[key]:cancel()
    end
    pending[key] = hw.timeout(ms, function()
      pending[key] = nil
      fn(v)
    end)
  end
end

local function title(meta)
  -- Wrap long names so they don't push the panel past `leftPanel.width`.
  return W.Label({ hexpand = true, xalign = 0, wrap = true, label = meta.name })
end

-- One AGSSetting row: slider (int/float), switch (bool), toggles (select),
-- entry (string). `on_change(value)` runs after the setting is stored.
function M.Setting(key, opts)
  opts = opts or {}
  local meta = opts.meta or hw.settings:meta(key) or setting(key, hw.settings:get(key), "string", 0, 0)
  local value = opts.value or hw.settings[key]
  local function store(v)
    if not opts.no_store then
      hw.settings:set(key, v)
    end
    if opts.on_change then
      opts.on_change(v)
    end
  end
  local row
  if meta.type == "int" or meta.type == "float" then
    local info = hw.state(fmt(meta, value:peek()))
    local apply = debounced(key, 150, store)
    row = Box({
      spacing = 5,
      title(meta),
      Box({
        hexpand = true,
        halign = "end",
        spacing = 5,
        W.Slider({
          class = "slider",
          width_request = 200,
          draw_value = false,
          -- The wheel scrolls the settings list, not the value.
          scroll = false,
          min = meta.min,
          max = meta.max,
          step = meta.type == "int" and 1 or 0.01,
          value = tonumber(value:peek()) or 0,
          on_change = function(_, v)
            if meta.type == "int" then
              v = math.floor(v + 0.5)
            else
              v = math.floor(v * 100 + 0.5) / 100
            end
            info:set(fmt(meta, v))
            apply(v)
          end,
        }),
        W.Label({ class = "setting-value", width_chars = 4, label = info }),
      }),
    })
  elseif meta.type == "bool" then
    row = Box({
      spacing = 5,
      title(meta),
      Box({
        hexpand = true,
        halign = "end",
        spacing = 5,
        W.Switch({
          valign = "center",
          active = value:map(function(v)
            return v == true
          end),
          on_change = function(_, on)
            if value:peek() ~= on then
              store(on)
            end
          end,
        }),
        W.Label({
          label = value:map(function(v)
            return v and "On" or "Off"
          end),
        }),
      }),
    })
  elseif meta.type == "select" then
    local buttons = {}
    for _, choice in ipairs(opts.choices or {}) do
      local enc = hw.json.encode(choice.value)
      table.insert(
        buttons,
        common.toggle({
          hexpand = true,
          label = choice.label,
          active = value:map(function(v)
            return hw.json.encode(v) == enc
          end),
          on_toggle = function(on)
            if on then
              store(choice.value)
            end
          end,
        })
      )
    end
    row = Box({ vertical = true, spacing = 10, title(meta), Box({ spacing = 5, table.unpack(buttons) }) })
  else
    row = Box({
      vertical = true,
      spacing = 10,
      title(meta),
      W.Entry({
        hexpand = true,
        text = tostring(value:peek() or ""),
        placeholder = "Enter " .. meta.name,
        on_change = function(self, text)
          local unsaved = text ~= tostring(value:peek() or "")
          self:toggle_class("unsaved", unsaved)
          self:set("tooltip_markup", unsaved and "<b>Unsaved Changes</b>\nPress Enter to save changes" or "")
        end,
        on_activate = function(self, text)
          if text ~= value:peek() then
            store(text)
            notify(meta.name, "Changed to " .. text)
            self:remove_class("unsaved")
            self:set("tooltip_markup", "")
          end
        end,
      }),
    })
  end
  return Box({ class = "setting", spacing = 5, tooltip_markup = meta.tooltip or "", row })
end

-- Current value of a Hyprland option from the running compositor.
local function hypr_value(name, meta)
  local ok, r = pcall(function()
    return hw.service.hyprland:message("j/getoption " .. name)
  end)
  if not ok or type(r) ~= "table" then
    return nil
  end
  if meta.type == "bool" then
    return r.int == 1
  end
  return r.int or r.float
end

-- Writes and applies one Hyprland option (AGS applyHyprlandSetting).
function M.apply_hypr(name, v)
  local ok, err = pcall(hw.set_option, name, v, "hypr")
  if not ok then
    notify("Hyprland setting", tostring(err))
  end
end

local function hyprland_settings()
  local rows = {}
  for _, h in ipairs(M.hyprland) do
    local name, meta = h[1], hw.settings:meta("hyprland." .. h[1]:gsub(":", ".")) or h[2]
    local key = "hyprland." .. name:gsub(":", ".")
    -- Show what Hyprland really uses; settings.json follows the UI.
    local live = hypr_value(name, meta)
    local value = hw.state(live == nil and hw.settings:get(key) or live)
    table.insert(
      rows,
      M.Setting(key, {
        meta = meta,
        value = value,
        on_change = function(v)
          value:set(v)
          M.apply_hypr(name, v)
        end,
      })
    )
  end
  return rows
end

-- Widget-only options (written to <config>/config/custom/).
local function widget_settings()
  local alpha = hw.option("blur:ignore_alpha")
  local meta = setting("Blur ignore alpha", alpha:peek() or 0.1, "float", 0, 1, "Layer rule ignore_alpha of hyprwidget's namespaces (blur:ignore_alpha)")
  local value = hw.state(tonumber(alpha:peek()) or 0.1)
  return {
    M.Setting("widgets.blur.ignore_alpha", {
      meta = meta,
      value = value,
      no_store = true,
      on_change = function(v)
        value:set(v)
        hw.set_option("blur:ignore_alpha", v, "widget")
      end,
    }),
  }
end

local function night_light()
  local sun = hw.service.hyprsunset
  local kelvin = hw.state(tonumber(sun.temperature:peek()) or hw.settings:get("hyprsunset.kelvin") or 6500)
  local gamma = hw.state(tonumber(sun.gamma:peek()) or 100)
  return Box({
    vertical = true,
    spacing = 16,
    visible = sun.available,
    M.Setting("hyprsunset.kelvin", {
      meta = setting("Night Light Temperature (K)", kelvin:peek(), "int", 1000, 6500),
      value = kelvin,
      on_change = function(v)
        kelvin:set(v)
        pcall(sun.set_temperature, sun, v)
      end,
    }),
    M.Setting("hyprsunset.gamma", {
      meta = setting("Night Light Gamma (%)", gamma:peek(), "int", 20, 100),
      value = gamma,
      no_store = true,
      on_change = function(v)
        gamma:set(v)
        pcall(sun.set_gamma, sun, v)
      end,
    }),
    W.Button({
      class = "reset-button",
      halign = "end",
      label = "Night Light Off",
      tooltip_markup = "hyprsunset identity (no filter)",
      on_click = function()
        pcall(sun.set_identity, sun)
      end,
    }),
  })
end

local FILE_MANAGERS = {
  { id = "nautilus", name = "Nautilus (GNOME)", command = "nautilus" },
  { id = "thunar", name = "Thunar (XFCE)", command = "thunar" },
  { id = "dolphin", name = "Dolphin (KDE)", command = "dolphin" },
  { id = "nemo", name = "Nemo (Cinnamon)", command = "nemo" },
  { id = "pcmanfm", name = "PCManFM", command = "pcmanfm" },
  { id = "ranger", name = "Ranger (Terminal)", command = "kitty ranger" },
}

local function file_managers()
  local installed = hw.state({})
  local script = "for c in " .. (function()
    local t = {}
    for _, fm in ipairs(FILE_MANAGERS) do
      table.insert(t, fm.command:match("^(%S+)"))
    end
    return table.concat(t, " ")
  end)() .. '; do command -v "$c" >/dev/null 2>&1 && echo "$c"; done; true'
  hw.exec_async({ "bash", "-c", script }, function(out)
    local found, list = {}, {}
    for line in (out or ""):gmatch("[^\n]+") do
      found[line] = true
    end
    for _, fm in ipairs(FILE_MANAGERS) do
      if found[fm.command:match("^(%S+)")] then
        table.insert(list, fm)
      end
    end
    installed:set(list)
  end)
  local current = hw.settings["fileManager"]
  return Box({
    vertical = true,
    spacing = 5,
    W.Label({ class = "subcategory-label", label = "File Manager", halign = "start" }),
    Box({
      class = "setting",
      spacing = 10,
      hw.For(installed, function(fm)
        return common.toggle({
          hexpand = true,
          class = "widget",
          label = fm.id,
          tooltip_markup = "<b>" .. fm.name .. "</b>\nCommand: " .. fm.command,
          active = current:map(function(c)
            return c == fm.id
          end),
          on_toggle = function(on)
            if on then
              hw.settings:set("fileManager", fm.id)
              notify("File Manager", "Changed to " .. fm.name)
            end
          end,
        })
      end, function(fm)
        return fm.id
      end),
    }),
  })
end

local function bar_layout()
  local layout = hw.settings["bar.layout"]
  return Box({
    vertical = true,
    spacing = 5,
    W.Label({ class = "subcategory-label", label = "bar Layout", halign = "start" }),
    Box({
      class = "setting",
      spacing = 10,
      hw.For(layout, function(w)
        return common.toggle({
          hexpand = true,
          class = "widget",
          label = w.name,
          active = hw.state(w.enabled == true),
          on_toggle = function(on)
            local list = hw.settings:get("bar.layout") or {}
            for _, it in ipairs(list) do
              if it.name == w.name then
                it.enabled = on
              end
            end
            hw.settings:set("bar.layout", list)
          end,
        })
      end, function(w)
        return w.name .. tostring(w.enabled)
      end),
    }),
  })
end

local function category(label, children)
  local t = { class = "category", vertical = true, spacing = 16, W.Label({ label = label, halign = "start" }) }
  for _, c in ipairs(children) do
    table.insert(t, c)
  end
  return Box(t)
end

local function sub(children)
  children.class = "sub-category"
  children.spacing = 5
  return Box(children)
end

local function reset()
  for key, value in pairs(M.defaults) do
    hw.settings:set(key, type(value) == "table" and value.value ~= nil and value.value or value)
  end
  set_theme_flag("autocolor", true)
  set_theme_flag("autovariant", true)
  for _, h in ipairs(M.hyprland) do
    M.apply_hypr(h[1], h[2].value)
  end
end

hw.define("SettingsWidget", function(props)
  local keystrokes = hw.service.keystrokes
  return Box({
    class = "settings " .. (props.class or ""),
    vertical = true,
    spacing = 5,
    W.Scrolled({
      vexpand = true,
      hscroll = "never",
      Box({
        vertical = true,
        spacing = 16,
        category("hyprwidget -- Global", {
          bar_layout(),
          M.Setting("bar.orientation"),
          M.Setting("alwaysOnWidget.visibility"),
          M.Setting("ui.opacity"),
          M.Setting("ui.scale"),
          M.Setting("ui.fontSize"),
        }),
        category("Api Keys", {
          sub({ M.Setting("apiKeys.wallhaven.key") }),
        }),
        category("Notifications", {
          M.Setting("notifications.sound"),
          sub({ M.Setting("notifications.soundPath") }),
        }),
        category("KeyStrokeVisualizer", {
          M.Setting("keyStrokeVisualizer.visibility", {
            on_change = function(on)
              local err = keystrokes.error:peek()
              if on and err then
                notify("Key Stroke Visualizer", err)
              end
            end,
          }),
          M.Setting("keyStrokeVisualizer.anchor", {
            choices = {
              { label = "Bottom Left", value = { "bottom", "left" } },
              { label = "Bottom", value = { "bottom" } },
              { label = "Bottom Right", value = { "bottom", "right" } },
            },
          }),
        }),
        category("Hyprland", hyprland_settings()),
        category("Night Light", { night_light() }),
        category("Widgets", widget_settings()),
        category("Custom", {
          M.Setting("dynamicThemeColors", {
            on_change = function(on)
              set_theme_flag("autocolor", on)
            end,
          }),
          M.Setting("dynamicThemeVariants", {
            on_change = function(on)
              set_theme_flag("autovariant", on)
            end,
          }),
          file_managers(),
        }),
      }),
    }),
    Box({
      spacing = 5,
      halign = "end",
      W.Button({
        class = "apply-button",
        label = "Apply Hyprland Settings",
        tooltip_markup = "Reload Hyprland's config (hyprctl reload)",
        on_click = function()
          common.spawn({ "hyprctl", "reload" })
        end,
      }),
      W.Button({
        class = "reset-button",
        label = "Reset to Default",
        on_click = reset,
      }),
    }),
  })
end)

return M
