// Filesystem helper utilities

use std::path::Path;

/// Check if a path exists
pub fn path_exists(p: &str) -> bool {
    Path::new(p).exists()
}

/// Ensure a directory exists, creating it recursively if needed
pub async fn ensure_dir(dir_path: &str) -> Result<(), String> {
    tokio::fs::create_dir_all(dir_path)
        .await
        .map_err(|e| format!("Failed to create directory {}: {}", dir_path, e))
}

/// Read a JSON file and deserialize into type T
pub fn read_json_file<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path, e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse JSON {}: {}", path, e))
}

/// Write a value as JSON to a file
pub fn write_json_file<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    // Ensure parent directory exists
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create parent dir: {}", e))?;
    }
    std::fs::write(path, content)
        .map_err(|e| format!("Failed to write {}: {}", path, e))
}
