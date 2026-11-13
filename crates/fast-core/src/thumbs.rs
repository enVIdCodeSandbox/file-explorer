//! Thumbnail generation with an on-disk cache.
//!
//! The UI only asks for thumbnails of rows on or near the screen, and only
//! after a row has been visible for a moment, so fast scrolling never queues
//! work. Results are cached on disk keyed by path, size and modification
//! time, so each file is rendered once.
//!
//! Pictures are decoded here. Documents and videos are rendered by what the
//! system already has: the Windows shell's own thumbnail handlers (the ones
//! Explorer uses), macOS Quick Look, and on Linux `pdftoppm` for PDFs and
//! `ffmpegthumbnailer` or `ffmpeg` for videos when they are installed. A file
//! the system cannot render simply has no thumbnail and keeps its icon.

use crate::{CoreError, Result};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const IMAGES: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];
/// Kept for callers that only care about pictures.
pub const SUPPORTED: &[&str] = IMAGES;
pub const DOCUMENTS: &[&str] = &["pdf", "docx", "doc", "xlsx", "xls", "pptx", "ppt", "odt", "ods", "odp", "rtf", "epub", "svg"];
pub const VIDEOS: &[&str] = &["mp4", "m4v", "mov", "mkv", "avi", "webm", "wmv", "flv", "mpg", "mpeg", "3gp", "mts", "m2ts"];

/// What kind of preview a file gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Document,
    Video,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    let ext = ext.as_str();
    if IMAGES.contains(&ext) {
        Some(Kind::Image)
    } else if DOCUMENTS.contains(&ext) {
        Some(Kind::Document)
    } else if VIDEOS.contains(&ext) {
        Some(Kind::Video)
    } else {
        None
    }
}

/// Is `name` on the system's PATH (and so usable as a program)?
fn have(name: &str) -> bool {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(&exe).is_file()))
}

/// The file extensions this computer can make thumbnails for: always the
/// pictures, plus whatever documents and videos the system can render.
pub fn supported_extensions() -> &'static [&'static str] {
    static LIST: OnceLock<Vec<&'static str>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut v: Vec<&'static str> = IMAGES.to_vec();
        if cfg!(any(windows, target_os = "macos")) {
            v.extend(DOCUMENTS.iter().filter(|e| **e != "svg"));
            v.extend(VIDEOS);
        } else {
            if have("pdftoppm") {
                v.push("pdf");
            }
            if have("ffmpegthumbnailer") || have("ffmpeg") {
                v.extend(VIDEOS);
            }
        }
        v
    })
}

/// How a thumbnail is made, beyond its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// Skip pictures larger than this many bytes (0 = no limit beyond the decoder's own).
    pub max_image_bytes: u64,
    /// Which frame of a video to show, as a percentage of its length.
    pub video_percent: u8,
}

impl Default for Options {
    fn default() -> Self {
        Options { max_image_bytes: MAX_SOURCE_BYTES, video_percent: 10 }
    }
}

/// Decoder memory ceiling; protects against decompression bombs.
const MAX_DECODE_BYTES: u64 = 384 * 1024 * 1024;
/// Skip very large sources: decoding them costs more than the preview is worth.
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;
/// How long an outside program may take to render one thumbnail.
const RENDER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .map(|e| supported_extensions().contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

fn cache_file(cache_dir: &Path, path: &Path, len: u64, mtime: i64, px: u32, variant: u32) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    len.hash(&mut h);
    mtime.hash(&mut h);
    px.hash(&mut h);
    variant.hash(&mut h);
    cache_dir.join(format!("{:016x}.png", h.finish()))
}

/// PNG bytes of a thumbnail at most `px` pixels on its longest side.
pub fn thumbnail(path: &Path, px: u32, cache_dir: &Path) -> Result<Vec<u8>> {
    thumbnail_cached(path, px, Some(cache_dir))
}

/// Like [`thumbnail`], but with `None` nothing is read from or written to disk.
pub fn thumbnail_cached(path: &Path, px: u32, cache_dir: Option<&Path>) -> Result<Vec<u8>> {
    thumbnail_with(path, px, cache_dir, &Options::default())
}

pub fn thumbnail_with(path: &Path, px: u32, cache_dir: Option<&Path>, opts: &Options) -> Result<Vec<u8>> {
    if !is_supported(path) {
        return Err(CoreError::Unsupported("thumbnail for this file type"));
    }
    let kind = kind_of(path).ok_or(CoreError::Unsupported("thumbnail for this file type"))?;
    let meta = fs::metadata(path).map_err(|e| CoreError::io(path, e))?;
    if kind == Kind::Image && opts.max_image_bytes > 0 && meta.len() > opts.max_image_bytes {
        return Err(CoreError::Invalid("file too large for a thumbnail".into()));
    }
    let px = px.clamp(16, 512);
    let mtime = crate::to_millis(meta.modified()).unwrap_or(0);
    let variant = if kind == Kind::Video { opts.video_percent as u32 + 1 } else { 0 };
    let cached = cache_dir.map(|d| cache_file(d, path, meta.len(), mtime, px, variant));
    if let Some(Ok(bytes)) = cached.as_ref().map(fs::read) {
        return Ok(bytes);
    }

    let bytes = match kind {
        Kind::Image => {
            let mut reader = image::ImageReader::open(path)
                .map_err(|e| CoreError::io(path, e))?
                .with_guessed_format()
                .map_err(|e| CoreError::io(path, e))?;
            // Refuse images that would need a huge buffer, however small the file.
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(MAX_DECODE_BYTES);
            reader.limits(limits);
            let img = reader
                .decode()
                .map_err(|e| CoreError::Invalid(format!("cannot decode image: {e}")))?;
            encode(img.thumbnail(px, px))?
        }
        Kind::Document | Kind::Video => fit(&render::render(path, px, kind, opts)?, px)?,
    };

    // Best effort: a failed cache write only costs a re-render next time.
    if let (Some(dir), Some(file)) = (cache_dir, cached) {
        if fs::create_dir_all(dir).is_ok() && fs::write(&file, &bytes).is_ok() {
            note(dir, &file, path);
        }
    }
    Ok(bytes)
}

fn encode(img: image::DynamicImage) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| CoreError::Invalid(format!("cannot encode thumbnail: {e}")))?;
    Ok(bytes)
}

/// Scale an image another program made down to at most `px` on its longest side, as PNG.
fn fit(png: &[u8], px: u32) -> Result<Vec<u8>> {
    let img = image::load_from_memory(png).map_err(|e| CoreError::Invalid(format!("cannot read the rendered preview: {e}")))?;
    if img.width().max(img.height()) <= px {
        return encode(img);
    }
    encode(img.thumbnail(px, px))
}

/// Rendering documents and videos with what the system provides.
mod render {
    use super::{Kind, Options, RENDER_TIMEOUT};
    use crate::{CoreError, Result};
    use std::path::Path;

    /// PNG bytes of a preview of `path`, at about `px` pixels.
    pub fn render(path: &Path, px: u32, kind: Kind, opts: &Options) -> Result<Vec<u8>> {
        let _ = (kind, opts);
        platform(path, px, kind, opts)
    }

    #[cfg(windows)]
    fn platform(path: &Path, px: u32, _kind: Kind, _opts: &Options) -> Result<Vec<u8>> {
        windows_shell(path, px)
    }

    #[cfg(target_os = "macos")]
    fn platform(path: &Path, px: u32, _kind: Kind, _opts: &Options) -> Result<Vec<u8>> {
        let out = scratch_dir()?;
        let result = (|| {
            let mut cmd = std::process::Command::new("qlmanage");
            cmd.arg("-t").arg("-s").arg(px.to_string()).arg("-o").arg(&out).arg(path);
            run(cmd)?;
            let name = format!("{}.png", path.file_name().unwrap_or_default().to_string_lossy());
            std::fs::read(out.join(name)).map_err(|_| CoreError::Invalid("Quick Look made no preview".into()))
        })();
        let _ = std::fs::remove_dir_all(&out);
        result
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    fn platform(path: &Path, px: u32, kind: Kind, opts: &Options) -> Result<Vec<u8>> {
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let out = scratch_dir()?;
        let png = out.join("t.png");
        let result = (|| {
            let cmd = match (kind, ext.as_str()) {
                (Kind::Document, "pdf") => {
                    let mut c = std::process::Command::new("pdftoppm");
                    c.args(["-png", "-singlefile", "-f", "1", "-l", "1", "-scale-to", &px.to_string()]).arg(path).arg(out.join("t"));
                    c
                }
                (Kind::Video, _) if super::have("ffmpegthumbnailer") => {
                    let mut c = std::process::Command::new("ffmpegthumbnailer");
                    c.arg("-i").arg(path).arg("-o").arg(&png).arg("-s").arg(px.to_string()).arg("-t").arg(format!("{}%", opts.video_percent.min(95)));
                    c
                }
                (Kind::Video, _) => {
                    let mut c = std::process::Command::new("ffmpeg");
                    // Without the length of the clip a percentage cannot be turned into a time: take a frame at two seconds, or the first frame of a shorter clip.
                    c.args(["-v", "error", "-y", "-ss", "2", "-i"]).arg(path).args(["-frames:v", "1", "-vf"]).arg(format!("scale={px}:{px}:force_original_aspect_ratio=decrease")).arg(&png);
                    c
                }
                _ => return Err(CoreError::Unsupported("preview for this file type")),
            };
            run(cmd)?;
            match std::fs::read(&png) {
                Ok(b) if !b.is_empty() => Ok(b),
                // A video shorter than two seconds has nothing at that time: ask for its first frame.
                _ if kind == Kind::Video && !super::have("ffmpegthumbnailer") => {
                    let mut c = std::process::Command::new("ffmpeg");
                    c.args(["-v", "error", "-y", "-i"]).arg(path).args(["-frames:v", "1", "-vf"]).arg(format!("scale={px}:{px}:force_original_aspect_ratio=decrease")).arg(&png);
                    run(c)?;
                    std::fs::read(&png).map_err(|_| CoreError::Invalid("no preview was made".into()))
                }
                _ => Err(CoreError::Invalid("no preview was made".into())),
            }
        })();
        let _ = std::fs::remove_dir_all(&out);
        result
    }

    /// A fresh empty folder for one render's output.
    #[cfg(not(windows))]
    fn scratch_dir() -> Result<std::path::PathBuf> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!("fast-thumb-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
        Ok(dir)
    }

    /// Run a program to the end, but not forever.
    #[cfg(not(windows))]
    fn run(mut cmd: std::process::Command) -> Result<()> {
        use std::process::Stdio;
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CoreError::Invalid(format!("cannot start the preview program: {e}")))?;
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(_)) => return Err(CoreError::Invalid("the preview program failed".into())),
                Ok(None) if started.elapsed() > RENDER_TIMEOUT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CoreError::Invalid("the preview took too long".into()));
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(15)),
                Err(e) => return Err(CoreError::Invalid(format!("the preview program failed: {e}"))),
            }
        }
    }

    /// The thumbnail Windows Explorer would show: whatever handler the system has for the file type.
    #[cfg(windows)]
    fn windows_shell(path: &Path, px: u32) -> Result<Vec<u8>> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::SIZE;
        use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
        use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
        use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_THUMBNAILONLY};

        let fail = |what: &str, e: windows::core::Error| CoreError::Invalid(format!("{what}: {e}"));
        // SAFETY: plain calls into the shell with buffers sized from what Windows reports for the bitmap.
        unsafe {
            let inited = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let result = (|| {
                let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
                let factory: IShellItemImageFactory = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).map_err(|e| fail("the shell cannot open the file", e))?;
                // THUMBNAILONLY: no generic icon when the system has no preview for this kind of file.
                let hbmp = factory
                    .GetImage(SIZE { cx: px as i32, cy: px as i32 }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK)
                    .map_err(|e| fail("no preview available", e))?;
                let mut bmp = BITMAP::default();
                GetObjectW(hbmp.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bmp as *mut BITMAP as *mut _));
                let (w, h) = (bmp.bmWidth.max(1), bmp.bmHeight.max(1));
                let mut info = BITMAPINFO::default();
                info.bmiHeader = BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                };
                let mut pixels = vec![0u8; (w as usize) * (h as usize) * 4];
                let dc = GetDC(None);
                let lines = GetDIBits(dc, hbmp, 0, h as u32, Some(pixels.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
                ReleaseDC(None, dc);
                let _ = DeleteObject(hbmp.into());
                if lines == 0 {
                    return Err(CoreError::Invalid("could not read the preview".into()));
                }
                // Windows hands back BGRA; PNG wants RGBA.
                for px4 in pixels.chunks_exact_mut(4) {
                    px4.swap(0, 2);
                }
                let img = image::RgbaImage::from_raw(w as u32, h as u32, pixels).ok_or_else(|| CoreError::Invalid("unexpected preview size".into()))?;
                let mut out = Vec::new();
                image::DynamicImage::ImageRgba8(img)
                    .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                    .map_err(|e| CoreError::Invalid(format!("cannot encode thumbnail: {e}")))?;
                Ok(out)
            })();
            if inited {
                CoUninitialize();
            }
            result
        }
    }
}

const MANIFEST: &str = "sources.tsv";
static MANIFEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Remember which source file a cached thumbnail came from, so the thumbnails
/// of one folder or drive can be found and removed later without scanning it.
fn note(dir: &Path, file: &Path, source: &Path) {
    use std::io::Write;
    let _g = MANIFEST_LOCK.lock();
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let src = source.to_string_lossy();
    if src.contains(['\t', '\n']) {
        return; // cannot be recorded on one line; trim_cache will age it out
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(dir.join(MANIFEST)) {
        let _ = writeln!(f, "{name}\t{src}");
    }
}

/// Delete the cached thumbnails of every picture inside `root` (the folder
/// itself, everything below it). Returns how many files and bytes went.
pub fn clear_under(cache_dir: &Path, root: &Path) -> (usize, u64) {
    let _g = MANIFEST_LOCK.lock();
    let rule = root.to_string_lossy().into_owned();
    let manifest = cache_dir.join(MANIFEST);
    let Ok(text) = fs::read_to_string(&manifest) else { return (0, 0) };
    let (mut kept, mut files, mut bytes) = (String::new(), 0usize, 0u64);
    for line in text.lines() {
        let Some((name, src)) = line.split_once('\t') else { continue };
        if crate::cachepolicy::is_under(Path::new(src), &rule) {
            let f = cache_dir.join(name);
            if let Ok(m) = fs::metadata(&f) {
                if fs::remove_file(&f).is_ok() {
                    files += 1;
                    bytes += m.len();
                }
            }
        } else if cache_dir.join(name).exists() {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let _ = fs::write(&manifest, kept);
    (files, bytes)
}

/// Delete cached thumbnails, oldest first, until the cache fits `max_bytes`.
pub fn trim_cache(cache_dir: &Path, max_bytes: u64) -> u64 {
    let Ok(rd) = fs::read_dir(cache_dir) else { return 0 };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort();
    let mut freed = 0;
    for (_, len, path) in files {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(path).is_ok() {
            total -= len;
            freed += len;
        }
    }
    freed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_told_apart() {
        assert_eq!(kind_of(Path::new("a.JPG")), Some(Kind::Image));
        assert_eq!(kind_of(Path::new("a.pdf")), Some(Kind::Document));
        assert_eq!(kind_of(Path::new("a.MKV")), Some(Kind::Video));
        assert_eq!(kind_of(Path::new("a.txt")), None);
        assert_eq!(kind_of(Path::new("noext")), None);
        assert!(supported_extensions().contains(&"png"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn renders_a_video_frame_when_ffmpeg_is_installed() {
        if !have("ffmpeg") {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let video = d.path().join("clip.mp4");
        let made = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=duration=3:size=320x180:rate=10", "-pix_fmt", "yuv420p"])
            .arg(&video)
            .status();
        if !made.map(|s| s.success()).unwrap_or(false) {
            return; // this ffmpeg cannot encode H.264 here
        }
        assert!(is_supported(&video));
        let bytes = thumbnail_cached(&video, 96, None).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert!(img.width() <= 96 && img.height() <= 96 && img.width() >= 90, "{}x{}", img.width(), img.height());
        assert!(img.width() > img.height(), "keeps the 16:9 shape");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn renders_the_first_page_of_a_pdf_when_pdftoppm_is_installed() {
        if !have("pdftoppm") {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let pdf = d.path().join("a.pdf");
        // The smallest valid one-page PDF.
        let body = "%PDF-1.1\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 300]>>endobj\n";
        let xref_at = body.len();
        let pdf_text = format!("{body}xref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \ntrailer<</Size 4/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n");
        fs::write(&pdf, pdf_text).unwrap();
        let bytes = thumbnail_cached(&pdf, 100, None).unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert!(img.height() > img.width(), "a portrait page: {}x{}", img.width(), img.height());
        assert!(img.height() <= 100);
    }

    #[test]
    fn unsupported_files_are_refused_not_attempted() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("notes.txt");
        fs::write(&f, "hi").unwrap();
        assert!(thumbnail_cached(&f, 64, None).is_err());
    }

    #[test]
    fn clears_the_thumbnails_of_one_folder_only() {
        let d = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        for dir in ["a", "a/sub", "ab", "b"] {
            fs::create_dir_all(d.path().join(dir)).unwrap();
            image::RgbImage::new(40, 40).save(d.path().join(dir).join("p.png")).unwrap();
            thumbnail(&d.path().join(dir).join("p.png"), 32, cache.path()).unwrap();
        }
        let count = || fs::read_dir(cache.path()).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "png")).count();
        assert_eq!(count(), 4);
        let (files, bytes) = clear_under(cache.path(), &d.path().join("a"));
        assert_eq!(files, 2, "a and a/sub");
        assert!(bytes > 0);
        assert_eq!(count(), 2, "ab and b stay");
        assert_eq!(clear_under(cache.path(), &d.path().join("a")).0, 0);
        assert_eq!(clear_under(cache.path(), d.path()).0, 2);
        assert_eq!(count(), 0);
    }

    #[test]
    fn makes_caches_and_trims_thumbnails() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("pic.png");
        let cache = d.path().join("cache");
        image::RgbImage::from_pixel(400, 200, image::Rgb([200, 30, 30])).save(&src).unwrap();

        let bytes = thumbnail(&src, 64, &cache).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (64, 32));
        let pngs = || fs::read_dir(&cache).unwrap().filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "png")).count();
        assert_eq!(pngs(), 1);
        // Second call is served from the cache.
        assert_eq!(thumbnail(&src, 64, &cache).unwrap(), bytes);

        assert!(thumbnail(&d.path().join("notes.txt"), 64, &cache).is_err());
        assert!(trim_cache(&cache, 0) > 0);
        assert_eq!(pngs(), 0);
    }
}
