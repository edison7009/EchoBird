mod api;
mod login;
mod native;
mod proxy;
mod voice;
pub use login::LoginStart;
pub(crate) use proxy::{proxy_credentials, proxy_credentials_after_rejection};
pub(crate) use voice::voice_credentials;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditBucket {
    pub name: String,
    pub source: String,
    pub reward: bool,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReward {
    pub status: String,
    pub checked_at: i64,
    pub next_claim_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub enterprise: bool,
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
    pub active: bool,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub credit_buckets: Vec<CreditBucket>,
    pub daily_reward: Option<DailyReward>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Credentials {
    profile: Value,
    cookies: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gateway_token: Option<proxy::GatewayToken>,
}
impl Credentials {
    fn validate(&self) -> Result<(), String> {
        if api::text(&self.profile, "bceUserId").is_none()
            || !matches!(
                api::text(&self.profile, "loginType"),
                Some("bce" | "enterprise")
            )
            || self.cookies.is_empty()
        {
            return Err("accountError.invalidAccount".into());
        }
        if self.enterprise() {
            api::enterprise_origin(&self.profile)?;
        } else if api::text(&self.profile, "bceAccountId").is_none() {
            return Err("accountError.invalidAccount".into());
        }
        Ok(())
    }
    fn enterprise(&self) -> bool {
        self.profile["loginType"] == "enterprise"
    }
    fn id(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}",
                self.profile["loginType"],
                if self.enterprise() {
                    self.profile["enterpriseApiBaseUrl"].as_str().unwrap_or("")
                } else {
                    self.profile["bceAccountId"].as_str().unwrap_or("")
                },
                self.profile["bceUserId"].as_str().unwrap_or("")
            ))
        )
    }
    fn summary(&self) -> Account {
        Account {
            id: self.id(),
            name: api::text(&self.profile, "displayName")
                .or_else(|| api::text(&self.profile, "bceUserId"))
                .unwrap_or_default()
                .into(),
            enterprise: self.enterprise(),
            plan: self.enterprise().then(|| "team".into()),
            expires_at: None,
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
fn path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/dumate-accounts.json"))
}
fn load() -> Result<Store, String> {
    load_at(&path()?)
}
fn load_at(path: &std::path::Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    super::cursor_auth::read(path)
}
fn save(store: &Store) -> Result<(), String> {
    super::cursor_auth::write(&path()?, store)
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
fn credential(store: &Store, id: &str) -> Result<Credentials, String> {
    let saved = store
        .get(id)
        .filter(|s| s.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    if !saved.credential.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    let c: Credentials = serde_json::from_str(&super::model_manager::decrypt_key_for_use(
        &saved.credential,
    ))
    .map_err(|_| "accountError.keychain")?;
    c.validate()?;
    if c.id() != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(c)
}
async fn credentials(id: &str) -> Result<Credentials, String> {
    let _guard = STORE_LOCK.lock().await;
    credential(&load()?, id)
}
fn request_lock(id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    static LOCKS: OnceLock<Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    Ok(LOCKS
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| "accountError.read")?
        .entry(id.into())
        .or_default()
        .clone())
}
pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = STORE_LOCK.lock().await;
    Ok(load()?.into_values().map(|s| s.account).collect())
}
pub async fn start_login(app: tauri::AppHandle) -> Result<LoginStart, String> {
    login::start(app).await
}
pub fn cancel_login(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    login::cancel(app, id)
}
pub async fn poll_login(app: &tauri::AppHandle, id: &str) -> Result<Option<Account>, String> {
    let Some((c, mut account)) = login::poll(app, id).await? else {
        return Ok(None);
    };
    let request = request_lock(&c.id())?;
    let _request = request.lock().await;
    let _guard = STORE_LOCK.lock().await;
    login::complete(app, id, || {
        let mut store = load()?;
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
        save(&store)?;
        Ok(Some(account))
    })
}
pub async fn delete(id: &str) -> Result<(), String> {
    let request = request_lock(id)?;
    let _request = request.lock().await;
    let _guard = STORE_LOCK.lock().await;
    let mut store = load()?;
    if store.remove(id).is_none() {
        return Err("accountError.invalidAccount".into());
    }
    save(&store)
}
async fn update(
    id: &str,
    quota: Option<api::Quota>,
    daily: Option<DailyReward>,
) -> Result<Account, String> {
    let _guard = STORE_LOCK.lock().await;
    let mut store = load()?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    if let Some(quota) = quota {
        quota.apply(&mut saved.account);
    }
    saved.account.daily_reward = daily;
    let account = saved.account.clone();
    save(&store)?;
    Ok(account)
}
pub async fn refresh(id: &str) -> Result<Account, String> {
    let request = request_lock(id)?;
    let _request = request.lock().await;
    let c = credentials(id).await?;
    let client = api::Client::authenticated(&c)?;
    let quota = client.quota(&c).await?;
    let daily = if c.enterprise() {
        None
    } else {
        client.daily(&c).await.ok()
    };
    update(id, Some(quota), daily).await
}
pub async fn claim(id: &str) -> Result<Account, String> {
    let request = request_lock(id)?;
    let _request = request.lock().await;
    let c = credentials(id).await?;
    let client = api::Client::authenticated(&c)?;
    let daily = client.claim(&c).await?;
    // A successful claim stays visible even if the subsequent balance read fails.
    update(id, client.quota(&c).await.ok(), Some(daily)).await
}
pub async fn apply(id: &str) -> Result<Account, String> {
    let request = request_lock(id)?;
    let _request = request.lock().await;
    let c = credentials(id).await?;
    api::Client::installed(&c)?.verify(&c).await?;
    let transaction = native::apply(&c).await?;
    let _guard = STORE_LOCK.lock().await;
    let mut store = load()?;
    credential(&store, id)?;
    for saved in store.values_mut() {
        saved.account.active = saved.account.id == id;
    }
    let account = store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone();
    save(&store)?;
    transaction.commit();
    Ok(account)
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let _guard = STORE_LOCK.lock().await;
    let Some(c) = native::current()? else {
        return Ok(None);
    };
    c.validate()?;
    let account = c.summary();
    let mut rows = load()?;
    if rows.values().any(|s| {
        super::auto_import::native::same_account(
            &account.id,
            &account.name,
            &s.account.id,
            &s.account.name,
        )
    }) {
        return Ok(Some(false));
    }
    rows.insert(
        account.id.clone(),
        Saved {
            account,
            credential: protect(&c)?,
        },
    );
    save(&rows)?;
    Ok(Some(true))
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod tests;
