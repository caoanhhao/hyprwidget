-- Port of widgets/WallpaperSwitcher.tsx (`wallpaper-switcher-<monitor>`,
-- SUPER + W): wallpapers of each workspace on top, the chosen category below,
-- and actions (target workspace / sddm / lockscreen, palette, category,
-- random, reload daemon, add). Wallpapers are set through the hypr wallpaper
-- daemon scripts, as AGS did; the palette dots show hw.theme.colors.
local W = hw.widget
local common = require("config.bar.common")
local Box = common.Box

local DAEMON = HOME .. "/.config/hypr/wallpaper-daemon"
local LIST = CONFIG_DIR .. "/scripts/get-wallpapers.sh"
local CACHE = (os.getenv("XDG_CACHE_HOME") or (HOME .. "/.cache")) .. "/hyprwidget/thumbnails/"
local TARGETS = { "workspace", "sddm", "lockscreen" }

local function sh_quote(s)
  return "'" .. tostring(s):gsub("'", "'\\''") .. "'"
end

-- ~/.config/wallpapers/<cat>/x.png → <cache>/thumbnails/<cat>/x.jpg
local function thumbnail(file)
  local rel = file:match("/%.config/wallpapers/(.+)$")
  if not rel then
    return file
  end
  return CACHE .. rel:gsub("%.[^/.]+$", ".jpg")
end

local function notify(summary, body)
  hw.exec_async({ "notify-send", "-a", "hyprwidget", summary, body or "" }, function() end)
end

local function file_size(path)
  local f = io.open(path, "rb")
  if not f then
    return "N/A"
  end
  local size = f:seek("end")
  f:close()
  if size >= 1024 * 1024 then
    return string.format("%.1f MB", size / 1024 / 1024)
  end
  return string.format("%.0f KB", size / 1024)
end

-- Shared by every monitor: categories → files.
local wallpapers = hw.state({})
local function fetch_wallpapers(done)
  hw.exec_async({ "bash", LIST }, function(out, err)
    if err then
      notify("Error", err)
    else
      local ok, t = pcall(hw.json.decode, out or "{}")
      if ok and type(t) == "table" then
        wallpapers:set(t)
      end
    end
    if done then
      done()
    end
  end)
end

local category = hw.settings["wallpaperSwitcher.category"]
-- The chosen category, else the first one (AGS' default "defaults/sfw" may
-- not exist).
local shown_category = hw.computed(function()
  local t, c = wallpapers(), category()
  if c and t[c] then
    return c
  end
  local names = {}
  for k in pairs(t) do
    table.insert(names, k)
  end
  table.sort(names)
  return names[1] or c or ""
end)
local selected = hw.computed(function()
  return wallpapers()[shown_category()] or {}
end)

-- AGS Picture: a cover-fitted image with small info labels in a corner.
local function Picture(file, info, props)
  props = props or {}
  local labels = {}
  for _, i in ipairs(info or {}) do
    table.insert(labels, W.Label({ class = "image-info-item", label = i }))
  end
  return W.Overlay({
    class = "image",
    hexpand = props.hexpand,
    vexpand = true,
    child = Box({
      class = "wallpaper",
      hexpand = true,
      vexpand = true,
      css = 'background-image: url("file://'
        .. file:gsub('"', '\\"')
        .. '"); background-size: cover; background-position: center;',
    }),
    Box({ class = "image-info", halign = "end", valign = "end", spacing = 5, table.unpack(labels) }),
  })
end

-- AGS widgets/Progress.tsx (status icon that slides in while working).
local function Progress(status)
  local icons = { loading = "\u{f110}", error = "❌", success = "✅" }
  return W.Revealer({
    transition = "swing_right",
    reveal = status:map(function(s)
      return s ~= "idle"
    end),
    Box({
      class = status:map(function(s)
        return "progress-widget " .. s
      end),
      W.Label({
        class = "icon",
        label = status:map(function(s)
          return icons[s] or ""
        end),
      }),
    }),
  })
end

local function WallpaperSwitcher(monitor)
  local name = "wallpaper-switcher-" .. monitor.name
  local hypr = hw.service.hyprland
  local workspace = hw.state((hypr.focused_workspace:peek() or {}).id or 1)
  local target = hw.state("workspace")
  local status = hw.state("idle")
  local current = hw.state({})

  local function done(ok)
    status:set(ok and "success" or "error")
    hw.timeout(2000, function()
      if status:peek() ~= "loading" then
        status:set("idle")
      end
    end)
  end

  local function fetch_current()
    hw.exec_async({ "bash", LIST, "--current", monitor.name }, function(out, err)
      if err then
        notify("Error", err)
        return
      end
      local ok, t = pcall(hw.json.decode, out or "[]")
      if ok and type(t) == "table" then
        current:set(t)
      end
    end)
  end

  local function set_workspace_wallpaper(file)
    status:set("loading")
    hw.exec_async({ DAEMON .. "/set-wallpaper.sh", tostring(workspace:peek()), monitor.name, file }, function(_, err)
      if err then
        notify("Error", err)
      end
      fetch_current()
      done(not err)
    end)
  end

  local function apply(file)
    local t = target:peek()
    if t == "workspace" then
      set_workspace_wallpaper(file)
      return
    end
    local cmd
    if t == "sddm" then
      cmd = {
        "pkexec",
        "sed",
        "-i",
        "s|^background=.*|background=" .. file .. "|",
        "/usr/share/sddm/themes/where_is_my_sddm_theme/theme.conf",
      }
    else
      cmd = {
        "bash",
        "-c",
        "mkdir -p $HOME/.config/wallpapers/lockscreen && cp " .. sh_quote(file) .. " $HOME/.config/wallpapers/lockscreen/wallpaper",
      }
    end
    status:set("loading")
    hw.exec_async(cmd, function(_, err)
      if err then
        notify("Error", err)
      end
      done(not err)
    end)
  end

  -- Right click asks for a second right click before deleting the file.
  local armed
  local function delete(file)
    if armed ~= file then
      armed = file
      notify("Delete wallpaper?", "Right-click again within 3 s to delete\n" .. file)
      hw.timeout(3000, function()
        if armed == file then
          armed = nil
        end
      end)
      return
    end
    armed = nil
    status:set("loading")
    hw.exec_async({ "rm", "-f", "--", thumbnail(file), file }, function(_, err)
      if err then
        notify("Error", err)
      else
        notify("Success", "Wallpaper deleted successfully!")
      end
      fetch_wallpapers(function()
        done(not err)
      end)
    end)
  end

  local function add()
    status:set("loading")
    hw.exec_async({
      "zenity",
      "--file-selection",
      "--title=Select Wallpaper",
      "--file-filter=Images (png, jpg, webp, gif, mp4) | *.png *.jpg *.jpeg *.webp *.gif *.mp4",
    }, function(out, err)
      local path = (out or ""):gsub("%s+$", "")
      if err or path == "" then
        status:set("idle")
        return
      end
      local dir = HOME .. "/.config/wallpapers/custom"
      local base = path:match("([^/]+)$") or "wallpaper"
      hw.exec_async({ "bash", "-c", "mkdir -p " .. sh_quote(dir) .. " && cp -- " .. sh_quote(path) .. " " .. sh_quote(dir .. "/" .. base) }, function(_, cerr)
        if cerr then
          notify("Error", cerr)
          done(false)
          return
        end
        notify("Success", "Wallpaper added successfully!")
        -- get-wallpapers.sh makes the missing thumbnail.
        fetch_wallpapers(function()
          done(true)
        end)
      end)
    end)
  end

  local workspaces_row = hw.With(current, function(list)
    local buttons = {}
    for i, file in ipairs(list) do
      local child
      if file == "" then
        child = W.Label({ class = "no-wallpaper", label = "No Wallpaper", halign = "center", valign = "center" })
      else
        child = Picture(thumbnail(file), { tostring(i), file:match("%.([^.]+)$") or "unknown" })
      end
      table.insert(
        buttons,
        W.Button({
          class = hypr.focused_workspace:map(function(ws)
            return ((ws and ws.id) or 1) == i and "wallpaper-button focused" or "wallpaper-button"
          end),
          css = file == "" and "background-color: black;" or "",
          tooltip_markup = "Set wallpaper for <b>Workspace " .. i .. "</b>",
          on_click = function()
            target:set("workspace")
            workspace:set(i)
          end,
          child,
        })
      )
    end
    buttons.hexpand = true
    buttons.vexpand = true
    buttons.halign = "center"
    buttons.spacing = 10
    return Box(buttons)
  end)

  local previews = W.Scrolled({
    hscroll = "always",
    vscroll = "never",
    propagate_natural_height = true,
    hexpand = true,
    vexpand = true,
    Box({
      halign = "center",
      Box({
        class = "all-wallpapers",
        spacing = 5,
        hexpand = true,
        hw.For(selected, function(file)
          return W.Button({
            class = "wallpaper-button preview",
            tooltip_markup = target:map(function(t)
              return "Click to set as <b>"
                .. t
                .. "</b> wallpaper.\nRight-click twice to delete.\n "
                .. (file:match("([^/]+)$") or file)
                .. "\n Size: "
                .. file_size(file)
            end),
            on_click = function(_, button)
              if button == 3 then
                delete(file)
              else
                apply(file)
              end
            end,
            Picture(thumbnail(file), { file:match("%.([^.]+)$") or "unknown" }),
          })
        end, function(file)
          return file
        end),
      }),
    }),
  })

  local target_buttons = { class = "targets", hexpand = true, halign = "center" }
  for _, t in ipairs(TARGETS) do
    table.insert(
      target_buttons,
      common.toggle({
        valign = "center",
        class = t,
        label = t,
        active = target:map(function(x)
          return x == t
        end),
        on_toggle = function(on)
          if on then
            target:set(t)
          end
        end,
      })
    )
  end

  local ws_label = W.Label({
    class = "selected-workspace",
    label = hw.computed(function()
      return "Wallpaper -> " .. target() .. " " .. (target() == "workspace" and tostring(workspace()) or "")
    end),
    setup = function(self)
      local function ping()
        self:add_class("ping")
        hw.timeout(500, function()
          self:remove_class("ping")
        end)
      end
      workspace:subscribe(ping)
      target:subscribe(ping)
    end,
  })

  -- Material palette of the current wallpaper (AGS showed pywal's --color1..7).
  local dots = { class = "color-scheme", spacing = 10 }
  for _, role in ipairs({ "primary", "secondary", "tertiary", "primary_container", "secondary_container", "tertiary_container", "error" }) do
    table.insert(
      dots,
      W.Label({
        class = "color",
        label = "\u{f10c}",
        css = hw.theme.colors:map(function(c)
          return "color: " .. ((c and c[role]) or "inherit") .. ";"
        end),
      })
    )
  end
  dots.tooltip_markup = hw.theme.source:map(function(s)
    return "Dynamic Colors from the wallpaper (<b>material-colors</b>)\nsource " .. tostring(s)
  end)

  local menu
  local category_selector = W.MenuButton({
    class = "category-selector",
    halign = "center",
    setup = function(self)
      menu = self
    end,
    child = W.Label({ label = shown_category }),
    popover = W.Popover({
      hw.With(wallpapers, function(t)
        local names = {}
        for k in pairs(t) do
          table.insert(names, k)
        end
        table.sort(names)
        local items = { class = "popover", vertical = true, spacing = 5 }
        for _, k in ipairs(names) do
          table.insert(
            items,
            W.Button({
              class = "category",
              label = k,
              on_click = function()
                hw.settings:set("wallpaperSwitcher.category", k)
                if menu then
                  menu:set("active", false)
                end
              end,
            })
          )
        end
        return Box(items)
      end),
    }),
  })

  local actions = Box({
    class = "actions",
    hexpand = true,
    halign = "center",
    spacing = 10,
    Box(target_buttons),
    ws_label,
    Box(dots),
    category_selector,
    W.Button({
      valign = "center",
      class = "random-wallpaper",
      label = "\u{f074}",
      tooltip_markup = "Set a <b>Random</b> wallpaper",
      on_click = function()
        local list = selected:peek()
        if #list > 0 then
          set_workspace_wallpaper(list[math.random(#list)])
        end
      end,
    }),
    W.Button({
      valign = "center",
      class = "reload-wallpapers",
      label = "󰑐",
      tooltip_markup = "Reload <b>Wallpaper Daemon</b>",
      on_click = function()
        status:set("loading")
        hw.exec_async({ DAEMON .. "/reload.sh" }, function(_, err)
          if err then
            notify("Error", err)
          end
          fetch_wallpapers(function()
            done(not err)
          end)
        end)
      end,
    }),
    W.Button({ class = "upload", label = "\u{f067}", tooltip_markup = "Add a <b>New Custom Wallpaper</b>", on_click = add }),
    Progress(status),
  })

  hypr.focused_workspace:subscribe(function(ws)
    if ws and ws.id then
      workspace:set(ws.id)
    end
  end)

  return W.Window({
    name = name,
    namespace = "wallpaper-switcher",
    monitor = monitor,
    layer = "overlay",
    keymode = "on_demand",
    exclusivity = "ignore",
    anchor = { "left", "bottom", "right" },
    visible = false,
    on_show = function()
      fetch_current()
    end,
    on_key = function(_, key)
      if key == "Escape" then
        local w = hw.window.get(name)
        if w then
          w:hide()
        end
        return true
      end
    end,
    Box({
      class = "wallpaper-switcher",
      vertical = true,
      spacing = 20,
      Box({ workspaces_row }),
      actions,
      previews,
    }),
  })
end

fetch_wallpapers()

hw.per_monitor(function(monitor)
  return { WallpaperSwitcher(monitor) }
end)
