//! One-time migration support for configurations written by the removed
//! Codex Responses-to-Chat proxy.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const LEGACY_PROXY_URL_FRAGMENT: &str = "127.0.0.1:53682";
const CONFIG_FILENAME: &str = "config.toml";
const LEGACY_RELAY_FILENAME: &str = "codex.json";

pub fn default_codex_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CODEX_HOME") {
        let path = path.trim().trim_matches('"').trim_matches('\'').trim();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    if let Ok(path) = std::env::var("ECHOBIRD_CODEX_CONFIG_DIR") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    dirs::home_dir().map(|home| home.join(".codex"))
}

fn legacy_relay_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("ECHOBIRD_RELAY_DIR") {
        if !path.is_empty() {
            return Some(PathBuf::from(path).join(LEGACY_RELAY_FILENAME));
        }
    }
    dirs::home_dir().map(|home| home.join(".echobird").join(LEGACY_RELAY_FILENAME))
}

/// Convert the last proxy-backed config to a direct Responses connection.
/// Returns `true` only when a legacy config was rewritten.
pub fn migrate_legacy_proxy_config(codex_dir: &Path) -> io::Result<bool> {
    let relay_path = legacy_relay_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "legacy Codex relay path unavailable",
        )
    })?;
    migrate_legacy_proxy_config_from(codex_dir, &relay_path)
}

fn migrate_legacy_proxy_config_from(codex_dir: &Path, relay_path: &Path) -> io::Result<bool> {
    let config_path = codex_dir.join(CONFIG_FILENAME);
    match fs::read_to_string(&config_path) {
        Ok(content) if content.contains(LEGACY_PROXY_URL_FRAGMENT) => {}
        Ok(_) | Err(_) => return Ok(false),
    }

    let relay: serde_json::Value = serde_json::from_str(&fs::read_to_string(relay_path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    let base_url = relay
        .get("baseUrl")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "legacy baseUrl is missing"))?;
    let model = relay
        .get("actualModel")
        .or_else(|| relay.get("modelName"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "legacy model is missing"))?;
    let model_info = crate::services::tool_config_manager::ModelInfo {
        name: relay
            .get("modelName")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        model: Some(model.to_string()),
        base_url: Some(base_url.to_string()),
        api_key: relay
            .get("apiKey")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        anthropic_url: None,
        protocol: Some("openai".to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
    };
    let state_dir = relay_path.parent().unwrap_or_else(|| Path::new(""));
    let result = crate::services::tool_config_manager::apply_codex_at(
        "codex",
        &model_info,
        codex_dir,
        state_dir,
    );
    if result.success {
        Ok(true)
    } else {
        Err(io::Error::other(result.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmpdir() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "echobird_codex_migration_{}_{}",
            std::process::id(),
            id
        ));
        fs::create_dir_all(&dir).expect("create temp directory");
        dir
    }

    #[test]
    fn direct_config_is_untouched() {
        let dir = unique_tmpdir();
        let relay = dir.join(LEGACY_RELAY_FILENAME);
        fs::write(
            dir.join(CONFIG_FILENAME),
            "model = \"glm-5.2\"\nbase_url = \"https://example.com/v1\"\n",
        )
        .unwrap();

        assert!(!migrate_legacy_proxy_config_from(&dir, &relay).unwrap());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn proxy_config_is_migrated_to_direct_responses() {
        let dir = unique_tmpdir();
        let relay = dir.join(LEGACY_RELAY_FILENAME);
        fs::write(
            dir.join(CONFIG_FILENAME),
            "model_provider = \"OpenAI\"\nmodel = \"gpt-5.5\"\n\n[model_providers.OpenAI]\nbase_url = \"http://127.0.0.1:53682/v1\"\n",
        )
        .unwrap();
        fs::write(
            &relay,
            serde_json::json!({
                "apiKey": "test-key",
                "baseUrl": "https://provider.example/v1",
                "actualModel": "provider-model",
                "modelName": "Provider Model"
            })
            .to_string(),
        )
        .unwrap();

        assert!(migrate_legacy_proxy_config_from(&dir, &relay).unwrap());
        let config = fs::read_to_string(dir.join(CONFIG_FILENAME)).unwrap();
        assert!(config.contains("model = \"provider-model\""));
        assert!(config.contains("base_url = \"https://provider.example/v1\""));
        assert!(config.contains("wire_api = \"responses\""));
        assert!(config.contains("web_search = \"live\""));
        assert!(!config.contains(LEGACY_PROXY_URL_FRAGMENT));
        assert!(!relay.exists());
        fs::remove_dir_all(dir).ok();
    }
}
