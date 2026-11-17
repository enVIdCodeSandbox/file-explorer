//! Predicts which folders the user will open next, so their listings can be
//! loaded before the click.
//!
//! Two signals, both decayed over time so old habits fade:
//! * transitions: "after being in A, the user usually opens B"
//! * frecency: how often and how recently a folder was visited at all

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// Scores halve after this many seconds without a visit.
const HALF_LIFE_SECS: f64 = 14.0 * 24.0 * 3600.0;
const MAX_TRANSITIONS_PER_DIR: usize = 32;
const MAX_DIRS: usize = 4096;
/// How many navigations are remembered for the Frequently used list.
const MAX_LOG: usize = 3000;
/// A folder left open for hours counts as this long at most, so one forgotten window does not outweigh a day's work.
const DWELL_CAP_SECS: f32 = 900.0;

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

/// One navigation into a folder.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Visit {
    /// The folder.
    p: String,
    /// Unix seconds.
    at: i64,
    /// Seconds it stayed open (until the next navigation), up to a limit.
    #[serde(default)]
    d: f32,
}

/// A folder put in the Frequently used list by hand: it counts as this many visits, and fades as other navigations
/// pass without it being opened.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Seed {
    count: f64,
    /// The number of navigations made so far when it was added.
    nav: u64,
}

/// How the Frequently used list is worked out.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct FrequentPolicy {
    /// Only this many of the latest navigations count; 0 is all that are remembered.
    pub window: usize,
    /// 0 to 100: how much of the score is time spent open, the rest being how many times it was opened.
    pub time_weight: f64,
    /// A folder must have been opened at least this many times to be listed.
    pub min_visits: f64,
    /// 0 to 100: how much more the newest navigations in the window count than the oldest.
    pub recency_bias: f64,
}

impl Default for FrequentPolicy {
    fn default() -> Self {
        FrequentPolicy { window: 300, time_weight: 25.0, min_visits: 2.0, recency_bias: 30.0 }
    }
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct Predictor {
    transitions: HashMap<String, HashMap<String, Score>>,
    visits: HashMap<String, Score>,
    #[serde(default)]
    log: VecDeque<Visit>,
    #[serde(default)]
    seeds: HashMap<String, Seed>,
    /// Navigations made in all.
    #[serde(default)]
    navs: u64,
    /// Folders taken out of the list by hand; opening one again brings it back.
    #[serde(default)]
    removed: HashSet<String>,
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
        // The folder just left was open until now.
        if let (Some(from), Some(last)) = (from, self.log.back_mut()) {
            if last.p == from {
                last.d = ((now - last.at).max(0) as f32).min(DWELL_CAP_SECS);
            }
        }
        self.log.push_back(Visit { p: to.to_string(), at: now, d: 0.0 });
        while self.log.len() > MAX_LOG {
            self.log.pop_front();
        }
        self.navs += 1;
        self.removed.remove(to);
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

    /// Forget every folder under `root` (visits and transitions in or out of it); how many were visited folders.
    pub fn remove_under(&mut self, root: &std::path::Path) -> usize {
        let rule = root.to_string_lossy().into_owned();
        let inside = |k: &str| crate::cachepolicy::is_under(std::path::Path::new(k), &rule);
        let before = self.visits.len();
        self.visits.retain(|k, _| !inside(k));
        self.transitions.retain(|k, _| !inside(k));
        for row in self.transitions.values_mut() {
            row.retain(|k, _| !inside(k));
        }
        self.transitions.retain(|_, row| !row.is_empty());
        before - self.visits.len()
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

    /// The folders to list as Frequently used, best first (folders that are no longer there are left to the caller).
    pub fn frequent_list(&self, n: usize, pol: &FrequentPolicy) -> Vec<String> {
        let window = if pol.window == 0 { self.log.len() } else { pol.window.min(self.log.len()) };
        let skip = self.log.len() - window;
        let bias = (pol.recency_bias / 100.0).clamp(0.0, 1.0);
        let tw = (pol.time_weight / 100.0).clamp(0.0, 1.0);
        // (opened, opened with the newest counting more, seconds open)
        let mut by: HashMap<&str, (f64, f64, f64)> = HashMap::new();
        for (i, v) in self.log.iter().skip(skip).enumerate() {
            let weight = 1.0 + bias * (i as f64 / window.max(1) as f64);
            let e = by.entry(v.p.as_str()).or_default();
            e.0 += 1.0;
            e.1 += weight;
            e.2 += v.d as f64;
        }
        for (p, seed) in &self.seeds {
            // Fades as navigations pass without it being opened; with no window it stays.
            let fade = if pol.window == 0 { 1.0 } else { (1.0 - self.navs.saturating_sub(seed.nav) as f64 / pol.window as f64).max(0.0) };
            let c = seed.count * fade;
            if c > 0.0 {
                let e = by.entry(p.as_str()).or_default();
                e.0 += c;
                e.1 += c;
            }
        }
        let max_count = by.values().map(|e| e.1).fold(0.0, f64::max).max(1e-9);
        let max_dwell = by.values().map(|e| e.2).fold(0.0, f64::max);
        let mut list: Vec<(&str, f64)> = by
            .into_iter()
            .filter(|(p, e)| e.0 + 1e-9 >= pol.min_visits && !self.removed.contains(*p))
            .map(|(p, e)| {
                let time = if max_dwell > 0.0 { e.2 / max_dwell } else { e.1 / max_count };
                let score = (1.0 - tw) * (e.1 / max_count) + tw * time;
                // Scores that are nearly the same count as the same, so those fall back to the alphabet.
                (p, (score * 100.0).round() / 100.0)
            })
            .collect();
        list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())));
        list.into_iter().take(n).map(|(p, _)| p.to_string()).collect()
    }

    /// Put folders in the list by hand, each counting as `count` visits to begin with.
    pub fn seed(&mut self, paths: &[String], count: f64) {
        for p in paths {
            self.removed.remove(p);
            self.seeds.insert(p.clone(), Seed { count: count.max(0.0), nav: self.navs });
        }
    }

    /// Take a folder out of the list; opening it again brings it back.
    pub fn forget_frequent(&mut self, path: &str) {
        self.seeds.remove(path);
        self.removed.insert(path.to_string());
    }

    pub fn clear(&mut self) {
        self.transitions.clear();
        self.visits.clear();
        self.log.clear();
        self.seeds.clear();
        self.removed.clear();
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

    #[test]
    fn frequently_used_counts_only_the_latest_navigations() {
        let mut p = Predictor::default();
        // An old habit, then a newer one.
        for i in 0..20 {
            p.record(None, "/old", 1000 + i);
        }
        for i in 0..10 {
            p.record(None, "/new", 2000 + i);
        }
        let all = FrequentPolicy { window: 0, time_weight: 0.0, min_visits: 1.0, recency_bias: 0.0 };
        assert_eq!(p.frequent_list(5, &all)[0], "/old");
        let recent = FrequentPolicy { window: 10, ..all };
        assert_eq!(p.frequent_list(5, &recent), vec!["/new"], "what fell out of the window is gone");
    }

    #[test]
    fn time_open_can_outweigh_how_often() {
        let mut p = Predictor::default();
        let mut t = 0;
        let mut at: Option<&str> = None;
        let mut go = |p: &mut Predictor, to: &'static str, wait: i64| {
            p.record(at, to, t);
            at = Some(to);
            t += wait;
        };
        // /quick: opened 6 times for a second each. /long: opened 3 times for ten minutes each.
        for _ in 0..3 {
            go(&mut p, "/quick", 1);
            go(&mut p, "/other", 1);
            go(&mut p, "/quick", 1);
            go(&mut p, "/long", 600);
        }
        go(&mut p, "/end", 1);
        let by_count = FrequentPolicy { window: 0, time_weight: 0.0, min_visits: 1.0, recency_bias: 0.0 };
        let by_time = FrequentPolicy { time_weight: 100.0, ..by_count };
        assert_eq!(p.frequent_list(2, &by_count)[0], "/quick");
        assert_eq!(p.frequent_list(2, &by_time)[0], "/long");
    }

    #[test]
    fn a_folder_needs_enough_visits_to_be_listed() {
        let mut p = Predictor::default();
        p.record(None, "/once", 1);
        p.record(None, "/twice", 2);
        p.record(None, "/twice", 3);
        let pol = FrequentPolicy { min_visits: 2.0, ..Default::default() };
        assert_eq!(p.frequent_list(5, &pol), vec!["/twice"]);
    }

    #[test]
    fn dragged_in_folders_start_with_a_default_and_fade_if_unused() {
        let mut p = Predictor::default();
        p.seed(&["/b".into(), "/a".into()], 3.0);
        let pol = FrequentPolicy { window: 10, time_weight: 0.0, min_visits: 1.0, recency_bias: 0.0 };
        // Equal frequency: alphabetical.
        assert_eq!(p.frequent_list(5, &pol), vec!["/a", "/b"]);
        // Other navigations pass; the seeds fade away unless opened.
        for i in 0..6 {
            p.record(None, "/busy", i);
        }
        p.record(None, "/a", 7);
        let list = p.frequent_list(5, &FrequentPolicy { min_visits: 1.0, ..pol });
        assert_eq!(list[0], "/busy");
        assert!(list.contains(&"/a".to_string()), "opened, so it stays: {list:?}");
        for i in 0..12 {
            p.record(None, "/busy", 10 + i);
        }
        assert!(!p.frequent_list(5, &FrequentPolicy { min_visits: 1.0, ..pol }).contains(&"/b".to_string()), "never opened, so it faded out");
    }

    #[test]
    fn a_removed_folder_returns_when_opened() {
        let mut p = Predictor::default();
        for i in 0..3 {
            p.record(None, "/x", i);
        }
        let pol = FrequentPolicy { min_visits: 1.0, ..Default::default() };
        p.forget_frequent("/x");
        assert!(p.frequent_list(5, &pol).is_empty());
        p.record(None, "/x", 10);
        assert_eq!(p.frequent_list(5, &pol), vec!["/x"]);
    }
}
