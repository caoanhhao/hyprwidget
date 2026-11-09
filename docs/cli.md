# CLI and IPC

```
hyprwidget [-c DIR] [-i INSTANCE] <command>
```

| Option | |
|---|---|
| `-c, --config DIR` | config directory (`run`, `check`, and `option` without a daemon); defaults to `$HYPRWIDGET_CONFIG`, then `$XDG_CONFIG_HOME/hyprwidget` |
| `-i, --instance NAME` | instance name (default `default`, or `$HYPRWIDGET_INSTANCE`) |

| Command | |
|---|---|
| `run` | run the daemon |
| `request <name> [args…]` | call a `hw.on_request(name, fn)` handler or a plugin's `requests`; prints the result |
| `toggle <window>` | show/hide a window by registry name; prints `shown`/`hidden` |
| `reload [css\|lua]` | reload the stylesheet, Lua, or both |
| `eval '<lua>'` | run Lua in the daemon; tried as an expression first (`eval 1+1` → `2`), then as a statement; prints the returned value |
| `list` | windows (`name<TAB>visible\|hidden`) and `request:<name>` |
| `option <name>` | merged value of an option (`decoration:rounding`); without a daemon it is computed from the config |
| `theme [name\|none]` | switch theme; no argument = list them (`*` = active) |
| `check` | check the config (Lua, options, plugin/theme manifests, SCSS) without opening windows; errors are printed as `file:line: …`, exit 1 |
| `inspect` | open the GTK Inspector |
| `quit` | stop the daemon |

The daemon logs to stderr (level `info`, change it with `RUST_LOG`, e.g.
`RUST_LOG=hw_lua=debug,info`). The other commands only print warnings. The GSK
renderer defaults to `vulkan` when `GSK_RENDERER` is unset ([renderer.md](renderer.md)).

## Request handlers

```lua
-- hyprwidget request translate DP-2 "xin chào"
hw.on_request("translate", function(args)        -- args = { "DP-2", "xin chào" }
  local out = hw.exec_async({ "trans", "-b", args[2] })   -- may yield
  return out                                      -- printed to the request command's stdout
end)
```

An error in the handler → the `request` command prints it to stderr and exits 1.
Registering the same name again replaces the old handler (handlers are cleared on
Lua reload). Hyprland keybinds call `hyprwidget request <name> …` instead of the
former `ags request <name> …`.

## IPC protocol

Unix socket `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`. Per connection, the
client sends **one JSON line** `{"cmd": "<command>", "args": ["…"]}`, the daemon
answers with **one JSON line** `{"ok": true|false, "output": "…"}` and closes. `cmd`
is a command name from the table above (`request` takes the handler name as
`args[0]`).

```sh
printf '%s\n' '{"cmd":"toggle","args":["bar-DP-2"]}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprwidget/default.sock
```
