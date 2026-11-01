// Model Store: fetch remote model list + download GGUF models + install llama-server engine

use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

use super::gpu::get_gpu_info;
use super::server::LocalLlmServer;
use super::settings::get_download_dir;
use super::types::DownloadProgress;

// ─── Global download state ───

static DOWNLOAD_ABORT: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_PAUSED: AtomicBool = AtomicBool::new(false);
static DOWNLOAD_ACTIVE: AtomicBool = AtomicBool::new(false);
// Holds all shard filenames of the currently-running download job. cancel_download
// reads this to know which .downloading temp files to wipe. Single-file engine
// installs wrap their identifier ("llama-server") in a one-element Vec to keep the
// shape uniform.
static DOWNLOAD_FILE: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

struct DownloadJob;

fn acquire_download_job() -> Result<DownloadJob, String> {
    DOWNLOAD_ACTIVE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "A download or engine installation is already running".to_string())?;
    Ok(DownloadJob)
}

impl Drop for DownloadJob {
    fn drop(&mut self) {
        *DOWNLOAD_FILE.lock().unwrap() = None;
        DOWNLOAD_ACTIVE.store(false, Ordering::SeqCst);
    }
}

fn finish_download_job<T>(
    job: DownloadJob,
    result: Result<T, String>,
    on_terminal: impl FnOnce(&str),
) -> Result<T, String> {
    let status = if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
        "cancelled"
    } else if DOWNLOAD_PAUSED.load(Ordering::SeqCst) && result.is_err() {
        "paused"
    } else if result.is_err() {
        "error"
    } else {
        "completed"
    };
    if status == "cancelled" {
        if let Some(files) = DOWNLOAD_FILE.lock().unwrap().as_ref() {
            remove_partial_downloads(files);
        }
    }
    drop(job);
    on_terminal(status);
    match status {
        "cancelled" => Err("Download cancelled".to_string()),
        "paused" => Err("Download paused".to_string()),
        _ => result,
    }
}

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
    // ModelScope has its own repository namespace; an HF repo is not a
    // ModelScope mirror identifier.
    [
        ("HuggingFace", "https://huggingface.co"),
        ("HF-Mirror", "https://hf-mirror.com"),
    ]
    .into_iter()
    .map(|(name, endpoint)| {
        let mut url = reqwest::Url::parse(endpoint).unwrap();
        url.path_segments_mut()
            .unwrap()
            .extend(repo.split('/'))
            .extend(["resolve", "main"])
            .extend(file_name.split('/'));
        DownloadSource {
            name: name.to_string(),
            url: url.to_string(),
        }
    })
    .collect()
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

    if let Ok(resp) = client
        .get(&source.url)
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        if resp.status().is_success() {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if start.elapsed() >= test_duration {
                    break;
                }
                if let Ok(data) = chunk {
                    bytes += data.len() as u64;
                }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        bytes as f64 / elapsed
    } else {
        0.0
    };
    log::info!(
        "[ModelStore] Speed test {}: {:.0} KB/s ({:.0} KB in {:.1}s)",
        source.name,
        speed / 1024.0,
        bytes as f64 / 1024.0,
        elapsed
    );
    (source.name.clone(), speed)
}

fn rank_download_sources(mut speeds: Vec<(String, f64)>) -> Vec<(String, f64)> {
    // Zero signal during the short probe does not mean the source cannot
    // download a later shard. Keep it as a fallback after measured sources.
    speeds.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    speeds
}

/// Emit a download-progress event. Centralised so adding fields (e.g.
/// shard_index/shard_count) is a one-line struct update rather than 17
/// scattered struct-literal edits.
fn emit_dl_progress(
    app: &tauri::AppHandle,
    file_name: &str,
    progress: u32,
    downloaded: u64,
    total: u64,
    status: &str,
    shard: Option<(u32, u32)>,
) {
    let _ = app.emit(
        "download-progress",
        DownloadProgress {
            file_name: file_name.to_string(),
            progress,
            downloaded,
            total,
            status: status.to_string(),
            shard_index: shard.map(|(i, _)| i),
            shard_count: shard.map(|(_, c)| c),
        },
    );
}

/// Download a model that may be a single GGUF or a sharded multi-file GGUF.
///
/// All shards share one speed-test round (we only re-run speed test for the
/// first shard); subsequent shards reuse the source ranking. Each shard gets
/// independent Range-resume + source fallback. `file_name` reported in
/// progress events stays pinned to `files[0]` so the frontend's
/// Map<fileName, DownloadItem> keeps working across the whole job. Returns
/// the path of the first shard on success — callers (model scan) discover
/// the rest by listing the download directory.
pub async fn download_model(
    app_handle: tauri::AppHandle,
    repo: String,
    files: Vec<String>,
) -> Result<String, String> {
    if files.is_empty() {
        return Err("download_model: empty file list".to_string());
    }
    let job = acquire_download_job()?;
    let primary = std::path::Path::new(&files[0])
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| files[0].clone());
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some(files.clone());
    let mut progress = DownloadProgress {
        file_name: primary,
        ..Default::default()
    };
    let result = download_model_inner(app_handle.clone(), repo, files, &mut progress).await;
    finish_download_job(job, result, |status| {
        progress.status = status.to_string();
        let _ = app_handle.emit("download-progress", progress);
    })
}

async fn download_model_inner(
    app_handle: tauri::AppHandle,
    repo: String,
    files: Vec<String>,
    progress: &mut DownloadProgress,
) -> Result<String, String> {
    let download_dir = get_download_dir();
    std::fs::create_dir_all(&download_dir)
        .map_err(|e| format!("Download directory error: {}", e))?;
    // Match the frontend's flat download directory and primary event key.
    let primary = progress.file_name.clone();
    let shard_count = files.len() as u32;
    let is_multi_shard = shard_count > 1;

    // Initial speed test against the first file. Subsequent shards reuse the
    // ranking — running a fresh 5s test per shard would cost minutes on a
    // 50-shard model with no signal value (HF/Mirror speed is
    // per-host-pair, not per-file).
    let sources = build_download_sources(&repo, &files[0]);

    emit_dl_progress(&app_handle, &primary, 0, 0, 0, "speed_test", None);

    log::info!(
        "[ModelStore] Speed testing {} sources for {} ({} shard{})...",
        sources.len(),
        primary,
        shard_count,
        if is_multi_shard { "s" } else { "" }
    );
    let speed_results = tokio::select! {
        results = futures_util::future::join_all(sources.iter().map(test_source_speed)) => results,
        reason = wait_for_download_stop(&DOWNLOAD_ABORT, &DOWNLOAD_PAUSED) => {
            return Err(reason);
        }
    };

    let sorted = rank_download_sources(speed_results);

    if sorted.iter().all(|(_, speed)| *speed == 0.0) {
        // Speed test got no bytes in 5s. This isn't reliable on flaky
        // connections — HF's first-byte latency for sharded large files
        // can spike past 5s while the actual download streams fine.
        // Don't reject the job here; fall back to the default source
        // order and let try_download_shard's longer-lived connection
        // do the real attempt.
        log::warn!(
            "[ModelStore] Speed test got no signal in 5s for {} — falling back to default source order",
            primary
        );
    } else {
        log::info!(
            "[ModelStore] Fastest source: {} ({:.0} KB/s)",
            sorted[0].0,
            sorted[0].1 / 1024.0
        );
    }

    // Track aggregated bytes across already-completed shards so the overall
    // progress bar climbs monotonically across shard boundaries.
    let mut aggregated_completed_bytes: u64 = 0;
    let mut first_shard_path: Option<String> = None;

    for (shard_idx_0, file_name) in files.iter().enumerate() {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }

        // `file_name` is the HF-side relative path, which for sharded unsloth
        // GGUFs includes a quantization subdir (e.g.
        // "UD-Q4_K_XL/Model-UD-Q4_K_XL-00001-of-00050.gguf"). The subdir is
        // needed to build the download URL, but we flatten on save so the
        // local tree matches what scan_gguf_files (which reports basenames)
        // expects — and so llama-server's --model arg points at a flat dir.
        let basename = std::path::Path::new(file_name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| file_name.clone());
        let save_path = PathBuf::from(&download_dir).join(&basename);
        let temp_path = save_path.with_file_name(format!("{}.downloading", basename));

        // Skip shards already fully present on disk (e.g. resuming after
        // app restart, or partial completion before). isDownloaded check
        // on the frontend reflects whole-model state; per-shard we just
        // probe disk.
        if save_path.exists() {
            if let Ok(meta) = std::fs::metadata(&save_path) {
                aggregated_completed_bytes += meta.len();
            }
            log::info!(
                "[ModelStore] Shard {}/{} already on disk, skipping: {}",
                shard_idx_0 + 1,
                shard_count,
                file_name
            );
            if shard_idx_0 == 0 {
                first_shard_path = Some(save_path.to_string_lossy().to_string());
            }
            continue;
        }

        let shard_info = if is_multi_shard {
            Some((shard_idx_0 as u32 + 1, shard_count))
        } else {
            None
        };

        let shard_sources = build_download_sources(&repo, file_name);
        let mut shard_ok = false;
        for (source_name, _) in &sorted {
            let source = match shard_sources.iter().find(|s| &s.name == source_name) {
                Some(s) => s,
                None => continue,
            };
            match try_download_shard(
                &app_handle,
                source,
                &save_path,
                &temp_path,
                progress,
                aggregated_completed_bytes,
                shard_info,
            )
            .await
            {
                Ok((path, shard_bytes)) => {
                    aggregated_completed_bytes += shard_bytes;
                    if shard_idx_0 == 0 {
                        first_shard_path = Some(path);
                    }
                    shard_ok = true;
                    break;
                }
                Err(e) => {
                    log::warn!(
                        "[ModelStore] {} failed on shard {}: {}",
                        source_name,
                        file_name,
                        e
                    );
                    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                        return Err("Download cancelled".to_string());
                    }
                    if DOWNLOAD_PAUSED.load(Ordering::SeqCst) {
                        return Err("Download paused".to_string());
                    }
                }
            }
        }

        if !shard_ok {
            return Err(format!(
                "All download sources failed for shard {}",
                file_name
            ));
        }
    }

    progress.progress = 100;
    progress.downloaded = aggregated_completed_bytes;
    progress.total = aggregated_completed_bytes;
    progress.shard_index = None;
    progress.shard_count = None;
    log::info!(
        "[ModelStore] Download complete: {} ({} shard{}, {} bytes)",
        primary,
        shard_count,
        if is_multi_shard { "s" } else { "" },
        aggregated_completed_bytes
    );
    Ok(first_shard_path.unwrap_or_else(|| {
        PathBuf::from(&download_dir)
            .join(&primary)
            .to_string_lossy()
            .to_string()
    }))
}

/// Download one shard with Range-resume support. Returns the final on-disk
/// path and the total byte count for this shard so the caller can
/// accumulate overall progress across shards.
async fn try_download_shard(
    app_handle: &tauri::AppHandle,
    source: &DownloadSource,
    save_path: &std::path::Path,
    temp_path: &std::path::Path,
    progress: &mut DownloadProgress,
    aggregated_completed_bytes: u64,
    shard: Option<(u32, u32)>,
) -> Result<(String, u64), String> {
    download_shard_to_file(
        source,
        save_path,
        temp_path,
        &DOWNLOAD_ABORT,
        &DOWNLOAD_PAUSED,
        |downloaded, total, status| {
            progress.progress = if total > 0 {
                ((downloaded as f64 / total as f64) * 100.0).min(100.0) as u32
            } else {
                0
            };
            progress.downloaded = aggregated_completed_bytes + downloaded;
            progress.total = if total > 0 {
                aggregated_completed_bytes + total
            } else {
                0
            };
            progress.status = status.to_string();
            progress.shard_index = shard.map(|(i, _)| i);
            progress.shard_count = shard.map(|(_, count)| count);
            // A terminal event opens the frontend for retries. Publish it only
            // after the worker has stopped and released the job guard.
            if status != "paused" {
                let _ = app_handle.emit("download-progress", progress.clone());
            }
        },
    )
    .await
}

async fn wait_for_download_stop(abort: &AtomicBool, paused: &AtomicBool) -> String {
    loop {
        if abort.load(Ordering::SeqCst) {
            return "Download cancelled".to_string();
        }
        if paused.load(Ordering::SeqCst) {
            return "Download paused".to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

fn shard_response_size(resp: &reqwest::Response, start: u64) -> Result<Option<u64>, String> {
    if resp.status() == reqwest::StatusCode::OK {
        return Ok(resp.content_length());
    }
    let range = resp
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("bytes "))
        .and_then(|v| v.split_once('/'))
        .and_then(|(range, total)| {
            let (first, last) = range.split_once('-')?;
            Some((
                first.parse::<u64>().ok()?,
                last.parse::<u64>().ok()?,
                total.parse::<u64>().ok()?,
            ))
        })
        .ok_or_else(|| "Invalid or missing Content-Range".to_string())?;
    let (first, last, total) = range;
    if first != start || last < first || last >= total {
        return Err("Content-Range does not match the requested offset".to_string());
    }
    if resp
        .content_length()
        .is_some_and(|length| length != last - first + 1)
    {
        return Err("Content-Length does not match Content-Range".to_string());
    }
    Ok(Some(total))
}

async fn download_shard_to_file(
    source: &DownloadSource,
    save_path: &std::path::Path,
    temp_path: &std::path::Path,
    abort: &AtomicBool,
    paused: &AtomicBool,
    mut on_progress: impl FnMut(u64, u64, &str),
) -> Result<(String, u64), String> {
    use std::io::{Read, Write};

    let etag_path = temp_path.with_extension("downloading.etag");
    let resume_etag = std::fs::read_to_string(&etag_path)
        .ok()
        .filter(|etag| etag.starts_with('"') && etag.ends_with('"'));
    // An old partial without a strong validator cannot safely be combined
    // with today's main revision or another source's representation.
    let mut downloaded = if resume_etag.is_some() {
        std::fs::metadata(temp_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    let mut total = None;
    let result = async {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .connect_timeout(std::time::Duration::from_secs(15))
            .read_timeout(std::time::Duration::from_secs(30))
            .no_gzip()
            .build()
            .map_err(|e| e.to_string())?;
        let mut request = client.get(&source.url)
            .header("User-Agent", "Echobird/1.1")
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if downloaded > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={}-", downloaded))
                .header(reqwest::header::IF_RANGE, resume_etag.as_deref().unwrap());
        }
        let mut resp = tokio::select! {
            response = request.send() => response.map_err(|e| format!("[{}] {}", source.name, e))?,
            reason = wait_for_download_stop(abort, paused) => return Err(reason),
        };
        // A complete or outdated partial may be outside the current file.
        // Retry from zero rather than getting stuck on the same 416 forever.
        if resp.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && downloaded > 0 {
            downloaded = 0;
            resp = tokio::select! {
                response = client.get(&source.url).header("User-Agent", "Echobird/1.1")
                    .header(reqwest::header::ACCEPT_ENCODING, "identity").send() =>
                    response.map_err(|e| format!("[{}] {}", source.name, e))?,
                reason = wait_for_download_stop(abort, paused) => return Err(reason),
            };
        }
        let status = resp.status();
        if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(format!("[{}] HTTP {}", source.name, status.as_u16()));
        }
        if resp.headers().get(reqwest::header::CONTENT_ENCODING)
            .is_some_and(|encoding| encoding != "identity") {
            return Err("Unexpected compressed model response".to_string());
        }
        let response_etag = resp.headers().get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|etag| etag.starts_with('"') && etag.ends_with('"'));
        if status == reqwest::StatusCode::PARTIAL_CONTENT && downloaded > 0
            && response_etag != resume_etag.as_deref() {
            return Err("Resumed model ETag changed".to_string());
        }
        total = shard_response_size(&resp, downloaded)?;
        if status == reqwest::StatusCode::OK {
            downloaded = 0;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true).write(true)
            .append(downloaded > 0).truncate(downloaded == 0)
            .open(temp_path).map_err(|e| format!("File open error: {}", e))?;
        if let Some(etag) = response_etag {
            std::fs::write(&etag_path, etag).map_err(|e| format!("Resume metadata error: {}", e))?;
        } else {
            let _ = std::fs::remove_file(&etag_path);
        }
        on_progress(downloaded, total.unwrap_or(0), "downloading");
        let mut stream = resp.bytes_stream();
        let mut last_emit = std::time::Instant::now();
        loop {
            let chunk = tokio::select! {
                chunk = stream.next() => match chunk {
                    Some(chunk) => chunk.map_err(|e| format!("[{}] Stream error: {}", source.name, e))?,
                    None => break,
                },
                reason = wait_for_download_stop(abort, paused) => return Err(reason),
            };
            file.write_all(&chunk).map_err(|e| format!("Write error: {}", e))?;
            downloaded += chunk.len() as u64;
            if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
                on_progress(downloaded, total.unwrap_or(0), "downloading");
                last_emit = std::time::Instant::now();
            }
        }
        if downloaded == 0 || total.is_some_and(|expected| downloaded != expected) {
            return Err(format!("Incomplete model download: {} bytes, expected {:?}", downloaded, total));
        }
        file.flush().map_err(|e| format!("Flush error: {}", e))?;
        drop(file);
        let mut magic = [0; 4];
        if std::fs::File::open(temp_path).and_then(|mut file| file.read_exact(&mut magic)).is_err()
            || magic != *b"GGUF" {
            let _ = std::fs::remove_file(temp_path);
            let _ = std::fs::remove_file(&etag_path);
            return Err("Downloaded file is not a GGUF model".to_string());
        }
        if abort.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }
        if paused.load(Ordering::SeqCst) {
            return Err("Download paused".to_string());
        }
        std::fs::rename(temp_path, save_path).map_err(|e| format!("Rename error: {}", e))?;
        let _ = std::fs::remove_file(&etag_path);
        log::info!("[ModelStore] [{}] Shard complete: {}", source.name, save_path.display());
        Ok((save_path.to_string_lossy().to_string(), downloaded))
    }.await;
    // Release the write handle before cleanup, including on Windows. Cancel
    // can arrive while a response/body is pending and must still finish.
    if abort.load(Ordering::SeqCst) {
        let _ = std::fs::remove_file(temp_path);
        let _ = std::fs::remove_file(&etag_path);
    } else if paused.load(Ordering::SeqCst) && result.is_err() {
        on_progress(downloaded, total.unwrap_or(0), "paused");
    }
    result
}

/// Pause current download
pub fn pause_download() {
    DOWNLOAD_PAUSED.store(true, Ordering::SeqCst);
    log::info!("[ModelStore] Download paused");
}

/// Cancel download and delete temp files for ALL shards of the current job.
///
/// `target_files` lets a paused-then-cancelled UI explicitly name the shards
/// to wipe (because the paused job releases DOWNLOAD_FILE). When
/// None, falls back to DOWNLOAD_FILE — the shards currently being downloaded.
pub fn cancel_download(app_handle: &tauri::AppHandle, target_files: Option<Vec<String>>) {
    let files = request_download_cancel(target_files);
    if let Some(files) = &files {
        remove_partial_downloads(files);
        // UI tracks the job under files[0] as the primary key; emit the
        // basename to match what the download loop reports as fileName.
        if let Some(primary) = files.first() {
            let primary_basename = std::path::Path::new(primary)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| primary.clone());
            emit_dl_progress(app_handle, &primary_basename, 0, 0, 0, "cancelled", None);
        }
    }

    log::info!("[ModelStore] Download cancelled");
}

fn request_download_cancel(target_files: Option<Vec<String>>) -> Option<Vec<String>> {
    let active_files = DOWNLOAD_FILE.lock().unwrap();
    if cancel_targets_active_job(&target_files, &active_files) {
        DOWNLOAD_ABORT.store(true, Ordering::SeqCst);
        // The worker owns cleanup and the final event. Keep its identity
        // until it exits so repeated cancels cannot emit an early terminal.
        None
    } else {
        target_files
    }
}

fn remove_partial_downloads(files: &[String]) {
    let download_dir = get_download_dir();
    for name in files {
        // Keep cleanup aligned with download_model's flat on-disk paths.
        let basename = std::path::Path::new(name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| name.clone());
        let temp_path = PathBuf::from(&download_dir).join(format!("{}.downloading", basename));
        let _ = std::fs::remove_file(&temp_path);
        let _ = std::fs::remove_file(temp_path.with_extension("downloading.etag"));
    }
}

fn cancel_targets_active_job(target: &Option<Vec<String>>, active: &Option<Vec<String>>) -> bool {
    match target {
        None => active.is_some(),
        Some(target) => active.as_ref() == Some(target),
    }
}

#[cfg(test)]
mod model_download_tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    static DOWNLOAD_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct Fixture {
        dir: PathBuf,
        source: DownloadSource,
        requests: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        server: tokio::task::JoinHandle<()>,
    }

    impl Fixture {
        async fn new(responses: Vec<&'static str>, hold_open: bool) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let source = DownloadSource {
                name: "Fixture".into(),
                url: format!("http://{}/model.gguf", listener.local_addr().unwrap()),
            };
            let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = requests.clone();
            let server = tokio::spawn(async move {
                for response in responses {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 1024];
                    while !request.ends_with(b"\r\n\r\n") {
                        let size = socket.read(&mut buffer).await.unwrap();
                        assert!(size > 0);
                        request.extend_from_slice(&buffer[..size]);
                    }
                    captured
                        .lock()
                        .unwrap()
                        .push(String::from_utf8(request).unwrap());
                    socket.write_all(response.as_bytes()).await.unwrap();
                    if hold_open {
                        std::future::pending::<()>().await;
                    }
                }
            });
            let dir =
                std::env::temp_dir().join(format!("echobird-download-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self {
                dir,
                source,
                requests,
                server,
            }
        }

        fn save_path(&self) -> PathBuf {
            self.dir.join("model.gguf")
        }
        fn temp_path(&self) -> PathBuf {
            self.dir.join("model.gguf.downloading")
        }

        fn partial(&self, bytes: &[u8], etag: Option<&str>) {
            std::fs::write(self.temp_path(), bytes).unwrap();
            if let Some(etag) = etag {
                std::fs::write(self.temp_path().with_extension("downloading.etag"), etag).unwrap();
            }
        }

        async fn download(&self) -> Result<(String, u64), String> {
            download_shard_to_file(
                &self.source,
                &self.save_path(),
                &self.temp_path(),
                &AtomicBool::new(false),
                &AtomicBool::new(false),
                |_, _, _| {},
            )
            .await
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.abort();
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn shard_sources_preserve_subdirectories_and_escape_filename_segments() {
        let sources = build_download_sources("publisher/model", "Q4/model #1.gguf");
        assert_eq!(sources.len(), 2);
        assert_eq!(
            sources[0].url,
            "https://huggingface.co/publisher/model/resolve/main/Q4/model%20%231.gguf"
        );
        assert_eq!(
            sources[1].url,
            "https://hf-mirror.com/publisher/model/resolve/main/Q4/model%20%231.gguf"
        );
        let ranked = rank_download_sources(vec![("HF".into(), 0.0), ("mirror".into(), 100.0)]);
        assert_eq!(ranked, vec![("mirror".into(), 100.0), ("HF".into(), 0.0)]);
        assert_eq!(
            rank_download_sources(vec![("HF".into(), 0.0), ("mirror".into(), 0.0)]).len(),
            2
        );
    }

    #[test]
    fn download_job_rejects_overlap_and_releases_on_drop() {
        let _lock = DOWNLOAD_TEST_LOCK.blocking_lock();
        let job = acquire_download_job().unwrap();
        assert!(acquire_download_job().is_err());
        drop(job);
        assert!(acquire_download_job().is_ok());
    }

    #[test]
    fn cancelling_a_paused_model_does_not_abort_another_active_job() {
        let _lock = DOWNLOAD_TEST_LOCK.blocking_lock();
        let job = acquire_download_job().unwrap();
        let model = Some(vec!["model.gguf".into()]);
        let engine = Some(vec!["llama-server".into()]);
        DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
        *DOWNLOAD_FILE.lock().unwrap() = engine.clone();
        assert_eq!(request_download_cancel(model.clone()), model);
        assert!(!DOWNLOAD_ABORT.load(Ordering::SeqCst));
        assert_eq!(*DOWNLOAD_FILE.lock().unwrap(), engine);
        assert!(!cancel_targets_active_job(&model, &engine));
        assert!(!cancel_targets_active_job(&model, &None));
        assert!(cancel_targets_active_job(&model, &model));
        assert!(cancel_targets_active_job(&None, &engine));
        drop(job);
        assert_eq!(request_download_cancel(model.clone()), model);
        assert!(!DOWNLOAD_ABORT.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stopped_workers_emit_their_last_event_after_releasing_the_job() {
        let _lock = DOWNLOAD_TEST_LOCK.lock().await;
        for cancel in [false, true] {
            let fixture = Fixture::new(
                vec!["HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"same\"\r\n\r\nGGUF"],
                true,
            )
            .await;
            let job = acquire_download_job().unwrap();
            DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
            DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
            let files = Some(vec![format!(
                "echobird-download-fixture-{}.gguf",
                uuid::Uuid::new_v4()
            )]);
            *DOWNLOAD_FILE.lock().unwrap() = files.clone();
            let temp = fixture.temp_path();
            let save = fixture.save_path();
            let mut statuses = Vec::new();
            let mut last_bytes = (0, 0);
            let stop = async {
                while std::fs::metadata(&temp).map(|m| m.len()).unwrap_or(0) != 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                // Positive control: a live worker has written data and owns the lock.
                assert!(acquire_download_job().is_err());
                if cancel {
                    assert!(request_download_cancel(files.clone()).is_none());
                    assert!(request_download_cancel(files.clone()).is_none());
                    assert_eq!(*DOWNLOAD_FILE.lock().unwrap(), files);
                } else {
                    pause_download();
                }
            };
            let download = download_shard_to_file(
                &fixture.source,
                &save,
                &temp,
                &DOWNLOAD_ABORT,
                &DOWNLOAD_PAUSED,
                |downloaded, total, status| {
                    last_bytes = (downloaded, total);
                    if status != "paused" {
                        statuses.push(status.to_string());
                    }
                },
            );
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::join!(download, stop)
            })
            .await
            .expect("pause and cancel must stop the active HTTP worker");
            let expected = if cancel { "cancelled" } else { "paused" };
            let result = finish_download_job(job, result, |status| {
                assert_eq!(status, expected);
                assert_eq!(temp.exists(), !cancel);
                assert_eq!(temp.with_extension("downloading.etag").exists(), !cancel);
                if !cancel {
                    assert_eq!(last_bytes, (4, 8));
                }
                let retry =
                    acquire_download_job().expect("a terminal event must permit immediate retry");
                // A new worker resets global flags; the old result must stay final.
                DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
                DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
                drop(retry);
                statuses.push(status.to_string());
            });
            assert_eq!(result.unwrap_err(), format!("Download {expected}"));
            assert_eq!(statuses.last().map(String::as_str), Some(expected));
            assert!(!save.exists());
        }
    }

    #[test]
    fn errors_and_success_finish_after_releasing_the_job() {
        let _lock = DOWNLOAD_TEST_LOCK.blocking_lock();
        for result in [
            Err("Create directory failed".into()),
            Err("Engine validation failed".into()),
            Ok(8_u64),
        ] {
            let job = acquire_download_job().unwrap();
            DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
            DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
            *DOWNLOAD_FILE.lock().unwrap() = Some(vec!["llama-server".into()]);
            let expected = if result.is_ok() { "completed" } else { "error" };
            let mut statuses = Vec::new();
            let finished = finish_download_job(job, result.clone(), |status| {
                assert_eq!(status, expected);
                let retry = acquire_download_job().expect("terminal events must release the job");
                drop(retry);
                statuses.push(status.to_string());
            });
            assert_eq!(finished, result);
            assert_eq!(statuses, [expected]);
        }
    }

    #[tokio::test]
    async fn validated_partial_resume_uses_range_and_if_range() {
        let fixture = Fixture::new(vec!["HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nETag: \"same\"\r\nConnection: close\r\n\r\ndata"], false).await;
        fixture.partial(b"GGUF", Some("\"same\""));
        assert_eq!(fixture.download().await.unwrap().1, 8);
        assert_eq!(std::fs::read(fixture.save_path()).unwrap(), b"GGUFdata");
        let request = fixture.requests.lock().unwrap()[0].to_lowercase();
        assert!(request.contains("range: bytes=4-\r\n"));
        assert!(request.contains("if-range: \"same\"\r\n"));
        assert!(!fixture
            .temp_path()
            .with_extension("downloading.etag")
            .exists());
    }

    #[tokio::test]
    async fn changed_file_or_unvalidated_partial_restarts_without_combining_bytes() {
        for etag in [None, Some("\"old\"")] {
            let fixture = Fixture::new(vec!["HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"new\"\r\nConnection: close\r\n\r\nGGUFnew!"], false).await;
            fixture.partial(b"GGUFold", etag);
            assert_eq!(fixture.download().await.unwrap().1, 8);
            assert_eq!(std::fs::read(fixture.save_path()).unwrap(), b"GGUFnew!");
            if etag.is_none() {
                assert!(!fixture.requests.lock().unwrap()[0]
                    .to_lowercase()
                    .contains("\r\nrange:"));
            }
        }
    }

    #[tokio::test]
    async fn invalid_ranges_and_changed_validators_never_publish_a_model() {
        for response in [
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 0-3/8\r\nETag: \"same\"\r\nConnection: close\r\n\r\nbad!",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nETag: \"same\"\r\nConnection: close\r\n\r\ndata",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nETag: \"changed\"\r\nConnection: close\r\n\r\ndata",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 4-5/8\r\nETag: \"same\"\r\nConnection: close\r\n\r\nda",
        ] {
            let fixture = Fixture::new(vec![response], false).await;
            fixture.partial(b"GGUF", Some("\"same\""));
            assert!(fixture.download().await.is_err());
            assert!(!fixture.save_path().exists());
            assert!(fixture.temp_path().exists());
        }
    }

    #[tokio::test]
    async fn unknown_length_counts_actual_bytes_and_html_is_rejected() {
        let fixture = Fixture::new(vec!["HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\nGGUFdata\r\n0\r\n\r\n"], false).await;
        assert_eq!(fixture.download().await.unwrap().1, 8);
        assert_eq!(std::fs::read(fixture.save_path()).unwrap(), b"GGUFdata");
        let invalid = Fixture::new(
            vec!["HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\n<html>"],
            false,
        )
        .await;
        assert!(invalid.download().await.unwrap_err().contains("not a GGUF"));
        assert!(!invalid.save_path().exists());
        assert!(!invalid.temp_path().exists());
    }

    #[tokio::test]
    async fn truncated_response_preserves_partial_for_a_valid_resume() {
        let fixture = Fixture::new(vec![
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"same\"\r\nConnection: close\r\n\r\nGGUF",
            "HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nETag: \"same\"\r\nConnection: close\r\n\r\ndata",
        ], false).await;
        assert!(fixture.download().await.is_err());
        assert!(!fixture.save_path().exists());
        assert_eq!(std::fs::read(fixture.temp_path()).unwrap(), b"GGUF");
        assert_eq!(fixture.download().await.unwrap().1, 8);
        assert_eq!(std::fs::read(fixture.save_path()).unwrap(), b"GGUFdata");
    }

    #[tokio::test]
    async fn range_not_satisfiable_retries_from_zero() {
        let fixture = Fixture::new(vec![
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nContent-Range: bytes */8\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"same\"\r\nConnection: close\r\n\r\nGGUFdata",
        ], false).await;
        fixture.partial(b"GGUFdata", Some("\"same\""));
        assert_eq!(fixture.download().await.unwrap().1, 8);
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
        assert!(!fixture.requests.lock().unwrap()[1]
            .to_lowercase()
            .contains("\r\nrange:"));
    }

    #[tokio::test]
    async fn pause_and_cancel_stop_a_stalled_body_with_expected_partial_cleanup() {
        for cancel in [false, true] {
            let fixture = Fixture::new(
                vec!["HTTP/1.1 200 OK\r\nContent-Length: 8\r\nETag: \"same\"\r\n\r\nGGUF"],
                true,
            )
            .await;
            let abort = AtomicBool::new(false);
            let paused = AtomicBool::new(false);
            let mut statuses = Vec::new();
            let temp = fixture.temp_path();
            let save = fixture.save_path();
            let stop = async {
                while std::fs::metadata(&temp).map(|m| m.len()).unwrap_or(0) != 4 {
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
                if cancel {
                    abort.store(true, Ordering::SeqCst);
                } else {
                    paused.store(true, Ordering::SeqCst);
                }
            };
            let download = download_shard_to_file(
                &fixture.source,
                &save,
                &temp,
                &abort,
                &paused,
                |_, _, status| statuses.push(status.to_string()),
            );
            let (result, ()) = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                tokio::join!(download, stop)
            })
            .await
            .expect("download control must interrupt stalled HTTP bodies");
            assert_eq!(
                result.unwrap_err(),
                if cancel {
                    "Download cancelled"
                } else {
                    "Download paused"
                }
            );
            assert!(!save.exists());
            assert_eq!(temp.exists(), !cancel);
            assert_eq!(temp.with_extension("downloading.etag").exists(), !cancel);
            if !cancel {
                assert!(statuses.iter().any(|status| status == "paused"));
            }
        }
    }
}

// ─── Engine version config (remote + cached + fallback) ───
//
// All three engines auto-track their upstream sources at runtime — no CDN
// snapshot or manual JSON bump required on our side:
//   • llama.cpp  ← newest release containing native binary assets
//   • vllm       ← pypi.org/pypi/vllm/json
//   • sglang     ← pypi.org/pypi/sglang/json
// FALLBACK_LLAMA_VERSION is only used when GitHub is unreachable (offline,
// rate limit, etc.); the client falls back to the previously-cached map and
// finally to this constant. Bump it during release prep to a known-good build.

const FALLBACK_LLAMA_VERSION: &str = "b11553";
const FALLBACK_CUDA_VER: &str = "13.4";

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
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache");
    let cache_path = cache_dir.join("engine-versions.json");

    // Try cache first (synchronous — remote fetch is done in background)
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if let Ok(mut map) =
            serde_json::from_str::<std::collections::HashMap<String, EngineVersionInfo>>(&text)
        {
            // Migrate caches written by the old /releases/latest lookup.
            if map.get("llama-server").is_some_and(|info| {
                info.version
                    .strip_prefix('b')
                    .and_then(|tag| tag.parse::<u64>().ok())
                    .is_none()
            }) {
                map.insert(
                    "llama-server".to_string(),
                    EngineVersionInfo {
                        version: FALLBACK_LLAMA_VERSION.to_string(),
                        cuda_version: Some(FALLBACK_CUDA_VER.to_string()),
                        changelog: None,
                    },
                );
            }
            return map;
        }
    }

    // Fallback defaults
    let mut map = std::collections::HashMap::new();
    map.insert(
        "llama-server".to_string(),
        EngineVersionInfo {
            version: FALLBACK_LLAMA_VERSION.to_string(),
            cuda_version: Some(FALLBACK_CUDA_VER.to_string()),
            changelog: None,
        },
    );
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
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    json["info"]["version"].as_str().map(|s| s.to_string())
}

/// Semantic releases contain only a nightly-tag.txt pointer. Keep following
/// the newest actual binary build, including upstream's nightly prereleases.
async fn fetch_llama_latest_binary_release(client: &reqwest::Client) -> Option<EngineVersionInfo> {
    let url = "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page=30";
    let resp = client
        .get(url)
        .header("User-Agent", "Echobird/3.0")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let releases: Vec<serde_json::Value> = resp.json().await.ok()?;
    latest_llama_binary_release(&releases, std::env::consts::OS, std::env::consts::ARCH)
}

fn parse_win_cuda_asset(name: &str, arch: &str) -> Option<(String, String)> {
    let asset_arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return None,
    };
    let stem = name
        .strip_prefix("llama-")?
        .strip_suffix(&format!("-{}.zip", asset_arch))?;
    let (tag, cuda) = stem.split_once("-bin-win-cuda-")?;
    tag.strip_prefix('b')?.parse::<u64>().ok()?;
    let (major, minor) = cuda.split_once('.')?;
    major.parse::<u32>().ok()?;
    minor.parse::<u32>().ok()?;
    Some((tag.to_string(), cuda.to_string()))
}

fn latest_llama_binary_release(
    releases: &[serde_json::Value],
    os: &str,
    arch: &str,
) -> Option<EngineVersionInfo> {
    for release in releases {
        if release["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let Some(tag) = release["tag_name"].as_str() else {
            continue;
        };
        if tag
            .strip_prefix('b')
            .and_then(|s| s.parse::<u64>().ok())
            .is_none()
        {
            continue;
        }
        let Some(assets) = release["assets"].as_array() else {
            continue;
        };
        let Ok(cpu_files) =
            get_llama_platform_files(os, arch, false, false, tag, FALLBACK_CUDA_VER)
        else {
            return None;
        };
        if !assets
            .iter()
            .any(|asset| asset["name"].as_str() == Some(cpu_files[0].as_str()))
        {
            continue;
        }
        let cuda_version = assets
            .iter()
            .filter_map(|asset| parse_win_cuda_asset(asset["name"].as_str()?, arch))
            .filter(|(asset_tag, _)| asset_tag == tag)
            .map(|(_, cuda)| cuda)
            .max_by_key(|cuda| {
                let (major, minor) = cuda.split_once('.').unwrap();
                (major.parse::<u32>().unwrap(), minor.parse::<u32>().unwrap())
            });
        return Some(EngineVersionInfo {
            version: tag.to_string(),
            cuda_version,
            changelog: None,
        });
    }
    None
}

/// One pickable Windows CUDA build of llama.cpp — surfaced to the user
/// so they can choose which engine variant to install. Picker is gated
/// on Windows + NVIDIA in the frontend; non-NVIDIA variants are noise.
///
/// `rename_all = "camelCase"` so the JS side sees `cudaVersion` rather
/// than `cuda_version`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlamaReleaseOption {
    pub tag: String,
    pub cuda_version: String,
    pub asset_name: String,
    pub size_bytes: u64,
}

/// Fetch top N recent llama.cpp releases, emit one entry per Windows CUDA
/// variant per release. Newest first, flattened.
///
/// Returns empty Vec on network failure / rate-limit; the frontend should
/// fall back to the existing "auto-latest" install path.
pub async fn fetch_llama_release_options(top_n: usize) -> Vec<LlamaReleaseOption> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let url = format!(
        "https://api.github.com/repos/ggml-org/llama.cpp/releases?per_page={}",
        top_n.clamp(1, 30)
    );
    let resp = match client
        .get(&url)
        .header("User-Agent", "Echobird/3.0")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };

    let releases: Vec<serde_json::Value> = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for release in releases {
        if release["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag_in_release = release["tag_name"].as_str().map(|s| s.to_string());
        let assets = match release["assets"].as_array() {
            Some(a) => a,
            None => continue,
        };
        for asset in assets {
            let name = match asset["name"].as_str() {
                Some(s) => s,
                None => continue,
            };
            let (parsed_tag, cuda_version) =
                match parse_win_cuda_asset(name, std::env::consts::ARCH) {
                    Some(t) => t,
                    None => continue,
                };
            // Prefer the asset-name-embedded tag; the release tag is a fallback.
            let tag = if !parsed_tag.is_empty() {
                parsed_tag
            } else if let Some(t) = tag_in_release.clone() {
                t
            } else {
                continue;
            };
            let size_bytes = asset["size"].as_u64().unwrap_or(0);
            out.push(LlamaReleaseOption {
                tag,
                cuda_version,
                asset_name: name.to_string(),
                size_bytes,
            });
        }
    }

    out
}

/// Fetch engine versions from remote and cache locally (async, called on page load)
pub async fn refresh_engine_versions() -> std::collections::HashMap<String, EngineVersionInfo> {
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache");
    let cache_path = cache_dir.join("engine-versions.json");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .unwrap_or_default();

    // All three engines fetched concurrently from their canonical upstreams.
    let (llama_info, vllm_ver, sglang_ver) = tokio::join!(
        fetch_llama_latest_binary_release(&client),
        fetch_pypi_latest(&client, "vllm"),
        fetch_pypi_latest(&client, "sglang")
    );

    // Start with fallback / cached map
    let mut map = get_engine_versions();

    // Merge llama-server result
    if let Some(info) = llama_info {
        log::info!(
            "[EngineVersions] llama.cpp latest from GitHub: {}",
            info.version
        );
        map.insert("llama-server".to_string(), info);
    }

    // Merge vllm latest
    if let Some(ver) = vllm_ver {
        log::info!("[EngineVersions] vllm latest from PyPI: {}", ver);
        map.insert(
            "vllm".to_string(),
            EngineVersionInfo {
                version: ver,
                cuda_version: None,
                changelog: None,
            },
        );
    }

    // Merge sglang latest
    if let Some(ver) = sglang_ver {
        log::info!("[EngineVersions] sglang latest from PyPI: {}", ver);
        map.insert(
            "sglang".to_string(),
            EngineVersionInfo {
                version: ver,
                cuda_version: None,
                changelog: None,
            },
        );
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
    format!(
        "https://github.com/ggml-org/llama.cpp/releases/download/{}",
        version
    )
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
    if n.contains("rtx")
        || n.contains("gtx")
        || n.contains("tesla")
        || n.contains("quadro")
        || n.contains("titan")
        || n.contains("nvidia")
        || n.starts_with("a100")
        || n.starts_with("h100")
        || n.starts_with("v100")
    {
        "nvidia"
    } else if n.contains("radeon")
        || n.contains("vega")
        || n.contains("rdna")
        || n.contains("amd")
        || n.starts_with("rx ")
    {
        "amd"
    } else {
        "other"
    }
}

fn get_llama_platform_files(
    os: &str,
    arch: &str,
    has_nvidia: bool,
    has_amd: bool,
    version: &str,
    cuda_ver: &str,
) -> Result<Vec<String>, String> {
    let asset_arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err(format!("No llama.cpp binary installer for {} {}", os, arch)),
    };
    Ok(match os {
        "windows" => {
            if has_nvidia {
                vec![
                    format!(
                        "llama-{}-bin-win-cuda-{}-{}.zip",
                        version, cuda_ver, asset_arch
                    ),
                    format!("cudart-llama-bin-win-cuda-{}-{}.zip", cuda_ver, asset_arch),
                ]
            } else {
                // Upstream renamed the non-CUDA Windows build from `avx2` to
                // `cpu` somewhere before b8672. The old name 404s on every
                // recent release.
                vec![format!("llama-{}-bin-win-cpu-{}.zip", version, asset_arch)]
            }
        }
        "macos" => vec![format!("llama-{}-bin-macos-{}.tar.gz", version, asset_arch)],
        "linux" => {
            if arch == "aarch64" {
                vec![format!("llama-{}-bin-ubuntu-arm64.tar.gz", version)]
            } else if has_nvidia || has_amd {
                // Preserve the existing GPU installer choice; upstream also
                // ships CUDA/ROCm variants with separate runtime dependencies.
                vec![format!("llama-{}-bin-ubuntu-vulkan-x64.tar.gz", version)]
            } else {
                vec![format!("llama-{}-bin-ubuntu-x64.tar.gz", version)]
            }
        }
        _ => return Err(format!("No llama.cpp binary installer for {}", os)),
    })
}

fn llama_install_dir() -> PathBuf {
    crate::utils::platform::echobird_dir().join("llama-server")
}

async fn test_mirror_speed(url: String, name: String) -> (String, String, f64) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(_) => return (name, url, 0.0),
    };

    let start = std::time::Instant::now();
    let mut bytes: u64 = 0;

    if let Ok(resp) = client
        .get(&url)
        .header("Range", "bytes=0-1048575")
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        if resp.status().is_success() {
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                if let Ok(data) = chunk {
                    bytes += data.len() as u64;
                }
                if bytes >= 1048576 || start.elapsed() >= std::time::Duration::from_secs(5) {
                    break;
                }
            }
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 {
        bytes as f64 / elapsed
    } else {
        0.0
    };
    log::info!(
        "[LlamaDownloader] Speed test {}: {:.0} KB/s ({} KB in {:.1}s)",
        name,
        speed / 1024.0,
        bytes / 1024,
        elapsed
    );
    (name, url, speed)
}

/// Download and install llama-server binary.
///
/// Optional overrides let the frontend's engine-version picker pin a
/// specific (tag, cuda) pair instead of auto-resolving "latest". If
/// both are `None`, the original auto-latest behavior runs unchanged.
pub async fn download_llama_server(
    app_handle: tauri::AppHandle,
    override_version: Option<String>,
    override_cuda: Option<String>,
) -> Result<String, String> {
    let job = acquire_download_job()?;
    DOWNLOAD_ABORT.store(false, Ordering::SeqCst);
    DOWNLOAD_PAUSED.store(false, Ordering::SeqCst);
    *DOWNLOAD_FILE.lock().unwrap() = Some(vec!["llama-server".to_string()]);
    let result =
        download_llama_server_inner(app_handle.clone(), override_version, override_cuda).await;
    finish_download_job(job, result, |status| {
        emit_dl_progress(
            &app_handle,
            "llama-server",
            if status == "completed" { 100 } else { 0 },
            0,
            0,
            status,
            None,
        );
    })
}

async fn download_llama_server_inner(
    app_handle: tauri::AppHandle,
    override_version: Option<String>,
    override_cuda: Option<String>,
) -> Result<String, String> {
    let bin_final = llama_install_dir().join("bin");
    // An explicit retry must restore the last usable engine even if its
    // release lookup or download subsequently fails.
    recover_interrupted_llama_install(&bin_final)?;
    let ignore_pause = AtomicBool::new(false);
    // Fetch latest version from remote config (used when overrides are absent)
    let versions = tokio::select! {
        versions = refresh_engine_versions() => versions,
        reason = wait_for_download_stop(&DOWNLOAD_ABORT, &ignore_pause) => return Err(reason),
    };
    let llama_info = versions.get("llama-server");
    let version: String = override_version.unwrap_or_else(|| {
        llama_info
            .map(|i| i.version.clone())
            .unwrap_or_else(|| FALLBACK_LLAMA_VERSION.to_string())
    });
    let cuda_ver: String = override_cuda.unwrap_or_else(|| {
        llama_info
            .and_then(|i| i.cuda_version.clone())
            .unwrap_or_else(|| FALLBACK_CUDA_VER.to_string())
    });
    let version = version.as_str();
    let cuda_ver = cuda_ver.as_str();
    log::info!(
        "[LlamaDownloader] Using version={}, cuda={}",
        version,
        cuda_ver
    );

    let gpu_info = get_gpu_info();
    let gpu_vendor = gpu_info
        .as_ref()
        .map(|g| classify_gpu_vendor_for_download(&g.gpu_name))
        .unwrap_or("none");
    let has_nvidia = gpu_vendor == "nvidia";
    let has_amd = gpu_vendor == "amd";
    log::info!(
        "[LlamaDownloader] GPU vendor: {}, has_nvidia={}, has_amd={}",
        gpu_info
            .as_ref()
            .map(|g| g.gpu_name.as_str())
            .unwrap_or("none"),
        has_nvidia,
        has_amd
    );

    if version
        .strip_prefix('b')
        .and_then(|s| s.parse::<u64>().ok())
        .is_none()
    {
        return Err(format!("Invalid llama.cpp binary build: {}", version));
    }
    let file_names = get_llama_platform_files(
        std::env::consts::OS,
        std::env::consts::ARCH,
        has_nvidia,
        has_amd,
        version,
        cuda_ver,
    )?;
    // Atomic install: extract to bin.new/, swap to bin/ only on success.
    // Cancelling or failing the download leaves the existing bin/ runnable.
    let bin_dir = llama_install_dir().join("bin.new");
    let temp_dir = llama_install_dir().join("temp");
    let mirrors = llama_download_mirrors(version);

    struct InstallCleanup {
        staging: PathBuf,
        temp: PathBuf,
    }
    impl Drop for InstallCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.staging);
            let _ = std::fs::remove_dir_all(&self.temp);
        }
    }
    let _cleanup = InstallCleanup {
        staging: bin_dir.clone(),
        temp: temp_dir.clone(),
    };

    // Clear any leftover bin.new from a previous interrupted run.
    let _ = std::fs::remove_dir_all(&bin_dir);
    std::fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("Create install directory failed: {}", e))?;
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("Create download directory failed: {}", e))?;

    let total_files = file_names.len();

    for (completed_files, file_name) in file_names.iter().enumerate() {
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            return Err("Download cancelled".to_string());
        }

        let temp_file = temp_dir.join(file_name);

        emit_dl_progress(&app_handle, "llama-server", 0, 0, 0, "speed_test", None);

        log::info!(
            "[LlamaDownloader] Speed testing {} mirrors for {}...",
            mirrors.len(),
            file_name
        );
        let speed_tests =
            futures_util::future::join_all(mirrors.iter().enumerate().map(|(i, mirror)| {
                let url = format!("{}/{}", mirror, file_name);
                let name = if i == 0 {
                    "GitHub".to_string()
                } else {
                    url::Url::parse(mirror)
                        .map(|u| u.host_str().unwrap_or("unknown").to_string())
                        .unwrap_or_else(|_| format!("Mirror-{}", i))
                };
                test_mirror_speed(url, name)
            }));
        let speed_results = tokio::select! {
            results = speed_tests => results,
            reason = wait_for_download_stop(&DOWNLOAD_ABORT, &ignore_pause) => return Err(reason),
        };

        // Keep zero-signal sources as fallbacks behind the measured ones.
        let mut sorted = speed_results;
        sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        log::info!(
            "[LlamaDownloader] Fastest: {} ({:.0} KB/s)",
            sorted[0].0,
            sorted[0].2 / 1024.0
        );

        let mut download_ok = false;
        for (mirror_name, mirror_url, _) in &sorted {
            if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                break;
            }
            log::info!(
                "[LlamaDownloader] Downloading via {}: {}",
                mirror_name,
                mirror_url
            );
            match download_engine_file(
                &app_handle,
                mirror_url,
                &temp_file,
                completed_files as u32,
                total_files as u32,
            )
            .await
            {
                Ok(_) => {
                    download_ok = true;
                    break;
                }
                Err(e) => {
                    log::warn!("[LlamaDownloader] {} failed: {}", mirror_name, e);
                    let _ = std::fs::remove_file(&temp_file);
                    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }

        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            let _ = std::fs::remove_dir_all(&temp_dir);
            let _ = std::fs::remove_dir_all(&bin_dir);
            return Err("Download cancelled".to_string());
        }

        if !download_ok {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err("All download mirrors failed".to_string());
        }

        // Extract
        log::info!("[LlamaDownloader] Extracting: {}", file_name);
        // CUDA runtime DLLs must live beside llama-server.exe, not in a
        // sibling cudart folder which Windows' loader does not search.
        let extract_name = file_names[0].replace(".zip", "").replace(".tar.gz", "");
        let extract_dir = bin_dir.join(&extract_name);
        std::fs::create_dir_all(&extract_dir)
            .map_err(|e| format!("Create extraction directory failed: {}", e))?;

        if file_name.ends_with(".zip") {
            #[cfg(windows)]
            {
                let status = crate::utils::process::command("powershell")
                    .args([
                        "-NoProfile",
                        "-Command",
                        &format!(
                            "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force -ErrorAction Stop",
                            temp_file.to_string_lossy().replace('\'', "''"),
                            extract_dir.to_string_lossy().replace('\'', "''")
                        ),
                    ])
                    .status()
                    .map_err(|e| format!("Extract failed: {}", e))?;
                if !status.success() {
                    return Err(format!(
                        "PowerShell Expand-Archive failed for {}",
                        file_name
                    ));
                }
            }
            #[cfg(not(windows))]
            return Err("ZIP extraction is only supported on Windows".to_string());
        } else {
            let status = crate::utils::process::command("tar")
                .args([
                    "-xzf",
                    &temp_file.to_string_lossy(),
                    "-C",
                    &extract_dir.to_string_lossy(),
                ])
                .status()
                .map_err(|e| format!("Extract failed: {}", e))?;
            if !status.success() {
                return Err(format!("tar extraction failed for {}", file_name));
            }
        }

        let _ = std::fs::remove_file(&temp_file);
    }

    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
        return Err("Download cancelled".to_string());
    }
    let staged_server = find_staged_llama_server(&bin_dir)
        .ok_or_else(|| "Downloaded archive does not contain llama-server".to_string())?;

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged_server, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Set executable permission failed: {}", e))?;
    }

    let probe = crate::utils::process::command(&staged_server)
        .arg("--version")
        .output()
        .map_err(|e| format!("Downloaded llama-server cannot start: {}", e))?;
    if !probe.status.success() {
        return Err(format!(
            "Downloaded llama-server failed validation: {}",
            String::from_utf8_lossy(&probe.stderr)
        ));
    }
    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
        return Err("Download cancelled".to_string());
    }

    let relative_server = staged_server
        .strip_prefix(&bin_dir)
        .map_err(|e| e.to_string())?;
    replace_llama_install(&bin_dir, &bin_final)?;

    let _ = std::fs::remove_dir_all(&temp_dir);
    let install_path = bin_final
        .join(relative_server)
        .to_string_lossy()
        .to_string();
    log::info!("[LlamaDownloader] Installation complete: {}", install_path);
    Ok(install_path)
}

#[cfg(test)]
mod engine_install_tests {
    use super::*;

    #[test]
    fn binary_discovery_skips_semantic_and_incomplete_releases() {
        let releases = serde_json::json!([
            {"tag_name": "v0.6.0", "assets": [{"name": "nightly-tag.txt"}]},
            {"tag_name": "b11554", "assets": [{"name": "llama-b11554-bin-macos-arm64.tar.gz"}]},
            {"tag_name": "b11553", "prerelease": true, "assets": [
                {"name": "llama-b11553-bin-win-cpu-x64.zip"},
                {"name": "llama-b11553-bin-win-cuda-12.4-x64.zip"},
                {"name": "llama-b11553-bin-win-cuda-13.4-x64.zip"}
            ]}
        ]);
        let info =
            latest_llama_binary_release(releases.as_array().unwrap(), "windows", "x86_64").unwrap();
        assert_eq!(info.version, "b11553");
        assert_eq!(info.cuda_version.as_deref(), Some("13.4"));
        // Positive control: the earlier ARM64 release is valid on its own platform.
        assert_eq!(
            latest_llama_binary_release(releases.as_array().unwrap(), "macos", "aarch64")
                .unwrap()
                .version,
            "b11554"
        );
        assert!(latest_llama_binary_release(&[], "windows", "x86_64").is_none());
    }

    #[test]
    fn cuda_picker_matches_architecture_and_rejects_malformed_assets() {
        assert_eq!(
            parse_win_cuda_asset("llama-b11553-bin-win-cuda-13.4-arm64.zip", "aarch64"),
            Some(("b11553".into(), "13.4".into()))
        );
        assert!(
            parse_win_cuda_asset("llama-b11553-bin-win-cuda-13.4-arm64.zip", "x86_64").is_none()
        );
        assert!(parse_win_cuda_asset("llama-b11553-bin-win-cuda-.4-x64.zip", "x86_64").is_none());
        assert!(
            parse_win_cuda_asset("llama-b11553-bin-win-cuda-13.4.1-x64.zip", "x86_64").is_none()
        );
        assert!(parse_win_cuda_asset("cudart-llama-bin-win-cuda-13.4-x64.zip", "x86_64").is_none());
    }

    #[test]
    fn native_binary_selection_matches_current_release_assets() {
        let platforms = [
            (
                "windows",
                "x86_64",
                false,
                false,
                "llama-b11553-bin-win-cpu-x64.zip",
            ),
            (
                "windows",
                "aarch64",
                false,
                false,
                "llama-b11553-bin-win-cpu-arm64.zip",
            ),
            (
                "macos",
                "x86_64",
                false,
                false,
                "llama-b11553-bin-macos-x64.tar.gz",
            ),
            (
                "macos",
                "aarch64",
                false,
                false,
                "llama-b11553-bin-macos-arm64.tar.gz",
            ),
            (
                "linux",
                "x86_64",
                false,
                false,
                "llama-b11553-bin-ubuntu-x64.tar.gz",
            ),
            (
                "linux",
                "x86_64",
                true,
                false,
                "llama-b11553-bin-ubuntu-vulkan-x64.tar.gz",
            ),
            (
                "linux",
                "x86_64",
                false,
                true,
                "llama-b11553-bin-ubuntu-vulkan-x64.tar.gz",
            ),
            (
                "linux",
                "aarch64",
                false,
                false,
                "llama-b11553-bin-ubuntu-arm64.tar.gz",
            ),
        ];
        for (os, arch, nvidia, amd, expected) in platforms {
            assert_eq!(
                get_llama_platform_files(os, arch, nvidia, amd, "b11553", "13.4").unwrap(),
                vec![expected]
            );
        }
        assert_eq!(
            get_llama_platform_files("windows", "aarch64", true, false, "b11553", "13.4").unwrap(),
            vec![
                "llama-b11553-bin-win-cuda-13.4-arm64.zip",
                "cudart-llama-bin-win-cuda-13.4-arm64.zip"
            ]
        );
        assert!(
            get_llama_platform_files("windows", "arm", false, false, "b11553", "13.4").is_err()
        );
        assert!(
            get_llama_platform_files("android", "aarch64", false, false, "b11553", "13.4").is_err()
        );
    }

    fn test_directory() -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("echobird-engine-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn failed_mirror_html_or_truncation_is_rejected_before_extraction() {
        let directory = test_directory();
        let zip = directory.join("engine.zip");
        let gzip = directory.join("engine.tar.gz");
        std::fs::write(&zip, b"PK\x03\x04fixture").unwrap();
        std::fs::write(&gzip, [0x1f, 0x8b, 0x08, 0x00]).unwrap();
        assert!(validate_engine_archive(&zip).is_ok());
        assert!(validate_engine_archive(&gzip).is_ok());
        std::fs::write(&zip, b"<html>Proxy error</html>").unwrap();
        assert!(validate_engine_archive(&zip).is_err());
        std::fs::write(&gzip, b"PK\x03\x04fixture").unwrap();
        assert!(validate_engine_archive(&gzip).is_err());
        std::fs::write(&zip, b"PK").unwrap();
        assert!(validate_engine_archive(&zip).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn staged_archive_requires_a_nonempty_server_and_finds_nested_bundles() {
        let directory = test_directory();
        let bundle = directory.join("llama-b11553-bin-test").join("build");
        std::fs::create_dir_all(&bundle).unwrap();
        let server = bundle.join(if cfg!(windows) {
            "llama-server.exe"
        } else {
            "llama-server"
        });
        assert!(find_staged_llama_server(&directory).is_none());
        std::fs::write(&server, []).unwrap();
        assert!(find_staged_llama_server(&directory).is_none());
        std::fs::write(&server, b"server fixture").unwrap();
        assert_eq!(find_staged_llama_server(&directory), Some(server));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_install_restores_existing_bundle_and_success_replaces_it() {
        let directory = test_directory();
        let installed = directory.join("bin");
        let staging = directory.join("bin.new");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(installed.join("version"), "old").unwrap();
        // Missing staging forces the second rename to fail after backup.
        assert!(replace_llama_install(&staging, &installed).is_err());
        assert_eq!(
            std::fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        assert!(!directory.join("bin.old").exists());
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("version"), "new").unwrap();
        replace_llama_install(&staging, &installed).unwrap();
        assert_eq!(
            std::fs::read_to_string(installed.join("version")).unwrap(),
            "new"
        );
        assert!(!staging.exists());
        assert!(!directory.join("bin.old").exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn interrupted_backup_is_recovered_when_next_install_fails() {
        let directory = test_directory();
        let backup = directory.join("bin.old");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("version"), "old").unwrap();
        let installed = directory.join("bin");
        assert!(replace_llama_install(&directory.join("bin.new"), &installed).is_err());
        assert_eq!(
            std::fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn explicit_install_recovers_backup_before_any_network_request() {
        let directory = test_directory();
        let backup = directory.join("bin.old");
        let installed = directory.join("bin");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("version"), "old").unwrap();
        recover_interrupted_llama_install(&installed).unwrap();
        assert_eq!(
            std::fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        assert!(!backup.exists());
        // Positive control: a current install is never overwritten by a backup.
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("version"), "stale").unwrap();
        recover_interrupted_llama_install(&installed).unwrap();
        assert_eq!(
            std::fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        assert!(backup.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}

fn find_staged_llama_server(bin_dir: &std::path::Path) -> Option<PathBuf> {
    let exe_name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let mut directories = vec![bin_dir.to_path_buf()];
    // Match the depth used by LocalLlmServer::find_llama_server.
    for _ in 0..3 {
        let mut children = Vec::new();
        for directory in directories {
            let candidate = directory.join(exe_name);
            if candidate.is_file() && std::fs::metadata(&candidate).ok()?.len() > 0 {
                return Some(candidate);
            }
            if let Ok(entries) = std::fs::read_dir(directory) {
                children.extend(
                    entries
                        .flatten()
                        .filter(|e| e.path().is_dir())
                        .map(|e| e.path()),
                );
            }
        }
        directories = children;
    }
    None
}

fn recover_interrupted_llama_install(installed: &std::path::Path) -> Result<(), String> {
    let backup = installed.with_file_name("bin.old");
    if backup.exists() && !installed.exists() {
        std::fs::rename(&backup, installed)
            .map_err(|e| format!("Restore interrupted install failed: {}", e))?;
    }
    Ok(())
}

fn replace_llama_install(
    staging: &std::path::Path,
    installed: &std::path::Path,
) -> Result<(), String> {
    recover_interrupted_llama_install(installed)?;
    let backup = installed.with_file_name("bin.old");
    if backup.exists() {
        std::fs::remove_dir_all(&backup)
            .map_err(|e| format!("Remove old install backup failed: {}", e))?;
    }
    let had_install = installed.exists();
    if had_install {
        std::fs::rename(installed, &backup)
            .map_err(|e| format!("Back up existing install failed: {}", e))?;
    }
    if let Err(error) = std::fs::rename(staging, installed) {
        if had_install {
            std::fs::rename(&backup, installed).map_err(|rollback| {
                format!(
                    "Install failed: {}; previous install remains at {} (restore failed: {})",
                    error,
                    backup.display(),
                    rollback
                )
            })?;
        }
        return Err(format!("Failed to finalize install: {}", error));
    }
    if had_install {
        let _ = std::fs::remove_dir_all(backup);
    }
    Ok(())
}

async fn download_engine_file(
    app_handle: &tauri::AppHandle,
    url: &str,
    dest: &PathBuf,
    completed_files: u32,
    total_files: u32,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|e| e.to_string())?;

    let ignore_pause = AtomicBool::new(false);
    let request = client.get(url).header("User-Agent", "Echobird/1.1").send();
    let resp = tokio::select! {
        result = request => result.map_err(|e| e.to_string())?,
        reason = wait_for_download_stop(&DOWNLOAD_ABORT, &ignore_pause) => return Err(reason),
    };

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }

    let content_length = resp.content_length().unwrap_or(0);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dest)
        .map_err(|e| format!("File open error: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut stream = resp.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => match chunk { Some(chunk) => chunk, None => break },
            reason = wait_for_download_stop(&DOWNLOAD_ABORT, &ignore_pause) => return Err(reason),
        };
        if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
            return Err("Download cancelled".to_string());
        }
        let data = chunk.map_err(|e| format!("Stream error: {}", e))?;
        file.write_all(&data)
            .map_err(|e| format!("Write error: {}", e))?;
        downloaded += data.len() as u64;

        if last_emit.elapsed() >= std::time::Duration::from_millis(250) {
            let file_progress = if content_length > 0 {
                (downloaded as f64 / content_length as f64) * 100.0
            } else {
                0.0
            };
            let overall = ((completed_files as f64 + file_progress / 100.0) / total_files as f64
                * 100.0) as u32;
            emit_dl_progress(
                app_handle,
                "llama-server",
                overall,
                downloaded,
                content_length,
                "downloading",
                None,
            );
            last_emit = std::time::Instant::now();
        }
    }

    if DOWNLOAD_ABORT.load(Ordering::SeqCst) {
        return Err("Download cancelled".to_string());
    }
    if downloaded == 0 || (content_length > 0 && downloaded != content_length) {
        return Err(format!(
            "Incomplete engine download: {} of {} bytes",
            downloaded, content_length
        ));
    }
    drop(file);
    validate_engine_archive(dest)?;

    Ok(())
}

fn validate_engine_archive(path: &std::path::Path) -> Result<(), String> {
    use std::io::Read;
    let mut header = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|e| format!("Invalid engine archive: {}", e))?;
    let is_zip = path.extension().is_some_and(|extension| extension == "zip");
    if (is_zip && header != *b"PK\x03\x04") || (!is_zip && header[..2] != [0x1f, 0x8b]) {
        return Err("Downloaded engine file is not the expected ZIP/gzip archive".to_string());
    }
    Ok(())
}

// ─── Engine status detection ───

#[allow(dead_code)]
fn check_python_package(package: &str) -> Option<String> {
    let result = crate::utils::process::command("python3")
        .args(["-m", "pip", "show", package])
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
    if !bin_dir.exists() {
        return vec![];
    }
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
    get_installed_llama_binary_names()
        .into_iter()
        .find(|n| n.starts_with("llama-b"))
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

/// Get installation status for the specified runtime only (lazy — avoids unnecessary Python calls).
/// Pass `runtime_filter = None` to check all engines (legacy / admin use).
///
/// Page-mount entry point: also refreshes the engine version map from upstream
/// (GitHub for llama.cpp, PyPI for vllm/sglang) before answering. Without this
/// the "升级引擎" button never appears on first open — the cache is empty so
/// `get_engine_versions()` returns the fallback constant, which always matches
/// the installed binary, hiding any actual upstream update.
pub async fn get_local_engine_status(runtime_filter: Option<&str>) -> serde_json::Value {
    // Best-effort refresh; on network failure refresh_engine_versions falls
    // back to the cached/fallback map so the UI still loads.
    let versions = refresh_engine_versions().await;

    // ── llama-server (always cheap — binary lookup, no pip) ──────────────────
    let check_llama = runtime_filter.map(|r| r == "llama-server").unwrap_or(true);
    let llama_installed = if check_llama {
        LocalLlmServer::find_llama_server().is_some()
    } else {
        false
    };
    let installed_ver = if check_llama {
        get_installed_llama_version().unwrap_or_default()
    } else {
        String::new()
    };
    let binary_names = if check_llama {
        get_installed_llama_binary_names()
    } else {
        vec![]
    };
    let latest_llama = versions
        .get("llama-server")
        .map(|i| i.version.as_str())
        .unwrap_or(FALLBACK_LLAMA_VERSION);

    // ── vllm / sglang — Linux-only + only when explicitly selected ───────────
    let _check_vllm = runtime_filter.map(|r| r == "vllm").unwrap_or(true);
    let _check_sglang = runtime_filter.map(|r| r == "sglang").unwrap_or(true);

    #[cfg(target_os = "linux")]
    let vllm_version = if _check_vllm {
        check_python_package("vllm")
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let vllm_version: Option<String> = None;

    #[cfg(target_os = "linux")]
    let sglang_version = if _check_sglang {
        check_python_package("sglang")
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let sglang_version: Option<String> = None;

    let latest_vllm = versions.get("vllm").map(|i| i.version.clone());
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
/// - vllm / sglang: python3 -m pip install (same interpreter as server launch)
pub async fn install_local_engine(
    app_handle: tauri::AppHandle,
    runtime: String,
    override_version: Option<String>,
    override_cuda: Option<String>,
) -> Result<(), String> {
    match runtime.as_str() {
        "llama-server" => {
            // Atomic upgrade: don't pre-delete bin/. download_llama_server
            // stages the new install under bin.new/ and only swaps in on
            // successful extract, so a cancelled or failed download leaves
            // the user's existing bin/ untouched and runnable.
            download_llama_server(app_handle, override_version, override_cuda)
                .await
                .map(|_| ())
        }
        "vllm" => {
            #[cfg(target_os = "linux")]
            {
                install_pip_engine(&app_handle, "vllm", &runtime).await
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err("vllm is only supported on Linux".to_string())
            }
        }
        "sglang" => {
            #[cfg(target_os = "linux")]
            {
                install_pip_engine(&app_handle, "sglang", &runtime).await
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err("sglang is only supported on Linux".to_string())
            }
        }
        other => Err(format!("Unknown runtime: {}", other)),
    }
}

/// Install into the same Python environment used to launch the server.
#[allow(dead_code)]
async fn install_pip_engine(
    app_handle: &tauri::AppHandle,
    package: &str,
    runtime: &str,
) -> Result<(), String> {
    let job = acquire_download_job()?;
    let runtime = runtime.to_string();
    let package = package.to_string();
    let app = app_handle.clone();

    // Emit: installing started
    emit_dl_progress(&app, &runtime, 5, 0, 0, "installing", None);

    log::info!(
        "[EngineInstaller] python3 -m pip install {} for runtime '{}'",
        package,
        runtime
    );

    // Pip install with PyPI mirrors (prefer China mirrors for faster access)
    let result = tokio::task::spawn_blocking({
        let package = package.clone();
        let runtime = runtime.clone();
        let app = app.clone();
        move || {
            let mut last_error = String::new();
            for (index, source) in [
                "https://pypi.tuna.tsinghua.edu.cn/simple",
                "https://pypi.org/simple",
            ]
            .iter()
            .enumerate()
            {
                emit_dl_progress(
                    &app,
                    &runtime,
                    if index == 0 { 30 } else { 50 },
                    0,
                    0,
                    "installing",
                    None,
                );
                // output() drains both pipes; reading only stderr can deadlock
                // once pip's normal stdout progress fills its pipe buffer.
                let output = crate::utils::process::command("python3")
                    .args([
                        "-m",
                        "pip",
                        "install",
                        &package,
                        "--upgrade",
                        "--index-url",
                        source,
                    ])
                    .output()
                    .map_err(|e| format!("python3 -m pip failed to start: {}", e))?;
                log::info!("[pip] {}", String::from_utf8_lossy(&output.stdout));
                if output.status.success() {
                    return Ok(());
                }
                last_error = String::from_utf8_lossy(&output.stderr).to_string();
                log::warn!("[EngineInstaller] {} failed: {}", source, last_error);
            }
            Err(format!("python3 -m pip install failed:\n{}", last_error))
        }
    })
    .await
    .unwrap_or_else(|e| Err(format!("Task join error: {}", e)));

    drop(job);
    match result {
        Ok(()) => {
            log::info!("[EngineInstaller] {} installed successfully", package);
            emit_dl_progress(&app, &runtime, 100, 0, 0, "completed", None);
            Ok(())
        }
        Err(e) => {
            log::error!("[EngineInstaller] {} install failed: {}", package, e);
            emit_dl_progress(&app, &runtime, 0, 0, 0, "error", None);
            Err(e)
        }
    }
}
