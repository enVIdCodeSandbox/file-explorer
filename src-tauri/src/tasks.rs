//! The scheduler for automated tasks: keeps the list, watches the clock, folders and drives,
//! starts runs, handles sleep and shutdown, and records what happened. What a task *is* and how it
//! runs lives in `fast_core::tasks`.

use crate::state::Shared;
use fast_core::history::Route;
use fast_core::tasks::{self as core, Action, Due, DueKind, Env, Estimate, Outcome, Preview, Progress, RunRecord, RunResult, SleepPolicy, Stats, Task, Trigger};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const TICK: Duration = Duration::from_secs(5);

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

struct Live {
    cancel: Arc<AtomicBool>,
    slept: Arc<AtomicBool>,
    progress: Progress,
    started: i64,
    reason: String,
    elevated: bool,
    /// For a run as administrator: the file whose appearance asks the helper to stop.
    cancel_file: Option<PathBuf>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunningInfo {
    pub id: String,
    pub started: i64,
    pub reason: String,
    pub elevated: bool,
    pub progress: Progress,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub tasks: Vec<Task>,
    pub running: Vec<RunningInfo>,
    /// Next clock-driven run of each task, milliseconds.
    pub next: HashMap<String, i64>,
    /// Latest outcome of each task.
    pub last: HashMap<String, RunRecord>,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SavedState {
    #[serde(default)]
    last_tick: i64,
    /// Runs in progress when the app last wrote this: if they are still here at start, the app was killed.
    #[serde(default)]
    running: Vec<String>,
}

pub struct Hub {
    app: AppHandle,
    st: Shared,
    tasks: Mutex<Vec<Task>>,
    running: Mutex<HashMap<String, Live>>,
    records: Mutex<Vec<RunRecord>>,
    last_tick: AtomicI64,
    shutting_down: AtomicBool,
    /// Folder rules for the watcher: (folder, task id).
    rules: Arc<Mutex<Vec<(PathBuf, String)>>>,
    changed: Arc<Mutex<HashMap<String, Instant>>>,
    watcher: fast_core::watch::Watcher,
    drive_sig: Mutex<(u64, HashSet<String>)>,
}

fn setting_bool(st: &Shared, key: &str, default: bool) -> bool {
    st.settings.read().advanced.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}
fn setting_u64(st: &Shared, key: &str, default: u64) -> u64 {
    st.settings.read().advanced.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

impl Hub {
    fn new(app: AppHandle, st: Shared) -> Arc<Hub> {
        let tasks: Vec<Task> = std::fs::read(&st.paths.tasks).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let records = core::load_records(&st.paths.task_history);
        let rules: Arc<Mutex<Vec<(PathBuf, String)>>> = Default::default();
        let changed: Arc<Mutex<HashMap<String, Instant>>> = Default::default();
        let watcher = {
            let (rules, changed) = (rules.clone(), changed.clone());
            fast_core::watch::Watcher::new(move |dirs| {
                let rules = rules.lock();
                let mut changed = changed.lock();
                for d in dirs {
                    for (folder, id) in rules.iter() {
                        if d.starts_with(folder) {
                            changed.insert(id.clone(), Instant::now());
                        }
                    }
                }
            })
        };
        let hub = Arc::new(Hub {
            app,
            st,
            tasks: Mutex::new(tasks),
            running: Mutex::new(HashMap::new()),
            records: Mutex::new(records),
            last_tick: AtomicI64::new(now_ms()),
            shutting_down: AtomicBool::new(false),
            rules,
            changed,
            watcher,
            drive_sig: Mutex::new((0, HashSet::new())),
        });
        hub.sync_watchers();
        hub
    }

    fn enabled(&self) -> bool {
        setting_bool(&self.st, "tasks.enabled", true)
    }

    fn save_tasks(&self) {
        let list = self.tasks.lock().clone();
        if let Ok(b) = serde_json::to_vec_pretty(&list) {
            let _ = std::fs::write(&self.st.paths.tasks, b);
        }
        self.sync_watchers();
    }

    fn save_state(&self) {
        let state = SavedState { last_tick: self.last_tick.load(Ordering::Relaxed), running: self.running.lock().keys().cloned().collect() };
        if let Ok(b) = serde_json::to_vec(&state) {
            let _ = std::fs::write(&self.st.paths.task_state, b);
        }
    }

    fn sync_watchers(&self) {
        let mut rules = Vec::new();
        for t in self.tasks.lock().iter().filter(|t| t.enabled) {
            for tr in &t.triggers {
                if let Trigger::OnChange { path, .. } = tr {
                    if !path.trim().is_empty() {
                        rules.push((PathBuf::from(path), t.id.clone()));
                    }
                }
            }
        }
        let mut dirs: Vec<PathBuf> = rules.iter().map(|r| r.0.clone()).collect();
        dirs.sort();
        dirs.dedup();
        *self.rules.lock() = rules;
        self.watcher.set_dirs(dirs);
    }

    fn emit_changed(&self) {
        let _ = self.app.emit("tasks-changed", ());
    }

    pub fn listing(&self) -> Listing {
        let tasks = self.tasks.lock().clone();
        let now = now_ms();
        let next = tasks.iter().filter_map(|t| core::next_run(t, now).map(|n| (t.id.clone(), n))).collect();
        let records = self.records.lock();
        let mut last = HashMap::new();
        for r in records.iter() {
            last.insert(r.task_id.clone(), r.clone());
        }
        let running = self.running.lock().iter().map(|(id, l)| RunningInfo { id: id.clone(), started: l.started, reason: l.reason.clone(), elevated: l.elevated, progress: l.progress.clone() }).collect();
        Listing { tasks, running, next, last, enabled: self.enabled() }
    }

    /// Write a record of a run (when records are kept) and tell the window.
    fn record(&self, r: RunRecord) {
        if setting_bool(&self.st, "tasks.recordHistory", true) {
            let _ = core::append_record(&self.st.paths.task_history, &r);
            self.records.lock().push(r.clone());
            // Trimmed now and then rather than after every run.
            if self.records.lock().len() % 50 == 0 {
                self.trim();
            }
        }
        let _ = self.app.emit("task-finished", &r);
        self.emit_changed();
    }

    fn trim(&self) {
        let per = setting_u64(&self.st, "tasks.historyPerTask", 500) as usize;
        let days = setting_u64(&self.st, "tasks.historyDays", 365) as u32;
        let _ = core::trim_records(&self.st.paths.task_history, per.max(1), days, now_ms());
        *self.records.lock() = core::load_records(&self.st.paths.task_history);
    }

    fn skipped(&self, task: &Task, reason: &str, why: &str) {
        let now = now_ms();
        self.record(RunRecord { task_id: task.id.clone(), started: now, ended: now, outcome: Outcome::Skipped, reason: reason.into(), items: 0, bytes: 0, errors: vec![why.into()], attempts: 0, interrupted_by: None, elevated: false, name: task.name.clone() });
    }

    /// Start a run. Returns an explanation when it did not start.
    pub fn start(self: &Arc<Self>, id: &str, reason: &str, as_admin: bool) -> Result<(), String> {
        let Some(task) = self.tasks.lock().iter().find(|t| t.id == id).cloned() else { return Err("No such task.".into()) };
        let manual = reason == "manual";
        if !self.enabled() {
            return Err("Automated tasks are turned off in Settings.".into());
        }
        if self.shutting_down.load(Ordering::Relaxed) {
            return Err("The app is closing.".into());
        }
        if self.running.lock().contains_key(id) {
            if !manual {
                self.skipped(&task, reason, "The previous run was still going.");
            }
            return Err("This task is already running.".into());
        }
        if task.options.only_on_ac && !manual && fast_core::resources::snapshot().on_battery {
            self.skipped(&task, reason, "The computer was running on battery.");
            return Ok(());
        }
        let elevated = as_admin || task.options.run_as_admin;
        let live = Live {
            cancel: Arc::new(AtomicBool::new(false)),
            slept: Arc::new(AtomicBool::new(false)),
            progress: Progress::default(),
            started: now_ms(),
            reason: reason.into(),
            elevated,
            cancel_file: None,
        };
        let (cancel, slept) = (live.cancel.clone(), live.slept.clone());
        self.running.lock().insert(id.to_string(), live);
        {
            let mut tasks = self.tasks.lock();
            if let Some(t) = tasks.iter_mut().find(|t| t.id == id) {
                t.resume_pending = false;
            }
        }
        self.save_state();
        let _ = self.app.emit("task-started", serde_json::json!({ "id": id, "reason": reason }));
        self.emit_changed();
        let hub = self.clone();
        let reason = reason.to_string();
        std::thread::Builder::new()
            .name(format!("task-{id}"))
            .spawn(move || hub.execute(task, reason, elevated, cancel, slept))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn execute(self: Arc<Self>, task: Task, reason: String, elevated: bool, cancel: Arc<AtomicBool>, slept: Arc<AtomicBool>) {
        let started = now_ms();
        let result = if elevated { self.run_elevated(&task, &cancel) } else {
            let hub = self.clone();
            let id = task.id.clone();
            let progress = move |p: &Progress| {
                if let Some(l) = hub.running.lock().get_mut(&id) {
                    l.progress = p.clone();
                }
                let _ = hub.app.emit("task-progress", serde_json::json!({ "id": id, "progress": p }));
            };
            let env = Env { cancel: &cancel, slept: &slept, on_progress: &progress, on_sleep: task.options.on_sleep };
            core::run(&task, &env, started)
        };
        let ended = now_ms();
        let closing = self.shutting_down.load(Ordering::Relaxed);
        let mut outcome = result.outcome;
        let mut interrupted_by = result.interrupted_by.clone();
        if closing && outcome == Outcome::Cancelled {
            outcome = Outcome::Interrupted;
            interrupted_by = Some("the computer or the app was shutting down".into());
        }
        self.running.lock().remove(&task.id);
        self.changed.lock().remove(&task.id);
        {
            let mut tasks = self.tasks.lock();
            if let Some(t) = tasks.iter_mut().find(|t| t.id == task.id) {
                t.needs_admin = result.denied && !elevated;
                // Shutting down in the middle of a run: carry on at the next start, unless the task would rather wait for its next turn.
                t.resume_pending = outcome == Outcome::Interrupted && closing && task.options.on_sleep == SleepPolicy::Resume;
            }
        }
        self.save_tasks();
        let record = RunRecord { task_id: task.id.clone(), started, ended, outcome, reason, items: result.items, bytes: result.bytes, errors: result.errors.clone(), attempts: result.attempts, interrupted_by, elevated, name: task.name.clone() };
        if setting_bool(&self.st, "tasks.trainEstimates", true) && matches!(outcome, Outcome::Success | Outcome::Partial) {
            self.teach_estimates(&task, &record);
        }
        // A message for the person, if this task and the settings ask for one.
        let tell = match task.options.notify {
            core::Notify::Never => false,
            core::Notify::Always => !matches!(outcome, Outcome::Cancelled),
            core::Notify::Failure => matches!(outcome, Outcome::Failed | Outcome::Partial | Outcome::Interrupted),
        };
        if tell && setting_bool(&self.st, "tasks.notify", true) {
            let what = match outcome {
                Outcome::Success => "finished".to_string(),
                Outcome::Partial => format!("finished with {} problem{}", record.errors.len(), if record.errors.len() == 1 { "" } else { "s" }),
                Outcome::Failed => format!("failed{}", record.errors.first().map(|e| format!(": {e}")).unwrap_or_default()),
                Outcome::Interrupted => "was interrupted".to_string(),
                _ => "ended".to_string(),
            };
            let _ = self.app.emit("task-notice", format!("Task \"{}\" {}.", task.name, what));
        }
        self.record(record);
        self.save_state();
    }

    /// Feed a finished copy or move to the transfer time estimates.
    fn teach_estimates(&self, task: &Task, r: &RunRecord) {
        let first = task.actions.iter().find_map(|a| match a {
            Action::Copy { from, to, .. } | Action::Move { from, to, .. } => from.first().map(|f| (f.clone(), to.clone())),
            Action::Sync { from, to, .. } => Some((from.clone(), to.clone())),
            _ => None,
        });
        let Some((from, to)) = first else { return };
        let drives = fast_core::drives::list_drives();
        let find = |p: &str| {
            drives.iter().filter(|d| std::path::Path::new(p).starts_with(&d.mount)).max_by_key(|d| d.mount.len()).cloned()
        };
        let (s, d) = (find(&from), find(&to));
        let route = Route {
            src: s.as_ref().map(|d| d.mount.clone()).unwrap_or_default(),
            dst: d.as_ref().map(|d| d.mount.clone()).unwrap_or_default(),
            src_media: s.as_ref().map(|d| d.media).unwrap_or(fast_core::drives::Media::Unknown),
            dst_media: d.as_ref().map(|d| d.media).unwrap_or(fast_core::drives::Media::Unknown),
            src_name: s.map(|d| d.name).unwrap_or_default(),
            dst_name: d.map(|d| d.name).unwrap_or_default(),
        };
        let (remember, limit) = {
            let s = self.st.settings.read();
            (s.transfer.remember_device_speeds, s.transfer.history_limit)
        };
        if !remember {
            return;
        }
        let mut h = self.st.history.lock();
        if h.record_external(&route, r.items, r.bytes, r.secs(), r.errors.len() as u64, now_ms() / 1000, limit) {
            let _ = h.save(&self.st.paths.history);
        }
    }

    /// Run as administrator: the OS asks the person, then a copy of the app does the work.
    fn run_elevated(&self, task: &Task, cancel: &Arc<AtomicBool>) -> RunResult {
        let fail = |msg: String, denied: bool| RunResult { outcome: Outcome::Failed, items: 0, bytes: 0, errors: vec![msg], denied, interrupted_by: None, attempts: 1 };
        let dir = self.st.paths.scratch.clone();
        if std::fs::create_dir_all(&dir).is_err() {
            return fail("Could not prepare the administrator helper.".into(), false);
        }
        let stem = format!("task-{}-{}", std::process::id(), now_ms());
        let (task_file, result_file, cancel_file) = (dir.join(format!("{stem}.task.json")), dir.join(format!("{stem}.result.json")), dir.join(format!("{stem}.cancel")));
        let progress_file = PathBuf::from(format!("{}.progress", result_file.display()));
        if std::fs::write(&task_file, serde_json::to_vec(task).unwrap_or_default()).is_err() {
            return fail("Could not prepare the administrator helper.".into(), false);
        }
        if let Some(l) = self.running.lock().get_mut(&task.id) {
            l.cancel_file = Some(cancel_file.clone());
        }
        let args: Vec<String> = ["--run-task", &task_file.to_string_lossy(), "--result", &result_file.to_string_lossy(), "--cancel", &cancel_file.to_string_lossy()].iter().map(|s| s.to_string()).collect();
        // Pass on the person's "stop" and show the helper's progress while it works.
        let stop = Arc::new(AtomicBool::new(false));
        let watcher = {
            let (stop, cancel, cancel_file, progress_file, app, id) = (stop.clone(), cancel.clone(), cancel_file.clone(), progress_file.clone(), self.app.clone(), task.id.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if cancel.load(Ordering::Relaxed) {
                        let _ = std::fs::write(&cancel_file, b"1");
                    }
                    if let Some(p) = std::fs::read(&progress_file).ok().and_then(|b| serde_json::from_slice::<Progress>(&b).ok()) {
                        if let Some(hub) = app.try_state::<Arc<Hub>>() {
                            if let Some(l) = hub.running.lock().get_mut(&id) {
                                l.progress = p.clone();
                            }
                        }
                        let _ = app.emit("task-progress", serde_json::json!({ "id": id, "progress": p }));
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            })
        };
        let exe = std::env::current_exe();
        let outcome = exe.map_err(|e| e.to_string()).and_then(|exe| fast_core::platform::run_elevated(&exe, &args).map_err(|e| e.to_string()));
        stop.store(true, Ordering::Relaxed);
        let _ = watcher.join();
        let result = std::fs::read(&result_file).ok().and_then(|b| serde_json::from_slice::<RunResult>(&b).ok());
        for f in [&task_file, &result_file, &cancel_file, &progress_file] {
            let _ = std::fs::remove_file(f);
        }
        match (outcome, result) {
            (Ok(fast_core::platform::Elevation::Declined), _) => fail("Administrator permission was declined, so nothing was done.".into(), true),
            (Ok(_), Some(r)) => r,
            (Ok(_), None) => fail("The administrator helper ended without a result.".into(), false),
            (Err(e), _) => fail(e, false),
        }
    }

    pub fn cancel(&self, id: &str) -> bool {
        let running = self.running.lock();
        match running.get(id) {
            Some(l) => {
                l.cancel.store(true, Ordering::Relaxed);
                if let Some(f) = &l.cancel_file {
                    let _ = std::fs::write(f, b"1");
                }
                true
            }
            None => false,
        }
    }

    /// The app is closing (or the computer is shutting down): stop every run and remember which to resume.
    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::Relaxed) {
            return;
        }
        let ids: Vec<String> = self.running.lock().keys().cloned().collect();
        for id in &ids {
            self.cancel(id);
        }
        // Give the runs a moment to write down how far they got.
        let until = Instant::now() + Duration::from_secs(4);
        while !self.running.lock().is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
        self.last_tick.store(now_ms(), Ordering::Relaxed);
        self.save_state();
    }

    // ---- the scheduler -----------------------------------------------------

    fn tick(self: &Arc<Self>, tick_no: u64) {
        let now = now_ms();
        let last = self.last_tick.swap(now, Ordering::Relaxed);
        if !self.enabled() {
            return;
        }
        if now - last > core::GAP_MS {
            // The computer was off or asleep: tell the runs that were going, so each follows its own policy.
            for l in self.running.lock().values() {
                l.slept.store(true, Ordering::Relaxed);
            }
        }
        let tasks = self.tasks.lock().clone();
        for Due { task_id, kind, .. } in core::due_runs(&tasks, last, now) {
            let Some(task) = tasks.iter().find(|t| t.id == task_id) else { continue };
            match kind {
                DueKind::Scheduled => {
                    let _ = self.start(&task_id, "scheduled", false);
                }
                DueKind::CatchUp => {
                    let _ = self.start(&task_id, "catch-up (it was due while the computer was off or asleep)", false);
                }
                DueKind::Missed => self.skipped(task, "missed", "It was due while the computer was off or asleep, and this task does not catch up."),
            }
        }
        // Folders that changed and then stayed quiet.
        let ready: Vec<String> = {
            let changed = self.changed.lock();
            tasks
                .iter()
                .filter(|t| t.enabled)
                .filter_map(|t| {
                    let quiet = t.triggers.iter().filter_map(|tr| if let Trigger::OnChange { quiet_secs, .. } = tr { Some(*quiet_secs) } else { None }).min()?;
                    let at = changed.get(&t.id)?;
                    (at.elapsed() >= Duration::from_secs(quiet.max(1) as u64)).then(|| t.id.clone())
                })
                .collect()
        };
        for id in ready {
            self.changed.lock().remove(&id);
            let _ = self.start(&id, "a folder changed", false);
        }
        // Drives and devices that appeared.
        if tick_no % 2 == 0 && tasks.iter().any(|t| t.enabled && t.triggers.iter().any(|tr| matches!(tr, Trigger::DriveConnected { .. }))) {
            self.check_drives(&tasks);
        }
        if tick_no % 6 == 0 {
            self.save_state();
        }
    }

    fn check_drives(self: &Arc<Self>, tasks: &[Task]) {
        let sig = fast_core::drives::signature();
        let mut guard = self.drive_sig.lock();
        if guard.0 == sig {
            return;
        }
        let list = fast_core::drives::list_drives();
        let now: HashSet<String> = list.iter().map(|d| format!("{}|{}", d.mount, d.name)).collect();
        let first = guard.0 == 0;
        let appeared: Vec<String> = if first { Vec::new() } else { now.difference(&guard.1).cloned().collect() };
        *guard = (sig, now);
        drop(guard);
        for key in appeared {
            let low = key.to_lowercase();
            for t in tasks.iter().filter(|t| t.enabled) {
                let hit = t.triggers.iter().any(|tr| matches!(tr, Trigger::DriveConnected { name } if name.trim().is_empty() || low.contains(&name.trim().to_lowercase())));
                if hit {
                    // A drive needs a moment before it can be read.
                    let (hub, id) = (self.clone(), t.id.clone());
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_secs(3));
                        let _ = hub.start(&id, "a drive was connected", false);
                    });
                }
            }
        }
    }

    /// At app start: what was cut short by the last shutdown, then everything set to run at startup.
    fn on_start(self: &Arc<Self>, saved: SavedState) {
        let tasks = self.tasks.lock().clone();
        // Runs that were going when the app was killed (power cut, crash) leave a trace.
        for id in &saved.running {
            if let Some(t) = tasks.iter().find(|t| &t.id == id) {
                self.record(RunRecord { task_id: id.clone(), started: saved.last_tick, ended: now_ms(), outcome: Outcome::Interrupted, reason: "unexpected stop".into(), items: 0, bytes: 0, errors: vec![], attempts: 0, interrupted_by: Some("the app was closed without warning".into()), elevated: false, name: t.name.clone() });
                if t.options.on_sleep == SleepPolicy::Resume {
                    if let Some(m) = self.tasks.lock().iter_mut().find(|m| &m.id == id) {
                        m.resume_pending = true;
                    }
                }
            }
        }
        self.save_tasks();
        let tasks = self.tasks.lock().clone();
        for t in tasks.iter().filter(|t| t.enabled) {
            if t.resume_pending {
                let _ = self.start(&t.id, "resumed after the app started again", false);
            } else if t.triggers.iter().any(|tr| matches!(tr, Trigger::Startup)) {
                let _ = self.start(&t.id, "the app started", false);
            }
        }
    }
}

/// Create the scheduler and start its thread (desktop only: phones do not keep apps running in the background).
pub fn start(app: &AppHandle, st: Shared) {
    let hub = Hub::new(app.clone(), st.clone());
    app.manage(hub.clone());
    let saved: SavedState = std::fs::read(&st.paths.task_state).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if saved.last_tick > 0 {
        hub.last_tick.store(saved.last_tick, Ordering::Relaxed);
    }
    // Old records are trimmed once at start.
    hub.trim();
    #[cfg(desktop)]
    {
        let h = hub.clone();
        let _ = std::thread::Builder::new().name("tasks".into()).spawn(move || {
            // The window comes first.
            std::thread::sleep(Duration::from_secs(3));
            if h.enabled() {
                h.on_start(saved);
            }
            let mut n = 0u64;
            while !h.shutting_down.load(Ordering::Relaxed) {
                h.tick(n);
                n += 1;
                std::thread::sleep(TICK);
            }
        });
    }
    #[cfg(not(desktop))]
    let _ = saved;
}

fn hub(app: &AppHandle) -> Result<Arc<Hub>, String> {
    app.try_state::<Arc<Hub>>().map(|h| h.inner().clone()).ok_or_else(|| "Tasks are not available yet.".to_string())
}

/// Called when the app is about to exit.
pub fn shutdown(app: &AppHandle) {
    if let Some(h) = app.try_state::<Arc<Hub>>() {
        h.shutdown();
    }
}

// ---- commands -------------------------------------------------------------

#[tauri::command]
pub fn tasks_list(app: AppHandle) -> Result<Listing, String> {
    Ok(hub(&app)?.listing())
}

fn new_id() -> String {
    format!("t{:x}{:04x}", now_ms(), rand_u16())
}

fn rand_u16() -> u16 {
    let mut b = [0u8; 2];
    let _ = getrandom::fill(&mut b);
    u16::from_le_bytes(b)
}

/// Add or change a task. A task without an id is new.
#[tauri::command]
pub fn tasks_save(app: AppHandle, mut task: Task) -> Result<Task, String> {
    let hub = hub(&app)?;
    if task.name.trim().is_empty() {
        return Err("Give the task a name.".into());
    }
    let now = now_ms();
    task.name = task.name.trim().to_string();
    {
        let mut tasks = hub.tasks.lock();
        if task.id.is_empty() {
            task.id = new_id();
            task.created = now;
        }
        task.updated = now;
        match tasks.iter_mut().find(|t| t.id == task.id) {
            Some(slot) => {
                task.created = slot.created;
                *slot = task.clone();
            }
            None => tasks.push(task.clone()),
        }
    }
    hub.save_tasks();
    hub.emit_changed();
    Ok(task)
}

#[tauri::command]
pub fn tasks_delete(app: AppHandle, id: String) -> Result<(), String> {
    let hub = hub(&app)?;
    hub.cancel(&id);
    hub.tasks.lock().retain(|t| t.id != id);
    hub.save_tasks();
    hub.emit_changed();
    Ok(())
}

#[tauri::command]
pub fn tasks_run(app: AppHandle, id: String, admin: bool) -> Result<(), String> {
    hub(&app)?.start(&id, "manual", admin)
}

#[tauri::command]
pub fn tasks_cancel(app: AppHandle, id: String) -> Result<bool, String> {
    Ok(hub(&app)?.cancel(&id))
}

/// What a task would do right now. Works on a task that has not been saved.
#[tauri::command]
pub async fn tasks_preview(task: Task) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || core::preview(&task, now_ms(), 400)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn tasks_history(app: AppHandle, id: Option<String>, limit: Option<usize>) -> Result<Vec<RunRecord>, String> {
    let hub = hub(&app)?;
    let records = hub.records.lock();
    let mut out: Vec<RunRecord> = records.iter().filter(|r| id.as_ref().is_none_or(|i| &r.task_id == i)).cloned().collect();
    out.reverse();
    out.truncate(limit.unwrap_or(200));
    Ok(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsReply {
    pub stats: Stats,
    /// How long the next run should take, judged from the past ones.
    pub estimate: Option<Estimate>,
}

/// Figures for one task, or for all of them.
#[tauri::command]
pub fn tasks_stats(app: AppHandle, id: Option<String>) -> Result<StatsReply, String> {
    let hub = hub(&app)?;
    let records = hub.records.lock();
    let mine: Vec<RunRecord> = records.iter().filter(|r| id.as_ref().is_none_or(|i| &r.task_id == i)).cloned().collect();
    let bytes = id.as_ref().and_then(|i| {
        let t = hub.tasks.lock().iter().find(|t| &t.id == i).cloned()?;
        let p = core::preview(&t, now_ms(), 0);
        (p.bytes > 0).then_some(p.bytes)
    });
    Ok(StatsReply { stats: core::stats(&mine), estimate: core::estimate(&mine, bytes) })
}

#[tauri::command]
pub fn tasks_clear_history(app: AppHandle, id: Option<String>) -> Result<(), String> {
    let hub = hub(&app)?;
    {
        let mut records = hub.records.lock();
        records.retain(|r| id.as_ref().is_some_and(|i| &r.task_id != i));
        let mut text = String::new();
        for r in records.iter() {
            if let Ok(l) = serde_json::to_string(r) {
                text.push_str(&l);
                text.push('\n');
            }
        }
        std::fs::write(&hub.st.paths.task_history, text).map_err(|e| e.to_string())?;
    }
    hub.emit_changed();
    Ok(())
}

/// Save every task to a file the person chose.
#[tauri::command]
pub fn tasks_export(app: AppHandle, path: String) -> Result<usize, String> {
    let hub = hub(&app)?;
    let tasks = hub.tasks.lock();
    std::fs::write(&path, serde_json::to_string_pretty(&*tasks).map_err(|e| e.to_string())?).map_err(|e| format!("{path}: {e}"))?;
    Ok(tasks.len())
}

/// Add tasks from a file made by `tasks_export` (they get new ids, and start disabled).
#[tauri::command]
pub fn tasks_import(app: AppHandle, path: String) -> Result<usize, String> {
    let hub = hub(&app)?;
    let json = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let list: Vec<Task> = serde_json::from_str(&json).map_err(|e| format!("That is not a task file: {e}"))?;
    let n = list.len();
    {
        let mut tasks = hub.tasks.lock();
        for mut t in list {
            t.id = new_id();
            t.enabled = false;
            t.resume_pending = false;
            t.created = now_ms();
            t.updated = t.created;
            tasks.push(t);
        }
    }
    hub.save_tasks();
    hub.emit_changed();
    Ok(n)
}

// ---- start with the computer ------------------------------------------------

#[cfg(desktop)]
fn autostart_command() -> Result<(PathBuf, Vec<String>), String> {
    Ok((std::env::current_exe().map_err(|e| e.to_string())?, vec!["--minimized".into()]))
}

#[tauri::command]
pub fn autostart_get() -> bool {
    #[cfg(windows)]
    {
        let mut c = std::process::Command::new("reg");
        c.args(["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run", "/v", "FASTFileExplorer"]);
        return fast_core::proc::quiet(&mut c).output().map(|o| o.status.success()).unwrap_or(false);
    }
    #[cfg(target_os = "linux")]
    {
        return dirs_autostart().map(|p| p.exists()).unwrap_or(false);
    }
    #[cfg(target_os = "macos")]
    {
        return mac_plist().map(|p| p.exists()).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(target_os = "linux")]
fn dirs_autostart() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("dev.envid.fastfileexplorer.desktop"))
}

#[cfg(target_os = "macos")]
fn mac_plist() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join("Library/LaunchAgents/dev.envid.fastfileexplorer.plist"))
}

/// Start the app when the person signs in to the computer, minimized, so scheduled tasks run.
#[tauri::command]
pub fn autostart_set(on: bool) -> Result<(), String> {
    #[cfg(desktop)]
    {
        let (exe, args) = autostart_command()?;
        #[cfg(windows)]
        {
            let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
            let mut c = std::process::Command::new("reg");
            if on {
                let value = format!("\"{}\" {}", exe.display(), args.join(" "));
                c.args(["add", key, "/v", "FASTFileExplorer", "/t", "REG_SZ", "/d", &value, "/f"]);
            } else {
                c.args(["delete", key, "/v", "FASTFileExplorer", "/f"]);
            }
            let out = fast_core::proc::quiet(&mut c).output().map_err(|e| e.to_string())?;
            if !out.status.success() && on {
                return Err("Windows would not save the start-up entry.".into());
            }
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let file = dirs_autostart().ok_or("No home folder.")?;
            if on {
                std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
                let text = format!("[Desktop Entry]\nType=Application\nName=FAST File Explorer\nExec=\"{}\" {}\nX-GNOME-Autostart-enabled=true\nTerminal=false\n", exe.display(), args.join(" "));
                std::fs::write(&file, text).map_err(|e| e.to_string())?;
            } else {
                let _ = std::fs::remove_file(&file);
            }
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            let file = mac_plist().ok_or("No home folder.")?;
            if on {
                std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
                let text = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>dev.envid.fastfileexplorer</string><key>ProgramArguments</key><array><string>{}</string><string>--minimized</string></array><key>RunAtLoad</key><true/></dict></plist>\n", exe.display());
                std::fs::write(&file, text).map_err(|e| e.to_string())?;
            } else {
                let _ = std::fs::remove_file(&file);
            }
            return Ok(());
        }
    }
    #[allow(unreachable_code)]
    {
        let _ = on;
        Err("Not available on this device.".into())
    }
}
