// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Headless helper mode. The app launches itself this way through the OS
    // administrator prompt (UAC, polkit, macOS authorization) when one
    // operation needs more rights than the person has, so the app itself never
    // has to run elevated. The job arrives on the command line.
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--job-b64" || a == "--job-file" || a == "--restore-file-manager") {
        std::process::exit(fast_explorer_lib::run_helper(&args));
    }
    fast_explorer_lib::run();
}
