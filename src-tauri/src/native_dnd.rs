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
use crate::state::Shared;
use serde::Serialize;
use tauri::State;

/// What another program put on the clipboard, as far as files go.
#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ClipFiles {
    pub paths: Vec<String>,
    /// The other program cut them (paste means move).
    pub cut: bool,
    /// The files exist only inside the other program; `clipboard_stage_virtual` saves them to real files.
    pub virtual_files: bool,
}

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

    /// Everything the clipboard says about files: where they are, whether the other program
    /// cut them, and whether they exist only inside that program (a zip entry, a phone file).
    pub fn get_all() -> CmdResult<super::ClipFiles> {
        let ctx = ClipboardContext::new().map_err(|e| e.to_string())?;
        let mut paths = clean_paths(ctx.get_files().unwrap_or_default());
        #[cfg_attr(not(any(target_os = "linux", windows)), allow(unused_mut))]
        let mut cut = false;
        // Linux file managers: GNOME's own list says copy or cut; KDE marks a cut separately.
        #[cfg(target_os = "linux")]
        {
            if let Ok(buf) = ctx.get_buffer("x-special/gnome-copied-files") {
                let (is_cut, uris) = parse_gnome_copied(&String::from_utf8_lossy(&buf));
                cut = is_cut;
                if paths.is_empty() {
                    paths = clean_paths(uris);
                }
            }
            if paths.is_empty() {
                if let Ok(buf) = ctx.get_buffer("text/uri-list") {
                    paths = clean_paths(parse_uri_list(&String::from_utf8_lossy(&buf)));
                }
            }
            if let Ok(buf) = ctx.get_buffer("application/x-kde-cutselection") {
                if buf.first() == Some(&b'1') {
                    cut = true;
                }
            }
        }
        // Windows: Explorer says what Cut and Copy meant in "Preferred DropEffect".
        #[cfg(windows)]
        {
            if let Ok(buf) = ctx.get_buffer("Preferred DropEffect") {
                if buf.len() >= 4 {
                    cut = is_move_effect(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]));
                }
            }
        }
        // "Copy as path" and some editors put plain paths on the clipboard as text.
        if paths.is_empty() {
            if let Ok(text) = ctx.get_text() {
                paths = existing_paths_in_text(&text);
            }
        }
        let virtual_files = paths.is_empty() && fast_core::virtual_clip::has_virtual_files();
        Ok(super::ClipFiles { paths, cut, virtual_files })
    }

    pub fn clear() -> CmdResult<()> {
        let ctx = ClipboardContext::new().map_err(|e| e.to_string())?;
        ctx.clear().map_err(|e| e.to_string())
    }

    /// GNOME's `x-special/gnome-copied-files`: the first line is `copy` or `cut`, then one URI per line.
    #[cfg(any(target_os = "linux", test))]
    pub fn parse_gnome_copied(text: &str) -> (bool, Vec<String>) {
        let mut lines = text.lines();
        let first = lines.next().unwrap_or("").trim().to_lowercase();
        let (cut, rest): (bool, Vec<String>) = match first.as_str() {
            "cut" => (true, lines.map(String::from).collect()),
            "copy" => (false, lines.map(String::from).collect()),
            // No verb: the first line was already a URI.
            _ => (false, text.lines().map(String::from).collect()),
        };
        (cut, rest.into_iter().filter(|l| !l.trim().is_empty()).collect())
    }

    /// `text/uri-list`: one URI per line, `#` starts a comment.
    #[cfg(any(target_os = "linux", test))]
    pub fn parse_uri_list(text: &str) -> Vec<String> {
        text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect()
    }

    /// Lines of text that name files or folders that exist (quotes around them are removed).
    pub fn existing_paths_in_text(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().map(|l| l.trim().trim_matches('"')).filter(|l| !l.is_empty()).collect();
        if lines.is_empty() || lines.len() > 500 {
            return Vec::new();
        }
        let paths = clean_paths(lines.iter().map(|l| l.to_string()).collect());
        // Text that merely contains one path-like line among others is just text.
        if paths.len() == lines.len() && paths.iter().all(|p| std::path::Path::new(p).exists()) {
            paths
        } else {
            Vec::new()
        }
    }

    /// Explorer's Preferred DropEffect: move without copy means Cut.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn is_move_effect(effect: u32) -> bool {
        effect & 2 != 0 && effect & 1 == 0
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

    /// The path the drag library will hand to the shell: it makes every path absolute and "real" first, which
    /// turns a mapped network drive (`U:\\folder`) into the share it points at (`\\\\server\\share\\folder`).
    #[cfg(windows)]
    fn as_the_library_sees_it(p: &std::path::Path) -> std::path::PathBuf {
        let real = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = real.to_string_lossy().into_owned();
        if let Some(rest) = s.strip_prefix("\\\\?\\UNC\\") {
            return std::path::PathBuf::from(format!("\\\\{rest}"));
        }
        std::path::PathBuf::from(s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string())
    }

    /// Windows builds the drag from `ILCreateFromPathW`, which answers null for a path the shell cannot
    /// resolve right now. The drag library passes that null on unchecked and the process dies inside the
    /// system, where nothing can catch it. Ask first, with the same path the library will use.
    #[cfg(windows)]
    fn shell_can_describe(p: &std::path::Path) -> bool {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "shell32")]
        extern "system" {
            fn ILCreateFromPathW(path: *const u16) -> *mut core::ffi::c_void;
            fn ILFree(pidl: *mut core::ffi::c_void);
        }
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
        let pidl = unsafe { ILCreateFromPathW(wide.as_ptr()) };
        if pidl.is_null() {
            return false;
        }
        unsafe { ILFree(pidl) };
        true
    }

    pub fn drag_out(window: &tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<()> {
        use std::path::PathBuf;
        let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).filter(|p| p.is_absolute() && p.exists()).collect();
        if files.is_empty() {
            return Err("Nothing to drag".into());
        }
        fast_core::crashlog::note(&format!("drag out: {} item(s), first {}", files.len(), files[0].display()));
        #[cfg(windows)]
        for f in &files {
            let seen = as_the_library_sees_it(f);
            if seen != *f {
                fast_core::crashlog::note(&format!("drag out: the system will see {} as {}", f.display(), seen.display()));
            }
            if !shell_can_describe(&seen) {
                fast_core::crashlog::note(&format!("drag out refused: Windows cannot describe {}", seen.display()));
                fast_core::crashlog::flush();
                return Err(format!("Windows could not prepare \"{}\" for dragging out of the app (it is on a network drive that did not answer). Use Copy, then Paste in the other program.", f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
        }
        fast_core::crashlog::note("drag out: handing over to the system");
        fast_core::crashlog::flush();
        let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let win = window.clone();
        window
            .run_on_main_thread(move || {
                // A panic here would take the whole window down; report it as an error instead.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let icon = drag::Image::Raw(include_bytes!("../icons/32x32.png").to_vec());
                let item = drag::DragItem::Files(files);
                #[cfg(target_os = "linux")]
                let result = match win.gtk_window() {
                    Ok(gtk) => drag::start_drag(&gtk, item, icon, |_, _| {}, drag::Options::default()).map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                #[cfg(not(target_os = "linux"))]
                let result = drag::start_drag(&win, item, icon, |_, _| {}, drag::Options::default()).map_err(|e| e.to_string());
                result
                }));
                let _ = tx.send(outcome.unwrap_or_else(|_| Err("The system could not start the drag for this file".to_string())));
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
    pub fn get_all() -> CmdResult<super::ClipFiles> {
        Ok(super::ClipFiles::default())
    }
    pub fn clear() -> CmdResult<()> {
        Ok(())
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

    #[test]
    fn gnome_and_kde_lists_are_read() {
        use super::imp::{existing_paths_in_text, is_move_effect, parse_gnome_copied, parse_uri_list};
        assert_eq!(parse_gnome_copied("copy\nfile:///a%20b\nfile:///c\n"), (false, vec!["file:///a%20b".to_string(), "file:///c".to_string()]));
        assert_eq!(parse_gnome_copied("cut\nfile:///a"), (true, vec!["file:///a".to_string()]));
        assert_eq!(parse_gnome_copied("file:///a\nfile:///b"), (false, vec!["file:///a".to_string(), "file:///b".to_string()]));
        assert_eq!(parse_uri_list("# comment\r\nfile:///a\r\n\r\nfile:///b\r\n"), ["file:///a", "file:///b"]);
        assert!(is_move_effect(2));
        assert!(!is_move_effect(1));
        assert!(!is_move_effect(5), "Explorer's Copy sets copy and link");
        // Paths pasted as text only count when every line is a real file.
        let dir = std::env::temp_dir();
        let here = dir.to_string_lossy().into_owned();
        if !cfg!(windows) {
            assert_eq!(existing_paths_in_text(&format!("\"{here}\"\n")), [here.clone()]);
            assert!(existing_paths_in_text(&format!("{here}\nthis is just a sentence")).is_empty());
            assert!(existing_paths_in_text("/definitely/not/here").is_empty());
            assert!(existing_paths_in_text("hello").is_empty());
        }
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

/// Everything on the system clipboard that is about files.
#[tauri::command]
pub async fn clipboard_get() -> CmdResult<ClipFiles> {
    tauri::async_runtime::spawn_blocking(imp::get_all).await.map_err(|e| e.to_string())?
}

/// Empty the system clipboard (after something another program cut was moved here).
#[tauri::command]
pub async fn clipboard_clear() -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(imp::clear).await.map_err(|e| e.to_string())?
}

/// How long staged files are kept before a later paste may remove them.
const STAGING_KEEP: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Save files that another program copied from inside itself (a zip, a phone) as real files,
/// and return where. Earlier staged pastes are removed.
#[tauri::command]
pub async fn clipboard_stage_virtual(state: State<'_, Shared>) -> CmdResult<Vec<String>> {
    let root = state.paths.remote_tmp.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Older staged pastes go, but never one a copy may still be reading from: a second
        // paste while the first is running used to delete the first one's files from under it.
        crate::remote::sweep_staging(&root, "paste-", Some(STAGING_KEEP));
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let made = fast_core::virtual_clip::extract_virtual_files(&root.join(format!("paste-{stamp}"))).map_err(|e| e.to_string())?;
        Ok(made.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}
