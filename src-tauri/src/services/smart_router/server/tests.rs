use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::routing::post;
use http_body_util::BodyExt;

use super::*;

fn candidate(addr: SocketAddr, id: &str) -> Candidate {
    let model_id = format!("{id}-model");
    let base_url = format!("http://{addr}/v1");
    let api_key = "upstream-key".to_string();
    Candidate {
        attempt: None,
        internal_id: id.to_string(),
        fingerprint: candidate_fingerprint(&model_id, &base_url, &api_key),
        model_id,
        base_url,
        api_key,
        account_source: None,
    }
}

fn inbound_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer local-key"),
    );
    headers
}

async fn spawn_mock(app: Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

#[test]
fn resolves_configured_models_with_blank_keys_without_a_catalog_match() {
    let models: Vec<crate::models::model::ModelConfig> = serde_json::from_value(json!([
        {"internalId":"kilo", "name":"Kilo", "modelId":"kilo-auto/free",
            "baseUrl":"https://api.kilo.ai/api/gateway", "apiKey":""},
        {"internalId":"zen", "name":"Zen", "modelId":"space-bunny-free",
            "baseUrl":"https://opencode.ai/zen/v1", "apiKey":"   "},
        {"internalId":"zen-free", "name":"Zen Free", "modelId":"space-bunny-free",
            "baseUrl":"https://opencode.ai/inference/v1", "apiKey":""},
        {"internalId":"paid", "name":"Paid", "modelId":"claude-opus-5",
            "baseUrl":"https://opencode.ai/zen/v1", "apiKey":""},
        {"internalId":"spoof", "name":"Spoof", "modelId":"kilo-auto/free",
            "baseUrl":"https://example.com/v1", "apiKey":""},
        {"internalId":"blank", "name":"Blank", "modelId":"ordinary-model",
            "baseUrl":"https://example.com/v1", "apiKey":""},
        {"internalId":"account", "name":"Account", "modelId":"account-model",
            "baseUrl":"account://kilo", "apiKey":"",
            "accountSource":{"provider":"kilo", "accountId":"saved-account"}},
        {"internalId":"loop", "name":"Loop", "modelId":"auto",
            "baseUrl":"http://127.0.0.1:53683/v1", "apiKey":"key"}
    ]))
    .unwrap();
    let resolved = resolve_candidates_from(
        [
            "paid", "zen", "account", "missing", "kilo", "zen-free", "spoof", "blank", "loop",
        ]
        .map(String::from)
        .to_vec(),
        &models,
        &crate::services::local_llm::LocalServerInfo::default(),
    );
    assert_eq!(
        resolved
            .iter()
            .map(|item| item.internal_id.as_str())
            .collect::<Vec<_>>(),
        ["paid", "zen", "account", "kilo", "zen-free", "spoof", "blank"]
    );
    assert!(resolved.iter().all(|item| item.api_key.trim().is_empty()));
    assert!(resolved[2].account_source.is_some());
}

#[test]
fn maintenance_billing_cooldown_preserves_known_free_models() {
    use crate::models::model::AccountSource;
    for provider in [AccountProvider::WorkBuddy, AccountProvider::WorkBuddyAi] {
        let state = AppState::for_tests().unwrap();
        let account_id = uuid::Uuid::new_v4().to_string();
        crate::services::account_proxy::remember_free_model_ids(
            provider,
            &json!({"accountId":account_id}),
            vec!["free-model".into()],
        );
        let mut paid = candidate("127.0.0.1:1".parse().unwrap(), "paid");
        paid.account_source = Some(AccountSource {
            provider,
            account_id,
        });
        let mut free = paid.clone();
        free.internal_id = "free".into();
        free.model_id = "free-model".into();
        begin_attempt(&state, &mut paid);
        mark_failure(&state, &paid, FailureClass::Billing, None);
        assert!(candidate_is_cooling_down(&state, &paid));
        assert!(!candidate_is_cooling_down(&state, &free));
        assert_eq!(
            prioritized_candidates(&state, vec![paid.clone(), free.clone()]).len(),
            1
        );
        let mut unknown = free.clone();
        unknown.model_id = "unknown-free-status".into();
        assert!(!candidate_is_cooling_down(&state, &unknown));
        mark_failure(&state, &paid, FailureClass::Authentication, None);
        assert!(candidate_is_cooling_down(&state, &free));
        let limited = AppState::for_tests().unwrap();
        begin_attempt(&limited, &mut paid);
        mark_failure(&limited, &paid, FailureClass::RateLimit, None);
        assert!(!candidate_is_cooling_down(&limited, &free));
    }
}

#[tokio::test]
async fn maintenance_deepseek_token_limit_reaches_the_chat_upstream() {
    let captured = Arc::new(Mutex::new(Value::Null));
    let addr = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post({
            let captured = captured.clone();
            move |Json(body): Json<Value>| {
                *captured.lock().unwrap() = body;
                async { Json(json!({"choices":[{"message":{"content":"ok"}}]})) }
            }
        }),
    ))
    .await;
    let mut state = AppState::for_tests().unwrap();
    state.http_client = reqwest::Client::builder()
        .no_proxy()
        .resolve("api.deepseek.com", addr)
        .build()
        .unwrap();
    let mut model = candidate(addr, "fixture");
    model.base_url = format!("http://api.deepseek.com:{}/v1", addr.port());
    let body = json!({"messages":[],"max_completion_tokens":32768,"custom":"keep"});
    send_api_request(
        &state,
        &HeaderMap::new(),
        &model,
        &body,
        RouteProtocol::Chat,
    )
    .await
    .unwrap();
    let sent = captured.lock().unwrap().clone();
    assert_eq!(sent["max_tokens"], 32768);
    assert!(sent.get("max_completion_tokens").is_none());
    assert_eq!(sent["custom"], "keep");
    let dual = json!({"messages":[],"max_tokens":8192,"max_completion_tokens":32768});
    send_api_request(
        &state,
        &HeaderMap::new(),
        &model,
        &dual,
        RouteProtocol::Chat,
    )
    .await
    .unwrap();
    assert_eq!(*captured.lock().unwrap(), dual);
    model.base_url = format!("http://{addr}/v1");
    send_api_request(
        &state,
        &HeaderMap::new(),
        &model,
        &body,
        RouteProtocol::Chat,
    )
    .await
    .unwrap();
    assert_eq!(*captured.lock().unwrap(), body);
}

#[tokio::test]
async fn maintenance_413_error_names_the_request_size_failure() {
    let response = json_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "Request body is too large",
        None,
    );
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["type"], "request_too_large");
}

#[tokio::test]
async fn maintenance_local_and_upstream_413_are_structured_without_retry() {
    let state = AppState::for_tests().unwrap();
    let app = Router::new()
        .route("/v1/chat/completions", post(handle_chat))
        .route("/v1/responses", post(handle_responses))
        .route("/v1/messages", post(handle_messages))
        .layer(DefaultBodyLimit::max(4))
        .with_state(state.clone());
    let addr = spawn_mock(app).await;
    let client = reqwest::Client::new();
    for endpoint in ["chat/completions", "responses", "messages"] {
        let response = client
            .post(format!("http://{addr}/v1/{endpoint}"))
            .json(&json!({"oversized":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["type"],
            "request_too_large"
        );
    }
    let upstream = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post(|| async { (StatusCode::PAYLOAD_TOO_LARGE, "<html>body too big</html>") }),
    ))
    .await;
    let fallback_hits = Arc::new(AtomicUsize::new(0));
    let backup = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post({
            let hits = fallback_hits.clone();
            move || {
                hits.fetch_add(1, Ordering::SeqCst);
                async { "unexpected" }
            }
        }),
    ))
    .await;
    let response = route_chat_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"messages":[]}),
        vec![candidate(upstream, "large"), candidate(backup, "backup")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["error"]["type"],
        "request_too_large"
    );
    assert_eq!(fallback_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn blank_upstream_keys_send_no_authorization_for_chat_or_responses() {
    let captured = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let handler = {
        let captured = captured.clone();
        move |headers: HeaderMap| {
            let captured = captured.clone();
            async move {
                captured.lock().unwrap().push(
                    headers
                        .get(header::AUTHORIZATION)
                        .map(|value| value.to_str().unwrap().to_string()),
                );
                Json(json!({
                    "id":"response-auth", "object":"response", "status":"completed",
                    "model":"fixture-model", "output":[],
                    "choices":[{"message":{"role":"assistant", "content":"ok"}}]
                }))
            }
        }
    };
    let addr = spawn_mock(
        Router::new()
            .route("/v1/chat/completions", post(handler.clone()))
            .route("/v1/responses", post(handler)),
    )
    .await;
    let state = AppState::for_tests().unwrap();
    for protocol in [RouteProtocol::Chat, RouteProtocol::Responses] {
        for key in ["", " \t ", "upstream-key"] {
            let models = vec![serde_json::from_value(json!({
                "internalId":"keyless", "name":"Custom API", "modelId":"fixture-model",
                "baseUrl":format!("http://{addr}/v1"), "apiKey":key
            }))
            .unwrap()];
            let resolved = resolve_candidates_from(
                vec!["keyless".into()],
                &models,
                &crate::services::local_llm::LocalServerInfo::default(),
            );
            assert_eq!(resolved.len(), 1);
            let upstream = &resolved[0];
            let response = send_api_request(
                &state,
                &inbound_headers(),
                upstream,
                &json!({"model":"fixture-model", "stream":false}),
                protocol,
            )
            .await
            .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        }
    }
    assert_eq!(
        *captured.lock().unwrap(),
        vec![
            None,
            None,
            Some("Bearer upstream-key".into()),
            None,
            None,
            Some("Bearer upstream-key".into())
        ]
    );
}

#[test]
fn zen_requires_a_user_key_even_for_models_named_free() {
    let mut upstream = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "free");
    upstream.api_key.clear();
    upstream.base_url = "https://opencode.ai/zen/v1".into();
    upstream.model_id = "space-bunny-free".into();
    assert_eq!(upstream_bearer_token(&upstream), None);
    upstream.api_key = "real-account-key".into();
    assert_eq!(upstream_bearer_token(&upstream), Some("real-account-key"));
    upstream.api_key.clear();
    upstream.model_id = "claude-opus-5".into();
    assert_eq!(upstream_bearer_token(&upstream), None);
    upstream.model_id = "space-bunny-free".into();
    upstream.base_url = "https://example.com/zen/v1".into();
    assert_eq!(upstream_bearer_token(&upstream), None);
    upstream.base_url = "https://api.kilo.ai/api/gateway".into();
    upstream.model_id = "kilo-auto/free".into();
    assert_eq!(upstream_bearer_token(&upstream), None);
}

#[tokio::test]
#[ignore = "Calls a public upstream model; run explicitly for live verification"]
async fn live_keyless_kilo_routes_through_the_actual_candidate_resolver() {
    let model: crate::models::model::ModelConfig = serde_json::from_value(json!({
        "internalId":"live-keyless-kilo", "name":"Kilo AI",
        "modelId":"kilo-auto/free", "baseUrl":"https://api.kilo.ai/api/gateway",
        "apiKey":""
    }))
    .unwrap();
    let candidates = resolve_candidates_from(
        vec![model.internal_id.clone()],
        &[model],
        &crate::services::local_llm::LocalServerInfo::default(),
    );
    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].api_key.is_empty());
    let state = AppState::for_tests().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_str(&format!("EchoBird/{}", env!("CARGO_PKG_VERSION"))).unwrap(),
    );
    let response = route_chat_with_candidates(
        &state,
        &headers,
        json!({"model":SMART_ROUTER_MODEL_ID, "stream":false, "max_tokens":128,
                "messages":[{"role":"user", "content":"Reply with exactly OK and nothing else."}]}),
        candidates,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        reply["choices"][0]["message"]["content"]
            .as_str()
            .map(str::trim),
        Some("OK")
    );
}

#[test]
fn route_activity_tracks_the_latest_in_flight_candidate() {
    let mut activity = RouteActivity::default();
    let first = activity.begin("first", 10);
    let second = activity.begin("second", 20);

    let snapshot = activity.snapshot();
    assert_eq!(snapshot.candidate_id.as_deref(), Some("second"));
    assert!(snapshot.active);
    assert_eq!(snapshot.active_candidate_ids, ["first", "second"]);

    activity.end(second, 30);
    let snapshot = activity.snapshot();
    assert_eq!(snapshot.candidate_id.as_deref(), Some("first"));
    assert!(snapshot.active);
    assert_eq!(snapshot.active_candidate_ids, ["first"]);

    activity.end(first, 40);
    let snapshot = activity.snapshot();
    assert_eq!(snapshot.candidate_id.as_deref(), Some("first"));
    assert!(!snapshot.active);
    assert!(snapshot.active_candidate_ids.is_empty());
    assert_eq!(snapshot.sequence, 4);
    assert_eq!(snapshot.updated_at_ms, 40);
}

#[test]
fn classifies_only_failover_worthy_statuses() {
    assert_eq!(
        classify_failure(StatusCode::UNAUTHORIZED, b""),
        Some(FailureClass::Authentication)
    );
    assert_eq!(
        classify_failure(StatusCode::TOO_MANY_REQUESTS, b""),
        Some(FailureClass::RateLimit)
    );
    assert_eq!(
        classify_failure(
            StatusCode::TOO_MANY_REQUESTS,
            br#"{"error":{"code":"insufficient_quota"}}"#
        ),
        Some(FailureClass::Billing)
    );
    assert_eq!(
        classify_failure(StatusCode::REQUEST_TIMEOUT, b""),
        Some(FailureClass::Transient)
    );
    assert_eq!(
        classify_failure(StatusCode::BAD_GATEWAY, b""),
        Some(FailureClass::Transient)
    );
    assert_eq!(
        classify_failure(StatusCode::BAD_REQUEST, b"bad input"),
        None
    );
    assert_eq!(
        classify_failure(StatusCode::FORBIDDEN, b"blocked by safety policy"),
        None
    );
    assert_eq!(
        classify_failure(StatusCode::PAYLOAD_TOO_LARGE, b"too large"),
        None
    );
    assert_eq!(
        classify_failure(StatusCode::BAD_REQUEST, b"model_not_found"),
        Some(FailureClass::ModelUnavailable)
    );
    assert_eq!(
        classify_failure(StatusCode::BAD_REQUEST, b"unknown model"),
        Some(FailureClass::ModelUnavailable)
    );
    assert_eq!(
        classify_failure(StatusCode::BAD_REQUEST, b"insufficient quota"),
        Some(FailureClass::Billing)
    );
    assert_eq!(
        classify_failure(
            StatusCode::FORBIDDEN,
            b"tools are not supported: blocked by safety policy"
        ),
        None
    );
}

#[test]
fn accepts_current_and_legacy_router_model_ids() {
    assert!(is_supported_router_model("auto"));
    assert!(is_supported_router_model("echobird/auto"));
    assert!(is_supported_router_model("echobird-auto"));
    assert!(is_supported_router_model("smart-router"));
    assert!(!is_supported_router_model("other-model"));
}

#[test]
fn authorization_requires_a_nonempty_expected_key_and_matching_credentials() {
    let mut headers = HeaderMap::new();
    assert!(!is_authorized(&headers, ""));
    assert!(!is_authorized(&headers, " \t\r\n"));
    assert!(!is_authorized(&headers, "fixture-key"));
    headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer "));
    assert!(!is_authorized(&headers, ""));
    headers.insert("x-api-key", HeaderValue::from_static(""));
    assert!(!is_authorized(&headers, ""));
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer fixture-key"),
    );
    assert!(is_authorized(&headers, "fixture-key"));
    headers.remove(header::AUTHORIZATION);
    headers.insert("x-api-key", HeaderValue::from_static("fixture-key"));
    assert!(is_authorized(&headers, "fixture-key"));
    assert!(!is_authorized(&headers, "different-key"));
}

#[test]
fn groups_keep_activity_memory_and_custom_stream_model_ids_separate() {
    let first_id = format!("smart-router-{}", uuid::Uuid::new_v4());
    let second_id = format!("smart-router-{}", uuid::Uuid::new_v4());
    let first_activity = super::super::profiles::with(&first_id, shared_activity);
    let second_activity = super::super::profiles::with(&second_id, shared_activity);
    let first_memory = super::super::profiles::with(&first_id, shared_response_memory);
    let second_memory = super::super::profiles::with(&second_id, shared_response_memory);
    assert!(!Arc::ptr_eq(&first_memory, &second_memory));
    assert!(first_memory.lock().unwrap().response_bindings.is_empty());
    assert!(second_memory.lock().unwrap().response_bindings.is_empty());
    let activity = RouteActivityGuard::begin_for("first-candidate", first_activity.clone());
    assert!(first_activity
        .lock()
        .unwrap()
        .active_attempts
        .values()
        .any(|id| id == "first-candidate"));
    assert!(second_activity.lock().unwrap().active_attempts.is_empty());
    drop(activity);
    assert!(first_activity.lock().unwrap().active_attempts.is_empty());
    assert_ne!(
        super::super::profiles::with(&first_id, response_memory_path),
        super::super::profiles::with(&second_id, response_memory_path)
    );

    let mut state = AppState::for_tests().unwrap();
    state.model_id = "custom-router".into();
    assert!(state.supports_model("custom-router"));
    assert!(!state.supports_model("auto"));
    let mut adapter = AnthropicSseAdapter::new();
    adapter.model_id = state.model_id;
    let event = String::from_utf8(adapter.start_event().to_vec()).unwrap();
    assert!(event.contains(r#""model":"custom-router""#));
}

#[test]
fn appends_chat_completions_once() {
    assert_eq!(
        chat_completions_url("https://example.com/v1"),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        chat_completions_url("https://example.com/v1/chat/completions"),
        "https://example.com/v1/chat/completions"
    );
}

#[tokio::test]
async fn sends_openrouter_app_attribution_headers() {
    let upstream = Router::new().route(
        "/v1/chat/completions",
        post(|headers: HeaderMap, Json(body): Json<Value>| async move {
            assert_eq!(headers["HTTP-Referer"], "https://echobird.ai");
            assert_eq!(headers["X-OpenRouter-Title"], "EchoBird");
            assert_eq!(
                headers["X-OpenRouter-Categories"],
                "programming-app,personal-agent"
            );
            Json(json!({
                "id": "chatcmpl-attribution",
                "model": body["model"],
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            }))
        }),
    );
    let addr = spawn_mock(upstream).await;
    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": []}),
        vec![candidate(addr, "openrouter")],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
}

#[test]
fn configured_order_wins_over_previous_success_and_survives_reordering() {
    let state = AppState::for_tests().unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], 1));
    let first = candidate(addr, "first");
    let second = candidate(addr, "second");
    let third = candidate(addr, "third");

    mark_success(&state, &third);
    let ordered =
        prioritized_candidates(&state, vec![first.clone(), second.clone(), third.clone()]);
    assert_eq!(
        ordered
            .iter()
            .map(|model| model.internal_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first", "second", "third"]
    );

    let reordered = prioritized_candidates(&state, vec![second, third, first]);
    assert_eq!(
        reordered
            .iter()
            .map(|model| model.internal_id.as_str())
            .collect::<Vec<_>>(),
        vec!["second", "third", "first"]
    );
}

#[test]
fn first_candidate_regains_priority_after_cooldown() {
    let state = AppState::for_tests().unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], 1));
    let first = candidate(addr, "first");
    let second = candidate(addr, "second");

    mark_failure(&state, &first, FailureClass::RateLimit, None);
    mark_success(&state, &second);
    let available = prioritized_candidates(&state, vec![first.clone(), second.clone()]);
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].internal_id, "second");

    state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .get_mut("first")
        .unwrap()
        .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    let recovered = prioritized_candidates(&state, vec![first, second]);
    assert_eq!(recovered.len(), 2);
    assert_eq!(recovered[0].internal_id, "first");
}

#[test]
fn exhausted_models_stay_local_but_authentication_protects_the_account() {
    use crate::models::model::{AccountProvider, AccountSource};
    let address = "127.0.0.1:8080".parse().unwrap();
    let mut first = candidate(address, "first");
    first.account_source = Some(AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "account-one".into(),
    });
    let mut sibling = candidate(address, "sibling");
    sibling.account_source = first.account_source.clone();
    let mut other = candidate(address, "other");
    other.account_source = Some(AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "account-two".into(),
    });
    let state = AppState::for_tests().unwrap();
    mark_failure(&state, &first, FailureClass::RateLimit, None);
    assert!(!candidate_is_cooling_down(&state, &sibling));
    assert_eq!(
        prioritized_candidates(&state, vec![first.clone(), sibling.clone(), other])
            .iter()
            .map(|c| c.internal_id.as_str())
            .collect::<Vec<_>>(),
        vec!["sibling", "other"]
    );
    mark_failure(&state, &first, FailureClass::Authentication, None);
    assert!(candidate_is_cooling_down(&state, &sibling));
    let state = AppState::for_tests().unwrap();
    mark_failure(&state, &first, FailureClass::ModelUnavailable, None);
    assert!(!candidate_is_cooling_down(&state, &sibling));
    assert_eq!(
        prioritized_candidates(&state, vec![first, sibling]).len(),
        1
    );
}

#[test]
fn legacy_success_memory_does_not_override_configured_order() {
    let state = AppState::for_tests().unwrap();
    *state.route_memory.lock().unwrap() = serde_json::from_value(json!({
        "version": 1,
        "lastSuccessId": "second",
        "candidates": {}
    }))
    .unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], 1));
    let available = prioritized_candidates(
        &state,
        vec![candidate(addr, "first"), candidate(addr, "second")],
    );
    assert_eq!(available[0].internal_id, "first");
}

#[test]
fn slow_writer_does_not_hold_the_routing_memory_mutex() {
    let root = std::env::temp_dir().join(format!("echobird-performance-{}", uuid::Uuid::new_v4()));
    let memory = Arc::new(Mutex::new(RouteMemory::default()));
    let writer = ROUTE_MEMORY_WRITERS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    let (ready, held) = std::sync::mpsc::channel();
    let shared = memory.clone();
    let path = root.join("state.json");
    let worker = std::thread::spawn(move || {
        let locked = shared.lock().unwrap();
        ready.send(()).unwrap();
        persist_memory_at(&path, locked);
    });
    held.recv_timeout(Duration::from_secs(5)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while memory.try_lock().is_err() {
        assert!(
            Instant::now() < deadline,
            "disk writer retained the routing lock"
        );
        std::thread::yield_now();
    }
    assert!(
        !worker.is_finished(),
        "positive control: writer should still be blocked"
    );
    drop(writer);
    worker.join().unwrap();
    assert!(root.join("state.json").is_file());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ordered_persistence_rejects_older_snapshots_and_releases_memory_before_io() {
    let root = std::env::temp_dir().join(format!("echobird-performance-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.as_path().join("state.json");
    let mut current = RouteMemory {
        revision: 20,
        ..RouteMemory::default()
    };
    write_memory_snapshot(&path, 20, &current);
    current.revision = 10;
    write_memory_snapshot(&path, 10, &current);
    assert_eq!(load_route_memory(&path).revision, 20);
    // The memory mutex is immediately reusable after the durable write.
    let memory = Mutex::new(RouteMemory::default());
    persist_memory_at(&root.as_path().join("other.json"), memory.lock().unwrap());
    assert!(memory.try_lock().is_ok());
    assert!(root.join("other.json").is_file());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn route_memory_survives_round_trip_and_skips_cooldown() {
    let state = AppState::for_tests().unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], 1));
    let first = candidate(addr, "first");
    let second = candidate(addr, "second");

    mark_success(&state, &second);
    let ordered = prioritized_candidates(&state, vec![first.clone(), second.clone()]);
    assert_eq!(ordered[0].internal_id, "first");

    mark_failure(&state, &second, FailureClass::RateLimit, None);
    let serialized = {
        let memory = state.route_memory.lock().unwrap();
        serde_json::to_string(&*memory).unwrap()
    };
    let restored: RouteMemory = serde_json::from_str(&serialized).unwrap();
    let restored_state = AppState {
        fixture_responses: None,
        fixture_authentication: None,
        http_client: state.http_client.clone(),
        route_memory: Arc::new(Mutex::new(restored)),
        route_memory_path: None,
        ..state.clone()
    };
    let available = prioritized_candidates(&restored_state, vec![first, second]);
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].internal_id, "first");
}

#[test]
fn cooldowns_match_failure_type_and_cap_retry_after() {
    assert_eq!(
        cooldown_for_failure(FailureClass::RateLimit, None, 1),
        Duration::from_secs(60)
    );
    assert_eq!(
        cooldown_for_failure(FailureClass::Transient, None, 4),
        Duration::from_secs(4 * 60)
    );
    assert_eq!(
        cooldown_for_failure(FailureClass::ModelUnavailable, None, 1),
        Duration::from_secs(6 * 60 * 60)
    );
    assert_eq!(
        cooldown_for_failure(FailureClass::RateLimit, Some(Duration::MAX), 1),
        MAX_RETRY_AFTER
    );
}

#[test]
fn parses_retry_after_seconds_and_http_dates() {
    let mut headers = HeaderMap::new();
    headers.insert(header::RETRY_AFTER, HeaderValue::from_static("3600"));
    assert_eq!(retry_after(&headers), Some(Duration::from_secs(3600)));

    let retry_at = chrono::Utc::now() + chrono::Duration::seconds(90);
    let http_date = retry_at.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    headers.insert(
        header::RETRY_AFTER,
        HeaderValue::from_str(&http_date).unwrap(),
    );
    let parsed = retry_after(&headers).unwrap();
    assert!(parsed >= Duration::from_secs(88));
    assert!(parsed <= Duration::from_secs(90));
}

#[test]
fn expired_transient_cooldown_keeps_failure_streak() {
    let state = AppState::for_tests().unwrap();
    let model = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "transient");

    mark_failure(&state, &model, FailureClass::Transient, None);
    {
        let mut memory = state.route_memory.lock().unwrap();
        memory
            .candidates
            .get_mut(&model.internal_id)
            .unwrap()
            .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    }
    assert_eq!(prioritized_candidates(&state, vec![model.clone()]).len(), 1);

    mark_failure(&state, &model, FailureClass::Transient, None);
    let memory = state.route_memory.lock().unwrap();
    let health = memory.candidates.get(&model.internal_id).unwrap();
    assert_eq!(health.consecutive_failures, 2);
    let remaining = health.cooldown_until_ms.unwrap().saturating_sub(now_ms());
    assert!(remaining >= Duration::from_secs(59).as_millis() as u64);
}

#[tokio::test]
async fn reaches_twentieth_candidate_after_nineteen_fast_failures() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let upstream = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(body): Json<Value>| {
            let attempt = counter.fetch_add(1, Ordering::SeqCst) + 1;
            async move {
                if attempt < MAX_ROUTE_ATTEMPTS {
                    StatusCode::TOO_MANY_REQUESTS.into_response()
                } else {
                    Json(json!({
                        "id": "chatcmpl-test",
                        "model": body["model"],
                        "choices": [{"message": {"role": "assistant", "content": "ok"}}]
                    }))
                    .into_response()
                }
            }
        }),
    );
    let addr = spawn_mock(upstream).await;
    let candidates = (0..MAX_ROUTE_ATTEMPTS)
        .map(|index| candidate(addr, &format!("candidate-{index}")))
        .collect();
    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": []}),
        candidates,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(hits.load(Ordering::SeqCst), MAX_ROUTE_ATTEMPTS);
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "candidate-19/candidate-19-model"
    );
}

#[tokio::test]
async fn fallback_budget_stops_starting_more_candidates() {
    let slow = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            StatusCode::TOO_MANY_REQUESTS
        }),
    );
    let slow_addr = spawn_mock(slow).await;

    let second_hits = Arc::new(AtomicUsize::new(0));
    let second_counter = second_hits.clone();
    let second = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            second_counter.fetch_add(1, Ordering::SeqCst);
            async { StatusCode::OK }
        }),
    );
    let second_addr = spawn_mock(second).await;
    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_policy(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": []}),
        vec![
            candidate(slow_addr, "slow"),
            candidate(second_addr, "second"),
        ],
        RoutePolicy {
            max_attempts: MAX_ROUTE_ATTEMPTS,
            time_budget: Duration::from_millis(30),
            header_timeout: Duration::from_secs(1),
            first_byte_timeout: Duration::from_secs(1),
            ..RoutePolicy::default()
        },
    )
    .await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(second_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn success_body_cannot_outlive_routing_budget() {
    let hanging = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .body(Body::from_stream(stream::pending::<
                    Result<Bytes, Infallible>,
                >()))
                .unwrap()
        }),
    );
    let addr = spawn_mock(hanging).await;
    let state = AppState::for_tests().unwrap();
    let routed = tokio::time::timeout(
        Duration::from_millis(500),
        route_chat_with_policy(
            &state,
            &inbound_headers(),
            json!({"model": SMART_ROUTER_MODEL_ID, "messages": []}),
            vec![candidate(addr, "hanging")],
            RoutePolicy {
                max_attempts: 1,
                time_budget: Duration::from_millis(30),
                header_timeout: Duration::from_secs(1),
                first_byte_timeout: Duration::from_secs(1),
                ..RoutePolicy::default()
            },
        ),
    )
    .await
    .expect("router exceeded its own budget");

    assert_eq!(routed.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn falls_back_after_rate_limit() {
    let first_hits = Arc::new(AtomicUsize::new(0));
    let first_counter = first_hits.clone();
    let first = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            first_counter.fetch_add(1, Ordering::SeqCst);
            async { StatusCode::TOO_MANY_REQUESTS }
        }),
    );
    let first_addr = spawn_mock(first).await;

    let second_hits = Arc::new(AtomicUsize::new(0));
    let second_counter = second_hits.clone();
    let second = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(body): Json<Value>| {
            second_counter.fetch_add(1, Ordering::SeqCst);
            async move {
                Json(json!({
                    "id": "chatcmpl-test",
                    "model": body["model"],
                    "choices": [{"message": {"role": "assistant", "content": "ok"}}]
                }))
            }
        }),
    );
    let second_addr = spawn_mock(second).await;

    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": [], "stream": false}),
        vec![
            candidate(first_addr, "first"),
            candidate(second_addr, "second"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "second/second-model"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["model"], "second-model");
    assert_eq!(first_hits.load(Ordering::SeqCst), 1);
    assert_eq!(second_hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_bound_account_never_uses_its_url_or_key_and_falls_back() {
    use crate::models::model::{AccountProvider, AccountSource};
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let upstream = Router::new().route("/v1/chat/completions",post(move |Json(body):Json<Value>| {
            counter.fetch_add(1,Ordering::SeqCst);
            async move { Json(json!({"id":"fixture","model":body["model"],"choices":[{"message":{"role":"assistant","content":"ok"}}]})) }
        }));
    let addr = spawn_mock(upstream).await;
    let mut missing = candidate(addr, "missing");
    missing.api_key.clear();
    missing.account_source = Some(AccountSource {
        provider: AccountProvider::Codex,
        account_id: "invalid-fixture-id".into(),
    });
    let mut sibling = missing.clone();
    sibling.internal_id = "same-account-sibling".into();
    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_policy(
        &state,
        &inbound_headers(),
        json!({"model":"auto","messages":[{"role":"user","content":"test"}]}),
        vec![missing, sibling, candidate(addr, "working")],
        RoutePolicy {
            max_attempts: 2,
            ..RoutePolicy::default()
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "working/working-model"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(
        state.route_memory.lock().unwrap().candidates["missing"].failure,
        Some(FailureClass::Authentication)
    );
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("same-account-sibling"));
}

#[tokio::test]
async fn never_falls_back_after_first_stream_chunk() {
    let first = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let (tx, rx) = tokio::sync::mpsc::channel(2);
            tokio::spawn(async move {
                tx.send(Ok::<Bytes, std::io::Error>(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"first\"}}]}\n\n",
                )))
                .await
                .unwrap();
                tokio::time::sleep(Duration::from_millis(25)).await;
                let _ = tx.send(Err(std::io::Error::other("stream failed"))).await;
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        }),
    );
    let first_addr = spawn_mock(first).await;

    let second_hits = Arc::new(AtomicUsize::new(0));
    let second_counter = second_hits.clone();
    let second = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            second_counter.fetch_add(1, Ordering::SeqCst);
            async { Json(json!({"choices": []})) }
        }),
    );
    let second_addr = spawn_mock(second).await;

    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": [], "stream": true}),
        vec![
            candidate(first_addr, "first"),
            candidate(second_addr, "second"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "first/first-model"
    );
    let collected = response.into_body().collect().await;
    assert!(collected.is_err());
    assert_eq!(second_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn falls_back_when_success_stream_has_no_first_byte() {
    let first = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::empty())
                .unwrap()
        }),
    );
    let first_addr = spawn_mock(first).await;
    let second = Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            let chunks = stream::once(async {
                Ok::<Bytes, Infallible>(Bytes::from_static(
                    b"data: {\"choices\":[{\"delta\":{\"content\":\"second\"}}]}\n\n",
                ))
            });
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(chunks))
                .unwrap()
        }),
    );
    let second_addr = spawn_mock(second).await;

    let state = AppState::for_tests().unwrap();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model": SMART_ROUTER_MODEL_ID, "messages": [], "stream": true}),
        vec![
            candidate(first_addr, "first"),
            candidate(second_addr, "second"),
        ],
    )
    .await;

    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "second/second-model"
    );
}

#[tokio::test]
async fn anthropic_request_falls_back_and_preserves_tool_use() {
    let first = Router::new().route(
        "/v1/chat/completions",
        post(|| async { StatusCode::TOO_MANY_REQUESTS }),
    );
    let first_addr = spawn_mock(first).await;
    let second = Router::new().route(
        "/v1/chat/completions",
        post(|Json(body): Json<Value>| async move {
            assert_eq!(body["model"], "second-model");
            assert_eq!(body["tool_choice"], "required");
            Json(json!({
                "id": "chatcmpl-tool",
                "model": body["model"],
                "choices": [{
                    "message": {
                        "role": "assistant",
                        "content": null,
                        "tool_calls": [{
                            "id": "call_weather",
                            "type": "function",
                            "function": {"name": "weather", "arguments": "{\"city\":\"Shanghai\"}"}
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 12, "completion_tokens": 8}
            }))
        }),
    );
    let second_addr = spawn_mock(second).await;

    let state = AppState::for_tests().unwrap();
    let response = route_messages_with_candidates(
        &state,
        &inbound_headers(),
        json!({
            "model": "claude-opus-5",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "weather?"}],
            "tools": [{
                "name": "weather",
                "description": "Get weather",
                "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}
            }],
            "tool_choice": {"type": "any"}
        }),
        vec![
            candidate(first_addr, "first"),
            candidate(second_addr, "second"),
        ],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "second/second-model"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["stop_reason"], "tool_use");
    assert_eq!(body["content"][0]["type"], "tool_use");
    assert_eq!(body["content"][0]["name"], "weather");
    assert_eq!(body["content"][0]["input"]["city"], "Shanghai");
}

#[tokio::test]
async fn anthropic_stream_converts_text_and_tool_events() {
    let upstream = Router::new().route(
            "/v1/chat/completions",
            post(|| async {
                let chunks = stream::iter([
                    Ok::<Bytes, Infallible>(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                    )),
                    Ok(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\"}}]},\"finish_reason\":null}]}\n\n",
                    )),
                    Ok(Bytes::from_static(
                        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"completion_tokens\":9}}\n\n",
                    )),
                    Ok(Bytes::from_static(b"data: [DONE]\n\n")),
                ]);
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from_stream(chunks))
                    .unwrap()
            }),
        );
    let upstream_addr = spawn_mock(upstream).await;

    let state = AppState::for_tests().unwrap();
    let response = route_messages_with_candidates(
        &state,
        &inbound_headers(),
        json!({
            "model": "claude-opus-5",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{"role": "user", "content": "read a file"}]
        }),
        vec![candidate(upstream_addr, "stream")],
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/event-stream"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: message_start"));
    assert!(body.contains("\"type\":\"text_delta\""));
    assert!(body.contains("\"text\":\"hello\""));
    assert!(body.contains("\"type\":\"tool_use\""));
    assert!(body.contains("\"partial_json\":\"{\\\"path\\\":\\\"a.txt\\\"}\""));
    assert!(body.contains("\"stop_reason\":\"tool_use\""));
    assert!(body.contains("event: message_stop"));
}

#[test]
fn anthropic_parser_preserves_split_utf8_multiline_frames_and_tool_fragments() {
    let text = "data: {\"choices\":\r\ndata: [{\"delta\":{\"content\":\"你好🌍\"}}]}\r\n\r\n";
    let first_tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_utf8","function":{"name":"read_file","arguments":"{\"路径\":"}}]}}]});
    let last_tool = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"你好\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"completion_tokens":7}});
    let input = format!("{text}data: {first_tool}\r\n\r\ndata: {last_tool}\r\n\r\n");
    let mut adapter = AnthropicSseAdapter::new();
    let mut output = Vec::new();
    for byte in input.as_bytes() {
        for chunk in adapter.push(std::slice::from_ref(byte)).unwrap() {
            output.extend_from_slice(&chunk);
        }
    }
    for chunk in adapter.finish().unwrap() {
        output.extend_from_slice(&chunk);
    }
    let mut parser = crate::services::account_proxy::SseParser::default();
    let frames: Vec<Value> = parser
        .push(&output)
        .into_iter()
        .map(|(_, data)| serde_json::from_str(&data).unwrap())
        .collect();
    assert!(frames
        .iter()
        .any(|frame| frame["delta"]["text"] == "你好🌍"));
    let tool = frames
        .iter()
        .find(|frame| frame["content_block"]["type"] == "tool_use")
        .unwrap();
    assert_eq!(tool["content_block"]["id"], "call_utf8");
    assert_eq!(tool["content_block"]["name"], "read_file");
    assert!(frames
        .iter()
        .any(|frame| frame["delta"]["partial_json"] == "{\"路径\":\"你好\"}"));
    assert!(frames
        .iter()
        .any(|frame| frame["delta"]["stop_reason"] == "tool_use"
            && frame["usage"]["output_tokens"] == 7));
    assert_eq!(frames.last().unwrap()["type"], "message_stop");
}

#[test]
fn anthropic_parser_accepts_done_without_finish_reason() {
    let mut adapter = AnthropicSseAdapter::new();
    let events = adapter.push(b"data: [DONE]\n\n").unwrap();
    assert!(events
        .iter()
        .any(|event| event.starts_with(b"event: message_stop\n")));
    assert!(adapter.finish().unwrap().is_empty());
}

#[tokio::test]
async fn anthropic_stream_rejects_errors_invalid_json_and_unfinished_eof() {
    let prefix =
        "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    for tail in [
        "",
        "data: {\"error\":{\"message\":\"upstream failed\"}}\n\n",
        "event: error\ndata: {\"message\":\"upstream failed\"}\n\n",
        "data: {invalid json}\n\n",
    ] {
        let response =
            openai_stream_to_anthropic(Response::new(Body::from(format!("{prefix}{tail}"))));
        let mut output = response.into_body().into_data_stream();
        let mut bytes = Vec::new();
        let mut failed = false;
        while let Some(item) = output.next().await {
            match item {
                Ok(chunk) => bytes.extend_from_slice(&chunk),
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        let text = String::from_utf8(bytes).unwrap();
        assert!(failed, "unfinished input must fail: {tail}");
        assert!(text.contains("event: message_start"));
        assert!(!text.contains("event: message_stop"));
        assert!(!text.contains("\"stop_reason\":\"end_turn\""));
    }
}

#[tokio::test]
async fn dropping_anthropic_output_releases_pending_source_and_activity() {
    struct PendingSource {
        entered: Arc<tokio::sync::Notify>,
        dropped: Arc<tokio::sync::Notify>,
        drop_count: Arc<AtomicUsize>,
        activity_guard: Option<RouteActivityGuard>,
    }
    impl futures_util::Stream for PendingSource {
        type Item = Result<Bytes, std::io::Error>;

        fn poll_next(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            self.entered.notify_one();
            std::task::Poll::Pending
        }
    }
    impl Drop for PendingSource {
        fn drop(&mut self) {
            self.activity_guard.take();
            self.drop_count.fetch_add(1, Ordering::SeqCst);
            self.dropped.notify_one();
        }
    }
    let entered = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(tokio::sync::Notify::new());
    let drop_count = Arc::new(AtomicUsize::new(0));
    let activity_guard = RouteActivityGuard::begin("pending-anthropic-fixture");
    let attempt_id = activity_guard.attempt_id.unwrap();
    let response = openai_stream_to_anthropic(Response::new(Body::from_stream(PendingSource {
        entered: entered.clone(),
        dropped: dropped.clone(),
        drop_count: drop_count.clone(),
        activity_guard: Some(activity_guard),
    })));
    let mut output = response.into_body().into_data_stream();
    assert!(output
        .next()
        .await
        .unwrap()
        .unwrap()
        .starts_with(b"event: message_start\n"));
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert_eq!(drop_count.load(Ordering::SeqCst), 0);
    assert!(ROUTE_ACTIVITY
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .active_attempts
        .contains_key(&attempt_id));
    drop(output);
    tokio::time::timeout(Duration::from_secs(5), dropped.notified())
        .await
        .unwrap();
    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
    assert!(!ROUTE_ACTIVITY
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .active_attempts
        .contains_key(&attempt_id));
}

fn bound_health_candidate(id: &str) -> Candidate {
    let mut model = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), id);
    model.account_source = Some(crate::models::model::AccountSource {
        provider: crate::models::model::AccountProvider::Kilo,
        account_id: "health-fixture-account".into(),
    });
    model
}

#[tokio::test]
async fn partial_stream_failures_accumulate_until_real_completion_without_replay() {
    let complete = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let upstream_complete = complete.clone();
    let first_addr = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            let complete = upstream_complete.clone();
            async move {
                let mut data =
                    "data: {\"choices\":[{\"delta\":{\"content\":\"中文\"}}]}\n\n".to_string();
                if complete.load(Ordering::SeqCst) {
                    data.push_str("data: [DONE]\n\n");
                }
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from(data))
                    .unwrap()
            }
        }),
    ))
    .await;
    let backup_hits = Arc::new(AtomicUsize::new(0));
    let counter = backup_hits.clone();
    let backup_addr = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Json(json!({"choices":[]})) }
        }),
    ))
    .await;
    let state = AppState::for_tests().unwrap();
    for expected_failures in 1..=3 {
        let started = now_ms();
        let response = route_chat_with_candidates(
            &state,
            &inbound_headers(),
            json!({"messages":[],"stream":true}),
            vec![
                candidate(first_addr, "partial-stream"),
                candidate(backup_addr, "partial-backup"),
            ],
        )
        .await;
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "partial-stream/partial-stream-model"
        );
        let mut stream = response.into_body().into_data_stream();
        let first = stream.next().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&first).contains("中文"));
        assert!(stream.next().await.unwrap().is_err());
        let mut memory = state.route_memory.lock().unwrap();
        let health = memory.candidates.get_mut("partial-stream").unwrap();
        assert_eq!(health.consecutive_failures, expected_failures);
        let cooldown = cooldown_for_failure(FailureClass::Transient, None, expected_failures);
        assert!(health.cooldown_until_ms.unwrap() >= started + cooldown.as_millis() as u64);
        health.cooldown_until_ms = Some(started.saturating_sub(1));
    }
    complete.store(true, Ordering::SeqCst);
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"messages":[],"stream":true}),
        vec![candidate(first_addr, "partial-stream")],
    )
    .await;
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("[DONE]"));
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("partial-stream"));
    assert_eq!(backup_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn raw_stream_error_preserves_http_retry_after_without_replaying_output() {
    let release_error = Arc::new(tokio::sync::Notify::new());
    let release = release_error.clone();
    let first_addr = spawn_mock(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let release = release.clone();
                async move {
                    let first = stream::once(async {
                        Ok::<_, std::io::Error>(Bytes::from_static(
                            b"data: {\"choices\":[{\"delta\":{\"content\":\"ready\"}}]}\n\n",
                        ))
                    });
                    let error = stream::once(async move {
                        release.notified().await;
                        Ok::<_, std::io::Error>(Bytes::from_static(
                            b"event: error\ndata: {\"error\":{\"code\":\"rate_limit_exceeded\",\"message\":\"Rate limit\"}}\n\n",
                        ))
                    });
                    Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .header(header::RETRY_AFTER, "120")
                        .body(Body::from_stream(first.chain(error)))
                        .unwrap()
                }
            }),
        ))
        .await;
    let backup_hits = Arc::new(AtomicUsize::new(0));
    let counter = backup_hits.clone();
    let backup_addr = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Json(json!({"choices":[]})) }
        }),
    ))
    .await;
    let state = AppState::for_tests().unwrap();
    let started = now_ms();
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"messages":[],"stream":true}),
        vec![
            candidate(first_addr, "raw-stream-limit"),
            candidate(backup_addr, "raw-limit-backup"),
        ],
    )
    .await;
    let mut body = response.into_body().into_data_stream();
    let first = body.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("ready"));
    release_error.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(5), body.next())
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert!(body.next().await.is_none());
    let memory = state.route_memory.lock().unwrap();
    let health = &memory.candidates["raw-stream-limit"];
    assert_eq!(health.failure, Some(FailureClass::RateLimit));
    assert!(health.cooldown_until_ms.unwrap() >= started + 120_000);
    assert_eq!(backup_hits.load(Ordering::SeqCst), 0);
}

#[test]
fn structured_body_failure_preserves_cooldown_and_explicit_model_scope() {
    let state = AppState::for_tests().unwrap();
    let model = bound_health_candidate("stream-quota");
    let mut error = crate::services::account_proxy::ProxyError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "Rate limit",
    );
    error.error_code = Some("rate_limit_exceeded".into());
    error.retry_after = Some(HeaderValue::from_static("3600"));
    error.model_scope = true;
    let error = std::io::Error::other(std::io::Error::other(error));
    let started = now_ms();
    assert_eq!(
        mark_stream_failure(&state, &model, &error),
        Some(FailureClass::RateLimit)
    );
    let memory = state.route_memory.lock().unwrap();
    assert!(memory.accounts.is_empty());
    assert!(memory.candidates["stream-quota"].cooldown_until_ms.unwrap() >= started + 3_600_000);
}

#[tokio::test]
async fn preparation_metadata_stream_errors_invalidate_only_known_account_failures() {
    use crate::models::model::AccountSource;

    for (provider, status, code, invalidates, expected_failure) in [
        (
            AccountProvider::Copilot,
            StatusCode::NOT_FOUND,
            Some("model_not_found"),
            true,
            Some(FailureClass::ModelUnavailable),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::BAD_REQUEST,
            Some("model_not_supported"),
            true,
            Some(FailureClass::ModelUnavailable),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::UNAUTHORIZED,
            None,
            true,
            Some(FailureClass::Authentication),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::FORBIDDEN,
            None,
            true,
            Some(FailureClass::Authentication),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::NOT_FOUND,
            None,
            false,
            Some(FailureClass::ModelUnavailable),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::BAD_REQUEST,
            Some("tools_not_supported"),
            false,
            Some(FailureClass::RequestIncompatible),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::PAYMENT_REQUIRED,
            Some("insufficient_quota"),
            false,
            Some(FailureClass::Billing),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::TOO_MANY_REQUESTS,
            Some("rate_limit_exceeded"),
            false,
            Some(FailureClass::RateLimit),
        ),
        (
            AccountProvider::Copilot,
            StatusCode::BAD_REQUEST,
            Some("context_length_exceeded"),
            false,
            None,
        ),
        (
            AccountProvider::Kilo,
            StatusCode::NOT_FOUND,
            Some("model_not_found"),
            false,
            Some(FailureClass::ModelUnavailable),
        ),
        (
            AccountProvider::Kilo,
            StatusCode::UNAUTHORIZED,
            None,
            false,
            Some(FailureClass::Authentication),
        ),
    ] {
        let catalogue_hits = Arc::new(AtomicUsize::new(0));
        let inference_hits = Arc::new(AtomicUsize::new(0));
        let catalogue_counter = catalogue_hits.clone();
        let inference_counter = inference_hits.clone();
        let addr = spawn_mock(
                Router::new()
                    .route("/v1/models", get(move || {
                        catalogue_counter.fetch_add(1, Ordering::SeqCst);
                        async { Json(json!({"data":[{"id":"fixture-model", "supported_endpoints":["/chat/completions"]}]})) }
                    }))
                    .route("/v1/chat/completions", post(move || {
                        inference_counter.fetch_add(1, Ordering::SeqCst);
                        async {
                            Response::builder()
                                .header(header::CONTENT_TYPE, "text/event-stream")
                                .body(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"fixture-answer\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
                                .unwrap()
                        }
                    })),
            ).await;
        let account_id = uuid::Uuid::new_v4().to_string();
        let credentials = json!({
            "accessToken":"fixture-token", "baseUrl":format!("http://{addr}/v1"),
            "_echobird_saved_account_id":account_id,
        });
        let client = reqwest::Client::new();
        assert_eq!(
            crate::services::account_proxy::list_models(
                AccountProvider::Copilot,
                credentials.clone(),
                &client,
            )
            .await
            .unwrap(),
            ["fixture-model"],
        );
        let route = || {
            crate::services::account_proxy::route(
                AccountProvider::Copilot,
                credentials.clone(),
                "fixture-model",
                json!({"messages":[], "stream":false}),
                &client,
            )
        };
        let response = route().await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("fixture-answer"));
        assert_eq!(catalogue_hits.load(Ordering::SeqCst), 1);
        assert_eq!(inference_hits.load(Ordering::SeqCst), 1);

        let mut model = candidate(addr, "metadata-stream");
        model.account_source = Some(AccountSource {
            provider,
            account_id,
        });
        let mut error =
            crate::services::account_proxy::ProxyError::new(status, "Fixture upstream failure");
        error.error_code = code.map(str::to_owned);
        error.model_scope = matches!(
            code,
            Some("model_not_found" | "model_not_supported" | "tools_not_supported")
        );
        error.retry_after =
            (status == StatusCode::TOO_MANY_REQUESTS).then(|| HeaderValue::from_static("120"));
        let error = std::io::Error::other(std::io::Error::other(error));
        let state = AppState::for_tests().unwrap();
        assert_eq!(
            mark_stream_failure(&state, &model, &error),
            expected_failure
        );
        assert_eq!(catalogue_hits.load(Ordering::SeqCst), 1);
        assert_eq!(inference_hits.load(Ordering::SeqCst), 1);
        {
            let memory = state.route_memory.lock().unwrap();
            let records_failure = expected_failure
                .is_some_and(|failure| failure != FailureClass::RequestIncompatible);
            assert_eq!(
                memory.candidates.contains_key(&model.internal_id),
                records_failure
            );
            let records_account = expected_failure == Some(FailureClass::Authentication);
            assert_eq!(
                memory
                    .accounts
                    .contains_key(&account_health_key(&model).unwrap()),
                records_account
            );
            if records_failure {
                assert_eq!(
                    memory.candidates[&model.internal_id].failure,
                    expected_failure
                );
            }
        }

        let response = route().await.unwrap();
        let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&body).contains("fixture-answer"));
        assert_eq!(
            catalogue_hits.load(Ordering::SeqCst),
            1 + usize::from(invalidates),
            "{provider:?} {status} {code:?}"
        );
        assert_eq!(inference_hits.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn incompatible_tools_request_does_not_cool_down_plain_text_model() {
    let first_hits = Arc::new(AtomicUsize::new(0));
    let first_counter = first_hits.clone();
    let first_addr = spawn_mock(Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let counter = first_counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    if body["tools"].as_array().is_some_and(|tools| !tools.is_empty()) {
                        return openai_error(StatusCode::BAD_REQUEST, "tools are not supported");
                    }
                    Json(json!({"choices":[{"message":{"role":"assistant","content":"plain text"},"finish_reason":"stop"}]})).into_response()
                }
            }),
        )).await;
    let second_hits = Arc::new(AtomicUsize::new(0));
    let second_counter = second_hits.clone();
    let second_addr = spawn_mock(Router::new().route(
            "/v1/chat/completions",
            post(move || {
                let counter = second_counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"choices":[{"message":{"role":"assistant","content":"tool capable"},"finish_reason":"stop"}]}))
                }
            }),
        )).await;
    let state = AppState::for_tests().unwrap();
    let models = vec![
        candidate(first_addr, "plain-first"),
        candidate(second_addr, "tools-second"),
    ];
    let with_tools = route_chat_with_candidates(
            &state,
            &inbound_headers(),
            json!({"model":SMART_ROUTER_MODEL_ID,"messages":[],"stream":false,
                "tools":[{"type":"function","function":{"name":"read_file","parameters":{"type":"object"}}}]}),
            models.clone(),
        ).await;
    assert_eq!(with_tools.status(), StatusCode::OK);
    assert_eq!(
        with_tools.headers()["X-EchoBird-Routed-Via"],
        "tools-second/tools-second-model"
    );
    with_tools.into_body().collect().await.unwrap();
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("plain-first"));
    let plain = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model":SMART_ROUTER_MODEL_ID,"messages":[],"stream":false}),
        models,
    )
    .await;
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(
        plain.headers()["X-EchoBird-Routed-Via"],
        "plain-first/plain-first-model"
    );
    plain.into_body().collect().await.unwrap();
    assert_eq!(first_hits.load(Ordering::SeqCst), 2);
    assert_eq!(second_hits.load(Ordering::SeqCst), 1);
}

#[test]
fn old_success_cannot_clear_a_newer_rate_limit_revision() {
    let state = AppState::for_tests().unwrap();
    let mut old = bound_health_candidate("concurrent");
    begin_attempt(&state, &mut old);
    bind_attempt_credentials(&state, &old, &json!({"accessToken":"T0"}));
    let mut rejected = old.clone();
    begin_attempt(&state, &mut rejected);
    bind_attempt_credentials(&state, &rejected, &json!({"accessToken":"T0"}));
    mark_failure(
        &state,
        &rejected,
        FailureClass::RateLimit,
        Some(Duration::from_secs(600)),
    );
    mark_success(&state, &old);
    {
        let mut memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory.candidates["concurrent"].failure,
            Some(FailureClass::RateLimit)
        );
        assert!(memory.accounts.is_empty());
        memory
            .candidates
            .get_mut("concurrent")
            .unwrap()
            .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    }
    mark_success(&state, &old);
    assert!(state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("concurrent"));
    let mut fresh = old.clone();
    begin_attempt(&state, &mut fresh);
    bind_attempt_credentials(&state, &fresh, &json!({"accessToken":"T0"}));
    mark_success(&state, &fresh);
    let memory = state.route_memory.lock().unwrap();
    assert!(!memory.candidates.contains_key("concurrent"));
    assert!(memory.accounts.is_empty());
}

#[tokio::test]
async fn unsupported_parameters_try_the_backup_without_cooling_down_the_account() {
    for streaming in [false, true] {
        let mut error = crate::services::account_proxy::ProxyError::new(
            StatusCode::BAD_REQUEST,
            "The selected model does not advertise this output limit",
        );
        error.error_code = Some("unsupported_parameter".into());
        error.model_scope = true;
        let backup = if streaming {
            Response::new(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"backup\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
        } else {
            crate::services::account_proxy::json_response(
                &json!({"choices":[{"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}]}),
            )
        };
        let responses = Arc::new(Mutex::new(HashMap::from([
            ("unsupported-limit".to_string(), error.into_response()),
            ("limit-backup".to_string(), backup),
        ])));
        let mut state = AppState::for_tests().unwrap();
        state.fixture_responses = Some(responses.clone());
        let candidates = vec![
            bound_health_candidate("unsupported-limit"),
            bound_health_candidate("limit-backup"),
        ];
        let response = route_chat_with_candidates(
            &state,
            &inbound_headers(),
            json!({"messages":[],"max_tokens":1,"stream":streaming}),
            candidates.clone(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "limit-backup/limit-backup-model"
        );
        response.into_body().collect().await.unwrap();
        assert!(responses.lock().unwrap().is_empty());
        {
            let memory = state.route_memory.lock().unwrap();
            assert!(memory.candidates.is_empty());
            assert!(memory.accounts.is_empty());
        }
        responses.lock().unwrap().insert("unsupported-limit".into(), crate::services::account_proxy::json_response(&json!({"choices":[{"message":{"role":"assistant","content":"plain text"},"finish_reason":"stop"}]})));
        let response = route_chat_with_candidates(
            &state,
            &inbound_headers(),
            json!({"messages":[],"stream":false}),
            candidates,
        )
        .await;
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "unsupported-limit/unsupported-limit-model"
        );
        response.into_body().collect().await.unwrap();
    }
}

#[test]
fn short_rate_limit_cannot_shorten_existing_candidate_deadline() {
    let state = AppState::for_tests().unwrap();
    let mut model = bound_health_candidate("long-rate-limit");
    begin_attempt(&state, &mut model);
    bind_attempt_credentials(&state, &model, &json!({"accessToken":"T0"}));
    mark_failure(
        &state,
        &model,
        FailureClass::RateLimit,
        Some(Duration::from_secs(600)),
    );
    let long_deadline =
        state.route_memory.lock().unwrap().candidates["long-rate-limit"].cooldown_until_ms;
    begin_attempt(&state, &mut model);
    bind_attempt_credentials(&state, &model, &json!({"accessToken":"T0"}));
    mark_failure(
        &state,
        &model,
        FailureClass::RateLimit,
        Some(Duration::from_secs(60)),
    );
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(
        memory.candidates["long-rate-limit"].cooldown_until_ms,
        long_deadline
    );
    assert!(memory.accounts.is_empty());
    assert_eq!(memory.candidates["long-rate-limit"].consecutive_failures, 2);
}

#[test]
fn another_model_success_preserves_the_failed_resource_quota_cooldown() {
    for failure in [FailureClass::Billing, FailureClass::RateLimit] {
        let state = AppState::for_tests().unwrap();
        let mut failed = bound_health_candidate("account-failed-model");
        begin_attempt(&state, &mut failed);
        bind_attempt_credentials(&state, &failed, &json!({"accessToken":"T0"}));
        mark_failure(&state, &failed, failure, Some(Duration::from_secs(600)));
        let key = account_health_key(&failed).unwrap();
        let mut sibling = bound_health_candidate("account-success-model");
        begin_attempt(&state, &mut sibling);
        bind_attempt_credentials(&state, &sibling, &json!({"accessToken":"T0"}));
        mark_success(&state, &sibling);
        assert!(!candidate_is_cooling_down(&state, &sibling));
        let mut memory = state.route_memory.lock().unwrap();
        assert!(!clear_account_authentication(
            &mut memory,
            &key,
            &HashSet::new()
        ));
        assert!(memory.accounts.is_empty());
        assert_eq!(
            memory.candidates["account-failed-model"].failure,
            Some(failure)
        );
    }
}

#[test]
fn old_unauthorized_response_is_ignored_after_credentials_rotate() {
    let state = AppState::for_tests().unwrap();
    let mut old = bound_health_candidate("rotated-token");
    begin_attempt(&state, &mut old);
    bind_attempt_credentials(&state, &old, &json!({"accessToken":"T0"}));
    let mut fresh = old.clone();
    begin_attempt(&state, &mut fresh);
    bind_attempt_credentials(&state, &fresh, &json!({"accessToken":"T1"}));
    mark_failure(&state, &old, FailureClass::Authentication, None);
    {
        let memory = state.route_memory.lock().unwrap();
        assert!(memory.candidates.is_empty());
        assert!(memory.accounts.is_empty());
    }
    mark_failure(&state, &fresh, FailureClass::Authentication, None);
    let key = account_health_key(&fresh).unwrap();
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(
        memory.candidates["rotated-token"].failure,
        Some(FailureClass::Authentication)
    );
    assert_eq!(
        memory.accounts[&key].failure,
        Some(FailureClass::Authentication)
    );
}

#[test]
fn late_first_credential_bind_cannot_restore_rejected_token_version() {
    let state = AppState::for_tests().unwrap();
    let mut old = bound_health_candidate("late-token-bind");
    begin_attempt(&state, &mut old);
    let mut fresh = old.clone();
    begin_attempt(&state, &mut fresh);
    bind_attempt_credentials(&state, &fresh, &json!({"accessToken":"T1"}));
    let key = account_health_key(&fresh).unwrap();
    let current_version = state.route_memory.lock().unwrap().credential_versions[&key].clone();
    bind_attempt_credentials(&state, &old, &json!({"accessToken":"T0"}));
    mark_failure(&state, &old, FailureClass::Authentication, None);
    {
        let memory = state.route_memory.lock().unwrap();
        assert_eq!(memory.credential_versions[&key], current_version);
        assert!(memory.candidates.is_empty());
        assert!(memory.accounts.is_empty());
    }
    mark_failure(&state, &fresh, FailureClass::Authentication, None);
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(memory.candidates["late-token-bind"].consecutive_failures, 1);
    assert_eq!(
        memory.accounts[&key].failure,
        Some(FailureClass::Authentication)
    );
}

#[test]
fn reauthentication_only_clears_authentication_and_rejects_old_epoch_results() {
    for provider in [AccountProvider::Kilo, AccountProvider::CodeArts] {
        assert_account_reauthentication(provider);
    }
}

fn assert_account_reauthentication(provider: AccountProvider) {
    let state = AppState::for_tests().unwrap();
    let mut auth = bound_health_candidate("reauthentication-auth");
    auth.account_source.as_mut().unwrap().provider = provider;
    begin_attempt(&state, &mut auth);
    bind_attempt_credentials(&state, &auth, &json!({"accessToken":"T0"}));
    mark_failure(&state, &auth, FailureClass::Authentication, None);
    let mut billing = bound_health_candidate("reauthentication-billing");
    billing.account_source = auth.account_source.clone();
    let mut limited = bound_health_candidate("reauthentication-limited");
    limited.account_source = auth.account_source.clone();
    mark_failure_scoped(&state, &billing, FailureClass::Billing, None, true);
    mark_failure_scoped(
        &state,
        &limited,
        FailureClass::RateLimit,
        Some(Duration::from_secs(600)),
        true,
    );
    let key = account_health_key(&auth).unwrap();
    {
        let mut memory = state.route_memory.lock().unwrap();
        assert!(clear_account_authentication(
            &mut memory,
            &key,
            &HashSet::new()
        ));
        assert!(!memory.candidates.contains_key("reauthentication-auth"));
        assert!(!memory.accounts.contains_key(&key));
        assert_eq!(
            memory.candidates["reauthentication-billing"].failure,
            Some(FailureClass::Billing)
        );
        assert_eq!(
            memory.candidates["reauthentication-limited"].failure,
            Some(FailureClass::RateLimit)
        );
        let epoch = memory.account_epochs.entry(key.clone()).or_default();
        *epoch = epoch.saturating_add(1);
        memory.credential_versions.remove(&key);
        memory.revision = memory.revision.saturating_add(1);
    }
    mark_failure(&state, &auth, FailureClass::Authentication, None);
    mark_success(&state, &auth);
    {
        let memory = state.route_memory.lock().unwrap();
        assert!(!memory.candidates.contains_key("reauthentication-auth"));
        assert!(!memory.accounts.contains_key(&key));
        assert!(memory.candidates.contains_key("reauthentication-billing"));
        assert!(memory.candidates.contains_key("reauthentication-limited"));
    }
    let mut fresh = auth.clone();
    begin_attempt(&state, &mut fresh);
    bind_attempt_credentials(&state, &fresh, &json!({"accessToken":"T1"}));
    mark_failure(&state, &fresh, FailureClass::Authentication, None);
    assert!(state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("reauthentication-auth"));
}

#[test]
fn model_local_long_cooldown_is_not_promoted_to_account_authentication() {
    for (failure, retry_after, model_scope) in [
        (FailureClass::ModelUnavailable, None, false),
        (
            FailureClass::RateLimit,
            Some(Duration::from_secs(24 * 60 * 60)),
            true,
        ),
    ] {
        let state = AppState::for_tests().unwrap();
        let mut local = bound_health_candidate("local-long-cooldown");
        let mut authentication = local.clone();
        begin_attempt(&state, &mut local);
        begin_attempt(&state, &mut authentication);
        bind_attempt_credentials(&state, &local, &json!({"accessToken":"T0"}));
        bind_attempt_credentials(&state, &authentication, &json!({"accessToken":"T0"}));
        mark_failure_scoped(&state, &local, failure, retry_after, model_scope);
        let key = account_health_key(&local).unwrap();
        let long_deadline =
            state.route_memory.lock().unwrap().candidates["local-long-cooldown"].cooldown_until_ms;
        assert!(state.route_memory.lock().unwrap().accounts.is_empty());
        let started = now_ms();
        mark_failure(&state, &authentication, FailureClass::Authentication, None);
        let finished = now_ms();
        let mut memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory.candidates["local-long-cooldown"].failure,
            Some(failure)
        );
        assert_eq!(
            memory.candidates["local-long-cooldown"].cooldown_until_ms,
            long_deadline
        );
        let account = memory.accounts.get_mut(&key).unwrap();
        assert_eq!(account.failure, Some(FailureClass::Authentication));
        let auth_deadline = account.cooldown_until_ms.unwrap();
        assert!(auth_deadline >= started + AUTH_OR_BILLING_COOLDOWN.as_millis() as u64);
        assert!(auth_deadline <= finished + AUTH_OR_BILLING_COOLDOWN.as_millis() as u64);
        assert!(Some(auth_deadline) < long_deadline);
        account.cooldown_until_ms = Some(started.saturating_sub(1));
        drop(memory);
        assert!(!candidate_is_cooling_down(
            &state,
            &bound_health_candidate("local-sibling")
        ));
        assert!(candidate_is_cooling_down(&state, &local));
    }
}

fn typed_body_failure(error: crate::services::account_proxy::ProxyError) -> Response {
    Response::new(Body::from_stream(stream::once(async move {
        Err::<Bytes, std::io::Error>(std::io::Error::other(std::io::Error::other(error)))
    })))
}

#[tokio::test]
async fn plugin_typed_quota_errors_keep_the_model_deadline() {
    for streaming in [false, true] {
        let deadline = now_ms() + 172800000;
        let mut error = crate::services::account_proxy::ProxyError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Model quota limit",
        );
        error.error_code = Some("rate_limit_exceeded".into());
        error.retry_after = Some(HeaderValue::from_static("172800"));
        error.model_scope = true;
        let backup = if streaming {
            Response::new(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"backup\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
        } else {
            crate::services::account_proxy::json_response(
                &json!({"choices":[{"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}]}),
            )
        };
        let fixtures = Arc::new(Mutex::new(HashMap::from([
            ("wb-typed".into(), typed_body_failure(error)),
            ("wb-backup".into(), backup),
        ])));
        let mut state = AppState::for_tests().unwrap();
        state.fixture_responses = Some(fixtures.clone());
        let mut limited = bound_health_candidate("wb-typed");
        limited.account_source.as_mut().unwrap().provider = AccountProvider::WorkBuddy;
        let backup = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "wb-backup");
        let response = route_chat_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"messages":[],"stream":streaming}),
            vec![limited, backup],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "wb-backup/wb-backup-model"
        );
        response.into_body().collect().await.unwrap();
        assert!(fixtures.lock().unwrap().is_empty());
        let memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory.candidates["wb-typed"].failure,
            Some(FailureClass::RateLimit)
        );
        assert!(
            memory.candidates["wb-typed"]
                .cooldown_until_ms
                .unwrap()
                .abs_diff(deadline)
                < 1000
        );
        assert!(memory.accounts.is_empty());
    }
}

#[tokio::test]
async fn early_typed_request_rejection_does_not_cool_down_or_call_backup() {
    for streaming in [false, true] {
        for (status, message) in [
            (StatusCode::FORBIDDEN, "blocked by safety policy"),
            (StatusCode::BAD_REQUEST, "invalid input"),
        ] {
            let mut error = crate::services::account_proxy::ProxyError::new(status, message);
            error.error_code = Some("request_rejected".into());
            let responses = Arc::new(Mutex::new(HashMap::from([
                ("typed-rejection".to_string(), typed_body_failure(error)),
                (
                    "rejection-backup".to_string(),
                    Response::new(Body::from("unused")),
                ),
            ])));
            let mut state = AppState::for_tests().unwrap();
            state.fixture_responses = Some(responses.clone());
            let response = route_chat_with_candidates(
                &state,
                &inbound_headers(),
                json!({"model":SMART_ROUTER_MODEL_ID,"messages":[],"stream":streaming}),
                vec![
                    bound_health_candidate("typed-rejection"),
                    bound_health_candidate("rejection-backup"),
                ],
            )
            .await;
            assert_eq!(response.status(), status);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let error: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(error["error"]["message"], message);
            assert_eq!(error["error"]["code"], "request_rejected");
            let memory = state.route_memory.lock().unwrap();
            assert!(memory.candidates.is_empty());
            assert!(memory.accounts.is_empty());
            let responses = responses.lock().unwrap();
            assert!(!responses.contains_key("typed-rejection"));
            assert!(responses.contains_key("rejection-backup"));
        }
    }
}

#[tokio::test]
async fn first_typed_body_rate_limit_preserves_retry_after_scope_and_calls_backup() {
    for model_scope in [false, true] {
        let mut error = crate::services::account_proxy::ProxyError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Rate limit",
        );
        error.error_code = Some("rate_limit_exceeded".into());
        error.retry_after = Some(HeaderValue::from_static("3600"));
        error.model_scope = model_scope;
        let responses = Arc::new(Mutex::new(HashMap::from([
                ("typed-rate-limit".to_string(), typed_body_failure(error)),
                ("rate-backup".to_string(), Response::new(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"backup\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))),
            ])));
        let mut state = AppState::for_tests().unwrap();
        state.fixture_responses = Some(responses.clone());
        let limited = bound_health_candidate("typed-rate-limit");
        let backup = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "rate-backup");
        let started = now_ms();
        let response = route_chat_with_candidates(
            &state,
            &inbound_headers(),
            json!({"model":SMART_ROUTER_MODEL_ID,"messages":[],"stream":true}),
            vec![limited, backup],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "rate-backup/rate-backup-model"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains("[DONE]"));
        assert!(responses.lock().unwrap().is_empty());
        let memory = state.route_memory.lock().unwrap();
        let limited = &memory.candidates["typed-rate-limit"];
        assert_eq!(limited.failure, Some(FailureClass::RateLimit));
        assert!(limited.cooldown_until_ms.unwrap() >= started + 3_600_000);
        assert!(memory.accounts.is_empty());
    }
}

fn authentication_fixture(initial: Response, retried: Response) -> AppState {
    let mut initial = initial;
    initial
        .extensions_mut()
        .insert(RejectedAccountCredentials(json!({"accessToken":"T0"})));
    let mut state = AppState::for_tests().unwrap();
    state.fixture_responses = Some(Arc::new(Mutex::new(HashMap::from([
        ("refresh-account".into(), initial),
        ("refresh-backup".into(), Response::new(Body::from("backup"))),
    ]))));
    state.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
        result: Ok(json!({"accessToken":"T1"})),
        delay: Duration::ZERO,
        rejections: Vec::new(),
        retry_responses: std::collections::VecDeque::from([retried]),
        sent_credentials: Vec::new(),
    })));
    state
}

fn unauthorized_body() -> Response {
    typed_body_failure(crate::services::account_proxy::ProxyError::new(
        StatusCode::UNAUTHORIZED,
        "Expired account credential",
    ))
}

fn successful_chat_fixture(streaming: bool) -> Response {
    if streaming {
        Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"backup\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
                .unwrap()
    } else {
        crate::services::account_proxy::json_response(
            &json!({"choices":[{"message":{"role":"assistant","content":"backup"},"finish_reason":"stop"}]}),
        )
    }
}

#[tokio::test]
async fn early_authentication_failure_refreshes_once_before_any_output() {
    for protocol in [RouteProtocol::Chat, RouteProtocol::Responses] {
        for streaming in [false, true] {
            for http_unauthorized in [false, true] {
                let initial = if http_unauthorized {
                    crate::services::account_proxy::ProxyError::new(
                        StatusCode::UNAUTHORIZED,
                        "Expired account credential",
                    )
                    .into_response()
                } else {
                    unauthorized_body()
                };
                let successful = if protocol == RouteProtocol::Responses {
                    if streaming {
                        Response::new(Body::from("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-fresh\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"fresh output\"}]}]}}\n\n"))
                    } else {
                        Response::new(Body::from("{\"id\":\"resp-fresh\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"fresh output\"}]}]}"))
                    }
                } else if streaming {
                    Response::new(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"fresh output\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
                } else {
                    Response::new(Body::from("fresh output"))
                };
                let state = authentication_fixture(initial, successful);
                let account = bound_health_candidate("refresh-account");
                let source = account.account_source.clone().unwrap();
                let response = route_with_policy(
                    &state,
                    &HeaderMap::new(),
                    json!({"stream":streaming,"input":"hello","messages":[]}),
                    vec![
                        account,
                        candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "refresh-backup"),
                    ],
                    RoutePolicy::default(),
                    protocol,
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response.headers()["X-EchoBird-Routed-Via"],
                    "refresh-account/refresh-account-model"
                );
                response.into_body().collect().await.unwrap();
                let fixture = state
                    .fixture_authentication
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap();
                assert_eq!(
                    fixture.rejections,
                    vec![(source, json!({"accessToken":"T0"}))]
                );
                assert_eq!(fixture.sent_credentials, vec![json!({"accessToken":"T1"})]);
                assert!(fixture.retry_responses.is_empty());
                assert!(state
                    .fixture_responses
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .contains_key("refresh-backup"));
                let memory = state.route_memory.lock().unwrap();
                assert!(memory.candidates.is_empty());
                assert!(memory.accounts.is_empty());
            }
        }
    }
}

#[tokio::test]
async fn repeated_early_unauthorized_does_not_refresh_again() {
    for streaming in [false, true] {
        let state = authentication_fixture(unauthorized_body(), unauthorized_body());
        state
            .fixture_responses
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .insert("refresh-backup".into(), successful_chat_fixture(streaming));
        let mut backup = candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "refresh-backup");
        // The backup has independent credentials and must remain eligible.
        backup.account_source = None;
        let response = route_chat_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"stream":streaming,"messages":[]}),
            vec![bound_health_candidate("refresh-account"), backup],
        )
        .await;
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "refresh-backup/refresh-backup-model"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(String::from_utf8_lossy(&bytes).contains("backup"));
        assert_eq!(
            state
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .rejections
                .len(),
            1
        );
        let memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory.candidates["refresh-account"].failure,
            Some(FailureClass::Authentication)
        );
        assert_eq!(memory.accounts.len(), 1);
    }
}

#[tokio::test]
async fn authentication_failure_after_delivered_output_is_never_replayed() {
    let partial = Response::new(Body::from_stream(stream::iter([
        Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        )),
        Err(std::io::Error::other(
            crate::services::account_proxy::ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Expired account credential",
            ),
        )),
    ])));
    let state = authentication_fixture(partial, Response::new(Body::from("unused")));
    let response = route_chat_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"stream":true,"messages":[]}),
        vec![
            bound_health_candidate("refresh-account"),
            candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "refresh-backup"),
        ],
    )
    .await;
    let mut output = response.into_body().into_data_stream();
    assert!(String::from_utf8_lossy(&output.next().await.unwrap().unwrap()).contains("partial"));
    assert!(output.next().await.unwrap().is_err());
    assert!(output.next().await.is_none());
    let fixture = state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap();
    assert!(fixture.rejections.is_empty());
    assert_eq!(fixture.retry_responses.len(), 1);
    assert!(state
        .fixture_responses
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .contains_key("refresh-backup"));
}

#[tokio::test]
async fn refresh_preserves_original_budget_and_can_be_cancelled() {
    let state = authentication_fixture(unauthorized_body(), Response::new(Body::from("unused")));
    state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .delay = Duration::from_secs(1);
    let account = bound_health_candidate("refresh-account");
    let policy = RoutePolicy {
        time_budget: Duration::from_millis(20),
        ..RoutePolicy::default()
    };
    let response = route_chat_with_policy(
        &state,
        &HeaderMap::new(),
        json!({"stream":true,"messages":[]}),
        vec![account],
        policy,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    {
        let fixture = state
            .fixture_authentication
            .as_ref()
            .unwrap()
            .lock()
            .unwrap();
        assert_eq!(fixture.rejections.len(), 1);
        assert!(fixture.sent_credentials.is_empty());
        assert_eq!(fixture.retry_responses.len(), 1);
    }
    assert_eq!(
        state.route_memory.lock().unwrap().candidates["refresh-account"].failure,
        Some(FailureClass::Transient)
    );

    let state = authentication_fixture(unauthorized_body(), Response::new(Body::from("unused")));
    state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .delay = Duration::from_secs(1);
    let headers = HeaderMap::new();
    let request = route_chat_with_candidates(
        &state,
        &headers,
        json!({"stream":true,"messages":[]}),
        vec![bound_health_candidate("refresh-account")],
    );
    assert!(tokio::time::timeout(Duration::from_millis(20), request)
        .await
        .is_err());
    let fixture = state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap();
    assert_eq!(fixture.rejections.len(), 1);
    assert!(fixture.sent_credentials.is_empty());
    assert_eq!(fixture.retry_responses.len(), 1);
    let memory = state.route_memory.lock().unwrap();
    assert!(memory.candidates.is_empty());
    assert!(memory.accounts.is_empty());
}

#[tokio::test]
async fn bound_continuation_refreshes_same_account_without_switching() {
    for streaming in [false, true] {
        let state = authentication_fixture(unauthorized_body(), unauthorized_body());
        let account = bound_health_candidate("refresh-account");
        remember_response(&state, &account, Some("resp-bound"));
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"stream":streaming,"input":"continue","previous_response_id":"resp-bound"}),
            vec![
                account,
                candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "refresh-backup"),
            ],
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            state
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .rejections
                .len(),
            1
        );
        assert!(state
            .fixture_responses
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key("refresh-backup"));
    }
}

#[tokio::test]
async fn refresh_failure_keeps_its_rate_limit_classification() {
    let state = authentication_fixture(unauthorized_body(), Response::new(Body::from("unused")));
    state
        .fixture_responses
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .insert("refresh-backup".into(), successful_chat_fixture(true));
    state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .result = Err("accountError.rateLimited|HTTP 429".into());
    let response = route_chat_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"stream":true,"messages":[]}),
        vec![
            bound_health_candidate("refresh-account"),
            candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "refresh-backup"),
        ],
    )
    .await;
    assert_eq!(
        response.headers()["X-EchoBird-Routed-Via"],
        "refresh-backup/refresh-backup-model"
    );
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&bytes).contains("backup"));
    let fixture = state
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap();
    assert_eq!(fixture.rejections.len(), 1);
    assert!(fixture.sent_credentials.is_empty());
    assert_eq!(fixture.retry_responses.len(), 1);
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(
        memory.candidates["refresh-account"].failure,
        Some(FailureClass::RateLimit)
    );
    assert!(memory.accounts.is_empty());
}

#[test]
fn changing_model_clears_candidate_health_without_clearing_account_cooldown() {
    {
        let failure = FailureClass::Authentication;
        let state = AppState::for_tests().unwrap();
        let account = bound_health_candidate("edited-account-model");
        mark_failure(&state, &account, failure, Some(Duration::from_secs(600)));
        let key = account_health_key(&account).unwrap();
        let deadline = state.route_memory.lock().unwrap().accounts[&key].cooldown_until_ms;
        assert!(clear_candidate_health(
            &mut state.route_memory.lock().unwrap(),
            &account.internal_id
        ));
        assert!(candidate_is_cooling_down(
            &state,
            &bound_health_candidate("sibling-account-model")
        ));
        let memory = state.route_memory.lock().unwrap();
        assert!(!memory.candidates.contains_key(&account.internal_id));
        assert_eq!(memory.accounts[&key].failure, Some(failure));
        assert_eq!(memory.accounts[&key].cooldown_until_ms, deadline);
    }
    for failure in [
        FailureClass::Transient,
        FailureClass::ModelUnavailable,
        FailureClass::RateLimit,
        FailureClass::Billing,
    ] {
        let state = AppState::for_tests().unwrap();
        let account = bound_health_candidate("edited-model-local");
        mark_failure(&state, &account, failure, None);
        clear_candidate_health(
            &mut state.route_memory.lock().unwrap(),
            &account.internal_id,
        );
        assert!(!candidate_is_cooling_down(&state, &account));
    }
}

#[tokio::test]
async fn delivered_stream_request_rejection_ends_without_health_or_backup() {
    let error = crate::services::account_proxy::ProxyError::new(
        StatusCode::FORBIDDEN,
        "blocked by safety policy",
    );
    let body = Body::from_stream(stream::iter([
        Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        )),
        Err(std::io::Error::other(error)),
    ]));
    let responses = Arc::new(Mutex::new(HashMap::from([
        ("delivered-rejection".to_string(), Response::new(body)),
        (
            "delivered-backup".to_string(),
            Response::new(Body::from("unused")),
        ),
    ])));
    let mut state = AppState::for_tests().unwrap();
    state.fixture_responses = Some(responses.clone());
    let response = route_chat_with_candidates(
        &state,
        &inbound_headers(),
        json!({"model":SMART_ROUTER_MODEL_ID,"messages":[],"stream":true}),
        vec![
            bound_health_candidate("delivered-rejection"),
            bound_health_candidate("delivered-backup"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut output = response.into_body().into_data_stream();
    let first = output.next().await.unwrap().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("partial"));
    assert!(output.next().await.unwrap().is_err());
    assert!(output.next().await.is_none());
    assert!(responses.lock().unwrap().contains_key("delivered-backup"));
    let memory = state.route_memory.lock().unwrap();
    assert!(memory.candidates.is_empty());
    assert!(memory.accounts.is_empty());
}

#[tokio::test]
async fn converted_responses_try_backup_before_creation_and_preserve_resource_limit() {
    for streaming in [false, true] {
        let request = json!({"model":SMART_ROUTER_MODEL_ID,"input":"hello","stream":streaming});
        let chat = responses_chat_request(&request, false).unwrap();
        let mut error = crate::services::account_proxy::ProxyError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "Trae account is rate limited",
        );
        error.error_code = Some("trae_4011".into());
        error.retry_after = Some(HeaderValue::from_static("120"));
        let first = responses_from_chat(typed_body_failure(error), &chat)
            .await
            .unwrap();
        let backup_chat = Response::builder().header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from("data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"backup output\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")).unwrap();
        let backup = responses_from_chat(backup_chat, &chat).await.unwrap();
        let fixtures = Arc::new(Mutex::new(HashMap::from([
            ("responses-early".into(), first),
            ("responses-backup".into(), backup),
        ])));
        let mut state = AppState::for_tests().unwrap();
        state.fixture_responses = Some(fixtures.clone());
        let mut limited = bound_health_candidate("responses-early");
        limited.account_source.as_mut().unwrap().provider =
            crate::models::model::AccountProvider::Traecn;
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state,
            &inbound_headers(),
            request,
            vec![
                limited,
                candidate(SocketAddr::from(([127, 0, 0, 1], 1)), "responses-backup"),
            ],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "responses-backup/responses-backup-model"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("backup output"));
        assert!(!text.contains("response.failed"));
        if streaming {
            assert_eq!(text.matches("event: response.created").count(), 1);
            assert!(text.contains("response.completed"));
        } else {
            let value: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(value["status"], "completed");
        }
        assert!(fixtures.lock().unwrap().is_empty());
        let memory = state.route_memory.lock().unwrap();
        assert!(memory.accounts.is_empty());
        assert_eq!(
            memory.candidates["responses-early"].failure,
            Some(FailureClass::RateLimit)
        );
        assert!(
            memory.candidates["responses-early"]
                .cooldown_until_ms
                .unwrap()
                >= started + 120_000
        );
    }
}
#[tokio::test]
async fn terminal_observes_real_replacements_without_changing_upstream_payload() {
    let root = std::env::temp_dir().join(format!("terminal-fixture-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let captured = Arc::new(Mutex::new(Value::Null));
    let addr = spawn_mock(Router::new().route(
        "/v1/chat/completions",
        post({
            let captured = captured.clone();
            move |Json(body): Json<Value>| {
                *captured.lock().unwrap() = body;
                async { Json(json!({"choices":[{"message":{"content":"ok"}}]})) }
            }
        }),
    ))
    .await;
    let mut state = AppState::for_tests().unwrap();
    state.http_client = reqwest::Client::builder().no_proxy().build().unwrap();
    state.config_path = root.join("terminal-fixture.json");
    state.protection_settings_path = root.join("settings.json");
    state.statistics_collector =
        Some(super::super::statistics::Collector::open(root.join("counts.json")).unwrap());
    let group = "terminal-fixture";
    let session = uuid::Uuid::new_v4().to_string();
    let content = format!("{} password = DemoPass!2026", "x".repeat(226));
    for enabled in [true, false] {
        fs::write(
            &state.protection_settings_path,
            json!({"sensitiveInfoProtection":enabled}).to_string(),
        )
        .unwrap();
        super::super::terminal::poll(&session, group);
        let response = protected_route(
            &state,
            &HeaderMap::new(),
            json!({"model":"auto","messages":[{"role":"user","content":content}]}),
            vec![candidate(addr, "fixture")],
            ClientProtocol::Chat,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response.into_body().collect().await.unwrap();
        let rows = super::super::terminal::poll(&session, group);
        assert!(!rows.is_empty());
        let segments: Vec<_> = rows.iter().flatten().collect();
        let displayed: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert!(displayed.contains("DemoPass!2026"), "enabled={enabled}");
        assert_eq!(segments.iter().any(|s| s.protected_text.is_some()), enabled);
        let sent = captured.lock().unwrap().to_string();
        assert_eq!(sent.contains("DemoPass!2026"), !enabled);
        assert_eq!(sent.contains("{{SECRET_"), enabled);
    }
    super::super::terminal::stop(&session);
    assert!(super::super::terminal::ticket(group).is_none());
    fs::remove_dir_all(root).unwrap();
}
