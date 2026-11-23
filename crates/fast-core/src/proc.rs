//! Helper programs the app starts for itself (not ones the person asked to open).

use std::process::Command;

/// Keep a helper program from opening a console window on Windows. On other systems this does nothing.
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

/// Make the calling thread yield to everything else the computer is doing (background work).
pub fn lower_thread_priority() {
    #[cfg(windows)]
    // SAFETY: both calls only act on the calling thread.
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
    }
    #[cfg(target_os = "linux")]
    // SAFETY: setpriority with `who` 0 means the calling thread on Linux.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

/// Adjust OS process scheduling priority according to the configured mode, performance intensity
/// (`0..=100`), and whether the window is currently backgrounded. Never uses real-time priority.
pub fn apply_process_priority(mode: &str, intensity: u8, background: bool) {
    let target = match mode {
        "low" => -1i8,
        "normal" => 0,
        "high" => {
            if background { 0 } else { 1 }
        }
        _ => {
            if background && intensity < 50 {
                -1
            } else if !background && intensity >= 80 {
                1
            } else {
                0
            }
        }
    };
    #[cfg(windows)]
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no closing; `SetPriorityClass`
    // only adjusts the current process priority class within standard unprivileged classes.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, SetPriorityClass, ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
        };
        let class = match target {
            x if x < 0 => BELOW_NORMAL_PRIORITY_CLASS,
            x if x > 0 => ABOVE_NORMAL_PRIORITY_CLASS,
            _ => NORMAL_PRIORITY_CLASS,
        };
        let _ = SetPriorityClass(GetCurrentProcess(), class);
    }
    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
    // SAFETY: `setpriority` for the current PID only attempts best-effort nice adjustment.
    unsafe {
        let nice = match target {
            x if x < 0 => 5,
            x if x > 0 => -5,
            _ => 0,
        };
        let _ = libc::setpriority(libc::PRIO_PROCESS, 0, nice);
    }
    let _ = target;
}

#[cfg(test)]
mod tests {
    #[test]
    fn quiet_still_runs_the_program() {
        let mut c = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "echo hi"]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", "echo hi"]);
            c
        };
        let out = super::quiet(&mut c).output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("hi"));
    }
}
