# GSK renderer on NVIDIA

Measured for M0 (2026-11-09) with `hyprwidget run -c examples/demo` (release
build, one layer-shell window with a label), each renderer run alone.

- GPU: RTX 4070, driver 615.78.08 · GTK 4.24.1 · gtk4-layer-shell 1.3.0 · Hyprland, DP-2 scale 1
- Values from `/proc/<pid>/status` and `smaps_rollup`, in MB (KiB / 1024, rounded)

| `GSK_RENDERER` | Renderer picked | RSS 3 s | RSS 60 s | RssAnon | RssFile | PSS 60 s | VRAM | Errors |
|---|---|---|---|---|---|---|---|---|
| `vulkan` | GskVulkanRenderer | 165.4 | 165.6 | 34.6 | 114.2 | 101.5 | 5 MiB | none |
| *(unset)* | GskVulkanRenderer | 165.4 | 165.6 | 34.6 | 114.2 | 101.6 | 5 MiB | none |
| `gl` | GskGLRenderer | 168.7 | 168.9 | 36.5 | 123.5 | 101.6 | n/a¹ | none |
| `ngl` | GskGLRenderer | 167.9 | 168.1 | 35.2 | 124.0 | 100.6 | n/a¹ | warning: *"The new GL renderer has been renamed to gl"* |
| `cairo` (reference) | GskCairoRenderer | 104.2 | 104.4 | 22.6 | 81.7 | 60.2 | – | none |

¹ `nvidia-smi --query-compute-apps` only lists Vulkan (C+G) contexts; the GL
process shows as type `G` in `pmon` without a memory figure.

Notes:

- No growth over 60 s idle for any renderer (anon memory identical at 3 s and 60 s).
- `ngl` is a deprecated alias of `gl` in GTK 4.24; use `gl`.
- Most of the RSS is file-backed mappings of the NVIDIA userspace driver
  (`RssFile` ≈ 115–125 MB, shared with every other GL/Vulkan client), so RSS
  overstates the real cost. Track **RssAnon/PSS** against the < 80 MB budget in
  `PLAN.md`; the 24 h leak test belongs to later milestones.
- A first, *concurrent* run (three instances started at once) showed `ngl` at
  91 MB anon vs 35–52 MB for Vulkan — likely GL shader-cache compilation; it did
  not reproduce when run alone.

## Decision

Default: **Vulkan** — lowest anon/RSS, no warnings, and already GTK 4.24's own
choice on this machine. `hyprwidget run` sets `GSK_RENDERER=vulkan` only when
the variable is unset, so `GSK_RENDERER=gl hyprwidget run` still works as a
fallback if Vulkan misbehaves (e.g. with dmabuf/syncobj in M0.5).
