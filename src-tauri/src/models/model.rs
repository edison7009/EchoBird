// Model configuration structures — mirrors old modelManager.ts types

use serde::{Deserialize, Serialize};

/// Model type indicator
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModelType {
    Cloud,
    Local,
    Tunnel,
    Demo,
}

/// Model configuration (stored in ~/.echobird/models.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub internal_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub model_type: Option<ModelType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_tested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_tested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_latency: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_latency: Option<f64>,
}

/// Model test result (returned to frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub success: bool,
    pub latency: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub protocol: String,
}

/// Model ping result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {
    pub success: bool,
    pub latency: f64,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
