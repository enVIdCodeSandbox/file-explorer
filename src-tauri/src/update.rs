//! Keeping the app up to date from the builds the repository's workflow publishes: looking for a
//! newer one, downloading it as a job on the Transfers card, and installing it without the
//! installer's own screens. What the build names and installers look like lives in `fast_core::update`.

use crate::state::Shared;
use fast_core::ops::{DeletePhase, DeleteProgress};
use fast_core::update::{self as core, InstallerKind, Latest, Newer, Source};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

/// What this program was built from, set by the workflow that builds it.
fn current_sha() -> Option<&'static str> {
    option_env!("GITHUB_SHA")
}

/// When this build was made (seconds since 1970), put in by the build script. A build is only an update if it is dated after this.
fn built_at() -> Option<u64> {
    option_env!("FAST_BUILD_TIME").and_then(|t| t.parse().ok())
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
    /// An update that was handed to the installer: if the app comes back as the same build, it did not take.
    applied: Option<Applied>,
    /// The last update that did not take, with what is known about why. It is not downloaded and installed again by itself.
    failed: Option<Failed>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Applied {
    key: String,
    target_version: Option<String>,
    from_version: String,
    from_sha: Option<String>,
    exe: String,
    installer: PathBuf,
    result_file: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Failed {
    pub key: String,
    pub note: String,
    pub installer: PathBuf,
    pub from_version: String,
    pub from_sha: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Ready {
    #[serde(default)]
    pub version: Option<String>,
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
    /// The last update did not take effect (the app came back as the same build).
    pub failed: Option<Failed>,
    /// A download needs an access token (or nightly.link) to work.
    pub token_set: bool,
    pub last_check: i64,
}

fn info(st: &Shared, status: &'static str, latest: Option<Latest>, message: String) -> Info {
    let m = memory(st);
    Info { status, version: env!("CARGO_PKG_VERSION"), sha: current_sha(), latest, ready: m.ready.filter(|r| r.installer.exists()), message, failed: m.failed, token_set: !st.settings.read().updates.token.is_empty(), last_check: m.last_check }
}

// ---------------------------------------------------------------------------
// Talking to GitHub
// ---------------------------------------------------------------------------

#[cfg(desktop)]
mod http {
    use super::*;

    /// One agent for the whole run, so connections (and their TLS handshakes) are reused between the
    /// listing, the redirect and the download. A host that does not answer is given up on quickly: the
    /// request is tried again once instead of waiting half a minute.
    fn agent() -> &'static ureq::Agent {
        static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
        AGENT.get_or_init(|| {
            let builder = ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(15)))
                .max_redirects(0)
                .http_status_as_error(false)
                .user_agent("FAST-File-Explorer-updater");
            // The tests talk to a server on this computer, whatever proxy the machine is set to use.
            #[cfg(test)]
            let builder = builder.proxy(None);
            builder.build().into()
        })
    }

    /// A connection that failed or timed out is worth one more try (a dead address among several, a dropped
    /// pooled connection); an answer from the server, even a refusal, is not.
    fn call_with_retry(agent: &ureq::Agent, url: &str, token: &str, accept: &str, range: Option<(u64, u64)>) -> Result<ureq::http::Response<ureq::Body>, String> {
        let build = || {
            let mut req = agent.get(url).header("Accept", accept);
            if let Some((a, b)) = range {
                req = req.header("Range", &format!("bytes={a}-{b}"));
            }
            if !token.is_empty() && url.starts_with("https://api.github.com/") {
                req = req.header("Authorization", &format!("Bearer {token}")).header("X-GitHub-Api-Version", "2022-11-28");
            }
            req.call()
        };
        match build() {
            Ok(r) => Ok(r),
            Err(_) => build().map_err(|e| e.to_string()),
        }
    }

    /// GET with redirects followed by hand, so the access token goes only to GitHub's API and never on to
    /// the storage address it redirects to.
    pub fn open(url: &str, token: &str, accept: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
        open_range(url, token, accept, None).map(|(r, _)| r)
    }

    /// Like `open`, for one part of the file when `range` is given, and the address it finally came from
    /// (the storage address, which accepts more requests for other parts without a token).
    pub fn open_range(url: &str, token: &str, accept: &str, range: Option<(u64, u64)>) -> Result<(ureq::http::Response<ureq::Body>, String), String> {
        let agent = agent();
        let mut url = url.to_string();
        for _ in 0..6 {
            let resp = call_with_retry(agent, &url, token, accept, range)?;
            let code = resp.status().as_u16();
            if (300..400).contains(&code) {
                let Some(next) = resp.headers().get("location").and_then(|v| v.to_str().ok()) else { return Err("a redirect with nowhere to go".into()) };
                url = next.to_string();
                continue;
            }
            if code == 200 || (code == 206 && range.is_some()) {
                return Ok((resp, url));
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

    /// How many connections a download of this size is split over: about one for each 2 MB, up to 8.
    pub fn streams_for(size: u64) -> usize {
        ((size / (2 << 20)) as usize).clamp(1, 8)
    }

    /// The byte ranges (first, last) of each stream.
    pub fn split(size: u64, streams: usize) -> Vec<(u64, u64)> {
        let n = streams.max(1) as u64;
        let part = size.div_ceil(n).max(1);
        (0..n).map(|i| (i * part, ((i + 1) * part).min(size) - 1)).filter(|(a, b)| a <= b && *a < size).collect()
    }

    fn write_at(file: &std::fs::File, buf: &[u8], at: u64) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileExt;
            file.write_all_at(buf, at)
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::FileExt;
            let (mut done, mut at) = (0usize, at);
            while done < buf.len() {
                let n = file.seek_write(&buf[done..], at)?;
                if n == 0 {
                    return Err(std::io::ErrorKind::WriteZero.into());
                }
                done += n;
                at += n as u64;
            }
            Ok(())
        }
    }

    /// Copy one stream's bytes into `file` from `*at` up to `end`, counting them in `done` and moving `*at`
    /// along, so a stream that breaks can be asked for again from where it stopped.
    fn pour(mut body: impl std::io::Read, file: &std::fs::File, at: &mut u64, end: u64, done: &AtomicU64, cancel: &AtomicBool, buf: &mut [u8]) -> Result<(), String> {
        while *at <= end {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let want = buf.len().min((end - *at + 1) as usize);
            let n = body.read(&mut buf[..want]).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("the connection ended early".into());
            }
            write_at(file, &buf[..n], *at).map_err(|e| e.to_string())?;
            *at += n as u64;
            done.fetch_add(n as u64, Ordering::Relaxed);
        }
        Ok(())
    }

    /// Download `url` into `dest`. A file of a few MB or more is fetched over several connections at once
    /// (each asks for its own part), which is what it takes to use a fast link: one connection to a
    /// download server is often held to a fraction of it. A server that does not do parts, or a file whose
    /// size is not known, is fetched over one connection as before. `progress(done, total)` is called about
    /// every 100 ms. Returns the bytes written.
    pub fn download(url: &str, token: &str, accept: &str, dest: &Path, size_hint: u64, cancel: &AtomicBool, progress: &(dyn Fn(u64, u64) + Sync)) -> Result<u64, String> {
        let streams = streams_for(size_hint);
        let done = AtomicU64::new(0);
        let file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        let single = |first: Option<ureq::http::Response<ureq::Body>>| -> Result<u64, String> {
            let resp = match first {
                Some(r) => r,
                None => open(url, token, accept)?,
            };
            let total = resp.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(size_hint);
            let body = resp.into_body().into_with_config().limit(8 << 30).reader();
            let mut buf = vec![0u8; 1 << 20];
            std::thread::scope(|s| {
                let job = s.spawn(|| {
                    // One connection to the end: how long the file is is not known to the reader, so it reads until it stops.
                    use std::io::{Read, Write};
                    let (mut body, mut out) = (body, &file);
                    loop {
                        if cancel.load(Ordering::Relaxed) {
                            return Err("cancelled".to_string());
                        }
                        let n = body.read(&mut buf).map_err(|e| e.to_string())?;
                        if n == 0 {
                            return Ok(());
                        }
                        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                        done.fetch_add(n as u64, Ordering::Relaxed);
                    }
                });
                while !job.is_finished() {
                    std::thread::sleep(Duration::from_millis(100));
                    progress(done.load(Ordering::Relaxed), total.max(done.load(Ordering::Relaxed)));
                }
                job.join().unwrap_or_else(|_| Err("the download stopped unexpectedly".to_string()))
            })?;
            Ok(done.load(Ordering::Relaxed))
        };
        if streams < 2 {
            return single(None);
        }
        let ranges = split(size_hint, streams);
        // The first part also tells whether the server does parts, and where the file really is stored.
        let (first, final_url) = open_range(url, token, accept, Some(ranges[0]))?;
        if first.status().as_u16() != 206 {
            // The whole file came back: use it as it is.
            return single(Some(first));
        }
        let total: u64 = first
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if total != size_hint {
            // The listing was out of date: start over, one connection, with the real size.
            drop(first);
            return single(None);
        }
        let _ = file.set_len(total);
        let first = Mutex::new(Some(first));
        let result: Result<(), String> = std::thread::scope(|s| {
            let (file, done, first, final_url) = (&file, &done, &first, &final_url);
            let workers: Vec<_> = ranges
                .iter()
                .enumerate()
                .map(|(i, &(a, b))| {
                    s.spawn(move || -> Result<(), String> {
                        let mut buf = vec![0u8; 256 << 10];
                        let (mut at, mut breaks) = (a, 0);
                        while at <= b {
                            // Part 0 starts on the connection that was opened to find out about the server; a part
                            // that broke is asked for again from where it stopped.
                            let resp = match first.lock().take().filter(|_| i == 0 && at == a) {
                                Some(r) => r,
                                None => {
                                    let (r, _) = open_range(final_url, "", accept, Some((at, b)))?;
                                    if r.status().as_u16() != 206 {
                                        return Err("the server stopped doing parts".into());
                                    }
                                    r
                                }
                            };
                            let body = resp.into_body().into_with_config().limit(8 << 30).reader();
                            match pour(body, file, &mut at, b, done, cancel, &mut buf) {
                                Err(e) if e == "cancelled" => return Err(e),
                                Err(e) => {
                                    breaks += 1;
                                    if breaks > 3 {
                                        return Err(e);
                                    }
                                }
                                Ok(()) => {}
                            }
                        }
                        Ok(())
                    })
                })
                .collect();
            while workers.iter().any(|w| !w.is_finished()) {
                std::thread::sleep(Duration::from_millis(100));
                progress(done.load(Ordering::Relaxed), total);
            }
            let mut result = Ok(());
            for w in workers {
                match w.join() {
                    Ok(Err(e)) if result.is_ok() || e != "cancelled" => result = Err(e),
                    Err(_) => result = Err("the download stopped unexpectedly".into()),
                    _ => {}
                }
            }
            result
        });
        result?;
        if done.load(Ordering::Relaxed) != total {
            return Err("the download ended before the whole file arrived".into());
        }
        Ok(total)
    }

    pub fn json(url: &str, token: &str) -> Result<serde_json::Value, String> {
        let mut resp = open(url, token, "application/vnd.github+json")?;
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}

/// Everything found at the places builds are published: the repository's releases (public) and the
/// artifacts of its workflow runs. One place failing does not hide what the other has; the reasons
/// are returned too.
#[cfg(desktop)]
fn find_all(st: &Shared) -> (Vec<Latest>, Vec<String>) {
    let (repo, branch, token, unreleased) = {
        let u = &st.settings.read().updates;
        (u.repo.trim().to_string(), u.branch.trim().to_string(), u.token.clone(), u.include_unreleased)
    };
    if repo.split('/').count() != 2 || repo.contains(' ') {
        return (Vec::new(), vec![format!("\"{repo}\" is not a repository name (use owner/name)")]);
    }
    let missing = |e: String| if e == "not found" { format!("Could not find {repo} on GitHub, or it is private (private repositories need an access token).") } else { e };
    let (mut found, mut problems) = (Vec::new(), Vec::new());
    // Workflow artifacts are unreleased builds and need a token or nightly.link to download: only on request.
    let artifacts = core::artifact_name().filter(|_| unreleased);
    // Both lists are asked for at the same time. Ten releases are plenty to find the newest installer.
    let (repo_ref, token_ref) = (&repo, &token);
    let (releases, actions) = std::thread::scope(|s| {
        let a = s.spawn(move || http::json(&format!("https://api.github.com/repos/{repo_ref}/releases?per_page=10"), token_ref));
        let b = artifacts.map(|name| s.spawn(move || http::json(&format!("https://api.github.com/repos/{repo_ref}/actions/artifacts?name={name}&per_page=50"), token_ref)));
        (a.join().unwrap_or_else(|_| Err("the check stopped unexpectedly".into())), b.map(|b| b.join().unwrap_or_else(|_| Err("the check stopped unexpectedly".into()))))
    });
    match releases {
        Ok(listing) => found.extend(core::pick_release(&listing, core::Target::here())),
        Err(e) => problems.push(format!("Releases: {}", missing(e))),
    }
    if let (Some(name), Some(result)) = (artifacts, actions) {
        match result {
            Ok(listing) => found.extend(core::pick_latest(&listing, name, &branch)),
            Err(e) => problems.push(format!("Actions: {}", missing(e))),
        }
    }
    (found, problems)
}

/// The last answer to a check, so pressing "Update" right after one does not ask GitHub the same thing again.
static LAST_CHECK: Mutex<Option<(Instant, Info)>> = Mutex::new(None);
const CHECK_FRESH: Duration = Duration::from_secs(20 * 60);

/// What a check found a short while ago, if it found a newer build.
fn fresh_find() -> Option<Latest> {
    LAST_CHECK.lock().as_ref().filter(|(at, i)| at.elapsed() < CHECK_FRESH && i.status == "available").and_then(|(_, i)| i.latest.clone())
}

/// Look for a newer build. Quiet: it tells nobody; `run_auto` and the commands decide what to say.
#[cfg(desktop)]
fn check_now(st: &Shared) -> Info {
    let result = check_uncached(st);
    *LAST_CHECK.lock() = Some((Instant::now(), result.clone()));
    result
}

#[cfg(desktop)]
fn check_uncached(st: &Shared) -> Info {
    if !st.settings.read().updates.enabled {
        return info(st, "off", None, "Updates are turned off.".into());
    }
    let (found, problems) = find_all(st);
    remember(st, |m| m.last_check = chrono::Local::now().timestamp());
    match core::best_of(found, current_sha(), env!("CARGO_PKG_VERSION"), built_at()) {
        None if !problems.is_empty() => info(st, "error", None, problems.join(" ")),
        None => info(st, "current", None, "No build is published for this kind of computer yet.".into()),
        Some((latest, Newer::No)) => info(st, "current", Some(latest), "You have the newest build.".into()),
        Some((latest, Newer::Yes)) => info(st, "available", Some(latest), "A newer build is available.".into()),
        Some((latest, Newer::Unknown)) => info(st, "unknown", Some(latest), "This copy does not know which build it is, so it cannot tell whether the newest one is newer.".into()),
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
                errors: error.map(|m| vec![fast_core::transfer::engine::TransferError { path: self.name.clone(), message: m.to_string(), needs_elevation: false, dest: None }]).unwrap_or_default(),
                error_count: u64::from(error.is_some()),
                elapsed_secs: self.started.elapsed().as_secs_f64(),
                first_name: self.name.clone(),
                action: "update".into(),
                result: String::new(),
                bytes_done: done,
                bytes_total: total,
                skipped: 0,
                conflict: None,
                stats: None,
                method: "HTTPS download from GitHub".into(),
            },
        );
    }
}

/// The address a build can be fetched from. A release's file is public. A workflow artifact comes
/// from GitHub itself with a token, or through nightly.link without one.
fn download_url(latest: &Latest, repo: &str, token: &str) -> String {
    if latest.source == Source::Release || !token.is_empty() { latest.download_url.clone() } else { format!("https://nightly.link/{repo}/actions/artifacts/{}.zip", latest.artifact_id) }
}

/// Windows Security (or another antivirus) can quarantine a downloaded installer the moment it is written; Windows then
/// refuses to open it ("os error 225", or 226 when it was already removed). Say that plainly, since it is not a fault
/// of the download, and what can be done about it.
pub fn explain_blocked(e: String) -> String {
    let l = e.to_lowercase();
    if l.contains("os error 225") || l.contains("os error 226") || l.contains("virus or potentially unwanted") {
        "Windows Security blocked the downloaded installer and reported it as a virus or unwanted software. New apps that are not code-signed are often flagged by mistake. To go on: open Windows Security → Virus & threat protection → Protection history, find the entry for this installer, and choose Allow on device (or Restore); then check for updates again. You can also download the installer from the release page and run it yourself. This app cannot override your antivirus.".to_string()
    } else {
        e
    }
}

#[cfg(desktop)]
fn download_and_unpack(job: &Job, st: &Shared, latest: &Latest) -> Result<Ready, String> {
    download_and_unpack_inner(job, st, latest).map_err(explain_blocked)
}

#[cfg(desktop)]
fn download_and_unpack_inner(job: &Job, st: &Shared, latest: &Latest) -> Result<Ready, String> {
    let (repo, token) = {
        let u = &st.settings.read().updates;
        (u.repo.trim().to_string(), u.token.clone())
    };
    let dir = st.paths.update_dir.join(latest.key());
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // A release hands over the installer itself; a workflow artifact is a zip that holds it.
    let is_release = latest.source == Source::Release;
    let zip_path = dir.join(if is_release { latest.name.as_str() } else { "build.zip" });
    // Only the file's own name, whatever the release called it.
    let zip_path = if is_release { dir.join(zip_path.file_name().unwrap_or_default()) } else { zip_path };
    let url = download_url(latest, &repo, &token);
    job.emit(DeletePhase::Scanning, 0, latest.size, "Connecting to GitHub", None);
    let mapped = |e: String| {
        if e == "not found" && token.is_empty() && !is_release {
            "GitHub only lets signed-in accounts download builds. Add an access token under Settings > Updates, or install nightly.link on the repository.".to_string()
        } else {
            e
        }
    };
    let progress = |done: u64, total: u64| job.emit(DeletePhase::Running, done, total.max(done), "Downloading", None);
    http::download(&url, if is_release { "" } else { &token }, "application/octet-stream", &zip_path, latest.size, &job.cancel, &progress).map_err(mapped)?;
    let done = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
    job.emit(DeletePhase::Running, done, done, "Checking and unpacking", None);
    let actual = fast_core::details::sha256(&zip_path, Some(&job.cancel)).map_err(|e| e.to_string())?.ok_or("cancelled")?;
    if !core::digest_matches(latest.digest.as_deref(), &actual) {
        return Err("the download does not match what GitHub published for it".into());
    }
    let (installer, kind) = if is_release {
        let kind = core::kind_of_name(&latest.name).ok_or("the release file is not an installer")?;
        (zip_path, kind)
    } else {
        let unpacked = dir.join("build");
        core::unzip(&zip_path, &unpacked).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip_path);
        core::find_installer(&unpacked).ok_or("the build holds no installer for this computer")?
    };
    Ok(Ready { version: latest.version.clone(), sha: latest.key(), installer, kind, created_at: latest.created_at.clone() })
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
                        let _ = std::fs::remove_dir_all(st.paths.update_dir.join(latest.key()));
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
    let result_file = work.join("last-install-result.txt");
    let _ = std::fs::remove_file(&result_file);
    // Written down before the app closes: if it comes back as this same build, the update did not take, and why is known.
    remember(st, |m| {
        m.applied = Some(Applied {
            key: ready.sha.clone(),
            target_version: ready.version.clone().or_else(|| ready.installer.file_name().and_then(|n| core::version_in_name(&n.to_string_lossy()))),
            from_version: env!("CARGO_PKG_VERSION").to_string(),
            from_sha: current_sha().map(str::to_string),
            exe: exe.display().to_string(),
            installer: ready.installer.clone(),
            result_file: result_file.clone(),
        })
    });
    match ready.kind {
        InstallerKind::Nsis | InstallerKind::Msi if cfg!(windows) => {
            #[cfg(windows)]
            {
                let script = work.join("finish-update.bat");
                std::fs::write(&script, core::windows_script(&ready.installer, ready.kind, &exe, &result_file)).map_err(|e| e.to_string())?;
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
            remember(st, |m| {
                m.ready = None;
                m.applied = None;
            });
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
    // An update that already failed to take effect is not fetched and installed again by itself, round and round.
    if memory(st).failed.as_ref().is_some_and(|f| f.key == latest.key()) {
        return;
    }
    let (notify, auto) = {
        let u = &st.settings.read().updates;
        (u.notify, u.auto_download)
    };
    if notify && memory(st).notified_sha != latest.key() {
        remember(st, |m| m.notified_sha = latest.key());
        let _ = app.emit("update-available", result);
    }
    if auto && memory(st).ready.as_ref().is_none_or(|r| r.sha != latest.key()) {
        let _ = start_download(app, st, latest);
    }
}

/// Did the update that was handed to the installer take effect? If this is the same build as before it, it did not: say so
/// (with what the installer reported) and do not fetch the same one again. Otherwise the last failure, if any, is over.
fn settle_pending(app: &AppHandle, st: &Shared) {
    let m = memory(st);
    let (version, sha) = (env!("CARGO_PKG_VERSION"), current_sha());
    let unchanged = |v: &str, s: &Option<String>| v == version && s.as_deref() == sha;
    if let Some(a) = m.applied.clone() {
        if unchanged(&a.from_version, &a.from_sha) {
            let code = std::fs::read_to_string(&a.result_file).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
            let build = format!("{version}{}", sha.map(|s| format!(" ({})", &s[..s.len().min(7)])).unwrap_or_default());
            let target = a.target_version.as_deref().map(|v| format!(" {v}")).unwrap_or_default();
            // What the installer file itself says it contains tells the two likely causes apart.
            let named = a.installer.file_name().and_then(|n| core::version_in_name(&n.to_string_lossy()));
            let named_text = named.as_deref().map(|v| format!(" The installer file is named for version {v}.")).unwrap_or_default();
            let note = match code.as_deref() {
                Some("0") => format!("The installer reported success for the update{target}, but this app is still {build}. It was started from {}. Either the installer put the new version in another folder than the one this app runs from, or the published build itself is still {version}.{named_text}", a.exe),
                Some(c) => format!("The installer for the update{target} stopped with code {c} and did not install it. Often another program was still using the files, or it needs administrator rights. This app is still {build}."),
                None => format!("The installer for the update{target} did not report a result, so it did not run or did not finish. This app is still {build}."),
            };
            let failed = Failed { key: a.key.clone(), note, installer: a.installer.clone(), from_version: a.from_version.clone(), from_sha: a.from_sha.clone() };
            remember(st, |m| {
                m.applied = None;
                m.failed = Some(failed);
            });
            let _ = app.emit("update-failed", info(st, "available", None, String::new()));
        } else {
            remember(st, |m| {
                m.applied = None;
                m.failed = None;
            });
        }
    } else if m.failed.as_ref().is_some_and(|f| !unchanged(&f.from_version, &f.from_sha)) {
        // Updated since (by hand, say): the earlier failure is over.
        remember(st, |m| m.failed = None);
    }
}

/// Looks once after the app opens, then every few hours while it stays open.
pub fn start(app: &AppHandle, st: Shared) {
    if !cfg!(desktop) {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("updates".into()).spawn(move || {
        // Looks soon after the window is up, not half a minute later.
        std::thread::sleep(Duration::from_secs(3));
        settle_pending(&app, &st);
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
        // The check that offered the update just told what to fetch: start with that, and only ask again when it is old.
        let latest = match fresh_find() {
            Some(l) => l,
            None => match check_now(&st) {
                Info { latest: Some(l), .. } => l,
                Info { message, .. } => return Err(message),
            },
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

/// Run the installer of the update that did not take effect with its own screens, so that what it asks and where it
/// installs can be seen. The app closes first if the installer needs it to (it asks).
#[tauri::command]
pub fn update_install_manual(state: State<'_, Shared>) -> CmdResult<()> {
    let st = state.inner();
    let m = memory(st);
    let installer = m.failed.map(|f| f.installer).or_else(|| m.ready.map(|r| r.installer)).filter(|p| p.exists()).ok_or("The downloaded installer is gone. Use Check now and download the update again.")?;
    fast_core::platform::open_path(&installer).map_err(|e| e.to_string())
}

/// Forget that the last update failed, so that it is offered (and downloaded) again.
#[tauri::command]
pub fn update_forget_failure(state: State<'_, Shared>) {
    remember(state.inner(), |m| m.failed = None);
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

#[cfg(all(test, desktop))]
mod download_tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;

    #[derive(Clone, Copy, PartialEq)]
    enum Server {
        Parts,
        /// Always sends the whole file, whatever is asked.
        Whole,
        /// Cuts the first few requests for a later part off halfway.
        Breaks,
    }

    fn data(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 31 % 251) as u8).collect()
    }

    /// A small file server on this computer; returns its address and how many connections it has seen.
    fn serve(body: Vec<u8>, mode: Server) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/file.bin", listener.local_addr().unwrap());
        let seen = Arc::new(AtomicUsize::new(0));
        let (count, body) = (seen.clone(), Arc::new(body));
        std::thread::spawn(move || {
            let cut = Arc::new(AtomicUsize::new(0));
            for conn in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::Relaxed);
                let (body, cut) = (body.clone(), cut.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(conn.try_clone().unwrap());
                    let mut range = None;
                    let mut line = String::new();
                    loop {
                        line.clear();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(r) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                            let (a, b) = r.trim().split_once('-').unwrap();
                            range = Some((a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()));
                        }
                    }
                    let mut out = conn;
                    let total = body.len();
                    let (status, a, b) = match (mode, range) {
                        (Server::Whole, _) | (_, None) => ("200 OK", 0, total - 1),
                        (_, Some((a, b))) => ("206 Partial Content", a, b.min(total - 1)),
                    };
                    let mut head = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n", b - a + 1);
                    if status.starts_with("206") {
                        head += &format!("Content-Range: bytes {a}-{b}/{total}\r\n");
                    }
                    let _ = out.write_all(format!("{head}\r\n").as_bytes());
                    let mut end = b + 1;
                    if mode == Server::Breaks && status.starts_with("206") && a > 0 && cut.fetch_add(1, Ordering::Relaxed) < 4 {
                        end = a + (b - a) / 2;
                    }
                    let _ = out.write_all(&body[a..end]);
                });
            }
        });
        (addr, seen)
    }

    fn fetch(url: &str, size: u64, cancel: &AtomicBool) -> (Result<u64, String>, PathBuf) {
        let dir = tempfile::tempdir().unwrap().keep();
        let dest = dir.join("out.bin");
        let calls = AtomicUsize::new(0);
        let progress = |_: u64, _: u64| {
            calls.fetch_add(1, Ordering::Relaxed);
        };
        (http::download(url, "", "application/octet-stream", &dest, size, cancel, &progress), dest)
    }

    #[test]
    fn parts_are_split_without_gaps_or_overlap() {
        for size in [1u64, 2, 7, 1000, (10 << 20) + 3] {
            for n in 1..=8 {
                let parts = http::split(size, n);
                assert_eq!(parts.first().unwrap().0, 0);
                assert_eq!(parts.last().unwrap().1, size - 1);
                assert!(parts.windows(2).all(|w| w[0].1 + 1 == w[1].0), "{size} in {n}: {parts:?}");
            }
        }
        assert_eq!(http::streams_for(100), 1);
        assert_eq!(http::streams_for(6 << 20), 3);
        assert_eq!(http::streams_for(500 << 20), 8);
    }

    #[test]
    fn a_big_file_arrives_whole_over_several_connections() {
        let body = data(9 << 20);
        let (url, seen) = serve(body.clone(), Server::Parts);
        let (r, dest) = fetch(&url, body.len() as u64, &AtomicBool::new(false));
        assert_eq!(r.unwrap(), body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(seen.load(Ordering::Relaxed) >= 4, "{} connections", seen.load(Ordering::Relaxed));
    }

    #[test]
    fn a_server_that_ignores_parts_is_still_fine() {
        let body = data(5 << 20);
        let (url, _) = serve(body.clone(), Server::Whole);
        let (r, dest) = fetch(&url, body.len() as u64, &AtomicBool::new(false));
        assert_eq!(r.unwrap(), body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[test]
    fn a_wrong_size_in_the_listing_does_not_matter() {
        let body = data(5 << 20);
        let (url, _) = serve(body.clone(), Server::Parts);
        // The listing said 8 MB; the file is 5.
        let (r, dest) = fetch(&url, 8 << 20, &AtomicBool::new(false));
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[test]
    fn a_connection_that_breaks_is_picked_up_where_it_stopped() {
        let body = data(9 << 20);
        let (url, _) = serve(body.clone(), Server::Breaks);
        let (r, dest) = fetch(&url, body.len() as u64, &AtomicBool::new(false));
        assert_eq!(r.unwrap(), body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body, "every byte is where it belongs");
    }

    #[test]
    fn a_small_file_uses_one_connection() {
        let body = data(300_000);
        let (url, seen) = serve(body.clone(), Server::Parts);
        let (r, dest) = fetch(&url, body.len() as u64, &AtomicBool::new(false));
        assert_eq!(r.unwrap(), body.len() as u64);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(seen.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancelling_stops_the_download() {
        let body = data(9 << 20);
        let (url, _) = serve(body.clone(), Server::Parts);
        let (r, _) = fetch(&url, body.len() as u64, &AtomicBool::new(true));
        assert_eq!(r.unwrap_err(), "cancelled");
    }
}

#[cfg(test)]
mod blocked_tests {
    use super::explain_blocked;

    #[test]
    fn antivirus_refusals_are_explained() {
        let raw = "C:\\x\\setup.exe: Operation did not complete successfully because the file contains a virus or potentially unwanted software. (os error 225)";
        assert!(explain_blocked(raw.into()).starts_with("Windows Security blocked"));
        assert!(explain_blocked("The system cannot find (os error 226)".into()).starts_with("Windows Security blocked"));
    }

    #[test]
    fn other_errors_pass_through() {
        assert_eq!(explain_blocked("the download stopped".into()), "the download stopped");
        assert_eq!(explain_blocked("Access denied (os error 5)".into()), "Access denied (os error 5)");
    }
}
