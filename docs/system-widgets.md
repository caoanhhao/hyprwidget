# Options, night light, wallpaper palette, key presses (M4b)

## `hw.set_option(name, value [, target])`

What the settings UI calls when an option changes (PLAN.md §3.7). Writes
`config/custom/<name>.lua` in the format AGS used and applies it live:

| target | file | live |
|---|---|---|
| `"hypr"` | `<hypr_custom_dir>/<name>.lua`: `hl.config({ decoration = { rounding = 16 } })` | `hyprctl eval` of that line, then Hyprland options are re-read |
| `"widget"` | `<config>/config/custom/<name>.lua`: `hw.config({ … })` | merged into the user option layer |
| `"auto"` (default) | Hyprland's if the running Hyprland knows the name | |

`hypr_custom_dir` comes from `config.json` (default
`~/.config/hypr/config/custom`); point it elsewhere to test without touching
the real Hyprland config. Hyprland 0.57 has no `hyprctl keyword` (it answers
"unknown request"), hence `eval`.

## `hw.service.hyprsunset`

Talks to hyprsunset's socket (`hyprctl hyprsunset` does the same). Props
`available`, `temperature` (K), `gamma` (%), `identity`; methods
`set_temperature(k)`, `set_gamma(pct)`, `set_identity()`. hyprsunset must be
running (it is started by `custom/autostart.lua`).

Brightness of the external DP-2 goes through `hw.service.brightness`, which
needs `ddcutil` (not installed: the slider stays hidden). To enable it:
`sudo pacman -S ddcutil`, load `i2c-dev` and be in the `i2c` group.

## `hw.theme` / `hw.service.theme`

Material You palette computed in Rust (`material-colors`) from an image:
`hw.theme:set_wallpaper(path, "auto"|"dark"|"light")`, `hw.theme.colors()`
(every Material role → `#rrggbb`), `mode`, `source`, `dominant`,
`hw.theme:from_image(path)` (replaces `get-image-color.sh`). The generated
partial `_material.scss` has `$md-<role>` and `--md-<role>`; it is rewritten
and the CSS recompiled whenever the palette changes. Videos use their first
second's frame (`ffmpeg`). `auto` = light when the image's average brightness
is above 128, like hypr's `detect_variant_from_wallpaper`.

## `hw.service.keystrokes`

Reads key presses from `/dev/input/event*` (keyboards and media-key devices
from `/proc/bus/input/devices`), maps them with xkbcommon using Hyprland's
`input:kb_layout`/`kb_variant`. Nothing is read until `enable(true)`.

Permissions: event devices are `root:input 0660`, so the user must be in the
`input` group:

```sh
sudo usermod -aG input "$USER"   # then log out and in again
```

Otherwise `available` is false and `error` says what is missing. Being in
`input` lets every program of the user read all input devices (keylogging);
the visualizer only reads while it is enabled.
