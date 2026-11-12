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

const NETWORK_FS: &[&str] = &[
    "nfs", "nfs4", "cifs", "smb", "smb2", "smb3", "smbfs", "afpfs", "webdav", "davfs",
    "fuse.sshfs", "sshfs", "9p", "ncpfs", "fuse.rclone", "ceph", "glusterfs",
];

#[cfg(not(any(target_os = "android", target_os = "ios", windows)))]
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
            if PSEUDO_FS.contains(&fs.as_str())
                || mount.starts_with("/snap")
                || mount.starts_with("/boot")
                || mount.starts_with("/System/Volumes")
                || mount.starts_with("/private/var")
                || d.total_space() == 0
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
    win::add_network_drives(&mut out);
    out.sort_by(|a, b| a.mount.cmp(&b.mount));
    out
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn list_drives() -> Vec<Drive> {
    // Mobile storage roots are granted by the OS per app; see docs/ROADMAP.md.
    Vec::new()
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

#[cfg(windows)]
mod win {
    use super::{Drive, Media};
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};

    const DRIVE_REMOTE: u32 = 4;

    /// sysinfo lists fixed and removable disks only. Add mapped network
    /// drives, querying each on its own thread with a timeout: a dead share
    /// can block for tens of seconds, which is what makes "This PC" slow.
    pub fn add_network_drives(out: &mut Vec<Drive>) {
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
            let wide: Vec<u16> = mount.encode_utf16().chain(std::iter::once(0)).collect();
            if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_REMOTE {
                continue;
            }
            let (tx, rx) = mpsc::channel();
            let (name_tx, name_rx) = mpsc::channel();
            let local: Vec<u16> = format!("{letter}:").encode_utf16().chain(std::iter::once(0)).collect();
            // The share name comes from the connection table and is normally
            // instant, but it gets its own thread so it can never hold up the list.
            std::thread::spawn(move || {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                let rc = unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) };
                let name = (rc == 0).then(|| {
                    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    String::from_utf16_lossy(&buf[..end])
                });
                let _ = name_tx.send(name);
            });
            std::thread::spawn(move || {
                let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
                let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, &mut total, &mut free) };
                let _ = tx.send((ok != 0, total, avail));
            });
            pending.push((letter, mount, rx, name_rx));
        }
        for (letter, mount, rx, name_rx) in pending {
            let (online, total, available) =
                rx.recv_timeout(Duration::from_millis(1500)).unwrap_or((false, 0, 0));
            let share = name_rx
                .recv_timeout(Duration::from_millis(400))
                .ok()
                .flatten()
                .and_then(|unc| super::parse_remote(&unc));
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
                online,
            });
        }
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
    fn lists_at_least_one_volume_here() {
        let drives = list_drives();
        assert!(!drives.is_empty());
        assert!(drives.iter().all(|d| d.available <= d.total));
    }
}
