//! File tools in the style of Total Commander and OneCommander: split and join files, UUE / XXE /
//! MIME (Base64) encoding, comparing files and folders, finding duplicates, searching inside files
//! (archives too), converting pictures in bulk, and the reads behind the file viewer.
//!
//! Everything works on paths and reports through a progress callback and a cancel flag, so the
//! interface can run it as a job. Nothing here changes a file unless its name says so.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

pub type Res<T> = Result<T, String>;
/// done, total, what is being worked on.
pub type Progress<'a> = &'a (dyn Fn(u64, u64, &str) + Sync);

fn io<E: std::fmt::Display>(path: &Path, e: E) -> String {
    format!("{}: {e}", path.display())
}
fn stopped(cancel: &AtomicBool) -> Res<()> {
    if cancel.load(Ordering::Relaxed) { Err("cancelled".into()) } else { Ok(()) }
}
fn mtime_ms(m: &fs::Metadata) -> i64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64).unwrap_or(0)
}
fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ---- Wildcards -----------------------------------------------------------------------------

/// `*` and `?` matching, ignoring case. One pattern.
pub fn wild(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None::<usize>, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Several patterns separated by `;`, `,` or `|` (or none, which matches everything).
pub fn wild_any(masks: &str, name: &str) -> bool {
    let list: Vec<&str> = masks.split([';', ',', '|']).map(str::trim).filter(|m| !m.is_empty()).collect();
    list.is_empty() || list.iter().any(|m| wild(m, name))
}

// ---- Split and join ------------------------------------------------------------------------

pub fn split_file(src: &Path, dest_dir: &Path, part_size: u64, cancel: &AtomicBool, progress: Progress) -> Res<Vec<PathBuf>> {
    if part_size == 0 {
        return Err("The part size must be more than zero.".into());
    }
    let meta = fs::metadata(src).map_err(|e| io(src, e))?;
    if !meta.is_file() {
        return Err("Only files can be split.".into());
    }
    fs::create_dir_all(dest_dir).map_err(|e| io(dest_dir, e))?;
    let name = file_name(src);
    let total = meta.len();
    let mut input = BufReader::with_capacity(1 << 20, File::open(src).map_err(|e| io(src, e))?);
    let mut parts = Vec::new();
    let mut crc = crc32fast::Hasher::new();
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    let mut index = 1;
    while done < total || (total == 0 && index == 1) {
        let part = dest_dir.join(format!("{name}.{index:03}"));
        let mut out = BufWriter::new(File::create(&part).map_err(|e| io(&part, e))?);
        let mut written = 0u64;
        while written < part_size {
            stopped(cancel)?;
            let want = buf.len().min((part_size - written) as usize);
            let n = input.read(&mut buf[..want]).map_err(|e| io(src, e))?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| io(&part, e))?;
            written += n as u64;
            done += n as u64;
            progress(done, total, &name);
        }
        out.flush().map_err(|e| io(&part, e))?;
        parts.push(part);
        index += 1;
        if written == 0 {
            break;
        }
    }
    let info = dest_dir.join(format!("{name}.crc"));
    fs::write(&info, format!("filename={name}\nsize={total}\ncrc32={:08x}\n", crc.finalize())).map_err(|e| io(&info, e))?;
    Ok(parts)
}

/// Join `name.001`, `name.002`… back into `name`. Checked against `name.crc` when that is next to the parts.
pub fn join_files(first: &Path, dest_dir: &Path, cancel: &AtomicBool, progress: Progress) -> Res<(PathBuf, Option<bool>)> {
    let fname = file_name(first);
    let (stem, ext) = fname.rsplit_once('.').ok_or("Choose the first part (name.001).")?;
    if ext.len() < 3 || !ext.chars().all(|c| c.is_ascii_digit()) {
        return Err("Choose the first part (name.001).".into());
    }
    let dir = first.parent().unwrap_or(Path::new("."));
    let width = ext.len();
    let start: u32 = ext.parse().unwrap_or(1);
    let mut parts = Vec::new();
    let mut i = start;
    loop {
        let p = dir.join(format!("{stem}.{i:0width$}"));
        if !p.is_file() {
            break;
        }
        parts.push(p);
        i += 1;
    }
    if parts.is_empty() {
        return Err("The part was not found.".into());
    }
    let info = fs::read_to_string(dir.join(format!("{stem}.crc"))).ok();
    let field = |k: &str| info.as_ref().and_then(|t| t.lines().find_map(|l| l.strip_prefix(&format!("{k}=")).map(|v| v.trim().to_string())));
    let out_name = field("filename").map(|n| file_name(Path::new(&n))).filter(|n| !n.is_empty()).unwrap_or_else(|| stem.to_string());
    fs::create_dir_all(dest_dir).map_err(|e| io(dest_dir, e))?;
    let out_path = dest_dir.join(&out_name);
    if parts.iter().any(|p| p == &out_path) {
        return Err("The joined file would overwrite a part.".into());
    }
    let total: u64 = parts.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum();
    let mut out = BufWriter::new(File::create(&out_path).map_err(|e| io(&out_path, e))?);
    let mut crc = crc32fast::Hasher::new();
    let (mut done, mut buf) = (0u64, vec![0u8; 1 << 20]);
    for p in &parts {
        let mut f = File::open(p).map_err(|e| io(p, e))?;
        loop {
            stopped(cancel).inspect_err(|_| {
                let _ = fs::remove_file(&out_path);
            })?;
            let n = f.read(&mut buf).map_err(|e| io(p, e))?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            out.write_all(&buf[..n]).map_err(|e| io(&out_path, e))?;
            done += n as u64;
            progress(done, total, &out_name);
        }
    }
    out.flush().map_err(|e| io(&out_path, e))?;
    let sum = format!("{:08x}", crc.finalize());
    let verified = field("crc32").map(|c| c.eq_ignore_ascii_case(&sum)).or_else(|| field("size").and_then(|s| s.parse::<u64>().ok()).map(|s| s == done));
    Ok((out_path, verified))
}

// ---- UUE, XXE and MIME ---------------------------------------------------------------------

const UU: &[u8; 64] = b"`!\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_";
const XX: &[u8; 64] = b"+-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn uu_encode(data: &[u8], alphabet: &[u8; 64], name: &str) -> String {
    let mut s = format!("begin 644 {name}\n");
    for line in data.chunks(45) {
        s.push(alphabet[line.len()] as char);
        for g in line.chunks(3) {
            let (a, b, c) = (g[0], *g.get(1).unwrap_or(&0), *g.get(2).unwrap_or(&0));
            for v in [a >> 2, ((a & 3) << 4) | (b >> 4), ((b & 15) << 2) | (c >> 6), c & 63] {
                s.push(alphabet[v as usize] as char);
            }
        }
        s.push('\n');
    }
    s.push(alphabet[0] as char);
    s.push_str("\nend\n");
    s
}

fn uu_decode(lines: &[&str], alphabet: &[u8; 64]) -> Res<Vec<u8>> {
    let value = |c: u8| -> Res<u8> {
        // Both programs have written a space for zero.
        if c == b' ' && alphabet[0] == b'`' {
            return Ok(0);
        }
        alphabet.iter().position(|a| *a == c).map(|p| p as u8).ok_or_else(|| format!("Unexpected character {:?} in the encoded text.", c as char))
    };
    let mut out = Vec::new();
    for line in lines {
        let b = line.trim_end_matches(['\r', '\n']).as_bytes();
        if b.is_empty() {
            continue;
        }
        let n = value(b[0])? as usize;
        if n == 0 {
            break;
        }
        let mut got = Vec::with_capacity(n + 3);
        for g in b[1..].chunks(4) {
            let v: Vec<u8> = (0..4).map(|i| g.get(i).map(|c| value(*c)).unwrap_or(Ok(0))).collect::<Res<_>>()?;
            got.extend([(v[0] << 2) | (v[1] >> 4), (v[1] << 4) | (v[2] >> 2), (v[2] << 6) | v[3]]);
        }
        got.truncate(n);
        out.extend(got);
    }
    Ok(out)
}

pub fn encode_file(src: &Path, dest_dir: &Path, kind: &str) -> Res<PathBuf> {
    use base64::Engine;
    let data = fs::read(src).map_err(|e| io(src, e))?;
    let name = file_name(src);
    let (text, ext) = match kind {
        "uue" => (uu_encode(&data, UU, &name), "uue"),
        "xxe" => (uu_encode(&data, XX, &name), "xxe"),
        "base64" | "mime" => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            let mut s = format!("Content-Type: application/octet-stream; name=\"{name}\"\nContent-Transfer-Encoding: base64\nContent-Disposition: attachment; filename=\"{name}\"\n\n");
            for chunk in b64.as_bytes().chunks(76) {
                s.push_str(std::str::from_utf8(chunk).unwrap_or(""));
                s.push('\n');
            }
            (s, "b64")
        }
        other => return Err(format!("Unknown encoding {other}.")),
    };
    fs::create_dir_all(dest_dir).map_err(|e| io(dest_dir, e))?;
    let out = dest_dir.join(format!("{name}.{ext}"));
    fs::write(&out, text).map_err(|e| io(&out, e))?;
    Ok(out)
}

fn safe_name(n: &str) -> String {
    let base = n.rsplit(['/', '\\']).next().unwrap_or(n).trim().trim_matches('"');
    if base.is_empty() || base == "." || base == ".." { "decoded.bin".into() } else { base.to_string() }
}

/// Decode a UUE, XXE or MIME file into `dest_dir`; returns what was written.
pub fn decode_file(src: &Path, dest_dir: &Path) -> Res<Vec<PathBuf>> {
    use base64::Engine;
    let bytes = fs::read(src).map_err(|e| io(src, e))?;
    let text = String::from_utf8_lossy(&bytes);
    fs::create_dir_all(dest_dir).map_err(|e| io(dest_dir, e))?;
    let mut written = Vec::new();
    let mut save = |name: &str, data: Vec<u8>| -> Res<()> {
        let p = dest_dir.join(safe_name(name));
        fs::write(&p, data).map_err(|e| io(&p, e))?;
        written.push(p);
        Ok(())
    };
    let lines: Vec<&str> = text.lines().collect();
    // One or several "begin <mode> <name>" … "end" blocks.
    let mut i = 0;
    let mut any = false;
    while i < lines.len() {
        if let Some(rest) = lines[i].strip_prefix("begin ") {
            let mut it = rest.splitn(2, ' ');
            let (mode, name) = (it.next().unwrap_or(""), it.next().unwrap_or("decoded.bin"));
            if mode.chars().all(|c| c.is_ascii_digit()) && !mode.is_empty() {
                let end = lines[i + 1..].iter().position(|l| l.trim() == "end").map(|p| p + i + 1).unwrap_or(lines.len());
                let body = &lines[i + 1..end];
                // XXE uses lowercase letters, which UUE never has.
                let xx = body.iter().any(|l| l.bytes().any(|c| c.is_ascii_lowercase()));
                save(name, uu_decode(body, if xx { XX } else { UU })?)?;
                any = true;
                i = end;
            }
        }
        i += 1;
    }
    if any {
        return Ok(written);
    }
    // MIME: every base64 part, or the whole file when it has no headers.
    let boundary = lines.iter().find_map(|l| l.to_lowercase().find("boundary=").map(|p| l[p + 9..].trim().trim_matches(|c| c == '"' || c == ';').to_string()));
    let sections: Vec<String> = match &boundary {
        Some(b) => text.split(&format!("--{b}")).skip(1).map(str::to_string).collect(),
        None => vec![text.to_string()],
    };
    for (n, sec) in sections.iter().enumerate() {
        let (head, body) = sec.split_once("\n\n").or_else(|| sec.split_once("\r\n\r\n")).unwrap_or(("", sec));
        let has_headers = head.to_lowercase().contains("content-");
        if has_headers && !head.to_lowercase().contains("base64") {
            continue;
        }
        let (head, body) = if has_headers { (head, body) } else { ("", sec.as_str()) };
        let clean: String = body.lines().take_while(|l| !l.starts_with("--")).flat_map(|l| l.chars()).filter(|c| !c.is_whitespace()).collect();
        if clean.is_empty() {
            continue;
        }
        let Ok(data) = base64::engine::general_purpose::STANDARD.decode(clean.trim_end_matches('=').to_string() + &"=".repeat((4 - clean.trim_end_matches('=').len() % 4) % 4)) else { continue };
        let name = head
            .lines()
            .find_map(|l| {
                let low = l.to_lowercase();
                let at = low.find("filename=").map(|p| p + 9).or_else(|| low.find("name=").map(|p| p + 5))?;
                Some(l[at..].trim().trim_matches(';').trim_matches('"').split('"').next().unwrap_or("").to_string())
            })
            .unwrap_or_else(|| if sections.len() > 1 { format!("part{}.bin", n + 1) } else { format!("{}.bin", file_name(src).trim_end_matches(".b64").trim_end_matches(".txt")) });
        save(&name, data)?;
    }
    if written.is_empty() {
        return Err("This does not look like UUE, XXE or MIME (Base64) data.".into());
    }
    Ok(written)
}

// ---- Compare files -------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffRow {
    /// "same", "changed", "removed" (only on the left) or "added" (only on the right)
    pub kind: String,
    pub a_no: Option<usize>,
    pub a: String,
    pub b_no: Option<usize>,
    pub b: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum FileCompare {
    Identical { size: u64 },
    Binary { a_size: u64, b_size: u64, first_diff: Option<u64> },
    Text { rows: Vec<DiffRow>, changed: usize, added: usize, removed: usize, truncated: bool },
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CompareOpts {
    pub ignore_whitespace: bool,
    pub ignore_case: bool,
}

/// Offset of the first byte that differs, or None when one is the start of the other and they are the same size.
fn first_difference(a: &Path, b: &Path, cancel: &AtomicBool) -> Res<Option<u64>> {
    let (mut fa, mut fb) = (BufReader::new(File::open(a).map_err(|e| io(a, e))?), BufReader::new(File::open(b).map_err(|e| io(b, e))?));
    let (mut ba, mut bb) = (vec![0u8; 1 << 18], vec![0u8; 1 << 18]);
    let mut at = 0u64;
    loop {
        stopped(cancel)?;
        let na = read_full(&mut fa, &mut ba).map_err(|e| io(a, e))?;
        let nb = read_full(&mut fb, &mut bb).map_err(|e| io(b, e))?;
        let n = na.min(nb);
        if let Some(p) = ba[..n].iter().zip(&bb[..n]).position(|(x, y)| x != y) {
            return Ok(Some(at + p as u64));
        }
        if na != nb {
            return Ok(Some(at + n as u64));
        }
        if na == 0 {
            return Ok(None);
        }
        at += n as u64;
    }
}

fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

fn looks_binary(path: &Path) -> bool {
    let mut b = vec![0u8; 8192];
    let n = File::open(path).and_then(|mut f| read_full(&mut f, &mut b)).unwrap_or(0);
    b[..n].contains(&0)
}

fn key(line: &str, o: &CompareOpts) -> String {
    let mut s: String = if o.ignore_whitespace { line.split_whitespace().collect::<Vec<_>>().join(" ") } else { line.to_string() };
    if o.ignore_case {
        s = s.to_lowercase();
    }
    s
}

const TEXT_LIMIT: u64 = 20 * 1024 * 1024;
const ROW_LIMIT: usize = 60_000;

pub fn compare_files(a: &Path, b: &Path, opts: &CompareOpts, cancel: &AtomicBool) -> Res<FileCompare> {
    let (ma, mb) = (fs::metadata(a).map_err(|e| io(a, e))?, fs::metadata(b).map_err(|e| io(b, e))?);
    if !ma.is_file() || !mb.is_file() {
        return Err("Both must be files.".into());
    }
    let diff = if ma.len() == mb.len() { first_difference(a, b, cancel)? } else { None.or(Some(0)) };
    if ma.len() == mb.len() && diff.is_none() {
        return Ok(FileCompare::Identical { size: ma.len() });
    }
    let texty = ma.len() <= TEXT_LIMIT && mb.len() <= TEXT_LIMIT && !looks_binary(a) && !looks_binary(b);
    if !texty {
        let first = first_difference(a, b, cancel)?;
        return Ok(FileCompare::Binary { a_size: ma.len(), b_size: mb.len(), first_diff: first });
    }
    let ta = String::from_utf8_lossy(&fs::read(a).map_err(|e| io(a, e))?).into_owned();
    let tb = String::from_utf8_lossy(&fs::read(b).map_err(|e| io(b, e))?).into_owned();
    let (la, lb): (Vec<&str>, Vec<&str>) = (ta.lines().collect(), tb.lines().collect());
    let (ka, kb): (Vec<String>, Vec<String>) = (la.iter().map(|l| key(l, opts)).collect(), lb.iter().map(|l| key(l, opts)).collect());
    if ka == kb {
        // Only whitespace or case differs, and that was asked to be ignored.
        return Ok(FileCompare::Identical { size: ma.len() });
    }
    use similar::{capture_diff_slices, Algorithm, DiffOp};
    let ops = capture_diff_slices(Algorithm::Myers, &ka, &kb);
    let (mut rows, mut changed, mut added, mut removed) = (Vec::new(), 0, 0, 0);
    let row = |kind: &str, i: Option<usize>, j: Option<usize>| DiffRow {
        kind: kind.into(),
        a_no: i.map(|x| x + 1),
        a: i.map(|x| la[x].to_string()).unwrap_or_default(),
        b_no: j.map(|x| x + 1),
        b: j.map(|x| lb[x].to_string()).unwrap_or_default(),
    };
    for op in ops {
        match op {
            DiffOp::Equal { old_index, new_index, len } => rows.extend((0..len).map(|k| row("same", Some(old_index + k), Some(new_index + k)))),
            DiffOp::Delete { old_index, old_len, .. } => {
                removed += old_len;
                rows.extend((0..old_len).map(|k| row("removed", Some(old_index + k), None)));
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                added += new_len;
                rows.extend((0..new_len).map(|k| row("added", None, Some(new_index + k))));
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for k in 0..old_len.max(new_len) {
                    let (i, j) = ((k < old_len).then_some(old_index + k), (k < new_len).then_some(new_index + k));
                    match (i, j) {
                        (Some(_), Some(_)) => {
                            changed += 1;
                            rows.push(row("changed", i, j));
                        }
                        (Some(_), None) => {
                            removed += 1;
                            rows.push(row("removed", i, j));
                        }
                        _ => {
                            added += 1;
                            rows.push(row("added", i, j));
                        }
                    }
                }
            }
        }
        if rows.len() > ROW_LIMIT {
            break;
        }
    }
    let truncated = rows.len() > ROW_LIMIT;
    rows.truncate(ROW_LIMIT);
    Ok(FileCompare::Text { rows, changed, added, removed, truncated })
}

// ---- Compare folders -----------------------------------------------------------------------

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    pub size: u64,
    pub modified: i64,
    pub is_dir: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirDiff {
    /// Path below each folder, with `/` between parts.
    pub rel: String,
    /// "leftOnly", "rightOnly", "leftNewer", "rightNewer", "different" (same date, other content) or "typeMismatch"
    pub status: String,
    pub left: Option<Side>,
    pub right: Option<Side>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DirCompare {
    pub entries: Vec<DirDiff>,
    pub same: usize,
    pub truncated: bool,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DirOpts {
    pub recursive: bool,
    pub by_content: bool,
    pub include_hidden: bool,
}
impl Default for DirOpts {
    fn default() -> Self {
        Self { recursive: true, by_content: false, include_hidden: true }
    }
}

const TREE_LIMIT: usize = 300_000;

fn tree(root: &Path, o: &DirOpts, cancel: &AtomicBool) -> Res<BTreeMap<String, Side>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = stack.pop() {
        stopped(cancel)?;
        let rd = fs::read_dir(&dir).map_err(|e| io(&dir, e))?;
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !o.include_hidden && name.starts_with('.') {
                continue;
            }
            let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let is_dir = m.is_dir();
            out.insert(r.clone(), Side { size: if is_dir { 0 } else { m.len() }, modified: mtime_ms(&m), is_dir });
            if is_dir && o.recursive && !m.file_type().is_symlink() {
                stack.push((e.path(), r));
            }
            if out.len() > TREE_LIMIT {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

pub fn compare_dirs(left: &Path, right: &Path, o: &DirOpts, cancel: &AtomicBool, progress: Progress) -> Res<DirCompare> {
    let (l, r) = (tree(left, o, cancel)?, tree(right, o, cancel)?);
    let truncated = l.len() > TREE_LIMIT || r.len() > TREE_LIMIT;
    let mut keys: Vec<&String> = l.keys().chain(r.keys()).collect();
    keys.sort();
    keys.dedup();
    let total = keys.len() as u64;
    let decide = |k: &&String| -> Res<Option<DirDiff>> {
        stopped(cancel)?;
        let (a, b) = (l.get(*k), r.get(*k));
        let mk = |status: &str| Some(DirDiff { rel: (*k).clone(), status: status.into(), left: a.cloned(), right: b.cloned() });
        Ok(match (a, b) {
            (Some(_), None) => mk("leftOnly"),
            (None, Some(_)) => mk("rightOnly"),
            (Some(x), Some(y)) if x.is_dir != y.is_dir => mk("typeMismatch"),
            (Some(x), Some(y)) if x.is_dir => {
                let _ = y;
                None
            }
            (Some(x), Some(y)) => {
                let close = (x.modified - y.modified).abs() <= 2000;
                if x.size == y.size && (!o.by_content || first_difference(&left.join(k.as_str()), &right.join(k.as_str()), cancel)?.is_none()) && (close || o.by_content) {
                    None
                } else if x.size != y.size || !close {
                    if x.size == y.size && o.by_content && first_difference(&left.join(k.as_str()), &right.join(k.as_str()), cancel)?.is_none() {
                        None
                    } else if close {
                        mk("different")
                    } else if x.modified > y.modified {
                        mk("leftNewer")
                    } else {
                        mk("rightNewer")
                    }
                } else {
                    mk("different")
                }
            }
            (None, None) => None,
        })
    };
    let results: Vec<Res<Option<DirDiff>>> = keys.par_iter().map(decide).collect();
    let mut entries = Vec::new();
    for res in results {
        if let Some(d) = res? {
            entries.push(d);
        }
    }
    let same = l.iter().filter(|(k, v)| !v.is_dir && r.get(*k).is_some_and(|w| !w.is_dir)).count().saturating_sub(entries.iter().filter(|e| e.left.is_some() && e.right.is_some() && !e.left.as_ref().unwrap().is_dir).count());
    progress(total, total, "");
    Ok(DirCompare { entries, same, truncated })
}

/// Unpack an archive into `into` so it can be compared like a folder. For zip files every file gets the date stored
/// in the archive; the other kinds do not keep dates, so the answer says so (`false`) and the caller compares contents.
pub fn stage_archive(archive: &Path, into: &Path, cancel: &AtomicBool, progress: Progress) -> Res<bool> {
    let _ = fs::remove_dir_all(into);
    crate::archive::extract_with(archive, into, cancel, &mut |done, total, note| progress(done, total, note)).map_err(|e| e.to_string())?;
    if crate::archive::format_of(archive) != Some(crate::archive::Format::Zip) {
        return Ok(false);
    }
    use chrono::TimeZone;
    let mut zip = zip::ZipArchive::new(BufReader::new(File::open(archive).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        stopped(cancel)?;
        let Ok(entry) = zip.by_index(i) else { continue };
        if entry.is_dir() || entry.is_symlink() {
            continue;
        }
        let (Some(rel), Some(t)) = (crate::archive::safe_relative(&entry.name().map(|n| n.into_owned()).unwrap_or_default()), entry.last_modified()) else { continue };
        let naive = chrono::NaiveDate::from_ymd_opt(t.year() as i32, t.month() as u32, t.day() as u32).and_then(|d| d.and_hms_opt(t.hour() as u32, t.minute() as u32, t.second() as u32));
        let Some(local) = naive.and_then(|n| chrono::Local.from_local_datetime(&n).earliest()) else { continue };
        let _ = filetime::set_file_mtime(into.join(rel), filetime::FileTime::from_unix_time(local.timestamp(), 0));
    }
    Ok(true)
}

// ---- Duplicates ----------------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct DupOpts {
    /// "content" (same bytes), "nameSize" (same name and size) or "size"
    pub by: String,
    pub min_size: u64,
    pub include_hidden: bool,
}
impl Default for DupOpts {
    fn default() -> Self {
        Self { by: "content".into(), min_size: 1, include_hidden: true }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupFile {
    pub path: String,
    pub modified: i64,
}
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    pub size: u64,
    pub files: Vec<DupFile>,
}

fn walk_files(roots: &[PathBuf], include_hidden: bool, cancel: &AtomicBool) -> Res<Vec<(PathBuf, u64, i64)>> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = roots.to_vec();
    while let Some(dir) = stack.pop() {
        stopped(cancel)?;
        let Ok(rd) = fs::read_dir(&dir) else {
            if dir.is_file() {
                if let Ok(m) = fs::metadata(&dir) {
                    out.push((dir.clone(), m.len(), mtime_ms(&m)));
                }
            }
            continue;
        };
        for e in rd.flatten() {
            if !include_hidden && e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(m) = fs::symlink_metadata(e.path()) else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else if m.is_file() {
                out.push((e.path(), m.len(), mtime_ms(&m)));
            }
        }
        if out.len() > 2_000_000 {
            break;
        }
    }
    Ok(out)
}

fn hash_part(path: &Path, limit: Option<usize>) -> Option<[u8; 32]> {
    let mut f = File::open(path).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 18];
    let mut left = limit.unwrap_or(usize::MAX);
    while left > 0 {
        let want = buf.len().min(left);
        let n = f.read(&mut buf[..want]).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        left -= n;
    }
    Some(h.finalize().into())
}

pub fn find_duplicates(roots: &[PathBuf], o: &DupOpts, cancel: &AtomicBool, progress: Progress) -> Res<Vec<DupGroup>> {
    let files = walk_files(roots, o.include_hidden, cancel)?;
    let mut by_size: HashMap<(u64, String), Vec<(PathBuf, i64)>> = HashMap::new();
    for (p, size, m) in files {
        if size < o.min_size {
            continue;
        }
        let name = if o.by == "nameSize" { file_name(&p).to_lowercase() } else { String::new() };
        by_size.entry((size, name)).or_default().push((p, m));
    }
    let mut groups: Vec<((u64, String), Vec<(PathBuf, i64)>)> = by_size.into_iter().filter(|(_, v)| v.len() > 1).collect();
    let total: u64 = groups.iter().map(|(_, v)| v.len() as u64).sum();
    let done = std::sync::atomic::AtomicU64::new(0);
    let mut out = Vec::new();
    if o.by == "content" {
        for ((size, _), members) in groups.drain(..) {
            stopped(cancel)?;
            // The first 64 KB tells most different files apart cheaply; only the rest are read in full.
            let heads: Vec<(Option<[u8; 32]>, (PathBuf, i64))> = members.into_par_iter().map(|m| (hash_part(&m.0, Some(64 * 1024)), m)).collect();
            let mut by_head: HashMap<[u8; 32], Vec<(PathBuf, i64)>> = HashMap::new();
            for (h, m) in heads {
                if let Some(h) = h {
                    by_head.entry(h).or_default().push(m);
                }
            }
            for (_, same_head) in by_head.into_iter().filter(|(_, v)| v.len() > 1) {
                let full: Vec<(Option<[u8; 32]>, (PathBuf, i64))> = same_head
                    .into_par_iter()
                    .map(|m| {
                        let h = if cancel.load(Ordering::Relaxed) { None } else { hash_part(&m.0, None) };
                        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                        progress(d, total, &file_name(&m.0));
                        (h, m)
                    })
                    .collect();
                stopped(cancel)?;
                let mut by_full: HashMap<[u8; 32], Vec<(PathBuf, i64)>> = HashMap::new();
                for (h, m) in full {
                    if let Some(h) = h {
                        by_full.entry(h).or_default().push(m);
                    }
                }
                for (_, v) in by_full.into_iter().filter(|(_, v)| v.len() > 1) {
                    out.push(DupGroup { size, files: v.into_iter().map(|(p, m)| DupFile { path: p.to_string_lossy().into_owned(), modified: m }).collect() });
                }
            }
        }
    } else {
        for ((size, _), v) in groups {
            out.push(DupGroup { size, files: v.into_iter().map(|(p, m)| DupFile { path: p.to_string_lossy().into_owned(), modified: m }).collect() });
        }
    }
    for g in &mut out {
        g.files.sort_by(|a, b| a.path.cmp(&b.path));
    }
    // The groups that waste the most come first.
    out.sort_by(|a, b| (b.size * (b.files.len() as u64 - 1)).cmp(&(a.size * (a.files.len() as u64 - 1))).then(a.files[0].path.cmp(&b.files[0].path)));
    out.truncate(5000);
    Ok(out)
}

// ---- Search inside files -------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct GrepOpts {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    /// File name patterns such as `*.txt;*.md`; empty means every file.
    pub masks: String,
    pub include_archives: bool,
    pub include_hidden: bool,
    pub include_binary: bool,
    pub max_file_size: u64,
    pub max_hits: usize,
}
impl Default for GrepOpts {
    fn default() -> Self {
        Self { pattern: String::new(), regex: false, case_sensitive: false, whole_word: false, masks: String::new(), include_archives: true, include_hidden: true, include_binary: false, max_file_size: 50 * 1024 * 1024, max_hits: 5000 }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GrepHit {
    pub path: String,
    /// The name inside the archive, when the hit is in one.
    pub entry: Option<String>,
    pub line: usize,
    pub text: String,
}

#[derive(Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct GrepSummary {
    pub files: u64,
    pub hits: usize,
    pub truncated: bool,
}

fn decode_text(bytes: &[u8]) -> Option<String> {
    if bytes.starts_with(b"\xFF\xFE") || bytes.starts_with(b"\xFE\xFF") {
        let be = bytes[0] == 0xFE;
        let units: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| if be { u16::from_be_bytes([c[0], c[1]]) } else { u16::from_le_bytes([c[0], c[1]]) }).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn scan_text(text: &str, re: &regex::Regex, path: &str, entry: Option<&str>, max: usize, out: &mut Vec<GrepHit>) {
    for (i, line) in text.lines().enumerate() {
        if re.is_match(line) {
            let shown: String = line.trim().chars().take(300).collect();
            out.push(GrepHit { path: path.to_string(), entry: entry.map(str::to_string), line: i + 1, text: shown });
            if out.len() >= max {
                return;
            }
        }
    }
}

pub fn build_regex(o: &GrepOpts) -> Res<regex::Regex> {
    if o.pattern.is_empty() {
        return Err("Enter something to search for.".into());
    }
    let body = if o.regex { o.pattern.clone() } else { regex::escape(&o.pattern) };
    let body = if o.whole_word { format!(r"\b(?:{body})\b") } else { body };
    regex::RegexBuilder::new(&body).case_insensitive(!o.case_sensitive).size_limit(10 << 20).build().map_err(|e| format!("The pattern is not valid: {e}"))
}

fn grep_one(path: &Path, re: &regex::Regex, o: &GrepOpts) -> Vec<GrepHit> {
    let mut out = Vec::new();
    let p = path.to_string_lossy().into_owned();
    let is_zip = o.include_archives && matches!(path.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("zip" | "jar" | "apk" | "epub" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp"));
    if is_zip {
        if let Ok(f) = File::open(path) {
            if let Ok(mut z) = zip::ZipArchive::new(f) {
                for i in 0..z.len().min(100_000) {
                    let Ok(mut e) = z.by_index(i) else { continue };
                    if e.is_dir() || e.size() > o.max_file_size.min(20 * 1024 * 1024) {
                        continue;
                    }
                    let name = e.name().map(|n| n.into_owned()).unwrap_or_default();
                    if !wild_any(&o.masks, name.rsplit('/').next().unwrap_or(&name)) && !o.masks.is_empty() {
                        continue;
                    }
                    let mut buf = Vec::with_capacity(e.size() as usize);
                    if e.read_to_end(&mut buf).is_err() || (!o.include_binary && buf[..buf.len().min(8192)].contains(&0) && !buf.starts_with(b"\xFF\xFE")) {
                        continue;
                    }
                    if let Some(t) = decode_text(&buf) {
                        scan_text(&t, re, &p, Some(&name), o.max_hits, &mut out);
                    }
                    if out.len() >= o.max_hits {
                        break;
                    }
                }
            }
        }
        return out;
    }
    let Ok(mut f) = File::open(path) else { return out };
    let mut buf = Vec::new();
    if Read::by_ref(&mut f).take(o.max_file_size).read_to_end(&mut buf).is_err() {
        return out;
    }
    if !o.include_binary && buf[..buf.len().min(8192)].contains(&0) && !buf.starts_with(b"\xFF\xFE") && !buf.starts_with(b"\xFE\xFF") {
        return out;
    }
    if let Some(t) = decode_text(&buf) {
        scan_text(&t, re, &p, None, o.max_hits, &mut out);
    }
    out
}

/// Search the contents of the files under `roots`. Hits are handed over in batches as they come.
pub fn grep(roots: &[PathBuf], o: &GrepOpts, cancel: &AtomicBool, on_hits: &mut dyn FnMut(Vec<GrepHit>), progress: Progress) -> Res<GrepSummary> {
    let re = build_regex(o)?;
    let all = walk_files(roots, o.include_hidden, cancel)?;
    let files: Vec<PathBuf> = all
        .into_iter()
        .filter(|(p, size, _)| {
            let n = file_name(p);
            let zipish = o.include_archives && n.rsplit('.').next().is_some_and(|e| matches!(e.to_lowercase().as_str(), "zip" | "jar" | "apk" | "epub" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp"));
            (wild_any(&o.masks, &n) || zipish) && (*size <= o.max_file_size || zipish)
        })
        .map(|(p, _, _)| p)
        .collect();
    let total = files.len() as u64;
    let mut summary = GrepSummary::default();
    for chunk in files.chunks(128) {
        stopped(cancel)?;
        let hits: Vec<Vec<GrepHit>> = chunk.par_iter().map(|p| grep_one(p, &re, o)).collect();
        summary.files += chunk.len() as u64;
        let mut batch: Vec<GrepHit> = hits.into_iter().flatten().collect();
        if summary.hits + batch.len() >= o.max_hits {
            batch.truncate(o.max_hits - summary.hits);
            summary.truncated = true;
        }
        summary.hits += batch.len();
        if !batch.is_empty() {
            on_hits(batch);
        }
        progress(summary.files, total, chunk.last().map(|p| file_name(p)).as_deref().unwrap_or(""));
        if summary.truncated {
            break;
        }
    }
    Ok(summary)
}

// ---- Convert pictures ----------------------------------------------------------------------

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase", default)]
pub struct ConvertOpts {
    /// "jpeg", "png", "webp", "bmp" or "gif"
    pub format: String,
    pub quality: u8,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    /// Scale to this percent of the original size (used when no maximum size is given).
    pub percent: Option<u32>,
    /// Where the copies go; the picture's own folder when empty.
    pub out_dir: Option<PathBuf>,
    /// Added to the name, before the extension.
    pub suffix: String,
    pub overwrite: bool,
}
impl Default for ConvertOpts {
    fn default() -> Self {
        Self { format: "jpeg".into(), quality: 85, max_width: None, max_height: None, percent: None, out_dir: None, suffix: String::new(), overwrite: false }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConvertResult {
    pub path: String,
    pub out: Option<String>,
    pub error: Option<String>,
}

fn convert_one(src: &Path, o: &ConvertOpts) -> Res<PathBuf> {
    use image::{codecs, ImageEncoder, ImageFormat};
    let mut img = image::open(src).map_err(|e| format!("Cannot read the picture: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let target = match (o.max_width, o.max_height, o.percent) {
        (None, None, Some(p)) if p > 0 && p != 100 => Some(((w as u64 * p as u64 / 100).max(1) as u32, (h as u64 * p as u64 / 100).max(1) as u32)),
        (mw, mh, _) if mw.is_some() || mh.is_some() => {
            let (bw, bh) = (mw.unwrap_or(u32::MAX), mh.unwrap_or(u32::MAX));
            (w > bw || h > bh).then_some((bw, bh))
        }
        _ => None,
    };
    if let Some((bw, bh)) = target {
        img = img.resize(bw, bh, image::imageops::FilterType::Lanczos3);
    }
    let (ext, fmt) = match o.format.as_str() {
        "jpeg" | "jpg" => ("jpg", ImageFormat::Jpeg),
        "png" => ("png", ImageFormat::Png),
        "webp" => ("webp", ImageFormat::WebP),
        "bmp" => ("bmp", ImageFormat::Bmp),
        "gif" => ("gif", ImageFormat::Gif),
        other => return Err(format!("Unknown format {other}.")),
    };
    let dir = o.out_dir.clone().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| src.parent().unwrap_or(Path::new(".")).to_path_buf());
    fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "picture".into());
    let mut out = dir.join(format!("{stem}{}.{ext}", o.suffix));
    if out == src {
        return Err("The copy would replace the original; set a suffix or another folder.".into());
    }
    if out.exists() && !o.overwrite {
        let mut n = 2;
        while out.exists() {
            out = dir.join(format!("{stem}{} ({n}).{ext}", o.suffix));
            n += 1;
        }
    }
    let file = BufWriter::new(File::create(&out).map_err(|e| io(&out, e))?);
    let res = match fmt {
        ImageFormat::Jpeg => {
            let rgb = img.to_rgb8();
            codecs::jpeg::JpegEncoder::new_with_quality(file, o.quality.clamp(1, 100)).write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)
        }
        _ => {
            drop(file);
            let r = img.save_with_format(&out, fmt);
            r
        }
    };
    if let Err(e) = res {
        let _ = fs::remove_file(&out);
        return Err(format!("Cannot write the picture: {e}"));
    }
    Ok(out)
}

pub fn convert_images(paths: &[PathBuf], o: &ConvertOpts, cancel: &AtomicBool, progress: Progress) -> Res<Vec<ConvertResult>> {
    let done = std::sync::atomic::AtomicU64::new(0);
    let total = paths.len() as u64;
    let results: Vec<ConvertResult> = paths
        .par_iter()
        .map(|p| {
            let r = if cancel.load(Ordering::Relaxed) { Err("cancelled".to_string()) } else { convert_one(p, o) };
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            progress(d, total, &file_name(p));
            match r {
                Ok(out) => ConvertResult { path: p.to_string_lossy().into_owned(), out: Some(out.to_string_lossy().into_owned()), error: None },
                Err(e) => ConvertResult { path: p.to_string_lossy().into_owned(), out: None, error: Some(e) },
            }
        })
        .collect();
    Ok(results)
}

// ---- Viewer reads --------------------------------------------------------------------------

/// Up to `len` bytes (at most 4 MB) starting at `offset`.
pub fn view_read(path: &Path, offset: u64, len: usize) -> Res<Vec<u8>> {
    let mut f = File::open(path).map_err(|e| io(path, e))?;
    f.seek(SeekFrom::Start(offset)).map_err(|e| io(path, e))?;
    let mut buf = vec![0u8; len.min(4 << 20)];
    let n = read_full(&mut f, &mut buf).map_err(|e| io(path, e))?;
    buf.truncate(n);
    Ok(buf)
}

/// Find `needle` (bytes, as they are written in the encoding being viewed) from `start`, forwards or backwards.
/// With `ascii_ci`, A–Z and a–z match each other.
pub fn view_find(path: &Path, needle: &[u8], start: u64, backward: bool, ascii_ci: bool, cancel: &AtomicBool) -> Res<Option<u64>> {
    if needle.is_empty() {
        return Ok(None);
    }
    let norm = |b: &[u8]| if ascii_ci { b.to_ascii_lowercase() } else { b.to_vec() };
    let needle = norm(needle);
    let mut f = File::open(path).map_err(|e| io(path, e))?;
    let size = f.metadata().map_err(|e| io(path, e))?.len();
    const CH: u64 = 1 << 20;
    let overlap = needle.len() as u64 - 1;
    if !backward {
        let mut pos = start.min(size);
        while pos < size {
            stopped(cancel)?;
            f.seek(SeekFrom::Start(pos)).map_err(|e| io(path, e))?;
            let mut buf = vec![0u8; (CH + overlap) as usize];
            let n = read_full(&mut f, &mut buf).map_err(|e| io(path, e))?;
            let hay = norm(&buf[..n]);
            if let Some(p) = memchr::memmem::find(&hay, &needle) {
                return Ok(Some(pos + p as u64));
            }
            if (n as u64) <= overlap {
                break;
            }
            pos += CH;
        }
        Ok(None)
    } else {
        // Matches that start before `start`.
        let mut end = start.min(size);
        while end > 0 {
            stopped(cancel)?;
            let from = end.saturating_sub(CH);
            f.seek(SeekFrom::Start(from)).map_err(|e| io(path, e))?;
            let mut buf = vec![0u8; (end - from + overlap).min(size - from) as usize];
            let n = read_full(&mut f, &mut buf).map_err(|e| io(path, e))?;
            let hay = norm(&buf[..n]);
            let mut best = None;
            let mut at = 0;
            while let Some(p) = memchr::memmem::find(&hay[at..], &needle) {
                let abs = from + (at + p) as u64;
                if abs < end {
                    best = Some(abs);
                }
                at += p + 1;
            }
            if best.is_some() {
                return Ok(best);
            }
            end = from;
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_zip_is_compared_like_a_folder_with_the_dates_it_holds() {
        use std::io::Write;
        let base = std::env::temp_dir().join(format!("fast-zipcmp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let left = base.join("left");
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join("same.txt"), "same").unwrap();
        fs::write(left.join("only-left.txt"), "x").unwrap();
        // The zip: same.txt (identical, dated 2001), only-zip.txt.
        let zip_path = base.join("a.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&zip_path).unwrap());
            let o = zip::write::SimpleFileOptions::default().last_modified_time(zip::DateTime::from_date_and_time(2001, 2, 3, 4, 5, 6).unwrap());
            z.start_file("same.txt", o).unwrap();
            z.write_all(b"same").unwrap();
            z.start_file("sub/only-zip.txt", o).unwrap();
            z.write_all(b"zip").unwrap();
            z.finish().unwrap();
        }
        let cancel = AtomicBool::new(false);
        let staged = base.join("staged");
        assert!(stage_archive(&zip_path, &staged, &cancel, &|_, _, _| {}).unwrap());
        let age = fs::metadata(staged.join("same.txt")).unwrap().modified().unwrap();
        let year: chrono::DateTime<chrono::Local> = age.into();
        assert_eq!(chrono::Datelike::year(&year), 2001);
        let r = compare_dirs(&left, &staged, &DirOpts { recursive: true, by_content: true, include_hidden: false }, &cancel, &|_, _, _| {}).unwrap();
        let rels: Vec<(&str, &str)> = r.entries.iter().map(|e| (e.rel.as_str(), e.status.as_str())).collect();
        assert!(rels.contains(&("only-left.txt", "leftOnly")), "{rels:?}");
        assert!(rels.contains(&("sub/only-zip.txt", "rightOnly")) || rels.contains(&("sub\\only-zip.txt", "rightOnly")), "{rels:?}");
        assert!(!rels.iter().any(|(n, _)| *n == "same.txt"), "identical content is the same: {rels:?}");
        let _ = fs::remove_dir_all(&base);
    }

    use super::*;

    fn no() -> AtomicBool {
        AtomicBool::new(false)
    }
    fn quiet(_: u64, _: u64, _: &str) {}

    #[test]
    fn wildcards() {
        assert!(wild("*.txt", "Notes.TXT"));
        assert!(wild("a?c*", "abcdef"));
        assert!(!wild("*.txt", "notes.txt.bak"));
        assert!(wild("*", ""));
        assert!(wild_any("*.md; *.txt", "x.txt"));
        assert!(wild_any("", "anything"));
        assert!(!wild_any("*.md|*.rs", "x.txt"));
    }

    #[test]
    fn split_and_join_round_trip_with_check() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("big.bin");
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        fs::write(&src, &data).unwrap();
        let parts = split_file(&src, &d.path().join("parts"), 3000, &no(), &quiet).unwrap();
        assert_eq!(parts.len(), 4);
        assert_eq!(fs::metadata(&parts[3]).unwrap().len(), 1000);
        assert!(d.path().join("parts/big.bin.crc").is_file());
        let (out, ok) = join_files(&parts[0], &d.path().join("joined"), &no(), &quiet).unwrap();
        assert_eq!(fs::read(&out).unwrap(), data);
        assert_eq!(ok, Some(true));
        // A damaged part is noticed.
        fs::write(&parts[1], vec![0u8; 3000]).unwrap();
        let (_, ok) = join_files(&parts[0], &d.path().join("joined2"), &no(), &quiet).unwrap();
        assert_eq!(ok, Some(false));
        assert!(join_files(&src, d.path(), &no(), &quiet).is_err());
    }

    #[test]
    fn uue_xxe_and_mime_round_trip() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("data.bin");
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 256) as u8).chain(*b"\0\0end").collect();
        fs::write(&src, &data).unwrap();
        for (kind, ext) in [("uue", "uue"), ("xxe", "xxe"), ("base64", "b64")] {
            let enc = encode_file(&src, &d.path().join(kind), kind).unwrap();
            assert!(enc.to_string_lossy().ends_with(ext));
            let out = decode_file(&enc, &d.path().join(format!("out-{kind}"))).unwrap();
            assert_eq!(out.len(), 1, "{kind}");
            assert_eq!(fs::read(&out[0]).unwrap(), data, "{kind}");
            assert_eq!(file_name(&out[0]), "data.bin");
        }
        let text = fs::read_to_string(d.path().join("uue/data.bin.uue")).unwrap();
        assert!(text.starts_with("begin 644 data.bin\n") && text.trim_end().ends_with("end"));
        // The classic example from the uuencode manual page.
        fs::write(d.path().join("cat.uue"), "begin 644 cat.txt\n#0V%T\n`\nend\n").unwrap();
        let out = decode_file(&d.path().join("cat.uue"), d.path()).unwrap();
        assert_eq!(fs::read(&out[0]).unwrap(), b"Cat");
        fs::write(d.path().join("junk.txt"), "hello world").unwrap();
        assert!(decode_file(&d.path().join("junk.txt"), d.path()).is_err());
    }

    #[test]
    fn mime_with_several_parts_and_hostile_names() {
        let d = tempfile::tempdir().unwrap();
        let mail = "From: a\nContent-Type: multipart/mixed; boundary=\"XX\"\n\n--XX\nContent-Type: text/plain\n\nhello\n--XX\nContent-Type: application/octet-stream; name=\"../../evil.txt\"\nContent-Transfer-Encoding: base64\n\naGVsbG8gdGhlcmU=\n--XX--\n";
        fs::write(d.path().join("m.eml"), mail).unwrap();
        let out = decode_file(&d.path().join("m.eml"), &d.path().join("o")).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0], d.path().join("o/evil.txt"), "only the file name is used, never a path");
        assert_eq!(fs::read(&out[0]).unwrap(), b"hello there");
    }

    #[test]
    fn compares_text_with_aligned_rows_and_binary_by_offset() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a.txt"), d.path().join("b.txt"));
        fs::write(&a, "one\ntwo\nthree\nfour\n").unwrap();
        fs::write(&b, "one\n2\nthree\nfour\nfive\n").unwrap();
        let FileCompare::Text { rows, changed, added, removed, .. } = compare_files(&a, &b, &CompareOpts::default(), &no()).unwrap() else { panic!("text") };
        assert_eq!((changed, added, removed), (1, 1, 0));
        assert_eq!(rows[1].kind, "changed");
        assert_eq!((rows[1].a.as_str(), rows[1].b.as_str()), ("two", "2"));
        assert_eq!(rows.last().unwrap().kind, "added");
        fs::write(&b, "ONE\n  two  \nthree\nfour\n").unwrap();
        assert!(matches!(compare_files(&a, &b, &CompareOpts { ignore_whitespace: true, ignore_case: true }, &no()).unwrap(), FileCompare::Identical { .. }));
        assert!(matches!(compare_files(&a, &a, &CompareOpts::default(), &no()).unwrap(), FileCompare::Identical { .. }));
        let (x, y) = (d.path().join("x.bin"), d.path().join("y.bin"));
        fs::write(&x, [1u8, 2, 0, 4]).unwrap();
        fs::write(&y, [1u8, 2, 0, 9, 9]).unwrap();
        let FileCompare::Binary { first_diff, a_size, b_size } = compare_files(&x, &y, &CompareOpts::default(), &no()).unwrap() else { panic!("binary") };
        assert_eq!((first_diff, a_size, b_size), (Some(3), 4, 5));
    }

    fn put(p: &Path, s: &str, secs_ago: u64) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
        let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago);
        filetime::set_file_mtime(p, filetime::FileTime::from_system_time(t)).unwrap();
    }

    #[test]
    fn folder_comparison_says_who_has_what() {
        let d = tempfile::tempdir().unwrap();
        let (l, r) = (d.path().join("l"), d.path().join("r"));
        put(&l.join("same.txt"), "x", 5000);
        put(&r.join("same.txt"), "x", 5000);
        put(&l.join("only-left.txt"), "a", 100);
        put(&r.join("only-right/deep.txt"), "b", 100);
        put(&l.join("newer-left.txt"), "new!", 10);
        put(&r.join("newer-left.txt"), "old", 9000);
        put(&l.join("sub/newer-right.txt"), "old", 9000);
        put(&r.join("sub/newer-right.txt"), "new!", 10);
        put(&l.join("pair.txt"), "aaaa", 777);
        put(&r.join("pair.txt"), "bbbb", 777);
        let c = compare_dirs(&l, &r, &DirOpts::default(), &no(), &quiet).unwrap();
        let st = |rel: &str| c.entries.iter().find(|e| e.rel == rel).map(|e| e.status.as_str());
        assert_eq!(st("only-left.txt"), Some("leftOnly"));
        assert_eq!(st("only-right"), Some("rightOnly"));
        assert_eq!(st("only-right/deep.txt"), Some("rightOnly"));
        assert_eq!(st("newer-left.txt"), Some("leftNewer"));
        assert_eq!(st("sub/newer-right.txt"), Some("rightNewer"));
        assert_eq!(st("same.txt"), None);
        assert_eq!(st("pair.txt"), None, "same size and date: taken as the same by default");
        assert_eq!(c.same, 2);
        let c = compare_dirs(&l, &r, &DirOpts { by_content: true, ..DirOpts::default() }, &no(), &quiet).unwrap();
        assert_eq!(c.entries.iter().find(|e| e.rel == "pair.txt").map(|e| e.status.as_str()), Some("different"));
        let flat = compare_dirs(&l, &r, &DirOpts { recursive: false, ..DirOpts::default() }, &no(), &quiet).unwrap();
        assert!(flat.entries.iter().all(|e| !e.rel.contains('/')));
    }

    #[test]
    fn duplicates_are_found_by_content_not_by_name() {
        let d = tempfile::tempdir().unwrap();
        put(&d.path().join("a/photo.jpg"), "same bytes here", 10);
        put(&d.path().join("b/copy of photo.jpg"), "same bytes here", 5);
        put(&d.path().join("b/other.jpg"), "same size!!!!!!", 5);
        put(&d.path().join("c/unique.txt"), "nothing alike but longer", 5);
        put(&d.path().join("c/empty"), "", 5);
        put(&d.path().join("c/empty2"), "", 5);
        let g = find_duplicates(&[d.path().to_path_buf()], &DupOpts::default(), &no(), &quiet).unwrap();
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].files.len(), 2);
        assert!(g[0].files.iter().all(|f| f.path.contains("photo.jpg")));
        let by_size = find_duplicates(&[d.path().to_path_buf()], &DupOpts { by: "size".into(), ..DupOpts::default() }, &no(), &quiet).unwrap();
        assert_eq!(by_size[0].files.len(), 3, "photo, copy and other.jpg are all 15 bytes");
        let with_empty = find_duplicates(&[d.path().to_path_buf()], &DupOpts { min_size: 0, ..DupOpts::default() }, &no(), &quiet).unwrap();
        assert_eq!(with_empty.len(), 2);
        let c = AtomicBool::new(true);
        assert!(find_duplicates(&[d.path().to_path_buf()], &DupOpts::default(), &c, &quiet).is_err());
    }

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (n, c) in files {
            z.start_file(*n, opts).unwrap();
            z.write_all(c).unwrap();
        }
        z.finish().unwrap();
    }

    #[test]
    fn content_search_finds_lines_in_files_and_zips() {
        let d = tempfile::tempdir().unwrap();
        put(&d.path().join("a.txt"), "first\nThe Needle is here\nlast", 1);
        put(&d.path().join("b.log"), "needle but a log", 1);
        put(&d.path().join("c.txt"), "nothing", 1);
        fs::write(d.path().join("bin.dat"), b"needle\0\0binary").unwrap();
        make_zip(&d.path().join("pack.zip"), &[("inside/notes.md", b"a line\nanother NEEDLE line\n")]);
        let roots = [d.path().to_path_buf()];
        let run = |o: GrepOpts| {
            let mut hits = Vec::new();
            let s = grep(&roots, &o, &no(), &mut |h| hits.extend(h), &quiet).unwrap();
            (hits, s)
        };
        let (hits, s) = run(GrepOpts { pattern: "needle".into(), ..GrepOpts::default() });
        let at = |p: &str| hits.iter().filter(|h| h.path.ends_with(p)).collect::<Vec<_>>();
        assert_eq!(at("a.txt")[0].line, 2);
        assert_eq!(at("b.log").len(), 1);
        assert_eq!(at("pack.zip")[0].entry.as_deref(), Some("inside/notes.md"));
        assert_eq!(at("pack.zip")[0].line, 2);
        assert!(at("bin.dat").is_empty(), "binary files are skipped");
        assert_eq!(s.hits, 3);
        let (hits, _) = run(GrepOpts { pattern: "needle".into(), masks: "*.txt".into(), include_archives: false, ..GrepOpts::default() });
        assert_eq!(hits.len(), 1);
        let (hits, _) = run(GrepOpts { pattern: "Needle".into(), case_sensitive: true, include_archives: false, ..GrepOpts::default() });
        assert_eq!(hits.len(), 1);
        let (hits, _) = run(GrepOpts { pattern: r"^n\w+dle".into(), regex: true, include_archives: false, ..GrepOpts::default() });
        assert_eq!(hits.len(), 1);
        let (hits, _) = run(GrepOpts { pattern: "need".into(), whole_word: true, ..GrepOpts::default() });
        assert!(hits.is_empty());
        let (_, s) = run(GrepOpts { pattern: "needle".into(), max_hits: 1, ..GrepOpts::default() });
        assert!(s.truncated && s.hits == 1);
        assert!(build_regex(&GrepOpts { pattern: "(".into(), regex: true, ..GrepOpts::default() }).is_err());
        assert!(build_regex(&GrepOpts::default()).is_err());
    }

    #[test]
    fn pictures_are_converted_and_resized() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("pic.png");
        image::RgbaImage::from_fn(200, 100, |x, y| image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])).save(&src).unwrap();
        let o = ConvertOpts { format: "jpeg".into(), max_width: Some(100), suffix: "-small".into(), ..ConvertOpts::default() };
        let r = convert_images(&[src.clone(), d.path().join("missing.png")], &o, &no(), &quiet).unwrap();
        let out = PathBuf::from(r[0].out.clone().unwrap());
        assert!(out.ends_with("pic-small.jpg"));
        let img = image::open(&out).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        assert!(r[1].error.is_some());
        // The same again does not overwrite.
        let r = convert_images(&[src.clone()], &o, &no(), &quiet).unwrap();
        assert!(r[0].out.as_ref().unwrap().ends_with("pic-small (2).jpg"));
        let half = convert_images(&[src.clone()], &ConvertOpts { format: "bmp".into(), percent: Some(50), out_dir: Some(d.path().join("half")), ..ConvertOpts::default() }, &no(), &quiet).unwrap();
        let img = image::open(half[0].out.as_ref().unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        // Converting a picture to its own format in its own folder with no suffix would replace it.
        let same = convert_images(&[src.clone()], &ConvertOpts { format: "png".into(), ..ConvertOpts::default() }, &no(), &quiet).unwrap();
        assert!(same[0].error.is_some());
        let webp = convert_images(&[src], &ConvertOpts { format: "webp".into(), ..ConvertOpts::default() }, &no(), &quiet).unwrap();
        assert!(webp[0].error.is_none(), "{:?}", webp[0].error);
    }

    #[test]
    fn viewer_reads_and_finds_both_ways() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("big.txt");
        let mut data = vec![b'a'; 3 * 1024 * 1024];
        data[1_500_000..1_500_006].copy_from_slice(b"Needle");
        data[2_900_000..2_900_006].copy_from_slice(b"needle");
        fs::write(&f, &data).unwrap();
        assert_eq!(view_read(&f, 1_500_000, 6).unwrap(), b"Needle");
        assert_eq!(view_read(&f, 3 * 1024 * 1024 - 2, 100).unwrap().len(), 2);
        assert_eq!(view_find(&f, b"needle", 0, false, false, &no()).unwrap(), Some(2_900_000));
        assert_eq!(view_find(&f, b"needle", 0, false, true, &no()).unwrap(), Some(1_500_000));
        assert_eq!(view_find(&f, b"needle", 3 * 1024 * 1024, true, true, &no()).unwrap(), Some(2_900_000));
        assert_eq!(view_find(&f, b"needle", 2_900_000, true, true, &no()).unwrap(), Some(1_500_000));
        assert_eq!(view_find(&f, b"needle", 1_500_000, true, true, &no()).unwrap(), None);
        assert_eq!(view_find(&f, b"nope", 0, false, true, &no()).unwrap(), None);
        // A match that straddles the 1 MB reading window.
        let g = d.path().join("edge.bin");
        let mut e = vec![0u8; 2 * 1024 * 1024];
        e[(1 << 20) - 3..(1 << 20) + 3].copy_from_slice(b"abcdef");
        fs::write(&g, &e).unwrap();
        assert_eq!(view_find(&g, b"abcdef", 0, false, false, &no()).unwrap(), Some((1 << 20) - 3));
        assert_eq!(view_find(&g, b"abcdef", 2 << 20, true, false, &no()).unwrap(), Some((1 << 20) - 3));
    }
}
