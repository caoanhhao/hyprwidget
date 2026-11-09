# sysmon — plugin mẫu

Đồng hồ CPU / RAM / GPU dựng trên `hw.service.sysinfo`. Không cần quyền nào
(`"permissions": []`), nên chạy trong sandbox chặt nhất: không `hw.exec`, không
`io`, không `os.execute`.

| File | |
|---|---|
| `plugin.json` | manifest: tên, version, quyền, widget khai báo trước (`Sysmon`, `SysmonMeter`) |
| `init.lua` | trả về `{ widgets, services, requests, setup }` |
| `lib/format.lua` | module riêng của plugin (`require("lib.format")`) |
| `style.scss` | biên dịch trước stylesheet của người dùng, có sẵn `@import "hypr"` và `"hw-theme"` |

Cung cấp:

- `W.Sysmon { items = { "cpu", "mem", "gpu" }, vertical = true }` và
  `W.SysmonMeter { name = "cpu" }` — cũng dùng được trong layout JSON
  (`{ "type": "Sysmon", "items": ["cpu"] }`, xem `layouts/sysmon.json`);
- `hw.service.sysmon.summary` — Signal chuỗi tóm tắt;
- `hyprwidget request sysmon` — in chuỗi tóm tắt.

```sh
hyprwidget -c examples/demo check
hyprwidget toggle sysmon-DP-2
hyprwidget request sysmon
```

Hướng dẫn đầy đủ về plugin (manifest, sandbox, quyền, theme):
[`docs/plugins.md`](../../../../docs/plugins.md).
