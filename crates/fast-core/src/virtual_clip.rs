//! Files that another program copied without having them on disk: entries inside a zip,
//! files on a phone, "online only" cloud files. Windows Explorer puts a *description* of
//! them on the clipboard and produces each one's bytes only when asked. The app asks, and
//! writes them into a staging folder, so they can be pasted like any other file.
//!
//! Only Windows has this kind of clipboard; elsewhere there is nothing to read.

use crate::error::{CoreError, Result};
use std::path::{Component, Path, PathBuf};

/// A relative path from another program, made safe to create under a folder: no drive,
/// no `..`, no empty parts. Folder separators of either kind are accepted.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in name.split(['\\', '/']) {
        let part = part.trim_end_matches(['.', ' ']);
        if part.is_empty() || part == "." || part == ".." || part.contains(':') {
            continue;
        }
        let clean: String = part.chars().map(|c| if matches!(c, '<' | '>' | '"' | '|' | '?' | '*' | '\0') { '_' } else { c }).collect();
        out.push(clean);
    }
    (out.components().count() > 0 && out.components().all(|c| matches!(c, Component::Normal(_)))).then_some(out)
}

/// Whether the clipboard holds files that exist only inside the other program.
pub fn has_virtual_files() -> bool {
    imp::has()
}

/// Write them into `into` (created if needed). Returns the top-level items made there.
pub fn extract_virtual_files(into: &Path) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(into).map_err(|e| CoreError::io(into, e))?;
    imp::extract(into)
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn has() -> bool {
        false
    }
    pub fn extract(_: &Path) -> Result<Vec<PathBuf>> {
        Err(CoreError::Unsupported("files copied from inside another program"))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::io::Write;
    use windows::core::w;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, TYMED_HGLOBAL, TYMED_ISTREAM};
    use windows::Win32::System::DataExchange::{IsClipboardFormatAvailable, RegisterClipboardFormatW};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Ole::{OleGetClipboard, OleInitialize, OleUninitialize, ReleaseStgMedium};
    use windows::Win32::UI::Shell::{FILEDESCRIPTORW, FILEGROUPDESCRIPTORW};

    const DIRECTORY: u32 = 0x10;

    fn descriptor_format() -> u32 {
        unsafe { RegisterClipboardFormatW(w!("FileGroupDescriptorW")) }
    }

    pub fn has() -> bool {
        unsafe { IsClipboardFormatAvailable(descriptor_format()).is_ok() }
    }

    fn fail(what: &str, e: windows::core::Error) -> CoreError {
        CoreError::Invalid(format!("{what}: {}", e.message().trim()))
    }

    fn format(cf: u32, index: i32, tymed: i32) -> FORMATETC {
        FORMATETC { cfFormat: cf as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: index, tymed: tymed as u32 }
    }

    pub fn extract(into: &Path) -> Result<Vec<PathBuf>> {
        let into = into.to_path_buf();
        // OLE wants its own apartment, and the clipboard's owner may call back into it.
        std::thread::spawn(move || {
            // SAFETY: plain OLE calls; every medium that Windows hands back is released, and
            // every pointer is read only inside the locked block it points into.
            unsafe {
                let started = OleInitialize(None).is_ok();
                let result = run(&into);
                if started {
                    OleUninitialize();
                }
                result
            }
        })
        .join()
        .unwrap_or_else(|_| Err(CoreError::Invalid("reading the clipboard failed".into())))
    }

    unsafe fn run(into: &Path) -> Result<Vec<PathBuf>> {
        let data: IDataObject = OleGetClipboard().map_err(|e| fail("cannot read the clipboard", e))?;
        let group = descriptors(&data)?;
        let contents = RegisterClipboardFormatW(w!("FileContents"));
        let mut tops: Vec<PathBuf> = Vec::new();
        for (i, (name, attrs)) in group.iter().enumerate() {
            let Some(rel) = safe_relative(name) else { continue };
            let target = into.join(&rel);
            if let Some(first) = rel.components().next() {
                let top = into.join(first.as_os_str());
                if !tops.contains(&top) {
                    tops.push(top);
                }
            }
            if attrs & DIRECTORY != 0 {
                std::fs::create_dir_all(&target).map_err(|e| CoreError::io(&target, e))?;
                continue;
            }
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
            }
            write_item(&data, contents, i as i32, &target)?;
        }
        if tops.is_empty() {
            return Err(CoreError::Invalid("the other program offered no files".into()));
        }
        Ok(tops)
    }

    /// `(relative name, attributes)` of each item in the group descriptor.
    unsafe fn descriptors(data: &IDataObject) -> Result<Vec<(String, u32)>> {
        let fe = format(descriptor_format(), -1, TYMED_HGLOBAL.0);
        let mut medium: STGMEDIUM = data.GetData(&fe).map_err(|e| fail("the other program would not list its files", e))?;
        let h: HGLOBAL = medium.u.hGlobal;
        let base = GlobalLock(h) as *const u8;
        let mut out = Vec::new();
        if !base.is_null() {
            let size = GlobalSize(h);
            let count = std::ptr::read_unaligned(base as *const u32) as usize;
            let stride = std::mem::size_of::<FILEDESCRIPTORW>();
            let first = std::mem::offset_of!(FILEGROUPDESCRIPTORW, fgd);
            for i in 0..count {
                if first + (i + 1) * stride > size {
                    break;
                }
                let d = std::ptr::read_unaligned(base.add(first + i * stride) as *const FILEDESCRIPTORW);
                let name = d.cFileName;
                let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                out.push((String::from_utf16_lossy(&name[..len]), d.dwFileAttributes));
            }
            let _ = GlobalUnlock(h);
        }
        ReleaseStgMedium(&mut medium);
        Ok(out)
    }

    /// Ask the other program for the bytes of item `index` and save them to `target`.
    unsafe fn write_item(data: &IDataObject, contents: u32, index: i32, target: &Path) -> Result<()> {
        let fe = format(contents, index, TYMED_ISTREAM.0 | TYMED_HGLOBAL.0);
        let mut medium: STGMEDIUM = data.GetData(&fe).map_err(|e| fail("the other program would not give a file", e))?;
        let mut file = std::fs::File::create(target).map_err(|e| CoreError::io(target, e))?;
        let result: Result<()> = (|| {
            if medium.tymed == TYMED_ISTREAM.0 as u32 {
                let Some(stream) = (*medium.u.pstm).as_ref() else { return Err(CoreError::Invalid("an empty stream".into())) };
                let mut buf = vec![0u8; 256 * 1024];
                loop {
                    let mut got = 0u32;
                    let hr = stream.Read(buf.as_mut_ptr() as *mut _, buf.len() as u32, Some(&mut got));
                    if hr.is_err() {
                        return Err(fail("reading a file failed", windows::core::Error::from(hr)));
                    }
                    if got == 0 {
                        return Ok(());
                    }
                    file.write_all(&buf[..got as usize]).map_err(|e| CoreError::io(target, e))?;
                }
            } else if medium.tymed == TYMED_HGLOBAL.0 as u32 {
                let h = medium.u.hGlobal;
                let p = GlobalLock(h) as *const u8;
                if !p.is_null() {
                    let n = GlobalSize(h);
                    let r = file.write_all(std::slice::from_raw_parts(p, n)).map_err(|e| CoreError::io(target, e));
                    let _ = GlobalUnlock(h);
                    return r;
                }
                Ok(())
            } else {
                Err(CoreError::Invalid("the other program gave a file in a form the app cannot read".into()))
            }
        })();
        ReleaseStgMedium(&mut medium);
        drop(file);
        if result.is_err() {
            let _ = std::fs::remove_file(target);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_other_programs_stay_inside_the_folder() {
        assert_eq!(safe_relative(r"photos\2024\a.jpg"), Some(PathBuf::from("photos").join("2024").join("a.jpg")));
        assert_eq!(safe_relative("dir/sub/file.txt"), Some(PathBuf::from("dir").join("sub").join("file.txt")));
        assert_eq!(safe_relative(r"..\..\windows\evil.dll"), Some(PathBuf::from("windows").join("evil.dll")));
        assert_eq!(safe_relative(r"C:\Windows\x.txt"), Some(PathBuf::from("Windows").join("x.txt")));
        assert_eq!(safe_relative("a<b>.txt"), Some(PathBuf::from("a_b_.txt")));
        assert_eq!(safe_relative(".."), None);
        assert_eq!(safe_relative(""), None);
    }

    #[test]
    #[cfg(not(windows))]
    fn nothing_virtual_elsewhere() {
        assert!(!has_virtual_files());
        assert!(extract_virtual_files(&std::env::temp_dir().join("fast-virtual-test")).is_err());
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("fast-virtual-test"));
    }
}
