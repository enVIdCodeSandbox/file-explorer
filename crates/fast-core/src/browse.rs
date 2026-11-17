//! Opening archives like folders.
//!
//! A location inside an archive is written `archive://<path of the archive>!/<folder inside>`,
//! for example `archive:///home/me/photos.zip!/2024/trip`. This module lists
//! such a location, and unpacks single items out of the archive.
//!
//! Zip, tar (plain, gzip, bzip2, xz), 7z and single gz/bz2/xz files are read
//! by code in this program. Everything else (rar, cab, iso, wim, zstd tar and
//! so on) is read through 7-Zip or `bsdtar` when one of them is installed, and
//! reported as unsupported with a hint when none is.
//!
//! Nothing here ever writes inside an archive: they are read-only.
//! Unpacking refuses names that climb out of the destination folder and never
//! replaces a file that already exists.

use crate::archive::safe_relative;
use crate::entry::FileEntry;
use crate::error::{CoreError, Result};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const SCHEME: &str = "archive://";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    SevenZ,
    Gz,
    Bz2,
    Xz,
    /// Anything 7-Zip or bsdtar can read.
    External,
}

/// Names that open as folders. Longest first so `.tar.gz` wins over `.gz`.
const EXTENSIONS: &[(&str, Kind)] = &[
    (".tar.gz", Kind::TarGz),
    (".tar.bz2", Kind::TarBz2),
    (".tar.xz", Kind::TarXz),
    (".tar.zst", Kind::External),
    (".tar.lz", Kind::External),
    (".tar.z", Kind::External),
    (".tgz", Kind::TarGz),
    (".tbz2", Kind::TarBz2),
    (".tbz", Kind::TarBz2),
    (".txz", Kind::TarXz),
    (".tzst", Kind::External),
    (".zip", Kind::Zip),
    (".tar", Kind::Tar),
    (".7z", Kind::SevenZ),
    (".gz", Kind::Gz),
    (".bz2", Kind::Bz2),
    (".xz", Kind::Xz),
    (".rar", Kind::External),
    (".cab", Kind::External),
    (".iso", Kind::External),
    (".wim", Kind::External),
    (".arj", Kind::External),
    (".lzh", Kind::External),
    (".lha", Kind::External),
    (".cpio", Kind::External),
    (".rpm", Kind::External),
    (".deb", Kind::External),
    (".zst", Kind::External),
    (".lz4", Kind::External),
    (".z", Kind::External),
];

/// What kind of archive a file name says it is.
pub fn kind_of(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    EXTENSIONS.iter().find(|(ext, _)| name.len() > ext.len() && name.ends_with(ext)).map(|(_, k)| *k)
}

// ---- Locations --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    pub archive: PathBuf,
    /// Folder or file inside the archive, `/`-separated, no leading or trailing slash. Empty is the top.
    pub inner: String,
}

pub fn is_loc(s: &str) -> bool {
    s.starts_with(SCHEME)
}

pub fn loc_url(archive: &Path, inner: &str) -> String {
    let inner = inner.trim_matches('/');
    format!("{SCHEME}{}!/{inner}", archive.to_string_lossy())
}

impl Loc {
    pub fn parse(s: &str) -> Option<Loc> {
        let rest = s.strip_prefix(SCHEME)?;
        for (i, _) in rest.match_indices('!') {
            let after = &rest[i + 1..];
            if !(after.is_empty() || after.starts_with('/')) {
                continue;
            }
            let archive = Path::new(&rest[..i]);
            if kind_of(archive).is_some() {
                return Some(Loc { archive: archive.to_path_buf(), inner: after.trim_matches('/').to_string() });
            }
        }
        None
    }

    pub fn url(&self) -> String {
        loc_url(&self.archive, &self.inner)
    }

    /// The folder one level up; for the top of an archive, the archive's own folder.
    pub fn parent(&self) -> Option<String> {
        if self.inner.is_empty() {
            return self.archive.parent().map(|p| p.to_string_lossy().into_owned());
        }
        let up = self.inner.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
        Some(loc_url(&self.archive, up))
    }

    pub fn name(&self) -> String {
        if self.inner.is_empty() {
            self.archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        } else {
            self.inner.rsplit('/').next().unwrap_or("").to_string()
        }
    }
}

// ---- Index ------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Item {
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: Option<i64>,
}

struct Cached {
    archive: PathBuf,
    stamp: (u64, Option<std::time::SystemTime>),
    items: Arc<Vec<Item>>,
}

static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());
const CACHE_SLOTS: usize = 3;

/// Forget every listing held in memory.
pub fn clear_cache() {
    CACHE.lock().clear();
}

fn stamp_of(archive: &Path) -> Result<(u64, Option<std::time::SystemTime>)> {
    let m = fs::metadata(archive).map_err(|e| CoreError::io(archive, e))?;
    Ok((m.len(), m.modified().ok()))
}

/// Everything inside the archive. Listings are kept in memory for the three
/// archives opened last, and re-read when the file changes.
pub fn index(archive: &Path) -> Result<Arc<Vec<Item>>> {
    let stamp = stamp_of(archive)?;
    {
        let mut cache = CACHE.lock();
        if let Some(pos) = cache.iter().position(|c| c.archive == archive && c.stamp == stamp) {
            let hit = cache.remove(pos);
            let items = hit.items.clone();
            cache.insert(0, hit);
            return Ok(items);
        }
    }
    let kind = kind_of(archive).ok_or_else(|| CoreError::Invalid("This is not a kind of archive that can be opened.".into()))?;
    let items = Arc::new(read_items(archive, kind)?);
    let mut cache = CACHE.lock();
    cache.retain(|c| c.archive != archive);
    cache.insert(0, Cached { archive: archive.to_path_buf(), stamp, items: items.clone() });
    cache.truncate(CACHE_SLOTS);
    Ok(items)
}

/// `a\b/` -> `a/b`; names with nothing left, or that climb out, are dropped.
pub(crate) fn clean(name: &str) -> Option<String> {
    let n = name.replace('\\', "/");
    let mut parts = Vec::new();
    for p in n.split('/') {
        match p {
            "" | "." => {}
            ".." => return None,
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub(crate) fn invalid(archive: &Path, what: impl std::fmt::Display) -> CoreError {
    CoreError::Invalid(format!("{}: {what}", archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
}

fn zip_error(archive: &Path, e: zip::result::ZipError) -> CoreError {
    match e {
        zip::result::ZipError::Io(io) => CoreError::io(archive, io),
        zip::result::ZipError::UnsupportedArchive(m) if m.to_lowercase().contains("password") => invalid(archive, "this archive is protected by a password, which this app cannot enter yet."),
        other => invalid(archive, format!("not a valid zip file ({other})")),
    }
}

pub(crate) fn open(archive: &Path) -> Result<File> {
    File::open(archive).map_err(|e| CoreError::io(archive, e))
}

/// A decompressed copy of an xz file kept in a temporary file, removed when dropped.
struct TempFile {
    file: File,
    path: PathBuf,
}
impl Read for TempFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn unxz(archive: &Path) -> Result<TempFile> {
    let mut id = [0u8; 8];
    let _ = getrandom::fill(&mut id);
    let path = std::env::temp_dir().join(format!("fast-xz-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    let mut input = BufReader::new(open(archive)?);
    let mut out = BufWriter::new(File::create(&path).map_err(|e| CoreError::io(&path, e))?);
    if let Err(e) = lzma_rs::xz_decompress(&mut input, &mut out) {
        let _ = fs::remove_file(&path);
        return Err(invalid(archive, format!("not a valid xz file ({e})")));
    }
    out.flush().map_err(|e| CoreError::io(&path, e))?;
    drop(out);
    let file = File::open(&path).map_err(|e| CoreError::io(&path, e))?;
    Ok(TempFile { file, path })
}

/// The decompressed bytes of a gz, bz2 or xz file.
pub(crate) fn decoded(archive: &Path, kind: Kind) -> Result<Box<dyn Read>> {
    Ok(match kind {
        Kind::TarGz | Kind::Gz => Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(open(archive)?))),
        Kind::TarBz2 | Kind::Bz2 => Box::new(bzip2_rs::DecoderReader::new(BufReader::new(open(archive)?))),
        Kind::TarXz | Kind::Xz => Box::new(BufReader::new(unxz(archive)?)),
        _ => Box::new(BufReader::new(open(archive)?)),
    })
}

/// `photos.txt.gz` -> `photos.txt`.
fn single_name(archive: &Path) -> String {
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => format!("{name}.out"),
    }
}

fn gz_size(archive: &Path) -> u64 {
    use std::io::{Seek, SeekFrom};
    let Ok(mut f) = File::open(archive) else { return 0 };
    let mut tail = [0u8; 4];
    if f.seek(SeekFrom::End(-4)).is_ok() && f.read_exact(&mut tail).is_ok() {
        u32::from_le_bytes(tail) as u64
    } else {
        0
    }
}

fn read_items(archive: &Path, kind: Kind) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(open(archive)?)).map_err(|e| zip_error(archive, e))?;
            for i in 0..zip.len() {
                let entry = zip.by_index_raw(i).map_err(|e| zip_error(archive, e))?;
                let Some(path) = clean(&entry.name().map(|n| n.into_owned()).unwrap_or_default()) else { continue };
                let modified = entry.last_modified().and_then(zip_millis);
                items.push(Item { path, is_dir: entry.is_dir(), size: entry.size(), modified });
            }
        }
        Kind::Tar | Kind::TarGz | Kind::TarBz2 | Kind::TarXz => {
            let mut tar = tar::Archive::new(decoded(archive, kind)?);
            for entry in tar.entries().map_err(|e| invalid(archive, format!("not a valid archive ({e})")))? {
                let entry = entry.map_err(|e| invalid(archive, format!("damaged or cut short ({e})")))?;
                let t = entry.header().entry_type();
                if !(t.is_file() || t.is_dir()) {
                    continue; // links, devices and the like are not shown
                }
                let Some(path) = entry.path().ok().and_then(|p| clean(&p.to_string_lossy())) else { continue };
                items.push(Item { path, is_dir: t.is_dir(), size: entry.header().size().unwrap_or(0), modified: entry.header().mtime().ok().map(|s| s as i64 * 1000) });
            }
        }
        Kind::SevenZ => {
            let reader = sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty()).map_err(|e| seven_error(archive, e))?;
            for f in reader.archive().files.iter() {
                if f.is_anti_item() {
                    continue;
                }
                let Some(path) = clean(f.name()) else { continue };
                let modified = f.has_last_modified_date.then(|| nt_millis(u64::from(f.last_modified_date()))).flatten();
                items.push(Item { path, is_dir: f.is_directory(), size: f.size(), modified });
            }
        }
        Kind::Gz | Kind::Bz2 | Kind::Xz => {
            let size = if kind == Kind::Gz { gz_size(archive) } else { 0 };
            let modified = fs::metadata(archive).ok().and_then(|m| m.modified().ok()).and_then(system_millis);
            items.push(Item { path: single_name(archive), is_dir: false, size, modified });
        }
        Kind::External => items = external_list(archive)?,
    }
    Ok(items)
}

fn seven_error(archive: &Path, e: sevenz_rust2::Error) -> CoreError {
    let text = e.to_string();
    if text.to_lowercase().contains("password") || text.to_lowercase().contains("encrypt") {
        invalid(archive, "this archive is protected by a password, which this app cannot enter yet.")
    } else {
        invalid(archive, format!("could not be read as a 7z file ({text})"))
    }
}

fn zip_millis(t: zip::DateTime) -> Option<i64> {
    use chrono::TimeZone;
    chrono::Local
        .with_ymd_and_hms(t.year() as i32, t.month() as u32, t.day() as u32, t.hour() as u32, t.minute() as u32, t.second() as u32)
        .single()
        .map(|d| d.timestamp_millis())
}

fn nt_millis(file_time: u64) -> Option<i64> {
    // 100 ns ticks since 1601-01-01.
    let secs = (file_time / 10_000_000) as i64 - 11_644_473_600;
    (file_time > 0).then_some(secs * 1000)
}

fn system_millis(t: std::time::SystemTime) -> Option<i64> {
    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as i64)
}

// ---- Listing a folder inside ------------------------------------------------

/// What is directly inside `dir` (`""` for the top). Folders that exist only
/// because a file's path goes through them are included.
pub fn entries_in(items: &[Item], dir: &str, show_hidden: bool) -> Vec<FileEntry> {
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let mut out: BTreeMap<String, FileEntry> = BTreeMap::new();
    for it in items {
        let Some(rest) = it.path.strip_prefix(&prefix) else { continue };
        if rest.is_empty() {
            continue;
        }
        let (name, deeper) = match rest.split_once('/') {
            Some((n, _)) => (n, true),
            None => (rest, false),
        };
        let is_dir = deeper || it.is_dir;
        let entry = out.entry(name.to_string()).or_insert_with(|| FileEntry {
            name: name.to_string(),
            is_dir,
            is_symlink: false,
            is_hidden: name.starts_with('.'),
            size: 0,
            modified: None,
            created: None,
        });
        if is_dir {
            entry.is_dir = true;
            if !deeper {
                entry.modified = it.modified.or(entry.modified);
            }
        } else {
            entry.size = it.size;
            entry.modified = it.modified;
        }
    }
    out.into_values().filter(|e| show_hidden || !e.is_hidden).collect()
}

/// One item inside the archive, for the details pane.
pub fn find(items: &[Item], inner: &str) -> Option<Item> {
    if inner.is_empty() {
        return Some(Item { path: String::new(), is_dir: true, size: 0, modified: None });
    }
    if let Some(it) = items.iter().find(|i| i.path == inner) {
        return Some(it.clone());
    }
    let prefix = format!("{inner}/");
    items.iter().any(|i| i.path.starts_with(&prefix)).then(|| Item { path: inner.to_string(), is_dir: true, size: 0, modified: None })
}

/// Bytes and files under a folder inside the archive.
pub fn size_under(items: &[Item], inner: &str) -> (u64, u64) {
    let prefix = if inner.is_empty() { String::new() } else { format!("{inner}/") };
    items.iter().filter(|i| !i.is_dir && i.path.starts_with(&prefix)).fold((0, 0), |(b, n), i| (b + i.size, n + 1))
}

// ---- Unpacking --------------------------------------------------------------

#[derive(Debug, Default)]
pub struct Out {
    pub files: u64,
    /// Files that were not written because one with that name was already there
    /// (and the choice was to leave it).
    pub existing: u64,
    /// Files that were written over one that was already there.
    pub replaced: u64,
    /// Files written under a new name because theirs was taken.
    pub kept_both: u64,
    /// Entries refused for trying to leave the folder, or that are links.
    pub refused: u64,
}

pub type Report<'a> = &'a mut dyn FnMut(u64, u64, &str);

/// A file that is in the archive and already in the folder it is being unpacked into.
#[derive(Debug, Clone)]
pub struct Clash {
    pub name: String,
    /// The incoming item, as named inside the archive or scratch folder.
    pub src: String,
    pub dst: PathBuf,
    pub src_size: u64,
    pub dst_size: u64,
    pub dst_modified: Option<i64>,
}

/// What to do about one [`Clash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Skip,
    Replace,
    KeepBoth,
}

/// Decides each clash; `None` stops the whole unpacking (the person cancelled).
pub type Decide<'a> = &'a mut dyn FnMut(&Clash) -> Option<Choice>;

/// Which of the wanted items an entry belongs to, and where it goes: an item
/// `a/b` wanted from the archive lands as `b` in the destination, with
/// everything under it keeping its place below.
struct Wanted {
    roots: Vec<String>,
}
impl Wanted {
    fn new(inners: &[String]) -> Self {
        Wanted { roots: inners.iter().map(|s| s.trim_matches('/').to_string()).collect() }
    }
    fn target(&self, path: &str) -> Option<String> {
        for root in &self.roots {
            if root.is_empty() {
                return Some(path.to_string());
            }
            let rest = path.strip_prefix(root.as_str()).filter(|r| r.is_empty() || r.starts_with('/'));
            if rest.is_some() {
                let parent_len = root.rfind('/').map(|i| i + 1).unwrap_or(0);
                return Some(path[parent_len..].to_string());
            }
        }
        None
    }
}

fn copy_to(from: &mut (impl Read + ?Sized), target: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut out = BufWriter::new(File::create(target).map_err(|e| CoreError::io(target, e))?);
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(out);
            let _ = fs::remove_file(target);
            return Err(CoreError::Invalid("Cancelled.".into()));
        }
        let n = from.read(&mut buf).map_err(|e| CoreError::io(target, e))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| CoreError::io(target, e))?;
    }
    out.flush().map_err(|e| CoreError::io(target, e))
}

/// Write one file entry under `into`. When a file is already there, `decide` says what
/// happens: leave it, replace it (through a temporary file, so a cancel keeps the
/// original) or write the new one under a free name. Returns where the file went.
fn put(into: &Path, rel: &str, is_dir: bool, size: u64, from: Option<&mut dyn Read>, cancel: &AtomicBool, out: &mut Out, decide: Decide) -> Result<Option<PathBuf>> {
    let Some(rel) = safe_relative(rel) else {
        out.refused += 1;
        return Ok(None);
    };
    let mut target = into.join(&rel);
    if is_dir {
        fs::create_dir_all(&target).map_err(|e| CoreError::io(&target, e))?;
        return Ok(None);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    }
    let mut replace = false;
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if !meta.is_file() {
            // A folder (or link) is in the way: a file cannot go over it.
            out.existing += 1;
            return Ok(None);
        }
        let clash = Clash {
            name: target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            src: rel.to_string_lossy().into_owned(),
            dst: target.clone(),
            src_size: size,
            dst_size: meta.len(),
            dst_modified: meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
        };
        match decide(&clash) {
            None => return Err(CoreError::Invalid("Cancelled.".into())),
            Some(Choice::Skip) => {
                out.existing += 1;
                return Ok(None);
            }
            Some(Choice::KeepBoth) => {
                target = crate::transfer::engine::unique_name(&target);
                out.kept_both += 1;
            }
            Some(Choice::Replace) => replace = true,
        }
    }
    let Some(from) = from else { return Ok(None) };
    if replace {
        let part = target.with_file_name(format!(".{}.fast-part", target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        copy_to(from, &part, cancel)?;
        if let Err(e) = fs::rename(&part, &target) {
            let _ = fs::remove_file(&part);
            return Err(CoreError::io(&target, e));
        }
        out.replaced += 1;
    } else {
        copy_to(from, &target, cancel)?;
    }
    out.files += 1;
    Ok(Some(target))
}

/// Unpack the wanted items (`[""]` is everything) into `into`, leaving anything already
/// there alone.
pub fn extract_items(archive: &Path, inners: &[String], into: &Path, cancel: &AtomicBool, report: Report) -> Result<Out> {
    extract_items_with(archive, inners, into, cancel, report, &mut |_| Some(Choice::Skip))
}

/// Like [`extract_items`], asking `decide` about every file that is already there.
pub fn extract_items_with(archive: &Path, inners: &[String], into: &Path, cancel: &AtomicBool, report: Report, decide: Decide) -> Result<Out> {
    let kind = kind_of(archive).ok_or_else(|| CoreError::Invalid("This is not a kind of archive that can be opened.".into()))?;
    fs::create_dir_all(into).map_err(|e| CoreError::io(into, e))?;
    let wanted = Wanted::new(inners);
    let mut out = Out::default();
    let cancelled = || CoreError::Invalid("Cancelled.".into());
    match kind {
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(open(archive)?)).map_err(|e| zip_error(archive, e))?;
            // Local headers must agree with the central directory the names were listed from.
            zip.validate_header_consistency().map_err(|e| zip_error(archive, e))?;
            let count = zip.len() as u64;
            for i in 0..zip.len() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let mut entry = zip.by_index_with_options(i, crate::archive::zip_read_options()).map_err(|e| zip_error(archive, e))?;
                let Some(path) = clean(&entry.name().map(|n| n.into_owned()).unwrap_or_default()) else {
                    out.refused += 1;
                    continue;
                };
                let Some(rel) = wanted.target(&path) else { continue };
                report(i as u64, count, &path);
                if entry.is_symlink() {
                    out.refused += 1;
                    continue;
                }
                let is_dir = entry.is_dir();
                #[cfg(unix)]
                let mode = entry.unix_mode();
                let size = entry.size();
                let written = put(into, &rel, is_dir, size, Some(&mut entry), cancel, &mut out, decide)?;
                #[cfg(unix)]
                if let (Some(mode), Some(path)) = (mode, written) {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode & 0o777));
                }
                #[cfg(not(unix))]
                let _ = written;
            }
        }
        Kind::Tar | Kind::TarGz | Kind::TarBz2 | Kind::TarXz => {
            let size = fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
            let mut tar = tar::Archive::new(decoded(archive, kind)?);
            for (n, entry) in tar.entries().map_err(|e| CoreError::io(archive, e))?.enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let mut entry = entry.map_err(|e| invalid(archive, format!("damaged or cut short ({e})")))?;
                let t = entry.header().entry_type();
                let Some(path) = entry.path().ok().and_then(|p| clean(&p.to_string_lossy())) else {
                    out.refused += 1;
                    continue;
                };
                let Some(rel) = wanted.target(&path) else { continue };
                report(n as u64, size.max(n as u64), &path);
                if t.is_dir() {
                    put(into, &rel, true, 0, None, cancel, &mut out, decide)?;
                } else if t.is_file() {
                    let size = entry.header().size().unwrap_or(0);
                    put(into, &rel, false, size, Some(&mut entry), cancel, &mut out, decide)?;
                } else {
                    out.refused += 1;
                }
            }
        }
        Kind::SevenZ => {
            let mut reader = sevenz_rust2::ArchiveReader::open(archive, sevenz_rust2::Password::empty()).map_err(|e| seven_error(archive, e))?;
            let mut failure: Option<CoreError> = None;
            let res = reader.for_each_entries(|entry, data| {
                if cancel.load(Ordering::Relaxed) {
                    failure = Some(cancelled());
                    return Ok(false);
                }
                let Some(path) = clean(entry.name()) else {
                    out.refused += 1;
                    return Ok(true);
                };
                let Some(rel) = wanted.target(&path) else { return Ok(true) };
                report(out.files, 0, &path);
                let r = if entry.is_directory() { put(into, &rel, true, 0, None, cancel, &mut out, decide).map(|_| ()) } else { put(into, &rel, false, entry.size(), Some(data), cancel, &mut out, decide).map(|_| ()) };
                if let Err(e) = r {
                    failure = Some(e);
                    return Ok(false);
                }
                Ok(true)
            });
            if let Some(e) = failure {
                return Err(e);
            }
            res.map_err(|e| seven_error(archive, e))?;
        }
        Kind::Gz | Kind::Bz2 | Kind::Xz => {
            let name = single_name(archive);
            if wanted.target(&name).is_some() {
                report(0, 1, &name);
                let mut data = decoded(archive, kind)?;
                put(into, &name, false, 0, Some(&mut data), cancel, &mut out, decide)?;
            }
        }
        Kind::External => external_extract(archive, inners, into, cancel, &mut out, decide)?,
    }
    Ok(out)
}

// ---- 7-Zip and bsdtar -------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tool {
    SevenZip,
    Bsdtar,
}

pub(crate) fn on_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&exe)).find(|p| p.is_file()))
}

pub(crate) fn find_tool() -> Option<(Tool, PathBuf)> {
    for name in ["7zz", "7z", "7za", "7zr"] {
        if let Some(p) = on_path(name) {
            return Some((Tool::SevenZip, p));
        }
    }
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            for rel in ["7-Zip\\7z.exe", "Programs\\7-Zip\\7z.exe"] {
                let p = PathBuf::from(&base).join(rel);
                if p.is_file() {
                    return Some((Tool::SevenZip, p));
                }
            }
        }
    }
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = PathBuf::from(root).join("System32").join("tar.exe");
        if p.is_file() {
            return Some((Tool::Bsdtar, p));
        }
    }
    on_path("bsdtar").map(|p| (Tool::Bsdtar, p))
}

pub(crate) fn no_tool(archive: &Path) -> CoreError {
    let what = archive.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_else(|| "this kind of".into());
    CoreError::Invalid(format!("Opening {what} files needs 7-Zip. Install it (on Linux the package is p7zip-full or 7zip, on macOS \"brew install sevenzip\") and open this again."))
}

pub(crate) fn command(path: &Path) -> std::process::Command {
    let mut c = std::process::Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.stdin(std::process::Stdio::null());
    c
}

fn external_list(archive: &Path) -> Result<Vec<Item>> {
    let (tool, exe) = find_tool().ok_or_else(|| no_tool(archive))?;
    let mut cmd = command(&exe);
    match tool {
        Tool::SevenZip => cmd.args(["l", "-slt", "-ba", "-sccUTF-8", "-p-", "--"]).arg(archive),
        Tool::Bsdtar => cmd.arg("-tf").arg(archive),
    };
    let output = cmd.output().map_err(|e| CoreError::io(&exe, e))?;
    if !output.status.success() {
        let text = String::from_utf8_lossy(&output.stderr).to_lowercase() + &String::from_utf8_lossy(&output.stdout).to_lowercase();
        return Err(if text.contains("password") || text.contains("encrypted") {
            invalid(archive, "this archive is protected by a password, which this app cannot enter yet.")
        } else {
            invalid(archive, "could not be read. It may be damaged, or of a kind the installed tool does not support.")
        });
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut items = Vec::new();
    match tool {
        Tool::Bsdtar => {
            for line in text.lines() {
                let is_dir = line.ends_with('/');
                if let Some(path) = clean(line) {
                    items.push(Item { path, is_dir, size: 0, modified: None });
                }
            }
        }
        Tool::SevenZip => {
            for block in text.split("\n\n").flat_map(|b| b.split("\r\n\r\n")) {
                let mut path = None;
                let (mut size, mut is_dir, mut modified) = (0u64, false, None);
                for line in block.lines() {
                    let Some((k, v)) = line.split_once(" = ") else { continue };
                    match k.trim() {
                        "Path" => path = Some(v.to_string()),
                        "Size" => size = v.trim().parse().unwrap_or(0),
                        "Folder" => is_dir = v.trim() == "+",
                        "Attributes" if v.trim_start().starts_with('D') => is_dir = true,
                        "Modified" => modified = parse_local_time(v.trim()),
                        _ => {}
                    }
                }
                if let Some(path) = path.and_then(|p| clean(&p)) {
                    items.push(Item { path, is_dir, size, modified });
                }
            }
        }
    }
    Ok(items)
}

/// `2024-05-17 10:22:31` (local time, as 7-Zip prints it).
fn parse_local_time(s: &str) -> Option<i64> {
    use chrono::TimeZone;
    let naive = chrono::NaiveDateTime::parse_from_str(s.split('.').next()?, "%Y-%m-%d %H:%M:%S").ok()?;
    chrono::Local.from_local_datetime(&naive).single().map(|d| d.timestamp_millis())
}

fn external_extract(archive: &Path, inners: &[String], into: &Path, cancel: &AtomicBool, out: &mut Out, decide: Decide) -> Result<()> {
    let (tool, exe) = find_tool().ok_or_else(|| no_tool(archive))?;
    // Unpack into a scratch folder beside the destination, then move each wanted item into place,
    // so that nothing already in the destination is touched.
    let mut id = [0u8; 6];
    let _ = getrandom::fill(&mut id);
    let scratch = into.join(format!(".fast-unpack-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    fs::create_dir_all(&scratch).map_err(|e| CoreError::io(&scratch, e))?;
    let result = (|| -> Result<()> {
        let everything = inners.iter().any(|s| s.trim_matches('/').is_empty());
        let mut cmd = command(&exe);
        match tool {
            Tool::SevenZip => {
                cmd.args(["x", "-y", "-aos", "-p-", "-sccUTF-8"]).arg(format!("-o{}", scratch.display())).arg("--").arg(archive);
                if !everything {
                    cmd.args(inners.iter().map(|s| s.trim_matches('/')));
                }
            }
            Tool::Bsdtar => {
                cmd.arg("-xf").arg(archive).arg("-C").arg(&scratch);
                if !everything {
                    cmd.arg("--").args(inners.iter().map(|s| s.trim_matches('/')));
                }
            }
        }
        let mut child = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| CoreError::io(&exe, e))?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CoreError::Invalid("Cancelled.".into()));
            }
            match child.try_wait().map_err(|e| CoreError::io(&exe, e))? {
                Some(status) => {
                    let mut err = String::new();
                    if let Some(mut s) = child.stderr.take() {
                        let _ = s.read_to_string(&mut err);
                    }
                    if !status.success() {
                        let low = err.to_lowercase();
                        return Err(if low.contains("password") || low.contains("encrypted") {
                            invalid(archive, "this archive is protected by a password, which this app cannot enter yet.")
                        } else {
                            invalid(archive, format!("could not be unpacked. {}", err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim()))
                        });
                    }
                    break;
                }
                None => std::thread::sleep(std::time::Duration::from_millis(40)),
            }
        }
        // Move the results into place.
        let wanted = Wanted::new(inners);
        let mut roots: Vec<String> = Vec::new();
        for r in &wanted.roots {
            roots.push(r.clone());
        }
        for root in roots {
            let (src, dest_name) = if root.is_empty() {
                (scratch.clone(), String::new())
            } else {
                (scratch.join(&root), root.rsplit('/').next().unwrap_or("").to_string())
            };
            if root.is_empty() {
                for e in fs::read_dir(&src).map_err(|e| CoreError::io(&src, e))?.flatten() {
                    merge_into(&e.path(), &into.join(e.file_name()), out, decide)?;
                }
            } else if src.exists() {
                merge_into(&src, &into.join(dest_name), out, decide)?;
            }
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&scratch);
    result
}

/// Move `from` to `to`, merging folders; a file that is already there is up to `decide`.
fn merge_into(from: &Path, to: &Path, out: &mut Out, decide: Decide) -> Result<()> {
    let meta = fs::symlink_metadata(from).map_err(|e| CoreError::io(from, e))?;
    if meta.file_type().is_symlink() {
        out.refused += 1;
    } else if meta.is_dir() {
        if to.exists() && !to.is_dir() {
            out.existing += 1;
            return Ok(());
        }
        fs::create_dir_all(to).map_err(|e| CoreError::io(to, e))?;
        for e in fs::read_dir(from).map_err(|e| CoreError::io(from, e))?.flatten() {
            merge_into(&e.path(), &to.join(e.file_name()), out, decide)?;
        }
    } else {
        let mut target = to.to_path_buf();
        let mut replace = false;
        if let Ok(have) = fs::symlink_metadata(to) {
            if !have.is_file() {
                out.existing += 1;
                return Ok(());
            }
            let clash = Clash {
                name: to.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                src: from.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                dst: to.to_path_buf(),
                src_size: meta.len(),
                dst_size: have.len(),
                dst_modified: have.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64),
            };
            match decide(&clash) {
                None => return Err(CoreError::Invalid("Cancelled.".into())),
                Some(Choice::Skip) => {
                    out.existing += 1;
                    return Ok(());
                }
                Some(Choice::KeepBoth) => {
                    target = crate::transfer::engine::unique_name(to);
                    out.kept_both += 1;
                }
                Some(Choice::Replace) => replace = true,
            }
        }
        if replace {
            let _ = fs::remove_file(&target);
            out.replaced += 1;
        }
        if fs::rename(from, &target).is_err() {
            fs::copy(from, &target).map_err(|e| CoreError::io(&target, e))?;
        }
        out.files += 1;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn tmp(name: &str) -> PathBuf {
        let mut id = [0u8; 4];
        let _ = getrandom::fill(&mut id);
        let p = std::env::temp_dir().join(format!("fast-browse-{name}-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn item(path: &str, is_dir: bool, size: u64) -> Item {
        Item { path: path.into(), is_dir, size, modified: Some(1000) }
    }

    #[test]
    fn locations_round_trip_and_climb() {
        let l = Loc::parse("archive:///home/me/a.tar.gz!/x/y").unwrap();
        assert_eq!(l.archive, PathBuf::from("/home/me/a.tar.gz"));
        assert_eq!(l.inner, "x/y");
        assert_eq!(l.url(), "archive:///home/me/a.tar.gz!/x/y");
        assert_eq!(l.parent().unwrap(), "archive:///home/me/a.tar.gz!/x");
        let top = Loc::parse("archive:///home/me/a.tar.gz!/").unwrap();
        assert_eq!(top.parent().unwrap(), "/home/me");
        assert_eq!(top.name(), "a.tar.gz");
        // A bang in a folder name does not confuse it.
        let odd = Loc::parse("archive:///home/me/wow!/a.zip!/in!side/f").unwrap();
        assert_eq!(odd.archive, PathBuf::from("/home/me/wow!/a.zip"));
        assert_eq!(odd.inner, "in!side/f");
        assert!(Loc::parse("archive:///home/me/notes.txt!/a").is_none());
    }

    #[test]
    fn known_kinds() {
        assert_eq!(kind_of(Path::new("a.TAR.GZ")), Some(Kind::TarGz));
        assert_eq!(kind_of(Path::new("a.tgz")), Some(Kind::TarGz));
        assert_eq!(kind_of(Path::new("a.rar")), Some(Kind::External));
        assert_eq!(kind_of(Path::new("a.7z")), Some(Kind::SevenZ));
        assert_eq!(kind_of(Path::new("a.txt")), None);
        assert_eq!(kind_of(Path::new(".zip")), None);
    }

    #[test]
    fn listing_makes_folders_that_only_appear_in_paths() {
        let items = vec![item("a/b/c.txt", false, 3), item("a/d.txt", false, 4), item("top.txt", false, 5), item(".hidden", false, 1)];
        let top = entries_in(&items, "", false);
        assert_eq!(top.iter().map(|e| (e.name.as_str(), e.is_dir)).collect::<Vec<_>>(), vec![("a", true), ("top.txt", false)]);
        let a = entries_in(&items, "a", false);
        assert_eq!(a.iter().map(|e| (e.name.as_str(), e.is_dir)).collect::<Vec<_>>(), vec![("b", true), ("d.txt", false)]);
        assert_eq!(entries_in(&items, "", true).len(), 3);
        assert_eq!(size_under(&items, "a"), (7, 2));
        assert!(find(&items, "a/b").unwrap().is_dir);
        assert!(find(&items, "nope").is_none());
    }

    #[test]
    fn wanted_items_land_by_their_own_name() {
        let w = Wanted::new(&["a/b".to_string()]);
        assert_eq!(w.target("a/b/c.txt").as_deref(), Some("b/c.txt"));
        assert_eq!(w.target("a/b").as_deref(), Some("b"));
        assert_eq!(w.target("a/bc/c.txt"), None);
        assert_eq!(Wanted::new(&[String::new()]).target("x/y").as_deref(), Some("x/y"));
    }

    fn make_tree(root: &Path) {
        fs::create_dir_all(root.join("docs/deep")).unwrap();
        fs::write(root.join("docs/a.txt"), "alpha").unwrap();
        fs::write(root.join("docs/deep/b.txt"), "bravo bravo").unwrap();
        fs::write(root.join("top.txt"), "top").unwrap();
    }

    fn check_archive(archive: &Path) {
        let items = index(archive).unwrap();
        let top: Vec<_> = entries_in(&items, "", false).into_iter().map(|e| e.name).collect();
        assert!(top.contains(&"docs".to_string()) && top.contains(&"top.txt".to_string()), "{top:?}");
        let docs = entries_in(&items, "docs", false);
        assert!(docs.iter().any(|e| e.name == "a.txt" && e.size == 5));
        let cancel = AtomicBool::new(false);
        let dest = tmp("out");
        let out = extract_items(archive, &["docs".to_string()], &dest, &cancel, &mut |_, _, _| {}).unwrap();
        assert_eq!(fs::read_to_string(dest.join("docs/deep/b.txt")).unwrap(), "bravo bravo");
        assert_eq!(out.files, 2);
        // A second time leaves what is there alone.
        let again = extract_items(archive, &["docs".to_string()], &dest, &cancel, &mut |_, _, _| {}).unwrap();
        assert_eq!((again.files, again.existing), (0, 2));
        // Replace: the files already there are written over.
        fs::write(dest.join("docs/a.txt"), "old").unwrap();
        let mut asked = Vec::new();
        let swapped = extract_items_with(archive, &["docs".to_string()], &dest, &cancel, &mut |_, _, _| {}, &mut |c| {
            asked.push((c.name.clone(), c.dst_size));
            Some(Choice::Replace)
        })
        .unwrap();
        assert_eq!((swapped.files, swapped.replaced, swapped.existing), (2, 2, 0));
        assert_eq!(asked.len(), 2);
        assert!(asked.contains(&("a.txt".to_string(), 3)));
        assert_eq!(fs::read_to_string(dest.join("docs/a.txt")).unwrap(), "alpha");
        // Keep both: the new one gets a free name and the old one is untouched.
        fs::write(dest.join("docs/a.txt"), "mine").unwrap();
        let both = extract_items_with(archive, &["docs".to_string()], &dest, &cancel, &mut |_, _, _| {}, &mut |_| Some(Choice::KeepBoth)).unwrap();
        assert_eq!((both.files, both.kept_both), (2, 2));
        assert_eq!(fs::read_to_string(dest.join("docs/a.txt")).unwrap(), "mine");
        assert_eq!(fs::read_to_string(dest.join("docs/a (2).txt")).unwrap(), "alpha");
        assert_eq!(fs::read_to_string(dest.join("docs/deep/b (2).txt")).unwrap(), "bravo bravo");
        // Cancelling the question stops, and leaves the original as it was.
        assert!(extract_items_with(archive, &["docs".to_string()], &dest, &cancel, &mut |_, _, _| {}, &mut |_| None).is_err());
        assert_eq!(fs::read_to_string(dest.join("docs/a.txt")).unwrap(), "mine");
        assert!(!fs::read_dir(dest.join("docs")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".fast-part")));
        let one = tmp("one");
        extract_items(archive, &["docs/deep/b.txt".to_string()], &one, &cancel, &mut |_, _, _| {}).unwrap();
        assert_eq!(fs::read_to_string(one.join("b.txt")).unwrap(), "bravo bravo");
        for d in [dest, one] {
            let _ = fs::remove_dir_all(d);
        }
    }

    #[test]
    fn zip_and_tar_families_open_like_folders() {
        let root = tmp("src");
        make_tree(&root);
        let work = tmp("work");
        // zip, made by this crate
        let zip = work.join("t.zip");
        crate::archive::compress(&[root.join("docs"), root.join("top.txt")], &zip, &AtomicBool::new(false)).unwrap();
        check_archive(&zip);
        // tar and tar.gz by the tar crate
        for (name, gz) in [("t.tar", false), ("t.tar.gz", true)] {
            let p = work.join(name);
            let f = File::create(&p).unwrap();
            let w: Box<dyn Write> = if gz { Box::new(flate2::write::GzEncoder::new(f, flate2::Compression::fast())) } else { Box::new(f) };
            let mut b = tar::Builder::new(w);
            b.append_dir_all("docs", root.join("docs")).unwrap();
            b.append_path_with_name(root.join("top.txt"), "top.txt").unwrap();
            b.into_inner().unwrap().flush().unwrap();
            check_archive(&p);
        }
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(work);
    }

    /// Archives made by other programs (py7zr and Python's tarfile), kept in tests/fixtures.
    #[test]
    fn seven_z_bzip2_and_xz_made_elsewhere_open() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for name in ["sample.7z", "sample.tar.bz2", "sample.tar.xz"] {
            let work = tmp("fx");
            let copy = work.join(name);
            fs::copy(dir.join(name), &copy).unwrap();
            check_archive(&copy);
            let _ = fs::remove_dir_all(work);
        }
    }

    /// Formats read through the installed 7-Zip (a .wim stands in for .rar, which cannot be made here).
    #[test]
    fn formats_read_by_seven_zip_open_when_it_is_installed() {
        if find_tool().is_none() {
            return;
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let work = tmp("wim");
        let copy = work.join("sample.wim");
        fs::copy(dir.join("sample.wim"), &copy).unwrap();
        if find_tool().map(|(t, _)| t) == Some(Tool::SevenZip) {
            check_archive(&copy);
        }
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn single_gz_shows_one_file() {
        let work = tmp("gz");
        let p = work.join("note.txt.gz");
        let mut enc = flate2::write::GzEncoder::new(File::create(&p).unwrap(), flate2::Compression::fast());
        enc.write_all(b"hello gzip").unwrap();
        enc.finish().unwrap();
        let items = index(&p).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].path.as_str(), items[0].size), ("note.txt", 10));
        let dest = tmp("gzout");
        extract_items(&p, &[String::new()], &dest, &AtomicBool::new(false), &mut |_, _, _| {}).unwrap();
        assert_eq!(fs::read_to_string(dest.join("note.txt")).unwrap(), "hello gzip");
        let _ = fs::remove_dir_all(work);
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn names_that_climb_out_are_never_written() {
        let work = tmp("evil");
        let p = work.join("evil.tar");
        // Hand-write a tar header whose name is "../escape.txt".
        let mut header = [0u8; 512];
        let name = b"../escape.txt";
        header[..name.len()].copy_from_slice(name);
        header[100..107].copy_from_slice(b"0000644");
        header[108..115].copy_from_slice(b"0000000");
        header[116..123].copy_from_slice(b"0000000");
        header[124..135].copy_from_slice(b"00000000004");
        header[136..147].copy_from_slice(b"00000000000");
        header[156] = b'0';
        header[257..262].copy_from_slice(b"ustar");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|b| *b as u32).sum();
        header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(b"evil");
        bytes.resize(1024, 0);
        bytes.extend_from_slice(&[0u8; 1024]);
        fs::write(&p, bytes).unwrap();
        let dest = work.join("dest");
        let out = extract_items(&p, &[String::new()], &dest, &AtomicBool::new(false), &mut |_, _, _| {}).unwrap();
        assert_eq!(out.files, 0);
        assert!(!work.join("escape.txt").exists());
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn rar_without_a_tool_says_what_to_install() {
        // Only meaningful where no tool is installed; otherwise the call may fail for another reason.
        if find_tool().is_none() {
            let work = tmp("rar");
            let p = work.join("a.rar");
            fs::write(&p, b"Rar!").unwrap();
            let err = index(&p).unwrap_err().to_string();
            assert!(err.contains("7-Zip"), "{err}");
            let _ = fs::remove_dir_all(work);
        }
    }
}
