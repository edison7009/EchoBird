// Free-model catalog fetched only after the user requests it.

use serde_json::Value;
use std::path::{Path, PathBuf};

const REMOTE_URL: &str = "https://echobird.ai/api/free-models/index.json";
const BUNDLED_CATALOG: &str = include_str!("../../../docs/api/free-models/index.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeylessAuth {
    Anonymous,
    Public,
}

pub(crate) fn bundled_free_model_directory() -> Result<Value, String> {
    serde_json::from_str::<Value>(BUNDLED_CATALOG)
        .map_err(|error| format!("Failed to parse bundled free models: {error}"))
        .and_then(|catalog| {
            if has_catalog_shape(&catalog) {
                Ok(catalog)
            } else {
                Err("Bundled free model catalog has an invalid shape".to_string())
            }
        })
}

pub fn is_keyless_model(base_url: &str, model_id: &str) -> bool {
    keyless_auth(base_url, model_id).is_some()
}

pub(crate) fn keyless_auth(base_url: &str, model_id: &str) -> Option<KeylessAuth> {
    keyless_endpoint(base_url)?;
    let cached = load_cached_free_model_directory();
    let bundled = bundled_free_model_directory().ok()?;
    keyless_auth_from_catalogs(&cached, &bundled, base_url, model_id)
}

fn keyless_auth_from_catalogs(
    cached: &Value,
    bundled: &Value,
    base_url: &str,
    model_id: &str,
) -> Option<KeylessAuth> {
    let auth = keyless_endpoint(base_url)?;
    // A cached matching entry can revoke anonymous access declared by the bundle.
    if catalog_contains_model(cached, base_url, model_id) {
        return keyless_auth_in_catalog(cached, base_url, model_id);
    }
    if catalog_contains_model(bundled, base_url, model_id) {
        return keyless_auth_in_catalog(bundled, base_url, model_id);
    }
    (auth == KeylessAuth::Public
        && model_id
            .strip_suffix("-free")
            .is_some_and(|name| !name.is_empty()))
    .then_some(auth)
}

fn catalog_contains_model(catalog: &Value, base_url: &str, model_id: &str) -> bool {
    let Some(endpoint) = keyless_endpoint(base_url) else {
        return false;
    };
    catalog["models"].as_array().is_some_and(|models| {
        models.iter().any(|model| {
            model["modelId"].as_str() == Some(model_id)
                && model["baseUrl"].as_str().and_then(keyless_endpoint) == Some(endpoint)
        })
    })
}

pub(crate) fn keyless_auth_in_catalog(
    catalog: &Value,
    base_url: &str,
    model_id: &str,
) -> Option<KeylessAuth> {
    let auth = keyless_endpoint(base_url)?;
    catalog["models"]
        .as_array()?
        .iter()
        .any(|model| {
            model["modelId"].as_str() == Some(model_id)
                && model["baseUrl"].as_str().and_then(keyless_endpoint) == Some(auth)
                && model["notes"]
                    .as_str()
                    .is_some_and(|notes| notes.starts_with("Leave the API key empty."))
        })
        .then_some(auth)
}

pub(crate) fn keyless_endpoint(raw: &str) -> Option<KeylessAuth> {
    let url = url::Url::parse(raw.trim()).ok()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port_or_known_default() != Some(443)
    {
        return None;
    }
    match (url.host_str()?, url.path().trim_end_matches('/')) {
        ("api.kilo.ai", "/api/gateway") => Some(KeylessAuth::Anonymous),
        ("opencode.ai", "/inference/v1") => Some(KeylessAuth::Public),
        _ => None,
    }
}

fn cache_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("cache")
}

pub fn load_cached_free_model_directory() -> Value {
    read_cached_catalog(&cache_dir().join("free-models.json"))
}

pub(crate) fn seed_catalog_cache_at(root: &Path, catalog: &Value) -> Result<(), String> {
    use std::io::Write;

    if !has_catalog_shape(catalog) {
        return Err("Cannot save an invalid free model catalog".to_string());
    }
    let path = root.join("cache").join("free-models.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            return serde_json::from_str::<Value>(&text)
                .ok()
                .filter(has_catalog_shape)
                .map(|_| ())
                .ok_or_else(|| "Saved free model catalog has an invalid shape".to_string());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Failed to read free model cache: {error}")),
    }
    let parent = path.parent().expect("cache path has a parent");
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create free model cache: {error}"))?;
    let content = serde_json::to_vec_pretty(catalog)
        .map_err(|error| format!("Failed to serialize free model cache: {error}"))?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&content)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(|error| format!("Failed to save free model cache: {error}"))
}

fn read_cached_catalog(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(has_catalog_shape)
        .map(without_restricted_free_models)
        .unwrap_or(Value::Null)
}

fn without_restricted_free_models(mut catalog: Value) -> Value {
    if let Some(models) = catalog["models"].as_array_mut() {
        models.retain(|model| model["providerId"] != "opencode-zen");
    }
    catalog
}

pub async fn fetch_free_model_directory() -> Value {
    let cache_dir = cache_dir();
    let cache_path = cache_dir.join("free-models.json");

    if let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    {
        if let Ok(response) = client
            .get(REMOTE_URL)
            .header("User-Agent", "EchoBird/1.1")
            .send()
            .await
        {
            if response.status().is_success() {
                if let Ok(text) = response.text().await {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        if has_catalog_shape(&parsed) {
                            let _ = std::fs::create_dir_all(&cache_dir);
                            let _ = std::fs::write(&cache_path, &text);
                            log::info!("[FreeModels] Loaded from remote");
                            return without_restricted_free_models(parsed);
                        }
                    }
                }
            }
        }
    }

    let cached = read_cached_catalog(&cache_path);
    if !cached.is_null() {
        log::info!("[FreeModels] Loaded from disk cache");
        return cached;
    }

    log::warn!("[FreeModels] Remote catalog and disk cache unavailable");
    Value::Null
}

fn has_catalog_shape(value: &Value) -> bool {
    value.get("version").and_then(Value::as_u64).is_some()
        && value.get("models").is_some_and(Value::is_array)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_public_model_authorization_keeps_endpoint_and_paid_model_boundaries() {
        let empty = serde_json::json!({"version":1,"models":[]});
        for endpoint in [
            "http://opencode.ai/inference/v1",
            "https://opencode.ai.evil.example/inference/v1",
            "https://user@opencode.ai/inference/v1",
            "https://opencode.ai:444/inference/v1",
            "https://opencode.ai/inference/v1/other",
            "https://opencode.ai/inference/v1?key=public",
            "https://opencode.ai/inference/v1#other",
            "https://opencode.ai/zen/v1",
            "https://api.kilo.ai/api/gateway",
            "https://api.example.com/v1",
        ] {
            assert_eq!(
                keyless_auth_from_catalogs(&empty, &empty, endpoint, "cat-jam-free"),
                None,
                "{endpoint}"
            );
        }
        for model in ["claude-opus-5", "cat-jam", "", "-free"] {
            assert_eq!(
                keyless_auth_from_catalogs(
                    &empty,
                    &empty,
                    "https://opencode.ai/inference/v1",
                    model
                ),
                None,
                "{model}"
            );
        }
        assert_eq!(
            keyless_auth_from_catalogs(
                &empty,
                &empty,
                "https://opencode.ai/inference/v1/",
                "cat-jam-free"
            ),
            Some(KeylessAuth::Public)
        );
    }

    #[test]
    fn dynamic_public_model_authorization_preserves_cached_and_bundled_decisions() {
        let empty = serde_json::json!({"version":1,"models":[]});
        let revoked = serde_json::json!({"version":1,"models":[{"baseUrl":"https://opencode.ai/inference/v1", "modelId":"cat-jam-free", "notes":"An API key is now required."}]});
        let allowed = serde_json::json!({"version":1,"models":[{"baseUrl":"https://opencode.ai/inference/v1", "modelId":"cat-jam-free", "notes":"Leave the API key empty. Public access."}]});
        let endpoint = "https://opencode.ai/inference/v1";
        assert_eq!(
            keyless_auth_from_catalogs(&revoked, &allowed, endpoint, "cat-jam-free"),
            None
        );
        assert_eq!(
            keyless_auth_from_catalogs(&allowed, &revoked, endpoint, "cat-jam-free"),
            Some(KeylessAuth::Public)
        );
        assert_eq!(
            keyless_auth_from_catalogs(&empty, &revoked, endpoint, "cat-jam-free"),
            None
        );
        let bundled = bundled_free_model_directory().unwrap();
        assert_eq!(
            keyless_auth_from_catalogs(&empty, &bundled, endpoint, "big-pickle"),
            Some(KeylessAuth::Public)
        );
    }

    #[test]
    fn dynamic_public_model_authorization_does_not_follow_general_free_metadata() {
        let empty = serde_json::json!({"version":1,"models":[]});
        let marked_free = serde_json::json!({"version":1,"models":[{"baseUrl":"https://opencode.ai/inference/v1", "modelId":"paid-model", "free":true,"is_free":true,"pricing":{"prompt":0,"completion":0}}]});
        assert_eq!(
            keyless_auth_from_catalogs(
                &marked_free,
                &empty,
                "https://opencode.ai/inference/v1",
                "paid-model"
            ),
            None
        );
        assert_eq!(
            keyless_auth_from_catalogs(
                &empty,
                &marked_free,
                "https://opencode.ai/inference/v1",
                "paid-model"
            ),
            None
        );
    }

    #[test]
    fn bundled_keyless_entries_have_explicit_authentication_modes() {
        let catalog = bundled_free_model_directory().unwrap();
        let zen_models: Vec<_> = catalog["models"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|model| model["providerId"] == "opencode-zen-free")
            .map(|model| model["modelId"].as_str().unwrap())
            .collect();
        assert_eq!(zen_models, ["big-pickle"]);
        assert_eq!(
            keyless_auth_in_catalog(
                &catalog,
                "https://api.kilo.ai/api/gateway/",
                "kilo-auto/free"
            ),
            Some(KeylessAuth::Anonymous)
        );
        assert_eq!(
            keyless_auth_in_catalog(&catalog, "https://opencode.ai/inference/v1/", "big-pickle"),
            Some(KeylessAuth::Public)
        );
        assert_eq!(
            keyless_auth_in_catalog(&catalog, "https://opencode.ai:443/zen/v1/", "big-pickle"),
            None
        );
        assert_eq!(
            keyless_auth_in_catalog(&catalog, "https://opencode.ai/zen/v1", "claude-opus-5"),
            None
        );
        assert_eq!(
            keyless_auth_in_catalog(&catalog, "https://api.kilo.ai/api/gateway", "made-up/free"),
            None
        );
    }

    #[test]
    fn keyless_auth_rejects_changed_authorities_paths_and_url_credentials() {
        let catalog = bundled_free_model_directory().unwrap();
        for endpoint in [
            "http://api.kilo.ai/api/gateway",
            "https://api.kilo.ai.evil.example/api/gateway",
            "https://user@api.kilo.ai/api/gateway",
            "https://api.kilo.ai:444/api/gateway",
            "https://api.kilo.ai/api/gateway/other",
            "https://api.kilo.ai/api/gateway?other=1",
            "https://api.kilo.ai/api/gateway#other",
            "https://api.kilo.ai/api/%67ateway",
            "https://opencode.ai/zen/v1/other",
            "https://user:password@opencode.ai/zen/v1",
            "https://opencode.ai/zen/v1?key=value",
            "http://127.0.0.1/zen/v1",
            "https://opencode.ai/inference/v1/other",
            "https://user@opencode.ai/inference/v1",
            "https://opencode.ai/inference/v1?key=value",
        ] {
            assert_eq!(
                keyless_auth_in_catalog(&catalog, endpoint, "kilo-auto/free"),
                None
            );
            assert_eq!(
                keyless_auth_in_catalog(&catalog, endpoint, "big-pickle"),
                None
            );
        }
    }

    #[test]
    fn cached_matching_entry_can_revoke_keyless_access() {
        let catalog = serde_json::json!({
            "version": 1,
            "models": [{
                "baseUrl": "https://api.kilo.ai/api/gateway",
                "modelId": "kilo-auto/free",
                "notes": "An API key is now required."
            }]
        });
        assert!(catalog_contains_model(
            &catalog,
            "https://api.kilo.ai/api/gateway/",
            "kilo-auto/free"
        ));
        assert_eq!(
            keyless_auth_in_catalog(
                &catalog,
                "https://api.kilo.ai/api/gateway",
                "kilo-auto/free"
            ),
            None
        );
        assert!(!catalog_contains_model(
            &Value::Null,
            "https://api.kilo.ai/api/gateway",
            "kilo-auto/free"
        ));
    }

    struct CacheFile(PathBuf);

    impl CacheFile {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "echobird-free-models-{}.json",
                uuid::Uuid::new_v4()
            )))
        }
    }

    impl Drop for CacheFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn restores_catalog_from_disk_without_changing_it() {
        let cache = CacheFile::new();
        let catalog = serde_json::json!({
            "version": 1,
            "updatedAt": "2026-10-12",
            "models": [{ "id": "saved-free-model" }]
        });
        let text = serde_json::to_string(&catalog).unwrap();
        std::fs::write(&cache.0, &text).unwrap();

        assert_eq!(read_cached_catalog(&cache.0), catalog);
        assert_eq!(std::fs::read_to_string(&cache.0).unwrap(), text);
    }

    #[test]
    fn old_cached_zen_free_model_is_hidden_without_rewriting_cache() {
        let cache = CacheFile::new();
        let catalog = serde_json::json!({
            "version": 1,
            "models": [
                {"id": "kilo:kilo-auto/free", "providerId": "kilo"},
                {"id": "opencode-zen:space-bunny-free", "providerId": "opencode-zen"}
            ]
        });
        let text = serde_json::to_string(&catalog).unwrap();
        std::fs::write(&cache.0, &text).unwrap();

        let loaded = read_cached_catalog(&cache.0);
        assert_eq!(loaded["models"].as_array().unwrap().len(), 1);
        assert_eq!(loaded["models"][0]["providerId"], "kilo");
        assert_eq!(std::fs::read_to_string(&cache.0).unwrap(), text);
    }

    #[test]
    fn missing_cache_returns_null_without_creating_a_file() {
        let cache = CacheFile::new();
        assert!(read_cached_catalog(&cache.0).is_null());
        assert!(!cache.0.exists());
    }

    #[test]
    fn corrupt_cache_returns_null() {
        let cache = CacheFile::new();
        std::fs::write(&cache.0, "{invalid-json}").unwrap();
        assert!(read_cached_catalog(&cache.0).is_null());
    }

    #[test]
    fn wrong_shape_cache_returns_null() {
        let cache = CacheFile::new();
        std::fs::write(&cache.0, r#"{"version":1,"models":{}}"#).unwrap();
        assert!(read_cached_catalog(&cache.0).is_null());
    }

    #[test]
    fn accepts_versioned_catalog() {
        assert!(has_catalog_shape(&serde_json::json!({
            "version": 1,
            "models": []
        })));
    }

    #[test]
    fn rejects_missing_or_wrong_fields() {
        assert!(!has_catalog_shape(&serde_json::json!({ "models": [] })));
        assert!(!has_catalog_shape(&serde_json::json!({
            "version": 1,
            "models": {}
        })));
    }
}
