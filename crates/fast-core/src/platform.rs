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

/// The Windows build number (22621 for Windows 11 22H2), or 0 anywhere else or when unknown.
pub fn windows_build() -> u32 {
    #[cfg(windows)]
    {
        parse_build(&sysinfo::System::kernel_version().unwrap_or_default())
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// The build number from text like "22621" or "10.0.22621".
#[cfg(any(windows, test))]
fn parse_build(text: &str) -> u32 {
    text.split('.').filter_map(|p| p.trim().parse::<u32>().ok()).filter(|n| *n >= 1000).last().unwrap_or(0)
}

#[cfg(test)]
mod build_tests {
    use super::parse_build;

    #[test]
    fn reads_the_build_number_however_it_is_written() {
        assert_eq!(parse_build("22621"), 22621);
        assert_eq!(parse_build("10.0.19045"), 19045);
        assert_eq!(parse_build(""), 0);
        assert_eq!(parse_build("10.0"), 0);
    }
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

/// The folder a program started for `path` should run in: the folder the file is in (a folder runs nowhere in particular).
/// Without this a `.bat` runs in whatever folder this app was started from, and acts on the wrong files.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_folder(path: &Path) -> Option<&Path> {
    if path.is_dir() {
        return None;
    }
    path.parent().filter(|d| !d.as_os_str().is_empty() && d.is_dir())
}

/// Open a file or folder with the system's default application. Programs and scripts run in their own folder.
#[cfg(windows)]
pub fn open_path(path: &Path) -> Result<()> {
    shell_open(path, None)
}

/// Open a file with the program Windows has set for another kind of file (a ProgID such as `VLC.mp4`), whatever the
/// file's own extension is set to open with. Used for an extension two kinds of file share, such as `.ts`.
#[cfg(windows)]
pub fn open_path_as_class(path: &Path, prog_id: &str) -> Result<()> {
    shell_open(path, Some(prog_id))
}

#[cfg(windows)]
fn shell_open(path: &Path, class: Option<&str>) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_CLASSNAME, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide("open".as_ref());
    let file = wide(path.as_os_str());
    let dir = run_folder(path).map(|d| wide(d.as_os_str()));
    let class = class.map(|c| wide(c.as_ref()));
    // SAFETY: every pointer refers to a NUL-terminated buffer that outlives the call, and `info` is
    // zero-initialized as the API requires.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOASYNC | if class.is_some() { SEE_MASK_CLASSNAME } else { 0 };
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpClass = class.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
        info.lpDirectory = dir.as_ref().map_or(std::ptr::null(), |d| d.as_ptr());
        info.nShow = SW_SHOWNORMAL as i32;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(CoreError::io(path, std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(all(not(windows), not(any(target_os = "android", target_os = "ios"))))]
pub fn open_path(path: &Path) -> Result<()> {
    use std::process::Stdio;
    let dir = run_folder(path);
    let mut last: Option<std::io::Error> = None;
    for mut cmd in open::commands(path) {
        if let Some(d) = dir {
            cmd.current_dir(d);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match cmd.spawn() {
            Ok(mut child) => {
                // A launcher that fails at once hands over to the next one; one still running has handed the file over.
                let mut failed = false;
                for _ in 0..20 {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            failed = !status.success();
                            break;
                        }
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                        Err(_) => break,
                    }
                }
                if failed {
                    last = Some(std::io::Error::other("the launcher could not open it"));
                    continue;
                }
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(e) => last = Some(e),
        }
    }
    Err(CoreError::io(path, last.unwrap_or_else(|| std::io::Error::other("there is no program to open it with"))))
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
        TerminalMode::Home => home_dir().unwrap_or_else(|| path.to_path_buf()),
        TerminalMode::DriveRoot => drive_root(path),
    }
}

#[allow(dead_code)] // not every platform variant uses it
fn spawn(mut cmd: Command, what: &str) -> Result<()> {
    cmd.spawn().map(|_| ()).map_err(|e| CoreError::Invalid(format!("could not start {what}: {e}")))
}

/// How to open a terminal (Settings > Terminal). Every field may be empty.
#[derive(Default, Clone, Debug, PartialEq)]
pub struct TerminalOptions {
    /// Windows: "cmd", "powershell", "pwsh", "wt", "wsl" or "gitbash". Linux: the terminal program. macOS: the app.
    pub shell: String,
    /// A command to run as soon as the terminal opens; the terminal stays open afterwards.
    pub startup: String,
    /// Replaces everything: the command to run, with `{path}` standing for the folder.
    pub custom: String,
    /// Windows Terminal: the profile to open (by name).
    pub profile: String,
    /// Linux and macOS: the shell to run inside the terminal (bash, zsh, fish or a full path).
    pub login_shell: String,
    /// Windows Terminal, GNOME Terminal, Ptyxis and Konsole: open a tab in the existing window.
    pub new_tab: bool,
}

/// Split a command line into words, honoring single and double quotes, and
/// put `dir` where `{path}` appears (after splitting, so a path with spaces
/// stays one word).
pub fn command_words(template: &str, dir: &str) -> Vec<String> {
    let mut words = Vec::new();
    let (mut cur, mut quote, mut started) = (String::new(), None::<char>, false);
    for ch in template.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().map(|w| w.replace("{path}", dir)).collect()
}

/// What `sh -c` runs on Linux and macOS: the startup command, then a shell that keeps the window open.
pub fn unix_script(startup: &str, shell: &str) -> String {
    let shell = if shell.trim().is_empty() { "\"${SHELL:-sh}\"".to_string() } else { format!("'{}'", shell.replace('\'', r"'\''")) };
    if startup.trim().is_empty() {
        format!("exec {shell}")
    } else {
        format!("{startup}\nexec {shell}")
    }
}

/// The program and arguments that open a terminal on Windows (not administrator).
pub fn windows_plan(dir: &str, o: &TerminalOptions) -> (String, Vec<String>) {
    let v = |a: &[&str]| a.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let start = |exe: &str, tail: Vec<String>| {
        let mut a = v(&["/C", "start", "", "/D", dir, exe]);
        a.extend(tail);
        ("cmd.exe".to_string(), a)
    };
    let ps_tail = |startup: &str| if startup.trim().is_empty() { v(&["-NoExit"]) } else { vec!["-NoExit".into(), "-Command".into(), startup.to_string()] };
    match o.shell.as_str() {
        "powershell" => start("powershell.exe", ps_tail(&o.startup)),
        "pwsh" => start("pwsh.exe", ps_tail(&o.startup)),
        "wt" => {
            let mut a = Vec::new();
            if o.new_tab {
                a.extend(v(&["-w", "0", "new-tab"]));
            }
            if !o.profile.trim().is_empty() {
                a.extend(v(&["-p"]));
                a.push(o.profile.trim().to_string());
            }
            a.extend(v(&["-d", dir]));
            if !o.startup.trim().is_empty() {
                a.push("powershell.exe".into());
                a.extend(ps_tail(&o.startup));
            }
            ("wt.exe".to_string(), a)
        }
        "wsl" => {
            let mut tail = v(&["--cd", dir]);
            if !o.startup.trim().is_empty() {
                tail.extend(["--".into(), "bash".into(), "-lc".into(), unix_script(&o.startup, "bash")]);
            }
            start("wsl.exe", tail)
        }
        "gitbash" => {
            let exe = ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"]
                .iter()
                .filter_map(|k| std::env::var(k).ok())
                .flat_map(|base| [format!("{base}\\Git\\git-bash.exe"), format!("{base}\\Programs\\Git\\git-bash.exe")])
                .find(|p| Path::new(p).is_file())
                .unwrap_or_else(|| "git-bash.exe".to_string());
            start(&exe, vec![format!("--cd={dir}")])
        }
        _ => {
            let tail = if o.startup.trim().is_empty() { Vec::new() } else { vec!["/K".into(), o.startup.clone()] };
            start("cmd.exe", tail)
        }
    }
}

/// Arguments after the program name for a Linux terminal.
/// `wd_flag` and `exec_flag` are what the program uses for "start in this
/// folder" and "run this command" (either may be empty).
pub fn unix_terminal_args(prog: &str, wd_flag: &str, exec_flag: &str, dir: &str, o: &TerminalOptions, admin: bool) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    if !wd_flag.is_empty() {
        a.push(wd_flag.into());
        a.push(dir.into());
    }
    if o.new_tab {
        match prog {
            "gnome-terminal" | "ptyxis" => a.push("--tab".into()),
            "konsole" => a.push("--new-tab".into()),
            _ => {}
        }
    }
    let command: Option<Vec<String>> = if admin {
        Some(vec!["sudo".into(), "-s".into()])
    } else if !o.startup.trim().is_empty() || !o.login_shell.trim().is_empty() {
        Some(vec!["sh".into(), "-c".into(), unix_script(&o.startup, &o.login_shell)])
    } else {
        None
    };
    if let Some(c) = command {
        if !exec_flag.is_empty() {
            a.push(exec_flag.into());
        }
        a.extend(c);
    }
    a
}

/// The shell line the macOS terminals are given.
pub fn mac_line(dir: &str, startup: &str, shell: &str) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    let mut line = format!("cd {}", q(dir));
    if !startup.trim().is_empty() {
        line.push_str(&format!(" && {startup}"));
    }
    if !shell.trim().is_empty() {
        line.push_str(&format!(" && exec {}", q(shell)));
    }
    line
}

/// Run the user's own command (Settings > Terminal > Custom command).
#[allow(dead_code)]
fn open_custom(dir: &Path, custom: &str) -> Result<()> {
    let words = command_words(custom, &dir.to_string_lossy());
    let Some((prog, rest)) = words.split_first() else { return Err(CoreError::Invalid("the custom terminal command is empty".into())) };
    let mut cmd = Command::new(prog);
    cmd.args(rest).current_dir(dir);
    spawn(cmd, "the custom terminal command")
}

/// Open a terminal. `o` holds the person's choices from settings; empty
/// fields mean the platform default.
#[cfg(windows)]
pub fn open_terminal(path: &Path, mode: TerminalMode, o: &TerminalOptions) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    let dir_s = dir.to_string_lossy().into_owned();
    let shell = o.shell.as_str();
    let ps_quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
    if mode == TerminalMode::Admin {
        // Start-Process -Verb RunAs raises the standard UAC prompt.
        let (exe, args) = match shell {
            "powershell" | "pwsh" => (
                if shell == "pwsh" { "pwsh.exe" } else { "powershell.exe" },
                format!("'-NoExit','-Command',{}", ps_quote(&format!("Set-Location -LiteralPath {}", ps_quote(&dir_s)))),
            ),
            "wt" => ("wt.exe", format!("'-d',{}", ps_quote(&dir_s))),
            _ => ("cmd.exe", format!("'/K',{}", ps_quote(&format!("cd /d \"{dir_s}\"")))),
        };
        let mut cmd = Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-WindowStyle", "Hidden", "-Command"])
            .arg(format!("Start-Process {exe} -Verb RunAs -ArgumentList {args}"));
        crate::proc::quiet(&mut cmd);
        return spawn(cmd, "an elevated terminal");
    }
    if !o.custom.trim().is_empty() {
        return open_custom(&dir, &o.custom);
    }
    let (prog, args) = windows_plan(&dir_s, o);
    let mut cmd = Command::new(prog);
    cmd.args(args).current_dir(&dir);
    spawn(cmd, "a terminal")
}

#[cfg(target_os = "macos")]
pub fn open_terminal(path: &Path, mode: TerminalMode, o: &TerminalOptions) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    let dir_s = dir.to_string_lossy().into_owned();
    let app = if o.shell.is_empty() { "Terminal" } else { o.shell.as_str() };
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
    if !o.custom.trim().is_empty() {
        return open_custom(&dir, &o.custom);
    }
    let wants_line = !o.startup.trim().is_empty() || !o.login_shell.trim().is_empty();
    if wants_line && (app == "Terminal" || app.starts_with("iTerm")) {
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let line = esc(&mac_line(&dir_s, &o.startup, &o.login_shell));
        let mut cmd = Command::new("osascript");
        if app == "Terminal" {
            cmd.arg("-e").arg(format!("tell application \"Terminal\" to do script \"{line}\"")).arg("-e").arg("tell application \"Terminal\" to activate");
        } else {
            cmd.args(["-e", "tell application \"iTerm\"", "-e", "activate", "-e", "set w to (create window with default profile)", "-e"])
                .arg(format!("tell current session of w to write text \"{line}\""))
                .args(["-e", "end tell"]);
        }
        return spawn(cmd, "a terminal");
    }
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(app).arg(&dir);
    spawn(cmd, "a terminal")
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
pub fn open_terminal(path: &Path, mode: TerminalMode, o: &TerminalOptions) -> Result<()> {
    let dir = resolve_terminal_dir(path, mode);
    if mode != TerminalMode::Admin && !o.custom.trim().is_empty() {
        return open_custom(&dir, &o.custom);
    }
    let dir_s = dir.to_string_lossy().into_owned();
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
    let preferred = [o.shell.as_str(), env_term.as_str()];
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
        cmd.args(unix_terminal_args(prog, wd_flag, exec_flag, &dir_s, o, mode == TerminalMode::Admin));
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
    // the call, and `info` is zero-initialized as the API requires.
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

/// The usual folders of the device's built-in storage.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn known_folders() -> Vec<(&'static str, String)> {
    home_dir().map(|h| crate::phone::folders_in(&h)).unwrap_or_default()
}

/// A phone's "home" is its built-in storage (Android) or the app's own folder (iOS).
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn home_dir() -> Option<std::path::PathBuf> {
    crate::phone::roots().into_iter().find(|r| !r.removable).map(|r| r.path)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn open_path(_path: &Path) -> Result<()> {
    Err(CoreError::Unsupported("opening files is handled by the mobile shell"))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn trash(_paths: &[std::path::PathBuf]) -> Result<()> {
    Err(CoreError::Invalid("This device has no recycle bin. Delete the items permanently instead.".into()))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn open_terminal(path: &Path, mode: TerminalMode, _o: &TerminalOptions) -> Result<()> {
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
    use super::TerminalOptions;
    fn opts() -> TerminalOptions {
        TerminalOptions::default()
    }

    #[test]
    fn command_words_honor_quotes_and_fill_in_the_path() {
        let w = super::command_words(r#"alacritty --working-directory {path} -e "my shell" --flag='a b'"#, "/home/me/My Files");
        assert_eq!(w, ["alacritty", "--working-directory", "/home/me/My Files", "-e", "my shell", "--flag=a b"]);
        assert_eq!(super::command_words("  ", "/x"), Vec::<String>::new());
        assert_eq!(super::command_words("run ''", "/x"), ["run", ""]);
    }

    #[test]
    fn unix_script_runs_the_startup_then_keeps_a_shell_open() {
        assert_eq!(super::unix_script("", ""), "exec \"${SHELL:-sh}\"");
        assert_eq!(super::unix_script("git status", "zsh"), "git status\nexec 'zsh'");
        assert_eq!(super::unix_script("", "/opt/it's/fish"), "exec '/opt/it'\\''s/fish'");
    }

    #[test]
    fn linux_terminal_arguments() {
        let plain = super::unix_terminal_args("gnome-terminal", "--working-directory", "--", "/d", &opts(), false);
        assert_eq!(plain, ["--working-directory", "/d"], "nothing extra when nothing is set");
        let o = TerminalOptions { startup: "ls".into(), new_tab: true, ..opts() };
        let a = super::unix_terminal_args("gnome-terminal", "--working-directory", "--", "/d", &o, false);
        assert_eq!(a, ["--working-directory", "/d", "--tab", "--", "sh", "-c", "ls\nexec \"${SHELL:-sh}\""]);
        // A terminal without a command flag takes the command directly.
        let a = super::unix_terminal_args("kitty", "--directory", "", "/d", &o, false);
        assert_eq!(a[..3], ["--directory", "/d", "sh"]);
        let admin = super::unix_terminal_args("xterm", "", "-e", "/d", &o, true);
        assert_eq!(admin, ["-e", "sudo", "-s"], "an administrator terminal ignores the startup command");
    }

    #[test]
    fn windows_terminal_plans() {
        let (p, a) = super::windows_plan(r"C:\Work", &opts());
        assert_eq!((p.as_str(), a.join(" ")), ("cmd.exe", r"/C start  /D C:\Work cmd.exe".to_string()));
        let o = TerminalOptions { shell: "cmd".into(), startup: "dir".into(), ..opts() };
        assert!(super::windows_plan("C:\\W", &o).1.ends_with(&["/K".to_string(), "dir".to_string()]));
        let o = TerminalOptions { shell: "pwsh".into(), startup: "git status".into(), ..opts() };
        let (p, a) = super::windows_plan("C:\\W", &o);
        assert_eq!(p, "cmd.exe");
        assert!(a.contains(&"pwsh.exe".to_string()) && a.ends_with(&["-NoExit".to_string(), "-Command".to_string(), "git status".to_string()]));
        let o = TerminalOptions { shell: "wt".into(), profile: "Ubuntu".into(), new_tab: true, ..opts() };
        let (p, a) = super::windows_plan("C:\\W", &o);
        assert_eq!((p.as_str(), a.join(" ")), ("wt.exe", "-w 0 new-tab -p Ubuntu -d C:\\W".to_string()));
        let o = TerminalOptions { shell: "wsl".into(), ..opts() };
        assert!(super::windows_plan("C:\\W", &o).1.ends_with(&["wsl.exe".to_string(), "--cd".to_string(), "C:\\W".to_string()]));
    }

    #[test]
    fn macos_line() {
        assert_eq!(super::mac_line("/a b", "", ""), "cd '/a b'");
        assert_eq!(super::mac_line("/a", "make", "zsh"), "cd '/a' && make && exec 'zsh'");
    }
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

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod run_folder_tests {
    use super::run_folder;

    #[test]
    fn a_file_runs_in_its_own_folder() {
        let dir = std::env::temp_dir().join(format!("fast-runfolder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("start.bat");
        std::fs::write(&file, "echo hi").unwrap();
        assert_eq!(run_folder(&file), Some(dir.as_path()));
        // A folder is simply opened; it has no folder to run in.
        assert_eq!(run_folder(&dir), None);
        // Nothing to run in when the parent is not there.
        assert_eq!(run_folder(std::path::Path::new("start.bat")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
