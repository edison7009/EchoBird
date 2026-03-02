use serde::{Deserialize, Serialize};
use crate::utils::platform::echobird_dir;

// ─── Types ───

/// Cached skills data stored at ~/.echobird/skills.json
/// Skills are raw JSON values from the registry search-index.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsData {
    /// Raw skill entries from search-index.json (kept as Value to avoid mapping every field)
    #[serde(default)]
    pub skills: Vec<serde_json::Value>,
    /// User-defined category keywords for filtering
    #[serde(default, rename = "userCategories")]
    pub user_categories: Vec<String>,
    /// User-added skill source URLs
    #[serde(default)]
    pub sources: Vec<String>,
    /// ISO timestamp of last fetch
    #[serde(default, rename = "lastUpdated")]
    pub last_updated: Option<String>,
}

/// User favorites stored at ~/.echobird/skills_favorites.json
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillsFavorites {
    #[serde(default)]
    pub favorites: Vec<String>,
}

fn skills_path() -> std::path::PathBuf {
    echobird_dir().join("skills.json")
}

fn skills_favorites_path() -> std::path::PathBuf {
    echobird_dir().join("skills_favorites.json")
}

fn skills_i18n_path() -> std::path::PathBuf {
    echobird_dir().join("skills_i18n.json")
}

/// Translation overlay entry for a single skill
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillI18nEntry {
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub d: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub locale: String,
}

/// Translation overlay map: skill_id -> translation entry
pub type SkillsI18nMap = std::collections::HashMap<String, SkillI18nEntry>;

// ─── Commands ───

/// Load skills data from local cache (~/.echobird/skills.json)
#[tauri::command]
pub fn load_skills_data() -> SkillsData {
    let path = skills_path();
    if !path.exists() {
        return SkillsData::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => SkillsData::default(),
    }
}

/// Save skills data to local cache
#[tauri::command]
pub fn save_skills_data(data: SkillsData) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    std::fs::write(skills_path(), json).map_err(|e| e.to_string())
}

/// Load skill favorites from ~/.echobird/skills_favorites.json
#[tauri::command]
pub fn load_skills_favorites() -> SkillsFavorites {
    let path = skills_favorites_path();
    if !path.exists() {
        return SkillsFavorites::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => SkillsFavorites::default(),
    }
}

/// Save skill favorites to ~/.echobird/skills_favorites.json
#[tauri::command]
pub fn save_skills_favorites(data: SkillsFavorites) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    std::fs::write(skills_favorites_path(), json).map_err(|e| e.to_string())
}

/// Load skill translations from ~/.echobird/skills_i18n.json
#[tauri::command]
pub fn load_skills_i18n() -> SkillsI18nMap {
    let path = skills_i18n_path();
    if !path.exists() {
        return SkillsI18nMap::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => SkillsI18nMap::new(),
    }
}

/// Save skill translations to ~/.echobird/skills_i18n.json
#[tauri::command]
pub fn save_skills_i18n(data: SkillsI18nMap) -> Result<(), String> {
    let dir = echobird_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(&data).map_err(|e| e.to_string())?;
    std::fs::write(skills_i18n_path(), json).map_err(|e| e.to_string())
}

/// Fetch content from a remote URL (used by frontend to load skill registry)
#[tauri::command]
pub async fn fetch_skill_source(url: String) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(&url)
        .header("User-Agent", "Echobird/1.1.0")
        .send()
        .await
        .map_err(|e| format!("Failed to fetch {}: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    response
        .text()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))
}

/// Quick non-streaming LLM chat for skill translation/fix.
/// Accepts model config + prompt, returns response text.
#[derive(Debug, Deserialize)]
pub struct LlmQuickConfig {
    pub provider: String,   // "openai" or "anthropic"
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub proxy_url: Option<String>,
}

#[tauri::command]
pub async fn llm_quick_chat(config: LlmQuickConfig, prompt: String) -> Result<String, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120));

    if let Some(ref proxy_url) = config.proxy_url {
        if !proxy_url.is_empty() {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|e| format!("Invalid proxy: {}", e))?;
            builder = builder.proxy(proxy);
        }
    }

    let client = builder.build().map_err(|e| format!("HTTP client error: {}", e))?;
    let api_key = crate::services::model_manager::decrypt_key_for_use(&config.api_key);

    let is_anthropic = config.provider.to_lowercase() == "anthropic";
    let url = if is_anthropic {
        format!("{}/messages", config.base_url.trim_end_matches('/'))
    } else {
        format!("{}/chat/completions", config.base_url.trim_end_matches('/'))
    };

    let body = if is_anthropic {
        serde_json::json!({
            "model": config.model,
            "max_tokens": 4096,
            "messages": [{"role": "user", "content": prompt}]
        })
    } else {
        serde_json::json!({
            "model": config.model,
            "messages": [
                {"role": "system", "content": "You are a helpful translation and content assistant."},
                {"role": "user", "content": prompt}
            ]
        })
    };

    let mut req = client.post(&url)
        .header("Content-Type", "application/json");

    if is_anthropic {
        req = req.header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        req = req.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req.json(&body).send().await
        .map_err(|e| format!("LLM request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("LLM API error {}: {}", status, &body[..body.len().min(200)]));
    }

    let data: serde_json::Value = response.json().await
        .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

    // Extract text from OpenAI or Anthropic format
    let text = if is_anthropic {
        data["content"][0]["text"].as_str().unwrap_or("").to_string()
    } else {
        data["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string()
    };

    Ok(text)
}
