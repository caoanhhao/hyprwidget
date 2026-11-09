-- Launcher result providers: port of ~/.config/ags/widgets/applauncher/
-- utilities/*.tsx and utils/{arithmetic,convert,url}.ts.
--
-- A result is a table:
--   name, description?, arg?, type? ("app" shows `icon` as an icon),
--   icon?, actions? = {{label, class?, tooltip?, on_click}},
--   close_on_launch? (default true), launch = function(ctx)
-- ctx = { monitor = "DP-2", prefill = function(text) }.
local util = require("lib.util")

local M = {}

M.MAX_ITEMS = 10
-- Written by ~/.config/hypr/scripts/clipboard-monitor.sh.
M.CLIPBOARD_HISTORY = util.CACHE_DIR .. "/launcher/clipboard-history.json"
M.NOTES = util.CACHE_DIR .. "/launcher/notes.json"
M.EMOJIS = CONFIG_DIR .. "/assets/emojis.json"
M.SCRIPTS = CONFIG_DIR .. "/scripts"

local NOTE_ICON = "\u{F039A}"

--- `prefix` + space at the start (case-insensitive) → the rest, else nil.
local function strip_prefix(text, prefix)
  local t = text:gsub("^%s+", "")
  if t:sub(1, #prefix + 1):lower() ~= prefix .. " " then
    return nil
  end
  return t:sub(#prefix + 2)
end

----------------------------------------------------------------------------
-- Clipboard: `cb <filter>`

local function strip_html(s)
  return util.trim((s:gsub("<[^>]*>", " "):gsub("%s+", " ")))
end

function M.parse_clipboard(text)
  local rest = strip_prefix(text, "cb")
  return rest and util.trim(rest)
end

-- Only this much of an entry is used for previews and search: the history
-- can hold huge texts, and Lua patterns get slow on them.
local CLIP_SCAN = 2000

local function normalize_clipboard(entries, max)
  if type(entries) ~= "table" then
    return {}
  end
  local out = {}
  local count = #entries
  for i, e in ipairs(entries) do
    local content = type(e.content) == "string" and e.content or ""
    local head = util.trim(content:sub(1, CLIP_SCAN))
    if head ~= "" then
      local kind = (type(e.type) == "string" and util.trim(e.type) ~= "") and util.trim(e.type):lower() or "text"
      local mime = (type(e.mimeType) == "string" and util.trim(e.mimeType) ~= "") and util.trim(e.mimeType):lower()
        or "text/plain"
      local fallback = math.max(0, count - (i - 1))
      local is_image = kind == "image" or mime:sub(1, 6) == "image/"
      local entry = {
        timestamp = tonumber(e.timestamp) or fallback,
        type = kind,
        content = is_image and util.trim(content) or content,
        mime = mime,
        is_image = is_image,
      }
      if is_image then
        local path = entry.content
        local basename = path:sub((path:find("[^/]*$")))
        entry.preview = util.truncate("🖼 " .. basename, 128)
        entry.search = (path .. " " .. basename .. " " .. mime .. " " .. kind .. " image"):lower()
      else
        local text = mime:find("html", 1, true) and strip_html(head) or head
        entry.preview = util.truncate(text ~= "" and text or "(empty)", 128)
        entry.search = (text .. " " .. head .. " " .. mime .. " " .. kind .. " text html"):lower()
      end
      table.insert(out, entry)
    end
  end
  table.sort(out, function(a, b)
    return a.timestamp > b.timestamp
  end)
  while #out > max do
    table.remove(out)
  end
  return out
end

-- Parsed history, reused while the file size is unchanged.
local clip_cache = { size = -1, entries = {} }

local function clipboard_entries()
  local f = io.open(M.CLIPBOARD_HISTORY, "r")
  if not f then
    return {}
  end
  local size = f:seek("end")
  if size ~= clip_cache.size then
    f:seek("set")
    local ok, v = pcall(hw.json.decode, f:read("a"))
    clip_cache = { size = size, entries = normalize_clipboard(ok and v or {}, 128) }
  end
  f:close()
  return clip_cache.entries
end

local function copy_clipboard_entry(e)
  if e.is_image then
    hw.exec_async({
      "sh",
      "-c",
      'if [ -f "$1" ]; then wl-copy --type "$2" < "$1"; else echo "Missing image file" >&2; exit 1; fi',
      "sh",
      e.content,
      e.mime,
    }, function(_, err)
      util.notify("Clipboard", err or ("Copied image (" .. e.mime .. ") to clipboard"))
    end)
    return
  end
  util.run({ "wl-copy", "--type", e.mime, "--", e.content }, "Clipboard")
end

function M.clipboard_results(query)
  local entries = clipboard_entries()
  local term = query:lower()
  local out = {}
  for _, e in ipairs(entries) do
    if term == "" or e.search:find(term, 1, true) then
      table.insert(out, {
        name = e.preview,
        description = e.mime .. " • " .. e.type,
        type = "clipboard",
        launch = function()
          copy_clipboard_entry(e)
        end,
      })
    end
  end
  return out
end

----------------------------------------------------------------------------
-- Emojis: `emoji <tags>`

local emojis

function M.parse_emoji(text)
  local rest = strip_prefix(text, "emoji")
  return rest and util.trim(rest):lower()
end

function M.emoji_results(query)
  emojis = emojis or util.read_json(M.EMOJIS, {})
  local out = {}
  for _, e in ipairs(emojis) do
    if (e.app_tags or ""):lower():find(query, 1, true) then
      table.insert(out, {
        name = e.app_name,
        icon = e.app_name,
        type = "emoji",
        launch = function()
          util.copy(e.app_name)
        end,
      })
    end
  end
  return out
end

----------------------------------------------------------------------------
-- Translate: `translate <text> > <lang>` (default: setting translate.lang),
-- via scripts/translate.sh

local function default_lang()
  local l = hw.settings:get("translate.lang")
  return type(l) == "string" and l ~= "" and l or "en"
end

function M.parse_translate(text)
  local rest = strip_prefix(text, "translate")
  if not rest then
    return nil
  end
  rest = util.trim(rest)
  if rest == "" then
    return nil
  end
  local source, target = rest:match("^(.-)>(.*)$")
  if not source then
    return { text = rest, lang = default_lang() }
  end
  target = util.trim(target):match("^[^>]*")
  return { text = util.trim(source), lang = target ~= "" and util.trim(target) or default_lang() }
end

--- Yields (call from a coroutine).
function M.translate_results(q)
  local translation = hw.exec_async({ "bash", M.SCRIPTS .. "/translate.sh", q.text, q.lang })
  return {
    {
      name = translation,
      launch = function()
        util.copy(translation)
      end,
    },
  }
end

----------------------------------------------------------------------------
-- Notes: `note [list|add|edit N text|rm N|text]`

local function read_notes()
  local raw = util.read_json(M.NOTES, {})
  local notes = {}
  if type(raw) ~= "table" then
    return notes
  end
  local now = os.time() * 1000
  for i, n in ipairs(raw) do
    local content = type(n.content) == "string" and util.trim(n.content) or ""
    if content ~= "" then
      local created = tonumber(n.createdAt) or (now - i)
      table.insert(notes, {
        content = content,
        createdAt = created,
        updatedAt = tonumber(n.updatedAt) or created,
      })
    end
  end
  table.sort(notes, function(a, b)
    return a.createdAt < b.createdAt
  end)
  for i, n in ipairs(notes) do
    n.id = i
  end
  return notes
end

local function write_notes(notes)
  util.write_json(M.NOTES, notes)
end

function M.parse_note(text)
  local rest = strip_prefix(text, "note")
  if not rest then
    return nil
  end
  local raw = util.trim(rest)
  if raw == "" or raw:lower() == "list" then
    return { action = "list" }
  end
  local word, args = raw:match("^(%S+)%s*(.*)$")
  word = word:lower()
  if word == "edit" or word == "set" or word == "update" or word == "modify" then
    local idx, content = args:match("^(%d+)$")
    if not idx then
      idx, content = args:match("^(%d+)%s+(.+)$")
    end
    if idx then
      return { action = "update", index = tonumber(idx), content = util.trim(content or "") }
    end
  elseif word == "del" or word == "rm" or word == "remove" or word == "delete" then
    local idx = args:match("^(%d+)$")
    if idx then
      return { action = "remove", index = tonumber(idx) }
    end
  elseif (word == "add" or word == "new") and args ~= "" then
    return { action = "add", content = util.trim(args) }
  end
  return { action = "add", content = raw }
end

local function now_ms()
  return os.time() * 1000
end

local function note_list(notes, max)
  if #notes == 0 then
    return {
      { name = "No notes yet", description = "Type: note buy milk", icon = NOTE_ICON, launch = function() end },
    }
  end
  local out = {}
  for i = 1, math.min(max, #notes) do
    local note = notes[i]
    table.insert(out, {
      name = note.id .. ". " .. util.truncate(note.content, 80),
      description = "Updated " .. os.date("%c", math.floor(note.updatedAt / 1000)),
      icon = NOTE_ICON,
      type = "note",
      actions = {
        {
          label = "\u{F044}",
          class = "note-action-btn-edit",
          tooltip = "Edit note #" .. note.id,
          on_click = function(ctx)
            ctx.prefill("note edit " .. note.id .. " " .. note.content)
          end,
        },
        {
          label = "\u{EE23}",
          class = "note-action-btn-delete",
          tooltip = "Delete note #" .. note.id,
          on_click = function(ctx)
            local next_notes = read_notes()
            if not next_notes[note.id] then
              util.notify("Notes", "Note not found")
              return
            end
            table.remove(next_notes, note.id)
            write_notes(next_notes)
            util.notify("Notes", "Note removed")
            ctx.prefill("note ")
          end,
        },
      },
      launch = function()
        util.copy(note.content)
      end,
    })
  end
  return out
end

local function concat(first, rest)
  for _, r in ipairs(rest) do
    table.insert(first, r)
  end
  return first
end

function M.note_results(cmd, max)
  local notes = read_notes()
  if cmd.action == "list" then
    return note_list(notes, max)
  end

  if cmd.action == "add" then
    if cmd.content == "" then
      return {
        { name = "Note content is empty", description = "Type: note buy milk", icon = NOTE_ICON, launch = function() end },
      }
    end
    return concat({
      {
        name = "Add note: " .. util.truncate(cmd.content, 80),
        description = "Press Enter to save",
        icon = "\u{F0C7}",
        close_on_launch = false,
        launch = function(ctx)
          local next_notes = read_notes()
          local t = now_ms()
          table.insert(next_notes, { id = #next_notes + 1, content = cmd.content, createdAt = t, updatedAt = t })
          write_notes(next_notes)
          util.notify("Notes", "Note added")
          ctx.prefill("note ")
        end,
      },
    }, note_list(notes, math.max(0, max - 1)))
  end

  local target = notes[cmd.index]
  if not target then
    return concat({
      { name = "Note #" .. cmd.index .. " not found", description = "Use: note list", icon = NOTE_ICON, launch = function() end },
    }, note_list(notes, math.max(0, max - 1)))
  end

  if cmd.action == "remove" then
    return concat({
      {
        name = "Remove note #" .. cmd.index,
        description = util.truncate(target.content, 80),
        icon = "\u{F0A7A}",
        close_on_launch = false,
        launch = function(ctx)
          local next_notes = read_notes()
          if not next_notes[cmd.index] then
            util.notify("Notes", "Note not found")
            return
          end
          table.remove(next_notes, cmd.index)
          write_notes(next_notes)
          util.notify("Notes", "Note removed")
          ctx.prefill("note ")
        end,
      },
    }, note_list(notes, math.max(0, max - 1)))
  end

  if cmd.content == "" then
    return concat({
      {
        name = "Updated note content is empty",
        description = "Use: note edit " .. cmd.index .. " your text",
        icon = NOTE_ICON,
        launch = function() end,
      },
    }, note_list(notes, math.max(0, max - 1)))
  end

  return concat({
    {
      name = "Update note #" .. cmd.index,
      description = util.truncate(cmd.content, 80),
      icon = "\u{F06FF}",
      close_on_launch = false,
      launch = function(ctx)
        local next_notes = read_notes()
        local n = next_notes[cmd.index]
        if not n then
          util.notify("Notes", "Note not found")
          return
        end
        n.content = cmd.content
        n.updatedAt = now_ms()
        write_notes(next_notes)
        util.notify("Notes", "Note updated")
        ctx.prefill("note ")
      end,
    },
    {
      name = "Current: " .. util.truncate(target.content, 80),
      description = "Existing note",
      icon = NOTE_ICON,
      launch = function() end,
    },
  }, note_list(notes, math.max(0, max - 2)))
end

----------------------------------------------------------------------------
-- Arithmetic: `2*(3+4)`

function M.is_arithmetic(text)
  local first = util.trim(text):match("^(%S*)") or ""
  return first:find("[%+%-%*/%%%^]") ~= nil or first:find("×", 1, true) ~= nil or first:find("÷", 1, true) ~= nil
end

local function format_number(n)
  if math.type(n) == "integer" then
    return tostring(n)
  end
  if n == math.floor(n) and math.abs(n) < 1e15 then
    return string.format("%d", n)
  end
  return (string.format("%.12g", n))
end

--- Result string, or nil when `text` isn't a valid expression.
function M.arithmetic(text)
  local expr = text:gsub("×", "*"):gsub("÷", "/"):gsub("%*%*", "^")
  if not expr:match("^[%d%s%.%+%-%*/%%%^%(%)]+$") then
    return nil
  end
  local f = load("return (" .. expr .. ")", "=calc", "t", {})
  if not f then
    return nil
  end
  local ok, v = pcall(f)
  if not ok or type(v) ~= "number" or v ~= v or v == math.huge or v == -math.huge then
    return nil
  end
  return format_number(v)
end

function M.arithmetic_results(text)
  local out = M.arithmetic(text)
  if not out then
    return nil
  end
  return {
    {
      name = out,
      launch = function()
        util.copy(out)
      end,
    },
  }
end

----------------------------------------------------------------------------
-- Unit conversion: `100c to f`, `10 kg in lb`, `5km`

-- Linear units: factor to the type's base unit.
local UNITS = {
  kg = { "weight", 1 },
  g = { "weight", 0.001 },
  lb = { "weight", 0.453592 },
  oz = { "weight", 0.0283495 },
  ton = { "weight", 1000 },
  m = { "length", 1 },
  km = { "length", 1000 },
  cm = { "length", 0.01 },
  mm = { "length", 0.001 },
  mi = { "length", 1609.34 },
  ft = { "length", 0.3048 },
  ["in"] = { "length", 0.0254 },
  yd = { "length", 0.9144 },
  l = { "volume", 1 },
  ml = { "volume", 0.001 },
  gal = { "volume", 3.78541 },
  qt = { "volume", 0.946353 },
  pt = { "volume", 0.473176 },
  cup = { "volume", 0.236588 },
  tbsp = { "volume", 0.0147868 },
  tsp = { "volume", 0.00492892 },
  ["fl oz"] = { "volume", 0.0295735 },
  b = { "digital", 1 },
  kb = { "digital", 1024 },
  mb = { "digital", 1024 ^ 2 },
  gb = { "digital", 1024 ^ 3 },
  tb = { "digital", 1024 ^ 4 },
  pb = { "digital", 1024 ^ 5 },
  ["m/s"] = { "speed", 1 },
  ["km/h"] = { "speed", 1 / 3.6 },
  mph = { "speed", 0.44704 },
  knot = { "speed", 0.514444 },
  celsius = { "temperature" },
  fahrenheit = { "temperature" },
  kelvin = { "temperature" },
}

local COMMON = {
  temperature = { "celsius", "fahrenheit", "kelvin" },
  weight = { "kg", "g", "lb", "oz" },
  length = { "m", "cm", "mm", "km", "mi", "ft", "in" },
  volume = { "l", "ml", "gal", "cup" },
  digital = { "b", "kb", "mb", "gb", "tb" },
  speed = { "km/h", "mph", "m/s", "knot" },
}

local ALIASES = {
  c = "celsius", ["°c"] = "celsius", celcius = "celsius",
  f = "fahrenheit", ["°f"] = "fahrenheit", fahr = "fahrenheit",
  k = "kelvin", ["°k"] = "kelvin",
  kilogram = "kg", kilograms = "kg", kilo = "kg", kilos = "kg",
  gram = "g", grams = "g", pound = "lb", pounds = "lb", lbs = "lb",
  ounce = "oz", ounces = "oz", tonne = "ton", tonnes = "ton", tons = "ton",
  meter = "m", meters = "m", metre = "m", metres = "m",
  centimeter = "cm", centimeters = "cm", centimetre = "cm", centimetres = "cm",
  millimeter = "mm", millimeters = "mm", millimetre = "mm", millimetres = "mm",
  kilometer = "km", kilometers = "km", kilometre = "km", kilometres = "km",
  mile = "mi", miles = "mi", foot = "ft", feet = "ft", inch = "in", inches = "in",
  yard = "yd", yards = "yd",
  liter = "l", liters = "l", litre = "l", litres = "l",
  milliliter = "ml", milliliters = "ml", millilitre = "ml", millilitres = "ml",
  gallon = "gal", gallons = "gal", quart = "qt", quarts = "qt", pint = "pt", pints = "pt",
  cups = "cup", tablespoon = "tbsp", tablespoons = "tbsp", teaspoon = "tsp", teaspoons = "tsp",
  ["fluid ounce"] = "fl oz", ["fluid ounces"] = "fl oz",
  byte = "b", bytes = "b", kilobyte = "kb", kilobytes = "kb", kib = "kb",
  megabyte = "mb", megabytes = "mb", mib = "mb", gigabyte = "gb", gigabytes = "gb", gib = "gb",
  terabyte = "tb", terabytes = "tb", tib = "tb", petabyte = "pb", petabytes = "pb", pib = "pb",
  kph = "km/h", kmph = "km/h", ["km per hour"] = "km/h", mps = "m/s", ["meters per second"] = "m/s",
  ["miles per hour"] = "mph", knots = "knot", kts = "knot",
}

local function unit(s)
  s = util.trim(s):lower()
  return ALIASES[s] or s
end

local function to_celsius(v, u)
  if u == "fahrenheit" then
    return (v - 32) * 5 / 9
  elseif u == "kelvin" then
    return v - 273.15
  end
  return v
end

local function from_celsius(c, u)
  if u == "fahrenheit" then
    return c * 9 / 5 + 32
  elseif u == "kelvin" then
    return c + 273.15
  end
  return c
end

local function convert_value(v, from, to)
  if UNITS[from][1] == "temperature" then
    return from_celsius(to_celsius(v, from), to)
  end
  return v * UNITS[from][2] / UNITS[to][2]
end

-- utils/convert.ts formatNumber
local function format_conv(n)
  local a = math.abs(n)
  if a ~= 0 and (a < 0.01 or a >= 1e6) then
    return string.format("%.4e", n)
  end
  local digits = a < 1 and 4 or a < 10 and 3 or a < 1000 and 2 or 1
  local s = string.format("%." .. digits .. "f", n)
  return (s:gsub("%.?0+$", ""))
end

local UNIT_CHARS = "[%a°/%%]+"

--- { value, from, to? } or nil.
local function parse_conversion(text)
  local t = util.trim(text):lower():gsub("^convert%s+", "")
  local num, rest = t:match("^(%d+%.?%d*)%s*(.+)$")
  if not num then
    return nil
  end
  local from, to
  for _, sep in ipairs({ " to ", " in ", " as ", " => " }) do
    local a, b = rest:find(sep, 1, true)
    if a then
      from, to = rest:sub(1, a - 1), rest:sub(b + 1)
      break
    end
  end
  from = from or rest
  if not from:match("^" .. UNIT_CHARS .. "%s?%a*$") or (to and not to:match("^" .. UNIT_CHARS .. "%s?%a*$")) then
    return nil
  end
  return { value = tonumber(num), from = unit(from), to = to and unit(to) }
end

function M.is_conversion(text)
  return parse_conversion(text) ~= nil
end

--- Results, or nil when the units are unknown.
function M.conversion_results(text)
  local q = parse_conversion(text)
  if not q or not UNITS[q.from] then
    return nil
  end
  local kind = UNITS[q.from][1]
  local targets = {}
  if q.to then
    if not UNITS[q.to] or UNITS[q.to][1] ~= kind then
      return nil
    end
    targets = { q.to }
  else
    for _, u in ipairs(COMMON[kind]) do
      if u ~= q.from then
        table.insert(targets, u)
      end
    end
  end
  local out = {}
  for _, to in ipairs(targets) do
    local formatted = format_conv(convert_value(q.value, q.from, to)) .. " " .. to
    table.insert(out, {
      name = formatted,
      icon = "\u{F07DB}",
      description = "Converted from " .. format_number(q.value) .. " " .. q.from,
      launch = function()
        util.copy(formatted)
      end,
    })
  end
  return out
end

----------------------------------------------------------------------------
-- URLs: `github.com/x`, `https://…`

local TLDS = { "com", "net", "org", "io", "gov", "edu", "co", "info", "biz", "me" }

function M.is_url(text)
  local first = util.trim(text):match("^(%S*)") or ""
  if first:match("^https?://") then
    return true
  end
  for _, tld in ipairs(TLDS) do
    if first:match("%." .. tld .. "$") or first:match("%." .. tld .. "/.*$") then
      return true
    end
  end
  return false
end

local function format_url(s)
  s = util.trim(s):lower()
  if not s:match("^https?://") then
    s = "https://" .. s
  end
  return s
end

local function domain_label(url)
  local rest = util.trim(url):gsub("^https?://", ""):gsub("^www%.", "")
  local domain, path = rest:match("^([^/]+)(.*)$")
  if not domain then
    return ""
  end
  domain = domain:gsub("%.[^.]*$", "")
  domain = domain:sub(1, 1):upper() .. domain:sub(2)
  local parts = {}
  for seg in path:gmatch("[^/]+") do
    table.insert(parts, seg:lower())
  end
  if #parts > 0 then
    return domain .. " - " .. table.concat(parts, " - ")
  end
  return domain
end

function M.url_results(text)
  return {
    {
      name = domain_label(text),
      launch = function()
        hw.exec_async({ "xdg-open", format_url(text) }, function(_, err)
          if err then
            util.notify("URL", err)
            return
          end
          hw.exec_async({ "xdg-settings", "get", "default-web-browser" }, function(browser)
            util.notify("URL", "Opening " .. text .. " in " .. (browser or "browser"):gsub("%.desktop$", ""))
          end)
        end)
      end,
    },
  }
end

return M
