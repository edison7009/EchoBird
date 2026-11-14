//! VS Code's native Custom Endpoint models and new-chat default selection.

use super::{
    jsonc::{input, set},
    ApplyResult, ModelInfo,
};
use crate::services::{codex_catalog, tool_manager};
use jsonc_parser::{cst::CstRootNode, ParseOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const GROUP: &str = "EchoBird";
const VENDOR: &str = "customendpoint";
const DEFAULT_MODEL: &str = "chat.defaultModel";
const BACKUP: &str = "echobird-model-backup.json";

fn user_dir() -> PathBuf {
    resolve_user_dir(
        dirs::config_dir().unwrap_or_default(),
        tool_manager::get_tool_exe_path("vscode")
            .as_deref()
            .map(Path::new),
    )
}

fn resolve_user_dir(config: PathBuf, exe: Option<&Path>) -> PathBuf {
    if let Some(exe) = exe {
        let exe = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_owned());
        for root in exe.ancestors().skip(1).take(3) {
            let portable = root.join("data");
            if portable.is_dir()
                && (Some(root) == exe.parent()
                    || root.join("resources/app/package.json").is_file()
                    || root.join("bin/code.cmd").is_file())
            {
                return portable.join("user-data/User");
            }
        }
        if let Some(bundle) = exe
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        {
            if let Some(root) = bundle.parent() {
                let portable = root.join("code-portable-data");
                if portable.is_dir() {
                    return portable.join("user-data/User");
                }
            }
        }
    }
    config.join("Code/User")
}

pub(crate) fn settings_path() -> PathBuf {
    user_dir().join("settings.json")
}

fn parse(text: &str, path: &Path, array: bool) -> Result<CstRootNode, String> {
    let options = ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    };
    let root = CstRootNode::parse(text.trim_start_matches('\u{feff}'), &options)
        .map_err(|_| format!("Invalid VS Code configuration: {}", path.display()))?;
    let valid = if array {
        root.array_value().is_some()
    } else {
        root.object_value().is_some()
    };
    if !valid {
        return Err(format!(
            "Unsupported VS Code configuration: {}",
            path.display()
        ));
    }
    Ok(root)
}

fn read_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(format!(
            "Cannot read VS Code configuration: {}",
            path.display()
        )),
    }
}

fn load(path: &Path, array: bool) -> Result<(CstRootNode, Option<Vec<u8>>), String> {
    let previous = read_bytes(path)?;
    let text = match previous.as_ref() {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| format!("Invalid VS Code configuration encoding: {}", path.display()))?,
        None if array => "[]",
        None => "{}",
    };
    Ok((parse(text, path, array)?, previous))
}

pub(crate) fn installed_version(exe: &Path) -> Option<String> {
    installed_version_at(exe, Path::new("/snap/code/current"))
}

fn installed_version_at(exe: &Path, snap: &Path) -> Option<String> {
    if exe == Path::new("/snap/bin/code") {
        return package_version(&snap.join("usr/share/code/resources/app/package.json"));
    }
    let exe = fs::canonicalize(exe).unwrap_or_else(|_| exe.to_owned());
    exe.ancestors().take(5).find_map(|root| {
        // New installations keep the active application under a commit directory.
        // The CLI launcher identifies that directory even when older copies remain.
        if let Ok(launcher) = fs::read_to_string(root.join("bin/code.cmd")) {
            for argument in launcher.split('"') {
                if let Some(relative) = argument.strip_prefix("%~dp0") {
                    let relative = relative.replace('\\', "/");
                    if let Some(app) = relative.strip_suffix("/out/cli.js") {
                        if app.ends_with("/resources/app") {
                            return package_version(
                                &root.join("bin").join(app).join("package.json"),
                            );
                        }
                    }
                }
            }
        }
        ["resources/app/package.json", "Resources/app/package.json"]
            .into_iter()
            .find_map(|relative| package_version(&root.join(relative)))
    })
}

fn package_version(path: &Path) -> Option<String> {
    let package: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    package["version"].as_str().map(str::to_owned)
}

fn supports_default_model(version: &str) -> bool {
    let mut parts = version.split('.');
    match (
        parts.next().and_then(|v| v.parse::<u32>().ok()),
        parts.next().and_then(|v| v.parse::<u32>().ok()),
    ) {
        (Some(major), Some(minor)) => major > 1 || (major == 1 && minor >= 127),
        _ => false,
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("Invalid VS Code configuration path")?)
        .map_err(|_| "Cannot create VS Code configuration directory")?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Cannot stage VS Code configuration")?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "Cannot save VS Code configuration")?;
        drop(file);
        fs::rename(&temporary, path).map_err(|_| "Cannot replace VS Code configuration".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

struct Change {
    path: PathBuf,
    previous: Option<Vec<u8>>,
    next: Option<Vec<u8>>,
}

fn replace(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
    match bytes {
        Some(bytes) => write(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(format!(
                "Cannot remove VS Code configuration: {}",
                path.display()
            )),
        },
    }
}

fn commit(
    changes: Vec<Change>,
    mut save: impl FnMut(&Path, Option<&[u8]>) -> Result<(), String>,
) -> Result<(), String> {
    let changes: Vec<_> = changes
        .into_iter()
        .filter(|change| change.previous != change.next)
        .collect();
    for change in &changes {
        if read_bytes(&change.path)? != change.previous {
            return Err("VS Code configuration changed during model switching. Try again.".into());
        }
    }
    for (index, change) in changes.iter().enumerate() {
        if let Err(error) = save(&change.path, change.next.as_deref()) {
            let mut rollback_failed = false;
            for previous in changes[..index].iter().rev() {
                rollback_failed |= save(&previous.path, previous.previous.as_deref()).is_err();
            }
            return Err(if rollback_failed {
                format!("{error}. Could not restore all VS Code configuration files.")
            } else {
                error
            });
        }
    }
    Ok(())
}

fn serialized(root: &CstRootNode, previous: Option<&[u8]>) -> Vec<u8> {
    let text = root.to_string();
    if previous.is_some_and(|bytes| bytes.starts_with(b"\xef\xbb\xbf")) {
        format!("\u{feff}{text}").into_bytes()
    } else {
        text.into_bytes()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DefaultBackup {
    version: u32,
    previous_default: Option<String>,
    applied_model: String,
}

fn read_backup(path: &Path, bytes: Option<&[u8]>) -> Result<Option<DefaultBackup>, String> {
    bytes
        .map(|bytes| {
            serde_json::from_slice::<DefaultBackup>(bytes)
                .ok()
                .filter(|backup| backup.version == 1)
                .ok_or_else(|| format!("Invalid VS Code model backup: {}", path.display()))
        })
        .transpose()
}

fn default_model(root: &CstRootNode) -> Result<Option<String>, String> {
    let settings = root.object_value().unwrap();
    let count = settings
        .properties()
        .iter()
        .filter(|property| {
            property
                .name()
                .and_then(|name| name.decoded_value().ok())
                .as_deref()
                == Some(DEFAULT_MODEL)
        })
        .count();
    if count > 1 {
        return Err("Duplicate VS Code default-model settings".into());
    }
    match settings
        .get(DEFAULT_MODEL)
        .and_then(|property| property.to_serde_value())
    {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err("VS Code default-model setting must be a string".into()),
        None => Ok(None),
    }
}

fn owned(group: &Value) -> bool {
    group["vendor"] == VENDOR && group["name"] == GROUP
}

fn normalized_id(model: &str) -> String {
    model.trim().to_lowercase()
}

fn profile_dirs(user: &Path) -> Result<Vec<PathBuf>, String> {
    let mut paths = vec![user.to_owned()];
    let profiles = user.join("profiles");
    let entries = match fs::read_dir(&profiles) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
        Err(_) => return Err("Cannot read VS Code profiles".into()),
    };
    for entry in entries {
        let entry = entry.map_err(|_| "Cannot read VS Code profile directory")?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|_| "Cannot read VS Code profile type")?
            .is_dir()
        {
            paths.push(path);
        }
    }
    paths[1..].sort();
    Ok(paths)
}

fn provider(info: &ModelInfo) -> Result<Value, String> {
    let model = info
        .model
        .as_deref()
        .filter(|model| !model.trim().is_empty())
        .ok_or("VS Code model ID is empty")?;
    let api_type = match info.protocol.as_deref().unwrap_or("openai") {
        "openai" => "chat-completions",
        "openai-responses" => "responses",
        "anthropic" => "messages",
        _ => return Err("Unsupported VS Code model protocol".into()),
    };
    let base = if api_type == "messages" {
        info.anthropic_url.as_deref().or(info.base_url.as_deref())
    } else {
        info.base_url.as_deref()
    }
    .filter(|url| !url.trim().is_empty())
    .ok_or("VS Code API URL is empty")?;
    let mut url = reqwest::Url::parse(base).map_err(|_| "Invalid VS Code API URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Invalid VS Code API URL scheme".into());
    }
    let endpoint = match api_type {
        "chat-completions" => "/chat/completions",
        "responses" => "/responses",
        _ => "/messages",
    };
    let path = url.path().trim_end_matches('/');
    if ["/chat/completions", "/responses", "/messages"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
    {
        if !path.ends_with(endpoint) {
            return Err("VS Code API endpoint does not match the selected protocol".into());
        }
    } else {
        let version = path.rsplit('/').next().unwrap_or("");
        let versioned = version.strip_prefix('v').is_some_and(|version| {
            !version.is_empty() && version.chars().all(|character| character.is_ascii_digit())
        });
        // Anthropic bases use /v1/messages; OpenAI bases keep their configured path.
        url.set_path(&format!(
            "{path}{}{endpoint}",
            if api_type == "messages" && !versioned {
                "/v1"
            } else {
                ""
            }
        ));
    }
    let (max_input_tokens, max_output_tokens, vision) = if url.host_str()
        == Some("api.deepseek.com")
        && matches!(
            model,
            "deepseek-flash"
                | "deepseek-v4-pro"
                | "deepseek-v4-flash"
                | "deepseek-v4-flash-vision-exp"
        ) {
        // Reserve the published output limit within the 1,048,576-token window.
        (655360, 393216, model != "deepseek-v4-pro")
    } else if let Some(entry) =
        codex_catalog::template_for_model(url.as_str(), model).filter(|entry| {
            entry["slug"].as_str() == Some(model)
                && entry["context_window"]
                    .as_u64()
                    .is_some_and(|tokens| tokens > 4096)
        })
    {
        let context = entry["context_window"].as_u64().unwrap();
        let vision = entry["input_modalities"]
            .as_array()
            .is_some_and(|modalities| modalities.iter().any(|modality| modality == "image"));
        (context - 4096, 4096, vision)
    } else {
        (28672, 4096, false)
    };
    let mut headers = json!({});
    if let Some(key) = info.api_key.as_deref().filter(|key| !key.is_empty()) {
        if api_type == "messages" {
            headers["x-api-key"] = json!(key);
        } else {
            headers["Authorization"] = json!(format!("Bearer {key}"));
        }
    }
    let mut group = json!({
        "name": GROUP,
        "vendor": VENDOR,
        "apiType": api_type,
        "models": [{
            "id": model,
            "name": info.name.as_deref().filter(|name| !name.is_empty()).unwrap_or(model),
            "url": url.as_str(),
            "toolCalling": true,
            "vision": vision,
            "maxInputTokens": max_input_tokens,
            "maxOutputTokens": max_output_tokens,
            "requestHeaders": headers,
        }],
    });
    super::switching::model_array(info, &mut group["models"], "id", "name")?;
    Ok(group)
}

fn apply_changes(dir: &Path, group: &Value) -> Result<Vec<Change>, String> {
    let models_path = dir.join("chatLanguageModels.json");
    let settings_path = dir.join("settings.json");
    let backup_path = dir.join(BACKUP);
    let (models, old_models) = load(&models_path, true)?;
    let (settings, old_settings) = load(&settings_path, false)?;
    let old_backup = read_bytes(&backup_path)?;
    let model = group["models"][0]["id"].as_str().unwrap();
    let current = default_model(&settings)?;
    let mut backup = read_backup(&backup_path, old_backup.as_deref())?.unwrap_or(DefaultBackup {
        version: 1,
        previous_default: current,
        applied_model: model.into(),
    });
    backup.applied_model = model.into();
    let groups = models.array_value().unwrap();
    let mut replaced = false;
    for node in groups.elements() {
        let existing = node
            .to_serde_value()
            .ok_or("Invalid VS Code provider configuration")?;
        if !existing.is_object() {
            return Err("VS Code model providers must be objects".into());
        }
        if owned(&existing) {
            if replaced {
                node.remove();
            } else {
                node.as_object().unwrap().replace_with(input(group));
                replaced = true;
            }
        } else if existing["models"].as_array().is_some_and(|models| {
            models.iter().any(|other| {
                other["id"]
                    .as_str()
                    .is_some_and(|other| normalized_id(other) == normalized_id(model))
            })
        }) {
            return Err(format!("VS Code model ID '{model}' already exists in another provider group. Choose a distinct model ID."));
        }
    }
    if !replaced {
        groups.append(input(group));
    }
    set(
        &settings.object_value().unwrap(),
        DEFAULT_MODEL,
        &json!(model),
    );
    Ok(vec![
        Change {
            path: backup_path,
            previous: old_backup,
            next: Some(
                serde_json::to_vec_pretty(&backup)
                    .map_err(|_| "Cannot save VS Code model backup")?,
            ),
        },
        Change {
            path: models_path,
            next: Some(serialized(&models, old_models.as_deref())),
            previous: old_models,
        },
        Change {
            path: settings_path,
            next: Some(serialized(&settings, old_settings.as_deref())),
            previous: old_settings,
        },
    ])
}

fn apply_at(user: &Path, info: &ModelInfo) -> Result<(), String> {
    let group = provider(info)?;
    let mut changes = Vec::new();
    for dir in profile_dirs(user)? {
        changes.extend(apply_changes(&dir, &group)?);
    }
    commit(changes, replace)
}

fn result(result: Result<(), String>, success: &str) -> ApplyResult {
    ApplyResult {
        success: result.is_ok(),
        message: result.err().unwrap_or_else(|| success.into()),
    }
}

pub(super) fn apply(info: &ModelInfo) -> ApplyResult {
    let configured = (|| {
        let exe = tool_manager::get_tool_exe_path("vscode").ok_or("VS Code is not installed")?;
        let version = installed_version(Path::new(&exe)).ok_or(
            "Cannot verify the installed VS Code version. VS Code 1.127 or newer is required.",
        )?;
        if !supports_default_model(&version) {
            return Err(
                "VS Code 1.127 or newer is required to switch the default chat model.".into(),
            );
        }
        let user = resolve_user_dir(
            dirs::config_dir().unwrap_or_default(),
            Some(Path::new(&exe)),
        );
        apply_at(&user, info)
    })();
    result(
        configured,
        "VS Code model configured for new chats. Existing chats keep their selected model.",
    )
}

fn read_at(dir: &Path) -> Option<ModelInfo> {
    let (settings, _) = load(&dir.join("settings.json"), false).ok()?;
    let selected = default_model(&settings).ok()??;
    let (models, _) = load(&dir.join("chatLanguageModels.json"), true).ok()?;
    let groups = models.to_serde_value()?;
    let group = groups.as_array()?.iter().find(|group| owned(group))?;
    let model = group["models"].as_array()?.iter().find(|model| {
        model["id"]
            .as_str()
            .is_some_and(|id| normalized_id(id) == normalized_id(&selected))
    })?;
    let api_type = model["apiType"]
        .as_str()
        .or_else(|| group["apiType"].as_str())
        .unwrap_or("chat-completions");
    let protocol = match api_type {
        "messages" => "anthropic",
        "responses" => "openai-responses",
        "chat-completions" => "openai",
        _ => return None,
    };
    let url = model["url"].as_str()?.to_owned();
    let api_key = if protocol == "anthropic" {
        model["requestHeaders"]["x-api-key"]
            .as_str()
            .map(str::to_owned)
    } else {
        model["requestHeaders"]["Authorization"]
            .as_str()
            .and_then(|header| header.strip_prefix("Bearer "))
            .map(str::to_owned)
    };
    Some(ModelInfo {
        name: model["name"].as_str().map(str::to_owned),
        model: model["id"].as_str().map(str::to_owned),
        base_url: (protocol != "anthropic").then_some(url.clone()),
        api_key,
        anthropic_url: (protocol == "anthropic").then_some(url),
        protocol: Some(protocol.into()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

pub(super) fn read() -> Option<ModelInfo> {
    let exe = tool_manager::get_tool_exe_path("vscode")?;
    let user = resolve_user_dir(
        dirs::config_dir().unwrap_or_default(),
        Some(Path::new(&exe)),
    );
    read_at(&user)
}

fn restore_changes(dir: &Path) -> Result<Vec<Change>, String> {
    let models_path = dir.join("chatLanguageModels.json");
    let settings_path = dir.join("settings.json");
    let backup_path = dir.join(BACKUP);
    let (models, old_models) = load(&models_path, true)?;
    let (settings, old_settings) = load(&settings_path, false)?;
    let old_backup = read_bytes(&backup_path)?;
    let backup = read_backup(&backup_path, old_backup.as_deref())?;
    let selected = default_model(&settings)?;
    let mut owned_models = Vec::new();
    for node in models.array_value().unwrap().elements() {
        let group = node
            .to_serde_value()
            .ok_or("Invalid VS Code provider configuration")?;
        if owned(&group) {
            if let Some(models) = group["models"].as_array() {
                owned_models.extend(
                    models
                        .iter()
                        .filter_map(|model| model["id"].as_str().map(str::to_owned)),
                );
            }
            node.remove();
        }
    }
    if backup.is_none() && owned_models.is_empty() {
        return Ok(Vec::new());
    }
    let restore_default = selected.as_deref().is_some_and(|selected| {
        backup
            .as_ref()
            .is_some_and(|backup| normalized_id(&backup.applied_model) == normalized_id(selected))
            || owned_models
                .iter()
                .any(|model| normalized_id(model) == normalized_id(selected))
    });
    let object = settings.object_value().unwrap();
    if restore_default {
        if let Some(previous) = backup
            .as_ref()
            .and_then(|backup| backup.previous_default.as_ref())
        {
            set(&object, DEFAULT_MODEL, &json!(previous));
        } else if let Some(property) = object.get(DEFAULT_MODEL) {
            property.remove();
        }
    }
    Ok(vec![
        Change {
            path: models_path,
            next: old_models
                .as_ref()
                .map(|_| serialized(&models, old_models.as_deref())),
            previous: old_models,
        },
        Change {
            path: settings_path,
            next: old_settings
                .as_ref()
                .map(|_| serialized(&settings, old_settings.as_deref())),
            previous: old_settings,
        },
        Change {
            path: backup_path,
            previous: old_backup,
            next: None,
        },
    ])
}

fn restore_at(user: &Path) -> Result<(), String> {
    let mut changes = Vec::new();
    for dir in profile_dirs(user)? {
        changes.extend(restore_changes(&dir)?);
    }
    commit(changes, replace)
}

pub(super) fn restore() -> ApplyResult {
    result(restore_at(&user_dir()), "VS Code's previous default model restored; other provider groups and settings were preserved.")
}

#[cfg(test)]
mod tests {
    #[test]
    fn smart_switch_native_catalog_regression() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let info = super::super::switching::test_info();
            let group = provider(&info).unwrap();
            let models = group["models"].as_array().unwrap();
            assert_eq!(models.len(), 3);
            assert_eq!(models[0]["id"], "auto");
            assert_eq!(models[1]["name"], "First model");
            assert_eq!(models[0]["requestHeaders"], models[1]["requestHeaders"]);
        });
    }

    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("echobird-vscode-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn save(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        fn value(&self, relative: &str, array: bool) -> Value {
            load(&self.0.join(relative), array)
                .unwrap()
                .0
                .to_serde_value()
                .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn info(protocol: &str, model: &str, url: &str) -> ModelInfo {
        serde_json::from_value(json!({
            "name": "Fixture model",
            "model": model,
            "baseUrl": url,
            "apiKey": "fixture-key",
            "protocol": protocol,
        }))
        .unwrap()
    }

    #[test]
    fn three_protocols_use_native_direct_endpoints_and_authentication() {
        for (protocol, api_type, suffix, auth) in [
            (
                "openai",
                "chat-completions",
                "/chat/completions",
                "Authorization",
            ),
            (
                "openai-responses",
                "responses",
                "/responses",
                "Authorization",
            ),
            ("anthropic", "messages", "/messages", "x-api-key"),
        ] {
            let fixture = Fixture::new();
            apply_at(
                &fixture.0,
                &info(protocol, "fixture-model", "https://api.example/v1"),
            )
            .unwrap();
            let groups = fixture.value("chatLanguageModels.json", true);
            let group = &groups[0];
            let model = &group["models"][0];
            assert_eq!(group["name"], GROUP);
            assert_eq!(group["vendor"], VENDOR);
            assert_eq!(group["apiType"], api_type);
            assert!(group.get("apiKey").is_none());
            assert!(group.get("url").is_none());
            assert_eq!(model["id"], "fixture-model");
            assert_eq!(model["url"], format!("https://api.example/v1{suffix}"));
            assert_eq!(model["toolCalling"], true);
            assert_eq!(model["vision"], false);
            assert_eq!(model["maxInputTokens"], 28672);
            assert_eq!(model["maxOutputTokens"], 4096);
            assert_eq!(
                model["requestHeaders"][auth],
                if protocol == "anthropic" {
                    "fixture-key"
                } else {
                    "Bearer fixture-key"
                }
            );
            assert_eq!(model["requestHeaders"].as_object().unwrap().len(), 1);
            assert_eq!(
                fixture.value("settings.json", false)[DEFAULT_MODEL],
                "fixture-model"
            );
            let active = read_at(&fixture.0).unwrap();
            assert_eq!(active.model.as_deref(), Some("fixture-model"));
            assert_eq!(active.protocol.as_deref(), Some(protocol));
            assert_eq!(active.api_key.as_deref(), Some("fixture-key"));
            let endpoint = if protocol == "anthropic" {
                active.anthropic_url
            } else {
                active.base_url
            };
            assert_eq!(endpoint.unwrap(), format!("https://api.example/v1{suffix}"));
        }
    }

    #[test]
    fn endpoints_are_explicit_and_anthropic_url_takes_precedence() {
        for (protocol, base, expected) in [
            (
                "openai",
                "https://api.example",
                "https://api.example/chat/completions",
            ),
            (
                "openai",
                "https://api.example/v2/",
                "https://api.example/v2/chat/completions",
            ),
            (
                "openai",
                "https://api.example/custom/api/",
                "https://api.example/custom/api/chat/completions",
            ),
            (
                "openai",
                "https://api.example/v1beta/openai",
                "https://api.example/v1beta/openai/chat/completions",
            ),
            (
                "openai",
                "https://api.example/v1/chat/completions?api-version=test",
                "https://api.example/v1/chat/completions?api-version=test",
            ),
            (
                "openai-responses",
                "https://api.example",
                "https://api.example/responses",
            ),
            (
                "openai-responses",
                "https://api.example/custom/api/?api-version=test",
                "https://api.example/custom/api/responses?api-version=test",
            ),
            (
                "openai-responses",
                "https://api.example/v1/responses",
                "https://api.example/v1/responses",
            ),
            (
                "anthropic",
                "https://api.example/bridge",
                "https://api.example/bridge/v1/messages",
            ),
            (
                "anthropic",
                "https://api.example/bridge/v2/",
                "https://api.example/bridge/v2/messages",
            ),
        ] {
            assert_eq!(
                provider(&info(protocol, "fixture", base)).unwrap()["models"][0]["url"],
                expected
            );
        }
        let mut model = info("anthropic", "fixture", "https://openai.example/v1");
        model.anthropic_url = Some("https://anthropic.example/bridge/v1/messages".into());
        assert_eq!(
            provider(&model).unwrap()["models"][0]["url"],
            "https://anthropic.example/bridge/v1/messages"
        );
        for model in [
            info("unsupported", "fixture", "https://api.example/v1"),
            info("openai", "", "https://api.example/v1"),
            info("openai", "fixture", "file:///fixture"),
            info("openai", "fixture", "https://api.example/v1/messages"),
        ] {
            assert!(provider(&model).is_err());
        }
    }

    #[test]
    fn official_deepseek_models_use_the_published_token_window() {
        for protocol in ["openai", "openai-responses", "anthropic"] {
            for id in [
                "deepseek-flash",
                "deepseek-v4-pro",
                "deepseek-v4-flash",
                "deepseek-v4-flash-vision-exp",
            ] {
                let group = provider(&info(protocol, id, "https://api.deepseek.com")).unwrap();
                let model = &group["models"][0];
                assert_eq!(model["id"], id);
                assert_eq!(model["maxInputTokens"], 655360);
                assert_eq!(model["maxOutputTokens"], 393216);
                assert_eq!(model["vision"], id != "deepseek-v4-pro");
                assert_eq!(
                    model["maxInputTokens"].as_u64().unwrap()
                        + model["maxOutputTokens"].as_u64().unwrap(),
                    1048576
                );
            }
        }
        let fixture = Fixture::new();
        let model = info("openai", "deepseek-flash", "https://api.deepseek.com");
        let mut old_group = provider(&model).unwrap();
        old_group["models"][0]["maxInputTokens"] = json!(28672);
        old_group["models"][0]["maxOutputTokens"] = json!(4096);
        fixture.save("chatLanguageModels.json", &json!([old_group]).to_string());
        fixture.save("settings.json", "{\"chat.defaultModel\":\"original\"}");
        apply_at(&fixture.0, &model).unwrap();
        assert_eq!(
            fixture.value("chatLanguageModels.json", true)[0]["models"][0]["maxInputTokens"],
            655360
        );
        restore_at(&fixture.0).unwrap();
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "original"
        );
    }

    #[test]
    fn documented_native_catalog_models_reuse_capacity_and_image_support() {
        for (id, vision) in [("mimo-v2.6-pro", true), ("mimo-v2.5-pro", false)] {
            let group = provider(&info("openai", id, "https://api.xiaomimimo.com/v1")).unwrap();
            let model = &group["models"][0];
            assert_eq!(model["maxInputTokens"], 1044480);
            assert_eq!(model["maxOutputTokens"], 4096);
            assert_eq!(model["vision"], vision);
        }
        for (id, base) in [
            ("deepseek-chat", "https://api.deepseek.com"),
            ("deepseek/deepseek-flash", "https://api.deepseek.com"),
            ("deepseek-flash", "https://api.deepseek.com.example"),
            ("deepseek-flash", "https://api.reseller.example"),
            ("mimo-future", "https://api.xiaomimimo.com/v1"),
            ("mimo-v2.6-pro", "https://api.xiaomimimo.com.example/v1"),
            ("mimo-v2.6-pro", "https://api.reseller.example/v1"),
        ] {
            let group = provider(&info("openai", id, base)).unwrap();
            let model = &group["models"][0];
            assert_eq!(model["maxInputTokens"], 28672);
            assert_eq!(model["maxOutputTokens"], 4096);
            assert_eq!(model["vision"], false);
        }
    }

    #[test]
    fn repeated_switches_preserve_jsonc_and_the_original_default() {
        let fixture = Fixture::new();
        let personal = "{ \"vendor\": \"other\", \"name\": \"Personal\", \"models\": [{\"id\":\"personal-model\"}] }";
        fixture.save("settings.json", "\u{feff}{\r\n  // keep editor settings\r\n  \"editor.fontSize\": 17,\r\n  \"chat.defaultModel\": \"original\",\r\n}\r\n");
        fixture.save(
            "chatLanguageModels.json",
            &format!("[\n // keep personal provider\n {personal},\n]\n"),
        );
        apply_at(
            &fixture.0,
            &info("openai", "first", "https://first.example/v1"),
        )
        .unwrap();
        apply_at(
            &fixture.0,
            &info("anthropic", "second", "https://second.example/v1"),
        )
        .unwrap();
        apply_at(
            &fixture.0,
            &info("anthropic", "second", "https://second.example/v1"),
        )
        .unwrap();
        let settings = fs::read_to_string(fixture.0.join("settings.json")).unwrap();
        let models = fs::read_to_string(fixture.0.join("chatLanguageModels.json")).unwrap();
        assert!(settings.starts_with('\u{feff}'));
        assert!(settings.contains("// keep editor settings\r\n"));
        assert!(settings.contains("\"editor.fontSize\": 17,"));
        assert!(settings.contains("\"chat.defaultModel\": \"second\",\r\n}"));
        assert!(models.contains("// keep personal provider"));
        assert!(models.contains(personal));
        assert_eq!(
            fixture
                .value("chatLanguageModels.json", true)
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let backup: DefaultBackup =
            serde_json::from_slice(&fs::read(fixture.0.join(BACKUP)).unwrap()).unwrap();
        assert_eq!(backup.previous_default.as_deref(), Some("original"));
        assert_eq!(backup.applied_model, "second");
        restore_at(&fixture.0).unwrap();
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "original"
        );
        assert_eq!(
            fixture
                .value("chatLanguageModels.json", true)
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            fs::read_to_string(fixture.0.join("chatLanguageModels.json"))
                .unwrap()
                .contains(personal)
        );
        assert!(!fixture.0.join(BACKUP).exists());
    }

    #[test]
    fn restore_clears_an_originally_absent_default_and_preserves_later_user_choices() {
        let fixture = Fixture::new();
        fixture.save("settings.json", "{\"editor.fontSize\":17}");
        let model = info("openai", "fixture", "http://localhost:8080/v1");
        apply_at(&fixture.0, &model).unwrap();
        restore_at(&fixture.0).unwrap();
        assert!(fixture
            .value("settings.json", false)
            .get(DEFAULT_MODEL)
            .is_none());
        assert_eq!(fixture.value("settings.json", false)["editor.fontSize"], 17);
        apply_at(&fixture.0, &model).unwrap();
        fixture.save(
            "settings.json",
            "{\"chat.defaultModel\":\"user-choice\",\"editor.fontSize\":19}",
        );
        restore_at(&fixture.0).unwrap();
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "user-choice"
        );
        assert_eq!(fixture.value("settings.json", false)["editor.fontSize"], 19);
    }

    #[test]
    fn same_raw_id_in_another_group_fails_without_writes() {
        let fixture = Fixture::new();
        let original =
            "[{\"vendor\":\"openai\",\"name\":\"Personal\",\"models\":[{\"id\":\"same-id\"}]}]";
        fixture.save("chatLanguageModels.json", original);
        fixture.save("settings.json", "{\"chat.defaultModel\":\"personal\"}");
        let error = apply_at(
            &fixture.0,
            &info("openai", "same-id", "https://selected.example/v1"),
        )
        .unwrap_err();
        assert!(error.contains("another provider group"));
        assert_eq!(
            fs::read_to_string(fixture.0.join("chatLanguageModels.json")).unwrap(),
            original
        );
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "personal"
        );
        assert!(!fixture.0.join(BACKUP).exists());
        assert!(apply_at(
            &fixture.0,
            &info("openai", "  SAME-ID  ", "https://selected.example/v1")
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(fixture.0.join("chatLanguageModels.json")).unwrap(),
            original
        );
    }

    #[test]
    fn the_same_owned_model_can_switch_endpoints_without_duplicate_groups() {
        let fixture = Fixture::new();
        for base in ["https://first.example/v1", "https://second.example/v1"] {
            apply_at(&fixture.0, &info("openai", "same-id", base)).unwrap();
        }
        let groups = fixture.value("chatLanguageModels.json", true);
        assert_eq!(groups.as_array().unwrap().len(), 1);
        assert_eq!(
            groups[0]["models"][0]["url"],
            "https://second.example/v1/chat/completions"
        );
    }

    #[test]
    fn invalid_configuration_is_never_replaced() {
        for (file, text) in [
            ("settings.json", "{ broken"),
            ("settings.json", "[]"),
            ("settings.json", "{\"chat.defaultModel\":false}"),
            (
                "settings.json",
                "{\"chat.defaultModel\":\"one\",\"chat.defaultModel\":\"two\"}",
            ),
            ("chatLanguageModels.json", "{}"),
            ("chatLanguageModels.json", "[false]"),
            (BACKUP, "{\"version\":99}"),
        ] {
            let fixture = Fixture::new();
            fixture.save(file, text);
            assert!(apply_at(
                &fixture.0,
                &info("openai", "fixture", "https://api.example/v1")
            )
            .is_err());
            assert_eq!(fs::read_to_string(fixture.0.join(file)).unwrap(), text);
            if file != BACKUP {
                assert!(!fixture.0.join(BACKUP).exists());
            }
            if file != "settings.json" {
                assert!(!fixture.0.join("settings.json").exists());
            }
        }
    }

    #[test]
    fn existing_profiles_switch_together_and_all_validate_before_writing() {
        let fixture = Fixture::new();
        fixture.save(
            "settings.json",
            "{\"chat.defaultModel\":\"default-original\"}",
        );
        fixture.save(
            "profiles/work/settings.json",
            "{\"chat.defaultModel\":\"work-original\",\"editor.fontSize\":18}",
        );
        fixture.save("profiles/personal/chatLanguageModels.json", "[]");
        let extensions = "{\"extensions\":[\"keep\"]}";
        fixture.save("profiles/extensions-only/extensions.json", extensions);
        fs::create_dir_all(fixture.0.join("profiles/empty")).unwrap();
        let model = info("openai", "fixture", "https://api.example/v1");
        apply_at(&fixture.0, &model).unwrap();
        for relative in [
            "settings.json",
            "profiles/work/settings.json",
            "profiles/personal/settings.json",
            "profiles/extensions-only/settings.json",
            "profiles/empty/settings.json",
        ] {
            assert_eq!(fixture.value(relative, false)[DEFAULT_MODEL], "fixture");
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("profiles/extensions-only/extensions.json")).unwrap(),
            extensions
        );
        assert!(!fixture.0.join("profiles/not-created").exists());
        restore_at(&fixture.0).unwrap();
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "default-original"
        );
        assert_eq!(
            fixture.value("profiles/work/settings.json", false)[DEFAULT_MODEL],
            "work-original"
        );
        assert_eq!(
            fixture.value("profiles/work/settings.json", false)["editor.fontSize"],
            18
        );
        assert!(fixture
            .value("profiles/personal/settings.json", false)
            .get(DEFAULT_MODEL)
            .is_none());
        for profile in ["extensions-only", "empty"] {
            assert!(fixture
                .value(&format!("profiles/{profile}/settings.json"), false)
                .get(DEFAULT_MODEL)
                .is_none());
            assert_eq!(
                fixture.value(&format!("profiles/{profile}/chatLanguageModels.json"), true),
                json!([])
            );
            assert!(!fixture
                .0
                .join("profiles")
                .join(profile)
                .join(BACKUP)
                .exists());
        }
        assert_eq!(
            fs::read_to_string(fixture.0.join("profiles/extensions-only/extensions.json")).unwrap(),
            extensions
        );
        assert!(!fixture.0.join("profiles/not-created").exists());
        fixture.save("profiles/work/settings.json", "{ broken");
        let before = fs::read(fixture.0.join("settings.json")).unwrap();
        assert!(apply_at(&fixture.0, &model).is_err());
        assert_eq!(fs::read(fixture.0.join("settings.json")).unwrap(), before);
        assert!(!fixture.0.join(BACKUP).exists());
    }

    #[test]
    fn write_failure_rolls_back_every_prior_file_and_positive_control_reaches_settings() {
        let fixture = Fixture::new();
        fixture.save("settings.json", "{\"chat.defaultModel\":\"original\"}");
        fixture.save("chatLanguageModels.json", "[ /* keep */ ]");
        let before_models = fs::read(fixture.0.join("chatLanguageModels.json")).unwrap();
        let before_settings = fs::read(fixture.0.join("settings.json")).unwrap();
        let group = provider(&info("openai", "fixture", "https://api.example/v1")).unwrap();
        let changes = apply_changes(&fixture.0, &group).unwrap();
        let mut writes = 0;
        let error = commit(changes, |path, bytes| {
            writes += 1;
            if writes == 3 {
                assert_eq!(path, fixture.0.join("settings.json"));
                return Err("fixture write failure".into());
            }
            replace(path, bytes)
        })
        .unwrap_err();
        assert_eq!(error, "fixture write failure");
        assert_eq!(writes, 5);
        assert_eq!(
            fs::read(fixture.0.join("chatLanguageModels.json")).unwrap(),
            before_models
        );
        assert_eq!(
            fs::read(fixture.0.join("settings.json")).unwrap(),
            before_settings
        );
        assert!(!fixture.0.join(BACKUP).exists());
        let changes = apply_changes(&fixture.0, &group).unwrap();
        let mut writes = 0;
        commit(changes, |path, bytes| {
            writes += 1;
            replace(path, bytes)
        })
        .unwrap();
        assert_eq!(writes, 3);
        assert_eq!(
            fixture.value("settings.json", false)[DEFAULT_MODEL],
            "fixture"
        );
    }

    #[test]
    fn passive_read_does_not_create_files_or_require_authentication() {
        let fixture = Fixture::new();
        assert!(read_at(&fixture.0).is_none());
        assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
        let mut model = info("openai", "local", "http://127.0.0.1:8080/v1");
        model.api_key = None;
        apply_at(&fixture.0, &model).unwrap();
        let active = read_at(&fixture.0).unwrap();
        assert_eq!(active.model.as_deref(), Some("local"));
        assert!(active.api_key.is_none());
        assert_eq!(
            fixture.value("chatLanguageModels.json", true)[0]["models"][0]["requestHeaders"],
            json!({})
        );
        fixture.save("settings.json", "{\"chat.defaultModel\":\" LOCAL \"}");
        assert_eq!(read_at(&fixture.0).unwrap().model.as_deref(), Some("local"));
        restore_at(&fixture.0).unwrap();
        let before = fs::read(fixture.0.join("settings.json")).unwrap();
        restore_at(&fixture.0).unwrap();
        assert_eq!(fs::read(fixture.0.join("settings.json")).unwrap(), before);
    }

    #[test]
    fn standard_and_portable_paths_cover_all_three_platform_layouts() {
        let fixture = Fixture::new();
        for config in [
            "AppData/Roaming",
            "Library/Application Support",
            ".config",
            "xdg-config",
        ] {
            let config = fixture.0.join(config);
            assert_eq!(
                resolve_user_dir(config.clone(), None),
                config.join("Code/User")
            );
        }
        for exe in ["windows/Code.exe", "linux/code"] {
            fixture.save(exe, "fixture");
            let exe = fixture.0.join(exe);
            let portable = exe.parent().unwrap().join("data/user-data/User");
            fs::create_dir_all(&portable).unwrap();
            assert_eq!(
                fs::canonicalize(resolve_user_dir(fixture.0.join("fallback"), Some(&exe))).unwrap(),
                fs::canonicalize(portable).unwrap()
            );
        }
        fixture.save("linux/bin/code", "fixture");
        fixture.save(
            "linux/resources/app/package.json",
            "{\"version\":\"1.127.0\"}",
        );
        assert_eq!(
            fs::canonicalize(resolve_user_dir(
                fixture.0.join("fallback"),
                Some(&fixture.0.join("linux/bin/code"))
            ))
            .unwrap(),
            fs::canonicalize(fixture.0.join("linux/data/user-data/User")).unwrap()
        );
        fixture.save(
            "mac/Visual Studio Code.app/Contents/MacOS/Electron",
            "fixture",
        );
        let exe = fixture
            .0
            .join("mac/Visual Studio Code.app/Contents/MacOS/Electron");
        let portable = fixture.0.join("mac/code-portable-data/user-data/User");
        fs::create_dir_all(&portable).unwrap();
        assert_eq!(
            fs::canonicalize(resolve_user_dir(fixture.0.join("fallback"), Some(&exe))).unwrap(),
            fs::canonicalize(portable).unwrap()
        );
    }

    #[test]
    fn unstarted_portable_installs_resolve_without_writes_and_apply_to_their_own_data() {
        let fixture = Fixture::new();
        let fallback = fixture.0.join("fallback");
        for (exe, data) in [
            ("windows/Code.exe", "windows/data"),
            ("linux/code", "linux/data"),
            (
                "mac/Visual Studio Code.app/Contents/MacOS/Electron",
                "mac/code-portable-data",
            ),
        ] {
            fixture.save(exe, "fixture");
            let data = fixture.0.join(data);
            fs::create_dir_all(&data).unwrap();
            let user = resolve_user_dir(fallback.clone(), Some(&fixture.0.join(exe)));
            assert_eq!(
                user,
                fs::canonicalize(&data).unwrap().join("user-data/User")
            );
            assert!(!user.exists());
            assert!(read_at(&user).is_none());
            assert!(!user.exists());
            apply_at(
                &user,
                &info("openai", "portable-model", "https://api.example/v1"),
            )
            .unwrap();
            assert_eq!(
                read_at(&user).unwrap().model.as_deref(),
                Some("portable-model")
            );
            assert!(user.join("settings.json").is_file());
            assert!(user.join("chatLanguageModels.json").is_file());
            assert!(user.join(BACKUP).is_file());
            assert!(!fallback.join("Code/User").exists());
        }
    }

    #[test]
    fn package_metadata_checks_the_active_versioned_install_and_rejects_old_or_unknown_versions() {
        let fixture = Fixture::new();
        fixture.save("windows/Code.exe", "fixture");
        fixture.save(
            "windows/resources/app/package.json",
            "{\"version\":\"1.126.9\"}",
        );
        let exe = fixture.0.join("windows/Code.exe");
        assert_eq!(installed_version(&exe).as_deref(), Some("1.126.9"));
        fixture.save(
            "windows/bin/code.cmd",
            "\"%~dp0..\\Code.exe\" \"%~dp0..\\active\\resources\\app\\out\\cli.js\" %*",
        );
        fixture.save(
            "windows/active/resources/app/package.json",
            "{\"version\":\"1.134.1\"}",
        );
        fixture.save(
            "windows/stale-newer/resources/app/package.json",
            "{\"version\":\"1.135.0\"}",
        );
        assert_eq!(installed_version(&exe).as_deref(), Some("1.134.1"));
        fixture.save(
            "mac/Visual Studio Code.app/Contents/MacOS/Electron",
            "fixture",
        );
        fixture.save(
            "mac/Visual Studio Code.app/Contents/Resources/app/package.json",
            "{\"version\":\"1.127.0\"}",
        );
        assert_eq!(
            installed_version(
                &fixture
                    .0
                    .join("mac/Visual Studio Code.app/Contents/MacOS/Electron")
            )
            .as_deref(),
            Some("1.127.0")
        );
        for version in ["1.127.0", "1.134.1", "2.0.0"] {
            assert!(supports_default_model(version));
        }
        for version in ["1.126.9", "0.200.0", "", "unknown"] {
            assert!(!supports_default_model(version));
        }
        assert!(installed_version(&fixture.0.join("missing/Code.exe")).is_none());
        fixture.save(
            "snap-current/usr/share/code/resources/app/package.json",
            "{\"version\":\"1.134.1\"}",
        );
        assert_eq!(
            installed_version_at(Path::new("/snap/bin/code"), &fixture.0.join("snap-current"))
                .as_deref(),
            Some("1.134.1")
        );
    }
}
