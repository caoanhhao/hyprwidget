-- hyprwidget Lua config entrypoint (same layout as ~/.config/hypr/hyprland.lua).
-- `require_all` loads config/<name>.lua in order; `require_custom_dir` then
-- loads every config/custom/*.lua (per-option overrides written by the UI).
require_all({
  "general",
  "decoration",
  "theme",
  "bar",
  "services",
  "notifications",
  "launcher",
  "sysmon",
  "embed",
})

require_custom_dir()
