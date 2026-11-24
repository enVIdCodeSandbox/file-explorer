//! macOS privacy protection ("TCC"): which folders ask the person for permission, and whether the app has been
//! given access to everything at once (Full Disk Access).
//!
//! Without that one permission macOS asks separately for Desktop, Documents, Downloads, Music, Photos, iCloud Drive,
//! every removable and network volume, and other apps' data, each time something *opens* them. A file manager that
//! indexes the disk, sizes folders and watches them in the background would trigger a prompt for each. So:
//!
//! * background work (search index, disk usage, folder sizes, listing preload, folder watching) never enters a
//!   protected folder unless the app has full access; [`background_ok`] answers that;
//! * what the person opens themselves still works, and macOS asks at most once per kind of folder;
//! * the app asks for the one permission itself, once (first run, or the first launch after this was added), and
//!   Settings keeps the buttons for later.
//!
//! On every other system nothing is protected and every function says "yes".

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A group of protected places the person can switch on or off for background work in Settings.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: &'static str,
    pub label: &'static str,
    /// Where it lives, for the settings page ("~/Documents").
    #[serde(rename = "where")]
    pub where_: String,
}

/// The categories, in the order the settings page lists them. Empty where nothing is protected.
pub fn categories() -> Vec<Category> {
    if !applies() {
        return Vec::new();
    }
    let c = |id, label, where_: &str| Category { id, label, where_: where_.to_string() };
    vec![
        c("desktop", "Desktop", "~/Desktop"),
        c("documents", "Documents", "~/Documents"),
        c("downloads", "Downloads", "~/Downloads"),
        c("media", "Music, Pictures and Movies", "~/Music, ~/Pictures, ~/Movies"),
        c("icloud", "iCloud Drive", "~/Library/Mobile Documents"),
        c("library", "Other apps' data", "~/Library"),
        c("volumes", "External and network drives", "/Volumes"),
    ]
}

/// The home folder, from the environment, which is how macOS sets it for a program started from Finder. (The `dirs`
/// crate is not built for phones and tablets, where this file must still compile and says "nothing is protected".)
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// True where this system protects folders the way macOS does.
pub fn applies() -> bool {
    cfg!(target_os = "macos")
}

/// Which category a path belongs to, or None when it is not protected. `home` is passed in so this can be tested.
pub fn category_of(path: &Path, home: Option<&Path>) -> Option<&'static str> {
    if let Some(rest) = path.strip_prefix("/Volumes").ok() {
        // /Volumes itself and the boot volume's link are fine to list; what is inside any other volume is protected.
        return if rest.as_os_str().is_empty() { None } else { Some("volumes") };
    }
    let home = home?;
    let rest = path.strip_prefix(home).ok()?;
    let mut parts = rest.components();
    let first = parts.next()?.as_os_str().to_str()?;
    match first {
        "Desktop" => Some("desktop"),
        "Documents" => Some("documents"),
        "Downloads" => Some("downloads"),
        "Music" | "Pictures" | "Movies" => Some("media"),
        "Library" => {
            let second = parts.next().and_then(|c| c.as_os_str().to_str());
            Some(if second == Some("Mobile Documents") { "icloud" } else { "library" })
        }
        _ => None,
    }
}

/// What the app can see of the protected places. Cached for a few seconds: asked on every listing, it must be cheap.
static PROBE: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
const PROBE_FOR: Duration = Duration::from_secs(4);

/// Whether the app has been given access to everything. Always true where nothing is protected.
pub fn full_access() -> bool {
    if !applies() {
        return true;
    }
    let mut slot = PROBE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, v)) = *slot {
        if at.elapsed() < PROBE_FOR {
            return v;
        }
    }
    let v = probe();
    *slot = Some((Instant::now(), v));
    v
}

/// Forget the cached answer, so the next check looks again (after the person came back from System Settings).
pub fn recheck() -> bool {
    *PROBE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    full_access()
}

/// Safari's folder and the privacy database are readable only with Full Disk Access, and reading them is refused
/// quietly: no prompt is shown for it, which is what makes it a safe test.
fn probe() -> bool {
    let Some(home) = home() else { return true };
    decide(
        std::fs::read_dir(home.join("Library/Safari")).map(|_| ()),
        || std::fs::File::open("/Library/Application Support/com.apple.TCC/TCC.db").map(|_| ()),
    )
}

/// `first` is the result of reading Safari's folder. Only a refusal means "no access"; a folder that is not there
/// says nothing, so the privacy database is asked instead; if neither can be judged, assume access.
fn decide(first: std::io::Result<()>, second: impl FnOnce() -> std::io::Result<()>) -> bool {
    match first {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => false,
        Err(_) => match second() {
            Ok(()) => true,
            Err(e) => e.kind() != std::io::ErrorKind::PermissionDenied,
        },
    }
}

/// The result of asking for one kind of folder.
#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asked {
    pub id: &'static str,
    pub label: &'static str,
    /// macOS said yes (or there was nothing to ask about).
    pub granted: bool,
}

/// The folders that ask on their own, one by one, in the order a person expects them.
const ASKED: [(&str, &str, &str); 6] = [
    ("desktop", "Desktop", "Desktop"),
    ("documents", "Documents", "Documents"),
    ("downloads", "Downloads", "Downloads"),
    ("music", "Music", "Music"),
    ("pictures", "Pictures", "Pictures"),
    ("movies", "Movies", "Movies"),
];

/// Without Full Disk Access: open each of the usual folders once, right now, so macOS shows all of its questions
/// together instead of scattering them over the next days. The answers are remembered by macOS. Nothing is read
/// from the folders; one listing is enough to make the question appear. External and network drives are not
/// included, because those ask per drive, and only when one is used.
pub fn ask_for_folders() -> Vec<Asked> {
    if !applies() {
        return Vec::new();
    }
    match home() {
        Some(home) => ask_in(&home),
        None => Vec::new(),
    }
}

fn ask_in(home: &Path) -> Vec<Asked> {
    let mut out: Vec<Asked> = ASKED
        .iter()
        .map(|&(id, label, dir)| {
            let found = std::fs::read_dir(home.join(dir)).map(|_| ());
            // A folder that is not there is not a refusal.
            Asked { id, label, granted: !matches!(&found, Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied) }
        })
        .collect();
    let icloud = std::fs::read_dir(home.join("Library/Mobile Documents")).map(|_| ());
    out.push(Asked { id: "icloud", label: "iCloud Drive", granted: !matches!(&icloud, Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied) });
    out
}

/// May background work look inside `path`? `off` holds the category ids the person switched off in Settings.
pub fn background_ok(path: &Path, off: &[String]) -> bool {
    if !applies() {
        return true;
    }
    background_ok_with(path, off, home().as_deref(), full_access)
}

fn background_ok_with(path: &Path, off: &[String], home: Option<&Path>, full: impl Fn() -> bool) -> bool {
    match category_of(path, home) {
        None => true,
        Some(cat) => full() && !off.iter().any(|o| o == cat),
    }
}

/// The folders background work must stay out of, as absolute paths (for a scan's skip list).
pub fn skip_list(off: &[String]) -> Vec<PathBuf> {
    if !applies() {
        return Vec::new();
    }
    let Some(home) = home() else { return Vec::new() };
    let all = [
        ("desktop", home.join("Desktop")),
        ("documents", home.join("Documents")),
        ("downloads", home.join("Downloads")),
        ("media", home.join("Music")),
        ("media", home.join("Pictures")),
        ("media", home.join("Movies")),
        ("icloud", home.join("Library/Mobile Documents")),
        ("library", home.join("Library")),
    ];
    let full = full_access();
    all.into_iter().filter(|(cat, _)| !full || off.iter().any(|o| o == cat)).map(|(_, p)| p).collect()
}

/// Mounted volumes background work must stay out of (everything under /Volumes except the system disk's link).
pub fn blocked_volumes(off: &[String]) -> bool {
    applies() && !(full_access() && !off.iter().any(|o| o == "volumes"))
}

/// The panes of System Settings the app can open.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pane {
    FullDisk,
    FilesAndFolders,
    Photos,
    Media,
}

impl Pane {
    pub fn parse(s: &str) -> Option<Pane> {
        match s {
            "fullDisk" => Some(Pane::FullDisk),
            "files" => Some(Pane::FilesAndFolders),
            "photos" => Some(Pane::Photos),
            "media" => Some(Pane::Media),
            _ => None,
        }
    }

    fn anchor(self) -> &'static str {
        match self {
            Pane::FullDisk => "Privacy_AllFiles",
            Pane::FilesAndFolders => "Privacy_FilesAndFolders",
            Pane::Photos => "Privacy_Photos",
            Pane::Media => "Privacy_Media",
        }
    }
}

/// The address `open` is given for a pane.
pub fn pane_url(pane: Pane) -> String {
    format!("x-apple.systempreferences:com.apple.preference.security?{}", pane.anchor())
}

/// Open a pane of System Settings. Does nothing, and says so, on other systems.
pub fn open_pane(pane: Pane) -> crate::Result<()> {
    if !applies() {
        return Err(crate::CoreError::Invalid("this system has no such settings".into()));
    }
    // Looking at what the app is allowed also makes the app appear in the list for the person to switch on.
    let _ = recheck();
    std::process::Command::new("open").arg(pane_url(pane)).spawn().map(|_| ()).map_err(|e| crate::CoreError::Invalid(e.to_string()))
}

/// The app's own bundle ("…/FAST File Explorer.app"), found from the running program, so the person can drag it into
/// the Full Disk Access list when it is not there yet.
pub fn app_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(|p| p.to_path_buf())
}

/// Show the app in Finder, selected.
pub fn reveal_app() -> crate::Result<()> {
    let Some(app) = app_bundle() else { return Err(crate::CoreError::Invalid("the app is not running from an application bundle".into())) };
    std::process::Command::new("open").arg("-R").arg(app).spawn().map(|_| ()).map_err(|e| crate::CoreError::Invalid(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    const HOME: &str = "/Users/ada";

    #[test]
    fn asking_for_folders_lists_each_and_treats_missing_ones_as_fine() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("Desktop")).unwrap();
        let got = ask_in(dir.path());
        assert_eq!(got.len(), 7);
        assert!(got.iter().all(|a| a.granted), "{got:?}");
        assert_eq!(got[0].id, "desktop");
        assert_eq!(got[6].id, "icloud");
    }

    fn cat(p: &str) -> Option<&'static str> {
        category_of(Path::new(p), Some(Path::new(HOME)))
    }

    #[test]
    fn protected_places_are_named() {
        assert_eq!(cat("/Users/ada/Documents"), Some("documents"));
        assert_eq!(cat("/Users/ada/Documents/work/a.txt"), Some("documents"));
        assert_eq!(cat("/Users/ada/Desktop"), Some("desktop"));
        assert_eq!(cat("/Users/ada/Downloads/x.zip"), Some("downloads"));
        assert_eq!(cat("/Users/ada/Music/Music/Library"), Some("media"));
        assert_eq!(cat("/Users/ada/Pictures"), Some("media"));
        assert_eq!(cat("/Users/ada/Movies"), Some("media"));
        assert_eq!(cat("/Users/ada/Library/Mobile Documents/com~apple~CloudDocs"), Some("icloud"));
        assert_eq!(cat("/Users/ada/Library/Mail"), Some("library"));
        assert_eq!(cat("/Volumes/Backup/photos"), Some("volumes"));
    }

    #[test]
    fn ordinary_places_are_not_protected() {
        assert_eq!(cat("/Users/ada"), None);
        assert_eq!(cat("/Users/ada/Projects/app"), None);
        assert_eq!(cat("/Users/ada/DocumentsOld"), None);
        assert_eq!(cat("/Applications"), None);
        assert_eq!(cat("/Volumes"), None);
        assert_eq!(cat("/tmp/x"), None);
    }

    #[test]
    fn without_full_access_nothing_protected_is_entered_in_the_background() {
        let no = || false;
        assert!(!background_ok_with(Path::new("/Users/ada/Documents"), &[], Some(Path::new(HOME)), no));
        assert!(!background_ok_with(Path::new("/Volumes/USB/a"), &[], Some(Path::new(HOME)), no));
        assert!(background_ok_with(Path::new("/Users/ada/Projects"), &[], Some(Path::new(HOME)), no));
    }

    #[test]
    fn with_full_access_everything_is_allowed_except_what_the_person_switched_off() {
        let yes = || true;
        let off = vec!["downloads".to_string()];
        assert!(background_ok_with(Path::new("/Users/ada/Documents"), &off, Some(Path::new(HOME)), yes));
        assert!(!background_ok_with(Path::new("/Users/ada/Downloads/a"), &off, Some(Path::new(HOME)), yes));
    }

    #[test]
    fn only_a_refusal_means_no_access() {
        let refused = || Err(Error::from(ErrorKind::PermissionDenied));
        let missing = || Err(Error::from(ErrorKind::NotFound));
        assert!(decide(Ok(()), refused));
        assert!(!decide(refused(), || Ok(())));
        assert!(decide(missing(), || Ok(())));
        assert!(!decide(missing(), refused));
        assert!(decide(missing(), missing), "nothing to judge by: assume access");
    }

    #[test]
    fn panes_have_addresses() {
        assert_eq!(pane_url(Pane::FullDisk), "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles");
        assert_eq!(Pane::parse("files"), Some(Pane::FilesAndFolders));
        assert_eq!(Pane::parse("nope"), None);
    }

    #[test]
    fn other_systems_are_never_held_back() {
        if !applies() {
            assert!(full_access());
            assert!(background_ok(Path::new("/anything"), &[]));
            assert!(skip_list(&[]).is_empty());
            assert!(categories().is_empty());
        }
    }
}
