//! Phones, cameras and media players, browsed in place (Windows and macOS).
//!
//! A device is written `mtp://<key>/Internal storage/DCIM`, where `<key>` is a
//! short code for the device (see [`device_key`]) and the first folder is one of
//! the device's storages. The `mtp-rs` library speaks MTP: through Windows' own
//! portable-device service on Windows, and straight over USB on macOS. This module
//! presents a device as an ordinary [`Session`], so browsing, copying, deleting and
//! renaming use the same code as for servers.
//!
//! Linux does not use this: the file manager's mount (GVFS) already holds the phone
//! there, and it appears as a normal folder. The engine itself is the same code on
//! every system and is tested against `mtp-rs`'s virtual device.

use super::{fail, Control, Entry, Session};
use crate::error::{CoreError, Result};
use bytes::Bytes;
use mtp_rs::{ByteRange, Error as MtpError, MtpDevice, NewObjectInfo, ObjectHandle, ObjectInfo, Storage, UtcOffset};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

/// A short, stable code for a device id, used as the "host" of its address.
/// The real id (`\\?\usb#vid_04e8&pid_6860#...`) is full of characters that do
/// not belong in an address.
pub fn device_key(pnp_id: &str) -> String {
    // FNV-1a, 64 bit.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in pnp_id.to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// The address of a device's top folder (its list of storages).
pub fn device_url(pnp_id: &str) -> String {
    format!("mtp://{}/", device_key(pnp_id))
}

/// A device can hold two items with one name; an address cannot. Later ones get
/// ` (2)`, ` (3)`… before the extension, and `/` never appears in a name.
pub fn unique_names(names: &mut [String]) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for n in names.iter_mut() {
        if n.contains('/') {
            *n = n.replace('/', "_");
        }
        if n.is_empty() {
            *n = "_".into();
        }
        let mut candidate = n.clone();
        let mut i = 2;
        while !seen.insert(candidate.to_lowercase()) {
            let (stem, ext) = match n.rfind('.') {
                Some(d) if d > 0 => (&n[..d], &n[d..]),
                _ => (n.as_str(), ""),
            };
            candidate = format!("{stem} ({i}){ext}");
            i += 1;
        }
        *n = candidate;
    }
}

/// What to tell the person when a device lists no storage.
pub const LOCKED_HINT: &str = "The device shows no storage. Unlock it and choose \"File transfer\" (MTP) in its USB options, then try again.";


/// A device that is plugged in and speaks MTP.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub key: String,
    pub name: String,
    pub maker: String,
    pub serial: String,
    pub vendor_id: u16,
    pub product_id: u16,
}

fn identity(vendor: u16, product: u16, serial: &Option<String>, location: u64) -> String {
    format!("{vendor:04x}:{product:04x}:{}", serial.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("@{location}")))
}

/// The MTP devices that are plugged in right now. Only Windows and macOS read phones this
/// way; elsewhere the list is empty.
pub fn found_devices() -> Vec<Found> {
    if !cfg!(any(windows, target_os = "macos")) {
        return Vec::new();
    }
    MtpDevice::list_devices()
        .unwrap_or_default()
        .into_iter()
        .map(|d| Found {
            key: device_key(&identity(d.vendor_id, d.product_id, &d.serial_number, d.location_id)),
            name: d.product.clone().unwrap_or_else(|| "Portable device".into()),
            maker: d.manufacturer.clone().unwrap_or_default(),
            serial: d.serial_number.clone().unwrap_or_default(),
            vendor_id: d.vendor_id,
            product_id: d.product_id,
        })
        .collect()
}

/// Open the device whose address host is `key`.
pub fn connect(key: &str) -> Result<Box<dyn Session>> {
    if !cfg!(any(windows, target_os = "macos")) {
        return Err(fail("Phones are opened through the file manager's mount on this system."));
    }
    let key = key.to_lowercase();
    let info = MtpDevice::list_devices()
        .map_err(explain)?
        .into_iter()
        .find(|d| device_key(&identity(d.vendor_id, d.product_id, &d.serial_number, d.location_id)) == key)
        .ok_or_else(|| fail("That device is not connected any more."))?;
    let rt = runtime()?;
    let device = rt
        .block_on(async {
            match info.serial_number.as_deref().filter(|s| !s.is_empty()) {
                Some(serial) => MtpDevice::open_by_serial(serial).await,
                None => MtpDevice::open_by_location(info.location_id).await,
            }
        })
        .map_err(|e| explain_at("opening the device", e))?;
    Mtp::new(rt, device)
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().map_err(fail)
}

/// As [`explain`], for a step whose failure is not obvious: says which step it was and keeps
/// the library's own wording, so a device that refuses something unusual can be recognised.
fn explain_at(step: &str, e: MtpError) -> CoreError {
    match e {
        MtpError::Unsupported | MtpError::Other { .. } => fail(format!("The device refused \"{step}\" ({e}; {e:?}). Unlock it, choose \"File transfer\" in its USB options, and try again.")),
        other => explain(other),
    }
}

/// A device's own error, in words a person can act on.
fn explain(e: MtpError) -> CoreError {
    fail(match e {
        MtpError::ExclusiveAccess => "Another program is using the phone. On a Mac, quit Android File Transfer, and macOS's own camera service may also hold it: run \"sudo launchctl disable system/com.apple.ptpcamerad\" in Terminal, then replug the phone.".to_string(),
        MtpError::PermissionDenied | MtpError::AccessDenied => "The device would not allow this. Unlock it and choose \"File transfer\" (MTP) in its USB options.".to_string(),
        MtpError::Disconnected | MtpError::NoDevice => "The device was disconnected.".to_string(),
        MtpError::StorageFull => "The device is full.".to_string(),
        MtpError::Timeout => "The device took too long to answer. Unlock it and try again.".to_string(),
        MtpError::Busy => "The device is busy. Try again in a moment.".to_string(),
        MtpError::Unsupported => "This device does not support that.".to_string(),
        MtpError::NotFound => "That item is not on the device any more.".to_string(),
        other => other.to_string(),
    })
}

#[derive(Clone, Copy, Debug)]
enum Node {
    Root,
    Storage(usize),
    Object { storage: usize, handle: ObjectHandle, dir: bool },
}

/// One open device.
struct Mtp {
    rt: tokio::runtime::Runtime,
    /// Kept open for as long as the storages are used.
    _device: MtpDevice,
    storages: Vec<Storage>,
    storage_names: Vec<String>,
    /// Address path -> what it is, for everything listed so far.
    nodes: HashMap<String, Node>,
    offset: UtcOffset,
    dead: bool,
}

impl Mtp {
    fn new(rt: tokio::runtime::Runtime, device: MtpDevice) -> Result<Box<dyn Session>> {
        let storages = rt.block_on(device.storages()).map_err(|e| explain_at("reading the list of storages", e))?;
        if storages.is_empty() {
            return Err(fail(LOCKED_HINT));
        }
        let mut names: Vec<String> = storages
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let d = s.info().description.trim().to_string();
                if d.is_empty() { format!("Storage {}", i + 1) } else { d }
            })
            .collect();
        unique_names(&mut names);
        let minutes = chrono::Local::now().offset().local_minus_utc() / 60;
        let offset = i16::try_from(minutes).ok().and_then(UtcOffset::from_minutes).unwrap_or_else(|| UtcOffset::from_minutes(0).expect("zero is a valid offset"));
        Ok(Box::new(Mtp { rt, _device: device, storages, storage_names: names, nodes: HashMap::new(), offset, dead: false }))
    }

    fn note<T>(&mut self, r: std::result::Result<T, MtpError>) -> Result<T> {
        r.map_err(|e| {
            if e.is_disconnected() || matches!(e, MtpError::DeviceReset) {
                self.dead = true;
            }
            explain(e)
        })
    }

    fn entry_of(&self, o: &ObjectInfo) -> Entry {
        Entry {
            name: o.filename.clone(),
            is_dir: o.is_folder(),
            size: if o.is_folder() { 0 } else { o.size },
            modified: o.modified.and_then(|m| m.to_unix_seconds_with_fallback(self.offset)),
        }
    }

    /// What is directly inside `node`, with names made unique.
    fn children(&mut self, node: Node) -> Result<Vec<(Entry, Node)>> {
        match node {
            Node::Root => Ok(self
                .storage_names
                .iter()
                .enumerate()
                .map(|(i, n)| (Entry { name: n.clone(), is_dir: true, size: 0, modified: None }, Node::Storage(i)))
                .collect()),
            Node::Storage(i) | Node::Object { storage: i, dir: true, .. } => {
                let parent = match node {
                    Node::Object { handle, .. } => Some(handle),
                    _ => None,
                };
                let listed = self.rt.block_on(self.storages[i].list_objects(parent));
                let objects = listed.map_err(|e| {
                    if e.is_disconnected() || matches!(e, MtpError::DeviceReset) {
                        self.dead = true;
                    }
                    explain_at("listing this folder", e)
                })?;
                let mut names: Vec<String> = objects.iter().map(|o| o.filename.clone()).collect();
                unique_names(&mut names);
                Ok(objects
                    .iter()
                    .zip(names)
                    .map(|(o, n)| {
                        let mut e = self.entry_of(o);
                        e.name = n;
                        (e, Node::Object { storage: i, handle: o.handle, dir: o.is_folder() })
                    })
                    .collect())
            }
            Node::Object { .. } => Err(fail("That is a file, not a folder.")),
        }
    }

    fn resolve(&mut self, path: &str) -> Result<Node> {
        let path = if path.len() > 1 { path.trim_end_matches('/') } else { path };
        if path == "/" || path.is_empty() {
            return Ok(Node::Root);
        }
        if let Some(n) = self.nodes.get(path) {
            return Ok(*n);
        }
        let (parent, name) = split(path)?;
        let pnode = self.resolve(&parent)?;
        let mut found = None;
        for (e, node) in self.children(pnode)? {
            let child = if parent == "/" { format!("/{}", e.name) } else { format!("{parent}/{}", e.name) };
            if e.name == name {
                found = Some(node);
            }
            self.nodes.insert(child, node);
        }
        found.ok_or_else(|| fail(format!("\"{name}\" was not found on the device.")))
    }

    /// The storage and parent handle for a new item inside folder `parent` (`None` = top of a storage).
    fn place(&mut self, parent: &str) -> Result<(usize, Option<ObjectHandle>)> {
        match self.resolve(parent)? {
            Node::Root => Err(fail("Open a storage first, then use one of its folders.")),
            Node::Storage(i) => Ok((i, None)),
            Node::Object { storage, handle, dir: true } => Ok((storage, Some(handle))),
            Node::Object { .. } => Err(fail("That is a file, not a folder.")),
        }
    }
}

/// Split `/a/b/c` into (`/a/b`, `c`).
fn split(path: &str) -> Result<(String, String)> {
    let t = path.trim_end_matches('/');
    let cut = t.rfind('/').ok_or_else(|| fail("Bad path."))?;
    let name = &t[cut + 1..];
    if name.is_empty() {
        return Err(fail("Pick a folder on a storage first."));
    }
    Ok((if cut == 0 { "/".into() } else { t[..cut].to_string() }, name.to_string()))
}

/// A file read in pieces for upload; stops at once when the job is cancelled.
struct Chunks<'a> {
    file: std::fs::File,
    ctl: &'a Control<'a>,
}

impl Iterator for Chunks<'_> {
    type Item = std::result::Result<Bytes, std::io::Error>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.ctl.check().is_err() {
            return Some(Err(std::io::Error::other("Cancelled.")));
        }
        let mut buf = vec![0u8; 512 * 1024];
        match std::io::Read::read(&mut self.file, &mut buf) {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                (self.ctl.on_bytes)(n as u64);
                Some(Ok(Bytes::from(buf)))
            }
            Err(e) => Some(Err(e)),
        }
    }
}

impl Session for Mtp {
    fn home(&mut self) -> Result<String> {
        if self.dead {
            return Err(fail("The device was disconnected."));
        }
        Ok("/".into())
    }

    fn list(&mut self, path: &str) -> Result<Vec<Entry>> {
        let node = self.resolve(path)?;
        let base = if matches!(node, Node::Root) { String::new() } else { path.trim_end_matches('/').to_string() };
        let kids = self.children(node)?;
        let mut out = Vec::with_capacity(kids.len());
        for (e, n) in kids {
            self.nodes.insert(format!("{base}/{}", e.name), n);
            out.push(e);
        }
        Ok(out)
    }

    fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
        let Node::Object { storage, handle, dir: false } = self.resolve(remote)? else {
            return Err(fail("That is not a file."));
        };
        let started = self.rt.block_on(self.storages[storage].download(handle, ByteRange::Full));
        let mut dl = self.note(started)?;
        let mut file = std::fs::File::create(local).map_err(|e| CoreError::io(local, e))?;
        let mut total = 0u64;
        let result: Result<()> = (|| {
            loop {
                if ctl.check().is_err() {
                    let _ = self.rt.block_on(dl.cancel(Duration::from_secs(5)));
                    return Err(fail("Cancelled."));
                }
                match self.rt.block_on(dl.next_chunk()) {
                    None => return Ok(()),
                    Some(Ok(chunk)) => {
                        std::io::Write::write_all(&mut file, &chunk).map_err(|e| CoreError::io(local, e))?;
                        total += chunk.len() as u64;
                        (ctl.on_bytes)(chunk.len() as u64);
                    }
                    Some(Err(e)) => return Err(explain(e)),
                }
            }
        })();
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(local);
        }
        result.map(|_| total)
    }

    fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
        let (parent, name) = split(remote)?;
        let (storage, parent_handle) = self.place(&parent)?;
        // Replacing: the device would otherwise keep two items with one name.
        if let Ok(Node::Object { handle, dir: false, .. }) = self.resolve(remote) {
            let r = self.rt.block_on(self.storages[storage].delete(handle));
            self.note(r)?;
        }
        let size = std::fs::metadata(local).map_err(|e| CoreError::io(local, e))?.len();
        let file = std::fs::File::open(local).map_err(|e| CoreError::io(local, e))?;
        let stream = futures::stream::iter(Chunks { file, ctl });
        let uploaded = self.rt.block_on(self.storages[storage].upload(parent_handle, NewObjectInfo::file(name, size), stream));
        self.nodes.clear();
        match uploaded {
            Ok(_) => Ok(size),
            Err(e) => {
                if let Some(partial) = e.partial {
                    let _ = self.rt.block_on(self.storages[storage].delete(partial));
                }
                self.note(Err(e.source))
            }
        }
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        let (parent, name) = split(path)?;
        let (storage, parent_handle) = self.place(&parent)?;
        let r = self.rt.block_on(self.storages[storage].create_folder(parent_handle, &name));
        self.nodes.clear();
        self.note(r).map(|_| ())
    }

    fn remove(&mut self, path: &str, _is_dir: bool) -> Result<()> {
        let Node::Object { storage, handle, .. } = self.resolve(path)? else {
            return Err(fail("A storage cannot be deleted."));
        };
        let r = self.rt.block_on(self.storages[storage].delete(handle));
        self.nodes.clear();
        self.note(r)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        let (p1, old_name) = split(from)?;
        let (p2, new_name) = split(to)?;
        let Node::Object { storage, handle, .. } = self.resolve(from)? else {
            return Err(fail("A storage cannot be renamed."));
        };
        if p1 != p2 {
            let (dest_storage, dest) = self.place(&p2)?;
            if dest_storage != storage {
                return Err(fail("Items cannot be moved between storages. Copy them, then delete the originals."));
            }
            let r = self.rt.block_on(self.storages[storage].move_object(handle, dest.unwrap_or(ObjectHandle::ROOT), None));
            self.note(r)?;
        }
        if old_name != new_name {
            let r = self.rt.block_on(self.storages[storage].rename(handle, &new_name));
            self.note(r)?;
        }
        self.nodes.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_short_stable_and_case_blind() {
        let a = device_key(r"\\?\usb#vid_04e8&pid_6860#r5cw#{6ac27878-a6fa-4155-ba85-f98f491d4f33}");
        let b = device_key(r"\\?\USB#VID_04E8&PID_6860#R5CW#{6AC27878-A6FA-4155-BA85-F98F491D4F33}");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, device_key("other"));
        assert_eq!(device_url("x"), format!("mtp://{}/", device_key("x")));
    }

    #[test]
    fn duplicate_names_get_numbers() {
        let mut n: Vec<String> = ["a.jpg", "A.jpg", "b", "a.jpg", "b", "x/y", ""].iter().map(|s| s.to_string()).collect();
        unique_names(&mut n);
        assert_eq!(n, ["a.jpg", "A (2).jpg", "b", "a (3).jpg", "b (2)", "x_y", "_"]);
    }

    // ---- the whole engine, against a pretend phone ---------------------------

    use mtp_rs::virtual_device::{VirtualDeviceConfig, VirtualStorageConfig};
    use std::sync::atomic::AtomicBool;

    fn phone(dirs: &[&Path]) -> Box<dyn Session> {
        let rt = runtime().unwrap();
        let cfg = VirtualDeviceConfig {
            manufacturer: "Test".into(),
            model: "Pretend Phone".into(),
            serial: "virtual-001".into(),
            storages: dirs
                .iter()
                .enumerate()
                .map(|(i, d)| VirtualStorageConfig { description: if i == 0 { "Internal storage".into() } else { "SD card".into() }, capacity: 1 << 30, backing_dir: d.to_path_buf(), read_only: false })
                .collect(),
            supports_rename: true,
            ..Default::default()
        };
        let device = rt.block_on(MtpDevice::builder().open_virtual(cfg)).expect("the virtual phone opens");
        Mtp::new(rt, device).unwrap()
    }

    #[test]
    fn browses_copies_renames_and_deletes_on_a_pretend_phone() {
        let internal = tempfile::tempdir().unwrap();
        let card = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(internal.path().join("DCIM/Camera")).unwrap();
        std::fs::write(internal.path().join("DCIM/Camera/IMG_1.jpg"), b"photo one").unwrap();
        std::fs::write(card.path().join("notes.txt"), b"hello").unwrap();
        let mut s = phone(&[internal.path(), card.path()]);

        assert_eq!(s.home().unwrap(), "/");
        let top: Vec<String> = s.list("/").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(top, ["Internal storage", "SD card"]);
        let dcim = s.list("/Internal storage").unwrap();
        assert!(dcim.iter().any(|e| e.name == "DCIM" && e.is_dir), "{dcim:?}");
        let cam = s.list("/Internal storage/DCIM/Camera").unwrap();
        assert_eq!(cam.len(), 1);
        assert_eq!((cam[0].name.as_str(), cam[0].is_dir, cam[0].size), ("IMG_1.jpg", false, 9));
        assert!(s.exists("/SD card/notes.txt").unwrap());
        assert!(s.list("/Nope").is_err());

        let cancel = AtomicBool::new(false);
        let seen = std::sync::atomic::AtomicU64::new(0);
        let on_bytes = |n: u64| {
            seen.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        };
        let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };

        // Phone to computer.
        let out = tempfile::tempdir().unwrap();
        let n = s.download("/Internal storage/DCIM/Camera/IMG_1.jpg", &out.path().join("got.jpg"), &ctl).unwrap();
        assert_eq!(n, 9);
        assert_eq!(std::fs::read(out.path().join("got.jpg")).unwrap(), b"photo one");
        assert_eq!(seen.load(std::sync::atomic::Ordering::Relaxed), 9);

        // Computer to phone, into a new folder; sending again replaces rather than duplicates.
        std::fs::write(out.path().join("up.bin"), vec![7u8; 1_300_000]).unwrap();
        s.mkdir("/SD card/Backup").unwrap();
        assert_eq!(s.upload(&out.path().join("up.bin"), "/SD card/Backup/up.bin", &ctl).unwrap(), 1_300_000);
        assert_eq!(s.upload(&out.path().join("up.bin"), "/SD card/Backup/up.bin", &ctl).unwrap(), 1_300_000);
        let backup = s.list("/SD card/Backup").unwrap();
        assert_eq!(backup.len(), 1, "{backup:?}");
        assert_eq!(backup[0].size, 1_300_000);

        // Rename, then move to another folder.
        s.rename("/SD card/Backup/up.bin", "/SD card/Backup/big.bin").unwrap();
        assert!(s.exists("/SD card/Backup/big.bin").unwrap());
        assert!(!s.exists("/SD card/Backup/up.bin").unwrap());
        s.rename("/SD card/Backup/big.bin", "/SD card/big.bin").unwrap();
        assert!(s.exists("/SD card/big.bin").unwrap());
        assert!(s.rename("/SD card/big.bin", "/Internal storage/big.bin").is_err(), "no moving between storages");

        // Delete a file, then the emptied folder.
        s.remove("/SD card/big.bin", false).unwrap();
        s.remove("/SD card/Backup", true).unwrap();
        let left: Vec<String> = s.list("/SD card").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(left, ["notes.txt"]);
        assert!(s.upload(&out.path().join("up.bin"), "/up.bin", &ctl).is_err(), "files go inside a storage");
    }

    #[test]
    fn a_cancelled_upload_leaves_nothing_behind() {
        let internal = tempfile::tempdir().unwrap();
        let mut s = phone(&[internal.path()]);
        let src = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("a.bin"), vec![1u8; 3_000_000]).unwrap();
        let cancel = AtomicBool::new(true);
        let on_bytes = |_n: u64| {};
        let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };
        assert!(s.upload(&src.path().join("a.bin"), "/Internal storage/a.bin", &ctl).is_err());
        assert!(s.list("/Internal storage").unwrap().is_empty());
        // A cancelled download keeps no half file.
        std::fs::write(internal.path().join("b.bin"), b"data").unwrap();
        let mut s = phone(&[internal.path()]);
        assert!(s.download("/Internal storage/b.bin", &src.path().join("b.bin"), &ctl).is_err());
        assert!(!src.path().join("b.bin").exists());
    }
}
