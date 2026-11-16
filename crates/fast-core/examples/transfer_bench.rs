//! Copy speed of the built-in engine against `cp` and `rsync`, on your own disks.
//!
//!     cargo run --release -p fast-core --example transfer_bench -- SRC_DIR DEST_DIR [--runs 3] [--big-mb 512] [--small 20000]
//!
//! SRC_DIR and DEST_DIR are scratch folders on the drives you want to compare
//! (say an SSD and an external HDD, or a mounted network share). The program
//! makes three data sets in SRC_DIR, copies each one to DEST_DIR with each
//! tool, and prints the median time of the runs. Everything it makes is
//! deleted afterwards. Caches are dropped between runs when the system lets
//! it (Linux, as root); otherwise repeat runs are partly served from memory,
//! and the numbers say so.

use fast_core::resources::{self, Budget};
use fast_core::settings::Settings;
use fast_core::transfer::{self, Conflict, Op, Phase};
use fast_core::transfer::engine::TransferSpec;
use fast_core::transfer::method::MethodPref;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Set {
    name: &'static str,
    folder: PathBuf,
    files: u64,
    bytes: u64,
}

fn arg(args: &[String], flag: &str, default: u64) -> u64 {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Cheap deterministic noise, so files do not compress or dedupe away.
fn fill(buf: &mut [u8], seed: &mut u64) {
    for chunk in buf.chunks_mut(8) {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let b = seed.to_le_bytes();
        chunk.copy_from_slice(&b[..chunk.len()]);
    }
}

fn write_file(path: &Path, size: u64, seed: &mut u64) {
    let mut f = fs::File::create(path).expect("create file");
    let mut buf = vec![0u8; 1 << 20];
    let mut left = size;
    while left > 0 {
        let n = left.min(buf.len() as u64) as usize;
        fill(&mut buf[..n], seed);
        f.write_all(&buf[..n]).expect("write");
        left -= n as u64;
    }
}

fn make_sets(src: &Path, small: u64, big_mb: u64) -> Vec<Set> {
    let mut seed = 0x9E3779B97F4A7C15u64;
    let mut sets = Vec::new();

    // Many tiny files: dominated by per-file cost.
    let dir = src.join("small");
    fs::create_dir_all(&dir).unwrap();
    let mut bytes = 0;
    for i in 0..small {
        let sub = dir.join(format!("d{:03}", i % 200));
        fs::create_dir_all(&sub).unwrap();
        let size = 512 + (i * 37) % 7000;
        write_file(&sub.join(format!("f{i}.dat")), size, &mut seed);
        bytes += size;
    }
    sets.push(Set { name: "many small files", folder: dir, files: small, bytes });

    // A project-like mix.
    let dir = src.join("mixed");
    fs::create_dir_all(&dir).unwrap();
    let (mut files, mut bytes) = (0, 0);
    for i in 0..(small / 10).max(100) {
        let sub = dir.join(format!("p{:02}", i % 40));
        fs::create_dir_all(&sub).unwrap();
        let size = match i % 20 {
            0 => 20 << 20,
            1..=3 => 1 << 20,
            _ => 4096 + (i * 131) % 60_000,
        };
        write_file(&sub.join(format!("m{i}.bin")), size, &mut seed);
        files += 1;
        bytes += size;
    }
    sets.push(Set { name: "mixed folder", folder: dir, files, bytes });

    // A few large files: dominated by throughput.
    let dir = src.join("large");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..4 {
        write_file(&dir.join(format!("big{i}.bin")), big_mb << 20, &mut seed);
    }
    sets.push(Set { name: "4 large files", folder: dir, files: 4, bytes: 4 * (big_mb << 20) });
    sets
}

fn drop_caches() -> bool {
    let _ = Command::new("sync").status();
    fs::write("/proc/sys/vm/drop_caches", "3").is_ok()
}

fn timed(f: impl FnOnce() -> bool) -> Option<Duration> {
    let t = Instant::now();
    f().then(|| t.elapsed())
}

fn median(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[v.len() / 2]
}

fn engine(set: &Set, dest: &Path, method: MethodPref, max_parallel: usize, budget: &Budget) -> bool {
    let spec = TransferSpec { sources: vec![set.folder.clone()], dest_dir: dest.to_path_buf(), op: Op::Copy, conflict: Conflict::Overwrite, preserve_times: true, large_threshold: 0, max_parallel, method, skip_identical: true, active_files: 0, speed_test: false, early_start_ms: 0 };
    let last = std::sync::Arc::new(parking_lot::Mutex::new(None));
    let sink = last.clone();
    let handle = transfer::start(1, spec, budget.clone(), fast_core::drives::list_drives(), None, move |p| *sink.lock() = Some(p));
    handle.wait();
    let ok = matches!(last.lock().as_ref().map(|p| (p.phase, p.error_count)), Some((Phase::Done, 0)));
    ok
}

fn tool(program: &str, args: &[&str]) -> bool {
    Command::new(program).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

fn have(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(src), Some(dst)) = (args.first(), args.get(1)) else {
        eprintln!("usage: transfer_bench SRC_DIR DEST_DIR [--runs 3] [--big-mb 512] [--small 20000]");
        std::process::exit(2);
    };
    let (src, dst) = (PathBuf::from(src).join("fast-bench-src"), PathBuf::from(dst).join("fast-bench-dst"));
    let runs = arg(&args, "--runs", 3).max(1) as usize;
    let big_mb = arg(&args, "--big-mb", 512);
    let small = arg(&args, "--small", 20_000);
    let budget = resources::compute(&Settings::default().performance, &resources::snapshot());

    println!("building data sets in {} …", src.display());
    let sets = make_sets(&src, small, big_mb);
    let cold = drop_caches();
    println!("caches dropped between runs: {}", if cold { "yes" } else { "no (repeat runs are partly served from memory)" });
    println!("workers: {} I/O threads\n", budget.io_threads);

    let rsync = have("rsync");
    for set in &sets {
        let mb = set.bytes as f64 / 1_048_576.0;
        println!("{} — {} files, {:.0} MB", set.name, set.files, mb);
        type Run<'a> = Box<dyn Fn(&Path) -> bool + 'a>;
        let mut contenders: Vec<(&str, Run)> = vec![
            ("FAST engine (auto)", Box::new(|d| engine(set, d, MethodPref::Auto, 0, &budget))),
            ("FAST engine (built-in)", Box::new(|d| engine(set, d, MethodPref::Native, 0, &budget))),
            ("FAST engine (1 stream)", Box::new(|d| engine(set, d, MethodPref::Native, 1, &budget))),
            ("cp -R", Box::new(|d| tool("cp", &["-R", set.folder.to_str().unwrap(), d.to_str().unwrap()]))),
        ];
        if rsync {
            contenders.push(("rsync -a", Box::new(|d| tool("rsync", &["-a", set.folder.to_str().unwrap(), d.to_str().unwrap()]))));
        }
        // One throwaway run first: the first copy after making the data pays for
        // the disk settling, which would otherwise be blamed on whoever goes first.
        let _ = fs::remove_dir_all(&dst);
        fs::create_dir_all(&dst).unwrap();
        contenders[2].1(&dst);
        let mut results = Vec::new();
        for (name, run) in &contenders {
            let mut times = Vec::new();
            for _ in 0..runs {
                let _ = fs::remove_dir_all(&dst);
                fs::create_dir_all(&dst).unwrap();
                drop_caches();
                if let Some(t) = timed(|| run(&dst)) {
                    times.push(t);
                }
            }
            if times.len() == runs {
                results.push((*name, median(times)));
            } else {
                println!("  {name:<24} failed");
            }
        }
        let best = results.iter().map(|r| r.1).min();
        for (name, t) in &results {
            let secs = t.as_secs_f64();
            let mark = if Some(*t) == best { "  ← fastest" } else { "" };
            println!("  {name:<24} {secs:>8.2} s   {:>8.0} MB/s   {:>9.0} files/s{mark}", mb / secs, set.files as f64 / secs);
        }
        println!();
    }
    let _ = fs::remove_dir_all(&src);
    let _ = fs::remove_dir_all(&dst);
}
