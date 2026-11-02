use super::*;

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) async fn check(
    state: &AppState,
    mut candidate: Candidate,
    protocol: RouteProtocol,
) -> bool {
    let mut state = state.clone();
    state.request_statistics = None;
    let state = &state;
    if !try_begin_attempt(state, &mut candidate) {
        return false;
    }
    match tokio::time::timeout(CHECK_TIMEOUT, run(state, &candidate, protocol)).await {
        Ok(ready) => ready,
        Err(_) => {
            mark_failure(state, &candidate, FailureClass::Transient, None);
            false
        }
    }
}

async fn run(state: &AppState, candidate: &Candidate, protocol: RouteProtocol) -> bool {
    let body = match protocol {
        RouteProtocol::Chat => json!({
            "model":candidate.model_id,
            "messages":[{"role":"user","content":"Reply only OK."}],
            "stream":true
        }),
        RouteProtocol::Responses => json!({
            "model":candidate.model_id,
            "input":"Reply only OK.",
            "stream":true
        }),
    };
    let started = Instant::now();
    let mut refreshed = None;
    let mut authentication_retried = false;
    loop {
        let response = match send_candidate(
            state,
            &HeaderMap::new(),
            candidate,
            body.clone(),
            protocol,
            refreshed.take(),
            false,
        )
        .await
        {
            Ok(response) => response,
            Err(_) => {
                mark_failure(state, candidate, FailureClass::Transient, None);
                return false;
            }
        };
        let rejected = response
            .extensions()
            .get::<RejectedAccountCredentials>()
            .cloned();
        let response = match read_response(response, protocol).await {
            Ok(()) => {
                mark_success(state, candidate);
                return true;
            }
            Err(response) => *response,
        };
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED && !authentication_retried {
            if let Some(rejected) = &rejected {
                let policy = RoutePolicy {
                    time_budget: CHECK_TIMEOUT,
                    ..RoutePolicy::default()
                };
                match refresh_rejected_account(state, candidate, rejected, started, policy).await {
                    Ok(credentials) => {
                        refreshed = Some(credentials);
                        authentication_retried = true;
                        continue;
                    }
                    Err(error) => {
                        mark_stream_failure(state, candidate, &error);
                        return false;
                    }
                }
            }
        }
        let retry = retry_after(response.headers());
        let model_scope = response
            .extensions()
            .get::<crate::services::account_proxy::ProxyFailureMetadata>()
            .is_some_and(|metadata| metadata.model_scope);
        match axum::body::to_bytes(response.into_body(), MAX_STREAM_PREFLIGHT_BYTES).await {
            Ok(bytes) => {
                mark_upstream_failure(state, candidate, status, &bytes, retry, model_scope);
            }
            Err(_) => mark_failure(state, candidate, FailureClass::Transient, None),
        }
        return false;
    }
}

async fn read_response(response: Response, protocol: RouteProtocol) -> Result<(), Box<Response>> {
    if !response.status().is_success() {
        return Err(Box::new(response));
    }
    let headers = response.headers().clone();
    let mut received = 0usize;
    let mut upstream = response.into_body().into_data_stream().map(|chunk| {
        let chunk = chunk?;
        received = received.saturating_add(chunk.len());
        if received > MAX_STREAM_PREFLIGHT_BYTES {
            return Err(axum::Error::new(std::io::Error::other(
                "Standby probe exceeded its response limit",
            )));
        }
        Ok(chunk)
    });
    let mut bytes = Vec::new();
    let first = loop {
        let Some(chunk) = upstream.next().await else {
            return Err(invalid_response());
        };
        bytes.extend_from_slice(&chunk.map_err(stream_failure)?);
        if let Some(first) = bytes.iter().find(|byte| !byte.is_ascii_whitespace()) {
            break *first;
        }
    };
    // API models can omit or generalize Content-Type on an otherwise valid SSE
    // response. Inspect the bounded body prefix without waiting for stream EOF.
    if !matches!(first, b'{' | b'[') {
        let mut completion = StreamCompletion {
            headers,
            protocol,
            ..StreamCompletion::default()
        };
        let mut frames = completion.parser.push(&bytes);
        let mut eof = false;
        loop {
            let mut actual_output = false;
            for (event, data) in frames {
                if !completion.done {
                    actual_output |= event_has_output(&event, &data, protocol);
                }
                completion
                    .observe(vec![(event, data)])
                    .map_err(|error| stream_failure(axum::Error::new(error)))?;
                if let Some(error) = &completion.failure {
                    return Err(Box::new(error.clone().into_response()));
                }
            }
            if actual_output {
                return Ok(());
            }
            if completion.done || eof {
                return Err(invalid_response());
            }
            frames = match upstream.next().await {
                Some(chunk) => completion.parser.push(&chunk.map_err(stream_failure)?),
                None => {
                    eof = true;
                    completion.parser.finish()
                }
            };
        }
    }
    while let Some(chunk) = upstream.next().await {
        bytes.extend_from_slice(&chunk.map_err(stream_failure)?);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_response())?;
    if let Some(error) = crate::services::account_proxy::stream_error(
        if value["status"] == "failed" {
            "response.failed"
        } else {
            ""
        },
        &value.to_string(),
        &headers,
    ) {
        return Err(Box::new(error.into_response()));
    }
    let valid = match protocol {
        RouteProtocol::Chat => value["choices"].as_array().is_some_and(|choices| {
            !choices.is_empty()
                && choices
                    .iter()
                    .any(|choice| chat_message_has_content(&choice["message"]))
        }),
        RouteProtocol::Responses => {
            value["object"] == "response"
                && matches!(value["status"].as_str(), Some("completed" | "incomplete"))
                && value["id"].as_str().is_some_and(|id| !id.is_empty())
                && response_has_output(&value)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(invalid_response())
    }
}

fn stream_failure(error: axum::Error) -> Box<Response> {
    proxy_error_from_chain(&error)
        .map(|error| Box::new(error.clone().into_response()))
        .unwrap_or_else(invalid_response)
}

fn event_has_output(event: &str, data: &str, protocol: RouteProtocol) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return false;
    };
    if protocol == RouteProtocol::Chat {
        return value["choices"].as_array().is_some_and(|choices| {
            choices
                .iter()
                .any(|choice| chat_message_has_content(&choice["delta"]))
        });
    }
    let kind = value["type"].as_str().unwrap_or(event);
    (matches!(
        kind,
        "response.output_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.refusal.delta"
            | "response.audio.delta"
            | "response.output_audio.delta"
            | "response.audio_transcript.delta"
            | "response.output_audio_transcript.delta"
            | "response.function_call_arguments.delta"
            | "response.custom_tool_call_input.delta"
    ) && nonempty_text(&value["delta"]))
        || item_has_output(&value["item"])
        || part_has_output(&value["part"])
        || response_has_output(&value["response"])
}

fn part_has_output(part: &Value) -> bool {
    ["text", "refusal", "thinking", "data", "transcript"]
        .iter()
        .any(|field| nonempty_text(&part[*field]))
}

fn item_has_output(item: &Value) -> bool {
    nonempty_text(&item["content"])
        || nonempty_text(&item["encrypted_content"])
        || nonempty_text(&item["arguments"])
        || nonempty_text(&item["input"])
        || ["content", "summary"].iter().any(|field| {
            item[*field]
                .as_array()
                .is_some_and(|parts| parts.iter().any(part_has_output))
        })
}

pub(super) fn response_has_output(response: &Value) -> bool {
    response["output"]
        .as_array()
        .is_some_and(|items| items.iter().any(item_has_output))
}

fn invalid_response() -> Box<Response> {
    Box::new(empty_response_error().into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    fn candidate(id: &str) -> Candidate {
        Candidate {
            internal_id: id.into(),
            model_id: "standby-model".into(),
            base_url: "http://127.0.0.1:1/v1".into(),
            api_key: "fixture-key".into(),
            account_source: None,
            fingerprint: format!("standby-{id}"),
            attempt: None,
        }
    }

    fn state(response: Response) -> AppState {
        let mut state = AppState::for_tests().unwrap();
        state.activity = Arc::new(Mutex::new(RouteActivity::default()));
        state.fixture_responses = Some(Arc::new(Mutex::new(HashMap::from([(
            "standby".into(),
            response,
        )]))));
        state
    }

    fn sse(body: Body) -> Response {
        Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(body)
            .unwrap()
    }

    #[tokio::test]
    async fn standby_stops_after_content_without_changing_activity_or_bindings() {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(2);
        tx.send(Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
        )))
        .await
        .unwrap();
        tx.send(Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n",
        )))
        .await
        .unwrap();
        let state = state(sse(Body::from_stream(ReceiverStream::new(rx))));
        assert!(check(&state, candidate("standby"), RouteProtocol::Chat).await);
        tokio::time::timeout(Duration::from_secs(1), tx.closed())
            .await
            .unwrap();
        assert_eq!(state.activity.lock().unwrap().sequence, 0);
        assert!(state
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .is_empty());
    }

    #[tokio::test]
    async fn standby_rejects_http_and_stream_quota_errors_without_locking_siblings() {
        for response in [
            Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .body(Body::from("{\"error\":{\"code\":\"rate_limit_exceeded\"}}"))
                .unwrap(),
            sse(Body::from(
                "data: {\"error\":{\"code\":\"insufficient_quota\"}}\n\n",
            )),
        ] {
            let state = state(response);
            let mut model = candidate("standby");
            model.account_source = Some(crate::models::model::AccountSource {
                provider: AccountProvider::WorkBuddyAi,
                account_id: "fixture-account".into(),
            });
            assert!(!check(&state, model.clone(), RouteProtocol::Chat).await);
            assert!(candidate_is_cooling_down(&state, &model));
            model.internal_id = "sibling".into();
            model.model_id = "another-model".into();
            model.fingerprint = "another-fingerprint".into();
            assert!(!candidate_is_cooling_down(&state, &model));
            assert!(state.route_memory.lock().unwrap().accounts.is_empty());
        }
    }

    #[tokio::test]
    async fn standby_rejects_empty_incomplete_and_invalid_successes() {
        for response in [
            sse(Body::from("data: [DONE]\n\n")),
            sse(Body::from(": heartbeat\n\n")),
            sse(Body::from("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n")),
            sse(Body::from("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n")),
            sse(Body::from("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10}}\n\ndata: [DONE]\n\n")),
            sse(Body::from(
                "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n",
            )),
            Response::new(Body::from("{}")),
            Response::new(Body::from("{\"choices\":[]}")),
            Response::new(Body::from(
                "{\"choices\":[{\"message\":{\"content\":\"\"}}]}",
            )),
            Response::new(Body::from("<html>login</html>")),
        ] {
            let state = state(response);
            assert!(!check(&state, candidate("standby"), RouteProtocol::Chat).await);
            assert!(candidate_is_cooling_down(&state, &candidate("standby")));
        }
    }

    #[tokio::test]
    async fn standby_does_not_cool_down_parameter_incompatibility() {
        let state = state(sse(Body::from(
            "data: {\"error\":{\"code\":\"unsupported_parameter\",\"message\":\"not supported\"}}\n\n",
        )));
        assert!(!check(&state, candidate("standby"), RouteProtocol::Chat).await);
        assert!(state.route_memory.lock().unwrap().candidates.is_empty());
    }

    #[tokio::test]
    async fn standby_rejects_json_errors_and_validates_responses_structure() {
        for (value, expected) in [
            (json!({"error":{"code":"insufficient_quota"}}), false),
            (json!({"status":"completed","output":[]}), false),
            (
                json!({"object":"response","id":"probe","status":"completed","output":[]}),
                false,
            ),
            (
                json!({"object":"response","id":"probe","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}]}),
                true,
            ),
        ] {
            let state = state(Response::new(Body::from(value.to_string())));
            assert_eq!(
                check(&state, candidate("standby"), RouteProtocol::Responses).await,
                expected
            );
            assert!(state
                .response_memory
                .lock()
                .unwrap()
                .response_bindings
                .is_empty());
        }
    }

    #[tokio::test]
    async fn standby_bounds_the_first_chunk_and_does_not_accept_its_content() {
        let body = format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
            "x".repeat(MAX_STREAM_PREFLIGHT_BYTES)
        );
        let state = state(sse(Body::from(body)));
        assert!(!check(&state, candidate("standby"), RouteProtocol::Chat).await);
    }

    #[tokio::test]
    async fn standby_preserves_json_error_scope_and_retry_after() {
        let response = Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::RETRY_AFTER, "150")
            .body(Body::from(
                json!({"error":{"code":"rate_limit_exceeded","message":"quota limit"}}).to_string(),
            ))
            .unwrap();
        let state = state(response);
        assert!(!check(&state, candidate("standby"), RouteProtocol::Chat).await);
        let memory = state.route_memory.lock().unwrap();
        let health = &memory.candidates["standby"];
        assert_eq!(health.failure, Some(FailureClass::RateLimit));
        let remaining = health.cooldown_until_ms.unwrap().saturating_sub(now_ms());
        assert!((149_000..=150_000).contains(&remaining));
    }

    #[tokio::test]
    async fn standby_refreshes_http_or_stream_authentication_only_once() {
        for mut response in [
            Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from("{\"error\":{\"code\":\"invalid_api_key\"}}"))
                .unwrap(),
            sse(Body::from(
                "data: {\"error\":{\"code\":\"authentication_error\"}}\n\n",
            )),
        ] {
            response
                .extensions_mut()
                .insert(RejectedAccountCredentials(json!({"accessToken":"old"})));
            let mut state = state(response);
            state.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
                result: Ok(json!({"accessToken":"fresh"})),
                delay: Duration::ZERO,
                rejections: Vec::new(),
                retry_responses: VecDeque::from([sse(Body::from(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n",
                ))]),
                sent_credentials: Vec::new(),
            })));
            let mut model = candidate("standby");
            model.account_source = Some(crate::models::model::AccountSource {
                provider: AccountProvider::WorkBuddyAi,
                account_id: "fixture-refresh-account".into(),
            });
            assert!(check(&state, model, RouteProtocol::Chat).await);
            let fixture = state
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap();
            assert_eq!(fixture.rejections.len(), 1);
            assert_eq!(fixture.sent_credentials, [json!({"accessToken":"fresh"})]);
            assert!(state.route_memory.lock().unwrap().accounts.is_empty());
        }
    }

    #[tokio::test]
    async fn standby_uses_an_isolated_minimal_request_for_each_protocol() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().fallback(post({
            let captured = captured.clone();
            move |headers: HeaderMap, Json(body): Json<Value>| {
                captured.lock().unwrap().push((headers, body.clone()));
                async move {
                    if body.get("input").is_some() {
                        sse(Body::from("event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\n"))
                    } else {
                        sse(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n"))
                    }
                }
            }
        }));
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        for protocol in [RouteProtocol::Chat, RouteProtocol::Responses] {
            let mut state = AppState::for_tests().unwrap();
            state.activity = Arc::new(Mutex::new(RouteActivity::default()));
            let mut model = candidate("standby");
            model.base_url = format!("http://{address}/v1");
            assert!(check(&state, model, protocol).await);
        }
        task.abort();
        let captured = captured.lock().unwrap();
        assert_eq!(captured.len(), 2);
        for (headers, body) in captured.iter() {
            assert!(headers.get("x-session-id").is_none());
            assert_eq!(body["model"], "standby-model");
            assert_eq!(body["stream"], true);
            assert_eq!(body.as_object().unwrap().len(), 3);
            if let Some(messages) = body.get("messages") {
                assert_eq!(
                    messages,
                    &json!([{"role":"user","content":"Reply only OK."}])
                );
            } else {
                assert_eq!(body["input"], "Reply only OK.");
            }
        }
    }

    #[tokio::test]
    async fn standby_waits_for_actual_reasoning_content_and_preserves_a_later_quota_error() {
        for progress in [
            json!({"type":"response.reasoning_text.delta","delta":"thinking"}),
            json!({"type":"response.output_item.done","item":{"type":"reasoning","id":"rs_probe","summary":[{"type":"summary_text","text":"thinking"}]}}),
            json!({"type":"response.output_item.done","item":{"type":"reasoning","id":"rs_probe","encrypted_content":"opaque"}}),
            json!({"type":"response.failed","response":{"error":{"code":"rate_limit_exceeded","message":"quota limit"}}}),
        ] {
            let expected = progress["type"] != "response.failed";
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(2);
            tx.send(Ok(Bytes::from_static(b"event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"reasoning\",\"id\":\"rs_probe\",\"status\":\"in_progress\",\"summary\":[]}}\n\n"))).await.unwrap();
            let state = state(sse(Body::from_stream(ReceiverStream::new(rx))));
            let model = candidate("standby");
            let mut pending = Box::pin(check(&state, model.clone(), RouteProtocol::Responses));
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut pending)
                    .await
                    .is_err(),
                "creating a reasoning item is not evidence of usable model output"
            );
            tx.send(Ok(Bytes::from(format!("data: {progress}\n\n"))))
                .await
                .unwrap();
            assert_eq!(pending.await, expected);
            tokio::time::timeout(Duration::from_secs(1), tx.closed())
                .await
                .unwrap();
            if !expected {
                assert_eq!(
                    state.route_memory.lock().unwrap().candidates["standby"].failure,
                    Some(FailureClass::RateLimit)
                );
            }
        }
    }

    #[tokio::test]
    async fn standby_rejects_json_reasoning_identifiers_without_output() {
        for (item, expected) in [
            (
                json!({"type":"reasoning","id":"rs_probe","summary":[]}),
                false,
            ),
            (
                json!({"type":"reasoning","id":"rs_probe","summary":[{"type":"summary_text","text":"thinking"}]}),
                true,
            ),
            (
                json!({"type":"reasoning","id":"rs_probe","encrypted_content":"opaque"}),
                true,
            ),
        ] {
            for status in ["completed", "incomplete"] {
                let response = json!({"object":"response","id":"resp_probe","status":status,
                    "output":[item]});
                let state = state(Response::new(Body::from(response.to_string())));
                assert_eq!(
                    check(&state, candidate("standby"), RouteProtocol::Responses).await,
                    expected
                );
            }
        }
    }

    #[tokio::test]
    async fn standby_checks_all_received_errors_and_ignores_content_after_completion() {
        for (protocol, payload) in [
            (RouteProtocol::Chat, "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\ndata: {\"error\":{\"code\":\"rate_limit_exceeded\"}}\n\n"),
            (RouteProtocol::Responses, "data: {\"type\":\"response.output_text.delta\",\"delta\":\"OK\"}\n\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"rate_limit_exceeded\"}}}\n\n"),
            (RouteProtocol::Chat, "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"late\"}}]}\n\n"),
            (RouteProtocol::Responses, "data: {\"type\":\"response.incomplete\",\"response\":{\"object\":\"response\",\"id\":\"resp_probe\",\"status\":\"incomplete\",\"output\":[]}}\n\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"late\"}\n\n"),
        ] {
            let state = state(sse(Body::from(payload)));
            assert!(!check(&state, candidate("standby"), protocol).await);
        }
    }

    #[tokio::test]
    async fn standby_accepts_valid_sse_without_a_specific_content_type() {
        for content_type in [None, Some("application/octet-stream")] {
            for protocol in [RouteProtocol::Chat, RouteProtocol::Responses] {
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, Infallible>>(2);
                tx.send(Ok(Bytes::from_static(b" \n"))).await.unwrap();
                let payload = if protocol == RouteProtocol::Chat {
                    "data: {\"choices\":[{\"delta\":{\"content\":\"OK\"}}]}\n\n"
                } else {
                    "data: {\"type\":\"response.reasoning_text.delta\",\"delta\":\"thinking\"}\n\n"
                };
                tx.send(Ok(Bytes::from_static(payload.as_bytes())))
                    .await
                    .unwrap();
                let mut builder = Response::builder();
                if let Some(content_type) = content_type {
                    builder = builder.header(header::CONTENT_TYPE, content_type);
                }
                let state = state(
                    builder
                        .body(Body::from_stream(ReceiverStream::new(rx)))
                        .unwrap(),
                );
                assert!(tokio::time::timeout(
                    Duration::from_millis(100),
                    check(&state, candidate("standby"), protocol)
                )
                .await
                .unwrap());
                assert!(state.route_memory.lock().unwrap().candidates.is_empty());
                tokio::time::timeout(Duration::from_secs(1), tx.closed())
                    .await
                    .unwrap();
            }
        }
    }
}
