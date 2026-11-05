mod api;
mod native;
mod oauth;
pub use oauth::LoginStart;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    QwenWork,
    QwenWorkCn,
}

impl Edition {
    pub(crate) fn tool(self) -> &'static str {
        match self {
            Self::QwenWork => "qwenwork",
            Self::QwenWorkCn => "qwenworkcn",
        }
    }
    fn data_name(self) -> &'static str {
        match self {
            Self::QwenWork => "QwenWork",
            Self::QwenWorkCn => "QwenWorkCN",
        }
    }
    fn package(self) -> &'static str {
        match self {
            Self::QwenWork => "qwen-work",
            Self::QwenWorkCn => "qwen-work-cn",
        }
    }
    fn base(self) -> &'static str {
        match self {
            Self::QwenWork => "https://gateway.qwenwork.ai",
            Self::QwenWorkCn => "https://gateway.qwenwork.cn",
        }
    }
    fn client_id(self) -> &'static str {
        match self {
            Self::QwenWork => "cc65e5fc-05bd-4f5d-a4e8-0df19aa3d75a",
            Self::QwenWorkCn => "e883ade2-e6e3-4d6d-adf7-f92ceff5fdcb",
        }
    }
    fn redirect(self) -> &'static str {
        match self {
            Self::QwenWork => "qwenwork://",
            Self::QwenWorkCn => "qwenwork-cn://",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditBucket {
    pub name: String,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReward {
    pub status: String,
    pub reward_credits: Option<f64>,
    pub checked_at: i64,
    pub next_claim_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub edition: Edition,
    pub name: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub plan: Option<String>,
    pub plan_name: Option<String>,
    pub expires_at: Option<i64>,
    pub active: bool,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub credit_buckets: Vec<CreditBucket>,
    pub daily_reward: Option<DailyReward>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Credentials {
    edition: Edition,
    auth: Value,
}

impl Credentials {
    fn validate(&self) -> Result<(), String> {
        if self.auth["schemaVersion"] != 2
            || [
                self.auth["token"].as_str(),
                self.auth["refreshToken"].as_str(),
                self.auth["user"]["id"].as_str(),
            ]
            .iter()
            .any(|value| value.is_none_or(|value| value.trim().is_empty()))
            || api::timestamp(&self.auth["expiresAt"]).is_none()
            || (self.auth["user"]["isBiz"] == true
                && api::text(&self.auth["user"], &["teamId"]).is_none())
        {
            return Err("accountError.invalidAccount".into());
        }
        Ok(())
    }
    fn id(&self) -> String {
        let user = &self.auth["user"];
        format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}\0{}\0{}",
                self.edition.tool(),
                user["id"].as_str().unwrap_or(""),
                user["isBiz"] == true,
                user["orgId"].as_str().unwrap_or(""),
                user["teamId"].as_str().unwrap_or("")
            ))
        )
    }
    fn summary(&self) -> Account {
        let user = &self.auth["user"];
        Account {
            id: self.id(),
            edition: self.edition,
            name: api::text(user, &["name", "email", "id"])
                .unwrap_or_default()
                .into(),
            email: api::text(user, &["email"]).map(str::to_owned),
            organization: api::text(user, &["orgName"]).map(str::to_owned),
            plan: api::text(user, &["planId"]).map(str::to_owned),
            plan_name: api::text(user, &["planName"]).map(str::to_owned),
            expires_at: api::timestamp(&user["planNextDueDate"]),
            active: false,
            remaining: None,
            total: None,
            credit_buckets: vec![],
            daily_reward: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
}
type Store = BTreeMap<String, Saved>;

fn path(edition: Edition) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(format!(".echobird/{}-accounts.json", edition.tool())))
}
fn load(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    super::cursor_auth::read(path)
}
fn protect(c: &Credentials) -> Result<String, String> {
    c.validate()?;
    let value = super::model_manager::encrypt_key_for_storage(
        &serde_json::to_string(c).map_err(|_| "accountError.format")?,
    );
    if !value.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(value)
}
fn credential(store: &Store, edition: Edition, id: &str) -> Result<Credentials, String> {
    let saved = store
        .get(id)
        .filter(|saved| saved.account.edition == edition && saved.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    if !saved.credential.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    let c: Credentials = serde_json::from_str(&super::model_manager::decrypt_key_for_use(
        &saved.credential,
    ))
    .map_err(|_| "accountError.keychain")?;
    c.validate()?;
    if c.edition != edition || c.id() != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(c)
}
fn request_lock(edition: Edition, id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    static LOCKS: OnceLock<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    Ok(LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "accountError.read")?
        .entry(format!("{}:{id}", edition.tool()))
        .or_default()
        .clone())
}

pub async fn list(edition: Edition) -> Result<Vec<Account>, String> {
    let _guard = STORE_LOCK.lock().await;
    Ok(load(&path(edition)?)?
        .into_values()
        .filter(|s| s.account.edition == edition)
        .map(|s| s.account)
        .collect())
}
pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
    oauth::start(edition).await
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    oauth::cancel(id)
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some(c) = oauth::poll(id)? else {
        return Ok(None);
    };
    let request = request_lock(c.edition, &c.id())?;
    let _request = request.lock().await;
    let _guard = STORE_LOCK.lock().await;
    oauth::complete(id, || {
        let path = path(c.edition)?;
        let mut store = load(&path)?;
        let mut account = c.summary();
        if let Some(previous) = store.get(&account.id) {
            account.active = previous.account.active;
        }
        store.insert(
            account.id.clone(),
            Saved {
                account: account.clone(),
                credential: protect(&c)?,
            },
        );
        super::cursor_auth::write(&path, &store)?;
        Ok(Some(account))
    })
}
pub async fn delete(edition: Edition, id: &str) -> Result<(), String> {
    let request = request_lock(edition, id)?;
    let _request = request.lock().await;
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    if !store
        .get(id)
        .is_some_and(|row| row.account.edition == edition)
    {
        return Err("accountError.invalidAccount".into());
    }
    store.remove(id);
    super::cursor_auth::write(&path, &store)?;
    pending_rotations()
        .lock()
        .map_err(|_| "accountError.write")?
        .retain(|_, c| c.edition != edition || c.id() != id);
    Ok(())
}

fn pending_rotations() -> &'static Mutex<BTreeMap<String, Credentials>> {
    static ROTATIONS: OnceLock<Mutex<BTreeMap<String, Credentials>>> = OnceLock::new();
    ROTATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn rotation_key(c: &Credentials) -> Result<String, String> {
    Ok(format!(
        "{}:{:x}",
        c.id(),
        Sha256::digest(
            c.auth["refreshToken"]
                .as_str()
                .ok_or("accountError.invalidAccount")?
        )
    ))
}
async fn renew_once(
    c: &Credentials,
    client: &api::Client,
) -> Result<(String, Credentials), String> {
    let key = rotation_key(c)?;
    let cached = pending_rotations()
        .lock()
        .map_err(|_| "accountError.read")?
        .get(&key)
        .cloned();
    let next = match cached {
        Some(next) => next,
        None => {
            let next = client.refresh(c).await?;
            pending_rotations()
                .lock()
                .map_err(|_| "accountError.write")?
                .insert(key.clone(), next.clone());
            next
        }
    };
    Ok((key, next))
}

async fn credentials(edition: Edition, id: &str) -> Result<Credentials, String> {
    credentials_at(&path(edition)?, edition, id, credential).await
}

async fn credentials_at(
    path: &Path,
    edition: Edition,
    id: &str,
    open: fn(&Store, Edition, &str) -> Result<Credentials, String>,
) -> Result<Credentials, String> {
    let _guard = STORE_LOCK.lock().await;
    open(&load(path)?, edition, id)
}

async fn rotate(c: &Credentials, force: bool, client: &api::Client) -> Result<Credentials, String> {
    rotate_at(c, force, client, &path(c.edition)?, credential, protect).await
}

async fn rotate_at(
    c: &Credentials,
    force: bool,
    client: &api::Client,
    path: &Path,
    open: fn(&Store, Edition, &str) -> Result<Credentials, String>,
    seal: fn(&Credentials) -> Result<String, String>,
) -> Result<Credentials, String> {
    if !force
        && !pending_rotations()
            .lock()
            .map_err(|_| "accountError.read")?
            .contains_key(&rotation_key(c)?)
        && api::timestamp(&c.auth["expiresAt"])
            .is_some_and(|t| t > chrono::Utc::now().timestamp() + 300)
    {
        return Ok(c.clone());
    }
    let (key, next) = renew_once(c, client).await?;
    let _guard = STORE_LOCK.lock().await;
    let mut store = load(path)?;
    let current = open(&store, c.edition, &c.id())?;
    if current.auth != c.auth {
        pending_rotations()
            .lock()
            .map_err(|_| "accountError.write")?
            .remove(&key);
        return Ok(current);
    }
    store
        .get_mut(&c.id())
        .ok_or("accountError.invalidAccount")?
        .credential = seal(&next)?;
    super::cursor_auth::write(path, &store)?;
    pending_rotations()
        .lock()
        .map_err(|_| "accountError.write")?
        .remove(&key);
    Ok(next)
}

// The signed-in client holds the same single-use refresh token: use its newer
// copy, and give it ours after a rotation.
async fn adopt_client(edition: Edition, id: &str) -> Result<Option<String>, String> {
    let Some(live) = native::read_active(edition).filter(|live| live.id() == id) else {
        return Ok(None);
    };
    let path = path(edition)?;
    let _guard = STORE_LOCK.lock().await;
    let mut store = load(&path)?;
    let saved = credential(&store, edition, id)?;
    if live.auth["refreshToken"] != saved.auth["refreshToken"]
        && api::timestamp(&live.auth["expiresAt"]) > api::timestamp(&saved.auth["expiresAt"])
    {
        store
            .get_mut(id)
            .ok_or("accountError.invalidAccount")?
            .credential = protect(&live)?;
        super::cursor_auth::write(&path, &store)?;
    }
    Ok(live.auth["refreshToken"].as_str().map(str::to_owned))
}

async fn share_with_client(edition: Edition, id: &str, client_refresh: Option<String>) {
    let Some(client_refresh) = client_refresh else {
        return;
    };
    let Ok(saved) = credentials(edition, id).await else {
        return;
    };
    if saved.auth["refreshToken"].as_str() != Some(client_refresh.as_str()) {
        if let Err(error) = native::store_rotation(&saved) {
            log::warn!("[QwenWork] Could not update the client login: {error}");
        }
    }
}

pub(crate) async fn speech_credentials(
    edition: Edition,
    id: &str,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    let client_refresh = adopt_client(edition, id).await.unwrap_or(None);
    let result = speech_credentials_at(
        edition,
        id,
        rejected,
        &path(edition)?,
        &api::Client::for_account(edition)?,
        credential,
        protect,
    )
    .await;
    share_with_client(edition, id, client_refresh).await;
    result
}

pub(crate) async fn proxy_credentials(
    edition: Edition,
    id: &str,
    force: bool,
) -> Result<Value, String> {
    let rejected = if force {
        let current = credentials(edition, id).await?;
        Some(serde_json::json!({"accessToken":api::token(&current)?}))
    } else {
        None
    };
    speech_credentials(edition, id, rejected.as_ref()).await
}

async fn speech_credentials_at(
    edition: Edition,
    id: &str,
    rejected: Option<&Value>,
    path: &Path,
    client: &api::Client,
    open: fn(&Store, Edition, &str) -> Result<Credentials, String>,
    seal: fn(&Credentials) -> Result<String, String>,
) -> Result<Value, String> {
    if client.edition != edition {
        return Err("accountError.invalidAccount".into());
    }
    let rejected = rejected
        .map(|value| {
            value["accessToken"]
                .as_str()
                .filter(|token| !token.is_empty())
                .ok_or("accountError.invalidAccount")
        })
        .transpose()?;
    let request = request_lock(edition, id)?;
    let _request = request.lock().await;
    let c = credentials_at(path, edition, id, open).await?;
    if c.auth["authRealm"]
        .as_str()
        .is_some_and(|realm| realm != "standard")
    {
        return Err("accountError.invalidAccount".into());
    }
    let force = rejected.is_some_and(|token| c.auth["token"].as_str() == Some(token));
    let c = rotate_at(&c, force, client, path, open, seal).await?;
    Ok(serde_json::json!({
        "edition": c.edition,
        "accessToken": api::token(&c)?,
        "userId": c.auth["user"]["id"],
        "name": c.auth["user"]["name"],
        "email": c.auth["user"]["email"],
        "version": client.version,
    }))
}

pub async fn refresh(edition: Edition, id: &str) -> Result<Account, String> {
    let request = request_lock(edition, id)?;
    let _request = request.lock().await;
    let client = api::Client::for_account(edition)?;
    let c = rotate(&credentials(edition, id).await?, false, &client).await?;
    let result = client.quota(&c).await;
    let quota = if matches!(&result, Err(e) if e == "accountError.loginRequired") {
        let c = rotate(&c, true, &client).await?;
        client.quota(&c).await?
    } else {
        result?
    };
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    credential(&store, edition, id)?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    quota.apply(&mut saved.account);
    saved.account.daily_reward = None;
    let account = saved.account.clone();
    super::cursor_auth::write(&path, &store)?;
    Ok(account)
}

pub async fn apply(edition: Edition, id: &str) -> Result<Account, String> {
    let request = request_lock(edition, id)?;
    let _request = request.lock().await;
    let client = api::Client::installed(edition)?;
    let mut c = rotate(&credentials(edition, id).await?, false, &client).await?;
    c.auth["user"] = client.user(api::token(&c)?).await?;
    if c.id() != id {
        return Err("accountError.invalidAccount".into());
    }
    let transaction = native::apply(&c).await?;
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    credential(&store, edition, id)?;
    for saved in store.values_mut() {
        saved.account.active = saved.account.id == id;
    }
    let account = store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone();
    super::cursor_auth::write(&path, &store)?;
    transaction.commit();
    Ok(account)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod speech_tests {
    use super::*;
    use axum::{http::StatusCode, routing::post, Json, Router};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Only synthetic credentials use this codec; fixtures never read the machine encryption key.
    fn seal(c: &Credentials) -> Result<String, String> {
        c.validate()?;
        Ok(format!(
            "enc:v1:fixture:{}",
            serde_json::to_string(c).map_err(|_| "fixture encoding")?
        ))
    }

    fn open(store: &Store, edition: Edition, id: &str) -> Result<Credentials, String> {
        let saved = store
            .get(id)
            .filter(|saved| saved.account.edition == edition && saved.account.id == id)
            .ok_or("accountError.invalidAccount")?;
        let raw = saved
            .credential
            .strip_prefix("enc:v1:fixture:")
            .ok_or("fixture encoding")?;
        let c: Credentials = serde_json::from_str(raw).map_err(|_| "fixture encoding")?;
        c.validate()?;
        if c.edition != edition || c.id() != id {
            return Err("accountError.invalidAccount".into());
        }
        Ok(c)
    }

    struct Fixture {
        path: PathBuf,
        c: Credentials,
        id: String,
    }

    impl Fixture {
        fn new(edition: Edition, expired: bool) -> Self {
            let mut c = tests::fixture(edition);
            c.auth["user"]["id"] = json!(uuid::Uuid::new_v4().to_string());
            if expired {
                c.auth["expiresAt"] = json!("2000-01-01T00:00:00.000Z");
            }
            let id = c.id();
            let path = std::env::temp_dir().join(format!("qwenwork-speech-{id}.json"));
            let mut account = c.summary();
            account.remaining = Some(7.0);
            account.active = true;
            let store = Store::from([(
                id.clone(),
                Saved {
                    account,
                    credential: seal(&c).unwrap(),
                },
            )]);
            super::super::cursor_auth::write(&path, &store).unwrap();
            Self { path, c, id }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            pending_rotations()
                .lock()
                .unwrap()
                .retain(|key, _| !key.starts_with(&format!("{}:", self.id)));
        }
    }

    struct Server {
        client: api::Client,
        refreshes: Arc<AtomicUsize>,
        unexpected: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server(edition: Edition, reject: bool) -> Server {
        let refreshes = Arc::new(AtomicUsize::new(0));
        let unexpected = Arc::new(AtomicUsize::new(0));
        let refreshed = refreshes.clone();
        let other = unexpected.clone();
        let app = Router::new()
            .route("/api/v1/deviceToken/refresh", post(move |Json(body): Json<Value>| {
                let refreshed = refreshed.clone();
                async move {
                    let attempt = refreshed.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(body["refresh_token"], "fixture-refresh-secret");
                    assert_eq!(body["target"], "c");
                    assert_eq!(body["client_id"], edition.client_id());
                    if reject || attempt != 0 {
                        return (StatusCode::UNAUTHORIZED, Json(json!({"error":"private consumed token"})));
                    }
                    (StatusCode::OK, Json(json!({"device_token":"rotated-access","refresh_token":"rotated-refresh","expires_in":3600})))
                }
            }))
            .fallback(move || {
                let other = other.clone();
                async move {
                    other.fetch_add(1, Ordering::SeqCst);
                    StatusCode::NOT_FOUND
                }
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let client = api::Client::new(edition, &base, "1.3.2-26101704").unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Server {
            client,
            refreshes,
            unexpected,
            task,
        }
    }

    async fn read(
        fixture: &Fixture,
        server: &Server,
        rejected: Option<&Value>,
    ) -> Result<Value, String> {
        speech_credentials_at(
            fixture.c.edition,
            &fixture.id,
            rejected,
            &fixture.path,
            &server.client,
            open,
            seal,
        )
        .await
    }

    #[tokio::test]
    async fn valid_saved_accounts_supply_speech_identity_and_app_version_without_refresh_or_writes()
    {
        for edition in [Edition::QwenWork, Edition::QwenWorkCn] {
            let fixture = Fixture::new(edition, false);
            let server = server(edition, false).await;
            let before = std::fs::read(&fixture.path).unwrap();
            let value = read(&fixture, &server, None).await.unwrap();
            assert_eq!(value["edition"], serde_json::to_value(edition).unwrap());
            assert_eq!(value["accessToken"], fixture.c.auth["token"]);
            assert_eq!(value["userId"], fixture.c.auth["user"]["id"]);
            assert_eq!(value["name"], "Fixture");
            assert_eq!(value["email"], "fixture@example.test");
            assert_eq!(value["version"], "1.3.2");
            assert!(value.get("refreshToken").is_none());
            assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
            assert_eq!(server.refreshes.load(Ordering::SeqCst), 0);
            assert_eq!(server.unexpected.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn concurrent_rejections_renew_once_and_keep_selected_account_metadata() {
        let fixture = Fixture::new(Edition::QwenWorkCn, false);
        let server = server(fixture.c.edition, false).await;
        let rejected = json!({"accessToken":fixture.c.auth["token"]});
        let before =
            serde_json::to_value(&load(&fixture.path).unwrap()[&fixture.id].account).unwrap();
        let (first, second) = tokio::join!(
            read(&fixture, &server, Some(&rejected)),
            read(&fixture, &server, Some(&rejected))
        );
        assert_eq!(first.unwrap()["accessToken"], "rotated-access");
        assert_eq!(second.unwrap()["accessToken"], "rotated-access");
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(server.unexpected.load(Ordering::SeqCst), 0);
        let saved = load(&fixture.path).unwrap();
        assert_eq!(
            open(&saved, fixture.c.edition, &fixture.id).unwrap().auth["refreshToken"],
            "rotated-refresh"
        );
        assert_eq!(
            serde_json::to_value(&saved[&fixture.id].account).unwrap(),
            before
        );
        let bytes = std::fs::read(&fixture.path).unwrap();
        assert_eq!(
            read(&fixture, &server, None).await.unwrap()["accessToken"],
            "rotated-access"
        );
        assert_eq!(std::fs::read(&fixture.path).unwrap(), bytes);
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_token_rotates_before_speech_and_failed_refresh_preserves_the_store() {
        for rejected_refresh in [false, true] {
            let fixture = Fixture::new(Edition::QwenWork, true);
            let server = server(fixture.c.edition, rejected_refresh).await;
            let before = std::fs::read(&fixture.path).unwrap();
            let result = read(&fixture, &server, None).await;
            if rejected_refresh {
                assert_eq!(result.unwrap_err(), "accountError.loginRequired");
                assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
            } else {
                assert_eq!(result.unwrap()["accessToken"], "rotated-access");
                assert_eq!(
                    open(
                        &load(&fixture.path).unwrap(),
                        fixture.c.edition,
                        &fixture.id
                    )
                    .unwrap()
                    .auth["token"],
                    "rotated-access"
                );
            }
            assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
            assert_eq!(server.unexpected.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn retry_after_local_save_failure_reuses_the_single_use_rotation() {
        fn cannot_seal(_: &Credentials) -> Result<String, String> {
            Err("fixture write failure".into())
        }
        let fixture = Fixture::new(Edition::QwenWorkCn, false);
        let server = server(fixture.c.edition, false).await;
        let rejected = json!({"accessToken":fixture.c.auth["token"]});
        let before = std::fs::read(&fixture.path).unwrap();
        let error = speech_credentials_at(
            fixture.c.edition,
            &fixture.id,
            Some(&rejected),
            &fixture.path,
            &server.client,
            open,
            cannot_seal,
        )
        .await
        .unwrap_err();
        assert_eq!(error, "fixture write failure");
        assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
        assert_eq!(
            read(&fixture, &server, Some(&rejected)).await.unwrap()["accessToken"],
            "rotated-access"
        );
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(server.unexpected.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn missing_or_cross_region_accounts_and_invalid_rejections_do_not_refresh() {
        let fixture = Fixture::new(Edition::QwenWorkCn, false);
        let server = server(fixture.c.edition, false).await;
        let before = std::fs::read(&fixture.path).unwrap();
        assert!(speech_credentials_at(
            Edition::QwenWork,
            &fixture.id,
            None,
            &fixture.path,
            &server.client,
            open,
            seal
        )
        .await
        .is_err());
        assert!(speech_credentials_at(
            fixture.c.edition,
            "missing",
            None,
            &fixture.path,
            &server.client,
            open,
            seal
        )
        .await
        .is_err());
        assert!(read(&fixture, &server, Some(&json!({}))).await.is_err());
        let unrelated = json!({"accessToken":"already-replaced-token"});
        assert_eq!(
            read(&fixture, &server, Some(&unrelated)).await.unwrap()["accessToken"],
            fixture.c.auth["token"]
        );
        assert_eq!(std::fs::read(&fixture.path).unwrap(), before);
        assert_eq!(server.refreshes.load(Ordering::SeqCst), 0);
        assert_eq!(server.unexpected.load(Ordering::SeqCst), 0);
    }
}
