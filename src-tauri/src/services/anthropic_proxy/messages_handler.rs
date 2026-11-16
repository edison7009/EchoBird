use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use bytes::Bytes;
use serde_json::{json, Value};
use std::path::PathBuf;

use super::AppState;

// Anthropic API version pinned by spec; Anthropic's docs say "the latest
// version is automatically used if the header is missing on most
// compatible providers" but it's safer to send an explicit value.
// 2023-06-01 is the long-standing stable header that every compatible
// upstream we ship in modelDirectory.json accepts.
const ANTHROPIC_VERSION: &str = "2023-06-01";

const RELAY_FILENAME: &str = "claudedesktop.json";
const RELAY_FILENAME_CLAUDECODE: &str = "claudecode.json";
const RELAY_FILENAME_CLAUDESCIENCE: &str = "claudescience.json";

struct Relay {
    base_url: String,
    api_key: String,
    web_search: Option<bool>,
    model_name: Option<String>,
    /// Upstream's actual model id (e.g. "mimo-v2.5-pro"). When present
    /// we rewrite Claude Desktop's request model field to this. When
    /// absent the request goes through unchanged — works fine for
    /// smart upstreams that auto-route claude-* names.
    real_model_id: Option<String>,
}

/// Claude Desktop route — POST /v1/messages. Relay: claudedesktop.json.
pub async fn handle_messages(state: State<AppState>, body: Bytes) -> Response {
    handle_messages_with(state, body, RELAY_FILENAME).await
}

/// Claude Code route — POST /claudecode/v1/messages. Relay: claudecode.json.
/// A dedicated relay file (not claudedesktop.json) keeps Claude Code and
/// Claude Desktop independent — the user can point each at a different
/// upstream without one clobbering the other, even though both speak the
/// same Anthropic Messages API and would otherwise collide on /v1/messages.
pub async fn handle_messages_claudecode(state: State<AppState>, body: Bytes) -> Response {
    handle_messages_with(state, body, RELAY_FILENAME_CLAUDECODE).await
}

pub async fn handle_messages_claudescience(state: State<AppState>, body: Bytes) -> Response {
    handle_messages_with(state, body, RELAY_FILENAME_CLAUDESCIENCE).await
}

pub async fn handle_models_claudescience() -> Response {
    let Some(relay) = read_relay(RELAY_FILENAME_CLAUDESCIENCE) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    science_models_response(&relay)
}

fn science_models_response(relay: &Relay) -> Response {
    let Some(id) = relay.real_model_id.as_deref() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let models = match crate::services::smart_router::catalog::models_for_url(&relay.base_url) {
        Ok(models) => models,
        Err(error) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":{"message":error}})),
            )
                .into_response()
        }
    };
    let mut data = vec![json!({
        "type": "model",
        "id": id,
        "display_name": relay.model_name.as_deref().unwrap_or(id),
        "created_at": "1970-01-01T00:00:00Z"
    })];
    if !models.is_empty() {
        data = models
            .into_iter()
            .map(|model| {
                json!({"type":"model","id":model.id,
            "display_name":model.name,"created_at":"1970-01-01T00:00:00Z"})
            })
            .collect();
    }
    Json(json!({
        "data": data,
        "first_id": id,
        "last_id": data.last().map(|model| &model["id"]),
        "has_more": false
    }))
    .into_response()
}

async fn handle_messages_with(
    State(state): State<AppState>,
    body: Bytes,
    relay_filename: &str,
) -> Response {
    let mut req_body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "message": format!("Invalid JSON body: {}", e),
                    }
                })),
            )
                .into_response();
        }
    };

    let relay = match read_relay(relay_filename) {
        Some(r) => r,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "service_unavailable",
                        "message": "No active model configured in EchoBird for this Claude app. Open EchoBird and apply a model.",
                    }
                })),
            )
                .into_response();
        }
    };

    let deepseek = url::Url::parse(&relay.base_url)
        .is_ok_and(|url| url.host_str() == Some("api.deepseek.com"))
        || relay
            .real_model_id
            .as_deref()
            .is_some_and(|model| model.starts_with("deepseek-"));
    if deepseek {
        if let Err(message) = filter_deepseek_tools(&mut req_body) {
            return (StatusCode::BAD_REQUEST, Json(json!({"type":"error","error":{"type":"invalid_request_error","message":message}}))).into_response();
        }
    }

    if relay.web_search == Some(false) {
        if let Some(tools) = req_body.get_mut("tools").and_then(Value::as_array_mut) {
            tools.retain(|tool| {
                tool["name"] != "WebSearch"
                    && !tool["type"]
                        .as_str()
                        .is_some_and(|kind| kind.starts_with("web_search"))
            });
        }
    }

    // Model id rewrite — Claude Desktop hard-codes claude-sonnet-4-* /
    // claude-opus-* / claude-haiku-4-*. Strict upstreams need the
    // real id. Smart upstreams ignore this and use their own.
    if let Some(real) = relay.real_model_id.as_deref() {
        let current = req_body.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let router_pick = matches!(
            relay_filename,
            RELAY_FILENAME | RELAY_FILENAME_CLAUDESCIENCE
        ) && crate::services::smart_router::is_router_url(&relay.base_url)
            && current.starts_with(crate::services::smart_router::catalog::SELECTOR_PREFIX);
        if !real.is_empty() && real != current && !router_pick {
            log::info!("[AnthropicProxy] Model ID rewrite: {} → {}", current, real);
            req_body["model"] = Value::String(real.to_string());
        }
    }

    // Downstream id hygiene, last step of the conversion: the `[1m]` logical-
    // variant suffix is Claude-client vocabulary and must NEVER reach a
    // third-party upstream. It can still show up here two ways — Claude
    // Code / Desktop sending their built-in `claude-*[1m]` ids straight
    // through (no actualModel in the relay file), or a relay file written by
    // ≤v5.3.8 whose actualModel carries the suffix. Strip it unconditionally.
    let stripped = req_body
        .get("model")
        .and_then(|v| v.as_str())
        .and_then(|m| m.strip_suffix("[1m]"))
        .map(str::to_string);
    if let Some(bare) = stripped {
        log::info!("[AnthropicProxy] Stripped [1m] variant suffix → {}", bare);
        req_body["model"] = Value::String(bare);
    }

    let model = req_body["model"].as_str().unwrap_or_default();
    let router_upstream = crate::services::smart_router::is_router_url(&relay.base_url);
    if !super::billing_line::keeps_billing_line(model, router_upstream) {
        super::billing_line::strip_from_anthropic(&mut req_body);
    }

    // URL composition matches llm_client.rs:404 — handle every shape
    // users (and our model directory templates) realistically put in
    // the relay's anthropic URL:
    //   1. already includes "/messages"     → use as-is
    //   2. ends with "/v1"                   → append "/messages"
    //      (avoid `/v1/v1/messages` doubling for vLLM/sglang/OpenAI-
    //      style baseURLs that include /v1 — issue #108)
    //   3. otherwise                          → append "/v1/messages"
    //      (canonical Anthropic-spec path)
    let base = relay.base_url.trim_end_matches('/');
    let upstream_url = if base.contains("/messages") {
        base.to_string()
    } else if base.ends_with("/v1") {
        format!("{}/messages", base)
    } else {
        format!("{}/v1/messages", base)
    };
    let is_stream = req_body
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let upstream_req = state
        .http_client
        .post(&upstream_url)
        .header(header::CONTENT_TYPE, "application/json")
        // Anthropic auth: providers accept either x-api-key or Bearer.
        // Send both — some "compatible" upstreams check the wrong one.
        .header("x-api-key", &relay.api_key)
        .header(header::AUTHORIZATION, format!("Bearer {}", relay.api_key))
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header(
            header::ACCEPT,
            if is_stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&req_body);

    let upstream_resp = match upstream_req.send().await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[AnthropicProxy] Upstream connect error: {e}");
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "upstream_connect_error",
                        "message": e.to_string(),
                    }
                })),
            )
                .into_response();
        }
    };

    let status = upstream_resp.status();
    let upstream_content_type = upstream_resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    // Stream upstream body straight back — no parsing, no re-emission.
    // Works for both SSE (text/event-stream) and JSON. SSE keeps its
    // chunked semantics since we never buffer.
    let upstream_stream = upstream_resp.bytes_stream();
    let body = Body::from_stream(upstream_stream);

    let mut response = Response::builder()
        .status(status)
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty()));
    if let Ok(ct) = HeaderValue::from_str(&upstream_content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, ct);
    }
    response
}

fn filter_deepseek_tools(body: &mut Value) -> Result<(), &'static str> {
    let forced = (body["tool_choice"]["type"] == "tool")
        .then(|| body["tool_choice"]["name"].as_str().map(str::to_owned))
        .flatten();
    let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let supported = |tool: &Value| {
        matches!(
            tool["type"].as_str(),
            None | Some(
                "custom"
                    | "web_search_20250305"
                    | "web_search_20260209"
                    | "deepseek_web_search"
                    | "deepseek_read_url"
            )
        )
    };
    if forced.as_deref().is_some_and(|name| {
        tools
            .iter()
            .any(|tool| tool["name"] == name && !supported(tool))
    }) {
        return Err("The selected DeepSeek API does not support the requested server tool.");
    }
    tools.retain(supported);
    // DeepSeek selects server tools by type. Client functions use an untyped schema.
    for tool in tools {
        if tool["type"] == "custom" {
            tool.as_object_mut()
                .expect("custom tool is an object")
                .remove("type");
        }
    }
    Ok(())
}

fn read_relay(filename: &str) -> Option<Relay> {
    let path = relay_path(filename)?;
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: Value = serde_json::from_str(&content).ok()?;

    let base_url = parsed
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let api_key = parsed
        .get("apiKey")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let real_model_id = parsed
        .get("actualModel")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model_name = parsed
        .get("modelName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Some(Relay {
        base_url,
        api_key,
        web_search: parsed["webSearch"].as_bool(),
        model_name,
        real_model_id,
    })
}

fn relay_path(filename: &str) -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".echobird").join(filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_requests_without_tools_preserve_optional_fields() {
        let mut body =
            json!({"model":"deepseek-flash","messages":[{"role":"user","content":"hello"}]});
        let original = body.clone();
        filter_deepseek_tools(&mut body).unwrap();
        assert_eq!(body, original);
    }

    #[tokio::test]
    async fn deepseek_relay_removes_unsupported_server_tools_before_network_and_preserves_functions(
    ) {
        use axum::{routing::post, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let app = Router::new().route(
            "/anthropic/v1/messages",
            post(move |Json(body): Json<Value>| {
                let calls = observed.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["model"], "deepseek-flash");
                    assert_eq!(body["tools"].as_array().unwrap().len(), 3);
                    assert_eq!(body["tools"][0]["name"], "Bash");
                    assert_eq!(body["tools"][1]["type"], "web_search_20260209");
                    assert_eq!(body["tools"][2]["name"], "advisor");
                    assert!(body["tools"][2].get("type").is_none());
                    assert_eq!(body["tools"][2]["input_schema"], json!({"type":"object"}));
                    Json(json!({"content":[{"type":"text","text":"OK"}]}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let path =
            std::env::temp_dir().join(format!("deepseek-relay-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path,serde_json::to_vec(&json!({"baseUrl":format!("http://api.deepseek.com:{}/anthropic",address.port()),"apiKey":"fixture","actualModel":"deepseek-flash","webSearch":true})).unwrap()).unwrap();
        let state = AppState {
            http_client: reqwest::Client::builder()
                .no_proxy()
                .resolve("api.deepseek.com", address)
                .build()
                .unwrap(),
        };
        let body = json!({"model":"claude-opus-5-5","messages":[{"role":"user","content":"hello"}],"tools":[
            {"name":"Bash","input_schema":{"type":"object"}},
            {"type":"advisor_20260301","name":"native_advisor","model":"claude-opus-5-5"},
            {"type":"web_search_20260209","name":"web_search"},
            {"type":"custom","name":"advisor","input_schema":{"type":"object"}},
            {"type":"web_fetch_20260209","name":"web_fetch"}
        ]});
        let response = handle_messages_with(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&body).unwrap()),
            path.to_str().unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&bytes).unwrap().contains("OK"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut forced = body.clone();
        forced["tool_choice"] = json!({"type":"tool","name":"native_advisor"});
        let response = handle_messages_with(
            State(state.clone()),
            Bytes::from(serde_json::to_vec(&forced).unwrap()),
            path.to_str().unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let mut forced_client = body;
        forced_client["tool_choice"] = json!({"type":"tool","name":"advisor"});
        let response = handle_messages_with(
            State(state),
            Bytes::from(serde_json::to_vec(&forced_client).unwrap()),
            path.to_str().unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        std::fs::remove_file(path).unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn smart_switch_science_catalog_exposes_native_picker_names() {
        let relay = Relay {
            base_url: "http://127.0.0.1:53683/v1".into(),
            api_key: "fixture".into(),
            web_search: None,
            model_name: Some("Auto Router".into()),
            real_model_id: Some("auto".into()),
        };
        let response = crate::services::smart_router::catalog::with_test_models(|| {
            science_models_response(&relay)
        });
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let catalog: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(catalog["data"].as_array().unwrap().len(), 3);
        assert_eq!(catalog["data"][1]["display_name"], "First model");
        assert_eq!(
            catalog["last_id"],
            crate::services::smart_router::catalog::selector("second")
        );
    }

    #[tokio::test]
    async fn science_catalog_exposes_the_applied_model() {
        let relay = Relay {
            base_url: "https://example.test/anthropic".into(),
            api_key: "secret".into(),
            web_search: None,
            model_name: Some("DeepSeek Flash".into()),
            real_model_id: Some("deepseek-flash".into()),
        };
        let response = science_models_response(&relay);
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let catalog: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(catalog["data"][0]["id"], "deepseek-flash");
        assert_eq!(catalog["data"][0]["display_name"], "DeepSeek Flash");
        assert_eq!(catalog["has_more"], false);
    }
}
