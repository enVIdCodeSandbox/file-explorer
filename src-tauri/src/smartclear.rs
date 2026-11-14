//! Smart Clear: the commands behind the page. The rules live in `fast_core::smartclear`.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::smartclear::{self, Options, Report};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// The switch that stops the scan in progress.
static CANCEL: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Look for what can be cleared on one drive (`drive` is its mount point) or, with no drive,
/// on every local drive. Progress goes out as `smart-clear-progress` events.
#[tauri::command]
pub async fn smart_clear_scan(app: AppHandle, state: State<'_, Shared>, drive: Option<String>, options: Option<Options>) -> CmdResult<Report> {
    let st = state.inner().clone();
    let cancel = Arc::new(AtomicBool::new(false));
    // A new scan replaces the one before it.
    if let Some(old) = CANCEL.lock().replace(cancel.clone()) {
        old.store(true, Ordering::Relaxed);
    }
    tauri::async_runtime::spawn_blocking(move || {
        // Local drives that can be written to; network shares and read-only media have nothing to clear.
        let mounts: Vec<PathBuf> = st
            .drives
            .read()
            .iter()
            .filter(|d| d.online && !d.read_only && d.remote.is_none())
            .map(|d| PathBuf::from(&d.mount))
            .collect();
        let drive = drive.map(PathBuf::from);
        if let Some(d) = &drive {
            if !mounts.contains(d) {
                return Err("That drive cannot be cleared: it is read-only, on the network, or no longer connected.".to_string());
            }
        }
        // The app's own data is left alone (its caches have their own page in Settings),
        // and so is everything behind a passcode.
        let mut own: Vec<PathBuf> = Vec::new();
        own.extend(st.paths.settings.parent().map(Path::to_path_buf));
        own.extend(st.paths.thumbs.parent().map(Path::to_path_buf));
        let locked: Vec<PathBuf> = st.locks.read().file.locks.iter().map(|l| PathBuf::from(&l.path)).collect();
        own.extend(locked.iter().cloned());
        let mut places = smartclear::places_for(drive.as_deref(), &mounts, &own);
        for list in [&mut places.temp, &mut places.caches, &mut places.thumbnails, &mut places.trash, &mut places.walk] {
            list.retain(|p| !locked.iter().any(|l| p.starts_with(l)));
        }
        let options = options.unwrap_or_default();
        Ok(smartclear::scan(&places, &options, &cancel, &|p| {
            let _ = app.emit("smart-clear-progress", p);
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn smart_clear_cancel() {
    if let Some(cancel) = CANCEL.lock().as_ref() {
        cancel.store(true, Ordering::Relaxed);
    }
}
