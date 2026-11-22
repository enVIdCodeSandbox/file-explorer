//! Shared application state. Everything here is safe to touch from any
//! thread; commands never hold a lock across slow work.

use fast_core::drives::Drive;
use fast_core::entry::FileEntry;
use fast_core::indexset::IndexSet;
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

/// Where a running job waits for the person's answer to "this file already exists".
#[derive(Default)]
pub struct JobAsk {
    /// Set by the `transfer_resolve` command; taken by the job.
    pub answer: Mutex<Option<(fast_core::transfer::Resolution, bool)>>,
}

pub struct Paths {
    pub settings: PathBuf,
    pub session: PathBuf,
    pub predictor: PathBuf,
    pub history: PathBuf,
    /// Remembered folder sizes.
    pub sizes: PathBuf,
    /// The last hardware report, so About opens instantly.
    pub hardware: PathBuf,
    /// Hashed passcodes for locked folders and drives.
    pub locks: PathBuf,
    /// Disk-usage snapshots (one small file per scanned folder).
    pub usage: PathBuf,
    /// Working files for administrator helpers; private to the app.
    pub scratch: PathBuf,
    pub index: PathBuf,
    pub thumbs: PathBuf,
    /// Fingerprints of SFTP servers the person trusted.
    pub known_hosts: PathBuf,
    /// Saved server addresses (never passwords).
    pub servers: PathBuf,
    /// Files fetched from a server to open them.
    pub remote_tmp: PathBuf,
    /// Automated tasks, what they have done, and where the scheduler had got to.
    pub tasks: PathBuf,
    pub task_history: PathBuf,
    pub task_state: PathBuf,
    /// Downloaded builds waiting to be installed, and what was already announced.
    pub update_dir: PathBuf,
    pub update_state: PathBuf,
    /// The terminal host's address file, and what each terminal printed before the app closed.
    pub terminal: PathBuf,
    /// Crash reports (plain text) and the running trail.
    pub crash: PathBuf,
    /// Icon collections the person installed (one small file each); downloaded again on request.
    pub icon_packs: PathBuf,
}

impl Paths {
    pub fn new(config: &Path, cache: &Path) -> Paths {
        Paths {
            settings: config.join("settings.json"),
            session: config.join("session.json"),
            predictor: config.join("predictor.json"),
            history: config.join("transfer-history.bin"),
            sizes: cache.join("folder-sizes.json"),
            hardware: cache.join("hardware-report.json"),
            locks: config.join("locks.json"),
            usage: cache.join("usage"),
            scratch: cache.join("elevated"),
            index: cache.join("index"),
            thumbs: cache.join("thumbnails"),
            known_hosts: config.join("known_hosts"),
            servers: config.join("servers.json"),
            remote_tmp: cache.join("remote"),
            tasks: config.join("tasks.json"),
            task_history: config.join("task-history.jsonl"),
            task_state: config.join("task-state.json"),
            update_dir: cache.join("updates"),
            update_state: config.join("update-state.json"),
            terminal: config.join("terminal"),
            crash: config.join("crash-logs"),
            icon_packs: cache.join("icon-packs"),
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
    /// Drives whose index is ready, of the drives being indexed.
    pub drives_ready: usize,
    pub drives_total: usize,
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

    /// Forget the folder `root` and everything below it; how many listings went.
    pub fn remove_under(&mut self, root: &Path) -> usize {
        let rule = root.to_string_lossy().into_owned();
        let before = self.map.len();
        self.map.retain(|p, _| !fast_core::cachepolicy::is_under(p, &rule));
        before - self.map.len()
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
    /// 0 in use, 1 visible behind other windows, 2 minimized or hidden (see `resources::Attention`).
    pub attention: std::sync::atomic::AtomicU8,
    /// One index per drive, each swapped in as soon as it is ready.
    pub index: RwLock<Arc<IndexSet>>,
    pub index_status: Mutex<IndexStatus>,
    pub index_busy: AtomicBool,
    /// Folders that changed and whose index entries need re-reading, and when the last change came.
    pub index_pending: Mutex<(std::collections::HashSet<PathBuf>, Option<std::time::Instant>)>,
    pub index_wake: parking_lot::Condvar,
    pub search_cancel: Mutex<Arc<AtomicBool>>,
    /// Stops the running folder-size job when another folder is opened.
    pub size_cancel: Mutex<Arc<AtomicBool>>,
    /// Folder sizes measured recently: path to (bytes, files, when).
    pub folder_sizes: Mutex<fast_core::sizecache::SizeCache>,
    pub locks: RwLock<fast_core::locks::Locks>,
    /// Kept between calls so CPU use can be measured as a difference.
    pub monitor: Mutex<sysinfo::System>,
    pub transfers: Mutex<HashMap<u64, TransferHandle>>,
    pub next_id: AtomicU64,
    pub listings: Mutex<ListingCache>,
    pub predictor: Mutex<Predictor>,
    pub visits_since_save: AtomicU64,
    /// How past transfers went, for predicting future ones.
    pub history: Mutex<History>,
    /// Running deletes, by id; setting the flag cancels.
    pub deletes: Mutex<HashMap<u64, Arc<AtomicBool>>>,
    /// Jobs (other than transfers) that may stop to ask about a file that already exists, by id.
    pub job_asks: Mutex<HashMap<u64, Arc<JobAsk>>>,
    /// Running administrator helpers, by id: the file whose existence asks
    /// the helper to stop (the app cannot signal a process with more rights).
    pub elevated: Mutex<HashMap<u64, PathBuf>>,
    /// Running disk-usage scans, by scanned path.
    pub analyses: Mutex<HashMap<String, Arc<AtomicBool>>>,
    pub drives: RwLock<Vec<Drive>>,
    /// Phones, cameras and other connected devices that are not drives.
    pub devices: RwLock<Vec<fast_core::devices::Device>>,
    /// Servers the person logged in to this run, with open connections ready for reuse.
    pub remotes: Mutex<HashMap<String, crate::remote::Remote>>,
    /// Temporary download links that are currently being served.
    pub shares: fast_core::share::Shares,
    /// Tells the app when an open folder changed on disk.
    pub watcher: fast_core::watch::Watcher,
    /// The folders each window has open, by window label.
    pub watch_sets: Mutex<HashMap<String, Vec<PathBuf>>>,
    /// Which window started each job: only that window is asked "replace this file?".
    pub job_owners: Mutex<HashMap<u64, String>>,
    pub thumb_permits: Arc<tokio::sync::Semaphore>,
    /// Videos and documents render slowly, so they have their own lane and never hold up the pictures.
    pub slow_thumb_permits: Arc<tokio::sync::Semaphore>,
    /// Bumped each time a folder asks for its videos to be prepared; older runs stop.
    pub prewarm_generation: std::sync::atomic::AtomicU64,
    pub started: std::time::Instant,
    #[cfg(desktop)]
    pub terminal: crate::terminal::Bridge,
}

pub type Shared = Arc<AppState>;
