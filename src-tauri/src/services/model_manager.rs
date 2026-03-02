// Model Manager �?mirrors old modelManager.ts
// Handles model CRUD, API testing, ping, and API key encryption

use std::fs;
use std::time::Instant;

use crate::models::model::{ModelConfig, ModelType, PingResult, TestResult};
use crate::utils::platform::echobird_dir;

// ─── Config paths ───

/// Models config file: ~/.echobird/config/models.json
fn models_config_path() -> std::path::PathBuf {
    echobird_dir().join("config").join("models.json")
}

/// Ensure ~/.echobird/config/ exists
fn ensure_config_dir() {
    let config_dir = echobird_dir().join("config");
    let _ = fs::create_dir_all(&config_dir);
}

// ─── API Key encryption (AES-256-GCM) ───

const ENCRYPTED_PREFIX: &str = "enc:v1:";

/// Check if a key is already encrypted
fn is_encrypted(key: &str) -> bool {
    key.starts_with(ENCRYPTED_PREFIX)
}

/// Get stable machine fingerprint (does NOT change on app upgrade/reinstall)
/// Windows: HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid
/// macOS: IOPlatformUUID via ioreg
/// Linux: /etc/machine-id
fn get_machine_fingerprint() -> String {
    #[cfg(windows)]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("reg")
            .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            // Output format: "    MachineGuid    REG_SZ    <guid>"
            if let Some(line) = text.lines().find(|l| l.contains("MachineGuid")) {
                if let Some(guid) = line.split_whitespace().last() {
                    return guid.to_string();
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;
        if let Ok(output) = Command::new("ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(3) {
                        return uuid.to_string();
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(id) = std::fs::read_to_string("/etc/machine-id") {
            let trimmed = id.trim().to_string();
            if !trimmed.is_empty() {
                return trimmed;
            }
        }
    }

    // Fallback: hostname via env var
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "echobird-fallback".to_string())
}

/// Get or create the persistent encryption key, derived with machine fingerprint.
/// Actual AES key = HMAC-SHA256(random_file_key, machine_fingerprint)
/// This ensures: same machine + same file = works; different machine = self-destruct.
fn get_encryption_key() -> Result<[u8; 32], String> {
    use hmac::Mac;
    type HmacSha256 = hmac::Hmac<sha2::Sha256>;

    let key_path = echobird_dir().join("config").join(".encryption_key");

    let file_key = if key_path.exists() {
        let data = fs::read(&key_path)
            .map_err(|e| format!("Failed to read encryption key: {}", e))?;
        if data.len() == 32 {
            data
        } else {
            log::warn!("[ModelManager] Invalid encryption key file, regenerating");
            generate_key_file(&key_path)?
        }
    } else {
        generate_key_file(&key_path)?
    };

    // Derive actual key: HMAC-SHA256(file_key, machine_fingerprint + username)
    let fingerprint = get_machine_fingerprint();
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "default".to_string());
    let identity = format!("{}|{}", fingerprint, username);

    let mut mac = <HmacSha256 as Mac>::new_from_slice(&file_key)
        .map_err(|e| format!("HMAC init error: {}", e))?;
    hmac::Mac::update(&mut mac, identity.as_bytes());
    let result = mac.finalize().into_bytes();

    let mut derived_key = [0u8; 32];
    derived_key.copy_from_slice(&result);
    Ok(derived_key)
}

fn generate_key_file(key_path: &std::path::Path) -> Result<Vec<u8>, String> {
    use rand::RngCore;
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);

    ensure_config_dir();
    fs::write(key_path, &key)
        .map_err(|e| format!("Failed to write encryption key: {}", e))?;
    log::info!("[ModelManager] Generated new encryption key");
    Ok(key)
}

/// Encrypt API key using AES-256-GCM.
/// Stores "enc:v1:<hex(nonce + ciphertext)>" in models.json.
fn encrypt_api_key(plain_key: &str) -> String {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use rand::RngCore;

    if plain_key.is_empty() || is_encrypted(plain_key) || plain_key == "local" {
        return plain_key.to_string();
    }

    let key_bytes = match get_encryption_key() {
        Ok(k) => k,
        Err(e) => {
            log::error!("[ModelManager] Encryption key error: {}", e);
            return plain_key.to_string();
        }
    };

    let cipher = Aes256Gcm::new((&key_bytes).into());

    // Generate random 12-byte nonce
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    match cipher.encrypt(nonce, plain_key.as_bytes()) {
        Ok(ciphertext) => {
            // Concatenate nonce + ciphertext, encode as hex
            let mut combined = Vec::with_capacity(12 + ciphertext.len());
            combined.extend_from_slice(&nonce_bytes);
            combined.extend_from_slice(&ciphertext);
            let hex = combined.iter().map(|b| format!("{:02x}", b)).collect::<String>();
            log::info!("[ModelManager] API key encrypted successfully");
            format!("{}{}", ENCRYPTED_PREFIX, hex)
        }
        Err(e) => {
            log::error!("[ModelManager] AES encryption failed: {}", e);
            plain_key.to_string()
        }
    }
}

/// Decrypt API key using AES-256-GCM.
fn decrypt_api_key(stored_key: &str) -> String {
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};

    if stored_key.is_empty() || !is_encrypted(stored_key) {
        return stored_key.to_string();
    }

    let hex_data = &stored_key[ENCRYPTED_PREFIX.len()..];

    // Decode hex to bytes
    let combined: Vec<u8> = match (0..hex_data.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_data[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
    {
        Ok(bytes) => bytes,
        Err(e) => {
            log::error!("[ModelManager] Hex decode failed: {}", e);
            return String::new();
        }
    };

    if combined.len() < 13 {
        log::error!("[ModelManager] Encrypted data too short");
        return String::new();
    }

    let key_bytes = match get_encryption_key() {
        Ok(k) => k,
        Err(e) => {
            log::error!("[ModelManager] Encryption key error: {}", e);
            return String::new();
        }
    };

    let cipher = Aes256Gcm::new((&key_bytes).into());
    let nonce = aes_gcm::Nonce::from_slice(&combined[..12]);
    let ciphertext = &combined[12..];

    match cipher.decrypt(nonce, ciphertext) {
        Ok(plaintext) => String::from_utf8(plaintext).unwrap_or_else(|_| {
            log::error!("[ModelManager] Decrypted key is not valid UTF-8");
            String::new()
        }),
        Err(e) => {
            log::error!("[ModelManager] AES decryption failed: {}", e);
            String::new()
        }
    }
}

/// Get usable plaintext API key (auto-decrypts if needed)
pub fn decrypt_key_for_use(api_key: &str) -> String {
    decrypt_api_key(api_key)
}

/// Encrypt a key/password for storage (public, reused by SSH)
pub fn encrypt_key_for_storage(plain_key: &str) -> String {
    encrypt_api_key(plain_key)
}

// ─── Model CRUD ───

/// Read raw user models from disk
fn get_raw_user_models() -> Vec<ModelConfig> {
    let path = models_config_path();
    if !path.exists() {
        return Vec::new();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(e) => {
            log::error!("[ModelManager] Failed to load user models: {}", e);
            Vec::new()
        }
    }
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
                    log::info!("[ModelManager] Loaded {} built-in models from {:?}", models.len(), path);
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
    let user_models = get_user_models();
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
            anthropic_url: Some(format!("http://127.0.0.1:{}/anthropic", server_info.port)),
            model_type: Some(crate::models::model::ModelType::Local),
            proxy_url: None,
            ss_node: None,
            openai_tested: None,
            anthropic_tested: None,
            openai_latency: None,
            anthropic_latency: None,
        }]
    } else {
        Vec::new()
    };

    // Order: local �?user �?built-in
    let mut all = Vec::new();
    all.extend(local_models);
    all.extend(user_models);
    all.extend(built_in_models);
    all
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

/// Save user models to disk
fn save_user_models(models: &[ModelConfig]) {
    ensure_config_dir();
    let path = models_config_path();
    let content = serde_json::to_string_pretty(models).unwrap_or_default();
    if let Err(e) = fs::write(&path, content) {
        log::error!("[ModelManager] Failed to save models: {}", e);
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
    pub model_id: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub ss_node: Option<crate::models::model::SSNode>,
}

pub fn add_model(input: AddModelInput) -> ModelConfig {
    let mut models = get_user_models();

    let base_url = input.base_url.unwrap_or_default();
    let auto_type = detect_model_type(&base_url);
    let internal_id = generate_internal_id();

    let new_model = ModelConfig {
        internal_id: internal_id.clone(),
        name: input.name.unwrap_or_default(),
        model_id: input.model_id,
        base_url,
        api_key: input.api_key.unwrap_or_default(),
        anthropic_url: input.anthropic_url,
        model_type: auto_type,
        proxy_url: input.proxy_url,
        ss_node: input.ss_node,
        openai_tested: None,
        anthropic_tested: None,
        openai_latency: None,
        anthropic_latency: None,
    };

    log::info!("[ModelManager] Added model: {} ({})", new_model.name, internal_id);
    models.push(new_model.clone());
    save_user_models(&models);
    new_model
}

/// Delete a user model by internal ID
pub fn delete_model(internal_id: &str) -> bool {
    let mut models = get_user_models();
    let original_len = models.len();
    models.retain(|m| m.internal_id != internal_id);

    if models.len() == original_len {
        return false; // Not found
    }

    save_user_models(&models);
    log::info!("[ModelManager] Deleted model: {}", internal_id);
    true
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
    pub model_id: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub ss_node: Option<crate::models::model::SSNode>,
    #[serde(default)]
    pub openai_tested: Option<bool>,
    #[serde(default)]
    pub anthropic_tested: Option<bool>,
    #[serde(default)]
    pub openai_latency: Option<f64>,
    #[serde(default)]
    pub anthropic_latency: Option<f64>,
}

pub fn update_model(internal_id: &str, updates: UpdateModelInput) -> Option<ModelConfig> {
    let mut models = get_user_models();
    let index = models.iter().position(|m| m.internal_id == internal_id)?;

    // Apply updates
    if let Some(name) = updates.name {
        models[index].name = name;
    }
    if let Some(url) = &updates.base_url {
        models[index].base_url = url.clone();
        models[index].model_type = detect_model_type(url);
    }
    if let Some(url) = updates.anthropic_url {
        models[index].anthropic_url = Some(url);
    }
    if let Some(key) = updates.api_key {
        models[index].api_key = key;
    }
    if let Some(id) = updates.model_id {
        models[index].model_id = Some(id);
    }
    if let Some(url) = updates.proxy_url {
        models[index].proxy_url = Some(url);
    }
    if let Some(node) = updates.ss_node {
        models[index].ss_node = Some(node);
    }
    if let Some(v) = updates.openai_tested {
        models[index].openai_tested = Some(v);
    }
    if let Some(v) = updates.anthropic_tested {
        models[index].anthropic_tested = Some(v);
    }
    if let Some(v) = updates.openai_latency {
        models[index].openai_latency = Some(v);
    }
    if let Some(v) = updates.anthropic_latency {
        models[index].anthropic_latency = Some(v);
    }

    let updated = models[index].clone();
    save_user_models(&models);
    log::info!("[ModelManager] Updated model: {}", internal_id);
    Some(updated)
}

// ─── Test & Ping ───

/// Build HTTP client (optionally with proxy)
fn build_client(_model: &ModelConfig) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30));

    // TODO: integrate with ssProxyServer for TUNNEL mode
    if let Some(proxy_url) = &_model.proxy_url {
        if let Ok(proxy) = reqwest::Proxy::http(proxy_url) {
            builder = builder.proxy(proxy);
        }
    }

    builder.build().unwrap_or_default()
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
                    let content = data["content"][0]["text"]
                        .as_str()
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

                    log::info!("[ModelManager] Anthropic test succeeded: {} {}ms", model.name, latency);
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

                    log::info!("[ModelManager] OpenAI test succeeded: {} {}ms", model.name, latency);
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
pub async fn ping_model(internal_id: &str) -> PingResult {
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

    // Extract base domain
    let base_url = match url::Url::parse(url) {
        Ok(parsed) => format!("{}://{}", parsed.scheme(), parsed.host_str().unwrap_or("")),
        Err(_) => url.to_string(),
    };

    let client = build_client(&model);
    let start = Instant::now();

    match client
        .head(&base_url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(_) => {
            let latency = start.elapsed().as_millis() as f64;
            let _ = update_model(
                internal_id,
                UpdateModelInput {
                    openai_latency: Some(latency),
                    ..Default::default()
                },
            );
            log::info!("[ModelManager] Ping succeeded: {} {}ms", model.name, latency);
            PingResult {
                success: true,
                latency,
                url: base_url,
                error: None,
            }
        }
        Err(e) => PingResult {
            success: false,
            latency: -1.0,
            url: base_url,
            error: Some(e.to_string()),
        },
    }
}

// ─── API Key encryption toggle ───

/// Toggle encryption state for a model's API key
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToggleEncryptionResult {
    pub success: bool,
    pub api_key: String,
    pub encrypted: bool,
}

pub fn toggle_key_encryption(internal_id: &str) -> ToggleEncryptionResult {
    let mut models = get_user_models();
    let index = match models.iter().position(|m| m.internal_id == internal_id) {
        Some(i) => i,
        None => {
            return ToggleEncryptionResult {
                success: false,
                api_key: String::new(),
                encrypted: false,
            };
        }
    };

    let current_key = &models[index].api_key;
    if current_key.is_empty() || current_key == "local" {
        return ToggleEncryptionResult {
            success: false,
            api_key: current_key.clone(),
            encrypted: false,
        };
    }

    let (new_key, encrypted) = if is_encrypted(current_key) {
        // Unlock: decrypt AES ciphertext back to plaintext
        let decrypted = decrypt_api_key(current_key);
        if decrypted.is_empty() {
            log::error!("[ModelManager] Decryption failed, clearing key");
        }
        (decrypted, false)
    } else {
        // Lock: encrypt plaintext with AES-256-GCM
        let enc = encrypt_api_key(current_key);
        let is_enc = is_encrypted(&enc);
        (enc, is_enc)
    };

    models[index].api_key = new_key.clone();
    save_user_models(&models);
    log::info!(
        "[ModelManager] Key encryption toggled for: {} -> {}",
        models[index].name,
        if encrypted { "encrypted" } else { "plaintext" }
    );

    ToggleEncryptionResult {
        success: true,
        api_key: new_key,
        encrypted,
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
