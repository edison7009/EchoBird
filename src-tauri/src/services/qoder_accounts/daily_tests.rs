use super::*;
use axum::{
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

struct Fixture {
    root: PathBuf,
    path: PathBuf,
    credentials: Credentials,
    id: String,
}

impl Fixture {
    fn new(edition: Edition) -> Self {
        let nonce = uuid::Uuid::new_v4().to_string();
        let credentials = Credentials {
            edition,
            access_token: format!("job-{nonce}"),
            refresh_token: format!("job-refresh-{nonce}"),
            expires_at: i64::MAX,
            user_id: nonce.clone(),
            name: "Fixture".into(),
            email: "fixture@example.test".into(),
            machine_id: nonce.clone(),
            device_token: format!("device-{nonce}"),
            device_refresh: format!("device-refresh-{nonce}"),
            device_expires_at: i64::MAX,
            device_chat: false,
            native_device_only: false,
        };
        let mut account = summary(&credentials);
        account.active = true;
        account.remaining = Some(7.0);
        account.total = Some(10.0);
        account.quota_windows = vec![QuotaWindow {
            name: "Credits".into(),
            total: Some(10.0),
            remaining: Some(7.0),
            expires_at: None,
        }];
        let id = account.id.clone();
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("qoder-daily-{nonce}"));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("accounts.json");
        super::super::cursor_auth::write(
            &path,
            &Store::from([(
                id.clone(),
                Saved {
                    account,
                    credential: protect(&credentials).unwrap(),
                },
            )]),
        )
        .unwrap();
        Self {
            root,
            path,
            credentials,
            id,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn daily(claimed: bool) -> Value {
    let now = chrono::Utc::now().timestamp();
    let reset = campaigns::next_reset(now);
    // Keep the positive fixture valid during the final minute of a reset window.
    let end = if reset - now > 60 { reset - 60 } else { reset };
    json!({"campaigns":[{
        "campaignId":"daily-current", "actionType":"CLAIM_BENEFIT",
        "claimStatus":if claimed { "CLAIMED" } else { "CLAIMABLE" },
        "startAt":reset - 86_400,"endAt":end,
        "benefit":{"kind":"CREDITS","amount":100,
            "modelScope":{"modelSeries":{"key":"ALL_MODELS"}},
            "validity":{"mode":"RELATIVE_DAYS","days":30}},
        "placements":[{"content":{"zh":{"detailUrl":"https://docs.qoder.cn/events/100credits"}}}]
    }]})
}

async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, server)
}

#[test]
fn loading_legacy_rewards_invalidates_only_unverified_public_cache_without_writing() {
    for edition in [Edition::Qoder, Edition::QoderCn] {
        let fixture = Fixture::new(edition);
        for verified in [None, Some(false), Some(true)] {
            let now = chrono::Utc::now().timestamp();
            let reset = campaigns::next_reset(now);
            let mut account = serde_json::to_value(summary(&fixture.credentials)).unwrap();
            account["dailyReward"] = json!({
                "status":"claimed", "checkedAt":now, "resetAt":reset
            });
            if let Some(verified) = verified {
                account["dailyReward"]["verified"] = json!(verified);
            }
            super::super::cursor_auth::write(
                &fixture.path,
                &json!({(fixture.id.clone()):{
                    "account":account,"credential":"opaque-without-decryption"
                }}),
            )
            .unwrap();
            let original = std::fs::read(&fixture.path).unwrap();
            let loaded = load_at(&fixture.path).unwrap();
            let reward = &loaded[&fixture.id].account.daily_reward;
            if verified == Some(true) {
                let reward = reward.as_ref().unwrap();
                assert!(reward.verified);
                assert_eq!(reward.status, RewardStatus::Claimed);
                assert_eq!(reward.reset_at, reset);
            } else {
                assert!(reward.is_none());
            }
            assert_eq!(loaded[&fixture.id].credential, "opaque-without-decryption");
            assert_eq!(std::fs::read(&fixture.path).unwrap(), original);
        }
    }
}

#[tokio::test]
async fn both_regions_use_selected_account_identity_and_only_post_new_claims_once() {
    for edition in [Edition::Qoder, Edition::QoderCn] {
        let fixture = Fixture::new(edition);
        let claimed = Arc::new(AtomicBool::new(false));
        let posts = Arc::new(AtomicUsize::new(0));
        let quota_calls = Arc::new(AtomicUsize::new(0));
        let get_claimed = claimed.clone();
        let post_claimed = claimed.clone();
        let post_count = posts.clone();
        let quota_count = quota_calls.clone();
        let bearer = format!("Bearer {}", fixture.credentials.device_token);
        let post_bearer = bearer.clone();
        let machine = fixture.credentials.machine_id.clone();
        let post_machine = machine.clone();
        let app = Router::new()
            .route(
                "/sash/api/v1/me/campaigns",
                get(move |headers: HeaderMap| {
                    let claimed = get_claimed.clone();
                    let bearer = bearer.clone();
                    let machine = machine.clone();
                    async move {
                        assert_eq!(headers["authorization"].to_str().unwrap(), bearer);
                        assert_eq!(headers["cosy-machinetoken"], "fixture-sdk-token");
                        assert_eq!(headers["cosy-machinetype"], "fixture-sdk-type");
                        assert_eq!(headers["cosy-machinecode"], "fixture-sdk-code");
                        assert_eq!(headers["cosy-machineid"].to_str().unwrap(), machine);
                        assert_eq!(headers["cosy-clienttype"], "10");
                        let mut campaigns = daily(claimed.load(Ordering::SeqCst));
                        if edition == Edition::Qoder {
                            campaigns["campaigns"][0]["placements"][0]["content"]["zh"]
                                ["detailUrl"] = json!("https://docs.qoder.com/events/100credits");
                        }
                        Json(campaigns)
                    }
                }),
            )
            .route(
                "/sash/api/v1/me/campaigns/daily-current/claim",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let claimed = post_claimed.clone();
                    let count = post_count.clone();
                    let bearer = post_bearer.clone();
                    let machine = post_machine.clone();
                    async move {
                        assert_eq!(headers["authorization"].to_str().unwrap(), bearer);
                        assert_eq!(headers["cosy-machinetoken"], "fixture-sdk-token");
                        assert_eq!(headers["cosy-machineid"].to_str().unwrap(), machine);
                        assert_eq!(body, json!({}));
                        count.fetch_add(1, Ordering::SeqCst);
                        claimed.store(true, Ordering::SeqCst);
                        Json(json!({"data":{"status":"CLAIMED"}}))
                    }
                }),
            )
            .route(
                "/sash/api/v2/me/usage",
                get(move || {
                    let count = quota_count.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                }),
            );
        let (base, server) = serve(app).await;
        let identity = campaigns::fixture_identity(&fixture.credentials.machine_id);
        for _ in 0..2 {
            let account =
                claim_daily_at(edition, &fixture.id, &fixture.path, &base, Some(&identity))
                    .await
                    .unwrap();
            let reward = account.daily_reward.unwrap();
            assert!(reward.verified);
            assert_eq!(reward.status, RewardStatus::Claimed);
            assert_eq!(account.remaining, Some(7.0));
        }
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert_eq!(quota_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            saved_credentials_at(&fixture.path, edition, &fixture.id)
                .await
                .unwrap()
                .device_token,
            fixture.credentials.device_token
        );
        server.abort();
    }
}

#[tokio::test]
async fn simultaneous_claims_preserve_the_receipt_when_quota_refresh_fails() {
    let fixture = Fixture::new(Edition::QoderCn);
    let claimed = Arc::new(AtomicBool::new(false));
    let posts = Arc::new(AtomicUsize::new(0));
    let quotas = Arc::new(AtomicUsize::new(0));
    let get_claimed = claimed.clone();
    let post_claimed = claimed.clone();
    let post_count = posts.clone();
    let quota_count = quotas.clone();
    let app = Router::new()
        .route(
            "/sash/api/v1/me/campaigns",
            get(move || {
                let claimed = get_claimed.clone();
                async move { Json(daily(claimed.load(Ordering::SeqCst))) }
            }),
        )
        .route(
            "/sash/api/v1/me/campaigns/daily-current/claim",
            post(move || {
                let claimed = post_claimed.clone();
                let count = post_count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    claimed.store(true, Ordering::SeqCst);
                    Json(json!({"status":"CLAIMED"}))
                }
            }),
        )
        .route(
            "/sash/api/v2/me/usage",
            get(move || {
                let count = quota_count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }),
        );
    let (base, server) = serve(app).await;
    let identity = campaigns::fixture_identity(&fixture.credentials.machine_id);
    let claim = || {
        claim_daily_at(
            Edition::QoderCn,
            &fixture.id,
            &fixture.path,
            &base,
            Some(&identity),
        )
    };
    let (first, second) = tokio::join!(claim(), claim());
    for account in [first.unwrap(), second.unwrap()] {
        assert_eq!(account.daily_reward.unwrap().status, RewardStatus::Claimed);
        assert_eq!(account.remaining, Some(7.0));
        assert_eq!(account.total, Some(10.0));
        assert!(account.active);
    }
    let stored = load_at(&fixture.path).unwrap();
    assert_eq!(
        stored[&fixture.id]
            .account
            .daily_reward
            .as_ref()
            .unwrap()
            .status,
        RewardStatus::Claimed
    );
    assert_eq!(
        stored[&fixture.id].account.quota_windows[0].remaining,
        Some(7.0)
    );
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    assert_eq!(quotas.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test]
async fn unconfirmed_receipts_are_not_replayed_and_the_next_click_rechecks_status() {
    for (status, body) in [
        (StatusCode::OK, json!({"errorCode":"GRANT_RESULT_UNKNOWN"})),
        (StatusCode::OK, json!({"code":0})),
        (StatusCode::INTERNAL_SERVER_ERROR, json!({})),
    ] {
        let fixture = Fixture::new(Edition::QoderCn);
        let posts = Arc::new(AtomicUsize::new(0));
        let get_count = posts.clone();
        let post_count = posts.clone();
        let app = Router::new()
            .route(
                "/sash/api/v1/me/campaigns",
                get(move || {
                    let count = get_count.clone();
                    async move { Json(daily(count.load(Ordering::SeqCst) > 0)) }
                }),
            )
            .route(
                "/sash/api/v1/me/campaigns/daily-current/claim",
                post(move || {
                    let count = post_count.clone();
                    let body = body.clone();
                    async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        (status, Json(body))
                    }
                }),
            );
        let (base, server) = serve(app).await;
        assert!(claim_daily_at(
            Edition::QoderCn,
            &fixture.id,
            &fixture.path,
            &base,
            Some(&campaigns::fixture_identity(
                &fixture.credentials.machine_id
            ))
        )
        .await
        .is_err());
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        let account = claim_daily_at(
            Edition::QoderCn,
            &fixture.id,
            &fixture.path,
            &base,
            Some(&campaigns::fixture_identity(
                &fixture.credentials.machine_id,
            )),
        )
        .await
        .unwrap();
        assert_eq!(account.daily_reward.unwrap().status, RewardStatus::Claimed);
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert_eq!(
            load_at(&fixture.path).unwrap()[&fixture.id]
                .account
                .remaining,
            Some(7.0)
        );
        server.abort();
    }
}

#[tokio::test]
async fn device_rotation_is_saved_even_when_the_following_campaign_or_post_fails() {
    for fail_campaign in [true, false] {
        let fixture = Fixture::new(Edition::QoderCn);
        let gets = Arc::new(AtomicUsize::new(0));
        let refreshes = Arc::new(AtomicUsize::new(0));
        let posts = Arc::new(AtomicUsize::new(0));
        let get_count = gets.clone();
        let refresh_count = refreshes.clone();
        let post_count = posts.clone();
        let old_token = fixture.credentials.device_token.clone();
        let old_refresh = fixture.credentials.device_refresh.clone();
        let app = Router::new()
            .route("/sash/api/v1/me/campaigns", get(move |headers: HeaderMap| {
                let count = get_count.clone();
                let old_token = old_token.clone();
                async move {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        assert_eq!(headers["authorization"].to_str().unwrap(), format!("Bearer {old_token}"));
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    assert_eq!(headers["authorization"], "Bearer new-device");
                    if fail_campaign {
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    } else {
                        Json(daily(false)).into_response()
                    }
                }
            }))
            .route("/api/v1/deviceToken/refresh", post(move |Json(body): Json<Value>| {
                let count = refresh_count.clone();
                let old_refresh = old_refresh.clone();
                async move {
                    assert_eq!(body["refresh_token"], old_refresh);
                    count.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"token":"new-device","refresh_token":"new-device-refresh","expires_in":3600}))
                }
            }))
            .route("/sash/api/v1/me/campaigns/daily-current/claim", post(move || {
                let count = post_count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }));
        let (base, server) = serve(app).await;
        assert!(claim_daily_at(
            Edition::QoderCn,
            &fixture.id,
            &fixture.path,
            &base,
            Some(&campaigns::fixture_identity(
                &fixture.credentials.machine_id
            ))
        )
        .await
        .is_err());
        let stored = saved_credentials_at(&fixture.path, Edition::QoderCn, &fixture.id)
            .await
            .unwrap();
        assert_eq!(stored.device_token, "new-device");
        assert_eq!(stored.device_refresh, "new-device-refresh");
        assert_eq!(stored.access_token, fixture.credentials.access_token);
        assert_eq!(stored.refresh_token, fixture.credentials.refresh_token);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(gets.load(Ordering::SeqCst), 2);
        assert_eq!(
            posts.load(Ordering::SeqCst),
            if fail_campaign { 0 } else { 1 }
        );
        server.abort();
    }
}

#[tokio::test]
async fn stale_results_do_not_replace_relogin_credentials_active_state_or_deleted_accounts() {
    let fixture = Fixture::new(Edition::QoderCn);
    let mut newer = fixture.credentials.clone();
    newer.device_token = "relogin-device".into();
    {
        let _guard = ACCOUNT_LOCK.lock().await;
        let mut store = load_at(&fixture.path).unwrap();
        let saved = store.get_mut(&fixture.id).unwrap();
        saved.credential = protect(&newer).unwrap();
        saved.account.active = false;
        saved.account.name = "New identity".into();
        super::super::cursor_auth::write(&fixture.path, &store).unwrap();
    }
    let now = chrono::Utc::now().timestamp();
    let reward = DailyReward {
        status: RewardStatus::Claimed,
        checked_at: now,
        reset_at: campaigns::next_reset(now),
        verified: true,
    };
    let original = std::fs::read(&fixture.path).unwrap();
    assert!(update_account_at(
        &fixture.path,
        &fixture.id,
        &fixture.credentials,
        None,
        Some(reward.clone())
    )
    .await
    .is_err());
    assert!(commit_rotation_at(
        &fixture.path,
        &fixture.id,
        &fixture.credentials,
        &fixture.credentials
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(&fixture.path).unwrap(), original);
    let account = update_account_at(
        &fixture.path,
        &fixture.id,
        &newer,
        None,
        Some(reward.clone()),
    )
    .await
    .unwrap();
    assert!(!account.active);
    assert_eq!(account.name, "New identity");
    assert_eq!(account.remaining, Some(7.0));
    assert_eq!(
        saved_credentials_at(&fixture.path, Edition::QoderCn, &fixture.id)
            .await
            .unwrap()
            .device_token,
        "relogin-device"
    );
    {
        let _guard = ACCOUNT_LOCK.lock().await;
        super::super::cursor_auth::write(&fixture.path, &Store::new()).unwrap();
    }
    assert!(
        update_account_at(&fixture.path, &fixture.id, &newer, None, Some(reward))
            .await
            .is_err()
    );
    assert!(
        commit_rotation_at(&fixture.path, &fixture.id, &newer, &newer)
            .await
            .is_err()
    );
    assert!(load_at(&fixture.path).unwrap().is_empty());
}

#[tokio::test]
async fn same_account_writers_wait_without_blocking_other_accounts_or_regions() {
    for delete in [true, false] {
        let fixture = Fixture::new(Edition::QoderCn);
        let lock = request_lock(Edition::QoderCn, &fixture.id).unwrap();
        let pending = lock.lock().await;
        let actor_path = fixture.path.clone();
        let actor_id = fixture.id.clone();
        let mut relogin = fixture.credentials.clone();
        relogin.device_token = "writer-device".into();
        let (started, queued) = tokio::sync::oneshot::channel();
        let actor = tokio::spawn(async move {
            let lock = request_lock(Edition::QoderCn, &actor_id).unwrap();
            started.send(()).unwrap();
            let _request = lock.lock().await;
            let _guard = ACCOUNT_LOCK.lock().await;
            let mut store = load_at(&actor_path).unwrap();
            if delete {
                store.remove(&actor_id).unwrap();
            } else {
                store.get_mut(&actor_id).unwrap().credential = protect(&relogin).unwrap();
            }
            super::super::cursor_auth::write(&actor_path, &store).unwrap();
        });
        queued.await.unwrap();
        assert!(!actor.is_finished());
        for edition in [Edition::QoderCn, Edition::Qoder] {
            let ready = Fixture::new(edition);
            tokio::time::timeout(std::time::Duration::from_millis(250), async {
                let lock = request_lock(edition, &ready.id).unwrap();
                let _request = lock.lock().await;
                let _guard = ACCOUNT_LOCK.lock().await;
                let mut store = load_at(&ready.path).unwrap();
                store.get_mut(&ready.id).unwrap().account.active = false;
                super::super::cursor_auth::write(&ready.path, &store).unwrap();
            })
            .await
            .unwrap();
            assert!(!load_at(&ready.path).unwrap()[&ready.id].account.active);
        }
        assert!(!actor.is_finished());
        drop(pending);
        tokio::time::timeout(std::time::Duration::from_secs(2), actor)
            .await
            .unwrap()
            .unwrap();
        let store = load_at(&fixture.path).unwrap();
        if delete {
            assert!(store.is_empty());
        } else {
            assert_eq!(
                selected(&store, Edition::QoderCn, &fixture.id)
                    .unwrap()
                    .device_token,
                "writer-device"
            );
            assert!(store[&fixture.id].account.active);
        }
    }
}

#[tokio::test]
async fn native_session_holds_selected_credentials_until_the_native_transaction_commits() {
    for edition in [Edition::Qoder, Edition::QoderCn] {
        let mut fixture = Fixture::new(edition);
        if edition == Edition::QoderCn {
            fixture.credentials.device_chat = true;
            fixture.credentials.access_token = fixture.credentials.device_token.clone();
            fixture.credentials.refresh_token = fixture.credentials.device_refresh.clone();
            let mut store = load_at(&fixture.path).unwrap();
            store.get_mut(&fixture.id).unwrap().credential = protect(&fixture.credentials).unwrap();
            super::super::cursor_auth::write(&fixture.path, &store).unwrap();
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let recorded = calls.clone();
        let expected = fixture.credentials.refresh_token.clone();
        let endpoint = if fixture.credentials.device_chat {
            "/api/v1/deviceToken/refresh"
        } else {
            "/api/v1/jobToken/refresh"
        };
        let app = Router::new().route(endpoint, post(move |Json(body): Json<Value>| {
            let recorded = recorded.clone();
            let expected = expected.clone();
            async move {
                recorded.fetch_add(1, Ordering::SeqCst);
                assert_eq!(body["refresh_token"], expected);
                Json(json!({"token":"new-native-peer-token","refresh_token":"new-native-peer-refresh",
                    "expires_in":if edition==Edition::QoderCn {3600}else{3_600_000}}))
            }
        }));
        let (base, server) = serve(app).await;
        let session = native_session_at(&fixture.path, edition, &fixture.id, &base)
            .await
            .unwrap();
        let mut refresh = Box::pin(proxy_credentials_at(
            &fixture.path,
            edition,
            &fixture.id,
            Some(&fixture.credentials.access_token),
            &base,
        ));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut refresh)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let native_path = fixture.path.with_extension("native.json");
        let mut transaction = super::super::native_account_files::Transaction::new();
        transaction
            .replace(
                &native_path,
                &serde_json::to_vec(&session.credentials).unwrap(),
            )
            .unwrap();
        assert!(
            selected(&load_at(&fixture.path).unwrap(), edition, &fixture.id).unwrap()
                == session.credentials
        );
        transaction.commit();
        drop(session);
        let renewed = refresh.await.unwrap();
        assert_eq!(renewed["accessToken"], "new-native-peer-token");
        let native: Credentials =
            serde_json::from_slice(&std::fs::read(&native_path).unwrap()).unwrap();
        assert!(native == fixture.credentials);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_file(native_path).unwrap();
        server.abort();
    }
}
