use super::{ApplyResult, ModelInfo};
use crate::services::{native_account_files as files, process_manager, tool_manager};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const KEY: &[u8] = b"_od://app\0\x01open-design:config";
const MISSING: &str = "Open Open Design once before configuring its model in EchoBird";

struct NativePaths {
    storage: PathBuf,
    preferences: PathBuf,
}

fn packaged_config_path(executable: &Path, macos: bool) -> Option<PathBuf> {
    let parent = executable.parent()?;
    Some(if macos {
        parent.parent()?.join("Resources/open-design-config.json")
    } else {
        parent.join("resources/open-design-config.json")
    })
}

fn namespace_paths(root: &Path, namespace: &str) -> Result<NativePaths, String> {
    if namespace.is_empty()
        || namespace == "."
        || namespace == ".."
        || !namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err("Invalid Open Design namespace".into());
    }
    let root = root.join(namespace);
    Ok(NativePaths {
        storage: root.join("user-data/session/Local Storage/leveldb"),
        preferences: root.join("data/app-config.json"),
    })
}

fn native_paths() -> Result<Option<NativePaths>, String> {
    let config = tool_manager::get_tool_exe_path("opendesign")
        .and_then(|exe| packaged_config_path(Path::new(&exe), cfg!(target_os = "macos")))
        .filter(|path| path.is_file())
        .map(|path| files::object(&path))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let root = std::env::var("OD_PACKAGED_NAMESPACE_BASE_ROOT")
        .ok()
        .or_else(|| config["namespaceBaseRoot"].as_str().map(str::to_owned))
        .filter(|value| !value.trim().is_empty())
        .map(|value| tool_manager::expand_path(&value))
        .unwrap_or(
            dirs::config_dir()
                .ok_or("accountError.home")?
                .join("Open Design/namespaces"),
        );
    let mut paths = if let Some(namespace) = config["namespace"].as_str() {
        namespace_paths(&root, namespace)?
    } else {
        let root = std::env::var("OD_SIDECAR_BASE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(|value| tool_manager::expand_path(&value))
            .unwrap_or(root);
        let mut candidates = Vec::new();
        if root.is_dir() {
            for entry in fs::read_dir(&root).map_err(|_| "Cannot read Open Design profiles")? {
                let entry = entry.map_err(|_| "Cannot read Open Design profiles")?;
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                let candidate = namespace_paths(&root, name)?;
                if candidate.storage.join("CURRENT").is_file() {
                    candidates.push(candidate);
                }
            }
        }
        match candidates.len() {
            0 => return Ok(None),
            1 => candidates.remove(0),
            _ => return Err("Cannot determine the active Open Design profile".into()),
        }
    };
    if let Some(data) = std::env::var("OD_DATA_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        let data = tool_manager::expand_path(&data);
        if !data.is_absolute() {
            return Err("Open Design OD_DATA_DIR must be absolute".into());
        }
        let namespace = paths
            .preferences
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .ok_or(MISSING)?;
        paths.preferences = if data.file_name().is_some_and(|name| name == "data")
            && data.parent().and_then(Path::file_name) == Some(namespace)
        {
            data.join("app-config.json")
        } else if config["namespace"].is_string() {
            data.join("namespaces")
                .join(namespace)
                .join("data/app-config.json")
        } else {
            data.join("app-config.json")
        };
    }
    Ok(Some(paths))
}

fn open_storage(path: &Path) -> Result<rusty_leveldb::DB, String> {
    if !path.join("CURRENT").is_file() {
        return Err(MISSING.into());
    }
    rusty_leveldb::DB::open(
        path,
        rusty_leveldb::Options {
            create_if_missing: false,
            paranoid_checks: true,
            ..Default::default()
        },
    )
    .map_err(|_| {
        "Cannot open Open Design model settings; fully quit Open Design and try again".into()
    })
}

fn decode(bytes: &[u8]) -> Result<Value, String> {
    let text = match bytes.first() {
        Some(1) => bytes[1..].iter().map(|b| char::from(*b)).collect(),
        Some(0) if (bytes.len() - 1).is_multiple_of(2) => String::from_utf16(
            &bytes[1..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid Open Design model settings encoding")?,
        _ => return Err("Unsupported Open Design model settings encoding".into()),
    };
    let value: Value =
        serde_json::from_str(&text).map_err(|_| "Invalid Open Design model settings")?;
    if !value.is_object() {
        return Err("Invalid Open Design model settings".into());
    }
    Ok(value)
}

fn encode(value: &Value) -> Result<Vec<u8>, String> {
    let text =
        serde_json::to_string(value).map_err(|_| "Cannot serialize Open Design model settings")?;
    let mut bytes = vec![0];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    Ok(bytes)
}

struct Snapshot(PathBuf);
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_at(storage: &Path) -> Result<Value, String> {
    if !storage.join("CURRENT").is_file() {
        return Err(MISSING.into());
    }
    // LevelDB may recover logs when opened; passive reads only open a private copy.
    let snapshot = Snapshot(std::env::temp_dir().join(format!(
        "echobird-opendesign-model-{}",
        uuid::Uuid::new_v4()
    )));
    files::private_dir(&snapshot.0)?;
    for entry in fs::read_dir(storage).map_err(|_| MISSING)? {
        let entry = entry.map_err(|_| "Cannot read Open Design model settings")?;
        if entry.file_name() != "LOCK" && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            fs::copy(entry.path(), snapshot.0.join(entry.file_name()))
                .map_err(|_| "Cannot snapshot Open Design model settings")?;
        }
    }
    let mut db = open_storage(&snapshot.0)?;
    decode(&db.get(KEY).ok_or(MISSING)?)
}

fn apply_at(paths: &NativePaths, info: Option<&ModelInfo>) -> Result<(), String> {
    let provider = info.map(|info| {
        let protocol = info.protocol.as_deref().unwrap_or("openai");
        if !matches!(protocol, "openai" | "anthropic") { return Err("Unsupported Open Design model protocol"); }
        let model = info.model.as_deref().filter(|s| !s.trim().is_empty()).ok_or("Model ID is empty")?;
        let base = if protocol == "anthropic" { info.anthropic_url.as_deref().or(info.base_url.as_deref()) }
            else { info.base_url.as_deref() }.ok_or("Open Design API URL is empty")?.trim();
        let url = reqwest::Url::parse(base).map_err(|_| "Invalid Open Design API URL")?;
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty()
            || url.password().is_some() || url.query().is_some() || url.fragment().is_some() {
            return Err("Invalid Open Design API URL");
        }
        let key = info.api_key.as_deref().filter(|s| !s.is_empty()).unwrap_or("public");
        Ok((protocol, json!({"apiKey":key, "baseUrl":base.trim_end_matches('/'), "model":model, "apiProviderBaseUrl":base.trim_end_matches('/')})))
    }).transpose()?;
    let _lock = files::DirectoryLock::acquire(&paths.preferences)?;
    let mut db = open_storage(&paths.storage)?;
    let mut config = decode(&db.get(KEY).ok_or(MISSING)?)?;
    let mut transaction = files::Transaction::new();
    if let Some((protocol, provider)) = provider {
        let profiles = config
            .as_object_mut()
            .ok_or(MISSING)?
            .entry("apiProtocolConfigs")
            .or_insert_with(|| json!({}));
        let profiles = profiles
            .as_object_mut()
            .ok_or("Invalid Open Design API profiles")?;
        let profile = profiles.entry(protocol).or_insert_with(|| json!({}));
        let profile = profile
            .as_object_mut()
            .ok_or("Invalid Open Design API profile")?;
        for (key, value) in provider.as_object().ok_or(MISSING)? {
            profile.insert(key.clone(), value.clone());
        }
        for (key, value) in provider.as_object().ok_or(MISSING)? {
            config[key] = value.clone();
        }
        config["apiProtocol"] = json!(protocol);
        config["mode"] = json!("api");
    } else {
        let mut prefs = files::object(&paths.preferences)?;
        prefs["agentId"] = json!("amr");
        transaction.replace(&paths.preferences, &files::json(&prefs)?)?;
        config["mode"] = json!("daemon");
        config["agentId"] = json!("amr");
    }
    db.put(KEY, &encode(&config)?)
        .and_then(|_| db.flush())
        .map_err(|_| "Cannot save Open Design model settings")?;
    transaction.commit();
    Ok(())
}

pub(crate) fn restore_for_account() -> Result<(), String> {
    if let Some(paths) = native_paths()?.filter(|paths| paths.storage.join("CURRENT").is_file()) {
        apply_at(&paths, None)?;
    }
    Ok(())
}

async fn configure(info: Option<&ModelInfo>) -> ApplyResult {
    let result = async {
        if process_manager::desktop_tool_is_running("opendesign").await? {
            return Err("Fully quit Open Design before configuring its model".into());
        }
        if info.is_some() {
            apply_at(&native_paths()?.ok_or(MISSING)?, info)
        } else {
            restore_for_account()
        }
    }
    .await;
    match result {
        Ok(()) => ApplyResult {
            success: true,
            message: "Open Design model configured".into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

pub(super) async fn apply(info: &ModelInfo) -> ApplyResult {
    configure(Some(info)).await
}
pub(super) async fn restore() -> ApplyResult {
    configure(None).await
}

pub(super) fn read() -> Option<ModelInfo> {
    let config = read_at(&native_paths().ok()??.storage).ok()?;
    if config["mode"] != "api" {
        return None;
    }
    let protocol = config["apiProtocol"].as_str().unwrap_or("anthropic");
    let mut info: ModelInfo = serde_json::from_value(json!({})).ok()?;
    info.model = config["model"].as_str().map(str::to_owned);
    info.api_key = config["apiKey"].as_str().map(str::to_owned);
    info.base_url = config["baseUrl"].as_str().map(str::to_owned);
    if protocol == "anthropic" {
        info.anthropic_url = info.base_url.clone();
    }
    info.protocol = Some(protocol.into());
    Some(info)
}

#[cfg(test)]
mod tests;
