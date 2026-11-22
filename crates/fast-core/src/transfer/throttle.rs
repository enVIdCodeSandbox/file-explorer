//! Limits on how fast and how many transfers run: a speed limit (one per transfer and one for the whole
//! app) and a queue that lets only so many transfers work at once, highest priority first.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const LOW: u8 = 0;
pub const NORMAL: u8 = 1;
pub const HIGH: u8 = 2;

pub fn priority_from(name: &str) -> u8 {
    match name {
        "low" => LOW,
        "high" => HIGH,
        _ => NORMAL,
    }
}

/// A speed limit in bytes per second shared by every thread that moves bytes under it. 0 = no limit.
#[derive(Default)]
pub struct Limiter {
    rate: AtomicU64,
    next: Mutex<Option<Instant>>,
}

impl Limiter {
    pub fn new(rate: u64) -> Limiter {
        Limiter { rate: AtomicU64::new(rate), next: Mutex::new(None) }
    }
    pub fn set_rate(&self, bytes_per_sec: u64) {
        self.rate.store(bytes_per_sec, Ordering::Relaxed);
    }
    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }
    /// Claim `n` bytes and say how long to wait before moving them so the average stays at the limit.
    /// Waiting never builds up credit: a pause does not allow a burst afterwards.
    pub fn reserve(&self, n: u64) -> Duration {
        let rate = self.rate();
        if rate == 0 {
            return Duration::ZERO;
        }
        let now = Instant::now();
        let mut next = self.next.lock();
        let start = next.map_or(now, |t| t.max(now));
        *next = Some(start + Duration::from_secs_f64(n as f64 / rate as f64));
        start.saturating_duration_since(now)
    }
}

/// The limit for the whole app, set from the settings.
pub fn global_limiter() -> &'static Limiter {
    static G: OnceLock<Limiter> = OnceLock::new();
    G.get_or_init(Limiter::default)
}

/// The queue every transfer passes through before it starts working.
pub fn gate() -> &'static Gate {
    static G: OnceLock<Gate> = OnceLock::new();
    G.get_or_init(Gate::default)
}

struct Waiter {
    id: u64,
    seq: u64,
    priority: Arc<AtomicU8>,
    start_now: Arc<AtomicBool>,
    /// The hard disks (spinning drives) this transfer reads from or writes to.
    disks: Vec<String>,
}

#[derive(Default)]
struct GateState {
    /// What is working now, and on which hard disks.
    running: Vec<(u64, Vec<String>)>,
    seq: u64,
    waiting: Vec<Waiter>,
}

fn share(a: &[String], b: &[String]) -> Option<String> {
    a.iter().find(|x| b.contains(x)).cloned()
}

/// Lets `max` transfers work at once (0 = all of them). The rest wait; the highest priority goes first,
/// and among equals the one that has waited longest.
///
/// Transfers on different drives always work at the same time. Two that use the same hard disk take turns
/// (unless that is switched off), because a spinning disk asked for two things at once spends its time
/// moving its head between them and both go slower than one after the other.
#[derive(Default)]
pub struct Gate {
    max: AtomicUsize,
    /// Set when transfers on the same hard disk should work together anyway.
    together: AtomicBool,
    state: Mutex<GateState>,
}

impl Gate {
    pub fn set_max(&self, max: usize) {
        self.max.store(max, Ordering::Relaxed);
    }
    /// Whether transfers that use the same hard disk take turns (the default).
    pub fn set_disk_turns(&self, on: bool) {
        self.together.store(!on, Ordering::Relaxed);
    }
    pub fn join(&self, id: u64, priority: Arc<AtomicU8>, start_now: Arc<AtomicBool>) {
        self.join_on(id, priority, start_now, Vec::new());
    }
    /// Join the line, saying which hard disks the transfer works on.
    pub fn join_on(&self, id: u64, priority: Arc<AtomicU8>, start_now: Arc<AtomicBool>, disks: Vec<String>) {
        let mut s = self.state.lock();
        s.seq += 1;
        let seq = s.seq;
        s.waiting.push(Waiter { id, seq, priority, start_now, disks });
    }
    /// A hard disk that `w` has to wait for: one a working transfer is using, or one that a transfer
    /// ahead of it in the line is going to use first.
    fn busy_disk(&self, s: &GateState, w: &Waiter) -> Option<String> {
        if self.together.load(Ordering::Relaxed) || w.disks.is_empty() {
            return None;
        }
        let key = |w: &Waiter| (std::cmp::Reverse(w.priority.load(Ordering::Relaxed)), w.seq);
        let mine = key(w);
        s.running
            .iter()
            .find_map(|(_, d)| share(d, &w.disks))
            .or_else(|| s.waiting.iter().filter(|o| o.id != w.id && key(o) < mine && !o.start_now.load(Ordering::Relaxed)).find_map(|o| share(&o.disks, &w.disks)))
    }
    /// True when it is `id`'s turn; it then counts as running until `leave`.
    pub fn try_enter(&self, id: u64) -> bool {
        let max = self.max.load(Ordering::Relaxed);
        let mut s = self.state.lock();
        let Some(pos) = s.waiting.iter().position(|w| w.id == id) else { return true };
        let forced = s.waiting[pos].start_now.load(Ordering::Relaxed);
        // The first in line among those that are not held back by a disk.
        let first = s
            .waiting
            .iter()
            .filter(|w| !w.start_now.load(Ordering::Relaxed) && self.busy_disk(&s, w).is_none())
            .min_by_key(|w| (std::cmp::Reverse(w.priority.load(Ordering::Relaxed)), w.seq))
            .map(|w| w.id);
        let free = self.busy_disk(&s, &s.waiting[pos]).is_none();
        if forced || (free && (max == 0 || (s.running.len() < max && first == Some(id)))) {
            let w = s.waiting.remove(pos);
            s.running.push((w.id, w.disks));
            true
        } else {
            false
        }
    }
    /// Give up waiting (cancelled before its turn).
    pub fn withdraw(&self, id: u64) {
        self.state.lock().waiting.retain(|w| w.id != id);
    }
    pub fn leave(&self, id: u64) {
        let mut s = self.state.lock();
        if let Some(i) = s.running.iter().position(|(r, _)| *r == id) {
            s.running.remove(i);
        }
    }
    /// How many are ahead of `id` in the queue.
    pub fn ahead_of(&self, id: u64) -> u32 {
        let s = self.state.lock();
        let Some(me) = s.waiting.iter().find(|w| w.id == id) else { return 0 };
        let key = |w: &Waiter| (std::cmp::Reverse(w.priority.load(Ordering::Relaxed)), w.seq);
        let mine = key(me);
        s.waiting.iter().filter(|w| w.id != id && key(w) < mine).count() as u32
    }
    /// The hard disk `id` is waiting for another transfer to finish with, if that is what it waits for.
    pub fn waiting_for_disk(&self, id: u64) -> Option<String> {
        let s = self.state.lock();
        let me = s.waiting.iter().find(|w| w.id == id)?;
        self.busy_disk(&s, me)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: u8) -> Arc<AtomicU8> {
        Arc::new(AtomicU8::new(v))
    }
    fn f() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn limiter_spaces_out_bytes_without_banking_credit() {
        let l = Limiter::new(1_000_000);
        assert_eq!(l.reserve(500_000), Duration::ZERO, "the first chunk goes at once");
        let w = l.reserve(500_000);
        assert!(w > Duration::from_millis(400) && w <= Duration::from_millis(500), "{w:?}");
        l.set_rate(0);
        assert_eq!(l.reserve(1 << 30), Duration::ZERO);
        let idle = Limiter::new(1_000);
        assert_eq!(idle.reserve(1), Duration::ZERO);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(idle.reserve(1), Duration::ZERO, "time passed, so no wait, and no stored-up burst either");
    }

    #[test]
    fn gate_runs_the_most_important_first() {
        let g = Gate::default();
        g.set_max(1);
        let (low, high, normal) = (p(LOW), p(HIGH), p(NORMAL));
        g.join(1, normal.clone(), f());
        assert!(g.try_enter(1));
        g.join(2, low.clone(), f());
        g.join(3, high.clone(), f());
        g.join(4, normal.clone(), f());
        assert!(!g.try_enter(2) && !g.try_enter(3), "one at a time");
        assert_eq!(g.ahead_of(2), 2);
        g.leave(1);
        assert!(!g.try_enter(2) && !g.try_enter(4), "the high one is first");
        assert!(g.try_enter(3));
        g.leave(3);
        assert!(!g.try_enter(2), "normal before low");
        assert!(g.try_enter(4));
        g.leave(4);
        // A priority changed while waiting counts at once.
        low.store(HIGH, Ordering::Relaxed);
        assert!(g.try_enter(2));
    }

    #[test]
    fn gate_unlimited_start_now_and_withdraw() {
        let g = Gate::default();
        g.join(1, p(NORMAL), f());
        g.join(2, p(NORMAL), f());
        assert!(g.try_enter(1) && g.try_enter(2), "0 = no limit");
        g.set_max(1);
        let now = f();
        g.join(3, p(LOW), now.clone());
        assert!(!g.try_enter(3));
        now.store(true, Ordering::Relaxed);
        assert!(g.try_enter(3), "start now ignores the limit");
        g.join(4, p(NORMAL), f());
        g.withdraw(4);
        assert!(g.try_enter(4), "not waiting any more, nothing to hold");
    }

    fn hdd(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn transfers_on_the_same_hard_disk_take_turns_but_others_run_together() {
        let g = Gate::default();
        g.join_on(1, p(NORMAL), f(), hdd(&["d:\\"]));
        assert!(g.try_enter(1));
        // Another copy on the same hard disk waits; one on an SSD, or on another hard disk, does not.
        g.join_on(2, p(NORMAL), f(), hdd(&["d:\\", "e:\\"]));
        g.join_on(3, p(NORMAL), f(), Vec::new());
        g.join_on(4, p(NORMAL), f(), hdd(&["f:\\"]));
        assert!(!g.try_enter(2));
        assert_eq!(g.waiting_for_disk(2).as_deref(), Some("d:\\"));
        assert!(g.try_enter(3) && g.try_enter(4));
        // Those that wait for the same disk keep their order.
        g.join_on(5, p(NORMAL), f(), hdd(&["d:\\"]));
        g.leave(1);
        assert!(!g.try_enter(5), "2 was first");
        assert!(g.try_enter(2));
        assert!(!g.try_enter(5), "2 is working on it now");
        g.leave(2);
        assert!(g.try_enter(5));
    }

    #[test]
    fn the_same_hard_disk_can_be_shared_when_asked_for_or_when_started_by_hand() {
        let g = Gate::default();
        g.join_on(1, p(NORMAL), f(), hdd(&["d:\\"]));
        assert!(g.try_enter(1));
        let now = f();
        g.join_on(2, p(NORMAL), now.clone(), hdd(&["d:\\"]));
        assert!(!g.try_enter(2));
        now.store(true, Ordering::Relaxed);
        assert!(g.try_enter(2), "start now does not wait for the disk");
        g.set_disk_turns(false);
        g.join_on(3, p(NORMAL), f(), hdd(&["d:\\"]));
        assert!(g.try_enter(3), "turned off: they work together");
        assert_eq!(g.waiting_for_disk(3), None);
    }

    #[test]
    fn a_disk_wait_does_not_hold_up_a_transfer_that_could_go_with_a_limit_of_one_more() {
        let g = Gate::default();
        g.set_max(2);
        g.join_on(1, p(NORMAL), f(), hdd(&["d:\\"]));
        assert!(g.try_enter(1));
        g.join_on(2, p(NORMAL), f(), hdd(&["d:\\"])); // held back by the disk
        g.join_on(3, p(NORMAL), f(), Vec::new()); // free, and there is room
        assert!(!g.try_enter(2));
        assert!(g.try_enter(3), "the one behind it is not stuck behind a disk wait");
    }
}
