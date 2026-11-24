//! Crash log commands, and the sending of reports to the developer through the project's bug report form.
//! Automatic sending is off until the person turns it on in Settings (`crash.github`), or presses Send on one report.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::crashlog;
use serde::Serialize;
use tauri::State;

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

/// Sends one report to the developer: it is filled into the project's bug report form (a Google Form that needs no
/// sign-in and collects no e-mail address), so there is nothing for the person to do.
#[tauri::command]
pub async fn crash_send(state: State<'_, Shared>, name: String) -> CmdResult<String> {
    if !safe_name(&name) {
        return Err("Not a crash log".into());
    }
    let path = state.paths.crash.join(&name);
    tauri::async_runtime::spawn_blocking(move || send(&path)).await.map_err(|e| e.to_string())?
}

/// Where the form takes its answers, and the one question in it (a paragraph box).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const FORM_URL: &str = "https://docs.google.com/forms/d/e/1FAIpQLSdoBKWIIjyzHu5WcpxBid2BwjiaEnd_gwVwOiTb3Oze0sJBIw/formResponse";
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const FORM_FIELD: &str = "entry.1859183185";
/// Longest answer sent. A longer report keeps its start (problem, computer, state) and its end (the last events).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
const FORM_MAX_CHARS: usize = 40_000;

#[cfg(any(target_os = "android", target_os = "ios"))]
fn send(_path: &std::path::Path) -> CmdResult<String> {
    Err("Sending crash reports is available on computers only. Copy the report from Settings > Storage instead.".into())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn send(path: &std::path::Path) -> CmdResult<String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let body = form_body(&text);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(15)))
        .timeout_global(Some(std::time::Duration::from_secs(40)))
        .http_status_as_error(false)
        // A redirect is how Google says "sign in first": it must be seen, not followed.
        .max_redirects(0)
        .user_agent("FAST-File-Explorer-crash")
        .build()
        .into();
    let resp = agent.post(FORM_URL).content_type("application/x-www-form-urlencoded").send(body).map_err(|e| format!("Could not reach Google Forms: {e}. Check the internet connection and try again."))?;
    let location = resp.headers().get("location").and_then(|v| v.to_str().ok()).map(str::to_string);
    outcome(resp.status().as_u16(), location.as_deref())?;
    crashlog::mark_sent(path);
    Ok("Sent to the developer. Thank you.".into())
}

/// What the form's answer means. Success is a plain 200; a redirect to Google's sign-in, or a 401/403, means the form
/// asks for an account now.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn outcome(code: u16, location: Option<&str>) -> Result<(), String> {
    match code {
        200 => Ok(()),
        301 | 302 | 303 | 307 | 308 if location.is_some_and(|l| l.contains("accounts.google.")) => Err("Google asked for a sign-in before it would take the report. Sign in to a Google account in your browser, then try again.".into()),
        401 | 403 => Err("Google asked for a sign-in before it would take the report. Sign in to a Google account in your browser, then try again.".into()),
        301 | 302 | 303 | 307 | 308 => Err(format!("Google Forms redirected the report instead of taking it ({code}). Try again later.")),
        400 | 404 => Err(format!("Google Forms did not accept the report ({code}); the form may have been changed or closed. Save the crash log as a text file and send that instead.")),
        429 => Err("Google Forms says there were too many reports just now. Try again in a few minutes.".into()),
        other => Err(format!("Google Forms answered {other}. Try again later.")),
    }
}

/// The answer as the form wants it: one field, the whole report, shortened in the middle if it is very long.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn form_body(text: &str) -> String {
    format!("{FORM_FIELD}={}", enc(&shorten(text, FORM_MAX_CHARS)))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn shorten(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let (head, tail) = (max * 3 / 10, max * 7 / 10);
    format!("{}\n[... shortened ...]\n{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
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

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod form_tests {
    use super::*;

    #[test]
    fn the_report_goes_into_the_one_field() {
        let b = form_body("Kind: Crash\nline two & more=");
        assert_eq!(b, "entry.1859183185=Kind%3A%20Crash%0Aline%20two%20%26%20more%3D");
    }

    #[test]
    fn a_long_report_keeps_its_start_and_its_end() {
        let text: String = (0..100_000).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let s = shorten(&text, 1000);
        assert!(s.chars().count() < 1100);
        assert!(s.starts_with(&text[..300]));
        assert!(s.ends_with(&text[text.len() - 700..]));
        assert!(s.contains("[... shortened ...]"));
        assert_eq!(shorten("short", 1000), "short");
    }

    #[test]
    fn google_answers_are_read() {
        assert!(outcome(200, None).is_ok());
        assert!(outcome(302, Some("https://accounts.google.com/ServiceLogin?x=1")).unwrap_err().contains("sign-in"));
        assert!(outcome(401, None).unwrap_err().contains("sign-in"));
        assert!(outcome(302, Some("https://docs.google.com/forms/x")).unwrap_err().contains("redirected"));
        assert!(outcome(400, None).unwrap_err().contains("did not accept"));
        assert!(outcome(429, None).unwrap_err().contains("too many"));
        assert!(outcome(500, None).unwrap_err().contains("500"));
    }
}

/// At start: if sending is on, send reports that were not sent yet (a few at most, quietly).
pub fn send_pending(st: &crate::state::AppState) {
    let on = st.settings.read().advanced.get("crash.github").and_then(|v| v.as_bool()).unwrap_or(false);
    if !on {
        return;
    }
    let todo: Vec<_> = crashlog::list(&st.paths.crash).into_iter().filter(|l| !l.sent).take(3).collect();
    if todo.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for l in todo {
            if send(&l.path).is_err() {
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
