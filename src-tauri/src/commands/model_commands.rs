// Tauri Commands for model operations �?exposed to frontend via invoke()

use crate::models::model::{ModelConfig, PingResult, TestResult};
use crate::services::model_manager::{self, AddModelInput, UpdateModelInput};

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
