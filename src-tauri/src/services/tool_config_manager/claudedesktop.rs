//! Model configuration for claudedesktop.

use super::{echobird_dir, read_json_file, write_json_file, ApplyResult, ModelInfo};
use std::fs;
use std::path::{Path, PathBuf};

// ════════════════════════════════════════════════════════════════
//  Claude Desktop — 3P provider profile writer.
//
// Claude Desktop has an officially-supported "third-party profile"
// (3P) mechanism that's completely separate from Claude Code's
// ~/.claude/settings.json. Setting `deploymentMode = "3p"` in
// claude_desktop_config.json + writing a profile JSON to
// Claude-3p/configLibrary/ tells Desktop to route /v1/messages
// traffic to a custom inferenceGatewayBaseUrl instead of Anthropic.
//
// We only support providers that natively speak the Anthropic
// Messages API (i.e. `model.anthropicUrl` is set in
// modelDirectory.json — DeepSeek, GLM, Kimi, Qwen, MiniMax,
// Xiaomi, WorldRouter, Anthropic itself). Non-Anthropic providers
// would need a translation proxy, which we deliberately skip —
// the AppManager UI filters out incompatible models via the
// existing apiProtocol/anthropicUrl gate.
// ════════════════════════════════════════════════════════════════

pub(super) const CLAUDE_DESKTOP_PROFILE_ID: &str = "7d8f4e2a-9c3b-4f1a-b0e5-1a2b3c4d5e6f";

const CLAUDE_DESKTOP_PROFILE_NAME: &str = "EchoBird";

pub(super) struct ClaudeDesktopLayout {
    cfg_official: PathBuf,
    cfg_threep: PathBuf,
    pub(super) lib_dir: PathBuf,
}

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
pub(super) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
    #[cfg(any(target_os = "macos", windows))]
    let home = dirs::home_dir()?;

    #[cfg(target_os = "macos")]
    let config_root = home.join("Library").join("Application Support");

    #[cfg(windows)]
    let config_root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("AppData").join("Local"));

    // Electron uses $XDG_CONFIG_HOME, falling back to ~/.config on Linux.
    #[cfg(target_os = "linux")]
    let config_root = dirs::config_dir()?;

    Some(claudedesktop_paths_at(&config_root))
}

fn claudedesktop_paths_at(config_root: &Path) -> ClaudeDesktopLayout {
    let official_dir = config_root.join("Claude");
    let threep_dir = config_root.join("Claude-3p");
    ClaudeDesktopLayout {
        cfg_official: official_dir.join("claude_desktop_config.json"),
        cfg_threep: threep_dir.join("claude_desktop_config.json"),
        lib_dir: threep_dir.join("configLibrary"),
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
pub(super) fn resolve_claudedesktop_paths() -> Option<ClaudeDesktopLayout> {
    None
}

fn read_config_object(path: &Path) -> Result<serde_json::Value, String> {
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(serde_json::json!({}));
        }
        Err(error) => return Err(format!("Failed to read {}: {error}", path.display())),
    };
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid JSON in {}: {error}", path.display()))?;
    if !value.is_object() {
        return Err(format!("Expected a JSON object in {}", path.display()));
    }
    Ok(value)
}

#[cfg(test)]
fn third_party_mode_at(paths: &ClaudeDesktopLayout) -> Result<bool, String> {
    Ok(read_config_object(&paths.cfg_official)?["deploymentMode"] == "3p")
}

/// Change the deployment mode while preserving other native settings.
fn set_claude_deployment_mode(path: &Path, mode: &str) -> Result<(), String> {
    let mut cfg = read_config_object(path)?;
    if let Some(obj) = cfg.as_object_mut() {
        obj.insert(
            "deploymentMode".to_string(),
            serde_json::Value::String(mode.to_string()),
        );
    }
    write_json_file(path, &cfg)
}

fn profile_meta(path: &Path, apply: bool) -> Result<serde_json::Value, String> {
    let mut meta = read_config_object(path)?;
    let entries = meta
        .as_object_mut()
        .expect("validated object")
        .entry("entries")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .ok_or_else(|| format!("Invalid profile entries in {}", path.display()))?;
    entries.retain(|entry| entry["id"] != CLAUDE_DESKTOP_PROFILE_ID);
    if apply {
        entries.push(serde_json::json!({
            "id": CLAUDE_DESKTOP_PROFILE_ID,
            "name": CLAUDE_DESKTOP_PROFILE_NAME,
        }));
        meta["appliedId"] = serde_json::json!(CLAUDE_DESKTOP_PROFILE_ID);
    } else if meta["appliedId"] == CLAUDE_DESKTOP_PROFILE_ID {
        meta.as_object_mut()
            .expect("validated object")
            .remove("appliedId");
    }
    Ok(meta)
}

fn is_deepseek_official(url: &str) -> bool {
    let authority = url.trim().split("://").last().unwrap_or_default();
    let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
    authority
        .split(':')
        .next()
        .is_some_and(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
}

fn desktop_model_id<'a>(real_id: &'a str, anthropic_url: &str) -> &'a str {
    let id = real_id.trim();
    let model = id.rsplit('/').next().unwrap_or(id).to_ascii_lowercase();
    // DeepSeek resolves Claude aliases itself: opus selects its pro model, sonnet selects flash.
    if model == "deepseek-flash" && is_deepseek_official(anthropic_url) {
        return "claude-sonnet";
    }
    if ["claude", "opus", "sonnet", "fable", "haiku"]
        .iter()
        .any(|prefix| {
            model == *prefix
                || model
                    .strip_prefix(*prefix)
                    .and_then(|rest| rest.strip_prefix('-'))
                    .is_some_and(|version| !version.is_empty())
        })
    {
        id
    } else {
        "claude-opus"
    }
}

/// Desktop's tier for a published name, read from its Claude tier word.
fn desktop_tier(name: &str) -> Option<&'static str> {
    let model = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
    ["opus", "sonnet", "haiku", "fable"]
        .into_iter()
        .find(|tier| model.split(['-', '.', '_']).any(|part| part == *tier))
}

fn newest_claude_model(tier: &str) -> &'static str {
    match tier {
        "sonnet" => "claude-sonnet-5-5",
        "haiku" => "claude-haiku-5-5",
        "fable" => "claude-fable-5-1",
        _ => "claude-opus-5-5",
    }
}

pub(super) fn apply_claudedesktop(model_info: &ModelInfo) -> ApplyResult {
    let paths = match resolve_claudedesktop_paths() {
        Some(p) => p,
        None => {
            return ApplyResult {
                success: false,
                message: "Cannot resolve the Claude Desktop configuration directory.".to_string(),
            };
        }
    };

    apply_claudedesktop_at(
        model_info,
        &paths,
        &echobird_dir().join("claudedesktop.json"),
    )
}

fn apply_claudedesktop_at(
    model_info: &ModelInfo,
    paths: &ClaudeDesktopLayout,
    relay_path: &Path,
) -> ApplyResult {
    let switch_models = match super::switching::models(model_info) {
        Ok(models) => models,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    // The frontend's AppManagerProvider.applyModel collapses the chosen
    // protocol's URL into `base_url` when sending to the backend (the
    // long-standing claudecode convention — see line `const apiUrl =
    // useAnthropicUrl ? model.anthropicUrl! : model.baseUrl`). For
    // claudedesktop the picked model's Anthropic endpoint will arrive
    // in `base_url` with `protocol == "anthropic"`. Accept either field
    // so we work today and stay compatible if the frontend ever sends
    // `anthropic_url` explicitly. The AppManager protocol filter is the
    // real gate that ensures the URL is actually Anthropic-shaped.
    let anthropic_url = model_info
        .anthropic_url
        .as_deref()
        .or(model_info.base_url.as_deref())
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| u.trim_end_matches('/').to_string());
    let anthropic_url = match anthropic_url {
        Some(u) => u,
        None => {
            return ApplyResult {
                success: false,
                message: "Base URL is empty. Pick a model first.".to_string(),
            };
        }
    };

    // Mirror the local-provider fallback in `apply_codex` (line 1153):
    // local LLM proxies (llama.cpp / vllm / sglang under our unified
    // proxy) don't require an API key, so an empty user-supplied key
    // is legitimate when the upstream is loopback. Substitute a
    // non-empty sentinel for Desktop's gateway authentication field.
    let raw_api_key = model_info.api_key.as_deref().unwrap_or("");
    let is_local_provider =
        anthropic_url.contains("127.0.0.1") || anthropic_url.contains("localhost");
    let api_key = if raw_api_key.is_empty() {
        if is_local_provider {
            "local-no-auth".to_string()
        } else {
            return ApplyResult {
                success: false,
                message: "API Key is empty, cannot apply Claude Desktop config.".to_string(),
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
        .unwrap_or("");

    let profile_path = paths
        .lib_dir
        .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
    let mut profile = match read_config_object(&profile_path) {
        Ok(profile) => profile,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    if let Some(enabled) = model_info.web_search {
        let disabled = profile
            .as_object_mut()
            .unwrap()
            .entry("disabledBuiltinTools")
            .or_insert_with(|| serde_json::json!([]));
        let Some(tools) = disabled
            .as_array_mut()
            .filter(|tools| tools.iter().all(serde_json::Value::is_string))
        else {
            return ApplyResult {
                success: false,
                message: "Claude Desktop's disabled tools setting is invalid. Nothing was changed."
                    .into(),
            };
        };
        tools.retain(|tool| tool != "WebSearch");
        if !enabled {
            tools.push(serde_json::json!("WebSearch"));
        }
    }

    let meta_path = paths.lib_dir.join("_meta.json");
    let meta = match profile_meta(&meta_path, true) {
        Ok(meta) => meta,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    // Validate both files before changing either one.
    for path in [&paths.cfg_official, &paths.cfg_threep] {
        if let Err(message) = read_config_object(path) {
            return ApplyResult {
                success: false,
                message,
            };
        }
    }
    if let Err(e) = set_claude_deployment_mode(&paths.cfg_official, "3p") {
        return ApplyResult {
            success: false,
            message: format!("Failed to set Claude config to 3p mode: {}", e),
        };
    }
    if let Err(e) = set_claude_deployment_mode(&paths.cfg_threep, "3p") {
        return ApplyResult {
            success: false,
            message: format!("Failed to set Claude-3p config to 3p mode: {}", e),
        };
    }

    let _ = fs::create_dir_all(&paths.lib_dir);

    // Profile body — Desktop's gateway reads these on launch. The profile's
    // display name is NOT a field of this object; it belongs in _meta.json
    // entries[].name (Desktop's profile picker reads it from there).
    //
    // Populate the picker explicitly without requiring the upstream to
    // implement Desktop's `/v1/models` discovery probe.
    // `name` is sent directly upstream: retain Claude-like IDs, otherwise use
    // the provider's Claude alias. Save the actual model independently.
    let base_model_id = desktop_model_id(real_model_id, &anthropic_url);
    let display_name = model_info
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(real_model_id);
    // prefer1m selects the 1M variant without decorating the model ID.
    // The tier comes from the published name, never a hardcoded one, so a
    // Sonnet upstream is never tagged as Opus. Desktop's Code tab fills
    // ANTHROPIC_DEFAULT_<TIER>_MODEL from the tagged entry, and gives Claude
    // Code `behavesAs` (that tier's Claude prompt and defaults) only from this
    // list. A real Claude upstream is known to Claude Code by its own id.
    let tier = desktop_tier(base_model_id);
    let behaves_as = (base_model_id != real_model_id.trim())
        .then(|| tier.map(newest_claude_model))
        .flatten();
    let mut model_entry = serde_json::Map::new();
    model_entry.insert(
        "name".to_string(),
        serde_json::Value::String(base_model_id.to_string()),
    );
    model_entry.insert(
        "labelOverride".to_string(),
        serde_json::Value::String(display_name.to_string()),
    );
    model_entry.insert("supports1m".to_string(), serde_json::Value::Bool(true));
    model_entry.insert(
        "prefer1m".to_string(),
        serde_json::Value::Bool(model_info.one_m_context.unwrap_or(false)),
    );
    if let Some(behaves_as) = behaves_as {
        model_entry.insert("behavesAs".to_string(), serde_json::json!(behaves_as));
    }
    let mut first_entry = model_entry.clone();
    if let Some(tier) = tier {
        first_entry.insert("anthropicFamilyTier".to_string(), serde_json::json!(tier));
        first_entry.insert("isFamilyDefault".to_string(), serde_json::json!(true));
    }
    let smart_access = !switch_models.is_empty();
    let mut inference_models = vec![serde_json::Value::Object(first_entry)];
    // The auto row uses Desktop's Claude alias and stays first (a new
    // session's default). Concrete rows keep their stable selectors, carry
    // no tier of their own, and run as the auto row's tier.
    for model in switch_models
        .into_iter()
        .filter(|model| model.id != real_model_id)
    {
        let mut entry = model_entry.clone();
        entry.insert("name".to_string(), serde_json::json!(model.id));
        entry.insert("labelOverride".to_string(), serde_json::json!(model.name));
        if !entry.contains_key("behavesAs") {
            entry.insert(
                "behavesAs".to_string(),
                serde_json::json!(newest_claude_model(tier.unwrap_or("opus"))),
            );
        }
        inference_models.push(serde_json::Value::Object(entry));
    }
    // With a list present Desktop skips `/v1/models` unless told otherwise.
    // Smart Access serves its own list with descriptions, so Desktop takes the
    // subtitles from it instead of the selector's tier word; the list itself
    // stays ours. Direct providers keep the probe off.
    if smart_access {
        profile["modelDiscoveryEnabled"] = serde_json::json!(true);
    } else {
        profile
            .as_object_mut()
            .unwrap()
            .remove("modelDiscoveryEnabled");
    }
    let updates = serde_json::json!({
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": anthropic_url,
        "inferenceModels": inference_models,
        "inferenceProvider": "gateway",
        // Claude Desktop 3P mode derives the built-in web_fetch egress policy
        // from this field. Without it, egress falls back to the gateway host
        // only (127.0.0.1), so web_fetch can't reach external URLs. ["*"]
        // lets web_fetch fetch any URL the user asks Claude to fetch.
        "coworkEgressAllowedHosts": ["*"],
    });
    profile
        .as_object_mut()
        .unwrap()
        .extend(updates.as_object().unwrap().clone());
    if let Err(e) = write_json_file(&profile_path, &profile) {
        return ApplyResult {
            success: false,
            message: format!("Failed to write Claude Desktop profile: {}", e),
        };
    }

    // Meta — Desktop reads `appliedId` to know which profile is active,
    // and `entries[]` to populate the in-app profile picker. Without the
    // entries array the picker won't render our profile as named.
    if let Err(message) = write_json_file(&meta_path, &meta) {
        return ApplyResult {
            success: false,
            message,
        };
    }

    // Preserve actual identity for configuration reads and official restore.
    let relay = serde_json::json!({
        "baseUrl": anthropic_url,
        "apiKey": api_key,
        "actualModel": real_model_id,
        "modelName": model_info.name.as_deref().unwrap_or(real_model_id),
        "relayMode": true,
    });
    if let Err(e) = write_json_file(relay_path, &relay) {
        return ApplyResult {
            success: false,
            message: format!("Failed to write Claude Desktop relay file: {}", e),
        };
    }

    ApplyResult {
        success: true,
        message:
            "Claude Desktop configured. Please fully quit and reopen Claude Desktop for the change to take effect."
                .to_string(),
    }
}

pub(super) fn restore_claudedesktop_to_official() -> ApplyResult {
    let paths = match resolve_claudedesktop_paths() {
        Some(p) => p,
        None => {
            return ApplyResult {
                success: true,
                message: "Claude Desktop is not supported on this OS — nothing to restore."
                    .to_string(),
            };
        }
    };

    restore_claudedesktop_at(&paths, &echobird_dir().join("claudedesktop.json"))
}

fn restore_claudedesktop_at(paths: &ClaudeDesktopLayout, relay_path: &Path) -> ApplyResult {
    match restore_claudedesktop_files(paths, relay_path) {
        Ok(()) => ApplyResult {
            success: true,
            message: "Claude Desktop restored to official provider. Please fully quit and reopen Claude Desktop.".into(),
        },
        Err(message) => ApplyResult { success: false, message },
    }
}

fn remove_config_file(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove {}: {error}", path.display())),
    }
}

fn restore_claudedesktop_files(
    paths: &ClaudeDesktopLayout,
    relay_path: &Path,
) -> Result<(), String> {
    let meta_path = paths.lib_dir.join("_meta.json");
    let meta = profile_meta(&meta_path, false)?;
    for path in [&paths.cfg_official, &paths.cfg_threep] {
        read_config_object(path)?;
    }
    // Keep the working profile and relay if either native config cannot be updated.
    set_claude_deployment_mode(&paths.cfg_official, "1p")?;
    set_claude_deployment_mode(&paths.cfg_threep, "1p")?;

    if meta.as_object().is_some_and(|obj| obj.len() == 1)
        && meta["entries"].as_array().is_some_and(Vec::is_empty)
    {
        remove_config_file(&meta_path)?;
    } else {
        write_json_file(&meta_path, &meta)?;
    }

    let profile_path = paths
        .lib_dir
        .join(format!("{}.json", CLAUDE_DESKTOP_PROFILE_ID));
    remove_config_file(&profile_path)?;

    // Drop the relay file so anthropic_proxy stops accepting requests
    // for the previously-applied provider. (The proxy keeps running and
    // returns 503 on a missing relay — graceful no-op for any stale
    // request still in flight.)
    remove_config_file(relay_path)
}

pub(super) fn read_claudedesktop() -> Option<ModelInfo> {
    // Saved state retains the actual model even when Desktop uses an alias.
    let relay_path = echobird_dir().join("claudedesktop.json");
    let relay = read_json_file(&relay_path)?;

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

    Some(ModelInfo {
        name: relay
            .get("modelName")
            .and_then(|v| v.as_str())
            .map(String::from),
        model: relay
            .get("actualModel")
            .and_then(|v| v.as_str())
            .map(String::from),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_damaged_configs_before_applying_or_removing_proxy_files() {
        let dir = std::env::temp_dir().join(format!("claude-damaged-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir);
        let relay = dir.join("relay.json");
        let profile = paths
            .lib_dir
            .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
        let info: ModelInfo = serde_json::from_value(serde_json::json!({
            "model": "test", "baseUrl": "https://example.test", "apiKey": "fixture"
        }))
        .unwrap();
        write_json_file(
            &paths.cfg_official,
            &serde_json::json!({"deploymentMode":"3p", "mcpServers":{"keep":{}}}),
        )
        .unwrap();
        write_json_file(&paths.cfg_threep, &serde_json::json!({})).unwrap();
        write_json_file(&relay, &serde_json::json!({"keep":true})).unwrap();
        write_json_file(&profile, &serde_json::json!({"keep":true})).unwrap();
        let original = fs::read(&paths.cfg_official).unwrap();
        for damaged in ["{broken", "[]", "null"] {
            fs::write(&paths.cfg_threep, damaged).unwrap();
            assert!(!apply_claudedesktop_at(&info, &paths, &relay).success);
            assert!(!restore_claudedesktop_at(&paths, &relay).success);
            assert_eq!(fs::read_to_string(&paths.cfg_threep).unwrap(), damaged);
            assert_eq!(fs::read(&paths.cfg_official).unwrap(), original);
            assert!(relay.exists());
            assert!(profile.exists());
        }
        // A directory at the file path produces a portable I/O failure, even as root.
        fs::remove_file(&paths.cfg_threep).unwrap();
        fs::create_dir(&paths.cfg_threep).unwrap();
        assert!(!restore_claudedesktop_at(&paths, &relay).success);
        assert!(relay.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_other_profiles_and_reports_cleanup_failures() {
        let dir = std::env::temp_dir().join(format!("claude-profiles-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir);
        let relay = dir.join("relay.json");
        let meta_path = paths.lib_dir.join("_meta.json");
        let original = serde_json::json!({"appliedId":"other", "entries":[{"id":"other","name":"Other"}],"unknown":42});
        write_json_file(&meta_path, &original).unwrap();
        let info: ModelInfo = serde_json::from_value(serde_json::json!({
            "model":"test", "baseUrl":"https://example.test", "apiKey":"fixture"
        }))
        .unwrap();
        assert!(apply_claudedesktop_at(&info, &paths, &relay).success);
        assert_eq!(
            read_json_file(&meta_path).unwrap()["entries"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(restore_claudedesktop_at(&paths, &relay).success);
        let meta = read_json_file(&meta_path).unwrap();
        assert_eq!(meta["entries"], original["entries"]);
        assert_eq!(meta["unknown"], 42);
        assert!(meta.get("appliedId").is_none());

        // A failed relay deletion must be visible rather than claiming success.
        fs::create_dir(&relay).unwrap();
        assert!(!restore_claudedesktop_at(&paths, &relay).success);
        fs::remove_dir_all(dir).unwrap();
    }
    use serde_json::json;

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_resolves_desktop_config_in_xdg_directory() {
        let root = dirs::config_dir().unwrap();
        let paths =
            resolve_claudedesktop_paths().expect("Linux must support Desktop model switching");
        assert_eq!(
            paths.cfg_official,
            root.join("Claude/claude_desktop_config.json")
        );
        assert_eq!(
            paths.cfg_threep,
            root.join("Claude-3p/claude_desktop_config.json")
        );
        assert_eq!(paths.lib_dir, root.join("Claude-3p/configLibrary"));
    }

    #[test]
    fn desktop_direct_models_automatically_choose_client_id_and_keep_real_identity() {
        let dir = std::env::temp_dir().join(format!("desktop-direct-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir);
        let relay_path = dir.join("saved.json");
        for (model, expected) in [
            ("claude-opus-5-5", "claude-opus-5-5"),
            ("anthropic/claude-sonnet-5", "anthropic/claude-sonnet-5"),
            ("vendor/Haiku-5", "vendor/Haiku-5"),
            ("  vendor/Claude-opus-5-5  ", "vendor/Claude-opus-5-5"),
            ("OPUS-5.5", "OPUS-5.5"),
            ("fable-5-1", "fable-5-1"),
            ("sonnet", "sonnet"),
            ("deepseek-flash", "claude-opus"),
            ("vendor/mimo-pro", "claude-opus"),
            ("my-claude-model", "claude-opus"),
            ("opuslike-model", "claude-opus"),
            ("sonnet-", "claude-opus"),
        ] {
            for legacy_mode in [None, Some(false), Some(true)] {
                for one_m in [false, true] {
                    let info: ModelInfo = serde_json::from_value(json!({
                        "name": "Actual model display name",
                        "model": model,
                        "baseUrl": "https://upstream.example/anthropic",
                        "apiKey": "fixture-key",
                        "relayMode": legacy_mode,
                        "oneMContext": one_m
                    }))
                    .unwrap();
                    let result = apply_claudedesktop_at(&info, &paths, &relay_path);
                    assert!(result.success, "{}", result.message);
                    let profile = read_json_file(
                        &paths
                            .lib_dir
                            .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json")),
                    )
                    .unwrap();
                    assert_eq!(
                        profile["inferenceGatewayBaseUrl"],
                        "https://upstream.example/anthropic"
                    );
                    assert_eq!(profile["inferenceGatewayApiKey"], "fixture-key");
                    assert_eq!(profile["inferenceModels"][0]["name"], expected);
                    assert_eq!(
                        profile["inferenceModels"][0]["labelOverride"],
                        "Actual model display name"
                    );
                    assert_eq!(profile["inferenceModels"][0]["prefer1m"], one_m);
                    let entry = &profile["inferenceModels"][0];
                    let (tier, behaves_as) = match expected {
                        "claude-opus" => (json!("opus"), json!("claude-opus-5-5")),
                        "anthropic/claude-sonnet-5" | "sonnet" => (json!("sonnet"), json!(null)),
                        "vendor/Haiku-5" => (json!("haiku"), json!(null)),
                        "fable-5-1" => (json!("fable"), json!(null)),
                        _ => (json!("opus"), json!(null)),
                    };
                    assert_eq!(entry["anthropicFamilyTier"], tier, "{model}");
                    assert_eq!(entry["isFamilyDefault"], true);
                    assert_eq!(entry["behavesAs"], behaves_as, "{model}");
                    assert!(profile.get("modelDiscoveryEnabled").is_none());
                    let saved = read_json_file(&relay_path).unwrap();
                    assert_eq!(saved["actualModel"], model);
                    assert_eq!(saved["relayMode"], true);
                }
            }
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn deepseek_official_flash_uses_the_sonnet_alias_only_on_its_own_host() {
        let official = "https://api.deepseek.com/anthropic";
        assert_eq!(
            desktop_model_id("deepseek-flash", official),
            "claude-sonnet"
        );
        assert_eq!(
            desktop_model_id(" DeepSeek-Flash ", "HTTPS://API.DEEPSEEK.COM:443/anthropic"),
            "claude-sonnet"
        );
        assert_eq!(desktop_model_id("deepseek-v4-pro", official), "claude-opus");
        for url in [
            "https://relay.example/anthropic",
            "http://127.0.0.1:53683/anthropic",
            "https://api.deepseek.com.evil.example/anthropic",
            "https://relay.example/api.deepseek.com",
        ] {
            assert_eq!(desktop_model_id("deepseek-flash", url), "claude-opus");
        }
    }

    #[test]
    fn desktop_web_search_switch_preserves_other_profile_fields_and_tools() {
        let dir = std::env::temp_dir().join(format!("desktop-search-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir);
        let relay = dir.join("saved.json");
        let profile = paths
            .lib_dir
            .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
        write_json_file(
            &profile,
            &json!({"disabledBuiltinTools":["Bash","WebSearch"],"custom":{"keep":true}}),
        )
        .unwrap();
        for enabled in [false, false, true, false, true] {
            let info: ModelInfo = serde_json::from_value(json!({
                "model":"claude-opus-5-5","baseUrl":"https://provider.example", "apiKey":"fixture", "webSearch":enabled
            })).unwrap();
            assert!(apply_claudedesktop_at(&info, &paths, &relay).success);
            let saved = read_json_file(&profile).unwrap();
            assert_eq!(saved["custom"], json!({"keep":true}));
            assert_eq!(
                saved["disabledBuiltinTools"],
                if enabled {
                    json!(["Bash"])
                } else {
                    json!(["Bash", "WebSearch"])
                }
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_invalid_tool_policy_does_not_partially_enable_third_party_mode() {
        let dir =
            std::env::temp_dir().join(format!("desktop-search-invalid-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir);
        let relay = dir.join("saved.json");
        let profile = paths
            .lib_dir
            .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
        let original = json!({"disabledBuiltinTools":"user-config"});
        write_json_file(&profile, &original).unwrap();
        let info: ModelInfo = serde_json::from_value(json!({
            "model":"claude-opus-5-5","baseUrl":"https://provider.example", "apiKey":"fixture", "webSearch":false
        })).unwrap();
        assert!(!apply_claudedesktop_at(&info, &paths, &relay).success);
        assert_eq!(read_json_file(&profile).unwrap(), original);
        assert!(!paths.cfg_official.exists());
        assert!(!paths.cfg_threep.exists());
        assert!(!relay.exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn switches_desktop_models_and_restores_official_without_changing_cli_or_user_settings() {
        let dir =
            std::env::temp_dir().join(format!("echobird-claude-desktop-{}", uuid::Uuid::new_v4()));
        let paths = claudedesktop_paths_at(&dir.join(".config"));
        let relay_path = dir.join(".echobird/claudedesktop.json");
        let cli_path = dir.join(".claude/settings.json");
        let cli_config = json!({"env":{"ANTHROPIC_MODEL":"cli-model"}});
        write_json_file(&cli_path, &cli_config).unwrap();
        let settings = json!({"mcpServers":{"demo":{"command":"demo"}},"theme":"dark"});
        for path in [&paths.cfg_official, &paths.cfg_threep] {
            write_json_file(path, &settings).unwrap();
        }
        assert!(!third_party_mode_at(&paths).unwrap());
        let profile_path = paths
            .lib_dir
            .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
        for relay_mode in [false, true] {
            let model = if relay_mode {
                "second-model"
            } else {
                "first-model"
            };
            let info: ModelInfo = serde_json::from_value(json!({
                "model": model,
                "baseUrl": "https://example.test/anthropic",
                "apiKey": "test-key",
                "relayMode": relay_mode,
                "oneMContext": relay_mode
            }))
            .unwrap();
            let result = apply_claudedesktop_at(&info, &paths, &relay_path);
            assert!(result.success, "{}", result.message);
            assert!(third_party_mode_at(&paths).unwrap());
            let profile = read_json_file(&profile_path).unwrap();
            assert_eq!(profile["inferenceModels"][0]["labelOverride"], model);
            assert_eq!(profile["inferenceModels"][0]["prefer1m"], relay_mode);
            assert_eq!(profile["inferenceModels"][0]["name"], "claude-opus");
            assert_eq!(
                profile["inferenceGatewayBaseUrl"],
                "https://example.test/anthropic"
            );
            assert_eq!(read_json_file(&relay_path).unwrap()["actualModel"], model);
            assert_eq!(
                read_json_file(&paths.lib_dir.join("_meta.json")).unwrap()["appliedId"],
                CLAUDE_DESKTOP_PROFILE_ID
            );
            for path in [&paths.cfg_official, &paths.cfg_threep] {
                assert_eq!(read_json_file(path).unwrap()["deploymentMode"], "3p");
            }
        }
        assert!(restore_claudedesktop_at(&paths, &relay_path).success);
        assert!(!third_party_mode_at(&paths).unwrap());
        assert!(!profile_path.exists());
        assert!(!paths.lib_dir.join("_meta.json").exists());
        assert!(!relay_path.exists());
        for path in [&paths.cfg_official, &paths.cfg_threep] {
            let config = read_json_file(path).unwrap();
            assert_eq!(config["deploymentMode"], "1p");
            assert_eq!(config["mcpServers"], settings["mcpServers"]);
            assert_eq!(config["theme"], settings["theme"]);
        }
        assert_eq!(read_json_file(&cli_path).unwrap(), cli_config);
        fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod switch_tests {
    use super::*;
    #[test]
    fn smart_profile_lists_concrete_models_and_direct_apply_returns_to_one() {
        crate::services::smart_router::catalog::with_test_models(|| {
            let root = super::super::switching::TestDir::new();
            let paths = claudedesktop_paths_at(root.path());
            let relay = root.path().join("relay.json");
            let mut info = super::super::switching::test_info();
            let profile = paths
                .lib_dir
                .join(format!("{CLAUDE_DESKTOP_PROFILE_ID}.json"));
            let result = apply_claudedesktop_at(&info, &paths, &relay);
            assert!(result.success, "{}", result.message);
            let value = read_json_file(&profile).unwrap();
            assert_eq!(value["inferenceModels"].as_array().unwrap().len(), 3);
            assert_eq!(value["inferenceModels"][1]["labelOverride"], "First model");
            let models = value["inferenceModels"].as_array().unwrap();
            assert_eq!(models[0]["name"], "claude-opus");
            assert_eq!(models[0]["anthropicFamilyTier"], "opus");
            assert_eq!(models[0]["isFamilyDefault"], true);
            for entry in models {
                assert_eq!(entry["behavesAs"], "claude-opus-5-5");
            }
            for entry in &models[1..] {
                assert!(entry.get("anthropicFamilyTier").is_none());
                assert!(entry.get("isFamilyDefault").is_none());
            }
            assert_eq!(value["modelDiscoveryEnabled"], true);
            assert_eq!(
                value["inferenceModels"][1]["name"],
                crate::services::smart_router::catalog::selector("first")
            );
            info.base_url = Some("https://direct.invalid".into());
            assert!(apply_claudedesktop_at(&info, &paths, &relay).success);
            assert_eq!(
                read_json_file(&profile).unwrap()["inferenceModels"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert!(read_json_file(&profile)
                .unwrap()
                .get("modelDiscoveryEnabled")
                .is_none());
            assert!(restore_claudedesktop_at(&paths, &relay).success);
        });
    }
}
