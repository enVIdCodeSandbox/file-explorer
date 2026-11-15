//! Talking to the terminal host.

use super::proto::*;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::Duration;

pub fn read_info(dir: &Path) -> Option<HostInfo> {
    let bytes = std::fs::read(super::host::info_path(dir)).ok()?;
    serde_json::from_slice(&bytes).ok().filter(|i: &HostInfo| i.version == VERSION)
}

fn open(info: &HostInfo) -> std::io::Result<(TcpStream, BufReader<TcpStream>)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    let mut write = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let hello = serde_json::to_string(&Frame { id: 0, req: Request::Hello { token: info.token.clone() } }).unwrap_or_default();
    write.write_all(format!("{hello}\n").as_bytes())?;
    let mut answer = String::new();
    reader.read_line(&mut answer)?;
    let ok = serde_json::from_str::<Reply>(&answer).map(|r| r.ok).unwrap_or(false);
    if !ok {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "the terminal host did not accept the connection"));
    }
    reader.get_ref().set_read_timeout(None)?;
    Ok((write, reader))
}

/// Is a host answering at this address?
pub fn ping(info: &HostInfo) -> bool {
    open(info).is_ok()
}

pub struct Client {
    write: Mutex<TcpStream>,
    pending: Arc<Mutex<HashMap<u64, Sender<Reply>>>>,
    next: AtomicU64,
    closed: Arc<AtomicBool>,
}

impl Client {
    /// Connect to the host that `dir` says is running. `on_event` gets output and exits (on the
    /// reading thread: keep it quick); `on_close` runs once if the host goes away.
    pub fn connect(dir: &Path, on_event: impl Fn(Event) + Send + 'static, on_close: impl FnOnce() + Send + 'static) -> std::io::Result<Client> {
        let info = read_info(dir).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no terminal host is running"))?;
        let (write, mut reader) = open(&info)?;
        let pending: Arc<Mutex<HashMap<u64, Sender<Reply>>>> = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        {
            let pending = pending.clone();
            let closed = closed.clone();
            std::thread::Builder::new().name("term-client".into()).spawn(move || {
                let mut buf = String::new();
                loop {
                    buf.clear();
                    match reader.read_line(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&buf) else { continue };
                    if v.get("re").is_some() {
                        if let Ok(r) = serde_json::from_value::<Reply>(v) {
                            if let Some(tx) = pending.lock().remove(&r.re) {
                                let _ = tx.send(r);
                            }
                        }
                    } else if let Ok(e) = serde_json::from_value::<Event>(v) {
                        on_event(e);
                    }
                }
                closed.store(true, Ordering::SeqCst);
                // Anyone still waiting for an answer is told it will not come.
                pending.lock().clear();
                on_close();
            })?;
        }
        Ok(Client { write: Mutex::new(write), pending, next: AtomicU64::new(1), closed })
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Send a request and wait for its answer.
    pub fn call(&self, req: Request) -> Result<serde_json::Value, String> {
        if self.is_closed() {
            return Err("The terminal host is not running.".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().insert(id, tx);
        let text = serde_json::to_string(&Frame { id, req }).map_err(|e| e.to_string())?;
        if let Err(e) = self.write.lock().write_all(format!("{text}\n").as_bytes()) {
            self.pending.lock().remove(&id);
            return Err(e.to_string());
        }
        match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(r) if r.ok => Ok(r.data.unwrap_or(serde_json::Value::Null)),
            Ok(r) => Err(r.error.unwrap_or_else(|| "The terminal host refused.".into())),
            Err(_) => {
                self.pending.lock().remove(&id);
                Err("The terminal host did not answer.".into())
            }
        }
    }

    /// Send a request without waiting (keystrokes).
    pub fn send(&self, req: Request) -> Result<(), String> {
        if self.is_closed() {
            return Err("The terminal host is not running.".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let text = serde_json::to_string(&Frame { id, req }).map_err(|e| e.to_string())?;
        self.write.lock().write_all(format!("{text}\n").as_bytes()).map_err(|e| e.to_string())
    }
}

/// Make sure a host is running for `dir`, starting one with `launch` when none answers.
pub fn ensure_host(dir: &Path, launch: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<()> {
    if read_info(dir).map(|i| ping(&i)).unwrap_or(false) {
        return Ok(());
    }
    // A leftover file from a host that is gone points nowhere.
    let _ = std::fs::remove_file(super::host::info_path(dir));
    launch()?;
    for _ in 0..60 {
        std::thread::sleep(Duration::from_millis(100));
        if read_info(dir).map(|i| ping(&i)).unwrap_or(false) {
            return Ok(());
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "the terminal host did not start"))
}

/// Start the host as a background process of this program (`<exe> --terminal-host <dir>`), detached from the app.
pub fn launch_host(exe: &Path, dir: &Path) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--terminal-host").arg(dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        cmd.creation_flags(0x0000_0008 | 0x0000_0200 | 0x0800_0000);
    }
    cmd.spawn().map(|_| ())
}
