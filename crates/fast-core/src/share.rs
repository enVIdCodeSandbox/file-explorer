//! Temporary download links.
//!
//! Sharing a file starts a tiny HTTP server for that one file. The address
//! contains a random 128-bit token, so it cannot be guessed, and the share ends
//! by itself when it expires or has been downloaded as often as allowed. The
//! server only ever serves the single file it was started for: no paths from the
//! request are used, so nothing else on the computer can be reached through it.
//!
//! What this is not: it is plain HTTP (no encryption), and it is reachable on
//! the local network (or just this computer), not from the internet. Anyone
//! who can see the link and the network traffic could use it until it ends.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct ShareOptions {
    /// How long the link works.
    pub minutes: u64,
    /// Complete downloads allowed; 0 means no limit until it expires.
    pub max_downloads: u32,
    /// Reachable by other devices on the network, not just this computer.
    pub lan: bool,
    /// Reachable from anywhere on the internet, through a tunnel program
    /// (`cloudflared`). The file is still served from this computer, which only
    /// listens on its own loopback address; the tunnel carries the traffic out.
    pub internet: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub url: String,
    /// Unix seconds.
    pub expires_at: u64,
    pub max_downloads: u32,
    pub downloads: u32,
    pub lan: bool,
    pub internet: bool,
}

struct Inner {
    stop: AtomicBool,
    downloads: AtomicU32,
    active: AtomicU32,
    /// The tunnel program for an internet share, stopped when the share ends.
    tunnel: Mutex<Option<std::process::Child>>,
}

struct Active {
    info: ShareInfo,
    inner: Arc<Inner>,
}

#[derive(Default)]
pub struct Shares {
    map: Mutex<HashMap<String, Active>>,
}

const MAX_CONNECTIONS: u32 = 8;

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn token() -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| format!("No secure random numbers available: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// This computer's address on the local network, found without sending anything:
/// connecting a UDP socket only asks the OS which interface it would use.
/// The tunnel program, if one is installed. Only `cloudflared` is supported: its
/// "quick tunnels" need no account and give an https address.
pub fn tunnel_program() -> Option<String> {
    let mut c = std::process::Command::new("cloudflared");
    c.arg("--version").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    crate::proc::quiet(&mut c);
    c.status().ok().filter(|s| s.success()).map(|_| "cloudflared".to_string())
}

/// The public address in a line of the tunnel program's output, if there is one.
pub fn find_tunnel_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let url: String = line[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '/')).collect();
    let host = url.strip_prefix("https://")?.split('/').next()?;
    (host.ends_with(".trycloudflare.com") && host.len() > ".trycloudflare.com".len()).then(|| format!("https://{host}"))
}

/// Start a quick tunnel to `127.0.0.1:port` and wait for its public address.
fn open_tunnel(program: &str, port: u16, wait: Duration) -> Result<(std::process::Child, String), String> {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let mut tunnel = Command::new(program);
    crate::proc::quiet(&mut tunnel);
    let mut child = tunnel
        .args(["tunnel", "--no-autoupdate", "--url", &format!("http://127.0.0.1:{port}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start the tunnel: {e}"))?;
    let Some(err) = child.stderr.take() else {
        let _ = child.kill();
        return Err("Could not read the tunnel's output.".into());
    };
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    // The pipe must keep being read, or the tunnel would stall once it fills.
    std::thread::spawn(move || {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            if let Some(u) = find_tunnel_url(&line) {
                let _ = tx.send(u);
            }
        }
    });
    match rx.recv_timeout(wait) {
        Ok(url) => Ok((child, url)),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err("The tunnel did not come up. Check your internet connection and try again.".into())
        }
    }
}

pub fn lan_address() -> Option<IpAddr> {
    let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    s.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

impl Shares {
    pub fn start(&self, path: &Path, opts: ShareOptions) -> Result<ShareInfo, String> {
        let meta = std::fs::metadata(path).map_err(|e| format!("Cannot read that file: {e}"))?;
        if !meta.is_file() {
            return Err("Only single files can be shared this way. Put a folder in an archive first.".into());
        }
        File::open(path).map_err(|e| format!("Cannot read that file: {e}"))?;
        let minutes = opts.minutes.clamp(1, 7 * 24 * 60);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());

        let lan = opts.lan && !opts.internet;
        let bind = if lan { Ipv4Addr::UNSPECIFIED } else { Ipv4Addr::LOCALHOST };
        let listener = TcpListener::bind(SocketAddr::new(bind.into(), 0)).map_err(|e| format!("Cannot open a port: {e}"))?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let host = if lan { lan_address().map(|a| a.to_string()).unwrap_or_else(|| "127.0.0.1".into()) } else { "127.0.0.1".into() };

        let id = token()?;
        let tunnel = if opts.internet {
            let program = tunnel_program().ok_or("Sharing over the internet needs the free cloudflared program, which is not installed. Install it from developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads, then try again.")?;
            Some(open_tunnel(&program, port, Duration::from_secs(30))?)
        } else {
            None
        };
        let base = match &tunnel {
            Some((_, public)) => public.trim_end_matches('/').to_string(),
            None => format!("http://{host}:{port}"),
        };
        let info = ShareInfo {
            id: id.clone(),
            url: format!("{base}/{id}/"),
            name,
            path: path.to_string_lossy().into_owned(),
            size: meta.len(),
            expires_at: now() + minutes * 60,
            max_downloads: opts.max_downloads,
            downloads: 0,
            lan,
            internet: opts.internet,
        };
        let inner = Arc::new(Inner { stop: AtomicBool::new(false), downloads: AtomicU32::new(0), active: AtomicU32::new(0), tunnel: Mutex::new(tunnel.map(|t| t.0)) });
        if opts.internet {
            // Close the tunnel as soon as the share ends, whether stopped, expired or used up.
            let watch = inner.clone();
            std::thread::Builder::new()
                .name("share-tunnel".into())
                .spawn(move || {
                    while !watch.stop.load(Ordering::SeqCst) {
                        std::thread::sleep(Duration::from_millis(300));
                    }
                    if let Some(mut c) = watch.tunnel.lock().take() {
                        let _ = c.kill();
                        let _ = c.wait();
                    }
                })
                .map_err(|e| e.to_string())?;
        }
        let job = Job { info: info.clone(), path: path.to_path_buf(), inner: inner.clone() };
        std::thread::Builder::new().name("share".into()).spawn(move || serve(listener, job)).map_err(|e| e.to_string())?;
        self.map.lock().insert(id, Active { info: info.clone(), inner });
        Ok(info)
    }

    pub fn stop(&self, id: &str) {
        if let Some(a) = self.map.lock().remove(id) {
            a.inner.stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn stop_all(&self) {
        for (_, a) in self.map.lock().drain() {
            a.inner.stop.store(true, Ordering::SeqCst);
            // Do not wait for the watcher thread: the app may be closing.
            if let Some(mut c) = a.inner.tunnel.lock().take() {
                let _ = c.kill();
            }
        }
    }

    /// Shares still running, with their current download counts. Ended ones are dropped.
    pub fn list(&self) -> Vec<ShareInfo> {
        let mut map = self.map.lock();
        map.retain(|_, a| !a.inner.stop.load(Ordering::SeqCst));
        let mut out: Vec<_> = map
            .values()
            .map(|a| ShareInfo { downloads: a.inner.downloads.load(Ordering::SeqCst), ..a.info.clone() })
            .collect();
        out.sort_by_key(|i| i.expires_at);
        out
    }
}

struct Job {
    info: ShareInfo,
    path: PathBuf,
    inner: Arc<Inner>,
}

fn serve(listener: TcpListener, job: Job) {
    let job = Arc::new(job);
    loop {
        if job.inner.stop.load(Ordering::SeqCst) {
            return;
        }
        if now() >= job.info.expires_at {
            job.inner.stop.store(true, Ordering::SeqCst);
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                if job.inner.active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    job.inner.active.fetch_sub(1, Ordering::SeqCst);
                    let mut s = stream;
                    let _ = respond(&mut s, 503, "Busy", "Too many downloads at once. Try again in a moment.");
                    continue;
                }
                let job = job.clone();
                std::thread::spawn(move || {
                    // Close politely: say "no more data" and let the other side finish reading, so a reply is not
                    // cut short by a reset (macOS does that when a socket closes with anything still unread).
                    let closer = stream.try_clone().ok();
                    let _ = handle(stream, &job);
                    if let Some(c) = closer {
                        let _ = c.shutdown(std::net::Shutdown::Write);
                        let _ = c.set_read_timeout(Some(Duration::from_millis(300)));
                        let mut sink = [0u8; 512];
                        let mut c = c;
                        while matches!(c.read(&mut sink), Ok(n) if n > 0) {}
                    }
                    job.inner.active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(80)),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn respond(s: &mut TcpStream, code: u16, reason: &str, text: &str) -> std::io::Result<()> {
    let body = format!("<!doctype html><meta charset=utf-8><title>{reason}</title><body style=\"font:16px system-ui;margin:3em\">{}", html(text));
    write!(
        s,
        "HTTP/1.1 {code} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn html(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A file name that is safe inside a header: no control characters, quotes or separators.
fn header_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_control() || matches!(c, '"' | '\\' | '/' | ';') || !c.is_ascii() { '_' } else { c }).collect();
    if clean.is_empty() { "file".into() } else { clean }
}

fn pct(name: &str) -> String {
    let mut o = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { o.push(b as char) } else { o.push_str(&format!("%{b:02X}")) }
    }
    o
}

fn ended(job: &Job) -> bool {
    job.inner.stop.load(Ordering::SeqCst) || now() >= job.info.expires_at
}

fn handle(mut s: TcpStream, job: &Job) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(30)))?;
    // Only the request line matters. Cap how much is read.
    let mut buf = [0u8; 4096];
    let mut n = 0;
    while n < buf.len() {
        let r = s.read(&mut buf[n..])?;
        if r == 0 {
            break;
        }
        n += r;
        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    let line = text.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" && method != "HEAD" {
        return respond(&mut s, 405, "Method Not Allowed", "Not allowed.");
    }
    let target = target.split('?').next().unwrap_or("");
    let base = format!("/{}/", job.info.id);
    // Every wrong address gets the same answer, so nothing is revealed about what exists.
    let Some(rest) = target.strip_prefix(&base).or_else(|| (target == base.trim_end_matches('/')).then_some("")) else {
        return respond(&mut s, 404, "Not Found", "Nothing here.");
    };
    if ended(job) {
        return respond(&mut s, 410, "Gone", "This link has ended.");
    }
    let hname = header_name(&job.info.name);
    if rest.is_empty() {
        let page = format!(
            "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>{n}</title>\
             <body style=\"font:16px system-ui;max-width:30em;margin:15vh auto;padding:0 1em\"><h2 style=\"word-break:break-all\">{n}</h2><p>{sz} · shared with you</p>\
             <p><a href=\"{base}{p}\" style=\"display:inline-block;padding:.6em 1.2em;background:#0a64d8;color:#fff;border-radius:6px;text-decoration:none\">Download</a></p>",
            n = html(&job.info.name),
            sz = human(job.info.size),
            p = pct(&job.info.name),
        );
        write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n", page.len())?;
        if method == "GET" {
            s.write_all(page.as_bytes())?;
        }
        return Ok(());
    }
    if rest != pct(&job.info.name) {
        return respond(&mut s, 404, "Not Found", "Nothing here.");
    }
    // Claim a download first so parallel requests cannot go past the limit.
    let max = job.info.max_downloads;
    let mine = job.inner.downloads.fetch_add(1, Ordering::SeqCst) + 1;
    if max > 0 && mine > max {
        job.inner.downloads.fetch_sub(1, Ordering::SeqCst);
        return respond(&mut s, 410, "Gone", "This link has been used up.");
    }
    let mut file = match File::open(&job.path) {
        Ok(f) => f,
        Err(_) => {
            job.inner.downloads.fetch_sub(1, Ordering::SeqCst);
            return respond(&mut s, 404, "Not Found", "The file is no longer available.");
        }
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\nContent-Disposition: attachment; filename=\"{hname}\"; filename*=UTF-8''{}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        pct(&job.info.name)
    )?;
    let mut sent = 0u64;
    if method == "GET" {
        let mut chunk = vec![0u8; 256 * 1024];
        loop {
            if job.inner.stop.load(Ordering::SeqCst) {
                break;
            }
            let r = match file.read(&mut chunk) {
                Ok(0) => break,
                Ok(r) => r,
                Err(_) => break,
            };
            if s.write_all(&chunk[..r]).is_err() {
                break;
            }
            sent += r as u64;
        }
    }
    if method == "GET" && sent < len {
        // An interrupted download does not use up the allowance.
        job.inner.downloads.fetch_sub(1, Ordering::SeqCst);
    } else if method == "HEAD" {
        job.inner.downloads.fetch_sub(1, Ordering::SeqCst);
    } else if max > 0 && job.inner.downloads.load(Ordering::SeqCst) >= max {
        job.inner.stop.store(true, Ordering::SeqCst);
    }
    Ok(())
}

fn human(b: u64) -> String {
    let (mut v, mut i) = (b as f64, 0);
    while v >= 1000.0 && i < 4 {
        v /= 1000.0;
        i += 1;
    }
    format!("{} {}", if i == 0 { format!("{v:.0}") } else { format!("{v:.1}") }, ["B", "kB", "MB", "GB", "TB"][i])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(url: &str) -> (u16, Vec<u8>) {
        let rest = url.strip_prefix("http://").unwrap();
        let (host, path) = rest.split_once('/').unwrap();
        let mut s = TcpStream::connect(host).unwrap();
        write!(s, "GET /{path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = Vec::new();
        // A reset after the reply has arrived is still a complete reply.
        match s.read_to_end(&mut out) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset && !out.is_empty() => {}
            Err(e) => panic!("reading the reply failed: {e}"),
        }
        let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = String::from_utf8_lossy(&out[..split]).into_owned();
        let code = head.split(' ').nth(1).unwrap().parse().unwrap();
        (code, out[split + 4..].to_vec())
    }

    fn make(content: &[u8], name: &str) -> (tempdir::Dir, PathBuf) {
        let d = tempdir::Dir::new();
        let p = d.0.join(name);
        std::fs::write(&p, content).unwrap();
        (d, p)
    }

    mod tempdir {
        use std::path::PathBuf;
        pub struct Dir(pub PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!("fast-share-{}-{:?}", std::process::id(), std::thread::current().id()).replace(['(', ')'], ""));
                let _ = std::fs::remove_dir_all(&p);
                std::fs::create_dir_all(&p).unwrap();
                Dir(p)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    const LOCAL: ShareOptions = ShareOptions { minutes: 5, max_downloads: 0, lan: false, internet: false };

    #[test]
    fn serves_only_the_shared_file_at_the_secret_address() {
        let (_d, p) = make(b"hello world", "a b.txt");
        let shares = Shares::default();
        let info = shares.start(&p, LOCAL).unwrap();
        assert_eq!(info.id.len(), 32);
        let (code, body) = get(&format!("{}a%20b.txt", info.url));
        assert_eq!((code, body.as_slice()), (200, b"hello world".as_slice()));
        // Landing page works; anything else, including traversal and a wrong token, is a 404.
        assert_eq!(get(&info.url).0, 200);
        assert_eq!(get(&format!("{}../x", info.url)).0, 404);
        assert_eq!(get(&format!("{}other.txt", info.url)).0, 404);
        let wrong = info.url.replace(&info.id, &"0".repeat(32));
        assert_eq!(get(&wrong).0, 404);
        shares.stop_all();
    }

    #[test]
    fn download_limit_ends_the_share() {
        let (_d, p) = make(b"x", "one.bin");
        let shares = Shares::default();
        let info = shares.start(&p, ShareOptions { max_downloads: 1, ..LOCAL }).unwrap();
        assert_eq!(get(&format!("{}one.bin", info.url)).0, 200);
        std::thread::sleep(Duration::from_millis(300));
        // Over the limit the link is gone: the request is refused, cut off, or never connects.
        assert!(shares.list().iter().all(|i| i.id != info.id) || try_status(&format!("{}one.bin", info.url)) != Some(200));
    }

    /// The status of a request, or None when the connection could not be made, written to or read (a closing share).
    fn try_status(url: &str) -> Option<u16> {
        let rest = url.strip_prefix("http://")?;
        let (host, path) = rest.split_once('/')?;
        let mut s = TcpStream::connect(host).ok()?;
        s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        write!(s, "GET /{path} HTTP/1.1\r\nHost: x\r\n\r\n").ok()?;
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).split(' ').nth(1)?.parse().ok()
    }

    #[test]
    fn folders_and_missing_files_are_refused() {
        let shares = Shares::default();
        assert!(shares.start(&std::env::temp_dir(), LOCAL).is_err());
        assert!(shares.start(Path::new("/definitely/not/here"), LOCAL).is_err());
    }

    #[test]
    fn stopping_closes_the_link() {
        let (_d, p) = make(b"x", "s.bin");
        let shares = Shares::default();
        let info = shares.start(&p, LOCAL).unwrap();
        shares.stop(&info.id);
        std::thread::sleep(Duration::from_millis(400));
        let host = info.url.strip_prefix("http://").unwrap().split('/').next().unwrap().to_string();
        assert!(TcpStream::connect(host).is_err());
    }

    #[test]
    fn finds_the_public_address_in_tunnel_output() {
        let l = "2026-11-12T01:00:00Z INF |  https://random-words-here.trycloudflare.com                                     |";
        assert_eq!(find_tunnel_url(l).as_deref(), Some("https://random-words-here.trycloudflare.com"));
        assert_eq!(find_tunnel_url("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
        assert_eq!(find_tunnel_url("see https://developers.cloudflare.com/docs"), None);
        assert_eq!(find_tunnel_url("https://.trycloudflare.com"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_tunnel_that_prints_an_address_is_used() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _f) = make(b"x", "t.txt");
        let script = dir.0.join("fake-tunnel");
        std::fs::write(&script, "#!/bin/sh\necho 'INF https://fake-one.trycloudflare.com' >&2\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (mut child, url) = open_tunnel(script.to_str().unwrap(), 1234, Duration::from_secs(5)).unwrap();
        assert_eq!(url, "https://fake-one.trycloudflare.com");
        assert!(child.try_wait().unwrap().is_none(), "still running");
        let _ = child.kill();
        // One that never prints an address gives up with a message instead of hanging.
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        let err = open_tunnel(script.to_str().unwrap(), 1234, Duration::from_millis(400)).unwrap_err();
        assert!(err.contains("did not come up"));
    }

    #[test]
    fn names_are_safe_in_headers() {
        assert_eq!(header_name("a\"b\r\nc/é.txt"), "a_b__c__.txt");
        assert_eq!(pct("a b.txt"), "a%20b.txt");
    }
}
