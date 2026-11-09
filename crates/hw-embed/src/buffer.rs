//! Client buffers → GDK textures, and the logical size of a surface.

use std::os::fd::AsRawFd;

use gtk4::{cairo, gdk, glib, prelude::*};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::Buffer as _;
use smithay::reexports::wayland_server::Resource;
use smithay::reexports::wayland_server::protocol::wl_buffer::WlBuffer;
use smithay::reexports::wayland_server::protocol::wl_output::Transform;
use smithay::utils::{Logical, Size};
use smithay::wayland::compositor::Damage;
use smithay::wayland::dmabuf::get_dmabuf;
use smithay::wayland::drm_syncobj::DrmSyncPoint;
use smithay::wayland::shm::with_buffer_contents;
use smithay::wayland::single_pixel_buffer::get_single_pixel_buffer;

/// Size of a buffer in buffer pixels.
pub(crate) fn buffer_size(buffer: &WlBuffer) -> Option<(i32, i32)> {
    if let Ok(d) = get_dmabuf(buffer) {
        return Some((d.width() as i32, d.height() as i32));
    }
    if get_single_pixel_buffer(buffer).is_ok() {
        return Some((1, 1));
    }
    with_buffer_contents(buffer, |_, _, data| (data.width, data.height)).ok()
}

/// Logical (surface-local) size: viewport destination, else viewport
/// source, else buffer size / scale with the transform applied.
pub(crate) fn logical_size(
    buffer: (i32, i32),
    scale: i32,
    transform: Transform,
    dst: Option<Size<i32, Logical>>,
    src: Option<Size<f64, Logical>>,
) -> Size<i32, Logical> {
    if let Some(d) = dst {
        return d;
    }
    if let Some(s) = src {
        return (s.w.round() as i32, s.h.round() as i32).into();
    }
    let scale = scale.max(1);
    let (w, h) = (buffer.0 / scale, buffer.1 / scale);
    match transform {
        Transform::_90 | Transform::_270 | Transform::Flipped90 | Transform::Flipped270 => {
            (h, w).into()
        }
        _ => (w, h).into(),
    }
}

/// Imports `buffer`. shm and single-pixel buffers are copied and released
/// right away; a dmabuf is released (with its syncobj release point) when
/// GDK drops the texture. `previous` lets GSK re-upload only the damage.
pub(crate) fn import(
    display: &gdk::Display,
    buffer: WlBuffer,
    damage: &[Damage],
    previous: Option<&gdk::Texture>,
    release: Option<DrmSyncPoint>,
    on_release: impl FnOnce() + Send + 'static,
) -> anyhow::Result<(gdk::Texture, bool)> {
    if let Ok(dmabuf) = get_dmabuf(&buffer) {
        let dmabuf = dmabuf.clone();
        let tex = dmabuf_texture(display, &dmabuf, buffer, release, on_release)?;
        return Ok((tex, true));
    }
    let result = if let Ok(px) = get_single_pixel_buffer(&buffer) {
        let bytes = glib::Bytes::from(&px.rgba8888());
        Ok(gdk::MemoryTexture::new(1, 1, gdk::MemoryFormat::R8g8b8a8, &bytes, 4).upcast())
    } else {
        shm_texture(&buffer, damage, previous)
    };
    buffer.release();
    if let Some(point) = release
        && let Err(e) = point.signal()
    {
        tracing::warn!("syncobj release: {e}");
    }
    result.map(|t| (t, false))
}

/// Releases a buffer that is not going to be shown.
pub(crate) fn discard(buffer: WlBuffer, release: Option<DrmSyncPoint>) {
    if let Some(point) = release {
        let _ = point.signal();
    }
    if buffer.is_alive() {
        buffer.release();
    }
}

fn dmabuf_texture(
    display: &gdk::Display,
    dmabuf: &Dmabuf,
    buffer: WlBuffer,
    release: Option<DrmSyncPoint>,
    on_release: impl FnOnce() + Send + 'static,
) -> anyhow::Result<gdk::Texture> {
    let format = dmabuf.format();
    let mut builder = gdk::DmabufTextureBuilder::new()
        .set_display(display)
        .set_width(dmabuf.width())
        .set_height(dmabuf.height())
        .set_fourcc(format.code as u32)
        .set_modifier(format.modifier.into())
        .set_premultiplied(true)
        .set_n_planes(dmabuf.num_planes() as u32);
    for (i, ((fd, offset), stride)) in dmabuf
        .handles()
        .zip(dmabuf.offsets())
        .zip(dmabuf.strides())
        .enumerate()
    {
        // SAFETY: the fd stays open while the `Dmabuf` (moved into the release
        // func below) is alive, i.e. until GDK is done with the texture.
        builder = unsafe { builder.set_fd(i as u32, fd.as_raw_fd()) }
            .set_offset(i as u32, offset)
            .set_stride(i as u32, stride);
    }
    let keep = dmabuf.clone();
    // SAFETY: see above; GDK calls the release func exactly once.
    let tex = unsafe {
        builder.build_with_release_func(move || {
            drop(keep);
            if let Some(point) = release
                && let Err(e) = point.signal()
            {
                tracing::warn!("syncobj release: {e}");
            }
            if buffer.is_alive() {
                buffer.release();
            }
            on_release();
        })
    }?;
    Ok(tex)
}

/// Copies an shm buffer into a `GdkMemoryTexture`.
fn shm_texture(
    buffer: &WlBuffer,
    damage: &[Damage],
    previous: Option<&gdk::Texture>,
) -> anyhow::Result<gdk::Texture> {
    use smithay::reexports::wayland_server::protocol::wl_shm::Format as ShmFormat;

    with_buffer_contents(buffer, |ptr, len, data| {
        let format = match data.format {
            ShmFormat::Argb8888 => gdk::MemoryFormat::B8g8r8a8Premultiplied,
            ShmFormat::Xrgb8888 => gdk::MemoryFormat::B8g8r8x8,
            ShmFormat::Abgr8888 => gdk::MemoryFormat::R8g8b8a8Premultiplied,
            ShmFormat::Xbgr8888 => gdk::MemoryFormat::R8g8b8x8,
            other => anyhow::bail!("unsupported shm format {other:?}"),
        };
        let start = data.offset as usize;
        let size = data.stride as usize * data.height as usize;
        anyhow::ensure!(start + size <= len, "shm buffer out of bounds");
        // SAFETY: smithay mapped `len` bytes at `ptr`; range checked above.
        let bytes = unsafe { std::slice::from_raw_parts(ptr.add(start), size) };
        let mut builder = gdk::MemoryTextureBuilder::new()
            .set_bytes(Some(&glib::Bytes::from(bytes)))
            .set_width(data.width)
            .set_height(data.height)
            .set_stride(data.stride as usize)
            .set_format(format);
        if let Some(prev) = previous
            .filter(|p| p.width() == data.width && p.height() == data.height && !damage.is_empty())
        {
            let region = cairo::Region::create();
            for d in damage {
                let (loc, size) = match d {
                    Damage::Surface(r) => (r.loc.into(), r.size.into()),
                    Damage::Buffer(r) => ((r.loc.x, r.loc.y), (r.size.w, r.size.h)),
                };
                let _ =
                    region.union_rectangle(&cairo::RectangleInt::new(loc.0, loc.1, size.0, size.1));
            }
            builder = builder
                .set_update_texture(Some(prev))
                .set_update_region(Some(&region));
        }
        Ok(builder.build())
    })?
}
