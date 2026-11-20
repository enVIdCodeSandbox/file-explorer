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
    ("cameraMaker", "Camera maker"), ("flash", "Flash"), ("orientation", "Orientation"), ("gps", "Location"),
    ("description", "Description"), ("copyright", "Copyright"), ("photographer", "Photographer"),
    ("whiteBalance", "White balance"), ("metering", "Metering"), ("exposureProgram", "Exposure program"),
    ("exposureBias", "Exposure bias"), ("colorSpace", "Color space"), ("dpi", "Resolution"), ("altitude", "Altitude"),
    ("track", "Track"), ("albumArtist", "Album artist"), ("composer", "Composer"), ("disc", "Disc"),
    ("comment", "Comment"), ("channels", "Channels"), ("videoBitrate", "Video bit rate"), ("bpm", "BPM"),
    ("bitDepth", "Bit depth"), ("publisher", "Publisher"),
];

/// Derived from the picture or video size.
const SHAPE: &[&str] = &["width", "height", "megapixels", "aspect", "shape"];

/// Taken from the file system rather than the header.
const FROM_DETAILS: &[&str] = &["accessed", "sizeOnDisk", "items", "attributes", "owner", "mime", "linkTarget", "dimensions"];

/// The values of `keys` for one file or folder. A key with nothing to say is left out. Dates are
/// milliseconds and sizes are bytes, as text, for the interface to format.
pub fn values(path: &Path, keys: &[String]) -> BTreeMap<String, String> {
    use crate::columns_extra as x;
    let mut out = BTreeMap::new();
    let asked = |k: &str| keys.iter().any(|v| v == k);
    // The length in seconds (which sorts properly) is worked out from the length the header gives.
    let want_seconds = asked("lengthSeconds");
    let wants = |k: &str| asked(k) || (k == "length" && want_seconds);
    let wants_any = |list: &[&str]| list.iter().any(|k| asked(k));
    let media_keys: Vec<&(&str, &str)> = MEDIA.iter().filter(|(k, _)| wants(k)).collect();
    let is_dir = path.is_dir();

    if FROM_DETAILS.iter().any(|k| wants(k)) {
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
    } else if !media_keys.is_empty() && !is_dir {
        let facts = crate::media::read(path);
        for (k, label) in &media_keys {
            if let Some(f) = facts.iter().find(|f| f.label == *label) {
                out.insert((*k).to_string(), f.value.clone());
            }
        }
    }

    // Size in pixels, and what follows from it.
    if wants_any(SHAPE) && !is_dir {
        if let Some((w, h)) = pixel_size(path) {
            let mut put = |k: &str, v: String| {
                if asked(k) {
                    out.insert(k.to_string(), v);
                }
            };
            put("width", w.to_string());
            put("height", h.to_string());
            put("megapixels", format!("{:.1}", f64::from(w) * f64::from(h) / 1_000_000.0));
            put("aspect", aspect(w, h));
            put("shape", (if w > h { "Landscape" } else if h > w { "Portrait" } else { "Square" }).to_string());
        }
    }
    if want_seconds {
        let secs = out.get("length").and_then(|l| clock_seconds(l));
        if !asked("length") {
            out.remove("length");
        }
        if let Some(s) = secs {
            out.insert("lengthSeconds".into(), s.to_string());
        }
    }

    // The program a file opens with (looked up once per kind of file, not per file).
    if asked("openWith") && !is_dir {
        out.insert("openWith".into(), crate::openwith::default_opener(path, false).text());
    }

    // Read from the file system or from the file's contents.
    if wants_any(x::FS_KEYS) {
        x::file_system(path, &|k| asked(k), &mut out);
    }
    if !is_dir {
        if wants_any(x::CONTENT_KEYS) {
            x::content(path, &|k| asked(k), &mut out);
        }
        if wants_any(x::HASH_KEYS) {
            x::hashes(path, &|k| asked(k), &mut out);
        }
        if wants_any(x::DOC_KEYS) {
            x::documents(path, &|k| asked(k), &mut out);
        }
        if wants_any(x::EXE_KEYS) {
            x::programs(path, &|k| asked(k), &mut out);
        }
        if wants_any(x::ZIP_KEYS) {
            x::archive(path, &|k| asked(k), &mut out);
        }
    }
    out
}

/// Pixel size of a picture, or of a video's picture.
fn pixel_size(path: &Path) -> Option<(u32, u32)> {
    if let Ok(s) = imagesize::size(path) {
        return Some((s.width as u32, s.height as u32));
    }
    let facts = crate::media::read(path);
    let size = facts.iter().find(|f| f.label == "Frame size")?;
    let (w, h) = size.value.split_once('×')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// "16:9", "4:3", … for the ratios people know, else "1.37:1".
fn aspect(w: u32, h: u32) -> String {
    if w == 0 || h == 0 {
        return String::new();
    }
    let r = f64::from(w) / f64::from(h);
    for (a, b) in [(1.0, 1.0), (4.0, 3.0), (3.0, 2.0), (16.0, 10.0), (16.0, 9.0), (5.0, 4.0), (21.0, 9.0), (2.0, 1.0), (7.0, 5.0)] {
        for (n, d) in [(a, b), (b, a)] {
            if ((r / (n / d)) - 1.0).abs() < 0.02 {
                return format!("{n}:{d}");
            }
        }
    }
    format!("{r:.2}:1")
}

/// "1:02:05" -> 3725, "3:45" -> 225.
fn clock_seconds(s: &str) -> Option<u64> {
    s.split(':').try_fold(0u64, |acc, p| Some(acc * 60 + p.trim().parse::<u64>().ok()?))
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
    fn picture_shape_and_clock_lengths() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("pic.png");
        std::fs::write(&f, png(4000, 3000)).unwrap();
        let v = values(&f, &keys(&["width", "height", "megapixels", "aspect", "shape"]));
        assert_eq!((v["width"].as_str(), v["height"].as_str(), v["megapixels"].as_str(), v["aspect"].as_str(), v["shape"].as_str()), ("4000", "3000", "12.0", "4:3", "Landscape"));
        std::fs::write(&f, png(1080, 1920)).unwrap();
        let v = values(&f, &keys(&["aspect", "shape"]));
        assert_eq!((v["aspect"].as_str(), v["shape"].as_str()), ("9:16", "Portrait"));
        assert_eq!(aspect(1366, 768), "16:9");
        assert_eq!(aspect(1000, 1371), "0.73:1");
        assert_eq!(clock_seconds("1:02:05"), Some(3725));
        assert_eq!(clock_seconds("3:45"), Some(225));
        assert_eq!(clock_seconds("x"), None);
    }

    #[test]
    fn a_missing_file_gives_nothing() {
        assert!(values(Path::new("/definitely/not/here"), &keys(&["accessed"])).is_empty());
    }
}
