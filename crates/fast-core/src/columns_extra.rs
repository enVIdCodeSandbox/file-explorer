//! More Details columns, read from the file system or from the file's own contents: permissions
//! and owners, text encoding and line counts, checksums, document properties (Office, OpenDocument,
//! PDF), program details (Windows, Linux and macOS executables) and what a zip holds.
//!
//! Everything here is asked for by column id and read only when that column is shown, so a folder
//! pays for nothing it does not display. Values follow the conventions of `columns`: dates are
//! milliseconds and sizes are bytes, as text; an empty value means "nothing to say" and is left out.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub type Out = BTreeMap<String, String>;

/// Files larger than this are not hashed or searched through (it would stall the list).
const HASH_LIMIT: u64 = 64 * 1024 * 1024;
/// Largest text file whose lines and words are counted.
const COUNT_LIMIT: u64 = 8 * 1024 * 1024;
/// Largest file searched for program or PDF details.
const SCAN_LIMIT: u64 = 64 * 1024 * 1024;

pub const FS_KEYS: &[&str] = &[
    "changed", "inode", "links", "group", "ownerName", "groupName", "permsOctal", "permsText", "readOnly", "executable",
    "winSystem", "winArchive", "winCompressed", "winEncrypted", "winOffline", "winTemporary", "winSparse", "winReparse",
    "filesInside", "foldersInside", "hiddenInside",
];
pub const CONTENT_KEYS: &[&str] = &["detected", "textOrBinary", "encoding", "lineEndings", "lines", "words", "interpreter"];
pub const HASH_KEYS: &[&str] = &["sha256", "sha1", "md5", "crc32"];
pub const DOC_KEYS: &[&str] = &[
    "docTitle", "docAuthor", "docLastBy", "docSubject", "docKeywords", "docComments", "docCategory", "docCompany", "docApp",
    "docPages", "docWords", "docChars", "docSlides", "docCreated", "docSaved",
];
pub const EXE_KEYS: &[&str] = &["exeArch", "exeType", "exeSubsystem", "exeCompiled", "exeCompany", "exeDescription", "exeVersion", "exeProduct", "exeCopyright"];
pub const ZIP_KEYS: &[&str] = &["archiveItems", "archiveSize", "archiveRatio"];

fn put(out: &mut Out, k: &str, v: impl Into<String>) {
    let v = v.into();
    if !v.trim().is_empty() {
        out.insert(k.to_string(), v);
    }
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn yes(b: bool) -> &'static str {
    if b { "Yes" } else { "" }
}

/// Read up to `n` bytes from the start of a file.
fn head(path: &Path, n: usize) -> Vec<u8> {
    let Ok(f) = File::open(path) else { return Vec::new() };
    let mut buf = Vec::with_capacity(n.min(1 << 16));
    let _ = f.take(n as u64).read_to_end(&mut buf);
    buf
}

// ---- File system -------------------------------------------------------------------------

pub fn file_system(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    let Ok(lmeta) = fs::symlink_metadata(path) else { return };
    let meta = if lmeta.file_type().is_symlink() { fs::metadata(path).unwrap_or_else(|_| lmeta.clone()) } else { lmeta.clone() };
    let is_dir = meta.is_dir();

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if wants("changed") {
            put(out, "changed", (meta.ctime() * 1000 + meta.ctime_nsec() / 1_000_000).to_string());
        }
        if wants("inode") {
            put(out, "inode", meta.ino().to_string());
        }
        if wants("links") {
            put(out, "links", meta.nlink().to_string());
        }
        if wants("group") {
            put(out, "group", meta.gid().to_string());
        }
        if wants("ownerName") {
            put(out, "ownerName", unix_name(meta.uid(), true).unwrap_or_else(|| meta.uid().to_string()));
        }
        if wants("groupName") {
            put(out, "groupName", unix_name(meta.gid(), false).unwrap_or_else(|| meta.gid().to_string()));
        }
        let mode = meta.mode();
        if wants("permsOctal") {
            put(out, "permsOctal", format!("{:03o}", mode & 0o7777));
        }
        if wants("permsText") {
            put(out, "permsText", mode_text(mode, is_dir, lmeta.file_type().is_symlink()));
        }
        if wants("executable") && !is_dir {
            put(out, "executable", yes(mode & 0o111 != 0));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        let a = meta.file_attributes();
        let flag = |bit: u32| yes(a & bit != 0);
        if wants("winSystem") {
            put(out, "winSystem", flag(0x4));
        }
        if wants("winArchive") {
            put(out, "winArchive", flag(0x20));
        }
        if wants("winTemporary") {
            put(out, "winTemporary", flag(0x100));
        }
        if wants("winSparse") {
            put(out, "winSparse", flag(0x200));
        }
        if wants("winReparse") {
            put(out, "winReparse", flag(0x400));
        }
        if wants("winCompressed") {
            put(out, "winCompressed", flag(0x800));
        }
        if wants("winOffline") {
            put(out, "winOffline", flag(0x1000));
        }
        if wants("winEncrypted") {
            put(out, "winEncrypted", flag(0x4000));
        }
        if wants("executable") && !is_dir {
            put(out, "executable", yes(matches!(ext_of(path).as_str(), "exe" | "bat" | "cmd" | "com" | "msi" | "ps1" | "scr")));
        }
    }
    if wants("readOnly") && !is_dir {
        put(out, "readOnly", yes(meta.permissions().readonly()));
    }
    if is_dir && (wants("filesInside") || wants("foldersInside") || wants("hiddenInside")) {
        let (mut files, mut folders, mut hidden) = (0u64, 0u64, 0u64);
        if let Ok(rd) = fs::read_dir(path) {
            for e in rd.flatten().take(200_000) {
                let ft = e.file_type();
                if ft.map(|t| t.is_dir()).unwrap_or(false) {
                    folders += 1;
                } else {
                    files += 1;
                }
                if e.file_name().to_string_lossy().starts_with('.') {
                    hidden += 1;
                }
            }
        }
        if wants("filesInside") {
            put(out, "filesInside", files.to_string());
        }
        if wants("foldersInside") {
            put(out, "foldersInside", folders.to_string());
        }
        if wants("hiddenInside") {
            put(out, "hiddenInside", hidden.to_string());
        }
    }
}

/// `-rwxr-xr-x`, with the type letter in front.
pub fn mode_text(mode: u32, is_dir: bool, is_link: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_link { 'l' } else if is_dir { 'd' } else { '-' });
    let bit = |m: u32, c: char| if mode & m != 0 { c } else { '-' };
    let special = |m: u32, x: u32, on: char, off: char| match (mode & m != 0, mode & x != 0) {
        (true, true) => on,
        (true, false) => off,
        (false, true) => 'x',
        (false, false) => '-',
    };
    s.extend([bit(0o400, 'r'), bit(0o200, 'w'), special(0o4000, 0o100, 's', 'S')]);
    s.extend([bit(0o040, 'r'), bit(0o020, 'w'), special(0o2000, 0o010, 's', 'S')]);
    s.extend([bit(0o004, 'r'), bit(0o002, 'w'), special(0o1000, 0o001, 't', 'T')]);
    s
}

#[cfg(unix)]
fn unix_name(id: u32, user: bool) -> Option<String> {
    use std::ffi::CStr;
    let mut buf = vec![0u8; 4096];
    // SAFETY: the buffer outlives the call and the returned strings are copied before it ends.
    unsafe {
        if user {
            let mut pwd: libc::passwd = std::mem::zeroed();
            let mut res: *mut libc::passwd = std::ptr::null_mut();
            if libc::getpwuid_r(id, &mut pwd, buf.as_mut_ptr() as *mut libc::c_char, buf.len(), &mut res) != 0 || res.is_null() {
                return None;
            }
            Some(CStr::from_ptr(pwd.pw_name).to_string_lossy().into_owned())
        } else {
            let mut grp: libc::group = std::mem::zeroed();
            let mut res: *mut libc::group = std::ptr::null_mut();
            if libc::getgrgid_r(id, &mut grp, buf.as_mut_ptr() as *mut libc::c_char, buf.len(), &mut res) != 0 || res.is_null() {
                return None;
            }
            Some(CStr::from_ptr(grp.gr_name).to_string_lossy().into_owned())
        }
    }
}

// ---- Content -----------------------------------------------------------------------------

/// What a file is, going by its first bytes rather than its name.
pub fn detect(head: &[u8]) -> Option<&'static str> {
    let starts = |m: &[u8]| head.starts_with(m);
    const MAGIC: &[(&[u8], &str)] = &[
        (b"\x89PNG\r\n\x1a\n", "PNG picture"),
        (b"\xFF\xD8\xFF", "JPEG picture"),
        (b"GIF87a", "GIF picture"),
        (b"GIF89a", "GIF picture"),
        (b"BM", "BMP picture"),
        (b"II*\0", "TIFF picture"),
        (b"MM\0*", "TIFF picture"),
        (b"%PDF-", "PDF document"),
        (b"PK\x03\x04", "Zip archive"),
        (b"PK\x05\x06", "Zip archive (empty)"),
        (b"\x1F\x8B", "Gzip compressed"),
        (b"BZh", "Bzip2 compressed"),
        (b"\xFD7zXZ\0", "XZ compressed"),
        (b"7z\xBC\xAF\x27\x1C", "7-Zip archive"),
        (b"Rar!\x1A\x07", "RAR archive"),
        (b"\x28\xB5\x2F\xFD", "Zstandard compressed"),
        (b"\x7FELF", "ELF program or library"),
        (b"SQLite format 3\0", "SQLite database"),
        (b"fLaC", "FLAC audio"),
        (b"OggS", "Ogg media"),
        (b"ID3", "MP3 audio"),
        (b"\x1A\x45\xDF\xA3", "Matroska or WebM video"),
        (b"\0asm", "WebAssembly module"),
        (b"{\\rtf", "Rich Text document"),
        (b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1", "Old Office document (OLE)"),
        (b"MZ", "Windows program or library"),
        (b"\xFE\xED\xFA\xCE", "macOS program or library"),
        (b"\xFE\xED\xFA\xCF", "macOS program or library"),
        (b"\xCE\xFA\xED\xFE", "macOS program or library"),
        (b"\xCF\xFA\xED\xFE", "macOS program or library"),
        (b"%!PS", "PostScript document"),
        (b"wOFF", "Web font (WOFF)"),
        (b"wOF2", "Web font (WOFF2)"),
        (b"\0\x01\0\0\0", "TrueType font"),
        (b"OTTO", "OpenType font"),
        (b"\0\0\x01\0", "Windows icon"),
    ];
    if starts(b"\xCA\xFE\xBA\xBE") {
        // The same bytes begin Java classes and macOS universal binaries; the next number tells them apart.
        let n = u32::from_be_bytes(head.get(4..8)?.try_into().ok()?);
        return Some(if n < 40 { "macOS universal program" } else { "Java class" });
    }
    if starts(b"RIFF") && head.len() >= 12 {
        return Some(match &head[8..12] {
            b"WAVE" => "WAV audio",
            b"AVI " => "AVI video",
            b"WEBP" => "WebP picture",
            _ => "RIFF media",
        });
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        return Some(match &head[8..12] {
            b"heic" | b"heix" | b"mif1" | b"hevc" => "HEIC picture",
            b"avif" => "AVIF picture",
            b"M4A " | b"M4B " => "M4A audio",
            b"qt  " => "QuickTime video",
            _ => "MP4 video",
        });
    }
    if head.len() >= 262 && &head[257..262] == b"ustar" {
        return Some("Tar archive");
    }
    if starts(b"\xFF\xFB") || starts(b"\xFF\xF3") || starts(b"\xFF\xF2") {
        return Some("MP3 audio");
    }
    for (m, name) in MAGIC {
        if starts(m) {
            return Some(name);
        }
    }
    // Text: look at what it says.
    if is_text(head) {
        let start = String::from_utf8_lossy(&head[..head.len().min(512)]).trim_start_matches('\u{feff}').trim_start().to_lowercase();
        return Some(if start.starts_with("#!") {
            "Script"
        } else if start.starts_with("<!doctype html") || start.starts_with("<html") {
            "HTML document"
        } else if start.starts_with("<svg") || (start.starts_with("<?xml") && start.contains("<svg")) {
            "SVG picture"
        } else if start.starts_with("<?xml") {
            "XML document"
        } else {
            "Plain text"
        });
    }
    if head.is_empty() { None } else { Some("Binary data") }
}

/// Text in UTF-8, ASCII or an 8-bit code page: no zero bytes, and not cut in the middle of a character by the end of the sample.
pub fn is_text(head: &[u8]) -> bool {
    if head.is_empty() || head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(e) => e.valid_up_to() + 4 > head.len() || head.iter().filter(|b| **b < 0x09 || (**b > 0x0D && **b < 0x20)).count() * 50 < head.len(),
    }
}

pub fn encoding(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(b"\xFF\xFE") {
        return Some("UTF-16 (little-endian)");
    }
    if head.starts_with(b"\xFE\xFF") {
        return Some("UTF-16 (big-endian)");
    }
    if head.starts_with(b"\xEF\xBB\xBF") {
        return Some("UTF-8 with BOM");
    }
    if !is_text(head) {
        return None;
    }
    if head.is_ascii() {
        Some("ASCII")
    } else if std::str::from_utf8(head).is_ok() || std::str::from_utf8(head).err().is_some_and(|e| e.valid_up_to() + 4 > head.len()) {
        Some("UTF-8")
    } else {
        Some("8-bit (ANSI)")
    }
}

pub fn line_endings(data: &[u8]) -> Option<&'static str> {
    let (mut crlf, mut lf, mut cr) = (0usize, 0usize, 0usize);
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'\r' if data.get(i + 1) == Some(&b'\n') => {
                crlf += 1;
                i += 1;
            }
            b'\r' => cr += 1,
            b'\n' => lf += 1,
            _ => {}
        }
        i += 1;
    }
    match (crlf > 0, lf > 0, cr > 0) {
        (false, false, false) => None,
        (true, false, false) => Some("Windows (CRLF)"),
        (false, true, false) => Some("Unix (LF)"),
        (false, false, true) => Some("Old Mac (CR)"),
        _ => Some("Mixed"),
    }
}

pub fn content(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    let Ok(meta) = fs::metadata(path) else { return };
    if !meta.is_file() {
        return;
    }
    let sample = head(path, 64 * 1024);
    if wants("detected") {
        if let Some(d) = detect(&sample) {
            put(out, "detected", d);
        }
    }
    let text = is_text(&sample) || encoding(&sample).is_some_and(|e| e.starts_with("UTF-8"));
    if wants("textOrBinary") && !sample.is_empty() {
        put(out, "textOrBinary", if text { "Text" } else { "Binary" });
    }
    if wants("encoding") {
        if let Some(e) = encoding(&sample) {
            put(out, "encoding", e);
        }
    }
    if wants("interpreter") && sample.starts_with(b"#!") {
        let line = String::from_utf8_lossy(&sample[2..]).lines().next().unwrap_or("").trim().to_string();
        let mut parts = line.split_whitespace();
        let first = parts.next().unwrap_or("");
        let prog = if first.ends_with("/env") || first == "env" { parts.find(|p| !p.starts_with('-')).unwrap_or("") } else { first };
        put(out, "interpreter", prog.rsplit('/').next().unwrap_or(prog));
    }
    if text && (wants("lineEndings") || wants("lines") || wants("words")) {
        // The sample is enough for the line ending style; counting needs the whole file.
        let whole;
        let data: &[u8] = if meta.len() <= COUNT_LIMIT && (wants("lines") || wants("words")) {
            whole = fs::read(path).unwrap_or_default();
            &whole
        } else {
            &sample
        };
        if wants("lineEndings") {
            if let Some(l) = line_endings(&sample) {
                put(out, "lineEndings", l);
            }
        }
        if meta.len() <= COUNT_LIMIT && !data.is_empty() {
            if wants("lines") {
                let n = data.iter().filter(|b| **b == b'\n').count() + usize::from(data.last() != Some(&b'\n'));
                put(out, "lines", n.to_string());
            }
            if wants("words") {
                put(out, "words", String::from_utf8_lossy(data).split_whitespace().count().to_string());
            }
        }
    }
}

// ---- Checksums ---------------------------------------------------------------------------

pub fn hashes(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    use sha2::Digest;
    let Ok(meta) = fs::metadata(path) else { return };
    if !meta.is_file() || meta.len() > HASH_LIMIT {
        return;
    }
    let Ok(mut f) = File::open(path) else { return };
    let mut sha256 = wants("sha256").then(sha2::Sha256::new);
    let mut sha1 = wants("sha1").then(sha1::Sha1::new);
    let mut md5 = wants("md5").then(md5::Md5::new);
    let mut crc = wants("crc32").then(crc32fast::Hasher::new);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return,
        };
        if let Some(h) = sha256.as_mut() {
            h.update(&buf[..n]);
        }
        if let Some(h) = sha1.as_mut() {
            h.update(&buf[..n]);
        }
        if let Some(h) = md5.as_mut() {
            h.update(&buf[..n]);
        }
        if let Some(h) = crc.as_mut() {
            h.update(&buf[..n]);
        }
    }
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    if let Some(h) = sha256 {
        put(out, "sha256", hex(&h.finalize()));
    }
    if let Some(h) = sha1 {
        put(out, "sha1", hex(&h.finalize()));
    }
    if let Some(h) = md5 {
        put(out, "md5", hex(&h.finalize()));
    }
    if let Some(h) = crc {
        put(out, "crc32", format!("{:08x}", h.finalize()));
    }
}

// ---- Documents ---------------------------------------------------------------------------

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// The text of the first `<tag>…</tag>` (the tag may carry attributes).
fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let mut from = 0;
    while let Some(i) = xml[from..].find(&open) {
        let at = from + i + open.len();
        let next = xml[at..].chars().next()?;
        if next == '>' || next.is_whitespace() || next == '/' {
            let gt = xml[at..].find('>')? + at;
            if xml[..gt].ends_with('/') {
                return None;
            }
            let end = xml[gt + 1..].find(&format!("</{tag}>"))? + gt + 1;
            let v = unescape(xml[gt + 1..end].trim());
            return (!v.is_empty()).then_some(v);
        }
        from = at;
    }
    None
}

/// The value of one attribute of the first `<tag …>`.
fn xml_attr(xml: &str, tag: &str, attr: &str) -> Option<String> {
    let i = xml.find(&format!("<{tag}"))?;
    let gt = xml[i..].find('>')? + i;
    let el = &xml[i..gt];
    let a = el.find(&format!("{attr}=\""))? + attr.len() + 2;
    let end = el[a..].find('"')? + a;
    Some(unescape(&el[a..end]))
}

fn iso_ms(s: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.timestamp_millis().to_string())
}

fn zip_entry(zip: &mut zip::ZipArchive<File>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    if f.size() > 4 * 1024 * 1024 {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

pub fn documents(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    let ext = ext_of(path);
    match ext.as_str() {
        "docx" | "xlsx" | "pptx" | "docm" | "xlsm" | "pptm" | "dotx" | "xltx" | "potx" | "ppsx" | "vsdx" => {
            let Ok(file) = File::open(path) else { return };
            let Ok(mut zip) = zip::ZipArchive::new(file) else { return };
            if let Some(core) = zip_entry(&mut zip, "docProps/core.xml") {
                let get = |t: &str| xml_text(&core, t);
                for (key, tag) in [
                    ("docTitle", "dc:title"), ("docAuthor", "dc:creator"), ("docLastBy", "cp:lastModifiedBy"), ("docSubject", "dc:subject"),
                    ("docKeywords", "cp:keywords"), ("docComments", "dc:description"), ("docCategory", "cp:category"),
                ] {
                    if wants(key) {
                        put(out, key, get(tag).unwrap_or_default());
                    }
                }
                if wants("docCreated") {
                    put(out, "docCreated", get("dcterms:created").and_then(|d| iso_ms(&d)).unwrap_or_default());
                }
                if wants("docSaved") {
                    put(out, "docSaved", get("dcterms:modified").and_then(|d| iso_ms(&d)).unwrap_or_default());
                }
            }
            if ["docCompany", "docApp", "docPages", "docWords", "docChars", "docSlides"].iter().any(|k| wants(k)) {
                if let Some(app) = zip_entry(&mut zip, "docProps/app.xml") {
                    for (key, tag) in [("docCompany", "Company"), ("docApp", "Application"), ("docPages", "Pages"), ("docWords", "Words"), ("docChars", "Characters"), ("docSlides", "Slides")] {
                        if wants(key) {
                            put(out, key, xml_text(&app, tag).filter(|v| key != "docPages" && key != "docWords" && key != "docChars" && key != "docSlides" || v.parse::<u64>().is_ok_and(|n| n > 0)).unwrap_or_default());
                        }
                    }
                }
            }
        }
        "odt" | "ods" | "odp" | "odg" | "ott" | "ots" | "otp" => {
            let Ok(file) = File::open(path) else { return };
            let Ok(mut zip) = zip::ZipArchive::new(file) else { return };
            let Some(meta) = zip_entry(&mut zip, "meta.xml") else { return };
            for (key, tag) in [
                ("docTitle", "dc:title"), ("docAuthor", "meta:initial-creator"), ("docLastBy", "dc:creator"), ("docSubject", "dc:subject"),
                ("docKeywords", "meta:keyword"), ("docComments", "dc:description"), ("docApp", "meta:generator"),
            ] {
                if wants(key) {
                    put(out, key, xml_text(&meta, tag).unwrap_or_default());
                }
            }
            if wants("docCreated") {
                put(out, "docCreated", xml_text(&meta, "meta:creation-date").and_then(|d| iso_ms(&format!("{d}Z").replace("ZZ", "Z"))).unwrap_or_default());
            }
            if wants("docSaved") {
                put(out, "docSaved", xml_text(&meta, "dc:date").and_then(|d| iso_ms(&format!("{d}Z").replace("ZZ", "Z"))).unwrap_or_default());
            }
            for (key, attr) in [("docPages", "meta:page-count"), ("docWords", "meta:word-count"), ("docChars", "meta:character-count"), ("docSlides", "meta:page-count")] {
                if wants(key) && (key != "docSlides" || ext.starts_with("odp") || ext == "otp") && (key != "docPages" || !ext.starts_with("odp")) {
                    put(out, key, xml_attr(&meta, "meta:document-statistic", attr).filter(|v| v != "0").unwrap_or_default());
                }
            }
        }
        "pdf" => pdf(path, wants, out),
        _ => {}
    }
}

/// A PDF string after a key: `(text)` with escapes, or `<hex>`, either of them possibly UTF-16.
fn pdf_string(data: &[u8], key: &[u8]) -> Option<String> {
    let at = data.windows(key.len()).position(|w| w == key)? + key.len();
    let mut i = at;
    while data.get(i).is_some_and(|b| b.is_ascii_whitespace()) {
        i += 1;
    }
    let mut raw = Vec::new();
    match *data.get(i)? {
        b'(' => {
            i += 1;
            let mut depth = 1;
            while i < data.len() {
                let b = data[i];
                match b {
                    b'\\' => {
                        i += 1;
                        match *data.get(i)? {
                            b'n' => raw.push(b'\n'),
                            b'r' => raw.push(b'\r'),
                            b't' => raw.push(b'\t'),
                            d @ b'0'..=b'7' => {
                                let mut v = u32::from(d - b'0');
                                for _ in 0..2 {
                                    match data.get(i + 1) {
                                        Some(n @ b'0'..=b'7') => {
                                            v = v * 8 + u32::from(n - b'0');
                                            i += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                raw.push(v as u8);
                            }
                            other => raw.push(other),
                        }
                    }
                    b'(' => {
                        depth += 1;
                        raw.push(b);
                    }
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        raw.push(b);
                    }
                    _ => raw.push(b),
                }
                i += 1;
                if raw.len() > 4096 {
                    break;
                }
            }
        }
        b'<' if data.get(i + 1) != Some(&b'<') => {
            let end = data[i..].iter().position(|b| *b == b'>')? + i;
            let hex: Vec<u8> = data[i + 1..end].iter().copied().filter(|b| b.is_ascii_hexdigit()).collect();
            for pair in hex.chunks(2) {
                let s = std::str::from_utf8(pair).ok()?;
                raw.push(u8::from_str_radix(s, 16).ok().or_else(|| u8::from_str_radix(&format!("{s}0"), 16).ok())?);
            }
        }
        _ => return None,
    }
    let text = if raw.starts_with(b"\xFE\xFF") {
        let units: Vec<u16> = raw[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else if raw.starts_with(b"\xEF\xBB\xBF") {
        String::from_utf8_lossy(&raw[3..]).into_owned()
    } else {
        raw.iter().map(|b| *b as char).collect()
    };
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// "D:20240509140322Z" or "D:20240509140322+02'00'" to milliseconds.
fn pdf_date(s: &str) -> Option<String> {
    let d = s.trim().strip_prefix("D:").unwrap_or(s.trim());
    let digits: String = d.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.len() < 8 {
        return None;
    }
    let num = |a: usize, b: usize, default: u32| digits.get(a..b).and_then(|x| x.parse::<u32>().ok()).unwrap_or(default);
    let date = chrono::NaiveDate::from_ymd_opt(num(0, 4, 1970) as i32, num(4, 6, 1), num(6, 8, 1))?;
    let dt = date.and_hms_opt(num(8, 10, 0), num(10, 12, 0), num(12, 14, 0))?;
    // A zone like +02'00' moves the clock; without one the time is taken as UTC.
    let rest = &d[digits.len()..];
    let offset = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let z: Vec<u32> = rest[1..].split(|c: char| !c.is_ascii_digit()).filter(|p| !p.is_empty()).filter_map(|p| p.parse().ok()).collect();
            let secs = (z.first().copied().unwrap_or(0) * 3600 + z.get(1).copied().unwrap_or(0) * 60) as i64;
            if sign == '+' { secs } else { -secs }
        }
        _ => 0,
    };
    Some(((dt.and_utc().timestamp() - offset) * 1000).to_string())
}

fn pdf(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    let Ok(meta) = fs::metadata(path) else { return };
    if meta.len() > SCAN_LIMIT {
        return;
    }
    let Ok(data) = fs::read(path) else { return };
    if !data.starts_with(b"%PDF") {
        return;
    }
    for (key, name) in [("docTitle", "/Title"), ("docAuthor", "/Author"), ("docSubject", "/Subject"), ("docKeywords", "/Keywords")] {
        if wants(key) {
            put(out, key, pdf_string(&data, name.as_bytes()).unwrap_or_default());
        }
    }
    if wants("docApp") {
        put(out, "docApp", pdf_string(&data, b"/Creator").or_else(|| pdf_string(&data, b"/Producer")).unwrap_or_default());
    }
    if wants("docCreated") {
        put(out, "docCreated", pdf_string(&data, b"/CreationDate").and_then(|d| pdf_date(&d)).unwrap_or_default());
    }
    if wants("docSaved") {
        put(out, "docSaved", pdf_string(&data, b"/ModDate").and_then(|d| pdf_date(&d)).unwrap_or_default());
    }
    if wants("docPages") {
        // Page objects, when they are stored in the open; else the page count the page tree itself gives.
        let mut pages = 0u64;
        let mut max_count = 0u64;
        let mut i = 0;
        while let Some(p) = memchr::memchr(b'/', &data[i..]) {
            let at = i + p;
            let rest = &data[at..];
            if rest.starts_with(b"/Type") {
                let mut j = 5;
                while rest.get(j).is_some_and(|b| b.is_ascii_whitespace()) {
                    j += 1;
                }
                if rest[j..].starts_with(b"/Page") && !rest.get(j + 5).is_some_and(|b| b.is_ascii_alphanumeric()) {
                    pages += 1;
                }
            } else if rest.starts_with(b"/Count") {
                let mut j = 6;
                while rest.get(j).is_some_and(|b| b.is_ascii_whitespace()) {
                    j += 1;
                }
                let digits: String = rest[j..].iter().take_while(|b| b.is_ascii_digit()).map(|b| *b as char).collect();
                max_count = max_count.max(digits.parse().unwrap_or(0));
            }
            i = at + 1;
        }
        let n = if pages > 0 { pages } else { max_count };
        if n > 0 {
            put(out, "docPages", n.to_string());
        }
    }
}

// ---- Archives ----------------------------------------------------------------------------

pub fn archive(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    if !matches!(ext_of(path).as_str(), "zip" | "jar" | "apk" | "aab" | "epub" | "whl" | "nupkg" | "vsix" | "xpi" | "war" | "ear" | "ipa" | "kmz") {
        return;
    }
    let Ok(file) = File::open(path) else { return };
    let Ok(mut zip) = zip::ZipArchive::new(file) else { return };
    let (mut items, mut unpacked, mut packed) = (0u64, 0u64, 0u64);
    for i in 0..zip.len().min(500_000) {
        let Ok(e) = zip.by_index_raw(i) else { continue };
        if e.is_dir() {
            continue;
        }
        items += 1;
        unpacked += e.size();
        packed += e.compressed_size();
    }
    if wants("archiveItems") {
        put(out, "archiveItems", items.to_string());
    }
    if wants("archiveSize") && unpacked > 0 {
        put(out, "archiveSize", unpacked.to_string());
    }
    if wants("archiveRatio") && unpacked > 0 {
        put(out, "archiveRatio", ((packed as f64 / unpacked as f64) * 100.0).round().to_string());
    }
}

// ---- Programs ----------------------------------------------------------------------------

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn u16be(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32be(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The text a Windows program stores under a version key, which is in UTF-16: key, a zero, padding, then the value.
fn pe_string(data: &[u8], key: &str) -> Option<String> {
    let mut pat: Vec<u8> = key.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    pat.extend([0, 0]);
    let at = data.windows(pat.len()).position(|w| w == pat.as_slice())? + pat.len();
    let mut i = at;
    while data.get(i..i + 2) == Some(&[0, 0]) {
        i += 2;
    }
    let mut units = Vec::new();
    while let Some(c) = data.get(i..i + 2) {
        let u = u16::from_le_bytes([c[0], c[1]]);
        if u == 0 || units.len() > 260 {
            break;
        }
        units.push(u);
        i += 2;
    }
    let s = String::from_utf16_lossy(&units).trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn programs(path: &Path, wants: &dyn Fn(&str) -> bool, out: &mut Out) {
    let Ok(meta) = fs::metadata(path) else { return };
    if !meta.is_file() {
        return;
    }
    let h = head(path, 64 * 1024);
    let ext = ext_of(path);
    if h.starts_with(b"MZ") {
        let Some(pe) = u32le(&h, 0x3C).map(|x| x as usize) else { return };
        if h.get(pe..pe + 4) != Some(b"PE\0\0") {
            put(out, "exeType", "DOS program");
            return;
        }
        let machine = u16le(&h, pe + 4).unwrap_or(0);
        let stamp = u32le(&h, pe + 8).unwrap_or(0);
        let chars = u16le(&h, pe + 22).unwrap_or(0);
        let subsystem = u16le(&h, pe + 24 + 68).unwrap_or(0);
        if wants("exeArch") {
            put(
                out,
                "exeArch",
                match machine {
                    0x14c => "x86 (32-bit)",
                    0x8664 => "x64 (64-bit)",
                    0xAA64 => "ARM64",
                    0x1c0 | 0x1c4 => "ARM (32-bit)",
                    0x200 => "Itanium",
                    0x5064 => "RISC-V",
                    _ => "",
                },
            );
        }
        if wants("exeType") {
            put(out, "exeType", if chars & 0x2000 != 0 { "Windows library (DLL)" } else if subsystem == 1 { "Windows driver" } else { "Windows program" });
        }
        if wants("exeSubsystem") {
            put(out, "exeSubsystem", match subsystem { 1 => "Native", 2 => "Windows (window)", 3 => "Console", 9 => "Windows CE", 10..=14 => "EFI", _ => "" });
        }
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if wants("exeCompiled") && stamp > 631_152_000 && u64::from(stamp) <= now + 86_400 {
            put(out, "exeCompiled", (i64::from(stamp) * 1000).to_string());
        }
        let strings = [("exeCompany", "CompanyName"), ("exeDescription", "FileDescription"), ("exeVersion", "FileVersion"), ("exeProduct", "ProductName"), ("exeCopyright", "LegalCopyright")];
        if strings.iter().any(|(k, _)| wants(k)) && meta.len() <= SCAN_LIMIT {
            if let Ok(data) = fs::read(path) {
                for (key, name) in strings {
                    if wants(key) {
                        put(out, key, pe_string(&data, name).unwrap_or_default());
                    }
                }
            }
        }
    } else if h.starts_with(b"\x7FELF") {
        let big = h.get(5) == Some(&2);
        let rd16 = |at| if big { u16be(&h, at) } else { u16le(&h, at) };
        if wants("exeArch") {
            let bits = if h.get(4) == Some(&2) { "64-bit" } else { "32-bit" };
            let m = match rd16(18).unwrap_or(0) {
                0x3E => "x64",
                0x03 => "x86",
                0xB7 => "ARM64",
                0x28 => "ARM",
                0xF3 => "RISC-V",
                0x15 => "PowerPC (64-bit)",
                0x14 => "PowerPC",
                0x08 => "MIPS",
                0x16 => "IBM Z",
                _ => "",
            };
            if !m.is_empty() {
                put(out, "exeArch", if m.contains("bit") { m.to_string() } else { format!("{m} ({bits})") });
            }
        }
        if wants("exeType") {
            put(
                out,
                "exeType",
                match rd16(16).unwrap_or(0) {
                    1 => "ELF object file",
                    2 => "Linux program (ELF)",
                    3 if ext == "so" || path.to_string_lossy().contains(".so.") => "Linux shared library",
                    3 => "Linux program (ELF)",
                    4 => "Core dump",
                    _ => "",
                },
            );
        }
    } else if matches!(h.get(..4), Some(b"\xFE\xED\xFA\xCE" | b"\xFE\xED\xFA\xCF" | b"\xCE\xFA\xED\xFE" | b"\xCF\xFA\xED\xFE")) {
        let little = h[0] == 0xCE || h[0] == 0xCF;
        let rd32 = |at| if little { u32le(&h, at) } else { u32be(&h, at) };
        if wants("exeArch") {
            put(
                out,
                "exeArch",
                match rd32(4).unwrap_or(0) {
                    0x7 => "x86 (32-bit)",
                    0x0100_0007 => "x64 (64-bit)",
                    0x0100_000C => "ARM64",
                    0xC => "ARM (32-bit)",
                    _ => "",
                },
            );
        }
        if wants("exeType") {
            put(out, "exeType", match rd32(12).unwrap_or(0) { 2 => "macOS program", 6 => "macOS library", 8 => "macOS bundle", _ => "" });
        }
    } else if h.starts_with(b"\xCA\xFE\xBA\xBE") && u32be(&h, 4).is_some_and(|n| n < 40) {
        put(out, "exeType", "macOS universal program");
        put(out, "exeArch", "Several (universal)");
    } else if h.starts_with(b"#!") && wants("exeType") {
        put(out, "exeType", "Script");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn keys(k: &[&str]) -> Box<dyn Fn(&str) -> bool> {
        let k: Vec<String> = k.iter().map(|s| s.to_string()).collect();
        Box::new(move |x: &str| k.iter().any(|y| y == x))
    }

    fn run(f: fn(&Path, &dyn Fn(&str) -> bool, &mut Out), path: &Path, k: &[&str]) -> Out {
        let mut out = Out::new();
        f(path, &*keys(k), &mut out);
        out
    }

    #[test]
    fn modes_read_like_ls() {
        assert_eq!(mode_text(0o100644, false, false), "-rw-r--r--");
        assert_eq!(mode_text(0o40755, true, false), "drwxr-xr-x");
        assert_eq!(mode_text(0o104755, false, false), "-rwsr-xr-x");
        assert_eq!(mode_text(0o101777, false, false), "-rwxrwxrwt");
    }

    #[cfg(unix)]
    #[test]
    fn permissions_owner_and_folder_counts() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("run.sh");
        fs::write(&f, "#!/usr/bin/env python3\nprint(1)\n").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o750)).unwrap();
        let v = run(file_system, &f, &["permsOctal", "permsText", "executable", "ownerName", "groupName", "inode", "links", "changed", "readOnly"]);
        assert_eq!(v["permsOctal"], "750");
        assert_eq!(v["permsText"], "-rwxr-x---");
        assert_eq!(v["executable"], "Yes");
        assert!(!v["ownerName"].is_empty() && !v["groupName"].is_empty());
        assert_eq!(v["links"], "1");
        assert!(v["inode"].parse::<u64>().is_ok() && v["changed"].parse::<i64>().is_ok());
        assert!(!v.contains_key("readOnly"));
        fs::create_dir(d.path().join("sub")).unwrap();
        fs::write(d.path().join(".hid"), "x").unwrap();
        let c = run(file_system, d.path(), &["filesInside", "foldersInside", "hiddenInside"]);
        assert_eq!((c["filesInside"].as_str(), c["foldersInside"].as_str(), c["hiddenInside"].as_str()), ("2", "1", "1"));
    }

    #[test]
    fn content_is_recognised_by_its_bytes() {
        assert_eq!(detect(b"\x89PNG\r\n\x1a\nxxxx"), Some("PNG picture"));
        assert_eq!(detect(b"%PDF-1.7\n"), Some("PDF document"));
        assert_eq!(detect(b"RIFF\0\0\0\0WAVEfmt "), Some("WAV audio"));
        assert_eq!(detect(b"\0\0\0\x18ftypheic\0\0\0\0"), Some("HEIC picture"));
        assert_eq!(detect(b"\xCA\xFE\xBA\xBE\0\0\0\x34"), Some("Java class"));
        assert_eq!(detect(b"\xCA\xFE\xBA\xBE\0\0\0\x02"), Some("macOS universal program"));
        assert_eq!(detect(b"<!DOCTYPE html><html>"), Some("HTML document"));
        assert_eq!(detect(b"<?xml version='1.0'?><svg/>"), Some("SVG picture"));
        assert_eq!(detect(b"hello world\n"), Some("Plain text"));
        assert_eq!(detect(b"\x01\x02\0\x03"), Some("Binary data"));
        assert_eq!(detect(b""), None);
    }

    #[test]
    fn text_facts() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.txt");
        fs::write(&f, "one two\r\nthree\r\nfour").unwrap();
        let v = run(content, &f, &["detected", "textOrBinary", "encoding", "lineEndings", "lines", "words"]);
        assert_eq!(v["detected"], "Plain text");
        assert_eq!(v["textOrBinary"], "Text");
        assert_eq!(v["encoding"], "ASCII");
        assert_eq!(v["lineEndings"], "Windows (CRLF)");
        assert_eq!(v["lines"], "3");
        assert_eq!(v["words"], "4");
        let s = d.path().join("run");
        fs::write(&s, "#!/usr/bin/env -S python3 -u\nx\n").unwrap();
        assert_eq!(run(content, &s, &["interpreter"])["interpreter"], "python3");
        fs::write(&f, "héllo\n").unwrap();
        assert_eq!(run(content, &f, &["encoding"])["encoding"], "UTF-8");
        fs::write(&f, b"\xEF\xBB\xBFhi").unwrap();
        assert_eq!(run(content, &f, &["encoding"])["encoding"], "UTF-8 with BOM");
        assert_eq!(line_endings(b"a\nb\r\nc"), Some("Mixed"));
        assert_eq!(line_endings(b"abc"), None);
        let b = d.path().join("b.bin");
        fs::write(&b, [0u8, 1, 2, 3]).unwrap();
        let v = run(content, &b, &["textOrBinary", "encoding", "lines"]);
        assert_eq!(v["textOrBinary"], "Binary");
        assert!(!v.contains_key("encoding") && !v.contains_key("lines"));
    }

    #[test]
    fn checksums_match_known_values() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("abc");
        fs::write(&f, "abc").unwrap();
        let v = run(hashes, &f, &["sha256", "sha1", "md5", "crc32"]);
        assert_eq!(v["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(v["sha1"], "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(v["md5"], "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(v["crc32"], "352441c2");
        assert_eq!(run(hashes, &f, &["md5"]).len(), 1, "only what was asked for");
        assert!(run(hashes, d.path(), &["md5"]).is_empty(), "folders have none");
    }

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (n, c) in files {
            z.start_file(*n, opts).unwrap();
            z.write_all(c.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn office_documents_give_their_properties() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("report.docx");
        make_zip(
            &f,
            &[
                ("docProps/core.xml", r#"<cp:coreProperties><dc:title>Q3 &amp; Q4</dc:title><dc:creator>Ada</dc:creator><cp:lastModifiedBy>Bob</cp:lastModifiedBy><dcterms:created xsi:type="dcterms:W3CDTF">2024-05-09T14:03:22Z</dcterms:created></cp:coreProperties>"#),
                ("docProps/app.xml", "<Properties><Application>Microsoft Office Word</Application><Pages>12</Pages><Words>3400</Words><Company>Acme</Company></Properties>"),
            ],
        );
        let v = run(documents, &f, &["docTitle", "docAuthor", "docLastBy", "docApp", "docPages", "docWords", "docCompany", "docCreated", "docSlides"]);
        assert_eq!(v["docTitle"], "Q3 & Q4");
        assert_eq!((v["docAuthor"].as_str(), v["docLastBy"].as_str()), ("Ada", "Bob"));
        assert_eq!((v["docApp"].as_str(), v["docPages"].as_str(), v["docWords"].as_str(), v["docCompany"].as_str()), ("Microsoft Office Word", "12", "3400", "Acme"));
        assert_eq!(v["docCreated"], "1715263402000");
        assert!(!v.contains_key("docSlides"));
    }

    #[test]
    fn opendocument_and_pdf_properties() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("memo.odt");
        make_zip(&f, &[("meta.xml", r#"<office:meta><dc:title>Memo</dc:title><meta:initial-creator>Cy</meta:initial-creator><meta:document-statistic meta:page-count="3" meta:word-count="250"/></office:meta>"#)]);
        let v = run(documents, &f, &["docTitle", "docAuthor", "docPages", "docWords"]);
        assert_eq!((v["docTitle"].as_str(), v["docAuthor"].as_str(), v["docPages"].as_str(), v["docWords"].as_str()), ("Memo", "Cy", "3", "250"));

        let p = d.path().join("a.pdf");
        fs::write(
            &p,
            "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type /Pages/Count 2/Kids[3 0 R 4 0 R]>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R>>endobj\n4 0 obj<</Type /Page /Parent 2 0 R>>endobj\n5 0 obj<</Title(A \\(b\\) \\101)/Author<FEFF00480069>/CreationDate(D:20240509140322+02'00')/Producer(LibreOffice)>>endobj\n%%EOF",
        )
        .unwrap();
        let v = run(documents, &p, &["docTitle", "docAuthor", "docPages", "docApp", "docCreated"]);
        assert_eq!(v["docTitle"], "A (b) A");
        assert_eq!(v["docAuthor"], "Hi");
        assert_eq!(v["docPages"], "2");
        assert_eq!(v["docApp"], "LibreOffice");
        assert_eq!(v["docCreated"], "1715256202000", "two hours ahead of UTC");
    }

    #[test]
    fn zip_contents_and_compression() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.zip");
        make_zip(&f, &[("a.txt", &"x".repeat(10_000)), ("b.txt", &"y".repeat(10_000))]);
        let v = run(archive, &f, &["archiveItems", "archiveSize", "archiveRatio"]);
        assert_eq!(v["archiveItems"], "2");
        assert_eq!(v["archiveSize"], "20000");
        assert!(v["archiveRatio"].parse::<u32>().unwrap() < 5);
    }

    #[test]
    fn programs_say_what_they_are() {
        let d = tempfile::tempdir().unwrap();
        // A tiny PE header for an x64 console program built in 2020.
        let mut pe = vec![0u8; 0x200];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3C..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        pe[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[0x88..0x8C].copy_from_slice(&1_600_000_000u32.to_le_bytes());
        pe[0x80 + 24 + 68..0x80 + 24 + 70].copy_from_slice(&3u16.to_le_bytes());
        for (key, val) in [("CompanyName", "Acme Ltd"), ("FileVersion", "1.2.3.4")] {
            pe.extend([0u8; 6]);
            pe.extend(key.encode_utf16().flat_map(|u| u.to_le_bytes()));
            pe.extend([0, 0, 0, 0]);
            pe.extend(val.encode_utf16().flat_map(|u| u.to_le_bytes()));
            pe.extend([0, 0]);
        }
        let f = d.path().join("tool.exe");
        fs::write(&f, &pe).unwrap();
        let v = run(programs, &f, &["exeArch", "exeType", "exeSubsystem", "exeCompiled", "exeCompany", "exeVersion", "exeProduct"]);
        assert_eq!(v["exeArch"], "x64 (64-bit)");
        assert_eq!(v["exeType"], "Windows program");
        assert_eq!(v["exeSubsystem"], "Console");
        assert_eq!(v["exeCompiled"], "1600000000000");
        assert_eq!((v["exeCompany"].as_str(), v["exeVersion"].as_str()), ("Acme Ltd", "1.2.3.4"));
        assert!(!v.contains_key("exeProduct"));

        let mut elf = vec![0u8; 64];
        elf[..4].copy_from_slice(b"\x7FELF");
        elf[4] = 2;
        elf[5] = 1;
        elf[16..18].copy_from_slice(&3u16.to_le_bytes());
        elf[18..20].copy_from_slice(&0xB7u16.to_le_bytes());
        let so = d.path().join("libx.so");
        fs::write(&so, &elf).unwrap();
        let v = run(programs, &so, &["exeArch", "exeType"]);
        assert_eq!((v["exeArch"].as_str(), v["exeType"].as_str()), ("ARM64 (64-bit)", "Linux shared library"));
        let plain = d.path().join("x.txt");
        fs::write(&plain, "hi").unwrap();
        assert!(run(programs, &plain, &["exeArch", "exeType"]).is_empty());
    }
}
