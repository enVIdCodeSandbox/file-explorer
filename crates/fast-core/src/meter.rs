//! Speed, time left and the speed graph for jobs that are not copies (deleting, compressing,
//! unpacking), measured the same way a transfer's are, so the Transfers window shows them alike
//! and what they take teaches the time estimates for the next one.

use crate::drives::{self, Drive, Media};
use crate::history::{Prediction, Query, Route};
use crate::transfer::eta::EtaTracker;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobStats {
    pub items_per_sec: f64,
    pub bytes_per_sec: f64,
    pub eta_secs: Option<f64>,
    /// How far to trust `eta_secs`, 0 (rough guess) to 1 (settled).
    pub eta_confidence: f64,
    /// 0..=1, only ever increasing.
    pub fraction: f64,
    pub avg_items_per_sec: f64,
    pub avg_bytes_per_sec: f64,
    pub peak_items_per_sec: f64,
    pub peak_bytes_per_sec: f64,
    /// What similar earlier jobs took, in seconds.
    pub typical_secs: Option<f64>,
    pub samples: u32,
}

impl JobStats {
    /// The numbers for a job that has just ended.
    pub fn finish(mut self, completed: bool) -> JobStats {
        if completed {
            self.fraction = 1.0;
            self.eta_secs = Some(0.0);
            self.eta_confidence = 1.0;
        }
        self
    }
}

/// Speed is judged over this many recent seconds.
const WINDOW: f64 = 3.0;

pub struct Meter {
    window: VecDeque<(f64, u64, u64)>,
    tracker: EtaTracker,
    last_t: f64,
    fraction: f64,
    peak_items: f64,
    peak_bytes: f64,
    prediction: Option<Prediction>,
}

impl Meter {
    pub fn new(prediction: Option<Prediction>) -> Meter {
        Meter {
            window: VecDeque::new(),
            tracker: EtaTracker::with_head_start(if prediction.is_some() { 0.5 } else { 0.0 }),
            last_t: 0.0,
            fraction: 0.0,
            peak_items: 0.0,
            peak_bytes: 0.0,
            prediction,
        }
    }

    /// Give the job a prediction once it is known (after the items have been counted).
    pub fn set_prediction(&mut self, p: Option<Prediction>) {
        self.prediction = p;
    }

    /// Feed the job's counters `secs` after it began and get the numbers to show.
    pub fn update(&mut self, secs: f64, items_done: u64, items_total: u64, bytes_done: u64, bytes_total: u64) -> JobStats {
        self.window.push_back((secs, items_done, bytes_done));
        while self.window.len() > 2 && self.window.front().is_some_and(|f| secs - f.0 > WINDOW) {
            self.window.pop_front();
        }
        let (items_ps, bytes_ps) = match (self.window.front(), self.window.back()) {
            (Some(a), Some(b)) if b.0 - a.0 > 0.05 => ((b.1.saturating_sub(a.1)) as f64 / (b.0 - a.0), (b.2.saturating_sub(a.2)) as f64 / (b.0 - a.0)),
            _ => (0.0, 0.0),
        };
        self.peak_items = self.peak_items.max(items_ps);
        self.peak_bytes = self.peak_bytes.max(bytes_ps);

        // By bytes when the job knows how many there are, otherwise by items.
        let by_bytes = bytes_total > 0;
        let share = if by_bytes { bytes_done as f64 / bytes_total as f64 } else if items_total > 0 { items_done as f64 / items_total as f64 } else { 0.0 };
        self.fraction = self.fraction.max(share.clamp(0.0, 1.0));

        let measured = if by_bytes && bytes_ps > 0.0 {
            Some(bytes_total.saturating_sub(bytes_done) as f64 / bytes_ps)
        } else if !by_bytes && items_ps > 0.0 && items_total > 0 {
            Some(items_total.saturating_sub(items_done) as f64 / items_ps)
        } else {
            None
        };
        let predicted = self.prediction.map(|p| p.typical_secs * (1.0 - self.fraction));
        // Early on the earlier jobs speak; as measurements accumulate they take over.
        let weight = (secs / 5.0).clamp(0.0, 1.0);
        let raw = match (measured, predicted) {
            (Some(m), Some(p)) => Some(m * weight + p * (1.0 - weight)),
            (Some(m), None) if secs >= 0.5 => Some(m),
            (None, Some(p)) => Some(p),
            _ => None,
        };
        let dt = (secs - self.last_t).max(0.0);
        self.last_t = secs;
        let (eta, confidence) = self.tracker.update(raw, dt, self.fraction, secs);
        let avg = |n: u64| if secs > 0.0 { n as f64 / secs } else { 0.0 };
        JobStats {
            items_per_sec: items_ps,
            bytes_per_sec: bytes_ps,
            eta_secs: eta,
            eta_confidence: confidence,
            fraction: self.fraction,
            avg_items_per_sec: avg(items_done),
            avg_bytes_per_sec: avg(bytes_done),
            peak_items_per_sec: self.peak_items,
            peak_bytes_per_sec: self.peak_bytes,
            typical_secs: self.prediction.map(|p| p.typical_secs),
            samples: self.prediction.map(|p| p.samples).unwrap_or(0),
        }
    }
}

/// How a job to or from a server travels, by the kind of server at each end.
pub fn server_method(places: &[&str]) -> String {
    let mut kinds: Vec<&str> = Vec::new();
    for p in places {
        let scheme = p.split("://").next().filter(|_| p.contains("://")).unwrap_or("").to_ascii_lowercase();
        let k = match scheme.as_str() {
            "sftp" => "SFTP (over SSH)",
            "ftp" => "FTP",
            "dav" | "davs" => "WebDAV",
            "mtp" => "MTP (phone or camera on a cable)",
            _ => continue,
        };
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    if kinds.is_empty() { "Server connection".into() } else { kinds.join(" → ") }
}

/// What reads or writes an archive, by its name.
pub fn archive_method(archive: &str, writing: bool) -> String {
    let n = archive.to_ascii_lowercase();
    let n = n.split('!').next().unwrap_or("");
    let built_in = |what: &str| format!("Built-in {what} {}", if writing { "writer" } else { "reader" });
    if n.ends_with(".zip") {
        built_in("zip")
    } else if [".tar", ".tgz", ".tbz2", ".tbz", ".txz", ".gz", ".bz2", ".xz", ".zst"].iter().any(|e| n.ends_with(e)) {
        built_in("tar and gzip / bzip2 / xz")
    } else if n.ends_with(".7z") {
        built_in("7z")
    } else {
        "7-Zip or bsdtar, whichever is installed".into()
    }
}

/// Where a job of this `kind` ("delete", "compress", "extract") runs, for the history. The kind takes
/// the place of a destination, so deleting is judged against earlier deletes and not against copies.
pub fn route_for(kind: &str, path: &Path, drives: &[Drive]) -> Route {
    let vol = drives::volume_for(path, drives);
    let name = vol.map(|d| d.name.clone()).unwrap_or_default();
    Route { src: vol.map(|d| d.mount.clone()).unwrap_or_default(), dst: format!("<{kind}>"), src_media: drives::media_for(path, drives), dst_media: Media::Unknown, src_name: name.clone(), dst_name: kind.to_string() }
}

/// The question to ask the history about a job that is about to run.
pub fn query_for(route: Route, files: u64, bytes: u64) -> Query {
    // `is_move` is true for every job of these kinds, which keeps copies further away.
    Query { route, files, bytes, is_move: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::transfer::eta::EtaParams;

    #[test]
    fn measures_speed_and_settles_on_a_time_left() {
        let mut m = Meter::new(None);
        let mut last = JobStats::default();
        // 1000 items per second, 100 000 items in all.
        for i in 0..80 {
            let t = i as f64 * 0.1;
            last = m.update(t, (t * 1000.0) as u64, 100_000, 0, 0);
        }
        assert!((last.items_per_sec - 1000.0).abs() < 60.0, "{}", last.items_per_sec);
        let eta = last.eta_secs.unwrap();
        assert!((eta - 92.0).abs() < 10.0, "{eta}");
        assert!(last.eta_confidence > 0.5, "{}", last.eta_confidence);
        assert!(last.fraction > 0.07 && last.fraction < 0.09);
        assert!(last.peak_items_per_sec >= last.items_per_sec);
    }

    #[test]
    fn progress_by_bytes_when_known_and_never_goes_back() {
        let mut m = Meter::new(None);
        let a = m.update(1.0, 10, 100, 50, 100);
        let b = m.update(2.0, 10, 100, 40, 100); // a count that dipped
        assert!(a.fraction >= 0.5 && b.fraction >= a.fraction);
        assert!(a.bytes_per_sec > 0.0 || b.bytes_per_sec >= 0.0);
    }

    #[test]
    fn an_earlier_run_gives_a_first_guess_before_any_speed_is_known() {
        let p = Prediction { params: EtaParams { per_file_secs: 0.001, per_mb_secs: 0.0 }, typical_secs: 30.0, samples: 3, same_route: true, shape_match: 1.0 };
        let mut m = Meter::new(Some(p));
        let s = m.update(0.2, 0, 1000, 0, 0);
        assert_eq!(s.eta_secs.map(|e| e.round()), Some(30.0));
        assert_eq!((s.typical_secs, s.samples), (Some(30.0), 3));
    }

    #[test]
    fn says_how_a_job_travels() {
        assert_eq!(server_method(&["sftp://nas/a", "/home/me"]), "SFTP (over SSH)");
        assert_eq!(server_method(&["/x", "ftp://h/y"]), "FTP");
        assert_eq!(server_method(&["sftp://a/x", "davs://b/y"]), "SFTP (over SSH) → WebDAV");
        assert_eq!(server_method(&["/x", "/y"]), "Server connection");
        assert_eq!(archive_method("/a/b.zip", true), "Built-in zip writer");
        assert_eq!(archive_method("archive:///a/b.tar.gz!/docs", false), "Built-in tar and gzip / bzip2 / xz reader");
        assert!(archive_method("/a/b.rar", false).starts_with("7-Zip"));
    }

    #[test]
    fn deletes_teach_the_estimates_for_deletes_only() {
        let mut h = History::default();
        let route = Route { src: "C:\\".into(), dst: "<delete>".into(), src_media: Media::Ssd, dst_media: Media::Unknown, src_name: "C".into(), dst_name: "delete".into() };
        assert!(h.record_job(&route, true, 20_000, 0, 10.0, 0, 1_000_000, 50), "20 000 files in 10 s");
        assert!(!h.record_job(&route, true, 5, 0, 0.01, 0, 1_000_000, 50), "too short to time");
        let p = h.predict(&query_for(route.clone(), 40_000, 0), 1_000_000).unwrap();
        assert!((p.typical_secs - 20.0).abs() < 2.0, "{}", p.typical_secs);
        // A copy between the same kind of drives is not judged by it.
        let copy = Route { dst: "C:\\".into(), dst_media: Media::Ssd, ..route };
        assert!(h.predict(&Query { route: copy, files: 40_000, bytes: 0, is_move: false }, 1_000_000).is_none());
    }
}
