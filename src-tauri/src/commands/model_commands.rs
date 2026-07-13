// Tauri Commands for model operations �?exposed to frontend via invoke()

use crate::models::model::{ModelConfig, PingResult, TestResult};
use crate::services::model_manager::{self, AddModelInput, UpdateModelInput};
use crate::services::usage_providers::{self, UsageResult};

/// Get all models (user + built-in + local)
#[tauri::command]
pub fn get_models() -> Vec<ModelConfig> {
    model_manager::get_models()
}

/// Add a new model
#[tauri::command]
pub fn add_model(input: AddModelInput) -> ModelConfig {
    model_manager::add_model(input)
}

/// Delete a model by internal ID
#[tauri::command]
pub fn delete_model(internal_id: String) -> bool {
    model_manager::delete_model(&internal_id)
}

/// Update a model
#[tauri::command]
pub fn update_model(internal_id: String, updates: UpdateModelInput) -> Option<ModelConfig> {
    model_manager::update_model(&internal_id, updates)
}

/// Test model with API request
#[tauri::command]
pub async fn test_model(
    internal_id: String,
    prompt: String,
    protocol: String,
) -> Result<TestResult, String> {
    Ok(model_manager::test_model(&internal_id, &prompt, &protocol).await)
}

/// Ping model server
#[tauri::command]
pub async fn ping_model(internal_id: String) -> Result<PingResult, String> {
    Ok(model_manager::ping_model(&internal_id).await)
}

/// Check if encrypted key is destroyed
#[tauri::command]
pub fn is_key_destroyed(internal_id: String) -> bool {
    model_manager::is_key_destroyed(&internal_id)
}

/// Query model usage (quota/balance)
#[tauri::command]
pub async fn query_model_usage(internal_id: String) -> Result<UsageResult, String> {
    let models = model_manager::get_models();
    let model = models
        .iter()
        .find(|m| m.internal_id == internal_id)
        .ok_or_else(|| format!("Model not found: {}", internal_id))?;

    // Determine base_url and api_key
    let base_url = if !model.base_url.is_empty() {
        &model.base_url
    } else if let Some(ref url) = model.anthropic_url {
        url
    } else {
        return Err("Model has no base URL configured".to_string());
    };

    let api_key = model_manager::decrypt_key_for_use(&model.api_key);

    // Query usage from provider
    usage_providers::query_model_usage(base_url, &api_key).await
}

/// Volcengine SSO login: runs `arkcli auth login volc-sso` (browser auth),
/// then creates a platform profile so usage queries work.
#[tauri::command]
pub async fn volc_sso_login() -> Result<bool, String> {
    // Step 1: SSO login (blocks until browser auth completes or fails)
    let mut login = usage_providers::arkcli_command();
    login.args(["auth", "login", "volc-sso"]);
    let login = login
        .output()
        .map_err(|e| format!("Failed to run arkcli: {}", e))?;

    if !login.status.success() {
        let stderr = String::from_utf8_lossy(&login.stderr);
        return Err(format!("arkcli SSO login failed: {}", stderr.trim()));
    }

    // Step 2: Create platform profile (non-interactive)
    let mut profile = usage_providers::arkcli_command();
    profile.args([
        "profile",
        "create",
        "--type",
        "platform",
        "--project",
        "default",
        "--region",
        "cn-beijing",
        "--set-default",
        "--no-interactive",
    ]);
    let profile = profile
        .output()
        .map_err(|e| format!("Failed to create profile: {}", e))?;

    // Profile creation may fail if one already exists; that's OK
    let profile_ok = profile.status.success()
        || String::from_utf8_lossy(&profile.stderr).contains("already");
    Ok(profile_ok)
}

/// Check if Volcengine SSO is valid (not expired).
///
/// Runs `arkcli auth whoami` in a blocking task so the main/UI thread is
/// not stalled while the subprocess runs (this fires on every Model Nexus
/// mount). Same pattern as ai_career_commands.
#[tauri::command]
pub async fn check_volc_sso() -> bool {
    tauri::async_runtime::spawn_blocking(|| {
        let mut cmd = usage_providers::arkcli_command();
        cmd.args(["auth", "whoami", "--format", "json"]);
        let output = cmd.output();

        match output {
            Ok(o) if o.status.success() => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    let logged_in = json.get("logged_in").and_then(|v| v.as_bool()).unwrap_or(false);
                    let sso_expired = json
                        .get("sso_expired")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let auth_method = json
                        .get("auth_method")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    logged_in && !sso_expired && auth_method == "sso"
                } else {
                    false
                }
            }
            _ => false,
        }
    })
    .await
    .unwrap_or(false)
}
