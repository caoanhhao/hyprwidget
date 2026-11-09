# Services (`hw.service.*`)

Native Rust services are **lazy**: a service starts the first time Lua accesses
`hw.service.<name>`, then lives across every Lua reload. Available names:
`hw.service.names` = hyprland, sysinfo, apps, audio, battery, brightness,
hyprsunset, theme, keystrokes; plus the D-Bus services notifd, mpris, tray, upower,
power_profiles; plus services registered by plugins.

Common conventions:

- **Props are Signals** (read-only): `svc.prop()` to read, `svc.prop:map(fn)`,
  `svc.prop:subscribe(fn)`, `hw.bind(svc, "prop")`, or assign them straight to a
  widget prop. Lists/objects are snapshot tables (JSON data) — pass the `id` to
  methods.
- **Methods** are called with `:` (`svc:method(args…)`), synchronously, and raise
  on error. For native services, `svc.props` / `svc.methods` list the names and
  `svc.name` is the service name.
- A prop without data yet is `nil` (e.g. a machine without a battery).

The prop/method lists below match the code (the `docs_cover_api` test checks that
every service name is on this page and in `types/`). To get them straight from the
daemon:

```sh
hyprwidget eval 'local s = hw.service.audio return table.concat(s.props, ", ") .. " | " .. table.concat(s.methods, ", ")'
```

## hyprland

Hyprland state through its IPC sockets (`.socket.sock` for requests,
`.socket2.sock` for events); event bursts are coalesced (12 ms) and only the
affected parts are reloaded.

| Prop | Value |
|---|---|
| `available` | running under Hyprland |
| `workspaces` | list as in `hyprctl -j workspaces`: `{id, name, monitor, monitorID, windows, hasfullscreen, lastwindow, lastwindowtitle, ispersistent, …}` |
| `monitors` | list as in `hyprctl -j monitors` (`name`, `id`, `width`, `height`, `scale`, `focused`, `activeWorkspace`, `specialWorkspace`, `reserved`…) |
| `clients` | list as in `hyprctl -j clients` (`address`, `class`, `title`, `workspace = {id, name}`, `monitor`, `pid`, `at`, `size`, `floating`, `fullscreen`, `focusHistoryID`…) |
| `focused_client` | the focused client (`activewindow`), or empty |
| `focused_workspace` | the active workspace |
| `focused_monitor` | name of the focused monitor (`"DP-2"`) |
| `fullscreen` | `{ [monitor] = mode }`: highest fullscreen mode on each monitor's current workspace (0 = none) |
| `special` | `{ [monitor] = name of the open special workspace }` |
| `event` | the last raw event `{name, data, args}` (every event notifies, even repeats) |

| Method | |
|---|---|
| `:dispatch(...)` | `dispatch <args joined by spaces>`; Hyprland 0.57 takes Lua: `hypr:dispatch('hl.dsp.focus({ workspace = 3 })')` |
| `:keyword(option, value...)` | `keyword` (removed in Hyprland 0.57 — use `hw.set_option`) |
| `:message(...)` | raw request; starting with `j/` returns a JSON table |
| `:refresh()` | reload everything |

## sysinfo

CPU, RAM, GPU, temperatures, disks, network; a worker thread polls every
`interval` ms (default 2000). Sizes are in bytes.

| Prop | Value |
|---|---|
| `cpu` | `{usage (%), cores = {%…}, threads, clock_ghz, temp}` |
| `memory` | `{total, used, available, usage (%), swap_total, swap_used, swap_usage}` |
| `gpu` | `{label, load (%), memory_total, memory_used, temp}` (NVIDIA through `nvidia-smi`, at most every 5 s) |
| `temperatures` | list of `{label, temp, critical}` |
| `disks` | list of `{device, mount, fs, total, used, available, usage, removable}` |
| `network` | `{interface, rx_speed, tx_speed (B/s), rx_today, tx_today, rx_total, tx_total}`; daily totals are stored in `~/.cache/hyprwidget/bandwidth.log` (AGS format) |
| `uptime` | seconds |
| `load` | `{1m, 5m, 15m}` |
| `interval` | ms |

Method: `:set_interval(ms)` (minimum 250).

## apps

Desktop entries, fuzzy search (`nucleo`) ranked by usage frequency, launched
through Hyprland (`hl.dsp.exec_cmd`, so apps are Hyprland's children and follow its
rules), history in `$XDG_CACHE_HOME/hyprwidget/apps.json`.

| Prop | Value |
|---|---|
| `list` | every visible app: `{id, name, generic_name, description, icon, exec, terminal, categories, keywords, wm_class, path, frequency}` |
| `history` | recently launched apps (at most 20) |

| Method | |
|---|---|
| `:query(text [, limit])` | fuzzy, best first (empty text = by frequency) |
| `:exact_query(text [, limit])` | substring match |
| `:get(id)` | app by id (the `.desktop` file name without extension) or nil |
| `:launch(id_or_app)` | runs the app and records it in the history |
| `:reload()` | rescan desktop entries |

## audio

PipeWire/WirePlumber: read with `pw-dump --monitor`, written with `wpctl`. Volume
uses WirePlumber's "cubic" scale (1.0 = 100 %, as in `wpctl get-volume`).

| Prop | Value |
|---|---|
| `available` | `pw-dump` can run |
| `speaker`, `microphone` | default node: `{id, name, description, icon, kind, media_class, volume, volumes, mute, state, is_default}` |
| `sinks`, `sources` | lists of nodes as above |
| `streams`, `recorders` | apps' playback / recording streams (plus `app_name`, `media_name`) |

| Method | |
|---|---|
| `:set_volume(target, v)` | `v` 0..1.5 |
| `:set_mute(target [, on])` | `on` = true/false/`"toggle"` (default toggle) |
| `:toggle_mute(target)` | |
| `:set_default(target)` | |

`target`: `nil`/`"speaker"`/`"sink"` (default sink), `"microphone"`/`"source"`, a
node id, or a node table with an `id`.

## battery

Batteries and AC from `/sys/class/power_supply`, polled every 10 s (no daemon
needed; the UPower version is `upower`).

| Prop | Value |
|---|---|
| `available` | there is a battery |
| `percentage` | 0..100 (all batteries combined) |
| `state` | `"charging"`, `"discharging"`, `"full"`, … |
| `charging`, `ac_online` | bool |
| `time_to_empty`, `time_to_full` | seconds |
| `batteries` | list of `{name, percentage, state, now, full, rate, model}` |

## brightness

Laptop screens through `/sys/class/backlight` (written with `brightnessctl`) and
external screens through DDC/CI (`ddcutil`, VCP 0x10; polled every 30 s, updated
optimistically on set).

| Prop | Value |
|---|---|
| `available` | at least one screen can be controlled |
| `brightness` | 0..1 of the first screen (nil if none) |
| `displays` | list of `{id ("backlight:<dev>" / "ddc:<n>"), kind, name, brightness (0..1), value, max}` |

Method: `:set(v [, display_id])` — `v` 0..1, every screen or one screen.

## hyprsunset

Blue-light filter through hyprsunset's IPC socket (`.hyprsunset.sock`); polled
every 10 s.

| Prop | Value |
|---|---|
| `available` | hyprsunset is running |
| `temperature` | Kelvin |
| `gamma` | % |
| `identity` | in no-filter mode |

Methods: `:set_temperature(k)` (1000..20000, leaves identity), `:set_gamma(pct)`
(0..200), `:set_identity()`, `:refresh()`.

## theme

Material You palette computed from the wallpaper (the `material-colors` crate,
replacing `get-image-color.sh`). Also reachable as `hw.theme` (keys that are not
`hw.theme`'s own are forwarded: `hw.theme:set_wallpaper(p)`). The palette is the
`wallpaper` color source of the theme system ([plugins.md](plugins.md#theme)) and is
written to the `_material.scss` partial (`@import "material";` → `$md-primary`,
`$md-on-primary`…, `--md-*`).

| Prop | Value |
|---|---|
| `colors` | every Material role: `primary`, `on_primary`, `primary_container`, `secondary`, `tertiary`, `error`, `background`, `surface`, `surface_variant`, `outline`, `inverse_*`, `*_fixed`… (`"#rrggbb"`) |
| `mode` | `"dark"` / `"light"` |
| `source` | source color |
| `dominant` | dominant color of the image |
| `wallpaper` | image path |
| `scss` | partial contents |
| `busy` | computing |

| Method | |
|---|---|
| `:set_wallpaper(path [, mode])` | `mode` `"auto"` (light if the image is light), `"dark"`, `"light"` |
| `:set_mode(mode)` | keeps the source color |
| `:set_source(hex [, mode])` | |
| `:from_image(path)` | → `{source, dominant, brightness, mode}` (does not change the theme) |

## keystrokes

Key presses read from `/dev/input/event*` (replacing `keystroke-loop-ags.c`),
keycode → keysym through xkbcommon using Hyprland's `input:kb_layout`/`kb_variant`.
Needs permission to read the devices (`sudo usermod -aG input $USER`, then log in
again). Reads nothing until `:enable(true)`.

| Prop | Value |
|---|---|
| `enabled`, `available` | bool |
| `error` | why reading failed |
| `devices` | open keyboard devices |
| `key` | last key `{label, keysym}` (every press notifies) |

Method: `:enable([on = true])`.

## D-Bus services

Full details: [services-dbus.md](services-dbus.md). Summary:

| Service | Fields (Signals) | Methods |
|---|---|---|
| `notifd` | `notifications`, `dnd` (writable), `active` | `:on_notified(fn(n, replaced))`, `:on_resolved(fn(id, reason))` (return an unsubscribe function), `:dismiss(id)`, `:close(id)`, `:clear()`, `:invoke(id, action)`, `:get(id)`, `:set_dnd(b)`, `:set_ignore_timeout(b)` |
| `mpris` | `players` | `:position(bus)` (Signal), `:play_pause/play/pause/stop/next/previous/raise(bus)`, `:seek(bus, s)`, `:set_position(bus, s)`, `:set_volume(bus, v)`, `:set_shuffle(bus, b)`, `:set_loop_status(bus, s)` |
| `tray` | `items`, `watcher_active` | `:activate/secondary_activate/context_menu(id, x, y)`, `:scroll(id, delta, orientation)`, `:attach_menu(id, menubutton)`, `:popup_menu(id, widget)`, `:menu_layout(id)`, `:about_to_show(id)` |
| `upower` | `battery`, `available`, `is_present`, `percentage`, `state`, `charging`, `time_to_empty`, `time_to_full`, `icon_name`, `energy_rate`, `energy`, `energy_full`, `warning_level` | |
| `power_profiles` | `state`, `available`, `profiles`, `performance_degraded`, `active_profile` (writable) | `:set_active_profile(name)` |

## Plugin services

A plugin returning `services = { name = table_or_fn }` → `hw.service.name` (see
[plugins.md](plugins.md)). For example `hw.service.sysmon.summary` from the `sysmon`
plugin.
