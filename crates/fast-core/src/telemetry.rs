//! On-demand application performance, latency, and throughput telemetry.
//!
//! Sampling and recording stay completely dormant (`is_active() == false`) until
//! a user explicitly opens the Metrics view or pops it out, keeping normal file
//! browsing free of per-operation locking and allocation overhead.

use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const MAX_OP_SAMPLES: usize = 48;
const MAX_RECENT_OPS: usize = 40;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static REMOTE_READ_BYTES: AtomicU64 = AtomicU64::new(0);
static REMOTE_WRITE_BYTES: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct OpBucket {
    count: u64,
    errors: u64,
    sum_ms: f64,
    last_ms: f64,
    max_ms: f64,
    ema_ms: f64,
    samples: VecDeque<f64>,
}

#[derive(Clone)]
struct RawEvent {
    seq: u64,
    op: &'static str,
    detail: String,
    duration_ms: f64,
    ok: bool,
    at: Instant,
}

#[derive(Default)]
struct TelemetryStore {
    next_seq: u64,
    ops: BTreeMap<&'static str, OpBucket>,
    recent: VecDeque<RawEvent>,
}

fn store() -> &'static Mutex<TelemetryStore> {
    static S: OnceLock<Mutex<TelemetryStore>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn set_active(on: bool) {
    let prev = ACTIVE.swap(on, Ordering::Relaxed);
    if prev && !on {
        reset();
    }
}

pub fn reset() {
    REMOTE_READ_BYTES.store(0, Ordering::Relaxed);
    REMOTE_WRITE_BYTES.store(0, Ordering::Relaxed);
    if let Ok(mut g) = store().lock() {
        *g = TelemetryStore::default();
    }
}

pub fn record_remote_io(read_bytes: u64, write_bytes: u64) {
    if !is_active() {
        return;
    }
    if read_bytes > 0 {
        REMOTE_READ_BYTES.fetch_add(read_bytes, Ordering::Relaxed);
    }
    if write_bytes > 0 {
        REMOTE_WRITE_BYTES.fetch_add(write_bytes, Ordering::Relaxed);
    }
}

pub fn remote_io_totals() -> (u64, u64) {
    (REMOTE_READ_BYTES.load(Ordering::Relaxed), REMOTE_WRITE_BYTES.load(Ordering::Relaxed))
}

pub fn record_op(op: &'static str, detail: &str, duration: Duration, ok: bool) {
    if !is_active() {
        return;
    }
    let ms = duration.as_secs_f64() * 1000.0;
    let Ok(mut g) = store().lock() else { return };
    g.next_seq += 1;
    let seq = g.next_seq;
    let bucket = g.ops.entry(op).or_default();
    bucket.count += 1;
    if !ok {
        bucket.errors += 1;
    }
    bucket.sum_ms += ms;
    bucket.last_ms = ms;
    if ms > bucket.max_ms {
        bucket.max_ms = ms;
    }
    bucket.ema_ms = if bucket.count == 1 { ms } else { bucket.ema_ms * 0.7 + ms * 0.3 };
    if bucket.samples.len() >= MAX_OP_SAMPLES {
        bucket.samples.pop_front();
    }
    bucket.samples.push_back(ms);

    if g.recent.len() >= MAX_RECENT_OPS {
        g.recent.pop_front();
    }
    g.recent.push_back(RawEvent {
        seq,
        op,
        detail: detail.chars().take(160).collect(),
        duration_ms: ms,
        ok,
        at: Instant::now(),
    });
}

/// Scoped timer that does zero work when telemetry is disabled.
pub struct OpGuard {
    op: &'static str,
    detail: String,
    started: Instant,
    ok: bool,
}

impl OpGuard {
    pub fn finish<T, E>(mut self, res: &std::result::Result<T, E>) {
        self.ok = res.is_ok();
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        record_op(self.op, &self.detail, self.started.elapsed(), self.ok);
    }
}

/// Start timing `op` only if telemetry is currently active (`detail` closure is not evaluated when inactive).
pub fn begin<S: Into<String>>(op: &'static str, detail: impl FnOnce() -> S) -> Option<OpGuard> {
    if !is_active() {
        return None;
    }
    Some(OpGuard {
        op,
        detail: detail().into(),
        started: Instant::now(),
        ok: true,
    })
}

pub fn finish_guard<T, E>(guard: Option<OpGuard>, res: &std::result::Result<T, E>) {
    if let Some(g) = guard {
        g.finish(res);
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OpLatencySummary {
    pub op: String,
    pub count: u64,
    pub errors: u64,
    pub last_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub expected_ms: f64,
    pub samples_ms: Vec<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecentOpEvent {
    pub seq: u64,
    pub op: String,
    pub detail: String,
    pub duration_ms: f64,
    pub ok: bool,
    pub age_ms: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryReport {
    pub operations: Vec<OpLatencySummary>,
    pub recent: Vec<RecentOpEvent>,
}

fn percentile_95(samples: &VecDeque<f64>) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = samples.iter().copied().collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let idx = ((sorted.len() as f64) * 0.95).ceil() as usize;
    sorted[idx.saturating_sub(1).min(sorted.len() - 1)]
}

pub fn snapshot() -> TelemetryReport {
    let Ok(g) = store().lock() else { return TelemetryReport::default() };
    let mut operations: Vec<OpLatencySummary> = g
        .ops
        .iter()
        .map(|(op, b)| {
            let p95 = percentile_95(&b.samples);
            let avg = if b.count > 0 { b.sum_ms / b.count as f64 } else { 0.0 };
            let expected = if b.count <= 1 { b.last_ms } else { b.ema_ms * 0.65 + p95 * 0.35 };
            OpLatencySummary {
                op: (*op).to_string(),
                count: b.count,
                errors: b.errors,
                last_ms: b.last_ms,
                avg_ms: avg,
                p95_ms: p95,
                max_ms: b.max_ms,
                expected_ms: expected,
                samples_ms: b.samples.iter().copied().collect(),
            }
        })
        .collect();
    operations.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.op.cmp(&b.op)));
    let now = Instant::now();
    let recent: Vec<RecentOpEvent> = g
        .recent
        .iter()
        .rev()
        .map(|e| RecentOpEvent {
            seq: e.seq,
            op: e.op.to_string(),
            detail: e.detail.clone(),
            duration_ms: e.duration_ms,
            ok: e.ok,
            age_ms: now.saturating_duration_since(e.at).as_millis() as u64,
        })
        .collect();
    TelemetryReport { operations, recent }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TURN: Mutex<()> = Mutex::new(());

    #[test]
    fn skips_recording_when_inactive_and_records_when_active() {
        let _lock = TURN.lock().unwrap_or_else(|e| e.into_inner());
        set_active(false);
        assert!(begin("list_dir", || "should_not_eval").is_none());
        record_op("list_dir", "inactive", Duration::from_millis(25), true);
        assert!(snapshot().operations.is_empty());

        set_active(true);
        record_op("list_dir", "C:\\Work", Duration::from_millis(10), true);
        record_op("list_dir", "D:\\Slow", Duration::from_millis(50), false);
        let snap = snapshot();
        assert_eq!(snap.operations.len(), 1);
        let op = &snap.operations[0];
        assert_eq!(op.count, 2);
        assert_eq!(op.errors, 1);
        assert!(op.avg_ms >= 29.0 && op.avg_ms <= 31.0);
        assert!(op.expected_ms >= 20.0);
        assert_eq!(snap.recent.len(), 2);

        set_active(false);
        assert!(snapshot().operations.is_empty());
    }
}
