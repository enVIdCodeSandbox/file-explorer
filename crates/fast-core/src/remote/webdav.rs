//! WebDAV over HTTP or HTTPS (Nextcloud, ownCloud, Synology, Apache, nginx,
//! NAS boxes). Logs in with Basic authentication, which is why plain HTTP is
//! flagged in the interface.

use super::http::Client;
use super::{fail, pump, Batches, Control, Entry, Server, Session};
use crate::error::{CoreError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

struct Dav {
    http: Client,
}

pub(super) fn connect(server: &Server) -> Result<Box<dyn Session>> {
    let mut dav = Dav { http: Client::new(&server.host, server.port, server.tls, &server.user, &server.password) };
    // Check the address and login now, so a mistake shows up when connecting.
    let status = dav.http.call("PROPFIND", "/", &[("Depth", "0"), ("Content-Type", "application/xml; charset=utf-8")], PROPFIND_BODY.as_bytes())?.status;
    match status {
        200 | 207 => Ok(Box::new(dav)),
        401 => Err(fail("The server did not accept the login (check the user name and password).")),
        403 => Err(fail("The server refused access.")),
        404 => Err(fail("The server has nothing at this address. Check the path.")),
        s => Err(fail(format!("The server answered {s}. Is this a WebDAV address?"))),
    }
}

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getlastmodified/></d:prop></d:propfind>"#;

/// Percent-encode a path for a request, keeping the slashes.
pub(super) fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(super) fn decode_path(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn expect(what: &str, status: u16, ok: &[u16]) -> Result<()> {
    if ok.contains(&status) {
        return Ok(());
    }
    Err(fail(match status {
        401 => "The server did not accept the login.".to_string(),
        403 => format!("The server does not allow {what}."),
        404 => format!("Not found on the server ({what})."),
        405 => format!("The server does not allow {what} here."),
        409 => format!("The server could not {what}: a folder in the path is missing."),
        412 => format!("The server could not {what}: the destination already exists."),
        423 => format!("The server could not {what}: the item is locked."),
        507 => "The server is out of space.".to_string(),
        s => format!("The server answered {s} when asked to {what}."),
    }))
}

/// Parse a 207 Multi-Status listing into (path, entry) pairs.
pub(super) fn parse_multistatus(xml: &str) -> Vec<(String, Entry)> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut tag = String::new();
    let (mut href, mut len, mut modified, mut dir) = (String::new(), 0u64, None, false);
    let mut in_response = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_lowercase();
                match name.as_str() {
                    "response" => {
                        in_response = true;
                        (href, len, modified, dir) = (String::new(), 0, None, false);
                    }
                    "collection" if in_response => dir = true,
                    _ => {}
                }
                tag = name;
            }
            Ok(Event::Text(t)) if in_response => {
                let text = t.xml10_content().into_owned();
                match tag.as_str() {
                    "href" => href.push_str(text.trim()),
                    "getcontentlength" => len = text.trim().parse().unwrap_or(0),
                    "getlastmodified" => modified = chrono::DateTime::parse_from_rfc2822(text.trim()).ok().map(|d| d.timestamp()),
                    _ => {}
                }
            }
            // Entities such as &amp; arrive as their own events.
            Ok(Event::GeneralRef(r)) if in_response && tag == "href" => {
                let name = r.xml10_content();
                match name.as_ref() {
                    "amp" => href.push('&'),
                    "lt" => href.push('<'),
                    "gt" => href.push('>'),
                    "quot" => href.push('"'),
                    "apos" => href.push('\''),
                    _ => {
                        if let Ok(Some(c)) = r.resolve_char_ref() {
                            href.push(c);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                if e.local_name().as_ref() == "response" && in_response {
                    in_response = false;
                    // An href may be a full URL or just a path.
                    let path = match href.split_once("://") {
                        Some((_, rest)) => rest.find('/').map(|i| &rest[i..]).unwrap_or("/").to_string(),
                        None => href.clone(),
                    };
                    let path = decode_path(&path);
                    let trimmed = path.trim_end_matches('/');
                    let name = trimmed.rsplit('/').next().unwrap_or("").to_string();
                    out.push((path.clone(), Entry { name, is_dir: dir || path.ends_with('/'), size: if dir { 0 } else { len }, modified }));
                }
                tag.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Where the last complete `<response>…</response>` in `buf` ends (0 when there is none yet).
fn complete_upto(buf: &[u8]) -> usize {
    let tail = b"response>";
    let mut end = buf.len();
    while let Some(at) = buf[..end].windows(tail.len()).rposition(|w| w == tail) {
        // A closing tag has "</" (and maybe a prefix such as "d:") before the name.
        let open = buf[..at].iter().rposition(|&b| b == b'<');
        if open.is_some_and(|o| buf.get(o + 1) == Some(&b'/')) {
            return at + tail.len();
        }
        end = at;
    }
    0
}

impl Dav {
    fn simple(&mut self, method: &str, path: &str, headers: &[(&str, &str)]) -> Result<u16> {
        let reply = self.http.call(method, path, headers, b"")?;
        let status = reply.status;
        let _ = reply.text(self.http.idle());
        Ok(status)
    }
}

impl Session for Dav {
    fn home(&mut self) -> Result<String> {
        Ok("/".into())
    }

    fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let reply = self.http.call("PROPFIND", path, &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")], PROPFIND_BODY.as_bytes())?;
        let status = reply.status;
        let xml = reply.text(self.http.idle())?;
        expect("list this folder", status, &[207, 200])?;
        let me = path.trim_end_matches('/');
        Ok(parse_multistatus(&xml).into_iter().filter(|(p, _)| p.trim_end_matches('/') != me && !p.is_empty()).map(|(_, e)| e).filter(|e| !e.name.is_empty()).collect())
    }

    /// The answer is parsed while it arrives, so a big folder starts to fill before the server has finished sending.
    fn list_streaming(&mut self, path: &str, cancel: &AtomicBool, sink: &mut dyn FnMut(&[Entry], crate::entry::ListProgress)) -> Result<Vec<Entry>> {
        let reply = self.http.call("PROPFIND", path, &[("Depth", "1"), ("Content-Type", "application/xml; charset=utf-8")], PROPFIND_BODY.as_bytes())?;
        expect("list this folder", reply.status, &[207, 200])?;
        let me = path.trim_end_matches('/').to_string();
        let mut body = reply.into_reader();
        let (mut out, mut batches) = (Vec::new(), Batches::new(sink, 0));
        let (mut buf, mut chunk, mut tail) = (Vec::<u8>::new(), vec![0u8; 64 * 1024], Vec::<u8>::new());
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(fail("Cancelled."));
            }
            let n = match body.read(&mut chunk) {
                Ok(n) => n,
                // Some servers reset the connection right after the last byte. If the closing tag already came, the
                // list is whole and the reset is only how the server hung up.
                Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted) && ended(&tail) => 0,
                Err(e) => return Err(fail(format!("Could not read the answer: {e}"))),
            };
            buf.extend_from_slice(&chunk[..n]);
            tail.extend_from_slice(&chunk[..n]);
            if tail.len() > 64 {
                tail.drain(..tail.len() - 64);
            }
            let upto = if n == 0 { buf.len() } else { complete_upto(&buf) };
            if upto > 0 {
                let part = String::from_utf8_lossy(&buf[..upto]).into_owned();
                buf.drain(..upto);
                for (p, e) in parse_multistatus(&part) {
                    if p.trim_end_matches('/') == me || p.is_empty() || e.name.is_empty() {
                        continue;
                    }
                    out.push(e.clone());
                    batches.add(Some(e));
                }
            }
            if n == 0 {
                break;
            }
        }
        batches.finish();
        Ok(out)
    }

    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let reply = self.http.get(remote)?;
        expect("download this file", reply.status, &[200])?;
        let mut body = reply.into_reader();
        let mut out = File::create(local).map_err(|e| CoreError::io(local, e))?;
        let result = pump(&mut body, &mut out, ctl);
        if result.is_err() {
            drop(out);
            let _ = std::fs::remove_file(local);
        }
        result
    }

    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
        let mut file = File::open(local).map_err(|e| CoreError::io(local, e))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut counting = Counting { inner: &mut file, ctl, total: 0, cancelled: false };
        let result = self.http.put(remote, &mut counting, len);
        if counting.cancelled {
            // Do not leave half a file on the server. The server may only
            // finish noticing the broken upload a moment later, so look again.
            for _ in 0..8 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if matches!(self.simple("DELETE", remote, &[]), Ok(200 | 202 | 204)) {
                    break;
                }
            }
            return Err(fail("Cancelled."));
        }
        expect("upload this file", result?, &[200, 201, 204])?;
        Ok(counting.total)
    }

    fn streams(&self) -> bool {
        true
    }

    fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
        let reply = self.http.get(remote)?;
        expect("download this file", reply.status, &[200])?;
        let mut body = reply.into_reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut total = 0u64;
        loop {
            ctl.check()?;
            let n = body.read(&mut buf).map_err(|e| fail(e))?;
            if n == 0 {
                return Ok(total);
            }
            sink(&buf[..n])?;
            total += n as u64;
        }
    }

    fn write_stream(&mut self, source: &mut (dyn Read + Send), size: u64, remote: &str, ctl: &Control) -> Result<u64> {
        let mut counting = Counting { inner: source, ctl, total: 0, cancelled: false };
        let result = self.http.put(remote, &mut counting, size);
        let failed = result.is_err() || counting.cancelled;
        let status = match result {
            Ok(s) if !counting.cancelled => expect("upload this file", s, &[200, 201, 204]),
            Ok(_) => Err(fail("Cancelled.")),
            Err(e) => Err(e),
        };
        if failed || status.is_err() {
            // Do not leave half a file on the server (it may only notice the broken upload a moment later).
            for _ in 0..8 {
                std::thread::sleep(std::time::Duration::from_millis(150));
                if matches!(self.simple("DELETE", remote, &[]), Ok(200 | 202 | 204 | 404)) {
                    break;
                }
            }
        }
        status?;
        Ok(counting.total)
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        let status = self.simple("MKCOL", path, &[])?;
        expect("create this folder", status, &[201])
    }

    fn remove(&mut self, path: &str, is_dir: bool) -> Result<()> {
        let target = if is_dir && !path.ends_with('/') { format!("{path}/") } else { path.to_string() };
        let status = self.simple("DELETE", &target, &[])?;
        expect("delete this item", status, &[200, 202, 204])
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let dest = format!("{}{}", self.http.origin(), encode_path(to));
        let status = self.simple("MOVE", from, &[("Destination", &dest), ("Overwrite", "F")])?;
        expect("rename this item", status, &[201, 204])
    }
}

/// Reads a file for upload, counting bytes and noticing Cancel.
struct Counting<'a, 'b, R: Read + ?Sized = File> {
    inner: &'a mut R,
    ctl: &'a Control<'b>,
    total: u64,
    cancelled: bool,
}

impl<R: Read + ?Sized> Read for Counting<'_, '_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.ctl.check().is_err() {
            self.cancelled = true;
            return Err(std::io::Error::other("Cancelled."));
        }
        let n = self.inner.read(buf)?;
        self.total += n as u64;
        (self.ctl.on_bytes)(n as u64);
        Ok(n)
    }
}

/// True when the last bytes of an answer close the multistatus element (whatever prefix the server gave it).
fn ended(tail: &[u8]) -> bool {
    let text = String::from_utf8_lossy(tail).to_ascii_lowercase();
    let text = text.trim_end();
    text.ends_with("multistatus>") && text.rfind("</").is_some_and(|at| text[at..].contains("multistatus>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_encoded_and_decoded() {
        assert_eq!(encode_path("/a b/100%/ü.txt"), "/a%20b/100%25/%C3%BC.txt");
        assert_eq!(decode_path("/a%20b/100%25/%C3%BC.txt"), "/a b/100%/ü.txt");
        assert_eq!(decode_path("/plain"), "/plain");
        assert_eq!(decode_path("/trailing%"), "/trailing%");
        assert_eq!(decode_path("/bad%zzx"), "/bad%zzx");
    }

    #[test]
    fn basic_auth_encodes() {
        use super::super::http::base64;
        assert_eq!(base64(b"user:pass"), "dXNlcjpwYXNz");
        assert_eq!(base64(b"a"), "YQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(base64(b""), "");
    }

    #[test]
    fn listings_are_read_from_multistatus() {
        let xml = r#"<?xml version="1.0"?>
<D:multistatus xmlns:D="DAV:">
 <D:response><D:href>/dav/docs/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
 <D:response><D:href>/dav/docs/My%20File.txt</D:href><D:propstat><D:prop>
   <D:resourcetype/><D:getcontentlength>1234</D:getcontentlength>
   <D:getlastmodified>Wed, 09 Oct 2024 14:03:22 GMT</D:getlastmodified></D:prop></D:propstat></D:response>
 <D:response><D:href>http://host:8080/dav/docs/sub/</D:href><D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop></D:propstat></D:response>
</D:multistatus>"#;
        let items = parse_multistatus(xml);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].1, Entry { name: "My File.txt".into(), is_dir: false, size: 1234, modified: Some(1728482602) });
        assert_eq!(items[2].1.name, "sub");
        assert!(items[2].1.is_dir);
        assert_eq!(items[2].0, "/dav/docs/sub/");
    }

    #[test]
    fn different_namespace_prefixes_work() {
        let xml = r#"<multistatus xmlns="DAV:"><response><href>/x/a.bin</href><propstat><prop><getcontentlength>7</getcontentlength><resourcetype/></prop></propstat></response></multistatus>"#;
        let items = parse_multistatus(xml);
        assert_eq!(items[0].1.size, 7);
        assert_eq!(items[0].1.name, "a.bin");
    }

    #[test]
    fn only_whole_responses_are_cut_off() {
        let xml = b"<?xml?><d:multistatus><d:response><d:href>/a</d:href></d:response><d:response><d:href>/b";
        let upto = complete_upto(xml);
        assert!(xml[..upto].ends_with(b"</d:response>"));
        assert_eq!(complete_upto(b"<d:multistatus><d:response><d:href>/a"), 0);
        assert_eq!(complete_upto(b""), 0);
        assert!(complete_upto(b"<response></response>") > 0);
    }

    #[test]
    fn the_end_of_an_answer_is_recognised() {
        assert!(ended(b"</d:response></d:multistatus>\r\n"));
        assert!(ended(b"</multistatus>"));
        assert!(!ended(b"</d:response>"));
        assert!(!ended(b"<d:multistatus>"));
        assert!(!ended(b""));
    }

    /// A folder answer that arrives in pieces fills the list in pieces too.
    #[test]
    fn a_slow_answer_is_listed_as_it_arrives() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            // Take in the whole request, body included. Closing a socket that still has unread bytes makes Windows
            // reset the connection, which would cut the client's reading short.
            let mut seen = Vec::new();
            let mut b = [0u8; 1024];
            let head_end = loop {
                if let Some(at) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                    break at + 4;
                }
                let n = c.read(&mut b).unwrap();
                assert!(n > 0, "the client hung up before it finished asking");
                seen.extend_from_slice(&b[..n]);
            };
            let head = String::from_utf8_lossy(&seen[..head_end]).to_ascii_lowercase();
            let body_len = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
            while seen.len() < head_end + body_len {
                let n = c.read(&mut b).unwrap();
                assert!(n > 0, "the client hung up before it finished asking");
                seen.extend_from_slice(&b[..n]);
            }
            c.write_all(b"HTTP/1.1 207 Multi-Status\r\nConnection: close\r\n\r\n<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">").unwrap();
            let item = |p: &str, dir: bool| format!("<d:response><d:href>{p}</d:href><d:propstat><d:prop><d:resourcetype>{}</d:resourcetype><d:getcontentlength>5</d:getcontentlength></d:prop></d:propstat></d:response>", if dir { "<d:collection/>" } else { "" });
            for group in [["/f/", "/f/a.txt", "/f/b.txt"], ["/f/c.txt", "/f/d/", "/f/e.txt"]] {
                for p in group {
                    c.write_all(item(p, p.ends_with('/') && p != "/f/").as_bytes()).unwrap();
                }
                c.flush().unwrap();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            c.write_all(b"</d:multistatus>").unwrap();
            // Say we are done, then wait for the client to finish reading and hang up, so nothing is cut off.
            c.shutdown(std::net::Shutdown::Write).unwrap();
            c.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            while matches!(c.read(&mut b), Ok(n) if n > 0) {}
        });
        let mut dav = Dav { http: Client::new("127.0.0.1", port, false, "", "") };
        let mut batches: Vec<usize> = Vec::new();
        let all = dav.list_streaming("/f", &AtomicBool::new(false), &mut |b, _| batches.push(b.len())).unwrap();
        let names: Vec<_> = all.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "b.txt", "c.txt", "d", "e.txt"]);
        assert!(all[3].is_dir && !all[0].is_dir);
        // The first names were handed over before the rest had even been sent.
        assert!(batches.len() >= 2, "{batches:?}");
        assert!(batches[0] < 5);
        assert_eq!(batches.iter().sum::<usize>(), 5);
    }
}
