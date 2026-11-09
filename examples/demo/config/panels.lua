-- LeftPanel / RightPanel and their hover strips on every monitor (AGS
-- app.tsx perMonitorDisplay). The panel windows are in config/panels/panel.lua,
-- the selectable widgets (each a hw.define'd W.<Name>) in config/panels/*.lua.
require("config.panels.defaults").ensure()

require("config.panels.cava")
require("config.panels.player")
require("config.panels.crypto")
require("config.panels.booru")
require("config.panels.right_widgets")
require("config.panels.left_widgets")

hw.per_monitor(require("config.panels.panel").per_monitor)
