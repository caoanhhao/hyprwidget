# D-Bus services (M2 part B)

`crates/hw-services` implements the D-Bus services natively with `zbus`; `crates/hw-lua/src/dbus_services.rs`
exposes them as `hw.service.<name>`. Each table is built on first access, which starts the service. Services are
per-process singletons and survive Lua reloads.

Fields are signals: call them (`s()`), `:map`, `:subscribe`, or `hw.bind(service, "field")`. Methods are called
with `:`. Lists are plain Lua tables (snapshots), so pass ids (`bus_name`, `id`) to the methods.

All service logic runs as futures on the GTK main loop; zbus drives the socket on its own thread. Object-server
handlers (notifd, SNI watcher) forward to the main thread through `async_channel`.

## `hw.service.notifd`

A full `org.freedesktop.Notifications` server (spec 1.2). If another daemon owns the name (AGS), we queue for it:
`active` stays `false`, history is still readable, and we take over when the other daemon exits. Nothing is
killed or replaced.

| Field / method | |
|---|---|
| `notifications` | unresolved notifications, newest first; persisted (minus `transient`) to `$XDG_CACHE_HOME/hyprwidget/notifd/history.json`, max 500 |
| `dnd` | do-not-disturb, writable (`dnd:set(true)`), persisted. Notifications are still recorded; popups are up to Lua |
| `active` | we own the bus name |
| `:on_notified(fn(n, replaced))` / `:on_resolved(fn(id, reason))` | returns an unsubscribe function; `reason` is `expired`, `dismissed`, `closed`, `undefined` |
| `:dismiss(id)`, `:close(id)`, `:clear()` | `NotificationClosed` with reason 2 / 3 / 2 |
| `:invoke(id, action_id)` | `ActionInvoked`, then dismisses unless `resident` |
| `:get(id)`, `:set_dnd(b)`, `:set_ignore_timeout(b)` | |

A notification: `id, app_name, app_icon, summary, body, actions = {{id, label}}, urgency ("low"/"normal"/"critical"),
expire_timeout, time (unix s), image, desktop_entry, category, resident, transient, action_icons, sound_file,
sound_name, suppress_sound, value`. `image` is the `image-data` hint saved as a PNG in the cache, else the
`image-path` hint (file path or icon name). Positive expire timeouts close with `expired` (never for critical,
or with `set_ignore_timeout(true)`).

## `hw.service.mpris`

| Field / method | |
|---|---|
| `players` | `{bus_name, name, identity, desktop_entry, playback_status ("playing"/"paused"/"stopped"), playing, track_id, title, artist, artists, album, album_artist, url, art_url, cover_art, length (s), position (s, last update), rate, volume, shuffle, loop_status, can_play, can_pause, can_go_next, can_go_previous, can_seek, can_control, can_raise, can_quit}` |
| `:position(bus_name)` | interpolated position signal (s), ticks every second while playing, updated on `Seeked` |
| `:play_pause/play/pause/stop/next/previous/raise(bus_name)` | |
| `:seek(bus_name, offset_s)`, `:set_position(bus_name, s)` | |
| `:set_volume(bus_name, v)`, `:set_shuffle(bus_name, b)`, `:set_loop_status(bus_name, "None"/"Track"/"Playlist")` | |

`cover_art` is a local file: `file://` art directly, `http(s)` art downloaded through GIO to
`$XDG_CACHE_HOME/hyprwidget/mpris-art`. A process owning several MPRIS names (VLC) is listed once.

## `hw.service.tray`

StatusNotifierWatcher (served when free, queued otherwise) + StatusNotifierHost.

| Field / method | |
|---|---|
| `items` | `{id, bus_name, object_path, item_id, title, category, status, icon, icon_name, icon_theme_path, attention_icon_name, overlay_icon_name, tooltip_title, tooltip_body, tooltip_text, item_is_menu, menu_path, has_menu}` |
| `watcher_active` | we serve `org.kde.StatusNotifierWatcher` |
| `:activate(id, x, y)`, `:secondary_activate(id, x, y)`, `:context_menu(id, x, y)`, `:scroll(id, delta, "vertical"/"horizontal")` | |
| `:attach_menu(id, widget)` | in a `MenuButton`'s `setup`: sets its `gio::MenuModel` and the `dbusmenu` action group (kept in sync; `AboutToShow` on open) |
| `:popup_menu(id, widget)` | pops a `PopoverMenu` under any widget |
| `:menu_layout(id)` | the DBusMenu tree as tables (`id, label, separator, enabled, visible, icon_name, toggle, toggle_state, submenu, children`), nil until loaded |
| `:about_to_show(id)` | |

`icon` is what to display: a file (pixmap saved as PNG in `$XDG_RUNTIME_DIR/hyprwidget/tray`, an absolute
`IconName`, or a match in `IconThemePath`) or an icon name. The attention icon is used for `NeedsAttention`.

```lua
W.MenuButton {
  class = "tray-icon",
  tooltip = item.tooltip_text,
  setup = function(self) hw.service.tray:attach_menu(item.id, self) end,
  W.Image { icon = item.icon, pixel_size = 11 },
}
```

## `hw.service.upower`

`battery` (whole table) and one signal per field: `available, is_present, percentage (0–100), state
("charging", "discharging", "fully-charged", …), charging, time_to_empty, time_to_full (s), on_battery, icon_name,
energy_rate, energy, energy_full, warning_level`. Source: UPower's `DisplayDevice` and `OnBattery`.

## `hw.service.power_profiles`

`state`, `available`, `profiles` (`{{profile, driver}}`), `performance_degraded`, `active_profile` (writable) and
`:set_active_profile(name)`.

## Testing without touching the running session

AGS owns the notification and watcher names on the real session bus. To exercise notifd / the watcher, run a
private bus:

```sh
dbus-run-session -- sh -c 'hyprwidget -i test run -c CONFIG & sleep 2; notify-send hi; hyprwidget -i test eval "hw.service.notifd.notifications()"'
```

`cvlc --intf dummy --control dbus --aout dummy FILE` gives a silent MPRIS player on that bus.
