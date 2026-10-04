//! Model configuration for workbuddy.

use super::{extract_domain_name, write_json_file, ApplyResult, ModelInfo};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

// ════════════════════════════════════════════════════════════════
//  WorkBuddy (Tencent CodeBuddy 办公版) → ~/.workbuddy/models.json
//  WorkBuddy AI → ~/.workbuddy-ai/models.json
//  Desktop UI shape: [{ id, name, vendor, url, apiKey, ... }].
//  Also read the legacy { models: [...], availableModels: [...] } shape
//  documented by DeepSeek for WorkBuddy/CodeBuddy.
//
//  Notes:
//   • Write to each desktop edition's own directory. The documented
//     ~/.codebuddy compatibility path is not the desktop UI's native file.
//   • Standard models use the full /chat/completions endpoint; existing
//     custom-protocol models retain their exact endpoint.
//   • File must be UTF-8 WITHOUT BOM — some desktop builds fail to parse a
//     BOM'd models.json. write_json_file writes raw UTF-8 (no BOM), so this
//     is satisfied for free.
//   • Preserve other models and existing capability/reasoning settings.
//     Registering a model does not select it in the client's conversation.
// ════════════════════════════════════════════════════════════════

fn config_path(home: &Path, tool_id: &str) -> PathBuf {
    home.join(if tool_id == "workbuddyai" {
        ".workbuddy-ai"
    } else {
        ".workbuddy"
    })
    .join("models.json")
}

fn completion_url(base_url: &str) -> String {
    let url = base_url.trim_end_matches('/');
    if url.ends_with("/chat/completions") {
        url.to_string()
    } else {
        format!("{url}/chat/completions")
    }
}

fn read_models(path: &Path) -> Result<Vec<Value>, String> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("Failed to read {}: {e}", path.display())),
    };
    let content = content.trim_start_matches('\u{feff}').trim();
    if content.is_empty() {
        return Ok(Vec::new());
    }
    let mut config: Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid WorkBuddy models.json: {e}"))?;
    if config.is_object() {
        config = config["models"].take();
    }
    match config {
        Value::Array(models) if models.iter().all(Value::is_object) => Ok(models),
        _ => Err("Invalid WorkBuddy models.json: expected a model array".to_string()),
    }
}

pub(super) fn apply_workbuddy(tool_id: &str, model_info: &ModelInfo) -> ApplyResult {
    let Some(home) = dirs::home_dir() else {
        return ApplyResult {
            success: false,
            message: "Cannot find the home directory".to_string(),
        };
    };
    apply_workbuddy_at(&config_path(&home, tool_id), model_info)
}

fn apply_workbuddy_at(config_path: &Path, model_info: &ModelInfo) -> ApplyResult {
    let model_id = model_info
        .model
        .as_deref()
        .or(model_info.name.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or("echobird-model");
    let display_name = model_info
        .name
        .as_deref()
        .or(model_info.model.as_deref())
        .filter(|s| !s.is_empty())
        .unwrap_or(model_id);

    let base_url = model_info.base_url.as_deref().unwrap_or("");
    let api_key = model_info.api_key.as_deref().unwrap_or("");
    if base_url.is_empty() || api_key.is_empty() {
        return ApplyResult {
            success: false,
            message: "WorkBuddy needs both a base URL and an API key — pick a model first."
                .to_string(),
        };
    }

    let url = completion_url(base_url);

    let mut models = match read_models(config_path) {
        Ok(models) => models,
        Err(message) => {
            return ApplyResult {
                success: false,
                message,
            }
        }
    };
    // The same model ID can be served by different providers. Only update
    // the matching endpoint, leaving the other provider's entry untouched.
    let existing = models.iter().position(|m| {
        m["id"].as_str() == Some(model_id)
            && m["url"].as_str().is_some_and(|existing_url| {
                if m["useCustomProtocol"] == true {
                    existing_url == base_url || existing_url == url
                } else {
                    completion_url(existing_url) == url
                }
            })
    });
    let mut model = existing.map(|i| models.remove(i)).unwrap_or_else(|| {
        json!({
            "id": model_id,
            "vendor": extract_domain_name(base_url),
            "maxInputTokens": 200000,
            "maxOutputTokens": 8192,
            "supportsToolCall": true,
            "supportsImages": true,
        })
    });
    model["name"] = json!(display_name);
    if model["useCustomProtocol"] != true {
        model["url"] = json!(url);
    }
    model["apiKey"] = json!(api_key);
    // EchoBird reads the first configured model; this is not the client's
    // active conversation selection.
    models.insert(0, model);

    match write_json_file(config_path, &json!(models)) {
        Ok(_) => ApplyResult {
            success: true,
            message: format!(
                "Model \"{}\" saved to WorkBuddy configuration. Fully quit and reopen WorkBuddy, then select it in the model picker.",
                display_name
            ),
        },
        Err(e) => ApplyResult {
            success: false,
            message: format!("WorkBuddy error: {}", e),
        },
    }
}

pub(super) fn read_workbuddy(tool_id: &str) -> Option<ModelInfo> {
    read_workbuddy_at(&config_path(&dirs::home_dir()?, tool_id))
}

fn read_workbuddy_at(path: &Path) -> Option<ModelInfo> {
    let models = read_models(path).ok()?;
    let m = models.first()?;
    let model = m.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if model.is_empty() {
        return None;
    }
    let url = m.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let base_url = if m["useCustomProtocol"] == true {
        url.to_string()
    } else {
        url.trim_end_matches('/')
            .trim_end_matches("/chat/completions")
            .trim_end_matches('/')
            .to_string()
    };
    Some(ModelInfo {
        name: m.get("name").and_then(|v| v.as_str()).map(String::from),
        model: Some(model.to_string()),
        base_url: if base_url.is_empty() {
            None
        } else {
            Some(base_url)
        },
        api_key: m.get("apiKey").and_then(|v| v.as_str()).map(String::from),
        anthropic_url: None,
        protocol: None,
        display_model: None,
        relay_mode: None,
        web_search: None,
        one_m_context: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("workbuddy-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self, tool_id: &str) -> PathBuf {
            config_path(&self.0, tool_id)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn model() -> ModelInfo {
        serde_json::from_value(json!({
            "model": "test-model",
            "name": "Test model",
            "baseUrl": "https://example.test/v1/",
            "apiKey": "test-key"
        }))
        .unwrap()
    }

    #[test]
    fn writes_native_arrays_and_keeps_editions_separate() {
        let fixture = Fixture::new();
        let cn = fixture.path("workbuddy");
        let ai = fixture.path("workbuddyai");
        assert_eq!(cn, fixture.0.join(".workbuddy/models.json"));
        assert_eq!(ai, fixture.0.join(".workbuddy-ai/models.json"));
        for path in [&cn, &ai] {
            assert!(apply_workbuddy_at(path, &model()).success);
            let bytes = std::fs::read(path).unwrap();
            assert_eq!(bytes[0], b'[');
            let entries = read_models(path).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(
                entries[0]["url"],
                "https://example.test/v1/chat/completions"
            );
            let actual = read_workbuddy_at(path).unwrap();
            assert_eq!(actual.model, model().model);
            assert_eq!(actual.api_key, model().api_key);
            assert_eq!(actual.base_url.as_deref(), Some("https://example.test/v1"));
        }
        let cn_bytes = std::fs::read(&cn).unwrap();
        let mut changed = model();
        changed.api_key = Some("ai-only-key".into());
        assert!(apply_workbuddy_at(&ai, &changed).success);
        assert_eq!(std::fs::read(&cn).unwrap(), cn_bytes);
    }

    #[test]
    fn updates_matching_model_preserving_other_providers_and_reasoning() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        let other = json!({
            "id": "test-model", "url": "https://other.test/v1/chat/completions",
            "apiKey": "other-key", "name": "Other provider"
        });
        let existing = json!({
            "id": "test-model", "url": "https://example.test/v1/chat/completions",
            "apiKey": "old-key", "name": "Old name", "vendor": "Custom",
            "supportsImages": false, "supportsToolCall": true,
            "supportsReasoning": true, "onlyReasoning": false,
            "reasoning": {"effort": "high"}, "useCustomProtocol": false,
            "maxInputTokens": 128000, "maxOutputTokens": 16384
        });
        write_json_file(&path, &json!([other, existing])).unwrap();
        let mut info = model();
        info.base_url = Some("https://example.test/v1/chat/completions".into());
        assert!(apply_workbuddy_at(&path, &info).success);
        assert!(apply_workbuddy_at(&path, &info).success);
        let entries = read_models(&path).unwrap();
        let mut expected = existing;
        expected["name"] = json!("Test model");
        expected["apiKey"] = json!("test-key");
        assert_eq!(entries, vec![expected, other]);
    }

    #[test]
    fn reads_legacy_models_and_migrates_without_losing_entries() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        let legacy = json!({"id": "legacy-model", "apiKey": "legacy-key"});
        write_json_file(
            &path,
            &json!({
                "models": [legacy], "availableModels": ["legacy-model"]
            }),
        )
        .unwrap();
        assert_eq!(
            read_workbuddy_at(&path).unwrap().model.as_deref(),
            Some("legacy-model")
        );
        assert!(apply_workbuddy_at(&path, &model()).success);
        assert_eq!(std::fs::read(&path).unwrap()[0], b'[');
        assert_eq!(read_models(&path).unwrap()[1], legacy);
    }

    #[test]
    fn custom_protocol_endpoints_round_trip_without_losing_settings() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        for url in [
            "https://example.test/gateway/invoke/",
            "https://example.test/v1/chat/completions",
        ] {
            let existing = json!({
                "id": "test-model", "url": url, "apiKey": "old-key",
                "useCustomProtocol": true, "supportsReasoning": true,
                "maxInputTokens": 262144, "maxOutputTokens": 65536
            });
            write_json_file(&path, &json!([existing])).unwrap();
            let mut info = read_workbuddy_at(&path).unwrap();
            assert_eq!(info.base_url.as_deref(), Some(url));
            info.api_key = Some("new-key".into());
            assert!(apply_workbuddy_at(&path, &info).success);
            let mut expected = existing;
            expected["name"] = json!("test-model");
            expected["apiKey"] = json!("new-key");
            assert_eq!(read_models(&path).unwrap(), vec![expected]);
        }
    }

    #[test]
    fn standard_base_urls_match_the_completed_endpoint() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        write_json_file(
            &path,
            &json!([{
                "id": "test-model", "url": "https://example.test/v1/",
                "maxOutputTokens": 65536
            }]),
        )
        .unwrap();
        assert!(apply_workbuddy_at(&path, &model()).success);
        let models = read_models(&path).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["maxOutputTokens"], 65536);
        assert_eq!(models[0]["url"], "https://example.test/v1/chat/completions");
    }

    #[test]
    fn accepts_empty_files_and_utf8_bom() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddyai");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for content in ["", " \n", "\u{feff}[]"] {
            std::fs::write(&path, content).unwrap();
            assert!(apply_workbuddy_at(&path, &model()).success);
            assert_eq!(std::fs::read(&path).unwrap()[0], b'[');
        }
    }

    #[test]
    fn malformed_or_unknown_configs_are_not_overwritten() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for content in ["{broken", "{}", "null", "[null]", r#"{"models":{}}"#] {
            std::fs::write(&path, content).unwrap();
            assert!(!apply_workbuddy_at(&path, &model()).success);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        }
    }

    #[test]
    fn missing_credentials_leave_existing_config_untouched() {
        let fixture = Fixture::new();
        let path = fixture.path("workbuddy");
        write_json_file(&path, &json!([])).unwrap();
        for info in [
            ModelInfo {
                api_key: None,
                ..model()
            },
            ModelInfo {
                base_url: None,
                ..model()
            },
        ] {
            assert!(!apply_workbuddy_at(&path, &info).success);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "[]");
        }
    }
}
