use std::collections::VecDeque;
use std::net::SocketAddr;

use axum::extract::OriginalUri;

use super::*;

struct HttpFixture {
    calls: Mutex<Vec<(String, Value)>>,
    native: Mutex<VecDeque<Response>>,
    chat: Mutex<VecDeque<Response>>,
}

struct MockServer {
    addr: SocketAddr,
    fixture: Arc<HttpFixture>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl MockServer {
    async fn new(native: Vec<Response>, chat: Vec<Response>) -> Self {
        let fixture = Arc::new(HttpFixture {
            calls: Mutex::new(Vec::new()),
            native: Mutex::new(native.into()),
            chat: Mutex::new(chat.into()),
        });
        let app = Router::new()
            .route("/v1/responses", post(record_request))
            .route("/v1/chat/completions", post(record_request))
            .with_state(fixture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            fixture,
            task,
        }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.fixture.calls.lock().unwrap().clone()
    }

    fn candidate(&self, id: &str) -> Candidate {
        let model_id = format!("{id}-model");
        let base_url = format!("http://{}/v1", self.addr);
        let api_key = "upstream-key".to_string();
        Candidate {
            internal_id: id.into(),
            fingerprint: candidate_fingerprint(&model_id, &base_url, &api_key),
            model_id,
            base_url,
            api_key,
            account_source: None,
            attempt: None,
        }
    }
}

async fn record_request(
    State(fixture): State<Arc<HttpFixture>>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Response {
    let path = uri.path().to_string();
    fixture.calls.lock().unwrap().push((path.clone(), body));
    let replies = if path == "/v1/responses" {
        &fixture.native
    } else {
        &fixture.chat
    };
    let response = replies.lock().unwrap().pop_front().unwrap_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"Unexpected fixture request"}})),
        )
            .into_response()
    });
    response
}

fn native_json(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "object": "response",
        "status": "completed",
        "model": "actual-upstream-model",
        "error": null,
        "output": [{"id":"msg_native","type":"message","role":"assistant",
            "status":"completed","content":[{"type":"output_text","text":text,
                "annotations":[]}]}],
        "usage": {"input_tokens":3,"output_tokens":2,"total_tokens":5}
    })
}

fn native_ok(id: &str, text: &str) -> Response {
    Json(native_json(id, text)).into_response()
}

fn chat_ok(text: &str) -> Response {
    let delta = json!({"id":"chatcmpl-fixture","object":"chat.completion.chunk",
        "model":"chat-model","choices":[{"index":0,"delta":{"role":"assistant",
        "content":text},"finish_reason":null}]});
    let finished = json!({"id":"chatcmpl-fixture","object":"chat.completion.chunk",
        "model":"chat-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}});
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(format!(
            "data: {delta}\n\ndata: {finished}\n\ndata: [DONE]\n\n"
        )))
        .unwrap()
}

fn endpoint_missing(status: StatusCode) -> Response {
    (
        status,
        Json(json!({"error":{"code":"endpoint_not_found",
            "message":"POST /v1/responses is not supported"}})),
    )
        .into_response()
}

fn sse(event: &str, data: &Value) -> String {
    format!("event: {event}\r\ndata: {data}\r\n\r\n")
}

async fn response_json(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn native_json_preserves_request_fields_and_complete_upstream_response() {
    let mut expected = native_json("resp_native_preserved", "native output");
    expected["output"].as_array_mut().unwrap().push(json!({
        "type":"reasoning","id":"rs_native","encrypted_content":"opaque",
        "summary":[{"type":"summary_text","text":"summary"}]
    }));
    expected["output"][0]["content"][0]["annotations"] = json!([
        {"type":"url_citation","url":"https://example.com","title":"source",
            "start_index":0,"end_index":6}
    ]);
    expected["vendor_extension"] = json!({"opaque":"kept"});
    let native = MockServer::new(vec![Json(expected.clone()).into_response()], vec![]).await;
    let backup = MockServer::new(vec![native_ok("resp_backup", "backup")], vec![]).await;
    let request = json!({
        "model":SMART_ROUTER_MODEL_ID,
        "input":[{"type":"reasoning","id":"rs_previous","encrypted_content":"sealed"},
            {"type":"message","role":"user","content":[{"type":"input_text","text":"question"}]},
            {"type":"function_call","id":"fc_input","call_id":"call_1","name":"lookup","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"answer"}],
        "instructions":"preserve these instructions",
        "tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}},
            {"type":"web_search"}],
        "reasoning":{"effort":"high"},"include":["reasoning.encrypted_content"],
        "store":true,"metadata":{"request_id":"metadata-id"},
        "text":{"format":{"type":"json_schema","name":"result","schema":{"type":"object"}}},
        "vendor_extension":{"opaque":"request-kept"},"stream":false
    });
    let state = AppState::for_tests().unwrap();
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        request.clone(),
        vec![
            native.candidate("native-preserved"),
            backup.candidate("backup-preserved"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, expected);
    let mut forwarded = request;
    forwarded["model"] = json!("native-preserved-model");
    assert_eq!(native.calls(), vec![("/v1/responses".into(), forwarded)]);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn native_stream_preserves_unknown_events_ids_annotations_and_error_null() {
    let wire = [
        sse(
            "response.created",
            &json!({"type":"response.created","response":{
            "id":"resp_stream_preserved","status":"in_progress","error":null}}),
        ),
        sse(
            "response.future_extension",
            &json!({"type":"response.future_extension",
            "opaque":{"retained":true},"sequence_number":1}),
        ),
        sse(
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta",
            "item_id":"msg_native","output_index":0,"content_index":0,"delta":"中文"}),
        ),
        sse(
            "response.output_text.annotation.added",
            &json!({
            "type":"response.output_text.annotation.added","item_id":"msg_native",
            "annotation":{"type":"url_citation","url":"https://example.com"}}),
        ),
        sse(
            "response.completed",
            &json!({"type":"response.completed",
            "response":native_json("resp_stream_preserved", "中文")}),
        ),
    ]
    .concat();
    let chunks = wire
        .as_bytes()
        .chunks(3)
        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
        .collect::<Vec<_>>();
    let reply = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(stream::iter(chunks)))
        .unwrap();
    let native = MockServer::new(vec![reply], vec![]).await;
    let backup = MockServer::new(vec![native_ok("resp_stream_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let first = native.candidate("native-stream-preserved");
    mark_failure(&state, &first, FailureClass::Transient, None);
    state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .get_mut(&first.internal_id)
        .unwrap()
        .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true}),
        vec![first.clone(), backup.candidate("native-stream-backup")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let returned = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(returned.as_ref(), wire.as_bytes());
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key(&first.internal_id));
    assert_eq!(native.calls().len(), 1);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn missing_native_endpoint_falls_back_to_chat_without_trying_backup() {
    for status in [StatusCode::NOT_FOUND, StatusCode::METHOD_NOT_ALLOWED] {
        let first =
            MockServer::new(vec![endpoint_missing(status)], vec![chat_ok("via chat")]).await;
        let backup =
            MockServer::new(vec![native_ok("resp_unneeded_backup", "backup")], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":false}),
            vec![
                first.candidate("chat-fallback"),
                backup.candidate("fallback-backup"),
            ],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let returned = response_json(response).await;
        assert_eq!(returned["object"], "response");
        assert_eq!(returned["output"][0]["content"][0]["text"], "via chat");
        let calls = first.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, "/v1/responses");
        assert_eq!(calls[1].0, "/v1/chat/completions");
        assert_eq!(calls[1].1["model"], "chat-fallback-model");
        assert_eq!(calls[1].1["messages"][0]["content"], "question");
        assert_eq!(calls[1].1["stream"], true);
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn converted_account_body_errors_preserve_auth_rate_limits_and_model_scope() {
    use crate::models::model::{AccountProvider, AccountSource};
    use crate::services::account_proxy::ProxyError;

    let date = (chrono::Utc::now() + chrono::Duration::seconds(150)).to_rfc2822();
    for (status, code, retry_after, model_scope, minimum_cooldown) in [
        (
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            None,
            false,
            3_590_000,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            Some("180".to_string()),
            false,
            175_000,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            Some(date.clone()),
            true,
            140_000,
        ),
    ] {
        let fixture = MockServer::new(vec![], vec![]).await;
        let mut candidate = fixture.candidate("converted-account-error");
        candidate.account_source = Some(AccountSource {
            provider: AccountProvider::WorkBuddy,
            account_id: "fixture-account".into(),
        });
        let backup = fixture.candidate("unused-converted-backup");
        let mut error = ProxyError::new(status, "Account rejected this turn");
        error.error_code = Some(code.into());
        error.error_type = Some(code.into());
        error.retry_after = retry_after
            .as_deref()
            .map(|value| HeaderValue::from_str(value).unwrap());
        error.model_scope = model_scope;
        let chat = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream::once(async move {
                Err::<Bytes, _>(std::io::Error::other(error))
            })))
            .unwrap();
        let request = json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true});
        let converted = responses_from_chat(chat, &json!({"_echobird_responses":request}))
            .await
            .unwrap();
        let backup_reply =
            responses_from_chat(chat_ok("backup"), &json!({"_echobird_responses":request}))
                .await
                .unwrap();
        let mut state = AppState::for_tests().unwrap();
        state.fixture_responses = Some(Arc::new(Mutex::new(HashMap::from([
            (candidate.internal_id.clone(), converted),
            (backup.internal_id.clone(), backup_reply),
        ]))));
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            request,
            vec![candidate.clone(), backup.clone()],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "unused-converted-backup/unused-converted-backup-model"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let events = crate::services::account_proxy::SseParser::default().push(&bytes);
        assert!(String::from_utf8_lossy(&bytes).contains("backup"));
        assert!(!events.iter().any(|(event, _)| event == "response.failed"));
        assert!(events
            .iter()
            .any(|(event, _)| event == "response.completed"));
        let memory = state.route_memory.lock().unwrap();
        let health = memory.candidates.get(&candidate.internal_id).unwrap();
        assert_eq!(
            health.failure,
            Some(if status == StatusCode::UNAUTHORIZED {
                FailureClass::Authentication
            } else {
                FailureClass::RateLimit
            })
        );
        assert!(health.cooldown_until_ms.unwrap() >= started + minimum_cooldown);
        assert_eq!(
            memory
                .accounts
                .contains_key(&account_health_key(&candidate).unwrap()),
            status == StatusCode::UNAUTHORIZED && !model_scope
        );
        assert!(state
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .is_empty());
        drop(memory);
        assert!(!state
            .fixture_responses
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .contains_key(&backup.internal_id));
        assert!(
            fixture.calls().is_empty(),
            "fixture account must not read native credentials"
        );
    }
}

#[tokio::test]
async fn chat_fallback_sse_errors_keep_header_and_usage_limit_recovery() {
    for retry_after in [None, Some("120")] {
        let detail = json!({"error":{"type":"usage_limit_reached","message":"Rate limit reached",
            "resets_in_seconds":210}});
        let mut reply = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(format!("data: {detail}\n\n")))
            .unwrap();
        if let Some(value) = retry_after {
            reply
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static(value));
        }
        let first =
            MockServer::new(vec![endpoint_missing(StatusCode::NOT_FOUND)], vec![reply]).await;
        let backup_reply = Response::builder().header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(sse("response.completed", &json!({"type":"response.completed","response":native_json("resp_error_backup","backup")})))).unwrap();
        let backup = MockServer::new(vec![backup_reply], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let candidate = first.candidate("chat-sse-limit");
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true}),
            vec![
                candidate.clone(),
                backup.candidate("chat-sse-unused-backup"),
            ],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "chat-sse-unused-backup/chat-sse-unused-backup-model"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let events = crate::services::account_proxy::SseParser::default().push(&bytes);
        assert!(String::from_utf8_lossy(&bytes).contains("backup"));
        assert!(events
            .iter()
            .any(|(event, _)| event == "response.completed"));
        assert!(!events.iter().any(|(event, _)| event == "response.failed"));
        let seconds = if retry_after.is_some() { 120 } else { 210 };
        let memory = state.route_memory.lock().unwrap();
        let health = memory.candidates.get(&candidate.internal_id).unwrap();
        assert_eq!(health.failure, Some(FailureClass::RateLimit));
        assert!(health.cooldown_until_ms.unwrap() >= started + (seconds - 1) * 1000);
        assert!(state
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .contains_key("resp_error_backup"));
        assert_eq!(first.calls().len(), 2);
        assert_eq!(backup.calls().len(), 1);
    }
}

#[tokio::test]
async fn model_errors_limits_and_bad_input_never_repeat_as_chat_requests() {
    for (status, error, should_fail_over) in [
        (
            StatusCode::NOT_FOUND,
            json!({"code":"model_not_found","message":"Unknown model"}),
            true,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"code":"rate_limit_exceeded","message":"Rate limit"}),
            true,
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"code":"invalid_request_error","message":"Invalid input"}),
            false,
        ),
    ] {
        let first = MockServer::new(
            vec![(status, Json(json!({"error":error}))).into_response()],
            vec![chat_ok("must not replay")],
        )
        .await;
        let backup = MockServer::new(
            vec![native_ok("resp_after_failure", "native backup")],
            vec![],
        )
        .await;
        let state = AppState::for_tests().unwrap();
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":false}),
            vec![
                first.candidate("no-chat-repeat"),
                backup.candidate("failure-backup"),
            ],
        )
        .await;
        if should_fail_over {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_json(response).await["id"], "resp_after_failure");
            assert_eq!(backup.calls().len(), 1);
        } else {
            assert_eq!(response.status(), status);
            assert!(response_json(response).await.get("error").is_some());
            assert!(backup.calls().is_empty());
        }
        assert_eq!(first.calls().len(), 1);
        assert_eq!(first.calls()[0].0, "/v1/responses");
    }
}

#[tokio::test]
async fn hosted_tools_skip_incompatible_chat_fallback_and_reach_native_backup() {
    let first = MockServer::new(
        vec![endpoint_missing(StatusCode::NOT_FOUND)],
        vec![chat_ok("must not erase tool")],
    )
    .await;
    let backup = MockServer::new(vec![native_ok("resp_hosted_tool", "tool output")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let request = json!({"model":SMART_ROUTER_MODEL_ID,"input":"search question",
        "tools":[{"type":"web_search"}],"stream":false});
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        request.clone(),
        vec![
            first.candidate("hosted-chat-only"),
            backup.candidate("hosted-native"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["id"], "resp_hosted_tool");
    assert_eq!(first.calls().len(), 1);
    assert_eq!(first.calls()[0].0, "/v1/responses");
    let mut forwarded = request;
    forwarded["model"] = json!("hosted-native-model");
    assert_eq!(backup.calls(), vec![("/v1/responses".into(), forwarded)]);
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key("hosted-chat-only"));
}

#[tokio::test]
async fn the_last_chat_candidate_answers_without_hosted_tools() {
    let only = MockServer::new(
        vec![endpoint_missing(StatusCode::NOT_FOUND)],
        vec![chat_ok("answered without search")],
    )
    .await;
    let state = AppState::for_tests().unwrap();
    let request = json!({"model":SMART_ROUTER_MODEL_ID,"input":"search question",
        "tools":[{"type":"web_search"},{"type":"function","name":"exec","parameters":{}}],
        "tool_choice":{"type":"web_search"},"stream":false});
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        request,
        vec![only.candidate("hosted-last-chat")],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let calls = only.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "/v1/responses");
    assert_eq!(calls[1].0, "/v1/chat/completions");
    assert_eq!(calls[1].1["tools"].as_array().unwrap().len(), 1);
    assert_eq!(calls[1].1["tools"][0]["function"]["name"], "exec");
    assert!(calls[1].1.get("tool_choice").is_none());
}

#[tokio::test]
async fn continuation_keeps_original_candidate_after_reorder_but_full_history_uses_new_order() {
    let first = MockServer::new(
        vec![
            native_ok("resp_bound", "first"),
            native_ok("resp_continued", "continued"),
        ],
        vec![],
    )
    .await;
    let second = MockServer::new(vec![native_ok("resp_new_history", "new history")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let a = first.candidate("affinity-first");
    let b = second.candidate("affinity-second");
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"first question","stream":false}),
        vec![a.clone(), b.clone()],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["id"], "resp_bound");
    let continuation = json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_bound",
        "input":"follow-up","stream":false});
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        continuation.clone(),
        vec![b.clone(), a.clone()],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["id"], "resp_continued");
    assert!(second.calls().is_empty());
    let mut forwarded = continuation;
    forwarded["model"] = json!("affinity-first-model");
    assert_eq!(first.calls()[1], ("/v1/responses".into(), forwarded));
    let response = route_responses_with_candidates(
        &state, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":[{"role":"user","content":"complete history"}],"stream":false}),
        vec![b, a],
    ).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await["id"], "resp_new_history");
    assert_eq!(first.calls().len(), 2);
    assert_eq!(second.calls().len(), 1);
}

#[tokio::test]
async fn continuation_failures_do_not_switch_candidate_or_replay_as_chat() {
    for status in [
        StatusCode::UNAUTHORIZED,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::BAD_GATEWAY,
    ] {
        let error = (
            status,
            Json(json!({"error":{"message":"Continuation failed"}})),
        )
            .into_response();
        let first = MockServer::new(
            vec![native_ok("resp_failed_binding", "first"), error],
            vec![chat_ok("must not repeat")],
        )
        .await;
        let backup = MockServer::new(
            vec![native_ok("resp_wrong_account", "must not switch")],
            vec![],
        )
        .await;
        let state = AppState::for_tests().unwrap();
        let a = first.candidate("failed-affinity");
        let b = backup.candidate("forbidden-affinity-backup");
        let initial = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"first"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(response_json(initial).await["id"], "resp_failed_binding");
        let response = route_responses_with_candidates(
            &state, &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_failed_binding","input":"follow-up"}),
            vec![b, a],
        ).await;
        assert!(!response.status().is_success());
        assert!(response_json(response).await.get("error").is_some());
        assert_eq!(first.calls().len(), 2);
        assert!(first.calls().iter().all(|call| call.0 == "/v1/responses"));
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn unknown_removed_or_reconfigured_continuation_is_rejected_without_upstream_calls() {
    let first = MockServer::new(vec![native_ok("resp_configuration_bound", "first")], vec![]).await;
    let backup = MockServer::new(
        vec![native_ok("resp_forbidden_rebind", "must not switch")],
        vec![],
    )
    .await;
    let state = AppState::for_tests().unwrap();
    let a = first.candidate("config-affinity");
    let b = backup.candidate("config-backup");
    let initial = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"first"}),
        vec![a.clone(), b.clone()],
    )
    .await;
    assert_eq!(
        response_json(initial).await["id"],
        "resp_configuration_bound"
    );
    let mut changed = a.clone();
    changed.api_key = "replaced-credential".into();
    changed.fingerprint =
        candidate_fingerprint(&changed.model_id, &changed.base_url, &changed.api_key);
    for (id, candidates) in [
        ("resp_unknown", vec![a.clone(), b.clone()]),
        ("resp_configuration_bound", vec![b.clone()]),
        ("resp_configuration_bound", vec![changed, b.clone()]),
    ] {
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":id,"input":"follow-up"}),
            candidates,
        )
        .await;
        assert!(response.status().is_client_error());
        assert!(response_json(response).await.get("error").is_some());
        assert_eq!(first.calls().len(), 1);
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn delivered_native_failure_preserves_event_and_cooldown_without_replaying() {
    let first = [
        sse(
            "response.created",
            &json!({"type":"response.created","response":{
                "id":"resp_native_failed","status":"in_progress","error":null}}),
        ),
        sse(
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta","item_id":"msg_native_failed",
                "output_index":0,"content_index":0,"delta":"partial output before failure"}),
        ),
    ]
    .concat();
    let failure = sse(
        "response.failed",
        &json!({"type":"response.failed","response":{"id":"resp_native_failed",
            "status":"failed","error":{"code":"rate_limit_exceeded","message":"Rate limit"}}}),
    );
    let release = Arc::new(tokio::sync::Notify::new());
    let released = release.clone();
    let first_bytes = Bytes::copy_from_slice(first.as_bytes());
    let failure_bytes = Bytes::copy_from_slice(failure.as_bytes());
    let source = stream::once(async { Ok::<_, std::io::Error>(first_bytes) }).chain(stream::once(
        async move {
            released.notified().await;
            Ok::<_, std::io::Error>(failure_bytes)
        },
    ));
    let reply = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::RETRY_AFTER, "120")
        .body(Body::from_stream(source))
        .unwrap();
    let native = MockServer::new(vec![reply], vec![]).await;
    let backup = MockServer::new(vec![native_ok("resp_failure_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let started = now_ms();
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true}),
            vec![
                native.candidate("delivered-native-failure"),
                backup.candidate("native-failure-backup"),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut returned = body.next().await.unwrap().unwrap().to_vec();
    assert_eq!(returned, first.as_bytes());
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(chunk) = body.next().await {
            match chunk {
                Ok(bytes) => returned.extend_from_slice(&bytes),
                Err(_) => break,
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(returned, [first, failure].concat().as_bytes());
    let memory = state.route_memory.lock().unwrap();
    let health = &memory.candidates["delivered-native-failure"];
    assert_eq!(health.failure, Some(FailureClass::RateLimit));
    assert!(health.cooldown_until_ms.unwrap() >= started + 120_000);
    assert_eq!(native.calls().len(), 1);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn truncated_native_stream_is_failure_and_never_replayed_after_delivery() {
    let wire = sse(
        "response.output_text.delta",
        &json!({"type":"response.output_text.delta","item_id":"msg_truncated",
            "output_index":0,"content_index":0,"delta":"partial output"}),
    );
    let reply = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(wire.clone()))
        .unwrap();
    let native = MockServer::new(vec![reply], vec![]).await;
    let backup = MockServer::new(vec![native_ok("resp_truncation_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true}),
        vec![
            native.candidate("native-truncated"),
            backup.candidate("native-truncation-backup"),
        ],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    assert_eq!(
        body.next().await.unwrap().unwrap().as_ref(),
        wire.as_bytes()
    );
    assert!(body.next().await.unwrap().is_err());
    assert!(body.next().await.is_none());
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(
        memory.candidates["native-truncated"].failure,
        Some(FailureClass::Transient)
    );
    assert_eq!(native.calls().len(), 1);
    assert!(backup.calls().is_empty());
}

struct PendingNativeStream {
    first: Option<Bytes>,
    dropped: Option<tokio::sync::oneshot::Sender<()>>,
}

impl futures_util::Stream for PendingNativeStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.first.take() {
            Some(bytes) => std::task::Poll::Ready(Some(Ok(bytes))),
            None => std::task::Poll::Pending,
        }
    }
}

impl Drop for PendingNativeStream {
    fn drop(&mut self) {
        if let Some(sender) = self.dropped.take() {
            let _ = sender.send(());
        }
    }
}

fn candidate_has_activity(id: &str) -> bool {
    ROUTE_ACTIVITY
        .get()
        .unwrap()
        .lock()
        .unwrap()
        .active_attempts
        .values()
        .any(|active| active == id)
}

#[tokio::test]
async fn dropping_native_body_releases_activity_and_pending_upstream_without_failure() {
    let (sender, mut dropped) = tokio::sync::oneshot::channel();
    let first = [
        sse(
            "response.created",
            &json!({"type":"response.created","response":{
                "id":"resp_cancelled_native","status":"in_progress","error":null}}),
        ),
        sse(
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta","item_id":"msg_cancelled_native",
                "output_index":0,"content_index":0,"delta":"partial output before cancellation"}),
        ),
    ]
    .concat();
    let reply = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(PendingNativeStream {
            first: Some(Bytes::copy_from_slice(first.as_bytes())),
            dropped: Some(sender),
        }))
        .unwrap();
    let native = MockServer::new(vec![reply], vec![]).await;
    let backup = MockServer::new(vec![native_ok("resp_cancel_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let id = "native-cancel-fixture";
    let response = tokio::time::timeout(
        Duration::from_secs(5),
        route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":true}),
            vec![
                native.candidate(id),
                backup.candidate("native-cancel-backup"),
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    assert_eq!(
        body.next().await.unwrap().unwrap().as_ref(),
        first.as_bytes()
    );
    assert!(candidate_has_activity(id));
    assert!(matches!(
        dropped.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    drop(body);
    tokio::time::timeout(Duration::from_secs(5), dropped)
        .await
        .unwrap()
        .unwrap();
    assert!(!candidate_has_activity(id));
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key(id));
    assert_eq!(native.calls().len(), 1);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn native_failed_json_preserves_top_level_error_health_and_never_binds_failed_id() {
    for (status, code, failure) in [
        (
            StatusCode::UNAUTHORIZED,
            "invalid_api_key",
            FailureClass::Authentication,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_exceeded",
            FailureClass::RateLimit,
        ),
    ] {
        let mut failed = native_json("resp_failed_json", "");
        failed["status"] = json!("failed");
        failed["output"] = json!([]);
        failed["error"] = json!({"code":code,"status":status.as_u16(),"message":"Account failed"});
        let mut reply = Json(failed.clone()).into_response();
        reply
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("120"));
        let native = MockServer::new(vec![reply], vec![]).await;
        let backup =
            MockServer::new(vec![native_ok("resp_failed_json_backup", "backup")], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let a = native.candidate("native-failed-json");
        let b = backup.candidate("failed-json-backup");
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":false}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await, failed);
        {
            let memory = state.route_memory.lock().unwrap();
            let health = &memory.candidates[&a.internal_id];
            assert_eq!(health.failure, Some(failure));
            let minimum = if failure == FailureClass::RateLimit {
                120_000
            } else {
                3_600_000
            };
            assert!(health.cooldown_until_ms.unwrap() >= started + minimum);
            assert!(!state
                .response_memory
                .lock()
                .unwrap()
                .response_bindings
                .contains_key("resp_failed_json"));
        }
        let continuation = route_responses_with_candidates(
            &state, &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_failed_json","input":"follow-up"}),
            vec![a, b],
        ).await;
        assert_eq!(continuation.status(), StatusCode::BAD_REQUEST);
        assert!(response_json(continuation).await.get("error").is_some());
        assert_eq!(native.calls().len(), 1);
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn conflicting_native_response_ids_cannot_silently_rebind_continuation() {
    let first = MockServer::new(vec![native_ok("resp_collision", "first provider")], vec![]).await;
    let second =
        MockServer::new(vec![native_ok("resp_collision", "second provider")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let a = first.candidate("collision-first");
    let b = second.candidate("collision-second");
    for (index, candidates) in [vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]]
        .into_iter()
        .enumerate()
    {
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"}),
            candidates,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await["id"], "resp_collision");
        if index == 0 {
            state
                .response_memory
                .lock()
                .unwrap()
                .response_bindings
                .get_mut("resp_collision")
                .unwrap()
                .expires_at_ms = now_ms() + 60_000;
        }
    }
    {
        let memory = state.response_memory.lock().unwrap();
        let binding = &memory.response_bindings["resp_collision"];
        assert!(binding.ambiguous);
        assert!(binding.expires_at_ms >= now_ms() + 23 * 60 * 60 * 1000);
    }
    let continuation = route_responses_with_candidates(
        &state, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_collision","input":"follow-up"}),
        vec![a, b],
    ).await;
    assert_eq!(continuation.status(), StatusCode::BAD_REQUEST);
    assert!(response_json(continuation).await.get("error").is_some());
    assert_eq!(first.calls().len(), 1);
    assert_eq!(second.calls().len(), 1);
}

#[tokio::test]
async fn continuation_keeps_unrelated_candidate_rate_limit_and_authentication_cooldowns() {
    for failure in [FailureClass::RateLimit, FailureClass::Authentication] {
        let first = MockServer::new(
            vec![
                native_ok("resp_health_bound", "first"),
                native_ok("resp_health_followup", "follow-up"),
            ],
            vec![],
        )
        .await;
        let second = MockServer::new(vec![native_ok("resp_health_other", "other")], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let a = first.candidate("health-bound");
        let b = second.candidate("health-unrelated");
        let initial = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"first"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(response_json(initial).await["id"], "resp_health_bound");
        mark_failure(&state, &b, failure, Some(Duration::from_secs(600)));
        let before = state.route_memory.lock().unwrap().candidates[&b.internal_id].clone();
        let continuation = route_responses_with_candidates(
            &state, &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_health_bound","input":"follow-up"}),
            vec![b.clone(), a],
        ).await;
        assert_eq!(continuation.status(), StatusCode::OK);
        assert_eq!(
            response_json(continuation).await["id"],
            "resp_health_followup"
        );
        let memory = state.route_memory.lock().unwrap();
        let after = &memory.candidates[&b.internal_id];
        assert_eq!(after.failure, before.failure);
        assert_eq!(after.consecutive_failures, before.consecutive_failures);
        assert_eq!(after.cooldown_until_ms, before.cooldown_until_ms);
        assert_eq!(first.calls().len(), 2);
        assert!(second.calls().is_empty());
    }
}

#[tokio::test]
async fn native_continuation_sse_errors_only_exempt_request_scoped_statuses_from_cooldown() {
    for (status, expected) in [
        (StatusCode::BAD_REQUEST, None),
        (StatusCode::NOT_FOUND, None),
        (StatusCode::CONFLICT, None),
        (StatusCode::UNPROCESSABLE_ENTITY, None),
        (StatusCode::UNAUTHORIZED, Some(FailureClass::Authentication)),
        (StatusCode::TOO_MANY_REQUESTS, Some(FailureClass::RateLimit)),
    ] {
        let wire = sse(
            "response.failed",
            &json!({"type":"response.failed","response":{
            "id":"resp_continuation_failed","object":"response","status":"failed",
            "error":{"code":"response_not_found","message":"previous_response_id expired",
                "status":status.as_u16()}}}),
        );
        let reply = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::RETRY_AFTER, "120")
            .body(Body::from(wire.clone()))
            .unwrap();
        let native = MockServer::new(
            vec![native_ok("resp_continuation_initial", "first"), reply],
            vec![],
        )
        .await;
        let backup =
            MockServer::new(vec![native_ok("resp_continuation_wrong", "backup")], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let a = native.candidate("continuation-error-health");
        let b = backup.candidate("continuation-health-backup");
        let initial = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"first"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(
            response_json(initial).await["id"],
            "resp_continuation_initial"
        );
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state, &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_continuation_initial","input":"follow-up","stream":true}),
            vec![b, a.clone()],
        ).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .as_ref(),
            wire.as_bytes()
        );
        let memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory
                .candidates
                .get(&a.internal_id)
                .and_then(|health| health.failure),
            expected
        );
        if expected == Some(FailureClass::RateLimit) {
            assert!(
                memory.candidates[&a.internal_id].cooldown_until_ms.unwrap() >= started + 120_000
            );
        }
        assert!(!state
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .contains_key("resp_continuation_failed"));
        assert_eq!(native.calls().len(), 2);
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn background_and_conversation_requests_are_rejected_before_any_upstream_call() {
    let native = MockServer::new(
        vec![native_ok("resp_null_conversation", "normal request")],
        vec![],
    )
    .await;
    let backup = MockServer::new(vec![native_ok("resp_background_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let candidates = vec![
        native.candidate("unsupported-lifecycle"),
        backup.candidate("lifecycle-backup"),
    ];
    for extra in [
        json!({"background":true}),
        json!({"conversation":"conv_existing"}),
        json!({"conversation":{"id":"conv_existing"}}),
    ] {
        let mut request = json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"});
        request
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let response =
            route_responses_with_candidates(&state, &HeaderMap::new(), request, candidates.clone())
                .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response_json(response).await.get("error").is_some());
        assert!(native.calls().is_empty());
        assert!(backup.calls().is_empty());
    }
    let response = route_responses_with_candidates(
        &state, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","background":false,"conversation":null}),
        candidates,
    ).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["id"],
        "resp_null_conversation"
    );
    assert_eq!(native.calls().len(), 1);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn continuation_binding_survives_memory_restore_and_expiry_never_sends_upstream() {
    let native = MockServer::new(
        vec![
            native_ok("resp_persisted_binding", "first"),
            native_ok("resp_restored_followup", "follow-up"),
        ],
        vec![],
    )
    .await;
    let backup = MockServer::new(vec![native_ok("resp_restored_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let a = native.candidate("persisted-affinity");
    let b = backup.candidate("persisted-backup");
    let initial = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"do-not-persist-conversation"}),
        vec![a.clone(), b.clone()],
    )
    .await;
    assert_eq!(response_json(initial).await["id"], "resp_persisted_binding");
    let serialized = serde_json::to_vec(&*state.response_memory.lock().unwrap()).unwrap();
    let stored = std::str::from_utf8(&serialized).unwrap();
    assert!(!stored.contains("do-not-persist-conversation"));
    assert!(!stored.contains("upstream-key"));
    let restored = AppState::for_tests().unwrap();
    *restored.response_memory.lock().unwrap() = serde_json::from_slice(&serialized).unwrap();
    let response = route_responses_with_candidates(
        &restored, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_persisted_binding","input":"follow-up"}),
        vec![b.clone(), a.clone()],
    ).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response_json(response).await["id"],
        "resp_restored_followup"
    );
    restored
        .response_memory
        .lock()
        .unwrap()
        .response_bindings
        .get_mut("resp_restored_followup")
        .unwrap()
        .expires_at_ms = now_ms().saturating_sub(1);
    let expired = route_responses_with_candidates(
        &restored, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_restored_followup","input":"another follow-up"}),
        vec![a, b],
    ).await;
    assert_eq!(expired.status(), StatusCode::BAD_REQUEST);
    assert!(response_json(expired).await.get("error").is_some());
    assert_eq!(native.calls().len(), 2);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn native_incomplete_is_preserved_as_terminal_output_and_can_continue() {
    let mut incomplete = native_json("resp_incomplete_binding", "partial answer");
    incomplete["status"] = json!("incomplete");
    incomplete["incomplete_details"] = json!({"reason":"max_output_tokens"});
    let native = MockServer::new(
        vec![
            Json(incomplete.clone()).into_response(),
            native_ok("resp_incomplete_followup", "rest of answer"),
        ],
        vec![],
    )
    .await;
    let backup = MockServer::new(vec![native_ok("resp_incomplete_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let a = native.candidate("native-incomplete");
    let b = backup.candidate("incomplete-backup");
    mark_failure(&state, &a, FailureClass::Transient, None);
    state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .get_mut(&a.internal_id)
        .unwrap()
        .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    let response = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","max_output_tokens":1}),
        vec![a.clone(), b.clone()],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response_json(response).await, incomplete);
    assert!(!state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .contains_key(&a.internal_id));
    let continuation = route_responses_with_candidates(
        &state, &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_incomplete_binding","input":"continue"}),
        vec![b, a],
    ).await;
    assert_eq!(continuation.status(), StatusCode::OK);
    assert_eq!(
        response_json(continuation).await["id"],
        "resp_incomplete_followup"
    );
    assert_eq!(native.calls().len(), 2);
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn unsupported_endpoint_cache_avoids_repeat_probe_and_retries_native_after_expiry() {
    let native = MockServer::new(
        vec![
            endpoint_missing(StatusCode::NOT_FOUND),
            native_ok("resp_capability_recovered", "native recovered"),
        ],
        vec![chat_ok("first chat"), chat_ok("cached chat")],
    )
    .await;
    let backup = MockServer::new(vec![native_ok("resp_capability_backup", "backup")], vec![]).await;
    let state = AppState::for_tests().unwrap();
    let a = native.candidate("capability-cache");
    let b = backup.candidate("capability-backup");
    for expected_text in ["first chat", "cached chat"] {
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await["output"][0]["content"][0]["text"],
            expected_text
        );
    }
    assert_eq!(
        native
            .calls()
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/v1/responses",
            "/v1/chat/completions",
            "/v1/chat/completions"
        ]
    );
    *state
        .route_memory
        .lock()
        .unwrap()
        .unsupported_responses
        .get_mut(&a.fingerprint)
        .unwrap() = now_ms().saturating_sub(1);
    let recovered = route_responses_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"}),
        vec![a, b],
    )
    .await;
    assert_eq!(recovered.status(), StatusCode::OK);
    assert_eq!(
        response_json(recovered).await["id"],
        "resp_capability_recovered"
    );
    assert_eq!(native.calls().len(), 4);
    assert_eq!(native.calls()[3].0, "/v1/responses");
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn explicit_model_protocol_rejection_uses_chat_cache_without_model_cooldown() {
    let unsupported = (
        StatusCode::BAD_REQUEST,
        Json(json!({"error":{"code":"unsupported_protocol",
            "message":"The model does not support Responses"}})),
    )
        .into_response();
    let native = MockServer::new(
        vec![unsupported],
        vec![
            chat_ok("supported through chat"),
            chat_ok("cached chat support"),
        ],
    )
    .await;
    let backup = MockServer::new(
        vec![native_ok("resp_model_protocol_backup", "backup")],
        vec![],
    )
    .await;
    let state = AppState::for_tests().unwrap();
    let a = native.candidate("model-protocol-rejection");
    let b = backup.candidate("model-protocol-backup");
    for expected_text in ["supported through chat", "cached chat support"] {
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response_json(response).await["output"][0]["content"][0]["text"],
            expected_text
        );
        assert!(!state
            .route_memory
            .lock()
            .unwrap()
            .candidates
            .contains_key(&a.internal_id));
    }
    assert_eq!(
        native
            .calls()
            .iter()
            .map(|call| call.0.as_str())
            .collect::<Vec<_>>(),
        vec![
            "/v1/responses",
            "/v1/chat/completions",
            "/v1/chat/completions"
        ]
    );
    assert!(backup.calls().is_empty());
}

#[tokio::test]
async fn safety_policy_rejection_is_not_replayed_despite_protocol_rejection_text() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::FORBIDDEN] {
        let error = json!({"error":{"code":"content_policy_violation",
            "message":"The model does not support Responses: blocked by safety content policy"}});
        let native = MockServer::new(
            vec![(status, Json(error.clone())).into_response()],
            vec![chat_ok("must not bypass policy")],
        )
        .await;
        let backup = MockServer::new(
            vec![native_ok("resp_policy_backup", "must not replay")],
            vec![],
        )
        .await;
        let state = AppState::for_tests().unwrap();
        let a = native.candidate("mixed-policy-rejection");
        let response = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"question"}),
            vec![a.clone(), backup.candidate("mixed-policy-backup")],
        )
        .await;
        assert_eq!(response.status(), status);
        assert_eq!(response_json(response).await, error);
        let memory = state.route_memory.lock().unwrap();
        assert!(!memory.candidates.contains_key(&a.internal_id));
        assert!(!memory.unsupported_responses.contains_key(&a.fingerprint));
        assert_eq!(native.calls().len(), 1);
        assert_eq!(native.calls()[0].0, "/v1/responses");
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn nonstream_continuation_sse_failure_preserves_actual_status_and_scoped_health() {
    for (status, expected) in [
        (StatusCode::BAD_REQUEST, None),
        (StatusCode::NOT_FOUND, None),
        (StatusCode::UNAUTHORIZED, Some(FailureClass::Authentication)),
        (StatusCode::TOO_MANY_REQUESTS, Some(FailureClass::RateLimit)),
    ] {
        let wire = sse(
            "response.failed",
            &json!({"type":"response.failed","response":{
                "id":"resp_nonstream_continuation_failed","object":"response","status":"failed",
                "error":{"code":"response_not_found","message":"previous_response_id expired",
                    "status":status.as_u16()}}}),
        );
        let reply = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::RETRY_AFTER, "120")
            .body(Body::from(wire))
            .unwrap();
        let native = MockServer::new(
            vec![
                native_ok("resp_nonstream_continuation_initial", "first"),
                reply,
            ],
            vec![],
        )
        .await;
        let backup = MockServer::new(
            vec![native_ok("resp_nonstream_continuation_backup", "backup")],
            vec![],
        )
        .await;
        let state = AppState::for_tests().unwrap();
        let a = native.candidate("nonstream-continuation-error");
        let b = backup.candidate("nonstream-continuation-backup");
        let initial = route_responses_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"input":"first"}),
            vec![a.clone(), b.clone()],
        )
        .await;
        assert_eq!(
            response_json(initial).await["id"],
            "resp_nonstream_continuation_initial"
        );
        let started = now_ms();
        let response = route_responses_with_candidates(
            &state, &HeaderMap::new(),
            json!({"model":SMART_ROUTER_MODEL_ID,"previous_response_id":"resp_nonstream_continuation_initial","input":"follow-up","stream":false}),
            vec![b, a.clone()],
        ).await;
        assert_eq!(response.status(), status);
        let error = response_json(response).await;
        assert_eq!(error["error"]["code"], "response_not_found");
        assert_eq!(error["error"]["message"], "previous_response_id expired");
        let memory = state.route_memory.lock().unwrap();
        assert_eq!(
            memory
                .candidates
                .get(&a.internal_id)
                .and_then(|health| health.failure),
            expected
        );
        if expected == Some(FailureClass::RateLimit) {
            assert!(
                memory.candidates[&a.internal_id].cooldown_until_ms.unwrap() >= started + 120_000
            );
        }
        assert!(!state
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .contains_key("resp_nonstream_continuation_failed"));
        assert_eq!(native.calls().len(), 2);
        assert!(backup.calls().is_empty());
    }
}

#[tokio::test]
async fn responses_budget_covers_native_error_and_chat_body_collection_together() {
    let delay = Duration::from_millis(650);
    let delayed = |response: Response| {
        let (parts, body) = response.into_parts();
        let body = Body::from_stream(stream::once(async move {
            tokio::time::sleep(delay).await;
            body.collect()
                .await
                .map(|body| body.to_bytes())
                .map_err(std::io::Error::other)
        }));
        Response::from_parts(parts, body)
    };
    for (budget, should_succeed) in [
        (Duration::from_secs(1), false),
        (Duration::from_secs(4), true),
    ] {
        let first = MockServer::new(
            vec![delayed(endpoint_missing(StatusCode::NOT_FOUND))],
            vec![delayed(chat_ok("budget controlled response"))],
        )
        .await;
        let backup = MockServer::new(vec![native_ok("resp_budget_backup", "backup")], vec![]).await;
        let state = AppState::for_tests().unwrap();
        let a = first.candidate("cumulative-responses-budget");
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            route_with_policy(
                &state,
                &HeaderMap::new(),
                json!({"model":SMART_ROUTER_MODEL_ID,"input":"question","stream":false}),
                vec![a.clone(), backup.candidate("responses-budget-backup")],
                RoutePolicy {
                    max_attempts: 2,
                    time_budget: budget,
                    header_timeout: Duration::from_secs(4),
                    first_byte_timeout: Duration::from_secs(4),
                    ..RoutePolicy::default()
                },
                RouteProtocol::Responses,
            ),
        )
        .await
        .unwrap();
        let elapsed = started.elapsed();
        assert_eq!(
            first
                .calls()
                .iter()
                .map(|call| call.0.as_str())
                .collect::<Vec<_>>(),
            vec!["/v1/responses", "/v1/chat/completions"]
        );
        assert!(backup.calls().is_empty());
        if should_succeed {
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response_json(response).await["output"][0]["content"][0]["text"],
                "budget controlled response"
            );
            assert!(elapsed >= delay * 2);
            assert!(elapsed < budget);
            assert!(!state
                .route_memory
                .lock()
                .unwrap()
                .candidates
                .contains_key(&a.internal_id));
        } else {
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert!(response_json(response).await.get("error").is_some());
            assert!(elapsed < Duration::from_secs(2));
            assert_eq!(
                state.route_memory.lock().unwrap().candidates[&a.internal_id].failure,
                Some(FailureClass::Transient)
            );
        }
    }
}
