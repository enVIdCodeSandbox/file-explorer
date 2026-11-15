//! File operations that can need administrator rights: delete (with real
//! progress), rename, create and transfers, described as a [`Job`].
//!
//! The flow for an operation that the current user may not be allowed to do:
//!
//! 1. [`check_access`] probes the folders involved *before* anything runs.
//! 2. If access is denied, the app asks the person and, only if they agree,
//!    starts [`run_elevated_job`]. That re-launches this executable through the
//!    OS prompt (UAC, polkit, the macOS authorization dialog) in headless
//!    helper mode ([`helper_main`]).
//! 3. If the person dismisses the OS prompt, nothing runs.
//!
//! The job travels on the helper's command line, not in a file another
//! process could edit between the prompt and the run. Only a job too large for
//! a command line goes through a file, and that file lives in the app's own
//! cache folder.

use crate::platform::{self, Elevation};
use crate::transfer::engine::TransferError;
use crate::transfer::{self, Phase, Progress, TransferSpec};
use crate::{drives, entry, resources, settings::Settings, CoreError, Result};
use base64::Engine;
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Job {
    Transfer { spec: TransferSpec },
    Delete { paths: Vec<PathBuf>, permanent: bool },
    Rename { path: PathBuf, new_name: String },
    Create { dir: PathBuf, name: String, folder: bool },
}

impl Job {
    /// Folders that change when this job runs (their listings go stale).
    pub fn touched_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = match self {
            Job::Transfer { spec } => {
                let mut d = parents(&spec.sources);
                d.push(spec.dest_dir.clone());
                d
            }
            Job::Delete { paths, .. } => parents(paths),
            Job::Rename { path, .. } => parents(std::slice::from_ref(path)),
            Job::Create { dir, .. } => vec![dir.clone()],
        };
        dirs.sort();
        dirs.dedup();
        dirs
    }
}

pub fn parents(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)).collect();
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// Pre-flight access check
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AccessCheck {
    /// The current user is refused somewhere this job needs to touch.
    pub needs_elevation: bool,
    /// Elevated, this job cannot use the user's Trash and would delete for good.
    pub becomes_permanent: bool,
}

/// True when creating a file in `dir` is refused for lack of permission.
/// Other failures (missing folder, read-only media) are not fixed by
/// elevation, so they are left to fail with their own message.
pub fn write_denied(dir: &Path) -> bool {
    use std::sync::atomic::AtomicU32;
    static N: AtomicU32 = AtomicU32::new(0);
    let probe = dir.join(format!(".fast-access-probe-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(f) => {
            drop(f);
            let _ = std::fs::remove_file(&probe);
            false
        }
        Err(e) => e.kind() == io::ErrorKind::PermissionDenied,
    }
}

/// True when `path` cannot be read for lack of permission.
fn read_denied(path: &Path) -> bool {
    let r = if path.is_dir() { std::fs::read_dir(path).map(|_| ()) } else { std::fs::File::open(path).map(|_| ()) };
    matches!(r, Err(e) if e.kind() == io::ErrorKind::PermissionDenied)
}

/// Probe whether `job` would be refused. Cheap: a handful of probes, no scanning.
pub fn check_access(job: &Job) -> AccessCheck {
    let any_write_denied = |dirs: &[PathBuf]| dirs.iter().any(|d| write_denied(d));
    let needs = match job {
        Job::Transfer { spec } => {
            write_denied(&spec.dest_dir)
                || spec.sources.iter().any(|s| read_denied(s))
                // A move also removes the originals.
                || (spec.op == transfer::Op::Move && any_write_denied(&parents(&spec.sources)))
        }
        Job::Delete { paths, .. } => any_write_denied(&parents(paths)),
        Job::Rename { path, .. } => any_write_denied(&parents(std::slice::from_ref(path))),
        Job::Create { dir, .. } => write_denied(dir),
    };
    // Outside Windows an elevated process is root, whose Trash is not the
    // user's, so a Trash request would silently become a permanent delete.
    let becomes_permanent = needs && !cfg!(windows) && matches!(job, Job::Delete { permanent: false, .. });
    AccessCheck { needs_elevation: needs, becomes_permanent }
}

// ---------------------------------------------------------------------------
// Delete with progress
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeletePhase {
    Scanning,
    Running,
    Done,
    Cancelled,
    Failed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProgress {
    pub id: u64,
    pub phase: DeletePhase,
    pub permanent: bool,
    /// Items the person selected.
    pub selected: u64,
    /// Files and folders to remove, counted before deleting starts. Grows
    /// while the phase is `scanning`.
    pub items_total: u64,
    pub items_done: u64,
    pub current: String,
    pub errors: Vec<TransferError>,
    pub error_count: u64,
    pub elapsed_secs: f64,
    /// Name of the first selected item, for the card title.
    pub first_name: String,
    /// "delete", or "compress" / "extract" when the same card shows an archive job.
    pub action: String,
    /// For a finished archive job: the zip or folder that was made.
    pub result: String,
    /// For jobs that move bytes (uploads and downloads): how far along, and the expected total.
    #[serde(default)]
    pub bytes_done: u64,
    #[serde(default)]
    pub bytes_total: u64,
    /// Files left alone because they were already there (by the setting or the person's answer);
    /// not a failure.
    #[serde(default)]
    pub skipped: u64,
    /// A file that exists on both sides and is waiting for the person's decision.
    #[serde(default)]
    pub conflict: Option<crate::transfer::ConflictInfo>,
    /// Speed, time left and the graph's numbers, measured the way a transfer's are.
    #[serde(default)]
    pub stats: Option<crate::meter::JobStats>,
}

const MAX_REPORTED_ERRORS: usize = 50;

struct DeleteCtx {
    cancel: Arc<AtomicBool>,
    total: AtomicU64,
    done: AtomicU64,
    /// Bytes in the files to remove, and how many have gone (permanent deletes only).
    bytes_total: AtomicU64,
    bytes_done: AtomicU64,
    error_count: AtomicU64,
    errors: Mutex<Vec<TransferError>>,
    current: Mutex<String>,
}

impl DeleteCtx {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn fail(&self, path: &Path, e: &io::Error) {
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

/// Count the files and folders under `path` (including `path` itself).
fn count_tree(path: &Path, ctx: &DeleteCtx) {
    if ctx.cancelled() {
        return;
    }
    ctx.total.fetch_add(1, Ordering::Relaxed);
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if !meta.is_dir() {
        ctx.bytes_total.fetch_add(meta.len(), Ordering::Relaxed);
        return;
    }
    let Ok(rd) = std::fs::read_dir(path) else { return };
    let kids: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    kids.par_iter().for_each(|k| count_tree(k, ctx));
}

/// Remove a file, clearing the read-only flag first if that is what stops it.
fn remove_file_forced(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            if let Ok(meta) = std::fs::symlink_metadata(path) {
                let mut perms = meta.permissions();
                if perms.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                    if std::fs::set_permissions(path, perms).is_ok() {
                        return std::fs::remove_file(path);
                    }
                }
            }
            Err(e)
        }
        // A directory symlink or junction on Windows is removed as a directory.
        Err(_) if cfg!(windows) && std::fs::remove_dir(path).is_ok() => Ok(()),
        other => other,
    }
}

fn remove_tree(path: &Path, ctx: &DeleteCtx) {
    if ctx.cancelled() {
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        // Already gone: count it so the bar still reaches the end.
        ctx.done.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if !meta.is_dir() {
        match remove_file_forced(path) {
            Ok(()) => {
                ctx.bytes_done.fetch_add(meta.len(), Ordering::Relaxed);
            }
            Err(e) => ctx.fail(path, &e),
        }
        ctx.done.fetch_add(1, Ordering::Relaxed);
        return;
    }
    match std::fs::read_dir(path) {
        Ok(rd) => {
            let kids: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            if let Some(first) = kids.first() {
                if let Some(parent) = first.parent() {
                    *ctx.current.lock() = parent.to_string_lossy().into_owned();
                }
            }
            kids.par_iter().for_each(|k| remove_tree(k, ctx));
        }
        Err(e) => {
            ctx.fail(path, &e);
            ctx.done.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    if !ctx.cancelled() {
        if let Err(e) = std::fs::remove_dir(path) {
            // Left non-empty because a child failed: that failure is already reported.
            if ctx.error_count.load(Ordering::Relaxed) == 0 {
                ctx.fail(path, &e);
            }
        }
    }
    ctx.done.fetch_add(1, Ordering::Relaxed);
}

/// Asked once the items are counted (how many items, how many bytes) what similar earlier jobs took.
pub type JobPredictor<'a> = &'a (dyn Fn(u64, u64) -> Option<crate::history::Prediction> + Sync);

/// Delete `paths`, reporting progress every ~150 ms through `on_progress`.
/// Blocks until finished. Setting `cancel` stops it between items.
pub fn delete_with_progress(id: u64, paths: &[PathBuf], permanent: bool, threads: usize, cancel: Arc<AtomicBool>, on_progress: impl Fn(DeleteProgress) + Send + Sync) -> DeletePhase {
    delete_with_meter(id, paths, permanent, threads, cancel, None, on_progress)
}

/// Like [`delete_with_progress`], with speed, time left and a first guess from `predict`.
pub fn delete_with_meter(
    id: u64,
    paths: &[PathBuf],
    permanent: bool,
    threads: usize,
    cancel: Arc<AtomicBool>,
    predict: Option<JobPredictor<'_>>,
    on_progress: impl Fn(DeleteProgress) + Send + Sync,
) -> DeletePhase {
    let ctx = DeleteCtx {
        cancel,
        total: AtomicU64::new(0),
        done: AtomicU64::new(0),
        bytes_total: AtomicU64::new(0),
        bytes_done: AtomicU64::new(0),
        error_count: AtomicU64::new(0),
        errors: Mutex::new(Vec::new()),
        current: Mutex::new(String::new()),
    };
    let started = Instant::now();
    let phase = Mutex::new(DeletePhase::Scanning);
    let first_name = paths
        .first()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let meter = Mutex::new(crate::meter::Meter::new(None));
    let counted = AtomicBool::new(false);
    let snapshot = |ctx: &DeleteCtx, phase: DeletePhase| {
        let (items_done, items_total) = (ctx.done.load(Ordering::Relaxed), ctx.total.load(Ordering::Relaxed));
        let (bytes_done, bytes_total) = (ctx.bytes_done.load(Ordering::Relaxed), ctx.bytes_total.load(Ordering::Relaxed));
        let secs = started.elapsed().as_secs_f64();
        let stats = (phase != DeletePhase::Scanning).then(|| {
            let mut m = meter.lock();
            // The first guess is asked for once, as soon as everything has been counted.
            if let (Some(predict), false) = (predict, counted.swap(true, Ordering::Relaxed)) {
                m.set_prediction(predict(items_total, bytes_total));
            }
            let mut s = m.update(secs, items_done, items_total, bytes_done, bytes_total);
            if phase == DeletePhase::Done {
                s.fraction = 1.0;
                s.eta_secs = Some(0.0);
                s.eta_confidence = 1.0;
            }
            s
        });
        DeleteProgress {
        id,
        phase,
        permanent,
        selected: paths.len() as u64,
        items_total: ctx.total.load(Ordering::Relaxed),
        items_done: ctx.done.load(Ordering::Relaxed),
        current: ctx.current.lock().clone(),
        errors: ctx.errors.lock().clone(),
        error_count: ctx.error_count.load(Ordering::Relaxed),
        elapsed_secs: started.elapsed().as_secs_f64(),
        first_name: first_name.clone(),
        action: "delete".into(),
        result: String::new(),
        bytes_done,
        bytes_total,
        skipped: 0,
        conflict: None,
        stats,
        }
    };
    let finished = AtomicBool::new(false);

    let work = || {
        if permanent {
            for p in paths {
                count_tree(p, &ctx);
            }
            *phase.lock() = DeletePhase::Running;
            for p in paths {
                if ctx.cancelled() {
                    break;
                }
                *ctx.current.lock() = p.to_string_lossy().into_owned();
                remove_tree(p, &ctx);
            }
        } else {
            // The OS moves each item to the Trash in one step, so progress is
            // by selected item.
            ctx.total.store(paths.len() as u64, Ordering::Relaxed);
            *phase.lock() = DeletePhase::Running;
            for p in paths {
                if ctx.cancelled() {
                    break;
                }
                *ctx.current.lock() = p.to_string_lossy().into_owned();
                if let Err(e) = platform::trash(std::slice::from_ref(p)) {
                    ctx.fail(p, &io::Error::other(e.to_string()));
                }
                ctx.done.fetch_add(1, Ordering::Relaxed);
            }
        }
    };

    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !finished.load(Ordering::Relaxed) {
                on_progress(snapshot(&ctx, *phase.lock()));
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build();
        match pool {
            Ok(pool) => pool.install(work),
            Err(_) => work(),
        }
        finished.store(true, Ordering::Relaxed);
    });

    let end = if ctx.cancelled() {
        DeletePhase::Cancelled
    } else if ctx.error_count.load(Ordering::Relaxed) > 0 && ctx.done.load(Ordering::Relaxed) == 0 {
        DeletePhase::Failed
    } else {
        DeletePhase::Done
    };
    on_progress(snapshot(&ctx, end));
    end
}

// ---------------------------------------------------------------------------
// Elevated helper
// ---------------------------------------------------------------------------

/// What the elevated helper reports, as one JSON document it rewrites as it goes.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HelperReport {
    Transfer { progress: Progress },
    Delete { progress: DeleteProgress },
    /// Result of a rename or create.
    Simple { ok: bool, error: Option<String>, path: Option<String> },
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ElevatedOutcome {
    pub elevation: Elevation,
    /// The helper's last report, if it got far enough to write one.
    #[serde(skip)]
    pub report: Option<HelperReport>,
}

impl ElevatedOutcome {
    pub fn declined(&self) -> bool {
        self.elevation == Elevation::Declined
    }
}

const MAX_ARG_CHARS: usize = 16_000;

/// Run `job` through the OS elevation prompt. Blocks until the helper exits or
/// the prompt is dismissed. `on_report` is called (from another thread) each
/// time the helper writes new progress. Setting the returned cancel file's
/// existence asks the helper to stop.
pub fn run_elevated_job(
    job: &Job,
    scratch_dir: &Path,
    on_report: impl Fn(HelperReport) + Send + Sync + 'static,
    cancel_file_out: impl FnOnce(PathBuf),
) -> Result<ElevatedOutcome> {
    std::fs::create_dir_all(scratch_dir).map_err(|e| CoreError::io(scratch_dir, e))?;
    let stem = format!("elevated-{}-{}", std::process::id(), unique());
    let progress = scratch_dir.join(format!("{stem}.progress.json"));
    let cancel = scratch_dir.join(format!("{stem}.cancel"));
    let job_file = scratch_dir.join(format!("{stem}.job.json"));
    cancel_file_out(cancel.clone());

    let json = serde_json::to_vec(job).map_err(|e| CoreError::Invalid(e.to_string()))?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&json);
    let mut args: Vec<String> = Vec::new();
    if encoded.len() <= MAX_ARG_CHARS {
        args.extend(["--job-b64".into(), encoded]);
    } else {
        std::fs::write(&job_file, &json).map_err(|e| CoreError::io(&job_file, e))?;
        args.extend(["--job-file".into(), job_file.to_string_lossy().into_owned()]);
    }
    args.extend(["--progress".into(), progress.to_string_lossy().into_owned()]);

    // Watch the progress file while the helper runs.
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (stop, progress) = (stop.clone(), progress.clone());
        let last: Arc<Mutex<Option<HelperReport>>> = Arc::new(Mutex::new(None));
        let last2 = last.clone();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            loop {
                let stopping = stop.load(Ordering::Relaxed);
                if let Ok(bytes) = std::fs::read(&progress) {
                    if bytes != seen {
                        if let Ok(report) = serde_json::from_slice::<HelperReport>(&bytes) {
                            seen = bytes;
                            *last2.lock() = Some(report.clone());
                            on_report(report);
                        }
                    }
                }
                if stopping {
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        (handle, last)
    };

    let exe = std::env::current_exe().map_err(|e| CoreError::Invalid(e.to_string()))?;
    let result = platform::run_elevated(&exe, &args);

    stop.store(true, Ordering::Relaxed);
    let _ = watcher.0.join();
    let report = watcher.1.lock().take();
    for f in [&progress, &cancel, &job_file] {
        let _ = std::fs::remove_file(f);
    }
    Ok(ElevatedOutcome { elevation: result?, report })
}

fn unique() -> u64 {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Entry point for the headless elevated helper (`--job-b64`/`--job-file` and
/// `--progress`). Returns the process exit code: 0 on success, 1 on failure
/// or cancellation, 2 if the job could not be read.
pub fn helper_main(args: &[String]) -> i32 {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let Some(progress_path) = value("--progress").map(PathBuf::from) else { return 2 };
    let cancel_path = progress_path.with_file_name(
        progress_path.file_name().map(|n| n.to_string_lossy().replace(".progress.json", ".cancel")).unwrap_or_default(),
    );
    let job: Job = {
        let bytes = if let Some(b64) = value("--job-b64") {
            base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64).ok()
        } else if let Some(file) = value("--job-file") {
            let data = std::fs::read(&file).ok();
            // Read once; the file is not needed again.
            let _ = std::fs::remove_file(&file);
            data
        } else {
            None
        };
        match bytes.and_then(|b| serde_json::from_slice(&b).ok()) {
            Some(j) => j,
            None => return 2,
        }
    };

    let write = {
        let progress_path = progress_path.clone();
        move |r: &HelperReport| {
            if let Ok(bytes) = serde_json::to_vec(r) {
                let _ = std::fs::write(&progress_path, bytes);
            }
        }
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let threads = resources::compute(&Settings::default().performance, &resources::snapshot()).io_threads;

    match job {
        Job::Transfer { spec } => {
            let budget = resources::compute(&Settings::default().performance, &resources::snapshot());
            let outcome = Arc::new(Mutex::new(None));
            let slot = outcome.clone();
            let w = write.clone();
            let handle = transfer::start(0, spec, budget, drives::list_drives(), None, move |p| {
                if matches!(p.phase, Phase::Done | Phase::Cancelled | Phase::Failed) {
                    *slot.lock() = Some((p.phase, p.error_count));
                }
                w(&HelperReport::Transfer { progress: p });
            });
            let watch = handle.clone();
            let cp = cancel_path.clone();
            std::thread::spawn(move || {
                while !watch.is_finished() {
                    if cp.exists() {
                        watch.cancel();
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            handle.wait();
            let result = *outcome.lock();
            match result {
                Some((Phase::Done, 0)) => 0,
                _ => 1,
            }
        }
        Job::Delete { paths, permanent } => {
            // Root's Trash is not the person's, so outside Windows an
            // elevated delete is permanent (the app said so before asking).
            let permanent = permanent || !cfg!(windows);
            let flag = cancel.clone();
            let cp = cancel_path.clone();
            let finished = Arc::new(AtomicBool::new(false));
            let done = finished.clone();
            std::thread::spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    if cp.exists() {
                        flag.store(true, Ordering::Relaxed);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let w = write.clone();
            let end = delete_with_progress(0, &paths, permanent, threads, cancel, move |p| {
                w(&HelperReport::Delete { progress: p });
            });
            finished.store(true, Ordering::Relaxed);
            if end == DeletePhase::Done { 0 } else { 1 }
        }
        Job::Rename { path, new_name } => {
            let r = entry::rename(&path, &new_name);
            report_simple(&write, r.map(|p| p.to_string_lossy().into_owned()))
        }
        Job::Create { dir, name, folder } => {
            let target = transfer::engine::unique_name(&dir.join(&name));
            let r = if folder { entry::create_dir(&target) } else { entry::create_file(&target) };
            report_simple(&write, r.map(|_| target.to_string_lossy().into_owned()))
        }
    }
}

fn report_simple(write: &impl Fn(&HelperReport), r: Result<String>) -> i32 {
    match r {
        Ok(path) => {
            write(&HelperReport::Simple { ok: true, error: None, path: Some(path) });
            0
        }
        Err(e) => {
            write(&HelperReport::Simple { ok: false, error: Some(e.to_string()), path: None });
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for dir in ["a/b/c", "a/d", "e"] {
            std::fs::create_dir_all(d.path().join(dir)).unwrap();
        }
        for f in ["a/1.txt", "a/b/2.txt", "a/b/c/3.txt", "a/d/4.txt", "e/5.txt", "6.txt"] {
            std::fs::write(d.path().join(f), b"data").unwrap();
        }
        d
    }

    #[test]
    fn permanent_delete_removes_everything_and_reports_progress() {
        let d = tree();
        let targets = vec![d.path().join("a"), d.path().join("6.txt")];
        let reports = Mutex::new(Vec::new());
        let end = delete_with_progress(7, &targets, true, 2, Arc::new(AtomicBool::new(false)), |p| {
            reports.lock().push(p)
        });
        assert_eq!(end, DeletePhase::Done);
        assert!(!d.path().join("a").exists());
        assert!(!d.path().join("6.txt").exists());
        assert!(d.path().join("e/5.txt").exists(), "unselected items are untouched");
        let reports = reports.into_inner();
        let last = reports.last().unwrap();
        assert_eq!(last.id, 7);
        assert_eq!(last.phase, DeletePhase::Done);
        // a, a/b, a/b/c, a/d, 4 files under a, plus 6.txt = 9 items.
        assert_eq!(last.items_total, 9);
        assert_eq!(last.items_done, last.items_total);
        assert_eq!(last.error_count, 0);
        assert_eq!(last.first_name, "a");
    }

    #[test]
    fn a_delete_reports_speed_and_asks_for_a_first_guess_once() {
        let d = tree();
        let asked = std::sync::atomic::AtomicU32::new(0);
        let guess = |files: u64, bytes: u64| {
            asked.fetch_add(1, Ordering::Relaxed);
            assert_eq!((files, bytes), (9, 4 * 5), "asked with what was counted");
            None
        };
        let reports = Mutex::new(Vec::new());
        let targets = vec![d.path().join("a"), d.path().join("6.txt")];
        delete_with_meter(1, &targets, true, 2, Arc::new(AtomicBool::new(false)), Some(&guess), |p| reports.lock().push(p));
        let reports = reports.into_inner();
        assert_eq!(asked.load(Ordering::Relaxed), 1);
        assert!(reports.iter().filter(|p| p.phase == DeletePhase::Scanning).all(|p| p.stats.is_none()), "nothing to measure while counting");
        let last = reports.last().unwrap();
        assert_eq!((last.bytes_done, last.bytes_total), (4 * 5, 4 * 5));
        let s = last.stats.as_ref().expect("a finished delete has its numbers");
        assert_eq!((s.fraction, s.eta_secs), (1.0, Some(0.0)));
    }

    #[test]
    fn cancelled_delete_stops_early() {
        let d = tree();
        let cancel = Arc::new(AtomicBool::new(true));
        let end = delete_with_progress(1, &[d.path().join("a")], true, 1, cancel, |_| {});
        assert_eq!(end, DeletePhase::Cancelled);
        assert!(d.path().join("a").exists());
    }

    #[test]
    fn deletes_read_only_files() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("locked.txt");
        std::fs::write(&f, b"x").unwrap();
        let mut perms = std::fs::metadata(&f).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&f, perms).unwrap();
        let end = delete_with_progress(1, &[f.clone()], true, 1, Arc::new(AtomicBool::new(false)), |_| {});
        assert_eq!(end, DeletePhase::Done);
        assert!(!f.exists());
    }

    #[test]
    fn missing_item_is_not_an_error() {
        let d = tempfile::tempdir().unwrap();
        let end = delete_with_progress(1, &[d.path().join("gone")], true, 1, Arc::new(AtomicBool::new(false)), |_| {});
        assert_eq!(end, DeletePhase::Done);
    }

    #[test]
    fn writable_folder_needs_no_elevation() {
        let d = tempfile::tempdir().unwrap();
        assert!(!write_denied(d.path()));
        let job = Job::Create { dir: d.path().to_path_buf(), name: "x".into(), folder: true };
        assert_eq!(check_access(&job), AccessCheck::default());
        // No probe file is left behind.
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn protected_folder_is_detected_for_non_root() {
        use std::os::unix::fs::PermissionsExt;
        // Root ignores permission bits, so this only means something for other users.
        if unsafe { libc_geteuid() } == 0 {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let locked = d.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        assert!(write_denied(&locked));
        let job = Job::Delete { paths: vec![locked.join("x")], permanent: false };
        let check = check_access(&job);
        assert!(check.needs_elevation && check.becomes_permanent);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    unsafe fn libc_geteuid() -> u32 {
        extern "C" {
            fn geteuid() -> u32;
        }
        geteuid()
    }

    #[test]
    fn helper_runs_each_job_kind_and_reports() {
        let d = tempfile::tempdir().unwrap();
        let progress = d.path().join("run.progress.json");
        let b64 = |j: &Job| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(j).unwrap());
        let args = |j: &Job| -> Vec<String> {
            vec!["--job-b64".into(), b64(j), "--progress".into(), progress.to_string_lossy().into_owned()]
        };

        let made = d.path().join("made");
        let code = helper_main(&args(&Job::Create { dir: d.path().to_path_buf(), name: "made".into(), folder: true }));
        assert_eq!(code, 0);
        assert!(made.is_dir());
        let report: HelperReport = serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
        assert!(matches!(report, HelperReport::Simple { ok: true, .. }));

        let code = helper_main(&args(&Job::Rename { path: made.clone(), new_name: "renamed".into() }));
        assert_eq!(code, 0);
        assert!(d.path().join("renamed").is_dir());

        let code = helper_main(&args(&Job::Delete { paths: vec![d.path().join("renamed")], permanent: true }));
        assert_eq!(code, 0);
        assert!(!d.path().join("renamed").exists());
        let report: HelperReport = serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
        assert!(matches!(report, HelperReport::Delete { .. }));

        // A garbled job is refused rather than guessed at.
        assert_eq!(helper_main(&["--job-b64".into(), "!!!".into(), "--progress".into(), "x".into()]), 2);
    }

    #[test]
    fn helper_transfer_copies_files() {
        let d = tree();
        let dest = tempfile::tempdir().unwrap();
        let progress = dest.path().join("t.progress.json");
        let spec = TransferSpec {
            sources: vec![d.path().join("a")],
            dest_dir: dest.path().to_path_buf(),
            op: transfer::Op::Copy,
            conflict: transfer::Conflict::KeepBoth,
            preserve_times: true,
            large_threshold: 0,
            max_parallel: 0,
            method: Default::default(),
            skip_identical: true,
        };
        let job = Job::Transfer { spec };
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(&job).unwrap());
        let code = helper_main(&["--job-b64".into(), b64, "--progress".into(), progress.to_string_lossy().into_owned()]);
        assert_eq!(code, 0);
        assert!(dest.path().join("a/b/c/3.txt").exists());
        let report: HelperReport = serde_json::from_slice(&std::fs::read(&progress).unwrap()).unwrap();
        assert!(matches!(report, HelperReport::Transfer { progress } if progress.phase == Phase::Done));
    }

    #[test]
    fn touched_dirs_cover_sources_and_destination() {
        let job = Job::Transfer {
            spec: TransferSpec {
                sources: vec![PathBuf::from("/x/a"), PathBuf::from("/x/b"), PathBuf::from("/y/c")],
                dest_dir: PathBuf::from("/z"),
                op: transfer::Op::Move,
                conflict: transfer::Conflict::Skip,
                preserve_times: true,
                large_threshold: 0,
                max_parallel: 0,
                method: Default::default(),
                skip_identical: true,
            },
        };
        assert_eq!(job.touched_dirs(), vec![PathBuf::from("/x"), PathBuf::from("/y"), PathBuf::from("/z")]);
    }
}
