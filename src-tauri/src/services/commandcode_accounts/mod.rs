//! Command Code's browser callback login and native account storage.
use super::{cursor_auth::write, secret_store};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub const BASE: &str = "https://api.commandcode.ai/provider/v1";
const API: &str = "https://api.commandcode.ai";
const ORIGIN: &str = "https://commandcode.ai";
static ACCOUNT_LOCK: Mutex<()> = Mutex::const_new(());
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub remaining: Option<f64>,
    #[serde(default)]
    pub total: Option<f64>,
    #[serde(default)]
    pub active: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    #[serde(flatten)]
    account: Account,
    key: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    accounts: Vec<Saved>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub authorization_url: String,
    pub expires_at: i64,
}
struct Pending {
    state: String,
    key: Mutex<Option<String>>,
    cancelled: CancellationToken,
}
fn logins() -> &'static Mutex<HashMap<String, Arc<Pending>>> {
    static LOGINS: OnceLock<Mutex<HashMap<String, Arc<Pending>>>> = OnceLock::new();
    LOGINS.get_or_init(Mutex::default)
}
pub(crate) fn native(name: &str) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".commandcode")
        .join(name))
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/commandcode-accounts.json"))
}
fn load() -> Result<Store, String> {
    super::json_store::read(&store_path()?)
}
fn saved(id: &str) -> Result<Saved, String> {
    load()?
        .accounts
        .into_iter()
        .find(|s| s.account.id == id)
        .ok_or_else(|| "accountError.invalidAccount".into())
}
pub fn credentials(id: &str) -> Result<String, String> {
    let key = saved(id)?.key;
    if key.starts_with(secret_store::ENCRYPTED_PREFIX) {
        secret_store::decrypt(&key)
    } else {
        Ok(key)
    }
}
pub fn list_saved_accounts() -> Result<Vec<Account>, String> {
    Ok(load()?.accounts.into_iter().map(|s| s.account).collect())
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    let current = read_object(&native("auth.json")?)?["apiKey"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    load()?
        .accounts
        .into_iter()
        .map(|mut s| {
            let key = if s.key.starts_with(secret_store::ENCRYPTED_PREFIX) {
                secret_store::decrypt(&s.key)?
            } else {
                s.key
            };
            s.account.active = !current.is_empty() && current == key;
            Ok(s.account)
        })
        .collect()
}
pub(crate) fn read_object(path: &std::path::Path) -> Result<Value, String> {
    let value: Value = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "accountError.format")?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(_) => return Err("accountError.read".into()),
    };
    if !value.is_object() {
        return Err("accountError.format".into());
    }
    Ok(value)
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}
async fn request(client: &reqwest::Client, path: &str, key: &str) -> Result<Value, String> {
    let response = client
        .get(format!("{API}{path}"))
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| "accountError.failed")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err("accountError.failed".into());
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.format".into())
}
fn plan_name(id: &str) -> String {
    let normalized = id.to_lowercase().replace('_', "-");
    for (prefix, name) in [
        ("individual-provider", "Provider"),
        ("individual-goat", "GOAT"),
        ("individual-ultra", "Ultra"),
        ("individual-max", "Max"),
        ("individual-pro", "Pro"),
        ("individual-go", "Go"),
        ("teams-pro", "Teams Pro"),
    ] {
        if let Some(suffix) = normalized.strip_prefix(prefix) {
            return match suffix
                .strip_prefix('-')
                .and_then(|s| s.split('-').next())
                .filter(|s| {
                    s.ends_with('x') && s[..s.len() - 1].chars().all(|c| c.is_ascii_digit())
                }) {
                Some(tier) => format!("{name} {}×", &tier[..tier.len() - 1]),
                None => name.into(),
            };
        }
    }
    id.into()
}
async fn details(key: &str) -> Result<Account, String> {
    let client = client()?;
    let user = request(&client, "/alpha/whoami", key).await?;
    let (subscription, credits) = tokio::join!(
        request(&client, "/alpha/billing/subscriptions", key),
        request(&client, "/alpha/billing/credits", key)
    );
    let subscription = subscription.ok();
    let credits = credits.ok();
    let plan = subscription
        .as_ref()
        .filter(|s| {
            !matches!(
                s["data"]["status"].as_str(),
                Some("canceled" | "incomplete_expired")
            )
        })
        .and_then(|s| s["data"]["planId"].as_str())
        .or_else(|| {
            credits
                .as_ref()
                .and_then(|s| s["credits"]["planId"].as_str())
        })
        .or_else(|| {
            subscription
                .as_ref()
                .filter(|s| s["success"] == true && s["data"].is_null())
                .map(|_| "No plan")
        })
        .map(plan_name);
    let remaining = credits
        .as_ref()
        .and_then(|c| c.get("credits"))
        .filter(|c| c.is_object())
        .and_then(|c| {
            let values: Vec<f64> = ["monthlyCredits", "purchasedCredits", "freeCredits"]
                .iter()
                .filter_map(|k| c[k].as_f64())
                .collect();
            (!values.is_empty()).then(|| values.into_iter().sum())
        });
    let email = ["email", "userName", "id"]
        .iter()
        .find_map(|k| user["user"][k].as_str())
        .ok_or("accountError.authResponse")?
        .to_owned();
    Ok(Account {
        id: format!("{:x}", Sha256::digest(key.as_bytes())),
        email,
        plan,
        remaining,
        total: None,
        active: false,
    })
}
fn save_to(path: &std::path::Path, entry: Saved, require_existing: bool) -> Result<(), String> {
    let mut store: Store = super::json_store::read(path)?;
    if require_existing
        && !store
            .accounts
            .iter()
            .any(|saved| saved.account.id == entry.account.id)
    {
        return Err("accountError.invalidAccount".into());
    }
    store
        .accounts
        .retain(|saved| saved.account.id != entry.account.id);
    store.accounts.push(entry);
    write(path, &store)
}

async fn save(
    account: Account,
    key: &str,
    cancelled: Option<&CancellationToken>,
    require_existing: bool,
) -> Result<Account, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    if cancelled.is_some_and(CancellationToken::is_cancelled) {
        return Err("accountError.cancelled".into());
    }
    save_to(
        &store_path()?,
        Saved {
            account: account.clone(),
            key: secret_store::encrypt(key)?,
        },
        require_existing,
    )?;
    Ok(account)
}
pub async fn refresh(id: &str) -> Result<Account, String> {
    let key = credentials(id)?;
    let mut account = details(&key).await?;
    account.active = read_object(&native("auth.json")?)?["apiKey"].as_str() == Some(key.as_str());
    save(account, &key, None, true).await
}
pub async fn remove(id: &str) -> Result<(), String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    let mut store = load()?;
    store.accounts.retain(|s| s.account.id != id);
    write(&store_path()?, &store)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::CommandCode,
        id,
    );
    Ok(())
}
pub async fn apply(id: &str) -> Result<Account, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    let s = saved(id)?;
    let key = credentials(id)?;
    let path = native("auth.json")?;
    let mut auth = read_object(&path)?;
    auth["apiKey"] = key.into();
    auth["userName"] = s.account.email.clone().into();
    let mut transaction = super::native_account_files::Transaction::guarded();
    transaction.replace_private_file(&path, &super::native_account_files::json(&auth)?)?;
    super::tool_config_manager::commandcode::restore()?;
    transaction.commit();
    let mut account = s.account;
    account.active = true;
    Ok(account)
}
async fn callback(
    State(pending): State<Arc<Pending>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(ORIGIN);
    if ![ORIGIN, "https://staging.commandcode.ai"].contains(&origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if pending.cancelled.is_cancelled() {
        return StatusCode::GONE.into_response();
    }
    let input: Value = if headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
    {
        serde_json::from_slice(&body).unwrap_or(Value::Null)
    } else {
        Value::Object(
            url::form_urlencoded::parse(&body)
                .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                .collect(),
        )
    };
    if input["state"] != pending.state {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(key) = input["apiKey"].as_str().filter(|s| !s.trim().is_empty()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    *pending.key.lock().await = Some(key.into());
    let mut response = "You can close this tab.".into_response();
    response.headers_mut().insert(
        "access-control-allow-origin",
        origin.parse().expect("known origin"),
    );
    response
}
async fn options(headers: HeaderMap) -> Response {
    let origin = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(ORIGIN);
    if ![ORIGIN, "https://staging.commandcode.ai"].contains(&origin) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    for (name, value) in [
        ("access-control-allow-origin", origin),
        ("access-control-allow-methods", "POST, OPTIONS"),
        ("access-control-allow-headers", "Content-Type"),
        ("access-control-allow-private-network", "true"),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(name),
            value.parse().expect("known header"),
        );
    }
    response
}
pub async fn start() -> Result<LoginStart, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let state = uuid::Uuid::new_v4().to_string();
    let pending = Arc::new(Pending {
        state: state.clone(),
        key: Mutex::new(None),
        cancelled: CancellationToken::new(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "accountError.failed")?;
    let port = listener
        .local_addr()
        .map_err(|_| "accountError.failed")?
        .port();
    let router = Router::new()
        .route("/callback", post(callback).options(options))
        .layer(axum::extract::DefaultBodyLimit::max(10000))
        .with_state(pending.clone());
    logins().lock().await.insert(id.clone(), pending.clone());
    let cancelled = pending.cancelled.clone();
    let cleanup_id = id.clone();
    tokio::spawn(async move {
        let cancel = cancelled.clone();
        let server = axum::serve(listener, router)
            .with_graceful_shutdown(async move { cancel.cancelled().await });
        tokio::select! { _=server=>{}, _=tokio::time::sleep(Duration::from_secs(600))=>{cancelled.cancel();}}
        logins().lock().await.remove(&cleanup_id);
    });
    let mut url = url::Url::parse("https://commandcode.ai/studio/auth/cli").expect("constant URL");
    url.query_pairs_mut()
        .append_pair("callback", &format!("http://127.0.0.1:{port}/callback"))
        .append_pair("state", &state)
        .append_pair("mode", "redirect");
    Ok(LoginStart {
        login_id: id,
        authorization_url: url.into(),
        expires_at: chrono::Utc::now().timestamp() + 600,
    })
}
pub async fn cancel(id: &str) -> Result<(), String> {
    if let Some(p) = logins().lock().await.remove(id) {
        p.cancelled.cancel();
    }
    Ok(())
}
pub async fn poll(id: &str) -> Result<Option<Account>, String> {
    let pending = logins()
        .lock()
        .await
        .get(id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    let Some(key) = pending.key.lock().await.take() else {
        return Ok(None);
    };
    let account = match details(&key).await {
        Ok(account) => account,
        Err(error) => {
            cancel(id).await?;
            return Err(error);
        }
    };
    if pending.cancelled.is_cancelled() {
        return Err("accountError.cancelled".into());
    }
    let account = save(account, &key, Some(&pending.cancelled), false).await?;
    cancel(id).await?;
    Ok(Some(account))
}
pub async fn models(id: &str) -> Result<Vec<String>, String> {
    let key = credentials(id)?;
    let value = request(&client()?, "/provider/v1/models", &key).await?;
    Ok(value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| match m["supported_endpoints"].as_array() {
            Some(routes) if !routes.is_empty() => routes
                .iter()
                .any(|r| r.as_str().is_some_and(|r| r.contains("/chat/completions"))),
            _ => m["id"]
                .as_str()
                .is_some_and(|id| !id.starts_with("claude-")),
        })
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .collect())
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let path = native("auth.json")?;
    let auth = read_object(&path)?;
    let Some(key) = auth["apiKey"].as_str().filter(|s| !s.trim().is_empty()) else {
        return Ok(None);
    };
    let id = format!("{:x}", Sha256::digest(key.as_bytes()));
    if load()?.accounts.iter().any(|s| s.account.id == id) {
        return Ok(Some(false));
    }
    let user = request(&client()?, "/alpha/whoami", key).await?;
    let email = ["email", "userName", "id"]
        .iter()
        .find_map(|k| user["user"][k].as_str())
        .ok_or("accountError.authResponse")?
        .to_owned();
    let _guard = ACCOUNT_LOCK.lock().await;
    let rows = load()?;
    if rows.accounts.iter().any(|s| {
        super::auto_import::native::same_account(&id, &email, &s.account.id, &s.account.email)
    }) {
        return Ok(Some(false));
    }
    save_to(
        &store_path()?,
        Saved {
            account: Account {
                id,
                email,
                plan: None,
                remaining: None,
                total: None,
                active: false,
            },
            key: secret_store::encrypt(key)?,
        },
        false,
    )?;
    Ok(Some(true))
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keeps_legacy_account_document_and_plan_labels() {
        let store:Store=serde_json::from_value(json!({"accounts":[{"id":"fixture","email":"fixture@example.test","plan":"individual-max-5x","remaining":null,"key":"legacy-fixture"}]})).unwrap();
        assert_eq!(store.accounts[0].key, "legacy-fixture");
        assert_eq!(store.accounts[0].account.remaining, None);
        assert_eq!(plan_name("individual-max-5x"), "Max 5×");
        assert_eq!(plan_name("individual_provider"), "Provider");
        assert_eq!(plan_name("future-plan"), "future-plan");
    }
    #[test]
    fn delayed_refresh_cannot_recreate_a_deleted_account() {
        let directory =
            std::env::temp_dir().join(format!("commandcode-refresh-{}", uuid::Uuid::new_v4()));
        let path = directory.join("accounts.json");
        let entry = Saved {
            account: Account {
                id: "fixture".into(),
                email: "fixture@example.test".into(),
                plan: None,
                remaining: None,
                total: None,
                active: false,
            },
            key: "fixture-key".into(),
        };
        save_to(&path, entry.clone(), false).unwrap();
        let mut refreshed = entry.clone();
        refreshed.account.remaining = Some(42.0);
        save_to(&path, refreshed.clone(), true).unwrap();
        let updated: Store = super::super::json_store::read(&path).unwrap();
        assert_eq!(updated.accounts[0].account.remaining, Some(42.0));
        write(&path, &Store::default()).unwrap();
        assert!(save_to(&path, refreshed, true).is_err());
        let deleted: Store = super::super::json_store::read(&path).unwrap();
        assert!(deleted.accounts.is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn callback_checks_origin_state_and_cancellation() {
        let login = start().await.unwrap();
        let url = url::Url::parse(&login.authorization_url).unwrap();
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let callback = &query["callback"];
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(callback)
            .header("origin", "https://unrelated.example")
            .json(&json!({"state":query["state"],"apiKey":"fixture"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = client
            .post(callback)
            .header("origin", ORIGIN)
            .json(&json!({"state":"incorrect","apiKey":"fixture"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = client
            .post(callback)
            .header("origin", ORIGIN)
            .json(&json!({"state":query["state"],"apiKey":"fixture"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        cancel(&login.login_id).await.unwrap();
        assert!(poll(&login.login_id).await.is_err());
    }
}
