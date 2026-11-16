fn main() {
    // The moment this build was made, so the updater can tell a newer build from an older one by date.
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=FAST_BUILD_TIME={now}");
    tauri_build::build()
}
