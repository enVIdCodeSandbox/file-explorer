//! Crash logs: what was happening when the app stopped, in one plain text file that can be pasted anywhere.
//!
//! * A short trail of events (what the person did, which commands ran) is kept in memory and flushed
//!   to disk every couple of seconds, so it survives even a crash that no panic hook can see.
//! * A panic writes a report straight away (message, place, backtrace, trail, computer information).
//! * A marker file says "running". If it is still there at the next start, the last run ended without
//!   a clean exit (a native crash, a kill, a power cut) and a report is written from the saved trail.
//!
//! Names of people are kept out: the user name in paths becomes `<user>`; the computer name and e-mail
//! addresses are never written.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const TRAIL: usize = 250;
const KEEP_LOGS: usize = 30;

struct State {
    dir: PathBuf,
    trail: VecDeque<String>,
    dirty: bool,
    context: String,
    started: u64,
    version: String,
    webview: String,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// `2026-11-15 19:52:03` (UTC) from seconds since 1970.
pub fn stamp(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn file_stamp(secs: u64) -> String {
    stamp(secs).replace([' ', ':'], "-")
}

/// Starts the crash log in `dir`. Returns the report written for an unclean last exit, if any.
pub fn init(dir: &Path, version: &str, webview: &str) -> Option<PathBuf> {
    let _ = std::fs::create_dir_all(dir);
    let marker = dir.join("running.marker");
    let events = dir.join("last-events.log");
    let mut found = None;
    if marker.exists() {
        let trail = std::fs::read_to_string(&events).unwrap_or_default();
        let since = std::fs::read_to_string(&marker).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
        let ctx = std::fs::read_to_string(dir.join("last-context.txt")).unwrap_or_default();
        // A panic that took the app down already wrote its own report; do not write a second one.
        let lines: Vec<&str> = trail.lines().collect();
        let died_of_panic = lines.iter().rev().take(5).any(|l| l.contains(" PANIC on thread "));
        let text = build(
            "The last run ended without closing properly",
            "No panic was recorded. That usually means a crash inside the system or a graphics/driver library, the program being killed, or the computer losing power. The trail below is what happened right before.",
            None,
            since,
            version,
            webview,
            &ctx,
            trail.lines().map(str::to_string).collect(),
        );
        found = if died_of_panic { list(dir).first().map(|l| l.path.clone()) } else { write_report(dir, &text) };
    }
    let started = now_ms() as u64 / 1000;
    let _ = std::fs::write(&marker, started.to_string());
    let _ = std::fs::remove_file(&events);
    let st = State { dir: dir.to_path_buf(), trail: VecDeque::new(), dirty: false, context: String::new(), started, version: version.into(), webview: webview.into() };
    let _ = STATE.set(Mutex::new(st));
    note("app started");
    found
}

/// The app is closing normally: no crash report will be made for this run.
pub fn clean_exit() {
    if let Some(s) = STATE.get() {
        let dir = s.lock().dir.clone();
        let _ = std::fs::remove_file(dir.join("running.marker"));
        let _ = std::fs::remove_file(dir.join("last-events.log"));
        let _ = std::fs::remove_file(dir.join("last-context.txt"));
    }
}

/// One line in the trail.
pub fn note(event: &str) {
    let Some(s) = STATE.get() else { return };
    let mut s = s.lock();
    let line = format!("{} {}", stamp_ms(now_ms()), redact(event.trim()));
    if s.trail.len() >= TRAIL {
        s.trail.pop_front();
    }
    s.trail.push_back(line);
    s.dirty = true;
}

fn stamp_ms(ms: u128) -> String {
    format!("{}.{:03}", &stamp((ms / 1000) as u64)[11..], ms % 1000)
}

/// What the window looks like right now (tabs, view, selection...), set by the page every few seconds.
pub fn set_context(text: &str) {
    if let Some(s) = STATE.get() {
        let mut s = s.lock();
        s.context = redact(text);
        let (dir, ctx) = (s.dir.clone(), s.context.clone());
        drop(s);
        let _ = std::fs::write(dir.join("last-context.txt"), ctx);
    }
}

/// Writes the trail to disk if it changed. Called from a timer.
pub fn flush() {
    let Some(s) = STATE.get() else { return };
    let (dir, text) = {
        let mut s = s.lock();
        if !s.dirty {
            return;
        }
        s.dirty = false;
        (s.dir.clone(), s.trail.iter().cloned().collect::<Vec<_>>().join("\n"))
    };
    let _ = std::fs::write(dir.join("last-events.log"), text);
}

/// Keeps a background thread flushing the trail.
pub fn start_flusher() {
    std::thread::Builder::new()
        .name("crashlog-flush".into())
        .spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            flush();
        })
        .ok();
}

/// Writes a report for a problem the app caught itself (a panic, an error in the page).
pub fn report(kind: &str, message: &str, location: Option<&str>) -> Option<PathBuf> {
    let s = STATE.get()?;
    let (dir, trail, ctx, started, version, webview) = {
        let s = s.lock();
        (s.dir.clone(), s.trail.iter().cloned().collect::<Vec<_>>(), s.context.clone(), s.started, s.version.clone(), s.webview.clone())
    };
    let text = build(kind, message, location, started, &version, &webview, &ctx, trail);
    write_report(&dir, &text)
}

/// Makes panics write a report (and still print as usual).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "(no message)".to_string()
        };
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let loc = info.location().map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let bt = std::backtrace::Backtrace::force_capture();
        note(&format!("PANIC on thread {thread}: {msg}"));
        report("Panic", &format!("{msg}\nthread: {thread}\n\nBacktrace:\n{bt}"), loc.as_deref());
        previous(info);
    }));
}

fn write_report(dir: &Path, text: &str) -> Option<PathBuf> {
    let name = format!("crash-{}-{}.txt", file_stamp(now_ms() as u64 / 1000), now_ms() % 1000);
    let path = dir.join(name);
    std::fs::write(&path, text).ok()?;
    prune(dir);
    Some(path)
}

fn prune(dir: &Path) {
    let mut logs = list(dir);
    while logs.len() > KEEP_LOGS {
        if let Some(old) = logs.pop() {
            let _ = std::fs::remove_file(&old.path);
            let _ = std::fs::remove_file(old.path.with_extension("sent"));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Log {
    pub path: PathBuf,
    pub name: String,
    pub bytes: u64,
    pub modified: u64,
    pub sent: bool,
}

/// Saved reports, newest first.
pub fn list(dir: &Path) -> Vec<Log> {
    let mut out: Vec<Log> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("crash-") && name.ends_with(".txt")) {
                return None;
            }
            let m = e.metadata().ok()?;
            let modified = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some(Log { sent: e.path().with_extension("sent").exists(), path: e.path(), name, bytes: m.len(), modified })
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// Removes every saved report. Returns how many.
pub fn clear(dir: &Path) -> usize {
    let logs = list(dir);
    for l in &logs {
        let _ = std::fs::remove_file(&l.path);
        let _ = std::fs::remove_file(l.path.with_extension("sent"));
    }
    logs.len()
}

pub fn mark_sent(path: &Path) {
    let _ = std::fs::write(path.with_extension("sent"), "");
}

/// The user's name, home folder and e-mail addresses removed from text.
pub fn redact(text: &str) -> String {
    let mut out = text.to_string();
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default();
    if home.len() > 3 {
        out = out.replace(&home, "~");
        out = out.replace(&home.replace('\\', "/"), "~");
    }
    let user = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_default();
    if user.len() > 2 && !matches!(user.to_lowercase().as_str(), "root" | "user" | "admin" | "users" | "home") {
        out = out.replace(&user, "<user>");
    }
    // anything shaped like a@b.c
    let mut res = String::with_capacity(out.len());
    for (i, word) in out.split(' ').enumerate() {
        if i > 0 {
            res.push(' ');
        }
        let at = word.find('@');
        if at.map_or(false, |a| a > 0 && word[a..].contains('.')) {
            res.push_str("<email>");
        } else {
            res.push_str(word);
        }
    }
    res
}

/// The computer, as far as helps find a problem. No name of the computer or the person.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn computer_info(version: &str, webview: &str) -> String {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let gb = |b: u64| format!("{:.1} GB", b as f64 / 1_073_741_824.0);
    let mut s = String::new();
    s.push_str(&format!("App:        FAST File Explorer {version}\n"));
    s.push_str(&format!("OS:         {} {} (kernel {})\n", System::name().unwrap_or_default(), System::os_version().unwrap_or_default(), System::kernel_version().unwrap_or_default()));
    s.push_str(&format!("Arch:       {}\n", std::env::consts::ARCH));
    s.push_str(&format!("CPU:        {} ({} logical cores)\n", cpu, sys.cpus().len()));
    s.push_str(&format!("Memory:     {} total, {} free, {} swap in use\n", gb(sys.total_memory()), gb(sys.available_memory()), gb(sys.used_swap())));
    s.push_str(&format!("WebView:    {webview}\n"));
    s.push_str(&format!("Build:      {}\n", if cfg!(debug_assertions) { "debug" } else { "release" }));
    s
}

/// Phones: the system library used on computers is not part of the mobile builds.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn computer_info(version: &str, webview: &str) -> String {
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    format!(
        "App:        FAST File Explorer {version}\nOS:         {}\nArch:       {}\nCPU:        {cores} logical cores\nWebView:    {webview}\nBuild:      {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) { "debug" } else { "release" }
    )
}

#[allow(clippy::too_many_arguments)]
fn build(kind: &str, message: &str, location: Option<&str>, started: u64, version: &str, webview: &str, ctx: &str, trail: Vec<String>) -> String {
    let now = now_ms() as u64 / 1000;
    let mut s = String::new();
    s.push_str("=== FAST File Explorer crash report ===\n");
    s.push_str(&format!("Time (UTC): {}\n", stamp(now)));
    if started > 0 {
        s.push_str(&format!("Run started (UTC): {}  (up {} s)\n", stamp(started), now.saturating_sub(started)));
    }
    s.push_str(&format!("Kind: {kind}\n"));
    if let Some(l) = location {
        s.push_str(&format!("Where: {}\n", redact(l)));
    }
    s.push_str("\n--- Problem ---\n");
    s.push_str(&redact(message));
    s.push_str("\n\n--- Computer ---\n");
    s.push_str(&computer_info(version, webview));
    if !ctx.trim().is_empty() {
        s.push_str("\n--- Window state (last known) ---\n");
        s.push_str(ctx.trim());
        s.push('\n');
    }
    let pending = pending_commands(&trail);
    if !pending.is_empty() {
        s.push_str("\n--- Commands still running ---\n");
        for p in pending {
            s.push_str(&format!("{p}\n"));
        }
    }
    s.push_str(&format!("\n--- What happened last (oldest first, last {} events) ---\n", trail.len()));
    for l in &trail {
        s.push_str(l);
        s.push('\n');
    }
    s.push_str("\n=== end ===\n");
    s
}

/// Lines like `12:00:01.100 -> copy_items` with no matching `<- copy_items`.
fn pending_commands(trail: &[String]) -> Vec<String> {
    let mut open: Vec<(String, String)> = Vec::new();
    for l in trail {
        let body = l.splitn(2, ' ').nth(1).unwrap_or("");
        if let Some(name) = body.strip_prefix("-> ") {
            open.push((name.trim().to_string(), l.clone()));
        } else if let Some(rest) = body.strip_prefix("<- ").or_else(|| body.strip_prefix("!! ")) {
            let name = rest.split(' ').next().unwrap_or("");
            if let Some(i) = open.iter().position(|(n, _)| n == name) {
                open.remove(i);
            }
        }
    }
    open.into_iter().map(|(_, l)| l).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00");
        assert_eq!(stamp(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn commands_without_an_end_are_the_ones_that_were_running() {
        let t: Vec<String> = ["00:00:01.000 -> a", "00:00:01.100 -> b", "00:00:01.200 <- a 100ms", "00:00:01.300 !! c boom"].iter().map(|s| s.to_string()).collect();
        let p = pending_commands(&t);
        assert_eq!(p.len(), 1);
        assert!(p[0].ends_with("-> b"));
    }

    #[test]
    fn e_mail_addresses_do_not_get_into_a_log() {
        assert_eq!(redact("sent to me@example.com now"), "sent to <email> now");
    }

    #[test]
    fn an_unclean_exit_leaves_a_report_and_a_clean_one_does_not() {
        let dir = std::env::temp_dir().join(format!("fast-crash-test-{}", now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("running.marker"), "1700000000").unwrap();
        std::fs::write(dir.join("last-events.log"), "00:00:01.000 -> copy_items\n").unwrap();
        let r = init(&dir, "0.0.0", "test").expect("a report");
        let text = std::fs::read_to_string(r).unwrap();
        assert!(text.contains("ended without closing properly"));
        assert!(text.contains("copy_items"));
        assert!(text.contains("Commands still running"));
        assert_eq!(list(&dir).len(), 1);
        assert_eq!(clear(&dir), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
