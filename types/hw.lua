---@meta hw
-- LuaLS type stubs for the `hw` API of hyprwidget. Docs: docs/README.md.
-- Use with `"workspace.library": ["<repo>/types"]` in .luarc.json.

---A reactive value. Call it (`s()`) or `:get()` to read with dependency
---tracking, `:peek()` to read without. Signals of options and services are
---read-only; `hw.state` and `hw.settings` signals are writable.
---@class hw.Signal
---@operator call: any
local Signal = {}
---@return any
function Signal:get() end
---Reads without tracking (inside `hw.computed`).
---@return any
function Signal:peek() end
---Notifies subscribers when the value changed (tables compare by reference).
---@param value any
function Signal:set(value) end
---`set(not value)`.
function Signal:toggle() end
---A derived signal.
---@param fn fun(value: any): any
---@return hw.Signal
function Signal:map(fn) end
---Calls `fn(value)` on every change; returns an unsubscribe function.
---@param fn fun(value: any)
---@return fun()
function Signal:subscribe(fn) end

---@class hw.Handle
local Handle = {}
---Stops the timer / file watch.
function Handle:cancel() end

---@class hw.Process
local Process = {}
function Process:kill() end
---@param signum integer
function Process:signal(signum) end
---Writes to the process' stdin.
---@param text string
function Process:write(text) end
---@return string?
function Process:pid() end

---@class hw.Monitor
---@field name string Connector (`"DP-2"`).
---@field connector string
---@field description string?
---@field manufacturer string?
---@field model string?
---@field x integer
---@field y integer
---@field width integer
---@field height integer
---@field scale number
---@field refresh_rate integer mHz

---@class hw.Window
---@field visible boolean
local Window = {}
function Window:show() end
function Window:hide() end
---@return boolean visible
function Window:toggle() end
---@return boolean
function Window:is_visible() end
---@param prop string
---@param value any
function Window:set(prop, value) end
---@return hw.Widget
function Window:widget() end
function Window:destroy() end

---@alias hw.Command string|string[]

---@class hw.json
local json = {}
---@param text string
---@return any
function json.decode(text) end
---@param value any
---@param pretty? boolean
---@return string
function json.encode(value, pretty) end

---`hw.settings` (AGS-compatible settings.json). Indexing returns a
---writable signal: `hw.settings["bar.lock"]`.
---@class hw.Settings
---@field [string] hw.Signal
local Settings = {}
---@param key string Dotted path.
---@return any
function Settings:get(key) end
---@param key string
---@param value any
---@return any stored Validated/clamped value.
function Settings:set(key, value) end
---@param key string
---@return boolean
function Settings:toggle(key) end
---`{ name, value, type, min, max }` of an AGSSetting, or nil.
---@param key string
---@return table?
function Settings:meta(key) end
---@param key string
---@return hw.Signal
function Settings:bind(key) end

---@class hw.window
local window = {}
---@param name string
---@return hw.Window?
function window.get(name) end
---@param props hw.WindowProps
---@return hw.Window?
function window.new(props) end
---@return string[]
function window.list() end
---@param name string
---@return boolean? visible
function window.toggle(name) end

---@class hw.PluginInfo
---@field name string
---@field version string
---@field dir string
---@field permissions {exec: boolean, network: boolean, fs: boolean}

---@class hw.plugins
local plugins = {}
---@return string[]
function plugins.list() end
---@param name string
---@return {name: string, version: string, description: string?, dir: string, entry: string, permissions: table, widgets: string[], error: string?}?
function plugins.get(name) end
---@return table<string, string>
function plugins.errors() end

---`hw.theme.colors.primary` → signal; `hw.theme.colors()` → table.
---@class hw.ThemeColors
---@operator call: table<string, string>
---@field [string] hw.Signal

---Selected theme (`themes/<name>`). Unknown keys are forwarded to
---`hw.service.theme` (`hw.theme:set_wallpaper(path)`).
---@class hw.theme: hw.services.theme
---@field name hw.Signal Theme name (nil = none); writable.
---@field colors hw.ThemeColors
---@field palette hw.Signal Whole color table.
local theme = {}
---@param name string? nil/"none" = no theme.
function theme.set(name) end
---@return string[]
function theme.list() end
---@return {name: string?, colors: table<string, string>, source: string?}
function theme.get() end
---@return string[]
function theme.sources() end
---Registers a color source for themes with `"source": "<name>"`.
---@param name string
---@param colors table<string, string>|hw.Signal
function theme.add_source(name, colors) end

---@class hw
---@field widget hw.W
---@field W hw.W
---@field service hw.services
---@field settings hw.Settings
---@field opt hw.Opt Option tree; leaves are signals (`hw.opt.general.gaps_out`).
---@field monitors hw.Signal List of `hw.Monitor` (debounced 300 ms).
---@field window hw.window
---@field theme hw.theme
---@field plugins hw.plugins
---@field json hw.json
---@field config_dir string
---@field headless boolean True in `hyprwidget check` / offline `option`.
---@field plugin hw.PluginInfo Plugins only: this plugin.
hw = {}

---Writable signal.
---@param value any
---@return hw.Signal
function hw.state(value) end

---Signal recomputed when a signal read by `fn` changes.
---@param fn fun(): any
---@return hw.Signal
function hw.computed(fn) end

---Signal set from a command's stdout (async, every `ms`) or from `fn()`.
---@param ms integer
---@param cmd_or_fn hw.Command|fun(): any
---@param transform? fun(out: string, prev: any): any
---@return hw.Signal
function hw.poll(ms, cmd_or_fn, transform) end

---AGS' createBinding: `hw.bind(signal)`, `hw.bind(hw.settings, key)`,
---`hw.bind(service_or_table, key)`.
---@param obj any
---@param key? string
---@return hw.Signal
function hw.bind(obj, key) end

---@param value any
---@return boolean
function hw.is_signal(value) end

---One child rebuilt from `fn(value)` when `signal` changes.
---@param signal hw.Signal
---@param fn? fun(value: any): hw.Child
---@return hw.WithNode
function hw.With(signal, fn) end

---Keyed list: widgets of items whose key survives are reused.
---@param signal hw.Signal
---@param fn fun(item: any, index: integer): hw.Child
---@param key? fun(item: any): any
---@return hw.ForNode
function hw.For(signal, fn, key) end

---@param ms integer
---@param fn function
---@return hw.Handle
function hw.timeout(ms, fn) end

---Runs `fn` now, then every `ms`.
---@param ms integer
---@param fn function
---@return hw.Handle
function hw.interval(ms, fn) end

---Monotonic time in seconds.
---@return number
function hw.now() end

---Runs a command synchronously; returns stdout, raises on failure.
---@param cmd hw.Command
---@return string
function hw.exec(cmd) end

---Runs a command async. With a callback: `cb(stdout)` / `cb(nil, err)`.
---Without: yields (inside handlers / `hw.async`) and returns stdout.
---@param cmd hw.Command
---@param cb? fun(out: string?, err: string?)
---@return string?
function hw.exec_async(cmd, cb) end

---Long-running process; `on_line` gets stdout lines, `on_err` stderr lines.
---@param cmd hw.Command
---@param on_line? fun(line: string)
---@param on_err? fun(line: string)
---@return hw.Process
function hw.subprocess(cmd, on_line, on_err) end

---Runs `fn(...)` in a coroutine (so it may call `hw.exec_async`).
---@param fn function
---@param ... any
---@return any
function hw.async(fn, ...) end

---Plugins with the `network` permission: HTTP(S) GET through curl.
---@param url string
---@param cb? fun(body: string?, err: string?)
---@return string?
function hw.fetch(url, cb) end

---Signal of a merged option (`"decoration:rounding"`).
---@param name hw.OptionName|string
---@return hw.Signal
function hw.option(name) end

---Deep-merges options (Hyprland names) in load order. Values: constants,
---`"inherit"`, or `function(h)` of the Hyprland values.
---@param options hw.Config
function hw.config(options) end

---Writes `config/custom/<name>.lua` and applies the option live.
---@param name hw.OptionName|string
---@param value boolean|number|string|table
---@param target? "auto"|"hypr"|"widget"
---@return "hypr"|"widget"
function hw.set_option(name, value, target) end

---Adds a Hyprland layer rule (`hl.layer_rule{...}`).
---@param rule table
function hw.layer_rule(rule) end

---Writes the SCSS partial `_<name>.scss` (`@import "<name>";`).
---@param name string
---@param text string
function hw.scss(name, text) end

---@param path string
---@param fn fun()
---@return hw.Handle
function hw.watch_file(path, fn) end

---Handler for `hyprwidget request <name> [args…]`; its return value is printed.
---@param name string
---@param fn fun(args: string[]): any
function hw.on_request(name, fn) end

---Creates windows for every monitor (recreated on hot-plug).
---@param fn fun(monitor: hw.Monitor): hw.Node|hw.Node[]
function hw.per_monitor(fn) end

---Custom widget usable as `W.<name>{}` and `{"type": "<name>"}`.
---@param name string
---@param fn fun(props: table): hw.Child
function hw.define(name, fn) end

---Loads a JSON layout (path relative to the config dir) as widget tables.
---@param path string
---@return hw.Node
function hw.layout(path) end

---Requires `<prefix>.<name>` for each name (prefix default `"config"`).
---@param names string[]
---@param prefix? string
function hw.require_all(names, prefix) end

---Requires every `<dir>/*.lua` (default `config/custom`), sorted.
---@param dir? string
function hw.require_custom_dir(dir) end

function hw.quit() end

---`debug.traceback` replacement for `xpcall`.
---@param msg any
---@return string
function hw._traceback(msg) end

---@type hw.W
W = nil -- only a global inside plugins; configs use `local W = hw.widget`

---@type string
HOME = nil
---@type string
CONFIG_DIR = nil
---Plugins only.
---@type string
PLUGIN_DIR = nil

---@param names string[]
---@param prefix? string
function require_all(names, prefix) end

---@param dir? string
function require_custom_dir(dir) end

return hw
