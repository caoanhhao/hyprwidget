-- Formatting helpers (plugin-local module: require("lib.format")).
local M = {}

function M.pct(p)
  return string.format("%d%%", math.floor((p or 0) + 0.5))
end

function M.gib(bytes)
  return string.format("%.1fG", (bytes or 0) / 1073741824)
end

function M.temp(t)
  return t and string.format("%d°", math.floor(t + 0.5)) or ""
end

return M
