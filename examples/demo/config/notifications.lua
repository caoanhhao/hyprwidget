-- Notifications: port of ~/.config/ags/widgets/NotificationPopups.tsx,
-- rightPanel/components/{Notification,NotificationHistory}.tsx and the DND
-- toggle of ControlPanel.tsx, on top of hw.service.notifd.
--
--   W.Notification{ n = n, popup = false }  one notification card
--   W.NotificationHistory{}                 stacked history (right panel)
--   W.DndToggle{}                           do-not-disturb button (bar)
--
--   hyprwidget toggle notification-history-DP-2
--   hyprwidget request delete-notification <id>
local W = hw.widget
local util = require("lib.util")

local notifd = hw.service.notifd

-- Popups leave the screen after this long; they stay in the history.
local TIMEOUT_DELAY = 4000
local TRANSITION = util.TRANSITION
local MAX_HISTORY = 50

local ICON = {
  expand = "\u{F107}",
  collapse = "\u{F106}",
  copy = "\u{F0C5}",
  close = "\u{F48E}",
  dnd_on = "\u{F1F7}",
  dnd_off = "\u{F0A2}",
}

local function is_file(s)
  return type(s) == "string" and (s:sub(1, 1) == "/" or s:sub(1, 7) == "file://")
end

local function file_path(s)
  return (s:gsub("^file://", ""))
end

-- Same priority as AGS getNotificationIcon: app_icon, image, desktop entry,
-- then a generic icon by urgency.
local function notification_icon(n)
  for _, src in ipairs({ n.app_icon, n.image }) do
    if src and src ~= "" then
      if is_file(src) then
        return W.Image({ class = "image", file = file_path(src), pixel_size = 48 })
      end
      return W.Image({ class = "icon", icon = src })
    end
  end
  if n.desktop_entry and n.desktop_entry ~= "" then
    return W.Image({ class = "icon", icon = n.desktop_entry })
  end
  if n.urgency == "critical" then
    return W.Image({ class = "icon", icon = "dialog-warning-symbolic" })
  end
  return W.Image({ class = "icon", icon = "dialog-information-symbolic" })
end

-- Summaries are shown as markup (like AGS); escape stray `&` so text
-- such as "Tom & Jerry" still renders.
local function markup(s)
  return (tostring(s or ""):gsub("&([#%w]*;?)", function(rest)
    if rest:match("^#?%w+;$") then
      return "&" .. rest
    end
    return "&amp;" .. rest
  end))
end

local function copy_content(n)
  for _, src in ipairs({ n.app_icon, n.image }) do
    if is_file(src) then
      local path = file_path(src)
      hw.exec_async({ "sh", "-c", 'wl-copy --type image/png < "$1"', "sh", path }, function(_, err)
        if err then
          util.notify("Error", err)
        else
          util.notify("Copied", path)
        end
      end)
      return
    end
  end
  local content = (n.body ~= "" and n.body) or n.app_name
  if content and content ~= "" then
    util.copy(content)
  end
end

--- One notification card.
--- props: n (notification table), popup (slides in), closing (signal of
--- {id = true} for popups being removed by the popup list).
hw.define("Notification", function(props)
  local n, closing = props.n, props.closing
  local ellipsized = hw.state(true)
  local shown = hw.state(not props.popup)

  local reveal = hw.computed(function()
    return shown() and not (closing and closing()[n.id])
  end)

  local function close(dismiss)
    shown:set(false)
    hw.timeout(TRANSITION, function()
      if dismiss then
        notifd:dismiss(n.id)
      end
    end)
  end

  local actions = {}
  for _, a in ipairs(n.actions or {}) do
    local label = a.label or a.id or ""
    table.insert(
      actions,
      W.Button({
        hexpand = true,
        on_click = function()
          notifd:invoke(n.id, a.id)
        end,
        W.Label({ label = label:match("([^:]*)$") }),
      })
    )
  end

  local top_bar = W.Box({
    class = "top-bar",
    spacing = 5,
    W.Box({
      spacing = 5,
      W.Label({ class = "app-name", label = n.app_name or "", ellipsize = "end" }),
      W.Button({
        class = "copy",
        label = ICON.copy,
        on_click = function()
          copy_content(n)
        end,
      }),
    }),
    W.Box({ class = "separator", hexpand = true }),
    W.Box({
      class = "quick-actions",
      W.Button({
        class = "expand",
        label = ellipsized:map(function(e)
          return e and ICON.expand or ICON.collapse
        end),
        on_click = function()
          ellipsized:set(not ellipsized:peek())
        end,
      }),
      W.Button({
        class = "close",
        label = ICON.close,
        on_click = function()
          close(true)
        end,
      }),
    }),
    W.Label({ class = "time", halign = "end", label = os.date("%H:%M", n.time or os.time()) }),
  })

  local card = W.Box({
    class = "notification " .. (n.urgency or "normal"),
    hexpand = true,
    vertical = true,
    top_bar,
    W.Box({
      class = "content",
      spacing = 10,
      W.Box({ class = "icon", valign = "start", halign = "start", notification_icon(n) }),
      W.Box({
        vertical = true,
        spacing = 5,
        W.Label({
          class = "title",
          xalign = 0,
          max_width_chars = 24,
          wrap = true,
          use_markup = true,
          label = markup(n.summary),
        }),
        W.Label({
          class = "body",
          visible = (n.body or "") ~= "",
          label = n.body or "",
          xalign = 0,
          wrap = true,
          wrap_mode = "word_char",
          max_width_chars = 10,
          vexpand = false,
          ellipsize = ellipsized:map(function(e)
            return e and "end" or "none"
          end),
          single_line_mode = ellipsized,
        }),
      }),
    }),
    W.Box({ class = "actions", visible = #actions > 0, table.unpack(actions) }),
  })

  return W.Box({
    class = "notification-parent",
    W.Revealer({
      transition = "swing_down",
      duration = TRANSITION,
      reveal = reveal,
      hexpand = true,
      setup = function()
        if props.popup then
          hw.timeout(1, function()
            shown:set(true)
          end)
        end
      end,
      card,
    }),
  })
end)

-- Popups (shared by every monitor): newest first. `key` changes when a
-- notification is replaced so its card is rebuilt.
local popups = hw.state({})
local closing = hw.state({})
local version = 0
local sounded = {}

local function without(list, id)
  local out = {}
  for _, p in ipairs(list) do
    if p.n.id ~= id then
      table.insert(out, p)
    end
  end
  return out
end

local function set_closing(id, on)
  local c = {}
  for k, v in pairs(closing:peek()) do
    c[k] = v
  end
  c[id] = on or nil
  closing:set(c)
end

local function close_popup(id, key)
  for _, p in ipairs(popups:peek()) do
    if p.n.id == id and (key == nil or p.key == key) then
      if closing:peek()[id] then
        return
      end
      set_closing(id, true)
      hw.timeout(TRANSITION, function()
        set_closing(id, false)
        popups:set(without(popups:peek(), id))
      end)
      return
    end
  end
end

local function play_sound()
  if hw.settings:get("notifications.sound") == false then
    return
  end
  local path = hw.settings:get("notifications.soundPath")
    or "/usr/share/sounds/freedesktop/stereo/message-new-instant.oga"
  if not util.file_exists(path) then
    print("Notification sound file not found: " .. path)
    return
  end
  util.run({ "paplay", path })
end

-- DND "ping": the toggle flashes when a notification arrives silently.
local dnd_ping = hw.state(false)
local ping_timer

notifd:on_notified(function(n)
  if notifd.dnd:peek() then
    dnd_ping:set(true)
    if ping_timer then
      ping_timer:cancel()
    end
    ping_timer = hw.timeout(600, function()
      dnd_ping:set(false)
    end)
    return
  end

  if not sounded[n.id] then
    sounded[n.id] = true
    hw.timeout(TIMEOUT_DELAY * 2, function()
      sounded[n.id] = nil
    end)
    play_sound()
  end

  version = version + 1
  local key = n.id .. ":" .. version
  local list = { { n = n, key = key } }
  for _, p in ipairs(without(popups:peek(), n.id)) do
    table.insert(list, p)
  end
  set_closing(n.id, false)
  popups:set(list)

  hw.timeout(TIMEOUT_DELAY, function()
    close_popup(n.id, key)
  end)
end)

-- Dismissed from the history or closed by the sender.
notifd:on_resolved(function(id)
  close_popup(id)
end)

hw.on_request("delete-notification", function(args)
  local id = tonumber(args[1] or "")
  if id and notifd:get(id) then
    notifd:dismiss(id)
    return "Notification " .. id .. " dismissed."
  end
  return "Notification " .. tostring(args[1]) .. " not found."
end)

--- Do-not-disturb toggle (ControlPanel.tsx DndToggle).
hw.define("DndToggle", function(props)
  return W.Button({
    class = hw.computed(function()
      local c = "dnd-toggle"
      if notifd.dnd() then
        c = c .. " checked"
      end
      if dnd_ping() then
        c = c .. " active"
      end
      return c .. " " .. (props.class or "")
    end),
    tooltip_markup = notifd.dnd:map(function(on)
      return on and "Disable Do Not Disturb" or "Enable Do Not Disturb"
    end),
    on_click = function()
      notifd.dnd:set(not notifd.dnd:peek())
    end,
    W.Label({
      label = notifd.dnd:map(function(on)
        return on and ICON.dnd_on or ICON.dnd_off
      end),
    }),
  })
end)

-- History: notifications stacked by summary, newest first.
local function stack_notifications(list)
  local sorted = {}
  for _, n in ipairs(list or {}) do
    table.insert(sorted, n)
  end
  table.sort(sorted, function(a, b)
    return (a.time or 0) > (b.time or 0)
  end)

  local stacks, by_title = {}, {}
  for i, n in ipairs(sorted) do
    if i > MAX_HISTORY then
      -- Like AGS: only the newest MAX_HISTORY are kept.
      local id = n.id
      hw.timeout(0, function()
        notifd:dismiss(id)
      end)
    else
      local title = (n.summary ~= "" and n.summary) or "Unknown"
      local s = by_title[title]
      if not s then
        s = { title = title, notifications = {} }
        by_title[title] = s
        table.insert(stacks, s)
      end
      table.insert(s.notifications, n)
    end
  end
  return stacks
end

hw.define("NotificationHistory", function(props)
  local expanded = hw.state({})

  local function stack_view(stack)
    local is_expanded = expanded:map(function(e)
      return e[stack.title] == true
    end)
    local count = #stack.notifications

    return W.Box({
      class = "notification-stack",
      vertical = true,
      W.Box({
        class = "stack-header",
        spacing = 5,
        W.Label({
          class = "stack-title",
          hexpand = true,
          xalign = 0,
          ellipsize = "end",
          label = "(" .. count .. ") " .. stack.title,
        }),
        W.Box({
          class = "actions",
          spacing = 5,
          W.Button({
            class = "stack-expand",
            visible = count > 1,
            label = is_expanded:map(function(e)
              return e and ICON.collapse or ICON.expand
            end),
            on_click = function()
              local e = {}
              for k, v in pairs(expanded:peek()) do
                e[k] = v
              end
              e[stack.title] = not e[stack.title] or nil
              expanded:set(e)
            end,
          }),
          W.Button({
            class = "clear",
            label = ICON.close,
            on_click = function()
              for _, n in ipairs(stack.notifications) do
                notifd:dismiss(n.id)
              end
            end,
          }),
        }),
      }),
      W.Scrolled({
        class = is_expanded:map(function(e)
          return e and "stack-scrolled expanded" or "stack-scrolled collapsed"
        end),
        hscroll = "never",
        util.For(
          is_expanded:map(function(e)
            return e and stack.notifications or { stack.notifications[1] }
          end),
          function(n)
            return W.Notification({ n = n })
          end,
          function(n)
            return n.id
          end,
          { class = "stack-content", vertical = true, spacing = 5 }
        ),
      }),
    })
  end

  local stacks = notifd.notifications:map(stack_notifications)

  return W.Box({
    class = "notification-history " .. (props.class or ""),
    vertical = true,
    spacing = 8,
    vexpand = props.vexpand,
    W.Scrolled({
      vexpand = true,
      hscroll = "never",
      W.Box({
        vertical = true,
        W.Label({
          class = "empty",
          label = "No notifications",
          visible = stacks:map(function(s)
            return #s == 0
          end),
        }),
        util.For(stacks, stack_view, function(s)
          -- Rebuild a stack when its content changes.
          return s.title .. "#" .. #s.notifications .. "#" .. s.notifications[1].id
        end, { vertical = true }),
      }),
    }),
  })
end)

hw.per_monitor(function(monitor)
  local windows = {}

  table.insert(
    windows,
    W.Window({
      name = "notification-popups-" .. monitor.name,
      namespace = "notification-popups",
      class = "NotificationPopups",
      monitor = monitor,
      layer = "overlay",
      anchor = { "top", "right" },
      exclusivity = "normal",
      margin = hw.opt.general.gaps_out,
      width_request = 400,
      visible = popups:map(function(p)
        return #p > 0
      end),
      util.For(popups, function(p)
        return W.Notification({ n = p.n, popup = true, closing = closing })
      end, function(p)
        return p.key
      end, { class = "notification-popups", vertical = true, spacing = 5 }),
    })
  )

  -- Stand-alone history until the right panel is ported (M4).
  table.insert(
    windows,
    W.Window({
      name = "notification-history-" .. monitor.name,
      namespace = "notification-history",
      monitor = monitor,
      layer = "top",
      anchor = { "top", "right", "bottom" },
      exclusivity = "normal",
      keymode = "on_demand",
      margin = hw.opt.general.gaps_out,
      width_request = 400,
      visible = false,
      W.Box({
        class = "notification-history-window",
        vertical = true,
        spacing = 8,
        W.Box({
          class = "header",
          spacing = 5,
          W.Label({ class = "header-title", label = "Notifications", hexpand = true, xalign = 0 }),
          W.DndToggle({}),
          W.Button({
            class = "clear-all danger",
            label = ICON.close,
            tooltip = "Clear all",
            on_click = function()
              notifd:clear()
            end,
          }),
        }),
        W.NotificationHistory({ vexpand = true }),
      }),
    })
  )

  return windows
end)
