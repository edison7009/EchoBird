// channel_history — Persist Channels page chat messages to
// ~/.echobird/config/channel_history/{channel_key}.json
// Each file is a simple JSON array of {role, content} objects (max 500 entries).

use serde::{Deserialize, Serialize};

/// A single chat message stored for a channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub role: String,   // "user" | "assistant" | "system"
    pub content: String,
}

// Maximum messages to keep per channel file
const MAX_STORED: usize = 500;

fn history_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".echobird")
        .join("config")
        .join("channel_history")
}

fn history_file(channel_key: &str) -> std::path::PathBuf {
    // Sanitize key for filesystem
    let safe = channel_key
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect::<String>();
    history_dir().join(format!("{}.json", safe))
}

// ── Internal read/write ───────────────────────────────────────────────────────

fn read_all(channel_key: &str) -> Vec<ChannelMessage> {
    let path = history_file(channel_key);
    if !path.exists() { return Vec::new(); }
    match std::fs::read_to_string(&path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn write_all(channel_key: &str, messages: &[ChannelMessage]) {
    let dir = history_dir();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::error!("[ChannelHistory] create_dir_all failed: {}", e);
        return;
    }
    let path = history_file(channel_key);
    // Trim to MAX_STORED (keep newest)
    let trimmed = if messages.len() > MAX_STORED {
        &messages[messages.len() - MAX_STORED..]
    } else {
        messages
    };
    match serde_json::to_string_pretty(trimmed) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("[ChannelHistory] write failed for {}: {}", channel_key, e);
            }
        }
        Err(e) => log::error!("[ChannelHistory] serialize failed: {}", e),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Save the full message list for a channel (replaces existing file)
pub fn save_channel_history(channel_key: &str, messages: Vec<ChannelMessage>) {
    write_all(channel_key, &messages);
}

/// Load a paginated slice of messages for a channel.
/// `offset` counts from the END (0 = newest).
/// Returns messages in chronological order (oldest first within the slice).
///
/// Example: total=200, offset=0, limit=30 → messages[170..200]  (newest 30)
///          offset=30, limit=30 → messages[140..170]  (next older batch)
pub fn load_channel_history(channel_key: &str, offset: usize, limit: usize) -> Vec<ChannelMessage> {
    let all = read_all(channel_key);
    let len = all.len();
    if len == 0 || offset >= len { return Vec::new(); }
    // Compute slice from the end
    let end = len.saturating_sub(offset);
    let start = end.saturating_sub(limit);
    all[start..end].to_vec()
}

/// Return total number of stored messages (for pagination logic)
pub fn channel_history_count(channel_key: &str) -> usize {
    read_all(channel_key).len()
}

/// Delete the channel history file
pub fn clear_channel_history(channel_key: &str) {
    let path = history_file(channel_key);
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            log::error!("[ChannelHistory] remove_file failed: {}", e);
        } else {
            log::info!("[ChannelHistory] Cleared history for {}", channel_key);
        }
    }
}
