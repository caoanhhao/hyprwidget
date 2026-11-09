-- Fallback colors from cwal (needs the "fs" permission to read the file).
-- The "wallpaper" color source (hw.service.theme, Material You) wins over
-- these when it is running.
local path = HOME .. "/.cache/cwal/colors.scss"
local colors = hw.state({})

local function read()
  local f = io.open(path, "r")
  if not f then return end
  local t = {}
  for name, value in f:read("a"):gmatch("%$([%w_-]+):%s*(#%x+);") do
    t[name] = value
  end
  f:close()
  colors:set(t)
end

read()
hw.watch_file(path, read)
return { colors = colors }
