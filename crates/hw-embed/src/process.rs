//! Spawning embedded apps: `WAYLAND_SOCKET` (one connection, exact) or a
//! per-session listening socket (`WAYLAND_DISPLAY=hyprwidget-embed-N`) for
//! apps that open several connections or fork.

use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use gtk4::glib;

use crate::server::{EmbedServer, ViewId};

/// A listening socket; unlinked when dropped.
pub(crate) struct SocketGuard {
    path: PathBuf,
    source: Option<glib::SourceId>,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Some(id) = self.source.take() {
            id.remove();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn runtime_dir() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Binds the first free `hyprwidget-embed-N`, removing stale sockets.
fn bind_socket() -> io::Result<(String, PathBuf, UnixListener)> {
    let dir = runtime_dir();
    for n in 1..1000 {
        let name = format!("hyprwidget-embed-{n}");
        let path = dir.join(&name);
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                continue;
            }
            let _ = std::fs::remove_file(&path);
        }
        match UnixListener::bind(&path) {
            Ok(l) => return Ok((name, path, l)),
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free hyprwidget-embed-N socket"))
}

pub(crate) struct Spawned {
    pub pid: u32,
    pub socket: Option<SocketGuard>,
}

/// Starts `argv` as a client of session `id`. The child leads its own
/// process group so it can be terminated with everything it forked.
pub(crate) fn spawn(
    server: &EmbedServer,
    id: ViewId,
    argv: &[String],
    env: &[(String, String)],
    socket: bool,
) -> anyhow::Result<Spawned> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env("GDK_BACKEND", "wayland")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("SDL_VIDEODRIVER", "wayland")
        .env("CLUTTER_BACKEND", "wayland")
        .env("MOZ_ENABLE_WAYLAND", "1")
        .env("XDG_SESSION_TYPE", "wayland")
        // Typing goes through text-input-v3 → hyprwidget's own IM context.
        .env("GTK_IM_MODULE", "wayland")
        .env("QT_IM_MODULE", "wayland")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("WAYLAND_DISPLAY")
        .process_group(0);
    cmd.envs(env.iter().map(|(k, v)| (k, v)));

    let mut guard = None;
    let mut client_end = None;
    if socket {
        let (name, path, listener) = bind_socket()?;
        listener.set_nonblocking(true)?;
        let fd = listener.as_raw_fd();
        let srv = server.clone();
        let source = glib_unix::unix_fd_add_local(fd, glib::IOCondition::IN, move |_, _| {
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        if let Err(e) = srv.connect(id, stream) {
                            tracing::warn!("embed socket: {e:#}");
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => {
                        tracing::warn!("embed socket accept: {e}");
                        return glib::ControlFlow::Break;
                    }
                }
            }
            glib::ControlFlow::Continue
        });
        cmd.env("WAYLAND_DISPLAY", &name);
        guard = Some(SocketGuard {
            path,
            source: Some(source),
        });
    } else {
        let (server_end, client) = UnixStream::pair()?;
        server.connect(id, server_end)?;
        let fd = client.as_raw_fd();
        cmd.env("WAYLAND_SOCKET", fd.to_string());
        // SAFETY: only async-signal-safe fcntl between fork and exec.
        unsafe {
            cmd.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        client_end = Some(client);
    }
    let child = cmd.spawn()?;
    drop(client_end);
    server.flush();
    tracing::info!(pid = child.id(), ?argv, socket, "spawned embedded app");
    Ok(Spawned {
        pid: child.id(),
        socket: guard,
    })
}

/// Sends `sig` to the app's process group (falls back to the pid).
pub(crate) fn signal(pid: u32, sig: i32) {
    let pid = pid as libc::pid_t;
    // SAFETY: plain kill(2).
    unsafe {
        if libc::kill(-pid, sig) == -1 {
            libc::kill(pid, sig);
        }
    }
}

/// Exit code from a wait status (128 + signal for killed processes).
pub(crate) fn exit_code(status: i32) -> i32 {
    if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else if libc::WIFSIGNALED(status) {
        128 + libc::WTERMSIG(status)
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn exit_codes() {
        assert_eq!(super::exit_code(0), 0);
        assert_eq!(super::exit_code(3 << 8), 3);
        assert_eq!(super::exit_code(libc::SIGTERM), 128 + libc::SIGTERM);
    }
}
