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
    #[serde(skip)]
    pub score: u32,
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
            if let Some(d) = q.want_dir {
                if node.is_dir() != d {
                    return;
                }
            }
            if q.min_size.map(|m| node.size < m).unwrap_or(false)
                || q.max_size.map(|m| node.size > m).unwrap_or(false)
            {
                return;
            }
            let name = self.lower_name(id);
            for t in &q.terms {
                if memmem::find(name, t.as_bytes()).is_none() {
                    return;
                }
            }
            for g in &q.globs {
                if !glob_match(g.as_bytes(), name) {
                    return;
                }
            }
            if let Some(ext) = &q.ext {
                let ok = name
                    .iter()
                    .rposition(|&b| b == b'.')
                    .map(|i| &name[i + 1..] == ext.as_bytes())
                    .unwrap_or(false);
                if !ok || node.is_dir() {
                    return;
                }
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

        // Best score first; ties broken by shorter name, then index order.
        scored.sort_unstable_by_key(|&(s, id)| (std::cmp::Reverse(s), self.node(id).name_len_hint(), id));
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
}
