//! WebDAV over HTTP or HTTPS (Nextcloud, ownCloud, Synology, Apache, nginx,
//! NAS boxes). Logs in with Basic authentication, which is why plain HTTP is
//! flagged in the interface.

use super::http::Client;
use super::{fail, pump, Control, Entry, Server, Session};
use crate::error::{CoreError, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::fs::File;
use std::io::Read;
use std::path::Path;

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
        let file = File::open(local).map_err(|e| CoreError::io(local, e))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut counting = Counting { inner: file, ctl, total: 0, cancelled: false };
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
struct Counting<'a, 'b> {
    inner: File,
    ctl: &'a Control<'b>,
    total: u64,
    cancelled: bool,
}

impl Read for Counting<'_, '_> {
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
}
