//! Smart Clear: find what can probably go, and say how much space that gives back.
//!
//! [`scan`] looks through a drive (or the whole device) and lists candidates in
//! groups, from the plainly disposable (temporary files, app caches, the Trash)
//! to things only the person can judge (old downloads, big files nobody has
//! touched in months, exact copies). Nothing is deleted here: the interface
//! shows the list, the person picks, and the ordinary delete does the work.
//!
//! Where to look is described by [`Places`], which [`places_for`] fills in for
//! the system the app runs on. Tests build their own, so every rule runs
//! against a folder made for the purpose.
//!
//! Rules of caution that hold everywhere:
//!
//! * System folders are never walked, and neither are other people's files.
//! * A whole cache folder is one item, so an app's cache goes or stays as one.
//! * Inside settings-like territory (hidden folders, `AppData`, `Library`) only
//!   caches, temporary files and crash dumps are looked for, never documents.
//! * Links are not followed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DAY: i64 = 86_400;
const MB: u64 = 1024 * 1024;

/// Why something is on the list. The order is the order of caution: the first
/// kinds are safe to clear without looking; the later ones want a look first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Left behind in the system's temporary folders, or named like a temporary file.
    Temp,
    /// What apps keep to start faster and can rebuild.
    Cache,
    /// Small previews of pictures, rebuilt when needed.
    Thumbnails,
    /// Already deleted once: the Trash or Recycle Bin.
    Trash,
    /// Downloads that never finished.
    Partial,
    /// Old logs and crash dumps.
    Logs,
    /// Installers and disk images in Downloads, usually done with once run.
    Installers,
    /// `node_modules`, build output and the like in projects not touched for a long time.
    BuildLeftovers,
    /// Exact copies of another file (one copy is always kept).
    Duplicates,
    /// Downloads nobody has opened in a long time.
    OldDownloads,
    /// Big files nobody has opened in a long time.
    LargeOld,
}

impl Kind {
    /// Kinds whose items can be recreated or were already thrown away.
    pub fn safe(self) -> bool {
        matches!(self, Kind::Temp | Kind::Cache | Kind::Thumbnails | Kind::Trash | Kind::Partial)
    }
}

/// One thing that could be cleared.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub path: String,
    /// Deleted together with `path` (the Trash keeps a note beside each item).
    pub also: Vec<String>,
    pub name: String,
    /// Bytes given back; for a folder, everything in it.
    pub size: u64,
    /// When it was last changed or opened, seconds since 1970 (0 when unknown).
    pub used: i64,
    pub kind: Kind,
    pub is_dir: bool,
    /// Copies of one file share a number.
    pub group: Option<u32>,
    /// A few words on why it is listed.
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Options {
    /// "A long time": days without being changed or opened.
    pub old_days: u32,
    /// "Big": megabytes.
    pub large_mb: u64,
    /// Look for exact copies. Reads the contents of same-sized files, so it is the slow part.
    pub duplicates: bool,
    /// At most this many items are listed per kind (the largest); totals still count all.
    pub max_per_kind: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { old_days: 180, large_mb: 200, duplicates: true, max_per_kind: 2000 }
    }
}

/// Where to look. Every list may be empty.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// Temporary folders: each thing directly inside is a candidate once a day old.
    pub temp: Vec<PathBuf>,
    /// Folders of caches: each folder directly inside is one app's cache.
    pub caches: Vec<PathBuf>,
    /// Folders that are, as a whole, thumbnails.
    pub thumbnails: Vec<PathBuf>,
    /// Folders whose contents are deleted items.
    pub trash: Vec<PathBuf>,
    /// Download folders.
    pub downloads: Vec<PathBuf>,
    /// Where the person's own files are: walked for everything else.
    pub walk: Vec<PathBuf>,
    /// Never entered or listed (system folders, other drives, the app's own data).
    pub skip: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KindTotal {
    pub kind: Kind,
    /// Everything found, including what the list leaves out.
    pub count: u64,
    pub bytes: u64,
    /// How many of them are in `items`.
    pub listed: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub items: Vec<Item>,
    pub totals: Vec<KindTotal>,
    /// Files looked at.
    pub scanned: u64,
    pub cancelled: bool,
    /// The folders that were walked, for the page to name.
    pub looked_in: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub scanned: u64,
    pub found: u64,
    pub bytes: u64,
    /// What is being done, in a few words.
    pub step: String,
}

fn secs(t: std::io::Result<SystemTime>) -> i64 {
    t.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// The later of "changed" and "opened". Many systems do not keep "opened" up to date, so it only ever helps a file stay.
fn last_used(meta: &std::fs::Metadata) -> i64 {
    secs(meta.modified()).max(secs(meta.accessed()))
}

fn text(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| text(p))
}

fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// On systems with several people, only the files of whoever runs the app.
#[cfg(unix)]
fn mine(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.uid() == unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn mine(_meta: &std::fs::Metadata) -> bool {
    true
}

/// Everything in a folder: bytes, and when any of it was last used.
fn tree(dir: &Path, cancel: &AtomicBool) -> (u64, i64) {
    let (mut bytes, mut newest) = (0u64, 0i64);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(read) = std::fs::read_dir(&d) else { continue };
        for entry in read.flatten() {
            let Ok(meta) = entry.metadata() else { continue }; // does not follow links
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                bytes += meta.len();
                newest = newest.max(last_used(&meta));
            }
        }
    }
    (bytes, newest)
}

const INSTALLERS: [&str; 14] = ["exe", "msi", "msix", "dmg", "pkg", "deb", "rpm", "appimage", "iso", "apk", "xapk", "img", "flatpak", "snap"];
const PARTIAL: [&str; 5] = ["crdownload", "part", "partial", "download", "opdownload"];
const DUMPS: [&str; 4] = ["dmp", "mdmp", "hprof", "stackdump"];
/// Folder names that are a cache wherever they appear.
const CACHE_NAMES: [&str; 7] = ["Cache", "Code Cache", "GPUCache", "cache2", "CachedData", "ShaderCache", "GrShaderCache"];
/// Folders that hold settings and app data rather than the person's documents.
const PRIVATE_NAMES: [&str; 3] = ["AppData", "Library", "Application Data"];

struct Scan<'a> {
    places: &'a Places,
    opts: &'a Options,
    cancel: &'a AtomicBool,
    progress: &'a dyn Fn(Progress),
    now: i64,
    items: Vec<Item>,
    listed: HashSet<PathBuf>,
    /// Same-sized files, for finding copies: size → (path, last used).
    sizes: HashMap<u64, Vec<(PathBuf, i64)>>,
    scanned: u64,
    bytes: u64,
    last_report: std::time::Instant,
}

impl Scan<'_> {
    fn stopped(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn older_than(&self, when: i64, days: i64) -> bool {
        when > 0 && self.now - when > days * DAY
    }

    fn skipped(&self, p: &Path) -> bool {
        self.places.skip.iter().any(|s| p == s)
    }

    fn tell(&mut self, step: &str, force: bool) {
        if force || self.last_report.elapsed().as_millis() > 150 {
            self.last_report = std::time::Instant::now();
            (self.progress)(Progress { scanned: self.scanned, found: self.items.len() as u64, bytes: self.bytes, step: step.to_string() });
        }
    }

    fn add(&mut self, path: PathBuf, kind: Kind, size: u64, used: i64, is_dir: bool, note: &str) {
        if size == 0 || !self.listed.insert(path.clone()) {
            return;
        }
        self.bytes += size;
        self.items.push(Item { name: name_of(&path), path: text(&path), also: vec![], size, used, kind, is_dir, group: None, note: note.to_string() });
    }

    /// Each thing directly inside `dir` becomes one item.
    fn children(&mut self, dir: &Path, kind: Kind, min_age_days: i64, note: &str) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            if self.stopped() {
                return;
            }
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            if self.skipped(&path) || !mine(&meta) || self.places.thumbnails.contains(&path) {
                continue;
            }
            self.scanned += 1;
            let (size, used) = if meta.is_dir() {
                let (bytes, newest) = tree(&path, self.cancel);
                (bytes, newest.max(secs(meta.modified())))
            } else {
                (meta.len(), last_used(&meta))
            };
            if min_age_days > 0 && !self.older_than(used, min_age_days) {
                continue;
            }
            self.add(path, kind, size, used, meta.is_dir(), note);
            self.tell(note, false);
        }
    }

    /// The Trash: the Recycle Bin keeps a small `$I…` note beside each `$R…` item, and Linux a `.trashinfo`.
    fn trash(&mut self, dir: &Path) {
        let before = self.items.len();
        self.children(dir, Kind::Trash, 0, "In the Trash");
        let linux_info = dir.parent().filter(|_| dir.file_name().is_some_and(|n| n == "files")).map(|p| p.join("info"));
        let mut notes: HashSet<String> = HashSet::new();
        for item in &mut self.items[before..] {
            if let Some(info) = &linux_info {
                let note = info.join(format!("{}.trashinfo", item.name));
                if note.is_file() {
                    item.also.push(text(&note));
                }
            }
            if let Some(rest) = item.name.strip_prefix("$R") {
                let note = dir.join(format!("$I{rest}"));
                if note.is_file() {
                    notes.insert(text(&note));
                    item.also.push(text(&note));
                }
            }
        }
        // The notes themselves are not items, and neither is the folder's own settings file.
        let dropped: Vec<usize> = (before..self.items.len())
            .filter(|&i| notes.contains(&self.items[i].path) || self.items[i].name.eq_ignore_ascii_case("desktop.ini"))
            .collect();
        for i in dropped.into_iter().rev() {
            let gone = self.items.remove(i);
            self.bytes -= gone.size;
        }
    }

    fn in_downloads(&self, p: &Path) -> bool {
        self.places.downloads.iter().any(|d| p.starts_with(d))
    }

    /// One of the person's folders, and everything under it.
    fn walk(&mut self, root: &Path) {
        // (folder, inside settings territory)
        let mut stack: Vec<(PathBuf, bool)> = vec![(root.to_path_buf(), false)];
        let handled: Vec<&PathBuf> =
            self.places.temp.iter().chain(&self.places.caches).chain(&self.places.thumbnails).chain(&self.places.trash).collect();
        let handled: HashSet<PathBuf> = handled.into_iter().cloned().collect();
        let old = self.opts.old_days as i64;
        while let Some((dir, private)) = stack.pop() {
            if self.stopped() {
                return;
            }
            let Ok(read) = std::fs::read_dir(&dir) else { continue };
            let entries: Vec<std::fs::DirEntry> = read.flatten().collect();
            let has = |name: &str| entries.iter().any(|e| e.file_name() == name);
            let rust_project = has("Cargo.toml");
            for entry in &entries {
                let path = entry.path();
                let Ok(meta) = entry.metadata() else { continue };
                if meta.file_type().is_symlink() || self.skipped(&path) || handled.contains(&path) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                if meta.is_dir() {
                    if CACHE_NAMES.contains(&name.as_str()) {
                        let (size, used) = tree(&path, self.cancel);
                        self.add(path, Kind::Cache, size, used, true, "An app's cache");
                    } else if name == ".thumbnails" {
                        let (size, used) = tree(&path, self.cancel);
                        self.add(path, Kind::Thumbnails, size, used, true, "Picture previews");
                    } else if name == "node_modules" || name == "__pycache__" || (name == "target" && rust_project) {
                        // Never worth going into; listed when the project has been left alone.
                        if !private {
                            let (size, used) = tree(&path, self.cancel);
                            let project = secs(std::fs::metadata(&dir).and_then(|m| m.modified()));
                            if self.older_than(used.max(project), old) {
                                self.add(path, Kind::BuildLeftovers, size, used, true, "Can be rebuilt by the project's tools");
                            }
                        }
                    } else {
                        let private = private || name.starts_with('.') || name.starts_with('$') || PRIVATE_NAMES.contains(&name.as_str());
                        stack.push((path, private));
                    }
                    continue;
                }
                self.scanned += 1;
                let (size, used) = (meta.len(), last_used(&meta));
                let ext = ext_of(&name);
                if PARTIAL.contains(&ext.as_str()) && self.older_than(used, 2) {
                    self.add(path, Kind::Partial, size, used, false, "A download that never finished");
                } else if (ext == "tmp" || ext == "temp" || name.starts_with("~$") || name.starts_with(".~lock.")) && self.older_than(used, 7) {
                    self.add(path, Kind::Temp, size, used, false, "A leftover temporary file");
                } else if DUMPS.contains(&ext.as_str()) && self.older_than(used, 7) {
                    self.add(path, Kind::Logs, size, used, false, "A crash dump");
                } else if ext == "log" && size >= MB && self.older_than(used, 30) {
                    self.add(path, Kind::Logs, size, used, false, "An old log");
                } else if !private {
                    let downloads = self.in_downloads(&path);
                    if downloads && INSTALLERS.contains(&ext.as_str()) && self.older_than(used, 30) {
                        self.add(path, Kind::Installers, size, used, false, "An installer in Downloads");
                    } else if downloads && self.older_than(used, old) {
                        self.add(path, Kind::OldDownloads, size, used, false, "In Downloads, not opened for a long time");
                    } else if size >= self.opts.large_mb * MB && self.older_than(used, old) {
                        self.add(path, Kind::LargeOld, size, used, false, "Large, and not opened for a long time");
                    } else if self.opts.duplicates && size >= MB {
                        self.sizes.entry(size).or_default().push((path, used));
                    }
                }
            }
            self.tell(&format!("Looking in {}", name_of(&dir)), false);
        }
    }

    /// Among same-sized files, the ones whose contents are identical. One of each set is kept.
    fn duplicates(&mut self) {
        let mut sets: Vec<(u64, Vec<(PathBuf, i64)>)> = std::mem::take(&mut self.sizes).into_iter().filter(|(_, v)| v.len() > 1).collect();
        sets.sort_by_key(|s| std::cmp::Reverse(s.0));
        let mut group = 0u32;
        for (size, files) in sets {
            if self.stopped() {
                return;
            }
            self.tell("Comparing files of the same size", false);
            // Cheap first: the opening of each file. Only matching openings get read in full.
            let mut by_head: BTreeMap<Vec<u8>, Vec<(PathBuf, i64)>> = BTreeMap::new();
            for (path, used) in files {
                if let Some(h) = digest(&path, Some(64 * 1024), self.cancel) {
                    by_head.entry(h).or_default().push((path, used));
                }
            }
            for same_head in by_head.into_values().filter(|v| v.len() > 1) {
                let mut by_all: BTreeMap<Vec<u8>, Vec<(PathBuf, i64)>> = BTreeMap::new();
                for (path, used) in same_head {
                    if let Some(h) = digest(&path, None, self.cancel) {
                        by_all.entry(h).or_default().push((path, used));
                    }
                }
                for mut copies in by_all.into_values().filter(|v| v.len() > 1) {
                    // The copy to keep: outside Downloads if there is one, then the oldest, then the shortest path.
                    copies.sort_by(|a, b| {
                        (self.in_downloads(&a.0), a.1, a.0.as_os_str().len()).cmp(&(self.in_downloads(&b.0), b.1, b.0.as_os_str().len()))
                    });
                    let kept = text(&copies[0].0);
                    group += 1;
                    for (path, used) in copies.into_iter().skip(1) {
                        let at = self.items.len();
                        self.add(path, Kind::Duplicates, size, used, false, &format!("Same contents as {kept}"));
                        if let Some(item) = self.items.get_mut(at) {
                            item.group = Some(group);
                        }
                    }
                }
            }
        }
    }
}

/// SHA-256 of a file, or of its first `limit` bytes. `None` when it cannot be read or the scan was stopped.
fn digest(path: &Path, limit: Option<u64>, cancel: &AtomicBool) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut left = limit.unwrap_or(u64::MAX);
    while left > 0 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let want = buf.len().min(left.min(usize::MAX as u64) as usize);
        let n = file.read(&mut buf[..want]).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Some(hasher.finalize().to_vec())
}

/// Look through `places` and list what could be cleared.
pub fn scan(places: &Places, opts: &Options, cancel: &AtomicBool, progress: &dyn Fn(Progress)) -> Report {
    let mut s = Scan {
        places,
        opts,
        cancel,
        progress,
        now: secs(Ok(SystemTime::now())),
        items: Vec::new(),
        listed: HashSet::new(),
        sizes: HashMap::new(),
        scanned: 0,
        bytes: 0,
        last_report: std::time::Instant::now(),
    };
    for dir in &places.temp {
        s.children(dir, Kind::Temp, 1, "Temporary files");
    }
    for dir in &places.caches {
        s.children(dir, Kind::Cache, 0, "An app's cache");
    }
    for dir in &places.thumbnails {
        if !s.skipped(dir) {
            let (size, used) = tree(dir, cancel);
            s.add(dir.clone(), Kind::Thumbnails, size, used, true, "Picture previews");
        }
    }
    for dir in &places.trash {
        s.trash(dir);
    }
    for root in &places.walk {
        s.walk(root);
    }
    if opts.duplicates {
        s.duplicates();
    }
    s.tell("Done", true);

    // Totals count everything; the list keeps the largest of each kind.
    let mut totals: BTreeMap<Kind, KindTotal> = BTreeMap::new();
    for item in &s.items {
        let t = totals.entry(item.kind).or_insert(KindTotal { kind: item.kind, count: 0, bytes: 0, listed: 0 });
        t.count += 1;
        t.bytes += item.size;
    }
    let mut items = s.items;
    items.sort_by(|a, b| a.kind.cmp(&b.kind).then(b.size.cmp(&a.size)));
    let mut kept: HashMap<Kind, usize> = HashMap::new();
    items.retain(|i| {
        let n = kept.entry(i.kind).or_default();
        *n += 1;
        *n <= opts.max_per_kind.max(1)
    });
    for t in totals.values_mut() {
        t.listed = kept.get(&t.kind).copied().unwrap_or(0).min(opts.max_per_kind.max(1)) as u64;
    }
    Report {
        items,
        totals: totals.into_values().collect(),
        scanned: s.scanned,
        cancelled: cancel.load(Ordering::Relaxed),
        looked_in: places.walk.iter().map(|p| text(p)).collect(),
    }
}

// ---- Where to look on this system ----------------------------------------------

/// The drive a path is on: the longest mount point that leads to it.
fn drive_of<'a>(path: &Path, mounts: &'a [PathBuf]) -> Option<&'a PathBuf> {
    mounts.iter().filter(|m| path.starts_with(m)).max_by_key(|m| m.as_os_str().len())
}

/// Keep what is on `drive` (everything when `drive` is `None`), and never walk from one drive into another.
fn scoped(mut all: Places, drive: Option<&Path>, mounts: &[PathBuf]) -> Places {
    if let Some(drive) = drive {
        let on = |p: &PathBuf| drive_of(p, mounts).map(|m| m.as_path() == drive).unwrap_or(false);
        for list in [&mut all.temp, &mut all.caches, &mut all.thumbnails, &mut all.trash, &mut all.walk] {
            list.retain(on);
        }
    }
    // A drive mounted inside a walked folder is its own drive.
    for m in mounts {
        if !all.walk.contains(m) && !all.skip.contains(m) {
            all.skip.push(m.clone());
        }
    }
    all
}

/// Where to look on this computer. `drive` is a mount point from the drive list, or `None`
/// for the whole device; `mounts` are the local drives' mount points; `own` are the app's
/// own folders, which are left alone.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn places_for(drive: Option<&Path>, mounts: &[PathBuf], own: &[PathBuf]) -> Places {
    let mut p = Places { skip: own.to_vec(), ..Default::default() };
    let home = dirs::home_dir();
    let home_drive = home.as_ref().and_then(|h| drive_of(h, mounts)).cloned();
    p.temp.push(std::env::temp_dir());
    p.downloads.extend(dirs::download_dir());
    if let Some(home) = &home {
        p.walk.push(home.clone());
    }
    // Other drives are walked whole; the system drive only through the home folder.
    for m in mounts {
        // The root of a Unix system is system folders; the person's part of it is the home folder.
        if Some(m) != home_drive.as_ref() && !(cfg!(unix) && m == Path::new("/")) {
            p.walk.push(m.clone());
            p.downloads.push(m.join("Downloads"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let uid = unsafe { libc::geteuid() };
        p.temp.extend([PathBuf::from("/tmp"), PathBuf::from("/var/tmp")]);
        if let Some(cache) = dirs::cache_dir() {
            p.thumbnails.push(cache.join("thumbnails"));
            p.caches.push(cache);
        }
        if let Some(data) = dirs::data_dir() {
            p.trash.push(data.join("Trash/files"));
        }
        for m in mounts {
            p.trash.push(m.join(format!(".Trash-{uid}/files")));
        }
        for sys in ["proc", "sys", "dev", "run", "boot", "usr", "bin", "sbin", "lib", "lib64", "etc", "var", "snap", "opt", "root", "lost+found"] {
            p.skip.extend(mounts.iter().map(|m| m.join(sys)));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let uid = unsafe { libc::geteuid() };
        p.temp.push(PathBuf::from("/private/tmp"));
        p.caches.extend(dirs::cache_dir());
        if let Some(home) = &home {
            p.trash.push(home.join(".Trash"));
            // Photos, Mail and the like keep their own libraries; they are not for clearing by hand.
            p.skip.extend([home.join("Pictures/Photos Library.photoslibrary"), home.join("Library/Mail"), home.join("Library/Mobile Documents")]);
        }
        for m in mounts {
            p.trash.push(m.join(format!(".Trashes/{uid}")));
            for sys in ["System", "Library", "Applications", "private", "usr", "bin", "sbin", "cores", ".Spotlight-V100", ".fseventsd"] {
                p.skip.push(m.join(sys));
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot").map(PathBuf::from) {
            p.temp.push(root.join("Temp"));
        }
        if let Some(local) = dirs::data_local_dir() {
            p.caches.push(local.join("CrashDumps"));
            p.caches.push(local.join("Microsoft").join("Windows").join("INetCache"));
        }
        for m in mounts {
            // One folder per person in the Recycle Bin; only the signed-in person's can be read.
            if let Ok(read) = std::fs::read_dir(m.join("$Recycle.Bin")) {
                p.trash.extend(read.flatten().map(|e| e.path()).filter(|d| d.is_dir()));
            }
            for sys in ["Windows", "Program Files", "Program Files (x86)", "ProgramData", "System Volume Information", "$Recycle.Bin", "Recovery", "Users"] {
                p.skip.push(m.join(sys));
            }
        }
        // The home folder is under Users, which is skipped as a whole for other people's sake.
        if let Some(home) = &home {
            p.skip.retain(|s| !home.starts_with(s) || s == home);
        }
    }
    p.temp.sort();
    p.temp.dedup();
    scoped(p, drive, mounts)
}

/// Where to look on a phone or tablet: shared storage. Other apps' private caches are closed
/// to every app, so the system's own storage page remains the place for those.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn places_for(drive: Option<&Path>, mounts: &[PathBuf], own: &[PathBuf]) -> Places {
    let mut p = Places { skip: own.to_vec(), ..Default::default() };
    for m in mounts {
        p.walk.push(m.clone());
        p.downloads.extend([m.join("Download"), m.join("Downloads")]);
        // Closed to the app since Android 11; trying only costs time.
        p.skip.push(m.join("Android"));
    }
    scoped(p, drive, mounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_times, FileTime};
    use std::fs;

    fn aged(path: &Path, days: i64) {
        let t = FileTime::from_unix_time(secs(Ok(SystemTime::now())) - days * DAY, 0);
        set_file_times(path, t, t).unwrap();
    }

    fn file(path: &Path, bytes: usize, days: i64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![7u8; bytes]).unwrap();
        aged(path, days);
    }

    fn run(places: &Places, opts: &Options) -> Report {
        scan(places, opts, &AtomicBool::new(false), &|_| {})
    }

    fn kinds(r: &Report) -> Vec<(Kind, String)> {
        let mut v: Vec<(Kind, String)> = r.items.iter().map(|i| (i.kind, i.name.clone())).collect();
        v.sort();
        v
    }

    #[test]
    fn each_kind_of_leftover_is_found_and_ordinary_files_are_left_alone() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        // Temporary folder: old things go, today's stay.
        file(&d.path().join("tmp/old-session/a.bin"), 5000, 3);
        aged(&d.path().join("tmp/old-session"), 3);
        file(&d.path().join("tmp/just-now.bin"), 5000, 0);
        // One item per app's cache.
        file(&d.path().join("cache/browser/x/1"), 4000, 1);
        file(&d.path().join("cache/browser/x/2"), 4000, 1);
        file(&d.path().join("cache/thumbnails/large/t.png"), 3000, 1);
        // The Trash, Linux style.
        file(&d.path().join("Trash/files/gone.txt"), 2000, 9);
        file(&d.path().join("Trash/info/gone.txt.trashinfo"), 50, 9);
        // The person's files.
        file(&home.join("Documents/thesis.docx"), 9000, 400);
        file(&home.join("Documents/big-video.mkv"), 3 * MB as usize, 400);
        file(&home.join("Documents/recent-video.mkv"), 3 * MB as usize + 1, 5);
        file(&home.join("Downloads/setup.exe"), 8000, 60);
        file(&home.join("Downloads/new-setup.exe"), 8000, 2);
        file(&home.join("Downloads/manual.pdf"), 7000, 300);
        file(&home.join("Downloads/movie.mp4.crdownload"), 6000, 10);
        file(&home.join("Documents/~$draft.docx"), 160, 30);
        file(&home.join("Documents/app.log"), 2 * MB as usize, 90);
        file(&home.join("Documents/small.log"), 100, 90);
        file(&home.join(".config/app/Cache/data_0"), 5000, 1);
        file(&home.join(".config/app/settings-backup-very-old.json"), 3 * MB as usize + 2, 900);
        file(&home.join("Pictures/.thumbnails/1.jpg"), 2500, 50);

        let places = Places {
            temp: vec![d.path().join("tmp")],
            caches: vec![d.path().join("cache")],
            thumbnails: vec![d.path().join("cache/thumbnails")],
            trash: vec![d.path().join("Trash/files")],
            downloads: vec![home.join("Downloads")],
            walk: vec![home.clone()],
            skip: vec![],
        };
        let opts = Options { large_mb: 2, duplicates: false, ..Default::default() };
        let r = run(&places, &opts);
        assert_eq!(
            kinds(&r),
            vec![
                (Kind::Temp, "old-session".to_string()),
                (Kind::Temp, "~$draft.docx".to_string()),
                (Kind::Cache, "Cache".to_string()),
                (Kind::Cache, "browser".to_string()),
                (Kind::Thumbnails, ".thumbnails".to_string()),
                (Kind::Thumbnails, "thumbnails".to_string()),
                (Kind::Trash, "gone.txt".to_string()),
                (Kind::Partial, "movie.mp4.crdownload".to_string()),
                (Kind::Logs, "app.log".to_string()),
                (Kind::Installers, "setup.exe".to_string()),
                (Kind::OldDownloads, "manual.pdf".to_string()),
                (Kind::LargeOld, "big-video.mkv".to_string()),
            ]
        );
        // A cache folder counts everything in it, and the Trash takes its note along.
        let browser = r.items.iter().find(|i| i.name == "browser").unwrap();
        assert_eq!((browser.size, browser.is_dir), (8000, true));
        let gone = r.items.iter().find(|i| i.name == "gone.txt").unwrap();
        assert_eq!(gone.also.len(), 1);
        assert!(gone.also[0].ends_with("gone.txt.trashinfo"));
        // The totals add up to what the items say.
        let sum: u64 = r.items.iter().map(|i| i.size).sum();
        assert_eq!(r.totals.iter().map(|t| t.bytes).sum::<u64>(), sum);
        assert!(!r.cancelled);
    }

    #[test]
    fn exact_copies_are_found_and_one_is_always_kept() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().to_path_buf();
        let body: Vec<u8> = (0..(MB as usize + 10)).map(|i| (i % 251) as u8).collect();
        for (name, days) in [("Photos/trip.raw", 100), ("Downloads/trip (1).raw", 20), ("Backup/trip.raw", 50)] {
            let p = home.join(name);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, &body).unwrap();
            aged(&p, days);
        }
        // Same size, different contents at the very end: not a copy.
        let mut other = body.clone();
        *other.last_mut().unwrap() ^= 1;
        fs::write(home.join("Photos/other.raw"), &other).unwrap();

        let places = Places { downloads: vec![home.join("Downloads")], walk: vec![home.clone()], ..Default::default() };
        let r = run(&places, &Options::default());
        let mut copies: Vec<&Item> = r.items.iter().filter(|i| i.kind == Kind::Duplicates).collect();
        copies.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(copies.len(), 2);
        // The oldest copy outside Downloads stays.
        assert!(copies.iter().all(|c| Path::new(&c.path) != home.join("Photos").join("trip.raw") && !c.path.contains("other.raw")));
        assert!(copies.iter().all(|c| c.note.ends_with("trip.raw") && c.group == copies[0].group));
        assert!(!r.items.iter().any(|i| i.path.contains("other.raw")));
    }

    #[test]
    fn projects_left_alone_give_up_their_build_folders() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().to_path_buf();
        file(&home.join("old-site/node_modules/lib/index.js"), 9000, 400);
        file(&home.join("old-site/index.html"), 100, 400);
        aged(&home.join("old-site"), 400);
        file(&home.join("active-site/node_modules/lib/index.js"), 9000, 400);
        file(&home.join("active-site/index.html"), 100, 1);
        file(&home.join("crate/target/debug/app"), 9000, 400);
        file(&home.join("crate/Cargo.toml"), 10, 400);
        aged(&home.join("crate"), 400);
        // A folder that merely happens to be called "target".
        file(&home.join("archery/target/photo.jpg"), 9000, 400);
        aged(&home.join("archery"), 400);

        let places = Places { walk: vec![home.clone()], ..Default::default() };
        let r = run(&places, &Options { duplicates: false, ..Default::default() });
        let mut found: Vec<String> = r.items.iter().filter(|i| i.kind == Kind::BuildLeftovers).map(|i| i.path.clone()).collect();
        found.sort();
        assert_eq!(found, vec![text(&home.join("crate").join("target")), text(&home.join("old-site").join("node_modules"))]);
    }

    #[test]
    fn skipped_folders_and_links_are_never_entered() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        file(&home.join("Downloads/a.iso"), 5000, 90);
        file(&home.join("system/Downloads/b.iso"), 5000, 90);
        file(&d.path().join("elsewhere/c.tmp"), 5000, 90);
        #[cfg(unix)]
        std::os::unix::fs::symlink(d.path().join("elsewhere"), home.join("link")).unwrap();

        let places = Places {
            downloads: vec![home.join("Downloads"), home.join("system/Downloads")],
            walk: vec![home.clone()],
            skip: vec![home.join("system")],
            ..Default::default()
        };
        let r = run(&places, &Options::default());
        assert_eq!(kinds(&r), vec![(Kind::Installers, "a.iso".to_string())]);
    }

    #[test]
    fn long_lists_keep_the_largest_and_still_count_everything() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..10 {
            file(&d.path().join(format!("Downloads/old-{i}.pdf")), 1000 + i * 100, 400);
        }
        let places = Places { downloads: vec![d.path().join("Downloads")], walk: vec![d.path().to_path_buf()], ..Default::default() };
        let r = run(&places, &Options { max_per_kind: 3, duplicates: false, ..Default::default() });
        assert_eq!(r.items.iter().map(|i| i.size).collect::<Vec<_>>(), vec![1900, 1800, 1700]);
        let t = &r.totals[0];
        assert_eq!((t.kind, t.count, t.listed, t.bytes), (Kind::OldDownloads, 10, 3, 14_500));
    }

    #[test]
    fn stopping_ends_the_scan_early() {
        let d = tempfile::tempdir().unwrap();
        file(&d.path().join("Downloads/a.iso"), 5000, 90);
        let places = Places { walk: vec![d.path().to_path_buf()], ..Default::default() };
        let r = scan(&places, &Options::default(), &AtomicBool::new(true), &|_| {});
        assert!(r.cancelled && r.items.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn one_drive_only_looks_at_that_drive() {
        let mounts = vec![PathBuf::from("/"), PathBuf::from("/mnt/data")];
        let all = Places {
            temp: vec![PathBuf::from("/tmp")],
            caches: vec![PathBuf::from("/home/me/.cache")],
            trash: vec![PathBuf::from("/home/me/.local/share/Trash/files"), PathBuf::from("/mnt/data/.Trash-1000/files")],
            walk: vec![PathBuf::from("/home/me"), PathBuf::from("/mnt/data")],
            ..Default::default()
        };
        let data = scoped(all.clone(), Some(Path::new("/mnt/data")), &mounts);
        assert_eq!(data.walk, vec![PathBuf::from("/mnt/data")]);
        assert_eq!(data.trash, vec![PathBuf::from("/mnt/data/.Trash-1000/files")]);
        assert!(data.temp.is_empty() && data.caches.is_empty());
        let system = scoped(all.clone(), Some(Path::new("/")), &mounts);
        assert_eq!(system.walk, vec![PathBuf::from("/home/me")]);
        assert_eq!(system.temp, vec![PathBuf::from("/tmp")]);
        // Walking the system drive never wanders into the other one.
        assert!(system.skip.contains(&PathBuf::from("/mnt/data")));
        assert_eq!(scoped(all, None, &mounts).walk.len(), 2);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn this_machine_gets_sensible_places() {
        let Some(home) = dirs::home_dir() else { return };
        let mounts = vec![PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" })];
        let own = vec![home.join("app-data")];
        let p = places_for(None, &mounts, &own);
        assert!(p.walk.contains(&home));
        // The system drive is never walked from its root, and the app's own data is left alone.
        assert!(!p.walk.contains(&mounts[0]) || home == mounts[0]);
        assert!(p.skip.contains(&own[0]));
        assert!(!p.temp.is_empty());
    }
}
