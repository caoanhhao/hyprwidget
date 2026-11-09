# M0.5 spike: apps inside a widget (`hw-embed`)

Spike for `PLAN.md` §6.6 steps 1–2, measured 2026-11-09.

- GPU: RTX 4070 (driver 615.78.08) + Intel UHD 770 (unused), Hyprland 0.57.1 on
  NVIDIA (`AQ_DRM_DEVICES=/dev/dri/nvidia:/dev/dri/intel-igpu`), DP-2 2560x1080
  @ 144 Hz, 8-bit `XRGB8888`, scale 1
- GTK 4.24.1 (`GSK_RENDERER=vulkan`), smithay 0.7.0, foot 1.28.0, vulkan-tools
  1.4.363, mpv 0.41.0, mesa-utils 9.0.0 (`es2gears_wayland`), egl-wayland2 1.0.2

Harness (not part of the `hyprwidget` binary):

```sh
cargo run -p hw-embed --example embed-test -- foot
RUST_LOG=info GDK_DEBUG=offload cargo run -p hw-embed --example embed-test -- vkcube --wsi wayland
```

It opens a 900x600 layer-shell window (top-right, `HW_EMBED_SIZE=WxH` to
change) containing a label and one `EmbedView`. `kill -USR1 <pid>` toggles the
view's visibility. `HW_EMBED_NO_SYNCOBJ=1` hides `linux-drm-syncobj-v1`;
`HW_EMBED_DRM_NODE=/dev/dri/renderDN` overrides the render node.

## Result

**Both done-criteria met.** foot renders inside the layer surface, and three
GPU clients (Vulkan WSI, EGL, mpv on both APIs) render through dmabuf +
`GtkGraphicsOffload` + explicit sync at the monitor/video rate, with no
smearing, tearing or stale frames visible in `grim` captures.

| Client | Buffer path | Format / modifier | syncobj | Rate | Offloaded |
|---|---|---|---|---|---|
| `foot` | shm → `GdkMemoryTexture` | `XRGB8888` | – | on damage | no (shm) |
| `vkcube --wsi wayland` | dmabuf | `XB30` (10-bit) / `0x300000000e08014` | yes | 144 fps | yes |
| `es2gears_wayland` (EGL, egl-wayland2) | dmabuf | `XR24` / `0x300000000606014` | yes | 144 fps | yes |
| `mpv --vo=gpu-next --gpu-api=vulkan` | dmabuf | `AB30` (10-bit) / `0x300000000e08014` | yes | 60 fps (video) | yes |
| `mpv --vo=gpu-next --gpu-api=opengl` | dmabuf | `XR24` / `0x300000000606014` | yes | 60 fps (video) | yes |

"Offloaded" = `GDK_DEBUG=offload` printed `Subsurface … created` and
`GdkDmabufTexture Attaching` every frame, i.e. GTK hands the client's buffer
straight to Hyprland as a subsurface of the layer surface; hyprwidget itself
never samples it. The modifiers are NVIDIA block-linear layouts, so the
format list from `gdk::Display::dmabuf_formats()` (757 entries) is what the
NVIDIA clients picked from.

Frame pacing: with the view hidden (`kill -USR1`) es2gears made **zero**
commits for 7 s and went straight back to 144 fps when shown.

Memory (release build, 16 s): RSS 223 MB with foot, 226 MB with vkcube
(RssAnon 48 / 62 MB, RssFile ~145 MB, mostly NVIDIA driver mappings). Flat
between 4 s and 16 s, fd count stable → buffers and textures are released.
That is ~60 MB above the bare M0 window (`docs/renderer.md`); not yet
attributed (suspect: GDK initialising EGL as well as Vulkan for the dmabuf
format query). Worth measuring before M5.

## How it works

`crates/hw-embed/src/server.rs`, `view.rs`.

- **Server on the GLib loop.** `smithay::reexports::wayland_server::Display`
  is dispatched from `glib_unix::unix_fd_add_local` on its poll fd; no thread,
  no calloop. Display and state live in `RefCell`s on the GTK thread; widget
  callbacks use `try_borrow` so they never re-enter a dispatch.
  Note: in glib-rs 0.22 the unix fd/signal sources moved to the separate
  `glib-unix` crate.
- **Globals:** `wl_compositor` v5, `wl_subcompositor`, `wl_shm`, `xdg_wm_base`,
  `zxdg_decoration_manager_v1` (always `server_side`), `wl_seat` (pointer +
  keyboard, US keymap), `wl_output` (fake 1920x1080@60, scale 1),
  `wl_data_device_manager` (stub), `wp_viewporter`, `zwp_linux_dmabuf_v1` (v4+)
  with default feedback, `wp_linux_drm_syncobj_manager_v1`.
- **Spawn:** `UnixStream::pair()` → `DisplayHandle::insert_client` with a
  per-client `ClientState { view }`; the other end is un-`CLOEXEC`ed in
  `pre_exec` and passed as `WAYLAND_SOCKET`, plus `GDK_BACKEND=wayland`,
  `QT_QPA_PLATFORM=wayland`, `SDL_VIDEODRIVER=wayland`,
  `XDG_SESSION_TYPE=wayland`; `DISPLAY` and `WAYLAND_DISPLAY` removed. The
  child is reaped with `glib::child_watch_add_local`.
- **`EmbedView`** = `GtkWidget` subclass › `GtkGraphicsOffload` ›
  `GtkPicture` (`ContentFit::Fill`). `size_allocate` sends
  `xdg_toplevel.configure(w, h, [maximized, activated, tiled_*])`; smithay
  only emits it when the pending state changed. The first configure answers
  the client's initial commit with the current allocation.
- **shm:** one copy into a new `GdkMemoryTexture`; when the size is unchanged
  the previous texture and the surface damage are passed as
  `update_texture`/`update_region`, so GSK re-uploads only the damage. The
  `wl_buffer` is released immediately after the copy.
- **dmabuf:** planes go into `GdkDmabufTextureBuilder` (fourcc, modifier,
  fds, offsets, strides, premultiplied). `build_with_release_func` keeps the
  `Dmabuf` alive and, when GDK drops the texture, signals the syncobj release
  point, sends `wl_buffer.release` and schedules a flush (idle, because the
  release func must be `Send`).
- **Acquire:** if the commit carries an acquire point, its
  `drmSyncobjEventfd` (`DrmSyncPoint::eventfd`) is watched with
  `unix_fd_add_local`; the texture is handed to the picture only when it
  fires.
- **DRM node:** first entry of `AQ_DRM_DEVICES` mapped to its render node
  via `/sys/class/drm/cardN/device/drm/renderD*` (here `renderD129`, NVIDIA),
  fallback `renderD128`. Used as the dmabuf feedback main device and for
  syncobj (`supports_syncobj_eventfd` = true on 615.78).
- **Frame callbacks** are collected on commit and answered from a one-shot
  `add_tick_callback` (time = frame clock time). Not installed while
  unmapped; `unmap` cancels a pending one, `map` re-arms if callbacks wait.
- **Input stub:** `EventControllerMotion`/`GestureClick`/`EventControllerKey`
  → `wl_pointer` motion/button and `wl_keyboard` key (GDK hardware keycode =
  XKB keycode). Wired up but **not exercised** in this spike.

## NVIDIA notes

- Explicit sync works end to end: Vulkan WSI and egl-wayland2 both bind
  `wp_linux_drm_syncobj_manager_v1` and attach acquire/release points to
  every dmabuf commit; eventfd waits fire on the GLib loop with no visible
  latency (rate stays locked to 144 Hz).
- With `HW_EMBED_NO_SYNCOBJ=1` vkcube and es2gears also looked clean in
  captures. Driver 615 apparently attaches implicit fences to the dmabufs.
  `grim` captures cannot prove the absence of a transient tear, so explicit
  sync stays the default; the comparison only shows that dropping it is not
  an instant failure.
- Clients pick **10-bit formats** (`XB30`, `AB30`) when offered. They
  displayed correctly on the 8-bit DP-2 output (Hyprland converts when
  compositing the subsurface). The earlier DP-2 smearing was the *output*
  running at 10-bit, not 10-bit client buffers. If it ever recurs, filter
  `*30` fourccs out of the advertised list.
- Opening the render node logs smithay's harmless *"Unable to become drm
  master"* warning.
- The NVIDIA device must be the feedback main device; advertising the Intel
  node would make NVIDIA EGL/Vulkan clients allocate on the wrong GPU.

## Not working / gaps (→ M5)

- **`WAYLAND_SOCKET` allows one connection.** mpv opens a second one for its
  clipboard backend and then fails *"Failed initializing any suitable GPU
  context"*; works with `--clipboard-backends-clr`. Fix: the socket mode from
  §6.2.1 (`WAYLAND_DISPLAY=hyprwidget-embed-N`).
- **Subsurfaces are ignored**, only the toplevel's root surface is shown.
  Fine for foot/vkcube/es2gears/mpv-gpu-next; `mpv --vo=dmabuf-wayland`
  fails (needs subsurfaces and `wp_single_pixel_buffer`), and GTK/Qt apps with
  subsurface-based content will be incomplete.
- `xdg_popup` (menus) not implemented; popups are created but never shown.
- Missing protocols foot reports: primary selection, xdg-activation,
  fractional-scale, cursor-shape, xdg-toplevel-icon, text-input-v3.
  es2gears tries libdecor-gtk and falls back.
- `wp_viewporter` destination size and `buffer_scale`/transform are not
  applied; the picture is simply stretched to the widget (`Fill`).
- Only one toplevel per client; a second one is ignored.
- No placeholder/restart on client exit; a dropped view sends
  `xdg_toplevel.close` but no SIGTERM timeout.
- Keyboard focus/`text-input-v3`, keymap from Hyprland, cursor images,
  clipboard: M5.
- Apps tested only: foot, vkcube, es2gears_wayland, mpv. GTK/Qt apps are the
  M5 matrix.
