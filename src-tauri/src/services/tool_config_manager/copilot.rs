//! Copilot CLI's documented BYOK registry. Desktop has a separate native store.
//! https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference
use super::{echobird_dir, write_json_file, ApplyResult, ModelInfo};
use serde_json::{json, Value};
use std::{fs, path::Path, path::PathBuf, sync::Mutex};

const PROVIDER: &str = "EchoBird";
const MODEL: &str = "selected";
const SELECTION: &str = "EchoBird/selected";
static CONFIG_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn config_dir() -> PathBuf {
    std::env::var_os("COPILOT_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".copilot"))
}

fn registry_path(dir: &Path) -> PathBuf {
    std::env::var_os("COPILOT_PROVIDERS_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("providers.json"))
}

fn read_object(path: &Path) -> Result<Value, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(_) => return Err("accountError.read".into()),
    };
    let value = jsonc_parser::parse_to_serde_value::<Value>(&content, &Default::default())
        .map_err(|_| "accountError.format")?;
    if !value.is_object() {
        return Err("accountError.format".into());
    }
    Ok(value)
}

fn owned_entries(registry: &mut Value) -> Result<(), String> {
    for key in ["providers", "models"] {
        if registry.get(key).is_none() {
            registry[key] = json!([]);
        }
        let entries = registry[key].as_array_mut().ok_or("accountError.format")?;
        entries.retain(|entry| {
            if key == "providers" {
                entry["name"] != PROVIDER
            } else {
                entry["provider"] != PROVIDER
            }
        });
    }
    Ok(())
}

fn stored_model_info(model: &ModelInfo) -> Result<Value, String> {
    let mut stored = serde_json::to_value(model).map_err(|_| "accountError.format")?;
    if let Some(key) = model
        .api_key
        .as_deref()
        .filter(|key| !key.is_empty() && *key != "local")
    {
        let protected = super::super::model_manager::encrypt_key_for_storage(key);
        if !protected.starts_with("enc:v1:") {
            return Err("accountError.keychain".into());
        }
        stored["apiKey"] = json!(protected);
    }
    Ok(stored)
}

fn restored_model_info(stored: &Value) -> Result<ModelInfo, String> {
    let mut model: ModelInfo =
        serde_json::from_value(stored.clone()).map_err(|_| "accountError.format")?;
    if let Some(key) = model.api_key.as_mut() {
        if let Some(encoded) = key.strip_prefix("enc:v1:") {
            if encoded.len() % 2 != 0 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("accountError.keychain".into());
            }
            *key = super::super::model_manager::decrypt_key_for_use(key);
            if key.is_empty() {
                return Err("accountError.keychain".into());
            }
        }
    }
    Ok(model)
}

fn apply_at(
    registry_path: &Path,
    settings_path: &Path,
    snapshot_path: &Path,
    model: &ModelInfo,
) -> Result<(), String> {
    let model_id = model
        .model
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("Model ID is empty.")?;
    let base_url = model
        .base_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .ok_or("Provider URL is empty.")?;
    let url = url::Url::parse(base_url).map_err(|_| "Invalid provider URL.")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Invalid provider URL.".into());
    }
    let mut registry = read_object(registry_path)?;
    let mut settings = read_object(settings_path)?;
    let mut snapshot = read_object(snapshot_path)?;
    owned_entries(&mut registry)?;
    let provider_type = if model.protocol.as_deref() == Some("anthropic") {
        "anthropic"
    } else {
        "openai"
    };
    let mut provider =
        json!({"name":PROVIDER,"type":provider_type,"baseUrl":base_url.trim_end_matches('/')});
    if provider_type == "openai" {
        provider["wireApi"] = json!("completions");
    }
    if let Some(key) = model.api_key.as_deref().filter(|key| !key.is_empty()) {
        provider["apiKey"] = json!(key);
    }
    registry["providers"].as_array_mut().unwrap().push(provider);
    registry["models"].as_array_mut().unwrap().push(json!({
        "id":MODEL,"provider":PROVIDER,"wireModel":model_id,
        "name":model.name.as_deref().unwrap_or(model_id)
    }));
    for entry in super::switching::models(model)?
        .into_iter()
        .filter(|entry| entry.id != model_id)
    {
        registry["models"].as_array_mut().unwrap().push(json!({
            "id":entry.id,"provider":PROVIDER,"wireModel":entry.id,"name":entry.name
        }));
    }
    if snapshot.get("previousModel").is_none() {
        snapshot["previousModel"] = settings.get("model").cloned().unwrap_or(Value::Null);
    }
    snapshot["modelInfo"] = stored_model_info(model)?;
    settings["model"] = json!(SELECTION);
    let read = |path: &Path| match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("accountError.read".to_owned()),
    };
    let before = [snapshot_path, registry_path, settings_path]
        .into_iter()
        .map(|path| read(path).map(|bytes| (path, bytes)))
        .collect::<Result<Vec<_>, _>>()?;
    let result = (|| {
        // Keep the recovery record before either native write. Re-applying retains it.
        super::super::cursor_auth::write(snapshot_path, &snapshot)?;
        super::super::cursor_auth::write(registry_path, &registry)?;
        write_json_file(settings_path, &settings)
    })();
    if result.is_err() {
        let mut failed = false;
        for (path, bytes) in before {
            if read(path).is_ok_and(|current| current == bytes) {
                continue;
            }
            let restored = match bytes {
                Some(bytes) => fs::write(path, bytes),
                None => fs::remove_file(path),
            };
            failed |= restored.is_err();
        }
        if failed {
            return Err("accountError.write".into());
        }
    }
    result
}

fn restore_at(
    registry_path: &Path,
    settings_path: &Path,
    snapshot_path: &Path,
) -> Result<(), String> {
    if !snapshot_path.exists() {
        return Ok(());
    }
    let snapshot = read_object(snapshot_path)?;
    let mut registry = read_object(registry_path)?;
    let mut settings = read_object(settings_path)?;
    owned_entries(&mut registry)?;
    if settings["model"] == SELECTION
        || settings["model"].as_str().is_some_and(|id| {
            id.starts_with(&format!(
                "{PROVIDER}/{}",
                crate::services::smart_router::catalog::SELECTOR_PREFIX
            ))
        })
    {
        if snapshot["previousModel"].is_null() {
            settings.as_object_mut().unwrap().remove("model");
        } else {
            settings["model"] = snapshot["previousModel"].clone();
        }
    }
    super::super::cursor_auth::write(registry_path, &registry)?;
    write_json_file(settings_path, &settings)?;
    fs::remove_file(snapshot_path).map_err(|_| "accountError.write".into())
}

pub(super) fn apply_copilot(model: &ModelInfo) -> ApplyResult {
    let result = CONFIG_LOCK
        .lock()
        .map_err(|_| "accountError.busy".into())
        .and_then(|_guard| {
            let dir = config_dir();
            apply_at(
                &registry_path(&dir),
                &dir.join("settings.json"),
                &echobird_dir().join("copilot-model.json"),
                model,
            )
        });
    outcome(result, "Model applied to GitHub Copilot CLI.")
}

pub(crate) fn restore_copilot_to_official() -> ApplyResult {
    let result = CONFIG_LOCK
        .lock()
        .map_err(|_| "accountError.busy".into())
        .and_then(|_guard| {
            let dir = config_dir();
            restore_at(
                &registry_path(&dir),
                &dir.join("settings.json"),
                &echobird_dir().join("copilot-model.json"),
            )
        });
    outcome(result, "GitHub Copilot CLI restored to its previous model.")
}

pub(super) fn read_copilot() -> Option<ModelInfo> {
    let dir = config_dir();
    read_copilot_at(
        &dir.join("settings.json"),
        &echobird_dir().join("copilot-model.json"),
    )
}

fn read_copilot_at(settings_path: &Path, snapshot_path: &Path) -> Option<ModelInfo> {
    let selected = read_object(settings_path).ok()?["model"]
        .as_str()?
        .to_string();
    if selected != SELECTION
        && !selected.starts_with(&format!(
            "{PROVIDER}/{}",
            crate::services::smart_router::catalog::SELECTOR_PREFIX
        ))
    {
        return None;
    }
    let snapshot = read_object(snapshot_path).ok()?;
    restored_model_info(&snapshot["modelInfo"]).ok()
}

fn outcome(result: Result<(), String>, success: &str) -> ApplyResult {
    match result {
        Ok(()) => ApplyResult {
            success: true,
            message: success.into(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

fn desktop_snapshot_path() -> PathBuf {
    echobird_dir().join("copilotdesktop-model.json")
}

#[cfg(test)]
fn trace_live_result<T>(stage: &str, result: &Result<T, String>) {
    if std::env::var("ECHOBIRD_LIVE_COPILOT_DESKTOP_APPLY")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }
    let code = match result {
        Ok(_) => "ok",
        Err(error) => match error.as_str() {
            "accountError.initializeClient"
            | "accountError.closeClient"
            | "accountError.read"
            | "accountError.write"
            | "accountError.keychain"
            | "accountError.expired"
            | "accountError.network"
            | "accountError.format" => error,
            _ => "unclassified",
        },
    };
    eprintln!("Copilot native probe {stage}: {code}");
}

fn desktop_payload(provider_id: &str, model: &ModelInfo) -> Result<(Value, Value), String> {
    let model_id = model
        .model
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("accountError.format")?;
    let base_url = model
        .base_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .ok_or("accountError.format")?;
    let url = url::Url::parse(base_url).map_err(|_| "accountError.format")?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("accountError.format".into());
    }
    let key = model.api_key.as_deref().filter(|key| !key.is_empty());
    let provider = json!({"type":"upsert_model_provider","provider":{"id":provider_id,"name":PROVIDER,
        "kind":if model.protocol.as_deref()==Some("anthropic"){"anthropic"}else{"openai"},
        "settings":{"baseUrl":base_url.trim_end_matches('/'),"wireApi":"completions","authKind":if key.is_some(){"api_key"}else{"none"},"headersJson":"{}"}},
        "secret":key.map(|value|json!({"kind":"apiKey","value":value}))});
    let selected = json!({"type":"upsert_provider_model","model":{"providerId":provider_id,"modelId":MODEL,
        "wireModel":model_id,"displayName":model.name.as_deref().unwrap_or(model_id)}});
    Ok((provider, selected))
}

fn desktop_model_entries(provider_id: &str, model: &ModelInfo) -> Result<Vec<Value>, String> {
    let (_, selected) = desktop_payload(provider_id, model)?;
    let mut entries = vec![selected["model"].clone()];
    for entry in super::switching::models(model)? {
        if Some(entry.id.as_str()) != model.model.as_deref() {
            entries.push(json!({"providerId":provider_id,"modelId":entry.id,
                "wireModel":entry.id,"displayName":entry.name}));
        }
    }
    Ok(entries)
}

fn desktop_entries(connection: &rusqlite::Connection, id: &str) -> Result<Vec<Value>, String> {
    let mut statement = connection.prepare(
        "SELECT id, model_id, wire_model, display_name FROM provider_models WHERE provider_id=?1 ORDER BY model_id",
    ).map_err(|_| "accountError.read")?;
    let rows = statement
        .query_map([id], |row| {
            Ok(json!({"id":row.get::<_,String>(0)?,"providerId":id,
            "modelId":row.get::<_,String>(1)?,"wireModel":row.get::<_,String>(2)?,
            "displayName":row.get::<_,String>(3)?}))
        })
        .map_err(|_| "accountError.read")?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "accountError.read".into())
}

async fn write_desktop_models(
    desktop: &mut super::super::copilot_accounts::desktop::Desktop,
    connection: &mut rusqlite::Connection,
    id: &str,
    model: &ModelInfo,
    entries: &[Value],
) -> Result<(), String> {
    let (provider, _) = desktop_payload(id, model)?;
    let old = desktop_entries(connection, id)?;
    let response = desktop.request(provider, "model_provider_upserted").await?;
    if response["provider"]["id"] != id
        || (model.api_key.as_deref().is_some_and(|key| !key.is_empty())
            && response["provider"]["hasSecret"] != true)
    {
        return Err("accountError.write".into());
    }
    for entry in entries {
        let mut entry = entry.clone();
        let row_id = old
            .iter()
            .find(|old| old["modelId"] == entry["modelId"])
            .and_then(|old| old["id"].as_str())
            .map(str::to_owned)
            .or_else(|| entry["id"].as_str().map(str::to_owned))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        entry["id"] = json!(row_id);
        let response = desktop
            .request(
                json!({"type":"upsert_provider_model","model":entry}),
                "provider_model_upserted",
            )
            .await?;
        if response["model"]["providerId"] != id
            || response["model"]["modelId"] != entry["modelId"]
            || response["model"]["id"] != row_id
        {
            return Err("accountError.write".into());
        }
    }
    for stale in old.iter().filter(|old| {
        !entries
            .iter()
            .any(|entry| entry["modelId"] == old["modelId"])
    }) {
        let response = desktop
            .request(
                json!({"type":"delete_provider_model","model_id":stale["id"]}),
                "provider_model_deleted",
            )
            .await?;
        if response["model_id"] != stale["id"] {
            return Err("accountError.write".into());
        }
    }
    Ok(())
}

fn desktop_owns_selection(provider_id: &str, selection: &str) -> bool {
    selection == format!("{provider_id}/{MODEL}")
        || selection
            .strip_prefix(&format!("{provider_id}/"))
            .is_some_and(|model| {
                model.starts_with(crate::services::smart_router::catalog::SELECTOR_PREFIX)
            })
}

async fn delete_desktop_provider(
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
    id: &str,
) -> Result<(), String> {
    let response = bridge
        .request(
            json!({"type":"list_model_providers"}),
            "model_providers_list",
        )
        .await?;
    let providers = response["providers"]
        .as_array()
        .ok_or("accountError.format")?;
    let Some(provider) = providers.iter().find(|provider| provider["id"] == id) else {
        return Ok(());
    };
    if provider["name"] != PROVIDER {
        return Err("accountError.format".into());
    }
    let response = bridge
        .request(
            json!({"type":"delete_model_provider","provider_id":id}),
            "model_provider_deleted",
        )
        .await?;
    if response["provider_id"] != id {
        return Err("accountError.write".into());
    }
    Ok(())
}

pub(super) async fn apply_copilot_desktop(model: &ModelInfo) -> ApplyResult {
    outcome(
        apply_desktop_at(model, &desktop_snapshot_path()).await,
        "Model applied to GitHub Copilot Desktop.",
    )
}

async fn apply_desktop_at(model: &ModelInfo, path: &Path) -> Result<(), String> {
    desktop_payload("validation", model)?;
    let starting = super::super::copilot_accounts::desktop::WriteSession::start().await;
    #[cfg(test)]
    trace_live_result("apply.initialize", &starting);
    let mut session = starting?;
    let result = apply_desktop_with_bridge(model, path, &mut session.bridge).await;
    #[cfg(test)]
    trace_live_result("apply.native_write", &result);
    let cleanup = session.finish();
    #[cfg(test)]
    trace_live_result("apply.owned_cleanup", &cleanup);
    result.and(cleanup)
}

async fn apply_desktop_with_bridge(
    model: &ModelInfo,
    path: &Path,
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
) -> Result<(), String> {
    let mut connection = super::super::copilot_accounts::desktop::database()?;
    apply_desktop_with_connection(model, path, bridge, &mut connection).await
}

async fn apply_desktop_with_connection(
    model: &ModelInfo,
    path: &Path,
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
    connection: &mut rusqlite::Connection,
) -> Result<(), String> {
    use super::super::copilot_accounts::desktop;
    let had_snapshot = path.exists();
    let mut snapshot = read_object(path)?;
    let original = snapshot.clone();
    let id = snapshot["providerId"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let entries = desktop_model_entries(&id, model)?;
    let old_entries = desktop_entries(connection, &id)?;
    let selected = format!("{id}/{MODEL}");
    let previous = desktop::selected_model(connection)?;
    let old_model = if snapshot.get("modelInfo").is_some() {
        Some(restored_model_info(&snapshot["modelInfo"])?)
    } else {
        None
    };
    if snapshot.get("previousModel").is_none() {
        snapshot["previousModel"] = json!(previous);
    }
    snapshot["providerId"] = json!(id);
    snapshot["pending"] = json!(true);
    super::super::cursor_auth::write(path, &snapshot)?;
    let written = async {
        write_desktop_models(bridge, connection, &id, model, &entries).await?;
        desktop::set_selected_model(connection, Some(&selected))?;
        if desktop::selected_model(connection)?.as_deref() != Some(&selected) {
            return Err("accountError.write".into());
        }
        snapshot["modelInfo"] = stored_model_info(model)?;
        snapshot["pending"] = json!(false);
        super::super::cursor_auth::write(path, &snapshot)
    }
    .await;
    if let Err(error) = written {
        // Restore the previous owned provider/key after a partial native write.
        let rollback = if let Some(old_model) = old_model {
            write_desktop_models(bridge, connection, &id, &old_model, &old_entries).await
        } else {
            delete_desktop_provider(bridge, &id).await
        };
        let current = desktop::selected_model(connection)?;
        if current
            .as_deref()
            .is_some_and(|value| desktop_owns_selection(&id, value))
            || current.is_none()
        {
            desktop::set_selected_model(connection, previous.as_deref())?;
        }
        if rollback.is_err() {
            return Err("accountError.write".into());
        }
        if had_snapshot {
            super::super::cursor_auth::write(path, &original)?;
        } else {
            fs::remove_file(path).map_err(|_| "accountError.write")?;
        }
        return Err(error);
    }
    Ok(())
}

pub(super) async fn read_copilot_desktop() -> Option<ModelInfo> {
    read_desktop_at(&desktop_snapshot_path())
}

fn read_desktop_at(path: &Path) -> Option<ModelInfo> {
    read_desktop_from(path, &config_dir().join("data.db"))
}

fn read_desktop_from(path: &Path, database: &Path) -> Option<ModelInfo> {
    let snapshot = read_object(path).ok()?;
    if snapshot["pending"] == true {
        return None;
    }
    let id = snapshot["providerId"].as_str()?;
    // Passive navigation performs no IPC, processes, credential reads, or network.
    let connection =
        rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .ok()?;
    let selected = super::super::copilot_accounts::desktop::selected_model(&connection).ok()??;
    if !desktop_owns_selection(id, &selected) {
        return None;
    }
    restored_model_info(&snapshot["modelInfo"]).ok()
}

pub(super) async fn restore_copilot_desktop_to_official() -> ApplyResult {
    outcome(
        restore_desktop_at(&desktop_snapshot_path()).await,
        "GitHub Copilot Desktop restored to its previous model.",
    )
}

async fn restore_desktop_at(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let starting = super::super::copilot_accounts::desktop::WriteSession::start().await;
    #[cfg(test)]
    trace_live_result("restore.initialize", &starting);
    let mut session = starting?;
    let result = restore_desktop_with_bridge(path, &mut session.bridge).await;
    #[cfg(test)]
    trace_live_result("restore.native_write", &result);
    let cleanup = session.finish();
    #[cfg(test)]
    trace_live_result("restore.owned_cleanup", &cleanup);
    result.and(cleanup)
}

pub(super) async fn restore_copilot_desktop_with_bridge(
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
) -> ApplyResult {
    outcome(
        restore_desktop_with_bridge(&desktop_snapshot_path(), bridge).await,
        "GitHub Copilot Desktop restored to its previous model.",
    )
}

async fn restore_desktop_with_bridge(
    path: &Path,
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut connection = super::super::copilot_accounts::desktop::database()?;
    restore_desktop_with_connection(path, bridge, &mut connection).await
}

async fn restore_desktop_with_connection(
    path: &Path,
    bridge: &mut super::super::copilot_accounts::desktop::Desktop,
    connection: &mut rusqlite::Connection,
) -> Result<(), String> {
    use super::super::copilot_accounts::desktop;
    if !path.exists() {
        return Ok(());
    }
    let snapshot = read_object(path)?;
    let id = snapshot["providerId"]
        .as_str()
        .ok_or("accountError.format")?;
    let selected = desktop::selected_model(connection)?;
    let mut pending = snapshot.clone();
    pending["pending"] = json!(true);
    super::super::cursor_auth::write(path, &pending)?;
    delete_desktop_provider(bridge, id).await?;
    if selected
        .as_deref()
        .is_some_and(|value| desktop_owns_selection(id, value))
    {
        desktop::set_selected_model(connection, snapshot["previousModel"].as_str())?;
    }
    fs::remove_file(path).map_err(|_| "accountError.write".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "Explicit live app probe; requires ECHOBIRD_LIVE_COPILOT_DESKTOP_APPLY=1"]
    async fn live_copilot_desktop_apply_read_restore() {
        live_desktop_roundtrip(false).await;
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "Explicit closed app probe; requires ECHOBIRD_LIVE_COPILOT_DESKTOP_APPLY=1"]
    async fn live_copilot_desktop_closed_apply_read_restore() {
        live_desktop_roundtrip(true).await;
    }

    #[cfg(windows)]
    async fn live_desktop_roundtrip(initially_closed: bool) {
        use super::super::super::copilot_accounts::desktop;
        assert!(std::env::var("ECHOBIRD_LIVE_COPILOT_DESKTOP_APPLY").ok().as_deref()==Some("1"),
            "Set ECHOBIRD_LIVE_COPILOT_DESKTOP_APPLY=1 explicitly before this ignored native write probe.");
        fn optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
            match fs::read(path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err("Unable to snapshot the existing EchoBird configuration.".into()),
            }
        }
        fn native_tables() -> Result<Vec<Vec<Vec<rusqlite::types::Value>>>, String> {
            let connection = rusqlite::Connection::open_with_flags(
                config_dir().join("data.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .map_err(|_| "Native database is not initialized.")?;
            [
                "SELECT * FROM model_providers ORDER BY id",
                "SELECT * FROM provider_models ORDER BY id",
            ]
            .into_iter()
            .map(|sql| {
                let mut statement = connection
                    .prepare(sql)
                    .map_err(|_| "Native model schema is not supported.")?;
                let count = statement.column_count();
                let rows = statement
                    .query_map([], |row| {
                        (0..count)
                            .map(|column| row.get(column))
                            .collect::<Result<Vec<rusqlite::types::Value>, _>>()
                    })
                    .map_err(|_| "Unable to snapshot native model metadata.")?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "Unable to snapshot native model metadata.")?;
                Ok(rows)
            })
            .collect()
        }
        let initially_running =
            super::super::super::process_manager::desktop_tool_is_running("copilotdesktop")
                .await
                .unwrap();
        assert!(
            initially_running != initially_closed,
            "The open/closed probe must begin with the requested native app state."
        );
        let before_tables = native_tables().expect("Unable to snapshot native model rows.");
        let before_selected = desktop::selected_model(&desktop::database().unwrap()).unwrap();
        let before_owned_file = optional_file(&desktop_snapshot_path()).unwrap();
        let mut initial_session = desktop::WriteSession::start().await.unwrap();
        let before_providers = initial_session
            .bridge
            .request(
                json!({"type":"list_model_providers"}),
                "model_providers_list",
            )
            .await
            .expect("Unable to read the native provider metadata.")["providers"]
            .clone();
        initial_session.finish().unwrap();
        if initially_closed {
            assert!(
                !super::super::super::process_manager::desktop_tool_is_running("copilotdesktop")
                    .await
                    .unwrap(),
                "Initial metadata session left its temporary native app running."
            );
        }
        // A separate recovery file and a generated provider UUID leave every real
        // EchoBird provider/key untouched, including an already applied selection.
        let unique = uuid::Uuid::new_v4().to_string();
        let fixture_dir = std::env::temp_dir().join(format!("echobird-copilot-live-{unique}"));
        fs::create_dir(&fixture_dir).unwrap();
        let fixture_path = fixture_dir.join("model.json");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let model_id = format!("echobird-live-{unique}");
        let catalog_model = model_id.clone();
        let catalog = axum::Router::new()
            .route(
                "/v1/models",
                axum::routing::get(move || {
                    let model = catalog_model.clone();
                    async move {
                        axum::Json(json!({"object":"list","data":[{"id":model,"object":"model"}]}))
                    }
                }),
            )
            .fallback(|| async { axum::http::StatusCode::METHOD_NOT_ALLOWED });
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            axum::serve(listener, catalog)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        let model:ModelInfo=serde_json::from_value(json!({"name":"EchoBird live fixture","model":model_id,"baseUrl":format!("http://{address}/v1"),"apiKey":"echobird-live-fixture-key","protocol":"openai"})).unwrap();
        let applied = apply_desktop_at(&model, &fixture_path).await;
        let reapplied = if applied.is_ok() {
            apply_desktop_at(&model, &fixture_path).await
        } else {
            Err("Initial native apply failed.".into())
        };
        let read_matches = read_desktop_at(&fixture_path).is_some_and(|actual| {
            actual.model == model.model
                && actual.base_url == model.base_url
                && actual.api_key == model.api_key
        });
        // Cleanup also runs after a failed apply/read; all errors are credential-free.
        let restored = restore_desktop_at(&fixture_path).await;
        let _ = stop.send(());
        let _ = server.await;
        assert!(
            restored.is_ok(),
            "Native restoration or owned cleanup failed (recovery file present={}); isolated path {}.",
            fixture_path.exists(),
            fixture_path.display()
        );
        assert!(
            !fixture_path.exists(),
            "Isolated recovery file was not removed."
        );
        let after_selected = desktop::selected_model(&desktop::database().unwrap()).unwrap();
        let after_tables = native_tables().unwrap();
        let mut final_session = desktop::WriteSession::start().await.unwrap();
        let after_providers = final_session
            .bridge
            .request(
                json!({"type":"list_model_providers"}),
                "model_providers_list",
            )
            .await
            .unwrap()["providers"]
            .clone();
        final_session.finish().unwrap();
        let finally_running =
            super::super::super::process_manager::desktop_tool_is_running("copilotdesktop")
                .await
                .unwrap();
        assert!(
            initially_running == finally_running,
            "Native app running state was not restored."
        );
        assert!(
            before_selected == after_selected,
            "Native default model was not restored."
        );
        assert!(
            before_tables == after_tables,
            "An original native provider/model row changed or the owned fixture remains."
        );
        assert!(
            before_providers == after_providers,
            "The official app's provider metadata was not restored."
        );
        assert!(
            before_owned_file == optional_file(&desktop_snapshot_path()).unwrap(),
            "The real EchoBird configuration changed."
        );
        fs::remove_dir(fixture_dir).unwrap();
        assert!(
            applied.is_ok(),
            "Native fixture apply failed; cleanup has completed."
        );
        assert!(
            reapplied.is_ok(),
            "Repeated native fixture apply failed; cleanup has completed."
        );
        assert!(
            read_matches,
            "Native applied selection could not be read; cleanup has completed."
        );
        eprintln!("Native Copilot Desktop apply/read/restore verified; original provider/model rows and EchoBird configuration restored.");
    }

    #[test]
    fn desktop_uses_official_provider_and_secret_protocol() {
        let model: ModelInfo=serde_json::from_value(json!({"model":"claude-sonnet-4-6","baseUrl":"https://api.example.com","apiKey":"fixture-secret","protocol":"anthropic"})).unwrap();
        let (provider, selected) = desktop_payload("owned-provider", &model).unwrap();
        assert_eq!(provider["provider"]["kind"], "anthropic");
        assert_eq!(provider["provider"]["settings"]["authKind"], "api_key");
        assert!(!provider["provider"]["settings"]
            .to_string()
            .contains("fixture-secret"));
        assert_eq!(
            provider["secret"],
            json!({"kind":"apiKey","value":"fixture-secret"})
        );
        assert_eq!(selected["model"]["providerId"], "owned-provider");
        assert_eq!(selected["model"]["modelId"], MODEL);
        assert_eq!(selected["model"]["wireModel"], "claude-sonnet-4-6");
    }

    #[test]
    fn applies_only_owned_entries_and_restores_previous_model() {
        let dir =
            std::env::temp_dir().join(format!("echobird-copilot-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let registry = dir.join("providers.json");
        let settings = dir.join("settings.json");
        let snapshot = dir.join("snapshot.json");
        write_json_file(&registry, &json!({"providers":[{"name":"user","baseUrl":"https://example.com"}],"models":[{"id":"mine","provider":"user"}],"other":true})).unwrap();
        fs::write(
            &settings,
            "{ // personal preference\n \"model\":\"claude-sonnet-4.6\",\"theme\":\"dark\",\n}",
        )
        .unwrap();
        let model: ModelInfo = serde_json::from_value(json!({"model":"deepseek-chat","baseUrl":"https://example.net/v1","apiKey":"fixture-key","protocol":"openai"})).unwrap();
        apply_at(&registry, &settings, &snapshot, &model).unwrap();
        apply_at(&registry, &settings, &snapshot, &model).unwrap();
        assert!(!fs::read_to_string(&snapshot)
            .unwrap()
            .contains("fixture-key"));
        assert_eq!(
            restored_model_info(&read_object(&snapshot).unwrap()["modelInfo"])
                .unwrap()
                .api_key,
            model.api_key
        );
        let applied = read_object(&registry).unwrap();
        assert_eq!(applied["providers"].as_array().unwrap().len(), 2);
        assert_eq!(applied["models"][0]["provider"], "user");
        assert_eq!(applied["models"][1]["wireModel"], "deepseek-chat");
        assert_eq!(read_object(&settings).unwrap()["theme"], "dark");
        restore_at(&registry, &settings, &snapshot).unwrap();
        assert_eq!(
            read_object(&settings).unwrap()["model"],
            "claude-sonnet-4.6"
        );
        assert_eq!(
            read_object(&registry).unwrap()["providers"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        fs::remove_file(registry).unwrap();
        fs::remove_file(settings).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn refuses_invalid_registry_without_replacing_native_files() {
        let dir =
            std::env::temp_dir().join(format!("echobird-copilot-invalid-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let registry = dir.join("providers.json");
        fs::write(&registry, "{\"providers\":\"invalid\"}").unwrap();
        let model: ModelInfo =
            serde_json::from_value(json!({"model":"m","baseUrl":"https://example.net"})).unwrap();
        assert!(apply_at(
            &registry,
            &dir.join("settings.json"),
            &dir.join("snapshot.json"),
            &model
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(&registry).unwrap(),
            "{\"providers\":\"invalid\"}"
        );
        assert!(!dir.join("settings.json").exists());
        fs::remove_file(registry).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn cli_reapply_write_failure_restores_native_files_and_the_saved_model() {
        let dir = std::env::temp_dir().join(format!(
            "echobird-copilot-rollback-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir(&dir).unwrap();
        let registry = dir.join("providers.json");
        let settings = dir.join("settings.json");
        let snapshot = dir.join("snapshot.json");
        fs::write(&registry, b"{\"providers\":[{\"name\":\"user\",\"keep\":true}],\"models\":[],\"unknown\":\"preserve\"}\n").unwrap();
        fs::write(
            &settings,
            b"// User settings\n{\"model\":\"official-model\",\"theme\":\"dark\"}\n",
        )
        .unwrap();
        let old: ModelInfo = serde_json::from_value(json!({"model":"old-model","baseUrl":"https://old.example/v1","apiKey":"fixture-old-key"})).unwrap();
        let new: ModelInfo = serde_json::from_value(json!({"model":"new-model","baseUrl":"https://new.example/v1","apiKey":"fixture-new-key"})).unwrap();
        apply_at(&registry, &settings, &snapshot, &old).unwrap();
        let blocked_registry = dir.join(format!("{}.json", "a".repeat(230)));
        fs::rename(&registry, &blocked_registry).unwrap();
        let old_time = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400);
        fs::OpenOptions::new()
            .write(true)
            .open(&blocked_registry)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old_time))
            .unwrap();
        let registry_time = fs::metadata(&blocked_registry).unwrap().modified().unwrap();
        let before = [&blocked_registry, &settings, &snapshot].map(|path| fs::read(path).unwrap());
        assert_eq!(
            read_copilot_at(&settings, &snapshot)
                .unwrap()
                .model
                .as_deref(),
            Some("old-model")
        );
        // Only the atomic temporary filename exceeds the filesystem component limit.
        assert!(apply_at(&blocked_registry, &settings, &snapshot, &new).is_err());
        for (path, bytes) in [&blocked_registry, &settings, &snapshot]
            .into_iter()
            .zip(before)
        {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        assert_eq!(
            fs::metadata(&blocked_registry).unwrap().modified().unwrap(),
            registry_time
        );
        let current = read_copilot_at(&settings, &snapshot).unwrap();
        assert_eq!(current.model, old.model);
        assert_eq!(current.base_url, old.base_url);
        assert_eq!(current.api_key, old.api_key);

        fs::rename(&blocked_registry, &registry).unwrap();
        apply_at(&registry, &settings, &snapshot, &new).unwrap();
        let current = read_copilot_at(&settings, &snapshot).unwrap();
        assert_eq!(current.model, new.model);
        assert_eq!(current.base_url, new.base_url);
        assert_eq!(current.api_key, new.api_key);
        let native = read_object(&registry).unwrap();
        assert_eq!(native["models"][0]["wireModel"], "new-model");
        assert_eq!(native["providers"][0]["keep"], true);
        assert_eq!(native["unknown"], "preserve");
        assert_eq!(read_object(&settings).unwrap()["theme"], "dark");
        assert_eq!(
            read_object(&snapshot).unwrap()["previousModel"],
            "official-model"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_first_cli_apply_restores_missing_files() {
        let dir =
            std::env::temp_dir().join(format!("echobird-copilot-missing-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir).unwrap();
        let blocked_registry = dir.join(format!("{}.json", "a".repeat(230)));
        let settings = dir.join("settings.json");
        let snapshot = dir.join("snapshot.json");
        let model: ModelInfo =
            serde_json::from_value(json!({"model":"new-model","baseUrl":"https://new.example/v1"}))
                .unwrap();
        assert!(apply_at(&blocked_registry, &settings, &snapshot, &model).is_err());
        assert!(!blocked_registry.exists());
        assert!(!settings.exists());
        assert!(!snapshot.exists());
        assert!(read_copilot_at(&settings, &snapshot).is_none());
        let registry = dir.join("providers.json");
        apply_at(&registry, &settings, &snapshot, &model).unwrap();
        assert_eq!(
            read_copilot_at(&settings, &snapshot).unwrap().model,
            model.model
        );
        assert!(registry.exists());
        assert!(settings.exists());
        assert!(snapshot.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
#[path = "copilot_desktop_tests.rs"]
mod desktop_tests;
