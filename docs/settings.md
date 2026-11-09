# Settings (`hw.settings`)

A persistent settings store, **compatible with AGS' `settings.json`**: the file is
a nested JSON object; a leaf is either a plain value or an `AGSSetting` object
`{ name, value, type, min?, max? }`. Keys are dotted paths
(`"hyprland.decoration.rounding"`) and always refer to the **value** (an
AGSSetting's `value`). Writes are debounced by 500 ms and atomic (temp file +
rename).

File: `settings_file` in `config.json`, default
`<config>/cache/settings/settings.json`.

```lua
hw.settings:get("bar.lock")              -- current value (table/number/…)
hw.settings:set("bar.lock", true)        -- returns the stored value (type-checked / clamped to min..max)
hw.settings:toggle("notifications.dnd")  -- bool → returns the new value
hw.settings:meta("hyprland.decoration.rounding")  -- { name, value, type, min, max } or nil
hw.settings:bind("bar.lock")             -- writable Signal (= hw.bind(hw.settings, "bar.lock"))
hw.settings["bar.lock"]                  -- the same Signal

W.Switch {
  active = hw.settings["bar.lock"],
  on_change = function(_, on) hw.settings:set("bar.lock", on) end,
}
```

- For AGSSetting keys, `set` checks the `type`: `int` (rounded, then clamped to
  `min`..`max`), `float` (clamped to `min`..`max`), `bool`/`boolean`,
  `string`/`str`; a wrong type raises an error.
- Setting a missing key creates the intermediate objects.
- The Signal of a parent key (`hw.settings.bar`) changes when any child key
  changes.
- Settings survive Lua reloads; several widgets bound to the same key share one
  Signal.
- In JSON layouts: `{"$bind": "settings.bar.lock"}` (see [widgets.md](widgets.md#layout-json)).

The demo (`examples/demo/config/bar/defaults.lua`, `config/panels/defaults.lua`)
seeds AGS' defaults for missing keys, so `settings_file` can point at an old
`~/.config/ags/cache/settings/settings.json`.
