//! Live folder watching: tells the app when a folder it is showing changed
//! on disk (another program saved a file, a download finished, a USB stick
//! was emptied), so the listing can refresh without pressing F5.
//!
//! Only the folders that are open are watched, and not recursively, so the
//! cost stays tiny however large the drive is. Bursts of events (a build
//! writing thousands of files) are merged: the callback gets each folder
//! once per quiet moment, never once per file.

use std::path::PathBuf;

/// How long a folder must stay quiet before it is reported.
pub const QUIET: std::time::Duration = std::time::Duration::from_millis(200);
/// A folder that never goes quiet is still reported this often.
pub const AT_MOST: std::time::Duration = std::time::Duration::from_secs(2);
/// More watches than this are not worth their kernel resources.
pub const MAX_DIRS: usize = 48;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod imp {
    use super::*;
    use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
    use parking_lot::Mutex;
    use std::collections::HashSet;
    use std::sync::{mpsc, Arc};

    pub struct Watcher {
        /// The folders being watched; the event callback reads this too.
        watched: Arc<Mutex<HashSet<PathBuf>>>,
        os: Mutex<Option<RecommendedWatcher>>,
    }

    /// The watched folder an event belongs to: the folder itself, or the
    /// folder holding the changed item.
    fn owner(path: &std::path::Path, dirs: &HashSet<PathBuf>) -> Option<PathBuf> {
        if dirs.contains(path) {
            return Some(path.to_path_buf());
        }
        path.parent().filter(|p| dirs.contains(*p)).map(|p| p.to_path_buf())
    }

    impl Watcher {
        /// `on_change` runs on a helper thread with the folders that changed.
        pub fn new(on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Watcher {
            let (tx, rx) = mpsc::channel::<PathBuf>();
            let watched = Arc::new(Mutex::new(HashSet::<PathBuf>::new()));
            let for_events = watched.clone();
            let os = notify::recommended_watcher(move |res: notify::Result<Event>| {
                let Ok(event) = res else { return };
                // Merely reading a file is not a change (and would loop with
                // the app's own listing).
                if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
                    return;
                }
                let dirs = for_events.lock();
                for p in &event.paths {
                    if let Some(d) = owner(p, &dirs) {
                        let _ = tx.send(d);
                    }
                }
            })
            .ok();

            let spawned = std::thread::Builder::new().name("watch-debounce".into()).spawn(move || {
                // Ends when the OS watcher (and with it the sender) is dropped.
                while let Ok(first) = rx.recv() {
                    let mut pending: HashSet<PathBuf> = HashSet::from([first]);
                    let began = std::time::Instant::now();
                    loop {
                        let left = AT_MOST.saturating_sub(began.elapsed());
                        if left.is_zero() {
                            break;
                        }
                        match rx.recv_timeout(QUIET.min(left)) {
                            Ok(d) => {
                                pending.insert(d);
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => break,
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    on_change(pending.into_iter().collect());
                }
            });
            // Without the helper thread nothing could be reported.
            Watcher { watched, os: Mutex::new(if spawned.is_ok() { os } else { None }) }
        }

        /// Watch exactly these folders (at most `MAX_DIRS`) and stop watching
        /// any others. Folders that cannot be watched are skipped quietly:
        /// the app then simply does not refresh them by itself.
        pub fn set_dirs(&self, wanted: Vec<PathBuf>) {
            let wanted: HashSet<PathBuf> = wanted.into_iter().take(MAX_DIRS).collect();
            let mut os = self.os.lock();
            let Some(os) = os.as_mut() else { return };
            let mut watched = self.watched.lock();
            for gone in watched.difference(&wanted).cloned().collect::<Vec<_>>() {
                let _ = os.unwatch(&gone);
                watched.remove(&gone);
            }
            for new in wanted.difference(&watched.clone()) {
                if os.watch(new, RecursiveMode::NonRecursive).is_ok() {
                    watched.insert(new.clone());
                }
            }
        }

        pub fn watching(&self) -> usize {
            self.watched.lock().len()
        }
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::*;
    pub struct Watcher;
    impl Watcher {
        pub fn new(_on_change: impl Fn(Vec<PathBuf>) + Send + 'static) -> Watcher {
            Watcher
        }
        pub fn set_dirs(&self, _wanted: Vec<PathBuf>) {}
        pub fn watching(&self) -> usize {
            0
        }
    }
}

pub use imp::Watcher;

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn watcher() -> (Watcher, mpsc::Receiver<Vec<PathBuf>>) {
        let (tx, rx) = mpsc::channel();
        let w = Watcher::new(move |dirs| {
            let _ = tx.send(dirs);
        });
        (w, rx)
    }

    #[test]
    fn reports_a_new_file_in_a_watched_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let (w, rx) = watcher();
        w.set_dirs(vec![dir.clone()]);
        assert_eq!(w.watching(), 1);
        std::fs::write(dir.join("new.txt"), b"x").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("a change is reported");
        assert_eq!(got, vec![dir]);
    }

    #[test]
    fn a_burst_of_files_is_reported_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let (w, rx) = watcher();
        w.set_dirs(vec![dir.clone()]);
        for i in 0..200 {
            std::fs::write(dir.join(format!("f{i}.txt")), b"x").unwrap();
        }
        let first = rx.recv_timeout(Duration::from_secs(5)).expect("reported");
        assert_eq!(first, vec![dir]);
        // Nothing more follows once the burst is over.
        assert!(rx.recv_timeout(Duration::from_millis(700)).is_err());
    }

    #[test]
    fn folders_that_are_no_longer_open_are_not_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let (w, rx) = watcher();
        w.set_dirs(vec![dir.clone()]);
        w.set_dirs(vec![]);
        assert_eq!(w.watching(), 0);
        std::fs::write(dir.join("late.txt"), b"x").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
    }

    #[test]
    fn missing_folders_are_skipped_without_failing() {
        let (w, _rx) = watcher();
        w.set_dirs(vec![PathBuf::from("/definitely/not/here")]);
        assert_eq!(w.watching(), 0);
    }

    #[test]
    fn the_cap_limits_how_many_folders_are_watched() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs: Vec<PathBuf> = (0..MAX_DIRS + 10)
            .map(|i| {
                let d = tmp.path().join(format!("d{i}"));
                std::fs::create_dir(&d).unwrap();
                d
            })
            .collect();
        let (w, _rx) = watcher();
        w.set_dirs(dirs);
        assert!(w.watching() <= MAX_DIRS);
    }
}
