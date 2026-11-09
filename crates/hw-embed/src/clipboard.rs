//! `wl_data_device` / `primary-selection` ↔ `GdkClipboard`, both ways.
//!
//! - An embedded client sets a selection → a [`ClientContent`] provider
//!   becomes the GDK clipboard's content; reading it asks the client to
//!   write into a pipe.
//! - The GDK clipboard changes (hyprwidget itself or another app) → the
//!   server offers its mime types; a client reading it gets the bytes from
//!   `gdk_clipboard_read_async`.

use std::io::{Read, Write};
use std::os::fd::OwnedFd;

use gtk4::gdk::subclass::prelude::*;
use gtk4::{gdk, gio, glib, prelude::*};
use smithay::input::Seat;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::wayland::selection::data_device::{
    clear_data_device_selection, request_data_device_client_selection, set_data_device_selection,
};
use smithay::wayland::selection::primary_selection::{
    clear_primary_selection, request_primary_client_selection, set_primary_selection,
};
use smithay::wayland::selection::{SelectionHandler, SelectionSource, SelectionTarget};

use crate::server::{EmbedServer, State};

pub(crate) struct Clipboards {
    clipboard: gdk::Clipboard,
    primary: gdk::Clipboard,
}

impl Clipboards {
    pub(crate) fn new(display: &gdk::Display) -> Self {
        let this = Self {
            clipboard: display.clipboard(),
            primary: display.primary_clipboard(),
        };
        for (cb, target) in [
            (&this.clipboard, SelectionTarget::Clipboard),
            (&this.primary, SelectionTarget::Primary),
        ] {
            cb.connect_changed(move |_| {
                if let Some(s) = EmbedServer::current() {
                    s.run(move |st| st.clipboards.sync(&st.dh, &st.seat, target));
                }
            });
        }
        this
    }

    fn get(&self, target: SelectionTarget) -> &gdk::Clipboard {
        match target {
            SelectionTarget::Clipboard => &self.clipboard,
            SelectionTarget::Primary => &self.primary,
        }
    }

    pub(crate) fn sync_all(&self, dh: &DisplayHandle, seat: &Seat<State>) {
        self.sync(dh, seat, SelectionTarget::Clipboard);
        self.sync(dh, seat, SelectionTarget::Primary);
    }

    /// GDK clipboard → server selection (unless it came from a client).
    fn sync(&self, dh: &DisplayHandle, seat: &Seat<State>, target: SelectionTarget) {
        let cb = self.get(target);
        if cb.content().is_some_and(|c| c.is::<ClientContent>()) {
            return;
        }
        let mimes: Vec<String> = cb
            .formats()
            .mime_types()
            .iter()
            .map(ToString::to_string)
            .collect();
        match (target, mimes.is_empty()) {
            (SelectionTarget::Clipboard, false) => set_data_device_selection(dh, seat, mimes, ()),
            (SelectionTarget::Primary, false) => set_primary_selection(dh, seat, mimes, ()),
            (SelectionTarget::Clipboard, true) => clear_data_device_selection(dh, seat),
            (SelectionTarget::Primary, true) => clear_primary_selection(dh, seat),
        }
    }
}

impl SelectionHandler for State {
    type SelectionUserData = ();

    fn new_selection(
        &mut self,
        ty: SelectionTarget,
        source: Option<SelectionSource>,
        _seat: Seat<Self>,
    ) {
        let Some(source) = source else {
            return;
        };
        let content = ClientContent::new(ty, source.mime_types());
        self.clipboards
            .get(ty)
            .set_content(Some(&content))
            .unwrap_or_else(|e| tracing::warn!("clipboard: {e}"));
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        _user_data: &(),
    ) {
        let cb = self.clipboards.get(ty).clone();
        glib::spawn_future_local(async move {
            let bytes = match read_all(&cb, &mime_type).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::debug!("clipboard read {mime_type}: {e}");
                    return;
                }
            };
            let _ = gio::spawn_blocking(move || {
                let _ = std::fs::File::from(fd).write_all(&bytes);
            })
            .await;
        });
    }
}

async fn read_all(cb: &gdk::Clipboard, mime: &str) -> Result<Vec<u8>, glib::Error> {
    let (input, _) = cb.read_future(&[mime], glib::Priority::DEFAULT).await?;
    let mut out = Vec::new();
    loop {
        let chunk = input
            .read_bytes_future(64 * 1024, glib::Priority::DEFAULT)
            .await?;
        if chunk.is_empty() {
            break;
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;
    let mut fds = [0; 2];
    // SAFETY: plain pipe2 into a local array.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the read end was just created and is owned by nobody else.
    let read = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    // SAFETY: the write end was just created and is owned by nobody else.
    let write = unsafe { OwnedFd::from_raw_fd(fds[1]) };
    Ok((read, write))
}

glib::wrapper! {
    /// GDK clipboard content backed by an embedded client's selection.
    pub(crate) struct ClientContent(ObjectSubclass<imp::ClientContent>)
        @extends gdk::ContentProvider;
}

impl ClientContent {
    fn new(target: SelectionTarget, mimes: Vec<String>) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().target.set(Some(target));
        obj.imp().mimes.replace(mimes);
        obj
    }
}

mod imp {
    use std::cell::{Cell, RefCell};
    use std::future::Future;
    use std::pin::Pin;

    use gtk4::gdk::subclass::prelude::*;
    use gtk4::prelude::*;

    use super::{
        EmbedServer, Read, SelectionTarget, gdk, gio, glib, pipe,
        request_data_device_client_selection, request_primary_client_selection,
    };

    #[derive(Default)]
    pub(crate) struct ClientContent {
        pub(super) target: Cell<Option<SelectionTarget>>,
        pub(super) mimes: RefCell<Vec<String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ClientContent {
        const NAME: &'static str = "HwEmbedClientContent";
        type Type = super::ClientContent;
        type ParentType = gdk::ContentProvider;
    }

    impl ObjectImpl for ClientContent {}

    impl ContentProviderImpl for ClientContent {
        fn formats(&self) -> gdk::ContentFormats {
            let mimes = self.mimes.borrow();
            let refs: Vec<&str> = mimes.iter().map(String::as_str).collect();
            gdk::ContentFormats::new(&refs)
        }

        fn write_mime_type_future(
            &self,
            mime_type: &str,
            stream: &gio::OutputStream,
            io_priority: glib::Priority,
        ) -> Pin<Box<dyn Future<Output = Result<(), glib::Error>> + 'static>> {
            let target = self.target.get();
            let mime = mime_type.to_owned();
            let stream = stream.clone();
            Box::pin(async move {
                let err = |m: String| glib::Error::new(gio::IOErrorEnum::Failed, &m);
                let (read, write) = pipe().map_err(|e| err(e.to_string()))?;
                let server = EmbedServer::current().ok_or_else(|| err("no server".into()))?;
                let requested = server
                    .with_state(|st| match target {
                        Some(SelectionTarget::Primary) => {
                            request_primary_client_selection(&st.seat, mime, write)
                                .map_err(|e| e.to_string())
                        }
                        _ => request_data_device_client_selection(&st.seat, mime, write)
                            .map_err(|e| e.to_string()),
                    })
                    .ok_or_else(|| err("server busy".into()))?;
                requested.map_err(err)?;
                let bytes = gio::spawn_blocking(move || {
                    let mut buf = Vec::new();
                    std::fs::File::from(read).read_to_end(&mut buf).map(|_| buf)
                })
                .await
                .map_err(|_| err("reader panicked".into()))?
                .map_err(|e| err(e.to_string()))?;
                stream
                    .write_all_future(bytes, io_priority)
                    .await
                    .map_err(|(_, e)| e)?;
                Ok(())
            })
        }
    }
}
