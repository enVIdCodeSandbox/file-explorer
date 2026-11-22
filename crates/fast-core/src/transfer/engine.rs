//! Copy/move engine. One coordinator thread per job scans the sources, picks
//! a [`Plan`], runs worker threads and reports [`Progress`] about ten times a
//! second. Nothing here touches the UI thread.

use super::eta::{blend_estimates, DisplayMode, EtaModel, EtaParams, EtaTracker, LiveRate};
use crate::history::{Prediction, Query, Route};
use super::method::{self, Method, MethodChooser, MethodPref};
use super::probe;
use super::strategy::{self, JobProfile, Plan, Strategy};
use super::throttle;
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
    /// Ask about each file that already exists, while the job runs.
    Ask,
    Skip,
    Overwrite,
    KeepBoth,
}

/// What the person decided for one conflicting file.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    Skip,
    Replace,
    KeepBoth,
}

impl Resolution {
    fn conflict(self) -> Conflict {
        match self {
            Resolution::Skip => Conflict::Skip,
            Resolution::Replace => Conflict::Overwrite,
            Resolution::KeepBoth => Conflict::KeepBoth,
        }
    }
}

/// A file that exists on both sides, waiting for a decision.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub name: String,
    pub src: String,
    pub dst: String,
    pub src_size: u64,
    pub dst_size: u64,
    /// Seconds since 1970, when known.
    pub src_modified: Option<i64>,
    pub dst_modified: Option<i64>,
    /// How many conflicts have been asked about so far in this job (this one included).
    pub number: u64,
}

fn modified_secs(m: &fs::Metadata) -> Option<i64> {
    m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

#[derive(Default)]
struct Asker {
    /// "Do this for all": applies to every later conflict.
    sticky: Option<Conflict>,
    pending: Option<ConflictInfo>,
    answer: Option<(Resolution, bool)>,
    asked: u64,
}

/// A file that is being copied at this moment.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveFile {
    pub path: String,
    pub size: u64,
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
    /// Which way to move the bytes; automatic by default.
    #[serde(default)]
    pub method: MethodPref,
    /// A file already at the destination with the same size and modified time is the same file:
    /// it is kept as it is and skipped, whatever the conflict choice, and never asked about.
    #[serde(default = "yes")]
    pub skip_identical: bool,
    /// How many of the files being copied are reported in `Progress::active_files` (0: none, and nothing is
    /// tracked for it).
    #[serde(default)]
    pub active_files: usize,
    /// Time the devices with a short test before a big copy, to start the estimate better informed. Off by
    /// default: it delays the start, and what earlier transfers and the first seconds of the copy tell is nearly as good.
    #[serde(default)]
    pub speed_test: bool,
    /// Begin copying while the folders are still being counted, once the count has taken this many
    /// milliseconds (0: wait for the whole count first).
    #[serde(default)]
    pub early_start_ms: u64,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Waiting for its turn (see `throttle::Gate`).
    Queued,
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
    /// What failed: the file (or folder) that was being read.
    pub path: String,
    pub message: String,
    pub needs_elevation: bool,
    /// Where it was being written, when that is known. A retry of just this item needs it.
    #[serde(default)]
    pub dest: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: u64,
    pub op: Op,
    pub phase: Phase,
    pub strategy: Option<Strategy>,
    /// What actually moved the bytes: the built-in engine or a system tool.
    pub method: Option<Method>,
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
    /// A file that exists on both sides and needs an answer (see `TransferHandle::resolve`).
    pub conflict: Option<ConflictInfo>,
    /// 0 low, 1 normal, 2 high.
    pub priority: u8,
    /// This transfer's own speed limit in bytes per second (0 = none).
    pub limit_bps: u64,
    /// Stops after this many seconds of work (0 = no time limit).
    pub max_secs: u64,
    /// The time limit ran out.
    pub timed_out: bool,
    /// While queued: how many transfers go first.
    pub queued_ahead: u32,
    /// While queued: the hard disk it is waiting for another transfer to finish with, when that is the reason.
    #[serde(default)]
    pub queued_disk: Option<String>,
    /// The files being copied right now, one per busy worker (as many as `TransferSpec::active_files` allows).
    #[serde(default)]
    pub active_files: Vec<ActiveFile>,
    /// Copying has begun but the folders are still being counted: the totals are what has been found so far,
    /// and there is no time estimate yet.
    #[serde(default)]
    pub counting: bool,
}

/// Asked once, after the sources are scanned, what similar past transfers say.
pub type Predictor = Box<dyn FnOnce(&Query) -> Option<Prediction> + Send>;

/// Rough per-file and per-MB costs used only to weight the progress bar when
/// nothing is known about the devices yet.
pub(crate) fn default_costs(src: crate::drives::Media, dst: crate::drives::Media) -> EtaParams {
    use crate::drives::Media::*;
    match (src, dst) {
        (Network, _) | (_, Network) => EtaParams { per_file_secs: 0.010, per_mb_secs: 0.020 },
        (Hdd, _) | (_, Hdd) => EtaParams { per_file_secs: 0.008, per_mb_secs: 0.012 },
        _ => EtaParams { per_file_secs: 0.001, per_mb_secs: 0.004 },
    }
}

/// Jobs expected to take less than this (by the default costs) do not measure the devices first.
const PROBE_WORTH_SECS: f64 = 10.0;

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
    /// One question at a time: workers queue on `ask_gate`.
    ask_gate: Mutex<()>,
    asker: Mutex<Asker>,
    priority: Arc<AtomicU8>,
    start_now: Arc<AtomicBool>,
    limiter: throttle::Limiter,
    max_secs: AtomicU64,
    /// What to do when the time limit runs out: stop (true) or pause (false).
    stop_on_timeout: AtomicBool,
    timed_out: AtomicBool,
    run_started: Mutex<Option<Instant>>,
    /// Files found so far while the sources are being looked at.
    scanned: AtomicU64,
    scan_bytes: AtomicU64,
    /// The walk was stopped on purpose (a tool finished the job by itself).
    scan_abort: AtomicBool,
    /// The walk of the sources is over, and `total_files`/`total_bytes` are final.
    scan_done: AtomicBool,
    /// What the job comes to so far, when copying began before the count was over.
    total_files: AtomicU64,
    total_bytes: AtomicU64,
    moved_dirs: Mutex<Vec<PathBuf>>,
    /// Copying began before the count was over: a file's folder is made when it is needed.
    lazy_dirs: AtomicBool,
    made_dirs: Mutex<std::collections::HashSet<PathBuf>>,
    /// This job's place in the queue has been given back (by finishing, or by being let go of while stuck).
    gate_released: AtomicBool,
    /// What each worker is copying now (empty text when idle). Only used when the spec asks for it.
    slots: Mutex<Vec<Option<ActiveFile>>>,
    /// The folder the count is reading now and since when, so a drive that is slow to answer can be named.
    reading: Mutex<(String, Instant)>,
    /// A source on a memory stick, card or spinning disk: folders are read one or two at a time there, because
    /// reading many at once makes such a device slower, not faster.
    slow_source: bool,
}

thread_local! {
    static LOWERED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

impl Shared {
    /// Is any speed limit (this transfer's or the whole app's) in force?
    fn limited(&self) -> bool {
        self.limiter.rate() > 0 || throttle::global_limiter().rate() > 0
    }
    /// A block size that keeps a limited copy smooth: about an eighth of a second of the limit.
    fn chunk_len(&self, buffer: usize) -> usize {
        let rate = [self.limiter.rate(), throttle::global_limiter().rate()].into_iter().filter(|r| *r > 0).min();
        match rate {
            Some(r) => ((r / 8) as usize).clamp(64 * 1024, buffer.max(64 * 1024)),
            None => buffer,
        }
    }
    /// After moving `n` bytes: wait as long as the limits ask, and let a low priority transfer yield to
    /// other programs. Returns false if the job was cancelled meanwhile.
    fn pace(&self, n: u64) -> bool {
        if self.priority.load(Ordering::Relaxed) == throttle::LOW && !LOWERED.with(|l| l.replace(true)) {
            crate::proc::lower_thread_priority();
        }
        let mut wait = self.limiter.reserve(n).max(throttle::global_limiter().reserve(n));
        while wait > Duration::ZERO {
            let slice = wait.min(Duration::from_millis(50));
            std::thread::sleep(slice);
            wait -= slice;
            if !self.checkpoint() {
                return false;
            }
        }
        self.checkpoint()
    }
    /// Apply the time limit once it has run out.
    fn check_deadline(&self) {
        let max = self.max_secs.load(Ordering::Relaxed);
        if max == 0 || self.timed_out.load(Ordering::Relaxed) {
            return;
        }
        let Some(t) = *self.run_started.lock() else { return };
        if t.elapsed().as_secs() >= max {
            self.timed_out.store(true, Ordering::Relaxed);
            let to = if self.stop_on_timeout.load(Ordering::Relaxed) { ST_CANCEL } else { ST_PAUSE };
            let _ = self.control.compare_exchange(ST_RUN, to, Ordering::Relaxed, Ordering::Relaxed);
        }
    }
    /// Make a destination folder once (when copying began before the count was over, so no skeleton was made).
    fn ensure_dir(&self, d: &Path, report: bool) {
        if self.made_dirs.lock().contains(d) {
            return;
        }
        match fs::create_dir_all(d) {
            Ok(()) => {
                self.made_dirs.lock().insert(d.to_path_buf());
            }
            Err(e) if report => self.error(d, &e),
            // A file that cannot go into its folder says so itself when it is copied.
            Err(_) => {}
        }
    }

    /// Note what a worker is copying (or that it is idle). Costs nothing when the spec did not ask for it.
    fn set_slot(&self, slot: usize, item: Option<&Item>) {
        let mut slots = self.slots.lock();
        if let Some(entry) = slots.get_mut(slot) {
            *entry = item.map(|i| ActiveFile { path: i.src.to_string_lossy().into_owned(), size: i.size });
        }
    }

    fn fill(&self, p: &mut Progress) {
        p.priority = self.priority.load(Ordering::Relaxed);
        p.limit_bps = self.limiter.rate();
        p.max_secs = self.max_secs.load(Ordering::Relaxed);
        p.timed_out = self.timed_out.load(Ordering::Relaxed);
    }

    /// What to do about `dst`, which already exists. With the Ask policy this
    /// blocks until the person answers (or the job is cancelled, giving None).
    fn resolve(&self, policy: Conflict, src: &Path, dst: &Path) -> Option<Conflict> {
        if policy != Conflict::Ask {
            return Some(policy);
        }
        let _turn = self.ask_gate.lock();
        if let Some(c) = self.asker.lock().sticky {
            return Some(c);
        }
        let (sm, dm) = (fs::metadata(src).ok(), fs::metadata(dst).ok());
        {
            let mut a = self.asker.lock();
            a.asked += 1;
            a.answer = None;
            a.pending = Some(ConflictInfo {
                name: dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                src: src.to_string_lossy().into_owned(),
                dst: dst.to_string_lossy().into_owned(),
                src_size: sm.as_ref().map(|m| m.len()).unwrap_or(0),
                dst_size: dm.as_ref().map(|m| m.len()).unwrap_or(0),
                src_modified: sm.as_ref().and_then(modified_secs),
                dst_modified: dm.as_ref().and_then(modified_secs),
                number: a.asked,
            });
        }
        loop {
            if self.cancelled() {
                self.asker.lock().pending = None;
                return None;
            }
            {
                let mut a = self.asker.lock();
                if let Some((r, all)) = a.answer.take() {
                    a.pending = None;
                    if all {
                        a.sticky = Some(r.conflict());
                    }
                    return Some(r.conflict());
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn cancelled(&self) -> bool {
        self.control.load(Ordering::Relaxed) == ST_CANCEL
    }
    /// Block while paused. Returns false if the job was cancelled.
    fn checkpoint(&self) -> bool {
        loop {
            self.check_deadline();
            match self.control.load(Ordering::Relaxed) {
                ST_CANCEL => return false,
                ST_PAUSE => std::thread::sleep(Duration::from_millis(50)),
                _ => return true,
            }
        }
    }
    /// Note which folder the count is in, so the card can say where a slow drive is making it wait.
    fn reading_now(&self, path: &Path) {
        let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
        *self.reading.lock() = (name, Instant::now());
    }

    /// What the card says while the count goes on: how many files so far, and (when the drive is
    /// slow to answer) which folder it is waiting on. No path separators: the card shows only the last part.
    fn counting_text(&self) -> String {
        let n = self.scanned.load(Ordering::Relaxed);
        let (name, since) = self.reading.lock().clone();
        let found = if n > 0 { format!("Counting files… {n} so far") } else { "Reading the folders…".to_string() };
        if name.is_empty() {
            found
        } else if since.elapsed() >= Duration::from_secs(5) {
            format!("{found} (the drive is slow to answer: still in \u{201c}{name}\u{201d})")
        } else {
            format!("{found} (in \u{201c}{name}\u{201d})")
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
                dest: None,
            });
        }
    }
    /// A failure whose destination is known, with the reason as the tool gave it.
    fn error_to(&self, path: &Path, dest: &Path, message: String, needs_elevation: bool) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        let mut errs = self.errors.lock();
        if errs.len() < MAX_REPORTED_ERRORS {
            errs.push(TransferError { path: path.to_string_lossy().into_owned(), message, needs_elevation, dest: Some(dest.to_string_lossy().into_owned()) });
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
    /// Let go of a job that is stuck waiting for a drive: cancel it, and give its place in the queue to the
    /// next one, so a job stuck on a dead share does not hold up every copy after it. The job itself ends
    /// whenever the drive finally answers.
    pub fn abandon(&self) {
        self.cancel();
        if self.shared.run_started.lock().is_some() && !self.shared.gate_released.swap(true, Ordering::SeqCst) {
            throttle::gate().leave(self.id);
        }
    }
    /// Answer the conflict that `Progress::conflict` is showing. With `all`,
    /// the same answer is used for every later conflict in this job.
    pub fn resolve(&self, answer: Resolution, all: bool) {
        let mut a = self.shared.asker.lock();
        if a.pending.is_some() {
            a.answer = Some((answer, all));
        }
    }
    /// Limit this transfer's speed, in bytes per second (0 removes the limit). Takes effect at once.
    pub fn set_speed_limit(&self, bytes_per_sec: u64) {
        self.shared.limiter.set_rate(bytes_per_sec);
    }
    /// 0 low, 1 normal, 2 high: the order of the queue, and low yields to other programs.
    pub fn set_priority(&self, priority: u8) {
        self.shared.priority.store(priority.min(throttle::HIGH), Ordering::Relaxed);
    }
    /// Stop after `secs` seconds of work (0 = no limit); `stop` ends it, otherwise it pauses.
    pub fn set_time_limit(&self, secs: u64, stop: bool) {
        self.shared.stop_on_timeout.store(stop, Ordering::Relaxed);
        self.shared.max_secs.store(secs, Ordering::Relaxed);
    }
    /// Leave the queue now, whatever the limit on simultaneous transfers says.
    pub fn start_now(&self) {
        self.shared.start_now.store(true, Ordering::Relaxed);
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
    start_with(id, spec, budget, drives, predict, None, on_progress)
}

/// Like [`start`], with a chooser that picks the method when the spec says "auto".
pub fn start_with(
    id: u64,
    spec: TransferSpec,
    budget: Budget,
    drives: Vec<Drive>,
    predict: Option<Predictor>,
    choose: Option<MethodChooser>,
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
        ask_gate: Mutex::new(()),
        asker: Mutex::new(Asker::default()),
        priority: Arc::new(AtomicU8::new(throttle::NORMAL)),
        start_now: Arc::new(AtomicBool::new(false)),
        limiter: throttle::Limiter::new(0),
        max_secs: AtomicU64::new(0),
        stop_on_timeout: AtomicBool::new(true),
        timed_out: AtomicBool::new(false),
        run_started: Mutex::new(None),
        scanned: AtomicU64::new(0),
        scan_bytes: AtomicU64::new(0),
        scan_abort: AtomicBool::new(false),
        scan_done: AtomicBool::new(false),
        total_files: AtomicU64::new(0),
        total_bytes: AtomicU64::new(0),
        moved_dirs: Mutex::new(Vec::new()),
        lazy_dirs: AtomicBool::new(false),
        made_dirs: Mutex::new(Default::default()),
        gate_released: AtomicBool::new(false),
        slots: Mutex::new(Vec::new()),
        reading: Mutex::new((String::new(), Instant::now())),
        slow_source: spec.sources.iter().any(|p| matches!(drives::media_for(p, &drives), drives::Media::Removable | drives::Media::Hdd)),
    });
    let done = Arc::new(AtomicBool::new(false));
    let handle = TransferHandle { id, shared: shared.clone(), done: done.clone() };
    std::thread::Builder::new()
        .name(format!("transfer-{id}"))
        .spawn(move || {
            run(id, spec, budget, drives, predict, choose, &shared, &on_progress);
            done.store(true, Ordering::Relaxed);
        })
        .expect("spawn transfer thread");
    handle
}

/// What the walk of the sources reports, in the order it finds things.
enum Found {
    /// The top-level items are decided (quick). Everything else follows while folders are walked.
    Tops {
        tops: Vec<method::Top>,
        renamed_all: bool,
        /// Something with the same name is already at the destination (a folder merge, or a file to replace).
        clash_possible: bool,
    },
    Items(Vec<Item>),
    /// Destination folders that will be needed (including empty ones).
    Dirs(Vec<PathBuf>),
    /// The walk is over. Source folders to remove after a copy-based move, deepest last.
    Done { moved_dirs: Vec<PathBuf> },
}

/// Look at every source: finish what a rename can finish, decide the top-level conflicts (quick, all
/// sources first), then walk the folders, sending what is found as it is found.
fn phase1(spec: &TransferSpec, shared: &Shared, tx: &chan::Sender<Found>) {
    let mut renamed_all = spec.op == Op::Move;
    let mut tops: Vec<method::Top> = Vec::new();
    let mut files: Vec<Item> = Vec::new();
    let mut clash_possible = false;
    for src in &spec.sources {
        if shared.cancelled() {
            break;
        }
        shared.reading_now(src);
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
        // With "ask", files are decided one by one while the job runs (in `process`).
        if dst.exists() {
            clash_possible = true;
        }
        if dst.exists() && !(meta.is_dir() && dst.is_dir()) && spec.conflict != Conflict::Ask {
            match spec.conflict {
                Conflict::Ask => {}
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
        tops.push(method::Top { src: src.clone(), dst: dst.clone(), is_dir: meta.is_dir() });
        if !meta.is_dir() {
            shared.scanned.fetch_add(1, Ordering::Relaxed);
            shared.scan_bytes.fetch_add(meta.len(), Ordering::Relaxed);
            files.push(Item { src: src.clone(), dst, size: meta.len() });
        }
    }
    let dir_tops: Vec<(PathBuf, PathBuf)> = tops.iter().filter(|t| t.is_dir).map(|t| (t.src.clone(), t.dst.clone())).collect();
    let _ = tx.send(Found::Tops { tops, renamed_all, clash_possible });
    if !files.is_empty() {
        let _ = tx.send(Found::Items(files));
    }
    let moved = parking_lot::Mutex::new(Vec::new());
    for (src, dst) in &dir_tops {
        scan(src, dst, shared, tx, &moved);
    }
    let _ = tx.send(Found::Done { moved_dirs: moved.into_inner() });
}

fn run(
    id: u64,
    spec: TransferSpec,
    budget: Budget,
    drives: Vec<Drive>,
    predict: Option<Predictor>,
    choose: Option<MethodChooser>,
    shared: &Arc<Shared>,
    on_progress: &(impl Fn(Progress) + Send + 'static),
) {
    let started = Instant::now();
    let mut progress = Progress {
        id,
        op: spec.op,
        phase: Phase::Scanning,
        strategy: None,
        method: None,
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
        conflict: None,
        priority: throttle::NORMAL,
        limit_bps: 0,
        max_secs: 0,
        timed_out: false,
        queued_ahead: 0,
        queued_disk: None,
        active_files: Vec::new(),
        counting: false,
    };
    // Wait for a turn when too many transfers are already working.
    {
        let gate = throttle::gate();
        // Spinning disks this transfer would keep busy: another transfer on the same one waits its turn.
        let mut disks: Vec<String> = spec
            .sources
            .iter()
            .chain(std::iter::once(&spec.dest_dir))
            .filter_map(|p| drives::volume_for(p, &drives))
            .filter(|d| d.media == drives::Media::Hdd)
            .map(|d| d.mount.to_lowercase())
            .collect();
        disks.sort();
        disks.dedup();
        gate.join_on(id, shared.priority.clone(), shared.start_now.clone(), disks);
        let mut last = Instant::now() - Duration::from_secs(1);
        while !gate.try_enter(id) {
            if shared.cancelled() {
                gate.withdraw(id);
                shared.fill(&mut progress);
                progress.phase = Phase::Cancelled;
                on_progress(progress);
                return;
            }
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                progress.phase = Phase::Queued;
                progress.queued_ahead = gate.ahead_of(id);
                progress.queued_disk = gate.waiting_for_disk(id);
                shared.fill(&mut progress);
                on_progress(progress.clone());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        progress.queued_ahead = 0;
        progress.queued_disk = None;
    }
    struct Turn(Arc<Shared>, u64);
    impl Drop for Turn {
        fn drop(&mut self) {
            if !self.0.gate_released.swap(true, Ordering::SeqCst) {
                throttle::gate().leave(self.1);
            }
        }
    }
    let _turn = Turn(shared.clone(), id);
    *shared.run_started.lock() = Some(Instant::now());
    shared.fill(&mut progress);
    on_progress(progress.clone());

    // ---- Phase 1: fast path for moves, then scan what is left ----------------
    // This touches the drives (a network share can take its time to answer), so it runs on its own thread
    // while this one watches: it reports what has been found so far, and Cancel ends the job at once
    // instead of waiting for the drive. A walk that is cancelled stops by itself at the next folder.
    let phase1_started = Instant::now();
    let scan_rx = {
        let (tx, rx) = chan::unbounded::<Found>();
        let (spec, shared) = (spec.clone(), shared.clone());
        let _ = std::thread::Builder::new().name(format!("prepare-{id}")).spawn(move || phase1(&spec, &shared, &tx));
        rx
    };
    let mut items: Vec<Item> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut moved_dirs: Vec<PathBuf> = Vec::new();
    let mut found_tops: Option<(Vec<method::Top>, bool, bool)> = None;
    // Once the count has taken this long, copying begins with what has been found, and the count carries on.
    let early_after = Duration::from_millis(spec.early_start_ms);
    let mut go_early = false;
    {
        let mut last = Instant::now() - Duration::from_secs(1);
        let cancelled_now = |progress: &mut Progress| {
            shared.fill(progress);
            progress.phase = Phase::Cancelled;
            progress.current.clear();
            progress.elapsed_secs = started.elapsed().as_secs_f64();
        };
        loop {
            if shared.cancelled() {
                // Nothing was copied yet, or not much: stop at once instead of waiting for the drive.
                cancelled_now(&mut progress);
                on_progress(progress);
                return;
            }
            match scan_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(Found::Tops { tops, renamed_all, clash_possible }) => found_tops = Some((tops, renamed_all, clash_possible)),
                Ok(Found::Items(v)) => items.extend(v),
                Ok(Found::Dirs(v)) => dirs.extend(v),
                Ok(Found::Done { moved_dirs: m }) => {
                    moved_dirs = m;
                    shared.scan_done.store(true, Ordering::Relaxed);
                    break;
                }
                Err(chan::RecvTimeoutError::Timeout) => {}
                // The walk died without finishing: go on with what it found.
                Err(chan::RecvTimeoutError::Disconnected) => {
                    shared.scan_done.store(true, Ordering::Relaxed);
                    break;
                }
            }
            if early_after > Duration::ZERO && found_tops.is_some() && !items.is_empty() && phase1_started.elapsed() >= early_after {
                go_early = true;
                break;
            }
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                shared.fill(&mut progress);
                progress.phase = Phase::Scanning;
                progress.current = shared.counting_text();
                progress.elapsed_secs = started.elapsed().as_secs_f64();
                on_progress(progress.clone());
            }
        }
    }
    let (tops, renamed_all, clash_possible) = found_tops.unwrap_or_else(|| (Vec::new(), spec.op == Op::Move, false));

    let large_threshold =
        if spec.large_threshold > 0 { spec.large_threshold } else { strategy::DEFAULT_LARGE_THRESHOLD };
    let mut bytes_total: u64 = items.iter().map(|i| i.size).sum();
    let renamed = shared.files_done.load(Ordering::Relaxed);
    let mut files_total = items.len() as u64 + renamed;
    if go_early {
        shared.total_files.store(items.len() as u64, Ordering::Relaxed);
        shared.total_bytes.store(bytes_total, Ordering::Relaxed);
        shared.lazy_dirs.store(true, Ordering::Relaxed);
    }

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
    // Built-in engine or a system tool? Only plain copies that the tool can do
    // exactly the same way are candidates; moves, renames, clones and copies
    // that keep both versions stay with the engine.
    let tool_ok = spec.op == Op::Copy
        // "Ask" needs the engine only when something could actually clash (tools cannot ask).
        && (spec.conflict == Conflict::Ask && !clash_possible || !matches!(spec.conflict, Conflict::KeepBoth | Conflict::Ask))
        && plan.strategy != Strategy::Rename
        && !plan.try_clone
        && !tops.is_empty()
        && tops.iter().all(method::can_run)
        // A system tool cannot be held to a speed limit, so a limited job uses the built-in engine.
        && !shared.limited();
    let mut method = Method::Native;
    if tool_ok {
        let can = method::candidates(&tops);
        method = match spec.method.method() {
            Some(m) if can.contains(&m) => m,
            Some(_) => Method::Native, // asked for a tool this computer or route cannot use
            None => match choose {
                // A tool needs the whole count first, and a memory stick or card can take minutes to list: copying
                // by itself while the count goes on is the better start there.
                Some(choose) if can.len() > 1 && !(go_early && profile.src == drives::Media::Removable) => choose(
                    &Query {
                        route: route.clone(),
                        files: items.len() as u64,
                        bytes: bytes_total,
                        is_move: false,
                    },
                    &can,
                ),
                _ => Method::Native,
            },
        };
    }
    // How much to trust the first estimate: earlier transfers on this very
    // route count for most; a quick speed test of the devices for a fair bit.
    // Even one earlier transfer on this very route vouches for the estimate (every transfer measures what the
    // route costs per file and per megabyte); more of them, and ones of a shape like this job's, vouch more.
    let mut head_start = match &prediction {
        Some(p) if p.same_route => (0.7 + 0.05 * (p.samples.min(5) as f64 - 1.0)) * (0.65 + 0.35 * p.shape_match),
        Some(p) => 0.3 * (0.65 + 0.35 * p.shape_match),
        None => 0.0,
    };
    // With little to go on, test the real devices for a moment. A job that
    // should be over within seconds is over before the test would pay off (it
    // took about half a second here, which was a third of a 1 GB copy), so it
    // skips the test and the estimate stays a rough guess.
    let guess = default_costs(profile.src, profile.dst);
    let guessed_secs = files_total as f64 * guess.per_file_secs + bytes_total as f64 / (1024.0 * 1024.0) * guess.per_mb_secs;
    let worth_probing = spec.speed_test && files_total > 0 && (bytes_total >= 48 << 20 || files_total >= 300) && guessed_secs >= PROBE_WORTH_SECS;
    let well_known = matches!(&prediction, Some(p) if p.same_route);
    if worth_probing && !well_known && !shared.cancelled() && plan.strategy != Strategy::Rename {
        *shared.current.lock() = "Measuring speed…".into();
        progress.phase = Phase::Scanning;
        progress.current = "Measuring speed…".into();
        on_progress(progress.clone());
        let biggest = items.iter().max_by_key(|i| i.size).map(|i| i.src.clone());
        if let Some(m) = probe::run_bounded(biggest.as_deref(), &spec.dest_dir, Duration::from_millis(2500), || shared.cancelled()) {
            let shared_bw = matches!(profile.src, drives::Media::Network | drives::Media::Hdd | drives::Media::Removable)
                || matches!(profile.dst, drives::Media::Network | drives::Media::Hdd | drives::Media::Removable);
            let streams = plan.small_workers + plan.large_workers;
            let seed = m.params(streams, shared_bw);
            // A probe of this very route beats the generic guess, but not real history.
            if prediction.is_none() {
                shared.model.lock().set_prior(Some(seed));
                head_start = 0.45;
            }
        } else if prediction.is_none() {
            shared.model.lock().set_prior(Some(default_costs(profile.src, profile.dst)));
        }
        shared.current.lock().clear();
        progress.current.clear();
    } else if prediction.is_none() {
        // No history and no test: a rough guess by device type, flagged as rough.
        shared.model.lock().set_prior(Some(default_costs(profile.src, profile.dst)));
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
    // An instant move did all its work while scanning: that is the time it took.
    let copy_started = if plan.strategy == Strategy::Rename { phase1_started } else { Instant::now() };
    let (mut peak_bps, mut peak_fps) = (0f64, 0f64);

    progress.route = Some(route);
    progress.plan = Some(plan.clone());
    progress.prediction = prediction;
    progress.files_total = files_total;
    progress.bytes_total = bytes_total;
    progress.strategy = Some(plan.strategy);
    progress.method = Some(method);
    progress.phase = Phase::Running;

    // A system tool copies whole top-level items by itself, so it has to know where they end: finish the count.
    if go_early && method != Method::Native {
        let mut last = Instant::now() - Duration::from_secs(1);
        loop {
            if shared.cancelled() {
                shared.fill(&mut progress);
                progress.phase = Phase::Cancelled;
                progress.current.clear();
                progress.elapsed_secs = started.elapsed().as_secs_f64();
                on_progress(progress);
                return;
            }
            match scan_rx.recv_timeout(Duration::from_millis(25)) {
                Ok(Found::Items(v)) => items.extend(v),
                Ok(Found::Dirs(v)) => dirs.extend(v),
                Ok(Found::Done { moved_dirs: m }) => {
                    moved_dirs = m;
                    break;
                }
                Ok(Found::Tops { .. }) | Err(chan::RecvTimeoutError::Timeout) => {}
                Err(chan::RecvTimeoutError::Disconnected) => break,
            }
            if last.elapsed() >= Duration::from_millis(250) {
                last = Instant::now();
                shared.fill(&mut progress);
                progress.phase = Phase::Scanning;
                progress.current = shared.counting_text();
                progress.elapsed_secs = started.elapsed().as_secs_f64();
                on_progress(progress.clone());
            }
        }
        shared.scan_done.store(true, Ordering::Relaxed);
        go_early = false;
        shared.lazy_dirs.store(false, Ordering::Relaxed);
        bytes_total = items.iter().map(|i| i.size).sum();
        files_total = items.len() as u64 + renamed;
        progress.files_total = files_total;
        progress.bytes_total = bytes_total;
    }

    // ---- Phase 2: create the directory skeleton, then copy -------------------
    // (When copying begins before the count is over, folders are made as they are needed instead.)
    if method == Method::Native && !go_early {
        for d in &dirs {
            if shared.cancelled() {
                break;
            }
            if let Err(e) = fs::create_dir_all(d) {
                shared.error(d, &e);
            }
        }
    }

    let (small_tx, small_rx) = chan::unbounded::<Item>();
    let (large_tx, large_rx) = chan::unbounded::<Item>();
    // Largest first in the large lane so the long poles start early; small
    // files keep scan order, which preserves on-disk locality.
    let items = if method == Method::Native { items } else { Vec::new() };
    let (mut large, small): (Vec<Item>, Vec<Item>) = items.into_iter().partition(|i| i.size >= large_threshold);
    large.sort_by(|a, b| b.size.cmp(&a.size));
    for i in small {
        let _ = small_tx.send(i);
    }
    for i in large {
        let _ = large_tx.send(i);
    }
    // While the count goes on, the walk keeps feeding the lanes: their senders go to the feeder.
    let mut feed_lanes = if go_early {
        Some((small_tx, large_tx))
    } else {
        // Everything is already queued: closing the lanes is what tells the workers they are done.
        drop((small_tx, large_tx));
        None
    };

    let workers = Arc::new(AtomicUsize::new(0));
    let next_slot = AtomicUsize::new(0);
    *shared.slots.lock() = vec![None; spec.active_files.min(16)];
    std::thread::scope(|s| {
        let next_slot = &next_slot;
        let spawn = |n: usize, primary: chan::Receiver<Item>, secondary: chan::Receiver<Item>| {
            for _ in 0..n {
                let (primary, secondary, plan, spec, workers) =
                    (primary.clone(), secondary.clone(), &plan, &spec, workers.clone());
                workers.fetch_add(1, Ordering::Relaxed);
                let slot = next_slot.fetch_add(1, Ordering::Relaxed);
                s.spawn(move || {
                    // Drain our own lane, then the secondary one (if any).
                    for item in primary.iter().chain(secondary.iter()) {
                        if !shared.checkpoint() {
                            break;
                        }
                        shared.active.fetch_add(1, Ordering::Relaxed);
                        shared.set_slot(slot, Some(&item));
                        process(&item, plan, spec, shared);
                        shared.set_slot(slot, None);
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
        if method == Method::Native {
            spawn(plan.small_workers, small_rx.clone(), closed_rx);
            spawn(plan.large_workers, large_rx.clone(), small_rx.clone());
        } else {
            workers.fetch_add(1, Ordering::Relaxed);
            let (workers, tops, spec, plan) = (workers.clone(), &tops, &spec, &plan);
            s.spawn(move || {
                run_tool(method, tops, spec, plan, shared);
                workers.fetch_sub(1, Ordering::Relaxed);
            });
        }
        if let Some((small_tx, large_tx)) = feed_lanes.take() {
            let (dir_tx, dir_rx) = chan::unbounded::<PathBuf>();
            let native = method == Method::Native;
            if native {
                for d in dirs.drain(..) {
                    let _ = dir_tx.send(d);
                }
                s.spawn(move || {
                    for d in dir_rx.iter() {
                        if shared.cancelled() {
                            break;
                        }
                        shared.ensure_dir(&d, true);
                    }
                });
            }
            s.spawn(move || feed(scan_rx, small_tx, large_tx, dir_tx, large_threshold, native, shared));
        }

        // ---- Progress ticker -------------------------------------------------
        let mut last = Instant::now();
        let (mut last_bytes, mut last_files) = (0u64, renamed);
        let (mut bps, mut fps) = (0f64, 0f64);
        let mut tracker = EtaTracker::with_head_start(head_start);
        let mut live = LiveRate::default();
        loop {
            // Report every `progress_ms`, but notice at once when the workers are done:
            // sleeping the whole interval made every job end up to that much late.
            let tick = Duration::from_millis(budget.progress_ms.clamp(50, 2000));
            let waited = Instant::now();
            while waited.elapsed() < tick && workers.load(Ordering::Relaxed) > 0 {
                std::thread::sleep(Duration::from_millis(5));
            }
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
            // Copying began before the count was over: the totals are what has been found so far.
            let counting = go_early && !shared.scan_done.load(Ordering::Relaxed);
            let (files_total, bytes_total) = if go_early {
                (renamed + shared.total_files.load(Ordering::Relaxed), shared.total_bytes.load(Ordering::Relaxed))
            } else {
                (files_total, bytes_total)
            };
            let files_left = files_total.saturating_sub(files + skipped + errors);
            let bytes_left = bytes_total.saturating_sub(bytes);
            let est = shared.model.lock().estimate(files_left, bytes_left, alive.max(1));

            shared.fill(&mut progress);
            progress.phase = if state == ST_PAUSE { Phase::Paused } else { Phase::Running };
            progress.files_done = files;
            progress.bytes_done = bytes;
            progress.skipped = skipped;
            progress.conflict = shared.asker.lock().pending.clone();
            progress.bytes_per_sec = bps;
            progress.files_per_sec = fps;
            let copying_now = copy_started.elapsed().as_secs_f64();
            let done_share = {
                let f = if files_total > 0 { (files + skipped + errors) as f64 / files_total as f64 } else { 0.0 };
                let b = if bytes_total > 0 { bytes as f64 / bytes_total as f64 } else { f };
                f.max(b).min(1.0)
            };
            progress.counting = counting;
            progress.files_total = files_total;
            progress.bytes_total = bytes_total;
            if counting {
                // No honest estimate while the size of the job is still growing.
                progress.eta_secs = None;
                progress.eta_confidence = 0.0;
            } else if state == ST_RUN {
                // What the cost model expects is only a start: the measured pace of the job takes over within seconds.
                let weighted = {
                    let f = if files_total > 0 { ((files + skipped + errors) as f64 / files_total as f64).min(1.0) } else { 0.0 };
                    let b = if bytes_total > 0 { (bytes as f64 / bytes_total as f64).min(1.0) } else { f };
                    file_weight * f + (1.0 - file_weight) * b
                };
                live.update(copying_now, weighted);
                let raw = blend_estimates(est.map(|e| e.seconds), live.remaining(weighted), copying_now, weighted);
                let (eta, conf) = tracker.update(raw, dt, done_share, copying_now);
                progress.eta_secs = eta;
                progress.eta_confidence = conf;
            }
            if let Some(e) = est {
                progress.display = e.display;
                progress.overhead_share = e.overhead_share;
            }
            progress.current = shared.current.lock().clone();
            progress.active_files = shared.slots.lock().iter().flatten().cloned().collect();
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
            if !counting {
                last_fraction = last_fraction.max(file_weight * by_files + (1.0 - file_weight) * by_bytes).min(0.999);
            }
            progress.fraction = last_fraction;
            if alive == 0 {
                break;
            }
            on_progress(progress.clone());
        }
    });

    // ---- Phase 3: finish a copy-based move -----------------------------------
    let cancelled = shared.cancelled();
    if go_early {
        // The count finished while copying; its totals are the job's totals.
        files_total = renamed + shared.total_files.load(Ordering::Relaxed);
        bytes_total = shared.total_bytes.load(Ordering::Relaxed);
        moved_dirs = std::mem::take(&mut *shared.moved_dirs.lock());
    }
    progress.files_total = files_total;
    progress.bytes_total = bytes_total;
    progress.counting = false;
    // A move only removes folders it has seen to the end.
    if spec.op == Op::Move && !cancelled && shared.scan_done.load(Ordering::Relaxed) {
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
    progress.conflict = None;
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
    shared.fill(&mut progress);
    progress.phase = if cancelled {
        Phase::Cancelled
    } else if errors > 0 && progress.files_done == 0 {
        Phase::Failed
    } else {
        Phase::Done
    };
    on_progress(progress);
}

/// Run a system copy tool over each top-level item, feeding its progress into
/// the shared counters the ticker reports from. Pausing stops the tool; resuming
/// starts it again, and both tools skip what is already there, so it carries on
/// where it left off. Cancelling stops it for good.
fn run_tool(method: Method, tops: &[method::Top], spec: &TransferSpec, plan: &Plan, shared: &Arc<Shared>) {
    use std::process::Stdio;
    let workers = plan.small_workers.max(plan.large_workers);
    for top in tops {
        'again: loop {
            if !shared.checkpoint() {
                return;
            }
            // Already-finished items are left alone on a restart after a pause.
            let mut cmd = method::command(method, top, if spec.conflict == Conflict::Ask { Conflict::Skip } else { spec.conflict }, spec.preserve_times, workers);
            cmd.stdin(Stdio::null());
            if method.reports_on_stderr() {
                cmd.stdout(Stdio::null()).stderr(Stdio::piped());
            } else {
                cmd.stdout(Stdio::piped()).stderr(Stdio::null());
            }
            method::no_window(&mut cmd);
            *shared.current.lock() = top.src.to_string_lossy().into_owned();
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    shared.error(&top.src, &e);
                    break 'again;
                }
            };
            let out: Box<dyn Read + Send> = if method.reports_on_stderr() {
                match child.stderr.take() {
                    Some(o) => Box::new(o),
                    None => {
                        let _ = child.kill();
                        break 'again;
                    }
                }
            } else {
                match child.stdout.take() {
                    Some(o) => Box::new(o),
                    None => {
                        let _ = child.kill();
                        break 'again;
                    }
                }
            };
            // Output is read on its own thread so that pause and cancel are noticed while it is quiet.
            let (tx, rx) = chan::unbounded::<String>();
            let reader = std::thread::spawn(move || {
                let mut out = io::BufReader::new(out);
                let mut buf = Vec::new();
                let mut byte = [0u8; 1];
                // Split on both \n and \r: rsync redraws its counter with \r.
                while out.read(&mut byte).map(|n| n == 1).unwrap_or(false) {
                    if byte[0] == b'\n' || byte[0] == b'\r' {
                        if !buf.is_empty() {
                            let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
                            buf.clear();
                        }
                    } else {
                        buf.push(byte[0]);
                    }
                }
                if !buf.is_empty() {
                    let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
                }
            });
            let mut credited = 0u64; // bytes already counted of the file in flight
            let (mut total_bytes, mut total_files) = (0u64, 0u64); // for tools that report running totals
            // Robocopy names a file before copying it and says nothing while it goes, so the files
            // it has begun are followed by their size on disk: the speed graph then shows a real
            // speed, not the whole file at once when it starts.
            let mut flights: Vec<Flight> = Vec::new();
            // The files Robocopy says it could not copy, each with the reason it gave on the line after.
            let mut failures: Vec<(method::RobocopyFailure, String)> = Vec::new();
            let mut awaiting_reason = false;
            let mut polled = Instant::now();
            let mut interrupted = false;
            loop {
                if !flights.is_empty() && polled.elapsed() >= Duration::from_millis(100) {
                    polled = Instant::now();
                    Flight::settle(&mut flights, shared, false);
                }
                match rx.recv_timeout(Duration::from_millis(40)) {
                    Ok(line) => match {
                        let event = method::parse(method, &line);
                        if method.is_robocopy() && failures.len() < 200 {
                            if let Some(f) = method::robocopy_failure(&line) {
                                failures.push((f, String::new()));
                                awaiting_reason = true;
                            } else if awaiting_reason && matches!(event, method::Event::Nothing) && !line.trim().is_empty() {
                                if let Some(last) = failures.last_mut() {
                                    last.1 = line.trim().to_string();
                                }
                                awaiting_reason = false;
                            }
                        }
                        event
                    } {
                        method::Event::Started { size, name } => {
                            flights.push(Flight { dst: flight_destination(top, &name), size, credited: 0, born: Instant::now() });
                            Flight::settle(&mut flights, shared, false);
                        }
                        method::Event::Done(size) => {
                            shared.bytes_done.fetch_add(size.saturating_sub(credited), Ordering::Relaxed);
                            shared.files_done.fetch_add(1, Ordering::Relaxed);
                            credited = 0;
                        }
                        method::Event::Partial(n) => {
                            shared.bytes_done.fetch_add(n.saturating_sub(credited), Ordering::Relaxed);
                            credited = credited.max(n);
                        }
                        method::Event::Total { bytes, files } => {
                            shared.bytes_done.fetch_add(bytes.saturating_sub(total_bytes), Ordering::Relaxed);
                            shared.files_done.fetch_add(files.saturating_sub(total_files), Ordering::Relaxed);
                            total_bytes = total_bytes.max(bytes);
                            total_files = total_files.max(files);
                        }
                        method::Event::Nothing => {}
                    },
                    Err(chan::RecvTimeoutError::Timeout) => {}
                    Err(chan::RecvTimeoutError::Disconnected) => break,
                }
                if shared.control.load(Ordering::Relaxed) != ST_RUN {
                    interrupted = true;
                    let _ = child.kill();
                    break;
                }
            }
            let status = child.wait();
            let _ = reader.join();
            if !interrupted {
                Flight::settle(&mut flights, shared, true);
            }
            if interrupted {
                if shared.cancelled() {
                    return;
                }
                continue 'again; // paused: wait at the checkpoint, then start again
            }
            match status {
                Ok(st) if method::succeeded(method, st.code().unwrap_or(-1)) => {}
                Ok(st) if method.is_robocopy() && !failures.is_empty() => report_robocopy_failures(shared, top, failures, st.code().unwrap_or(-1)),
                Ok(st) => shared.error(&top.src, &io::Error::other(format!("{} stopped with code {}", method.label(), st.code().unwrap_or(-1)))),
                Err(e) => shared.error(&top.src, &e),
            }
            break;
        }
    }
}

/// The parts of `path` below `base`, ignoring case and the kind of slash (Windows paths).
fn below(path: &str, base: &Path) -> Option<Vec<String>> {
    let norm = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    let (p, b) = (norm(path), norm(&base.to_string_lossy()));
    let rest = if p == b { "" } else { p.strip_prefix(&b)?.strip_prefix('\\')? };
    // The names are taken from the original spelling; lowercasing does not change where the parts split.
    let cut = path.trim_end_matches(['/', '\\']).len().saturating_sub(rest.len());
    let original = path.trim_end_matches(['/', '\\']).get(cut..)?;
    Some(original.split(['/', '\\']).filter(|c| !c.is_empty()).map(str::to_string).collect())
}

fn join_parts(base: &Path, parts: &[String]) -> PathBuf {
    parts.iter().fold(base.to_path_buf(), |p, c| p.join(c))
}

/// Robocopy stopped with failures: name each file (or folder) that failed, where it was being read and
/// where it was going, instead of blaming the whole folder it was asked to copy.
fn report_robocopy_failures(shared: &Shared, top: &method::Top, failures: Vec<(method::RobocopyFailure, String)>, code: i32) {
    let mut seen: Vec<(PathBuf, PathBuf, String, bool)> = Vec::new();
    for (f, reason) in failures {
        let (src, dst) = if let Some(rel) = below(&f.path, &top.src).or_else(|| below(&f.path, &top.dst)) {
            (join_parts(&top.src, &rel), join_parts(&top.dst, &rel))
        } else {
            (top.src.clone(), top.dst.clone())
        };
        let message = if reason.is_empty() { format!("Robocopy could not copy it (error {}, code {code})", f.code) } else { reason };
        // Access denied: running it again as an administrator can work.
        let elevate = f.code == 5;
        match seen.iter_mut().find(|(s, ..)| *s == src) {
            Some(entry) => *entry = (src, dst, message, elevate), // the last try is the one that counts
            None => seen.push((src, dst, message, elevate)),
        }
    }
    for (src, dst, message, elevate) in seen {
        shared.error_to(&src, &dst, message, elevate);
    }
}

/// A file a copy tool has begun, followed by how much of it is on disk.
struct Flight {
    dst: Option<PathBuf>,
    size: u64,
    credited: u64,
    born: Instant,
}

impl Flight {
    /// Count what has been written since the last look. A file is finished when all of it is
    /// there, or (with `all`) when the tool has ended. A file that cannot be found by name is
    /// counted as soon as it is clear it cannot be followed, as before.
    fn settle(flights: &mut Vec<Flight>, shared: &Shared, all: bool) {
        flights.retain_mut(|f| {
            let on_disk = f.dst.as_ref().and_then(|d| fs::metadata(d).ok()).map(|m| m.len());
            let unfollowable = on_disk.is_none() && f.born.elapsed() >= Duration::from_millis(1500);
            let reach = if all || unfollowable { f.size } else { on_disk.unwrap_or(0).min(f.size) };
            shared.bytes_done.fetch_add(reach.saturating_sub(f.credited), Ordering::Relaxed);
            f.credited = f.credited.max(reach);
            let finished = f.credited >= f.size;
            if finished {
                shared.files_done.fetch_add(1, Ordering::Relaxed);
            }
            !finished
        });
    }
}

/// Where Robocopy is writing the file it read from `name` (a full path, or only a name).
fn flight_destination(top: &method::Top, name: &str) -> Option<PathBuf> {
    if !top.is_dir {
        return Some(top.dst.clone());
    }
    let name = Path::new(name.trim());
    match name.strip_prefix(&top.src) {
        Ok(rel) => Some(top.dst.join(rel)),
        // Not under the folder as written (another spelling): a file directly in it can still be found.
        Err(_) => name.file_name().map(|n| top.dst.join(n)),
    }
}

/// Threads used to walk folders. Listing a folder is mostly waiting (for the disk, and on a
/// share for the network), so more than one folder at a time is a large win on both.
const SCAN_THREADS: usize = 8;
/// Folders read at once on a stick, card or spinning disk.
const SLOW_SCAN_THREADS: usize = 2;

/// Recursively collect files and directories under `src`. Folders are listed in parallel;
/// the result is in the same order as a sequential walk (a folder, its files, then each
/// subfolder in turn), which keeps files that sit together on disk together in the queue.
fn scan(src: &Path, dst: &Path, shared: &Shared, tx: &chan::Sender<Found>, moved: &parking_lot::Mutex<Vec<PathBuf>>) {
    // Over a network the walk waits on the wire, not the disk: more folders are read at once.
    let threads = if method::NetMap::read().kind(src).is_some() {
        SCAN_THREADS * 3
    } else if shared.slow_source {
        SLOW_SCAN_THREADS
    } else {
        SCAN_THREADS
    };
    let walk = || scan_dir(src, dst, shared, tx, moved);
    match rayon::ThreadPoolBuilder::new().num_threads(threads).thread_name(|i| format!("scan-{i}")).build() {
        Ok(pool) => pool.install(walk),
        Err(_) => walk(),
    }
}

/// Passes what the walk finds on to the copy while the count is still going: files into the lanes the workers
/// drain, folders to be made, totals for the progress. Ends when the walk does (or on cancel), which closes the lanes.
fn feed(
    rx: chan::Receiver<Found>,
    small_tx: chan::Sender<Item>,
    large_tx: chan::Sender<Item>,
    dir_tx: chan::Sender<PathBuf>,
    large_threshold: u64,
    native: bool,
    shared: &Shared,
) {
    loop {
        if shared.cancelled() || shared.scan_abort.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(Found::Items(v)) => {
                shared.total_files.fetch_add(v.len() as u64, Ordering::Relaxed);
                shared.total_bytes.fetch_add(v.iter().map(|i| i.size).sum::<u64>(), Ordering::Relaxed);
                if native {
                    for i in v {
                        let _ = if i.size >= large_threshold { large_tx.send(i) } else { small_tx.send(i) };
                    }
                }
            }
            Ok(Found::Dirs(v)) => {
                if native {
                    for d in v {
                        let _ = dir_tx.send(d);
                    }
                }
            }
            Ok(Found::Done { moved_dirs }) => {
                *shared.moved_dirs.lock() = moved_dirs;
                shared.scan_done.store(true, Ordering::Relaxed);
                break;
            }
            Ok(Found::Tops { .. }) | Err(chan::RecvTimeoutError::Timeout) => {}
            Err(chan::RecvTimeoutError::Disconnected) => {
                shared.scan_done.store(true, Ordering::Relaxed);
                break;
            }
        }
    }
}

/// Files are handed over in batches this big, so a huge folder starts producing work before it is read to the end.
const SCAN_BATCH: usize = 512;

fn scan_dir(src: &Path, dst: &Path, shared: &Shared, tx: &chan::Sender<Found>, moved: &parking_lot::Mutex<Vec<PathBuf>>) {
    use rayon::prelude::*;
    if shared.cancelled() || shared.scan_abort.load(Ordering::Relaxed) {
        return;
    }
    // Tests make a walk slow (as a network share is) by naming a folder so.
    #[cfg(test)]
    if src.to_string_lossy().contains("slowscan") {
        std::thread::sleep(Duration::from_millis(120));
    }
    shared.reading_now(src);
    let _ = tx.send(Found::Dirs(vec![dst.to_path_buf()]));
    moved.lock().push(src.to_path_buf());
    let rd = match fs::read_dir(src) {
        Ok(rd) => rd,
        Err(e) => {
            shared.error(src, &e);
            return;
        }
    };
    let mut subdirs: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut batch: Vec<Item> = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let path = e.path();
        let target = dst.join(e.file_name());
        match e.file_type() {
            Ok(ft) if ft.is_dir() => subdirs.push((path, target)),
            Ok(_) => {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                shared.scanned.fetch_add(1, Ordering::Relaxed);
                shared.scan_bytes.fetch_add(size, Ordering::Relaxed);
                batch.push(Item { src: path, dst: target, size });
                if batch.len() >= SCAN_BATCH {
                    let _ = tx.send(Found::Items(std::mem::take(&mut batch)));
                }
            }
            Err(err) => shared.error(&path, &err),
        }
    }
    if !batch.is_empty() {
        let _ = tx.send(Found::Items(batch));
    }
    subdirs.par_iter().for_each(|(s, d)| scan_dir(s, d, shared, tx, moved));
}

/// Whether `dst` is, as far as its size and modified time say, the very file `src` is.
/// File systems keep times to different precision (FAT only to two seconds), hence the margin.
fn same_file(src: &Path, dst: &Path) -> bool {
    let (Ok(a), Ok(b)) = (fs::metadata(src), fs::metadata(dst)) else { return false };
    if !a.is_file() || !b.is_file() || a.len() != b.len() {
        return false;
    }
    let close = match (a.modified(), b.modified()) {
        (Ok(x), Ok(y)) => x.duration_since(y).unwrap_or_else(|e| e.duration()) <= std::time::Duration::from_secs(2),
        _ => false,
    };
    // Small files are also compared byte for byte, which is cheap; big ones go by size and time.
    close && (a.len() > 8 * 1024 * 1024 || same_bytes(src, dst))
}

/// Byte-for-byte comparison of two files already known to be the same length.
fn same_bytes(a: &Path, b: &Path) -> bool {
    use std::io::Read;
    let Ok(len) = fs::metadata(a).map(|m| m.len()) else { return false };
    let (Ok(mut x), Ok(mut y)) = (fs::File::open(a), fs::File::open(b)) else { return false };
    let (mut bx, mut by) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    let mut left = len;
    while left > 0 {
        let n = left.min(bx.len() as u64) as usize;
        if x.read_exact(&mut bx[..n]).is_err() || y.read_exact(&mut by[..n]).is_err() || bx[..n] != by[..n] {
            return false;
        }
        left -= n as u64;
    }
    true
}

fn process(item: &Item, plan: &Plan, spec: &TransferSpec, shared: &Shared) {
    if shared.lazy_dirs.load(Ordering::Relaxed) {
        if let Some(parent) = item.dst.parent() {
            shared.ensure_dir(parent, false);
        }
    }
    let mut dst = item.dst.clone();
    if spec.skip_identical && same_file(&item.src, &dst) {
        shared.skipped.fetch_add(1, Ordering::Relaxed);
        return;
    }
    if dst.exists() {
        let Some(choice) = shared.resolve(spec.conflict, &item.src, &dst) else { return };
        match choice {
            Conflict::Skip => {
                shared.skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
            Conflict::KeepBoth => dst = unique_name(&dst),
            Conflict::Overwrite | Conflict::Ask => {}
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
                    shared.error_to(&item.src, &item.dst, e.to_string(), e.kind() == io::ErrorKind::PermissionDenied);
                }
            }
        }
        Ok(false) => {
            // Cancelled mid-file: never leave a partial file behind.
            let _ = fs::remove_file(&dst);
        }
        Err(e) => {
            let _ = fs::remove_file(&dst);
            shared.error_to(&item.src, &item.dst, e.to_string(), e.kind() == io::ErrorKind::PermissionDenied);
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

    if size < plan.large_threshold && !shared.limited() {
        // 2. Small/medium file: one call into the OS fast path
        //    (copy_file_range on Linux, CopyFileEx on Windows, fcopyfile on macOS).
        let n = fs::copy(src, dst)?;
        shared.bytes_done.fetch_add(n, Ordering::Relaxed);
    } else {
        // 3. Large file: chunked copy with big buffers so progress, pause and
        //    cancel stay responsive within one file.
        let mut input = fs::File::open(src)?;
        let mut output = fs::File::create(dst)?;
        // Reserve the space up front where that really allocates (Windows). On
        // Unix, extending the length only makes a sparse file, which measured
        // slower to fill, so it is left out there.
        #[cfg(not(unix))]
        let _ = output.set_len(size);
        let mut chunk_start = Instant::now();
        let block = shared.chunk_len(plan.buffer_bytes);
        // Linux: let the kernel move each chunk without bringing it into this
        // program (measured about 1.6x faster than read and write here).
        loop {
            if !shared.checkpoint() {
                return Ok(false);
            }
            #[cfg(target_os = "linux")]
            {
                match kernel_copy_chunk(&input, &output, block) {
                    Ok(0) => break,
                    Ok(n) => {
                        shared.bytes_done.fetch_add(n as u64, Ordering::Relaxed);
                        let now = Instant::now();
                        shared.model.lock().observe_chunk(n as u64, now.duration_since(chunk_start).as_secs_f64());
                        chunk_start = now;
                        if !shared.pace(n as u64) {
                            return Ok(false);
                        }
                        continue;
                    }
                    // Not possible between these two files (another filesystem,
                    // an old kernel): carry on from here with plain reads.
                    Err(_) => {}
                }
            }
            // Everything that is not a kernel-side copy: read ahead on a second thread while
            // this one writes, so the source and the destination are busy at the same time.
            match pipelined_copy(&mut input, &mut output, block, shared, &mut chunk_start)? {
                true => break,
                false => return Ok(false),
            }
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

/// Copy the rest of `input` to `output` with reading and writing overlapped: a helper thread
/// reads the next block while this one writes the last. Returns Ok(false) if cancelled.
fn pipelined_copy(input: &mut fs::File, output: &mut fs::File, block: usize, shared: &Shared, chunk_start: &mut Instant) -> io::Result<bool> {
    // Three blocks circulate: one being read, one being written, one waiting.
    const IN_FLIGHT: usize = 3;
    let (full_tx, full_rx) = chan::bounded::<io::Result<Vec<u8>>>(IN_FLIGHT);
    let (free_tx, free_rx) = chan::bounded::<Vec<u8>>(IN_FLIGHT);
    for _ in 0..IN_FLIGHT {
        let _ = free_tx.send(vec![0u8; block]);
    }
    std::thread::scope(|s| {
        s.spawn(move || {
            // Ends when the writer lets go of the channels (done, failed or cancelled).
            while let Ok(mut buf) = free_rx.recv() {
                buf.resize(block, 0);
                match input.read(&mut buf) {
                    Ok(0) => return,
                    Ok(n) => {
                        buf.truncate(n);
                        if full_tx.send(Ok(buf)).is_err() {
                            return;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                        // Not a failure: read again into the same block.
                        loop {
                            match input.read(&mut buf) {
                                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                                Ok(0) => return,
                                Ok(n) => {
                                    buf.truncate(n);
                                    if full_tx.send(Ok(buf)).is_err() {
                                        return;
                                    }
                                    break;
                                }
                                Err(e) => {
                                    let _ = full_tx.send(Err(e));
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = full_tx.send(Err(e));
                        return;
                    }
                }
            }
        });
        let result = (|| -> io::Result<bool> {
            loop {
                if !shared.checkpoint() {
                    return Ok(false);
                }
                // The reader hanging up with nothing queued means the input is finished.
                let Ok(next) = full_rx.recv() else { return Ok(true) };
                let buf = next?;
                output.write_all(&buf)?;
                shared.bytes_done.fetch_add(buf.len() as u64, Ordering::Relaxed);
                let now = Instant::now();
                shared.model.lock().observe_chunk(buf.len() as u64, now.duration_since(*chunk_start).as_secs_f64());
                *chunk_start = now;
                let n = buf.len() as u64;
                let _ = free_tx.send(buf);
                if !shared.pace(n) {
                    return Ok(false);
                }
            }
        })();
        // Let go of both channels before the scope waits for the reader, or a reader that is
        // waiting for a free block would never end.
        drop(full_rx);
        drop(free_tx);
        result
    })
}

/// Copy up to `len` bytes from where `input` is to where `output` is, inside the kernel.
/// Returns 0 at the end of the input.
#[cfg(target_os = "linux")]
fn kernel_copy_chunk(input: &fs::File, output: &fs::File, len: usize) -> io::Result<usize> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: both descriptors are open for the duration of the call, and null offsets
        // make the kernel use and advance the files' own positions.
        let n = unsafe { libc::copy_file_range(input.as_raw_fd(), std::ptr::null_mut(), output.as_raw_fd(), std::ptr::null_mut(), len, 0) };
        if n >= 0 {
            return Ok(n as usize);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
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
            preserve_times: true, large_threshold: 64 * 1024, max_parallel: 0, method: MethodPref::Native, skip_identical: true, active_files: 0, speed_test: false, early_start_ms: 0,
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

    #[test]
    fn a_robocopy_failure_names_the_file_and_where_it_was_going() {
        let shared = test_shared();
        let top = method::Top { src: PathBuf::from("C:\\src\\survival lol"), dst: PathBuf::from("D:\\copy\\survival lol"), is_dir: true };
        let f = |code, path: &str| method::RobocopyFailure { code, path: path.into() };
        report_robocopy_failures(
            &shared,
            &top,
            vec![
                (f(32, "C:\\src\\Survival Lol\\region\\r.0.0.mca"), "in use".into()),
                (f(32, "C:\\src\\survival lol\\region\\r.0.0.mca"), "still in use".into()), // the retry of the same file
                (f(5, "D:\\copy\\survival lol\\data"), String::new()),
            ],
            9,
        );
        let errs = shared.errors.lock();
        assert_eq!(shared.error_count.load(Ordering::Relaxed), 2);
        assert_eq!(errs[0].path.replace('/', "\\"), "C:\\src\\survival lol\\region\\r.0.0.mca");
        assert_eq!(errs[0].dest.as_deref().unwrap().replace('/', "\\"), "D:\\copy\\survival lol\\region\\r.0.0.mca");
        assert_eq!(errs[0].message, "still in use");
        assert!(!errs[0].needs_elevation);
        assert_eq!(errs[1].path.replace('/', "\\"), "C:\\src\\survival lol\\data");
        assert!(errs[1].needs_elevation);
        assert!(errs[1].message.contains("error 5"));
    }

    #[test]
    fn a_tool_file_is_followed_by_its_size_on_disk() {
        let d = tempfile::tempdir().unwrap();
        let top = method::Top { src: PathBuf::from("/src/photos"), dst: d.path().join("photos"), is_dir: true };
        std::fs::create_dir_all(top.dst.join("sub")).unwrap();
        // The folder is named as written in the command, so the sub-folder is kept.
        assert_eq!(flight_destination(&top, "/src/photos/sub/a.bin"), Some(top.dst.join("sub/a.bin")));
        // Another spelling of the folder: a file name alone still finds it directly inside.
        assert_eq!(flight_destination(&top, "/SRC/Photos/a.bin"), Some(top.dst.join("a.bin")));
        let single = method::Top { src: "/a/x.iso".into(), dst: "/b/x.iso".into(), is_dir: false };
        assert_eq!(flight_destination(&single, "x.iso"), Some(PathBuf::from("/b/x.iso")));

        let shared = test_shared();
        let target = top.dst.join("sub/a.bin");
        let mut flights = vec![Flight { dst: Some(target.clone()), size: 1000, credited: 0, born: Instant::now() }];
        Flight::settle(&mut flights, &shared, false);
        assert_eq!((shared.bytes_done.load(Ordering::Relaxed), flights.len()), (0, 1), "nothing written yet: nothing counted");
        std::fs::write(&target, vec![0u8; 400]).unwrap();
        Flight::settle(&mut flights, &shared, false);
        assert_eq!(shared.bytes_done.load(Ordering::Relaxed), 400, "counted as it is written, not all at the start");
        assert_eq!(shared.files_done.load(Ordering::Relaxed), 0);
        std::fs::write(&target, vec![0u8; 1000]).unwrap();
        Flight::settle(&mut flights, &shared, false);
        assert_eq!((shared.bytes_done.load(Ordering::Relaxed), shared.files_done.load(Ordering::Relaxed), flights.len()), (1000, 1, 0));
    }

    #[test]
    fn a_tool_file_that_cannot_be_followed_is_counted_when_the_tool_ends() {
        let shared = test_shared();
        let mut flights = vec![Flight { dst: None, size: 500, credited: 0, born: Instant::now() }];
        Flight::settle(&mut flights, &shared, false);
        assert_eq!(flights.len(), 1);
        Flight::settle(&mut flights, &shared, true);
        assert_eq!((shared.bytes_done.load(Ordering::Relaxed), shared.files_done.load(Ordering::Relaxed), flights.len()), (500, 1, 0));
    }

    fn test_shared() -> Shared {
        Shared {
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
            ask_gate: Mutex::new(()),
            asker: Mutex::new(Asker::default()),
            priority: Arc::new(AtomicU8::new(throttle::NORMAL)),
            start_now: Arc::new(AtomicBool::new(false)),
            limiter: throttle::Limiter::new(0),
            max_secs: AtomicU64::new(0),
            stop_on_timeout: AtomicBool::new(true),
            timed_out: AtomicBool::new(false),
            run_started: Mutex::new(None),
            scanned: AtomicU64::new(0),
            scan_bytes: AtomicU64::new(0),
            scan_abort: AtomicBool::new(false),
            scan_done: AtomicBool::new(false),
            total_files: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            moved_dirs: Mutex::new(Vec::new()),
            lazy_dirs: AtomicBool::new(false),
            made_dirs: Mutex::new(Default::default()),
            gate_released: AtomicBool::new(false),
            slots: Mutex::new(Vec::new()),
            reading: Mutex::new((String::new(), Instant::now())),
            slow_source: false,
        }
    }

    #[test]
    fn the_count_says_where_a_slow_drive_is_making_it_wait() {
        let s = test_shared();
        assert_eq!(s.counting_text(), "Reading the folders…");
        s.scanned.store(3, Ordering::Relaxed);
        s.reading_now(Path::new("/media/usb/DCIM"));
        assert_eq!(s.counting_text(), "Counting files… 3 so far (in \u{201c}DCIM\u{201d})");
        *s.reading.lock() = ("DCIM".into(), Instant::now() - Duration::from_secs(6));
        let text = s.counting_text();
        assert!(text.contains("slow to answer") && text.contains("DCIM"), "{text}");
        assert!(!text.contains('/') && !text.contains('\\'), "the card shows only what follows the last separator: {text}");
    }

    #[test]
    fn workers_report_what_they_are_copying_only_when_asked() {
        let s = test_shared();
        let item = Item { src: PathBuf::from("/a/b.bin"), dst: PathBuf::from("/c/b.bin"), size: 5 };
        s.set_slot(0, Some(&item));
        assert!(s.slots.lock().is_empty(), "nothing is tracked unless the spec asked for it");
        *s.slots.lock() = vec![None; 2];
        s.set_slot(1, Some(&item));
        s.set_slot(5, Some(&item));
        assert_eq!(s.slots.lock().iter().flatten().cloned().collect::<Vec<_>>(), vec![ActiveFile { path: "/a/b.bin".into(), size: 5 }]);
        s.set_slot(1, None);
        assert!(s.slots.lock().iter().flatten().next().is_none());
    }

    #[test]
    fn a_speed_limit_holds_a_copy_back_and_stays_exact() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..1_000_000usize).map(|i| (i * 7 % 253) as u8).collect();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        fs::write(&a, &data).unwrap();
        let shared = test_shared();
        shared.limiter.set_rate(1_000_000);
        let plan = strategy::choose(&JobProfile { file_count: 1, total_bytes: 1_000_000, large_bytes: 0, large_count: 0, same_volume: false, is_move: false, src: crate::drives::Media::Ssd, dst: crate::drives::Media::Ssd }, &Budget::minimal(), 64 * 1024 * 1024, 0);
        let spec = TransferSpec { sources: vec![], dest_dir: dir.path().to_path_buf(), op: Op::Copy, conflict: Conflict::Overwrite, preserve_times: false, large_threshold: 0, max_parallel: 0, method: MethodPref::Native, skip_identical: true, active_files: 0, speed_test: false, early_start_ms: 0 };
        shared.clone_ok.store(false, Ordering::Relaxed);
        let t = Instant::now();
        assert!(copy_one(&a, &b, 1_000_000, &plan, &spec, &shared).unwrap());
        assert!(t.elapsed() >= Duration::from_millis(700), "1 MB at 1 MB/s took {:?}", t.elapsed());
        assert_eq!(fs::read(&b).unwrap(), data);
        assert_eq!(shared.bytes_done.load(Ordering::Relaxed), 1_000_000);
    }

    #[test]
    fn the_time_limit_stops_or_pauses() {
        let s = test_shared();
        *s.run_started.lock() = Some(Instant::now() - Duration::from_secs(10));
        s.max_secs.store(5, Ordering::Relaxed);
        assert!(!s.checkpoint(), "stopped");
        assert!(s.timed_out.load(Ordering::Relaxed));
        let p = test_shared();
        *p.run_started.lock() = Some(Instant::now() - Duration::from_secs(10));
        p.max_secs.store(5, Ordering::Relaxed);
        p.stop_on_timeout.store(false, Ordering::Relaxed);
        p.check_deadline();
        assert_eq!(p.control.load(Ordering::Relaxed), ST_PAUSE, "paused, ready to resume");
        assert!(p.timed_out.load(Ordering::Relaxed) && p.max_secs.load(Ordering::Relaxed) == 5);
    }

    #[test]
    fn overlapped_copy_is_exact_for_odd_sizes() {
        let dir = tempfile::tempdir().unwrap();
        for len in [0usize, 1, 4095, 4096, 4097, 3 * 65536, 3 * 65536 + 17, 1_000_003] {
            let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
            let (a, b) = (dir.path().join("a"), dir.path().join("b"));
            fs::write(&a, &data).unwrap();
            let shared = test_shared();
            let mut input = fs::File::open(&a).unwrap();
            let mut output = fs::File::create(&b).unwrap();
            let mut t = Instant::now();
            assert!(pipelined_copy(&mut input, &mut output, 65536, &shared, &mut t).unwrap());
            drop(output);
            assert_eq!(fs::read(&b).unwrap(), data, "length {len}");
            assert_eq!(shared.bytes_done.load(Ordering::Relaxed), len as u64);
        }
    }

    #[test]
    fn overlapped_copy_stops_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        fs::write(&a, vec![1u8; 2_000_000]).unwrap();
        let shared = test_shared();
        shared.control.store(ST_CANCEL, Ordering::Relaxed);
        let mut input = fs::File::open(&a).unwrap();
        let mut output = fs::File::create(dir.path().join("b")).unwrap();
        let mut t = Instant::now();
        // Returns instead of hanging on its reader thread.
        assert!(!pipelined_copy(&mut input, &mut output, 65536, &shared, &mut t).unwrap());
    }

    #[test]
    fn scan_finds_everything_in_walk_order() {
        let src = tempfile::tempdir().unwrap();
        make_tree(src.path());
        let shared = test_shared();
        let (tx, rx) = chan::unbounded();
        let moved = parking_lot::Mutex::new(Vec::new());
        scan(src.path(), Path::new("/dst"), &shared, &tx, &moved);
        drop(tx);
        let (mut items, mut dirs): (Vec<Item>, Vec<PathBuf>) = (Vec::new(), Vec::new());
        for f in rx.iter() {
            match f {
                Found::Items(v) => items.extend(v),
                Found::Dirs(v) => dirs.extend(v),
                _ => {}
            }
        }
        let moved = moved.into_inner();
        assert_eq!(items.len(), 40 + 1 + 1);
        assert_eq!(dirs.len(), 4); // root, a, a/b, empty
        assert_eq!(dirs[0], Path::new("/dst"));
        assert_eq!(moved[0], src.path());
        assert!(items.iter().all(|i| i.dst.starts_with("/dst")));
    }

    #[test]
    fn copying_the_same_files_again_keeps_and_skips_them() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        make_tree(src.path());
        let first = run_job(spec(vec![src.path().to_path_buf()], dst.path(), Op::Copy, Conflict::KeepBoth));
        assert_eq!(first.error_count, 0);
        // Same again with "keep both": nothing new is made, nothing is asked, nothing fails.
        let second = run_job(spec(vec![src.path().to_path_buf()], dst.path(), Op::Copy, Conflict::KeepBoth));
        assert_eq!((second.error_count, second.phase), (0, Phase::Done));
        assert_eq!(second.skipped, 42, "every file was already there");
        let names: Vec<_> = fs::read_dir(dst.path().join(src.path().file_name().unwrap())).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(!names.iter().any(|n| n.contains("(2)")), "{names:?}");
        // A file that changed is not the same file.
        fs::write(src.path().join("top.txt"), b"changed!").unwrap();
        let third = run_job(spec(vec![src.path().to_path_buf()], dst.path(), Op::Copy, Conflict::Overwrite));
        assert_eq!(third.skipped, 41);
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
                shape_match: 1.0,
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
        // Its numbers are real, so the graph and history have something to show.
        assert_eq!(p.files_done, 1);
        assert!(p.avg_files_per_sec > 0.0 && p.peak_files_per_sec >= p.avg_files_per_sec);
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

    /// Run an "ask" job and answer each question with `decide(file name, question number)`.
    fn run_asking(s: TransferSpec, decide: impl Fn(&str, u64) -> (Resolution, bool)) -> (Progress, Vec<String>) {
        let last = Arc::new(StdMutex::new(None::<Progress>));
        let l2 = last.clone();
        let budget = Budget { io_threads: 4, ..Budget::minimal() };
        let h = start(1, s, budget, Vec::new(), None, move |p| *l2.lock().unwrap() = Some(p));
        let mut asked = Vec::new();
        let t0 = Instant::now();
        while !h.is_finished() && t0.elapsed() < Duration::from_secs(20) {
            let c = last.lock().unwrap().as_ref().and_then(|p| p.conflict.clone());
            match c {
                Some(c) if !asked.contains(&c.name) => {
                    asked.push(c.name.clone());
                    let (r, all) = decide(&c.name, c.number);
                    h.resolve(r, all);
                }
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        h.wait();
        let p = last.lock().unwrap().clone().unwrap();
        (p, asked)
    }

    fn conflict_tree(src: &Path, dst: &Path) {
        for n in ["a.txt", "b.txt", "c.txt"] {
            fs::create_dir_all(src.join("dir")).unwrap();
            fs::create_dir_all(dst.join("dir")).unwrap();
            fs::write(src.join("dir").join(n), "new").unwrap();
            fs::write(dst.join("dir").join(n), "old").unwrap();
        }
        fs::write(src.join("dir/d.txt"), "fresh").unwrap();
    }

    #[test]
    fn ask_decides_file_by_file() {
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        conflict_tree(src.path(), dst.path());
        let (p, asked) = run_asking(spec(vec![src.path().join("dir")], dst.path(), Op::Copy, Conflict::Ask), |name, _| {
            (if name == "b.txt" { Resolution::Skip } else { Resolution::Replace }, false)
        });
        assert_eq!(asked.len(), 3, "one question per clashing file, none for d.txt");
        let read = |n: &str| fs::read_to_string(dst.path().join("dir").join(n)).unwrap();
        assert_eq!((read("a.txt").as_str(), read("b.txt").as_str(), read("c.txt").as_str(), read("d.txt").as_str()), ("new", "old", "new", "fresh"));
        assert_eq!(p.skipped, 1);
        assert!(p.conflict.is_none());
    }

    #[test]
    fn ask_with_apply_to_all_asks_once() {
        for (choice, expect) in [(Resolution::Skip, "old"), (Resolution::Replace, "new")] {
            let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
            conflict_tree(src.path(), dst.path());
            let (p, asked) = run_asking(spec(vec![src.path().join("dir")], dst.path(), Op::Copy, Conflict::Ask), move |_, _| (choice, true));
            assert_eq!(asked.len(), 1);
            for n in ["a.txt", "b.txt", "c.txt"] {
                assert_eq!(fs::read_to_string(dst.path().join("dir").join(n)).unwrap(), expect);
            }
            assert_eq!(fs::read_to_string(dst.path().join("dir/d.txt")).unwrap(), "fresh");
            assert_eq!(p.skipped, if choice == Resolution::Skip { 3 } else { 0 });
        }
    }

    #[test]
    fn ask_on_a_top_level_file_and_keep_both() {
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(src.path().join("x.txt"), "new").unwrap();
        fs::write(dst.path().join("x.txt"), "old").unwrap();
        let (_, asked) = run_asking(spec(vec![src.path().join("x.txt")], dst.path(), Op::Copy, Conflict::Ask), |_, _| (Resolution::KeepBoth, false));
        assert_eq!(asked, ["x.txt"]);
        assert_eq!(fs::read_to_string(dst.path().join("x.txt")).unwrap(), "old");
        let mut names: Vec<String> = fs::read_dir(dst.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names.len(), 2, "the copy arrived under another name: {names:?}");
    }

    #[test]
    fn cancelling_while_a_question_is_open_ends_the_job() {
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(src.path().join("x.txt"), "new").unwrap();
        fs::write(dst.path().join("x.txt"), "old").unwrap();
        let last = Arc::new(StdMutex::new(None::<Progress>));
        let l2 = last.clone();
        let h = start(1, spec(vec![src.path().join("x.txt")], dst.path(), Op::Copy, Conflict::Ask), Budget { io_threads: 2, ..Budget::minimal() }, Vec::new(), None, move |p| *l2.lock().unwrap() = Some(p));
        let t0 = Instant::now();
        while last.lock().unwrap().as_ref().map_or(true, |p| p.conflict.is_none()) && t0.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
        }
        h.cancel();
        h.wait();
        assert_eq!(fs::read_to_string(dst.path().join("x.txt")).unwrap(), "old");
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

    /// A tree whose folders are slow to read (the name makes the test walk wait), with files at several depths.
    fn slow_tree(root: &Path, folders: usize) -> (PathBuf, usize) {
        let src = root.join("slowscan");
        let mut n = 0;
        for i in 0..folders {
            let d = src.join(format!("f{i}"));
            fs::create_dir_all(d.join("deep")).unwrap();
            for j in 0..3 {
                fs::write(d.join(format!("a{j}.txt")), format!("{i}-{j}")).unwrap();
                fs::write(d.join("deep").join(format!("b{j}.txt")), format!("{i}-{j}-deep")).unwrap();
                n += 2;
            }
        }
        fs::create_dir_all(src.join("empty")).unwrap();
        (src, n)
    }

    #[test]
    fn copying_begins_before_the_count_is_over_and_ends_exactly() {
        let d = tempfile::tempdir().unwrap();
        let (src, n) = slow_tree(d.path(), 12);
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let mut s = spec(vec![src.clone()], &dest, Op::Copy, Conflict::Overwrite);
        s.early_start_ms = 60;
        let seen = Arc::new(StdMutex::new(Vec::<Progress>::new()));
        let s2 = seen.clone();
        let budget = Budget { io_threads: 4, progress_ms: 50, ..Budget::minimal() };
        let h = start(7, s, budget, Vec::new(), None, move |p| s2.lock().unwrap().push(p));
        h.wait();
        let seen = seen.lock().unwrap();
        let last = seen.last().unwrap();
        assert_eq!(last.phase, Phase::Done, "{last:?}");
        assert_eq!((last.files_done, last.files_total), (n as u64, n as u64), "{last:?}");
        assert!(!last.counting);
        assert!(seen.iter().any(|p| p.counting && p.files_done > 0), "files were copied while the count was still going");
        assert!(seen.iter().filter(|p| p.counting).all(|p| p.eta_secs.is_none()), "no estimate while counting");
        // Every file, every folder (also the empty one), with its content.
        assert!(dest.join("slowscan/empty").is_dir());
        for i in 0..12 {
            assert_eq!(fs::read(dest.join(format!("slowscan/f{i}/deep/b2.txt"))).unwrap(), format!("{i}-2-deep").into_bytes());
        }
    }

    #[test]
    fn a_small_job_never_goes_early() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        fs::create_dir_all(&src).unwrap();
        for i in 0..5 {
            fs::write(src.join(format!("{i}.txt")), b"x").unwrap();
        }
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let mut s = spec(vec![src], &dest, Op::Copy, Conflict::Overwrite);
        s.early_start_ms = 500;
        let seen = Arc::new(StdMutex::new(Vec::<Progress>::new()));
        let s2 = seen.clone();
        let h = start(8, s, Budget { io_threads: 4, ..Budget::minimal() }, Vec::new(), None, move |p| s2.lock().unwrap().push(p));
        h.wait();
        let seen = seen.lock().unwrap();
        assert!(seen.iter().all(|p| !p.counting));
        assert_eq!(seen.last().unwrap().files_done, 5);
    }

    #[test]
    fn cancelling_during_the_count_stops_at_once() {
        let d = tempfile::tempdir().unwrap();
        let (src, n) = slow_tree(d.path(), 40);
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let mut s = spec(vec![src], &dest, Op::Copy, Conflict::Overwrite);
        s.early_start_ms = 40;
        let last = Arc::new(StdMutex::new(None::<Progress>));
        let l2 = last.clone();
        let h = start(9, s, Budget { io_threads: 4, ..Budget::minimal() }, Vec::new(), None, move |p| *l2.lock().unwrap() = Some(p));
        std::thread::sleep(Duration::from_millis(500));
        let t = Instant::now();
        h.cancel();
        h.wait();
        assert!(t.elapsed() < Duration::from_secs(2), "took {:?}", t.elapsed());
        let p = last.lock().unwrap().clone().unwrap();
        assert_eq!(p.phase, Phase::Cancelled);
        assert!(p.files_done < n as u64);
    }

    #[test]
    fn a_move_that_began_early_removes_its_source_folders_when_done() {
        let d = tempfile::tempdir().unwrap();
        let (src, n) = slow_tree(d.path(), 8);
        let dest_root = d.path().join("elsewhere");
        fs::create_dir(&dest_root).unwrap();
        // A rename would finish the move instantly: ask for a copy-based one by making the destination exist.
        let dest = dest_root.clone();
        fs::create_dir_all(dest.join("slowscan")).unwrap();
        let mut s = spec(vec![src.clone()], &dest, Op::Move, Conflict::Overwrite);
        s.early_start_ms = 60;
        let p = run_job(s);
        assert_eq!(p.phase, Phase::Done, "{p:?}");
        assert_eq!(p.files_done, n as u64);
        assert!(!src.exists(), "the emptied source folders are gone");
        assert!(dest.join("slowscan/f3/a1.txt").exists());
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

    #[cfg(unix)]
    #[test]
    fn rsync_copies_a_tree_and_reports_progress() {
        if !method::installed().contains(&Method::Rsync) {
            return; // rsync is not installed here
        }
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        make_tree(src.path());
        let mut sp = spec(vec![src.path().join("a")], dst.path(), Op::Copy, Conflict::Overwrite);
        sp.method = MethodPref::Rsync;
        let p = run_job(sp);
        assert_eq!(p.method, Some(Method::Rsync));
        assert_eq!(p.phase, Phase::Done, "{:?}", p.errors);
        assert!(dst.path().join("a/small7.txt").exists());
        assert_eq!(fs::read(dst.path().join("a/b/big.bin")).unwrap().len(), 300 * 1024);
        assert_eq!(p.files_done, p.files_total, "{p:?}");
        assert!(p.bytes_done >= p.bytes_total);
        // Skip leaves an existing file untouched.
        fs::write(dst.path().join("a/small7.txt"), "mine").unwrap();
        let mut sp = spec(vec![src.path().join("a")], dst.path(), Op::Copy, Conflict::Skip);
        sp.method = MethodPref::Rsync;
        run_job(sp);
        assert_eq!(fs::read_to_string(dst.path().join("a/small7.txt")).unwrap(), "mine");
    }

    #[test]
    fn moves_and_keep_both_stay_with_the_engine() {
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        make_tree(src.path());
        let mut sp = spec(vec![src.path().join("a")], dst.path(), Op::Copy, Conflict::KeepBoth);
        sp.method = MethodPref::Rsync;
        assert_eq!(run_job(sp).method, Some(Method::Native));
    }

    #[test]
    fn speed_test_before_a_big_copy_leaves_nothing_behind() {
        let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::write(src.path().join("big.bin"), vec![3u8; 50 << 20]).unwrap();
        let mut s = spec(vec![src.path().join("big.bin")], dst.path(), Op::Copy, Conflict::Overwrite);
        s.speed_test = true;
        let p = run_job(s);
        assert_eq!(p.phase, Phase::Done);
        let names: Vec<_> = fs::read_dir(dst.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["big.bin"]);
    }
}
