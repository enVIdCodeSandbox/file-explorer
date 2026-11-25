//! Mounted volumes: local disks, removable media and network shares.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Media {
    Ssd,
    Hdd,
    Network,
    Removable,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Drive {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub total: u64,
    pub available: u64,
    pub media: Media,
    pub removable: bool,
    pub read_only: bool,
    /// False when the volume did not answer (e.g. an unreachable share).
    pub online: bool,
    /// For network volumes, where the share lives: `\\server\share` or `server:/export`.
    #[serde(default)]
    pub remote: Option<String>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const NETWORK_FS: &[&str] = &[
    "nfs", "nfs4", "cifs", "smb", "smb2", "smb3", "smbfs", "afpfs", "webdav", "davfs",
    "fuse.sshfs", "sshfs", "9p", "ncpfs", "fuse.rclone", "ceph", "glusterfs",
];

#[cfg(any(all(unix, not(any(target_os = "android", target_os = "ios"))), test))]
const PSEUDO_FS: &[&str] = &[
    "tmpfs", "devtmpfs", "overlay", "squashfs", "proc", "sysfs", "devfs", "autofs",
    "efivarfs", "ramfs", "cgroup", "cgroup2", "fusectl", "tracefs", "debugfs", "nullfs",
];

/// List volumes. Statistics for each volume come from the OS and can block on
/// an unreachable network share, so callers should show a cached list first
/// and run this off the UI thread (the shell does both).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn list_drives() -> Vec<Drive> {
    use sysinfo::{DiskKind, Disks};
    let disks = Disks::new_with_refreshed_list();
    let mut out: Vec<Drive> = Vec::new();
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().into_owned();
        let fs = d.file_system().to_string_lossy().to_lowercase();
        #[cfg(not(windows))]
        {
            if d.total_space() == 0
                || is_hidden_unix_mount(
                    &mount,
                    &fs,
                    d.is_read_only(),
                    cfg!(target_os = "macos"),
                    || Path::new(&mount).join("FAST File Explorer.app").exists(),
                )
            {
                continue;
            }
        }
        if out.iter().any(|o| o.mount == mount) {
            continue;
        }
        let media = if NETWORK_FS.contains(&fs.as_str()) {
            Media::Network
        } else if d.is_removable() {
            Media::Removable
        } else {
            match d.kind() {
                DiskKind::SSD => Media::Ssd,
                DiskKind::HDD => Media::Hdd,
                _ => Media::Unknown,
            }
        };
        let label = d.name().to_string_lossy().into_owned();
        let share = (media == Media::Network).then(|| parse_remote(&label)).flatten();
        out.push(Drive {
            name: match &share {
                Some(s) => s.display(&mount),
                None => display_name(&label, &mount),
            },
            remote: share.map(|s| s.remote),
            mount,
            fs,
            total: d.total_space(),
            available: d.available_space(),
            media,
            removable: d.is_removable(),
            read_only: d.is_read_only(),
            online: true,
        });
    }
    #[cfg(windows)]
    win::add_other_drives(&mut out);
    #[cfg(target_os = "macos")]
    mac::add_network_drives(&mut out);
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

#[cfg(any(all(unix, not(any(target_os = "android", target_os = "ios"))), test))]
fn is_hidden_unix_mount(
    mount: &str,
    fs: &str,
    read_only: bool,
    is_macos: bool,
    has_app_bundle: impl FnOnce() -> bool,
) -> bool {
    if PSEUDO_FS.contains(&fs)
        || mount.starts_with("/snap")
        || mount.starts_with("/boot")
        || mount.starts_with("/System/Volumes")
        || mount.starts_with("/private/var")
        || mount.starts_with("/dev")
        || mount.starts_with("/Volumes/.timemachine")
    {
        return true;
    }
    if is_macos && read_only && mount.starts_with("/Volumes/") {
        let name = Path::new(mount).file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
        if name == "FAST File Explorer" || name.starts_with("FAST File Explorer ") || has_app_bundle() {
            return true;
        }
    }
    false
}

#[cfg(any(target_os = "macos", test))]
fn normalize_mac_remote(from: &str) -> Option<String> {
    let trimmed = from.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((scheme, rest)) = trimmed.split_once("://") {
        if matches!(scheme.to_ascii_lowercase().as_str(), "smb" | "cifs" | "afp") {
            return Some(format!("//{rest}"));
        }
        return None;
    }
    Some(trimmed.to_string())
}

/// A cheap fingerprint of "what is plugged in": it changes when a volume is mounted,
/// unmounted, connected or disconnected, and costs far less than [`list_drives`], so the
/// shell can check it every second or so and only list the volumes when it moved.
pub fn signature() -> u64 {
    #[allow(unused_imports)] // Windows hashes through its own module
    use std::hash::{Hash, Hasher};
    #[allow(unused_mut)] // phones have nothing to feed it
    let mut h = std::collections::hash_map::DefaultHasher::new();
    #[cfg(windows)]
    win::signature(&mut h);
    #[cfg(target_os = "linux")]
    {
        // Every mount, unmount and remount shows up in this table.
        std::fs::read("/proc/self/mountinfo").unwrap_or_default().hash(&mut h);
    }
    #[cfg(target_os = "macos")]
    {
        let mut names: Vec<String> = std::fs::read_dir("/Volumes")
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        names.sort();
        names.hash(&mut h);
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "android", target_os = "ios")))]
    {
        use sysinfo::Disks;
        let mut mounts: Vec<String> = Disks::new_with_refreshed_list().list().iter().map(|d| d.mount_point().to_string_lossy().into_owned()).collect();
        mounts.sort();
        mounts.hash(&mut h);
    }
    h.finish()
}

/// What changed between two listings, by mount point: `(connected, removed)`.
pub fn changes<'a>(before: &'a [Drive], after: &'a [Drive]) -> (Vec<&'a Drive>, Vec<&'a Drive>) {
    let added = after.iter().filter(|d| !before.iter().any(|o| o.mount == d.mount)).collect();
    let removed = before.iter().filter(|o| !after.iter().any(|d| d.mount == o.mount)).collect();
    (added, removed)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn list_drives() -> Vec<Drive> {
    crate::phone::roots()
        .into_iter()
        .map(|r| {
            let space = crate::phone::space(&r.path);
            Drive {
                name: r.name,
                mount: r.path.to_string_lossy().into_owned(),
                fs: String::new(),
                total: space.map(|s| s.0).unwrap_or(0),
                available: space.map(|s| s.1).unwrap_or(0),
                media: if r.removable { Media::Removable } else { Media::Ssd },
                removable: r.removable,
                read_only: false,
                // An unreadable root still appears, so the app can explain how to allow access.
                online: true,
                remote: None,
            }
        })
        .collect()
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn display_name(label: &str, mount: &str) -> String {
    if cfg!(windows) {
        let letter = mount.trim_end_matches('\\');
        if label.is_empty() {
            format!("Local Disk ({letter})")
        } else {
            format!("{label} ({letter})")
        }
    } else if mount == "/" {
        "System".to_string()
    } else {
        Path::new(mount)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| mount.to_string())
    }
}

/// A network share split into the parts a person wants to see.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Share {
    /// Share or export name, e.g. `photos`.
    pub share: String,
    /// Host, e.g. `nas`.
    pub server: String,
    /// The location as written by the OS, e.g. `\\nas\photos` or `nas:/export/photos`.
    pub remote: String,
}

impl Share {
    /// Windows style: `photos (\\nas) (Z:)`; elsewhere `photos on nas`.
    pub fn display(&self, mount: &str) -> String {
        if cfg!(windows) {
            let letter = mount.trim_end_matches('\\');
            format!("{} (\\\\{}) ({})", self.share, self.server, letter)
        } else {
            format!("{} on {}", self.share, self.server)
        }
    }
}

/// Parse a UNC path (`\\server\share\sub`), an SMB device (`//user@server/share`)
/// or an NFS/SSHFS device (`server:/export/path`, `user@host:path`).
pub fn parse_remote(device: &str) -> Option<Share> {
    let device = device.trim();
    let unc = device.replace('\\', "/");
    if let Some(rest) = unc.strip_prefix("//") {
        let mut parts = rest.split('/').filter(|p| !p.is_empty());
        let host = parts.next()?;
        let host = host.rsplit('@').next().unwrap_or(host);
        let share = parts.next()?;
        return Some(Share {
            share: share.to_string(),
            server: host.to_string(),
            remote: format!("\\\\{host}\\{share}"),
        });
    }
    // host:/path (NFS), user@host:path (SSHFS). A drive letter like "C:" is not a host.
    let (host, path) = device.split_once(':')?;
    if host.len() < 2 || host.contains('/') || path.is_empty() {
        return None;
    }
    let host = host.rsplit('@').next().unwrap_or(host);
    let share = path.trim_end_matches('/').rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("/");
    Some(Share { share: share.to_string(), server: host.to_string(), remote: device.to_string() })
}

/// Can a connection be made to `host` on one of these ports within `wait`? For telling whether a network drive's server
/// answers at all, which the drive's own (cached) state does not say.
pub fn can_connect(host: &str, ports: &[u16], wait: std::time::Duration) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    for &port in ports {
        let (tx, host) = (tx.clone(), host.to_string());
        std::thread::spawn(move || {
            // Looking the name up can take as long as the connection, so it is part of the wait too.
            let ok = (host.as_str(), port)
                .to_socket_addrs()
                .ok()
                .and_then(|mut a| a.next())
                .is_some_and(|addr| TcpStream::connect_timeout(&addr, wait).is_ok());
            let _ = tx.send(ok);
        });
    }
    drop(tx);
    let deadline = std::time::Instant::now() + wait + std::time::Duration::from_millis(300);
    for _ in ports {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match rx.recv_timeout(left) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(_) => return false,
        }
    }
    false
}

/// The ports a share's file service listens on: SMB for `\\host\share`, NFS for `host:/path`.
fn service_ports(remote: &str) -> &'static [u16] {
    if remote.starts_with("\\\\") || remote.starts_with("//") {
        &[445, 139]
    } else {
        &[2049, 111]
    }
}

/// Mark each network drive that cannot be reached as offline: asked of the server, every time this is called, all at
/// once (a drive that is mapped keeps looking fine for a while after its server has gone). Takes at most about two seconds.
pub fn check_network(drives: &mut [Drive]) {
    let wait = std::time::Duration::from_millis(1500);
    let jobs: Vec<(usize, String, &'static [u16])> = drives
        .iter()
        .enumerate()
        .filter(|(_, d)| d.media == Media::Network && d.online)
        .filter_map(|(i, d)| {
            let share = parse_remote(d.remote.as_deref()?)?;
            Some((i, share.server, service_ports(&share.remote)))
        })
        .collect();
    let answers: Vec<(usize, bool)> = std::thread::scope(|scope| {
        let handles: Vec<_> = jobs.iter().map(|(i, host, ports)| scope.spawn(move || (*i, can_connect(host, ports, wait)))).collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for (i, ok) in answers {
        if !ok {
            drives[i].online = false;
        }
    }
}

/// The volume containing `path`: the drive with the longest matching mount point.
pub fn volume_for<'a>(path: &Path, drives: &'a [Drive]) -> Option<&'a Drive> {
    drives
        .iter()
        .filter(|d| path.starts_with(&d.mount))
        .max_by_key(|d| d.mount.len())
}

/// Media type for a path, treating UNC paths as network even when unmapped.
pub fn media_for(path: &Path, drives: &[Drive]) -> Media {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\") && !s.starts_with(r"\\?\") {
        return Media::Network;
    }
    volume_for(path, drives).map(|d| d.media).unwrap_or(Media::Unknown)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{display_name, is_hidden_unix_mount, normalize_mac_remote, parse_remote, Drive, Media, NETWORK_FS};
    use std::ffi::CStr;
    use std::path::Path;

    const MNT_RDONLY: u32 = 0x0000_0001;

    /// `sysinfo` skips every non-local volume on macOS (`kCFURLVolumeIsLocalKey == false`),
    /// which hides SMB, AFP, NFS and WebDAV shares mounted under `/Volumes`. Read the
    /// live mount table non-blockingly (`MNT_NOWAIT`) so network locations appear in This Mac.
    pub(super) fn add_network_drives(out: &mut Vec<Drive>) {
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if count <= 0 {
            return;
        }
        let mut stats: Vec<libc::statfs> = Vec::with_capacity(count as usize);
        let bufsize = (count as usize).saturating_mul(std::mem::size_of::<libc::statfs>()) as libc::c_int;
        let actual = unsafe { libc::getfsstat(stats.as_mut_ptr(), bufsize, libc::MNT_NOWAIT) };
        if actual <= 0 {
            return;
        }
        unsafe { stats.set_len((actual as usize).min(count as usize)) };

        for s in stats {
            if (s.f_flags & (libc::MNT_LOCAL as u32)) != 0 || (s.f_flags & (libc::MNT_DONTBROWSE as u32)) != 0 {
                continue;
            }
            let mount = unsafe { CStr::from_ptr(s.f_mntonname.as_ptr()) }.to_string_lossy().into_owned();
            let fs = unsafe { CStr::from_ptr(s.f_fstypename.as_ptr()) }.to_string_lossy().to_lowercase();
            let read_only = (s.f_flags & MNT_RDONLY) != 0;
            if mount == "/"
                || mount.starts_with("/System")
                || out.iter().any(|o| o.mount == mount)
                || is_hidden_unix_mount(&mount, &fs, read_only, true, || {
                    Path::new(&mount).join("FAST File Explorer.app").exists()
                })
            {
                continue;
            }
            if !NETWORK_FS.contains(&fs.as_str()) && !mount.starts_with("/Volumes/") {
                continue;
            }
            let from = unsafe { CStr::from_ptr(s.f_mntfromname.as_ptr()) }.to_string_lossy().into_owned();
            let share = normalize_mac_remote(&from).as_deref().and_then(parse_remote);
            let bsize = s.f_bsize as u64;
            let total = s.f_blocks.saturating_mul(bsize);
            let available = s.f_bavail.saturating_mul(bsize);
            out.push(Drive {
                name: match &share {
                    Some(sh) => sh.display(&mount),
                    None => display_name(&from, &mount),
                },
                remote: share.map(|sh| sh.remote).or_else(|| (!from.is_empty()).then_some(from)),
                mount,
                fs,
                total,
                available,
                media: Media::Network,
                removable: false,
                read_only,
                online: true,
            });
        }
    }
}

#[cfg(windows)]
mod win {
    use super::{Drive, Media};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_REMOTE: u32 = 4;
    const DRIVE_CDROM: u32 = 5;
    const FILE_READ_ONLY_VOLUME: u32 = 0x0008_0000;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn text(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    /// Label, file system and flags of a volume, or `None` when there is nothing to read
    /// (an empty card reader or DVD drive, a locked BitLocker drive).
    fn volume_info(root: &[u16]) -> Option<(String, String, u32)> {
        use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;
        let (mut label, mut fs) = ([0u16; 261], [0u16; 64]);
        let mut flags = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                root.as_ptr(), label.as_mut_ptr(), label.len() as u32, std::ptr::null_mut(),
                std::ptr::null_mut(), &mut flags, fs.as_mut_ptr(), fs.len() as u32,
            )
        };
        (ok != 0).then(|| (text(&label), text(&fs), flags))
    }

    /// Feeds the drive letters that exist, and for removable and optical drives whether
    /// anything is in them, into `h`: inserting a card into a reader that already has a
    /// letter changes no letters, but it changes this.
    pub fn signature(h: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        let mask = unsafe { GetLogicalDrives() };
        mask.hash(h);
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let root = wide(&format!("{}:\\", (b'A' + i as u8) as char));
            let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
            if kind == DRIVE_REMOVABLE || kind == DRIVE_CDROM {
                volume_info(&root).map(|(label, ..)| label).hash(h);
            }
        }
    }

    /// sysinfo lists the disks that answer. This adds every other drive letter, so that
    /// nothing is missing from "This PC": mapped network drives, and removable, optical or
    /// locked drives that sysinfo leaves out (shown as "No media" when they are empty).
    /// Each is queried on its own thread with a timeout: a dead share can block for tens of
    /// seconds, which is what makes "This PC" slow.
    pub fn add_other_drives(out: &mut Vec<Drive>) {
        let mask = unsafe { GetLogicalDrives() };
        let mut pending = Vec::new();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = (b'A' + i as u8) as char;
            let mount = format!("{letter}:\\");
            if out.iter().any(|d| d.mount.eq_ignore_ascii_case(&mount)) {
                continue;
            }
            let root = wide(&mount);
            let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
            // 0 unknown, 1 no root: nothing to show.
            if kind <= 1 {
                continue;
            }
            let (tx, rx) = mpsc::channel();
            let (name_tx, name_rx) = mpsc::channel();
            let local = wide(&format!("{letter}:"));
            // The share name comes from the connection table and is normally
            // instant, but it gets its own thread so it can never hold up the list.
            if kind == DRIVE_REMOTE {
                std::thread::spawn(move || {
                    let mut buf = [0u16; 1024];
                    let mut len = buf.len() as u32;
                    let rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
                    let _ = name_tx.send((rc == 0).then(|| text(&buf)));
                });
            }
            std::thread::spawn(move || {
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                let ok = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut avail, &mut total, &mut free) };
                let info = if kind == DRIVE_REMOTE { None } else { volume_info(&root) };
                let _ = tx.send((ok != 0, total, avail, info));
            });
            pending.push((letter, mount, kind, rx, name_rx));
        }
        for (letter, mount, kind, rx, name_rx) in pending {
            let (ok, total, available, info) = rx.recv_timeout(Duration::from_millis(1500)).unwrap_or((false, 0, 0, None));
            if kind == DRIVE_REMOTE {
                let share = name_rx.recv_timeout(Duration::from_millis(400)).ok().flatten().and_then(|unc| super::parse_remote(&unc));
                out.push(Drive {
                    name: match &share {
                        Some(s) => s.display(&mount),
                        None => format!("Network Drive ({letter}:)"),
                    },
                    remote: share.map(|s| s.remote),
                    mount,
                    fs: "network".into(),
                    total,
                    available,
                    media: Media::Network,
                    removable: false,
                    read_only: false,
                    online: ok,
                });
                continue;
            }
            let removable = kind == DRIVE_REMOVABLE || kind == DRIVE_CDROM;
            let (label, fs, flags) = info.clone().unwrap_or_default();
            let generic = match kind {
                DRIVE_CDROM => "DVD Drive",
                DRIVE_REMOVABLE => "Removable Disk",
                _ => "Local Disk",
            };
            out.push(Drive {
                name: if label.is_empty() { format!("{generic} ({letter}:)") } else { format!("{label} ({letter}:)") },
                remote: None,
                mount,
                fs: fs.to_lowercase(),
                total,
                available,
                media: if removable { Media::Removable } else { Media::Unknown },
                removable,
                read_only: kind == DRIVE_CDROM || flags & FILE_READ_ONLY_VOLUME != 0,
                // Something readable is in it. An empty drive is listed, as "No media".
                online: ok || info.is_some(),
            });
        }
        add_unseen_mappings(out, mask);
    }

    /// Mapped drives belong to a logon session. A process started as
    /// administrator runs in a different one (UAC), so Windows does not list
    /// the user's mapped letters for it at all. They are still recorded under
    /// `HKEY_CURRENT_USER\Network`, so read them from there and mount each by
    /// its UNC path, which an elevated process can open with the user's
    /// saved credentials or the machine's.
    fn add_unseen_mappings(out: &mut Vec<Drive>, mask: u32) {
        for (letter, remote) in registry_mappings() {
            let idx = (letter.to_ascii_uppercase() as u8 - b'A') as u32;
            if mask & (1 << idx) != 0 {
                continue; // Visible normally, already handled above.
            }
            let Some(share) = super::parse_remote(&remote) else { continue };
            let mount = format!("{}\\", share.remote);
            if out.iter().any(|d| d.mount.eq_ignore_ascii_case(&mount)) {
                continue;
            }
            let wide: Vec<u16> = mount.encode_utf16().chain(std::iter::once(0)).collect();
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
                let _ = tx.send((ok != 0, total, avail));
            });
            let (online, total, available) =
                rx.recv_timeout(Duration::from_millis(1500)).unwrap_or((false, 0, 0));
            out.push(Drive {
                name: share.display(&format!("{}:", letter.to_ascii_uppercase())),
                remote: Some(share.remote),
                mount,
                fs: "network".into(),
                total,
                available,
                media: Media::Network,
                removable: false,
                read_only: false,
                online,
            });
        }
    }

    /// `(letter, \\server\share)` for every persistent mapping of the user.
    fn registry_mappings() -> Vec<(char, String)> {
        use windows_sys::Win32::System::Registry::{
            RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ,
        };
        let mut found = Vec::new();
        unsafe {
            let root: Vec<u16> = "Network\0".encode_utf16().collect();
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_CURRENT_USER, root.as_ptr(), 0, KEY_READ, &mut key) != 0 {
                return found;
            }
            let mut index = 0;
            loop {
                let mut name = [0u16; 16];
                let mut len = name.len() as u32;
                let rc = RegEnumKeyExW(
                    key, index, name.as_mut_ptr(), &mut len,
                    std::ptr::null(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
                );
                if rc != 0 {
                    break;
                }
                index += 1;
                let letter = String::from_utf16_lossy(&name[..len as usize]);
                let mut chars = letter.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else { continue };
                if !c.is_ascii_alphabetic() {
                    continue;
                }
                let value: Vec<u16> = "RemotePath\0".encode_utf16().collect();
                let mut buf = [0u16; 1024];
                let mut size = (buf.len() * 2) as u32;
                let sub: Vec<u16> = letter.encode_utf16().chain(std::iter::once(0)).collect();
                let rc = RegGetValueW(
                    key, sub.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ,
                    std::ptr::null_mut(), buf.as_mut_ptr().cast(), &mut size,
                );
                if rc == 0 {
                    let end = buf.iter().position(|&x| x == 0).unwrap_or(buf.len());
                    found.push((c, String::from_utf16_lossy(&buf[..end])));
                }
            }
            RegCloseKey(key);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(mount: &str, media: Media) -> Drive {
        Drive {
            name: mount.into(), mount: mount.into(), fs: "ext4".into(), total: 100, available: 50,
            media, removable: false, read_only: false, online: true, remote: None,
        }
    }

    #[test]
    fn a_server_that_answers_is_reachable_and_one_that_does_not_is_not() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(can_connect("127.0.0.1", &[port], std::time::Duration::from_millis(1000)));
        drop(l);
        assert!(!can_connect("127.0.0.1", &[port], std::time::Duration::from_millis(500)));
        assert_eq!(service_ports(r"\\nas\photos"), &[445, 139]);
        assert_eq!(service_ports("nas:/export"), &[2049, 111]);
    }

    #[test]
    fn parses_network_locations() {
        let s = parse_remote(r"\\nas\photos\2024").unwrap();
        assert_eq!((s.server.as_str(), s.share.as_str()), ("nas", "photos"));
        assert_eq!(s.remote, r"\\nas\photos");
        let s = parse_remote("//user@fileserver/backup").unwrap();
        assert_eq!((s.server.as_str(), s.share.as_str()), ("fileserver", "backup"));
        let s = parse_remote("nas.local:/export/media").unwrap();
        assert_eq!((s.server.as_str(), s.share.as_str()), ("nas.local", "media"));
        let s = parse_remote("me@host:docs").unwrap();
        assert_eq!((s.server.as_str(), s.share.as_str()), ("host", "docs"));
        assert!(parse_remote("C:").is_none());
        assert!(parse_remote("/dev/sda1").is_none());
        assert!(parse_remote("").is_none());
    }

    #[test]
    fn picks_longest_mount_prefix() {
        let drives = vec![drive("/", Media::Ssd), drive("/mnt/nas", Media::Network)];
        assert_eq!(volume_for(Path::new("/mnt/nas/a/b"), &drives).unwrap().mount, "/mnt/nas");
        assert_eq!(volume_for(Path::new("/home/x"), &drives).unwrap().mount, "/");
        assert_eq!(media_for(Path::new("/mnt/nas/a"), &drives), Media::Network);
        assert_eq!(media_for(Path::new(r"\\server\share\x"), &drives), Media::Network);
    }

    #[test]
    fn reports_what_was_connected_and_removed() {
        let before = vec![drive("/", Media::Ssd), drive("/media/old", Media::Removable)];
        let after = vec![drive("/", Media::Ssd), drive("/media/usb", Media::Removable)];
        let (added, removed) = changes(&before, &after);
        assert_eq!(added.iter().map(|d| d.mount.as_str()).collect::<Vec<_>>(), ["/media/usb"]);
        assert_eq!(removed.iter().map(|d| d.mount.as_str()).collect::<Vec<_>>(), ["/media/old"]);
        let (a, r) = changes(&after, &after);
        assert!(a.is_empty() && r.is_empty());
    }

    #[test]
    fn signature_is_steady_while_nothing_changes() {
        assert_eq!(signature(), signature());
    }

    #[test]
    fn lists_at_least_one_volume_here() {
        let drives = list_drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().all(|d| d.available <= d.total));
    }

    #[test]
    fn hides_macos_app_installer_volume_and_normalizes_network_remotes() {
        assert!(is_hidden_unix_mount("/Volumes/FAST File Explorer", "hfs", true, true, || true));
        assert!(is_hidden_unix_mount("/Volumes/FAST File Explorer 1", "apfs", true, true, || false));
        assert!(is_hidden_unix_mount("/Volumes/CustomName", "hfs", true, true, || true));
        assert!(!is_hidden_unix_mount("/Volumes/Backup", "apfs", false, true, || false));
        assert!(!is_hidden_unix_mount("/Volumes/media", "smbfs", false, true, || false));
        assert_eq!(normalize_mac_remote("//user@nas/media").as_deref(), Some("//user@nas/media"));
        assert_eq!(normalize_mac_remote("smb://user@nas/media").as_deref(), Some("//user@nas/media"));
        assert_eq!(normalize_mac_remote("https://dav.example.com/files"), None);
    }
}

// ---------------------------------------------------------------------------
// Windows `net use`: what is asked of it, kept apart so it can be tested anywhere
// ---------------------------------------------------------------------------

/// The arguments that connect `remote` (`\\server\share`) to a drive letter (`letter` like "Z:", or "*" for a free one;
/// `None` connects it with no letter), signed in as `login` (user name, password) when given. The password goes
/// before the switches, as `net use` wants it; an empty one is passed as an empty argument (a blank password).
pub fn net_use_connect(letter: Option<&str>, remote: &str, login: Option<(&str, &str)>) -> Vec<String> {
    let mut args = vec!["use".to_string()];
    if let Some(l) = letter {
        args.push(l.to_string());
    }
    args.push(remote.to_string());
    if let Some((user, password)) = login {
        args.push(password.to_string());
        args.push(format!("/user:{user}"));
    }
    args.push("/persistent:yes".to_string());
    args
}

/// The arguments that disconnect a drive letter ("Z:") or a share (`\\server\share`), even with files open.
pub fn net_use_delete(target: &str) -> Vec<String> {
    vec!["use".into(), target.trim_end_matches('\\').to_string(), "/delete".into(), "/y".into()]
}

/// The drive letter ("Z:") a mount point stands for, when it is one.
pub fn drive_letter_of(mount: &str) -> Option<String> {
    let b = mount.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':').then(|| mount[..2].to_ascii_uppercase())
}

/// The server of `\\server\share`.
pub fn unc_server(remote: &str) -> Option<&str> {
    remote.strip_prefix("\\\\")?.split('\\').next().filter(|s| !s.is_empty())
}

/// What happened to one drive when signing in again.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReloginOutcome {
    pub mount: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// Sign in again to existing network drives with a new login, without making them again. Every drive is let go of
/// first (so the server sees no sign-in under the old name, which it would refuse a second one beside), then each is
/// connected again at its letter, with its address, kept for next time. A drive that cannot be signed in is put back
/// the way it was connected, so a wrong password does not cost the drive. `run` carries out one `net use`; `busy` says
/// whether a copy is using a drive (such a drive is left alone).
pub fn relogin(
    drives: &[Drive],
    mounts: &[String],
    user: &str,
    password: &str,
    busy: &dyn Fn(&str) -> bool,
    run: &mut dyn FnMut(Vec<String>) -> std::result::Result<String, String>,
) -> Vec<ReloginOutcome> {
    let fail = |mount: &str, why: String| ReloginOutcome { mount: mount.to_string(), ok: false, error: Some(why) };
    let mut out = Vec::new();
    let mut jobs: Vec<(String, Option<String>, String)> = Vec::new();
    for mount in mounts {
        let remote = drives.iter().find(|d| &d.mount == mount && d.media == Media::Network).and_then(|d| d.remote.clone());
        match remote {
            _ if busy(mount) => out.push(fail(mount, "A copy is using this drive. Sign in again when it has finished.".into())),
            Some(remote) => jobs.push((mount.clone(), drive_letter_of(mount), remote)),
            None => out.push(fail(mount, "Windows did not say where this drive points, so it cannot be signed in again.".into())),
        }
    }
    for (_, letter, remote) in &jobs {
        let _ = run(net_use_delete(letter.as_deref().unwrap_or(remote)));
    }
    for (mount, letter, remote) in jobs {
        match run(net_use_connect(letter.as_deref(), &remote, Some((user, password)))) {
            Ok(_) => out.push(ReloginOutcome { mount, ok: true, error: None }),
            Err(e) => {
                let restored = run(net_use_connect(letter.as_deref(), &remote, None)).is_ok();
                let note = if restored { "The drive is connected again with its earlier sign-in." } else { "The drive is now disconnected: use Map network drive to add it again." };
                out.push(fail(&mount, format!("{e} {note}")));
            }
        }
    }
    out
}

#[cfg(test)]
mod net_use_tests {
    use super::*;

    fn net(mount: &str, remote: &str) -> Drive {
        Drive { name: mount.into(), mount: mount.into(), fs: "smb".into(), total: 0, available: 0, media: Media::Network, removable: false, read_only: false, online: true, remote: Some(remote.into()) }
    }

    #[test]
    fn signing_in_again_lets_go_of_all_first_then_connects_each_with_the_new_login() {
        let drives = [net(r"S:\", r"\\nas\games"), net(r"T:\", r"\\nas\media")];
        let mut seen: Vec<Vec<String>> = Vec::new();
        let out = relogin(&drives, &[r"S:\".into(), r"T:\".into()], "me", "pw", &|_| false, &mut |a| {
            seen.push(a);
            Ok(String::new())
        });
        assert!(out.iter().all(|o| o.ok));
        let verbs: Vec<&str> = seen.iter().map(|a| a.get(2).map(String::as_str).unwrap_or("")).collect();
        assert_eq!(&verbs[..2], ["/delete", "/delete"], "both are let go of before any is connected: {seen:?}");
        assert_eq!(seen[2], ["use", "S:", r"\\nas\games", "pw", "/user:me", "/persistent:yes"]);
        assert_eq!(seen[3], ["use", "T:", r"\\nas\media", "pw", "/user:me", "/persistent:yes"]);
    }

    #[test]
    fn a_refused_login_puts_the_drive_back_as_it_was() {
        let drives = [net(r"S:\", r"\\nas\games")];
        let mut calls = 0;
        let out = relogin(&drives, &[r"S:\".into()], "me", "wrong", &|_| false, &mut |a| {
            calls += 1;
            if a.iter().any(|x| x == "/user:me") { Err("System error 1326 has occurred.".into()) } else { Ok(String::new()) }
        });
        assert!(!out[0].ok);
        let e = out[0].error.as_deref().unwrap();
        assert!(e.contains("1326") && e.contains("earlier sign-in"), "{e}");
        assert_eq!(calls, 3, "delete, failed connect, restore");
    }

    #[test]
    fn a_drive_that_is_busy_or_unknown_is_left_alone() {
        let drives = [net(r"S:\", r"\\nas\games")];
        let mut ran = 0;
        let out = relogin(&drives, &[r"S:\".into(), r"Q:\".into()], "me", "pw", &|m| m == r"S:\", &mut |_| {
            ran += 1;
            Ok(String::new())
        });
        assert_eq!(ran, 0);
        assert!(out.iter().all(|o| !o.ok));
        assert!(out[0].error.as_deref().unwrap().contains("copy is using"));
    }

    #[test]
    fn connecting_puts_the_password_before_the_switches() {
        assert_eq!(net_use_connect(Some("Z:"), r"\\nas\media", Some(("me", "pw"))), ["use", "Z:", r"\\nas\media", "pw", "/user:me", "/persistent:yes"]);
        assert_eq!(net_use_connect(Some("*"), r"\\nas\media", None), ["use", "*", r"\\nas\media", "/persistent:yes"]);
        assert_eq!(net_use_connect(None, r"\\nas\media", Some(("D\\me", ""))), ["use", r"\\nas\media", "", r"/user:D\me", "/persistent:yes"]);
    }

    #[test]
    fn disconnecting_names_the_letter_or_share() {
        assert_eq!(net_use_delete("Z:\\"), ["use", "Z:", "/delete", "/y"]);
        assert_eq!(net_use_delete(r"\\nas\media"), ["use", r"\\nas\media", "/delete", "/y"]);
    }

    #[test]
    fn letters_and_servers_are_read_from_mounts_and_addresses() {
        assert_eq!(drive_letter_of(r"z:\"), Some("Z:".into()));
        assert_eq!(drive_letter_of(r"\\nas\media"), None);
        assert_eq!(unc_server(r"\\nas\media"), Some("nas"));
        assert_eq!(unc_server("nas:/export"), None);
    }
}
