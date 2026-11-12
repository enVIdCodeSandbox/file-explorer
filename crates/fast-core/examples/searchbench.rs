//! Times the search paths on a large synthetic tree:
//!   cargo run --release -p fast-core --example searchbench -- [files]
use fast_core::index::{Index, ScanOptions};
use fast_core::indexset::IndexSet;
use fast_core::search::{live_search, LiveOptions, Query};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;

fn main() {
    let files: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let dir = std::env::temp_dir().join(format!("fast-bench-{files}"));
    if !dir.exists() {
        let t = Instant::now();
        let per = 100;
        for i in 0..files / per {
            let d = dir.join(format!("group{}", i / 50)).join(format!("folder{}", i % 50));
            std::fs::create_dir_all(&d).unwrap();
            for j in 0..per {
                let name = match j % 7 {
                    0 => format!("report-{i}-{j}.pdf"),
                    1 => format!("IMG_{i:05}_{j}.jpg"),
                    _ => format!("file_{i}_{j}.dat"),
                };
                std::fs::write(d.join(name), b"x").unwrap();
            }
        }
        println!("made {files} files in {:.1}s", t.elapsed().as_secs_f64());
    }
    let t = Instant::now();
    let idx = Index::scan(&[dir.clone()], &ScanOptions { threads: 8, ..Default::default() });
    println!("scan: {} entries in {:.2}s ({:.0}/s)", idx.len(), t.elapsed().as_secs_f64(), idx.len() as f64 / t.elapsed().as_secs_f64());
    let set = IndexSet::default().with_part(Arc::new(idx));
    for q in ["report", "r", "report-77", "ext:pdf", "*.jpg", "zzzz-nothing"] {
        let query = Query::parse(q);
        let t = Instant::now();
        let mut n = 0;
        for _ in 0..20 {
            n = set.search(&query, None, true, 1000).len();
        }
        println!("index search {q:>14}: {n:>5} hits, {:.2} ms", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
    }
    let cancel = AtomicBool::new(false);
    let opts = LiveOptions { threads: 8, include_hidden: true, cancel: &cancel, skip: vec![] };
    let t = Instant::now();
    let mut first: Option<f64> = None;
    let mut total = 0;
    let seen = live_search(&dir, &Query::parse("report-77"), &opts, |hits, _| {
        if first.is_none() && !hits.is_empty() {
            first = Some(t.elapsed().as_secs_f64() * 1000.0);
        }
        total += hits.len();
    });
    println!("live search: first hit after {:.1} ms; {total} hits; whole walk of {seen} entries took {:.2}s", first.unwrap_or(-1.0), t.elapsed().as_secs_f64());
}
