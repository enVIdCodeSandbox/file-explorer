//! Commands callable from the frontend. Every command that touches the disk
//! runs on a blocking worker thread, so the UI thread never waits on I/O.

use crate::state::{IndexStatus, Shared};
use fast_core::details::{self, Details, FolderStats};
use fast_core::drives::{self, Drive};
use fast_core::entry::{self, FileEntry};
use fast_core::index::{Index, ScanOptions};
use fast_core::platform::{self, TerminalMode};
use fast_core::resources::{self, Budget, SystemSnapshot};
use fast_core::search::{Hit, Query};
use fast_core::session::Session;
use fast_core::settings::Settings;
use fast_core::history::HistoryInfo;
use fast_core::ops::{self, AccessCheck, HelperReport, Job};
use fast_core::platform::Elevation;
use fast_core::transfer::{self, Phase, Predictor, Progress, TransferSpec};
use fast_core::treemap::Analysis;
use fast_core::usage::{self, ScanProgress, UsageOptions};
use fast_core::thumbs;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter, State};

pub type CmdResult<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn err<E: ToString>(e: E) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// Listing
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Arc<Vec<FileEntry>>,
    /// True when served from memory; a background refresh follows and a
    /// `dir-updated` event is emitted if the folder changed.
    pub cached: bool,
}

fn listing(path: &Path, entries: Arc<Vec<FileEntry>>, cached: bool) -> Listing {
    Listing {
        path: path.to_string_lossy().into_owned(),
        parent: path.parent().map(|p| p.to_string_lossy().into_owned()),
        entries,
        cached,
    }
}

#[tauri::command]
pub async fn list_dir(app: AppHandle, state: State<'_, Shared>, path: String) -> CmdResult<Listing> {
    let st = state.inner().clone();
    blocking(move || {
        let p = PathBuf::from(&path);
        let show_hidden = st.settings.read().general.show_hidden;
        let hit = st.listings.lock().get(&p, show_hidden);
        if let Some(entries) = hit {
            // Stale-while-revalidate: answer now, verify in the background.
            let (st2, p2, shown) = (st.clone(), p.clone(), entries.clone());
            std::thread::spawn(move || {
                let Ok(fresh) = entry::list_dir(&p2, show_hidden) else {
                    st2.listings.lock().invalidate(&p2);
                    return;
                };
                if entry::fingerprint(&fresh) != entry::fingerprint(&shown) {
                    let fresh = Arc::new(fresh);
                    st2.listings.lock().put(p2.clone(), fresh.clone(), show_hidden);
                    let _ = app.emit("dir-updated", listing(&p2, fresh, false));
                }
            });
            return Ok(listing(&p, entries, true));
        }
        let entries = Arc::new(entry::list_dir(&p, show_hidden).map_err(err)?);
        st.listings.lock().put(p.clone(), entries.clone(), show_hidden);
        Ok(listing(&p, entries, false))
    })
    .await
}

/// Warm the listing cache for folders the user is likely to open next.
#[tauri::command]
pub async fn prefetch(state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    if !st.budget.read().prefetch {
        return Ok(());
    }
    blocking(move || {
        prefetch_now(&st, paths);
        Ok(())
    })
    .await
}

fn prefetch_now(st: &Shared, paths: Vec<String>) {
    let show_hidden = st.settings.read().general.show_hidden;
    for path in paths.into_iter().take(8) {
        let p = PathBuf::from(path);
        if st.listings.lock().contains(&p) {
            continue;
        }
        if let Ok(entries) = entry::list_dir(&p, show_hidden) {
            // Huge folders are not worth holding speculatively.
            if entries.len() <= 20_000 {
                st.listings.lock().put(p, Arc::new(entries), show_hidden);
            }
        }
    }
}

/// Record a navigation and return the folders most likely to be opened next.
/// Their listings are warmed in the background.
#[tauri::command]
pub async fn record_visit(state: State<'_, Shared>, from: Option<String>, to: String) -> CmdResult<Vec<String>> {
    let st = state.inner().clone();
    blocking(move || {
        let now = now_secs();
        let predictions = {
            let mut p = st.predictor.lock();
            p.record(from.as_deref(), &to, now);
            p.predict(&to, 4, now)
        };
        if st.visits_since_save.fetch_add(1, Ordering::Relaxed) % 20 == 19 {
            let _ = st.predictor.lock().save(&st.paths.predictor);
        }
        if st.budget.read().prefetch {
            prefetch_now(&st, predictions.clone());
        }
        Ok(predictions)
    })
    .await
}

/// The folders opened most often (recent visits weigh more), skipping any
/// that no longer exist.
#[tauri::command]
pub async fn frequent_folders(state: State<'_, Shared>, limit: usize) -> CmdResult<Vec<String>> {
    let st = state.inner().clone();
    blocking(move || {
        let candidates = st.predictor.lock().frequent(limit.saturating_mul(2).max(limit), now_secs());
        Ok(candidates.into_iter().filter(|p| Path::new(p).is_dir()).take(limit).collect())
    })
    .await
}

// ---------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_details(state: State<'_, Shared>, path: String) -> CmdResult<Details> {
    let st = state.inner().clone();
    blocking(move || details::details(Path::new(&path), &st.drives.read()).map_err(err)).await
}

#[tauri::command]
pub async fn folder_stats(state: State<'_, Shared>, path: String) -> CmdResult<FolderStats> {
    let st = state.inner().clone();
    blocking(move || {
        let threads = st.budget.read().index_threads;
        Ok(details::folder_stats(Path::new(&path), threads, None))
    })
    .await
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FolderSizeEvent {
    dir: String,
    name: String,
    bytes: u64,
    files: u64,
    /// False when the measurement hit the scan limit, so the size is a minimum.
    complete: bool,
}

/// How long a measured folder size is trusted.
const FOLDER_SIZE_TTL_SECS: u64 = 120;

/// Measure the given subfolders of `dir` one after another in the background,
/// sending a `folder-size` event for each. Starting another job (opening a
/// different folder) cancels this one. Recent results are reused unless
/// `refresh` is set.
#[tauri::command]
pub async fn folder_sizes_start(app: AppHandle, state: State<'_, Shared>, dir: String, names: Vec<String>, refresh: bool) -> CmdResult<()> {
    let st = state.inner().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = st.size_cancel.lock();
        slot.store(true, Ordering::Relaxed);
        *slot = cancel.clone();
    }
    std::thread::spawn(move || {
        let threads = st.budget.read().index_threads.clamp(1, 4);
        for name in names {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let path = Path::new(&dir).join(&name);
            let cached = if refresh {
                None
            } else {
                st.folder_sizes
                    .lock()
                    .get(&path)
                    .filter(|(_, _, t)| t.elapsed().as_secs() < FOLDER_SIZE_TTL_SECS)
                    .map(|(b, f, _)| (*b, *f))
            };
            let (bytes, files, complete) = match cached {
                Some((b, f)) => (b, f, true),
                None => {
                    let stats = details::folder_stats(&path, threads, Some(&cancel));
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    if stats.complete {
                        let mut map = st.folder_sizes.lock();
                        if map.len() > 5000 {
                            map.clear();
                        }
                        map.insert(path.clone(), (stats.total_bytes, stats.files, Instant::now()));
                    }
                    (stats.total_bytes, stats.files, stats.complete)
                }
            };
            let _ = app.emit("folder-size", FolderSizeEvent { dir: dir.clone(), name, bytes, files, complete });
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn hash_file(path: String) -> CmdResult<String> {
    blocking(move || {
        details::sha256(Path::new(&path), None)
            .map_err(err)?
            .ok_or_else(|| "cancelled".to_string())
    })
    .await
}

#[tauri::command]
pub async fn thumbnail(state: State<'_, Shared>, path: String, size: u32) -> CmdResult<tauri::ipc::Response> {
    let st = state.inner().clone();
    if !st.budget.read().thumbnails {
        return Err("thumbnails are disabled".into());
    }
    // Bounded concurrency: decoding is CPU-heavy and must not starve listing.
    let _permit = st.thumb_permits.clone().acquire_owned().await.map_err(err)?;
    let bytes = blocking(move || thumbs::thumbnail(Path::new(&path), size, &st.paths.thumbs).map_err(err)).await?;
    Ok(tauri::ipc::Response::new(bytes))
}

// ---------------------------------------------------------------------------
// Drives and places
// ---------------------------------------------------------------------------

/// Query the OS for volumes. The frontend shows the cached list from the
/// session first, so this never delays first paint.
#[tauri::command]
pub async fn get_drives(state: State<'_, Shared>) -> CmdResult<Vec<Drive>> {
    let st = state.inner().clone();
    blocking(move || {
        let list = drives::list_drives();
        *st.drives.write() = list.clone();
        st.session.lock().drives = list.clone();
        Ok(list)
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    label: String,
    path: String,
}

#[tauri::command]
pub fn known_folders() -> Vec<Place> {
    platform::known_folders()
        .into_iter()
        .map(|(label, path)| Place { label: label.to_string(), path })
        .collect()
}

// ---------------------------------------------------------------------------
// Index and search
// ---------------------------------------------------------------------------

fn index_roots(st: &Shared) -> Vec<PathBuf> {
    if st.settings.read().performance.index_drives {
        let mounts: Vec<PathBuf> = st
            .drives
            .read()
            .iter()
            .filter(|d| d.online && d.media != drives::Media::Network)
            .map(|d| PathBuf::from(&d.mount))
            .collect();
        // Drop mounts nested inside another indexed mount.
        let roots: Vec<PathBuf> = mounts
            .iter()
            .filter(|m| !mounts.iter().any(|o| o != *m && m.starts_with(o)))
            .cloned()
            .collect();
        if !roots.is_empty() {
            return roots;
        }
    }
    platform::home_dir().into_iter().collect()
}

fn publish_index_status(app: &AppHandle, st: &Shared, state: &str) {
    let set = st.index.read().clone();
    let ready = set.parts().len();
    let status = IndexStatus {
        state: state.to_string(),
        entries: set.len(),
        memory_bytes: set.memory_bytes(),
        built_at: set.built_at(),
        truncated: set.truncated(),
        roots: set.root_names(),
        drives_ready: ready,
        drives_total: index_roots(st).len().max(ready),
    };
    *st.index_status.lock() = status.clone();
    let _ = app.emit("index-status", status);
}

/// Where one drive's saved index lives.
fn part_file(st: &Shared, root: &Path) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in root.to_string_lossy().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    st.paths.index.join(format!("{h:016x}.bin"))
}

/// Scan the drives and swap each one in as soon as it is done, so search can
/// use the first drive while the others are still being read. Drives are read
/// side by side (they are usually separate disks), within the thread budget.
/// Runs on the caller's thread, so call it from a background thread.
pub fn rebuild_index(app: &AppHandle, st: &Shared) {
    if st.index_busy.swap(true, Ordering::SeqCst) {
        return;
    }
    *st.drives.write() = drives::list_drives();
    let roots = index_roots(st);
    // A drive that went away takes its index with it.
    {
        let names: Vec<String> = roots.iter().map(|r| r.to_string_lossy().into_owned()).collect();
        let mut w = st.index.write();
        *w = Arc::new(w.retain_roots(&names));
    }
    publish_index_status(app, st, "scanning");
    let budget = st.budget.read().clone();
    let workers = budget.index_threads.max(1).min(roots.len().max(1)).min(4);
    let per_root_threads = (budget.index_threads / workers).max(1);
    let cap = budget.index_max_entries;
    let per_root_cap = if cap == 0 { 0 } else { (cap / roots.len().max(1)).max(cap / 3) };
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(root) = roots.get(i) else { break };
                let idx = Index::scan(
                    std::slice::from_ref(root),
                    &ScanOptions { threads: per_root_threads, max_entries: per_root_cap, ..Default::default() },
                );
                let _ = idx.save(&part_file(st, root));
                {
                    let mut w = st.index.write();
                    *w = Arc::new(w.with_part(Arc::new(idx)));
                }
                publish_index_status(app, st, "scanning");
            });
        }
    });
    st.index_busy.store(false, Ordering::SeqCst);
    publish_index_status(app, st, "ready");
}

/// How often the background index is refreshed while the app is open.
const REFRESH_SECS: u64 = 30 * 60;

/// Load the saved index of every drive at once (search works the moment the
/// window opens), then refresh in the background if the budget allows, and
/// again every half hour.
pub fn start_indexing(app: AppHandle, st: Shared) {
    std::thread::Builder::new()
        .name("indexer".into())
        .spawn(move || {
            publish_index_status(&app, &st, "loading");
            // Older versions kept one file for everything.
            if let Some(dir) = st.paths.index.parent() {
                let _ = std::fs::remove_file(dir.join("index.bin"));
            }
            *st.drives.write() = drives::list_drives();
            let roots = index_roots(&st);
            std::thread::scope(|scope| {
                for root in &roots {
                    scope.spawn(|| {
                        let Ok(idx) = Index::load(&part_file(&st, root)) else { return };
                        // Only trust a file that really is this drive's index.
                        if idx.roots.first().map(|&r| idx.name(r) == root.to_string_lossy()) != Some(true) {
                            return;
                        }
                        {
                            let mut w = st.index.write();
                            *w = Arc::new(w.with_part(Arc::new(idx)));
                        }
                        publish_index_status(&app, &st, "loading");
                    });
                }
            });
            let has_index = !st.index.read().is_empty();
            publish_index_status(&app, &st, if has_index { "ready" } else { "idle" });
            loop {
                if st.budget.read().background_indexing {
                    rebuild_index(&app, &st);
                }
                std::thread::sleep(std::time::Duration::from_secs(REFRESH_SECS));
            }
        })
        .expect("spawn indexer");
}

#[tauri::command]
pub fn index_status(state: State<'_, Shared>) -> IndexStatus {
    state.index_status.lock().clone()
}

#[tauri::command]
pub async fn reindex(app: AppHandle, state: State<'_, Shared>) -> CmdResult<()> {
    let st = state.inner().clone();
    std::thread::spawn(move || rebuild_index(&app, &st));
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    /// What the index knew straight away. More may follow as `search-hits` events.
    hits: Vec<Hit>,
    /// "index": answered from the cache. "live": some folders are being read now.
    source: &'static str,
    complete: bool,
    /// True while a live walk is still streaming results.
    searching: bool,
    elapsed_ms: f64,
    searched_entries: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SearchBatch {
    id: u64,
    hits: Vec<Hit>,
    scanned: usize,
    done: bool,
}

/// Search. Whatever the index covers is answered at once, from memory. Any
/// folder it does not cover yet (a drive still being indexed, a folder made a
/// moment ago) is walked live on another thread, and its matches are sent as
/// `search-hits` events as they are found, so nothing waits for a full scan.
#[tauri::command]
pub async fn search(app: AppHandle, state: State<'_, Shared>, query: String, scope: Option<String>, id: u64) -> CmdResult<SearchResponse> {
    let st = state.inner().clone();
    // Only the newest query matters: stop whatever was still running.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = st.search_cancel.lock();
        slot.store(true, Ordering::Relaxed);
        *slot = cancel.clone();
    }
    blocking(move || {
        let t0 = Instant::now();
        let q = Query::parse(&query);
        let (limit, include_hidden) = {
            let s = st.settings.read();
            (s.search.limit.clamp(10, 10_000), s.search.include_hidden || s.general.show_hidden)
        };
        let scope_path = scope.map(PathBuf::from);
        let set = st.index.read().clone();

        let (hits, live_roots): (Vec<Hit>, Vec<PathBuf>) = match &scope_path {
            Some(p) if set.covers(p) => (set.search(&q, Some(p), include_hidden, limit), vec![]),
            Some(p) => (vec![], vec![p.clone()]),
            None => {
                let pending = index_roots(&st).into_iter().filter(|r| !set.has_root_for(r)).collect();
                (set.search(&q, None, include_hidden, limit), pending)
            }
        };
        let mut response = SearchResponse {
            hits,
            source: "index",
            complete: !set.truncated(),
            searching: false,
            elapsed_ms: t0.elapsed().as_secs_f64() * 1000.0,
            searched_entries: set.len(),
        };
        if live_roots.is_empty() || q.is_empty() {
            return Ok(response);
        }
        response.source = "live";
        response.complete = false;
        response.searching = true;

        let threads = st.budget.read().io_threads.clamp(4, 16);
        std::thread::Builder::new()
            .name("live-search".into())
            .spawn(move || {
                let opts = fast_core::search::LiveOptions {
                    threads,
                    include_hidden,
                    cancel: &cancel,
                    skip: fast_core::index::default_skip_list(),
                };
                let mut base = 0usize;
                for root in &live_roots {
                    let seen = fast_core::search::live_search(root, &q, &opts, |hits, so_far| {
                        if !hits.is_empty() && !cancel.load(Ordering::Relaxed) {
                            let _ = app.emit("search-hits", SearchBatch { id, hits, scanned: base + so_far, done: false });
                        }
                    });
                    base += seen;
                }
                if !cancel.load(Ordering::Relaxed) {
                    let _ = app.emit("search-hits", SearchBatch { id, hits: Vec::new(), scanned: base, done: true });
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(response)
    })
    .await
}

// ---------------------------------------------------------------------------
// Disk usage
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    path: String,
    analysis: Analysis,
    /// Unix seconds when the numbers were measured.
    taken_at: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AnalysisProgress {
    path: String,
    entries: u64,
    bytes: u64,
    elapsed_secs: f64,
}

/// The last saved result for a folder, if any: tiny to read, so the view
/// can show it immediately while a fresh scan runs.
#[tauri::command]
pub async fn analysis_snapshot(state: State<'_, Shared>, path: String) -> CmdResult<Option<AnalysisResult>> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(usage::load_snapshot(&st.paths.usage, Path::new(&path))
            .map(|(analysis, taken_at)| AnalysisResult { path, analysis, taken_at }))
    })
    .await
}

/// Scan a folder in the background. Progress arrives as `analysis-progress`
/// events and the finished result as `analysis-done` (and is saved as the
/// new snapshot). Starting a scan of a path that is already being scanned
/// replaces the old scan.
#[tauri::command]
pub async fn analyze_start(app: AppHandle, state: State<'_, Shared>, path: String) -> CmdResult<()> {
    let st = state.inner().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(old) = st.analyses.lock().insert(path.clone(), cancel.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    let threads = st.budget.read().index_threads;
    std::thread::Builder::new()
        .name("disk-usage".into())
        .spawn(move || {
            let root = PathBuf::from(&path);
            let progress = ScanProgress::default();
            let finished = AtomicBool::new(false);
            let started = Instant::now();
            // A finished snapshot is kept while the scan runs; only without
            // one are the partial results saved as checkpoints, so closing
            // the app mid-scan still leaves something to show next time.
            let has_complete = usage::load_snapshot(&st.paths.usage, &root).is_some_and(|(a, _)| a.complete);
            let on_partial = |analysis: Analysis| {
                let taken_at = now_secs();
                if !has_complete {
                    let _ = usage::save_snapshot(&st.paths.usage, &root, &analysis, taken_at);
                }
                let _ = app.emit("analysis-partial", AnalysisResult { path: path.clone(), analysis, taken_at });
            };
            let result = std::thread::scope(|scope| {
                scope.spawn(|| {
                    while !finished.load(Ordering::Relaxed) {
                        let _ = app.emit(
                            "analysis-progress",
                            AnalysisProgress {
                                path: path.clone(),
                                entries: progress.entries.load(Ordering::Relaxed),
                                bytes: progress.bytes.load(Ordering::Relaxed),
                                elapsed_secs: started.elapsed().as_secs_f64(),
                            },
                        );
                        std::thread::sleep(std::time::Duration::from_millis(250));
                    }
                });
                let r = usage::scan(
                    &root,
                    &UsageOptions { threads, cancel: Some(&cancel), progress: Some(&progress), partial: Some(&on_partial), ..Default::default() },
                );
                finished.store(true, Ordering::Relaxed);
                r
            });
            match result {
                Some(analysis) => {
                    let taken_at = now_secs();
                    let _ = usage::save_snapshot(&st.paths.usage, &root, &analysis, taken_at);
                    usage::prune_snapshots(&st.paths.usage, 40, 8 * 1024 * 1024);
                    let _ = app.emit("analysis-done", AnalysisResult { path: path.clone(), analysis, taken_at });
                }
                // Cancelled because the person left or started it again: say nothing.
                None if cancel.load(Ordering::Relaxed) => {}
                None => {
                    let _ = app.emit("analysis-failed", (path.clone(), format!("cannot read {path}")));
                }
            }
            let mut running = st.analyses.lock();
            if running.get(&path).is_some_and(|c| Arc::ptr_eq(c, &cancel)) {
                running.remove(&path);
            }
        })
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn analyze_cancel(state: State<'_, Shared>, path: String) {
    if let Some(c) = state.analyses.lock().remove(&path) {
        c.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// File operations
// ---------------------------------------------------------------------------

fn fs_changed(app: &AppHandle, st: &Shared, dirs: Vec<PathBuf>) {
    {
        let mut cache = st.listings.lock();
        for d in &dirs {
            cache.invalidate(d);
        }
    }
    let dirs: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
    let _ = app.emit("fs-changed", dirs);
}

/// Names in `dest_dir` that the given sources would collide with.
#[tauri::command]
pub async fn check_conflicts(sources: Vec<String>, dest_dir: String) -> CmdResult<Vec<String>> {
    blocking(move || {
        let dest = PathBuf::from(dest_dir);
        Ok(sources
            .iter()
            .filter_map(|s| {
                let src = Path::new(s);
                let name = src.file_name()?;
                let target = dest.join(name);
                // Folder onto folder merges; that is not a conflict by itself.
                let clash = target.exists() && target != src && !(src.is_dir() && target.is_dir());
                clash.then(|| name.to_string_lossy().into_owned())
            })
            .collect())
    })
    .await
}

/// Remember how a finished transfer went, if the person allows it.
fn remember_transfer(st: &Shared, p: &Progress) {
    let (remember, limit) = {
        let s = st.settings.read();
        (s.transfer.remember_device_speeds, s.transfer.history_limit)
    };
    if !remember || limit == 0 {
        return;
    }
    let mut history = st.history.lock();
    if history.record(p, now_secs(), limit) {
        let _ = history.save(&st.paths.history);
    }
}

#[tauri::command]
pub async fn transfer_start(app: AppHandle, state: State<'_, Shared>, mut spec: TransferSpec) -> CmdResult<u64> {
    let st = state.inner().clone();
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let (mut budget, drive_list) = (st.budget.read().clone(), st.drives.read().clone());
    let (remember, limit, explore) = {
        let s = st.settings.read();
        // The progress rate follows the device (slower on laptops and weak
        // machines) unless it was set by hand.
        if s.transfer.progress_interval_ms > 0 {
            budget.progress_ms = s.transfer.progress_interval_ms.clamp(50, 2000);
        }
        spec.method = fast_core::transfer::method::MethodPref::from_setting(&s.transfer.method);
        spec.preserve_times = s.transfer.preserve_timestamps;
        spec.large_threshold = s.transfer.large_file_threshold_mb.max(1) * 1024 * 1024;
        spec.max_parallel = s.transfer.max_parallel;
        (s.transfer.remember_device_speeds, s.transfer.history_limit, s.transfer.try_other_methods)
    };
    // After the sources are scanned, the engine asks what similar past
    // transfers say about this one.
    let predict: Option<Predictor> = (remember && limit > 0).then(|| {
        let st = st.clone();
        Box::new(move |q: &fast_core::history::Query| st.history.lock().predict(q, now_secs())) as Predictor
    });

    // ...and which method (built-in, Robocopy, rsync) did best on jobs like it.
    let choose: Option<fast_core::transfer::method::MethodChooser> = (remember && limit > 0).then(|| {
        let st = st.clone();
        Box::new(move |q: &fast_core::history::Query, can: &[fast_core::transfer::method::Method]| {
            st.history.lock().choose_method(q, can, explore, now_secs())
        }) as fast_core::transfer::method::MethodChooser
    });

    let touched = Job::Transfer { spec: spec.clone() }.touched_dirs();
    let st2 = st.clone();
    let on_progress = move |p: Progress| {
        let finished = matches!(p.phase, Phase::Done | Phase::Cancelled | Phase::Failed);
        if finished {
            st2.transfers.lock().remove(&p.id);
            remember_transfer(&st2, &p);
            fs_changed(&app, &st2, touched.clone());
        }
        let _ = app.emit("transfer-progress", p);
    };
    let handle = transfer::start_with(id, spec, budget, drive_list, predict, choose, on_progress);
    let mut transfers = st.transfers.lock();
    transfers.insert(id, handle.clone());
    // A tiny job can finish before the handle is stored; do not leak it.
    if handle.is_finished() {
        transfers.remove(&id);
    }
    Ok(id)
}

/// Ask an administrator helper to stop (it cannot be signalled directly).
fn cancel_elevated(st: &Shared, id: u64) -> Option<CmdResult<()>> {
    let file = st.elevated.lock().get(&id).cloned()?;
    Some(std::fs::write(file, b"").map_err(err))
}

#[tauri::command]
pub fn transfer_control(state: State<'_, Shared>, id: u64, action: String) -> CmdResult<()> {
    if let Some(r) = cancel_elevated(&state, id) {
        return match action.as_str() {
            "cancel" => r,
            _ => Err("Pause is not available while running as administrator; you can cancel it.".into()),
        };
    }
    let transfers = state.transfers.lock();
    let h = transfers.get(&id).ok_or("transfer not found")?;
    match action.as_str() {
        "pause" => h.pause(),
        "resume" => h.resume(),
        "cancel" => h.cancel(),
        other => return Err(format!("unknown action {other}")),
    }
    Ok(())
}

/// Answer the "file already exists" question a running transfer is showing.
/// `answer` is `skip`, `replace` or `keepBoth`; with `all`, it applies to every
/// later conflict in the same transfer.
#[tauri::command]
pub fn transfer_resolve(state: State<'_, Shared>, id: u64, answer: String, all: bool) -> CmdResult<()> {
    let r = match answer.as_str() {
        "skip" => fast_core::transfer::Resolution::Skip,
        "replace" => fast_core::transfer::Resolution::Replace,
        "keepBoth" => fast_core::transfer::Resolution::KeepBoth,
        other => return Err(format!("unknown answer {other}")),
    };
    let transfers = state.transfers.lock();
    transfers.get(&id).ok_or("transfer not found")?.resolve(r, all);
    Ok(())
}

/// Stop a transfer without waiting for it to answer. A normal cancel is a
/// request the copy has to notice, and one stuck waiting on a dead network
/// drive never does. This asks it to stop, stops any copy tool outright, and
/// lets go of the job at once; whatever it is stuck in is abandoned and ends
/// whenever the drive finally answers (or when the app closes).
#[tauri::command]
pub fn transfer_force_stop(state: State<'_, Shared>, id: u64) -> CmdResult<()> {
    if let Some(r) = cancel_elevated(&state, id) {
        return r;
    }
    if let Some(h) = state.transfers.lock().remove(&id) {
        h.cancel();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Operations that may need administrator rights
// ---------------------------------------------------------------------------

/// Would the current user be refused if this job ran now? The app calls this
/// *before* running anything and asks the person first if the answer is yes.
#[tauri::command]
pub async fn check_access(job: Job) -> CmdResult<AccessCheck> {
    blocking(move || Ok(ops::check_access(&job))).await
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JobFinished {
    id: u64,
    /// The OS prompt was dismissed: nothing was run.
    declined: bool,
    ok: bool,
    error: Option<String>,
    /// For create and rename: the resulting path.
    path: Option<String>,
}

/// Run a job through the OS administrator prompt. Returns at once with the
/// job's id; progress arrives as `transfer-progress` / `delete-progress`
/// events and the outcome as a `job-finished` event. If the person dismisses
/// the prompt, nothing runs and `job-finished` says `declined`.
#[tauri::command]
pub async fn elevated_start(app: AppHandle, state: State<'_, Shared>, mut job: Job) -> CmdResult<u64> {
    // The elevated helper has no window to ask in, so it never overwrites without being told to.
    if let Job::Transfer { spec } = &mut job {
        if spec.conflict == fast_core::transfer::Conflict::Ask {
            spec.conflict = fast_core::transfer::Conflict::Skip;
        }
    }
    let st = state.inner().clone();
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    std::thread::Builder::new()
        .name(format!("elevated-{id}"))
        .spawn(move || {
            let touched = job.touched_dirs();
            let live = app.clone();
            let on_report = move |r: HelperReport| match r {
                HelperReport::Transfer { mut progress } => {
                    progress.id = id;
                    let _ = live.emit("transfer-progress", progress);
                }
                HelperReport::Delete { mut progress } => {
                    progress.id = id;
                    let _ = live.emit("delete-progress", progress);
                }
                HelperReport::Simple { .. } => {}
            };
            let registry = st.clone();
            let outcome = ops::run_elevated_job(&job, &st.paths.scratch, on_report, move |cancel_file| {
                registry.elevated.lock().insert(id, cancel_file);
            });
            st.elevated.lock().remove(&id);
            let finished = match outcome {
                Err(e) => JobFinished { id, declined: false, ok: false, error: Some(e.to_string()), path: None },
                Ok(o) if o.declined() => JobFinished { id, declined: true, ok: false, error: None, path: None },
                Ok(o) => {
                    if let Some(HelperReport::Transfer { progress }) = &o.report {
                        remember_transfer(&st, progress);
                    }
                    let code = matches!(o.elevation, Elevation::Completed(0));
                    match o.report {
                        Some(HelperReport::Simple { ok, error, path }) => JobFinished { id, declined: false, ok, error, path },
                        _ => JobFinished { id, declined: false, ok: code, error: None, path: None },
                    }
                }
            };
            fs_changed(&app, &st, touched);
            let _ = app.emit("job-finished", finished);
        })
        .map_err(err)?;
    Ok(id)
}

/// Delete in the background with progress (`delete-progress` events).
/// Returns the job id.
#[tauri::command]
pub async fn delete_start(
    app: AppHandle,
    state: State<'_, Shared>,
    paths: Vec<String>,
    permanent: bool,
) -> CmdResult<u64> {
    let st = state.inner().clone();
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    let threads = st.budget.read().io_threads;
    std::thread::Builder::new()
        .name(format!("delete-{id}"))
        .spawn(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let live = app.clone();
            ops::delete_with_progress(id, &paths, permanent, threads, cancel, move |p| {
                let _ = live.emit("delete-progress", p);
            });
            st.deletes.lock().remove(&id);
            fs_changed(&app, &st, ops::parents(&paths));
        })
        .map_err(err)?;
    Ok(id)
}

#[tauri::command]
pub fn delete_control(state: State<'_, Shared>, id: u64, action: String) -> CmdResult<()> {
    if action != "cancel" {
        return Err(format!("unknown action {action}"));
    }
    if let Some(r) = cancel_elevated(&state, id) {
        return r;
    }
    match state.deletes.lock().get(&id) {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        }
        None => Err("delete not found".into()),
    }
}

/// Show the operating system's own properties window. Returns false when it
/// has none for this request (or the person turned that off in settings), so
/// the app shows its own.
#[tauri::command]
pub async fn show_properties(state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<bool> {
    if !state.settings.read().general.native_properties {
        return Ok(false);
    }
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        platform::show_properties(&paths).map_err(err)
    })
    .await
}

#[tauri::command]
pub fn history_info(state: State<'_, Shared>) -> HistoryInfo {
    state.history.lock().info(&state.paths.history)
}

#[tauri::command]
pub async fn clear_history(state: State<'_, Shared>) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        st.history.lock().clear();
        let _ = std::fs::remove_file(&st.paths.history);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn rename_item(app: AppHandle, state: State<'_, Shared>, path: String, new_name: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let p = PathBuf::from(path);
        let target = entry::rename(&p, &new_name).map_err(err)?;
        fs_changed(&app, &st, ops::parents(&[p]));
        Ok(target.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn create_item(
    app: AppHandle,
    state: State<'_, Shared>,
    dir: String,
    name: String,
    folder: bool,
) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(format!("invalid name: {name:?}"));
        }
        let target = transfer::engine::unique_name(&Path::new(&dir).join(&name));
        if folder { entry::create_dir(&target) } else { entry::create_file(&target) }.map_err(err)?;
        fs_changed(&app, &st, vec![PathBuf::from(dir)]);
        Ok(target.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn open_item(path: String) -> CmdResult<()> {
    blocking(move || platform::open_path(Path::new(&path)).map_err(err)).await
}

#[tauri::command]
pub async fn open_terminal(state: State<'_, Shared>, path: String, mode: TerminalMode) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let shell = {
            let s = st.settings.read();
            match platform::os_name() {
                "windows" => s.terminal.windows_shell.clone(),
                "macos" => s.terminal.mac_terminal.clone(),
                _ => s.terminal.linux_terminal.clone(),
            }
        };
        platform::open_terminal(Path::new(&path), mode, &shell).map_err(err)
    })
    .await
}

// ---------------------------------------------------------------------------
// Settings, session, app info
// ---------------------------------------------------------------------------

pub fn apply_budget(app: &AppHandle, st: &Shared) {
    let budget = resources::compute(&st.settings.read().performance, &resources::snapshot());
    let changed = *st.budget.read() != budget;
    if changed {
        st.listings.lock().set_capacity(budget.listing_cache_dirs);
        *st.budget.write() = budget.clone();
        let _ = app.emit("budget-changed", budget);
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, Shared>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command]
pub async fn set_settings(app: AppHandle, state: State<'_, Shared>, settings: Settings) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let hidden_changed = st.settings.read().general.show_hidden != settings.general.show_hidden;
        settings.save(&st.paths.settings).map_err(err)?;
        *st.settings.write() = settings;
        if hidden_changed {
            st.listings.lock().clear();
        }
        {
            let limit = st.settings.read().transfer.history_limit;
            let mut history = st.history.lock();
            if history.len() > limit {
                history.trim(limit);
                let _ = history.save(&st.paths.history);
            }
        }
        apply_budget(&app, &st);
        // Other windows (a detached Settings window, tabs in their own windows) follow.
        let _ = app.emit("settings-changed", st.settings.read().clone());
        Ok(())
    })
    .await
}

#[tauri::command]
pub fn get_session(state: State<'_, Shared>) -> Session {
    state.session.lock().clone()
}

#[tauri::command]
pub async fn save_session(state: State<'_, Shared>, mut session: Session) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        session.normalize();
        // The drive cache is owned by the backend.
        session.drives = st.session.lock().drives.clone();
        session.save(&st.paths.session).map_err(err)?;
        *st.session.lock() = session;
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    os: &'static str,
    version: &'static str,
    separator: char,
    home: Option<String>,
    budget: Budget,
    system: SystemSnapshot,
    index: IndexStatus,
    thumbnail_types: &'static [&'static str],
}

#[tauri::command]
pub async fn app_info(state: State<'_, Shared>) -> CmdResult<AppInfo> {
    let st = state.inner().clone();
    blocking(move || {
        Ok(AppInfo {
            os: platform::os_name(),
            version: env!("CARGO_PKG_VERSION"),
            separator: std::path::MAIN_SEPARATOR,
            home: platform::home_dir().map(|p| p.to_string_lossy().into_owned()),
            budget: st.budget.read().clone(),
            system: resources::snapshot(),
            index: st.index_status.lock().clone(),
            thumbnail_types: thumbs::SUPPORTED,
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppDetails {
    version: &'static str,
    license: &'static str,
    profile: &'static str,
    arch: &'static str,
    os: &'static str,
    tauri: &'static str,
    webview: Option<String>,
    started_secs_ago: u64,
    settings_dir: String,
    cache_dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullReport {
    app: AppDetails,
    system: fast_core::sysreport::SystemReport,
}

/// Everything the About page shows. Reading drive health and hardware can
/// take a second or two, so this runs off the UI thread and the page shows
/// what it already has while it waits.
#[tauri::command]
pub async fn system_report(state: State<'_, Shared>) -> CmdResult<FullReport> {
    let st = state.inner().clone();
    blocking(move || {
        let system = fast_core::sysreport::collect();
        Ok(FullReport {
            app: AppDetails {
                version: env!("CARGO_PKG_VERSION"),
                license: env!("CARGO_PKG_LICENSE"),
                profile: if cfg!(debug_assertions) { "debug" } else { "release" },
                arch: std::env::consts::ARCH,
                os: std::env::consts::OS,
                tauri: tauri::VERSION,
                webview: tauri::webview_version().ok(),
                started_secs_ago: st.started.elapsed().as_secs(),
                settings_dir: st.paths.settings.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
                cache_dir: st.paths.thumbs.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
            },
            system,
        })
    })
    .await
}

/// Size of a file or of everything in a folder, and the number of files.
fn disk_footprint(path: &Path) -> (u64, u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return (0, 0) };
    if !meta.is_dir() {
        return (meta.len(), 1);
    }
    let Ok(rd) = std::fs::read_dir(path) else { return (0, 0) };
    rd.filter_map(|e| e.ok()).fold((0, 0), |(b, n), e| {
        let (eb, en) = disk_footprint(&e.path());
        (b + eb, n + en)
    })
}

fn remove_path(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    /// Stable id passed back to `clear_cache`.
    id: &'static str,
    label: &'static str,
    description: &'static str,
    /// Bytes on disk.
    disk_bytes: u64,
    /// Bytes held in memory right now.
    memory_bytes: u64,
    /// Files, entries or records, whichever is natural for this cache.
    items: u64,
    /// Clearing it only costs time to rebuild (true) or forgets something
    /// the app learned from you (false).
    rebuildable: bool,
}

/// How much storage every kind of cache uses, so each can be cleared alone.
#[tauri::command]
pub async fn cache_usage(state: State<'_, Shared>) -> CmdResult<Vec<CacheEntry>> {
    let st = state.inner().clone();
    blocking(move || {
        let (thumb_bytes, thumb_items) = disk_footprint(&st.paths.thumbs);
        let (index_bytes, _) = disk_footprint(&st.paths.index);
        let (usage_bytes, usage_items) = disk_footprint(&st.paths.usage);
        let (history_bytes, _) = disk_footprint(&st.paths.history);
        let (predict_bytes, _) = disk_footprint(&st.paths.predictor);
        let (scratch_bytes, scratch_items) = disk_footprint(&st.paths.scratch);
        let idx = st.index.read().clone();
        let history = st.history.lock().info(&st.paths.history);
        let (listing_items, listing_mem) = {
            let l = st.listings.lock();
            (l.len() as u64, l.memory_bytes() as u64)
        };
        Ok(vec![
            CacheEntry {
                id: "thumbnails",
                label: "Thumbnails",
                description: "Small previews of images, so folders of pictures open instantly.",
                disk_bytes: thumb_bytes,
                memory_bytes: 0,
                items: thumb_items,
                rebuildable: true,
            },
            CacheEntry {
                id: "index",
                label: "Search index",
                description: "Names of files on your drives, for instant search. Rebuilt in the background.",
                disk_bytes: index_bytes,
                memory_bytes: idx.memory_bytes() as u64,
                items: idx.len() as u64,
                rebuildable: true,
            },
            CacheEntry {
                id: "listings",
                label: "Folder listings",
                description: "Recently opened folders kept in memory so going back is instant.",
                disk_bytes: 0,
                memory_bytes: listing_mem,
                items: listing_items,
                rebuildable: true,
            },
            CacheEntry {
                id: "usage",
                label: "Disk usage snapshots",
                description: "The last result for each folder analysed, shown immediately next time.",
                disk_bytes: usage_bytes,
                memory_bytes: 0,
                items: usage_items,
                rebuildable: true,
            },
            CacheEntry {
                id: "history",
                label: "Transfer history",
                description: "How past transfers went, used to predict the time of similar ones.",
                disk_bytes: history_bytes,
                memory_bytes: 0,
                items: history.records as u64,
                rebuildable: false,
            },
            CacheEntry {
                id: "predictions",
                label: "Folder predictions",
                description: "Which folders you tend to open next, used to prepare them early.",
                disk_bytes: predict_bytes,
                memory_bytes: 0,
                items: 0,
                rebuildable: false,
            },
            CacheEntry {
                id: "scratch",
                label: "Administrator work files",
                description: "Temporary files used while running an action as administrator.",
                disk_bytes: scratch_bytes,
                memory_bytes: 0,
                items: scratch_items,
                rebuildable: true,
            },
        ])
    })
    .await
}

/// Clear one kind of cache (ids from `cache_usage`), or "all".
#[tauri::command]
pub async fn clear_cache(app: AppHandle, state: State<'_, Shared>, id: String) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let all = id == "all";
        let wants = |name: &str| all || id == name;
        if wants("listings") {
            st.folder_sizes.lock().clear();
            st.listings.lock().clear();
        }
        if wants("index") {
            *st.index.write() = Arc::new(fast_core::indexset::IndexSet::default());
            let _ = std::fs::remove_dir_all(&st.paths.index);
            publish_index_status(&app, &st, "idle");
        }
        if wants("thumbnails") {
            thumbs::trim_cache(&st.paths.thumbs, 0);
        }
        if wants("usage") {
            remove_path(&st.paths.usage);
        }
        if wants("history") {
            st.history.lock().clear();
            let _ = std::fs::remove_file(&st.paths.history);
        }
        if wants("predictions") {
            st.predictor.lock().clear();
            let _ = st.predictor.lock().save(&st.paths.predictor);
        }
        if wants("scratch") && st.elevated.lock().is_empty() {
            remove_path(&st.paths.scratch);
        }
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// Network drives
// ---------------------------------------------------------------------------

/// Run a short Windows console tool without flashing a console window.
#[cfg(windows)]
fn run_net(args: &[&str]) -> CmdResult<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("net")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(err)?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() {
        Ok(text(&out.stdout))
    } else {
        let message = [text(&out.stderr), text(&out.stdout)].into_iter().find(|t| !t.is_empty());
        Err(message.unwrap_or_else(|| "Windows could not connect to that location.".into()))
    }
}

/// Turn `\\\\server\\share`, `//server/share` or `smb://server/share` into an `smb://` address.
#[cfg(not(windows))]
fn smb_url(input: &str) -> CmdResult<String> {
    let t = input.trim().replace('\\', "/");
    let url = if t.contains("://") {
        t
    } else if let Some(rest) = t.strip_prefix("//") {
        format!("smb://{rest}")
    } else {
        return Err("Enter the location like smb://server/share".into());
    };
    if url.trim_start_matches(|c: char| c != '/').trim_start_matches('/').is_empty() {
        return Err("Enter the location like smb://server/share".into());
    }
    Ok(url)
}

/// Run a short helper program and report what it said if it failed.
#[cfg(not(windows))]
fn run_tool(program: &str, args: &[&str]) -> CmdResult<String> {
    let out = std::process::Command::new(program).args(args).output().map_err(|e| format!("Could not run {program}: {e}"))?;
    let text = |b: &[u8]| String::from_utf8_lossy(b).trim().to_string();
    if out.status.success() {
        Ok(text(&out.stdout))
    } else {
        Err([text(&out.stderr), text(&out.stdout)].into_iter().find(|t| !t.is_empty()).unwrap_or_else(|| format!("{program} failed")))
    }
}

/// Connect a network share. Windows maps it to a drive letter (`letter` like
/// "Z:", or a free one when empty); macOS mounts it under /Volumes; Linux
/// mounts it through the desktop's GVFS. Returns a folder to open, if known.
#[tauri::command]
pub async fn network_map(app: AppHandle, state: State<'_, Shared>, remote: String, letter: Option<String>) -> CmdResult<Option<String>> {
    #[cfg(windows)]
    {
        let st = state.inner().clone();
        let remote = remote.trim().to_string();
        if !remote.starts_with("\\\\") {
            return Err("Enter the location like \\\\server\\share".into());
        }
        blocking(move || {
            let target = letter.filter(|l| !l.is_empty()).unwrap_or_else(|| "*".into());
            run_net(&["use", &target, &remote, "/persistent:yes"])?;
            *st.drives.write() = fast_core::drives::list_drives();
            Ok(())
        })
        .await?;
        let _ = app.emit("fs-changed", Vec::<String>::new());
        Ok(None)
    }
    #[cfg(not(windows))]
    {
        let _ = letter;
        let st = state.inner().clone();
        let opened = blocking(move || {
            let url = smb_url(&remote)?;
            #[cfg(target_os = "macos")]
            let open = {
                run_tool("osascript", &["-e", &format!("mount volume \"{}\"", url.replace('"', ""))])?;
                let share = url.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
                Some(format!("/Volumes/{share}")).filter(|p| Path::new(p).exists())
            };
            #[cfg(not(target_os = "macos"))]
            let open = {
                run_tool("gio", &["mount", &url]).map_err(|e| {
                    if e.contains("Could not run") {
                        "Connecting needs the 'gio' tool (package gvfs). Install it, or mount the share with your file manager.".to_string()
                    } else {
                        e
                    }
                })?;
                // GVFS shows the share as a folder under the user's runtime dir.
                let uid = run_tool("id", &["-u"]).unwrap_or_default();
                let base = PathBuf::from(format!("/run/user/{uid}/gvfs"));
                let host = url.trim_start_matches("smb://").split('/').next().unwrap_or_default().to_string();
                std::fs::read_dir(&base)
                    .ok()
                    .and_then(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| p.to_string_lossy().contains(&format!("server={host}"))))
                    .map(|p| p.to_string_lossy().into_owned())
            };
            *st.drives.write() = fast_core::drives::list_drives();
            Ok(open)
        })
        .await?;
        let _ = app.emit("fs-changed", Vec::<String>::new());
        Ok(opened)
    }
}

/// Disconnect a network drive (`mount` like "Z:\\" on Windows, a mount point elsewhere).
#[tauri::command]
pub async fn network_disconnect(app: AppHandle, state: State<'_, Shared>, mount: String) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        #[cfg(windows)]
        {
            let letter = mount.trim_end_matches('\\').to_string();
            run_net(&["use", &letter, "/delete", "/y"])?;
        }
        #[cfg(target_os = "macos")]
        run_tool("diskutil", &["unmount", &mount])?;
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            if run_tool("gio", &["mount", "-u", &mount]).is_err() {
                run_tool("umount", &[&mount])?;
            }
        }
        *st.drives.write() = fast_core::drives::list_drives();
        Ok(())
    })
    .await?;
    let _ = app.emit("fs-changed", Vec::<String>::new());
    Ok(())
}

// ---------------------------------------------------------------------------
// The system's right-click entries, shortcuts and imported assets
// ---------------------------------------------------------------------------

/// Entries other programs add to the right-click menu for these items (or for
/// a folder's background). Slow, so the app shows its own menu first.
#[tauri::command]
pub async fn shell_menu(paths: Vec<String>, background: bool) -> CmdResult<Vec<fast_core::shellmenu::ShellItem>> {
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        fast_core::shellmenu::build(paths, background).map_err(err)
    })
    .await
}

#[tauri::command]
pub async fn shell_invoke(id: u32) -> CmdResult<()> {
    blocking(move || fast_core::shellmenu::invoke(id).map_err(err)).await
}

/// Make a shortcut next to an item: a symbolic link on Linux and macOS, a
/// `.lnk` file on Windows.
#[tauri::command]
pub async fn create_shortcut(path: String) -> CmdResult<String> {
    blocking(move || {
        let target = PathBuf::from(&path);
        let dir = target.parent().ok_or("no folder to put the shortcut in")?.to_path_buf();
        let stem = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Shortcut".into());
        let mut n = 0;
        let link = loop {
            let name = match (cfg!(windows), n) {
                (true, 0) => format!("{stem} - Shortcut.lnk"),
                (true, n) => format!("{stem} - Shortcut ({n}).lnk"),
                (false, 0) => format!("{stem} - Shortcut"),
                (false, n) => format!("{stem} - Shortcut ({n})"),
            };
            let candidate = dir.join(name);
            if !candidate.exists() {
                break candidate;
            }
            n += 1;
        };
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).map_err(err)?;
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Paths travel in environment variables, never inside the script text.
            let status = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-Command",
                    "$s=(New-Object -ComObject WScript.Shell).CreateShortcut($env:FAST_LINK);$s.TargetPath=$env:FAST_TARGET;$s.Save()",
                ])
                .env("FAST_LINK", &link)
                .env("FAST_TARGET", &target)
                .creation_flags(0x0800_0000)
                .status()
                .map_err(err)?;
            if !status.success() {
                return Err("Windows could not create the shortcut.".to_string());
            }
        }
        Ok(link.to_string_lossy().into_owned())
    })
    .await
}

/// Imported fonts, style sheets and images live together in the app's config
/// folder, in a subfolder per kind.
fn asset_dir(st: &Shared, kind: &str) -> CmdResult<PathBuf> {
    let sub = match kind {
        "font" => "fonts",
        "css" => "styles",
        "image" => "backgrounds",
        _ => return Err("unknown asset kind".into()),
    };
    let dir = st.paths.settings.parent().ok_or("no config folder")?.join(sub);
    std::fs::create_dir_all(&dir).map_err(err)?;
    Ok(dir)
}

fn allowed_asset(kind: &str, name: &str) -> bool {
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match kind {
        "font" => ["ttf", "otf", "woff", "woff2"].contains(&ext.as_str()),
        "css" => ext == "css",
        "image" => ["png", "jpg", "jpeg", "webp", "gif", "bmp"].contains(&ext.as_str()),
        _ => false,
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    name: String,
    bytes: u64,
}

/// Copy a file the person chose into the app's own folder; returns its name.
#[tauri::command]
pub async fn import_asset(state: State<'_, Shared>, kind: String, path: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let src = PathBuf::from(&path);
        let name = src.file_name().ok_or("not a file")?.to_string_lossy().into_owned();
        if !allowed_asset(&kind, &name) {
            return Err(format!("\"{name}\" is not a supported {kind} file."));
        }
        let meta = std::fs::metadata(&src).map_err(err)?;
        if meta.len() > 64 * 1024 * 1024 {
            return Err("That file is larger than 64 MB.".into());
        }
        std::fs::copy(&src, asset_dir(&st, &kind)?.join(&name)).map_err(err)?;
        Ok(name)
    })
    .await
}

#[tauri::command]
pub async fn list_assets(state: State<'_, Shared>, kind: String) -> CmdResult<Vec<Asset>> {
    let st = state.inner().clone();
    blocking(move || {
        let dir = asset_dir(&st, &kind)?;
        let mut out: Vec<Asset> = std::fs::read_dir(dir)
            .map_err(err)?
            .filter_map(|e| e.ok())
            .filter_map(|e| Some(Asset { name: e.file_name().to_string_lossy().into_owned(), bytes: e.metadata().ok()?.len() }))
            .filter(|a| allowed_asset(&kind, &a.name))
            .collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    })
    .await
}

#[tauri::command]
pub async fn read_asset(state: State<'_, Shared>, kind: String, name: String) -> CmdResult<tauri::ipc::Response> {
    let st = state.inner().clone();
    let bytes = blocking(move || {
        if name.contains(['/', '\\']) || !allowed_asset(&kind, &name) {
            return Err("invalid asset name".to_string());
        }
        std::fs::read(asset_dir(&st, &kind)?.join(&name)).map_err(err)
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn delete_asset(state: State<'_, Shared>, kind: String, name: String) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        if name.contains(['/', '\\']) || !allowed_asset(&kind, &name) {
            return Err("invalid asset name".to_string());
        }
        std::fs::remove_file(asset_dir(&st, &kind)?.join(&name)).map_err(err)
    })
    .await
}

// ---------------------------------------------------------------------------
// Custom right-click actions
// ---------------------------------------------------------------------------

/// Split an argument string like `--open "{path}" -v` into words, honouring quotes.
fn split_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut cur, mut quote, mut had) = (String::new(), None::<char>, false);
    for c in text.chars() {
        match (c, quote) {
            ('"' | '\'', None) => {
                quote = Some(c);
                had = true;
            }
            (q, Some(open)) if q == open => quote = None,
            (c, None) if c.is_whitespace() => {
                if had || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    had = false;
                }
            }
            _ => cur.push(c),
        }
    }
    if had || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Run one of the person's own actions. The program is started directly,
/// never through a shell, so file names cannot inject commands.
#[tauri::command]
pub async fn run_custom_action(state: State<'_, Shared>, id: String, paths: Vec<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let action = st
            .settings
            .read()
            .custom_actions
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or("That action no longer exists.")?;
        if action.program.trim().is_empty() {
            return Err("That action has no program set.".into());
        }
        let template = split_args(&action.args);
        let runs: Vec<Vec<String>> = if action.all_at_once { vec![paths.clone()] } else { paths.iter().map(|p| vec![p.clone()]).collect() };
        for group in runs {
            let mut args: Vec<String> = Vec::new();
            for word in &template {
                if word.contains("{path}") && group.len() > 1 && word == "{path}" {
                    args.extend(group.iter().cloned());
                } else {
                    let first = group.first().cloned().unwrap_or_default();
                    let dir = Path::new(&first).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                    args.push(word.replace("{path}", &first).replace("{dir}", &dir));
                }
            }
            if template.iter().all(|w| !w.contains("{path}")) {
                args.extend(group.iter().cloned());
            }
            let mut cmd = std::process::Command::new(action.program.trim());
            cmd.args(&args).stdin(std::process::Stdio::null());
            if let Some(dir) = group.first().and_then(|p| Path::new(p).parent()) {
                cmd.current_dir(dir);
            }
            cmd.spawn().map_err(|e| format!("{}: {e}", action.program))?;
        }
        Ok(())
    })
    .await
}

/// The folder that holds the app's settings, sessions and imported files.
#[tauri::command]
pub fn config_folder(state: State<'_, Shared>) -> Option<String> {
    state.paths.settings.parent().map(|p| p.to_string_lossy().into_owned())
}

/// Replace all settings with a JSON document (import, or the raw editor).
/// Unknown keys are dropped and missing ones take their defaults.
#[tauri::command]
pub async fn import_settings(app: AppHandle, state: State<'_, Shared>, json: String) -> CmdResult<Settings> {
    let parsed: Settings = serde_json::from_str(&json).map_err(|e| format!("Not valid settings: {e}"))?;
    set_settings(app, state, parsed.clone()).await?;
    Ok(parsed)
}


/// Open another window showing `location` in its only tab, near the given
/// screen position. Used when a tab is dragged out of the tab bar.
#[tauri::command]
pub async fn open_window(app: AppHandle, state: State<'_, Shared>, location: String, x: Option<f64>, y: Option<f64>) -> CmdResult<()> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    let n = state.next_id.fetch_add(1, Ordering::Relaxed);
    let script = format!("window.__OPEN__ = {};", serde_json::to_string(&location).map_err(err)?);
    let mut builder = WebviewWindowBuilder::new(&app, format!("window-{n}"), WebviewUrl::App("index.html".into()))
        .title("FAST File Explorer")
        .inner_size(1100.0, 720.0)
        .min_inner_size(520.0, 360.0)
        .initialization_script(&script);
    #[cfg(target_os = "macos")]
    {
        builder = builder.decorations(true).title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.decorations(false);
    }
    if let (Some(x), Some(y)) = (x, y) {
        builder = builder.position((x - 120.0).max(0.0), (y - 20.0).max(0.0));
    }
    builder.build().map_err(err)?;
    Ok(())
}


/// A tab was let go at screen position (x, y) of the calling window. If that
/// is over the tab bar of another app window, hand the tab to it (it picks the
/// slot from the position) and report true; the caller then removes its copy.
/// With `close_source` the calling window had only that tab, so it closes.
#[tauri::command]
pub async fn drop_tab(app: AppHandle, window: tauri::WebviewWindow, location: String, x: f64, y: f64, close_source: bool) -> CmdResult<bool> {
    use tauri::Manager;
    const TAB_BAR_HEIGHT: f64 = 46.0; // logical px: title bar with tabs
    let scale = window.scale_factor().map_err(err)?;
    let (px, py) = (x * scale, y * scale);
    for (label, target) in app.webview_windows() {
        if label == window.label() || !(label == "main" || label.starts_with("window-")) {
            continue;
        }
        if target.is_minimized().unwrap_or(false) || !target.is_visible().unwrap_or(true) {
            continue;
        }
        let (Ok(pos), Ok(size), Ok(ts)) = (target.outer_position(), target.outer_size(), target.scale_factor()) else { continue };
        let (left, top) = (pos.x as f64, pos.y as f64);
        let inside = px >= left && px <= left + size.width as f64 && py >= top && py <= top + TAB_BAR_HEIGHT * ts;
        if !inside {
            continue;
        }
        let payload = serde_json::json!({ "location": location, "x": (px - left) / ts, "y": (py - top) / ts });
        app.emit_to(label.as_str(), "tab-drop", payload).map_err(err)?;
        let _ = target.unminimize();
        let _ = target.set_focus();
        if close_source {
            let _ = window.close();
        }
        return Ok(true);
    }
    Ok(false)
}

/// Open one of the app's panels as its own native window (it moves, resizes
/// and closes like any window). Asking again focuses the open one. Windows
/// are labelled by `kind`, and every window is told when one opens or closes.
fn open_detached(app: &AppHandle, kind: &'static str, title: &str, size: (f64, f64), min: (f64, f64)) -> CmdResult<()> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window(kind) {
        let _ = w.unminimize();
        w.set_focus().map_err(err)?;
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, kind, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(min.0, min.1)
        .resizable(true)
        .initialization_script(&format!("window.__VIEW__ = '{kind}';"))
        .build()
        .map_err(err)?;
    let handle = app.clone();
    win.on_window_event(move |e| {
        if matches!(e, tauri::WindowEvent::Destroyed) {
            let _ = handle.emit("detached", DetachedEvent { kind: kind.into(), open: false });
        }
    });
    let _ = app.emit("detached", DetachedEvent { kind: kind.into(), open: true });
    Ok(())
}

#[derive(Serialize, Clone)]
pub struct DetachedEvent {
    kind: String,
    open: bool,
}

#[tauri::command]
pub async fn open_transfers_window(app: AppHandle) -> CmdResult<()> {
    open_detached(&app, "transfers", "Transfers - FAST File Explorer", (780.0, 580.0), (420.0, 300.0))
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> CmdResult<()> {
    open_detached(&app, "settings", "Settings - FAST File Explorer", (980.0, 700.0), (560.0, 360.0))
}

/// Put a detached panel back into the main window: tell the main window what
/// came back, bring it forward, and close the window this was called from.
#[tauri::command]
pub async fn reattach(app: AppHandle, window: tauri::WebviewWindow, kind: String, payload: Option<serde_json::Value>) -> CmdResult<()> {
    use tauri::Manager;
    let main = app.get_webview_window("main").ok_or("The main window is closed, so there is nothing to attach to.")?;
    app.emit_to("main", "reattach", serde_json::json!({ "kind": kind, "payload": payload })).map_err(err)?;
    let _ = main.unminimize();
    let _ = main.set_focus();
    window.close().map_err(err)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedPath {
    pub path: String,
    pub is_dir: bool,
}

/// Turn what someone typed in the address bar into a real path: quotes,
/// `~`, `%VAR%` / `$VAR`, `/` on Windows and a bare drive letter are all
/// understood. Fails with a readable message when nothing is there.
#[tauri::command]
pub async fn resolve_path(input: String) -> CmdResult<ResolvedPath> {
    blocking(move || {
        let mut text = input.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
        if text.is_empty() {
            return Err("Type a path".to_string());
        }
        if let Some(rest) = text.strip_prefix("file://") {
            text = rest.trim_start_matches('/').to_string();
        }
        // ~ for the home folder.
        if text == "~" || text.starts_with("~/") || text.starts_with("~\\") {
            if let Some(home) = platform::home_dir() {
                text = format!("{}{}", home.to_string_lossy(), &text[1..]);
            }
        }
        text = expand_env(&text);
        #[cfg(windows)]
        {
            text = text.replace('/', "\\");
            // "G:" alone means the root of the drive, not "the current folder on G:".
            if text.len() == 2 && text.as_bytes()[1] == b':' {
                text.push('\\');
            }
        }
        let path = std::path::PathBuf::from(&text);
        match std::fs::metadata(&path) {
            Ok(m) => Ok(ResolvedPath { path: text, is_dir: m.is_dir() }),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Ok(ResolvedPath { path: text, is_dir: true }),
            Err(_) => Err(format!("Can't find \"{text}\"")),
        }
    })
    .await
}

/// Replace `%NAME%` and `$NAME` with environment values; unknown names stay as typed.
fn expand_env(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            if let Some(end) = chars[i + 1..].iter().position(|&x| x == '%') {
                let name: String = chars[i + 1..i + 1 + end].iter().collect();
                if !name.is_empty() {
                    if let Ok(v) = std::env::var(&name) {
                        out.push_str(&v);
                        i += end + 2;
                        continue;
                    }
                }
            }
        } else if c == '$' {
            let name: String = chars[i + 1..].iter().take_while(|x| x.is_alphanumeric() || **x == '_').collect();
            if !name.is_empty() {
                if let Ok(v) = std::env::var(&name) {
                    out.push_str(&v);
                    i += name.chars().count() + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}


// ---- Temporary download links ------------------------------------------------

/// Start serving one file at a secret, temporary address.
#[tauri::command]
pub async fn share_start(state: State<'_, Shared>, path: String, minutes: u64, max_downloads: u32, lan: bool, internet: bool) -> CmdResult<fast_core::share::ShareInfo> {
    let state = state.inner().clone();
    blocking(move || state.shares.start(std::path::Path::new(&path), fast_core::share::ShareOptions { minutes, max_downloads, lan, internet })).await
}

/// Is the tunnel program for sharing over the internet installed?
#[tauri::command]
pub async fn share_internet_available() -> CmdResult<bool> {
    blocking(|| Ok(fast_core::share::tunnel_program().is_some())).await
}

#[tauri::command]
pub async fn share_stop(state: State<'_, Shared>, id: String) -> CmdResult<()> {
    state.shares.stop(&id);
    Ok(())
}

#[tauri::command]
pub async fn share_list(state: State<'_, Shared>) -> CmdResult<Vec<fast_core::share::ShareInfo>> {
    Ok(state.shares.list())
}
