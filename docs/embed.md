# Apps as widgets (`W.App`, `hw-embed`) — M5

Completes `PLAN.md` §6 (§6.6 steps 3–5) on top of the M0.5 spike
(`docs/embed-spike.md`). Measured 2026-11-09 on the same machine: RTX 4070,
Hyprland 0.57.1, DP-2 2560x1080 scale 1, GTK 4.24.1, Qt 6.12, fcitx5 5.1.23 +
lotus.

## Usage

```lua
local app
W.App {
  command   = { "pavucontrol" },        -- or a shell string ("sh -c")
  env       = { GTK_THEME = "Adwaita:dark" },
  class     = "embedded-mixer",
  size      = { 560, 600 },             -- natural size; nil = the app's own size
  mode      = "resize",                 -- "resize" | "scale"
  keep_alive = false,                   -- false: closed 2 s after the widget is hidden
  restart   = "on-failure",             -- "never" | "on-exit" | "on-failure"
  frame_when_hidden = false,
  socket    = false,                    -- WAYLAND_DISPLAY=hyprwidget-embed-N instead of WAYLAND_SOCKET
  rounding  = 0,                        -- default: decoration:rounding; 0 keeps zero-copy
  on_title  = function(self, t) title:set(t) end,
  on_exit   = function(self, code) end,
  setup     = function(self) app = self end,
}
-- app:send_close(), app:restart(), app:focus(), app.pid,
-- app.title / app.app_id (read-only Signals)
```

JSON: `{"type": "App", "command": ["kitty"], "keep_alive": true}`.

Demo: `examples/demo/config/embed.lua` (mixer panel with pavucontrol), off by
default; enable with `"embed": {"mixer": true}` in
`cache/settings/settings.json`, then `hyprwidget toggle embed-mixer-DP-2`.

Dev harness: `cargo run -p hw-embed --example embed-test -- <cmd>` (env:
`HW_EMBED_SOCKET=1`, `HW_EMBED_MODE=scale`, `HW_EMBED_ROUNDING=16`,
`HW_EMBED_RESTART=on-exit`, `HW_EMBED_SIZE=WxH`; `kill -USR1` hides/shows).

## App matrix

| App | Installed | Mode | Result |
|---|---|---|---|
| foot | **no** (was installed for M0.5, now absent) | – | not tested; kitty used instead |
| kitty 0.49.2 (terminal, GL) | yes | `WAYLAND_SOCKET` | renders, title/app_id reported |
| pavucontrol 6.2 (GTK4) | yes | `WAYLAND_SOCKET` | renders, no headerbar (maximized/tiled), demo panel works incl. keep_alive stop/restart |
| zenity 4 (GTK4/libadwaita) | yes | `WAYLAND_SOCKET` | renders; **Vietnamese typing works** (see below) |
| nautilus 51 (GTK4) | yes | `dbus-run-session nautilus` | renders (own CSD buttons). Plain `nautilus` would open in the user's running instance via D-Bus — single-instance limit, see below |
| Qt6: fcitx5-config-qt | yes | `WAYLAND_SOCKET` | renders, no titlebar (xdg-decoration server-side). qalculate-qt / dolphin not installed |
| mpv 0.41 | yes | **socket** | renders 60 fps. With `WAYLAND_SOCKET` mpv fails (opens a 2nd connection for its clipboard) → use `socket = true` |
| firefox (`--new-instance -P widget`) | yes | **socket** | renders. With `WAYLAND_SOCKET` it falls back to X11 ("no DISPLAY"). A separate `widget` profile was created with `firefox -CreateProfile widget`; the default profile is untouched |

Rule of thumb: GTK/Qt/terminals work with the default `WAYLAND_SOCKET`;
multi-connection or multi-process apps (mpv, firefox, chromium, anything that
re-execs) need `socket = true`.

## Vietnamese typing (fcitx5 + lotus)

Verified in an embedded GTK4 app (zenity `--text-info --editable`): typing
`xin chaof, tieesng vieejt` with lotus active produced **"xin chào, tiếng
việt"**. Keys were injected with `wtype -d 120` (≈ human speed) through
Hyprland → fcitx5 → hyprwidget's `GtkIMContext` → `text-input-v3` → app.

Known issue: at machine speed (`wtype` without delay) one transform was lost
("tieếng"): lotus edits through surrounding text, and the app's updated
surrounding text arrives one round-trip after the raw key. Not seen at typing
speed.

Embedded apps get `GTK_IM_MODULE=wayland` and `QT_IM_MODULE=wayland` so they
use text-input-v3 (the user's session sets `fcitx`, which would bypass the
bridge and put the candidate window in the wrong place). Override via `env`.

Not verified interactively (the desktop was in use; synthetic keys briefly
landed in another window, so testing stopped): typing in Qt apps, popups
opened by clicks, clipboard both ways, touch.

## How it works (additions over the spike)

`crates/hw-embed/src/`:

- `surface.rs` — `SurfaceView`: one widget per `xdg_toplevel` / `xdg_popup`
  root. Every surface of the tree (root + subsurfaces) gets its own
  `GtkGraphicsOffload › GtkPicture` placed at its subsurface offset, window
  geometry applied, `scale` mode via a transform. Owns the input controllers
  and the `GtkIMMulticontext`.
- `input.rs` — pointer (motion, enter/leave, buttons via
  `EventControllerLegacy`, axis: wheel → value 15/notch + v120, touchpad →
  smooth + axis stop), keyboard (XKB keycodes from GDK; GDK's own repeats
  dropped, the client repeats with Hyprland's `input:repeat_rate/delay`;
  keymap = Hyprland's `input:kb_*`), touch (slots from event sequences).
  Hit-testing walks the surface tree with input regions. Keyboard focus
  follows GTK focus; held keys are released on focus-out.
- `text_input.rs` — `zwp_text_input_v3` implemented directly (smithay's only
  talks to an input-method client). Enable → the `SurfaceView` attaches its IM
  context to the key controller; cursor rectangle, surrounding text and
  content type are mirrored; IM commit/preedit/delete-surrounding are batched
  per idle into one `done` (delete is flushed first if a commit is pending).
- `popup.rs` — `xdg_popup` → `GtkPopover` (no arrow, no padding, halign
  start, pointing at the positioner rectangle), so on Hyprland it becomes an
  `xdg_popup` of the layer surface and is not clipped. `grab` → autohide +
  focus; popover closed → `popup_done`; reposition supported.
- `clipboard.rs` — client selection → `GdkContentProvider` subclass that pipes
  from the client; GDK clipboard changes → server selection, reads served by
  `gdk_clipboard_read_async`. Same for primary selection.
- `server.rs` — adds `wp_cursor_shape_v1` + cursor surfaces (→
  `gdk::Cursor`), `wp_fractional_scale_v1` (widget surface scale),
  `wp_single_pixel_buffer_v1`, viewporter sizes, KDE server-decoration
  (GTK3/Qt drop their titlebar), dialogs (toplevels with a parent) centred at
  their own size over the main toplevel.
- `view.rs` / `process.rs` — lifecycle: lazy start on first map
  (`keep_alive` apps start immediately), stop 2 s after unmap unless
  `keep_alive`, `send_close` = `xdg_toplevel.close` → SIGTERM (3 s) → SIGKILL
  (6 s) to the process group, restart policy with backoff (0.5 s doubling,
  gives up after 5 quick exits), placeholder label ("click to start"),
  `PR_SET_PDEATHSIG` so apps die with hyprwidget. Socket mode keeps the
  session alive while forked clients stay connected.
- `hw-ui/src/app.rs` + `hw-lua/src/app_widget.rs` — `W.App` kind; a layer
  window containing one switches `keyboard-mode` none → `on_demand`.

## Limits

- **Single-instance apps** (nautilus, firefox, chromium, gnome-calculator…):
  if an instance already runs on Hyprland, the new one hands over via D-Bus
  and exits (code 0 → placeholder). Use a separate profile/bus:
  `firefox --new-instance -P widget`, `chromium --user-data-dir=…`,
  `dbus-run-session nautilus` (loses the session bus: no portals/trash/gvfs).
- **Memory:** the server is created on the first `W.App`. RSS of a demo
  instance went 108 MB → 234 MB once pavucontrol was embedded (release
  build; RssFile 160 MB, mostly NVIDIA driver mappings from the dmabuf/GL
  setup), plus the app's own process. Far from the <80 MB goal; embedding
  should stay opt-in.
- **Rounded corners** disable offload (GTK can't offload a rounded clip):
  use `rounding = 0` for mpv/games.
- GTK4 apps keep their client-side headerbar (GTK4 has no SSD support);
  maximized/tiled states remove shadows and rounded corners.
- Viewport source crop is not applied; drag and drop not implemented; one
  output (fake 1920x1080) is advertised; X11 apps need
  `xwayland-satellite` (not installed, not done).
- Frame callbacks stop while hidden (`frame_when_hidden = true` paces them at
  ~60 Hz instead).
