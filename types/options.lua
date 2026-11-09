---@meta
-- LuaLS type stubs for the options hyprwidget inherits from Hyprland
-- (names as on https://wiki.hypr.land/Configuring/Variables/).
-- Docs: docs/options.md. Every option of `INHERITED` in
-- crates/hw-core/src/options.rs must appear in `hw.OptionName` (test
-- `docs_cover_api`).

---A value, `"inherit"` (keep Hyprland's), or a function of the Hyprland
---values (`function(h) return h.general.gaps_out * 2 end`).
---@alias hw.Inherit "inherit"|fun(h: table): any

---@alias hw.Gaps integer|string|hw.Inherit `6`, `"6 8"`, `"1 2 3 4"`
---@alias hw.Color string|hw.Inherit `"rgb(808080)"`, `"rgba(33ccffee) rgba(00ff99ee) 45deg"`, `"0xff444444"`, CSS

---`hw.config{...}` / `config.json`: Hyprland option names, nested or flat
---(`["decoration:rounding"] = 16`). Unknown top-level sections are widget
---options.
---@class hw.Config
---@field general? hw.Config.general
---@field decoration? hw.Config.decoration
---@field animations? hw.Config.animations
---@field misc? hw.Config.misc
---@field blur? hw.Config.blur Widget option (not Hyprland's).
---@field [string] any

---@class hw.Config.general
---@field gaps_in? hw.Gaps Default `spacing` of W.Box.
---@field gaps_out? hw.Gaps Default `margin` of W.Window.
---@field border_size? integer|hw.Inherit
---@field col? hw.Config.general.col
---@field [string] any

---@class hw.Config.general.col
---@field active_border? hw.Color
---@field inactive_border? hw.Color

---@class hw.Config.decoration
---@field rounding? integer|hw.Inherit
---@field rounding_power? number|hw.Inherit
---@field active_opacity? number|hw.Inherit
---@field inactive_opacity? number|hw.Inherit
---@field dim_inactive? boolean|hw.Inherit
---@field dim_strength? number|hw.Inherit
---@field shadow? hw.Config.decoration.shadow
---@field blur? hw.Config.decoration.blur
---@field [string] any

---@class hw.Config.decoration.shadow
---@field enabled? boolean|hw.Inherit
---@field range? integer|hw.Inherit
---@field render_power? integer|hw.Inherit
---@field color? hw.Color
---@field offset? number[]|string|hw.Inherit

---@class hw.Config.decoration.blur
---@field enabled? boolean|hw.Inherit
---@field size? integer|hw.Inherit
---@field passes? integer|hw.Inherit
---@field ignore_opacity? boolean|hw.Inherit
---@field popups? boolean|hw.Inherit
---@field popups_ignorealpha? number|hw.Inherit

---@class hw.Config.animations
---@field enabled? boolean|hw.Inherit
---@field bezier? table<string, string> `name = "cubic-bezier(...)"`
---@field animation? table<string, {enabled?: boolean, speed?: number, bezier?: string, style?: string}>

---@class hw.Config.misc
---@field font_family? string|hw.Inherit
---@field [string] any

---@class hw.Config.blur
---@field ignore_alpha? number|false `ignore_alpha` of hyprwidget's blur layer rules (default 0.1).

---Option names accepted by `hw.option` / `hw.set_option` (any other
---Hyprland or widget option name works too).
---@alias hw.OptionName
---| "general:gaps_in"
---| "general:gaps_out"
---| "general:border_size"
---| "general:col.active_border"
---| "general:col.inactive_border"
---| "decoration:rounding"
---| "decoration:rounding_power"
---| "decoration:active_opacity"
---| "decoration:inactive_opacity"
---| "decoration:dim_inactive"
---| "decoration:dim_strength"
---| "decoration:shadow:enabled"
---| "decoration:shadow:range"
---| "decoration:shadow:render_power"
---| "decoration:shadow:color"
---| "decoration:shadow:offset"
---| "decoration:blur:enabled"
---| "decoration:blur:size"
---| "decoration:blur:passes"
---| "decoration:blur:ignore_opacity"
---| "decoration:blur:popups"
---| "decoration:blur:popups_ignorealpha"
---| "animations:enabled"
---| "misc:font_family"
---| "blur:ignore_alpha"

---`hw.opt`: path proxy over the merged options. Leaves are signals;
---calling a branch (`hw.opt.decoration()`) returns it as a table.
---@class hw.Opt
---@operator call: table
---@field general hw.Opt.general
---@field decoration hw.Opt.decoration
---@field animations hw.Opt
---@field misc hw.Opt.misc
---@field [string] hw.Opt|hw.Signal

---@class hw.Opt.general: hw.Opt
---@field gaps_in hw.Signal
---@field gaps_out hw.Signal
---@field border_size hw.Signal
---@field col hw.Opt.general.col

---@class hw.Opt.general.col: hw.Opt
---@field active_border hw.Signal
---@field inactive_border hw.Signal

---@class hw.Opt.decoration: hw.Opt
---@field rounding hw.Signal
---@field rounding_power hw.Signal
---@field active_opacity hw.Signal
---@field inactive_opacity hw.Signal
---@field dim_inactive hw.Signal
---@field dim_strength hw.Signal
---@field shadow hw.Opt.decoration.shadow
---@field blur hw.Opt.decoration.blur

---@class hw.Opt.decoration.shadow: hw.Opt
---@field enabled hw.Signal
---@field range hw.Signal
---@field render_power hw.Signal
---@field color hw.Signal
---@field offset hw.Signal

---@class hw.Opt.decoration.blur: hw.Opt
---@field enabled hw.Signal
---@field size hw.Signal
---@field passes hw.Signal
---@field ignore_opacity hw.Signal
---@field popups hw.Signal
---@field popups_ignorealpha hw.Signal

---@class hw.Opt.misc: hw.Opt
---@field font_family hw.Signal
