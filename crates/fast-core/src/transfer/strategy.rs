//! Chooses how to run a transfer from the shape of the job and the devices
//! involved. No single method wins everywhere:
//!
//! * Same-volume moves are a rename: instant, no data copied.
//! * Same-volume copies try a copy-on-write clone first (APFS, Btrfs, XFS,
//!   ReFS): instant and uses no extra space.
//! * Large files want few streams and big buffers; parallelism only adds
//!   seeking on hard disks and contention on SSDs.
//! * Many small files are dominated by per-file latency, so they want many
//!   workers, especially over a network where each file costs round trips.
//! * Mixed jobs get both: a wide pool for small files and a narrow lane for
//!   large ones, so big files never starve the small-file workers.

use crate::drives::Media;
use crate::resources::Budget;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    Rename,
    LargeSequential,
    SmallParallel,
    Mixed,
}

#[derive(Clone, Debug)]
pub struct JobProfile {
    pub file_count: u64,
    pub total_bytes: u64,
    /// Bytes held in files at or above the large-file threshold.
    pub large_bytes: u64,
    pub large_count: u64,
    pub same_volume: bool,
    pub is_move: bool,
    pub src: Media,
    pub dst: Media,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub strategy: Strategy,
    pub small_workers: usize,
    pub large_workers: usize,
    pub buffer_bytes: usize,
    pub large_threshold: u64,
    /// Attempt copy-on-write clones before copying data.
    pub try_clone: bool,
}

pub const DEFAULT_LARGE_THRESHOLD: u64 = 32 * 1024 * 1024;

pub fn choose(p: &JobProfile, budget: &Budget, large_threshold: u64, max_parallel: usize) -> Plan {
    let cap = if max_parallel > 0 { max_parallel } else { budget.io_threads.max(1) };
    let hdd = p.src == Media::Hdd || p.dst == Media::Hdd;
    let network = p.src == Media::Network || p.dst == Media::Network;

    // Device ceilings.
    let (small_max, large_max, buffer) = if hdd {
        // A spinning disk has one head: parallel streams just cause seeking.
        (2, 1, 4 << 20)
    } else if network {
        // Latency-bound: many small requests in flight help a lot; large
        // streams are bandwidth-bound so a couple is enough.
        (16, 2, 1 << 20)
    } else {
        (cap.max(4) * 2, 2, 8 << 20)
    };
    let small_workers = small_max.min(cap * 2).max(1);
    let large_workers = large_max.min(cap).max(1);
    let try_clone = p.same_volume && !p.is_move;

    if p.is_move && p.same_volume {
        return Plan {
            strategy: Strategy::Rename,
            small_workers: 1,
            large_workers: 1,
            buffer_bytes: buffer,
            large_threshold,
            try_clone: false,
        };
    }

    let large_share = if p.total_bytes > 0 { p.large_bytes as f64 / p.total_bytes as f64 } else { 0.0 };
    let small_count = p.file_count - p.large_count;

    let strategy = if p.file_count <= 4 || (large_share > 0.9 && small_count < 64) {
        Strategy::LargeSequential
    } else if p.large_count == 0 || large_share < 0.1 {
        Strategy::SmallParallel
    } else {
        Strategy::Mixed
    };

    let (small_workers, large_workers) = match strategy {
        Strategy::LargeSequential => (1, large_workers.min(p.file_count.max(1) as usize)),
        Strategy::SmallParallel => (small_workers, 1),
        _ => (small_workers, large_workers),
    };

    Plan { strategy, small_workers, large_workers, buffer_bytes: buffer, large_threshold, try_clone }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> Budget {
        Budget { io_threads: 8, ..Budget::minimal() }
    }
    fn profile(files: u64, total: u64, large: u64, large_count: u64) -> JobProfile {
        JobProfile {
            file_count: files, total_bytes: total, large_bytes: large, large_count,
            same_volume: false, is_move: false, src: Media::Ssd, dst: Media::Ssd,
        }
    }
    const GB: u64 = 1 << 30;

    #[test]
    fn picks_by_job_shape() {
        let t = DEFAULT_LARGE_THRESHOLD;
        let one_big = choose(&profile(2, 20 * GB, 20 * GB, 2), &budget(), t, 0);
        assert_eq!(one_big.strategy, Strategy::LargeSequential);
        assert_eq!(one_big.large_workers, 2);

        let many_small = choose(&profile(50_000, GB, 0, 0), &budget(), t, 0);
        assert_eq!(many_small.strategy, Strategy::SmallParallel);
        assert!(many_small.small_workers >= 8);

        let mixed = choose(&profile(5_000, 30 * GB, 20 * GB, 20), &budget(), t, 0);
        assert_eq!(mixed.strategy, Strategy::Mixed);
        assert!(mixed.small_workers > mixed.large_workers);
    }

    #[test]
    fn respects_devices_and_limits() {
        let t = DEFAULT_LARGE_THRESHOLD;
        let mut p = profile(50_000, GB, 0, 0);
        p.dst = Media::Hdd;
        assert_eq!(choose(&p, &budget(), t, 0).small_workers, 2);

        p.dst = Media::Network;
        let net = choose(&p, &budget(), t, 0);
        assert_eq!(net.small_workers, 16);
        assert_eq!(net.buffer_bytes, 1 << 20);

        p.dst = Media::Ssd;
        assert_eq!(choose(&p, &budget(), t, 1).small_workers, 2);

        p.same_volume = true;
        assert!(choose(&p, &budget(), t, 0).try_clone);
        p.is_move = true;
        assert_eq!(choose(&p, &budget(), t, 0).strategy, Strategy::Rename);
    }
}
