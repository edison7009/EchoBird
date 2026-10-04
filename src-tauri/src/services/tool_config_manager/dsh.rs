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

pub(crate) fn dsh_config_dir() -> PathBuf {
    std::env::var_os("DSH_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".dsh"))
}

fn read_document(path: &Path, default: Value) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value: Value = serde_yaml_ng::from_str(&text).map_err(|_| "accountError.format")?;
            Ok(if value.is_null() { default } else { value })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(default),
        Err(_) => Err("accountError.read".into()),
    }
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
    if map.get(yaml_str("version")).and_then(Value::as_u64) != Some(1)
        || map
            .keys()
            .any(|k| !matches!(k.as_str(), Some("version" | "refs" | "records")))
    {
        return Err("accountError.format".into());
    }
    for key in ["refs", "records"] {
        let value = map.entry(yaml_str(key)).or_insert_with(yaml_map);
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
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        let id = yaml_get(entry, "id")
            .and_then(Value::as_str)
            .ok_or("accountError.format")?;
        if !ids.insert(id) || yaml_get(entry, "config").is_some_and(|v| !v.is_mapping()) {
            return Err("accountError.format".into());
        }
    }
    Ok(value)
}

fn settings_at(home: &Path) -> Result<Value, String> {
    let mut settings = read_document(&home.join("settings.yaml"), yaml_map())?;
    let map = settings.as_mapping_mut().ok_or("accountError.format")?;
    for entry in profile(home)?.as_sequence().unwrap() {
        if let Some(config) = yaml_get(entry, "config") {
            map.insert(yaml_get(entry, "id").unwrap().clone(), config.clone());
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
            .find(|e| yaml_get(e, "id").and_then(Value::as_str) == Some(id))
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
    let bytes = serde_yaml_ng::to_string(value).map_err(|_| "accountError.format")?;
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
    let api = if is_anthropic {
        "anthropic-messages"
    } else {
        "openai-completions"
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
    let mut creds = credentials_at(home)?;
    {
        let settings_map = yaml_as_map_mut(&mut settings);
        {
            let llm = yaml_child_map(settings_map, "llm-pi-ai");
            let providers = yaml_child_map(llm, "providers");

            let mut route = serde_yaml_ng::Mapping::new();
            route.insert(yaml_str("displayName"), yaml_str("EchoBird"));
            route.insert(yaml_str("apiKeyEnv"), yaml_str(DSH_API_KEY_ENV));
            route.insert(yaml_str("api"), yaml_str(api));
            route.insert(yaml_str("baseURL"), yaml_str(&endpoint));
            let mut model = serde_yaml_ng::Mapping::new();
            model.insert(yaml_str("id"), yaml_str(model_id));
            model.insert(yaml_str("name"), yaml_str(display_name));
            route.insert(
                yaml_str("models"),
                serde_yaml_ng::Value::Sequence(vec![serde_yaml_ng::Value::Mapping(model)]),
            );
            providers.insert(yaml_str("echobird"), serde_yaml_ng::Value::Mapping(route));
        }

        // Active selector — new sessions default to this route/model.
        let mut selector = serde_yaml_ng::Mapping::new();
        selector.insert(yaml_str("provider"), yaml_str("echobird"));
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
    let settings = settings_at(home).ok()?;
    let selector = yaml_get(&settings, "agent-default-model")?;
    if yaml_get(selector, "provider")?.as_str()? != "echobird" {
        // Not our provider - show as unconfigured so the user can apply.
        return None;
    }
    let model_id = yaml_get(selector, "model")?.as_str()?;

    let route = yaml_get(
        yaml_get(yaml_get(&settings, "llm-pi-ai")?, "providers")?,
        "echobird",
    )?;
    let api = yaml_get(route, "api")
        .and_then(|v| v.as_str())
        .unwrap_or("openai-completions");
    let protocol = if api == "anthropic-messages" {
        "anthropic"
    } else {
        "openai"
    };
    let base_url = yaml_get(route, "baseURL")
        .and_then(|v| v.as_str())
        .map(String::from);
    // Display name: read the models list for the entry matching `model_id`.
    let name = yaml_get(route, "models")
        .and_then(|m| m.as_sequence())
        .and_then(|seq| {
            seq.iter()
                .find(|e| yaml_get(e, "id").and_then(|v| v.as_str()) == Some(model_id))
        })
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
        api_key: None,
        anthropic_url,
        protocol: Some(protocol.to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
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

fn remove_route(settings: &mut Value) {
    if let Some(providers) = settings
        .get_mut("llm-pi-ai")
        .and_then(|v| v.get_mut("providers"))
        .and_then(Value::as_mapping_mut)
    {
        providers.remove(yaml_str("echobird"));
    }
}

fn restore_at(home: &Path) -> Result<(), String> {
    let mut settings = settings_at(home)?;
    let mut creds = credentials_at(home)?;
    remove_route(&mut settings);
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
    remove_route(&mut settings);
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
    fn malformed_documents_are_not_replaced() {
        for (relative, text) in [
            (".credentials.yaml", "version: 2\nrefs: {}\n"),
            ("profiles/desktop/cordis.patch.yml", "invalid: shape\n"),
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
