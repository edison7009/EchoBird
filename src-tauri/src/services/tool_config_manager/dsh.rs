//! Model configuration for dsh.

use super::{
    yaml_as_map_mut, yaml_child_map, yaml_get, yaml_map, yaml_str, ApplyResult, ModelInfo,
};
use serde_yaml_ng::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(crate) static CONFIG_LOCK: Mutex<()> = Mutex::new(());

// Desktop settings live in the profile patch; credentials use the shared v1 store.
const DSH_API_KEY_ENV: &str = "ECHOBIRD_API_KEY";
const DSH_PROVIDER: &str = "echobird";
const ONE_M_CONTEXT: u64 = 1_048_576;

fn check_owned_route(settings: &Value) -> Result<(), String> {
    if let Some(route) = settings
        .get("llm-pi-ai")
        .and_then(|v| v.get("providers"))
        .and_then(|v| v.get(DSH_PROVIDER))
    {
        if route["displayName"].as_str() != Some("EchoBird")
            || route["apiKeyEnv"].as_str() != Some(DSH_API_KEY_ENV)
        {
            return Err(
                "DeepSeek Harness provider ID 'echobird' is already in use by another provider."
                    .into(),
            );
        }
    }
    Ok(())
}

// serde_yaml_ng drops tags in YAML's standard namespace, including DSH's !!js.
// Rewrite tag tokens only; quoted strings and block scalars must stay literal.
fn expression_tags(text: &str, restoring: bool) -> Result<String, String> {
    use yaml_rust2::scanner::{Scanner, Token, TokenType};
    let mut scanner = Scanner::new(text.chars());
    let offsets: Vec<_> = text.char_indices().map(|(index, _)| index).collect();
    let mut ranges = Vec::new();
    let mut handles =
        std::collections::HashMap::from([("!!".to_string(), "tag:yaml.org,2002:".to_string())]);
    while let Some(Token(marker, token)) =
        scanner.next_token().map_err(|_| "accountError.format")?
    {
        if let TokenType::TagDirective(handle, prefix) = token {
            handles.insert(handle, prefix);
        } else if let TokenType::Tag(handle, suffix) = token {
            let matches = if restoring {
                (handle == "!" || handle.is_empty()) && suffix == "echobird-dsh-js"
            } else {
                (handle.is_empty() && suffix == "tag:yaml.org,2002:js")
                    || handles
                        .get(&handle)
                        .is_some_and(|prefix| format!("{prefix}{suffix}") == "tag:yaml.org,2002:js")
            };
            if !matches {
                continue;
            }
            let start = offsets[marker.index()];
            let tail = &text[start..];
            let length = if tail.starts_with("!<") {
                tail.find('>').unwrap() + 1
            } else {
                tail.find(|c: char| c.is_whitespace() || ",[]{}".contains(c))
                    .unwrap_or(tail.len())
            };
            ranges.push(start..start + length);
        }
    }
    let mut result = text.to_string();
    for range in ranges.into_iter().rev() {
        result.replace_range(
            range,
            if restoring {
                "!!js"
            } else {
                "!<!echobird-dsh-js>"
            },
        );
    }
    Ok(result)
}

pub(crate) fn dsh_config_dir() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".dsh"))
}

fn read_document(path: &Path, default: Value) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = parse_yaml(&text)?;
            Ok(if value.is_null() { default } else { value })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(default),
        Err(_) => Err("accountError.read".into()),
    }
}

pub(crate) fn parse_yaml(text: &str) -> Result<Value, String> {
    serde_yaml_ng::from_str(&expression_tags(text, false)?)
        .map_err(|_| "accountError.format".into())
}

pub(crate) fn credentials_at(home: &Path) -> Result<Value, String> {
    let mut document = read_document(&home.join(".credentials.yaml"), yaml_map())?;
    let map = document.as_mapping_mut().ok_or("accountError.format")?;
    if !map.contains_key(yaml_str("version")) {
        if !map
            .values()
            .all(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        {
            return Err("accountError.format".into());
        }
        document =
            serde_yaml_ng::to_value(serde_json::json!({"version": 1, "refs": map, "records": {}}))
                .map_err(|_| "accountError.format")?;
    }
    let map = document.as_mapping_mut().ok_or("accountError.format")?;
    if map.get(yaml_str("version")).and_then(Value::as_u64) != Some(1) {
        return Err("accountError.format".into());
    }
    for key in ["refs", "records"] {
        let value = map.entry(yaml_str(key)).or_insert_with(yaml_map);
        if value.is_null() {
            *value = yaml_map();
        }
        if !value.is_mapping() {
            return Err("accountError.format".into());
        }
    }
    Ok(document)
}

fn profile_at(home: &Path) -> PathBuf {
    home.join("profiles/desktop/cordis.patch.yml")
}

fn profile(home: &Path) -> Result<Value, String> {
    let value = read_document(&profile_at(home), Value::Sequence(vec![]))?;
    let entries = value.as_sequence().ok_or("accountError.format")?;
    // Loader patches also include anonymous inserts, groups and repeated IDs.
    if entries.iter().any(|entry| !entry.is_mapping()) {
        return Err("accountError.format".into());
    }
    Ok(value)
}

fn settings_patch_id(entry: &Value) -> Option<&str> {
    if yaml_get(entry, "insert").is_some() {
        return None;
    }
    let id = yaml_get(entry, "id")?.as_str()?;
    let name = match id {
        "llm-pi-ai" => "@deepseek-ai/dsh-llm-pi-ai",
        "agent-default-model" => "@deepseek-ai/dsh-agent-default-model",
        _ => return Some(id),
    };
    if yaml_get(entry, "name").is_some_and(|value| {
        !value.is_null() && value.as_str() != Some("") && value.as_str() != Some(name)
    }) {
        return None;
    }
    Some(id)
}

fn settings_at(home: &Path) -> Result<Value, String> {
    let mut settings = read_document(&home.join("settings.yaml"), yaml_map())?;
    let map = settings.as_mapping_mut().ok_or("accountError.format")?;
    for entry in profile(home)?.as_sequence().unwrap() {
        if let (Some(id), Some(config)) = (settings_patch_id(entry), yaml_get(entry, "config")) {
            map.insert(yaml_str(id), config.clone());
        }
    }
    for id in ["llm-pi-ai", "agent-default-model"] {
        if let Some(config) = map.get_mut(yaml_str(id)) {
            if config.is_null() {
                *config = yaml_map();
            }
            if !config.is_mapping() {
                return Err("accountError.format".into());
            }
        }
    }
    Ok(settings)
}

fn write_settings_at(home: &Path, settings: &Value) -> Result<(), String> {
    let mut document = profile(home)?;
    let entries = document.as_sequence_mut().unwrap();
    for id in ["llm-pi-ai", "agent-default-model"] {
        if let Some(entry) = entries
            .iter_mut()
            .rev()
            .find(|e| settings_patch_id(e) == Some(id))
        {
            entry.as_mapping_mut().ok_or("accountError.format")?.insert(
                yaml_str("config"),
                yaml_get(settings, id).cloned().unwrap_or_else(yaml_map),
            );
        } else if let Some(config) = yaml_get(settings, id) {
            let mut entry = serde_yaml_ng::Mapping::new();
            entry.insert(yaml_str("id"), yaml_str(id));
            entry.insert(yaml_str("config"), config.clone());
            entries.push(Value::Mapping(entry));
        }
    }
    // A leftover legacy document is imported on the next DSH boot. Keep these
    // two sections in sync so that import cannot undo the user's selection.
    let legacy_path = home.join("settings.yaml");
    let mut legacy = read_document(&legacy_path, yaml_map())?;
    let legacy_map = legacy.as_mapping_mut().ok_or("accountError.format")?;
    for id in ["llm-pi-ai", "agent-default-model"] {
        legacy_map.remove(yaml_str(id));
        if let Some(config) = yaml_get(settings, id) {
            legacy_map.insert(yaml_str(id), config.clone());
        }
    }
    if legacy_path.exists() {
        write_private_yaml(&legacy_path, &legacy)?;
    }
    write_private_yaml(&profile_at(home), &document)
}

pub(crate) fn write_private_yaml(path: &Path, value: &Value) -> Result<(), String> {
    use std::io::Write;
    fs::create_dir_all(path.parent().ok_or("accountError.write")?)
        .map_err(|_| "accountError.write")?;
    let bytes = expression_tags(
        &serde_yaml_ng::to_string(value).map_err(|_| "accountError.format")?,
        true,
    )?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp).map_err(|_| "accountError.write")?;
        file.write_all(bytes.as_bytes())
            .map_err(|_| "accountError.write")?;
        file.sync_all().map_err(|_| "accountError.write")?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| "accountError.write".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

pub(super) fn apply_dsh(model_info: &ModelInfo) -> ApplyResult {
    let result = CONFIG_LOCK
        .lock()
        .map_err(|_| "accountError.busy".to_string())
        .and_then(|_guard| apply_at(&dsh_config_dir(), model_info));
    ApplyResult {
        success: result.is_ok(),
        message: result
            .err()
            .unwrap_or_else(|| "DeepSeek Harness configured.".into()),
    }
}

fn apply_at(home: &Path, model_info: &ModelInfo) -> Result<(), String> {
    let model_id = model_info
        .model
        .as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");
    if model_id.is_empty() {
        return Err("Model ID is empty, cannot apply config".into());
    }

    // pi-ai protocol per wire: Anthropic → anthropic-messages, else
    // openai-completions. `endpoint` is always set after this block (mirror
    // apply_openscience: the anthropic arm early-returns on an empty URL).
    let is_anthropic = model_info.protocol.as_deref() == Some("anthropic");
    let api = match model_info.protocol.as_deref() {
        Some("anthropic") => "anthropic-messages",
        Some("openai-responses") => "openai-responses",
        _ => "openai-completions",
    };
    let endpoint = if is_anthropic {
        match model_info
            .anthropic_url
            .as_deref()
            .or(model_info.base_url.as_deref())
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(|u| u.trim_end_matches('/').to_string())
        {
            Some(u) => u,
            None => {
                return Err("Base URL is empty. Pick a model first.".into());
            }
        }
    } else {
        model_info
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/')
            .to_string()
    };

    // Local llama-server / vllm proxies need no real key (mirror openscience).
    let raw_api_key = model_info.api_key.as_deref().unwrap_or("");
    let is_local_provider = endpoint.contains("127.0.0.1") || endpoint.contains("localhost");
    let api_key = if !raw_api_key.is_empty() {
        raw_api_key.to_string()
    } else if is_local_provider {
        "local-no-auth".to_string()
    } else {
        return Err("API Key is empty, cannot apply DeepSeek Harness config.".into());
    };

    let display_name = model_info.name.as_deref().unwrap_or(model_id);

    let mut settings = settings_at(home)?;
    check_owned_route(&settings)?;
    let mut creds = credentials_at(home)?;
    {
        let settings_map = yaml_as_map_mut(&mut settings);
        {
            let llm = yaml_child_map(settings_map, "llm-pi-ai");
            let providers = yaml_child_map(llm, "providers");

            let mut route = providers
                .get(yaml_str(DSH_PROVIDER))
                .filter(|route| {
                    route["api"].as_str() == Some(api)
                        && route["baseURL"]
                            .as_str()
                            .map(|url| url.trim_end_matches('/'))
                            == Some(endpoint.as_str())
                })
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            route.insert(yaml_str("displayName"), yaml_str("EchoBird"));
            route.insert(yaml_str("apiKeyEnv"), yaml_str(DSH_API_KEY_ENV));
            route.insert(yaml_str("api"), yaml_str(api));
            route.insert(yaml_str("baseURL"), yaml_str(&endpoint));
            let mut models = route
                .get(yaml_str("models"))
                .and_then(Value::as_sequence)
                .cloned()
                .unwrap_or_default();
            let index = models
                .iter()
                .position(|entry| entry["id"].as_str() == Some(model_id));
            let mut model = index
                .and_then(|index| models[index].as_mapping())
                .cloned()
                .unwrap_or_default();
            model.insert(yaml_str("id"), yaml_str(model_id));
            model.insert(yaml_str("name"), yaml_str(display_name));
            let mut efforts = model
                .get(yaml_str("reasoningEfforts"))
                .and_then(Value::as_mapping)
                .cloned()
                .unwrap_or_default();
            efforts.entry(yaml_str("off")).or_insert(Value::Null);
            for level in ["minimal", "low", "medium", "high", "xhigh", "max"] {
                efforts
                    .entry(yaml_str(level))
                    .or_insert_with(|| yaml_str(level));
            }
            model.insert(yaml_str("reasoningEfforts"), Value::Mapping(efforts));
            if let Some(image) = model_info.image_input {
                let mut input = vec![yaml_str("text")];
                if image {
                    input.push(yaml_str("image"));
                }
                model.insert(yaml_str("input"), Value::Sequence(input));
            }
            match model_info.one_m_context {
                Some(true) => {
                    model.insert(
                        yaml_str("contextWindow"),
                        Value::Number(ONE_M_CONTEXT.into()),
                    );
                }
                Some(false)
                    if model.get(yaml_str("contextWindow")).and_then(Value::as_u64)
                        == Some(ONE_M_CONTEXT) =>
                {
                    model.remove(yaml_str("contextWindow"));
                }
                _ => {}
            }
            if let Some(index) = index {
                models[index] = Value::Mapping(model);
            } else {
                models.push(Value::Mapping(model));
            }
            if super::switching::models(model_info)?.is_empty() {
                route.insert(yaml_str("models"), Value::Sequence(models));
            } else {
                let mut entries = serde_json::to_value(models).map_err(|e| e.to_string())?;
                super::switching::model_array(model_info, &mut entries, "id", "name")?;
                route.insert(
                    yaml_str("models"),
                    serde_yaml_ng::to_value(entries).map_err(|e| e.to_string())?,
                );
            }
            providers.insert(yaml_str(DSH_PROVIDER), serde_yaml_ng::Value::Mapping(route));
        }

        // Active selector — new sessions default to this route/model.
        let mut selector = serde_yaml_ng::Mapping::new();
        selector.insert(yaml_str("provider"), yaml_str(DSH_PROVIDER));
        selector.insert(yaml_str("model"), yaml_str(model_id));
        settings_map.insert(
            yaml_str("agent-default-model"),
            serde_yaml_ng::Value::Mapping(selector),
        );
    }

    yaml_child_map(yaml_as_map_mut(&mut creds), "refs")
        .insert(yaml_str(DSH_API_KEY_ENV), yaml_str(&api_key));
    write_settings_at(home, &settings)?;
    write_private_yaml(&home.join(".credentials.yaml"), &creds)
}

pub(super) fn read_dsh() -> Option<ModelInfo> {
    let _guard = CONFIG_LOCK.lock().ok()?;
    read_at(&dsh_config_dir())
}

fn read_at(home: &Path) -> Option<ModelInfo> {
    read_model_at(home, false)
}

pub(super) fn read_for_import() -> Option<ModelInfo> {
    let _guard = CONFIG_LOCK.lock().ok()?;
    read_model_at(&dsh_config_dir(), true)
}

fn read_model_at(home: &Path, for_import: bool) -> Option<ModelInfo> {
    let settings = settings_at(home).ok()?;
    let selector = yaml_get(&settings, "agent-default-model")?;
    let provider = yaml_get(selector, "provider")?.as_str()?;
    if !for_import && provider != "echobird" {
        return None;
    }
    let model_id = yaml_get(selector, "model")?.as_str()?;

    let route = yaml_get(
        yaml_get(yaml_get(&settings, "llm-pi-ai")?, "providers")?,
        provider,
    )?;
    let api = yaml_get(route, "api")
        .and_then(|v| v.as_str())
        .unwrap_or("openai-completions");
    let protocol = match api {
        "anthropic-messages" => "anthropic",
        "openai-responses" if for_import => "openai-responses",
        "openai-completions" | "openai-responses" => "openai",
        _ if for_import => return None,
        _ => "openai",
    };
    let api_key = if for_import {
        let reference = yaml_get(route, "apiKeyEnv").and_then(|value| value.as_str());
        reference.and_then(|reference| {
            let credentials = credentials_at(home).ok()?;
            credentials["refs"][reference].as_str().map(str::to_owned)
        })
    } else {
        None
    };
    let base_url = yaml_get(route, "baseURL")
        .and_then(|v| v.as_str())
        .map(String::from);
    // Display name: read the models list for the entry matching `model_id`.
    let entry = yaml_get(route, "models")
        .and_then(|m| m.as_sequence())
        .and_then(|seq| {
            seq.iter()
                .find(|e| yaml_get(e, "id").and_then(|v| v.as_str()) == Some(model_id))
        });
    let name = entry
        .and_then(|e| yaml_get(e, "name").and_then(|v| v.as_str()))
        .map(String::from)
        .unwrap_or_else(|| model_id.to_string());

    let anthropic_url = if protocol == "anthropic" {
        base_url.clone()
    } else {
        None
    };

    Some(ModelInfo {
        name: Some(name),
        model: Some(model_id.to_string()),
        base_url,
        api_key,
        anthropic_url,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: entry
            .and_then(|e| yaml_get(e, "input"))
            .and_then(Value::as_sequence)
            .map(|inputs| inputs.contains(&yaml_str("image"))),
        one_m_context: entry
            .and_then(|e| yaml_get(e, "contextWindow"))
            .and_then(Value::as_u64)
            .map(|context| context == ONE_M_CONTEXT),
    })
}

pub(super) fn restore_dsh_to_official() -> ApplyResult {
    let result = CONFIG_LOCK
        .lock()
        .map_err(|_| "accountError.busy".to_string())
        .and_then(|_guard| restore_at(&dsh_config_dir()));
    ApplyResult {
        success: result.is_ok(),
        message: result
            .err()
            .unwrap_or_else(|| "DeepSeek Harness restored.".into()),
    }
}

fn remove_route(settings: &mut Value) -> Result<(), String> {
    check_owned_route(settings)?;
    if let Some(providers) = settings
        .get_mut("llm-pi-ai")
        .and_then(|v| v.get_mut("providers"))
        .and_then(Value::as_mapping_mut)
    {
        providers.remove(yaml_str(DSH_PROVIDER));
    }
    Ok(())
}

fn restore_at(home: &Path) -> Result<(), String> {
    let mut settings = settings_at(home)?;
    let mut creds = credentials_at(home)?;
    remove_route(&mut settings)?;
    if settings["agent-default-model"]["provider"].as_str() == Some("echobird") {
        settings
            .as_mapping_mut()
            .unwrap()
            .remove(yaml_str("agent-default-model"));
    }
    creds["refs"]
        .as_mapping_mut()
        .unwrap()
        .remove(yaml_str(DSH_API_KEY_ENV));
    write_settings_at(home, &settings)?;
    write_private_yaml(&home.join(".credentials.yaml"), &creds)
}

// The caller holds CONFIG_LOCK and stops DSH before applying account credentials.
pub(crate) fn apply_account_at(home: &Path, token: &str, device: &str) -> Result<(), String> {
    let mut settings = settings_at(home)?;
    let mut creds = credentials_at(home)?;
    remove_route(&mut settings)?;
    // Keep a previously selected official account model; otherwise use the shipped default.
    let model = if settings["agent-default-model"]["provider"].as_str() == Some("deepseek-account")
    {
        settings["agent-default-model"]["model"]
            .as_str()
            .unwrap_or("deepseek-flash")
            .to_string()
    } else {
        "deepseek-flash".to_string()
    };
    settings.as_mapping_mut().unwrap().insert(
        yaml_str("agent-default-model"),
        serde_yaml_ng::to_value(serde_json::json!({"provider":"deepseek-account","model":model}))
            .unwrap(),
    );
    creds["refs"]
        .as_mapping_mut()
        .unwrap()
        .remove(yaml_str(DSH_API_KEY_ENV));
    let records = creds["records"].as_mapping_mut().unwrap();
    records.insert(yaml_str("deepseek-account-platform/default"), serde_yaml_ng::to_value(serde_json::json!({
        "kind":"grant", "payload":{"version":1,"token":token,"issuer":"https://platform.deepseek.com"}
    })).unwrap());
    records.insert(
        yaml_str("deepseek-account-platform/device"),
        serde_yaml_ng::to_value(serde_json::json!({
            "kind":"grant", "payload":{"id":device}
        }))
        .unwrap(),
    );
    write_settings_at(home, &settings)?;
    write_private_yaml(&home.join(".credentials.yaml"), &creds)
}

pub(crate) fn active_account_token(home: &Path) -> Option<String> {
    if settings_at(home).ok()?["agent-default-model"]["provider"].as_str()? != "deepseek-account" {
        return None;
    }
    let creds = credentials_at(home).ok()?;
    let payload = &creds["records"]["deepseek-account-platform/default"]["payload"];
    if payload["issuer"].as_str()? != "https://platform.deepseek.com" {
        return None;
    }
    payload["token"].as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echobird-dsh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("profiles/desktop")).unwrap();
        fs::write(profile_at(&dir), "- id: ui-chat\n  config:\n    keep: true\n- id: llm-pi-ai\n  config:\n    providers:\n      other:\n        apiKeyEnv: OTHER\n").unwrap();
        fs::write(dir.join(".credentials.yaml"), "version: 1\nrefs:\n  OTHER: other-key\nrecords:\n  other/login:\n    kind: grant\n    payload:\n      token: preserved\n").unwrap();
        dir
    }
    fn model() -> ModelInfo {
        serde_json::from_value(json!({"name":"Test", "model":"test-model", "apiKey":"test-key", "baseUrl":"https://example.test/v1", "protocol":"openai"})).unwrap()
    }

    #[test]
    fn model_capabilities_preserve_limits_and_reset_only_one_m() {
        let dir = fixture();
        apply_at(&dir, &model()).unwrap();
        let mut settings = settings_at(&dir).unwrap();
        let route = &mut settings["llm-pi-ai"]["providers"]["echobird"];
        route["models"][0]["maxTokens"] = serde_yaml_ng::to_value(64000).unwrap();
        route["models"][0]["custom"] = serde_yaml_ng::to_value(true).unwrap();
        route["models"]
            .as_sequence_mut()
            .unwrap()
            .push(serde_yaml_ng::from_str("id: manual\nname: Keep").unwrap());
        write_settings_at(&dir, &settings).unwrap();
        let mut info = model();
        info.image_input = Some(true);
        info.one_m_context = Some(true);
        apply_at(&dir, &info).unwrap();
        let saved = settings_at(&dir).unwrap();
        let route = &saved["llm-pi-ai"]["providers"]["echobird"];
        let entry = &route["models"][0];
        assert_eq!(entry["contextWindow"].as_u64(), Some(1_048_576));
        assert_eq!(entry["maxTokens"].as_u64(), Some(64000));
        assert_eq!(entry["custom"].as_bool(), Some(true));
        assert_eq!(
            entry["input"],
            serde_yaml_ng::to_value(["text", "image"]).unwrap()
        );
        assert_eq!(entry["reasoningEfforts"], serde_yaml_ng::from_str::<Value>("off: null\nminimal: minimal\nlow: low\nmedium: medium\nhigh: high\nxhigh: xhigh\nmax: max").unwrap());
        assert_eq!(route["models"][1]["id"].as_str(), Some("manual"));
        let read = read_at(&dir).unwrap();
        assert_eq!(read.image_input, Some(true));
        assert_eq!(read.one_m_context, Some(true));
        info.image_input = Some(false);
        info.one_m_context = Some(false);
        apply_at(&dir, &info).unwrap();
        let saved = settings_at(&dir).unwrap();
        let entry = &saved["llm-pi-ai"]["providers"]["echobird"]["models"][0];
        assert!(entry.get("contextWindow").is_none());
        assert_eq!(entry["maxTokens"].as_u64(), Some(64000));
        assert_eq!(entry["input"], serde_yaml_ng::to_value(["text"]).unwrap());
        info.base_url = Some("https://other.test/v1".into());
        info.one_m_context = Some(true);
        apply_at(&dir, &info).unwrap();
        let saved = settings_at(&dir).unwrap();
        let entry = &saved["llm-pi-ai"]["providers"]["echobird"]["models"][0];
        assert_eq!(entry["contextWindow"].as_u64(), Some(1_048_576));
        assert!(entry.get("maxTokens").is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn foreign_provider_id_is_never_overwritten_or_removed() {
        let dir = fixture();
        let mut settings = settings_at(&dir).unwrap();
        settings["llm-pi-ai"]["providers"]["echobird"] = serde_yaml_ng::from_str(
            "displayName: Personal\napiKeyEnv: PERSONAL\nbaseURL: https://personal.test/v1",
        )
        .unwrap();
        write_settings_at(&dir, &settings).unwrap();
        let before = fs::read(profile_at(&dir)).unwrap();
        let credentials = fs::read(dir.join(".credentials.yaml")).unwrap();
        assert!(apply_at(&dir, &model())
            .unwrap_err()
            .contains("already in use"));
        assert!(restore_at(&dir).is_err());
        assert!(apply_account_at(&dir, "test-token", "device").is_err());
        assert_eq!(fs::read(profile_at(&dir)).unwrap(), before);
        assert_eq!(
            fs::read(dir.join(".credentials.yaml")).unwrap(),
            credentials
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_desktop_documents_accept_first_configuration() {
        let dir = fixture();
        fs::write(profile_at(&dir), "# no overrides yet\n").unwrap();
        fs::write(dir.join(".credentials.yaml"), "").unwrap();
        apply_at(&dir, &model()).unwrap();
        assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
        assert_eq!(credentials_at(&dir).unwrap()["version"].as_u64(), Some(1));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn official_patch_operations_survive_model_and_account_changes() {
        for text in [
            "- insert:\n    - id: custom\n      name: custom-plugin\n      config: {keep: true}\n",
            "- id: group\n  group: true\n  config:\n    - id: nested\n      name: nested-plugin\n",
            "- id: custom\n  config: null\n- id: disabled\n  config: false\n",
            "- id: custom\n  config: {keep: true}\n- id: custom\n  disabled: true\n",
            "- id: custom\n  config: !!js 'ctx.example'\n",
        ] {
            let dir = fixture();
            fs::write(profile_at(&dir), text).unwrap();
            let original = profile(&dir).unwrap();
            let original = original.as_sequence().unwrap();
            apply_at(&dir, &model()).unwrap();
            assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
            restore_at(&dir).unwrap();
            assert!(read_at(&dir).is_none());
            apply_account_at(&dir, "account-token", "device").unwrap();
            assert_eq!(active_account_token(&dir).as_deref(), Some("account-token"));
            let saved = profile(&dir).unwrap();
            assert_eq!(&saved.as_sequence().unwrap()[..original.len()], original);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn expression_tags_survive_writes_without_changing_literal_strings() {
        let dir = fixture();
        fs::write(profile_at(&dir), "- id: custom\n  config:\n    title: 中文\n    short: !!js 'ctx.example'\n    full: !<tag:yaml.org,2002:js> 'ctx.other'\n    literal: '!!js !echobird-dsh-js !<!echobird-dsh-js>'\n    block: |\n      !!js literal text\n").unwrap();
        apply_at(&dir, &model()).unwrap();
        let saved = fs::read_to_string(profile_at(&dir)).unwrap();
        let restored = profile(&dir).unwrap();
        assert!(saved.contains("short: !!js "));
        assert!(saved.contains("full: !!js "));
        assert_eq!(
            restored[0]["config"]["literal"].as_str(),
            Some("!!js !echobird-dsh-js !<!echobird-dsh-js>")
        );
        assert_eq!(
            restored[0]["config"]["block"].as_str(),
            Some("!!js literal text\n")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn equivalent_expression_tags_survive_model_changes() {
        for (directive, tag) in [
            ("", "!!%6as"),
            ("", "!<tag:yaml.org,2002:j%73>"),
            ("%TAG !dsh! tag:yaml.org,2002:\n---\n", "!dsh!js"),
            ("%TAG ! tag:yaml.org,2002:\n---\n", "!js"),
            ("%TAG !dsh! tag:yaml.org,2002:j\n---\n", "!dsh!s"),
        ] {
            let dir = fixture();
            fs::write(
                profile_at(&dir),
                format!("{directive}- id: custom\n  config: {tag} ctx.example\n"),
            )
            .unwrap();
            apply_at(&dir, &model()).unwrap();
            let saved = fs::read_to_string(profile_at(&dir)).unwrap();
            assert!(saved.contains("config: !!js ctx.example"), "{saved}");
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn model_changes_update_the_last_override_without_touching_insert_patches() {
        let dir = fixture();
        fs::write(profile_at(&dir), "- id: llm-pi-ai\n  config: {providers: {earlier: {keep: true}}}\n- id: llm-pi-ai\n  config: {providers: {other: {apiKeyEnv: OTHER}}}\n- id: agent-default-model\n  config: {provider: echobird, model: old}\n- id: agent-default-model\n  config: {provider: echobird, model: latest}\n- id: llm-pi-ai\n  insert: []\n").unwrap();
        let original = profile(&dir).unwrap();
        assert_eq!(
            settings_at(&dir).unwrap()["agent-default-model"]["model"].as_str(),
            Some("latest")
        );
        apply_at(&dir, &model()).unwrap();
        let saved = profile(&dir).unwrap();
        assert_eq!(saved[0], original[0]);
        assert_eq!(saved[2], original[2]);
        assert_eq!(saved[4], original[4]);
        assert_eq!(
            saved[1]["config"]["providers"]["echobird"]["models"][0]["id"].as_str(),
            Some("test-model")
        );
        assert_eq!(
            saved[1]["config"]["providers"]["other"]["apiKeyEnv"].as_str(),
            Some("OTHER")
        );
        assert_eq!(saved[3]["config"]["model"].as_str(), Some("test-model"));
        assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn superseded_model_configs_do_not_block_the_effective_override() {
        for config in ["{}", "null"] {
            let dir = fixture();
            fs::write(profile_at(&dir), format!("- id: llm-pi-ai\n  config: false\n- id: llm-pi-ai\n  config: {config}\n- id: agent-default-model\n  config: []\n- id: agent-default-model\n  config: {config}\n")).unwrap();
            let original = profile(&dir).unwrap();
            apply_at(&dir, &model()).unwrap();
            assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
            restore_at(&dir).unwrap();
            apply_account_at(&dir, "account-token", "device").unwrap();
            assert_eq!(active_account_token(&dir).as_deref(), Some("account-token"));
            let saved = profile(&dir).unwrap();
            assert_eq!(saved[0], original[0]);
            assert_eq!(saved[2], original[2]);
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn official_credentials_allow_null_sections_and_preserve_foreign_fields() {
        for text in [
            "version: 1\nrefs: null\nrecords: null\n",
            "version: 1\nrefs: {}\nrecords: {}\nforeign: {keep: [1, 2]}\n",
        ] {
            let dir = fixture();
            fs::write(dir.join(".credentials.yaml"), text).unwrap();
            let original: Value = serde_yaml_ng::from_str(text).unwrap();
            apply_at(&dir, &model()).unwrap();
            let saved = credentials_at(&dir).unwrap();
            assert_eq!(saved.get("foreign"), original.get("foreign"));
            assert_eq!(saved["refs"][DSH_API_KEY_ENV].as_str(), Some("test-key"));
            apply_account_at(&dir, "account-token", "device").unwrap();
            restore_at(&dir).unwrap();
            assert_eq!(
                credentials_at(&dir).unwrap().get("foreign"),
                original.get("foreign")
            );
            assert_eq!(active_account_token(&dir).as_deref(), Some("account-token"));
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn null_model_overrides_and_name_assertions_follow_loader_semantics() {
        let dir = fixture();
        fs::write(profile_at(&dir), "- id: llm-pi-ai\n  config: null\n- id: agent-default-model\n  config: null\n- id: agent-default-model\n  name: another-plugin\n  config: {provider: echobird, model: ignored}\n").unwrap();
        let original = profile(&dir).unwrap();
        assert!(read_at(&dir).is_none());
        apply_at(&dir, &model()).unwrap();
        assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
        assert_eq!(profile(&dir).unwrap()[2], original[2]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_patch_names_do_not_hide_existing_model_configuration() {
        for name in ["''", "null"] {
            let dir = fixture();
            fs::write(profile_at(&dir), format!("- id: llm-pi-ai\n  name: {name}\n  config:\n    providers:\n      echobird: {{displayName: EchoBird, apiKeyEnv: ECHOBIRD_API_KEY, baseURL: 'https://example.com/v1'}}\n- id: agent-default-model\n  name: {name}\n  config: {{provider: echobird, model: existing}}\n")).unwrap();
            assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("existing"));
            let original = profile(&dir).unwrap();
            apply_at(&dir, &model()).unwrap();
            let saved = profile(&dir).unwrap();
            assert_eq!(saved.as_sequence().unwrap().len(), 2);
            assert_eq!(saved[0]["name"], original[0]["name"]);
            assert_eq!(saved[1]["name"], original[1]["name"]);
            assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn auto_router_configuration_uses_the_selected_protocol_and_local_credentials() {
        for (protocol, endpoint, api) in [
            ("openai", "http://127.0.0.1:53683/v1", "openai-completions"),
            ("anthropic", "http://127.0.0.1:53683", "anthropic-messages"),
        ] {
            let dir = fixture();
            let mut router = model();
            router.model = Some("auto".into());
            router.base_url = Some(endpoint.into());
            router.protocol = Some(protocol.into());
            router.api_key = Some("router-fixture-key".into());
            apply_at(&dir, &router).unwrap();
            let settings = settings_at(&dir).unwrap();
            let route = &settings["llm-pi-ai"]["providers"]["echobird"];
            assert_eq!(route["baseURL"].as_str(), Some(endpoint));
            assert_eq!(route["api"].as_str(), Some(api));
            assert_eq!(route["apiKeyEnv"].as_str(), Some(DSH_API_KEY_ENV));
            assert_eq!(route["models"][0]["id"].as_str(), Some("auto"));
            assert_eq!(
                credentials_at(&dir).unwrap()["refs"][DSH_API_KEY_ENV].as_str(),
                Some("router-fixture-key")
            );
            let read = read_at(&dir).unwrap();
            assert_eq!(read.model.as_deref(), Some("auto"));
            assert_eq!(read.protocol.as_deref(), Some(protocol));
            assert_eq!(read.base_url.as_deref(), Some(endpoint));
            fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn desktop_apply_read_restore_preserves_unrelated_settings_and_accounts() {
        let dir = fixture();
        apply_at(&dir, &model()).unwrap();
        assert!(!dir.join("settings.yaml").exists());
        assert_eq!(read_at(&dir).unwrap().model.as_deref(), Some("test-model"));
        let creds = credentials_at(&dir).unwrap();
        assert_eq!(creds["refs"][DSH_API_KEY_ENV].as_str(), Some("test-key"));
        assert!(creds.get(DSH_API_KEY_ENV).is_none());
        assert_eq!(
            creds["records"]["other/login"]["payload"]["token"].as_str(),
            Some("preserved")
        );
        restore_at(&dir).unwrap();
        assert!(read_at(&dir).is_none());
        assert!(credentials_at(&dir).unwrap()["refs"]
            .get(DSH_API_KEY_ENV)
            .is_none());
        let settings = settings_at(&dir).unwrap();
        assert!(settings["llm-pi-ai"]["providers"].get("echobird").is_none());
        assert_eq!(
            settings["llm-pi-ai"]["providers"]["other"]["apiKeyEnv"].as_str(),
            Some("OTHER")
        );
        assert_eq!(settings["ui-chat"]["keep"].as_bool(), Some(true));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn flat_credentials_upgrade_and_legacy_import_cannot_undo_account_selection() {
        let dir = fixture();
        fs::write(dir.join(".credentials.yaml"), "OTHER: preserved\n").unwrap();
        fs::write(
            dir.join("settings.yaml"),
            "agent-default-model:\n  provider: echobird\n  model: old\nui-chat:\n  old: true\n",
        )
        .unwrap();
        apply_account_at(&dir, "account-token", "device").unwrap();
        assert_eq!(active_account_token(&dir).as_deref(), Some("account-token"));
        apply_account_at(&dir, "second-token", "second-device").unwrap();
        assert_eq!(active_account_token(&dir).as_deref(), Some("second-token"));
        apply_account_at(&dir, "account-token", "device").unwrap();
        let legacy: Value =
            serde_yaml_ng::from_str(&fs::read_to_string(dir.join("settings.yaml")).unwrap())
                .unwrap();
        assert_eq!(
            legacy["agent-default-model"]["provider"].as_str(),
            Some("deepseek-account")
        );
        assert_eq!(legacy["ui-chat"]["old"].as_bool(), Some(true));
        assert_eq!(
            credentials_at(&dir).unwrap()["refs"]["OTHER"].as_str(),
            Some("preserved")
        );
        apply_at(&dir, &model()).unwrap();
        assert!(active_account_token(&dir).is_none());
        // API configuration must not delete the user's saved desktop login.
        assert_eq!(
            credentials_at(&dir).unwrap()["records"]["deepseek-account-platform/default"]
                ["payload"]["token"]
                .as_str(),
            Some("account-token")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn switching_accounts_preserves_shared_raw_and_compressed_session_files() {
        let dir = fixture();
        let mut originals = Vec::new();
        for (session, name, bytes) in [
            ("a-session", "session.jsonl", b"{\"type\":\"session\",\"version\":0,\"id\":\"a-session\",\"createdAt\":1,\"cwd\":\"/workspace\",\"delegationDepth\":0}\n{\"type\":\"tool-result\",\"data\":\"original\"}\n".as_slice()),
            // A real Zstandard frame containing the original b-session header.
            ("b-session", "session.jsonl.zstd", b"\x28\xb5\x2f\xfd\x20\x65\xb5\x02\x00\x22\x45\x12\x19\x90\xb5\x3a\x08\x39\x32\x82\xe9\xe6\xa8\xfd\x95\xdd\xfc\xc7\x45\x84\xf0\x3f\x8f\xe6\x3c\x6f\x06\x01\x43\xc1\xa4\x97\x6d\x47\x16\x53\x7b\x94\x07\x9e\x8c\x3e\x3a\x4d\x01\x8e\xbd\xe2\x21\xa3\x91\xde\x23\x7d\xcf\x1d\x22\x12\xa7\x93\x32\x9a\x74\x17\x18\xbd\x98\x52\xbd\x3a\xfd\x36\x58\x13\x02\x03\x00\x86\x0a\x0b\xa8\xf5\xda\x96\x26".as_slice()),
        ] {
            let path = dir.join("sessions/workspace").join(session).join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, bytes).unwrap();
            originals.push((path, bytes));
        }
        for (token, device) in [
            ("token-a", "device-a"),
            ("token-b", "device-b"),
            ("token-b", "device-b"),
            ("token-a", "device-a"),
        ] {
            apply_account_at(&dir, token, device).unwrap();
            assert_eq!(active_account_token(&dir).as_deref(), Some(token));
            assert_eq!(
                credentials_at(&dir).unwrap()["records"]["deepseek-account-platform/device"]
                    ["payload"]["id"]
                    .as_str(),
                Some(device)
            );
            for (path, bytes) in &originals {
                assert_eq!(fs::read(path).unwrap(), *bytes);
            }
        }
        fs::write(dir.join(".credentials.yaml"), "version: 2\n").unwrap();
        assert!(apply_account_at(&dir, "token-b", "device-b").is_err());
        for (path, bytes) in &originals {
            assert_eq!(fs::read(path).unwrap(), *bytes);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn malformed_documents_are_not_replaced() {
        for (relative, text) in [
            (".credentials.yaml", "version: 2\nrefs: {}\n"),
            (".credentials.yaml", "version: 1\nrefs: []\n"),
            (".credentials.yaml", "version: 1\nrecords: false\n"),
            ("profiles/desktop/cordis.patch.yml", "invalid: shape\n"),
            ("profiles/desktop/cordis.patch.yml", "- false\n"),
            (
                "profiles/desktop/cordis.patch.yml",
                "- id: llm-pi-ai\n  config: []\n",
            ),
            (
                "profiles/desktop/cordis.patch.yml",
                "- id: agent-default-model\n  config: invalid\n",
            ),
        ] {
            let dir = fixture();
            fs::write(dir.join(relative), text).unwrap();
            let before_config = fs::read(profile_at(&dir)).unwrap();
            let before_creds = fs::read(dir.join(".credentials.yaml")).unwrap();
            assert!(apply_at(&dir, &model()).is_err());
            assert!(apply_account_at(&dir, "token", "device").is_err());
            assert!(restore_at(&dir).is_err());
            assert_eq!(fs::read(profile_at(&dir)).unwrap(), before_config);
            assert_eq!(
                fs::read(dir.join(".credentials.yaml")).unwrap(),
                before_creds
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }
}
