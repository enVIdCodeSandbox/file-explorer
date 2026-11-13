//! Thumbnail generation with an on-disk cache.
//!
//! The UI only asks for thumbnails of rows on or near the screen, and only
//! after a row has been visible for a moment, so fast scrolling never queues
//! work. Results are cached on disk keyed by path, size and modification
//! time, so each image is decoded once.

use crate::{CoreError, Result};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub const SUPPORTED: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp"];
/// Skip very large sources: decoding them costs more than the preview is worth.
/// Decoder memory ceiling; protects against decompression bombs.
const MAX_DECODE_BYTES: u64 = 384 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = 64 * 1024 * 1024;

pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .map(|e| SUPPORTED.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

fn cache_file(cache_dir: &Path, path: &Path, len: u64, mtime: i64, px: u32) -> PathBuf {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    len.hash(&mut h);
    mtime.hash(&mut h);
    px.hash(&mut h);
    cache_dir.join(format!("{:016x}.png", h.finish()))
}

/// PNG bytes of a thumbnail at most `px` pixels on its longest side.
pub fn thumbnail(path: &Path, px: u32, cache_dir: &Path) -> Result<Vec<u8>> {
    thumbnail_cached(path, px, Some(cache_dir))
}

/// Like [`thumbnail`], but with `None` nothing is read from or written to disk.
pub fn thumbnail_cached(path: &Path, px: u32, cache_dir: Option<&Path>) -> Result<Vec<u8>> {
    if !is_supported(path) {
        return Err(CoreError::Unsupported("thumbnail for this file type"));
    }
    let meta = fs::metadata(path).map_err(|e| CoreError::io(path, e))?;
    if meta.len() > MAX_SOURCE_BYTES {
        return Err(CoreError::Invalid("file too large for a thumbnail".into()));
    }
    let px = px.clamp(16, 512);
    let mtime = crate::to_millis(meta.modified()).unwrap_or(0);
    let cached = cache_dir.map(|d| cache_file(d, path, meta.len(), mtime, px));
    if let Some(Ok(bytes)) = cached.as_ref().map(fs::read) {
        return Ok(bytes);
    }

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
    let thumb = img.thumbnail(px, px);
    let mut bytes = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| CoreError::Invalid(format!("cannot encode thumbnail: {e}")))?;

    // Best effort: a failed cache write only costs a re-decode next time.
    if let (Some(dir), Some(file)) = (cache_dir, cached) {
        if fs::create_dir_all(dir).is_ok() {
            let _ = fs::write(&file, &bytes);
        }
    }
    Ok(bytes)
}

/// Delete cached thumbnails, oldest first, until the cache fits `max_bytes`.
pub fn trim_cache(cache_dir: &Path, max_bytes: u64) -> u64 {
    let Ok(rd) = fs::read_dir(cache_dir) else { return 0 };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
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
    fn makes_caches_and_trims_thumbnails() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("pic.png");
        let cache = d.path().join("cache");
        image::RgbImage::from_pixel(400, 200, image::Rgb([200, 30, 30])).save(&src).unwrap();

        let bytes = thumbnail(&src, 64, &cache).unwrap();
        let thumb = image::load_from_memory(&bytes).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (64, 32));
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 1);
        // Second call is served from the cache.
        assert_eq!(thumbnail(&src, 64, &cache).unwrap(), bytes);

        assert!(thumbnail(&d.path().join("notes.txt"), 64, &cache).is_err());
        assert!(trim_cache(&cache, 0) > 0);
        assert_eq!(fs::read_dir(&cache).unwrap().count(), 0);
    }
}
