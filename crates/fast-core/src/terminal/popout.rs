//! Opening a session in the system's own terminal app: Windows Terminal (or the console window),
//! Terminal on a Mac, and on Linux whichever terminal is installed. The window runs
//! `<this program> --terminal-attach <folder> <session>`, which attaches to the running shell.

use std::path::Path;

/// The program and arguments that open a terminal window running `command`, for a Linux terminal
/// emulator named `name` (as found on the PATH).
pub fn linux_command(name: &str, command: &[String]) -> (String, Vec<String>) {
    let mut args: Vec<String> = Vec::new();
    match name {
        // These take the command after a separator.
        "gnome-terminal" | "kgx" => args.push("--".into()),
        "tilix" => args.push("-e".into()),
        "xfce4-terminal" | "mate-terminal" | "terminator" | "lxterminal" => args.push("-x".into()),
        "wezterm" => args.extend(["start".into(), "--".into()]),
        "foot" | "kitty" => {}
        // konsole, xterm, alacritty, x-terminal-emulator and most others: -e.
        _ => args.push("-e".into()),
    }
    args.extend(command.iter().cloned());
    (name.to_string(), args)
}

/// Terminal emulators to try on Linux, most likely first.
pub const LINUX_TERMINALS: &[&str] = &[
    "x-terminal-emulator", "gnome-terminal", "kgx", "konsole", "xfce4-terminal", "tilix", "kitty", "alacritty", "wezterm", "foot", "mate-terminal", "terminator", "lxterminal", "xterm",
];

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file())).unwrap_or(false)
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Open a terminal window attached to session `sid` of the host in `dir`.
pub fn open_window(exe: &Path, dir: &Path, sid: &str) -> std::io::Result<()> {
    let attach: Vec<String> = vec![exe.to_string_lossy().into_owned(), "--terminal-attach".into(), dir.to_string_lossy().into_owned(), sid.to_string()];
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console is inherited; the attach program makes one, and Windows shows it in the default terminal app.
        return std::process::Command::new(&attach[0]).args(&attach[1..]).creation_flags(0x0000_0008).spawn().map(|_| ());
    }
    #[cfg(target_os = "macos")]
    {
        let script = dir.join(format!("popout-{sid}.command"));
        let body = format!("#!/bin/sh\nexec {}\n", attach.iter().map(|a| sh_quote(a)).collect::<Vec<_>>().join(" "));
        std::fs::write(&script, body)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        return std::process::Command::new("open").arg("-a").arg("Terminal").arg(&script).spawn().map(|_| ());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use std::os::unix::process::CommandExt;
        let preferred = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty());
        let mut names: Vec<String> = preferred.into_iter().collect();
        names.extend(LINUX_TERMINALS.iter().map(|s| s.to_string()));
        for name in names {
            let base = Path::new(&name).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name.clone());
            if !on_path(&name) && !Path::new(&name).is_file() {
                continue;
            }
            let (program, args) = linux_command(&base, &attach);
            let launched = std::process::Command::new(if Path::new(&name).is_file() { name.clone() } else { program })
                .args(args)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .process_group(0)
                .spawn();
            if launched.is_ok() {
                return Ok(());
            }
        }
        let _ = sh_quote;
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "No terminal program was found to open a window with."));
    }
    #[allow(unreachable_code)]
    {
        let _ = (on_path, sh_quote, attach);
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Terminal windows are not available here."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_terminal_gets_its_own_way_to_run_a_command() {
        let cmd = vec!["/app".to_string(), "--terminal-attach".into()];
        assert_eq!(linux_command("gnome-terminal", &cmd).1, vec!["--", "/app", "--terminal-attach"]);
        assert_eq!(linux_command("konsole", &cmd).1, vec!["-e", "/app", "--terminal-attach"]);
        assert_eq!(linux_command("xfce4-terminal", &cmd).1, vec!["-x", "/app", "--terminal-attach"]);
        assert_eq!(linux_command("kitty", &cmd).1, vec!["/app", "--terminal-attach"]);
        assert_eq!(linux_command("wezterm", &cmd).1, vec!["start", "--", "/app", "--terminal-attach"]);
        assert_eq!(linux_command("xterm", &cmd).1[0], "-e");
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(sh_quote("it's here"), "'it'\\''s here'");
    }
}
