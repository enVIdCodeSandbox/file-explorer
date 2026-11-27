//! The built-in terminal, app side: starts and talks to the terminal host (see
//! `fast_core::terminal`), passes its output to the window, and keeps a copy of what was
//! printed when the app closes so the next start can show it again.

use crate::state::Shared;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use fast_core::remote::ssh::{open_terminal as open_ssh_terminal, SshTerminal};
use fast_core::remote::{Location, Protocol};
use fast_core::terminal::{client, host, popout, proto::RING_BYTES, Client, Event, Request, SessionInfo};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

struct SshSession {
    info: SessionInfo,
    ring: VecDeque<u8>,
    term: Option<SshTerminal>,
}

/// The connection to the host, made when the terminal is first used, plus active SSH PTY shells.
#[derive(Default)]
pub struct Bridge {
    client: Mutex<Option<Arc<Client>>>,
    /// Held while a connection is being made, so two requests that arrive together make one connection, not two
    /// (each connection passes on the shell's output, so two would print everything twice).
    connecting: Mutex<()>,
    ssh: Mutex<HashMap<String, Arc<Mutex<SshSession>>>>,
    next_ssh: AtomicU64,
}

#[derive(Serialize, Clone)]
struct OutEvent {
    sid: String,
    /// Base64.
    data: String,
}

#[derive(Serialize, Clone)]
struct ExitEvent {
    sid: String,
    code: Option<i32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInfo {
    /// Whether the app itself runs as administrator (Windows) or root.
    app_elevated: bool,
    os: &'static str,
    /// The Windows build number (0 elsewhere): it decides how the screen must treat resizing.
    windows_build: u32,
    /// The shell used when none is chosen.
    default_shell: String,
}

#[derive(Serialize)]
pub struct Attached {
    /// Base64 of everything the session has printed (up to what the host keeps).
    data: String,
    info: SessionInfo,
}

fn dir(st: &Shared) -> PathBuf {
    st.paths.terminal.clone()
}

fn setting_bool(st: &Shared, key: &str, default: bool) -> bool {
    st.settings.read().advanced.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// A command line from the settings, split at spaces; "double quotes" keep words together.
pub fn split_command(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for ch in text.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Connect to the host, starting it when it is not running.
fn connect(app: &AppHandle, st: &Shared) -> CmdResult<Arc<Client>> {
    let current = || st.terminal.client.lock().as_ref().filter(|c| !c.is_closed()).cloned();
    if let Some(c) = current() {
        return Ok(c);
    }
    let _one_at_a_time = st.terminal.connecting.lock();
    // Another request may have connected while this one waited.
    if let Some(c) = current() {
        return Ok(c);
    }
    let dir = dir(st);
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    client::ensure_host(&dir, || client::launch_host(&exe, &dir)).map_err(|e| format!("The terminal could not start: {e}"))?;
    let handle = app.clone();
    let closing = app.clone();
    let c = Client::connect(
        &dir,
        move |e: Event| match e.ev.as_str() {
            "out" => {
                let _ = handle.emit("term-out", OutEvent { sid: e.sid, data: e.data.unwrap_or_default() });
            }
            "exit" => {
                let _ = handle.emit("term-exit", ExitEvent { sid: e.sid, code: e.code });
            }
            _ => {
                let _ = handle.emit("term-sessions", ());
            }
        },
        move || {
            let _ = closing.emit("term-sessions", ());
        },
    )
    .map_err(|e| format!("The terminal could not connect: {e}"))?;
    let c = Arc::new(c);
    *st.terminal.client.lock() = Some(c.clone());
    Ok(c)
}

fn call(app: &AppHandle, st: &Shared, req: Request) -> CmdResult<serde_json::Value> {
    connect(app, st)?.call(req)
}

fn sessions(v: serde_json::Value) -> Vec<SessionInfo> {
    serde_json::from_value(v).unwrap_or_default()
}

#[tauri::command]
pub async fn term_info(state: State<'_, Shared>) -> CmdResult<TerminalInfo> {
    let st = state.inner().clone();
    let shell = st.settings.read().advanced.get("terminal.shell").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
    Ok(TerminalInfo {
        app_elevated: host::is_elevated(),
        os: fast_core::platform::os_name(),
        windows_build: fast_core::platform::windows_build(),
        default_shell: split_command(&shell).first().cloned().unwrap_or_else(|| host::default_shell().0),
    })
}

fn ssh_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn is_ssh_sid(sid: &str) -> bool {
    sid.starts_with("ssh-")
}

fn ssh_sessions(st: &Shared) -> Vec<SessionInfo> {
    st.terminal.ssh.lock().values().map(|e| e.lock().info.clone()).collect()
}

fn ssh_spawn(app: &AppHandle, st: &Shared, cwd: &str, cols: u16, rows: u16, title: Option<String>) -> CmdResult<String> {
    let loc = Location::parse(cwd).map_err(|e| e.to_string())?;
    if loc.protocol != Protocol::Ssh {
        return Err("Only ssh:// folders can open an SSH terminal.".into());
    }
    let key = loc.key();
    let server = crate::remote::server_for(st, &key).ok_or_else(|| format!("needs-login:{}", loc.url()))?;
    let sid = format!("ssh-{}", st.terminal.next_ssh.fetch_add(1, Ordering::Relaxed) + 1);
    let default_title = if loc.user.is_empty() { loc.host.clone() } else { format!("{}@{}", loc.user, loc.host) };
    let label = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or(default_title);
    let entry = Arc::new(Mutex::new(SshSession {
        info: SessionInfo {
            sid: sid.clone(),
            title: label,
            cwd: loc.url(),
            program: "ssh".into(),
            cols: cols.max(2),
            rows: rows.max(2),
            alive: true,
            exit: None,
            elevated: false,
            created: ssh_now_ms(),
            bytes: 0,
        },
        ring: VecDeque::new(),
        term: None,
    }));
    st.terminal.ssh.lock().insert(sid.clone(), entry.clone());

    let out_app = app.clone();
    let out_sid = sid.clone();
    let out_entry = entry.clone();
    let exit_app = app.clone();
    let exit_sid = sid.clone();
    let exit_entry = entry.clone();

    let term = match open_ssh_terminal(
        &server,
        &st.paths.known_hosts,
        &loc.path,
        cols,
        rows,
        move |bytes| {
            {
                let mut s = out_entry.lock();
                s.ring.extend(bytes);
                while s.ring.len() > RING_BYTES {
                    let drop_n = s.ring.len() - RING_BYTES;
                    s.ring.drain(..drop_n);
                }
                s.info.bytes += bytes.len() as u64;
            }
            let _ = out_app.emit("term-out", OutEvent { sid: out_sid.clone(), data: B64.encode(bytes) });
        },
        move |code| {
            {
                let mut s = exit_entry.lock();
                s.info.alive = false;
                s.info.exit = Some(code.unwrap_or(0));
                s.term = None;
            }
            let _ = exit_app.emit("term-exit", ExitEvent { sid: exit_sid, code });
            let _ = exit_app.emit("term-sessions", ());
        },
    ) {
        Ok(t) => t,
        Err(e) => {
            st.terminal.ssh.lock().remove(&sid);
            return Err(e.to_string());
        }
    };
    entry.lock().term = Some(term);
    let _ = app.emit("term-sessions", ());
    Ok(sid)
}

/// Connect (starting the host if needed) and list the sessions that exist.
#[tauri::command]
pub async fn term_list(app: AppHandle, state: State<'_, Shared>) -> CmdResult<Vec<SessionInfo>> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut list = call(&app, &st, Request::List).map(sessions)?;
        list.extend(ssh_sessions(&st));
        Ok(list)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn term_spawn(app: AppHandle, state: State<'_, Shared>, cwd: String, cols: u16, rows: u16, title: Option<String>, elevated: bool) -> CmdResult<String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if cwd.len() >= 6 && cwd[..6].eq_ignore_ascii_case("ssh://") {
            return ssh_spawn(&app, &st, &cwd, cols, rows, title);
        }
        let configured = st.settings.read().advanced.get("terminal.shell").and_then(|v| v.as_str().map(String::from)).unwrap_or_default();
        let mut parts = split_command(&configured);
        let program = if parts.is_empty() { None } else { Some(parts.remove(0)) };
        let reply = call(&app, &st, Request::Spawn { cwd, program, args: parts, cols, rows, title: title.unwrap_or_default(), elevated })?;
        reply["sid"].as_str().map(String::from).ok_or_else(|| "The terminal host gave no session.".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Start receiving a session's output: returns what it has printed so far; new output follows as `term-out` events.
#[tauri::command]
pub async fn term_attach(app: AppHandle, state: State<'_, Shared>, sid: String) -> CmdResult<Attached> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if is_ssh_sid(&sid) {
            let entry = st.terminal.ssh.lock().get(&sid).cloned().ok_or_else(|| "Unknown SSH terminal session.".to_string())?;
            let mut s = entry.lock();
            let data = B64.encode(s.ring.make_contiguous());
            return Ok(Attached { data, info: s.info.clone() });
        }
        let v = call(&app, &st, Request::Snapshot { sid })?;
        let info: SessionInfo = serde_json::from_value(v["info"].clone()).map_err(|e| e.to_string())?;
        Ok(Attached { data: v["data"].as_str().unwrap_or_default().to_string(), info })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn term_input(app: AppHandle, state: State<'_, Shared>, sid: String, data: String) -> CmdResult<()> {
    if is_ssh_sid(&sid) {
        let bytes = B64.decode(&data).map_err(|e| e.to_string())?;
        if let Some(entry) = state.inner().terminal.ssh.lock().get(&sid).cloned() {
            if let Some(term) = &entry.lock().term {
                term.write(bytes);
            }
        }
        return Ok(());
    }
    connect(&app, state.inner())?.send(Request::Input { sid, data })
}

#[tauri::command]
pub fn term_resize(app: AppHandle, state: State<'_, Shared>, sid: String, cols: u16, rows: u16) -> CmdResult<()> {
    if is_ssh_sid(&sid) {
        if let Some(entry) = state.inner().terminal.ssh.lock().get(&sid).cloned() {
            let mut s = entry.lock();
            s.info.cols = cols.max(2);
            s.info.rows = rows.max(2);
            if let Some(term) = &s.term {
                term.resize(cols, rows);
            }
        }
        return Ok(());
    }
    connect(&app, state.inner())?.send(Request::Resize { sid, cols, rows })
}

/// kill, remove, clear, title or cwd: the small requests.
#[tauri::command]
pub async fn term_do(app: AppHandle, state: State<'_, Shared>, action: String, sid: String, title: Option<String>) -> CmdResult<()> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if is_ssh_sid(&sid) {
            if action == "remove" {
                if let Some(entry) = st.terminal.ssh.lock().remove(&sid) {
                    if let Some(term) = entry.lock().term.take() {
                        term.close();
                    }
                }
                return Ok(());
            }
            let Some(entry) = st.terminal.ssh.lock().get(&sid).cloned() else {
                return Ok(());
            };
            let mut s = entry.lock();
            match action.as_str() {
                "kill" => {
                    if let Some(term) = s.term.take() {
                        term.close();
                    }
                    s.info.alive = false;
                }
                "clear" => s.ring.clear(),
                "title" => s.info.title = title.unwrap_or_default(),
                "cwd" => {
                    if let Some(cwd) = title {
                        s.info.cwd = cwd;
                    }
                }
                other => return Err(format!("Unknown terminal action {other}.")),
            }
            return Ok(());
        }
        let req = match action.as_str() {
            "kill" => Request::Kill { sid },
            "remove" => Request::Remove { sid },
            "clear" => Request::Clear { sid },
            "title" => Request::Title { sid, title: title.unwrap_or_default() },
            "cwd" => return Ok(()),
            other => return Err(format!("Unknown terminal action {other}.")),
        };
        call(&app, &st, req).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open a link that was printed in a terminal. Only web links.
#[tauri::command]
pub async fn term_open_url(url: String) -> CmdResult<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Only web links can be opened from the terminal.".into());
    }
    tauri::async_runtime::spawn_blocking(move || open::that_detached(&url).map_err(|e| e.to_string())).await.map_err(|e| e.to_string())?
}

/// Show a session in the system's own terminal app (it keeps running here too).
#[tauri::command]
pub async fn term_popout(state: State<'_, Shared>, sid: String) -> CmdResult<()> {
    if is_ssh_sid(&sid) {
        return Err("SSH terminals run inside FAST File Explorer. Drag the terminal tab out to open it in its own window.".into());
    }
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        popout::open_window(&exe, &dir(&st), &sid).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn log_path(st: &Shared, sid: &str) -> PathBuf {
    st.paths.terminal.join("logs").join(format!("{}.log", sid.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>()))
}

/// What a session printed before the app was closed (base64), or nothing.
#[tauri::command]
pub async fn term_transcript(state: State<'_, Shared>, sid: String, forget: bool) -> CmdResult<Option<String>> {
    let path = log_path(state.inner(), &sid);
    let found = std::fs::read(&path).ok().map(|b| B64.encode(b));
    if forget {
        let _ = std::fs::remove_file(&path);
    }
    Ok(found)
}

/// Delete saved transcripts that no tab refers to.
#[tauri::command]
pub async fn term_sweep_transcripts(state: State<'_, Shared>, keep: Vec<String>) -> CmdResult<()> {
    let folder = state.inner().paths.terminal.join("logs");
    let keep: std::collections::HashSet<String> = keep.into_iter().collect();
    if let Ok(read) = std::fs::read_dir(folder) {
        for f in read.flatten() {
            let stem = f.path().file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            if !keep.contains(&stem) {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    Ok(())
}

/// The app is closing. Keep what each session printed; then either open the sessions in the
/// system's terminal app (they keep running) or end them.
pub fn on_exit(app: &AppHandle) {
    let Some(st) = app.try_state::<Shared>() else { return };
    let st: Shared = st.inner().clone();
    for entry in st.terminal.ssh.lock().drain().map(|(_, e)| e) {
        if let Some(term) = entry.lock().term.take() {
            term.close();
        }
    }
    let client = st.terminal.client.lock().clone();
    // No terminal was used this run, but a host from a crash may still be keeping sessions.
    let client = match client {
        Some(c) if !c.is_closed() => c,
        _ => match client::read_info(&dir(&st)).and_then(|_| Client::connect(&dir(&st), |_| {}, || {}).ok()) {
            Some(c) => Arc::new(c),
            None => return,
        },
    };
    let Ok(list) = client.call(Request::List).map(sessions) else { return };
    let keep_logs = setting_bool(&st, "terminal.restore", true);
    let popout_on_close = setting_bool(&st, "terminal.popOutOnClose", false);
    let _ = std::fs::create_dir_all(st.paths.terminal.join("logs"));
    for s in &list {
        if keep_logs {
            if let Ok(v) = client.call(Request::Snapshot { sid: s.sid.clone() }) {
                if let Some(Ok(bytes)) = v["data"].as_str().map(|d| B64.decode(d)) {
                    let _ = std::fs::write(log_path(&st, &s.sid), bytes);
                }
            }
        }
    }
    if popout_on_close {
        if let Ok(exe) = std::env::current_exe() {
            for s in list.iter().filter(|s| s.alive) {
                let _ = popout::open_window(&exe, &dir(&st), &s.sid);
            }
        }
        // The host stays, with the shells; this app lets go of it.
    } else {
        let _ = client.call(Request::Shutdown);
    }
}

#[cfg(test)]
mod tests {
    use super::split_command;

    #[test]
    fn splits_a_command_line() {
        assert_eq!(split_command("pwsh -NoLogo"), vec!["pwsh", "-NoLogo"]);
        assert_eq!(split_command(r#""C:\Program Files\Git\bin\bash.exe" --login -i"#), vec![r"C:\Program Files\Git\bin\bash.exe", "--login", "-i"]);
        assert!(split_command("   ").is_empty());
    }
}
