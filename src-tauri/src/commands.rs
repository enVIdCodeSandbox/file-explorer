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
use serde::{Deserialize, Serialize};
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

/// How long an unlock lasts, from Advanced settings (0 = until the app closes).
fn relock_after(st: &crate::state::AppState) -> Option<std::time::Duration> {
    let mins = st.settings.read().advanced.get("locks.relockMin").and_then(|v| v.as_u64()).unwrap_or(15);
    (mins > 0).then(|| std::time::Duration::from_secs(mins * 60))
}

/// Is this path behind a lock that has not been opened? Remote paths never are.
fn is_locked(st: &crate::state::AppState, path: &str) -> bool {
    let locks = st.locks.read();
    !locks.is_empty() && !fast_core::remote::is_remote(path) && locks.blocking(path, relock_after(st)).is_some()
}

/// Refuse with a message the interface turns into a passcode prompt.
pub(crate) fn guard(st: &crate::state::AppState, path: &str) -> CmdResult<()> {
    if is_locked(st, path) {
        Err(format!("locked:{path}"))
    } else {
        Ok(())
    }
}

/// A batch of a folder's items, sent while the folder is still being read.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirChunk {
    pub ticket: u64,
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub done: usize,
    pub total: usize,
    /// The device started the listing over: drop what came before.
    pub restart: bool,
}

static LISTINGS: std::sync::LazyLock<parking_lot::Mutex<std::collections::HashMap<u64, Arc<std::sync::atomic::AtomicBool>>>> =
    std::sync::LazyLock::new(Default::default);

/// Stop a folder listing that is no longer wanted (the person went elsewhere).
#[tauri::command]
pub fn list_dir_cancel(ticket: u64) {
    if let Some(flag) = LISTINGS.lock().get(&ticket) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// With a `ticket`, the folder's items are also sent as `dir-chunk` events while it is read,
/// so the interface can show them at once; the result is still the whole listing.
#[tauri::command]
pub async fn list_dir(app: AppHandle, state: State<'_, Shared>, path: String, ticket: Option<u64>) -> CmdResult<Listing> {
    let st = state.inner().clone();
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if let Some(t) = ticket {
        LISTINGS.lock().insert(t, cancel.clone());
    }
    let result = blocking(move || {
        let emit_chunk = |at: &str, entries: Vec<FileEntry>, p: entry::ListProgress| {
            if let Some(t) = ticket {
                if !entries.is_empty() || p.restart {
                    let _ = app.emit("dir-chunk", DirChunk { ticket: t, path: at.to_string(), entries, done: p.done, total: p.total, restart: p.restart });
                }
            }
        };
        if fast_core::browse::is_loc(&path) {
            return crate::remote::archive_list(&st, &path);
        }
        guard(&st, &path)?;
        if fast_core::remote::is_remote(&path) {
            if ticket.is_some() {
                return crate::remote::list_streaming(&st, &path, &cancel, &mut |e, p| emit_chunk(&path, e, p));
            }
            return crate::remote::list(&st, &path);
        }
        let p = PathBuf::from(&path);
        let show_hidden = st.settings.read().general.show_hidden;
        let remember = !cache_off(&st, &p);
        let hit = if remember { st.listings.lock().get(&p, show_hidden) } else { None };
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
        let entries = if ticket.is_some() {
            let at = p.to_string_lossy().into_owned();
            entry::list_dir_streaming(&p, show_hidden, &cancel, &mut |e, pr| emit_chunk(&at, e.to_vec(), pr)).map_err(err)?
        } else {
            entry::list_dir(&p, show_hidden).map_err(err)?
        };
        let entries = Arc::new(entries);
        if remember {
            st.listings.lock().put(p.clone(), entries.clone(), show_hidden);
        }
        Ok(listing(&p, entries, false))
    })
    .await;
    if let Some(t) = ticket {
        LISTINGS.lock().remove(&t);
    }
    result
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
        if st.listings.lock().contains(&p) || cache_off(st, &p) || on_network(st, &p) {
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
        if !advanced_bool(&st, "cache.predictions", true) {
            return Ok(Vec::new());
        }
        if cache_off(&st, Path::new(&to)) {
            return Ok(Vec::new());
        }
        let from = from.filter(|f| !cache_off(&st, Path::new(f)));
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
            // The folder just opened is being listed right now; warm the next ones after it.
            std::thread::sleep(std::time::Duration::from_millis(300));
            prefetch_now(&st, predictions.clone());
        }
        Ok(predictions)
    })
    .await
}

/// The folders opened most often under the person's own rules (how far back, time open against times opened, how many
/// opens it takes), skipping any that no longer exist.
#[tauri::command]
pub async fn frequent_folders(state: State<'_, Shared>, limit: usize, policy: Option<fast_core::predict::FrequentPolicy>) -> CmdResult<Vec<String>> {
    let st = state.inner().clone();
    blocking(move || {
        let candidates = st.predictor.lock().frequent_list(limit.saturating_mul(2).max(limit + 10), &policy.unwrap_or_default());
        Ok(candidates.into_iter().filter(|p| Path::new(p).is_dir()).take(limit).collect())
    })
    .await
}

/// Folders dragged into Frequently used: each starts out counting as `count` visits.
#[tauri::command]
pub async fn frequent_seed(state: State<'_, Shared>, paths: Vec<String>, count: f64) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let mut p = st.predictor.lock();
        p.seed(&paths, count);
        let _ = p.save(&st.paths.predictor);
        Ok(())
    })
    .await
}

/// Take a folder out of Frequently used; opening it again brings it back.
#[tauri::command]
pub async fn frequent_forget(state: State<'_, Shared>, path: String) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let mut p = st.predictor.lock();
        p.forget_frequent(&path);
        let _ = p.save(&st.paths.predictor);
        Ok(())
    })
    .await
}

// ---------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------

/// The size and dates of a few files, cheaply, for following the ones that are being written.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStat {
    pub path: String,
    pub exists: bool,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: Option<i64>,
}

#[tauri::command]
pub async fn stat_many(paths: Vec<String>) -> Vec<LiveStat> {
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .into_iter()
            .take(64)
            .map(|path| {
                let meta = std::fs::metadata(&path).ok();
                let modified = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
                LiveStat { exists: meta.is_some(), size: meta.as_ref().map_or(0, |m| m.len()), modified, path }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
pub async fn get_details(state: State<'_, Shared>, path: String) -> CmdResult<Details> {
    let st = state.inner().clone();
    blocking(move || {
        if fast_core::browse::is_loc(&path) {
            return crate::remote::archive_details(&st, &path);
        }
        guard(&st, &path)?;
        if fast_core::remote::is_remote(&path) {
            return crate::remote::details(&st, &path);
        }
        details::details(Path::new(&path), &st.drives.read()).map_err(err)
    })
    .await
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
    /// True when this is a remembered size; a fresh measurement may follow.
    cached: bool,
}

/// How remembered folder sizes are used (from the Advanced settings).
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct SizePolicy {
    /// Remember sizes between visits and restarts.
    pub remember: bool,
    /// A remembered size younger than this is current and not measured again.
    pub fresh_secs: u64,
    /// Most folders to remember.
    pub max_entries: usize,
}

/// Measure the given subfolders of `dir` one after another in the background,
/// sending a `folder-size` event for each. A remembered size is sent at once;
/// if it is older than the policy allows (or `refresh` is set) the folder is
/// measured again and the new size follows. Starting another job (opening a
/// different folder) cancels this one.
#[tauri::command]
pub async fn folder_sizes_start(app: AppHandle, state: State<'_, Shared>, dir: String, names: Vec<String>, refresh: bool, policy: SizePolicy) -> CmdResult<()> {
    let st = state.inner().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = st.size_cancel.lock();
        slot.store(true, Ordering::Relaxed);
        *slot = cancel.clone();
    }
    std::thread::spawn(move || {
        // Remembered sizes are shown at once, with no waiting.
        let mut todo: Vec<String> = Vec::new();
        for name in names {
            let path = Path::new(&dir).join(&name);
            if is_locked(&st, &path.to_string_lossy()) {
                continue;
            }
            let remember = policy.remember && !cache_off(&st, &path);
            let remembered = if remember { st.folder_sizes.lock().get(&path) } else { None };
            if let Some(e) = remembered {
                let _ = app.emit("folder-size", FolderSizeEvent { dir: dir.clone(), name: name.clone(), bytes: e.bytes, files: e.files, complete: true, cached: true });
                if !refresh && (now_secs().max(0) as u64).saturating_sub(e.at) < policy.fresh_secs {
                    continue;
                }
            }
            todo.push(name);
        }
        // Measuring comes second: the listing and the first thumbnails get a head start, and
        // shares are not walked at all.
        for _ in 0..12 {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if on_network(&st, Path::new(&dir)) {
            return;
        }
        let threads = st.budget.read().index_threads.clamp(1, 2);
        let save = |st: &crate::state::AppState| {
            let mut c = st.folder_sizes.lock();
            c.trim(policy.max_entries.max(100));
            let _ = c.save(&st.paths.sizes);
        };
        for name in todo {
            if cancel.load(Ordering::Relaxed) {
                save(&st);
                return;
            }
            let path = Path::new(&dir).join(&name);
            let remember = policy.remember && !cache_off(&st, &path);
            let stats = details::folder_stats(&path, threads, Some(&cancel));
            if cancel.load(Ordering::Relaxed) {
                save(&st);
                return;
            }
            if stats.complete && remember {
                st.folder_sizes.lock().put(&path, stats.total_bytes, stats.files, now_secs().max(0) as u64);
            }
            let _ = app.emit("folder-size", FolderSizeEvent { dir: dir.clone(), name, bytes: stats.total_bytes, files: stats.files, complete: stats.complete, cached: false });
        }
        if policy.remember {
            save(&st);
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

/// The most a file may weigh to be shown in the app's own viewer. It is read into memory
/// whole, and a phone has little to spare.
const VIEW_LIMIT: u64 = 96 * 1024 * 1024;

/// The bytes of a file, for the app's own viewer on devices that cannot hand a file to
/// another app. Folders, locked places and very large files are refused.
#[tauri::command]
pub async fn read_for_view(state: State<'_, Shared>, path: String) -> CmdResult<tauri::ipc::Response> {
    let st = state.inner().clone();
    guard(&st, &path)?;
    let bytes = blocking(move || {
        let meta = std::fs::metadata(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
        if !meta.is_file() {
            return Err("That is not a file.".to_string());
        }
        if meta.len() > VIEW_LIMIT {
            return Err(format!(
                "This file is {} MB; the viewer shows files up to {} MB.",
                meta.len() / (1024 * 1024),
                VIEW_LIMIT / (1024 * 1024)
            ));
        }
        std::fs::read(&path).map_err(|e| format!("Could not read {path}: {e}"))
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn thumbnail(state: State<'_, Shared>, path: String, size: u32) -> CmdResult<tauri::ipc::Response> {
    let st = state.inner().clone();
    if !st.budget.read().thumbnails {
        return Err("thumbnails are disabled".into());
    }
    guard(&st, &path)?;
    // A phone or camera on a cable: ask the device for the preview picture it keeps.
    if fast_core::remote::is_device(&path) {
        // A phone answers one request at a time, so these go through one at a time.
        static ONE_AT_A_TIME: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let _permit = ONE_AT_A_TIME.acquire().await.map_err(err)?;
        let bytes = blocking(move || {
            let raw = crate::remote::device_thumbnail(&st, &path)?.ok_or_else(|| "this device has no preview".to_string())?;
            fast_core::thumbs::from_device(&raw, size).map_err(err)
        })
        .await?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    // A file on a server: fetch it (within a size limit) and make the thumbnail from the copy.
    if fast_core::remote::is_remote(&path) {
        let _permit = st.thumb_permits.clone().acquire_owned().await.map_err(err)?;
        let bytes = blocking(move || {
            let kind = thumbs::kind_of(Path::new(&path)).ok_or("no thumbnail for this kind of file")?;
            let (cap, key) = if kind == thumbs::Kind::Image { (advanced_u64(&st, "thumbs.maxImageMb", 64).min(25), "images") } else { (advanced_u64(&st, "thumbs.networkCopyMb", 200), "videos") };
            let allowed = match kind {
                thumbs::Kind::Image => advanced_bool(&st, "thumbs.images", true),
                thumbs::Kind::Document => advanced_bool(&st, "thumbs.documents", true),
                thumbs::Kind::Video => advanced_bool(&st, "thumbs.videos", true),
            };
            if !allowed {
                return Err(format!("thumbnails for {key} are switched off"));
            }
            let opts = thumbs::Options { max_image_bytes: cap * 1024 * 1024, video_percent: advanced_u64(&st, "thumbs.videoPercent", 10).min(95) as u8 };
            let copy = crate::remote::fetch_for_thumbnail(&st, &path, cap * 1024 * 1024)?;
            let made = thumbs::thumbnail_with(&copy, size, None, &opts).map_err(err);
            let _ = std::fs::remove_file(&copy);
            made
        })
        .await?;
        return Ok(tauri::ipc::Response::new(bytes));
    }
    // Bounded concurrency: decoding is CPU-heavy and must not starve listing. Videos and documents wait in their own
    // lane, so a slow one never holds up the pictures behind it.
    let slow = thumbs::kind_of(Path::new(&path)).is_some_and(|k| k != thumbs::Kind::Image);
    let _permit = if slow { st.slow_thumb_permits.clone() } else { st.thumb_permits.clone() }.acquire_owned().await.map_err(err)?;
    let bytes = blocking(move || {
        let keep = advanced_bool(&st, "cache.thumbnails", true) && !cache_off(&st, Path::new(&path));
        // Each kind of file can be switched off on its own (Settings > Thumbnails).
        let allowed = match thumbs::kind_of(Path::new(&path)) {
            Some(thumbs::Kind::Image) => advanced_bool(&st, "thumbs.images", true),
            Some(thumbs::Kind::Document) => advanced_bool(&st, "thumbs.documents", true),
            Some(thumbs::Kind::Video) => advanced_bool(&st, "thumbs.videos", true),
            None => false,
        };
        if !allowed {
            return Err("thumbnails for this kind of file are switched off".to_string());
        }
        let opts = thumbs::Options {
            max_image_bytes: advanced_u64(&st, "thumbs.maxImageMb", 64) * 1024 * 1024,
            video_percent: advanced_u64(&st, "thumbs.videoPercent", 10).min(95) as u8,
        };
        let direct = thumbs::thumbnail_with(Path::new(&path), size, keep.then_some(st.paths.thumbs.as_path()), &opts);
        match direct {
            // A video or document on a network drive that the system will not preview in place: try a local copy.
            Err(first) if thumbs::kind_of(Path::new(&path)) != Some(thumbs::Kind::Image) && on_network(&st, Path::new(&path)) => {
                let max = advanced_u64(&st, "thumbs.networkCopyMb", 200) * 1024 * 1024;
                thumbs::via_local_copy(Path::new(&path), size, &opts, 24 * 1024 * 1024, max).map_err(|_| err(first))
            }
            other => other.map_err(err),
        }
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// For each folder: nothing in it, only (empty) folders, or files. Used to draw empty folders differently. Local folders only.
#[tauri::command]
pub async fn folder_fill(state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<Vec<fast_core::fill::Fill>> {
    let st = state.inner().clone();
    blocking(move || {
        use rayon::prelude::*;
        Ok(paths
            .par_iter()
            .take(96)
            .map(|p| {
                if fast_core::remote::is_remote(p) || fast_core::browse::is_loc(p) || guard(&st, p).is_err() || on_network(&st, Path::new(p)) {
                    return fast_core::fill::Fill::Files;
                }
                fast_core::fill::probe(Path::new(p), 8, 2000)
            })
            .collect())
    })
    .await
}

/// Prepare the pictures of the videos and documents in a folder in the background (kept in the thumbnail cache), so they
/// are there when the tiles come into view. A newer call replaces an older one. Only files on this computer.
#[tauri::command]
pub async fn thumbs_prewarm(state: State<'_, Shared>, paths: Vec<String>, size: u32) -> CmdResult<()> {
    let st = state.inner().clone();
    let generation = st.prewarm_generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    if !st.budget.read().thumbnails || !advanced_bool(&st, "cache.thumbnails", true) {
        return Ok(());
    }
    tauri::async_runtime::spawn(async move {
        for path in paths.into_iter().take(300) {
            if st.prewarm_generation.load(std::sync::atomic::Ordering::SeqCst) != generation {
                return;
            }
            if fast_core::remote::is_remote(&path) || fast_core::remote::is_device(&path) || guard(&st, &path).is_err() || cache_off(&st, Path::new(&path)) || on_network(&st, Path::new(&path)) {
                continue;
            }
            let allowed = match thumbs::kind_of(Path::new(&path)) {
                Some(thumbs::Kind::Video) => advanced_bool(&st, "thumbs.videos", true),
                Some(thumbs::Kind::Document) => advanced_bool(&st, "thumbs.documents", true),
                _ => false,
            };
            if !allowed {
                continue;
            }
            let Ok(permit) = st.slow_thumb_permits.clone().acquire_owned().await else { return };
            let st2 = st.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _permit = permit;
                let opts = thumbs::Options { max_image_bytes: 0, video_percent: advanced_u64(&st2, "thumbs.videoPercent", 10).min(95) as u8 };
                let _ = thumbs::thumbnail_with(Path::new(&path), size, Some(st2.paths.thumbs.as_path()), &opts);
            });
        }
    });
    Ok(())
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

fn device_settings(st: &Shared) -> (bool, bool) {
    let s = st.settings.read();
    let show = s.advanced.get("devices.show").and_then(|v| v.as_bool()).unwrap_or(true);
    let other = s.advanced.get("devices.showOther").and_then(|v| v.as_bool()).unwrap_or(true);
    (show, other)
}

/// Phones, cameras and other connected devices (not drives).
/// Whether the computer is on a cable, on Wi-Fi, or it could not be told.
#[tauri::command]
pub async fn network_link() -> CmdResult<fast_core::network::Link> {
    blocking(|| Ok(fast_core::network::link())).await
}

#[tauri::command]
pub async fn get_devices(state: State<'_, Shared>) -> CmdResult<Vec<fast_core::devices::Device>> {
    let st = state.inner().clone();
    blocking(move || {
        let (show, other) = device_settings(&st);
        let list = if show { fast_core::devices::list_devices(other) } else { Vec::new() };
        *st.devices.write() = list.clone();
        Ok(list)
    })
    .await
}

/// Mount a device the system knows about (a phone, on Linux) and return the folder it opens at.
#[tauri::command]
pub async fn connect_device(id: String) -> CmdResult<Option<String>> {
    blocking(move || {
        #[cfg(target_os = "linux")]
        {
            fast_core::devices::connect(&id).map_err(err)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = id;
            Err("This device cannot be opened from inside the app on this system.".to_string())
        }
    })
    .await
}

/// Open a device in the system's own tool (File Explorer, Image Capture, ...).
#[tauri::command]
pub fn open_device_in_system(open: String) -> CmdResult<()> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        fast_core::devices::open_in_system(&open).map_err(err)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = open;
        Err("There is no system tool to hand this device to.".to_string())
    }
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

/// Everything the index leaves out: the built-in skips plus the folders and drives with caching off.
fn index_skip(st: &Shared) -> Vec<PathBuf> {
    let mut skip = fast_core::index::default_skip_list();
    skip.extend(cache_rules(st).into_iter().map(|r| {
        let t = r.trim_end_matches(['/', '\\']);
        PathBuf::from(if t.is_empty() { r.as_str() } else { t })
    }));
    skip
}

fn index_roots(st: &Shared) -> Vec<PathBuf> {
    let rules = cache_rules(st);
    let all = index_roots_all(st);
    all.into_iter().filter(|r| !fast_core::cachepolicy::excluded(&rules, r)).collect()
}

/// The places search would index, before caching rules are applied.
fn index_roots_all(st: &Shared) -> Vec<PathBuf> {
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
                    &ScanOptions { threads: per_root_threads, max_entries: per_root_cap, skip: index_skip(st), ..Default::default() },
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

/// Remember that these folders changed, so the index can re-read just them.
fn note_index_change(st: &Shared, dirs: &[PathBuf]) {
    let mut pending = st.index_pending.lock();
    for d in dirs {
        // Only folders some drive's index covers are worth re-reading.
        if st.index.read().has_root_for(d) {
            pending.0.insert(d.clone());
        }
    }
    if !pending.0.is_empty() {
        pending.1 = Some(std::time::Instant::now());
        st.index_wake.notify_one();
    }
}

/// Quiet period after the last change before the index is updated, so a
/// burst (a build, an unzip, a big copy) costs one update instead of many.
const INDEX_QUIET: std::time::Duration = std::time::Duration::from_millis(1500);
/// Saving a drive's index to disk is slow for big drives; changes are saved at most this often.
const INDEX_SAVE_EVERY: std::time::Duration = std::time::Duration::from_secs(300);
/// If more folders than this changed at once, a full refresh is cheaper than re-reading each.
const INDEX_MAX_FOLDERS: usize = 400;

/// Keep search current between full rescans: wait for changes to settle,
/// then re-read only the folders that changed (`Index::refreshed_dir`) and
/// swap the result in. While a full rescan runs, changes wait for it: it will
/// see them anyway.
fn run_index_updates(app: AppHandle, st: Shared) {
    let mut unsaved: std::collections::HashSet<PathBuf> = Default::default();
    let mut last_save = std::time::Instant::now();
    loop {
        let dirs: Vec<PathBuf> = {
            let mut pending = st.index_pending.lock();
            while pending.0.is_empty() {
                if !unsaved.is_empty() && last_save.elapsed() >= INDEX_SAVE_EVERY {
                    break;
                }
                // Wake for changes, or when saved changes are due to be written.
                let timeout = if unsaved.is_empty() { std::time::Duration::from_secs(3600) } else { INDEX_SAVE_EVERY.saturating_sub(last_save.elapsed()).max(std::time::Duration::from_secs(1)) };
                st.index_wake.wait_for(&mut pending, timeout);
            }
            // Let a burst settle.
            while let Some(at) = pending.1 {
                let left = INDEX_QUIET.saturating_sub(at.elapsed());
                if left.is_zero() {
                    break;
                }
                st.index_wake.wait_for(&mut pending, left);
            }
            if st.index_busy.load(Ordering::SeqCst) {
                // A full scan is running; try again after it.
                drop(pending);
                std::thread::sleep(std::time::Duration::from_secs(2));
                continue;
            }
            pending.1 = None;
            std::mem::take(&mut pending.0).into_iter().collect()
        };

        if !dirs.is_empty() && dirs.len() <= INDEX_MAX_FOLDERS {
            let skip = index_skip(&st);
            let mut changed = false;
            // Parents first, so a refreshed parent does not undo work on a child's entry.
            let mut dirs = dirs;
            dirs.sort();
            for dir in dirs {
                if cache_off(&st, &dir) {
                    continue;
                }
                let set = st.index.read().clone();
                let Some(part) = set.parts().iter().find(|p| p.covers(&dir) && p.find_path(&dir).is_some()).cloned() else { continue };
                let Some(updated) = part.refreshed_dir(&dir, &skip) else { continue };
                let root = part.roots.first().map(|&r| PathBuf::from(part.name(r))).unwrap_or_default();
                let mut w = st.index.write();
                *w = Arc::new(w.with_part(Arc::new(updated)));
                unsaved.insert(root);
                changed = true;
            }
            if changed {
                publish_index_status(&app, &st, "ready");
            }
        }

        if !unsaved.is_empty() && last_save.elapsed() >= INDEX_SAVE_EVERY {
            let set = st.index.read().clone();
            for root in unsaved.drain() {
                if let Some(part) = set.parts().iter().find(|p| p.roots.first().map(|&r| p.name(r) == root.to_string_lossy()) == Some(true)) {
                    let _ = part.save(&part_file(&st, &root));
                }
            }
            last_save = std::time::Instant::now();
        }
    }
}

/// How often the background index is refreshed while the app is open.
const REFRESH_SECS: u64 = 30 * 60;

/// Load the saved index of every drive at once (search works the moment the
/// window opens), then refresh in the background if the budget allows, and
/// again every half hour.
pub fn start_indexing(app: AppHandle, st: Shared) {
    {
        let (app, st) = (app.clone(), st.clone());
        let _ = std::thread::Builder::new().name("index-updates".into()).spawn(move || run_index_updates(app, st));
    }
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
        // Folders with caching off are not in the index, so they are searched live.
        let mut live_roots = live_roots;
        for r in cache_rules(&st) {
            let rp = PathBuf::from(&r);
            let wanted = match &scope_path {
                None => true,
                Some(sp) => fast_core::cachepolicy::is_under(&rp, &sp.to_string_lossy()),
            };
            let nested = live_roots.iter().any(|l| fast_core::cachepolicy::is_under(&rp, &l.to_string_lossy()));
            if wanted && !nested && rp.is_dir() {
                live_roots.push(rp);
            }
        }
        let hits: Vec<Hit> = if st.locks.read().is_empty() { hits } else { hits.into_iter().filter(|h| !is_locked(&st, &h.path)).collect() };
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
        let st_live = st.clone();
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
                        let hits: Vec<Hit> = if st_live.locks.read().is_empty() { hits } else { hits.into_iter().filter(|h| !is_locked(&st_live, &h.path)).collect() };
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
    /// How busy the drive is right now. Only sent while the live graph is on.
    activity: Option<DriveActivity>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DriveActivity {
    read_bps: f64,
    write_bps: f64,
}

/// Should the disk-usage view draw live drive activity? "on" and "off" are the
/// person's choice; "auto" means only on a computer with power to spare.
fn live_activity_wanted(st: &crate::state::AppState) -> bool {
    match st.settings.read().advanced.get("usage.liveActivity").and_then(|v| v.as_str()).unwrap_or("auto") {
        "on" => true,
        "off" => false,
        _ => resources::has_spare_power(&st.budget.read(), &resources::snapshot()),
    }
}

/// The last saved result for a folder, if any: tiny to read, so the view
/// can show it immediately while a fresh scan runs.
#[tauri::command]
pub async fn analysis_snapshot(state: State<'_, Shared>, path: String) -> CmdResult<Option<AnalysisResult>> {
    let st = state.inner().clone();
    blocking(move || {
        if !advanced_bool(&st, "cache.usageSnapshots", true) || fast_core::cachepolicy::touches(&cache_rules(&st), Path::new(&path)) {
            return Ok(None);
        }
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
            let keep = advanced_bool(&st, "cache.usageSnapshots", true) && !fast_core::cachepolicy::touches(&cache_rules(&st), &root);
            let has_complete = !keep || usage::load_snapshot(&st.paths.usage, &root).is_some_and(|(a, _)| a.complete);
            let on_partial = |analysis: Analysis| {
                let taken_at = now_secs();
                if !has_complete {
                    let _ = usage::save_snapshot(&st.paths.usage, &root, &analysis, taken_at);
                }
                let _ = app.emit("analysis-partial", AnalysisResult { path: path.clone(), analysis, taken_at });
            };
            let mut sampler = if live_activity_wanted(&st) { fast_core::driveio::Sampler::new(&root) } else { None };
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
                                activity: sampler.as_mut().map(|s| {
                                    let io = s.sample();
                                    DriveActivity { read_bps: io.read_bps, write_bps: io.write_bps }
                                }),
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
                    if keep {
                        let _ = usage::save_snapshot(&st.paths.usage, &root, &analysis, taken_at);
                        usage::prune_snapshots(&st.paths.usage, 40, 8 * 1024 * 1024);
                    }
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

pub(crate) fn fs_changed(app: &AppHandle, st: &Shared, dirs: Vec<PathBuf>) {
    {
        let mut cache = st.listings.lock();
        for d in &dirs {
            cache.invalidate(d);
        }
    }
    note_index_change(st, &dirs);
    let dirs: Vec<String> = dirs.iter().map(|d| d.to_string_lossy().into_owned()).collect();
    let _ = app.emit("fs-changed", dirs);
}

/// Move items to the Trash right away and wait for it (Undo uses this; a
/// bigger delete goes through `delete_start`, which reports progress).
#[tauri::command]
pub async fn trash_paths(app: AppHandle, state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        platform::trash(&paths).map_err(err)?;
        fs_changed(&app, &st, ops::parents(&paths));
        Ok(())
    })
    .await
}

/// What is in the Recycle Bin / Trash.
#[tauri::command]
pub async fn trash_list() -> CmdResult<fast_core::trashbin::Bin> {
    blocking(|| fast_core::trashbin::list().map_err(err)).await
}

/// Put items from the bin back where they were; says how many were found.
#[tauri::command]
pub async fn trash_restore(app: AppHandle, state: State<'_, Shared>, ids: Vec<String>, parents: Vec<String>) -> CmdResult<usize> {
    let st = state.inner().clone();
    blocking(move || {
        let n = fast_core::trashbin::restore(&ids).map_err(err)?;
        let parents: Vec<PathBuf> = parents.into_iter().map(PathBuf::from).collect();
        fs_changed(&app, &st, parents);
        Ok(n)
    })
    .await
}

/// Delete items in the bin for good.
#[tauri::command]
pub async fn trash_purge(ids: Vec<String>) -> CmdResult<usize> {
    blocking(move || fast_core::trashbin::purge(&ids).map_err(err)).await
}

/// Empty the bin.
#[tauri::command]
pub async fn trash_empty() -> CmdResult<usize> {
    blocking(|| fast_core::trashbin::empty().map_err(err)).await
}

/// Put items back from the Trash; says how many were found there.
#[tauri::command]
pub async fn restore_from_trash(app: AppHandle, state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<usize> {
    let st = state.inner().clone();
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let found = platform::restore_from_trash(&paths).map_err(err)?;
        fs_changed(&app, &st, ops::parents(&paths));
        Ok(found)
    })
    .await
}

/// Make a zip next to the first item (`action` "compress"), or unpack an
/// archive into a new folder named after it or, with `here`, into its own
/// folder ("extract"). Runs in the background and reports on the same card as
/// deletes (`delete-progress`, with `action` set); `delete_control` cancels it.
#[tauri::command]
pub async fn archive_start(app: AppHandle, state: State<'_, Shared>, action: String, paths: Vec<String>, here: bool) -> CmdResult<u64> {
    use fast_core::ops::{DeletePhase, DeleteProgress};
    if action != "compress" && action != "extract" {
        return Err(format!("unknown archive action {action}"));
    }
    let st = state.inner().clone();
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    std::thread::Builder::new()
        .name(format!("archive-{id}"))
        .spawn(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let started = std::time::Instant::now();
            let first_name = paths.first().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let meter = parking_lot::Mutex::new(fast_core::meter::Meter::new(None));
            let predicted = AtomicBool::new(false);
            let snapshot = |phase: DeletePhase, done: u64, total: u64, current: &str, result: &str, error: Option<String>| {
                let secs = started.elapsed().as_secs_f64();
                let stats = (phase != DeletePhase::Scanning).then(|| {
                    let mut m = meter.lock();
                    if total > 0 && !predicted.swap(true, Ordering::Relaxed) {
                        if let Some(first) = paths.first() {
                            m.set_prediction(job_prediction(&st, &action, first, total, 0));
                        }
                    }
                    m.update(secs, done, total, 0, 0).finish(phase == DeletePhase::Done)
                });
                DeleteProgress {
                id,
                phase,
                permanent: false,
                selected: paths.len() as u64,
                items_total: total,
                items_done: done,
                current: current.to_string(),
                errors: error.iter().map(|m| fast_core::transfer::engine::TransferError { path: first_name.clone(), message: m.clone(), needs_elevation: false }).collect(),
                error_count: error.iter().count() as u64,
                elapsed_secs: started.elapsed().as_secs_f64(),
                first_name: first_name.clone(),
                action: action.clone(),
                result: result.to_string(),
                bytes_done: 0,
                bytes_total: 0,
                skipped: 0,
                conflict: None,
                stats,
                method: fast_core::meter::archive_method(&paths.first().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(), action == "compress"),
                }
            };
            let _ = app.emit("delete-progress", snapshot(DeletePhase::Scanning, 0, 0, "", "", None));
            let mut last = std::time::Instant::now();
            let mut report = |done: u64, total: u64, current: &str| {
                if last.elapsed() >= std::time::Duration::from_millis(100) {
                    last = std::time::Instant::now();
                    let _ = app.emit("delete-progress", snapshot(DeletePhase::Running, done, total, current, "", None));
                }
            };
            let outcome: Result<(PathBuf, u64), fast_core::error::CoreError> = if action == "compress" {
                match fast_core::archive::default_zip_path(&paths) {
                    Some(dest) => fast_core::archive::compress_with(&paths, &dest, &cancel, &mut report).map(|m| (m.path, m.files)),
                    None => Err(fast_core::error::CoreError::Invalid("Nothing to compress.".into())),
                }
            } else {
                let archive = paths[0].clone();
                let into = if here { archive.parent().map(Path::to_path_buf) } else { fast_core::archive::default_folder(&archive) };
                match into {
                    Some(into) => fast_core::archive::extract_with(&archive, &into, &cancel, &mut report).map(|u| (u.folder, u.files)),
                    None => Err(fast_core::error::CoreError::Invalid("Cannot choose a folder for this archive.".into())),
                }
            };
            let was_cancelled = cancel.load(Ordering::Relaxed);
            let made = outcome.as_ref().ok().map(|(p, _)| p.clone());
            let final_progress = match outcome {
                Ok((path, files)) => {
                    if let Some(first) = paths.first() {
                        remember_job(&st, &action, first, false, files, 0, started.elapsed().as_secs_f64(), 0);
                    }
                    snapshot(DeletePhase::Done, files, files, "", &path.to_string_lossy(), None)
                }
                Err(_) if was_cancelled => snapshot(DeletePhase::Cancelled, 0, 0, "", "", None),
                Err(e) => snapshot(DeletePhase::Failed, 0, 0, "", "", Some(e.to_string())),
            };
            let _ = app.emit("delete-progress", final_progress);
            st.deletes.lock().remove(&id);
            let mut touched: Vec<PathBuf> = paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
            if let Some(made) = made {
                // The new zip's folder, or the folder that was unpacked into and the one holding it.
                touched.extend(made.parent().map(Path::to_path_buf));
                if action == "extract" {
                    touched.push(made);
                }
            }
            fs_changed(&app, &st, touched);
        })
        .map_err(err)?;
    Ok(id)
}

/// The folders a window has open. They are watched, so a change made by
/// another program refreshes the listing without pressing F5.
#[tauri::command]
pub async fn watch_dirs(state: State<'_, Shared>, window: tauri::WebviewWindow, dirs: Vec<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    let label = window.label().to_string();
    blocking(move || {
        let all: Vec<PathBuf> = {
            let mut sets = st.watch_sets.lock();
            if dirs.is_empty() {
                sets.remove(&label);
            } else {
                sets.insert(label, dirs.into_iter().map(PathBuf::from).collect());
            }
            let mut all: Vec<PathBuf> = sets.values().flatten().cloned().collect();
            all.sort();
            all.dedup();
            all
        };
        st.watcher.set_dirs(all);
        Ok(())
    })
    .await
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

/// What similar earlier jobs of this kind ("delete", "trash", "compress", "extract") took.
pub(crate) fn job_prediction(st: &Shared, kind: &str, path: &Path, files: u64, bytes: u64) -> Option<fast_core::history::Prediction> {
    if !st.settings.read().transfer.remember_device_speeds {
        return None;
    }
    let route = fast_core::meter::route_for(kind, path, &st.drives.read());
    st.history.lock().predict(&fast_core::meter::query_for(route, files, bytes), now_secs())
}

/// Let a finished delete, compress or unpack teach the time estimates, if the person allows it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn remember_job(st: &Shared, kind: &str, path: &Path, is_move: bool, files: u64, bytes: u64, secs: f64, errors: u64) {
    let (remember, limit) = {
        let s = st.settings.read();
        (s.transfer.remember_device_speeds, s.transfer.history_limit)
    };
    if !remember || limit == 0 {
        return;
    }
    let route = fast_core::meter::route_for(kind, path, &st.drives.read());
    let mut history = st.history.lock();
    if history.record_job(&route, is_move, files, bytes, secs, errors, now_secs(), limit) {
        let _ = history.save(&st.paths.history);
    }
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
    for p in spec.sources.iter().chain(std::iter::once(&spec.dest_dir)) {
        guard(&st, &p.to_string_lossy())?;
    }
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let (mut budget, drive_list) = (st.budget.read().clone(), st.drives.read().clone());
    let (remember, limit, explore, rules) = {
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
        // The list of files being copied is only tracked when its line is switched on.
        let shown = |id: &str| s.advanced.get(&format!("transfer.show.{id}")).and_then(|v| v.as_bool()).unwrap_or(true);
        spec.early_start_ms = adv_num(&s, "transfer.earlyStartMs", 500).min(10_000) as u64;
        spec.speed_test = s.advanced.get("transfers.speedTest").and_then(|v| v.as_bool()).unwrap_or(false);
        spec.active_files = if shown("activeFiles") { adv_num(&s, "transfer.activeFilesCount", 4).clamp(1, 16) as usize } else { 0 };
        let rules = fast_core::transfer::method::Rules {
            small: fast_core::transfer::method::MethodPref::from_setting(&adv_text(&s, "transfer.smallFilesMethod")),
            large: fast_core::transfer::method::MethodPref::from_setting(&adv_text(&s, "transfer.largeFilesMethod")),
            small_avg_bytes: adv_num(&s, "transfer.smallFileKb", 1024) * 1024,
            small_min_files: adv_num(&s, "transfer.smallFileMinCount", 50),
        };
        (s.transfer.remember_device_speeds, s.transfer.history_limit, s.transfer.try_other_methods, rules)
    };
    // After the sources are scanned, the engine asks what similar past
    // transfers say about this one.
    let predict: Option<Predictor> = (remember && limit > 0).then(|| {
        let st = st.clone();
        Box::new(move |q: &fast_core::history::Query| st.history.lock().predict(q, now_secs())) as Predictor
    });

    // ...and which method (built-in, Robocopy, rsync) did best on jobs like it.
    // A method the person chose for this kind of job wins; otherwise what earlier jobs showed; and with
    // nothing to learn from yet, a rule by the shape of the job (many small files: a tool, big files: built-in).
    let choose: Option<fast_core::transfer::method::MethodChooser> = Some({
        let st = st.clone();
        Box::new(move |q: &fast_core::history::Query, can: &[fast_core::transfer::method::Method]| {
            let by_rule = fast_core::transfer::method::rule_choice(q.files, q.bytes, q.is_move, can, &rules);
            let small = q.files >= rules.small_min_files && q.bytes / q.files.max(1) < rules.small_avg_bytes;
            let fixed = if small { rules.small } else { rules.large };
            if fixed.method().is_some() {
                return by_rule;
            }
            if remember && limit > 0 {
                if let Some(m) = st.history.lock().learned_method(q, can, explore, now_secs()) {
                    return m;
                }
            }
            by_rule
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
    {
        let s = st.settings.read();
        handle.set_priority(transfer::throttle::priority_from(&s.transfer.default_priority));
        handle.set_speed_limit(s.transfer.target_speed_kb * 1024);
        handle.set_time_limit(s.transfer.max_minutes * 60, s.transfer.on_time_limit == "stop");
    }
    let mut transfers = st.transfers.lock();
    transfers.insert(id, handle.clone());
    // A tiny job can finish before the handle is stored; do not leak it.
    if handle.is_finished() {
        transfers.remove(&id);
    }
    Ok(id)
}

/// Change one transfer while it runs: its speed limit (KB per second, 0 = none), its priority
/// ("low", "normal", "high"), its time limit (minutes, 0 = none, stopping or pausing at the end), or
/// leave the queue now. `what` is `speed`, `priority`, `time` or `startNow`.
#[tauri::command]
pub fn transfer_adjust(state: State<'_, Shared>, id: u64, what: String, value: Option<String>, stop: Option<bool>) -> CmdResult<()> {
    let transfers = state.transfers.lock();
    let h = transfers.get(&id).ok_or("transfer not found")?;
    let number = || value.as_deref().unwrap_or("0").trim().parse::<u64>().map_err(|_| "not a number".to_string());
    match what.as_str() {
        "speed" => h.set_speed_limit(number()? * 1024),
        "priority" => h.set_priority(transfer::throttle::priority_from(value.as_deref().unwrap_or("normal"))),
        "time" => h.set_time_limit(number()? * 60, stop.unwrap_or(false)),
        "startNow" => h.start_now(),
        other => return Err(format!("unknown setting {other}")),
    }
    Ok(())
}

/// Apply the transfer settings that are not per transfer: the combined speed cap and the queue length.
pub fn apply_transfer_limits(st: &Shared) {
    let s = st.settings.read();
    transfer::throttle::global_limiter().set_rate(s.transfer.max_speed_kb * 1024);
    transfer::throttle::gate().set_max(s.transfer.max_active);
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
    // Jobs that are not transfers (copying out of an archive) ask through their own slot.
    if let Some(ask) = state.job_asks.lock().get(&id) {
        *ask.answer.lock() = Some((r, all));
        return Ok(());
    }
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
        h.abandon();
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
    for p in &paths {
        guard(&st, p)?;
    }
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    let threads = st.budget.read().io_threads;
    std::thread::Builder::new()
        .name(format!("delete-{id}"))
        .spawn(move || {
            let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let kind = if permanent { "delete" } else { "trash" };
            let first = paths.first().cloned().unwrap_or_default();
            let predict = |files: u64, bytes: u64| job_prediction(&st, kind, &first, files, bytes);
            let last: parking_lot::Mutex<Option<ops::DeleteProgress>> = parking_lot::Mutex::new(None);
            ops::delete_with_meter(id, &paths, permanent, threads, cancel, Some(&predict), |p| {
                *last.lock() = Some(p.clone());
                let _ = app.emit("delete-progress", p);
            });
            if let Some(p) = last.into_inner() {
                if p.phase == ops::DeletePhase::Done {
                    remember_job(&st, kind, &first, true, p.items_done, p.bytes_done, p.elapsed_secs, p.error_count);
                }
            }
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

/// What the Transfers group in Advanced settings shows about copy methods.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodsReport {
    /// Methods this computer can use at all (tools found on it), the built-in engine first.
    pub installed: Vec<fast_core::transfer::method::Method>,
    /// How each has done on this computer's remembered transfers.
    pub stats: Vec<fast_core::history::MethodStat>,
    /// How the other kinds of job (delete, trash, compress, unpack) have done.
    pub jobs: Vec<fast_core::history::JobKindStat>,
    /// Moves inside one drive that were remembered.
    pub instant_moves: u32,
}

#[tauri::command]
pub async fn transfer_methods(state: State<'_, Shared>) -> CmdResult<MethodsReport> {
    let st = state.inner().clone();
    // Looking for the tools runs them once, so it is not done on the interface's thread.
    blocking(move || {
        let h = st.history.lock();
        Ok(MethodsReport { installed: fast_core::transfer::method::installed().to_vec(), stats: h.method_stats(), jobs: h.job_stats(), instant_moves: h.instant_moves() })
    }).await
}

/// One row of extra column values.
#[derive(Serialize)]
pub struct ColumnRow {
    pub name: String,
    pub values: std::collections::BTreeMap<String, String>,
}

/// The values of the extra Details columns (`keys`) for some items of one folder on this computer.
#[tauri::command]
pub async fn column_values(state: State<'_, Shared>, dir: String, names: Vec<String>, keys: Vec<String>) -> CmdResult<Vec<ColumnRow>> {
    use rayon::prelude::*;
    let st = state.inner().clone();
    guard(&st, &dir)?;
    if fast_core::remote::is_remote(&dir) || fast_core::browse::is_loc(&dir) {
        return Err("These details are not available for this kind of location.".into());
    }
    blocking(move || {
        let base = PathBuf::from(dir);
        Ok(names.par_iter().map(|n| ColumnRow { name: n.clone(), values: fast_core::columns::values(&base.join(n), &keys) }).collect())
    })
    .await
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
        guard(&st, &path)?;
        let p = PathBuf::from(path);
        let target = match entry::rename(&p, &new_name) {
            Ok(t) => t,
            // The server would not rename a file it can read (a pooled share, a file that moved behind it):
            // the interface can offer to do it by copying.
            Err(e) if entry::rename_could_copy(&p, &e) => return Err(format!("rename-refused:{e}")),
            Err(e) => return Err(err(e)),
        };
        fs_changed(&app, &st, ops::parents(&[p]));
        Ok(target.to_string_lossy().into_owned())
    })
    .await
}

/// Rename a file by copying it under the new name and removing the original, as a job with progress
/// (see `entry::rename_by_copy`). The result arrives as `delete-progress` with action "rename-copy"; its
/// `result` is the new path.
#[tauri::command]
pub fn rename_by_copy(app: AppHandle, state: State<'_, Shared>, path: String, new_name: String) -> CmdResult<u64> {
    let st = state.inner().clone();
    guard(&st, &path)?;
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let emit = {
        let app = app.clone();
        let name = name.clone();
        let started = Instant::now();
        move |phase: ops::DeletePhase, done: u64, total: u64, error: Option<&str>, result: &str| {
            let _ = app.emit(
                "delete-progress",
                ops::DeleteProgress {
                    id,
                    phase,
                    permanent: false,
                    selected: 1,
                    items_total: 1,
                    items_done: u64::from(phase == ops::DeletePhase::Done),
                    current: name.clone(),
                    errors: error.map(|m| vec![transfer::engine::TransferError { path: name.clone(), message: m.to_string(), needs_elevation: false }]).unwrap_or_default(),
                    error_count: u64::from(error.is_some()),
                    elapsed_secs: started.elapsed().as_secs_f64(),
                    first_name: name.clone(),
                    action: "rename-copy".into(),
                    result: result.to_string(),
                    bytes_done: done,
                    bytes_total: total,
                    skipped: 0,
                    conflict: None,
                    stats: None,
                    method: "Copy under the new name, then remove the original".into(),
                },
            );
        }
    };
    emit(ops::DeletePhase::Scanning, 0, 0, None, "");
    std::thread::Builder::new()
        .name(format!("rename-copy-{id}"))
        .spawn(move || {
            let source = PathBuf::from(&path);
            let mut last = Instant::now() - std::time::Duration::from_secs(1);
            let r = entry::rename_by_copy(&source, &new_name, &cancel, &mut |done, total| {
                if last.elapsed() >= std::time::Duration::from_millis(200) {
                    last = Instant::now();
                    emit(ops::DeletePhase::Running, done, total, None, "");
                }
            });
            match r {
                Ok(Some(target)) => emit(ops::DeletePhase::Done, 1, 1, None, &target.to_string_lossy()),
                Ok(None) => emit(ops::DeletePhase::Cancelled, 0, 0, None, ""),
                Err(e) => emit(ops::DeletePhase::Failed, 0, 0, Some(&err(e)), ""),
            }
            fs_changed(&app, &st, ops::parents(&[source]));
            st.deletes.lock().remove(&id);
        })
        .map_err(|e| e.to_string())?;
    Ok(id)
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
        guard(&st, &dir)?;
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
pub async fn open_item(state: State<'_, Shared>, path: String) -> CmdResult<Option<String>> {
    let st = state.inner().clone();
    blocking(move || {
        guard(&st, &path)?;
        // The order: the app set for this kind of file, then what the system itself does with it,
        // then the program that suits it best, and only when there is none the chooser.
        let local = !path.contains("://") && Path::new(&path).exists();
        let why = if !local {
            None
        } else if let fast_core::openwith::Check::Missing(why) = fast_core::openwith::check(Path::new(&path)) {
            Some(why)
        } else {
            // Something is set (or the system will decide): let the system open it.
            match platform::open_path(Path::new(&path)) {
                Ok(()) => return Ok(None),
                Err(e) => Some(format!("The system could not open this file ({}).", err(e))),
            }
        };
        if let Some(why) = why {
            let choices = fast_core::openwith::choices(Path::new(&path));
            if let Some(app) = fast_core::openwith::likely_app(&choices) {
                if fast_core::openwith::open_with(Path::new(&path), &app.id, false).is_ok() {
                    return Ok(Some(app.name));
                }
            }
            return Err(format!("no-handler:{why}"));
        }
        platform::open_path(Path::new(&path)).map_err(err)?;
        Ok(None)
    })
    .await
}

/// Programs that could open a file: the default, the ones made for it, and the rest.
#[tauri::command]
pub async fn open_choices(path: String) -> CmdResult<fast_core::openwith::Choices> {
    blocking(move || Ok(fast_core::openwith::choices(Path::new(&path)))).await
}

/// The program a file opens with when double-clicked (remembered per kind of file for a couple of minutes).
#[tauri::command]
pub async fn default_app(path: String, icon: bool) -> CmdResult<fast_core::openwith::Opener> {
    blocking(move || Ok(fast_core::openwith::default_opener(Path::new(&path), icon))).await
}

/// Every program on this computer that could be chosen to open a file, by name.
#[tauri::command]
pub async fn installed_apps(path: String) -> CmdResult<Vec<fast_core::openwith::App>> {
    blocking(move || Ok(fast_core::openwith::installed_apps(Path::new(&path)))).await
}

/// Open a file with a chosen program; `always` makes it the default where the system allows.
#[tauri::command]
pub async fn open_with_app(state: State<'_, Shared>, path: String, app: String, always: bool) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        guard(&st, &path)?;
        fast_core::openwith::open_with(Path::new(&path), &app, always).map_err(err)
    })
    .await
}

/// The system's own chooser window (Windows). Other systems answer with an error and the interface uses its own.
#[tauri::command]
pub async fn open_system_chooser(path: String) -> CmdResult<()> {
    blocking(move || fast_core::openwith::system_chooser(Path::new(&path)).map_err(err)).await
}

#[tauri::command]
pub async fn open_terminal(state: State<'_, Shared>, path: String, mode: TerminalMode, shell: Option<String>, login_shell: Option<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let opts = {
            let s = st.settings.read();
            let t = &s.terminal;
            platform::TerminalOptions {
                shell: match platform::os_name() {
                    "windows" => t.windows_shell.clone(),
                    "macos" => t.mac_terminal.clone(),
                    _ => t.linux_terminal.clone(),
                },
                startup: t.startup_command.clone(),
                custom: t.custom_command.clone(),
                profile: t.windows_profile.clone(),
                login_shell: t.shell_program.clone(),
                new_tab: t.new_tab,
            }
        };
        // Typing "cmd" or "powershell" in the address bar asks for that one, whatever the settings prefer.
        let mut opts = opts;
        if let Some(sh) = shell.filter(|s| !s.is_empty()) {
            opts.shell = sh;
            // A shell chosen by name is run as it is, not through a custom command line or a startup command.
            opts.custom.clear();
            opts.profile.clear();
        }
        if let Some(l) = login_shell.filter(|s| !s.is_empty()) {
            opts.login_shell = l;
        }
        // Only a real folder on this computer can hold a terminal (not one inside an archive or on a server).
        if matches!(mode, TerminalMode::Here | TerminalMode::Admin) && !Path::new(&path).is_dir() {
            return Err(format!("\"{path}\" is not a folder on this computer, so a terminal cannot open there."));
        }
        platform::open_terminal(Path::new(&path), mode, &opts).map_err(err)
    })
    .await
}

// ---------------------------------------------------------------------------
// Settings, session, app info
// ---------------------------------------------------------------------------

/// Sent when the set of volumes (or their free space) changed.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DrivesChanged {
    drives: Vec<Drive>,
    devices: Vec<fast_core::devices::Device>,
    /// Display names of the volumes that appeared and disappeared.
    added: Vec<String>,
    removed: Vec<String>,
}

fn drive_watch_settings(st: &Shared) -> (bool, u64, u64) {
    let s = st.settings.read();
    let on = s.advanced.get("drives.watch").and_then(|v| v.as_bool()).unwrap_or(true);
    let poll = s.advanced.get("drives.pollMs").and_then(|v| v.as_u64()).unwrap_or(1500).clamp(250, 60_000);
    let refresh = s.advanced.get("drives.refreshSecs").and_then(|v| v.as_u64()).unwrap_or(10).min(3600);
    (on, poll, refresh)
}

/// Watch for drives being plugged in, removed, mounted or disconnected, and tell the window.
/// A cheap fingerprint is checked every second or two; the volumes are only listed again
/// when it moved, and (less often) to keep the free-space figures current.
pub fn start_drive_watch(app: AppHandle, st: Shared) {
    let _ = std::thread::Builder::new().name("drive-watch".into()).spawn(move || {
        let mut last_sig = drives::signature();
        let mut last_full = Instant::now();
        // Looking at what is plugged in costs far more than checking drive letters, so it runs
        // on every third tick; a drive appearing is still noticed at once.
        let mut tick: u32 = 0;
        let mut dev_sig: u64 = 0;
        loop {
            let (on, poll, refresh) = drive_watch_settings(&st);
            std::thread::sleep(std::time::Duration::from_millis(if on { poll } else { 1000 }));
            if !on {
                continue;
            }
            let (show_dev, show_other) = device_settings(&st);
            if show_dev && tick % 3 == 0 {
                use std::hash::Hasher;
                let mut h = std::collections::hash_map::DefaultHasher::new();
                fast_core::devices::signature(&mut h);
                dev_sig = h.finish();
            }
            tick = tick.wrapping_add(1);
            let sig = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                drives::signature().hash(&mut h);
                if show_dev {
                    dev_sig.hash(&mut h);
                }
                show_dev.hash(&mut h);
                show_other.hash(&mut h);
                h.finish()
            };
            let due = refresh > 0 && last_full.elapsed().as_secs() >= refresh;
            if sig == last_sig && !due {
                continue;
            }
            last_sig = sig;
            last_full = Instant::now();
            let list = drives::list_drives();
            let devices = if show_dev { fast_core::devices::list_devices(show_other) } else { Vec::new() };
            let before = st.drives.read().clone();
            let before_devices = st.devices.read().clone();
            if list == before && devices == before_devices {
                continue;
            }
            let (added, removed) = drives::changes(&before, &list);
            let mut added: Vec<String> = added.iter().map(|d| d.name.clone()).collect();
            let mut removed: Vec<String> = removed.iter().map(|d| d.name.clone()).collect();
            added.extend(devices.iter().filter(|d| !before_devices.iter().any(|o| o.id == d.id)).map(|d| d.name.clone()));
            removed.extend(before_devices.iter().filter(|o| !devices.iter().any(|d| d.id == o.id)).map(|o| o.name.clone()));
            let event = DrivesChanged { added, removed, drives: list.clone(), devices: devices.clone() };
            *st.devices.write() = devices;
            *st.drives.write() = list.clone();
            st.session.lock().drives = list;
            let _ = app.emit("drives-changed", event);
        }
    });
}

pub fn apply_budget(app: &AppHandle, st: &Shared) {
    let (perf_mode, base) = {
        let s = st.settings.read();
        (s.performance.background_mode.clone(), resources::compute(&s.performance, &resources::snapshot()))
    };
    let attention = match st.attention.load(Ordering::Relaxed) {
        0 => resources::Attention::Active,
        1 => resources::Attention::Behind,
        _ => resources::Attention::Away,
    };
    let budget = resources::dimmed(&base, &perf_mode, attention);
    let changed = *st.budget.read() != budget;
    if changed {
        let entering = budget.background && !st.budget.read().background;
        st.listings.lock().set_capacity(budget.listing_cache_dirs);
        *st.budget.write() = budget.clone();
        let _ = app.emit("budget-changed", budget);
        if entering && attention == resources::Attention::Away {
            // Out of sight: give back what is only kept for speed.
            st.listings.lock().clear();
            fast_core::browse::clear_cache();
            #[cfg(all(target_os = "linux", target_env = "gnu"))]
            // SAFETY: malloc_trim only returns unused heap pages to the system.
            unsafe {
                libc::malloc_trim(0);
            }
        }
    }
}

/// Works out whether the main window is in use, behind others, or minimized, and applies the budget.
/// Being behind other windows counts only after a few seconds, so switching to another app and back is free.
#[cfg(desktop)]
pub fn attention_changed(app: &AppHandle) {
    use tauri::Manager;
    let Some(win) = app.get_webview_window("main") else { return };
    let away = win.is_minimized().unwrap_or(false) || !win.is_visible().unwrap_or(true);
    let focused = win.is_focused().unwrap_or(true);
    let st = app.state::<Shared>().inner().clone();
    let level = if away { 2 } else if focused { 0 } else { 1 };
    if level == 1 {
        let app2 = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let st = app2.state::<Shared>().inner().clone();
            if let Some(w) = app2.get_webview_window("main") {
                let away = w.is_minimized().unwrap_or(false) || !w.is_visible().unwrap_or(true);
                let level = if away { 2 } else if w.is_focused().unwrap_or(true) { 0 } else { 1 };
                st.attention.store(level, Ordering::Relaxed);
                apply_budget(&app2, &st);
            }
        });
        return;
    }
    st.attention.store(level, Ordering::Relaxed);
    apply_budget(app, &st);
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
        apply_transfer_limits(&st);
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
            thumbnail_types: thumbs::supported_extensions(),
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
    /// Unix seconds when `system` was read, when this came from the saved copy.
    cached_at: Option<i64>,
}

/// What is kept on disk between runs (the app's own numbers are never stored).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedHardware {
    system: fast_core::sysreport::SystemReport,
    at: i64,
}

fn load_saved(st: &crate::state::AppState) -> Option<SavedHardware> {
    std::fs::read(&st.paths.hardware).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn save_hardware(st: &crate::state::AppState, system: &fast_core::sysreport::SystemReport) -> i64 {
    let at = now_secs();
    if advanced_bool(st, "cache.hardware", true) {
        let _ = fast_core::settings::write_json(&st.paths.hardware, &SavedHardware { system: system.clone(), at });
    }
    at
}

/// Folders and drives where nothing may be remembered (Settings > Storage > Locations).
fn cache_rules(st: &crate::state::AppState) -> Vec<String> {
    fast_core::cachepolicy::rules(&st.settings.read())
}

/// Is saving things about this path switched off?
/// Is this on a network share? Reading those is slow and can stall for seconds, so nothing is
/// done on them speculatively.
fn on_network(st: &crate::state::AppState, path: &Path) -> bool {
    let text = path.to_string_lossy().to_lowercase();
    if text.starts_with("\\\\") {
        return true;
    }
    st.drives.read().iter().any(|d| {
        let mount = d.mount.to_lowercase();
        let mount = mount.trim_end_matches(['/', '\\']);
        d.media == fast_core::drives::Media::Network && !mount.is_empty() && text.starts_with(mount)
    })
}

fn cache_off(st: &crate::state::AppState, path: &Path) -> bool {
    let rules = cache_rules(st);
    !rules.is_empty() && fast_core::cachepolicy::excluded(&rules, path)
}

fn advanced_bool(st: &crate::state::AppState, key: &str, default: bool) -> bool {
    st.settings.read().advanced.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// Keep the thumbnail cache within its limit (Settings > Storage); 0 means no limit.
pub fn trim_thumbnails(st: &crate::state::AppState) {
    let mb = advanced_u64(st, "cache.thumbnailsMaxMb", 512);
    if mb > 0 {
        thumbs::trim_cache(&st.paths.thumbs, mb * 1024 * 1024);
    }
}

fn advanced_u64(st: &crate::state::AppState, key: &str, default: u64) -> u64 {
    st.settings.read().advanced.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

fn app_details(st: &crate::state::AppState) -> AppDetails {
    AppDetails {
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
    }
}

/// Everything the About page shows. Reading drive health and hardware can
/// take a second or two, so a saved copy is used while it is recent enough
/// (Settings > Storage), and `refresh` forces a new reading.
#[tauri::command]
pub async fn system_report(state: State<'_, Shared>, refresh: Option<bool>) -> CmdResult<FullReport> {
    let st = state.inner().clone();
    blocking(move || {
        let keep = advanced_bool(&st, "cache.hardware", true);
        let max_age = advanced_u64(&st, "cache.hardwareMaxAgeHours", 24) * 3600;
        if keep && !refresh.unwrap_or(false) {
            if let Some(saved) = load_saved(&st) {
                let age = (now_secs() - saved.at).max(0) as u64;
                if max_age == 0 || age <= max_age {
                    return Ok(FullReport { app: app_details(&st), system: saved.system, cached_at: Some(saved.at) });
                }
            }
        }
        let system = fast_core::sysreport::collect();
        let at = save_hardware(&st, &system);
        Ok(FullReport { app: app_details(&st), system, cached_at: keep.then_some(at) })
    })
    .await
}

/// Read one drive's SMART data with administrator rights, then update the saved report.
#[tauri::command]
pub async fn disk_smart_admin(state: State<'_, Shared>, device: String) -> CmdResult<FullReport> {
    let st = state.inner().clone();
    blocking(move || {
        let args = fast_core::sysreport::smartctl_args(&device).ok_or("That drive cannot be read this way.")?;
        let program = which_program("smartctl").ok_or("Reading drive health needs smartmontools (smartctl). Install it and try again.")?;
        std::fs::create_dir_all(&st.paths.scratch).map_err(err)?;
        let out = st.paths.scratch.join(format!("smart-{}.json", st.next_id.fetch_add(1, Ordering::Relaxed)));
        // The elevated helper cannot hand output back, so it writes a file only the app reads.
        #[cfg(windows)]
        let (exe, argv) = ("cmd.exe".to_string(), vec!["/c".to_string(), format!("\"{}\" {} > \"{}\"", program.display(), args.join(" "), out.display())]);
        #[cfg(not(windows))]
        let (exe, argv) = {
            let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
            ("sh".to_string(), vec!["-c".to_string(), format!("{} {} > {}", q(&program.to_string_lossy()), args.iter().map(|a| q(a)).collect::<Vec<_>>().join(" "), q(&out.to_string_lossy()))])
        };
        let result = platform::run_elevated(Path::new(&exe), &argv).map_err(err)?;
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        if matches!(result, platform::Elevation::Declined) {
            return Err("Administrator permission was not given.".to_string());
        }
        let smart = fast_core::sysreport::parse_smartctl(&text).ok_or("The drive did not return SMART data (it may not support it).")?;
        let mut system = load_saved(&st).map(|s| s.system).unwrap_or_else(fast_core::sysreport::collect);
        if let Some(d) = system.disks.iter_mut().find(|d| d.device == device) {
            fast_core::sysreport::apply_smart(d, smart);
        }
        let at = save_hardware(&st, &system);
        Ok(FullReport { app: app_details(&st), system, cached_at: Some(at) })
    })
    .await
}

/// Whether the drive-health tool (smartctl) is installed.
#[tauri::command]
pub async fn smartctl_installed() -> bool {
    which_program("smartctl").is_some()
}

/// Install smartmontools with what the system has (winget, Homebrew or the distribution's
/// package manager). The system asks for permission itself where it must.
#[tauri::command]
pub async fn install_smartmontools() -> CmdResult<()> {
    blocking(|| {
        if which_program("smartctl").is_some() {
            return Ok(());
        }
        let manual = "Install smartmontools from https://www.smartmontools.org/wiki/Download and try again.";
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("winget");
            cmd.args(["install", "--id", "smartmontools.smartmontools", "-e", "--silent", "--accept-package-agreements", "--accept-source-agreements"]);
            fast_core::proc::quiet(&mut cmd);
            let out = cmd.output().map_err(|_| format!("Windows' package installer (winget) is not available here. {manual}"))?;
            if !out.status.success() {
                let said = String::from_utf8_lossy(&out.stdout);
                let said = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it did not finish").trim().to_string();
                return Err(format!("The installer stopped: {said}. {manual}"));
            }
        }
        #[cfg(target_os = "macos")]
        {
            let brew = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"].iter().map(Path::new).find(|p| p.is_file()).ok_or(format!("Homebrew is not installed. {manual}"))?;
            let out = std::process::Command::new(brew).args(["install", "smartmontools"]).output().map_err(err)?;
            if !out.status.success() {
                return Err(format!("Homebrew could not install it. {manual}"));
            }
        }
        #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
        {
            let plan: Option<(&str, Vec<&str>)> = [
                ("/usr/bin/apt-get", vec!["install", "-y", "smartmontools"]),
                ("/usr/bin/dnf", vec!["install", "-y", "smartmontools"]),
                ("/usr/bin/pacman", vec!["-S", "--noconfirm", "smartmontools"]),
                ("/usr/bin/zypper", vec!["--non-interactive", "install", "smartmontools"]),
            ]
            .into_iter()
            .find(|(p, _)| Path::new(p).is_file());
            let (program, args) = plan.ok_or(format!("No supported package manager was found. {manual}"))?;
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            match platform::run_elevated(Path::new(program), &args).map_err(err)? {
                platform::Elevation::Declined => return Err("Administrator permission was not given.".to_string()),
                platform::Elevation::Completed(0) => {}
                platform::Elevation::Completed(_) => return Err(format!("The package manager could not install it. {manual}")),
            }
        }
        #[cfg(any(target_os = "android", target_os = "ios"))]
        return Err("Drive health is not available on phones.".to_string());
        // Windows puts the tool where this app's own PATH may not yet include it; which_program looks there too.
        which_program("smartctl").map(|_| ()).ok_or_else(|| format!("It installed, but the app cannot find it yet. Restart the app and try again. {manual}"))
    })
    .await
}

fn which_program(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&exe)).find(|p| p.is_file()))
        .or_else(|| ["/usr/sbin", "/usr/local/sbin", "/opt/homebrew/bin", r"C:\Program Files\smartmontools\bin"].iter().map(|d| Path::new(d).join(&exe)).find(|p| p.is_file()))
}

// ---------------------------------------------------------------------------
// In-app passcodes for folders and drives
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockInfo {
    id: String,
    path: String,
    /// Unlocked right now (until it relocks).
    open: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocksState {
    has_main: bool,
    locks: Vec<LockInfo>,
}

fn locks_state(st: &crate::state::AppState) -> LocksState {
    let relock = relock_after(st);
    let locks = st.locks.read();
    LocksState {
        has_main: locks.file.main.is_some(),
        locks: locks.file.locks.iter().map(|l| LockInfo { id: l.id.clone(), path: l.path.clone(), open: locks.is_open(&l.id, relock) }).collect(),
    }
}

fn save_locks(st: &crate::state::AppState) -> CmdResult<()> {
    st.locks.read().save(&st.paths.locks).map_err(err)
}

#[tauri::command]
pub async fn locks_list(state: State<'_, Shared>) -> CmdResult<LocksState> {
    Ok(locks_state(&state))
}

/// Lock a folder or drive. The first lock may also set the main password.
#[tauri::command]
pub async fn lock_add(state: State<'_, Shared>, path: String, passcode: String, main_password: Option<String>) -> CmdResult<LocksState> {
    let st = state.inner().clone();
    blocking(move || {
        {
            let mut locks = st.locks.write();
            if let Some(main) = main_password.filter(|m| !m.is_empty()) {
                locks.set_main("", &main).map_err(err)?;
            }
            locks.add(&path, &passcode).map_err(err)?;
        }
        save_locks(&st)?;
        st.listings.lock().clear();
        Ok(locks_state(&st))
    })
    .await
}

/// Try a passcode for whatever covers `path`. Returns whether it opened.
#[tauri::command]
pub async fn lock_unlock(state: State<'_, Shared>, path: String, passcode: String) -> CmdResult<bool> {
    let st = state.inner().clone();
    blocking(move || Ok(st.locks.write().unlock(&path, &passcode))).await
}

#[tauri::command]
pub async fn lock_remove(state: State<'_, Shared>, id: String, secret: String) -> CmdResult<LocksState> {
    let st = state.inner().clone();
    blocking(move || {
        st.locks.write().remove(&id, &secret).map_err(err)?;
        save_locks(&st)?;
        Ok(locks_state(&st))
    })
    .await
}

#[tauri::command]
pub async fn lock_set_main(state: State<'_, Shared>, old: String, new: String) -> CmdResult<LocksState> {
    let st = state.inner().clone();
    blocking(move || {
        st.locks.write().set_main(&old, &new).map_err(err)?;
        save_locks(&st)?;
        Ok(locks_state(&st))
    })
    .await
}

#[tauri::command]
pub async fn locks_relock(state: State<'_, Shared>) -> CmdResult<LocksState> {
    state.locks.write().relock_all();
    state.listings.lock().clear();
    Ok(locks_state(&state))
}

// ---------------------------------------------------------------------------
// What the app itself is using
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUsage {
    /// Share of one CPU core the app used since the last reading (can exceed 100 with several threads).
    cpu_percent: f32,
    /// Memory held by the app (resident).
    memory_bytes: u64,
    virtual_bytes: u64,
    /// Bytes the app has read and written since it started.
    disk_read_total: u64,
    disk_write_total: u64,
    threads: usize,
    cpus: usize,
    /// The whole machine's memory, and how much of it is free.
    system_total_bytes: u64,
    system_available_bytes: u64,
    /// What the app keeps for later: caches on disk and in memory (see Storage).
    stored_disk_bytes: u64,
    cached_memory_bytes: u64,
}

#[tauri::command]
pub async fn resource_usage(state: State<'_, Shared>) -> CmdResult<ResourceUsage> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
    let st = state.inner().clone();
    blocking(move || {
        let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
        let mut sys = st.monitor.lock();
        sys.refresh_memory();
        sys.refresh_cpu_all();
        sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::everything());
        let p = sys.process(pid).ok_or("The app's own process could not be read.")?;
        let disk = p.disk_usage();
        let (cpu, mem, virt) = (p.cpu_usage(), p.memory(), p.virtual_memory());
        let (total, avail, cpus) = (sys.total_memory(), sys.available_memory(), sys.cpus().len().max(1));
        drop(sys);
        let disk_bytes: u64 = [&st.paths.thumbs, &st.paths.index, &st.paths.usage, &st.paths.sizes, &st.paths.hardware, &st.paths.scratch, &st.paths.history, &st.paths.predictor]
            .iter()
            .map(|p| disk_footprint(p).0)
            .sum();
        let cached_mem = st.index.read().memory_bytes() as u64 + st.listings.lock().memory_bytes() as u64 + st.folder_sizes.lock().memory_bytes() as u64;
        Ok(ResourceUsage {
            cpu_percent: cpu,
            memory_bytes: mem,
            virtual_bytes: virt,
            disk_read_total: disk.total_read_bytes,
            disk_write_total: disk.total_written_bytes,
            threads: thread_count(),
            cpus,
            system_total_bytes: total,
            system_available_bytes: avail,
            stored_disk_bytes: disk_bytes,
            cached_memory_bytes: cached_mem,
        })
    })
    .await
}

fn thread_count() -> usize {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Give back memory the app is not using: drop cached folder listings and ask
/// the allocator to return freed pages to the system. Returns (before, after).
#[tauri::command]
pub async fn compact_memory(state: State<'_, Shared>) -> CmdResult<(u64, u64)> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
    let st = state.inner().clone();
    blocking(move || {
        let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
        let read = |st: &crate::state::AppState| {
            let mut sys = st.monitor.lock();
            sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::nothing().with_memory());
            sys.process(pid).map(|p| p.memory()).unwrap_or(0)
        };
        let before = read(&st);
        st.listings.lock().clear();
        fast_core::browse::clear_cache();
        #[cfg(all(target_os = "linux", target_env = "gnu"))]
        // SAFETY: malloc_trim only returns unused heap pages to the system.
        unsafe {
            libc::malloc_trim(0);
        }
        Ok((before, read(&st)))
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
        let (icon_bytes, icon_files) = fast_core::iconpacks::footprint(&st.paths.icon_packs);
        let crash_logs = fast_core::crashlog::list(&st.paths.crash);
        let idx = st.index.read().clone();
        let history = st.history.lock().info(&st.paths.history);
        let (size_items, size_mem) = {
            let c = st.folder_sizes.lock();
            (c.len() as u64, c.memory_bytes() as u64)
        };
        let (listing_items, listing_mem) = {
            let l = st.listings.lock();
            (l.len() as u64, l.memory_bytes() as u64)
        };
        Ok(vec![
            CacheEntry {
                id: "thumbnails",
                label: "Thumbnails",
                description: "Small previews of pictures, documents and videos, so folders open with previews already there.",
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
                id: "iconpacks",
                label: "Icon collections",
                description: "Extra icon sets you installed for the icon picker. Icons already chosen keep working; a collection can be downloaded again. Not removed by \"Clear every cache\".",
                disk_bytes: icon_bytes,
                memory_bytes: 0,
                items: icon_files,
                rebuildable: true,
            },
            CacheEntry {
                id: "crashlogs",
                label: "Crash logs",
                description: "Reports written when the app stopped unexpectedly: what it was doing, and basic computer information. Not removed by \"Clear every cache\".",
                disk_bytes: crash_logs.iter().map(|l| l.bytes).sum(),
                memory_bytes: 0,
                items: crash_logs.len() as u64,
                rebuildable: true,
            },
            CacheEntry {
                id: "sizes",
                label: "Folder sizes",
                description: "How big each folder was the last time it was measured, shown at once and refreshed in the background.",
                disk_bytes: disk_footprint(&st.paths.sizes).0,
                memory_bytes: size_mem,
                items: size_items,
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
                id: "hardware",
                label: "Hardware details",
                description: "What About shows about this computer and its drives, saved so the page opens at once.",
                disk_bytes: disk_footprint(&st.paths.hardware).0,
                memory_bytes: 0,
                items: u64::from(st.paths.hardware.is_file()),
                rebuildable: true,
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
/// What forgetting one place removed.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ForgetReport {
    listings: usize,
    sizes: usize,
    thumbnails: usize,
    snapshots: usize,
    predictions: usize,
    index_entries: usize,
    bytes: u64,
}

/// Forget everything saved about one folder or drive (and what is inside it),
/// leaving the rest of every cache alone.
#[tauri::command]
pub async fn clear_cache_for(app: AppHandle, state: State<'_, Shared>, path: String) -> CmdResult<ForgetReport> {
    let st = state.inner().clone();
    blocking(move || {
        let root = PathBuf::from(path.trim());
        if root.as_os_str().is_empty() {
            return Err("no folder given".to_string());
        }
        let mut r = ForgetReport::default();
        r.listings = st.listings.lock().remove_under(&root);
        {
            let mut c = st.folder_sizes.lock();
            r.sizes = c.remove_under(&root);
            let _ = c.save(&st.paths.sizes);
        }
        {
            let mut p = st.predictor.lock();
            r.predictions = p.remove_under(&root);
            let _ = p.save(&st.paths.predictor);
        }
        let (files, bytes) = thumbs::clear_under(&st.paths.thumbs, &root);
        r.thumbnails = files;
        r.bytes += bytes;
        let (files, bytes) = usage::remove_snapshots_under(&st.paths.usage, &root);
        r.snapshots = files;
        r.bytes += bytes;
        let (set, gone) = st.index.read().without_under(&root);
        if gone > 0 {
            *st.index.write() = Arc::new(set.clone());
            r.index_entries = gone;
            // Save what is left so the forgotten entries are not loaded again next time.
            for part in set.parts() {
                if let Some(&first) = part.roots.first() {
                    let _ = part.save(&part_file(&st, Path::new(part.name(first))));
                }
            }
            // A drive that went away entirely: its file must go too.
            for root_path in index_roots_all(&st) {
                if fast_core::cachepolicy::is_under(&root_path, &root.to_string_lossy()) {
                    let _ = std::fs::remove_file(part_file(&st, &root_path));
                }
            }
            publish_index_status(&app, &st, "ready");
        }
        Ok(r)
    })
    .await
}

#[tauri::command]
pub async fn clear_cache(app: AppHandle, state: State<'_, Shared>, id: String) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let all = id == "all";
        let wants = |name: &str| all || id == name;
        if wants("listings") {
            st.listings.lock().clear();
        }
        if wants("sizes") {
            let mut c = st.folder_sizes.lock();
            c.clear();
            let _ = c.save(&st.paths.sizes);
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
        if wants("hardware") {
            let _ = std::fs::remove_file(&st.paths.hardware);
        }
        if id == "iconpacks" {
            fast_core::iconpacks::clear(&st.paths.icon_packs);
        }
        if id == "crashlogs" {
            fast_core::crashlog::clear(&st.paths.crash);
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
fn run_tool(program: &str, args: &[&str]) -> CmdResult<String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let out = cmd.output().map_err(|e| format!("Could not run {program}: {e}"))?;
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

/// The block device behind a Linux mount point, from /proc/mounts.
#[cfg(all(unix, not(target_os = "macos")))]
fn device_of(mount: &str) -> Option<String> {
    let text = std::fs::read_to_string("/proc/mounts").ok()?;
    text.lines().find_map(|l| {
        let mut f = l.split_whitespace();
        let (dev, at) = (f.next()?, f.next()?);
        // Mount points with spaces are written as \040.
        (at.replace("\\040", " ") == mount && dev.starts_with("/dev/")).then(|| dev.to_string())
    })
}

/// Safely remove a detachable drive: unmount it, and power the device down
/// where the system supports that. Only drives the system reports as removable
/// can be ejected, so a system volume is never touched by mistake.
#[tauri::command]
pub async fn drive_eject(app: AppHandle, state: State<'_, Shared>, mount: String) -> CmdResult<()> {
    let st = state.inner().clone();
    let allowed = st.drives.read().iter().any(|d| d.mount == mount && (d.removable || d.media == fast_core::drives::Media::Removable));
    if !allowed {
        return Err("This drive is not a detachable one.".into());
    }
    blocking(move || {
        #[cfg(windows)]
        {
            let letter = mount.trim_end_matches('\\').to_string();
            if letter.len() != 2 || !letter.ends_with(':') || !letter.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                return Err("Unexpected drive name.".to_string());
            }
            let script = format!("$s = New-Object -ComObject Shell.Application; $s.Namespace(17).ParseName('{letter}').InvokeVerb('Eject')");
            run_tool("powershell", &["-NoProfile", "-NonInteractive", "-Command", &script])?;
        }
        #[cfg(target_os = "macos")]
        run_tool("diskutil", &["eject", &mount])?;
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let dev = device_of(&mount);
            let by_udisks = dev.as_ref().map(|d| run_tool("udisksctl", &["unmount", "-b", d]).is_ok()).unwrap_or(false);
            if by_udisks {
                // Power down the whole device (the parent of a partition), if the system allows it.
                if let Some(d) = &dev {
                    let parent = run_tool("lsblk", &["-no", "pkname", d]).ok().filter(|p| !p.is_empty()).map(|p| format!("/dev/{p}")).unwrap_or_else(|| d.clone());
                    let _ = run_tool("udisksctl", &["power-off", "-b", &parent]);
                }
            } else {
                run_tool("umount", &[&mount]).map_err(|e| format!("Could not remove the drive: {e}"))?;
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
pub async fn shell_menu(paths: Vec<String>, background: bool, max_age_secs: Option<u64>) -> CmdResult<Vec<fast_core::shellmenu::ShellItem>> {
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        // A menu built within `max_age_secs` is reused (0 or none: always built fresh).
        let max_age = std::time::Duration::from_secs(max_age_secs.unwrap_or(0));
        fast_core::shellmenu::build_cached(paths, background, max_age).map_err(err)
    })
    .await
}

/// Run an entry of the menu shown for `paths`. The menu is built again first if the one alive in the system is for
/// something else (the menu shown came from memory).
#[tauri::command]
pub async fn shell_invoke(paths: Vec<String>, background: bool, id: u32, label: String) -> CmdResult<()> {
    blocking(move || {
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        fast_core::shellmenu::invoke_for(paths, background, id, &label).map_err(err)
    })
    .await
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

/// The fonts installed on this computer.
#[tauri::command]
pub async fn list_fonts() -> Vec<fast_core::fonts::InstalledFont> {
    tauri::async_runtime::spawn_blocking(fast_core::fonts::installed).await.unwrap_or_default()
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

/// Start a program with the given arguments in a folder (a user command or the command line). Never through a shell.
#[tauri::command]
pub async fn run_program(program: String, args: Vec<String>, cwd: Option<String>) -> CmdResult<()> {
    blocking(move || {
        if program.trim().is_empty() {
            return Err("There is no program to run.".into());
        }
        let mut cmd = std::process::Command::new(program.trim());
        cmd.args(&args).stdin(std::process::Stdio::null());
        if let Some(dir) = cwd.filter(|d| !d.is_empty() && Path::new(d).is_dir()) {
            cmd.current_dir(dir);
        }
        cmd.spawn().map_err(|e| format!("{program}: {e}"))?;
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
    if cfg!(mobile) {
        return Err(ONE_WINDOW.into());
    }
    let n = state.next_id.fetch_add(1, Ordering::Relaxed);
    let script = format!("window.__OPEN__ = {};", serde_json::to_string(&location).map_err(err)?);
    let mut builder = WebviewWindowBuilder::new(&app, format!("window-{n}"), WebviewUrl::App("index.html".into()))
        .title("FAST File Explorer")
        .inner_size(1100.0, 720.0)
        .min_inner_size(520.0, 360.0)
        .background_color(crate::theme::for_new_window(&app))
        .initialization_script(&script);
    #[cfg(target_os = "macos")]
    {
        builder = builder.decorations(true).title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    }
    #[cfg(all(desktop, not(target_os = "macos")))]
    {
        builder = builder.decorations(false);
    }
    if let (Some(x), Some(y)) = (x, y) {
        builder = builder.position((x - 120.0).max(0.0), (y - 20.0).max(0.0));
    }
    builder.build().map_err(err)?;
    Ok(())
}


/// Phones and tablets show the app in one window.
const ONE_WINDOW: &str = "This device shows the app in one window, so panels and tabs stay inside it.";

/// Logical height of the tab strip, used to decide whether a pointer is over another window's tabs.
const TAB_BAR_HEIGHT: f64 = 46.0;

/// The other app window whose tab strip is under a screen position (physical px):
/// its label, its left and top edges, and its scale factor.
fn tab_bar_under(app: &AppHandle, source: &str, px: f64, py: f64) -> Option<(String, f64, f64, f64)> {
    use tauri::Manager;
    for (label, target) in app.webview_windows() {
        if label == source || !(label == "main" || label.starts_with("window-")) {
            continue;
        }
        if target.is_minimized().unwrap_or(false) || !target.is_visible().unwrap_or(true) {
            continue;
        }
        let (Ok(pos), Ok(size), Ok(ts)) = (target.outer_position(), target.outer_size(), target.scale_factor()) else { continue };
        let (left, top) = (pos.x as f64, pos.y as f64);
        if px >= left && px <= left + size.width as f64 && py >= top && py <= top + TAB_BAR_HEIGHT * ts {
            return Some((label, left, top, ts));
        }
    }
    None
}

/// While a tab is being dragged outside its own window: tell the window under
/// the pointer (if its tab strip is there) where the tab would land, so it can
/// outline the slot and move its tabs aside. `active: false` clears every outline.
#[tauri::command]
pub async fn tab_hover(app: AppHandle, window: tauri::WebviewWindow, x: f64, y: f64, width: f64, active: bool) -> CmdResult<()> {
    use tauri::Manager;
    let scale = window.scale_factor().map_err(err)?;
    let hit = if active { tab_bar_under(&app, window.label(), x * scale, y * scale) } else { None };
    for (label, _) in app.webview_windows() {
        if !(label == "main" || label.starts_with("window-")) {
            continue;
        }
        let payload = match &hit {
            Some((l, left, _, ts)) if *l == label => serde_json::json!({ "x": (x * scale - left) / ts, "w": width }),
            _ => serde_json::json!({ "x": serde_json::Value::Null, "w": width }),
        };
        let _ = app.emit_to(label.as_str(), "tab-hover", payload);
    }
    Ok(())
}

/// A tab was let go at screen position (x, y) of the calling window. If that
/// is over the tab bar of another app window, hand the tab to it (it picks the
/// slot from the position) and report true; the caller then removes its copy.
/// With `close_source` the calling window had only that tab, so it closes.
#[tauri::command]
pub async fn drop_tab(app: AppHandle, window: tauri::WebviewWindow, location: String, x: f64, y: f64, close_source: bool) -> CmdResult<bool> {
    use tauri::Manager;
    let scale = window.scale_factor().map_err(err)?;
    let (px, py) = (x * scale, y * scale);
    let Some((label, left, top, ts)) = tab_bar_under(&app, window.label(), px, py) else { return Ok(false) };
    let Some(target) = app.get_webview_window(&label) else { return Ok(false) };
    let payload = serde_json::json!({ "location": location, "x": (px - left) / ts, "y": (py - top) / ts });
    app.emit_to(label.as_str(), "tab-drop", payload).map_err(err)?;
    #[cfg(desktop)]
    let _ = target.unminimize();
    let _ = target.set_focus();
    if close_source {
        let _ = window.close();
    }
    Ok(true)
}

/// Open one of the app's panels as its own native window (it moves, resizes
/// and closes like any window). Asking again focuses the open one. Windows
/// are labelled by `kind`, and every window is told when one opens or closes.
fn open_detached(app: &AppHandle, kind: &'static str, title: &str, size: (f64, f64), min: (f64, f64)) -> CmdResult<()> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if cfg!(mobile) {
        return Err(ONE_WINDOW.into());
    }
    if let Some(w) = app.get_webview_window(kind) {
        #[cfg(desktop)]
        let _ = w.unminimize();
        w.set_focus().map_err(err)?;
        return Ok(());
    }
    let win = WebviewWindowBuilder::new(app, kind, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(size.0, size.1)
        .min_inner_size(min.0, min.1)
        .resizable(true)
        .background_color(crate::theme::for_new_window(app))
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
    #[cfg(desktop)]
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

/// An advanced text setting (a choice), or "auto".
fn adv_text(s: &fast_core::settings::Settings, key: &str) -> String {
    s.advanced.get(key).and_then(|v| v.as_str()).unwrap_or("auto").to_string()
}

fn adv_num(s: &fast_core::settings::Settings, key: &str, default: u64) -> u64 {
    s.advanced.get(key).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))).unwrap_or(default)
}
