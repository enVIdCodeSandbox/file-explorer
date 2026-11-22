//! A record of what the program does, kept for finding out why something stopped.
//!
//! Every action is noted with the time it began, how long it took and how it ended. An action that is still
//! going and has made no progress for a while is flagged as **hung** (and again, less often, while it stays
//! that way); when it ends it says how long it was stuck. The newest events are kept in memory for the
//! Settings > Logs page, and are also written to a file as they happen, so that a program that had to be
//! closed (or that stopped) still leaves the trail.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Events kept in memory.
pub const KEEP: usize = 20_000;
/// The file is moved aside (replacing the last one) when it grows past this.
const FILE_LIMIT: u64 = 5 * 1024 * 1024;
/// A quick action that went well is left out of the record unless "everything" is on.
const QUICK: Duration = Duration::from_millis(150);

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warn,
    Error,
    Hung,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
            Level::Hung => "HUNG ",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: u64,
    /// Milliseconds since 1970 (UTC).
    pub at_ms: u64,
    pub level: Level,
    pub area: String,
    pub what: String,
    pub detail: String,
    pub took_ms: Option<u64>,
}

/// Something going on right now.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Active {
    pub id: u64,
    pub area: String,
    pub what: String,
    pub detail: String,
    pub age_ms: u64,
    /// Time since it last showed any progress.
    pub quiet_ms: u64,
    pub hung: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub events: Vec<Event>,
    pub active: Vec<Active>,
    /// The newest event's number; ask for what came after it next time.
    pub last_seq: u64,
    pub folder: String,
}

struct Running {
    area: String,
    what: String,
    detail: String,
    started: Instant,
    beat: Instant,
    always: bool,
    /// Quiet for this long means hung (0: the general limit).
    limit: Duration,
    /// Quiet time at which the next "still hung" line is due; none while not flagged.
    next_report: Option<Duration>,
    /// Waiting on purpose (queued, paused): never flagged.
    waiting: bool,
}

struct FileSink {
    path: PathBuf,
    file: std::fs::File,
    written: u64,
}

pub struct Log {
    events: VecDeque<Event>,
    next_seq: u64,
    running: HashMap<u64, Running>,
    next_span: u64,
    sink: Option<FileSink>,
    folder: Option<PathBuf>,
    to_file: bool,
    everything: bool,
    hung_after: Duration,
}

impl Log {
    pub fn new() -> Log {
        Log { events: VecDeque::new(), next_seq: 1, running: HashMap::new(), next_span: 1, sink: None, folder: None, to_file: true, everything: false, hung_after: Duration::from_secs(15) }
    }

    fn push(&mut self, level: Level, area: &str, what: &str, detail: &str, took: Option<Duration>) {
        let at_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let event = Event { seq: self.next_seq, at_ms, level, area: area.to_string(), what: what.to_string(), detail: detail.to_string(), took_ms: took.map(|d| d.as_millis() as u64) };
        self.next_seq += 1;
        if self.to_file {
            self.write_line(&event);
        }
        self.events.push_back(event);
        while self.events.len() > KEEP {
            self.events.pop_front();
        }
    }

    fn write_line(&mut self, event: &Event) {
        let Some(folder) = self.folder.clone() else { return };
        if self.sink.is_none() {
            let _ = std::fs::create_dir_all(&folder);
            let path = folder.join("activity.log");
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                let written = file.metadata().map(|m| m.len()).unwrap_or(0);
                self.sink = Some(FileSink { path, file, written });
            }
        }
        let line = format!("{}\n", format_line(event));
        let Some(sink) = self.sink.as_mut() else { return };
        if sink.written + line.len() as u64 > FILE_LIMIT {
            let old = sink.path.with_file_name("activity.old.log");
            let path = sink.path.clone();
            self.sink = None;
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&path, &old);
            if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                self.sink = Some(FileSink { path, file, written: 0 });
            }
        }
        if let Some(sink) = self.sink.as_mut() {
            if sink.file.write_all(line.as_bytes()).is_ok() {
                sink.written += line.len() as u64;
            } else {
                self.sink = None;
            }
        }
    }

    pub fn begin(&mut self, area: &str, what: &str, detail: &str, always: bool, limit: Duration) -> u64 {
        let id = self.next_span;
        self.next_span += 1;
        let now = Instant::now();
        self.running.insert(id, Running { area: area.into(), what: what.into(), detail: detail.into(), started: now, beat: now, always, limit, next_report: None, waiting: false });
        id
    }

    pub fn beat(&mut self, id: u64) {
        if let Some(r) = self.running.get_mut(&id) {
            r.beat = Instant::now();
        }
    }

    pub fn set_detail(&mut self, id: u64, detail: &str) {
        if let Some(r) = self.running.get_mut(&id) {
            if r.detail != detail {
                r.detail = detail.to_string();
            }
        }
    }

    pub fn set_waiting(&mut self, id: u64, waiting: bool) {
        if let Some(r) = self.running.get_mut(&id) {
            r.waiting = waiting;
            if waiting {
                r.beat = Instant::now();
            }
        }
    }

    /// Finish an action. `error` is the reason when it did not go well.
    pub fn end(&mut self, id: u64, error: Option<&str>) {
        let Some(r) = self.running.remove(&id) else { return };
        let took = r.started.elapsed();
        let was_hung = r.next_report.is_some();
        let noteworthy = r.always || self.everything || took >= QUICK || error.is_some() || was_hung;
        if !noteworthy {
            return;
        }
        let (level, what) = match error {
            Some(_) => (Level::Error, format!("{} failed", r.what)),
            None if was_hung => (Level::Warn, format!("{} finished after being stuck", r.what)),
            None => (Level::Info, r.what.clone()),
        };
        let detail = match error {
            Some(e) if r.detail.is_empty() => e.to_string(),
            Some(e) => format!("{} — {}", r.detail, e),
            None => r.detail.clone(),
        };
        self.push(level, &r.area, &what, &detail, Some(took));
    }

    /// Flag what has been quiet too long. Called about once a second; `now` is a parameter so that it can be tested.
    pub fn check_hung(&mut self, now: Instant) {
        let general = self.hung_after;
        let mut lines: Vec<(String, String, String, Duration)> = Vec::new();
        for r in self.running.values_mut() {
            if r.waiting {
                continue;
            }
            let quiet = now.saturating_duration_since(r.beat);
            let limit = if r.limit.is_zero() { general } else { r.limit };
            let due = match r.next_report {
                None => quiet >= limit,
                Some(at) => quiet >= at,
            };
            if !due {
                continue;
            }
            // Again at double the time, and then every minute.
            r.next_report = Some(if quiet < Duration::from_secs(60) { (quiet * 2).max(Duration::from_secs(30)) } else { quiet + Duration::from_secs(60) });
            lines.push((r.area.clone(), format!("{} is not responding", r.what), r.detail.clone(), quiet));
        }
        for (area, what, detail, quiet) in lines {
            let detail = if detail.is_empty() { format!("no progress for {} s", quiet.as_secs()) } else { format!("{detail} — no progress for {} s", quiet.as_secs()) };
            self.push(Level::Hung, &area, &what, &detail, Some(quiet));
        }
    }

    pub fn snapshot(&self, after: u64, now: Instant) -> Snapshot {
        let events: Vec<Event> = self.events.iter().filter(|e| e.seq > after).cloned().collect();
        let mut active: Vec<Active> = self
            .running
            .iter()
            .map(|(id, r)| Active {
                id: *id,
                area: r.area.clone(),
                what: r.what.clone(),
                detail: r.detail.clone(),
                age_ms: now.saturating_duration_since(r.started).as_millis() as u64,
                quiet_ms: now.saturating_duration_since(r.beat).as_millis() as u64,
                hung: r.next_report.is_some() && !r.waiting,
            })
            .collect();
        active.sort_by_key(|a| a.id);
        Snapshot { events, active, last_seq: self.next_seq - 1, folder: self.folder.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default() }
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }

    pub fn export(&self) -> String {
        let mut out = String::new();
        for e in &self.events {
            out.push_str(&format_line(e));
            out.push('\n');
        }
        out
    }
}

impl Default for Log {
    fn default() -> Self {
        Log::new()
    }
}

/// `2026-11-22 10:15:03.250 UTC  HUNG  command open_item — D:\clip.ts — no progress for 15 s (15000 ms)`
pub fn format_line(e: &Event) -> String {
    let secs = (e.at_ms / 1000) as i64;
    let when = chrono::DateTime::from_timestamp(secs, 0).map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
    let took = e.took_ms.map(|t| format!(" ({t} ms)")).unwrap_or_default();
    let detail = if e.detail.is_empty() { String::new() } else { format!(" — {}", e.detail) };
    format!("{when}.{:03} UTC {} {:<8} {}{detail}{took}", e.at_ms % 1000, e.level.word(), e.area, e.what)
}

// ---------------------------------------------------------------------------
// The one record the program keeps
// ---------------------------------------------------------------------------

fn log() -> &'static Mutex<Log> {
    static L: OnceLock<Mutex<Log>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(Log::new()))
}

fn with<T>(f: impl FnOnce(&mut Log) -> T) -> Option<T> {
    log().lock().ok().map(|mut l| f(&mut l))
}

/// Say where the file goes, and begin watching for hung actions.
pub fn init(folder: PathBuf) {
    with(|l| l.folder = Some(folder));
    start_watchdog();
    note(Level::Info, "app", "started", &format!("version {}", env!("CARGO_PKG_VERSION")));
}

/// `to_file`: also write each event to the file. `everything`: keep quick successful actions too.
/// `hung_secs`: how long without progress counts as hung.
pub fn configure(to_file: bool, everything: bool, hung_secs: u64) {
    with(|l| {
        l.to_file = to_file;
        l.everything = everything;
        l.hung_after = Duration::from_secs(hung_secs.clamp(3, 3600));
    });
}

fn start_watchdog() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("activity-watch".into()).spawn(|| loop {
        std::thread::sleep(Duration::from_secs(1));
        with(|l| l.check_hung(Instant::now()));
    });
}

pub fn note(level: Level, area: &str, what: &str, detail: &str) {
    with(|l| l.push(level, area, what, detail, None));
}

pub fn snapshot(after: u64) -> Snapshot {
    with(|l| l.snapshot(after, Instant::now())).unwrap_or_else(|| Snapshot { events: vec![], active: vec![], last_seq: 0, folder: String::new() })
}

pub fn clear() {
    with(|l| l.clear());
}

pub fn export() -> String {
    with(|l| l.export()).unwrap_or_default()
}

thread_local! {
    static CURRENT: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Names the thing an action is working on ("D:\clip.ts"), for the action running on this thread.
pub fn detail(text: &str) {
    let id = CURRENT.with(|c| c.get());
    if id != 0 {
        with(|l| l.set_detail(id, text));
    }
}

/// One action, from `begin` until it is dropped or finished.
pub struct Span {
    id: u64,
    done: bool,
}

/// Starts noting an action. `always`: it is recorded even when it is quick and goes well.
pub fn begin(area: &str, what: &str, detail: &str, always: bool) -> Span {
    let id = with(|l| l.begin(area, what, detail, always, Duration::ZERO)).unwrap_or(0);
    Span { id, done: false }
}

impl Span {
    /// Makes this the action that [`detail`] describes, on the calling thread.
    pub fn enter(&self) {
        CURRENT.with(|c| c.set(self.id));
    }
    /// It is making progress: restart the clock for hung.
    pub fn beat(&self) {
        with(|l| l.beat(self.id));
    }
    pub fn detail(&self, text: &str) {
        with(|l| l.set_detail(self.id, text));
    }
    /// Waiting on purpose (queued, paused): not counted as hung.
    pub fn waiting(&self, yes: bool) {
        with(|l| l.set_waiting(self.id, yes));
    }
    pub fn ok(mut self) {
        self.finish(None);
    }
    pub fn fail(mut self, why: &str) {
        self.finish(Some(why));
    }
    fn finish(&mut self, error: Option<&str>) {
        if !self.done {
            self.done = true;
            with(|l| l.end(self.id, error));
            CURRENT.with(|c| {
                if c.get() == self.id {
                    c.set(0);
                }
            });
        }
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if !self.done {
            self.finish(Some("ended without a result"));
        }
    }
}

/// The name of the command behind a closure's type: `app::commands::open_item::{{closure}}` is `open_item`.
pub fn command_name(type_name: &str) -> String {
    let before = type_name.split("::{{closure}}").next().unwrap_or(type_name);
    before.rsplit("::").next().unwrap_or(before).to_string()
}

/// Actions the person asked for that change something: always recorded.
pub fn is_action(name: &str) -> bool {
    const WORDS: &[&str] = &["create", "rename", "delete", "trash", "copy", "move", "paste", "open", "transfer", "mkdir", "compress", "extract", "upload", "download", "elevated", "restore", "connect", "eject", "format", "write", "save", "share", "stage"];
    WORDS.iter().any(|w| name.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_log() -> Log {
        let mut l = Log::new();
        l.to_file = false;
        l
    }

    #[test]
    fn quick_actions_that_went_well_are_left_out_unless_everything_is_on() {
        let mut l = quiet_log();
        let a = l.begin("command", "list_dir", "", false, Duration::ZERO);
        l.end(a, None);
        assert!(l.events.is_empty());
        let b = l.begin("command", "create_item", "", true, Duration::ZERO);
        l.end(b, None);
        assert_eq!(l.events.len(), 1, "what the person asked for is always kept");
        let c = l.begin("command", "list_dir", "", false, Duration::ZERO);
        l.end(c, Some("access denied"));
        assert_eq!(l.events.back().unwrap().level, Level::Error);
        l.everything = true;
        let d = l.begin("command", "list_dir", "", false, Duration::ZERO);
        l.end(d, None);
        assert_eq!(l.events.len(), 3);
    }

    #[test]
    fn an_action_with_no_progress_is_flagged_hung_then_again_less_often_and_says_how_it_ended() {
        let mut l = quiet_log();
        let id = l.begin("transfer", "copy #4", "reading folder event", false, Duration::ZERO);
        let start = Instant::now();
        l.check_hung(start + Duration::from_secs(5));
        assert!(l.events.is_empty(), "not yet");
        l.check_hung(start + Duration::from_secs(16));
        assert_eq!(l.events.len(), 1);
        assert_eq!(l.events[0].level, Level::Hung);
        assert!(l.events[0].detail.contains("reading folder event"));
        l.check_hung(start + Duration::from_secs(20));
        assert_eq!(l.events.len(), 1, "once, not every second");
        l.check_hung(start + Duration::from_secs(50));
        assert_eq!(l.events.len(), 2);
        assert!(l.snapshot(0, start + Duration::from_secs(50)).active[0].hung);
        l.end(id, None);
        let last = l.events.back().unwrap();
        assert_eq!(last.level, Level::Warn);
        assert!(last.what.contains("finished after being stuck"));
    }

    #[test]
    fn progress_and_waiting_on_purpose_are_not_hung() {
        let mut l = quiet_log();
        let busy = l.begin("transfer", "copy #1", "", false, Duration::ZERO);
        let queued = l.begin("transfer", "copy #2", "", false, Duration::ZERO);
        l.set_waiting(queued, true);
        let later = Instant::now() + Duration::from_secs(20);
        l.beat(busy);
        l.check_hung(Instant::now() + Duration::from_secs(5));
        assert!(l.events.is_empty());
        l.check_hung(later);
        // Only the one that was not waiting is flagged.
        assert_eq!(l.events.len(), 1);
        assert!(l.events[0].what.contains("copy #1"));
    }

    #[test]
    fn only_the_newest_events_are_kept_and_asking_after_a_number_gives_what_is_newer() {
        let mut l = quiet_log();
        for i in 0..(KEEP + 10) {
            l.push(Level::Info, "x", "e", &i.to_string(), None);
        }
        assert_eq!(l.events.len(), KEEP);
        let snap = l.snapshot((KEEP + 5) as u64, Instant::now());
        assert_eq!(snap.events.len(), 5);
        assert_eq!(snap.last_seq, (KEEP + 10) as u64);
    }

    #[test]
    fn the_file_is_written_as_things_happen_and_rolled_over_when_big() {
        let dir = tempfile::tempdir().unwrap();
        let mut l = Log::new();
        l.folder = Some(dir.path().to_path_buf());
        l.push(Level::Warn, "read", "stalled", "E:\\clip.ts", None);
        let text = std::fs::read_to_string(dir.path().join("activity.log")).unwrap();
        assert!(text.contains("WARN") && text.contains("stalled") && text.contains("E:\\clip.ts"));
        // A file already over the limit is moved aside.
        let big = "x".repeat(FILE_LIMIT as usize + 10);
        l.sink = None;
        std::fs::write(dir.path().join("activity.log"), big).unwrap();
        l.push(Level::Info, "app", "next", "", None);
        assert!(dir.path().join("activity.old.log").exists());
        let fresh = std::fs::read_to_string(dir.path().join("activity.log")).unwrap();
        assert!(fresh.contains("next") && fresh.len() < 1000);
    }

    #[test]
    fn names_come_from_the_closure_type_and_actions_are_told_from_lookups() {
        assert_eq!(command_name("app_lib::commands::open_item::{{closure}}::{{closure}}"), "open_item");
        assert_eq!(command_name("plain"), "plain");
        assert!(is_action("create_item") && is_action("open_item") && is_action("delete_start"));
        assert!(!is_action("list_dir") && !is_action("get_details"));
    }

    #[test]
    fn a_line_reads_well() {
        let e = Event { seq: 1, at_ms: 1_700_000_000_250, level: Level::Hung, area: "command".into(), what: "open_item is not responding".into(), detail: "D:\\a.ts".into(), took_ms: Some(15000) };
        let line = format_line(&e);
        assert!(line.starts_with("2023-11-14 22:13:20.250 UTC HUNG "), "{line}");
        assert!(line.contains("open_item is not responding — D:\\a.ts (15000 ms)"));
    }
}
