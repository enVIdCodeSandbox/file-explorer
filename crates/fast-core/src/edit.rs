//! Changing what is inside an archive: add files, move and rename items, make
//! folders, delete.
//!
//! Zip and every kind of tar (plain, gzip, bzip2, xz) are rewritten by this
//! program (a new file is written beside the archive and swapped in only when
//! it is complete, so a failure or a cancel leaves the original untouched). 7z
//! goes through the installed 7-Zip. The rest are unpacked to a scratch folder,
//! changed there and packed again by a tool that can write that format: wim by
//! 7-Zip; iso, cpio and zstd, lzip and compress tars by bsdtar (built into
//! Windows 10 and later and macOS); rar by WinRAR's `rar` when it is installed.
//! Formats nothing can write here (cab, arj, lzh, rpm, deb and single
//! compressed files) say so.
//!
//! Names that already exist in the target folder are never overwritten: the
//! new item gets "name (2)".

use crate::archive::{add_tree, cancelled, count_files, zip_err};
use crate::browse::{self, clean, command, decoded, find_tool, invalid, no_tool, Kind, Tool};
use crate::error::{CoreError, Result};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub type Report<'a> = &'a mut dyn FnMut(u64, u64, &str);

/// How an archive that this program cannot write itself is packed again.
#[derive(Clone, Debug)]
enum Repack {
    /// 7-Zip with `-t<type>`.
    SevenZip(&'static str),
    /// bsdtar with these options placed before `-f`.
    Bsdtar(&'static [&'static str]),
    /// WinRAR's command line.
    Rar,
}

fn repack_for(archive: &Path) -> Option<Repack> {
    let name = archive.file_name()?.to_string_lossy().to_lowercase();
    let ends = |e: &[&str]| e.iter().any(|x| name.ends_with(x));
    if ends(&[".wim"]) {
        Some(Repack::SevenZip("wim"))
    } else if ends(&[".iso"]) {
        Some(Repack::Bsdtar(&["--format", "iso9660"]))
    } else if ends(&[".cpio"]) {
        Some(Repack::Bsdtar(&["--format", "cpio"]))
    } else if ends(&[".tar.zst", ".tzst"]) {
        Some(Repack::Bsdtar(&["--zstd"]))
    } else if ends(&[".tar.lz"]) {
        Some(Repack::Bsdtar(&["--lzip"]))
    } else if ends(&[".tar.z"]) {
        Some(Repack::Bsdtar(&["-Z"]))
    } else if ends(&[".rar"]) {
        Some(Repack::Rar)
    } else {
        None
    }
}

fn find_bsdtar() -> Option<PathBuf> {
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        let p = PathBuf::from(root).join("System32").join("tar.exe");
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(p) = browse::on_path("bsdtar") {
        return Some(p);
    }
    // macOS (and some Linux systems) ship bsdtar under the name tar.
    let tar = browse::on_path("tar")?;
    let out = command(&tar).arg("--version").output().ok()?;
    String::from_utf8_lossy(&out.stdout).to_lowercase().contains("bsdtar").then_some(tar)
}

fn find_rar() -> Option<PathBuf> {
    if let Some(p) = browse::on_path("rar") {
        return Some(p);
    }
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(var) {
            let p = PathBuf::from(base).join("WinRAR").join("Rar.exe");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn seven_zip() -> Option<PathBuf> {
    match find_tool() {
        Some((Tool::SevenZip, exe)) => Some(exe),
        _ => None,
    }
}

/// Whether this archive can be changed, and why not.
pub fn check_editable(archive: &Path) -> Result<Kind> {
    let kind = browse::kind_of(archive).ok_or_else(|| CoreError::Invalid("This is not a kind of archive that can be opened.".into()))?;
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let needs = |tool: &str| Err(CoreError::Invalid(format!("{name} cannot be changed without {tool}. Install it and try again.")));
    match kind {
        Kind::Zip | Kind::Tar | Kind::TarGz | Kind::TarBz2 | Kind::TarXz => Ok(kind),
        Kind::SevenZ => match seven_zip() {
            Some(_) => Ok(kind),
            None => needs("7-Zip"),
        },
        Kind::External => match repack_for(archive) {
            Some(Repack::SevenZip(_)) if seven_zip().is_none() => needs("7-Zip"),
            Some(Repack::Bsdtar(_)) if find_bsdtar().is_none() => needs("bsdtar (built into Windows 10 and later and macOS; on Linux the package is libarchive-tools or bsdtar)"),
            Some(Repack::Rar) if find_rar().is_none() => needs("WinRAR's command line (rar): RAR files can only be written by WinRAR"),
            Some(_) => Ok(kind),
            None => Err(CoreError::Invalid(format!("{name} cannot be changed: no tool can write this kind of file here, only read it. Extract it to change what is in it."))),
        },
        _ => Err(CoreError::Invalid(format!("{name} cannot be changed, only read. Extract it to change what is in it."))),
    }
}

#[derive(Default, Debug)]
struct Plan {
    /// (old path, new path) for an item and everything under it.
    renames: Vec<(String, String)>,
    /// Items (and everything under them) to leave out.
    drops: Vec<String>,
    /// Files and folders on disk, and the path each gets inside the archive.
    add: Vec<(PathBuf, String)>,
    /// Empty folders to make.
    dirs: Vec<String>,
}

enum Action {
    Keep,
    Drop,
    Rename(String),
}

fn under(path: &str, root: &str) -> bool {
    path == root || path.strip_prefix(root).is_some_and(|r| r.starts_with('/'))
}

impl Plan {
    fn action(&self, path: &str) -> Action {
        if self.drops.iter().any(|d| under(path, d)) {
            return Action::Drop;
        }
        for (from, to) in &self.renames {
            if under(path, from) {
                return Action::Rename(format!("{to}{}", &path[from.len()..]));
            }
        }
        Action::Keep
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

fn unique(taken: &HashSet<String>, dir: &str, name: &str) -> String {
    if !taken.contains(&join(dir, name)) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2u32..).map(|n| format!("{stem} ({n}){ext}")).find(|c| !taken.contains(&join(dir, c))).unwrap()
}

/// Every path in the archive, folders included, and the folders that only appear in paths.
fn taken_names(archive: &Path) -> Result<HashSet<String>> {
    let items = browse::index(archive)?;
    let mut set = HashSet::new();
    for it in items.iter() {
        let mut acc = String::new();
        for part in it.path.split('/') {
            acc = join(&acc, part);
            set.insert(acc.clone());
        }
    }
    Ok(set)
}

fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(p, _)| p).unwrap_or("")
}

fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or("")
}

fn check_dest_folder(archive: &Path, dest: &str) -> Result<()> {
    if dest.is_empty() {
        return Ok(());
    }
    let items = browse::index(archive)?;
    match browse::find(&items, dest) {
        Some(i) if i.is_dir => Ok(()),
        _ => Err(CoreError::Invalid(format!("There is no folder \"{dest}\" in this archive."))),
    }
}

// ---- Operations -------------------------------------------------------------

/// Put files and folders from disk into `dest_inner`. Returns how many files went in.
pub fn add(archive: &Path, dest_inner: &str, sources: &[PathBuf], cancel: &AtomicBool, report: Report) -> Result<u64> {
    check_editable(archive)?;
    let dest_inner = dest_inner.trim_matches('/');
    check_dest_folder(archive, dest_inner)?;
    let mut taken = taken_names(archive)?;
    let mut plan = Plan::default();
    for s in sources {
        let name = s.file_name().ok_or_else(|| CoreError::Invalid("A name is missing.".into()))?.to_string_lossy().into_owned();
        if archive.starts_with(s) {
            return Err(CoreError::Invalid(format!("\"{name}\" cannot be added to an archive that is inside it.")));
        }
        let name = unique(&taken, dest_inner, &name);
        let path = join(dest_inner, &name);
        taken.insert(path.clone());
        plan.add.push((s.clone(), path));
    }
    rewrite(archive, &plan, cancel, report)
}

/// Move items to another folder of the same archive. Returns the new paths.
pub fn move_within(archive: &Path, inners: &[String], dest_inner: &str, cancel: &AtomicBool, report: Report) -> Result<Vec<String>> {
    check_editable(archive)?;
    let dest_inner = dest_inner.trim_matches('/');
    check_dest_folder(archive, dest_inner)?;
    let mut taken = taken_names(archive)?;
    let mut plan = Plan::default();
    let mut moved = Vec::new();
    for from in inners.iter().map(|s| s.trim_matches('/')) {
        if from.is_empty() || parent_of(from) == dest_inner {
            continue; // already there
        }
        if under(dest_inner, from) {
            return Err(CoreError::Invalid(format!("\"{}\" cannot be moved into itself.", leaf(from))));
        }
        let name = unique(&taken, dest_inner, leaf(from));
        let to = join(dest_inner, &name);
        taken.insert(to.clone());
        plan.renames.push((from.to_string(), to.clone()));
        moved.push(to);
    }
    if plan.renames.is_empty() {
        return Ok(moved);
    }
    rewrite(archive, &plan, cancel, report)?;
    Ok(moved)
}

/// Give one item a new name. Returns its new path.
pub fn rename(archive: &Path, inner: &str, new_name: &str, cancel: &AtomicBool) -> Result<String> {
    check_editable(archive)?;
    let inner = inner.trim_matches('/');
    if new_name.is_empty() || new_name.contains(['/', '\\']) || new_name == "." || new_name == ".." {
        return Err(CoreError::Invalid(format!("\"{new_name}\" is not a valid name.")));
    }
    let to = join(parent_of(inner), new_name);
    if to == inner {
        return Ok(to);
    }
    if taken_names(archive)?.contains(&to) {
        return Err(CoreError::Invalid(format!("\"{new_name}\" already exists here.")));
    }
    let plan = Plan { renames: vec![(inner.to_string(), to.clone())], ..Plan::default() };
    rewrite(archive, &plan, cancel, &mut |_, _, _| {})?;
    Ok(to)
}

pub fn delete(archive: &Path, inners: &[String], cancel: &AtomicBool) -> Result<()> {
    check_editable(archive)?;
    let drops: Vec<String> = inners.iter().map(|s| s.trim_matches('/').to_string()).filter(|s| !s.is_empty()).collect();
    if drops.is_empty() {
        return Ok(());
    }
    rewrite(archive, &Plan { drops, ..Plan::default() }, cancel, &mut |_, _, _| {})?;
    Ok(())
}

/// A new empty folder, or an empty file, in `dest_inner`. Returns its path.
pub fn create(archive: &Path, dest_inner: &str, name: &str, folder: bool, cancel: &AtomicBool) -> Result<String> {
    check_editable(archive)?;
    let dest_inner = dest_inner.trim_matches('/');
    if name.is_empty() || name.contains(['/', '\\']) {
        return Err(CoreError::Invalid(format!("\"{name}\" is not a valid name.")));
    }
    check_dest_folder(archive, dest_inner)?;
    let taken = taken_names(archive)?;
    let path = join(dest_inner, &unique(&taken, dest_inner, name));
    let mut plan = Plan::default();
    if folder {
        plan.dirs.push(path.clone());
    } else {
        let scratch = scratch_dir()?;
        let file = scratch.join(leaf(&path));
        File::create(&file).map_err(|e| CoreError::io(&file, e))?;
        plan.add.push((file, path.clone()));
        let r = rewrite(archive, &plan, cancel, &mut |_, _, _| {});
        let _ = fs::remove_dir_all(&scratch);
        r?;
        return Ok(path);
    }
    rewrite(archive, &plan, cancel, &mut |_, _, _| {})?;
    Ok(path)
}

fn scratch_dir() -> Result<PathBuf> {
    let mut id = [0u8; 6];
    let _ = getrandom::fill(&mut id);
    let dir = std::env::temp_dir().join(format!("fast-edit-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    Ok(dir)
}

// ---- Rewriting --------------------------------------------------------------

fn temp_beside(archive: &Path) -> PathBuf {
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut id = [0u8; 4];
    let _ = getrandom::fill(&mut id);
    archive.with_file_name(format!(".{name}.{}.fast-edit", id.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

fn rewrite(archive: &Path, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let kind = check_editable(archive)?;
    let result = match kind {
        Kind::SevenZ => seven_rewrite(archive, plan, cancel, report),
        Kind::External => external_rewrite(archive, plan, cancel, report),
        _ => {
            let tmp = temp_beside(archive);
            let r = match kind {
                Kind::Zip => zip_rewrite(archive, &tmp, plan, cancel, report),
                _ => tar_rewrite(archive, &tmp, kind, plan, cancel, report),
            };
            match r {
                Ok(n) => fs::rename(&tmp, archive).map(|_| n).map_err(|e| {
                    let _ = fs::remove_file(&tmp);
                    CoreError::io(archive, e)
                }),
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    Err(e)
                }
            }
        }
    };
    browse::clear_cache();
    result
}

fn zip_rewrite(archive: &Path, tmp: &Path, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(browse::open(archive)?)).map_err(zip_err(archive))?;
    let out = File::create(tmp).map_err(|e| CoreError::io(tmp, e))?;
    let mut w = zip::ZipWriter::new(BufWriter::with_capacity(256 * 1024, out));
    let total = zip.len() as u64 + plan.add.iter().map(|(p, _)| count_files(p)).sum::<u64>();
    for i in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let file = zip.by_index_raw(i).map_err(zip_err(archive))?;
        let raw = file.name().to_string();
        let is_dir = file.is_dir();
        let Some(path) = clean(&raw) else {
            w.raw_copy_file(file).map_err(zip_err(archive))?; // not ours to judge: kept as it was
            continue;
        };
        match plan.action(&path) {
            Action::Drop => {}
            Action::Keep => w.raw_copy_file(file).map_err(zip_err(archive))?,
            Action::Rename(to) => w.raw_copy_file_rename(file, if is_dir { format!("{to}/") } else { to }).map_err(zip_err(archive))?,
        }
        report(i as u64, total, &path);
    }
    let (mut skipped, mut files) = (0u32, 0u64);
    for (src, name) in &plan.add {
        add_tree(&mut w, src, name, cancel, &mut skipped, &mut files, total, report)?;
    }
    for d in &plan.dirs {
        w.add_directory(format!("{d}/"), zip::write::SimpleFileOptions::default()).map_err(zip_err(archive))?;
    }
    let mut out = w.finish().map_err(zip_err(archive))?;
    out.flush().map_err(|e| CoreError::io(tmp, e))?;
    Ok(files)
}

/// Where a rewritten tar goes: plain, or through the compressor its name asks for.
enum Sink {
    Plain(BufWriter<File>),
    Gz(flate2::write::GzEncoder<BufWriter<File>>),
    Bz(bzip2::write::BzEncoder<BufWriter<File>>),
    Xz(Box<lzma_rust2::XzWriter<BufWriter<File>>>),
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Sink::Plain(w) => w.write(buf),
            Sink::Gz(w) => w.write(buf),
            Sink::Bz(w) => w.write(buf),
            Sink::Xz(w) => w.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Sink::Plain(w) => w.flush(),
            Sink::Gz(w) => w.flush(),
            Sink::Bz(w) => w.flush(),
            Sink::Xz(w) => w.flush(),
        }
    }
}

impl Sink {
    /// Write whatever the compressor still holds and flush the file.
    fn finish(self) -> std::io::Result<()> {
        match self {
            Sink::Plain(mut w) => w.flush(),
            Sink::Gz(w) => w.finish()?.flush(),
            Sink::Bz(w) => w.finish()?.flush(),
            Sink::Xz(w) => (*w).finish()?.flush(),
        }
    }
}

fn tar_rewrite(archive: &Path, tmp: &Path, kind: Kind, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let file = BufWriter::with_capacity(256 * 1024, File::create(tmp).map_err(|e| CoreError::io(tmp, e))?);
    let sink = match kind {
        Kind::TarGz => Sink::Gz(flate2::write::GzEncoder::new(file, flate2::Compression::default())),
        Kind::TarBz2 => Sink::Bz(bzip2::write::BzEncoder::new(file, bzip2::Compression::default())),
        Kind::TarXz => Sink::Xz(Box::new(
            lzma_rust2::XzWriter::new(file, lzma_rust2::XzOptions::with_preset(6)).map_err(|e| invalid(archive, format!("could not be rewritten ({e})")))?,
        )),
        _ => Sink::Plain(file),
    };
    let mut b = tar::Builder::new(sink);
    b.follow_symlinks(false);
    let mut input = tar::Archive::new(decoded(archive, kind)?);
    let total = plan.add.iter().map(|(p, _)| count_files(p)).sum::<u64>();
    let mut n = 0u64;
    let tar_err = |e: std::io::Error| invalid(archive, format!("could not be rewritten ({e})"));
    for entry in input.entries().map_err(tar_err)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let mut entry = entry.map_err(tar_err)?;
        let raw = entry.path().map_err(tar_err)?.to_string_lossy().into_owned();
        let Some(path) = clean(&raw) else { continue };
        let mut header = entry.header().clone();
        let target = match plan.action(&path) {
            Action::Drop => continue,
            Action::Keep => raw,
            Action::Rename(to) => to,
        };
        b.append_data(&mut header, target, &mut entry).map_err(tar_err)?;
        n += 1;
        report(n, n.max(total), &path);
    }
    let mut files = 0u64;
    for (src, name) in &plan.add {
        let meta = fs::symlink_metadata(src).map_err(|e| CoreError::io(src, e))?;
        if meta.file_type().is_symlink() {
            continue;
        } else if meta.is_dir() {
            b.append_dir_all(name, src).map_err(tar_err)?;
            files += count_files(src);
        } else {
            b.append_path_with_name(src, name).map_err(tar_err)?;
            files += 1;
        }
        report(files, total, name);
    }
    for d in &plan.dirs {
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Directory);
        h.set_mode(0o755);
        h.set_size(0);
        h.set_mtime(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
        b.append_data(&mut h, format!("{d}/"), std::io::empty()).map_err(tar_err)?;
    }
    let sink = b.into_inner().map_err(tar_err)?;
    sink.finish().map_err(tar_err)?;
    Ok(files)
}

// ---- 7z through 7-Zip -------------------------------------------------------

fn seven_run(exe: &Path, args: Vec<std::ffi::OsString>, cwd: Option<&Path>, cancel: &AtomicBool, archive: &Path) -> Result<()> {
    let mut cmd = command(exe);
    cmd.args(args);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let mut child = cmd.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::piped()).spawn().map_err(|e| CoreError::io(exe, e))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(cancelled());
        }
        if let Some(status) = child.try_wait().map_err(|e| CoreError::io(exe, e))? {
            if !status.success() {
                let mut err = String::new();
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut err);
                }
                return Err(invalid(archive, format!("could not be changed. {}", err.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim())));
            }
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

fn copy_tree(from: &Path, to: &Path, cancel: &AtomicBool) -> Result<u64> {
    if cancel.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    let meta = fs::symlink_metadata(from).map_err(|e| CoreError::io(from, e))?;
    if meta.file_type().is_symlink() {
        Ok(0)
    } else if meta.is_dir() {
        fs::create_dir_all(to).map_err(|e| CoreError::io(to, e))?;
        let mut n = 0;
        for e in fs::read_dir(from).map_err(|e| CoreError::io(from, e))?.flatten() {
            n += copy_tree(&e.path(), &to.join(e.file_name()), cancel)?;
        }
        Ok(n)
    } else {
        if let Some(p) = to.parent() {
            fs::create_dir_all(p).map_err(|e| CoreError::io(p, e))?;
        }
        fs::copy(from, to).map_err(|e| CoreError::io(to, e))?;
        Ok(1)
    }
}

fn seven_rewrite(archive: &Path, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let Some((Tool::SevenZip, exe)) = find_tool() else { return Err(no_tool(archive)) };
    let archive = fs::canonicalize(archive).map_err(|e| CoreError::io(archive, e))?;
    let items = browse::index(&archive)?;
    let os = |s: &str| std::ffi::OsString::from(s);
    // Renames and deletions work on every entry under the item, listed one by one.
    if !plan.renames.is_empty() {
        let mut args = vec![os("rn"), os("-y"), archive.as_os_str().to_owned()];
        for it in items.iter() {
            if let Action::Rename(to) = plan.action(&it.path) {
                args.push(os(&it.path));
                args.push(os(&to));
            }
        }
        for chunk in args[3..].chunks(400) {
            let mut a = args[..3].to_vec();
            a.extend(chunk.iter().cloned());
            seven_run(&exe, a, None, cancel, &archive)?;
        }
    }
    if !plan.drops.is_empty() {
        let names: Vec<String> = items.iter().filter(|i| matches!(plan.action(&i.path), Action::Drop)).map(|i| i.path.clone()).collect();
        for chunk in names.chunks(400) {
            let mut a = vec![os("d"), os("-y"), archive.as_os_str().to_owned(), os("--")];
            a.extend(chunk.iter().map(|s| os(s)));
            seven_run(&exe, a, None, cancel, &archive)?;
        }
    }
    let mut files = 0u64;
    if !plan.add.is_empty() || !plan.dirs.is_empty() {
        // A staging tree laid out as it should appear in the archive, added as a whole.
        let stage = scratch_dir()?;
        let result = (|| -> Result<()> {
            for (src, name) in &plan.add {
                files += copy_tree(src, &stage.join(name), cancel)?;
                report(files, files, name);
            }
            for d in &plan.dirs {
                fs::create_dir_all(stage.join(d)).map_err(|e| CoreError::io(&stage, e))?;
            }
            seven_run(&exe, vec![os("a"), os("-y"), archive.as_os_str().to_owned(), os("*")], Some(&stage), cancel, &archive)
        })();
        let _ = fs::remove_dir_all(&stage);
        result?;
    }
    Ok(files)
}

// ---- Everything else: unpack, change, pack again -----------------------------

fn external_rewrite(archive: &Path, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let repack = repack_for(archive).ok_or_else(|| invalid(archive, "cannot be changed here."))?;
    let stage = scratch_dir()?;
    let tmp = temp_beside(archive);
    let result = (|| -> Result<u64> {
        // 1. Everything out, as it is now.
        browse::extract_items(archive, &[String::new()], &stage, cancel, &mut |d, t, c| report(d, t.max(1), c))?;
        // 2. The change, on disk.
        for d in &plan.drops {
            let p = stage.join(d);
            let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
        }
        for (from, to) in &plan.renames {
            let dest = stage.join(to);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
            }
            fs::rename(stage.join(from), &dest).map_err(|e| CoreError::io(&dest, e))?;
        }
        let mut files = 0u64;
        for (src, name) in &plan.add {
            files += copy_tree(src, &stage.join(name), cancel)?;
            report(files, files, name);
        }
        for d in &plan.dirs {
            fs::create_dir_all(stage.join(d)).map_err(|e| CoreError::io(&stage, e))?;
        }
        // 3. Packed again into a new file, which replaces the old one only when complete.
        let os = |s: &str| std::ffi::OsString::from(s);
        match repack {
            Repack::SevenZip(kind) => {
                let exe = seven_zip().ok_or_else(|| no_tool(archive))?;
                seven_run(&exe, vec![os("a"), os("-y"), os(&format!("-t{kind}")), tmp.as_os_str().to_owned(), os("*")], Some(&stage), cancel, archive)?;
            }
            Repack::Bsdtar(opts) => {
                let exe = find_bsdtar().ok_or_else(|| no_tool(archive))?;
                let mut args = vec![os("-c")];
                args.extend(opts.iter().map(|o| os(o)));
                args.extend([os("-f"), tmp.as_os_str().to_owned(), os(".")]);
                seven_run(&exe, args, Some(&stage), cancel, archive)?;
            }
            Repack::Rar => {
                let exe = find_rar().ok_or_else(|| no_tool(archive))?;
                // WinRAR decides the format from the name given; ".rar" is added to a bare one.
                let named = tmp.with_extension("rar");
                seven_run(&exe, vec![os("a"), os("-r"), os("-ep1"), os("-idq"), os("-y"), named.as_os_str().to_owned(), os("*")], Some(&stage), cancel, archive)?;
                fs::rename(&named, &tmp).map_err(|e| CoreError::io(&named, e))?;
            }
        }
        fs::rename(&tmp, archive).map_err(|e| CoreError::io(archive, e))?;
        Ok(files)
    })();
    let _ = fs::remove_dir_all(&stage);
    let _ = fs::remove_file(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::{entries_in, extract_items, index};

    fn tmp(name: &str) -> PathBuf {
        let mut id = [0u8; 4];
        let _ = getrandom::fill(&mut id);
        let p = std::env::temp_dir().join(format!("fast-edit-test-{name}-{}", id.iter().map(|b| format!("{b:02x}")).collect::<String>()));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }
    fn names(archive: &Path, dir: &str) -> Vec<String> {
        entries_in(&index(archive).unwrap(), dir, true).into_iter().map(|e| e.name).collect()
    }
    fn read(archive: &Path, inner: &str) -> String {
        let out = tmp("read");
        extract_items(archive, &[inner.to_string()], &out, &no(), &mut |_, _, _| {}).unwrap();
        let s = fs::read_to_string(out.join(leaf(inner))).unwrap();
        let _ = fs::remove_dir_all(out);
        s
    }

    /// The same edits on every kind of archive that can be changed.
    fn exercise(archive: &Path, work: &Path) {
        let src = work.join("new");
        fs::create_dir_all(src.join("pics")).unwrap();
        fs::write(src.join("hello.txt"), "hello").unwrap();
        fs::write(src.join("pics/p.txt"), "pic").unwrap();
        // add into the top, and into a folder
        add(archive, "", &[src.join("hello.txt"), src.join("pics")], &no(), &mut |_, _, _| {}).unwrap();
        assert!(names(archive, "").contains(&"hello.txt".to_string()) && names(archive, "").contains(&"pics".to_string()), "{:?}", names(archive, ""));
        assert_eq!(read(archive, "pics/p.txt"), "pic");
        // the old content is still intact
        assert_eq!(read(archive, "docs/a.txt"), "alpha");
        // adding the same name again keeps both
        add(archive, "docs", &[src.join("hello.txt")], &no(), &mut |_, _, _| {}).unwrap();
        add(archive, "docs", &[src.join("hello.txt")], &no(), &mut |_, _, _| {}).unwrap();
        assert!(names(archive, "docs").contains(&"hello (2).txt".to_string()), "{:?}", names(archive, "docs"));
        // move a folder inside, and refuse to move it into itself
        let moved = move_within(archive, &["pics".to_string()], "docs", &no(), &mut |_, _, _| {}).unwrap();
        assert_eq!(moved, vec!["docs/pics".to_string()]);
        assert_eq!(read(archive, "docs/pics/p.txt"), "pic");
        assert!(!names(archive, "").contains(&"pics".to_string()));
        assert!(move_within(archive, &["docs".to_string()], "docs/deep", &no(), &mut |_, _, _| {}).is_err());
        // rename a file and a folder
        assert_eq!(rename(archive, "docs/a.txt", "alpha.txt", &no()).unwrap(), "docs/alpha.txt");
        assert_eq!(rename(archive, "docs/pics", "images", &no()).unwrap(), "docs/images");
        assert_eq!(read(archive, "docs/images/p.txt"), "pic");
        assert!(rename(archive, "docs/alpha.txt", "images", &no()).is_err());
        // new folder, then delete a folder with things in it
        assert_eq!(create(archive, "", "fresh", true, &no()).unwrap(), "fresh");
        assert!(names(archive, "").contains(&"fresh".to_string()));
        delete(archive, &["docs/images".to_string(), "hello.txt".to_string()], &no()).unwrap();
        assert!(!names(archive, "docs").contains(&"images".to_string()));
        assert!(!names(archive, "").contains(&"hello.txt".to_string()));
        assert_eq!(read(archive, "docs/alpha.txt"), "alpha");
        // nothing was left behind
        let stray: Vec<_> = fs::read_dir(archive.parent().unwrap()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".fast-edit")).collect();
        assert!(stray.is_empty());
    }

    fn base_tree(root: &Path) {
        fs::create_dir_all(root.join("docs/deep")).unwrap();
        fs::write(root.join("docs/a.txt"), "alpha").unwrap();
        fs::write(root.join("docs/deep/b.txt"), "bravo").unwrap();
        fs::write(root.join("top.txt"), "top").unwrap();
    }

    #[test]
    fn zip_can_be_changed() {
        let work = tmp("zip");
        let root = work.join("base");
        base_tree(&root);
        let archive = work.join("t.zip");
        crate::archive::compress(&[root.join("docs"), root.join("top.txt")], &archive, &no()).unwrap();
        exercise(&archive, &work);
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn every_kind_of_tar_can_be_changed() {
        let work = tmp("tar");
        let root = work.join("base");
        base_tree(&root);
        for name in ["t.tar", "t.tar.gz", "t.tar.bz2", "t.tar.xz", "t.tbz2", "t.txz"] {
            let archive = work.join(name);
            let f = std::io::BufWriter::new(File::create(&archive).unwrap());
            let sink = match name {
                "t.tar.gz" => Sink::Gz(flate2::write::GzEncoder::new(f, flate2::Compression::fast())),
                "t.tar.bz2" | "t.tbz2" => Sink::Bz(bzip2::write::BzEncoder::new(f, bzip2::Compression::fast())),
                "t.tar.xz" | "t.txz" => Sink::Xz(Box::new(lzma_rust2::XzWriter::new(f, lzma_rust2::XzOptions::with_preset(1)).unwrap())),
                _ => Sink::Plain(f),
            };
            let mut b = tar::Builder::new(sink);
            b.append_dir_all("docs", root.join("docs")).unwrap();
            b.append_path_with_name(root.join("top.txt"), "top.txt").unwrap();
            b.into_inner().unwrap().finish().unwrap();
            exercise(&archive, &work);
        }
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn seven_z_can_be_changed_when_seven_zip_is_installed() {
        if !matches!(find_tool(), Some((Tool::SevenZip, _))) {
            return;
        }
        let work = tmp("7z");
        let root = work.join("base");
        base_tree(&root);
        let archive = work.join("t.7z");
        let exe = find_tool().unwrap().1;
        let st = std::process::Command::new(exe).current_dir(&root).args(["a", "-y"]).arg(&archive).arg("*").output().unwrap();
        assert!(st.status.success());
        exercise(&archive, &work);
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn wim_can_be_changed_when_seven_zip_is_installed() {
        let Some(exe) = seven_zip() else { return };
        let work = tmp("wim");
        let root = work.join("base");
        base_tree(&root);
        let archive = work.join("t.wim");
        let st = std::process::Command::new(exe).current_dir(&root).args(["a", "-y", "-twim"]).arg(&archive).arg("*").output().unwrap();
        if !st.status.success() {
            return; // this build of 7-Zip cannot write wim
        }
        exercise(&archive, &work);
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn formats_nothing_can_write_say_they_are_read_only() {
        let work = tmp("ro");
        for name in ["a.cab", "a.gz", "a.deb", "a.arj", "a.zst"] {
            let p = work.join(name);
            fs::write(&p, b"x").unwrap();
            let e = delete(&p, &["x".into()], &no()).unwrap_err().to_string();
            assert!(e.contains("cannot be changed"), "{name}: {e}");
        }
        let _ = fs::remove_dir_all(work);
    }

    #[test]
    fn a_cancelled_edit_leaves_the_archive_alone() {
        let work = tmp("cancel");
        let root = work.join("base");
        base_tree(&root);
        let archive = work.join("t.zip");
        crate::archive::compress(&[root.join("docs")], &archive, &no()).unwrap();
        let before = fs::read(&archive).unwrap();
        let cancel = AtomicBool::new(true);
        assert!(delete(&archive, &["docs/a.txt".into()], &cancel).is_err());
        assert_eq!(fs::read(&archive).unwrap(), before);
        assert_eq!(fs::read_dir(&work).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains("fast-edit")).count(), 0);
        let _ = fs::remove_dir_all(work);
    }
}
