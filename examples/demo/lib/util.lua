-- Small helpers shared by the AGS port (utils/*.ts in ~/.config/ags).
local M = {}

-- AGS `globalTransition` (ms): revealer animations, delayed removals.
M.TRANSITION = 300

M.CACHE_DIR = (os.getenv("XDG_CACHE_HOME") or (HOME .. "/.cache")) .. "/hyprwidget"

--- Reads a JSON file; `fallback` when missing or invalid.
function M.read_json(path, fallback)
  local f = io.open(path, "r")
  if not f then
    return fallback
  end
  local text = f:read("a")
  f:close()
  local ok, v = pcall(hw.json.decode, text)
  if not ok or v == nil then
    return fallback
  end
  return v
end

--- Writes `value` as pretty JSON, creating the parent directory.
function M.write_json(path, value)
  local dir = path:match("^(.*)/[^/]*$")
  if dir then
    os.execute("mkdir -p '" .. dir:gsub("'", "'\\''") .. "'")
  end
  local f, err = io.open(path, "w")
  if not f then
    print("write_json " .. path .. ": " .. tostring(err))
    return
  end
  f:write(hw.json.encode(value, true))
  f:close()
end

--- notify-send, like AGS utils/notification.ts `notify`.
function M.notify(summary, body)
  hw.exec_async({ "notify-send", tostring(summary or ""), tostring(body or "") }, function(_, err)
    if err then
      print("notify-send: " .. err)
    end
  end)
end

--- Runs a command, ignoring the output; errors become a notification
--- titled `title` (or are just logged).
function M.run(cmd, title)
  hw.exec_async(cmd, function(_, err)
    if err then
      if title then
        M.notify(title, err)
      else
        print(tostring(err))
      end
    end
  end)
end

function M.copy(text)
  M.run({ "wl-copy", "--", tostring(text) }, "Clipboard")
end

--- Truncates to `max` characters (UTF-8 aware), ending with "…".
function M.truncate(s, max)
  s = tostring(s or "")
  if (utf8.len(s) or #s) <= max then
    return s
  end
  local cut = utf8.offset(s, max)
  return s:sub(1, (cut or (max + 1)) - 1) .. "…"
end

function M.trim(s)
  return (tostring(s or ""):gsub("^%s+", ""):gsub("%s+$", ""))
end

function M.file_exists(path)
  local f = io.open(path, "r")
  if f then
    f:close()
    return true
  end
  return false
end

--- Case-insensitive plain substring test.
function M.contains(haystack, needle)
  return tostring(haystack or ""):lower():find(tostring(needle or ""):lower(), 1, true) ~= nil
end

--- hw.For with props for its container box (class, vertical, spacing…).
function M.For(signal, fn, key, props)
  local node = hw.For(signal, fn, key)
  for k, v in pairs(props or {}) do
    node[k] = v
  end
  return node
end

return M
