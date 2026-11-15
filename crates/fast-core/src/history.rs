//! Transfer history: a compact log of how past transfers went, used to predict
//! how similar ones will go.
//!
//! Every finished transfer adds one fixed-size [`Record`] (about 70 bytes
//! on disk: the drives involved, how many files and bytes, and the speeds and
//! per-file / per-megabyte costs that were measured). Records are kept up to a
//! user-set limit, oldest dropped first, and the file is rewritten after each
//! transfer, so it stays small and always current.
//!
//! To predict a new transfer, [`History::predict`] finds the past transfers
//! whose *profile* is closest: same route (source drive to destination drive)
//! first, then the same kinds of media; similar average file size; similar
//! file count; recent ones counting more. Their measured costs are blended into
//! starting parameters for the time estimate, and their durations, rescaled to
//! the new job's size, give a "typical time".

use crate::drives::Media;
use crate::transfer::engine::{Op, Phase, Progress};
use crate::transfer::eta::EtaParams;
use crate::transfer::method::Method;
use crate::transfer::strategy::Strategy;
use crate::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::Path;

const MB: f64 = 1024.0 * 1024.0;
const FORMAT_VERSION: u32 = 2;

/// Records at least this similar (distance below this) can inform a prediction.
const MAX_DISTANCE: f64 = 3.2;
/// Jobs smaller than this always use the built-in engine.
const SMALL_JOB: u64 = 64 << 20;
/// Only jobs this big are used to try a method that has no results yet.
const EXPLORE_JOB: u64 = 256 << 20;
/// Past jobs this alike (distance below this) count towards choosing a method.
const CHOICE_DISTANCE: f64 = 2.2;
/// Results needed before a method is trusted.
const MIN_SAMPLES: u32 = 2;
/// How many nearest records are blended.
const NEIGHBOURS: usize = 8;
/// A record's weight halves every this many days.
const HALF_LIFE_DAYS: f64 = 45.0;

/// Where a transfer came from and went to, as the engine sees it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    /// Mount point of the source volume (`C:\`, `/mnt/nas`), empty if unknown.
    pub src: String,
    pub dst: String,
    pub src_media: Media,
    pub dst_media: Media,
    /// Human-readable names for display.
    pub src_name: String,
    pub dst_name: String,
}

/// The shape of a job about to run.
#[derive(Clone, Debug)]
pub struct Query {
    pub route: Route,
    pub files: u64,
    pub bytes: u64,
    pub is_move: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    pub params: EtaParams,
    /// Expected wall-clock seconds for this job, from similar past transfers.
    pub typical_secs: f64,
    /// How many past transfers it is based on.
    pub samples: u32,
    /// True when at least one of them used this exact source and destination.
    pub same_route: bool,
}

fn media_code(m: Media) -> u8 {
    match m {
        Media::Ssd => 0,
        Media::Hdd => 1,
        Media::Network => 2,
        Media::Removable => 3,
        Media::Unknown => 4,
    }
}

fn media_from(c: u8) -> Media {
    match c {
        0 => Media::Ssd,
        1 => Media::Hdd,
        2 => Media::Network,
        3 => Media::Removable,
        _ => Media::Unknown,
    }
}

fn strategy_code(s: Strategy) -> u8 {
    match s {
        Strategy::Rename => 0,
        Strategy::LargeSequential => 1,
        Strategy::SmallParallel => 2,
        Strategy::Mixed => 3,
    }
}

/// One finished transfer. Fixed size; drive names live in a shared table.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Record {
    /// Unix seconds when it finished.
    pub at: i64,
    src: u16,
    dst: u16,
    src_media: u8,
    dst_media: u8,
    strategy: u8,
    is_move: bool,
    pub files: u32,
    pub bytes: u64,
    pub secs: f32,
    pub avg_bytes_per_sec: f32,
    pub avg_files_per_sec: f32,
    pub peak_bytes_per_sec: f32,
    /// Fitted cost model of the run (0 when it could not be fitted).
    pub per_file_secs: f32,
    pub per_mb_secs: f32,
    pub overhead_share: f32,
    pub workers: u8,
    pub errors: u16,
    /// `Method::code()`.
    pub method: u8,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    version: u32,
    names: Vec<String>,
    records: Vec<Record>,
}

#[derive(Default)]
pub struct History {
    names: Vec<String>,
    records: VecDeque<Record>,
}

/// What the settings screen shows.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryInfo {
    pub records: usize,
    /// Size the history takes on disk.
    pub bytes: u64,
    /// Distinct source-to-destination routes seen.
    pub routes: usize,
}

impl History {
    pub fn load(file: &Path) -> History {
        let Ok(bytes) = std::fs::read(file) else { return History::default() };
        let Ok(stored) = bincode::deserialize::<Stored>(&bytes) else { return History::default() };
        if stored.version != FORMAT_VERSION {
            return History::default();
        }
        let names_ok = |r: &Record| (r.src as usize) < stored.names.len() && (r.dst as usize) < stored.names.len();
        History {
            records: stored.records.iter().copied().filter(names_ok).collect(),
            names: stored.names,
        }
    }

    pub fn save(&self, file: &Path) -> Result<()> {
        // Only keep names some record still refers to.
        let mut used: Vec<u16> = self.records.iter().flat_map(|r| [r.src, r.dst]).collect();
        used.sort_unstable();
        used.dedup();
        let remap = |old: u16| used.binary_search(&old).unwrap_or(0) as u16;
        let stored = Stored {
            version: FORMAT_VERSION,
            names: used.iter().map(|&i| self.names[i as usize].clone()).collect(),
            records: self.records.iter().map(|r| Record { src: remap(r.src), dst: remap(r.dst), ..*r }).collect(),
        };
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let bytes = bincode::serialize(&stored).map_err(|e| CoreError::Invalid(e.to_string()))?;
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|e| CoreError::io(&tmp, e))?;
        std::fs::rename(&tmp, file).map_err(|e| CoreError::io(file, e))
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.names.clear();
    }

    pub fn info(&self, file: &Path) -> HistoryInfo {
        let mut routes: Vec<(u16, u16)> = self.records.iter().map(|r| (r.src, r.dst)).collect();
        routes.sort_unstable();
        routes.dedup();
        HistoryInfo {
            records: self.records.len(),
            bytes: std::fs::metadata(file).map(|m| m.len()).unwrap_or(0),
            routes: routes.len(),
        }
    }

    fn name_id(&mut self, name: &str) -> u16 {
        if let Some(i) = self.names.iter().position(|n| n == name) {
            return i as u16;
        }
        if self.names.len() >= u16::MAX as usize {
            return 0;
        }
        self.names.push(name.to_string());
        (self.names.len() - 1) as u16
    }

    /// Keep at most `limit` records, dropping the oldest.
    pub fn trim(&mut self, limit: usize) {
        while self.records.len() > limit {
            self.records.pop_front();
        }
    }

    /// Add a finished transfer. Transfers that teach nothing (cancelled,
    /// failed, instant renames, too short to time) are ignored.
    /// Returns whether a record was stored.
    pub fn record(&mut self, p: &Progress, now: i64, limit: usize) -> bool {
        if limit == 0 {
            return false;
        }
        let Some(route) = &p.route else { return false };
        let Some(plan) = &p.plan else { return false };
        if p.phase != Phase::Done || plan.strategy == Strategy::Rename || p.files_done == 0 || p.elapsed_secs < 0.2 {
            return false;
        }
        let src = self.name_id(&route.src);
        let dst = self.name_id(&route.dst);
        let (a, b) = p.learned.map(|l| (l.per_file_secs, l.per_mb_secs)).unwrap_or((0.0, 0.0));
        self.records.push_back(Record {
            at: now,
            src,
            dst,
            src_media: media_code(route.src_media),
            dst_media: media_code(route.dst_media),
            strategy: strategy_code(plan.strategy),
            is_move: p.op == Op::Move,
            files: p.files_done.min(u32::MAX as u64) as u32,
            bytes: p.bytes_done,
            secs: p.elapsed_secs as f32,
            avg_bytes_per_sec: p.avg_bytes_per_sec as f32,
            avg_files_per_sec: p.avg_files_per_sec as f32,
            peak_bytes_per_sec: p.peak_bytes_per_sec as f32,
            per_file_secs: a as f32,
            per_mb_secs: b as f32,
            overhead_share: p.overhead_share as f32,
            workers: (plan.small_workers + plan.large_workers).min(255) as u8,
            errors: p.error_count.min(u16::MAX as u64) as u16,
            method: p.method.map(Method::code).unwrap_or(0),
        });
        self.trim(limit);
        true
    }

    /// Add a run that did not come from the transfer engine (an automated task that copied or
    /// moved files) so that the time estimates for ordinary transfers learn from it too.
    pub fn record_external(&mut self, route: &Route, files: u64, bytes: u64, secs: f64, errors: u64, now: i64, limit: usize) -> bool {
        if limit == 0 || files == 0 || bytes == 0 || secs < 0.2 {
            return false;
        }
        let src = self.name_id(&route.src);
        let dst = self.name_id(&route.dst);
        self.records.push_back(Record {
            at: now,
            src,
            dst,
            src_media: media_code(route.src_media),
            dst_media: media_code(route.dst_media),
            strategy: strategy_code(Strategy::Mixed),
            is_move: false,
            files: files.min(u32::MAX as u64) as u32,
            bytes,
            secs: secs as f32,
            avg_bytes_per_sec: (bytes as f64 / secs) as f32,
            avg_files_per_sec: (files as f64 / secs) as f32,
            peak_bytes_per_sec: (bytes as f64 / secs) as f32,
            per_file_secs: 0.0,
            per_mb_secs: 0.0,
            overhead_share: 0.0,
            workers: 1,
            errors: errors.min(u16::MAX as u64) as u16,
            method: 0,
        });
        self.trim(limit);
        true
    }

    /// How alike a past record and a new job are. 0 is identical; each unit is
    /// roughly "a factor of e" in file size or count, or a route mismatch.
    fn distance(&self, r: &Record, q: &Query) -> f64 {
        let route = if self.names.get(r.src as usize) == Some(&q.route.src)
            && self.names.get(r.dst as usize) == Some(&q.route.dst)
            && !q.route.src.is_empty()
        {
            0.0
        } else if media_from(r.src_media) == q.route.src_media && media_from(r.dst_media) == q.route.dst_media {
            1.2
        } else {
            // Different kinds of devices say little about this one.
            f64::INFINITY
        };
        let avg = |bytes: f64, files: f64| (bytes / files.max(1.0)).max(1.0).ln();
        let size = (avg(r.bytes as f64, r.files as f64) - avg(q.bytes as f64, q.files as f64)).abs();
        let count = ((r.files.max(1) as f64).ln() - (q.files.max(1) as f64).ln()).abs() * 0.5;
        let op = if r.is_move == q.is_move { 0.0 } else { 0.3 };
        route + size * 0.8 + count + op
    }

    /// Pick how to move this job from how earlier similar jobs went.
    ///
    /// * Small jobs and jobs with no history use the built-in engine: starting
    ///   a tool costs more than it could save.
    /// * Each method is scored by the speed it reached on similar jobs (bytes
    ///   per second for big files, files per second for small ones), newer and
    ///   more alike counting more. A method needs [`MIN_SAMPLES`] such jobs.
    /// * With every candidate measured, the fastest wins, but an alternative
    ///   must beat the built-in engine by 10% so noise does not cause flip-flopping.
    /// * With `explore`, a big job is allowed to try a candidate that has too
    ///   few measurements, once the built-in engine has enough of its own.
    pub fn choose_method(&self, q: &Query, candidates: &[Method], explore: bool, now: i64) -> Method {
        if candidates.len() < 2 || q.bytes < SMALL_JOB || q.is_move {
            return Method::Native;
        }
        let by_bytes = q.bytes / q.files.max(1) >= 1 << 20;
        // (weight sum, weighted speed sum, sample count) per method.
        let mut stats: std::collections::HashMap<Method, (f64, f64, u32)> = Default::default();
        for r in self.records.iter().filter(|r| r.strategy != 0 && r.errors == 0 && r.secs >= 0.5) {
            let d = self.distance(r, q);
            if d >= CHOICE_DISTANCE {
                continue;
            }
            let age_days = ((now - r.at).max(0) as f64) / 86_400.0;
            let w = (-d).exp() * 0.5f64.powf(age_days / HALF_LIFE_DAYS);
            let speed = if by_bytes { r.bytes as f64 / r.secs as f64 } else { r.files as f64 / r.secs as f64 };
            let e = stats.entry(Method::from_code(r.method)).or_default();
            *e = (e.0 + w, e.1 + w * speed, e.2 + 1);
        }
        let score = |m: Method| stats.get(&m).filter(|s| s.2 >= MIN_SAMPLES && s.0 > 0.0).map(|s| s.1 / s.0);
        let Some(native) = score(Method::Native) else { return Method::Native };
        let unmeasured = candidates.iter().copied().filter(|m| score(*m).is_none());
        if explore && q.bytes >= EXPLORE_JOB {
            if let Some(m) = unmeasured.min_by_key(|m| stats.get(m).map(|s| s.2).unwrap_or(0)) {
                return m;
            }
        }
        candidates
            .iter()
            .copied()
            .filter(|m| *m != Method::Native)
            .filter_map(|m| score(m).map(|s| (m, s)))
            .filter(|(_, s)| *s > native * 1.10)
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(m, _)| m)
            .unwrap_or(Method::Native)
    }

    /// Predict a job from the past ones most like it, if any are alike enough.
    pub fn predict(&self, q: &Query, now: i64) -> Option<Prediction> {
        if q.files == 0 {
            return None;
        }
        let mut near: Vec<(f64, &Record)> = self
            .records
            .iter()
            .filter(|r| r.strategy != 0 && r.secs > 0.0)
            .map(|r| (self.distance(r, q), r))
            .filter(|(d, _)| *d < MAX_DISTANCE)
            .collect();
        if near.is_empty() {
            return None;
        }
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(NEIGHBOURS);

        let (mut wsum, mut a_sum, mut b_sum, mut t_sum, mut fitted) = (0.0, 0.0, 0.0, 0.0, 0.0);
        let q_mb = q.bytes as f64 / MB;
        for (d, r) in &near {
            let age_days = ((now - r.at).max(0) as f64) / 86_400.0;
            let w = (-d).exp() * 0.5f64.powf(age_days / HALF_LIFE_DAYS);
            let (a, b) = (r.per_file_secs as f64, r.per_mb_secs as f64);
            let r_mb = r.bytes as f64 / MB;
            let (cost_r, cost_q) = (r.files as f64 * a + r_mb * b, q.files as f64 * a + q_mb * b);
            // Rescale this record's real duration to the new job's size using
            // its own fitted costs; without a fit, scale by bytes (or files).
            let scaled = if cost_r > 0.0 {
                r.secs as f64 * cost_q / cost_r
            } else if r.bytes > 0 && q.bytes > 0 {
                r.secs as f64 * q.bytes as f64 / r.bytes as f64
            } else {
                r.secs as f64 * q.files as f64 / r.files.max(1) as f64
            };
            wsum += w;
            t_sum += w * scaled;
            if cost_r > 0.0 {
                a_sum += w * a;
                b_sum += w * b;
                fitted += w;
            }
        }
        if wsum <= 0.0 {
            return None;
        }
        let params = if fitted > 0.0 {
            EtaParams { per_file_secs: a_sum / fitted, per_mb_secs: b_sum / fitted }
        } else {
            // No fitted costs: derive a rough pair from the average speeds.
            let (r_bps, r_fps) = near.iter().fold((0.0, 0.0), |acc, (_, r)| {
                (acc.0 + r.avg_bytes_per_sec as f64, acc.1 + r.avg_files_per_sec as f64)
            });
            let n = near.len() as f64;
            EtaParams {
                per_file_secs: if r_fps > 0.0 { n / r_fps } else { 0.0 },
                per_mb_secs: if r_bps > 0.0 { MB * n / r_bps } else { 0.0 },
            }
        };
        Some(Prediction {
            params,
            typical_secs: t_sum / wsum,
            samples: near.len() as u32,
            same_route: near.iter().any(|(d, _)| *d < 1.0 && !q.route.src.is_empty()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::engine::{TransferError, Progress};
    use crate::transfer::eta::DisplayMode;
    use crate::transfer::strategy::Plan;

    fn route(src: &str, dst: &str, sm: Media, dm: Media) -> Route {
        Route { src: src.into(), dst: dst.into(), src_media: sm, dst_media: dm, src_name: src.into(), dst_name: dst.into() }
    }

    fn done(route: Route, files: u64, bytes: u64, secs: f64, a: f64, b: f64) -> Progress {
        Progress {
            id: 1,
            op: Op::Copy,
            phase: Phase::Done,
            strategy: Some(Strategy::SmallParallel),
            dest_dir: String::new(),
            files_total: files,
            files_done: files,
            bytes_total: bytes,
            bytes_done: bytes,
            skipped: 0,
            bytes_per_sec: 0.0,
            files_per_sec: 0.0,
            eta_secs: None,
            eta_confidence: 0.0,
            display: DisplayMode::Files,
            overhead_share: 0.5,
            current: String::new(),
            errors: Vec::<TransferError>::new(),
            error_count: 0,
            elapsed_secs: secs,
            learned: Some(EtaParams { per_file_secs: a, per_mb_secs: b }),
            fraction: 1.0,
            avg_bytes_per_sec: bytes as f64 / secs,
            avg_files_per_sec: files as f64 / secs,
            peak_bytes_per_sec: bytes as f64 / secs * 1.5,
            peak_files_per_sec: files as f64 / secs * 1.5,
            route: Some(route),
            plan: Some(Plan {
                strategy: Strategy::SmallParallel,
                small_workers: 8,
                large_workers: 1,
                buffer_bytes: 1 << 20,
                large_threshold: 32 << 20,
                try_clone: false,
            }),
            prediction: None,
            conflict: None,
            method: Some(Method::Native),
        }
    }

    const NOW: i64 = 1_700_000_000;

    #[test]
    fn records_only_transfers_that_teach_something() {
        let mut h = History::default();
        let r = route("C:\\", "U:\\", Media::Ssd, Media::Network);
        assert!(h.record(&done(r.clone(), 100, 10_000_000, 3.0, 0.01, 0.01), NOW, 50));
        let mut cancelled = done(r.clone(), 100, 10_000_000, 3.0, 0.01, 0.01);
        cancelled.phase = Phase::Cancelled;
        assert!(!h.record(&cancelled, NOW, 50));
        let mut instant = done(r.clone(), 100, 0, 3.0, 0.01, 0.01);
        instant.plan.as_mut().unwrap().strategy = Strategy::Rename;
        assert!(!h.record(&instant, NOW, 50));
        assert!(!h.record(&done(r.clone(), 100, 10_000_000, 0.05, 0.01, 0.01), NOW, 50), "too short to time");
        assert!(!h.record(&done(r, 100, 10_000_000, 3.0, 0.01, 0.01), NOW, 0), "limit 0 stores nothing");
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn limit_drops_oldest_first() {
        let mut h = History::default();
        let r = route("C:\\", "D:\\", Media::Ssd, Media::Ssd);
        for i in 0..10 {
            h.record(&done(r.clone(), 10 + i, 1_000_000, 1.0, 0.01, 0.01), NOW + i as i64, 4);
        }
        assert_eq!(h.len(), 4);
        assert_eq!(h.records.front().unwrap().files, 16);
        h.trim(2);
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn predicts_from_similar_transfers_not_dissimilar_ones() {
        let mut h = History::default();
        let nas = route("C:\\", "U:\\", Media::Ssd, Media::Network);
        // Many small files to the NAS: 5 ms of overhead each, slow data.
        for i in 0..4 {
            h.record(&done(nas.clone(), 2000 + i * 100, 40_000_000, 12.0, 0.005, 0.03), NOW, 100);
        }
        // Huge files to the NAS: negligible overhead, fast data.
        for _ in 0..4 {
            h.record(&done(nas.clone(), 2, 8_000_000_000, 70.0, 0.05, 0.008), NOW, 100);
        }
        let small_job = Query { route: nas.clone(), files: 3000, bytes: 60_000_000, is_move: false };
        let p = h.predict(&small_job, NOW).unwrap();
        assert!(p.same_route);
        assert!(p.params.per_file_secs > 0.004 && p.params.per_mb_secs > 0.02, "learned the small-file profile: {p:?}");
        // 3000 files at ~6 ms each over ~8 workers... close to the recorded 12 s scaled by 1.5.
        assert!((p.typical_secs - 18.0).abs() < 4.0, "typical {}", p.typical_secs);

        let big_job = Query { route: nas.clone(), files: 2, bytes: 4_000_000_000, is_move: false };
        let p = h.predict(&big_job, NOW).unwrap();
        assert!(p.params.per_mb_secs < 0.012, "big-file profile: {p:?}");
        assert!((p.typical_secs - 35.0).abs() < 8.0, "typical {}", p.typical_secs);
    }

    #[test]
    fn falls_back_to_same_media_and_refuses_unrelated_routes() {
        let mut h = History::default();
        h.record(&done(route("C:\\", "U:\\", Media::Ssd, Media::Network), 500, 50_000_000, 5.0, 0.01, 0.02), NOW, 100);
        // A different NAS drive letter, same kind of devices: usable, but not "same route".
        let other = Query { route: route("C:\\", "V:\\", Media::Ssd, Media::Network), files: 500, bytes: 50_000_000, is_move: false };
        let p = h.predict(&other, NOW).unwrap();
        assert!(!p.same_route);
        // SSD to HDD says nothing about SSD to network.
        let unrelated = Query { route: route("C:\\", "E:\\", Media::Ssd, Media::Hdd), files: 500, bytes: 50_000_000, is_move: false };
        assert!(h.predict(&unrelated, NOW).is_none());
        // Nothing alike in size either.
        let huge = Query { route: route("C:\\", "U:\\", Media::Ssd, Media::Network), files: 2, bytes: 50_000_000_000, is_move: false };
        assert!(h.predict(&huge, NOW).is_none());
        assert!(History::default().predict(&other, NOW).is_none());
    }

    #[test]
    fn recent_transfers_outweigh_old_ones() {
        let mut h = History::default();
        let r = route("C:\\", "D:\\", Media::Ssd, Media::Ssd);
        let old = NOW - 400 * 86_400;
        h.record(&done(r.clone(), 1000, 100_000_000, 10.0, 0.02, 0.01), old, 100);
        h.record(&done(r.clone(), 1000, 100_000_000, 2.0, 0.004, 0.002), NOW, 100);
        let q = Query { route: r, files: 1000, bytes: 100_000_000, is_move: false };
        let p = h.predict(&q, NOW).unwrap();
        assert!(p.typical_secs < 2.5, "recent run dominates: {}", p.typical_secs);
    }

    #[test]
    fn saves_compactly_and_round_trips() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("history.bin");
        let mut h = History::default();
        let nas = route("C:\\", "U:\\", Media::Ssd, Media::Network);
        let usb = route("C:\\", "F:\\", Media::Ssd, Media::Removable);
        for i in 0..200 {
            let r = if i % 2 == 0 { nas.clone() } else { usb.clone() };
            h.record(&done(r, 50 + i, 5_000_000, 2.0, 0.01, 0.01), NOW + i as i64, 500);
        }
        h.save(&file).unwrap();
        let size = std::fs::metadata(&file).unwrap().len();
        assert!(size < 200 * 90, "200 records take {size} bytes");
        let again = History::load(&file);
        assert_eq!(again.len(), 200);
        let q = Query { route: nas, files: 100, bytes: 5_000_000, is_move: false };
        assert_eq!(h.predict(&q, NOW + 300), again.predict(&q, NOW + 300));
        let info = again.info(&file);
        assert_eq!((info.records, info.routes), (200, 2));
        assert_eq!(info.bytes, size);
    }

    #[test]
    fn trimming_then_saving_drops_unused_drive_names() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("history.bin");
        let mut h = History::default();
        h.record(&done(route("C:\\", "U:\\", Media::Ssd, Media::Network), 50, 5_000_000, 2.0, 0.01, 0.01), NOW, 10);
        h.record(&done(route("C:\\", "D:\\", Media::Ssd, Media::Ssd), 50, 5_000_000, 2.0, 0.01, 0.01), NOW + 1, 10);
        h.trim(1);
        h.save(&file).unwrap();
        let again = History::load(&file);
        assert_eq!(again.names, vec!["C:\\".to_string(), "D:\\".to_string()]);
        let q = Query { route: route("C:\\", "D:\\", Media::Ssd, Media::Ssd), files: 50, bytes: 5_000_000, is_move: false };
        assert!(again.predict(&q, NOW + 2).unwrap().same_route);
    }

    #[test]
    fn damaged_or_foreign_files_load_as_empty() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("history.bin");
        assert!(History::load(&file).is_empty());
        std::fs::write(&file, b"garbage that is not bincode").unwrap();
        assert!(History::load(&file).is_empty());
    }

    fn with_method(mut p: Progress, m: Method) -> Progress {
        p.method = Some(m);
        p
    }

    #[test]
    fn chooses_the_method_that_was_faster_on_similar_jobs() {
        let r = route("C:\\", "Z:\\", Media::Ssd, Media::Network);
        let q = Query { route: r.clone(), files: 100, bytes: 4 << 30, is_move: false };
        let both = [Method::Native, Method::Robocopy];
        let mut h = History::default();
        // No history: built-in.
        assert_eq!(h.choose_method(&q, &both, false, NOW), Method::Native);
        // Two built-in runs at 100 MB/s.
        for _ in 0..2 {
            h.record(&with_method(done(r.clone(), 100, 4 << 30, 40.0, 0.0, 0.0), Method::Native), NOW, 100);
        }
        // Without exploring, nothing else is tried.
        assert_eq!(h.choose_method(&q, &both, false, NOW), Method::Native);
        // With exploring, a big job tries the unmeasured one.
        assert_eq!(h.choose_method(&q, &both, true, NOW), Method::Robocopy);
        // A small job never gambles.
        let small = Query { bytes: 10 << 20, ..q.clone() };
        assert_eq!(h.choose_method(&small, &both, true, NOW), Method::Native);
        // Robocopy measured twice and clearly faster: it wins.
        for _ in 0..2 {
            h.record(&with_method(done(r.clone(), 100, 4 << 30, 25.0, 0.0, 0.0), Method::Robocopy), NOW, 100);
        }
        assert_eq!(h.choose_method(&q, &both, true, NOW), Method::Robocopy);
        // Robocopy only 5% faster than built-in is within noise: stay put.
        let mut h2 = History::default();
        for _ in 0..2 {
            h2.record(&with_method(done(r.clone(), 100, 4 << 30, 40.0, 0.0, 0.0), Method::Native), NOW, 100);
            h2.record(&with_method(done(r.clone(), 100, 4 << 30, 38.5, 0.0, 0.0), Method::Robocopy), NOW, 100);
        }
        assert_eq!(h2.choose_method(&q, &both, true, NOW), Method::Native);
        // A different route says nothing about this one.
        let other = Query { route: route("C:\\", "E:\\", Media::Ssd, Media::Hdd), ..q };
        assert_eq!(h.choose_method(&other, &both, true, NOW), Method::Native);
    }

    #[test]
    fn runs_of_automated_tasks_teach_the_estimates() {
        let mut h = History::default();
        let r = route("C:\\", "D:\\", Media::Ssd, Media::Hdd);
        assert!(!h.record_external(&r, 0, 100, 5.0, 0, 1, 10), "nothing moved, nothing learned");
        assert!(!h.record_external(&r, 5, 100, 0.05, 0, 1, 10), "too quick to time");
        assert!(h.record_external(&r, 50, 500_000_000, 10.0, 0, 1_000, 10));
        assert!(h.record_external(&r, 50, 500_000_000, 10.0, 0, 2_000, 10));
        let q = Query { route: r, files: 50, bytes: 500_000_000, is_move: false };
        let p = h.predict(&q, 3_000).expect("a prediction from task runs");
        assert!(p.same_route && p.samples >= 2);
        assert!((p.typical_secs - 10.0).abs() < 2.0, "{}", p.typical_secs);
    }
}
