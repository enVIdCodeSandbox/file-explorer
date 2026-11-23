//! Read-only file and folder permissions inspection across platforms.
//!
//! * Windows: reads the NTFS security descriptor (owner, group, inheritance,
//!   and DACL access control entries with resolved account names).
//! * Unix: reads POSIX mode bits, special flags, and owner/group names.

use crate::{CoreError, Result};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Triple {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UnixPermissions {
    /// e.g. `-rwxr-xr-x`
    pub mode_text: String,
    /// e.g. `755` or `4755`
    pub octal: String,
    pub uid: u32,
    pub gid: u32,
    pub setuid: bool,
    pub setgid: bool,
    pub sticky: bool,
    pub owner_bits: Triple,
    pub group_bits: Triple,
    pub other_bits: Triple,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WindowsAce {
    /// Full qualified name when available (e.g. `BUILTIN\Administrators`), else SID string.
    pub account: String,
    /// Short account name (e.g. `Administrators`).
    pub name: String,
    /// Domain or authority (e.g. `BUILTIN`, `NT AUTHORITY`), if any.
    pub domain: Option<String>,
    /// `"allow"` or `"deny"`.
    pub kind: String,
    /// Summary label: `"Full control"`, `"Modify"`, `"Read & execute"`, `"Read"`, `"Write"`, etc.
    pub level: String,
    pub full_control: bool,
    pub modify: bool,
    pub read_execute: bool,
    pub read: bool,
    pub write: bool,
    /// Granular rights included in this entry.
    pub rights: Vec<String>,
    pub inherited: bool,
    /// For folders, which items this entry applies to (empty for files).
    pub applies_to: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WindowsPermissions {
    /// Whether this item inherits permissions from its parent folder.
    pub inherited: bool,
    pub entries: Vec<WindowsAce>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FilePermissions {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub readonly: bool,
    /// `"windows"`, `"unix"`, or `"basic"`.
    pub kind: String,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub unix: Option<UnixPermissions>,
    pub windows: Option<WindowsPermissions>,
    pub note: Option<String>,
}

pub fn permissions(path: &Path) -> Result<FilePermissions> {
    let lmeta = fs::symlink_metadata(path).map_err(|e| CoreError::io(path, e))?;
    let is_symlink = lmeta.file_type().is_symlink();
    let meta = if is_symlink { fs::metadata(path).unwrap_or_else(|_| lmeta.clone()) } else { lmeta.clone() };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let is_dir = meta.is_dir();
    let readonly = meta.permissions().readonly();

    platform_permissions(path, &name, is_dir, readonly, &lmeta, &meta)
}

#[cfg(windows)]
fn platform_permissions(
    path: &Path,
    name: &str,
    is_dir: bool,
    readonly: bool,
    _lmeta: &fs::Metadata,
    _meta: &fs::Metadata,
) -> Result<FilePermissions> {
    windows_imp::read(path, name, is_dir, readonly)
}

#[cfg(unix)]
fn platform_permissions(
    path: &Path,
    name: &str,
    is_dir: bool,
    readonly: bool,
    lmeta: &fs::Metadata,
    meta: &fs::Metadata,
) -> Result<FilePermissions> {
    use std::os::unix::fs::MetadataExt;
    let mode = meta.mode();
    let is_link = lmeta.file_type().is_symlink();
    let uid = meta.uid();
    let gid = meta.gid();
    let owner_name = unix_name(uid, true);
    let group_name = unix_name(gid, false);
    let owner = Some(match owner_name {
        Some(n) => format!("{n} ({uid})"),
        None => uid.to_string(),
    });
    let group = Some(match group_name {
        Some(n) => format!("{n} ({gid})"),
        None => gid.to_string(),
    });
    let special = mode & 0o7000;
    let octal = if special != 0 { format!("{:04o}", mode & 0o7777) } else { format!("{:03o}", mode & 0o777) };
    Ok(FilePermissions {
        path: path.to_string_lossy().into_owned(),
        name: name.to_string(),
        is_dir,
        readonly,
        kind: "unix".into(),
        owner,
        group,
        unix: Some(UnixPermissions {
            mode_text: crate::columns_extra::mode_text(mode, is_dir, is_link),
            octal,
            uid,
            gid,
            setuid: mode & 0o4000 != 0,
            setgid: mode & 0o2000 != 0,
            sticky: mode & 0o1000 != 0,
            owner_bits: Triple { read: mode & 0o400 != 0, write: mode & 0o200 != 0, execute: mode & 0o100 != 0 },
            group_bits: Triple { read: mode & 0o040 != 0, write: mode & 0o020 != 0, execute: mode & 0o010 != 0 },
            other_bits: Triple { read: mode & 0o004 != 0, write: mode & 0o002 != 0, execute: mode & 0o001 != 0 },
        }),
        windows: None,
        note: None,
    })
}

#[cfg(not(any(windows, unix)))]
fn platform_permissions(
    path: &Path,
    name: &str,
    is_dir: bool,
    readonly: bool,
    _lmeta: &fs::Metadata,
    _meta: &fs::Metadata,
) -> Result<FilePermissions> {
    Ok(FilePermissions {
        path: path.to_string_lossy().into_owned(),
        name: name.to_string(),
        is_dir,
        readonly,
        kind: "basic".into(),
        owner: None,
        group: None,
        unix: None,
        windows: None,
        note: Some("Detailed per-user permissions are not supported on this platform.".into()),
    })
}

#[cfg(unix)]
fn unix_name(id: u32, user: bool) -> Option<String> {
    use std::ffi::CStr;
    let mut buf = vec![0u8; 4096];
    // SAFETY: `buf` outlives the libc call and the returned string is cloned before `buf` drops.
    unsafe {
        if user {
            let mut pwd: libc::passwd = std::mem::zeroed();
            let mut res: *mut libc::passwd = std::ptr::null_mut();
            if libc::getpwuid_r(id, &mut pwd, buf.as_mut_ptr() as *mut libc::c_char, buf.len(), &mut res) != 0 || res.is_null() {
                return None;
            }
            Some(CStr::from_ptr(pwd.pw_name).to_string_lossy().into_owned())
        } else {
            let mut grp: libc::group = std::mem::zeroed();
            let mut res: *mut libc::group = std::ptr::null_mut();
            if libc::getgrgid_r(id, &mut grp, buf.as_mut_ptr() as *mut libc::c_char, buf.len(), &mut res) != 0 || res.is_null() {
                return None;
            }
            Some(CStr::from_ptr(grp.gr_name).to_string_lossy().into_owned())
        }
    }
}

#[cfg(windows)]
mod windows_imp {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, BOOL, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, GetFileSecurityW, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorGroup, GetSecurityDescriptorOwner,
        GetSidIdentifierAuthority, GetSidSubAuthority, GetSidSubAuthorityCount, IsValidSid, LookupAccountSidW,
        MapGenericMask, ACCESS_ALLOWED_ACE, ACCESS_DENIED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
        CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GENERIC_MAPPING, GROUP_SECURITY_INFORMATION,
        INHERITED_ACE, INHERIT_ONLY_ACE, NO_PROPAGATE_INHERIT_ACE, OBJECT_INHERIT_ACE,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, SID_NAME_USE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_EXECUTE, FILE_GENERIC_EXECUTE,
        FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_READ_EA,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
    };

    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;

    const FILE_MAPPING: GENERIC_MAPPING = GENERIC_MAPPING {
        GenericRead: FILE_GENERIC_READ,
        GenericWrite: FILE_GENERIC_WRITE,
        GenericExecute: FILE_GENERIC_EXECUTE,
        GenericAll: FILE_ALL_ACCESS,
    };

    pub fn read(path: &Path, name: &str, is_dir: bool, readonly: bool) -> Result<FilePermissions> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let all_info = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;

        let (buf, has_dacl_query, err_code) = match query_sd(&wide, all_info) {
            Ok(b) => (Some(b), true, 0),
            Err(code) => {
                // If reading DACL is denied, try at least owner & group.
                if let Ok(b) = query_sd(&wide, OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION) {
                    (Some(b), false, code)
                } else {
                    (None, false, code)
                }
            }
        };

        let Some(buf) = buf else {
            let note = if err_code == ERROR_ACCESS_DENIED {
                "Access denied when reading security permissions for this item.".into()
            } else {
                format!("Could not read security permissions (system error {err_code}).")
            };
            return Ok(FilePermissions {
                path: path.to_string_lossy().into_owned(),
                name: name.to_string(),
                is_dir,
                readonly,
                kind: "windows".into(),
                owner: None,
                group: None,
                unix: None,
                windows: None,
                note: Some(note),
            });
        };

        let sd = buf.as_ptr() as PSECURITY_DESCRIPTOR;
        let owner = unsafe { read_owner(sd) }.map(|(full, _, _)| full);
        let group = unsafe { read_group(sd) }.map(|(full, _, _)| full);

        if !has_dacl_query {
            return Ok(FilePermissions {
                path: path.to_string_lossy().into_owned(),
                name: name.to_string(),
                is_dir,
                readonly,
                kind: "windows".into(),
                owner,
                group,
                unix: None,
                windows: None,
                note: Some(if err_code == ERROR_ACCESS_DENIED {
                    "Access denied when reading the access control list.".into()
                } else {
                    format!("Could not read the access control list (system error {err_code}).")
                }),
            });
        }

        let mut control: u16 = 0;
        let mut revision: u32 = 0;
        let inherited = unsafe {
            if GetSecurityDescriptorControl(sd, &mut control, &mut revision) != 0 {
                (control & SE_DACL_PROTECTED) == 0
            } else {
                true
            }
        };

        let mut dacl_present: BOOL = 0;
        let mut dacl_defaulted: BOOL = 0;
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let ok = unsafe { GetSecurityDescriptorDacl(sd, &mut dacl_present, &mut dacl, &mut dacl_defaulted) };
        if ok == 0 || dacl_present == 0 || dacl.is_null() {
            return Ok(FilePermissions {
                path: path.to_string_lossy().into_owned(),
                name: name.to_string(),
                is_dir,
                readonly,
                kind: "windows".into(),
                owner,
                group,
                unix: None,
                windows: None,
                note: Some("This volume or item does not have a per-user access control list (for example FAT32/exFAT or unrestricted access).".into()),
            });
        }

        let entries = unsafe { read_aces(dacl, is_dir) };
        Ok(FilePermissions {
            path: path.to_string_lossy().into_owned(),
            name: name.to_string(),
            is_dir,
            readonly,
            kind: "windows".into(),
            owner,
            group,
            unix: None,
            windows: Some(WindowsPermissions { inherited, entries }),
            note: None,
        })
    }

    fn query_sd(wide: &[u16], info: u32) -> std::result::Result<Vec<u8>, u32> {
        let mut needed: u32 = 0;
        let ok = unsafe { GetFileSecurityW(wide.as_ptr(), info, std::ptr::null_mut(), 0, &mut needed) };
        if ok != 0 {
            return Ok(Vec::new());
        }
        let err = unsafe { GetLastError() };
        if err != ERROR_INSUFFICIENT_BUFFER || needed == 0 {
            return Err(err);
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe { GetFileSecurityW(wide.as_ptr(), info, buf.as_mut_ptr() as PSECURITY_DESCRIPTOR, needed, &mut needed) };
        if ok == 0 {
            return Err(unsafe { GetLastError() });
        }
        Ok(buf)
    }

    unsafe fn read_owner(sd: PSECURITY_DESCRIPTOR) -> Option<(String, String, Option<String>)> {
        let mut sid: PSID = std::ptr::null_mut();
        let mut defaulted: BOOL = 0;
        if GetSecurityDescriptorOwner(sd, &mut sid, &mut defaulted) == 0 || sid.is_null() {
            return None;
        }
        Some(resolve_sid(sid))
    }

    unsafe fn read_group(sd: PSECURITY_DESCRIPTOR) -> Option<(String, String, Option<String>)> {
        let mut sid: PSID = std::ptr::null_mut();
        let mut defaulted: BOOL = 0;
        if GetSecurityDescriptorGroup(sd, &mut sid, &mut defaulted) == 0 || sid.is_null() {
            return None;
        }
        Some(resolve_sid(sid))
    }

    /// Returns `(full_account, short_name, domain)`.
    unsafe fn resolve_sid(sid: PSID) -> (String, String, Option<String>) {
        if IsValidSid(sid) == 0 {
            return ("Unknown".into(), "Unknown".into(), None);
        }
        let mut name_len: u32 = 0;
        let mut dom_len: u32 = 0;
        let mut use_kind: SID_NAME_USE = 0;
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            std::ptr::null_mut(),
            &mut name_len,
            std::ptr::null_mut(),
            &mut dom_len,
            &mut use_kind,
        );
        if name_len > 0 {
            let mut name_buf = vec![0u16; name_len as usize];
            let mut dom_buf = vec![0u16; dom_len.max(1) as usize];
            if LookupAccountSidW(
                std::ptr::null(),
                sid,
                name_buf.as_mut_ptr(),
                &mut name_len,
                dom_buf.as_mut_ptr(),
                &mut dom_len,
                &mut use_kind,
            ) != 0
            {
                let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
                let dom = String::from_utf16_lossy(&dom_buf[..dom_len as usize]);
                let domain = (!dom.is_empty()).then_some(dom);
                let full = match &domain {
                    Some(d) => format!("{d}\\{name}"),
                    None => name.clone(),
                };
                return (full, name, domain);
            }
        }
        let s = format_sid(sid);
        (s.clone(), s, None)
    }

    unsafe fn format_sid(sid: PSID) -> String {
        let count_ptr = GetSidSubAuthorityCount(sid);
        let auth_ptr = GetSidIdentifierAuthority(sid);
        if count_ptr.is_null() || auth_ptr.is_null() {
            return "Unknown SID".into();
        }
        let count = *count_ptr as u32;
        let auth = (*auth_ptr).Value;
        let authority = u64::from_be_bytes([0, 0, auth[0], auth[1], auth[2], auth[3], auth[4], auth[5]]);
        let mut out = format!("S-1-{authority}");
        for i in 0..count {
            let sub = GetSidSubAuthority(sid, i);
            if sub.is_null() {
                break;
            }
            out.push_str(&format!("-{}", *sub));
        }
        out
    }

    unsafe fn read_aces(dacl: *const ACL, is_dir: bool) -> Vec<WindowsAce> {
        let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
        if GetAclInformation(
            dacl,
            &mut info as *mut ACL_SIZE_INFORMATION as *mut core::ffi::c_void,
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        ) == 0
        {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(info.AceCount as usize);
        for i in 0..info.AceCount {
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            if GetAce(dacl, i, &mut ptr) == 0 || ptr.is_null() {
                continue;
            }
            let header = &*(ptr as *const ACE_HEADER);
            let (kind, mut mask, sid) = match header.AceType {
                ACCESS_ALLOWED_ACE_TYPE => {
                    let ace = &*(ptr as *const ACCESS_ALLOWED_ACE);
                    ("allow", ace.Mask, &ace.SidStart as *const u32 as PSID)
                }
                ACCESS_DENIED_ACE_TYPE => {
                    let ace = &*(ptr as *const ACCESS_DENIED_ACE);
                    ("deny", ace.Mask, &ace.SidStart as *const u32 as PSID)
                }
                _ => continue,
            };
            MapGenericMask(&mut mask, &FILE_MAPPING);
            let (account, name, domain) = resolve_sid(sid);
            let flags = header.AceFlags;
            let inherited = (flags & INHERITED_ACE as u8) != 0;
            out.push(classify_ace(account, name, domain, kind, mask, flags, inherited, is_dir));
        }
        out
    }

    fn classify_ace(
        account: String,
        name: String,
        domain: Option<String>,
        kind: &str,
        mask: u32,
        flags: u8,
        inherited: bool,
        is_dir: bool,
    ) -> WindowsAce {
        let modify_mask = FILE_GENERIC_READ | FILE_GENERIC_WRITE | FILE_GENERIC_EXECUTE | DELETE;
        let read_exec_mask = FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;

        let full_control = (mask & FILE_ALL_ACCESS) == FILE_ALL_ACCESS;
        let modify = (mask & modify_mask) == modify_mask;
        let read_execute = (mask & read_exec_mask) == read_exec_mask;
        let read = (mask & FILE_GENERIC_READ) == FILE_GENERIC_READ;
        let write = (mask & FILE_GENERIC_WRITE) == FILE_GENERIC_WRITE;

        let level = if full_control {
            "Full control"
        } else if modify {
            "Modify"
        } else if read_execute && write {
            "Read, execute & write"
        } else if read_execute {
            "Read & execute"
        } else if read && write {
            "Read & write"
        } else if read {
            "Read"
        } else if write {
            "Write"
        } else {
            "Special"
        }
        .to_string();

        let rights = describe_rights(mask, is_dir);
        let applies_to = if is_dir { describe_scope(flags) } else { String::new() };

        WindowsAce {
            account,
            name,
            domain,
            kind: kind.to_string(),
            level,
            full_control,
            modify,
            read_execute,
            read,
            write,
            rights,
            inherited,
            applies_to,
        }
    }

    fn describe_scope(flags: u8) -> String {
        let obj = (flags & OBJECT_INHERIT_ACE as u8) != 0;
        let cont = (flags & CONTAINER_INHERIT_ACE as u8) != 0;
        let inh_only = (flags & INHERIT_ONLY_ACE as u8) != 0;
        let no_prop = (flags & NO_PROPAGATE_INHERIT_ACE as u8) != 0;
        let base = match (obj, cont, inh_only) {
            (false, false, _) => "This folder only",
            (true, true, false) => "This folder, subfolders and files",
            (true, true, true) => "Subfolders and files only",
            (false, true, false) => "This folder and subfolders",
            (false, true, true) => "Subfolders only",
            (true, false, false) => "This folder and files",
            (true, false, true) => "Files only",
        };
        if no_prop && (obj || cont) {
            format!("{base} (direct children only)")
        } else {
            base.to_string()
        }
    }

    fn describe_rights(mask: u32, is_dir: bool) -> Vec<String> {
        let bits: &[(u32, &str)] = if is_dir {
            &[
                (FILE_READ_DATA, "List folder / read data"),
                (FILE_WRITE_DATA, "Create files / write data"),
                (FILE_APPEND_DATA, "Create folders / append data"),
                (FILE_READ_EA, "Read extended attributes"),
                (FILE_WRITE_EA, "Write extended attributes"),
                (FILE_EXECUTE, "Traverse folder / execute"),
                (FILE_DELETE_CHILD, "Delete subfolders and files"),
                (FILE_READ_ATTRIBUTES, "Read attributes"),
                (FILE_WRITE_ATTRIBUTES, "Write attributes"),
                (DELETE, "Delete"),
                (READ_CONTROL, "Read permissions"),
                (WRITE_DAC, "Change permissions"),
                (WRITE_OWNER, "Take ownership"),
            ]
        } else {
            &[
                (FILE_READ_DATA, "Read data"),
                (FILE_WRITE_DATA, "Write data"),
                (FILE_APPEND_DATA, "Append data"),
                (FILE_READ_EA, "Read extended attributes"),
                (FILE_WRITE_EA, "Write extended attributes"),
                (FILE_EXECUTE, "Execute file"),
                (FILE_READ_ATTRIBUTES, "Read attributes"),
                (FILE_WRITE_ATTRIBUTES, "Write attributes"),
                (DELETE, "Delete"),
                (READ_CONTROL, "Read permissions"),
                (WRITE_DAC, "Change permissions"),
                (WRITE_OWNER, "Take ownership"),
            ]
        };
        bits.iter().filter(|(bit, _)| mask & bit != 0).map(|(_, label)| (*label).to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_file_and_folder_permissions() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("sample.txt");
        fs::write(&file, b"hello").unwrap();

        let dir_perms = permissions(d.path()).unwrap();
        assert!(dir_perms.is_dir);
        assert_eq!(dir_perms.path, d.path().to_string_lossy());

        let file_perms = permissions(&file).unwrap();
        assert!(!file_perms.is_dir);
        assert_eq!(file_perms.name, "sample.txt");

        #[cfg(windows)]
        {
            assert_eq!(file_perms.kind, "windows");
            assert!(file_perms.owner.is_some());
            let win = file_perms.windows.as_ref().expect("windows ACL expected on NTFS");
            assert!(!win.entries.is_empty());
        }
        #[cfg(unix)]
        {
            assert_eq!(file_perms.kind, "unix");
            let u = file_perms.unix.as_ref().unwrap();
            assert_eq!(u.mode_text.len(), 10);
            assert!(!u.octal.is_empty());
        }
    }

    #[test]
    fn missing_path_errors() {
        let d = tempfile::tempdir().unwrap();
        assert!(permissions(&d.path().join("does-not-exist")).is_err());
    }
}
