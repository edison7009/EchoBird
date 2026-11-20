//! Model configuration for claudecode.

#[cfg(test)]
use super::write_json_file;
use super::{echobird_dir, read_json_file, ApplyResult, ModelInfo};
use crate::services::native_account_files;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fs, path::Path, path::PathBuf};

static CONFIG_MUTATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_and_recover_selection(
    settings_path: &Path,
    relay_path: &Path,
) -> Result<native_account_files::DirectoryLock, String> {
    let lock = native_account_files::DirectoryLock::acquire(relay_path)?;
    native_account_files::recover_file_mutations(
        &relay_path.with_extension("pending.json"),
        &[settings_path, relay_path],
    )?;
    Ok(lock)
}

pub(crate) fn recover_interrupted_selection() -> Result<(), String> {
    let relay = echobird_dir().join("claudecode.json");
    if !native_account_files::has_file_mutations(&relay.with_extension("pending.json"))? {
        return Ok(());
    }
    let settings = config_dir()?.join("settings.json");
    let _lock = lock_and_recover_selection(&settings, &relay)?;
    Ok(())
}

pub(super) fn normalize_model_info_for_tool(tool_id: &str, mut model_info: ModelInfo) -> ModelInfo {
    if tool_id == "claudecode" && model_info.protocol.as_deref() == Some("anthropic") {
        if let Some(ref mut base_url) = model_info.base_url {
            let trimmed = base_url.trim_end_matches('/').to_string();
            if let Some(without_v1) = trimmed.strip_suffix("/v1") {
                *base_url = without_v1.to_string();
            } else {
                *base_url = trimmed;
            }
        }
    }
    model_info
}

// Model-tier env vars Claude Code consults. Pin supported tiers to the real
// upstream model. Hoisted to module
// scope so the set is lockable by tests — the apply path writes to
// ~/.claude/settings.json via dirs::home_dir(), which isn't injectable.
//
// `ANTHROPIC_SMALL_FAST_MODEL` is deliberately NOT here — Claude Code
// deprecated it in favor of `ANTHROPIC_DEFAULT_HAIKU_MODEL` (still pinned).
// `CLAUDE_CODE_SUBAGENT_MODEL` IS here so applies pin subagents to the
// upstream model too (otherwise subagents fall back to claude-* ids and
// bypass the third-party router, breaking the "全量" write-in).
// https://code.claude.com/docs/en/model-config
const CLAUDECODE_MODEL_VARS: [&str; 6] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

/// The 1M-capable subset of [`CLAUDECODE_MODEL_VARS`]: env vars that receive
/// a `[1m]` suffix when the user opts into the 1M context window. `HAIKU` and
/// `CLAUDE_CODE_SUBAGENT_MODEL` are deliberately excluded — Haiku has no 1M
/// tier, and subagents pin to the bare upstream id.
const CLAUDECODE_MODEL_VARS_1M: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
];

const AUTO_MODE_SERVER: &str = "CLAUDE_CODE_AUTO_MODE_SERVER";

const CLOUD_PROVIDER_VARS: [&str; 6] = [
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "CLAUDE_CODE_USE_ANTHROPIC_AWS",
    "CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD",
    "CLAUDE_CODE_USE_MANTLE",
];

fn cloud_switch_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn inherited_provider(
    get: impl Fn(&str) -> Option<String>,
    env: Option<&Map<String, Value>>,
) -> Option<&'static str> {
    CLOUD_PROVIDER_VARS.into_iter().find(|key| {
        let overridden = env
            .and_then(|env| env.get(*key))
            .and_then(Value::as_str)
            .is_some_and(|value| !cloud_switch_enabled(value));
        !overridden && get(key).is_some_and(|value| cloud_switch_enabled(&value))
    })
}

fn managed_env_keys() -> impl Iterator<Item = &'static str> {
    CLAUDECODE_MODEL_VARS
        .into_iter()
        .chain(CLOUD_PROVIDER_VARS)
        .chain([
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "API_TIMEOUT_MS",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
            AUTO_MODE_SERVER,
        ])
}

fn managed_env(env: &Map<String, Value>) -> Map<String, Value> {
    managed_env_keys()
        .filter_map(|key| env.get(key).map(|value| (key.to_string(), value.clone())))
        .collect()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WebSearchPolicy {
    permissions_existed: bool,
    deny_existed: bool,
    original_positions: Vec<usize>,
    applied_denied: bool,
    applied_deny_existed: bool,
}

fn web_search_denials(config: &Value) -> Result<Vec<Value>, String> {
    let Some(permissions) = config.get("permissions") else {
        return Ok(Vec::new());
    };
    if permissions.is_object() {
        match permissions.get("deny") {
            None => return Ok(Vec::new()),
            Some(Value::Array(rules)) if rules.iter().all(Value::is_string) => {
                return Ok(rules.clone())
            }
            _ => {}
        }
    }
    Err("Claude Code's search permission settings are invalid. Nothing was changed.".into())
}

impl WebSearchPolicy {
    fn write_rules(&self, config: &mut Value, rules: Vec<Value>) {
        if !rules.is_empty() || self.deny_existed {
            let permissions = config
                .as_object_mut()
                .unwrap()
                .entry("permissions")
                .or_insert_with(|| serde_json::json!({}));
            permissions["deny"] = Value::Array(rules);
        } else if let Some(permissions) =
            config.get_mut("permissions").and_then(Value::as_object_mut)
        {
            permissions.remove("deny");
        }
        if !self.permissions_existed
            && config
                .get("permissions")
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
        {
            config.as_object_mut().unwrap().remove("permissions");
        }
    }

    fn apply(config: &mut Value, enabled: bool, previous: Option<Self>) -> Result<Self, String> {
        let mut rules = web_search_denials(config)?;
        let positions: Vec<_> = rules
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| (rule == "WebSearch").then_some(index))
            .collect();
        let deny_existed = config.pointer("/permissions/deny").is_some();
        let mut policy = previous
            .filter(|policy| {
                policy.applied_denied != positions.is_empty()
                    && policy.applied_deny_existed == deny_existed
            })
            .unwrap_or_else(|| Self {
                permissions_existed: config.get("permissions").is_some(),
                deny_existed,
                original_positions: positions,
                applied_denied: false,
                applied_deny_existed: deny_existed,
            });
        rules.retain(|rule| rule != "WebSearch");
        if !enabled {
            rules.push(Value::String("WebSearch".into()));
        }
        policy.write_rules(config, rules);
        policy.applied_denied = !enabled;
        policy.applied_deny_existed = config.pointer("/permissions/deny").is_some();
        Ok(policy)
    }

    fn restore(&self, config: &mut Value) {
        let Ok(mut rules) = web_search_denials(config) else {
            return;
        };
        if rules.iter().any(|rule| rule == "WebSearch") != self.applied_denied
            || config.pointer("/permissions/deny").is_some() != self.applied_deny_existed
        {
            return;
        }
        rules.retain(|rule| rule != "WebSearch");
        for index in &self.original_positions {
            rules.insert((*index).min(rules.len()), Value::String("WebSearch".into()));
        }
        self.write_rules(config, rules);
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedSettings {
    version: u8,
    path: PathBuf,
    settings_existed: bool,
    env_existed: bool,
    original_env: Map<String, Value>,
    applied_env: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    web_search: Option<WebSearchPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    original_model_picker: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    applied_model_picker: Option<Value>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    relay: Map<String, Value>,
}

fn relay_fields(relay: &Value) -> Map<String, Value> {
    [
        "baseUrl",
        "apiKey",
        "actualModel",
        "modelName",
        "relayMode",
        "fableTierManaged",
        "autoModeServerSettingsPath",
    ]
    .into_iter()
    .filter_map(|key| relay.get(key).map(|value| (key.to_string(), value.clone())))
    .collect()
}

fn saved_settings(relay: &Value) -> Result<Vec<ManagedSettings>, String> {
    let mut profiles: Vec<ManagedSettings> = serde_json::from_value(
        relay
            .get("otherSettings")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|_| "Claude Code's saved model configuration is invalid. Nothing was changed.")?;
    if let Some(value) = relay.get("managedSettings") {
        let mut current: ManagedSettings = serde_json::from_value(value.clone()).map_err(|_| {
            "Claude Code's saved model configuration is invalid. Nothing was changed."
        })?;
        current.relay = relay_fields(relay);
        profiles.push(current);
    }
    if profiles.iter().enumerate().any(|(index, saved)| {
        saved.version != 1
            || saved.path.as_os_str().is_empty()
            || saved.relay.get("baseUrl").and_then(Value::as_str).is_none()
            || saved.relay.get("apiKey").and_then(Value::as_str).is_none()
            || saved
                .relay
                .get("actualModel")
                .and_then(Value::as_str)
                .is_none()
            || saved
                .original_env
                .keys()
                .chain(saved.applied_env.keys())
                .any(|key| !managed_env_keys().any(|known| known == key))
            || profiles[..index]
                .iter()
                .any(|other| other.path == saved.path)
    }) {
        return Err(
            "Claude Code's saved model configuration is invalid. Nothing was changed.".into(),
        );
    }
    Ok(profiles)
}

fn read_object(path: &Path) -> Result<(Option<Vec<u8>>, Value), String> {
    let raw = native_account_files::read_optional(path)
        .map_err(|_| format!("Cannot read Claude Code configuration: {}", path.display()))?;
    if raw.is_none() && fs::symlink_metadata(path).is_ok() {
        return Err(format!(
            "Cannot read Claude Code configuration: {}",
            path.display()
        ));
    }
    let value = match raw.as_deref() {
        Some(bytes) => serde_json::from_slice::<Value>(bytes)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| format!("Invalid Claude Code configuration: {}", path.display()))?,
        None => serde_json::json!({}),
    };
    Ok((raw, value))
}

fn config_error(message: impl Into<String>) -> ApplyResult {
    ApplyResult {
        success: false,
        message: message.into(),
    }
}

/// Value written to a `CLAUDECODE_MODEL_VARS` entry. Appends
/// `[1m]` only when the user opted in (`one_m`) AND `var` is in the 1M-capable
/// tier; `ANTHROPIC_DEFAULT_HAIKU_MODEL` / `CLAUDE_CODE_SUBAGENT_MODEL` always
/// return the bare id.
fn claudecode_env_model_id(real_id: &str, var: &str, one_m: bool) -> String {
    if one_m && CLAUDECODE_MODEL_VARS_1M.contains(&var) {
        format!("{}[1m]", real_id)
    } else {
        real_id.to_string()
    }
}

/// Connect Claude Code directly to the selected Anthropic endpoint, preserving
/// saved model identity and original settings for configuration reads/restore.
pub(super) fn apply_claudecode(model_info: &ModelInfo) -> ApplyResult {
    let config_dir = match config_dir() {
        Ok(dir) => dir,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    let settings_path = config_dir.join("settings.json");
    apply_claudecode_with_environment(
        model_info,
        &settings_path,
        &echobird_dir().join("claudecode.json"),
        |key| std::env::var(key).ok(),
    )
}

#[cfg(test)]
fn apply_claudecode_at(
    model_info: &ModelInfo,
    settings_path: &std::path::Path,
    relay_path: &std::path::Path,
) -> ApplyResult {
    apply_claudecode_with_environment(model_info, settings_path, relay_path, |_| None)
}

fn apply_claudecode_with_environment(
    model_info: &ModelInfo,
    settings_path: &Path,
    relay_path: &Path,
    inherited: impl Fn(&str) -> Option<String>,
) -> ApplyResult {
    let _mutation = match CONFIG_MUTATIONS.lock() {
        Ok(guard) => guard,
        Err(_) => return config_error("Claude Code's previous configuration update was interrupted. Restart EchoBird before trying again."),
    };
    // Frontend collapses the chosen protocol's URL into base_url (same
    // convention as apply_claudedesktop). Accept either field.
    let anthropic_url = model_info
        .anthropic_url
        .as_deref()
        .or(model_info.base_url.as_deref())
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| {
            // Trim a trailing '/' and a trailing '/v1' so
            // Claude Code's SDK appending '/v1/messages' to ANTHROPIC_BASE_URL
            // can't produce a doubled '/v1/v1/messages'. normalize already does
            // this for base_url; do it here too so the anthropic_url branch is
            // equally safe.
            let u = u.trim_end_matches('/');
            u.strip_suffix("/v1")
                .unwrap_or(u)
                .trim_end_matches('/')
                .to_string()
        });
    let anthropic_url = match anthropic_url {
        Some(u) => u,
        None => {
            return ApplyResult {
                success: false,
                message: "Base URL is empty. Pick a model first.".to_string(),
            }
        }
    };

    // Local providers (loopback) legitimately have no key — substitute a
    // sentinel so the relay always has something to forward as Bearer.
    let raw_api_key = model_info.api_key.as_deref().unwrap_or("");
    let is_local_provider =
        anthropic_url.contains("127.0.0.1") || anthropic_url.contains("localhost");
    let api_key = if raw_api_key.is_empty() {
        if is_local_provider {
            "local-no-auth".to_string()
        } else {
            return ApplyResult {
                success: false,
                message: "API Key is empty, cannot apply Claude Code config.".to_string(),
            };
        }
    } else {
        raw_api_key.to_string()
    };

    let real_model_id = model_info
        .model
        .as_deref()
        .or(model_info.name.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();

    let _selection_lock = match lock_and_recover_selection(settings_path, relay_path) {
        Ok(lock) => lock,
        Err(message) => return config_error(message),
    };
    let (previous_raw, mut config) = match read_object(settings_path) {
        Ok(value) => value,
        Err(message) => return config_error(message),
    };
    if config.get("env").is_some_and(|env| !env.is_object()) {
        return config_error(
            "Claude Code's env configuration isn't an object. Nothing was changed.",
        );
    }
    if let Some(key) = inherited_provider(inherited, config.get("env").and_then(Value::as_object)) {
        return config_error(format!(
            "Claude Code's {key} is enabled in EchoBird's environment. Disable it and restart EchoBird before applying a model."
        ));
    }
    let (previous_relay_raw, previous_relay) = match read_object(relay_path) {
        Ok(value) => value,
        Err(message) => return config_error(message),
    };
    let mut profiles = match saved_settings(&previous_relay) {
        Ok(value) => value,
        Err(message) => return config_error(message),
    };
    let legacy_relay = profiles.is_empty();
    let previous_settings = profiles
        .iter()
        .position(|saved| saved.path == settings_path)
        .map(|index| profiles.remove(index));
    let env_existed = config.get("env").is_some();
    config
        .as_object_mut()
        .unwrap()
        .entry("env")
        .or_insert_with(|| serde_json::json!({}));
    let env = config
        .get_mut("env")
        .and_then(|v| v.as_object_mut())
        .expect("env is an object");
    if previous_relay_raw.is_none() && uses_local_bridge(env) {
        return config_error("Claude Code still points to EchoBird, but its saved model configuration is missing. Repair its settings before applying a model.");
    }
    if !legacy_relay && previous_settings.is_none() && uses_local_bridge(env) {
        return config_error("Claude Code's original settings for this configuration directory are missing. Nothing was changed.");
    }
    let mut saved = previous_settings.unwrap_or_else(|| {
        let mut original_env = env.clone();
        if legacy_relay {
            restore_legacy_env(&mut original_env, &previous_relay, settings_path);
        }
        ManagedSettings {
            version: 1,
            path: settings_path.to_owned(),
            settings_existed: previous_raw.is_some(),
            env_existed,
            original_env: managed_env(&original_env),
            applied_env: managed_env(env),
            web_search: None,
            original_model_picker: None,
            applied_model_picker: None,
            relay: if legacy_relay {
                relay_fields(&previous_relay)
            } else {
                Map::new()
            },
        }
    });
    let previous_profile_relay = Value::Object(saved.relay.clone());
    for key in managed_env_keys() {
        if env.get(key) != saved.applied_env.get(key) {
            match env.get(key) {
                Some(value) => {
                    saved.original_env.insert(key.to_owned(), value.clone());
                }
                None => {
                    saved.original_env.remove(key);
                }
            }
        }
    }
    if previous_raw.is_none() {
        saved.settings_existed = false;
        saved.env_existed = false;
    } else if !env_existed {
        saved.env_existed = false;
    }

    for key in CLOUD_PROVIDER_VARS {
        if env.get(key).is_some_and(|value| match value {
            Value::String(value) => cloud_switch_enabled(value),
            Value::Bool(value) => *value,
            Value::Number(value) => value.as_u64() == Some(1),
            _ => false,
        }) {
            env.remove(key);
        }
    }

    // Shared knobs (match the long-standing generic claudecode mapping).
    env.insert(
        "API_TIMEOUT_MS".to_string(),
        serde_json::Value::String("3000000".to_string()),
    );
    env.insert(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
        serde_json::Value::String("1".to_string()),
    );

    // Gateways may not return server classifier verdicts. Preserve explicit
    // user values and track our default separately so official restore can
    // remove it, including after repeated applies.
    let auto_mode_inserted = !env.contains_key(AUTO_MODE_SERVER);
    let auto_mode_managed = auto_mode_inserted
        || (env.get(AUTO_MODE_SERVER).and_then(|v| v.as_str()) == Some("0")
            && previous_profile_relay["autoModeServerSettingsPath"]
                .as_str()
                .is_some_and(|path| Path::new(path) == settings_path));
    env.entry(AUTO_MODE_SERVER.to_string())
        .or_insert_with(|| serde_json::json!("0"));

    // Authenticate via ANTHROPIC_AUTH_TOKEN (Bearer), so always drop
    // any ANTHROPIC_API_KEY (x-api-key) the user may have left in settings.json —
    // a stale real key would otherwise conflict. We *remove* the key rather than
    // write an empty string (the old generic mapping could only set "", not
    // delete) so settings.json stays clean.
    env.remove("ANTHROPIC_API_KEY");

    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        serde_json::Value::String(anthropic_url.clone()),
    );
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        serde_json::Value::String(api_key.clone()),
    );
    let switch_models = match super::switching::models(model_info) {
        Ok(models) => models,
        Err(message) => return config_error(message),
    };
    let one_m = model_info.one_m_context.unwrap_or(false);
    let default_one_m = one_m
        && (switch_models.is_empty()
            || switch_models
                .iter()
                .any(|model| model.id == real_model_id && model.supports_claude_1m()));
    for var in CLAUDECODE_MODEL_VARS {
        env.insert(
            var.to_string(),
            serde_json::Value::String(claudecode_env_model_id(&real_model_id, var, default_one_m)),
        );
    }

    saved.applied_env = managed_env(env);
    if !switch_models.is_empty() {
        if saved.applied_model_picker.is_none()
            || config.get("modelPicker") != saved.applied_model_picker.as_ref()
        {
            saved.original_model_picker = config.get("modelPicker").cloned();
        }
        let options: Vec<_> = switch_models
            .into_iter()
            .map(|model| {
                serde_json::json!({
                    "model": claudecode_env_model_id(&model.id, "ANTHROPIC_MODEL", one_m && model.supports_claude_1m()),
                    "label": model.name,
                    "description": "EchoBird"
                })
            })
            .collect();
        let picker = serde_json::json!({"options":options});
        config["modelPicker"] = picker.clone();
        saved.applied_model_picker = Some(picker);
    } else if let Some(applied) = saved.applied_model_picker.take() {
        if config.get("modelPicker") == Some(&applied) {
            match saved.original_model_picker.take() {
                Some(original) => config["modelPicker"] = original,
                None => {
                    config.as_object_mut().unwrap().remove("modelPicker");
                }
            }
        }
    }
    if let Some(enabled) = model_info.web_search {
        saved.web_search =
            match WebSearchPolicy::apply(&mut config, enabled, saved.web_search.take()) {
                Ok(policy) => Some(policy),
                Err(message) => return config_error(message),
            };
    }
    saved.relay.clear();

    // Save actual identity and original settings for configuration reads/restore.
    let relay = serde_json::json!({
        "baseUrl": anthropic_url,
        "apiKey": api_key,
        "actualModel": real_model_id,
        "modelName": model_info.name.as_deref().unwrap_or(real_model_id.as_str()),
        "relayMode": true,
        "fableTierManaged": true,
        "autoModeServerSettingsPath": auto_mode_managed.then(|| settings_path.to_string_lossy()),
        "managedSettings": saved,
        "otherSettings": profiles,
    });
    let result = (|| {
        let mut transaction = native_account_files::Transaction::recoverable(
            &relay_path.with_extension("pending.json"),
            &[settings_path, relay_path],
        )?;
        if native_account_files::read_optional(relay_path)? != previous_relay_raw {
            return Err("Claude Code's model configuration changed. Try again.".into());
        }
        transaction.replace(relay_path, &native_account_files::json(&relay)?)?;
        #[cfg(test)]
        regression_tests::after_mutation_stage();
        if native_account_files::read_optional(settings_path)? != previous_raw {
            return Err("Claude Code's settings changed. Try again.".into());
        }
        transaction.replace_private_file(settings_path, &native_account_files::json(&config)?)?;
        transaction.commit_files()
    })();
    if let Err(error) = result {
        return config_error(format!(
            "Failed to apply Claude Code configuration: {error}"
        ));
    }

    ApplyResult {
        success: true,
        message:
            "Claude Code configured. Restart Claude Code (or /exit and reopen) for the change to take effect."
                .to_string(),
    }
}

pub(super) fn read_claudecode() -> Option<ModelInfo> {
    let settings_path = config_dir().ok()?.join("settings.json");
    read_claudecode_at(&settings_path, &echobird_dir().join("claudecode.json"))
}

fn read_claudecode_at(settings_path: &Path, relay_path: &Path) -> Option<ModelInfo> {
    // Source of truth is the relay file (written in both modes), NOT
    // settings.json — mirrors read_claudedesktop. Surfaces the real upstream
    // model so the App Desktop card shows it after a rescan.
    let mut relay = read_json_file(relay_path)?;
    let profiles = saved_settings(&relay).ok()?;
    if !profiles.is_empty() {
        let profile = profiles.iter().find(|saved| saved.path == settings_path)?;
        let (_, settings) = read_object(settings_path).ok()?;
        let env = settings.get("env")?.as_object()?;
        for key in ["ANTHROPIC_BASE_URL", "ANTHROPIC_AUTH_TOKEN"] {
            if env.get(key) != profile.applied_env.get(key) {
                return None;
            }
        }
        // Direct relays live in the client's own settings. Bridge clients all
        // use the one current top-level relay, regardless of their last pick.
        if profile.relay.get("relayMode") == Some(&Value::Bool(true)) {
            relay = Value::Object(profile.relay.clone());
        }
    }

    let anthropic_url = relay
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let api_key = relay
        .get("apiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let model = relay
        .get("actualModel")
        .and_then(|v| v.as_str())
        // Legacy tolerance: relay files written by ≤v5.3.8 (which had a 1M
        // toggle) may carry a `[1m]` suffix on actualModel. Strip it so the
        // surfaced active-model id matches the user's stored modelId —
        // otherwise the App Desktop card's "currently applied" highlight
        // fails to match after a rescan. Nothing writes the suffix any more.
        .map(|s| s.strip_suffix("[1m]").unwrap_or(s))
        .filter(|s| !s.is_empty())
        .map(String::from);
    let name = relay
        .get("modelName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);

    Some(ModelInfo {
        name,
        model,
        base_url: None,
        api_key,
        anthropic_url: Some(anthropic_url),
        protocol: Some("anthropic".to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
        image_input: None,
        one_m_context: None,
    })
}

/// Surgically remove the env keys we own from ~/.claude/settings.json and
/// drop the relay file. Unlike the generic restore (which deletes the whole
/// config file), this preserves allowedTools / hooks / anything else the user
/// keeps in settings.json — deleting it wholesale would wipe their Claude Code
/// setup, not just our model config.
pub(super) fn restore_claudecode_to_official() -> ApplyResult {
    let config_dir = match config_dir() {
        Ok(dir) => dir,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    restore_claudecode_at(
        &config_dir.join("settings.json"),
        &echobird_dir().join("claudecode.json"),
    )
}

fn restore_claudecode_at(
    settings_path: &std::path::Path,
    relay_path: &std::path::Path,
) -> ApplyResult {
    let _mutation = match CONFIG_MUTATIONS.lock() {
        Ok(guard) => guard,
        Err(_) => return config_error("Claude Code's previous configuration update was interrupted. Restart EchoBird before trying again."),
    };
    let result = (|| {
        let _selection_lock = lock_and_recover_selection(settings_path, relay_path)?;
        let (relay_raw, relay) = read_object(relay_path)?;
        let Some(relay_raw) = relay_raw else {
            let (_, config) = read_object(settings_path)?;
            if config
                .get("env")
                .and_then(Value::as_object)
                .is_some_and(uses_local_bridge)
            {
                return Err("Claude Code still points to EchoBird, but its saved model configuration is missing. Restore was stopped to preserve its settings.".into());
            }
            return Ok(());
        };
        let mut profiles = saved_settings(&relay)?;
        let managed = !profiles.is_empty();
        let saved = profiles
            .iter()
            .position(|saved| saved.path == settings_path)
            .map(|index| profiles.remove(index));
        if managed && saved.is_none() {
            return Err("Claude Code's saved model belongs to another configuration directory. Nothing was changed.".into());
        }
        let (settings_raw, mut config) = read_object(settings_path)?;
        if config.get("env").is_some_and(|env| !env.is_object()) {
            return Err(
                "Claude Code's env configuration isn't an object. Nothing was changed.".into(),
            );
        }
        if let Some(env) = config.get_mut("env").and_then(Value::as_object_mut) {
            if let Some(saved) = &saved {
                for key in managed_env_keys() {
                    if env.get(key) == saved.applied_env.get(key) {
                        match saved.original_env.get(key) {
                            Some(value) => {
                                env.insert(key.to_string(), value.clone());
                            }
                            None => {
                                env.remove(key);
                            }
                        }
                    }
                }
                if !saved.env_existed && env.is_empty() {
                    config.as_object_mut().unwrap().remove("env");
                }
            } else {
                restore_legacy_env(env, &relay, settings_path);
            }
        }
        if let Some(policy) = saved.as_ref().and_then(|saved| saved.web_search.as_ref()) {
            policy.restore(&mut config);
        }
        if let Some(saved) = &saved {
            if saved.applied_model_picker.is_some()
                && config.get("modelPicker") == saved.applied_model_picker.as_ref()
            {
                match &saved.original_model_picker {
                    Some(original) => config["modelPicker"] = original.clone(),
                    None => {
                        config.as_object_mut().unwrap().remove("modelPicker");
                    }
                }
            }
        }
        let next_settings = if settings_raw.is_none()
            || (saved.as_ref().is_some_and(|saved| !saved.settings_existed)
                && config.as_object().unwrap().is_empty())
        {
            None
        } else {
            Some(native_account_files::json(&config)?)
        };
        let next_relay = if let Some(mut remaining) = profiles.pop() {
            let mut relay = Value::Object(std::mem::take(&mut remaining.relay));
            relay["managedSettings"] = serde_json::to_value(remaining)
                .map_err(|_| "Cannot save Claude Code's remaining model configuration")?;
            relay["otherSettings"] = serde_json::to_value(profiles)
                .map_err(|_| "Cannot save Claude Code's remaining model configuration")?;
            Some(native_account_files::json(&relay)?)
        } else {
            None
        };
        if native_account_files::read_optional(settings_path)? != settings_raw
            || native_account_files::read_optional(relay_path)?.as_deref()
                != Some(relay_raw.as_slice())
        {
            return Err("Claude Code's configuration changed. Try again.".into());
        }
        let mut transaction = native_account_files::Transaction::recoverable(
            &relay_path.with_extension("pending.json"),
            &[settings_path, relay_path],
        )?;
        if settings_raw != next_settings {
            replace_config(&mut transaction, settings_path, next_settings.as_deref())?;
        }
        #[cfg(test)]
        regression_tests::after_mutation_stage();
        if native_account_files::read_optional(relay_path)?.as_deref() != Some(relay_raw.as_slice())
        {
            return Err("Claude Code's model configuration changed. Try again.".into());
        }
        replace_config(&mut transaction, relay_path, next_relay.as_deref()).map_err(|error| {
            format!("Cannot restore Claude Code's model configuration: {error}")
        })?;
        transaction.commit_files()
    })();
    if let Err(message) = result {
        return config_error(message);
    }

    ApplyResult {
        success: true,
        message: "Claude Code restored to official provider. Restart Claude Code for the change to take effect."
            .to_string(),
    }
}

fn replace_config(
    transaction: &mut native_account_files::Transaction,
    path: &Path,
    bytes: Option<&[u8]>,
) -> Result<(), String> {
    match bytes {
        Some(bytes) => transaction.replace_private_file(path, bytes),
        None => transaction.remove_private_file(path),
    }
}

fn uses_local_bridge(env: &Map<String, Value>) -> bool {
    env.get("ANTHROPIC_AUTH_TOKEN").and_then(Value::as_str) == Some("echobird-local-proxy")
        && env
            .get("ANTHROPIC_BASE_URL")
            .and_then(Value::as_str)
            .is_some_and(|url| url.starts_with("http://127.0.0.1:") && url.ends_with("/claudecode"))
}

fn restore_legacy_env(env: &mut Map<String, Value>, relay: &Value, settings_path: &Path) {
    // Older relays did not retain original values. Only remove fields whose
    // current values still identify the last applied configuration.
    let connected = if relay["relayMode"] == true {
        env.get("ANTHROPIC_BASE_URL") == relay.get("baseUrl")
            && env.get("ANTHROPIC_AUTH_TOKEN") == relay.get("apiKey")
            && relay["baseUrl"].is_string()
            && relay["apiKey"].is_string()
    } else {
        uses_local_bridge(env)
    };
    if !connected {
        return;
    }
    env.remove("ANTHROPIC_BASE_URL");
    env.remove("ANTHROPIC_AUTH_TOKEN");
    for key in CLAUDECODE_MODEL_VARS
        .into_iter()
        .chain(["ANTHROPIC_SMALL_FAST_MODEL"])
    {
        if env.get(key).and_then(Value::as_str).is_some_and(|value| {
            relay["actualModel"].as_str().is_some_and(|model| {
                value.strip_suffix("[1m]").unwrap_or(value)
                    == model.strip_suffix("[1m]").unwrap_or(model)
            })
        }) {
            env.remove(key);
        }
    }
    for (key, expected) in [
        ("API_TIMEOUT_MS", "3000000"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
    ] {
        if env.get(key).and_then(Value::as_str) == Some(expected) {
            env.remove(key);
        }
    }
    if env.get(AUTO_MODE_SERVER).and_then(Value::as_str) == Some("0")
        && relay["autoModeServerSettingsPath"]
            .as_str()
            .is_some_and(|path| Path::new(path) == settings_path)
    {
        env.remove(AUTO_MODE_SERVER);
    }
}

#[cfg(test)]
#[path = "claudecode_regression_tests.rs"]
mod regression_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_and_restores_within_the_selected_config_directory() {
        let dir = std::env::temp_dir().join(format!("claude-config-{}", uuid::Uuid::new_v4()));
        let relay = dir.join("relay.json");
        let unchanged = dir.join("unrelated/settings.json");
        let original = serde_json::json!({"permissions":{"allow":["Read"]}, "env":{"KEEP":"yes"}});
        write_json_file(&unchanged, &original).unwrap();
        for name in [".claude", "custom-profile"] {
            let path = dir.join(name).join("settings.json");
            write_json_file(&path, &original).unwrap();
            for mode in [false, true] {
                assert!(
                    apply_claudecode_at(&claudecode_model_info(Some(mode)), &path, &relay).success
                );
                assert!(read_json_file(&path).unwrap()["env"]["ANTHROPIC_BASE_URL"].is_string());
                assert_eq!(
                    read_json_file(&path).unwrap()["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"],
                    "0"
                );
                assert!(restore_claudecode_at(&path, &relay).success);
                assert_eq!(read_json_file(&path).unwrap(), original);
                assert_eq!(read_json_file(&unchanged).unwrap(), original);
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn auto_mode_default_survives_repeated_model_switches_until_official_restore() {
        let dir = std::env::temp_dir().join(format!("claude-auto-mode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        let original = serde_json::json!({"env":{"KEEP":"yes"}});
        write_json_file(&path, &original).unwrap();
        for mode in [false, true, false] {
            assert!(apply_claudecode_at(&claudecode_model_info(Some(mode)), &path, &relay).success);
            assert_eq!(
                read_json_file(&path).unwrap()["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"],
                "0"
            );
        }
        assert!(restore_claudecode_at(&path, &relay).success);
        assert_eq!(read_json_file(&path).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn auto_mode_preserves_user_values_and_manual_edits() {
        let dir = std::env::temp_dir().join(format!("claude-auto-mode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        for mode in [false, true] {
            for value in ["0", "1"] {
                let original =
                    serde_json::json!({"env":{"CLAUDE_CODE_AUTO_MODE_SERVER":value,"KEEP":"yes"}});
                write_json_file(&path, &original).unwrap();
                assert!(
                    apply_claudecode_at(&claudecode_model_info(Some(mode)), &path, &relay).success
                );
                assert_eq!(
                    read_json_file(&path).unwrap()["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"],
                    value
                );
                assert!(restore_claudecode_at(&path, &relay).success);
                assert_eq!(read_json_file(&path).unwrap(), original);
            }
            for reapply in [false, true] {
                write_json_file(&path, &serde_json::json!({"env":{"KEEP":"yes"}})).unwrap();
                assert!(
                    apply_claudecode_at(&claudecode_model_info(Some(mode)), &path, &relay).success
                );
                let mut edited = read_json_file(&path).unwrap();
                edited["env"]["CLAUDE_CODE_AUTO_MODE_SERVER"] = serde_json::json!("1");
                write_json_file(&path, &edited).unwrap();
                if reapply {
                    assert!(
                        apply_claudecode_at(&claudecode_model_info(Some(!mode)), &path, &relay)
                            .success
                    );
                }
                assert!(restore_claudecode_at(&path, &relay).success);
                assert_eq!(
                    read_json_file(&path).unwrap(),
                    serde_json::json!({"env":{"KEEP":"yes","CLAUDE_CODE_AUTO_MODE_SERVER":"1"}})
                );
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn auto_mode_ownership_does_not_cross_config_directories() {
        let dir = std::env::temp_dir().join(format!("claude-auto-mode-{}", uuid::Uuid::new_v4()));
        let first = dir.join("first/settings.json");
        let second = dir.join("second/settings.json");
        let relay = dir.join("relay.json");
        let original = serde_json::json!({"env":{"CLAUDE_CODE_AUTO_MODE_SERVER":"0"}});
        write_json_file(&second, &original).unwrap();
        assert!(apply_claudecode_at(&claudecode_model_info(Some(false)), &first, &relay).success);
        assert!(apply_claudecode_at(&claudecode_model_info(Some(true)), &second, &relay).success);
        assert!(restore_claudecode_at(&second, &relay).success);
        assert_eq!(read_json_file(&second).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_relay_write_does_not_leave_an_untracked_auto_mode_default() {
        let dir = std::env::temp_dir().join(format!("claude-auto-mode-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        fs::create_dir_all(&relay).unwrap();
        for mode in [false, true] {
            write_json_file(&path, &serde_json::json!({"env":{"KEEP":"yes"}})).unwrap();
            assert!(
                !apply_claudecode_at(&claudecode_model_info(Some(mode)), &path, &relay).success
            );
            let config = read_json_file(&path).unwrap();
            assert!(config["env"].get(AUTO_MODE_SERVER).is_none());
            assert_eq!(config["env"]["KEEP"], "yes");
        }
        fs::remove_dir_all(dir).unwrap();
    }

    fn claudecode_model_info(relay_mode: Option<bool>) -> ModelInfo {
        ModelInfo {
            name: Some("MiMo v2.5 Pro".to_string()),
            model: Some("mimo-v2.5-pro".to_string()),
            base_url: Some("https://api.example.com/v1".to_string()),
            api_key: Some("test-key".to_string()),
            anthropic_url: None,
            protocol: Some("anthropic".to_string()),
            display_model: None,
            relay_mode,
            web_search: None,
            image_input: None,
            one_m_context: None,
        }
    }

    #[test]
    fn code_direct_models_ignore_legacy_routing_flags_and_keep_one_m() {
        let dir = std::env::temp_dir().join(format!("code-direct-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        for legacy_mode in [None, Some(false), Some(true)] {
            for one_m in [false, true] {
                let mut info = claudecode_model_info(legacy_mode);
                info.one_m_context = Some(one_m);
                assert!(apply_claudecode_at(&info, &path, &relay).success);
                let settings = read_json_file(&path).unwrap();
                assert_eq!(
                    settings["env"]["ANTHROPIC_BASE_URL"],
                    "https://api.example.com"
                );
                assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "test-key");
                assert_eq!(
                    settings["env"]["ANTHROPIC_MODEL"],
                    if one_m {
                        "mimo-v2.5-pro[1m]"
                    } else {
                        "mimo-v2.5-pro"
                    }
                );
                assert_eq!(
                    settings["env"]["CLAUDE_CODE_SUBAGENT_MODEL"],
                    "mimo-v2.5-pro"
                );
                assert_eq!(read_json_file(&relay).unwrap()["relayMode"], true);
                assert_eq!(
                    read_claudecode_at(&path, &relay).unwrap().model.as_deref(),
                    Some("mimo-v2.5-pro")
                );
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relay_models_pin_all_tiers_including_fable() {
        let dir = std::env::temp_dir().join(format!("claude-fable-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        for model in [
            "gpt-6.1-sol",
            "deepseek-flash",
            "mimo-v2.5-pro",
            "claude-opus-5-5",
            "claude-fable-5-1",
            "vendor/claude-fable-5-1",
        ] {
            for one_m in [false, true] {
                write_json_file(&path, &serde_json::json!({"env":{"KEEP":"yes"}})).unwrap();
                let mut info = claudecode_model_info(Some(true));
                info.model = Some(model.into());
                info.one_m_context = Some(one_m);
                assert!(apply_claudecode_at(&info, &path, &relay).success);
                let settings = read_json_file(&path).unwrap();
                let env = &settings["env"];
                for var in CLAUDECODE_MODEL_VARS {
                    assert_eq!(env[var], claudecode_env_model_id(model, var, one_m));
                }
                assert_eq!(env["KEEP"], "yes");
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relay_fable_mapping_restores_prior_values_after_model_and_context_changes() {
        let dir = std::env::temp_dir().join(format!("claude-fable-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        for existing in ["custom-fable", "custom-fable[1m]", "claude-fable-5-1"] {
            let original =
                serde_json::json!({"env":{"ANTHROPIC_DEFAULT_FABLE_MODEL":existing,"KEEP":"yes"}});
            write_json_file(&path, &original).unwrap();
            write_json_file(&relay, &serde_json::json!({"actualModel":"previous-model","fableTierManaged":false,"autoModeServerSettingsPath":path.to_string_lossy()})).unwrap();
            for (model, one_m) in [("fixture-model", true), ("other-model", false)] {
                let mut info = claudecode_model_info(Some(true));
                info.model = Some(model.into());
                info.one_m_context = Some(one_m);
                assert!(apply_claudecode_at(&info, &path, &relay).success);
                let settings = read_json_file(&path).unwrap();
                assert_eq!(
                    settings["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"],
                    if one_m {
                        format!("{model}[1m]")
                    } else {
                        model.into()
                    }
                );
                assert_eq!(read_json_file(&relay).unwrap()["fableTierManaged"], true);
                assert_eq!(
                    read_claudecode_at(&path, &relay).unwrap().model.as_deref(),
                    Some(model)
                );
                assert_eq!(settings["env"]["KEEP"], "yes");
            }
            assert!(restore_claudecode_at(&path, &relay).success);
            assert_eq!(read_json_file(&path).unwrap(), original);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn relay_fable_mapping_preserves_manual_edits_on_restore() {
        let dir = std::env::temp_dir().join(format!("claude-fable-{}", uuid::Uuid::new_v4()));
        let path = dir.join("settings.json");
        let relay = dir.join("relay.json");
        for user_model in ["user-fable", "user-fable[1m]"] {
            write_json_file(&path, &serde_json::json!({"env":{"KEEP":"yes"}})).unwrap();
            let mut info = claudecode_model_info(Some(true));
            info.model = Some("fixture-model".into());
            assert!(apply_claudecode_at(&info, &path, &relay).success);
            let mut settings = read_json_file(&path).unwrap();
            settings["env"]["ANTHROPIC_DEFAULT_FABLE_MODEL"] = serde_json::json!(user_model);
            write_json_file(&path, &settings).unwrap();
            assert!(restore_claudecode_at(&path, &relay).success);
            assert_eq!(
                read_json_file(&path).unwrap(),
                serde_json::json!({"env":{"ANTHROPIC_DEFAULT_FABLE_MODEL":user_model,"KEEP":"yes"}})
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claudecode_normalize_never_decorates_model_id() {
        // The model id must reach the upstream verbatim in every mode. Claude
        // decorations like the `[1m]` suffix are gone entirely: in bridge mode
        // Claude Code's own built-in claude-* ids already budget the full
        // window, and the rewritten upstream id must be exactly what the
        // third-party provider advertises.
        for relay_mode in [None, Some(false), Some(true)] {
            let info =
                normalize_model_info_for_tool("claudecode", claudecode_model_info(relay_mode));
            assert_eq!(info.model.as_deref(), Some("mimo-v2.5-pro"));
        }
    }

    #[test]
    fn claudecode_normalize_strips_trailing_v1_from_base_url() {
        // Relay mode appends `/v1/messages` client-side, so a `/v1` left on the
        // base URL would double into `/v1/v1/messages`.
        let info = normalize_model_info_for_tool("claudecode", claudecode_model_info(None));
        assert_eq!(info.base_url.as_deref(), Some("https://api.example.com"));
    }

    // API Router (relay) full write-in pins every model tier to the real
    // upstream model so Claude Code uses the third-party model end-to-end.
    // Two requirements locked here, per https://code.claude.com/docs/en/model-config:
    //   * CLAUDE_CODE_SUBAGENT_MODEL must be written — otherwise subagents
    //     fall back to claude-* ids and bypass the router (breaks "全量").
    //   * ANTHROPIC_SMALL_FAST_MODEL must NOT be written — Claude Code
    //     deprecated it in favor of ANTHROPIC_DEFAULT_HAIKU_MODEL (still pinned).
    #[test]
    fn claudecode_model_vars_pin_subagent_and_drop_small_fast() {
        assert!(
            CLAUDECODE_MODEL_VARS.contains(&"CLAUDE_CODE_SUBAGENT_MODEL"),
            "relay mode must pin CLAUDE_CODE_SUBAGENT_MODEL so subagents use the third-party model"
        );
        assert!(
            !CLAUDECODE_MODEL_VARS.contains(&"ANTHROPIC_SMALL_FAST_MODEL"),
            "ANTHROPIC_SMALL_FAST_MODEL is deprecated (favor ANTHROPIC_DEFAULT_HAIKU_MODEL, also pinned) — stop writing it"
        );
    }

    // ── claudecode_env_model_id: [1m] opt-in, 1M-tier-only ──
    // The 1M-context toggle is relay-only and Claude-Code-only. When opted in,
    // `[1m]` is appended to the 1M-capable tier (MODEL / SONNET / OPUS / FABLE) so CC
    // budgets the 1M window; HAIKU + SUBAGENT stay bare (no 1M concept). CC
    // strips the suffix before sending upstream, so the provider sees the bare id.
    #[test]
    fn claudecode_env_model_id_appends_1m_for_1m_tier_when_opt_in() {
        for &var in CLAUDECODE_MODEL_VARS_1M {
            assert_eq!(
                claudecode_env_model_id("mimo-v2.5-pro", var, true),
                "mimo-v2.5-pro[1m]",
                "opt-in 1M must decorate the 1M-capable tier var {}",
                var
            );
        }
    }

    #[test]
    fn claudecode_env_model_id_leaves_haiku_and_subagent_bare_even_when_opt_in() {
        for var in [
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "CLAUDE_CODE_SUBAGENT_MODEL",
        ] {
            assert_eq!(
                claudecode_env_model_id("mimo-v2.5-pro", var, true),
                "mimo-v2.5-pro",
                "opt-in must NOT decorate the non-1M tier var {}",
                var
            );
        }
    }

    #[test]
    fn claudecode_env_model_id_bare_when_opt_out() {
        // Opt-out (or unset) → every var stays bare, regardless of tier.
        for var in CLAUDECODE_MODEL_VARS {
            assert_eq!(
                claudecode_env_model_id("mimo-v2.5-pro", var, false),
                "mimo-v2.5-pro",
                "opt-out must leave {} bare",
                var
            );
        }
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;
    #[test]
    fn smart_picker_roundtrips_original_settings_and_preserves_manual_edits() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let root = super::super::switching::TestDir::new();
            let settings = root.path().join("settings.json");
            let relay = root.path().join("relay.json");
            let original = serde_json::json!({"modelPicker":{"options":[{"model":"personal","label":"Keep"}]},"custom":true});
            write_json_file(&settings, &original).unwrap();
            let mut info = super::super::switching::test_info();
            info.one_m_context = Some(true);
            for _ in 0..2 {
                let result = apply_claudecode_at(&info, &settings, &relay);
                assert!(result.success, "{}", result.message);
                let value = read_json_file(&settings).unwrap();
                assert_eq!(value["modelPicker"]["options"].as_array().unwrap().len(), 3);
                assert_eq!(value["env"]["ANTHROPIC_MODEL"], "auto");
                assert_eq!(value["modelPicker"]["options"][0]["model"], "auto");
                assert!(value["modelPicker"]["options"][1]["model"]
                    .as_str()
                    .unwrap()
                    .ends_with("[1m]"));
                assert!(!value["modelPicker"]["options"][2]["model"]
                    .as_str()
                    .unwrap()
                    .ends_with("[1m]"));
            }
            assert!(restore_claudecode_at(&settings, &relay).success);
            assert_eq!(read_json_file(&settings).unwrap(), original);
            assert!(apply_claudecode_at(&info, &settings, &relay).success);
            let manual = serde_json::json!({"modelPicker":{"options":[]},"custom":true});
            let mut value = read_json_file(&settings).unwrap();
            value["modelPicker"] = manual["modelPicker"].clone();
            write_json_file(&settings, &value).unwrap();
            assert!(restore_claudecode_at(&settings, &relay).success);
            assert_eq!(read_json_file(&settings).unwrap(), manual);
        });
    }
}

fn config_dir() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !value.trim().is_empty() {
            return Ok(PathBuf::from(value.trim().trim_matches('"')));
        }
    }
    Ok(dirs::home_dir()
        .ok_or("Cannot find the home directory")?
        .join(".claude"))
}
