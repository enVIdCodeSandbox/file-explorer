//! Servers: browse and copy files on FTP, SFTP (SSH) and WebDAV servers.
//!
//! A server location is written as a URL that never contains the password:
//! `sftp://anna@example.com:22/home/anna/docs`. Everything after the host is
//! the path exactly as the server knows it (names are not percent-encoded, so
//! a file called `100% #1.txt` just works).
//!
//! Each protocol implements [`Session`]; the tree helpers here (download a
//! folder, upload a folder, delete a folder) are written once on top of it.

mod ftp;
mod http;
pub mod mtp;
pub mod proxy;
mod sftp;
pub mod ssh;
mod webdav;

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ftp,
    Sftp,
    Ssh,
    /// WebDAV over HTTP or HTTPS.
    Dav,
    /// A phone, camera or player plugged in (see [`mtp`]); the host is the device's key.
    Mtp,
}

impl Protocol {
    pub fn default_port(self, tls: bool) -> u16 {
        match (self, tls) {
            (Protocol::Ftp, _) => 21,
            (Protocol::Sftp | Protocol::Ssh, _) => 22,
            (Protocol::Dav, false) => 80,
            (Protocol::Dav, true) => 443,
            (Protocol::Mtp, _) => 0,
        }
    }
}

/// Everything needed to open a connection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// Never saved to disk by the app.
    #[serde(default)]
    pub password: String,
    /// SFTP: a private key file to log in with instead of a password.
    #[serde(default)]
    pub key_file: Option<String>,
    /// WebDAV: use HTTPS.
    #[serde(default)]
    pub tls: bool,
    /// SFTP: the host key fingerprint the person agreed to trust for this connection.
    #[serde(default)]
    pub trust: Option<String>,
}

impl Server {
    /// `sftp://user@host:port` — the part of a location that names the server.
    pub fn base_url(&self) -> String {
        let scheme = match (self.protocol, self.tls) {
            (Protocol::Ftp, _) => "ftp",
            (Protocol::Sftp, _) => "sftp",
            (Protocol::Ssh, _) => "ssh",
            (Protocol::Dav, false) => "dav",
            (Protocol::Dav, true) => "davs",
            (Protocol::Mtp, _) => "mtp",
        };
        let user = if self.user.is_empty() { String::new() } else { format!("{}@", self.user) };
        let port = if self.port == self.protocol.default_port(self.tls) { String::new() } else { format!(":{}", self.port) };
        format!("{scheme}://{user}{}{port}", self.host)
    }

    /// A key that identifies the connection: two locations on the same
    /// server with the same login share it.
    pub fn key(&self) -> String {
        self.base_url()
    }
}

/// A location on a server, split into parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub protocol: Protocol,
    pub tls: bool,
    pub user: String,
    pub host: String,
    pub port: u16,
    /// Always starts with `/`.
    pub path: String,
}

/// Whether text is the address of something on a server.
pub fn is_remote(s: &str) -> bool {
    ["ftp://", "sftp://", "ssh://", "dav://", "davs://", "mtp://"].iter().any(|p| s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p))
}

/// Whether text is the address of something on a plugged-in device (a phone or camera).
pub fn is_device(s: &str) -> bool {
    s.len() >= 6 && s[..6].eq_ignore_ascii_case("mtp://")
}

impl Location {
    pub fn parse(s: &str) -> Result<Location> {
        let bad = || CoreError::Invalid(format!("\"{s}\" is not a server address (like sftp://user@host/folder)."));
        let (scheme, rest) = s.split_once("://").ok_or_else(bad)?;
        let (protocol, tls) = match scheme.to_ascii_lowercase().as_str() {
            "ftp" => (Protocol::Ftp, false),
            "sftp" => (Protocol::Sftp, false),
            "ssh" => (Protocol::Ssh, false),
            "dav" => (Protocol::Dav, false),
            "davs" => (Protocol::Dav, true),
            "mtp" => (Protocol::Mtp, false),
            _ => return Err(bad()),
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let (user, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (u.to_string(), h),
            None => (String::new(), authority),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (h.to_string(), p.parse::<u16>().map_err(|_| bad())?),
            _ => (hostport.to_string(), protocol.default_port(tls)),
        };
        if host.is_empty() {
            return Err(bad());
        }
        Ok(Location { protocol, tls, user, host, port, path: normalize(path) })
    }

    /// The server this location is on (without a password).
    pub fn server(&self) -> Server {
        Server { protocol: self.protocol, host: self.host.clone(), port: self.port, user: self.user.clone(), password: String::new(), key_file: None, tls: self.tls, trust: None }
    }

    pub fn url(&self) -> String {
        format!("{}{}", self.server().base_url(), if self.path == "/" { "/".to_string() } else { self.path.clone() })
    }

    pub fn key(&self) -> String {
        self.server().key()
    }

    pub fn parent(&self) -> Option<Location> {
        if self.path == "/" {
            return None;
        }
        let cut = self.path.rfind('/')?;
        Some(Location { path: if cut == 0 { "/".into() } else { self.path[..cut].to_string() }, ..self.clone() })
    }

    pub fn child(&self, name: &str) -> Location {
        let path = if self.path == "/" { format!("/{name}") } else { format!("{}/{name}", self.path) };
        Location { path, ..self.clone() }
    }

    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }
}

/// Collapse `//`, `.` and a trailing slash; resolve `..`.
pub fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: Option<i64>,
}

/// Gathers the items of a listing and hands them to the sink in batches: the first ones at once (so the folder
/// starts to fill straight away), after that about every tenth of a second.
pub(crate) struct Batches<'a> {
    sink: &'a mut dyn FnMut(&[Entry], crate::entry::ListProgress),
    batch: Vec<Entry>,
    flushed: std::time::Instant,
    first: bool,
    restarted: bool,
    total: usize,
    pub done: usize,
}

impl<'a> Batches<'a> {
    pub(crate) const EVERY: std::time::Duration = std::time::Duration::from_millis(100);

    pub(crate) fn new(sink: &'a mut dyn FnMut(&[Entry], crate::entry::ListProgress), total: usize) -> Self {
        Batches { sink, batch: Vec::new(), flushed: std::time::Instant::now(), first: true, restarted: false, total, done: 0 }
    }

    /// One more item was read (kept, or left out: `kept` is false for the entries the listing does not show).
    pub(crate) fn add(&mut self, entry: Option<Entry>) {
        self.done += 1;
        if let Some(e) = entry {
            self.batch.push(e);
        }
        if !self.batch.is_empty() && (self.first || self.flushed.elapsed() >= Self::EVERY) {
            self.flush();
        }
    }

    fn flush(&mut self) {
        (self.sink)(&self.batch, crate::entry::ListProgress { done: self.done, total: self.total, restart: self.first && self.restarted });
        self.batch.clear();
        self.flushed = std::time::Instant::now();
        self.first = false;
    }

    /// Hand over what is left.
    pub(crate) fn finish(mut self) {
        self.flush();
    }
}

/// Cancellation and byte counting for one operation.
pub struct Control<'a> {
    pub cancel: &'a AtomicBool,
    /// Called with the number of bytes just moved.
    pub on_bytes: &'a (dyn Fn(u64) + Sync),
}

impl Control<'_> {
    pub fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(CoreError::Invalid("Cancelled.".into()))
        } else {
            Ok(())
        }
    }
}

/// One open connection.
pub trait Session: Send {
    /// The folder the server starts in.
    fn home(&mut self) -> Result<String>;
    fn list(&mut self, path: &str) -> Result<Vec<Entry>>;
    /// As `list`, but a device that hands over its items one at a time (a phone) passes
    /// each batch to `sink` as it arrives, with the count the device announced. The result
    /// is the whole listing. The default does no batching.
    fn list_streaming(&mut self, path: &str, cancel: &AtomicBool, sink: &mut dyn FnMut(&[Entry], crate::entry::ListProgress)) -> Result<Vec<Entry>> {
        let _ = (cancel, sink);
        self.list(path)
    }
    /// The small preview picture a device keeps for a file (JPEG or similar), when it has
    /// one. `Ok(None)`: this kind of connection has none; an error: the device has none for
    /// this file.
    fn thumbnail(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        let _ = path;
        Ok(None)
    }
    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64>;
    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64>;
    /// Read up to `limit` leading bytes of a remote file to tell text from binary without copying the whole file.
    fn read_head(&mut self, remote: &str, limit: usize) -> Result<Vec<u8>> {
        let _ = (remote, limit);
        Err(fail("Cannot inspect remote file contents."))
    }
    /// Whether `read_stream` and `write_stream` work on this kind of connection. Copying from one server (or phone)
    /// to another passes the bytes straight through when both ends can, with nothing kept on this computer.
    fn streams(&self) -> bool {
        false
    }
    /// Send a file's bytes to `sink`, a piece at a time, keeping no copy. Returns how many bytes were sent.
    /// Bytes are counted by the writing end (`write_stream`), not here.
    fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
        let _ = (remote, sink, ctl);
        Err(fail("This kind of connection cannot pass files straight through."))
    }
    /// Store a file of `size` bytes read from `source`. Counts the bytes with `ctl.on_bytes`. Does not leave half a
    /// file behind when it fails.
    fn write_stream(&mut self, source: &mut (dyn std::io::Read + Send), size: u64, remote: &str, ctl: &Control) -> Result<u64> {
        let _ = (source, size, remote, ctl);
        Err(fail("This kind of connection cannot pass files straight through."))
    }
    fn mkdir(&mut self, path: &str) -> Result<()>;
    /// Delete a file, or an empty folder.
    fn remove(&mut self, path: &str, is_dir: bool) -> Result<()>;
    fn rename(&mut self, from: &str, to: &str) -> Result<()>;
    /// Whether something exists at this path.
    fn exists(&mut self, path: &str) -> Result<bool> {
        let trimmed = path.trim_end_matches('/');
        let cut = trimmed.rfind('/').unwrap_or(0);
        let parent = if cut == 0 { "/" } else { &trimmed[..cut] };
        let name = &trimmed[cut + 1..];
        Ok(self.list(parent)?.iter().any(|e| e.name == name))
    }
}

/// Open a connection.
pub fn connect(server: &Server, known_hosts: &Path) -> Result<Box<dyn Session>> {
    match server.protocol {
        Protocol::Ftp => ftp::connect(server),
        Protocol::Sftp => sftp::connect(server, known_hosts),
        Protocol::Ssh => ssh::connect(server, known_hosts),
        Protocol::Dav => webdav::connect(server),
        Protocol::Mtp => mtp::connect(&server.host),
    }
}

/// Connecting to an SFTP server whose key is not known yet fails with a
/// message that starts with this, followed by the fingerprint, so the
/// interface can ask the person whether to trust it.
pub const UNTRUSTED_HOST: &str = "untrusted-host:";
/// Same, for a server whose key is different from the one remembered.
pub const CHANGED_HOST: &str = "changed-host:";

pub(crate) fn fail(what: impl std::fmt::Display) -> CoreError {
    CoreError::Invalid(what.to_string())
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

/// What a tree operation did.
#[derive(Default, Debug, Clone)]
pub struct Tally {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub failed: Vec<(String, String)>,
}

/// Copy a file or folder from the server into the local folder `dest_dir`
/// (under the same name). Existing local files are kept unless `replace`.
pub fn download_tree(s: &mut dyn Session, remote: &str, is_dir: bool, dest_dir: &Path, replace: bool, ctl: &Control, tally: &mut Tally) -> Result<()> {
    let name = remote.rsplit('/').next().unwrap_or("download").to_string();
    download_tree_as(s, remote, is_dir, dest_dir, &name, replace, ctl, tally)
}

/// Like [`download_tree`], but the top item is saved as `name`.
#[allow(clippy::too_many_arguments)]
pub fn download_tree_as(s: &mut dyn Session, remote: &str, is_dir: bool, dest_dir: &Path, name: &str, replace: bool, ctl: &Control, tally: &mut Tally) -> Result<()> {
    ctl.check()?;
    let target = dest_dir.join(safe_name(name));
    if !is_dir {
        if target.exists() && !replace {
            return Ok(());
        }
        match s.download(remote, &target, ctl) {
            Ok(n) => {
                tally.files += 1;
                tally.bytes += n;
            }
            Err(e) if ctl.cancel.load(Ordering::Relaxed) => return Err(e),
            Err(e) => tally.failed.push((remote.to_string(), e.to_string())),
        }
        return Ok(());
    }
    std::fs::create_dir_all(&target).map_err(|e| CoreError::io(&target, e))?;
    tally.folders += 1;
    for e in s.list(remote)? {
        download_tree(s, &format!("{}/{}", remote.trim_end_matches('/'), e.name), e.is_dir, &target, replace, ctl, tally)?;
    }
    Ok(())
}

/// Copy a local file or folder to the server folder `dest_dir`.
pub fn upload_tree(s: &mut dyn Session, local: &Path, dest_dir: &str, replace: bool, ctl: &Control, tally: &mut Tally) -> Result<()> {
    let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    upload_tree_as(s, local, dest_dir, &name, replace, ctl, tally)
}

/// Like [`upload_tree`], but the top item is stored as `name`.
#[allow(clippy::too_many_arguments)]
pub fn upload_tree_as(s: &mut dyn Session, local: &Path, dest_dir: &str, name: &str, replace: bool, ctl: &Control, tally: &mut Tally) -> Result<()> {
    ctl.check()?;
    let target = format!("{}/{}", dest_dir.trim_end_matches('/'), name);
    let meta = std::fs::metadata(local).map_err(|e| CoreError::io(local, e))?;
    if meta.is_dir() {
        if !s.exists(&target).unwrap_or(false) {
            s.mkdir(&target)?;
        }
        tally.folders += 1;
        let mut kids: Vec<_> = std::fs::read_dir(local).map_err(|e| CoreError::io(local, e))?.filter_map(|e| e.ok()).collect();
        kids.sort_by_key(|k| k.file_name());
        for k in kids {
            upload_tree(s, &k.path(), &target, replace, ctl, tally)?;
        }
        return Ok(());
    }
    if !replace && s.exists(&target).unwrap_or(false) {
        return Ok(());
    }
    match s.upload(local, &target, ctl) {
        Ok(n) => {
            tally.files += 1;
            tally.bytes += n;
        }
        Err(e) if ctl.cancel.load(Ordering::Relaxed) => return Err(e),
        Err(e) => tally.failed.push((local.to_string_lossy().into_owned(), e.to_string())),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Straight from one server to another
// ---------------------------------------------------------------------------

/// How much is read ahead of the writer: `RELAY_DEPTH` pieces of `RELAY_PIECE` bytes. Memory use never passes this,
/// whatever the size of the file.
const RELAY_PIECE: usize = 1 << 20;
const RELAY_DEPTH: usize = 8;

/// The receiving end's view of what the sending thread produces.
struct Pipe<'a> {
    rx: std::sync::mpsc::Receiver<Vec<u8>>,
    piece: Vec<u8>,
    at: usize,
    /// Set by the sending thread when it stopped early, so a cut-off file is never mistaken for a whole one.
    broke: &'a AtomicBool,
}

impl std::io::Read for Pipe<'_> {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.at >= self.piece.len() {
            match self.rx.recv() {
                Ok(next) => {
                    self.piece = next;
                    self.at = 0;
                }
                Err(_) if self.broke.load(Ordering::SeqCst) => return Err(std::io::Error::other("The source stopped sending.")),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.piece.len() - self.at);
        out[..n].copy_from_slice(&self.piece[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// Copy one file from `src` to `dst` while it is being read: the two connections work at the same time, so the time
/// taken is that of the slower one, and nothing is stored on this computer. Returns the bytes copied.
pub fn relay_file(src: &mut dyn Session, from: &str, dst: &mut dyn Session, to: &str, size: u64, ctl: &Control) -> Result<u64> {
    let broke = AtomicBool::new(false);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(RELAY_DEPTH);
    let (read, written) = std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut piece: Vec<u8> = Vec::with_capacity(RELAY_PIECE);
            let result = src.read_stream(
                from,
                &mut |bytes| {
                    piece.extend_from_slice(bytes);
                    if piece.len() >= RELAY_PIECE {
                        // The writer is gone (it failed): stop reading.
                        tx.send(std::mem::replace(&mut piece, Vec::with_capacity(RELAY_PIECE))).map_err(|_| fail("The destination stopped receiving."))?;
                    }
                    ctl.check()
                },
                ctl,
            );
            let result = result.and_then(|n| {
                if !piece.is_empty() {
                    tx.send(std::mem::take(&mut piece)).map_err(|_| fail("The destination stopped receiving."))?;
                }
                Ok(n)
            });
            if result.is_err() {
                broke.store(true, Ordering::SeqCst);
            }
            drop(tx);
            result
        });
        let mut pipe = Pipe { rx, piece: Vec::new(), at: 0, broke: &broke };
        let written = dst.write_stream(&mut pipe, size, to, ctl);
        // A writer that stopped early must not leave the reader waiting for room that never comes.
        drop(pipe);
        (reader.join().unwrap_or_else(|_| Err(fail("The source stopped unexpectedly."))), written)
    });
    match (read, written) {
        (Ok(r), Ok(w)) if r == w => Ok(w),
        (Ok(r), Ok(w)) => Err(fail(format!("Only {w} of {r} bytes arrived."))),
        // The sending end's explanation comes first: the receiving end only noticed the file stopped.
        (Err(e), _) if !matches!(&e, CoreError::Invalid(m) if m.starts_with("The destination stopped")) => Err(e),
        (_, Err(e)) => Err(e),
        (Err(e), Ok(_)) => Err(e),
    }
}

/// Copy a file or folder from one server to another, passing the bytes straight through (see [`relay_file`]). The
/// top item is stored as `name` in `dest_dir`. Existing items are kept unless `replace`.
#[allow(clippy::too_many_arguments)]
pub fn relay_tree_as(src: &mut dyn Session, remote: &str, is_dir: bool, size: u64, dst: &mut dyn Session, dest_dir: &str, name: &str, replace: bool, ctl: &Control, tally: &mut Tally) -> Result<()> {
    ctl.check()?;
    let target = format!("{}/{}", dest_dir.trim_end_matches('/'), name);
    if !is_dir {
        if !replace && dst.exists(&target).unwrap_or(false) {
            return Ok(());
        }
        match relay_file(src, remote, dst, &target, size, ctl) {
            Ok(n) => {
                tally.files += 1;
                tally.bytes += n;
            }
            Err(e) if ctl.cancel.load(Ordering::Relaxed) => return Err(e),
            Err(e) => tally.failed.push((remote.to_string(), e.to_string())),
        }
        return Ok(());
    }
    if !dst.exists(&target).unwrap_or(false) {
        dst.mkdir(&target)?;
    }
    tally.folders += 1;
    for e in src.list(remote)? {
        relay_tree_as(src, &format!("{}/{}", remote.trim_end_matches('/'), e.name), e.is_dir, e.size, dst, &target, &e.name, replace, ctl, tally)?;
    }
    Ok(())
}

/// Count the files and bytes in a file or folder on the server.
pub fn measure_tree(s: &mut dyn Session, path: &str, is_dir: bool, size: u64, ctl: &Control) -> Result<(u64, u64)> {
    ctl.check()?;
    if !is_dir {
        return Ok((1, size));
    }
    let (mut files, mut bytes) = (0, 0);
    for e in s.list(path)? {
        let (f, b) = measure_tree(s, &format!("{}/{}", path.trim_end_matches('/'), e.name), e.is_dir, e.size, ctl)?;
        files += f;
        bytes += b;
    }
    Ok((files, bytes))
}

/// Count the files and bytes in a local file or folder.
pub fn measure_local(path: &Path) -> (u64, u64) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::read_dir(path).map(|rd| rd.filter_map(|e| e.ok()).map(|e| measure_local(&e.path())).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))).unwrap_or((0, 0)),
        Ok(m) => (1, m.len()),
        Err(_) => (0, 0),
    }
}

/// A name for a new item that does not clash: `name`, then `name (2).ext`, `name (3).ext`…
pub fn free_name(name: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..10_000).map(|n| format!("{stem} ({n}){ext}")).find(|c| !taken(c)).unwrap_or_else(|| format!("{stem} (copy){ext}"))
}

/// Delete a file, or a folder with everything in it.
pub fn remove_tree(s: &mut dyn Session, path: &str, is_dir: bool, ctl: &Control) -> Result<u64> {
    ctl.check()?;
    if !is_dir {
        s.remove(path, false)?;
        return Ok(1);
    }
    let mut n = 0;
    for e in s.list(path)? {
        n += remove_tree(s, &format!("{}/{}", path.trim_end_matches('/'), e.name), e.is_dir, ctl)?;
    }
    s.remove(path, true)?;
    Ok(n + 1)
}

/// A server can send any name; never let one write outside the target folder.
/// `name` with " (n)" before its extension: "New file.txt" becomes "New file (2).txt".
pub fn numbered_name(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

pub fn safe_name(name: &str) -> PathBuf {
    let cleaned: String = name.chars().map(|c| if matches!(c, '/' | '\\' | '\0') { '_' } else { c }).collect();
    let cleaned = match cleaned.as_str() {
        "" | "." | ".." => "_".to_string(),
        _ => cleaned,
    };
    PathBuf::from(cleaned)
}

/// Remember fingerprints of SFTP hosts the person trusted, one `host:port fingerprint` per line.
pub(crate) fn known_host(file: &Path, host: &str, port: u16) -> Option<String> {
    let wanted = format!("{host}:{port}");
    std::fs::read_to_string(file).ok()?.lines().find_map(|l| {
        let (h, fp) = l.split_once(' ')?;
        (h == wanted).then(|| fp.trim().to_string())
    })
}

pub(crate) fn remember_host(file: &Path, host: &str, port: u16, fingerprint: &str) {
    let wanted = format!("{host}:{port}");
    let mut lines: Vec<String> = std::fs::read_to_string(file).unwrap_or_default().lines().filter(|l| !l.starts_with(&format!("{wanted} "))).map(String::from).collect();
    lines.push(format!("{wanted} {fingerprint}"));
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, lines.join("\n") + "\n");
}

/// Copy between a reader and a writer, counting bytes and honouring Cancel.
pub(crate) fn pump(from: &mut dyn std::io::Read, to: &mut dyn std::io::Write, ctl: &Control) -> Result<u64> {
    let mut buf = vec![0u8; 128 * 1024];
    let mut total = 0u64;
    loop {
        ctl.check()?;
        let n = from.read(&mut buf).map_err(|e| fail(e))?;
        if n == 0 {
            return Ok(total);
        }
        to.write_all(&buf[..n]).map_err(|e| fail(e))?;
        total += n as u64;
        (ctl.on_bytes)(n as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_names_skip_taken_ones() {
        let taken = ["a.txt", "a (2).txt", "dir", ".hidden"];
        let t = |n: &str| taken.contains(&n);
        assert_eq!(free_name("b.txt", t), "b.txt");
        assert_eq!(free_name("a.txt", t), "a (3).txt");
        assert_eq!(free_name("dir", t), "dir (2)");
        assert_eq!(free_name(".hidden", t), ".hidden (2)");
    }

    #[test]
    fn parses_server_addresses() {
        let l = Location::parse("sftp://anna@example.com:2222/home/anna/My Docs").unwrap();
        assert_eq!((l.protocol, l.user.as_str(), l.host.as_str(), l.port, l.path.as_str()), (Protocol::Sftp, "anna", "example.com", 2222, "/home/anna/My Docs"));
        let l = Location::parse("ftp://files.example.com").unwrap();
        assert_eq!((l.port, l.path.as_str(), l.user.as_str()), (21, "/", ""));
        let l = Location::parse("davs://u@cloud.example.com/remote.php/dav/files/u/").unwrap();
        assert_eq!((l.protocol, l.tls, l.port, l.path.as_str()), (Protocol::Dav, true, 443, "/remote.php/dav/files/u"));
        assert!(Location::parse("http://example.com").is_err());
        assert!(Location::parse("sftp://").is_err());
        assert!(Location::parse("/just/a/path").is_err());
    }

    #[test]
    fn names_with_url_characters_survive() {
        let l = Location::parse("ftp://h/a").unwrap().child("100% #1?.txt");
        assert_eq!(l.url(), "ftp://h/a/100% #1?.txt");
        assert_eq!(Location::parse(&l.url()).unwrap().name(), "100% #1?.txt");
    }

    #[test]
    fn urls_round_trip_and_navigate() {
        let l = Location::parse("sftp://u@h:2200/a/b/c").unwrap();
        assert_eq!(l.url(), "sftp://u@h:2200/a/b/c");
        assert_eq!(l.parent().unwrap().url(), "sftp://u@h:2200/a/b");
        assert_eq!(Location::parse("sftp://u@h/a").unwrap().parent().unwrap().url(), "sftp://u@h/");
        assert!(Location::parse("sftp://u@h/").unwrap().parent().is_none());
        assert_eq!(l.child("d").url(), "sftp://u@h:2200/a/b/c/d");
        assert_eq!(l.key(), "sftp://u@h:2200");
        assert_eq!(l.key(), Location::parse("sftp://u@h:2200/other").unwrap().key());
    }

    #[test]
    fn paths_are_normalised() {
        assert_eq!(normalize("/a//b/./c/../d/"), "/a/b/d");
        assert_eq!(normalize(""), "/");
        assert_eq!(normalize("/../.."), "/");
    }

    #[test]
    fn recognizes_server_addresses_only() {
        assert!(is_remote("sftp://h/x"));
        assert!(is_remote("ssh://u@h/x"));
        assert!(is_remote("FTP://h"));
        assert!(!is_remote("/home/x"));
        assert!(!is_remote("C:\\x"));
        assert!(!is_remote("https://h"));
        let s = Location::parse("ssh://me@example.com:2222/var/www").unwrap();
        assert_eq!((s.protocol, s.port, s.path.as_str()), (Protocol::Ssh, 2222, "/var/www"));
    }

    #[test]
    fn server_names_cannot_escape_the_target_folder() {
        assert_eq!(safe_name("../../etc/passwd"), PathBuf::from(".._.._etc_passwd"));
        assert_eq!(safe_name(".."), PathBuf::from("_"));
        assert_eq!(safe_name("a\\b"), PathBuf::from("a_b"));
        assert_eq!(safe_name("fine name.txt"), PathBuf::from("fine name.txt"));
    }

    #[test]
    fn trusted_hosts_are_remembered_and_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("sub/known_hosts");
        assert_eq!(known_host(&f, "h", 22), None);
        remember_host(&f, "h", 22, "SHA256:aaa");
        remember_host(&f, "other", 22, "SHA256:bbb");
        assert_eq!(known_host(&f, "h", 22).as_deref(), Some("SHA256:aaa"));
        remember_host(&f, "h", 22, "SHA256:ccc");
        assert_eq!(known_host(&f, "h", 22).as_deref(), Some("SHA256:ccc"));
        assert_eq!(known_host(&f, "other", 22).as_deref(), Some("SHA256:bbb"));
        assert_eq!(known_host(&f, "h", 2222), None);
    }

    // ---- straight through from one server to another ----

    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    /// A server held in memory. `cut_after` makes reading a file stop with an error after that many bytes.
    #[derive(Clone, Default)]
    struct Memory {
        files: Arc<Mutex<BTreeMap<String, Vec<u8>>>>,
        dirs: Arc<Mutex<Vec<String>>>,
        cut_after: Option<usize>,
        /// The most that was ever waiting between the two ends.
        reads: Arc<Mutex<Vec<usize>>>,
    }

    impl Session for Memory {
        fn home(&mut self) -> Result<String> {
            Ok("/".into())
        }
        fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
            let base = format!("{}/", path.trim_end_matches('/'));
            let mut out = Vec::new();
            for (k, v) in self.files.lock().unwrap().iter() {
                if let Some(rest) = k.strip_prefix(&base) {
                    if !rest.contains('/') {
                        out.push(Entry { name: rest.into(), is_dir: false, size: v.len() as u64, modified: None });
                    }
                }
            }
            for d in self.dirs.lock().unwrap().iter() {
                if let Some(rest) = d.strip_prefix(&base) {
                    if !rest.contains('/') {
                        out.push(Entry { name: rest.into(), is_dir: true, size: 0, modified: None });
                    }
                }
            }
            Ok(out)
        }
        fn download(&mut self, _: &str, _: &Path, _: &Control) -> Result<u64> {
            Err(fail("not used"))
        }
        fn upload(&mut self, _: &Path, _: &str, _: &Control) -> Result<u64> {
            Err(fail("not used"))
        }
        fn streams(&self) -> bool {
            true
        }
        fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
            let data = self.files.lock().unwrap().get(remote).cloned().ok_or_else(|| fail("missing"))?;
            let mut sent = 0;
            for piece in data.chunks(100_000) {
                ctl.check()?;
                if self.cut_after.is_some_and(|c| sent >= c) {
                    return Err(fail("The connection dropped."));
                }
                sink(piece)?;
                sent += piece.len();
            }
            self.reads.lock().unwrap().push(sent);
            Ok(sent as u64)
        }
        fn write_stream(&mut self, source: &mut (dyn std::io::Read + Send), _size: u64, remote: &str, ctl: &Control) -> Result<u64> {
            let mut data = Vec::new();
            let result = pump(source, &mut data, ctl);
            match result {
                Ok(n) => {
                    self.files.lock().unwrap().insert(remote.into(), data);
                    Ok(n)
                }
                Err(e) => Err(e),
            }
        }
        fn mkdir(&mut self, path: &str) -> Result<()> {
            self.dirs.lock().unwrap().push(path.into());
            Ok(())
        }
        fn remove(&mut self, _: &str, _: bool) -> Result<()> {
            Ok(())
        }
        fn rename(&mut self, _: &str, _: &str) -> Result<()> {
            Ok(())
        }
    }

    fn bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    fn with_control<T>(f: impl FnOnce(&Control, &std::sync::atomic::AtomicU64) -> T) -> T {
        let cancel = AtomicBool::new(false);
        let counted = std::sync::atomic::AtomicU64::new(0);
        let on_bytes = |n: u64| {
            counted.fetch_add(n, Ordering::Relaxed);
        };
        let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };
        f(&ctl, &counted)
    }

    #[test]
    fn a_file_passes_straight_through_and_is_counted_once() {
        let mut a = Memory::default();
        let mut b = Memory::default();
        // Bigger than everything that can wait between the ends, so the two really do work at the same time.
        let data = bytes(40 * (1 << 20) + 123);
        a.files.lock().unwrap().insert("/src/big.bin".into(), data.clone());
        with_control(|ctl, counted| {
            let n = relay_file(&mut a, "/src/big.bin", &mut b, "/dst/big.bin", data.len() as u64, ctl).unwrap();
            assert_eq!(n, data.len() as u64);
            assert_eq!(counted.load(Ordering::Relaxed), data.len() as u64, "moved once, not twice");
        });
        assert!(b.files.lock().unwrap().get("/dst/big.bin").unwrap() == &data);
    }

    #[test]
    fn a_source_that_stops_halfway_never_leaves_a_short_file() {
        let mut a = Memory { cut_after: Some(300_000), ..Default::default() };
        let mut b = Memory::default();
        a.files.lock().unwrap().insert("/f".into(), bytes(1_000_000));
        with_control(|ctl, _| {
            let e = relay_file(&mut a, "/f", &mut b, "/g", 1_000_000, ctl).unwrap_err();
            assert!(e.to_string().contains("connection dropped"), "{e}");
        });
        assert!(b.files.lock().unwrap().is_empty());
    }

    #[test]
    fn cancelling_stops_both_ends() {
        let mut a = Memory::default();
        let mut b = Memory::default();
        a.files.lock().unwrap().insert("/f".into(), bytes(5_000_000));
        let cancel = AtomicBool::new(true);
        let on_bytes = |_: u64| {};
        let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };
        assert!(relay_file(&mut a, "/f", &mut b, "/g", 5_000_000, &ctl).is_err());
        assert!(b.files.lock().unwrap().is_empty());
    }

    #[test]
    fn folders_are_copied_whole_and_existing_files_are_kept() {
        let mut a = Memory::default();
        let mut b = Memory::default();
        {
            let mut f = a.files.lock().unwrap();
            f.insert("/pics/one.jpg".into(), bytes(10));
            f.insert("/pics/sub/two.jpg".into(), bytes(20));
        }
        a.dirs.lock().unwrap().push("/pics/sub".into());
        b.files.lock().unwrap().insert("/out/pics/one.jpg".into(), vec![9; 3]);
        b.dirs.lock().unwrap().push("/out/pics".into());
        with_control(|ctl, counted| {
            let mut tally = Tally::default();
            relay_tree_as(&mut a, "/pics", true, 0, &mut b, "/out", "pics", false, ctl, &mut tally).unwrap();
            assert!(tally.failed.is_empty());
            assert_eq!(tally.files, 1, "only the file that was not there yet");
            assert_eq!(counted.load(Ordering::Relaxed), 20);
        });
        let files = b.files.lock().unwrap();
        assert_eq!(files["/out/pics/one.jpg"], vec![9; 3]);
        assert_eq!(files["/out/pics/sub/two.jpg"], bytes(20));
    }

    #[test]
    fn a_listing_goes_out_in_batches_and_all_of_it_arrives() {
        let mut got: Vec<(usize, crate::entry::ListProgress)> = Vec::new();
        {
            let mut sink = |b: &[Entry], p: crate::entry::ListProgress| got.push((b.len(), p));
            let mut batches = Batches::new(&mut sink, 7);
            for i in 0..5 {
                let hidden = i == 2;
                batches.add((!hidden).then(|| Entry { name: format!("f{i}"), is_dir: false, size: 1, modified: None }));
                if i == 3 {
                    std::thread::sleep(Batches::EVERY + std::time::Duration::from_millis(20));
                }
            }
            batches.finish();
        }
        // The first item at once; then the next ones when the interval had passed; then the rest.
        assert_eq!(got.iter().map(|g| g.0).sum::<usize>(), 4);
        assert_eq!(got[0].0, 1);
        assert!(got.len() >= 3, "{got:?}");
        assert_eq!(got.last().unwrap().1.done, 5);
        assert_eq!(got[0].1.total, 7);
        assert!(got.iter().all(|g| !g.1.restart));
    }
}

#[cfg(test)]
mod numbered_name_tests {
    use super::numbered_name;

    #[test]
    fn the_number_goes_before_the_extension() {
        assert_eq!(numbered_name("New file.txt", 2), "New file (2).txt");
        assert_eq!(numbered_name("New folder", 3), "New folder (3)");
        assert_eq!(numbered_name(".hidden", 2), ".hidden (2)");
        assert_eq!(numbered_name("a.tar.gz", 2), "a.tar (2).gz");
    }
}
