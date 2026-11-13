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
mod sftp;
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
    /// WebDAV over HTTP or HTTPS.
    Dav,
}

impl Protocol {
    pub fn default_port(self, tls: bool) -> u16 {
        match (self, tls) {
            (Protocol::Ftp, _) => 21,
            (Protocol::Sftp, _) => 22,
            (Protocol::Dav, false) => 80,
            (Protocol::Dav, true) => 443,
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
            (Protocol::Dav, false) => "dav",
            (Protocol::Dav, true) => "davs",
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
    ["ftp://", "sftp://", "dav://", "davs://"].iter().any(|p| s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p))
}

impl Location {
    pub fn parse(s: &str) -> Result<Location> {
        let bad = || CoreError::Invalid(format!("\"{s}\" is not a server address (like sftp://user@host/folder)."));
        let (scheme, rest) = s.split_once("://").ok_or_else(bad)?;
        let (protocol, tls) = match scheme.to_ascii_lowercase().as_str() {
            "ftp" => (Protocol::Ftp, false),
            "sftp" => (Protocol::Sftp, false),
            "dav" => (Protocol::Dav, false),
            "davs" => (Protocol::Dav, true),
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
    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64>;
    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64>;
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
        Protocol::Dav => webdav::connect(server),
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
        assert!(is_remote("FTP://h"));
        assert!(!is_remote("/home/x"));
        assert!(!is_remote("C:\\x"));
        assert!(!is_remote("https://h"));
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
}
