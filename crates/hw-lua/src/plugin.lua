-- Plugin sandbox and loader (PLAN.md §3.5). Receives the real `hw` table
-- and private helpers from Rust (`P`); returns `make_env(spec)` and
-- `load_plugin(spec)`.
--
-- Every plugin runs in its own `_ENV`: safe base functions, copies of the
-- standard libraries, and a `hw` proxy that only exposes what the
-- permissions of plugin.json allow (`exec`, `network`, `fs`).
local hw, P = ...

local real_load = load

-- hw.* functions that need a permission.
local PERM_OF = {
  exec = "exec",
  exec_async = "exec",
  subprocess = "exec",
  watch_file = "fs",
}
-- hw.* functions plugins never get (global config, quitting, raw exec).
local NEVER = {
  quit = true,
  config = true,
  layer_rule = true,
  define = true,
  require_all = true,
  require_custom_dir = true,
  _exec = true,
}

local BASE = {
  "assert", "error", "ipairs", "next", "pairs", "pcall", "rawequal", "rawget",
  "rawlen", "rawset", "select", "setmetatable", "tonumber", "tostring", "type",
  "xpcall", "_VERSION",
}

local function copy(t)
  local c = {}
  for k, v in pairs(t) do c[k] = v end
  return c
end

-- Runs `cmd` through the private exec; yields like hw.exec_async.
local function exec_yield(argv, cb, what)
  if cb then return hw._exec(argv, cb) end
  local co, main = coroutine.running()
  if main or not coroutine.isyieldable() then
    error(what .. ": pass a callback, or call it from a handler / hw.async", 3)
  end
  hw._exec(argv, function(out, err)
    local ok, e = coroutine.resume(co, out, err)
    if not ok then hw._error(e) end
  end)
  local out, err = coroutine.yield()
  if err then error(err, 3) end
  return out
end

local function make_hw(spec)
  local perms = spec.permissions
  local own = {
    plugin = {
      name = spec.name,
      version = spec.version,
      dir = spec.dir,
      permissions = copy(perms),
    },
  }
  if not perms.exec then
    -- hw.poll with a function is fine; with a command it runs a process.
    own.poll = function(ms, cmd, transform)
      if type(cmd) ~= "function" then
        error(("hw.poll with a command needs the 'exec' permission (%s)"):format(spec.name), 2)
      end
      return hw.poll(ms, cmd, transform)
    end
  end
  if perms.network then
    --- hw.fetch(url [, cb(body, err)]) -> body: HTTP(S) GET through curl.
    own.fetch = function(url, cb)
      if type(url) ~= "string" or not url:match("^https?://") then
        error("hw.fetch: only http(s) URLs", 2)
      end
      return exec_yield({ "curl", "-fsSL", "--max-time", "20", "--", url }, cb, "hw.fetch")
    end
  end
  return setmetatable(own, {
    __index = function(_, k)
      if NEVER[k] then
        error(("hw.%s is not available to plugins (%s)"):format(k, spec.name), 2)
      end
      local perm = PERM_OF[k]
      if perm and not perms[perm] then
        error(("hw.%s needs the '%s' permission (%s)"):format(k, perm, spec.name), 2)
      end
      if k == "fetch" then
        error(("hw.fetch needs the 'network' permission (%s)"):format(spec.name), 2)
      end
      return hw[k]
    end,
  })
end

--- The sandboxed `_ENV` of a plugin (or theme init.lua).
local function make_env(spec)
  local perms = spec.permissions
  local env = {}
  for _, k in ipairs(BASE) do env[k] = _G[k] end
  -- The string metatable is shared by every Lua env: don't hand it out.
  env.getmetatable = function(v)
    if type(v) == "string" then return nil end
    return getmetatable(v)
  end
  env.string = copy(string)
  env.table = copy(table)
  env.math = copy(math)
  env.utf8 = copy(utf8)
  env.coroutine = copy(coroutine)
  env.os = {
    time = os.time,
    date = os.date,
    clock = os.clock,
    difftime = os.difftime,
    getenv = os.getenv,
  }
  if perms.fs then
    env.io = copy(io)
    env.io.popen = nil
    env.os.remove = os.remove
    env.os.rename = os.rename
    env.os.tmpname = os.tmpname
  end
  if perms.exec then
    env.io = env.io or {}
    env.io.popen = io.popen
    env.os.execute = os.execute
  end
  local tag = "[" .. spec.name .. "]"
  env.print = function(...) print(tag, ...) end
  env.HOME = HOME
  env.CONFIG_DIR = CONFIG_DIR
  env.PLUGIN_DIR = spec.dir
  env.load = function(chunk, name, _, e)
    return real_load(chunk, name, "t", e or env)
  end
  env.hw = make_hw(spec)
  env.W = hw.W
  env._G = env

  -- require(): modules from the plugin's own directory only.
  local loaded = { hw = env.hw }
  env.require = function(mod)
    if loaded[mod] ~= nil then return loaded[mod] end
    if type(mod) ~= "string" or not mod:match("^[%w_%-]+[%w_%-%.]*$") or mod:find("..", 1, true) then
      error("require: invalid module name '" .. tostring(mod) .. "'", 2)
    end
    local base = spec.dir .. "/" .. mod:gsub("%.", "/")
    local fn, err = P.load_file(base .. ".lua", env)
    if not fn and err == "not found" then
      fn, err = P.load_file(base .. "/init.lua", env)
    end
    if not fn then
      if err == "not found" then err = "module '" .. mod .. "' not found in " .. spec.dir end
      error(err, 2)
    end
    local r = fn(mod)
    if r == nil then r = true end
    loaded[mod] = r
    return r
  end
  return env
end

-- name -> { widgets = { Name = fn }, error = msg|nil }
local registry = {}
-- widget name -> plugin name
local widget_owner = {}

local function first_line(s)
  return (tostring(s):match("^[^\n]*"))
end

local function error_widget(plugin, msg)
  return {
    type = "Label",
    label = "⚠ " .. plugin,
    tooltip = first_line(msg),
    class = "plugin-error",
  }
end

local function wrapper(wname)
  return function(props)
    local owner = widget_owner[wname]
    local entry = registry[owner]
    if not entry or entry.error or not entry.widgets[wname] then
      return error_widget(owner or wname, entry and entry.error or ("W." .. wname .. " is not loaded"))
    end
    local ok, w = xpcall(entry.widgets[wname], hw._traceback, props)
    if not ok then
      P.report(owner, "W." .. wname .. ": " .. tostring(w))
      return error_widget(owner, w)
    end
    return w
  end
end

local function claim_widget(plugin, wname)
  if type(wname) ~= "string" or not wname:match("^[%a_][%w_]*$") then
    return ("invalid widget name '%s'"):format(tostring(wname))
  end
  if hw._is_kind(wname) then
    return ("widget '%s' shadows a built-in widget"):format(wname)
  end
  local owner = widget_owner[wname]
  if owner and owner ~= plugin then
    return ("widget '%s' is already provided by plugin '%s'"):format(wname, owner)
  end
  if not owner and hw._defined(wname) then
    return ("widget '%s' is already defined"):format(wname)
  end
  widget_owner[wname] = plugin
  P.define(wname, wrapper(wname))
end

local KEYS = { widgets = true, services = true, requests = true, setup = true }

local function load_plugin(spec)
  local entry = registry[spec.name] or { widgets = {} }
  registry[spec.name] = entry
  local function fail(msg)
    entry.error = msg
    return false, msg
  end
  -- Widgets declared in plugin.json get placeholders even if init.lua fails.
  for _, wname in ipairs(spec.widgets or {}) do
    local e = claim_widget(spec.name, wname)
    if e then return fail(e) end
  end

  local env = make_env(spec)
  local chunk, err = P.load_file(spec.entry, env)
  if not chunk then return fail(err) end
  local ok, ret = xpcall(chunk, hw._traceback)
  if not ok then return fail(ret) end
  if ret == nil then ret = {} end
  if type(ret) ~= "table" then
    return fail(("%s must return a table, got %s"):format(spec.entry_name, type(ret)))
  end
  for k in pairs(ret) do
    if not KEYS[k] then
      return fail(("unknown key '%s' in the returned table (widgets, services, requests, setup)"):format(tostring(k)))
    end
  end
  for key, want in pairs({ widgets = "table", services = "table", requests = "table", setup = "function" }) do
    if ret[key] ~= nil and type(ret[key]) ~= want then
      return fail(("'%s' must be a %s, got %s"):format(key, want, type(ret[key])))
    end
  end

  local widgets = {}
  for wname, fn in pairs(ret.widgets or {}) do
    if type(fn) ~= "function" then
      return fail(("widgets.%s must be a function"):format(tostring(wname)))
    end
    local e = claim_widget(spec.name, wname)
    if e then return fail(e) end
    widgets[wname] = fn
  end
  for sname, v in pairs(ret.services or {}) do
    local e = P.register_service(spec.name, sname, v)
    if e then return fail(e) end
  end
  for rname, fn in pairs(ret.requests or {}) do
    if type(rname) ~= "string" or type(fn) ~= "function" then
      return fail("requests must map names to functions")
    end
    hw.on_request(rname, fn)
  end
  entry.widgets = widgets
  entry.error = nil
  entry.env = env

  if ret.setup then
    -- In a coroutine so setup may use hw.exec_async; an error after a
    -- yield is only logged.
    local suspended = false
    local co = coroutine.create(function(info)
      local sok, e = xpcall(ret.setup, hw._traceback, info)
      if not sok and suspended then P.report(spec.name, "setup: " .. tostring(e)) end
      return sok, e
    end)
    local rok, sok, e = coroutine.resume(co, env.hw.plugin)
    suspended = coroutine.status(co) == "suspended"
    if not rok then return fail("setup: " .. tostring(sok)) end
    if coroutine.status(co) == "dead" and not sok then return fail("setup: " .. tostring(e)) end
  end
  return true
end

return make_env, load_plugin
