//! Windows: the app becomes the default action for folders and drives.
//!
//! Written under `HKEY_CURRENT_USER\Software\Classes` only, so it applies to the
//! signed-in person and needs no administrator rights:
//!
//! ```text
//! Directory\shell            (Default) = FastFileExplorer
//! Directory\shell\FastFileExplorer            (Default) = Open in FAST File Explorer
//! Directory\shell\FastFileExplorer\command    (Default) = "<exe>" "%1"
//! Drive\shell                ... the same
//! ```
//!
//! Explorer itself keeps running (it is also the taskbar and the desktop), and
//! programs that ask Explorer directly, such as a browser's "Show in folder",
//! still get an Explorer window.

use super::*;
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RRF_RT_REG_SZ,
};
use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

const VERB: &str = "FastFileExplorer";
const LABEL: &str = "Open in FAST File Explorer";
/// The kinds of thing whose default action is taken over: file-system folders, and drives.
const CLASSES: [&str; 2] = ["Directory", "Drive"];

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn shell_key(class: &str) -> String {
    format!("Software\\Classes\\{class}\\shell")
}

fn failed(what: &str, code: u32) -> CoreError {
    CoreError::Invalid(format!("Windows did not accept the change ({what}, error {code})."))
}

/// The default value of a key under the person's own settings, if it has one.
fn read_default(key: &str) -> Option<String> {
    let key = wide(key);
    let mut buf = vec![0u16; 1024];
    let mut bytes = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            std::ptr::null(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let len = (bytes as usize / 2).min(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]);
    let text = text.trim_end_matches('\0').to_string();
    (!text.is_empty()).then_some(text)
}

/// Set a key's default value, making the key when it is missing.
fn write_default(key: &str, value: &str) -> Result<()> {
    let wkey = wide(key);
    let mut handle: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, wkey.as_ptr(), &mut handle) };
    if rc != ERROR_SUCCESS {
        return Err(failed(key, rc));
    }
    let data = wide(value);
    let rc = unsafe { RegSetValueExW(handle, std::ptr::null(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32) };
    unsafe { RegCloseKey(handle) };
    if rc != ERROR_SUCCESS {
        return Err(failed(key, rc));
    }
    Ok(())
}

/// Remove a key's default value (the key itself stays).
fn clear_default(key: &str) -> Result<()> {
    let wkey = wide(key);
    let mut handle: HKEY = std::ptr::null_mut();
    let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wkey.as_ptr(), 0, KEY_READ | KEY_WRITE, &mut handle) };
    if rc == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if rc != ERROR_SUCCESS {
        return Err(failed(key, rc));
    }
    let rc = unsafe { RegDeleteValueW(handle, std::ptr::null()) };
    unsafe { RegCloseKey(handle) };
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        return Err(failed(key, rc));
    }
    Ok(())
}

/// Remove a key and everything under it. A key that is not there is fine.
fn delete_tree(key: &str) -> Result<()> {
    let wkey = wide(key);
    let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, wkey.as_ptr()) };
    if rc != ERROR_SUCCESS && rc != ERROR_FILE_NOT_FOUND {
        return Err(failed(key, rc));
    }
    Ok(())
}

/// Tell Explorer the associations changed, so it does not need a restart.
fn announce() {
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}

pub fn status(note: &Saved) -> Status {
    let active = read_default(&shell_key("Directory")).as_deref() == Some(VERB);
    let detail = if active {
        "Folders and drives opened from other apps, the desktop and shortcuts open here.".to_string()
    } else {
        "Folders opened from other apps go to File Explorer.".to_string()
    };
    Status {
        supported: true,
        active,
        previous: note.previous.get("Directory").cloned(),
        detail,
        limits: vec![
            "\"Show in folder\" in browsers and many apps asks File Explorer directly and still opens there.".to_string(),
            "Win+E, the taskbar's folder button, Control Panel and other special folders stay with File Explorer.".to_string(),
        ],
    }
}

pub fn set_default(note: &mut Saved, exe: &Path, _app_id: &str) -> Result<()> {
    let command = format!("\"{}\" \"%1\"", exe.to_string_lossy());
    for class in CLASSES {
        let shell = shell_key(class);
        match read_default(&shell) {
            Some(old) if old != VERB => {
                note.previous.insert(class.to_string(), old);
            }
            _ => {}
        }
        let made = write_default(&format!("{shell}\\{VERB}"), LABEL)
            .and_then(|_| write_default(&format!("{shell}\\{VERB}\\command"), &command))
            .and_then(|_| write_default(&shell, VERB));
        if let Err(e) = made {
            // Half a change is worse than none.
            let _ = restore(note);
            return Err(e);
        }
    }
    announce();
    Ok(())
}

pub fn restore(note: &mut Saved) -> Result<()> {
    let mut first: Option<CoreError> = None;
    for class in CLASSES {
        let shell = shell_key(class);
        // Only undo what is still the app's: someone may have picked another program since.
        if read_default(&shell).as_deref() == Some(VERB) {
            let put_back = match note.previous.get(class) {
                Some(old) => write_default(&shell, old),
                None => clear_default(&shell),
            };
            if let Err(e) = put_back {
                first.get_or_insert(e);
            }
        }
        if let Err(e) = delete_tree(&format!("{shell}\\{VERB}")) {
            first.get_or_insert(e);
        }
    }
    announce();
    match first {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
