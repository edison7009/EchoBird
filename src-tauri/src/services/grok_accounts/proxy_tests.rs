use super::*;
use crate::services::minimax_accounts::test_support::{json as response_json, MockServer};
use serde_json::json;
use std::{
    collections::BTreeSet,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const SCOPE: &str = "https://auth.x.ai::fixture-client";

pub(super) fn failed_writes() -> &'static Mutex<BTreeSet<PathBuf>> {
    static FAILED: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    FAILED.get_or_init(|| Mutex::new(BTreeSet::new()))
}
struct FailedWrite(PathBuf);
impl FailedWrite {
    fn new(path: &Path) -> Self {
        failed_writes().lock().unwrap().insert(path.into());
        Self(path.into())
    }
}
impl Drop for FailedWrite {
    fn drop(&mut self) {
        failed_writes().lock().unwrap().remove(&self.0);
    }
}

struct Fixture {
    root: PathBuf,
    store: PathBuf,
    native: PathBuf,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("echobird-grok-proxy-{}", uuid::Uuid::new_v4()));
        let store = root.join("accounts");
        let native = root.join("native/auth.json");
        let key = SCOPE;
        let account = save_at(&store, key, &json!({key:{
            "key":"old-access", "refresh_token":"old-refresh", "user_id":"fixture-user",
            "email":"fixture@example.test", "oidc_issuer":"https://auth.x.ai", "oidc_client_id":"fixture-client",
            "principal_type":"personal", "principal_id":"fixture-user",
            "create_time":"2025-01-01T00:00:00Z", "expires_at":"2099-01-01T00:00:00Z"
        }})).unwrap();
        let path = store.join(format!("{}.json", account.id));
        Self {
            root,
            store,
            native,
            path,
        }
    }
    fn saved(&self) -> Saved {
        crate::services::cursor_auth::read(&self.path).unwrap()
    }
    fn apply(&self) {
        storage::apply_at(
            &self.store,
            &self.native,
            &self.native.parent().unwrap().join("settings_cache.json"),
            &self.saved(),
        )
        .unwrap();
    }
    fn native_auth(&self) -> Value {
        crate::services::cursor_auth::read(&self.native).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        forget_pending(&self.path);
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn encrypted_snapshots_list_only_metadata_without_native_reads_or_decryption() {
    let fixture = Fixture::new();
    let original = fs::read(&fixture.path).unwrap();
    let serialized: Value = serde_json::from_slice(&original).unwrap();
    assert!(serialized["auth"].as_str().unwrap().starts_with("enc:v1:"));
    assert!(!String::from_utf8_lossy(&original).contains("old-refresh"));
    assert_eq!(fixture.saved().auth["key"], "old-access");
    let mut unreadable = serialized;
    unreadable["auth"] = json!("enc:v1:invalid");
    fs::write(&fixture.path, unreadable.to_string()).unwrap();
    fs::create_dir_all(&fixture.native).unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert_eq!(list_at(&fixture.store, &fixture.native).unwrap().len(), 1);
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
    assert!(current(&fixture.path).is_err());
    assert!(!fixture.native.with_file_name("auth.json.lock").exists());
}

#[tokio::test]
async fn legacy_active_snapshots_adopt_native_rotation_without_passive_migration() {
    for encrypted in [false, true] {
        let fixture = Fixture::new();
        let mut saved = fixture.saved();
        saved.summary.active = true;
        let mut legacy = serde_json::to_value(&saved).unwrap();
        assert!(legacy.get("native_pair").is_none());
        if !encrypted {
            legacy["auth"] = saved.auth.clone();
        }
        fs::write(&fixture.path, legacy.to_string()).unwrap();
        let mut native = json!({saved.key.clone():saved.auth});
        native[SCOPE]["key"] = json!("native-new-access");
        native[SCOPE]["refresh_token"] = json!("native-new-refresh");
        native[SCOPE]["expires_at"] = json!("2026-01-01T00:00:00Z");
        crate::services::cursor_auth::write(&fixture.native, &native).unwrap();
        let own_before = fs::read(&fixture.path).unwrap();
        let native_before = fs::read(&fixture.native).unwrap();
        let rows = list_at(&fixture.store, &fixture.native).unwrap();
        assert!(rows[0].active);
        assert_eq!(fs::read(&fixture.path).unwrap(), own_before);
        assert_eq!(fs::read(&fixture.native).unwrap(), native_before);
        assert!(!fixture.native.with_file_name("auth.json.lock").exists());
        let session = Session::open(&fixture.path, Some(&fixture.native))
            .await
            .unwrap();
        assert_eq!(session.saved.auth["key"], "native-new-access");
        assert_eq!(session.saved.auth["refresh_token"], "native-new-refresh");
        assert_eq!(
            session.saved.native_pair,
            Some(storage::pair(&native[SCOPE]))
        );
        assert!(session.saved.summary.active);
        assert_eq!(fixture.saved().auth, native[SCOPE]);
        assert_eq!(fs::read(&fixture.native).unwrap(), native_before);
        let rewritten: Value = serde_json::from_slice(&fs::read(&fixture.path).unwrap()).unwrap();
        assert!(rewritten["auth"].as_str().unwrap().starts_with("enc:v1:"));
    }
}

#[tokio::test]
async fn native_rotation_is_authoritative_even_when_its_expiry_is_shorter() {
    let fixture = Fixture::new();
    fixture.apply();
    let mut native = fixture.native_auth();
    let key = SCOPE;
    native[key]["key"] = json!("native-new-access");
    native[key]["refresh_token"] = json!("native-new-refresh");
    native[key]["expires_at"] = json!("2026-01-01T00:00:00Z");
    crate::services::cursor_auth::write(&fixture.native, &native).unwrap();
    let session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    assert_eq!(session.saved.auth["key"], "native-new-access");
    assert_eq!(fixture.saved().auth["refresh_token"], "native-new-refresh");
}

#[test]
fn native_apply_keeps_a_rotated_pair_when_reapplying_the_same_account() {
    for mode in ["bound", "legacy", "same_pair_metadata"] {
        let fixture = Fixture::new();
        fixture.apply();
        if mode == "legacy" {
            let mut saved = fixture.saved();
            saved.native_pair = None;
            crate::services::cursor_auth::write(&fixture.path, &saved).unwrap();
        }
        let mut native = fixture.native_auth();
        if mode != "same_pair_metadata" {
            native[SCOPE]["key"] = json!("native-new-access");
            native[SCOPE]["refresh_token"] = json!("native-new-refresh");
        }
        native[SCOPE]["expires_at"] = json!("2026-10-14T01:00:00Z");
        crate::services::cursor_auth::write(&fixture.native, &native).unwrap();

        fixture.apply();

        assert_eq!(fixture.native_auth(), native);
        assert_eq!(fixture.saved().auth, native[SCOPE]);
        assert_eq!(
            fixture.saved().native_pair,
            Some(storage::pair(&native[SCOPE]))
        );
    }
}

#[test]
fn native_apply_saves_the_outgoing_accounts_rotation_before_switching_back() {
    let fixture = Fixture::new();
    let mut other = fixture.saved().auth;
    other["user_id"] = json!("other-user");
    other["principal_id"] = json!("other-user");
    other["key"] = json!("other-access");
    other["refresh_token"] = json!("other-refresh");
    let account = save_at(&fixture.store, SCOPE, &json!({SCOPE:other})).unwrap();
    let other_path = fixture.store.join(format!("{}.json", account.id));
    let other: Saved = crate::services::cursor_auth::read(&other_path).unwrap();
    fixture.apply();
    let mut native = fixture.native_auth();
    native[SCOPE]["key"] = json!("native-new-access");
    native[SCOPE]["refresh_token"] = json!("native-new-refresh");
    crate::services::cursor_auth::write(&fixture.native, &native).unwrap();

    storage::apply_at(
        &fixture.store,
        &fixture.native,
        &fixture.native.parent().unwrap().join("settings_cache.json"),
        &other,
    )
    .unwrap();

    assert_eq!(fixture.native_auth()[SCOPE], other.auth);
    assert_eq!(fixture.saved().auth, native[SCOPE]);
    assert!(!fixture.saved().summary.active);
    fixture.apply();
    assert_eq!(fixture.native_auth(), native);
}

#[test]
fn native_apply_keeps_an_unbound_grant_separate_from_the_same_users_native_login() {
    let fixture = Fixture::new();
    let own = fixture.saved();
    let mut native = json!({SCOPE:own.auth});
    native[SCOPE]["key"] = json!("independent-access");
    native[SCOPE]["refresh_token"] = json!("independent-refresh");
    crate::services::cursor_auth::write(&fixture.native, &native).unwrap();

    fixture.apply();

    assert_eq!(fixture.native_auth()[SCOPE], own.auth);
    assert_eq!(fixture.saved().auth, own.auth);
}

#[test]
fn native_apply_preserves_usable_accounts_when_the_native_key_is_invalid() {
    for key in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(" \t ")),
    ] {
        let fixture = Fixture::new();
        let mut other = fixture.saved().auth;
        other["user_id"] = json!("other-user");
        other["principal_id"] = json!("other-user");
        other["key"] = json!("other-access");
        other["refresh_token"] = json!("other-refresh");
        let account = save_at(&fixture.store, SCOPE, &json!({SCOPE:other})).unwrap();
        let other_path = fixture.store.join(format!("{}.json", account.id));
        let other: Saved = crate::services::cursor_auth::read(&other_path).unwrap();
        fixture.apply();
        let own = fixture.saved();
        let mut native = fixture.native_auth();
        if let Some(key) = key {
            native[SCOPE]["key"] = key;
        } else {
            native[SCOPE].as_object_mut().unwrap().remove("key");
        }
        crate::services::cursor_auth::write(&fixture.native, &native).unwrap();

        storage::apply_at(
            &fixture.store,
            &fixture.native,
            &fixture.native.parent().unwrap().join("settings_cache.json"),
            &other,
        )
        .unwrap();

        assert_eq!(fixture.native_auth()[SCOPE], other.auth);
        assert_eq!(fixture.saved().auth, own.auth);
        assert!(fixture.saved().native_pair.is_none());
        fixture.apply();
        assert_eq!(fixture.native_auth()[SCOPE], own.auth);
    }
}

#[tokio::test]
async fn invalid_native_keys_preserve_own_credentials_and_release_the_binding() {
    for key in [
        None,
        Some(Value::Null),
        Some(json!("")),
        Some(json!(" \t ")),
    ] {
        let fixture = Fixture::new();
        fixture.apply();
        let own = fixture.saved();
        let mut native = fixture.native_auth();
        if let Some(key) = key {
            native[SCOPE]["key"] = key;
        } else {
            native[SCOPE].as_object_mut().unwrap().remove("key");
        }
        crate::services::cursor_auth::write(&fixture.native, &native).unwrap();
        let native_before = fs::read(&fixture.native).unwrap();

        let session = Session::open(&fixture.path, Some(&fixture.native))
            .await
            .unwrap();

        assert_eq!(session.saved.auth, own.auth);
        assert!(session.saved.native_pair.is_none());
        assert!(!session.saved.summary.active);
        assert_eq!(fixture.saved().auth, own.auth);
        assert_eq!(fs::read(&fixture.native).unwrap(), native_before);
        let external = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.native.with_file_name("auth.json.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&external).unwrap();
        fs2::FileExt::unlock(&external).unwrap();
    }
}

#[test]
fn native_apply_flushes_pending_newer_credentials_before_switching_away() {
    let fixture = Fixture::new();
    let mut other = fixture.saved().auth;
    other["user_id"] = json!("other-user");
    other["principal_id"] = json!("other-user");
    other["key"] = json!("other-access");
    other["refresh_token"] = json!("other-refresh");
    let account = save_at(&fixture.store, SCOPE, &json!({SCOPE:other})).unwrap();
    let other_path = fixture.store.join(format!("{}.json", account.id));
    let other: Saved = crate::services::cursor_auth::read(&other_path).unwrap();
    fixture.apply();
    let mut previous = fixture.saved();
    previous.summary.plan = Some("SuperGrok".into());
    crate::services::cursor_auth::write(&fixture.path, &previous).unwrap();
    let mut renewed = previous.clone();
    renewed.auth["key"] = json!("own-new-access");
    renewed.auth["refresh_token"] = json!("own-new-refresh");
    renewed.auth["create_time"] = json!("2026-10-14T00:00:00Z");
    renewed.auth["expires_at"] = json!("2026-10-14T01:00:00Z");
    let failed = FailedWrite::new(&fixture.path);
    persist(&fixture.path, &previous, &renewed).unwrap();
    drop(failed);

    storage::apply_at(
        &fixture.store,
        &fixture.native,
        &fixture.native.parent().unwrap().join("settings_cache.json"),
        &other,
    )
    .unwrap();

    assert_eq!(fixture.saved().auth, renewed.auth);
    assert!(!fixture.saved().summary.active);
    assert_eq!(fixture.saved().summary.plan, previous.summary.plan);
    assert_eq!(fixture.saved().native_pair, previous.native_pair);
    assert!(!pending().lock().unwrap().contains_key(&fixture.path));
    fixture.apply();
    assert_eq!(fixture.native_auth()[SCOPE], renewed.auth);
}

#[cfg(windows)]
#[test]
fn native_apply_rollback_keeps_pending_credentials_until_the_transaction_commits() {
    use std::os::windows::fs::OpenOptionsExt;

    let fixture = Fixture::new();
    fixture.apply();
    let previous = fixture.saved();
    let own_before = fs::read(&fixture.path).unwrap();
    let native_before = fs::read(&fixture.native).unwrap();
    let mut renewed = previous.clone();
    renewed.auth["key"] = json!("own-new-access");
    renewed.auth["refresh_token"] = json!("own-new-refresh");
    let failed = FailedWrite::new(&fixture.path);
    persist(&fixture.path, &previous, &renewed).unwrap();
    drop(failed);
    let cache = fixture.native.parent().unwrap().join("settings_cache.json");
    fs::write(&cache, "cached-plan").unwrap();
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&cache)
        .unwrap();
    assert_eq!(fs::read_to_string(&cache).unwrap(), "cached-plan");
    assert!(fs::remove_file(&cache).is_err());

    assert!(storage::apply_at(
        &fixture.store,
        &fixture.native,
        &cache,
        &current(&fixture.path).unwrap(),
    )
    .is_err());

    assert_eq!(fs::read(&fixture.path).unwrap(), own_before);
    assert_eq!(fs::read(&fixture.native).unwrap(), native_before);
    assert_eq!(current(&fixture.path).unwrap().auth, renewed.auth);
    assert!(pending().lock().unwrap().contains_key(&fixture.path));
    drop(lock);
    fixture.apply();
    assert_eq!(fixture.saved().auth, renewed.auth);
    assert_eq!(fixture.native_auth()[SCOPE], renewed.auth);
    assert!(!pending().lock().unwrap().contains_key(&fixture.path));
}

#[tokio::test]
async fn isolated_login_does_not_adopt_another_grant_or_overwrite_other_principals() {
    let fixture = Fixture::new();
    let own = fixture.saved();
    let mut native =
        json!({own.key.clone():own.auth, "https://another-issuer.test":{"key":"untouched"}});
    native[&own.key]["key"] = json!("independent-access");
    crate::services::cursor_auth::write(&fixture.native, &native).unwrap();
    let session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    assert_eq!(session.saved.auth["key"], "old-access");
    assert!(session.saved.native_pair.is_none());
    drop(session);
    fixture.apply();
    let mut native = fixture.native_auth();
    native[&own.key]["principal_id"] = json!("another-team");
    native[&own.key]["key"] = json!("other-principal");
    crate::services::cursor_auth::write(&fixture.native, &native).unwrap();
    let mut session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    assert!(session.saved.native_pair.is_none());
    assert!(!session.saved.summary.active);
    let consumed = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let captured = consumed.clone();
    let released = release.clone();
    let server = MockServer::start(move |_| {
        let captured = captured.clone();
        let released = released.clone();
        async move {
            captured.notify_one();
            released.notified().await;
            response_json(
                200,
                json!({"access_token":"own-renewed", "refresh_token":"own-refresh", "expires_in":3600}),
            )
        }
    })
    .await;
    let probe = async {
        tokio::time::timeout(Duration::from_secs(2), consumed.notified())
            .await
            .unwrap();
        let external = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.native.with_file_name("auth.json.lock"))
            .unwrap();
        fs2::FileExt::try_lock_exclusive(&external).unwrap();
        fs2::FileExt::unlock(&external).unwrap();
        release.notify_one();
    };
    let (renewed, ()) = tokio::join!(session.renew(Some("old-access"), &server.url), probe);
    renewed.unwrap();
    assert_eq!(session.saved.auth["key"], "own-renewed");
    assert_eq!(fixture.native_auth(), native);
    assert_eq!(
        fixture.native_auth()["https://another-issuer.test"]["key"],
        "untouched"
    );
}

#[tokio::test]
async fn concurrent_rejections_and_short_expiry_rotate_the_single_use_pair_once() {
    let fixture = Fixture::new();
    let observed = fixture.saved();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let server = MockServer::start(move |request| {
        let counter = counter.clone();
        async move {
            assert!(request.body.contains("refresh_token=old-refresh"));
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(20)).await;
            response_json(
                200,
                json!({"access_token":"new-access", "refresh_token":"new-refresh", "expires_in":1}),
            )
        }
    })
    .await;
    let (first, second) = tokio::join!(
        obtain(
            &fixture.path,
            None,
            &observed,
            true,
            Some("old-access"),
            None,
            &server.url
        ),
        obtain(
            &fixture.path,
            None,
            &observed,
            true,
            Some("old-access"),
            None,
            &server.url
        )
    );
    for credentials in [first.unwrap(), second.unwrap()] {
        assert_eq!(credentials["accessToken"], "new-access");
        assert!(credentials.get("refreshToken").is_none());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.saved().auth["refresh_token"], "new-refresh");
}

#[tokio::test]
async fn rejected_generation_does_not_rotate_again_when_access_token_is_reissued_unchanged() {
    let fixture = Fixture::new();
    let rejected = value(&fixture.saved());
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let server = MockServer::start(move |_| {
        let counter = counter.clone();
        async move {
            counter.fetch_add(1, Ordering::SeqCst);
            response_json(
                200,
                json!({"access_token":"old-access", "refresh_token":"new-refresh", "expires_in":1}),
            )
        }
    })
    .await;
    let observed = fixture.saved();
    let first = obtain(
        &fixture.path,
        None,
        &observed,
        true,
        Some("old-access"),
        rejected["credentialGeneration"].as_str(),
        &server.url,
    )
    .await
    .unwrap();
    let latest = fixture.saved();
    let second = obtain(
        &fixture.path,
        None,
        &latest,
        true,
        Some("old-access"),
        rejected["credentialGeneration"].as_str(),
        &server.url,
    )
    .await
    .unwrap();
    assert_eq!(first, second);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(latest.auth["refresh_token"], "new-refresh");
}

#[tokio::test]
async fn fresh_login_during_refresh_wins_and_deleted_snapshots_are_not_recreated() {
    for delete in [false, true] {
        let fixture = Fixture::new();
        let observed = fixture.saved();
        let path = fixture.path.clone();
        let store = fixture.store.clone();
        let key = observed.key.clone();
        let mut login = json!({key.clone():observed.auth.clone()});
        login[&key]["key"] = json!("fresh-login");
        login[&key]["refresh_token"] = json!("fresh-login-refresh");
        let server = MockServer::start(move |_| {
            let path = path.clone(); let store = store.clone(); let key = key.clone(); let login = login.clone();
            async move {
                if delete { fs::remove_file(path).unwrap(); }
                else { let _store = ACCOUNT_LOCK.lock().unwrap(); save_at(&store, &key, &login).unwrap(); }
                response_json(200, json!({"access_token":"obsolete-result", "refresh_token":"obsolete-refresh", "expires_in":3600}))
            }
        }).await;
        let result = obtain(
            &fixture.path,
            None,
            &observed,
            true,
            Some("old-access"),
            None,
            &server.url,
        )
        .await;
        if delete {
            assert!(result.is_err());
            assert!(!fixture.path.exists());
        } else {
            assert_eq!(result.unwrap()["accessToken"], "fresh-login");
            assert_eq!(fixture.saved().auth["refresh_token"], "fresh-login-refresh");
        }
    }
}

#[tokio::test]
async fn failed_save_retains_rotated_pair_in_memory_and_recovers_from_updated_native() {
    let fixture = Fixture::new();
    fixture.apply();
    let before = fs::read(&fixture.path).unwrap();
    let failed = FailedWrite::new(&fixture.path);
    let mut session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    let server = MockServer::start(|_| async {
        response_json(
            200,
            json!({"access_token":"new-access", "refresh_token":"new-refresh", "expires_in":3600}),
        )
    })
    .await;
    session
        .renew(Some("old-access"), &server.url)
        .await
        .unwrap();
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
    assert_eq!(
        current(&fixture.path).unwrap().auth["refresh_token"],
        "new-refresh"
    );
    assert_eq!(fixture.native_auth()[SCOPE]["refresh_token"], "new-refresh");
    drop(session);
    drop(failed);
    forget_pending(&fixture.path);
    let recovered = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    assert_eq!(recovered.saved.auth["key"], "new-access");
    assert_eq!(fixture.saved().auth["refresh_token"], "new-refresh");
}

#[tokio::test]
async fn failed_save_without_a_native_client_flushes_the_pair_on_the_next_explicit_operation() {
    let fixture = Fixture::new();
    let observed = fixture.saved();
    let before = fs::read(&fixture.path).unwrap();
    let failed = FailedWrite::new(&fixture.path);
    let server = MockServer::start(|_| async {
        response_json(
            200,
            json!({"access_token":"new-access", "refresh_token":"new-refresh", "expires_in":3600}),
        )
    })
    .await;
    let result = obtain(
        &fixture.path,
        None,
        &observed,
        true,
        Some("old-access"),
        None,
        &server.url,
    )
    .await
    .unwrap();
    assert_eq!(result["accessToken"], "new-access");
    assert_eq!(fs::read(&fixture.path).unwrap(), before);
    drop(failed);
    let recovered = Session::open(&fixture.path, None).await.unwrap();
    assert_eq!(recovered.saved.auth["refresh_token"], "new-refresh");
    assert_eq!(fixture.saved().auth["key"], "new-access");
    assert!(!pending().lock().unwrap().contains_key(&fixture.path));
    assert!(!fixture.native.exists());
}

#[tokio::test]
async fn failed_native_write_keeps_own_rotation_and_heals_original_native_pair_on_retry() {
    let fixture = Fixture::new();
    fixture.apply();
    let mut session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    let failed = FailedWrite::new(&fixture.native);
    let server = MockServer::start(|_| async {
        response_json(
            200,
            json!({"access_token":"new-access", "refresh_token":"new-refresh", "expires_in":3600}),
        )
    })
    .await;
    assert_eq!(
        session
            .renew(Some("old-access"), &server.url)
            .await
            .unwrap_err(),
        "accountError.write"
    );
    assert_eq!(fixture.saved().auth["refresh_token"], "new-refresh");
    assert_eq!(fixture.native_auth()[SCOPE]["key"], "old-access");
    drop(session);
    drop(failed);
    let healed = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    assert_eq!(healed.saved.auth["key"], "new-access");
    assert_eq!(fixture.native_auth()[SCOPE]["refresh_token"], "new-refresh");
}

#[tokio::test]
async fn caller_cancellation_finishes_consumed_refresh_and_retains_native_lock_until_saved() {
    for quota_refresh in [false, true] {
        let fixture = Fixture::new();
        fixture.apply();
        let consumed = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let captured = consumed.clone();
        let released = release.clone();
        let server = MockServer::start(move |request| {
            let captured = captured.clone(); let released = released.clone();
            async move {
                if request.path == "/settings" {
                    return if request.headers[axum::http::header::AUTHORIZATION] == "Bearer old-access" {
                        response_json(401, json!({}))
                    } else { response_json(200, json!({"subscription_tier_display":"Free"})) };
                }
                assert_eq!(request.path, "/token");
                assert!(request.body.contains("refresh_token=old-refresh"));
                captured.notify_one();
                released.notified().await;
                response_json(200, json!({"access_token":"delayed-access", "refresh_token":"delayed-refresh", "expires_in":3600}))
            }
        }).await;
        let path = fixture.path.clone();
        let native = fixture.native.clone();
        let base = server.url.clone();
        let observed = fixture.saved();
        let caller = tokio::spawn(async move {
            complete(async move {
                if quota_refresh {
                    refresh_with_native(
                        &path,
                        Some(&native),
                        &format!("{base}/settings"),
                        &format!("{base}/token"),
                        None,
                    )
                    .await
                    .map(|_| ())
                } else {
                    obtain(
                        &path,
                        Some(&native),
                        &observed,
                        true,
                        Some("old-access"),
                        None,
                        &format!("{base}/token"),
                    )
                    .await
                    .map(|_| ())
                }
            })
            .await
        });
        tokio::time::timeout(Duration::from_secs(2), consumed.notified())
            .await
            .unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        let external = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(fixture.native.with_file_name("auth.json.lock"))
            .unwrap();
        assert!(fs2::FileExt::try_lock_exclusive(&external).is_err());
        release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let saved = fixture.saved();
                let complete = saved.auth["refresh_token"] == "delayed-refresh"
                    && fixture.native_auth()[SCOPE]["refresh_token"] == "delayed-refresh"
                    && (!quota_refresh || saved.summary.plan.as_deref() == Some("Free"));
                if complete && fs2::FileExt::try_lock_exclusive(&external).is_ok() {
                    fs2::FileExt::unlock(&external).unwrap();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        drop(external);
        assert_eq!(fixture.saved().auth["key"], "delayed-access");
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_replaced_native_lock_is_detected_without_writing_through_the_old_inode() {
    let fixture = Fixture::new();
    fixture.apply();
    let guard = storage::NativeLock::acquire(&fixture.native, false)
        .await
        .unwrap()
        .unwrap();
    let path = fixture.native.with_file_name("auth.json.lock");
    fs::remove_file(&path).unwrap();
    fs::write(&path, "replacement").unwrap();
    assert_eq!(guard.ensure_current().unwrap_err(), "accountError.busy");
    drop(guard);
    assert_eq!(fs::read_to_string(path).unwrap(), "replacement");
}

#[cfg(unix)]
#[tokio::test]
async fn native_apply_and_rotation_preserve_the_auth_file_symlink() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.native.parent().unwrap()).unwrap();
    let target = fixture.root.join("shared/auth.json");
    crate::services::cursor_auth::write(&target, &json!({"other":{"key":"kept"}})).unwrap();
    std::os::unix::fs::symlink(&target, &fixture.native).unwrap();
    fixture.apply();
    assert!(fs::symlink_metadata(&fixture.native)
        .unwrap()
        .file_type()
        .is_symlink());
    let server = MockServer::start(|_| async {
        response_json(
            200,
            json!({"access_token":"new-access", "refresh_token":"new-refresh", "expires_in":3600}),
        )
    })
    .await;
    let mut session = Session::open(&fixture.path, Some(&fixture.native))
        .await
        .unwrap();
    session
        .renew(Some("old-access"), &server.url)
        .await
        .unwrap();
    assert!(fs::symlink_metadata(&fixture.native)
        .unwrap()
        .file_type()
        .is_symlink());
    let native: Value = crate::services::cursor_auth::read(&target).unwrap();
    assert_eq!(native[SCOPE]["refresh_token"], "new-refresh");
    assert_eq!(native["other"]["key"], "kept");
}

#[tokio::test]
async fn native_os_lock_blocks_another_holder_and_the_lock_file_is_retained() {
    let fixture = Fixture::new();
    fixture.apply();
    let lock_path = fixture.native.with_file_name("auth.json.lock");
    let external = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&external).unwrap();
    let native = fixture.native.clone();
    let waiting = tokio::spawn(async move { storage::NativeLock::acquire(&native, false).await });
    tokio::time::sleep(Duration::from_millis(75)).await;
    assert!(!waiting.is_finished());
    assert!(lock_path.exists());
    fs2::FileExt::unlock(&external).unwrap();
    let acquired = tokio::time::timeout(Duration::from_secs(2), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(fs2::FileExt::try_lock_exclusive(&external).is_err());
    drop(acquired);
    fs2::FileExt::try_lock_exclusive(&external).unwrap();
    fs2::FileExt::unlock(&external).unwrap();
    drop(external);
    assert!(lock_path.exists());
}
