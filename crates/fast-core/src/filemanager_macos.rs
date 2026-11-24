//! macOS: the app becomes the program Launch Services starts for folders.
//!
//! Two settings, both for the signed-in person only:
//!
//! * the handler for the `public.folder` content type (what "open this folder" uses), and
//! * the `NSFileViewer` default, which apps that say "Reveal in Finder" consult.
//!
//! The app must be an installed `.app` whose Info.plist declares that it opens
//! folders (`src-tauri/mac/Info.plist`); a development build run from a terminal
//! is not something Launch Services can point at. Finder keeps running: it owns
//! the desktop, and some apps talk to it directly.

use super::*;
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use std::process::Command;

const FOLDER: &str = "public.folder";
const FINDER: &str = "com.apple.finder";
/// `kLSRolesAll`
const ALL_ROLES: u32 = 0xFFFF_FFFF;

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSCopyDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32) -> CFStringRef;
    fn LSSetDefaultRoleHandlerForContentType(content_type: CFStringRef, role: u32, handler: CFStringRef) -> i32;
}

fn folder_handler() -> Option<String> {
    let kind = CFString::new(FOLDER);
    let got = unsafe { LSCopyDefaultRoleHandlerForContentType(kind.as_concrete_TypeRef(), ALL_ROLES) };
    if got.is_null() {
        return None;
    }
    // "Copy" in the name: the string is ours to release.
    Some(unsafe { CFString::wrap_under_create_rule(got) }.to_string())
}

/// The Launch Services preferences, read the way `defaults` reads them (so what the preferences daemon holds is seen).
const LS_DOMAIN: &str = "com.apple.LaunchServices/com.apple.launchservices.secure";

fn read_ls_prefs() -> plist::Dictionary {
    match Command::new("defaults").args(["export", LS_DOMAIN, "-"]).output() {
        Ok(o) if o.status.success() => plist::Value::from_reader(std::io::Cursor::new(o.stdout)).ok().and_then(plist::Value::into_dictionary).unwrap_or_default(),
        _ => plist::Dictionary::new(),
    }
}

fn write_ls_prefs(dict: plist::Dictionary) -> Result<()> {
    use std::io::Write;
    let mut xml = Vec::new();
    plist::Value::Dictionary(dict).to_writer_xml(&mut xml).map_err(|e| CoreError::Invalid(e.to_string()))?;
    let mut child = Command::new("defaults")
        .args(["import", LS_DOMAIN, "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| CoreError::Invalid(format!("could not run defaults: {e}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&xml).map_err(|e| CoreError::Invalid(e.to_string()))?;
    }
    let status = child.wait().map_err(|e| CoreError::Invalid(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::Invalid("defaults refused to write the Launch Services preferences".into()))
    }
}

/// The folder handler named in the preferences file itself.
fn prefs_folder_handler() -> Option<String> {
    crate::lshandlers::folder_handler(&read_ls_prefs())
}

/// What opens folders now: the preferences entry when there is one (that is what is written below), else what
/// Launch Services reports.
fn current_folder_handler() -> Option<String> {
    prefs_folder_handler().or_else(folder_handler)
}

fn set_folder_handler(id: &str) -> Result<()> {
    let (kind, id_ref) = (CFString::new(FOLDER), CFString::new(id));
    let rc = unsafe { LSSetDefaultRoleHandlerForContentType(kind.as_concrete_TypeRef(), ALL_ROLES, id_ref.as_concrete_TypeRef()) };
    if rc == 0 {
        return Ok(());
    }
    // Current macOS answers -50 for folders. File managers take over by writing the preference that call would write.
    // Putting Finder back is done by removing the entry, which is what "never changed" looks like.
    let entry = if same(id, FINDER) { None } else { Some(id) };
    write_ls_prefs(crate::lshandlers::with_folder_handler(read_ls_prefs(), entry))
        .map_err(|e| CoreError::Invalid(format!("macOS did not accept the change (Launch Services error {rc}), and writing the setting directly failed too: {e}")))
}

fn file_viewer() -> Option<String> {
    let out = Command::new("defaults").args(["read", "-g", "NSFileViewer"]).output().ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty()).then_some(id)
}

fn set_file_viewer(id: Option<&str>) {
    let _ = match id {
        Some(id) => Command::new("defaults").args(["write", "-g", "NSFileViewer", "-string", id]).status(),
        None => Command::new("defaults").args(["delete", "-g", "NSFileViewer"]).status(),
    };
}

/// True when the program runs from inside an application bundle.
fn bundled() -> bool {
    std::env::current_exe().map(|p| p.to_string_lossy().contains(".app/Contents/MacOS/")).unwrap_or(false)
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

pub fn status(note: &Saved) -> Status {
    let limits = vec![
        "Finder keeps running: it shows the desktop, and some apps ask Finder directly when they reveal a file.".to_string(),
        "Sign out and back in once after switching, so every app notices the change.".to_string(),
    ];
    if !bundled() {
        return Status {
            supported: false,
            detail: "Install the app (drag it into Applications) and open it from there to use this.".into(),
            limits,
            ..Default::default()
        };
    }
    let ours = note.previous.get("id").cloned();
    let active = match (&ours, current_folder_handler()) {
        (Some(id), Some(now)) => same(id, &now),
        _ => false,
    };
    let detail = if active {
        "Folders opened from other apps, and \"Reveal in Finder\" in apps that honour the setting, open here.".to_string()
    } else {
        "Folders opened from other apps go to Finder.".to_string()
    };
    Status { supported: true, active, previous: note.previous.get("folder").cloned(), detail, limits }
}

pub fn set_default(note: &mut Saved, _exe: &Path, app_id: &str) -> Result<()> {
    if !bundled() {
        return Err(CoreError::Unsupported("the app is not running from an application bundle"));
    }
    match current_folder_handler() {
        Some(now) if !same(&now, app_id) => {
            note.previous.insert("folder".into(), now);
        }
        _ => {}
    }
    match file_viewer() {
        Some(now) if !same(&now, app_id) => {
            note.previous.insert("viewer".into(), now);
        }
        _ => {}
    }
    set_folder_handler(app_id)?;
    set_file_viewer(Some(app_id));
    note.previous.insert("id".into(), app_id.to_string());
    Ok(())
}

pub fn restore(note: &mut Saved) -> Result<()> {
    let Some(id) = note.previous.get("id").cloned() else { return Ok(()) };
    // Only undo what is still the app's: another program may have been chosen since.
    if current_folder_handler().is_some_and(|now| same(&now, &id)) {
        let back = note.previous.get("folder").cloned().unwrap_or_else(|| FINDER.to_string());
        set_folder_handler(&back)?;
    }
    if file_viewer().is_some_and(|now| same(&now, &id)) {
        set_file_viewer(note.previous.get("viewer").map(String::as_str));
    }
    Ok(())
}
