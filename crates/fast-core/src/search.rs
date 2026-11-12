//! Search over an [`Index`].
//!
//! Query syntax (space separated, all conditions must match):
//!   `report`        substring of the name, case-insensitive
//!   `*.pdf` `a?c`   wildcard against the whole name
//!   `ext:pdf`       extension filter
//!   `type:file` / `type:folder`
//!   `size:>10mb` / `size:<1gb`
//!
//! Strategy: pick the longest plain term as an anchor and run one SIMD
//! substring scan (memchr::memmem) across the contiguous lower-case name
//! arena. Only names containing the anchor are checked against the remaining
//! conditions. A million names is a few tens of megabytes, which scans in
//! single-digit milliseconds.

use crate::index::{Index, SEP};
use memchr::memmem;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Query {
    pub terms: Vec<String>,
    pub globs: Vec<String>,
    pub ext: Option<String>,
    pub want_dir: Option<bool>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
}

impl Query {
    pub fn parse(input: &str) -> Query {
        let mut q = Query::default();
        for tok in input.split_whitespace() {
            let lower = tok.to_lowercase();
            if let Some(v) = lower.strip_prefix("ext:") {
                q.ext = Some(v.trim_start_matches('.').to_string());
            } else if let Some(v) = lower.strip_prefix("type:") {
                q.want_dir = match v {
                    "folder" | "dir" | "directory" => Some(true),
                    "file" => Some(false),
                    _ => None,
                };
            } else if let Some(v) = lower.strip_prefix("size:") {
                if let Some(n) = v.strip_prefix('>').and_then(parse_size) {
                    q.min_size = Some(n);
                } else if let Some(n) = v.strip_prefix('<').and_then(parse_size) {
                    q.max_size = Some(n);
                }
            } else if lower.contains(['*', '?']) {
                q.globs.push(lower);
            } else {
                q.terms.push(lower);
            }
        }
        q
    }

    /// Whether a name (already lower-cased) satisfies every condition of the
    /// query. Shared by the index scan and the live walk, so both agree.
    pub fn matches_name(&self, lower: &[u8], is_dir: bool, size: u64) -> bool {
        if let Some(d) = self.want_dir {
            if is_dir != d {
                return false;
            }
        }
        if self.min_size.map(|m| size < m).unwrap_or(false) || self.max_size.map(|m| size > m).unwrap_or(false) {
            return false;
        }
        if self.terms.iter().any(|t| memmem::find(lower, t.as_bytes()).is_none()) {
            return false;
        }
        if self.globs.iter().any(|g| !glob_match(g.as_bytes(), lower)) {
            return false;
        }
        if let Some(ext) = &self.ext {
            let ok = lower.iter().rposition(|&b| b == b'.').map(|i| &lower[i + 1..] == ext.as_bytes()).unwrap_or(false);
            if !ok || is_dir {
                return false;
            }
        }
        true
    }

    /// The longest plain term, used to rank results.
    pub fn anchor(&self) -> Option<&str> {
        self.terms.iter().max_by_key(|t| t.len()).map(|s| s.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
            && self.globs.is_empty()
            && self.ext.is_none()
            && self.want_dir.is_none()
            && self.min_size.is_none()
            && self.max_size.is_none()
    }
}

/// Parse sizes like `10mb`, `1.5gb`, `300k`, `42`.
pub fn parse_size(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mult: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" => 1024.0,
        "m" | "mb" => 1024.0 * 1024.0,
        "g" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "t" | "tb" => 1024.0f64.powi(4),
        _ => return None,
    };
    Some((n * mult) as u64)
}

/// Iterative wildcard match supporting `*` and `?` over bytes.
pub fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0, 0);
    let (mut star, mut mark) = (usize::MAX, 0);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = p;
            mark = t;
            p += 1;
        } else if star != usize::MAX {
            p = star + 1;
            mark += 1;
            t = mark;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: i64,
    /// Relevance: exact name > prefix > word start > anywhere.
    pub score: u32,
}

/// Best first; ties go to the shorter name, then alphabetical order, so the
/// order never depends on how a filesystem happened to list a folder.
pub fn rank(a: &Hit, b: &Hit) -> std::cmp::Ordering {
    b.score
        .cmp(&a.score)
        .then_with(|| a.name.len().cmp(&b.name.len()))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.path.cmp(&b.path))
}

/// Upper bound on candidates ranked per query, to bound worst-case latency
/// for one-letter queries on huge indexes.
const MAX_CANDIDATES: usize = 200_000;

impl Index {
    pub fn search(&self, q: &Query, scope: Option<&Path>, include_hidden: bool, limit: usize) -> Vec<Hit> {
        if q.is_empty() || self.is_empty() {
            return Vec::new();
        }
        let scope_id = match scope {
            Some(p) => match self.find_path(p) {
                Some(id) => Some(id),
                None => return Vec::new(),
            },
            None => None,
        };

        let anchor = q.terms.iter().max_by_key(|t| t.len());
        let mut scored: Vec<(u32, u32)> = Vec::new(); // (score, id)

        let consider = |id: u32, scored: &mut Vec<(u32, u32)>| {
            let node = self.node(id);
            if node.parent == crate::index::NO_PARENT {
                return; // roots are not search results
            }
            if !include_hidden && node.is_hidden() {
                return;
            }
            let name = self.lower_name(id);
            if !q.matches_name(name, node.is_dir(), node.size) {
                return;
            }
            if let Some(s) = scope_id {
                if id == s || !self.is_under(id, s) {
                    return;
                }
            }
            scored.push((score(name, anchor.map(|a| a.as_bytes())), id));
        };

        match anchor {
            Some(a) => {
                let finder = memmem::Finder::new(a.as_bytes());
                let mut last = u32::MAX;
                for off in finder.find_iter(&self.lower) {
                    let id = self.node_at_lower_offset(off);
                    if id == last {
                        continue; // several matches inside the same name
                    }
                    last = id;
                    consider(id, &mut scored);
                    if scored.len() >= MAX_CANDIDATES {
                        break;
                    }
                }
            }
            None => {
                let (start, end) = match scope_id {
                    Some(s) => (s, self.subtree_end(s)),
                    None => (0, self.len() as u32),
                };
                for id in start..end {
                    consider(id, &mut scored);
                    if scored.len() >= MAX_CANDIDATES {
                        break;
                    }
                }
            }
        }

        // Best score first; ties broken by shorter name, then alphabetically,
        // so the order never depends on how the filesystem happened to list a
        // folder (it differs between machines). Index order is the last resort.
        let cmp = |&(sa, a): &(u32, u32), &(sb, b): &(u32, u32)| {
            sb.cmp(&sa)
                .then_with(|| self.node(a).name_len_hint().cmp(&self.node(b).name_len_hint()))
                .then_with(|| self.lower_name(a).cmp(self.lower_name(b)))
                .then_with(|| self.name(a).cmp(self.name(b)))
                .then_with(|| a.cmp(&b))
        };
        // Only the best `limit` are ever shown, so pick them in linear time and
        // sort just those, instead of sorting every candidate.
        if limit > 0 && scored.len() > limit {
            scored.select_nth_unstable_by(limit - 1, cmp);
            scored.truncate(limit);
        }
        scored.sort_unstable_by(cmp);
        scored.truncate(limit);
        scored
            .into_iter()
            .map(|(score, id)| {
                let n = self.node(id);
                Hit {
                    path: self.path_of(id).to_string_lossy().into_owned(),
                    name: self.name(id).to_string(),
                    is_dir: n.is_dir(),
                    size: n.size,
                    modified: n.mtime * 1000,
                    score,
                }
            })
            .collect()
    }
}

/// Settings for [`live_search`].
pub struct LiveOptions<'a> {
    pub threads: usize,
    pub include_hidden: bool,
    pub cancel: &'a std::sync::atomic::AtomicBool,
    pub skip: Vec<std::path::PathBuf>,
}

/// Search a folder that has no index yet by walking it, and hand matches to
/// `emit` as they are found instead of after the whole walk: the first results
/// arrive in milliseconds, whatever the size of the drive.
///
/// The walk goes breadth first, so shallow (usually more relevant) matches come
/// first, and is split into chunks of folders read in parallel, so a slow disk
/// or network share keeps every thread busy. Returns the number of entries
/// looked at. `emit` gets `(matches, entries_seen_so_far)`.
pub fn live_search(root: &Path, q: &Query, opts: &LiveOptions, mut emit: impl FnMut(Vec<Hit>, usize)) -> usize {
    use rayon::prelude::*;
    use std::sync::atomic::Ordering;
    const CHUNK: usize = 48;
    if q.is_empty() {
        return 0;
    }
    let anchor = q.anchor().map(|a| a.as_bytes().to_vec());
    let pool = rayon::ThreadPoolBuilder::new().num_threads(opts.threads.max(1)).build().ok();
    let mut frontier: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    let mut seen = 0usize;
    // Folders one level deeper than the chunk being read, kept for the next round.
    let mut next: Vec<std::path::PathBuf> = Vec::new();
    while !frontier.is_empty() {
        let mut queue = std::mem::take(&mut frontier);
        queue.reverse(); // pop from the front, in order
        while !queue.is_empty() {
            if opts.cancel.load(Ordering::Relaxed) {
                return seen;
            }
            let take = queue.len().min(CHUNK);
            let chunk: Vec<std::path::PathBuf> = queue.split_off(queue.len() - take);
            let work = || -> Vec<(Vec<std::path::PathBuf>, Vec<Hit>, usize)> {
                chunk
                    .par_iter()
                    .map(|dir| walk_one(dir, q, anchor.as_deref(), opts))
                    .collect()
            };
            let results = match &pool {
                Some(p) => p.install(work),
                None => work(),
            };
            let mut batch: Vec<Hit> = Vec::new();
            for (dirs, hits, count) in results {
                seen += count;
                next.extend(dirs);
                batch.extend(hits);
            }
            if !batch.is_empty() {
                batch.sort_by(rank);
                emit(batch, seen);
            }
        }
        frontier = std::mem::take(&mut next);
    }
    emit(Vec::new(), seen);
    seen
}

fn walk_one(dir: &Path, q: &Query, anchor: Option<&[u8]>, opts: &LiveOptions) -> (Vec<std::path::PathBuf>, Vec<Hit>, usize) {
    let mut dirs = Vec::new();
    let mut hits = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return (dirs, hits, 0) };
    let mut count = 0;
    for e in rd.flatten() {
        count += 1;
        let Ok(ft) = e.file_type() else { continue };
        let is_dir = ft.is_dir(); // symlinks are never followed
        let name = e.file_name();
        let name = name.to_string_lossy();
        let lower = name.to_lowercase();
        let path = e.path();
        if is_dir && !opts.skip.iter().any(|s| s == &path) {
            dirs.push(path.clone());
        }
        // Size is only needed when the query filters on it.
        let needs_meta = q.min_size.is_some() || q.max_size.is_some();
        let early = if needs_meta { None } else { Some(0u64) };
        let size = match early {
            Some(s) => s,
            None => e.metadata().map(|m| if is_dir { 0 } else { m.len() }).unwrap_or(0),
        };
        if !q.matches_name(lower.as_bytes(), is_dir, size) {
            continue;
        }
        let meta = e.metadata().ok();
        if !opts.include_hidden && crate::entry::is_hidden(&name, meta.as_ref()) {
            continue;
        }
        let (size, modified) = match &meta {
            Some(m) => (
                if is_dir { 0 } else { m.len() },
                m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0),
            ),
            None => (0, 0),
        };
        hits.push(Hit { path: path.to_string_lossy().into_owned(), name: name.into_owned(), is_dir, size, modified, score: score(lower.as_bytes(), anchor) });
    }
    (dirs, hits, count)
}

/// Rank: exact name > exact stem > prefix > word start > anywhere.
fn score(name: &[u8], anchor: Option<&[u8]>) -> u32 {
    let Some(a) = anchor else { return 1 };
    debug_assert!(!name.contains(&SEP));
    if name == a {
        return 100;
    }
    let stem = name.iter().rposition(|&b| b == b'.').map(|i| &name[..i]).unwrap_or(name);
    if stem == a {
        return 90;
    }
    if name.starts_with(a) {
        return 70;
    }
    if let Some(pos) = memmem::find(name, a) {
        let prev = name[pos - 1]; // pos > 0 because starts_with was handled
        if !prev.is_ascii_alphanumeric() {
            return 50;
        }
    }
    10
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{tests::sample_tree, ScanOptions};

    fn names(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.name.as_str()).collect()
    }

    #[test]
    fn parses_query_syntax() {
        let q = Query::parse("Report *.PDF ext:.pdf type:file size:>1.5kb size:<1gb");
        assert_eq!(q.terms, vec!["report"]);
        assert_eq!(q.globs, vec!["*.pdf"]);
        assert_eq!(q.ext.as_deref(), Some("pdf"));
        assert_eq!(q.want_dir, Some(false));
        assert_eq!(q.min_size, Some(1536));
        assert_eq!(q.max_size, Some(1 << 30));
        assert!(Query::parse("   ").is_empty());
    }

    #[test]
    fn glob_matching() {
        assert!(glob_match(b"*.pdf", b"q1-report.pdf"));
        assert!(glob_match(b"q?-*", b"q1-report.pdf"));
        assert!(!glob_match(b"*.pdf", b"readme.md"));
        assert!(glob_match(b"*", b""));
        assert!(!glob_match(b"?", b""));
        assert!(glob_match(b"a*b*c", b"aXXbYYc"));
    }

    #[test]
    fn substring_search_is_case_insensitive_and_ranked() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        let hits = idx.search(&Query::parse("REPORT"), None, true, 50);
        // "reports" (prefix) outranks "q1-report.pdf" (word start).
        assert_eq!(names(&hits), vec!["reports", "Q1-Report.pdf", "q2-report.pdf"]);
        assert!(hits[1].path.ends_with("Q1-Report.pdf"));
    }

    #[test]
    fn filters_and_scope() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        assert_eq!(names(&idx.search(&Query::parse("ext:jpg"), None, true, 50)), vec!["Beach.JPG"]);
        assert_eq!(idx.search(&Query::parse("type:folder"), None, true, 50).len(), 3);
        assert_eq!(names(&idx.search(&Query::parse("size:>4kb type:file"), None, true, 50)).len(), 2);
        assert_eq!(idx.search(&Query::parse("report q2"), None, true, 50).len(), 1);
        let scope = d.path().join("photos");
        assert!(idx.search(&Query::parse("report"), Some(&scope), true, 50).is_empty());
        assert_eq!(idx.search(&Query::parse("*"), Some(&scope), true, 50).len(), 1);
        assert_eq!(idx.search(&Query::parse("e"), None, true, 2).len(), 2);
    }

    #[test]
    fn live_search_streams_matches_and_respects_cancel() {
        let d = sample_tree();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let opts = LiveOptions { threads: 2, include_hidden: true, cancel: &cancel, skip: vec![] };
        let mut got: Vec<Hit> = Vec::new();
        let mut batches = 0;
        let seen = live_search(d.path(), &Query::parse("REPORT"), &opts, |hits, _| {
            if !hits.is_empty() {
                batches += 1;
            }
            got.extend(hits);
        });
        let mut names: Vec<_> = got.iter().map(|h| h.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["Q1-Report.pdf", "q2-report.pdf", "reports"]);
        assert!(batches >= 1);
        assert_eq!(seen, 8); // everything under the root
        // Filters work the same as in the index.
        let mut pdfs = Vec::new();
        live_search(d.path(), &Query::parse("ext:pdf size:>4000"), &opts, |h, _| pdfs.extend(h));
        assert_eq!(pdfs.len(), 1);
        assert_eq!(pdfs[0].name, "q2-report.pdf");
        // A cancelled search stops at once.
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(live_search(d.path(), &Query::parse("report"), &opts, |_, _| panic!("no results after cancel")), 0);
    }
}
