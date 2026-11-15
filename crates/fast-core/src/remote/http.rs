//! A small HTTP/1.1 client, just enough for WebDAV.
//!
//! WebDAV needs methods such as PROPFIND, MKCOL and MOVE, which the common
//! Rust HTTP clients refuse to send, so this talks to the server directly
//! (plain TCP, or TLS through rustls). Short requests reuse one connection;
//! file transfers use their own and stream straight to or from disk.

use super::fail;
use crate::error::Result;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IO_TIMEOUT: Duration = Duration::from_secs(60);
/// Answers to short requests (listings) are read into memory up to this size.
const MAX_TEXT: u64 = 64 * 1024 * 1024;
const MAX_REDIRECTS: u32 = 5;

pub(super) enum Stream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

pub(super) type Conn = BufReader<Stream>;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Framing {
    None,
    Length(u64),
    Chunked,
    Close,
}

pub(super) struct Reply {
    pub status: u16,
    headers: Vec<(String, String)>,
    conn: Conn,
    framing: Framing,
    keep_alive: bool,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Read the whole body as text and give the connection back for reuse.
    pub fn text(self, idle: &mut Option<Conn>) -> Result<String> {
        let keep = self.keep_alive && self.framing != Framing::Close;
        let mut body = Body { conn: self.conn, framing: self.framing, left: match self.framing { Framing::Length(n) => n, _ => 0 }, chunk_started: false, done: self.framing == Framing::None };
        let mut bytes = Vec::new();
        (&mut body).take(MAX_TEXT).read_to_end(&mut bytes).map_err(|e| fail(format!("Could not read the answer: {e}")))?;
        if keep && body.done {
            *idle = Some(body.conn);
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Stream the body.
    pub fn into_reader(self) -> impl Read {
        Body { conn: self.conn, framing: self.framing, left: match self.framing { Framing::Length(n) => n, _ => 0 }, chunk_started: false, done: self.framing == Framing::None }
    }
}

struct Body {
    conn: Conn,
    framing: Framing,
    left: u64,
    chunk_started: bool,
    done: bool,
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done || buf.is_empty() {
            return Ok(0);
        }
        match self.framing {
            Framing::None => Ok(0),
            Framing::Close => {
                let n = self.conn.read(buf)?;
                if n == 0 {
                    self.done = true;
                }
                Ok(n)
            }
            Framing::Length(_) => {
                let want = buf.len().min(self.left as usize);
                let n = self.conn.read(&mut buf[..want])?;
                if n == 0 {
                    return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "The connection ended before the file was complete."));
                }
                self.left -= n as u64;
                if self.left == 0 {
                    self.done = true;
                }
                Ok(n)
            }
            Framing::Chunked => {
                if self.left == 0 {
                    if self.chunk_started {
                        // The CRLF after the chunk's data.
                        let mut crlf = String::new();
                        self.conn.read_line(&mut crlf)?;
                    }
                    let mut line = String::new();
                    if self.conn.read_line(&mut line)? == 0 {
                        return Err(std::io::ErrorKind::UnexpectedEof.into());
                    }
                    let size = line.trim().split(';').next().unwrap_or("0");
                    self.left = u64::from_str_radix(size.trim(), 16).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad chunk size"))?;
                    self.chunk_started = true;
                    if self.left == 0 {
                        // Trailers, up to the empty line.
                        loop {
                            let mut t = String::new();
                            if self.conn.read_line(&mut t)? == 0 || t.trim().is_empty() {
                                break;
                            }
                        }
                        self.done = true;
                        return Ok(0);
                    }
                }
                let want = buf.len().min(self.left as usize);
                let n = self.conn.read(&mut buf[..want])?;
                if n == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                self.left -= n as u64;
                Ok(n)
            }
        }
    }
}

pub(super) struct Client {
    host: String,
    port: u16,
    tls: bool,
    auth: Option<String>,
    config: Option<Arc<ClientConfig>>,
    idle: Option<Conn>,
}

fn tls_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Also trust what the operating system trusts (company and home CAs).
    let native = rustls_native_certs::load_native_certs();
    for cert in native.certs {
        let _ = roots.add(cert);
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions().expect("TLS versions").with_root_certificates(roots).with_no_client_auth();
    Arc::new(config)
}

impl Client {
    pub fn new(host: &str, port: u16, tls: bool, user: &str, password: &str) -> Client {
        let auth = (!user.is_empty()).then(|| format!("Basic {}", base64(format!("{user}:{password}").as_bytes())));
        Client { host: host.to_string(), port, tls, auth, config: tls.then(tls_config), idle: None }
    }

    /// `scheme://host[:port]` as other code writes it in a `Destination` header.
    pub fn origin(&self) -> String {
        let default = if self.tls { 443 } else { 80 };
        let host = if self.port == default { self.host.clone() } else { format!("{}:{}", self.host, self.port) };
        format!("{}://{host}", if self.tls { "https" } else { "http" })
    }

    fn open(&self) -> Result<Conn> {
        let addr = (self.host.as_str(), self.port).to_socket_addrs().map_err(|e| fail(format!("Could not find {}: {e}", self.host)))?.next().ok_or_else(|| fail(format!("Could not find {}.", self.host)))?;
        let tcp = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|e| fail(format!("Could not connect to {}: {e}", self.host)))?;
        let _ = tcp.set_nodelay(true);
        let _ = tcp.set_read_timeout(Some(IO_TIMEOUT));
        let _ = tcp.set_write_timeout(Some(IO_TIMEOUT));
        let stream = match &self.config {
            None => Stream::Plain(tcp),
            Some(config) => {
                let name = ServerName::try_from(self.host.clone()).map_err(|_| fail(format!("\"{}\" is not a valid server name.", self.host)))?;
                let conn = ClientConnection::new(config.clone(), name).map_err(|e| fail(format!("Secure connection failed: {e}")))?;
                Stream::Tls(Box::new(StreamOwned::new(conn, tcp)))
            }
        };
        Ok(BufReader::with_capacity(64 * 1024, stream))
    }

    fn write_head(&self, conn: &mut Conn, method: &str, path: &str, headers: &[(&str, &str)], length: Option<u64>, close: bool) -> std::io::Result<()> {
        let host = if self.port == if self.tls { 443 } else { 80 } { self.host.clone() } else { format!("{}:{}", self.host, self.port) };
        let mut head = format!("{method} {} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: FAST-File-Explorer\r\nAccept: */*\r\n", super::webdav::encode_path(path));
        if let Some(a) = &self.auth {
            head.push_str(&format!("Authorization: {a}\r\n"));
        }
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if let Some(n) = length {
            head.push_str(&format!("Content-Length: {n}\r\n"));
        }
        head.push_str(if close { "Connection: close\r\n\r\n" } else { "Connection: keep-alive\r\n\r\n" });
        conn.get_mut().write_all(head.as_bytes())
    }

    fn read_head(mut conn: Conn, method: &str) -> std::io::Result<Reply> {
        let mut line = String::new();
        loop {
            line.clear();
            if conn.read_line(&mut line)? == 0 {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            // Skip "100 Continue" style interim answers.
            let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "The server did not answer with HTTP."))?;
            let mut headers = Vec::new();
            loop {
                line.clear();
                if conn.read_line(&mut line)? == 0 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                let l = line.trim_end();
                if l.is_empty() {
                    break;
                }
                if let Some((k, v)) = l.split_once(':') {
                    headers.push((k.trim().to_string(), v.trim().to_string()));
                }
            }
            if (100..200).contains(&status) && status != 101 {
                continue;
            }
            let get = |name: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.to_ascii_lowercase());
            let framing = if method == "HEAD" || status == 204 || status == 304 {
                Framing::None
            } else if get("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
                Framing::Chunked
            } else if let Some(n) = get("content-length").and_then(|v| v.parse::<u64>().ok()) {
                if n == 0 { Framing::None } else { Framing::Length(n) }
            } else {
                Framing::Close
            };
            let keep_alive = !get("connection").is_some_and(|v| v.contains("close"));
            return Ok(Reply { status, headers, conn, framing, keep_alive });
        }
    }

    /// A short request with an in-memory body, on the shared connection.
    /// Follows redirects within the same server.
    pub fn call(&mut self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Reply> {
        let mut path = path.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let reply = self.call_once(method, &path, headers, body)?;
            if matches!(reply.status, 301 | 302 | 307 | 308) {
                if let Some(next) = reply.header("Location").and_then(|l| self.local_path(l)) {
                    // Throw the redirect's body away.
                    let _ = reply.text(&mut self.idle);
                    path = next;
                    continue;
                }
            }
            return Ok(reply);
        }
        Err(fail("The server keeps redirecting."))
    }

    /// The path part of a redirect target, when it points to this server.
    fn local_path(&self, location: &str) -> Option<String> {
        match location.split_once("://") {
            Some((_, rest)) => {
                let i = rest.find('/')?;
                let host = &rest[..i];
                (host.split(':').next() == Some(self.host.as_str())).then(|| super::webdav::decode_path(&rest[i..]))
            }
            None if location.starts_with('/') => Some(super::webdav::decode_path(location)),
            None => None,
        }
    }

    fn call_once(&mut self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Reply> {
        let mut last = None;
        // A kept connection may have been closed by the server meanwhile: try again on a fresh one.
        for attempt in 0..2 {
            let reused = attempt == 0 && self.idle.is_some();
            let mut conn = match (reused, self.idle.take()) {
                (true, Some(c)) => c,
                _ => self.open()?,
            };
            let length = (!body.is_empty() || matches!(method, "PUT" | "POST" | "PROPFIND" | "PROPPATCH")).then_some(body.len() as u64);
            let sent = self.write_head(&mut conn, method, path, headers, length, false).and_then(|_| if body.is_empty() { Ok(()) } else { conn.get_mut().write_all(body) }).and_then(|_| conn.get_mut().flush());
            let result = sent.and_then(|_| Self::read_head(conn, method));
            match result {
                Ok(r) => return Ok(r),
                Err(e) if reused => last = Some(e),
                Err(e) => return Err(fail(describe(&self.host, &e))),
            }
        }
        Err(fail(describe(&self.host, &last.unwrap())))
    }

    /// Start a download on its own connection; read the body with [`Reply::into_reader`].
    pub fn get(&mut self, path: &str) -> Result<Reply> {
        let mut path = path.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let mut conn = self.open()?;
            self.write_head(&mut conn, "GET", &path, &[], None, true).map_err(|e| fail(describe(&self.host, &e)))?;
            let reply = Self::read_head(conn, "GET").map_err(|e| fail(describe(&self.host, &e)))?;
            if matches!(reply.status, 301 | 302 | 307 | 308) {
                if let Some(next) = reply.header("Location").and_then(|l| self.local_path(l)) {
                    path = next;
                    continue;
                }
            }
            return Ok(reply);
        }
        Err(fail("The server keeps redirecting."))
    }

    /// Send a file with PUT on its own connection. Returns the status.
    pub fn put(&mut self, path: &str, body: &mut dyn Read, length: u64) -> Result<u16> {
        let mut conn = self.open()?;
        self.write_head(&mut conn, "PUT", path, &[("Content-Type", "application/octet-stream")], Some(length), true).map_err(|e| fail(describe(&self.host, &e)))?;
        let mut buf = vec![0u8; 128 * 1024];
        let mut sent = 0u64;
        while sent < length {
            let want = buf.len().min((length - sent) as usize);
            let n = body.read(&mut buf[..want]).map_err(|e| fail(e))?;
            if n == 0 {
                return Err(fail("The file changed while it was being sent."));
            }
            if let Err(e) = conn.get_mut().write_all(&buf[..n]) {
                // The server may have refused early (full disk, no permission): its answer says why.
                if let Ok(r) = Self::read_head(conn, "PUT") {
                    return Ok(r.status);
                }
                return Err(fail(describe(&self.host, &e)));
            }
            sent += n as u64;
        }
        conn.get_mut().flush().map_err(|e| fail(describe(&self.host, &e)))?;
        Self::read_head(conn, "PUT").map(|r| r.status).map_err(|e| fail(describe(&self.host, &e)))
    }

    pub fn idle(&mut self) -> &mut Option<Conn> {
        &mut self.idle
    }
}

fn describe(host: &str, e: &std::io::Error) -> String {
    let text = e.to_string();
    if text.to_lowercase().contains("certificate") || text.contains("UnknownIssuer") || text.contains("invalid peer") {
        return format!("{host} has a security certificate that this computer does not trust ({text}).");
    }
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => format!("{host} did not answer in time."),
        _ => format!("Lost the connection to {host}: {text}"),
    }
}

pub(super) fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// A tiny server that answers each connection with canned bytes.
    fn serve(answers: Vec<&'static str>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for answer in answers {
                let (mut s, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                s.write_all(answer.as_bytes()).unwrap();
            }
        });
        port
    }

    #[test]
    fn reads_content_length_and_chunked_bodies() {
        let port = serve(vec!["HTTP/1.1 207 Multi-Status\r\nContent-Length: 5\r\n\r\nhello", "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n4;ext=1\r\ndefg\r\n0\r\nX: y\r\n\r\n", "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nuntil close"]);
        let mut c = Client::new("127.0.0.1", port, false, "", "");
        let r = c.call("PROPFIND", "/", &[], b"").unwrap();
        assert_eq!(r.status, 207);
        assert_eq!(r.text(&mut None).unwrap(), "hello");
        let mut c = Client::new("127.0.0.1", port, false, "", "");
        let r = c.call("GET", "/", &[], b"").unwrap();
        assert_eq!(r.text(&mut None).unwrap(), "abcdefg");
        let mut c = Client::new("127.0.0.1", port, false, "", "");
        let mut body = String::new();
        c.get("/").unwrap().into_reader().read_to_string(&mut body).unwrap();
        assert_eq!(body, "until close");
    }

    #[test]
    fn follows_redirects_on_the_same_server() {
        let port = serve(vec!["HTTP/1.1 301 Moved\r\nLocation: /folder/\r\nContent-Length: 0\r\n\r\n", "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"]);
        let mut c = Client::new("127.0.0.1", port, false, "", "");
        let r = c.call("GET", "/folder", &[], b"").unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.text(&mut None).unwrap(), "ok");
    }

    #[test]
    fn short_answers_without_a_body_work() {
        let port = serve(vec!["HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n", "HTTP/1.1 204 No Content\r\n\r\n"]);
        let mut c = Client::new("127.0.0.1", port, false, "u", "p");
        assert_eq!(c.call("MKCOL", "/x", &[], b"").unwrap().status, 201);
        let mut c = Client::new("127.0.0.1", port, false, "u", "p");
        assert_eq!(c.call("DELETE", "/x", &[], b"").unwrap().status, 204);
    }

    #[test]
    fn an_unreachable_server_is_explained() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut c = Client::new("127.0.0.1", port, false, "", "");
        let err = c.call("GET", "/", &[], b"").err().unwrap().to_string();
        // The port was free a moment ago, but another test may have taken it since (macOS hands out
        // the same numbers quickly), and then the connection is reset instead of refused. Either way
        // the person gets a sentence that names the server, not a bare system error.
        assert!(err.contains("Could not connect") || err.contains("Lost the connection to 127.0.0.1"), "{err}");
    }
}
