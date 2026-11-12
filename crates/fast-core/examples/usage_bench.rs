//! Compare the disk-usage scanner with the index-based analysis.
//! `cargo run --release -p fast-core --example usage_bench -- <folder> [index]`

use fast_core::index::{Index, ScanOptions};
use fast_core::treemap::{self, TreeOptions};
use fast_core::usage::{self, UsageOptions};
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(args.get(1).expect("usage: usage_bench <folder> [index]"));
    let t = Instant::now();
    let a = if args.get(2).map(String::as_str) == Some("index") {
        let idx = Index::scan(&[root], &ScanOptions::default());
        treemap::analyze(&idx, 0, &TreeOptions::default())
    } else {
        usage::scan(&root, &UsageOptions::default()).expect("scan")
    };
    println!("{} files, {} bytes, {:.2}s", a.file_count, a.total_bytes, t.elapsed().as_secs_f64());
}
