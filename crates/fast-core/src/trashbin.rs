//! The Recycle Bin (Windows) and the Trash (Linux, macOS) as a list the app can show, with
//! restore, delete-for-good and empty.
//!
//! Windows and Linux read it through the system's own trash (so every drive's bin is included and
//! items can be put back where they were). macOS has no way to list or restore from the Trash
//! programmatically, so its Trash folder is read directly and items cannot be put back from here.

use crate::error::{CoreError, Result};
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BinItem {
    /// What the system calls the item; used to ask for it again.
    pub id: String,
    pub name: String,
    /// Where it was before it was deleted, when that is known.
    pub original_parent: Option<String>,
    /// Unix seconds.
    pub deleted: Option<i64>,
    pub size: Option<u64>,
    /// For a folder, how many things it holds, when that is known.
    pub entries: Option<u64>,
    pub is_dir: bool,
    /// Where the item really is on disk, when that is a place that can be opened.
    pub path: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Bin {
    pub items: Vec<BinItem>,
    /// Items can be put back where they came from.
    pub can_restore: bool,
    /// The name people know it by on this system.
    pub label: &'static str,
}

/// What a Windows `$I` file in a Recycle Bin says about the item it describes.
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq)]
struct RecycleRecord {
    original: String,
    size: u64,
    /// Unix seconds.
    deleted: i64,
}

/// Reads a `$I` file: version 1 (Vista to 8) has a fixed 520-byte name, version 2 (10 and later) a length first.
#[cfg(any(windows, test))]
fn parse_recycle_record(b: &[u8]) -> Option<RecycleRecord> {
    let u64_at = |at: usize| b.get(at..at + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap()));
    let version = u64_at(0)?;
    let size = u64_at(8)?;
    let filetime = u64_at(16)?;
    let name = match version {
        1 => b.get(24..24 + 520)?,
        2 => {
            let chars = u32::from_le_bytes(b.get(24..28)?.try_into().ok()?) as usize;
            b.get(28..28 + chars.checked_mul(2)?)?
        }
        _ => return None,
    };
    let units: Vec<u16> = name.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|u| *u != 0).collect();
    let original = String::from_utf16(&units).ok().filter(|s| !s.is_empty())?;
    // FILETIME counts 100 ns from 1601; Unix time from 1970 (11644473600 s later).
    let deleted = (filetime / 10_000_000) as i64 - 11_644_473_600;
    Some(RecycleRecord { original, size, deleted })
}

/// Which `$R` item holds the deleted thing: the one whose record names the same original path, nearest in time.
#[cfg(any(windows, test))]
fn match_record<'a, T>(records: &'a [(RecycleRecord, T)], parent: &str, name: &str, deleted: i64) -> Option<&'a T> {
    let want = format!("{}\\{}", parent.trim_end_matches(['\\', '/']), name).to_lowercase().replace('/', "\\");
    records
        .iter()
        .filter(|(r, _)| r.original.to_lowercase().replace('/', "\\") == want)
        .min_by_key(|(r, _)| (r.deleted - deleted).abs())
        .filter(|(r, _)| (r.deleted - deleted).abs() <= 5)
        .map(|(_, t)| t)
}

/// The newest first.
fn newest_first(items: &mut [BinItem]) {
    items.sort_by(|a, b| b.deleted.cmp(&a.deleted).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

/// More than this many items are listed without sizes: asking the system for each one takes too long.
const SIZE_LIMIT: usize = 3000;

#[cfg(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
mod imp {
    use super::*;
    use std::collections::HashSet;
    use trash::os_limited;
    use trash::TrashItemSize;

    pub const LABEL: &str = if cfg!(windows) { "Recycle Bin" } else { "Trash" };

    /// A freedesktop trash keeps `info/<n>.trashinfo` beside `files/<n>`; Windows hands over the item's own path.
    fn real_path(id: &std::ffi::OsStr) -> Option<std::path::PathBuf> {
        let id = std::path::Path::new(id);
        if cfg!(windows) {
            return id.exists().then(|| id.to_path_buf());
        }
        let stem = id.file_name()?.to_str()?.strip_suffix(".trashinfo")?;
        let file = id.parent()?.parent()?.join("files").join(stem);
        file.symlink_metadata().is_ok().then_some(file)
    }

    /// Every `$I` record in the bins this person can read, with the `$R` item it describes.
    #[cfg(windows)]
    fn windows_records() -> Vec<(RecycleRecord, std::path::PathBuf)> {
        let mut out = Vec::new();
        for letter in b'A'..=b'Z' {
            let bin = std::path::PathBuf::from(format!("{}:\\$Recycle.Bin", letter as char));
            let Ok(sids) = std::fs::read_dir(&bin) else { continue };
            for sid in sids.flatten() {
                let Ok(files) = std::fs::read_dir(sid.path()) else { continue };
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    let Some(rest) = name.strip_prefix("$I") else { continue };
                    let data = sid.path().join(format!("$R{rest}"));
                    if data.symlink_metadata().is_err() {
                        continue;
                    }
                    if let Some(r) = std::fs::read(f.path()).ok().and_then(|b| parse_recycle_record(&b)) {
                        out.push((r, data));
                    }
                }
            }
        }
        out
    }

    pub fn list() -> Result<Vec<BinItem>> {
        #[cfg(windows)]
        let records = windows_records();
        let items = os_limited::list().map_err(|e| CoreError::Invalid(format!("could not read the {}: {e}", LABEL)))?;
        let sizes = items.len() <= SIZE_LIMIT;
        let mut out: Vec<BinItem> = items
            .iter()
            .map(|i| {
                let (size, entries, is_dir) = match sizes.then(|| os_limited::metadata(i).ok()).flatten().map(|m| m.size) {
                    Some(TrashItemSize::Bytes(b)) => (Some(b), None, false),
                    Some(TrashItemSize::Entries(n)) => (None, Some(n as u64), true),
                    None => (None, None, false),
                };
                let path = real_path(&i.id);
                // When the system's name for an item is not where it is kept, the bin's own records say where it is.
                #[cfg(windows)]
                let path = path.or_else(|| match_record(&records, &i.original_parent.to_string_lossy(), &i.name.to_string_lossy(), i.time_deleted).cloned());
                let is_dir = is_dir || path.as_ref().is_some_and(|p| p.is_dir());
                BinItem {
                    id: i.id.to_string_lossy().into_owned(),
                    name: i.name.to_string_lossy().into_owned(),
                    original_parent: Some(i.original_parent.to_string_lossy().into_owned()).filter(|p| !p.is_empty()),
                    deleted: Some(i.time_deleted),
                    size,
                    entries,
                    is_dir,
                    path: path.map(|p| p.to_string_lossy().into_owned()),
                }
            })
            .collect();
        newest_first(&mut out);
        Ok(out)
    }

    fn chosen(ids: &[String]) -> Result<Vec<trash::TrashItem>> {
        let want: HashSet<&str> = ids.iter().map(String::as_str).collect();
        Ok(os_limited::list()
            .map_err(|e| CoreError::Invalid(format!("could not read the {}: {e}", LABEL)))?
            .into_iter()
            .filter(|i| want.contains(i.id.to_string_lossy().as_ref()))
            .collect())
    }

    pub fn restore(ids: &[String]) -> Result<usize> {
        let items = chosen(ids)?;
        let n = items.len();
        if n > 0 {
            os_limited::restore_all(items).map_err(|e| CoreError::Invalid(format!("could not put the items back: {e}")))?;
        }
        Ok(n)
    }

    pub fn purge(ids: &[String]) -> Result<usize> {
        let items = chosen(ids)?;
        let n = items.len();
        if n > 0 {
            os_limited::purge_all(items).map_err(|e| CoreError::Invalid(format!("could not delete the items: {e}")))?;
        }
        Ok(n)
    }

    pub fn empty() -> Result<usize> {
        let items = os_limited::list().map_err(|e| CoreError::Invalid(format!("could not read the {}: {e}", LABEL)))?;
        let n = items.len();
        if n > 0 {
            os_limited::purge_all(items).map_err(|e| CoreError::Invalid(format!("could not empty the {}: {e}", LABEL)))?;
        }
        Ok(n)
    }

    pub const CAN_RESTORE: bool = true;
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))))]
mod imp {
    use super::*;
    use std::path::PathBuf;

    pub const LABEL: &str = "Trash";
    pub const CAN_RESTORE: bool = false;

    fn folder() -> Option<PathBuf> {
        if cfg!(any(target_os = "ios", target_os = "android")) {
            return None;
        }
        crate::platform::home_dir().map(|h| h.join(".Trash"))
    }

    pub fn list() -> Result<Vec<BinItem>> {
        let Some(dir) = folder() else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&dir) else { return Ok(out) };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".DS_Store" {
                continue;
            }
            let meta = e.metadata().ok();
            let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
            out.push(BinItem {
                id: e.path().to_string_lossy().into_owned(),
                name,
                original_parent: None,
                deleted: meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
                size: meta.as_ref().filter(|m| !m.is_dir()).map(|m| m.len()),
                entries: None,
                is_dir,
                path: Some(e.path().to_string_lossy().into_owned()),
            });
        }
        newest_first(&mut out);
        Ok(out)
    }

    /// Only things inside the Trash folder may be removed through this.
    fn inside(id: &str) -> Option<PathBuf> {
        let dir = folder()?;
        let p = PathBuf::from(id);
        (p.parent() == Some(dir.as_path())).then_some(p)
    }

    pub fn restore(_ids: &[String]) -> Result<usize> {
        Err(CoreError::Invalid("Items cannot be put back from the Trash from here on this system. Drag them out of it, or use Put Back in Finder.".into()))
    }

    pub fn purge(ids: &[String]) -> Result<usize> {
        let mut n = 0;
        for id in ids {
            let Some(p) = inside(id) else { continue };
            let done = if p.is_dir() && !p.is_symlink() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            done.map_err(|e| CoreError::io(&p, e))?;
            n += 1;
        }
        Ok(n)
    }

    pub fn empty() -> Result<usize> {
        let ids: Vec<String> = list()?.into_iter().map(|i| i.id).collect();
        purge(&ids)
    }
}

/// Everything in the bin, newest first.
pub fn list() -> Result<Bin> {
    Ok(Bin { items: imp::list()?, can_restore: imp::CAN_RESTORE, label: imp::LABEL })
}

/// Put items back where they were deleted from; says how many were found.
pub fn restore(ids: &[String]) -> Result<usize> {
    imp::restore(ids)
}

/// Delete items for good; says how many were found.
pub fn purge(ids: &[String]) -> Result<usize> {
    imp::purge(ids)
}

/// Delete everything in the bin for good.
pub fn empty() -> Result<usize> {
    imp::empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_comes_first_then_by_name() {
        let item = |name: &str, t: i64| BinItem { id: name.into(), name: name.into(), original_parent: None, deleted: Some(t), size: None, entries: None, is_dir: false, path: None };
        let mut v = vec![item("b", 1), item("a", 5), item("C", 5)];
        newest_first(&mut v);
        assert_eq!(v.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["a", "C", "b"]);
    }


    #[test]
    fn a_recycle_record_of_either_version_is_read() {
        let name: Vec<u16> = "C:\\Users\\me\\Videos\\clip.mp4".encode_utf16().collect();
        let filetime = (1_700_000_000u64 + 11_644_473_600) * 10_000_000;
        let mut v1 = Vec::new();
        v1.extend(1u64.to_le_bytes());
        v1.extend(4096u64.to_le_bytes());
        v1.extend(filetime.to_le_bytes());
        let mut field = vec![0u8; 520];
        for (i, u) in name.iter().enumerate() {
            field[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
        }
        v1.extend(&field);
        let want = RecycleRecord { original: "C:\\Users\\me\\Videos\\clip.mp4".into(), size: 4096, deleted: 1_700_000_000 };
        assert_eq!(parse_recycle_record(&v1), Some(want));

        let mut v2 = Vec::new();
        v2.extend(2u64.to_le_bytes());
        v2.extend(4096u64.to_le_bytes());
        v2.extend(filetime.to_le_bytes());
        v2.extend(((name.len() + 1) as u32).to_le_bytes());
        for u in name.iter().chain(std::iter::once(&0u16)) {
            v2.extend(u.to_le_bytes());
        }
        assert_eq!(parse_recycle_record(&v2).unwrap().original, "C:\\Users\\me\\Videos\\clip.mp4");
        // Damaged or unknown ones are skipped, not guessed at.
        assert!(parse_recycle_record(&v2[..20]).is_none());
        let mut v3 = v2.clone();
        v3[0] = 9;
        assert!(parse_recycle_record(&v3).is_none());
    }

    #[test]
    fn an_item_is_matched_to_its_record_by_path_and_time() {
        let rec = |p: &str, t: i64| RecycleRecord { original: p.into(), size: 1, deleted: t };
        let records = vec![
            (rec("C:\\Users\\me\\a.txt", 100), "old"),
            (rec("C:\\Users\\me\\a.txt", 5000), "new"),
            (rec("C:\\Users\\me\\b.txt", 5000), "other"),
        ];
        assert_eq!(match_record(&records, "c:/users/me/", "a.txt", 5001), Some(&"new"));
        assert_eq!(match_record(&records, "C:\\Users\\me", "a.txt", 99), Some(&"old"));
        assert_eq!(match_record(&records, "C:\\Users\\me", "a.txt", 3000), None, "nothing close in time");
        assert_eq!(match_record(&records, "C:\\Users\\you", "a.txt", 5000), None);
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_linux_trash_is_listed_and_emptied_for_real() {
        // Uses the real trash of this user, but only touches what the test put there itself.
        let dir = tempfile::tempdir_in(std::env::var("HOME").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir())).unwrap();
        let f = dir.path().join("fast-trashbin-test-file.txt");
        std::fs::write(&f, b"hello").unwrap();
        if crate::platform::trash(&[f.clone()]).is_err() {
            return; // no usable trash here
        }
        let bin = list().unwrap();
        let mine: Vec<_> = bin.items.iter().filter(|i| i.name == "fast-trashbin-test-file.txt").collect();
        assert!(!mine.is_empty(), "the trashed file is listed");
        let item = mine[0].clone();
        assert_eq!(item.original_parent.as_deref(), dir.path().to_str());
        assert_eq!(item.size, Some(5));
        assert!(item.path.as_deref().is_some_and(|p| std::path::Path::new(p).exists()), "{item:?}");
        // Put back.
        assert_eq!(restore(&[item.id.clone()]).unwrap(), 1);
        assert_eq!(std::fs::read(&f).unwrap(), b"hello");
        // Trash again and delete for good.
        crate::platform::trash(&[f.clone()]).unwrap();
        let id = list().unwrap().items.into_iter().find(|i| i.name == "fast-trashbin-test-file.txt").unwrap().id;
        assert_eq!(purge(&[id]).unwrap(), 1);
        assert!(!f.exists());
    }
}
