//! Phones, cameras, players and other devices that are plugged in but are not drives.
//!
//! Drives (USB sticks, cards, discs) are listed by [`crate::drives`]. This is for the rest:
//! what Explorer calls "portable devices", what Finder keeps out of sight, and what Linux
//! file managers show as a phone you can open. Each platform finds them its own way:
//!
//! * Linux reads `/sys/bus/usb/devices` and the GVFS mounts (`mtp:`, `gphoto2:`, `afc:`), and
//!   can mount a phone through `gio mount`.
//! * macOS asks `system_profiler SPUSBDataType`.
//! * Windows walks the "This PC" shell folder for items that are not file-system folders.
//!
//! Where the app can open the device's files itself, `path` is set. Otherwise `open` names
//! what the system's own tool should be asked to open.

use serde::{Deserialize, Serialize};
#[cfg(any(target_os = "linux", test))]
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Phone,
    Camera,
    Printer,
    Audio,
    Input,
    Bluetooth,
    Other,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Stable while the device stays connected.
    pub id: String,
    pub name: String,
    pub maker: String,
    pub kind: DeviceKind,
    /// A folder the app can browse itself (a phone mounted through GVFS, say).
    pub path: Option<String>,
    /// True when the device can be mounted on request, after which `path` appears.
    pub connectable: bool,
    /// What the system's own tool opens it with, when the app cannot browse it:
    /// a shell location on Windows, an application name on macOS.
    pub open: Option<String>,
    /// What the person may need to do, e.g. unlock the phone.
    pub note: String,
}

const PHONE_WORDS: &[&str] = &[
    "android", "pixel", "galaxy", "iphone", "ipad", "ipod", "xiaomi", "redmi", "poco", "oneplus", "oppo", "vivo",
    "realme", "huawei", "honor", "motorola", "moto ", "nokia", "sony xperia", "xperia", "lg-", "nothing phone",
    "samsung", "kindle", "fire tablet", "tablet", "phone",
];
const CAMERA_WORDS: &[&str] = &["camera", "canon", "nikon", "gopro", "fujifilm", "olympus", "panasonic lumix", "eos", "dslr", "webcam"];

const UNLOCK: &str = "Unlock the phone and choose \"File transfer\" in its USB notification.";

fn has_word(name: &str, words: &[&str]) -> bool {
    let n = name.to_lowercase();
    words.iter().any(|w| n.contains(w))
}

/// Guess what a device with this name is.
pub fn kind_from_name(name: &str) -> Option<DeviceKind> {
    if has_word(name, CAMERA_WORDS) {
        Some(DeviceKind::Camera)
    } else if has_word(name, PHONE_WORDS) {
        Some(DeviceKind::Phone)
    } else {
        None
    }
}

/// Whether to include devices that are neither phones nor cameras (printers, keyboards,
/// receivers...). Phones and cameras are always listed.
pub fn is_main(kind: DeviceKind) -> bool {
    matches!(kind, DeviceKind::Phone | DeviceKind::Camera)
}

pub fn list_devices(show_other: bool) -> Vec<Device> {
    let mut all = list_all();
    all.retain(|d| show_other || is_main(d.kind));
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    all
}

/// Feeds what is connected into `h`, cheaply, so a change can be noticed.
pub fn signature(h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    #[cfg(target_os = "macos")]
    {
        // Far cheaper than system_profiler; the full listing is only built when this moves.
        if let Ok(o) = std::process::Command::new("ioreg").args(["-p", "IOUSB", "-w0"]).output() {
            o.stdout.hash(h);
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut ids: Vec<String> = std::fs::read_dir(usb_root())
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        ids.sort();
        ids.hash(h);
        // A phone being mounted or unmounted through GVFS changes the device, not the USB bus.
        let mut mounts: Vec<String> = gvfs_dir()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        mounts.sort();
        mounts.hash(h);
    }
    #[cfg(windows)]
    {
        list_all().iter().map(|d| (&d.id, &d.name)).collect::<Vec<_>>().hash(h);
    }
    let _ = h;
}

#[cfg(target_os = "linux")]
fn list_all() -> Vec<Device> {
    let usb = parse_usb(&usb_root());
    let mounts = gvfs_dir().map(|d| gvfs_mounts(&d)).unwrap_or_default();
    let roots = gio_roots();
    build_linux(&usb, &mounts, &roots)
}

#[cfg(target_os = "macos")]
fn list_all() -> Vec<Device> {
    let json = std::process::Command::new("system_profiler")
        .args(["SPUSBDataType", "-json"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    parse_system_profiler(&json, |app| Path::new("/Applications").join(format!("{app}.app")).exists())
}

#[cfg(windows)]
fn list_all() -> Vec<Device> {
    win::list()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn list_all() -> Vec<Device> {
    Vec::new()
}

// ---------------------------------------------------------------------------
// Linux
// ---------------------------------------------------------------------------

/// One USB device as the kernel describes it.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usb {
    pub id: String,
    pub vendor_id: String,
    pub product_id: String,
    pub maker: String,
    pub product: String,
    pub serial: String,
    pub device_class: u8,
    /// `(class, interface name)` for each interface.
    pub interfaces: Vec<(u8, String)>,
}

#[cfg(any(target_os = "linux", test))]
fn read_attr(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).map(|s| s.trim().to_string()).unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
fn hex_attr(dir: &Path, name: &str) -> u8 {
    u8::from_str_radix(&read_attr(dir, name), 16).unwrap_or(0)
}

/// Read the devices under a sysfs USB directory (`/sys/bus/usb/devices`).
#[cfg(any(target_os = "linux", test))]
pub fn parse_usb(root: &Path) -> Vec<Usb> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let mut out = Vec::new();
    for name in &names {
        // Interfaces are "1-2:1.0"; root hubs are "usb1". Neither is a device of its own.
        if name.contains(':') || name.starts_with("usb") {
            continue;
        }
        let dir = root.join(name);
        let mut interfaces = Vec::new();
        for n in &names {
            if let Some(rest) = n.strip_prefix(name.as_str()) {
                if rest.starts_with(':') {
                    let d = root.join(n);
                    interfaces.push((hex_attr(&d, "bInterfaceClass"), read_attr(&d, "interface")));
                }
            }
        }
        interfaces.sort();
        out.push(Usb {
            id: name.clone(),
            vendor_id: read_attr(&dir, "idVendor"),
            product_id: read_attr(&dir, "idProduct"),
            maker: read_attr(&dir, "manufacturer"),
            product: read_attr(&dir, "product"),
            serial: read_attr(&dir, "serial"),
            device_class: hex_attr(&dir, "bDeviceClass"),
            interfaces,
        });
    }
    out
}

/// What kind of thing a USB device is, or `None` for what is not worth listing (hubs, and mass
/// storage, which is listed as a drive once mounted).
#[cfg(any(target_os = "linux", test))]
pub fn classify_usb(u: &Usb) -> Option<DeviceKind> {
    let classes: Vec<u8> = std::iter::once(u.device_class).chain(u.interfaces.iter().map(|i| i.0)).collect();
    let has = |c: u8| classes.contains(&c);
    if has(0x08) {
        return None;
    }
    if classes.iter().all(|&c| c == 0x09 || c == 0) && has(0x09) {
        return None;
    }
    let name = format!("{} {}", u.maker, u.product);
    let mtp = u.interfaces.iter().any(|(_, n)| n.to_lowercase().contains("mtp") || n.to_lowercase().contains("android") || n.to_lowercase().contains("adb"));
    if has(0x06) || mtp {
        return Some(kind_from_name(&name).filter(|k| *k == DeviceKind::Camera).unwrap_or(DeviceKind::Phone));
    }
    if let Some(k) = kind_from_name(&name) {
        return Some(k);
    }
    if u.vendor_id == "05ac" && u.interfaces.iter().any(|i| i.0 == 0xff) {
        return Some(DeviceKind::Phone); // Apple's device interface (iPhone, iPad)
    }
    Some(if has(0x07) {
        DeviceKind::Printer
    } else if has(0x01) {
        DeviceKind::Audio
    } else if has(0x03) {
        DeviceKind::Input
    } else if has(0xe0) {
        DeviceKind::Bluetooth
    } else if has(0x0e) {
        DeviceKind::Camera
    } else {
        DeviceKind::Other
    })
}

/// A GVFS mount of a device: `mtp:host=Name_1234` is `("mtp", "Name_1234", path)`.
#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, PartialEq)]
pub struct Mounted {
    pub scheme: String,
    pub host: String,
    pub path: String,
}

/// Where the kernel lists USB devices. `FAST_EXPLORER_USB_ROOT` points it at a made-up tree for tests.
#[cfg(target_os = "linux")]
fn usb_root() -> std::path::PathBuf {
    std::env::var_os("FAST_EXPLORER_USB_ROOT").map(std::path::PathBuf::from).unwrap_or_else(|| "/sys/bus/usb/devices".into())
}

#[cfg(target_os = "linux")]
fn gvfs_dir() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("FAST_EXPLORER_GVFS_DIR") {
        return Some(std::path::PathBuf::from(d));
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).or_else(|| {
        let uid = unsafe { libc::getuid() };
        Some(std::path::PathBuf::from(format!("/run/user/{uid}")))
    })?;
    let d = base.join("gvfs");
    d.is_dir().then_some(d)
}

/// The devices GVFS has mounted in `dir` (`$XDG_RUNTIME_DIR/gvfs`).
#[cfg(any(target_os = "linux", test))]
pub fn gvfs_mounts(dir: &Path) -> Vec<Mounted> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (scheme, rest) = name.split_once(':')?;
            if !matches!(scheme, "mtp" | "gphoto2" | "afc") {
                return None;
            }
            let host = rest.split(',').find_map(|p| p.strip_prefix("host="))?;
            Some(Mounted { scheme: scheme.into(), host: percent_decode(host), path: e.path().to_string_lossy().into_owned() })
        })
        .collect()
}

#[cfg(any(target_os = "linux", test))]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Devices GVFS knows about but has not mounted, from `gio mount -li` (`activation_root=mtp://host/`).
#[cfg(any(target_os = "linux", test))]
pub fn parse_gio_roots(output: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        let Some(url) = line.strip_prefix("activation_root=") else { continue };
        let Some((scheme, rest)) = url.split_once("://") else { continue };
        if !matches!(scheme, "mtp" | "gphoto2" | "afc") {
            continue;
        }
        let host = percent_decode(rest.trim_end_matches('/'));
        if !host.is_empty() && !out.iter().any(|(s, h)| s == scheme && *h == host) {
            out.push((scheme.to_string(), host));
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn gio_roots() -> Vec<(String, String)> {
    std::process::Command::new("gio")
        .args(["mount", "-li"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse_gio_roots(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

#[cfg(any(target_os = "linux", test))]
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// Whether a GVFS host name (`SAMSUNG_SAMSUNG_Android_RF8M1234`) belongs to this USB device:
/// by serial number when there is one, otherwise by maker and product name.
#[cfg(any(target_os = "linux", test))]
pub fn host_matches(host: &str, u: &Usb) -> bool {
    let h = norm(host);
    let serial = norm(&u.serial);
    if serial.len() >= 4 {
        return h.contains(&serial);
    }
    let product = norm(&u.product);
    !product.is_empty() && h.contains(&product) && (norm(&u.maker).is_empty() || h.contains(&norm(&u.maker)))
}

#[cfg(any(target_os = "linux", test))]
pub fn build_linux(usb: &[Usb], mounts: &[Mounted], roots: &[(String, String)]) -> Vec<Device> {
    let mut out = Vec::new();
    for u in usb {
        let Some(kind) = classify_usb(u) else { continue };
        let mounted = mounts.iter().find(|m| host_matches(&m.host, u));
        let root = roots.iter().find(|(_, h)| host_matches(h, u));
        let name = if !u.product.is_empty() {
            u.product.clone()
        } else if !u.maker.is_empty() {
            u.maker.clone()
        } else {
            format!("USB device {}:{}", u.vendor_id, u.product_id)
        };
        let phone_like = matches!(kind, DeviceKind::Phone | DeviceKind::Camera);
        let note = match (mounted.is_some(), root.is_some(), phone_like) {
            (true, ..) => String::new(),
            (false, true, _) => UNLOCK.to_string(),
            (false, false, true) => format!("{UNLOCK} File access needs gvfs (gvfs-mtp / gvfs-gphoto2) to be installed."),
            _ => String::new(),
        };
        out.push(Device {
            id: format!("usb:{}", u.id),
            name,
            maker: u.maker.clone(),
            kind,
            path: mounted.map(|m| m.path.clone()),
            connectable: mounted.is_none() && root.is_some(),
            open: None,
            note,
        });
    }
    out
}

/// Mount a device GVFS knows about and return the folder it appears at.
#[cfg(target_os = "linux")]
pub fn connect(id: &str) -> crate::error::Result<Option<String>> {
    use crate::error::CoreError;
    let usb = parse_usb(&usb_root());
    let u = usb.iter().find(|u| format!("usb:{}", u.id) == id).ok_or_else(|| CoreError::Invalid("That device is no longer connected.".into()))?;
    let roots = gio_roots();
    let (scheme, host) = roots.iter().find(|(_, h)| host_matches(h, u)).ok_or_else(|| CoreError::Invalid("The system does not offer this device for opening. Unlock it and allow file transfer, then try again.".into()))?;
    let out = std::process::Command::new("gio")
        .args(["mount", &format!("{scheme}://{host}/")])
        .output()
        .map_err(|e| CoreError::Invalid(format!("could not run gio: {e}")))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(CoreError::Invalid(if msg.is_empty() { "The device would not open. Unlock it and choose File transfer.".into() } else { msg }));
    }
    for _ in 0..20 {
        if let Some(d) = gvfs_dir() {
            if let Some(m) = gvfs_mounts(&d).into_iter().find(|m| host_matches(&m.host, u)) {
                return Ok(Some(m.path));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    Ok(None)
}

// ---------------------------------------------------------------------------
// macOS
// ---------------------------------------------------------------------------

/// Devices out of `system_profiler SPUSBDataType -json`. `has_app` says whether an
/// application (by name) is installed, to pick the tool that can open a phone.
#[cfg(any(target_os = "macos", test))]
pub fn parse_system_profiler(json: &str, has_app: impl Fn(&str) -> bool) -> Vec<Device> {
    use serde_json::Value;
    fn walk(v: &Value, out: &mut Vec<Value>) {
        if let Some(items) = v.get("_items").and_then(|i| i.as_array()) {
            for it in items {
                out.push(it.clone());
                walk(it, out);
            }
        }
    }
    let Ok(root) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let mut items = Vec::new();
    for bus in root.get("SPUSBDataType").and_then(|b| b.as_array()).into_iter().flatten() {
        walk(bus, &mut items);
    }
    let text = |it: &Value, k: &str| it.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let mut out = Vec::new();
    for it in items {
        let name = text(&it, "_name");
        // Drives are listed as drives; hubs are plumbing.
        if name.is_empty() || it.get("Media").is_some() || name.to_lowercase().contains("hub") {
            continue;
        }
        let maker = text(&it, "manufacturer");
        let vendor = text(&it, "vendor_id");
        let kind = if vendor.contains("apple_vendor_id") && (name.contains("iPhone") || name.contains("iPad") || name.contains("iPod")) {
            DeviceKind::Phone
        } else if let Some(k) = kind_from_name(&format!("{maker} {name}")) {
            k
        } else {
            let n = name.to_lowercase();
            if n.contains("keyboard") || n.contains("mouse") || n.contains("trackpad") || n.contains("receiver") {
                DeviceKind::Input
            } else if n.contains("printer") {
                DeviceKind::Printer
            } else if n.contains("audio") || n.contains("headset") || n.contains("microphone") {
                DeviceKind::Audio
            } else if n.contains("bluetooth") {
                DeviceKind::Bluetooth
            } else {
                DeviceKind::Other
            }
        };
        let apple = name.contains("iPhone") || name.contains("iPad") || name.contains("iPod");
        let (open, note) = match kind {
            DeviceKind::Phone if apple => (
                has_app("Image Capture").then(|| "Image Capture".to_string()),
                "Unlock it and tap Trust. Photos open in Image Capture; Finder does not show an iPhone's files as a folder.".to_string(),
            ),
            DeviceKind::Phone => {
                let tool = ["Android File Transfer", "OpenMTP"].into_iter().find(|a| has_app(a)).map(String::from);
                let note = if tool.is_some() { UNLOCK.to_string() } else { format!("{UNLOCK} macOS has no built-in way to browse an Android phone; install Android File Transfer or OpenMTP.") };
                (tool, note)
            }
            DeviceKind::Camera => (has_app("Image Capture").then(|| "Image Capture".to_string()), String::new()),
            _ => (None, String::new()),
        };
        let serial = text(&it, "serial_num");
        let loc = text(&it, "location_id");
        out.push(Device {
            id: format!("usb:{}:{}", if loc.is_empty() { &name } else { &loc }, serial),
            name,
            maker,
            kind,
            path: None,
            connectable: false,
            open,
            note,
        });
    }
    out
}

/// Ask macOS to open a device in `app` (see [`Device::open`]).
#[cfg(target_os = "macos")]
pub fn open_in_system(open: &str) -> crate::error::Result<()> {
    std::process::Command::new("open")
        .args(["-a", open])
        .spawn()
        .map(|_| ())
        .map_err(|e| crate::error::CoreError::Invalid(format!("could not open {open}: {e}")))
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use super::{kind_from_name, Device, DeviceKind};
    use windows::core::PWSTR;
    use windows::Win32::System::SystemServices::SFGAO_FILESYSTEM;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{
        BHID_EnumItems, FOLDERID_ComputerFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT,
        SIGDN, SIGDN_DESKTOPABSOLUTEPARSING, SIGDN_NORMALDISPLAY,
    };

    fn name_of(item: &IShellItem, kind: SIGDN) -> String {
        unsafe {
            match item.GetDisplayName(kind) {
                Ok(p) => {
                    let s = PWSTR(p.0).to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                }
                Err(_) => String::new(),
            }
        }
    }

    /// Portable devices (phones, cameras, players) are the items of "This PC" that are not
    /// file-system folders and whose shell path names a device (`\\?\usb#vid_...`). Phone Link
    /// shows up the same way when Windows exposes it there.
    pub fn list() -> Vec<Device> {
        let mut out = Vec::new();
        unsafe {
            let started = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
            let computer: windows::core::Result<IShellItem> = SHGetKnownFolderItem(&FOLDERID_ComputerFolder, KF_FLAG_DEFAULT, None);
            if let Ok(computer) = computer {
                if let Ok(en) = computer.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) {
                    loop {
                        let mut slot: [Option<IShellItem>; 1] = [None];
                        let mut got = 0u32;
                        if en.Next(&mut slot, Some(&mut got)).is_err() || got == 0 {
                            break;
                        }
                        let Some(item) = slot[0].take() else { break };
                        // Drives and folders are listed by the drives code.
                        if item.GetAttributes(SFGAO_FILESYSTEM).map(|a| a.0 != 0).unwrap_or(false) {
                            continue;
                        }
                        let parsing = name_of(&item, SIGDN_DESKTOPABSOLUTEPARSING);
                        let lower = parsing.to_lowercase();
                        let device = lower.contains("\\\\?\\") || lower.contains("usb#") || lower.contains("wpdbusenum") || lower.contains("{6ac27878-a6fa-4155-ba85-f98f491d4f33}");
                        let name = name_of(&item, SIGDN_NORMALDISPLAY);
                        if !device || name.is_empty() {
                            continue;
                        }
                        let kind = kind_from_name(&name).unwrap_or(DeviceKind::Phone);
                        out.push(Device {
                            id: parsing.clone(),
                            name,
                            maker: String::new(),
                            kind,
                            path: None,
                            connectable: false,
                            open: Some(parsing),
                            note: "Opens in File Explorer. Unlock the phone and choose File transfer if it looks empty.".into(),
                        });
                    }
                }
            }
            if started {
                CoUninitialize();
            }
        }
        out
    }

    /// Open a shell location (as `Device::open` gives it) in File Explorer.
    pub fn open(target: &str) -> crate::error::Result<()> {
        std::process::Command::new("explorer.exe")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|e| crate::error::CoreError::Invalid(format!("could not open File Explorer: {e}")))
    }
}

#[cfg(windows)]
pub use win::open as open_in_system;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn usb(id: &str, maker: &str, product: &str, serial: &str, classes: &[u8]) -> Usb {
        Usb {
            id: id.into(), vendor_id: "18d1".into(), product_id: "4ee1".into(), maker: maker.into(), product: product.into(),
            serial: serial.into(), device_class: 0, interfaces: classes.iter().map(|c| (*c, String::new())).collect(),
        }
    }

    #[test]
    fn reads_a_sysfs_tree() {
        let d = tempfile::tempdir().unwrap();
        let w = |p: &str, v: &str| {
            let f = d.path().join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, v).unwrap();
        };
        w("1-2/idVendor", "18d1\n");
        w("1-2/idProduct", "4ee1\n");
        w("1-2/manufacturer", "Google\n");
        w("1-2/product", "Pixel 8\n");
        w("1-2/serial", "ABC12345\n");
        w("1-2/bDeviceClass", "00\n");
        w("1-2:1.0/bInterfaceClass", "06\n");
        w("1-2:1.0/interface", "MTP\n");
        w("1-3/product", "USB Hub\n");
        w("1-3/bDeviceClass", "09\n");
        w("usb1/product", "xHCI Host Controller\n");
        let found = parse_usb(d.path());
        assert_eq!(found.len(), 2, "root hubs and interfaces are not devices: {found:?}");
        let phone = found.iter().find(|u| u.id == "1-2").unwrap();
        assert_eq!((phone.maker.as_str(), phone.product.as_str(), phone.serial.as_str()), ("Google", "Pixel 8", "ABC12345"));
        assert_eq!(phone.interfaces, vec![(6, "MTP".to_string())]);
        assert_eq!(classify_usb(phone), Some(DeviceKind::Phone));
        assert_eq!(classify_usb(found.iter().find(|u| u.id == "1-3").unwrap()), None);
    }

    #[test]
    fn classifies_by_interface_and_name() {
        assert_eq!(classify_usb(&usb("1", "Canon", "EOS R6", "", &[6])), Some(DeviceKind::Camera));
        assert_eq!(classify_usb(&usb("1", "Kingston", "DataTraveler", "", &[8])), None);
        assert_eq!(classify_usb(&usb("1", "Logitech", "USB Receiver", "", &[3])), Some(DeviceKind::Input));
        assert_eq!(classify_usb(&usb("1", "HP", "LaserJet", "", &[7])), Some(DeviceKind::Printer));
        assert_eq!(classify_usb(&usb("1", "Samsung", "SAMSUNG_Android", "", &[0xff])), Some(DeviceKind::Phone));
        let mut apple = usb("1", "Apple Inc.", "iPhone", "", &[0xff]);
        apple.vendor_id = "05ac".into();
        assert_eq!(classify_usb(&apple), Some(DeviceKind::Phone));
        assert!(!is_main(DeviceKind::Input) && is_main(DeviceKind::Phone));
    }

    #[test]
    fn finds_mounted_and_mountable_phones() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("mtp:host=Google_Pixel_8_ABC12345")).unwrap();
        fs::create_dir(d.path().join("smb-share:server=nas,share=x")).unwrap();
        let mounts = gvfs_mounts(d.path());
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].host, "Google_Pixel_8_ABC12345");

        let pixel = usb("1-2", "Google", "Pixel 8", "ABC12345", &[6]);
        let other = usb("1-4", "Sony", "Xperia 1", "ZZ998877", &[6]);
        let roots = vec![("mtp".to_string(), "Sony_Xperia_1_ZZ998877".to_string())];
        let list = build_linux(&[pixel, other], &mounts, &roots);
        let p = list.iter().find(|x| x.name == "Pixel 8").unwrap();
        assert_eq!(p.path.as_deref(), Some(mounts[0].path.as_str()));
        assert!(!p.connectable && p.note.is_empty());
        let x = list.iter().find(|x| x.name == "Xperia 1").unwrap();
        assert!(x.path.is_none() && x.connectable);
        assert!(x.note.contains("File transfer"));
    }

    #[test]
    fn a_phone_with_no_gvfs_says_what_is_missing() {
        let list = build_linux(&[usb("1-2", "Google", "Pixel 8", "ABC12345", &[6])], &[], &[]);
        assert!(list[0].path.is_none() && !list[0].connectable);
        assert!(list[0].note.contains("gvfs"));
    }

    #[test]
    fn parses_gio_output() {
        let text = "Drive(0): Pixel 8\n  Type: GProxyDrive (GProxyVolumeMonitorMTP)\n  Volume(0): Pixel 8\n    Type: GProxyVolume (GProxyVolumeMonitorMTP)\n    activation_root=mtp://Google_Pixel_8_ABC12345/\n    themed icons:  [phone]\nMount(0): x -> file:///x\n";
        assert_eq!(parse_gio_roots(text), vec![("mtp".to_string(), "Google_Pixel_8_ABC12345".to_string())]);
        assert!(parse_gio_roots("activation_root=file:///x/\n").is_empty());
    }

    #[test]
    fn host_names_match_by_serial_then_by_name() {
        let with_serial = usb("1", "Google", "Pixel 8", "ABC12345", &[6]);
        assert!(host_matches("Google_Pixel_8_ABC12345", &with_serial));
        assert!(!host_matches("Google_Pixel_8_OTHER999", &with_serial));
        let no_serial = usb("1", "Google", "Pixel 8", "", &[6]);
        assert!(host_matches("Google_Pixel_8_XYZ", &no_serial));
        assert!(!host_matches("Sony_Xperia_1", &no_serial));
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn reads_system_profiler_json() {
        let json = r#"{"SPUSBDataType":[{"_name":"USB31Bus","_items":[
          {"_name":"USB3.0 Hub","_items":[
             {"_name":"Pixel 8","manufacturer":"Google","serial_num":"ABC","location_id":"0x01100000 / 3","vendor_id":"0x18d1  (Google Inc.)"},
             {"_name":"iPhone","manufacturer":"Apple Inc.","serial_num":"0000","location_id":"0x01200000 / 4","vendor_id":"apple_vendor_id"},
             {"_name":"Magic Keyboard","manufacturer":"Apple Inc.","location_id":"0x01300000 / 5"},
             {"_name":"Flash Drive","Media":[{"_name":"x"}],"location_id":"0x01400000 / 6"}]}]}]}"#;
        let list = parse_system_profiler(json, |a| a == "Android File Transfer" || a == "Image Capture");
        let names: Vec<&str> = list.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Pixel 8", "iPhone", "Magic Keyboard"], "hubs and drives are left out");
        assert_eq!(list[0].kind, DeviceKind::Phone);
        assert_eq!(list[0].open.as_deref(), Some("Android File Transfer"));
        assert_eq!(list[1].open.as_deref(), Some("Image Capture"));
        assert_eq!(list[2].kind, DeviceKind::Input);
        let bare = parse_system_profiler(json, |_| false);
        assert!(bare[0].open.is_none() && bare[0].note.contains("install"));
        assert!(parse_system_profiler("not json", |_| true).is_empty());
    }

    #[test]
    fn names_say_what_a_device_is() {
        assert_eq!(kind_from_name("Galaxy S24"), Some(DeviceKind::Phone));
        assert_eq!(kind_from_name("GoPro HERO12"), Some(DeviceKind::Camera));
        assert_eq!(kind_from_name("Desk Lamp"), None);
    }

    #[test]
    fn listing_does_not_fail_here() {
        let _ = list_devices(true);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        signature(&mut h);
    }
}
