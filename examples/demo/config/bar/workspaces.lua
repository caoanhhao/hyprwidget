-- Port of widgets/bar/components/Workspaces.tsx (+ WorkspaceOverview.tsx):
-- special-workspace toggle, one button per workspace of this monitor with
-- an icon of its first window, focus/transition classes for the SCSS
-- animations, and a hover popover sketching the workspace's window layout.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box
local hypr = hw.service.hyprland

local SPECIAL_ICON = ""
local FALLBACK_ICON = "" -- AGS extraWorkspaceIcon (no rule matched)

-- AGS constants/workspace.constants.ts, as Lua patterns over the lowercased
-- window class (first match wins).
local ICONS = {
  { { "cord$" }, "󰙯" },
  { { "foot", "kitty", "alacritty", "xterm", "gnome%-terminal", "konsole" }, "" },
  { { "thunar", "nautilus", "dolphin", "ranger" }, "󰉋" },
  { { "chrome", "chromium", "brave" }, "" },
  { { "firefox", "zen" }, "󰈹" },
  { { "vlc" }, "󰕼" },
  { { "spotify", "spotube" }, "" },
  { { "code", "vscode", "sublime", "jetbrains" }, "" },
  { { "steam" }, "" },
  { { "lutris", "game", "%.exe$" }, "" },
  { { "telegram" }, "" },
}

local function workspace_icon(class)
  class = (class or "empty"):lower()
  for _, rule in ipairs(ICONS) do
    for _, pat in ipairs(rule[1]) do
      if class:find(pat) then
        return rule[2]
      end
    end
  end
  return FALLBACK_ICON
end

local function clients_of(id)
  local out = {}
  for _, c in ipairs(hypr.clients() or {}) do
    if c.workspace and c.workspace.id == id then
      table.insert(out, c)
    end
  end
  return out
end

-- WorkspaceOverview.tsx buildTree: split the windows along a vertical, then
-- horizontal line that separates them; else keep the biggest.
local function build_tree(clients)
  if #clients == 1 then
    return { leaf = clients[1] }
  end
  local function try(axis)
    local pos, size = axis == "x" and 1 or 2, axis == "x" and 1 or 2
    local cuts, seen = {}, {}
    for _, c in ipairs(clients) do
      for _, v in ipairs({ c.at[pos], c.at[pos] + c.size[size] }) do
        if not seen[v] then
          seen[v] = true
          table.insert(cuts, v)
        end
      end
    end
    table.sort(cuts)
    for _, x in ipairs(cuts) do
      local a, b = {}, {}
      for _, c in ipairs(clients) do
        if c.at[pos] + c.size[size] <= x + 5 then
          table.insert(a, c)
        end
        if c.at[pos] >= x - 5 then
          table.insert(b, c)
        end
      end
      if #a > 0 and #b > 0 and #a + #b == #clients then
        return { split = axis, a = a, b = b }
      end
    end
  end
  local s = try("x") or try("y")
  if s then
    return { split = s.split, a = build_tree(s.a), b = build_tree(s.b) }
  end
  table.sort(clients, function(p, q)
    return p.size[1] * p.size[2] > q.size[1] * q.size[2]
  end)
  return { leaf = clients[1] }
end

local function render_node(node)
  if node.leaf then
    local c = node.leaf
    return W.Overlay({
      class = "workspace-client",
      tooltip_markup = "<b>Hold to drag</b>\n" .. (c.class or ""),
      width_request = math.floor(c.size[1] / 7),
      height_request = math.floor(c.size[2] / 7),
      child = W.Image({
        icon = common.app_icon(c.class) or "application-x-executable",
        hexpand = true,
        vexpand = true,
      }),
      W.Label({
        class = "title",
        label = c.title,
        valign = "end",
        ellipsize = "end",
      }),
      W.Label({ class = "move", label = "󰆾", valign = "start", halign = "start" }),
    })
  end
  return Box({
    vertical = node.split == "y",
    spacing = 5,
    homogeneous = true,
    render_node(node.a),
    render_node(node.b),
  })
end

local function client_layout(id)
  -- Rebuilt only when this workspace's windows change.
  local key = hw.computed(function()
    local parts = {}
    for _, c in ipairs(clients_of(id)) do
      table.insert(
        parts,
        table.concat({ c.address, c.title, c.at[1], c.at[2], c.size[1], c.size[2] }, ",")
      )
    end
    return table.concat(parts, ";")
  end)
  return Box({
    class = "workspace-client-layout",
    hw.With(key, function()
      local clients = clients_of(id)
      if #clients == 0 then
        return W.Label({ label = "empty", class = "workspace-client-layout" })
      end
      return render_node(build_tree(clients))
    end),
  })
end

return function(monitor, opts)
  local on_active = opts.on_active
  local prev = {} -- workspace id -> { focused = bool }

  local function button(ws)
    local id = ws.id
    local class = hw.computed(function()
      local focused = (hypr.focused_workspace() or {}).id == id
      local classes = { focused and "is-focused" or "is-unfocused", "is-active" }
      local p = prev[id]
      if p then
        if not p.focused and focused then
          table.insert(classes, "trans-gaining-focus")
        elseif p.focused and not focused then
          table.insert(classes, "trans-losing-focus")
        end
      end
      prev[id] = { focused = focused }
      return table.concat(classes, " ")
    end)
    local label = hw.computed(function()
      local first = clients_of(id)[1]
      return workspace_icon(first and first.class)
    end)
    local props = common.hover_popover(client_layout(id), { on_active = on_active })
    props.class = class
    props.label = label
    props.on_click = function()
      common.dispatch(string.format("hl.dsp.focus({workspace = %d})", id))
    end
    return W.Button(props)
  end

  local list = hw.computed(function()
    local out = {}
    for _, ws in ipairs(hypr.workspaces() or {}) do
      if ws.monitor == monitor.name and ws.id and ws.id >= 0 then
        table.insert(out, ws)
      end
    end
    table.sort(out, function(a, b)
      return a.id < b.id
    end)
    return out
  end)

  -- AGS: the focused window is on a special workspace.
  local special = hypr.focused_client:map(function(c)
    local ws = c and c.workspace
    return ws ~= nil and (ws.id == nil or ws.id < 0 or (ws.name or ""):find("^special") ~= nil)
  end)

  return Box({
    class = "workspaces",
    spacing = 5,
    halign = opts.halign,
    hexpand = true,
    W.Button({
      class = special:map(function(s)
        return s and "special active" or "special inactive"
      end),
      label = SPECIAL_ICON,
      tooltip_markup = "Special Workspace\n<b>SUPER + S</b>",
      on_click = function()
        common.dispatch('hl.dsp.workspace.toggle_special("special")')
      end,
    }),
    Box({
      class = "workspaces-display",
      Box({
        class = "workspace-group active",
        spacing = 0,
        hw.For(list, button, function(ws)
          return ws.id
        end),
      }),
    }),
  })
end
