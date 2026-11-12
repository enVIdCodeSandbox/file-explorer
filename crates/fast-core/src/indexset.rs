//! One [`Index`] per drive, searched together.
//!
//! Each drive is scanned, saved and swapped in on its own, so search works on
//! the first drive the moment it finishes instead of waiting for the slowest
//! one, and refreshing one drive never rebuilds the others. Drives are
//! searched in parallel and their results merged by relevance.

use crate::index::Index;
use crate::search::{rank, Hit, Query};
use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct IndexSet {
    parts: Vec<Arc<Index>>,
}

/// Stable identity of a part: the full path of its root.
fn key(i: &Index) -> String {
    i.roots.first().map(|&r| i.name(r).to_string()).unwrap_or_default()
}

impl IndexSet {
    pub fn parts(&self) -> &[Arc<Index>] {
        &self.parts
    }

    /// A new set with `part` added, or replacing the part for the same root.
    pub fn with_part(&self, part: Arc<Index>) -> IndexSet {
        let k = key(&part);
        let mut parts: Vec<Arc<Index>> = self.parts.iter().filter(|p| key(p) != k).cloned().collect();
        parts.push(part);
        parts.sort_by_key(|p| key(p));
        IndexSet { parts }
    }

    /// Keep only parts whose root is in `roots` (a drive that went away).
    pub fn retain_roots(&self, roots: &[String]) -> IndexSet {
        IndexSet { parts: self.parts.iter().filter(|p| roots.contains(&key(p))).cloned().collect() }
    }

    pub fn len(&self) -> usize {
        self.parts.iter().map(|p| p.len()).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(|p| p.is_empty())
    }
    pub fn memory_bytes(&self) -> usize {
        self.parts.iter().map(|p| p.memory_bytes()).sum()
    }
    pub fn truncated(&self) -> bool {
        self.parts.iter().any(|p| p.truncated)
    }
    pub fn built_at(&self) -> i64 {
        self.parts.iter().map(|p| p.built_at).max().unwrap_or(0)
    }
    pub fn root_names(&self) -> Vec<String> {
        self.parts.iter().map(|p| key(p)).collect()
    }

    /// Whether the index holds this path (its drive is indexed and the folder was seen).
    pub fn covers(&self, path: &Path) -> bool {
        self.parts.iter().any(|p| p.covers(path) && p.find_path(path).is_some())
    }

    /// Whether some part's root contains `path`, even if the folder itself was not seen.
    pub fn has_root_for(&self, path: &Path) -> bool {
        self.parts.iter().any(|p| p.covers(path))
    }

    pub fn search(&self, q: &Query, scope: Option<&Path>, include_hidden: bool, limit: usize) -> Vec<Hit> {
        if q.is_empty() {
            return Vec::new();
        }
        if let Some(s) = scope {
            return match self.parts.iter().find(|p| p.covers(s) && p.find_path(s).is_some()) {
                Some(p) => p.search(q, Some(s), include_hidden, limit),
                None => Vec::new(),
            };
        }
        if self.parts.len() == 1 {
            return self.parts[0].search(q, None, include_hidden, limit);
        }
        let mut hits: Vec<Hit> = self.parts.par_iter().flat_map(|p| p.search(q, None, include_hidden, limit)).collect();
        hits.sort_by(rank);
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{tests::sample_tree, ScanOptions};

    fn part(d: &tempfile::TempDir) -> Arc<Index> {
        Arc::new(Index::scan(&[d.path().to_path_buf()], &ScanOptions::default()))
    }

    #[test]
    fn searches_every_part_and_merges_by_relevance() {
        let (a, b) = (sample_tree(), sample_tree());
        let set = IndexSet::default().with_part(part(&a)).with_part(part(&b));
        assert_eq!(set.parts().len(), 2);
        let hits = set.search(&Query::parse("report"), None, true, 50);
        assert_eq!(hits.len(), 6); // 3 per drive
        assert!(hits.windows(2).all(|w| rank(&w[0], &w[1]) != std::cmp::Ordering::Greater));
        // A scope picks just its own drive.
        let scoped = set.search(&Query::parse("report"), Some(a.path()), true, 50);
        assert_eq!(scoped.len(), 3);
        assert!(scoped.iter().all(|h| h.path.starts_with(a.path().to_str().unwrap())));
    }

    #[test]
    fn replacing_a_part_does_not_duplicate_it_and_roots_can_go() {
        let a = sample_tree();
        let set = IndexSet::default().with_part(part(&a)).with_part(part(&a));
        assert_eq!(set.parts().len(), 1);
        assert!(set.covers(&a.path().join("docs")));
        assert!(!set.covers(&a.path().join("missing")));
        assert!(set.has_root_for(&a.path().join("missing")));
        assert!(set.retain_roots(&[]).is_empty());
    }
}
