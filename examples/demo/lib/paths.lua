-- Paths into the rest of the desktop setup (theme scripts, wallpaper daemon…).
-- Every entry is optional: a feature whose path is unset or missing is
-- skipped (its button/menu item does nothing and says so).
--
-- Set your own in `paths.local.lua` next to hyprwidget.lua (git-ignored),
-- returning a table with any of the keys below, e.g.
--   return {
--     system_theme = HOME .. "/.config/hypr/theme/scripts/system-theme.sh",
--     wallpaper_daemon = HOME .. "/.config/hypr/wallpaper-daemon",
--   }
local M = {
  -- Script: `<script> get` prints light|dark, `<script> switch light|dark`.
  system_theme = nil,
  -- Flags file (`key=true|false`) read by the theme scripts.
  theme_conf = nil,
  -- Commands (run with bash -c): "System Sleep" (default: systemctl suspend),
  -- and the left panel's screenshot / screen-record shortcuts.
  suspend = nil,
  screenshot_area = nil,
  screenrecord = nil,
  -- Wallpaper daemon dir: set-wallpaper.sh, reload.sh, config/current.conf.
  wallpaper_daemon = nil,
  -- Wallpapers, one sub-directory per category; added files go to custom/.
  wallpapers = HOME .. "/Pictures/Wallpapers",
  -- Where the "lockscreen" target copies the chosen wallpaper.
  lockscreen_wallpaper = HOME .. "/Pictures/Wallpapers/lockscreen/wallpaper",
  -- SDDM theme.conf whose `background=` the "sddm" target rewrites (pkexec).
  sddm_theme_conf = nil,
  -- Hyprland keybinds shown by the Keybinds panel (hl.bind(...) Lua config).
  keybinds = HOME .. "/.config/hypr/hyprland.lua",
  -- cwal output used by the "cwal" palette.
  cwal_colors = HOME .. "/.cache/cwal/colors.scss",
  -- Rounded images for fastfetch ("pin to terminal" in the booru viewer).
  fastfetch_cache = HOME .. "/.config/fastfetch/cache",
}

local local_file = CONFIG_DIR .. "/paths.local.lua"
local chunk = loadfile(local_file)
if chunk then
  local ok, t = pcall(chunk)
  if ok and type(t) == "table" then
    for k, v in pairs(t) do
      M[k] = v
    end
  else
    print("paths.local.lua: " .. tostring(ok and "must return a table" or t))
  end
end

--- `M[key]` when it is set and exists on disk, else nil.
function M.get(key)
  local p = M[key]
  if type(p) ~= "string" or p == "" then
    return nil
  end
  local f = io.open(p, "r")
  if not f then
    return nil
  end
  f:close()
  return p
end

return M
