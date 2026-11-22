//! Drives that a copy, move or delete is working on right now.
//!
//! A memory card or a network share answers one request at a time, quickly or not at all. While a transfer is
//! reading from or writing to one, the app's own background reading of it (thumbnails of the videos on it, folder
//! sizes, previews) competes with the transfer and can make either stall. Those wait while the drive is in this list.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static BUSY: Mutex<Vec<(u64, PathBuf)>> = Mutex::new(Vec::new());

/// The drives are free again when this is dropped.
pub struct Hold(u64);

impl Drop for Hold {
    fn drop(&mut self) {
        if let Ok(mut l) = BUSY.lock() {
            l.retain(|(id, _)| *id != self.0);
        }
    }
}

/// Mark these places (a drive's root, or a folder on a share) as in use by transfer `id`.
pub fn hold(id: u64, places: impl IntoIterator<Item = PathBuf>) -> Hold {
    if let Ok(mut l) = BUSY.lock() {
        l.extend(places.into_iter().map(|p| (id, p)));
    }
    Hold(id)
}

/// Is `path` on a drive a transfer is using?
pub fn is_busy(path: &Path) -> bool {
    BUSY.lock().map(|l| l.iter().any(|(_, p)| path.starts_with(p))).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_is_busy_only_while_a_transfer_holds_it() {
        let h = hold(990_001, [PathBuf::from("/mnt/card-test-a")]);
        assert!(is_busy(Path::new("/mnt/card-test-a/videos/clip.ts")));
        assert!(!is_busy(Path::new("/mnt/card-test-b/clip.ts")));
        drop(h);
        assert!(!is_busy(Path::new("/mnt/card-test-a/videos/clip.ts")));
    }
}
