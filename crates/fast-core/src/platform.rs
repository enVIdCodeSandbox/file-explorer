//! Everything that differs per operating system lives here, behind one API:
//! known folders, opening files, the recycle bin, terminals and elevation.
//!
//! The app itself never runs elevated. When an operation needs administrator
//! rights, only that operation is run through the OS's own prompt (UAC on
//! Windows, polkit on Linux, the authorization dialog on macOS), and only
//! after the person has approved it there.

use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TerminalMode {
    /// In the given folder.
    Here,
    /// In the user's home folder.
    Home,
    /// In the root of the drive containing the given folder.
    DriveRoot,
    /// In the given folder, as administrator / root.
    Admin,
}

pub fn os_name() -> &'static str {
    std::env::consts::OS
}

// ---------------------------------------------------------------------------
// Desktop
// ---------------------------------------------------------------------------

/// Standard user folders that exist, as (label, path).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn known_folders() -> Vec<(&'static str, String)> {
    let candidates: [(&str, Option<std::path::PathBuf>); 7] = [
        ("Home", dirs::home_dir()),
        ("Desktop", dirs::desktop_dir()),
        ("Downloads", dirs::download_dir()),
        ("Documents", dirs::document_dir()),
        ("Pictures", dirs::picture_dir()),
        ("Music", dirs::audio_dir()),
        ("Videos", dirs::video_dir()),
    ];
    let home = dirs::home_dir();
    candidates
        .into_iter()
        .filter_map(|(label, p)| {
            // Some Linux setups have no XDG user-dirs configuration; fall back
            // to the conventional folder name under the home directory.
            let p = p.or_else(|| home.as_ref().map(|h| h.join(label)))?;
            p.is_dir().then(|| (label, p.to_string_lossy().into_owned()))
        })
        .collect()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn home_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir()
}

/// Open a file or folder with the system's default application.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn open_path(path: &Path) -> Result<()> {
    open::that_detached(path).map_err(|e| CoreError::io(path, e))
}

/// Move to the recycle bin / trash.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn trash(paths: &[std::path::PathBuf]) -> Result<()> {
    trash::delete_all(paths).map_err(|e| CoreError::Invalid(format!("could not move to trash: {e}")))
}

/// Put items that were moved to the Trash back where they came from, and say
/// how many were found there. When the same path was trashed more than once,
/// the most recent one comes back. Items that are not in the Trash (already
/// emptied, deleted permanently) are skipped.
#[cfg(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))))]
pub fn restore_from_trash(paths: &[std::path::PathBuf]) -> Result<usize> {
    let items = trash::os_limited::list().map_err(|e| CoreError::Invalid(format!("could not read the trash: {e}")))?;
    let chosen: Vec<_> = paths
        .iter()
        .filter_map(|p| items.iter().filter(|i| i.original_path() == *p).max_by_key(|i| i.time_deleted).cloned())
        .collect();
    let found = chosen.len();
    if found > 0 {
        trash::os_limited::restore_all(chosen).map_err(|e| CoreError::Invalid(format!("could not restore from the trash: {e}")))?;
    }
    Ok(found)
}

/// macOS has no programmatic way to put an item back from the Trash.
#[cfg(not(any(windows, all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))))]
pub fn restore_from_trash(_paths: &[std::path::PathBuf]) -> Result<usize> {
    Err(CoreError::Invalid("Items cannot be put back from the Trash automatically on this system. Open the Trash and use Put Back.".into()))
}

/// The root of the volume containing `path` (`C:\` or the mount point).
pub fn drive_root(path: &Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        use std::path::Component;
        if let Some(Component::Prefix(p)) = path.components().next() {
            let mut root = std::path::PathBuf::from(p.as_os_str());
            root.push("\\");
            return root;
        }
        path.to_path_buf()
    }
    #[cfg(not(windows))]
    {
        let drives = crate::drives::list_drives();
        crate::drives::volume_for(path, &drives)
            .map(|d| std::path::PathBuf::from(&d.mount))
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
    }
}

fn resolve_terminal_dir(path: &Path, mode: TerminalMode) -> std::path::PathBuf {
    match mode {
        TerminalMode::Here | TerminalMode::Admin => path.to_path_buf(),
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        TerminalMode::Home => home_dir().unwrap_or_else(|| path.to_path_buf()),
        #[cfg(any(target_os = "android", target_os = "ios"))]
        TerminalMode::Home => path.to_path_buf(),
        TerminalMode::DriveRoot => drive_root(path),
    }
}

#[allow(dead_code)] // not every platform variant uses it
fn spawn(mut cmd: Command, what: &str) -> Result<()> {
    cmd.spawn().map(|_| ()).map_err(|e| CoreError::Invalid(format!("could not start {what}: {e}")))
}

/// Open a terminal. `shell` is the user's choice from settings; empty means
/// the platform default.
#[cfg(windows)]
pub fn open_terminal(path: &Path, mode: TerminalMode, shell: &str) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    let dir_s = dir.to_string_lossy().into_owned();
    let ps_quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    if mode == TerminalMode::Admin {
        // Start-Process -Verb RunAs raises the standard UAC prompt.
        let (exe, args) = match shell {
            "powershell" => (
                "powershell.exe",
                format!("'-NoExit','-Command',{}", ps_quote(&format!("Set-Location -LiteralPath {}", ps_quote(&dir_s)))),
            ),
            "wt" => ("wt.exe", format!("'-d',{}", ps_quote(&dir_s))),
            _ => ("cmd.exe", format!("'/K',{}", ps_quote(&format!("cd /d \"{dir_s}\"")))),
        };
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command"])
            .arg(format!("Start-Process {exe} -Verb RunAs -ArgumentList {args}"));
        return spawn(cmd, "an elevated terminal");
    }
    let mut cmd = match shell {
        "powershell" => {
            let mut c = Command::new("cmd.exe");
            c.args(["/C", "start", "", "/D"]).arg(&dir).arg("powershell.exe");
            c
        }
        "wt" => {
            let mut c = Command::new("wt.exe");
            c.arg("-d").arg(&dir);
            c
        }
        _ => {
            let mut c = Command::new("cmd.exe");
            c.args(["/C", "start", "", "/D"]).arg(&dir).arg("cmd.exe");
            c
        }
    };
    cmd.current_dir(&dir);
    spawn(cmd, "a terminal")
}

#[cfg(target_os = "macos")]
pub fn open_terminal(path: &Path, mode: TerminalMode, shell: &str) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    let app = if shell.is_empty() { "Terminal" } else { shell };
    if mode == TerminalMode::Admin {
        // AppleScript string escaping, then shell single-quote escaping.
        let sh_dir = dir.to_string_lossy().replace('\'', r"'\''");
        let script = format!("cd '{sh_dir}' && sudo -s").replace('\\', "\\\\").replace('"', "\\\"");
        let mut cmd = Command::new("osascript");
        cmd.arg("-e")
            .arg(format!("tell application \"Terminal\" to do script \"{script}\""))
            .arg("-e")
            .arg("tell application \"Terminal\" to activate");
        return spawn(cmd, "an elevated terminal");
    }
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(app).arg(&dir);
    spawn(cmd, "a terminal")
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
pub fn open_terminal(path: &Path, mode: TerminalMode, shell: &str) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    // (program, working-directory flag, flag that precedes a command to run)
    const TERMINALS: &[(&str, &str, &str)] = &[
        ("x-terminal-emulator", "", "-e"),
        ("gnome-terminal", "--working-directory", "--"),
        ("ptyxis", "--working-directory", "--"),
        ("kgx", "--working-directory", "-e"),
        ("konsole", "--workdir", "-e"),
        ("xfce4-terminal", "--working-directory", "-x"),
        ("tilix", "--working-directory", "-e"),
        ("alacritty", "--working-directory", "-e"),
        ("kitty", "--directory", ""),
        ("wezterm", "", "-e"),
        ("foot", "--working-directory", ""),
        ("xterm", "", "-e"),
    ];
    let env_term = std::env::var("TERMINAL").unwrap_or_default();
    let preferred = [shell, env_term.as_str()];
    let candidates = preferred
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| {
            TERMINALS.iter().find(|t| t.0 == *s).copied().unwrap_or((*s, "", "-e"))
        })
        .chain(TERMINALS.iter().copied());

    for (prog, wd_flag, exec_flag) in candidates {
        let mut cmd = Command::new(prog);
        cmd.current_dir(&dir);
        if !wd_flag.is_empty() {
            cmd.arg(wd_flag).arg(&dir);
        }
        if mode == TerminalMode::Admin {
            if !exec_flag.is_empty() {
                cmd.arg(exec_flag);
            }
            cmd.args(["sudo", "-s"]);
        }
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }
    Err(CoreError::Invalid(
        "no terminal emulator found; set one in Settings > Terminal".into(),
    ))
}

/// How an elevation request ended.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Elevation {
    /// The OS prompt was accepted and the program ran; this is its exit code.
    Completed(i32),
    /// The person dismissed the OS prompt (or is not allowed to approve it).
    /// Nothing was run.
    Declined,
}

/// Run a program with administrator rights through the OS prompt and wait for
/// it. Used for single operations that need it, never to elevate the app.
#[cfg(windows)]
pub fn run_elevated(program: &Path, args: &[String]) -> Result<Elevation> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let quote = |a: &str| if a.contains([' ', '\t']) { format!("\"{a}\"") } else { a.to_string() };
    let verb = wide("runas".as_ref());
    let file = wide(program.as_os_str());
    let params = wide(args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ").as_ref());

    // SAFETY: every pointer refers to a NUL-terminated buffer that outlives
    // the call, and `info` is zero-initialised as the API requires.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE as i32;
        if ShellExecuteExW(&mut info) == 0 {
            let code = GetLastError();
            return if code == ERROR_CANCELLED {
                Ok(Elevation::Declined)
            } else {
                Err(CoreError::Invalid(format!("could not request administrator permission (error {code})")))
            };
        }
        let process = info.hProcess;
        if process.is_null() {
            return Err(CoreError::Invalid("the elevated helper did not start".into()));
        }
        WaitForSingleObject(process, INFINITE);
        let mut exit = 1u32;
        GetExitCodeProcess(process, &mut exit);
        CloseHandle(process);
        Ok(Elevation::Completed(exit as i32))
    }
}

#[cfg(target_os = "macos")]
pub fn run_elevated(program: &Path, args: &[String]) -> Result<Elevation> {
    let sh_quote = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let line = std::iter::once(program.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .map(|s| sh_quote(&s))
        .collect::<Vec<_>>()
        .join(" ");
    let script = line.replace('\\', "\\\\").replace('"', "\\\"");
    let out = Command::new("osascript")
        .arg("-e")
        .arg(format!("do shell script \"{script}\" with administrator privileges"))
        .output()
        .map_err(|e| CoreError::Invalid(format!("could not request administrator permission: {e}")))?;
    if out.status.success() {
        return Ok(Elevation::Completed(0));
    }
    // AppleScript error -128 is "User canceled."
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("-128") || err.to_lowercase().contains("canceled") {
        Ok(Elevation::Declined)
    } else {
        Ok(Elevation::Completed(out.status.code().unwrap_or(1)))
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
pub fn run_elevated(program: &Path, args: &[String]) -> Result<Elevation> {
    let status = Command::new("pkexec")
        .arg(program)
        .args(args)
        .status()
        .map_err(|e| CoreError::Invalid(format!("could not request administrator permission (is polkit installed?): {e}")))?;
    // pkexec: 126 = the person dismissed the dialog, 127 = not authorised.
    match status.code() {
        Some(126) | Some(127) => Ok(Elevation::Declined),
        Some(c) => Ok(Elevation::Completed(c)),
        None => Ok(Elevation::Completed(1)),
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// Ask the OS to show its own properties window for `paths`. Returns false
/// when the OS has none for this request (several items on Windows, no file
/// manager service on Linux, mobile); the caller then shows the app's own.
#[cfg(windows)]
pub fn show_properties(paths: &[std::path::PathBuf]) -> Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_INVOKEIDLIST, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, SW_SHOW};

    // The shell's properties verb takes one item. Several items use the
    // in-app dialog, which can total them.
    let [path] = paths else { return Ok(false) };
    let file: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "properties".encode_utf16().chain(std::iter::once(0)).collect();
    let (tx, rx) = std::sync::mpsc::channel();
    // The window belongs to the thread that asked for it, so that thread needs
    // COM and a message loop for as long as the shell is setting it up.
    std::thread::Builder::new()
        .name("properties".into())
        .spawn(move || unsafe {
            CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_INVOKEIDLIST;
            info.lpVerb = verb.as_ptr();
            info.lpFile = file.as_ptr();
            info.nShow = SW_SHOW as i32;
            let ok = ShellExecuteExW(&mut info) != 0;
            let _ = tx.send(ok);
            let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut msg: MSG = std::mem::zeroed();
            while std::time::Instant::now() < until {
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            CoUninitialize();
        })
        .map_err(|e| CoreError::Invalid(format!("could not open properties: {e}")))?;
    Ok(rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap_or(false))
}

#[cfg(target_os = "macos")]
pub fn show_properties(paths: &[std::path::PathBuf]) -> Result<bool> {
    let [path] = paths else { return Ok(false) };
    let quoted = path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\"");
    let status = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "tell application \"Finder\" to open information window of (POSIX file \"{quoted}\" as alias)"
        ))
        .arg("-e")
        .arg("tell application \"Finder\" to activate")
        .status()
        .map_err(|e| CoreError::Invalid(format!("could not open Get Info: {e}")))?;
    Ok(status.success())
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
pub fn show_properties(paths: &[std::path::PathBuf]) -> Result<bool> {
    if paths.is_empty() {
        return Ok(false);
    }
    // The freedesktop file manager interface is implemented by Nautilus,
    // Dolphin, Nemo, Thunar and others.
    let uris: Vec<String> = paths.iter().map(|p| format!("'{}'", file_uri(p))).collect();
    let status = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.FileManager1",
            "--object-path", "/org/freedesktop/FileManager1",
            "--method", "org.freedesktop.FileManager1.ShowItemProperties",
        ])
        .arg(format!("[{}]", uris.join(", ")))
        .arg("")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(matches!(status, Ok(s) if s.success()))
}

/// `file:///home/me/My%20Files` for a path.
#[allow(dead_code)] // only some platforms use it
fn file_uri(path: &Path) -> String {
    use std::fmt::Write;
    let mut out = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Mobile: the OS sandbox does not offer these. See docs/ROADMAP.md.
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn known_folders() -> Vec<(&'static str, String)> {
    Vec::new()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn home_dir() -> Option<std::path::PathBuf> {
    None
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn open_path(_path: &Path) -> Result<()> {
    Err(CoreError::Unsupported("opening files is handled by the mobile shell"))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn trash(_paths: &[std::path::PathBuf]) -> Result<()> {
    Err(CoreError::Unsupported("trash"))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn open_terminal(path: &Path, mode: TerminalMode, _shell: &str) -> Result<()> {
    let _ = (resolve_terminal_dir(path, mode), spawn as fn(Command, &str) -> Result<()>);
    Err(CoreError::Unsupported("terminal"))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn run_elevated(_program: &Path, _args: &[String]) -> Result<Elevation> {
    Err(CoreError::Unsupported("elevation"))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn show_properties(_paths: &[std::path::PathBuf]) -> Result<bool> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))]
    #[test]
    fn a_trashed_file_comes_back_where_it_was() {
        // Under $HOME so the file is on the home volume, whose trash always exists.
        let home = dirs::home_dir().unwrap();
        let dir = tempfile::Builder::new().prefix("fast-restore-").tempdir_in(&home).unwrap();
        let file = dir.path().join("note.txt");
        std::fs::write(&file, b"hello").unwrap();
        trash(std::slice::from_ref(&file)).unwrap();
        assert!(!file.exists());
        assert_eq!(restore_from_trash(std::slice::from_ref(&file)).unwrap(), 1);
        assert_eq!(std::fs::read(&file).unwrap(), b"hello");
        // Asking again finds nothing in the trash and is not an error.
        assert_eq!(restore_from_trash(std::slice::from_ref(&file)).unwrap(), 0);
    }

    #[test]
    fn known_folders_exist() {
        for (_, path) in known_folders() {
            assert!(Path::new(&path).is_dir());
        }
    }

    #[test]
    fn file_uris_are_percent_encoded() {
        assert_eq!(file_uri(Path::new("/home/me/My Files/a#b.txt")), "file:///home/me/My%20Files/a%23b.txt");
    }

    #[test]
    fn drive_root_is_an_ancestor() {
        let here = std::env::current_dir().unwrap();
        let root = drive_root(&here);
        assert!(here.starts_with(&root));
    }
}
