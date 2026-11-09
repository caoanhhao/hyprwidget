# Plugins and themes

## Plugins

A plugin is a `plugins/<name>/` directory inside the config directory:

```
plugins/sysmon/
├── plugin.json      -- manifest (required)
├── init.lua         -- entry, runs in a sandbox
├── lib/format.lua   -- private module: require("lib.format")
└── style.scss       -- (optional) compiled before the user's stylesheet
```

### `plugin.json`

```json
{
  "name": "sysmon",
  "version": "1.0.0",
  "description": "CPU / RAM / GPU meters",
  "author": "…", "license": "MIT",
  "entry": "init.lua",
  "style": "style.scss",
  "permissions": ["exec", "network", "fs"],
  "dependencies": { "other": ">=1.0" },
  "widgets": ["Sysmon", "SysmonMeter"]
}
```

| Key | | |
|---|---|---|
| `name` | required | same as the directory name; letters, digits, `_`, `-` |
| `version` | required | `x.y.z` |
| `description`, `author`, `license` | | informational |
| `entry` | `"init.lua"` | path relative to the plugin |
| `style` | `"style.scss"` (if present) | |
| `permissions` | `[]` | `exec`, `network`, `fs` (table below) |
| `dependencies` | | `{ "name": "version requirement" }` or `["name", …]`; requirements: `*`, `1.2`, `=1.2`, `>=1.2`, `>1.2`, `<2`, `<=2`, `^1.2` (same major), `~1.2` (same minor). The plugin loads after its dependencies; missing/wrong version/cycle → the plugin fails |
| `widgets` | | widgets the plugin provides: placeholders are kept even when `init.lua` fails (showing a `⚠ <plugin>` label, class `plugin-error`) so layouts do not break |

Unknown keys, wrong types, missing files… are reported by `hyprwidget check` as
`plugins/x/plugin.json:<line>: …`.

Choosing plugins in `config.json`: `"plugins": ["a", "b"]` (only these) or
`"plugins": { "enabled": […], "disabled": […] }`; not set = every plugin found.

### `init.lua`

`init.lua` returns a table; every key is optional:

```lua
return {
  widgets = {                       -- W.<Name> and JSON {"type": "<Name>"}
    Sysmon = function(props) return W.Box { … } end,
  },
  services = {                      -- hw.service.<name>: a table, or a function returning one (called lazily)
    sysmon = { summary = summary_signal },
  },
  requests = {                      -- hyprwidget request <name> [args…]
    sysmon = function(args) return "cpu 12%" end,
  },
  setup = function(info)            -- runs after loading (coroutine; info = hw.plugin)
    print("sysmon " .. info.version)
  end,
}
```

- Widget names must not clash with built-in widgets, `hw.define` widgets or other
  plugins; service names must not clash with existing services.
- Plugin errors (syntax, runtime, inside widgets) are logged, reported by
  `hyprwidget check`, shown as a desktop notification (`notify-send`), and do **not**
  stop other plugins or the config.
- Editing a file in `plugins/<name>/` reloads only that plugin and then rebuilds the
  `per_monitor` windows (top-level windows returned from the entry are not rebuilt).

### Sandbox and permissions

Each plugin runs in its own `_ENV`:

- available: basic functions (`assert`, `error`, `ipairs`, `pairs`, `pcall`,
  `select`, `tonumber`, `tostring`, `type`, `xpcall`, `setmetatable`,
  `getmetatable` (does not return the string metatable),
  `rawget/rawset/rawequal/rawlen`, `next`, `load` (text only)), copies of `string`,
  `table`, `math`, `utf8`, `coroutine`, `os.time/date/clock/difftime/getenv`,
  `HOME`, `CONFIG_DIR`, `PLUGIN_DIR`, `W`, `print` (prefixed with `[name]`);
- `require` only loads modules inside the plugin directory;
- `hw` is a proxy of the real `hw`, plus `hw.plugin = { name, version, dir, permissions }`.

| Permission | Unlocks |
|---|---|
| *(none)* | every other `hw.*`: widgets, signals, services, settings, options, timers, `hw.poll` with a **function** |
| `exec` | `hw.exec`, `hw.exec_async`, `hw.subprocess`, `hw.poll` with a command, `io.popen`, `os.execute` |
| `network` | `hw.fetch(url [, cb])` — HTTP(S) GET through `curl` (yields like `exec_async`) |
| `fs` | `io.*` (except `popen`), `os.remove`, `os.rename`, `os.tmpname`, `hw.watch_file` |

Never available to plugins: `hw.quit`, `hw.config`, `hw.layer_rule`, `hw.define`,
`hw.require_all`, `hw.require_custom_dir` (plugins do not change the global config;
widgets are declared through `widgets = {}`).

The sandbox prevents accidental mistakes and limits default permissions; it is not
a security boundary against deliberately malicious code (plugins still run in the
same process). Only install plugins you trust.

### Lua API for plugins

```lua
hw.plugins.list()        -- names of loaded plugins
hw.plugins.get("sysmon") -- { name, version, description, dir, entry, permissions, widgets, error }
hw.plugins.errors()      -- { [name] = message }
```

### Example: the `sysmon` plugin

`examples/demo/plugins/sysmon` is a complete sample plugin that **needs no
permissions**: CPU/RAM/GPU meters built on `hw.service.sysinfo`.

- `plugin.json` declares `widgets: ["Sysmon", "SysmonMeter"]`, `permissions: []`.
- `init.lua`:
  - `require("lib.format")` — the plugin's private module;
  - each meter is `sys.cpu:map(...)` → a % Signal assigned to `W.Label`/`W.Progress`
    (no callbacks);
  - `summary = hw.computed(...)` reads three signals and is used both by the service
    `hw.service.sysmon.summary` and by the request `hyprwidget request sysmon`;
  - `setup(info)` prints the version on load.
- `style.scss` uses theme variables (`$background`, `$primary`) and Hyprland options
  (`$decoration-rounding`, `$general-gaps_in`) — available because plugin styles are
  compiled with `@import "hypr"; @import "hw-theme";`.

Used in the demo: `config/sysmon.lua` creates a (hidden) `sysmon-<monitor>` window
from the JSON layout `layouts/sysmon.json` (`Sysmon` + a `Terminal` running btop):

```sh
hyprwidget -c examples/demo check          # validates the manifests too
hyprwidget toggle sysmon-DP-2
hyprwidget request sysmon                  # → "cpu 7% mem 41% gpu 3%"
hyprwidget eval 'return hw.service.sysmon.summary()'
```

Writing a new plugin: copy the `sysmon` directory, change `name` in `plugin.json`
and the directory name, enable it in `config.json`
(`"plugins": {"enabled": ["sysmon", "mine"]}`), run `hyprwidget check`.

## Theme

A theme is a `themes/<name>/` directory:

```
themes/nord/
├── theme.json      -- name, colors
├── _vars.scss      -- (optional) extra SCSS variables, loaded after the colors
└── init.lua        -- (optional) returns { colors = table_or_signal }, sandboxed like a plugin
```

```json
{ "name": "nord", "version": "1.0.0", "description": "…", "variant": "dark",
  "colors": { "background": "#2e3440", "foreground": "#d8dee9", "primary": "#88c0d0" },
  "source": "wallpaper", "permissions": ["fs"] }
```

Theme colors (`hw.theme.colors`, reactive), later wins:

1. `colors` from `theme.json`;
2. `colors` returned from `init.lua` (a table or a Signal; runs with the theme's
   `permissions`);
3. the **color source** named by `"source"` if registered — e.g. `"wallpaper"` = the
   Material palette from `hw.service.theme`, or a source added with
   `hw.theme.add_source`.

No theme selected (`"theme": null`): `hw.theme.colors` is the `wallpaper` source.

Selecting a theme: `"theme": "nord"` in `config.json`, or at runtime
`hyprwidget theme nord` / `hyprwidget theme none` / `hyprwidget theme` (list, `*` =
active). Runtime switches are not written to `config.json`.

### SCSS

The `_hw-theme.scss` partial (`@import "hw-theme";`), rewritten whenever the theme
or its colors change: `$theme-name`, a `!default` fallback palette (`$background`,
`$foreground`, `$surface`, `$primary`, `$secondary`, `$accent`, `$urgent`),
`$<color>` for each theme color, `$theme-colors` (a map), then the theme's
`_vars.scss`. Plugin styles are always compiled with this partial.

### Lua

```lua
hw.theme.name()                 -- Signal of the theme name (settable: hw.theme.name:set("nord"))
hw.theme.colors.primary         -- Signal of one color
hw.theme.colors()               -- table of every color
hw.theme.palette                -- Signal of the whole color table
hw.theme.set("nord")            -- nil/"none" = no theme
hw.theme.list()                 -- theme names in themes/
hw.theme.get()                  -- { name, colors, source }
hw.theme.sources()              -- names of registered color sources
hw.theme.add_source("mine", { primary = "#ff8800" })   -- table or Signal; lost on Lua reload
hw.theme:set_wallpaper(path)    -- forwarded to hw.service.theme
```

`W.Terminal` takes its colors from `hw.theme.colors` by default.
