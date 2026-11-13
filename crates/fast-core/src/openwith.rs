//! Choosing a program to open a file with.
//!
//! Opening a file normally hands it to the system's default program. When the
//! system has none for that kind of file (or the program is gone), the person
//! is asked instead of the open silently doing nothing. This module answers
//! three questions: *can this file be opened as things stand* (`check`), *which
//! programs could open it* (`choices`), and *open it with this one*
//! (`open_with`, optionally making that the default).
//!
//! Linux reads the desktop entries and `xdg-mime`. Windows reads the registry
//! through `reg`. macOS lists applications and opens with `open -a`; it has no
//! way to ask whether a default exists, so `check` is always `Ready` there (the
//! system shows its own chooser). The Windows and macOS parts are written
//! against those systems' documented behaviour and have not been run.

use crate::error::{CoreError, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// What to pass back to `open_with`: a desktop entry id, or a program path.
    pub id: String,
    pub name: String,
    /// A small picture as a data: URL, when one could be found.
    pub icon_url: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Choices {
    /// ".zip", or "" for a file without an extension.
    pub extension: String,
    pub default: Option<App>,
    pub suggested: Vec<App>,
    pub others: Vec<App>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// Opening should work.
    Ready,
    /// It will not, and this says why in a sentence.
    Missing(String),
}

pub fn extension_of(path: &Path) -> String {
    path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

fn what(path: &Path) -> String {
    let ext = extension_of(path);
    if ext.is_empty() { "this kind of file".into() } else { format!("{ext} files") }
}

/// Is `program` (a name looked up on PATH, or a path) runnable?
pub fn program_exists(program: &str) -> bool {
    let p = Path::new(program);
    if p.is_absolute() || program.contains('/') || program.contains('\\') {
        return p.is_file();
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|d| {
                let f = d.join(program);
                f.is_file() || (cfg!(windows) && d.join(format!("{program}.exe")).is_file())
            })
        })
        .unwrap_or(false)
}

/// Split a command line the way a shell would for plain arguments: spaces
/// separate, quotes group, a backslash escapes the next character.
pub fn split_command(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => match chars.peek().copied() {
                // Inside quotes only these are escapes; anything else keeps its backslash (Windows paths).
                Some(n @ ('"' | '`' | '$' | '\\')) => {
                    chars.next();
                    cur.push(n);
                }
                _ => cur.push('\\'),
            },
            (Some(_), c) => cur.push(c),
            (None, '"') | (None, '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') if cfg!(unix) => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                    started = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn encode_icon(file: &Path) -> Option<String> {
    use base64::Engine;
    let ext = file.extension()?.to_string_lossy().to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => return None,
    };
    let meta = std::fs::metadata(file).ok()?;
    if meta.len() > 200_000 {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

// ===========================================================================
// Linux and other Unix without a desktop database of its own
// ===========================================================================

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
mod platform {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    #[derive(Clone, Debug)]
    pub(super) struct Entry {
        pub id: String,
        pub name: String,
        pub exec: String,
        pub icon: String,
        pub mimes: Vec<String>,
        pub terminal: bool,
    }

    fn app_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = dirs::data_dir() {
            dirs.push(d.join("applications"));
        }
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        for d in system.split(':').filter(|d| !d.is_empty()) {
            dirs.push(Path::new(d).join("applications"));
        }
        if let Some(h) = dirs::home_dir() {
            dirs.push(h.join(".local/share/flatpak/exports/share/applications"));
        }
        dirs.push("/var/lib/flatpak/exports/share/applications".into());
        dirs.push("/var/lib/snapd/desktop/applications".into());
        dirs
    }

    /// The `[Desktop Entry]` group of a .desktop file.
    pub(super) fn parse_entry(id: &str, text: &str) -> Option<Entry> {
        let mut in_main = false;
        let (mut name, mut exec, mut icon, mut mimes) = (String::new(), String::new(), String::new(), Vec::new());
        let (mut hidden, mut terminal, mut is_app) = (false, false, false);
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_main = line == "[Desktop Entry]";
                continue;
            }
            if !in_main || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "Name" => name = v.trim().to_string(),
                "Exec" => exec = v.trim().to_string(),
                "Icon" => icon = v.trim().to_string(),
                "MimeType" => mimes = v.split(';').filter(|m| !m.is_empty()).map(str::to_string).collect(),
                "NoDisplay" | "Hidden" => hidden |= v.trim() == "true",
                "Terminal" => terminal = v.trim() == "true",
                "Type" => is_app = v.trim() == "Application",
                _ => {}
            }
        }
        (is_app && !hidden && !name.is_empty() && !exec.is_empty()).then(|| Entry { id: id.to_string(), name, exec, icon, mimes, terminal })
    }

    pub(super) fn installed() -> Vec<Entry> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for dir in app_dirs() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.filter_map(|e| e.ok()) {
                let file = e.file_name().to_string_lossy().into_owned();
                if !file.ends_with(".desktop") || !seen.insert(file.clone()) {
                    continue;
                }
                if let Some(entry) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_entry(&file, &t)) {
                    out.push(entry);
                }
            }
        }
        out
    }

    pub(super) fn find(id: &str) -> Option<Entry> {
        installed().into_iter().find(|e| e.id == id)
    }

    /// The program an entry would start, for "is it installed".
    pub(super) fn program_of(exec: &str) -> Option<String> {
        let words = split_command(exec);
        let mut it = words.into_iter().skip_while(|w| w == "env" || w.contains('=') && !w.starts_with('/'));
        it.next()
    }

    fn mime_of(path: &Path) -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "filetype"]).arg(path).output().ok()?;
        let m = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (out.status.success() && !m.is_empty()).then_some(m)
    }

    fn default_id(mime: &str) -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "default", mime]).output().ok()?;
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!id.is_empty()).then_some(id)
    }

    /// Programs and scripts are run by the system, not opened with a viewer.
    fn is_program_type(mime: &str) -> bool {
        matches!(mime, "application/x-executable" | "application/x-pie-executable" | "application/x-sharedlib" | "inode/directory")
    }

    fn icon_for(name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        if name.starts_with('/') {
            return encode_icon(Path::new(name));
        }
        let mut bases: Vec<PathBuf> = vec!["/usr/share/icons".into(), "/usr/share/pixmaps".into()];
        if let Some(d) = dirs::data_dir() {
            bases.push(d.join("icons"));
        }
        for base in &bases {
            for size in ["48x48", "64x64", "32x32", "128x128", "256x256", "scalable"] {
                for ext in ["png", "svg"] {
                    let f = base.join("hicolor").join(size).join("apps").join(format!("{name}.{ext}"));
                    if f.is_file() {
                        return encode_icon(&f);
                    }
                }
            }
            for ext in ["png", "svg"] {
                let f = base.join(format!("{name}.{ext}"));
                if f.is_file() {
                    return encode_icon(&f);
                }
            }
        }
        None
    }

    fn app_of(e: &Entry, with_icon: bool) -> App {
        App { id: e.id.clone(), name: e.name.clone(), icon_url: if with_icon { icon_for(&e.icon) } else { None } }
    }

    pub fn check(path: &Path) -> Check {
        if path.is_dir() {
            return Check::Ready;
        }
        let Some(mime) = mime_of(path) else { return Check::Ready }; // no xdg-mime: leave it to the opener
        if is_program_type(&mime) {
            return Check::Ready;
        }
        let Some(id) = default_id(&mime) else {
            return Check::Missing(format!("No app is set to open {}.", what(path)));
        };
        match find(&id) {
            None => Check::Missing(format!("The app set to open {} ({id}) is not installed.", what(path))),
            Some(e) => match program_of(&e.exec) {
                Some(p) if program_exists(&p) => Check::Ready,
                _ => Check::Missing(format!("{} cannot be found on this computer.", e.name)),
            },
        }
    }

    pub fn choices(path: &Path) -> Choices {
        let mime = mime_of(path).unwrap_or_default();
        let default_entry = default_id(&mime).and_then(|id| find(&id)).filter(|e| program_of(&e.exec).is_some_and(|p| program_exists(&p)));
        let parent = if mime.starts_with("text/") { "text/plain" } else { "" };
        let mut suggested = Vec::new();
        let mut others = Vec::new();
        let mut names = HashSet::new();
        let mut all = installed();
        all.sort_by_key(|e| e.name.to_lowercase());
        for e in all {
            if default_entry.as_ref().is_some_and(|d| d.id == e.id) || !names.insert(e.name.clone()) {
                continue;
            }
            if !program_of(&e.exec).is_some_and(|p| program_exists(&p)) {
                continue;
            }
            let fits = !mime.is_empty() && e.mimes.iter().any(|m| *m == mime || (!parent.is_empty() && m == parent));
            let app = app_of(&e, fits || suggested.len() + others.len() < 60);
            if fits { suggested.push(app) } else { others.push(app) }
        }
        Choices { extension: extension_of(path), default: default_entry.map(|e| app_of(&e, true)), suggested, others }
    }

    /// Expand a desktop entry's Exec line for one file.
    pub(super) fn command_for(e: &Entry, path: &Path) -> Vec<String> {
        let file = path.to_string_lossy().into_owned();
        let mut argv = Vec::new();
        let mut had_file = false;
        for w in split_command(&e.exec) {
            let mut word = String::new();
            let mut chars = w.chars();
            while let Some(c) = chars.next() {
                if c != '%' {
                    word.push(c);
                    continue;
                }
                match chars.next() {
                    Some('f') | Some('F') | Some('u') | Some('U') => {
                        word.push_str(&file);
                        had_file = true;
                    }
                    Some('c') => word.push_str(&e.name),
                    Some('%') => word.push('%'),
                    _ => {} // %i %k %d %D %n %N %m %v: nothing for a plain file
                }
            }
            if !word.is_empty() || w.is_empty() {
                argv.push(word);
            }
        }
        if !had_file {
            argv.push(file);
        }
        argv
    }

    pub fn open_with(path: &Path, id: &str, always: bool) -> Result<()> {
        // A program given by path (from "Browse…") is run directly.
        let entry = if id.starts_with('/') {
            Entry { id: id.into(), name: id.rsplit('/').next().unwrap_or(id).into(), exec: format!("\"{id}\" %f"), icon: String::new(), mimes: vec![], terminal: false }
        } else {
            find(id).ok_or(CoreError::Unsupported("that app is not installed"))?
        };
        let mut argv = command_for(&entry, path);
        if entry.terminal {
            let mut full = vec!["x-terminal-emulator".to_string(), "-e".to_string()];
            full.append(&mut argv);
            argv = full;
        }
        let (program, args) = argv.split_first().ok_or(CoreError::Unsupported("empty command"))?;
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CoreError::io(Path::new(program), e))?;
        // Reap it when it ends so it does not linger as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        if always && !id.starts_with('/') {
            if let Some(mime) = mime_of(path) {
                let _ = Command::new("xdg-mime").args(["default", id, &mime]).status();
            }
        }
        Ok(())
    }
}

// ===========================================================================
// Windows
// ===========================================================================

#[cfg(windows)]
mod platform {
    use super::*;
    use std::process::Command;

    fn reg(args: &[&str]) -> Option<String> {
        let out = Command::new("reg").args(args).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The text after the value type in a `reg query` line.
    fn value_of(output: &str, name: &str) -> Option<String> {
        output.lines().find_map(|l| {
            let t = l.trim();
            let rest = if name.is_empty() { t.strip_prefix("(Default)") } else { t.strip_prefix(name) }?;
            let rest = rest.trim_start();
            let rest = rest.strip_prefix("REG_SZ").or_else(|| rest.strip_prefix("REG_EXPAND_SZ"))?;
            Some(rest.trim().to_string())
        })
    }

    fn prog_id(ext: &str) -> Option<String> {
        let user = format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\{ext}\UserChoice");
        reg(&["query", &user, "/v", "ProgId"])
            .and_then(|o| value_of(&o, "ProgId"))
            .or_else(|| reg(&["query", &format!(r"HKCR\{ext}"), "/ve"]).and_then(|o| value_of(&o, "")))
    }

    fn open_command(prog: &str) -> Option<String> {
        reg(&["query", &format!(r"HKCR\{prog}\shell\open\command"), "/ve"]).and_then(|o| value_of(&o, ""))
    }

    fn expand_env(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            match rest[i + 1..].find('%') {
                Some(j) => {
                    let var = &rest[i + 1..i + 1 + j];
                    out.push_str(&std::env::var(var).unwrap_or_else(|_| format!("%{var}%")));
                    rest = &rest[i + j + 2..];
                }
                None => {
                    out.push_str(&rest[i..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    pub fn check(path: &Path) -> Check {
        if path.is_dir() {
            return Check::Ready;
        }
        let ext = extension_of(path);
        if ext.is_empty() {
            return Check::Missing("No app is set to open this kind of file.".into());
        }
        if matches!(ext.as_str(), ".exe" | ".bat" | ".cmd" | ".com" | ".msi" | ".lnk" | ".ps1" | ".scr") {
            return Check::Ready;
        }
        let Some(prog) = prog_id(&ext) else { return Check::Missing(format!("No app is set to open {}.", what(path))) };
        match open_command(&prog) {
            None => Check::Missing(format!("No app is set to open {}.", what(path))),
            Some(cmd) => {
                let first = split_command(&expand_env(&cmd)).into_iter().next().unwrap_or_default();
                if first.is_empty() || first.to_lowercase().ends_with("openwith.exe") || program_exists(&first) {
                    Check::Ready
                } else {
                    Check::Missing(format!("The app set to open {} cannot be found.", what(path)))
                }
            }
        }
    }

    fn app_from_exe(exe: &str, with_name: Option<&str>) -> App {
        let stem = Path::new(exe).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| exe.to_string());
        App { id: exe.to_string(), name: with_name.map(str::to_string).unwrap_or(stem), icon_url: None }
    }

    pub fn choices(path: &Path) -> Choices {
        let ext = extension_of(path);
        let mut suggested = Vec::new();
        let mut seen = std::collections::HashSet::new();
        // Programs Windows remembers for this extension.
        if let Some(o) = reg(&["query", &format!(r"HKCR\{ext}\OpenWithList"), "/s"]) {
            for exe in o.lines().filter_map(|l| l.trim().split_whitespace().last()).filter(|e| e.to_lowercase().ends_with(".exe")) {
                if let Some(full) = app_path(exe) {
                    if seen.insert(full.to_lowercase()) {
                        suggested.push(app_from_exe(&full, None));
                    }
                }
            }
        }
        // Everything registered under App Paths.
        let mut others = Vec::new();
        if let Some(o) = reg(&["query", r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths", "/s", "/ve"]) {
            for exe in o.lines().filter_map(|l| value_of(l, "(Default)")).map(|p| expand_env(p.trim_matches('"'))) {
                if exe.to_lowercase().ends_with(".exe") && Path::new(&exe).is_file() && seen.insert(exe.to_lowercase()) {
                    others.push(app_from_exe(&exe, None));
                }
            }
        }
        others.sort_by_key(|a| a.name.to_lowercase());
        let default = prog_id(&ext).and_then(|p| open_command(&p)).and_then(|c| split_command(&expand_env(&c)).into_iter().next()).filter(|e| Path::new(e).is_file()).map(|e| app_from_exe(&e, None));
        Choices { extension: ext, default, suggested, others }
    }

    fn app_path(exe: &str) -> Option<String> {
        reg(&["query", &format!(r"HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}"), "/ve"])
            .and_then(|o| value_of(&o, ""))
            .map(|p| expand_env(p.trim_matches('"')))
            .filter(|p| Path::new(p).is_file())
    }

    pub fn open_with(path: &Path, id: &str, _always: bool) -> Result<()> {
        Command::new(id).arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(Path::new(id), e))
    }

    /// Windows' own "How do you want to open this file?" window.
    pub fn system_chooser(path: &Path) -> Result<()> {
        Command::new("rundll32.exe").arg("shell32.dll,OpenAs_RunDLL").arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(path, e))
    }
}

// ===========================================================================
// macOS
// ===========================================================================

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use std::process::Command;

    pub fn check(_path: &Path) -> Check {
        Check::Ready
    }

    pub fn choices(path: &Path) -> Choices {
        let mut apps = Vec::new();
        let mut roots = vec![std::path::PathBuf::from("/Applications"), "/System/Applications".into()];
        if let Some(h) = dirs::home_dir() {
            roots.push(h.join("Applications"));
        }
        for root in roots {
            if let Ok(rd) = std::fs::read_dir(root) {
                for e in rd.filter_map(|e| e.ok()) {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "app") {
                        let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        apps.push(App { id: p.to_string_lossy().into_owned(), name, icon_url: None });
                    }
                }
            }
        }
        apps.sort_by_key(|a| a.name.to_lowercase());
        Choices { extension: extension_of(path), default: None, suggested: vec![], others: apps }
    }

    pub fn open_with(path: &Path, id: &str, _always: bool) -> Result<()> {
        Command::new("open").args(["-a", id]).arg(path).spawn().map(|_| ()).map_err(|e| CoreError::io(path, e))
    }
}

// ===========================================================================
// Phones and anything else: the shell does it
// ===========================================================================

#[cfg(any(target_os = "android", target_os = "ios", not(any(unix, windows))))]
mod platform {
    use super::*;
    pub fn check(_: &Path) -> Check {
        Check::Ready
    }
    pub fn choices(path: &Path) -> Choices {
        Choices { extension: extension_of(path), ..Default::default() }
    }
    pub fn open_with(_: &Path, _: &str, _: bool) -> Result<()> {
        Err(CoreError::Unsupported("choosing an app is handled by the mobile shell"))
    }
}

pub use platform::{check, choices, open_with};

/// The system's own chooser window, where there is one (Windows only).
#[cfg(windows)]
pub fn system_chooser(path: &Path) -> Result<()> {
    platform::system_chooser(path)
}

#[cfg(not(windows))]
pub fn system_chooser(_path: &Path) -> Result<()> {
    Err(CoreError::Unsupported("this system has no app chooser of its own"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_a_shell() {
        if cfg!(unix) {
            assert_eq!(split_command(r#"vlc --one "a b" c\ d 'e f'"#), vec!["vlc", "--one", "a b", "c d", "e f"]);
        }
        assert_eq!(split_command(r#""C:\Program Files\x.exe" %1"#), vec![r"C:\Program Files\x.exe", "%1"]);
        assert_eq!(split_command(""), Vec::<String>::new());
        assert_eq!(split_command(r#"a "" b"#), vec!["a", "", "b"]);
    }

    #[test]
    fn extension_is_lowercase_with_dot() {
        assert_eq!(extension_of(Path::new("/x/Photo.JPG")), ".jpg");
        assert_eq!(extension_of(Path::new("/x/Makefile")), "");
    }

    #[test]
    fn finds_programs_on_path() {
        assert!(program_exists(if cfg!(windows) { "cmd" } else { "sh" }));
        assert!(!program_exists("definitely-not-a-program-xyz"));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    mod desktop {
        use super::super::platform::{command_for, parse_entry, program_of};
        use std::path::Path;

        const ENTRY: &str = "[Desktop Entry]\nType=Application\nName=Viewer\nName[de]=Anzeiger\nExec=env FOO=1 viewer --open %F\nIcon=viewer\nMimeType=image/png;image/jpeg;\n\n[Desktop Action x]\nName=Other\nExec=nothing\n";

        #[test]
        fn reads_the_main_group_only() {
            let e = parse_entry("viewer.desktop", ENTRY).unwrap();
            assert_eq!(e.name, "Viewer");
            assert_eq!(e.mimes, vec!["image/png", "image/jpeg"]);
            assert_eq!(program_of(&e.exec).as_deref(), Some("viewer"));
        }

        #[test]
        fn hidden_entries_are_skipped() {
            assert!(parse_entry("a.desktop", "[Desktop Entry]\nType=Application\nName=A\nExec=a\nNoDisplay=true\n").is_none());
            assert!(parse_entry("a.desktop", "[Desktop Entry]\nType=Link\nName=A\nExec=a\n").is_none());
        }

        #[test]
        fn expands_field_codes() {
            let e = parse_entry("viewer.desktop", ENTRY).unwrap();
            let argv = command_for(&e, Path::new("/tmp/a b.png"));
            assert_eq!(argv, vec!["env", "FOO=1", "viewer", "--open", "/tmp/a b.png"]);
            let bare = parse_entry("b.desktop", "[Desktop Entry]\nType=Application\nName=B\nExec=b -x\n").unwrap();
            assert_eq!(command_for(&bare, Path::new("/f")), vec!["b", "-x", "/f"]);
        }
    }
}
