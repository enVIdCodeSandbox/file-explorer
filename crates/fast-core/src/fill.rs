//! Does a folder hold anything? Used to draw empty folders differently from folders with files in them, the way
//! Windows does. Folders that only hold other (empty) folders count as empty, found by looking down through them.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Fill {
    /// Nothing in it.
    Empty,
    /// Only folders, and none of them (as far as was looked) holds a file.
    OnlyFolders,
    /// At least one file.
    Files,
}

/// Names the system puts in folders on its own; they do not count as contents.
fn ignored(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "desktop.ini" | "thumbs.db" | ".ds_store" | "$recycle.bin" | ".directory")
}

/// Look inside `path`: first its own entries, then down through its subfolders (breadth first), stopping at the first
/// file. Looks at no more than `budget` entries and `max_depth` levels; past that it answers `Files` (when in doubt, "full").
pub fn probe(path: &Path, max_depth: usize, budget: usize) -> Fill {
    let mut queue: VecDeque<(PathBuf, usize)> = VecDeque::from([(path.to_path_buf(), 0)]);
    let mut seen = 0usize;
    let mut any = false;
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            // Cannot be read (no permission, gone): do not claim it is empty.
            if dir == path {
                return Fill::Files;
            }
            continue;
        };
        for e in rd.flatten() {
            if ignored(&e.file_name().to_string_lossy()) {
                continue;
            }
            any = true;
            seen += 1;
            if seen > budget {
                return Fill::Files;
            }
            // `file_type` does not follow links: a link counts as a file, so a link loop cannot make us wander.
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    if depth + 1 >= max_depth {
                        return Fill::Files;
                    }
                    queue.push_back((e.path(), depth + 1));
                }
                _ => return Fill::Files,
            }
        }
    }
    if any { Fill::OnlyFolders } else { Fill::Empty }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn tells_empty_from_folders_only_from_files() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::create_dir_all(r.join("empty")).unwrap();
        fs::create_dir_all(r.join("nest/a/b")).unwrap();
        fs::create_dir_all(r.join("deep/a/b")).unwrap();
        fs::write(r.join("deep/a/b/file.txt"), "x").unwrap();
        fs::create_dir_all(r.join("junk")).unwrap();
        fs::write(r.join("junk/desktop.ini"), "x").unwrap();
        fs::create_dir_all(r.join("top")).unwrap();
        fs::write(r.join("top/f"), "x").unwrap();
        assert_eq!(probe(&r.join("empty"), 8, 1000), Fill::Empty);
        assert_eq!(probe(&r.join("nest"), 8, 1000), Fill::OnlyFolders);
        assert_eq!(probe(&r.join("deep"), 8, 1000), Fill::Files);
        assert_eq!(probe(&r.join("junk"), 8, 1000), Fill::Empty);
        assert_eq!(probe(&r.join("top"), 8, 1000), Fill::Files);
        assert_eq!(probe(&r.join("missing"), 8, 1000), Fill::Files);
        // Too deep to see the bottom: assume there is something.
        assert_eq!(probe(&r.join("nest"), 2, 1000), Fill::Files);
    }
}
