---@meta
-- LuaLS type stubs for `hw.service.*`. Docs: docs/services.md,
-- docs/services-dbus.md. Prop names are checked against the services'
-- PROPS by the `docs_cover_api` test.

---Native services also expose `name`, `props` and `methods`.
---@class hw.NativeService
---@field name string
---@field props string[]
---@field methods string[]

---@class hw.services
---@field names string[] Native service names.
---@field hyprland hw.services.hyprland
---@field sysinfo hw.services.sysinfo
---@field apps hw.services.apps
---@field audio hw.services.audio
---@field battery hw.services.battery
---@field brightness hw.services.brightness
---@field hyprsunset hw.services.hyprsunset
---@field theme hw.services.theme
---@field keystrokes hw.services.keystrokes
---@field notifd hw.services.notifd
---@field mpris hw.services.mpris
---@field tray hw.services.tray
---@field upower hw.services.upower
---@field power_profiles hw.services.power_profiles
---@field [string] table Services registered by plugins.

---@class hw.services.hyprland: hw.NativeService
---@field available hw.Signal boolean
---@field workspaces hw.Signal list as `hyprctl -j workspaces`
---@field monitors hw.Signal list as `hyprctl -j monitors`
---@field clients hw.Signal list as `hyprctl -j clients`
---@field focused_client hw.Signal active window (`activewindow`)
---@field focused_workspace hw.Signal active workspace
---@field focused_monitor hw.Signal monitor name
---@field fullscreen hw.Signal `{ [monitor] = fullscreen mode }`
---@field special hw.Signal `{ [monitor] = open special workspace }`
---@field event hw.Signal last raw event `{name, data, args}`
local hyprland = {}
---`dispatch <args>`; 0.57: `hypr:dispatch('hl.dsp.focus({ workspace = 3 })')`.
---@param ... string
function hyprland:dispatch(...) end
---@param option string
---@param ... any
function hyprland:keyword(option, ...) end
---Raw request; `j/…` returns decoded JSON.
---@param ... string
---@return any
function hyprland:message(...) end
function hyprland:refresh() end

---@class hw.services.sysinfo: hw.NativeService
---@field cpu hw.Signal `{usage, cores, threads, clock_ghz, temp}`
---@field memory hw.Signal `{total, used, available, usage, swap_total, swap_used, swap_usage}` (bytes, %)
---@field gpu hw.Signal `{label, load, memory_total, memory_used, temp}`
---@field temperatures hw.Signal list `{label, temp, critical}`
---@field disks hw.Signal list `{device, mount, fs, total, used, available, usage, removable}`
---@field network hw.Signal `{interface, rx_speed, tx_speed, rx_today, tx_today, rx_total, tx_total}`
---@field uptime hw.Signal seconds
---@field load hw.Signal `{1m, 5m, 15m}`
---@field interval hw.Signal ms
local sysinfo = {}
---@param ms integer ≥ 250
function sysinfo:set_interval(ms) end

---@class hw.App
---@field id string
---@field name string
---@field generic_name string?
---@field description string?
---@field icon string?
---@field exec string[]
---@field terminal boolean
---@field categories string[]
---@field keywords string[]
---@field wm_class string?
---@field path string
---@field frequency integer

---@class hw.services.apps: hw.NativeService
---@field list hw.Signal list of `hw.App`
---@field history hw.Signal recently launched `hw.App`s
local apps = {}
function apps:reload() end
---@param text string
---@param limit? integer
---@return hw.App[]
function apps:query(text, limit) end
---@param text string
---@param limit? integer
---@return hw.App[]
function apps:exact_query(text, limit) end
---@param id string
---@return hw.App?
function apps:get(id) end
---@param app string|hw.App
function apps:launch(app) end

---@alias hw.AudioTarget nil|"speaker"|"sink"|"microphone"|"source"|integer|{id: integer}

---@class hw.services.audio: hw.NativeService
---@field available hw.Signal boolean
---@field speaker hw.Signal default sink `{id, name, description, icon, kind, media_class, volume, volumes, mute, state, is_default}`
---@field microphone hw.Signal default source
---@field sinks hw.Signal list of nodes
---@field sources hw.Signal list of nodes
---@field streams hw.Signal playback streams (`app_name`, `media_name`)
---@field recorders hw.Signal recording streams
local audio = {}
---@param target hw.AudioTarget
---@param volume number 0..1.5
function audio:set_volume(target, volume) end
---@param target hw.AudioTarget
---@param mute? boolean|"toggle"
function audio:set_mute(target, mute) end
---@param target hw.AudioTarget
function audio:toggle_mute(target) end
---@param target hw.AudioTarget
function audio:set_default(target) end

---@class hw.services.battery: hw.NativeService
---@field available hw.Signal boolean
---@field percentage hw.Signal 0..100
---@field state hw.Signal `"charging"`, `"discharging"`, `"full"`…
---@field charging hw.Signal boolean
---@field ac_online hw.Signal boolean
---@field time_to_empty hw.Signal seconds
---@field time_to_full hw.Signal seconds
---@field batteries hw.Signal list `{name, percentage, state, now, full, rate, model}`

---@class hw.services.brightness: hw.NativeService
---@field available hw.Signal boolean
---@field brightness hw.Signal 0..1 of the first display
---@field displays hw.Signal list `{id, kind, name, brightness, value, max}`
local brightness = {}
---@param value number 0..1
---@param display? string `"backlight:<dev>"` or `"ddc:<n>"`; nil = all
function brightness:set(value, display) end

---@class hw.services.hyprsunset: hw.NativeService
---@field available hw.Signal boolean
---@field temperature hw.Signal Kelvin
---@field gamma hw.Signal percent
---@field identity hw.Signal boolean
local hyprsunset = {}
---@param kelvin number
function hyprsunset:set_temperature(kelvin) end
---@param percent number
function hyprsunset:set_gamma(percent) end
function hyprsunset:set_identity() end
function hyprsunset:refresh() end

---@alias hw.ThemeMode "auto"|"dark"|"light"

---@class hw.services.theme: hw.NativeService
---@field colors hw.Signal Material roles (`primary`, `on_primary`, …) as `"#rrggbb"`
---@field mode hw.Signal `"dark"`/`"light"`
---@field source hw.Signal source color
---@field dominant hw.Signal dominant image color
---@field wallpaper hw.Signal image path
---@field scss hw.Signal `_material.scss` contents
---@field busy hw.Signal boolean
local theme_svc = {}
---@param path string
---@param mode? hw.ThemeMode
function theme_svc:set_wallpaper(path, mode) end
---@param mode hw.ThemeMode
function theme_svc:set_mode(mode) end
---@param hex string
---@param mode? hw.ThemeMode
function theme_svc:set_source(hex, mode) end
---@param path string
---@return {source: string, dominant: string, brightness: number, mode: string}
function theme_svc:from_image(path) end

---@class hw.services.keystrokes: hw.NativeService
---@field enabled hw.Signal boolean
---@field available hw.Signal boolean
---@field error hw.Signal string?
---@field devices hw.Signal string[]
---@field key hw.Signal last key `{label, keysym}` (every press notifies)
local keystrokes = {}
---@param on? boolean default true
function keystrokes:enable(on) end

---@class hw.Notification
---@field id integer
---@field app_name string
---@field app_icon string
---@field summary string
---@field body string
---@field actions {id: string, label: string}[]
---@field urgency "low"|"normal"|"critical"
---@field expire_timeout integer
---@field time integer unix seconds
---@field image string?
---@field desktop_entry string?
---@field category string?
---@field resident boolean
---@field transient boolean

---@class hw.services.notifd
---@field notifications hw.Signal list of `hw.Notification`, newest first
---@field dnd hw.Signal boolean, writable
---@field active hw.Signal we own org.freedesktop.Notifications
local notifd = {}
---@param fn fun(n: hw.Notification, replaced: boolean)
---@return fun() unsubscribe
function notifd:on_notified(fn) end
---@param fn fun(id: integer, reason: "expired"|"dismissed"|"closed"|"undefined")
---@return fun() unsubscribe
function notifd:on_resolved(fn) end
---@param id integer
function notifd:dismiss(id) end
---@param id integer
function notifd:close(id) end
function notifd:clear() end
---@param id integer
---@param action string
function notifd:invoke(id, action) end
---@param id integer
---@return hw.Notification?
function notifd:get(id) end
---@param on boolean
function notifd:set_dnd(on) end
---@param on boolean
function notifd:set_ignore_timeout(on) end

---@class hw.services.mpris
---@field players hw.Signal list `{bus_name, name, identity, playback_status, playing, title, artist, album, art_url, cover_art, length, position, volume, shuffle, loop_status, can_*…}`
local mpris = {}
---Interpolated position (s), ticks every second while playing.
---@param bus string
---@return hw.Signal
function mpris:position(bus) end
---@param bus string
function mpris:play_pause(bus) end
---@param bus string
function mpris:play(bus) end
---@param bus string
function mpris:pause(bus) end
---@param bus string
function mpris:stop(bus) end
---@param bus string
function mpris:next(bus) end
---@param bus string
function mpris:previous(bus) end
---@param bus string
function mpris:raise(bus) end
---@param bus string
---@param offset number seconds
function mpris:seek(bus, offset) end
---@param bus string
---@param seconds number
function mpris:set_position(bus, seconds) end
---@param bus string
---@param volume number
function mpris:set_volume(bus, volume) end
---@param bus string
---@param on boolean
function mpris:set_shuffle(bus, on) end
---@param bus string
---@param status "None"|"Track"|"Playlist"
function mpris:set_loop_status(bus, status) end

---@class hw.services.tray
---@field items hw.Signal list `{id, bus_name, title, status, icon, tooltip_text, item_is_menu, has_menu, …}`
---@field watcher_active hw.Signal boolean
local tray = {}
---@param id string
---@param x integer
---@param y integer
function tray:activate(id, x, y) end
---@param id string
---@param x integer
---@param y integer
function tray:secondary_activate(id, x, y) end
---@param id string
---@param x integer
---@param y integer
function tray:context_menu(id, x, y) end
---@param id string
---@param delta integer
---@param orientation "vertical"|"horizontal"
function tray:scroll(id, delta, orientation) end
---In a MenuButton's `setup`: sets its menu (kept in sync with the app).
---@param id string
---@param widget hw.Widget
---@return boolean
function tray:attach_menu(id, widget) end
---Pops the menu under any widget.
---@param id string
---@param widget hw.Widget
---@return boolean
function tray:popup_menu(id, widget) end
---@param id string
---@return table? tree `{id, label, separator, enabled, visible, icon_name, toggle, toggle_state, submenu, children}`
function tray:menu_layout(id) end
---@param id string
function tray:about_to_show(id) end

---@class hw.services.upower
---@field battery hw.Signal whole table
---@field available hw.Signal
---@field is_present hw.Signal
---@field percentage hw.Signal 0..100
---@field state hw.Signal
---@field charging hw.Signal
---@field time_to_empty hw.Signal
---@field time_to_full hw.Signal
---@field icon_name hw.Signal
---@field energy_rate hw.Signal
---@field energy hw.Signal
---@field energy_full hw.Signal
---@field warning_level hw.Signal

---@class hw.services.power_profiles
---@field state hw.Signal
---@field available hw.Signal
---@field profiles hw.Signal list `{profile, driver}`
---@field performance_degraded hw.Signal
---@field active_profile hw.Signal writable
local power_profiles = {}
---@param name string
function power_profiles:set_active_profile(name) end
