//! Zip files: make one from files and folders, and unpack zip, tar and
//! tar.gz archives. Nothing here shells out, so it behaves the same on every
//! system and does not need 7-Zip or `tar` to be installed.
//!
//! Unpacking never writes outside the folder it was given: names that climb
//! out (`../x`, absolute paths, drive letters) are refused.

use crate::error::{CoreError, Result};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar,
    TarGz,
}

/// Which archive format a file name says it is.
pub fn format_of(path: &Path) -> Option<Format> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        Some(Format::Zip)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(Format::TarGz)
    } else if name.ends_with(".tar") {
        Some(Format::Tar)
    } else {
        None
    }
}

/// The name without its archive extension: `photos.tar.gz` -> `photos`.
pub fn stem_of(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    for ext in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst", ".tgz", ".tbz2", ".tbz", ".txz", ".tar", ".zip", ".7z", ".rar", ".gz", ".bz2", ".xz", ".cab", ".iso", ".wim"] {
        if lower.ends_with(ext) && name.len() > ext.len() {
            return name[..name.len() - ext.len()].to_string();
        }
    }
    name
}

fn cancelled() -> CoreError {
    CoreError::Invalid("Cancelled.".into())
}

fn unique(path: PathBuf) -> PathBuf {
    crate::transfer::engine::unique_name(&path)
}

/// Where `compress` puts the archive for these sources: `name.zip` for one
/// item, `Archive.zip` for several, next to the first one and never over an
/// existing file.
pub fn default_zip_path(sources: &[PathBuf]) -> Option<PathBuf> {
    let first = sources.first()?;
    let dir = first.parent()?;
    let name = if sources.len() == 1 {
        let n = first.file_name()?.to_string_lossy().into_owned();
        // report.txt -> report.zip, but a folder named v1.2 -> v1.2.zip
        match (first.is_dir(), first.file_stem()) {
            (false, Some(stem)) => stem.to_string_lossy().into_owned(),
            _ => n,
        }
    } else {
        "Archive".to_string()
    };
    Some(unique(dir.join(format!("{name}.zip"))))
}

/// A zip stores the wall-clock time of the machine that made it, not UTC, so
/// the time is converted to local time first.
fn zip_time(t: std::time::SystemTime) -> Option<zip::DateTime> {
    use chrono::{Datelike, Local, Timelike};
    let local: chrono::DateTime<Local> = t.into();
    zip::DateTime::from_date_and_time(
        u16::try_from(local.year()).ok()?,
        local.month() as u8,
        local.day() as u8,
        local.hour() as u8,
        local.minute() as u8,
        local.second() as u8,
    )
    .ok()
}

/// Progress: how many of how many, and what is being worked on. For tar
/// archives the units are bytes of the archive read so far.
pub type Report<'a> = &'a mut dyn FnMut(u64, u64, &str);

fn count_files(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path).map(|rd| rd.filter_map(|e| e.ok()).map(|e| count_files(&e.path())).sum()).unwrap_or(0),
        Ok(m) if m.is_file() => 1,
        _ => 0,
    }
}

fn add_tree(
    zip: &mut zip::ZipWriter<BufWriter<File>>,
    path: &Path,
    name_in_zip: &str,
    cancel: &AtomicBool,
    skipped: &mut u32,
    files: &mut u64,
    total: u64,
    report: Report,
) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| CoreError::io(path, e))?;
    let mut options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(meta.len() >= 0xFFFF_FFFF);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        options = options.unix_permissions(meta.permissions().mode() & 0o7777);
    }
    if let Some(z) = meta.modified().ok().and_then(zip_time) {
        options = options.last_modified_time(z);
    }
    if meta.file_type().is_symlink() {
        // Links are left out rather than followed: following one could pull a
        // whole other part of the disk (or loop forever) into the archive.
        *skipped += 1;
    } else if meta.is_dir() {
        zip.add_directory(format!("{name_in_zip}/"), options).map_err(zip_err(path))?;
        let mut children: Vec<_> = fs::read_dir(path).map_err(|e| CoreError::io(path, e))?.filter_map(|e| e.ok()).collect();
        children.sort_by_key(|c| c.file_name());
        for child in children {
            let child_name = format!("{name_in_zip}/{}", child.file_name().to_string_lossy());
            add_tree(zip, &child.path(), &child_name, cancel, skipped, files, total, report)?;
        }
    } else if meta.is_file() {
        zip.start_file(name_in_zip, options).map_err(zip_err(path))?;
        let mut src = BufReader::with_capacity(256 * 1024, File::open(path).map_err(|e| CoreError::io(path, e))?);
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(cancelled());
            }
            let n = src.read(&mut buf).map_err(|e| CoreError::io(path, e))?;
            if n == 0 {
                break;
            }
            zip.write_all(&buf[..n]).map_err(|e| CoreError::io(path, e))?;
        }
        *files += 1;
        report(*files, total, name_in_zip);
    }
    Ok(())
}

fn zip_err(path: &Path) -> impl Fn(zip::result::ZipError) -> CoreError + '_ {
    move |e| match e {
        zip::result::ZipError::Io(io) => CoreError::io(path, io),
        other => CoreError::Invalid(format!("zip: {other}")),
    }
}

/// What `compress` made.
#[derive(Debug)]
pub struct Made {
    pub path: PathBuf,
    pub files: u64,
    /// Symbolic links that were left out.
    pub skipped_links: u32,
}

/// Put `sources` (files and folders, kept by name) into a new zip at `dest`.
/// A cancelled or failed run removes the half-written file.
pub fn compress(sources: &[PathBuf], dest: &Path, cancel: &AtomicBool) -> Result<Made> {
    compress_with(sources, dest, cancel, &mut |_, _, _| {})
}

/// `compress`, reporting progress after every file.
pub fn compress_with(sources: &[PathBuf], dest: &Path, cancel: &AtomicBool, report: Report) -> Result<Made> {
    if sources.is_empty() {
        return Err(CoreError::Invalid("Nothing to compress.".into()));
    }
    for s in sources {
        if dest.starts_with(s) {
            return Err(CoreError::Invalid(format!("\"{}\" cannot be compressed into itself.", s.file_name().unwrap_or_default().to_string_lossy())));
        }
    }
    let file = File::create(dest).map_err(|e| CoreError::io(dest, e))?;
    let mut zip = zip::ZipWriter::new(BufWriter::with_capacity(256 * 1024, file));
    let (mut skipped, mut files) = (0u32, 0u64);
    let total: u64 = sources.iter().map(|s| count_files(s)).sum();
    report(0, total, "");
    let result = (|| -> Result<()> {
        for s in sources {
            let name = s.file_name().ok_or_else(|| CoreError::Invalid("A name is missing.".into()))?.to_string_lossy().into_owned();
            add_tree(&mut zip, s, &name, cancel, &mut skipped, &mut files, total, report)?;
        }
        let mut out = zip.finish().map_err(zip_err(dest))?;
        out.flush().map_err(|e| CoreError::io(dest, e))
    })();
    match result {
        Ok(()) => Ok(Made { path: dest.to_path_buf(), files, skipped_links: skipped }),
        Err(e) => {
            let _ = fs::remove_file(dest);
            Err(e)
        }
    }
}

/// A path inside an archive made safe to join onto the output folder, or
/// `None` when it tries to leave it.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    // Archives made on Windows use backslashes.
    let name = name.replace('\\', "/");
    let mut out = PathBuf::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') && p.len() >= 2 && p.as_bytes()[1] == b':' => return None,
            p => out.push(p),
        }
    }
    if out.components().any(|c| !matches!(c, Component::Normal(_))) || out.as_os_str().is_empty() {
        return None;
    }
    Some(out)
}

/// What `extract` made.
#[derive(Debug)]
pub struct Unpacked {
    /// The folder the files went into.
    pub folder: PathBuf,
    pub files: u64,
    /// Entries that were refused for trying to leave the folder.
    pub refused: u32,
}

/// Unpack `archive` into `into`, which is created when it does not exist.
/// Existing files are never replaced: they keep their place and the
/// archive's copy is skipped, which is the safe default for "extract here".
pub fn extract(archive: &Path, into: &Path, cancel: &AtomicBool) -> Result<Unpacked> {
    extract_with(archive, into, cancel, &mut |_, _, _| {})
}

/// Counts the bytes read from the file under a tar so progress can be shown.
struct CountingRead<R> {
    inner: R,
    count: std::sync::Arc<std::sync::atomic::AtomicU64>,
}
impl<R: Read> Read for CountingRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// `extract`, reporting progress: entries for zip, bytes of the archive for tar.
pub fn extract_with(archive: &Path, into: &Path, cancel: &AtomicBool, report: Report) -> Result<Unpacked> {
    let Some(format) = format_of(archive) else {
        // 7z, rar, bzip2, xz and the rest go through the reader that also browses them.
        if crate::browse::kind_of(archive).is_none() {
            return Err(CoreError::Invalid("This kind of archive is not supported.".into()));
        }
        let out = crate::browse::extract_items(archive, &[String::new()], into, cancel, report)?;
        return Ok(Unpacked { folder: into.to_path_buf(), files: out.files, refused: out.refused as u32 });
    };
    fs::create_dir_all(into).map_err(|e| CoreError::io(into, e))?;
    let mut files = 0u64;
    let mut refused = 0u32;
    match format {
        Format::Zip => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive).map_err(|e| CoreError::io(archive, e))?)).map_err(zip_err(archive))?;
            let count = zip.len() as u64;
            report(0, count, "");
            for i in 0..zip.len() {
                if cancel.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let mut entry = zip.by_index(i).map_err(zip_err(archive))?;
                report(i as u64, count, entry.name());
                let Some(rel) = safe_relative(entry.name()) else {
                    refused += 1;
                    continue;
                };
                let target = into.join(&rel);
                if entry.is_dir() {
                    fs::create_dir_all(&target).map_err(|e| CoreError::io(&target, e))?;
                    continue;
                }
                if entry.is_symlink() {
                    refused += 1;
                    continue;
                }
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
                }
                if target.exists() {
                    continue;
                }
                let mut out = BufWriter::new(File::create(&target).map_err(|e| CoreError::io(&target, e))?);
                copy_cancellable(&mut entry, &mut out, cancel, &target)?;
                out.flush().map_err(|e| CoreError::io(&target, e))?;
                drop(out);
                #[cfg(unix)]
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    // Keep the executable bit and the like, but never setuid/setgid.
                    let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777));
                }
                files += 1;
            }
        }
        Format::Tar | Format::TarGz => {
            let size = fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
            let read = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
            let raw = BufReader::new(CountingRead { inner: File::open(archive).map_err(|e| CoreError::io(archive, e))?, count: read.clone() });
            report(0, size, "");
            let reader: Box<dyn Read> = if format == Format::TarGz { Box::new(flate2::read::GzDecoder::new(raw)) } else { Box::new(raw) };
            let mut tar = tar::Archive::new(reader);
            tar.set_preserve_permissions(false);
            tar.set_unpack_xattrs(false);
            for entry in tar.entries().map_err(|e| CoreError::io(archive, e))? {
                if cancel.load(Ordering::Relaxed) {
                    return Err(cancelled());
                }
                let mut entry = entry.map_err(|e| CoreError::io(archive, e))?;
                let kind = entry.header().entry_type();
                let path = entry.path().map_err(|e| CoreError::io(archive, e))?.to_string_lossy().into_owned();
                report(read.load(Ordering::Relaxed), size, &path);
                let Some(rel) = safe_relative(&path) else {
                    refused += 1;
                    continue;
                };
                let target = into.join(&rel);
                if kind.is_dir() {
                    fs::create_dir_all(&target).map_err(|e| CoreError::io(&target, e))?;
                } else if kind.is_file() {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
                    }
                    if target.exists() {
                        continue;
                    }
                    let mut out = BufWriter::new(File::create(&target).map_err(|e| CoreError::io(&target, e))?);
                    copy_cancellable(&mut entry, &mut out, cancel, &target)?;
                    out.flush().map_err(|e| CoreError::io(&target, e))?;
                    drop(out);
                    #[cfg(unix)]
                    if let Ok(mode) = entry.header().mode() {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777));
                    }
                    files += 1;
                } else {
                    // Links, devices and the like are not unpacked.
                    refused += 1;
                }
            }
        }
    }
    Ok(Unpacked { folder: into.to_path_buf(), files, refused })
}

fn copy_cancellable(from: &mut impl Read, to: &mut impl Write, cancel: &AtomicBool, target: &Path) -> Result<()> {
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let n = from.read(&mut buf).map_err(|e| CoreError::io(target, e))?;
        if n == 0 {
            return Ok(());
        }
        to.write_all(&buf[..n]).map_err(|e| CoreError::io(target, e))?;
    }
}

/// Where "Extract to folder" puts things: a new folder named after the
/// archive, next to it.
pub fn default_folder(archive: &Path) -> Option<PathBuf> {
    let dir = archive.parent()?;
    let stem = stem_of(archive);
    if stem.is_empty() {
        return None;
    }
    Some(unique(dir.join(stem)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }

    fn tree(root: &Path) {
        fs::create_dir_all(root.join("docs/deep")).unwrap();
        fs::write(root.join("a.txt"), b"alpha").unwrap();
        fs::write(root.join("docs/b.txt"), b"bravo").unwrap();
        fs::write(root.join("docs/deep/c.bin"), vec![7u8; 600_000]).unwrap();
        fs::create_dir(root.join("empty")).unwrap();
    }

    #[test]
    fn recognises_archive_names() {
        assert_eq!(format_of(Path::new("x.ZIP")), Some(Format::Zip));
        assert_eq!(format_of(Path::new("x.tar.gz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.tgz")), Some(Format::TarGz));
        assert_eq!(format_of(Path::new("x.tar")), Some(Format::Tar));
        assert_eq!(format_of(Path::new("x.txt")), None);
        assert_eq!(stem_of(Path::new("/a/photos.tar.gz")), "photos");
        assert_eq!(stem_of(Path::new("/a/v1.2.zip")), "v1.2");
    }

    #[test]
    fn zip_round_trip_keeps_files_folders_and_empty_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("project");
        tree(&src);
        let zip = tmp.path().join("project.zip");
        let made = compress(&[src.clone()], &zip, &no()).unwrap();
        assert_eq!(made.files, 3);
        assert!(fs::metadata(&zip).unwrap().len() < 100_000, "the repeated bytes compress");

        let out = tmp.path().join("out");
        let done = extract(&zip, &out, &no()).unwrap();
        assert_eq!(done.files, 3);
        assert_eq!(fs::read(out.join("project/a.txt")).unwrap(), b"alpha");
        assert_eq!(fs::read(out.join("project/docs/deep/c.bin")).unwrap().len(), 600_000);
        assert!(out.join("project/empty").is_dir());
    }

    #[test]
    fn several_sources_go_in_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("one.txt"), b"1").unwrap();
        fs::write(tmp.path().join("two.txt"), b"2").unwrap();
        let sources = vec![tmp.path().join("one.txt"), tmp.path().join("two.txt")];
        let dest = default_zip_path(&sources).unwrap();
        assert_eq!(dest.file_name().unwrap(), "Archive.zip");
        compress(&sources, &dest, &no()).unwrap();
        let out = tmp.path().join("out");
        extract(&dest, &out, &no()).unwrap();
        assert_eq!(fs::read(out.join("two.txt")).unwrap(), b"2");
    }

    #[test]
    fn default_names_do_not_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("report.txt");
        fs::write(&file, b"x").unwrap();
        assert_eq!(default_zip_path(std::slice::from_ref(&file)).unwrap().file_name().unwrap(), "report.zip");
        fs::write(tmp.path().join("report.zip"), b"taken").unwrap();
        assert_eq!(default_zip_path(std::slice::from_ref(&file)).unwrap().file_name().unwrap(), "report (2).zip");
        let zip = tmp.path().join("pack.zip");
        assert_eq!(default_folder(&zip).unwrap().file_name().unwrap(), "pack");
        fs::create_dir(tmp.path().join("pack")).unwrap();
        assert_eq!(default_folder(&zip).unwrap().file_name().unwrap(), "pack (2)");
    }

    #[test]
    fn cannot_compress_a_folder_into_itself() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("d");
        fs::create_dir(&dir).unwrap();
        let err = compress(&[dir.clone()], &dir.join("d.zip"), &no()).unwrap_err();
        assert!(err.to_string().contains("itself"));
    }

    #[test]
    fn cancelling_leaves_no_half_written_file() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("project");
        tree(&src);
        let zip = tmp.path().join("project.zip");
        let stop = AtomicBool::new(true);
        assert!(compress(&[src], &zip, &stop).is_err());
        assert!(!zip.exists());
    }

    #[test]
    fn progress_is_reported_and_ends_complete() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("project");
        tree(&src);
        let zip = tmp.path().join("project.zip");
        let mut seen = Vec::new();
        compress_with(&[src], &zip, &no(), &mut |done, total, name| seen.push((done, total, name.to_string()))).unwrap();
        assert_eq!(seen.first().map(|s| (s.0, s.1)), Some((0, 3)));
        assert_eq!(seen.last().map(|s| (s.0, s.1)), Some((3, 3)));
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "never goes backwards");

        let mut entries = 0;
        extract_with(&zip, &tmp.path().join("out"), &no(), &mut |_, total, _| entries = total).unwrap();
        assert!(entries >= 3);
    }

    #[test]
    fn zip_times_are_local_wall_clock() {
        // A file changed "now" must not come out hours away from now.
        let z = zip_time(std::time::SystemTime::now()).unwrap();
        let local = chrono::Local::now();
        use chrono::Timelike;
        assert_eq!(z.hour() as u32, local.hour());
    }

    #[test]
    fn paths_that_climb_out_are_refused() {
        assert_eq!(safe_relative("a/b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_relative("./a//b"), Some(PathBuf::from("a/b")));
        assert_eq!(safe_relative("a\\b.txt"), Some(PathBuf::from("a/b.txt")));
        assert_eq!(safe_relative("../evil"), None);
        assert_eq!(safe_relative("a/../../evil"), None);
        assert_eq!(safe_relative("/etc/passwd"), Some(PathBuf::from("etc/passwd")));
        assert_eq!(safe_relative("C:\\Windows\\x"), None);
        assert_eq!(safe_relative(""), None);
        assert_eq!(safe_relative("/"), None);
    }

    #[test]
    fn a_zip_that_tries_to_escape_is_contained() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("evil.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&path).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("../escaped.txt", o).unwrap();
            z.write_all(b"bad").unwrap();
            z.start_file("fine.txt", o).unwrap();
            z.write_all(b"ok").unwrap();
            z.finish().unwrap();
        }
        let out = tmp.path().join("out");
        let done = extract(&path, &out, &no()).unwrap();
        assert_eq!(done.refused, 1);
        assert_eq!(done.files, 1);
        assert!(!tmp.path().join("escaped.txt").exists());
        assert_eq!(fs::read(out.join("fine.txt")).unwrap(), b"ok");
    }

    #[test]
    fn existing_files_are_not_replaced() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("p");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("a.txt"), b"from the archive").unwrap();
        let zip = tmp.path().join("p.zip");
        compress(&[src], &zip, &no()).unwrap();
        let out = tmp.path().join("out");
        fs::create_dir_all(out.join("p")).unwrap();
        fs::write(out.join("p/a.txt"), b"mine").unwrap();
        extract(&zip, &out, &no()).unwrap();
        assert_eq!(fs::read(out.join("p/a.txt")).unwrap(), b"mine");
    }

    fn make_tar(path: &Path, gz: bool, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let writer: Box<dyn Write> = if gz { Box::new(flate2::write::GzEncoder::new(file, flate2::Compression::fast())) } else { Box::new(file) };
        let mut b = tar::Builder::new(writer);
        for (name, data) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, name, *data).unwrap();
        }
        b.into_inner().unwrap().flush().unwrap();
    }

    #[test]
    fn unpacks_tar_and_tar_gz() {
        let tmp = tempfile::tempdir().unwrap();
        for (file, gz) in [("a.tar", false), ("b.tar.gz", true)] {
            let archive = tmp.path().join(file);
            make_tar(&archive, gz, &[("dir/one.txt", b"1"), ("two.txt", b"22")]);
            let out = tmp.path().join(format!("out-{file}"));
            let done = extract(&archive, &out, &no()).unwrap();
            assert_eq!(done.files, 2, "{file}");
            assert_eq!(fs::read(out.join("dir/one.txt")).unwrap(), b"1");
            assert_eq!(fs::read(out.join("two.txt")).unwrap(), b"22");
        }
    }

    #[test]
    fn unsupported_and_corrupt_archives_fail_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let rar = tmp.path().join("x.rar");
        fs::write(&rar, b"x").unwrap();
        // A rar goes through 7-Zip when it is installed, so it may say either thing, but never succeed.
        let msg = extract(&rar, &tmp.path().join("o1"), &no()).unwrap_err().to_string();
        assert!(msg.contains("7-Zip") || msg.contains("could not be"), "{msg}");
        let odd = tmp.path().join("x.xyz");
        fs::write(&odd, b"x").unwrap();
        assert!(extract(&odd, &tmp.path().join("o3"), &no()).unwrap_err().to_string().contains("not supported"));
        let bad = tmp.path().join("bad.zip");
        fs::write(&bad, b"this is not a zip file").unwrap();
        assert!(extract(&bad, &tmp.path().join("o2"), &no()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_left_out_and_permissions_survive() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("p");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("run.sh"), b"#!/bin/sh").unwrap();
        fs::set_permissions(src.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        symlink("/etc", src.join("link")).unwrap();
        let zip = tmp.path().join("p.zip");
        let made = compress(&[src], &zip, &no()).unwrap();
        assert_eq!(made.skipped_links, 1);
        let out = tmp.path().join("out");
        extract(&zip, &out, &no()).unwrap();
        assert!(!out.join("p/link").exists());
        assert_eq!(fs::metadata(out.join("p/run.sh")).unwrap().permissions().mode() & 0o777, 0o755);
    }
}
