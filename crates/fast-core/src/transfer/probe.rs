//! A quick speed test before a big transfer, so the time estimate is useful
//! from the first second instead of after the first files finish.
//!
//! It does what a transfer does in miniature, on the real devices: reads a few
//! megabytes from the largest source file, writes a few megabytes (random data,
//! flushed to the device, so caches and compression cannot flatter the result)
//! and creates a handful of tiny files in a hidden folder next to the
//! destination, which it removes afterwards. It takes a fraction of a second
//! and gives up if the devices are very slow, because by then the numbers it
//! has are already telling.

use super::eta::EtaParams;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const CHUNK: usize = 1 << 20;
const DATA_MB: usize = 8;
const SMALL_FILES: usize = 24;
const BUDGET: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy)]
pub struct Measured {
    /// Seconds to read one megabyte from the source, if it could be read.
    pub read_secs_per_mb: Option<f64>,
    pub write_secs_per_mb: f64,
    /// Seconds to create, fill and close one small file.
    pub per_file_secs: f64,
    pub took: Duration,
}

impl Measured {
    /// Starting costs for the time estimate. `streams` is how many files the
    /// engine will have in flight. On devices where more streams do not add
    /// bandwidth (a network link, a spinning disk) the per-megabyte cost is
    /// scaled so that dividing by the streams gives back the measured rate;
    /// elsewhere two streams are assumed to be twice as fast. Real results
    /// replace this within the first few files.
    pub fn params(&self, streams: usize, shared_bandwidth: bool) -> EtaParams {
        let one_stream = self.read_secs_per_mb.unwrap_or(0.0) + self.write_secs_per_mb;
        let streams = streams.max(1) as f64;
        let scale = if shared_bandwidth { streams } else { (streams / 2.0).max(1.0) };
        EtaParams { per_file_secs: self.per_file_secs, per_mb_secs: one_stream * scale }
    }
}

fn noise(len: usize) -> Vec<u8> {
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15 ^ len as u64;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x >> 24) as u8
        })
        .collect()
}

/// Measure the route. Returns `None` if the destination cannot be written
/// to (the transfer itself will then report the real problem).
pub fn run(source_file: Option<&Path>, dest_dir: &Path) -> Option<Measured> {
    let started = Instant::now();
    let dir = dest_dir.join(format!(".fast-probe-{}-{}", std::process::id(), started.elapsed().as_nanos() ^ rand_tag()));
    fs::create_dir(&dir).ok()?;
    let result = measure(source_file, &dir, started);
    let _ = fs::remove_dir_all(&dir);
    result.map(|mut m| {
        m.took = started.elapsed();
        m
    })
}

/// Like [`run`], but never waits longer than `limit` or past a cancel. A drive that is slow to answer (a
/// network share, a disk that went to sleep) can hold any one of the test's calls for far longer than its
/// time budget, and the copy must not wait for a test. The test carries on by itself on its own thread and
/// removes its folder when the drive answers; its result is simply not used.
pub fn run_bounded(source_file: Option<&Path>, dest_dir: &Path, limit: Duration, cancelled: impl Fn() -> bool) -> Option<Measured> {
    let (tx, rx) = std::sync::mpsc::channel();
    let (source, dest) = (source_file.map(Path::to_path_buf), dest_dir.to_path_buf());
    std::thread::Builder::new()
        .name("speed-test".into())
        .spawn(move || {
            let _ = tx.send(run(source.as_deref(), &dest));
        })
        .ok()?;
    let started = Instant::now();
    loop {
        // A cancel wins even over a test that happens to finish in the same moment.
        if cancelled() {
            return None;
        }
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if started.elapsed() >= limit {
                    return None;
                }
            }
        }
    }
}

fn rand_tag() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn measure(source_file: Option<&Path>, dir: &Path, started: Instant) -> Option<Measured> {
    // Reading: from the largest source file, up to DATA_MB.
    let read_secs_per_mb = source_file.and_then(|p| {
        let mut f = File::open(p).ok()?;
        let mut buf = vec![0u8; CHUNK];
        let t = Instant::now();
        let mut got = 0usize;
        while got < DATA_MB * CHUNK && started.elapsed() < BUDGET {
            match f.read(&mut buf).ok()? {
                0 => break,
                n => got += n,
            }
        }
        (got >= CHUNK / 4).then(|| t.elapsed().as_secs_f64() / (got as f64 / CHUNK as f64))
    });

    // Writing: random data, flushed.
    let data = noise(CHUNK);
    let t = Instant::now();
    let mut f = File::create(dir.join("w.bin")).ok()?;
    let mut wrote = 0usize;
    for _ in 0..DATA_MB {
        f.write_all(&data).ok()?;
        wrote += CHUNK;
        if started.elapsed() > BUDGET {
            break;
        }
    }
    f.sync_all().ok()?;
    drop(f);
    let write_secs_per_mb = (t.elapsed().as_secs_f64() / (wrote as f64 / CHUNK as f64)).max(1e-6);

    // Per-file cost: create, write a little, close.
    let t = Instant::now();
    let mut made = 0usize;
    for i in 0..SMALL_FILES {
        if started.elapsed() > BUDGET + Duration::from_millis(500) {
            break;
        }
        let mut f = File::create(dir.join(format!("s{i}.tmp"))).ok()?;
        f.write_all(&data[..1024]).ok()?;
        made += 1;
    }
    if made == 0 {
        return None;
    }
    let per_file_secs = t.elapsed().as_secs_f64() / made as f64;
    Some(Measured { read_secs_per_mb, write_secs_per_mb, per_file_secs, took: Duration::ZERO })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_cleans_up() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        let big = src.path().join("big.bin");
        fs::write(&big, vec![1u8; 3 << 20]).unwrap();
        let m = run(Some(&big), dst.path()).expect("probe runs");
        assert!(m.read_secs_per_mb.unwrap() > 0.0);
        assert!(m.write_secs_per_mb > 0.0 && m.per_file_secs > 0.0);
        assert!(m.took < Duration::from_secs(5));
        // Nothing left behind.
        assert_eq!(fs::read_dir(dst.path()).unwrap().count(), 0);
        let p = m.params(4, true);
        assert!(p.per_mb_secs > m.params(4, false).per_mb_secs);
    }

    #[test]
    fn a_bounded_test_gives_up_when_cancelled() {
        let dst = tempfile::tempdir().unwrap();
        let t = Instant::now();
        assert!(run_bounded(None, dst.path(), Duration::from_secs(5), || true).is_none());
        assert!(t.elapsed() < Duration::from_secs(1));
        // It still cleans up after itself.
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(fs::read_dir(dst.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_bounded_test_returns_the_measurement_when_quick() {
        let dst = tempfile::tempdir().unwrap();
        assert!(run_bounded(None, dst.path(), Duration::from_secs(10), || false).is_some());
    }

    #[test]
    fn unwritable_destination_is_not_an_error() {
        assert!(run(None, Path::new("/definitely/not/here")).is_none());
    }
}
