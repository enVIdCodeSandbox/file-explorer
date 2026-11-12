//! The right-click entries other programs add (Edit with…, Open with, 7-Zip,
//! Print…). Building them can be slow, so callers show their own menu first
//! and merge these in when they arrive.
//!
//! * Windows: the real shell menu (`IContextMenu`), including icons and
//!   submenus, run on one dedicated thread that keeps the last menu alive so
//!   a chosen entry can be invoked.
//! * Linux: "Open with" built from the installed applications' `.desktop`
//!   files.
//! * macOS: "Open with" from the installed applications, and Quick Look.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ShellItem {
    /// Pass back to [`invoke`].
    pub id: u32,
    pub label: String,
    /// Small PNG as a `data:` URL.
    pub icon_url: Option<String>,
    pub disabled: bool,
    pub checked: bool,
    pub children: Vec<ShellItem>,
}

/// Build the menu for these items (all in one folder), or for the folder's
/// own background when `background` is set and `paths` holds that folder.
pub fn build(paths: Vec<PathBuf>, background: bool) -> crate::Result<Vec<ShellItem>> {
    imp::build(paths, background)
}

/// Run an entry from the menu most recently built.
pub fn invoke(id: u32) -> crate::Result<()> {
    imp::invoke(id)
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------
#[cfg(windows)]
mod imp {
    use super::ShellItem;
    use crate::error::CoreError as Error;
    use crate::Result;
    use base64::Engine;
    use crossbeam_channel::{bounded, unbounded, Sender};
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::Duration;
    use windows::core::{Interface, PCSTR, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        IContextMenu, IContextMenu2, IShellFolder, SHBindToParent, SHParseDisplayName, CMF_CANRENAME, CMF_NORMAL,
        CMINVOKECOMMANDINFOEX, CMIC_MASK_PTINVOKE, SEE_MASK_UNICODE, GCS_VERBW,
    };
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreatePopupMenu, DestroyMenu, DispatchMessageW, GetMenuItemCount, GetMenuItemInfoW, PeekMessageW,
        TranslateMessage, HMENU, MENUITEMINFOW, MFS_CHECKED, MFS_DISABLED, MFS_GRAYED, MFT_OWNERDRAW, MFT_SEPARATOR,
        MIIM_BITMAP, MIIM_FTYPE, MIIM_ID, MIIM_STATE, MIIM_STRING, MIIM_SUBMENU, PM_REMOVE, SW_SHOWNORMAL, WM_INITMENUPOPUP,
        MSG,
    };

    const FIRST: u32 = 1;
    const LAST: u32 = 0x7FFF;

    enum Req {
        Build { paths: Vec<PathBuf>, background: bool, reply: Sender<Result<Vec<ShellItem>>> },
        Invoke { id: u32, reply: Sender<Result<()>> },
    }

    /// The menu kept alive for the entry the person may choose.
    struct Active {
        menu: IContextMenu,
        hmenu: HMENU,
        /// Folder whose background the menu is for (the working directory).
        dir: Option<PathBuf>,
    }

    static WORKER: OnceLock<Sender<Req>> = OnceLock::new();

    fn worker() -> &'static Sender<Req> {
        WORKER.get_or_init(|| {
            let (tx, rx) = unbounded::<Req>();
            std::thread::Builder::new()
                .name("shell-menu".into())
                .spawn(move || {
                    // A single-threaded apartment, as shell extensions expect.
                    unsafe {
                        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    }
                    let mut active: Option<Active> = None;
                    loop {
                        match rx.recv_timeout(Duration::from_millis(25)) {
                            Ok(Req::Build { paths, background, reply }) => {
                                release(&mut active);
                                let result = unsafe { build_menu(&paths, background) };
                                let _ = match result {
                                    Ok((items, a)) => {
                                        active = Some(a);
                                        reply.send(Ok(items))
                                    }
                                    Err(e) => reply.send(Err(e)),
                                };
                            }
                            Ok(Req::Invoke { id, reply }) => {
                                let r = match &active {
                                    Some(a) => unsafe { invoke_command(a, id) },
                                    None => Err(Error::Invalid("the menu is no longer open".into())),
                                };
                                let _ = reply.send(r);
                            }
                            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                        }
                        // Some handlers show windows or post messages to this thread.
                        unsafe {
                            let mut msg = MSG::default();
                            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                                let _ = TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                    }
                })
                .expect("start the shell menu thread");
            tx
        })
    }

    fn release(active: &mut Option<Active>) {
        if let Some(a) = active.take() {
            unsafe {
                let _ = DestroyMenu(a.hmenu);
            }
        }
    }

    pub fn build(paths: Vec<PathBuf>, background: bool) -> Result<Vec<ShellItem>> {
        let (reply, rx) = bounded(1);
        worker().send(Req::Build { paths, background, reply }).map_err(|_| Error::Invalid("shell menu unavailable".into()))?;
        // Some extensions take seconds; the caller already shows its own menu.
        rx.recv_timeout(Duration::from_secs(4)).map_err(|_| Error::Invalid("the system menu took too long".into()))?
    }

    pub fn invoke(id: u32) -> Result<()> {
        let (reply, rx) = bounded(1);
        worker().send(Req::Invoke { id, reply }).map_err(|_| Error::Invalid("shell menu unavailable".into()))?;
        rx.recv_timeout(Duration::from_secs(30)).map_err(|_| Error::Invalid("the command did not respond".into()))?
    }

    fn wide(p: &Path) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    fn pidl_of(path: &Path) -> Result<*mut ITEMIDLIST> {
        let w = wide(path);
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        unsafe { SHParseDisplayName(PCWSTR(w.as_ptr()), None, &mut pidl, 0, None) }
            .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
        Ok(pidl)
    }

    unsafe fn build_menu(paths: &[PathBuf], background: bool) -> Result<(Vec<ShellItem>, Active)> {
        let first = paths.first().ok_or_else(|| Error::Invalid("nothing selected".into()))?;
        let mut owned: Vec<*mut ITEMIDLIST> = Vec::new();
        let result = (|| -> Result<(IContextMenu, Option<PathBuf>)> {
            if background {
                let pidl = pidl_of(first)?;
                owned.push(pidl);
                let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
                let parent: IShellFolder = SHBindToParent(pidl, Some(&mut child)).map_err(win_err)?;
                let folder: IShellFolder = parent.BindToObject(child, None).map_err(win_err)?;
                let menu: IContextMenu = folder.CreateViewObject(HWND::default()).map_err(win_err)?;
                Ok((menu, Some(first.clone())))
            } else {
                let mut children: Vec<*const ITEMIDLIST> = Vec::new();
                let mut parent: Option<IShellFolder> = None;
                for p in paths {
                    let pidl = pidl_of(p)?;
                    owned.push(pidl);
                    let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
                    let folder: IShellFolder = SHBindToParent(pidl, Some(&mut child)).map_err(win_err)?;
                    children.push(child as *const _);
                    parent.get_or_insert(folder);
                }
                let parent = parent.ok_or_else(|| Error::Invalid("nothing selected".into()))?;
                let menu: IContextMenu = parent.GetUIObjectOf(HWND::default(), &children, None).map_err(win_err)?;
                Ok((menu, None))
            }
        })();
        let (menu, dir) = match result {
            Ok(v) => v,
            Err(e) => {
                for p in owned {
                    CoTaskMemFree(Some(p as *const _));
                }
                return Err(e);
            }
        };

        let hmenu = CreatePopupMenu().map_err(win_err)?;
        let flags = if background { CMF_NORMAL } else { CMF_NORMAL | CMF_CANRENAME };
        menu.QueryContextMenu(hmenu, 0, FIRST, LAST, flags).ok().map_err(win_err)?;
        let menu2: Option<IContextMenu2> = menu.cast().ok();
        let items = read_menu(hmenu, &menu, menu2.as_ref(), background, true);
        // The ITEMIDLISTs must outlive the menu object's use of them only
        // while it is built; the menu keeps its own references.
        for p in owned {
            CoTaskMemFree(Some(p as *const _));
        }
        Ok((items, Active { menu, hmenu, dir }))
    }

    fn win_err(e: windows::core::Error) -> Error {
        Error::Invalid(e.to_string())
    }

    /// Entries the app already draws itself, matched by the command's verb.
    fn own_verb(verb: &str, background: bool) -> bool {
        const OWN: &[&str] = &[
            "cut", "copy", "paste", "pastelink", "delete", "rename", "properties", "link", "undo", "pintohome",
            "pintostartscreen_xxx", "refresh", "view", "sort", "group", "new", "personalize",
        ];
        (!background && verb == "open") || OWN.contains(&verb)
    }

    /// Entries the app already draws itself, matched by their text.
    fn own_label(label: &str) -> bool {
        const OWN: &[&str] = &[
            "open", "cut", "copy", "paste", "paste shortcut", "delete", "rename", "properties", "create shortcut",
            "undo", "refresh", "view", "sort by", "group by", "new", "copy as path", "share", "send to",
            "customize this folder...", "display settings", "personalize", "show more options",
        ];
        OWN.contains(&label.to_lowercase().as_str())
    }

    unsafe fn read_menu(
        hmenu: HMENU,
        menu: &IContextMenu,
        menu2: Option<&IContextMenu2>,
        background: bool,
        top: bool,
    ) -> Vec<ShellItem> {
        let mut out: Vec<ShellItem> = Vec::new();
        let count = GetMenuItemCount(Some(hmenu)).max(0) as u32;
        let mut rule = false;
        for pos in 0..count {
            let mut buf = [0u16; 512];
            let mut info = MENUITEMINFOW {
                cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
                fMask: MIIM_ID | MIIM_FTYPE | MIIM_STATE | MIIM_STRING | MIIM_SUBMENU | MIIM_BITMAP,
                dwTypeData: PWSTR(buf.as_mut_ptr()),
                cch: buf.len() as u32 - 1,
                ..Default::default()
            };
            if GetMenuItemInfoW(hmenu, pos, true, &mut info).is_err() {
                continue;
            }
            if info.fType.0 & MFT_SEPARATOR.0 != 0 {
                rule = true;
                continue;
            }
            if info.fType.0 & MFT_OWNERDRAW.0 != 0 && info.cch == 0 {
                continue;
            }
            let raw = String::from_utf16_lossy(&buf[..info.cch as usize]);
            let label = raw.split('\t').next().unwrap_or("").replace('&', "");
            if label.is_empty() {
                continue;
            }
            let sub = if info.hSubMenu.is_invalid() { None } else { Some(info.hSubMenu) };
            let offset = info.wID.wrapping_sub(FIRST);
            if sub.is_none() && info.wID >= FIRST && info.wID <= LAST {
                if let Some(verb) = verb_of(menu, offset) {
                    if top && own_verb(&verb, background) {
                        continue;
                    }
                }
            }
            if top && own_label(&label) {
                continue;
            }
            let children = match sub {
                Some(h) => {
                    // Lazy submenus (Send to, Open with, 7-Zip) fill in when opened.
                    if let Some(m2) = menu2 {
                        let _ = m2.HandleMenuMsg(WM_INITMENUPOPUP, WPARAM(h.0 as usize), LPARAM(pos as isize));
                    }
                    read_menu(h, menu, menu2, background, false)
                }
                None => Vec::new(),
            };
            if sub.is_some() && children.is_empty() {
                continue;
            }
            out.push(ShellItem {
                id: info.wID,
                label,
                icon_url: icon_of(info.hbmpItem),
                disabled: info.fState.0 & (MFS_DISABLED.0 | MFS_GRAYED.0) != 0,
                checked: info.fState.0 & MFS_CHECKED.0 != 0,
                children,
            });
            // Group breaks are kept as a flag on the next item by the caller;
            // here an item following a rule simply starts a new group.
            let _ = std::mem::take(&mut rule);
        }
        out
    }

    unsafe fn verb_of(menu: &IContextMenu, offset: u32) -> Option<String> {
        let mut buf = [0u16; 128];
        menu.GetCommandString(offset as usize, GCS_VERBW, None, PSTR_FROM(buf.as_mut_ptr()), buf.len() as u32)
            .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]).to_lowercase())
    }

    #[allow(non_snake_case)]
    fn PSTR_FROM(p: *mut u16) -> windows::core::PSTR {
        windows::core::PSTR(p as *mut u8)
    }

    /// A menu item's picture as a PNG data URL.
    unsafe fn icon_of(h: HBITMAP) -> Option<String> {
        // Small values are system placeholders (HBMMENU_*), not bitmaps.
        if h.is_invalid() || (h.0 as isize) < 12 {
            return None;
        }
        let mut bm = BITMAP::default();
        if GetObjectW(HGDIOBJ(h.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _)) == 0 {
            return None;
        }
        let (w, hgt) = (bm.bmWidth, bm.bmHeight.abs());
        if w <= 0 || hgt <= 0 || w > 128 || hgt > 128 {
            return None;
        }
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -hgt, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w * hgt * 4) as usize];
        let lines = GetDIBits(dc, h, 0, hgt as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);
        if lines == 0 {
            return None;
        }
        // BGRA -> RGBA. Menu bitmaps are premultiplied when they have alpha;
        // those without alpha come back with alpha 0 everywhere.
        let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
        for p in px.chunks_exact_mut(4) {
            let (b, g, r, a) = (p[0], p[1], p[2], p[3]);
            if has_alpha {
                let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255) / a as u32).min(255) as u8 };
                p[0] = un(r);
                p[1] = un(g);
                p[2] = un(b);
            } else {
                p[0] = r;
                p[1] = g;
                p[2] = b;
                p[3] = 255;
            }
        }
        let img = image::RgbaImage::from_raw(w as u32, hgt as u32, px)?;
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
        Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
    }

    unsafe fn invoke_command(active: &Active, id: u32) -> Result<()> {
        let offset = id.wrapping_sub(FIRST) as usize;
        let dir = active.dir.as_ref().map(|d| wide(d));
        let dir_ansi = active.dir.as_ref().map(|d| {
            let mut b = d.to_string_lossy().into_owned().into_bytes();
            b.push(0);
            b
        });
        let info = CMINVOKECOMMANDINFOEX {
            cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32,
            fMask: SEE_MASK_UNICODE | CMIC_MASK_PTINVOKE,
            hwnd: HWND::default(),
            lpVerb: PCSTR(offset as *const u8),
            lpDirectory: dir_ansi.as_ref().map_or(PCSTR::null(), |d| PCSTR(d.as_ptr())),
            nShow: SW_SHOWNORMAL.0,
            lpVerbW: PCWSTR(offset as *const u16),
            lpDirectoryW: dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr())),
            ptInvoke: POINT::default(),
            ..Default::default()
        };
        active
            .menu
            .InvokeCommand(&info as *const _ as *const windows::Win32::UI::Shell::CMINVOKECOMMANDINFO)
            .map_err(win_err)
    }
}

// ---------------------------------------------------------------------------
// Linux and other Unix: "Open with" from the installed applications
// ---------------------------------------------------------------------------
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
mod imp {
    use super::ShellItem;
    use crate::error::CoreError as Error;
    use crate::Result;
    use parking_lot::Mutex;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    /// What each id in the last menu runs.
    static LAST: Mutex<Vec<(String, Vec<PathBuf>)>> = Mutex::new(Vec::new());

    struct App {
        name: String,
        exec: String,
    }

    fn app_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(d) = dirs::data_dir() {
            dirs.push(d.join("applications"));
        }
        let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
        dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(|d| Path::new(d).join("applications")));
        dirs.push("/var/lib/flatpak/exports/share/applications".into());
        dirs
    }

    /// The MIME type of a file, from its name first and `xdg-mime` second.
    fn mime_of(path: &Path) -> Option<String> {
        if path.is_dir() {
            return Some("inode/directory".into());
        }
        if let Some(m) = mime_guess::from_path(path).first() {
            return Some(m.essence_str().to_string());
        }
        let out = Command::new("xdg-mime").args(["query", "filetype"]).arg(path).output().ok()?;
        let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!t.is_empty()).then_some(t)
    }

    fn desktop_ids_for(mime: &str) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for dir in app_dirs() {
            let Ok(text) = std::fs::read_to_string(dir.join("mimeinfo.cache")) else { continue };
            for line in text.lines() {
                if let Some(list) = line.strip_prefix(&format!("{mime}=")) {
                    for id in list.split(';').filter(|s| !s.is_empty()) {
                        if !ids.iter().any(|x| x == id) {
                            ids.push(id.to_string());
                        }
                    }
                }
            }
        }
        ids
    }

    fn read_app(id: &str) -> Option<App> {
        for dir in app_dirs() {
            let Ok(text) = std::fs::read_to_string(dir.join(id)) else { continue };
            let mut in_entry = false;
            let (mut name, mut exec, mut hidden) = (None, None, false);
            for line in text.lines() {
                if line.starts_with('[') {
                    in_entry = line == "[Desktop Entry]";
                } else if in_entry {
                    if let Some(v) = line.strip_prefix("Name=") {
                        name.get_or_insert_with(|| v.to_string());
                    } else if let Some(v) = line.strip_prefix("Exec=") {
                        exec.get_or_insert_with(|| v.to_string());
                    } else if line == "NoDisplay=true" || line == "Hidden=true" {
                        hidden = true;
                    }
                }
            }
            if hidden {
                return None;
            }
            return Some(App { name: name?, exec: exec? });
        }
        None
    }

    /// Split an `Exec=` line into arguments, filling in the file and dropping
    /// the other field codes.
    pub(super) fn command_for(exec: &str, files: &[PathBuf]) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        let mut chars = exec.chars().peekable();
        let mut had = false;
        while let Some(c) = chars.next() {
            match (c, quote) {
                ('"' | '\'', None) => (quote = Some(c), had = true).1,
                (q, Some(open)) if q == open => quote = None,
                ('\\', Some('"')) => {
                    if let Some(n) = chars.next() {
                        cur.push(n);
                    }
                }
                (' ' | '\t', None) => {
                    if had || !cur.is_empty() {
                        args.push(std::mem::take(&mut cur));
                        had = false;
                    }
                }
                _ => cur.push(c),
            }
        }
        if had || !cur.is_empty() {
            args.push(cur);
        }
        let mut out = Vec::new();
        let mut used_file = false;
        for a in args {
            match a.as_str() {
                "%f" | "%u" => {
                    used_file = true;
                    out.extend(files.first().map(|f| f.to_string_lossy().into_owned()));
                }
                "%F" | "%U" => {
                    used_file = true;
                    out.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
                }
                s if s.starts_with('%') && s.len() == 2 => {}
                _ => out.push(a),
            }
        }
        if !used_file {
            out.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
        }
        out
    }

    pub fn build(paths: Vec<PathBuf>, background: bool) -> Result<Vec<ShellItem>> {
        if background || paths.is_empty() {
            return Ok(Vec::new());
        }
        // Offer the apps that handle every selected file's type.
        let mut common: Option<Vec<String>> = None;
        for p in &paths {
            let Some(mime) = mime_of(p) else { return Ok(Vec::new()) };
            let ids = desktop_ids_for(&mime);
            common = Some(match common {
                None => ids,
                Some(c) => c.into_iter().filter(|i| ids.contains(i)).collect(),
            });
        }
        let mut seen: HashMap<String, ()> = HashMap::new();
        let mut apps: Vec<(String, App)> = common
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| read_app(&id).map(|a| (id, a)))
            .filter(|(_, a)| seen.insert(a.name.clone(), ()).is_none())
            .collect();
        apps.truncate(24);
        if apps.is_empty() {
            return Ok(Vec::new());
        }
        let mut last = LAST.lock();
        last.clear();
        let children = apps
            .into_iter()
            .map(|(_, a)| {
                last.push((a.exec, paths.clone()));
                ShellItem { id: last.len() as u32, label: a.name, ..Default::default() }
            })
            .collect();
        Ok(vec![ShellItem { id: 0, label: "Open with".into(), children, ..Default::default() }])
    }

    pub fn invoke(id: u32) -> Result<()> {
        let (exec, files) = LAST.lock().get((id as usize).wrapping_sub(1)).cloned().ok_or_else(|| Error::Invalid("the menu is no longer open".into()))?;
        let args = command_for(&exec, &files);
        let (program, rest) = args.split_first().ok_or_else(|| Error::Invalid("empty command".into()))?;
        Command::new(program)
            .args(rest)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| Error::Invalid(format!("{program}: {e}")))
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ShellItem;
    use crate::error::CoreError as Error;
    use crate::Result;
    use parking_lot::Mutex;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    #[derive(Clone)]
    enum Action {
        OpenWith(String),
        QuickLook,
    }

    struct Last {
        paths: Vec<PathBuf>,
        actions: Vec<Action>,
    }

    static LAST: Mutex<Option<Last>> = Mutex::new(None);

    /// Application names installed in the usual places.
    fn applications() -> Vec<String> {
        let mut dirs = vec![
            PathBuf::from("/Applications"),
            PathBuf::from("/Applications/Utilities"),
            PathBuf::from("/System/Applications"),
        ];
        if let Some(home) = crate::platform::home_dir() {
            dirs.push(home.join("Applications"));
        }
        let mut names: Vec<String> = dirs
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".app").map(str::to_owned))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        names.dedup();
        names
    }

    pub fn build(paths: Vec<PathBuf>, background: bool) -> Result<Vec<ShellItem>> {
        if background || paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut actions = Vec::new();
        let mut items = Vec::new();
        let mut push = |label: String, children: Vec<ShellItem>, a: Option<Action>| {
            let id = actions.len() as u32;
            if let Some(a) = a {
                actions.push(a);
            } else {
                actions.push(Action::QuickLook);
            }
            ShellItem { id, label, children, ..Default::default() }
        };
        let apps: Vec<ShellItem> = applications()
            .into_iter()
            .take(120)
            .map(|name| push(name.clone(), Vec::new(), Some(Action::OpenWith(name))))
            .collect();
        if !apps.is_empty() {
            let mut parent = push("Open with".into(), apps, None);
            parent.disabled = false;
            items.push(parent);
        }
        items.push(push("Quick Look".into(), Vec::new(), Some(Action::QuickLook)));
        *LAST.lock() = Some(Last { paths, actions });
        Ok(items)
    }

    pub fn invoke(id: u32) -> Result<()> {
        let (paths, action) = {
            let guard = LAST.lock();
            let last = guard.as_ref().ok_or_else(|| Error::Invalid("the menu has expired".into()))?;
            let action = last.actions.get(id as usize).cloned().ok_or_else(|| Error::Invalid("unknown menu entry".into()))?;
            (last.paths.clone(), action)
        };
        let mut cmd = match &action {
            Action::OpenWith(app) => {
                let mut c = Command::new("open");
                c.arg("-a").arg(app).args(&paths);
                c
            }
            Action::QuickLook => {
                let mut c = Command::new("qlmanage");
                c.arg("-p").args(&paths);
                c
            }
        };
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.spawn().map(|_| ()).map_err(|e| Error::Invalid(format!("could not run it: {e}")))
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    use super::ShellItem;
    use crate::Result;
    use std::path::PathBuf;

    pub fn build(_paths: Vec<PathBuf>, _background: bool) -> Result<Vec<ShellItem>> {
        Ok(Vec::new())
    }

    pub fn invoke(_id: u32) -> Result<()> {
        Ok(())
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn exec_lines_get_the_file_filled_in() {
        let f = vec![PathBuf::from("/tmp/a b.png")];
        assert_eq!(super::imp::command_for("gimp %U", &f), vec!["gimp", "/tmp/a b.png"]);
        assert_eq!(super::imp::command_for("\"/opt/My App/run\" --new %f %i", &f), vec!["/opt/My App/run", "--new", "/tmp/a b.png"]);
        assert_eq!(super::imp::command_for("viewer", &f), vec!["viewer", "/tmp/a b.png"]);
    }
}
