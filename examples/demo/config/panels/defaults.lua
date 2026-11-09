-- Settings used by the side panels, with AGS' defaults
-- (constants/settings.constants.ts, constants/widget.constants.ts). Keys
-- missing from settings.json are created; existing AGS files work as is.
local M = {}

local function setting(name, value, type_, min, max, tooltip)
  return { name = name, value = value, type = type_, min = min, max = max, tooltip = tooltip }
end

-- constants/api.constants.ts
M.BOORU_APIS = {
  { name = "Danbooru", value = "danbooru", url = "https://danbooru.donmai.us/", idSearchUrl = "https://danbooru.donmai.us/posts/" },
  { name = "Safebooru", value = "safebooru", url = "https://safebooru.donmai.us/", idSearchUrl = "https://safebooru.donmai.us/posts/" },
  { name = "Wallhaven", value = "wallhaven", url = "https://wallhaven.cc/", idSearchUrl = "https://wallhaven.cc/w/" },
}

-- Selectors as stored in settings (AGS keeps name/icon/enabled/tooltip).
M.RIGHT_WIDGETS = {
  { name = "Waifu", icon = "\u{f004}", enabled = true },
  { name = "Media", icon = "\u{f04b}", enabled = true },
  {
    name = "NotificationHistory",
    icon = "\u{eb9a}",
    tooltip = "<b>Right Click:</b> Toggle Notification Sound",
    enabled = true,
  },
  { name = "ScriptTimer", icon = "󰀠", enabled = false },
  { name = "Crypto", icon = "\u{f15a}", enabled = false },
  { name = "Calendar", icon = "󰃰", enabled = false },
  { name = "SystemResources", icon = "󰍛", enabled = true },
  { name = "VPN", icon = "\u{ed25}", enabled = false },
  { name = "Docker", icon = "󰡨", enabled = false },
  { name = "WayDroid", icon = "󰓷", enabled = false },
}

M.LEFT_WIDGETS = {
  { name = "UserProfile", icon = "\u{f007}", enabled = true },
  { name = "BooruViewer", icon = "\u{f03e}", enabled = false },
  { name = "Settings", icon = "\u{f013}", enabled = false },
  { name = "CustomScripts", icon = "\u{f120}", enabled = false },
  { name = "KeyBinds", icon = "\u{f11c}", enabled = false },
}

local function api_keys(name)
  return {
    user = setting(name .. " API User", "", "string", 1, 256),
    key = setting(name .. " API Key", "", "string", 1, 256),
  }
end

-- Current Hyprland value of `opt` (e.g. "general:gaps_in"), else `default`.
local function hypr(opt, default)
  local ok, v = pcall(function()
    return hw.option(opt):peek()
  end)
  if ok and type(v) == "table" then
    v = v.top or v[1]
  end
  if ok and (type(v) == "number" or type(v) == "boolean") then
    return v
  end
  return default
end

function M.defaults()
  return {
    ["rightPanel.exclusivity"] = true,
    ["rightPanel.lock"] = false,
    ["rightPanel.width"] = 250,
    ["rightPanel.widgets"] = M.RIGHT_WIDGETS,
    ["leftPanel.exclusivity"] = true,
    ["leftPanel.lock"] = false,
    ["leftPanel.width"] = 400,
    ["leftPanel.widget"] = M.LEFT_WIDGETS[1],

    ["waifuWidget.input_history"] = "",
    ["waifuWidget.visibility"] = true,
    ["waifuWidget.current"] = {},
    ["waifuWidget.api"] = M.BOORU_APIS[1],
    ["waifuWidget.random_interval"] = setting("Random waifu interval (seconds)", "600", "string", 0, 600000),

    ["booru.api"] = M.BOORU_APIS[1],
    ["booru.tags"] = {},
    ["booru.limit"] = 12,
    ["booru.page"] = 1,
    ["booru.columns"] = 2,
    ["booru.bookmarks"] = {},
    ["booru.selectedTab"] = M.BOORU_APIS[1].name,

    ["apiKeys.danbooru"] = api_keys("Danbooru"),
    ["apiKeys.gelbooru"] = api_keys("Gelbooru"),
    ["apiKeys.safebooru"] = api_keys("Safebooru"),
    ["apiKeys.wallhaven"] = api_keys("Wallhaven"),

    ["notifications.sound"] = setting(
      "Notification Sound",
      true,
      "bool",
      0,
      1,
      "Play a sound when a notification arrives"
    ),
    ["notifications.soundPath"] = setting(
      "Notification Sound Path",
      "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga",
      "string",
      1,
      512,
      "Path to the audio file played on notification"
    ),
    ["fileManager"] = "nautilus",
    ["alwaysOnWidget.visibility"] = setting("Always On Widget Visibility", true, "bool", 0, 1),
    ["keyStrokeVisualizer.visibility"] = setting("Key Stroke Visualizer Visibility", false, "bool", 0, 1),
    ["keyStrokeVisualizer.anchor"] = setting("Key Stroke Visualizer Anchor", { "bottom" }, "select", 0, 0),
    ["dynamicThemeColors"] = setting(
      "Dynamic Theme Colors",
      true,
      "bool",
      0,
      1,
      "Enable dynamic theme colors based on the current wallpaper"
    ),
    ["dynamicThemeVariants"] = setting(
      "Dynamic Theme Variants",
      true,
      "bool",
      0,
      1,
      "Enable dynamic theme variants (light/dark) based on the current wallpaper"
    ),

    -- Seeded from the running Hyprland so the Settings widget starts from
    -- the real values (AGS used fixed defaults).
    ["hyprland.general.border_size"] = setting("Border Size", hypr("general:border_size", 0), "int", 0, 10),
    ["hyprland.general.gaps_in"] = setting("Gaps In", hypr("general:gaps_in", 7), "int", 0, 20),
    ["hyprland.general.gaps_out"] = setting("Gaps Out", hypr("general:gaps_out", 10), "int", 0, 40),
    ["hyprland.decoration.rounding"] = setting("Rounding", hypr("decoration:rounding", 16), "int", 0, 50),
    ["hyprland.decoration.active_opacity"] = setting(
      "Active Opacity",
      hypr("decoration:active_opacity", 0.9),
      "float",
      0,
      1
    ),
    ["hyprland.decoration.inactive_opacity"] = setting(
      "Inactive Opacity",
      hypr("decoration:inactive_opacity", 0.8),
      "float",
      0,
      1
    ),
    ["hyprland.decoration.blur.enabled"] = setting("Blur Enabled", hypr("decoration:blur:enabled", true), "bool", 0, 1),
    ["hyprland.decoration.blur.size"] = setting("Blur Size", hypr("decoration:blur:size", 4), "int", 0, 10),
    ["hyprland.decoration.blur.passes"] = setting("Blur Passes", hypr("decoration:blur:passes", 4), "int", 0, 10),
    ["hyprland.decoration.blur.xray"] = setting("Blur Xray", hypr("decoration:blur:xray", false), "bool", 0, 1),
    ["hyprland.decoration.shadow.enabled"] = setting(
      "Shadow Enabled",
      hypr("decoration:shadow:enabled", true),
      "bool",
      0,
      1
    ),
    ["hyprland.decoration.shadow.range"] = setting("Shadow Range", hypr("decoration:shadow:range", 15), "int", 0, 20),
    ["hyprland.decoration.shadow.render_power"] = setting(
      "Shadow Render Power",
      hypr("decoration:shadow:render_power", 3),
      "int",
      0,
      20
    ),
  }
end

function M.ensure()
  for key, value in pairs(M.defaults()) do
    if hw.settings:get(key) == nil then
      hw.settings:set(key, value)
    end
  end
end

return M
