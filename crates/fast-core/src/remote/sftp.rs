//! SFTP over SSH, in pure Rust. The server's key is checked against a list of
//! hosts the person has already trusted (trust on first use, like OpenSSH).

use super::{fail, known_host, remember_host, Batches, Control, Entry, Server, Session, CHANGED_HOST, UNTRUSTED_HOST};
use crate::error::{CoreError, Result};
use russh::client::{self, Handle, Handler};
use russh::keys::{ssh_key::HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh_sftp::client::{RawSftpSession, SftpSession};
use russh_sftp::protocol::OpenFlags;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;

pub(super) struct Client {
    /// The fingerprint that is acceptable, if any.
    accept: Option<String>,
    /// The fingerprint the server showed.
    seen: Arc<Mutex<Option<String>>>,
}

impl Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> std::result::Result<bool, Self::Error> {
        let fp = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.fingerprint(HashAlg::Sha256).to_string(),
            PublicKeyOrCertificate::Certificate(_) => return Ok(false),
        };
        *self.seen.lock().unwrap() = Some(fp.clone());
        Ok(self.accept.as_deref() == Some(fp.as_str()))
    }
}

struct Sftp {
    rt: Runtime,
    // Kept so the connection stays open.
    _handle: Handle<Client>,
    sftp: SftpSession,
    /// A second channel that reads folders page by page (the library's own listing waits for the last page). `None`
    /// until first wanted; `raw_failed` when the server would not open one, and folders are read the plain way.
    raw: Option<Arc<RawSftpSession>>,
    raw_failed: bool,
}

pub(super) fn connect_handle(server: &Server, known_hosts: &Path) -> Result<(Runtime, Handle<Client>)> {
    // One background thread keeps the connection alive between calls.
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().map_err(fail)?;
    let known = known_host(known_hosts, &server.host, server.port);
    let seen = Arc::new(Mutex::new(None));
    // A fingerprint the person just agreed to is accepted, and remembered.
    let accept = server.trust.clone().or_else(|| known.clone());
    let handler = Client { accept, seen: seen.clone() };
    let config = Arc::new(client::Config { inactivity_timeout: Some(Duration::from_secs(3600)), keepalive_interval: Some(Duration::from_secs(30)), ..Default::default() });

    let handle = rt.block_on(async {
        let addr = (server.host.as_str(), server.port);
        // Through the proxy, when one is set: the connection is made first and handed to the SSH client.
        let connecting = async {
            if super::proxy::current().is_some() {
                let (host, port) = (server.host.clone(), server.port);
                let tcp = tokio::task::spawn_blocking(move || super::proxy::connect(&host, port, Duration::from_secs(20)))
                    .await
                    .map_err(|e| russh::Error::IO(std::io::Error::other(e.to_string())))?
                    .map_err(russh::Error::IO)?;
                tcp.set_nonblocking(true).map_err(russh::Error::IO)?;
                let tcp = tokio::net::TcpStream::from_std(tcp).map_err(russh::Error::IO)?;
                client::connect_stream(config, tcp, handler).await
            } else {
                client::connect(config, addr, handler).await
            }
        };
        let mut handle = match tokio::time::timeout(Duration::from_secs(20), connecting).await {
            Err(_) => return Err(fail(format!("{} did not answer in time.", server.host))),
            Ok(Ok(h)) => h,
            Ok(Err(e)) => {
                let shown = seen.lock().unwrap().clone();
                return Err(match shown {
                    Some(fp) if known.is_some() => fail(format!("{CHANGED_HOST}{fp}")),
                    Some(fp) => fail(format!("{UNTRUSTED_HOST}{fp}")),
                    None => fail(format!("Could not connect to {}: {e}", server.host)),
                });
            }
        };
        let user = if server.user.is_empty() { "anonymous".to_string() } else { server.user.clone() };
        let authed = match &server.key_file {
            Some(file) if !file.is_empty() => {
                let pass = (!server.password.is_empty()).then_some(server.password.as_str());
                let key = russh::keys::load_secret_key(file, pass).map_err(|e| fail(format!("Could not read the key file: {e}")))?;
                let hash = handle.best_supported_rsa_hash().await.map_err(fail)?.flatten();
                handle.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await.map_err(fail)?
            }
            _ => handle.authenticate_password(user, server.password.clone()).await.map_err(fail)?,
        };
        if !authed.success() {
            return Err(fail("The server did not accept the login (check the user name and password or key)."));
        }
        Ok(handle)
    })?;

    // The key was acceptable: remember it (a no-op when it was already known).
    if let Some(fp) = seen.lock().unwrap().clone() {
        if known.as_deref() != Some(fp.as_str()) {
            remember_host(known_hosts, &server.host, server.port, &fp);
        }
    }
    Ok((rt, handle))
}

pub(super) fn try_from_handle(rt: Runtime, handle: Handle<Client>) -> std::result::Result<Box<dyn Session>, (Runtime, Handle<Client>, CoreError)> {
    let opened = rt.block_on(async {
        let channel = handle.channel_open_session().await.map_err(fail)?;
        channel.request_subsystem(true, "sftp").await.map_err(|_| fail("The server does not offer SFTP."))?;
        SftpSession::new(channel.into_stream()).await.map_err(|e| fail(format!("Could not start SFTP: {e}")))
    });
    match opened {
        Ok(sftp) => Ok(Box::new(Sftp { rt, _handle: handle, sftp, raw: None, raw_failed: false })),
        Err(e) => Err((rt, handle, e)),
    }
}

pub(super) fn connect(server: &Server, known_hosts: &Path) -> Result<Box<dyn Session>> {
    let (rt, handle) = connect_handle(server, known_hosts)?;
    try_from_handle(rt, handle).map_err(|(_, _, e)| e)
}

fn sftp_err(path: &str, e: russh_sftp::client::error::Error) -> CoreError {
    let text = e.to_string();
    let lower = text.to_lowercase();
    if lower.contains("no such file") || lower.contains("nosuchfile") {
        fail(format!("Not found on the server: {path}"))
    } else if lower.contains("permission") {
        fail(format!("The server does not allow this: {path}"))
    } else {
        fail(format!("{path}: {text}"))
    }
}

impl Sftp {
    /// The channel that reads folders a page at a time, opened on first use.
    fn pager(&mut self) -> Option<Arc<RawSftpSession>> {
        if self.raw.is_none() && !self.raw_failed {
            let opened = self.rt.block_on(async {
                let channel = self._handle.channel_open_session().await.ok()?;
                channel.request_subsystem(true, "sftp").await.ok()?;
                let raw = RawSftpSession::new(channel.into_stream());
                raw.init().await.ok()?;
                Some(Arc::new(raw))
            });
            self.raw_failed = opened.is_none();
            self.raw = opened;
        }
        self.raw.clone()
    }

    /// One item of a folder as the listing shows it. A link to a folder opens like a folder.
    fn entry_of(&mut self, dir: &str, name: &str, attrs: &russh_sftp::protocol::FileAttributes) -> Entry {
        let mut is_dir = attrs.file_type().is_dir();
        let mut size = attrs.len();
        let mut modified = attrs.mtime.map(|t| t as i64);
        if attrs.file_type().is_symlink() {
            let target = if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") };
            if let Ok(m) = self.rt.block_on(self.sftp.metadata(target)) {
                is_dir = m.file_type().is_dir();
                size = m.len();
                modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            }
        }
        Entry { name: name.to_string(), is_dir, size: if is_dir { 0 } else { size }, modified }
    }
}

impl Session for Sftp {
    fn home(&mut self) -> Result<String> {
        self.rt.block_on(self.sftp.canonicalize(".")).map_err(|e| sftp_err(".", e))
    }

    fn list_streaming(&mut self, path: &str, cancel: &AtomicBool, sink: &mut dyn FnMut(&[Entry], crate::entry::ListProgress)) -> Result<Vec<Entry>> {
        use russh_sftp::client::error::Error as SftpError;
        use russh_sftp::protocol::StatusCode;
        let Some(raw) = self.pager() else { return self.list(path) };
        let handle = self.rt.block_on(raw.opendir(path)).map_err(|e| sftp_err(path, e))?.handle;
        let (mut out, mut batches) = (Vec::new(), Batches::new(sink, 0));
        let result = loop {
            if cancel.load(Ordering::Relaxed) {
                break Err(fail("Cancelled."));
            }
            match self.rt.block_on(raw.readdir(handle.as_str())) {
                Ok(page) => {
                    for f in page.files {
                        if f.filename == "." || f.filename == ".." {
                            batches.add(None);
                            continue;
                        }
                        let entry = self.entry_of(path, &f.filename, &f.attrs);
                        out.push(entry.clone());
                        batches.add(Some(entry));
                    }
                }
                Err(SftpError::Status(st)) if st.status_code == StatusCode::Eof => break Ok(()),
                Err(e) => break Err(sftp_err(path, e)),
            }
        };
        let _ = self.rt.block_on(raw.close(handle));
        result?;
        batches.finish();
        Ok(out)
    }

    fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let dir = self.rt.block_on(self.sftp.read_dir(path)).map_err(|e| sftp_err(path, e))?;
        let mut out = Vec::new();
        for item in dir {
            let name = item.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let meta = item.metadata();
            let mut is_dir = item.file_type().is_dir();
            let mut size = meta.len();
            let mut modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            // A link to a folder should open like a folder.
            if item.file_type().is_symlink() {
                let target = if path.ends_with('/') { format!("{path}{name}") } else { format!("{path}/{name}") };
                if let Ok(m) = self.rt.block_on(self.sftp.metadata(target)) {
                    is_dir = m.file_type().is_dir();
                    size = m.len();
                    modified = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
                }
            }
            out.push(Entry { name, is_dir, size: if is_dir { 0 } else { size }, modified });
        }
        Ok(out)
    }

    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let result: Result<u64> = self.rt.block_on(async {
            let mut src = self.sftp.open(remote).await.map_err(|e| sftp_err(remote, e))?;
            let mut out = tokio::fs::File::create(local).await.map_err(|e| CoreError::io(local, e))?;
            let mut buf = vec![0u8; 128 * 1024];
            let mut total = 0u64;
            loop {
                ctl.check()?;
                let n = src.read(&mut buf).await.map_err(fail)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).await.map_err(|e| CoreError::io(local, e))?;
                total += n as u64;
                (ctl.on_bytes)(n as u64);
            }
            out.flush().await.map_err(|e| CoreError::io(local, e))?;
            Ok(total)
        });
        if result.is_err() {
            let _ = std::fs::remove_file(local);
        }
        result
    }

    fn read_head(&mut self, remote: &str, limit: usize) -> Result<Vec<u8>> {
        let cap = limit.min(64 * 1024);
        self.rt.block_on(async {
            let mut src = self.sftp.open(remote).await.map_err(|e| sftp_err(remote, e))?;
            let mut out = vec![0u8; cap];
            let mut len = 0usize;
            while len < cap {
                let n = src.read(&mut out[len..]).await.map_err(fail)?;
                if n == 0 {
                    break;
                }
                len += n;
            }
            out.truncate(len);
            Ok(out)
        })
    }

    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
        let result: Result<u64> = self.rt.block_on(async {
            let mut src = tokio::fs::File::open(local).await.map_err(|e| CoreError::io(local, e))?;
            let mut out = self.sftp.open_with_flags(remote, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE).await.map_err(|e| sftp_err(remote, e))?;
            let mut buf = vec![0u8; 128 * 1024];
            let mut total = 0u64;
            let copied: Result<()> = async {
                loop {
                    ctl.check()?;
                    let n = src.read(&mut buf).await.map_err(|e| CoreError::io(local, e))?;
                    if n == 0 {
                        return Ok(());
                    }
                    out.write_all(&buf[..n]).await.map_err(fail)?;
                    total += n as u64;
                    (ctl.on_bytes)(n as u64);
                }
            }
            .await;
            let closed = out.shutdown().await;
            copied?;
            closed.map_err(fail)?;
            Ok(total)
        });
        if result.is_err() {
            // Do not leave half a file behind.
            let _ = self.rt.block_on(self.sftp.remove_file(remote));
        }
        result
    }

    fn streams(&self) -> bool {
        true
    }

    fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
        self.rt.block_on(async {
            let mut src = self.sftp.open(remote).await.map_err(|e| sftp_err(remote, e))?;
            let mut buf = vec![0u8; 256 * 1024];
            let mut total = 0u64;
            loop {
                ctl.check()?;
                let n = src.read(&mut buf).await.map_err(fail)?;
                if n == 0 {
                    return Ok(total);
                }
                sink(&buf[..n])?;
                total += n as u64;
            }
        })
    }

    fn write_stream(&mut self, source: &mut (dyn std::io::Read + Send), _size: u64, remote: &str, ctl: &Control) -> Result<u64> {
        let result: Result<u64> = self.rt.block_on(async {
            let mut out = self.sftp.open_with_flags(remote, OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE).await.map_err(|e| sftp_err(remote, e))?;
            let mut buf = vec![0u8; 128 * 1024];
            let mut total = 0u64;
            let copied: Result<()> = async {
                loop {
                    ctl.check()?;
                    let n = source.read(&mut buf).map_err(fail)?;
                    if n == 0 {
                        return Ok(());
                    }
                    out.write_all(&buf[..n]).await.map_err(fail)?;
                    total += n as u64;
                    (ctl.on_bytes)(n as u64);
                }
            }
            .await;
            let closed = out.shutdown().await;
            copied?;
            closed.map_err(fail)?;
            Ok(total)
        });
        if result.is_err() {
            let _ = self.rt.block_on(self.sftp.remove_file(remote));
        }
        result
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.rt.block_on(self.sftp.create_dir(path)).map_err(|e| sftp_err(path, e))
    }

    fn remove(&mut self, path: &str, is_dir: bool) -> Result<()> {
        if is_dir {
            self.rt.block_on(self.sftp.remove_dir(path)).map_err(|e| sftp_err(path, e))
        } else {
            self.rt.block_on(self.sftp.remove_file(path)).map_err(|e| sftp_err(path, e))
        }
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.rt.block_on(self.sftp.rename(from, to)).map_err(|e| sftp_err(from, e))
    }

    fn exists(&mut self, path: &str) -> Result<bool> {
        self.rt.block_on(self.sftp.try_exists(path)).map_err(|e| sftp_err(path, e))
    }
}

#[allow(dead_code)]
fn _unused(_: PathBuf) {}
