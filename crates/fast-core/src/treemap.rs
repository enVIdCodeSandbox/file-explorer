//! Disk usage analysis (the WinDirStat-style view). Produces a size tree
//! pruned to what a treemap can actually draw, plus a flat list of the
//! largest files.

use crate::index::{Index, NO_PARENT};
use serde::Serialize;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SizeNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub children: Vec<SizeNode>,
    /// Largest file types inside this folder: (index into `Analysis::ext_names`, bytes).
    /// Empty for files and for scans that do not track types.
    pub types: Vec<(u16, u64)>,
}

/// Bytes and files of one file type across the whole scan.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TypeStat {
    pub ext: u16,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LargeFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: i64,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub tree: SizeNode,
    pub largest: Vec<LargeFile>,
    pub total_bytes: u64,
    pub file_count: u64,
    pub complete: bool,
    /// File type names (lower case, no dot) that `SizeNode::types` and `types` refer to.
    pub ext_names: Vec<String>,
    /// File types by total size, largest first.
    pub types: Vec<TypeStat>,
}

pub struct TreeOptions {
    /// Levels below the root to include.
    pub max_depth: u32,
    /// Children kept per folder; the rest merge into one "(other)" node.
    pub max_children: usize,
    /// Drop nodes smaller than this fraction of the root (too small to draw).
    pub min_fraction: f64,
    pub largest_files: usize,
}

impl Default for TreeOptions {
    fn default() -> Self {
        TreeOptions { max_depth: 4, max_children: 40, min_fraction: 0.0005, largest_files: 100 }
    }
}

pub fn analyze(idx: &Index, root: u32, opts: &TreeOptions) -> Analysis {
    let sizes = idx.subtree_sizes();
    let end = idx.subtree_end(root);

    // Direct children of every folder in the subtree, built in one pass.
    let span = (end - root) as usize;
    let mut children: Vec<Vec<u32>> = vec![Vec::new(); span];
    let mut file_count = 0u64;
    let mut largest: Vec<(u64, u32)> = Vec::new();
    for id in root + 1..end {
        let n = idx.node(id);
        if n.parent != NO_PARENT && n.parent >= root {
            children[(n.parent - root) as usize].push(id);
        }
        if !n.is_dir() {
            file_count += 1;
            largest.push((n.size, id));
            if largest.len() > opts.largest_files * 8 + 64 {
                largest.sort_unstable_by(|a, b| b.cmp(a));
                largest.truncate(opts.largest_files);
            }
        }
    }
    largest.sort_unstable_by(|a, b| b.cmp(a));
    largest.truncate(opts.largest_files);

    let total = sizes[root as usize];
    let min_size = (total as f64 * opts.min_fraction) as u64;
    let tree = build(idx, root, root, &sizes, &children, opts, min_size, 0);

    Analysis {
        tree,
        largest: largest
            .into_iter()
            .map(|(size, id)| LargeFile {
                path: idx.path_of(id).to_string_lossy().into_owned(),
                name: idx.name(id).to_string(),
                size,
                modified: idx.node(id).mtime * 1000,
            })
            .collect(),
        total_bytes: total,
        file_count,
        complete: !idx.truncated,
        ext_names: Vec::new(),
        types: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build(
    idx: &Index,
    root: u32,
    id: u32,
    sizes: &[u64],
    children: &[Vec<u32>],
    opts: &TreeOptions,
    min_size: u64,
    depth: u32,
) -> SizeNode {
    let node = idx.node(id);
    let path = idx.path_of(id).to_string_lossy().into_owned();
    let mut out = SizeNode {
        name: if id == root { path.clone() } else { idx.name(id).to_string() },
        path,
        size: sizes[id as usize],
        is_dir: node.is_dir(),
        children: Vec::new(),
        types: Vec::new(),
    };
    if !node.is_dir() || depth >= opts.max_depth {
        return out;
    }
    let mut kids: Vec<u32> = children[(id - root) as usize].clone();
    kids.sort_unstable_by_key(|&c| std::cmp::Reverse(sizes[c as usize]));
    let mut other = 0u64;
    for (i, &c) in kids.iter().enumerate() {
        let s = sizes[c as usize];
        if i >= opts.max_children || s < min_size || s == 0 {
            other += s;
        } else {
            out.children.push(build(idx, root, c, sizes, children, opts, min_size, depth + 1));
        }
    }
    if other > 0 {
        out.children.push(SizeNode {
            name: "(other)".into(),
            path: String::new(),
            size: other,
            is_dir: false,
            children: Vec::new(),
            types: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{tests::sample_tree, ScanOptions};

    #[test]
    fn builds_sorted_tree_and_largest_list() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        let a = analyze(&idx, 0, &TreeOptions { min_fraction: 0.0, ..Default::default() });
        assert_eq!(a.total_bytes, 28110);
        assert_eq!(a.file_count, 5);
        assert_eq!(a.tree.size, 28110);
        let names: Vec<&str> = a.tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["photos", "docs", "notes.txt"]);
        // Every folder's size equals the sum of its children.
        fn check(n: &SizeNode) {
            if n.is_dir && !n.children.is_empty() {
                assert_eq!(n.size, n.children.iter().map(|c| c.size).sum::<u64>());
                n.children.iter().for_each(check);
            }
        }
        check(&a.tree);
        assert_eq!(a.largest[0].name, "Beach.JPG");
        assert_eq!(a.largest.len(), 5);
    }

    #[test]
    fn prunes_small_and_excess_children_into_other() {
        let d = sample_tree();
        let idx = Index::scan(&[d.path().to_path_buf()], &ScanOptions::default());
        let a = analyze(&idx, 0, &TreeOptions { max_children: 1, max_depth: 1, min_fraction: 0.0, largest_files: 2 });
        assert_eq!(a.tree.children.len(), 2);
        assert_eq!(a.tree.children[1].name, "(other)");
        assert_eq!(a.tree.children[1].size, 8110);
        assert!(a.tree.children[0].children.is_empty());
        assert_eq!(a.largest.len(), 2);
    }
}
