//! Directory listing. This is the hottest path in the app, so it does the
//! minimum work per entry and fans metadata calls out across threads for
//! large folders.

use crate::{to_millis, CoreError, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub is_hidden: bool,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: Option<i64>,
    pub created: Option<i64>,
}

/// Folders above this size have their metadata fetched in parallel.
const PARALLEL_THRESHOLD: usize = 512;

pub fn list_dir(path: &Path, show_hidden: bool) -> Result<Vec<FileEntry>> {
    let rd = fs::read_dir(path).map_err(|e| CoreError::io(path, e))?;
    let raw: Vec<fs::DirEntry> = rd.filter_map(|e| e.ok()).collect();

    let convert = |e: &fs::DirEntry| -> Option<FileEntry> {
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type().ok()?;
        let is_symlink = ft.is_symlink();
        // For symlinks, follow to learn whether the target is a directory.
        let meta = if is_symlink {
            fs::metadata(e.path()).or_else(|_| e.metadata()).ok()
        } else {
            e.metadata().ok()
        };
        let is_hidden = is_hidden(&name, meta.as_ref());
        if is_hidden && !show_hidden {
            return None;
        }
        let (is_dir, size, modified, created) = match &meta {
            Some(m) => (
                m.is_dir(),
                if m.is_dir() { 0 } else { m.len() },
                to_millis(m.modified()),
                to_millis(m.created()),
            ),
            None => (ft.is_dir(), 0, None, None),
        };
        Some(FileEntry { name, is_dir, is_symlink, is_hidden, size, modified, created })
    };

    let entries = if raw.len() >= PARALLEL_THRESHOLD {
        raw.par_iter().filter_map(convert).collect()
    } else {
        raw.iter().filter_map(convert).collect()
    };
    Ok(entries)
}

#[cfg(windows)]
pub(crate) fn is_hidden(name: &str, meta: Option<&fs::Metadata>) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    let _ = name;
    meta.map(|m| m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0).unwrap_or(false)
}

#[cfg(not(windows))]
pub(crate) fn is_hidden(name: &str, _meta: Option<&fs::Metadata>) -> bool {
    name.starts_with('.')
}

/// Cheap fingerprint of a listing, used to decide whether a cached listing
/// shown to the user needs replacing after a background refresh.
pub fn fingerprint(entries: &[FileEntry]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut total: u64 = entries.len() as u64;
    for e in entries {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        e.name.hash(&mut h);
        e.size.hash(&mut h);
        e.modified.hash(&mut h);
        // Order-independent combine: listings come back in arbitrary order.
        total = total.wrapping_add(h.finish());
    }
    total
}

pub fn create_dir(path: &Path) -> Result<()> {
    fs::create_dir(path).map_err(|e| CoreError::io(path, e))
}

pub fn create_file(path: &Path) -> Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|_| ())
        .map_err(|e| CoreError::io(path, e))
}

pub fn rename(path: &Path, new_name: &str) -> Result<std::path::PathBuf> {
    if new_name.is_empty() || new_name.contains(['/', '\\']) {
        return Err(CoreError::Invalid(format!("invalid name: {new_name:?}")));
    }
    let target = path
        .parent()
        .ok_or_else(|| CoreError::Invalid("cannot rename a root".into()))?
        .join(new_name);
    if target.exists() {
        return Err(CoreError::Invalid(format!("{new_name} already exists")));
    }
    fs::rename(path, &target).map_err(|e| CoreError::io(path, e))?;
    Ok(target)
}

/// Permanently delete a file or a whole directory tree.
pub fn delete_permanently(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| CoreError::io(path, e))?;
    let r = if meta.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
    r.map_err(|e| CoreError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_filters_hidden() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("a.txt"), b"hello").unwrap();
        fs::create_dir(d.path().join("sub")).unwrap();
        fs::write(d.path().join(".secret"), b"x").unwrap();
        let all = list_dir(d.path(), true).unwrap();
        assert_eq!(all.len(), 3);
        let a = all.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!(a.size, 5);
        assert!(!a.is_dir);
        assert!(all.iter().find(|e| e.name == "sub").unwrap().is_dir);
        #[cfg(not(windows))]
        assert_eq!(list_dir(d.path(), false).unwrap().len(), 2);
    }

    #[test]
    fn rename_rejects_separators_and_collisions() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        fs::write(&a, b"").unwrap();
        fs::write(d.path().join("b"), b"").unwrap();
        assert!(rename(&a, "x/y").is_err());
        assert!(rename(&a, "b").is_err());
        assert!(rename(&a, "c").unwrap().ends_with("c"));
    }

    #[test]
    fn fingerprint_is_order_independent() {
        let e = |n: &str| FileEntry {
            name: n.into(), is_dir: false, is_symlink: false, is_hidden: false,
            size: 1, modified: Some(1), created: None,
        };
        assert_eq!(fingerprint(&[e("a"), e("b")]), fingerprint(&[e("b"), e("a")]));
        assert_ne!(fingerprint(&[e("a")]), fingerprint(&[e("b")]));
    }
}
