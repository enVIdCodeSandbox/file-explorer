//! Native graphics backend detection, WebView GPU pipeline configuration, and rendering diagnostics.
//!
//! Configures OS-native graphics APIs (DirectX 12, DirectX 11, Vulkan, OpenGL, Metal, or software
//! fallback) before WebView creation, and exposes detected hardware adapters and active pipeline
//! state to the UI so presets and user overrides work coherently.

use crate::resources::{Budget, Tier};
use crate::settings::{Performance, Settings};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;

pub const BACKENDS: &[&str] = &["auto", "directx12", "directx11", "vulkan", "opengl", "metal", "software"];

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RenderingStatus {
    pub requested_backend: String,
    pub active_backend: String,
    pub launch_backend: String,
    pub restart_required: bool,
    pub hardware_acceleration: bool,
    pub gpu_rasterization: bool,
    pub zero_copy: bool,
    pub vsync: bool,
    pub available_backends: Vec<String>,
    pub adapters: Vec<String>,
    pub launch_args: String,
}

#[derive(Clone, Debug)]
struct LaunchState {
    backend: String,
    hardware_acceleration: bool,
    gpu_rasterization: bool,
    zero_copy: bool,
    vsync: bool,
    args: String,
}

static LAUNCH: OnceLock<LaunchState> = OnceLock::new();

/// Standard config file path matching Tauri's desktop app identifier before `tauri::Builder` runs.
pub fn early_settings_path(identifier: &str) -> Option<PathBuf> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dirs::config_dir().map(|d| d.join(identifier).join("settings.json"))
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = identifier;
        None
    }
}

/// Probe which native graphics backends exist on the host without spawning external processes.
pub fn available_backends() -> Vec<String> {
    let mut out = vec!["auto".to_string()];
    #[cfg(windows)]
    {
        if has_windows_dll("d3d12.dll") {
            out.push("directx12".into());
        }
        if has_windows_dll("d3d11.dll") {
            out.push("directx11".into());
        }
        if has_windows_dll("vulkan-1.dll") {
            out.push("vulkan".into());
        }
        if has_windows_dll("opengl32.dll") {
            out.push("opengl".into());
        }
    }
    #[cfg(target_os = "macos")]
    {
        if std::path::Path::new("/System/Library/Frameworks/Metal.framework/Metal").exists() {
            out.push("metal".into());
        }
        out.push("opengl".into());
    }
    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "ios"))))]
    {
        if has_linux_lib(&["/usr/lib/libvulkan.so.1", "/usr/lib/x86_64-linux-gnu/libvulkan.so.1", "/usr/lib/aarch64-linux-gnu/libvulkan.so.1", "/lib/x86_64-linux-gnu/libvulkan.so.1"]) {
            out.push("vulkan".into());
        }
        out.push("opengl".into());
    }
    out.push("software".into());
    out
}

#[cfg(windows)]
fn has_windows_dll(name: &str) -> bool {
    let Ok(sys) = std::env::var("SystemRoot") else {
        return name == "d3d11.dll" || name == "opengl32.dll";
    };
    let p = std::path::Path::new(&sys).join("System32").join(name);
    p.is_file()
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "ios"))))]
fn has_linux_lib(candidates: &[&str]) -> bool {
    candidates.iter().any(|p| std::path::Path::new(p).exists())
}

/// Detect active GPU/display adapter names directly from the OS (microseconds; never spawns shells).
pub fn detect_adapters() -> Vec<String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ACTIVE};

        let mut adapters = Vec::new();
        let mut dev = DISPLAY_DEVICEW::default();
        dev.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        let mut i = 0u32;
        // SAFETY: `DISPLAY_DEVICEW.cb` is initialized and `EnumDisplayDevicesW` only fills `dev`.
        while unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dev, 0) }.as_bool() {
            i += 1;
            if (dev.StateFlags & DISPLAY_DEVICE_ACTIVE).0 == 0 {
                continue;
            }
            let end = dev.DeviceString.iter().position(|&c| c == 0).unwrap_or(dev.DeviceString.len());
            let name = String::from_utf16_lossy(&dev.DeviceString[..end]).trim().to_string();
            if !name.is_empty() && !adapters.contains(&name) {
                adapters.push(name);
            }
            dev = DISPLAY_DEVICEW::default();
            dev.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
        }
        adapters
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Normalize and resolve `"auto"` / user-requested backend to the concrete native graphics backend.
pub fn resolve_backend(requested: &str, hardware_acceleration: bool, tier: Tier, intensity: u8, available: &[String]) -> String {
    if !hardware_acceleration || requested == "software" {
        return "software".into();
    }
    let has = |k: &str| available.iter().any(|a| a == k);
    match requested {
        "directx12" if has("directx12") => "directx12".into(),
        "directx11" if has("directx11") => "directx11".into(),
        "vulkan" if has("vulkan") => "vulkan".into(),
        "metal" if has("metal") => "metal".into(),
        "opengl" if has("opengl") => "opengl".into(),
        _ => {
            // Auto: pick the fastest native GPU API matching the host and tier intensity.
            if cfg!(target_os = "macos") && has("metal") {
                return "metal".into();
            }
            if (tier == Tier::Full || tier == Tier::Manual || intensity >= 65) && has("directx12") {
                return "directx12".into();
            }
            if has("directx11") {
                return "directx11".into();
            }
            if has("vulkan") {
                return "vulkan".into();
            }
            if has("opengl") {
                return "opengl".into();
            }
            "software".into()
        }
    }
}

/// Build the Chromium / WebView2 command-line argument string for native GPU acceleration.
pub fn webview_args_for(perf: &Performance, budget: &Budget, backend: &str) -> String {
    if backend == "software" || !perf.hardware_acceleration {
        return "--use-angle=swiftshader --disable-gpu --disable-gpu-compositing".into();
    }
    let mut args: Vec<String> = Vec::new();
    match backend {
        "directx12" => {
            args.push("--use-angle=d3d11on12".into());
            args.push("--enable-features=D3D12VideoDecoder".into());
        }
        "directx11" => {
            args.push("--use-angle=d3d11".into());
        }
        "vulkan" => {
            args.push("--use-angle=vulkan".into());
            args.push("--enable-features=Vulkan,DefaultANGLEVulkan,VulkanFromANGLE".into());
        }
        "opengl" => {
            args.push("--use-angle=gl".into());
        }
        _ => {}
    }

    args.push("--ignore-gpu-blocklist".into());
    if perf.gpu_rasterization {
        args.push("--enable-gpu-rasterization".into());
    }
    if perf.zero_copy {
        args.push("--enable-zero-copy".into());
        args.push("--enable-native-gpu-memory-buffers".into());
    }
    if !perf.vsync {
        args.push("--disable-gpu-vsync".into());
        args.push("--disable-frame-rate-limit".into());
    }
    let raster_threads = budget.io_threads.clamp(1, 16);
    args.push(format!("--num-raster-threads={raster_threads}"));
    if budget.cache_mb >= 1024 {
        let gpu_mb = (budget.cache_mb / 2).clamp(256, 4096);
        args.push(format!("--force-gpu-mem-available-mb={gpu_mb}"));
    }
    if perf.background_mode == "off" || budget.intensity >= 95 {
        args.push("--disable-backgrounding-occluded-windows".into());
        args.push("--disable-renderer-backgrounding".into());
    }
    args.join(" ")
}

/// Apply OS-level environment flags before Tauri instantiates the WebView window.
/// Idempotent across the process lifetime; records the launch configuration for restart detection.
pub fn configure_webview(perf: &Performance, budget: &Budget) {
    let available = available_backends();
    let backend = resolve_backend(&perf.render_backend, perf.hardware_acceleration, budget.tier, budget.intensity, &available);
    let args = webview_args_for(perf, budget, &backend);

    #[cfg(windows)]
    if LAUNCH.get().is_none() {
        let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
        let combined = if existing.trim().is_empty() { args.clone() } else { format!("{} {args}", existing.trim()) };
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", &combined);
    }

    #[cfg(all(unix, not(any(target_os = "macos", target_os = "android", target_os = "ios"))))]
    if LAUNCH.get().is_none() {
        match backend.as_str() {
            "vulkan" => {
                std::env::set_var("GSK_RENDERER", "vulkan");
                std::env::remove_var("WEBKIT_DISABLE_COMPOSITING_MODE");
                if perf.zero_copy {
                    std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER");
                }
            }
            "opengl" => {
                std::env::set_var("GSK_RENDERER", "ngl");
                std::env::remove_var("WEBKIT_DISABLE_COMPOSITING_MODE");
                if !perf.zero_copy {
                    std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
                }
            }
            "software" => {
                std::env::set_var("GSK_RENDERER", "cairo");
                std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
                std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            }
            _ => {}
        }
    }

    let _ = LAUNCH.set(LaunchState {
        backend,
        hardware_acceleration: perf.hardware_acceleration,
        gpu_rasterization: perf.gpu_rasterization,
        zero_copy: perf.zero_copy,
        vsync: perf.vsync,
        args,
    });
}

/// Convenience helper for early startup before `AppState` is constructed.
pub fn configure_from_early_settings(identifier: &str) {
    let settings = early_settings_path(identifier).map(|p| Settings::load(&p)).unwrap_or_default();
    let budget = crate::resources::compute(&settings.performance, &crate::resources::snapshot());
    configure_webview(&settings.performance, &budget);
}

/// Current rendering capabilities and whether pending settings need an app restart.
pub fn status(perf: &Performance, budget: &Budget) -> RenderingStatus {
    let available = available_backends();
    let desired = resolve_backend(&perf.render_backend, perf.hardware_acceleration, budget.tier, budget.intensity, &available);
    let launch = LAUNCH.get();
    let launch_backend = launch.map(|l| l.backend.clone()).unwrap_or_else(|| desired.clone());
    let launch_args = launch.map(|l| l.args.clone()).unwrap_or_else(|| webview_args_for(perf, budget, &desired));
    let restart_required = launch.is_some_and(|l| {
        l.backend != desired
            || l.hardware_acceleration != perf.hardware_acceleration
            || l.gpu_rasterization != perf.gpu_rasterization
            || l.zero_copy != perf.zero_copy
            || l.vsync != perf.vsync
    });

    RenderingStatus {
        requested_backend: perf.render_backend.clone(),
        active_backend: launch_backend.clone(),
        launch_backend,
        restart_required,
        hardware_acceleration: perf.hardware_acceleration,
        gpu_rasterization: perf.gpu_rasterization,
        zero_copy: perf.zero_copy,
        vsync: perf.vsync,
        available_backends: available,
        adapters: detect_adapters(),
        launch_args,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_native_backends_with_fallbacks() {
        let avail: Vec<String> = ["auto", "directx12", "directx11", "vulkan", "opengl", "software"].iter().map(|s| (*s).into()).collect();
        assert_eq!(resolve_backend("vulkan", true, Tier::Full, 80, &avail), "vulkan");
        assert_eq!(resolve_backend("directx12", true, Tier::Full, 80, &avail), "directx12");
        assert_eq!(resolve_backend("directx12", false, Tier::Full, 80, &avail), "software");
        assert_eq!(resolve_backend("software", true, Tier::Full, 100, &avail), "software");
        let auto_hi = resolve_backend("auto", true, Tier::Manual, 100, &avail);
        if cfg!(target_os = "macos") {
            assert_eq!(auto_hi, "opengl"); // mock avail omitted metal
        } else {
            assert_eq!(auto_hi, "directx12");
        }
    }

    #[test]
    fn webview_args_scale_with_features_and_backend() {
        let mut perf = Performance::default();
        perf.vsync = false;
        perf.background_mode = "off".into();
        let mut b = Budget::minimal();
        b.io_threads = 12;
        b.cache_mb = 4096;
        b.intensity = 100;

        let dx12 = webview_args_for(&perf, &b, "directx12");
        assert!(dx12.contains("--use-angle=d3d11on12"));
        assert!(dx12.contains("--enable-gpu-rasterization"));
        assert!(dx12.contains("--enable-zero-copy"));
        assert!(dx12.contains("--disable-gpu-vsync"));
        assert!(dx12.contains("--disable-frame-rate-limit"));
        assert!(dx12.contains("--num-raster-threads=12"));
        assert!(dx12.contains("--force-gpu-mem-available-mb=2048"));
        assert!(dx12.contains("--disable-renderer-backgrounding"));

        let vk = webview_args_for(&perf, &b, "vulkan");
        assert!(vk.contains("--use-angle=vulkan"));
        assert!(vk.contains("DefaultANGLEVulkan"));

        let sw = webview_args_for(&perf, &b, "software");
        assert!(sw.contains("swiftshader"));
        assert!(sw.contains("--disable-gpu"));
    }

    #[test]
    fn probes_host_backends_and_adapters_without_crashing() {
        let backends = available_backends();
        assert!(backends.contains(&"auto".to_string()));
        assert!(backends.contains(&"software".to_string()));
        let _ = detect_adapters();
    }
}
