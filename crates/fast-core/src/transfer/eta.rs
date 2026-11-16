//! Transfer time estimation.
//!
//! Windows Explorer estimates remaining time from MB/s alone. That is right
//! for a few big files and badly wrong for thousands of small ones, where
//! almost all the time goes to per-file work (create, metadata, close).
//!
//! This model treats each file as costing a fixed overhead plus time
//! proportional to its size:
//!
//! ```text
//! time(file)  = a + b * size
//! remaining   = (files_left * a + bytes_left * b) / workers
//! ```
//!
//! `a` (seconds per file) and `b` (seconds per byte) are fitted online with an
//! exponentially weighted least-squares regression over completed files, so
//! the estimate follows changing conditions. Whichever term dominates the
//! remaining work decides whether the UI shows MB/s, files/s, or both.

use serde::{Deserialize, Serialize};

const MB: f64 = 1024.0 * 1024.0;

/// Learned parameters, persisted per source/destination pair so the next
/// transfer between the same devices has a good estimate from the first frame.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EtaParams {
    /// Seconds of fixed overhead per file.
    pub per_file_secs: f64,
    /// Seconds per megabyte.
    pub per_mb_secs: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    /// Throughput dominates: show MB/s.
    Bytes,
    /// Per-file overhead dominates: show files/s.
    Files,
    /// Neither dominates: show both.
    Mixed,
}

#[derive(Clone, Copy, Debug)]
pub struct Estimate {
    pub seconds: f64,
    /// Fraction of the remaining time attributed to per-file overhead (0..=1).
    pub overhead_share: f64,
    pub display: DisplayMode,
}

#[derive(Debug, Clone)]
pub struct EtaModel {
    // Weighted sums for regressing t on s (s in MB): w, s, t, s², s·t.
    sw: f64,
    ss: f64,
    st: f64,
    sss: f64,
    sst: f64,
    samples: u64,
    /// Per-sample decay. 0.995 gives the last ~200 files most of the weight.
    decay: f64,
    /// Seconds per MB measured from in-flight chunks of large files.
    stream_b: Option<f64>,
    prior: Option<EtaParams>,
    smoothed: Option<f64>,
}

impl EtaModel {
    pub fn new(prior: Option<EtaParams>) -> Self {
        EtaModel {
            sw: 0.0, ss: 0.0, st: 0.0, sss: 0.0, sst: 0.0,
            samples: 0, decay: 0.995, stream_b: None, prior, smoothed: None,
        }
    }

    /// Set what is believed about the devices before any file has finished
    /// (for example from similar past transfers).
    pub fn set_prior(&mut self, prior: Option<EtaParams>) {
        self.prior = prior;
    }

    /// Record a completed file: its size and how long one worker spent on it.
    pub fn observe_file(&mut self, bytes: u64, secs: f64) {
        if !secs.is_finite() || secs < 0.0 {
            return;
        }
        let s = bytes as f64 / MB;
        let d = self.decay;
        self.sw = self.sw * d + 1.0;
        self.ss = self.ss * d + s;
        self.st = self.st * d + secs;
        self.sss = self.sss * d + s * s;
        self.sst = self.sst * d + s * secs;
        self.samples += 1;
    }

    /// Record progress inside a large file that has not finished yet.
    pub fn observe_chunk(&mut self, bytes: u64, secs: f64) {
        if bytes == 0 || !secs.is_finite() || secs <= 0.0 {
            return;
        }
        let b = secs / (bytes as f64 / MB);
        self.stream_b = Some(match self.stream_b {
            Some(prev) => prev * 0.8 + b * 0.2,
            None => b,
        });
    }

    /// Current best parameters, or `None` before any evidence exists.
    pub fn params(&self) -> Option<EtaParams> {
        let (mut a, mut b);
        if self.samples == 0 {
            match (self.stream_b, self.prior) {
                (Some(sb), p) => {
                    a = p.map(|p| p.per_file_secs).unwrap_or(0.0);
                    b = sb;
                }
                (None, Some(p)) => return Some(p),
                (None, None) => return None,
            }
        } else {
            let mean_s = self.ss / self.sw;
            let mean_t = self.st / self.sw;
            // Weighted variance of sizes; near zero means every file is about
            // the same size and the two parameters cannot be separated.
            let var = self.sss / self.sw - mean_s * mean_s;
            let separable = self.samples >= 8 && var > 1e-9 && var > (0.05 * mean_s).powi(2);
            if separable {
                let denom = self.sw * self.sss - self.ss * self.ss;
                b = (self.sw * self.sst - self.ss * self.st) / denom;
                a = (self.st - b * self.ss) / self.sw;
                if b < 0.0 {
                    b = 0.0;
                    a = mean_t;
                }
                if a < 0.0 {
                    a = 0.0;
                    b = if self.sss > 0.0 { self.sst / self.sss } else { 0.0 };
                }
            } else {
                // Split the mean using outside knowledge of throughput.
                let known_b = self.stream_b.or(self.prior.map(|p| p.per_mb_secs));
                match known_b {
                    Some(kb) => {
                        b = kb;
                        a = (mean_t - kb * mean_s).max(0.0);
                        if a == 0.0 && mean_s > 0.0 {
                            b = mean_t / mean_s;
                        }
                    }
                    // No throughput evidence: small files are mostly
                    // overhead, large files mostly throughput.
                    None if mean_s < 0.25 => {
                        a = mean_t;
                        b = 0.0;
                    }
                    None => {
                        a = 0.0;
                        b = mean_t / mean_s;
                    }
                }
            }
            // Live chunk timing reflects current throughput better than
            // history when a large file is mid-flight.
            if let (true, Some(sb)) = (separable, self.stream_b) {
                b = 0.5 * b + 0.5 * sb;
            }
        }
        Some(EtaParams { per_file_secs: a, per_mb_secs: b })
    }

    /// Estimate remaining wall-clock time.
    pub fn estimate(&mut self, files_left: u64, bytes_left: u64, workers: usize) -> Option<Estimate> {
        let p = self.params()?;
        let overhead = files_left as f64 * p.per_file_secs;
        let stream = bytes_left as f64 / MB * p.per_mb_secs;
        let total = overhead + stream;
        let effective = (workers.max(1) as f64).min(files_left.max(1) as f64);
        let raw = total / effective;
        // Light smoothing so the displayed number does not jitter, while still
        // counting down. The first value is taken as is.
        let secs = match self.smoothed {
            Some(prev) if raw > 0.0 => prev * 0.7 + raw * 0.3,
            _ => raw,
        };
        self.smoothed = Some(secs);
        let share = if total > 0.0 { overhead / total } else { 0.0 };
        let display = if share >= 0.65 {
            DisplayMode::Files
        } else if share <= 0.35 {
            DisplayMode::Bytes
        } else {
            DisplayMode::Mixed
        };
        Some(Estimate { seconds: secs, overhead_share: share, display })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simulated device: 4 ms per file plus 100 MB/s.
    const A: f64 = 0.004;
    const B: f64 = 0.01;
    fn cost(bytes: u64) -> f64 {
        A + B * bytes as f64 / MB
    }

    fn feed_mixed(m: &mut EtaModel) {
        for i in 0..400u64 {
            let size = match i % 10 {
                0 => 50 * MB as u64,
                1 | 2 => 2 * MB as u64,
                _ => 4096 + i * 13,
            };
            m.observe_file(size, cost(size));
        }
    }

    #[test]
    fn recovers_both_parameters_from_mixed_sizes() {
        let mut m = EtaModel::new(None);
        feed_mixed(&mut m);
        let p = m.params().unwrap();
        assert!((p.per_file_secs - A).abs() / A < 0.02, "a = {}", p.per_file_secs);
        assert!((p.per_mb_secs - B).abs() / B < 0.02, "b = {}", p.per_mb_secs);
    }

    #[test]
    fn small_file_job_is_predicted_far_better_than_mb_per_sec() {
        // Phase 1 copied a few big files, phase 2 is 100k tiny files.
        let mut m = EtaModel::new(None);
        feed_mixed(&mut m);
        let files_left = 100_000u64;
        let bytes_left = files_left * 2048;
        let truth = files_left as f64 * cost(2048);

        let est = m.estimate(files_left, bytes_left, 1).unwrap();
        assert!((est.seconds - truth).abs() / truth < 0.05, "est {} truth {}", est.seconds, truth);
        assert_eq!(est.display, DisplayMode::Files);

        // What a throughput-only estimate (Explorer style) would say.
        let naive = bytes_left as f64 / MB * B;
        assert!(truth / naive > 100.0, "MB/s-only is off by {}x", truth / naive);
    }

    #[test]
    fn large_file_job_shows_bytes_mode() {
        let mut m = EtaModel::new(None);
        feed_mixed(&mut m);
        let est = m.estimate(3, 3 * 4096 * MB as u64, 1).unwrap();
        assert_eq!(est.display, DisplayMode::Bytes);
        assert!((est.seconds - 3.0 * cost(4096 * MB as u64)).abs() < 1.0);
    }

    #[test]
    fn uniform_sizes_fall_back_sensibly() {
        // All tiny, no throughput evidence: everything is overhead.
        let mut m = EtaModel::new(None);
        for _ in 0..50 {
            m.observe_file(1000, 0.005);
        }
        let p = m.params().unwrap();
        assert!((p.per_file_secs - 0.005).abs() < 1e-9);
        assert_eq!(p.per_mb_secs, 0.0);

        // All large and equal: everything is throughput.
        let mut m = EtaModel::new(None);
        for _ in 0..50 {
            m.observe_file(100 * MB as u64, 1.0);
        }
        let p = m.params().unwrap();
        assert_eq!(p.per_file_secs, 0.0);
        assert!((p.per_mb_secs - 0.01).abs() < 1e-9);
    }

    #[test]
    fn prior_and_chunks_give_an_estimate_before_any_file_finishes() {
        let mut none = EtaModel::new(None);
        assert!(none.estimate(10, 1000, 1).is_none());

        let prior = EtaParams { per_file_secs: 0.002, per_mb_secs: 0.02 };
        let mut m = EtaModel::new(Some(prior));
        assert_eq!(m.params(), Some(prior));

        // First big file is mid-flight at 200 MB/s.
        m.observe_chunk(8 * MB as u64, 0.04);
        let p = m.params().unwrap();
        assert!((p.per_mb_secs - 0.005).abs() < 1e-9);
        assert_eq!(p.per_file_secs, 0.002);
    }

    #[test]
    fn workers_divide_the_estimate() {
        let mut a = EtaModel::new(Some(EtaParams { per_file_secs: 0.01, per_mb_secs: 0.0 }));
        let mut b = a.clone();
        let one = a.estimate(1000, 0, 1).unwrap().seconds;
        let four = b.estimate(1000, 0, 4).unwrap().seconds;
        assert!((one / four - 4.0).abs() < 1e-9);
    }

    #[test]
    fn adapts_when_the_device_slows_down() {
        let mut m = EtaModel::new(None);
        feed_mixed(&mut m);
        // Device becomes 5x slower per file (e.g. antivirus kicks in).
        for i in 0..1500u64 {
            let size = if i % 10 == 0 { 20 * MB as u64 } else { 4096 + i };
            m.observe_file(size, 5.0 * A + B * size as f64 / MB);
        }
        let p = m.params().unwrap();
        assert!((p.per_file_secs - 5.0 * A).abs() / (5.0 * A) < 0.1, "a = {}", p.per_file_secs);
    }
}


/// Smooths the time-left readout and says how far to trust it (0..=1).
///
/// Confidence needs both enough data (some seconds of copying, or a good
/// share of the job already done, whichever comes first) and a steady
/// prediction: the projected finish time must stop moving around.
#[derive(Debug, Default)]
pub struct EtaTracker {
    /// (seconds since copying began, projected finish time), newest last.
    finishes: std::collections::VecDeque<(f64, f64)>,
    shown: Option<f64>,
    trusted: bool,
    /// How much a speed test or earlier transfers already vouch for the first
    /// estimate (0..=1), so it can be trusted sooner.
    head_start: f64,
}

/// Stability is judged over this many seconds of recent estimates.
const STABILITY_WINDOW: f64 = 5.0;

const TRACKER_SAMPLES: usize = 64;

impl EtaTracker {
    pub fn with_head_start(head_start: f64) -> Self {
        EtaTracker { head_start: head_start.clamp(0.0, 0.9), ..Default::default() }
    }

    /// `raw` is the model's estimate; `dt` the seconds since the last call;
    /// `done` the finished share of the job (0..=1); `copying` seconds spent copying.
    /// Returns the smoothed estimate and the confidence.
    pub fn update(&mut self, raw: Option<f64>, dt: f64, done: f64, copying: f64) -> (Option<f64>, f64) {
        let Some(raw) = raw.filter(|r| r.is_finite()) else {
            self.finishes.clear();
            self.shown = None;
            self.trusted = false;
            return (None, 0.0);
        };
        self.finishes.push_back((copying, copying + raw));
        while self.finishes.len() > TRACKER_SAMPLES || self.finishes.front().is_some_and(|f| copying - f.0 > STABILITY_WINDOW) {
            self.finishes.pop_front();
        }
        let data = (copying / 2.5).max(done / 0.04).max(self.head_start).min(1.0);
        // Needs a few estimates over at least most of a second, however often they arrive.
        let span = self.finishes.front().map(|f| copying - f.0).unwrap_or(0.0);
        let stability = if self.finishes.len() < 3 || span < 0.8 {
            0.0
        } else {
            let (lo, hi) = self.finishes.iter().fold((f64::MAX, f64::MIN), |(l, h), &(_, f)| (l.min(f), h.max(f)));
            // Small files over a network come in bursts: a swing of a third of the time left within a few
            // seconds is still a steady estimate.
            (1.0 - (hi - lo) / raw.max(2.0) / 0.35).clamp(0.0, 1.0)
        };
        let mut confidence = data * stability;
        // However bumpy the speed, a quarter of the job done is a real basis: the estimate is at least
        // "getting more accurate" by a fifth of the way through, and trusted a little later.
        confidence = confidence.max((done * 2.5).min(0.7));
        if self.trusted {
            if confidence > 0.5 {
                confidence = confidence.max(0.85);
            } else {
                self.trusted = false;
            }
        } else if confidence >= 0.85 {
            self.trusted = true;
        }
        let alpha = 0.1 + 0.4 * (1.0 - confidence);
        let shown = match self.shown {
            Some(prev) => (prev - dt).max(0.0) * (1.0 - alpha) + raw * alpha,
            None => raw,
        };
        self.shown = Some(shown);
        (Some(shown), confidence)
    }
}

#[cfg(test)]
mod tracker_tests {
    use super::EtaTracker;

    #[test]
    fn steady_estimate_becomes_trusted() {
        let mut t = EtaTracker::default();
        let mut conf = 0.0;
        for i in 0..60 {
            let copying = i as f64 * 0.1;
            (_, conf) = t.update(Some(60.0 - copying), 0.1, copying / 66.0, copying);
        }
        assert!(conf >= 0.85, "{conf}");
    }

    #[test]
    fn jumpy_estimate_stays_untrusted() {
        let mut t = EtaTracker::default();
        let mut conf = 1.0;
        for i in 0..60 {
            let copying = i as f64 * 0.1;
            let raw = if i % 2 == 0 { 20.0 } else { 90.0 };
            (_, conf) = t.update(Some(raw), 0.1, 0.01, copying);
        }
        assert!(conf < 0.5, "{conf}");
    }

    #[test]
    fn a_big_head_start_settles_fast() {
        let mut t = EtaTracker::default();
        let mut conf = 0.0;
        for i in 0..12 {
            let copying = i as f64 * 0.1;
            (_, conf) = t.update(Some(30.0 - copying), 0.1, 0.5, copying);
        }
        assert!(conf >= 0.85, "{conf}");
    }

    #[test]
    fn smoothing_counts_down() {
        let mut t = EtaTracker::default();
        let (a, _) = t.update(Some(30.0), 0.1, 0.0, 0.0);
        let (b, _) = t.update(Some(40.0), 0.1, 0.0, 0.1);
        assert!(b.unwrap() < 40.0 && b.unwrap() > a.unwrap() - 0.1);
    }
}
