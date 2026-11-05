use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use url::Url;

use super::model_manager;

const INVALID_URL: &str = "UPSTREAM_MODELS_INVALID_URL";
const INVALID_KEY: &str = "UPSTREAM_MODELS_KEY_INVALID";
const UNSUPPORTED: &str = "UPSTREAM_MODELS_UNSUPPORTED";
const REQUEST_FAILED: &str = "UPSTREAM_MODELS_REQUEST_FAILED";
const INVALID_RESPONSE: &str = "UPSTREAM_MODELS_INVALID_RESPONSE";
const MAX_BODY: usize = 4 * 1024 * 1024;
const MAX_PAGES: usize = 20;
const MAX_MODELS: usize = 20_000;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInput {
    pub base_url: String,
    pub anthropic_url: String,
    pub api_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub free: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCatalog {
    pub models: Vec<UpstreamModel>,
    pub requires_api_key: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Protocol {
    OpenAi,
    Anthropic,
    Google,
}

struct Endpoint {
    url: Url,
    protocol: Protocol,
}

fn cache() -> &'static Mutex<HashMap<[u8; 32], ModelCatalog>> {
    static CACHE: OnceLock<Mutex<HashMap<[u8; 32], ModelCatalog>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalized_url(raw: &str) -> Result<Option<Url>, String> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    let mut url = Url::parse(raw.trim()).map_err(|_| INVALID_URL.to_string())?;
    if !matches!(url.scheme(), "https" | "http")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(INVALID_URL.into());
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    Ok(Some(url))
}

fn usable_key(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    if raw == "not-needed" {
        return Ok(String::new());
    }
    if let Some(hex) = raw.strip_prefix("enc:v1:") {
        if hex.len() < 56 || hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(INVALID_KEY.into());
        }
        // Decryption must not regenerate a missing or damaged local key file.
        let key_path = crate::utils::platform::echobird_dir()
            .join("config")
            .join(".encryption_key");
        if std::fs::read(key_path)
            .ok()
            .is_none_or(|key| key.len() != 32)
        {
            return Err(INVALID_KEY.into());
        }
    }
    let key = model_manager::decrypt_key_for_use(raw);
    if (raw.starts_with("enc:v1:") && key.trim().is_empty())
        || reqwest::header::HeaderValue::from_str(&key).is_err()
    {
        return Err(INVALID_KEY.into());
    }
    Ok(key.trim().to_string())
}

struct Source {
    base: Option<Url>,
    anthropic: Option<Url>,
    key: String,
    fingerprint: [u8; 32],
}

fn source(input: &CatalogInput) -> Result<Source, String> {
    let base = normalized_url(&input.base_url)?;
    let anthropic = normalized_url(&input.anthropic_url)?;
    if base.is_none() && anthropic.is_none() {
        return Err(INVALID_URL.into());
    }
    let key = usable_key(&input.api_key)?;
    let mut hash = Sha256::new();
    for part in [
        base.as_ref().map_or("", Url::as_str),
        anthropic.as_ref().map_or("", Url::as_str),
        key.as_str(),
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    Ok(Source {
        base,
        anthropic,
        key,
        fingerprint: hash.finalize().into(),
    })
}

pub fn get_cached(input: CatalogInput) -> Result<Option<ModelCatalog>, String> {
    let fingerprint = source(&input)?.fingerprint;
    Ok(cache()
        .lock()
        .map_err(|_| REQUEST_FAILED.to_string())?
        .get(&fingerprint)
        .cloned())
}

fn unsupported_plan(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default();
    let path = url.path();
    ((host.ends_with(".volces.com") || host.ends_with(".bytepluses.com"))
        && path.starts_with("/api/coding"))
        || (host == "qianfan.baidubce.com"
            && (path.starts_with("/v2/coding") || path.starts_with("/anthropic/coding")))
}

fn needs_key(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some("api.openai.com" | "api.anthropic.com" | "generativelanguage.googleapis.com")
    )
}

fn with_path(base: &Url, path: &str) -> Url {
    let mut url = base.clone();
    url.set_path(path);
    url
}

fn endpoints(base: &Url, anthropic: bool) -> Vec<Endpoint> {
    let path = base.path().trim_end_matches('/');
    let protocol = if anthropic || base.host_str() == Some("api.anthropic.com") {
        Protocol::Anthropic
    } else {
        Protocol::OpenAi
    };
    let mut result = Vec::new();
    if base.host_str() == Some("generativelanguage.googleapis.com")
        || path.ends_with("/v1beta/openai")
    {
        let native =
            path.strip_suffix("/openai")
                .unwrap_or(if path.is_empty() { "/v1beta" } else { path });
        result.push(Endpoint {
            url: with_path(base, &format!("{native}/models")),
            protocol: Protocol::Google,
        });
    }
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    let versioned = last_segment
        .strip_prefix('v')
        .is_some_and(|version| !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit()));
    let first_path = if path.ends_with("/models") {
        path.to_string()
    } else if versioned {
        format!("{path}/models")
    } else {
        format!("{path}/v1/models")
    };
    result.push(Endpoint {
        url: with_path(base, &first_path),
        protocol,
    });
    if !versioned && !path.ends_with("/models") {
        result.push(Endpoint {
            url: with_path(base, &format!("{path}/models")),
            protocol,
        });
        for suffix in [
            "/api/claudecode",
            "/api/anthropic",
            "/apps/anthropic",
            "/api/coding",
            "/claudecode",
            "/anthropic",
            "/step_plan",
            "/coding",
            "/claude",
        ] {
            if let Some(prefix) = path.strip_suffix(suffix) {
                result.push(Endpoint {
                    url: with_path(base, &format!("{prefix}/v1/models")),
                    protocol,
                });
                break;
            }
        }
    }
    result
}

enum FetchResult {
    Catalog(ModelCatalog),
    NotFound,
}

pub async fn fetch(input: CatalogInput) -> Result<ModelCatalog, String> {
    let Source {
        base,
        anthropic,
        key,
        fingerprint,
    } = source(&input)?;
    let primary = base.as_ref().or(anthropic.as_ref()).ok_or(INVALID_URL)?;
    if unsupported_plan(primary) {
        return Err(UNSUPPORTED.into());
    }
    if key.is_empty() && needs_key(primary) {
        return Ok(ModelCatalog {
            models: Vec::new(),
            requires_api_key: true,
        });
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(12))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| REQUEST_FAILED.to_string())?;
    let mut candidates = endpoints(primary, base.is_none());
    if base.is_some() {
        if let Some(anthropic) = anthropic.as_ref().filter(|url| !unsupported_plan(url)) {
            candidates.extend(endpoints(anthropic, true));
        }
    }
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut tried = HashSet::new();
        for endpoint in candidates {
            if !tried.insert((endpoint.url.to_string(), endpoint.protocol)) {
                continue;
            }
            match fetch_endpoint(&client, &endpoint, &key).await? {
                FetchResult::NotFound => continue,
                FetchResult::Catalog(catalog) => return Ok(catalog),
            }
        }
        Err(UNSUPPORTED.to_string())
    })
    .await
    .map_err(|_| REQUEST_FAILED.to_string())??;
    // A missing-key prompt is not a successfully retrieved directory.
    if !result.models.is_empty() || !result.requires_api_key {
        let mut cache = cache().lock().map_err(|_| REQUEST_FAILED.to_string())?;
        if cache.len() >= 64 && !cache.contains_key(&fingerprint) {
            if let Some(previous) = cache.keys().next().copied() {
                cache.remove(&previous);
            }
        }
        cache.insert(fingerprint, result.clone());
    }
    Ok(result)
}

async fn fetch_endpoint(
    client: &Client,
    endpoint: &Endpoint,
    key: &str,
) -> Result<FetchResult, String> {
    let mut models = BTreeMap::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    for page in 0..MAX_PAGES {
        if page == 0 && key.is_empty() && needs_key(&endpoint.url) {
            return Ok(FetchResult::Catalog(ModelCatalog {
                models: Vec::new(),
                requires_api_key: true,
            }));
        }
        let mut request = client.get(endpoint.url.clone());
        if !key.is_empty() {
            request = match endpoint.protocol {
                Protocol::OpenAi => request.bearer_auth(key),
                Protocol::Anthropic => request.header("x-api-key", key),
                Protocol::Google => request.header("x-goog-api-key", key),
            };
        }
        if endpoint.protocol == Protocol::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        if let Some(cursor) = cursor.as_ref() {
            let field = if endpoint.protocol == Protocol::Google {
                "pageToken"
            } else {
                "after_id"
            };
            request = request.query(&[(field, cursor)]);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| REQUEST_FAILED.to_string())?;
        let status = response.status();
        if page == 0
            && matches!(
                status,
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            )
        {
            return Ok(FetchResult::NotFound);
        }
        if page == 0
            && key.is_empty()
            && matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        {
            return Ok(FetchResult::Catalog(ModelCatalog {
                models: Vec::new(),
                requires_api_key: true,
            }));
        }
        if !status.is_success() {
            return Err(format!("UPSTREAM_MODELS_HTTP_{}", status.as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_BODY as u64)
        {
            return Err(INVALID_RESPONSE.into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| REQUEST_FAILED.to_string())?
        {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(INVALID_RESPONSE.into());
            }
            body.extend_from_slice(&chunk);
        }
        let json: Value =
            serde_json::from_slice(&body).map_err(|_| INVALID_RESPONSE.to_string())?;
        for model in parse_models(&json, endpoint.protocol)? {
            models.entry(model.id.clone()).or_insert(model);
        }
        if models.len() > MAX_MODELS {
            return Err(INVALID_RESPONSE.into());
        }
        cursor = next_cursor(&json, endpoint.protocol)?;
        match cursor.as_ref() {
            None => {
                return Ok(FetchResult::Catalog(ModelCatalog {
                    models: models.into_values().collect(),
                    requires_api_key: needs_key(&endpoint.url),
                }));
            }
            Some(cursor) if cursor.len() > 2048 || !seen_cursors.insert(cursor.clone()) => {
                return Err(INVALID_RESPONSE.into());
            }
            Some(_) => {}
        }
    }
    Err(INVALID_RESPONSE.into())
}

fn parse_models(json: &Value, protocol: Protocol) -> Result<Vec<UpstreamModel>, String> {
    let rows = json["data"]
        .as_array()
        .or_else(|| json["models"].as_array())
        .or_else(|| json.as_array())
        .ok_or(INVALID_RESPONSE)?;
    let mut models = Vec::new();
    for row in rows {
        if protocol == Protocol::Google
            && row["supportedGenerationMethods"]
                .as_array()
                .is_some_and(|methods| !methods.iter().any(|method| method == "generateContent"))
        {
            continue;
        }
        let id = if protocol == Protocol::Google {
            row["name"]
                .as_str()
                .map(|id| id.strip_prefix("models/").unwrap_or(id))
        } else {
            row["id"].as_str().or_else(|| row["slug"].as_str())
        };
        let Some(id) = id.filter(|id| !id.trim().is_empty() && id.len() <= 512) else {
            return Err(INVALID_RESPONSE.into());
        };
        let flags = [
            row["free"].as_bool(),
            row["is_free"].as_bool(),
            row["isFree"].as_bool(),
        ];
        let free = if flags.contains(&Some(false)) {
            Some(false)
        } else if flags.contains(&Some(true)) {
            Some(true)
        } else {
            pricing_free(&row["pricing"])
        };
        let multiplier = ["multiplier", "price_factor", "priceFactor"]
            .iter()
            .find_map(|field| {
                finite_number(&row[*field]).or_else(|| finite_number(&row["pricing"][*field]))
            });
        models.push(UpstreamModel {
            id: id.to_string(),
            free,
            multiplier,
        });
    }
    Ok(models)
}

fn finite_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn pricing_free(pricing: &Value) -> Option<bool> {
    let pricing = pricing.as_object()?;
    let prompt = finite_number(pricing.get("prompt")?)?;
    let completion = finite_number(pricing.get("completion")?)?;
    if prompt != 0.0 || completion != 0.0 {
        return Some(false);
    }
    for (field, value) in pricing {
        if matches!(
            field.as_str(),
            "multiplier" | "price_factor" | "priceFactor"
        ) {
            continue;
        }
        if finite_number(value)? != 0.0 {
            return Some(false);
        }
    }
    Some(true)
}

fn next_cursor(json: &Value, protocol: Protocol) -> Result<Option<String>, String> {
    if protocol == Protocol::Google {
        return match json.get("nextPageToken") {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(cursor)) if cursor.is_empty() => Ok(None),
            Some(Value::String(cursor)) => Ok(Some(cursor.clone())),
            Some(_) => Err(INVALID_RESPONSE.into()),
        };
    }
    if json["has_more"].as_bool() != Some(true) {
        return Ok(None);
    }
    json["last_id"]
        .as_str()
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| Some(cursor.to_string()))
        .ok_or_else(|| INVALID_RESPONSE.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Request;
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;
    use axum::Router;
    use serde_json::json;
    use std::sync::Arc;

    struct Fixture {
        url: String,
        requests: Arc<Mutex<Vec<(String, HeaderMap)>>>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn fixture(responses: Vec<(StatusCode, Value)>) -> Fixture {
        fixture_with_headers(responses, HeaderMap::new()).await
    }

    async fn fixture_with_headers(
        responses: Vec<(StatusCode, Value)>,
        headers: HeaderMap,
    ) -> Fixture {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let responses = Arc::new(Mutex::new(responses.into_iter()));
        let app = Router::new().fallback(move |request: Request| {
            let requests = recorded.clone();
            let responses = responses.clone();
            let headers = headers.clone();
            async move {
                requests
                    .lock()
                    .unwrap()
                    .push((request.uri().to_string(), request.headers().clone()));
                let (status, body) = responses
                    .lock()
                    .unwrap()
                    .next()
                    .unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, Value::Null));
                (status, headers, axum::Json(body)).into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Fixture {
            url,
            requests,
            task,
        }
    }

    fn input(url: &str, key: &str) -> CatalogInput {
        CatalogInput {
            base_url: url.into(),
            anthropic_url: String::new(),
            api_key: key.into(),
        }
    }

    #[test]
    fn prices_and_flags_never_infer_unknown_costs_or_convert_currency_to_multipliers() {
        let models = parse_models(&json!({"data": [
            {"id": "Free-Zero", "pricing": {"prompt": "0", "completion": "0", "request": "0"}},
            {"id": "false-wins", "free": false, "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "input-only", "pricing": {"prompt": "0"}},
            {"id": "paid-request", "pricing": {"prompt": "0", "completion": "0", "request": ".001"}},
            {"id": "unknown-charge", "pricing": {"prompt": "0", "completion": "0", "image": "unknown"}},
            {"id": "Explicit", "is_free": true, "multiplier": "0.0001"},
            {"id": "invalid-ratio", "multiplier": "NaN"},
            {"id": "negative-ratio", "price_factor": -2},
            {"id": "relative", "price_factor": 1.6}
        ]}), Protocol::OpenAi).unwrap();
        assert_eq!(models[0].free, Some(true));
        assert_eq!(models[1].free, Some(false));
        assert_eq!(models[2].free, None);
        assert_eq!(models[3].free, Some(false));
        assert_eq!(models[4].free, None);
        assert_eq!(models[5].free, Some(true));
        assert_eq!(models[5].multiplier, Some(0.0001));
        assert_eq!(models[6].multiplier, None);
        assert_eq!(models[7].multiplier, None);
        assert_eq!(models[8].multiplier, Some(1.6));
        assert_eq!(models[0].multiplier, None);
        assert!(parse_models(&json!({"error": "missing list"}), Protocol::OpenAi).is_err());
        assert!(parse_models(&json!({"data": [{"id": ""}]}), Protocol::OpenAi).is_err());
    }

    #[test]
    fn url_validation_and_candidates_preserve_configured_versions_and_origins() {
        for url in [
            "file:///tmp/models",
            "https://user:key@example.com/v1",
            "https://example.com/v1?key=secret",
        ] {
            assert_eq!(normalized_url(url).unwrap_err(), INVALID_URL);
        }
        for (base, expected) in [
            ("https://example.com/", "/v1/models"),
            (
                "https://example.com/api/coding/paas/v4",
                "/api/coding/paas/v4/models",
            ),
            (
                "https://example.com/compatibility/v1/",
                "/compatibility/v1/models",
            ),
            ("https://example.com/openai", "/openai/v1/models"),
            ("https://example.com/zen/go/v1", "/zen/go/v1/models"),
        ] {
            let base = normalized_url(base).unwrap().unwrap();
            let candidates = endpoints(&base, false);
            assert_eq!(candidates[0].url.path(), expected);
            assert!(candidates
                .iter()
                .all(|endpoint| endpoint.url.origin() == base.origin()));
            if base.path().ends_with("/v4") {
                assert_eq!(candidates.len(), 1);
            }
        }
    }

    #[tokio::test]
    async fn anonymous_catalog_cache_is_local_and_key_isolated() {
        let server = fixture(vec![(
            StatusCode::OK,
            json!({"data": [
                {"id": "Model-B"}, {"id": "Model-A", "isFree": true}, {"id": "Model-B"}
            ]}),
        )])
        .await;
        let configured = input(&server.url, "not-needed");
        assert!(get_cached(configured.clone()).unwrap().is_none());
        assert!(server.requests.lock().unwrap().is_empty());
        let catalog = fetch(configured.clone()).await.unwrap();
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["Model-A", "Model-B"]
        );
        assert_eq!(get_cached(input(&server.url, "")).unwrap(), Some(catalog));
        assert!(get_cached(input(&server.url, "other-key"))
            .unwrap()
            .is_none());
        let recorded = server.requests.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, "/v1/models");
        assert!(recorded[0].1.get("authorization").is_none());
    }

    #[tokio::test]
    async fn key_requirement_does_not_misreport_auth_failure_or_try_other_endpoints() {
        let anonymous = fixture(vec![(
            StatusCode::UNAUTHORIZED,
            json!({"error": "not printed"}),
        )])
        .await;
        let catalog = fetch(input(&anonymous.url, "")).await.unwrap();
        assert!(catalog.requires_api_key);
        assert!(catalog.models.is_empty());
        assert!(get_cached(input(&anonymous.url, "")).unwrap().is_none());
        assert_eq!(anonymous.requests.lock().unwrap().len(), 1);

        let authenticated = fixture(vec![(
            StatusCode::UNAUTHORIZED,
            json!({"error": "fixture-key"}),
        )])
        .await;
        let mut configured = input(&authenticated.url, "fixture-key");
        configured.anthropic_url = anonymous.url.clone();
        assert_eq!(
            fetch(configured).await.unwrap_err(),
            "UPSTREAM_MODELS_HTTP_401"
        );
        let recorded = authenticated.requests.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].1["authorization"], "Bearer fixture-key");
        assert_eq!(anonymous.requests.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn missing_endpoint_falls_back_but_failed_refresh_preserves_cache() {
        let server = fixture(vec![
            (StatusCode::NOT_FOUND, Value::Null),
            (StatusCode::OK, json!({"data": [{"id": "Keep-Me"}]})),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"secret": "never exposed"}),
            ),
        ])
        .await;
        let configured = input(&server.url, "key");
        let original = fetch(configured.clone()).await.unwrap();
        assert_eq!(server.requests.lock().unwrap()[1].0, "/models");
        assert_eq!(
            fetch(configured.clone()).await.unwrap_err(),
            "UPSTREAM_MODELS_HTTP_500"
        );
        assert_eq!(get_cached(configured).unwrap(), Some(original));
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn anthropic_pages_use_api_key_and_cursor_and_keep_exact_model_ids() {
        let server = fixture(vec![
            (
                StatusCode::OK,
                json!({"data": [{"id": "Claude-A"}], "has_more": true, "last_id": "Claude-A"}),
            ),
            (
                StatusCode::OK,
                json!({"data": [{"id": "Claude-B"}, {"id": "Claude-A"}], "has_more": false}),
            ),
        ])
        .await;
        let configured = CatalogInput {
            base_url: String::new(),
            anthropic_url: server.url.clone(),
            api_key: "secret".into(),
        };
        let catalog = fetch(configured).await.unwrap();
        assert_eq!(catalog.models.len(), 2);
        assert_eq!(catalog.models[0].id, "Claude-A");
        let recorded = server.requests.lock().unwrap();
        assert_eq!(recorded[1].0, "/v1/models?after_id=Claude-A");
        for (_, headers) in recorded.iter() {
            assert_eq!(headers["x-api-key"], "secret");
            assert_eq!(headers["anthropic-version"], "2023-06-01");
            assert!(headers.get("authorization").is_none());
        }
    }

    #[tokio::test]
    async fn google_pages_use_native_same_host_endpoint_and_filter_non_chat_models() {
        let server = fixture(vec![
            (
                StatusCode::OK,
                json!({"models": [
                {"name": "models/Gemini-A", "supportedGenerationMethods": ["generateContent"]},
                {"name": "models/Embedding", "supportedGenerationMethods": ["embedContent"]}
            ], "nextPageToken": "next/page"}),
            ),
            (
                StatusCode::OK,
                json!({"models": [{"name": "models/Gemini-B"}]}),
            ),
        ])
        .await;
        let catalog = fetch(input(
            &format!("{}/v1beta/openai", server.url),
            "google-key",
        ))
        .await
        .unwrap();
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["Gemini-A", "Gemini-B"]
        );
        let recorded = server.requests.lock().unwrap();
        assert_eq!(recorded[0].0, "/v1beta/models");
        assert_eq!(recorded[1].0, "/v1beta/models?pageToken=next%2Fpage");
        assert_eq!(recorded[0].1["x-goog-api-key"], "google-key");
        assert!(recorded[0].1.get("authorization").is_none());
    }

    #[tokio::test]
    async fn incomplete_pagination_never_overwrites_a_successful_cache() {
        let server = fixture(vec![
            (StatusCode::OK, json!({"data": [{"id": "Original"}]})),
            (
                StatusCode::OK,
                json!({"data": [{"id": "Partial"}], "has_more": true, "last_id": "Partial"}),
            ),
            (StatusCode::FORBIDDEN, Value::Null),
        ])
        .await;
        let configured = input(&server.url, "key");
        let original = fetch(configured.clone()).await.unwrap();
        assert_eq!(
            fetch(configured.clone()).await.unwrap_err(),
            "UPSTREAM_MODELS_HTTP_403"
        );
        assert_eq!(get_cached(configured).unwrap(), Some(original));
    }

    #[tokio::test]
    async fn redirects_never_forward_credentials_and_bad_encryption_never_becomes_anonymous() {
        let sink = fixture(vec![(StatusCode::OK, json!({"data": []}))]).await;
        let mut headers = HeaderMap::new();
        headers.insert("location", sink.url.parse().unwrap());
        let source = fixture_with_headers(vec![(StatusCode::FOUND, Value::Null)], headers).await;
        assert_eq!(
            fetch(input(&source.url, "private-key")).await.unwrap_err(),
            "UPSTREAM_MODELS_HTTP_302"
        );
        assert_eq!(source.requests.lock().unwrap().len(), 1);
        assert!(sink.requests.lock().unwrap().is_empty());
        for invalid in ["enc:v1:f", "enc:v1:中文", "enc:v1:zzzz", "bad\nheader"] {
            assert_eq!(
                fetch(input(&sink.url, invalid)).await.unwrap_err(),
                INVALID_KEY
            );
        }
        let corrupted = format!("enc:v1:{}", "00".repeat(28));
        assert_eq!(
            fetch(input(&sink.url, &corrupted)).await.unwrap_err(),
            INVALID_KEY
        );
        assert!(sink.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mandatory_keys_and_known_unavailable_coding_catalogs_return_before_network() {
        for url in [
            "https://api.openai.com/v1",
            "https://api.anthropic.com",
            "https://generativelanguage.googleapis.com/v1beta/openai",
        ] {
            let catalog = fetch(input(url, "")).await.unwrap();
            assert!(catalog.requires_api_key);
            assert!(catalog.models.is_empty());
        }
        for url in [
            "https://ark.cn-beijing.volces.com/api/coding/v3",
            "https://ark.ap-southeast.bytepluses.com/api/coding/v3",
            "https://qianfan.baidubce.com/v2/coding",
        ] {
            assert_eq!(fetch(input(url, "key")).await.unwrap_err(), UNSUPPORTED);
        }
    }

    #[tokio::test]
    async fn malformed_pagination_and_oversized_responses_are_rejected() {
        let google = fixture(vec![(
            StatusCode::OK,
            json!({"models": [{"name": "models/Partial"}], "nextPageToken": 7}),
        )])
        .await;
        let configured = input(&format!("{}/v1beta/openai", google.url), "key");
        assert_eq!(
            fetch(configured.clone()).await.unwrap_err(),
            INVALID_RESPONSE
        );
        assert!(get_cached(configured).unwrap().is_none());
        let pages = fixture(vec![
            (
                StatusCode::OK,
                json!({"data": [{"id": "A"}], "has_more": true, "last_id": "A"}),
            ),
            (
                StatusCode::OK,
                json!({"data": [{"id": "A"}], "has_more": true, "last_id": "A"}),
            ),
        ])
        .await;
        assert_eq!(
            fetch(input(&pages.url, "key")).await.unwrap_err(),
            INVALID_RESPONSE
        );
        assert_eq!(pages.requests.lock().unwrap().len(), 2);
        let oversized = fixture(vec![(
            StatusCode::OK,
            json!({"data": [], "padding": "x".repeat(MAX_BODY)}),
        )])
        .await;
        assert_eq!(
            fetch(input(&oversized.url, "key")).await.unwrap_err(),
            INVALID_RESPONSE
        );
    }

    #[tokio::test]
    #[ignore = "requires public upstream model directory access"]
    async fn public_upstream_catalogs_live_positive_control() {
        let openrouter = fetch(input("https://openrouter.ai/api/v1", "not-needed"))
            .await
            .expect("public model directory is reachable");
        assert!(!openrouter.models.is_empty());
        assert!(openrouter
            .models
            .iter()
            .any(|model| model.free == Some(true)));
        let zen = fetch(input("https://opencode.ai/zen/v1", "not-needed"))
            .await
            .expect("public model directory is reachable");
        assert!(!zen.models.is_empty());
        assert!(!zen.requires_api_key);
    }
}
