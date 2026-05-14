// Native tool patcher — replaces Node.js patch-*.cjs scripts
// Pure Rust file operations: find install dir → backup → inject marker code → write back

use std::fs;
use std::path::{Path, PathBuf};

const ECHOBIRD_BACKUP_EXT: &str = ".echobird-backup";

// ─── Patch Markers ───

const OPENCLAW_MARKER: &str = "/* [Echobird-Patched] */";

// ─── Injection Code Strings ───

// OpenClaw injection: ESM import style
// Compatible with OpenClaw v2026.3.11+: removed deprecated auth/authHeader fields,
// added required input/reasoning fields to model objects.
const OPENCLAW_INJECT: &str = r#"
/* [Echobird-Patched] */
import { readFileSync as _wc_readFileSync, writeFileSync as _wc_writeFileSync, existsSync as _wc_existsSync, mkdirSync as _wc_mkdirSync } from "node:fs";
import { join as _wc_join } from "node:path";
import { homedir as _wc_homedir } from "node:os";
(function _Echobird_inject() {
  try {
    const wcConfigPath = _wc_join(_wc_homedir(), ".echobird", "openclaw.json");
    if (!_wc_existsSync(wcConfigPath)) return;
    const wcConfig = JSON.parse(_wc_readFileSync(wcConfigPath, "utf-8"));
    if (!wcConfig.modelId || !wcConfig.apiKey) return;

    const ocDir = _wc_join(_wc_homedir(), ".openclaw");
    const ocConfigPath = _wc_join(ocDir, "openclaw.json");
    if (!_wc_existsSync(ocDir)) _wc_mkdirSync(ocDir, { recursive: true });

    let ocConfig = {};
    if (_wc_existsSync(ocConfigPath)) {
      try { ocConfig = JSON.parse(_wc_readFileSync(ocConfigPath, "utf-8")); } catch {}
    }

    if (!ocConfig.models) ocConfig.models = { providers: {} };
    if (!ocConfig.models.providers) ocConfig.models.providers = {};
    if (!ocConfig.agents) ocConfig.agents = {};
    if (!ocConfig.agents.defaults) ocConfig.agents.defaults = {};
    if (!ocConfig.agents.defaults.model) ocConfig.agents.defaults.model = {};

    for (const key of Object.keys(ocConfig.models.providers)) {
      if (key.startsWith("eb_")) {
        delete ocConfig.models.providers[key];
      }
    }

    const protocol = wcConfig.protocol || "openai";
    const isAnthropic = protocol === "anthropic" || wcConfig.modelId?.toLowerCase().includes("claude") || wcConfig.baseUrl?.toLowerCase().includes("anthropic");
    const apiType = isAnthropic ? "anthropic-messages" : "openai-completions";

    let providerTag = "custom";
    try {
      const hostname = new URL(wcConfig.baseUrl || "").hostname;
      if (hostname === "localhost" || hostname.startsWith("127.") || hostname.startsWith("192.168.")) {
        providerTag = "local";
      } else {
        const parts = hostname.split(".");
        providerTag = parts.length >= 2 ? parts[parts.length - 2] : hostname;
      }
    } catch {}

    const ebProviderName = "eb_" + providerTag;
    let baseUrl = wcConfig.baseUrl || "https://api.openai.com/v1";
    if (baseUrl.endsWith("/")) baseUrl = baseUrl.slice(0, -1);

    ocConfig.models.providers[ebProviderName] = {
      baseUrl: baseUrl,
      apiKey: wcConfig.apiKey,
      api: apiType,
      models: [{
        id: wcConfig.modelId,
        name: wcConfig.modelName || wcConfig.modelId,
        contextWindow: 128000,
        maxTokens: 8192,
        input: ["text"],
        reasoning: false,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }
      }]
    };
    ocConfig.agents.defaults.model.primary = ebProviderName + "/" + wcConfig.modelId;
    console.log("[EchoBird] Injected " + apiType + " model: " + ebProviderName + "/" + wcConfig.modelId);

    _wc_writeFileSync(ocConfigPath, JSON.stringify(ocConfig, null, 2), "utf-8");
  } catch (err) {
    console.warn("[EchoBird] Config injection failed:", err.message);
  }
})();

"#;

// ─── Installation Directories ───

/// Find npm global module install directory
fn find_npm_global_module(package_name: &str) -> Option<PathBuf> {
    // Try `npm root -g` first
    if let Ok(output) = std::process::Command::new("npm")
        .args(["root", "-g"])
        .output()
    {
        if output.status.success() {
            let npm_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let candidate = PathBuf::from(&npm_root).join(package_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // Try known paths
    let home = dirs::home_dir()?;
    let candidates = vec![
        // Windows
        PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
            .join("npm")
            .join("node_modules")
            .join(package_name),
        home.join(".npm-global")
            .join("lib")
            .join("node_modules")
            .join(package_name),
        // macOS/Linux
        PathBuf::from("/usr/local/lib/node_modules").join(package_name),
        PathBuf::from("/usr/lib/node_modules").join(package_name),
    ];

    candidates.into_iter().find(|p| p.exists())
}

// ─── Generic Patcher ───

struct PatchConfig {
    marker: &'static str,
    inject_code: &'static str,
    /// Search patterns to find injection point (tried in order)
    search_patterns: Vec<&'static str>,
    /// true = inject AFTER pattern, false = inject BEFORE
    inject_after: bool,
}

/// Generic patch function: find entry file, backup, inject code
fn patch_entry_file(entry_path: &Path, config: &PatchConfig) -> bool {
    if !entry_path.exists() {
        log::warn!("[Patcher] Entry file not found: {:?}", entry_path);
        return false;
    }

    let backup_path = PathBuf::from(format!("{}{}", entry_path.display(), ECHOBIRD_BACKUP_EXT));

    let mut content = match fs::read_to_string(entry_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read {:?}: {}", entry_path, e);
            return false;
        }
    };

    // Already patched? Restore from backup first
    if content.contains(config.marker) {
        if backup_path.exists() {
            content = match fs::read_to_string(&backup_path) {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("[Patcher] Failed to read backup: {}", e);
                    return false;
                }
            };
        } else {
            log::warn!("[Patcher] Already patched but no backup found");
            return false;
        }
    } else {
        // First patch — create backup
        if let Err(e) = fs::copy(entry_path, &backup_path) {
            log::warn!("[Patcher] Failed to create backup: {}", e);
            return false;
        }
    }

    // Find injection point
    let mut inject_pos = None;
    for pattern in &config.search_patterns {
        if let Some(idx) = content.find(pattern) {
            inject_pos = Some(if config.inject_after {
                idx + pattern.len()
            } else {
                idx
            });
            break;
        }
    }

    let pos = match inject_pos {
        Some(p) => p,
        None => {
            log::warn!("[Patcher] No injection point found in {:?}", entry_path);
            return false;
        }
    };

    let patched = format!(
        "{}\n{}{}",
        &content[..pos],
        config.inject_code,
        &content[pos..]
    );

    if let Err(e) = fs::write(entry_path, &patched) {
        log::warn!("[Patcher] Failed to write patched file: {}", e);
        return false;
    }

    log::info!("[Patcher] Successfully patched: {:?}", entry_path);
    true
}

// ─── Tool-Specific Patchers ───

/// Patch OpenClaw CLI tool
pub fn patch_openclaw() {
    let install_dir = match find_npm_global_module("openclaw") {
        Some(d) => d,
        None => {
            log::info!("[Patcher] OpenClaw not found, skipping");
            return;
        }
    };

    let entry = install_dir.join("openclaw.mjs");
    patch_entry_file(
        &entry,
        &PatchConfig {
            marker: OPENCLAW_MARKER,
            inject_code: OPENCLAW_INJECT,
            search_patterns: vec![
                "await installProcessWarningFilter();",
                "if (await tryImport(",
            ],
            inject_after: true,
        },
    );
}

// ─── OpenCode Patcher ───

const OPENCODE_MARKER: &str = "/* [Echobird-OpenCode-Patched] */";

// OpenCode injection: CJS style, injected BEFORE `run()` is called.
// Reads ~/.echobird/opencode.json for API key and base URL,
// sets process.env so the spawned binary inherits them.
// Writes ~/.config/opencode/opencode.json with provider + model config.
const OPENCODE_INJECT: &str = r#"
/* [Echobird-OpenCode-Patched] */
;(function() { try {
  var _eb_p = path.join(os.homedir(), ".echobird", "opencode.json");
  if (fs.existsSync(_eb_p)) {
    var _eb_c = JSON.parse(fs.readFileSync(_eb_p, "utf-8"));
    if (_eb_c.apiKey) process.env.OPENAI_API_KEY = _eb_c.apiKey;
    if (_eb_c.baseUrl) process.env.OPENAI_BASE_URL = _eb_c.baseUrl;
    // Write provider + model config for OpenCode to pick up
    var _eb_cfgDir = path.join(os.homedir(), ".config", "opencode");
    if (!fs.existsSync(_eb_cfgDir)) fs.mkdirSync(_eb_cfgDir, {recursive:true});
    var _eb_cfgPath = path.join(_eb_cfgDir, "opencode.json");
    var _eb_provId = "echobird";
    var _eb_cfg = {"$schema":"https://opencode.ai/config.json","provider":{}};
    try { if (fs.existsSync(_eb_cfgPath)) _eb_cfg = JSON.parse(fs.readFileSync(_eb_cfgPath,"utf-8")); } catch {}
    if (!_eb_cfg.provider) _eb_cfg.provider = {};
    _eb_cfg.provider[_eb_provId] = {
      npm: "@ai-sdk/openai-compatible",
      name: _eb_c.providerName || "Echobird",
      options: { baseURL: _eb_c.baseUrl || "", apiKey: _eb_c.apiKey || "" },
      models: {}
    };
    if (_eb_c.modelId) {
      _eb_cfg.provider[_eb_provId].models[_eb_c.modelId] = {name: _eb_c.modelName || _eb_c.modelId};
      // Set model at top level — this is how OpenCode selects the default model
      _eb_cfg.model = _eb_provId + "/" + _eb_c.modelId;
      _eb_cfg.small_model = _eb_provId + "/" + _eb_c.modelId;
    }
    fs.writeFileSync(_eb_cfgPath, JSON.stringify(_eb_cfg, null, 2), "utf-8");
    // Also set OPENCODE_CONFIG_CONTENT for runtime override (highest priority)
    if (_eb_c.modelId) {
      process.env.OPENCODE_CONFIG_CONTENT = JSON.stringify({model: _eb_provId + "/" + _eb_c.modelId, small_model: _eb_provId + "/" + _eb_c.modelId});
    }
    console.log("[EchoBird] OpenCode configured: model=" + _eb_provId + "/" + (_eb_c.modelId || "default"));
  }
} catch(_e) { console.warn("[EchoBird] OpenCode inject error:", _e.message); } })();
"#;

/// Patch OpenCode CLI tool
pub fn patch_opencode() {
    let install_dir = match find_npm_global_module("opencode-ai") {
        Some(d) => d,
        None => {
            log::info!("[Patcher] OpenCode not found, skipping");
            return;
        }
    };

    let entry = install_dir.join("bin").join("opencode");

    patch_entry_file(
        &entry,
        &PatchConfig {
            marker: OPENCODE_MARKER,
            inject_code: OPENCODE_INJECT,
            search_patterns: vec![
                "const envPath = process.env.OPENCODE_BIN_PATH",
                "function run(target) {",
            ],
            inject_after: false, // inject BEFORE these patterns
        },
    );
}

// ─── ZeroClaw Patcher ───

/// Patch ZeroClaw by writing its native TOML config file.
/// ZeroClaw is a Rust binary (not Node.js), so we write ~/.zeroclaw/config.toml
/// directly from ~/.echobird/zeroclaw.json instead of injecting code.
pub fn patch_zeroclaw() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("[Patcher] Cannot determine home dir for ZeroClaw");
            return;
        }
    };

    let echobird_cfg = home.join(".echobird").join("zeroclaw.json");
    if !echobird_cfg.exists() {
        log::info!(
            "[Patcher] ZeroClaw config not found at {:?}, skipping",
            echobird_cfg
        );
        return;
    }

    let content = match fs::read_to_string(&echobird_cfg) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read zeroclaw.json: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Patcher] Invalid zeroclaw.json: {}", e);
            return;
        }
    };

    let api_key = json.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = json.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("");

    if api_key.is_empty() || model_id.is_empty() {
        log::info!("[Patcher] ZeroClaw config incomplete (missing apiKey/modelId), skipping");
        return;
    }

    // Build provider string: custom:baseUrl (ZeroClaw format)
    let mut provider_url = base_url.to_string();
    if !provider_url.is_empty() && !provider_url.ends_with("/v1") {
        if provider_url.ends_with('/') {
            provider_url.push_str("v1");
        } else {
            provider_url.push_str("/v1");
        }
    }
    let provider = if provider_url.is_empty() {
        "openai".to_string()
    } else {
        format!("custom:{}", provider_url)
    };

    // Write ~/.zeroclaw/config.toml
    let zeroclaw_dir = home.join(".zeroclaw");
    if !zeroclaw_dir.exists() {
        let _ = fs::create_dir_all(&zeroclaw_dir);
    }

    let toml_content = format!(
        "api_key = \"{}\"\ndefault_model = \"{}\"\ndefault_provider = \"{}\"\ndefault_temperature = 0.7\n",
        api_key.replace('"', "\\\""),
        model_id.replace('"', "\\\""),
        provider.replace('"', "\\\""),
    );

    let config_path = zeroclaw_dir.join("config.toml");
    match fs::write(&config_path, &toml_content) {
        Ok(_) => log::info!(
            "[Patcher] ZeroClaw config written: {:?} (model={})",
            config_path,
            model_id
        ),
        Err(e) => log::warn!("[Patcher] Failed to write ZeroClaw config: {}", e),
    }
}

// ─── NanoBot Patcher ───

/// Patch NanoBot by writing its JSON config file.
/// NanoBot config: ~/.nanobot/config.json
/// Format: { "providers": { "custom": { "apiKey": "...", "apiBase": "..." } }, "agents": { "defaults": { "model": "..." } } }
pub fn patch_nanobot() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("[Patcher] Cannot determine home dir for NanoBot");
            return;
        }
    };

    let echobird_cfg = home.join(".echobird").join("nanobot.json");
    if !echobird_cfg.exists() {
        log::info!(
            "[Patcher] NanoBot config not found at {:?}, skipping",
            echobird_cfg
        );
        return;
    }

    let content = match fs::read_to_string(&echobird_cfg) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read nanobot.json: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Patcher] Invalid nanobot.json: {}", e);
            return;
        }
    };

    let api_key = json.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = json.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("");

    if api_key.is_empty() || model_id.is_empty() {
        log::info!("[Patcher] NanoBot config incomplete (missing apiKey/modelId), skipping");
        return;
    }

    // Ensure /v1 suffix for OpenAI-compatible APIs
    let mut api_base = base_url.to_string();
    if !api_base.is_empty() && !api_base.ends_with("/v1") {
        if api_base.ends_with('/') {
            api_base.push_str("v1");
        } else {
            api_base.push_str("/v1");
        }
    }

    // Build NanoBot config JSON
    let config = serde_json::json!({
        "providers": {
            "custom": {
                "apiKey": api_key,
                "apiBase": api_base
            }
        },
        "agents": {
            "defaults": {
                "model": model_id
            }
        }
    });

    let nanobot_dir = home.join(".nanobot");
    if !nanobot_dir.exists() {
        let _ = fs::create_dir_all(&nanobot_dir);
    }

    // Merge with existing config (preserve other settings)
    let config_path = nanobot_dir.join("config.json");
    let mut existing: serde_json::Value = if config_path.exists() {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Deep merge: providers.custom and agents.defaults
    if let Some(obj) = existing.as_object_mut() {
        if let Some(providers) = config.get("providers") {
            obj.insert("providers".to_string(), providers.clone());
        }
        if let Some(agents) = config.get("agents") {
            obj.insert("agents".to_string(), agents.clone());
        }
    }

    match fs::write(
        &config_path,
        serde_json::to_string_pretty(&existing).unwrap_or_default(),
    ) {
        Ok(_) => log::info!(
            "[Patcher] NanoBot config written: {:?} (model={})",
            config_path,
            model_id
        ),
        Err(e) => log::warn!("[Patcher] Failed to write NanoBot config: {}", e),
    }
}

// ─── PicoClaw Patcher ───

/// Patch PicoClaw by writing its JSON config file.
/// PicoClaw config: ~/.picoclaw/config.json (same format as NanoBot — Go rewrite)
pub fn patch_picoclaw() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("[Patcher] Cannot determine home dir for PicoClaw");
            return;
        }
    };

    let echobird_cfg = home.join(".echobird").join("picoclaw.json");
    if !echobird_cfg.exists() {
        log::info!(
            "[Patcher] PicoClaw config not found at {:?}, skipping",
            echobird_cfg
        );
        return;
    }

    let content = match fs::read_to_string(&echobird_cfg) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read picoclaw.json: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Patcher] Invalid picoclaw.json: {}", e);
            return;
        }
    };

    let api_key = json.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = json.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("");

    if api_key.is_empty() || model_id.is_empty() {
        log::info!("[Patcher] PicoClaw config incomplete (missing apiKey/modelId), skipping");
        return;
    }

    // PicoClaw uses model_list array format (not providers object)
    // model field uses "vendor/model" format, agents.defaults.model uses bare model_name
    // Detect vendor from baseUrl or default to "openai"
    let vendor = if base_url.contains("minimaxi.com") || base_url.contains("minimax.") {
        "minimax"
    } else if base_url.contains("anthropic.com") {
        "anthropic"
    } else if base_url.contains("deepseek.com") {
        "deepseek"
    } else if base_url.contains("openrouter.ai") {
        "openrouter"
    } else if base_url.contains("groq.com") {
        "groq"
    } else {
        "openai"
    };

    let mut model_entry = serde_json::json!({
        "model_name": model_id,
        "model": format!("{}/{}", vendor, model_id),
        "api_key": api_key
    });

    // Add custom api_base if not using default vendor endpoint
    if !base_url.is_empty() {
        let mut api_base = base_url.to_string();
        if !api_base.ends_with("/v1") {
            if api_base.ends_with('/') {
                api_base.push_str("v1");
            } else {
                api_base.push_str("/v1");
            }
        }
        model_entry["api_base"] = serde_json::Value::String(api_base);
    }

    let picoclaw_dir = home.join(".picoclaw");
    if !picoclaw_dir.exists() {
        let _ = fs::create_dir_all(&picoclaw_dir);
    }

    // Merge: read existing config, update model_list entry and agents.defaults.model
    let config_path = picoclaw_dir.join("config.json");
    let mut existing: serde_json::Value = if config_path.exists() {
        fs::read_to_string(&config_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_else(|| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Always one entry — never accumulate. Each apply_picoclaw replaces
    // the entire model_list with a single fresh entry. The previous design
    // deduped by model_name and pushed, which left stale entries with
    // different model_names in the array across model switches. EchoBird
    // is the single source of truth for which model the user is running,
    // so a one-element list is exactly what we want.
    existing["model_list"] = serde_json::json!([model_entry]);

    // Set default model
    existing["agents"]["defaults"]["model"] = serde_json::Value::String(model_id.to_string());

    // Remove deprecated providers key if present
    if let Some(obj) = existing.as_object_mut() {
        obj.remove("providers");
    }

    match fs::write(
        &config_path,
        serde_json::to_string_pretty(&existing).unwrap_or_default(),
    ) {
        Ok(_) => log::info!(
            "[Patcher] PicoClaw config written: {:?} (model={})",
            config_path,
            model_id
        ),
        Err(e) => log::warn!("[Patcher] Failed to write PicoClaw config: {}", e),
    }
}

// ─── OpenFang Patcher ───

/// Patch OpenFang by writing its TOML config file.
/// OpenFang config: ~/.openfang/config.toml
/// Format: [default_model] section with provider, model, base_url, api_key_env
/// OpenFang uses environment variable references for API keys (api_key_env),
/// so we also set the env var OPENFANG_LLM_KEY.
pub fn patch_openfang() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("[Patcher] Cannot determine home dir for OpenFang");
            return;
        }
    };

    let echobird_cfg = home.join(".echobird").join("openfang.json");
    if !echobird_cfg.exists() {
        log::info!(
            "[Patcher] OpenFang config not found at {:?}, skipping",
            echobird_cfg
        );
        return;
    }

    let content = match fs::read_to_string(&echobird_cfg) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read openfang.json: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Patcher] Invalid openfang.json: {}", e);
            return;
        }
    };

    let api_key = json.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = json.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("");

    if api_key.is_empty() || model_id.is_empty() {
        log::info!("[Patcher] OpenFang config incomplete (missing apiKey/modelId), skipping");
        return;
    }

    // Build provider URL with /v1 suffix
    let mut provider_url = base_url.to_string();
    if !provider_url.is_empty() && !provider_url.ends_with("/v1") {
        if provider_url.ends_with('/') {
            provider_url.push_str("v1");
        } else {
            provider_url.push_str("/v1");
        }
    }

    // Set the API key as an environment variable so OpenFang can read it
    std::env::set_var("OPENFANG_LLM_KEY", api_key);

    // Write TOML config using OpenFang's [default_model] format
    let openfang_dir = home.join(".openfang");
    if !openfang_dir.exists() {
        let _ = fs::create_dir_all(&openfang_dir);
    }

    // Read existing config to preserve other settings (api_listen, memory, etc.)
    let config_path = openfang_dir.join("config.toml");
    let existing = fs::read_to_string(&config_path).unwrap_or_default();

    // Build new config: keep lines before [default_model], replace that section
    let mut new_config = String::new();
    let mut skip_section = false;
    for line in existing.lines() {
        if line.starts_with("[default_model]") || line.starts_with("[llm]") {
            skip_section = true;
            continue;
        }
        if skip_section && (line.starts_with('[') || line.is_empty()) {
            if line.starts_with('[') {
                skip_section = false;
                new_config.push_str(line);
                new_config.push('\n');
            }
            continue;
        }
        if skip_section {
            continue;
        }
        new_config.push_str(line);
        new_config.push('\n');
    }

    // Append [default_model] section
    new_config.push_str(&format!(
        "\n[default_model]\nprovider = \"custom\"\nmodel = \"{}\"\nbase_url = \"{}\"\napi_key_env = \"OPENFANG_LLM_KEY\"\n",
        model_id.replace('"', "\\\""),
        provider_url.replace('"', "\\\""),
    ));

    match fs::write(&config_path, &new_config) {
        Ok(_) => log::info!(
            "[Patcher] OpenFang config written: {:?} (model={})",
            config_path,
            model_id
        ),
        Err(e) => log::warn!("[Patcher] Failed to write OpenFang config: {}", e),
    }
}

// ── Hermes Agent Patcher ───

/// Patch Hermes Agent config files for custom endpoint.
///
/// Hermes config rules (confirmed from cli.py source, 2026-03-23):
///   - Model comes from: CLI arg or config.yaml `model.default` (single source of truth)
///   - LLM_MODEL/OPENAI_MODEL env vars are NOT checked by Hermes
///   - API keys (OPENAI_API_KEY) and base URLs (OPENAI_BASE_URL) read from .env only
///   - Default fallback model: anthropic/claude-opus-4.6
///
/// Correct approach:
///   1. Write `model:\n  default: {model_id}` to config.yaml
///   2. Write OPENAI_API_KEY + OPENAI_BASE_URL to .env
///   3. Clear ANTHROPIC_API_KEY from .env (prevents auto-detection override)
///   4. Clean stale entries (LLM_MODEL, flat OPENAI_BASE_URL:) from config.yaml
pub fn patch_hermes() {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log::warn!("[Patcher] Cannot determine home dir for Hermes");
            return;
        }
    };

    let echobird_cfg = home.join(".echobird").join("hermes.json");
    if !echobird_cfg.exists() {
        log::info!(
            "[Patcher] Hermes config not found at {:?}, skipping",
            echobird_cfg
        );
        return;
    }

    let content = match fs::read_to_string(&echobird_cfg) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[Patcher] Failed to read hermes.json: {}", e);
            return;
        }
    };

    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Patcher] Invalid hermes.json: {}", e);
            return;
        }
    };

    let api_key = json.get("apiKey").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = json.get("baseUrl").and_then(|v| v.as_str()).unwrap_or("");
    let model_id = json.get("modelId").and_then(|v| v.as_str()).unwrap_or("");

    if api_key.is_empty() || model_id.is_empty() {
        log::info!("[Patcher] Hermes config incomplete (missing apiKey/modelId), skipping");
        return;
    }

    // Ensure base URL has /v1 suffix (Hermes expects full endpoint)
    let base_url_full = {
        let raw = base_url.trim_end_matches('/');
        if raw.is_empty() {
            String::new()
        } else if raw.ends_with("/v1") {
            raw.to_string()
        } else {
            format!("{}/v1", raw)
        }
    };

    let hermes_dir = home.join(".hermes");
    if !hermes_dir.exists() {
        let _ = fs::create_dir_all(&hermes_dir);
    }

    // ── Step 1: Update .env ──
    // Write OPENAI_API_KEY + OPENAI_BASE_URL, clear ANTHROPIC_API_KEY
    // Remove stale LLM_MODEL (Hermes does NOT read this from .env)
    let env_path = hermes_dir.join(".env");
    let mut env_lines: Vec<String> = Vec::new();
    if env_path.exists() {
        if let Ok(existing) = fs::read_to_string(&env_path) {
            for line in existing.lines() {
                let trimmed = line.trim();
                // Remove entries we'll re-add or clear
                if trimmed.starts_with("OPENAI_API_KEY=")
                    || trimmed.starts_with("OPENAI_BASE_URL=")
                    || trimmed.starts_with("LLM_MODEL=")
                {
                    continue;
                }
                // Clear ANTHROPIC_API_KEY (prevents Hermes auto-detecting Anthropic provider)
                if trimmed.starts_with("ANTHROPIC_API_KEY=") {
                    env_lines.push("ANTHROPIC_API_KEY=".to_string());
                    continue;
                }
                env_lines.push(line.to_string());
            }
        }
    }
    env_lines.push(format!("OPENAI_API_KEY={}", api_key));
    if !base_url_full.is_empty() {
        env_lines.push(format!("OPENAI_BASE_URL={}", base_url_full));
    }

    if let Err(e) = fs::write(&env_path, env_lines.join("\n") + "\n") {
        log::warn!("[Patcher] Failed to write Hermes .env: {}", e);
        return;
    }

    // ── Step 2: Update config.yaml ──
    // Write `model:\n  default: {model_id}` (the ONLY key Hermes reads for model)
    // Clean stale entries: LLM_MODEL:, flat OPENAI_BASE_URL:, flat model: (non-nested)
    let config_path = hermes_dir.join("config.yaml");
    let mut yaml_lines: Vec<String> = Vec::new();
    let mut skip_model_block = false;
    if config_path.exists() {
        if let Ok(existing) = fs::read_to_string(&config_path) {
            for line in existing.lines() {
                let trimmed = line.trim();
                // Skip existing model: block (we'll re-add it)
                if trimmed == "model:" || trimmed.starts_with("model:") {
                    // Check if it's the nested model: block (next line would be indented)
                    skip_model_block = trimmed == "model:";
                    if !skip_model_block {
                        // Flat `model: xxx` — skip it
                        continue;
                    }
                    continue; // skip `model:` header
                }
                if skip_model_block {
                    if line.starts_with(' ') || line.starts_with('\t') {
                        continue; // skip indented lines under model:
                    }
                    skip_model_block = false; // end of model block
                }
                // Remove stale entries from previous Bridge versions
                if trimmed.starts_with("LLM_MODEL:") || trimmed.starts_with("OPENAI_BASE_URL:") {
                    continue;
                }
                yaml_lines.push(line.to_string());
            }
        }
    }
    // Add correct model block
    yaml_lines.push("model:".to_string());
    yaml_lines.push(format!("  default: {}", model_id));

    match fs::write(&config_path, yaml_lines.join("\n") + "\n") {
        Ok(_) => log::info!(
            "[Patcher] Hermes config written: model.default={}, base_url={}",
            model_id,
            base_url_full
        ),
        Err(e) => log::warn!("[Patcher] Failed to write Hermes config.yaml: {}", e),
    }
}

/// Dispatch patch by tool ID
pub fn patch_tool(tool_id: &str) {
    match tool_id {
        "openclaw" => patch_openclaw(),
        "opencode" => patch_opencode(),
        "zeroclaw" => patch_zeroclaw(),
        "nanobot" => patch_nanobot(),
        "picoclaw" => patch_picoclaw(),
        "openfang" => patch_openfang(),
        "hermes" => patch_hermes(),
        _ => log::debug!("[Patcher] No patch needed for tool: {}", tool_id),
    }
}
