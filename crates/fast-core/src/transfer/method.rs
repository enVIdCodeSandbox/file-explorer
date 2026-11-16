//! Ways to move the bytes. The built-in engine is always available. Where the
//! system has a well-known copy tool, it is a second candidate: Robocopy on
//! Windows, rsync on macOS and Linux. Which one a job uses is decided from how
//! earlier, similar jobs actually went (see `History::choose_method`), not
//! from a guess about which tool is "faster".
//!
//! Network drives get two more candidates, chosen by what the route touches:
//!
//! * **SMB** (Windows file shares, `\\server\share`, cifs/smbfs mounts):
//!   Robocopy with SMB compression on Windows, rsync in whole-file mode
//!   elsewhere. Needs a Robocopy that knows `/COMPRESS` (Windows 11 / Server
//!   2022 and later) on Windows.
//! * **NFS** (nfs/nfs4 mounts): rsync in whole-file mode. The delta
//!   algorithm reads the whole destination file back over the wire before it
//!   sends a byte, which is the slowest thing to do on a mount.
//!
//! The operating system speaks the actual protocol; these are the tool
//! settings that suit it. **rclone**, when installed, is a candidate for any
//! route, and the one to try for cloud and FUSE mounts.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use super::engine::Conflict;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Native,
    Robocopy,
    Rsync,
    Rclone,
    Smb,
    Nfs,
}

impl Method {
    pub fn code(self) -> u8 {
        match self {
            Method::Native => 0,
            Method::Robocopy => 1,
            Method::Rsync => 2,
            Method::Rclone => 3,
            Method::Smb => 4,
            Method::Nfs => 5,
        }
    }
    pub fn from_code(c: u8) -> Method {
        match c {
            1 => Method::Robocopy,
            2 => Method::Rsync,
            3 => Method::Rclone,
            4 => Method::Smb,
            5 => Method::Nfs,
            _ => Method::Native,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Method::Native => "Built-in",
            Method::Robocopy => "Robocopy",
            Method::Rsync => "rsync",
            Method::Rclone => "rclone",
            Method::Smb => "SMB-tuned",
            Method::Nfs => "NFS-tuned",
        }
    }

    /// Which external program runs this method.
    fn program(self) -> Option<&'static str> {
        match self {
            Method::Native => None,
            Method::Robocopy => Some("robocopy"),
            Method::Rsync | Method::Nfs => Some("rsync"),
            Method::Rclone => Some("rclone"),
            Method::Smb => Some(if cfg!(windows) { "robocopy" } else { "rsync" }),
        }
    }

    /// Whether the progress comes on stderr instead of stdout.
    pub fn reports_on_stderr(self) -> bool {
        self == Method::Rclone
    }

    fn is_robocopy(self) -> bool {
        self == Method::Robocopy || (self == Method::Smb && cfg!(windows))
    }
}

/// What the person asked for: leave it to the app, or force one method.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum MethodPref {
    #[default]
    Auto,
    Native,
    Robocopy,
    Rsync,
    Rclone,
    Smb,
    Nfs,
}

impl MethodPref {
    pub fn method(self) -> Option<Method> {
        match self {
            MethodPref::Auto => None,
            MethodPref::Native => Some(Method::Native),
            MethodPref::Robocopy => Some(Method::Robocopy),
            MethodPref::Rsync => Some(Method::Rsync),
            MethodPref::Rclone => Some(Method::Rclone),
            MethodPref::Smb => Some(Method::Smb),
            MethodPref::Nfs => Some(Method::Nfs),
        }
    }
    pub fn from_setting(s: &str) -> MethodPref {
        match s {
            "native" => MethodPref::Native,
            "robocopy" => MethodPref::Robocopy,
            "rsync" => MethodPref::Rsync,
            "rclone" => MethodPref::Rclone,
            "smb" => MethodPref::Smb,
            "nfs" => MethodPref::Nfs,
            _ => MethodPref::Auto,
        }
    }
}

/// What to do before there is any history to learn from: many small files go to a tool that handles
/// them in parallel, big files stay with the built-in engine (which uses the operating system's own
/// file copy, so over SMB the large files go the native way). The person can set either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rules {
    pub small: MethodPref,
    pub large: MethodPref,
    /// A job is "small files" when its average file is under this many bytes.
    pub small_avg_bytes: u64,
    /// ...and it has at least this many files (a handful of files is not worth starting a tool for).
    pub small_min_files: u64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { small: MethodPref::Auto, large: MethodPref::Auto, small_avg_bytes: 1 << 20, small_min_files: 50 }
    }
}

/// The method for a job by its shape alone. `can` is what this computer and route can run.
pub fn rule_choice(files: u64, bytes: u64, is_move: bool, can: &[Method], r: &Rules) -> Method {
    if is_move || can.len() < 2 {
        return Method::Native;
    }
    let small = files >= r.small_min_files && bytes / files.max(1) < r.small_avg_bytes;
    let wanted = if small { r.small } else { r.large };
    if let Some(m) = wanted.method() {
        return if can.contains(&m) { m } else { Method::Native };
    }
    if !small {
        return Method::Native;
    }
    [Method::Rclone, Method::Robocopy, Method::Rsync].into_iter().find(|m| can.contains(m)).unwrap_or(Method::Native)
}

/// Decides between the candidates for a job. Called once, after the sources are scanned.
pub type MethodChooser = Box<dyn FnOnce(&super::super::history::Query, &[Method]) -> Method + Send>;

fn runs(program: &str, arg: &str) -> bool {
    let mut c = Command::new(program);
    c.arg(arg).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    no_window(&mut c);
    c.status().is_ok()
}

#[cfg(windows)]
pub fn no_window(c: &mut Command) {
    use std::os::windows::process::CommandExt;
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}
#[cfg(not(windows))]
pub fn no_window(_: &mut Command) {}

/// Tools installed on this computer, before looking at any route. Looked up once.
pub fn installed() -> &'static [Method] {
    static CACHE: OnceLock<Vec<Method>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let mut v = vec![Method::Native];
        if cfg!(any(target_os = "android", target_os = "ios")) {
            return v;
        }
        let robocopy = cfg!(windows) && runs("robocopy", "/?");
        let rsync = cfg!(unix) && runs("rsync", "--version");
        if robocopy {
            v.push(Method::Robocopy);
        }
        if rsync {
            v.push(Method::Rsync);
        }
        if runs("rclone", "version") {
            v.push(Method::Rclone);
        }
        if (cfg!(windows) && robocopy && robocopy_has_compress()) || (cfg!(unix) && rsync) {
            v.push(Method::Smb);
        }
        if cfg!(unix) && rsync {
            v.push(Method::Nfs);
        }
        v
    })
}

/// Older Robocopy builds fail on `/COMPRESS` (SMB compression, Windows 11 and later).
fn robocopy_has_compress() -> bool {
    let mut c = Command::new("robocopy");
    c.arg("/?").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    no_window(&mut c);
    c.output().map(|o| String::from_utf8_lossy(&o.stdout).to_ascii_uppercase().contains("/COMPRESS")).unwrap_or(false)
}

/// What kind of network file system a path is on, if it is on one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetKind {
    Smb,
    Nfs,
    Other,
}

/// File system types as a mount table names them.
#[allow(dead_code)] // used on some platforms only
fn kind_of_fs(fs: &str) -> Option<NetKind> {
    match fs.to_ascii_lowercase().as_str() {
        "nfs" | "nfs4" => Some(NetKind::Nfs),
        "cifs" | "smb" | "smb2" | "smb3" | "smbfs" => Some(NetKind::Smb),
        "afpfs" | "webdav" | "davfs" | "fuse.sshfs" | "sshfs" | "9p" | "ncpfs" | "fuse.rclone" | "ceph" | "glusterfs" => Some(NetKind::Other),
        _ => None,
    }
}

/// Mount points and their types (`/proc/self/mounts` on Linux, `mount` on macOS).
#[cfg(unix)]
fn mount_table() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/self/mounts") {
        for line in text.lines() {
            let mut f = line.split_whitespace();
            if let (Some(_dev), Some(mp), Some(fs)) = (f.next(), f.next(), f.next()) {
                out.push((mp.replace("\\040", " "), fs.to_string()));
            }
        }
        return out;
    }
    let mut c = Command::new("mount");
    c.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    if let Ok(o) = c.output() {
        // "//user@nas/share on /Volumes/share (smbfs, nodev, nosuid)"
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            if let Some((_, rest)) = line.split_once(" on ") {
                if let Some((mp, opts)) = rest.rsplit_once(" (") {
                    let fs = opts.split(',').next().unwrap_or("").trim().to_string();
                    out.push((mp.to_string(), fs));
                }
            }
        }
    }
    out
}

/// Classify every path of a job once, then ask about each.
pub struct NetMap {
    #[cfg(unix)]
    table: Vec<(String, String)>,
}

impl NetMap {
    pub fn read() -> NetMap {
        NetMap {
            #[cfg(unix)]
            table: mount_table(),
        }
    }

    pub fn kind(&self, path: &Path) -> Option<NetKind> {
        #[cfg(windows)]
        {
            return windows_kind(path);
        }
        #[cfg(unix)]
        {
            return self
                .table
                .iter()
                .filter(|(mp, _)| path.starts_with(mp))
                .max_by_key(|(mp, _)| mp.len())
                .and_then(|(_, fs)| kind_of_fs(fs));
        }
        #[allow(unreachable_code)]
        None
    }
}

#[cfg(windows)]
fn windows_kind(path: &Path) -> Option<NetKind> {
    use std::os::windows::ffi::OsStrExt;
    let s = path.to_string_lossy();
    if s.starts_with(r"\\") && !s.starts_with(r"\\?\") {
        return Some(NetKind::Smb);
    }
    let letter = s.chars().next().filter(|c| c.is_ascii_alphabetic() && s[1..].starts_with(':'))?;
    let root: Vec<u16> = std::ffi::OsString::from(format!("{}:\\", letter.to_ascii_uppercase())).encode_wide().chain([0]).collect();
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    // DRIVE_REMOTE == 4. Mapped drives on Windows are SMB (or the NFS client, which we treat the same).
    (unsafe { GetDriveTypeW(root.as_ptr()) } == 4).then_some(NetKind::Smb)
}

/// The methods worth considering for these items, given what is installed and
/// what the route touches. The built-in engine is always first.
pub fn candidates(tops: &[Top]) -> Vec<Method> {
    let net = NetMap::read();
    let kinds: Vec<NetKind> = tops.iter().flat_map(|t| [net.kind(&t.src), net.kind(&t.dst)]).flatten().collect();
    let (smb, nfs) = (kinds.contains(&NetKind::Smb), kinds.contains(&NetKind::Nfs));
    installed()
        .iter()
        .copied()
        .filter(|m| match m {
            Method::Smb => smb,
            Method::Nfs => nfs,
            _ => true,
        })
        .collect()
}

/// One top-level item to copy: where from, where to, and whether it is a folder.
#[derive(Clone, Debug)]
pub struct Top {
    pub src: PathBuf,
    pub dst: PathBuf,
    pub is_dir: bool,
}

/// Can an external tool do this item exactly like the built-in engine would?
/// The tools keep the name, so a rename on arrival (a copy next to its source)
/// stays with the engine.
pub fn can_run(t: &Top) -> bool {
    t.src.file_name().is_some() && t.src.file_name() == t.dst.file_name() && t.src.parent().is_some() && t.dst.parent().is_some()
}

fn trim_slash(p: &Path) -> OsString {
    let s = p.as_os_str().to_string_lossy();
    let t = s.trim_end_matches(['/', '\\']);
    if t.is_empty() { p.as_os_str().to_os_string() } else { OsString::from(t) }
}

/// The command line for one item.
pub fn command(method: Method, t: &Top, conflict: Conflict, preserve_times: bool, workers: usize) -> Command {
    let program = method.program().unwrap_or("rsync");
    if method.is_robocopy() {
        let mut c = Command::new(program);
        if t.is_dir {
            c.arg(trim_slash(&t.src)).arg(trim_slash(&t.dst)).arg("/E");
        } else {
            c.arg(trim_slash(t.src.parent().unwrap())).arg(trim_slash(t.dst.parent().unwrap())).arg(t.src.file_name().unwrap());
        }
        c.arg(if preserve_times { "/COPY:DAT" } else { "/COPY:D" })
            .arg("/DCOPY:T")
            .args(["/R:1", "/W:1", "/BYTES", "/FP", "/NJH", "/NJS", "/NP", "/NDL"])
            .arg(format!("/MT:{}", workers.clamp(1, 32)));
        if method == Method::Smb {
            c.arg("/COMPRESS"); // SMB compression: fewer bytes over the wire when the link is the limit
        }
        if conflict == Conflict::Skip {
            // Leave anything that already exists exactly as it is.
            c.args(["/XC", "/XN", "/XO"]);
        }
        return c;
    }
    if method == Method::Rclone {
        let mut c = Command::new(program);
        // `copyto` names the destination itself, which is what a file or a renamed-on-arrival folder needs.
        c.arg(if t.is_dir { "copy" } else { "copyto" }).arg(trim_slash(&t.src)).arg(trim_slash(&t.dst));
        let n = workers.clamp(1, 32).to_string();
        c.args(["--transfers", &n, "--checkers", &n, "--use-json-log", "--stats", "1s", "--stats-log-level", "NOTICE", "--log-level", "NOTICE"]);
        match conflict {
            Conflict::Skip => {
                c.arg("--ignore-existing");
            }
            _ => {
                c.arg("--ignore-times");
            }
        }
        if !preserve_times {
            c.arg("--no-update-modtime");
        }
        return c;
    }
    let mut c = Command::new(program);
    c.arg(if preserve_times { "-rlDt" } else { "-rlD" });
    if matches!(method, Method::Nfs | Method::Smb) {
        // Do not read the far copy back to look for differences; just send the file.
        c.arg("--whole-file");
    }
    if conflict == Conflict::Skip {
        c.arg("--ignore-existing");
    }
    c.arg("--progress").arg("--out-format=%l\t%n").arg("--").arg(trim_slash(&t.src));
    let mut parent = trim_slash(t.dst.parent().unwrap());
    parent.push("/");
    c.arg(parent);
    c
}

/// What one line (or carriage-return chunk) of a tool's output says happened.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A file is finished: its size.
    Done(u64),
    /// A file has just begun copying (Robocopy says so before it copies, not after): its size and
    /// the path it was read from. How far it has got is read from the file being written.
    Started { size: u64, name: String },
    /// Bytes of the current file transferred so far (rsync's live counter).
    Partial(u64),
    /// Everything transferred so far by this run of the tool: bytes and files (rclone's statistics).
    Total { bytes: u64, files: u64 },
    Nothing,
}

pub fn parse(method: Method, line: &str) -> Event {
    let line = line.trim_end_matches(['\r', '\n']);
    if method == Method::Rclone {
        // {"level":"notice","msg":"...","stats":{"bytes":123,"transfers":2,...}}
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return Event::Nothing };
        let Some(st) = v.get("stats") else { return Event::Nothing };
        return match (st.get("bytes").and_then(|b| b.as_u64()), st.get("transfers").and_then(|b| b.as_u64())) {
            (Some(bytes), Some(files)) => Event::Total { bytes, files },
            _ => Event::Nothing,
        };
    }
    match if method.is_robocopy() { Method::Robocopy } else { Method::Rsync } {
        Method::Robocopy => {
            // "\t  New File  \t\t   1048576\tname": class, size, name. Lines for
            // files that were left alone ("Same", "*EXTRA", skipped) start with
            // another word or a star and are not counted.
            let parts: Vec<&str> = line.split('\t').map(str::trim).filter(|p| !p.is_empty()).collect();
            let copied = matches!(parts.first().map(|c| c.to_ascii_lowercase()).as_deref(), Some("new file" | "newer" | "older" | "changed" | "modified" | "lonely"));
            match (copied, parts.get(1).and_then(|s| s.parse::<u64>().ok())) {
                (true, Some(size)) => Event::Started { size, name: parts.get(2).map(|s| s.to_string()).unwrap_or_default() },
                _ => Event::Nothing,
            }
        }
        _ => {
            if let Some((size, name)) = line.split_once('\t') {
                // Folders are listed with a trailing slash; they carry no data.
                return match size.trim().parse::<u64>() {
                    Ok(n) if !name.ends_with('/') => Event::Done(n),
                    _ => Event::Nothing,
                };
            }
            // "  1,234,567  12%  10.5MB/s  0:00:05"
            let mut it = line.split_whitespace();
            match (it.next(), it.next()) {
                (Some(bytes), Some(pct)) if pct.ends_with('%') => {
                    bytes.replace(',', "").parse::<u64>().map(Event::Partial).unwrap_or(Event::Nothing)
                }
                _ => Event::Nothing,
            }
        }
    }
}

/// Did the tool succeed? Robocopy uses bit flags: below 8 means nothing failed.
pub fn succeeded(method: Method, code: i32) -> bool {
    if method.is_robocopy() {
        (0..8).contains(&code)
    } else if method == Method::Rclone {
        code == 0
    } else {
        code == 0 || code == 24 // 24: a source file vanished while copying
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_robocopy_lines() {
        let started = |size: u64, name: &str| Event::Started { size, name: name.into() };
        assert_eq!(parse(Method::Robocopy, "\t    New File  \t\t  1048576\tbig.bin"), started(1048576, "big.bin"));
        assert_eq!(parse(Method::Robocopy, "\t    Newer     \t\t      12\tC:\\src\\sub\\a.txt"), started(12, "C:\\src\\sub\\a.txt"));
        assert_eq!(parse(Method::Robocopy, "\t    same      \t\t      12\ta.txt"), Event::Nothing);
        assert_eq!(parse(Method::Robocopy, "\t  *EXTRA File \t\t      12\ta.txt"), Event::Nothing);
        assert_eq!(parse(Method::Robocopy, ""), Event::Nothing);
    }

    #[test]
    fn parses_rsync_lines() {
        assert_eq!(parse(Method::Rsync, "1048576\tdir/big.bin"), Event::Done(1048576));
        assert_eq!(parse(Method::Rsync, "4096\tdir/"), Event::Nothing);
        assert_eq!(parse(Method::Rsync, "  1,234,567  12%   10.5MB/s    0:00:05"), Event::Partial(1234567));
        assert_eq!(parse(Method::Rsync, "sending incremental file list"), Event::Nothing);
    }

    #[test]
    fn parses_rclone_statistics() {
        let l = r#"{"level":"notice","msg":"\nTransferred: 1 MiB","stats":{"bytes":1048576,"transfers":3,"errors":0},"source":"accounting/stats.go:526"}"#;
        assert_eq!(parse(Method::Rclone, l), Event::Total { bytes: 1048576, files: 3 });
        assert_eq!(parse(Method::Rclone, r#"{"level":"notice","msg":"hello"}"#), Event::Nothing);
        assert_eq!(parse(Method::Rclone, "not json"), Event::Nothing);
        assert!(succeeded(Method::Rclone, 0) && !succeeded(Method::Rclone, 1));
    }

    fn args(c: &Command) -> Vec<String> {
        c.get_args().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn rclone_command() {
        let d = Top { src: "/a/photos".into(), dst: "/b/photos".into(), is_dir: true };
        let a = args(&command(Method::Rclone, &d, Conflict::Skip, true, 8));
        assert_eq!(&a[..3], ["copy", "/a/photos", "/b/photos"]);
        assert!(a.contains(&"--ignore-existing".to_string()) && a.contains(&"--use-json-log".to_string()));
        let f = Top { src: "/a/x.iso".into(), dst: "/b/x.iso".into(), is_dir: false };
        let a = args(&command(Method::Rclone, &f, Conflict::Overwrite, false, 8));
        assert_eq!(a[0], "copyto");
        assert!(a.contains(&"--ignore-times".to_string()) && a.contains(&"--no-update-modtime".to_string()));
    }

    #[test]
    fn network_methods_are_tuned() {
        let d = Top { src: "/a/photos".into(), dst: "/mnt/nas/photos".into(), is_dir: true };
        let a = args(&command(Method::Nfs, &d, Conflict::Overwrite, true, 4));
        assert!(a.contains(&"--whole-file".to_string()) && a[0] == "-rlDt");
        assert!(!args(&command(Method::Rsync, &d, Conflict::Overwrite, true, 4)).contains(&"--whole-file".to_string()));
        let w = Top { src: "C:\\a".into(), dst: "\\\\nas\\a".into(), is_dir: true };
        let a = args(&command(Method::Robocopy, &w, Conflict::Overwrite, true, 4));
        assert!(!a.contains(&"/COMPRESS".to_string()));
        if cfg!(windows) {
            assert!(args(&command(Method::Smb, &w, Conflict::Overwrite, true, 4)).contains(&"/COMPRESS".to_string()));
        }
    }

    #[test]
    fn file_system_names_map_to_kinds() {
        assert_eq!(kind_of_fs("nfs4"), Some(NetKind::Nfs));
        assert_eq!(kind_of_fs("CIFS"), Some(NetKind::Smb));
        assert_eq!(kind_of_fs("smbfs"), Some(NetKind::Smb));
        assert_eq!(kind_of_fs("fuse.rclone"), Some(NetKind::Other));
        assert_eq!(kind_of_fs("ext4"), None);
    }

    #[test]
    fn exit_codes() {
        assert!(succeeded(Method::Robocopy, 1) && succeeded(Method::Robocopy, 3) && !succeeded(Method::Robocopy, 8));
        assert!(succeeded(Method::Rsync, 0) && !succeeded(Method::Rsync, 23));
    }

    #[test]
    fn renamed_items_stay_with_the_engine() {
        let t = Top { src: "/a/x.txt".into(), dst: "/b/x.txt".into(), is_dir: false };
        assert!(can_run(&t));
        let t = Top { src: "/a/x.txt".into(), dst: "/a/x (2).txt".into(), is_dir: false };
        assert!(!can_run(&t));
    }

    #[test]
    fn robocopy_command_for_a_folder_and_a_file() {
        let t = Top { src: "C:\\data\\photos\\".into(), dst: "D:\\backup\\photos".into(), is_dir: true };
        let c = command(Method::Robocopy, &t, Conflict::Skip, true, 16);
        let a: Vec<_> = c.get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(&a[..3], ["C:\\data\\photos", "D:\\backup\\photos", "/E"]);
        assert!(a.contains(&"/MT:16".to_string()) && a.contains(&"/XC".to_string()));
        assert!(a.contains(&"/FP".to_string()), "full paths, so the file being written can be found");
        let f = Top { src: "/d/a.iso".into(), dst: "/b/a.iso".into(), is_dir: false };
        let a: Vec<_> = command(Method::Robocopy, &f, Conflict::Overwrite, true, 4).get_args().map(|s| s.to_string_lossy().into_owned()).collect();
        assert_eq!(&a[..3], ["/d", "/b", "a.iso"]);
        assert!(!a.contains(&"/XC".to_string()));
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    const ALL: &[Method] = &[Method::Native, Method::Robocopy, Method::Rclone, Method::Smb];

    #[test]
    fn many_small_files_use_a_tool_and_big_files_stay_built_in() {
        let r = Rules::default();
        assert_eq!(rule_choice(5000, 200 << 20, false, ALL, &r), Method::Rclone);
        assert_eq!(rule_choice(5000, 200 << 20, false, &[Method::Native, Method::Robocopy], &r), Method::Robocopy);
        assert_eq!(rule_choice(3, 4 << 30, false, ALL, &r), Method::Native);
        assert_eq!(rule_choice(10, 1 << 20, false, ALL, &r), Method::Native, "too few files to bother");
        assert_eq!(rule_choice(5000, 200 << 20, true, ALL, &r), Method::Native, "moves stay built-in");
        assert_eq!(rule_choice(5000, 200 << 20, false, &[Method::Native], &r), Method::Native);
    }

    #[test]
    fn a_chosen_method_wins_when_it_can_run() {
        let r = Rules { small: MethodPref::Robocopy, large: MethodPref::Smb, ..Rules::default() };
        assert_eq!(rule_choice(5000, 200 << 20, false, ALL, &r), Method::Robocopy);
        assert_eq!(rule_choice(3, 4 << 30, false, ALL, &r), Method::Smb);
        let r = Rules { small: MethodPref::Rsync, ..Rules::default() };
        assert_eq!(rule_choice(5000, 200 << 20, false, ALL, &r), Method::Native, "not installed: built-in");
    }
}
