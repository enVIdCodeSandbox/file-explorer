//! A detailed report about this computer for the About page: operating
//! system, processor, memory, graphics, battery, what is running, and the
//! health and expected life of each physical drive.
//!
//! Everything here is best effort and read-only. Anything the machine will not
//! tell us (no administrator rights, no `smartctl`, no GPU driver data) is
//! left out and, where it matters, explained in `notes` rather than guessed.
//! Parsers are plain functions over text so they can be tested anywhere.

#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
    pub host: String,
    pub uptime_secs: u64,
    pub desktop: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub board: Option<String>,
    pub firmware: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CpuInfo {
    pub brand: String,
    pub vendor: String,
    pub physical_cores: Option<usize>,
    pub logical: usize,
    pub mhz: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct MemInfo {
    pub total: u64,
    pub available: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    /// "16 GB DDR4-3200" style lines, one per module, when the OS lists them.
    pub modules: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct BatteryInfo {
    pub percent: f32,
    pub state: String,
    pub health_percent: Option<f32>,
    pub cycles: Option<u32>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub technology: Option<String>,
    pub design_wh: Option<f32>,
    pub full_wh: Option<f32>,
    pub lifespan: Option<Lifespan>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Lifespan {
    /// One sentence a person can read.
    pub summary: String,
    /// Estimated years left; None when there is nothing to project from.
    pub years_left: Option<f32>,
    /// "projection" from a real wear counter, or "rule of thumb" when it is only a guide.
    pub basis: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhysicalDisk {
    pub name: String,
    pub model: String,
    pub kind: String,
    pub bus: Option<String>,
    pub size: u64,
    pub health: Option<String>,
    pub temperature_c: Option<f32>,
    pub power_on_hours: Option<u64>,
    /// Share of the rated write endurance already used (SSDs).
    pub wear_percent: Option<f32>,
    pub bytes_written: Option<u64>,
    pub reallocated_sectors: Option<u64>,
    pub media_errors: Option<u64>,
    pub lifespan: Option<Lifespan>,
    /// Where the numbers came from, or why there are none.
    pub source: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Proc {
    pub name: String,
    pub memory: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Software {
    pub process_count: usize,
    pub biggest: Vec<Proc>,
    pub own_memory: u64,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub os: OsInfo,
    pub machine: Machine,
    pub cpu: CpuInfo,
    pub memory: MemInfo,
    pub gpus: Vec<String>,
    pub battery: Option<BatteryInfo>,
    pub disks: Vec<PhysicalDisk>,
    pub software: Software,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Expected life
// ---------------------------------------------------------------------------

const HOURS_PER_YEAR: f32 = 8766.0;

/// Rough remaining life of a drive. With an SSD wear counter this is a
/// projection of the drive's own pace; without one it is only a rule of
/// thumb, and says so.
pub fn estimate_drive_life(kind: &str, hours: Option<u64>, wear: Option<f32>, health: Option<&str>) -> Option<Lifespan> {
    let bad = health.map(|h| !matches!(h.to_lowercase().as_str(), "healthy" | "ok" | "passed" | "verified")).unwrap_or(false);
    if bad {
        return Some(Lifespan {
            summary: "The drive reports a problem. Back up what matters now and plan to replace it.".into(),
            years_left: Some(0.0),
            basis: "drive-reported status".into(),
        });
    }
    let ssd = kind.contains("SSD");
    if let (Some(p), Some(h)) = (wear, hours) {
        if ssd && h > 0 {
            if p <= 0.5 {
                return Some(Lifespan {
                    summary: format!("Almost no wear yet ({p:.0}% of rated writes used after {}).", span(h)),
                    years_left: None,
                    basis: "projection".into(),
                });
            }
            let remaining_hours = (100.0 - p).max(0.0) / (p / h as f32);
            let years = remaining_hours / HOURS_PER_YEAR;
            let summary = if years > 20.0 {
                format!("{p:.0}% of rated writes used after {}. At this pace it should outlast 20 years.", span(h))
            } else {
                format!("{p:.0}% of rated writes used after {}. At this pace, about {years:.0} more years.", span(h))
            };
            return Some(Lifespan { summary, years_left: Some(years.min(99.0)), basis: "projection".into() });
        }
    }
    let h = hours?;
    let age = h as f32 / HOURS_PER_YEAR;
    if ssd {
        return Some(Lifespan {
            summary: format!("Powered on for {}. This drive does not report wear, so no projection is possible. SSDs commonly last 5 to 10 years.", span(h)),
            years_left: None,
            basis: "rule of thumb".into(),
        });
    }
    let left = (5.0 - age).max(0.0);
    Some(Lifespan {
        summary: format!(
            "Powered on for {}. Hard drives are commonly good for 3 to 5 years of use, and failures get more likely with age. Keep backups.",
            span(h)
        ),
        years_left: Some(left),
        basis: "rule of thumb".into(),
    })
}

fn span(hours: u64) -> String {
    let days = hours / 24;
    if days >= 730 {
        format!("{:.1} years", hours as f32 / HOURS_PER_YEAR)
    } else if days >= 60 {
        format!("{} months", (days as f32 / 30.4).round() as u64)
    } else {
        format!("{hours} hours")
    }
}

/// Battery life from its health (full-charge capacity against design) and
/// charge cycles: most laptop batteries are rated for about 500 to 1000 cycles.
pub fn estimate_battery_life(health: Option<f32>, cycles: Option<u32>) -> Option<Lifespan> {
    match (health, cycles) {
        (Some(h), Some(c)) if c > 20 && h < 100.0 => {
            let lost = (100.0 - h).max(0.1);
            let to_80 = ((h - 80.0).max(0.0) / lost * c as f32).round();
            let summary = if h <= 80.0 {
                format!("Holds {h:.0}% of its original charge after {c} cycles. It has passed the point where it is usually worth replacing.")
            } else {
                format!("Holds {h:.0}% of its original charge after {c} cycles. About {to_80:.0} more cycles until it reaches 80%.")
            };
            Some(Lifespan { summary, years_left: None, basis: "projection".into() })
        }
        (Some(h), _) => Some(Lifespan { summary: format!("Holds {h:.0}% of its original charge."), years_left: None, basis: "projection".into() }),
        (None, Some(c)) => Some(Lifespan { summary: format!("{c} charge cycles. Most batteries are rated for 500 to 1000."), years_left: None, basis: "rule of thumb".into() }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parsers (pure)
// ---------------------------------------------------------------------------

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(|x| x.as_f64())
}
/// PowerShell turns a one-item list into a bare object; accept both.
fn list(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Null => vec![],
        other => vec![other],
    }
}

/// What the Windows PowerShell script prints, as the pieces of the report.
pub struct WindowsData {
    pub machine: Machine,
    pub gpus: Vec<String>,
    pub modules: Vec<String>,
    pub disks: Vec<PhysicalDisk>,
}

pub fn parse_windows(json: &str) -> Option<WindowsData> {
    let v: Value = serde_json::from_str(json.trim().trim_start_matches('\u{feff}')).ok()?;
    let machine = Machine {
        manufacturer: text(&v["system"], "Manufacturer"),
        model: text(&v["system"], "Model"),
        board: match (text(&v["board"], "Manufacturer"), text(&v["board"], "Product")) {
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (None, Some(b)) => Some(b),
            _ => None,
        },
        firmware: text(&v["bios"], "SMBIOSBIOSVersion").map(|ver| match text(&v["bios"], "Manufacturer") {
            Some(m) => format!("{m} {ver}"),
            None => ver,
        }),
    };
    let gpus = list(&v["gpu"])
        .into_iter()
        .filter_map(|g| {
            let name = text(g, "Name")?;
            let ram = num(g, "AdapterRAM").filter(|r| *r > 0.0 && *r < 4_294_967_295.0);
            let driver = text(g, "DriverVersion");
            Some(match (ram, driver) {
                (Some(r), Some(d)) => format!("{name} · {:.0} GB · driver {d}", r / 1_073_741_824.0),
                (None, Some(d)) => format!("{name} · driver {d}"),
                _ => name,
            })
        })
        .collect();
    let modules = list(&v["memory"])
        .into_iter()
        .filter_map(|m| {
            let gb = num(m, "Capacity")? / 1_073_741_824.0;
            let speed = num(m, "Speed").filter(|s| *s > 0.0);
            let maker = text(m, "Manufacturer").filter(|s| !s.starts_with("0000") && s != "Unknown");
            Some(format!(
                "{gb:.0} GB{}{}",
                speed.map(|s| format!(" at {s:.0} MT/s")).unwrap_or_default(),
                maker.map(|m| format!(" · {m}")).unwrap_or_default()
            ))
        })
        .collect();
    let disks = list(&v["disks"])
        .into_iter()
        .map(|d| {
            let media = text(d, "Media").unwrap_or_default();
            let bus = text(d, "Bus");
            let kind = match (media.as_str(), bus.as_deref()) {
                (_, Some("USB")) => "External (USB)".to_string(),
                ("SSD", Some("NVMe")) | (_, Some("NVMe")) => "NVMe SSD".to_string(),
                ("SSD", _) => "SSD".to_string(),
                ("HDD", _) => "HDD".to_string(),
                _ => "Unknown".to_string(),
            };
            let health = text(d, "Health");
            let hours = num(d, "Hours").map(|h| h as u64);
            let wear = num(d, "Wear").map(|w| w as f32);
            let any_counter = hours.is_some() || wear.is_some() || num(d, "Temp").is_some();
            PhysicalDisk {
                name: text(d, "Name").unwrap_or_else(|| "Disk".into()),
                model: text(d, "Model").or_else(|| text(d, "Name")).unwrap_or_default(),
                lifespan: estimate_drive_life(&kind, hours, wear, health.as_deref()),
                kind,
                bus,
                size: num(d, "Size").unwrap_or(0.0) as u64,
                health,
                temperature_c: num(d, "Temp").filter(|t| *t > 0.0).map(|t| t as f32),
                power_on_hours: hours,
                wear_percent: wear,
                bytes_written: None,
                reallocated_sectors: None,
                media_errors: num(d, "ReadErrors").map(|e| e as u64),
                source: if any_counter {
                    "Windows storage reliability counters".into()
                } else {
                    "Windows reports health only; this drive gives no detailed counters (running as administrator may reveal more)".into()
                },
            }
        })
        .collect();
    Some(WindowsData { machine, gpus, modules, disks })
}

/// Fields read from `smartctl -j -a`.
#[derive(Debug, Default, PartialEq)]
pub struct Smart {
    pub passed: Option<bool>,
    pub temperature_c: Option<f32>,
    pub power_on_hours: Option<u64>,
    pub wear_percent: Option<f32>,
    pub bytes_written: Option<u64>,
    pub reallocated: Option<u64>,
    pub media_errors: Option<u64>,
    pub rotational: Option<bool>,
    pub nvme: bool,
    pub model: Option<String>,
}

pub fn parse_smartctl(json: &str) -> Option<Smart> {
    let v: Value = serde_json::from_str(json).ok()?;
    // smartctl prints a JSON error object when it cannot read the device.
    if v.get("smart_status").is_none() && v.get("nvme_smart_health_information_log").is_none() && v.get("ata_smart_attributes").is_none() {
        return None;
    }
    let mut s = Smart {
        passed: v["smart_status"]["passed"].as_bool(),
        temperature_c: v["temperature"]["current"].as_f64().map(|t| t as f32),
        power_on_hours: v["power_on_time"]["hours"].as_u64(),
        model: text(&v, "model_name"),
        nvme: v["device"]["protocol"].as_str() == Some("NVMe") || v.get("nvme_smart_health_information_log").is_some(),
        rotational: v["rotation_rate"].as_u64().map(|r| r > 0),
        ..Smart::default()
    };
    if let Some(log) = v.get("nvme_smart_health_information_log") {
        s.wear_percent = log["percentage_used"].as_f64().map(|p| p as f32);
        s.bytes_written = log["data_units_written"].as_u64().map(|u| u.saturating_mul(512_000));
        s.media_errors = log["media_errors"].as_u64();
        s.power_on_hours = s.power_on_hours.or(log["power_on_hours"].as_u64());
    }
    if let Some(table) = v["ata_smart_attributes"]["table"].as_array() {
        for a in table {
            let id = a["id"].as_u64().unwrap_or(0);
            let norm = a["value"].as_f64();
            let raw = a["raw"]["value"].as_u64();
            match id {
                5 => s.reallocated = raw,
                // Normalized "life left" counters used by SSD makers: 100 is new.
                177 | 231 | 233 if s.wear_percent.is_none() => s.wear_percent = norm.map(|n| (100.0 - n).clamp(0.0, 100.0) as f32),
                _ => {}
            }
        }
    }
    Some(s)
}

/// Fields read from `diskutil info` text on macOS.
#[derive(Debug, Default, PartialEq)]
pub struct DiskutilInfo {
    pub name: Option<String>,
    pub solid_state: Option<bool>,
    pub smart: Option<String>,
    pub size: Option<u64>,
    pub protocol: Option<String>,
    pub internal: Option<bool>,
}

pub fn parse_diskutil(out: &str) -> DiskutilInfo {
    let mut d = DiskutilInfo::default();
    for line in out.lines() {
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Device / Media Name" => d.name = Some(v.to_string()),
            "Solid State" => d.solid_state = Some(v.eq_ignore_ascii_case("yes")),
            "SMART Status" => d.smart = Some(v.to_string()),
            "Protocol" => d.protocol = Some(v.to_string()),
            "Internal" | "Device Location" => d.internal = Some(v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("internal")),
            "Disk Size" | "Total Size" => {
                // "500.3 GB (500277792768 Bytes) (exactly ...)"
                if let Some(open) = v.find('(') {
                    d.size = v[open + 1..].split_whitespace().next().and_then(|n| n.parse().ok());
                }
            }
            _ => {}
        }
    }
    d
}

// ---------------------------------------------------------------------------
// Running helper programs without ever hanging the About page
// ---------------------------------------------------------------------------

/// Run a program, collect its output, and give up after `secs`.
fn run(program: &str, args: &[&str], secs: u64) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // no console window
    }
    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(Duration::from_millis(30)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let bytes = reader.join().ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned()).filter(|s| !s.trim().is_empty())
}

#[cfg(windows)]
const WINDOWS_SCRIPT: &str = r#"
$ErrorActionPreference='SilentlyContinue'
$disks=@(Get-PhysicalDisk | ForEach-Object {
  $r = $_ | Get-StorageReliabilityCounter
  [pscustomobject]@{ Name=$_.FriendlyName; Model=$_.Model; Media=[string]$_.MediaType; Bus=[string]$_.BusType;
    Size=$_.Size; Health=[string]$_.HealthStatus; Temp=$r.Temperature; Hours=$r.PowerOnHours; Wear=$r.Wear;
    ReadErrors=$r.ReadErrorsTotal }
})
@{
  system = Get-CimInstance Win32_ComputerSystem | Select-Object Manufacturer,Model
  bios   = Get-CimInstance Win32_BIOS | Select-Object Manufacturer,SMBIOSBIOSVersion
  board  = Get-CimInstance Win32_BaseBoard | Select-Object Manufacturer,Product
  gpu    = @(Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion)
  memory = @(Get-CimInstance Win32_PhysicalMemory | Select-Object Manufacturer,Speed,Capacity)
  disks  = $disks
} | ConvertTo-Json -Depth 4 -Compress
"#;

// ---------------------------------------------------------------------------
// Gathering
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn collect() -> SystemReport {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let mut report = SystemReport::default();
    let cpu = sys.cpus().first();
    report.cpu = CpuInfo {
        brand: cpu.map(|c| c.brand().trim().to_string()).unwrap_or_default(),
        vendor: cpu.map(|c| c.vendor_id().to_string()).unwrap_or_default(),
        physical_cores: sys.physical_core_count(),
        logical: sys.cpus().len().max(1),
        mhz: cpu.map(|c| c.frequency()).unwrap_or(0),
    };
    report.memory = MemInfo {
        total: sys.total_memory(),
        available: sys.available_memory(),
        swap_total: sys.total_swap(),
        swap_used: sys.used_swap(),
        modules: vec![],
    };
    report.os = OsInfo {
        name: System::long_os_version().or_else(System::name).unwrap_or_else(|| std::env::consts::OS.into()),
        version: System::os_version().unwrap_or_default(),
        kernel: System::kernel_version().unwrap_or_default(),
        arch: std::env::consts::ARCH.into(),
        host: System::host_name().unwrap_or_default(),
        uptime_secs: System::uptime(),
        desktop: std::env::var("XDG_CURRENT_DESKTOP").ok().filter(|s| !s.is_empty()),
    };

    let own = sysinfo::get_current_pid().ok();
    let mut procs: Vec<Proc> = sys
        .processes()
        .values()
        .map(|p| Proc { name: p.name().to_string_lossy().into_owned(), memory: p.memory() })
        .collect();
    report.software.process_count = procs.len();
    report.software.own_memory = own.and_then(|pid| sys.process(pid)).map(|p| p.memory()).unwrap_or(0);
    // Combine helper processes of the same program (browsers run dozens).
    procs.sort_by(|a, b| a.name.cmp(&b.name));
    let mut merged: Vec<Proc> = Vec::new();
    for p in procs {
        match merged.last_mut() {
            Some(last) if last.name == p.name => last.memory += p.memory,
            _ => merged.push(p),
        }
    }
    merged.sort_by(|a, b| b.memory.cmp(&a.memory));
    merged.truncate(8);
    report.software.biggest = merged;

    battery(&mut report);
    platform_details(&mut report);
    report
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn collect() -> SystemReport {
    SystemReport { notes: vec!["Detailed hardware information is not available on this platform yet.".into()], ..SystemReport::default() }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn battery(report: &mut SystemReport) {
    use starship_battery::Manager;
    let Ok(manager) = Manager::new() else { return };
    let Ok(mut all) = manager.batteries() else { return };
    let Some(Ok(b)) = all.next() else { return };
    let design = b.energy_full_design().value;
    let full = b.energy_full().value;
    let health = if design > 0.0 { Some((full / design * 100.0).min(100.0)) } else { None };
    // Joules to watt-hours.
    let wh = |j: f32| (j > 0.0).then_some(j / 3600.0);
    let cycles = b.cycle_count();
    report.battery = Some(BatteryInfo {
        percent: b.state_of_charge().value * 100.0,
        state: format!("{:?}", b.state()),
        health_percent: health,
        cycles,
        vendor: b.vendor().map(str::to_string),
        model: b.model().map(str::to_string),
        technology: Some(format!("{:?}", b.technology())).filter(|t| t != "Unknown"),
        design_wh: wh(design),
        full_wh: wh(full),
        lifespan: estimate_battery_life(health, cycles),
    });
}

#[cfg(windows)]
fn platform_details(report: &mut SystemReport) {
    let Some(out) = run("powershell", &["-NoProfile", "-NonInteractive", "-Command", WINDOWS_SCRIPT], 25) else {
        report.notes.push("Windows did not answer the hardware query in time, so machine, graphics and drive details are missing.".into());
        return;
    };
    match parse_windows(&out) {
        Some(w) => {
            report.machine = w.machine;
            report.gpus = w.gpus;
            report.memory.modules = w.modules;
            report.disks = w.disks;
        }
        None => report.notes.push("Could not read the hardware details Windows returned.".into()),
    }
}

#[cfg(target_os = "linux")]
fn platform_details(report: &mut SystemReport) {
    use std::fs;
    let dmi = |f: &str| fs::read_to_string(format!("/sys/class/dmi/id/{f}")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != "To Be Filled By O.E.M." && s != "Default string");
    report.machine = Machine {
        manufacturer: dmi("sys_vendor"),
        model: dmi("product_name"),
        board: match (dmi("board_vendor"), dmi("board_name")) {
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (_, b) => b,
        },
        firmware: dmi("bios_version").map(|v| match dmi("bios_vendor") {
            Some(m) => format!("{m} {v}"),
            None => v,
        }),
    };

    // Graphics: ask lspci when present, otherwise read the DRM device names.
    if let Some(out) = run("lspci", &[], 5) {
        report.gpus = out
            .lines()
            .filter(|l| l.contains("VGA compatible") || l.contains("3D controller") || l.contains("Display controller"))
            .filter_map(|l| l.splitn(3, ": ").nth(1).map(|s| s.trim().to_string()))
            .collect();
    }

    // Physical drives from /sys/block, with SMART data when smartctl may read it.
    let mut smart_denied = false;
    let mut smart_missing = false;
    let mut names: Vec<String> = fs::read_dir("/sys/block").map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    names.sort();
    for name in names {
        if ["loop", "ram", "dm-", "zram", "sr", "md", "nbd", "fd"].iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let read = |f: &str| fs::read_to_string(format!("/sys/block/{name}/{f}")).ok().map(|s| s.trim().to_string());
        let sectors: u64 = read("size").and_then(|s| s.parse().ok()).unwrap_or(0);
        if sectors == 0 {
            continue;
        }
        let rotational = read("queue/rotational").map(|s| s == "1").unwrap_or(false);
        let removable = read("removable").map(|s| s == "1").unwrap_or(false);
        let nvme = name.starts_with("nvme");
        let kind = if nvme { "NVMe SSD" } else if removable { "External" } else if rotational { "HDD" } else { "SSD" };
        let model = read("device/model").or_else(|| read("device/name")).unwrap_or_else(|| name.clone());
        let mut disk = PhysicalDisk {
            name: name.clone(),
            model,
            kind: kind.into(),
            bus: Some(if nvme { "NVMe".into() } else { "SATA/USB".into() }),
            size: sectors * 512,
            source: "Kernel device information only".into(),
            ..PhysicalDisk::default()
        };
        match run("smartctl", &["-j", "-a", &format!("/dev/{name}")], 8) {
            Some(out) => match parse_smartctl(&out) {
                Some(s) => {
                    disk.health = s.passed.map(|p| if p { "Healthy".to_string() } else { "Failing".to_string() });
                    disk.temperature_c = s.temperature_c;
                    disk.power_on_hours = s.power_on_hours;
                    disk.wear_percent = s.wear_percent;
                    disk.bytes_written = s.bytes_written;
                    disk.reallocated_sectors = s.reallocated;
                    disk.media_errors = s.media_errors;
                    disk.source = "SMART data from smartctl".into();
                    disk.lifespan = estimate_drive_life(&disk.kind, disk.power_on_hours, disk.wear_percent, disk.health.as_deref());
                }
                None => smart_denied = true,
            },
            None => smart_missing = true,
        }
        report.disks.push(disk);
    }
    if smart_missing {
        report.notes.push("Drive health needs smartmontools. Install it (for example `sudo apt install smartmontools`) and restart to see SMART data.".into());
    } else if smart_denied {
        report.notes.push("Reading drive health needs permission. Start the app as an administrator (root) to see SMART data.".into());
    }
}

#[cfg(target_os = "macos")]
fn platform_details(report: &mut SystemReport) {
    if let Some(out) = run("system_profiler", &["SPHardwareDataType", "-json"], 12) {
        if let Ok(v) = serde_json::from_str::<Value>(&out) {
            if let Some(h) = v["SPHardwareDataType"].as_array().and_then(|a| a.first()) {
                report.machine.manufacturer = Some("Apple".into());
                report.machine.model = text(h, "machine_name").map(|n| match text(h, "machine_model") {
                    Some(m) => format!("{n} ({m})"),
                    None => n,
                });
                report.machine.firmware = text(h, "boot_rom_version");
                if let Some(chip) = text(h, "chip_type") {
                    report.cpu.brand = chip;
                }
            }
        }
    }
    if let Some(out) = run("system_profiler", &["SPDisplaysDataType", "-json"], 12) {
        if let Ok(v) = serde_json::from_str::<Value>(&out) {
            report.gpus = v["SPDisplaysDataType"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|g| {
                            let name = text(g, "sppci_model")?;
                            let cores = text(g, "sppci_cores").map(|c| format!(" · {c} cores")).unwrap_or_default();
                            Some(format!("{name}{cores}"))
                        })
                        .collect()
                })
                .unwrap_or_default();
        }
    }
    let Some(list_out) = run("diskutil", &["list", "physical"], 8) else { return };
    let mut smartctl_seen = false;
    for line in list_out.lines().filter(|l| l.starts_with("/dev/disk")) {
        let id = line.split_whitespace().next().unwrap_or("").to_string();
        let Some(info_out) = run("diskutil", &["info", &id], 8) else { continue };
        let d = parse_diskutil(&info_out);
        let ssd = d.solid_state.unwrap_or(false);
        let external = d.internal == Some(false) || line.contains("external");
        let kind = if external { "External" } else if d.protocol.as_deref() == Some("PCI-Express") || d.protocol.as_deref() == Some("Apple Fabric") { "NVMe SSD" } else if ssd { "SSD" } else { "HDD" };
        let mut disk = PhysicalDisk {
            name: id.trim_start_matches("/dev/").to_string(),
            model: d.name.clone().unwrap_or_else(|| id.clone()),
            kind: kind.into(),
            bus: d.protocol.clone(),
            size: d.size.unwrap_or(0),
            health: d.smart.as_ref().map(|s| if s.eq_ignore_ascii_case("verified") { "Healthy".to_string() } else { s.clone() }).filter(|s| s != "Not Supported"),
            source: "macOS SMART status".into(),
            ..PhysicalDisk::default()
        };
        if let Some(out) = run("smartctl", &["-j", "-a", &id], 8) {
            if let Some(s) = parse_smartctl(&out) {
                smartctl_seen = true;
                disk.temperature_c = s.temperature_c;
                disk.power_on_hours = s.power_on_hours;
                disk.wear_percent = s.wear_percent;
                disk.bytes_written = s.bytes_written;
                disk.reallocated_sectors = s.reallocated;
                disk.media_errors = s.media_errors;
                disk.source = "SMART data from smartctl".into();
            }
        }
        disk.lifespan = estimate_drive_life(&disk.kind, disk.power_on_hours, disk.wear_percent, disk.health.as_deref());
        report.disks.push(disk);
    }
    if !smartctl_seen {
        report.notes.push("macOS only reports a pass or fail for drive health. Install smartmontools (`brew install smartmontools`) for temperature, wear and hours.".into());
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "android", target_os = "ios")))]
fn platform_details(_report: &mut SystemReport) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssd_life_is_projected_from_wear_and_hours() {
        // 10% used after 1 year of use: about 9 more years at the same pace.
        let l = estimate_drive_life("NVMe SSD", Some(8766), Some(10.0), Some("Healthy")).unwrap();
        assert_eq!(l.basis, "projection");
        assert!((l.years_left.unwrap() - 9.0).abs() < 0.1);
    }

    #[test]
    fn unused_ssd_gets_no_projection() {
        let l = estimate_drive_life("SSD", Some(500), Some(0.0), Some("Healthy")).unwrap();
        assert_eq!(l.years_left, None);
    }

    #[test]
    fn failing_drive_says_replace() {
        let l = estimate_drive_life("HDD", Some(100), None, Some("Failing")).unwrap();
        assert_eq!(l.years_left, Some(0.0));
        assert!(l.summary.contains("Back up"));
    }

    #[test]
    fn hdd_is_labelled_a_rule_of_thumb() {
        let l = estimate_drive_life("HDD", Some(2 * 8766), None, Some("Healthy")).unwrap();
        assert_eq!(l.basis, "rule of thumb");
        assert!((l.years_left.unwrap() - 3.0).abs() < 0.1);
        assert!(estimate_drive_life("HDD", None, None, None).is_none());
    }

    #[test]
    fn battery_projects_cycles_to_eighty_percent() {
        let l = estimate_battery_life(Some(90.0), Some(200)).unwrap();
        assert!(l.summary.contains("200 more cycles"), "{}", l.summary);
        assert!(estimate_battery_life(Some(70.0), Some(900)).unwrap().summary.contains("replacing"));
        assert!(estimate_battery_life(None, None).is_none());
    }

    #[test]
    fn parses_the_windows_report() {
        let json = r#"{"system":{"Manufacturer":"LENOVO","Model":"82JW"},
          "bios":{"Manufacturer":"LENOVO","SMBIOSBIOSVersion":"GKCN40WW"},
          "board":{"Manufacturer":"LENOVO","Product":"LNVNB161216"},
          "gpu":{"Name":"NVIDIA GeForce RTX 3050","AdapterRAM":4293918720,"DriverVersion":"31.0.15.3623"},
          "memory":[{"Manufacturer":"Samsung","Speed":3200,"Capacity":8589934592},{"Manufacturer":"0000","Speed":3200,"Capacity":8589934592}],
          "disks":[{"Name":"WDC PC SN730","Model":"WDC PC SN730","Media":"SSD","Bus":"NVMe","Size":512110190592,"Health":"Healthy","Temp":38,"Hours":4380,"Wear":4,"ReadErrors":0},
                   {"Name":"ST1000","Model":"ST1000","Media":"HDD","Bus":"SATA","Size":1000204886016,"Health":"Healthy","Temp":null,"Hours":null,"Wear":null}]}"#;
        let w = parse_windows(json).unwrap();
        assert_eq!(w.machine.model.as_deref(), Some("82JW"));
        assert_eq!(w.machine.firmware.as_deref(), Some("LENOVO GKCN40WW"));
        assert_eq!(w.gpus.len(), 1);
        assert!(w.gpus[0].contains("RTX 3050") && w.gpus[0].contains("driver 31.0"));
        assert_eq!(w.modules, vec!["8 GB at 3200 MT/s · Samsung", "8 GB at 3200 MT/s"]);
        assert_eq!(w.disks.len(), 2);
        assert_eq!(w.disks[0].kind, "NVMe SSD");
        assert_eq!(w.disks[0].wear_percent, Some(4.0));
        assert!(w.disks[0].lifespan.as_ref().unwrap().years_left.unwrap() > 10.0);
        assert_eq!(w.disks[1].kind, "HDD");
        assert!(w.disks[1].source.contains("no detailed counters"));
        assert!(parse_windows("not json").is_none());
    }

    #[test]
    fn parses_nvme_smartctl() {
        let json = r#"{"model_name":"Samsung SSD 980","device":{"protocol":"NVMe"},"smart_status":{"passed":true},
          "temperature":{"current":41},"power_on_time":{"hours":1200},
          "nvme_smart_health_information_log":{"percentage_used":3,"data_units_written":1000000,"media_errors":0}}"#;
        let s = parse_smartctl(json).unwrap();
        assert!(s.nvme);
        assert_eq!(s.passed, Some(true));
        assert_eq!(s.wear_percent, Some(3.0));
        assert_eq!(s.bytes_written, Some(512_000_000_000));
        assert_eq!(s.power_on_hours, Some(1200));
    }

    #[test]
    fn parses_sata_smartctl_and_rejects_errors() {
        let json = r#"{"smart_status":{"passed":true},"rotation_rate":0,"power_on_time":{"hours":900},
          "ata_smart_attributes":{"table":[{"id":5,"value":100,"raw":{"value":2}},{"id":177,"value":97,"raw":{"value":30}}]}}"#;
        let s = parse_smartctl(json).unwrap();
        assert_eq!(s.reallocated, Some(2));
        assert_eq!(s.wear_percent, Some(3.0));
        assert_eq!(s.rotational, Some(false));
        assert!(parse_smartctl(r#"{"smartctl":{"messages":[{"string":"Permission denied"}]}}"#).is_none());
    }

    #[test]
    fn parses_diskutil_text() {
        let text = "   Device / Media Name:       APPLE SSD AP0512Q\n   Solid State:               Yes\n   SMART Status:              Verified\n   Disk Size:                 500.3 GB (500277792768 Bytes) (exactly 977105064 512-Byte-Units)\n   Protocol:                  Apple Fabric\n";
        let d = parse_diskutil(text);
        assert_eq!(d.name.as_deref(), Some("APPLE SSD AP0512Q"));
        assert_eq!(d.solid_state, Some(true));
        assert_eq!(d.smart.as_deref(), Some("Verified"));
        assert_eq!(d.size, Some(500_277_792_768));
    }

    #[test]
    fn live_collection_does_not_panic() {
        let r = collect();
        assert!(r.cpu.logical >= 1);
        assert!(r.memory.total > 0);
    }
}
