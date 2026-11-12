//! Quick, honest numbers for the engine on your own machine:
//!
//!     cargo run --release -p fast-core --example bench -- /path/to/scan [query]

use fast_core::index::{Index, ScanOptions};
use fast_core::search::Query;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".into());
    let query = args.next().unwrap_or_else(|| "report".into());

    let t = Instant::now();
    let idx = Index::scan(&[root.clone().into()], &ScanOptions::default());
    let scan = t.elapsed();
    println!(
        "scan    {:>9} entries in {:>8.1?}  ({:.0} entries/s, {:.1} MB in memory)",
        idx.len(),
        scan,
        idx.len() as f64 / scan.as_secs_f64(),
        idx.memory_bytes() as f64 / 1048576.0
    );

    for q in [query.as_str(), "a", "*.rs", "ext:json size:>1kb"] {
        let parsed = Query::parse(q);
        let t = Instant::now();
        let hits = idx.search(&parsed, None, true, 500);
        println!("search  {:<22} {:>4} hits in {:>8.1?}", format!("{q:?}"), hits.len(), t.elapsed());
    }

    let t = Instant::now();
    let sizes = idx.subtree_sizes();
    println!("sizes   whole tree ({:.1} GB) aggregated in {:.1?}", sizes[0] as f64 / 1e9, t.elapsed());

    let t = Instant::now();
    let listing = fast_core::entry::list_dir(std::path::Path::new(&root), true).map(|l| l.len()).unwrap_or(0);
    println!("list    {listing} entries in {:.1?}", t.elapsed());
}
