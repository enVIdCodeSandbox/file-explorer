//! Copy/move engine. One coordinator thread per job scans the sources, picks
//! a [`Plan`], runs worker threads and reports [`Progress`] about ten times a
//! second. Nothing here touches the UI thread.

use super::eta::{DisplayMode, EtaModel, EtaParams, EtaTracker};
use crate::history::{Prediction, Query, Route};
use super::strategy::{self, JobProfile, Plan, Strategy};
use crate::drives::{self, Drive};
use crate::resources::Budget;
use crossbeam_channel as chan;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Op {
    Copy,
    Move,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    Skip,
    Overwrite,
    KeepBoth,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransferSpec {
    pub sources: Vec<PathBuf>,
    pub dest_dir: PathBuf,
    pub op: Op,
    pub conflict: Conflict,
    #[serde(default = "yes")]
    pub preserve_times: bool,
    /// 0 = use the default threshold.
    #[serde(default)]
    pub large_threshold: u64,
    /// 0 = automatic.
    #[serde(default)]
    pub max_parallel: usize,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Scanning,
    Running,
    Paused,
    Done,
    Cancelled,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TransferError {
    pub path: String,
    pub message: String,
    pub needs_elevation: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: u64,
    pub op: Op,
    pub phase: Phase,
    pub strategy: Option<Strategy>,
    pub dest_dir: String,
    pub files_total: u64,
    pub files_done: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub skipped: u64,
    pub bytes_per_sec: f64,
    pub files_per_sec: f64,
    pub eta_secs: Option<f64>,
    /// How far to trust `eta_secs`, 0 (rough guess) to 1 (settled).
    pub eta_confidence: f64,
    pub display: DisplayMode,
    /// Share of the remaining time that is per-file overhead (0..=1).
    pub overhead_share: f64,
    pub current: String,
    pub errors: Vec<TransferError>,
    pub error_count: u64,
    pub elapsed_secs: f64,
    /// Final learned parameters, present once the job is done.
    pub learned: Option<EtaParams>,
    /// How far along the job is, 0..=1. Only ever increases: it is derived
    /// from counters that cannot go backwards, with weights fixed at the start.
    pub fraction: f64,
    /// Averages since copying began.
    pub avg_bytes_per_sec: f64,
    pub avg_files_per_sec: f64,
    pub peak_bytes_per_sec: f64,
    pub peak_files_per_sec: f64,
    /// The drives involved (known once the sources have been scanned).
    pub route: Option<Route>,
    /// The algorithm chosen for this job.
    pub plan: Option<Plan>,
    /// What similar past transfers predict for this job.
    pub prediction: Option<Prediction>,
}

/// Asked once, after the sources are scanned, what similar past transfers say.
pub type Predictor = Box<dyn FnOnce(&Query) -> Option<Prediction> + Send>;

/// Rough per-file and per-MB costs used only to weight the progress bar when
/// nothing is known about the devices yet.
fn default_costs(src: crate::drives::Media, dst: crate::drives::Media) -> EtaParams {
    use crate::drives::Media::*;
    match (src, dst) {
        (Network, _) | (_, Network) => EtaParams { per_file_secs: 0.010, per_mb_secs: 0.020 },
        (Hdd, _) | (_, Hdd) => EtaParams { per_file_secs: 0.008, per_mb_secs: 0.012 },
        _ => EtaParams { per_file_secs: 0.001, per_mb_secs: 0.004 },
    }
}

const MAX_REPORTED_ERRORS: usize = 50;

const ST_RUN: u8 = 0;
const ST_PAUSE: u8 = 1;
const ST_CANCEL: u8 = 2;

struct Shared {
    control: AtomicU8,
    files_done: AtomicU64,
    bytes_done: AtomicU64,
    skipped: AtomicU64,
    error_count: AtomicU64,
    active: AtomicUsize,
    clone_ok: AtomicBool,
    errors: Mutex<Vec<TransferError>>,
    current: Mutex<String>,
    model: Mutex<EtaModel>,
}

impl Shared {
    fn cancelled(&self) -> bool {
        self.control.load(Ordering::Relaxed) == ST_CANCEL
    }
    /// Block while paused. Returns false if the job was cancelled.
    fn checkpoint(&self) -> bool {
        loop {
            match self.control.load(Ordering::Relaxed) {
                ST_CANCEL => return false,
                ST_PAUSE => std::thread::sleep(Duration::from_millis(50)),
                _ => return true,
            }
        }
    }
    fn error(&self, path: &Path, e: &io::Error) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        let mut errs = self.errors.lock();
        if errs.len() < MAX_REPORTED_ERRORS {
            errs.push(TransferError {
                path: path.to_string_lossy().into_owned(),
                message: e.to_string(),
                needs_elevation: e.kind() == io::ErrorKind::PermissionDenied,
            });
        }
    }
}

/// Handle for controlling a running transfer.
#[derive(Clone)]
pub struct TransferHandle {
    pub id: u64,
    shared: Arc<Shared>,
    done: Arc<AtomicBool>,
}

impl TransferHandle {
    pub fn pause(&self) {
        let _ = self.shared.control.compare_exchange(ST_RUN, ST_PAUSE, Ordering::Relaxed, Ordering::Relaxed);
    }
    pub fn resume(&self) {
        let _ = self.shared.control.compare_exchange(ST_PAUSE, ST_RUN, Ordering::Relaxed, Ordering::Relaxed);
    }
    pub fn cancel(&self) {
        self.shared.control.store(ST_CANCEL, Ordering::Relaxed);
    }
    pub fn is_finished(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
    /// Block until the job ends. Mainly for tests and the elevated helper.
    pub fn wait(&self) {
        while !self.is_finished() {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

struct Item {
    src: PathBuf,
    dst: PathBuf,
    size: u64,
}

/// Start a transfer on background threads. `on_progress` is called from the
/// coordinator thread roughly every 100 ms and once more with the final state.
pub fn start(
    id: u64,
    spec: TransferSpec,
    budget: Budget,
    drives: Vec<Drive>,
    predict: Option<Predictor>,
    on_progress: impl Fn(Progress) + Send + 'static,
) -> TransferHandle {
    let shared = Arc::new(Shared {
        control: AtomicU8::new(ST_RUN),
        files_done: AtomicU64::new(0),
        bytes_done: AtomicU64::new(0),
        skipped: AtomicU64::new(0),
        error_count: AtomicU64::new(0),
        active: AtomicUsize::new(0),
        clone_ok: AtomicBool::new(true),
        errors: Mutex::new(Vec::new()),
        current: Mutex::new(String::new()),
        model: Mutex::new(EtaModel::new(None)),
    });
    let done = Arc::new(AtomicBool::new(false));
    let handle = TransferHandle { id, shared: shared.clone(), done: done.clone() };
    std::thread::Builder::new()
        .name(format!("transfer-{id}"))
        .spawn(move || {
            run(id, spec, budget, drives, predict, &shared, &on_progress);
            done.store(true, Ordering::Relaxed);
        })
        .expect("spawn transfer thread");
    handle
}

fn run(
    id: u64,
    spec: TransferSpec,
    budget: Budget,
    drives: Vec<Drive>,
    predict: Option<Predictor>,
    shared: &Arc<Shared>,
    on_progress: &(impl Fn(Progress) + Send + 'static),
) {
    let started = Instant::now();
    let mut progress = Progress {
        id,
        op: spec.op,
        phase: Phase::Scanning,
        strategy: None,
        dest_dir: spec.dest_dir.to_string_lossy().into_owned(),
        files_total: 0,
        files_done: 0,
        bytes_total: 0,
        bytes_done: 0,
        skipped: 0,
        bytes_per_sec: 0.0,
        files_per_sec: 0.0,
        eta_secs: None,
        eta_confidence: 0.0,
        display: DisplayMode::Bytes,
        overhead_share: 0.0,
        current: String::new(),
        errors: Vec::new(),
        error_count: 0,
        elapsed_secs: 0.0,
        learned: None,
        fraction: 0.0,
        avg_bytes_per_sec: 0.0,
        avg_files_per_sec: 0.0,
        peak_bytes_per_sec: 0.0,
        peak_files_per_sec: 0.0,
        route: None,
        plan: None,
        prediction: None,
    };
    on_progress(progress.clone());

    // ---- Phase 1: fast path for moves, then scan what is left ----------------
    let mut items: Vec<Item> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    // Source directories to remove after a copy-based move, deepest last.
    let mut moved_dirs: Vec<PathBuf> = Vec::new();
    let mut renamed_all = spec.op == Op::Move;

    for src in &spec.sources {
        if shared.cancelled() {
            break;
        }
        let Some(name) = src.file_name() else {
            shared.error(src, &io::Error::new(io::ErrorKind::InvalidInput, "cannot transfer a root"));
            continue;
        };
        if spec.dest_dir.starts_with(src) {
            shared.error(src, &io::Error::new(io::ErrorKind::InvalidInput, "destination is inside the source"));
            continue;
        }
        let mut dst = spec.dest_dir.join(name);
        if dst == *src {
            // Copying into the same folder always makes a duplicate; moving is a no-op.
            if spec.op == Op::Move {
                continue;
            }
            dst = unique_name(&dst);
        }
        let meta = match fs::symlink_metadata(src) {
            Ok(m) => m,
            Err(e) => {
                shared.error(src, &e);
                continue;
            }
        };
        if dst.exists() && !(meta.is_dir() && dst.is_dir()) {
            match spec.conflict {
                Conflict::Skip => {
                    shared.skipped.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                Conflict::KeepBoth => dst = unique_name(&dst),
                Conflict::Overwrite => {}
            }
        }
        // A move within one volume is a rename: no data is copied at all.
        if spec.op == Op::Move && !dst.exists() && fs::rename(src, &dst).is_ok() {
            shared.files_done.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        renamed_all = false;
        if meta.is_dir() {
            scan(src, &dst, &mut items, &mut dirs, &mut moved_dirs, shared);
        } else {
            items.push(Item { src: src.clone(), dst, size: meta.len() });
        }
    }

    let large_threshold =
        if spec.large_threshold > 0 { spec.large_threshold } else { strategy::DEFAULT_LARGE_THRESHOLD };
    let bytes_total: u64 = items.iter().map(|i| i.size).sum();
    let renamed = shared.files_done.load(Ordering::Relaxed);
    let files_total = items.len() as u64 + renamed;

    let first_src = spec.sources.first().cloned().unwrap_or_default();
    let src_vol = drives::volume_for(&first_src, &drives).map(|d| d.mount.clone());
    let dst_vol = drives::volume_for(&spec.dest_dir, &drives).map(|d| d.mount.clone());
    let profile = JobProfile {
        file_count: items.len() as u64,
        total_bytes: bytes_total,
        large_bytes: items.iter().filter(|i| i.size >= large_threshold).map(|i| i.size).sum(),
        large_count: items.iter().filter(|i| i.size >= large_threshold).count() as u64,
        same_volume: src_vol.is_some() && src_vol == dst_vol,
        is_move: spec.op == Op::Move,
        src: drives::media_for(&first_src, &drives),
        dst: drives::media_for(&spec.dest_dir, &drives),
    };
    let mut plan = strategy::choose(&profile, &budget, large_threshold, spec.max_parallel);
    if renamed_all && items.is_empty() {
        plan.strategy = Strategy::Rename;
    } else if plan.strategy == Strategy::Rename {
        // The volumes looked identical but rename failed (e.g. subvolumes).
        plan = strategy::choose(&JobProfile { same_volume: false, ..profile }, &budget, large_threshold, spec.max_parallel);
    }
    shared.clone_ok.store(plan.try_clone, Ordering::Relaxed);

    // What do similar past transfers say? That seeds the time estimate before
    // the first file finishes, and is shown as the "typical" time.
    let volume_name = |mount: &Option<String>| {
        mount
            .as_ref()
            .and_then(|m| drives.iter().find(|d| &d.mount == m))
            .map(|d| d.name.clone())
            .or_else(|| mount.clone())
            .unwrap_or_default()
    };
    let route = Route {
        src: src_vol.clone().unwrap_or_default(),
        dst: dst_vol.clone().unwrap_or_default(),
        src_media: profile.src,
        dst_media: profile.dst,
        src_name: volume_name(&src_vol),
        dst_name: volume_name(&dst_vol),
    };
    let prediction = match (predict, plan.strategy) {
        (Some(predict), s) if s != Strategy::Rename && files_total > 0 => predict(&Query {
            route: route.clone(),
            files: items.len() as u64,
            bytes: bytes_total,
            is_move: spec.op == Op::Move,
        }),
        _ => None,
    };
    if let Some(pr) = &prediction {
        shared.model.lock().set_prior(Some(pr.params));
    }
    // Weights for the progress bar, fixed for the whole job so the bar can only
    // move forward: how much of the work is per-file vs per-byte.
    let costs = prediction.map(|p| p.params).unwrap_or_else(|| default_costs(profile.src, profile.dst));
    let file_cost = files_total as f64 * costs.per_file_secs;
    let byte_cost = bytes_total as f64 / (1024.0 * 1024.0) * costs.per_mb_secs;
    let file_weight = if bytes_total == 0 || file_cost + byte_cost <= 0.0 {
        1.0
    } else if files_total <= 1 {
        0.0
    } else {
        file_cost / (file_cost + byte_cost)
    };
    let mut last_fraction = 0.0f64;
    let copy_started = Instant::now();
    let (mut peak_bps, mut peak_fps) = (0f64, 0f64);

    progress.route = Some(route);
    progress.plan = Some(plan.clone());
    progress.prediction = prediction;
    progress.files_total = files_total;
    progress.bytes_total = bytes_total;
    progress.strategy = Some(plan.strategy);
    progress.phase = Phase::Running;

    // ---- Phase 2: create the directory skeleton, then copy -------------------
    for d in &dirs {
        if let Err(e) = fs::create_dir_all(d) {
            shared.error(d, &e);
        }
    }

    let (small_tx, small_rx) = chan::unbounded::<Item>();
    let (large_tx, large_rx) = chan::unbounded::<Item>();
    // Largest first in the large lane so the long poles start early; small
    // files keep scan order, which preserves on-disk locality.
    let (mut large, small): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.size >= large_threshold);
    large.sort_by(|a, b| b.size.cmp(&a.size));
    for i in small {
        let _ = small_tx.send(i);
    }
    for i in large {
        let _ = large_tx.send(i);
    }
    drop((small_tx, large_tx));

    let workers = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|s| {
        let spawn = |n: usize, primary: chan::Receiver<Item>, secondary: chan::Receiver<Item>| {
            for _ in 0..n {
                let (primary, secondary, plan, spec, workers) =
                    (primary.clone(), secondary.clone(), &plan, &spec, workers.clone());
                workers.fetch_add(1, Ordering::Relaxed);
                s.spawn(move || {
                    // Drain our own lane, then the secondary one (if any).
                    for item in primary.iter().chain(secondary.iter()) {
                        if !shared.checkpoint() {
                            break;
                        }
                        shared.active.fetch_add(1, Ordering::Relaxed);
                        process(&item, plan, spec, shared);
                        shared.active.fetch_sub(1, Ordering::Relaxed);
                    }
                    workers.fetch_sub(1, Ordering::Relaxed);
                });
            }
        };
        // Large-lane workers help with small files once the big ones are
        // done. Small-lane workers never take large files: that would exceed
        // the stream count chosen for the device (fatal for HDD throughput).
        let (closed_tx, closed_rx) = chan::bounded::<Item>(0);
        drop(closed_tx);
        spawn(plan.small_workers, small_rx.clone(), closed_rx);
        spawn(plan.large_workers, large_rx.clone(), small_rx.clone());

        // ---- Progress ticker -------------------------------------------------
        let mut last = Instant::now();
        let (mut last_bytes, mut last_files) = (0u64, renamed);
        let (mut bps, mut fps) = (0f64, 0f64);
        let mut tracker = EtaTracker::default();
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let alive = workers.load(Ordering::Relaxed);
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f64();
            let bytes = shared.bytes_done.load(Ordering::Relaxed);
            let files = shared.files_done.load(Ordering::Relaxed);
            let state = shared.control.load(Ordering::Relaxed);
            if state == ST_RUN && dt > 0.0 {
                let alpha = 0.3;
                bps = bps * (1.0 - alpha) + (bytes - last_bytes) as f64 / dt * alpha;
                fps = fps * (1.0 - alpha) + (files - last_files) as f64 / dt * alpha;
            }
            last = now;
            last_bytes = bytes;
            last_files = files;

            let skipped = shared.skipped.load(Ordering::Relaxed);
            let errors = shared.error_count.load(Ordering::Relaxed);
            let files_left = files_total.saturating_sub(files + skipped + errors);
            let bytes_left = bytes_total.saturating_sub(bytes);
            let est = shared.model.lock().estimate(files_left, bytes_left, alive.max(1));

            progress.phase = if state == ST_PAUSE { Phase::Paused } else { Phase::Running };
            progress.files_done = files;
            progress.bytes_done = bytes;
            progress.skipped = skipped;
            progress.bytes_per_sec = bps;
            progress.files_per_sec = fps;
            let copying_now = copy_started.elapsed().as_secs_f64();
            let done_share = {
                let f = if files_total > 0 { (files + skipped + errors) as f64 / files_total as f64 } else { 0.0 };
                let b = if bytes_total > 0 { bytes as f64 / bytes_total as f64 } else { f };
                f.max(b).min(1.0)
            };
            if state == ST_RUN {
                let (eta, conf) = tracker.update(est.map(|e| e.seconds), dt, done_share, copying_now);
                progress.eta_secs = eta;
                progress.eta_confidence = conf;
            }
            if let Some(e) = est {
                progress.display = e.display;
                progress.overhead_share = e.overhead_share;
            }
            progress.current = shared.current.lock().clone();
            progress.error_count = errors;
            progress.errors = shared.errors.lock().clone();
            progress.elapsed_secs = started.elapsed().as_secs_f64();

            let copying = copy_started.elapsed().as_secs_f64().max(0.001);
            peak_bps = peak_bps.max(bps);
            peak_fps = peak_fps.max(fps);
            progress.avg_bytes_per_sec = bytes as f64 / copying;
            progress.avg_files_per_sec = files as f64 / copying;
            progress.peak_bytes_per_sec = peak_bps;
            progress.peak_files_per_sec = peak_fps;
            let by_files = if files_total > 0 { ((files + skipped + errors) as f64 / files_total as f64).min(1.0) } else { 0.0 };
            let by_bytes = if bytes_total > 0 { (bytes as f64 / bytes_total as f64).min(1.0) } else { by_files };
            // Counters only grow, but clamp anyway so nothing can move it back.
            last_fraction = last_fraction.max(file_weight * by_files + (1.0 - file_weight) * by_bytes).min(0.999);
            progress.fraction = last_fraction;
            if alive == 0 {
                break;
            }
            on_progress(progress.clone());
        }
    });

    // ---- Phase 3: finish a copy-based move -----------------------------------
    let cancelled = shared.cancelled();
    if spec.op == Op::Move && !cancelled {
        // Deepest first; remove_dir only succeeds on empty directories, so
        // anything that failed to copy keeps its folder.
        for d in moved_dirs.iter().rev() {
            let _ = fs::remove_dir(d);
        }
    }

    let errors = shared.error_count.load(Ordering::Relaxed);
    progress.files_done = shared.files_done.load(Ordering::Relaxed);
    progress.bytes_done = shared.bytes_done.load(Ordering::Relaxed);
    progress.skipped = shared.skipped.load(Ordering::Relaxed);
    progress.error_count = errors;
    progress.errors = shared.errors.lock().clone();
    progress.elapsed_secs = started.elapsed().as_secs_f64();
    progress.eta_secs = None;
    progress.eta_confidence = 1.0;
    progress.current.clear();
    progress.learned = shared.model.lock().params();
    let copying = copy_started.elapsed().as_secs_f64().max(0.001);
    progress.avg_bytes_per_sec = progress.bytes_done as f64 / copying;
    progress.avg_files_per_sec = progress.files_done as f64 / copying;
    progress.peak_bytes_per_sec = peak_bps.max(progress.avg_bytes_per_sec);
    progress.peak_files_per_sec = peak_fps.max(progress.avg_files_per_sec);
    progress.fraction = if cancelled || errors > 0 && progress.files_done == 0 { last_fraction } else { 1.0 };
    progress.phase = if cancelled {
        Phase::Cancelled
    } else if errors > 0 && progress.files_done == 0 {
        Phase::Failed
    } else {
        Phase::Done
    };
    on_progress(progress);
}

/// Recursively collect files and directories under `src`.
fn scan(
    src: &Path,
    dst: &Path,
    items: &mut Vec<Item>,
    dirs: &mut Vec<PathBuf>,
    moved_dirs: &mut Vec<PathBuf>,
    shared: &Shared,
) {
    if shared.cancelled() {
        return;
    }
    dirs.push(dst.to_path_buf());
    moved_dirs.push(src.to_path_buf());
    let rd = match fs::read_dir(src) {
        Ok(rd) => rd,
        Err(e) => {
            shared.error(src, &e);
            return;
        }
    };
    for e in rd.filter_map(|e| e.ok()) {
        let path = e.path();
        let target = dst.join(e.file_name());
        match e.file_type() {
            Ok(ft) if ft.is_dir() => scan(&path, &target, items, dirs, moved_dirs, shared),
            Ok(_) => {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                items.push(Item { src: path, dst: target, size });
            }
            Err(err) => shared.error(&path, &err),
        }
    }
}

fn process(item: &Item, plan: &Plan, spec: &TransferSpec, shared: &Shared) {
    let mut dst = item.dst.clone();
    if dst.exists() {
        match spec.conflict {
            Conflict::Skip => {
                shared.skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Conflict::KeepBoth => dst = unique_name(&dst),
            Conflict::Overwrite => {}
        }
    }
    if item.size >= plan.large_threshold {
        *shared.current.lock() = item.src.to_string_lossy().into_owned();
    }
    let t0 = Instant::now();
    match copy_one(&item.src, &dst, item.size, plan, spec, shared) {
        Ok(true) => {
            shared.model.lock().observe_file(item.size, t0.elapsed().as_secs_f64());
            shared.files_done.fetch_add(1, Ordering::Relaxed);
            if spec.op == Op::Move {
                if let Err(e) = fs::remove_file(&item.src) {
                    shared.error(&item.src, &e);
                }
            }
        }
        Ok(false) => {
            // Cancelled mid-file: never leave a partial file behind.
            let _ = fs::remove_file(&dst);
        }
        Err(e) => {
            let _ = fs::remove_file(&dst);
            shared.error(&item.src, &e);
        }
    }
}

/// Copy one file. Returns Ok(false) if cancelled part-way.
fn copy_one(src: &Path, dst: &Path, size: u64, plan: &Plan, spec: &TransferSpec, shared: &Shared) -> io::Result<bool> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        copy_symlink(src, dst)?;
        return Ok(true);
    }

    // 1. Copy-on-write clone: instant and space-free where supported.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if shared.clone_ok.load(Ordering::Relaxed) {
        if dst.exists() {
            let _ = fs::remove_file(dst);
        }
        match reflink_copy::reflink(src, dst) {
            Ok(()) => {
                shared.bytes_done.fetch_add(size, Ordering::Relaxed);
                return Ok(true);
            }
            // Unsupported on this filesystem: stop trying for this job.
            Err(_) => shared.clone_ok.store(false, Ordering::Relaxed),
        }
    }

    if size < plan.large_threshold {
        // 2. Small/medium file: one call into the OS fast path
        //    (copy_file_range on Linux, CopyFileEx on Windows, fcopyfile on macOS).
        let n = fs::copy(src, dst)?;
        shared.bytes_done.fetch_add(n, Ordering::Relaxed);
    } else {
        // 3. Large file: chunked copy with big buffers so progress, pause and
        //    cancel stay responsive within one file.
        let mut input = fs::File::open(src)?;
        let mut output = fs::File::create(dst)?;
        // Reserve the space up front: fails fast when the disk is full and
        // reduces fragmentation.
        let _ = output.set_len(size);
        let mut buf = vec![0u8; plan.buffer_bytes];
        let mut chunk_start = Instant::now();
        loop {
            if !shared.checkpoint() {
                return Ok(false);
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n])?;
            shared.bytes_done.fetch_add(n as u64, Ordering::Relaxed);
            let now = Instant::now();
            shared.model.lock().observe_chunk(n as u64, now.duration_since(chunk_start).as_secs_f64());
            chunk_start = now;
        }
        // The source may have shrunk while copying.
        let written = output.stream_position_or(size);
        let _ = output.set_len(written);
        drop(output);
        let _ = fs::set_permissions(dst, meta.permissions());
    }

    if spec.preserve_times {
        let mtime = filetime::FileTime::from_last_modification_time(&meta);
        let atime = filetime::FileTime::from_last_access_time(&meta);
        let _ = filetime::set_file_times(dst, atime, mtime);
    }
    Ok(true)
}

trait StreamPos {
    fn stream_position_or(&mut self, fallback: u64) -> u64;
}
impl StreamPos for fs::File {
    fn stream_position_or(&mut self, fallback: u64) -> u64 {
        use std::io::Seek;
        self.stream_position().unwrap_or(fallback)
    }
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    let target = fs::read_link(src)?;
    if fs::symlink_metadata(dst).is_ok() {
        fs::remove_file(dst)?;
    }
    std::os::unix::fs::symlink(target, dst)
}

#[cfg(not(unix))]
fn copy_symlink(src: &Path, dst: &Path) -> io::Result<()> {
    // Creating symlinks on Windows needs a privilege most users lack, so copy
    // what the link points to instead.
    fs::copy(src, dst).map(|_| ())
}

/// `name.ext` -> `name (2).ext`, `name (3).ext`, ... until unused.
pub fn unique_name(path: &Path) -> PathBuf {
    if fs::symlink_metadata(path).is_err() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 2u32.. {
        let candidate = parent.join(format!("{stem} ({n}){ext}"));
        if fs::symlink_metadata(&candidate).is_err() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn spec(sources: Vec<PathBuf>, dest: &Path, op: Op, conflict: Conflict) -> TransferSpec {
        TransferSpec {
            sources, dest_dir: dest.to_path_buf(), op, conflict,
            preserve_times: true, large_threshold: 64 * 1024, max_parallel: 0,
        }
    }

    fn run_job(s: TransferSpec) -> Progress {
        let last = Arc::new(StdMutex::new(None));
        let l2 = last.clone();
        let budget = Budget { io_threads: 4, ..Budget::minimal() };
        let h = start(1, s, budget, Vec::new(), None, move |p| *l2.lock().unwrap() = Some(p));
        h.wait();
        let p = last.lock().unwrap().clone().unwrap();
        p
    }

    fn make_tree(root: &Path) {
        fs::create_dir_all(root.join("a/b")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        for i in 0..40 {
            fs::write(root.join(format!("a/small{i}.txt")), format!("file {i}")).unwrap();
        }
        fs::write(root.join("a/b/big.bin"), vec![7u8; 300 * 1024]).unwrap();
        fs::write(root.join("top.txt"), b"top").unwrap();
    }

    #[test]
    fn progress_only_moves_forward_and_reports_plan_and_prediction() {
        use crate::history::Prediction;
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        make_tree(src.path());
        let seen: Arc<StdMutex<Vec<Progress>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        let asked = Arc::new(StdMutex::new(None));
        let asked2 = asked.clone();
        let predict: Predictor = Box::new(move |q| {
            *asked2.lock().unwrap() = Some((q.files, q.bytes));
            Some(Prediction {
                params: EtaParams { per_file_secs: 0.001, per_mb_secs: 0.01 },
                typical_secs: 4.0,
                samples: 3,
                same_route: true,
            })
        });
        let budget = Budget { io_threads: 4, ..Budget::minimal() };
        let s = spec(vec![src.path().join("a"), src.path().join("top.txt")], dst.path(), Op::Copy, Conflict::KeepBoth);
        start(1, s, budget, Vec::new(), Some(predict), move |p| sink.lock().unwrap().push(p)).wait();

        let seen = seen.lock().unwrap();
        let fractions: Vec<f64> = seen.iter().map(|p| p.fraction).collect();
        assert!(fractions.windows(2).all(|w| w[1] >= w[0]), "fraction went backwards: {fractions:?}");
        let last = seen.last().unwrap();
        assert_eq!(last.phase, Phase::Done);
        assert_eq!(last.fraction, 1.0);
        assert!(seen.iter().filter(|p| p.phase != Phase::Done).all(|p| p.fraction < 1.0));

        // 40 small files + big.bin + top.txt, asked about once, after scanning.
        assert_eq!(*asked.lock().unwrap(), Some((42, 300 * 1024 + 3 + (0..40).map(|i| format!("file {i}").len() as u64).sum::<u64>())));
        // A job this small can finish before the first "running" update, so the
        // final report carries the plan, route and prediction too.
        assert_eq!(last.prediction.unwrap().samples, 3);
        assert!(last.plan.is_some() && last.route.is_some());
        assert!(last.avg_files_per_sec > 0.0 && last.peak_files_per_sec >= last.avg_files_per_sec);
    }

    #[test]
    fn nothing_is_predicted_for_an_instant_rename() {
        let src = tempfile::tempdir().unwrap();
        make_tree(src.path());
        let dst = src.path().join("moved-here");
        fs::create_dir(&dst).unwrap();
        let called = Arc::new(StdMutex::new(false));
        let c2 = called.clone();
        let predict: Predictor = Box::new(move |_| {
            *c2.lock().unwrap() = true;
            None
        });
        let last = Arc::new(StdMutex::new(None));
        let l2 = last.clone();
        let budget = Budget { io_threads: 4, ..Budget::minimal() };
        let s = spec(vec![src.path().join("a")], &dst, Op::Move, Conflict::KeepBoth);
        start(1, s, budget, Vec::new(), Some(predict), move |p| *l2.lock().unwrap() = Some(p)).wait();
        let p = last.lock().unwrap().clone().unwrap();
        assert_eq!(p.phase, Phase::Done);
        assert_eq!(p.fraction, 1.0);
        assert!(!*called.lock().unwrap());
        assert!(p.prediction.is_none());
    }

    #[test]
    fn copies_a_tree_with_mixed_sizes() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dest = d.path().join("dest");
        make_tree(&src);
        fs::create_dir(&dest).unwrap();
        let old = filetime::FileTime::from_unix_time(1_000_000_000, 0);
        filetime::set_file_mtime(src.join("top.txt"), old).unwrap();

        let p = run_job(spec(vec![src.clone()], &dest, Op::Copy, Conflict::Overwrite));
        assert_eq!(p.phase, Phase::Done);
        assert_eq!(p.files_total, 42);
        assert_eq!(p.files_done, 42);
        assert_eq!(p.error_count, 0);
        assert_eq!(p.bytes_done, p.bytes_total);
        assert_eq!(p.strategy, Some(Strategy::LargeSequential)); // one big file holds >90% of the bytes
        assert!(p.learned.is_some());
        assert_eq!(fs::read(dest.join("src/a/b/big.bin")).unwrap(), vec![7u8; 300 * 1024]);
        assert_eq!(fs::read_to_string(dest.join("src/a/small7.txt")).unwrap(), "file 7");
        assert!(dest.join("src/empty").is_dir());
        let m = fs::metadata(dest.join("src/top.txt")).unwrap();
        assert_eq!(filetime::FileTime::from_last_modification_time(&m), old);
        assert!(src.join("top.txt").exists());
    }

    #[test]
    fn move_within_a_volume_is_a_rename() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dest = d.path().join("dest");
        make_tree(&src);
        fs::create_dir(&dest).unwrap();
        let p = run_job(spec(vec![src.clone()], &dest, Op::Move, Conflict::Overwrite));
        assert_eq!(p.phase, Phase::Done);
        assert_eq!(p.strategy, Some(Strategy::Rename));
        assert!(!src.exists());
        assert!(dest.join("src/a/b/big.bin").exists());
    }

    #[test]
    fn move_into_existing_folder_merges_and_cleans_up() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dest = d.path().join("dest");
        make_tree(&src);
        fs::create_dir_all(dest.join("src/a")).unwrap();
        fs::write(dest.join("src/a/keep.txt"), b"keep").unwrap();
        let p = run_job(spec(vec![src.clone()], &dest, Op::Move, Conflict::Overwrite));
        assert_eq!(p.phase, Phase::Done);
        assert_eq!(p.error_count, 0);
        assert!(!src.exists(), "source tree should be removed after a merged move");
        assert!(dest.join("src/a/keep.txt").exists());
        assert!(dest.join("src/a/small0.txt").exists());
    }

    #[test]
    fn conflict_policies() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let f = d.path().join("doc.txt");
        fs::write(&f, b"new").unwrap();
        fs::write(dest.join("doc.txt"), b"old").unwrap();

        let p = run_job(spec(vec![f.clone()], &dest, Op::Copy, Conflict::Skip));
        assert_eq!(p.skipped, 1);
        assert_eq!(fs::read(dest.join("doc.txt")).unwrap(), b"old");

        run_job(spec(vec![f.clone()], &dest, Op::Copy, Conflict::KeepBoth));
        assert_eq!(fs::read(dest.join("doc.txt")).unwrap(), b"old");
        assert_eq!(fs::read(dest.join("doc (2).txt")).unwrap(), b"new");

        run_job(spec(vec![f.clone()], &dest, Op::Copy, Conflict::Overwrite));
        assert_eq!(fs::read(dest.join("doc.txt")).unwrap(), b"new");

        // Copying into the folder the file already lives in duplicates it.
        run_job(spec(vec![f.clone()], d.path(), Op::Copy, Conflict::Overwrite));
        assert!(d.path().join("doc (2).txt").exists());
    }

    #[test]
    fn rejects_copying_a_folder_into_itself_and_reports_missing_sources() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        fs::create_dir_all(src.join("inner")).unwrap();
        let p = run_job(spec(vec![src.clone()], &src.join("inner"), Op::Copy, Conflict::Overwrite));
        assert_eq!(p.error_count, 1);
        assert_eq!(p.phase, Phase::Failed);

        let p = run_job(spec(vec![d.path().join("missing")], d.path(), Op::Copy, Conflict::Overwrite));
        assert_eq!(p.error_count, 1);
    }

    #[test]
    fn cancel_leaves_no_partial_file() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let big = d.path().join("big.bin");
        fs::write(&big, vec![1u8; 8 * 1024 * 1024]).unwrap();
        let mut s = spec(vec![big], &dest, Op::Copy, Conflict::Overwrite);
        s.large_threshold = 1024;
        let budget = Budget { io_threads: 1, ..Budget::minimal() };
        let h = start(2, s, budget, Vec::new(), None, |_| {});
        h.pause();
        h.cancel();
        h.wait();
        let leftover = dest.join("big.bin");
        assert!(!leftover.exists() || fs::metadata(&leftover).unwrap().len() == 8 * 1024 * 1024);
    }

    #[test]
    fn unique_names_count_up() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.tar.gz");
        assert_eq!(unique_name(&f), f);
        fs::write(&f, b"").unwrap();
        assert_eq!(unique_name(&f), d.path().join("a.tar (2).gz"));
        fs::write(d.path().join("a.tar (2).gz"), b"").unwrap();
        assert_eq!(unique_name(&f), d.path().join("a.tar (3).gz"));
    }
}
