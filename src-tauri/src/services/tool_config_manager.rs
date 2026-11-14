// Model configuration entry points and shared file-format helpers.
// Tool-specific implementations live in tool_config_manager/<tool>.rs.

mod aider;
mod claudecode;
mod claudedesktop;
mod claudescience;

pub(crate) fn claude_science_restart_file() -> std::path::PathBuf {
    echobird_dir().join(claudescience::RESTART_FILE)
}
pub(crate) mod cline;
mod codearts;
mod codex;
mod copilot;
pub(crate) mod dsh;
mod generic;
mod grok;
pub(crate) mod hermes;
mod jsonc;
mod kilo;
pub(crate) mod kimicode;
mod mimo_capabilities;
mod mimocode;
mod mimodesktop;
pub(crate) mod minimaxcode;
pub(crate) mod musecode;
mod omp;
mod openclaw;
mod opencode;
mod opendesign;
mod openscience;
mod pi;
mod provider_selection;
mod proxy_ports;
mod qoder;
mod qwencode;
mod relay;
mod switching;
mod toml;
mod vibe_trading;
pub(crate) mod vscode;
mod workbuddy;
mod zcode;

use crate::services::{codex_accounts, copilot_accounts, tool_manager};
use aider::{apply_aider, read_aider};
use claudecode::{
    apply_claudecode, normalize_model_info_for_tool, read_claudecode,
    restore_claudecode_to_official,
};
use claudedesktop::{apply_claudedesktop, read_claudedesktop, restore_claudedesktop_to_official};
pub(crate) use codex::{apply_codex, apply_codex_at};
use codex::{read_codex, restore_codex_to_official};
use copilot::{
    apply_copilot, apply_copilot_desktop, read_copilot, read_copilot_desktop,
    restore_copilot_desktop_to_official, restore_copilot_to_official,
};
use dsh::{apply_dsh, read_dsh, restore_dsh_to_official};
use generic::{apply_generic_json, read_generic_json};
use grok::{apply_grok, read_grok, restore_grok_to_official};
pub(crate) use kilo::kilo_apply_account_selection;
pub use kilo::kilo_echobird_model;
use kilo::{
    apply_kilo, apply_kilo_desktop, read_kilo, read_kilo_desktop, restore_kilo_desktop_to_official,
    restore_kilo_to_official,
};
use kimicode::{
    apply_kimicode, apply_kimidesktop, read_kimicode, read_kimidesktop,
    restore_kimicode_to_official, restore_kimidesktop_to_official,
};
pub use mimocode::mimocode_echobird_model;
use mimocode::{apply_mimocode, read_mimocode, restore_mimocode_to_official};
use openclaw::{apply_openclaw, read_openclaw};
use opencode::{apply_opencode, read_opencode, restore_opencode_to_official};
pub(crate) use opendesign::restore_for_account as restore_opendesign_account_mode;
use openscience::{apply_openscience, read_openscience, restore_openscience_to_official};
use pi::{apply_pi, read_pi, restore_pi_to_official};
pub(crate) use proxy_ports::migrate_local_proxy_port;
use qwencode::{apply_qwen_code, read_qwen_code};
use relay::{apply_echobird_relay, read_echobird_relay};
use std::fs;
use std::path::{Path, PathBuf};
use vibe_trading::{apply_vibe_trading, read_vibe_trading};
use workbuddy::{apply_workbuddy, read_workbuddy};
use zcode::{apply_zcode, read_zcode, restore_zcode_to_official};

pub(crate) fn restore_zcode_account_config(dir: &std::path::Path) -> ApplyResult {
    zcode::restore_zcode_to_official_at(dir)
}

pub(crate) fn restore_copilot_account_config() -> ApplyResult {
    restore_copilot_to_official()
}

pub(crate) async fn restore_copilot_desktop_account_config_with_bridge(
    bridge: &mut copilot_accounts::desktop::Desktop,
) -> ApplyResult {
    copilot::restore_copilot_desktop_with_bridge(bridge).await
}

pub(crate) fn copilot_config_dir() -> PathBuf {
    copilot::config_dir()
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_model: Option<String>,
    /// Accepted for compatibility with older callers; Claude apps always connect directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_mode: Option<bool>,
    /// Codex / ChatGPT / Claude / ZCode. None preserves the existing search policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_search: Option<bool>,
    /// Image input capability override. Clients without this setting ignore it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_input: Option<bool>,
    /// Claude Code: when `Some(true)`,
    /// append `[1m]` to the 1M-capable env vars (`ANTHROPIC_MODEL` /
    /// `ANTHROPIC_DEFAULT_SONNET_MODEL` / `ANTHROPIC_DEFAULT_OPUS_MODEL` / `ANTHROPIC_DEFAULT_FABLE_MODEL`)
    /// written to ~/.claude/settings.json so Claude Code budgets the 1M
    /// context window. Claude Code strips the suffix before sending the id
    /// upstream, so the provider still sees the bare id. `HAIKU` and
    /// `CLAUDE_CODE_SUBAGENT_MODEL` never get the suffix — no 1M concept.
    /// Claude Desktop uses its profile prefer1m setting. MiMo, MiniMax, Qoder, DSH and WorkBuddy use native context limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub one_m_context: Option<bool>,
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

/// Extract domain name from URL for use in identifiers
/// Example: "https://api.openai.com/v1" -> "api_openai_com"
fn extract_domain_name(url: &str) -> String {
    url.trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(url)
        .split(':')
        .next()
        .unwrap_or(url)
        .replace('.', "_")
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

/// `serde_yaml_ng` shorthand: a string value.
fn yaml_str(s: &str) -> serde_yaml_ng::Value {
    serde_yaml_ng::Value::String(s.to_string())
}

/// `serde_yaml_ng` shorthand: an empty mapping value.
fn yaml_map() -> serde_yaml_ng::Value {
    serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::new())
}

/// Coerce a YAML value into a mapping, returning `&mut Mapping`. Replaces a
/// non-mapping value with a fresh mapping — the YAML analogue of the jsonc
/// guards used by the openscience/zcode configs.
fn yaml_as_map_mut(value: &mut serde_yaml_ng::Value) -> &mut serde_yaml_ng::Mapping {
    if !value.is_mapping() {
        *value = yaml_map();
    }
    value.as_mapping_mut().expect("coerced to mapping")
}

/// Get-or-create a child mapping under `key` in `map`, returning `&mut Mapping`.
fn yaml_child_map<'a>(
    map: &'a mut serde_yaml_ng::Mapping,
    key: &str,
) -> &'a mut serde_yaml_ng::Mapping {
    let child = map.entry(yaml_str(key)).or_insert_with(yaml_map);
    yaml_as_map_mut(child)
}

/// Read a child value from a mapping by string key (None if absent/non-mapping).
fn yaml_get<'a>(value: &'a serde_yaml_ng::Value, key: &str) -> Option<&'a serde_yaml_ng::Value> {
    value.as_mapping()?.get(yaml_str(key))
}

/// Write a `serde_yaml_ng::Value` back to a YAML file (block style), creating
/// parent directories as needed. Mirrors `write_json_file`.
fn write_yaml_file(path: &Path, value: &serde_yaml_ng::Value) -> Result<(), String> {
    ensure_parent(path);
    let content = serde_yaml_ng::to_string(value).map_err(|e| e.to_string())?;
    fs::write(path, content).map_err(|e| format!("Failed to write {}: {}", path.display(), e))
}

/// Look up a model's real input modalities. Returns `["text"]` for models the
/// registry does not list so ZCode's `modalities.input` keeps the safe
/// text-only default for unknown ids.
fn model_input_modalities_for(model_id: &str) -> &'static [&'static str] {
    match model_id {
        "MiniMax-M3" => &["text", "image", "video"],
        _ => &["text"],
    }
}

// ════════════════════════════════════════════════════════════════
//  APPLY MODEL �?main entry point
// ════════════════════════════════════════════════════════════════

/// Plugin applications own their native model configuration.
async fn plugin_model_operation(
    tool_id: &str,
    operation: &str,
    input: serde_json::Value,
) -> ApplyResult {
    match crate::services::plugin_accounts::call(tool_id, operation, input).await {
        Ok(_) => ApplyResult {
            success: true,
            message: String::new(),
        },
        Err(message) => ApplyResult {
            success: false,
            message,
        },
    }
}

pub async fn apply_model_to_tool(tool_id: &str, model_info: ModelInfo) -> ApplyResult {
    log::info!("[ToolConfigManager] Applying model to {}", tool_id);
    let model_info = normalize_model_info_for_tool(tool_id, model_info);
    if let Err(message) = switching::models(&model_info) {
        return ApplyResult {
            success: false,
            message,
        };
    }
    if crate::services::plugin_accounts::provides_tool(tool_id) {
        let input = serde_json::to_value(&model_info).unwrap_or_default();
        return plugin_model_operation(tool_id, "model.apply", input).await;
    }

    // Dispatch custom tools to their own handlers
    match tool_id {
        "codearts" => return codearts::configure(tool_id, Some(&model_info)).await,
        "vscode" => return vscode::apply(&model_info),
        // OpenClaw: direct write to ~/.openclaw/openclaw.json (no patch needed since v2026.3.13)
        "openclaw" => return apply_openclaw(&model_info),

        // Type 3: Direct JSON overwrite (special format).
        // CLI and Desktop share ~/.config/opencode/opencode.jsonc — one apply
        // covers both (Desktop spawns `opencode serve` reading the same file).
        "opencode" | "opencodedesktop" => return apply_opencode(&model_info),

        // MiMo Code (Xiaomi fork of OpenCode): same provider schema,
        // own config at ~/.config/mimocode/mimocode.json(c).
        "mimocode" => return apply_mimocode(&model_info),
        "mimodesktop" => return mimodesktop::apply(&model_info),
        "minimaxcode" | "minimaxdesktop" => return minimaxcode::apply(&model_info),
        "musecode" => return musecode::apply(&model_info),
        "cline" | "clinedesktop" => {
            return cline::apply(&model_info, tool_id == "clinedesktop").await
        }
        "qoder" | "qodercn" => return qoder::apply(tool_id, &model_info).await,
        "kimidesktop" => return apply_kimidesktop(&model_info),

        // Kilo Code (Kilo fork of OpenCode): same provider schema,
        // own config at ~/.config/kilo/kilo.json.
        "kilo" => return apply_kilo(&model_info),
        "kilodesktop" => return apply_kilo_desktop(&model_info),
        "copilot" => return apply_copilot(&model_info),
        "copilotdesktop" => return apply_copilot_desktop(&model_info).await,

        // OpenScience (open-source Claude Science alt): models.dev provider
        // schema, dual-protocol (npm @ai-sdk/anthropic | @ai-sdk/openai-compatible),
        // config at ~/.config/openscience/openscience.json.
        "openscience" => return apply_openscience(&model_info),
        "dsh" => return apply_dsh(&model_info),
        "hermes" => return hermes::apply(&model_info),

        // ZCode keeps model configuration in its selected data directory.
        "zcode" => return apply_zcode(&model_info),
        "opendesign" => return opendesign::apply(&model_info).await,

        // Codex CLI and ChatGPT desktop share ~/.codex/config.toml.
        "codex" | "chatgptdesktop" => return apply_codex(tool_id, &model_info),

        // Claude Desktop 3P profile (Anthropic-native providers only)
        "claudedesktop" => {
            let mut result = apply_claudedesktop(&model_info);
            if result.success {
                if let Err(message) =
                    super::plugin_accounts::prepare_history("claudedesktop", true).await
                {
                    result.success = false;
                    result.message = message;
                }
            }
            return result;
        }
        "claudescience" => return claudescience::apply(&model_info),

        // Claude Code — same model-id-rewrite proxy path as Claude Desktop,
        // but writes ~/.claude/settings.json env vars + its own relay file.
        "claudecode" => return apply_claudecode(&model_info),

        // Type 4: YAML
        "aider" => return apply_aider(&model_info),

        // Grok Build CLI (xAI) — sectioned TOML with [model.echobird] + [models]
        "grok" => return apply_grok(&model_info),

        // Qwen Code: direct write to ~/.qwen/settings.json
        "qwencode" => return apply_qwen_code(&model_info),

        // Pi (earendil-works/pi): writes ~/.pi/agent/{models,settings}.json
        "pi" => return apply_pi(&model_info),
        "omp" => return omp::apply(&model_info),

        // Kimi CLI (Moonshot AI): TOML shared with Desktop at ~/.kimi-code/config.toml
        "kimicode" => return apply_kimicode(&model_info),

        // Vibe-Trading (HKUDS): dotenv at ~/.vibe-trading/.env. Every endpoint
        // we point it at is OpenAI-compatible, so pin LANGCHAIN_PROVIDER=openai.
        "vibe-trading" => return apply_vibe_trading(&model_info),

        // WorkBuddy (Tencent CodeBuddy 办公版): ~/.workbuddy/models.json.
        "workbuddy" | "workbuddyai" => return apply_workbuddy(tool_id, &model_info).await,

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
//  RESTORE TO OFFICIAL — delete config so tool regenerates defaults
// ════════════════════════════════════════════════════════════════

/// Delete the tool's config file (and any Echobird relay side-channel) so
/// the tool itself regenerates a fresh, vendor-default config on next launch.
/// Used by the App Desktop "restore to official" flow.
pub async fn restore_tool_to_official(tool_id: &str) -> ApplyResult {
    if crate::services::plugin_accounts::provides_tool(tool_id) {
        return plugin_model_operation(tool_id, "model.restore", serde_json::json!({})).await;
    }
    if tool_id == "hermes" {
        return ApplyResult {
            success: false,
            message: "Select a native provider in Hermes to restore its model configuration."
                .into(),
        };
    }
    if tool_id == "codearts" {
        return codearts::configure(tool_id, None).await;
    }
    if tool_id == "opendesign" {
        return opendesign::restore().await;
    }
    if tool_id == "claudescience" {
        return claudescience::restore();
    }
    if tool_id == "vscode" {
        return vscode::restore();
    }
    if matches!(tool_id, "cline" | "clinedesktop") {
        return ApplyResult {
            success: false,
            message: "Select a built-in provider in Cline to restore its native configuration."
                .into(),
        };
    }
    let config_path = match tool_manager::get_tool_config_mapping(tool_id) {
        Some((_, path)) => path,
        None => {
            return ApplyResult {
                success: false,
                message: format!("Unknown tool: {}", tool_id),
            }
        }
    };

    if matches!(tool_id, "codex" | "chatgptdesktop") {
        let codex_config_path = codex_accounts::codex_home().unwrap_or_default();
        return restore_codex_to_official(tool_id, &codex_config_path.join("config.toml"));
    }
    if tool_id == "claudedesktop" {
        let mut result = restore_claudedesktop_to_official();
        if result.success {
            if let Err(message) =
                super::plugin_accounts::prepare_history("claudedesktop", true).await
            {
                result.success = false;
                result.message = message;
            }
        }
        return result;
    }
    if tool_id == "claudecode" {
        return restore_claudecode_to_official();
    }
    if tool_id == "grok" {
        return restore_grok_to_official();
    }
    if matches!(tool_id, "opencode" | "opencodedesktop") {
        return restore_opencode_to_official();
    }
    if tool_id == "mimocode" {
        return restore_mimocode_to_official();
    }
    if tool_id == "mimodesktop" {
        return mimodesktop::restore();
    }
    if matches!(tool_id, "minimaxcode" | "minimaxdesktop") {
        return minimaxcode::restore();
    }
    if tool_id == "musecode" {
        return musecode::restore();
    }
    if tool_id == "kimidesktop" {
        return restore_kimidesktop_to_official();
    }
    if tool_id == "kilo" {
        return restore_kilo_to_official();
    }
    if tool_id == "kilodesktop" {
        return restore_kilo_desktop_to_official();
    }
    if tool_id == "copilot" {
        return restore_copilot_to_official();
    }
    if tool_id == "copilotdesktop" {
        return restore_copilot_desktop_to_official().await;
    }
    if tool_id == "zcode" {
        return restore_zcode_to_official();
    }
    if tool_id == "pi" {
        return restore_pi_to_official();
    }
    if tool_id == "omp" {
        return omp::restore();
    }
    if tool_id == "kimicode" {
        return restore_kimicode_to_official();
    }
    if tool_id == "openscience" {
        return restore_openscience_to_official();
    }
    if tool_id == "dsh" {
        return restore_dsh_to_official();
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
            message: format!(
                "{} already at defaults — no config file to remove.",
                tool_id
            ),
        };
    }

    match fs::remove_file(&config_path) {
        Ok(_) => {
            log::info!(
                "[ToolConfigManager] Restored {} — deleted {:?}",
                tool_id,
                config_path
            );
            ApplyResult {
                success: true,
                message: format!(
                    "{} restored — config deleted, tool will regenerate defaults on next launch.",
                    tool_id
                ),
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
    if crate::services::plugin_accounts::provides_tool(tool_id) {
        let value =
            crate::services::plugin_accounts::call(tool_id, "model.read", serde_json::json!({}))
                .await
                .ok()?;
        return serde_json::from_value(value).ok();
    }
    match tool_id {
        "hermes" => return hermes::read(),
        "codearts" => return codearts::read(tool_id),
        "vscode" => return vscode::read(),
        "openclaw" => return read_openclaw(),
        "opencode" | "opencodedesktop" => return read_opencode(),
        "mimocode" => return read_mimocode(),
        "mimodesktop" => return mimodesktop::read(),
        "minimaxcode" | "minimaxdesktop" => return minimaxcode::read(),
        "musecode" => return musecode::read(),
        "cline" | "clinedesktop" => return cline::read(tool_id == "clinedesktop"),
        "kimidesktop" => return read_kimidesktop(),
        "kilo" => return read_kilo(),
        "kilodesktop" => return read_kilo_desktop(),
        "copilot" => return read_copilot(),
        "copilotdesktop" => return read_copilot_desktop().await,
        "openscience" => return read_openscience(),
        "dsh" => return read_dsh(),
        "zcode" => return read_zcode(),
        "opendesign" => return opendesign::read(),
        "codex" | "chatgptdesktop" => return read_codex(),
        "claudedesktop" => return read_claudedesktop(),
        "claudescience" => return claudescience::read(),
        "claudecode" => return read_claudecode(),
        "aider" => return read_aider(),
        "grok" => return read_grok(),
        "qwencode" => return read_qwen_code(),
        "pi" => return read_pi(),
        "omp" => return omp::read(),
        "kimicode" => return read_kimicode(),
        "vibe-trading" => return read_vibe_trading(),
        "workbuddy" | "workbuddyai" => return read_workbuddy(tool_id),
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

// TOML edits preserve unrelated values, table spelling and comments.

pub(crate) fn toml_read_top(content: &str, key: &str) -> String {
    toml::read(content, "", key)
}

fn toml_delete_top(content: &str, key: &str) -> String {
    toml::write(content, "", key, None)
}

fn toml_write_top(content: &str, key: &str, value: &str) -> String {
    toml::write(content, "", key, Some(toml_edit::value(value)))
}

fn toml_write_top_raw(content: &str, key: &str, value: &str) -> String {
    match toml::raw(value) {
        Some(value) => toml::write(content, "", key, Some(value)),
        None => content.to_string(),
    }
}

pub(crate) fn toml_write_table_value(content: &str, table: &str, key: &str, value: &str) -> String {
    toml::write(content, table, key, Some(toml_edit::value(value)))
}

fn toml_write_table_value_raw(content: &str, table: &str, key: &str, value: &str) -> String {
    match toml::raw(value) {
        Some(value) => toml::write(content, table, key, Some(value)),
        None => content.to_string(),
    }
}

fn toml_delete_table_value(content: &str, table: &str, key: &str) -> String {
    toml::write(content, table, key, None)
}

pub(crate) fn toml_read_table_value(content: &str, table: &str, key: &str) -> String {
    toml::read(content, table, key)
}

fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_toml_equivalent_headers_and_string_examples_are_preserved() {
        for header in [
            "[model_providers.\"OpenAI\"]",
            "[ model_providers . 'OpenAI' ] # keep header",
        ] {
            let example = "Example:\n[model_providers.OpenAI]\nbase_url = \"example\"";
            let content = format!(
                "instructions = '''{example}'''\nmodel = \"old\"\n{header}\nbase_url = \"old\" # keep comment\n[projects.fixture]\ntrust_level = \"trusted\"\n"
            );
            let updated =
                toml_write_table_value(&content, "model_providers.OpenAI", "base_url", "new");
            assert!(updated.contains(&format!("instructions = '''{example}'''")));
            assert_eq!(
                toml_read_table_value(&updated, "model_providers.OpenAI", "base_url"),
                "new"
            );
            assert!(updated.contains(header));
            assert!(updated.contains("# keep comment"));
            assert!(!updated.contains("base_url = \"old\""));
            assert_eq!(
                toml_write_table_value(&updated, "model_providers.OpenAI", "base_url", "new"),
                updated
            );
            assert_eq!(updated.matches("base_url = \"new\"").count(), 1);
            assert!(updated.contains("trust_level = \"trusted\""));
        }
    }

    #[test]
    fn toml_delete_top_removes_a_top_level_key() {
        // The key we own sits before the first [section]: delete it, keep
        // the rest verbatim (whitespace, comments, sections all untouched).
        let content = "model_provider = \"OpenAI\"\n\
                       model = \"gpt-5.5\"\n\
                       review_model = \"gpt-5.5\"\n\
                       \n\
                       [model_providers.OpenAI]\n\
                       name = \"OpenAI\"\n";
        let out = toml_delete_top(content, "review_model");
        assert!(!out.contains("review_model"));
        assert!(out.contains("model = \"gpt-5.5\""));
        assert!(out.contains("[model_providers.OpenAI]"));
        assert!(out.contains("name = \"OpenAI\""));
    }

    #[test]
    fn toml_delete_top_is_noop_when_key_absent() {
        // No key removed → only the join-strips-trailing-newline behavior
        // of `content.lines().collect().join("\n")` changes the string (the
        // caller, write_codex_canonical_fields, re-adds it). Assert the scan
        // matched nothing by checking the lines content survives.
        let content = "model = \"gpt-5.5\"\n[model_providers.OpenAI]\n";
        let out = toml_delete_top(content, "review_model");
        assert!(out.contains("model = \"gpt-5.5\""));
        assert!(out.contains("[model_providers.OpenAI]"));
        assert!(!out.contains("review_model"));
    }

    #[test]
    fn toml_delete_top_ignores_same_named_key_inside_a_section() {
        // A `review_model` that lives INSIDE a [section] belongs to that
        // section — not a top-level key we own — so it must survive. Only
        // the pre-section scan should match.
        let content = "model = \"gpt-5.5\"\n\
                       [model_providers.OpenAI]\n\
                       review_model = \"leave-me\"\n";
        let out = toml_delete_top(content, "review_model");
        assert!(out.contains("review_model = \"leave-me\""));
        assert!(out.contains("model = \"gpt-5.5\""));
    }

    // ── Per-model input modalities (ZCode) ──
    // apply_zcode must declare the real input modalities for a multimodal
    // model instead of forcing text-only.

    #[test]
    fn model_input_modalities_for_multimodal_model() {
        let m = model_input_modalities_for("MiniMax-M3");
        assert_eq!(m, &["text", "image", "video"]);
    }

    #[test]
    fn model_input_modalities_for_unknown_model_defaults_to_text() {
        assert_eq!(model_input_modalities_for("glm-5.2"), &["text"]);
    }
}
