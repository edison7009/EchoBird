use super::{ApplyResult, ModelInfo};
use crate::services::{
    codearts_accounts,
    free_model_directory::{self, KeylessAuth},
    native_account_files as files, process_manager,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};

const UID: &str = "echobird-managed-model";
const SELECTION: &[u8] = b"_file://\0\x01codearts:selectedModelId";
const MISSING: &str = "Open CodeArts once before configuring its model in EchoBird";

fn parameters(info: &ModelInfo) -> Result<(&str, &str, &str, &str), String> {
    let protocol = info.protocol.as_deref().unwrap_or("openai");
    if !matches!(protocol, "openai" | "anthropic") {
        return Err("Unsupported CodeArts model protocol".into());
    }
    let model = info
        .model
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Model ID is empty")?;
    let base = if protocol == "anthropic" {
        info.anthropic_url.as_deref().or(info.base_url.as_deref())
    } else {
        info.base_url.as_deref()
    }
    .filter(|s| !s.trim().is_empty())
    .ok_or("CodeArts model URL is empty")?
    .trim_end_matches('/');
    let url = url::Url::parse(base).map_err(|_| "Invalid CodeArts model URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Invalid CodeArts model URL".into());
    }
    let key = info.api_key.as_deref().unwrap_or("");
    if key.is_empty()
        && !matches!(
            url.host_str(),
            Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
        )
        && free_model_directory::keyless_auth(base, model) != Some(KeylessAuth::Anonymous)
    {
        return Err("CodeArts API key is empty".into());
    }
    Ok((protocol, model, base, key))
}

fn storage(root: &Path) -> PathBuf {
    root.join("Local Storage/leveldb")
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
    .map_err(|_| "Cannot open CodeArts model selection; fully quit CodeArts and try again".into())
}
fn encode_selection(value: &str) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend(value.encode_utf16().flat_map(u16::to_le_bytes));
    bytes
}
fn decode_selection(bytes: &[u8]) -> Result<String, String> {
    match bytes.first() {
        Some(1) => Ok(bytes[1..].iter().map(|b| char::from(*b)).collect()),
        Some(0) if (bytes.len() - 1).is_multiple_of(2) => String::from_utf16(
            &bytes[1..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| "Invalid CodeArts model selection".into()),
        _ => Err("Invalid CodeArts model selection".into()),
    }
}
fn secret_path(root: &Path) -> PathBuf {
    root.join("secretStorage").join(format!(
        "{:x}.enc",
        Sha256::digest(format!("chat.lm.secret.{UID}"))
    ))
}

// Keeps model files and selection reversible until the update succeeds.
pub(crate) struct ModelRestore {
    transaction: Option<files::Transaction>,
    db: Option<rusty_leveldb::DB>,
    previous: Option<Vec<u8>>,
    changed: bool,
    committed: bool,
    _lock: Option<files::DirectoryLock>,
}
impl ModelRestore {
    fn new() -> Self {
        Self {
            _lock: None,
            transaction: Some(files::Transaction::new()),
            db: None,
            previous: None,
            changed: false,
            committed: false,
        }
    }
    pub(crate) fn commit(mut self) {
        self.committed = true;
        self.transaction.take().unwrap().commit();
    }
}
impl Drop for ModelRestore {
    fn drop(&mut self) {
        if self.committed || !self.changed {
            return;
        }
        if let Some(db) = self.db.as_mut() {
            let result = if let Some(previous) = &self.previous {
                db.put(SELECTION, previous)
            } else {
                db.delete(SELECTION)
            };
            if result.and_then(|_| db.flush()).is_err() {
                log::error!("Failed to restore CodeArts model selection");
            }
        }
    }
}

fn configure_desktop(root: &Path, info: Option<&ModelInfo>) -> Result<ModelRestore, String> {
    configure_desktop_with(
        root,
        info,
        codearts_accounts::native::encrypt_desktop_secret,
    )
}
fn configure_desktop_with(
    root: &Path,
    info: Option<&ModelInfo>,
    encrypt: fn(&Path, &str) -> Result<Vec<u8>, String>,
) -> Result<ModelRestore, String> {
    if let Some(info) = info {
        parameters(info)?;
    }
    let settings = root.join("app-settings.json");
    let lock = files::DirectoryLock::acquire(&settings)?;
    let mut config = files::object(&settings)?;
    if !settings.is_file() {
        return Err(MISSING.into());
    }
    if config.get("customModels").is_none() {
        config["customModels"] = json!([]);
    }
    let rows = config["customModels"]
        .as_array_mut()
        .ok_or("Invalid CodeArts custom model configuration")?;
    let had_entry = rows.iter().any(|row| row["uid"] == UID);
    rows.retain(|row| row["uid"] != UID);
    let mut guard = ModelRestore::new();
    guard._lock = Some(lock);
    guard.db = Some(open_storage(&storage(root))?);
    guard.previous = guard
        .db
        .as_mut()
        .unwrap()
        .get(SELECTION)
        .map(|bytes| bytes.to_vec());
    let current = guard
        .previous
        .as_deref()
        .map(decode_selection)
        .transpose()?;
    let own_selection = format!("custom:{UID}");
    if info.is_none()
        && !had_entry
        && !secret_path(root).exists()
        && current.as_deref() != Some(&own_selection)
    {
        return Ok(guard);
    }
    if let Some(info) = info {
        let (protocol, model, base, key) = parameters(info)?;
        let placeholder = if key.is_empty() {
            String::new()
        } else {
            format!("${{input:chat.lm.secret.{UID}}}")
        };
        rows.push(json!({"uid":UID,"modelId":model,"modelName":info.name.as_deref().unwrap_or(model),"sourceType":"custom","apiFormat":protocol,"modelUrl":base,"apiKey":placeholder,"modelType":"textConversation","inputContextWindow":184000,"outputContextWindow":16000,"enabled":true}));
        if !key.is_empty() {
            let encrypted = encrypt(root, key)?;
            guard
                .transaction
                .as_mut()
                .unwrap()
                .replace(&secret_path(root), STANDARD.encode(encrypted).as_bytes())?;
        } else {
            guard
                .transaction
                .as_mut()
                .unwrap()
                .remove(&secret_path(root))?;
        }
    } else {
        guard
            .transaction
            .as_mut()
            .unwrap()
            .remove(&secret_path(root))?;
    }
    guard
        .transaction
        .as_mut()
        .unwrap()
        .replace_private_file(&settings, &files::json(&config)?)?;
    if info.is_some() || current.as_deref() == Some(&format!("custom:{UID}")) {
        guard.changed = true;
        let db = guard.db.as_mut().unwrap();
        let result = if info.is_some() {
            db.put(SELECTION, &encode_selection(&format!("custom:{UID}")))
        } else {
            db.delete(SELECTION)
        };
        result
            .and_then(|_| db.flush())
            .map_err(|_| "Cannot save CodeArts model selection")?;
    }
    Ok(guard)
}

fn restore(tool_id: &str) -> Result<ModelRestore, String> {
    match tool_id {
        "codearts" => {
            let root = codearts_accounts::native::desktop_data_dir()?;
            if !root.join("app-settings.json").exists() {
                return Ok(ModelRestore::new());
            }
            configure_desktop(&root, None)
        }
        _ => Err("Unsupported CodeArts product".into()),
    }
}

pub(super) async fn configure(tool_id: &str, info: Option<&ModelInfo>) -> ApplyResult {
    let result = async {
        if let Some(info) = info {
            parameters(info)?;
        }
        if tool_id == "codearts" && process_manager::desktop_tool_is_running(tool_id).await? {
            return Err("Fully quit CodeArts before configuring its model".into());
        }
        let guard = match info {
            None => restore(tool_id)?,
            Some(info) if tool_id == "codearts" => {
                configure_desktop(&codearts_accounts::native::desktop_data_dir()?, Some(info))?
            }
            _ => return Err("Unsupported CodeArts product".into()),
        };
        guard.commit();
        Ok(())
    }
    .await;
    match result {
        Ok(()) => ApplyResult {
            success: true,
            message: "CodeArts model configured".into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

struct Snapshot(PathBuf);
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn desktop_selection(root: &Path) -> Result<Option<String>, String> {
    let snapshot = Snapshot(
        std::env::temp_dir().join(format!("echobird-codearts-model-{}", uuid::Uuid::new_v4())),
    );
    files::private_dir(&snapshot.0)?;
    for entry in fs::read_dir(storage(root)).map_err(|_| MISSING)? {
        let entry = entry.map_err(|_| "Cannot read CodeArts model selection")?;
        if entry.file_name() != "LOCK" && entry.file_type().is_ok_and(|kind| kind.is_file()) {
            fs::copy(entry.path(), snapshot.0.join(entry.file_name()))
                .map_err(|_| "Cannot snapshot CodeArts model selection")?;
        }
    }
    let mut db = open_storage(&snapshot.0)?;
    db.get(SELECTION)
        .map(|bytes| decode_selection(&bytes))
        .transpose()
}

fn read_desktop(root: &Path) -> Option<ModelInfo> {
    read_desktop_with(root, codearts_accounts::native::decrypt_desktop_secret)
}
fn read_desktop_with(
    root: &Path,
    decrypt: fn(&Path, &[u8]) -> Result<String, String>,
) -> Option<ModelInfo> {
    let selected = desktop_selection(root).ok()??;
    let uid = selected.strip_prefix("custom:")?;
    let config = files::object(&root.join("app-settings.json")).ok()?;
    let row = config["customModels"]
        .as_array()?
        .iter()
        .find(|row| row["uid"] == uid)?;
    let mut key = row["apiKey"].as_str()?.to_owned();
    if let Some(secret) = key
        .strip_prefix("${input:")
        .and_then(|s| s.strip_suffix('}'))
    {
        if !secret.starts_with("chat.lm.secret.") {
            return None;
        }
        let path = root
            .join("secretStorage")
            .join(format!("{:x}.enc", Sha256::digest(secret)));
        let bytes = STANDARD
            .decode(fs::read_to_string(path).ok()?.trim())
            .ok()?;
        key = decrypt(root, &bytes).ok()?;
    }
    let mut info: ModelInfo = serde_json::from_value(json!({})).ok()?;
    info.model = row["modelId"].as_str().map(str::to_owned);
    info.name = row["modelName"].as_str().map(str::to_owned);
    info.base_url = row["modelUrl"].as_str().map(str::to_owned);
    info.api_key = Some(key);
    info.protocol = row["apiFormat"].as_str().map(str::to_owned);
    if info.protocol.as_deref() == Some("anthropic") {
        info.anthropic_url = info.base_url.clone();
    }
    Some(info)
}
pub(super) fn read(tool_id: &str) -> Option<ModelInfo> {
    match tool_id {
        "codearts" => read_desktop(&codearts_accounts::native::desktop_data_dir().ok()?),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
