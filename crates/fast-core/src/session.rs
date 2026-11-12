//! Long-lived UI state: open tabs and groups, pins, recents and the last
//! known drive list (so "This PC" can paint instantly on the next launch).

use crate::drives::Drive;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tab {
    pub id: String,
    /// A filesystem path or an app location such as `fast://home`.
    pub location: String,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub history: Vec<String>,
    #[serde(default)]
    pub history_index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabGroup {
    pub id: String,
    pub name: String,
    pub color: String,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub path: String,
    pub label: String,
    /// Built-in pins (Downloads, Documents, ...) versus ones the user added.
    #[serde(default)]
    pub builtin: bool,
    #[serde(default)]
    pub is_file: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recent {
    pub path: String,
    pub is_dir: bool,
    /// Milliseconds since the Unix epoch.
    pub at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub tabs: Vec<Tab>,
    pub groups: Vec<TabGroup>,
    pub active_tab: Option<String>,
    /// `None` until first run, when the defaults are seeded.
    pub pins: Option<Vec<Pin>>,
    pub recent: Vec<Recent>,
    pub drives: Vec<Drive>,
}

pub const MAX_RECENT: usize = 100;

impl Session {
    pub fn load(file: &Path) -> Session {
        let mut s: Session = std::fs::read_to_string(file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        if s.pins.is_none() {
            s.pins = Some(default_pins());
        }
        s
    }

    pub fn save(&self, file: &Path) -> crate::Result<()> {
        crate::settings::write_json(file, self)
    }

    /// Keep the session small no matter what the frontend sends.
    pub fn normalize(&mut self) {
        self.recent.truncate(MAX_RECENT);
        for t in &mut self.tabs {
            if t.history.len() > 50 {
                let cut = t.history.len() - 50;
                t.history.drain(..cut);
                t.history_index = t.history_index.saturating_sub(cut);
            }
        }
    }
}

/// The standard user folders that exist on this machine. Unlike Explorer,
/// nothing is ever added here automatically.
pub fn default_pins() -> Vec<Pin> {
    crate::platform::known_folders()
        .into_iter()
        .filter(|(label, _)| *label != "Home")
        .map(|(label, path)| Pin { path, label: label.to_string(), builtin: true, is_file: false })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_seeds_pins_and_roundtrips() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("session.json");
        let mut s = Session::load(&f);
        assert!(s.pins.is_some());
        s.tabs.push(Tab {
            id: "t1".into(), location: "/tmp".into(), group_id: None,
            history: (0..80).map(|i| format!("/p{i}")).collect(), history_index: 79,
        });
        s.pins = Some(vec![]); // user removed every pin: must stay empty
        s.normalize();
        assert_eq!(s.tabs[0].history.len(), 50);
        assert_eq!(s.tabs[0].history_index, 49);
        s.save(&f).unwrap();
        let back = Session::load(&f);
        assert_eq!(back, s);
        assert_eq!(back.pins, Some(vec![]));
    }
}
