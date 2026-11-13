//! Changing what is inside an archive: add files, move and rename items, make
//! folders, delete.
//!
//! Zip, tar and tar.gz are rewritten by this program (a new file is written
//! beside the archive and swapped in only when it is complete, so a failure
//! or a cancel leaves the original untouched). 7z goes through the installed
//! 7-Zip. Everything else (rar, bzip2 and xz tars, iso ...) cannot be changed
//! and says so.
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

/// Whether this archive can be changed, and why not.
pub fn check_editable(archive: &Path) -> Result<Kind> {
    let kind = browse::kind_of(archive).ok_or_else(|| CoreError::Invalid("This is not a kind of archive that can be opened.".into()))?;
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match kind {
        Kind::Zip | Kind::Tar | Kind::TarGz => Ok(kind),
        Kind::SevenZ => match find_tool() {
            Some((Tool::SevenZip, _)) => Ok(kind),
            _ => Err(CoreError::Invalid(format!("Changing {name} needs 7-Zip. Install it and try again."))),
        },
        _ => Err(CoreError::Invalid(format!("Items inside {name} cannot be changed, only read. Extract it to change what is in it. Zip, tar, tar.gz and 7z files can be changed."))),
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
        _ => {
            let tmp = temp_beside(archive);
            let r = match kind {
                Kind::Zip => zip_rewrite(archive, &tmp, plan, cancel, report),
                _ => tar_rewrite(archive, &tmp, kind == Kind::TarGz, plan, cancel, report),
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

fn tar_rewrite(archive: &Path, tmp: &Path, gz: bool, plan: &Plan, cancel: &AtomicBool, report: Report) -> Result<u64> {
    let kind = if gz { Kind::TarGz } else { Kind::Tar };
    let file = File::create(tmp).map_err(|e| CoreError::io(tmp, e))?;
    let sink: Box<dyn Write> = if gz {
        Box::new(flate2::write::GzEncoder::new(BufWriter::with_capacity(256 * 1024, file), flate2::Compression::default()))
    } else {
        Box::new(BufWriter::with_capacity(256 * 1024, file))
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
    drop(sink); // the gzip encoder finishes when dropped
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
    fn tar_and_tar_gz_can_be_changed() {
        let work = tmp("tar");
        let root = work.join("base");
        base_tree(&root);
        for (name, gz) in [("t.tar", false), ("t.tar.gz", true)] {
            let archive = work.join(name);
            let f = File::create(&archive).unwrap();
            let w: Box<dyn Write> = if gz { Box::new(flate2::write::GzEncoder::new(f, flate2::Compression::fast())) } else { Box::new(f) };
            let mut b = tar::Builder::new(w);
            b.append_dir_all("docs", root.join("docs")).unwrap();
            b.append_path_with_name(root.join("top.txt"), "top.txt").unwrap();
            b.into_inner().unwrap().flush().unwrap();
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
    fn rar_and_other_formats_say_they_are_read_only() {
        let work = tmp("ro");
        for name in ["a.rar", "a.tar.bz2", "a.iso", "a.gz"] {
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
