//! Model configuration for zcode.

use super::{
    model_input_modalities_for, read_json_file, read_jsonc_file, write_json_file, ApplyResult,
    ModelInfo,
};
use crate::services::{native_account_files::Transaction, smart_router, tool_manager, zcode_paths};
use std::path::Path;

const DEFAULT_PROVIDER_ID: &str = "echobird";
const ROUTER_PROVIDER_PREFIX: &str = "echobird-";

fn router_provider_id(base_url: &str) -> Option<String> {
    smart_router::catalog::profile_id_for_url(base_url)
        .map(|id| format!("{ROUTER_PROVIDER_PREFIX}{id}"))
}

fn is_managed_provider(id: &str) -> bool {
    id == DEFAULT_PROVIDER_ID
        || id
            .strip_prefix(ROUTER_PROVIDER_PREFIX)
            .is_some_and(smart_router::profiles::is_router_id)
}

fn is_legacy_entry_for(base_url: &serde_json::Value, provider_id: &str) -> bool {
    provider_id != DEFAULT_PROVIDER_ID
        && base_url.as_str().and_then(router_provider_id).as_deref() == Some(provider_id)
}

const REASONING_LEVELS: [&str; 5] = ["disabled", "low", "medium", "high", "max"];

// ZCode's generic protocol maps, applied after its model-specific rules so
// every model supplied through EchoBird exposes the same five levels.
const OPENAI_REASONING_MAP: &str = r#"{
  "thinking": {"type": reasoningLevel == "disabled" ? "disabled" : "enabled"},
  "enable_thinking": reasoningLevel != "disabled",
  "reasoning_effort": reasoningLevel == "disabled" ? "none" : reasoningLevel,
  "reasoning": {"effort": reasoningLevel == "disabled" ? "none" : reasoningLevel}
}"#;
const ANTHROPIC_REASONING_MAP: &str = r#"reasoningLevel == "disabled"
  ? {"thinking": {"type": "disabled"}}
  : {"thinking": {"type": "adaptive"}, "output_config": {"effort": reasoningLevel}}"#;

fn prepare_zcode_personal_config(
    path: &std::path::Path,
    model_id: &str,
    (provider_id, provider_name): (&str, &str),
    api_key: &str,
    base_url: &str,
    protocol: &str,
    properties: &serde_json::Value,
) -> Result<Option<serde_json::Value>, String> {
    if !path.exists() {
        // Leave first-run migration to ZCode so other legacy providers are
        // imported too. Older clients continue using config.json below.
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
    let api_type = if protocol == "anthropic" {
        "anthropic-messages"
    } else {
        "openai-chat-completions"
    };
    let replace_legacy = config
        .get("providerConfigRules")
        .and_then(|rules| rules["providerRules"].as_array())
        .is_some_and(|rules| {
            rules.iter().any(|rule| {
                rule["providerId"] == DEFAULT_PROVIDER_ID
                    && is_legacy_entry_for(&rule["config"]["api"]["baseUrl"], provider_id)
            })
        });
    let is_replaced =
        |id: &serde_json::Value| id == provider_id || (replace_legacy && id == DEFAULT_PROVIDER_ID);
    let reasoning_level = config
        .get("defaultModelSelection")
        .filter(|selection| {
            is_replaced(&selection["providerId"]) && selection["modelId"] == model_id
        })
        .and_then(|selection| selection.pointer("/options/reasoningLevel"))
        .and_then(|level| level.as_str())
        .filter(|level| REASONING_LEVELS.contains(level))
        .unwrap_or("max")
        .to_owned();
    let provider_rules = config
        .get_mut("providerConfigRules")
        .and_then(|v| v.get_mut("providerRules"))
        .and_then(|v| v.as_array_mut())
        .ok_or_else(|| format!("Invalid ZCode provider rules: {}", path.display()))?;
    provider_rules.retain(|rule| !is_replaced(&rule["providerId"]));
    provider_rules.push(serde_json::json!({
        "providerId": provider_id,
        "providerName": provider_name,
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
        rules.retain(|rule| !is_replaced(&rule["providerId"]));
    }
    model_rules["providerModelRules"]
        .as_array_mut()
        .expect("providerModelRules was validated above")
        .push(serde_json::json!({
            "providerId": provider_id,
            "modelId": model_id,
            "config": { "properties": properties, "optionSpecs": { "reasoningLevel": {
                "values": REASONING_LEVELS,
                "map": if protocol == "anthropic" { ANTHROPIC_REASONING_MAP } else { OPENAI_REASONING_MAP }
            }}}
        }));
    config.insert(
        "defaultModelSelection".to_string(),
        serde_json::json!({
            "providerId": provider_id, "modelId": model_id,
            "options": { "reasoningLevel": reasoning_level }
        }),
    );
    if let Some(order) = config
        .get_mut("providerOrder")
        .and_then(|v| v.as_array_mut())
    {
        order.retain(|id| !is_replaced(id));
        order.push(serde_json::Value::String(provider_id.to_string()));
    }

    Ok(Some(root))
}

pub(super) fn apply_zcode(model_info: &ModelInfo) -> ApplyResult {
    match zcode_paths::resolve() {
        Ok(paths) => apply_zcode_at(model_info, &paths.config_dir),
        Err(_) => ApplyResult {
            success: false,
            message: "Cannot read ZCode storage settings. Configuration was left unchanged.".into(),
        },
    }
}

fn apply_zcode_at(model_info: &ModelInfo, dir: &Path) -> ApplyResult {
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

    let config_path = dir.join("config.json");
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

    let router_provider = router_provider_id(&base_url);
    let provider_id = router_provider.as_deref().unwrap_or(DEFAULT_PROVIDER_ID);
    if is_legacy_entry_for(
        &config["provider"][DEFAULT_PROVIDER_ID]["options"]["baseURL"],
        provider_id,
    ) {
        config["provider"]
            .as_object_mut()
            .unwrap()
            .remove(DEFAULT_PROVIDER_ID);
    }
    let display_name = model_info
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(model_id);
    // Resolve the real input modalities for the selected model so ZCode's
    // `modalities.input` reflects image/video support when the model has it,
    // instead of declaring every model text-only.
    let mut input_modalities = model_input_modalities_for(model_id).to_vec();
    if let Some(enabled) = model_info.image_input {
        input_modalities.retain(|modality| *modality != "image");
        if enabled {
            input_modalities.push("image");
        }
    }
    let mut properties = serde_json::json!({
        "inputFormat": {
            "supportsText": true,
            "supportsImage": input_modalities.contains(&"image"),
            "supportsVideo": input_modalities.contains(&"video")
        }
    });
    if let Some(enabled) = model_info.web_search {
        properties["supportsNativeWebSearch"] = serde_json::json!(enabled);
    }
    let personal_path = dir.join("provider_config.json");
    if !personal_path.exists()
        && (model_info.web_search.is_some() || model_info.image_input.is_some())
    {
        return ApplyResult {
            success: false,
            message: "Open ZCode once to initialize its provider configuration before changing model capabilities.".into(),
        };
    }
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
    // Keep the legacy selectors for clients that have not migrated yet.
    config["model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));
    config["small_model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));

    if let Err(message) =
        super::switching::model_map(model_info, &mut config["provider"][provider_id]["models"])
    {
        return ApplyResult {
            success: false,
            message,
        };
    }
    let mut personal = match prepare_zcode_personal_config(
        &personal_path,
        model_id,
        (provider_id, display_name),
        &api_key,
        &base_url,
        model_info.protocol.as_deref().unwrap_or("openai"),
        &properties,
    ) {
        Ok(config) => config,
        Err(error) => {
            return ApplyResult {
                success: false,
                message: error,
            }
        }
    };

    if let Some(personal) = personal.as_mut() {
        if let Err(message) = expand_personal_models(personal, provider_id, model_info) {
            return ApplyResult {
                success: false,
                message,
            };
        }
    }
    let publish = || -> Result<(), String> {
        let mut transaction = Transaction::guarded();
        transaction.replace_private_file(
            &config_path,
            &serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
        )?;
        if let Some(personal) = personal {
            transaction.replace_private_file(
                &personal_path,
                &serde_json::to_vec_pretty(&personal).map_err(|error| error.to_string())?,
            )?;
        }
        transaction.commit();
        Ok(())
    };
    match publish() {
        Ok(()) => ApplyResult {
            success: true,
            message: format!(
                "Model \"{display_name}\" configured for ZCode. Restart ZCode to apply."
            ),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

fn expand_personal_models(
    root: &mut serde_json::Value,
    provider_id: &str,
    info: &ModelInfo,
) -> Result<(), String> {
    let models = super::switching::models(info)?;
    if models.is_empty() {
        return Ok(());
    }
    let config = &mut root["config"];
    let provider = config["providerConfigRules"]["providerRules"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["providerId"] == provider_id)
        .unwrap();
    let ids: Vec<_> = models.iter().map(|model| model.id.as_str()).collect();
    provider["config"]["personalModelIds"] = serde_json::json!(ids);
    provider["config"]["modelOrder"] = serde_json::json!(ids);
    let rules = config["modelConfigRules"]["providerModelRules"]
        .as_array_mut()
        .unwrap();
    let template = rules
        .iter()
        .find(|entry| entry["providerId"] == provider_id)
        .unwrap()
        .clone();
    for model in models
        .into_iter()
        .filter(|model| Some(model.id.as_str()) != info.model.as_deref())
    {
        let mut entry = template.clone();
        entry["modelId"] = serde_json::json!(model.id);
        entry["config"]["properties"]["displayName"] = serde_json::json!(model.name);
        rules.push(entry);
    }
    Ok(())
}

fn read_zcode_personal_config(path: &std::path::Path) -> Option<ModelInfo> {
    let root = read_json_file(path)?;
    if root.get("schemaVersion")?.as_u64()? != 1 {
        return None;
    }
    let config = root.get("config")?;
    let selected = config.get("defaultModelSelection")?;
    let provider_id = selected.get("providerId")?.as_str()?;
    if !is_managed_provider(provider_id) {
        return None;
    }
    let model_id = selected.get("modelId")?.as_str()?;
    let provider = config
        .pointer("/providerConfigRules/providerRules")?
        .as_array()?
        .iter()
        .find(|rule| rule.get("providerId").and_then(|v| v.as_str()) == Some(provider_id))?;
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
    let properties = config
        .pointer("/modelConfigRules/providerModelRules")
        .and_then(|v| v.as_array())
        .and_then(|rules| {
            rules
                .iter()
                .find(|rule| rule["providerId"] == provider_id && rule["modelId"] == model_id)
        })
        .and_then(|rule| rule.pointer("/config/properties"));

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
        web_search: properties
            .and_then(|v| v.get("supportsNativeWebSearch"))
            .and_then(|v| v.as_bool()),
        image_input: properties
            .and_then(|v| v.pointer("/inputFormat/supportsImage"))
            .and_then(|v| v.as_bool()),
        one_m_context: None,
    })
}

pub(super) fn read_zcode() -> Option<ModelInfo> {
    read_zcode_at(&zcode_paths::resolve().ok()?.config_dir)
}

fn read_zcode_at(dir: &Path) -> Option<ModelInfo> {
    let personal_path = dir.join("provider_config.json");
    if personal_path.exists() {
        return read_zcode_personal_config(&personal_path);
    }
    let config = read_jsonc_file(&dir.join("config.json"))?;
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
        image_input: provider["models"][model_id]["modalities"]["input"]
            .as_array()
            .map(|modalities| modalities.iter().any(|value| value == "image")),
        one_m_context: None,
    })
}

pub(super) fn restore_zcode_to_official() -> ApplyResult {
    match zcode_paths::resolve() {
        Ok(paths) => restore_zcode_to_official_at(&paths.config_dir),
        Err(_) => ApplyResult {
            success: false,
            message: "Cannot read ZCode storage settings. Configuration was left unchanged.".into(),
        },
    }
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
            provider.retain(|id, _| !is_managed_provider(id));
        }
        for key in ["model", "small_model"] {
            if config
                .get(key)
                .and_then(|v| v.as_str())
                .and_then(|value| value.split_once('/'))
                .is_some_and(|(id, _)| is_managed_provider(id))
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
            rules.retain(|rule| !rule["providerId"].as_str().is_some_and(is_managed_provider));
        }
        if let Some(model_rules) = config
            .get_mut("modelConfigRules")
            .and_then(|v| v.as_object_mut())
        {
            for key in ["providerModelRules", "manualProviderModelRules"] {
                if let Some(rules) = model_rules.get_mut(key).and_then(|v| v.as_array_mut()) {
                    rules.retain(|rule| {
                        !rule["providerId"].as_str().is_some_and(is_managed_provider)
                    });
                }
            }
        }
        if config
            .get("defaultModelSelection")
            .and_then(|v| v.get("providerId"))
            .and_then(|v| v.as_str())
            .is_some_and(is_managed_provider)
        {
            config.remove("defaultModelSelection");
        }
        if let Some(order) = config
            .get_mut("providerOrder")
            .and_then(|v| v.as_array_mut())
        {
            order.retain(|id| !id.as_str().is_some_and(is_managed_provider));
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

    #[test]
    fn applying_two_router_groups_keeps_both_native_providers_and_selects_the_last_group() {
        use serde_json::json;
        let dir = super::super::switching::TestDir::new();
        let root = dir.path();
        let group = "smart-router-08100ee6-d594-4b1a-ae27-f746cfe1d154";
        let configs = root.join("config");
        write_json_file(
            &configs.join("smart-routers.json"),
            &json!({"enabled":true,"ids":["smart-router",group]}),
        )
        .unwrap();
        for (id, port) in [("smart-router", 53683), (group, 53684)] {
            write_json_file(&configs.join(format!("{id}.json")), &json!({"port":port})).unwrap();
        }
        let legacy_path = root.join("config.json");
        let personal_path = root.join("provider_config.json");
        let first_url = "http://127.0.0.1:53683/v1";
        let second_url = "http://127.0.0.1:53684/v1";
        let mut fixture = personal_fixture();
        fixture["config"]["providerOrder"] = json!(["existing", "echobird", "echobird-custom"]);
        fixture["config"]["providerConfigRules"]["providerRules"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"providerId":"echobird","config":{"api":{"baseUrl":first_url}}}),
                json!({"providerId":"echobird-custom","config":{"keep":true}}),
            ]);
        write_json_file(&personal_path, &fixture).unwrap();
        write_json_file(
            &legacy_path,
            &json!({"provider":{
                "echobird":{"options":{"baseURL":first_url}},
                "echobird-custom":{"keep":true}
            }}),
        )
        .unwrap();

        smart_router::profiles::with_fixture_root(root.to_path_buf(), || {
            let apply = |url, name, protocol| {
                let info = serde_json::from_value(json!({
                    "model":"auto","name":name,"baseUrl":url,"apiKey":"fixture","protocol":protocol
                }))
                .unwrap();
                smart_router::catalog::with_test_models_at(url, || {
                    let result = apply_zcode_at(&info, root);
                    assert!(result.success, "{}", result.message);
                });
            };
            apply(first_url, "First", "openai");
            apply(second_url, "Second", "anthropic");
            assert_eq!(
                read_zcode_at(root).unwrap().base_url.as_deref(),
                Some(second_url)
            );
            apply(first_url, "First updated", "openai");

            let first_id = "echobird-smart-router";
            let second_id = format!("echobird-{group}");
            let legacy = read_json_file(&legacy_path).unwrap();
            assert_eq!(legacy["model"], format!("{first_id}/auto"));
            assert_eq!(
                legacy["provider"][first_id]["options"]["baseURL"],
                first_url
            );
            assert_eq!(
                legacy["provider"][&second_id]["options"]["baseURL"],
                second_url
            );
            assert!(legacy["provider"].get(DEFAULT_PROVIDER_ID).is_none());

            let personal = read_json_file(&personal_path).unwrap();
            let config = &personal["config"];
            assert_eq!(config["defaultModelSelection"]["providerId"], first_id);
            let rules = config["providerConfigRules"]["providerRules"]
                .as_array()
                .unwrap();
            assert_eq!(rules.len(), 4);
            for id in [first_id, &second_id] {
                let rule = rules.iter().find(|rule| rule["providerId"] == id).unwrap();
                assert_eq!(
                    rule["config"]["personalModelIds"].as_array().unwrap().len(),
                    3
                );
                assert_eq!(
                    rule["config"]["personalModelIds"],
                    rule["config"]["modelOrder"]
                );
            }
            assert_eq!(
                read_zcode_at(root).unwrap().base_url.as_deref(),
                Some(first_url)
            );
            assert!(restore_zcode_to_official_at(root).success);
            assert!(read_zcode_at(root).is_none());
            let legacy = read_json_file(&legacy_path).unwrap();
            assert_eq!(legacy["provider"], json!({"echobird-custom":{"keep":true}}));
            let personal = read_json_file(&personal_path).unwrap();
            assert_eq!(
                personal["config"]["providerOrder"],
                json!(["existing", "echobird-custom"])
            );
        });
    }

    #[test]
    fn capability_switches_round_trip_and_preserve_other_providers_and_video() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        write_json_file(&dir.join("provider_config.json"), &personal_fixture()).unwrap();
        write_json_file(
            &dir.join("config.json"),
            &serde_json::json!({"provider":{"existing":{"keep":true}}}),
        )
        .unwrap();
        for protocol in ["openai", "anthropic"] {
            for search in [false, true] {
                for image in [false, true] {
                    let model: ModelInfo = serde_json::from_value(serde_json::json!({
                        "model":"MiniMax-M3", "apiKey":"fixture", "protocol":protocol,
                        "webSearch":search, "imageInput":image
                    }))
                    .unwrap();
                    assert!(apply_zcode_at(&model, &dir).success);
                    let personal = read_json_file(&dir.join("provider_config.json")).unwrap();
                    let properties = &personal["config"]["modelConfigRules"]["providerModelRules"]
                        [0]["config"]["properties"];
                    assert_eq!(properties["supportsNativeWebSearch"], search);
                    assert_eq!(properties["inputFormat"]["supportsImage"], image);
                    assert_eq!(properties["inputFormat"]["supportsText"], true);
                    assert_eq!(properties["inputFormat"]["supportsVideo"], true);
                    assert_eq!(
                        personal["config"]["providerConfigRules"]["providerRules"][0]["providerId"],
                        "existing"
                    );
                    let read = read_zcode_at(&dir).unwrap();
                    assert_eq!(read.web_search, Some(search));
                    assert_eq!(read.image_input, Some(image));
                    let legacy = read_json_file(&dir.join("config.json")).unwrap();
                    assert_eq!(legacy["provider"]["existing"]["keep"], true);
                    let modalities = legacy["provider"]["echobird"]["models"]["MiniMax-M3"]
                        ["modalities"]["input"]
                        .as_array()
                        .unwrap();
                    assert_eq!(modalities.contains(&serde_json::json!("image")), image);
                    assert!(modalities.contains(&serde_json::json!("video")));
                }
            }
        }
        assert!(restore_zcode_to_official_at(&dir).success);
        assert!(read_zcode_at(&dir).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_current_config_rejects_explicit_capabilities_before_writes() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config.json");
        write_json_file(
            &path,
            &serde_json::json!({"provider":{"existing":{"keep":true}}}),
        )
        .unwrap();
        let before = std::fs::read(&path).unwrap();
        let model: ModelInfo = serde_json::from_value(serde_json::json!({
            "model":"vision/unknown", "apiKey":"fixture", "webSearch":false, "imageInput":true
        }))
        .unwrap();
        let result = apply_zcode_at(&model, &dir);
        assert!(!result.success);
        assert!(result.message.contains("Open ZCode once"));
        assert_eq!(std::fs::read(path).unwrap(), before);
        assert!(!dir.join("provider_config.json").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn image_override_enables_unknown_models_without_guessing_from_model_name() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        write_json_file(&dir.join("provider_config.json"), &personal_fixture()).unwrap();
        let mut model: ModelInfo = serde_json::from_value(serde_json::json!({
            "model":"vision/unknown", "apiKey":"fixture", "imageInput":true
        }))
        .unwrap();
        assert!(apply_zcode_at(&model, &dir).success);
        assert_eq!(read_zcode_at(&dir).unwrap().image_input, Some(true));
        model.image_input = Some(false);
        assert!(apply_zcode_at(&model, &dir).success);
        assert_eq!(read_zcode_at(&dir).unwrap().image_input, Some(false));
        std::fs::remove_dir_all(dir).unwrap();
    }

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
            (DEFAULT_PROVIDER_ID, "My Provider"),
            "secret",
            "https://example.com/v1",
            "openai",
            &serde_json::json!({}),
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
        assert_eq!(rules[1]["providerName"], "My Provider");
        assert_eq!(
            updated.pointer("/config/modelConfigRules/providerModelRules/0/config/optionSpecs/reasoningLevel/values").unwrap(),
            &serde_json::json!(REASONING_LEVELS)
        );
        assert_eq!(
            updated["config"]["defaultModelSelection"]["modelId"],
            "vendor/model"
        );
        assert_eq!(
            updated["config"]["defaultModelSelection"]["options"]["reasoningLevel"],
            "max"
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
            (DEFAULT_PROVIDER_ID, "My Anthropic Provider"),
            "secret",
            "https://example.com/anthropic",
            "anthropic",
            &serde_json::json!({}),
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

    #[test]
    fn zcode_model_apply_read_restore_follow_custom_data_directory_and_preserve_home() {
        let root = std::env::temp_dir().join(format!(
            "echobird-zcode-model-paths-{}",
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let custom = root.join("custom data");
        let home_dir = home.join(".zcode/v2");
        write_json_file(
            &home_dir.join("setting.json"),
            &serde_json::json!({"dataBaseDir":custom}),
        )
        .unwrap();
        for name in ["config.json", "provider_config.json"] {
            write_json_file(&home_dir.join(name), &serde_json::json!({"untouched":name})).unwrap();
        }
        let originals: Vec<_> = ["setting.json", "config.json", "provider_config.json"]
            .into_iter()
            .map(|name| {
                let path = home_dir.join(name);
                let bytes = std::fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        let paths = zcode_paths::resolve_at(&home, None).unwrap();
        let dir = &paths.config_dir;
        write_json_file(&dir.join("provider_config.json"), &personal_fixture()).unwrap();
        write_json_file(
            &dir.join("config.json"),
            &serde_json::json!({"provider":{"existing":{"keep":true}},"mcp":{"keep":true}}),
        )
        .unwrap();
        for protocol in ["openai", "anthropic"] {
            let model: ModelInfo = serde_json::from_value(serde_json::json!({
                "name":"Fixture Supplier", "model":"vendor/fixture", "apiKey":"fixture-key", "baseUrl":"https://example.test/v1", "protocol":protocol
            })).unwrap();
            assert!(apply_zcode_at(&model, dir).success);
            let config = read_json_file(&dir.join("provider_config.json")).unwrap();
            assert_eq!(
                config["config"]["providerConfigRules"]["providerRules"][1]["providerName"],
                "Fixture Supplier"
            );
            let rules = config["config"]["modelConfigRules"]["providerModelRules"]
                .as_array()
                .unwrap();
            assert_eq!(rules.len(), 1);
            let reasoning = &rules[0]["config"]["optionSpecs"]["reasoningLevel"];
            assert_eq!(reasoning["values"], serde_json::json!(REASONING_LEVELS));
            assert_eq!(
                reasoning["map"],
                if protocol == "anthropic" {
                    ANTHROPIC_REASONING_MAP
                } else {
                    OPENAI_REASONING_MAP
                }
            );
            let before = std::fs::read(dir.join("provider_config.json")).unwrap();
            let read = read_zcode_at(dir).unwrap();
            assert_eq!(read.model.as_deref(), Some("vendor/fixture"));
            assert_eq!(read.protocol.as_deref(), Some(protocol));
            assert_eq!(
                std::fs::read(dir.join("provider_config.json")).unwrap(),
                before
            );
        }
        assert!(restore_zcode_to_official_at(dir).success);
        assert!(read_zcode_at(dir).is_none());
        let legacy = read_json_file(&dir.join("config.json")).unwrap();
        assert_eq!(legacy["provider"]["existing"]["keep"], true);
        assert_eq!(legacy["mcp"]["keep"], true);
        assert_eq!(
            read_json_file(&dir.join("provider_config.json")).unwrap()["config"]
                ["providerConfigRules"]["providerRules"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        for (path, bytes) in originals {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_personal_config_fails_before_writing_either_config() {
        let root = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let mut invalid = personal_fixture();
        invalid["config"]["modelConfigRules"]["providerModelRules"] = serde_json::json!({});
        write_json_file(&root.join("provider_config.json"), &invalid).unwrap();
        let legacy = serde_json::json!({"untouched":true});
        write_json_file(&root.join("config.json"), &legacy).unwrap();
        let personal = std::fs::read(root.join("provider_config.json")).unwrap();
        let model: ModelInfo = serde_json::from_value(serde_json::json!({
            "model":"test-model", "apiKey":"fixture"
        }))
        .unwrap();
        assert!(!apply_zcode_at(&model, &root).success);
        assert_eq!(
            std::fs::read(root.join("provider_config.json")).unwrap(),
            personal
        );
        assert_eq!(read_json_file(&root.join("config.json")).unwrap(), legacy);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_model_id_if_name_is_blank_without_requiring_catalog() {
        let root = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        write_json_file(&root.join("provider_config.json"), &personal_fixture()).unwrap();
        for name in [None, Some(" ")] {
            let model: ModelInfo = serde_json::from_value(serde_json::json!({
                "name":name, "model":"vendor/model", "apiKey":"fixture"
            }))
            .unwrap();
            assert!(apply_zcode_at(&model, &root).success);
            let config = read_json_file(&root.join("provider_config.json")).unwrap();
            assert_eq!(
                config["config"]["providerConfigRules"]["providerRules"][1]["providerName"],
                "vendor/model"
            );
            assert_eq!(
                config["config"]["modelConfigRules"]["providerModelRules"][0]["config"]
                    ["optionSpecs"]["reasoningLevel"]["values"],
                serde_json::json!(REASONING_LEVELS)
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reapply_preserves_valid_level_only_for_same_model_and_keeps_other_model_rules() {
        let root = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let path = root.join("provider_config.json");
        for (previous_model, level, expected) in [
            ("vendor/model", "low", "low"),
            ("vendor/model", "enabled", "max"),
            ("another/model", "low", "max"),
        ] {
            let mut fixture = personal_fixture();
            fixture["config"]["defaultModelSelection"] = serde_json::json!({
                "providerId":"echobird", "modelId":previous_model, "options":{"reasoningLevel":level}
            });
            let other = serde_json::json!({"providerId":"existing", "modelId":"keep", "config":{"enabled":false}});
            for key in ["providerModelRules", "manualProviderModelRules"] {
                fixture["config"]["modelConfigRules"][key] = serde_json::json!([
                    other, {"providerId":"echobird", "modelId":"old", "config":{}}
                ]);
            }
            write_json_file(&path, &fixture).unwrap();
            let updated = prepare_zcode_personal_config(
                &path,
                "vendor/model",
                (DEFAULT_PROVIDER_ID, "Supplier"),
                "fixture",
                "https://example.test",
                "anthropic",
                &serde_json::json!({}),
            )
            .unwrap()
            .unwrap();
            assert_eq!(
                updated["config"]["defaultModelSelection"]["options"]["reasoningLevel"],
                expected
            );
            assert_eq!(
                updated["config"]["modelConfigRules"]["providerModelRules"][0],
                other
            );
            assert_eq!(
                updated["config"]["modelConfigRules"]["providerModelRules"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                updated["config"]["modelConfigRules"]["manualProviderModelRules"],
                serde_json::json!([other])
            );
            write_json_file(&path, &updated).unwrap();
            assert_eq!(
                prepare_zcode_personal_config(
                    &path,
                    "vendor/model",
                    (DEFAULT_PROVIDER_ID, "Supplier"),
                    "fixture",
                    "https://example.test",
                    "anthropic",
                    &serde_json::json!({})
                )
                .unwrap(),
                Some(updated)
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_personal_config_keeps_zcode_first_run_migration_available() {
        let root = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        let legacy = serde_json::json!({"provider":{"existing":{"keep":true}}});
        write_json_file(&root.join("config.json"), &legacy).unwrap();
        let model: ModelInfo =
            serde_json::from_value(serde_json::json!({"model":"test", "apiKey":"fixture"}))
                .unwrap();
        assert!(apply_zcode_at(&model, &root).success);
        assert!(!root.join("provider_config.json").exists());
        assert_eq!(
            read_json_file(&root.join("config.json")).unwrap()["provider"]["existing"],
            legacy["provider"]["existing"]
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
