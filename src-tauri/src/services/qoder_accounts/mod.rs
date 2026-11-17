mod campaigns;
mod oauth;
mod quota;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
fn request_lock(
    edition: Edition,
    id: &str,
) -> Result<std::sync::Arc<tokio::sync::Mutex<()>>, String> {
    static LOCKS: OnceLock<Mutex<BTreeMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    Ok(LOCKS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "accountError.read")?
        .entry(format!("{}:{id}", edition.tool()))
        .or_default()
        .clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    Qoder,
    QoderCn,
}

impl Edition {
    pub(crate) fn tool(self) -> &'static str {
        match self {
            Self::Qoder => "qoder",
            Self::QoderCn => "qodercn",
        }
    }
    pub(crate) fn openapi(self) -> &'static str {
        match self {
            Self::Qoder => "https://openapi.qoder.sh",
            Self::QoderCn => "https://openapi.qoder.com.cn",
        }
    }
    pub(crate) fn gateway(self) -> &'static str {
        match self {
            Self::Qoder => "https://api3.qoder.sh",
            Self::QoderCn => "https://gateway.qoder.com.cn",
        }
    }
    fn web(self) -> &'static str {
        match self {
            Self::Qoder => "https://qoder.com",
            Self::QoderCn => "https://qoder.cn",
        }
    }
    fn client_id(self) -> &'static str {
        match self {
            Self::Qoder => "732aef47-9cf2-46a2-95fe-4cebb5d0d1fa",
            Self::QoderCn => "732aef47-9cf2-46a2-95fe-4cebb5d0d1fa",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub name: String,
    pub total: Option<f64>,
    pub remaining: Option<f64>,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyReward {
    pub status: RewardStatus,
    pub checked_at: i64,
    pub reset_at: i64,
    #[serde(default)]
    pub verified: bool,
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
    pub edition: Edition,
    pub name: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub active: bool,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub daily_reward: Option<DailyReward>,
    #[serde(default)]
    pub quota_windows: Vec<QuotaWindow>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Credentials {
    pub edition: Edition,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub machine_id: String,
    pub device_token: String,
    pub device_refresh: String,
    pub device_expires_at: i64,
    pub device_chat: bool,
    #[serde(default)]
    pub native_device_only: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
}
type Store = BTreeMap<String, Saved>;

pub(crate) struct NativeAccountSession {
    pub(crate) credentials: Credentials,
    _request: tokio::sync::OwnedMutexGuard<()>,
}

fn store_path(edition: Edition) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(format!(".echobird/{}-accounts.json", edition.tool())))
}
fn load_at(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    let mut store: Store = super::cursor_auth::read(path)?;
    for saved in store.values_mut() {
        if saved
            .account
            .daily_reward
            .as_ref()
            .is_some_and(|reward| !reward.verified)
        {
            saved.account.daily_reward = None;
        }
        saved.account.expires_at = quota::normalize_expiry(saved.account.expires_at);
        for window in &mut saved.account.quota_windows {
            window.expires_at = quota::normalize_expiry(window.expires_at);
        }
    }
    Ok(store)
}
fn protect(credentials: &Credentials) -> Result<String, String> {
    let raw = serde_json::to_string(credentials).map_err(|_| "accountError.format")?;
    let encrypted = super::model_manager::encrypt_key_for_storage(&raw);
    if !encrypted.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(encrypted)
}
fn unprotect(saved: &Saved) -> Result<Credentials, String> {
    saved
        .credential
        .strip_prefix("enc:v1:")
        .filter(|s| s.len() >= 56 && s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("accountError.keychain")?;
    let credentials: Credentials = serde_json::from_str(
        &super::model_manager::decrypt_key_for_use(&saved.credential),
    )
    .map_err(|_| "accountError.keychain")?;
    if credentials.edition != saved.account.edition || account_id(&credentials) != saved.account.id
    {
        return Err("accountError.invalidAccount".into());
    }
    validate(&credentials)?;
    Ok(credentials)
}
fn validate(c: &Credentials) -> Result<(), String> {
    if !c.native_device_only
        && (c.access_token.trim().is_empty() || c.refresh_token.trim().is_empty())
    {
        return Err("accountError.invalidAccount".into());
    }
    if [
        &c.user_id,
        &c.machine_id,
        &c.device_token,
        &c.device_refresh,
    ]
    .iter()
    .any(|s| s.trim().is_empty())
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(())
}
fn account_id(c: &Credentials) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}\0{}", c.edition.tool(), c.user_id))
    )
}
fn summary(c: &Credentials) -> Account {
    Account {
        id: account_id(c),
        edition: c.edition,
        name: if !c.email.is_empty() {
            c.email.clone()
        } else if !c.name.is_empty() {
            c.name.clone()
        } else {
            c.user_id.clone()
        },
        email: (!c.email.is_empty()).then(|| c.email.clone()),
        plan: None,
        active: false,
        remaining: None,
        total: None,
        expires_at: None,
        daily_reward: None,
        quota_windows: vec![],
    }
}
fn selected(store: &Store, edition: Edition, id: &str) -> Result<Credentials, String> {
    unprotect(
        store
            .get(id)
            .filter(|s| s.account.edition == edition && s.account.id == id)
            .ok_or("accountError.invalidAccount")?,
    )
}

pub async fn list_saved_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    Ok(load_at(&store_path(edition)?)?
        .into_values()
        .filter(|s| s.account.edition == edition)
        .map(|s| s.account)
        .collect())
}
pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
    oauth::start(edition).await
}
pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let Some(c) = oauth::poll(login_id)? else {
        return Ok(None);
    };
    validate(&c)?;
    let lock = request_lock(c.edition, &account_id(&c))?;
    let _request = lock.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    let account = oauth::complete(login_id, || {
        let path = store_path(c.edition)?;
        let mut store = load_at(&path)?;
        let account = store
            .get(&account_id(&c))
            .map(|s| s.account.clone())
            .unwrap_or_else(|| summary(&c));
        store.insert(
            account.id.clone(),
            Saved {
                account: account.clone(),
                credential: protect(&c)?,
            },
        );
        super::cursor_auth::write(&path, &store)?;
        Ok(account)
    })?;
    forget_authentication(c.edition, &account.id);
    Ok(Some(account))
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    oauth::cancel(id)
}
pub async fn delete_account(edition: Edition, id: &str) -> Result<(), String> {
    let lock = request_lock(edition, id)?;
    let _request = lock.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path(edition)?;
    let mut store = load_at(&path)?;
    store
        .remove(id)
        .filter(|s| s.account.edition == edition)
        .ok_or("accountError.invalidAccount")?;
    super::cursor_auth::write(&path, &store)?;
    forget_authentication(edition, id);
    Ok(())
}
pub(crate) async fn saved_credentials(edition: Edition, id: &str) -> Result<Credentials, String> {
    saved_credentials_at(&store_path(edition)?, edition, id).await
}
async fn saved_credentials_at(
    path: &Path,
    edition: Edition,
    id: &str,
) -> Result<Credentials, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    selected(&load_at(path)?, edition, id)
}
pub(crate) async fn credentials_for_native(
    edition: Edition,
    id: &str,
) -> Result<NativeAccountSession, String> {
    native_session_at(&store_path(edition)?, edition, id, edition.openapi()).await
}
async fn native_session_at(
    path: &Path,
    edition: Edition,
    id: &str,
    base: &str,
) -> Result<NativeAccountSession, String> {
    let request = request_lock(edition, id)?.lock_owned().await;
    let previous = saved_credentials_at(path, edition, id).await?;
    let incoming = rotate_at(&previous, true, false, base).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let current = save_rotation(&mut store, id, &previous, &incoming)?;
    if current != previous {
        super::cursor_auth::write(path, &store)?;
        rotations()
            .lock()
            .map_err(|_| "accountError.write")?
            .remove(&rotation_key(&previous, true));
    }
    Ok(NativeAccountSession {
        credentials: current,
        _request: request,
    })
}
pub(crate) async fn mark_active(edition: Edition, id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path(edition)?;
    let mut store = load_at(&path)?;
    selected(&store, edition, id)?;
    for saved in store.values_mut() {
        saved.account.active = saved.account.id == id;
    }
    let account = store[id].account.clone();
    super::cursor_auth::write(&path, &store)?;
    Ok(account)
}

fn rotations() -> &'static Mutex<BTreeMap<String, Credentials>> {
    static ROTATIONS: OnceLock<Mutex<BTreeMap<String, Credentials>>> = OnceLock::new();
    ROTATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn rotation_key(c: &Credentials, device: bool) -> String {
    format!(
        "{}:{device}:{:x}",
        account_id(c),
        Sha256::digest(if device {
            &c.device_refresh
        } else {
            &c.refresh_token
        })
    )
}
async fn rotate_at(
    c: &Credentials,
    device: bool,
    force: bool,
    base: &str,
) -> Result<Credentials, String> {
    let key = rotation_key(c, device);
    if let Some(next) = rotations()
        .lock()
        .map_err(|_| "accountError.read")?
        .get(&key)
        .cloned()
    {
        return Ok(next);
    }
    let expires = if device {
        c.device_expires_at
    } else {
        c.expires_at
    };
    if !force && expires > chrono::Utc::now().timestamp_millis() + 300_000 {
        return Ok(c.clone());
    }
    let next = oauth::refresh_at(&client()?, c, base, device).await?;
    rotations()
        .lock()
        .map_err(|_| "accountError.write")?
        .insert(key, next.clone());
    Ok(next)
}
fn save_rotation(
    store: &mut Store,
    id: &str,
    previous: &Credentials,
    incoming: &Credentials,
) -> Result<Credentials, String> {
    let current = selected(store, previous.edition, id)?;
    if &current != previous {
        return Ok(current);
    }
    if incoming != previous {
        store
            .get_mut(id)
            .ok_or("accountError.invalidAccount")?
            .credential = protect(incoming)?;
    }
    Ok(incoming.clone())
}
fn forget_authentication(edition: Edition, id: &str) {
    super::smart_router::forget_account_authentication(
        match edition {
            Edition::Qoder => crate::models::model::AccountProvider::Qoder,
            Edition::QoderCn => crate::models::model::AccountProvider::QoderCn,
        },
        id,
    );
}
pub(crate) async fn proxy_credentials(
    edition: Edition,
    id: &str,
    force: bool,
) -> Result<Value, String> {
    let rejected = if force {
        Some(saved_credentials(edition, id).await?.access_token)
    } else {
        None
    };
    proxy_credentials_inner(edition, id, rejected.as_deref()).await
}
pub(crate) async fn proxy_credentials_after_rejection(
    edition: Edition,
    id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    let access = rejected["accessToken"]
        .as_str()
        .ok_or("accountError.invalidAccount")?;
    proxy_credentials_inner(edition, id, Some(access)).await
}

pub(crate) async fn voice_credentials(
    edition: Edition,
    id: &str,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    let executable =
        super::tool_manager::get_tool_exe_path(edition.tool()).ok_or("accountError.unavailable")?;
    if super::qoder_native::desktop::identity(Path::new(&executable))
        != Some((edition, super::qoder_native::desktop::ProductKind::App))
    {
        return Err("accountError.unavailable".into());
    }
    let metadata = super::tool_manager::packaged_desktop_metadata(Path::new(&executable))
        .ok_or("accountError.unavailable")?;
    let version = metadata["version"]
        .as_str()
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 96
                && value.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .ok_or("accountError.unavailable")?;
    voice_credentials_at(
        &store_path(edition)?,
        edition,
        id,
        rejected,
        edition.openapi(),
        version,
    )
    .await
}

async fn voice_credentials_at(
    path: &Path,
    edition: Edition,
    id: &str,
    rejected: Option<&Value>,
    base: &str,
    app_version: &str,
) -> Result<Value, String> {
    let rejected = rejected
        .map(|value| {
            value["deviceToken"]
                .as_str()
                .filter(|token| !token.is_empty())
                .ok_or("accountError.invalidAccount")
        })
        .transpose()?;
    let lock = request_lock(edition, id)?;
    let _request = lock.lock().await;
    let previous = saved_credentials_at(path, edition, id).await?;
    let force = rejected.is_some_and(|token| token == previous.device_token);
    let incoming = rotate_at(&previous, true, force, base).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let current = save_rotation(&mut store, id, &previous, &incoming)?;
    if current != previous {
        super::cursor_auth::write(path, &store)?;
        rotations()
            .lock()
            .map_err(|_| "accountError.write")?
            .remove(&rotation_key(&previous, true));
    }
    Ok(
        serde_json::json!({"edition":current.edition,"deviceToken":current.device_token,"userId":current.user_id,"name":current.name,"email":current.email,"machineId":current.machine_id,"appVersion":app_version}),
    )
}

/// A running client signed in to this account rotates the device token pair
/// itself; spending it here would sign the client out. Returns the error to
/// report when the current token cannot be used, or None to go on.
fn client_owns_rotation(
    c: &Credentials,
    active: bool,
    running: bool,
    rejected: Option<&str>,
    now_ms: i64,
) -> Option<&'static str> {
    if !((c.device_chat && active || c.native_device_only) && running) {
        return None;
    }
    let expired = c.device_expires_at <= now_ms + 300_000;
    let refused = rejected.is_some_and(|token| token == c.access_token);
    (expired || refused).then_some("accountError.clientOwnsLogin")
}

async fn proxy_credentials_inner(
    edition: Edition,
    id: &str,
    rejected: Option<&str>,
) -> Result<Value, String> {
    let path = store_path(edition)?;
    let (current, active) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        let store = load_at(&path)?;
        let active = store.get(id).is_some_and(|saved| saved.account.active);
        (selected(&store, edition, id)?, active)
    };
    let running = (current.device_chat && active || current.native_device_only)
        && super::process_manager::desktop_tool_is_running(edition.tool())
            .await
            .unwrap_or(false);
    if running {
        if let Some(error) = client_owns_rotation(
            &current,
            active,
            running,
            rejected,
            chrono::Utc::now().timestamp_millis(),
        ) {
            return Err(error.into());
        }
        return proxy_credentials_at(&path, edition, id, None, edition.openapi()).await;
    }
    proxy_credentials_at(
        &store_path(edition)?,
        edition,
        id,
        rejected,
        edition.openapi(),
    )
    .await
}
async fn proxy_credentials_at(
    path: &Path,
    edition: Edition,
    id: &str,
    rejected: Option<&str>,
    base: &str,
) -> Result<Value, String> {
    let lock = request_lock(edition, id)?;
    let _request = lock.lock().await;
    let previous = saved_credentials_at(path, edition, id).await?;
    let force = rejected.is_some_and(|token| token == previous.access_token);
    let incoming = if previous.native_device_only {
        oauth::exchange_native(&client()?, &previous, base).await?
    } else {
        rotate_at(&previous, previous.device_chat, force, base).await?
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let current = save_rotation(&mut store, id, &previous, &incoming)?;
    if current != previous {
        super::cursor_auth::write(path, &store)?;
        rotations()
            .lock()
            .map_err(|_| "accountError.write")?
            .remove(&rotation_key(&previous, previous.device_chat));
    }
    Ok(
        serde_json::json!({"edition":current.edition,"accessToken":current.access_token,"userId":current.user_id,"name":current.name,"email":current.email,"machineId":current.machine_id}),
    )
}
pub async fn refresh_quota(edition: Edition, id: &str) -> Result<Account, String> {
    let lock = request_lock(edition, id)?;
    let _request = lock.lock().await;
    refresh_quota_at(edition, id, &store_path(edition)?, edition.openapi()).await
}
async fn refresh_quota_at(
    edition: Edition,
    id: &str,
    path: &Path,
    base: &str,
) -> Result<Account, String> {
    let previous = saved_credentials_at(path, edition, id).await?;
    let mut incoming = rotate_at(&previous, true, false, base).await?;
    let client = client()?;
    let mut result = quota::fetch(&client, base, &incoming.device_token).await;
    if matches!(result, Err(quota::Failure::Expired)) {
        incoming = rotate_at(&incoming, true, true, base).await?;
        result = quota::fetch(&client, base, &incoming.device_token).await;
    }
    let reward = if result.is_ok() {
        match campaigns::RequestIdentity::load(edition, &incoming.user_id, &incoming.machine_id)
            .await
        {
            Ok(identity) => {
                campaigns::fetch(&client, base, &incoming.device_token, edition, &identity)
                    .await
                    .ok()
                    .map(|eligibility| eligibility.reward)
            }
            Err(_) => None,
        }
    } else {
        None
    };
    let current = commit_rotation_at(path, id, &previous, &incoming).await?;
    let quota = result.map_err(|e| match e {
        quota::Failure::Expired if !current.device_chat => {
            "accountError.network|Qoder account-page authorization expired".into()
        }
        e => e.message(),
    })?;
    update_account_at(path, id, &incoming, Some(quota), reward).await
}

async fn commit_rotation_at(
    path: &Path,
    id: &str,
    previous: &Credentials,
    incoming: &Credentials,
) -> Result<Credentials, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    let current = save_rotation(&mut store, id, previous, incoming)?;
    if &current != incoming {
        return Err("accountError.invalidAccount".into());
    }
    if &current != previous {
        super::cursor_auth::write(path, &store)?;
        rotations()
            .lock()
            .map_err(|_| "accountError.write")?
            .remove(&rotation_key(previous, true));
    }
    Ok(current)
}

async fn update_account_at(
    path: &Path,
    id: &str,
    credentials: &Credentials,
    quota: Option<quota::Quota>,
    reward: Option<DailyReward>,
) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_at(path)?;
    if selected(&store, credentials.edition, id)? != *credentials {
        return Err("accountError.invalidAccount".into());
    }
    let changed = quota.is_some() || reward.is_some();
    let account = &mut store
        .get_mut(id)
        .ok_or("accountError.invalidAccount")?
        .account;
    if let Some(quota) = quota {
        quota.apply(account);
    }
    if let Some(reward) = reward {
        account.daily_reward = Some(reward);
    }
    let account = account.clone();
    if changed {
        super::cursor_auth::write(path, &store)?;
    }
    Ok(account)
}

pub async fn claim_daily_credits(edition: Edition, id: &str) -> Result<Account, String> {
    claim_daily_at(edition, id, &store_path(edition)?, edition.openapi(), None).await
}
async fn claim_daily_at(
    edition: Edition,
    id: &str,
    path: &Path,
    base: &str,
    identity: Option<&campaigns::RequestIdentity>,
) -> Result<Account, String> {
    let lock = request_lock(edition, id)?;
    let _request = lock.lock().await;
    let previous = saved_credentials_at(path, edition, id).await?;
    let native_identity;
    let identity = match identity {
        Some(identity) => identity,
        None => {
            native_identity =
                campaigns::RequestIdentity::load(edition, &previous.user_id, &previous.machine_id)
                    .await?;
            &native_identity
        }
    };
    let mut incoming = rotate_at(&previous, true, false, base).await?;
    let client = client()?;
    let mut result =
        campaigns::fetch(&client, base, &incoming.device_token, edition, identity).await;
    if matches!(result, Err(quota::Failure::Expired)) {
        incoming = rotate_at(&incoming, true, true, base).await?;
        result = campaigns::fetch(&client, base, &incoming.device_token, edition, identity).await;
    }
    commit_rotation_at(path, id, &previous, &incoming).await?;
    let eligibility = result.map_err(quota::Failure::message)?;
    let mut reward = eligibility.reward;
    let account = update_account_at(path, id, &incoming, None, Some(reward.clone())).await?;
    match reward.status {
        RewardStatus::Claimed => return Ok(account),
        RewardStatus::Unavailable => return Err("accountError.claimUnavailable".into()),
        RewardStatus::Claimable => {}
    }
    campaigns::claim(
        &client,
        base,
        &incoming.device_token,
        eligibility
            .campaign_id
            .as_deref()
            .ok_or("accountError.format")?,
        identity,
    )
    .await
    .map_err(quota::Failure::message)?;
    reward.status = RewardStatus::Claimed;
    update_account_at(path, id, &incoming, None, Some(reward)).await?;
    let quota = quota::fetch(&client, base, &incoming.device_token)
        .await
        .ok();
    update_account_at(path, id, &incoming, quota, None).await
}
pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|_| "accountError.network".into())
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod daily_tests;

#[cfg(test)]
mod client_rotation_tests {
    use super::*;

    #[test]
    fn a_running_client_keeps_its_device_tokens() {
        let now = 1_000_000_000;
        let mut c: Credentials = serde_json::from_value(serde_json::json!({
            "edition": "qodercn", "accessToken": "device", "refreshToken": "r",
            "expiresAt": 0, "userId": "u", "name": "n", "email": "e", "machineId": "m",
            "deviceToken": "device", "deviceRefresh": "dr",
            "deviceExpiresAt": now + 3_600_000, "deviceChat": true
        }))
        .unwrap();
        assert_eq!(client_owns_rotation(&c, true, true, None, now), None);
        assert_eq!(
            client_owns_rotation(&c, true, true, Some("device"), now),
            Some("accountError.clientOwnsLogin")
        );
        assert_eq!(
            client_owns_rotation(&c, true, false, Some("device"), now),
            None
        );
        assert_eq!(
            client_owns_rotation(&c, false, true, Some("device"), now),
            None
        );
        c.device_expires_at = now;
        assert_eq!(
            client_owns_rotation(&c, true, true, None, now),
            Some("accountError.clientOwnsLogin")
        );
        c.device_chat = false;
        assert_eq!(client_owns_rotation(&c, true, true, None, now), None);
    }
}

pub(crate) async fn import_native(edition: Edition) -> Result<Option<bool>, String> {
    let Some(c) = super::qoder_native::discover(edition)? else {
        return Ok(None);
    };
    validate(&c)?;
    let account = summary(&c);
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path(edition)?;
    let mut rows = load_at(&path)?;
    if rows.values().any(|s| {
        super::auto_import::native::same_account(
            &account.id,
            account.email.as_deref().unwrap_or_default(),
            &s.account.id,
            s.account.email.as_deref().unwrap_or_default(),
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
    super::cursor_auth::write(&path, &rows)?;
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Read-only campaign metadata probe using an explicitly selected saved account"]
    async fn installed_account_campaign_metadata_probe() {
        let id = std::env::var("ECHOBIRD_QODER_CAMPAIGN_PROBE_ACCOUNT_ID").unwrap();
        let edition = match std::env::var("ECHOBIRD_QODER_CAMPAIGN_PROBE_EDITION").as_deref() {
            Ok("qoder") => Edition::Qoder,
            Ok("qodercn") | Err(_) => Edition::QoderCn,
            _ => panic!("Unknown probe edition"),
        };
        let credentials = saved_credentials(edition, &id).await.unwrap();
        let identity = campaigns::RequestIdentity::load(
            edition,
            &credentials.user_id,
            &credentials.machine_id,
        )
        .await
        .unwrap();
        println!(
            "Campaign identity header fields: {:?}",
            identity
                .headers
                .keys()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
        );
        let response = client()
            .unwrap()
            .get(format!(
                "{}/sash/api/v1/me/campaigns",
                credentials.edition.openapi()
            ))
            .bearer_auth(&credentials.device_token)
            .header("Cosy-ClientType", "10")
            .header("Accept", "application/json")
            .header("User-Agent", "Qoder")
            .headers(identity.headers)
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "Campaign metadata HTTP {}",
            response.status()
        );
        let document: Value = response.json().await.unwrap();
        let campaigns = document["campaigns"]
            .as_array()
            .expect("Campaign list envelope");
        for campaign in campaigns {
            let links: Vec<_> = campaign["placements"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|placement| {
                    ["zh", "en"].map(|lang| &placement["content"][lang]["detailUrl"])
                })
                .filter_map(Value::as_str)
                .filter(|link| {
                    reqwest::Url::parse(link).is_ok_and(|url| {
                        url.scheme() == "https"
                            && matches!(url.host_str(), Some("docs.qoder.com" | "docs.qoder.cn"))
                            && url.path().ends_with("/events/100credits")
                            && url.query().is_none()
                            && url.fragment().is_none()
                            && url.username().is_empty()
                            && url.password().is_none()
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "actionType":campaign["actionType"],"claimStatus":campaign["claimStatus"],
                    "startAt":campaign["startAt"],"endAt":campaign["endAt"],
                    "amount":campaign["benefit"]["amount"],
                    "kind":campaign["benefit"]["kind"],
                    "validity":campaign["benefit"]["validity"],"dailyLinks":links,
                    "fieldNames":campaign.as_object().unwrap().keys().collect::<Vec<_>>()
                })
            );
        }
    }
    #[test]
    fn editions_and_public_accounts_are_isolated_and_secret_free() {
        let mut c = Credentials {
            edition: Edition::Qoder,
            access_token: "access-secret".into(),
            refresh_token: "refresh-secret".into(),
            expires_at: 100,
            user_id: "user".into(),
            name: "name".into(),
            email: "mail".into(),
            machine_id: "machine".into(),
            device_token: "device-secret".into(),
            device_refresh: "device-refresh".into(),
            device_expires_at: 200,
            device_chat: false,
            native_device_only: false,
        };
        let international = summary(&c);
        c.edition = Edition::QoderCn;
        assert_ne!(international.id, summary(&c).id);
        assert_eq!(serde_json::to_value(c.edition).unwrap(), "qodercn");
        let public = serde_json::to_string(&international).unwrap();
        assert!(!public.contains("secret"));
        assert!(!public.contains("machine"));
        assert_eq!(international.total, None);
        assert_eq!(international.remaining, None);
    }
    #[test]
    fn saved_listing_does_not_need_a_decryptable_credential() {
        let path = std::env::temp_dir().join(format!("qoder-{}.json", uuid::Uuid::new_v4()));
        let account = Account {
            id: "saved".into(),
            edition: Edition::Qoder,
            name: "saved".into(),
            email: None,
            plan: None,
            active: false,
            remaining: None,
            total: None,
            expires_at: None,
            daily_reward: None,
            quota_windows: vec![],
        };
        let store: Store = BTreeMap::from([(
            "saved".into(),
            Saved {
                account,
                credential: "unreadable".into(),
            },
        )]);
        super::super::cursor_auth::write(&path, &store).unwrap();
        assert_eq!(load_at(&path).unwrap()["saved"].account.name, "saved");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cached_public_quota_is_normalized_without_writing_or_decrypting_credentials() {
        let path = std::env::temp_dir().join(format!("qoder-cache-{}.json", uuid::Uuid::new_v4()));
        let original = serde_json::to_vec(&serde_json::json!({"saved":{
            "credential":"unreadable",
            "account":{"id":"saved","edition":"qodercn","name":"Fixture","email":null,
                "plan":"personal_standard","active":true,"remaining":300,"total":300,
                "expiresAt":253_402_214_400i64,"quotaWindows":[
                    {"name":"Credits","total":0,"remaining":0},
                    {"name":"Add-on","total":300,"remaining":300,"expiresAt":253_402_214_400i64},
                    {"name":"Fixture","total":10,"remaining":10,"expiresAt":1_893_456_000}
                ]}
        }}))
        .unwrap();
        std::fs::write(&path, &original).unwrap();
        let store = load_at(&path).unwrap();
        let saved = &store["saved"];
        assert_eq!(saved.credential, "unreadable");
        assert_eq!(saved.account.remaining, Some(300.0));
        assert_eq!(saved.account.expires_at, None);
        assert_eq!(saved.account.quota_windows[0].expires_at, None);
        assert_eq!(saved.account.quota_windows[1].expires_at, None);
        assert_eq!(
            saved.account.quota_windows[2].expires_at,
            Some(1_893_456_000)
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn rotating_a_single_use_token_is_serialized_and_deleted_rows_stay_deleted() {
        use axum::{routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let count = Arc::new(AtomicUsize::new(0));
        let requests = count.clone();
        let app=Router::new().route("/api/v1/jobToken/refresh",post(move|Json(body):Json<Value>|{let count=requests.clone();async move{assert_eq!(body["refresh_token"],"single-use");count.fetch_add(1,Ordering::SeqCst);tokio::time::sleep(std::time::Duration::from_millis(20)).await;Json(serde_json::json!({"token":"rotated","refresh_token":"next-pair","expires_in":3_600_000}))}}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let c = Credentials {
            edition: Edition::Qoder,
            access_token: "previous".into(),
            refresh_token: "single-use".into(),
            expires_at: 0,
            user_id: uuid::Uuid::new_v4().to_string(),
            name: "name".into(),
            email: String::new(),
            machine_id: "machine".into(),
            device_token: "separate-device".into(),
            device_refresh: "separate-refresh".into(),
            device_expires_at: i64::MAX,
            device_chat: false,
            native_device_only: false,
        };
        let lock = request_lock(c.edition, &account_id(&c)).unwrap();
        let renew = || async {
            let _guard = lock.lock().await;
            rotate_at(&c, false, true, &base).await.unwrap()
        };
        let (first, second) = tokio::join!(renew(), renew());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(first == second);
        assert_eq!(first.device_token, "separate-device");
        assert_eq!(first.device_refresh, "separate-refresh");
        let after = rotate_at(&first, false, false, &base).await.unwrap();
        assert!(after == first);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let id = account_id(&c);
        let mut deleted = Store::new();
        assert!(save_rotation(&mut deleted, &id, &c, &first).is_err());
        assert!(deleted.is_empty());
        rotations().lock().unwrap().remove(&rotation_key(&c, false));
        server.abort();
    }
    #[tokio::test]
    async fn a_pending_account_refresh_does_not_block_other_accounts_or_regions() {
        let a = request_lock(Edition::Qoder, "blocked").unwrap();
        let _pending = a.lock().await;
        for (edition, id) in [(Edition::Qoder, "other"), (Edition::QoderCn, "blocked")] {
            let b = request_lock(edition, id).unwrap();
            let _ready = tokio::time::timeout(std::time::Duration::from_millis(100), b.lock())
                .await
                .unwrap();
            let c = Credentials {
                edition,
                access_token: "valid".into(),
                refresh_token: "valid-refresh".into(),
                expires_at: i64::MAX,
                user_id: id.into(),
                name: String::new(),
                email: String::new(),
                machine_id: "machine".into(),
                device_token: "device".into(),
                device_refresh: "device-refresh".into(),
                device_expires_at: i64::MAX,
                device_chat: false,
                native_device_only: false,
            };
            let credentials = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                rotate_at(&c, false, false, "http://127.0.0.1:1"),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(credentials.access_token, "valid");
        }
    }

    #[tokio::test]
    async fn voice_credentials_rotate_only_the_rejected_device_family_once() {
        use axum::{routing::post, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        for (edition, device_chat) in [
            (Edition::Qoder, false),
            (Edition::QoderCn, false),
            (Edition::QoderCn, true),
        ] {
            let count = Arc::new(AtomicUsize::new(0));
            let requests = count.clone();
            let app = Router::new().route("/api/v1/deviceToken/refresh", post(move |Json(body): Json<Value>| {
                assert_eq!(body["refresh_token"], "single-use-device-refresh");
                requests.fetch_add(1, Ordering::SeqCst);
                async { Json(serde_json::json!({"token":"rotated-device","refresh_token":"next-device-refresh","expires_in":3600})) }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let c = Credentials {
                edition,
                access_token: "job-token".into(),
                refresh_token: "job-refresh".into(),
                expires_at: i64::MAX,
                user_id: uuid::Uuid::new_v4().to_string(),
                name: "User".into(),
                email: "mail".into(),
                machine_id: "machine".into(),
                device_token: "device-token".into(),
                device_refresh: "single-use-device-refresh".into(),
                device_expires_at: i64::MAX,
                device_chat,
                native_device_only: false,
            };
            let id = account_id(&c);
            let path =
                std::env::temp_dir().join(format!("qoder-voice-{}.json", uuid::Uuid::new_v4()));
            let store = Store::from([(
                id.clone(),
                Saved {
                    account: summary(&c),
                    credential: protect(&c).unwrap(),
                },
            )]);
            super::super::cursor_auth::write(&path, &store).unwrap();
            let before = std::fs::read(&path).unwrap();
            let first = voice_credentials_at(&path, edition, &id, None, &base, "0.4.9")
                .await
                .unwrap();
            assert_eq!(first["deviceToken"], "device-token");
            assert_eq!(first["appVersion"], "0.4.9");
            assert!(first.get("accessToken").is_none());
            assert!(first.get("deviceRefresh").is_none());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            let wrong_family =
                serde_json::json!({"deviceToken":"job-token","accessToken":"device-token"});
            voice_credentials_at(&path, edition, &id, Some(&wrong_family), &base, "0.4.9")
                .await
                .unwrap();
            let invalid = serde_json::json!({"accessToken":"device-token"});
            assert!(
                voice_credentials_at(&path, edition, &id, Some(&invalid), &base, "0.4.9")
                    .await
                    .is_err()
            );
            assert_eq!(count.load(Ordering::SeqCst), 0);
            let rejected = serde_json::json!({"deviceToken":"device-token"});
            let renewed =
                voice_credentials_at(&path, edition, &id, Some(&rejected), &base, "0.4.9")
                    .await
                    .unwrap();
            assert_eq!(renewed["deviceToken"], "rotated-device");
            assert_eq!(count.load(Ordering::SeqCst), 1);
            let current = selected(&load_at(&path).unwrap(), edition, &id).unwrap();
            assert_eq!(current.device_refresh, "next-device-refresh");
            assert_eq!(
                current.access_token,
                if device_chat {
                    "rotated-device"
                } else {
                    "job-token"
                }
            );
            assert_eq!(
                current.refresh_token,
                if device_chat {
                    "next-device-refresh"
                } else {
                    "job-refresh"
                }
            );
            assert_eq!(
                voice_credentials_at(&path, edition, &id, Some(&rejected), &base, "0.4.9")
                    .await
                    .unwrap(),
                renewed
            );
            assert_eq!(count.load(Ordering::SeqCst), 1);
            let other = if edition == Edition::Qoder {
                Edition::QoderCn
            } else {
                Edition::Qoder
            };
            assert!(
                voice_credentials_at(&path, other, &id, None, &base, "0.4.9")
                    .await
                    .is_err()
            );
            std::fs::remove_file(path).unwrap();
            server.abort();
        }
    }
}
