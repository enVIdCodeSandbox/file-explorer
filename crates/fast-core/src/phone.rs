//! Storage and machine facts on phones and tablets.
//!
//! A phone has no drive letters or mount table an app may read. Android keeps
//! the files people see under `/storage` (the built-in storage at
//! `/storage/emulated/0`, each memory card or USB stick in a folder named by
//! its volume id), and shows them to an app that holds "All files access". iOS
//! gives each app one folder of its own, which the Files app lists under "On
//! My iPhone".
//!
//! Everything here takes the folders to look in as arguments, so the rules are
//! tested on any system; the thin `cfg` wrappers at the bottom pass the real ones.

use std::path::{Path, PathBuf};

/// One place files live on the device.
#[derive(Clone, Debug, PartialEq)]
pub struct Root {
    pub name: String,
    pub path: PathBuf,
    /// A memory card or USB drive, as opposed to built-in storage.
    pub removable: bool,
}

/// Android's storage roots: built-in storage first, then cards and USB drives.
/// `storage` is normally `/storage`; `external` is `$EXTERNAL_STORAGE`, used when
/// the usual folder is missing (some vendors and emulators move it).
pub fn android_roots(storage: &Path, external: Option<&Path>) -> Vec<Root> {
    let mut out = Vec::new();
    let usual = storage.join("emulated").join("0");
    let internal = if usual.is_dir() { Some(usual) } else { external.filter(|p| p.is_dir()).map(Path::to_path_buf) };
    if let Some(path) = internal {
        out.push(Root { name: "Internal storage".into(), path, removable: false });
    }
    let mut cards: Vec<PathBuf> = std::fs::read_dir(storage)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !matches!(p.file_name().and_then(|n| n.to_str()), Some("emulated" | "self") | None))
        .collect();
    cards.sort();
    let many = cards.len() > 1;
    for path in cards {
        let id = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        out.push(Root { name: if many { format!("SD card ({id})") } else { "SD card".into() }, path, removable: true });
    }
    out
}

/// iOS: the app's own Documents folder, made if it is not there yet.
pub fn ios_roots(home: &Path) -> Vec<Root> {
    let docs = home.join("Documents");
    let _ = std::fs::create_dir_all(&docs);
    if docs.is_dir() {
        vec![Root { name: "On this device".into(), path: docs, removable: false }]
    } else {
        Vec::new()
    }
}

/// The usual folders inside built-in storage that exist, as (label, path).
pub fn folders_in(root: &Path) -> Vec<(&'static str, String)> {
    const NAMES: [(&str, &str); 7] = [
        ("Downloads", "Download"),
        ("Documents", "Documents"),
        ("Pictures", "Pictures"),
        ("Camera", "DCIM"),
        ("Music", "Music"),
        ("Videos", "Movies"),
        ("Screenshots", "Pictures/Screenshots"),
    ];
    let mut out = vec![("Home", root.to_string_lossy().into_owned())];
    for (label, rel) in NAMES {
        let p = root.join(rel);
        if p.is_dir() {
            out.push((label, p.to_string_lossy().into_owned()));
        }
    }
    out
}

/// Installed and available memory in MB, from the text of `/proc/meminfo`.
pub fn meminfo(text: &str) -> Option<(u64, u64)> {
    let kb = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
    };
    let total = kb("MemTotal:")?;
    let available = kb("MemAvailable:").or_else(|| kb("MemFree:")).unwrap_or(total / 4);
    Some((total / 1024, available / 1024))
}

/// Whether the device is running on its battery, and the charge in percent, from a
/// `power_supply` folder (`/sys/class/power_supply`). Phones often hide it from apps;
/// then the answer is "on battery, charge unknown", the careful assumption.
pub fn battery_in(dir: &Path) -> (bool, Option<f32>) {
    for name in ["battery", "Battery", "BAT0", "bms"] {
        let b = dir.join(name);
        let Ok(capacity) = std::fs::read_to_string(b.join("capacity")) else { continue };
        let percent = capacity.trim().parse::<f32>().ok().filter(|p| (0.0..=100.0).contains(p));
        let status = std::fs::read_to_string(b.join("status")).unwrap_or_default();
        let plugged = matches!(status.trim(), "Charging" | "Full" | "Not charging");
        return (!plugged, percent);
    }
    (true, None)
}

/// Total and free bytes of the volume holding `path`.
#[cfg(unix)]
pub fn space(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `c` is a valid NUL-terminated path and `s` is a zeroed struct the call fills in.
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        let block = if s.f_frsize > 0 { s.f_frsize as u64 } else { s.f_bsize as u64 };
        Some((s.f_blocks as u64 * block, s.f_bavail as u64 * block))
    }
}

#[cfg(not(unix))]
pub fn space(_path: &Path) -> Option<(u64, u64)> {
    None
}

/// The storage roots of the device this is running on.
#[cfg(target_os = "android")]
pub fn roots() -> Vec<Root> {
    let external = std::env::var_os("EXTERNAL_STORAGE").map(PathBuf::from);
    let found = android_roots(Path::new("/storage"), external.as_deref());
    if found.is_empty() && Path::new("/sdcard").is_dir() {
        return vec![Root { name: "Internal storage".into(), path: PathBuf::from("/sdcard"), removable: false }];
    }
    found
}

#[cfg(target_os = "ios")]
pub fn roots() -> Vec<Root> {
    std::env::var_os("HOME").map(|h| ios_roots(Path::new(&h))).unwrap_or_default()
}

/// Installed and available memory in MB on this device.
#[cfg(target_os = "android")]
pub fn memory_mb() -> Option<(u64, u64)> {
    meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "ios")]
pub fn memory_mb() -> Option<(u64, u64)> {
    let mut bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: the name is NUL-terminated and the output buffer is a u64 of the length given.
    let ok = unsafe { libc::sysctlbyname(c"hw.memsize".as_ptr(), (&mut bytes as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) } == 0;
    if !ok || bytes == 0 {
        return None;
    }
    let total = bytes / (1024 * 1024);
    // iOS does not say how much is free; an app may count on roughly a quarter.
    Some((total, total / 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_lists_built_in_storage_then_cards() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("emulated/0/Download")).unwrap();
        std::fs::create_dir_all(t.path().join("self/primary")).unwrap();
        std::fs::create_dir_all(t.path().join("1A2B-3C4D")).unwrap();
        let roots = android_roots(t.path(), None);
        assert_eq!(roots.len(), 2);
        assert_eq!((roots[0].name.as_str(), roots[0].removable), ("Internal storage", false));
        assert!(roots[0].path.ends_with("emulated/0"));
        assert_eq!((roots[1].name.as_str(), roots[1].removable), ("SD card", true));

        std::fs::create_dir_all(t.path().join("9F00-0001")).unwrap();
        let names: Vec<String> = android_roots(t.path(), None).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Internal storage", "SD card (1A2B-3C4D)", "SD card (9F00-0001)"]);
    }

    #[test]
    fn android_falls_back_to_the_folder_the_system_names() {
        let t = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let roots = android_roots(t.path(), Some(elsewhere.path()));
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].path, elsewhere.path());
        assert!(android_roots(t.path(), Some(Path::new("/no/such/place"))).is_empty());
    }

    #[test]
    fn ios_makes_and_lists_the_documents_folder() {
        let t = tempfile::tempdir().unwrap();
        let roots = ios_roots(t.path());
        assert_eq!(roots.len(), 1);
        assert!(t.path().join("Documents").is_dir());
        assert_eq!(roots[0].path, t.path().join("Documents"));
    }

    #[test]
    fn only_folders_that_exist_are_offered() {
        let t = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(t.path().join("Download")).unwrap();
        std::fs::create_dir_all(t.path().join("Pictures/Screenshots")).unwrap();
        let labels: Vec<&str> = folders_in(t.path()).into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, ["Home", "Downloads", "Pictures", "Screenshots"]);
    }

    #[test]
    fn memory_is_read_from_meminfo() {
        let text = "MemTotal:        7654321 kB\nMemFree:          123456 kB\nMemAvailable:    2345678 kB\n";
        assert_eq!(meminfo(text), Some((7474, 2290)));
        assert_eq!(meminfo("MemTotal: 2048000 kB\nMemFree: 512000 kB\n"), Some((2000, 500)));
        assert_eq!(meminfo("nothing useful"), None);
    }

    #[test]
    fn battery_is_read_when_the_system_shows_it() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(battery_in(t.path()), (true, None));
        std::fs::create_dir_all(t.path().join("battery")).unwrap();
        std::fs::write(t.path().join("battery/capacity"), "83\n").unwrap();
        std::fs::write(t.path().join("battery/status"), "Discharging\n").unwrap();
        assert_eq!(battery_in(t.path()), (true, Some(83.0)));
        std::fs::write(t.path().join("battery/status"), "Charging\n").unwrap();
        assert_eq!(battery_in(t.path()), (false, Some(83.0)));
    }

    #[cfg(unix)]
    #[test]
    fn space_of_a_real_folder_is_known() {
        let t = tempfile::tempdir().unwrap();
        let (total, free) = space(t.path()).unwrap();
        assert!(total > 0 && free <= total);
        assert!(space(Path::new("/no/such/place/at/all")).is_none());
    }
}
