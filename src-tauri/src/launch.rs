//! Being opened by something else: a folder on the command line, a second
//! start while the app is already running, "Show in folder" from another app,
//! or the macOS open-document event. All of them end up as [`Launch`] requests
//! that one window collects with `take_launch`.
//!
//! Also the settings switch that makes the app the system's file manager.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::filemanager::{self, Launch, Status};
use parking_lot::Mutex;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

/// Requests no window has collected yet. A plain static: requests can arrive
/// before the app's state exists (the very first moments of a start).
static PENDING: Mutex<Vec<Launch>> = Mutex::new(Vec::new());

/// The window that shows what other apps ask for: the main one, or the oldest
/// extra window when the main one has been closed.
fn receiver(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main").or_else(|| {
        let mut extra: Vec<(u64, WebviewWindow)> = app
            .webview_windows()
            .into_iter()
            .filter_map(|(label, w)| Some((label.strip_prefix("window-")?.parse().ok()?, w)))
            .collect();
        extra.sort_by_key(|(n, _)| *n);
        extra.into_iter().next().map(|(_, w)| w)
    })
}

/// Queue requests and bring the window that will show them to the front.
/// With nothing to show (the app was simply started a second time) the window still comes forward.
#[cfg_attr(target_os = "android", allow(dead_code))]
pub fn deliver(app: &AppHandle, launches: Vec<Launch>) {
    PENDING.lock().extend(launches);
    let Some(window) = receiver(app) else { return };
    #[cfg(desktop)]
    {
        let _ = window.unminimize();
        let _ = window.show();
    }
    let _ = window.set_focus();
    let _ = app.emit_to(window.label(), "launch", ());
}

/// What the app itself was started with.
#[cfg_attr(mobile, allow(dead_code))]
pub fn queue_own_arguments() {
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    PENDING.lock().extend(filemanager::parse_launch(&args, &cwd));
}

/// Hand over the folders other apps asked for. Only the receiving window gets
/// them, so a request is never shown twice.
#[tauri::command]
pub fn take_launch(app: AppHandle, window: WebviewWindow) -> Vec<Launch> {
    match receiver(&app) {
        Some(r) if r.label() == window.label() => filemanager::merge(std::mem::take(&mut *PENDING.lock())),
        _ => Vec::new(),
    }
}

fn saved_file(state: &Shared) -> PathBuf {
    state.paths.settings.with_file_name(filemanager::SAVED_FILE)
}

#[tauri::command]
pub async fn default_fm_status(state: State<'_, Shared>) -> CmdResult<Status> {
    let file = saved_file(state.inner());
    tauri::async_runtime::spawn_blocking(move || filemanager::status(&file)).await.map_err(|e| e.to_string())
}

/// Make the app the system's file manager (`on`), or put back what was there before.
#[tauri::command]
pub async fn default_fm_set(app: AppHandle, state: State<'_, Shared>, on: bool) -> CmdResult<Status> {
    let file = saved_file(state.inner());
    let id = app.config().identifier.clone();
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let status = if on {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            filemanager::set_default(&file, &exe, &id)
        } else {
            filemanager::restore(&file)
        }
        .map_err(|e| e.to_string())?;
        sync_service(&handle, status.active);
        Ok(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// At startup: keep the registration pointing at this copy of the app (it may
/// have been moved or updated), and answer "Show in folder" when the app is the default.
#[cfg_attr(mobile, allow(dead_code))]
pub fn start(app: &AppHandle) {
    let Some(state) = app.try_state::<Shared>() else { return };
    let file = saved_file(state.inner());
    let handle = app.clone();
    let id = app.config().identifier.clone();
    let _ = std::thread::Builder::new().name("file-manager".into()).spawn(move || {
        let status = filemanager::status(&file);
        if !status.active {
            return;
        }
        // The service first: an app that started this one for "Show in folder" is waiting for it.
        sync_service(&handle, true);
        if cfg!(any(windows, target_os = "linux")) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = filemanager::set_default(&file, &exe, &id);
            }
        }
    });
}

/// Linux: offer (or stop offering) the "show in folder" service other apps call.
#[cfg(target_os = "linux")]
fn sync_service(app: &AppHandle, active: bool) {
    static SERVICE: Mutex<Option<filemanager::service::Service>> = Mutex::new(None);
    let mut service = SERVICE.lock();
    if !active {
        *service = None;
    } else if service.is_none() {
        let handle = app.clone();
        *service = filemanager::service::serve(None, move |launches| deliver(&handle, launches)).ok();
    }
}

#[cfg(not(target_os = "linux"))]
fn sync_service(_app: &AppHandle, _active: bool) {}
