//! The system clipboard and drag-out, so files move between this app and
//! others (Explorer, Finder, Files, mail, chat, editors) like they do between
//! any two file managers.
//!
//! * Copy and Cut put real file references on the system clipboard (on Windows
//!   a file drop list, on macOS file URLs, on Linux a URI list), so Paste in
//!   another program works, and files copied elsewhere can be pasted here.
//! * Dragging items out of the window starts the operating system's own drag,
//!   carrying the files, so another program receives them as if dropped from a
//!   file manager.
//!
//! Neither is available on phones, where these commands report that.

use crate::commands::CmdResult;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod imp {
    use super::CmdResult;
    use clipboard_rs::{Clipboard, ClipboardContext};

    pub fn set_files(paths: Vec<String>) -> CmdResult<()> {
        let ctx = ClipboardContext::new().map_err(|e| e.to_string())?;
        ctx.set_files(paths).map_err(|e| e.to_string())
    }

    pub fn get_files() -> CmdResult<Vec<String>> {
        let ctx = ClipboardContext::new().map_err(|e| e.to_string())?;
        Ok(clean_paths(ctx.get_files().unwrap_or_default()))
    }

    /// Clipboards hand back file references in several spellings (plain paths,
    /// `file:///…` URIs with percent-escapes). Turn them into absolute paths and
    /// drop anything else.
    pub fn clean_paths(raw: Vec<String>) -> Vec<String> {
        raw.into_iter()
            .filter_map(|r| {
                let r = r.trim().trim_end_matches('\0');
                let p = match r.strip_prefix("file://") {
                    Some(rest) => {
                        // file://host/path: only the local form (empty host) is a path here.
                        let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                        let decoded = percent_decode(rest);
                        // file:///C:/dir on Windows has a leading slash before the drive letter.
                        if cfg!(windows) && decoded.len() > 2 && decoded.starts_with('/') && decoded.as_bytes()[2] == b':' {
                            decoded[1..].replace('/', "\\")
                        } else {
                            decoded
                        }
                    }
                    None => r.to_string(),
                };
                std::path::Path::new(&p).is_absolute().then_some(p)
            })
            .collect()
    }

    fn percent_decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    pub fn drag_out(window: &tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<()> {
        use std::path::PathBuf;
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).filter(|p| p.is_absolute() && p.exists()).collect();
        if files.is_empty() {
            return Err("Nothing to drag".into());
        }
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let win = window.clone();
        window
            .run_on_main_thread(move || {
                let icon = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
                let item = drag::DragItem::Files(files);
                #[cfg(target_os = "linux")]
                let result = match win.gtk_window() {
                    Ok(gtk) => drag::start_drag(&gtk, item, icon, |_, _| {}, drag::Options::default()).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                #[cfg(not(target_os = "linux"))]
                let result = drag::start_drag(&win, item, icon, |_, _| {}, drag::Options::default()).map_err(|e| e.to_string());
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or(Ok(()))
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::CmdResult;
    pub fn set_files(_: Vec<String>) -> CmdResult<()> {
        Err("Copying files to other apps is not available on this device.".into())
    }
    pub fn get_files() -> CmdResult<Vec<String>> {
        Ok(Vec::new())
    }
    pub fn drag_out(_: &tauri::WebviewWindow, _: Vec<String>) -> CmdResult<()> {
        Err("Dragging files to other apps is not available on this device.".into())
    }
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::imp::clean_paths;

    #[test]
    fn clipboard_paths_are_normalised() {
        let out = clean_paths(vec!["file:///tmp/a%20b.txt".into(), "/tmp/c.txt".into(), "relative.txt".into(), "file://localhost/tmp/d".into()]);
        if cfg!(windows) {
            return;
        }
        assert_eq!(out, ["/tmp/a b.txt", "/tmp/c.txt", "/tmp/d"]);
    }
}

/// Put these files on the system clipboard.
#[tauri::command]
pub async fn clipboard_set_files(paths: Vec<String>) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || imp::set_files(paths)).await.map_err(|e| e.to_string())?
}

/// Files currently on the system clipboard (put there by any program), or none.
#[tauri::command]
pub async fn clipboard_get_files() -> CmdResult<Vec<String>> {
    tauri::async_runtime::spawn_blocking(imp::get_files).await.map_err(|e| e.to_string())?
}

/// Start the operating system's drag with these files, for dropping into another program.
#[tauri::command]
pub async fn drag_out(window: tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<()> {
    imp::drag_out(&window, paths)
}
