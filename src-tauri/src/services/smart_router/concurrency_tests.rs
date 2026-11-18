use super::*;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Barrier, Notify};

struct Mock {
    base: String,
    task: tokio::task::JoinHandle<()>,
}

impl Mock {
    async fn start(app: Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { base, task }
    }

    fn candidate(&self, id: &str) -> Candidate {
        let mut candidate = model(id);
        candidate.base_url = self.base.clone();
        candidate.fingerprint = resource_key(&candidate);
        candidate
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn another_group(first: &AppState) -> AppState {
    let mut next = AppState::for_tests().unwrap();
    next.route_memory = first.route_memory.clone();
    next.activity = Arc::new(Mutex::new(RouteActivity::default()));
    next
}

fn expire(state: &AppState, candidate: &Candidate) {
    mark_failure(state, candidate, FailureClass::Transient, None);
    state
        .route_memory
        .lock()
        .unwrap()
        .candidates
        .get_mut(&candidate.internal_id)
        .unwrap()
        .cooldown_until_ms = Some(now_ms().saturating_sub(1));
}

fn ok() -> Json<Value> {
    Json(
        json!({"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}),
    )
}

fn model(id: &str) -> Candidate {
    Candidate {
        internal_id: id.into(),
        model_id: "concurrent-model".into(),
        base_url: "http://127.0.0.1:1/v1".into(),
        anthropic_url: None,
        api_key: "fixture-key".into(),
        account_source: None,
        fingerprint: candidate_fingerprint(
            "concurrent-model",
            "http://127.0.0.1:1/v1",
            "fixture-key",
        ),
        attempt: None,
    }
}

#[test]
fn concurrent_failure_wave_advances_transient_backoff_once() {
    let state = AppState::for_tests().unwrap();
    let requests: Vec<_> = (0..32)
        .map(|_| {
            let mut candidate = model("shared");
            begin_attempt(&state, &mut candidate);
            candidate
        })
        .collect();
    for candidate in &requests {
        mark_failure(&state, candidate, FailureClass::Transient, None);
    }
    let memory = state.route_memory.lock().unwrap();
    let health = &memory.candidates["shared"];
    assert_eq!(health.consecutive_failures, 1);
    assert!(health.cooldown_until_ms.unwrap() <= now_ms() + 30_000);
}

#[test]
fn edited_model_ignores_old_in_flight_failure_and_success() {
    let state = AppState::for_tests().unwrap();
    let mut old = model("editable");
    begin_attempt(&state, &mut old);
    clear_candidate_health(&mut state.route_memory.lock().unwrap(), "editable");
    mark_failure(&state, &old, FailureClass::Authentication, None);
    assert!(state.route_memory.lock().unwrap().candidates.is_empty());

    let mut replacement = model("editable");
    replacement.api_key = "new-key".into();
    replacement.fingerprint = candidate_fingerprint(
        &replacement.model_id,
        &replacement.base_url,
        &replacement.api_key,
    );
    begin_attempt(&state, &mut replacement);
    mark_failure(&state, &replacement, FailureClass::Transient, None);
    mark_success(&state, &old);
    let memory = state.route_memory.lock().unwrap();
    assert_eq!(
        memory.candidates["editable"].fingerprint,
        replacement.fingerprint
    );
}

#[test]
fn old_failure_cannot_reopen_a_successfully_recovered_resource() {
    let state = AppState::for_tests().unwrap();
    let mut old = model("old");
    begin_attempt(&state, &mut old);
    expire(&state, &old);
    let mut recovered = model("alias");
    assert!(try_begin_attempt(&state, &mut recovered));
    mark_success(&state, &recovered);
    mark_failure(&state, &old, FailureClass::Transient, None);
    assert!(state.route_memory.lock().unwrap().candidates.is_empty());
    // A slow, valid response can still be continued even though its health
    // outcome predates recovery and must no longer change the circuit.
    remember_response(&state, &old, Some("resp_slow_success"));
    assert!(response_candidates(
        &state,
        &json!({"previous_response_id":"resp_slow_success"}),
        vec![old],
        RouteProtocol::Responses
    )
    .is_ok());
}

#[test]
fn successful_response_can_continue_after_same_account_token_rotation() {
    let state = AppState::for_tests().unwrap();
    let mut old = model("rotated-response");
    old.account_source = Some(crate::models::model::AccountSource {
        provider: AccountProvider::Codex,
        account_id: "response-rotation-fixture".into(),
    });
    old.fingerprint = resource_key(&old);
    begin_attempt(&state, &mut old);
    bind_attempt_credentials(&state, &old, &json!({"accessToken":"old"}));
    let mut refreshed = old.clone();
    begin_attempt(&state, &mut refreshed);
    bind_attempt_credentials(&state, &refreshed, &json!({"accessToken":"fresh"}));
    mark_failure(&state, &old, FailureClass::Authentication, None);
    assert!(state.route_memory.lock().unwrap().candidates.is_empty());
    remember_response(&state, &old, Some("resp_valid_before_refresh"));
    assert!(response_candidates(
        &state,
        &json!({"previous_response_id":"resp_valid_before_refresh"}),
        vec![old.clone()],
        RouteProtocol::Responses
    )
    .is_ok());

    // Signing in again still invalidates the old request, unlike token refresh.
    *state
        .route_memory
        .lock()
        .unwrap()
        .account_epochs
        .entry(account_health_key(&old).unwrap())
        .or_default() += 1;
    remember_response(&state, &old, Some("resp_after_sign_in"));
    assert!(!state
        .response_memory
        .lock()
        .unwrap()
        .response_bindings
        .contains_key("resp_after_sign_in"));
}

#[test]
fn cancelled_or_failed_recovery_releases_its_lease() {
    let state = AppState::for_tests().unwrap();
    let mut first = model("probe");
    expire(&state, &first);
    assert!(try_begin_attempt(&state, &mut first));
    let mut second = model("alias");
    assert!(!try_begin_attempt(&state, &mut second));
    drop(first);
    assert!(try_begin_attempt(&state, &mut second));
    mark_failure(&state, &second, FailureClass::Transient, None);
    assert_eq!(
        state.route_memory.lock().unwrap().candidates["alias"].consecutive_failures,
        2
    );
    drop(second);
    for health in state.route_memory.lock().unwrap().candidates.values_mut() {
        health.cooldown_until_ms = Some(now_ms().saturating_sub(1));
    }
    assert!(try_begin_attempt(&state, &mut model("third")));
}

#[test]
fn reauthenticated_account_does_not_wait_for_an_old_recovery_lease() {
    let state = AppState::for_tests().unwrap();
    let mut old = model("reauth-probe");
    old.account_source = Some(crate::models::model::AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "reauth-probe-fixture".into(),
    });
    old.fingerprint = resource_key(&old);
    mark_failure(&state, &old, FailureClass::RateLimit, Some(Duration::ZERO));
    assert!(try_begin_attempt(&state, &mut old));
    let mut fresh = old.clone();
    *state
        .route_memory
        .lock()
        .unwrap()
        .account_epochs
        .entry(account_health_key(&old).unwrap())
        .or_default() += 1;
    assert!(try_begin_attempt(&state, &mut fresh));
    mark_failure(&state, &old, FailureClass::Authentication, None);
    assert!(!candidate_is_cooling_down(&state, &fresh));
    let mut blocked = fresh.clone();
    blocked.internal_id = "blocked-during-fresh-probe".into();
    blocked.attempt = None;
    assert!(!try_begin_attempt(&state, &mut blocked));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_http_failures_share_one_backoff_window_and_skip_it_next_time() {
    const REQUESTS: usize = 32;
    let barrier = Arc::new(Barrier::new(REQUESTS));
    let hits = Arc::new(AtomicUsize::new(0));
    let upstream = Mock::start(Router::new().route(
        "/v1/chat/completions",
        post({
            let barrier = barrier.clone();
            let hits = hits.clone();
            move || {
                let barrier = barrier.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    barrier.wait().await;
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "upstream temporarily unavailable",
                    )
                }
            }
        }),
    ))
    .await;
    let backup =
        Mock::start(Router::new().route("/v1/chat/completions", post(|| async { ok() }))).await;
    let state = AppState::for_tests().unwrap();
    let candidates = vec![
        upstream.candidate("failure-wave"),
        backup.candidate("backup"),
    ];
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..REQUESTS {
        let state = state.clone();
        let candidates = candidates.clone();
        requests.spawn(async move {
            let response = route_chat_with_candidates(
                &state,
                &HeaderMap::new(),
                json!({"messages":[]}),
                candidates,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["X-EchoBird-Routed-Via"],
                "backup/concurrent-model"
            );
            response.into_body().collect().await.unwrap();
        });
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), REQUESTS);
    assert_eq!(
        state.route_memory.lock().unwrap().candidates["failure-wave"].consecutive_failures,
        1
    );
    let response = route_chat_with_candidates(
        &state,
        &HeaderMap::new(),
        json!({"messages":[]}),
        candidates,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    response.into_body().collect().await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), REQUESTS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn simultaneous_credential_rejections_share_refresh_result_and_reauth_invalidates_it() {
    for result in [
        Ok(json!({"accessToken":"fresh"})),
        Err("accountError.loginRequired".into()),
    ] {
        let mut first = AppState::for_tests().unwrap();
        first.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
            result: result.clone(),
            delay: Duration::ZERO,
            rejections: Vec::new(),
            retry_responses: VecDeque::new(),
            sent_credentials: Vec::new(),
        })));
        let mut second = another_group(&first);
        second.fixture_authentication = first.fixture_authentication.clone();
        let mut candidate = model("refresh");
        candidate.account_source = Some(crate::models::model::AccountSource {
            provider: AccountProvider::Kilo,
            account_id: "refresh-concurrency-fixture".into(),
        });
        let mut requests = tokio::task::JoinSet::new();
        for index in 0..32 {
            let state = if index % 2 == 0 {
                first.clone()
            } else {
                second.clone()
            };
            let candidate = candidate.clone();
            let expected = result.clone();
            requests.spawn(async move {
                let value = refresh_rejected_account(
                    &state,
                    &candidate,
                    &RejectedAccountCredentials(json!({"accessToken":"old"})),
                    Instant::now(),
                    RoutePolicy::default(),
                )
                .await;
                assert_eq!(value.is_ok(), expected.is_ok());
                if let Ok(expected) = expected {
                    assert_eq!(value.unwrap(), expected);
                }
            });
        }
        while let Some(result) = requests.join_next().await {
            result.unwrap();
        }
        assert_eq!(
            first
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .rejections
                .len(),
            1
        );
        let key = account_health_key(&candidate).unwrap();
        *first
            .route_memory
            .lock()
            .unwrap()
            .account_epochs
            .entry(key)
            .or_default() += 1;
        let _ = refresh_rejected_account(
            &second,
            &candidate,
            &RejectedAccountCredentials(json!({"accessToken":"old"})),
            Instant::now(),
            RoutePolicy::default(),
        )
        .await;
        assert_eq!(
            first
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .rejections
                .len(),
            2
        );
        assert!(!serde_json::to_string(&*first.route_memory.lock().unwrap())
            .unwrap()
            .contains("accessToken"));
    }
}

#[tokio::test]
async fn pending_refresh_does_not_block_other_accounts_and_cancellation_allows_retry() {
    let mut first = AppState::for_tests().unwrap();
    first.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
        result: Ok(json!({"accessToken":"fresh"})),
        delay: Duration::from_secs(60),
        rejections: Vec::new(),
        retry_responses: VecDeque::new(),
        sent_credentials: Vec::new(),
    })));
    let mut account = model("slow-refresh");
    account.account_source = Some(crate::models::model::AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "slow-refresh-fixture".into(),
    });
    let pending = tokio::spawn({
        let state = first.clone();
        let account = account.clone();
        async move {
            refresh_rejected_account(
                &state,
                &account,
                &RejectedAccountCredentials(json!({"accessToken":"old"})),
                Instant::now(),
                RoutePolicy::default(),
            )
            .await
        }
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !first
                .fixture_authentication
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .rejections
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let mut independent = another_group(&first);
    independent.fixture_authentication = Some(Arc::new(Mutex::new(AuthenticationFixture {
        result: Ok(json!({"accessToken":"independent"})),
        delay: Duration::ZERO,
        rejections: Vec::new(),
        retry_responses: VecDeque::new(),
        sent_credentials: Vec::new(),
    })));
    let mut other = account.clone();
    other.account_source.as_mut().unwrap().account_id = "independent-refresh-fixture".into();
    let value = tokio::time::timeout(
        Duration::from_secs(5),
        refresh_rejected_account(
            &independent,
            &other,
            &RejectedAccountCredentials(json!({"accessToken":"old"})),
            Instant::now(),
            RoutePolicy::default(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(value["accessToken"], "independent");
    assert!(
        !pending.is_finished(),
        "positive control: the first refresh is still pending"
    );
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    first
        .fixture_authentication
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .delay = Duration::ZERO;
    assert!(refresh_rejected_account(
        &first,
        &account,
        &RejectedAccountCredentials(json!({"accessToken":"old"})),
        Instant::now(),
        RoutePolicy::default()
    )
    .await
    .is_ok());
    assert_eq!(
        first
            .fixture_authentication
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .rejections
            .len(),
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn healthy_priority_remains_parallel_across_groups_and_subagents() {
    const REQUESTS: usize = 32;
    let entered = Arc::new(Barrier::new(REQUESTS + 1));
    let hits = Arc::new(AtomicUsize::new(0));
    let upstream = Mock::start(Router::new().route(
        "/v1/chat/completions",
        post({
            let entered = entered.clone();
            let hits = hits.clone();
            move || {
                let entered = entered.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    entered.wait().await;
                    ok()
                }
            }
        }),
    ))
    .await;
    let backup_hits = Arc::new(AtomicUsize::new(0));
    let backup = Mock::start(Router::new().route(
        "/v1/chat/completions",
        post({
            let hits = backup_hits.clone();
            move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    ok()
                }
            }
        }),
    ))
    .await;
    let first = AppState::for_tests().unwrap();
    let second = another_group(&first);
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..REQUESTS {
        let state = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        let candidates = vec![upstream.candidate("preferred"), backup.candidate("backup")];
        requests.spawn(async move {
            let response = route_chat_with_candidates(
                &state,
                &HeaderMap::new(),
                json!({"messages":[]}),
                candidates,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["X-EchoBird-Routed-Via"],
                "preferred/concurrent-model"
            );
            response.into_body().collect().await.unwrap();
        });
    }
    // Positive control: all upstream requests must be in flight together.
    tokio::time::timeout(Duration::from_secs(5), entered.wait())
        .await
        .unwrap();
    while let Some(result) = requests.join_next().await {
        result.unwrap();
    }
    assert_eq!(hits.load(Ordering::SeqCst), REQUESTS);
    assert_eq!(backup_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_cooldown_admits_one_probe_across_groups_others_use_backup() {
    let release = Arc::new(Notify::new());
    let hits = Arc::new(AtomicUsize::new(0));
    let upstream = Mock::start(Router::new().route(
        "/v1/chat/completions",
        post({
            let release = release.clone();
            let hits = hits.clone();
            move || {
                let release = release.clone();
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    release.notified().await;
                    ok()
                }
            }
        }),
    ))
    .await;
    let backup =
        Mock::start(Router::new().route("/v1/chat/completions", post(|| async { ok() }))).await;
    let first = AppState::for_tests().unwrap();
    let second = another_group(&first);
    expire(&first, &upstream.candidate("preferred"));
    let mut requests = tokio::task::JoinSet::new();
    for index in 0..32 {
        let state = if index % 2 == 0 {
            first.clone()
        } else {
            second.clone()
        };
        // Separately saved models with the same actual endpoint/key share the probe.
        let candidates = vec![
            upstream.candidate(if index % 2 == 0 { "preferred" } else { "alias" }),
            backup.candidate("backup"),
        ];
        requests.spawn(async move {
            let response = route_chat_with_candidates(
                &state,
                &HeaderMap::new(),
                json!({"messages":[]}),
                candidates,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let via = response.headers()["X-EchoBird-Routed-Via"].clone();
            response.into_body().collect().await.unwrap();
            via
        });
    }
    for _ in 0..31 {
        let via = tokio::time::timeout(Duration::from_secs(5), requests.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(via, "backup/concurrent-model");
    }
    release.notify_one();
    let via = requests.join_next().await.unwrap().unwrap();
    assert_ne!(via, "backup/concurrent-model");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(first.route_memory.lock().unwrap().candidates.is_empty());
}

#[tokio::test]
async fn streaming_probe_lives_until_body_end_or_client_cancellation() {
    let release = Arc::new(Notify::new());
    let hits = Arc::new(AtomicUsize::new(0));
    let upstream = Mock::start(Router::new().route("/v1/chat/completions", post({
        let release = release.clone(); let hits = hits.clone();
        move || {
            let release = release.clone(); let hits = hits.clone();
            async move {
                hits.fetch_add(1, Ordering::SeqCst);
                let chunks = stream::once(async { Ok::<_, Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n")) })
                    .chain(stream::once(async move {
                        release.notified().await;
                        Ok::<_, Infallible>(Bytes::from_static(b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
                    }));
                ([(header::CONTENT_TYPE, "text/event-stream")], Body::from_stream(chunks))
            }
        }
    }))).await;
    let state = AppState::for_tests().unwrap();
    let candidate = upstream.candidate("stream");
    expire(&state, &candidate);
    let headers = HeaderMap::new();
    let request = || {
        route_chat_with_candidates(
            &state,
            &headers,
            json!({"messages":[],"stream":true}),
            vec![candidate.clone()],
        )
    };
    let first = request().await;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(request().await.status(), StatusCode::SERVICE_UNAVAILABLE);
    drop(first);
    let second = request().await;
    assert_eq!(second.status(), StatusCode::OK);
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    release.notify_waiters();
    second.into_body().collect().await.unwrap();
    assert!(state.route_memory.lock().unwrap().candidates.is_empty());
}

#[test]
fn shared_account_failure_does_not_affect_other_accounts_or_erase_other_groups() {
    let first = AppState::for_tests().unwrap();
    let second = another_group(&first);
    let mut a = model("a");
    a.account_source = Some(crate::models::model::AccountSource {
        provider: AccountProvider::Kilo,
        account_id: "shared-account".into(),
    });
    let mut sibling = a.clone();
    sibling.internal_id = "sibling".into();
    sibling.model_id = "other-model".into();
    sibling.fingerprint = resource_key(&sibling);
    let mut independent = sibling.clone();
    independent.account_source.as_mut().unwrap().account_id = "independent".into();
    independent.fingerprint = resource_key(&independent);
    mark_failure(
        &first,
        &a,
        FailureClass::Authentication,
        Some(Duration::from_secs(60)),
    );
    assert!(prioritized_candidates(&second, vec![sibling.clone()]).is_empty());
    assert_eq!(prioritized_candidates(&second, vec![independent]).len(), 1);
    assert!(candidate_is_cooling_down(&first, &a));
    assert!(candidate_is_cooling_down(&second, &sibling));
    remember_response(&first, &a, Some("resp_group_one"));
    assert!(response_candidates(
        &second,
        &json!({"previous_response_id":"resp_group_one"}),
        vec![a.clone()],
        RouteProtocol::Responses
    )
    .is_err());
    assert!(response_candidates(
        &first,
        &json!({"previous_response_id":"resp_group_one"}),
        vec![a],
        RouteProtocol::Responses
    )
    .is_ok());
}

#[test]
fn production_factory_migrates_health_once_and_keeps_sessions_per_group() {
    let dir = std::env::temp_dir().join(format!("echobird-concurrency-{}", uuid::Uuid::new_v4()));
    super::super::profiles::with_fixture_root(dir.clone(), || {
        let first_id = format!("smart-router-{}", uuid::Uuid::new_v4());
        let second_id = format!("smart-router-{}", uuid::Uuid::new_v4());
        let config = dir.join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("smart-routers.json"),
            serde_json::to_vec(&json!({"enabled":true,"ids":[first_id,second_id]})).unwrap(),
        )
        .unwrap();
        let candidate = model("migrated");
        let legacy = AppState::for_tests().unwrap();
        mark_failure(&legacy, &candidate, FailureClass::Transient, None);
        let mut stored = serde_json::to_value(&*legacy.route_memory.lock().unwrap()).unwrap();
        stored["responseBindings"] = json!({"resp_old": {"candidateId":candidate.internal_id,"fingerprint":candidate.fingerprint,"expiresAtMs":now_ms()+60_000}});
        fs::write(
            config.join(format!("{first_id}-state.json")),
            serde_json::to_vec(&stored).unwrap(),
        )
        .unwrap();
        let first = super::super::profiles::with(&first_id, AppState::new).unwrap();
        let second = super::super::profiles::with(&second_id, AppState::new).unwrap();
        assert!(Arc::ptr_eq(&first.route_memory, &second.route_memory));
        assert!(!Arc::ptr_eq(
            &first.response_memory,
            &second.response_memory
        ));
        assert!(candidate_is_cooling_down(&second, &candidate));
        assert!(first
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .contains_key("resp_old"));
        assert!(second
            .response_memory
            .lock()
            .unwrap()
            .response_bindings
            .is_empty());
        let saved = load_route_memory(first.route_memory_path.as_ref().unwrap());
        assert_eq!(saved.candidates["migrated"].consecutive_failures, 1);
        assert!(!serde_json::to_string(&saved).unwrap().contains("resp_old"));

        let mut other = model("other-group-model");
        other.model_id = "different-model".into();
        other.fingerprint = resource_key(&other);
        mark_failure(&second, &other, FailureClass::Transient, None);
        fs::write(
            config.join(format!("{second_id}.json")),
            serde_json::to_vec(&json!({"candidateIds":[other.internal_id]})).unwrap(),
        )
        .unwrap();
        let mut removed_in_flight = model("removed-in-flight");
        removed_in_flight.api_key = "different-key".into();
        removed_in_flight.fingerprint = resource_key(&removed_in_flight);
        assert!(try_begin_attempt(&first, &mut removed_in_flight));
        {
            // set_candidate_ids already owns this lock: peer reads must be
            // read-only and must not re-enter the configuration lifecycle.
            let _guard = super::super::config_lock().lock().unwrap();
            super::super::profiles::with(&first_id, || retain_candidate_memory(&[]));
        }
        assert!(candidate_is_cooling_down(&second, &other));
        mark_failure(&first, &removed_in_flight, FailureClass::Transient, None);
        assert!(!first
            .route_memory
            .lock()
            .unwrap()
            .candidates
            .contains_key("removed-in-flight"));
        assert!(!config.join(format!("{first_id}.json")).exists());
    });
    fs::remove_dir_all(dir).unwrap();
}
