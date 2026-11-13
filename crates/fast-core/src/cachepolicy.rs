//! Where saving things is switched off. A list of folders and drives
//! (`advanced["cache.exclude"]` in the settings) says "do not keep anything
//! learned about what is in here": no remembered listings, folder sizes,
//! thumbnails, disk-usage snapshots, search index entries or predictions.
//! A rule covers the folder and everything below it; a drive is just a folder.

use crate::settings::Settings;
use std::path::Path;

pub const KEY: &str = "cache.exclude";

/// A path as it is compared: no trailing separator, and without case on
/// systems whose file names have none.
fn normal(p: &str) -> String {
    let t = p.trim_end_matches(['/', '\\']);
    let t = if t.is_empty() { p } else { t }; // "/" stays "/"
    if cfg!(any(windows, target_os = "macos")) { t.to_lowercase() } else { t.to_string() }
}

/// Is `path` the folder `rule` or inside it?
pub fn is_under(path: &Path, rule: &str) -> bool {
    let (p, r) = (normal(&path.to_string_lossy()), normal(rule));
    if r.is_empty() {
        return false;
    }
    if p == r {
        return true;
    }
    let sep_ok = r.ends_with(['/', '\\']);
    p.starts_with(&r) && (sep_ok || p[r.len()..].starts_with(['/', '\\']))
}

/// The rules saved in the settings (anything that is not text is ignored).
pub fn rules(settings: &Settings) -> Vec<String> {
    settings
        .advanced
        .get(KEY)
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::to_string).filter(|s| !s.trim().is_empty()).collect())
        .unwrap_or_default()
}

/// Does a rule switch caching off for `path`?
pub fn excluded(rules: &[String], path: &Path) -> bool {
    rules.iter().any(|r| is_under(path, r))
}

/// Would saving something about the whole of `root` also save something about
/// an excluded place: `root` is inside a rule, or a rule is inside `root`?
pub fn touches(rules: &[String], root: &Path) -> bool {
    excluded(rules, root) || rules.iter().any(|r| is_under(Path::new(r), &root.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touching_goes_both_ways() {
        let r = vec!["/d/private".to_string()];
        assert!(touches(&r, Path::new("/d/private/x")), "inside a rule");
        assert!(touches(&r, Path::new("/d")), "a rule inside it");
        assert!(!touches(&r, Path::new("/e")));
        assert!(!touches(&r, Path::new("/d/public")));
    }

    #[test]
    fn a_rule_covers_the_folder_and_what_is_below_it() {
        assert!(is_under(Path::new("/data/photos"), "/data/photos"));
        assert!(is_under(Path::new("/data/photos/2024/a.jpg"), "/data/photos"));
        assert!(is_under(Path::new("/data/photos/"), "/data/photos/"));
        assert!(!is_under(Path::new("/data/photos-old"), "/data/photos"), "a sibling with the same start is not inside");
        assert!(!is_under(Path::new("/data"), "/data/photos"), "the parent is not covered");
        assert!(is_under(Path::new("/anything"), "/"), "the root covers everything");
        assert!(!is_under(Path::new("/x"), ""));
    }

    #[test]
    fn drive_roots_cover_their_contents() {
        if cfg!(windows) {
            assert!(is_under(Path::new(r"D:\Games\x"), r"D:\"));
            assert!(is_under(Path::new(r"d:\games"), r"D:"));
            assert!(!is_under(Path::new(r"E:\Games"), r"D:\"));
        }
    }

    #[test]
    fn reads_the_rules_from_settings() {
        let mut s = Settings::default();
        assert!(rules(&s).is_empty());
        s.advanced.insert(KEY.into(), serde_json::json!(["/a", "", 5, "/b"]));
        assert_eq!(rules(&s), vec!["/a".to_string(), "/b".to_string()]);
        assert!(excluded(&rules(&s), Path::new("/b/c")));
        assert!(!excluded(&rules(&s), Path::new("/c")));
    }
}
