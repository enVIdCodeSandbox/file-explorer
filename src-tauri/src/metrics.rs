//! Live on-demand system and application metrics, latency telemetry, network throughput,
//! file-logging, and administrator/sudo/root-gated kernel/hardware probes.

use crate::commands::CmdResult;
use crate::state::Shared;
use fast_core::platform;
use fast_core::telemetry::{self, TelemetryReport};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use sysinfo::{Components, Disks, Networks, ProcessRefreshKind, ProcessesToUpdate, System};
use tauri::State;

static RTT_PROBING: AtomicBool = AtomicBool::new(false);

struct SamplerState {
    subscribers: HashSet<String>,
    sys: Option<System>,
    networks: Option<Networks>,
    components: Option<Components>,
    disks: Option<Disks>,
    last_sample: Option<Instant>,
    last_slow_refresh: Option<Instant>,
    last_disk_read: u64,
    last_disk_write: u64,
    last_remote_read: u64,
    last_remote_write: u64,
    last_net_rx: u64,
    last_net_tx: u64,
    rtt_cache: HashMap<String, (Instant, Option<f64>)>,
    cached_disks: Vec<DiskVolumeMetric>,
    cached_temps: Vec<TempMetric>,
}

impl Default for SamplerState {
    fn default() -> Self {
        Self {
            subscribers: HashSet::new(),
            sys: None,
            networks: None,
            components: None,
            disks: None,
            last_sample: None,
            last_slow_refresh: None,
            last_disk_read: 0,
            last_disk_write: 0,
            last_remote_read: 0,
            last_remote_write: 0,
            last_net_rx: 0,
            last_net_tx: 0,
            rtt_cache: HashMap::new(),
            cached_disks: Vec::new(),
            cached_temps: Vec::new(),
        }
    }
}

impl SamplerState {
    fn clear_samplers(&mut self) {
        self.sys = None;
        self.networks = None;
        self.components = None;
        self.disks = None;
        self.last_sample = None;
        self.last_slow_refresh = None;
        self.rtt_cache.clear();
        self.cached_disks.clear();
        self.cached_temps.clear();
    }
}

fn sampler() -> &'static Mutex<SamplerState> {
    static S: OnceLock<Mutex<SamplerState>> = OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn set_active(client: &str, active: bool) {
    let mut g = sampler().lock();
    if active {
        g.subscribers.insert(client.to_string());
    } else {
        g.subscribers.remove(client);
    }
    let any = !g.subscribers.is_empty();
    telemetry::set_active(any);
    if !any {
        g.clear_samplers();
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NetIfaceMetric {
    pub name: String,
    pub rx_per_sec: f64,
    pub tx_per_sec: f64,
    pub total_rx: u64,
    pub total_tx: u64,
    pub packets_rx: u64,
    pub packets_tx: u64,
    pub errors_rx: u64,
    pub errors_tx: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TempMetric {
    pub label: String,
    pub celsius: f32,
    pub max_celsius: Option<f32>,
    pub critical_celsius: Option<f32>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DiskVolumeMetric {
    pub name: String,
    pub mount: String,
    pub fs: String,
    pub kind: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub read_only: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEndpointMetric {
    pub url: String,
    pub host: String,
    pub port: u16,
    pub idle_connections: usize,
    pub rtt_ms: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSnapshot {
    pub sampled_at_ms: u64,
    pub is_elevated: bool,
    pub can_elevate: bool,
    pub os_name: String,
    pub kernel_version: String,
    pub uptime_secs: u64,
    pub load_avg: [f64; 3],
    // CPU
    pub cpu_percent: f32,
    pub per_core_cpu: Vec<f32>,
    pub cpu_freq_mhz: u64,
    pub physical_cores: usize,
    pub logical_cores: usize,
    // Memory & Swap
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub memory_available_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    // App process
    pub app_pid: u32,
    pub app_uptime_secs: u64,
    pub app_cpu_percent: f32,
    pub app_memory_bytes: u64,
    pub app_virtual_bytes: u64,
    pub app_threads: usize,
    pub app_handles: u32,
    pub app_disk_read_per_sec: f64,
    pub app_disk_write_per_sec: f64,
    pub app_disk_read_total: u64,
    pub app_disk_write_total: u64,
    // Network
    pub net_rx_per_sec: f64,
    pub net_tx_per_sec: f64,
    pub net_total_rx: u64,
    pub net_total_tx: u64,
    pub interfaces: Vec<NetIfaceMetric>,
    // Remote / app network speeds
    pub remote_read_per_sec: f64,
    pub remote_write_per_sec: f64,
    pub remote_read_total: u64,
    pub remote_write_total: u64,
    pub remotes: Vec<RemoteEndpointMetric>,
    // App caches, workers & queues
    pub index_entries: usize,
    pub index_memory_bytes: usize,
    pub index_state: String,
    pub listings_cached: usize,
    pub listings_memory_bytes: usize,
    pub folder_sizes_cached: usize,
    pub folder_sizes_memory_bytes: usize,
    pub active_transfers: usize,
    pub active_deletes: usize,
    pub active_analyses: usize,
    pub active_shares: usize,
    pub watched_dirs: usize,
    pub open_terminals: usize,
    pub thumb_workers_busy: usize,
    pub thumb_workers_total: usize,
    pub slow_workers_busy: usize,
    pub slow_workers_total: usize,
    // Volumes & Temperatures
    pub disks: Vec<DiskVolumeMetric>,
    pub temperatures: Vec<TempMetric>,
    // Operation latencies & expectations
    pub telemetry: TelemetryReport,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminKernelMetrics {
    pub non_paged_pool_bytes: Option<u64>,
    pub paged_pool_bytes: Option<u64>,
    pub commit_bytes: Option<u64>,
    pub commit_limit_bytes: Option<u64>,
    pub page_faults_per_sec: Option<u64>,
    pub context_switches_per_sec: Option<u64>,
    pub system_calls_per_sec: Option<u64>,
    pub interrupt_percent: Option<f64>,
    pub dpc_percent: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminDiskPerf {
    pub name: String,
    pub queue_length: f64,
    pub avg_read_ms: f64,
    pub avg_write_ms: f64,
    pub busy_percent: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminSocketSummary {
    pub tcp_established: u64,
    pub tcp_listening: u64,
    pub tcp_time_wait: u64,
    pub tcp_total: u64,
    pub udp_endpoints: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminGpuMetric {
    pub name: String,
    pub dedicated_used_bytes: u64,
    pub shared_used_bytes: u64,
    pub engine_percent: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdminThermalZone {
    pub name: String,
    pub celsius: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegedMetrics {
    pub collected_at_ms: u64,
    pub elevated_identity: String,
    pub kernel: AdminKernelMetrics,
    pub physical_disks: Vec<AdminDiskPerf>,
    pub sockets: AdminSocketSummary,
    pub thermal_zones: Vec<AdminThermalZone>,
    pub gpu_adapters: Vec<AdminGpuMetric>,
    pub notes: Vec<String>,
}

pub fn is_process_elevated() -> bool {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        const TOKEN_QUERY: u32 = 0x0008;
        const TOKEN_ELEVATION_CLASS: u32 = 20;
        #[repr(C)]
        struct TokenElevation {
            token_is_elevated: u32,
        }
        #[link(name = "advapi32")]
        extern "system" {
            fn OpenProcessToken(process: *mut c_void, access: u32, token: *mut *mut c_void) -> i32;
            fn GetTokenInformation(
                token: *mut c_void,
                info_class: u32,
                info: *mut c_void,
                len: u32,
                ret_len: *mut u32,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let mut token: *mut c_void = std::ptr::null_mut();
        // SAFETY: `OpenProcessToken` opens the current process token and `CloseHandle` closes it.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elev = TokenElevation { token_is_elevated: 0 };
            let mut returned = 0u32;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION_CLASS,
                &mut elev as *mut _ as *mut c_void,
                std::mem::size_of::<TokenElevation>() as u32,
                &mut returned,
            );
            CloseHandle(token);
            ok != 0 && elev.token_is_elevated != 0
        }
    }
    #[cfg(unix)]
    {
        // SAFETY: `geteuid` has no preconditions and always succeeds.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(any(windows, unix)))]
    {
        false
    }
}

fn process_handles() -> u32 {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn GetProcessHandleCount(process: *mut c_void, count: *mut u32) -> i32;
        }
        let mut count = 0u32;
        // SAFETY: `count` points to valid stack storage.
        unsafe {
            let _ = GetProcessHandleCount(GetCurrentProcess(), &mut count);
        }
        count
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/proc/self/fd").map(|d| d.count() as u32).unwrap_or(0)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}

fn process_threads() -> usize {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        // Count live threads belonging to the current process via Windows ToolHelp.
        let pid = std::process::id();
        #[repr(C)]
        #[allow(non_snake_case)]
        struct ThreadEntry32 {
            dwSize: u32,
            cntUsage: u32,
            th32ThreadID: u32,
            th32OwnerProcessID: u32,
            tpBasePri: i32,
            tpDeltaPri: i32,
            dwFlags: u32,
        }
        const TH32CS_SNAPTHREAD: u32 = 0x0000_0004;
        const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
            fn Thread32First(snap: *mut c_void, entry: *mut ThreadEntry32) -> i32;
            fn Thread32Next(snap: *mut c_void, entry: *mut ThreadEntry32) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let mut n = 0usize;
        // SAFETY: ToolHelp snapshot handle is checked against `INVALID_HANDLE_VALUE` and closed.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap != INVALID_HANDLE_VALUE {
                let mut entry = std::mem::zeroed::<ThreadEntry32>();
                entry.dwSize = std::mem::size_of::<ThreadEntry32>() as u32;
                if Thread32First(snap, &mut entry) != 0 {
                    loop {
                        if entry.th32OwnerProcessID == pid {
                            n += 1;
                        }
                        if Thread32Next(snap, &mut entry) == 0 {
                            break;
                        }
                    }
                }
                CloseHandle(snap);
            }
        }
        n
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        0
    }
}

fn spawn_rtt_probes(endpoints: Vec<(String, String, u16)>) {
    if endpoints.is_empty() || RTT_PROBING.swap(true, Ordering::AcqRel) {
        return;
    }
    std::thread::Builder::new()
        .name("metrics-rtt".into())
        .spawn(move || {
            let mut results = Vec::new();
            for (url, host, port) in endpoints {
                if host.is_empty() || port == 0 {
                    continue;
                }
                let rtt = (host.as_str(), port)
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut addrs| addrs.next())
                    .and_then(|addr| {
                        let t0 = Instant::now();
                        TcpStream::connect_timeout(&addr, Duration::from_millis(800))
                            .ok()
                            .map(|_| t0.elapsed().as_secs_f64() * 1000.0)
                    });
                results.push((url, rtt));
            }
            let now = Instant::now();
            let mut g = sampler().lock();
            if !g.subscribers.is_empty() {
                for (url, rtt) in results {
                    g.rtt_cache.insert(url, (now, rtt));
                }
            }
            RTT_PROBING.store(false, Ordering::Release);
        })
        .ok();
}

#[tauri::command]
pub async fn metrics_set_active(client: String, active: bool) -> CmdResult<()> {
    set_active(&client, active);
    Ok(())
}

#[tauri::command]
pub async fn metrics_reset() -> CmdResult<()> {
    telemetry::reset();
    Ok(())
}

#[tauri::command]
pub async fn metrics_snapshot(state: State<'_, Shared>) -> CmdResult<MetricsSnapshot> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || collect_snapshot(&st))
        .await
        .map_err(|e| e.to_string())?
}

fn collect_snapshot(st: &Shared) -> CmdResult<MetricsSnapshot> {
    let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
    let mut g = sampler().lock();
    if g.subscribers.is_empty() {
        // If invoked directly (e.g. initial open), ensure active tracking is enabled.
        g.subscribers.insert("default".into());
        telemetry::set_active(true);
    }

    let now = Instant::now();
    let dt = g.last_sample.map(|t| now.saturating_duration_since(t).as_secs_f64()).unwrap_or(0.0);
    let do_slow = g.last_slow_refresh.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(1));

    let sys = g.sys.get_or_insert_with(System::new_all);
    sys.refresh_memory();
    sys.refresh_cpu_usage();
    sys.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, ProcessRefreshKind::everything());

    let p = sys.process(pid).ok_or("Could not inspect current process.")?;
    let proc_disk = p.disk_usage();
    let app_cpu_percent = p.cpu_usage();
    let app_memory_bytes = p.memory();
    let app_virtual_bytes = p.virtual_memory();

    let cpu_percent = sys.global_cpu_usage();
    let per_core_cpu: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
    let cpu_freq_mhz = sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
    let physical_cores = sys.physical_core_count().unwrap_or(per_core_cpu.len().max(1));
    let logical_cores = per_core_cpu.len().max(1);
    let mem_total = sys.total_memory();
    let mem_avail = sys.available_memory();
    let mem_used = mem_total.saturating_sub(mem_avail);
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();

    let networks = g.networks.get_or_insert_with(Networks::new_with_refreshed_list);
    networks.refresh(true);
    let mut net_total_rx = 0u64;
    let mut net_total_tx = 0u64;
    let mut interfaces = Vec::new();
    for (name, data) in networks.iter() {
        let rx_t = data.total_received();
        let tx_t = data.total_transmitted();
        net_total_rx = net_total_rx.saturating_add(rx_t);
        net_total_tx = net_total_tx.saturating_add(tx_t);
        let (rx_rate, tx_rate) = if dt > 0.002 {
            (data.received() as f64 / dt, data.transmitted() as f64 / dt)
        } else {
            (0.0, 0.0)
        };
        if rx_t > 0 || tx_t > 0 || rx_rate > 0.0 || tx_rate > 0.0 {
            interfaces.push(NetIfaceMetric {
                name: name.clone(),
                rx_per_sec: rx_rate,
                tx_per_sec: tx_rate,
                total_rx: rx_t,
                total_tx: tx_t,
                packets_rx: data.total_packets_received(),
                packets_tx: data.total_packets_transmitted(),
                errors_rx: data.total_errors_on_received(),
                errors_tx: data.total_errors_on_transmitted(),
            });
        }
    }
    interfaces.sort_by(|a, b| (b.rx_per_sec + b.tx_per_sec).total_cmp(&(a.rx_per_sec + a.tx_per_sec)).then_with(|| a.name.cmp(&b.name)));

    let net_rx_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        net_total_rx.saturating_sub(g.last_net_rx) as f64 / dt
    } else {
        0.0
    };
    let net_tx_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        net_total_tx.saturating_sub(g.last_net_tx) as f64 / dt
    } else {
        0.0
    };

    let app_disk_read_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        proc_disk.total_read_bytes.saturating_sub(g.last_disk_read) as f64 / dt
    } else {
        0.0
    };
    let app_disk_write_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        proc_disk.total_written_bytes.saturating_sub(g.last_disk_write) as f64 / dt
    } else {
        0.0
    };

    let (remote_read_total, remote_write_total) = telemetry::remote_io_totals();
    let remote_read_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        remote_read_total.saturating_sub(g.last_remote_read) as f64 / dt
    } else {
        0.0
    };
    let remote_write_per_sec = if dt > 0.002 && g.last_sample.is_some() {
        remote_write_total.saturating_sub(g.last_remote_write) as f64 / dt
    } else {
        0.0
    };

    if do_slow {
        let disks = g.disks.get_or_insert_with(Disks::new_with_refreshed_list);
        disks.refresh(true);
        g.cached_disks = disks
            .iter()
            .map(|d| DiskVolumeMetric {
                name: d.name().to_string_lossy().into_owned(),
                mount: d.mount_point().to_string_lossy().into_owned(),
                fs: d.file_system().to_string_lossy().into_owned(),
                kind: format!("{:?}", d.kind()),
                total_bytes: d.total_space(),
                available_bytes: d.available_space(),
                read_only: d.is_read_only(),
            })
            .collect();

        let comps = g.components.get_or_insert_with(Components::new_with_refreshed_list);
        comps.refresh(true);
        g.cached_temps = comps
            .iter()
            .filter_map(|c| {
                let t = c.temperature()?;
                Some(TempMetric {
                    label: c.label().to_string(),
                    celsius: t,
                    max_celsius: c.max(),
                    critical_celsius: c.critical(),
                })
            })
            .collect();
        g.last_slow_refresh = Some(now);
    }

    g.last_sample = Some(now);
    g.last_disk_read = proc_disk.total_read_bytes;
    g.last_disk_write = proc_disk.total_written_bytes;
    g.last_remote_read = remote_read_total;
    g.last_remote_write = remote_write_total;
    g.last_net_rx = net_total_rx;
    g.last_net_tx = net_total_tx;

    let disks_out = g.cached_disks.clone();
    let temps_out = g.cached_temps.clone();

    // Remote endpoint inspection + background RTT probe scheduling
    let raw_remotes: Vec<(String, String, u16, usize)> = {
        let map = st.remotes.lock();
        map.values().map(|r| r.summary()).collect()
    };
    let mut stale_endpoints = Vec::new();
    let mut remotes_out = Vec::new();
    for (url, host, port, idle) in &raw_remotes {
        let cached = g.rtt_cache.get(url);
        let rtt_ms = cached.and_then(|(_, ms)| *ms);
        if cached.is_none_or(|(at, _)| now.saturating_duration_since(*at) >= Duration::from_secs(3)) {
            stale_endpoints.push((url.clone(), host.clone(), *port));
        }
        remotes_out.push(RemoteEndpointMetric {
            url: url.clone(),
            host: host.clone(),
            port: *port,
            idle_connections: *idle,
            rtt_ms,
        });
    }
    drop(g);
    spawn_rtt_probes(stale_endpoints);

    let (index_entries, index_memory_bytes) = {
        let idx = st.index.read();
        (idx.len(), idx.memory_bytes())
    };
    let index_state = st.index_status.lock().state.clone();
    let (listings_cached, listings_memory_bytes) = {
        let l = st.listings.lock();
        (0, l.memory_bytes())
    };
    let (folder_sizes_cached, folder_sizes_memory_bytes) = {
        let f = st.folder_sizes.lock();
        (0, f.memory_bytes())
    };
    let budget = st.budget.read().clone();
    let thumb_sem = st.thumb_permits.read().clone();
    let slow_sem = st.slow_thumb_permits.read().clone();
    let thumb_workers_total = budget.thumb_threads.max(1);
    let slow_workers_total = budget.slow_thumb_threads.max(1);
    let thumb_workers_busy = thumb_workers_total.saturating_sub(thumb_sem.available_permits());
    let slow_workers_busy = slow_workers_total.saturating_sub(slow_sem.available_permits());

    let watched_dirs: usize = st.watch_sets.lock().values().map(|v| v.len()).sum();
    #[cfg(desktop)]
    let open_terminals = st.terminal.active_ssh_count();
    #[cfg(not(desktop))]
    let open_terminals = 0usize;

    let load = System::load_average();
    let sampled_at_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);

    Ok(MetricsSnapshot {
        sampled_at_ms,
        is_elevated: is_process_elevated(),
        can_elevate: cfg!(desktop),
        os_name: System::long_os_version().unwrap_or_else(|| platform::os_name().to_string()),
        kernel_version: System::kernel_version().unwrap_or_default(),
        uptime_secs: System::uptime(),
        load_avg: [load.one, load.five, load.fifteen],
        cpu_percent,
        per_core_cpu,
        cpu_freq_mhz,
        physical_cores,
        logical_cores,
        memory_total_bytes: mem_total,
        memory_used_bytes: mem_used,
        memory_available_bytes: mem_avail,
        swap_total_bytes: swap_total,
        swap_used_bytes: swap_used,
        app_pid: pid.as_u32(),
        app_uptime_secs: st.started.elapsed().as_secs(),
        app_cpu_percent,
        app_memory_bytes,
        app_virtual_bytes,
        app_threads: process_threads(),
        app_handles: process_handles(),
        app_disk_read_per_sec,
        app_disk_write_per_sec,
        app_disk_read_total: proc_disk.total_read_bytes,
        app_disk_write_total: proc_disk.total_written_bytes,
        net_rx_per_sec,
        net_tx_per_sec,
        net_total_rx,
        net_total_tx,
        interfaces,
        remote_read_per_sec,
        remote_write_per_sec,
        remote_read_total,
        remote_write_total,
        remotes: remotes_out,
        index_entries,
        index_memory_bytes,
        index_state,
        listings_cached,
        listings_memory_bytes,
        folder_sizes_cached,
        folder_sizes_memory_bytes,
        active_transfers: st.transfers.lock().len(),
        active_deletes: st.deletes.lock().len(),
        active_analyses: st.analyses.lock().len(),
        active_shares: st.shares.list().len(),
        watched_dirs,
        open_terminals,
        thumb_workers_busy,
        thumb_workers_total,
        slow_workers_busy,
        slow_workers_total,
        disks: disks_out,
        temperatures: temps_out,
        telemetry: telemetry::snapshot(),
    })
}

#[tauri::command]
pub async fn metrics_collect_admin(state: State<'_, Shared>) -> CmdResult<PrivilegedMetrics> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if is_process_elevated() {
            return Ok(collect_admin_now());
        }
        std::fs::create_dir_all(&st.paths.scratch).map_err(|e| e.to_string())?;
        let out = st.paths.scratch.join(format!("admin-metrics-{}.json", st.next_id.fetch_add(1, Ordering::Relaxed)));
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let args = vec!["--admin-metrics".to_string(), out.to_string_lossy().into_owned()];
        let res = platform::run_elevated(&exe, &args).map_err(|e| e.to_string())?;
        let text = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        if matches!(res, platform::Elevation::Declined) {
            return Err("Administrator permission was not given.".to_string());
        }
        if text.trim().is_empty() {
            return Err("Privileged telemetry helper returned no data.".to_string());
        }
        serde_json::from_str(&text).map_err(|e| format!("Could not read privileged metrics: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Entrypoint for headless `--admin-metrics <out.json>` helper.
pub fn admin_helper_main(args: &[String]) -> i32 {
    let Some(i) = args.iter().position(|a| a == "--admin-metrics") else { return 2 };
    let Some(out_path) = args.get(i + 1) else { return 2 };
    let data = collect_admin_now();
    match serde_json::to_vec(&data).ok().and_then(|b| std::fs::write(out_path, b).ok()) {
        Some(()) => 0,
        None => 1,
    }
}

fn collect_admin_now() -> PrivilegedMetrics {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let mut out = PrivilegedMetrics {
        collected_at_ms: now_ms,
        elevated_identity: if is_process_elevated() { "Administrator / root".into() } else { "Standard".into() },
        ..Default::default()
    };

    #[cfg(windows)]
    {
        const SCRIPT: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$ErrorActionPreference='SilentlyContinue'
$mem = Get-CimInstance Win32_PerfFormattedData_PerfOS_Memory | Select-Object -First 1 PoolNonpagedBytes,PoolPagedBytes,CommittedBytes,CommitLimit,PageFaultsPersec
$sys = Get-CimInstance Win32_PerfFormattedData_PerfOS_System | Select-Object -First 1 ContextSwitchesPersec,SystemCallsPersec
$cpu = Get-CimInstance Win32_PerfFormattedData_PerfOS_Processor -Filter "Name='_Total'" | Select-Object -First 1 PercentInterruptTime,PercentDPCTime
$disks = @(Get-CimInstance Win32_PerfFormattedData_PerfDisk_PhysicalDisk | Where-Object { $_.Name -ne '_Total' } | ForEach-Object {
  [pscustomobject]@{ name=$_.Name; queueLength=[double]$_.CurrentDiskQueueLength; avgReadMs=([double]$_.AvgDisksecPerRead)*1000.0; avgWriteMs=([double]$_.AvgDisksecPerWrite)*1000.0; busyPercent=[double]$_.PercentDiskTime }
})
$zones = @(Get-CimInstance -Namespace root\WMI MSAcpi_ThermalZoneTemperature | ForEach-Object {
  [pscustomobject]@{ name=$_.InstanceName; celsius=([double]$_.CurrentTemperature / 10.0) - 273.15 }
})
$tcp = @(Get-NetTCPConnection)
$udp = @(Get-NetUDPEndpoint)
$gpuMem = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUAdapterMemory)
$gpuEng = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine)
$gpus = @($gpuMem | ForEach-Object {
  [pscustomobject]@{ name=$_.Name; dedicatedUsedBytes=[uint64]$_.DedicatedUsage; sharedUsedBytes=[uint64]$_.SharedUsage; enginePercent=($gpuEng | Measure-Object -Property UtilizationPercentage -Sum).Sum }
})
[pscustomobject]@{
  kernel=@{
    nonPagedPoolBytes=$mem.PoolNonpagedBytes; pagedPoolBytes=$mem.PoolPagedBytes;
    commitBytes=$mem.CommittedBytes; commitLimitBytes=$mem.CommitLimit;
    pageFaultsPerSec=$mem.PageFaultsPersec; contextSwitchesPerSec=$sys.ContextSwitchesPersec;
    systemCallsPerSec=$sys.SystemCallsPersec; interruptPercent=$cpu.PercentInterruptTime; dpcPercent=$cpu.PercentDPCTime
  };
  physicalDisks=$disks;
  thermalZones=$zones;
  sockets=@{
    tcpEstablished=@($tcp | Where-Object { $_.State -eq 'Established' }).Count;
    tcpListening=@($tcp | Where-Object { $_.State -eq 'Listen' }).Count;
    tcpTimeWait=@($tcp | Where-Object { $_.State -eq 'TimeWait' }).Count;
    tcpTotal=$tcp.Count;
    udpEndpoints=$udp.Count
  };
  gpuAdapters=$gpus
} | ConvertTo-Json -Depth 5 -Compress"#;
        let mut cmd = std::process::Command::new("powershell");
        fast_core::proc::quiet(&mut cmd);
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT]);
        if let Ok(o) = cmd.output() {
            if o.status.success() {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
                    if let Ok(k) = serde_json::from_value(v["kernel"].clone()) {
                        out.kernel = k;
                    }
                    if let Ok(d) = serde_json::from_value(v["physicalDisks"].clone()) {
                        out.physical_disks = d;
                    }
                    if let Ok(z) = serde_json::from_value(v["thermalZones"].clone()) {
                        out.thermal_zones = z;
                    }
                    if let Ok(s) = serde_json::from_value(v["sockets"].clone()) {
                        out.sockets = s;
                    }
                    if let Ok(g) = serde_json::from_value(v["gpuAdapters"].clone()) {
                        out.gpu_adapters = g;
                    }
                }
            }
        }
        out.notes.push("Collected with Windows Administrator WMI/CIM kernel & physical disk counters.".into());
    }

    #[cfg(unix)]
    {
        if let Ok(mem) = std::fs::read_to_string("/proc/meminfo") {
            let mut slab = None;
            let mut commit = None;
            let mut limit = None;
            for line in mem.lines() {
                let mut parts = line.split_whitespace();
                let key = parts.next().unwrap_or("");
                let kb = parts.next().and_then(|v| v.parse::<u64>().ok()).map(|n| n * 1024);
                match key {
                    "Slab:" => slab = kb,
                    "Committed_AS:" => commit = kb,
                    "CommitLimit:" => limit = kb,
                    _ => {}
                }
            }
            out.kernel.non_paged_pool_bytes = slab;
            out.kernel.commit_bytes = commit;
            out.kernel.commit_limit_bytes = limit;
        }
        if let Ok(stat) = std::fs::read_to_string("/proc/stat") {
            for line in stat.lines() {
                if let Some(n) = line.strip_prefix("ctxt ").and_then(|v| v.trim().parse::<u64>().ok()) {
                    out.kernel.context_switches_per_sec = Some(n);
                }
            }
        }
        out.notes.push("Collected with elevated Unix privileges.".into());
    }

    out
}

/// Write a single metrics snapshot or append a JSON/CSV log line safely into `folder`.
#[tauri::command]
pub async fn metrics_save_file(state: State<'_, Shared>, folder: String, filename: String, content: String, append: bool) -> CmdResult<String> {
    let _ = state;
    tauri::async_runtime::spawn_blocking(move || save_metrics_file_sync(&folder, &filename, &content, append))
        .await
        .map_err(|e| e.to_string())?
}

pub fn save_metrics_file_sync(folder: &str, filename: &str, content: &str, append: bool) -> CmdResult<String> {
    let dir = Path::new(folder.trim());
    if dir.as_os_str().is_empty() {
        return Err("Choose a folder first.".into());
    }
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not open folder '{}': {e}", dir.display()))?;
    }
    if !dir.is_dir() {
        return Err(format!("'{}' is not a folder.", dir.display()));
    }
    let leaf = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let clean = fast_core::remote::safe_name(&leaf);
    if clean.is_empty() {
        return Err("Invalid metrics log filename.".into());
    }
    let target = dir.join(clean);
    if append {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .map_err(|e| format!("Could not write metrics log '{}': {e}", target.display()))?;
        f.write_all(content.as_bytes()).map_err(|e| format!("Could not write metrics log: {e}"))?;
    } else {
        std::fs::write(&target, content.as_bytes()).map_err(|e| format!("Could not save metrics file '{}': {e}", target.display()))?;
    }
    Ok(target.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_appends_metrics_logs_in_chosen_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = save_metrics_file_sync(&dir.path().to_string_lossy(), "session.jsonl", "{\"a\":1}\n", true).unwrap();
        save_metrics_file_sync(&dir.path().to_string_lossy(), "../session.jsonl", "{\"a\":2}\n", true).unwrap();
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(body.contains("{\"a\":1}"));
        assert!(body.contains("{\"a\":2}"));
        assert_eq!(Path::new(&path).parent().unwrap(), dir.path());
    }
}
