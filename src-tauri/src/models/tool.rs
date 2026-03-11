// Tool & Model data structures �?mirrors tools/types.ts + loader.ts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── paths.json data structure (tool detection & metadata) ───

/// Platform-specific candidate paths for tool detection
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformPaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub win32: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub darwin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<String>>,
}

/// Skills path configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillsPathConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub win32: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub darwin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_module: Option<String>,
}

/// paths.json �?tool metadata + detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<String, String>>,
    pub category: String,
    #[serde(default)]
    pub api_protocol: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(default)]
    pub docs: String,
    #[serde(default)]
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(default)]
    pub config_dir: String,
    #[serde(default)]
    pub config_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_file_alt: Option<String>,
    #[serde(default)]
    pub require_config_file: bool,
    #[serde(default)]
    pub detect_by_config_dir: bool,
    #[serde(default)]
    pub paths: PlatformPaths,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_path: Option<SkillsPathConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_skills_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_paths: Option<PlatformPaths>,
    #[serde(default)]
    pub always_installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub launchable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
}

// ─── config.json data structure (config read/write mapping) ───

/// Read mapping: ModelInfo field �?config file path(s) (priority order)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<Vec<String>>,
}

/// config.json �?configuration read/write mapping
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMapping {
    #[serde(default)]
    pub docs: String,
    pub config_file: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub custom: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ConfigReadMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_proxy: Option<HashMap<String, String>>,
}

fn default_format() -> String {
    "json".to_string()
}

/// Loaded tool definition (paths.json + config.json combined)
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub id: String,
    pub paths_config: PathsConfig,
    pub config_mapping: ConfigMapping,
    pub tool_dir: String,
}

// ─── DetectedTool (sent to frontend) ───

/// Tool category
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCategory {
    CLI,
    AgentOS,
    IDE,
    AutoTrading,
    Game,
    Utility,
    Custom,
}

/// Detected tool with runtime info (sent to frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub category: ToolCategory,
    pub official: bool,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_skills_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_protocol: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}
