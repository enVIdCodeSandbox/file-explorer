//! Prints when each phase of one engine copy starts, to see where time goes.
//!     cargo run --release -p fast-core --example engine_trace -- SRC_FOLDER DEST_DIR
use fast_core::resources;
use fast_core::settings::Settings;
use fast_core::transfer::engine::TransferSpec;
use fast_core::transfer::method::MethodPref;
use fast_core::transfer::{self, Conflict, Op};
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let budget = resources::compute(&Settings::default().performance, &resources::snapshot());
    let spec = TransferSpec { sources: vec![a[0].clone().into()], dest_dir: a[1].clone().into(), op: Op::Copy, conflict: Conflict::Overwrite, preserve_times: true, large_threshold: 0, max_parallel: 0, skip_identical: true, active_files: 0, speed_test: false, early_start_ms: 0, method: if a.get(2).map(|s| s.as_str()) == Some("auto") { MethodPref::Auto } else { MethodPref::Native } };
    let t0 = Instant::now();
    let h = transfer::start(1, spec, budget, fast_core::drives::list_drives(), None, move |p| {
        println!("{:>7.3}s  {:?}  files {}/{}  bytes {}/{}", t0.elapsed().as_secs_f64(), p.phase, p.files_done, p.files_total, p.bytes_done, p.bytes_total);
    });
    h.wait();
    println!("{:>7.3}s  finished", t0.elapsed().as_secs_f64());
}
