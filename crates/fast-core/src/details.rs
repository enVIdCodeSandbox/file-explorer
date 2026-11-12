//! Rich properties for the details pane: more than Explorer shows, and
//! computed off the UI thread.

use crate::drives::{self, Drive};
use crate::index::{Index, ScanOptions, NO_PARENT};
use crate::{to_millis, CoreError, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnixInfo {
    /// e.g. `rwxr-xr-x`
    pub mode: String,
    /// e.g. `755`
    pub octal: String,
    pub uid: u32,
    pub gid: u32,
    pub inode: u64,
    pub hard_links: u64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub name: String,
    pub path: String,
    pub parent: Option<String>,
    /// "file", "folder" or "symlink"
    pub kind: String,
    pub size: u64,
    /// Space actually allocated on disk, where the OS reports it.
    pub size_on_disk: Option<u64>,
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    pub readonly: bool,
    pub hidden: bool,
    pub extension: Option<String>,
    pub mime: Option<String>,
    pub symlink_target: Option<String>,
    pub unix: Option<UnixInfo>,
    /// Windows attribute names (Archive, System, Compressed, ...).
    pub attributes: Vec<String>,
    /// Pixel dimensions for images.
    pub image: Option<(usize, usize)>,
    /// Direct children, for folders.
    pub item_count: Option<u64>,
    pub volume: Option<Drive>,
}

pub fn details(path: &Path, drives: &[Drive]) -> Result<Details> {
    let lmeta = fs::symlink_metadata(path).map_err(|e| CoreError::io(path, e))?;
    let is_symlink = lmeta.file_type().is_symlink();
    // Describe the target for symlinks when it resolves, else the link itself.
    let meta = if is_symlink { fs::metadata(path).unwrap_or_else(|_| lmeta.clone()) } else { lmeta };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let is_dir = meta.is_dir();
    let extension = if is_dir { None } else { path.extension().map(|e| e.to_string_lossy().to_lowercase()) };
    let mime = if is_dir { None } else { mime_guess::from_path(path).first().map(|m| m.to_string()) };
    let image = match &mime {
        Some(m) if m.starts_with("image/") => imagesize::size(path).ok().map(|s| (s.width, s.height)),
        _ => None,
    };
    let item_count = if is_dir { fs::read_dir(path).ok().map(|rd| rd.count() as u64) } else { None };

    Ok(Details {
        hidden: crate::entry::is_hidden(&name, Some(&meta)),
        name,
        path: path.to_string_lossy().into_owned(),
        parent: path.parent().map(|p| p.to_string_lossy().into_owned()),
        kind: if is_symlink { "symlink" } else if is_dir { "folder" } else { "file" }.into(),
        size: if is_dir { 0 } else { meta.len() },
        size_on_disk: size_on_disk(&meta),
        created: to_millis(meta.created()),
        modified: to_millis(meta.modified()),
        accessed: to_millis(meta.accessed()),
        readonly: meta.permissions().readonly(),
        extension,
        mime,
        symlink_target: if is_symlink {
            fs::read_link(path).ok().map(|t| t.to_string_lossy().into_owned())
        } else {
            None
        },
        unix: unix_info(&meta),
        attributes: attributes(&meta),
        image,
        item_count,
        volume: drives::volume_for(path, drives).cloned(),
    })
}

#[cfg(unix)]
fn size_on_disk(meta: &fs::Metadata) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.blocks() * 512)
}
#[cfg(not(unix))]
fn size_on_disk(_meta: &fs::Metadata) -> Option<u64> {
    None
}

#[cfg(unix)]
fn unix_info(meta: &fs::Metadata) -> Option<UnixInfo> {
    use std::os::unix::fs::MetadataExt;
    let bits = meta.mode() & 0o777;
    let mut mode = String::with_capacity(9);
    for (i, c) in "rwxrwxrwx".chars().enumerate() {
        mode.push(if bits & (0o400 >> i) != 0 { c } else { '-' });
    }
    Some(UnixInfo {
        mode,
        octal: format!("{bits:o}"),
        uid: meta.uid(),
        gid: meta.gid(),
        inode: meta.ino(),
        hard_links: meta.nlink(),
    })
}
#[cfg(not(unix))]
fn unix_info(_meta: &fs::Metadata) -> Option<UnixInfo> {
    None
}

#[cfg(windows)]
fn attributes(meta: &fs::Metadata) -> Vec<String> {
    use std::os::windows::fs::MetadataExt;
    const NAMES: &[(u32, &str)] = &[
        (0x1, "Read-only"), (0x2, "Hidden"), (0x4, "System"), (0x20, "Archive"),
        (0x100, "Temporary"), (0x200, "Sparse"), (0x400, "Reparse point"),
        (0x800, "Compressed"), (0x1000, "Offline"), (0x2000, "Not indexed"),
        (0x4000, "Encrypted"),
    ];
    let a = meta.file_attributes();
    NAMES.iter().filter(|(bit, _)| a & bit != 0).map(|(_, n)| n.to_string()).collect()
}
#[cfg(not(windows))]
fn attributes(_meta: &fs::Metadata) -> Vec<String> {
    Vec::new()
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TypeUsage {
    pub extension: String,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PathStat {
    pub path: String,
    pub value: i64,
}

/// Deep statistics for a folder: what Explorer's Properties dialog shows,
/// plus the breakdown people usually open another tool to get.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FolderStats {
    pub files: u64,
    pub folders: u64,
    pub total_bytes: u64,
    pub max_depth: u32,
    pub largest: Vec<PathStat>,
    pub by_type: Vec<TypeUsage>,
    pub newest: Option<PathStat>,
    pub oldest: Option<PathStat>,
    pub empty_folders: u64,
    /// False when the scan was cancelled or hit the entry cap.
    pub complete: bool,
}

pub fn folder_stats(path: &Path, threads: usize, cancel: Option<&AtomicBool>) -> FolderStats {
    let idx = Index::scan(
        &[path.to_path_buf()],
        &ScanOptions { threads, cancel, ..Default::default() },
    );
    stats_from_index(&idx, 0)
}

/// Compute stats for the subtree rooted at `root` in an existing index.
pub fn stats_from_index(idx: &Index, root: u32) -> FolderStats {
    let end = idx.subtree_end(root);
    let (mut files, mut folders, mut total) = (0u64, 0u64, 0u64);
    let mut by_ext: HashMap<String, (u64, u64)> = HashMap::new();
    let mut newest: Option<(u32, i64)> = None;
    let mut oldest: Option<(u32, i64)> = None;
    let mut largest: Vec<(u64, u32)> = Vec::new();
    // Depth per node relative to root; parents precede children.
    let mut depth = vec![0u32; (end - root) as usize];
    let mut child_count = vec![0u32; (end - root) as usize];
    let mut max_depth = 0;

    for id in root + 1..end {
        let n = idx.node(id);
        let rel = (id - root) as usize;
        if n.parent != NO_PARENT && n.parent >= root {
            let p = (n.parent - root) as usize;
            depth[rel] = depth[p] + 1;
            child_count[p] += 1;
        }
        max_depth = max_depth.max(depth[rel]);
        if n.is_dir() {
            folders += 1;
            continue;
        }
        files += 1;
        total += n.size;
        let name = idx.name(id);
        let ext = match name.rfind('.') {
            Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_lowercase(),
            _ => "(none)".to_string(),
        };
        let e = by_ext.entry(ext).or_default();
        e.0 += 1;
        e.1 += n.size;
        if n.mtime > 0 {
            if newest.map(|(_, t)| n.mtime > t).unwrap_or(true) {
                newest = Some((id, n.mtime));
            }
            if oldest.map(|(_, t)| n.mtime < t).unwrap_or(true) {
                oldest = Some((id, n.mtime));
            }
        }
        largest.push((n.size, id));
        if largest.len() > 4096 {
            largest.sort_unstable_by(|a, b| b.cmp(a));
            largest.truncate(10);
        }
    }
    largest.sort_unstable_by(|a, b| b.cmp(a));
    largest.truncate(10);

    let empty_folders = (root + 1..end)
        .filter(|&id| idx.node(id).is_dir() && child_count[(id - root) as usize] == 0)
        .count() as u64;

    let mut by_type: Vec<TypeUsage> = by_ext
        .into_iter()
        .map(|(extension, (count, bytes))| TypeUsage { extension, count, bytes })
        .collect();
    by_type.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(b.count.cmp(&a.count)));
    by_type.truncate(12);

    let stat = |(id, t): (u32, i64)| PathStat { path: idx.path_of(id).to_string_lossy().into_owned(), value: t * 1000 };
    FolderStats {
        files,
        folders,
        total_bytes: total,
        max_depth,
        largest: largest
            .into_iter()
            .map(|(size, id)| PathStat { path: idx.path_of(id).to_string_lossy().into_owned(), value: size as i64 })
            .collect(),
        by_type,
        newest: newest.map(stat),
        oldest: oldest.map(stat),
        empty_folders,
        complete: !idx.truncated,
    }
}

/// SHA-256 of a file, streamed. Returns `None` if cancelled.
pub fn sha256(path: &Path, cancel: Option<&AtomicBool>) -> Result<Option<String>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = fs::File::open(path).map_err(|e| CoreError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.map(|c| c.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(false) {
            return Ok(None);
        }
        let n = f.read(&mut buf).map_err(|e| CoreError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(Some(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::sample_tree;

    #[test]
    fn file_and_folder_details() {
        let d = sample_tree();
        let f = details(&d.path().join("docs/reports/q2-report.pdf"), &[]).unwrap();
        assert_eq!(f.kind, "file");
        assert_eq!(f.size, 5000);
        assert_eq!(f.extension.as_deref(), Some("pdf"));
        assert_eq!(f.mime.as_deref(), Some("application/pdf"));
        assert!(f.modified.is_some());
        #[cfg(unix)]
        assert_eq!(f.unix.as_ref().unwrap().mode.len(), 9);

        let dir = details(&d.path().join("docs"), &[]).unwrap();
        assert_eq!(dir.kind, "folder");
        assert_eq!(dir.item_count, Some(2));
        assert!(details(&d.path().join("nope"), &[]).is_err());
    }

    #[test]
    fn folder_statistics() {
        let d = sample_tree();
        fs::create_dir(d.path().join("hollow")).unwrap();
        let s = folder_stats(d.path(), 2, None);
        assert_eq!((s.files, s.folders, s.total_bytes), (5, 4, 28110));
        assert_eq!(s.max_depth, 3);
        assert_eq!(s.empty_folders, 1);
        assert!(s.largest[0].path.ends_with("Beach.JPG"));
        assert_eq!(s.largest[0].value, 20000);
        assert_eq!(s.by_type[0].extension, "jpg");
        let pdf = s.by_type.iter().find(|t| t.extension == "pdf").unwrap();
        assert_eq!((pdf.count, pdf.bytes), (2, 8000));
        assert!(s.complete);
    }

    #[test]
    fn hashes_files() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256(&f, None).unwrap().unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
