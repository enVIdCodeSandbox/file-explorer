//! Times what the app reads when it starts. `cargo run --release --example startup_probe`
use fast_core::{history::History, resources, session::Session, settings::Settings, sizecache::SizeCache};
use std::time::Instant;
fn t<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let s = Instant::now();
    let r = f();
    println!("{name:<28}{:>8.1} ms", s.elapsed().as_secs_f64() * 1000.0);
    r
}
fn main() {
    let d = std::env::temp_dir().join("fast-startup-probe");
    let _ = std::fs::create_dir_all(&d);
    // A busy history of a year and a size cache of many folders.
    let mut sc = SizeCache::default();
    for i in 0..200_000u64 {
        sc.put(std::path::Path::new(&format!("/data/some/deep/folder/{}/{}/x{}", i % 997, i % 31, i)), i * 10, i, i);
    }
    sc.save(&d.join("sizes.json")).unwrap();
    println!("size cache file: {} KB", std::fs::metadata(d.join("sizes.json")).unwrap().len() / 1024);
    t("Settings::load", || Settings::load(&d.join("settings.json")));
    t("Session::load", || Session::load(&d.join("session.json")));
    t("History::load", || History::load(&d.join("history.bin")));
    t("SizeCache::load (200k)", || SizeCache::load(&d.join("sizes.json")));
    let snap = t("resources::snapshot", resources::snapshot);
    t("resources::snapshot again", resources::snapshot);
    let s = Settings::default();
    t("resources::compute", || resources::compute(&s.performance, &snap));
    t("drives::list", || fast_core::drives::list_drives());
    let _ = std::fs::remove_dir_all(&d);
}
