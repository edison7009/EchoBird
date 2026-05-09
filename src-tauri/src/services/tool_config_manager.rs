// Tool config manager �?handles model configuration for all tools
// Ports the old Electron model.ts/model.cjs logic into Rust
// Each custom tool has its own apply/read function

use std::fs;
use std::path::{Path, PathBuf};

use crate::services::tool_manager;

/// Model info to apply to a tool
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
}

/// Result of applying a model config
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
}

// ─── Helpers ───

fn echobird_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".echobird")
}

fn ensure_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            let _ = fs::create_dir_all(parent);
        }
    }
}

/// Extract domain name from URL string without url crate
/// e.g. "https://api.deepseek.com/v1" �?"deepseek"
/// e.g. "http://localhost:8080" �?"local"
fn extract_domain_name(url: &str) -> String {
    // Strip protocol
    let without_protocol = url
        .strip_prefix("https://").or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    // Get host part (before / or :)
    let host = without_protocol.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or(host);

    if host == "localhost" || host.starts_with("127.") || host.starts_with("192.168.") {
        return "local".to_string();
    }

    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2].to_string()
    } else {
        host.to_string()
    }
}

/// Read JSON file, return Value or None
fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn read_jsonc_file(path: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&strip_jsonc_comments(&content)).ok()
}

fn strip_jsonc_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            out.push(c);
            continue;
        }

        if c == '"' {
            in_string = true;
            out.push(c);
            continue;
        }

        if c == '/' {
            match chars.peek().copied() {
                Some('/') => {
                    chars.next();
                    for next in chars.by_ref() {
                        if next == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                    continue;
                }
                Some('*') => {
                    chars.next();
                    let mut prev = '\0';
                    for next in chars.by_ref() {
                        if prev == '*' && next == '/' {
                            break;
                        }
                        prev = next;
                    }
                    continue;
                }
                _ => {}
            }
        }

        out.push(c);
    }

    out
}

/// Write JSON value to file with pretty formatting
fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    ensure_parent(path);
    let content = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

// ─── Known ModelInfo fields ───

const KNOWN_MODEL_FIELDS: &[&str] = &[
    "id", "name", "baseUrl", "apiKey", "model", "protocol",
];

fn get_model_field(model_info: &ModelInfo, field_name: &str) -> Option<String> {
    match field_name {
        "model" => model_info.model.clone(),
        "name" => model_info.name.clone(),
        "baseUrl" | "base_url" => model_info.base_url.clone(),
        "apiKey" | "api_key" => model_info.api_key.clone(),
        "protocol" => model_info.protocol.clone(),
        "anthropicUrl" | "anthropic_url" => model_info.anthropic_url.clone(),
        _ => None,
    }
}

// ════════════════════════════════════════════════════════════════
//  APPLY MODEL �?main entry point
// ════════════════════════════════════════════════════════════════

pub async fn apply_model_to_tool(tool_id: &str, model_info: ModelInfo) -> ApplyResult {
    log::info!("[ToolConfigManager] Applying model to {}", tool_id);
    let model_info = normalize_model_info_for_tool(tool_id, model_info);

    // Dispatch custom tools to their own handlers
    match tool_id {
        // OpenClaw: direct write to ~/.openclaw/openclaw.json (no patch needed since v2026.3.13)
        "openclaw" => return apply_openclaw(&model_info),

        // Type 3: Direct JSON overwrite (special format)
        "opencode" => return apply_opencode(&model_info),

        // Codex CLI and Codex Desktop share ~/.codex/config.toml
        "codex" | "codexdesktop" => return apply_codex(tool_id, &model_info),

        // Type 4: YAML
        "aider" => return apply_aider(&model_info),

        // Type 5: TOML
        "zeroclaw" => return apply_zeroclaw(&model_info),

        // Qwen Code: direct write to ~/.qwen/settings.json
        "qwencode" => return apply_qwen_code(&model_info),

        // Pi (earendil-works/pi): writes ~/.pi/agent/{models,settings}.json
        "pi" => return apply_pi(&model_info),

        // Plug-and-play: check config.json custom flag
        _ => {
            if let Some((def, _)) = tool_manager::get_tool_config_mapping(tool_id) {
                if def.config_mapping.custom {
                    return apply_echobird_relay(tool_id, &model_info, false);
                }
            }
        }
    }

    apply_generic_json(tool_id, &model_info).await
}

fn normalize_model_info_for_tool(tool_id: &str, mut model_info: ModelInfo) -> ModelInfo {
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

// ════════════════════════════════════════════════════════════════
//  RESTORE TO OFFICIAL — delete config so tool regenerates defaults
// ════════════════════════════════════════════════════════════════

/// Delete the tool's config file (and any Echobird relay side-channel) so
/// the tool itself regenerates a fresh, vendor-default config on next launch.
/// Used by the App Manager "restore to official" flow.
pub async fn restore_tool_to_official(tool_id: &str) -> ApplyResult {
    let config_path = match tool_manager::get_tool_config_mapping(tool_id) {
        Some((_, path)) => path,
        None => return ApplyResult {
            success: false,
            message: format!("Unknown tool: {}", tool_id),
        },
    };

    if matches!(tool_id, "codex" | "codexdesktop") {
        return restore_codex_to_official(tool_id, &config_path);
    }
    if tool_id == "opencode" {
        return restore_opencode_to_official();
    }
    if tool_id == "pi" {
        return restore_pi_to_official();
    }

    // Side-channel relay file (openclaw and other "custom" tools) —
    // best-effort cleanup, ignored if absent.
    let relay_path = echobird_dir().join(format!("{}.json", tool_id));
    if relay_path.exists() {
        let _ = fs::remove_file(&relay_path);
    }

    if !config_path.exists() {
        return ApplyResult {
            success: true,
            message: format!("{} already at defaults — no config file to remove.", tool_id),
        };
    }

    match fs::remove_file(&config_path) {
        Ok(_) => {
            log::info!("[ToolConfigManager] Restored {} — deleted {:?}", tool_id, config_path);
            ApplyResult {
                success: true,
                message: format!("{} restored — config deleted, tool will regenerate defaults on next launch.", tool_id),
            }
        }
        Err(e) => ApplyResult {
            success: false,
            message: format!("Failed to delete {} config: {}", tool_id, e),
        },
    }
}

// ════════════════════════════════════════════════════════════════
//  GET MODEL INFO �?main entry point
// ════════════════════════════════════════════════════════════════

pub async fn get_tool_model_info(tool_id: &str) -> Option<ModelInfo> {
    match tool_id {
        "openclaw" => return read_openclaw(),
        "opencode" => return read_opencode(),
        "codex" | "codexdesktop" => return read_codex(),
        "aider" => return read_aider(),
        "zeroclaw" => return read_zeroclaw(),
        "qwencode" => return read_qwen_code(),
        "pi" => return read_pi(),
        // Plug-and-play: check config.json custom flag
        _ => {
            if let Some((def, _)) = tool_manager::get_tool_config_mapping(tool_id) {
                if def.config_mapping.custom {
                    return read_echobird_relay(tool_id);
                }
            }
        }
    }

    read_generic_json(tool_id)
}

// ════════════════════════════════════════════════════════════════
//  Type 1: Generic JSON mapping (ClaudeCode, etc.)
// ════════════════════════════════════════════════════════════════

async fn apply_generic_json(tool_id: &str, model_info: &ModelInfo) -> ApplyResult {
    let (def, config_path) = match tool_manager::get_tool_config_mapping(tool_id) {
        Some(pair) => pair,
        None => {
            return ApplyResult {
                success: false,
                message: format!("Unknown tool: {}", tool_id),
            };
        }
    };

    let cm = &def.config_mapping;

    if cm.format != "json" {
        return ApplyResult {
            success: false,
            message: format!("Config format '{}' not supported for generic apply", cm.format),
        };
    }

    let write_map = match &cm.write {
        Some(w) => w.clone(),
        None => {
            return ApplyResult {
                success: false,
                message: format!("Tool '{}' has no write mapping defined", tool_id),
            };
        }
    };

    let mut config = read_json_file(&config_path).unwrap_or(serde_json::json!({}));

    for (config_json_path, model_field) in &write_map {
        let value = get_model_field(model_info, model_field);
        if let Some(val) = value {
            tool_manager::set_nested_value(
                &mut config, config_json_path,
                serde_json::Value::String(val),
            );
        } else if model_field.is_empty() {
            tool_manager::set_nested_value(
                &mut config, config_json_path,
                serde_json::Value::String(String::new()),
            );
        } else if !KNOWN_MODEL_FIELDS.contains(&model_field.as_str()) {
            tool_manager::set_nested_value(
                &mut config, config_json_path,
                serde_json::Value::String(model_field.clone()),
            );
        }
    }

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!("[ToolConfigManager] Config written to {:?}", config_path);
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" applied to {} successfully.",
                    model_info.model.as_deref().unwrap_or(""), tool_id
                ),
            }
        }
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_generic_json(tool_id: &str) -> Option<ModelInfo> {
    let (def, config_path) = tool_manager::get_tool_config_mapping(tool_id)?;
    let cm = &def.config_mapping;
    if cm.format != "json" { return None; }
    let read_map = cm.read.as_ref()?;
    let config = read_json_file(&config_path)?;

    let read_field = |paths: &Option<Vec<String>>| -> Option<String> {
        for p in paths.as_ref()? {
            if let Some(val) = tool_manager::get_nested_value(&config, p) {
                if let Some(s) = val.as_str() {
                    if !s.is_empty() { return Some(s.to_string()); }
                }
            }
        }
        None
    };

    let model = read_field(&read_map.model);
    if model.is_none() { return None; }

    Some(ModelInfo {
        name: None, model,
        base_url: read_field(&read_map.base_url),
        api_key: read_field(&read_map.api_key),
        anthropic_url: None,
        protocol: None,
    })
}


// ════════════════════════════════════════════════════════════════
//  Type 2: Echobird relay JSON (OpenClaw + custom plug-and-play tools)
//  Write to ~/.echobird/{tool_id}.json
// ════════════════════════════════════════════════════════════════

fn apply_echobird_relay(tool_id: &str, model_info: &ModelInfo, include_provider: bool) -> ApplyResult {
    let config_path = echobird_dir().join(format!("{}.json", tool_id));
    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");

    if model_id.is_empty() {
        return ApplyResult {
            success: false,
            message: "Model ID is empty, cannot apply config".to_string(),
        };
    }

    let mut config = serde_json::json!({
        "apiKey": model_info.api_key.as_deref().unwrap_or(""),
        "modelId": model_id,
        "modelName": model_info.name.as_deref().unwrap_or(model_id),
    });

    if let Some(ref base_url) = model_info.base_url {
        config["baseUrl"] = serde_json::Value::String(base_url.clone());
    }
    if include_provider {
        config["provider"] = serde_json::Value::String("openai".to_string());
    }
    if tool_id == "openclaw" {
        config["protocol"] = serde_json::Value::String(
            model_info.protocol.as_deref().unwrap_or("openai").to_string()
        );
    }

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!("[ToolConfigManager] {} config written to {:?}", tool_id, config_path);
            crate::services::tool_patcher::patch_tool(tool_id);
            let tool_display = match tool_id {
                "openclaw" => "OpenClaw",
                _ => tool_id,
            };
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for {}. Restart to apply.",
                    model_info.name.as_deref().unwrap_or(model_id), tool_display
                ),
            }
        }
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_echobird_relay(tool_id: &str) -> Option<ModelInfo> {
    let config_path = echobird_dir().join(format!("{}.json", tool_id));
    let config = read_json_file(&config_path)?;
    let model_id = config.get("modelId")?.as_str()?.to_string();
    if model_id.is_empty() { return None; }

    Some(ModelInfo {
        name: config.get("modelName").and_then(|v| v.as_str()).map(|s| s.to_string()),
        model: Some(model_id),
        base_url: config.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: config.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None,
        protocol: config.get("protocol").and_then(|v| v.as_str()).map(|s| s.to_string()),
    })
}


// ════════════════════════════════════════════════════════════════
//  OpenClaw: Direct write to ~/.openclaw/openclaw.json
//  v2026.3.13+: models.providers custom provider, mode: "merge"
//  No longer patches openclaw.mjs — writes native config directly.
// ════════════════════════════════════════════════════════════════

fn apply_openclaw(model_info: &ModelInfo) -> ApplyResult {
    let home = dirs::home_dir().unwrap_or_default();
    let oc_dir = home.join(".openclaw");
    let oc_config_path = oc_dir.join("openclaw.json");

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");

    if model_id.is_empty() {
        return ApplyResult {
            success: false,
            message: "Model ID is empty, cannot apply config".to_string(),
        };
    }

    let api_key = model_info.api_key.as_deref().unwrap_or("");
    if api_key.is_empty() {
        return ApplyResult {
            success: false,
            message: "API Key is empty, cannot apply config".to_string(),
        };
    }

    // Preserve gateway token from existing config (if any)
    let gateway = if oc_config_path.exists() {
        read_json_file(&oc_config_path)
            .and_then(|c| c.get("gateway").cloned())
    } else {
        None
    };

    // Determine protocol and API type
    let protocol = model_info.protocol.as_deref().unwrap_or("openai");
    let is_anthropic = protocol == "anthropic"
        || model_id.to_lowercase().contains("claude")
        || model_info.base_url.as_deref().unwrap_or("").to_lowercase().contains("anthropic");
    let api_type = if is_anthropic { "anthropic-messages" } else { "openai-completions" };

    // Extract provider tag from base URL
    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/');
    let provider_tag = extract_domain_name(base_url);
    let eb_provider = format!("eb_{}", provider_tag);

    // Build fresh openclaw.json — full overwrite, no merge
    let mut oc_config = serde_json::json!({
        "models": {
            "mode": "merge",
            "providers": {
                eb_provider.clone(): {
                    "baseUrl": base_url,
                    "apiKey": api_key,
                    "api": api_type,
                    "models": [{
                        "id": model_id,
                        "name": model_info.name.as_deref().unwrap_or(model_id),
                        "contextWindow": 128000,
                        "maxTokens": 8192,
                        "input": ["text"],
                        "reasoning": false,
                        "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
                    }]
                }
            }
        },
        "agents": {
            "defaults": {
                "model": {
                    "primary": format!("{}/{}", eb_provider, model_id)
                }
            }
        }
    });

    // Restore gateway token
    if let Some(gw) = gateway {
        oc_config["gateway"] = gw;
    }

    // Write fresh config
    ensure_parent(&oc_config_path);
    if let Err(e) = write_json_file(&oc_config_path, &oc_config) {
        return ApplyResult { success: false, message: format!("Failed to write openclaw.json: {}", e) };
    }

    // Also write ~/.echobird/openclaw.json relay (used by Bridge/Channels)
    let relay_path = echobird_dir().join("openclaw.json");
    let relay = serde_json::json!({
        "apiKey": api_key,
        "modelId": model_id,
        "modelName": model_info.name.as_deref().unwrap_or(model_id),
        "baseUrl": base_url,
        "protocol": protocol,
    });
    let _ = write_json_file(&relay_path, &relay);

    log::info!("[ToolConfigManager] OpenClaw config overwritten: {}/{} ({})", eb_provider, model_id, api_type);

    ApplyResult {
        success: true,
        message: format!(
            "Model \"{}\" configured for OpenClaw ({}/{}).",
            model_info.name.as_deref().unwrap_or(model_id), eb_provider, model_id
        ),
    }
}

fn read_openclaw() -> Option<ModelInfo> {
    let oc_config_path = dirs::home_dir()?.join(".openclaw").join("openclaw.json");
    let config = read_json_file(&oc_config_path)?;

    // Read primary model: agents.defaults.model.primary = "eb_xxx/model-id"
    let primary = config.pointer("/agents/defaults/model/primary")
        .and_then(|v| v.as_str())?;

    // Parse "provider/model" format
    let (provider_name, model_id) = primary.split_once('/')?;

    // Only read eb_ providers (our custom ones)
    if !provider_name.starts_with("eb_") {
        return None;
    }

    // Get provider details from models.providers
    let provider = config.pointer(&format!("/models/providers/{}", provider_name))?;

    let base_url = provider.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string());
    let api_key = provider.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string());
    let api_type = provider.get("api").and_then(|v| v.as_str()).unwrap_or("openai-completions");
    let protocol = if api_type.contains("anthropic") { "anthropic" } else { "openai" };

    // Get model name from models array
    let model_name = provider.get("models")
        .and_then(|m| m.as_array())
        .and_then(|arr| arr.iter().find(|m| m.get("id").and_then(|v| v.as_str()) == Some(model_id)))
        .and_then(|m| m.get("name").and_then(|v| v.as_str()))
        .map(|s| s.to_string());

    Some(ModelInfo {
        name: model_name,
        model: Some(model_id.to_string()),
        base_url,
        api_key,
        anthropic_url: None,
        protocol: Some(protocol.to_string()),
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 3b: OpenCode
//  ~/.config/opencode/opencode.json  {provider: {X: {npm, options, models}}}
// ════════════════════════════════════════════════════════════════

fn apply_opencode(model_info: &ModelInfo) -> ApplyResult {
    // Write echobird relay JSON — the patched launcher reads this
    let config_path = echobird_dir().join("opencode.json");
    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("");

    if model_id.is_empty() {
        return ApplyResult {
            success: false,
            message: "Model ID is empty, cannot apply config".to_string(),
        };
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let provider_name = format!("{} (via Echobird)", extract_domain_name(&base_url));

    let config = serde_json::json!({
        "apiKey": model_info.api_key.as_deref().unwrap_or(""),
        "baseUrl": base_url,
        "modelId": model_id,
        "modelName": model_info.name.as_deref().unwrap_or(model_id),
        "providerName": provider_name,
    });

    if let Err(e) = write_opencode_native_config(model_info, model_id, &base_url, &provider_name) {
        return ApplyResult { success: false, message: e };
    }

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!("[ToolConfigManager] OpenCode config written to {:?}", config_path);
            crate::services::tool_patcher::patch_opencode();
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for OpenCode. Use /models in TUI to select echobird/{}.",
                    model_info.name.as_deref().unwrap_or(model_id), model_id
                ),
            }
        }
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn write_opencode_native_config(
    model_info: &ModelInfo,
    model_id: &str,
    base_url: &str,
    provider_name: &str,
) -> Result<(), String> {
    let config_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("opencode")
        .join("opencode.jsonc");

    let mut config = read_jsonc_file(&config_path)
        .or_else(|| read_json_file(&config_path.with_extension("json")))
        .unwrap_or(serde_json::json!({}));

    if config.get("$schema").is_none() {
        config["$schema"] = serde_json::json!("https://opencode.ai/config.json");
    }
    if !config.get("provider").map(|v| v.is_object()).unwrap_or(false) {
        config["provider"] = serde_json::json!({});
    }

    let provider_id = "echobird";
    config["provider"][provider_id] = serde_json::json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": provider_name,
        "options": {
            "baseURL": base_url,
            "apiKey": model_info.api_key.as_deref().unwrap_or("")
        },
        "models": {
            model_id: {
                "name": model_info.name.as_deref().unwrap_or(model_id)
            }
        }
    });
    config["model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));
    config["small_model"] = serde_json::Value::String(format!("{}/{}", provider_id, model_id));

    write_json_file(&config_path, &config)
}

fn read_opencode() -> Option<ModelInfo> {
    let native_path = dirs::home_dir()?.join(".config").join("opencode").join("opencode.jsonc");
    if let Some(info) = read_opencode_native_config(&native_path)
        .or_else(|| read_opencode_native_config(&native_path.with_extension("json")))
    {
        return Some(info);
    }

    // Read from echobird relay JSON
    let config_path = echobird_dir().join("opencode.json");
    let config = read_json_file(&config_path)?;
    let model_id = config.get("modelId")?.as_str()?.to_string();
    if model_id.is_empty() { return None; }

    Some(ModelInfo {
        name: config.get("modelName").and_then(|v| v.as_str()).map(|s| s.to_string()),
        model: Some(model_id),
        base_url: config.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: config.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None, protocol: None,
    })
}

fn read_opencode_native_config(path: &Path) -> Option<ModelInfo> {
    let config = if path.extension().and_then(|e| e.to_str()) == Some("jsonc") {
        read_jsonc_file(path)?
    } else {
        read_json_file(path)?
    };
    let selected = config.get("model")?.as_str()?;
    let (provider_id, model_id) = selected.split_once('/')?;
    let provider = config.pointer(&format!("/provider/{}", provider_id))?;

    Some(ModelInfo {
        name: provider
            .pointer(&format!("/models/{}/name", model_id))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        model: Some(model_id.to_string()),
        base_url: provider.pointer("/options/baseURL").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: provider.pointer("/options/apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None,
        protocol: Some("openai".to_string()),
    })
}

fn restore_opencode_to_official() -> ApplyResult {
    let relay_path = echobird_dir().join("opencode.json");
    if relay_path.exists() {
        let _ = fs::remove_file(&relay_path);
    }

    let native_path = dirs::home_dir()
        .unwrap_or_default()
        .join(".config")
        .join("opencode")
        .join("opencode.jsonc");

    if !native_path.exists() {
        return ApplyResult {
            success: true,
            message: "OpenCode already at defaults - no config file to update.".to_string(),
        };
    }

    let mut updated_any = false;
    for path in [&native_path, &native_path.with_extension("json")] {
        if !path.exists() {
            continue;
        }

        let mut config = match read_jsonc_file(path) {
            Some(c) => c,
            None => return ApplyResult {
                success: false,
                message: format!("Failed to parse OpenCode config: {}", path.display()),
            },
        };

        if let Some(provider) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
            provider.remove("echobird");
        }
        if config.get("model").and_then(|v| v.as_str()).map(|s| s.starts_with("echobird/")).unwrap_or(false) {
            tool_manager::delete_nested_value(&mut config, "model");
        }
        if config.get("small_model").and_then(|v| v.as_str()).map(|s| s.starts_with("echobird/")).unwrap_or(false) {
            tool_manager::delete_nested_value(&mut config, "small_model");
        }

        if let Err(e) = write_json_file(path, &config) {
            return ApplyResult { success: false, message: e };
        }
        updated_any = true;
    }

    if updated_any {
        ApplyResult {
            success: true,
            message: "OpenCode restored - Echobird provider removed.".to_string(),
        }
    } else {
        ApplyResult {
            success: true,
            message: "OpenCode already at defaults - no config file to update.".to_string(),
        }
    }
}


// ════════════════════════════════════════════════════════════════
//  Type 4a: Aider �?~/.aider.conf.yml (simple YAML key: value)
// ════════════════════════════════════════════════════════════════

// Codex CLI and Codex Desktop share ~/.codex/config.toml.
fn codex_env_key(base_url: &str) -> String {
    let domain = extract_domain_name(base_url);
    format!("ECHOBIRD_{}_API_KEY", domain.to_uppercase())
}

fn codex_provider_id(base_url: &str) -> String {
    let domain = extract_domain_name(base_url)
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect::<String>();
    format!("echobird_{}", if domain.is_empty() { "openai" } else { &domain })
}

fn set_user_env_var(key: &str, value: &str) {
    std::env::set_var(key, value);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("setx")
            .args([key, value])
            .creation_flags(0x08000000)
            .output();
    }
}

fn apply_codex(tool_id: &str, model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".codex").join("config.toml");
    let mut content = fs::read_to_string(&config_path).unwrap_or_default();

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref())
        .unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult { success: false, message: "Model ID is empty".to_string() };
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1")
        .trim_end_matches('/')
        .to_string();
    let api_key = model_info.api_key.as_deref().unwrap_or("");
    if api_key.is_empty() {
        return ApplyResult { success: false, message: "API Key is empty, cannot apply Codex config".to_string() };
    }

    let provider_id = codex_provider_id(&base_url);
    let provider_name = format!("{} (via Echobird)", extract_domain_name(&base_url));
    let env_key = codex_env_key(&base_url);

    content = toml_write_top(&content, "model_provider", &provider_id);
    content = toml_write_top(&content, "model", model_id);
    if toml_read_top(&content, "model_reasoning_effort").is_empty() {
        content = toml_write_top(&content, "model_reasoning_effort", "high");
    }
    content = toml_write_table_value(&content, &format!("model_providers.{}", provider_id), "name", &provider_name);
    content = toml_write_table_value(&content, &format!("model_providers.{}", provider_id), "base_url", &base_url);
    content = toml_write_table_value(&content, &format!("model_providers.{}", provider_id), "env_key", &env_key);

    ensure_parent(&config_path);
    if let Err(e) = fs::write(&config_path, &content) {
        return ApplyResult { success: false, message: format!("Codex config error: {}", e) };
    }

    set_user_env_var(&env_key, api_key);

    let relay_path = echobird_dir().join("codex.json");
    let relay = serde_json::json!({
        "apiKey": api_key,
        "baseUrl": base_url,
        "modelId": model_id,
        "modelName": model_info.name.as_deref().unwrap_or(model_id),
        "providerId": provider_id,
        "envKey": env_key,
    });
    let _ = write_json_file(&relay_path, &relay);

    let display = if tool_id == "codexdesktop" { "Codex Desktop" } else { "Codex CLI" };
    ApplyResult {
        success: true,
        message: format!("Model \"{}\" configured for {}.", model_info.name.as_deref().unwrap_or(model_id), display),
    }
}

fn read_codex() -> Option<ModelInfo> {
    let config_path = dirs::home_dir()?.join(".codex").join("config.toml");
    let content = fs::read_to_string(&config_path).ok()?;

    let model = toml_read_top(&content, "model");
    if model.is_empty() { return None; }

    let provider_id = toml_read_top(&content, "model_provider");
    let base_url = if provider_id.is_empty() {
        None
    } else {
        let value = toml_read_table_value(&content, &format!("model_providers.{}", provider_id), "base_url");
        if value.is_empty() { None } else { Some(value) }
    };

    let env_key = if provider_id.is_empty() {
        String::new()
    } else {
        toml_read_table_value(&content, &format!("model_providers.{}", provider_id), "env_key")
    };
    let api_key = if env_key.is_empty() {
        None
    } else {
        std::env::var(&env_key).ok()
    };

    Some(ModelInfo {
        name: Some(model.clone()),
        model: Some(model),
        base_url,
        api_key,
        anthropic_url: None,
        protocol: Some("openai".to_string()),
    })
}

fn restore_codex_to_official(tool_id: &str, config_path: &Path) -> ApplyResult {
    let mut content = fs::read_to_string(config_path).unwrap_or_default();
    content = toml_write_top(&content, "model_provider", "openai");
    content = toml_write_top(&content, "model", "gpt-4o");
    content = toml_remove_tables_with_prefix(&content, "model_providers.echobird_");

    ensure_parent(config_path);
    match fs::write(config_path, content) {
        Ok(_) => {
            let relay_path = echobird_dir().join("codex.json");
            if relay_path.exists() {
                let _ = fs::remove_file(&relay_path);
            }
            ApplyResult {
                success: true,
                message: format!("{} restored to OpenAI official provider.", if tool_id == "codexdesktop" { "Codex Desktop" } else { "Codex CLI" }),
            }
        }
        Err(e) => ApplyResult {
            success: false,
            message: format!("Failed to restore Codex config: {}", e),
        },
    }
}

fn apply_aider(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".aider.conf.yml");
    let mut content = fs::read_to_string(&config_path).unwrap_or_default();

    let model = model_info.model.as_deref().or(model_info.name.as_deref()).unwrap_or("");
    if !model.is_empty() { content = yaml_write(&content, "model", model); }

    let protocol = model_info.protocol.as_deref().unwrap_or("openai");
    if protocol == "anthropic" {
        if let Some(ref k) = model_info.api_key { content = yaml_write(&content, "anthropic-api-key", k); }
        content = yaml_remove(&content, "openai-api-key");
        content = yaml_remove(&content, "openai-api-base");
    } else {
        if let Some(ref k) = model_info.api_key { content = yaml_write(&content, "openai-api-key", k); }
        if let Some(ref u) = model_info.base_url { content = yaml_write(&content, "openai-api-base", u); }
        content = yaml_remove(&content, "anthropic-api-key");
    }

    ensure_parent(&config_path);
    match fs::write(&config_path, &content) {
        Ok(_) => ApplyResult {
            success: true,
            message: format!("Model \"{}\" applied to Aider ({}).", model, protocol),
        },
        Err(e) => ApplyResult { success: false, message: format!("Aider error: {}", e) },
    }
}

fn read_aider() -> Option<ModelInfo> {
    let path = dirs::home_dir()?.join(".aider.conf.yml");
    let content = fs::read_to_string(&path).ok()?;
    let model = yaml_read(&content, "model");
    if model.is_empty() { return None; }
    let ok = yaml_read(&content, "openai-api-key");
    let ak = yaml_read(&content, "anthropic-api-key");
    let api_key = if !ok.is_empty() { Some(ok) } else if !ak.is_empty() { Some(ak) } else { None };
    let bu = yaml_read(&content, "openai-api-base");
    Some(ModelInfo {
        name: Some(model.clone()), model: Some(model),
        base_url: if bu.is_empty() { None } else { Some(bu) },
        api_key, anthropic_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 5: ZeroClaw → ~/.zeroclaw/config.toml
// ════════════════════════════════════════════════════════════════

fn apply_zeroclaw(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".zeroclaw").join("config.toml");
    let mut content = fs::read_to_string(&config_path).unwrap_or_default();

    let model = model_info.model.as_deref().or(model_info.name.as_deref()).unwrap_or("");
    if !model.is_empty() { content = toml_write_top(&content, "default_model", model); }

    // Build ZeroClaw provider string (matches Bridge handle_set_model logic)
    if let Some(ref u) = model_info.base_url {
        let base = u.trim_end_matches('/');
        let provider_value = if base.contains("openrouter.ai") {
            "openrouter".to_string()
        } else if base.contains("anthropic.com") {
            "anthropic".to_string()
        } else if base.contains("openai.com") {
            "openai".to_string()
        } else {
            // Custom provider: use URL as-is (ZeroClaw handles endpoint paths internally)
            format!("custom:{}", base)
        };
        content = toml_write_top(&content, "default_provider", &provider_value);
    } else {
        content = toml_write_top(&content, "default_provider", "openrouter");
    }

    // default_temperature is required by ZeroClaw config parser
    content = toml_write_top(&content, "default_temperature", "0.7");

    // Write api_key to config.toml (official ZeroClaw config key) + env vars as fallback
    if let Some(ref k) = model_info.api_key {
        content = toml_write_top(&content, "api_key", k);
        std::env::set_var("OPENROUTER_API_KEY", k);
        std::env::set_var("OPENAI_API_KEY", k);
    }

    ensure_parent(&config_path);
    match fs::write(&config_path, &content) {
        Ok(_) => ApplyResult {
            success: true,
            message: format!("Model \"{}\" applied to ZeroClaw.", model),
        },
        Err(e) => ApplyResult { success: false, message: format!("ZeroClaw error: {}", e) },
    }
}

fn read_zeroclaw() -> Option<ModelInfo> {
    let path = dirs::home_dir()?.join(".zeroclaw").join("config.toml");
    let content = fs::read_to_string(&path).ok()?;
    let model = toml_read_top(&content, "default_model");
    if model.is_empty() { return None; }
    let key = toml_read_top(&content, "api_key");
    let prov = toml_read_top(&content, "default_provider");
    let base_url = prov.strip_prefix("custom:").map(|s| s.to_string());

    Some(ModelInfo {
        name: Some(model.clone()), model: Some(model), base_url,
        api_key: if key.is_empty() { None } else { Some(key) },
        anthropic_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Qwen Code: direct write to ~/.qwen/settings.json
//  Format: { modelProviders: { openai: [...] }, env: {...},
//           security: { auth: { selectedType } }, model: { name } }
// ════════════════════════════════════════════════════════════════

fn apply_qwen_code(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".qwen").join("settings.json");

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult { success: false, message: "Model ID is empty".to_string() };
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let api_key = model_info.api_key.as_deref().unwrap_or("");
    let protocol = model_info.protocol.as_deref().unwrap_or("openai");

    // Qwen Code supports: openai, anthropic, gemini
    let selected_type = match protocol {
        "anthropic" => "anthropic",
        "gemini" => "gemini",
        _ => "openai",
    };

    // Env key name derived from domain to avoid collisions
    let domain = extract_domain_name(&base_url);
    let env_key = format!("ECHOBIRD_{}_API_KEY", domain.to_uppercase());
    let display_name = model_info.name.as_deref().unwrap_or(model_id);

    // Read existing config or start fresh
    let mut config = read_json_file(&config_path).unwrap_or(serde_json::json!({}));

    // Build the model provider entry
    let provider_entry = serde_json::json!({
        "id": model_id,
        "name": display_name,
        "baseUrl": base_url,
        "description": format!("via Echobird ({})", domain),
        "envKey": env_key
    });

    // Write modelProviders — replace the protocol array with single entry
    config["modelProviders"][selected_type] = serde_json::json!([provider_entry]);

    // Write env — set the API key
    config["env"][&env_key] = serde_json::Value::String(api_key.to_string());

    // Write security auth type
    config["security"]["auth"]["selectedType"] = serde_json::Value::String(selected_type.to_string());

    // Write active model
    config["model"]["name"] = serde_json::Value::String(model_id.to_string());

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!("[ToolConfigManager] QwenCode config written: {} ({})", model_id, selected_type);
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for Qwen Code. Restart qwen to apply.",
                    display_name
                ),
            }
        }
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_qwen_code() -> Option<ModelInfo> {
    let config_path = dirs::home_dir()?.join(".qwen").join("settings.json");
    let config = read_json_file(&config_path)?;

    // Read active model name
    let model_name = config.pointer("/model/name")?.as_str()?.to_string();
    if model_name.is_empty() { return None; }

    // Read auth type to determine which provider array to look at
    let selected_type = config.pointer("/security/auth/selectedType")
        .and_then(|v| v.as_str()).unwrap_or("openai");

    // Find the model entry in the provider array
    let providers = config.pointer(&format!("/modelProviders/{}", selected_type))
        .and_then(|v| v.as_array())?;

    let entry = providers.iter().find(|e| {
        e.get("id").and_then(|v| v.as_str()) == Some(&model_name)
    }).or_else(|| providers.first())?;

    let base_url = entry.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string());

    // Resolve API key from env object via envKey reference
    let api_key = entry.get("envKey").and_then(|v| v.as_str())
        .and_then(|env_key| config.pointer(&format!("/env/{}", env_key)))
        .and_then(|v| v.as_str()).map(|s| s.to_string());

    Some(ModelInfo {
        name: entry.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        model: Some(model_name),
        base_url,
        api_key,
        anthropic_url: None,
        protocol: Some(selected_type.to_string()),
    })
}

// ════════════════════════════════════════════════════════════════
//  Pi (earendil-works/pi) — split config:
//   ~/.pi/agent/models.json    — provider definitions (custom OpenAI/Anthropic-compat)
//   ~/.pi/agent/settings.json  — defaultProvider + defaultModel
//  We register a single "echobird" provider in models.json and point
//  settings.json at it. Anthropic-protocol models switch the api type
//  to "anthropic-messages" and use anthropicUrl as the baseUrl.
//  Docs: https://pi.dev/docs/latest/models
// ════════════════════════════════════════════════════════════════

fn pi_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".pi").join("agent")
}

fn apply_pi(model_info: &ModelInfo) -> ApplyResult {
    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult { success: false, message: "Model ID is empty".to_string() };
    }

    let protocol = model_info.protocol.as_deref().unwrap_or("openai");
    let use_anthropic = protocol == "anthropic" && model_info.anthropic_url.is_some();
    let (base_url, api_type) = if use_anthropic {
        (
            model_info.anthropic_url.as_deref().unwrap_or("").trim_end_matches('/').to_string(),
            "anthropic-messages",
        )
    } else {
        (
            model_info.base_url.as_deref().unwrap_or("https://api.openai.com/v1")
                .trim_end_matches('/').to_string(),
            "openai-completions",
        )
    };

    let provider_id = "echobird";

    // models.json — register/replace the echobird provider
    let models_path = pi_dir().join("models.json");
    let mut models_config = read_json_file(&models_path).unwrap_or(serde_json::json!({}));
    if !models_config.is_object() { models_config = serde_json::json!({}); }
    if !models_config.get("providers").map(|v| v.is_object()).unwrap_or(false) {
        models_config["providers"] = serde_json::json!({});
    }
    models_config["providers"][provider_id] = serde_json::json!({
        "baseUrl": base_url,
        "api": api_type,
        "apiKey": model_info.api_key.as_deref().unwrap_or(""),
        "models": [{ "id": model_id }]
    });
    if let Err(e) = write_json_file(&models_path, &models_config) {
        return ApplyResult { success: false, message: e };
    }

    // settings.json — point defaultProvider/defaultModel at our provider
    let settings_path = pi_dir().join("settings.json");
    let mut settings = read_json_file(&settings_path).unwrap_or(serde_json::json!({}));
    if !settings.is_object() { settings = serde_json::json!({}); }
    settings["defaultProvider"] = serde_json::Value::String(provider_id.to_string());
    settings["defaultModel"] = serde_json::Value::String(model_id.to_string());
    if let Err(e) = write_json_file(&settings_path, &settings) {
        return ApplyResult { success: false, message: e };
    }

    log::info!("[ToolConfigManager] Pi configured: provider={}, model={}, api={}", provider_id, model_id, api_type);
    ApplyResult {
        success: true,
        message: format!(
            "Model \"{}\" configured for Pi ({}). Restart pi to apply.",
            model_info.name.as_deref().unwrap_or(model_id), api_type
        ),
    }
}

fn read_pi() -> Option<ModelInfo> {
    let dir = pi_dir();
    let settings = read_json_file(&dir.join("settings.json"))?;
    let provider_id = settings.get("defaultProvider")?.as_str()?;
    let model_id = settings.get("defaultModel")?.as_str()?.to_string();
    if model_id.is_empty() { return None; }

    let models = read_json_file(&dir.join("models.json"))?;
    let prov = models.pointer(&format!("/providers/{}", provider_id))?;

    let api_type = prov.get("api").and_then(|v| v.as_str()).unwrap_or("openai-completions");
    let base_url = prov.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string());
    let api_key = prov.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string());

    let (base, anthro) = if api_type == "anthropic-messages" {
        (None, base_url)
    } else {
        (base_url, None)
    };

    Some(ModelInfo {
        name: Some(model_id.clone()),
        model: Some(model_id),
        base_url: base,
        api_key,
        anthropic_url: anthro,
        protocol: Some(if api_type == "anthropic-messages" { "anthropic" } else { "openai" }.to_string()),
    })
}

fn restore_pi_to_official() -> ApplyResult {
    let dir = pi_dir();
    // Delete only our additions — the echobird provider entry in models.json,
    // and clear defaultProvider/defaultModel in settings.json. Don't nuke the
    // files (other providers and unrelated settings stay intact).
    let models_path = dir.join("models.json");
    if let Some(mut models) = read_json_file(&models_path) {
        if models.get("providers").map(|v| v.is_object()).unwrap_or(false) {
            if let Some(obj) = models["providers"].as_object_mut() {
                obj.remove("echobird");
            }
            let _ = write_json_file(&models_path, &models);
        }
    }

    let settings_path = dir.join("settings.json");
    if let Some(mut settings) = read_json_file(&settings_path) {
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("defaultProvider");
            obj.remove("defaultModel");
        }
        let _ = write_json_file(&settings_path, &settings);
    }

    ApplyResult {
        success: true,
        message: "Pi restored — echobird provider removed, defaults cleared. Pi will fall back to its built-in providers on next launch.".to_string(),
    }
}

// ════════════════════════════════════════════════════════════════
//  Simple YAML helpers (key: value format only)
// ════════════════════════════════════════════════════════════════

fn yaml_read(content: &str, key: &str) -> String {
    let prefix = format!("{}:", key);
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('#') { continue; }
        if let Some(rest) = t.strip_prefix(&prefix) {
            let v = rest.trim();
            if (v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')) {
                if v.len() >= 2 { return v[1..v.len()-1].to_string(); }
            }
            return v.to_string();
        }
    }
    String::new()
}

fn yaml_write(content: &str, key: &str, value: &str) -> String {
    let prefix = format!("{}:", key);
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found = false;
    for line in lines.iter_mut() {
        let t = line.trim();
        if !t.starts_with('#') && t.starts_with(&prefix) {
            *line = format!("{}: {}", key, value);
            found = true;
            break;
        }
    }
    if !found { lines.push(format!("{}: {}", key, value)); }
    lines.join("\n")
}

fn yaml_remove(content: &str, key: &str) -> String {
    let prefix = format!("{}:", key);
    content.lines()
        .filter(|l| l.trim().starts_with('#') || !l.trim().starts_with(&prefix))
        .collect::<Vec<_>>().join("\n")
}

// ════════════════════════════════════════════════════════════════
//  Simple TOML helpers (top-level key = "value" only)
// ════════════════════════════════════════════════════════════════

fn toml_read_top(content: &str, key: &str) -> String {
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') || t.starts_with('#') || t.is_empty() {
            if t.starts_with('[') { break; } // Entered sections, stop
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    return v[1..v.len()-1].to_string();
                }
                return v.to_string();
            }
        }
    }
    String::new()
}

fn toml_write_top(content: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut found = false;
    let mut first_section: Option<usize> = None;

    for (i, line) in lines.iter_mut().enumerate() {
        let t = line.trim();
        if first_section.is_none() && t.starts_with('[') { first_section = Some(i); }
        if first_section.is_some() && i >= first_section.unwrap() { continue; }
        if let Some((k, _)) = t.split_once('=') {
            if k.trim() == key {
                *line = format!("{} = \"{}\"", key, toml_escape(value));
                found = true;
                break;
            }
        }
    }

    if !found {
        let new_line = format!("{} = \"{}\"", key, toml_escape(value));
        match first_section {
            Some(i) => lines.insert(i, new_line),
            None => lines.push(new_line),
        }
    }
    lines.join("\n")
}

fn toml_read_table_value(content: &str, table: &str, key: &str) -> String {
    let header = format!("[{}]", table);
    let mut in_table = false;

    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            in_table = t == header;
            continue;
        }
        if !in_table || t.starts_with('#') || t.is_empty() {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                return toml_unquote(v.trim());
            }
        }
    }

    String::new()
}

fn toml_write_table_value(content: &str, table: &str, key: &str, value: &str) -> String {
    let header = format!("[{}]", table);
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let mut table_start = None;
    let mut table_end = lines.len();

    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            if t == header {
                table_start = Some(i);
            } else if table_start.is_some() {
                table_end = i;
                break;
            }
        }
    }

    let new_line = format!("{} = \"{}\"", key, toml_escape(value));

    if let Some(start) = table_start {
        for line in lines.iter_mut().take(table_end).skip(start + 1) {
            let t = line.trim();
            if let Some((k, _)) = t.split_once('=') {
                if k.trim() == key {
                    *line = new_line;
                    return lines.join("\n");
                }
            }
        }
        lines.insert(table_end, new_line);
    } else {
        if !lines.is_empty() && !lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
            lines.push(String::new());
        }
        lines.push(header);
        lines.push(new_line);
    }

    lines.join("\n")
}

fn toml_remove_tables_with_prefix(content: &str, prefix: &str) -> String {
    let mut out = Vec::new();
    let mut removing = false;

    for line in content.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            let table = &t[1..t.len() - 1];
            removing = table.starts_with(prefix);
            if removing {
                continue;
            }
        }
        if !removing {
            out.push(line);
        }
    }

    out.join("\n")
}

fn toml_unquote(value: &str) -> String {
    let v = value.trim();
    if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
        v[1..v.len()-1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        v.to_string()
    }
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
