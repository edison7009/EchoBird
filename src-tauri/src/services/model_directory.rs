// Model directory: serve the right-panel Providers + Relays list to the
// Model Center page from a remote JSON so we can update vendors without
// shipping an app version.
//
// Mirrors `local_llm::model_store::fetch_store_models` line for line —
// same three-tier flow:
//
//   1. Remote (https://echobird.ai/api/model-directory/index.json),
//      8-second timeout. On success: cache to disk + return.
//   2. Disk cache (~/.echobird/cache/model-directory.json) — keeps the
//      page populated when the user is offline / behind a firewall.
//   3. Return null. Frontend has a bundled fallback (src/data/
//      modelDirectory.json) which still satisfies the panel from the
//      app binary, so the page never goes blank.
//
// The two contracts on the JSON shape:
//   - top-level must have `providers: [...]` and `relays: [...]`
//   - each entry has { name, url, baseUrl, anthropicUrl, modelId, region }
// We only check the shape at this layer (presence of the two arrays);
// per-entry validation lives in the frontend so a stray bad row doesn't
// blank out the whole panel.

use serde_json::Value;

/// Fetch the Model Center directory: remote → cache → null.
///
/// `null` is the sentinel for "neither remote nor cache had a usable
/// directory" — the frontend's `getModelDirectory()` wrapper translates
/// that into "keep using the bundled JSON."
pub async fn fetch_model_directory() -> Value {
    // Previewing unpublished directory edits: skip remote and cache so the
    // frontend keeps the bundled JSON from this build.
    if std::env::var_os("ECHOBIRD_BUNDLED_MODEL_DIRECTORY").is_some() {
        log::info!("[ModelDirectory] Using bundled directory (ECHOBIRD_BUNDLED_MODEL_DIRECTORY)");
        return Value::Null;
    }
    let remote_url = "https://echobird.ai/api/model-directory/index.json";
    let cache_dir = dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache");
    let cache_path = cache_dir.join("model-directory.json");

    // 1. Try remote.
    if let Ok(resp) = super::http_transport::catalog()
        .get(remote_url)
        .header("User-Agent", "Echobird/1.1")
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(text) = resp.text().await {
                if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                    if has_directory_shape(&parsed) {
                        let _ = std::fs::create_dir_all(&cache_dir);
                        let _ = std::fs::write(&cache_path, &text);
                        log::info!("[ModelDirectory] Loaded from remote");
                        return parsed;
                    }
                }
            }
        }
    }

    // 2. Try cache.
    if let Ok(text) = std::fs::read_to_string(&cache_path) {
        if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
            if has_directory_shape(&parsed) {
                log::info!("[ModelDirectory] Loaded from disk cache");
                return parsed;
            }
        }
    }

    // 3. Tell the frontend to fall back to its bundled JSON.
    log::warn!("[ModelDirectory] Remote + cache miss; signalling frontend bundled fallback");
    Value::Null
}

/// Both `providers` and `relays` must be arrays for the shape to be
/// considered valid. Per-entry validation is the frontend's job.
fn has_directory_shape(v: &Value) -> bool {
    v.get("providers").map(|p| p.is_array()).unwrap_or(false)
        && v.get("relays").map(|r| r.is_array()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_accepts_minimal_valid_directory() {
        let v = serde_json::json!({
            "providers": [],
            "relays": [],
        });
        assert!(has_directory_shape(&v));
    }

    #[test]
    fn shape_accepts_directory_with_entries() {
        let v = serde_json::json!({
            "providers": [{ "name": "OpenAI", "url": "https://openai.com" }],
            "relays": [{ "name": "cc-vibe", "url": "https://cc-vibe.com" }],
        });
        assert!(has_directory_shape(&v));
    }

    #[test]
    fn shape_rejects_missing_providers_or_relays() {
        let only_providers = serde_json::json!({ "providers": [] });
        let only_relays = serde_json::json!({ "relays": [] });
        let neither = serde_json::json!({});
        assert!(!has_directory_shape(&only_providers));
        assert!(!has_directory_shape(&only_relays));
        assert!(!has_directory_shape(&neither));
    }

    #[test]
    fn shape_rejects_wrong_type() {
        // providers as object, not array.
        let bad = serde_json::json!({
            "providers": { "name": "x" },
            "relays": [],
        });
        assert!(!has_directory_shape(&bad));

        // relays as string.
        let bad2 = serde_json::json!({
            "providers": [],
            "relays": "not an array",
        });
        assert!(!has_directory_shape(&bad2));
    }
}
