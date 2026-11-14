//! Standing in for the system's file manager.
//!
//! Two halves:
//!
//! * [`parse_launch`] turns what the app was started with (a folder, a file, a
//!   `file://` address, `--select <file>`) into folders to show and items to
//!   highlight in them. The same shape comes out of the Linux "show in folder"
//!   service ([`service`]) and the macOS open-document event.
//! * [`status`], [`set_default`] and [`restore`] make the app the program the
//!   system starts for folders, and put back what was there before. Everything
//!   is written for the signed-in person only; nothing needs administrator rights.
//!
//! What each system allows differs: see `docs/USER-GUIDE.md` ("Default file manager").

use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One folder to show, and the items in it to highlight (full paths; may be empty).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub folder: String,
    pub select: Vec<String>,
}

/// Where the app stands as the system's file manager.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    /// False when this system (or this way of running the app) cannot do it; `detail` says why.
    pub supported: bool,
    /// The app is what the system starts for folders right now.
    pub active: bool,
    /// What handled folders before the app took over, when that is known.
    pub previous: Option<String>,
    /// One sentence for the settings page.
    pub detail: String,
    /// What still goes to the system's own file manager on this system.
    pub limits: Vec<String>,
}

/// The file, next to the settings, where the app notes what it changed.
pub const SAVED_FILE: &str = "default-file-manager.json";

/// Undo [`set_default`] without the app running: what an uninstaller calls
/// (`fast-file-explorer --restore-file-manager`). `app_id` names the settings folder.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn restore_for(app_id: &str) -> Result<()> {
    let dir = dirs::config_dir().ok_or(CoreError::Unsupported("no settings folder"))?;
    restore(&dir.join(app_id).join(SAVED_FILE)).map(|_| ())
}

/// What is remembered between "make default" and "put it back".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    active: bool,
    /// Per-system notes on what was set before, by name.
    previous: std::collections::BTreeMap<String, String>,
}

impl Saved {
    fn load(file: &Path) -> Saved {
        std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    #[cfg_attr(any(target_os = "android", target_os = "ios"), allow(dead_code))]
    fn save(&self, file: &Path) -> Result<()> {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let body = serde_json::to_vec_pretty(self).map_err(|e| CoreError::Invalid(e.to_string()))?;
        std::fs::write(file, body).map_err(|e| CoreError::io(file, e))
    }
}

// ---- What the app was started with -------------------------------------------

/// The local path a `file://` address points at. Anything else is `None`.
pub fn path_from_uri(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://").or_else(|| uri.strip_prefix("FILE://"))?;
    // file://host/path: only this computer counts.
    let rest = match rest.find('/') {
        Some(0) => rest,
        Some(i) if rest[..i].eq_ignore_ascii_case("localhost") => &rest[i..],
        _ => return None,
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let mut bytes = Vec::with_capacity(rest.len());
    let raw = rest.as_bytes();
    let mut i = 0;
    while i < raw.len() {
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        if raw[i] == b'%' && raw.len() >= i + 3 {
            if let (Some(h), Some(l)) = (hex(raw[i + 1]), hex(raw[i + 2])) {
                bytes.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        bytes.push(raw[i]);
        i += 1;
    }
    let text = String::from_utf8(bytes).ok()?;
    Some(PathBuf::from(native_from_uri_path(&text)))
}

/// `/C:/Users/me` is `C:\Users\me` on Windows; everywhere else the path is used as it is.
fn native_from_uri_path(path: &str) -> String {
    if cfg!(windows) {
        let p = path.strip_prefix('/').unwrap_or(path);
        if p.as_bytes().get(1) == Some(&b':') {
            return p.replace('/', "\\");
        }
    }
    path.to_string()
}

/// A path as it arrived on the command line: an address, an absolute path, or one relative to `cwd`.
fn resolve(arg: &str, cwd: &Path) -> Option<PathBuf> {
    // Windows hands a drive over as `"C:\"`, which arrives as `C:"` once the quoting is undone.
    let arg = arg.trim().trim_end_matches('"');
    if arg.is_empty() {
        return None;
    }
    if arg.len() >= 7 && arg[..7].eq_ignore_ascii_case("file://") {
        return path_from_uri(arg);
    }
    if arg.contains("://") {
        return None;
    }
    let p = Path::new(arg);
    let full = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    // A bare drive letter means the drive itself.
    if cfg!(windows) && arg.len() == 2 && arg.ends_with(':') {
        return Some(PathBuf::from(format!("{arg}\\")));
    }
    Some(full)
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Showing `path`: a folder opens; a file opens its folder with the file highlighted.
/// With `select`, the item is highlighted in its parent even when it is a folder.
pub fn launch_for(path: &Path, select: bool) -> Option<Launch> {
    let is_dir = path.is_dir();
    if is_dir && !select {
        return Some(Launch { folder: text(path), select: vec![] });
    }
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    match parent {
        Some(parent) if parent.is_dir() => {
            // Something that no longer exists still opens the folder it was in.
            let select = if path.exists() { vec![text(path)] } else { vec![] };
            Some(Launch { folder: text(parent), select })
        }
        // A drive or `/` has no folder above it to be highlighted in.
        _ if is_dir => Some(Launch { folder: text(path), select: vec![] }),
        _ => None,
    }
}

/// Fold several requests into one per folder, keeping the order they came in.
pub fn merge(launches: Vec<Launch>) -> Vec<Launch> {
    let mut out: Vec<Launch> = Vec::new();
    for l in launches {
        match out.iter_mut().find(|o| o.folder == l.folder) {
            Some(o) => {
                for s in l.select {
                    if !o.select.contains(&s) {
                        o.select.push(s);
                    }
                }
            }
            None => out.push(l),
        }
    }
    out
}

/// What to show for a command line (`args[0]` is the program).
///
/// * `fast-file-explorer <folder>` opens the folder.
/// * `fast-file-explorer <file>` opens the file's folder with the file highlighted.
/// * `--select <path>` (or `--select=<path>`, or Explorer's `/select,<path>`) highlights
///   the item in its folder, also when it is a folder.
/// * `file://` addresses work wherever a path does.
///
/// Options the app does not know are skipped, so whatever a system adds does no harm.
pub fn parse_launch(args: &[String], cwd: &Path) -> Vec<Launch> {
    let mut out = Vec::new();
    let mut plain = false;
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        if !plain {
            if arg == "--" {
                plain = true;
                continue;
            }
            if arg == "--select" {
                if let Some(p) = it.next().and_then(|a| resolve(a, cwd)) {
                    out.extend(launch_for(&p, true));
                }
                continue;
            }
            let lower = arg.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("--select=").or_else(|| lower.strip_prefix("/select,")) {
                let rest = &arg[arg.len() - rest.len()..];
                if let Some(p) = resolve(rest, cwd) {
                    out.extend(launch_for(&p, true));
                }
                continue;
            }
            if arg.starts_with("--") || (arg.starts_with('-') && arg.len() > 1 && !Path::new(arg).exists()) {
                continue;
            }
        }
        if let Some(p) = resolve(arg, cwd) {
            out.extend(launch_for(&p, false));
        }
    }
    merge(out)
}

// ---- Being the default ---------------------------------------------------------

/// The app's place as the system's file manager. `saved` is the app's own note of what it changed.
pub fn status(saved: &Path) -> Status {
    imp::status(&Saved::load(saved))
}

/// Make `exe` what the system starts for folders, remembering what was set before.
/// `app_id` is the app's identifier (the bundle identifier on macOS).
pub fn set_default(saved: &Path, exe: &Path, app_id: &str) -> Result<Status> {
    let mut note = Saved::load(saved);
    imp::set_default(&mut note, exe, app_id)?;
    note.active = true;
    note.save(saved)?;
    Ok(imp::status(&note))
}

/// Put back what handled folders before [`set_default`], and remove everything it wrote.
pub fn restore(saved: &Path) -> Result<Status> {
    let mut note = Saved::load(saved);
    imp::restore(&mut note)?;
    note.active = false;
    note.previous.clear();
    note.save(saved)?;
    Ok(imp::status(&note))
}

// ---- Linux ---------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::process::Command;

    /// The desktop entry the app writes for itself; it only exists while the app is the default.
    pub const ENTRY: &str = "dev.envid.fastfileexplorer.folders.desktop";
    const MIME: &str = "inode/directory";
    const SERVICE: &str = "org.freedesktop.FileManager1.service";
    const MARK: &str = "# Written by FAST File Explorer";

    fn data_home() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }

    fn config_home() -> Option<PathBuf> {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }

    fn entry_file() -> Option<PathBuf> {
        Some(data_home()?.join("applications").join(ENTRY))
    }

    fn service_file() -> Option<PathBuf> {
        Some(data_home()?.join("dbus-1/services").join(SERVICE))
    }

    fn current() -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "default", MIME]).output().ok()?;
        let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!id.is_empty()).then_some(id)
    }

    /// A program path the way both desktop entries and D-Bus service files want it quoted.
    /// Left bare when it has nothing that needs quoting: simpler launchers (plain `xdg-open`)
    /// do not understand quotes.
    pub fn quoted(exe: &Path) -> String {
        let plain = exe.to_string_lossy();
        if plain.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+@:,".contains(c)) {
            return plain.into_owned();
        }
        let mut s = String::from("\"");
        for c in exe.to_string_lossy().chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                s.push('\\');
            }
            s.push(c);
        }
        s.push('"');
        s
    }

    pub fn entry_text(exe: &Path) -> String {
        format!(
            "[Desktop Entry]\n{MARK}; removed again when it stops being the default file manager.\n\
             Type=Application\nTryExec={}\nName=FAST File Explorer\nComment=Open folders in FAST File Explorer\n\
             Exec={} %U\nIcon=fast-file-explorer\nTerminal=false\nNoDisplay=true\n\
             MimeType={MIME};\nCategories=System;FileManager;\nStartupNotify=true\n",
            exe.to_string_lossy(),
            quoted(exe)
        )
    }

    pub fn service_text(exe: &Path) -> String {
        format!("{MARK}\n[D-BUS Service]\nName=org.freedesktop.FileManager1\nExec={}\n", quoted(exe))
    }

    /// Take every mention of the app's entry out of a `mimeapps.list`.
    pub fn without_entry(list: &str) -> String {
        let mut out = String::with_capacity(list.len());
        for line in list.lines() {
            match line.split_once('=') {
                Some((key, value)) if value.split(';').any(|v| v.trim() == ENTRY) => {
                    let rest: Vec<&str> = value.split(';').map(str::trim).filter(|v| !v.is_empty() && *v != ENTRY).collect();
                    if !rest.is_empty() {
                        out.push_str(&format!("{key}={};\n", rest.join(";")));
                    }
                }
                _ => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        out
    }

    pub fn status(note: &Saved) -> Status {
        let limits = vec![
            "A file manager that is already running keeps answering \"Show in folder\" until it is closed or you sign in again.".to_string(),
        ];
        if entry_file().is_none() || Command::new("xdg-mime").arg("--version").output().is_err() {
            return Status {
                supported: false,
                detail: "This needs the xdg-utils package (xdg-mime), which was not found.".into(),
                limits,
                ..Default::default()
            };
        }
        let active = current().as_deref() == Some(ENTRY);
        let previous = note.previous.get("mime").cloned();
        let detail = if active {
            "Folders, and \"Show in folder\" from browsers and other apps, open here.".to_string()
        } else {
            "Folders opened from other apps go to the system's file manager.".to_string()
        };
        Status { supported: true, active, previous, detail, limits }
    }

    pub fn set_default(note: &mut Saved, exe: &Path, _app_id: &str) -> Result<()> {
        // An AppImage runs from a mount that is gone after it closes; the image itself is what to start.
        let exe = std::env::var_os("APPIMAGE").map(PathBuf::from).filter(|p| p.is_file()).unwrap_or_else(|| exe.to_path_buf());
        let entry = entry_file().ok_or(CoreError::Unsupported("no home folder"))?;
        let service = service_file().ok_or(CoreError::Unsupported("no home folder"))?;

        match current() {
            Some(id) if id != ENTRY => {
                note.previous.insert("mime".into(), id);
            }
            _ => {}
        }
        for file in [&entry, &service] {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
            }
        }
        std::fs::write(&entry, entry_text(&exe)).map_err(|e| CoreError::io(&entry, e))?;
        // Someone else's service file of the same name is kept so it can be put back.
        if let Ok(old) = std::fs::read_to_string(&service) {
            if !old.contains(MARK) {
                note.previous.insert("service".into(), old);
            }
        }
        std::fs::write(&service, service_text(&exe)).map_err(|e| CoreError::io(&service, e))?;

        let done = Command::new("xdg-mime").args(["default", ENTRY, MIME]).status();
        if !matches!(done, Ok(s) if s.success()) || current().as_deref() != Some(ENTRY) {
            let _ = std::fs::remove_file(&entry);
            let _ = std::fs::remove_file(&service);
            return Err(CoreError::Invalid("The system did not accept the change (xdg-mime failed).".into()));
        }
        if let Some(dir) = entry.parent() {
            let _ = Command::new("update-desktop-database").arg(dir).status();
        }
        Ok(())
    }

    pub fn restore(note: &mut Saved) -> Result<()> {
        // First forget the app everywhere, then name the earlier program again.
        if let Some(cfg) = config_home() {
            for list in [cfg.join("mimeapps.list"), data_home().unwrap_or_default().join("applications/mimeapps.list")] {
                if let Ok(text) = std::fs::read_to_string(&list) {
                    let cleaned = without_entry(&text);
                    if cleaned != text {
                        std::fs::write(&list, cleaned).map_err(|e| CoreError::io(&list, e))?;
                    }
                }
            }
        }
        if let Some(prev) = note.previous.get("mime") {
            let _ = Command::new("xdg-mime").args(["default", prev, MIME]).status();
        }
        if let Some(entry) = entry_file() {
            let _ = std::fs::remove_file(&entry);
            if let Some(dir) = entry.parent() {
                let _ = Command::new("update-desktop-database").arg(dir).status();
            }
        }
        if let Some(service) = service_file() {
            let ours = std::fs::read_to_string(&service).map(|t| t.contains(MARK)).unwrap_or(false);
            if ours {
                match note.previous.get("service") {
                    Some(old) => std::fs::write(&service, old).map_err(|e| CoreError::io(&service, e))?,
                    None => {
                        let _ = std::fs::remove_file(&service);
                    }
                }
            }
        }
        Ok(())
    }
}

/// The freedesktop "show in folder" service (`org.freedesktop.FileManager1`): what browsers
/// and other apps call for "Show in folder" on Linux.
#[cfg(target_os = "linux")]
pub mod service {
    use super::*;
    use zbus::blocking::Connection;
    use zbus::fdo::{RequestNameFlags, RequestNameReply};

    const NAME: &str = "org.freedesktop.FileManager1";
    const PATH: &str = "/org/freedesktop/FileManager1";

    struct Manager {
        on: Box<dyn Fn(Vec<Launch>) + Send + Sync>,
    }

    fn launches(uris: &[String], select: bool) -> Vec<Launch> {
        let found = uris
            .iter()
            .filter_map(|u| path_from_uri(u).or_else(|| Path::new(u).is_absolute().then(|| PathBuf::from(u))))
            .filter_map(|p| launch_for(&p, select))
            .collect();
        merge(found)
    }

    #[zbus::interface(name = "org.freedesktop.FileManager1")]
    impl Manager {
        fn show_folders(&self, uris: Vec<String>, _startup_id: &str) {
            (self.on)(launches(&uris, false));
        }

        fn show_items(&self, uris: Vec<String>, _startup_id: &str) {
            (self.on)(launches(&uris, true));
        }

        fn show_item_properties(&self, uris: Vec<String>, _startup_id: &str) {
            (self.on)(launches(&uris, true));
        }
    }

    /// The service while it is being offered; dropping it gives the name back.
    pub struct Service {
        conn: Connection,
        /// False while another file manager still holds the name (the app is next in line).
        pub owner: bool,
    }

    impl Drop for Service {
        fn drop(&mut self) {
            let _ = self.conn.release_name(NAME);
        }
    }

    /// Offer the service on the session bus (or on `address`, for tests). `on` runs on a
    /// background thread for every request.
    pub fn serve(address: Option<&str>, on: impl Fn(Vec<Launch>) + Send + Sync + 'static) -> Result<Service> {
        let bad = |e: zbus::Error| CoreError::Invalid(format!("The \"show in folder\" service could not start ({e})."));
        let builder = match address {
            Some(a) => zbus::blocking::connection::Builder::address(a).map_err(bad)?,
            None => zbus::blocking::connection::Builder::session().map_err(bad)?,
        };
        let conn = builder.serve_at(PATH, Manager { on: Box::new(on) }).map_err(bad)?.build().map_err(bad)?;
        // Take the name over where its holder allows that; otherwise wait in line for it.
        let reply = conn
            .request_name_with_flags(NAME, RequestNameFlags::ReplaceExisting | RequestNameFlags::AllowReplacement)
            .map_err(bad)?;
        let owner = matches!(reply, RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner);
        // A new service file is only seen once the bus looks again.
        let _ = conn.call_method(Some("org.freedesktop.DBus"), "/org/freedesktop/DBus", Some("org.freedesktop.DBus"), "ReloadConfig", &());
        Ok(Service { conn, owner })
    }
}

// ---- Windows -------------------------------------------------------------------

#[cfg(windows)]
#[path = "filemanager_windows.rs"]
mod imp;

// ---- macOS ---------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[path = "filemanager_macos.rs"]
mod imp;

// ---- Everything else -------------------------------------------------------------

#[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
mod imp {
    use super::*;

    pub fn status(_note: &Saved) -> Status {
        Status { detail: "Phones and tablets do not let an app replace their file manager.".into(), ..Default::default() }
    }

    pub fn set_default(_note: &mut Saved, _exe: &Path, _app_id: &str) -> Result<()> {
        Err(CoreError::Unsupported("default file manager"))
    }

    pub fn restore(_note: &mut Saved) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("app").chain(list.iter().copied()).map(String::from).collect()
    }

    #[test]
    fn a_folder_opens_and_a_file_is_highlighted_in_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("report.pdf");
        std::fs::write(&file, b"x").unwrap();
        let d = text(dir.path());

        assert_eq!(parse_launch(&args(&[&d]), dir.path()), vec![Launch { folder: d.clone(), select: vec![] }]);
        assert_eq!(
            parse_launch(&args(&[&text(&file)]), dir.path()),
            vec![Launch { folder: d.clone(), select: vec![text(&file)] }]
        );
        // Relative to where the command was run.
        assert_eq!(
            parse_launch(&args(&["report.pdf"]), dir.path()),
            vec![Launch { folder: d.clone(), select: vec![text(&file)] }]
        );
    }

    #[test]
    fn select_highlights_a_folder_in_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("Photos");
        std::fs::create_dir(&sub).unwrap();
        let want = vec![Launch { folder: text(dir.path()), select: vec![text(&sub)] }];
        assert_eq!(parse_launch(&args(&["--select", &text(&sub)]), dir.path()), want);
        assert_eq!(parse_launch(&args(&[&format!("--select={}", text(&sub))]), dir.path()), want);
        assert_eq!(parse_launch(&args(&[&format!("/select,{}", text(&sub))]), dir.path()), want);
    }

    #[test]
    fn several_items_in_one_folder_become_one_request() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, b"").unwrap();
        std::fs::write(&b, b"").unwrap();
        let got = parse_launch(&args(&[&text(&a), &text(&b), &text(&a)]), dir.path());
        assert_eq!(got, vec![Launch { folder: text(dir.path()), select: vec![text(&a), text(&b)] }]);
    }

    #[test]
    fn unknown_options_and_missing_things_do_no_harm() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("deleted.zip");
        // A file that no longer exists still opens the folder it was in.
        assert_eq!(
            parse_launch(&args(&["--flag", "-psn_0_123", &text(&gone)]), dir.path()),
            vec![Launch { folder: text(dir.path()), select: vec![] }]
        );
        assert!(parse_launch(&args(&["--job-b64", "https://example.com/x"]), dir.path()).len() <= 1);
        assert!(parse_launch(&args(&[]), dir.path()).is_empty());
        assert!(parse_launch(&args(&["/no/such/place/at/all/file.txt"]), dir.path()).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn addresses_are_decoded() {
        assert_eq!(path_from_uri("file:///home/me/My%20Files/a%23b.txt"), Some(PathBuf::from("/home/me/My Files/a#b.txt")));
        assert_eq!(path_from_uri("file://localhost/tmp/x"), Some(PathBuf::from("/tmp/x")));
        assert_eq!(path_from_uri("file:///tmp/caf%C3%A9"), Some(PathBuf::from("/tmp/café")));
        assert_eq!(path_from_uri("file://otherhost/tmp/x"), None);
        assert_eq!(path_from_uri("https://example.com/"), None);
        assert_eq!(path_from_uri("file:///tmp/100%"), Some(PathBuf::from("/tmp/100%")));

        let dir = tempfile::tempdir().unwrap();
        let spaced = dir.path().join("two words");
        std::fs::create_dir(&spaced).unwrap();
        let uri = format!("file://{}", text(&spaced).replace(' ', "%20"));
        assert_eq!(parse_launch(&args(&[&uri]), dir.path()), vec![Launch { folder: text(&spaced), select: vec![] }]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_app_is_taken_out_of_a_mimeapps_list_without_touching_the_rest() {
        let entry = imp::ENTRY;
        let list = format!(
            "[Default Applications]\ninode/directory={entry}\ntext/plain=org.gnome.gedit.desktop\n\n[Added Associations]\ninode/directory={entry};org.gnome.Nautilus.desktop;\n"
        );
        assert_eq!(
            imp::without_entry(&list),
            "[Default Applications]\ntext/plain=org.gnome.gedit.desktop\n\n[Added Associations]\ninode/directory=org.gnome.Nautilus.desktop;\n"
        );
    }

    /// A private message bus, when the machine has `dbus-daemon`; the test is skipped without it.
    #[cfg(target_os = "linux")]
    fn private_bus() -> Option<(std::process::Child, String)> {
        use std::io::BufRead;
        let mut child = std::process::Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address"])
            .stdout(std::process::Stdio::piped())
            .spawn()
            .ok()?;
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take()?).read_line(&mut line).ok()?;
        Some((child, line.trim().to_string()))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn show_in_folder_requests_from_other_apps_arrive() {
        let Some((mut bus, address)) = private_bus() else { return };
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("my download.zip");
        std::fs::write(&file, b"x").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let served = service::serve(Some(&address), move |l| tx.send(l).unwrap()).unwrap();
        assert!(served.owner);

        // What a browser does for "Show in folder".
        let caller = zbus::blocking::connection::Builder::address(address.as_str()).unwrap().build().unwrap();
        let uri = format!("file://{}", text(&file).replace(' ', "%20"));
        let call = |method: &str, uri: &str| {
            caller
                .call_method(
                    Some("org.freedesktop.FileManager1"),
                    "/org/freedesktop/FileManager1",
                    Some("org.freedesktop.FileManager1"),
                    method,
                    &(vec![uri.to_string()], ""),
                )
                .unwrap();
            rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap()
        };
        assert_eq!(call("ShowItems", &uri), vec![Launch { folder: text(dir.path()), select: vec![text(&file)] }]);
        let folder_uri = format!("file://{}", text(dir.path()));
        assert_eq!(call("ShowFolders", &folder_uri), vec![Launch { folder: text(dir.path()), select: vec![] }]);

        // Giving the service up frees the name for the next file manager.
        drop(served);
        let free = caller.request_name_with_flags("org.freedesktop.FileManager1", zbus::fdo::RequestNameFlags::DoNotQueue.into()).unwrap();
        assert_eq!(free, zbus::fdo::RequestNameReply::PrimaryOwner);
        let _ = bus.kill();
        let _ = bus.wait();
    }

    /// Changes real settings, so it only runs when asked, with a throwaway home folder:
    /// `HOME=/tmp/x XDG_CONFIG_HOME=/tmp/x/.config XDG_DATA_HOME=/tmp/x/.local/share cargo test -p fast-core becoming -- --ignored`
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "changes the default file manager of whoever runs it"]
    fn becoming_the_default_and_giving_it_back() {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        assert!(home.starts_with("/tmp"), "run this with a throwaway HOME");
        let apps = home.join(".local/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        // A file manager that was the default before.
        std::fs::write(apps.join("old-fm.desktop"), "[Desktop Entry]\nType=Application\nName=Old\nExec=/bin/true %U\nMimeType=inode/directory;\n").unwrap();
        assert!(std::process::Command::new("xdg-mime").args(["default", "old-fm.desktop", "inode/directory"]).status().unwrap().success());

        let saved = home.join("note.json");
        let exe = std::env::var_os("FAST_EXE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/bin/true"));
        assert!(!status(&saved).active);
        let on = set_default(&saved, &exe, "dev.envid.fastfileexplorer").unwrap();
        assert!(on.active);
        assert_eq!(on.previous.as_deref(), Some("old-fm.desktop"));
        assert!(apps.join(imp::ENTRY).is_file());
        assert!(home.join(".local/share/dbus-1/services/org.freedesktop.FileManager1.service").is_file());
        // Doing it twice keeps the note of what was there first.
        assert_eq!(set_default(&saved, &exe, "x").unwrap().previous.as_deref(), Some("old-fm.desktop"));

        if std::env::var_os("FAST_KEEP").is_some() {
            return;
        }
        let off = restore(&saved).unwrap();
        assert!(!off.active);
        assert!(!apps.join(imp::ENTRY).exists());
        assert!(!home.join(".local/share/dbus-1/services/org.freedesktop.FileManager1.service").exists());
        let out = std::process::Command::new("xdg-mime").args(["query", "default", "inode/directory"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "old-fm.desktop");
        let list = std::fs::read_to_string(home.join(".config/mimeapps.list")).unwrap();
        assert!(!list.contains(imp::ENTRY), "{list}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn program_paths_with_spaces_are_quoted() {
        assert!(imp::entry_text(Path::new("/usr/bin/fast-file-explorer")).contains("Exec=/usr/bin/fast-file-explorer %U\n"));
        let exe = Path::new("/opt/FAST File Explorer/fast-file-explorer");
        assert!(imp::entry_text(exe).contains("Exec=\"/opt/FAST File Explorer/fast-file-explorer\" %U\n"));
        assert!(imp::entry_text(exe).contains("MimeType=inode/directory;\n"));
        assert!(imp::service_text(exe).contains("Name=org.freedesktop.FileManager1\nExec=\"/opt/FAST File Explorer/fast-file-explorer\"\n"));
    }
}
