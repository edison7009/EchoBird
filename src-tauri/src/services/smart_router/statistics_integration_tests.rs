use super::super::statistics::Collector;
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    root: PathBuf,
    state: AppState,
    collector: Arc<Collector>,
}

impl Fixture {
    fn new(recording: bool, protection: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("router-statistics-route-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let collector = Collector::open(root.join("counts.json")).unwrap();
        collector.configure(recording);
        let mut state = AppState::for_tests().unwrap();
        state.config_path = root.join("group.json");
        state.protection_settings_path = root.join("settings.json");
        fs::write(
            &state.protection_settings_path,
            json!({"sensitiveInfoProtection":protection}).to_string(),
        )
        .unwrap();
        state.statistics_collector = Some(collector.clone());
        Self {
            root,
            state,
            collector,
        }
    }
    fn responses(&mut self, responses: Vec<(&str, Response)>) {
        self.state.fixture_responses = Some(Arc::new(Mutex::new(
            responses
                .into_iter()
                .map(|(id, response)| (id.into(), response))
                .collect(),
        )));
    }
    fn snapshot(&self) -> super::super::statistics::Snapshot {
        self.collector.snapshot(1, None).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}
fn model(id: &str) -> Candidate {
    Candidate {
        internal_id: id.into(),
        model_id: format!("{id}-model"),
        base_url: "http://127.0.0.1:1/v1".into(),
        anthropic_url: None,
        api_key: "fixture-auth".into(),
        fingerprint: id.into(),
        account_source: None,
        attempt: None,
    }
}
fn chat(text: &str) -> Response {
    crate::services::account_proxy::json_response(
        &json!({"choices":[{"message":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":123,"completion_tokens":456}}),
    )
}
fn request(stream: bool) -> Value {
    json!({"model":"auto","messages":[{"role":"user","content":"hello"}],"stream":stream})
}
fn sse(text: &str) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(text.to_owned()))
        .unwrap()
}
const PARTIAL: &str =
    "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n";
const COMPLETE: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

#[tokio::test]
async fn reported_tokens_share_statistics_and_durable_pet_credits_without_replay() {
    let mut fixture = Fixture::new(true, false);
    fixture.responses(vec![("tokens", chat("private response fixture"))]);
    let response = protected_route(
        &fixture.state,
        &HeaderMap::new(),
        request(false),
        vec![model("tokens")],
        ClientProtocol::Chat,
    )
    .await;
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["usage"]["prompt_tokens"],
        123
    );
    assert_eq!(fixture.snapshot().totals.usage.total_tokens, 579);
    assert_eq!(
        fixture.snapshot().rows[0].counts.usage.reported_responses,
        1
    );
    assert_eq!(fixture.collector.token_credits().unwrap().total, 0);
    fixture.collector.flush().unwrap();
    let credits = fixture.collector.token_credits().unwrap();
    assert_eq!(credits.total, 579);
    assert_eq!(fixture.collector.token_credits().unwrap().total, 579);
    let disk = fs::read_to_string(fixture.root.join("counts.json")).unwrap();
    assert!(!disk.contains("private response fixture"));
    assert!(!disk.contains("fixture-auth"));
    let restored = Collector::open(fixture.root.join("counts.json")).unwrap();
    restored.configure(true);
    assert_eq!(
        restored.token_credits().unwrap().ledger_id,
        credits.ledger_id
    );
    assert_eq!(restored.token_credits().unwrap().total, 579);
}

#[tokio::test]
async fn protection_response_failure_does_not_leave_an_account_success() {
    let mut fixture = Fixture::new(true, true);
    fixture.responses(vec![(
        "broken",
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("invalid upstream json"))
            .unwrap(),
    )]);
    let response = protected_route(
        &fixture.state,
        &HeaderMap::new(),
        request(false),
        vec![model("broken")],
        ClientProtocol::Chat,
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let _ = response.into_body().collect().await.unwrap();
    let snapshot = fixture.snapshot();
    assert_eq!((snapshot.totals.completed, snapshot.totals.failed), (0, 1));
    let counts = &snapshot.rows[0].counts;
    assert_eq!(
        (counts.attempts, counts.completed, counts.failed),
        (1, 0, 1)
    );
    assert_eq!(counts.errors, [0, 0, 0, 0, 0, 1]);
}

#[tokio::test]
async fn actual_http_three_protocols_keep_protection_and_body_identical_with_recording_off_or_on() {
    const SECRET: &str = "sk-statisticsfixtureabcdefghijklmnopqrstuvwx";
    for recording in [false, true] {
        for protocol in [
            ClientProtocol::Chat,
            ClientProtocol::Responses,
            ClientProtocol::Messages,
        ] {
            let fixture = Fixture::new(recording, true);
            let hits = Arc::new(AtomicUsize::new(0));
            let count = hits.clone();
            let app = Router::new().route("/v1/{*path}", post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let hits = count.clone();
                async move {
                    hits.fetch_add(1,Ordering::SeqCst);
                    assert_eq!(headers[header::AUTHORIZATION], "Bearer fixture-auth");
                    assert!(!body.to_string().contains(SECRET));
                    let echo = match protocol { ClientProtocol::Responses => body["input"].as_str().unwrap(), _=>body["messages"][0]["content"].as_str().unwrap() };
                    Json(match protocol {
                        ClientProtocol::Responses => json!({"id":"resp_fixture","object":"response","status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":echo}]}],"usage":{"input_tokens":123,"output_tokens":456}}),
                        _=>json!({"choices":[{"message":{"role":"assistant","content":echo},"finish_reason":"stop"}],"usage":{"prompt_tokens":123,"completion_tokens":456}}),
                    })
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let mut candidate = model("private-alias");
            candidate.base_url = format!("http://{addr}/v1");
            let content = format!("{SECRET} {SECRET} password=privatefixturepassword");
            let body = match protocol {
                ClientProtocol::Responses => json!({"model":"auto","input":content,"stream":false}),
                _ => {
                    json!({"model":"auto","messages":[{"role":"user","content":content}],"max_tokens":64,"stream":false})
                }
            };
            let response = protected_route(
                &fixture.state,
                &HeaderMap::new(),
                body,
                vec![candidate],
                protocol,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert!(String::from_utf8_lossy(&bytes).contains(&content));
            assert_eq!(
                hits.load(Ordering::SeqCst),
                1,
                "positive control: real HTTP upstream was used"
            );
            let snapshot = fixture.snapshot();
            assert_eq!(snapshot.totals.calls, u64::from(recording));
            assert_eq!(snapshot.totals.completed, u64::from(recording));
            if recording {
                assert_eq!(snapshot.rows[0].counts.protections, [2, 0, 1, 0, 0]);
                assert_eq!(snapshot.rows[0].counts.attempts, 1);
                fixture.collector.flush().unwrap();
                let stored = fs::read_to_string(fixture.root.join("counts.json")).unwrap();
                for private in [
                    SECRET,
                    "privatefixturepassword",
                    "private-alias",
                    "fixture-auth",
                    "http://",
                    "hello",
                ] {
                    assert!(
                        !stored.contains(private),
                        "persisted private data: {private}"
                    );
                }
            } else {
                fixture.collector.flush().unwrap();
                assert!(!fixture.root.join("counts.json").exists());
            }
            server.abort();
        }
    }
}

#[tokio::test]
async fn fallback_counts_one_call_two_attempts_and_one_rescue() {
    let mut fixture = Fixture::new(true, true);
    fixture.responses(vec![
        (
            "first",
            (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":{"message":"rate limit"}})),
            )
                .into_response(),
        ),
        ("second", chat("safe reply")),
    ]);
    let mut body = request(false);
    body["messages"][0]["content"] = json!(
        "api_key=sk-fallbackfixtureabcdefghijklmnopqrstuvwx password=fallbackfixturepassword"
    );
    let response = protected_route(
        &fixture.state,
        &HeaderMap::new(),
        body,
        vec![model("first"), model("second")],
        ClientProtocol::Chat,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    let snapshot = fixture.snapshot();
    assert_eq!(
        (
            snapshot.totals.calls,
            snapshot.totals.completed,
            snapshot.totals.rescued
        ),
        (1, 1, 1)
    );
    assert_eq!(
        snapshot
            .rows
            .iter()
            .map(|row| row.counts.attempts)
            .sum::<u64>(),
        2
    );
    assert_eq!(
        snapshot
            .rows
            .iter()
            .map(|row| row.counts.errors[0])
            .sum::<u64>(),
        1
    );
    assert!(snapshot
        .rows
        .iter()
        .all(|row| row.counts.protections == [1, 0, 1, 0, 0]));
    assert!(fixture
        .state
        .fixture_responses
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn refreshed_authentication_is_one_attempt_and_probe_is_excluded_with_a_positive_control() {
    let mut fixture = Fixture::new(true, true);
    let mut unauthorized = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":{"message":"expired credential"}})),
    )
        .into_response();
    unauthorized
        .extensions_mut()
        .insert(RejectedAccountCredentials(
            json!({"accessToken":"old-fixture"}),
        ));
    fixture.responses(vec![("account", unauthorized), ("probe", sse(COMPLETE))]);
    fixture.state.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
        result: Ok(json!({"accessToken":"new-fixture"})),
        delay: Duration::ZERO,
        rejections: vec![],
        retry_responses: VecDeque::from([chat("refreshed")]),
        sent_credentials: vec![],
    })));
    let mut candidate = model("account");
    candidate.account_source = Some(crate::models::model::AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "statistics-refresh-fixture".into(),
    });
    let mut body = request(false);
    body["messages"][0]["content"] =
        json!("api_key=sk-refreshfixtureabcdefghijklmnopqrstuvwx password=refreshfixturepassword");
    let response = protected_route(
        &fixture.state,
        &HeaderMap::new(),
        body,
        vec![candidate],
        ClientProtocol::Chat,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    let snapshot = fixture.snapshot();
    assert_eq!(
        (
            snapshot.totals.completed,
            snapshot.totals.rescued,
            snapshot.rows[0].counts.attempts,
            snapshot.rows[0].counts.failed
        ),
        (1, 0, 1, 0)
    );
    assert_eq!(snapshot.rows[0].counts.protections, [1, 0, 1, 0, 0]);
    assert_eq!(
        fixture
            .state
            .fixture_authentication
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .sent_credentials
            .len(),
        1
    );
    // Standby receives a clone of the request's state; it must strip that context.
    let request = fixture.collector.begin("group").unwrap();
    request.attempt("group", "real", "real-model", None);
    fixture.state.request_statistics = Some(request.clone());
    assert!(standby_probe::check(&fixture.state, model("probe"), RouteProtocol::Chat).await);
    assert!(
        fixture
            .state
            .fixture_responses
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .is_empty(),
        "probe actually sent and completed"
    );
    assert!(
        !request.settled(),
        "probe must not complete the real request"
    );
    assert_eq!(
        fixture
            .snapshot()
            .rows
            .iter()
            .map(|row| row.counts.attempts)
            .sum::<u64>(),
        2
    );
    request.finish(false, true);
}

#[tokio::test]
async fn streaming_completion_truncation_error_and_cancellation_are_distinct() {
    for ending in ["complete", "truncated", "error", "cancel"] {
        let mut fixture = Fixture::new(true, false);
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
        tx.send(Ok(Bytes::from_static(PARTIAL.as_bytes())))
            .await
            .unwrap();
        let response = Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(ReceiverStream::new(rx)))
            .unwrap();
        fixture.responses(vec![("stream", response)]);
        let response = protected_route(
            &fixture.state,
            &HeaderMap::new(),
            request(true),
            vec![model("stream")],
            ClientProtocol::Chat,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{ending}");
        let mut body = response.into_body();
        assert!(body.frame().await.unwrap().is_ok());
        if ending == "complete" {
            tx.send(Ok(Bytes::from_static(COMPLETE.as_bytes())))
                .await
                .unwrap();
        } else if ending == "error" {
            tx.send(Ok(Bytes::from_static(
                b"data: {\"error\":{\"message\":\"server error\",\"type\":\"server_error\"}}\n\n",
            )))
            .await
            .unwrap();
        }
        if ending == "cancel" {
            drop(body);
            drop(tx);
        } else {
            drop(tx);
            let _ = body.collect().await;
        }
        let snapshot = fixture.snapshot();
        let expected = match ending {
            "complete" => (1, 0, 0),
            "cancel" => (0, 0, 1),
            _ => (0, 1, 0),
        };
        assert_eq!(
            (
                snapshot.totals.completed,
                snapshot.totals.failed,
                snapshot.totals.cancelled
            ),
            expected,
            "{ending}"
        );
        assert_eq!(
            (
                snapshot.rows[0].counts.completed,
                snapshot.rows[0].counts.failed,
                snapshot.rows[0].counts.cancelled
            ),
            expected,
            "{ending}"
        );
    }
}
