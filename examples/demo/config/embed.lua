-- Embedded app demo (PLAN.md §6, M5): a mixer panel running pavucontrol
-- *inside* hyprwidget's layer surface (no Hyprland window of its own).
--
-- Off by default. Enable with `"embed": { "mixer": true }` in
-- cache/settings/settings.json (then `hyprwidget reload lua`), and open it:
--
--   hyprwidget toggle embed-mixer-DP-2
--
-- pavucontrol starts when the panel is first shown and is closed 2 s after
-- it is hidden (`keep_alive = false`); `keep_alive = true` keeps it running.
local W = hw.widget

if hw.settings:get("embed.mixer") == nil then
  hw.settings:set("embed.mixer", false)
end
if hw.settings:get("embed.mixer") ~= true then
  return
end

hw.per_monitor(function(monitor)
  local title = hw.state("Volume Control")
  local status = hw.state("")
  local app -- the W.App's `self`, set in its setup

  return W.Window({
    name = "embed-mixer-" .. monitor.name,
    namespace = "embed-mixer",
    monitor = monitor,
    layer = "top",
    anchor = { "top", "right" },
    keymode = "on_demand",
    visible = false,
    W.Box({
      vertical = true,
      class = "embed-mixer",
      css = "padding: 8px; background: alpha(@theme_bg_color, 0.85);"
        .. "border-radius: var(--decoration-rounding, 12px);",
      W.CenterBox({
        start = W.Label({ label = title, xalign = 0, class = "embed-mixer-title" }),
        ["end"] = W.Box({
          W.Label({ label = status, class = "dim-label" }),
          W.Button({
            icon = "view-refresh-symbolic",
            tooltip = "Restart pavucontrol",
            on_click = function()
              if app then
                status:set("")
                app:restart()
              end
            end,
          }),
        }),
      }),
      W.App({
        command = { "pavucontrol" },
        env = { GTK_THEME = "Adwaita:dark" },
        class = "embedded-mixer",
        size = { 560, 600 },
        mode = "resize",
        keep_alive = false,
        restart = "on-failure",
        frame_when_hidden = false,
        -- rounding defaults to decoration:rounding; 0 keeps zero-copy.
        on_title = function(_, t)
          title:set(t ~= "" and t or "Volume Control")
        end,
        on_exit = function(_, code)
          status:set(code == 0 and "" or ("exited " .. code))
        end,
        setup = function(self)
          app = self
        end,
      }),
    }),
  })
end)
