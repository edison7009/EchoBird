mod oauth;
mod quota;
mod signin;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
type RefreshLocks = Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>;
static REFRESH_LOCKS: OnceLock<RefreshLocks> = OnceLock::new();
static ROTATED: OnceLock<Mutex<HashMap<String, PendingRotation>>> = OnceLock::new();

struct PendingRotation {
    previous: Credentials,
    current: Credentials,
}

pub(crate) struct NativeAccountSession {
    pub(crate) credentials: Credentials,
    _action: tokio::sync::OwnedMutexGuard<()>,
    _rotation: tokio::sync::OwnedMutexGuard<()>,
}

fn action_lock(path: &Path, id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    static LOCKS: OnceLock<RefreshLocks> = OnceLock::new();
    Ok(LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "accountError.busy")?
        .entry(cache_key(path, id))
        .or_default()
        .clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    Global,
}

impl Region {
    pub(crate) fn account_host(self) -> &'static str {
        match self {
            Self::Cn => "https://account.minimax.cn",
            Self::Global => "https://account.minimax.io",
        }
    }
    pub(crate) fn agent_host(self) -> &'static str {
        match self {
            Self::Cn => "https://agent.minimaxi.com",
            Self::Global => "https://agent.minimax.io",
        }
    }
    pub(crate) fn platform_host(self) -> &'static str {
        match self {
            Self::Cn => "https://www.minimaxi.com",
            Self::Global => "https://platform.minimax.io",
        }
    }
    pub(crate) fn llm_host(self) -> &'static str {
        match self {
            Self::Cn => "https://agent.minimax.cn",
            Self::Global => "https://agent.minimax.io",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReward {
    pub status: RewardStatus,
    pub checked_at: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RewardStatus {
    Claimable,
    Claimed,
    Unavailable,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub region: Region,
    pub active: bool,
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
    pub subscription_end_at: Option<i64>,
    pub quota: Option<Quota>,
    #[serde(default)]
    pub daily_reward: Option<DailyReward>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub credits: Option<f64>,
    #[serde(default)]
    pub purchased_credits: Option<f64>,
    #[serde(default)]
    pub granted_credits: Option<f64>,
    #[serde(default)]
    pub purchased_total: Option<f64>,
    #[serde(default)]
    pub granted_total: Option<f64>,
    #[serde(default)]
    pub purchased_expires_at: Option<i64>,
    #[serde(default)]
    pub granted_expires_at: Option<i64>,
    pub windows: Vec<QuotaWindow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub name: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
    pub span_seconds: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub user_code: String,
    pub expires_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Credentials {
    pub(crate) region: Region,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: i64,
    pub(crate) user_id: String,
    pub(crate) email: String,
    pub(crate) name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
}
type Store = BTreeMap<String, Saved>;

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)?
        .as_str()
        .filter(|text| !text.trim().is_empty())
}

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/minimax-accounts.json"))
}

fn load(path: &Path) -> Result<Store, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "accountError.format".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Store::new()),
        Err(_) => Err("accountError.read".into()),
    }
}

fn id(credentials: &Credentials) -> Result<String, String> {
    if credentials.user_id.trim().is_empty()
        || credentials.access_token.trim().is_empty()
        || credentials.refresh_token.trim().is_empty()
        || credentials.expires_at <= 0
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("{:?}\0{}", credentials.region, credentials.user_id))
    ))
}

fn account(credentials: &Credentials) -> Result<Account, String> {
    Ok(Account {
        id: id(credentials)?,
        email: credentials.email.clone(),
        name: credentials.name.clone(),
        region: credentials.region,
        active: false,
        plan: None,
        expires_at: Some(credentials.expires_at),
        subscription_end_at: None,
        quota: None,
        daily_reward: None,
    })
}

fn protect(credentials: &Credentials) -> Result<String, String> {
    let raw = serde_json::to_string(credentials).map_err(|_| "accountError.format")?;
    let encrypted = encrypt(&raw);
    if !encrypted.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(encrypted)
}

fn unprotect(saved: &Saved) -> Result<Credentials, String> {
    let encoded = saved
        .credential
        .strip_prefix("enc:v1:")
        .ok_or("accountError.keychain")?;
    if encoded.len() < 56
        || encoded.len() % 2 != 0
        || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("accountError.keychain".into());
    }
    let credentials: Credentials =
        serde_json::from_str(&decrypt(&saved.credential)).map_err(|_| "accountError.keychain")?;
    if id(&credentials)? != saved.account.id || credentials.region != saved.account.region {
        return Err("accountError.invalidAccount".into());
    }
    Ok(credentials)
}

#[cfg(not(test))]
fn encrypt(raw: &str) -> String {
    super::model_manager::encrypt_key_for_storage(raw)
}
#[cfg(not(test))]
fn decrypt(encoded: &str) -> String {
    super::model_manager::decrypt_key_for_use(encoded)
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
tokio::task_local! {
    pub(crate) static LIVE_ACCOUNT_CRYPTO: ();
}

#[cfg(test)]
fn encrypt(raw: &str) -> String {
    if LIVE_ACCOUNT_CRYPTO.try_with(|()| ()).is_ok() {
        return super::model_manager::encrypt_key_for_storage(raw);
    }
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let mut bytes = nonce.to_vec();
    bytes.extend(
        Aes256Gcm::new(&[7u8; 32].into())
            .encrypt(aes_gcm::Nonce::from_slice(&nonce), raw.as_bytes())
            .unwrap(),
    );
    format!("enc:v1:{}", hex::encode(bytes))
}
#[cfg(test)]
fn decrypt(encoded: &str) -> String {
    if LIVE_ACCOUNT_CRYPTO.try_with(|()| ()).is_ok() {
        return super::model_manager::decrypt_key_for_use(encoded);
    }
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    hex::decode(encoded.trim_start_matches("enc:v1:"))
        .ok()
        .filter(|bytes| bytes.len() >= 28)
        .and_then(|bytes| {
            Aes256Gcm::new(&[7u8; 32].into())
                .decrypt(aes_gcm::Nonce::from_slice(&bytes[..12]), &bytes[12..])
                .ok()
        })
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default()
}

fn rotated() -> &'static Mutex<HashMap<String, PendingRotation>> {
    ROTATED.get_or_init(|| Mutex::new(HashMap::new()))
}
fn cache_key(path: &Path, id: &str) -> String {
    format!("{}\0{id}", path.display())
}

fn current(path: &Path, id: &str) -> Result<Credentials, String> {
    let saved = load(path)?
        .remove(id)
        .ok_or("accountError.invalidAccount")?;
    let credentials = unprotect(&saved)?;
    let mut pending = rotated().lock().map_err(|_| "accountError.busy")?;
    if let Some(rotation) = pending.get(&cache_key(path, id)) {
        if rotation.previous == credentials {
            return Ok(rotation.current.clone());
        }
        pending.remove(&cache_key(path, id));
    }
    Ok(credentials)
}

fn capture_at(path: &Path, credentials: &Credentials) -> Result<Account, String> {
    let mut accounts = load(path)?;
    let mut summary = account(credentials)?;
    if let Some(existing) = accounts.get(&summary.id) {
        summary.plan = existing.account.plan.clone();
        summary.quota = existing.account.quota.clone();
        summary.subscription_end_at = existing.account.subscription_end_at;
        summary.active = existing.account.active;
        summary.daily_reward = existing.account.daily_reward.clone();
    }
    accounts.insert(
        summary.id.clone(),
        Saved {
            account: summary.clone(),
            credential: protect(credentials)?,
        },
    );
    super::cursor_auth::write(path, &accounts)?;
    rotated()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&cache_key(path, &summary.id));
    Ok(summary)
}

pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    list_at(&store_path()?)
}
fn list_at(path: &Path) -> Result<Vec<Account>, String> {
    let mut accounts: Vec<_> = load(path)?
        .into_values()
        .map(|saved| saved.account)
        .collect();
    accounts.sort_by(|left, right| left.email.cmp(&right.email).then(left.id.cmp(&right.id)));
    Ok(accounts)
}

pub async fn start_login(region: Region) -> Result<LoginStart, String> {
    oauth::start(region).await
}
pub fn cancel_login(login_id: &str) -> Result<(), String> {
    oauth::cancel(login_id)
}
pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let Some(mut credential) = oauth::poll(login_id)? else {
        return Ok(None);
    };
    let identity = oauth::cancellable(
        login_id,
        quota::identity_at(
            &oauth::client()?,
            credential.region.agent_host(),
            &mut credential,
        ),
    )
    .await;
    if identity.as_ref().err().is_some_and(|error| {
        matches!(
            error.as_str(),
            "accountError.cancelled" | "accountError.expired"
        )
    }) {
        identity?;
    }
    let path = store_path()?;
    let lock = action_lock(&path, &id(&credential)?)?;
    let _action = lock.lock().await;
    let _guard = STORE_LOCK.lock().await;
    let account = oauth::complete(login_id, || capture_at(&path, &credential))?;
    drop(_guard);
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Minimax,
        &account.id,
    );
    Ok(Some(account))
}

pub async fn delete_account(account_id: &str) -> Result<(), String> {
    let path = store_path()?;
    let lock = action_lock(&path, account_id)?;
    let _action = lock.lock().await;
    let _guard = STORE_LOCK.lock().await;
    delete_at(&path, account_id)?;
    drop(_guard);
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Minimax,
        account_id,
    );
    Ok(())
}
pub(crate) async fn mark_active(account_id: &str) -> Result<Account, String> {
    let _guard = STORE_LOCK.lock().await;
    let path = store_path()?;
    mark_active_at(&path, account_id)
}

fn mark_active_at(path: &Path, account_id: &str) -> Result<Account, String> {
    let mut accounts = load(path)?;
    let credential = current(path, account_id)?;
    let saved = accounts
        .get_mut(account_id)
        .ok_or("accountError.invalidAccount")?;
    saved.credential = protect(&credential)?;
    saved.account.expires_at = Some(credential.expires_at);
    for (id, saved) in &mut accounts {
        saved.account.active = id == account_id;
    }
    let selected = accounts[account_id].account.clone();
    super::cursor_auth::write(path, &accounts)?;
    rotated()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&cache_key(path, account_id));
    Ok(selected)
}
fn delete_at(path: &Path, account_id: &str) -> Result<(), String> {
    let mut accounts = load(path)?;
    accounts
        .remove(account_id)
        .ok_or("accountError.invalidAccount")?;
    super::cursor_auth::write(path, &accounts)?;
    rotated()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&cache_key(path, account_id));
    Ok(())
}

async fn credentials_at(
    path: &Path,
    account_id: &str,
    force: bool,
    rejected: Option<&str>,
    host: Option<&str>,
) -> Result<Credentials, String> {
    let observed = current(path, account_id)?;
    let lock = rotation_lock(path, account_id)?;
    let _guard = lock.lock().await;
    credentials_locked_at(path, account_id, force, rejected, host, &observed).await
}

fn rotation_lock(path: &Path, account_id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    Ok(REFRESH_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(|_| "accountError.busy")?
        .entry(cache_key(path, account_id))
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone())
}

async fn credentials_locked_at(
    path: &Path,
    account_id: &str,
    force: bool,
    rejected: Option<&str>,
    host: Option<&str>,
    observed: &Credentials,
) -> Result<Credentials, String> {
    let before = current(path, account_id)?;
    let expected = rejected.unwrap_or(&observed.access_token);
    if before.expires_at > chrono::Utc::now().timestamp() + 120
        && (!force
            || before.access_token != expected
            || before.refresh_token != observed.refresh_token)
    {
        return Ok(before);
    }
    let renewed = oauth::refresh_at(
        &oauth::client()?,
        host.unwrap_or(before.region.account_host()),
        &before,
    )
    .await?;
    let _store_guard = STORE_LOCK.lock().await;
    let after = current(path, account_id)?;
    if after.access_token != before.access_token || after.refresh_token != before.refresh_token {
        return Ok(after);
    }
    let mut accounts = load(path)?;
    let saved = accounts
        .get_mut(account_id)
        .ok_or("accountError.invalidAccount")?;
    // Rotating refresh tokens cannot be reused if persisting their replacement fails.
    rotated().lock().map_err(|_| "accountError.busy")?.insert(
        cache_key(path, account_id),
        PendingRotation {
            previous: unprotect(saved)?,
            current: renewed.clone(),
        },
    );
    saved.credential = protect(&renewed)?;
    saved.account.expires_at = Some(renewed.expires_at);
    if super::cursor_auth::write(path, &accounts).is_ok() {
        rotated()
            .lock()
            .map_err(|_| "accountError.busy")?
            .remove(&cache_key(path, account_id));
    }
    Ok(renewed)
}

pub(crate) async fn credentials_for_native(
    account_id: &str,
) -> Result<NativeAccountSession, String> {
    native_session_at(&store_path()?, account_id).await
}

async fn native_session_at(path: &Path, account_id: &str) -> Result<NativeAccountSession, String> {
    let action = action_lock(path, account_id)?.lock_owned().await;
    let observed = current(path, account_id)?;
    let rotation = rotation_lock(path, account_id)?.lock_owned().await;
    let credentials = credentials_locked_at(path, account_id, false, None, None, &observed).await?;
    Ok(NativeAccountSession {
        credentials,
        _action: action,
        _rotation: rotation,
    })
}

fn proxy_value(credential: &Credentials, account_id: &str) -> Value {
    json!({"accessToken":credential.access_token,"accountId":account_id,"userId":credential.user_id,
        "region":credential.region,"baseUrl":credential.region.llm_host()})
}

// A signed-in client holds the same single-use refresh token: use its newer
// copy, and give it ours after a rotation.
async fn adopt_client(path: &Path, account_id: &str) -> Result<Option<String>, String> {
    let saved = current(path, account_id)?;
    let Some((access, refresh, expires)) = super::minimax_native::client_tokens(&saved) else {
        return Ok(None);
    };
    if refresh != saved.refresh_token && expires > saved.expires_at {
        let _guard = STORE_LOCK.lock().await;
        let mut accounts = load(path)?;
        let entry = accounts
            .get_mut(account_id)
            .ok_or("accountError.invalidAccount")?;
        let mut latest = unprotect(entry)?;
        if latest.refresh_token == saved.refresh_token {
            latest.access_token = access;
            latest.refresh_token = refresh.clone();
            latest.expires_at = expires;
            entry.credential = protect(&latest)?;
            entry.account.expires_at = Some(expires);
            super::cursor_auth::write(path, &accounts)?;
        }
    }
    Ok(Some(refresh))
}

fn share_with_client(client_refresh: Option<String>, credential: &Credentials) {
    if client_refresh.is_some_and(|refresh| refresh != credential.refresh_token) {
        if let Err(error) = super::minimax_native::store_rotation(credential) {
            log::warn!("[MiniMax] Could not update the client login: {error}");
        }
    }
}

pub(crate) async fn proxy_credentials(account_id: &str, force: bool) -> Result<Value, String> {
    let path = store_path()?;
    let client_refresh = adopt_client(&path, account_id).await.unwrap_or(None);
    let credential = credentials_at(&path, account_id, force, None, None).await?;
    share_with_client(client_refresh, &credential);
    Ok(proxy_value(&credential, account_id))
}
pub(crate) async fn credentials_after_rejection(
    account_id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    let rejected = text(rejected, "accessToken").ok_or("accountError.loginRequired")?;
    let path = store_path()?;
    let client_refresh = adopt_client(&path, account_id).await.unwrap_or(None);
    let credential = credentials_at(&path, account_id, true, Some(rejected), None).await?;
    share_with_client(client_refresh, &credential);
    Ok(proxy_value(&credential, account_id))
}

pub async fn refresh_account(account_id: &str) -> Result<Account, String> {
    let path = store_path()?;
    let lock = action_lock(&path, account_id)?;
    let _action = lock.lock().await;
    refresh_account_at(&path, account_id, None).await
}

async fn refresh_account_at(
    path: &Path,
    account_id: &str,
    quota_hosts: Option<(&str, &str)>,
) -> Result<Account, String> {
    let mut credential = credentials_at(path, account_id, false, None, None).await?;
    let client = oauth::client()?;
    let mut summary = load(path)?
        .remove(account_id)
        .ok_or("accountError.invalidAccount")?
        .account;
    let mut result = quota::refresh_at(
        &client,
        quota_hosts
            .map(|hosts| hosts.0)
            .unwrap_or(credential.region.agent_host()),
        quota_hosts
            .map(|hosts| hosts.1)
            .unwrap_or(credential.region.platform_host()),
        quota_hosts
            .map(|hosts| hosts.0)
            .unwrap_or(credential.region.llm_host()),
        &credential,
        &mut summary,
    )
    .await;
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error == "accountError.loginRequired")
    {
        credential =
            credentials_at(path, account_id, true, Some(&credential.access_token), None).await?;
        result = quota::refresh_at(
            &client,
            quota_hosts
                .map(|hosts| hosts.0)
                .unwrap_or(credential.region.agent_host()),
            quota_hosts
                .map(|hosts| hosts.1)
                .unwrap_or(credential.region.platform_host()),
            quota_hosts
                .map(|hosts| hosts.0)
                .unwrap_or(credential.region.llm_host()),
            &credential,
            &mut summary,
        )
        .await;
    }
    result?;
    let _guard = STORE_LOCK.lock().await;
    let mut accounts = load(path)?;
    let saved = accounts
        .get_mut(account_id)
        .ok_or("accountError.invalidAccount")?;
    saved.account.plan = summary.plan;
    saved.account.quota = summary.quota;
    saved.account.subscription_end_at = summary.subscription_end_at;
    let summary = saved.account.clone();
    super::cursor_auth::write(path, &accounts)?;
    Ok(summary)
}

pub async fn claim_daily_credits(account_id: &str) -> Result<Account, String> {
    claim_daily_at(&store_path()?, account_id, None).await
}

async fn claim_daily_at(
    path: &Path,
    account_id: &str,
    hosts: Option<(&str, &str)>,
) -> Result<Account, String> {
    let lock = action_lock(path, account_id)?;
    let _action = lock.lock().await;
    let mut credential = credentials_at(path, account_id, false, None, None).await?;
    let client = oauth::client()?;
    let host = hosts
        .map(|pair| pair.0)
        .unwrap_or(credential.region.llm_host());
    let version = env!("CARGO_PKG_VERSION");
    let mut result = signin::status(&client, host, &credential, version).await;
    if result
        .as_ref()
        .err()
        .is_some_and(|error| error == "accountError.loginRequired")
    {
        credential =
            credentials_at(path, account_id, true, Some(&credential.access_token), None).await?;
        result = signin::status(&client, host, &credential, version).await;
    }
    let eligibility = result?;
    let account = update_reward_at(path, account_id, &credential, eligibility.reward).await?;
    match account.daily_reward.as_ref().map(|reward| reward.status) {
        Some(RewardStatus::Claimed) => return Ok(account),
        Some(RewardStatus::Claimable) => {}
        _ => return Err("accountError.claimUnavailable".into()),
    }
    let claimed = signin::claim(
        &client,
        host,
        &credential,
        version,
        eligibility.day_no.ok_or("accountError.format")?,
    )
    .await?;
    update_reward_at(path, account_id, &credential, claimed).await?;
    let _ = refresh_account_at(path, account_id, hosts).await;
    let _guard = STORE_LOCK.lock().await;
    Ok(load(path)?
        .remove(account_id)
        .ok_or("accountError.invalidAccount")?
        .account)
}

async fn update_reward_at(
    path: &Path,
    account_id: &str,
    credential: &Credentials,
    reward: DailyReward,
) -> Result<Account, String> {
    let _guard = STORE_LOCK.lock().await;
    let latest = current(path, account_id)?;
    if latest != *credential
        && (reward.status != RewardStatus::Claimed
            || latest.region != credential.region
            || latest.user_id != credential.user_id)
    {
        return Err("accountError.invalidAccount".into());
    }
    let mut accounts = load(path)?;
    let saved = accounts
        .get_mut(account_id)
        .ok_or("accountError.invalidAccount")?;
    saved.account.daily_reward = Some(reward);
    saved.credential = protect(&latest)?;
    saved.account.expires_at = Some(latest.expires_at);
    let account = saved.account.clone();
    super::cursor_auth::write(path, &accounts)?;
    rotated()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&cache_key(path, account_id));
    Ok(account)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod signin_tests;

#[cfg(test)]
mod installed_reward_probe {
    use super::*;

    #[tokio::test]
    #[ignore = "Read-only reward status probe using an explicitly selected saved account"]
    async fn saved_minimax_reward_status_probe() {
        let account_id = std::env::var("ECHOBIRD_MINIMAX_REWARD_PROBE_ACCOUNT_ID").unwrap();
        let saved = load(&store_path().unwrap())
            .unwrap()
            .remove(&account_id)
            .unwrap();
        let raw = super::super::model_manager::decrypt_key_for_use(&saved.credential);
        let credential: Credentials = serde_json::from_str(&raw).unwrap();
        assert_eq!(id(&credential).unwrap(), saved.account.id);
        assert_eq!(credential.region, saved.account.region);
        let eligibility = signin::status(
            &oauth::client().unwrap(),
            credential.region.llm_host(),
            &credential,
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .unwrap();
        println!(
            "{}",
            serde_json::json!({"reward":eligibility.reward,"dayNo":eligibility.day_no})
        );
    }
}

#[cfg(test)]
pub(crate) mod test_support;

pub(crate) async fn import_native(
    region: Region,
) -> Result<super::auto_import::native::Counts, String> {
    let candidates = super::minimax_native::discover(region);
    let _guard = STORE_LOCK.lock().await;
    let path = store_path()?;
    let mut rows = load(&path)?;
    let mut counts = super::auto_import::native::Counts::default();
    for c in candidates {
        counts.record((|| {
            let account = account(&c)?;
            if rows.values().any(|s| {
                super::auto_import::native::same_account(
                    &account.id,
                    &account.email,
                    &s.account.id,
                    &s.account.email,
                )
            }) {
                return Ok(Some(false));
            }
            let saved = Saved {
                account: account.clone(),
                credential: protect(&c)?,
            };
            // Write a candidate copy first, so a failed save cannot count as an existing row.
            let mut next = rows.clone();
            next.insert(account.id, saved);
            super::cursor_auth::write(&path, &next)?;
            rows = next;
            Ok(Some(true))
        })());
    }
    Ok(counts)
}
