//! Local-only, versioned configuration migration. Never reads native client stores.
mod format;
mod records;
use records::{Record, STORES};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
static RUNNING: AtomicBool = AtomicBool::new(false);
const PREFERENCE_KEYS: &[&str] = &[
    "developerMode",
    "statisticsRecording",
    "aiNewsEnabled",
    "aiCareerEnabled",
    "localLlmEnabled",
    "locale",
    "themeMode",
    "colorTheme",
    "closeToTray",
    "closeWindowBehaviorSet",
    "displayName",
    "sensitiveInfoProtection",
];
const UI_KEYS: &[&str] = &[
    "echobird_background",
    "echobird_easter_egg",
    "echobird_auto_claim_rewards",
    "echobird-ui-scale",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Backup {
    version: u32,
    created_at: String,
    origin: String,
    records: Vec<Record>,
    ui_preferences: BTreeMap<String, String>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub records: usize,
    pub duplicates: usize,
    pub device_bound: usize,
    pub skipped: usize,
    pub file_name: String,
    pub ui_preferences: BTreeMap<String, String>,
}
pub(crate) fn active() -> bool {
    RUNNING.load(Ordering::Acquire)
}

struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::Release);
    }
}
fn guard() -> Result<Guard, String> {
    RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "backupError.busy")?;
    let guard = Guard;
    if super::auto_import::active() {
        return Err("backupError.busy".into());
    }
    Ok(guard)
}
fn crypt_export(s: &str) -> Result<String, String> {
    super::secret_store::decrypt(s).map_err(|_| "backupError.credentials".into())
}
fn crypt_restore(s: &str) -> Result<String, String> {
    super::secret_store::encrypt(s).map_err(|_| "backupError.credentials".into())
}
fn valid_router(id: &str) -> bool {
    id == "smart-router"
        || id
            .strip_prefix("smart-router-")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}
fn preference_valid(key: &str, value: &str) -> bool {
    match key {
        "echobird_background" => {
            ["none", "characters", "carbon", "basalt", "monogram"].contains(&value)
        }
        "echobird_easter_egg" | "echobird_auto_claim_rewards" => ["true", "false"].contains(&value),
        "echobird-ui-scale" => {
            value == "auto" || value.parse::<u16>().is_ok_and(|v| (70..=150).contains(&v))
        }
        _ => false,
    }
}
fn validate(backup: &Backup) -> Result<(), String> {
    if backup.version != 1 {
        return Err("backupError.version".into());
    }
    if backup.records.len() > 10_000 || backup.origin.len() > 128 {
        return Err("backupError.size".into());
    }
    if backup
        .ui_preferences
        .iter()
        .any(|(k, v)| !UI_KEYS.contains(&k.as_str()) || !preference_valid(k, v))
    {
        return Err("backupError.format".into());
    }
    let mut keys = BTreeSet::new();
    for r in &backup.records {
        if !r.data.is_object()
            || r.id.is_empty()
            || r.id.len() > 256
            || r.id.chars().any(char::is_control)
            || !keys.insert((&r.kind, &r.id))
        {
            return Err("backupError.format".into());
        }
        match r.kind.as_str() {
            "preferences" if r.id == "general" => {
                if r.data
                    .as_object()
                    .is_none_or(|m| m.keys().any(|key| !PREFERENCE_KEYS.contains(&key.as_str())))
                {
                    return Err("backupError.format".into());
                }
                let _: crate::commands::settings_commands::AppSettings =
                    serde_json::from_value(r.data.clone()).map_err(|_| "backupError.format")?;
            }
            "modelOrder" if r.id == "display" => {
                let _: Vec<String> = serde_json::from_value(r.data["ids"].clone())
                    .map_err(|_| "backupError.format")?;
            }
            "model" => {
                let model: crate::models::model::ModelConfig =
                    serde_json::from_value(r.data.clone()).map_err(|_| "backupError.format")?;
                if model.internal_id != r.id {
                    return Err("backupError.format".into());
                }
            }
            "router" if valid_router(&r.id) => {
                if !r.data["candidateIds"].is_array()
                    || r.data["candidateIds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|v| !v.is_string())
                {
                    return Err("backupError.format".into());
                }
            }
            kind => {
                let store = STORES
                    .iter()
                    .find(|s| kind == format!("account:{}", s.provider))
                    .ok_or("backupError.format")?;
                if matches!(
                    store.layout,
                    records::Layout::Directory | records::Layout::YamlDirectory
                ) && !records::directory_id(&r.id)
                {
                    return Err("backupError.format".into());
                }
                records::validate_row(store.provider, &r.data)?;
                for field in ["account", "summary"] {
                    if r.data[field]["id"].as_str().is_some_and(|id| id != r.id) {
                        return Err("backupError.format".into());
                    }
                }
                if matches!(store.layout, records::Layout::Vector)
                    && r.data["id"].as_str() != Some(&r.id)
                {
                    return Err("backupError.format".into());
                }
            }
        }
    }
    Ok(())
}
fn collect(
    root: &Path,
    origin: String,
    ui_preferences: BTreeMap<String, String>,
    decrypt: &impl Fn(&str) -> Result<String, String>,
) -> Result<Backup, String> {
    let mut entries = vec![];
    if let Some(mut value) = records::json_at(&root.join("settings.json"))? {
        value
            .as_object_mut()
            .ok_or("backupError.format")?
            .retain(|key, _| PREFERENCE_KEYS.contains(&key.as_str()));
        entries.push(Record {
            kind: "preferences".into(),
            id: "general".into(),
            data: value,
            device_bound: false,
        });
    }
    if let Some(value) = records::json_at(&root.join("config/models.json"))? {
        let models: Vec<crate::models::model::ModelConfig> =
            serde_json::from_value(value).map_err(|_| "backupError.format")?;
        for model in models {
            entries.push(Record {
                kind: "model".into(),
                id: model.internal_id.clone(),
                data: serde_json::to_value(model).map_err(|_| "backupError.format")?,
                device_bound: false,
            });
        }
    }
    if let Some(value) = records::json_at(&root.join("config/model-order.json"))? {
        entries.push(Record {
            kind: "modelOrder".into(),
            id: "display".into(),
            data: json!({"ids":value}),
            device_bound: false,
        });
    }
    let mut total_bytes = 0usize;
    for store in STORES {
        for (id, data) in records::rows(root, store)? {
            total_bytes += serde_json::to_vec(&data)
                .map_err(|_| "backupError.format")?
                .len();
            if total_bytes as u64 > format::LIMIT / 2 {
                return Err("backupError.size".into());
            }
            let device_bound = records::device_bound(store.provider, &data);
            entries.push(Record {
                kind: format!("account:{}", store.provider),
                id,
                data,
                device_bound,
            });
        }
    }
    let registry = records::json_at(&root.join("config/smart-routers.json"))?;
    let ids = match registry {
        Some(value) => value["ids"]
            .as_array()
            .ok_or("backupError.format")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or("backupError.format".into())
            })
            .collect::<Result<Vec<String>, String>>()?,
        None => vec!["smart-router".into()],
    };
    for id in ids {
        if !valid_router(&id) {
            return Err("backupError.format".into());
        }
        if let Some(data) = records::json_at(&root.join("config").join(format!("{id}.json")))? {
            entries.push(Record {
                kind: "router".into(),
                id,
                data,
                device_bound: false,
            });
        }
    }
    let backup = Backup {
        version: 1,
        created_at: chrono::Utc::now().to_rfc3339(),
        origin,
        records: entries,
        ui_preferences,
    };
    let mut backup = backup;
    for entry in &mut backup.records {
        records::map_secrets(&mut entry.data, true, decrypt, 0)?;
    }
    let mut plain = backup.clone();
    for record in &mut plain.records {
        records::map_secrets(&mut record.data, false, &|s| Ok(s.to_owned()), 0)?;
    }
    validate(&plain)?;
    Ok(backup)
}
pub fn backup(
    directory: &Path,
    password: &str,
    ui_preferences: BTreeMap<String, String>,
) -> Result<Outcome, String> {
    let _guard = guard()?;
    if password.chars().count() < 8 {
        return Err("backupError.passwordLength".into());
    }
    let backup = collect(
        &crate::utils::platform::echobird_dir(),
        super::secret_store::backup_device_id(),
        ui_preferences,
        &crypt_export,
    )?;
    let data = serde_json::to_vec(&backup).map_err(|_| "backupError.format")?;
    let bytes = format::encode(&data, password)?;
    if !directory.is_dir() {
        return Err("backupError.write".into());
    }
    let name = format!(
        "EchoBird-{}-{}.ebbackup",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().to_string()[..8]
    );
    let path = directory.join(&name);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|_| "backupError.write")?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err("backupError.write".into());
    }
    Ok(Outcome {
        records: backup.records.len(),
        device_bound: backup.records.iter().filter(|r| r.device_bound).count(),
        file_name: name,
        ..Default::default()
    })
}
struct Planned {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}
fn add_plan(
    root: &Path,
    path: PathBuf,
    value: &Value,
    plan: &mut Vec<Planned>,
) -> Result<(), String> {
    let after = serde_json::to_vec_pretty(value).map_err(|_| "backupError.format")?;
    add_bytes(root, path, after, plan)
}
fn add_bytes(
    root: &Path,
    path: PathBuf,
    after: Vec<u8>,
    plan: &mut Vec<Planned>,
) -> Result<(), String> {
    let relative = path.strip_prefix(root).map_err(|_| "backupError.format")?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("backupError.format".into());
        }
    }
    let before = records::read(&path)?;
    if before.as_deref() != Some(&after) {
        plan.push(Planned {
            path,
            before,
            after,
        });
    }
    Ok(())
}
fn same_realm(a: &Value, b: &Value) -> bool {
    let a = a.get("account").or_else(|| a.get("summary")).unwrap_or(a);
    let b = b.get("account").or_else(|| b.get("summary")).unwrap_or(b);
    ["edition", "region", "provider"]
        .iter()
        .all(|key| a.get(key).zip(b.get(key)).is_none_or(|(a, b)| a == b))
}
fn prepare(
    root: &Path,
    mut backup: Backup,
    origin: &str,
    encrypt: &impl Fn(&str) -> Result<String, String>,
    decrypt: &impl Fn(&str) -> Result<String, String>,
) -> Result<(Vec<Planned>, Outcome), String> {
    let sealed_records: BTreeMap<_, _> = backup
        .records
        .iter()
        .map(|r| ((r.kind.clone(), r.id.clone()), r.data.clone()))
        .collect();
    // Secrets are unwrapped in memory before schema validation, never into a temporary file.
    for record in &mut backup.records {
        records::map_secrets(&mut record.data, false, &|s| Ok(s.to_owned()), 0)?;
    }
    validate(&backup)?;
    let mut outcome = Outcome {
        ui_preferences: backup.ui_preferences,
        ..Default::default()
    };
    let mut plan = vec![];
    let mut account_map = BTreeMap::new();
    for store in STORES {
        let mut rows = records::rows(root, store)?;
        let mut inserted = BTreeMap::new();
        for record in backup
            .records
            .iter()
            .filter(|r| r.kind == format!("account:{}", store.provider))
        {
            let (_, email) = records::identity(&record.id, &record.data);
            let duplicate = rows.iter().find(|(id, row)| {
                *id == &record.id
                    || same_realm(row, &record.data)
                        && super::auto_import::native::same_account(
                            &record.id,
                            email,
                            id,
                            records::identity(id, row).1,
                        )
            });
            let effective = record.data["summary"]["edition"]
                .as_str()
                .unwrap_or(store.provider);
            if let Some((id, _)) = duplicate {
                account_map.insert((effective.to_owned(), record.id.clone()), id.clone());
                outcome.duplicates += 1;
                continue;
            }
            if (record.device_bound
                || records::device_bound(
                    store.provider,
                    sealed_records
                        .get(&(record.kind.clone(), record.id.clone()))
                        .ok_or("backupError.format")?,
                ))
                && backup.origin != origin
            {
                outcome.skipped += 1;
                outcome.device_bound += 1;
                continue;
            }
            let mut row = sealed_records
                .get(&(record.kind.clone(), record.id.clone()))
                .ok_or("backupError.format")?
                .clone();
            records::map_secrets(&mut row, false, encrypt, 0)?;
            records::clear_activation(&mut row);
            account_map.insert((effective.to_owned(), record.id.clone()), record.id.clone());
            inserted.insert(record.id.clone(), row.clone());
            rows.insert(record.id.clone(), row);
            outcome.records += 1;
        }
        if !inserted.is_empty() {
            let selected = if matches!(
                store.layout,
                records::Layout::Directory | records::Layout::YamlDirectory
            ) {
                &inserted
            } else {
                &rows
            };
            for (path, bytes) in records::store_files(root, store, selected)? {
                add_bytes(root, path, bytes, &mut plan)?;
            }
        }
    }
    let models_path = root.join("config/models.json");
    let mut models: Vec<Value> = records::json_at(&models_path)?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| "backupError.format")?
        .unwrap_or_default();
    let mut model_map = BTreeMap::new();
    let mut model_changed = false;
    for record in backup.records.iter().filter(|r| r.kind == "model") {
        let mut model = record.data.clone();
        if let Some(source) = model.get_mut("accountSource").filter(|v| v.is_object()) {
            let provider = source["provider"]
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            let id = source["accountId"].as_str().unwrap_or_default().to_owned();
            if let Some(mapped) = account_map.get(&(provider, id)) {
                source["accountId"] = json!(mapped);
            } else {
                outcome.skipped += 1;
                continue;
            }
        }
        let key = model["apiKey"].as_str().unwrap_or_default();
        let existing = models.iter().find(|m| {
            m["internalId"] == record.id
                || !key.is_empty()
                    && m["apiKey"]
                        .as_str()
                        .and_then(|s| decrypt(s).ok())
                        .as_deref()
                        == Some(key)
        });
        if let Some(existing) = existing {
            model_map.insert(
                record.id.clone(),
                existing["internalId"]
                    .as_str()
                    .ok_or("backupError.format")?
                    .to_owned(),
            );
            outcome.duplicates += 1;
            continue;
        }
        if !key.is_empty() {
            model["apiKey"] = json!(encrypt(key)?);
        }
        model_map.insert(record.id.clone(), record.id.clone());
        models.push(model);
        model_changed = true;
        outcome.records += 1;
    }
    if model_changed {
        add_plan(root, models_path, &json!(models), &mut plan)?;
    }
    for record in backup.records.iter().filter(|r| r.kind == "modelOrder") {
        let path = root.join("config/model-order.json");
        let mut order: Vec<String> = records::json_at(&path)?
            .map(serde_json::from_value)
            .transpose()
            .map_err(|_| "backupError.format")?
            .unwrap_or_default();
        for id in record.data["ids"].as_array().ok_or("backupError.format")? {
            let id = id.as_str().ok_or("backupError.format")?;
            let mapped = model_map.get(id).map(String::as_str).unwrap_or(id);
            if !order.iter().any(|s| s == mapped) {
                order.push(mapped.into());
            }
        }
        add_plan(root, path, &json!(order), &mut plan)?;
        outcome.records += 1;
    }
    let registry_path = root.join("config/smart-routers.json");
    let mut registry =
        records::json_at(&registry_path)?.unwrap_or(json!({"enabled":false,"ids":[]}));
    let ids = registry["ids"].as_array_mut().ok_or("backupError.format")?;
    let mut router_changed = false;
    for record in backup.records.iter().filter(|r| r.kind == "router") {
        let path = root.join("config").join(format!("{}.json", record.id));
        if path.exists() {
            outcome.duplicates += 1;
            continue;
        }
        if ids.len() >= 7 {
            outcome.skipped += 1;
            continue;
        }
        let mut config = record.data.clone();
        config["enabled"] = json!(false);
        for field in ["candidateIds", "disabledCandidateIds"] {
            if let Some(list) = config[field].as_array_mut() {
                for value in list {
                    if let Some(mapped) = value.as_str().and_then(|s| model_map.get(s)) {
                        *value = json!(mapped);
                    }
                }
            }
        }
        if let Some(key) = config["apiKey"].as_str().filter(|s| !s.is_empty()) {
            config["apiKey"] = json!(encrypt(key)?);
        }
        add_plan(root, path, &config, &mut plan)?;
        if !ids.iter().any(|id| id == &record.id) {
            ids.push(json!(record.id));
        }
        router_changed = true;
        outcome.records += 1;
    }
    if router_changed {
        add_plan(root, registry_path, &registry, &mut plan)?;
    }
    for record in backup.records.iter().filter(|r| r.kind == "preferences") {
        let path = root.join("settings.json");
        let mut settings = records::json_at(&path)?.unwrap_or(json!({}));
        settings
            .as_object_mut()
            .ok_or("backupError.format")?
            .extend(record.data.as_object().ok_or("backupError.format")?.clone());
        add_plan(root, path, &settings, &mut plan)?;
        outcome.records += 1;
    }
    Ok((plan, outcome))
}
pub fn restore(file: &Path, password: &str) -> Result<Outcome, String> {
    let _guard = guard()?;
    let bytes = records::read(file)?.ok_or("backupError.read")?;
    let data = format::decode(&bytes, password)?;
    let backup: Backup = serde_json::from_slice(&data).map_err(|_| "backupError.format")?;
    let root = crate::utils::platform::echobird_dir();
    let (plan, outcome) = prepare(
        &root,
        backup,
        &super::secret_store::backup_device_id(),
        &crypt_restore,
        &crypt_export,
    )?;
    publish(&plan)?;
    Ok(outcome)
}
fn publish(plan: &[Planned]) -> Result<(), String> {
    let mut transaction = super::native_account_files::Transaction::guarded();
    for item in plan {
        if records::read(&item.path)? != item.before {
            return Err("backupError.changed".into());
        }
        transaction
            .replace(&item.path, &item.after)
            .map_err(|_| "backupError.write")?;
    }
    transaction.commit();
    Ok(())
}

#[cfg(test)]
mod tests;
