//! Connecting to servers through a proxy: SOCKS4/4a, SOCKS5 (with user name and password) and HTTP CONNECT.
//! One proxy for the whole app, set from the settings (`servers.proxy`, for example `socks5://user:pass@host:1080`).
//! Used by FTP (control and data connections) and SFTP.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::RwLock;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Socks4,
    Socks5,
    Http,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proxy {
    pub kind: Kind,
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub password: Option<String>,
}

static CURRENT: RwLock<Option<Proxy>> = RwLock::new(None);

/// Set (or clear, with empty text) the proxy used from now on. An unreadable address clears it and says why.
pub fn set(url: &str) -> Result<(), String> {
    let t = url.trim();
    let parsed = if t.is_empty() { None } else { Some(parse(t)?) };
    *CURRENT.write().unwrap() = parsed;
    Ok(())
}

pub fn current() -> Option<Proxy> {
    CURRENT.read().unwrap().clone()
}

/// `socks5://user:pass@host:1080`, `socks4://host:1080`, `http://host:8080`. Without a scheme: SOCKS5.
pub fn parse(url: &str) -> Result<Proxy, String> {
    let (scheme, rest) = url.split_once("://").unwrap_or(("socks5", url));
    let kind = match scheme.to_ascii_lowercase().as_str() {
        "socks5" | "socks5h" | "socks" => Kind::Socks5,
        "socks4" | "socks4a" => Kind::Socks4,
        "http" | "https" => Kind::Http,
        other => return Err(format!("\"{other}\" is not a proxy type I know (use socks5, socks4 or http).")),
    };
    let rest = rest.trim_end_matches('/');
    let (auth, hostport) = match rest.rsplit_once('@') {
        Some((a, h)) => (Some(a), h),
        None => (None, rest),
    };
    let (user, password) = match auth {
        Some(a) => match a.split_once(':') {
            Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
            None => (Some(a.to_string()), None),
        },
        None => (None, None),
    };
    let default_port = if kind == Kind::Http { 8080 } else { 1080 };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) && !p.is_empty() => (h.to_string(), p.parse::<u16>().map_err(|_| "The proxy port is not a number up to 65535.".to_string())?),
        _ => (hostport.to_string(), default_port),
    };
    if host.is_empty() {
        return Err("The proxy address has no host.".into());
    }
    Ok(Proxy { kind, host, port, user, password })
}

fn to_io<E: std::fmt::Display>(e: E) -> io::Error {
    io::Error::other(e.to_string())
}

/// Open a connection to `host:port`, through the proxy when there is one.
pub fn connect(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    match current() {
        None => {
            let addr = (host, port).to_socket_addrs()?.next().ok_or_else(|| to_io(format!("Could not find {host}.")))?;
            TcpStream::connect_timeout(&addr, timeout)
        }
        Some(p) => connect_via(&p, host, port, timeout),
    }
}

/// As `connect`, for an address that is already known (the data connection of FTP).
pub fn connect_addr(addr: SocketAddr, timeout: Duration) -> io::Result<TcpStream> {
    match current() {
        None => TcpStream::connect_timeout(&addr, timeout),
        Some(p) => connect_via(&p, &addr.ip().to_string(), addr.port(), timeout),
    }
}

pub fn connect_via(p: &Proxy, host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let addr = (p.host.as_str(), p.port).to_socket_addrs()?.next().ok_or_else(|| to_io(format!("Could not find the proxy {}.", p.host)))?;
    let mut s = TcpStream::connect_timeout(&addr, timeout).map_err(|e| to_io(format!("Could not reach the proxy {}:{}: {e}", p.host, p.port)))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    match p.kind {
        Kind::Socks5 => socks5(&mut s, p, host, port)?,
        Kind::Socks4 => socks4(&mut s, p, host, port)?,
        Kind::Http => http_connect(&mut s, p, host, port)?,
    }
    // The proxy is done talking; the server on the other side sets the pace from here.
    s.set_read_timeout(None)?;
    s.set_write_timeout(None)?;
    Ok(s)
}

fn socks5(s: &mut TcpStream, p: &Proxy, host: &str, port: u16) -> io::Result<()> {
    let want_auth = p.user.is_some();
    s.write_all(if want_auth { &[5, 2, 0, 2] } else { &[5, 1, 0] })?;
    let mut reply = [0u8; 2];
    s.read_exact(&mut reply)?;
    if reply[0] != 5 {
        return Err(to_io("That is not a SOCKS5 proxy."));
    }
    match reply[1] {
        0 => {}
        2 => {
            let (u, pw) = (p.user.clone().unwrap_or_default(), p.password.clone().unwrap_or_default());
            if u.len() > 255 || pw.len() > 255 {
                return Err(to_io("The proxy user name or password is too long."));
            }
            let mut m = vec![1, u.len() as u8];
            m.extend(u.as_bytes());
            m.push(pw.len() as u8);
            m.extend(pw.as_bytes());
            s.write_all(&m)?;
            let mut ok = [0u8; 2];
            s.read_exact(&mut ok)?;
            if ok[1] != 0 {
                return Err(to_io("The proxy did not accept the user name and password."));
            }
        }
        0xff => return Err(to_io("The proxy accepts none of the ways to log in that I offered.")),
        _ => return Err(to_io("The proxy asked for a way to log in that I do not know.")),
    }
    let mut req = vec![5, 1, 0];
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        match ip {
            std::net::IpAddr::V4(v4) => {
                req.push(1);
                req.extend(v4.octets());
            }
            std::net::IpAddr::V6(v6) => {
                req.push(4);
                req.extend(v6.octets());
            }
        }
    } else {
        if host.len() > 255 {
            return Err(to_io("The server name is too long."));
        }
        req.push(3);
        req.push(host.len() as u8);
        req.extend(host.as_bytes());
    }
    req.extend(port.to_be_bytes());
    s.write_all(&req)?;
    let mut head = [0u8; 4];
    s.read_exact(&mut head)?;
    if head[1] != 0 {
        let why = match head[1] {
            1 => "general failure",
            2 => "not allowed by the proxy's rules",
            3 => "network unreachable",
            4 => "host unreachable",
            5 => "connection refused",
            6 => "time to live expired",
            7 => "command not supported",
            8 => "address type not supported",
            _ => "unknown error",
        };
        return Err(to_io(format!("The proxy could not connect to {host}:{port}: {why}.")));
    }
    // The address the proxy bound: skip it.
    let skip = match head[3] {
        1 => 4 + 2,
        4 => 16 + 2,
        3 => {
            let mut l = [0u8; 1];
            s.read_exact(&mut l)?;
            l[0] as usize + 2
        }
        _ => return Err(to_io("The proxy gave an answer I could not read.")),
    };
    let mut rest = vec![0u8; skip];
    s.read_exact(&mut rest)?;
    Ok(())
}

fn socks4(s: &mut TcpStream, p: &Proxy, host: &str, port: u16) -> io::Result<()> {
    let mut req = vec![4, 1];
    req.extend(port.to_be_bytes());
    let v4 = match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(a)) => Some(a),
        Ok(_) => return Err(to_io("SOCKS4 cannot connect to an IPv6 address. Use a SOCKS5 proxy.")),
        Err(_) => None,
    };
    match v4 {
        Some(a) => req.extend(a.octets()),
        None => req.extend([0, 0, 0, 1]), // SOCKS4a: the proxy looks the name up
    }
    req.extend(p.user.clone().unwrap_or_default().as_bytes());
    req.push(0);
    if v4.is_none() {
        req.extend(host.as_bytes());
        req.push(0);
    }
    s.write_all(&req)?;
    let mut r = [0u8; 8];
    s.read_exact(&mut r)?;
    if r[1] != 90 {
        return Err(to_io(format!("The proxy refused to connect to {host}:{port} (code {}).", r[1])));
    }
    Ok(())
}

fn http_connect(s: &mut TcpStream, p: &Proxy, host: &str, port: u16) -> io::Result<()> {
    let target = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    let mut req = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if let Some(u) = &p.user {
        use base64::Engine;
        let cred = base64::engine::general_purpose::STANDARD.encode(format!("{u}:{}", p.password.clone().unwrap_or_default()));
        req.push_str(&format!("Proxy-Authorization: Basic {cred}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes())?;
    // Read the answer one byte at a time up to the blank line, so nothing of the server's greeting is lost.
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if s.read(&mut b)? == 0 || head.len() > 16 * 1024 {
            return Err(to_io("The proxy closed the connection."));
        }
        head.push(b[0]);
    }
    let line = String::from_utf8_lossy(&head).lines().next().unwrap_or("").to_string();
    let code = line.split_whitespace().nth(1).unwrap_or("");
    if code != "200" {
        return Err(to_io(format!("The proxy refused to connect to {host}:{port}: {}", line.trim())));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn spawn_server(handler: impl FnOnce(TcpStream) + Send + 'static) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((s, _)) = l.accept() {
                handler(s);
            }
        });
        port
    }

    #[test]
    fn parses_addresses() {
        assert_eq!(parse("socks5://u:p@h:9").unwrap(), Proxy { kind: Kind::Socks5, host: "h".into(), port: 9, user: Some("u".into()), password: Some("p".into()) });
        assert_eq!(parse("proxy.example").unwrap().port, 1080);
        assert_eq!(parse("http://p").unwrap().port, 8080);
        assert!(parse("ftp://x").is_err());
        assert!(parse("socks5://:80").is_err());
    }

    #[test]
    fn socks5_with_login_carries_the_name_to_the_proxy() {
        let port = spawn_server(|mut s| {
            let mut g = [0u8; 4];
            s.read_exact(&mut g).unwrap();
            assert_eq!(g, [5, 2, 0, 2]);
            s.write_all(&[5, 2]).unwrap();
            let mut a = [0u8; 2];
            s.read_exact(&mut a).unwrap();
            let mut user = vec![0u8; a[1] as usize];
            s.read_exact(&mut user).unwrap();
            let mut pl = [0u8; 1];
            s.read_exact(&mut pl).unwrap();
            let mut pw = vec![0u8; pl[0] as usize];
            s.read_exact(&mut pw).unwrap();
            assert_eq!((user, pw), (b"bob".to_vec(), b"pw".to_vec()));
            s.write_all(&[1, 0]).unwrap();
            let mut head = [0u8; 5];
            s.read_exact(&mut head).unwrap();
            assert_eq!(&head[..4], &[5, 1, 0, 3]);
            let mut name = vec![0u8; head[4] as usize + 2];
            s.read_exact(&mut name).unwrap();
            assert_eq!(&name[..name.len() - 2], b"files.example");
            assert_eq!(&name[name.len() - 2..], &21u16.to_be_bytes());
            s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
            s.write_all(b"220 hello\r\n").unwrap();
        });
        let p = Proxy { kind: Kind::Socks5, host: "127.0.0.1".into(), port, user: Some("bob".into()), password: Some("pw".into()) };
        let mut c = connect_via(&p, "files.example", 21, Duration::from_secs(5)).unwrap();
        let mut greeting = [0u8; 11];
        c.read_exact(&mut greeting).unwrap();
        assert_eq!(&greeting, b"220 hello\r\n");
    }

    #[test]
    fn socks5_refusal_is_explained() {
        let port = spawn_server(|mut s| {
            let mut g = [0u8; 3];
            s.read_exact(&mut g).unwrap();
            s.write_all(&[5, 0]).unwrap();
            let mut req = [0u8; 10];
            s.read_exact(&mut req).unwrap();
            s.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        });
        let p = Proxy { kind: Kind::Socks5, host: "127.0.0.1".into(), port, user: None, password: None };
        let e = connect_via(&p, "10.0.0.5", 21, Duration::from_secs(5)).unwrap_err().to_string();
        assert!(e.contains("connection refused"), "{e}");
    }

    #[test]
    fn socks4a_sends_the_name() {
        let port = spawn_server(|mut s| {
            let mut buf = [0u8; 64];
            let n = s.read(&mut buf).unwrap();
            assert_eq!(&buf[..2], &[4, 1]);
            assert_eq!(&buf[4..8], &[0, 0, 0, 1]);
            assert!(buf[..n].ends_with(b"files.example\0"));
            s.write_all(&[0, 90, 0, 0, 0, 0, 0, 0]).unwrap();
        });
        let p = Proxy { kind: Kind::Socks4, host: "127.0.0.1".into(), port, user: None, password: None };
        connect_via(&p, "files.example", 21, Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn http_connect_with_login_and_no_lost_bytes() {
        let port = spawn_server(|mut s| {
            let mut got = Vec::new();
            let mut b = [0u8; 1];
            while !got.ends_with(b"\r\n\r\n") {
                s.read_exact(&mut b).unwrap();
                got.push(b[0]);
            }
            let text = String::from_utf8(got).unwrap();
            assert!(text.starts_with("CONNECT files.example:21 HTTP/1.1"));
            assert!(text.contains("Proxy-Authorization: Basic Ym9iOnB3"));
            s.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n220 hi\r\n").unwrap();
        });
        let p = Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port, user: Some("bob".into()), password: Some("pw".into()) };
        let mut c = connect_via(&p, "files.example", 21, Duration::from_secs(5)).unwrap();
        let mut greeting = [0u8; 8];
        c.read_exact(&mut greeting).unwrap();
        assert_eq!(&greeting, b"220 hi\r\n");
    }

    #[test]
    fn http_connect_refusal() {
        let port = spawn_server(|mut s| {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf);
            s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").unwrap();
        });
        let p = Proxy { kind: Kind::Http, host: "127.0.0.1".into(), port, user: None, password: None };
        assert!(connect_via(&p, "x.example", 21, Duration::from_secs(5)).unwrap_err().to_string().contains("403"));
    }
}
