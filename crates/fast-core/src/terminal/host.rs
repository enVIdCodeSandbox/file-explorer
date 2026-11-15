//! The terminal host: owns the shells. Runs as a background process of the app
//! (`fast-file-explorer --terminal-host <folder>`), listens on 127.0.0.1 only, and tells clients
//! how to reach it through `terminal-host.json` in the app's data folder.

use super::proto::*;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn info_path(dir: &Path) -> PathBuf {
    dir.join("terminal-host.json")
}

/// With no window connected for this long and shells still running, the host stops them and exits,
/// so a forgotten session does not run for ever.
const ORPHAN_LIMIT: Duration = Duration::from_secs(12 * 3600);

struct Session {
    info: SessionInfo,
    ring: VecDeque<u8>,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    osc_carry: Vec<u8>,
}

struct Conn {
    tx: Sender<String>,
    /// Sessions whose output this connection receives.
    subs: HashSet<String>,
}

#[derive(Default)]
struct State {
    sessions: HashMap<String, Session>,
    order: Vec<String>,
    conns: HashMap<u64, Conn>,
    next_conn: u64,
    next_session: u64,
    /// When the last connection went away (None while one is connected).
    alone_since: Option<Instant>,
    quit: bool,
}

type Shared = Arc<Mutex<State>>;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn line(v: &impl serde::Serialize) -> String {
    let mut s = serde_json::to_string(v).unwrap_or_default();
    s.push('\n');
    s
}

/// Tell every connection that the list of sessions changed.
fn announce(st: &State) {
    let msg = line(&Event { ev: "sessions".into(), sid: String::new(), data: None, code: None });
    for c in st.conns.values() {
        let _ = c.tx.send(msg.clone());
    }
}

/// The shell a person gets when they ask for no particular one.
pub fn default_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        let on_path = |name: &str| {
            std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file())).unwrap_or(false)
        };
        for candidate in ["pwsh.exe", "powershell.exe"] {
            if on_path(candidate) {
                return (candidate.to_string(), vec!["-NoLogo".to_string()]);
            }
        }
        (std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()), vec![])
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty() && Path::new(s).exists()).unwrap_or_else(|| "/bin/sh".into());
        // Terminal.app opens login shells, and so do the others people use on a Mac.
        let args = if cfg!(target_os = "macos") { vec!["-l".to_string()] } else { vec![] };
        (shell, args)
    }
}

fn spawn_session(shared: &Shared, req: &Request) -> Result<String, String> {
    let Request::Spawn { cwd, program, args, cols, rows, title, elevated } = req else { return Err("not a spawn".into()) };
    let size = PtySize { rows: (*rows).max(2), cols: (*cols).max(2), pixel_width: 0, pixel_height: 0 };
    let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;
    let (program_name, program_args) = match program.as_deref().filter(|p| !p.trim().is_empty()) {
        Some(p) => (p.to_string(), args.clone()),
        None => default_shell(),
    };
    // On Linux and macOS, "administrator" is sudo running the shell; it asks for the password inside the terminal.
    let (exe, exe_args) = if *elevated && !cfg!(windows) && !is_elevated() {
        let mut a = vec![];
        if program.as_deref().filter(|p| !p.trim().is_empty()).is_some() {
            a.push(program_name.clone());
            a.extend(program_args.clone());
        } else {
            a.push("-s".to_string());
        }
        ("sudo".to_string(), a)
    } else {
        (program_name.clone(), program_args.clone())
    };
    let mut cmd = CommandBuilder::new(&exe);
    cmd.args(&exe_args);
    if Path::new(cwd).is_dir() {
        cmd.cwd(cwd);
    } else if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        cmd.cwd(home);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("TERM_PROGRAM", "FASTFileExplorer");
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| format!("Could not start {exe}: {e}"))?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let killer = child.clone_killer();

    let sid = {
        let mut st = shared.lock();
        st.next_session += 1;
        let sid = format!("{:x}{:03x}", now_ms() & 0xffff_ffff, st.next_session & 0xfff);
        let base = Path::new(&program_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| program_name.clone());
        st.sessions.insert(
            sid.clone(),
            Session {
                info: SessionInfo {
                    sid: sid.clone(),
                    title: if title.trim().is_empty() { base } else { title.clone() },
                    cwd: cwd.clone(),
                    program: program_name.clone(),
                    cols: size.cols,
                    rows: size.rows,
                    alive: true,
                    exit: None,
                    elevated: *elevated || is_elevated(),
                    created: now_ms(),
                    bytes: 0,
                },
                ring: VecDeque::new(),
                writer: Some(writer),
                master: Some(pair.master),
                killer: Some(killer),
                osc_carry: Vec::new(),
            },
        );
        st.order.push(sid.clone());
        announce(&st);
        sid
    };

    // Output: keep it, and pass it to whoever is attached.
    let s1 = shared.clone();
    let id1 = sid.clone();
    std::thread::Builder::new().name(format!("term-read-{sid}")).spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut st = s1.lock();
            let State { sessions, conns, .. } = &mut *st;
            let Some(s) = sessions.get_mut(&id1) else { break };
            s.ring.extend(&buf[..n]);
            while s.ring.len() > RING_BYTES {
                let drop_n = s.ring.len() - RING_BYTES;
                s.ring.drain(..drop_n);
            }
            s.info.bytes += n as u64;
            let mut changed = false;
            if let Some(c) = osc7_cwd(&mut s.osc_carry, &buf[..n]).filter(|c| *c != s.info.cwd) {
                s.info.cwd = c;
                changed = true;
            }
            let msg = line(&Event { ev: "out".into(), sid: id1.clone(), data: Some(B64.encode(&buf[..n])), code: None });
            for c in conns.values().filter(|c| c.subs.contains(&id1)) {
                let _ = c.tx.send(msg.clone());
            }
            if changed {
                announce(&st);
            }
        }
    }).map_err(|e| e.to_string())?;

    // The end of the shell.
    let s2 = shared.clone();
    let id2 = sid.clone();
    std::thread::Builder::new().name(format!("term-wait-{sid}")).spawn(move || {
        let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
        // Let the last of the output through before the pseudo-terminal is closed.
        std::thread::sleep(Duration::from_millis(250));
        let mut st = s2.lock();
        if let Some(s) = st.sessions.get_mut(&id2) {
            s.info.alive = false;
            s.info.exit = Some(code);
            s.writer = None;
            s.master = None;
            s.killer = None;
        }
        let msg = line(&Event { ev: "exit".into(), sid: id2.clone(), data: None, code: Some(code) });
        for c in st.conns.values().filter(|c| c.subs.contains(&id2)) {
            let _ = c.tx.send(msg.clone());
        }
        announce(&st);
    }).map_err(|e| e.to_string())?;
    Ok(sid)
}

/// Is this process running with administrator (Windows) or root rights?
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        windows_elevated()
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    // SAFETY: plain Win32 calls with a handle we open and close here.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0u32;
        let ok = GetTokenInformation(token, TokenElevation, &mut elevation as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut len);
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

fn list(st: &State) -> Vec<SessionInfo> {
    st.order.iter().filter_map(|id| st.sessions.get(id).map(|s| s.info.clone())).collect()
}

enum Outcome {
    Data(Option<serde_json::Value>),
    /// The answer was already queued (a snapshot must be queued under the lock that guards the output).
    Sent,
}

/// Run one request that came from connection `conn` with request number `id`.
fn handle(shared: &Shared, conn: u64, id: u64, req: Request) -> Result<Outcome, String> {
    let some = |v: serde_json::Value| Ok(Outcome::Data(Some(v)));
    let none = || Ok(Outcome::Data(None));
    match req {
        Request::Hello { .. } => some(serde_json::json!({ "version": VERSION, "pid": std::process::id(), "elevated": is_elevated() })),
        Request::Spawn { .. } => spawn_session(shared, &req).and_then(|sid| some(serde_json::json!({ "sid": sid }))),
        Request::List => some(serde_json::to_value(list(&shared.lock())).unwrap_or_default()),
        Request::Input { sid, data } => {
            let bytes = B64.decode(data).map_err(|e| e.to_string())?;
            let mut st = shared.lock();
            let s = st.sessions.get_mut(&sid).ok_or("no such session")?;
            if let Some(w) = s.writer.as_mut() {
                w.write_all(&bytes).and_then(|_| w.flush()).map_err(|e| e.to_string())?;
            }
            none()
        }
        Request::Resize { sid, cols, rows } => {
            let mut st = shared.lock();
            let s = st.sessions.get_mut(&sid).ok_or("no such session")?;
            let (cols, rows) = (cols.max(2), rows.max(2));
            if let Some(m) = s.master.as_ref() {
                m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
            }
            s.info.cols = cols;
            s.info.rows = rows;
            none()
        }
        Request::Kill { sid } => {
            let mut st = shared.lock();
            let s = st.sessions.get_mut(&sid).ok_or("no such session")?;
            if let Some(k) = s.killer.as_mut() {
                let _ = k.kill();
            }
            none()
        }
        Request::Remove { sid } => {
            let mut st = shared.lock();
            if let Some(s) = st.sessions.get_mut(&sid) {
                if let Some(k) = s.killer.as_mut() {
                    let _ = k.kill();
                }
            }
            st.sessions.remove(&sid);
            st.order.retain(|x| *x != sid);
            for c in st.conns.values_mut() {
                c.subs.remove(&sid);
            }
            announce(&st);
            none()
        }
        Request::Clear { sid } => {
            let mut st = shared.lock();
            let s = st.sessions.get_mut(&sid).ok_or("no such session")?;
            s.ring.clear();
            none()
        }
        Request::Snapshot { sid } => {
            // The answer is queued under the same lock as the output, so everything after it on
            // this connection is new, and nothing is lost or repeated.
            let mut st = shared.lock();
            let s = st.sessions.get(&sid).ok_or("no such session")?;
            let data = B64.encode(s.ring.iter().copied().collect::<Vec<u8>>());
            let reply = line(&Reply { re: id, ok: true, error: None, data: Some(serde_json::json!({ "data": data, "info": s.info.clone() })) });
            if let Some(c) = st.conns.get_mut(&conn) {
                let _ = c.tx.send(reply);
                c.subs.insert(sid);
            }
            Ok(Outcome::Sent)
        }
        Request::Title { sid, title } => {
            let mut st = shared.lock();
            let s = st.sessions.get_mut(&sid).ok_or("no such session")?;
            s.info.title = title;
            announce(&st);
            none()
        }
        Request::Shutdown => {
            let mut st = shared.lock();
            for s in st.sessions.values_mut() {
                if let Some(k) = s.killer.as_mut() {
                    let _ = k.kill();
                }
            }
            st.quit = true;
            none()
        }
    }
}

fn serve_connection(shared: Shared, token: String, stream: TcpStream) {
    let _ = stream.set_nodelay(true);
    let Ok(write_half) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    // The first line must be hello with the right token.
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let ok = serde_json::from_str::<Frame>(&first).ok().filter(|f| matches!(&f.req, Request::Hello { token: t } if *t == token));
    let Some(hello) = ok else { return };
    let (tx, rx) = channel::<String>();
    let conn_id = {
        let mut st = shared.lock();
        st.next_conn += 1;
        let id = st.next_conn;
        st.conns.insert(id, Conn { tx: tx.clone(), subs: HashSet::new() });
        st.alone_since = None;
        id
    };
    let writer = std::thread::spawn(move || {
        let mut w = write_half;
        while let Ok(msg) = rx.recv() {
            if w.write_all(msg.as_bytes()).is_err() {
                break;
            }
        }
        let _ = w.shutdown(std::net::Shutdown::Both);
    });
    if let Ok(Outcome::Data(data)) = handle(&shared, conn_id, hello.id, hello.req) {
        let _ = tx.send(line(&Reply { re: hello.id, ok: true, error: None, data }));
    }
    let mut buf = String::new();
    loop {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let Ok(frame) = serde_json::from_str::<Frame>(&buf) else { continue };
        let id = frame.id;
        let reply = match handle(&shared, conn_id, id, frame.req) {
            Ok(Outcome::Sent) => continue,
            Ok(Outcome::Data(data)) => Reply { re: id, ok: true, error: None, data },
            Err(e) => Reply { re: id, ok: false, error: Some(e), data: None },
        };
        let _ = tx.send(line(&reply));
    }
    {
        let mut st = shared.lock();
        st.conns.remove(&conn_id);
        if st.conns.is_empty() {
            st.alone_since = Some(Instant::now());
        }
    }
    drop(tx);
    let _ = writer.join();
}

/// Run the host until there is nothing left to do. Returns the process exit code.
pub fn serve(dir: &Path) -> i32 {
    serve_until(dir, Arc::new(|| std::process::exit(0)))
}

/// Like [`serve`], with what to do when the host is finished (the process exits; tests stop a thread).
pub fn serve_until(dir: &Path, finish: Arc<dyn Fn() + Send + Sync>) -> i32 {
    let _ = std::fs::create_dir_all(dir);
    // Another host is already answering: leave it be.
    if super::client::read_info(dir).map(|i| super::client::ping(&i)).unwrap_or(false) {
        return 0;
    }
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(_) => return 1,
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let mut raw = [0u8; 16];
    let _ = getrandom::fill(&mut raw);
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let info = HostInfo { port, token: token.clone(), pid: std::process::id(), version: VERSION };
    let path = info_path(dir);
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, serde_json::to_vec(&info).unwrap_or_default()).is_err() {
        return 1;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    if std::fs::rename(&tmp, &path).is_err() {
        return 1;
    }

    let shared: Shared = Arc::new(Mutex::new(State { alone_since: Some(Instant::now()), ..Default::default() }));
    // Watch for the reasons to stop.
    {
        let shared = shared.clone();
        let path = path.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(500));
            let (quit, stop) = {
                let st = shared.lock();
                let alive = st.sessions.values().any(|s| s.info.alive);
                let alone = st.alone_since.map(|t| t.elapsed());
                let stop = match alone {
                    // Nothing running and nobody connected: nothing to keep.
                    Some(t) if !alive && t > Duration::from_secs(20) => true,
                    Some(t) if t > ORPHAN_LIMIT => true,
                    _ => false,
                };
                (st.quit, stop)
            };
            if quit || stop {
                // A moment for the answer to a shutdown request to be sent.
                std::thread::sleep(Duration::from_millis(300));
                let still_ours = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<HostInfo>(&b).ok()).map(|i| i.pid == std::process::id()).unwrap_or(false);
                if still_ours {
                    let _ = std::fs::remove_file(&path);
                }
                finish();
                break;
            }
        });
    }
    for stream in listener.incoming().flatten() {
        let shared = shared.clone();
        let token = token.clone();
        std::thread::spawn(move || serve_connection(shared, token, stream));
    }
    0
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::terminal::client::Client;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;

    fn start(dir: &Path) {
        let d = dir.to_path_buf();
        std::thread::spawn(move || {
            serve_until(&d, Arc::new(|| {}));
        });
        for _ in 0..100 {
            if super::super::client::read_info(dir).map(|i| super::super::client::ping(&i)).unwrap_or(false) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("host did not start");
    }

    fn spawn_req(script: &str) -> Request {
        Request::Spawn { cwd: "/".into(), program: Some("/bin/sh".into()), args: vec!["-c".into(), script.into()], cols: 80, rows: 24, title: "t".into(), elevated: false }
    }

    #[test]
    fn runs_a_shell_keeps_its_output_and_takes_input() {
        let dir = tempfile::tempdir().unwrap();
        start(dir.path());
        let (tx, rx) = channel::<Event>();
        let closed = Arc::new(AtomicBool::new(false));
        let c2 = closed.clone();
        let client = Client::connect(dir.path(), move |e| drop(tx.send(e)), move || c2.store(true, Ordering::SeqCst)).unwrap();
        let sid = client.call(spawn_req("echo ready; read x; echo got-$x")).unwrap()["sid"].as_str().unwrap().to_string();
        // Attach once it has printed something: the snapshot holds it, later output arrives as events.
        std::thread::sleep(Duration::from_millis(400));
        let snap = client.call(Request::Snapshot { sid: sid.clone() }).unwrap();
        let first = String::from_utf8(B64.decode(snap["data"].as_str().unwrap()).unwrap()).unwrap();
        assert!(first.contains("ready"), "snapshot was {first:?}");
        client.call(Request::Input { sid: sid.clone(), data: B64.encode(b"abc\n") }).unwrap();
        let mut text = String::new();
        let mut code = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && code.is_none() {
            if let Ok(e) = rx.recv_timeout(Duration::from_millis(200)) {
                match e.ev.as_str() {
                    "out" => text.push_str(&String::from_utf8_lossy(&B64.decode(e.data.unwrap()).unwrap())),
                    "exit" => code = e.code,
                    _ => {}
                }
            }
        }
        assert!(text.contains("got-abc"), "output was {text:?}");
        // "ready" came with the snapshot, so it is not repeated as an event.
        assert!(!text.contains("ready"), "repeated: {text:?}");
        assert_eq!(code, Some(0));
        let list = client.call(Request::List).unwrap();
        assert_eq!(list[0]["alive"], false);
        assert_eq!(list[0]["exit"], 0);
        // Clearing empties what is kept for later attachers.
        client.call(Request::Clear { sid: sid.clone() }).unwrap();
        let after = client.call(Request::Snapshot { sid: sid.clone() }).unwrap();
        assert_eq!(after["data"], "");
        client.call(Request::Remove { sid }).unwrap();
        assert_eq!(client.call(Request::List).unwrap().as_array().unwrap().len(), 0);
        client.call(Request::Shutdown).unwrap();
    }

    #[test]
    fn refuses_a_connection_without_the_token() {
        let dir = tempfile::tempdir().unwrap();
        start(dir.path());
        let mut info = super::super::client::read_info(dir.path()).unwrap();
        info.token = "wrong".into();
        assert!(!super::super::client::ping(&info));
        let _ = Client::connect(dir.path(), |_| {}, || {}).unwrap().call(Request::Shutdown);
    }

    #[test]
    fn a_second_client_sees_what_the_first_started() {
        let dir = tempfile::tempdir().unwrap();
        start(dir.path());
        let a = Client::connect(dir.path(), |_| {}, || {}).unwrap();
        let sid = a.call(spawn_req("echo from-a; sleep 3")).unwrap()["sid"].as_str().unwrap().to_string();
        std::thread::sleep(Duration::from_millis(400));
        let b = Client::connect(dir.path(), |_| {}, || {}).unwrap();
        let listed = b.call(Request::List).unwrap();
        assert_eq!(listed[0]["sid"], sid.as_str());
        let snap = b.call(Request::Snapshot { sid: sid.clone() }).unwrap();
        assert!(String::from_utf8_lossy(&B64.decode(snap["data"].as_str().unwrap()).unwrap()).contains("from-a"));
        b.call(Request::Kill { sid }).unwrap();
        a.call(Request::Shutdown).unwrap();
    }
}
