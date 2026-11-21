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
        *n = unique_name(&mut seen, std::mem::take(n));
    }
}

/// One name made unique among those already `seen`; the same rule as [`unique_names`], one at a time.
pub fn unique_name(seen: &mut std::collections::HashSet<String>, name: String) -> String {
    let n = if name.contains('/') { name.replace('/', "_") } else { name };
    let n = if n.is_empty() { "_".to_string() } else { n };
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
    candidate
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

/// The part that talks to a device. Phones are not read from phones, and the USB layer under
/// `mtp-rs` has no device list on Android, so mobile builds leave this out.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
mod engine {
    use super::*;
    use crate::remote::{fail, Control, Entry, Session};
    use crate::error::{CoreError, Result};
    use bytes::Bytes;
    use mtp_rs::{ByteRange, Error as MtpError, ListingItem, MtpDevice, NewObjectInfo, ObjectHandle, ObjectInfo, Storage, UtcOffset};
    use crate::entry::ListProgress;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Duration;

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
        let serial = info.serial_number.as_deref().filter(|s| !s.is_empty());
        let device = rt
            .block_on(async {
                // Windows keeps a phone behind its own portable-device service, and that service
                // can know the phone by a different serial number than the USB port reports
                // (Samsung and Pixel phones do this). Asking by serial then finds nothing there,
                // falls back to plain USB, and Windows refuses that ("not supported"). Asking by
                // port lets the library match the phone by its model instead, so that comes first.
                if cfg!(windows) {
                    match MtpDevice::open_by_location(info.location_id).await {
                        Ok(device) => return Ok(device),
                        Err(by_port) => {
                            return match serial {
                                Some(serial) => MtpDevice::open_by_serial(serial).await.map_err(|_| by_port),
                                None => Err(by_port),
                            }
                        }
                    }
                }
                match serial {
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
        /// Set when the device said a handle it was given is no longer valid (see `retrying`).
        stale: bool,
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
            Ok(Box::new(Mtp { rt, _device: device, storages, storage_names: names, nodes: HashMap::new(), offset, dead: false, stale: false }))
        }

        fn note<T>(&mut self, r: std::result::Result<T, MtpError>) -> Result<T> {
            r.map_err(|e| {
                if e.is_disconnected() || matches!(e, MtpError::DeviceReset) {
                    self.dead = true;
                }
                self.stale |= e.is_stale_handle();
                explain(e)
            })
        }

        /// Runs one action; when the device answers that a handle is stale (Android hands out new
        /// ids for items after it rescans its media, so an id remembered from an earlier listing can
        /// stop working), forgets everything remembered and runs it once more from the paths, which
        /// looks each item up again. Only one more try: a second failure is a real one.
        fn retrying<T>(&mut self, mut action: impl FnMut(&mut Self) -> Result<T>) -> Result<T> {
            self.stale = false;
            let first = action(self);
            if first.is_ok() || !self.stale {
                return first;
            }
            self.stale = false;
            self.nodes.clear();
            action(self)
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
                        self.stale |= e.is_stale_handle();
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
    struct Chunks<'a, R: std::io::Read> {
        file: R,
        ctl: &'a Control<'a>,
        /// Counted here only for files read from this computer; a pass-through is counted by whoever writes.
        count: bool,
    }

    impl<R: std::io::Read> Iterator for Chunks<'_, R> {
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
                    if self.count {
                        (self.ctl.on_bytes)(n as u64);
                    }
                    Some(Ok(Bytes::from(buf)))
                }
                Err(e) => Some(Err(e)),
            }
        }
    }

    impl Mtp {
        fn list_once(&mut self, path: &str) -> Result<Vec<Entry>> {
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

        /// `list_once`, handing over batches as the device yields its items.
        fn list_stream_once(&mut self, path: &str, cancel: &AtomicBool, restarted: bool, sink: &mut dyn FnMut(&[Entry], ListProgress)) -> Result<Vec<Entry>> {
            let node = self.resolve(path)?;
            let (storage, parent) = match node {
                Node::Storage(i) => (i, None),
                Node::Object { storage, handle, dir: true } => (storage, Some(handle)),
                // The list of storages is short and already in hand.
                _ => return self.list_once(path),
            };
            let base = path.trim_end_matches('/').to_string();
            let started = self.rt.block_on(self.storages[storage].list_objects_stream(parent));
            let mut listing = started.map_err(|e| self.listing_failed(e))?;
            let total = listing.total();
            let (mut seen, mut out, mut batch) = (std::collections::HashSet::new(), Vec::with_capacity(total), Vec::new());
            let (mut done, mut skipped, mut flushed, mut first) = (0usize, 0usize, Instant::now(), true);
            let mut last_error = None;
            loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err(fail("Cancelled."));
                }
                let Some(item) = self.rt.block_on(listing.next()) else { break };
                done += 1;
                match item {
                    Err(e) => return Err(self.listing_failed(e)),
                    Ok(ListingItem::Skipped(s)) => {
                        skipped += 1;
                        last_error = Some(s.error);
                    }
                    Ok(ListingItem::Object(o)) => {
                        let mut entry = self.entry_of(&o);
                        entry.name = unique_name(&mut seen, entry.name);
                        self.nodes.insert(format!("{base}/{}", entry.name), Node::Object { storage, handle: o.handle, dir: o.is_folder() });
                        batch.push(entry.clone());
                        out.push(entry);
                    }
                }
                // The first items go out at once; after that, a batch every tenth of a second.
                if first && !batch.is_empty() || flushed.elapsed() >= Duration::from_millis(100) && !batch.is_empty() {
                    sink(&batch, ListProgress { done, total, restart: first && restarted });
                    batch.clear();
                    flushed = Instant::now();
                    first = false;
                }
            }
            // A device that gave handles and then refused every one of them has told us nothing.
            if out.is_empty() && skipped > 0 {
                if let Some(e) = last_error {
                    return Err(explain_at("listing this folder", e));
                }
            }
            sink(&batch, ListProgress { done, total, restart: first && restarted });
            Ok(out)
        }

        /// Note what a failed listing says about the connection, and word it for the person.
        fn listing_failed(&mut self, e: MtpError) -> CoreError {
            if e.is_disconnected() || matches!(e, MtpError::DeviceReset) {
                self.dead = true;
            }
            self.stale |= e.is_stale_handle();
            explain_at("listing this folder", e)
        }

        fn thumbnail_once(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
            let Node::Object { storage, handle, dir: false } = self.resolve(path)? else {
                return Err(fail("Only files have previews."));
            };
            let got = self.rt.block_on(self.storages[storage].thumbnail(handle));
            self.note(got).map(Some)
        }

        fn download_once(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
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

        fn upload_once(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
            let (parent, name) = split(remote)?;
            let (storage, parent_handle) = self.place(&parent)?;
            // Replacing: the device would otherwise keep two items with one name.
            if let Ok(Node::Object { handle, dir: false, .. }) = self.resolve(remote) {
                let r = self.rt.block_on(self.storages[storage].delete(handle));
                self.note(r)?;
            }
            let size = std::fs::metadata(local).map_err(|e| CoreError::io(local, e))?.len();
            let file = std::fs::File::open(local).map_err(|e| CoreError::io(local, e))?;
            let stream = futures::stream::iter(Chunks { file, ctl, count: true });
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

        /// A file's bytes, piece by piece, to `sink`. Not retried: pieces already handed on cannot be taken back.
        fn read_stream_once(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
            let Node::Object { storage, handle, dir: false } = self.resolve(remote)? else {
                return Err(fail("That is not a file."));
            };
            let started = self.rt.block_on(self.storages[storage].download(handle, ByteRange::Full));
            let mut dl = self.note(started)?;
            let mut total = 0u64;
            loop {
                if ctl.check().is_err() {
                    let _ = self.rt.block_on(dl.cancel(Duration::from_secs(5)));
                    return Err(fail("Cancelled."));
                }
                match self.rt.block_on(dl.next_chunk()) {
                    None => return Ok(total),
                    Some(Ok(chunk)) => {
                        if let Err(e) = sink(&chunk) {
                            let _ = self.rt.block_on(dl.cancel(Duration::from_secs(5)));
                            return Err(e);
                        }
                        total += chunk.len() as u64;
                    }
                    Some(Err(e)) => return Err(explain(e)),
                }
            }
        }

        /// A file of `size` bytes, taken from `source` as the device asks for them.
        fn write_stream_once(&mut self, source: &mut (dyn std::io::Read + Send), size: u64, remote: &str, ctl: &Control) -> Result<u64> {
            let (parent, name) = split(remote)?;
            let (storage, parent_handle) = self.place(&parent)?;
            if let Ok(Node::Object { handle, dir: false, .. }) = self.resolve(remote) {
                let r = self.rt.block_on(self.storages[storage].delete(handle));
                self.note(r)?;
            }
            // The bytes are counted as they are read; the device announces the size first, so a source that ends
            // early or runs long is an error, not a short file.
            struct Counted<'s> {
                source: &'s mut (dyn std::io::Read + Send),
                seen: u64,
                size: u64,
            }
            impl std::io::Read for Counted<'_> {
                fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                    let room = (self.size - self.seen.min(self.size)) as usize;
                    if room == 0 {
                        return Ok(0);
                    }
                    let take = buf.len().min(room);
                    let n = self.source.read(&mut buf[..take])?;
                    self.seen += n as u64;
                    Ok(n)
                }
            }
            let mut counted = Counted { source, seen: 0, size };
            let stream = futures::stream::iter(Chunks { file: &mut counted, ctl, count: true });
            let uploaded = self.rt.block_on(self.storages[storage].upload(parent_handle, NewObjectInfo::file(name, size), stream));
            let seen = counted.seen;
            self.nodes.clear();
            match uploaded {
                Ok(_) if seen == size => Ok(size),
                Ok(_) => {
                    // The source ended before the announced size: whatever the device made is not the file.
                    if let Ok(Node::Object { handle, dir: false, .. }) = self.resolve(remote) {
                        let _ = self.rt.block_on(self.storages[storage].delete(handle));
                    }
                    Err(fail("The file ended before all of it arrived."))
                }
                Err(e) => {
                    if let Some(partial) = e.partial {
                        let _ = self.rt.block_on(self.storages[storage].delete(partial));
                    }
                    self.note(Err(e.source))
                }
            }
        }

        fn mkdir_once(&mut self, path: &str) -> Result<()> {
            let (parent, name) = split(path)?;
            let (storage, parent_handle) = self.place(&parent)?;
            let r = self.rt.block_on(self.storages[storage].create_folder(parent_handle, &name));
            self.nodes.clear();
            self.note(r).map(|_| ())
        }

        fn remove_once(&mut self, path: &str, _is_dir: bool) -> Result<()> {
            let Node::Object { storage, handle, .. } = self.resolve(path)? else {
                return Err(fail("A storage cannot be deleted."));
            };
            let r = self.rt.block_on(self.storages[storage].delete(handle));
            self.nodes.clear();
            self.note(r)
        }

        fn rename_once(&mut self, from: &str, to: &str) -> Result<()> {
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
                // Some devices give a moved item a new id, and a retry from the old path would no
                // longer find it, so the second step finds it again at its new place.
                self.nodes.clear();
                if old_name != new_name {
                    let moved = format!("{}/{old_name}", p2.trim_end_matches('/'));
                    if let Node::Object { handle, .. } = self.resolve(&moved)? {
                        let r = self.rt.block_on(self.storages[storage].rename(handle, &new_name));
                        self.nodes.clear();
                        return self.note(r);
                    }
                }
                return Ok(());
            }
            if old_name != new_name {
                let r = self.rt.block_on(self.storages[storage].rename(handle, &new_name));
                self.nodes.clear();
                self.note(r)?;
            }
            self.nodes.clear();
            Ok(())
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
            self.retrying(|s| s.list_once(path))
        }

        fn list_streaming(&mut self, path: &str, cancel: &AtomicBool, sink: &mut dyn FnMut(&[Entry], ListProgress)) -> Result<Vec<Entry>> {
            let mut attempt = 0;
            self.retrying(|s| {
                attempt += 1;
                s.list_stream_once(path, cancel, attempt > 1, &mut *sink)
            })
        }

        fn thumbnail(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
            self.retrying(|s| s.thumbnail_once(path))
        }

        fn download(&mut self, remote: &str, local: &Path, ctl: &Control) -> Result<u64> {
            self.retrying(|s| s.download_once(remote, local, ctl))
        }

        fn upload(&mut self, local: &Path, remote: &str, ctl: &Control) -> Result<u64> {
            self.retrying(|s| s.upload_once(local, remote, ctl))
        }

        fn streams(&self) -> bool {
            true
        }

        fn read_stream(&mut self, remote: &str, sink: &mut dyn FnMut(&[u8]) -> Result<()>, ctl: &Control) -> Result<u64> {
            self.read_stream_once(remote, sink, ctl)
        }

        fn write_stream(&mut self, source: &mut (dyn std::io::Read + Send), size: u64, remote: &str, ctl: &Control) -> Result<u64> {
            self.write_stream_once(source, size, remote, ctl)
        }

        fn mkdir(&mut self, path: &str) -> Result<()> {
            self.retrying(|s| s.mkdir_once(path))
        }

        fn remove(&mut self, path: &str, is_dir: bool) -> Result<()> {
            self.retrying(|s| s.remove_once(path, is_dir))
        }

        fn rename(&mut self, from: &str, to: &str) -> Result<()> {
            self.retrying(|s| s.rename_once(from, to))
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
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            phone_with_serial(dirs, &format!("virtual-{n:03}"))
        }

        /// The serial names the pretend phone, which is how a test reaches it from outside.
        fn phone_with_serial(dirs: &[&Path], serial: &str) -> Box<dyn Session> {
            let rt = runtime().unwrap();
            let cfg = VirtualDeviceConfig {
                manufacturer: "Test".into(),
                model: "Pretend Phone".into(),
                serial: serial.into(),
                storages: dirs
                    .iter()
                    .enumerate()
                    .map(|(i, d)| VirtualStorageConfig { description: if i == 0 { "Internal storage".into() } else { "SD card".into() }, capacity: 1 << 30, backing_dir: d.to_path_buf(), read_only: false })
                    .collect(),
                supports_rename: true,
                // The pretend phone normally watches its folder for outside changes. Those
                // notices arrive late and in batches on macOS and Windows, and can re-key a
                // file between a listing and the next step. These tests change the folder
                // only through the phone, so they have nothing to learn from the watcher.
                watch_backing_dirs: false,
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
        fn a_streamed_listing_hands_over_everything_once_and_matches_the_plain_one() {
            let internal = tempfile::tempdir().unwrap();
            let dir = internal.path().join("DCIM");
            std::fs::create_dir_all(&dir).unwrap();
            for i in 0..300 {
                std::fs::write(dir.join(format!("IMG_{i:04}.jpg")), vec![1u8; i + 1]).unwrap();
            }
            let mut s = phone(&[internal.path()]);
            let cancel = AtomicBool::new(false);
            let mut got: Vec<String> = Vec::new();
            let mut last = ListProgress::default();
            let whole = s
                .list_streaming("/Internal storage/DCIM", &cancel, &mut |batch, p| {
                    got.extend(batch.iter().map(|e| e.name.clone()));
                    last = p;
                })
                .unwrap();
            assert_eq!(whole.len(), 300);
            assert_eq!(got.len(), 300, "every item is handed over exactly once");
            assert_eq!(last.done, 300);
            let plain: Vec<String> = s.list("/Internal storage/DCIM").unwrap().into_iter().map(|e| e.name).collect();
            let mut a = got.clone();
            let mut b = plain;
            a.sort();
            b.sort();
            assert_eq!(a, b);
            // Items listed this way can be used straight away.
            assert!(s.exists("/Internal storage/DCIM/IMG_0007.jpg").unwrap());
            // A cancelled listing stops with an error.
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            assert!(s.list_streaming("/Internal storage/DCIM", &cancel, &mut |_, _| {}).is_err());
        }

        #[test]
        fn items_the_device_renumbered_are_looked_up_again() {
            use mtp_rs::virtual_device::rekey_virtual_object;
            let internal = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(internal.path().join("Pictures")).unwrap();
            std::fs::write(internal.path().join("Pictures/a.jpg"), b"aaa").unwrap();
            std::fs::write(internal.path().join("Pictures/b.jpg"), b"bbbb").unwrap();
            std::fs::write(internal.path().join("note.txt"), b"hello").unwrap();
            let serial = "virtual-rekey-test";
            let mut s = phone_with_serial(&[internal.path()], serial);
            let cancel = AtomicBool::new(false);
            let on_bytes = |_n: u64| {};
            let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };
            let out = tempfile::tempdir().unwrap();
            let renumber = |p: &str| {
                assert!(rekey_virtual_object(serial, Path::new(p)).is_some(), "{p} was listed, so it can be renumbered");
            };

            // Listing a folder whose number changed since it was last opened.
            s.list("/Internal storage").unwrap();
            s.list("/Internal storage/Pictures").unwrap();
            renumber("Pictures");
            let mut names: Vec<String> = s.list("/Internal storage/Pictures").unwrap().into_iter().map(|e| e.name).collect();
            names.sort();
            assert_eq!(names, ["a.jpg", "b.jpg"]);

            // Reading, renaming, moving, and deleting an item that was renumbered.
            renumber("Pictures/a.jpg");
            assert_eq!(s.download("/Internal storage/Pictures/a.jpg", &out.path().join("a.jpg"), &ctl).unwrap(), 3);
            s.list("/Internal storage/Pictures").unwrap();
            renumber("Pictures/a.jpg");
            s.rename("/Internal storage/Pictures/a.jpg", "/Internal storage/Pictures/c.jpg").unwrap();
            assert!(s.exists("/Internal storage/Pictures/c.jpg").unwrap());
            s.list("/Internal storage/Pictures").unwrap();
            renumber("Pictures/c.jpg");
            s.rename("/Internal storage/Pictures/c.jpg", "/Internal storage/d.jpg").unwrap();
            assert!(s.exists("/Internal storage/d.jpg").unwrap());
            assert!(!s.exists("/Internal storage/Pictures/c.jpg").unwrap());
            s.list("/Internal storage").unwrap();
            renumber("d.jpg");
            s.remove("/Internal storage/d.jpg", false).unwrap();
            assert!(!s.exists("/Internal storage/d.jpg").unwrap());

            // Putting something into, and making a folder inside, a folder that was renumbered.
            s.list("/Internal storage/Pictures").unwrap();
            renumber("Pictures");
            std::fs::write(out.path().join("new.bin"), b"12345").unwrap();
            assert_eq!(s.upload(&out.path().join("new.bin"), "/Internal storage/Pictures/new.bin", &ctl).unwrap(), 5);
            s.list("/Internal storage/Pictures").unwrap();
            renumber("Pictures");
            s.mkdir("/Internal storage/Pictures/Trips").unwrap();
            let mut names: Vec<String> = s.list("/Internal storage/Pictures").unwrap().into_iter().map(|e| e.name).collect();
            names.sort();
            assert_eq!(names, ["Trips", "b.jpg", "new.bin"]);
            assert_eq!(std::fs::read(internal.path().join("Pictures/new.bin")).unwrap(), b"12345");

            // An item that really is gone is still reported, not retried forever.
            assert!(s.remove("/Internal storage/nothing.txt", false).is_err());
        }

        #[test]
        fn a_file_goes_from_one_phone_to_another_without_a_copy_on_this_computer() {
            let from = tempfile::tempdir().unwrap();
            let to = tempfile::tempdir().unwrap();
            let data: Vec<u8> = (0..3_000_017u32).map(|i| (i % 253) as u8).collect();
            std::fs::write(from.path().join("clip.mp4"), &data).unwrap();
            let mut a = phone(&[from.path()]);
            let mut b = phone(&[to.path()]);
            assert!(a.streams() && b.streams());
            let cancel = AtomicBool::new(false);
            let counted = std::sync::atomic::AtomicU64::new(0);
            let on_bytes = |n: u64| {
                counted.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
            };
            let ctl = Control { cancel: &cancel, on_bytes: &on_bytes };
            let n = crate::remote::relay_file(a.as_mut(), "/Internal storage/clip.mp4", b.as_mut(), "/Internal storage/clip.mp4", data.len() as u64, &ctl).unwrap();
            assert_eq!(n, data.len() as u64);
            assert_eq!(counted.load(std::sync::atomic::Ordering::Relaxed), data.len() as u64);
            assert_eq!(std::fs::read(to.path().join("clip.mp4")).unwrap(), data);
            // A size that does not match what arrives is an error, and the device keeps nothing.
            let e = crate::remote::relay_file(a.as_mut(), "/Internal storage/clip.mp4", b.as_mut(), "/Internal storage/short.mp4", data.len() as u64 + 10, &ctl);
            assert!(e.is_err());
            assert!(!to.path().join("short.mp4").exists());
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
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub use engine::{connect, found_devices};

/// No phones are listed on a phone.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn found_devices() -> Vec<Found> {
    Vec::new()
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn connect(_key: &str) -> crate::error::Result<Box<dyn super::Session>> {
    Err(super::fail("Phones and cameras cannot be opened from a phone."))
}
