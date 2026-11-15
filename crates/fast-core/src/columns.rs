//! The values behind the extra Details columns (date accessed, dimensions, artist, camera…),
//! read from the file itself. The folder listing already carries name, size and dates; these are
//! fetched for the rows on screen, only for the columns that are shown.

use crate::details;
use std::collections::BTreeMap;
use std::path::Path;

/// Column id -> the label of the fact the file's own header gives for it.
const MEDIA: &[(&str, &str)] = &[
    ("taken", "Taken"), ("camera", "Camera"), ("lens", "Lens"), ("iso", "ISO"), ("aperture", "Aperture"),
    ("exposure", "Exposure"), ("focal", "Focal length"), ("software", "Software"), ("length", "Length"),
    ("artist", "Artist"), ("album", "Album"), ("title", "Title"), ("genre", "Genre"), ("year", "Year"),
    ("bitrate", "Bit rate"), ("sampleRate", "Sample rate"), ("frameSize", "Frame size"), ("frameRate", "Frame rate"),
    ("videoCodec", "Video codec"), ("audioCodec", "Audio codec"),
];

/// Taken from the file system rather than the header.
const FROM_DETAILS: &[&str] = &["accessed", "sizeOnDisk", "items", "attributes", "owner", "mime", "linkTarget", "dimensions"];

/// The values of `keys` for one file or folder. A key with nothing to say is left out. Dates are
/// milliseconds and sizes are bytes, as text, for the interface to format.
pub fn values(path: &Path, keys: &[String]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let wants = |k: &str| keys.iter().any(|x| x == k);
    let media_keys: Vec<&(&str, &str)> = MEDIA.iter().filter(|(k, _)| wants(k)).collect();
    let need_details = FROM_DETAILS.iter().any(|k| wants(k));
    if need_details {
        let Ok(d) = details::details(path, &[]) else { return out };
        let mut put = |k: &str, v: Option<String>| {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                out.insert(k.to_string(), v);
            }
        };
        if wants("accessed") {
            put("accessed", d.accessed.map(|t| t.to_string()));
        }
        if wants("sizeOnDisk") {
            put("sizeOnDisk", d.size_on_disk.map(|t| t.to_string()));
        }
        if wants("items") {
            put("items", d.item_count.map(|t| t.to_string()));
        }
        if wants("attributes") {
            let text = if !d.attributes.is_empty() { d.attributes.join(", ") } else { d.unix.as_ref().map(|u| u.mode.clone()).unwrap_or_default() };
            put("attributes", Some(text));
        }
        if wants("owner") {
            put("owner", d.unix.as_ref().map(|u| u.uid.to_string()));
        }
        if wants("mime") {
            put("mime", d.mime.clone());
        }
        if wants("linkTarget") {
            put("linkTarget", d.symlink_target.clone());
        }
        if wants("dimensions") {
            put("dimensions", d.image.map(|(w, h)| format!("{w} × {h}")));
        }
        for (k, label) in &media_keys {
            if let Some(f) = d.media.iter().find(|f| f.label == *label) {
                out.insert((*k).to_string(), f.value.clone());
            }
        }
    } else if !media_keys.is_empty() && !path.is_dir() {
        let facts = crate::media::read(path);
        for (k, label) in &media_keys {
            if let Some(f) = facts.iter().find(|f| f.label == *label) {
                out.insert((*k).to_string(), f.value.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R'];
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reads_only_what_was_asked_for() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pic.png");
        std::fs::write(&f, png(640, 480)).unwrap();
        let v = values(&f, &keys(&["dimensions", "mime", "accessed", "artist"]));
        assert_eq!(v.get("dimensions").map(String::as_str), Some("640 × 480"));
        assert_eq!(v.get("mime").map(String::as_str), Some("image/png"));
        assert!(v.get("accessed").is_some_and(|t| t.parse::<i64>().is_ok()));
        assert!(!v.contains_key("artist"), "a picture has no artist: left out");
        assert!(values(&f, &keys(&["mime"])).len() == 1);
        assert!(values(&f, &keys(&["bogus"])).is_empty());
    }

    #[test]
    fn counts_the_items_in_a_folder() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a"), b"x").unwrap();
        std::fs::create_dir(d.path().join("b")).unwrap();
        let v = values(d.path(), &keys(&["items", "dimensions"]));
        assert_eq!(v.get("items").map(String::as_str), Some("2"));
        assert!(!v.contains_key("dimensions"));
    }

    #[test]
    fn a_missing_file_gives_nothing() {
        assert!(values(Path::new("/definitely/not/here"), &keys(&["accessed"])).is_empty());
    }
}
