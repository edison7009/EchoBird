//! Hermes native model configuration; the desktop runtime reads the same YAML.

use super::{yaml_str, ApplyResult, ModelInfo};
use crate::services::native_account_files::{read_optional, Transaction};
use serde_yaml_ng::{Mapping, Value};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

const PROVIDER: &str = "echobird";
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

fn home_from(
    home: &Path,
    local: Option<&str>,
    override_home: Option<&str>,
    windows: bool,
) -> PathBuf {
    if let Some(value) = override_home
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(value);
    }
    if windows {
        local
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Local"))
            .join("hermes")
    } else {
        home.join(".hermes")
    }
}

pub(crate) fn config_path() -> PathBuf {
    home_from(
        &dirs::home_dir().unwrap_or_default(),
        std::env::var("LOCALAPPDATA").ok().as_deref(),
        std::env::var("HERMES_HOME").ok().as_deref(),
        cfg!(windows),
    )
    .join("config.yaml")
}

fn load(path: &Path) -> Result<Value, String> {
    let Some(bytes) = read_optional(path)? else {
        return Ok(Value::Mapping(Mapping::new()));
    };
    let value: Value = serde_yaml_ng::from_slice(&bytes).map_err(|_| "accountError.format")?;
    if value.is_null() {
        return Ok(Value::Mapping(Mapping::new()));
    }
    if !value.is_mapping() {
        return Err("accountError.format".into());
    }
    Ok(value)
}

fn map<'a>(parent: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping, String> {
    let value = parent
        .entry(yaml_str(key))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if value.is_null() {
        *value = Value::Mapping(Mapping::new());
    }
    value
        .as_mapping_mut()
        .ok_or_else(|| "accountError.format".into())
}

fn apply_at(path: &Path, info: &ModelInfo) -> Result<(), String> {
    if info
        .protocol
        .as_deref()
        .is_some_and(|protocol| protocol != "openai")
    {
        return Err("Hermes requires an OpenAI-compatible model.".into());
    }
    let model = info
        .model
        .as_deref()
        .or(info.name.as_deref())
        .unwrap_or("")
        .trim();
    if model.is_empty() {
        return Err("Model ID is empty, cannot apply config".into());
    }
    let endpoint = info
        .base_url
        .as_deref()
        .unwrap_or("")
        .trim()
        .trim_end_matches('/');
    let url = reqwest::Url::parse(endpoint).map_err(|_| "Invalid Hermes base URL")?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("Invalid Hermes base URL".into());
    }
    let key = info
        .api_key
        .as_deref()
        .filter(|key| !key.is_empty())
        .unwrap_or("no-key-required");
    let mut document = load(path)?;
    let root = document.as_mapping_mut().unwrap();
    let route = map(map(root, "providers")?, PROVIDER)?;
    let previous_router = route
        .get(yaml_str("base_url"))
        .and_then(Value::as_str)
        .is_some_and(crate::services::smart_router::is_router_url);
    // Credentials from an earlier route must not override the selected API key.
    for field in ["key_cmd", "key_env", "api_key_env"] {
        route.remove(yaml_str(field));
    }
    let api_mode = if info.protocol.as_deref() == Some("openai-responses") {
        "codex_responses"
    } else {
        "chat_completions"
    };
    for (field, value) in [
        ("name", PROVIDER),
        ("base_url", endpoint),
        ("api_key", key),
        ("api_mode", api_mode),
    ] {
        route.insert(yaml_str(field), yaml_str(value));
    }
    route.insert(yaml_str("enabled"), Value::Bool(true));
    let models = super::switching::models(info)?;
    if !models.is_empty() {
        route.insert(
            yaml_str("models"),
            serde_yaml_ng::to_value(models.iter().map(|model| &model.id).collect::<Vec<_>>())
                .map_err(|e| e.to_string())?,
        );
    } else if previous_router {
        route.remove(yaml_str("models"));
    } else if let Some(models) = route
        .get_mut(yaml_str("models"))
        .and_then(Value::as_sequence_mut)
    {
        models.retain(|model| {
            !model.as_str().is_some_and(|id| {
                id.starts_with(crate::services::smart_router::catalog::SELECTOR_PREFIX)
            })
        });
    }
    if root.get("model").is_some_and(Value::is_string) {
        root.insert(yaml_str("model"), Value::Mapping(Mapping::new()));
    }
    let selected = map(root, "model")?;
    // Named providers own their endpoint, credentials and protocol. Retain limits
    // and other model options, but remove overrides left by the old adapter.
    for field in [
        "base_url",
        "api_key",
        "api",
        "api_mode",
        "key_cmd",
        "key_env",
        "api_key_env",
    ] {
        selected.remove(yaml_str(field));
    }
    selected.insert(yaml_str("default"), yaml_str(model));
    selected.insert(yaml_str("provider"), yaml_str(PROVIDER));
    let bytes = serde_yaml_ng::to_string(&document).map_err(|_| "accountError.format")?;
    let mut transaction = Transaction::new();
    transaction.replace_private_file(path, bytes.as_bytes())?;
    transaction.commit();
    Ok(())
}

pub(super) fn apply(info: &ModelInfo) -> ApplyResult {
    let result = CONFIG_LOCK
        .lock()
        .map_err(|_| "accountError.busy".to_string())
        .and_then(|_guard| apply_at(&config_path(), info));
    ApplyResult {
        success: result.is_ok(),
        message: result
            .err()
            .unwrap_or_else(|| "Hermes model configured. Restart to apply to new sessions.".into()),
    }
}

fn read_at(path: &Path) -> Option<ModelInfo> {
    read_model_at(path, false)
}

pub(super) fn read_for_import() -> Option<ModelInfo> {
    let _guard = CONFIG_LOCK.lock().ok()?;
    read_model_at(&config_path(), true)
}

fn read_model_at(path: &Path, for_import: bool) -> Option<ModelInfo> {
    let document = load(path).ok()?;
    let selected = document.get("model")?;
    let model = selected.as_str().or_else(|| {
        selected
            .get("default")
            .or_else(|| selected.get("model"))?
            .as_str()
    })?;
    let provider = selected.get("provider").and_then(Value::as_str);
    let route = provider.and_then(|provider| document.get("providers")?.get(provider));
    let endpoint = selected
        .get("base_url")
        .or_else(|| route?.get("base_url"))
        .and_then(Value::as_str);
    if model.is_empty() {
        return None;
    }
    let api_key = if for_import {
        selected
            .get("api_key")
            .or_else(|| route?.get("api_key"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    } else {
        None
    };
    Some(ModelInfo {
        name: Some(model.into()),
        model: Some(model.into()),
        base_url: endpoint.map(String::from),
        protocol: match selected
            .get("api_mode")
            .or_else(|| route?.get("api_mode"))
            .and_then(Value::as_str)
        {
            Some("anthropic_messages") => Some("anthropic".into()),
            Some("codex_responses") if for_import => Some("openai-responses".into()),
            Some("chat_completions" | "codex_responses") => Some("openai".into()),
            _ if provider == Some("openai-api") => Some("openai".into()),
            _ => None,
        },
        api_key,
        anthropic_url: None,
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

pub(super) fn read() -> Option<ModelInfo> {
    let _guard = CONFIG_LOCK.lock().ok()?;
    read_at(&config_path())
}

#[cfg(test)]
mod tests {
    #[test]
    fn smart_switch_native_catalog_regression() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let info = super::super::switching::test_info();
            let dir = super::super::switching::TestDir::new();
            let path = dir.path().join("config.yaml");
            apply_at(&path, &info).unwrap();
            let saved = load(&path).unwrap();
            assert_eq!(
                saved["providers"][PROVIDER]["models"]
                    .as_sequence()
                    .unwrap()
                    .len(),
                3
            );
            let mut direct = info;
            direct.base_url = Some("https://direct.invalid/v1".into());
            apply_at(&path, &direct).unwrap();
            assert!(load(&path).unwrap()["providers"][PROVIDER]
                .get("models")
                .is_none());
        });
    }

    use super::*;
    use std::fs;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("echobird-hermes-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> PathBuf {
            self.0.join("config.yaml")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn info() -> ModelInfo {
        serde_json::from_value(serde_json::json!({
            "model": "vendor/model: latest #1", "name": "Test", "apiKey": "key # ' : ${OTHER}",
            "baseUrl": "https://example.test/api/v4", "protocol": "openai"
        }))
        .unwrap()
    }

    #[test]
    fn switches_native_route_without_changing_other_options_or_credentials() {
        let fixture = Fixture::new();
        let path = fixture.path();
        fs::write(&path, "model:\n  default: old\n  provider: openai-api\n  base_url: https://old.test/v1\n  api_key: old\n  api_mode: codex_responses\n  key_env: OLD_KEY\n  max_tokens: 8192\n  context_length: 100000\nproviders:\n  other: {base_url: 'https://other.test', api_key: other}\n  echobird: {key_cmd: stale-command, key_env: STALE, extra_body: {keep: true}}\ncustom_providers: [{name: user-owned, base_url: 'https://owned.test'}]\nagent: {model: nested-model, reasoning_effort: high}\nmcp_servers: {user: {command: user-command}}\n").unwrap();
        for name in [".env", "auth.json", "credential_pool.json"] {
            fs::write(fixture.0.join(name), "user credentials stay intact").unwrap();
        }
        let before = load(&path).unwrap();
        let mut selected = info();
        apply_at(&path, &selected).unwrap();
        let saved = load(&path).unwrap();
        assert_eq!(
            saved["model"]["default"].as_str(),
            selected.model.as_deref()
        );
        assert_eq!(saved["model"]["provider"].as_str(), Some(PROVIDER));
        for key in ["agent", "mcp_servers", "custom_providers"] {
            assert_eq!(saved[key], before[key]);
        }
        for key in ["max_tokens", "context_length"] {
            assert_eq!(saved["model"][key], before["model"][key]);
        }
        assert_eq!(saved["providers"]["other"], before["providers"]["other"]);
        assert_eq!(
            saved["providers"][PROVIDER]["extra_body"],
            before["providers"][PROVIDER]["extra_body"]
        );
        assert!(saved["model"].get("base_url").is_none());
        assert!(saved["model"].get("api_key").is_none());
        assert!(saved["providers"][PROVIDER].get("key_cmd").is_none());
        assert_eq!(
            saved["providers"][PROVIDER]["api_key"].as_str(),
            selected.api_key.as_deref()
        );
        assert_eq!(
            saved["providers"][PROVIDER]["api_mode"].as_str(),
            Some("chat_completions")
        );
        assert_eq!(read_at(&path).unwrap().base_url, selected.base_url);
        selected.model = Some("second-model".into());
        selected.base_url = Some("https://second.test/custom/path".into());
        selected.api_key = Some("second-key".into());
        apply_at(&path, &selected).unwrap();
        let read = read_at(&path).unwrap();
        assert_eq!(read.model, selected.model);
        assert_eq!(read.base_url, selected.base_url);
        assert_eq!(
            load(&path).unwrap()["providers"][PROVIDER]["api_key"].as_str(),
            Some("second-key")
        );
        for name in [".env", "auth.json", "credential_pool.json"] {
            assert_eq!(
                fs::read_to_string(fixture.0.join(name)).unwrap(),
                "user credentials stay intact"
            );
        }
    }

    #[test]
    fn keyless_model_is_applied_instead_of_silently_skipped() {
        let fixture = Fixture::new();
        let mut selected = info();
        selected.api_key = Some(String::new());
        apply_at(&fixture.path(), &selected).unwrap();
        assert_eq!(
            load(&fixture.path()).unwrap()["providers"][PROVIDER]["api_key"].as_str(),
            Some("no-key-required")
        );
        assert_eq!(read_at(&fixture.path()).unwrap().model, selected.model);
    }

    #[tokio::test]
    async fn generic_restore_cannot_delete_the_native_hermes_document() {
        let result = super::super::restore_tool_to_official("hermes").await;
        assert!(!result.success);
    }

    #[test]
    fn invalid_native_documents_are_never_overwritten() {
        let fixture = Fixture::new();
        for text in [
            "model: [broken",
            "- not-a-root-map",
            "model: []",
            "providers: false",
            "providers: {echobird: []}",
        ] {
            fs::write(fixture.path(), text).unwrap();
            assert!(apply_at(&fixture.path(), &info()).is_err(), "{text}");
            assert_eq!(fs::read_to_string(fixture.path()).unwrap(), text);
        }
    }

    #[test]
    fn invalid_selection_and_write_failure_are_errors() {
        let fixture = Fixture::new();
        for field in ["model", "baseUrl", "protocol"] {
            let mut value = serde_json::to_value(info()).unwrap();
            value[field] = serde_json::json!(if field == "protocol" { "anthropic" } else { "" });
            assert!(apply_at(&fixture.path(), &serde_json::from_value(value).unwrap()).is_err());
            assert!(!fixture.path().exists());
        }
        fs::write(fixture.0.join("blocked"), "file, not directory").unwrap();
        assert!(apply_at(&fixture.0.join("blocked/config.yaml"), &info()).is_err());
        assert_eq!(
            fs::read_to_string(fixture.0.join("blocked")).unwrap(),
            "file, not directory"
        );
    }

    #[test]
    fn native_read_does_not_create_files_or_use_legacy_relay_state() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("hermes.json"), r#"{"modelId":"stale"}"#).unwrap();
        assert!(read_at(&fixture.path()).is_none());
        assert!(!fixture.path().exists());
        fs::write(fixture.path(), "model: {default: native-model, provider: openai-api, base_url: 'https://native.test/api/v2'}\n").unwrap();
        assert_eq!(
            read_at(&fixture.path()).unwrap().model.as_deref(),
            Some("native-model")
        );
    }

    #[test]
    fn accepts_native_scalar_and_empty_sections() {
        let fixture = Fixture::new();
        for text in [
            "model: old-model\nproviders: null\n",
            "model: null\nproviders: {echobird: null}\n",
        ] {
            fs::write(fixture.path(), text).unwrap();
            apply_at(&fixture.path(), &info()).unwrap();
            assert_eq!(read_at(&fixture.path()).unwrap().model, info().model);
        }
    }

    #[cfg(windows)]
    #[test]
    fn locked_native_config_returns_write_failure_and_keeps_original_bytes() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let original = "model: {default: original, provider: openai-api}\n";
        fs::write(fixture.path(), original).unwrap();
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(fixture.path())
            .unwrap();
        assert_eq!(
            apply_at(&fixture.path(), &info()).unwrap_err(),
            "accountError.write"
        );
        assert_eq!(fs::read_to_string(fixture.path()).unwrap(), original);
        drop(lock);
        apply_at(&fixture.path(), &info()).unwrap();
        assert_eq!(read_at(&fixture.path()).unwrap().model, info().model);
    }

    #[test]
    fn home_resolution_matches_all_three_platforms() {
        let root = Path::new("fixture-home");
        assert_eq!(
            home_from(root, Some("local-data"), None, true),
            Path::new("local-data/hermes")
        );
        assert_eq!(
            home_from(root, Some(" "), None, true),
            root.join("AppData/Local/hermes")
        );
        for windows in [true, false] {
            assert_eq!(
                home_from(root, None, Some(" custom-home "), windows),
                Path::new("custom-home")
            );
        }
        assert_eq!(
            home_from(root, Some("ignored"), Some(" "), false),
            root.join(".hermes")
        );
    }
}
