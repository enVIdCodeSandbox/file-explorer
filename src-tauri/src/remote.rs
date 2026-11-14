//! Servers (FTP, SFTP, WebDAV): connections, browsing, and transfer jobs.
//!
//! A location on a server is written as a URL (`sftp://anna@host/docs`) and
//! travels through the app like any path. Passwords are only ever held in
//! memory, for the servers logged in to during this run.

use crate::commands::{CmdResult, Listing};
use crate::state::Shared;
use fast_core::entry::FileEntry;
use fast_core::ops::{DeletePhase, DeleteProgress};
use fast_core::remote::{self, Control, Location, Protocol, Server, Session, Tally};
use fast_core::transfer::engine::TransferError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

/// The error text that asks the interface to show the login dialog.
const NEEDS_LOGIN: &str = "needs-login:";
const MAX_IDLE: usize = 4;

/// A server logged in to during this run, and connections ready for reuse.
pub struct Remote {
    server: Server,
    idle: Vec<Box<dyn Session>>,
}

fn err<E: ToString>(e: E) -> String {
    e.to_string()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> CmdResult<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

/// A connection borrowed from the pool; it goes back when dropped, unless it broke.
struct Lease {
    st: Shared,
    key: String,
    session: Option<Box<dyn Session>>,
    from_pool: bool,
    broken: bool,
}

impl Lease {
    fn new(st: &Shared, key: &str, fresh: bool) -> Result<Lease, String> {
        let (server, idle) = {
            let mut remotes = st.remotes.lock();
            let r = remotes.get_mut(key).ok_or_else(|| format!("{NEEDS_LOGIN}{key}"))?;
            (r.server.clone(), if fresh { None } else { r.idle.pop() })
        };
        let from_pool = idle.is_some();
        let session = match idle {
            Some(s) => s,
            None => remote::connect(&server, &st.paths.known_hosts).map_err(err)?,
        };
        Ok(Lease { st: st.clone(), key: key.to_string(), session: Some(session), from_pool, broken: false })
    }

    fn get(&mut self) -> &mut dyn Session {
        self.session.as_deref_mut().expect("lease is open")
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let Some(mut session) = self.session.take() else { return };
        if self.broken || session.home().is_err() {
            return;
        }
        if let Some(r) = self.st.remotes.lock().get_mut(&self.key) {
            if r.idle.len() < MAX_IDLE {
                r.idle.push(session);
            }
        }
    }
}

/// Run one short operation on a pooled connection. A connection the server
/// closed in the meantime is replaced, and the operation tried once more.
fn with_session<T>(st: &Shared, key: &str, mut f: impl FnMut(&mut dyn Session) -> fast_core::Result<T>) -> CmdResult<T> {
    let mut lease = Lease::new(st, key, false)?;
    match f(lease.get()) {
        Ok(v) => Ok(v),
        Err(first) => {
            if lease.from_pool && lease.get().home().is_err() {
                lease.broken = true;
                drop(lease);
                let mut fresh = Lease::new(st, key, true)?;
                return f(fresh.get()).map_err(err);
            }
            Err(err(first))
        }
    }
}

// ---------------------------------------------------------------------------
// Connecting and browsing
// ---------------------------------------------------------------------------

/// Log in to a server. Returns the address of `path`, or of the server's
/// starting folder when `path` is empty.
#[tauri::command]
pub async fn remote_connect(state: State<'_, Shared>, server: Server, path: Option<String>) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let mut session = remote::connect(&server, &st.paths.known_hosts).map_err(err)?;
        let start = match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => remote::normalize(p),
            None => session.home().map_err(err)?,
        };
        let loc = Location { protocol: server.protocol, tls: server.tls, user: server.user.clone(), host: server.host.clone(), port: server.port, path: start };
        // Prove the folder opens before reporting success.
        session.list(&loc.path).map_err(err)?;
        let key = loc.key();
        let mut remembered = server;
        remembered.trust = None;
        st.remotes.lock().insert(key, Remote { server: remembered, idle: vec![session] });
        Ok(loc.url())
    })
    .await
}

/// Forget a server's login and close its connections.
#[tauri::command]
pub async fn remote_disconnect(state: State<'_, Shared>, location: String) -> CmdResult<()> {
    let key = Location::parse(&location).map_err(err)?.key();
    state.remotes.lock().remove(&key);
    Ok(())
}

/// Whether a login for this server is held in memory.
#[tauri::command]
pub async fn remote_is_connected(state: State<'_, Shared>, location: String) -> CmdResult<bool> {
    let key = Location::parse(&location).map_err(err)?.key();
    Ok(state.remotes.lock().contains_key(&key))
}

pub fn list(st: &Shared, url: &str) -> CmdResult<Listing> {
    let loc = Location::parse(url).map_err(err)?;
    let show_hidden = st.settings.read().general.show_hidden;
    let entries = with_session(st, &loc.key(), |s| s.list(&loc.path))?;
    let entries: Vec<FileEntry> = entries
        .into_iter()
        .map(|e| FileEntry { is_hidden: e.name.starts_with('.'), name: e.name, is_dir: e.is_dir, is_symlink: false, size: e.size, modified: e.modified.map(|s| s * 1000), created: None })
        .filter(|e| show_hidden || !e.is_hidden)
        .collect();
    Ok(Listing { path: loc.url(), parent: loc.parent().map(|p| p.url()), entries: Arc::new(entries), cached: false })
}

/// What the server says about one item, for the details pane.
pub fn details(st: &Shared, url: &str) -> CmdResult<fast_core::details::Details> {
    let loc = Location::parse(url).map_err(err)?;
    let name = if loc.path == "/" { loc.host.clone() } else { loc.name().to_string() };
    let entry = match loc.parent() {
        Some(parent) => with_session(st, &loc.key(), |s| s.list(&parent.path))?.into_iter().find(|e| e.name == name),
        None => None,
    };
    let (is_dir, size, modified) = entry.map(|e| (e.is_dir, e.size, e.modified.map(|s| s * 1000))).unwrap_or((true, 0, None));
    Ok(fast_core::details::Details {
        extension: (!is_dir).then(|| name.rsplit_once('.').map(|(_, e)| e.to_lowercase())).flatten(),
        name,
        path: loc.url(),
        parent: loc.parent().map(|p| p.url()),
        kind: if is_dir { "folder" } else { "file" }.to_string(),
        size,
        size_on_disk: None,
        created: None,
        modified,
        accessed: None,
        readonly: false,
        hidden: false,
        mime: None,
        symlink_target: None,
        unix: None,
        attributes: Vec::new(),
        image: None,
        item_count: None,
        volume: None,
        media: Vec::new(),
    })
}

#[tauri::command]
pub async fn remote_mkdir(state: State<'_, Shared>, parent: String, name: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let dir = Location::parse(&parent).map_err(err)?;
        let made = dir.child(remote::safe_name(&name).to_string_lossy().as_ref());
        with_session(&st, &dir.key(), |s| s.mkdir(&made.path))?;
        Ok(made.url())
    })
    .await
}

#[tauri::command]
pub async fn remote_rename(state: State<'_, Shared>, from: String, new_name: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let src = Location::parse(&from).map_err(err)?;
        let parent = src.parent().ok_or("This folder cannot be renamed.")?;
        let dest = parent.child(remote::safe_name(&new_name).to_string_lossy().as_ref());
        with_session(&st, &src.key(), |s| s.rename(&src.path, &dest.path))?;
        Ok(dest.url())
    })
    .await
}

// ---------------------------------------------------------------------------
// Saved servers (addresses only)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SavedServer {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub key_file: Option<String>,
    /// The folder to open.
    #[serde(default)]
    pub path: String,
}

fn load_saved(path: &Path) -> Vec<SavedServer> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn store_saved(path: &Path, list: &[SavedServer]) -> CmdResult<()> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(path, serde_json::to_string_pretty(list).map_err(err)?).map_err(err)
}

#[tauri::command]
pub async fn servers_list(state: State<'_, Shared>) -> CmdResult<Vec<SavedServer>> {
    Ok(load_saved(&state.paths.servers))
}

#[tauri::command]
pub async fn servers_save(state: State<'_, Shared>, entry: SavedServer) -> CmdResult<Vec<SavedServer>> {
    let mut list = load_saved(&state.paths.servers);
    list.retain(|s| s.name != entry.name);
    list.push(entry);
    list.sort_by_key(|s| s.name.to_lowercase());
    store_saved(&state.paths.servers, &list)?;
    Ok(list)
}

#[tauri::command]
pub async fn servers_remove(state: State<'_, Shared>, name: String) -> CmdResult<Vec<SavedServer>> {
    let mut list = load_saved(&state.paths.servers);
    list.retain(|s| s.name != name);
    store_saved(&state.paths.servers, &list)?;
    Ok(list)
}

/// Forget the fingerprint of a server (when its key legitimately changed).
#[tauri::command]
pub async fn servers_forget_host(state: State<'_, Shared>, host: String, port: u16) -> CmdResult<()> {
    let wanted = format!("{host}:{port} ");
    let file = &state.paths.known_hosts;
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with(&wanted)).collect();
    std::fs::write(file, kept.join("\n") + if kept.is_empty() { "" } else { "\n" }).map_err(err)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

/// Reports a job's progress on the transfers card (the same card deletes use).
struct Reporter {
    app: AppHandle,
    id: u64,
    action: &'static str,
    selected: u64,
    first_name: String,
    started: Instant,
    cancel: Arc<AtomicBool>,
    last: parking_lot::Mutex<Instant>,
    ask: Arc<crate::state::JobAsk>,
    /// Files left alone so far (shown as "skipped", not as failures).
    skipped: AtomicU64,
}

impl Reporter {
    fn emit(&self, phase: DeletePhase, items: (u64, u64), bytes: (u64, u64), current: &str, result: &str, errors: &[(String, String)]) {
        self.emit_with(phase, items, bytes, current, result, errors, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_with(&self, phase: DeletePhase, items: (u64, u64), bytes: (u64, u64), current: &str, result: &str, errors: &[(String, String)], conflict: Option<fast_core::transfer::ConflictInfo>) {
        let _ = self.app.emit(
            "delete-progress",
            DeleteProgress {
                id: self.id,
                phase,
                permanent: false,
                selected: self.selected,
                items_total: items.1,
                items_done: items.0,
                current: current.to_string(),
                errors: errors.iter().take(50).map(|(p, m)| TransferError { path: p.clone(), message: m.clone(), needs_elevation: false }).collect(),
                error_count: errors.len() as u64,
                elapsed_secs: self.started.elapsed().as_secs_f64(),
                first_name: self.first_name.clone(),
                action: self.action.to_string(),
                result: result.to_string(),
                bytes_done: bytes.0,
                bytes_total: bytes.1,
                skipped: self.skipped.load(Ordering::Relaxed),
                conflict,
            },
        );
    }

    /// What to do about a file that is already there: the setting says, or (with "ask") the
    /// person is asked and this waits for the answer. `None` means the job was cancelled.
    fn decide(&self, policy: fast_core::transfer::Conflict, sticky: &mut Option<fast_core::browse::Choice>, asked: &mut u64, c: &fast_core::browse::Clash, done: u64, total: u64) -> Option<fast_core::browse::Choice> {
        use fast_core::browse::Choice;
        use fast_core::transfer::{Conflict, Resolution};
        let from = |c: Conflict| match c {
            Conflict::Skip => Choice::Skip,
            Conflict::Overwrite => Choice::Replace,
            Conflict::KeepBoth | Conflict::Ask => Choice::KeepBoth,
        };
        let chosen = if policy != Conflict::Ask {
            Some(from(policy))
        } else if let Some(c) = *sticky {
            Some(c)
        } else {
            *asked += 1;
            let info = fast_core::transfer::ConflictInfo {
                name: c.name.clone(),
                src: c.src.clone(),
                dst: c.dst.to_string_lossy().into_owned(),
                src_size: c.src_size,
                dst_size: c.dst_size,
                src_modified: None,
                dst_modified: c.dst_modified,
                number: *asked,
            };
            *self.ask.answer.lock() = None;
            let mut last_sent: Option<Instant> = None;
            loop {
                if self.cancelled() {
                    return None;
                }
                if let Some((r, all)) = self.ask.answer.lock().take() {
                    let choice = match r {
                        Resolution::Skip => Choice::Skip,
                        Resolution::Replace => Choice::Replace,
                        Resolution::KeepBoth => Choice::KeepBoth,
                    };
                    if all {
                        *sticky = Some(choice);
                    }
                    break Some(choice);
                }
                // Say it again now and then: the app only shows one question at a time and retries.
                if last_sent.map_or(true, |t| t.elapsed() >= Duration::from_millis(300)) {
                    last_sent = Some(Instant::now());
                    self.emit_with(DeletePhase::Running, (done, total), (0, 0), &c.name, "", &[], Some(info.clone()));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        };
        if chosen == Some(Choice::Skip) {
            self.skipped.fetch_add(1, Ordering::Relaxed);
        }
        chosen
    }

    fn scanning(&self, found: u64) {
        self.emit(DeletePhase::Scanning, (0, found), (0, 0), "", "", &[]);
    }

    /// At most ten updates a second.
    fn running(&self, items: (u64, u64), bytes: (u64, u64), current: &str) {
        let mut last = self.last.lock();
        if last.elapsed() >= Duration::from_millis(100) {
            *last = Instant::now();
            drop(last);
            self.emit(DeletePhase::Running, items, bytes, current, "", &[]);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

#[derive(Default)]
struct Outcome {
    /// What was made (for "open": the downloaded file).
    result: String,
    items: u64,
    bytes: u64,
    failed: Vec<(String, String)>,
    /// Local folders whose listing changed.
    local_dirs: Vec<PathBuf>,
}

fn spawn_job(app: AppHandle, st: Shared, action: &'static str, selected: u64, first_name: String, body: impl FnOnce(&Reporter) -> CmdResult<Outcome> + Send + 'static) -> CmdResult<u64> {
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    let ask = Arc::new(crate::state::JobAsk::default());
    st.job_asks.lock().insert(id, ask.clone());
    let reporter = Reporter { app: app.clone(), id, action, selected, first_name, started: Instant::now(), cancel, last: parking_lot::Mutex::new(Instant::now() - Duration::from_secs(1)), ask, skipped: AtomicU64::new(0) };
    std::thread::Builder::new()
        .name(format!("remote-{id}"))
        .spawn(move || {
            reporter.scanning(0);
            let outcome = body(&reporter);
            let cancelled = reporter.cancelled();
            let mut dirs = Vec::new();
            match outcome {
                Ok(o) => {
                    dirs = o.local_dirs.clone();
                    let phase = if cancelled { DeletePhase::Cancelled } else { DeletePhase::Done };
                    reporter.emit(phase, (o.items, o.items), (o.bytes, o.bytes), "", &o.result, &o.failed);
                }
                Err(_) if cancelled => reporter.emit(DeletePhase::Cancelled, (0, 0), (0, 0), "", "", &[]),
                Err(e) => reporter.emit(DeletePhase::Failed, (0, 0), (0, 0), "", "", &[(String::new(), e)]),
            }
            st.deletes.lock().remove(&id);
            st.job_asks.lock().remove(&id);
            if !dirs.is_empty() {
                crate::commands::fs_changed(&app, &st, dirs);
            }
        })
        .map_err(err)?;
    Ok(id)
}

fn base_name(url_or_path: &str) -> String {
    url_or_path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
}

/// Whether something is at `dir/name` on the server.
fn remote_taken(s: &mut dyn Session, dir: &str, name: &str) -> bool {
    s.exists(&format!("{}/{}", dir.trim_end_matches('/'), name)).unwrap_or(false)
}

/// Find out whether a remote item is a folder, and its size, by listing its parent.
fn stat_remote(s: &mut dyn Session, loc: &Location) -> fast_core::Result<(bool, u64)> {
    let Some(parent) = loc.parent() else { return Ok((true, 0)) };
    let name = loc.name().to_string();
    Ok(s.list(&parent.path)?.into_iter().find(|e| e.name == name).map(|e| (e.is_dir, e.size)).unwrap_or((false, 0)))
}

/// Copy or move between this computer and servers, or between servers.
/// With `open`, one file is fetched to a temporary place and `result` names it.
#[tauri::command]
pub async fn remote_copy_start(app: AppHandle, state: State<'_, Shared>, sources: Vec<String>, dest: String, mv: bool, open: bool) -> CmdResult<u64> {
    let st = state.inner().clone();
    if sources.is_empty() {
        return Err("Nothing to copy.".into());
    }
    let src_remote = remote::is_remote(&sources[0]);
    if sources.iter().any(|s| remote::is_remote(s) != src_remote) {
        return Err("Items from this computer and from a server cannot be copied together.".into());
    }
    let dest_remote = remote::is_remote(&dest);
    let action: &'static str = if open { "open" } else if dest_remote { "upload" } else { "download" };
    let first = base_name(&sources[0]);
    let n = sources.len() as u64;
    let st2 = st.clone();
    spawn_job(app, st, action, n, first, move |rep| {
        let st = st2;
        let done = AtomicU64::new(0);
        let total = AtomicU64::new(0);
        let items = AtomicU64::new(0);
        let current = parking_lot::Mutex::new(String::new());
        let on_bytes = |n: u64| {
            let d = done.fetch_add(n, Ordering::Relaxed) + n;
            rep.running((items.load(Ordering::Relaxed), 0), (d, total.load(Ordering::Relaxed)), &current.lock());
        };
        let ctl = Control { cancel: &rep.cancel, on_bytes: &on_bytes };
        let mut out = Outcome::default();
        let mut tally = Tally::default();

        // Where files from a server land first when they have to pass through this computer.
        let stage: Option<PathBuf> = if open || (src_remote && dest_remote) {
            let d = st.paths.remote_tmp.join(format!("job-{}", rep.id));
            std::fs::create_dir_all(&d).map_err(err)?;
            Some(d)
        } else {
            None
        };

        let result = (|| -> CmdResult<()> {
            match (src_remote, dest_remote) {
                // This computer -> server
                (false, true) => {
                    let dloc = Location::parse(&dest).map_err(err)?;
                    let mut lease = Lease::new(&st, &dloc.key(), true)?;
                    let paths: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
                    let (mut files, mut bytes) = (0, 0);
                    for p in &paths {
                        let (f, b) = remote::measure_local(p);
                        files += f;
                        bytes += b;
                    }
                    total.store(bytes, Ordering::Relaxed);
                    for p in &paths {
                        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        *current.lock() = name.clone();
                        let unique = remote::free_name(&name, |n| remote_taken(lease.get(), &dloc.path, n));
                        let before = tally.failed.len();
                        remote::upload_tree_as(lease.get(), p, &dloc.path, &unique, false, &ctl, &mut tally).map_err(err)?;
                        if mv && tally.failed.len() == before {
                            let _ = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                            if let Some(parent) = p.parent() {
                                out.local_dirs.push(parent.to_path_buf());
                            }
                        }
                    }
                    let _ = files;
                    out.result = dloc.url();
                }
                // Server -> this computer (or a temporary place, to open)
                (true, false) => {
                    let into: PathBuf = stage.clone().unwrap_or_else(|| PathBuf::from(&dest));
                    download_all(&st, rep, &sources, &into, mv, &ctl, &total, &current, &mut tally, &mut out)?;
                    if open {
                        let first = remote::safe_name(&base_name(&sources[0]));
                        out.result = into.join(first).to_string_lossy().into_owned();
                    } else {
                        out.result = dest.clone();
                        out.local_dirs.push(PathBuf::from(&dest));
                    }
                }
                // Server -> server
                (true, true) => {
                    let dloc = Location::parse(&dest).map_err(err)?;
                    let same = Location::parse(&sources[0]).map_err(err)?.key() == dloc.key();
                    if mv && same && sources.iter().all(|s| Location::parse(s).map(|l| l.key() == dloc.key()).unwrap_or(false)) {
                        // On one server a move is just a rename.
                        let mut lease = Lease::new(&st, &dloc.key(), false)?;
                        for s in &sources {
                            let sloc = Location::parse(s).map_err(err)?;
                            let name = sloc.name().to_string();
                            let unique = remote::free_name(&name, |n| remote_taken(lease.get(), &dloc.path, n));
                            if let Err(e) = lease.get().rename(&sloc.path, &format!("{}/{}", dloc.path.trim_end_matches('/'), unique)) {
                                tally.failed.push((s.clone(), e.to_string()));
                            } else {
                                tally.files += 1;
                            }
                        }
                        out.result = dloc.url();
                    } else {
                        let tmp = stage.clone().expect("staging folder");
                        download_all(&st, rep, &sources, &tmp, false, &ctl, &total, &current, &mut tally, &mut out)?;
                        let mut lease = Lease::new(&st, &dloc.key(), true)?;
                        for s in &sources {
                            let name = remote::safe_name(&base_name(s));
                            let p = tmp.join(&name);
                            if !p.exists() {
                                continue;
                            }
                            let shown = name.to_string_lossy().into_owned();
                            *current.lock() = shown.clone();
                            let unique = remote::free_name(&shown, |n| remote_taken(lease.get(), &dloc.path, n));
                            remote::upload_tree_as(lease.get(), &p, &dloc.path, &unique, false, &ctl, &mut tally).map_err(err)?;
                        }
                        if mv && tally.failed.is_empty() {
                            for s in &sources {
                                let sloc = Location::parse(s).map_err(err)?;
                                let mut sl = Lease::new(&st, &sloc.key(), false)?;
                                let (is_dir, _) = stat_remote(sl.get(), &sloc).map_err(err)?;
                                remote::remove_tree(sl.get(), &sloc.path, is_dir, &ctl).map_err(err)?;
                            }
                        }
                        out.result = dloc.url();
                    }
                }
                (false, false) => return Err("Nothing to do on a server.".into()),
            }
            Ok(())
        })();

        // Staging is only needed while copying between servers; a file fetched
        // to be opened stays until the cache is cleaned.
        if let (Some(dir), false) = (&stage, open) {
            let _ = std::fs::remove_dir_all(dir);
        }
        result?;
        out.items = tally.files;
        out.bytes = done.load(Ordering::Relaxed);
        out.failed = tally.failed;
        Ok(out)
    })
}

#[allow(clippy::too_many_arguments)]
fn download_all(st: &Shared, rep: &Reporter, sources: &[String], into: &Path, mv: bool, ctl: &Control, total: &AtomicU64, current: &parking_lot::Mutex<String>, tally: &mut Tally, out: &mut Outcome) -> CmdResult<()> {
    let _ = out;
    let mut items: Vec<(Location, bool, u64)> = Vec::new();
    for s in sources {
        let loc = Location::parse(s).map_err(err)?;
        let mut lease = Lease::new(st, &loc.key(), false)?;
        let (is_dir, size) = stat_remote(lease.get(), &loc).map_err(err)?;
        items.push((loc, is_dir, size));
    }
    // Count what is coming, so the bar has an end.
    let mut bytes = 0;
    let mut files = 0;
    for (loc, is_dir, size) in &items {
        let mut lease = Lease::new(st, &loc.key(), false)?;
        let (f, b) = remote::measure_tree(lease.get(), &loc.path, *is_dir, *size, ctl).map_err(err)?;
        files += f;
        bytes += b;
        rep.scanning(files);
    }
    total.fetch_add(bytes, Ordering::Relaxed);
    for (loc, is_dir, _) in &items {
        let name = loc.name().to_string();
        *current.lock() = name.clone();
        let mut lease = Lease::new(st, &loc.key(), true)?;
        let unique = remote::free_name(&name, |n| into.join(remote::safe_name(n)).exists());
        let before = tally.failed.len();
        remote::download_tree_as(lease.get(), &loc.path, *is_dir, into, &unique, false, ctl, tally).map_err(err)?;
        if mv && tally.failed.len() == before {
            remote::remove_tree(lease.get(), &loc.path, *is_dir, ctl).map_err(err)?;
        }
    }
    Ok(())
}

/// Delete files and folders on a server.
#[tauri::command]
pub async fn remote_remove_start(app: AppHandle, state: State<'_, Shared>, paths: Vec<String>) -> CmdResult<u64> {
    let st = state.inner().clone();
    if paths.is_empty() {
        return Err("Nothing to delete.".into());
    }
    let first = base_name(&paths[0]);
    let n = paths.len() as u64;
    let st2 = st.clone();
    spawn_job(app, st, "remote-delete", n, first, move |rep| {
        let st = st2;
        let items = AtomicU64::new(0);
        let on_bytes = |_: u64| {};
        let ctl = Control { cancel: &rep.cancel, on_bytes: &on_bytes };
        let mut out = Outcome::default();
        let mut locs = Vec::new();
        for p in &paths {
            let loc = Location::parse(p).map_err(err)?;
            let mut lease = Lease::new(&st, &loc.key(), false)?;
            let (is_dir, size) = stat_remote(lease.get(), &loc).map_err(err)?;
            let (files, _) = remote::measure_tree(lease.get(), &loc.path, is_dir, size, &ctl).map_err(err)?;
            rep.scanning(items.fetch_add(files.max(1), Ordering::Relaxed) + files.max(1));
            locs.push((loc, is_dir));
        }
        let total = items.load(Ordering::Relaxed);
        let mut done = 0;
        for (loc, is_dir) in locs {
            rep.running((done, total), (0, 0), loc.name());
            let mut lease = Lease::new(&st, &loc.key(), false)?;
            match remote::remove_tree(lease.get(), &loc.path, is_dir, &ctl) {
                Ok(n) => done += n,
                Err(_) if rep.cancelled() => break,
                Err(e) => out.failed.push((loc.url(), e.to_string())),
            }
        }
        out.items = done;
        Ok(out)
    })
}

// ---------------------------------------------------------------------------
// Archives opened like folders (read-only). See fast_core::browse.
// ---------------------------------------------------------------------------

fn archive_loc(st: &Shared, url: &str) -> CmdResult<fast_core::browse::Loc> {
    let loc = fast_core::browse::Loc::parse(url).ok_or_else(|| "This is not a place inside an archive.".to_string())?;
    crate::commands::guard(st, &loc.archive.to_string_lossy())?;
    Ok(loc)
}

/// The contents of a folder inside an archive.
pub fn archive_list(st: &Shared, url: &str) -> CmdResult<Listing> {
    let loc = archive_loc(st, url)?;
    let show_hidden = st.settings.read().general.show_hidden;
    let items = fast_core::browse::index(&loc.archive).map_err(err)?;
    if !loc.inner.is_empty() && fast_core::browse::find(&items, &loc.inner).map(|i| !i.is_dir).unwrap_or(true) {
        return Err(format!("There is no folder \"{}\" in {}.", loc.inner, loc.name()));
    }
    let entries = fast_core::browse::entries_in(&items, &loc.inner, show_hidden);
    Ok(Listing { path: loc.url(), parent: loc.parent(), entries: Arc::new(entries), cached: false })
}

/// What the details pane shows for something inside an archive.
pub fn archive_details(st: &Shared, url: &str) -> CmdResult<fast_core::details::Details> {
    let loc = archive_loc(st, url)?;
    let items = fast_core::browse::index(&loc.archive).map_err(err)?;
    let found = fast_core::browse::find(&items, &loc.inner).ok_or_else(|| "Not found in the archive.".to_string())?;
    let name = loc.name();
    let (size, count) = if found.is_dir { fast_core::browse::size_under(&items, &loc.inner) } else { (found.size, 0) };
    Ok(fast_core::details::Details {
        extension: (!found.is_dir && !loc.inner.is_empty()).then(|| name.rsplit_once('.').map(|(_, e)| e.to_lowercase())).flatten(),
        name,
        path: loc.url(),
        parent: loc.parent(),
        kind: if found.is_dir { "folder" } else { "file" }.to_string(),
        size,
        size_on_disk: None,
        created: None,
        modified: found.modified,
        accessed: None,
        readonly: true,
        hidden: false,
        mime: None,
        symlink_target: None,
        unix: None,
        attributes: Vec::new(),
        image: None,
        item_count: found.is_dir.then_some(count),
        volume: None,
        media: Vec::new(),
    })
}

/// Unpack items from inside archives into a folder (`open` false), or one file into
/// a temporary place for opening (`open` true; `result` names it). A file that is already
/// in the folder is handled the way "When a file already exists" (File transfers) says:
/// asked about one by one, replaced, skipped or kept under a second name.
#[tauri::command]
pub async fn archive_copy_start(app: AppHandle, state: State<'_, Shared>, sources: Vec<String>, dest: String, open: bool, remove: Option<bool>) -> CmdResult<u64> {
    let remove = remove.unwrap_or(false) && !open;
    let st = state.inner().clone();
    if sources.is_empty() {
        return Err("Nothing to copy.".into());
    }
    let locs: Vec<fast_core::browse::Loc> = sources.iter().map(|s| archive_loc(&st, s)).collect::<CmdResult<_>>()?;
    let first = locs[0].name();
    let first_target = locs[0].inner.rsplit('/').next().unwrap_or("").to_string();
    let action: &'static str = if open { "open" } else { "unpack" };
    let n = locs.len() as u64;
    let policy: fast_core::transfer::Conflict = serde_json::from_value(serde_json::Value::String(st.settings.read().transfer.conflict.clone())).unwrap_or(fast_core::transfer::Conflict::Ask);
    let st2 = st.clone();
    spawn_job(app, st, action, n, first, move |rep| {
        let into = if open { st2.paths.remote_tmp.join(format!("archive-{}", rep.id)) } else { PathBuf::from(&dest) };
        std::fs::create_dir_all(&into).map_err(err)?;
        let mut groups: Vec<(PathBuf, Vec<String>)> = Vec::new();
        for l in locs {
            match groups.iter_mut().find(|(a, _)| *a == l.archive) {
                Some((_, v)) => v.push(l.inner),
                None => groups.push((l.archive, vec![l.inner])),
            }
        }
        let mut out = Outcome::default();
        // "Replace all" / "Skip all" in an answer holds for the rest of the job.
        let mut sticky: Option<fast_core::browse::Choice> = None;
        let mut asked = 0u64;
        for (archive, inners) in groups {
            let seen = std::cell::Cell::new((0u64, 0u64));
            let mut report = |done: u64, total: u64, current: &str| {
                seen.set((done, total));
                rep.running((done, total), (0, 0), current)
            };
            let mut decide = |c: &fast_core::browse::Clash| {
                let (done, total) = seen.get();
                rep.decide(policy, &mut sticky, &mut asked, c, done, total)
            };
            let r = fast_core::browse::extract_items_with(&archive, &inners, &into, &rep.cancel, &mut report, &mut decide).map_err(err)?;
            out.items += r.files;
            if r.refused > 0 {
                out.failed.push((archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), format!("{} item(s) were skipped (links, or names that lead outside the folder).", r.refused)));
            }
            if remove {
                // A move: what was copied out leaves the archive, but only when all of it arrived.
                let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if r.existing > 0 || r.refused > 0 {
                    out.failed.push((name, "Copied, but not removed from the archive because some items were not copied (skipped or left alone).".into()));
                } else if let Err(e) = fast_core::edit::delete(&archive, &inners, &rep.cancel) {
                    out.failed.push((name, format!("Copied, but could not be removed from the archive. {e}")));
                }
            }
        }
        if open {
            out.result = into.join(&first_target).to_string_lossy().into_owned();
        } else {
            out.result = dest.clone();
            out.local_dirs.push(PathBuf::from(&dest));
        }
        Ok(out)
    })
}

/// Put things into an archive, or move them around inside it: from disk, from the
/// same archive (move or copy) and from other archives. `op` is "copy" or "move".
/// A move takes the originals away once they are safely in.
#[tauri::command]
pub async fn archive_paste_start(app: AppHandle, state: State<'_, Shared>, sources: Vec<String>, dest: String, op: String) -> CmdResult<u64> {
    let st = state.inner().clone();
    let dest = archive_loc(&st, &dest)?;
    fast_core::edit::check_editable(&dest.archive).map_err(err)?;
    let moving = op == "move";
    let mut local: Vec<PathBuf> = Vec::new();
    let mut foreign: Vec<fast_core::browse::Loc> = Vec::new();
    let mut same: Vec<fast_core::browse::Loc> = Vec::new();
    for src in &sources {
        if fast_core::browse::is_loc(src) {
            let l = archive_loc(&st, src)?;
            if moving && l.archive == dest.archive {
                same.push(l);
            } else {
                foreign.push(l);
            }
        } else {
            crate::commands::guard(&st, src)?;
            local.push(PathBuf::from(src));
        }
    }
    let first = sources.first().map(|s| s.rsplit(['/', '\\']).next().unwrap_or("").to_string()).unwrap_or_default();
    let n = sources.len() as u64;
    let st2 = st.clone();
    spawn_job(app, st, "pack", n, first, move |rep| {
        let mut out = Outcome::default();
        let mut report = |done: u64, total: u64, current: &str| rep.running((done, total), (0, 0), current);
        // Moves inside the same archive are only renames of entries.
        if !same.is_empty() {
            let inners: Vec<String> = same.iter().map(|l| l.inner.clone()).collect();
            let moved = fast_core::edit::move_within(&dest.archive, &inners, &dest.inner, &rep.cancel, &mut report).map_err(err)?;
            out.items += moved.len() as u64;
        }
        // Everything else is first laid out on disk, then added.
        let stage = st2.paths.remote_tmp.join(format!("stage-{}", rep.id));
        let staged: Result<Vec<PathBuf>, String> = (|| {
            let mut paths = local.clone();
            for (i, l) in foreign.iter().enumerate() {
                let into = stage.join(i.to_string());
                fast_core::browse::extract_items(&l.archive, &[l.inner.clone()], &into, &rep.cancel, &mut report).map_err(err)?;
                paths.push(into.join(l.inner.rsplit('/').next().unwrap_or("")));
            }
            Ok(paths)
        })();
        let result = staged.and_then(|paths| {
            if paths.is_empty() {
                return Ok(0);
            }
            fast_core::edit::add(&dest.archive, &dest.inner, &paths, &rep.cancel, &mut report).map_err(err)
        });
        let _ = std::fs::remove_dir_all(&stage);
        out.items += result?;
        if moving {
            // The originals go once they are in: files on disk, and items of other archives.
            for p in &local {
                let r = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
                if let Err(e) = r {
                    out.failed.push((p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), format!("Added, but could not be removed from its folder: {e}")));
                } else {
                    out.local_dirs.extend(p.parent().map(|d| d.to_path_buf()));
                }
            }
            let mut by_archive: Vec<(PathBuf, Vec<String>)> = Vec::new();
            for l in &foreign {
                match by_archive.iter_mut().find(|(a, _)| *a == l.archive) {
                    Some((_, v)) => v.push(l.inner.clone()),
                    None => by_archive.push((l.archive.clone(), vec![l.inner.clone()])),
                }
            }
            for (archive, inners) in by_archive {
                if let Err(e) = fast_core::edit::delete(&archive, &inners, &rep.cancel) {
                    out.failed.push((archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), format!("Copied in, but could not be removed from where it was. {e}")));
                }
            }
        }
        out.result = dest.url();
        Ok(out)
    })
}

fn one_archive(st: &Shared, locs: &[String]) -> CmdResult<(PathBuf, Vec<String>)> {
    let mut archive: Option<PathBuf> = None;
    let mut inners = Vec::new();
    for l in locs {
        let loc = archive_loc(st, l)?;
        match &archive {
            Some(a) if *a != loc.archive => return Err("Delete from one archive at a time.".into()),
            _ => archive = Some(loc.archive.clone()),
        }
        inners.push(loc.inner);
    }
    Ok((archive.ok_or_else(|| "Nothing to delete.".to_string())?, inners))
}

/// Delete items from inside an archive. This cannot be undone.
#[tauri::command]
pub async fn archive_delete(state: State<'_, Shared>, locs: Vec<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    blocking(move || {
        let (archive, inners) = one_archive(&st, &locs)?;
        fast_core::edit::delete(&archive, &inners, &std::sync::atomic::AtomicBool::new(false)).map_err(err)
    })
    .await
}

/// Rename an item inside an archive. Returns its new location.
#[tauri::command]
pub async fn archive_rename(state: State<'_, Shared>, loc: String, new_name: String) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let l = archive_loc(&st, &loc)?;
        let to = fast_core::edit::rename(&l.archive, &l.inner, &new_name, &std::sync::atomic::AtomicBool::new(false)).map_err(err)?;
        Ok(fast_core::browse::loc_url(&l.archive, &to))
    })
    .await
}

/// Make a folder or an empty file inside an archive. Returns its location.
#[tauri::command]
pub async fn archive_create(state: State<'_, Shared>, dir: String, name: String, folder: bool) -> CmdResult<String> {
    let st = state.inner().clone();
    blocking(move || {
        let l = archive_loc(&st, &dir)?;
        let to = fast_core::edit::create(&l.archive, &l.inner, &name, folder, &std::sync::atomic::AtomicBool::new(false)).map_err(err)?;
        Ok(fast_core::browse::loc_url(&l.archive, &to))
    })
    .await
}

/// Unpack items from archives into a staging folder, so that real files can be put on
/// the system clipboard for other programs. Returns the staged paths, one per source.
/// The previous staging is removed.
#[tauri::command]
pub async fn archive_stage(state: State<'_, Shared>, sources: Vec<String>) -> CmdResult<Vec<String>> {
    let st = state.inner().clone();
    blocking(move || {
        let root = st.paths.remote_tmp.clone();
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("clip-") {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let stage = root.join(format!("clip-{stamp}"));
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut out = Vec::new();
        for (i, s) in sources.iter().enumerate() {
            let l = archive_loc(&st, s)?;
            let into = stage.join(i.to_string());
            fast_core::browse::extract_items(&l.archive, &[l.inner.clone()], &into, &cancel, &mut |_, _, _| {}).map_err(err)?;
            out.push(into.join(l.inner.rsplit('/').next().unwrap_or("")).to_string_lossy().into_owned());
        }
        Ok(out)
    })
    .await
}
