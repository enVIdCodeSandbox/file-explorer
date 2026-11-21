//! Dragging a whole window onto another one.
//!
//! The operating system moves a window while its title bar is held, and the app cannot see the mouse until the button
//! is let go. So the move is watched from the outside: when a window is moved by its title bar, with the pointer over
//! another app window, the other window is outlined. If the window is then held still there for a moment (`dwell`), it
//! offers: the dragged window minimizes (so it is out of the way), and the window underneath asks which of its tabs to
//! take, and whether as tabs or as split panes. Cancelling brings the dragged window back. Nothing is moved until the
//! person has chosen, so a pause by accident costs only a click.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// A window's place on the desktop, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

/// Whether the pointer is in the top strip of a window: where a window is held to be dragged.
pub fn on_title_strip(px: f64, py: f64, window: &Rect, strip: f64) -> bool {
    window.contains(px, py) && py <= window.y + strip
}

/// The other window the pointer is over (the first one in the list, so the caller orders them top first).
pub fn window_under(px: f64, py: f64, windows: &[(String, Rect)], source: &str) -> Option<String> {
    windows.iter().find(|(label, r)| label != source && r.contains(px, py)).map(|(l, _)| l.clone())
}

/// Windows that hold folders (the main one and those made from tabs), not Settings or Transfers.
pub fn is_folder_window(label: &str) -> bool {
    label == "main" || label.starts_with("window-")
}

/// Counts moves, so a waiter can tell whether the window has been still since it started waiting.
static MOVES: AtomicU64 = AtomicU64::new(0);
/// A waiter is running.
static WAITING: AtomicBool = AtomicBool::new(false);
/// The window that moved last, and the window currently outlined.
static SOURCE: Mutex<Option<String>> = Mutex::new(None);
static OUTLINED: Mutex<Option<String>> = Mutex::new(None);

/// Logical height of the strip a window is held by, in the title bar and tab strip.
const GRAB_STRIP: f64 = 90.0;

#[derive(Serialize, Clone)]
struct Hover {
    on: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OfferTab {
    pub id: String,
    pub location: String,
}

#[derive(Serialize, Clone)]
struct Offer {
    source: String,
    tabs: Vec<OfferTab>,
}

fn enabled(app: &AppHandle) -> bool {
    let st = app.state::<crate::state::Shared>();
    crate::commands::advanced_bool(&st, "windows.mergeByDrag", true)
}

fn dwell(app: &AppHandle) -> Duration {
    let st = app.state::<crate::state::Shared>();
    Duration::from_millis(crate::commands::advanced_u64(&st, "windows.mergeDwellMs", 900).clamp(300, 5000))
}

/// The folder windows that are showing, with their places, in the order the system lists them.
fn open_windows(app: &AppHandle) -> Vec<(String, Rect)> {
    app.webview_windows()
        .into_iter()
        .filter(|(label, w)| is_folder_window(label) && !w.is_minimized().unwrap_or(false) && w.is_visible().unwrap_or(true))
        .filter_map(|(label, w)| {
            let (p, s) = (w.outer_position().ok()?, w.outer_size().ok()?);
            Some((label, Rect { x: p.x as f64, y: p.y as f64, w: s.width as f64, h: s.height as f64 }))
        })
        .collect()
}

/// The window under the pointer, if `source` is being held by its title strip.
fn probe(app: &AppHandle, source: &str) -> Option<String> {
    let cursor = app.cursor_position().ok()?;
    let windows = open_windows(app);
    let (_, mine) = windows.iter().find(|(l, _)| l == source)?;
    let scale = app.get_webview_window(source)?.scale_factor().unwrap_or(1.0);
    if !on_title_strip(cursor.x, cursor.y, mine, GRAB_STRIP * scale) {
        return None;
    }
    window_under(cursor.x, cursor.y, &windows, source)
}

fn outline(app: &AppHandle, target: Option<String>) {
    let mut now = OUTLINED.lock();
    if *now == target {
        return;
    }
    if let Some(old) = now.take() {
        let _ = app.emit_to(old.as_str(), "merge-hover", Hover { on: false });
    }
    if let Some(new) = &target {
        let _ = app.emit_to(new.as_str(), "merge-hover", Hover { on: true });
    }
    *now = target;
}

/// A window moved. Called for every move event of every window.
pub fn moved(window: &tauri::Window) {
    let label = window.label();
    if !is_folder_window(label) {
        return;
    }
    let app = window.app_handle();
    if !enabled(app) || !window.is_focused().unwrap_or(false) {
        return;
    }
    MOVES.fetch_add(1, Ordering::SeqCst);
    *SOURCE.lock() = Some(label.to_string());
    let target = probe(app, label);
    outline(app, target.clone());
    if target.is_none() || WAITING.swap(true, Ordering::SeqCst) {
        return;
    }
    let (app, wait) = (app.clone(), dwell(app));
    std::thread::spawn(move || {
        loop {
            let seen = MOVES.load(Ordering::SeqCst);
            std::thread::sleep(wait);
            if MOVES.load(Ordering::SeqCst) != seen {
                // Still moving: wait for it to stop.
                continue;
            }
            break;
        }
        WAITING.store(false, Ordering::SeqCst);
        let Some(source) = SOURCE.lock().clone() else { return };
        // Held still over a window: ask the dragged one what it has.
        match probe(&app, &source) {
            Some(target) => {
                outline(&app, None);
                let _ = app.emit_to(source.as_str(), "merge-offer-request", serde_json::json!({ "target": target }));
            }
            None => outline(&app, None),
        }
    });
}

/// The dragged window says which tabs it has: minimize it and let the window underneath offer to take them.
#[tauri::command]
pub async fn merge_offer(app: AppHandle, window: tauri::WebviewWindow, target: String, tabs: Vec<OfferTab>) -> Result<(), String> {
    if !is_folder_window(&target) || target == window.label() {
        return Err("That window cannot take tabs.".into());
    }
    let Some(other) = app.get_webview_window(&target) else { return Err("The window is gone.".into()) };
    #[cfg(desktop)]
    {
        let _ = window.minimize();
        let _ = other.unminimize();
    }
    other.set_focus().map_err(|e| e.to_string())?;
    app.emit_to(target.as_str(), "merge-offer", Offer { source: window.label().to_string(), tabs }).map_err(|e| e.to_string())
}

/// Nothing was taken: bring the dragged window back.
#[tauri::command]
pub async fn merge_cancel(app: AppHandle, source: String) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(&source) {
        #[cfg(desktop)]
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    Ok(())
}

/// These tabs were taken: the window they came from lets them go.
#[tauri::command]
pub async fn merge_done(app: AppHandle, source: String, ids: Vec<String>) -> Result<(), String> {
    app.emit_to(source.as_str(), "merge-release", serde_json::json!({ "ids": ids })).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn the_window_underneath_is_found_but_never_the_dragged_one() {
        let windows = vec![("window-2".to_string(), r(100.0, 100.0, 800.0, 600.0)), ("main".to_string(), r(0.0, 0.0, 1000.0, 700.0))];
        assert_eq!(window_under(150.0, 120.0, &windows, "window-2"), Some("main".into()));
        assert_eq!(window_under(150.0, 120.0, &windows, "main"), Some("window-2".into()));
        assert_eq!(window_under(1500.0, 120.0, &windows, "window-2"), None);
        assert_eq!(window_under(50.0, 50.0, &windows, "window-2"), Some("main".into()));
        assert_eq!(window_under(50.0, 50.0, &windows[..1], "window-2"), None);
    }

    #[test]
    fn a_window_is_only_held_by_its_top_strip() {
        let w = r(100.0, 100.0, 800.0, 600.0);
        assert!(on_title_strip(300.0, 130.0, &w, 90.0));
        assert!(!on_title_strip(300.0, 400.0, &w, 90.0));
        assert!(!on_title_strip(50.0, 130.0, &w, 90.0));
    }

    #[test]
    fn only_folder_windows_take_part() {
        assert!(is_folder_window("main") && is_folder_window("window-3"));
        assert!(!is_folder_window("settings") && !is_folder_window("transfers"));
    }
}
