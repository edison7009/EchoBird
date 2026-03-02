// Native tool patcher — replaces Node.js patch-*.cjs scripts
// Pure Rust file operations: find install dir → backup → inject marker code → write back

use std::fs;
use std::path::{Path, PathBuf};

const ECHOBIRD_BACKUP_EXT: &str = ".echobird-backup";

// ─── Patch Markers ───

const CLINE_MARKER: &str = "/* [Echobird-Patched] */";
const ROOCODE_MARKER: &str = "/* [Echobird-RooCode-Patched] */";
const OPENCLAW_MARKER: &str = "/* [Echobird-Patched] */";
const CODEX_MARKER: &str = "/* [Echobird-Codex-Patched] */";

// ─── Injection Code Strings ───

const CLINE_INJECT: &str = r#"
/* [Echobird-Patched] */
(function(){try{
var _wc_fs=require("fs"),_wc_path=require("path"),_wc_os=require("os");
var _wc_cfg_path=_wc_path.join(_wc_os.homedir(),".echobird","cline.json");
if(_wc_fs.existsSync(_wc_cfg_path)){
var _wc_cfg=JSON.parse(_wc_fs.readFileSync(_wc_cfg_path,"utf-8"));
if(_wc_cfg.apiKey&&_wc_cfg.modelId){
var _inst=t.instance,_gs=_inst.globalStateCache,_sc=_inst.secretsCache;
var _mi={maxTokens:8192,contextWindow:128000,supportsImages:true,supportsPromptCache:false,inputPrice:0,outputPrice:0,description:"[Echobird] "+(_wc_cfg.modelName||_wc_cfg.modelId)};
_gs.actModeApiProvider="openai";
_gs.planModeApiProvider="openai";
_gs.actModeOpenAiModelId=_wc_cfg.modelId;
_gs.planModeOpenAiModelId=_wc_cfg.modelId;
if(_wc_cfg.baseUrl)_gs.openAiBaseUrl=_wc_cfg.baseUrl;
_gs.actModeOpenAiModelInfo=_mi;
_gs.planModeOpenAiModelInfo=_mi;
_sc.openAiApiKey=_wc_cfg.apiKey;
console.log("[Echobird] Loaded: openai-compat, model="+_wc_cfg.modelId);
}}
}catch(_wc_err){console.warn("[Echobird] Failed to load config:",_wc_err.message);}})(),
"#;

const ROOCODE_INJECT: &str = r#"
/* [Echobird-RooCode-Patched] */
(function(){try{
var _wc_fs=require("fs"),_wc_path=require("path"),_wc_os=require("os");
var _wc_cfg_path=_wc_path.join(_wc_os.homedir(),".echobird","roocode.json");
if(_wc_fs.existsSync(_wc_cfg_path)){
var _wc_cfg=JSON.parse(_wc_fs.readFileSync(_wc_cfg_path,"utf-8"));
if(_wc_cfg.apiKey&&_wc_cfg.modelId){
var _gs=this.stateCache,_sc=this.secretCache,_ctx=this.originalContext;
_gs.apiProvider="openai";
_gs.openAiModelId=_wc_cfg.modelId;
if(_wc_cfg.baseUrl)_gs.openAiBaseUrl=_wc_cfg.baseUrl;
_sc.openAiApiKey=_wc_cfg.apiKey;
_ctx.globalState.update("apiProvider","openai");
_ctx.globalState.update("openAiModelId",_wc_cfg.modelId);
if(_wc_cfg.baseUrl)_ctx.globalState.update("openAiBaseUrl",_wc_cfg.baseUrl);
_ctx.secrets.store("openAiApiKey",_wc_cfg.apiKey);
console.log("[Echobird] RooCode loaded: model="+_wc_cfg.modelId);
}}
}catch(_wc_err){console.warn("[Echobird] RooCode config error:",_wc_err.message);}}).call(this),
"#;

// OpenClaw injection: ESM import style
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
      if (key.startsWith("wc_")) {
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

    const wcProviderName = "wc_" + providerTag;
    let baseUrl = wcConfig.baseUrl || "https://api.openai.com/v1";
    if (baseUrl.endsWith("/")) baseUrl = baseUrl.slice(0, -1);

    ocConfig.models.providers[wcProviderName] = {
      baseUrl: baseUrl,
      apiKey: wcConfig.apiKey,
      api: apiType,
      auth: "api-key",
      authHeader: true,
      models: [{
        id: wcConfig.modelId,
        name: wcConfig.modelName || wcConfig.modelId,
        contextWindow: 128000,
        maxTokens: 8192,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }
      }]
    };
    ocConfig.agents.defaults.model.primary = wcProviderName + "/" + wcConfig.modelId;
    console.log("[Echobird] Injected " + apiType + " model: " + wcProviderName + "/" + wcConfig.modelId);

    _wc_writeFileSync(ocConfigPath, JSON.stringify(ocConfig, null, 2), "utf-8");
  } catch (err) {
    console.warn("[Echobird] Config injection failed:", err.message);
  }
})();

"#;

// Codex injection: ESM top-level await style
const CODEX_INJECT: &str = r#"
/* [Echobird-Codex-Patched] */
import { readFileSync as _eb_rf, existsSync as _eb_ex } from "node:fs";
import { join as _eb_j, dirname as _eb_dn } from "node:path";
import { homedir as _eb_h } from "node:os";
import { fileURLToPath as _eb_furl } from "node:url";
await (async function _Echobird_codex() {
  try {
    const p = _eb_j(_eb_h(), ".echobird", "codex.json");
    if (!_eb_ex(p)) return;
    const c = JSON.parse(_eb_rf(p, "utf-8"));
    if (!c.apiKey) return;

    process.env.OPENAI_API_KEY = c.apiKey;
    if (typeof env !== "undefined") env.OPENAI_API_KEY = c.apiKey;

    const baseUrl = c.baseUrl || "";
    const needsProxy = baseUrl && !baseUrl.includes("api.openai.com");

    if (needsProxy) {
      const proxyPath = _eb_j(_eb_dn(_eb_furl(import.meta.url)), "echobird-codex-proxy.mjs");
      if (_eb_ex(proxyPath)) {
        const proxyUrl = "file:///" + proxyPath.split("\\").join("/");
        const { startCodexProxy } = await import(proxyUrl);
        const port = await startCodexProxy(baseUrl, c.apiKey);
        const proxyBaseUrl = "http://127.0.0.1:" + port + "/v1";
        process.env.OPENAI_BASE_URL = proxyBaseUrl;
        if (typeof env !== "undefined") env.OPENAI_BASE_URL = proxyBaseUrl;
        console.log("[Echobird] Proxy active: Codex -> :" + port + " -> " + baseUrl);
      } else {
        process.env.OPENAI_BASE_URL = baseUrl;
        if (typeof env !== "undefined") env.OPENAI_BASE_URL = baseUrl;
        console.log("[Echobird] Proxy module not found, using direct URL: " + baseUrl);
      }
    } else {
      console.log("[Echobird] Using OpenAI directly, no proxy needed");
    }

    console.log("[Echobird] Codex configured: model=" + (c.modelId || "default"));
  } catch (err) {
    console.warn("[Echobird] Config injection failed:", err.message);
  }
})();

"#;

// ─── Installation Directories ───

/// Find VS Code extension directory by prefix
fn find_vscode_extension(prefix: &str) -> Option<PathBuf> {
    let extensions_dir = dirs::home_dir()?.join(".vscode").join("extensions");
    if !extensions_dir.exists() { return None; }

    let mut matches: Vec<String> = fs::read_dir(&extensions_dir).ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.starts_with(prefix))
        .collect();

    matches.sort();
    matches.reverse(); // latest version first

    matches.first().map(|name| extensions_dir.join(name))
}

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
        PathBuf::from(std::env::var("APPDATA").unwrap_or_default()).join("npm").join("node_modules").join(package_name),
        home.join(".npm-global").join("lib").join("node_modules").join(package_name),
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
        Err(e) => { log::warn!("[Patcher] Failed to read {:?}: {}", entry_path, e); return false; }
    };

    // Already patched? Restore from backup first
    if content.contains(config.marker) {
        if backup_path.exists() {
            content = match fs::read_to_string(&backup_path) {
                Ok(c) => c,
                Err(e) => { log::warn!("[Patcher] Failed to read backup: {}", e); return false; }
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

    let patched = format!("{}\n{}{}", &content[..pos], config.inject_code, &content[pos..]);

    if let Err(e) = fs::write(entry_path, &patched) {
        log::warn!("[Patcher] Failed to write patched file: {}", e);
        return false;
    }

    log::info!("[Patcher] Successfully patched: {:?}", entry_path);
    true
}

// ─── Tool-Specific Patchers ───

/// Patch Cline extension
pub fn patch_cline() {
    let ext_dir = match find_vscode_extension("saoudrizwan.claude-dev-") {
        Some(d) => d,
        None => { log::info!("[Patcher] Cline extension not found, skipping"); return; }
    };

    let entry = ext_dir.join("dist").join("extension.js");
    patch_entry_file(&entry, &PatchConfig {
        marker: CLINE_MARKER,
        inject_code: CLINE_INJECT,
        search_patterns: vec![".populateCache(r,n,o),"],
        inject_after: true,
    });
}

/// Patch Roo Code extension
pub fn patch_roocode() {
    let ext_dir = match find_vscode_extension("rooveterinaryinc.roo-cline-") {
        Some(d) => d,
        None => { log::info!("[Patcher] Roo Code extension not found, skipping"); return; }
    };

    let entry = ext_dir.join("dist").join("extension.js");

    // Roo Code needs special handling: find the _isInitialized=!0 near stateCache
    if !entry.exists() {
        log::warn!("[Patcher] Roo Code extension.js not found: {:?}", entry);
        return;
    }

    let backup = PathBuf::from(format!("{}{}", entry.display(), ECHOBIRD_BACKUP_EXT));
    let mut content = match fs::read_to_string(&entry) {
        Ok(c) => c,
        Err(e) => { log::warn!("[Patcher] Failed to read roocode: {}", e); return; }
    };

    if content.contains(ROOCODE_MARKER) {
        if backup.exists() {
            content = match fs::read_to_string(&backup) {
                Ok(c) => c,
                Err(_) => return,
            };
        } else { return; }
    } else {
        let _ = fs::copy(&entry, &backup);
    }

    // Find _isInitialized=!0 near stateCache/secretCache
    let pattern = "this._isInitialized=!0";
    let mut target_idx = None;
    let mut search_from = 0;
    while let Some(idx) = content[search_from..].find(pattern) {
        let abs_idx = search_from + idx;
        let start = if abs_idx > 2000 { abs_idx - 2000 } else { 0 };
        let nearby = &content[start..abs_idx];
        if nearby.contains("stateCache") && nearby.contains("secretCache") {
            target_idx = Some(abs_idx);
            break;
        }
        search_from = abs_idx + pattern.len();
    }

    if let Some(idx) = target_idx {
        let patched = format!("{}{}{}", &content[..idx], ROOCODE_INJECT, &content[idx..]);
        let _ = fs::write(&entry, &patched);
        log::info!("[Patcher] Roo Code patched successfully");
    } else {
        log::warn!("[Patcher] Roo Code injection point not found");
    }
}

/// Patch OpenClaw CLI tool
pub fn patch_openclaw() {
    let install_dir = match find_npm_global_module("openclaw") {
        Some(d) => d,
        None => { log::info!("[Patcher] OpenClaw not found, skipping"); return; }
    };

    let entry = install_dir.join("openclaw.mjs");
    patch_entry_file(&entry, &PatchConfig {
        marker: OPENCLAW_MARKER,
        inject_code: OPENCLAW_INJECT,
        search_patterns: vec![
            "await installProcessWarningFilter();",
            "if (await tryImport(",
        ],
        inject_after: true,
    });
}

/// Patch Codex CLI tool
pub fn patch_codex() {
    let install_dir = match find_npm_global_module("@openai/codex") {
        Some(d) => d,
        None => { log::info!("[Patcher] Codex not found, skipping"); return; }
    };

    let entry = install_dir.join("bin").join("codex.js");

    // Also copy proxy module if source exists
    let tools_dir = crate::services::tool_manager::get_tools_dir();
    let proxy_src = tools_dir.join("codex").join("codex-proxy.mjs");
    let proxy_dst = install_dir.join("bin").join("echobird-codex-proxy.mjs");
    if proxy_src.exists() {
        let _ = fs::copy(&proxy_src, &proxy_dst);
        log::info!("[Patcher] Proxy module copied to {:?}", proxy_dst);
    }

    patch_entry_file(&entry, &PatchConfig {
        marker: CODEX_MARKER,
        inject_code: CODEX_INJECT,
        search_patterns: vec![
            "await installProcessWarningFilter();",
            "if (await tryImport(",
            "const child = spawn(",
        ],
        inject_after: true,
    });
}

/// Dispatch patch by tool ID
pub fn patch_tool(tool_id: &str) {
    match tool_id {
        "cline" => patch_cline(),
        "roocode" => patch_roocode(),
        "openclaw" => patch_openclaw(),
        "codex" => patch_codex(),
        _ => log::debug!("[Patcher] No patch needed for tool: {}", tool_id),
    }
}
