//! How busy one drive is right now: bytes read and written per second.
//!
//! Used by the disk-usage view to draw a live graph while a scan runs. The
//! operating system reports the bytes moved since the last look, so a sampler
//! divides by the time between looks. It reports raw throughput (not a
//! percentage): the view compares it with the fastest it has seen.

use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IoSample {
    pub read_bps: f64,
    pub write_bps: f64,
}

/// The index of the mount that holds `root`: the longest mount point that is a parent of it.
pub fn pick_mount(mounts: &[PathBuf], root: &Path) -> Option<usize> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, m)| root.starts_with(m))
        .max_by_key(|(_, m)| m.components().count())
        .map(|(i, _)| i)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub struct Sampler {
    disks: sysinfo::Disks,
    index: usize,
    last: Instant,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Sampler {
    /// A sampler for the drive that holds `root`, or `None` when it is not one the system lists.
    pub fn new(root: &Path) -> Option<Sampler> {
        let disks = sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing().with_io_usage());
        let mounts: Vec<PathBuf> = disks.list().iter().map(|d| d.mount_point().to_path_buf()).collect();
        let index = pick_mount(&mounts, root)?;
        Some(Sampler { disks, index, last: Instant::now() })
    }

    /// Throughput since the previous call (or since `new`).
    pub fn sample(&mut self) -> IoSample {
        self.disks.refresh_specifics(false, sysinfo::DiskRefreshKind::nothing().with_io_usage());
        let secs = self.last.elapsed().as_secs_f64().max(0.001);
        self.last = Instant::now();
        let Some(d) = self.disks.list().get(self.index) else { return IoSample::default() };
        let u = d.usage();
        IoSample { read_bps: u.read_bytes as f64 / secs, write_bps: u.written_bytes as f64 / secs }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub struct Sampler;

#[cfg(any(target_os = "android", target_os = "ios"))]
impl Sampler {
    pub fn new(_root: &Path) -> Option<Sampler> {
        None
    }
    pub fn sample(&mut self) -> IoSample {
        IoSample::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_matching_mount_wins() {
        let mounts: Vec<PathBuf> = ["/", "/home", "/home/me/data", "/mnt/x"].iter().map(PathBuf::from).collect();
        assert_eq!(pick_mount(&mounts, Path::new("/home/me/data/photos")), Some(2));
        assert_eq!(pick_mount(&mounts, Path::new("/home/other")), Some(1));
        assert_eq!(pick_mount(&mounts, Path::new("/etc")), Some(0));
        assert_eq!(pick_mount(&mounts, Path::new("/mnt/xy")), Some(0), "xy is not inside /mnt/x");
        assert_eq!(pick_mount(&[], Path::new("/a")), None);
    }

    #[test]
    fn a_sampler_for_this_machine_reports_something_sane() {
        let Some(mut s) = Sampler::new(Path::new("/")) else { return };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let a = s.sample();
        assert!(a.read_bps >= 0.0 && a.write_bps >= 0.0 && a.read_bps.is_finite());
    }
}
