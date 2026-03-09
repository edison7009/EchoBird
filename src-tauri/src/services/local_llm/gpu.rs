// GPU detection for all platforms
// Supports: NVIDIA, AMD ROCm, Intel XPU, Apple Silicon,
// and Chinese domestic: Moore Threads, Iluvatar, Cambricon, Biren, KunlunXin

use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use super::types::{GpuInfo, SystemInfo};
use super::settings::{load_model_settings, save_model_settings};

/// Get system information: OS, architecture, and GPU details
pub fn get_system_info() -> SystemInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let gpu = detect_gpu();
    let has_gpu = gpu.is_some();
    let vendor = gpu.as_ref().map(|g| classify_gpu_vendor(&g.gpu_name)).unwrap_or("none");
    SystemInfo {
        os,
        arch,
        gpu_name: gpu.as_ref().map(|g| g.gpu_name.clone()),
        gpu_vram_gb: gpu.as_ref().map(|g| g.gpu_vram_gb),
        has_gpu,
        has_nvidia_gpu: vendor == "nvidia",
        has_amd_gpu: vendor == "amd",
    }
}

/// Detect GPU and persist to settings
pub fn detect_gpu() -> Option<GpuInfo> {
    let info = detect_gpu_system();
    if let Some(ref gpu) = info {
        let mut settings = load_model_settings();
        settings.gpu_name = Some(gpu.gpu_name.clone());
        settings.gpu_vram_gb = Some(gpu.gpu_vram_gb);
        save_model_settings(&settings);
    }
    info
}

/// Get cached GPU info from settings (no re-detection)
pub fn get_gpu_info() -> Option<GpuInfo> {
    let settings = load_model_settings();
    match (settings.gpu_name, settings.gpu_vram_gb) {
        (Some(name), Some(vram)) if !name.is_empty() => Some(GpuInfo {
            gpu_name: name,
            gpu_vram_gb: vram,
        }),
        _ => None,
    }
}

/// Classify GPU vendor from (already-shortened) gpu_name string
fn classify_gpu_vendor(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("rtx") || n.contains("gtx") || n.contains("tesla")
        || n.contains("quadro") || n.contains("titan") || n.contains("nvidia")
        || n.starts_with("a100") || n.starts_with("h100") || n.starts_with("v100")
        || n.starts_with("a10") || n.starts_with("l4") || n.starts_with("l40")
    {
        "nvidia"
    } else if n.starts_with("rx ") || n.contains(" rx ") || n.contains("radeon")
        || n.contains("vega") || n.contains("rdna") || n.contains("amd")
        || n.starts_with("mtt")    // Moore Threads
        || n.starts_with("bi-")    // Iluvatar CoreX
        || n.contains("mlu")       // Cambricon
        || n.starts_with("br")     // Biren
        || n.starts_with("k2") || n.starts_with("k3") // KunlunXin
    {
        "amd"
    } else if n.contains("arc") || n.contains("intel") || n.contains("uhd")
        || n.contains("iris")
    {
        "intel"
    } else {
        "other"
    }
}

/// Shorten verbose GPU names for display
fn shorten_gpu_name(name: &str) -> String {
    name
        // International brands
        .replace("NVIDIA GeForce ", "")
        .replace("NVIDIA RTX ", "RTX ")
        .replace("NVIDIA Tesla ", "Tesla ")
        .replace("NVIDIA ", "")
        .replace("AMD Radeon RX ", "RX ")
        .replace("AMD Radeon PRO ", "Radeon PRO ")
        .replace("AMD Radeon ", "")
        .replace("Intel(R) Arc\u{2122} ", "Arc ")
        .replace("Intel(R) Data Center GPU ", "Intel DC-GPU ")
        .replace("Intel(R) ", "Intel ")
        .replace("Apple ", "")
        // Chinese domestic brands
        .replace("Moore Threads ", "")
        .replace("Iluvatar CoreX ", "")
        .replace("Cambricon ", "")
        .replace("Biren ", "")
        .replace("KunlunXin ", "")
        // Cleanup
        .replace("(TM)", "")
        .replace("(R)", "")
        .replace("  ", " ")
        .trim()
        .to_string()
}

// ─── Platform-specific detection ───

#[cfg(windows)]
fn detect_gpu_system() -> Option<GpuInfo> {
    detect_gpu_nvidia_smi()
        .or_else(detect_gpu_rocm)
        .or_else(detect_gpu_wmic)
}

#[cfg(windows)]
fn detect_gpu_nvidia_smi() -> Option<GpuInfo> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .creation_flags(0x08000000)
        .output()
        .ok()?;

    if !output.status.success() { return None; }

    let stdout = String::from_utf8_lossy(&output.stdout);
    log::info!("[GPU] nvidia-smi output: {}", stdout.trim());

    let first_line = stdout.lines().next()?.trim().to_string();
    let parts: Vec<&str> = first_line.split(',').map(|s| s.trim()).collect();
    if parts.len() >= 2 {
        let vram_mb: f64 = parts[1].parse().unwrap_or(0.0);
        if vram_mb > 0.0 {
            let vram_gb = (vram_mb / 1024.0 * 10.0).round() / 10.0;
            let short_name = shorten_gpu_name(parts[0]);
            log::info!("[GPU] nvidia-smi detected: {} ({:.1} GB VRAM)", short_name, vram_gb);
            return Some(GpuInfo { gpu_name: short_name, gpu_vram_gb: vram_gb });
        }
    }
    None
}

#[cfg(windows)]
fn detect_gpu_wmic() -> Option<GpuInfo> {
    let output = Command::new("wmic")
        .args(["path", "win32_VideoController", "get", "Name,AdapterRAM", "/format:csv"])
        .creation_flags(0x08000000)
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    log::info!("[GPU] wmic output: {}", stdout.trim());

    let mut best_name = String::new();
    let mut best_vram: u64 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("Node") { continue; }
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 3 {
            let adapter_ram: u64 = parts[1].trim().parse().unwrap_or(0);
            let name = parts[2].trim().to_string();
            if adapter_ram > best_vram && !name.is_empty() {
                best_vram = adapter_ram;
                best_name = name;
            }
        }
    }

    if best_name.is_empty() { return None; }

    let vram_gb = best_vram as f64 / (1024.0 * 1024.0 * 1024.0);
    let vram_gb = (vram_gb * 10.0).round() / 10.0;
    let short_name = shorten_gpu_name(&best_name);
    log::info!("[GPU] wmic detected: {} ({:.1} GB VRAM)", short_name, vram_gb);

    Some(GpuInfo { gpu_name: short_name, gpu_vram_gb: vram_gb })
}

#[cfg(windows)]
fn detect_gpu_rocm() -> Option<GpuInfo> {
    let out = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--showname", "--csv"])
        .creation_flags(0x08000000)
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines().skip(1) {
        let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
        if p.len() >= 3 {
            let mb: f64 = p[2].parse().unwrap_or(0.0);
            if mb > 0.0 {
                let gb = (mb / 1024.0 * 10.0).round() / 10.0;
                return Some(GpuInfo { gpu_name: shorten_gpu_name(p[1]), gpu_vram_gb: gb });
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_system() -> Option<GpuInfo> {
    None
        .or_else(detect_gpu_nvidia_smi_unix)
        .or_else(detect_gpu_rocm)
        .or_else(detect_gpu_intel_xpu)
        .or_else(detect_gpu_apple)
        .or_else(detect_gpu_mthreads)
        .or_else(detect_gpu_iluvatar)
        .or_else(detect_gpu_cambricon)
        .or_else(detect_gpu_biren)
        .or_else(detect_gpu_kunlunxin)
}

#[cfg(not(windows))]
fn detect_gpu_nvidia_smi_unix() -> Option<GpuInfo> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim().to_string();
    let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
    if p.len() >= 2 {
        let mb: f64 = p[1].parse().unwrap_or(0.0);
        if mb > 0.0 {
            let gb = (mb / 1024.0 * 10.0).round() / 10.0;
            log::info!("[GPU] nvidia-smi: {} ({:.1}GB)", p[0], gb);
            return Some(GpuInfo { gpu_name: shorten_gpu_name(p[0]), gpu_vram_gb: gb });
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_rocm() -> Option<GpuInfo> {
    let out = Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--showname", "--csv"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    for line in s.lines().skip(1) {
        let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
        if p.len() >= 3 {
            let mb: f64 = p[2].parse().unwrap_or(0.0);
            if mb > 0.0 {
                let gb = (mb / 1024.0 * 10.0).round() / 10.0;
                let name = shorten_gpu_name(p[1]);
                log::info!("[GPU] rocm-smi: {} ({:.1}GB)", name, gb);
                return Some(GpuInfo { gpu_name: name, gpu_vram_gb: gb });
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_intel_xpu() -> Option<GpuInfo> {
    let out = Command::new("xpu-smi").args(["discovery", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dev = json.get("device_list")?.as_array()?.first()?;
    let name = dev.get("device_name").and_then(|v| v.as_str()).unwrap_or("Intel GPU");
    let mb = dev.get("memory_physical_size").and_then(|v| v.as_f64()).unwrap_or(0.0);
    if mb > 0.0 {
        let gb = (mb / 1024.0 * 10.0).round() / 10.0;
        log::info!("[GPU] xpu-smi: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_gpu_apple() -> Option<GpuInfo> {
    let out = Command::new("system_profiler")
        .args(["SPDisplaysDataType", "-json"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    for d in json.get("SPDisplaysDataType")?.as_array()? {
        let name = d.get("sppci_model").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() { continue; }
        let vraw = d.get("spdisplays_vram").and_then(|v| v.as_str()).unwrap_or("0 MB");
        let mb: f64 = vraw.split_whitespace().next()
            .and_then(|n| n.parse().ok()).unwrap_or(0.0);
        let gb = if mb >= 1024.0 { mb / 1024.0 } else { mb };
        let gb = (gb * 10.0).round() / 10.0;
        log::info!("[GPU] Apple: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn detect_gpu_apple() -> Option<GpuInfo> { None }

#[cfg(not(windows))]
fn detect_gpu_mthreads() -> Option<GpuInfo> {
    let out = Command::new("mthreads-gmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("mthreads-gmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Moore Threads MTT".to_string());
    log::info!("[GPU] mthreads-gmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_iluvatar() -> Option<GpuInfo> {
    let out = Command::new("ixsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("ixsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Iluvatar CoreX".to_string());
    log::info!("[GPU] ixsmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_cambricon() -> Option<GpuInfo> {
    let out = Command::new("cnmon").args(["info", "-j"]).output().ok()?;
    if !out.status.success() { return None; }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dev = json.get("device")?.as_array()?.first()?;
    let name = dev.get("Product Name").or_else(|| dev.get("name"))
        .and_then(|v| v.as_str()).unwrap_or("Cambricon MLU");
    let mb = dev.get("Memory Info").and_then(|m| m.get("Total"))
        .and_then(|v| v.as_f64()).unwrap_or(0.0);
    if mb > 0.0 {
        let gb = (mb / 1024.0 * 10.0).round() / 10.0;
        log::info!("[GPU] cnmon: {} ({:.1}GB)", name, gb);
        return Some(GpuInfo { gpu_name: shorten_gpu_name(name), gpu_vram_gb: gb });
    }
    None
}

#[cfg(not(windows))]
fn detect_gpu_biren() -> Option<GpuInfo> {
    let out = Command::new("brsmi").args(["-q", "--display=MEMORY"]).output().ok()?;
    if !out.status.success() { return None; }
    let name_o = Command::new("brsmi").args(["-q", "--display=NAME"]).output().ok()?;
    let gb = parse_vram_mb_line(&String::from_utf8_lossy(&out.stdout))?;
    let name = parse_name_colon(&String::from_utf8_lossy(&name_o.stdout))
        .unwrap_or_else(|| "Biren BR".to_string());
    log::info!("[GPU] brsmi: {} ({:.1}GB)", name, gb);
    Some(GpuInfo { gpu_name: shorten_gpu_name(&name), gpu_vram_gb: gb })
}

#[cfg(not(windows))]
fn detect_gpu_kunlunxin() -> Option<GpuInfo> {
    let out = Command::new("kunlunxin-smi")
        .args(["--query-xpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout);
    let line = s.lines().next()?.trim().to_string();
    let p: Vec<&str> = line.split(',').map(|x| x.trim()).collect();
    if p.len() >= 2 {
        let mb: f64 = p[1].parse().unwrap_or(0.0);
        if mb > 0.0 {
            let gb = (mb / 1024.0 * 10.0).round() / 10.0;
            log::info!("[GPU] kunlunxin-smi: {} ({:.1}GB)", p[0], gb);
            return Some(GpuInfo { gpu_name: shorten_gpu_name(p[0]), gpu_vram_gb: gb });
        }
    }
    None
}

#[cfg(not(windows))]
fn parse_vram_mb_line(text: &str) -> Option<f64> {
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("total") && (lower.contains("mib") || lower.contains("mb")) {
            if let Some(n) = line.split_whitespace().find(|s| s.parse::<f64>().is_ok()) {
                let mb: f64 = n.parse().ok()?;
                if mb > 0.0 { return Some((mb / 1024.0 * 10.0).round() / 10.0); }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn parse_name_colon(text: &str) -> Option<String> {
    text.lines()
        .find(|l| {
            let lower = l.to_lowercase();
            lower.contains("product name") || lower.contains("device name")
        })
        .and_then(|l| l.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
