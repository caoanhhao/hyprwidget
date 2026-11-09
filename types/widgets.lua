---@meta
-- LuaLS type stubs for hyprwidget widgets (`hw.widget` / `W`).
-- Docs: docs/widgets.md. Kept in sync with crates/hw-ui/src/widgets.rs by
-- the `docs_cover_api` test (every widget kind and event must appear here).

---A widget table (`{ type = "Box", ... }`) as returned by `W.*`; built into
---a GTK widget when it is placed in a window.
---@class hw.Node: table
---@field type string

---Anything that can stand where a widget goes: a widget table, text
---(→ Label), a signal (→ child rebuilt on change), a built widget, or
---nil/false (→ nothing).
---@alias hw.Child hw.Node|string|number|hw.Signal|hw.Widget|false|nil

---@alias hw.Align "start"|"left"|"top"|"end"|"right"|"bottom"|"center"|"fill"|"baseline"
---@alias hw.Sides number|string|number[]|{top?: number, right?: number, bottom?: number, left?: number}
---@alias hw.Restart "never"|"on-exit"|"on-failure"
---@alias hw.Mods ("shift"|"ctrl"|"alt"|"super")[]

---`self` in `setup` and event handlers.
---@class hw.Widget
---@field kind string? Widget kind (`"Box"`), nil for `:parent()`.
---@field name string Widget name.
---@field pid integer? `W.App` only: the app's process id.
---@field title hw.Signal? `W.App` only: toplevel title.
---@field app_id hw.Signal? `W.App` only: toplevel app id.
local Widget = {}
---Sets a property like in the widget table.
---@param prop string
---@param value any
function Widget:set(prop, value) end
---Reads a GObject property (`self:get("label")`).
---@param prop string
---@return any
function Widget:get(prop) end
---@param class string
function Widget:add_class(class) end
---@param class string
function Widget:remove_class(class) end
---@param class string
---@param on? boolean
---@return boolean on
function Widget:toggle_class(class, on) end
---@param class string
---@return boolean
function Widget:has_class(class) end
function Widget:show() end
function Widget:hide() end
---@return boolean
function Widget:is_visible() end
function Widget:queue_draw() end
---@return boolean
function Widget:grab_focus() end
---Connects an event after creation (`self:on("on_click", fn)`).
---@param event hw.EventName
---@param fn function
function Widget:on(event, fn) end
---@return hw.Widget?
function Widget:parent() end
---@return hw.Window?
function Widget:window() end
---`W.App` only: asks the app to close.
function Widget:send_close() end
---`W.App` only: restarts the app.
function Widget:restart() end
---`W.App` only: gives the app keyboard focus.
---@return boolean
function Widget:focus() end

---@alias hw.EventName "on_click"|"on_scroll"|"on_hover"|"on_leave"|"on_motion"|"on_key"|"on_change"|"on_activate"|"on_show"|"on_hide"|"on_destroy"|"on_title"|"on_exit"

---Props shared by every widget. Unknown props fall back to the GObject
---property of the same name (`_` → `-`).
---@class hw.WidgetProps
---@field [integer] hw.Child Positional children (or the single child).
---@field class? string|string[]|hw.Signal CSS classes (added to `hw-<kind>`).
---@field class_name? string|string[]|hw.Signal
---@field name? string Widget name (`#name` in CSS; Stack page name).
---@field visible? boolean|hw.Signal
---@field tooltip? string|hw.Signal
---@field tooltip_text? string|hw.Signal
---@field tooltip_markup? string|hw.Signal
---@field sensitive? boolean|hw.Signal
---@field hexpand? boolean|hw.Signal
---@field vexpand? boolean|hw.Signal
---@field halign? hw.Align|hw.Signal
---@field valign? hw.Align|hw.Signal
---@field width? integer|hw.Signal Minimum width (`width_request`).
---@field width_request? integer|hw.Signal
---@field height? integer|hw.Signal Minimum height (`height_request`).
---@field height_request? integer|hw.Signal
---@field margin? hw.Sides|hw.Signal
---@field opacity? number|hw.Signal
---@field focusable? boolean|hw.Signal
---@field can_focus? boolean|hw.Signal
---@field can_target? boolean|hw.Signal
---@field cursor? string|hw.Signal CSS cursor name (`"pointer"`).
---@field widget_name? string
---@field css? string|hw.Signal Inline CSS declarations for this widget.
---@field theme? hw.Config|hw.Signal Per-widget Hyprland option overrides (→ CSS custom properties).
---@field popover? hw.PopoverProps|hw.Node A `W.Popover` parented to this widget.
---@field setup? fun(self: hw.Widget) Runs once after the widget is built.
---@field col? integer Grid cell column (child of a Grid).
---@field row? integer Grid cell row.
---@field col_span? integer
---@field row_span? integer
---@field on_click? fun(self: hw.Widget, button: integer): boolean?
---@field on_scroll? fun(self: hw.Widget, dx: number, dy: number): boolean?
---@field on_hover? fun(self: hw.Widget, x: number, y: number)
---@field on_leave? fun(self: hw.Widget)
---@field on_motion? fun(self: hw.Widget, x: number, y: number)
---@field on_key? fun(self: hw.Widget, name: string, code: integer, mods: hw.Mods): boolean?
---@field on_show? fun(self: hw.Widget, shown: true)
---@field on_hide? fun(self: hw.Widget, shown: false)
---@field on_destroy? fun(self: hw.Widget)

---@class hw.WindowProps: hw.WidgetProps
---@field name? string Registry name (`hyprwidget toggle <name>`).
---@field namespace? string Layer namespace (default: name).
---@field monitor? hw.Monitor|string|nil Monitor or connector (`"DP-2"`).
---@field layer? "background"|"bottom"|"top"|"overlay"
---@field anchor? string|("top"|"bottom"|"left"|"right")[]
---@field exclusivity? "exclusive"|"normal"|"ignore"
---@field exclusive_zone? integer
---@field margin? hw.Sides|hw.Signal Default: `general:gaps_out`.
---@field keymode? "none"|"exclusive"|"on_demand"
---@field keyboard_mode? "none"|"exclusive"|"on_demand"
---@field default_width? integer
---@field default_height? integer
---@field title? string
---@field child? hw.Child

---@class hw.BoxProps: hw.WidgetProps
---@field vertical? boolean|hw.Signal
---@field spacing? integer|hw.Signal Default: `general:gaps_in`.
---@field homogeneous? boolean|hw.Signal
---@field children? hw.Child[]|hw.Signal

---@class hw.CenterBoxProps: hw.WidgetProps
---@field vertical? boolean|hw.Signal
---@field start? hw.Child
---@field center? hw.Child
---@field ["end"]? hw.Child

---@class hw.LabelProps: hw.WidgetProps
---@field label? string|number|hw.Signal
---@field text? string|number|hw.Signal
---@field markup? string|hw.Signal
---@field use_markup? boolean|hw.Signal
---@field wrap? boolean|hw.Signal
---@field xalign? number|hw.Signal
---@field yalign? number|hw.Signal
---@field max_width_chars? integer|hw.Signal
---@field width_chars? integer|hw.Signal
---@field selectable? boolean|hw.Signal
---@field ellipsize? boolean|"start"|"middle"|"end"|"none"|hw.Signal
---@field justify? "left"|"right"|"center"|"fill"|hw.Signal

---@class hw.ButtonProps: hw.WidgetProps
---@field label? string|hw.Signal
---@field icon? string|hw.Signal Icon name.
---@field has_frame? boolean|hw.Signal
---@field child? hw.Child

---@class hw.ImageProps: hw.WidgetProps
---@field icon? string|hw.Signal
---@field icon_name? string|hw.Signal
---@field file? string|hw.Signal
---@field pixel_size? integer|hw.Signal
---@field size? integer|hw.Signal

---@class hw.IconProps: hw.WidgetProps
---@field icon? string|hw.Signal
---@field icon_name? string|hw.Signal
---@field pixel_size? integer|hw.Signal
---@field size? integer|hw.Signal

---@class hw.PictureProps: hw.WidgetProps
---@field file? string|hw.Signal
---@field content_fit? "fill"|"contain"|"cover"|"scale_down"|hw.Signal
---@field can_shrink? boolean|hw.Signal

---@alias hw.RevealerTransition "none"|"crossfade"|"fade"|"slide_right"|"slide_left"|"slide_up"|"slide_down"|"swing_right"|"swing_left"|"swing_up"|"swing_down"

---@class hw.RevealerProps: hw.WidgetProps
---@field reveal? boolean|hw.Signal
---@field reveal_child? boolean|hw.Signal
---@field transition? hw.RevealerTransition|hw.Signal
---@field duration? integer|hw.Signal ms; default `animations:animation:layersIn:duration`.
---@field transition_duration? integer|hw.Signal
---@field child? hw.Child
---@field on_change? fun(self: hw.Widget, revealed: boolean)

---@class hw.StackProps: hw.WidgetProps
---@field shown? string|hw.Signal Visible page (child `name`).
---@field visible_child_name? string|hw.Signal
---@field transition? hw.RevealerTransition|"slide_left_right"|"slide_up_down"|"over_up"|"over_down"|"over_left"|"over_right"|"rotate_left"|"rotate_right"|hw.Signal
---@field duration? integer|hw.Signal
---@field transition_duration? integer|hw.Signal
---@field hhomogeneous? boolean|hw.Signal
---@field vhomogeneous? boolean|hw.Signal
---@field children? hw.Child[]|hw.Signal
---@field on_change? fun(self: hw.Widget, page: string?)

---@alias hw.ScrollPolicy "never"|"auto"|"automatic"|"always"|"external"

---@class hw.ScrolledProps: hw.WidgetProps
---@field hscroll? hw.ScrollPolicy|hw.Signal
---@field vscroll? hw.ScrollPolicy|hw.Signal
---@field propagate_natural_width? boolean|hw.Signal
---@field propagate_natural_height? boolean|hw.Signal
---@field child? hw.Child

---@class hw.EntryProps: hw.WidgetProps
---@field text? string|hw.Signal
---@field placeholder? string|hw.Signal
---@field placeholder_text? string|hw.Signal
---@field password? boolean|hw.Signal
---@field position? integer|hw.Signal Cursor position, -1 = end.
---@field cursor_position? integer|hw.Signal
---@field on_change? fun(self: hw.Widget, text: string)
---@field on_activate? fun(self: hw.Widget, text: string)

---@class hw.SliderProps: hw.WidgetProps
---@field value? number|hw.Signal
---@field min? number|hw.Signal
---@field max? number|hw.Signal
---@field step? number|hw.Signal
---@field draw_value? boolean|hw.Signal
---@field digits? integer|hw.Signal
---@field vertical? boolean|hw.Signal
---@field inverted? boolean|hw.Signal
---@field on_change? fun(self: hw.Widget, value: number)

---@class hw.SwitchProps: hw.WidgetProps
---@field active? boolean|hw.Signal
---@field on_change? fun(self: hw.Widget, active: boolean)

---@class hw.ProgressProps: hw.WidgetProps
---@field value? number|hw.Signal 0..1
---@field fraction? number|hw.Signal
---@field vertical? boolean|hw.Signal
---@field inverted? boolean|hw.Signal
---@field text? string|hw.Signal

---@class hw.OverlayProps: hw.WidgetProps
---@field child? hw.Child Base child (sizes the overlay); positional children are overlays.
---@field children? hw.Child[]|hw.Signal

---@class hw.GridProps: hw.WidgetProps
---@field columns? integer Auto-placement width.
---@field spacing? integer|hw.Signal
---@field row_spacing? integer|hw.Signal
---@field column_spacing? integer|hw.Signal
---@field homogeneous? boolean|hw.Signal
---@field children? hw.Child[]|hw.Signal

---@class hw.FlowBoxProps: hw.WidgetProps
---@field max_per_line? integer|hw.Signal
---@field max_children_per_line? integer|hw.Signal
---@field min_per_line? integer|hw.Signal
---@field min_children_per_line? integer|hw.Signal
---@field homogeneous? boolean|hw.Signal
---@field spacing? integer|hw.Signal
---@field children? hw.Child[]|hw.Signal

---@class hw.MenuButtonProps: hw.WidgetProps
---@field label? string|hw.Signal
---@field icon? string|hw.Signal
---@field always_show_arrow? boolean|hw.Signal
---@field active? boolean|hw.Signal
---@field child? hw.Child
---@field popover? hw.Node
---@field on_change? fun(self: hw.Widget, open: boolean)

---@class hw.PopoverProps: hw.WidgetProps
---@field has_arrow? boolean|hw.Signal
---@field autohide? boolean|hw.Signal
---@field open? boolean|hw.Signal
---@field child? hw.Child
---@field on_change? fun(self: hw.Widget, open: boolean)

---@class hw.DrawingAreaProps: hw.WidgetProps
---@field width? integer|hw.Signal Content width.
---@field content_width? integer|hw.Signal
---@field height? integer|hw.Signal Content height.
---@field content_height? integer|hw.Signal
---@field redraw? any|hw.Signal Any change redraws.
---@field draw? fun(cr: hw.Cairo, width: integer, height: integer)

---@class hw.CalendarProps: hw.WidgetProps
---@field date? string|hw.Signal `"YYYY-MM-DD"`
---@field show_heading? boolean|hw.Signal
---@field show_day_names? boolean|hw.Signal
---@field show_week_numbers? boolean|hw.Signal
---@field on_change? fun(self: hw.Widget, date: string)

---@class hw.TerminalProps: hw.WidgetProps
---@field command? string|string[] Default: `$SHELL`.
---@field argv? string|string[]
---@field cwd? string
---@field env? table<string, string>
---@field font? string|hw.Signal Pango font (`"JetBrainsMono Nerd Font 10"`).
---@field font_scale? number|hw.Signal
---@field colors? {foreground?: string, background?: string, cursor?: string, palette?: string[], [string]: string}|hw.Signal Default: `hw.theme.colors`.
---@field transparent? boolean|hw.Signal
---@field scrollback? integer
---@field size? {cols: integer, rows: integer}|integer[]
---@field cursor_blink? boolean|hw.Signal
---@field input? boolean|hw.Signal
---@field bold_is_bright? boolean|hw.Signal
---@field restart? hw.Restart
---@field send? string Written to the child's stdin when set.
---@field restart_now? boolean
---@field on_exit? fun(self: hw.Widget, status: integer)

---@class hw.AppProps: hw.WidgetProps
---@field command? string|string[] String = `sh -c`.
---@field env? table<string, string>
---@field size? integer[]|{width: integer, height: integer}
---@field mode? "resize"|"scale"
---@field keep_alive? boolean|hw.Signal
---@field restart? hw.Restart|boolean
---@field frame_when_hidden? boolean|hw.Signal
---@field socket? boolean Own WAYLAND_DISPLAY (apps that fork).
---@field rounding? number|false|hw.Signal Default: `decoration:rounding`.
---@field on_title? fun(self: hw.Widget, title: string)
---@field on_exit? fun(self: hw.Widget, code: integer)

---`{ type = "With", ... }`: one child rebuilt from `fn(value)`.
---@class hw.WithNode: hw.Node
---@field signal hw.Signal
---@field fn? fun(value: any): hw.Child

---`{ type = "For", ... }`: keyed list; extra keys are Box props.
---@class hw.ForNode: hw.Node, hw.BoxProps
---@field each hw.Signal
---@field fn fun(item: any, index: integer): hw.Child
---@field key? fun(item: any): any

---Cairo context passed to `DrawingArea.draw`.
---@class hw.Cairo
local Cairo = {}
---@param r number
---@param g number
---@param b number
---@param a number
function Cairo:set_source_rgba(r, g, b, a) end
---@param r number
---@param g number
---@param b number
function Cairo:set_source_rgb(r, g, b) end
---@param css string CSS color.
function Cairo:set_source_color(css) end
---@param w number
function Cairo:set_line_width(w) end
---@param cap "butt"|"round"|"square"
function Cairo:set_line_cap(cap) end
---@param x number
---@param y number
function Cairo:move_to(x, y) end
---@param x number
---@param y number
function Cairo:line_to(x, y) end
---@param x number
---@param y number
function Cairo:rel_line_to(x, y) end
---@param x1 number
---@param y1 number
---@param x2 number
---@param y2 number
---@param x3 number
---@param y3 number
function Cairo:curve_to(x1, y1, x2, y2, x3, y3) end
---@param x number
---@param y number
---@param w number
---@param h number
function Cairo:rectangle(x, y, w, h) end
---@param x number
---@param y number
---@param w number
---@param h number
---@param r number Corner radius.
function Cairo:rounded_rectangle(x, y, w, h, r) end
---@param xc number
---@param yc number
---@param r number
---@param a1 number
---@param a2 number
function Cairo:arc(xc, yc, r, a1, a2) end
---@param xc number
---@param yc number
---@param r number
---@param a1 number
---@param a2 number
function Cairo:arc_negative(xc, yc, r, a1, a2) end
function Cairo:new_path() end
function Cairo:close_path() end
function Cairo:fill() end
function Cairo:fill_preserve() end
function Cairo:stroke() end
function Cairo:stroke_preserve() end
---@param alpha? number
function Cairo:paint(alpha) end
function Cairo:clip() end
function Cairo:save() end
function Cairo:restore() end
---@param x number
---@param y number
function Cairo:translate(x, y) end
---@param x number
---@param y number
function Cairo:scale(x, y) end
---@param angle number Radians.
function Cairo:rotate(angle) end
---@param family string
---@param bold? boolean
function Cairo:select_font_face(family, bold) end
---@param size number
function Cairo:set_font_size(size) end
---@param text string
function Cairo:show_text(text) end
---@param text string
---@return {width: number, height: number, x_bearing: number, y_bearing: number, x_advance: number}
function Cairo:text_extents(text) end

---Widget constructors. `W.X(text_or_signal)` = `W.X{ label = ... }`.
---Widgets from `hw.define` and plugins are reachable by name too.
---@class hw.W
---@field Window fun(props: hw.WindowProps): hw.Node
---@field Box fun(props: hw.BoxProps): hw.Node
---@field CenterBox fun(props: hw.CenterBoxProps): hw.Node
---@field Label fun(props: hw.LabelProps|string|number|hw.Signal): hw.Node
---@field Button fun(props: hw.ButtonProps|string|hw.Signal): hw.Node
---@field Image fun(props: hw.ImageProps): hw.Node
---@field Icon fun(props: hw.IconProps): hw.Node
---@field Revealer fun(props: hw.RevealerProps): hw.Node
---@field Stack fun(props: hw.StackProps): hw.Node
---@field Scrolled fun(props: hw.ScrolledProps): hw.Node
---@field Entry fun(props: hw.EntryProps): hw.Node
---@field Slider fun(props: hw.SliderProps): hw.Node
---@field Switch fun(props: hw.SwitchProps): hw.Node
---@field Progress fun(props: hw.ProgressProps): hw.Node
---@field Overlay fun(props: hw.OverlayProps): hw.Node
---@field Grid fun(props: hw.GridProps): hw.Node
---@field FlowBox fun(props: hw.FlowBoxProps): hw.Node
---@field MenuButton fun(props: hw.MenuButtonProps|string|hw.Signal): hw.Node
---@field Popover fun(props: hw.PopoverProps): hw.Node
---@field DrawingArea fun(props: hw.DrawingAreaProps): hw.Node
---@field Calendar fun(props: hw.CalendarProps): hw.Node
---@field Terminal fun(props: hw.TerminalProps): hw.Node
---@field App fun(props: hw.AppProps): hw.Node
---@field Picture fun(props: hw.PictureProps): hw.Node
---@field With fun(signal: hw.Signal, fn?: (fun(value: any): hw.Child)): hw.WithNode
---@field For fun(signal: hw.Signal, fn: (fun(item: any, index: integer): hw.Child), key?: (fun(item: any): any)): hw.ForNode
---@field [string] fun(props: table|string|hw.Signal): hw.Node
