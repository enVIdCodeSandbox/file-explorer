//! Whether this computer is on a wired (Ethernet) connection, so that things which read many
//! files over the network, such as thumbnails of network shares, can be switched on only then.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Link {
    /// The connection that carries internet traffic is Ethernet.
    Wired,
    /// Wi-Fi, mobile data or anything else that is not a cable.
    Wireless,
    /// Could not tell (not connected, or the system would not say).
    Unknown,
}

/// The kind of connection in use. Looked at at most every few seconds.
pub fn link() -> Link {
    static LAST: parking_lot::Mutex<Option<(Instant, Link)>> = parking_lot::Mutex::new(None);
    let mut last = LAST.lock();
    if let Some((at, l)) = *last {
        if at.elapsed() < Duration::from_secs(4) {
            return l;
        }
    }
    let l = detect();
    *last = Some((Instant::now(), l));
    l
}

/// Whether an interface name that the system gives for a hardware port is a cable.
/// macOS: the port names in `networksetup -listallhardwareports`.
pub fn port_is_wired(port: &str) -> Option<bool> {
    let p = port.to_lowercase();
    if p.contains("wi-fi") || p.contains("wifi") || p.contains("wlan") || p.contains("airport") || p.contains("bluetooth") || p.contains("iphone") || p.contains("ipad") || p.contains("bridge") || p.contains("vpn") {
        Some(false)
    } else if p.contains("ethernet") || p.contains("lan") || p.contains("thunderbolt") && p.contains("eth") {
        Some(true)
    } else {
        None
    }
}

/// `networksetup -listallhardwareports` text -> the hardware port name of `device` (`en0`).
pub fn port_of_device(listing: &str, device: &str) -> Option<String> {
    let mut port = None;
    for line in listing.lines() {
        if let Some(v) = line.strip_prefix("Hardware Port:") {
            port = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Device:") {
            if v.trim() == device {
                return port;
            }
        }
    }
    None
}

/// `route -n get default` text -> the interface it names.
pub fn default_interface(route_output: &str) -> Option<String> {
    route_output.lines().find_map(|l| l.trim().strip_prefix("interface:").map(|v| v.trim().to_string()))
}

/// Linux: the interface of the default route, from `/proc/net/route`.
pub fn default_route_interface(table: &str) -> Option<String> {
    let mut best: Option<(u32, String)> = None;
    for line in table.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 7 || f[1] != "00000000" {
            continue;
        }
        let metric: u32 = f[6].parse().unwrap_or(u32::MAX);
        if best.as_ref().map_or(true, |(m, _)| metric < *m) {
            best = Some((metric, f[0].to_string()));
        }
    }
    best.map(|(_, i)| i)
}

#[cfg(target_os = "linux")]
fn detect() -> Link {
    use std::path::Path;
    let sys = Path::new("/sys/class/net");
    let is_wired = |name: &str| -> bool {
        let d = sys.join(name);
        // Wi-Fi, bridges, tunnels and virtual cards are not a cable.
        d.join("device").exists() && !d.join("wireless").exists() && !d.join("phy80211").exists() && !d.join("bridge").exists() && std::fs::read_to_string(d.join("type")).map(|t| t.trim() == "1").unwrap_or(false)
    };
    if let Some(name) = std::fs::read_to_string("/proc/net/route").ok().and_then(|t| default_route_interface(&t)) {
        return if is_wired(&name) { Link::Wired } else { Link::Wireless };
    }
    // No default route: wired if a cable card has a carrier.
    let any = std::fs::read_dir(sys)
        .map(|rd| rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            is_wired(&n) && std::fs::read_to_string(e.path().join("carrier")).map(|c| c.trim() == "1").unwrap_or(false)
        }))
        .unwrap_or(false);
    if any { Link::Wired } else { Link::Unknown }
}

#[cfg(target_os = "macos")]
fn detect() -> Link {
    let run = |cmd: &str, args: &[&str]| std::process::Command::new(cmd).args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    let Some(device) = run("route", &["-n", "get", "default"]).and_then(|t| default_interface(&t)) else { return Link::Unknown };
    let Some(port) = run("networksetup", &["-listallhardwareports"]).and_then(|t| port_of_device(&t, &device)) else { return Link::Unknown };
    match port_is_wired(&port) {
        Some(true) => Link::Wired,
        Some(false) => Link::Wireless,
        None => Link::Unknown,
    }
}

#[cfg(windows)]
fn detect() -> Link {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, IP_ADAPTER_ADDRESSES_LH};
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    const AF_UNSPEC: u32 = 0;
    let mut len = 16 * 1024u32;
    let mut buf: Vec<u64> = vec![0; (len as usize).div_ceil(8)];
    // SAFETY: the buffer is at least `len` bytes and 8-byte aligned; Windows fills it with a linked list that is only read while `buf` is alive.
    unsafe {
        let mut rc = GetAdaptersAddresses(AF_UNSPEC, GAA_FLAG_INCLUDE_GATEWAYS, std::ptr::null(), buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut len);
        if rc == 111 {
            // ERROR_BUFFER_OVERFLOW: `len` now says how much is needed.
            buf = vec![0; (len as usize).div_ceil(8)];
            rc = GetAdaptersAddresses(AF_UNSPEC, GAA_FLAG_INCLUDE_GATEWAYS, std::ptr::null(), buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut len);
        }
        if rc != 0 {
            return Link::Unknown;
        }
        let (mut wired, mut other) = (false, false);
        let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !p.is_null() {
            let a = &*p;
            // Only adapters that are up and lead somewhere (they have a gateway) carry traffic.
            if a.OperStatus == IfOperStatusUp && !a.FirstGatewayAddress.is_null() {
                // Ethernet in all its speeds (a fibre line ends in an Ethernet port too). Wi-Fi, mobile data and tunnels are not a cable.
                if matches!(a.IfType, 6 | 62 | 69 | 117) {
                    wired = true;
                } else {
                    other = true;
                }
            }
            p = a.Next;
        }
        if wired {
            Link::Wired
        } else if other {
            Link::Wireless
        } else {
            Link::Unknown
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn detect() -> Link {
    Link::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_default_route() {
        let t = "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\n\
                 wlan0\t00000000\t0102A8C0\t0003\t0\t0\t600\t00000000\n\
                 enp3s0\t00000000\t0102A8C0\t0003\t0\t0\t100\t00000000\n\
                 enp3s0\t0002A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\n";
        assert_eq!(default_route_interface(t).as_deref(), Some("enp3s0"), "the lowest metric wins");
        assert_eq!(default_route_interface("Iface\tDestination\n"), None);
    }

    #[test]
    fn reads_macos_ports() {
        let listing = "Hardware Port: Wi-Fi\nDevice: en0\nEthernet Address: aa\n\nHardware Port: USB 10/100/1000 LAN\nDevice: en7\nEthernet Address: bb\n\nHardware Port: Thunderbolt Bridge\nDevice: bridge0\n";
        assert_eq!(port_of_device(listing, "en7").as_deref(), Some("USB 10/100/1000 LAN"));
        assert_eq!(port_of_device(listing, "en9"), None);
        assert_eq!(port_is_wired("USB 10/100/1000 LAN"), Some(true));
        assert_eq!(port_is_wired("Ethernet"), Some(true));
        assert_eq!(port_is_wired("Wi-Fi"), Some(false));
        assert_eq!(port_is_wired("Thunderbolt Bridge"), Some(false));
        assert_eq!(port_is_wired("iPhone USB"), Some(false));
        assert_eq!(port_is_wired("Something odd"), None);
        assert_eq!(default_interface("   route to: default\ninterface: en7\n  flags: <UP>").as_deref(), Some("en7"));
    }

    #[test]
    fn answers_without_failing() {
        let _ = link();
        assert_eq!(link(), link(), "answers are reused for a few seconds");
    }
}
