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
    pub slow_thumb_threads: usize,
    pub cache_mb: usize,
    /// Cap on indexed entries, derived from the cache budget.
    pub index_max_entries: usize,
    /// Number of directory listings kept in memory.
    pub listing_cache_dirs: usize,
    pub background_indexing: bool,
    pub prefetch: bool,
    pub thumbnails: bool,
    /// How often a running transfer reports progress, in milliseconds. Slower
    /// on weak machines and laptops, where redrawing costs real battery and CPU.
    pub progress_ms: u64,
    /// Max background thumbnail prewarm items (0 = unlimited).
    pub prewarm_limit: usize,
    /// Also prewarm picture thumbnails ahead of scroll on high tiers.
    pub prewarm_images: bool,
    /// Max folders prefetched speculatively in one batch.
    pub prefetch_limit: usize,
    /// Max folders inspected for emptiness per batch (0 = unlimited).
    pub folder_fill_limit: usize,
    /// Largest directory streaming batch before emitting IPC progress.
    pub dir_batch_max: usize,
    /// Normalized active resource level (0 = Barebones/Saver .. 100 = God-like/uncapped).
    pub intensity: u8,
    /// The app is minimized or behind other windows and is running on a reduced budget.
    pub background: bool,
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
            slow_thumb_threads: 1,
            cache_mb: 32,
            index_max_entries: 32 * 1024 * 1024 * 3 / 4 / BYTES_PER_ENTRY,
            listing_cache_dirs: 32,
            background_indexing: false,
            prefetch: false,
            thumbnails: true,
            progress_ms: 500,
            prewarm_limit: 64,
            prewarm_images: false,
            prefetch_limit: 1,
            folder_fill_limit: 24,
            dir_batch_max: 512,
            intensity: 10,
            background: false,
        }
    }
}

/// How far from the person's attention the window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Attention {
    /// In front, or at least in use.
    Active,
    /// Visible but another window has the focus.
    Behind,
    /// Minimized or hidden.
    Away,
}

/// The budget while the window is out of the way. "gentle" acts only when the window is minimized or
/// hidden and halves the work; "aggressive" also acts when it is merely behind other windows and drops
/// to the minimum: one thread each, no thumbnails, indexing or preloading, a small cache, slow progress.
/// Running transfers are never slowed, only how often they report.
pub fn dimmed(b: &Budget, mode: &str, attention: Attention) -> Budget {
    let mut d = b.clone();
    match (mode, attention) {
        (_, Attention::Active) | ("off", _) => return d,
        ("gentle", Attention::Behind) => return d,
        ("gentle", Attention::Away) => {
            d.io_threads = (d.io_threads / 2).max(1);
            d.index_threads = (d.index_threads / 2).max(1);
            d.thumb_threads = 1;
            d.slow_thumb_threads = 1;
            d.cache_mb = (d.cache_mb / 2).max(8);
            d.prefetch = false;
            d.prefetch_limit = 0;
            d.prewarm_images = false;
            d.prewarm_limit = d.prewarm_limit.min(32);
            d.progress_ms = d.progress_ms.max(1000);
            d.intensity = (d.intensity / 2).max(10);
        }
        _ => {
            d.io_threads = 1;
            d.index_threads = 1;
            d.thumb_threads = 1;
            d.slow_thumb_threads = 1;
            d.cache_mb = d.cache_mb.min(32);
            d.listing_cache_dirs = d.listing_cache_dirs.min(32);
            d.background_indexing = false;
            d.prefetch = false;
            d.prefetch_limit = 0;
            // Only out of sight are thumbnails given up. Behind another window the picture is still on screen (and a
            // system permission dialog counts as "another window"): turning them off there left whole folders without
            // them, and they were not asked for again.
            d.thumbnails = d.thumbnails && attention != Attention::Away;
            d.prewarm_images = false;
            d.prewarm_limit = 0;
            d.folder_fill_limit = 16;
            d.progress_ms = d.progress_ms.max(if attention == Attention::Away { 2000 } else { 1000 });
            d.intensity = 5;
        }
    }
    if d.index_max_entries != 0 {
        d.index_max_entries = d.index_max_entries.min(d.cache_mb * 1024 * 1024 * 3 / 4 / BYTES_PER_ENTRY);
    }
    d.background = true;
    d
}

pub fn compute(perf: &Performance, sys: &SystemSnapshot) -> Budget {
    let cpus = sys.cpus.max(1);
    // The CPU share caps the number of worker threads; the thread limit caps it further.
    let cpu_cap = match perf.max_cpu_percent {
        0 => usize::MAX,
        p => (cpus * (p.min(100) as usize)).div_ceil(100).max(1),
    };
    let user_threads = (if perf.max_threads == 0 { cpus } else { perf.max_threads }).min(cpu_cap);
    // The memory share and the cache limit both bound the caches; 0 means none.
    let mem_cap = match perf.max_memory_percent {
        0 => usize::MAX,
        p => ((sys.total_ram_mb as usize) * (p.min(100) as usize) / 100).max(8),
    };
    let cache_cap = if perf.max_cache_mb == 0 { usize::MAX } else { perf.max_cache_mb }.min(mem_cap);
    let uncapped = cache_cap == usize::MAX && perf.priority == "manual";
    let full_cpu_uncapped = uncapped && perf.max_threads == 0 && perf.max_cpu_percent == 0;

    let (tier, io, index, thumbs_raw, cache_mb, bg, prefetch, base_intensity) = if perf.priority == "manual" {
        // Estimate tier intensity cleanly from manual thread/cache targets so higher tiers
        // actively scale worker pools, batching, prewarm, and GPU flags up to God-like (100).
        let intensity = if full_cpu_uncapped {
            100
        } else {
            let thread_score = ((user_threads * 50) / cpus).min(50);
            let cache_score = if cache_cap == usize::MAX {
                50
            } else {
                match cache_cap {
                    0..=32 => 5,
                    33..=128 => 15,
                    129..=512 => 28,
                    513..=1024 => 36,
                    1025..=2048 => 42,
                    2049..=3072 => 45,
                    _ => 48,
                }
            };
            (thread_score + cache_score).clamp(5, 99) as u8
        };
        let thumb_target = if full_cpu_uncapped || intensity >= 90 {
            user_threads
        } else if intensity >= 70 {
            (user_threads * 3 / 4).max(2)
        } else {
            (user_threads / 2).max(1)
        };
        (
            Tier::Manual,
            user_threads,
            user_threads,
            thumb_target,
            // With no cap the caches may take half of installed memory, and the index has no entry limit.
            if uncapped { (sys.total_ram_mb as usize / 2).max(512) } else { cache_cap },
            true,
            true,
            intensity,
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
            Tier::Full => (tier, cpus.min(32), cpus, (cpus * 3 / 4).max(2), pct(10).max(256), true, true, 80),
            Tier::Balanced => (tier, (cpus / 2).max(2), (cpus / 2).max(1), 2, pct(2), !constrained, true, 45),
            _ => (tier, 2, 1, 1, 64, false, false, 15),
        }
    };

    // The user's limits are always an upper bound.
    let io = io.min(user_threads).max(1);
    let index = index.min(user_threads).max(1);
    let thumbs = thumbs_raw.min(user_threads).max(1);
    let slow_thumbs = if base_intensity >= 95 {
        thumbs
    } else if base_intensity >= 70 {
        thumbs.clamp(2, 16)
    } else if base_intensity >= 40 {
        thumbs.clamp(2, 6)
    } else {
        1
    };
    let cache_mb = cache_mb.min(cache_cap).max(8);
    let prefetch_on = prefetch && perf.predictive_prefetch && perf.metadata_caching;

    Budget {
        tier,
        io_threads: io,
        index_threads: index,
        thumb_threads: thumbs,
        slow_thumb_threads: slow_thumbs,
        cache_mb,
        // 0 = no limit on how many entries the index holds.
        index_max_entries: if uncapped { 0 } else { cache_mb * 1024 * 1024 * 3 / 4 / BYTES_PER_ENTRY },
        listing_cache_dirs: if perf.metadata_caching {
            (cache_mb * 2).clamp(32, if uncapped { 65_536 } else { 8192 })
        } else {
            0
        },
        background_indexing: bg && perf.background_indexing,
        prefetch: prefetch_on,
        thumbnails: perf.thumbnails,
        progress_ms: if base_intensity >= 95 {
            16
        } else if base_intensity >= 75 {
            33
        } else {
            match tier {
                Tier::Full | Tier::Manual => 100,
                Tier::Balanced => 250,
                Tier::Saver => 500,
            }
        },
        prewarm_limit: if base_intensity >= 95 {
            0 // unlimited: prewarm the full folder
        } else if base_intensity >= 75 {
            1500
        } else if base_intensity >= 45 {
            300
        } else {
            64
        },
        prewarm_images: perf.thumbnails && base_intensity >= 75,
        prefetch_limit: if !prefetch_on {
            0
        } else if base_intensity >= 95 {
            64
        } else if base_intensity >= 75 {
            24
        } else if base_intensity >= 45 {
            8
        } else {
            2
        },
        folder_fill_limit: if base_intensity >= 95 {
            0 // unlimited
        } else if base_intensity >= 75 {
            512
        } else if base_intensity >= 45 {
            192
        } else {
            48
        },
        dir_batch_max: if base_intensity >= 95 {
            16_384
        } else if base_intensity >= 75 {
            8_192
        } else if base_intensity >= 45 {
            2_048
        } else {
            512
        },
        intensity: base_intensity,
        background: false,
    }
}

/// Does this machine have power to spare for extras that only look nice: live
/// graphs and the like? Plenty of cores and memory, on the Full or Manual
/// budget, and not running on battery.
pub fn has_spare_power(budget: &Budget, sys: &SystemSnapshot) -> bool {
    matches!(budget.tier, Tier::Full | Tier::Manual) && sys.cpus >= 8 && sys.total_ram_mb >= 8 * 1024 && !sys.on_battery
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

/// Phones and tablets: memory from the system, and the battery when the system shows it
/// to apps. When it does not, the device counts as running on battery, the careful guess.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn snapshot() -> SystemSnapshot {
    let (total_ram_mb, available_ram_mb) = crate::phone::memory_mb().unwrap_or((4096, 1024));
    let (on_battery, battery_percent) = crate::phone::battery_in(std::path::Path::new("/sys/class/power_supply"));
    SystemSnapshot {
        cpus: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2),
        total_ram_mb,
        available_ram_mb,
        on_battery,
        battery_percent,
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
    fn cpu_and_memory_shares_cap_threads_and_caches() {
        let manual = |cpu, mem, cache| Performance { priority: "manual".into(), max_cpu_percent: cpu, max_memory_percent: mem, max_cache_mb: cache, ..Default::default() };
        // 25% of 16 CPUs is 4 threads; 10% of 32 GB is about 3.2 GB.
        let b = compute(&manual(25, 10, 100_000), &sys(16, 32, false, None));
        assert_eq!((b.io_threads, b.index_threads), (4, 4));
        assert_eq!(b.cache_mb, 32 * 1024 / 10);
        // The lower of the share and the explicit limit wins.
        assert_eq!(compute(&manual(0, 10, 512), &sys(16, 32, false, None)).cache_mb, 512);
        // Never fewer than one thread, however small the share.
        assert_eq!(compute(&manual(1, 0, 256), &sys(16, 32, false, None)).io_threads, 1);
        // It also holds in automatic mode.
        let auto = Performance { max_cpu_percent: 50, ..Default::default() };
        assert!(compute(&auto, &sys(16, 32, false, None)).io_threads <= 8);
    }

    #[test]
    fn no_caps_means_no_limits() {
        let perf = Performance { priority: "manual".into(), max_threads: 0, max_cache_mb: 0, ..Default::default() };
        let b = compute(&perf, &sys(32, 64, false, None));
        assert_eq!(b.io_threads, 32);
        assert_eq!(b.thumb_threads, 32);
        assert_eq!(b.slow_thumb_threads, 32);
        assert_eq!(b.index_max_entries, 0, "the index has no entry limit");
        assert_eq!(b.prewarm_limit, 0, "prewarm has no item cap");
        assert_eq!(b.folder_fill_limit, 0, "folder fill has no batch cap");
        assert_eq!(b.cache_mb, 32 * 1024);
        assert_eq!(b.intensity, 100);
        assert_eq!(b.progress_ms, 16);
        assert!(b.prewarm_images);
    }

    #[test]
    fn spare_power_needs_cores_memory_and_mains() {
        let full = |sys: &SystemSnapshot| has_spare_power(&compute(&Performance::default(), sys), sys);
        assert!(full(&sys(16, 32, false, None)));
        assert!(!full(&sys(4, 32, false, None)), "too few cores");
        assert!(!full(&sys(16, 4, false, None)), "too little memory");
        assert!(!full(&sys(16, 32, true, Some(90.0))), "on battery");
        assert!(!has_spare_power(&Budget::minimal(), &sys(16, 32, false, None)), "the minimal budget is not spare");
    }

    #[test]
    fn snapshot_reads_something_plausible() {
        let s = snapshot();
        assert!(s.cpus >= 1);
        assert!(s.total_ram_mb > 0);
    }

    #[test]
    fn background_modes() {
        let b = compute(&Performance::default(), &sys(16, 32, false, None));
        assert_eq!(dimmed(&b, "off", Attention::Away), b);
        assert_eq!(dimmed(&b, "aggressive", Attention::Active), b);
        assert_eq!(dimmed(&b, "gentle", Attention::Behind), b, "gentle waits until minimized");
        let g = dimmed(&b, "gentle", Attention::Away);
        assert!(g.background && g.io_threads == b.io_threads / 2 && !g.prefetch && g.thumbnails);
        let a = dimmed(&b, "aggressive", Attention::Behind);
        assert!(a.background && a.io_threads == 1 && a.thumbnails && !a.background_indexing && a.cache_mb <= 32 && a.progress_ms >= 1000);
        assert!(a.index_max_entries <= b.index_max_entries);
        assert_eq!(dimmed(&b, "aggressive", Attention::Away).progress_ms, 2000);
        assert!(!dimmed(&b, "aggressive", Attention::Away).thumbnails, "out of sight: no thumbnails");
    }
}
