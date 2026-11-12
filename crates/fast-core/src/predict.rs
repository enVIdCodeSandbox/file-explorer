//! Predicts which folders the user will open next, so their listings can be
//! loaded before the click.
//!
//! Two signals, both decayed over time so old habits fade:
//! * transitions: "after being in A, the user usually opens B"
//! * frecency: how often and how recently a folder was visited at all

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Scores halve after this many seconds without a visit.
const HALF_LIFE_SECS: f64 = 14.0 * 24.0 * 3600.0;
const MAX_TRANSITIONS_PER_DIR: usize = 32;
const MAX_DIRS: usize = 4096;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
struct Score {
    value: f64,
    /// Unix seconds of the last update.
    at: i64,
}

impl Score {
    fn current(&self, now: i64) -> f64 {
        let age = (now - self.at).max(0) as f64;
        self.value * 0.5f64.powf(age / HALF_LIFE_SECS)
    }
    fn bump(&mut self, now: i64) {
        self.value = self.current(now) + 1.0;
        self.at = now;
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Predictor {
    transitions: HashMap<String, HashMap<String, Score>>,
    visits: HashMap<String, Score>,
}

impl Predictor {
    pub fn load(file: &std::path::Path) -> Predictor {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, file: &std::path::Path) -> crate::Result<()> {
        crate::settings::write_json(file, self)
    }

    /// Record that the user navigated from `from` (if any) to `to`.
    pub fn record(&mut self, from: Option<&str>, to: &str, now: i64) {
        self.visits.entry(to.to_string()).or_insert(Score { value: 0.0, at: now }).bump(now);
        if let Some(from) = from.filter(|f| *f != to) {
            let row = self.transitions.entry(from.to_string()).or_default();
            row.entry(to.to_string()).or_insert(Score { value: 0.0, at: now }).bump(now);
            if row.len() > MAX_TRANSITIONS_PER_DIR {
                prune(row, MAX_TRANSITIONS_PER_DIR / 2, now);
            }
        }
        if self.visits.len() > MAX_DIRS {
            prune(&mut self.visits, MAX_DIRS / 2, now);
            let keep = &self.visits;
            self.transitions.retain(|k, _| keep.contains_key(k));
        }
    }

    /// The `n` folders most likely to be opened next from `dir`.
    pub fn predict(&self, dir: &str, n: usize, now: i64) -> Vec<String> {
        let mut scored: HashMap<&str, f64> = HashMap::new();
        if let Some(row) = self.transitions.get(dir) {
            for (to, s) in row {
                // A direct transition is much stronger evidence than general popularity.
                *scored.entry(to).or_default() += 3.0 * s.current(now);
            }
        }
        // Popular children of this folder, even if never opened from here.
        for (path, s) in &self.visits {
            if is_child(dir, path) {
                *scored.entry(path).or_default() += s.current(now);
            }
        }
        let mut list: Vec<(&str, f64)> = scored.into_iter().filter(|(_, v)| *v > 0.05).collect();
        list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
        list.into_iter().take(n).map(|(p, _)| p.to_string()).collect()
    }

    /// Most visited folders overall (for the Home page's "frequent" list).
    pub fn frequent(&self, n: usize, now: i64) -> Vec<String> {
        let mut list: Vec<(&String, f64)> = self.visits.iter().map(|(p, s)| (p, s.current(now))).collect();
        list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(b.0)));
        list.into_iter().take(n).map(|(p, _)| p.clone()).collect()
    }

    pub fn clear(&mut self) {
        self.transitions.clear();
        self.visits.clear();
    }
}

fn is_child(dir: &str, path: &str) -> bool {
    let Some(rest) = path.strip_prefix(dir) else { return false };
    let rest = if dir.ends_with(['/', '\\']) {
        rest
    } else {
        match rest.strip_prefix(['/', '\\']) {
            Some(r) => r,
            None => return false,
        }
    };
    !rest.is_empty() && !rest.contains(['/', '\\'])
}

fn prune(map: &mut HashMap<String, Score>, keep: usize, now: i64) {
    let mut scores: Vec<f64> = map.values().map(|s| s.current(now)).collect();
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let cutoff = scores.get(keep).copied().unwrap_or(0.0);
    map.retain(|_, s| s.current(now) > cutoff);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_transitions_and_ranks_them_first() {
        let mut p = Predictor::default();
        let now = 1_700_000_000;
        for _ in 0..5 {
            p.record(Some("/home/u"), "/home/u/projects", now);
        }
        p.record(Some("/home/u"), "/home/u/Downloads", now);
        p.record(Some("/elsewhere"), "/home/u/Music", now);
        let top = p.predict("/home/u", 3, now);
        assert_eq!(top[0], "/home/u/projects");
        assert_eq!(top[1], "/home/u/Downloads");
        assert_eq!(top[2], "/home/u/Music"); // popular child, never opened from here
        assert!(p.predict("/nowhere", 3, now).is_empty());
    }

    #[test]
    fn old_habits_fade() {
        let mut p = Predictor::default();
        let t0 = 1_700_000_000;
        for _ in 0..4 {
            p.record(Some("/a"), "/a/old", t0);
        }
        let later = t0 + (HALF_LIFE_SECS as i64) * 4;
        p.record(Some("/a"), "/a/new", later);
        assert_eq!(p.predict("/a", 1, later), vec!["/a/new"]);
    }

    #[test]
    fn child_detection_handles_separators() {
        assert!(is_child("/a", "/a/b"));
        assert!(is_child("C:\\", "C:\\Users"));
        assert!(is_child("C:\\Users", "C:\\Users\\me"));
        assert!(!is_child("/a", "/a/b/c"));
        assert!(!is_child("/a", "/ab"));
        assert!(!is_child("/a", "/a"));
    }

    #[test]
    fn stays_bounded() {
        let mut p = Predictor::default();
        for i in 0..(MAX_DIRS + 500) {
            p.record(Some("/root"), &format!("/root/d{i}"), 1_700_000_000 + i as i64);
        }
        assert!(p.visits.len() <= MAX_DIRS);
        assert!(p.transitions["/root"].len() <= MAX_TRANSITIONS_PER_DIR);
    }
}
