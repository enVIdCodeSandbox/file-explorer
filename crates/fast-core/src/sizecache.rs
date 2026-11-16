//! Remembered folder sizes.
//!
//! Measuring a folder means walking everything inside it, so the answer is
//! kept on disk and shown at once the next time the folder is listed. The
//! caller decides when a remembered size is still current (`age`) and
//! measures again in the background when it is not.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct SizeEntry {
    pub bytes: u64,
    pub files: u64,
    /// Seconds since the Unix epoch when this was measured.
    pub at: u64,
}

#[derive(Default)]
pub struct SizeCache {
    map: HashMap<String, SizeEntry>,
    dirty: bool,
    /// The saved file is still being read in the background: nothing is written until it has been merged in.
    loading: bool,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    version: u32,
    entries: Vec<(String, SizeEntry)>,
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

impl SizeCache {
    /// Reads the saved file; a missing or damaged file is an empty cache.
    pub fn load(file: &Path) -> SizeCache {
        let entries = std::fs::read(file)
            .ok()
            .and_then(|b| serde_json::from_slice::<OnDisk>(&b).ok())
            .filter(|d| d.version == 1)
            .map(|d| d.entries)
            .unwrap_or_default();
        SizeCache { map: entries.into_iter().collect(), dirty: false, loading: false }
    }

    /// An empty cache that is waiting for `finish_loading`, so the app need not read a big file before it starts.
    pub fn loading() -> SizeCache {
        SizeCache { loading: true, ..Default::default() }
    }

    /// Merge what was read from the file; sizes measured in the meantime win over older ones.
    pub fn finish_loading(&mut self, loaded: SizeCache) {
        for (k, v) in loaded.map {
            self.map.entry(k).or_insert(v);
        }
        self.loading = false;
    }

    pub fn get(&self, path: &Path) -> Option<SizeEntry> {
        self.map.get(&key(path)).copied()
    }

    pub fn put(&mut self, path: &Path, bytes: u64, files: u64, now: u64) {
        let new = SizeEntry { bytes, files, at: now };
        if self.map.insert(key(path), new) != Some(new) {
            self.dirty = true;
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Rough memory held by the entries, for the cache list.
    pub fn memory_bytes(&self) -> usize {
        self.map.keys().map(|k| k.len() + 64).sum()
    }

    /// Forget everything remembered for `root` and the folders below it; how many entries went.
    pub fn remove_under(&mut self, root: &Path) -> usize {
        let rule = root.to_string_lossy().into_owned();
        let before = self.map.len();
        self.map.retain(|k, _| !crate::cachepolicy::is_under(Path::new(k), &rule));
        let gone = before - self.map.len();
        if gone > 0 {
            self.dirty = true;
        }
        gone
    }

    pub fn clear(&mut self) {
        if !self.map.is_empty() {
            self.dirty = true;
        }
        self.map.clear();
    }

    /// Keeps the `max` most recently measured entries.
    pub fn trim(&mut self, max: usize) {
        if self.map.len() <= max {
            return;
        }
        let mut by_age: Vec<(u64, String)> = self.map.iter().map(|(k, v)| (v.at, k.clone())).collect();
        by_age.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        for (_, k) in by_age.into_iter().skip(max) {
            self.map.remove(&k);
        }
        self.dirty = true;
    }

    /// Writes the file if anything changed since the last save.
    pub fn save(&mut self, file: &Path) -> crate::Result<()> {
        if !self.dirty || self.loading {
            return Ok(());
        }
        if self.map.is_empty() {
            let _ = std::fs::remove_file(file);
        } else {
            let mut entries: Vec<_> = self.map.iter().map(|(k, v)| (k.clone(), *v)).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            crate::settings::write_json(file, &OnDisk { version: 1, entries })?;
        }
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn forgets_one_location_only() {
        let mut c = super::SizeCache::default();
        for p in ["/d/a", "/d/a/b", "/d/ab", "/e/x"] {
            c.put(std::path::Path::new(p), 1, 1, 1);
        }
        assert_eq!(c.remove_under(std::path::Path::new("/d/a")), 2);
        assert!(c.get(std::path::Path::new("/d/ab")).is_some(), "a sibling stays");
        assert!(c.get(std::path::Path::new("/e/x")).is_some());
        assert_eq!(c.len(), 2);
    }

    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fast-sizecache-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn survives_a_restart() {
        let dir = tmp("restart");
        let file = dir.join("sizes.json");
        let mut c = SizeCache::load(&file);
        c.put(Path::new("/a/b"), 10, 2, 100);
        c.save(&file).unwrap();
        let again = SizeCache::load(&file);
        assert_eq!(again.get(Path::new("/a/b")), Some(SizeEntry { bytes: 10, files: 2, at: 100 }));
        assert_eq!(again.get(Path::new("/a/c")), None);
    }

    #[test]
    fn saving_without_changes_writes_nothing() {
        let dir = tmp("clean");
        let file = dir.join("sizes.json");
        let mut c = SizeCache::load(&file);
        c.save(&file).unwrap();
        assert!(!file.exists());
        c.put(Path::new("/x"), 1, 1, 1);
        c.save(&file).unwrap();
        assert!(file.exists());
        let before = std::fs::metadata(&file).unwrap().modified().unwrap();
        c.put(Path::new("/x"), 1, 1, 1); // same value
        c.save(&file).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn clearing_removes_the_file() {
        let dir = tmp("clear");
        let file = dir.join("sizes.json");
        let mut c = SizeCache::load(&file);
        c.put(Path::new("/x"), 1, 1, 1);
        c.save(&file).unwrap();
        c.clear();
        c.save(&file).unwrap();
        assert!(!file.exists());
        assert!(c.is_empty());
    }

    #[test]
    fn trim_keeps_the_newest() {
        let mut c = SizeCache::default();
        for i in 0..10u64 {
            c.put(Path::new(&format!("/f{i}")), i, 1, i);
        }
        c.trim(3);
        assert_eq!(c.len(), 3);
        assert!(c.get(Path::new("/f9")).is_some() && c.get(Path::new("/f7")).is_some());
        assert!(c.get(Path::new("/f0")).is_none());
    }

    #[test]
    fn damaged_file_is_an_empty_cache() {
        let dir = tmp("bad");
        let file = dir.join("sizes.json");
        std::fs::write(&file, b"{not json").unwrap();
        assert!(SizeCache::load(&file).is_empty());
    }

    #[test]
    fn a_cache_still_loading_writes_nothing_and_keeps_newer_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sizes.json");
        let mut old = super::SizeCache::default();
        old.put(std::path::Path::new("/a"), 1, 1, 1);
        old.put(std::path::Path::new("/b"), 2, 2, 2);
        old.save(&file).unwrap();

        let mut c = super::SizeCache::loading();
        c.put(std::path::Path::new("/a"), 99, 9, 50);
        c.save(&file).unwrap();
        assert!(super::SizeCache::load(&file).get(std::path::Path::new("/b")).is_some(), "the saved file was not overwritten while loading");
        c.finish_loading(super::SizeCache::load(&file));
        assert_eq!(c.get(std::path::Path::new("/a")).unwrap().bytes, 99);
        assert_eq!(c.get(std::path::Path::new("/b")).unwrap().bytes, 2);
        c.save(&file).unwrap();
        assert_eq!(super::SizeCache::load(&file).get(std::path::Path::new("/a")).unwrap().bytes, 99);
    }
}
