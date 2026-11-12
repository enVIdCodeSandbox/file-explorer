//! Compact in-memory file index.
//!
//! One `Node` per file or folder (40 bytes) plus two string arenas holding
//! every name (original and lower-cased). Nodes are stored in depth-first
//! pre-order, so a parent always has a smaller id than its children. That
//! ordering makes subtree aggregation a single reverse pass and lets search
//! run one SIMD substring scan over a contiguous buffer.
//!
//! The index is built by a parallel directory walk that needs no special
//! privileges. Faster privileged sources (NTFS MFT/USN journal on Windows)
//! can populate the same structure later; see `docs/ROADMAP.md`.

use crate::{to_millis, CoreError, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub const NO_PARENT: u32 = u32::MAX;
const FLAG_DIR: u8 = 1;
const FLAG_HIDDEN: u8 = 2;
/// Separator between names in the lower-case arena. It cannot occur in a file
/// name or in a query, so a substring match can never span two names.
pub(crate) const SEP: u8 = 0;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct Node {
    pub parent: u32,
    name_off: u32,
    lower_off: u32,
    name_len: u16,
    lower_len: u16,
    flags: u8,
    pub size: u64,
    /// Seconds since the Unix epoch (0 when unknown).
    pub mtime: i64,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.flags & FLAG_DIR != 0
    }
    pub fn is_hidden(&self) -> bool {
        self.flags & FLAG_HIDDEN != 0
    }
    pub(crate) fn name_len_hint(&self) -> u16 {
        self.name_len
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Index {
    pub(crate) nodes: Vec<Node>,
    names: String,
    pub(crate) lower: Vec<u8>,
    /// Ids of root nodes. A root's "name" is its full path.
    pub roots: Vec<u32>,
    /// True when the scan stopped early (entry cap or cancellation).
    pub truncated: bool,
    /// Unix seconds when the scan finished.
    pub built_at: i64,
}

pub struct ScanOptions<'a> {
    pub threads: usize,
    /// Stop adding entries beyond this count (0 = unlimited).
    pub max_entries: usize,
    pub cancel: Option<&'a AtomicBool>,
    /// Absolute paths never descended into.
    pub skip: Vec<PathBuf>,
}

impl Default for ScanOptions<'_> {
    fn default() -> Self {
        ScanOptions { threads: 0, max_entries: 0, cancel: None, skip: default_skip_list() }
    }
}

/// Pseudo-filesystems and duplicates that would only pollute an index.
pub fn default_skip_list() -> Vec<PathBuf> {
    let list: &[&str] = if cfg!(target_os = "linux") || cfg!(target_os = "android") {
        &["/proc", "/sys", "/dev", "/run", "/snap", "/var/lib/docker"]
    } else if cfg!(target_os = "macos") {
        &["/dev", "/System/Volumes", "/private/var/vm", "/Volumes/.timemachine"]
    } else {
        &[]
    };
    list.iter().map(PathBuf::from).collect()
}

struct Raw {
    name: String,
    is_dir: bool,
    hidden: bool,
    size: u64,
    mtime: i64,
    children: Vec<Raw>,
}

struct Ctx<'a> {
    opts: &'a ScanOptions<'a>,
    count: AtomicUsize,
    truncated: AtomicBool,
}

impl Ctx<'_> {
    fn stop(&self) -> bool {
        if self.opts.cancel.map(|c| c.load(Ordering::Relaxed)).unwrap_or(false) {
            self.truncated.store(true, Ordering::Relaxed);
            return true;
        }
        if self.opts.max_entries > 0 && self.count.load(Ordering::Relaxed) >= self.opts.max_entries {
            self.truncated.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }
}

fn scan_dir(path: &Path, ctx: &Ctx) -> Vec<Raw> {
    if ctx.stop() {
        return Vec::new();
    }
    let Ok(rd) = fs::read_dir(path) else { return Vec::new() };
    let mut out: Vec<Raw> = Vec::new();
    for e in rd.filter_map(|e| e.ok()) {
        let Ok(ft) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().into_owned();
        // Never follow symlinks: avoids cycles and double counting.
        let meta = e.metadata().ok();
        let is_dir = ft.is_dir();
        let hidden = crate::entry::is_hidden(&name, meta.as_ref());
        let (size, mtime) = match &meta {
            Some(m) => (
                if is_dir { 0 } else { m.len() },
                to_millis(m.modified()).map(|ms| ms / 1000).unwrap_or(0),
            ),
            None => (0, 0),
        };
        out.push(Raw { name, is_dir, hidden, size, mtime, children: Vec::new() });
    }
    ctx.count.fetch_add(out.len(), Ordering::Relaxed);

    out.par_iter_mut().filter(|r| r.is_dir).for_each(|r| {
        let child = path.join(&r.name);
        if !ctx.opts.skip.iter().any(|s| s == &child) {
            r.children = scan_dir(&child, ctx);
        }
    });
    out
}

impl Index {
    /// Walk `roots` in parallel and build an index of everything below them.
    pub fn scan(roots: &[PathBuf], opts: &ScanOptions) -> Index {
        let threads = if opts.threads == 0 {
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
        } else {
            opts.threads
        };
        let ctx = Ctx { opts, count: AtomicUsize::new(0), truncated: AtomicBool::new(false) };
        let run = || -> Vec<(PathBuf, Vec<Raw>)> {
            roots.par_iter().map(|r| (r.clone(), scan_dir(r, &ctx))).collect()
        };
        let scanned = match rayon::ThreadPoolBuilder::new().num_threads(threads).build() {
            Ok(pool) => pool.install(run),
            Err(_) => run(),
        };

        let mut idx = Index::default();
        let total = ctx.count.load(Ordering::Relaxed) + roots.len();
        idx.nodes.reserve(total);
        for (root, children) in scanned {
            let mtime = fs::metadata(&root)
                .ok()
                .and_then(|m| to_millis(m.modified()))
                .map(|ms| ms / 1000)
                .unwrap_or(0);
            let id = idx.push(NO_PARENT, &root.to_string_lossy(), FLAG_DIR, 0, mtime);
            idx.roots.push(id);
            idx.flatten(id, children);
        }
        idx.truncated = ctx.truncated.load(Ordering::Relaxed);
        idx.built_at = to_millis(Ok(std::time::SystemTime::now())).unwrap_or(0) / 1000;
        idx.nodes.shrink_to_fit();
        idx.names.shrink_to_fit();
        idx.lower.shrink_to_fit();
        idx
    }

    fn push(&mut self, parent: u32, name: &str, flags: u8, size: u64, mtime: i64) -> u32 {
        // Names longer than u16::MAX bytes cannot exist on supported filesystems.
        let name = if name.len() > u16::MAX as usize { &name[..floor_boundary(name, u16::MAX as usize)] } else { name };
        let lower = name.to_lowercase();
        let lower = if lower.len() > u16::MAX as usize { &lower[..floor_boundary(&lower, u16::MAX as usize)] } else { &lower[..] };
        let id = self.nodes.len() as u32;
        self.nodes.push(Node {
            parent,
            name_off: self.names.len() as u32,
            lower_off: self.lower.len() as u32,
            name_len: name.len() as u16,
            lower_len: lower.len() as u16,
            flags,
            size,
            mtime,
        });
        self.names.push_str(name);
        self.lower.extend_from_slice(lower.as_bytes());
        self.lower.push(SEP);
        id
    }

    fn flatten(&mut self, parent: u32, children: Vec<Raw>) {
        for c in children {
            // u32 ids and offsets cap the index at ~4 billion entries / 4 GB of names.
            if self.nodes.len() >= (u32::MAX - 1) as usize || self.names.len() >= (u32::MAX as usize - 70_000) {
                self.truncated = true;
                return;
            }
            let flags = (c.is_dir as u8 * FLAG_DIR) | (c.hidden as u8 * FLAG_HIDDEN);
            let id = self.push(parent, &c.name, flags, c.size, c.mtime);
            if !c.children.is_empty() {
                self.flatten(id, c.children);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }
    pub fn name(&self, id: u32) -> &str {
        let n = &self.nodes[id as usize];
        &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }
    pub(crate) fn lower_name(&self, id: u32) -> &[u8] {
        let n = &self.nodes[id as usize];
        &self.lower[n.lower_off as usize..n.lower_off as usize + n.lower_len as usize]
    }
    /// Map a byte offset in the lower-case arena back to the node that owns it.
    pub(crate) fn node_at_lower_offset(&self, off: usize) -> u32 {
        (self.nodes.partition_point(|n| n.lower_off as usize <= off) - 1) as u32
    }

    /// Approximate heap use in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.nodes.capacity() * std::mem::size_of::<Node>() + self.names.capacity() + self.lower.capacity()
    }

    pub fn path_of(&self, id: u32) -> PathBuf {
        let mut parts: Vec<&str> = Vec::new();
        let mut cur = id;
        loop {
            parts.push(self.name(cur));
            let p = self.nodes[cur as usize].parent;
            if p == NO_PARENT {
                break;
            }
            cur = p;
        }
        let mut path = PathBuf::from(parts.pop().unwrap_or_default());
        while let Some(p) = parts.pop() {
            path.push(p);
        }
        path
    }

    /// Find the node for an absolute path, if it is in the index.
    pub fn find_path(&self, path: &Path) -> Option<u32> {
        // Pick the deepest root that is a prefix of `path`.
        let (root, rest) = self
            .roots
            .iter()
            .filter_map(|&r| path.strip_prefix(self.name(r)).ok().map(|rest| (r, rest)))
            .min_by_key(|(_, rest)| rest.as_os_str().len())?;
        let mut cur = root;
        for comp in rest.components() {
            let want = comp.as_os_str().to_string_lossy();
            cur = self.children_scan(cur).find(|&c| self.name(c) == want)?;
        }
        Some(cur)
    }

    /// Iterate direct children of `id` by skipping over whole subtrees.
    fn children_scan(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        let end = self.subtree_end(id);
        let mut next = id + 1;
        std::iter::from_fn(move || {
            while next < end {
                let c = next;
                if self.nodes[c as usize].parent == id {
                    next = c + 1;
                    return Some(c);
                }
                next += 1;
            }
            None
        })
    }

    /// One past the last node in `id`'s subtree (pre-order makes it contiguous).
    pub fn subtree_end(&self, id: u32) -> u32 {
        let mut end = id + 1;
        let n = self.nodes.len() as u32;
        while end < n {
            // A node belongs to the subtree while its ancestor chain reaches `id`.
            // Because parents precede children, the first node whose parent is
            // < id (or a root) ends the subtree.
            let p = self.nodes[end as usize].parent;
            if p == NO_PARENT || p < id {
                break;
            }
            end += 1;
        }
        end
    }

    pub fn is_under(&self, mut id: u32, ancestor: u32) -> bool {
        loop {
            if id == ancestor {
                return true;
            }
            // Parents always have smaller ids, so stop once we pass the target.
            if id < ancestor {
                return false;
            }
            let p = self.nodes[id as usize].parent;
            if p == NO_PARENT {
                return false;
            }
            id = p;
        }
    }

    /// Whether `path` lies inside an indexed root.
    pub fn covers(&self, path: &Path) -> bool {
        self.roots.iter().any(|&r| path.starts_with(self.name(r)))
    }

    /// Total size of every subtree, indexed by node id. Single reverse pass.
    pub fn subtree_sizes(&self) -> Vec<u64> {
        let mut sizes: Vec<u64> = self.nodes.iter().map(|n| n.size).collect();
        for i in (0..self.nodes.len()).rev() {
            let p = self.nodes[i].parent;
            if p != NO_PARENT {
                sizes[p as usize] += sizes[i];
            }
        }
        sizes
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let tmp = file.with_extension("tmp");
        let f = fs::File::create(&tmp).map_err(|e| CoreError::io(&tmp, e))?;
        bincode::serialize_into(std::io::BufWriter::new(f), self)
            .map_err(|e| CoreError::Invalid(format!("index save failed: {e}")))?;
        fs::rename(&tmp, file).map_err(|e| CoreError::io(file, e))
    }

    pub fn load(file: &Path) -> Result<Index> {
        let f = fs::File::open(file).map_err(|e| CoreError::io(file, e))?;
        bincode::deserialize_from(std::io::BufReader::new(f))
            .map_err(|e| CoreError::Invalid(format!("index load failed: {e}")))
    }
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn sample_tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        fs::create_dir_all(p.join("docs/reports")).unwrap();
        fs::create_dir_all(p.join("photos")).unwrap();
        fs::write(p.join("docs/readme.md"), vec![0u8; 100]).unwrap();
        fs::write(p.join("docs/reports/Q1-Report.pdf"), vec![0u8; 3000]).unwrap();
        fs::write(p.join("docs/reports/q2-report.pdf"), vec![0u8; 5000]).unwrap();
        fs::write(p.join("photos/Beach.JPG"), vec![0u8; 20000]).unwrap();
        fs::write(p.join("notes.txt"), vec![0u8; 10]).unwrap();
        d
    }

    #[test]
    fn scan_builds_preorder_tree_with_paths() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        assert_eq!(idx.len(), 9); // root + 3 dirs + 5 files
        assert!(!idx.truncated);
        for (i, n) in idx.nodes.iter().enumerate() {
            assert!(n.parent == NO_PARENT || (n.parent as usize) < i);
        }
        let id = idx.find_path(&d.path().join("docs/reports/q2-report.pdf")).unwrap();
        assert_eq!(idx.node(id).size, 5000);
        assert_eq!(idx.path_of(id), d.path().join("docs/reports/q2-report.pdf"));
        assert!(idx.find_path(&d.path().join("nope")).is_none());
    }

    #[test]
    fn subtree_sizes_aggregate() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        let sizes = idx.subtree_sizes();
        assert_eq!(sizes[idx.roots[0] as usize], 28110);
        let docs = idx.find_path(&d.path().join("docs")).unwrap();
        assert_eq!(sizes[docs as usize], 8100);
        let end = idx.subtree_end(docs);
        assert_eq!(end - docs, 5); // docs, readme, reports, 2 pdfs
    }

    #[test]
    fn save_load_roundtrip_and_entry_cap() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        let f = d.path().join("cache/index.bin");
        idx.save(&f).unwrap();
        let back = Index::load(&f).unwrap();
        assert_eq!(back.len(), idx.len());
        assert_eq!(back.path_of(5), idx.path_of(5));

        let capped = Index::scan(
            &[d.path().to_path_buf()],
            &ScanOptions { max_entries: 2, threads: 1, ..Default::default() },
        );
        assert!(capped.truncated);
        assert!(capped.len() < 10);
    }
}
