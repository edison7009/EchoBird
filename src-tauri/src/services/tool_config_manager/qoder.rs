use super::{jsonc::set, ApplyResult, ModelInfo};
use crate::services::qoder_accounts::Edition;
use crate::services::{
    native_account_files::{DirectoryLock, Transaction},
    process_manager,
    qoder_native::{
        self,
        desktop::{self, ProductKind},
    },
    tool_manager,
};
use jsonc_parser::{
    cst::{CstInputValue, CstRootNode},
    ParseOptions,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const PROVIDER: &str = "echobird";
const MISSING_STORAGE: &str = "Open Qoder once before configuring its model in EchoBird";

fn edition(tool_id: &str) -> Result<Edition, String> {
    match tool_id {
        "qoder" => Ok(Edition::Qoder),
        "qodercn" => Ok(Edition::QoderCn),
        _ => Err("Unsupported Qoder edition".into()),
    }
}

fn settings_path(edition: Edition) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(if edition == Edition::Qoder {
            ".qoder"
        } else {
            ".qoder-cn"
        })
        .join("settings.json"))
}

fn storage_path(edition: Edition) -> Result<PathBuf, String> {
    let name = if edition == Edition::Qoder {
        "com.qoder.app.stable"
    } else {
        "com.qodercn.app.stable"
    };
    let root = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .ok_or("accountError.home")?
            .join("Library/Application Support")
    } else {
        dirs::config_dir().ok_or("accountError.home")?
    };
    Ok(root.join(name).join("Local Storage/leveldb"))
}

fn provider(info: &ModelInfo) -> Result<(Value, String), String> {
    let model = info
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .ok_or("Model ID is empty")?;
    let protocol = match info.protocol.as_deref().unwrap_or("openai") {
        "anthropic" => "anthropic",
        "openai" => "openai",
        _ => return Err("Unsupported Qoder model protocol".into()),
    };
    let base = if protocol == "anthropic" {
        info.anthropic_url.as_deref().or(info.base_url.as_deref())
    } else {
        info.base_url.as_deref()
    }
    .filter(|url| !url.trim().is_empty())
    .ok_or("Qoder API URL is empty")?;
    let url = reqwest::Url::parse(base).map_err(|_| "Invalid Qoder API URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid Qoder API URL".into());
    }
    let mut entry = json!({
        "displayName": "EchoBird",
        "type": "openai-compatible",
        "protocol": protocol,
        "baseUrl": base.trim_end_matches('/'),
        "apiKey": info.api_key.as_deref().unwrap_or(""),
        "model": model,
        "models": [{
            "model": model,
            "displayName": info.name.as_deref().filter(|name| !name.is_empty()).unwrap_or(model),
            "capabilities": {
                "tools": true,
                "vision": info.image_input.unwrap_or(false),
                "thinking": {
                    "modes": [if protocol == "anthropic" { "adaptive" } else { "enabled" }],
                    "supportsEffort": true,
                    "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]
                }
            }
        }]
    });
    if info.one_m_context == Some(true) {
        entry["models"][0]["contextWindow"] = json!(1_000_000);
    }
    Ok((entry, model.to_owned()))
}

fn settings_bytes(path: &Path, info: &ModelInfo) -> Result<(Vec<u8>, String), String> {
    let previous = fs::read(path).map_err(|_| "Cannot read Qoder settings")?;
    let text = std::str::from_utf8(&previous).map_err(|_| "Invalid Qoder settings encoding")?;
    let options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        ..Default::default()
    };
    let root = CstRootNode::parse(text.trim_start_matches('\u{feff}'), &options)
        .map_err(|_| "Invalid Qoder settings")?;
    let settings = root.object_value().ok_or("Unsupported Qoder settings")?;
    let providers = match settings.get("providers") {
        Some(value) => value
            .object_value()
            .ok_or("Unsupported Qoder provider settings")?,
        None => {
            settings.append("providers", CstInputValue::Object(vec![]));
            settings
                .get("providers")
                .and_then(|value| value.object_value())
                .ok_or("Cannot create Qoder provider settings")?
        }
    };
    let existing = if providers.get(PROVIDER).is_some() {
        let value = providers
            .get(PROVIDER)
            .and_then(|value| value.to_serde_value())
            .ok_or("Invalid EchoBird provider")?;
        if value["displayName"] != "EchoBird" {
            return Err("Qoder provider ID is already in use".into());
        }
        Some(value)
    } else {
        None
    };
    let (mut entry, model) = provider(info)?;
    if let Some(previous) = existing.filter(|previous| {
        previous["protocol"] == entry["protocol"]
            && previous["baseUrl"]
                .as_str()
                .map(|url| url.trim_end_matches('/'))
                == entry["baseUrl"].as_str()
    }) {
        if let Some(previous_model) = previous["models"]
            .as_array()
            .and_then(|models| models.iter().find(|value| value["model"] == model))
        {
            let mut fields = previous_model.as_object().cloned().unwrap_or_default();
            let mut capabilities = previous_model["capabilities"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            let mut updated = entry["models"][0].as_object().cloned().unwrap();
            capabilities.extend(
                updated
                    .remove("capabilities")
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            if info.image_input.is_none() {
                if let Some(vision) = previous_model["capabilities"]["vision"].as_bool() {
                    capabilities.insert("vision".into(), json!(vision));
                }
            }
            fields.extend(updated);
            fields.insert("capabilities".into(), json!(capabilities));
            if info.one_m_context == Some(false)
                && fields.get("contextWindow").and_then(Value::as_u64) == Some(1_000_000)
            {
                fields.remove("contextWindow");
            }
            entry["models"][0] = json!(fields);
        }
    }
    super::switching::model_array(info, &mut entry["models"], "model", "displayName")?;
    set(&providers, PROVIDER, &entry);
    let model_settings = match settings.get("model") {
        Some(value) => value
            .object_value()
            .ok_or("Unsupported Qoder model settings")?,
        None => {
            settings.append("model", CstInputValue::Object(vec![]));
            settings
                .get("model")
                .and_then(|value| value.object_value())
                .ok_or("Cannot create Qoder model settings")?
        }
    };
    set(
        &model_settings,
        "name",
        &json!(format!("{PROVIDER}/{model}")),
    );
    let output = root.to_string();
    let bytes = if previous.starts_with(b"\xef\xbb\xbf") {
        format!("\u{feff}{output}").into_bytes()
    } else {
        output.into_bytes()
    };
    Ok((bytes, model))
}

fn selection_key(edition: Edition, user_id: &str) -> Vec<u8> {
    let origin = if edition == Edition::Qoder {
        "qoder-app"
    } else {
        "qoder-cn-app"
    };
    format!("_{origin}://renderer\0\x01qoder:chat-composer:selected-model:account:{user_id}")
        .into_bytes()
}

fn selection_bytes(model: &str) -> Result<Vec<u8>, String> {
    let encoded: String = url::form_urlencoded::byte_serialize(model.as_bytes()).collect();
    let selection = serde_json::to_string(&format!("byok:{PROVIDER}/{encoded}"))
        .map_err(|_| "Invalid Qoder model selection")?;
    let mut bytes = vec![0];
    bytes.extend(selection.encode_utf16().flat_map(u16::to_le_bytes));
    Ok(bytes)
}

fn apply_at(
    settings: &Path,
    storage: &Path,
    edition: Edition,
    user_id: &str,
    info: &ModelInfo,
) -> Result<(), String> {
    if !storage.join("CURRENT").is_file() {
        return Err(MISSING_STORAGE.into());
    }
    let _lock = DirectoryLock::acquire(settings)?;
    let (settings_bytes, model) = settings_bytes(settings, info)?;
    let key = selection_key(edition, user_id);
    let value = selection_bytes(&model)?;
    let mut db = rusty_leveldb::DB::open(
        storage,
        rusty_leveldb::Options {
            create_if_missing: false,
            paranoid_checks: true,
            ..Default::default()
        },
    )
    .map_err(|_| "Cannot open Qoder model storage; fully quit Qoder and try again")?;
    let mut transaction = Transaction::new();
    transaction.replace(settings, &settings_bytes)?;
    db.put(&key, &value)
        .and_then(|_| db.flush())
        .map_err(|_| "Cannot save Qoder model selection")?;
    transaction.commit();
    Ok(())
}

pub(super) async fn apply(tool_id: &str, info: &ModelInfo) -> ApplyResult {
    let result = async {
        let edition = edition(tool_id)?;
        let executable =
            tool_manager::get_tool_exe_path(tool_id).ok_or("accountError.unavailable")?;
        if desktop::identity(Path::new(&executable)) != Some((edition, ProductKind::App)) {
            return Err("Qoder desktop app is required for model configuration".into());
        }
        if process_manager::desktop_tool_is_running(tool_id).await? {
            return Err("Fully quit Qoder before configuring its model".into());
        }
        let user_id = qoder_native::active_app_user_id(edition, Path::new(&executable))?;
        apply_at(
            &settings_path(edition)?,
            &storage_path(edition)?,
            edition,
            &user_id,
            info,
        )
    }
    .await;
    match result {
        Ok(()) => ApplyResult {
            success: true,
            message: "Qoder model configured".into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smart_switch_native_catalog_regression() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let info = super::super::switching::test_info();
            let dir = super::super::switching::TestDir::new();
            fs::write(dir.path().join("settings.json"), "{}").unwrap();
            let (bytes, selected) =
                settings_bytes(&dir.path().join("settings.json"), &info).unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(selected, "auto");
            assert_eq!(
                value["providers"][PROVIDER]["models"]
                    .as_array()
                    .unwrap()
                    .len(),
                3
            );
            assert_eq!(
                value["providers"][PROVIDER]["models"][1]["displayName"],
                "First model"
            );
        });
    }

    use super::*;
    use std::io::Write;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("echobird-qoder-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn info() -> ModelInfo {
        ModelInfo {
            name: Some("Example".into()),
            model: Some("vendor/model".into()),
            base_url: Some("https://example.test/v1".into()),
            api_key: Some("key".into()),
            anthropic_url: None,
            protocol: Some("openai".into()),
            display_model: None,
            relay_mode: None,
            web_search: None,
            image_input: None,
            one_m_context: None,
        }
    }

    #[test]
    fn context_and_image_overrides_preserve_native_model_settings() {
        let dir = Fixture::new();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"model":{"reasoningEffort":"high"},"providers":{"other":{"keep":true},"echobird":{"displayName":"EchoBird","baseUrl":"https://example.test/v1","protocol":"openai","models":[{"model":"vendor/model","contextWindow":400000,"maxOutputTokens":64000,"custom":true,"capabilities":{"vision":false,"other":true}}]}}}"#).unwrap();
        let mut model = info();
        model.image_input = Some(true);
        model.one_m_context = Some(true);
        let (bytes, _) = settings_bytes(&path, &model).unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &state["providers"][PROVIDER]["models"][0];
        assert_eq!(entry["contextWindow"], 1_000_000);
        assert_eq!(entry["maxOutputTokens"], 64000);
        assert_eq!(entry["capabilities"]["vision"], true);
        assert_eq!(entry["capabilities"]["other"], true);
        assert_eq!(entry["custom"], true);
        assert_eq!(state["model"]["reasoningEffort"], "high");
        assert_eq!(state["providers"]["other"]["keep"], true);
        fs::write(&path, bytes).unwrap();
        model.image_input = None;
        model.one_m_context = None;
        let (bytes, _) = settings_bytes(&path, &model).unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            state["providers"][PROVIDER]["models"][0]["contextWindow"],
            1_000_000
        );
        assert_eq!(
            state["providers"][PROVIDER]["models"][0]["capabilities"]["vision"],
            true
        );
        model.image_input = Some(false);
        model.one_m_context = Some(false);
        let (bytes, _) = settings_bytes(&path, &model).unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &state["providers"][PROVIDER]["models"][0];
        assert!(entry.get("contextWindow").is_none());
        assert_eq!(entry["maxOutputTokens"], 64000);
        assert_eq!(entry["capabilities"]["vision"], false);
        model.base_url = Some("https://other.test/v1".into());
        model.one_m_context = Some(true);
        let (bytes, _) = settings_bytes(&path, &model).unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        let entry = &state["providers"][PROVIDER]["models"][0];
        assert_eq!(entry["contextWindow"], 1_000_000);
        assert!(entry.get("maxOutputTokens").is_none());
        assert!(entry.get("custom").is_none());
    }

    #[test]
    fn declares_native_thinking_capabilities_for_both_protocols() {
        for (protocol, mode) in [("openai", "enabled"), ("anthropic", "adaptive")] {
            for model_id in ["vendor/model", "deepseek-flash", "another-model"] {
                let mut model = info();
                model.protocol = Some(protocol.into());
                model.model = Some(model_id.into());
                let (entry, _) = provider(&model).unwrap();
                assert_eq!(
                    entry["models"][0]["capabilities"]["thinking"],
                    json!({
                        "modes": [mode],
                        "supportsEffort": true,
                        "supportedEffortLevels": ["low", "medium", "high", "xhigh", "max"]
                    })
                );
                assert_eq!(entry["models"][0]["capabilities"]["tools"], true);
                assert_eq!(entry["models"][0]["capabilities"]["vision"], false);
            }
        }
    }

    #[test]
    fn preserves_other_settings_and_comments() {
        let dir = Fixture::new();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            "{\n  // keep this\n  \"model\": {\"reasoningEffort\": \"high\"},\n  \"providers\": {\"other\": {\"id\": 1}},\n}\n",
        )
        .unwrap();
        let (bytes, model) = settings_bytes(&path, &info()).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert_eq!(model, "vendor/model");
        assert!(text.contains("// keep this"));
        assert!(text.contains("\"other\""));
        let state =
            jsonc_parser::parse_to_serde_value::<Value>(&text, &Default::default()).unwrap();
        assert_eq!(state["model"]["name"], "echobird/vendor/model");
        assert_eq!(state["model"]["reasoningEffort"], "high");
        assert_eq!(state["providers"]["other"], json!({"id": 1}));
        assert_eq!(
            state["providers"]["echobird"]["models"][0]["model"],
            "vendor/model"
        );
    }

    #[test]
    fn writes_only_selected_edition_and_rolls_back_missing_storage() {
        let dir = Fixture::new();
        let settings = dir.path().join("settings.json");
        let original = b"{\"providers\":{},\"model\":{}}";
        fs::write(&settings, original).unwrap();
        let storage = dir.path().join("leveldb");
        fs::create_dir(&storage).unwrap();
        let mut db = rusty_leveldb::DB::open(&storage, Default::default()).unwrap();
        db.put(b"seed", b"value").unwrap();
        db.flush().unwrap();
        drop(db);
        apply_at(&settings, &storage, Edition::QoderCn, "user", &info()).unwrap();
        let state: Value = serde_json::from_slice(&fs::read(&settings).unwrap()).unwrap();
        assert_eq!(
            state["providers"]["echobird"]["models"][0]["capabilities"]["thinking"]["modes"],
            json!(["enabled"])
        );
        let mut db = rusty_leveldb::DB::open(&storage, Default::default()).unwrap();
        assert!(db.get(&selection_key(Edition::QoderCn, "user")).is_some());
        assert!(db.get(&selection_key(Edition::Qoder, "user")).is_none());
        drop(db);
        let mut file = fs::File::create(&settings).unwrap();
        file.write_all(original).unwrap();
        assert!(apply_at(
            &settings,
            &dir.path().join("missing"),
            Edition::QoderCn,
            "user",
            &info()
        )
        .is_err());
        assert_eq!(fs::read(&settings).unwrap(), original);
    }
}
