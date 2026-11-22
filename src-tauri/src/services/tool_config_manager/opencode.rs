//! Shared model configuration for OpenCode CLI and Desktop.

use super::{
    echobird_dir, read_json_file, read_jsonc_file, write_json_file, ApplyResult, ModelInfo,
};
use crate::services::tool_manager;
use std::fs;
use std::path::{Path, PathBuf};

// ════════════════════════════════════════════════════════════════
//  Type 3b: OpenCode
//  ~/.config/opencode/opencode.json  {provider: {X: {npm, options, models}}}
// ════════════════════════════════════════════════════════════════

pub(super) fn apply_opencode(model_info: &ModelInfo) -> ApplyResult {
    // Write echobird relay JSON — the patched launcher reads this
    let config_path = echobird_dir().join("opencode.json");
    let model_id = model_info
        .model
        .as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");

    if model_id.is_empty() {
        return ApplyResult {
            success: false,
            message: "Model ID is empty, cannot apply config".to_string(),
        };
    }

    let (npm, base_url) = match opencode_endpoint(model_info) {
        Ok(endpoint) => endpoint,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    let provider_name = model_id; // Use model ID instead of domain name

    let config = serde_json::json!({
        "apiKey": model_info.api_key.as_deref().unwrap_or(""),
        "baseUrl": base_url,
        "modelId": model_id,
        "modelName": model_info.name.as_deref().unwrap_or(model_id),
        "providerName": provider_name,
        "npm": npm,
    });

    if let Err(e) =
        write_opencode_native_config(model_info, model_id, npm, &base_url, provider_name)
    {
        return ApplyResult {
            success: false,
            message: e,
        };
    }

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!(
                "[ToolConfigManager] OpenCode config written to {:?}",
                config_path
            );
            crate::services::tool_patcher::patch_opencode();
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for OpenCode. Use /models in TUI to select echobird/{}.",
                    model_info.name.as_deref().unwrap_or(model_id), model_id
                ),
            }
        }
        Err(e) => ApplyResult {
            success: false,
            message: e,
        },
    }
}

const OPENAI_NPM: &str = "@ai-sdk/openai-compatible";
const ANTHROPIC_NPM: &str = "@ai-sdk/anthropic";

/// Picks the AI SDK package and base URL for the selected protocol.
///
/// Model Nexus stores Anthropic endpoints as the root that clients append
/// `/v1/messages` to. OpenCode's Anthropic provider appends only `/messages`,
/// so the root gains `/v1`; a stored `/v1` or full `/v1/messages` endpoint is
/// accepted without doubling the version segment.
fn opencode_endpoint(model_info: &ModelInfo) -> Result<(&'static str, String), String> {
    if model_info.protocol.as_deref() != Some("anthropic") {
        let base_url = model_info
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_string();
        return Ok((OPENAI_NPM, base_url));
    }
    let root = model_info
        .base_url
        .as_deref()
        .or(model_info.anthropic_url.as_deref())
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or("Anthropic URL is empty. Pick a model first.")?
        .trim_end_matches('/');
    let root = root
        .strip_suffix("/v1/messages")
        .or_else(|| root.strip_suffix("/v1"))
        .unwrap_or(root)
        .trim_end_matches('/');
    Ok((ANTHROPIC_NPM, format!("{root}/v1")))
}

fn write_opencode_native_config(
    model_info: &ModelInfo,
    model_id: &str,
    npm: &str,
    base_url: &str,
    provider_name: &str,
) -> Result<(), String> {
    let config_path = opencode_config_dir().join("opencode.jsonc");

    let mut config = read_jsonc_file(&config_path)
        .or_else(|| read_json_file(&config_path.with_extension("json")))
        .unwrap_or(serde_json::json!({}));

    if config.get("$schema").is_none() {
        config["$schema"] = serde_json::json!("https://opencode.ai/config.json");
    }
    if !config
        .get("provider")
        .map(|v| v.is_object())
        .unwrap_or(false)
    {
        config["provider"] = serde_json::json!({});
    }

    let provider_id = "echobird";
    super::provider_selection::enable_provider(&mut config, provider_id);
    super::provider_selection::allow_provider_policy(&mut config, provider_id);
    config["provider"][provider_id] = serde_json::json!({
        "npm": npm,
        "name": provider_name,
        "options": {
            "baseURL": base_url,
            "apiKey": model_info.api_key.as_deref().unwrap_or("")
        },
        "models": {
            model_id: {
                "name": model_info.name.as_deref().unwrap_or(model_id)
            }
        }
    });
    super::switching::model_map(model_info, &mut config["provider"][provider_id]["models"])?;
    config["model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));
    config["small_model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));

    write_json_file(&config_path, &config)
}

pub(super) fn read_opencode() -> Option<ModelInfo> {
    let native_path = opencode_config_dir().join("opencode.jsonc");
    if let Some(info) = read_opencode_native_config(&native_path)
        .or_else(|| read_opencode_native_config(&native_path.with_extension("json")))
    {
        return Some(info);
    }

    // Read from echobird relay JSON
    let config_path = echobird_dir().join("opencode.json");
    let config = read_json_file(&config_path)?;
    let model_id = config.get("modelId")?.as_str()?.to_string();
    if model_id.is_empty() {
        return None;
    }

    Some(ModelInfo {
        name: config
            .get("modelName")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model: Some(model_id),
        base_url: config
            .get("baseUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        api_key: config
            .get("apiKey")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        anthropic_url: None,
        protocol: None,
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

pub(super) fn read_opencode_native_config(path: &Path) -> Option<ModelInfo> {
    let config = if path.extension().and_then(|e| e.to_str()) == Some("jsonc") {
        read_jsonc_file(path)?
    } else {
        read_json_file(path)?
    };
    let selected = config.get("model")?.as_str()?;
    let (provider_id, model_id) = selected.split_once('/')?;
    let provider = config.pointer(&format!("/provider/{}", provider_id))?;
    let anthropic = provider.get("npm").and_then(|v| v.as_str()) == Some(ANTHROPIC_NPM);
    let mut base_url = provider
        .pointer("/options/baseURL")
        .and_then(|v| v.as_str())
        .map(|s| s.trim_end_matches('/').to_string());
    if anthropic {
        // Report the stored root form so it matches the Model Nexus URL.
        base_url = base_url.map(|url| url.strip_suffix("/v1").unwrap_or(&url).to_string());
    }

    Some(ModelInfo {
        name: provider
            .pointer(&format!("/models/{}/name", model_id))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model: Some(model_id.to_string()),
        anthropic_url: base_url.clone().filter(|_| anthropic),
        base_url,
        api_key: provider
            .pointer("/options/apiKey")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        protocol: Some(if anthropic { "anthropic" } else { "openai" }.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

pub(super) fn restore_opencode_to_official() -> ApplyResult {
    let relay_path = echobird_dir().join("opencode.json");
    if relay_path.exists() {
        let _ = fs::remove_file(&relay_path);
    }

    let native_path = opencode_config_dir().join("opencode.jsonc");

    if !native_path.exists() {
        return ApplyResult {
            success: true,
            message: "OpenCode already at defaults - no config file to update.".to_string(),
        };
    }

    let mut updated_any = false;
    for path in [&native_path, &native_path.with_extension("json")] {
        if !path.exists() {
            continue;
        }

        let mut config = match read_jsonc_file(path) {
            Some(c) => c,
            None => {
                return ApplyResult {
                    success: false,
                    message: format!("Failed to parse OpenCode config: {}", path.display()),
                }
            }
        };

        if let Some(provider) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
            provider.remove("echobird");
        }
        if config
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.starts_with("echobird/"))
            .unwrap_or(false)
        {
            tool_manager::delete_nested_value(&mut config, "model");
        }
        if config
            .get("small_model")
            .and_then(|v| v.as_str())
            .map(|s| s.starts_with("echobird/"))
            .unwrap_or(false)
        {
            tool_manager::delete_nested_value(&mut config, "small_model");
        }

        if let Err(e) = write_json_file(path, &config) {
            return ApplyResult {
                success: false,
                message: e,
            };
        }
        updated_any = true;
    }

    if updated_any {
        ApplyResult {
            success: true,
            message: "OpenCode restored - Echobird provider removed.".to_string(),
        }
    } else {
        ApplyResult {
            success: true,
            message: "OpenCode already at defaults - no config file to update.".to_string(),
        }
    }
}

pub(in crate::services) fn opencode_config_dir() -> PathBuf {
    opencode_config_dir_from(
        dirs::home_dir(),
        std::env::var_os("OPENCODE_CONFIG_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    )
}

fn opencode_config_dir_from(
    home: Option<PathBuf>,
    opencode_config_dir: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
) -> PathBuf {
    opencode_config_dir
        .or_else(|| xdg_config_home.map(|path| path.join("opencode")))
        .unwrap_or_else(|| home.unwrap_or_default().join(".config").join("opencode"))
}

#[cfg(test)]
mod tests {
    use super::{
        opencode_config_dir_from, opencode_endpoint, read_opencode_native_config, ANTHROPIC_NPM,
        OPENAI_NPM,
    };
    use crate::services::tool_config_manager::ModelInfo;
    use std::path::PathBuf;

    fn info(protocol: &str, url: &str) -> ModelInfo {
        serde_json::from_value(serde_json::json!({
            "model": "fixture-model", "baseUrl": url, "protocol": protocol
        }))
        .unwrap()
    }

    #[test]
    fn anthropic_roots_gain_one_version_segment() {
        for url in [
            "https://api.example.test/anthropic",
            "https://api.example.test/anthropic/",
            "https://api.example.test/anthropic/v1",
            "https://api.example.test/anthropic/v1/messages/",
        ] {
            assert_eq!(
                opencode_endpoint(&info("anthropic", url)).unwrap(),
                (
                    ANTHROPIC_NPM,
                    "https://api.example.test/anthropic/v1".into()
                )
            );
        }
        assert!(opencode_endpoint(&info("anthropic", " ")).is_err());
    }

    #[test]
    fn openai_endpoint_is_unchanged() {
        assert_eq!(
            opencode_endpoint(&info("openai", "https://api.example.test/v1/")).unwrap(),
            (OPENAI_NPM, "https://api.example.test/v1".into())
        );
    }

    #[test]
    fn reads_anthropic_provider_back_as_its_root() {
        let dir = std::env::temp_dir().join(format!("eb-opencode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("opencode.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "model": "echobird/fixture-model",
                "provider": {"echobird": {
                    "npm": ANTHROPIC_NPM,
                    "options": {"baseURL": "https://api.example.test/anthropic/v1", "apiKey": "test-only"},
                    "models": {"fixture-model": {"name": "Fixture"}}
                }}
            })
            .to_string(),
        )
        .unwrap();
        let read = read_opencode_native_config(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(read.protocol.as_deref(), Some("anthropic"));
        assert_eq!(
            read.base_url.as_deref(),
            Some("https://api.example.test/anthropic")
        );
        assert_eq!(read.anthropic_url, read.base_url);
    }

    #[test]
    fn config_dir_follows_opencode_v2_precedence() {
        let home = Some(PathBuf::from("home"));
        let xdg = Some(PathBuf::from("xdg"));

        assert_eq!(
            opencode_config_dir_from(home.clone(), Some(PathBuf::from("custom")), xdg.clone()),
            PathBuf::from("custom")
        );
        assert_eq!(
            opencode_config_dir_from(home.clone(), None, xdg),
            PathBuf::from("xdg").join("opencode")
        );
        assert_eq!(
            opencode_config_dir_from(home, None, None),
            PathBuf::from("home").join(".config").join("opencode")
        );
    }
}
