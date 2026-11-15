use super::*;

#[test]
fn trae_compatibility_reasoning_aliases_preserve_the_requested_native_level() {
    let model = Model {
        function: "chat_v3".into(),
        model_name: None,
        max_tokens: Some(32000),
        multimodal: false,
        reasoning: vec!["light".into(), "high".into(), "extra_high".into()],
    };
    for (input, expected) in [("low", "light"), ("xhigh", "extra_high"), ("high", "high")] {
        let output = prepare(
            &json!({"messages":[], "reasoning_effort":input}),
            "fixture",
            &model,
        )
        .unwrap();
        assert_eq!(output["reasoning_effort"], expected);
    }
    let output = prepare(
        &json!({"messages":[], "reasoning_effort":"max"}),
        "fixture",
        &model,
    )
    .unwrap();
    assert!(output.get("reasoning_effort").is_none());
}

#[test]
fn current_directory_fields_keep_native_reasoning_and_region_visibility() {
    let rows = json!([
        {"config_name":"current","usage":"chat_completion","reasoning_effort_config":{"options":["light","high","extra_high"]},"model_detail_list":[{"model_name":"current__max","max_tokens":64000},{"model_name":"current__dev","max_tokens":32000}]},
        {"config_name":"string-display","display_config":"{\"reasoning_effort_config\":{\"options\":[\"high\"]}}"},
        {"config_name":"off","config_switch":false},
        {"config_name":"helper","usage":"title_generation"},
        {"config_name":"custom","is_custom_model":true},
        {"config_name":"custom-display","display_config":"{\"is_custom_model\":true}"},
        {"config_name":"native-only","is_invisible_to_user":true},
        {"config_name":"no-thinking","support_thinking":false,"reasoning_effort_config":{"options":["high"]},"reasoning_effort_options":["low"]}
    ]);
    for provider in [AccountProvider::Trae, AccountProvider::Traecn] {
        let mut models = Catalog::new();
        add_models(&mut models, provider, "chat_v3", &rows);
        assert_eq!(
            models.contains_key("native-only"),
            provider == AccountProvider::Traecn
        );
        for hidden in ["off", "helper", "custom", "custom-display"] {
            assert!(!models.contains_key(hidden));
        }
        let model = &models["current"][0];
        assert_eq!(model.model_name.as_deref(), Some("current__dev"));
        assert_eq!(model.max_tokens, Some(32000));
        assert_eq!(model.reasoning, ["light", "high", "extra_high"]);
        assert_eq!(models["string-display"][0].reasoning, ["high"]);
        assert!(models["no-thinking"][0].reasoning.is_empty());
        let prepared = prepare(
            &json!({"messages":[],"reasoning":{"effort":"xhigh"}}),
            "current",
            model,
        )
        .unwrap();
        assert_eq!(prepared["model"], "current");
        assert_eq!(prepared["model_name"], "current__dev");
        assert_eq!(prepared["config_name"], "current");
        assert_eq!(prepared["reasoning_effort"], "extra_high");
    }
    assert!(serde_json::from_value::<Catalog>(
        json!({"old":{"function":"chat_v3","max_tokens":32000,"multimodal":false,"reasoning":[]}})
    )
    .is_err());
}

#[derive(Clone)]
struct Fixture {
    calls: std::sync::Arc<tokio::sync::Mutex<Vec<(String, Value)>>>,
    mode: &'static str,
}

async fn fixture(mode: &'static str) -> (String, Fixture, tokio::task::JoinHandle<()>) {
    use axum::{
        body::{to_bytes, Body},
        extract::{Request, State},
        routing::post,
        Router,
    };
    async fn handle(State(state): State<Fixture>, request: Request) -> Response {
        let path = request.uri().path().to_string();
        let body: Value =
            serde_json::from_slice(&to_bytes(request.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        state.calls.lock().await.push((path.clone(), body.clone()));
        if path == BATCH_MODEL_PATH {
            if state.mode == "batch401" || state.mode == "batch429" {
                return Response::builder()
                    .status(if state.mode == "batch401" { 401 } else { 429 })
                    .header("retry-after", "7")
                    .body(Body::empty())
                    .unwrap();
            }
            let groups: Vec<_> = CHAT_FUNCTIONS.iter().map(|function| json!({"function":function,"config_info_list":if matches!(*function,"chat_v3"|"solo_agent") {
                json!([{"config_name":"fixture","reasoning_effort_config":{"options":["light","high","extra_high"]},"model_detail_list":[{"model_name":"fixture__dev","max_tokens":32000}]}])
            } else {json!([])}})).collect();
            return super::super::json_response(&json!({"data":{"function_configs":groups}}));
        }
        if path == MODEL_PATH {
            panic!("Complete batch must not fan out to per-function requests");
        }
        if body["function"] == "chat_v3" {
            match state.mode {
                "http" => return Response::builder().status(400).header("content-type","application/json").body(Body::from("{\"code\":4001}")).unwrap(),
                "json" => return super::super::json_response(&json!({"error":{"code":"4023"}})),
                "sse" => return super::super::stream_response(Body::from("event: metadata\ndata: {}\n\nevent: error\ndata: {\"code\":1005}\n\n")),
                "auth" | "rate" => return Response::builder().status(if state.mode=="auth" {401} else {429}).header("content-type","application/json").header("retry-after","7").body(Body::from("{\"code\":4001}")).unwrap(),
                "partial" => return super::super::stream_response(Body::from("event: output\ndata: {\"response\":\"partial\"}\n\nevent: error\ndata: {\"code\":4001}\n\n")),
                _ => {},
            }
        }
        super::super::stream_response(Body::from("event: output\ndata: {\"response\":\"OK\"}\n\nevent: done\ndata: {\"finish_reason\":\"stop\"}\n\n"))
    }
    let state = Fixture {
        calls: Default::default(),
        mode,
    };
    let app = Router::new()
        .route(BATCH_MODEL_PATH, post(handle))
        .route(MODEL_PATH, post(handle))
        .route(CHAT_PATH, post(handle))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, state, task)
}

fn credentials(base: &str) -> Value {
    json!({"accessToken":"fixture","userId":uuid::Uuid::new_v4().to_string(),"machineId":"fixture","deviceId":"fixture","appVersion":"3.3.83","region":"sg","baseUrl":base})
}

#[tokio::test]
async fn retries_only_native_function_rejections_before_output_and_remembers_success() {
    for mode in ["http", "json", "sse"] {
        let (base, state, task) = fixture(mode).await;
        let credentials = credentials(&base);
        let client = reqwest::Client::new();
        assert_eq!(
            list_models(AccountProvider::Trae, credentials.clone(), &client)
                .await
                .unwrap(),
            ["fixture"]
        );
        for _ in 0..2 {
            let response = route(
                AccountProvider::Trae,
                credentials.clone(),
                "fixture",
                json!({"messages":[{"role":"user","content":"test"}],"reasoning_effort":"xhigh"}),
                &client,
            )
            .await
            .unwrap();
            let value: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(value["choices"][0]["message"]["content"], "OK");
        }
        let calls = state.calls.lock().await;
        assert_eq!(
            calls
                .iter()
                .filter(|(path, _)| path == BATCH_MODEL_PATH)
                .count(),
            1
        );
        let chats: Vec<_> = calls
            .iter()
            .filter(|(path, _)| path == CHAT_PATH)
            .map(|(_, body)| body)
            .collect();
        assert_eq!(
            chats
                .iter()
                .map(|body| body["function"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["chat_v3", "solo_agent", "solo_agent"]
        );
        assert_eq!(chats[0]["session_id"], chats[1]["session_id"]);
        assert_ne!(chats[0]["request_id"], chats[1]["request_id"]);
        assert!(chats.iter().all(|body| body["model"] == "fixture"
            && body["model_name"] == "fixture__dev"
            && body["config_name"] == "fixture"
            && body["reasoning_effort"] == "extra_high"));
        task.abort();
    }
}

#[tokio::test]
async fn auth_rate_and_partial_output_never_repeat_a_request() {
    for mode in ["auth", "rate", "partial"] {
        let (base, state, task) = fixture(mode).await;
        let credentials = credentials(&base);
        let client = reqwest::Client::new();
        list_models(AccountProvider::Trae, credentials.clone(), &client)
            .await
            .unwrap();
        let result = route(
            AccountProvider::Trae,
            credentials,
            "fixture",
            json!({"messages":[],"stream":true}),
            &client,
        )
        .await;
        if mode == "partial" {
            let response = result.unwrap();
            assert!(axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .is_err());
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                error.status,
                if mode == "auth" {
                    StatusCode::UNAUTHORIZED
                } else {
                    StatusCode::TOO_MANY_REQUESTS
                }
            );
            assert_eq!(error.retry_after.unwrap(), "7");
        }
        assert_eq!(
            state
                .calls
                .lock()
                .await
                .iter()
                .filter(|(path, _)| path == CHAT_PATH)
                .count(),
            1
        );
        task.abort();
    }
    for mode in ["batch401", "batch429"] {
        let (base, state, task) = fixture(mode).await;
        let error = list_models(
            AccountProvider::Trae,
            credentials(&base),
            &reqwest::Client::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.status,
            if mode == "batch401" {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::TOO_MANY_REQUESTS
            }
        );
        assert_eq!(state.calls.lock().await.len(), 1);
        task.abort();
    }
}
