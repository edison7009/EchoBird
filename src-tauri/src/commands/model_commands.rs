// Tauri Commands for model operations �?exposed to frontend via invoke()

use crate::models::model::{ModelConfig, PingResult, TestResult};
use crate::services::model_manager::{self, AddModelInput, UpdateModelInput};
use crate::services::upstream_model_catalog::{self, CatalogInput, ModelCatalog};
use crate::services::usage_providers::{self, UsageResult};

#[tauri::command]
pub async fn fetch_upstream_model_catalog(input: CatalogInput) -> Result<ModelCatalog, String> {
    upstream_model_catalog::fetch(input).await
}

#[tauri::command]
pub fn get_cached_upstream_model_catalog(
    input: CatalogInput,
) -> Result<Option<ModelCatalog>, String> {
    upstream_model_catalog::get_cached(input)
}

/// Get all models (user + built-in + local)
#[tauri::command]
pub fn get_models() -> Result<Vec<ModelConfig>, String> {
    model_manager::get_models_checked()
}

/// Add a new model
#[tauri::command]
pub async fn add_model(input: AddModelInput) -> Result<ModelConfig, String> {
    if let Some(source) = &input.account_source {
        crate::services::account_model_sources::validate_source(source).await?;
        if input
            .model_id
            .as_deref()
            .is_none_or(|id| id.trim().is_empty())
        {
            return Err("Model ID is required".into());
        }
    }
    model_manager::add_model(input)
}

/// Delete a model by internal ID
#[tauri::command]
pub fn delete_model(internal_id: String) -> Result<bool, String> {
    let deleted = model_manager::delete_model(&internal_id)?;
    if deleted {
        if let Err(error) = usage_providers::zhipu_team::save_access(&internal_id, None) {
            log::warn!("Failed to clear deleted model's team access: {error}");
        }
        if let Err(error) = crate::services::smart_router::remove_candidate_everywhere(&internal_id)
        {
            log::warn!("Failed to remove deleted model from Smart Router: {error}");
            crate::services::smart_router::forget_candidate_memory(&internal_id);
        }
    }
    Ok(deleted)
}

/// Update a model
#[tauri::command]
pub async fn update_model(
    internal_id: String,
    updates: UpdateModelInput,
) -> Result<Option<ModelConfig>, String> {
    if let Some(source) = &updates.account_source {
        crate::services::account_model_sources::validate_source(source).await?;
    }
    if let Some(existing) = model_manager::get_user_models()
        .iter()
        .find(|model| model.internal_id == internal_id)
    {
        validate_model_update(existing, &updates)?;
    }
    let resets_route_memory = updates.base_url.is_some()
        || updates.anthropic_url.is_some()
        || updates.api_key.is_some()
        || updates.model_id.is_some()
        || updates.account_source.is_some();
    let updated = model_manager::update_model(&internal_id, updates)?;
    if resets_route_memory && updated.is_some() {
        crate::services::smart_router::forget_candidate_memory(&internal_id);
    }
    Ok(updated)
}

fn validate_model_update(existing: &ModelConfig, updates: &UpdateModelInput) -> Result<(), String> {
    if (updates.account_source.is_some() || existing.account_source.is_some())
        && updates
            .model_id
            .as_deref()
            .or(existing.model_id.as_deref())
            .is_none_or(|id| id.trim().is_empty())
    {
        return Err("Model ID is required".into());
    }
    Ok(())
}

/// Persist a user-defined model display order (full visible list of
/// internal_ids in the new order).
#[tauri::command]
pub fn reorder_models(ordered_ids: Vec<String>) -> Result<bool, String> {
    model_manager::reorder_models(ordered_ids)
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
pub async fn ping_model(
    internal_id: String,
    batch_id: Option<String>,
) -> Result<PingResult, String> {
    Ok(model_manager::ping_model(&internal_id, batch_id.as_deref()).await)
}

#[tauri::command]
pub fn finish_connection_test_batch(batch_id: String) {
    crate::services::connection_probe::finish_batch(&batch_id);
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

    // Query usage from provider (internal_id lets the Volcengine provider look
    // up per-model AK/SK).
    usage_providers::query_model_usage(base_url, &api_key, &internal_id).await
}

/// Save Volcengine IAM Access Key / Secret Access Key (encrypted) for a specific
/// model's usage queries. One account per model.
#[tauri::command]
pub fn save_volc_aksk(
    internal_id: String,
    access_key: String,
    secret_key: String,
) -> Result<(), String> {
    if access_key.trim().is_empty() || secret_key.trim().is_empty() {
        return Err("Access Key and Secret Access Key must not be empty".to_string());
    }
    usage_providers::volcengine::write_creds(&internal_id, access_key.trim(), secret_key.trim())
}

/// Whether Volcengine AK/SK are stored for a specific model.
#[tauri::command]
pub fn has_volc_aksk(internal_id: String) -> bool {
    usage_providers::volcengine::has_creds(&internal_id)
}

/// Remove stored Volcengine AK/SK for a specific model.
#[tauri::command]
pub fn clear_volc_aksk(internal_id: String) -> bool {
    usage_providers::volcengine::clear_creds(&internal_id);
    true
}

/// Stored AK/SK pair (plaintext) returned to the frontend to pre-fill the modal.
#[derive(serde::Serialize)]
pub struct VolcAksk {
    pub access_key: String,
    pub secret_key: String,
}

/// Read stored AK/SK (plaintext) for a model, to pre-fill the config modal.
#[tauri::command]
pub fn get_volc_aksk(internal_id: String) -> Option<VolcAksk> {
    usage_providers::volcengine::read_creds(&internal_id).map(|(ak, sk)| VolcAksk {
        access_key: ak,
        secret_key: sk,
    })
}

#[tauri::command]
pub fn get_zhipu_team_access(
    internal_id: String,
) -> Result<Option<usage_providers::zhipu_team::TeamAccess>, String> {
    usage_providers::zhipu_team::read_access(&internal_id)
}

#[tauri::command]
pub fn save_zhipu_team_access(
    internal_id: String,
    access: Option<usage_providers::zhipu_team::TeamAccess>,
) -> Result<(), String> {
    let models = model_manager::get_models();
    if !models.iter().any(|model| model.internal_id == internal_id) {
        return Err("Model not found".to_string());
    }
    usage_providers::zhipu_team::save_access(&internal_id, access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::model::{AccountProvider, AccountSource};

    fn public_model_fixture() -> ModelConfig {
        serde_json::from_value(serde_json::json!({
            "internalId":"zen-fixture","name":"Public models","baseUrl":"https://opencode.ai/inference/v1",
            "modelId":"cat-jam-free","scope":"smartRouter","apiKey":""
        })).unwrap()
    }

    #[test]
    fn api_model_updates_allow_switching_models_and_clearing_the_key() {
        let existing = public_model_fixture();
        let before = serde_json::to_value(&existing).unwrap();
        let paid = UpdateModelInput {
            model_id: Some("paid-model".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &paid).is_ok());
        assert_eq!(serde_json::to_value(&existing).unwrap(), before);
        let mut keyed = existing.clone();
        keyed.api_key = "saved-key".into();
        assert!(validate_model_update(&keyed, &paid).is_ok());
        for key in ["", "   ", "enc:v1:00"] {
            let remove_key = UpdateModelInput {
                model_id: Some("paid-model".into()),
                api_key: Some(key.into()),
                ..Default::default()
            };
            assert!(validate_model_update(&keyed, &remove_key).is_ok());
        }
        let with_key = UpdateModelInput {
            model_id: Some("paid-model".into()),
            api_key: Some("new-key".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &with_key).is_ok());
    }

    #[test]
    fn dynamic_public_model_update_keeps_keyless_and_unrelated_configuration_flows() {
        let existing = public_model_fixture();
        let next_free = UpdateModelInput {
            model_id: Some("new-future-free".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &next_free).is_ok());
        let paid = UpdateModelInput {
            model_id: Some("paid-model".into()),
            ..Default::default()
        };
        let mut model_center = existing.clone();
        model_center.scope = crate::models::model::ModelScope::ModelCenter;
        assert!(validate_model_update(&model_center, &paid).is_ok());
        let other_endpoint = UpdateModelInput {
            base_url: Some("https://api.example.com/v1".into()),
            model_id: Some("paid-model".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &other_endpoint).is_ok());
        let health = UpdateModelInput {
            openai_tested: Some(false),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &health).is_ok());
        let account = UpdateModelInput {
            account_source: Some(AccountSource {
                provider: AccountProvider::Codex,
                account_id: "fixture-account".into(),
            }),
            model_id: Some("native-model".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &account).is_ok());
    }

    #[test]
    fn api_model_updates_allow_a_public_endpoint_without_a_key() {
        let mut existing = public_model_fixture();
        existing.base_url = "https://api.example.com/v1".into();
        let updates = UpdateModelInput {
            base_url: Some("https://opencode.ai/inference/v1".into()),
            model_id: Some("paid-model".into()),
            ..Default::default()
        };
        assert!(validate_model_update(&existing, &updates).is_ok());
    }

    #[test]
    fn account_update_requires_effective_model_id_and_preserves_partial_updates() {
        let mut existing: ModelConfig = serde_json::from_value(serde_json::json!({
            "internalId":"fixture","name":"Model","baseUrl":"account://echobird","modelId":"existing-model",
            "accountSource":{"provider":"codex","accountId":"saved-id"}
        })).unwrap();
        assert!(validate_model_update(&existing, &UpdateModelInput::default()).is_ok());
        for id in ["", "   "] {
            assert!(validate_model_update(
                &existing,
                &UpdateModelInput {
                    model_id: Some(id.into()),
                    ..Default::default()
                }
            )
            .is_err());
        }
        existing.account_source = None;
        existing.model_id = None;
        assert!(validate_model_update(&existing, &UpdateModelInput::default()).is_ok());
        assert!(validate_model_update(
            &existing,
            &UpdateModelInput {
                account_source: Some(AccountSource {
                    provider: AccountProvider::Codex,
                    account_id: "saved-id".into()
                }),
                ..Default::default()
            }
        )
        .is_err());
        assert!(validate_model_update(
            &existing,
            &UpdateModelInput {
                account_source: Some(AccountSource {
                    provider: AccountProvider::Codex,
                    account_id: "saved-id".into()
                }),
                model_id: Some("selected-model".into()),
                ..Default::default()
            }
        )
        .is_ok());
    }
}
