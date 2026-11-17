//! Icon collections the person can install. None ship with the app: a collection is downloaded on request (from the
//! Iconify data on a public CDN), reduced to the plain shapes the app can draw safely, and kept as one small file in
//! the app's cache folder, where it can be removed again at any time.
//!
//! This module only knows the catalog and the files; fetching is done by the shell.

use crate::error::{CoreError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// A collection that can be installed.
pub struct PackInfo {
    /// The Iconify prefix, which is also the file name.
    pub id: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    /// About how much is downloaded to install it, in bytes (what the CDN sends for the full set).
    pub download: u64,
}

pub const CATALOG: &[PackInfo] = &[
    PackInfo { id: "lucide", label: "Lucide", about: "clean outlines", download: 674_000 },
    PackInfo { id: "feather", label: "Feather", about: "simple outlines, small", download: 72_000 },
    PackInfo { id: "tabler", label: "Tabler", about: "outlines, very many", download: 2_250_000 },
    PackInfo { id: "heroicons", label: "Heroicons", about: "outline and solid", download: 627_000 },
    PackInfo { id: "bi", label: "Bootstrap Icons", about: "outline and solid", download: 1_100_000 },
    PackInfo { id: "mdi", label: "Material Design", about: "Google's icons, very many", download: 3_100_000 },
    PackInfo { id: "fa6-solid", label: "Font Awesome 6 Solid", about: "filled shapes", download: 867_000 },
    PackInfo { id: "fa6-regular", label: "Font Awesome 6 Regular", about: "outlined shapes", download: 108_000 },
    PackInfo { id: "ri", label: "Remix", about: "line and fill", download: 1_070_000 },
    PackInfo { id: "ion", label: "Ionicons", about: "outline, solid and sharp", download: 1_430_000 },
    PackInfo { id: "codicon", label: "Codicons", about: "Visual Studio Code's icons", download: 386_000 },
    PackInfo { id: "ph", label: "Phosphor", about: "many weights, very many", download: 4_570_000 },
    PackInfo { id: "carbon", label: "Carbon", about: "IBM's icons", download: 1_200_000 },
];

/// The most a collection's file may be, so a wrong answer from the network cannot fill the disk.
pub const MAX_DOWNLOAD: u64 = 12 * 1024 * 1024;

pub fn info(id: &str) -> Option<&'static PackInfo> {
    CATALOG.iter().find(|p| p.id == id)
}

fn check(id: &str) -> Result<&'static PackInfo> {
    info(id).ok_or_else(|| CoreError::Invalid(format!("There is no icon collection called {id}.")))
}

fn file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Parts of an icon that this app does not draw (it only takes plain shapes in the text color).
const REFUSED: &[&str] = &["<defs", "<mask", "<use", "<style", "<clipPath", "<image", "<text", "<filter", "Gradient", "<pattern", "<symbol", "<marker", "<foreignObject", "<script", "<animate", "<set", "url(", "&", "javascript:", "href", "style="];

/// An icon body is usable when it holds none of the refused parts.
pub fn body_ok(body: &str) -> bool {
    !body.is_empty() && body.len() <= 6000 && !REFUSED.iter().any(|r| body.contains(r))
}

/// Reduce what the CDN sent (`icons.json` of an Iconify set) to what is kept: the default size and, per icon, its
/// body, as `{"w":24,"h":24,"icons":{"name":"<path…/>"}}` or `"name":["<body>",w,h]` for an icon of its own size.
/// Aliases are left out, as they only repeat another name.
pub fn reduce(raw: &str) -> Result<(String, usize)> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|_| CoreError::Invalid("The icon collection was not in the expected form.".into()))?;
    let icons = v.get("icons").and_then(|i| i.as_object()).ok_or_else(|| CoreError::Invalid("The icon collection holds no icons.".into()))?;
    let dim = |key: &str| v.get(key).and_then(|x| x.as_u64()).unwrap_or(16);
    let (w, h) = (dim("width"), dim("height"));
    let mut out = serde_json::Map::new();
    for (name, icon) in icons {
        let Some(body) = icon.get("body").and_then(|b| b.as_str()) else { continue };
        if !body_ok(body) {
            continue;
        }
        let own = |key: &str, d: u64| icon.get(key).and_then(|x| x.as_u64()).unwrap_or(d);
        let (iw, ih) = (own("width", w), own("height", h));
        if icon.get("hFlip").is_some() || icon.get("vFlip").is_some() || icon.get("rotate").is_some() {
            continue;
        }
        let value = if (iw, ih) == (w, h) { serde_json::Value::String(body.to_string()) } else { serde_json::json!([body, iw, ih]) };
        out.insert(name.clone(), value);
    }
    if out.is_empty() {
        return Err(CoreError::Invalid("None of the icons in that collection can be used.".into()));
    }
    let n = out.len();
    let json = serde_json::json!({ "w": w, "h": h, "icons": out });
    Ok((json.to_string(), n))
}

/// Keep a collection (already reduced) in `dir`, replacing an earlier copy.
pub fn store(dir: &Path, id: &str, reduced: &str) -> Result<()> {
    check(id)?;
    std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let target = file(dir, id);
    let tmp = dir.join(format!("{id}.json.part"));
    std::fs::write(&tmp, reduced).map_err(|e| CoreError::io(&tmp, e))?;
    std::fs::rename(&tmp, &target).map_err(|e| CoreError::io(&target, e))?;
    Ok(())
}

/// What an installed collection holds, as stored.
pub fn read(dir: &Path, id: &str) -> Result<String> {
    check(id)?;
    std::fs::read_to_string(file(dir, id)).map_err(|_| CoreError::Invalid("That icon collection is not installed.".into()))
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PackState {
    pub id: String,
    pub label: String,
    pub about: String,
    /// About how much installing downloads.
    pub download: u64,
    pub installed: bool,
    /// What it takes on disk now (0 when not installed).
    pub disk: u64,
}

/// Every collection that can be installed, with whether it is and how much space it takes.
pub fn states(dir: &Path) -> Vec<PackState> {
    CATALOG
        .iter()
        .map(|p| {
            let disk = std::fs::metadata(file(dir, p.id)).map(|m| m.len()).unwrap_or(0);
            PackState { id: p.id.into(), label: p.label.into(), about: p.about.into(), download: p.download, installed: disk > 0, disk }
        })
        .collect()
}

/// Remove one collection. Not being installed is not an error.
pub fn remove(dir: &Path, id: &str) -> Result<()> {
    check(id)?;
    match std::fs::remove_file(file(dir, id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(CoreError::io(file(dir, id), e)),
    }
}

/// Remove every collection (and any half-finished download).
pub fn clear(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Bytes and number of files in use.
pub fn footprint(dir: &Path) -> (u64, u64) {
    let mut bytes = 0;
    let mut files = 0;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    bytes += m.len();
                    files += 1;
                }
            }
        }
    }
    (bytes, files)
}

/// Names (in order) from the collection listing the Iconify API gives: icons sit either in `uncategorized`
/// or under `categories`.
pub fn names_in_listing(listing: &serde_json::Value) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut take = |v: Option<&serde_json::Value>| {
        if let Some(a) = v.and_then(|x| x.as_array()) {
            names.extend(a.iter().filter_map(|s| s.as_str().map(String::from)));
        }
    };
    take(listing.get("uncategorized"));
    if let Some(c) = listing.get("categories").and_then(|c| c.as_object()) {
        for v in c.values() {
            take(Some(v));
        }
    }
    names.sort();
    names.dedup();
    names.retain(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"prefix":"x","width":24,"height":24,"icons":{
        "a":{"body":"<path d=\"M1 1h2\"/>"},
        "b":{"body":"<path d=\"M1 1\"/>","width":32,"height":32},
        "c":{"body":"<defs><linearGradient id=\"g\"/></defs><path fill=\"url(#g)\" d=\"M0 0\"/>"},
        "d":{"body":"<path d=\"M1\"/>","hFlip":true}},
        "aliases":{"e":{"parent":"a"}}}"#;

    #[test]
    fn reduce_keeps_plain_shapes_only() {
        let (json, n) = reduce(SAMPLE).unwrap();
        assert_eq!(n, 2);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["w"], 24);
        assert!(v["icons"]["a"].is_string());
        assert_eq!(v["icons"]["b"][1], 32);
        assert!(v["icons"].get("c").is_none());
        assert!(v["icons"].get("d").is_none());
        assert!(v["icons"].get("e").is_none());
    }

    #[test]
    fn reduce_refuses_nonsense() {
        assert!(reduce("not json").is_err());
        assert!(reduce(r#"{"icons":{}}"#).is_err());
        assert!(reduce(r#"{"icons":{"a":{"body":"<script/>"}}}"#).is_err());
    }

    #[test]
    fn store_read_remove_and_clear() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("packs");
        assert!(states(&dir).iter().all(|s| !s.installed));
        let (json, _) = reduce(SAMPLE).unwrap();
        store(&dir, "lucide", &json).unwrap();
        let s = states(&dir);
        let l = s.iter().find(|s| s.id == "lucide").unwrap();
        assert!(l.installed && l.disk == json.len() as u64);
        assert_eq!(read(&dir, "lucide").unwrap(), json);
        assert_eq!(footprint(&dir), (json.len() as u64, 1));
        remove(&dir, "lucide").unwrap();
        remove(&dir, "lucide").unwrap();
        assert!(read(&dir, "lucide").is_err());
        store(&dir, "feather", &json).unwrap();
        clear(&dir);
        assert_eq!(footprint(&dir), (0, 0));
    }

    #[test]
    fn unknown_collections_and_paths_are_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(store(d.path(), "../evil", "{}").is_err());
        assert!(read(d.path(), "nope").is_err());
        assert!(remove(d.path(), "a/b").is_err());
    }

    #[test]
    fn listing_names() {
        let v = serde_json::json!({"uncategorized":["b","a"],"categories":{"X":["c","a"]}});
        assert_eq!(names_in_listing(&v), vec!["a", "b", "c"]);
        let v = serde_json::json!({"uncategorized":["ok","bad name","x,y"]});
        assert_eq!(names_in_listing(&v), vec!["ok"]);
    }
}
