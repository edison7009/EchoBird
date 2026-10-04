//! Model configuration for claudedesktop.

use super::{echobird_dir, read_json_file, write_json_file, ApplyResult, ModelInfo};
use crate::services::anthropic_proxy;
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
    // non-empty sentinel so anthropic_proxy still has something to
    // forward as Bearer / x-api-key (the local server ignores it).
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

    // Two routing modes, picked by `model_info.relay_mode`:
    //
    // • Bridge (default): Desktop's gateway hits our local Anthropic
    //   proxy on `127.0.0.1:ANTHROPIC_PROXY_PORT/v1/messages`. The proxy
    //   reads the real upstream URL, API key, and model id fresh from
    //   ~/.echobird/claudedesktop.json on every request, rewrites the
    //   Anthropic-only model id (Desktop hardcodes claude-sonnet-4-*
    //   etc.) to whatever the EchoBird user actually picked, and
    //   forwards. The `inferenceGatewayApiKey` is a non-empty sentinel
    //   — Desktop refuses an empty value but the proxy ignores what
    //   Desktop sends here, using the real key from the relay file.
    //
    // • Relay (relay_mode = true): Desktop's gateway hits the upstream
    //   directly. We write the real URL + real API key into the
    //   profile JSON; the proxy is bypassed for /v1/messages. Used
    //   for relay stations (cc-vibe.com etc.) that natively serve
    //   Anthropic Messages and do their own model-id mapping. Caveat:
    //   model-id rewrite is lost, so the upstream sees whatever id
    //   Desktop chose (claude-sonnet-4-*, …) — fine for stations that
    //   accept those, broken for raw Chat-only providers.
    let proxy_base = format!("http://127.0.0.1:{}", anthropic_proxy::port());
    let relay_mode = model_info.relay_mode.unwrap_or(false);
    let (gateway_base_url, gateway_api_key) = if relay_mode {
        (anthropic_url.clone(), api_key.clone())
    } else {
        (proxy_base.clone(), "echobird-local-proxy".to_string())
    };

    let profile_path = paths
        .lib_dir
        .join(format!("{}.json", CLAUDE_DESKTOP_PROFILE_ID));
    // Profile body — Desktop's gateway reads these on launch. The profile's
    // display name is NOT a field of this object; it belongs in _meta.json
    // entries[].name (Desktop's profile picker reads it from there).
    //
    // `inferenceModels` populates Desktop's in-app model picker directly
    // and, crucially, BYPASSES Desktop's `/v1/models` discovery probe.
    // Without this field, Desktop 1.7.x falls back to GET /v1/models on
    // our gateway (which we don't implement) and surfaces a
    // "Gateway returned an error" banner on every fresh install. We
    // write a single entry whose `name` is the id Desktop sends on
    // `/v1/messages`. In bridge mode that's the canonical claude-* id
    // (messages_handler rewrites it to the real upstream model); in relay
    // mode Desktop hits the upstream directly with no rewrite, so `name`
    // must instead carry the real upstream id (computed below). `labelOverride` is
    // the upstream model id (`real_model_id`, source = model_info.model
    // with fallback to model_info.name) — NOT the user's editable card
    // display name, which can be anything ("deepseek你好" etc.) and
    // would surface garbage in Desktop's picker.
    // Bridge mode rewrites the id downstream, so the canonical claude-opus-5-5
    // is correct (and clears Desktop's Claude-name filter). Relay mode bypasses
    // the proxy — Desktop talks to the upstream directly — so the real upstream
    // id (e.g. "fable-5") must be sent as-is, or the station receives a model
    // name it never advertised. Fall back to the canonical id when no real id
    // is known, so we never emit an empty name.
    let base_model_id = if relay_mode && !real_model_id.is_empty() {
        real_model_id
    } else {
        "claude-opus-5-5"
    };
    // Offer the 1M variant in both routing modes. prefer1m selects it by default
    // without changing the model ID or adding a [1m] suffix.
    // We deliberately do NOT set `anthropicFamilyTier` / `isFamilyDefault`.
    // They were added to try to make the 1M variant the default, which turned
    // out to be an unfixable Desktop-side bug. Worse, hardcoding a tier
    // mislabels the model in relay mode: there the name is the real upstream id
    // (e.g. a Sonnet model), so tagging it "opus" makes Desktop's opus alias
    // resolve to a Sonnet model. The tier alias is optional — Desktop keeps the
    // model usable without it.
    let mut model_entry = serde_json::Map::new();
    model_entry.insert(
        "name".to_string(),
        serde_json::Value::String(base_model_id.to_string()),
    );
    model_entry.insert(
        "labelOverride".to_string(),
        serde_json::Value::String(real_model_id.to_string()),
    );
    model_entry.insert("supports1m".to_string(), serde_json::Value::Bool(true));
    model_entry.insert(
        "prefer1m".to_string(),
        serde_json::Value::Bool(model_info.one_m_context.unwrap_or(false)),
    );
    let model_entry = serde_json::Value::Object(model_entry);
    let profile = serde_json::json!({
        "disableDeploymentModeChooser": true,
        "inferenceGatewayApiKey": gateway_api_key,
        "inferenceGatewayAuthScheme": "bearer",
        "inferenceGatewayBaseUrl": gateway_base_url,
        "inferenceModels": [model_entry],
        "inferenceProvider": "gateway",
        // Claude Desktop 3P mode derives the built-in web_fetch egress policy
        // from this field. Without it, egress falls back to the gateway host
        // only (127.0.0.1), so web_fetch can't reach external URLs. ["*"]
        // lets web_fetch fetch any URL the user asks Claude to fetch.
        "coworkEgressAllowedHosts": ["*"],
    });
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

    // Relay — the real upstream config that anthropic_proxy reads on
    // every /v1/messages request. Updates here take effect on the next
    // request without restarting anything.
    // Relay file always reflects the real upstream config (even in Relay
    // mode where the proxy is bypassed for the network path), so `read_
    // claudedesktop` can round-trip the active model back to the UI
    // and so future debug tooling has a consistent source of truth.
    let relay = serde_json::json!({
        "baseUrl": anthropic_url,
        "apiKey": api_key,
        "actualModel": real_model_id,
        "modelName": model_info.name.as_deref().unwrap_or(real_model_id),
        "relayMode": relay_mode,
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
    // Source of truth is the relay file, NOT Claude Desktop's profile
    // JSON. The profile only holds the proxy URL (127.0.0.1:53682) and
    // a sentinel key; the real upstream URL / key / model id live in
    // ~/.echobird/claudedesktop.json so apply_claudedesktop can update
    // them without touching Desktop's config and triggering a restart.
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
        name: None,
        model: None,
        base_url: None,
        api_key,
        anthropic_url: Some(anthropic_url),
        protocol: Some("anthropic".to_string()),
        display_model: None,
        relay_mode: None,
        web_search: None,
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
            let profile = read_json_file(&profile_path).unwrap();
            assert_eq!(profile["inferenceModels"][0]["labelOverride"], model);
            assert_eq!(profile["inferenceModels"][0]["prefer1m"], relay_mode);
            assert_eq!(
                profile["inferenceModels"][0]["name"],
                if relay_mode { model } else { "claude-opus-5-5" }
            );
            assert_eq!(
                profile["inferenceGatewayBaseUrl"],
                if relay_mode {
                    "https://example.test/anthropic".to_string()
                } else {
                    format!("http://127.0.0.1:{}", anthropic_proxy::port())
                }
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
