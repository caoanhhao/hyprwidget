//! IPC between the CLI and the daemon over
//! `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`.
//!
//! One JSON line per connection each way: the client sends a [`Request`],
//! the daemon answers with a [`Response`] and closes.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// How long the CLI waits for an answer (Lua handlers may run async work).
pub const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    /// `request`, `reload`, `toggle`, `eval`, `list`, `option`, `inspect`…
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub output: String,
}

impl Response {
    pub fn ok(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            output: output.into(),
        }
    }

    pub fn err(output: impl Into<String>) -> Self {
        Self {
            ok: false,
            output: output.into(),
        }
    }
}

/// `$XDG_RUNTIME_DIR/hyprwidget/<instance>.sock`.
pub fn socket_path(instance: &str) -> Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").context("$XDG_RUNTIME_DIR is not set")?;
    Ok(PathBuf::from(runtime)
        .join("hyprwidget")
        .join(format!("{instance}.sock")))
}

/// Sends `req` to a running daemon.
pub fn send(instance: &str, req: &Request) -> Result<Response> {
    send_to(&socket_path(instance)?, req)
}

/// Sends `req` to the daemon listening on `path`.
pub fn send_to(path: &Path, req: &Request) -> Result<Response> {
    let mut s = UnixStream::connect(path).with_context(|| {
        format!(
            "hyprwidget is not running (no socket at {})",
            path.display()
        )
    })?;
    s.set_read_timeout(Some(TIMEOUT))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply)?;
    serde_json::from_str(&reply).context("invalid reply from daemon")
}

/// Answers one request; may be called later (async handlers). Dropping it
/// unanswered sends an error.
pub struct Reply(Option<async_channel::Sender<Response>>);

impl Reply {
    pub fn send(mut self, r: Response) {
        if let Some(tx) = self.0.take() {
            let _ = tx.try_send(r);
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.try_send(Response::err("request was dropped"));
        }
    }
}

/// Listening socket; removes the socket file when dropped.
pub struct Server {
    path: PathBuf,
}

impl Server {
    /// Binds the socket and dispatches requests to `handler` on the calling
    /// thread's GLib main context.
    pub fn start(instance: &str, handler: impl Fn(Request, Reply) + 'static) -> Result<Self> {
        Self::start_at(socket_path(instance)?, handler)
    }

    /// Like [`Server::start`] with an explicit socket path.
    pub fn start_at(path: PathBuf, handler: impl Fn(Request, Reply) + 'static) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                bail!("hyprwidget is already running ({})", path.display());
            }
            std::fs::remove_file(&path)?;
        }
        let listener =
            UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;

        type Job = (Request, async_channel::Sender<Response>);
        let (tx, rx) = async_channel::unbounded::<Job>();
        std::thread::Builder::new()
            .name("hw-ipc".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    let Ok(conn) = conn else { continue };
                    let jobs = tx.clone();
                    std::thread::spawn(move || {
                        if let Err(e) = serve(conn, &jobs) {
                            tracing::debug!("ipc connection: {e:#}");
                        }
                    });
                    if tx.is_closed() {
                        break;
                    }
                }
            })?;

        let handler = Rc::new(RefCell::new(handler));
        glib::spawn_future_local(async move {
            while let Ok((req, reply_tx)) = rx.recv().await {
                tracing::debug!(?req, "ipc");
                (handler.borrow())(req, Reply(Some(reply_tx)));
            }
        });
        Ok(Self { path })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn serve(
    conn: UnixStream,
    jobs: &async_channel::Sender<(Request, async_channel::Sender<Response>)>,
) -> Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    BufReader::new(&conn).read_line(&mut line)?;
    let resp = match serde_json::from_str::<Request>(&line) {
        Ok(req) => {
            let (tx, rx) = async_channel::bounded(1);
            jobs.send_blocking((req, tx))?;
            rx.recv_blocking()
                .unwrap_or_else(|_| Response::err("daemon dropped the request"))
        }
        Err(e) => Response::err(format!("bad request: {e}")),
    };
    let mut out = serde_json::to_string(&resp)?;
    out.push('\n');
    (&conn).write_all(out.as_bytes())?;
    Ok(())
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("hw-ipc-{}", std::process::id()));
        let path = dir.join("test.sock");
        let ctx = glib::MainContext::new();
        ctx.with_thread_default(|| {
            let _server = Server::start_at(path.clone(), |req, reply| {
                reply.send(Response::ok(format!("{}:{}", req.cmd, req.args.join(","))));
            })
            .unwrap();
            let client = std::thread::spawn(move || {
                send_to(
                    &path,
                    &Request {
                        cmd: "echo".into(),
                        args: vec!["a".into(), "b".into()],
                    },
                )
            });
            while !client.is_finished() {
                ctx.iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
            let resp = client.join().unwrap().unwrap();
            assert_eq!(resp, Response::ok("echo:a,b"));
        })
        .unwrap();
    }
}
