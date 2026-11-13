//! Disk usage scanning for the WinDirStat-style view, and the compact
//! snapshot that lets the view appear instantly next time.
//!
//! Unlike the search index, this scanner never keeps a record per file. It
//! keeps one record per *folder* plus a few of the largest files in each, which
//! is all a treemap can draw. A drive with millions of files therefore needs
//! memory proportional to its folder count, not its file count.
//!
//! The finished result is pruned to what the treemap draws and written as a
//! small binary snapshot (names and sizes only; paths are rebuilt from the
//! root on load), so reopening the view shows the last result at once while a
//! fresh scan runs.

use crate::index::default_skip_list;
use crate::treemap::{Analysis, LargeFile, SizeNode, TreeOptions, TypeStat};
use crate::{to_millis, CoreError, Result};
use parking_lot::Mutex;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Largest files remembered per folder; the rest of its files become "(other)".
const BIG_PER_DIR: usize = 32;

/// Live counters a caller can read while a scan runs.
#[derive(Default)]
pub struct ScanProgress {
    /// Files and folders seen so far.
    pub entries: AtomicU64,
    /// Total size of the files seen so far.
    pub bytes: AtomicU64,
}

/// Called now and then during a scan with what has been measured so far.
pub type PartialFn<'a> = &'a (dyn Fn(Analysis) + Sync);

pub struct UsageOptions<'a> {
    /// Receives partial results (`complete == false`): after a few seconds,
    /// then at growing intervals, so the picture settles slowly.
    pub partial: Option<PartialFn<'a>>,
    /// 0 = one per CPU.
    pub threads: usize,
    pub cancel: Option<&'a AtomicBool>,
    pub progress: Option<&'a ScanProgress>,
    pub skip: Vec<PathBuf>,
    pub tree: TreeOptions,
}

impl Default for UsageOptions<'_> {
    fn default() -> Self {
        UsageOptions {
            partial: None,
            threads: 0,
            cancel: None,
            progress: None,
            skip: default_skip_list(),
            tree: TreeOptions { max_depth: 6, ..TreeOptions::default() },
        }
    }
}

struct BigFile {
    name: String,
    size: u64,
}

struct DirData {
    name: String,
    /// Total size of the files directly in this folder.
    files_bytes: u64,
    file_count: u64,
    /// The largest files directly in this folder, in no particular order.
    big: Vec<BigFile>,
    /// Files directly in this folder by type: (type id, bytes, files).
    types: Vec<(u16, u64, u64)>,
    dirs: Vec<DirData>,
}

/// A folder as it is being scanned. Folders register with their parent before
/// their contents are read, so a snapshot taken mid-scan sees everything
/// found so far.
struct LiveDir {
    name: String,
    files_bytes: AtomicU64,
    file_count: AtomicU64,
    big: Mutex<Vec<BigFile>>,
    types: Mutex<Vec<(u16, u64, u64)>>,
    dirs: Mutex<Vec<std::sync::Arc<LiveDir>>>,
}

impl LiveDir {
    fn new(name: String) -> Self {
        LiveDir { name, files_bytes: AtomicU64::new(0), file_count: AtomicU64::new(0), big: Mutex::new(Vec::new()), types: Mutex::new(Vec::new()), dirs: Mutex::new(Vec::new()) }
    }

    /// A plain copy of what is known now.
    fn freeze(&self) -> DirData {
        DirData {
            name: self.name.clone(),
            files_bytes: self.files_bytes.load(Ordering::Relaxed),
            file_count: self.file_count.load(Ordering::Relaxed),
            big: self.big.lock().iter().map(|b| BigFile { name: b.name.clone(), size: b.size }).collect(),
            types: self.types.lock().clone(),
            dirs: self.dirs.lock().iter().map(|d| d.freeze()).collect(),
        }
    }
}

/// File type names, shared by all threads. Id 0 is "no extension", 1 is the
/// overflow bucket once there are too many distinct types.
struct ExtTable {
    map: Mutex<HashMap<String, u16>>,
    names: Mutex<Vec<String>>,
}

const MAX_TYPES: usize = 4000;

impl ExtTable {
    fn new() -> Self {
        ExtTable { map: Mutex::new(HashMap::new()), names: Mutex::new(vec!["(no extension)".into(), "(other)".into()]) }
    }

    fn id(&self, ext: &str) -> u16 {
        if ext.is_empty() {
            return 0;
        }
        let mut map = self.map.lock();
        if let Some(&id) = map.get(ext) {
            return id;
        }
        let mut names = self.names.lock();
        if names.len() >= MAX_TYPES {
            return 1;
        }
        let id = names.len() as u16;
        names.push(ext.to_string());
        map.insert(ext.to_string(), id);
        id
    }
}

/// The file type of a name: lower-case text after the last dot ("" if none).
fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() && name.len() - i <= 12 => name[i + 1..].to_ascii_lowercase(),
        _ => String::new(),
    }
}

struct Ctx<'a> {
    exts: ExtTable,
    opts: &'a UsageOptions<'a>,
    /// Smallest size currently in `largest`, so most files skip the lock.
    threshold: AtomicU64,
    largest: Mutex<BinaryHeap<Reverse<(u64, PathBuf, i64)>>>,
    largest_cap: usize,
}

impl Ctx<'_> {
    fn cancelled(&self) -> bool {
        self.opts.cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false)
    }

    fn consider_largest(&self, size: u64, path: impl FnOnce() -> PathBuf, mtime: i64) {
        if size < self.threshold.load(Ordering::Relaxed) {
            return;
        }
        let mut heap = self.largest.lock();
        heap.push(Reverse((size, path(), mtime)));
        if heap.len() > self.largest_cap {
            heap.pop();
        }
        if heap.len() == self.largest_cap {
            if let Some(Reverse((min, _, _))) = heap.peek() {
                self.threshold.store(*min, Ordering::Relaxed);
            }
        }
    }
}

fn scan_dir(path: &Path, node: &std::sync::Arc<LiveDir>, ctx: &Ctx) {
    if ctx.cancelled() {
        return;
    }
    let Ok(rd) = fs::read_dir(path) else { return };
    let mut subdirs: Vec<(PathBuf, String)> = Vec::new();
    let (mut seen, mut bytes) = (0u64, 0u64);
    let (mut files_bytes, mut file_count) = (0u64, 0u64);
    let mut big: Vec<BigFile> = Vec::new();
    let mut smallest_big = 0u64;
    let mut local_types: HashMap<String, (u64, u64)> = HashMap::new();
    for e in rd.filter_map(|e| e.ok()) {
        let Ok(ft) = e.file_type() else { continue };
        seen += 1;
        // Links are neither followed nor counted: that avoids cycles and
        // counting the same data twice.
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            let child = e.path();
            if !ctx.opts.skip.iter().any(|s| s == &child) {
                subdirs.push((child, e.file_name().to_string_lossy().into_owned()));
            }
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let size = meta.len();
        bytes += size;
        files_bytes += size;
        file_count += 1;
        {
            let t = local_types.entry(ext_of(&e.file_name().to_string_lossy())).or_insert((0, 0));
            t.0 += size;
            t.1 += 1;
        }
        if size > 0 && (big.len() < BIG_PER_DIR || size > smallest_big) {
            let name = e.file_name().to_string_lossy().into_owned();
            if big.len() >= BIG_PER_DIR {
                let i = big.iter().enumerate().min_by_key(|(_, b)| b.size).map(|(i, _)| i).unwrap_or(0);
                big.swap_remove(i);
            }
            big.push(BigFile { name, size });
            smallest_big = if big.len() >= BIG_PER_DIR { big.iter().map(|b| b.size).min().unwrap_or(0) } else { 0 };
        }
        ctx.consider_largest(size, || e.path(), to_millis(meta.modified()).map(|ms| ms / 1000).unwrap_or(0));
    }
    node.files_bytes.store(files_bytes, Ordering::Relaxed);
    node.file_count.store(file_count, Ordering::Relaxed);
    *node.big.lock() = big;
    {
        let mut merged: HashMap<u16, (u64, u64)> = HashMap::new();
        for (ext, (b, n)) in local_types {
            let t = merged.entry(ctx.exts.id(&ext)).or_insert((0, 0));
            t.0 += b;
            t.1 += n;
        }
        *node.types.lock() = merged.into_iter().map(|(id, (b, n))| (id, b, n)).collect();
    }
    if let Some(p) = ctx.opts.progress {
        p.entries.fetch_add(seen, Ordering::Relaxed);
        p.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    let kids: Vec<std::sync::Arc<LiveDir>> = subdirs.iter().map(|(_, n)| std::sync::Arc::new(LiveDir::new(n.clone()))).collect();
    *node.dirs.lock() = kids.clone();
    subdirs.par_iter().zip(kids.par_iter()).for_each(|((p, _), k)| scan_dir(p, k, ctx));
}

/// File types under a folder: type id to (bytes, files).
fn type_totals(d: &DirData, out: &mut HashMap<u16, (u64, u64)>) {
    for &(id, b, n) in &d.types {
        let t = out.entry(id).or_insert((0, 0));
        t.0 += b;
        t.1 += n;
    }
    d.dirs.iter().for_each(|c| type_totals(c, out));
}

/// Largest file types kept on each folder of the tree.
const TYPES_PER_NODE: usize = 24;

/// Total bytes and file count under a folder.
fn totals(d: &DirData) -> (u64, u64) {
    d.dirs.iter().map(totals).fold((d.files_bytes, d.file_count), |a, b| (a.0 + b.0, a.1 + b.1))
}

fn build(d: &DirData, path: &Path, display_name: String, opts: &TreeOptions, min_size: u64, depth: u32) -> SizeNode {
    let (size, _) = totals(d);
    let mut node = SizeNode {
        name: display_name,
        path: path.to_string_lossy().into_owned(),
        size,
        is_dir: true,
        children: Vec::new(),
        types: {
            let mut all = HashMap::new();
            type_totals(d, &mut all);
            let mut v: Vec<(u16, u64)> = all.into_iter().map(|(id, (b, _))| (id, b)).collect();
            v.sort_unstable_by_key(|t| Reverse(t.1));
            v.truncate(TYPES_PER_NODE);
            v
        },
    };
    if depth >= opts.max_depth {
        return node;
    }
    // Candidates: sub-folders (sized in full) and this folder's big files.
    let mut cands: Vec<(u64, Option<&DirData>, Option<&BigFile>)> = d
        .dirs
        .iter()
        .map(|c| (totals(c).0, Some(c), None))
        .chain(d.big.iter().map(|b| (b.size, None, Some(b))))
        .collect();
    cands.sort_unstable_by_key(|c| Reverse(c.0));
    let mut kept = 0u64;
    for (i, (s, dir, file)) in cands.into_iter().enumerate() {
        if i >= opts.max_children || s < min_size || s == 0 {
            break;
        }
        kept += s;
        let child_path = path.join(dir.map(|c| c.name.as_str()).or(file.map(|f| f.name.as_str())).unwrap_or_default());
        node.children.push(match (dir, file) {
            (Some(c), _) => build(c, &child_path, c.name.clone(), opts, min_size, depth + 1),
            (_, Some(f)) => SizeNode {
                name: f.name.clone(),
                path: child_path.to_string_lossy().into_owned(),
                size: f.size,
                is_dir: false,
                children: Vec::new(),
                types: Vec::new(),
            },
            _ => continue,
        });
    }
    if size > kept && !node.children.is_empty() {
        node.children.push(SizeNode {
            name: "(other)".into(),
            path: String::new(),
            size: size - kept,
            is_dir: false,
            children: Vec::new(),
            types: Vec::new(),
        });
    }
    node
}

/// Turn scan data into what the treemap draws.
fn analysis_from(data: &DirData, root: &Path, ctx: &Ctx, complete: bool) -> Analysis {
    let (total, files) = totals(data);
    let min_size = (total as f64 * ctx.opts.tree.min_fraction) as u64;
    let tree = build(data, root, root.to_string_lossy().into_owned(), &ctx.opts.tree, min_size, 0);
    let mut largest: Vec<(u64, PathBuf, i64)> = ctx.largest.lock().iter().map(|Reverse(t)| t.clone()).collect();
    largest.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut all = HashMap::new();
    type_totals(data, &mut all);
    let mut types: Vec<TypeStat> = all.into_iter().map(|(ext, (bytes, files))| TypeStat { ext, bytes, files }).collect();
    types.sort_unstable_by_key(|t| Reverse(t.bytes));
    types.truncate(300);
    Analysis {
        ext_names: ctx.exts.names.lock().clone(),
        types,
        tree,
        largest: largest
            .into_iter()
            .map(|(size, path, modified)| LargeFile {
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
                size,
                modified: modified * 1000,
            })
            .collect(),
        total_bytes: total,
        file_count: files,
        complete,
    }
}

/// First partial result after this long, then at growing gaps up to the cap.
const PARTIAL_FIRST_SECS: f64 = 4.0;
const PARTIAL_GROWTH: f64 = 1.6;
const PARTIAL_MAX_SECS: f64 = 30.0;

/// Measure everything under `root`. Returns `None` if cancelled or the root
/// cannot be read at all.
pub fn scan(root: &Path, opts: &UsageOptions) -> Option<Analysis> {
    fs::read_dir(root).ok()?;
    let ctx = Ctx {
        opts,
        exts: ExtTable::new(),
        threshold: AtomicU64::new(0),
        largest: Mutex::new(BinaryHeap::new()),
        largest_cap: opts.tree.largest_files.max(1),
    };
    let threads = if opts.threads == 0 {
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
    } else {
        opts.threads
    };
    let live = std::sync::Arc::new(LiveDir::new(root.to_string_lossy().into_owned()));
    let done = AtomicBool::new(false);
    std::thread::scope(|scope| {
        if let Some(partial) = opts.partial {
            let (live, ctx, done) = (&live, &ctx, &done);
            scope.spawn(move || {
                let mut gap = PARTIAL_FIRST_SECS;
                let mut next = std::time::Instant::now() + std::time::Duration::from_secs_f64(gap);
                while !done.load(Ordering::Relaxed) && !ctx.cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if std::time::Instant::now() >= next {
                        partial(analysis_from(&live.freeze(), root, ctx, false));
                        gap = (gap * PARTIAL_GROWTH).min(PARTIAL_MAX_SECS);
                        next = std::time::Instant::now() + std::time::Duration::from_secs_f64(gap);
                    }
                }
            });
        }
        let run = || scan_dir(root, &live, &ctx);
        match rayon::ThreadPoolBuilder::new().num_threads(threads).build() {
            Ok(pool) => pool.install(run),
            Err(_) => run(),
        }
        done.store(true, Ordering::Relaxed);
    });
    if ctx.cancelled() {
        return None;
    }
    Some(analysis_from(&live.freeze(), root, &ctx, true))
}

// ---------------------------------------------------------------------------
// Snapshots
// ---------------------------------------------------------------------------

const SNAPSHOT_VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct SnapNode {
    name: String,
    size: u64,
    is_dir: bool,
    types: Vec<(u16, u64)>,
    children: Vec<SnapNode>,
}

#[derive(Serialize, Deserialize)]
struct SnapLarge {
    /// Path below the scan root.
    rel: String,
    size: u64,
    modified: i64,
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    version: u32,
    root: String,
    taken_at: i64,
    total_bytes: u64,
    file_count: u64,
    /// False for a checkpoint written while a scan was still running.
    complete: bool,
    ext_names: Vec<String>,
    types: Vec<(u16, u64, u64)>,
    tree: SnapNode,
    largest: Vec<SnapLarge>,
}

fn compact(n: &SizeNode, is_root: bool) -> SnapNode {
    SnapNode {
        // Paths are rebuilt from the root, so only the last segment is kept.
        name: if is_root { String::new() } else { n.name.clone() },
        size: n.size,
        is_dir: n.is_dir,
        types: n.types.clone(),
        children: n.children.iter().map(|c| compact(c, false)).collect(),
    }
}

fn expand(n: &SnapNode, path: &Path, display: String) -> SizeNode {
    SizeNode {
        name: display,
        // The "(other)" bucket has no path: it is not a real item.
        path: if n.name == "(other)" { String::new() } else { path.to_string_lossy().into_owned() },
        size: n.size,
        is_dir: n.is_dir,
        types: n.types.clone(),
        children: n.children.iter().map(|c| expand(c, &path.join(&c.name), c.name.clone())).collect(),
    }
}

/// Stable file name for a root: FNV-1a of its lower-cased path.
fn key(root: &Path) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.to_string_lossy().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}.bin")
}

pub fn snapshot_file(dir: &Path, root: &Path) -> PathBuf {
    dir.join(key(root))
}

/// Save a finished analysis of `root`. Overwrites the previous snapshot.
pub fn save_snapshot(dir: &Path, root: &Path, a: &Analysis, taken_at: i64) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let snap = Snapshot {
        version: SNAPSHOT_VERSION,
        root: root.to_string_lossy().into_owned(),
        taken_at,
        total_bytes: a.total_bytes,
        file_count: a.file_count,
        complete: a.complete,
        ext_names: a.ext_names.clone(),
        types: a.types.iter().map(|t| (t.ext, t.bytes, t.files)).collect(),
        tree: compact(&a.tree, true),
        largest: a
            .largest
            .iter()
            .map(|l| SnapLarge {
                rel: Path::new(&l.path).strip_prefix(root).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| l.path.clone()),
                size: l.size,
                modified: l.modified,
            })
            .collect(),
    };
    let bytes = bincode::serialize(&snap).map_err(|e| CoreError::Invalid(e.to_string()))?;
    let file = snapshot_file(dir, root);
    let tmp = file.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| CoreError::io(&tmp, e))?;
    fs::rename(&tmp, &file).map_err(|e| CoreError::io(&file, e))
}

/// Load the last snapshot of `root` and when it was taken (Unix seconds).
/// A missing, damaged or outdated file simply yields `None`.
pub fn load_snapshot(dir: &Path, root: &Path) -> Option<(Analysis, i64)> {
    let bytes = fs::read(snapshot_file(dir, root)).ok()?;
    let snap: Snapshot = bincode::deserialize(&bytes).ok()?;
    if snap.version != SNAPSHOT_VERSION || Path::new(&snap.root) != root {
        return None;
    }
    let tree = expand(&snap.tree, root, root.to_string_lossy().into_owned());
    let largest = snap
        .largest
        .into_iter()
        .map(|l| {
            let path = root.join(&l.rel);
            LargeFile {
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                path: path.to_string_lossy().into_owned(),
                size: l.size,
                modified: l.modified,
            }
        })
        .collect();
    Some((
        Analysis {
            tree,
            largest,
            total_bytes: snap.total_bytes,
            file_count: snap.file_count,
            complete: snap.complete,
            ext_names: snap.ext_names,
            types: snap.types.into_iter().map(|(ext, bytes, files)| TypeStat { ext, bytes, files }).collect(),
        },
        snap.taken_at,
    ))
}

/// The root a snapshot file was taken of, read from its header alone (the
/// version number, then the root as a length-prefixed string).
fn snapshot_root(file: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = fs::File::open(file).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    let len = u64::from_le_bytes(head[4..12].try_into().ok()?) as usize;
    if len > 4096 {
        return None;
    }
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// Delete the saved analyses of `root` and of every folder or drive below it.
/// Returns how many files and bytes went.
pub fn remove_snapshots_under(dir: &Path, root: &Path) -> (usize, u64) {
    let rule = root.to_string_lossy().into_owned();
    let Ok(rd) = fs::read_dir(dir) else { return (0, 0) };
    let (mut files, mut bytes) = (0, 0);
    for e in rd.filter_map(|e| e.ok()) {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "bin") {
            continue;
        }
        let Some(r) = snapshot_root(&path) else { continue };
        if crate::cachepolicy::is_under(Path::new(&r), &rule) {
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            if fs::remove_file(&path).is_ok() {
                files += 1;
                bytes += len;
            }
        }
    }
    (files, bytes)
}

/// Keep the snapshot folder small: drop the oldest files beyond `max_files`
/// or `max_bytes` in total.
pub fn prune_snapshots(dir: &Path, max_files: usize, max_bytes: u64) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "bin"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort_by_key(|f| Reverse(f.0)); // newest first
    let (mut count, mut bytes) = (0usize, 0u64);
    for (_, len, path) in files {
        count += 1;
        bytes += len;
        if count > max_files || bytes > max_bytes {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// root/ photos/{a.jpg 9000, b.jpg 4000, raw/c.raw 20000}  docs/{d.txt 100}  n.txt 10
    fn sample() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::create_dir_all(p.join("photos/raw")).unwrap();
        fs::create_dir_all(p.join("docs")).unwrap();
        fs::create_dir_all(p.join("empty")).unwrap();
        for (f, n) in [("photos/a.jpg", 9000), ("photos/b.jpg", 4000), ("photos/raw/c.raw", 20000), ("docs/d.txt", 100), ("n.txt", 10)] {
            fs::write(p.join(f), vec![7u8; n]).unwrap();
        }
        d
    }

    fn full() -> UsageOptions<'static> {
        UsageOptions { tree: TreeOptions { min_fraction: 0.0, ..TreeOptions::default() }, skip: vec![], ..Default::default() }
    }

    #[test]
    fn measures_folders_and_files() {
        let d = sample();
        let a = scan(d.path(), &full()).unwrap();
        assert_eq!(a.total_bytes, 33110);
        assert_eq!(a.file_count, 5);
        assert_eq!(a.tree.size, 33110);
        let names: Vec<&str> = a.tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["photos", "docs", "n.txt"]);
        assert_eq!(a.tree.children[0].size, 33000);
        assert_eq!(a.largest[0].name, "c.raw");
        assert_eq!(a.largest.len(), 5);
        assert!(a.largest.windows(2).all(|w| w[0].size >= w[1].size));
        assert!(a.tree.children[0].path.ends_with("photos"));
    }

    #[test]
    fn every_folder_equals_the_sum_of_its_children() {
        let d = sample();
        let a = scan(d.path(), &full()).unwrap();
        fn check(n: &SizeNode) {
            if !n.children.is_empty() {
                assert_eq!(n.size, n.children.iter().map(|c| c.size).sum::<u64>(), "{}", n.name);
                n.children.iter().for_each(check);
            }
        }
        check(&a.tree);
    }

    #[test]
    fn small_files_and_excess_children_fold_into_other() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..5 {
            fs::write(d.path().join(format!("f{i}")), vec![0u8; 100 * (i + 1)]).unwrap();
        }
        let opts = UsageOptions {
            tree: TreeOptions { max_children: 2, min_fraction: 0.0, max_depth: 3, largest_files: 3 },
            skip: vec![],
            ..Default::default()
        };
        let a = scan(d.path(), &opts).unwrap();
        assert_eq!(a.tree.children.len(), 3);
        assert_eq!(a.tree.children[2].name, "(other)");
        assert_eq!(a.tree.children[2].size, 1500 - 500 - 400);
        assert_eq!(a.tree.children[2].path, "");
        assert_eq!(a.largest.len(), 3);
        assert_eq!(a.total_bytes, 1500);
    }

    #[test]
    fn many_files_in_one_folder_are_still_totalled() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..200u32 {
            fs::write(d.path().join(format!("f{i:03}")), vec![0u8; 10 + i as usize]).unwrap();
        }
        let a = scan(d.path(), &full()).unwrap();
        assert_eq!(a.file_count, 200);
        assert_eq!(a.total_bytes, (0..200u64).map(|i| 10 + i).sum::<u64>());
        // Only BIG_PER_DIR files are kept as tiles; the rest is one bucket.
        assert_eq!(a.tree.children.len(), BIG_PER_DIR + 1);
        assert_eq!(a.tree.children.iter().map(|c| c.size).sum::<u64>(), a.total_bytes);
        assert_eq!(a.largest[0].size, 10 + 199);
    }

    #[test]
    fn counts_progress_and_honours_cancel() {
        let d = sample();
        let progress = ScanProgress::default();
        let a = scan(d.path(), &UsageOptions { progress: Some(&progress), ..full() }).unwrap();
        assert_eq!(progress.bytes.load(Ordering::Relaxed), a.total_bytes);
        // 5 files + 4 folders (photos, raw, docs, empty).
        assert_eq!(progress.entries.load(Ordering::Relaxed), 9);

        let cancel = AtomicBool::new(true);
        assert!(scan(d.path(), &UsageOptions { cancel: Some(&cancel), ..full() }).is_none());
        assert!(scan(&d.path().join("missing"), &full()).is_none());
    }

    #[test]
    fn skipped_folders_are_left_out() {
        let d = sample();
        let skip = vec![d.path().join("photos")];
        let a = scan(d.path(), &UsageOptions { skip, ..full() }).unwrap();
        assert_eq!(a.total_bytes, 110);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed_or_counted() {
        let d = sample();
        std::os::unix::fs::symlink(d.path().join("photos"), d.path().join("loop")).unwrap();
        std::os::unix::fs::symlink(d.path(), d.path().join("photos/up")).unwrap();
        let a = scan(d.path(), &full()).unwrap();
        assert_eq!(a.total_bytes, 33110);
    }

    #[test]
    fn snapshot_round_trips_and_is_small() {
        let d = sample();
        let store = tempfile::tempdir().unwrap();
        let a = scan(d.path(), &full()).unwrap();
        save_snapshot(store.path(), d.path(), &a, 1_700_000_000).unwrap();
        let (b, at) = load_snapshot(store.path(), d.path()).unwrap();
        assert_eq!(at, 1_700_000_000);
        assert_eq!(b.total_bytes, a.total_bytes);
        assert_eq!(b.file_count, a.file_count);
        fn flatten(n: &SizeNode, out: &mut Vec<(String, String, u64, bool)>) {
            out.push((n.name.clone(), n.path.clone(), n.size, n.is_dir));
            n.children.iter().for_each(|c| flatten(c, out));
        }
        let (mut x, mut y) = (vec![], vec![]);
        flatten(&a.tree, &mut x);
        flatten(&b.tree, &mut y);
        assert_eq!(x, y, "paths, sizes and kinds survive the round trip");
        let paths = |l: &[LargeFile]| l.iter().map(|f| (f.path.clone(), f.size, f.modified)).collect::<Vec<_>>();
        assert_eq!(paths(&a.largest), paths(&b.largest));
        let len = fs::metadata(snapshot_file(store.path(), d.path())).unwrap().len();
        assert!(len < 2048, "snapshot of a tiny tree is {len} bytes");
    }

    #[test]
    fn file_types_are_counted_per_folder_and_overall() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("sub")).unwrap();
        fs::write(d.path().join("a.TXT"), vec![0u8; 100]).unwrap();
        fs::write(d.path().join("b.txt"), vec![0u8; 50]).unwrap();
        fs::write(d.path().join("noext"), vec![0u8; 10]).unwrap();
        fs::write(d.path().join("sub/c.png"), vec![0u8; 400]).unwrap();
        let a = scan(d.path(), &full()).unwrap();
        let name = |id: u16| a.ext_names[id as usize].clone();
        let overall: Vec<(String, u64, u64)> = a.types.iter().map(|t| (name(t.ext), t.bytes, t.files)).collect();
        assert_eq!(overall[0], ("png".to_string(), 400, 1));
        assert!(overall.contains(&("txt".to_string(), 150, 2)), "{overall:?}");
        assert!(overall.contains(&("(no extension)".to_string(), 10, 1)));
        let sub = a.tree.children.iter().find(|c| c.name == "sub").unwrap();
        assert_eq!(sub.types.iter().map(|t| (name(t.0), t.1)).collect::<Vec<_>>(), vec![("png".to_string(), 400)]);
    }

    #[test]
    fn partial_checkpoints_keep_their_flag() {
        let d = sample();
        let store = tempfile::tempdir().unwrap();
        let mut a = scan(d.path(), &full()).unwrap();
        assert!(a.complete);
        a.complete = false;
        save_snapshot(store.path(), d.path(), &a, 5).unwrap();
        let (b, _) = load_snapshot(store.path(), d.path()).unwrap();
        assert!(!b.complete, "a checkpoint must not pass for a finished scan");
    }

    #[test]
    fn a_live_tree_can_be_read_mid_scan() {
        let live = std::sync::Arc::new(LiveDir::new("root".into()));
        live.files_bytes.store(100, Ordering::Relaxed);
        live.file_count.store(2, Ordering::Relaxed);
        let child = std::sync::Arc::new(LiveDir::new("sub".into()));
        child.files_bytes.store(50, Ordering::Relaxed);
        child.file_count.store(1, Ordering::Relaxed);
        live.dirs.lock().push(child);
        // A folder that is registered but not read yet counts as empty, not as an error.
        live.dirs.lock().push(std::sync::Arc::new(LiveDir::new("pending".into())));
        let (bytes, files) = totals(&live.freeze());
        assert_eq!((bytes, files), (150, 3));
    }

    #[test]
    fn bad_or_foreign_snapshots_are_ignored() {
        let d = sample();
        let store = tempfile::tempdir().unwrap();
        assert!(load_snapshot(store.path(), d.path()).is_none());
        fs::write(snapshot_file(store.path(), d.path()), b"not a snapshot").unwrap();
        assert!(load_snapshot(store.path(), d.path()).is_none());
        let a = scan(d.path(), &full()).unwrap();
        save_snapshot(store.path(), d.path(), &a, 1).unwrap();
        // Same file name, different root recorded inside: rejected.
        let other = d.path().join("photos");
        fs::copy(snapshot_file(store.path(), d.path()), snapshot_file(store.path(), &other)).unwrap();
        assert!(load_snapshot(store.path(), &other).is_none());
    }

    #[test]
    fn snapshots_of_one_place_can_be_removed() {
        let (a, b) = (sample(), sample());
        let store = tempfile::tempdir().unwrap();
        for d in [&a, &b] {
            let an = scan(d.path(), &full()).unwrap();
            save_snapshot(store.path(), d.path(), &an, 1).unwrap();
        }
        assert_eq!(snapshot_root(&snapshot_file(store.path(), a.path())).as_deref(), Some(a.path().to_str().unwrap()));
        let (files, bytes) = remove_snapshots_under(store.path(), a.path());
        assert_eq!((files, bytes > 0), (1, true));
        assert!(load_snapshot(store.path(), a.path()).is_none());
        assert!(load_snapshot(store.path(), b.path()).is_some());
        assert_eq!(remove_snapshots_under(store.path(), Path::new("/nowhere")).0, 0);
    }

    #[test]
    fn old_snapshots_are_pruned() {
        let store = tempfile::tempdir().unwrap();
        for i in 0..5 {
            fs::write(store.path().join(format!("{i}.bin")), vec![0u8; 100]).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune_snapshots(store.path(), 3, u64::MAX);
        assert_eq!(fs::read_dir(store.path()).unwrap().count(), 3);
        assert!(store.path().join("4.bin").exists(), "newest are kept");
        prune_snapshots(store.path(), 10, 150);
        assert_eq!(fs::read_dir(store.path()).unwrap().count(), 1);
    }
}
