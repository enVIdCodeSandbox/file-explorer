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


/// How fast the job is really getting done, measured, not modelled.
///
/// The cost model above needs files to finish one by one under its own eyes, and starts from what past
/// jobs cost; a tool such as Robocopy reports only totals, and a past job on another day may have been
/// ten times slower. This measures the one number that matters: how much of the job (as a share of the
/// whole, weighted between files and bytes) is finished each second.
///
/// Which stretch of the past is worth looking at depends on how far ahead the guess has to reach. With
/// minutes to go, a few seconds of speed say little and a minute of it says more; with seconds to go the
/// last few seconds are what matters. So the rate is taken over a window about half as long as the time
/// left, grown from a few seconds as long as the older stretch was going at about the same pace as the
/// newer one. When the pace really changed, the older stretch disagrees, the window stops short of it,
/// and the estimate lands on the new pace within seconds instead of averaging it with the old one.
#[derive(Debug, Default)]
pub struct LiveRate {
    /// (seconds since copying began, finished share of the job), newest last.
    samples: std::collections::VecDeque<(f64, f64)>,
    /// Share of the job finished per second, as currently believed.
    rate: Option<f64>,
    /// Seconds left as last worked out: it says how far ahead the next guess has to reach.
    hint: f64,
}

const WINDOWS: [f64; 7] = [4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0];
/// Samples further back than this are not needed.
const HISTORY_SECS: f64 = 520.0;

impl LiveRate {
    /// The finished share at `t` seconds after the start, read linearly between samples (the job began at 0).
    fn share_at(&self, t: f64) -> Option<f64> {
        self.samples.back()?;
        let mut prev = (0.0, 0.0);
        for &(ts, sh) in self.samples.iter() {
            if ts >= t {
                let span = ts - prev.0;
                return Some(if span <= 0.0 { sh } else { prev.1 + (sh - prev.1) * ((t - prev.0) / span).clamp(0.0, 1.0) });
            }
            prev = (ts, sh);
        }
        Some(prev.1)
    }

    /// Rate over the stretch from `from` to `to` (seconds since the start), when there is data for all of it.
    fn rate_between(&self, from: f64, to: f64) -> Option<f64> {
        if to - from < 0.5 || from < -1e-9 {
            return None;
        }
        let (a, b) = (self.share_at(from)?, self.share_at(to)?);
        Some(((b - a) / (to - from)).max(0.0))
    }

    /// Add a reading and return the current rate (share of the job per second), once there is one.
    pub fn update(&mut self, t: f64, share: f64) -> Option<f64> {
        if !t.is_finite() || !share.is_finite() {
            return self.rate;
        }
        if self.samples.back().is_some_and(|l| t - l.0 < 0.2) {
            self.samples.pop_back();
        }
        self.samples.push_back((t, share));
        while self.samples.len() > 2 && t - self.samples[1].0 >= HISTORY_SECS {
            self.samples.pop_front();
        }
        if t < 0.8 {
            return self.rate;
        }
        // How far back to look: about half the time left, from a few seconds to minutes.
        let reach = if self.hint > 0.0 { (self.hint * 0.5).clamp(4.0, 256.0) } else { 256.0 };
        let mut best = self.rate_between((t - WINDOWS[0]).max(0.0), t).or(self.rate_between(0.0, t));
        let mut previous = WINDOWS[0];
        for &w in WINDOWS.iter().skip(1) {
            // Wider only while the stretch before the window can be looked at too, and the window is within reach.
            if w > reach || w > t {
                break;
            }
            let (Some(inner), Some(older), Some(wide)) = (self.rate_between(t - previous, t), self.rate_between(t - 2.0 * previous, t - previous), self.rate_between(t - w, t)) else { break };
            // The newer stretch against the one just before it: a different pace means the older one is another regime.
            let ratio = if inner.min(older) <= 1e-12 { if inner.max(older) <= 1e-12 { 1.0 } else { f64::MAX } } else { inner.max(older) / inner.min(older) };
            if ratio - 1.0 > 0.15 + 0.9 / previous.sqrt() {
                break;
            }
            best = Some(wide);
            previous = w;
        }
        self.rate = best;
        self.rate
    }

    /// Seconds left at the current rate, when the rate is known and above zero.
    pub fn remaining(&mut self, share: f64) -> Option<f64> {
        let rate = self.rate.filter(|r| *r > 1e-9)?;
        let left = ((1.0 - share).max(0.0)) / rate;
        self.hint = left;
        Some(left)
    }
}

/// Joins what the cost model expects with what is measured. Measurement takes over as it gets
/// reliable: after a few seconds, or when a few percent of the job is done, whichever comes first.
pub fn blend_estimates(model: Option<f64>, live: Option<f64>, copying: f64, share: f64) -> Option<f64> {
    let Some(live) = live else { return model };
    let Some(model) = model else { return Some(live) };
    let trust = (copying / 6.0).max(share / 0.02).clamp(0.0, 1.0);
    Some(live * trust + model * (1.0 - trust))
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
        // The shown time counts down by the clock, and is pulled toward the new estimate at a pace that depends on
        // the scale: with minutes left a few seconds of difference barely move it, with seconds left it follows closely,
        // and an estimate that is far off lands quickly whatever the scale.
        let shown = match self.shown {
            Some(prev) => {
                let counted = (prev - dt).max(0.0);
                let off = ((raw - counted).abs() / raw.max(counted).max(1.0)).min(1.0);
                let tau = (0.1 * raw).clamp(1.0, 30.0) / (1.0 + 10.0 * off);
                let alpha = 1.0 - (-dt.max(0.0) / tau).exp();
                counted + (raw - counted) * alpha
            }
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

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Runs a job whose speed (share per second) is given by `speed(t)`; returns (t, estimated total secs) each second.
    fn run(total: f64, speed: impl Fn(f64) -> f64, model: Option<f64>) -> Vec<(f64, f64)> {
        let (mut lr, mut share, mut t) = (LiveRate::default(), 0.0f64, 0.0f64);
        let mut out = Vec::new();
        while share < 1.0 && t < total * 5.0 {
            t += 0.25;
            share = (share + speed(t) * 0.25).min(1.0);
            let live = lr.update(t, share).and_then(|_| lr.remaining(share));
            if let Some(left) = blend_estimates(model, live, t, share) {
                out.push((t, t + left));
            }
        }
        out
    }

    fn at(v: &[(f64, f64)], t: f64) -> f64 {
        v.iter().find(|p| p.0 >= t).unwrap().1
    }

    #[test]
    fn a_steady_job_is_estimated_closely_within_seconds() {
        // 200 seconds in all.
        let v = run(200.0, |_| 1.0 / 200.0, None);
        for t in [5.0, 20.0, 100.0] {
            let est = at(&v, t);
            assert!((est - 200.0).abs() / 200.0 < 0.05, "at {t}s the job was said to take {est}s");
        }
    }

    #[test]
    fn a_model_that_is_ten_times_off_is_overruled_quickly() {
        // The model says 27 minutes (1620s ahead); the job takes 270s.
        let v = run(270.0, |_| 1.0 / 270.0, Some(1620.0));
        let est = at(&v, 10.0);
        assert!((est - 270.0).abs() / 270.0 < 0.15, "after 10s: {est}");
        let est = at(&v, 30.0);
        assert!((est - 270.0).abs() / 270.0 < 0.05, "after 30s: {est}");
    }

    #[test]
    fn a_burst_at_the_start_does_not_ruin_the_estimate() {
        // A cache soaks up the first two seconds at 20x speed, then the real speed (270 s job).
        let v = run(270.0, |t| if t < 2.0 { 20.0 / 270.0 } else { 1.0 / 270.0 }, None);
        // The burst did 15% of the job in 2 s: the job takes 2 + 0.85 * 270 s in all.
        let truth = 2.0 + 0.85 * 270.0;
        let est = at(&v, 30.0);
        assert!((est - truth).abs() / truth < 0.08, "after 30s: {est} vs {truth}");
    }

    #[test]
    fn a_real_change_of_speed_lands_within_seconds() {
        // 100 s at one speed, then twice as fast: the job (done 40% in the first 100s) ends at 100 + 60%/(2/250).
        let speed = |t: f64| if t < 100.0 { 0.4 / 100.0 } else { 2.0 * 0.4 / 100.0 };
        let v = run(250.0, speed, None);
        let truth = 100.0 + 0.6 / (0.8 / 100.0);
        let est = at(&v, 112.0);
        assert!((est - truth).abs() / truth < 0.12, "12s after the change: {est} vs {truth}");
        let est = at(&v, 130.0);
        assert!((est - truth).abs() / truth < 0.05, "30s after: {est} vs {truth}");
    }

    #[test]
    fn jitter_does_not_make_the_number_jump() {
        // Speed wobbles +-40% every second; the estimate stays within 10% of the truth.
        let v = run(300.0, |t| (1.0 + 0.4 * (t * 6.283).sin()) / 300.0, None);
        for (t, est) in v.iter().filter(|p| p.0 > 15.0 && p.0 < 250.0) {
            assert!((est - 300.0).abs() / 300.0 < 0.10, "at {t}s: {est}");
        }
    }

    #[test]
    fn a_stall_stretches_the_estimate_instead_of_dividing_by_zero() {
        let mut lr = LiveRate::default();
        let mut share = 0.0;
        for i in 1..=40 {
            let t = i as f64 * 0.5;
            share += 0.002;
            lr.update(t, share);
        }
        let before = lr.remaining(share).unwrap();
        for i in 41..=100 {
            lr.update(i as f64 * 0.5, share);
        }
        let after = lr.remaining(share).filter(|r| r.is_finite());
        assert!(after.is_none_or(|a| a > before * 2.0), "{before} then {after:?}");
    }

    #[test]
    fn the_screenshot_case() {
        // 12% done after 46 s (37 MB/s, 570 files/s, 128k files and 9.7 GB left): about 5.5 minutes, not 27.
        let mut lr = LiveRate::default();
        for i in 1..=46 {
            lr.update(i as f64, 0.12 * i as f64 / 46.0);
        }
        let left = blend_estimates(Some(27.0 * 60.0), lr.remaining(0.12), 46.0, 0.12).unwrap();
        assert!((left - 337.0).abs() < 40.0, "{left}");
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    /// A tracker whose estimate has settled at `secs` left.
    fn settled(secs: f64) -> EtaTracker {
        let mut t = EtaTracker::default();
        for i in 0..60 {
            t.update(Some(secs), 0.25, 0.3, 30.0 + i as f64 * 0.25);
        }
        t
    }

    #[test]
    fn with_minutes_left_seconds_of_noise_barely_move_the_time() {
        let mut t = settled(1200.0);
        let before = t.shown.unwrap();
        // One estimate that is 30 seconds off (2.5%) for a moment.
        let (shown, _) = t.update(Some(1230.0), 0.25, 0.3, 46.0);
        assert!((shown.unwrap() - (before - 0.25)).abs() < 1.0, "{before} -> {shown:?}");
    }

    #[test]
    fn with_seconds_left_a_few_seconds_matter_at_once() {
        let mut t = settled(20.0);
        let mut shown = 0.0;
        // The estimate jumps to 30 s left: within two seconds most of the way there.
        for i in 0..8 {
            let gone = (i + 1) as f64 * 0.25;
            shown = t.update(Some(30.0 - gone), 0.25, 0.9, 46.0 + gone).0.unwrap();
        }
        // 2 s later the truth is 28 s left; the readout is within 3 s of it.
        assert!((shown - 28.0).abs() < 3.0, "{shown}");
    }

    #[test]
    fn a_far_off_estimate_lands_quickly_at_any_scale() {
        let mut t = settled(1620.0);
        let mut shown = 0.0;
        // The truth is 300 s: 15 seconds later the readout is most of the way down.
        for i in 0..60 {
            let gone = (i + 1) as f64 * 0.25;
            shown = t.update(Some(300.0 - gone), 0.25, 0.3, 46.0 + gone).0.unwrap();
        }
        // 15 s later the truth is 285 s left: the readout is within 30% of it (a 5x error cut to under 1.3x).
        assert!((shown - 285.0).abs() / 285.0 < 0.30, "{shown}");
    }

    #[test]
    fn the_readout_changes_by_the_same_share_whether_it_counts_seconds_or_minutes() {
        // Same 10% disagreement: the readout moves a similar share of the way in a similar time.
        for left in [60.0, 600.0, 3600.0] {
            let mut t = settled(left);
            let start = t.shown.unwrap();
            let mut shown = start;
            for i in 0..12 {
                shown = t.update(Some(left * 1.1), 0.25, 0.3, 46.0 + i as f64 * 0.25).0.unwrap();
            }
            let moved = (shown - (start - 3.0)) / (left * 1.1 - (start - 3.0));
            assert!((0.0..=1.0).contains(&moved), "{left}: moved {moved}");
        }
    }
}
