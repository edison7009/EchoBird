use super::*;
use axum::extract::OriginalUri;
use tokio::sync::Notify;

#[derive(Clone)]
struct Call {
    path: String,
    headers: HeaderMap,
    body: Value,
}

impl Call {
    fn model(&self) -> &str {
        self.body["model"].as_str().unwrap()
    }

    fn is_probe(&self) -> bool {
        !self.body.to_string().contains("business-secret")
    }
}

#[derive(Default)]
struct Fixture {
    calls: Mutex<Vec<Call>>,
    failing: Mutex<HashSet<String>>,
    business_responses: Mutex<HashMap<String, (StatusCode, &'static str, String)>>,
    blocked: Mutex<HashMap<String, Arc<Notify>>>,
    stream_gate: Mutex<Option<(Arc<Notify>, Arc<Notify>)>>,
}

struct Upstream {
    base: String,
    fixture: Arc<Fixture>,
    task: tokio::task::JoinHandle<()>,
}

impl Upstream {
    async fn start() -> Self {
        let fixture = Arc::new(Fixture::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v1/chat/completions", post(reply))
            .route("/v1/responses", post(reply))
            .with_state(fixture.clone());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            base,
            fixture,
            task,
        }
    }

    fn candidates(&self, ids: &[&str]) -> Vec<Candidate> {
        ids.iter()
            .map(|id| Candidate {
                internal_id: (*id).into(),
                model_id: (*id).into(),
                base_url: self.base.clone(),
                anthropic_url: None,
                api_key: "standby-fixture-key".into(),
                account_source: None,
                fingerprint: candidate_fingerprint(id, &self.base, "standby-fixture-key"),
                attempt: None,
            })
            .collect()
    }

    fn calls(&self) -> Vec<Call> {
        self.fixture.calls.lock().unwrap().clone()
    }

    fn probes(&self) -> Vec<String> {
        self.calls()
            .iter()
            .filter(|call| call.is_probe())
            .map(|call| call.model().to_owned())
            .collect()
    }

    fn fail(&self, id: &str) {
        self.fixture.failing.lock().unwrap().insert(id.into());
    }

    fn block(&self, id: &str) -> Arc<Notify> {
        let release = Arc::new(Notify::new());
        self.fixture
            .blocked
            .lock()
            .unwrap()
            .insert(id.into(), release.clone());
        release
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn reply(
    State(fixture): State<Arc<Fixture>>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let model = body["model"].as_str().unwrap().to_owned();
    let probe = !body.to_string().contains("business-secret");
    fixture.calls.lock().unwrap().push(Call {
        path: uri.path().into(),
        headers,
        body: body.clone(),
    });
    if !probe {
        if let Some((status, content_type, payload)) = fixture
            .business_responses
            .lock()
            .unwrap()
            .get(&model)
            .cloned()
        {
            return Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, content_type)
                .body(Body::from(payload))
                .unwrap();
        }
    }
    let blocked = fixture.blocked.lock().unwrap().get(&model).cloned();
    if let Some(release) = blocked {
        release.notified().await;
    }
    if fixture.failing.lock().unwrap().contains(&model) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(
                json!({"error":{"code":"insufficient_quota","message":"fixture quota exhausted"}}),
            ),
        )
            .into_response();
    }
    let id = format!("resp_{}_{model}", if probe { "probe" } else { "business" });
    if uri.path() == "/v1/responses" {
        let response = json!({"id":id,"object":"response","status":"completed",
            "output":[{"id":"msg_fixture","type":"message","role":"assistant",
            "status":"completed","content":[{"type":"output_text","text":"ok","annotations":[]}]}]});
        if body["stream"] == true {
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(format!(
                    "event: response.output_text.delta\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
                    json!({"type":"response.output_text.delta","delta":"ok","response_id":id}),
                    json!({"type":"response.completed","response":response})
                )))
                .unwrap();
        }
        return Json(response).into_response();
    }
    if body["stream"] == true {
        let delta = json!({"id":id,"object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"content":"ok"},"finish_reason":null}]});
        let done = json!({"id":id,"object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
        let gate = (!probe)
            .then(|| fixture.stream_gate.lock().unwrap().clone())
            .flatten();
        if let Some((entered, release)) = gate {
            let (sender, receiver) = tokio::sync::mpsc::channel(2);
            tokio::spawn(async move {
                let role = json!({"choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]});
                sender
                    .send(Ok::<_, std::convert::Infallible>(Bytes::from(format!(
                        "data: {role}\n\n"
                    ))))
                    .await
                    .unwrap();
                entered.notify_one();
                release.notified().await;
                let _ = sender
                    .send(Ok(Bytes::from(format!(
                        "data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n"
                    ))))
                    .await;
            });
            return Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(receiver)))
                .unwrap();
        }
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(format!(
                "data: {delta}\n\ndata: {done}\n\ndata: [DONE]\n\n"
            )))
            .unwrap();
    }
    Json(json!({"id":id,"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]})).into_response()
}

fn state() -> AppState {
    let mut state = AppState::for_tests().unwrap();
    state.standby = Arc::new(standby::Standby::new());
    state.activity = Arc::new(Mutex::new(RouteActivity::default()));
    state
}

async fn wait_idle(state: &AppState) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while state.standby.snapshot().2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("standby round should finish");
}

async fn wait_probe(upstream: &Upstream, model: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !upstream.probes().iter().any(|id| id == model) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("expected standby request should reach fixture");
}

async fn business(state: &AppState, candidates: Vec<Candidate>) -> String {
    let response = route_chat_with_candidates(
        state,
        &HeaderMap::new(),
        json!({"messages":[{"role":"user","content":"business-secret"}]}),
        candidates,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let via = response.headers()["X-EchoBird-Routed-Via"]
        .to_str()
        .unwrap()
        .to_owned();
    response.into_body().collect().await.unwrap();
    via
}

#[tokio::test]
async fn sequential_standby_stops_at_one_ready_and_advances_only_after_real_failover() {
    let upstream = Upstream::start().await;
    upstream.fail("two");
    let candidates = upstream.candidates(&["one", "two", "three", "four"]);
    let state = state();
    assert_eq!(business(&state, candidates.clone()).await, "one/one");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three"]);
    assert_eq!(business(&state, candidates.clone()).await, "one/one");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three"]);

    state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .get_mut("two")
        .expect("failed standby should have resource health")
        .cooldown_until_ms = Some(now_ms().saturating_sub(1));
    upstream.fail("one");
    assert_eq!(business(&state, candidates.clone()).await, "three/three");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three", "four"]);
    assert_eq!(
        upstream
            .calls()
            .iter()
            .filter(|call| !call.is_probe())
            .map(|call| call.model().to_owned())
            .collect::<Vec<_>>(),
        ["one", "one", "one", "three"],
        "ready standby takes over without retrying an expired but still failing earlier model"
    );

    upstream.fixture.failing.lock().unwrap().remove("one");
    state.route_memory.lock().unwrap().candidates.remove("one");
    assert_eq!(business(&state, candidates).await, "three/three");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three", "four"]);
    state.shutdown.cancel();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_groups_and_subagents_share_one_probe_for_the_same_resource() {
    let upstream = Upstream::start().await;
    let release = upstream.block("two");
    let first = state();
    let mut second = state();
    second.probe_pool = first.probe_pool.clone();
    second.route_memory = first.route_memory.clone();
    let candidates = upstream.candidates(&["one", "two", "three"]);
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..16 {
        let state = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        let mut candidates = candidates.clone();
        if index % 2 != 0 {
            for candidate in &mut candidates {
                candidate.internal_id = format!("alias-{}", candidate.internal_id);
            }
        }
        requests.spawn(async move { business(&state, candidates).await });
    }
    while let Some(result) = requests.join_next().await {
        let via = result.unwrap();
        assert!(via == "one/one" || via == "alias-one/one");
    }
    wait_probe(&upstream, "two").await;
    assert_eq!(upstream.probes(), ["two"]);
    release.notify_one();
    wait_idle(&first).await;
    wait_idle(&second).await;
    assert_eq!(upstream.probes(), ["two"]);
    first.shutdown.cancel();
    second.shutdown.cancel();
}

#[tokio::test]
async fn unavailable_standbys_finish_one_circle_without_reprobing_the_working_model() {
    let upstream = Upstream::start().await;
    upstream.fail("two");
    upstream.fail("three");
    let state = state();
    let candidates = upstream.candidates(&["one", "two", "three"]);
    assert_eq!(business(&state, candidates.clone()).await, "one/one");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three"]);
    assert_eq!(business(&state, candidates).await, "one/one");
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "three"]);
    state.shutdown.cancel();
}

#[tokio::test]
async fn standby_requests_do_not_forward_user_context_or_change_activity_and_bindings() {
    let upstream = Upstream::start().await;
    let release = upstream.block("two");
    let state = state();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-session-id",
        HeaderValue::from_static("business-secret-session"),
    );
    headers.insert(
        header::USER_AGENT,
        HeaderValue::from_static("business-secret-user-agent"),
    );
    let response = route_responses_with_candidates(
        &state,
        &headers,
        json!({"input":"business-secret", "metadata":{"private":"business-secret"},
            "tools":[{"type":"function","name":"private_tool","parameters":{"type":"object"}}]}),
        upstream.candidates(&["one", "two", "three"]),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    wait_probe(&upstream, "two").await;
    let activity = state.activity.lock().unwrap().snapshot();
    assert!(!activity.active);
    assert_eq!(activity.candidate_id.as_deref(), Some("one"));
    assert_eq!(activity.sequence, 2);
    let calls = upstream.calls();
    assert_eq!(
        calls[0].headers[header::USER_AGENT],
        "business-secret-user-agent"
    );
    let call = calls.iter().find(|call| call.is_probe()).unwrap();
    assert!(matches!(
        call.path.as_str(),
        "/v1/chat/completions" | "/v1/responses"
    ));
    assert!(!call.body.to_string().contains("business-secret"));
    assert!(call.body.get("tools").is_none());
    assert!(call.body.get("metadata").is_none());
    assert!(call.body.get("previous_response_id").is_none());
    assert!(!call.headers.contains_key("x-session-id"));
    assert!(!call.headers.contains_key(header::USER_AGENT));
    release.notify_one();
    wait_idle(&state).await;
    let memory = state.response_memory.lock().unwrap();
    assert!(memory.response_bindings.contains_key("resp_business_one"));
    assert_eq!(memory.response_bindings.len(), 1);
    state.shutdown.cancel();
}

#[tokio::test]
async fn shutting_down_during_a_probe_cannot_start_the_next_candidate() {
    let upstream = Upstream::start().await;
    upstream.fail("two");
    let release = upstream.block("two");
    let state = state();
    assert_eq!(
        business(&state, upstream.candidates(&["one", "two", "three"])).await,
        "one/one"
    );
    wait_probe(&upstream, "two").await;
    state.shutdown.cancel();
    release.notify_one();
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two"]);
}

#[tokio::test]
async fn streaming_starts_standby_after_content_and_keeps_the_visible_model_active() {
    let upstream = Upstream::start().await;
    let entered = Arc::new(Notify::new());
    let release_content = Arc::new(Notify::new());
    *upstream.fixture.stream_gate.lock().unwrap() =
        Some((entered.clone(), release_content.clone()));
    let release_probe = upstream.block("two");
    let state = state();
    let request = tokio::spawn({
        let state = state.clone();
        let candidates = upstream.candidates(&["one", "two", "three"]);
        async move {
            route_chat_with_candidates(
                &state,
                &HeaderMap::new(),
                json!({"messages":[{"role":"user","content":"business-secret"}],"stream":true}),
                candidates,
            )
            .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    assert!(!request.is_finished(), "role-only SSE is not usable output");
    assert!(upstream.probes().is_empty());
    release_content.notify_one();
    let response = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    wait_probe(&upstream, "two").await;
    let activity = state.activity.lock().unwrap().snapshot();
    assert!(activity.active);
    assert_eq!(activity.active_candidate_ids, ["one"]);
    assert_eq!(activity.candidate_id.as_deref(), Some("one"));
    assert_eq!(activity.sequence, 1);
    response.into_body().collect().await.unwrap();
    release_probe.notify_one();
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two"]);
    assert!(!state.activity.lock().unwrap().snapshot().active);
    state.shutdown.cancel();
}

#[tokio::test]
async fn configuration_invalidation_discards_a_pending_old_probe_result() {
    let upstream = Upstream::start().await;
    let release = upstream.block("two");
    let mut state = state();
    state.config_path = PathBuf::from(format!("standby-fixture-{}", upstream.base));
    standby::register(&state.config_path, &state.standby);
    assert_eq!(
        business(&state, upstream.candidates(&["one", "two", "three"])).await,
        "one/one"
    );
    wait_probe(&upstream, "two").await;
    invalidate_standby(&state.config_path);
    assert_eq!(state.standby.snapshot().0, None);
    assert_eq!(state.standby.snapshot().1, None);
    release.notify_one();
    assert_eq!(
        business(&state, upstream.candidates(&["three", "one"])).await,
        "three/three"
    );
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two", "one"]);
    assert_eq!(state.standby.snapshot().0.as_deref(), Some("three"));
    assert_eq!(state.standby.snapshot().1.as_deref(), Some("one"));
    state.shutdown.cancel();
}

#[tokio::test]
async fn cancelling_one_group_preserves_another_groups_shared_probe() {
    let upstream = Upstream::start().await;
    let release = upstream.block("two");
    let first = state();
    let mut second = state();
    second.probe_pool = first.probe_pool.clone();
    second.route_memory = first.route_memory.clone();
    let candidates = upstream.candidates(&["one", "two", "three"]);
    assert_eq!(business(&first, candidates.clone()).await, "one/one");
    wait_probe(&upstream, "two").await;
    assert_eq!(business(&second, candidates.clone()).await, "one/one");
    tokio::time::timeout(Duration::from_secs(5), async {
        while first
            .probe_pool
            .owner_count(&candidates[1], RouteProtocol::Chat)
            < 2
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both groups must hold the same pending probe before cancellation");
    assert!(second.standby.snapshot().2);
    first.shutdown.cancel();
    wait_idle(&first).await;
    assert!(second.standby.snapshot().2);
    assert_eq!(upstream.probes(), ["two"]);
    release.notify_one();
    wait_idle(&second).await;
    assert_eq!(second.standby.snapshot().1.as_deref(), Some("two"));
    assert_eq!(upstream.probes(), ["two"]);
    second.shutdown.cancel();
}

#[tokio::test]
async fn reading_model_catalog_does_not_probe_until_a_real_business_request_succeeds() {
    let upstream = Upstream::start().await;
    let mut state = state();
    let path = std::env::temp_dir().join(format!(
        "echobird-standby-models-{}.json",
        uuid::Uuid::new_v4()
    ));
    fs::write(
        &path,
        serde_json::to_vec(&json!({"apiKey":"fixture-local-key","candidateIds":["one","two"]}))
            .unwrap(),
    )
    .unwrap();
    state.config_path = path.clone();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer fixture-local-key"),
    );
    let response = handle_models(State(state.clone()), headers).await;
    fs::remove_file(path).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    assert!(upstream.calls().is_empty());
    assert_eq!(state.standby.snapshot().0, None);
    assert_eq!(state.activity.lock().unwrap().snapshot().sequence, 0);
    assert_eq!(
        business(&state, upstream.candidates(&["one", "two"])).await,
        "one/one"
    );
    wait_idle(&state).await;
    assert_eq!(upstream.probes(), ["two"]);
    state.shutdown.cancel();
}

#[tokio::test]
async fn legitimate_empty_business_output_preserves_response_without_advancing_standby() {
    let incomplete = json!({"id":"resp_empty","object":"response","status":"incomplete",
        "incomplete_details":{"reason":"max_output_tokens"},"output":[]});
    let cases = [
        ("stream usage", RouteProtocol::Chat, true,
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"completion_tokens\":10}}\n\ndata: [DONE]\n\n".to_owned()),
        ("stream length", RouteProtocol::Chat, true,
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("stream filter", RouteProtocol::Chat, true,
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n\ndata: [DONE]\n\n".to_owned()),
        ("JSON length", RouteProtocol::Chat, false,
            json!({"choices":[{"message":{"role":"assistant","content":""},"finish_reason":"length"}]}).to_string()),
        ("JSON usage", RouteProtocol::Chat, false,
            json!({"choices":[{"message":{"role":"assistant","content":""},"finish_reason":"stop"}],"usage":{"completion_tokens":10}}).to_string()),
        ("JSON incomplete", RouteProtocol::Responses, false, incomplete.to_string()),
        ("stream incomplete", RouteProtocol::Responses, true,
            format!("event: response.incomplete\ndata: {}\n\n", json!({"type":"response.incomplete","response":incomplete}))),
    ];
    for (name, protocol, streaming, payload) in cases {
        let upstream = Upstream::start().await;
        let candidates = upstream.candidates(&["one", "two", "three"]);
        let state = state();
        *upstream.fixture.business_responses.lock().unwrap() = HashMap::from([
            (
                "one".into(),
                (
                    StatusCode::BAD_REQUEST,
                    "application/json",
                    "{\"error\":{\"code\":\"unsupported_parameter\"}}".into(),
                ),
            ),
            (
                "two".into(),
                (
                    StatusCode::OK,
                    if streaming {
                        "text/event-stream"
                    } else {
                        "application/json"
                    },
                    payload.clone(),
                ),
            ),
        ]);
        let response = route_with_policy(
            &state,
            &HeaderMap::new(),
            json!({"messages":[{"role":"user","content":"business-secret"}],"input":"business-secret","stream":streaming}),
            candidates,
            RoutePolicy::default(),
            protocol,
        ).await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(
            response.headers()["X-EchoBird-Routed-Via"],
            "two/two",
            "{name}"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            bytes.as_ref(),
            payload.as_bytes(),
            "{name}: business output must remain unchanged"
        );
        assert_eq!(
            upstream
                .calls()
                .iter()
                .filter(|call| !call.is_probe())
                .map(|call| call.model().to_owned())
                .collect::<Vec<_>>(),
            ["one", "two"],
            "{name}: business output must not be replayed"
        );
        assert_eq!(
            state.standby.snapshot().0.as_deref(),
            Some("one"),
            "{name}: no effective output must not move the working position"
        );
        assert!(state.standby.snapshot().1.is_none(), "{name}");
        assert!(
            !state.standby.snapshot().2,
            "{name}: no effective output must not start a probe"
        );
        assert!(
            upstream.probes().is_empty(),
            "{name}: no real standby request is allowed"
        );
        state.shutdown.cancel();
    }
}
