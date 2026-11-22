//! MiniMax Code CLI and Desktop share the native ~/.minimax/config.yaml.
use super::{ApplyResult, ModelInfo};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(crate) fn data_dir() -> PathBuf {
    ["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"]
        .iter()
        .find_map(|key| std::env::var_os(key).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".minimax"))
}

pub(crate) fn desktop_region(exe: &Path) -> Option<&'static str> {
    let parent = exe.parent()?;
    let resources = if parent.file_name()?.to_str()? == "MacOS" {
        parent.parent()?.join("Resources")
    } else {
        parent.join("resources")
    };
    let update = fs::read_to_string(resources.join("app-update.yml")).ok()?;
    let update: Value = serde_yaml_ng::from_str(&update).ok()?;
    let url = reqwest::Url::parse(update["url"].as_str()?).ok()?;
    if !matches!(
        url.path().trim_end_matches('/'),
        "/public/minimax-agent/release" | "/public/minimax-agent-prod/release"
    ) {
        return None;
    }
    match url.host_str()? {
        "filecdn.minimax.chat" => Some("cn"),
        "file.cdn.minimax.io" => Some("en"),
        _ => None,
    }
}

// Match proper-lockfile's directory lock, shared with the native clients.
struct FileLock(PathBuf);
impl FileLock {
    fn acquire(path: &Path) -> Result<Self, String> {
        private_dir(path.parent().ok_or("Invalid MiniMax config path")?)?;
        let lock = PathBuf::from(format!("{}.lock", path.display()));
        fs::create_dir(&lock).map_err(|_| "MiniMax configuration is busy or cannot be locked")?;
        Ok(Self(lock))
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|_| "Cannot create MiniMax config directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot secure MiniMax config directory")?;
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    private_dir(path.parent().ok_or("Invalid MiniMax config path")?)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .map_err(|_| "Cannot create MiniMax config file")?;
        file.write_all(bytes)
            .map_err(|_| "Cannot write MiniMax config file")?;
        file.sync_all()
            .map_err(|_| "Cannot save MiniMax config file")?;
        drop(file);
        fs::rename(&temp, path).map_err(|_| "Cannot replace MiniMax config file".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn config_at(home: &Path) -> Result<Value, String> {
    let path = home.join("config.yaml");
    let value: Value = match fs::read(&path) {
        Ok(bytes) => {
            serde_yaml_ng::from_slice(&bytes).map_err(|_| "Invalid MiniMax YAML config")?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return Err("Cannot read MiniMax config".into()),
    };
    let value = if value.is_null() { json!({}) } else { value };
    if !value.is_object() || value.get("custom_provider").is_some_and(|v| !v.is_object()) {
        return Err("Invalid MiniMax provider config".into());
    }
    Ok(value)
}

fn write_config(home: &Path, value: &Value) -> Result<(), String> {
    write_private(
        &home.join("config.yaml"),
        serde_yaml_ng::to_string(value)
            .map_err(|_| "Cannot serialize MiniMax config")?
            .as_bytes(),
    )
}

fn clear_model_options(config: &mut Value) {
    for key in [
        "defaultModelVariant",
        "defaultModelThinking",
        "defaultModelContextWindow",
    ] {
        config.as_object_mut().unwrap().remove(key);
    }
}

pub(crate) fn select_official(config: &mut Value) {
    // Let the native managed catalog choose its current default; preserve its models.
    config.as_object_mut().unwrap().remove("defaultModel");
    config.as_object_mut().unwrap().remove("defaultLightModel");
    config["minimaxModelSource"] = json!("token_plan");
    clear_model_options(config);
}

fn apply_at(home: &Path, model: &ModelInfo) -> Result<(), String> {
    let id = model
        .model
        .as_deref()
        .or(model.name.as_deref())
        .filter(|s| !s.trim().is_empty())
        .ok_or("Model ID is empty, cannot apply config")?;
    if matches!(id, "__proto__" | "constructor" | "prototype") {
        return Err("Invalid model ID".into());
    }
    let anthropic = model.protocol.as_deref() == Some("anthropic");
    let base = if anthropic {
        model.anthropic_url.as_deref().or(model.base_url.as_deref())
    } else {
        model.base_url.as_deref()
    }
    .filter(|s| !s.trim().is_empty())
    .ok_or("API URL is empty")?
    .trim_end_matches('/');
    let api = if anthropic {
        "anthropic-messages"
    } else if model.protocol.as_deref() == Some("openai-responses") {
        "openai-responses"
    } else {
        "openai-completions"
    };
    let _guard = FileLock::acquire(&home.join("config.yaml"))?;
    let mut config = config_at(home)?;
    if config.get("custom_provider").is_none() {
        config["custom_provider"] = json!({});
    }
    let previous = &config["custom_provider"]["echobird"];
    let same_endpoint = previous["api"] == api
        && previous["options"]["baseURL"]
            .as_str()
            .is_some_and(|url| url.trim_end_matches('/') == base);
    let mut provider = if same_endpoint {
        previous.clone()
    } else {
        json!({})
    };
    let mut entry = provider["models"][id]
        .as_object()
        .map(|value| json!(value))
        .unwrap_or(json!({}));
    entry["name"] = json!(model.name.as_deref().unwrap_or(id));
    entry["reasoning"] = json!(true);
    let mut thinking = entry["thinking"].as_object().cloned().unwrap_or_default();
    thinking.insert(
        "effortOptions".into(),
        json!(["minimal", "low", "medium", "high", "xhigh", "max"]),
    );
    entry["thinking"] = json!(thinking);
    if let Some(image) = model.image_input {
        let mut modalities = entry["modalities"].as_object().cloned().unwrap_or_default();
        let mut inputs = modalities
            .get("input")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        inputs.retain(|value| value != "image");
        if !inputs.contains(&json!("text")) {
            inputs.insert(0, json!("text"));
        }
        if image {
            inputs.push(json!("image"));
        }
        entry["attachment"] = json!(inputs.iter().any(|value| value != "text"));
        modalities.insert("input".into(), json!(inputs));
        modalities.entry("output").or_insert(json!(["text"]));
        entry["modalities"] = json!(modalities);
    }
    let mut limit = entry["limit"].as_object().cloned().unwrap_or_default();
    match model.one_m_context {
        Some(true) => {
            limit.insert("context".into(), json!(1_000_000));
        }
        Some(false) if limit.get("context").and_then(Value::as_u64) == Some(1_000_000) => {
            limit.remove("context");
        }
        _ => {}
    }
    if limit.is_empty() {
        entry.as_object_mut().unwrap().remove("limit");
    } else {
        entry["limit"] = json!(limit);
    }
    provider["kind"] = json!("custom");
    provider["name"] = json!("EchoBird");
    provider["enabled"] = json!(true);
    provider["api"] = json!(api);
    let mut options = provider["options"].as_object().cloned().unwrap_or_default();
    options.insert("baseURL".into(), json!(base));
    options.insert(
        "apiKey".into(),
        json!(model.api_key.as_deref().unwrap_or("")),
    );
    provider["options"] = json!(options);
    let mut models = provider["models"].as_object().cloned().unwrap_or_default();
    models.insert(id.into(), entry);
    provider["models"] = json!(models);
    super::switching::model_map(model, &mut provider["models"])?;
    config["custom_provider"]["echobird"] = provider;
    config["defaultModel"] = json!(format!("custom_provider:echobird/{id}"));
    config["defaultLightModel"] = config["defaultModel"].clone();
    clear_model_options(&mut config);
    write_config(home, &config)
}

fn outcome(result: Result<(), String>) -> ApplyResult {
    ApplyResult {
        success: result.is_ok(),
        message: result
            .err()
            .unwrap_or_else(|| "MiniMax Code configured.".into()),
    }
}
pub(super) fn apply(model: &ModelInfo) -> ApplyResult {
    outcome(apply_at(&data_dir(), model))
}
pub(super) fn restore() -> ApplyResult {
    outcome((|| {
        let home = data_dir();
        let _guard = FileLock::acquire(&home.join("config.yaml"))?;
        let mut config = config_at(&home)?;
        select_official(&mut config);
        if let Some(providers) = config["custom_provider"].as_object_mut() {
            providers.remove("echobird");
        }
        write_config(&home, &config)
    })())
}
pub(super) fn read() -> Option<ModelInfo> {
    read_at(&data_dir())
}
fn read_at(home: &Path) -> Option<ModelInfo> {
    let config = config_at(home).ok()?;
    let (provider, id) = config["defaultModel"]
        .as_str()?
        .strip_prefix("custom_provider:")?
        .split_once('/')?;
    let provider = &config["custom_provider"][provider];
    if provider["enabled"] == false {
        return None;
    }
    serde_json::from_value(json!({
        "model":id, "name":provider["models"][id]["name"].as_str().unwrap_or(id),
        "baseUrl":provider["options"]["baseURL"], "apiKey":provider["options"]["apiKey"],
        "protocol":match provider["api"].as_str() {
            Some("anthropic-messages") => "anthropic",
            Some("openai-responses") => "openai-responses",
            _ => "openai",
        },
        "imageInput":provider["models"][id]["modalities"]["input"].as_array().map(|inputs| inputs.contains(&json!("image"))),
        "oneMContext":provider["models"][id]["limit"]["context"].as_u64().map(|context| context == 1_000_000)
    }))
    .ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn smart_switch_native_catalog_regression() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let info = super::super::switching::test_info();
            let dir = super::super::switching::TestDir::new();
            apply_at(dir.path(), &info).unwrap();
            let value = config_at(dir.path()).unwrap();
            assert_eq!(
                value["custom_provider"]["echobird"]["models"]
                    .as_object()
                    .unwrap()
                    .len(),
                3
            );
            assert_eq!(value["defaultModel"], "custom_provider:echobird/auto");
        });
    }

    use super::*;
    #[test]
    fn capabilities_roundtrip_preserves_output_and_manual_models() {
        let dir =
            std::env::temp_dir().join(format!("echobird-minimax-caps-{}", uuid::Uuid::new_v4()));
        write_config(&dir, &json!({"theme":"dark","custom_provider":{"personal":{"keep":true},"echobird":{"api":"openai-completions","options":{"baseURL":"https://example.test/v1"},"models":{"arbitrary":{"limit":{"context":512000,"output":64000},"modalities":{"input":["text","audio"],"output":["audio"]},"custom":true},"manual":{"name":"Keep"}}}}})).unwrap();
        let mut model: ModelInfo = serde_json::from_value(json!({"model":"arbitrary","baseUrl":"https://example.test/v1/","imageInput":true,"oneMContext":true})).unwrap();
        apply_at(&dir, &model).unwrap();
        let config = config_at(&dir).unwrap();
        let entry = &config["custom_provider"]["echobird"]["models"]["arbitrary"];
        assert_eq!(entry["limit"], json!({"context":1000000,"output":64000}));
        assert_eq!(
            entry["thinking"]["effortOptions"],
            json!(["minimal", "low", "medium", "high", "xhigh", "max"])
        );
        assert_eq!(entry["reasoning"], true);
        assert_eq!(entry["attachment"], true);
        assert_eq!(entry["custom"], true);
        assert_eq!(
            entry["modalities"]["input"],
            json!(["text", "audio", "image"])
        );
        assert_eq!(entry["modalities"]["output"], json!(["audio"]));
        assert_eq!(
            config["custom_provider"]["echobird"]["models"]["manual"]["name"],
            "Keep"
        );
        assert_eq!(config["custom_provider"]["personal"]["keep"], true);
        let read = read_at(&dir).unwrap();
        assert_eq!(read.image_input, Some(true));
        assert_eq!(read.one_m_context, Some(true));
        model.image_input = Some(false);
        model.one_m_context = Some(false);
        apply_at(&dir, &model).unwrap();
        let config = config_at(&dir).unwrap();
        let entry = &config["custom_provider"]["echobird"]["models"]["arbitrary"];
        assert_eq!(entry["limit"], json!({"output":64000}));
        assert_eq!(entry["modalities"]["input"], json!(["text", "audio"]));
        assert_eq!(entry["attachment"], true);
        assert_eq!(read_at(&dir).unwrap().image_input, Some(false));
        model.base_url = Some("https://other.test/v1".into());
        model.one_m_context = Some(true);
        apply_at(&dir, &model).unwrap();
        let config = config_at(&dir).unwrap();
        let entry = &config["custom_provider"]["echobird"]["models"]["arbitrary"];
        assert_eq!(entry["limit"], json!({"context":1000000}));
        assert!(entry.get("custom").is_none());
        assert_eq!(entry["attachment"], false);
        assert_eq!(config["theme"], "dark");
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn desktop_editions_are_identified_by_release_channel_not_executable_name() {
        let root = std::env::temp_dir().join(format!(
            "echobird-minimax-editions-{}",
            uuid::Uuid::new_v4()
        ));
        for (relative, resources) in [
            ("MiniMax Code.exe", "resources"),
            (
                "MiniMax Code.app/Contents/MacOS/MiniMax Code",
                "MiniMax Code.app/Contents/Resources",
            ),
        ] {
            let exe = root.join(relative);
            let update = root.join(resources).join("app-update.yml");
            fs::create_dir_all(update.parent().unwrap()).unwrap();
            for (url, expected) in [
                (
                    "https://filecdn.minimax.chat/public/minimax-agent-prod/release",
                    Some("cn"),
                ),
                (
                    "https://filecdn.minimax.chat/public/minimax-agent/release",
                    Some("cn"),
                ),
                (
                    "https://file.cdn.minimax.io/public/minimax-agent/release",
                    Some("en"),
                ),
                (
                    "https://file.cdn.minimax.io/public/minimax-agent-prod/release",
                    Some("en"),
                ),
                ("https://example.test/public/minimax-agent/release", None),
                (
                    "https://filecdn.minimax.chat/public/minimax-agent-staging/release",
                    None,
                ),
                ("https://example.test/unknown", None),
            ] {
                fs::write(&update, format!("provider: generic\nurl: {url}\n")).unwrap();
                assert_eq!(desktop_region(&exe), expected);
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn shared_config_switches_protocols_and_preserves_user_settings() {
        let dir = std::env::temp_dir().join(format!("echobird-minimax-{}", uuid::Uuid::new_v4()));
        write_config(&dir, &json!({"theme":"dark", "defaultModelThinking":{"effort":"high"}, "custom_provider":{"personal":{"keep":true}}, "provider":{"minimax":{"models":{"native":{}}}}})).unwrap();
        for protocol in ["openai", "anthropic", "openai-responses"] {
            let model = serde_json::from_value(json!({"model":"vendor/model","baseUrl":"https://example.test/v1/","apiKey":"test-secret","protocol":protocol})).unwrap();
            apply_at(&dir, &model).unwrap();
            let config = config_at(&dir).unwrap();
            assert_eq!(
                config["defaultModel"],
                "custom_provider:echobird/vendor/model"
            );
            assert_eq!(config["theme"], "dark");
            assert_eq!(config["custom_provider"]["personal"]["keep"], true);
            assert!(config.get("defaultModelThinking").is_none());
            assert_eq!(
                read_at(&dir).unwrap().model.as_deref(),
                Some("vendor/model")
            );
        }
        let lock = FileLock::acquire(&dir.join("config.yaml")).unwrap();
        assert!(FileLock::acquire(&dir.join("config.yaml")).is_err());
        drop(lock);
        fs::write(dir.join("config.yaml"), "custom_provider: 42").unwrap();
        let model = serde_json::from_value(json!({"model":"one","baseUrl":"https://example.test"}))
            .unwrap();
        assert!(apply_at(&dir, &model).is_err());
        assert_eq!(
            fs::read_to_string(dir.join("config.yaml")).unwrap(),
            "custom_provider: 42"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
