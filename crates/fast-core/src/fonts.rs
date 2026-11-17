//! The fonts installed on this computer, so Settings can offer them by name.

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledFont {
    /// The family name, as CSS knows it.
    pub name: String,
    /// Every face in the family has letters of one width (fit for a terminal).
    pub fixed_width: bool,
}

/// One entry per family, by name (ignoring case), with whether it is fixed-width. Vertical fonts (names starting with
/// "@") and empty names are left out.
pub fn families<I: IntoIterator<Item = (String, bool)>>(faces: I) -> Vec<InstalledFont> {
    let mut by: BTreeMap<String, (String, bool)> = BTreeMap::new();
    for (name, mono) in faces {
        let name = name.trim().to_string();
        if name.is_empty() || name.starts_with('@') {
            continue;
        }
        let e = by.entry(name.to_lowercase()).or_insert((name, true));
        e.1 &= mono;
    }
    by.into_values().map(|(name, fixed_width)| InstalledFont { name, fixed_width }).collect()
}

/// Every installed family. Empty on phones, which cannot list them.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn installed() -> Vec<InstalledFont> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    families(db.faces().filter_map(|f| f.families.first().map(|(n, _)| (n.clone(), f.monospaced))))
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn installed() -> Vec<InstalledFont> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_entry_per_family_sorted_with_fixed_width_only_if_every_face_is() {
        let list = families([
            ("Zed Mono".to_string(), true),
            ("Arial".to_string(), false),
            ("zed mono".to_string(), true),
            ("Mixed".to_string(), true),
            ("Mixed".to_string(), false),
            ("@Vertical".to_string(), false),
            ("  ".to_string(), false),
        ]);
        let names: Vec<_> = list.iter().map(|f| (f.name.as_str(), f.fixed_width)).collect();
        assert_eq!(names, vec![("Arial", false), ("Mixed", false), ("Zed Mono", true)]);
    }

    #[test]
    fn listing_the_real_fonts_does_not_fail() {
        let list = installed();
        assert!(list.windows(2).all(|w| w[0].name.to_lowercase() < w[1].name.to_lowercase()));
    }
}
