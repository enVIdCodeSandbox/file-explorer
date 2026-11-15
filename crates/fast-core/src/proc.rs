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
