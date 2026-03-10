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
    pub proxy_url: Option<String>,
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

/// Extract root domain e.g. "api.minimaxi.com" �?"minimaxi.com"
fn extract_root_domain(url: &str) -> String {
    let without_protocol = url
        .strip_prefix("https://").or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = without_protocol.split('/').next().unwrap_or("");
    let host = host.split(':').next().unwrap_or(host);

    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() >= 2 {
        parts[parts.len() - 2..].join(".")
    } else {
        host.to_string()
    }
}

/// Read JSON file, return Value or None
fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Write JSON value to file with pretty formatting
fn write_json_file(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    ensure_parent(path);
    let content = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

// ─── Known ModelInfo fields ───

const KNOWN_MODEL_FIELDS: &[&str] = &[
    "id", "name", "baseUrl", "apiKey", "model", "proxyUrl", "protocol",
];

fn get_model_field(model_info: &ModelInfo, field_name: &str) -> Option<String> {
    match field_name {
        "model" => model_info.model.clone(),
        "name" => model_info.name.clone(),
        "baseUrl" | "base_url" => model_info.base_url.clone(),
        "apiKey" | "api_key" => model_info.api_key.clone(),
        "proxyUrl" | "proxy_url" => model_info.proxy_url.clone(),
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

    // Dispatch custom tools to their own handlers
    match tool_id {
        // Type 2: Echobird relay JSON (write ~/.echobird/{tool}.json)
        "cline" => return apply_echobird_relay(tool_id, &model_info, true),
        "roocode" => return apply_echobird_relay(tool_id, &model_info, false),
        "openclaw" => return apply_echobird_relay(tool_id, &model_info, false),

        // EasyClaw: direct write to ~/.easyclaw/easyclaw.json (Electron GUI, same format as OpenClaw)
        "easyclaw" => return apply_easyclaw(&model_info),

        // CoPaw: direct write to ~/.copaw/copaw.json (same models.providers format as OpenClaw)
        "copaw" => return apply_copaw(&model_info),

        // KiloCode VS Code extension (RooCode fork — same relay + patcher approach)
        "kilocode" => return apply_echobird_relay(tool_id, &model_info, false),

        // Type 3: Direct JSON overwrite (special format)
        "codebuddy" | "codebuddycn" | "workbuddy" => return apply_codebuddy(tool_id, &model_info),
        "opencode" => return apply_opencode(&model_info),

        // Type 4: YAML
        "aider" => return apply_aider(&model_info),
        "continue" => return apply_continue_dev(&model_info),

        // Type 5: TOML
        "codex" => return apply_codex(&model_info),
        "zeroclaw" => return apply_zeroclaw(&model_info),

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

// ════════════════════════════════════════════════════════════════
//  GET MODEL INFO �?main entry point
// ════════════════════════════════════════════════════════════════

pub async fn get_tool_model_info(tool_id: &str) -> Option<ModelInfo> {
    match tool_id {
        "cline" | "roocode" | "openclaw" => return read_echobird_relay(tool_id),
        "easyclaw" => return read_easyclaw(),
        "copaw" => return read_copaw(),
        "kilocode" => return read_echobird_relay(tool_id),
        "codebuddy" | "codebuddycn" | "workbuddy" => return read_codebuddy(tool_id),
        "opencode" => return read_opencode(),
        "aider" => return read_aider(),
        "continue" => return read_continue_dev(),
        "codex" => return read_codex(),
        "zeroclaw" => return read_zeroclaw(),
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
        } else if !KNOWN_MODEL_FIELDS.contains(&model_field.as_str()) {
            tool_manager::set_nested_value(
                &mut config, config_json_path,
                serde_json::Value::String(model_field.clone()),
            );
        }
    }

    // Handle proxy write/delete
    if let Some(proxy_map) = &cm.write_proxy {
        if let Some(ref proxy_url) = model_info.proxy_url {
            for (path, _) in proxy_map {
                tool_manager::set_nested_value(
                    &mut config, path,
                    serde_json::Value::String(proxy_url.clone()),
                );
            }
        } else {
            for (path, _) in proxy_map {
                tool_manager::delete_nested_value(&mut config, path);
            }
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
        proxy_url: read_field(&read_map.proxy_url),
        protocol: None,
    })
}



// ════════════════════════════════════════════════════════════════
//  EasyClaw: direct write to ~/.easyclaw/easyclaw.json
//  Same provider format as OpenClaw (models.providers.eb_xxx)
// ════════════════════════════════════════════════════════════════

fn apply_easyclaw(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".easyclaw").join("easyclaw.json");

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult { success: false, message: "Model ID is empty".to_string() };
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let api_key = model_info.api_key.as_deref().unwrap_or("");
    let protocol = model_info.protocol.as_deref().unwrap_or("openai");
    let api_type = if protocol == "anthropic" { "anthropic-messages" } else { "openai-completions" };

    let provider_tag = format!("eb_{}", extract_domain_name(&base_url));

    let mut config = read_json_file(&config_path).unwrap_or(serde_json::json!({}));

    // Remove previous eb_ providers
    if let Some(providers) = config.pointer_mut("/models/providers") {
        if let Some(obj) = providers.as_object_mut() {
            obj.retain(|k, _| !k.starts_with("eb_"));
            obj.insert(provider_tag.clone(), serde_json::json!({
                "baseUrl": base_url,
                "apiKey": api_key,
                "api": api_type,
                "auth": "api-key",
                "authHeader": true,
                "models": [{
                    "id": model_id,
                    "name": model_info.name.as_deref().unwrap_or(model_id),
                    "api": api_type,
                    "contextWindow": 128000,
                    "maxTokens": 8192
                }]
            }));
        }
    } else {
        // Create the providers section if it doesn't exist
        let providers = serde_json::json!({
            provider_tag.clone(): {
                "baseUrl": base_url,
                "apiKey": api_key,
                "api": api_type,
                "auth": "api-key",
                "authHeader": true,
                "models": [{
                    "id": model_id,
                    "name": model_info.name.as_deref().unwrap_or(model_id),
                    "api": api_type,
                    "contextWindow": 128000,
                    "maxTokens": 8192
                }]
            }
        });
        config["models"]["providers"] = providers;
    }

    // Set primary model
    let primary = format!("{}/{}", provider_tag, model_id);
    config["agents"]["defaults"]["model"]["primary"] = serde_json::Value::String(primary.clone());

    // Replace agents.defaults.models with only our model — picker shows exactly one entry
    // EasyClaw displays alias by taking everything after the first "." ("moonshot.kimi-k2.5" → "kimi-k2.5")
    let display_name = model_info.name.as_deref().unwrap_or(model_id);
    let alias = format!("{}.{}", extract_domain_name(&base_url), display_name);
    config["agents"]["defaults"]["models"] = serde_json::json!({
        primary.clone(): { "alias": alias }
    });
    // Clear fallbacks so only our model is active
    config["agents"]["defaults"]["model"]["fallbacks"] = serde_json::json!([]);

    match write_json_file(&config_path, &config) {
        Ok(_) => {
            log::info!("[ToolConfigManager] EasyClaw config written: {}", primary);
            ApplyResult {
                success: true,
                message: format!(
                    "Model \"{}\" configured for EasyClaw. Restart EasyClaw to apply.",
                    model_info.name.as_deref().unwrap_or(model_id)
                ),
            }
        }
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_easyclaw() -> Option<ModelInfo> {
    let config_path = dirs::home_dir()?.join(".easyclaw").join("easyclaw.json");
    let config = read_json_file(&config_path)?;

    // Find active eb_ provider from primary model
    let primary = config.pointer("/agents/defaults/model/primary")
        .and_then(|v| v.as_str())?;
    let provider_tag = primary.split('/').next()?;
    if !provider_tag.starts_with("eb_") { return None; }
    let model_id = primary.splitn(2, '/').nth(1).unwrap_or("").to_string();

    let provider_path = format!("/models/providers/{}", provider_tag);
    let provider = config.pointer(&provider_path)?;

    Some(ModelInfo {
        name: Some(model_id.clone()),
        model: Some(model_id),
        base_url: provider.get("baseUrl").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: provider.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None, proxy_url: None,
        protocol: provider.get("api").and_then(|v| v.as_str()).map(|api| {
            if api.contains("anthropic") { "anthropic".to_string() } else { "openai".to_string() }
        }),
    })
}

// ════════════════════════════════════════════════════════════════
//  CoPaw: write to ~/.copaw.secret/providers/custom/eb_xxx.json
//  + ~/.copaw.secret/providers/active_model.json
//  Format: CoPaw ProviderInfo (JSON, pydantic model_dump)
// ════════════════════════════════════════════════════════════════

fn copaw_secret_dir() -> std::path::PathBuf {
    // SECRET_DIR = ~/.copaw.secret (WORKING_DIR + ".secret")
    let working_dir = dirs::home_dir().unwrap_or_default().join(".copaw");
    let mut secret = working_dir.into_os_string();
    secret.push(".secret");
    std::path::PathBuf::from(secret)
}

fn apply_copaw(model_info: &ModelInfo) -> ApplyResult {
    let secret_dir = copaw_secret_dir();
    let custom_dir = secret_dir.join("providers").join("custom");
    let active_path = secret_dir.join("providers").join("active_model.json");

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("");
    if model_id.is_empty() {
        return ApplyResult { success: false, message: "Model ID is empty".to_string() };
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let api_key = model_info.api_key.as_deref().unwrap_or("");
    let protocol = model_info.protocol.as_deref().unwrap_or("openai");
    // CoPaw uses class names: "OpenAIChatModel" or "AnthropicChatModel"
    let chat_model = if protocol == "anthropic" { "AnthropicChatModel" } else { "OpenAIChatModel" };

    let provider_id = format!("eb_{}", extract_domain_name(&base_url));
    let provider_name = format!("Echobird ({})", extract_domain_name(&base_url));
    let display_name = model_info.name.as_deref().unwrap_or(model_id);

    // Ensure custom dir exists
    if let Err(e) = fs::create_dir_all(&custom_dir) {
        return ApplyResult { success: false, message: format!("Cannot create CoPaw dir: {}", e) };
    }

    // Remove previous eb_ custom providers
    if let Ok(entries) = fs::read_dir(&custom_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with("eb_") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    // Write new custom provider JSON (CoPaw ProviderInfo format)
    let provider_json = serde_json::json!({
        "id": provider_id,
        "name": provider_name,
        "base_url": base_url,
        "api_key": api_key,
        "chat_model": chat_model,
        "models": [],
        "extra_models": [{"id": model_id, "name": display_name}],
        "api_key_prefix": "",
        "is_local": false,
        "freeze_url": false,
        "require_api_key": true,
        "is_custom": true
    });

    let provider_path = custom_dir.join(format!("{}.json", provider_id));
    if let Err(e) = write_json_file(&provider_path, &provider_json) {
        return ApplyResult { success: false, message: e };
    }

    // Write active_model.json
    let active_json = serde_json::json!({
        "provider_id": provider_id,
        "model": model_id
    });
    if let Err(e) = write_json_file(&active_path, &active_json) {
        return ApplyResult { success: false, message: format!("active_model.json error: {}", e) };
    }

    log::info!("[ToolConfigManager] CoPaw configured: {}/{}", provider_id, model_id);
    ApplyResult {
        success: true,
        message: format!(
            "Model \"{}\" configured for CoPaw. Restart CoPaw to apply.",
            display_name
        ),
    }
}

fn read_copaw() -> Option<ModelInfo> {
    let secret_dir = copaw_secret_dir();
    let active_path = secret_dir.join("providers").join("active_model.json");
    let active = read_json_file(&active_path)?;

    let provider_id = active.get("provider_id")?.as_str()?;
    if !provider_id.starts_with("eb_") { return None; }
    let model_id = active.get("model")?.as_str()?.to_string();

    let provider_path = secret_dir.join("providers").join("custom")
        .join(format!("{}.json", provider_id));
    let provider = read_json_file(&provider_path)?;

    Some(ModelInfo {
        name: Some(model_id.clone()),
        model: Some(model_id),
        base_url: provider.get("base_url").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: provider.get("api_key").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None, proxy_url: None,
        protocol: provider.get("chat_model").and_then(|v| v.as_str()).map(|cm| {
            if cm.contains("Anthropic") { "anthropic".to_string() } else { "openai".to_string() }
        }),
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 2: Echobird relay JSON (Cline, RooCode, OpenClaw)
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
                "cline" => "Cline", "roocode" => "Roo Code", "openclaw" => "OpenClaw",
                "kilocode" => "KiloCode",
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
        anthropic_url: None, proxy_url: None,
        protocol: config.get("protocol").and_then(|v| v.as_str()).map(|s| s.to_string()),
    })
}


// ════════════════════════════════════════════════════════════════
//  Type 3a: CodeBuddy / CodeBuddyCN
//  ~/.codebuddy/models.json  {models: [...], availableModels: [...]}
// ════════════════════════════════════════════════════════════════

fn apply_codebuddy(tool_id: &str, model_info: &ModelInfo) -> ApplyResult {
    // CodeBuddy/CodeBuddyCN use ~/.codebuddy, WorkBuddy uses ~/.workbuddy
    let config_dir = if tool_id == "workbuddy" { ".workbuddy" } else { ".codebuddy" };
    let config_path = dirs::home_dir().unwrap_or_default().join(config_dir).join("models.json");

    // URL must end with /chat/completions for CodeBuddy
    let mut url = model_info.base_url.as_deref().unwrap_or("").to_string();
    if !url.is_empty() && !url.ends_with("/chat/completions") {
        if !url.ends_with('/') { url.push('/'); }
        url.push_str("chat/completions");
    }

    let vendor = model_info.base_url.as_deref()
        .map(|u| extract_domain_name(u)).unwrap_or_else(|| "unknown".to_string());

    let model_id = model_info.model.as_deref()
        .or(model_info.name.as_deref()).unwrap_or("Echobird-model");
    let display_name = model_info.name.as_deref()
        .or(model_info.model.as_deref()).unwrap_or("Echobird Model");

    let config = serde_json::json!({
        "models": [{
            "id": model_id,
            "name": display_name,
            "vendor": vendor,
            "apiKey": model_info.api_key.as_deref().unwrap_or(""),
            "url": url,
            "maxInputTokens": 200000,
            "maxOutputTokens": 8192,
            "supportsToolCall": true,
            "supportsImages": true,
        }],
        "availableModels": [model_id]
    });

    match write_json_file(&config_path, &config) {
        Ok(_) => ApplyResult {
            success: true,
            message: format!("Model \"{}\" applied to CodeBuddy. Config: {}", display_name, config_path.display()),
        },
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_codebuddy(tool_id: &str) -> Option<ModelInfo> {
    let config_dir = if tool_id == "workbuddy" { ".workbuddy" } else { ".codebuddy" };
    let config_path = dirs::home_dir()?.join(config_dir).join("models.json");
    let config = read_json_file(&config_path)?;
    let models = config.get("models")?.as_array()?;
    if models.is_empty() { return None; }
    let m = &models[0];
    let base_url = m.get("url").and_then(|v| v.as_str()).unwrap_or("")
        .trim_end_matches("/chat/completions").trim_end_matches('/').to_string();

    Some(ModelInfo {
        name: m.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        model: m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()),
        base_url: if base_url.is_empty() { None } else { Some(base_url) },
        api_key: m.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None, proxy_url: None, protocol: None,
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

fn read_opencode() -> Option<ModelInfo> {
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
        anthropic_url: None, proxy_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 4a: Aider �?~/.aider.conf.yml (simple YAML key: value)
// ════════════════════════════════════════════════════════════════

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
        api_key, anthropic_url: None, proxy_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 4b: Continue �?~/.continue/config.yaml
//  Simple JSON-like YAML: {models: [{name, provider, model, apiBase, roles, apiKey}]}
//  We read/write as JSON since the structure is JSON-compatible
// ════════════════════════════════════════════════════════════════

fn apply_continue_dev(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".continue").join("config.yaml");

    // Read as JSON (YAML superset of JSON for simple structures)
    let mut config = if config_path.exists() {
        let content = fs::read_to_string(&config_path).unwrap_or_default();
        // Try JSON parse first, then simple YAML
        serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if config.get("name").is_none() { config["name"] = serde_json::json!("Echobird Config"); }
    if config.get("version").is_none() { config["version"] = serde_json::json!("1.0.0"); }
    if config.get("schema").is_none() { config["schema"] = serde_json::json!("v1"); }

    let mut models_vec: Vec<serde_json::Value> = config.get("models")
        .and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let domain_tag = extract_root_domain(&base_url);
    let model_id = model_info.model.as_deref().unwrap_or("");
    let display = format!("{} ({})", model_info.name.as_deref().unwrap_or(model_id), domain_tag);

    let mut new_model = serde_json::json!({
        "name": display, "provider": "openai", "model": model_id,
        "apiBase": base_url, "roles": ["chat", "edit"],
    });
    if let Some(ref k) = model_info.api_key {
        new_model["apiKey"] = serde_json::Value::String(k.clone());
    }

    // Replace existing Echobird model or insert at top
    let idx = models_vec.iter().position(|m|
        m.get("name").and_then(|n| n.as_str())
            .map(|n| n.trim().ends_with(')') && n.contains('('))
            .unwrap_or(false)
    );
    if let Some(i) = idx { models_vec[i] = new_model; } else { models_vec.insert(0, new_model); }

    config["models"] = serde_json::Value::Array(models_vec);

    // Write as pretty JSON (valid YAML)
    match write_json_file(&config_path, &config) {
        Ok(_) => ApplyResult {
            success: true,
            message: format!("Continue updated: model={}, apiBase={}", model_id, base_url),
        },
        Err(e) => ApplyResult { success: false, message: e },
    }
}

fn read_continue_dev() -> Option<ModelInfo> {
    let path = dirs::home_dir()?.join(".continue").join("config.yaml");
    let content = fs::read_to_string(&path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&content).ok()?;
    let models = config.get("models")?.as_array()?;
    if models.is_empty() { return None; }

    let target = models.iter().find(|m|
        m.get("roles").and_then(|r| r.as_array())
            .map(|roles| roles.iter().any(|r| r.as_str() == Some("chat")))
            .unwrap_or(false)
    ).unwrap_or(&models[0]);

    Some(ModelInfo {
        name: target.get("name").and_then(|v| v.as_str()).map(|s| s.to_string()),
        model: target.get("model").and_then(|v| v.as_str()).map(|s| s.to_string()),
        base_url: target.get("apiBase").and_then(|v| v.as_str()).map(|s| s.to_string()),
        api_key: target.get("apiKey").and_then(|v| v.as_str()).map(|s| s.to_string()),
        anthropic_url: None, proxy_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 5a: Codex �?~/.codex/config.toml
//  API Key in ~/.echobird/codex.json
// ════════════════════════════════════════════════════════════════

fn apply_codex(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".codex").join("config.toml");

    // Write config to ~/.echobird/codex.json (used by injected proxy code)
    {
        let codex_json = echobird_dir().join("codex.json");
        let mut cfg = serde_json::json!({});
        if let Some(ref api_key) = model_info.api_key {
            cfg["apiKey"] = serde_json::json!(api_key);
        }
        if let Some(ref base_url) = model_info.base_url {
            cfg["baseUrl"] = serde_json::json!(base_url);
        }
        if let Some(ref model) = model_info.model {
            cfg["modelId"] = serde_json::json!(model);
        } else if let Some(ref name) = model_info.name {
            cfg["modelId"] = serde_json::json!(name);
        }
        if let Some(ref name) = model_info.name {
            cfg["modelName"] = serde_json::json!(name);
        }
        let _ = write_json_file(&codex_json, &cfg);
    }

    let base_url = model_info.base_url.as_deref()
        .unwrap_or("https://api.openai.com/v1").trim_end_matches('/').to_string();
    let is_openai = base_url.contains("api.openai.com");
    let provider = if is_openai { "openai".to_string() } else { extract_domain_name(&base_url) };
    let model_name = model_info.model.as_deref().or(model_info.name.as_deref()).unwrap_or("unknown");

    // Codex v0.107+ only supports wire_api = "responses" (chat is removed).
    // Third-party APIs use the proxy (codex-launcher.cjs) to convert Responses→Chat.
    let wire_api_line = "wire_api = \"responses\"\n";

    let toml_content = format!(
        "model = \"{model}\"\nmodel_provider = \"{prov}\"\nprofile = \"Echobird\"\n\n\
         [model_providers.{prov}]\nname = \"{prov} (via Echobird)\"\n\
         base_url = \"{url}\"\nenv_key = \"OPENAI_API_KEY\"\n\
         {wire}requires_openai_auth = false\n\n\
         [profiles.Echobird]\nmodel = \"{model}\"\nmodel_provider = \"{prov}\"\n",
        model = model_name, prov = provider, url = base_url, wire = wire_api_line,
    );

    ensure_parent(&config_path);
    match fs::write(&config_path, &toml_content) {
        Ok(_) => {
            crate::services::tool_patcher::patch_codex();
            ApplyResult {
                success: true,
                message: format!("Codex updated: provider={}, model={}. Restart to apply.", provider, model_name),
            }
        }
        Err(e) => ApplyResult { success: false, message: format!("Codex error: {}", e) },
    }
}

fn read_codex() -> Option<ModelInfo> {
    let path = dirs::home_dir()?.join(".codex").join("config.toml");
    let content = fs::read_to_string(&path).ok()?;
    let model = toml_read_top(&content, "model");
    if model.is_empty() { return None; }
    let prov = toml_read_top(&content, "model_provider");
    let base_url = if !prov.is_empty() {
        toml_read_section(&content, &format!("model_providers.{}", prov), "base_url")
    } else { None };

    Some(ModelInfo {
        name: Some(model.clone()), model: Some(model), base_url,
        api_key: None, anthropic_url: None, proxy_url: None, protocol: None,
    })
}

// ════════════════════════════════════════════════════════════════
//  Type 5b: ZeroClaw �?~/.zeroclaw/config.toml
// ════════════════════════════════════════════════════════════════

fn apply_zeroclaw(model_info: &ModelInfo) -> ApplyResult {
    let config_path = dirs::home_dir().unwrap_or_default().join(".zeroclaw").join("config.toml");
    let mut content = fs::read_to_string(&config_path).unwrap_or_default();

    if let Some(ref k) = model_info.api_key { content = toml_write_top(&content, "api_key", k); }
    let model = model_info.model.as_deref().or(model_info.name.as_deref()).unwrap_or("");
    if !model.is_empty() { content = toml_write_top(&content, "default_model", model); }
    if let Some(ref u) = model_info.base_url {
        let clean = u.trim_end_matches("/v1/").trim_end_matches("/v1").trim_end_matches('/');
        content = toml_write_top(&content, "default_provider", &format!("custom:{}", clean));
    } else {
        content = toml_write_top(&content, "default_provider", "openrouter");
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
        anthropic_url: None, proxy_url: None, protocol: None,
    })
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

fn toml_read_section(content: &str, section: &str, key: &str) -> Option<String> {
    let header = format!("[{}]", section);
    let mut in_target = false;
    for line in content.lines() {
        let t = line.trim();
        if t == header { in_target = true; continue; }
        if t.starts_with('[') { if in_target { break; } continue; }
        if !in_target || t.starts_with('#') || t.is_empty() { continue; }
        if let Some((k, v)) = t.split_once('=') {
            if k.trim() == key {
                let v = v.trim();
                if v.starts_with('"') && v.ends_with('"') && v.len() >= 2 {
                    return Some(v[1..v.len()-1].to_string());
                }
                return Some(v.to_string());
            }
        }
    }
    None
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
                *line = format!("{} = \"{}\"", key, value);
                found = true;
                break;
            }
        }
    }

    if !found {
        let new_line = format!("{} = \"{}\"", key, value);
        match first_section {
            Some(i) => lines.insert(i, new_line),
            None => lines.push(new_line),
        }
    }
    lines.join("\n")
}
