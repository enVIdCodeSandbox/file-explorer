//! Resource governor: turns "what machine is this and what state is it in"
//! into concrete limits for threads, caches and background work.
//!
//! In dynamic mode a desktop on mains power gets everything, a laptop on
//! battery gets less, and a phone or a nearly flat battery gets the minimum.
//! In manual mode the user's numbers are used as given.

use crate::settings::Performance;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemSnapshot {
    pub cpus: usize,
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub on_battery: bool,
    pub battery_percent: Option<f32>,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Tier {
    Full,
    Balanced,
    Saver,
    Manual,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub tier: Tier,
    pub io_threads: usize,
    pub index_threads: usize,
    pub thumb_threads: usize,
    pub cache_mb: usize,
    /// Cap on indexed entries, derived from the cache budget.
    pub index_max_entries: usize,
    /// Number of directory listings kept in memory.
    pub listing_cache_dirs: usize,
    pub background_indexing: bool,
    pub prefetch: bool,
    pub thumbnails: bool,
}

/// Rough bytes per index entry: 40-byte node plus two copies of an average name.
const BYTES_PER_ENTRY: usize = 96;

impl Budget {
    /// The smallest sensible budget; also a convenient base for tests.
    pub fn minimal() -> Budget {
        Budget {
            tier: Tier::Saver,
            io_threads: 2,
            index_threads: 1,
            thumb_threads: 1,
            cache_mb: 32,
            index_max_entries: 32 * 1024 * 1024 * 3 / 4 / BYTES_PER_ENTRY,
            listing_cache_dirs: 32,
            background_indexing: false,
            prefetch: false,
            thumbnails: true,
        }
    }
}

pub fn compute(perf: &Performance, sys: &SystemSnapshot) -> Budget {
    let cpus = sys.cpus.max(1);
    let user_threads = if perf.max_threads == 0 { cpus } else { perf.max_threads };

    let (tier, io, index, thumbs, cache_mb, bg, prefetch) = if perf.priority == "manual" {
        (
            Tier::Manual,
            user_threads,
            user_threads,
            (user_threads / 2).max(1),
            perf.max_cache_mb,
            true,
            true,
        )
    } else {
        let low_battery = sys.on_battery && sys.battery_percent.map(|p| p < 20.0).unwrap_or(false);
        let constrained = sys.on_battery && perf.reduce_on_battery;
        let tier = if low_battery && perf.reduce_on_battery || sys.available_ram_mb < 1024 || cpus <= 2 {
            Tier::Saver
        } else if constrained || sys.total_ram_mb < 8 * 1024 || cpus <= 4 {
            Tier::Balanced
        } else {
            Tier::Full
        };
        let pct = |p: u64| (sys.total_ram_mb * p / 100) as usize;
        match tier {
            Tier::Full => (tier, cpus.min(16), cpus, (cpus / 2).max(2), pct(5), true, true),
            Tier::Balanced => (tier, (cpus / 2).max(2), (cpus / 2).max(1), 2, pct(2), !constrained, true),
            _ => (tier, 2, 1, 1, 64, false, false),
        }
    };

    // The user's limits are always an upper bound.
    let io = io.min(user_threads).max(1);
    let index = index.min(user_threads).max(1);
    let thumbs = thumbs.min(user_threads).max(1);
    let cache_mb = cache_mb.min(perf.max_cache_mb).max(8);

    Budget {
        tier,
        io_threads: io,
        index_threads: index,
        thumb_threads: thumbs,
        cache_mb,
        index_max_entries: cache_mb * 1024 * 1024 * 3 / 4 / BYTES_PER_ENTRY,
        listing_cache_dirs: if perf.metadata_caching { (cache_mb * 2).clamp(32, 2048) } else { 0 },
        background_indexing: bg && perf.background_indexing,
        prefetch: prefetch && perf.predictive_prefetch && perf.metadata_caching,
        thumbnails: perf.thumbnails,
    }
}

/// Read the current machine state.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn snapshot() -> SystemSnapshot {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let (on_battery, battery_percent) = battery_state();
    SystemSnapshot {
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2),
        total_ram_mb: sys.total_memory() / (1024 * 1024),
        available_ram_mb: sys.available_memory() / (1024 * 1024),
        on_battery,
        battery_percent,
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn battery_state() -> (bool, Option<f32>) {
    use starship_battery::{Manager, State};
    let Ok(manager) = Manager::new() else { return (false, None) };
    let Ok(mut batteries) = manager.batteries() else { return (false, None) };
    match batteries.next() {
        Some(Ok(b)) => {
            let pct = b.state_of_charge().value * 100.0;
            (matches!(b.state(), State::Discharging), Some(pct))
        }
        _ => (false, None),
    }
}

/// Mobile: assume a battery-powered, memory-constrained device until the
/// native battery and memory APIs are wired in (see docs/ROADMAP.md).
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn snapshot() -> SystemSnapshot {
    SystemSnapshot {
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2),
        total_ram_mb: 4096,
        available_ram_mb: 1024,
        on_battery: true,
        battery_percent: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys(cpus: usize, ram_gb: u64, on_battery: bool, pct: Option<f32>) -> SystemSnapshot {
        SystemSnapshot {
            cpus,
            total_ram_mb: ram_gb * 1024,
            available_ram_mb: ram_gb * 1024 / 2,
            on_battery,
            battery_percent: pct,
        }
    }

    #[test]
    fn desktop_on_mains_gets_full_budget() {
        let b = compute(&Performance::default(), &sys(16, 32, false, None));
        assert_eq!(b.tier, Tier::Full);
        assert_eq!(b.io_threads, 16);
        assert_eq!(b.cache_mb, 256); // capped by the user's maxCacheMb
        assert!(b.background_indexing && b.prefetch);
    }

    #[test]
    fn laptop_on_battery_is_reduced_and_low_battery_is_minimal() {
        let b = compute(&Performance::default(), &sys(8, 16, true, Some(70.0)));
        assert_eq!(b.tier, Tier::Balanced);
        assert_eq!(b.io_threads, 4);
        assert!(!b.background_indexing);

        let b = compute(&Performance::default(), &sys(8, 16, true, Some(10.0)));
        assert_eq!(b.tier, Tier::Saver);
        assert!(!b.prefetch && !b.background_indexing);
        assert_eq!(b.cache_mb, 64);

        // The user can opt out of battery throttling.
        let perf = Performance { reduce_on_battery: false, ..Default::default() };
        assert_eq!(compute(&perf, &sys(8, 16, true, Some(10.0))).tier, Tier::Full);
    }

    #[test]
    fn manual_mode_and_user_caps_are_respected() {
        let perf = Performance { priority: "manual".into(), max_threads: 3, max_cache_mb: 1024, ..Default::default() };
        let b = compute(&perf, &sys(2, 4, true, Some(5.0)));
        assert_eq!(b.tier, Tier::Manual);
        assert_eq!(b.io_threads, 3);
        assert_eq!(b.cache_mb, 1024);

        let perf = Performance { max_threads: 2, metadata_caching: false, ..Default::default() };
        let b = compute(&perf, &sys(16, 32, false, None));
        assert_eq!(b.io_threads, 2);
        assert_eq!(b.listing_cache_dirs, 0);
        assert!(!b.prefetch);
    }

    #[test]
    fn snapshot_reads_something_plausible() {
        let s = snapshot();
        assert!(s.cpus >= 1);
        assert!(s.total_ram_mb > 0);
    }
}
