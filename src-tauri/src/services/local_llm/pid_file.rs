// PID file lifecycle for llama-server (and other local LLM runtimes).
//
// Mirrors the Codex launcher's PID file scheme — written when the server
// starts, deleted on graceful stop, read by Tauri's startup/exit
// cleanup so we kill ONLY our own llama-server process. Replaces the
// previous `taskkill /IM llama-server.exe` / `pkill -f llama-server`
// approach which would also kill any llama-server the user is running
// independently (e.g. for an unrelated project, or hosted somewhere
// EchoBird doesn't manage).
//
// File location: ~/.echobird/llama-server.pid
// Format:        { "pid": 12345, "runtime": "llama-server", "startedAt": "2026-..." }

use std::fs;
use std::path::PathBuf;

pub fn pid_file_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".echobird").join("llama-server.pid"))
}

/// Write the PID file atomically (tmp + rename). Returns true on success.
pub fn write_pid_file(pid: u32, runtime: &str) -> bool {
    let Some(path) = pid_file_path() else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    if fs::create_dir_all(parent).is_err() {
        return false;
    }
    let payload = serde_json::json!({
        "pid": pid,
        "runtime": runtime,
        "startedAt": chrono::Utc::now().to_rfc3339(),
    });
    let tmp = path.with_extension("pid.tmp");
    if fs::write(&tmp, payload.to_string()).is_err() {
        return false;
    }
    fs::rename(&tmp, &path).is_ok()
}

/// Read the recorded PID. Returns None when the file is missing or
/// malformed — caller treats either as "no stale process to kill".
pub fn read_pid_file() -> Option<u32> {
    let path = pid_file_path()?;
    let content = fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("pid")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .filter(|p| *p > 0)
}

/// Idempotent — succeeds whether or not the file existed.
pub fn delete_pid_file() {
    if let Some(path) = pid_file_path() {
        let _ = fs::remove_file(path);
    }
}
