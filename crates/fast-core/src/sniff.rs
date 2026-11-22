//! Telling apart files that share an extension but not a kind.
//!
//! `.ts` is a TypeScript source file, an MPEG transport stream (video from recorders, dashcams and TV capture) or a
//! Qt translation file; `.mod` is a camcorder video, a tracker music module or a Go module; `.key` is a Keynote
//! presentation or a private key; and so on. The name alone cannot say which, so the first bytes of the file are
//! looked at (about a kilobyte, only for the few extensions listed in `AMBIGUOUS`).

use std::collections::HashMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The extensions whose files are looked into.
pub const AMBIGUOUS: &[&str] = &["ts", "mts", "mod", "key", "pub", "db"];

/// How many bytes at the start of a file are enough to tell (the tracker signature sits at 1080).
pub const HEAD: usize = 1088;

/// Packets of 188 bytes, or of 192 with a time stamp in front (the same rule the "Opens with" choice uses).
fn transport_stream(head: &[u8]) -> bool {
    crate::openwith::looks_like_mpeg_ts(head)
}

fn text_start(head: &[u8]) -> &[u8] {
    let h = head.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(head);
    let skip = h.iter().take_while(|b| b.is_ascii_whitespace()).count();
    &h[skip..]
}

/// What the start of a file says it is, for a file with this (lower case) extension. `None` means "no reason to think
/// it is anything but what its name says".
///
/// The answers are names of looks the interface knows: `ts:video`, `ts:qt`, `mts:video`, `mod:video`, `mod:music`,
/// `key:private`, `pub:publisher`, `pub:public`, `db:thumbs`.
pub fn sniff_head(ext: &str, head: &[u8]) -> Option<&'static str> {
    const OLE: [u8; 4] = [0xD0, 0xCF, 0x11, 0xE0];
    match ext {
        "ts" if transport_stream(head) => Some("ts:video"),
        "ts" => {
            let t = text_start(head);
            (t.starts_with(b"<?xml") || t.starts_with(b"<!DOCTYPE TS")).then_some("ts:qt")
        }
        "mts" if transport_stream(head) => Some("mts:video"),
        "mod" if head.starts_with(&[0, 0, 1, 0xBA]) || head.starts_with(&[0, 0, 1, 0xB3]) => Some("mod:video"),
        "mod" => {
            let tag = head.get(1080..1084)?;
            let chan = |t: &[u8]| t.len() == 4 && t[0].is_ascii_digit() && &t[1..] == b"CHN";
            (matches!(tag, b"M.K." | b"M!K!" | b"FLT4" | b"FLT8" | b"4CHN") || chan(tag)).then_some("mod:music")
        }
        // A Keynote presentation is a zip; a key is text ("-----BEGIN …", "ssh-…") or an OpenSSH file.
        "key" => {
            let t = text_start(head);
            (t.starts_with(b"-----BEGIN") || t.starts_with(b"openssh-key") || t.starts_with(b"ssh-")).then_some("key:private")
        }
        // A Publisher file is an old Office file; a public key is text.
        "pub" if head.starts_with(&OLE) => Some("pub:publisher"),
        // Thumbs.db is an old Office file too; a database file starts with "SQLite format 3".
        "db" if head.starts_with(&OLE) => Some("db:thumbs"),
        _ => None,
    }
}

/// Answers already found: (path, length, modified) → answer. Emptied when it grows large.
static SEEN: Mutex<Option<HashMap<(PathBuf, u64, i64), Option<&'static str>>>> = Mutex::new(None);

/// As `sniff_head`, for a file on disk (cached while the file does not change). Fails when the file cannot be read.
pub fn sniff_path(path: &Path) -> io::Result<Option<&'static str>> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !AMBIGUOUS.contains(&ext.as_str()) {
        return Ok(None);
    }
    let meta = std::fs::metadata(path)?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0);
    let key = (path.to_path_buf(), meta.len(), mtime);
    if let Some(hit) = SEEN.lock().unwrap().as_ref().and_then(|m| m.get(&key)) {
        return Ok(*hit);
    }
    let mut head = Vec::with_capacity(HEAD);
    std::fs::File::open(path)?.take(HEAD as u64).read_to_end(&mut head)?;
    let found = sniff_head(&ext, &head);
    let mut seen = SEEN.lock().unwrap();
    let map = seen.get_or_insert_with(HashMap::new);
    if map.len() >= 8192 {
        map.clear();
    }
    map.insert(key, found);
    Ok(found)
}

/// Is this file a video, whatever its extension says?
pub fn is_video(path: &Path) -> bool {
    matches!(sniff_path(path), Ok(Some(kind)) if kind.ends_with(":video"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(step: usize, lead: usize, packets: usize) -> Vec<u8> {
        let mut v = vec![0u8; lead + step * packets];
        for i in 0..packets {
            v[lead + i * step] = 0x47;
        }
        v
    }

    #[test]
    fn transport_streams_are_video_by_either_packet_size() {
        assert_eq!(sniff_head("ts", &stream(188, 0, 6)), Some("ts:video"));
        assert_eq!(sniff_head("ts", &stream(192, 4, 6)), Some("ts:video"));
        assert_eq!(sniff_head("mts", &stream(192, 4, 6)), Some("mts:video"));
    }

    #[test]
    fn typescript_is_not_taken_for_video() {
        let src = b"import { x } from './x';\nexport const G = 1;\n".repeat(40);
        assert_eq!(sniff_head("ts", &src), None);
        assert_eq!(sniff_head("mts", &src), None);
        // Starting with a G, as some source does, is not enough.
        assert_eq!(sniff_head("ts", b"Get the thing\n".repeat(60).as_slice()), None);
        assert_eq!(sniff_head("ts", b""), None);
        assert_eq!(sniff_head("ts", &[0x47; 50]), None);
    }

    #[test]
    fn qt_translation_files_are_told_from_typescript() {
        assert_eq!(sniff_head("ts", b"<?xml version=\"1.0\"?><!DOCTYPE TS><TS version=\"2.1\">"), Some("ts:qt"));
        assert_eq!(sniff_head("ts", b"\xEF\xBB\xBF\n<!DOCTYPE TS>"), Some("ts:qt"));
    }

    #[test]
    fn mod_files_are_video_music_or_neither() {
        assert_eq!(sniff_head("mod", &[0, 0, 1, 0xBA, 0x44]), Some("mod:video"));
        let mut tracker = vec![0u8; 1100];
        tracker[1080..1084].copy_from_slice(b"M.K.");
        assert_eq!(sniff_head("mod", &tracker), Some("mod:music"));
        tracker[1080..1084].copy_from_slice(b"8CHN");
        assert_eq!(sniff_head("mod", &tracker), Some("mod:music"));
        assert_eq!(sniff_head("mod", b"module example.com/x\n\ngo 1.22\n"), None);
    }

    #[test]
    fn keys_and_old_office_files_are_told_apart() {
        assert_eq!(sniff_head("key", b"-----BEGIN OPENSSH PRIVATE KEY-----\n"), Some("key:private"));
        assert_eq!(sniff_head("key", b"PK\x03\x04rest of a zip"), None);
        assert_eq!(sniff_head("pub", &[0xD0, 0xCF, 0x11, 0xE0, 0, 0]), Some("pub:publisher"));
        assert_eq!(sniff_head("pub", b"ssh-ed25519 AAAA"), None);
        assert_eq!(sniff_head("db", &[0xD0, 0xCF, 0x11, 0xE0, 0, 0]), Some("db:thumbs"));
        assert_eq!(sniff_head("db", b"SQLite format 3\0"), None);
        assert_eq!(sniff_head("txt", &stream(188, 0, 4)), None);
    }

    #[test]
    fn a_file_on_disk_is_read_and_remembered() {
        let dir = std::env::temp_dir().join(format!("fast-sniff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.ts");
        std::fs::write(&video, stream(188, 0, 40)).unwrap();
        let code = dir.join("app.ts");
        std::fs::write(&code, "export const a = 1;\n".repeat(30)).unwrap();
        assert!(is_video(&video));
        assert!(is_video(&video));
        assert!(!is_video(&code));
        assert!(!is_video(&dir.join("missing.ts")));
        assert!(!is_video(&dir.join("x.mp4")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
