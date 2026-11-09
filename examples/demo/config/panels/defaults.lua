-- Settings used by the side panels, with AGS' defaults
-- (constants/settings.constants.ts, constants/widget.constants.ts). Keys
-- missing from settings.json are created; existing AGS files work as is.
local M = {}

local function setting(name, value, type_, min, max, tooltip)
  return { name = name, value = value, type = type_, min = min, max = max, tooltip = tooltip }
end

-- constants/api.constants.ts
M.BOORU_APIS = {
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

    ["apiKeys.gelbooru"] = api_keys("Gelbooru"),
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
