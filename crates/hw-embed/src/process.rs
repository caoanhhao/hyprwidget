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
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from)
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
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no free hyprwidget-embed-N socket"))
}

/// How the client reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Connection {
    /// One pre-connected `WAYLAND_SOCKET`.
    Inherited,
    /// A listening `WAYLAND_DISPLAY` socket, for several connections.
    Listener,
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
    connection: Connection,
) -> anyhow::Result<Spawned> {
    let socket = connection == Connection::Listener;
    let (program, program_args) = argv
        .split_first()
        .ok_or_else(|| anyhow::anyhow!("empty command"))?;
    let mut cmd = Command::new(program);
    cmd.args(program_args)
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
    // The app must not outlive hyprwidget (socket-mode apps survive a lost
    // display).
    let pdeathsig = || {
        // SAFETY: prctl(PR_SET_PDEATHSIG) takes no pointers and is
        // async-signal-safe.
        unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
        Ok(())
    };
    // SAFETY: the hook only calls async-signal-safe prctl between fork and
    // exec, and touches no memory shared with the parent.
    unsafe { cmd.pre_exec(pdeathsig) };

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
        let keep_fd = move || {
            // SAFETY: fcntl(F_SETFD) on an fd we own; async-signal-safe.
            if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        };
        // SAFETY: the hook only calls async-signal-safe fcntl between fork
        // and exec; `fd` stays open until after `spawn`.
        unsafe { cmd.pre_exec(keep_fd) };
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
    let Some(pid) = target_pid(pid) else {
        tracing::warn!(pid, "refusing to signal invalid pid");
        return;
    };
    // SAFETY: plain kill(2); `pid > 0`, so `-pid` is the app's group, not ours.
    if unsafe { libc::kill(-pid, sig) } == -1 {
        // SAFETY: plain kill(2); `pid > 0` targets that single process.
        unsafe { libc::kill(pid, sig) };
    }
}

/// `pid` as a positive `pid_t`; 0 and out-of-range values would make
/// `kill(-pid)` hit our own process group or every process.
fn target_pid(pid: u32) -> Option<libc::pid_t> {
    libc::pid_t::try_from(pid).ok().filter(|&p| p > 0)
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

    #[test]
    fn target_pids() {
        assert_eq!(super::target_pid(0), None);
        assert_eq!(super::target_pid(u32::MAX), None);
        assert_eq!(super::target_pid(1 << 31), None);
        assert_eq!(super::target_pid(42), Some(42));
    }
}
