-- Smaller right panel widgets (widgets/rightPanel/components/*.tsx):
-- Calendar, ScriptTimer, SystemResources and the VPN / Docker / WayDroid
-- toggles (enabling the widget starts the service, disabling stops it).
local W = hw.widget
local common = require("config.panels.common")
local util = require("lib.util")
local system_resources = require("config.bar.system_resources")
local Box = common.Box

hw.define("CalendarWidget", function(props)
  return Box({
    class = common.class("calendarBox", props.class),
    vertical = true,
    W.Calendar({ class = "calendar", focusable = false }),
  })
end)

-- Columns stack vertically while the panel is narrower than 400 px.
hw.define("SystemResources", function(props)
  local vertical = hw.settings["rightPanel.width"]:map(function(w)
    return (w or 250) < 400
  end)
  return Box({
    class = props.class,
    hw.With(vertical, function(v)
      return system_resources({ class = props.class, vertical = v })
    end),
  })
end)

-- ScriptTimer -------------------------------------------------------------

local TASKS_FILE = common.CACHE_DIR .. "/script-timer/tasks.json"
local PRESETS = {
  { label = "🔔 Notification", command = "notify-send 'Timer Alert' 'Scheduled task executed'" },
  { label = "🔒 Lock Screen", command = "hyprlock" },
  { label = "💤 Suspend", command = "systemctl suspend" },
  { label = "🔄 Reboot", command = "reboot" },
  { label = "⚡ Shutdown", command = "shutdown -h now" },
}

local function next_run(task)
  local h, m = task.time:match("^(%d%d):(%d%d)$")
  local now = os.time()
  local t = os.date("*t", now)
  t.hour, t.min, t.sec = tonumber(h), tonumber(m), 0
  local run = os.time(t)
  if run <= now then
    run = run + 86400
  end
  task.nextRun = run * 1000
  return task
end

local tasks = hw.state({})
do
  local loaded = {}
  for _, t in ipairs(util.read_json(TASKS_FILE, {})) do
    if type(t.time) == "string" and t.time:match("^%d%d:%d%d$") then
      table.insert(loaded, next_run(t))
    end
  end
  tasks:set(loaded)
end
local show_task_form = hw.state(false)
local editing_task = hw.state(false)

local function save_tasks(list)
  tasks:set(list)
  util.write_json(TASKS_FILE, list)
end

local function execute(task)
  hw.exec_async({ "bash", "-c", task.command }, function(_, err)
    if err then
      common.notify("Script Timer Error", 'Failed to execute "' .. task.name .. '"')
      return
    end
    common.notify("Script Timer", 'Task "' .. task.name .. '" executed')
  end)
  local list = {}
  for _, t in ipairs(tasks:peek()) do
    if t.id ~= task.id then
      table.insert(list, t)
    elseif t.type ~= false then
      local copy = {}
      for k, v in pairs(t) do
        copy[k] = v
      end
      table.insert(list, next_run(copy))
    end
  end
  save_tasks(list)
end

-- AGS only checked while the widget existed; tasks are global here.
hw.interval(10000, function()
  local now = os.time() * 1000
  for _, t in ipairs(tasks:peek()) do
    if t.active and t.nextRun and t.nextRun <= now then
      execute(t)
    end
  end
end)

local function TaskForm(task)
  local name = task and task.name or ""
  local time = task and task.time or "12:00"
  local command = hw.state(task and task.command or "")
  local daily = hw.state(task == nil or task.type ~= false)
  local query = hw.state("")

  local function submit()
    local n, c = name:match("^%s*(.-)%s*$"), command:peek():match("^%s*(.-)%s*$")
    if n == "" or c == "" or not time:match("^%d%d:%d%d$") then
      common.notify("Script Timer", "Please fill all fields correctly")
      return
    end
    local new = next_run({
      id = task and task.id or tostring(os.time() * 1000),
      name = n,
      command = c,
      time = time,
      type = daily:peek(),
      active = true,
    })
    local list = {}
    for _, t in ipairs(tasks:peek()) do
      table.insert(list, (task and t.id == task.id) and new or t)
    end
    if not task then
      table.insert(list, new)
    end
    save_tasks(list)
    show_task_form:set(false)
    editing_task:set(false)
  end

  return Box({
    class = "form-container",
    vertical = true,
    spacing = 8,
    W.Entry({
      text = name,
      placeholder = "Enter task name",
      on_change = function(_, t)
        name = t
      end,
    }),
    W.Entry({
      text = time,
      placeholder = "HH:MM (24-hour format)",
      max_length = 5,
      on_change = function(_, t)
        time = t
      end,
    }),
    Box({
      vertical = true,
      W.Entry({
        text = command,
        placeholder = "Enter command or select preset",
        on_change = function(_, t)
          if t ~= command:peek() then
            command:set(t)
            query:set(t)
          end
        end,
      }),
      W.Revealer({
        transition = "swing_down",
        duration = common.TRANSITION,
        reveal = query:map(function(q)
          if q:match("^%s*$") then
            return false
          end
          for _, p in ipairs(PRESETS) do
            if p.label:lower():find(q:lower(), 1, true) then
              return true
            end
          end
          return false
        end),
        hw.With(query, function(q)
          local box = { class = "suggestions", vertical = true, spacing = 2 }
          for _, p in ipairs(PRESETS) do
            if p.label:lower():find(q:lower(), 1, true) then
              table.insert(
                box,
                W.Button({
                  class = "suggestion",
                  label = p.label,
                  on_click = function()
                    command:set(p.command)
                    query:set("")
                  end,
                })
              )
            end
          end
          return Box(box)
        end),
      }),
    }),
    Box({
      class = "task-type-selector",
      spacing = 8,
      common.toggle({
        label = "Daily",
        active = daily,
        on_toggle = function()
          daily:set(true)
        end,
      }),
      common.toggle({
        label = "One-time",
        active = daily:map(function(d)
          return not d
        end),
        on_toggle = function()
          daily:set(false)
        end,
      }),
    }),
    Box({
      class = "form-actions",
      spacing = 8,
      W.Button({ class = "success", label = task and "✓ Update" or "+ Add Task", hexpand = true, on_click = submit }),
      W.Button({
        class = "danger",
        label = "✕ Cancel",
        on_click = function()
          show_task_form:set(false)
          editing_task:set(false)
        end,
      }),
    }),
  })
end

local function format_next_run(task)
  if not task.nextRun then
    return "Not scheduled"
  end
  local t = task.nextRun // 1000
  if os.date("%Y-%m-%d", t) == os.date("%Y-%m-%d") then
    return "Today, " .. os.date("%H:%M", t)
  end
  return os.date("%b %d, %H:%M", t)
end

local function update_task(id, fn)
  local list = {}
  for _, t in ipairs(tasks:peek()) do
    if t.id == id then
      local copy = {}
      for k, v in pairs(t) do
        copy[k] = v
      end
      t = fn(copy)
    end
    if t then
      table.insert(list, t)
    end
  end
  save_tasks(list)
end

local function TaskItem(task)
  return common.hover_actions({
    class = "task-eventbox",
    actions = Box({
      class = "task-actions",
      W.Button({
        class = task.active and "success checked" or "danger",
        label = task.active and "●" or "○",
        on_click = function()
          update_task(task.id, function(t)
            t.active = not t.active
            return t
          end)
        end,
      }),
      W.Button({
        label = "✏",
        on_click = function()
          editing_task:set(task)
          show_task_form:set(true)
        end,
      }),
      W.Button({
        label = "✕",
        on_click = function()
          update_task(task.id, function()
            return nil
          end)
        end,
      }),
    }),
    build = function(actions)
      local cmd = task.command
      if #cmd > 40 then
        cmd = cmd:sub(1, 40) .. "..."
      end
      return Box({
        class = "task " .. (task.active and "active" or "inactive"),
        vertical = true,
        spacing = 6,
        hexpand = true,
        Box({
          class = "task-header",
          Box({
            class = "task-info",
            hexpand = true,
            W.Label({ class = "task-name", label = task.name, hexpand = true, halign = "start" }),
            Box({
              spacing = 5,
              W.Label({ class = "task-schedule", label = format_next_run(task) }),
              W.Label({ class = "task-type icon", label = task.type ~= false and "\u{edfe}" or "1" }),
            }),
          }),
          actions,
        }),
        W.Label({ class = "task-command", label = cmd, halign = "start" }),
      })
    end,
  })
end

hw.define("ScriptTimer", function(props)
  return Box({
    class = common.class("script-timer module", props.class),
    vertical = true,
    spacing = 5,
    Box({
      class = "header",
      W.Label({ class = "title", label = "Script Timer", hexpand = true, halign = "start" }),
      W.Button({
        class = "add-btn",
        label = show_task_form:map(function(s)
          return s and "✕" or "+"
        end),
        on_click = function()
          editing_task:set(false)
          show_task_form:set(not show_task_form:peek())
        end,
      }),
    }),
    W.Revealer({
      reveal = show_task_form,
      transition = "swing_down",
      duration = common.TRANSITION,
      hw.With(editing_task, function(t)
        return TaskForm(t or nil)
      end),
    }),
    W.Scrolled({
      class = "task-list",
      vexpand = true,
      hscroll = "never",
      common.For(tasks, TaskItem, function(t)
        return t.id .. (t.active and "+" or "-") .. tostring(t.nextRun)
      end, { vertical = true, spacing = 5 }),
    }),
  })
end)

-- VPN / Docker / WayDroid ------------------------------------------------
-- Showing the widget starts the service; hiding it (widget disabled in the
-- right panel) stops it, like AGS' onCleanup.

local function enabled(name)
  for _, w in ipairs(hw.settings:get("rightPanel.widgets") or {}) do
    if w.name == name then
      return w.enabled == true
    end
  end
  return false
end

local function service_widget(name, class, check, start, stop)
  hw.define(name, function(props)
    hw.exec_async(check.cmd, function(out, err)
      if err or not check.running(out) then
        common.run(start, name)
      end
    end)
    return Box({
      class = common.class(class, props.class),
      vertical = true,
      on_destroy = function()
        if not enabled(name) then
          common.run(stop, name)
        end
      end,
    })
  end)
end

service_widget("VPN", "vpn", {
  cmd = { "warp-cli", "status" },
  running = function(out)
    return out:find("Connecting") ~= nil
  end,
}, { "warp-cli", "connect" }, { "warp-cli", "disconnect" })

service_widget("Docker", "docker", {
  cmd = { "systemctl", "is-active", "docker" },
  running = function(out)
    return out:find("inactive") == nil
  end,
}, { "systemctl", "--user", "start", "docker" }, { "systemctl", "--user", "stop", "docker.socket", "docker.service" })

service_widget("WayDroid", "waydroid", {
  cmd = { "waydroid", "status" },
  running = function(out)
    return out:find("STOPPED") == nil
  end,
}, { "bash", "-c", "$HOME/.local/bin/waydroidctl" }, { "bash", "-c", "$HOME/.local/bin/waydroidctl stop" })
