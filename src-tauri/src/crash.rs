//! Crash log commands, and the optional sending of reports to the project's GitHub issues.
//! Sending is off until the person turns it on in Settings (`crash.github`), or presses Send on one report.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::crashlog;
use serde::Serialize;
use tauri::State;

/// A read-and-issue token for the project's repository, put in at build time (`FAST_CRASH_TOKEN`).
/// It should be a fine-grained token that can only write issues on that one repository.
const TOKEN: Option<&str> = option_env!("FAST_CRASH_TOKEN");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInfo {
    pub name: String,
    pub bytes: u64,
    pub modified: u64,
    pub sent: bool,
}

#[tauri::command]
pub fn crash_note(lines: Vec<String>) {
    for l in lines.iter().take(200) {
        crashlog::note(&l.chars().take(300).collect::<String>());
    }
}

#[tauri::command]
pub fn crash_context(text: String) {
    crashlog::set_context(&text.chars().take(2000).collect::<String>());
}

/// The page caught an error of its own.
#[tauri::command]
pub fn crash_report(kind: String, message: String) {
    crashlog::note(&format!("!! page error: {}", message.lines().next().unwrap_or("").chars().take(200).collect::<String>()));
    crashlog::flush();
    crashlog::report(&kind.chars().take(60).collect::<String>(), &message.chars().take(8000).collect::<String>(), None);
}

#[tauri::command]
pub fn crash_logs(state: State<'_, Shared>) -> Vec<LogInfo> {
    crashlog::list(&state.paths.crash).into_iter().map(|l| LogInfo { name: l.name, bytes: l.bytes, modified: l.modified, sent: l.sent }).collect()
}

fn safe_name(name: &str) -> bool {
    name.starts_with("crash-") && name.ends_with(".txt") && !name.contains(['/', '\\'])
}

#[tauri::command]
pub fn crash_log_read(state: State<'_, Shared>, name: String) -> CmdResult<String> {
    if !safe_name(&name) {
        return Err("Not a crash log".into());
    }
    std::fs::read_to_string(state.paths.crash.join(name)).map_err(|e| e.to_string())
}

/// Sends one report to the project's issues. Without a token built in, opens a pre-filled "new issue"
/// page in the browser instead (the person signs in there and presses Submit).
#[tauri::command]
pub async fn crash_send(state: State<'_, Shared>, name: String) -> CmdResult<String> {
    if !safe_name(&name) {
        return Err("Not a crash log".into());
    }
    let st = state.inner().clone();
    let repo = st.settings.read().updates.repo.clone();
    let path = st.paths.crash.join(&name);
    tauri::async_runtime::spawn_blocking(move || send(&repo, &path)).await.map_err(|e| e.to_string())?
}

#[cfg(any(target_os = "android", target_os = "ios"))]
fn send(_repo: &str, _path: &std::path::Path) -> CmdResult<String> {
    Err("Sending crash reports is available on computers only. Copy the report from Settings > Storage instead.".into())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn send(repo: &str, path: &std::path::Path) -> CmdResult<String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let kind = text.lines().find_map(|l| l.strip_prefix("Kind: ")).unwrap_or("Crash").to_string();
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let title = format!("Crash report: {kind} (v{version}, {os})");
    let Some(token) = TOKEN.filter(|t| !t.is_empty()) else {
        // No token in this build: let the person file it themselves, with as much as fits in a link.
        // A link can only be about 8000 characters long once encoded: keep the start (problem, computer, state)
        // and the end (the last events), which are the parts that matter.
        let note = "\n\n(Shortened to fit a link. The full report was copied to the clipboard; paste it here if you can.)";
        let (mut head, mut tail) = (2200usize, 1800usize);
        let url = loop {
            let chars: Vec<char> = text.chars().collect();
            let body = if chars.len() <= head + tail {
                format!("```\n{text}\n```")
            } else {
                format!("```\n{}\n[...]\n{}\n```{note}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
            };
            let url = format!("https://github.com/{repo}/issues/new?labels=crash&title={}&body={}", enc(&title), enc(&body));
            if url.len() <= 7500 || head < 400 {
                break url;
            }
            head = head * 3 / 4;
            tail = tail * 3 / 4;
        };
        open::that_detached(url).map_err(|e| e.to_string())?;
        return Ok("Opened a new issue in your browser. Press Submit there to send it.".into());
    };
    let body: String = text.chars().take(60000).collect();
    let payload = serde_json::json!({ "title": title, "body": format!("```\n{body}\n```"), "labels": ["crash"] });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_global(Some(std::time::Duration::from_secs(40)))
        .http_status_as_error(false)
        .user_agent("FAST-File-Explorer-crash")
        .build()
        .into();
    let mut resp = agent
        .post(&format!("https://api.github.com/repos/{repo}/issues"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .content_type("application/json")
        .send(payload.to_string())
        .map_err(|e| e.to_string())?;
    let code = resp.status().as_u16();
    if code == 201 {
        crashlog::mark_sent(path);
        let url = resp.body_mut().read_to_string().ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).and_then(|v| v["html_url"].as_str().map(str::to_string)).unwrap_or_default();
        if !url.is_empty() {
            let _ = open::that_detached(&url);
        }
        Ok(format!("Sent. {url}").trim().to_string())
    } else {
        Err(format!("GitHub answered {code}"))
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn enc(s: &str) -> String {
    let mut o = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// At start: if sending is on, send reports that were not sent yet (a few at most, quietly).
pub fn send_pending(st: &crate::state::AppState) {
    let on = st.settings.read().advanced.get("crash.github").and_then(|v| v.as_bool()).unwrap_or(false);
    if !on || TOKEN.map_or(true, str::is_empty) {
        return;
    }
    let repo = st.settings.read().updates.repo.clone();
    let todo: Vec<_> = crashlog::list(&st.paths.crash).into_iter().filter(|l| !l.sent).take(3).collect();
    if todo.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for l in todo {
            if send(&repo, &l.path).is_err() {
                break;
            }
        }
    });
}

static PREVIOUS: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

pub fn remember_previous(path: Option<std::path::PathBuf>) {
    if let (Some(p), Ok(mut g)) = (path, PREVIOUS.lock()) {
        *g = p.file_name().map(|n| n.to_string_lossy().into_owned());
    }
}

/// The report about how the last run ended, once (so the page can tell the person).
#[tauri::command]
pub fn crash_previous() -> Option<String> {
    PREVIOUS.lock().ok().and_then(|mut g| g.take())
}

/// Where the reports are kept.
#[tauri::command]
pub fn crash_folder(state: State<'_, Shared>) -> String {
    state.paths.crash.to_string_lossy().into_owned()
}

/// Opens the folder of reports in the system's file manager.
#[tauri::command]
pub fn crash_open_folder(state: State<'_, Shared>) -> CmdResult<()> {
    let dir = state.paths.crash.clone();
    let _ = std::fs::create_dir_all(&dir);
    fast_core::platform::open_path(&dir).map_err(|e| e.to_string())
}

/// The page reports in every second. Cheap on purpose: no logging, one lock-free store.
#[tauri::command]
pub fn crash_heartbeat(state: State<'_, Shared>, visible: bool) {
    let (close, secs) = {
        let s = state.settings.read();
        (s.advanced.get("crash.closeWhenFrozen").and_then(|v| v.as_bool()).unwrap_or(true), s.advanced.get("crash.freezeSeconds").and_then(|v| v.as_u64()).unwrap_or(15))
    };
    crashlog::heartbeat(visible, close, secs);
}

/// Saves a copy of one report where the person chooses (a Save dialog). Returns where, or nothing if cancelled.
#[tauri::command]
pub async fn crash_log_save(app: tauri::AppHandle, state: State<'_, Shared>, name: String) -> CmdResult<Option<String>> {
    if !safe_name(&name) {
        return Err("Not a crash log".into());
    }
    let from = state.paths.crash.join(&name);
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let Some(to) = app.dialog().file().set_file_name(&name).add_filter("Text file", &["txt"]).blocking_save_file() else {
            return Ok(None);
        };
        let to = to.into_path().map_err(|e| e.to_string())?;
        std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        Ok(Some(to.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One way of writing a file the person is saving: the label for the Save dialog, the extension, the contents.
#[derive(serde::Deserialize)]
pub struct SaveVariant {
    pub label: String,
    pub ext: String,
    pub contents: String,
}

/// Asks where to save (a Save dialog offering each variant as a file type) and writes the one whose type was chosen.
/// Returns where it went, or nothing if the person cancelled.
#[tauri::command]
pub async fn save_text_file(app: tauri::AppHandle, name: String, variants: Vec<SaveVariant>) -> CmdResult<Option<String>> {
    if variants.is_empty() {
        return Err("Nothing to save".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " ._-()".contains(c) { c } else { '_' }).collect();
        let mut dialog = app.dialog().file().set_file_name(format!("{safe}.{}", variants[0].ext));
        for v in &variants {
            dialog = dialog.add_filter(&v.label, &[v.ext.as_str()]);
        }
        let Some(to) = dialog.blocking_save_file() else { return Ok(None) };
        let mut to = to.into_path().map_err(|e| e.to_string())?;
        // The extension typed (or chosen) decides the format; with none, the first.
        let chosen = to.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        let variant = variants.iter().find(|v| Some(v.ext.to_ascii_lowercase()) == chosen);
        let variant = match variant {
            Some(v) => v,
            None => {
                let v = &variants[0];
                let mut os = to.into_os_string();
                os.push(format!(".{}", v.ext));
                to = std::path::PathBuf::from(os);
                v
            }
        };
        std::fs::write(&to, variant.contents.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Some(to.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
