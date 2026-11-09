# Memory (M6)

Goal in PLAN §2: **< 80 MB RSS after 24h, no growth over time** (AGS/gjs: 2.8 GB
RSS after 28h). Measured on 2026-11-09 on the dev machine: RTX 4070 (NVIDIA
driver), Hyprland 0.57, GTK 4.24.1, renderer `vulkan` (default), a **release**
build of the M6 commit, the `examples/demo` config (full AGS port: bar, left/right
panels, launcher, notifications, user panel, wallpaper switcher, sysmon + btop in
`W.Terminal`…) on one screen (DP-2).

## Conclusions

- **No unbounded memory leak found.** RssAnon grows in steps as each window is
  opened for the first time (textures, glyphs, icons, CSS images, Vulkan pipelines,
  VTE + btop…) and then **stays flat**: the last 4 minutes of run 2 and 5 idle
  minutes did not change (±8 KB); opening/closing the same window 10–30 times no
  longer grows (0–180 KB / 10 times).
- **The < 80 MB budget is not met** with the full demo: ~73 MB anon right at
  startup, **105–150 MB** after every window has been opened. The bare framework
  (1 window, 1 label) is 41 MB. Since M6, hidden windows are built lazily:
  **~56.5 MB** at startup (see [Lazy window construction](#lazy-window-construction));
  the level after opening every window is unchanged.
- `VmRSS` (~330–380 MB) is not a suitable metric on NVIDIA: ~150–180 MB of it is
  driver libraries mapped from files (shared between processes). Use **RssAnon**
  (the process's private memory) and **PSS**.
- Most of it is memory really in use, not fragmentation: `malloc_stats` reports the
  main arena at 83 MB "in use" / 87 MB "system"; `malloc_trim(0)` only returns 4 MB.
  The Lua heap is just 2–3 MB (`collectgarbage("count")`).

## Method

Script (in the M6 session's scratchpad, summarized here):

1. `cargo build --release`, run `hyprwidget run -c examples/demo -i memtest`.
2. Every minute, read `/proc/<pid>/status` (`VmRSS`, `RssAnon`, `RssFile`,
   `RssShmem`) and `/proc/<pid>/smaps_rollup` (`Pss`, `Pss_Anon`, `Pss_File`), the
   fd count and the thread count.
3. Every 30 s, open a window with `hyprwidget toggle <window>` for ~4 s and close
   it, rotating through right-panel, left-panel, user-panel, notification-history,
   wallpaper-switcher, sysmon; every 2 minutes open/close the launcher; every round
   `hyprwidget request sysmon`.
4. Run 2 runs under `gdb` (with `ptrace_scope = 1`, gdb must be the parent process)
   to call `malloc_stats()` / `malloc_trim(0)` on `SIGUSR2`, and measures each
   window separately.

## Results

### Run 1 — 15 minutes from startup (RssAnon, MB)

| minute | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 15 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| RssAnon | 80.2 | 100.1 | 111.3 | 122.7 | 128.1 | 134.4 | 136.3 | 138.9 | 146.8 | 146.8 | 148.5 | 148.4 | 148.8 | 148.9 | 148.9 | 152.1 |
| PSS | 123.6 | 146.6 | 171.2 | 193.4 | 195.7 | 192.9 | 196.9 | 201.2 | 211.8 | 211.9 | 206.8 | 220.0 | 225.0 | 225.1 | 225.1 | 210.0 |

The 6-window rotation takes 3 minutes, so the first 3–4 minutes are first-open
costs. This run also had actions outside the script (`hyprwidget eval` dumping
every prop of every service, starting keystrokes/brightness too; 8 fcitx5 input
method switches to test the tray menu), so its level is higher than run 2. fds
ranged 128–256 (GTK/VTE/subprocesses), threads 27–34.

### Run 2 — per window (RssAnon / `[heap]`, MB)

| Step | 1st open | 2nd open |
|---|---|---|
| startup | 73.0 / 49.5 | |
| right-panel | +3.1 | +0.2 |
| left-panel | +3.2 | +0.2 |
| user-panel | +6.4 | 0 |
| notification-history | 0 | +0.7 |
| wallpaper-switcher | +8.4 | +6.6 (opens 3–7: +0.02) |
| sysmon (VTE + btop) | +3.9 | 0 |
| app-launcher | +1.0 | −0.2 |
| **total** | **99.0** / 74.6 | **106.6** / 82.1 |

The same process then ran the 15-minute script: 102.9 → 107.5 (first 10 minutes,
~0.45 MB/minute) → a +6.3 step at minute 11 → **113.7 flat** for the last 4
minutes. 6 idle minutes without interaction: 113.75 → 113.76, a +4 MB step at
minute 6 (the demo's periodic polls: weather, crypto…). Then opening/closing each
window 10 times: user-panel +9.3 MB, left-panel +2.5 MB, other windows ±0.2 MB;
user-panel another 3 × 10 times: +0.18, +0.01, 0 MB → saturated.
50 × `request sysmon`: 0.

### Breakdown

| | RssAnon |
|---|---|
| Bare framework, renderer `vulkan` / `gl` / `cairo` | 40.6 / 41.3 / 27.3 MB |
| Full demo at startup, `vulkan` / `cairo` | 73.0 / 58.7 MB |
| Full demo at startup, lazy windows, `vulkan` | 56.5 MB |
| Demo after opening every window twice, `vulkan` / `cairo` | 106.6 / 85.3 MB |

- **NVIDIA driver (Vulkan)**: ~14 MB at startup, ~21 MB after opening every window
  (vulkan − cairo difference).
- **Building every window upfront**: ~32 MB (73 − 41). Every `per_monitor` window of
  the demo (15 windows, mostly hidden) had its whole widget tree + style built at
  startup. Of that, ~16 MB is the 11 hidden windows (now built lazily); the other
  ~16 MB is the windows shown from the start (bar, always-on, 2 hovers) and the
  services/Lua state the demo starts.
- **Caches after the first open**: ~26–40 MB (image textures/thumbnails, glyphs,
  icon theme, GPU pipelines, VTE).
- Within anon: the main thread's `[heap]` is ~75% (82 / 107 MB); the worker
  threads' arenas add up to < 12 MB.

## Lazy window construction

A top-level `W.Window{ visible = false, … }` (or a `visible` signal that is
`false`) only creates the layer window, sets the window's props/classes/events and
registers its name; the child tree (the `child` slot) is built the first time the
window is shown — `hyprwidget toggle`, `hw.window.get(n):show()` / `:toggle()`, the
`visible` signal turning `true` — **before** `present()`, so the layer surface is
mapped at its final size (`hw_ui::windows::present`, `defer_content`;
`connect_show` as a fallback). `hyprwidget list`, `toggle`, `per_monitor` rebuilds,
`reload lua` and `check` (headless, builds no windows) are unchanged. Errors
building the child tree on show are logged (`window '<name>': …`) instead of
failing config loading. Consequence: `:widget()` of a window never shown has no
children yet.

Re-measured 2026-11-09 (same machine, release, `hyprwidget run -c examples/demo`,
RssAnon after ~21 s, 3 interleaved runs):

| | run 1 | run 2 | run 3 |
|---|---|---|---|
| before (commit `4578c0f`) | 76.7 | 72.8 | 72.8 MB |
| lazy construction | 56.2 | 56.8 | 56.7 MB |

Layer sizes on first open are identical to the old version (right-panel 308×980).
The level after opening every window has not been re-measured with the run 2
script; the build cost only moves to the first open.

## Bugs fixed in M6

**`Gtk-WARNING: duplicate child name in GtkStack: Charset`** (and the widget leak
that came with it): `GtkPopoverMenu` adds a stack page for each submenu but does
**not remove** that page when the *section* holding the submenu is removed from the
model (GTK 4.24). The tray's DBusMenu menus were updated in place (`remove_all` +
rebuild) whenever an app changed its layout — fcitx5 changes its menu with the
focused window's input method (Lotus has a "Charset" submenu) — so every focus
change leaked a page and printed the warning. Reproduced with a PyGObject script:
the popover's widget count grew 37 → 47 → 56 → 66 over each rebuild. Fix
(`crates/hw-services/src/tray/menu.rs`): every layout change creates a **new model**
and reassigns it to the attached `MenuButton`/`PopoverMenu`
(`DbusMenu::connect_model`); GTK builds a new popover and frees the old one. Check:
8 input method switches while the bar was running → 0 warnings.

The remaining warning in the log is GTK's, not hyprwidget's:
`gdk_frame_timings_throttling_hint: runtime check failed` (a few times a minute as
windows are shown/hidden).

## Not done yet / ways to reduce

- The full **24h** run has not been done; the "no growth" conclusion rests on ~35
  minutes (2 runs) that were flat at the end of each run.
- To get under 80 MB with the full demo:
  1. ~~**Lazy window construction**~~ (done, −16 MB at startup, see above); still
     open: an option to destroy the tree after being hidden for a long time, to give
     back memory after the first open.
  2. Wallpaper switcher (+15 MB over its first two opens): on every `on_show` it
     rebuilds the `Picture`s that use CSS `background-image` of the thumbnail files;
     re-measure with `W.Picture` (texture freed with the widget) instead of CSS
     background images.
  3. `GSK_RENDERER=cairo` for low-animation setups: −15 to −21 MB anon, at the cost
     of CPU rendering.
- Run 1 is ~35–40 MB higher than run 2 because of the actions outside the script
  (extra services started, data dumped through `eval`); the individual sources have
  not been separated.
