//! Model configuration for zcode.

use super::{
    model_input_modalities_for, read_json_file, read_jsonc_file, write_json_file, ApplyResult,
    ModelInfo,
};
use crate::services::tool_manager;
use std::path::PathBuf;

// ════════════════════════════════════════════════════════════════
//  ZCode — Z.AI's desktop OpenCode fork. ~/.zcode/v2/config.json.
//  OpenCode config schema, but the provider uses a `kind` discriminator
//  ("openai-compatible" | "anthropic") instead of OpenCode's `npm`, and
//  it supports BOTH protocols. The native config write is the whole
//  mechanism — desktop app, no launcher patch, no ~/.echobird relay.
//  Default model is the OpenCode-standard top-level `model` selector.
// ════════════════════════════════════════════════════════════════

fn zcode_config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".zcode")
        .join("v2")
}

fn zcode_config_path() -> PathBuf {
    zcode_config_dir().join("config.json")
}

fn zcode_personal_config_path() -> PathBuf {
    zcode_config_dir().join("provider_config.json")
}

fn prepare_zcode_personal_config(
    path: &std::path::Path,
    model_id: &str,
    api_key: &str,
    base_url: &str,
    protocol: &str,
) -> Result<Option<serde_json::Value>, String> {
    if !path.exists() {
        // ZCode imports config.json when this file is first created. Keeping
        // the legacy write below is therefore sufficient for a fresh 3.14+
        // install and remains compatible with older published clients.
        return Ok(None);
    }
    let mut root = read_json_file(path).ok_or_else(|| {
        format!(
            "Failed to parse ZCode provider config: {}. EchoBird left it unchanged.",
            path.display()
        )
    })?;
    if root.get("schemaVersion").and_then(|v| v.as_u64()) != Some(1) {
        return Err(format!(
            "Unsupported ZCode provider config version in {}. EchoBird left it unchanged.",
            path.display()
        ));
    }
    let config = root
        .get_mut("config")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| format!("Invalid ZCode provider config: {}", path.display()))?;
    let provider_rules = config
        .get_mut("providerConfigRules")
        .and_then(|v| v.get_mut("providerRules"))
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| format!("Invalid ZCode provider rules: {}", path.display()))?;
    provider_rules
        .retain(|rule| rule.get("providerId").and_then(|v| v.as_str()) != Some("echobird"));

    let api_type = if protocol == "anthropic" {
        "anthropic-messages"
    } else {
        "openai-chat-completions"
    };
    provider_rules.push(serde_json::json!({
        "providerId": "echobird",
        "providerName": "EchoBird",
        "enabled": true,
        "config": {
            "group": "standard-personal",
            "access": { "type": "api-key", "apiKey": api_key },
            "api": { "type": api_type, "baseUrl": base_url },
            "personalModelIds": [model_id],
            "modelOrder": [model_id]
        }
    }));

    let model_rules = config
        .get_mut("modelConfigRules")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| format!("Invalid ZCode model rules: {}", path.display()))?;
    for key in ["providerModelRules", "manualProviderModelRules"] {
        let rules = model_rules
            .get_mut(key)
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| format!("Invalid ZCode model rules: {}", path.display()))?;
        rules.retain(|rule| rule.get("providerId").and_then(|v| v.as_str()) != Some("echobird"));
    }
    config.insert(
        "defaultModelSelection".to_string(),
        serde_json::json!({ "providerId": "echobird", "modelId": model_id }),
    );
    if let Some(order) = config
        .get_mut("providerOrder")
        .and_then(|v| v.as_array_mut())
    {
        order.retain(|id| id.as_str() != Some("echobird"));
        order.push(serde_json::Value::String("echobird".to_string()));
    }

    Ok(Some(root))
}

pub(super) fn apply_zcode(model_info: &ModelInfo) -> ApplyResult {
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

    let base_url = model_info
        .base_url
        .as_deref()
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/')
        .to_string();

    // Local llama-server needs no real key; mirror the dummy used elsewhere.
    let is_local = base_url.contains("127.0.0.1") || base_url.contains("localhost");
    let api_key = match model_info.api_key.as_deref() {
        Some(k) if !k.is_empty() => k.to_string(),
        _ if is_local => "local-no-auth".to_string(),
        _ => {
            return ApplyResult {
                success: false,
                message: "API Key is empty, cannot apply config".to_string(),
            }
        }
    };

    // Protocol → provider `kind`. The frontend already collapsed the chosen
    // protocol's URL into base_url, so base_url is correct for either kind.
    let kind = if model_info.protocol.as_deref() == Some("anthropic") {
        "anthropic"
    } else {
        "openai-compatible"
    };

    let config_path = zcode_config_path();
    let mut config = read_jsonc_file(&config_path).unwrap_or(serde_json::json!({}));

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
    let display_name = model_info.name.as_deref().unwrap_or(model_id);
    // Resolve the real input modalities for the selected model so ZCode's
    // `modalities.input` reflects image/video support when the model has it,
    // instead of declaring every model text-only.
    let input_modalities = model_input_modalities_for(model_id);
    config["provider"][provider_id] = serde_json::json!({
        "name": display_name,
        "kind": kind,
        "options": {
            "apiKey": api_key,
            "baseURL": base_url,
            "apiKeyRequired": true
        },
        "source": "custom",
        "models": {
            model_id: {
                "name": display_name,
                "modalities": { "input": input_modalities, "output": ["text"] }
            }
        }
    });
    // OpenCode-standard active-model selectors (UI stores its pick elsewhere;
    // these set the configured default that ZCode reads on launch).
    config["model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));
    config["small_model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));

    let personal_path = zcode_personal_config_path();
    let personal = match prepare_zcode_personal_config(
        &personal_path,
        model_id,
        &api_key,
        &base_url,
        model_info.protocol.as_deref().unwrap_or("openai"),
    ) {
        Ok(config) => config,
        Err(error) => {
            return ApplyResult {
                success: false,
                message: error,
            }
        }
    };

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            if let Some(personal) = personal {
                if let Err(error) = write_json_file(&personal_path, &personal) {
                    return ApplyResult {
                        success: false,
                        message: error,
                    };
                }
            }
            log::info!(
                "[ToolConfigManager] ZCode config written to {:?}{}",
                config_path,
                if personal_path.exists() {
                    format!(" and {}", personal_path.display())
                } else {
                    String::new()
                }
            );
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for ZCode. Restart ZCode to apply.",
                    display_name
                ),
            }
        }
        Err(e) => ApplyResult {
            success: false,
            message: e,
        },
    }
}

fn read_zcode_personal_config(path: &std::path::Path) -> Option<ModelInfo> {
    let root = read_json_file(path)?;
    if root.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    let config = root.get("config")?;
    let selected = config.get("defaultModelSelection")?;
    if selected.get("providerId")?.as_str()? != "echobird" {
        return None;
    }
    let model_id = selected.get("modelId")?.as_str()?;
    let provider = config
        .pointer("/providerConfigRules/providerRules")?
        .as_array()?
        .iter()
        .find(|rule| rule.get("providerId").and_then(|v| v.as_str()) == Some("echobird"))?;
    let provider_config = provider.get("config")?;
    let api = provider_config.get("api")?;
    let protocol = if api.get("type").and_then(|v| v.as_str()) == Some("anthropic-messages") {
        "anthropic"
    } else {
        "openai"
    };
    let base_url = api
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .map(String::from);
    let api_key = provider_config
        .pointer("/access/apiKey")
        .and_then(|v| v.as_str())
        .map(String::from);

    Some(ModelInfo {
        name: Some(model_id.to_string()),
        model: Some(model_id.to_string()),
        anthropic_url: if protocol == "anthropic" {
            base_url.clone()
        } else {
            None
        },
        base_url,
        api_key,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
    })
}

pub(super) fn read_zcode() -> Option<ModelInfo> {
    let personal_path = zcode_personal_config_path();
    if personal_path.exists() {
        return read_zcode_personal_config(&personal_path);
    }
    let config = read_jsonc_file(&zcode_config_path())?;
    let selected = config.get("model")?.as_str()?;
    let (provider_id, model_id) = selected.split_once('/')?;
    let provider = config.pointer(&format!("/provider/{}", provider_id))?;
    let kind = provider
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("openai-compatible");
    let protocol = if kind == "anthropic" {
        "anthropic"
    } else {
        "openai"
    };

    Some(ModelInfo {
        name: provider
            .pointer(&format!("/models/{}/name", model_id))
            .and_then(|v| v.as_str())
            .map(String::from)
            .or_else(|| Some(model_id.to_string())),
        model: Some(model_id.to_string()),
        base_url: provider
            .pointer("/options/baseURL")
            .and_then(|v| v.as_str())
            .map(String::from),
        api_key: provider
            .pointer("/options/apiKey")
            .and_then(|v| v.as_str())
            .map(String::from),
        anthropic_url: None,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
    })
}

pub(super) fn restore_zcode_to_official() -> ApplyResult {
    restore_zcode_to_official_at(&zcode_config_dir())
}

pub(super) fn restore_zcode_to_official_at(dir: &std::path::Path) -> ApplyResult {
    let path = dir.join("config.json");
    let mut updated = false;
    if path.exists() {
        let mut config = match read_jsonc_file(&path) {
            Some(c) => c,
            None => {
                return ApplyResult {
                    success: false,
                    message: format!("Failed to parse ZCode config: {}", path.display()),
                }
            }
        };
        if let Some(provider) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
            provider.remove("echobird");
        }
        for key in ["model", "small_model"] {
            if config
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.starts_with("echobird/"))
                .unwrap_or(false)
            {
                tool_manager::delete_nested_value(&mut config, key);
            }
        }
        if let Err(error) = write_json_file(&path, &config) {
            return ApplyResult {
                success: false,
                message: error,
            };
        }
        updated = true;
    }

    let personal_path = dir.join("provider_config.json");
    if personal_path.exists() {
        let mut root = match read_json_file(&personal_path) {
            Some(config) if config.get("schemaVersion").and_then(|v| v.as_u64()) == Some(1) => {
                config
            }
            _ => {
                return ApplyResult {
                    success: false,
                    message: format!(
                        "Failed to parse supported ZCode provider config: {}",
                        personal_path.display()
                    ),
                }
            }
        };
        let Some(config) = root.get_mut("config").and_then(|v| v.as_object_mut()) else {
            return ApplyResult {
                success: false,
                message: format!("Invalid ZCode provider config: {}", personal_path.display()),
            };
        };
        if let Some(rules) = config
            .get_mut("providerConfigRules")
            .and_then(|v| v.get_mut("providerRules"))
            .and_then(|v| v.as_array_mut())
        {
            rules
                .retain(|rule| rule.get("providerId").and_then(|v| v.as_str()) != Some("echobird"));
        }
        if let Some(model_rules) = config
            .get_mut("modelConfigRules")
            .and_then(|v| v.as_object_mut())
        {
            for key in ["providerModelRules", "manualProviderModelRules"] {
                if let Some(rules) = model_rules.get_mut(key).and_then(|v| v.as_array_mut()) {
                    rules.retain(|rule| {
                        rule.get("providerId").and_then(|v| v.as_str()) != Some("echobird")
                    });
                }
            }
        }
        if config
            .get("defaultModelSelection")
            .and_then(|v| v.get("providerId"))
            .and_then(|v| v.as_str())
            == Some("echobird")
        {
            config.remove("defaultModelSelection");
        }
        if let Some(order) = config
            .get_mut("providerOrder")
            .and_then(|v| v.as_array_mut())
        {
            order.retain(|id| id.as_str() != Some("echobird"));
        }
        if let Err(error) = write_json_file(&personal_path, &root) {
            return ApplyResult {
                success: false,
                message: error,
            };
        }
        updated = true;
    }

    ApplyResult {
        success: true,
        message: if updated {
            "ZCode restored — Echobird provider removed.".to_string()
        } else {
            "ZCode already at defaults — no config file to update.".to_string()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn personal_fixture() -> serde_json::Value {
        serde_json::json!({
            "schemaVersion": 1,
            "config": {
                "providerOrder": ["existing"],
                "providerConfigRules": {
                    "providerRules": [{
                        "providerId": "existing",
                        "config": { "group": "standard-personal" }
                    }]
                },
                "modelConfigRules": {
                    "providerModelRules": [],
                    "manualProviderModelRules": []
                }
            }
        })
    }

    #[test]
    fn prepares_current_zcode_provider_config_without_dropping_other_providers() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("provider_config.json");
        write_json_file(&path, &personal_fixture()).unwrap();

        let updated = prepare_zcode_personal_config(
            &path,
            "vendor/model",
            "secret",
            "https://example.com/v1",
            "openai",
        )
        .unwrap()
        .unwrap();
        let rules = updated
            .pointer("/config/providerConfigRules/providerRules")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["providerId"], "existing");
        assert_eq!(rules[1]["providerId"], "echobird");
        assert_eq!(
            updated["config"]["defaultModelSelection"]["modelId"],
            "vendor/model"
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn reads_current_zcode_provider_config() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("provider_config.json");
        write_json_file(&path, &personal_fixture()).unwrap();
        let updated = prepare_zcode_personal_config(
            &path,
            "claude-test",
            "secret",
            "https://example.com/anthropic",
            "anthropic",
        )
        .unwrap()
        .unwrap();
        write_json_file(&path, &updated).unwrap();

        let model = read_zcode_personal_config(&path).unwrap();
        assert_eq!(model.model.as_deref(), Some("claude-test"));
        assert_eq!(model.protocol.as_deref(), Some("anthropic"));
        assert_eq!(
            model.anthropic_url.as_deref(),
            Some("https://example.com/anthropic")
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
