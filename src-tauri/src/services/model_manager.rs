// Model Manager �?mirrors old modelManager.ts
// Handles model CRUD, API testing, ping, and API key encryption

use std::fs;
use std::time::Instant;

use crate::models::model::{ModelConfig, ModelType, PingResult, TestResult};
use crate::utils::platform::echobird_dir;

#[cfg(test)]
#[path = "model_manager/persistence_tests.rs"]
mod persistence_tests;

// ─── Config paths ───

/// Models config file: ~/.echobird/config/models.json
fn models_config_path() -> std::path::PathBuf {
    echobird_dir().join("config").join("models.json")
}

// ─── User-defined display order ───
//
// The visible model list is a merge (local server → user → built-in). Drag
// reorder in the Model Center persists the FULL visible order here as a JSON
// array of internal_ids. get_models() sorts by it; ids absent from the stored
// order (newly added models, built-ins shipped in a later update) keep their
// default merge position and sit below the ordered ones.

/// Model order file: ~/.echobird/config/model-order.json
fn model_order_path() -> std::path::PathBuf {
    echobird_dir().join("config").join("model-order.json")
}

fn load_model_order() -> Vec<String> {
    let path = model_order_path();
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            log::error!("[ModelManager] Failed to parse model order: {}", e);
            Vec::new()
        }),
        Err(e) => {
            log::error!("[ModelManager] Failed to read model order: {}", e);
            Vec::new()
        }
    }
}

fn save_model_order(ordered_ids: &[String]) -> Result<(), String> {
    super::json_store::mutate(&model_order_path(), |saved: &mut Vec<String>| {
        *saved = ordered_ids.to_vec();
        Ok(((), true))
    })
}

/// Sort models by the persisted user order. Known ids follow the stored rank;
/// unknown ids keep their default merge order and sink below ordered ones.
fn apply_user_order(mut models: Vec<ModelConfig>, order: &[String]) -> Vec<ModelConfig> {
    if order.is_empty() || models.len() < 2 {
        return models;
    }
    let rank: std::collections::HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let default_rank: std::collections::HashMap<String, usize> = models
        .iter()
        .enumerate()
        .map(|(i, m)| (m.internal_id.clone(), i))
        .collect();
    models.sort_by_key(|m| match rank.get(m.internal_id.as_str()) {
        Some(&r) => (0usize, r),
        None => (
            1usize,
            default_rank
                .get(&m.internal_id)
                .copied()
                .unwrap_or(usize::MAX),
        ),
    });
    models
}

/// Persist a user-defined model display order (the frontend sends every
/// visible internal_id in the new order). Returns false for an empty payload.
pub fn reorder_models(ordered_ids: Vec<String>) -> Result<bool, String> {
    if ordered_ids.is_empty() {
        return Ok(false);
    }
    save_model_order(&ordered_ids)?;
    log::info!(
        "[ModelManager] Saved model order ({} ids)",
        ordered_ids.len()
    );
    Ok(true)
}

// ─── API Key encryption (AES-256-GCM) ───

#[cfg(test)]
use super::secret_store::{derive_encryption_key, machine_fingerprint_with, ENCRYPTED_PREFIX};

fn is_encrypted(key: &str) -> bool {
    key.starts_with("enc:v1:")
}
fn decrypt_api_key(key: &str) -> String {
    super::secret_store::decrypt(key).unwrap_or_default()
}

pub fn decrypt_key_for_use(key: &str) -> String {
    decrypt_api_key(key)
}

/// Compatibility for native adapters which validate the envelope before saving.
/// Errors never downgrade to plaintext; new consumers use secret_store::encrypt directly.
pub fn encrypt_key_for_storage(plain: &str) -> String {
    super::secret_store::encrypt(plain).unwrap_or_else(|error| {
        log::error!("[SecretStore] {error}");
        String::new()
    })
}

// ─── Model CRUD ───

pub(crate) fn prepare_initial_router_defaults() -> Result<Option<Vec<ModelConfig>>, String> {
    prepare_initial_router_defaults_at(&echobird_dir())
}

fn prepare_initial_router_defaults_at(
    root: &std::path::Path,
) -> Result<Option<Vec<ModelConfig>>, String> {
    if !is_new_installation_at(root)? {
        return Ok(None);
    }
    let catalog = super::free_model_directory::bundled_free_model_directory()?;
    initial_router_defaults(&catalog).map(Some)
}

fn is_new_installation_at(root: &std::path::Path) -> Result<bool, String> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(format!("Failed to inspect saved application data: {error}")),
    };
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("Failed to inspect application data: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("Failed to inspect application data: {error}"))?;
        if entry.file_name() != "config" || !file_type.is_dir() {
            return Ok(false);
        }
        // These listeners start before the router during first launch.
        for config in fs::read_dir(entry.path())
            .map_err(|error| format!("Failed to inspect saved configuration: {error}"))?
        {
            let config = config
                .map_err(|error| format!("Failed to inspect saved configuration: {error}"))?;
            let generated_port = matches!(
                config.file_name().to_str(),
                Some("anthropic-proxy-port.json" | "musecode-proxy-port.json")
            );
            if !generated_port
                || !config
                    .file_type()
                    .map_err(|error| format!("Failed to inspect saved configuration: {error}"))?
                    .is_file()
            {
                return Ok(false);
            }
            let port: serde_json::Value = serde_json::from_slice(
                &fs::read(config.path())
                    .map_err(|error| format!("Failed to read saved proxy port: {error}"))?,
            )
            .map_err(|error| format!("Failed to parse saved proxy port: {error}"))?;
            if !port["port"]
                .as_u64()
                .is_some_and(|value| value > 0 && value <= u16::MAX as u64)
            {
                return Err("Saved proxy port is invalid".to_string());
            }
        }
    }
    Ok(true)
}

fn initial_router_defaults(catalog: &serde_json::Value) -> Result<Vec<ModelConfig>, String> {
    let entries = catalog["models"]
        .as_array()
        .ok_or_else(|| "Bundled free model catalog has no models".to_string())?;
    [
        ("kilo", "kilo-auto/free", "router-default-kilo"),
        ("opencode-zen-free", "big-pickle", "router-default-zen-free"),
    ]
    .into_iter()
    .map(|(provider_id, model_id, internal_id)| {
        let entry = entries
            .iter()
            .find(|entry| {
                entry["providerId"].as_str() == Some(provider_id)
                    && entry["modelId"].as_str() == Some(model_id)
            })
            .ok_or_else(|| format!("Bundled default model is unavailable: {model_id}"))?;
        let base_url = entry["baseUrl"]
            .as_str()
            .ok_or_else(|| format!("Bundled default model has no endpoint: {model_id}"))?;
        if super::free_model_directory::keyless_auth_in_catalog(catalog, base_url, model_id)
            .is_none()
        {
            return Err(format!("Bundled default model requires a key: {model_id}"));
        }
        let name = entry["provider"]
            .as_str()
            .ok_or_else(|| format!("Bundled default model has no provider: {model_id}"))?;
        Ok(new_model_from(
            AddModelInput {
                name: Some(name.to_string()),
                model_id: Some(model_id.to_string()),
                base_url: Some(base_url.to_string()),
                scope: crate::models::model::ModelScope::SmartRouter,
                ..Default::default()
            },
            internal_id.to_string(),
        ))
    })
    .collect()
}

pub(crate) fn persist_initial_router_defaults() -> Result<(), String> {
    let catalog = super::free_model_directory::bundled_free_model_directory()?;
    persist_initial_router_defaults_at(&echobird_dir(), &catalog)
}

fn persist_initial_router_defaults_at(
    root: &std::path::Path,
    catalog: &serde_json::Value,
) -> Result<(), String> {
    let defaults = initial_router_defaults(catalog)?;
    let path = root.join("config").join("models.json");
    super::json_store::mutate(&path, |models: &mut Vec<ModelConfig>| {
        let original_len = models.len();
        for model in defaults {
            if !models
                .iter()
                .any(|saved| saved.internal_id == model.internal_id)
            {
                models.push(model);
            }
        }
        super::free_model_directory::seed_catalog_cache_at(root, catalog)?;
        Ok(((), models.len() != original_len))
    })
}

/// Read raw user models from disk
fn get_raw_user_models() -> Vec<ModelConfig> {
    super::json_store::read(&models_config_path()).unwrap_or_else(|error| {
        log::error!("[ModelManager] {error}");
        Vec::new()
    })
}

/// Get user models
pub fn get_user_models() -> Vec<ModelConfig> {
    get_raw_user_models()
}

/// Get built-in demo models from bundled config
fn get_built_in_models() -> Vec<ModelConfig> {
    // Try multiple possible paths for bundled models.json
    let possible_paths = vec![
        // Development: project_root/config/models.json
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("config").join("models.json")))
            .unwrap_or_default(),
        // Alternative location
        echobird_dir().join("config").join("built-in-models.json"),
    ];

    for path in possible_paths {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(models) = serde_json::from_str::<Vec<ModelConfig>>(&content) {
                    log::info!(
                        "[ModelManager] Loaded {} built-in models from {:?}",
                        models.len(),
                        path
                    );
                    return models;
                }
            }
        }
    }

    log::debug!("[ModelManager] No built-in models found");
    Vec::new()
}

/// Get all models (local server + user + built-in)
pub fn get_models() -> Vec<ModelConfig> {
    models_from_user(get_user_models())
}

pub fn get_models_checked() -> Result<Vec<ModelConfig>, String> {
    Ok(models_from_user(super::json_store::read(
        &models_config_path(),
    )?))
}

fn models_from_user(models: Vec<ModelConfig>) -> Vec<ModelConfig> {
    let user_models = models
        .into_iter()
        .filter(|model| model.scope == crate::models::model::ModelScope::ModelCenter)
        .collect::<Vec<_>>();
    let built_in_models = get_built_in_models();

    // Local server model (dynamic injection when running)
    let server_info = crate::services::local_llm::get_server_info_sync();
    let local_models: Vec<ModelConfig> = if server_info.running {
        vec![ModelConfig {
            internal_id: "local-server".to_string(),
            name: server_info.model_name.clone(),
            model_id: Some(server_info.model_name.clone()),
            base_url: format!("http://127.0.0.1:{}/v1", server_info.port),
            api_key: server_info.api_key.clone(),
            account_source: None,
            anthropic_url: Some(format!("http://127.0.0.1:{}/anthropic", server_info.port)),
            model_type: Some(crate::models::model::ModelType::Local),
            openai_tested: None,
            anthropic_tested: None,
            openai_latency: None,
            anthropic_latency: None,
            scope: crate::models::model::ModelScope::ModelCenter,
        }]
    } else {
        Vec::new()
    };

    let smart_router_models = crate::services::smart_router::profiles::model_configs();

    // Order: local → smart router → user → built-in, then apply the user's drag-reorder
    let mut all = Vec::new();
    all.extend(local_models);
    all.extend(smart_router_models);
    all.extend(user_models);
    all.extend(built_in_models);
    apply_user_order(all, &load_model_order())
}

/// Generate unique internal ID (m-abc123 format)
fn generate_internal_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("m-{:x}", seed % 0xFFFFFF)
}

/// Auto-detect model type from URL
fn detect_model_type(url: &str) -> Option<ModelType> {
    if url.is_empty() {
        return None;
    }
    if url.contains("localhost") || url.contains("127.0.0.1") || url.contains("192.168.") {
        Some(ModelType::Local)
    } else if url::Url::parse(url).is_ok() {
        Some(ModelType::Cloud)
    } else {
        None
    }
}

/// Add a new model
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddModelInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub anthropic_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub account_source: Option<crate::models::model::AccountSource>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub scope: crate::models::model::ModelScope,
}

pub fn add_model(input: AddModelInput) -> Result<ModelConfig, String> {
    add_model_at(&models_config_path(), input)
}

fn add_model_at(path: &std::path::Path, input: AddModelInput) -> Result<ModelConfig, String> {
    super::json_store::mutate(path, |models: &mut Vec<ModelConfig>| {
        let model = new_model_from(input, generate_internal_id());
        models.push(model.clone());
        Ok((model, true))
    })
}

fn new_model_from(input: AddModelInput, internal_id: String) -> ModelConfig {
    let base_url = if input.account_source.is_some() {
        "account://echobird".into()
    } else {
        input.base_url.unwrap_or_default()
    };
    let auto_type = detect_model_type(&base_url);
    ModelConfig {
        internal_id,
        name: input.name.unwrap_or_default(),
        model_id: input.model_id,
        base_url,
        api_key: if input.account_source.is_some() {
            String::new()
        } else {
            input.api_key.unwrap_or_default()
        },
        account_source: input.account_source.clone(),
        anthropic_url: if input.account_source.is_some() {
            None
        } else {
            input.anthropic_url
        },
        model_type: auto_type,
        openai_tested: None,
        anthropic_tested: None,
        openai_latency: None,
        anthropic_latency: None,
        scope: if input.account_source.is_some() {
            crate::models::model::ModelScope::SmartRouter
        } else {
            input.scope
        },
    }
}

/// Delete a user model by internal ID
pub fn delete_model(internal_id: &str) -> Result<bool, String> {
    delete_model_at(&models_config_path(), internal_id)
}

fn delete_model_at(path: &std::path::Path, internal_id: &str) -> Result<bool, String> {
    super::json_store::mutate(path, |models: &mut Vec<ModelConfig>| {
        let before = models.len();
        models.retain(|m| m.internal_id != internal_id);
        let changed = before != models.len();
        Ok((changed, changed))
    })
}

/// Update model fields
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateModelInput {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub anthropic_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub account_source: Option<crate::models::model::AccountSource>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub openai_tested: Option<bool>,
    #[serde(default)]
    pub anthropic_tested: Option<bool>,
    #[serde(default)]
    pub openai_latency: Option<f64>,
    #[serde(default)]
    pub anthropic_latency: Option<f64>,
}

pub fn update_model(
    internal_id: &str,
    updates: UpdateModelInput,
) -> Result<Option<ModelConfig>, String> {
    update_model_at(&models_config_path(), internal_id, updates)
}

fn update_model_at(
    path: &std::path::Path,
    internal_id: &str,
    updates: UpdateModelInput,
) -> Result<Option<ModelConfig>, String> {
    super::json_store::mutate(path, |models: &mut Vec<ModelConfig>| {
        let Some(model) = models.iter_mut().find(|m| m.internal_id == internal_id) else {
            return Ok((None, false));
        };
        apply_model_updates(model, updates);
        Ok((Some(model.clone()), true))
    })
}

#[cfg(test)]
fn save_updated_models_at(path: &std::path::Path, models: &[ModelConfig]) -> Result<(), String> {
    super::json_store::mutate(path, |saved: &mut Vec<ModelConfig>| {
        *saved = models.to_vec();
        Ok(((), true))
    })
}

fn apply_model_updates(model: &mut ModelConfig, updates: UpdateModelInput) {
    // Apply updates
    if let Some(name) = updates.name {
        model.name = name;
    }
    if let Some(url) = &updates.base_url {
        model.base_url = url.clone();
        model.model_type = detect_model_type(url);
    }
    if let Some(url) = updates.anthropic_url {
        model.anthropic_url = if url.is_empty() { None } else { Some(url) };
    }
    if let Some(key) = updates.api_key {
        model.api_key = key;
    }
    if let Some(id) = updates.model_id {
        model.model_id = Some(id);
    }
    if let Some(source) = updates.account_source {
        model.account_source = Some(source);
    }
    if model.account_source.is_some() {
        model.api_key.clear();
        model.base_url = "account://echobird".into();
        model.anthropic_url = None;
        model.scope = crate::models::model::ModelScope::SmartRouter;
    }
    if let Some(v) = updates.openai_tested {
        model.openai_tested = Some(v);
    }
    if let Some(v) = updates.anthropic_tested {
        model.anthropic_tested = Some(v);
    }
    if let Some(v) = updates.openai_latency {
        model.openai_latency = Some(v);
    }
    if let Some(v) = updates.anthropic_latency {
        model.anthropic_latency = Some(v);
    }
}

// ─── Test & Ping ───

/// Build HTTP client
fn build_client(_model: &ModelConfig) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

/// Test model with OpenAI or Anthropic protocol
pub async fn test_model(internal_id: &str, prompt: &str, protocol: &str) -> TestResult {
    let models = get_models();
    let model = match models.iter().find(|m| m.internal_id == internal_id) {
        Some(m) => m.clone(),
        None => {
            return TestResult {
                success: false,
                latency: 0.0,
                response: None,
                error: Some("Model not found".to_string()),
                protocol: protocol.to_string(),
            };
        }
    };

    let start = Instant::now();
    let client = build_client(&model);
    let usable_key = decrypt_key_for_use(&model.api_key);

    if protocol == "anthropic" {
        // Anthropic protocol test
        let anthropic_url = match &model.anthropic_url {
            Some(url) if !url.is_empty() => url.clone(),
            _ => {
                return TestResult {
                    success: false,
                    latency: 0.0,
                    response: None,
                    error: Some("Anthropic URL not configured".to_string()),
                    protocol: protocol.to_string(),
                };
            }
        };

        // Smart append /v1/messages
        let url = if anthropic_url.contains("/messages") {
            anthropic_url
        } else {
            format!("{}/v1/messages", anthropic_url.trim_end_matches('/'))
        };

        let body = serde_json::json!({
            "model": model.model_id.as_deref().unwrap_or(""),
            "max_tokens": 100,
            "messages": [{ "role": "user", "content": prompt }]
        });

        let mut req = client.post(&url).header("Content-Type", "application/json");

        // Xiaomi Mimo uses Bearer, standard Anthropic uses x-api-key
        if url.contains("xiaomimimo.com") {
            req = req.header("Authorization", format!("Bearer {}", usable_key));
        } else {
            req = req
                .header("x-api-key", &usable_key)
                .header("anthropic-version", "2023-06-01");
        }

        match req.json(&body).send().await {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as f64;
                if resp.status().is_success() {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    // Find first text block — model may prepend thinking blocks (MiniMax, Claude extended thinking)
                    let content = data["content"]
                        .as_array()
                        .and_then(|blocks| {
                            blocks
                                .iter()
                                .find(|b| b["type"].as_str() == Some("text"))
                                .and_then(|b| b["text"].as_str())
                        })
                        // Fallback: some Anthropic-compatible endpoints return OpenAI choices format
                        .or_else(|| data["choices"][0]["message"]["content"].as_str())
                        .unwrap_or("No response")
                        .to_string();

                    // Auto-save test status
                    let _ = update_model(
                        internal_id,
                        UpdateModelInput {
                            anthropic_tested: Some(true),
                            anthropic_latency: Some(latency),
                            ..Default::default()
                        },
                    );

                    log::info!(
                        "[ModelManager] Anthropic test succeeded: {} {}ms",
                        model.name,
                        latency
                    );
                    TestResult {
                        success: true,
                        latency,
                        response: Some(content),
                        error: None,
                        protocol: protocol.to_string(),
                    }
                } else {
                    let status = resp.status().as_u16();
                    let error_text = resp.text().await.unwrap_or_default();
                    TestResult {
                        success: false,
                        latency,
                        response: None,
                        error: Some(format!("HTTP {}: {}", status, error_text)),
                        protocol: protocol.to_string(),
                    }
                }
            }
            Err(e) => TestResult {
                success: false,
                latency: -1.0,
                response: None,
                error: Some(e.to_string()),
                protocol: protocol.to_string(),
            },
        }
    } else {
        // OpenAI protocol test
        if model.base_url.is_empty() {
            return TestResult {
                success: false,
                latency: 0.0,
                response: None,
                error: Some("OpenAI URL not configured".to_string()),
                protocol: protocol.to_string(),
            };
        }

        // Smart append /chat/completions
        let url = if model.base_url.contains("/chat/completions") {
            model.base_url.clone()
        } else {
            format!("{}/chat/completions", model.base_url.trim_end_matches('/'))
        };

        let body = serde_json::json!({
            "model": model.model_id.as_deref().unwrap_or(""),
            "messages": [{ "role": "user", "content": prompt }],
            "max_tokens": 100,
            "stream": false,
            "temperature": 0.7
        });

        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", usable_key))
            .json(&body)
            .send()
            .await
        {
            Ok(resp) => {
                let latency = start.elapsed().as_millis() as f64;
                if resp.status().is_success() {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    let content = data["choices"][0]["message"]["content"]
                        .as_str()
                        .unwrap_or("No response")
                        .to_string();

                    let _ = update_model(
                        internal_id,
                        UpdateModelInput {
                            openai_tested: Some(true),
                            openai_latency: Some(latency),
                            ..Default::default()
                        },
                    );

                    log::info!(
                        "[ModelManager] OpenAI test succeeded: {} {}ms",
                        model.name,
                        latency
                    );
                    TestResult {
                        success: true,
                        latency,
                        response: Some(content),
                        error: None,
                        protocol: protocol.to_string(),
                    }
                } else {
                    let status = resp.status().as_u16();
                    let error_text = resp.text().await.unwrap_or_default();
                    TestResult {
                        success: false,
                        latency,
                        response: None,
                        error: Some(format!("HTTP {}: {}", status, error_text)),
                        protocol: protocol.to_string(),
                    }
                }
            }
            Err(e) => TestResult {
                success: false,
                latency: -1.0,
                response: None,
                error: Some(e.to_string()),
                protocol: protocol.to_string(),
            },
        }
    }
}

/// Ping model server (HEAD request, test reachability only)
pub async fn ping_model(internal_id: &str, batch_id: Option<&str>) -> PingResult {
    let models = get_models();
    let model = match models.iter().find(|m| m.internal_id == internal_id) {
        Some(m) => m.clone(),
        None => {
            return PingResult {
                success: false,
                latency: 0.0,
                url: String::new(),
                error: Some("Model not found".to_string()),
            };
        }
    };

    let url = if !model.base_url.is_empty() {
        &model.base_url
    } else {
        match &model.anthropic_url {
            Some(u) if !u.is_empty() => u,
            _ => {
                return PingResult {
                    success: false,
                    latency: 0.0,
                    url: String::new(),
                    error: Some("No URL configured".to_string()),
                };
            }
        }
    };

    let result = ping_endpoint(url, batch_id).await;
    if result.success {
        let _ = update_model(
            internal_id,
            UpdateModelInput {
                openai_latency: Some(result.latency),
                ..Default::default()
            },
        );
        log::info!(
            "[ModelManager] Ping succeeded: {} {}ms",
            model.name,
            result.latency
        );
    }
    result
}

async fn ping_endpoint(endpoint: &str, batch_id: Option<&str>) -> PingResult {
    let result = super::connection_probe::measure(endpoint, batch_id).await;
    PingResult {
        success: result.is_ok(),
        latency: result.map(|latency| latency as f64).unwrap_or(-1.0),
        url: super::connection_probe::origin(endpoint).unwrap_or_else(|| endpoint.to_owned()),
        error: result.err().map(str::to_owned),
    }
}

/// Check if an encrypted key has been destroyed (decryption fails)
pub fn is_key_destroyed(internal_id: &str) -> bool {
    let models = get_raw_user_models();
    let model = match models.iter().find(|m| m.internal_id == internal_id) {
        Some(m) => m,
        None => return false,
    };

    if !is_encrypted(&model.api_key) {
        return false;
    }

    let decrypted = decrypt_api_key(&model.api_key);
    decrypted.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::model::ModelConfig;

    #[tokio::test]
    async fn ping_endpoint_measures_head_and_distinguishes_timeout_from_other_failures() {
        use axum::{
            http::{HeaderMap, Method, StatusCode, Uri},
            routing::any,
            Router,
        };
        use std::{
            sync::{Arc, Mutex},
            time::Duration,
        };
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured = calls.clone();
        let app =
            Router::new().fallback(any(move |method: Method, uri: Uri, headers: HeaderMap| {
                captured.lock().unwrap().push((method, uri, headers));
                async {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    StatusCode::UNAUTHORIZED
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = ping_endpoint(&format!("{origin}/v1?token=unused"), None).await;
        assert!(result.success);
        assert!(result.latency >= 25.0 && result.latency <= 1000.0);
        assert_eq!(result.url, origin);
        {
            let calls = calls.lock().unwrap();
            assert_eq!(calls.len(), 5);
            for call in calls.iter() {
                assert_eq!(call.0, Method::HEAD);
                assert_eq!(call.1.to_string(), "/");
                assert!(!call.2.contains_key("authorization"));
            }
        }
        server.abort();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        // A listening socket with no HTTP responder must hit the sample deadline.
        let started = Instant::now();
        let result = ping_endpoint(&endpoint, None).await;
        assert!(!result.success);
        assert_eq!(result.error.as_deref(), Some("timeout"));
        assert!(started.elapsed() >= super::super::connection_probe::SAMPLE_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(6));
        drop(listener);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let invalid_http = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"invalid HTTP\r\n\r\n").await.unwrap();
        });
        let result = ping_endpoint(&endpoint, None).await;
        assert!(!result.success);
        assert_ne!(result.error.as_deref(), Some("timeout"));
        assert!(result.error.is_some());
        invalid_http.await.unwrap();
    }

    #[test]
    fn fingerprint_successful_native_probe_is_reused() {
        let cache = std::sync::Mutex::new(None);
        let calls = std::cell::Cell::new(0);
        for _ in 0..8 {
            let fingerprint = machine_fingerprint_with(
                &cache,
                || {
                    calls.set(calls.get() + 1);
                    Some("machine-guid-fixture".to_string())
                },
                || panic!("native success must not use fallback"),
            );
            assert_eq!(fingerprint, "machine-guid-fixture");
        }
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn fingerprint_concurrent_cold_probes_are_coalesced() {
        let cache = std::sync::Mutex::new(None);
        let barrier = std::sync::Barrier::new(8);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        machine_fingerprint_with(
                            &cache,
                            || {
                                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                Some("machine-guid-fixture".to_string())
                            },
                            || panic!("native success must not use fallback"),
                        )
                    })
                })
                .collect();
            for handle in handles {
                assert_eq!(handle.join().unwrap(), "machine-guid-fixture");
            }
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn fingerprint_native_failure_keeps_fallback_and_allows_recovery() {
        let cache = std::sync::Mutex::new(None);
        let calls = std::cell::Cell::new(0);
        for hostname in ["first-hostname-fixture", "second-hostname-fixture"] {
            assert_eq!(
                machine_fingerprint_with(
                    &cache,
                    || {
                        calls.set(calls.get() + 1);
                        None
                    },
                    || hostname.to_string(),
                ),
                hostname
            );
            assert!(cache.lock().unwrap().is_none());
        }
        assert_eq!(calls.get(), 2);
        assert_eq!(
            machine_fingerprint_with(
                &cache,
                || Some("recovered-guid-fixture".to_string()),
                || panic!("native recovery must not use fallback"),
            ),
            "recovered-guid-fixture"
        );
        assert_eq!(
            machine_fingerprint_with(
                &cache,
                || panic!("recovered fingerprint must be reused"),
                || panic!("native recovery must not use fallback"),
            ),
            "recovered-guid-fixture"
        );
    }

    #[test]
    fn fingerprint_empty_native_value_is_preserved_without_being_cached() {
        let cache = std::sync::Mutex::new(None);
        assert_eq!(
            machine_fingerprint_with(
                &cache,
                || Some(String::new()),
                || panic!("empty native value previously did not use fallback"),
            ),
            ""
        );
        assert!(cache.lock().unwrap().is_none());
        assert_eq!(
            machine_fingerprint_with(
                &cache,
                || Some("recovered-guid-fixture".to_string()),
                || panic!("native recovery must not use fallback"),
            ),
            "recovered-guid-fixture"
        );
    }

    #[test]
    fn fingerprint_cache_preserves_identity_and_key_derivation() {
        let fingerprint = "machine-guid-fixture";
        let username = "user-fixture";
        assert_eq!(
            hex::encode(derive_encryption_key(&[7; 32], fingerprint, username).unwrap()),
            "c3af0f14876eab798f214c4c93f25e8047143d266d84dfa5ae132f42c2411b9b"
        );
        assert_eq!(
            hex::encode(
                derive_encryption_key(&[7; 32], fingerprint, "other-user-fixture").unwrap()
            ),
            "dee1a0f181f4f249e626a564f8bb7e1076bbad5e8f380da67064952f584dc5be"
        );
        assert_eq!(
            hex::encode(derive_encryption_key(&[8; 32], fingerprint, username).unwrap()),
            "eb1ace1edd9e43b35c134f2b56f0cab454408be59292ee8dd0ed267831a399a9"
        );
        assert_eq!(ENCRYPTED_PREFIX, "enc:v1:");
    }

    struct FirstLaunchDirectory(std::path::PathBuf);

    impl FirstLaunchDirectory {
        fn new() -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("echobird-first-launch-{}", uuid::Uuid::new_v4())),
            )
        }
    }

    impl Drop for FirstLaunchDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn first_launch_defaults_are_local_empty_key_router_models() {
        let directory = FirstLaunchDirectory::new();
        let defaults = prepare_initial_router_defaults_at(&directory.0)
            .unwrap()
            .unwrap();
        assert!(!directory.0.exists());
        assert_eq!(defaults.len(), 2);
        assert_eq!(
            defaults
                .iter()
                .map(|model| (
                    model.internal_id.as_str(),
                    model.name.as_str(),
                    model.model_id.as_deref().unwrap(),
                    model.base_url.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "router-default-kilo",
                    "Kilo AI",
                    "kilo-auto/free",
                    "https://api.kilo.ai/api/gateway",
                ),
                (
                    "router-default-zen-free",
                    "OpenCode Zen Free",
                    "big-pickle",
                    "https://opencode.ai/inference/v1",
                ),
            ]
        );
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        for model in defaults {
            assert!(model.api_key.is_empty());
            assert!(model.account_source.is_none());
            assert_eq!(model.scope, crate::models::model::ModelScope::SmartRouter);
            assert!(super::super::free_model_directory::keyless_auth_in_catalog(
                &catalog,
                &model.base_url,
                model.model_id.as_deref().unwrap()
            )
            .is_some());
        }
    }

    #[test]
    fn first_launch_allows_only_port_files_generated_before_router_start() {
        let directory = FirstLaunchDirectory::new();
        let config = directory.0.join("config");
        fs::create_dir_all(&config).unwrap();
        for name in ["anthropic-proxy-port.json", "musecode-proxy-port.json"] {
            fs::write(config.join(name), r#"{"port":12345}"#).unwrap();
        }
        assert!(prepare_initial_router_defaults_at(&directory.0)
            .unwrap()
            .is_some());
        fs::write(config.join(".encryption_key"), "legacy key").unwrap();
        assert!(prepare_initial_router_defaults_at(&directory.0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn existing_or_corrupt_data_never_receives_first_launch_defaults() {
        for name in [
            "settings.json",
            "config/models.json",
            "config/model-order.json",
            "config/smart-router.json",
            "kilo-accounts.json",
            "cache/free-models.json",
        ] {
            let directory = FirstLaunchDirectory::new();
            let path = directory.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "invalid existing content").unwrap();
            assert!(prepare_initial_router_defaults_at(&directory.0)
                .unwrap()
                .is_none());
            assert_eq!(
                fs::read_to_string(path).unwrap(),
                "invalid existing content"
            );
        }
        let directory = FirstLaunchDirectory::new();
        fs::write(&directory.0, "not a directory").unwrap();
        assert!(prepare_initial_router_defaults_at(&directory.0).is_err());
    }

    #[test]
    fn unreadable_or_invalid_preexisting_proxy_state_is_not_a_fresh_installation() {
        let directory = FirstLaunchDirectory::new();
        let config = directory.0.join("config");
        fs::create_dir_all(&config).unwrap();
        let port = config.join("anthropic-proxy-port.json");
        for content in ["invalid", r#"{"port":0}"#, r#"{"port":999999}"#] {
            fs::write(&port, content).unwrap();
            assert!(prepare_initial_router_defaults_at(&directory.0).is_err());
            assert_eq!(fs::read_to_string(&port).unwrap(), content);
        }
    }

    #[test]
    fn invalid_default_catalog_does_not_create_configuration() {
        let directory = FirstLaunchDirectory::new();
        for catalog in [
            serde_json::Value::Null,
            serde_json::json!({ "version": 1, "models": [] }),
        ] {
            assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
            assert!(!directory.0.exists());
        }
        let mut catalog =
            super::super::free_model_directory::bundled_free_model_directory().unwrap();
        catalog["models"][0]["notes"] = serde_json::json!("An API key is required.");
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert!(!directory.0.exists());

        let mut catalog =
            super::super::free_model_directory::bundled_free_model_directory().unwrap();
        let zen = catalog["models"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["modelId"] == "big-pickle")
            .unwrap();
        zen["notes"] = serde_json::json!("An API key is required.");
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert!(!directory.0.exists());
        catalog["models"]
            .as_array_mut()
            .unwrap()
            .retain(|entry| entry["providerId"] != "opencode-zen-free");
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert!(!directory.0.exists());
    }

    #[test]
    fn resumed_default_initialization_keeps_existing_models_and_catalog() {
        let directory = FirstLaunchDirectory::new();
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        let path = directory.0.join("config/models.json");
        let mut models: Vec<ModelConfig> =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        models[0].name = "Kept saved name".to_string();
        models[0].model_id = Some("kept-saved-model".to_string());
        models.insert(0, mk_model("other-user-model"));
        save_updated_models_at(&path, &models).unwrap();
        let saved_models = fs::read(&path).unwrap();
        let cache_path = directory.0.join("cache/free-models.json");
        let saved_catalog = fs::read(&cache_path).unwrap();

        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        assert_eq!(fs::read(&path).unwrap(), saved_models);
        assert_eq!(fs::read(cache_path).unwrap(), saved_catalog);
        let mut files: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        files.sort();
        assert_eq!(files, ["models.json", "models.json.write-lock"]);
    }

    #[test]
    fn deleted_defaults_do_not_return_on_later_launch() {
        let directory = FirstLaunchDirectory::new();
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        let path = directory.0.join("config/models.json");
        save_updated_models_at(&path, &[]).unwrap();
        fs::write(
            directory.0.join("config/smart-router.json"),
            r#"{"candidateIds":[]}"#,
        )
        .unwrap();
        assert!(prepare_initial_router_defaults_at(&directory.0)
            .unwrap()
            .is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
        fs::remove_file(path).unwrap();
        assert!(prepare_initial_router_defaults_at(&directory.0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn failed_default_write_can_resume_without_duplicates() {
        let directory = FirstLaunchDirectory::new();
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        fs::create_dir_all(&directory.0).unwrap();
        fs::write(directory.0.join("config"), "blocking file").unwrap();
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert_eq!(
            fs::read_to_string(directory.0.join("config")).unwrap(),
            "blocking file"
        );
        fs::remove_file(directory.0.join("config")).unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        let models: Vec<ModelConfig> =
            serde_json::from_slice(&fs::read(directory.0.join("config/models.json")).unwrap())
                .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].internal_id, "router-default-kilo");
        assert_eq!(models[1].internal_id, "router-default-zen-free");
    }

    #[test]
    fn pending_default_initialization_preserves_corrupt_user_data() {
        let directory = FirstLaunchDirectory::new();
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        let path = directory.0.join("config/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "invalid saved models").unwrap();
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "invalid saved models");
        assert!(!directory.0.join("cache").exists());
        fs::remove_file(path).unwrap();
        let cache_path = directory.0.join("cache/free-models.json");
        fs::create_dir_all(cache_path.parent().unwrap()).unwrap();
        fs::write(&cache_path, "invalid saved catalog").unwrap();
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert_eq!(
            fs::read_to_string(cache_path).unwrap(),
            "invalid saved catalog"
        );
        assert!(!directory.0.join("config/models.json").exists());
    }

    #[test]
    fn cache_write_failure_preserves_models_and_can_resume_initialization() {
        let directory = FirstLaunchDirectory::new();
        let catalog = super::super::free_model_directory::bundled_free_model_directory().unwrap();
        let path = directory.0.join("config/models.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        save_updated_models_at(&path, &[mk_model("other-user-model")]).unwrap();
        let previous = fs::read(&path).unwrap();
        fs::write(directory.0.join("cache"), "blocking cache file").unwrap();
        assert!(persist_initial_router_defaults_at(&directory.0, &catalog).is_err());
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(
            fs::read_to_string(directory.0.join("cache")).unwrap(),
            "blocking cache file"
        );
        fs::remove_file(directory.0.join("cache")).unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        persist_initial_router_defaults_at(&directory.0, &catalog).unwrap();
        let models: Vec<ModelConfig> = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].internal_id, "other-user-model");
        assert_eq!(models[1].internal_id, "router-default-kilo");
        assert_eq!(models[2].internal_id, "router-default-zen-free");
        assert!(directory.0.join("cache/free-models.json").is_file());
    }

    struct ModelFile(std::path::PathBuf);

    impl ModelFile {
        fn new(models: &[ModelConfig]) -> Self {
            let directory = std::env::temp_dir()
                .join(format!("echobird-model-update-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join("models.json");
            fs::write(&path, serde_json::to_vec_pretty(models).unwrap()).unwrap();
            Self(path)
        }
    }

    impl Drop for ModelFile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.0.parent().unwrap());
        }
    }

    #[test]
    fn model_id_updates_persist_without_changing_other_model_fields_or_order() {
        use crate::models::model::{AccountProvider, AccountSource, ModelScope};

        let mut api_model = new_model_from(
            AddModelInput {
                name: Some("Saved API model".into()),
                base_url: Some("https://api.example/v1".into()),
                anthropic_url: Some("https://api.example/anthropic".into()),
                api_key: Some("enc:v1:saved-credential".into()),
                model_id: Some("old-api-model".into()),
                ..Default::default()
            },
            "api-model".into(),
        );
        api_model.openai_tested = Some(true);
        api_model.openai_latency = Some(123.0);
        let account_model = new_model_from(
            AddModelInput {
                name: Some("Saved account model".into()),
                account_source: Some(AccountSource {
                    provider: AccountProvider::Codex,
                    account_id: "saved-account".into(),
                }),
                model_id: Some("old-account-model".into()),
                scope: ModelScope::SmartRouter,
                ..Default::default()
            },
            "account-model".into(),
        );
        let models = vec![api_model, account_model];
        let fixture = ModelFile::new(&models);
        let mut expected = serde_json::to_value(&models).unwrap();

        for (index, internal_id, model_id) in [
            (0, "api-model", "updated-api-model"),
            (1, "account-model", "updated-account-model"),
        ] {
            let updated = update_model_at(
                &fixture.0,
                internal_id,
                UpdateModelInput {
                    model_id: Some(model_id.into()),
                    ..Default::default()
                },
            )
            .unwrap()
            .unwrap();
            expected[index]["modelId"] = serde_json::json!(model_id);
            assert_eq!(serde_json::to_value(updated).unwrap(), expected[index]);
            let saved: serde_json::Value =
                serde_json::from_slice(&fs::read(&fixture.0).unwrap()).unwrap();
            assert_eq!(saved, expected);
        }
        assert_eq!(
            fs::read_dir(fixture.0.parent().unwrap()).unwrap().count(),
            2
        );
    }

    #[cfg(windows)]
    #[test]
    fn failed_model_update_preserves_saved_bytes_and_cleans_staged_file() {
        use std::os::windows::fs::OpenOptionsExt;

        let fixture = ModelFile::new(&[mk_model("saved")]);
        let previous = fs::read(&fixture.0).unwrap();
        // Allow reads while denying replacement of the saved file.
        let lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&fixture.0)
            .unwrap();
        let update = || UpdateModelInput {
            model_id: Some("new-model".into()),
            ..Default::default()
        };
        let error = update_model_at(&fixture.0, "saved", update()).unwrap_err();
        assert!(error.starts_with("Cannot save configuration"));
        assert_eq!(fs::read(&fixture.0).unwrap(), previous);
        assert_eq!(
            fs::read_dir(fixture.0.parent().unwrap()).unwrap().count(),
            2
        );

        drop(lock);
        let updated = update_model_at(&fixture.0, "saved", update())
            .unwrap()
            .unwrap();
        assert_eq!(updated.model_id.as_deref(), Some("new-model"));
        assert_ne!(fs::read(&fixture.0).unwrap(), previous);
    }

    #[test]
    fn missing_model_update_does_not_rewrite_the_saved_list() {
        let fixture = ModelFile::new(&[mk_model("saved")]);
        let previous = fs::read(&fixture.0).unwrap();
        let updated = update_model_at(
            &fixture.0,
            "missing",
            UpdateModelInput {
                model_id: Some("new-model".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(updated.is_none());
        assert_eq!(fs::read(&fixture.0).unwrap(), previous);
    }

    #[test]
    fn invalid_saved_model_list_is_not_overwritten_by_an_update() {
        let fixture = ModelFile::new(&[]);
        let previous = b"not a model list";
        fs::write(&fixture.0, previous).unwrap();
        let error = update_model_at(
            &fixture.0,
            "saved",
            UpdateModelInput {
                model_id: Some("new-model".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.starts_with("Invalid configuration"));
        assert_eq!(fs::read(&fixture.0).unwrap(), previous);
    }

    #[test]
    fn account_model_add_and_update_discard_api_credentials_and_force_router_scope() {
        use crate::models::model::{AccountProvider, AccountSource, ModelScope};
        let source = AccountSource {
            provider: AccountProvider::Codex,
            account_id: "saved-id".into(),
        };
        let mut model = new_model_from(
            AddModelInput {
                account_source: Some(source.clone()),
                base_url: Some("https://untrusted.example/v1".into()),
                anthropic_url: Some("https://untrusted.example/anthropic".into()),
                api_key: Some("must-never-be-stored".into()),
                scope: ModelScope::ModelCenter,
                model_id: Some("model".into()),
                ..Default::default()
            },
            "fixture".into(),
        );
        assert_eq!(model.scope, ModelScope::SmartRouter);
        assert_eq!(model.base_url, "account://echobird");
        assert!(model.anthropic_url.is_none());
        assert!(model.api_key.is_empty());
        assert_eq!(model.account_source, Some(source.clone()));
        apply_model_updates(
            &mut model,
            UpdateModelInput {
                base_url: Some("https://another.example".into()),
                anthropic_url: Some("https://another.example/anthropic".into()),
                api_key: Some("also-discarded".into()),
                name: Some("New name".into()),
                ..Default::default()
            },
        );
        assert_eq!(model.name, "New name");
        assert_eq!(model.account_source, Some(source));
        assert_eq!(model.base_url, "account://echobird");
        assert!(model.api_key.is_empty());
        assert!(model.anthropic_url.is_none());
        assert_eq!(model.scope, ModelScope::SmartRouter);
    }

    #[test]
    fn converting_api_model_to_account_source_discards_old_api_credentials() {
        use crate::models::model::{AccountProvider, AccountSource, ModelScope};
        let mut model = new_model_from(
            AddModelInput {
                base_url: Some("https://api.example/v1".into()),
                api_key: Some("old-api-key".into()),
                anthropic_url: Some("https://api.example/anthropic".into()),
                model_id: Some("old-model".into()),
                ..Default::default()
            },
            "existing".into(),
        );
        assert_eq!(model.scope, ModelScope::ModelCenter);
        assert_eq!(model.api_key, "old-api-key");
        assert!(model.account_source.is_none());
        apply_model_updates(
            &mut model,
            UpdateModelInput {
                account_source: Some(AccountSource {
                    provider: AccountProvider::WorkBuddy,
                    account_id: "saved-id".into(),
                }),
                model_id: Some("account-model".into()),
                ..Default::default()
            },
        );
        assert!(model.api_key.is_empty());
        assert_eq!(model.base_url, "account://echobird");
        assert!(model.anthropic_url.is_none());
        assert_eq!(model.scope, ModelScope::SmartRouter);
        assert_eq!(model.model_id.as_deref(), Some("account-model"));
    }

    fn mk_model(id: &str) -> ModelConfig {
        ModelConfig {
            internal_id: id.to_string(),
            name: id.to_string(),
            model_id: None,
            base_url: String::new(),
            api_key: String::new(),
            account_source: None,
            anthropic_url: None,
            model_type: None,
            openai_tested: None,
            anthropic_tested: None,
            openai_latency: None,
            anthropic_latency: None,
            scope: crate::models::model::ModelScope::ModelCenter,
        }
    }

    fn ids(models: &[ModelConfig]) -> Vec<String> {
        models.iter().map(|m| m.internal_id.clone()).collect()
    }

    #[test]
    fn apply_user_order_puts_known_ids_first_in_stored_order() {
        let models = vec![mk_model("a"), mk_model("b"), mk_model("c")];
        let order = vec!["c".to_string(), "a".to_string()];
        assert_eq!(ids(&apply_user_order(models, &order)), vec!["c", "a", "b"]);
    }

    #[test]
    fn apply_user_order_empty_order_keeps_default_merge() {
        let models = vec![mk_model("a"), mk_model("b")];
        assert_eq!(ids(&apply_user_order(models, &[])), vec!["a", "b"]);
    }

    #[test]
    fn apply_user_order_new_ids_sink_below_in_default_order() {
        // "b" is absent from the stored order (newly added) → below known "a"
        let models = vec![mk_model("b"), mk_model("a")];
        let order = vec!["a".to_string()];
        assert_eq!(ids(&apply_user_order(models, &order)), vec!["a", "b"]);
    }

    #[test]
    fn apply_user_order_ignores_stale_ids_from_deleted_models() {
        let models = vec![mk_model("a"), mk_model("b")];
        let order = vec!["b".to_string(), "gone".to_string(), "a".to_string()];
        assert_eq!(ids(&apply_user_order(models, &order)), vec!["b", "a"]);
    }

    #[test]
    fn apply_user_order_handles_single_model_and_duplicate_ids() {
        let single = vec![mk_model("a")];
        assert_eq!(
            ids(&apply_user_order(single, &["a".to_string()])),
            vec!["a"]
        );

        let models = vec![mk_model("a"), mk_model("b")];
        let order = vec!["b".to_string(), "b".to_string(), "a".to_string()];
        assert_eq!(ids(&apply_user_order(models, &order)), vec!["b", "a"]);
    }
}
