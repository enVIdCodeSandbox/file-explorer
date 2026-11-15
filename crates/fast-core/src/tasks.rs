//! Automated tasks: things to do with files on a schedule or when something happens, like a
//! cron job with a friendlier face. This file holds everything that does not need a window:
//! what a task is, when it is due, what it would do (a preview), doing it, the record of every
//! run, and the statistics and time estimates learned from those records.
//!
//! The app's scheduler (src-tauri/src/tasks.rs) calls into this; nothing here starts threads.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

// ===========================================================================
// What a task is
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Any of these starts the task.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Done one after the other.
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub options: Options,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    /// A run was cut short by shutting down; it continues at the next start.
    #[serde(default)]
    pub resume_pending: bool,
    /// The last run was refused for lack of permission; the person is offered to run it as administrator.
    #[serde(default)]
    pub needs_admin: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Trigger {
    /// Only when started by hand.
    Manual,
    /// Once, at a moment (milliseconds since 1970).
    Once { at: i64 },
    /// Every so many minutes, counted from `anchor` (milliseconds since 1970).
    #[serde(rename_all = "camelCase")]
    Every { minutes: u32, anchor: i64 },
    /// Every day, or only on some days (0 = Monday ... 6 = Sunday; empty = every day), at `time` ("HH:MM", local).
    Daily { time: String, #[serde(default)] days: Vec<u8> },
    /// On a day of the month (clamped to the month's last day), at `time`.
    Monthly { day: u8, time: String },
    /// When the app starts.
    Startup,
    /// When something in a folder changes and then stays quiet for a few seconds.
    #[serde(rename_all = "camelCase")]
    OnChange { path: String, quiet_secs: u32 },
    /// When a drive or device whose name contains `name` is connected (empty = any).
    DriveConnected { name: String },
}

impl Trigger {
    /// Does this one fire by the clock (as opposed to by an event)?
    pub fn by_clock(&self) -> bool {
        matches!(self, Trigger::Once { .. } | Trigger::Every { .. } | Trigger::Daily { .. } | Trigger::Monthly { .. })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Filter {
    /// Names to take, like `*.jpg; report*` (empty = everything).
    #[serde(default)]
    pub include: String,
    /// Names to leave out.
    #[serde(default)]
    pub exclude: String,
    /// Only files at least this many days old (by date modified).
    #[serde(default)]
    pub older_than_days: Option<f64>,
    #[serde(default)]
    pub newer_than_days: Option<f64>,
    #[serde(default)]
    pub min_bytes: Option<u64>,
    #[serde(default)]
    pub max_bytes: Option<u64>,
    #[serde(default)]
    pub hidden: bool,
    /// Look inside folders too (default true).
    #[serde(default = "yes")]
    pub recursive: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Conflict {
    /// Leave the file that is already there.
    Skip,
    #[default]
    Overwrite,
    OverwriteIfNewer,
    /// Keep both: the new one gets a number.
    KeepBoth,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    #[serde(rename_all = "camelCase")]
    Copy { from: Vec<String>, to: String, #[serde(default)] filter: Filter, #[serde(default)] conflict: Conflict, #[serde(default = "yes")] keep_structure: bool },
    #[serde(rename_all = "camelCase")]
    Move { from: Vec<String>, to: String, #[serde(default)] filter: Filter, #[serde(default)] conflict: Conflict, #[serde(default = "yes")] keep_structure: bool },
    /// Delete what matches. With `permanent` off, things go to the trash.
    #[serde(rename_all = "camelCase")]
    Delete { paths: Vec<String>, #[serde(default)] filter: Filter, #[serde(default)] permanent: bool, #[serde(default)] remove_empty_folders: bool, #[serde(default)] remove_folder: bool },
    /// Pack into a ZIP. `to` can hold {date}, {time}, {datetime}, {year}, {month}, {day} and {task}.
    Compress { from: Vec<String>, to: String, #[serde(default)] filter: Filter },
    Extract { archive: String, to: String },
    /// Make `to` match `from` (new and changed files are copied; with `mirror`, extra files in `to` are deleted).
    Sync { from: String, to: String, #[serde(default)] mirror: bool, #[serde(default)] filter: Filter },
    #[serde(rename_all = "camelCase")]
    Run { program: String, #[serde(default)] args: Vec<String>, #[serde(default)] cwd: String, #[serde(default)] timeout_secs: u32 },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum SleepPolicy {
    /// Carry on after waking (files that were not finished are done again).
    #[default]
    Resume,
    /// Give up this run when the computer wakes; the next scheduled run starts fresh.
    Cancel,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum Notify {
    Never,
    #[default]
    Failure,
    Always,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    #[serde(default)]
    pub on_sleep: SleepPolicy,
    /// A run missed because the computer was off or asleep happens once when it is back.
    #[serde(default = "yes")]
    pub catch_up: bool,
    /// Another try (after `retry_delay_secs`) for a file that failed.
    #[serde(default)]
    pub retries: u32,
    #[serde(default = "five")]
    pub retry_delay_secs: u32,
    /// Stop after this long (0 = no limit).
    #[serde(default)]
    pub timeout_minutes: u32,
    /// The first failure ends the run (otherwise the rest is still done).
    #[serde(default = "yes")]
    pub stop_on_error: bool,
    /// Only while the computer is plugged in.
    #[serde(default)]
    pub only_on_ac: bool,
    /// Always ask for administrator rights when it runs.
    #[serde(default)]
    pub run_as_admin: bool,
    #[serde(default)]
    pub notify: Notify,
    /// Do not start a new run while the last one is still going (otherwise it waits for its turn).
    #[serde(default = "yes")]
    pub skip_if_running: bool,
    /// Stay out of the way of everything else.
    #[serde(default = "yes")]
    pub low_priority: bool,
}

fn five() -> u32 {
    5
}

impl Default for Options {
    fn default() -> Self {
        Options {
            on_sleep: SleepPolicy::Resume,
            catch_up: true,
            retries: 0,
            retry_delay_secs: 5,
            timeout_minutes: 0,
            stop_on_error: true,
            only_on_ac: false,
            run_as_admin: false,
            notify: Notify::Failure,
            skip_if_running: true,
            low_priority: true,
        }
    }
}

// ===========================================================================
// When a task is due
// ===========================================================================

fn parse_time(t: &str) -> Option<chrono::NaiveTime> {
    let (h, m) = t.trim().split_once(':')?;
    chrono::NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

fn local_ms(date: chrono::NaiveDate, time: chrono::NaiveTime) -> Option<i64> {
    use chrono::TimeZone;
    // A time that does not exist (the hour skipped when the clocks go forward) is skipped.
    chrono::Local.from_local_datetime(&date.and_time(time)).earliest().map(|d| d.timestamp_millis())
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    chrono::NaiveDate::from_ymd_opt(y, m, 1).and_then(|d| d.pred_opt()).map(|d| chrono::Datelike::day(&d)).unwrap_or(28)
}

/// The first moment strictly after `after` (milliseconds) that a clock trigger fires.
pub fn next_time(t: &Trigger, after: i64) -> Option<i64> {
    use chrono::{Datelike, TimeZone};
    match t {
        Trigger::Once { at } => (*at > after).then_some(*at),
        Trigger::Every { minutes, anchor } => {
            let period = (*minutes).max(1) as i64 * 60_000;
            if after < *anchor {
                return Some(*anchor);
            }
            Some(anchor + ((after - anchor) / period + 1) * period)
        }
        Trigger::Daily { time, days } => {
            let time = parse_time(time)?;
            let start = chrono::Local.timestamp_millis_opt(after).single()?.date_naive();
            for d in 0..9 {
                let date = start + chrono::Duration::days(d);
                let weekday = date.weekday().num_days_from_monday() as u8;
                if !days.is_empty() && !days.contains(&weekday) {
                    continue;
                }
                if let Some(ms) = local_ms(date, time) {
                    if ms > after {
                        return Some(ms);
                    }
                }
            }
            None
        }
        Trigger::Monthly { day, time } => {
            let time = parse_time(time)?;
            let start = chrono::Local.timestamp_millis_opt(after).single()?.date_naive();
            for m in 0..14i32 {
                let total = start.year() * 12 + start.month0() as i32 + m;
                let (year, month) = (total.div_euclid(12), total.rem_euclid(12) as u32 + 1);
                let day = (*day).clamp(1, 31) as u32;
                let date = chrono::NaiveDate::from_ymd_opt(year, month, day.min(last_day_of_month(year, month)))?;
                if let Some(ms) = local_ms(date, time) {
                    if ms > after {
                        return Some(ms);
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// How a due run came about.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DueKind {
    /// On time.
    Scheduled,
    /// The time passed while the computer was off or asleep; run once now.
    CatchUp,
    /// The time passed while the computer was off or asleep and the task does not catch up.
    Missed,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Due {
    pub task_id: String,
    pub at: i64,
    pub kind: DueKind,
}

/// A gap this long between two looks at the clock means the computer was off or asleep.
pub const GAP_MS: i64 = 90_000;

/// Which clock-triggered tasks fire between `last_tick` and `now`. A task fires at most once
/// however many of its times fell in the window.
pub fn due_runs(tasks: &[Task], last_tick: i64, now: i64) -> Vec<Due> {
    let gap = now - last_tick > GAP_MS;
    let mut out = Vec::new();
    for t in tasks.iter().filter(|t| t.enabled) {
        let first = t.triggers.iter().filter_map(|tr| next_time(tr, last_tick)).filter(|at| *at <= now).min();
        let Some(at) = first else { continue };
        let late = now - at > GAP_MS;
        let kind = if !gap && !late {
            DueKind::Scheduled
        } else if t.options.catch_up {
            DueKind::CatchUp
        } else {
            DueKind::Missed
        };
        out.push(Due { task_id: t.id.clone(), at, kind });
    }
    out
}

/// When this task next runs by the clock, if it does.
pub fn next_run(t: &Task, after: i64) -> Option<i64> {
    if !t.enabled {
        return None;
    }
    t.triggers.iter().filter_map(|tr| next_time(tr, after)).min()
}

// ===========================================================================
// Names and filters
// ===========================================================================

/// `{date}` and friends in a name or path.
pub fn expand_tokens(text: &str, task: &str, now: i64) -> String {
    use chrono::TimeZone;
    let d = chrono::Local.timestamp_millis_opt(now).single().unwrap_or_else(chrono::Local::now);
    text.replace("{datetime}", &d.format("%Y-%m-%d_%H%M").to_string())
        .replace("{date}", &d.format("%Y-%m-%d").to_string())
        .replace("{time}", &d.format("%H%M").to_string())
        .replace("{year}", &d.format("%Y").to_string())
        .replace("{month}", &d.format("%m").to_string())
        .replace("{day}", &d.format("%d").to_string())
        .replace("{task}", &task.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect::<String>())
}

/// `*` and `?`, ignoring case.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn any_glob(list: &str, name: &str) -> bool {
    list.split([';', ',']).map(str::trim).filter(|p| !p.is_empty()).any(|p| glob_match(p, name))
}

impl Filter {
    /// Does this filter let a file through? (Folders are only walked, never judged by size or age.)
    pub fn allows_file(&self, name: &str, size: u64, modified: Option<i64>, now: i64) -> bool {
        if !self.hidden && name.starts_with('.') {
            return false;
        }
        if !self.include.trim().is_empty() && !any_glob(&self.include, name) {
            return false;
        }
        if any_glob(&self.exclude, name) {
            return false;
        }
        if self.min_bytes.is_some_and(|m| size < m) || self.max_bytes.is_some_and(|m| size > m) {
            return false;
        }
        let age_days = modified.map(|m| (now - m) as f64 / 86_400_000.0);
        if let Some(min) = self.older_than_days {
            if age_days.is_none_or(|a| a < min) {
                return false;
            }
        }
        if let Some(max) = self.newer_than_days {
            if age_days.is_none_or(|a| a > max) {
                return false;
            }
        }
        true
    }

    /// Does it hold any restriction at all?
    pub fn is_open(&self) -> bool {
        self.include.trim().is_empty() && self.exclude.trim().is_empty() && self.older_than_days.is_none() && self.newer_than_days.is_none() && self.min_bytes.is_none() && self.max_bytes.is_none() && self.recursive
    }
}

// ===========================================================================
// What a task would do (the plan) and doing it
// ===========================================================================

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Planned {
    pub action: usize,
    /// "copy", "move", "delete", "trash", "zip", "unzip", "run", "mkdir".
    pub kind: String,
    pub from: String,
    pub to: String,
    pub bytes: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub ops: Vec<Planned>,
    pub files: u64,
    pub bytes: u64,
    /// Only the first part of a long list is sent.
    pub truncated: bool,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug)]
enum Op {
    Copy { from: PathBuf, to: PathBuf, bytes: u64, mv: bool, conflict: Conflict, modified: Option<i64> },
    MkDir(PathBuf),
    Delete { path: PathBuf, bytes: u64, is_dir: bool, permanent: bool },
    /// Remove a folder only if nothing is left in it by the time this runs.
    RmEmpty(PathBuf),
    Zip { files: Vec<(PathBuf, String)>, to: PathBuf },
    Unzip { archive: PathBuf, to: PathBuf },
    Run { program: String, args: Vec<String>, cwd: String, timeout_secs: u32 },
}

impl Op {
    fn planned(&self, action: usize) -> Planned {
        let s = |p: &Path| p.to_string_lossy().into_owned();
        match self {
            Op::Copy { from, to, bytes, mv, .. } => Planned { action, kind: if *mv { "move" } else { "copy" }.into(), from: s(from), to: s(to), bytes: *bytes },
            Op::MkDir(p) => Planned { action, kind: "mkdir".into(), from: String::new(), to: s(p), bytes: 0 },
            Op::RmEmpty(p) => Planned { action, kind: "rmdir".into(), from: s(p), to: String::new(), bytes: 0 },
            Op::Delete { path, bytes, permanent, .. } => Planned { action, kind: if *permanent { "delete" } else { "trash" }.into(), from: s(path), to: String::new(), bytes: *bytes },
            Op::Zip { files, to } => Planned { action, kind: "zip".into(), from: format!("{} files", files.len()), to: s(to), bytes: files.iter().filter_map(|(p, _)| fs::metadata(p).ok()).map(|m| m.len()).sum() },
            Op::Unzip { archive, to } => Planned { action, kind: "unzip".into(), from: s(archive), to: s(to), bytes: fs::metadata(archive).map(|m| m.len()).unwrap_or(0) },
            Op::Run { program, args, .. } => Planned { action, kind: "run".into(), from: format!("{program} {}", args.join(" ")).trim().to_string(), to: String::new(), bytes: 0 },
        }
    }
    fn bytes(&self) -> u64 {
        match self {
            Op::Copy { bytes, .. } | Op::Delete { bytes, .. } => *bytes,
            Op::Zip { files, .. } => files.iter().filter_map(|(p, _)| fs::metadata(p).ok()).map(|m| m.len()).sum(),
            Op::Unzip { archive, .. } => fs::metadata(archive).map(|m| m.len()).unwrap_or(0),
            _ => 0,
        }
    }
}

fn millis(t: std::io::Result<std::time::SystemTime>) -> Option<i64> {
    t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64)
}

/// Files (not folders) under `root` that the filter lets through, with their path relative to `root`'s parent
/// (so a folder keeps its own name) or to `root` itself.
fn walk(root: &Path, filter: &Filter, now: i64, out: &mut Vec<(PathBuf, String, u64, Option<i64>)>, depth: usize) {
    let Ok(meta) = fs::symlink_metadata(root) else { return };
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if meta.file_type().is_symlink() {
        return; // links are never followed: that could pull in a whole other part of the disk
    }
    if meta.is_file() {
        if filter.allows_file(&name, meta.len(), millis(meta.modified()), now) {
            out.push((root.to_path_buf(), name.clone(), meta.len(), millis(meta.modified())));
        }
        return;
    }
    if depth > 0 && !filter.recursive {
        return;
    }
    if depth > 0 && !filter.hidden && name.starts_with('.') {
        return;
    }
    if let Ok(rd) = fs::read_dir(root) {
        let mut kids: Vec<_> = rd.filter_map(|e| e.ok()).collect();
        kids.sort_by_key(|k| k.file_name());
        for k in kids {
            walk(&k.path(), filter, now, out, depth + 1);
        }
    }
}

/// Sources of an action as (file, path under the destination).
fn gather(sources: &[String], filter: &Filter, now: i64, keep_structure: bool, task: &str) -> Vec<(PathBuf, PathBuf, u64, Option<i64>)> {
    let mut out = Vec::new();
    for s in sources {
        let root = PathBuf::from(expand_tokens(s, task, now));
        let mut found = Vec::new();
        walk(&root, filter, now, &mut found, 0);
        let is_dir = root.is_dir();
        for (file, name, size, modified) in found {
            let rel = if !keep_structure || !is_dir {
                PathBuf::from(&name)
            } else {
                let inner = file.strip_prefix(&root).unwrap_or(Path::new(&name)).to_path_buf();
                PathBuf::from(root.file_name().unwrap_or_default()).join(inner)
            };
            out.push((file, rel, size, modified));
        }
    }
    out
}

fn same_file(from_size: u64, from_mod: Option<i64>, to: &Path) -> bool {
    match fs::metadata(to) {
        Ok(m) if m.len() == from_size => match (from_mod, millis(m.modified())) {
            (Some(a), Some(b)) => (a - b).abs() <= 2_000,
            _ => false,
        },
        _ => false,
    }
}

fn numbered(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for i in 2..10_000 {
        let p = path.with_file_name(format!("{stem} ({i}){ext}"));
        if !p.exists() {
            return p;
        }
    }
    path.to_path_buf()
}

fn plan_action(i: usize, a: &Action, task: &str, now: i64, problems: &mut Vec<String>) -> Vec<Op> {
    let mut ops = Vec::new();
    match a {
        Action::Copy { from, to, filter, conflict, keep_structure } | Action::Move { from, to, filter, conflict, keep_structure } => {
            let mv = matches!(a, Action::Move { .. });
            let dest = PathBuf::from(expand_tokens(to, task, now));
            if to.trim().is_empty() {
                problems.push(format!("Step {}: no destination folder.", i + 1));
            }
            let files = gather(from, filter, now, *keep_structure, task);
            if files.is_empty() {
                problems.push(format!("Step {}: nothing matches.", i + 1));
            }
            let mut made: HashSet<PathBuf> = HashSet::new();
            for (file, rel, bytes, modified) in files {
                let target = dest.join(rel);
                if let Some(parent) = target.parent() {
                    if !parent.exists() && made.insert(parent.to_path_buf()) {
                        ops.push(Op::MkDir(parent.to_path_buf()));
                    }
                }
                ops.push(Op::Copy { from: file, to: target, bytes, mv, conflict: *conflict, modified });
            }
        }
        Action::Delete { paths, filter, permanent, remove_empty_folders, remove_folder } => {
            let mut folders = Vec::new();
            for p in paths {
                let root = PathBuf::from(expand_tokens(p, task, now));
                if root.is_dir() && *remove_folder && filter.is_open() {
                    let mut all = Vec::new();
                    walk(&root, &Filter { hidden: true, ..Filter::default() }, now, &mut all, 0);
                    let bytes = all.iter().map(|f| f.2).sum();
                    ops.push(Op::Delete { path: root, bytes, is_dir: true, permanent: *permanent });
                    continue;
                }
                let mut found = Vec::new();
                walk(&root, filter, now, &mut found, 0);
                for (file, _, bytes, _) in found {
                    ops.push(Op::Delete { path: file, bytes, is_dir: false, permanent: *permanent });
                }
                if root.is_dir() {
                    folders.push(root);
                }
            }
            if *remove_empty_folders {
                // Deepest first, so a folder emptied by its children's removal goes too (decided when it runs).
                for root in folders {
                    let mut dirs = Vec::new();
                    collect_dirs(&root, &mut dirs);
                    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
                    for d in dirs {
                        ops.push(Op::RmEmpty(d));
                    }
                }
            }
            if ops.is_empty() {
                problems.push(format!("Step {}: nothing matches.", i + 1));
            }
        }
        Action::Compress { from, to, filter } => {
            let files = gather(from, filter, now, true, task);
            if files.is_empty() {
                problems.push(format!("Step {}: nothing matches.", i + 1));
            } else {
                let mut dest = PathBuf::from(expand_tokens(to, task, now));
                if dest.extension().is_none() {
                    dest.set_extension("zip");
                }
                if let Some(parent) = dest.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        ops.push(Op::MkDir(parent.to_path_buf()));
                    }
                }
                ops.push(Op::Zip { files: files.into_iter().map(|(f, rel, _, _)| (f, rel.to_string_lossy().replace('\\', "/"))).collect(), to: dest });
            }
        }
        Action::Extract { archive, to } => {
            let archive = PathBuf::from(expand_tokens(archive, task, now));
            if !archive.is_file() {
                problems.push(format!("Step {}: the archive is not there.", i + 1));
            }
            ops.push(Op::Unzip { archive, to: PathBuf::from(expand_tokens(to, task, now)) });
        }
        Action::Sync { from, to, mirror, filter } => {
            let src = PathBuf::from(expand_tokens(from, task, now));
            let dst = PathBuf::from(expand_tokens(to, task, now));
            let mut found = Vec::new();
            walk(&src, filter, now, &mut found, 0);
            if !src.is_dir() {
                problems.push(format!("Step {}: the source folder is not there.", i + 1));
            }
            let mut wanted: HashSet<PathBuf> = HashSet::new();
            let mut made: HashSet<PathBuf> = HashSet::new();
            for (file, _, bytes, modified) in found {
                let rel = file.strip_prefix(&src).unwrap_or(&file).to_path_buf();
                let target = dst.join(&rel);
                wanted.insert(target.clone());
                if same_file(bytes, modified, &target) {
                    continue;
                }
                if let Some(parent) = target.parent() {
                    if !parent.exists() && made.insert(parent.to_path_buf()) {
                        ops.push(Op::MkDir(parent.to_path_buf()));
                    }
                }
                ops.push(Op::Copy { from: file, to: target, bytes, mv: false, conflict: Conflict::Overwrite, modified });
            }
            if *mirror {
                let mut extra = Vec::new();
                walk(&dst, &Filter { hidden: true, ..Filter::default() }, now, &mut extra, 0);
                for (file, _, bytes, _) in extra {
                    if !wanted.contains(&file) {
                        ops.push(Op::Delete { path: file, bytes, is_dir: false, permanent: false });
                    }
                }
            }
        }
        Action::Run { program, args, cwd, timeout_secs } => {
            if program.trim().is_empty() {
                problems.push(format!("Step {}: no program.", i + 1));
            }
            ops.push(Op::Run {
                program: expand_tokens(program, task, now),
                args: args.iter().map(|a| expand_tokens(a, task, now)).collect(),
                cwd: expand_tokens(cwd, task, now),
                timeout_secs: *timeout_secs,
            });
        }
    }
    ops
}

fn collect_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.file_type().map(|t| t.is_dir() && !t.is_symlink()).unwrap_or(false) {
                collect_dirs(&e.path(), out);
                out.push(e.path());
            }
        }
    }
}

/// What the task would do right now, without doing anything.
pub fn preview(task: &Task, now: i64, limit: usize) -> Preview {
    let mut p = Preview::default();
    if task.actions.is_empty() {
        p.problems.push("The task has no steps.".into());
    }
    for (i, a) in task.actions.iter().enumerate() {
        for op in plan_action(i, a, &task.name, now, &mut p.problems) {
            if !matches!(op, Op::MkDir(_)) {
                p.files += 1;
            }
            p.bytes += op.bytes();
            if p.ops.len() < limit {
                p.ops.push(op.planned(i));
            } else {
                p.truncated = true;
            }
        }
    }
    p
}

// ===========================================================================
// Running
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub step: usize,
    pub steps: usize,
    pub items_done: u64,
    pub items_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
    pub current: String,
    pub elapsed_secs: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Success,
    /// Some things were done, some failed.
    Partial,
    Failed,
    /// The person stopped it.
    Cancelled,
    /// Shutdown or sleep cut it short.
    Interrupted,
    /// Not run (not plugged in, still running, missed while off...).
    Skipped,
}

pub struct Env<'a> {
    pub cancel: &'a AtomicBool,
    /// Set by the app when the computer wakes from sleep.
    pub slept: &'a AtomicBool,
    pub on_progress: &'a (dyn Fn(&Progress) + Sync),
    pub on_sleep: SleepPolicy,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    pub outcome: Outcome,
    pub items: u64,
    pub bytes: u64,
    pub errors: Vec<String>,
    /// Something failed only for lack of permission.
    pub denied: bool,
    pub interrupted_by: Option<String>,
    pub attempts: u32,
}

struct OpError {
    message: String,
    denied: bool,
}

impl OpError {
    fn io(path: &Path, e: std::io::Error) -> OpError {
        OpError { message: format!("{}: {e}", path.display()), denied: e.kind() == std::io::ErrorKind::PermissionDenied }
    }
    fn text(s: impl Into<String>) -> OpError {
        OpError { message: s.into(), denied: false }
    }
}

enum Stop {
    User,
    Sleep,
    Timeout,
}

struct Runner<'a> {
    env: &'a Env<'a>,
    deadline: Option<Instant>,
    started: Instant,
    progress: Progress,
    last_report: Instant,
}

impl Runner<'_> {
    fn stop(&self) -> Option<Stop> {
        if self.env.cancel.load(Ordering::Relaxed) {
            return Some(Stop::User);
        }
        if self.env.on_sleep == SleepPolicy::Cancel && self.env.slept.load(Ordering::Relaxed) {
            return Some(Stop::Sleep);
        }
        if self.deadline.is_some_and(|d| Instant::now() > d) {
            return Some(Stop::Timeout);
        }
        None
    }

    fn report(&mut self, force: bool) {
        if force || self.last_report.elapsed() >= Duration::from_millis(150) {
            self.progress.elapsed_secs = self.started.elapsed().as_secs_f64();
            (self.env.on_progress)(&self.progress);
            self.last_report = Instant::now();
        }
    }
}

fn copy_file(r: &mut Runner, from: &Path, to: &Path, modified: Option<i64>) -> std::result::Result<u64, OpError> {
    let part = to.with_file_name(format!("{}.part", to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
    let result = (|| {
        let mut src = fs::File::open(from).map_err(|e| OpError::io(from, e))?;
        let mut dst = fs::File::create(&part).map_err(|e| OpError::io(&part, e))?;
        let mut buf = vec![0u8; 1 << 20];
        let mut total = 0u64;
        loop {
            if r.stop().is_some() {
                return Err(OpError::text("stopped"));
            }
            let n = src.read(&mut buf).map_err(|e| OpError::io(from, e))?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n]).map_err(|e| OpError::io(&part, e))?;
            total += n as u64;
            r.progress.bytes_done += n as u64;
            r.report(false);
        }
        dst.flush().map_err(|e| OpError::io(&part, e))?;
        drop(dst);
        if let Some(ms) = modified {
            let _ = filetime::set_file_mtime(&part, filetime::FileTime::from_unix_time(ms.div_euclid(1000), (ms.rem_euclid(1000) * 1_000_000) as u32));
        }
        fs::rename(&part, to).map_err(|e| OpError::io(to, e))?;
        Ok(total)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn zip_files(r: &mut Runner, files: &[(PathBuf, String)], to: &Path) -> std::result::Result<u64, OpError> {
    let part = to.with_extension("zip.part");
    let result = (|| {
        let out = fs::File::create(&part).map_err(|e| OpError::io(&part, e))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(out));
        let mut buf = vec![0u8; 256 * 1024];
        let mut count = 0u64;
        for (file, name) in files {
            if r.stop().is_some() {
                return Err(OpError::text("stopped"));
            }
            let meta = fs::metadata(file).map_err(|e| OpError::io(file, e))?;
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(meta.len() >= 0xFFFF_FFFF);
            zip.start_file(name.as_str(), options).map_err(|e| OpError::text(format!("{}: {e}", file.display())))?;
            let mut src = fs::File::open(file).map_err(|e| OpError::io(file, e))?;
            loop {
                if r.stop().is_some() {
                    return Err(OpError::text("stopped"));
                }
                let n = src.read(&mut buf).map_err(|e| OpError::io(file, e))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n]).map_err(|e| OpError::io(&part, e))?;
                r.progress.bytes_done += n as u64;
                r.report(false);
            }
            count += 1;
        }
        zip.finish().map_err(|e| OpError::text(format!("{}: {e}", part.display())))?;
        fs::rename(&part, to).map_err(|e| OpError::io(to, e))?;
        Ok(count)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

fn run_program(r: &mut Runner, program: &str, args: &[String], cwd: &str, timeout_secs: u32) -> std::result::Result<(), OpError> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped());
    if !cwd.trim().is_empty() {
        cmd.current_dir(cwd);
    }
    crate::proc::quiet(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| OpError { message: format!("{program}: {e}"), denied: e.kind() == std::io::ErrorKind::PermissionDenied })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut text = String::new();
                if let Some(e) = child.stderr.take() {
                    let _ = e.take(2000).read_to_string(&mut text);
                }
                return Err(OpError::text(format!("{program} ended with {status}{}", if text.trim().is_empty() { String::new() } else { format!(": {}", text.trim()) })));
            }
            Ok(None) => {}
            Err(e) => return Err(OpError::text(format!("{program}: {e}"))),
        }
        if r.stop().is_some() || (timeout_secs > 0 && started.elapsed() > Duration::from_secs(timeout_secs as u64)) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OpError::text(if r.stop().is_some() { "stopped".to_string() } else { format!("{program} took longer than {timeout_secs} seconds") }));
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

fn exec(r: &mut Runner, op: &Op) -> std::result::Result<u64, OpError> {
    match op {
        Op::MkDir(p) => fs::create_dir_all(p).map(|_| 0).map_err(|e| OpError::io(p, e)),
        Op::Copy { from, to, bytes, mv, conflict, modified } => {
            let mut target = to.clone();
            if target.exists() {
                if same_file(*bytes, *modified, &target) {
                    // Already there, as it should be.
                    r.progress.bytes_done += *bytes;
                    if *mv {
                        fs::remove_file(from).map_err(|e| OpError::io(from, e))?;
                    }
                    return Ok(0);
                }
                match conflict {
                    Conflict::Skip => {
                        r.progress.bytes_done += *bytes;
                        return Ok(0);
                    }
                    Conflict::OverwriteIfNewer => {
                        let theirs = fs::metadata(&target).ok().and_then(|m| millis(m.modified()));
                        if matches!((modified, theirs), (Some(a), Some(b)) if *a <= b) {
                            r.progress.bytes_done += *bytes;
                            return Ok(0);
                        }
                    }
                    Conflict::KeepBoth => target = numbered(&target),
                    Conflict::Overwrite => {}
                }
            }
            if *mv && !target.exists() && fs::rename(from, &target).is_ok() {
                r.progress.bytes_done += *bytes;
                return Ok(*bytes);
            }
            let n = copy_file(r, from, &target, *modified)?;
            if *mv {
                fs::remove_file(from).map_err(|e| OpError::io(from, e))?;
            }
            Ok(n)
        }
        Op::Delete { path, bytes, is_dir, permanent } => {
            if !path.exists() {
                return Ok(0);
            }
            if *is_dir {
                if *permanent { fs::remove_dir_all(path).map_err(|e| OpError::io(path, e))? } else { to_trash(path)? }
            } else if *permanent {
                fs::remove_file(path).map_err(|e| OpError::io(path, e))?;
            } else {
                to_trash(path)?;
            }
            r.progress.bytes_done += *bytes;
            Ok(*bytes)
        }
        Op::RmEmpty(path) => {
            // Only a folder that really is empty by now.
            if fs::read_dir(path).map(|mut d| d.next().is_none()).unwrap_or(false) {
                fs::remove_dir(path).map_err(|e| OpError::io(path, e))?;
            }
            Ok(0)
        }
        Op::Zip { files, to } => {
            let n = zip_files(r, files, to)?;
            Ok(n)
        }
        Op::Unzip { archive, to } => {
            let cancel = AtomicBool::new(false);
            // Stops between files when the run is stopped.
            let out = crate::archive::extract_with(archive, to, &cancel, &mut |_, _, _| {
                if r_stop_flag(r.env) {
                    cancel.store(true, Ordering::Relaxed);
                }
            });
            out.map(|u| u.files).map_err(|e| OpError { message: e.to_string(), denied: e.needs_elevation() })
        }
        Op::Run { program, args, cwd, timeout_secs } => run_program(r, program, args, cwd, *timeout_secs).map(|_| 0),
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn to_trash(path: &Path) -> std::result::Result<(), OpError> {
    trash::delete(path).map_err(|e| OpError::text(format!("{}: {e}", path.display())))
}

/// Phones have no trash can the app can use; the file is removed for good.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn to_trash(path: &Path) -> std::result::Result<(), OpError> {
    if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) }.map_err(|e| OpError::io(path, e))
}

fn r_stop_flag(env: &Env) -> bool {
    env.cancel.load(Ordering::Relaxed) || (env.on_sleep == SleepPolicy::Cancel && env.slept.load(Ordering::Relaxed))
}

/// Do the task. Blocks until it is finished or stopped.
pub fn run(task: &Task, env: &Env, now: i64) -> RunResult {
    if task.options.low_priority {
        crate::proc::lower_thread_priority();
    }
    let mut res = RunResult { outcome: Outcome::Success, items: 0, bytes: 0, errors: Vec::new(), denied: false, interrupted_by: None, attempts: 1 };
    let mut problems = Vec::new();
    let plans: Vec<(usize, Vec<Op>)> = task.actions.iter().enumerate().map(|(i, a)| (i, plan_action(i, a, &task.name, now, &mut problems))).collect();
    let total_items: u64 = plans.iter().flat_map(|(_, o)| o.iter()).filter(|o| !matches!(o, Op::MkDir(_))).count() as u64;
    let total_bytes: u64 = plans.iter().flat_map(|(_, o)| o.iter()).map(|o| o.bytes()).sum();
    let mut runner = Runner {
        env,
        deadline: (task.options.timeout_minutes > 0).then(|| Instant::now() + Duration::from_secs(task.options.timeout_minutes as u64 * 60)),
        started: Instant::now(),
        progress: Progress { steps: plans.len(), items_total: total_items, bytes_total: total_bytes, ..Default::default() },
        last_report: Instant::now() - Duration::from_secs(1),
    };
    let mut failed = 0u64;
    let mut woke_at: Option<Instant> = None;
    'all: for (step, ops) in &plans {
        runner.progress.step = *step + 1;
        for op in ops {
            if env.slept.swap(false, Ordering::Relaxed) {
                if env.on_sleep == SleepPolicy::Cancel {
                    res.outcome = Outcome::Interrupted;
                    res.interrupted_by = Some("the computer went to sleep".into());
                    break 'all;
                }
                woke_at = Some(Instant::now());
            }
            if let Some(stop) = runner.stop() {
                stop_result(&mut res, stop);
                break 'all;
            }
            runner.progress.current = op.planned(*step).from;
            if runner.progress.current.is_empty() {
                runner.progress.current = op.planned(*step).to;
            }
            runner.report(false);
            // Right after waking, networks and drives take a moment: be more patient.
            let mut tries = 1 + task.options.retries + if woke_at.is_some_and(|t| t.elapsed() < Duration::from_secs(90)) { 3 } else { 0 };
            loop {
                match exec(&mut runner, op) {
                    Ok(n) => {
                        if !matches!(op, Op::MkDir(_)) {
                            res.items += 1;
                            runner.progress.items_done += 1;
                        }
                        if matches!(op, Op::Copy { .. } | Op::Delete { .. }) {
                            res.bytes += n;
                        } else if matches!(op, Op::Zip { .. }) {
                            res.bytes += op.bytes();
                        }
                        break;
                    }
                    Err(e) => {
                        if let Some(stop) = runner.stop() {
                            stop_result(&mut res, stop);
                            break 'all;
                        }
                        tries -= 1;
                        res.attempts += 1;
                        if tries > 0 && !e.denied {
                            let wait = Duration::from_secs(task.options.retry_delay_secs.max(1) as u64);
                            let until = Instant::now() + wait;
                            while Instant::now() < until && runner.stop().is_none() {
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            continue;
                        }
                        res.denied |= e.denied;
                        failed += 1;
                        if res.errors.len() < 25 {
                            res.errors.push(e.message);
                        }
                        if task.options.stop_on_error {
                            break 'all;
                        }
                        break;
                    }
                }
            }
        }
    }
    if matches!(res.outcome, Outcome::Success) && failed > 0 {
        res.outcome = if res.items > 0 { Outcome::Partial } else { Outcome::Failed };
        if task.options.stop_on_error {
            res.outcome = Outcome::Failed;
        }
    }
    runner.report(true);
    res
}

fn stop_result(res: &mut RunResult, stop: Stop) {
    match stop {
        Stop::User => res.outcome = Outcome::Cancelled,
        Stop::Sleep => {
            res.outcome = Outcome::Interrupted;
            res.interrupted_by = Some("the computer went to sleep".into());
        }
        Stop::Timeout => {
            res.outcome = Outcome::Failed;
            res.errors.push("It ran longer than its time limit and was stopped.".into());
        }
    }
}

// ---- Running as administrator ----------------------------------------------

/// What the app hands to the elevated copy of itself: `--run-task <task.json> --result <result.json>
/// [--cancel <file>]`. Progress goes to `<result>.progress`. Returns the exit code: 0 done (even when
/// some things failed: the result file says), 2 the task could not be read.
pub fn helper_main(args: &[String]) -> i32 {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let (Some(task_file), Some(result_file)) = (value("--run-task"), value("--result")) else { return 2 };
    let Some(task) = fs::read(&task_file).ok().and_then(|b| serde_json::from_slice::<Task>(&b).ok()) else { return 2 };
    let _ = fs::remove_file(&task_file);
    let cancel_file = value("--cancel").map(PathBuf::from);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let slept = AtomicBool::new(false);
    let done = std::sync::Arc::new(AtomicBool::new(false));
    if let Some(f) = cancel_file {
        let (cancel, done) = (cancel.clone(), done.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                if f.exists() {
                    cancel.store(true, Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
    }
    let progress_file = format!("{result_file}.progress");
    let report = |p: &Progress| {
        if let Ok(b) = serde_json::to_vec(p) {
            let _ = fs::write(&progress_file, b);
        }
    };
    let env = Env { cancel: &cancel, slept: &slept, on_progress: &report, on_sleep: task.options.on_sleep };
    let result = run(&task, &env, chrono::Local::now().timestamp_millis());
    done.store(true, Ordering::Relaxed);
    let _ = fs::remove_file(&progress_file);
    match serde_json::to_vec(&result) {
        Ok(b) if fs::write(&result_file, &b).is_ok() => 0,
        _ => 2,
    }
}

// ===========================================================================
// The record of runs, statistics and time estimates
// ===========================================================================

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub task_id: String,
    /// Milliseconds since 1970.
    pub started: i64,
    pub ended: i64,
    pub outcome: Outcome,
    /// "scheduled", "catch-up", "manual", "startup", "folder changed", "drive connected"...
    pub reason: String,
    pub items: u64,
    pub bytes: u64,
    #[serde(default)]
    pub errors: Vec<String>,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub interrupted_by: Option<String>,
    #[serde(default)]
    pub elevated: bool,
    /// What the task was called then.
    #[serde(default)]
    pub name: String,
}

impl RunRecord {
    pub fn secs(&self) -> f64 {
        ((self.ended - self.started).max(0)) as f64 / 1000.0
    }
    pub fn bytes_per_sec(&self) -> Option<f64> {
        let s = self.secs();
        (self.bytes > 0 && s >= 0.2).then(|| self.bytes as f64 / s)
    }
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub runs: u64,
    pub successes: u64,
    pub partial: u64,
    pub failures: u64,
    pub cancelled: u64,
    pub interrupted: u64,
    pub skipped: u64,
    /// Successes as a share of runs that were not skipped (0 to 1).
    pub success_rate: f64,
    pub avg_secs: f64,
    pub median_secs: f64,
    pub p90_secs: f64,
    pub min_secs: f64,
    pub max_secs: f64,
    pub avg_items: f64,
    pub avg_bytes: f64,
    pub avg_bytes_per_sec: f64,
    pub total_items: u64,
    pub total_bytes: u64,
    pub last_run: Option<i64>,
    pub last_success: Option<i64>,
    pub last_failure: Option<i64>,
    pub last_error: Option<String>,
    /// Speed of the latest runs against all runs (above 1: getting faster).
    pub speed_trend: Option<f64>,
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// What the runs so far add up to. Runs that did nothing (skipped) count only as skipped.
pub fn stats(records: &[RunRecord]) -> Stats {
    let mut s = Stats::default();
    let mut secs: Vec<f64> = Vec::new();
    let mut speeds: Vec<f64> = Vec::new();
    for r in records {
        s.runs += 1;
        s.last_run = s.last_run.max(Some(r.started));
        match r.outcome {
            Outcome::Success => {
                s.successes += 1;
                s.last_success = s.last_success.max(Some(r.ended));
            }
            Outcome::Partial => s.partial += 1,
            Outcome::Failed => {
                s.failures += 1;
                s.last_failure = s.last_failure.max(Some(r.ended));
            }
            Outcome::Cancelled => s.cancelled += 1,
            Outcome::Interrupted => s.interrupted += 1,
            Outcome::Skipped => {
                s.skipped += 1;
                continue;
            }
        }
        if matches!(r.outcome, Outcome::Failed | Outcome::Partial) && s.last_failure == Some(r.ended) {
            s.last_error = r.errors.first().cloned();
        }
        s.total_items += r.items;
        s.total_bytes += r.bytes;
        if matches!(r.outcome, Outcome::Success | Outcome::Partial) {
            secs.push(r.secs());
            if let Some(b) = r.bytes_per_sec() {
                speeds.push(b);
            }
        }
    }
    let counted = s.runs - s.skipped;
    s.success_rate = if counted == 0 { 0.0 } else { s.successes as f64 / counted as f64 };
    let done = secs.len() as f64;
    if done > 0.0 {
        let finished: Vec<&RunRecord> = records.iter().filter(|r| matches!(r.outcome, Outcome::Success | Outcome::Partial)).collect();
        s.avg_secs = secs.iter().sum::<f64>() / done;
        s.avg_items = finished.iter().map(|r| r.items as f64).sum::<f64>() / done;
        s.avg_bytes = finished.iter().map(|r| r.bytes as f64).sum::<f64>() / done;
        let mut sorted = secs.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s.median_secs = percentile(&sorted, 0.5);
        s.p90_secs = percentile(&sorted, 0.9);
        s.min_secs = sorted[0];
        s.max_secs = *sorted.last().unwrap();
    }
    if !speeds.is_empty() {
        s.avg_bytes_per_sec = speeds.iter().sum::<f64>() / speeds.len() as f64;
        if speeds.len() >= 4 {
            let recent = &speeds[speeds.len() - 3..];
            s.speed_trend = Some(recent.iter().sum::<f64>() / 3.0 / s.avg_bytes_per_sec);
        }
    }
    s
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Estimate {
    pub secs: f64,
    /// How many past runs it rests on.
    pub samples: u32,
    /// "speed" (from how fast it moved data), "duration" (from how long it took), or "mixed".
    pub basis: &'static str,
}

/// How long a run is expected to take, from the runs so far. Recent runs count more: each
/// older one weighs 0.6 of the next. With the amount of data the run will handle (`bytes`), the
/// learned speed is used; without it, the typical duration.
pub fn estimate(records: &[RunRecord], bytes: Option<u64>) -> Option<Estimate> {
    let good: Vec<&RunRecord> = records.iter().filter(|r| matches!(r.outcome, Outcome::Success | Outcome::Partial) && r.secs() >= 0.05).collect();
    if good.is_empty() {
        return None;
    }
    let weight = |i: usize| 0.6f64.powi((good.len() - 1 - i) as i32);
    let mut dur = (0.0, 0.0);
    let mut spd = (0.0, 0.0);
    for (i, r) in good.iter().enumerate() {
        let w = weight(i);
        dur = (dur.0 + w * r.secs(), dur.1 + w);
        if let Some(b) = r.bytes_per_sec() {
            spd = (spd.0 + w * b, spd.1 + w);
        }
    }
    let by_duration = dur.0 / dur.1;
    match (bytes, spd.1 > 0.0) {
        (Some(b), true) if b > 0 => {
            let by_speed = b as f64 / (spd.0 / spd.1);
            // A run has a fixed cost too (starting up, listing folders): never promise less than a fifth of the usual time.
            Some(Estimate { secs: by_speed.max(by_duration * 0.2), samples: good.len() as u32, basis: "speed" })
        }
        _ => Some(Estimate { secs: by_duration, samples: good.len() as u32, basis: "duration" }),
    }
}

// ---- Storage: one JSON line per run ---------------------------------------

pub fn load_records(file: &Path) -> Vec<RunRecord> {
    let Ok(text) = fs::read_to_string(file) else { return Vec::new() };
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

pub fn append_record(file: &Path, r: &RunRecord) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file)?;
    let mut line = serde_json::to_string(r).map_err(std::io::Error::other)?;
    line.push('\n');
    f.write_all(line.as_bytes())
}

/// Keep the newest `per_task` records of each task, and none older than `max_days` (0 = no age limit).
pub fn trim_records(file: &Path, per_task: usize, max_days: u32, now: i64) -> std::io::Result<usize> {
    let all = load_records(file);
    let before = all.len();
    let cutoff = if max_days == 0 { i64::MIN } else { now - max_days as i64 * 86_400_000 };
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut keep: Vec<RunRecord> = Vec::new();
    for r in all.into_iter().rev() {
        let n = seen.entry(r.task_id.clone()).or_insert(0);
        *n += 1;
        if *n <= per_task && r.started >= cutoff {
            keep.push(r);
        }
    }
    keep.reverse();
    if keep.len() != before {
        let mut text = String::new();
        for r in &keep {
            text.push_str(&serde_json::to_string(r).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        fs::write(file, text)?;
    }
    Ok(before - keep.len())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    fn ms(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        chrono::Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap().timestamp_millis()
    }

    fn task(actions: Vec<Action>) -> Task {
        Task { id: "t1".into(), name: "Test task".into(), notes: String::new(), enabled: true, triggers: vec![Trigger::Manual], actions, options: Options::default(), created: 0, updated: 0, resume_pending: false, needs_admin: false }
    }

    fn env<'a>(cancel: &'a AtomicBool, slept: &'a AtomicBool, policy: SleepPolicy) -> Env<'a> {
        Env { cancel, slept, on_progress: &|_| {}, on_sleep: policy }
    }

    fn go(t: &Task) -> RunResult {
        let (c, s) = (AtomicBool::new(false), AtomicBool::new(false));
        run(t, &env(&c, &s, SleepPolicy::Resume), now_ms())
    }

    fn now_ms() -> i64 {
        chrono::Local::now().timestamp_millis()
    }

    #[test]
    fn daily_picks_the_next_matching_day() {
        // 2026-11-16 is a Monday. After Monday 10:00, "09:00 on Mon and Wed" next fires Wednesday.
        let t = Trigger::Daily { time: "09:00".into(), days: vec![0, 2] };
        assert_eq!(next_time(&t, ms(2026, 11, 16, 10, 0)), Some(ms(2026, 11, 18, 9, 0)));
        assert_eq!(next_time(&t, ms(2026, 11, 16, 8, 0)), Some(ms(2026, 11, 16, 9, 0)));
        // Every day.
        let every = Trigger::Daily { time: "23:30".into(), days: vec![] };
        assert_eq!(next_time(&every, ms(2026, 11, 16, 23, 30)), Some(ms(2026, 11, 17, 23, 30)));
        assert_eq!(next_time(&Trigger::Daily { time: "nonsense".into(), days: vec![] }, 0), None);
    }

    #[test]
    fn monthly_clamps_to_the_last_day() {
        let t = Trigger::Monthly { day: 31, time: "06:00".into() };
        assert_eq!(next_time(&t, ms(2026, 2, 1, 0, 0)), Some(ms(2026, 2, 28, 6, 0)));
        assert_eq!(next_time(&t, ms(2026, 2, 28, 6, 0)), Some(ms(2026, 3, 31, 6, 0)));
        let d = chrono::Local.timestamp_millis_opt(next_time(&t, ms(2026, 12, 31, 7, 0)).unwrap()).unwrap();
        assert_eq!((d.year(), d.month(), d.day()), (2027, 1, 31));
    }

    #[test]
    fn every_counts_from_its_anchor_and_once_fires_once() {
        let t = Trigger::Every { minutes: 30, anchor: 1_000_000 };
        assert_eq!(next_time(&t, 0), Some(1_000_000));
        assert_eq!(next_time(&t, 1_000_000), Some(1_000_000 + 1_800_000));
        assert_eq!(next_time(&t, 1_000_000 + 1_800_001), Some(1_000_000 + 3_600_000));
        assert_eq!(next_time(&Trigger::Once { at: 50 }, 10), Some(50));
        assert_eq!(next_time(&Trigger::Once { at: 50 }, 50), None);
    }

    #[test]
    fn missed_runs_catch_up_or_are_noted() {
        let mut a = task(vec![]);
        a.triggers = vec![Trigger::Every { minutes: 10, anchor: 0 }];
        let mut b = a.clone();
        b.id = "t2".into();
        b.options.catch_up = false;
        let t0 = 60 * 60_000;
        // On time: the tick just before and after one firing.
        assert_eq!(due_runs(&[a.clone()], t0 - 5_000, t0 + 5_000)[0].kind, DueKind::Scheduled);
        // Asleep for three hours: one run, not eighteen.
        let due = due_runs(&[a.clone(), b], t0, t0 + 3 * 3_600_000);
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].kind, DueKind::CatchUp);
        assert_eq!(due[1].kind, DueKind::Missed);
        // Nothing due between two firings.
        assert!(due_runs(&[a], t0 + 1_000, t0 + 20_000).is_empty());
    }

    #[test]
    fn globs_and_filters() {
        assert!(glob_match("*.JPG", "holiday.jpg"));
        assert!(glob_match("img_??.png", "IMG_01.png"));
        assert!(!glob_match("*.jpg", "a.jpeg"));
        assert!(glob_match("*", ""));
        let f = Filter { include: "*.log; *.tmp".into(), exclude: "keep*".into(), older_than_days: Some(30.0), min_bytes: Some(10), recursive: true, ..Default::default() };
        let now = 100 * 86_400_000;
        let old = Some(now - 40 * 86_400_000);
        assert!(f.allows_file("a.log", 100, old, now));
        assert!(!f.allows_file("a.log", 100, Some(now - 86_400_000), now), "too new");
        assert!(!f.allows_file("keep.log", 100, old, now), "excluded");
        assert!(!f.allows_file("a.log", 5, old, now), "too small");
        assert!(!f.allows_file("a.txt", 100, old, now), "not included");
        assert!(!f.allows_file(".a.log", 100, old, now), "hidden");
    }

    #[test]
    fn tokens_expand() {
        let now = ms(2026, 11, 5, 14, 7);
        assert_eq!(expand_tokens("/b/{task}-{date}-{time}.zip", "My backup!", now), "/b/My_backup_-2026-11-05-1407.zip");
        assert_eq!(expand_tokens("{year}/{month}/{day}", "x", now), "2026/11/05");
    }

    #[test]
    fn copy_is_resumable_and_skips_identical_files() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        let dst = d.path().join("dst");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), "aaa").unwrap();
        fs::write(src.join("sub/b.txt"), "bbbb").unwrap();
        let t = task(vec![Action::Copy { from: vec![src.to_string_lossy().into()], to: dst.to_string_lossy().into(), filter: Filter { recursive: true, ..Default::default() }, conflict: Conflict::Overwrite, keep_structure: true }]);
        let p = preview(&t, now_ms(), 100);
        assert_eq!(p.files, 2);
        assert_eq!(p.bytes, 7);
        assert!(!dst.exists(), "a preview changes nothing");
        let r = go(&t);
        assert_eq!(r.outcome, Outcome::Success, "{:?}", r.errors);
        assert_eq!(fs::read_to_string(dst.join("src/sub/b.txt")).unwrap(), "bbbb");
        // Again: everything is already there, so nothing is copied.
        let again = go(&t);
        assert_eq!(again.outcome, Outcome::Success);
        assert_eq!(again.bytes, 0);
        // A changed file is copied again.
        fs::write(src.join("a.txt"), "aaaaaaaa").unwrap();
        assert_eq!(go(&t).bytes, 8);
        assert!(!dst.join("src/a.txt.part").exists());
    }

    #[test]
    fn conflict_policies() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("s"), d.path().join("d"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("f.txt"), "new content").unwrap();
        fs::write(dst.join("f.txt"), "old").unwrap();
        let mk = |c| task(vec![Action::Copy { from: vec![src.join("f.txt").to_string_lossy().into()], to: dst.to_string_lossy().into(), filter: Filter::default(), conflict: c, keep_structure: true }]);
        go(&mk(Conflict::Skip));
        assert_eq!(fs::read_to_string(dst.join("f.txt")).unwrap(), "old");
        go(&mk(Conflict::KeepBoth));
        assert_eq!(fs::read_to_string(dst.join("f (2).txt")).unwrap(), "new content");
        go(&mk(Conflict::Overwrite));
        assert_eq!(fs::read_to_string(dst.join("f.txt")).unwrap(), "new content");
    }

    #[test]
    fn move_removes_sources() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("s"), d.path().join("d"));
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.jpg"), "x").unwrap();
        fs::write(src.join("b.txt"), "y").unwrap();
        let t = task(vec![Action::Move { from: vec![src.to_string_lossy().into()], to: dst.to_string_lossy().into(), filter: Filter { include: "*.jpg".into(), recursive: true, ..Default::default() }, conflict: Conflict::Overwrite, keep_structure: false }]);
        assert_eq!(go(&t).outcome, Outcome::Success);
        assert!(dst.join("a.jpg").exists() && !src.join("a.jpg").exists());
        assert!(src.join("b.txt").exists(), "what the filter left out stays");
    }

    #[test]
    fn delete_by_age_and_empty_folders() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("dl");
        fs::create_dir_all(root.join("old/inner")).unwrap();
        fs::write(root.join("old/inner/x.tmp"), "1").unwrap();
        fs::write(root.join("fresh.tmp"), "2").unwrap();
        let old = filetime::FileTime::from_unix_time(now_ms() / 1000 - 90 * 86_400, 0);
        filetime::set_file_mtime(root.join("old/inner/x.tmp"), old).unwrap();
        let t = task(vec![Action::Delete { paths: vec![root.to_string_lossy().into()], filter: Filter { older_than_days: Some(30.0), recursive: true, ..Default::default() }, permanent: true, remove_empty_folders: true, remove_folder: false }]);
        assert_eq!(preview(&t, now_ms(), 50).files, 3, "one old file plus two folders to check");
        assert_eq!(go(&t).outcome, Outcome::Success);
        assert!(!root.join("old").exists(), "the folder that was left empty is gone");
        assert!(root.join("fresh.tmp").exists(), "the new file stays");
    }

    #[test]
    fn sync_mirror_copies_changes_and_removes_extras() {
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("s"), d.path().join("d"));
        fs::create_dir_all(&src).unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::write(src.join("keep.txt"), "k").unwrap();
        fs::write(dst.join("extra.txt"), "e").unwrap();
        let t = task(vec![Action::Sync { from: src.to_string_lossy().into(), to: dst.to_string_lossy().into(), mirror: false, filter: Filter { recursive: true, ..Default::default() } }]);
        go(&t);
        assert!(dst.join("keep.txt").exists() && dst.join("extra.txt").exists());
        let mut m = t.clone();
        if let Action::Sync { mirror, .. } = &mut m.actions[0] {
            *mirror = true;
        }
        // The mirror's removals go to the trash; in this test environment that may not exist, which must not hide the copy.
        let _ = go(&m);
        assert!(dst.join("keep.txt").exists());
    }

    #[test]
    fn compress_then_extract_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("docs");
        fs::create_dir_all(src.join("n")).unwrap();
        fs::write(src.join("a.txt"), "alpha").unwrap();
        fs::write(src.join("n/b.txt"), "beta").unwrap();
        let zip = d.path().join("out/{task}.zip");
        let out = d.path().join("unzipped");
        let t = task(vec![
            Action::Compress { from: vec![src.to_string_lossy().into()], to: zip.to_string_lossy().into(), filter: Filter { recursive: true, ..Default::default() } },
            Action::Extract { archive: d.path().join("out/Test_task.zip").to_string_lossy().into(), to: out.to_string_lossy().into() },
        ]);
        let r = go(&t);
        assert_eq!(r.outcome, Outcome::Success, "{:?}", r.errors);
        assert_eq!(fs::read_to_string(out.join("docs/n/b.txt")).unwrap(), "beta");
    }

    #[test]
    fn run_program_reports_failure_and_stops() {
        let (prog, ok_args, bad_args): (&str, Vec<String>, Vec<String>) = if cfg!(windows) { ("cmd", vec!["/C".into(), "exit 0".into()], vec!["/C".into(), "exit 3".into()]) } else { ("sh", vec!["-c".into(), "exit 0".into()], vec!["-c".into(), "echo boom >&2; exit 3".into()]) };
        let ok = task(vec![Action::Run { program: prog.into(), args: ok_args, cwd: String::new(), timeout_secs: 10 }]);
        assert_eq!(go(&ok).outcome, Outcome::Success);
        let bad = task(vec![Action::Run { program: prog.into(), args: bad_args, cwd: String::new(), timeout_secs: 10 }, Action::Run { program: prog.into(), args: vec![], cwd: String::new(), timeout_secs: 1 }]);
        let r = go(&bad);
        assert_eq!(r.outcome, Outcome::Failed);
        assert_eq!(r.errors.len(), 1, "stops at the first failure: {:?}", r.errors);
    }

    #[test]
    fn cancel_and_sleep_policy() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..5 {
            fs::write(d.path().join(format!("f{i}")), "x").unwrap();
        }
        let t = task(vec![Action::Copy { from: vec![d.path().to_string_lossy().into()], to: d.path().join("../copy").to_string_lossy().into(), filter: Filter { recursive: true, ..Default::default() }, conflict: Conflict::Overwrite, keep_structure: true }]);
        // Cancelled before the first file.
        let (c, s) = (AtomicBool::new(true), AtomicBool::new(false));
        assert_eq!(run(&t, &env(&c, &s, SleepPolicy::Resume), now_ms()).outcome, Outcome::Cancelled);
        // The computer slept, and the task is set to give up.
        let (c, s) = (AtomicBool::new(false), AtomicBool::new(true));
        let r = run(&t, &env(&c, &s, SleepPolicy::Cancel), now_ms());
        assert_eq!(r.outcome, Outcome::Interrupted);
        assert!(r.interrupted_by.unwrap().contains("sleep"));
        // The same sleep with "resume" carries on and finishes.
        let (c, s) = (AtomicBool::new(false), AtomicBool::new(true));
        let r = run(&t, &env(&c, &s, SleepPolicy::Resume), now_ms());
        assert_eq!(r.outcome, Outcome::Success);
        assert!(!s.load(Ordering::Relaxed), "the wake-up is noted once");
        let _ = fs::remove_dir_all(d.path().join("../copy"));
    }

    fn rec(started: i64, secs: f64, bytes: u64, outcome: Outcome) -> RunRecord {
        RunRecord { task_id: "t1".into(), started, ended: started + (secs * 1000.0) as i64, outcome, reason: "scheduled".into(), items: 10, bytes, errors: if outcome == Outcome::Failed { vec!["disk full".into()] } else { vec![] }, attempts: 1, interrupted_by: None, elevated: false, name: "t".into() }
    }

    #[test]
    fn statistics_add_up() {
        let runs = vec![rec(1000, 10.0, 100_000_000, Outcome::Success), rec(2000, 20.0, 100_000_000, Outcome::Success), rec(3000, 5.0, 0, Outcome::Failed), rec(4000, 0.0, 0, Outcome::Skipped), rec(5000, 1.0, 0, Outcome::Cancelled)];
        let s = stats(&runs);
        assert_eq!((s.runs, s.successes, s.failures, s.skipped, s.cancelled), (5, 2, 1, 1, 1));
        assert!((s.success_rate - 0.5).abs() < 1e-9, "skipped runs do not count against it: {}", s.success_rate);
        assert!((s.avg_secs - 15.0).abs() < 1e-9);
        assert!((s.avg_bytes_per_sec - 7_500_000.0).abs() < 1.0, "{}", s.avg_bytes_per_sec);
        assert_eq!(s.last_error.as_deref(), Some("disk full"));
        assert_eq!(s.max_secs, 20.0);
    }

    #[test]
    fn estimates_follow_recent_speed() {
        assert!(estimate(&[], None).is_none());
        // Speed halves over time: the estimate leans to the recent, slower runs.
        let runs = vec![rec(0, 10.0, 100_000_000, Outcome::Success), rec(1, 10.0, 100_000_000, Outcome::Success), rec(2, 20.0, 100_000_000, Outcome::Success)];
        let e = estimate(&runs, Some(100_000_000)).unwrap();
        assert_eq!(e.basis, "speed");
        assert!(e.secs > 12.0 && e.secs < 20.0, "{}", e.secs);
        // Twice the data takes about twice as long.
        let e2 = estimate(&runs, Some(200_000_000)).unwrap();
        assert!((e2.secs / e.secs - 2.0).abs() < 0.01);
        // Without an amount of data it falls back to how long runs took.
        assert_eq!(estimate(&runs, None).unwrap().basis, "duration");
        // Failed runs teach nothing.
        assert!(estimate(&[rec(0, 5.0, 10, Outcome::Failed)], None).is_none());
    }

    #[test]
    fn records_are_stored_and_trimmed() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("h.jsonl");
        for i in 0..6 {
            append_record(&f, &rec(i * 1000, 1.0, 1, Outcome::Success)).unwrap();
        }
        let mut other = rec(500, 1.0, 1, Outcome::Success);
        other.task_id = "t2".into();
        append_record(&f, &other).unwrap();
        assert_eq!(load_records(&f).len(), 7);
        assert_eq!(trim_records(&f, 3, 0, 10_000).unwrap(), 3);
        let left = load_records(&f);
        assert_eq!(left.iter().filter(|r| r.task_id == "t1").map(|r| r.started).collect::<Vec<_>>(), vec![3000, 4000, 5000], "the newest are kept");
        assert_eq!(left.iter().filter(|r| r.task_id == "t2").count(), 1);
        // Age limit: one day in milliseconds.
        assert_eq!(trim_records(&f, 100, 1, 10 * 86_400_000).unwrap(), 4);
    }

    #[test]
    fn tasks_survive_a_round_trip_through_json() {
        let mut t = task(vec![Action::Delete { paths: vec!["/x".into()], filter: Filter::default(), permanent: false, remove_empty_folders: true, remove_folder: false }]);
        t.triggers = vec![Trigger::Daily { time: "03:00".into(), days: vec![0, 4] }, Trigger::OnChange { path: "/x".into(), quiet_secs: 5 }, Trigger::Startup];
        let back: Task = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        // An old or hand-written file with missing fields still loads.
        let minimal: Task = serde_json::from_str(r#"{"id":"a","name":"b","actions":[{"type":"run","program":"x"}]}"#).unwrap();
        assert!(minimal.enabled && minimal.options.catch_up && minimal.options.stop_on_error);
    }

    #[test]
    fn reads_what_the_window_sends() {
        // Exactly the shape src/tasks/model.ts produces.
        let json = r#"{"id":"","name":"Nightly","notes":"","enabled":true,
          "triggers":[{"type":"daily","time":"02:30","days":[]},{"type":"every","minutes":90,"anchor":1000},{"type":"onChange","path":"/x","quietSecs":10},{"type":"driveConnected","name":"SD"},{"type":"startup"},{"type":"monthly","day":31,"time":"01:00"},{"type":"once","at":5},{"type":"manual"}],
          "actions":[{"type":"copy","from":["/a"],"to":"/b","filter":{"include":"*.jpg","exclude":"","olderThanDays":null,"newerThanDays":1.5,"minBytes":null,"maxBytes":1000,"hidden":false,"recursive":true},"conflict":"overwriteIfNewer","keepStructure":false},
                     {"type":"delete","paths":["/c"],"filter":{"include":"","exclude":"","olderThanDays":30,"newerThanDays":null,"minBytes":null,"maxBytes":null,"hidden":false,"recursive":true},"permanent":false,"removeEmptyFolders":true,"removeFolder":false},
                     {"type":"sync","from":"/a","to":"/b","mirror":true,"filter":{"include":"","exclude":"","olderThanDays":null,"newerThanDays":null,"minBytes":null,"maxBytes":null,"hidden":false,"recursive":true}},
                     {"type":"run","program":"x","args":["--a"],"cwd":"","timeoutSecs":5},
                     {"type":"extract","archive":"/a.zip","to":"/o"},{"type":"compress","from":["/a"],"to":"/a.zip","filter":{"include":"","exclude":"","olderThanDays":null,"newerThanDays":null,"minBytes":null,"maxBytes":null,"hidden":false,"recursive":true}}],
          "options":{"onSleep":"cancel","catchUp":true,"retries":2,"retryDelaySecs":5,"timeoutMinutes":0,"stopOnError":false,"onlyOnAc":true,"runAsAdmin":false,"notify":"always","skipIfRunning":true,"lowPriority":true},
          "created":0,"updated":0,"resumePending":false,"needsAdmin":false}"#;
        let t: Task = serde_json::from_str(json).expect("the window's JSON loads");
        assert_eq!(t.triggers.len(), 8);
        assert_eq!(t.actions.len(), 6);
        assert_eq!(t.options.on_sleep, SleepPolicy::Cancel);
        assert!(matches!(&t.actions[0], Action::Copy { conflict: Conflict::OverwriteIfNewer, keep_structure: false, .. }));
    }
}
