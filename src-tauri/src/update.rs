//! Keeping the app up to date from the builds the repository's workflow publishes: looking for a
//! newer one, downloading it as a job on the Transfers card, and installing it without the
//! installer's own screens. What the build names and installers look like lives in `fast_core::update`.

use crate::state::Shared;
use fast_core::ops::{DeletePhase, DeleteProgress};
use fast_core::update::{self as core, InstallerKind, Latest, Newer};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

/// What this program was built from, set by the workflow that builds it.
fn current_sha() -> Option<&'static str> {
    option_env!("GITHUB_SHA")
}

/// What is remembered between runs.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Memory {
    /// The newest build the person was told about.
    notified_sha: String,
    /// A downloaded build waiting for a restart.
    ready: Option<Ready>,
    last_check: i64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Ready {
    pub sha: String,
    pub installer: PathBuf,
    pub kind: InstallerKind,
    pub created_at: String,
}

static MEMORY: Mutex<Option<Memory>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);

fn memory(st: &Shared) -> Memory {
    MEMORY.lock().get_or_insert_with(|| std::fs::read_to_string(&st.paths.update_state).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()).clone()
}

fn remember(st: &Shared, change: impl FnOnce(&mut Memory)) {
    let mut m = memory(st);
    change(&mut m);
    let _ = std::fs::write(&st.paths.update_state, serde_json::to_string(&m).unwrap_or_default());
    *MEMORY.lock() = Some(m);
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// "off", "current", "available", "unknown" (a build of unknown origin), "unsupported" or "error".
    pub status: &'static str,
    pub version: &'static str,
    pub sha: Option<&'static str>,
    pub latest: Option<Latest>,
    pub ready: Option<Ready>,
    pub message: String,
    /// A download needs an access token (or nightly.link) to work.
    pub token_set: bool,
    pub last_check: i64,
}

fn info(st: &Shared, status: &'static str, latest: Option<Latest>, message: String) -> Info {
    let m = memory(st);
    Info { status, version: env!("CARGO_PKG_VERSION"), sha: current_sha(), latest, ready: m.ready.filter(|r| r.installer.exists()), message, token_set: !st.settings.read().updates.token.is_empty(), last_check: m.last_check }
}

// ---------------------------------------------------------------------------
// Talking to GitHub
// ---------------------------------------------------------------------------

#[cfg(desktop)]
mod http {
    use super::*;

    fn agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent("FAST-File-Explorer-updater")
            .build()
            .into()
    }

    /// GET with redirects followed by hand, so the access token goes only to GitHub's API and never on to
    /// the storage address it redirects to.
    pub fn open(url: &str, token: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
        let agent = agent();
        let mut url = url.to_string();
        for _ in 0..6 {
            let mut req = agent.get(&url).header("Accept", accept);
            if !token.is_empty() && url.starts_with("https://api.github.com/") {
                req = req.header("Authorization", &format!("Bearer {token}")).header("X-GitHub-Api-Version", "2022-11-28");
            }
            let resp = req.call().map_err(|e| e.to_string())?;
            let code = resp.status().as_u16();
            if (300..400).contains(&code) {
                let Some(next) = resp.headers().get("location").and_then(|v| v.to_str().ok()) else { return Err("a redirect with nowhere to go".into()) };
                url = next.to_string();
                continue;
            }
            if code == 200 {
                return Ok(resp);
            }
            return Err(match code {
                401 | 403 if url.starts_with("https://api.github.com/") && resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0") => "GitHub's limit for anonymous requests is used up for now; try again in a while, or add an access token.".to_string(),
                401 | 403 => "GitHub refused the request: check the access token and that it may read Actions.".to_string(),
                404 => "not found".to_string(),
                c => format!("GitHub answered with status {c}"),
            });
        }
        Err("too many redirects".into())
    }

    pub fn json(url: &str, token: &str) -> Result<serde_json::Value, String> {
        let mut resp = open(url, token, "application/vnd.github+json")?;
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}

#[cfg(desktop)]
fn find_latest(st: &Shared) -> Result<Option<Latest>, String> {
    let name = core::artifact_name().ok_or("this kind of computer has no published builds")?;
    let (repo, branch, token) = {
        let u = &st.settings.read().updates;
        (u.repo.trim().to_string(), u.branch.trim().to_string(), u.token.clone())
    };
    if repo.split('/').count() != 2 || repo.contains(' ') {
        return Err(format!("\"{repo}\" is not a repository name (use owner/name)"));
    }
    let url = format!("https://api.github.com/repos/{repo}/actions/artifacts?name={name}&per_page=50");
    let listing = http::json(&url, &token).map_err(|e| if e == "not found" { format!("Could not find {repo} on GitHub, or it is private (private repositories need an access token).") } else { e })?;
    Ok(core::pick_latest(&listing, name, &branch))
}

/// Look for a newer build. Quiet: it tells nobody; `run_auto` and the commands decide what to say.
#[cfg(desktop)]
fn check_now(st: &Shared) -> Info {
    if !st.settings.read().updates.enabled {
        return info(st, "off", None, "Updates are turned off.".into());
    }
    let result = find_latest(st);
    remember(st, |m| m.last_check = chrono::Local::now().timestamp());
    match result {
        Err(e) => info(st, "error", None, e),
        Ok(None) => info(st, "current", None, format!("No build of the {} branch is available to download.", st.settings.read().updates.branch)),
        Ok(Some(latest)) => match core::is_newer(current_sha(), &latest.sha) {
            Newer::No => info(st, "current", Some(latest), "You have the newest build.".into()),
            Newer::Yes => info(st, "available", Some(latest), "A newer build is available.".into()),
            Newer::Unknown => info(st, "unknown", Some(latest), "This copy does not know which build it is, so it cannot tell whether the newest one is newer.".into()),
        },
    }
}

#[cfg(not(desktop))]
fn check_now(st: &Shared) -> Info {
    info(st, "unsupported", None, "Phones and tablets are updated through the file you install them from.".into())
}

// ---------------------------------------------------------------------------
// Downloading
// ---------------------------------------------------------------------------

struct Job {
    app: AppHandle,
    id: u64,
    started: Instant,
    cancel: Arc<AtomicBool>,
    name: String,
}

impl Job {
    fn emit(&self, phase: DeletePhase, done: u64, total: u64, current: &str, error: Option<&str>) {
        let _ = self.app.emit(
            "delete-progress",
            DeleteProgress {
                id: self.id,
                phase,
                permanent: false,
                selected: 1,
                items_total: 1,
                items_done: u64::from(phase == DeletePhase::Done),
                current: current.to_string(),
                errors: error.map(|m| vec![fast_core::transfer::engine::TransferError { path: self.name.clone(), message: m.to_string(), needs_elevation: false }]).unwrap_or_default(),
                error_count: u64::from(error.is_some()),
                elapsed_secs: self.started.elapsed().as_secs_f64(),
                first_name: self.name.clone(),
                action: "update".into(),
                result: String::new(),
                bytes_done: done,
                bytes_total: total,
                skipped: 0,
                conflict: None,
            },
        );
    }
}

/// The address a build can be fetched from: GitHub itself with a token, nightly.link without one.
fn download_url(latest: &Latest, repo: &str, token: &str) -> String {
    if token.is_empty() { format!("https://nightly.link/{repo}/actions/artifacts/{}.zip", latest.artifact_id) } else { latest.download_url.clone() }
}

#[cfg(desktop)]
fn download_and_unpack(job: &Job, st: &Shared, latest: &Latest) -> Result<Ready, String> {
    use std::io::{Read, Write};
    let (repo, token) = {
        let u = &st.settings.read().updates;
        (u.repo.trim().to_string(), u.token.clone())
    };
    let dir = st.paths.update_dir.join(&latest.sha);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip_path = dir.join("build.zip");
    let url = download_url(latest, &repo, &token);
    let resp = http::open(&url, &token, "application/octet-stream").map_err(|e| {
        if e == "not found" && token.is_empty() {
            "GitHub only lets signed-in accounts download builds. Add an access token under Settings > Updates, or install nightly.link on the repository.".to_string()
        } else {
            e
        }
    })?;
    let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(latest.size);
    let mut body = resp.into_body().into_with_config().limit(8 << 30).reader();
    let mut out = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1024];
    let (mut done, mut last) = (0u64, Instant::now() - Duration::from_secs(1));
    loop {
        if job.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if last.elapsed() >= Duration::from_millis(250) {
            last = Instant::now();
            job.emit(DeletePhase::Running, done, total.max(done), "Downloading", None);
        }
    }
    drop(out);
    job.emit(DeletePhase::Running, done, done, "Checking and unpacking", None);
    let actual = fast_core::details::sha256(&zip_path, Some(&job.cancel)).map_err(|e| e.to_string())?.ok_or("cancelled")?;
    if !core::digest_matches(latest.digest.as_deref(), &actual) {
        return Err("the download does not match what GitHub published for it".into());
    }
    let unpacked = dir.join("build");
    core::unzip(&zip_path, &unpacked).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip_path);
    let (installer, kind) = core::find_installer(&unpacked).ok_or("the build holds no installer for this computer")?;
    Ok(Ready { sha: latest.sha.clone(), installer, kind, created_at: latest.created_at.clone() })
}

#[cfg(not(desktop))]
fn download_and_unpack(_: &Job, _: &Shared, _: &Latest) -> Result<Ready, String> {
    Err("not supported here".into())
}

/// Download `latest` as a job on the Transfers card; announces `update-ready` when it is done.
fn start_download(app: &AppHandle, st: &Shared, latest: Latest) -> CmdResult<u64> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("An update is already being downloaded.".into());
    }
    let id = st.next_id.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    st.deletes.lock().insert(id, cancel.clone());
    let job = Job { app: app.clone(), id, started: Instant::now(), cancel, name: "FAST File Explorer update".into() };
    job.emit(DeletePhase::Scanning, 0, latest.size, "Starting", None);
    let st: Shared = app.state::<Shared>().inner().clone();
    std::thread::Builder::new()
        .name("update-download".into())
        .spawn({
            let app = app.clone();
            move || {
                match download_and_unpack(&job, &st, &latest) {
                    Ok(ready) => {
                        remember(&st, |m| m.ready = Some(ready));
                        job.emit(DeletePhase::Done, latest.size, latest.size, "", None);
                        let _ = app.emit("update-ready", info(&st, "available", Some(latest), String::new()));
                    }
                    Err(e) if e == "cancelled" => job.emit(DeletePhase::Cancelled, 0, 0, "", None),
                    Err(e) => {
                        let _ = std::fs::remove_dir_all(st.paths.update_dir.join(&latest.sha));
                        job.emit(DeletePhase::Failed, 0, 0, "", Some(&e));
                    }
                }
                st.deletes.lock().remove(&job.id);
                BUSY.store(false, Ordering::SeqCst);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(id)
}

// ---------------------------------------------------------------------------
// Applying
// ---------------------------------------------------------------------------

/// Install the downloaded build and start the app again. Windows and AppImage swap the files in the
/// background after the app has closed, with no installer screens. Other packages cannot be installed
/// silently by a program without administrator rights, so they are handed to the system's own installer.
#[cfg(desktop)]
fn apply(app: &AppHandle, st: &Shared, ready: &Ready) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let work = st.paths.update_dir.clone();
    match ready.kind {
        InstallerKind::Nsis | InstallerKind::Msi if cfg!(windows) => {
            #[cfg(windows)]
            {
                let script = work.join("finish-update.bat");
                std::fs::write(&script, core::windows_script(&ready.installer, ready.kind, &exe)).map_err(|e| e.to_string())?;
                let mut cmd = std::process::Command::new("cmd");
                cmd.arg("/C").arg(&script);
                fast_core::proc::quiet(&mut cmd);
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000 | 0x0000_0200); // no window, own process group (outlives this app)
                cmd.spawn().map_err(|e| e.to_string())?;
            }
        }
        InstallerKind::AppImage if cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some() => {
            let target = PathBuf::from(std::env::var_os("APPIMAGE").unwrap_or_default());
            let script = work.join("finish-update.sh");
            std::fs::write(&script, core::appimage_script(&ready.installer, &target)).map_err(|e| e.to_string())?;
            std::process::Command::new("sh").arg(&script).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().map_err(|e| e.to_string())?;
        }
        _ => {
            // .deb, .rpm, .dmg and the like: the system's installer takes over, with its own screens.
            let _ = &exe;
            fast_core::platform::open_path(&ready.installer).map_err(|e| e.to_string())?;
            remember(st, |m| m.ready = None);
            return Ok(());
        }
    }
    remember(st, |m| m.ready = None);
    app.exit(0);
    Ok(())
}

#[cfg(not(desktop))]
fn apply(_: &AppHandle, _: &Shared, _: &Ready) -> Result<(), String> {
    Err("not supported here".into())
}

// ---------------------------------------------------------------------------
// Automatic checks
// ---------------------------------------------------------------------------

/// A check made by the app itself: tells the person once for each new build, and downloads it if the
/// settings say so.
fn run_auto(app: &AppHandle, st: &Shared) {
    let result = check_now(st);
    if result.status != "available" {
        return;
    }
    let Some(latest) = result.latest.clone() else { return };
    let (notify, auto) = {
        let u = &st.settings.read().updates;
        (u.notify, u.auto_download)
    };
    if notify && memory(st).notified_sha != latest.sha {
        remember(st, |m| m.notified_sha = latest.sha.clone());
        let _ = app.emit("update-available", result);
    }
    if auto && memory(st).ready.as_ref().is_none_or(|r| r.sha != latest.sha) {
        let _ = start_download(app, st, latest);
    }
}

/// Looks once after the app opens, then every few hours while it stays open.
pub fn start(app: &AppHandle, st: Shared) {
    if !cfg!(desktop) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("updates".into()).spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        // A build that was downloaded and never installed is offered again.
        if let Some(r) = memory(&st).ready.filter(|r| r.installer.exists()) {
            let _ = app.emit("update-ready", info(&st, "available", None, String::new()));
            let _ = r;
        }
        if st.settings.read().updates.enabled && st.settings.read().updates.check_on_open {
            run_auto(&app, &st);
        }
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let (on, hours) = {
                let u = &st.settings.read().updates;
                (u.enabled && u.background, u.check_hours.max(1))
            };
            // Nothing is checked while the window is minimized or hidden and the background saver is on.
            let away = st.attention.load(Ordering::Relaxed) == 2 && st.settings.read().performance.background_mode != "off";
            let due = chrono::Local::now().timestamp() - memory(&st).last_check >= i64::from(hours) * 3600;
            if on && due && !away {
                run_auto(&app, &st);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Look for a newer build now (the "Check now" button). Never downloads by itself.
#[tauri::command]
pub async fn update_check(state: State<'_, Shared>) -> CmdResult<Info> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || check_now(&st)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_status(state: State<'_, Shared>) -> Info {
    let st = state.inner();
    info(st, if st.settings.read().updates.enabled { "current" } else { "off" }, None, String::new())
}

/// Download the newest build (the "Update" button).
#[tauri::command]
pub async fn update_download(app: AppHandle, state: State<'_, Shared>) -> CmdResult<u64> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let latest = match check_now(&st) {
            Info { latest: Some(l), .. } => l,
            Info { message, .. } => return Err(message),
        };
        start_download(&app, &st, latest)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install what was downloaded and restart.
#[tauri::command]
pub fn update_install(app: AppHandle, state: State<'_, Shared>) -> CmdResult<()> {
    let st = state.inner();
    let ready = memory(st).ready.filter(|r| r.installer.exists()).ok_or("There is no downloaded update to install.")?;
    apply(&app, st, &ready)
}

/// Remove a downloaded build that was not installed.
#[tauri::command]
pub fn update_discard(state: State<'_, Shared>) {
    let st = state.inner();
    remember(st, |m| m.ready = None);
    let _ = std::fs::remove_dir_all(&st.paths.update_dir);
}

#[allow(dead_code)]
fn _unused(_: &Path) {}
