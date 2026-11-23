use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn candidate(id: &str, base_url: &str) -> Candidate {
    Candidate {
        internal_id: id.into(),
        model_id: "same-upstream-id".into(),
        base_url: base_url.into(),
        anthropic_url: None,
        api_key: "fixture".into(),
        fingerprint: id.into(),
        account_source: None,
        attempt: None,
    }
}

#[test]
fn picker_selection_is_scoped_to_active_candidates_and_preserves_auto_aliases() {
    let mut state = AppState::for_tests().unwrap();
    let models = || {
        vec![
            candidate("first", "http://first.invalid/v1"),
            candidate("second", "http://second.invalid/v1"),
        ]
    };
    let id = super::super::catalog::selector("second");
    let selected = select_candidates(&mut state, &format!("{id}[1m]"), models(), true).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].internal_id, "second");
    assert_eq!(state.model_id, id);
    // A paused/deleted model or a model from another group cannot fall back to auto.
    assert!(select_candidates(
        &mut state,
        &id,
        vec![candidate("first", "http://first.invalid/v1")],
        true
    )
    .is_err());
    state.model_id = "auto".into();
    assert_eq!(
        select_candidates(&mut state, "echobird/auto", models(), false)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        select_candidates(&mut state, "claude-opus-4-6", models(), true)
            .unwrap()
            .len(),
        2
    );
    assert!(select_candidates(&mut state, "foreign", models(), false).is_err());
}

#[tokio::test]
async fn picker_routes_chat_responses_and_messages_to_only_the_selected_upstream() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |Json(body):Json<Value>| {
        let calls = recorded.clone(); async move {
            calls.lock().unwrap().push(body.clone());
            Json(json!({"id":"fixture","object":"chat.completion","model":body["model"],
                "choices":[{"index":0,"message":{"role":"assistant","content":"selected upstream"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":2,"completion_tokens":2,"total_tokens":4}}))
        }
    }));
    let recorded = calls.clone();
    let app = app.route("/v1/responses", post(move |Json(body):Json<Value>| {
        let calls = recorded.clone(); async move {
            calls.lock().unwrap().push(body.clone());
            Json(json!({"id":"resp_picker","object":"response","status":"completed","model":body["model"],
                "output":[{"id":"msg_picker","type":"message","role":"assistant","status":"completed",
                "content":[{"type":"output_text","text":"selected upstream","annotations":[]}]}]}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    for protocol in ["chat", "responses", "messages"] {
        let mut state = AppState::for_tests().unwrap();
        let selector = super::super::catalog::selector("second");
        let candidates = select_candidates(
            &mut state,
            &selector,
            vec![
                candidate("first", "http://127.0.0.1:1/v1"),
                candidate("second", &base_url),
            ],
            protocol == "messages",
        )
        .unwrap();
        let body = if protocol == "responses" {
            json!({"model":selector,"input":"hello","stream":false})
        } else {
            json!({"model":selector,"messages":[{"role":"user","content":"hello"}],"stream":false})
        };
        let response = match protocol {
            "responses" => {
                route_responses_with_candidates(&state, &HeaderMap::new(), body, candidates).await
            }
            "messages" => {
                route_messages_with_candidates(&state, &HeaderMap::new(), body, candidates).await
            }
            _ => route_chat_with_candidates(&state, &HeaderMap::new(), body, candidates).await,
        };
        assert!(
            response.status().is_success(),
            "{protocol}: {}",
            response.status()
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert!(
            String::from_utf8_lossy(&bytes).contains("selected upstream"),
            "{protocol}"
        );
    }
    assert_eq!(calls.lock().unwrap().len(), 3);
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .all(|body| body["model"] == "same-upstream-id"));
    task.abort();
}

#[tokio::test]
async fn gemini_picker_keeps_each_key_and_preserves_streamed_tool_calls() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let tool_call = json!({
        "index": 0, "id": "call_fixture", "type": "function",
        "function": {"name": "lookup", "arguments": "{}"},
        "extra_content": {"google": {"thought_signature": "fixture-signature"}}
    });
    let streamed_tool_call = tool_call.clone();
    let app = Router::new().route(
        "/v1beta/openai/chat/completions",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let authorization = headers[header::AUTHORIZATION].to_str().unwrap().to_owned();
            recorded.lock().unwrap().push((authorization, body));
            let tool_call = streamed_tool_call.clone();
            async move {
                let delta = json!({
                    "id": "chatcmpl-gemini-fixture", "object": "chat.completion.chunk",
                    "model": "gemini-fixture",
                    "choices": [{"index": 0, "delta": {"role": "assistant", "tool_calls": [tool_call]},
                        "finish_reason": "tool_calls"}]
                });
                ([(header::CONTENT_TYPE, "text/event-stream")], format!("data: {delta}\n\ndata: [DONE]\n\n"))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1beta/openai", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let ids = ["first-key", "second-key"];
    let models = ids.map(|key| {
        serde_json::from_value(json!({
            "internalId": key, "name": key, "baseUrl": base_url,
            "modelId": "gemini-fixture", "apiKey": key
        }))
        .unwrap()
    });
    let candidates = || {
        resolve_candidates_from(
            ids.map(str::to_owned).to_vec(),
            &models,
            &crate::services::local_llm::LocalServerInfo::default(),
        )
    };
    let mut state = AppState::for_tests().unwrap();
    for key in ids {
        let selector = super::super::catalog::selector(key);
        let selected = select_candidates(&mut state, &selector, candidates(), false).unwrap();
        let response = route_chat_with_candidates(
            &state,
            &HeaderMap::new(),
            json!({
                "model": selector, "stream": true,
                "messages": [
                    {"role": "user", "content": "look up the value"},
                    {"role": "assistant", "tool_calls": [tool_call.clone()]},
                    {"role": "tool", "tool_call_id": "call_fixture", "content": "value"}
                ],
                "tools": [{"type": "function", "function": {"name": "lookup", "parameters": {"type":"object"}}}]
            }),
            selected,
        ).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let stream = String::from_utf8_lossy(&bytes);
        assert!(stream.contains("fixture-signature"));
        assert!(stream.contains("data: [DONE]"));
    }
    let recorded = calls.lock().unwrap();
    assert_eq!(recorded.len(), 2);
    for ((authorization, body), key) in recorded.iter().zip(ids) {
        assert_eq!(authorization, &format!("Bearer {key}"));
        assert_eq!(body["model"], "gemini-fixture");
        assert_eq!(body["messages"][1]["tool_calls"][0], tool_call);
        assert_eq!(body["messages"][2]["tool_call_id"], "call_fixture");
    }
    task.abort();
}

#[tokio::test]
async fn selected_model_failure_never_uses_another_candidate() {
    let calls = Arc::new(AtomicUsize::new(0));
    let recorded = calls.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move || {
            recorded.fetch_add(1, Ordering::SeqCst);
            async {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":{"message":"unavailable"}})),
                )
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut state = AppState::for_tests().unwrap();
    let id = super::super::catalog::selector("first");
    let selected = select_candidates(
        &mut state,
        &id,
        vec![
            candidate("first", &base_url),
            candidate("second", &base_url),
        ],
        false,
    )
    .unwrap();
    let response = route_chat_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"model":id,"messages":[]}),
        selected,
    )
    .await;
    assert!(!response.status().is_success());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
#[ignore = "Requires an installed Codex CLI and an explicit fixture output directory; uses only local fake upstreams"]
async fn installed_codex_uses_the_picker_catalog_and_responses_chat_fallback() {
    let executable =
        std::env::var("ECHOBIRD_SWITCH_CODEX_EXE").expect("set fixture Codex executable");
    let directory = PathBuf::from(
        std::env::var("ECHOBIRD_SWITCH_PROBE_DIR").expect("set fixture output directory"),
    )
    .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&directory).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    let upstream = Router::new()
        .route("/v1/responses", post(|| async { StatusCode::NOT_FOUND }))
        .route("/v1/chat/completions", post(move |Json(body):Json<Value>| {
            recorded.lock().unwrap().push(body["model"].clone());
            async move {
                let delta = json!({"id":"chatcmpl-picker","object":"chat.completion.chunk","model":"fixture-model",
                    "choices":[{"index":0,"delta":{"role":"assistant","content":"PICKER_OK"},"finish_reason":null}]});
                let done = json!({"id":"chatcmpl-picker","object":"chat.completion.chunk","model":"fixture-model",
                    "choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
                    "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}});
                ([(header::CONTENT_TYPE, "text/event-stream")], format!("data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let state = AppState::for_tests().unwrap();
    let router = Router::new().route(
        "/v1/responses",
        post(move |headers: HeaderMap, Json(body): Json<Value>| {
            let mut state = state.clone();
            let upstream_url = upstream_url.clone();
            async move {
                assert!(is_authorized(&headers, "fixture-key"));
                let selected = match select_candidates(
                    &mut state,
                    body["model"].as_str().unwrap(),
                    vec![
                        candidate("first", "http://127.0.0.1:1/v1"),
                        candidate("second", &upstream_url),
                    ],
                    false,
                ) {
                    Ok(selected) => selected,
                    Err(error) => return openai_error(StatusCode::NOT_FOUND, &error),
                };
                route_responses_with_candidates(&state, &headers, body, selected).await
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let router_url = format!("http://{}/v1", listener.local_addr().unwrap());
    let router_task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let codex_home = directory.join("codex");
    super::super::catalog::with_test_models_at(&router_url, || {
        let info = serde_json::from_value(
            json!({"model":"auto","name":"Auto Router","baseUrl":router_url,
            "apiKey":"fixture-key","protocol":"openai","webSearch":false}),
        )
        .unwrap();
        let result = crate::services::tool_config_manager::apply_codex_at(
            "codex",
            &info,
            &codex_home,
            &directory.join("state"),
        );
        assert!(result.success, "{}", result.message);
    });
    // Explicitly use the HTTP transport in this isolated provider fixture.
    let path = codex_home.join("config.toml");
    let content = fs::read_to_string(&path).unwrap();
    let mut content = content.parse::<toml_edit::DocumentMut>().unwrap();
    content["model_providers"]["OpenAI"]["supports_websockets"] = toml_edit::value(false);
    fs::write(path, content.to_string()).unwrap();
    let mut command = crate::utils::process::command(executable);
    command
        .args([
            "exec",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--json",
            "-m",
            &super::super::catalog::selector("second"),
            "Reply with PICKER_OK.",
        ])
        .current_dir(&directory)
        .env("CODEX_HOME", &codex_home)
        .env("OPENAI_API_KEY", "fixture-key")
        .env_remove("OPENAI_BASE_URL")
        .env_remove("CODEX_API_KEY");
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(45), command.output()).await;
    router_task.abort();
    upstream_task.abort();
    let output = output.expect("Codex fixture timed out").unwrap();
    fs::write(directory.join("stdout.jsonl"), &output.stdout).unwrap();
    fs::write(directory.join("stderr.log"), &output.stderr).unwrap();
    assert!(
        output.status.success(),
        "Codex fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("PICKER_OK"));
    assert!(!calls.lock().unwrap().is_empty());
    assert!(calls
        .lock()
        .unwrap()
        .iter()
        .all(|model| model == "same-upstream-id"));
    eprintln!("Isolated Codex fixture passed: {}", directory.display());
}
