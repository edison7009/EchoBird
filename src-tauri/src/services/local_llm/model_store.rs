// Model Store: fetch remote model list + download GGUF models + install llama-server engine

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use futures_util::StreamExt;
use tauri::Emitter;

use super::types::DownloadProgress;
use super::settings::get_download_dir;
use super::gpu::get_gpu_info;
use super::server::LocalLlmServer;

// ─── Global download state ───

static DOWNLOAD_ABORT: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_PAUSED: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_FILE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

// ─── Fetch store models ───

/// Fetch store models: remote → cache → empty fallback
pub async fn fetch_store_models() -> Vec<serde_json::Value> {
    let remote_url = "https://echobird.ai/api/store/models.json";
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache");
    let cache_path = cache_dir.join("store-models.json");

    // 1. Try remote
    if let Ok(resp) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default()
        .get(remote_url)
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(models) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    if !models.is_empty() {
                        let _ = std::fs::create_dir_all(&cache_dir);
                        let _ = std::fs::write(&cache_path, &text);
                        log::info!("[ModelStore] Loaded {} models from remote", models.len());
                        return models;
                    }
                }
            }
        }
    }

    // 2. Try cache
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if let Ok(models) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
            if !models.is_empty() {
                log::info!("[ModelStore] Loaded {} models from cache", models.len());
                return models;
            }
        }
    }

    log::warn!("[ModelStore] No models available from remote or cache");
    vec![]
}

// ─── Model download (GGUF) ───

#[derive(Debug, Clone)]
struct DownloadSource {
    name: String,
    url: String,
}

fn build_download_sources(repo: &str, file_name: &str) -> Vec<DownloadSource> {
    vec![
        DownloadSource {
            name: "HuggingFace".to_string(),
            url: format!("https://huggingface.co/{}/resolve/main/{}", repo, file_name),
        },
        DownloadSource {
            name: "HF-Mirror".to_string(),
            url: format!("https://hf-mirror.com/{}/resolve/main/{}", repo, file_name),
        },
        DownloadSource {
            name: "ModelScope".to_string(),
            url: format!("https://modelscope.cn/models/{}/resolve/master/{}", repo, file_name),
        },
    ]
}

async fn test_source_speed(source: &DownloadSource) -> (String, f64) {
    let test_duration = std::time::Duration::from_secs(5);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default();

    let start = std::time::Instant::now();
    let mut bytes: u64 = 0;

    if let Ok(resp) = client.get(&source.url).header("User-Agent", "Echobird/1.1").send().await {
        if resp.status().is_success() {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if start.elapsed() >= test_duration { break; }
                if let Ok(data) = chunk { bytes += data.len() as u64; }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
    log::info!("[ModelStore] Speed test {}: {:.0} KB/s ({:.0} KB in {:.1}s)",
        source.name, speed / 1024.0, bytes as f64 / 1024.0, elapsed);
    (source.name.clone(), speed)
}

/// Download model file with speed test + resume + progress events
pub async fn download_model(
    app_handle: tauri::AppHandle,
    repo: String,
    file_name: String,
) -> Result<String, String> {
    let download_dir = get_download_dir();
    let save_path = PathBuf::from(&download_dir).join(&file_name);
    let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", file_name));

    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some(file_name.clone());

    let _ = std::fs::create_dir_all(&download_dir);

    let sources = build_download_sources(&repo, &file_name);

    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.clone(), progress: 0, downloaded: 0, total: 0,
        status: "speed_test".to_string(),
    });

    log::info!("[ModelStore] Speed testing {} sources for {}...", sources.len(), file_name);
    let speed_results = futures_util::future::join_all(
        sources.iter().map(|s| test_source_speed(s))
    ).await;

    let mut sorted: Vec<_> = speed_results.into_iter()
        .filter(|(_, speed)| *speed > 0.0).collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if sorted.is_empty() {
        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: file_name.clone(), progress: 0, downloaded: 0, total: 0,
            status: "error".to_string(),
        });
        return Err("All download sources unreachable".to_string());
    }

    log::info!("[ModelStore] Fastest source: {} ({:.0} KB/s)", sorted[0].0, sorted[0].1 / 1024.0);

    for (source_name, _) in &sorted {
        let source = sources.iter().find(|s| &s.name == source_name).unwrap();
        match try_download_from_source(&app_handle, source, &save_path, &temp_path, &file_name).await {
            Ok(path) => {
                *DOWNLOAD_FILE.lock().unwrap() = None;
                return Ok(path);
            }
            Err(e) => {
                log::warn!("[ModelStore] {} failed: {}", source_name, e);
                if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                    *DOWNLOAD_FILE.lock().unwrap() = None;
                    return Err("Download cancelled".to_string());
                }
                if DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
                    return Err("Download paused".to_string());
                }
            }
        }
    }

    *DOWNLOAD_FILE.lock().unwrap() = None;
    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.clone(), progress: 0, downloaded: 0, total: 0,
        status: "error".to_string(),
    });
    Err("All download sources failed".to_string())
}

async fn try_download_from_source(
    app_handle: &tauri::AppHandle,
    source: &DownloadSource,
    save_path: &PathBuf,
    temp_path: &PathBuf,
    file_name: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    let start_byte: u64 = if temp_path.exists() {
        std::fs::metadata(temp_path).map(|m| m.len()).unwrap_or(0)
    } else { 0 };

    if start_byte > 0 {
        log::info!("[ModelStore] [{}] Resume mode, {} bytes already downloaded", source.name, start_byte);
    }

    let mut request = client.get(&source.url).header("User-Agent", "Echobird/1.1");
    if start_byte > 0 {
        request = request.header("Range", format!("bytes={}-", start_byte));
    }

    let resp = request.send().await.map_err(|e| format!("[{}] {}", source.name, e))?;
    let status = resp.status();
    if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(format!("[{}] HTTP {}", source.name, status.as_u16()));
    }

    let actual_start = if status == reqwest::StatusCode::OK && start_byte > 0 { 0u64 } else { start_byte };
    let content_length = resp.content_length().unwrap_or(0);
    let total_size = actual_start + content_length;

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true).write(true)
        .append(actual_start > 0).truncate(actual_start == 0)
        .open(temp_path)
        .map_err(|e| format!("File open error: {}", e))?;

    let mut downloaded = actual_start;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }
        if DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: file_name.to_string(), progress: 0, downloaded, total: total_size,
                status: "paused".to_string(),
            });
            return Err("Download paused".to_string());
        }

        let data = chunk.map_err(|e| format!("[{}] Stream error: {}", source.name, e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            let progress = if total_size > 0 {
                ((downloaded as f64 / total_size as f64) * 100.0) as u32
            } else { 0 };
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: file_name.to_string(), progress, downloaded, total: total_size,
                status: "downloading".to_string(),
            });
            last_emit = std::time::Instant::now();
        }
    }

    if save_path.exists() { let _ = std::fs::remove_file(save_path); }
    std::fs::rename(temp_path, save_path).map_err(|e| format!("Rename error: {}", e))?;

    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: file_name.to_string(), progress: 100,
        downloaded: total_size, total: total_size,
        status: "completed".to_string(),
    });

    log::info!("[ModelStore] [{}] Download complete: {}", source.name, file_name);
    Ok(save_path.to_string_lossy().to_string())
}

/// Pause current download
pub fn pause_download() {
    DOWNLOAD_PAUSED.store(true, Ordering::SeqCst);
    log::info!("[ModelStore] Download paused");
}

/// Cancel download and delete temp file
pub fn cancel_download(app_handle: &tauri::AppHandle, target_file_name: Option<String>) {
    DOWNLOAD_ABORT.store(true, Ordering::SeqCst);

    let file_name = target_file_name.or_else(|| DOWNLOAD_FILE.lock().unwrap().clone());

    if let Some(name) = &file_name {
        let download_dir = get_download_dir();
        let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", name));
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
            log::info!("[ModelStore] Cleaned temp file: {}", temp_path.display());
        }
        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: name.clone(), progress: 0, downloaded: 0, total: 0,
            status: "cancelled".to_string(),
        });
    }

    *DOWNLOAD_FILE.lock().unwrap() = None;
    log::info!("[ModelStore] Download cancelled");
}

// ─── Engine version config (remote + cached + fallback) ───

const FALLBACK_LLAMA_VERSION: &str = "b8672";
const FALLBACK_CUDA_VER: &str = "13.1";
const ENGINE_VERSIONS_URL: &str = "https://echobird.ai/api/engine-versions.json";

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct EngineVersionInfo {
    pub version: String,
    #[serde(rename = "cudaVersion", default)]
    pub cuda_version: Option<String>,
    #[serde(default)]
    pub changelog: Option<String>,
}

/// Fetch engine versions: remote → cache → hardcoded fallback
pub fn get_engine_versions() -> std::collections::HashMap<String, EngineVersionInfo> {
    let cache_dir = dirs::home_dir().unwrap_or_default().join(".echobird").join("cache");
    let cache_path = cache_dir.join("engine-versions.json");

    // Try cache first (synchronous — remote fetch is done in background)
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if let Ok(map) = serde_json::from_str(&text) {
            return map;
        }
    }

    // Fallback defaults
    let mut map = std::collections::HashMap::new();
    map.insert("llama-server".to_string(), EngineVersionInfo {
        version: FALLBACK_LLAMA_VERSION.to_string(),
        cuda_version: Some(FALLBACK_CUDA_VER.to_string()),
        changelog: None,
    });
    map
}

/// Fetch latest version of a PyPI package
async fn fetch_pypi_latest(client: &reqwest::Client, package: &str) -> Option<String> {
    let url = format!("https://pypi.org/pypi/{}/json", package);
    let resp = client
        .get(&url)
        .header("User-Agent", "Echobird/3.0")
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() { return None; }
    let json: serde_json::Value = resp.json().await.ok()?;
    json["info"]["version"].as_str().map(|s| s.to_string())
}

/// Fetch engine versions from remote and cache locally (async, called on page load)
pub async fn refresh_engine_versions() -> std::collections::HashMap<String, EngineVersionInfo> {
    let cache_dir = dirs::home_dir().unwrap_or_default().join(".echobird").join("cache");
    let cache_path = cache_dir.join("engine-versions.json");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .unwrap_or_default();

    // Fetch llama-server version from echobird CDN + vllm/sglang from PyPI concurrently
    let (llama_result, vllm_ver, sglang_ver) = tokio::join!(
        client
            .get(ENGINE_VERSIONS_URL)
            .header("User-Agent", "Echobird/3.0")
            .send(),
        fetch_pypi_latest(&client, "vllm"),
        fetch_pypi_latest(&client, "sglang")
    );

    // Start with fallback / cached map
    let mut map = get_engine_versions();

    // Merge llama-server result
    if let Ok(resp) = llama_result {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(remote_map) = serde_json::from_str::<std::collections::HashMap<String, EngineVersionInfo>>(&text) {
                    if !remote_map.is_empty() {
                        for (k, v) in remote_map {
                            map.insert(k, v);
                        }
                        log::info!("[EngineVersions] Refreshed llama-server from remote");
                    }
                }
            }
        }
    }

    // Merge vllm latest
    if let Some(ver) = vllm_ver {
        log::info!("[EngineVersions] vllm latest from PyPI: {}", ver);
        map.insert("vllm".to_string(), EngineVersionInfo {
            version: ver,
            cuda_version: None,
            changelog: None,
        });
    }

    // Merge sglang latest
    if let Some(ver) = sglang_ver {
        log::info!("[EngineVersions] sglang latest from PyPI: {}", ver);
        map.insert("sglang".to_string(), EngineVersionInfo {
            version: ver,
            cuda_version: None,
            changelog: None,
        });
    }

    // Write merged result to cache
    if !map.is_empty() {
        if let Ok(json_str) = serde_json::to_string_pretty(&map) {
            let _ = std::fs::create_dir_all(&cache_dir);
            let _ = std::fs::write(&cache_path, &json_str);
        }
    }

    log::info!("[EngineVersions] Final map has {} engines", map.len());
    map
}

// ─── Engine download: llama-server binary installer ───

fn llama_github_base(version: &str) -> String {
    format!("https://github.com/ggml-org/llama.cpp/releases/download/{}", version)
}

fn llama_download_mirrors(version: &str) -> Vec<String> {
    let base = llama_github_base(version);
    vec![
        base.clone(),
        format!("https://ghfast.top/{}", base),
        format!("https://ghproxy.net/{}", base),
        format!("https://ghproxy.homeboyc.cn/{}", base),
        format!("https://github.ur1.fun/{}", base),
        format!("https://gh-proxy.com/{}", base),
        format!("https://mirror.ghproxy.com/{}", base),
    ]
}

fn classify_gpu_vendor_for_download(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if n.contains("rtx") || n.contains("gtx") || n.contains("tesla")
        || n.contains("quadro") || n.contains("titan") || n.contains("nvidia")
        || n.starts_with("a100") || n.starts_with("h100") || n.starts_with("v100")
    { "nvidia" } else { "other" }
}

fn get_llama_platform_files(has_nvidia: bool, version: &str, cuda_ver: &str) -> Vec<String> {
    match std::env::consts::OS {
        "windows" => {
            if has_nvidia {
                vec![
                    format!("llama-{}-bin-win-cuda-{}-x64.zip", version, cuda_ver),
                    format!("cudart-llama-bin-win-cuda-{}-x64.zip", cuda_ver),
                ]
            } else {
                vec![format!("llama-{}-bin-win-avx2-x64.zip", version)]
            }
        }
        "macos" => vec![format!("llama-{}-bin-macos-arm64.tar.gz", version)],
        _ => {
            let arch = std::env::consts::ARCH;
            if arch == "aarch64" || arch == "arm" {
                vec![format!("llama-{}-bin-ubuntu-arm64.tar.gz", version)]
            } else {
                vec![format!("llama-{}-bin-ubuntu-x64.tar.gz", version)]
            }
        }
    }
}

fn llama_install_dir() -> PathBuf {
    crate::utils::platform::echobird_dir().join("llama-server")
}

async fn test_mirror_speed(url: String, name: String) -> (String, String, f64) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(10))
        .build() {
        Ok(c) => c,
        Err(_) => return (name, url, 0.0),
    };

    let start = std::time::Instant::now();
    let mut bytes: u64 = 0;

    if let Ok(resp) = client.get(&url).header("User-Agent", "Echobird/1.1").send().await {
        if resp.status().is_success() {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if let Ok(data) = chunk { bytes += data.len() as u64; }
                if start.elapsed() >= std::time::Duration::from_secs(5) { break; }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 { bytes as f64 / elapsed } else { 0.0 };
    log::info!("[LlamaDownloader] Speed test {}: {:.0} KB/s ({} KB in {:.1}s)",
        name, speed / 1024.0, bytes / 1024, elapsed);
    (name, url, speed)
}

/// Download and install llama-server binary
pub async fn download_llama_server(app_handle: tauri::AppHandle) -> Result<String, String> {
    // Fetch latest version from remote config
    let versions = refresh_engine_versions().await;
    let llama_info = versions.get("llama-server");
    let version = llama_info.map(|i| i.version.as_str()).unwrap_or(FALLBACK_LLAMA_VERSION);
    let cuda_ver = llama_info.and_then(|i| i.cuda_version.as_deref()).unwrap_or(FALLBACK_CUDA_VER);
    log::info!("[LlamaDownloader] Using version={}, cuda={}", version, cuda_ver);

    let gpu_info = get_gpu_info();
    let has_nvidia = gpu_info.as_ref()
        .map(|g| classify_gpu_vendor_for_download(&g.gpu_name) == "nvidia")
        .unwrap_or(false);
    log::info!("[LlamaDownloader] GPU vendor: {}, has_nvidia={}",
        gpu_info.as_ref().map(|g| g.gpu_name.as_str()).unwrap_or("none"), has_nvidia);

    let file_names = get_llama_platform_files(has_nvidia, version, cuda_ver);
    let bin_dir = llama_install_dir().join("bin");
    let temp_dir = llama_install_dir().join("temp");
    let mirrors = llama_download_mirrors(version);

    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some("llama-server".to_string());

    let _ = std::fs::create_dir_all(&bin_dir);
    let _ = std::fs::create_dir_all(&temp_dir);

    let total_files = file_names.len();
    let mut completed_files = 0u32;

    for file_name in &file_names {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("Download cancelled".to_string());
        }

        let temp_file = temp_dir.join(file_name);

        let _ = app_handle.emit("download-progress", DownloadProgress {
            file_name: "llama-server".to_string(), progress: 0, downloaded: 0, total: 0,
            status: "speed_test".to_string(),
        });

        log::info!("[LlamaDownloader] Speed testing {} mirrors for {}...", mirrors.len(), file_name);
        let speed_results = futures_util::future::join_all(
            mirrors.iter().enumerate().map(|(i, mirror)| {
                let url = format!("{}/{}", mirror, file_name);
                let name = if i == 0 {
                    "GitHub".to_string()
                } else {
                    url::Url::parse(mirror)
                        .map(|u| u.host_str().unwrap_or("unknown").to_string())
                        .unwrap_or_else(|_| format!("Mirror-{}", i))
                };
                test_mirror_speed(url, name)
            })
        ).await;

        let mut sorted: Vec<_> = speed_results.into_iter()
            .filter(|(_, _, speed)| *speed > 0.0).collect();
        sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        if sorted.is_empty() {
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: 0, downloaded: 0, total: 0, status: "error".to_string(),
            });
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("All download mirrors unreachable".to_string());
        }

        log::info!("[LlamaDownloader] Fastest: {} ({:.0} KB/s)", sorted[0].0, sorted[0].2 / 1024.0);

        let mut download_ok = false;
        for (mirror_name, mirror_url, _) in &sorted {
            if DOWNLOAD_ABORT.load(Ordering::SeqCst) { break; }
            log::info!("[LlamaDownloader] Downloading via {}: {}", mirror_name, mirror_url);
            match download_engine_file(&app_handle, mirror_url, &temp_file, completed_files, total_files as u32).await {
                Ok(_) => { download_ok = true; break; }
                Err(e) => {
                    log::warn!("[LlamaDownloader] {} failed: {}", mirror_name, e);
                    let _ = std::fs::remove_file(&temp_file);
                    if DOWNLOAD_ABORT.load(Ordering::SeqCst) { break; }
                }
            }
        }

        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("Download cancelled".to_string());
        }

        if !download_ok {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: 0, downloaded: 0, total: 0, status: "error".to_string(),
            });
            *DOWNLOAD_FILE.lock().unwrap() = None;
            return Err("All download mirrors failed".to_string());
        }

        // Extract
        log::info!("[LlamaDownloader] Extracting: {}", file_name);
        let extract_name = file_name.replace(".zip", "").replace(".tar.gz", "");
        let extract_dir = bin_dir.join(&extract_name);
        let _ = std::fs::create_dir_all(&extract_dir);

        if file_name.ends_with(".zip") {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let status = Command::new("powershell")
                    .args(["-NoProfile", "-Command",
                        &format!("Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                            temp_file.display(), extract_dir.display())])
                    .creation_flags(0x08000000)
                    .status()
                    .map_err(|e| format!("Extract failed: {}", e))?;
                if !status.success() {
                    return Err(format!("PowerShell Expand-Archive failed for {}", file_name));
                }
            }
            #[cfg(not(windows))]
            return Err("ZIP extraction is only supported on Windows".to_string());
        } else {
            let status = Command::new("tar")
                .args(["-xzf", &temp_file.to_string_lossy(), "-C", &extract_dir.to_string_lossy()])
                .status()
                .map_err(|e| format!("Extract failed: {}", e))?;
            if !status.success() {
                return Err(format!("tar extraction failed for {}", file_name));
            }
        }

        let _ = std::fs::remove_file(&temp_file);
        completed_files += 1;
    }

    #[cfg(not(windows))]
    {
        if let Some(exe_path) = LocalLlmServer::find_llama_server() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&exe_path, std::fs::Permissions::from_mode(0o755));
            log::info!("[LlamaDownloader] Set executable permission: {}", exe_path.display());
        }
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
    let _ = app_handle.emit("download-progress", DownloadProgress {
        file_name: "llama-server".to_string(), progress: 100, downloaded: 0, total: 0,
        status: "completed".to_string(),
    });

    *DOWNLOAD_FILE.lock().unwrap() = None;

    let install_path = LocalLlmServer::find_llama_server()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    log::info!("[LlamaDownloader] Installation complete: {}", install_path);
    Ok(install_path)
}

async fn download_engine_file(
    app_handle: &tauri::AppHandle,
    url: &str,
    dest: &PathBuf,
    completed_files: u32,
    total_files: u32,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(url).header("User-Agent", "Echobird/1.1")
        .send().await.map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }

    let content_length = resp.content_length().unwrap_or(0);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true).write(true).truncate(true)
        .open(dest)
        .map_err(|e| format!("File open error: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    while let Some(chunk) = stream.next().await {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }
        let data = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&data).map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            let file_progress = if content_length > 0 {
                (downloaded as f64 / content_length as f64) * 100.0
            } else { 0.0 };
            let overall = ((completed_files as f64 + file_progress / 100.0) / total_files as f64 * 100.0) as u32;
            let _ = app_handle.emit("download-progress", DownloadProgress {
                file_name: "llama-server".to_string(),
                progress: overall, downloaded, total: content_length,
                status: "downloading".to_string(),
            });
            last_emit = std::time::Instant::now();
        }
    }

    Ok(())

}

// ─── Engine status detection ───

#[allow(dead_code)]
fn check_python_package(package: &str) -> Option<String> {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        Command::new("pip3")
            .args(["show", package])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    };
    #[cfg(not(windows))]
    let result = Command::new("pip3")
        .args(["show", package])
        .output();

    if let Ok(out) = result {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.to_lowercase().starts_with("version:") {
                    return Some(line[8..].trim().to_string());
                }
            }
            return Some(String::new());
        }
    }
    None
}

/// Collect all installed binary directory names under the bin folder
fn get_installed_llama_binary_names() -> Vec<String> {
    let bin_dir = llama_install_dir().join("bin");
    if !bin_dir.exists() { return vec![]; }
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&bin_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
    }
    // Sort: llama-b* first, then cudart-*, then rest
    names.sort_by(|a, b| {
        let a_main = a.starts_with("llama-b");
        let b_main = b.starts_with("llama-b");
        b_main.cmp(&a_main).then(a.cmp(b))
    });
    names
}

/// Detect installed llama-server binary directory name for version parsing
fn get_installed_llama_binary_name() -> Option<String> {
    get_installed_llama_binary_names().into_iter().find(|n| n.starts_with("llama-b"))
}

/// Detect installed llama-server version from directory name (e.g. "llama-b7981-bin-win-cuda-...")
fn get_installed_llama_version() -> Option<String> {
    get_installed_llama_binary_name().and_then(|name| {
        if let Some(ver_end) = name.find("-bin-") {
            let ver = &name[6..ver_end]; // skip "llama-" prefix → already "bNNNN"
            Some(ver.to_string())
        } else {
            None
        }
    })
}

/// Get installation status for the specified runtime only (lazy — avoids unnecessary pip3 calls).
/// Pass `runtime_filter = None` to check all engines (legacy / admin use).
pub fn get_local_engine_status(runtime_filter: Option<&str>) -> serde_json::Value {
    let versions = get_engine_versions();

    // ── llama-server (always cheap — binary lookup, no pip) ──────────────────
    let check_llama = runtime_filter.map(|r| r == "llama-server").unwrap_or(true);
    let llama_installed = if check_llama { LocalLlmServer::find_llama_server().is_some() } else { false };
    let installed_ver   = if check_llama { get_installed_llama_version().unwrap_or_default() } else { String::new() };
    let binary_names    = if check_llama { get_installed_llama_binary_names() } else { vec![] };
    let latest_llama    = versions.get("llama-server").map(|i| i.version.as_str()).unwrap_or(FALLBACK_LLAMA_VERSION);

    // ── vllm / sglang — Linux-only + only when explicitly selected ───────────
    let _check_vllm   = runtime_filter.map(|r| r == "vllm").unwrap_or(false);
    let _check_sglang = runtime_filter.map(|r| r == "sglang").unwrap_or(false);

    #[cfg(target_os = "linux")]
    let vllm_version = if _check_vllm { check_python_package("vllm") } else { None };
    #[cfg(not(target_os = "linux"))]
    let vllm_version: Option<String> = None;

    #[cfg(target_os = "linux")]
    let sglang_version = if _check_sglang { check_python_package("sglang") } else { None };
    #[cfg(not(target_os = "linux"))]
    let sglang_version: Option<String> = None;

    let latest_vllm   = versions.get("vllm").map(|i| i.version.clone());
    let latest_sglang = versions.get("sglang").map(|i| i.version.clone());

    serde_json::json!({
        "engines": [
            {
                "name": "llama-server",
                "installed": llama_installed,
                "version": installed_ver,
                "latestVersion": latest_llama,
                "installDir": llama_install_dir().to_string_lossy(),
                "binaryNames": binary_names
            },
            {
                "name": "vllm",
                "installed": vllm_version.is_some(),
                "version": vllm_version.clone().unwrap_or_default(),
                "latestVersion": latest_vllm
            },
            {
                "name": "sglang",
                "installed": sglang_version.is_some(),
                "version": sglang_version.clone().unwrap_or_default(),
                "latestVersion": latest_sglang
            }
        ]
    })
}

/// Install engine for local use. Routes by runtime:
/// - llama-server: binary download (auto-versioned from remote config)
/// - vllm / sglang: pip3 install
pub async fn install_local_engine(app_handle: tauri::AppHandle, runtime: String) -> Result<(), String> {
    match runtime.as_str() {
        "llama-server" => {
            // Upgrade: remove old bin directory before installing new version
            let bin_dir = llama_install_dir().join("bin");
            if bin_dir.exists() {
                log::info!("[EngineInstaller] Removing old llama-server installation at {:?}", bin_dir);
                let _ = std::fs::remove_dir_all(&bin_dir);
            }
            download_llama_server(app_handle).await.map(|_| ())
        }
        "vllm" => {
            #[cfg(target_os = "linux")]
            { install_pip_engine(&app_handle, "vllm", &runtime).await }
            #[cfg(not(target_os = "linux"))]
            { Err("vllm is only supported on Linux".to_string()) }
        }
        "sglang" => {
            #[cfg(target_os = "linux")]
            { install_pip_engine(&app_handle, "sglang[all]", &runtime).await }
            #[cfg(not(target_os = "linux"))]
            { Err("sglang is only supported on Linux".to_string()) }
        }
        other => {
            Err(format!("Unknown runtime: {}", other))
        }
    }
}

/// Install a Python package via pip3, emitting download-progress events for UI
#[allow(dead_code)]
async fn install_pip_engine(
    app_handle: &tauri::AppHandle,
    package: &str,
    runtime: &str,
) -> Result<(), String> {
    let runtime = runtime.to_string();
    let package = package.to_string();
    let app = app_handle.clone();

    // Emit: installing started
    let _ = app.emit("download-progress", DownloadProgress {
        file_name: runtime.clone(),
        progress: 5,
        downloaded: 0,
        total: 0,
        status: "installing".to_string(),
    });

    log::info!("[EngineInstaller] pip3 install {} for runtime '{}'", package, runtime);

    // Pip install with PyPI mirrors (prefer China mirrors for faster access)
    let result = tokio::task::spawn_blocking({
        let package = package.clone();
        let runtime = runtime.clone();
        let app = app.clone();
        move || {
            // Try primary install
            #[cfg(windows)]
            let status = {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                Command::new("pip3")
                    .args([
                        "install", &package,
                        "--upgrade",
                        "-i", "https://pypi.tuna.tsinghua.edu.cn/simple",
                        "--trusted-host", "pypi.tuna.tsinghua.edu.cn",
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            };
            #[cfg(not(windows))]
            let status = Command::new("pip3")
                .args([
                    "install", &package,
                    "--upgrade",
                    "-i", "https://pypi.tuna.tsinghua.edu.cn/simple",
                    "--trusted-host", "pypi.tuna.tsinghua.edu.cn",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn();

            match status {
                Ok(mut child) => {
                    use std::io::{BufRead, BufReader};

                    // Emit progress=30 once child spawned
                    let _ = app.emit("download-progress", DownloadProgress {
                        file_name: runtime.clone(),
                        progress: 30,
                        downloaded: 0,
                        total: 0,
                        status: "installing".to_string(),
                    });

                    // Stream stderr (pip outputs to stderr)
                    if let Some(stderr) = child.stderr.take() {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines().flatten() {
                            log::info!("[pip3] {}", line);
                        }
                    }

                    match child.wait() {
                        Ok(status) if status.success() => Ok(()),
                        Ok(status) => {
                            // Fallback: retry with official PyPI
                            log::warn!("[EngineInstaller] Tsinghua mirror failed ({}), retrying with official PyPI", status);
                            let _ = app.emit("download-progress", DownloadProgress {
                                file_name: runtime.clone(),
                                progress: 50,
                                downloaded: 0,
                                total: 0,
                                status: "installing".to_string(),
                            });
                            #[cfg(windows)]
                            let result2 = {
                                use std::os::windows::process::CommandExt;
                                const CREATE_NO_WINDOW: u32 = 0x08000000;
                                Command::new("pip3")
                                    .args(["install", &package, "--upgrade"])
                                    .creation_flags(CREATE_NO_WINDOW)
                                    .output()
                            };
                            #[cfg(not(windows))]
                            let result2 = Command::new("pip3")
                                .args(["install", &package, "--upgrade"])
                                .output();
                            match result2 {
                                Ok(out) if out.status.success() => Ok(()),
                                Ok(out) => Err(format!(
                                    "pip3 install failed:\n{}",
                                    String::from_utf8_lossy(&out.stderr)
                                )),
                                Err(e) => Err(format!("pip3 not found: {}", e)),
                            }
                        }
                        Err(e) => Err(format!("Failed to wait for pip3: {}", e)),
                    }
                }
                Err(e) => Err(format!("pip3 not found or failed to spawn: {}", e)),
            }
        }
    }).await.map_err(|e| format!("Task join error: {}", e))?;

    match result {
        Ok(()) => {
            log::info!("[EngineInstaller] {} installed successfully", package);
            let _ = app.emit("download-progress", DownloadProgress {
                file_name: runtime.clone(),
                progress: 100,
                downloaded: 0,
                total: 0,
                status: "completed".to_string(),
            });
            Ok(())
        }
        Err(e) => {
            log::error!("[EngineInstaller] {} install failed: {}", package, e);
            let _ = app.emit("download-progress", DownloadProgress {
                file_name: runtime.clone(),
                progress: 0,
                downloaded: 0,
                total: 0,
                status: "error".to_string(),
            });
            Err(e)
        }
    }
}
