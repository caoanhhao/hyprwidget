# Settings (`hw.settings`)

Kho settings bền vững, **tương thích `settings.json` của AGS**: file là một object
JSON lồng nhau; một lá là giá trị thường hoặc object `AGSSetting`
`{ name, value, type, min?, max? }`. Key là path có dấu chấm
(`"hyprland.decoration.rounding"`) và luôn trỏ tới **giá trị** (`value` của
AGSSetting). Ghi được debounce 500 ms và ghi nguyên tử (file tạm + rename).

File: `settings_file` trong `config.json`, mặc định
`<config>/cache/settings/settings.json`.

```lua
hw.settings:get("bar.lock")              -- giá trị hiện tại (table/number/…)
hw.settings:set("bar.lock", true)        -- trả về giá trị đã lưu (đã kiểm tra kiểu / kẹp min..max)
hw.settings:toggle("notifications.dnd")  -- bool → trả về giá trị mới
hw.settings:meta("hyprland.decoration.rounding")  -- { name, value, type, min, max } hoặc nil
hw.settings:bind("bar.lock")             -- Signal ghi được (= hw.bind(hw.settings, "bar.lock"))
hw.settings["bar.lock"]                  -- cũng là Signal đó

W.Switch {
  active = hw.settings["bar.lock"],
  on_change = function(_, on) hw.settings:set("bar.lock", on) end,
}
```

- Với key là AGSSetting, `set` kiểm tra theo `type`: `int` (làm tròn rồi kẹp
  `min`..`max`), `float` (kẹp `min`..`max`), `bool`/`boolean`, `string`/`str`; sai
  kiểu thì báo lỗi.
- Set key chưa có sẽ tạo các object trung gian.
- Signal của một key cha (`hw.settings.bar`) đổi khi bất kỳ key con nào đổi.
- Settings sống qua các lần reload Lua; nhiều widget bind cùng key dùng chung một
  Signal.
- Trong layout JSON: `{"$bind": "settings.bar.lock"}` (xem [widgets.md](widgets.md#layout-json)).

Demo (`examples/demo/config/bar/defaults.lua`, `config/panels/defaults.lua`) gieo
giá trị mặc định của AGS cho key còn thiếu, nên có thể trỏ `settings_file` vào
`~/.config/ags/cache/settings/settings.json` cũ.
