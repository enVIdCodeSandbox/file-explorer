//! Shared application state. Everything here is safe to touch from any
//! thread; commands never hold a lock across slow work.

use fast_core::drives::Drive;
use fast_core::entry::FileEntry;
use fast_core::index::Index;
use fast_core::predict::Predictor;
use fast_core::resources::Budget;
use fast_core::session::Session;
use fast_core::settings::Settings;
use fast_core::history::History;
use fast_core::transfer::TransferHandle;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

pub struct Paths {
    pub settings: PathBuf,
    pub session: PathBuf,
    pub predictor: PathBuf,
    pub history: PathBuf,
    /// Disk-usage snapshots (one small file per scanned folder).
    pub usage: PathBuf,
    /// Working files for administrator helpers; private to the app.
    pub scratch: PathBuf,
    pub index: PathBuf,
    pub thumbs: PathBuf,
}

impl Paths {
    pub fn new(config: &Path, cache: &Path) -> Paths {
        Paths {
            settings: config.join("settings.json"),
            session: config.join("session.json"),
            predictor: config.join("predictor.json"),
            history: config.join("transfer-history.bin"),
            usage: cache.join("usage"),
            scratch: cache.join("elevated"),
            index: cache.join("index.bin"),
            thumbs: cache.join("thumbnails"),
        }
    }
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    /// "idle", "loading", "scanning" or "ready"
    pub state: String,
    pub entries: usize,
    pub memory_bytes: usize,
    pub built_at: i64,
    pub truncated: bool,
    pub roots: Vec<String>,
}

/// Small LRU of directory listings, validated against the folder's
/// modification time before use.
pub struct ListingCache {
    map: HashMap<PathBuf, CachedListing>,
    clock: u64,
    pub capacity: usize,
}

#[derive(Clone)]
pub struct CachedListing {
    pub entries: Arc<Vec<FileEntry>>,
    pub show_hidden: bool,
    stamp: u64,
}

impl ListingCache {
    pub fn new(capacity: usize) -> Self {
        ListingCache { map: HashMap::new(), clock: 0, capacity }
    }

    pub fn get(&mut self, path: &Path, show_hidden: bool) -> Option<Arc<Vec<FileEntry>>> {
        self.clock += 1;
        let clock = self.clock;
        let c = self.map.get_mut(path)?;
        if c.show_hidden != show_hidden {
            return None;
        }
        c.stamp = clock;
        Some(c.entries.clone())
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.map.contains_key(path)
    }

    pub fn put(&mut self, path: PathBuf, entries: Arc<Vec<FileEntry>>, show_hidden: bool) {
        if self.capacity == 0 {
            return;
        }
        self.clock += 1;
        self.map.insert(path, CachedListing { entries, show_hidden, stamp: self.clock });
        while self.map.len() > self.capacity {
            let Some(oldest) = self.map.iter().min_by_key(|(_, c)| c.stamp).map(|(p, _)| p.clone()) else { break };
            self.map.remove(&oldest);
        }
    }

    pub fn invalidate(&mut self, path: &Path) {
        self.map.remove(path);
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Rough memory held by the cached listings.
    pub fn memory_bytes(&self) -> usize {
        self.map
            .values()
            .map(|c| c.entries.iter().map(|e| std::mem::size_of_val(e) + e.name.len() + 32).sum::<usize>())
            .sum()
    }

    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        if capacity == 0 {
            self.map.clear();
        }
    }
}

pub struct AppState {
    pub paths: Paths,
    pub settings: RwLock<Settings>,
    pub session: Mutex<Session>,
    pub budget: RwLock<Budget>,
    pub index: RwLock<Arc<Index>>,
    pub index_status: Mutex<IndexStatus>,
    pub index_busy: AtomicBool,
    /// Result of the last live (non-indexed) scan, reused while the user
    /// keeps typing in the same search scope.
    pub adhoc: Mutex<Option<(PathBuf, std::time::Instant, Arc<Index>)>>,
    pub search_cancel: Mutex<Arc<AtomicBool>>,
    /// Stops the running folder-size job when another folder is opened.
    pub size_cancel: Mutex<Arc<AtomicBool>>,
    /// Folder sizes measured recently: path to (bytes, files, when).
    pub folder_sizes: Mutex<HashMap<PathBuf, (u64, u64, std::time::Instant)>>,
    pub transfers: Mutex<HashMap<u64, TransferHandle>>,
    pub next_id: AtomicU64,
    pub listings: Mutex<ListingCache>,
    pub predictor: Mutex<Predictor>,
    pub visits_since_save: AtomicU64,
    /// How past transfers went, for predicting future ones.
    pub history: Mutex<History>,
    /// Running deletes, by id; setting the flag cancels.
    pub deletes: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    /// Running administrator helpers, by id: the file whose existence asks
    /// the helper to stop (the app cannot signal a process with more rights).
    pub elevated: Mutex<HashMap<u64, PathBuf>>,
    /// Running disk-usage scans, by scanned path.
    pub analyses: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub drives: RwLock<Vec<Drive>>,
    pub thumb_permits: Arc<tokio::sync::Semaphore>,
    pub started: std::time::Instant,
}

pub type Shared = Arc<AppState>;
