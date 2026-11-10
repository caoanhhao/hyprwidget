-- Settings used by the bar, with AGS' defaults (constants/settings.constants.ts).
-- settings.json keeps AGS' format, so an existing AGS file works as is; keys
-- missing from it are created here (like AGS' autoCreateSettings).
local M = {}

local function setting(name, value, type_, min, max)
  return { name = name, value = value, type = type_, min = min, max = max }
end

M.defaults = {
  ["dateFormat"] = "%H:%M",
  ["ui.opacity"] = setting("Opacity", 0.5, "float", 0, 1),
  ["ui.scale"] = setting("Scale", 10, "int", 10, 30),
  ["ui.fontSize"] = setting("Font Size", 12, "int", 10, 30),
  ["bar.lock"] = true,
  -- Off: while unlocked (auto-hiding), the bar floats over windows instead of
  -- reserving space, like the side panels, so showing it never resizes the
  -- tiled windows. A locked bar always reserves its space.
  ["bar.exclusivity"] = false,
  ["bar.orientation"] = setting("Orientation", true, "bool", 0, 1),
  ["bar.layout"] = {
    { name = "workspaces", icon = "󰒘", enabled = true },
    { name = "information", icon = "󰒘", enabled = true },
    { name = "utilities", icon = "󰒘", enabled = true },
  },
  ["rightPanel.lock"] = false,
  ["leftPanel.lock"] = false,
  ["crypto.favorite"] = { symbol = "", timeframe = "" },
  ["weather.city"] = "",
  -- Target language of `translate <text>` in the launcher.
  ["translate.lang"] = "en",
}

function M.ensure()
  for key, value in pairs(M.defaults) do
    if hw.settings:get(key) == nil then
      hw.settings:set(key, value)
    end
  end
end

return M
