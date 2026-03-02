// Application-level configuration structures

use serde::{Deserialize, Serialize};

/// Skill info (from skill browser)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: String,
    pub name: String,
    pub author: String,
    pub category: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Tool config definition (from default-tools.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfigDef {
    pub id: String,
    pub name: String,
    pub category: String,
    pub official: bool,
    pub description: String,
    pub website: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_rules: Option<ModelRules>,
}

/// Model configuration rules for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRules {
    pub config_format: ConfigFormat,
    pub model_field: String,
    pub template: serde_json::Value,
}

/// Supported config file formats
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFormat {
    Json,
    Yaml,
    Toml,
}

/// Application log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLogEntry {
    pub timestamp: String,
    pub category: LogCategory,
    pub message: String,
}

/// Log categories
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LogCategory {
    Error,
    Model,
    Download,
    Tool,
    Security,
    Channel,
}

/// Proxy rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRule {
    pub pattern: String,
    pub enabled: bool,
}
