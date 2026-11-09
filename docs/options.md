# Options inherited from Hyprland

Widget borders, colors, gaps, rounding, opacity, shadows, blur and animations
**take exactly the values Hyprland is using** by default, and can be overridden
with **the same option names as Hyprland** (as on the
[Hyprland wiki](https://wiki.hypr.land/Configuring/Variables/)).

Precedence (later wins):

1. built-in defaults (Hyprland's own defaults — used when not running under Hyprland);
2. **the running Hyprland**: read through its IPC socket (`getoption`,
   `animations`) at startup, again when Hyprland reports `configreloaded`, and after
   every `hw.set_option`. Since the real values are read, Hyprland's
   `config/custom/*.lua` files count too;
3. hyprwidget's config: `config.json`, then the `hw.config{}` calls in load order;
4. per widget: the `theme = {…}` prop or CSS.

Every option is a Signal: change `rounding` in Hyprland and widgets re-round
immediately.

## Option → effect table

This is the `INHERITED` list in `crates/hw-core/src/options.rs` (the
`docs_cover_api` test checks this table is complete). The "Default" column is
the value without Hyprland.

| Option | Type | Default | Effect in hyprwidget |
|---|---|---|---|
| `general:gaps_in` | gaps | `5` | default `spacing` of every `W.Box` |
| `general:gaps_out` | gaps | `20` | default `margin` of every `W.Window` from the screen edge |
| `general:border_size` | int (px) | `1` | border width of `.hw-panel`, tooltips, popovers, entries… |
| `general:col.active_border` | color/gradient | `0xffffffff` | `.hw-panel:hover` border, accent color `--hw-accent` (selection, focus, sliders…) |
| `general:col.inactive_border` | color/gradient | `0xff444444` | normal border (`--hw-border`) |
| `decoration:rounding` | int (px) | `0` | `border-radius` of panels, cards, tooltips (half), popovers; `W.App` corners |
| `decoration:rounding_power` | float | `2.0` | exported to SCSS only |
| `decoration:active_opacity` | float | `1.0` | `.hw-panel:hover` opacity |
| `decoration:inactive_opacity` | float | `1.0` | `.hw-panel` opacity |
| `decoration:dim_inactive` | bool | `false` | exported to SCSS only |
| `decoration:dim_strength` | float | `0.5` | exported to SCSS only |
| `decoration:shadow:enabled` | bool | `true` | enables `.hw-panel`'s `box-shadow` |
| `decoration:shadow:range` | int (px) | `4` | shadow radius |
| `decoration:shadow:render_power` | int | `3` | exported to SCSS only |
| `decoration:shadow:color` | color | `0xee1a1a1a` | shadow color |
| `decoration:shadow:offset` | vec2 (px) | `[0, 0]` | exported to SCSS only (`0px 0px`) |
| `decoration:blur:enabled` | bool | `true` | registers a `blur` layer rule for every hyprwidget namespace |
| `decoration:blur:size` | int (px) | `8` | exported to SCSS only (Hyprland does the blurring) |
| `decoration:blur:passes` | int | `1` | exported to SCSS only |
| `decoration:blur:ignore_opacity` | bool | `true` | exported to SCSS only |
| `decoration:blur:popups` | bool | `false` | adds a `blur_popups` layer rule |
| `decoration:blur:popups_ignorealpha` | float | `0.2` | exported to SCSS only (layers use `blur:ignore_alpha` below) |
| `animations:enabled` | bool | `true` | exported to SCSS only |
| `misc:font_family` | string | `"Sans"` | default `font-family` |

In addition, Hyprland's **whole `animations` tree** is read (`hyprctl animations`):
`animations:bezier:<name>` = `cubic-bezier(…)`, and for each animation
`animations:animation:<name>:{enabled, speed, bezier, style, duration, easing}` —
`duration` (ms) = `speed` × 100 ms, `easing` = the bezier's `cubic-bezier()`.
Undeclared animations inherit from their parent as in Hyprland (`layersIn` ←
`layers` ← `global`). `Revealer`/`Stack` default to
`duration = animations:animation:layersIn:duration`; default CSS transitions use
`fadeLayersIn`.

**Widget-only** options (sections that are not Hyprland's):

| Option | Default | |
|---|---|---|
| `blur:ignore_alpha` | `0.1` | `ignore_alpha` of the blur layer rule (`false` = not set). Hyprland's `popups_ignorealpha` is not used because its value (usually ~0.97) makes translucent panels lose their blur |

Hyprland's sections (`general`, `decoration`, `animations`, `input`, `gestures`,
`group`, `misc`, `binds`, `xwayland`, `opengl`, `render`, `cursor`, `ecosystem`,
`experimental`, `debug`, `dwindle`, `master`): `hyprwidget check` reports options
with the wrong type. Any other top-level key is a widget-only option, named as you
like (read with `hw.option`).

Hyprland color formats (`rgb(…)`, `rgba(…)`, `0xAARRGGBB`, multi-color gradients
with an angle `"rgba(33ccffee) rgba(00ff99ee) 45deg"`) are converted to CSS
colors; gradients become `linear-gradient(…)`.

## `hw.config`

```lua
hw.config({
  general = {
    gaps_out = 10,                       -- a constant
    gaps_in = "inherit",                 -- (default) follow Hyprland; cancels an earlier constant
    col = { active_border = "rgb(808080)" },
  },
  decoration = {
    rounding = function(h) return h.decoration.rounding * 2 end,   -- computed from Hyprland's value
    blur = { enabled = true },
  },
  ["decoration:shadow:range"] = 8,       -- flat keys work too
  blur = { ignore_alpha = 0.3 },         -- widget-only option
})
```

- Can be called many times; tables are deep-merged in load order (like
  Hyprland's `hl.config`).
- A `function(h)` receives the option tree of layers 1+2 (`h.general.gaps_in`…)
  and is recomputed when Hyprland changes.
- Values of the wrong type are reported with `file:line` (`hyprwidget check`).
- `config.json` takes the same structure: `{"general": {"gaps_out": 10}}` or
  `{"decoration:rounding": 16}`.

## Reading from Lua

```lua
hw.option("decoration:rounding")        -- Signal<number>
hw.option("general:col.active_border")  -- Signal<string> (CSS)
hw.opt.general.gaps_out                 -- same value, accessed by path (leaves are Signals)
hw.opt.decoration()                     -- the whole current branch as a table
W.Box { spacing = hw.opt.general.gaps_in, … }
```

Per-side gaps are a `{top, right, bottom, left}` table. The command
`hyprwidget option decoration:rounding` prints the merged value (it also works
without a daemon).

## SCSS / CSS

hyprwidget generates a `_hypr.scss` partial (`@import "hypr";`) and updates it
live. Variable names keep the option names, with `:` and `.` replaced by `-`:

```scss
// generated
$general-gaps_in: 4px;
$general-col-active_border: rgba(128,128,128,0.6);     // a color or linear-gradient(...)
$general-col-active_border-color: rgba(128,128,128,0.6); // the first color (usable as border-color)
$decoration-rounding: 16px;
$animations-animation-layersIn-duration: 400ms;
$animations-animation-layersIn-easing: cubic-bezier(0, 1, 0, 1);
:root { --decoration-rounding: 16px; --general-gaps_out: 6px; … }

// your stylesheet
@import "hypr";
.panel { border-radius: $decoration-rounding; border: $general-border_size solid $general-col-inactive_border-color; }
```

A widget's `theme` prop generates CSS custom properties for that widget only:
`theme = { decoration = { rounding = 0 } }` → `--decoration-rounding: 0px`.

## `config/custom/` and `hw.set_option`

`hw.set_option(name, value [, target])` is what ControlPanel/UserPanel call when
the user changes an option; it follows AGS' convention of writing into
`hypr/config/custom/`:

| target | Writes to | Applies by |
|---|---|---|
| `"hypr"` | `<hypr_custom_dir>/<name>.lua`: `hl.config({ decoration = { rounding = 16 } })` | `hyprctl eval` of that same line (Hyprland 0.57 no longer has `keyword`), then re-reading the option |
| `"widget"` | `<config>/config/custom/<name>.lua`: `hw.config({ … })` | merging it into hyprwidget's option layer at once |
| `"auto"` (default) | `"hypr"` if Hyprland knows the option, otherwise `"widget"` | |

Returns the target used. `value`: boolean, number, string or list.
`hypr_custom_dir` defaults to `~/.config/hypr/config/custom` (change it in
`config.json`); `require_custom_dir()` in `hyprwidget.lua` loads the widget's
`config/custom/*.lua` files again at startup.

```lua
hw.set_option("decoration:rounding", 12)            -- → Hyprland
hw.set_option("blur:ignore_alpha", 0.2)             -- → widget
```

Calling Hyprland directly: `hw.service.hyprland:dispatch(...)`, `:message(...)` — see
[services.md](services.md#hyprland).
