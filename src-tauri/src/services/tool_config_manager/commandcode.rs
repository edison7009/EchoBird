//! Command Code's native provider registry and selected-model settings.
use super::{ApplyResult, ModelInfo};
use crate::services::commandcode_accounts::{native, read_object};
use crate::services::native_account_files::{json, Transaction};
use serde_json::{json, Value};
use std::path::PathBuf;
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn host_file(name: &str) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird")
        .join(name))
}
pub fn apply(info: &ModelInfo) -> ApplyResult {
    match apply_inner(info) {
        Ok(()) => ApplyResult {
            success: true,
            message: "Command Code model configured.".into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}
fn apply_inner(info: &ModelInfo) -> Result<(), String> {
    let _lock = CONFIG_LOCK.lock().map_err(|_| "accountError.failed")?;
    let model = info
        .model
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or("Select a model first.")?;
    let anthropic = info.protocol.as_deref() == Some("anthropic");
    let base = if anthropic {
        info.anthropic_url.as_deref().or(info.base_url.as_deref())
    } else {
        info.base_url.as_deref()
    }
    .ok_or("Model endpoint unavailable.")?;
    let url = url::Url::parse(base).map_err(|_| "Invalid model endpoint.")?;
    if !matches!(url.scheme(), "https" | "http") {
        return Err("Invalid model endpoint.".into());
    }
    let settings_path = native("settings.json")?;
    let providers_path = native("providers.json")?;
    let mut settings = read_object(&settings_path)?;
    let mut providers = read_object(&providers_path)?;
    if providers.get("provider").is_some_and(|p| !p.is_object()) {
        return Err("Invalid Command Code providers.".into());
    }
    let backup = host_file("commandcode-model-previous.json")?;
    let mut transaction = Transaction::guarded();
    if settings["modelProvider"] != "echobird" && !backup.exists() {
        transaction.replace(
            &backup,
            &json(&json!({"model":settings.get("model"),"modelProvider":settings.get("modelProvider")}))?,
        )?;
    }
    let key_path = host_file("commandcode-model-key")?;
    let key = crate::services::model_manager::decrypt_key_for_use(
        info.api_key.as_deref().unwrap_or_default(),
    );
    let key_command = if key.is_empty() {
        Value::Bool(false)
    } else {
        transaction.replace(&key_path, key.as_bytes())?;
        #[cfg(windows)]
        let command = format!(
            "!powershell -NoProfile -NonInteractive -Command \"[IO.File]::ReadAllText('{}')\"",
            key_path.to_string_lossy().replace('\'', "''")
        );
        #[cfg(not(windows))]
        let command = format!(
            "!cat '{}'",
            key_path.to_string_lossy().replace('\'', "'\\''")
        );
        Value::String(command)
    };
    if !providers["provider"].is_object() {
        providers["provider"] = json!({});
    }
    providers["provider"]["echobird"] = json!({"name":"EchoBird","api":if anthropic{"anthropic-messages"}else{"openai-completions"},"baseURL":base,"apiKey":key_command,"models":{model:{"name":info.name.as_deref().unwrap_or(model)}}});
    settings["model"] = format!("echobird/{model}").into();
    settings["modelProvider"] = "echobird".into();
    transaction.replace(&providers_path, &json(&providers)?)?;
    transaction.replace(&settings_path, &json(&settings)?)?;
    if key.is_empty() {
        transaction.remove(&key_path)?;
    }
    transaction.commit();
    Ok(())
}
pub fn restore() -> Result<(), String> {
    let _lock = CONFIG_LOCK.lock().map_err(|_| "accountError.failed")?;
    let settings_path = native("settings.json")?;
    let providers_path = native("providers.json")?;
    let mut settings = read_object(&settings_path)?;
    let mut providers = read_object(&providers_path)?;
    let backup_path = host_file("commandcode-model-previous.json")?;
    let previous = read_object(&backup_path)?;
    if let Some(provider) = providers["provider"].as_object_mut() {
        provider.remove("echobird");
    }
    if settings["modelProvider"] == "echobird"
        || settings["model"]
            .as_str()
            .is_some_and(|s| s.starts_with("echobird/"))
    {
        for field in ["model", "modelProvider"] {
            if previous[field].is_null() {
                settings.as_object_mut().expect("validated").remove(field);
            } else {
                settings[field] = previous[field].clone();
            }
        }
    }
    let mut transaction = Transaction::guarded();
    transaction.replace(&providers_path, &json(&providers)?)?;
    transaction.replace(&settings_path, &json(&settings)?)?;
    for path in [backup_path, host_file("commandcode-model-key")?] {
        transaction.remove(&path)?;
    }
    transaction.commit();
    Ok(())
}
pub fn read() -> Option<ModelInfo> {
    let settings = read_object(&native("settings.json").ok()?).ok()?;
    let model = settings["model"].as_str()?;
    if settings["modelProvider"] != "echobird" {
        return serde_json::from_value(json!({"model":model})).ok();
    }
    let providers = read_object(&native("providers.json").ok()?).ok()?;
    let provider = &providers["provider"]["echobird"];
    let model = model.strip_prefix("echobird/")?;
    let anthropic = provider["api"] == "anthropic-messages";
    serde_json::from_value(json!({"model":model,"name":provider["models"][model]["name"],"baseUrl":if anthropic{Value::Null}else{provider["baseURL"].clone()},"anthropicUrl":if anthropic{provider["baseURL"].clone()}else{Value::Null},"protocol":if anthropic{"anthropic"}else{"openai"}})).ok()
}
