//! Model configuration for openclaw.

use super::{echobird_dir, extract_domain_name, read_json_file, ApplyResult, ModelInfo};
use crate::services::native_account_files::{self, DirectoryLock, Transaction};
use serde_json::{json, Map, Value};
use std::path::Path;

// ════════════════════════════════════════════════════════════════
//  OpenClaw: Direct write to ~/.openclaw/openclaw.json
//  v2026.3.13+: models.providers custom provider, mode: "merge"
//  No longer patches openclaw.mjs — writes native config directly.
// ════════════════════════════════════════════════════════════════

pub(super) fn apply_openclaw(model_info: &ModelInfo) -> ApplyResult {
    let home = dirs::home_dir().unwrap_or_default();
    let oc_dir = home.join(".openclaw");
    let oc_config_path = oc_dir.join("openclaw.json");

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

    let api_key = model_info.api_key.as_deref().unwrap_or("");
    if api_key.is_empty() {
        return ApplyResult {
            success: false,
            message: "API Key is empty, cannot apply config".to_string(),
        };
    }

    let base_url = model_info
        .base_url
        .as_deref()
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/');
    let provider_tag = extract_domain_name(base_url);
    let provider = format!("eb_{provider_tag}");
    let protocol = match model_info.protocol.as_deref() {
        Some("openai-responses") => "openai-responses",
        Some("anthropic") => "anthropic",
        Some("openai") => "openai",
        _ if model_id.to_lowercase().contains("claude")
            || base_url.to_lowercase().contains("anthropic") =>
        {
            "anthropic"
        }
        _ => "openai",
    };
    let api = match protocol {
        "anthropic" => "anthropic-messages",
        "openai-responses" => "openai-responses",
        _ => "openai-completions",
    };
    let name = model_info.name.as_deref().unwrap_or(model_id);
    let entry = Map::from_iter([
        ("baseUrl".into(), json!(base_url)),
        ("apiKey".into(), json!(api_key)),
        ("api".into(), json!(api)),
        (
            "models".into(),
            json!([{
                "id": model_id, "name": name,
                "contextWindow": 128000, "maxTokens": 8192, "input": ["text"],
                "reasoning": false, "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}
            }]),
        ),
    ]);
    let relay = json!({
        "apiKey": api_key, "modelId": model_id, "modelName": name,
        "baseUrl": base_url, "protocol": protocol,
    });
    let selector = format!("{provider}/{model_id}");
    let outcome = save_selection(
        &oc_config_path,
        &echobird_dir().join("openclaw.json"),
        &provider,
        &selector,
        entry,
        &relay,
    );
    if let Err(message) = outcome {
        return ApplyResult {
            success: false,
            message,
        };
    }
    log::info!("[ToolConfigManager] OpenClaw model selected: {selector} ({api})");

    ApplyResult {
        success: true,
        message: format!(
            "Model \"{}\" configured for OpenClaw ({}/{}).",
            model_info.name.as_deref().unwrap_or(model_id),
            provider,
            model_id
        ),
    }
}

fn section<'a>(
    root: &'a mut Map<String, Value>,
    name: &str,
) -> Result<&'a mut Map<String, Value>, String> {
    root.entry(name)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("OpenClaw configuration field '{name}' must be an object"))
}

fn save_selection(
    config: &Path,
    relay: &Path,
    provider: &str,
    selector: &str,
    entry: Map<String, Value>,
    relay_data: &Value,
) -> Result<(), String> {
    let _lock = DirectoryLock::acquire(config)?;
    let mut document = native_account_files::object(config)?;
    let previous = current_selector(&document);
    let old_relay = read_json_file(relay);
    let previous_primary = match old_relay.filter(|data| owns_selection(data, previous)) {
        Some(data) => data["previousPrimary"].clone(),
        None => json!(previous),
    };
    let mut relay_data = relay_data.clone();
    relay_data["selector"] = json!(selector);
    relay_data["previousPrimary"] = previous_primary;
    let root = document
        .as_object_mut()
        .ok_or("Invalid OpenClaw configuration")?;
    let models = section(root, "models")?;
    models.entry("mode").or_insert_with(|| json!("merge"));
    section(section(models, "providers")?, provider)?.extend(entry);

    set_primary(root, Some(selector))?;

    let mut transaction = Transaction::guarded();
    transaction.replace_private_file(config, &native_account_files::json(&document)?)?;
    transaction.replace_private_file(relay, &native_account_files::json(&relay_data)?)?;
    transaction.commit();
    Ok(())
}

fn current_selector(config: &Value) -> Option<&str> {
    let model = config.pointer("/agents/defaults/model")?;
    model.get("primary").unwrap_or(model).as_str()
}

fn owns_selection(relay: &Value, selected: Option<&str>) -> bool {
    let Some(selected) = selected else {
        return false;
    };
    if let Some(selector) = relay["selector"].as_str() {
        return selected == selector;
    }
    // Earlier versions stored only the provider URL and model in the relay.
    let Some((base, model)) = relay["baseUrl"].as_str().zip(relay["modelId"].as_str()) else {
        return false;
    };
    selected == format!("eb_{}/{model}", extract_domain_name(base))
}

fn set_primary(root: &mut Map<String, Value>, selector: Option<&str>) -> Result<(), String> {
    let defaults = section(section(root, "agents")?, "defaults")?;
    if defaults.get("model").is_some_and(Value::is_string) {
        if let Some(selector) = selector {
            defaults.insert("model".into(), json!(selector));
        } else {
            defaults.remove("model");
        }
    } else {
        let model = section(defaults, "model")?;
        if let Some(selector) = selector {
            model.insert("primary".into(), json!(selector));
        } else {
            model.remove("primary");
        }
    }
    Ok(())
}

pub(super) fn restore() -> ApplyResult {
    let result = dirs::home_dir()
        .ok_or_else(|| "Cannot locate OpenClaw configuration".to_owned())
        .and_then(|home| {
            restore_at(
                &home.join(".openclaw").join("openclaw.json"),
                &echobird_dir().join("openclaw.json"),
            )
        });
    ApplyResult {
        success: result.is_ok(),
        message: result
            .err()
            .unwrap_or_else(|| "OpenClaw model selection restored.".into()),
    }
}

fn restore_at(config: &Path, relay: &Path) -> Result<(), String> {
    if !relay.try_exists().map_err(|error| error.to_string())? {
        return Ok(());
    }
    let _lock = DirectoryLock::acquire(config)?;
    let relay_data = native_account_files::object(relay)?;
    let mut document = native_account_files::object(config)?;
    let mut transaction = Transaction::guarded();
    if owns_selection(&relay_data, current_selector(&document)) {
        let root = document
            .as_object_mut()
            .ok_or("Invalid OpenClaw configuration")?;
        set_primary(root, relay_data["previousPrimary"].as_str())?;
        transaction.replace_private_file(config, &native_account_files::json(&document)?)?;
    }
    transaction.remove_private_file(relay)?;
    transaction.commit();
    Ok(())
}

pub(super) fn read_openclaw() -> Option<ModelInfo> {
    read_configured_model(false)
}

pub(super) fn read_for_import() -> Option<ModelInfo> {
    read_configured_model(true)
}

fn read_configured_model(for_import: bool) -> Option<ModelInfo> {
    let oc_config_path = dirs::home_dir()?.join(".openclaw").join("openclaw.json");
    let config = read_json_file(&oc_config_path)?;
    selected_model(&config, for_import)
}

fn selected_model(config: &Value, for_import: bool) -> Option<ModelInfo> {
    let primary = current_selector(config)?;
    let (provider_name, model_id) = primary.split_once('/')?;

    if !for_import && !provider_name.starts_with("eb_") {
        return None;
    }

    let provider = config.pointer("/models/providers")?.get(provider_name)?;

    let base_url = provider
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let api_key = provider
        .get("apiKey")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let api_type = provider
        .get("api")
        .and_then(|v| v.as_str())
        .unwrap_or("openai-completions");
    let protocol = match api_type {
        "anthropic-messages" => "anthropic",
        "openai-responses" => "openai-responses",
        "openai-completions" => "openai",
        _ if for_import => return None,
        _ if api_type.contains("anthropic") => "anthropic",
        _ => "openai",
    };

    // Get model name from models array
    let model_name = provider
        .get("models")
        .and_then(|m| m.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id))
        })
        .and_then(|m| m.get("name").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    Some(ModelInfo {
        name: model_name,
        model: Some(model_id.to_string()),
        base_url,
        api_key,
        anthropic_url: None,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_the_native_selection_and_preserves_other_settings_when_switching() {
        let initial = json!({
            "models": {"mode": "merge", "providers": {
                "custom": {
                    "baseUrl": "https://custom.example/v1", "apiKey": "test-key",
                    "api": "openai-responses",
                    "models": [{"id": "vendor/test", "name": "Test Model"}]
                }
            }},
            "agents": {"defaults": {"model": {"primary": "custom/vendor/test", "fallbacks": ["custom/other"]}}, "list": []},
            "gateway": {"mode": "local"},
        });
        assert!(selected_model(&initial, false).is_none());
        let model = selected_model(&initial, true).unwrap();
        assert_eq!(model.model.as_deref(), Some("vendor/test"));
        assert_eq!(model.name.as_deref(), Some("Test Model"));
        assert_eq!(model.protocol.as_deref(), Some("openai-responses"));

        let files = tempfile::tempdir().unwrap();
        let config = files.path().join("openclaw.json");
        let relay = files.path().join("relay.json");
        std::fs::write(&config, serde_json::to_vec(&initial).unwrap()).unwrap();
        let provider = initial["models"]["providers"]["custom"]
            .as_object()
            .unwrap();
        save_selection(
            &config,
            &relay,
            "eb_test",
            "eb_test/vendor/test",
            provider.clone(),
            &json!({}),
        )
        .unwrap();
        let updated = read_json_file(&config).unwrap();
        assert_eq!(
            updated["models"]["providers"]["custom"],
            initial["models"]["providers"]["custom"]
        );
        assert_eq!(
            updated["agents"]["defaults"]["model"]["fallbacks"],
            json!(["custom/other"])
        );
        assert_eq!(updated["agents"]["list"], json!([]));
        assert_eq!(updated["gateway"], initial["gateway"]);
        assert_eq!(selected_model(&updated, false).unwrap().model, model.model);
        save_selection(
            &config,
            &relay,
            "eb_other",
            "eb_other/second",
            provider.clone(),
            &json!({}),
        )
        .unwrap();
        restore_at(&config, &relay).unwrap();
        let restored = read_json_file(&config).unwrap();
        assert_eq!(restored["agents"], initial["agents"]);
        assert_eq!(restored["gateway"], initial["gateway"]);
        assert_eq!(
            restored["models"]["providers"]["custom"],
            initial["models"]["providers"]["custom"]
        );
        assert!(!relay.exists());
    }

    #[test]
    fn restore_leaves_an_external_selection_untouched() {
        let files = tempfile::tempdir().unwrap();
        let config = files.path().join("openclaw.json");
        let relay = files.path().join("relay.json");
        let original = br#"{"agents":{"defaults":{"model":"custom/changed"}}}"#;
        std::fs::write(&config, original).unwrap();
        std::fs::write(
            &relay,
            br#"{"selector":"eb_test/model","previousPrimary":"custom/first"}"#,
        )
        .unwrap();
        restore_at(&config, &relay).unwrap();
        assert_eq!(std::fs::read(config).unwrap(), original);
        assert!(!relay.exists());
    }

    #[test]
    fn preserves_config_when_write_fails() {
        let files = tempfile::tempdir().unwrap();
        let config = files.path().join("openclaw.json");
        std::fs::write(&config, b"{}\n").unwrap();
        let relay = config.join("relay.json");
        assert!(save_selection(
            &config,
            &relay,
            "eb_test",
            "eb_test/model",
            Map::new(),
            &json!({})
        )
        .is_err());
        assert_eq!(std::fs::read(config).unwrap(), b"{}\n");
    }
}
