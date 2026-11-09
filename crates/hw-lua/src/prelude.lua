-- Lua half of the `hw` API. Receives the `hw` table, returns the internal
-- `spawn` and `request` dispatchers used by Rust.
local hw = ...

local traceback = hw._traceback

-- Runs fn(...) in a coroutine so it may call hw.exec_async. Returns fn's
-- first result if it finished without yielding.
local function spawn(fn, ...)
  local co = coroutine.create(function(...)
    local ok, r = xpcall(fn, traceback, ...)
    if not ok then hw._error(r) end
    return r
  end)
  local ok, r = coroutine.resume(co, ...)
  if not ok then hw._error(r) end
  if coroutine.status(co) == "dead" then return r end
  return nil
end

-- Request handlers: reply(ok, result) once fn finishes, even after yields.
local function request(fn, args, reply)
  local co = coroutine.create(function()
    local ok, r = xpcall(fn, traceback, args)
    reply(ok, r)
  end)
  local ok, err = coroutine.resume(co)
  if not ok then reply(false, err) end
end

--- Runs fn in a coroutine (for hw.exec_async outside handlers).
function hw.async(fn, ...)
  return spawn(fn, ...)
end

--- hw.exec_async(cmd) -> stdout (yields; raises on failure), or
--- hw.exec_async(cmd, function(out, err) end).
function hw.exec_async(cmd, cb)
  if cb then return hw._exec(cmd, cb) end
  local co, main = coroutine.running()
  if main or not coroutine.isyieldable() then
    error("hw.exec_async: pass a callback, or call it from a handler / hw.async", 2)
  end
  hw._exec(cmd, function(out, err)
    local ok, e = coroutine.resume(co, out, err)
    if not ok then hw._error(e) end
  end)
  local out, err = coroutine.yield()
  if err then error(err, 2) end
  return out
end

--- hw.poll(ms, cmd_or_fn [, transform(out, prev)]) -> Signal
function hw.poll(ms, cmd, transform)
  if type(cmd) == "function" then return hw._poll_fn(ms, cmd) end
  local s = hw.state(nil)
  hw.interval(ms, function()
    hw._exec(cmd, function(out, err)
      if err then
        hw._error("hw.poll " .. tostring(cmd) .. ": " .. err)
        return
      end
      if transform then out = transform(out, s:peek()) end
      s:set(out)
    end)
  end)
  return s
end

--- hw.With(signal, function(value) return widget end)
function hw.With(sig, fn)
  return { type = "With", signal = sig, fn = fn }
end

--- hw.For(list_signal, function(item, index) return widget end [, key_fn(item)])
function hw.For(sig, fn, key)
  return { type = "For", each = sig, fn = fn, key = key }
end

-- W.Box{...}, W.Label("text"), W.MyDefined{...}
local W = setmetatable({ With = hw.With, For = hw.For }, {
  __index = function(_, name)
    return function(props)
      if type(props) ~= "table" or hw.is_signal(props) then
        props = { label = props }
      end
      local def = hw._defined(name)
      if def then return def(props) end
      if hw._is_kind(name) then
        props.type = name
        return props
      end
      error("unknown widget W." .. tostring(name), 2)
    end
  end,
})
hw.widget = W
hw.W = W

return spawn, request
