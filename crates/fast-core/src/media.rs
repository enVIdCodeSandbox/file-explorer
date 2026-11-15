//! What a photo, song or video says about itself: camera and exposure for
//! pictures, tags and length for audio, size and length for video. Only the
//! file's header is read, never the whole file, so it is cheap enough to run
//! every time a file is selected.

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    pub label: String,
    pub value: String,
}

fn fact(label: &str, value: impl Into<String>) -> Option<Fact> {
    let value = value.into();
    let value = value.trim().trim_matches('"').trim().to_string();
    (!value.is_empty()).then(|| Fact { label: label.into(), value })
}

const IMAGE: &[&str] = &["jpg", "jpeg", "tif", "tiff", "png", "webp", "heic", "heif", "avif"];
const AUDIO: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "aac", "wav", "wv", "ape", "aiff", "aif", "mpc"];
const VIDEO: &[&str] = &["mp4", "m4v", "mov"];

/// The facts a file holds about itself, in the order worth showing. Empty for
/// anything that is not a recognized media file or that cannot be read.
pub fn read(path: &Path) -> Vec<Fact> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let facts = if IMAGE.contains(&ext.as_str()) {
        exif_facts(path)
    } else if AUDIO.contains(&ext.as_str()) {
        audio_facts(path)
    } else if VIDEO.contains(&ext.as_str()) {
        video_facts(path)
    } else {
        Vec::new()
    };
    facts.into_iter().flatten().collect()
}

fn exif_facts(path: &Path) -> Vec<Option<Fact>> {
    use exif::{In, Reader, Tag, Value};
    let Ok(file) = File::open(path) else { return Vec::new() };
    let Ok(exif) = Reader::new().read_from_container(&mut BufReader::new(file)) else { return Vec::new() };
    let text = |tag: Tag| exif.get_field(tag, In::PRIMARY).map(|f| f.display_value().with_unit(&exif).to_string());
    let ascii = |tag: Tag| {
        exif.get_field(tag, In::PRIMARY).and_then(|f| match &f.value {
            Value::Ascii(v) => v.first().map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string()),
            _ => None,
        })
    };

    let make = ascii(Tag::Make).unwrap_or_default();
    let model = ascii(Tag::Model).unwrap_or_default();
    // Many cameras repeat the maker in the model ("Canon" + "Canon EOS R5").
    let camera = if !make.is_empty() && model.to_lowercase().starts_with(&make.to_lowercase()) { model.clone() } else { format!("{make} {model}") };

    let taken = ascii(Tag::DateTimeOriginal).or_else(|| ascii(Tag::DateTime)).map(|d| nice_date(&d));
    let exposure = text(Tag::ExposureTime);
    let aperture = text(Tag::FNumber);
    let iso = text(Tag::PhotographicSensitivity);
    let focal = text(Tag::FocalLength);

    vec![
        fact("Camera", camera),
        fact("Camera maker", make.clone()),
        fact("Lens", ascii(Tag::LensModel).unwrap_or_default()),
        fact("Taken", taken.unwrap_or_default()),
        fact("Exposure", exposure.unwrap_or_default()),
        fact("Aperture", aperture.unwrap_or_default()),
        fact("ISO", iso.unwrap_or_default()),
        fact("Focal length", focal.unwrap_or_default()),
        fact("Flash", text(Tag::Flash).unwrap_or_default()),
        fact("Orientation", text(Tag::Orientation).filter(|o| !o.contains("row 0 at top")).unwrap_or_default()),
        fact("Software", ascii(Tag::Software).unwrap_or_default()),
        fact("Location", gps(&exif).unwrap_or_default()),
        fact("Altitude", text(Tag::GPSAltitude).unwrap_or_default()),
        fact("White balance", text(Tag::WhiteBalance).unwrap_or_default()),
        fact("Metering", text(Tag::MeteringMode).unwrap_or_default()),
        fact("Exposure program", text(Tag::ExposureProgram).unwrap_or_default()),
        fact("Exposure bias", text(Tag::ExposureBiasValue).unwrap_or_default()),
        fact("Color space", text(Tag::ColorSpace).unwrap_or_default()),
        fact("Resolution", text(Tag::XResolution).unwrap_or_default()),
        fact("Photographer", ascii(Tag::Artist).unwrap_or_default()),
        fact("Copyright", ascii(Tag::Copyright).unwrap_or_default()),
        fact("Description", ascii(Tag::ImageDescription).unwrap_or_default()),
    ]
}

/// "2024:05:09 14:03:22" -> "2024-05-09 14:03:22"
fn nice_date(raw: &str) -> String {
    let raw = raw.trim().trim_end_matches('\0');
    match raw.split_once(' ') {
        Some((d, t)) if d.len() == 10 => format!("{} {}", d.replace(':', "-"), t),
        _ => raw.to_string(),
    }
}

fn gps(exif: &exif::Exif) -> Option<String> {
    use exif::{In, Tag, Value};
    let degrees = |tag: Tag, reference: Tag| -> Option<f64> {
        let f = exif.get_field(tag, In::PRIMARY)?;
        let Value::Rational(v) = &f.value else { return None };
        if v.len() < 3 || v.iter().any(|r| r.denom == 0) {
            return None;
        }
        let value = v[0].to_f64() + v[1].to_f64() / 60.0 + v[2].to_f64() / 3600.0;
        let r = exif.get_field(reference, In::PRIMARY).map(|f| f.display_value().to_string()).unwrap_or_default();
        Some(if r.contains('S') || r.contains('W') { -value } else { value })
    };
    let lat = degrees(Tag::GPSLatitude, Tag::GPSLatitudeRef)?;
    let lon = degrees(Tag::GPSLongitude, Tag::GPSLongitudeRef)?;
    Some(format!("{:.5}° {}, {:.5}° {}", lat.abs(), if lat < 0.0 { "S" } else { "N" }, lon.abs(), if lon < 0.0 { "W" } else { "E" }))
}

fn audio_facts(path: &Path) -> Vec<Option<Fact>> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::prelude::*;
    let Ok(tagged) = lofty::read_from_path(path) else { return Vec::new() };
    let props = tagged.properties();
    let mut out = Vec::new();
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        out.push(fact("Title", tag.title().map(|s| s.to_string()).unwrap_or_default()));
        out.push(fact("Artist", tag.artist().map(|s| s.to_string()).unwrap_or_default()));
        out.push(fact("Album", tag.album().map(|s| s.to_string()).unwrap_or_default()));
        out.push(fact("Year", tag.date().map(|d| d.year.to_string()).unwrap_or_default()));
        out.push(fact(
            "Track",
            match (tag.track(), tag.track_total()) {
                (Some(n), Some(t)) => format!("{n} of {t}"),
                (Some(n), None) => n.to_string(),
                _ => String::new(),
            },
        ));
        out.push(fact("Genre", tag.genre().map(|s| s.to_string()).unwrap_or_default()));
        out.push(fact("Album artist", tag.get_string(ItemKey::AlbumArtist).unwrap_or_default()));
        out.push(fact("Composer", tag.get_string(ItemKey::Composer).unwrap_or_default()));
        out.push(fact(
            "Disc",
            match (tag.disk(), tag.disk_total()) {
                (Some(n), Some(t)) => format!("{n} of {t}"),
                (Some(n), None) => n.to_string(),
                _ => String::new(),
            },
        ));
        out.push(fact("BPM", tag.get_string(ItemKey::IntegerBpm).or_else(|| tag.get_string(ItemKey::Bpm)).unwrap_or_default()));
        out.push(fact("Publisher", tag.get_string(ItemKey::Publisher).unwrap_or_default()));
        out.push(fact("Copyright", tag.get_string(ItemKey::CopyrightMessage).unwrap_or_default()));
        out.push(fact("Comment", tag.comment().map(|s| s.to_string()).unwrap_or_default()));
    }
    out.push(fact("Length", duration(props.duration().as_secs_f64())));
    out.push(fact("Bit rate", props.audio_bitrate().map(|b| format!("{b} kbps")).unwrap_or_default()));
    out.push(fact("Sample rate", props.sample_rate().map(|r| format!("{:.1} kHz", r as f64 / 1000.0)).unwrap_or_default()));
    out.push(fact("Bit depth", props.bit_depth().map(|b| format!("{b}-bit")).unwrap_or_default()));
    out.push(fact(
        "Channels",
        match props.channels() {
            Some(1) => "Mono".to_string(),
            Some(2) => "Stereo".to_string(),
            Some(n) => format!("{n} channels"),
            None => String::new(),
        },
    ));
    out
}

fn video_facts(path: &Path) -> Vec<Option<Fact>> {
    let Ok(file) = File::open(path) else { return Vec::new() };
    let Ok(size) = file.metadata().map(|m| m.len()) else { return Vec::new() };
    let Ok(mp4) = mp4::Mp4Reader::read_header(BufReader::new(file), size) else { return Vec::new() };
    let mut out = vec![fact("Length", duration(mp4.duration().as_secs_f64()))];
    for track in mp4.tracks().values() {
        match track.track_type() {
            Ok(mp4::TrackType::Video) => {
                out.push(fact("Frame size", format!("{} × {}", track.width(), track.height())));
                out.push(fact("Frame rate", format!("{:.2} fps", track.frame_rate())));
                out.push(fact("Video codec", track.media_type().map(|m| m.to_string()).unwrap_or_default()));
                out.push(fact("Video bit rate", format!("{} kbps", track.bitrate() / 1000)).filter(|f| f.value != "0 kbps"));
            }
            Ok(mp4::TrackType::Audio) => out.push(fact("Audio codec", track.media_type().map(|m| m.to_string()).unwrap_or_default())),
            _ => {}
        }
    }
    out
}

/// 3725 s -> "1:02:05", 75 s -> "1:15", 0 -> nothing.
pub fn duration(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.5 {
        return String::new();
    }
    let total = seconds.round() as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration(75.0), "1:15");
        assert_eq!(duration(3725.0), "1:02:05");
        assert_eq!(duration(0.0), "");
        assert_eq!(duration(f64::NAN), "");
        assert_eq!(duration(59.6), "1:00");
    }

    #[test]
    fn exif_dates_are_made_readable() {
        assert_eq!(nice_date("2024:05:09 14:03:22"), "2024-05-09 14:03:22");
        assert_eq!(nice_date("unknown"), "unknown");
    }

    #[test]
    fn unknown_and_unreadable_files_give_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let txt = tmp.path().join("a.txt");
        std::fs::write(&txt, b"hello").unwrap();
        assert!(read(&txt).is_empty());
        let fake = tmp.path().join("fake.jpg");
        std::fs::write(&fake, b"not a jpeg").unwrap();
        assert!(read(&fake).is_empty());
        let fake = tmp.path().join("fake.mp3");
        std::fs::write(&fake, b"not an mp3").unwrap();
        assert!(read(&fake).is_empty());
        assert!(read(&tmp.path().join("missing.mp4")).is_empty());
    }

    /// A tiny TIFF with Make, Model and a GPS position, the layout every EXIF block uses.
    fn tiff_with_camera() -> Vec<u8> {
        fn entry(tag: u16, kind: u16, count: u32, value: u32) -> Vec<u8> {
            let mut e = Vec::new();
            e.extend(tag.to_le_bytes());
            e.extend(kind.to_le_bytes());
            e.extend(count.to_le_bytes());
            e.extend(value.to_le_bytes());
            e
        }
        let make = b"Canon\0";
        let model = b"Canon EOS R5\0";
        // Layout: header (8) + IFD0 (2 + 3*12 + 4) + data
        let ifd0_len = 2 + 3 * 12 + 4;
        let data_start = 8 + ifd0_len as u32;
        let model_at = data_start + make.len() as u32;
        let gps_ifd_at = model_at + model.len() as u32;
        let mut t = Vec::new();
        t.extend(b"II*\0");
        t.extend(8u32.to_le_bytes());
        t.extend(3u16.to_le_bytes());
        t.extend(entry(0x010F, 2, make.len() as u32, data_start));
        t.extend(entry(0x0110, 2, model.len() as u32, model_at));
        t.extend(entry(0x8825, 4, 1, gps_ifd_at));
        t.extend(0u32.to_le_bytes());
        t.extend(make);
        t.extend(model);
        // GPS IFD: LatitudeRef "N", Latitude 47/1 36/1 22/1, LongitudeRef "W", Longitude 122/1 19/1 59/1
        let gps_len = 2 + 4 * 12 + 4;
        let rationals_at = gps_ifd_at + gps_len as u32;
        t.extend(4u16.to_le_bytes());
        t.extend(entry(0x0001, 2, 2, u32::from_le_bytes([b'N', 0, 0, 0])));
        t.extend(entry(0x0002, 5, 3, rationals_at));
        t.extend(entry(0x0003, 2, 2, u32::from_le_bytes([b'W', 0, 0, 0])));
        t.extend(entry(0x0004, 5, 3, rationals_at + 24));
        t.extend(0u32.to_le_bytes());
        for (n, d) in [(47u32, 1u32), (36, 1), (22, 1), (122, 1), (19, 1), (59, 1)] {
            t.extend(n.to_le_bytes());
            t.extend(d.to_le_bytes());
        }
        t
    }

    #[test]
    fn photo_camera_and_position_are_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("photo.tif");
        std::fs::write(&path, tiff_with_camera()).unwrap();
        let facts = read(&path);
        let get = |l: &str| facts.iter().find(|f| f.label == l).map(|f| f.value.clone());
        assert_eq!(get("Camera").as_deref(), Some("Canon EOS R5"), "the maker is not repeated: {facts:?}");
        let loc = get("Location").expect("a location");
        assert!(loc.starts_with("47.60611° N, 122.33306° W"), "{loc}");
    }

    fn ffmpeg(args: &[&str]) -> bool {
        Command::new("ffmpeg").args(["-y", "-loglevel", "error"]).args(args).status().map(|s| s.success()).unwrap_or(false)
    }

    #[test]
    fn audio_tags_and_length_are_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("song.mp3");
        if !ffmpeg(&["-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-metadata", "title=Test Tone", "-metadata", "artist=Nobody", "-metadata", "album=Beeps", "-metadata", "track=2/9", "-metadata", "date=2021", path.to_str().unwrap()]) {
            eprintln!("ffmpeg not available; skipping");
            return;
        }
        let facts = read(&path);
        let get = |l: &str| facts.iter().find(|f| f.label == l).map(|f| f.value.clone());
        assert_eq!(get("Title").as_deref(), Some("Test Tone"));
        assert_eq!(get("Artist").as_deref(), Some("Nobody"));
        assert_eq!(get("Album").as_deref(), Some("Beeps"));
        assert_eq!(get("Track").as_deref(), Some("2 of 9"));
        assert_eq!(get("Year").as_deref(), Some("2021"));
        assert_eq!(get("Length").as_deref(), Some("0:03"));
    }

    #[test]
    fn video_size_rate_and_length_are_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("clip.mp4");
        if !ffmpeg(&["-f", "lavfi", "-i", "testsrc=size=320x240:rate=25:duration=2", "-pix_fmt", "yuv420p", path.to_str().unwrap()]) {
            eprintln!("ffmpeg not available; skipping");
            return;
        }
        let facts = read(&path);
        let get = |l: &str| facts.iter().find(|f| f.label == l).map(|f| f.value.clone());
        assert_eq!(get("Frame size").as_deref(), Some("320 × 240"));
        assert_eq!(get("Frame rate").as_deref(), Some("25.00 fps"));
        assert_eq!(get("Length").as_deref(), Some("0:02"));
    }
}
