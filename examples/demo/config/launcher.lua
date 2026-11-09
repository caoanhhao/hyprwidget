-- App launcher: port of ~/.config/ags/widgets/applauncher/{AppLauncher,
-- QuickApps,AppHistory}.tsx and the launcher requests of app.tsx.
--
--   hyprwidget toggle app-launcher-DP-2
--   hyprwidget request translate|clipboard|emojis|notes DP-2   (prefilled)
--
-- Keys in the entry: Up/Down select, Enter launches the selection,
-- Escape hides; Tab moves the focus through the buttons (like AGS).
local W = hw.widget
local util = require("lib.util")
local U = require("lib.launcher_utils")

local apps = hw.service.apps
local mpris = hw.service.mpris

local MAX_ITEMS = U.MAX_ITEMS
local MAX_HISTORY = 10
local DEBOUNCE = 200
local QUICK_APP_HISTORY = util.CACHE_DIR .. "/launcher/quick-app-history.json"

--- Starts `cmd` through Hyprland (so apps are its children and get its
--- rules), or detached with setsid outside Hyprland.
local function spawn(cmd)
  local hypr = hw.service.hyprland
  if hypr.available:peek() then
    local ok, err = pcall(function()
      hypr:dispatch("hl.dsp.exec_cmd(" .. string.format("%q", cmd) .. ")")
    end)
    if ok then
      return
    end
    print("launcher: dispatch failed: " .. tostring(err))
  end
  util.run({ "setsid", "-f", "sh", "-c", cmd })
end

local FILE_MANAGERS = {
  nautilus = "nautilus",
  thunar = "thunar",
  dolphin = "dolphin",
  nemo = "nemo",
  pcmanfm = "pcmanfm",
  ranger = "kitty ranger",
}

local paths = require("lib.paths")

local function system_theme(light)
  local script = paths.get("system_theme")
  if not script then
    print("launcher: paths.system_theme is not set")
    return
  end
  util.run({ "bash", "-c", script .. " switch " .. (light and "light" or "dark") })
end

-- `> …` (constants/app.constants.ts customApps)
local CUSTOM_APPS = {
  { name = "Light Theme", icon = "\u{F185}", launch = function() system_theme(true) end },
  { name = "Dark Theme", icon = "\u{F186}", launch = function() system_theme(false) end },
  {
    name = "System Sleep",
    icon = "\u{F186}",
    launch = function()
      util.run({ "bash", "-c", paths.suspend or "systemctl suspend" })
    end,
  },
  { name = "System Restart", icon = "\u{F0709}", launch = function() util.run({ "reboot" }) end },
  { name = "System Shutdown", icon = "\u{F011}", launch = function() util.run({ "shutdown", "now" }) end },
}

-- Right column (quickApps), reordered by use.
local QUICK_APPS = {
  {
    name = "Keybinds",
    icon = "\u{F11C}",
    description = "View or edit your Hyprland keybinds",
    launch = function(ctx)
      local panel = hw.window.get("left-panel-" .. ctx.monitor)
      if not panel then
        print("Left panel for monitor " .. ctx.monitor .. " not found.")
        return
      end
      hw.settings:set("leftPanel.widget", "Keybinds")
      panel:show()
    end,
  },
  {
    name = "Browser",
    icon = "\u{F269}",
    description = "Open your default web browser",
    launch = function()
      util.run({ "xdg-open", "http://www.google.com" })
    end,
  },
  {
    name = "Terminal",
    icon = "\u{F120}",
    description = "Open a new terminal window",
    launch = function()
      spawn("kitty")
    end,
  },
  {
    name = "Files",
    icon = "\u{F07C}",
    description = "Open your file manager",
    launch = function()
      local fm = hw.settings:get("fileManager") or "nautilus"
      spawn(FILE_MANAGERS[fm] or "nautilus")
    end,
  },
  {
    name = "Calculator",
    icon = "\u{F1EC}",
    description = "Open the calculator",
    launch = function()
      spawn("kitty bc")
    end,
  },
  {
    name = "Text Editor",
    icon = "\u{F121}",
    description = "Open your default text editor",
    launch = function()
      spawn("code")
    end,
  },
}

local HELP = {
  { command = "cb ...", description = "clipboard history (text/html/image)", keys = { "SUPER", "SHIFT", "v" } },
  { command = "note ...", description = "add/list/edit/remove notes", keys = { "SUPER", "SHIFT", "n" } },
  { command = "emoji ...", description = "search emojis", keys = { "SUPER", "." } },
  { command = "... ...", description = "open with argument" },
  { command = "translate .. > ..", description = "translate into (en,fr,es,de,pt,ru,ar...)" },
  { command = "... .com OR https://...", description = "open link" },
  { command = "..*/+-..", description = "arithmetics" },
  { command = "100c to f / 10kg in lb", description = "unit conversion (temp/weight/length/volume/speed)" },
}

-- Quick app order, shared by all monitors and persisted.
local quick_order = hw.state({})
do
  local known, list = {}, {}
  for _, a in ipairs(QUICK_APPS) do
    known[a.name] = true
  end
  for _, name in ipairs(util.read_json(QUICK_APP_HISTORY, {})) do
    if type(name) == "string" and known[name] then
      table.insert(list, name)
    end
  end
  quick_order:set(list)
end

local ordered_quick_apps = quick_order:map(function(order)
  local rank = {}
  for i, name in ipairs(order) do
    rank[name] = i
  end
  local list = {}
  for i, a in ipairs(QUICK_APPS) do
    table.insert(list, { app = a, rank = rank[a.name] or (1000 + i) })
  end
  table.sort(list, function(x, y)
    return x.rank < y.rank
  end)
  local out = {}
  for _, x in ipairs(list) do
    table.insert(out, x.app)
  end
  return out
end)

local function touch_quick_app(name)
  local list = { name }
  for _, n in ipairs(quick_order:peek()) do
    if n ~= name then
      table.insert(list, n)
    end
  end
  quick_order:set(list)
  util.write_json(QUICK_APP_HISTORY, list)
end

local function app_result(app, arg)
  return {
    name = app.name,
    icon = app.icon,
    description = app.description or app.generic_name,
    type = "app",
    arg = arg,
    launch = function()
      apps:launch(app.id)
    end,
  }
end

-- Launcher history: hw.service.apps keeps the launch order.
local history = apps.history:map(function(list)
  local out = {}
  for i = 1, math.min(MAX_HISTORY, #(list or {})) do
    table.insert(out, list[i])
  end
  return out
end)

--- Results for `text` (may yield: call inside hw.async).
local function search(text, ctx)
  if util.trim(text) == "" then
    return {}
  end

  local cb = U.parse_clipboard(text)
  if cb then
    return U.clipboard_results(cb)
  end

  local note = U.parse_note(text)
  if note then
    return U.note_results(note, MAX_ITEMS, ctx)
  end

  if U.is_conversion(text) then
    local r = U.conversion_results(text)
    if r then
      return r
    end
  end

  local args = {}
  for word in text:gmatch("%S+") do
    table.insert(args, word)
  end

  if args[1]:find(">", 1, true) then
    local filter = util.trim(text:gsub(">", "")):lower()
    local out = {}
    for _, a in ipairs(CUSTOM_APPS) do
      if a.name:lower():find(filter, 1, true) then
        table.insert(out, a)
      end
    end
    return out
  end

  local translate = U.parse_translate(text)
  if translate then
    return U.translate_results(translate)
  end
  if util.trim(text):lower() == "translate" then
    return {
      { name = "Type the text to translate", description = "translate hello > fr (default: settings translate.lang)", close_on_launch = false, launch = function() end },
    }
  end

  local emoji = U.parse_emoji(text)
  if emoji then
    return U.emoji_results(emoji)
  end

  if U.is_arithmetic(text) then
    local r = U.arithmetic_results(text)
    if r then
      return r
    end
  end

  if U.is_url(text) then
    return U.url_results(text)
  end

  local query = table.remove(args, 1)
  local arg = table.concat(args, " ")
  local out = {}
  for _, app in ipairs(apps:query(query, MAX_ITEMS)) do
    table.insert(out, app_result(app, arg))
  end
  if #out == 0 then
    out = {
      {
        name = "Try " .. text .. " in terminal",
        icon = "\u{F02D6}",
        launch = function()
          spawn("kitty bash -c " .. util.sh_quote(text))
        end,
      },
    }
  end
  return out
end

local function escape_markup(s)
  return (tostring(s or ""):gsub("&", "&amp;"):gsub("<", "&lt;"):gsub(">", "&gt;"))
end

local function KeyBind(keys)
  local buttons = {}
  for _, k in ipairs(keys) do
    table.insert(buttons, W.Button({ class = "binding", W.Label({ label = k }) }))
  end
  return W.Box({ class = "keybind", spacing = 5, valign = "center", table.unpack(buttons) })
end

--- One result row (AGS AppButton). `selected` is a boolean signal or nil.
local function AppButton(r, ctx, on_launch, selected)
  local action_buttons = {}
  for _, a in ipairs(r.actions or {}) do
    table.insert(
      action_buttons,
      W.Button({
        class = a.class or "",
        tooltip = a.tooltip or a.label,
        on_click = function()
          a.on_click(ctx)
        end,
        W.Label({ label = a.label }),
      })
    )
  end

  local tooltip = escape_markup(r.name)
  if r.description and r.description ~= "" then
    tooltip = tooltip .. "\n<b>" .. escape_markup(r.description) .. "</b>"
  end

  return W.Box({
    class = "app-row",
    spacing = 5,
    hexpand = true,
    W.Button({
      hexpand = true,
      class = selected and selected:map(function(s)
        return s and "checked" or ""
      end) or "",
      on_click = function()
        on_launch(r)
      end,
      W.Box({
        spacing = 10,
        hexpand = true,
        tooltip_markup = tooltip,
        W.Image({ visible = r.type == "app", icon = r.type == "app" and (r.icon or "application-x-executable") or nil }),
        W.Box({
          vertical = true,
          spacing = 5,
          hexpand = true,
          W.Box({
            W.Label({ xalign = 0, wrap = true, wrap_mode = "word_char", label = r.name or "" }),
            W.Label({
              class = "argument",
              hexpand = true,
              xalign = 0,
              label = (r.arg and r.arg ~= "") and (" " .. r.arg) or "",
            }),
          }),
          W.Label({
            class = "description",
            visible = (r.description or "") ~= "",
            xalign = 0,
            ellipsize = "end",
            label = r.description or "",
          }),
        }),
      }),
    }),
    W.Box({ class = "app-actions", spacing = 6, visible = #action_buttons > 0, table.unpack(action_buttons) }),
  })
end

--- Compact media player for the left column (the full Player is M4).
local function LauncherPlayer()
  local player = mpris.players:map(function(players)
    players = players or {}
    for _, p in ipairs(players) do
      if p.playing then
        return p
      end
    end
    return players[1]
  end)

  return hw.With(player, function(p)
    if not p then
      return W.Box({})
    end
    return W.Box({
      class = "player launcher-player",
      vertical = true,
      spacing = 10,
      width_request = 300,
      W.Image({
        class = "cover",
        file = p.cover_art,
        icon = (not p.cover_art) and "audio-x-generic" or nil,
        pixel_size = 260,
      }),
      W.Label({ class = "title", label = p.title or "Unknown Track", ellipsize = "end", xalign = 0 }),
      W.Label({ class = "artist", label = p.artist or "Unknown Artist", ellipsize = "end", xalign = 0 }),
      W.Box({
        class = "controls",
        halign = "center",
        spacing = 10,
        W.Button({
          label = "\u{F04AE}",
          sensitive = p.can_go_previous,
          on_click = function()
            mpris:previous(p.bus_name)
          end,
        }),
        W.Button({
          class = "play-pause",
          label = p.playing and "\u{F03E4}" or "\u{F040A}",
          on_click = function()
            mpris:play_pause(p.bus_name)
          end,
        }),
        W.Button({
          label = "\u{F04AD}",
          sensitive = p.can_go_next,
          on_click = function()
            mpris:next(p.bus_name)
          end,
        }),
      }),
    })
  end)
end

-- Per-monitor launcher handles, for the requests.
local launchers = {}

local function focused_monitor()
  local m = hw.service.hyprland.focused_monitor:peek()
  return m and m.name
end

local function open_prefilled(args, text)
  local name = args[1] or focused_monitor()
  local l = launchers[name]
  if not l then
    for _, any in pairs(launchers) do
      l = any
      break
    end
  end
  if l then
    l.show()
    l.prefill(text)
  end
end

for request, prefix in pairs({ translate = "translate ", clipboard = "cb ", emojis = "emoji ", notes = "note " }) do
  hw.on_request(request, function(args)
    open_prefilled(args, prefix)
    local what = ({
      translate = "Translate widget",
      clipboard = "Clipboard widget",
      emojis = "Emoji picker",
      notes = "Notes widget",
    })[request]
    return what .. " opened."
  end)
end

hw.per_monitor(function(monitor)
  local results = hw.state({})
  local selected = hw.state(1)
  local window_name = "app-launcher-" .. monitor.name
  local entry -- LuaWidget
  local debounce
  local generation = 0

  local function set_results(list)
    generation = generation + 1
    for i, r in ipairs(list) do
      r._key = generation .. ":" .. i
    end
    selected:set(1)
    results:set(list)
  end

  local function hide()
    local win = hw.window.get(window_name)
    if win then
      win:hide()
    end
  end

  local function clear()
    if entry then
      entry:set("text", "")
    end
    set_results({})
  end

  local function prefill(text)
    if not entry then
      return
    end
    entry:set("text", text)
    -- grab_focus selects the whole text; moving the cursor afterwards
    -- clears the selection so typing appends.
    entry:grab_focus()
    entry:set("position", -1)
  end

  local ctx = { monitor = monitor.name, prefill = prefill }

  local function launch(r)
    local ok, err = pcall(r.launch, ctx)
    if not ok then
      util.notify("Error", tostring(err))
    end
    if r.close_on_launch == false then
      return
    end
    hide()
    clear()
  end

  local function on_text(text)
    if debounce then
      debounce:cancel()
    end
    debounce = hw.timeout(DEBOUNCE, function()
      debounce = nil
      local gen = generation
      hw.async(function()
        local ok, list = pcall(search, text, ctx)
        -- Drop results of a query that was superseded while it ran.
        if gen ~= generation or (entry and entry:get("text") ~= text) then
          return
        end
        if not ok then
          util.notify("Error", tostring(list))
          return
        end
        set_results(list)
      end)
    end)
  end

  local function move(delta)
    local n = #results:peek()
    if n == 0 then
      return
    end
    selected:set((selected:peek() - 1 + delta) % n + 1)
  end

  launchers[monitor.name] = {
    show = function()
      local win = hw.window.get(window_name)
      if win then
        win:show()
      end
    end,
    prefill = prefill,
  }

  local help_rows = {}
  for _, tip in ipairs(HELP) do
    table.insert(
      help_rows,
      W.Box({
        spacing = 10,
        W.Box({
          vertical = true,
          spacing = 5,
          hexpand = true,
          W.Label({ class = "command", label = tip.command, wrap = true, xalign = 0 }),
          W.Label({ class = "description", label = tip.description, wrap = true, xalign = 0 }),
        }),
        tip.keys and KeyBind(tip.keys) or nil,
      })
    )
  end

  local has_results = results:map(function(r)
    return #r > 0
  end)

  return W.Window({
    name = window_name,
    namespace = "app-launcher",
    monitor = monitor,
    layer = "top",
    anchor = { "top" },
    keymode = "exclusive",
    margin = hw.opt.general.gaps_out,
    visible = false,
    on_show = function()
      apps:reload()
      selected:set(1)
      if entry then
        entry:grab_focus()
      end
    end,
    on_key = function(_, key)
      if key == "Escape" then
        hide()
        return true
      end
    end,

    W.Box({
      class = "app-launcher",
      spacing = 10,
      W.Box({ class = "left", LauncherPlayer() }),
      W.Box({
        class = "center",
        hexpand = true,
        vertical = true,
        spacing = 10,
        width_request = 500,
        W.Entry({
          hexpand = true,
          placeholder = "Search apps and utilities",
          tooltip_markup = "Search apps and utilities\n<b>Enter</b> launch the selected result\n<b>Up/Down</b> select",
          setup = function(self)
            entry = self
          end,
          on_change = function(_, text)
            on_text(text)
          end,
          -- Enter (GtkText turns it into `activate`).
          on_activate = function()
            local r = results:peek()[selected:peek()]
            if r then
              launch(r)
            end
          end,
          on_key = function(_, key)
            if key == "Down" then
              move(1)
              return true
            elseif key == "Up" then
              move(-1)
              return true
            elseif key == "Escape" then
              hide()
              return true
            end
          end,
        }),
        W.Scrolled({
          hexpand = true,
          vexpand = true,
          hscroll = "never",
          W.Box({
            vertical = true,
            W.Box({
              class = "help",
              vertical = true,
              spacing = 10,
              visible = has_results:map(function(h)
                return not h
              end),
              table.unpack(help_rows),
            }),
            util.For(results, function(r, i)
              return AppButton(r, ctx, launch, selected:map(function(s)
                return s == i
              end))
            end, function(r)
              return r._key
            end, { class = "results", vertical = true, spacing = 10, visible = has_results }),
          }),
        }),
      }),
      W.Box({
        class = "right",
        vertical = true,
        spacing = 10,
        width_request = 300,
        W.Scrolled({
          vexpand = true,
          hexpand = true,
          hscroll = "never",
          util.For(ordered_quick_apps, function(a)
            return W.Button({
              hexpand = true,
              class = "quick-app",
              on_click = function()
                touch_quick_app(a.name)
                a.launch(ctx)
                hide()
              end,
              W.Box({
                spacing = 5,
                W.Label({ width_request = 24, label = a.icon }),
                W.Box({
                  vertical = true,
                  spacing = 5,
                  W.Label({ label = a.name, xalign = 0 }),
                  W.Label({ class = "description", label = a.description or "", xalign = 0 }),
                }),
              }),
            })
          end, function(a)
            return a.name
          end, { class = "quick-apps results", vertical = true, spacing = 5 }),
        }),
        W.Scrolled({
          vexpand = true,
          hscroll = "never",
          W.Box({
            class = "history",
            valign = "start",
            vertical = true,
            spacing = 5,
            W.Label({
              label = "Empty History",
              visible = history:map(function(h)
                return #h == 0
              end),
            }),
            util.For(history, function(app)
              return AppButton(app_result(app), ctx, launch)
            end, function(app)
              return app.id
            end, { vertical = true, spacing = 5 }),
          }),
        }),
      }),
    }),
  })
end)
