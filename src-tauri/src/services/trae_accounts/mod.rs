mod native;
mod oauth;
mod quota;

use native::{Credentials, Identity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static REQUEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edition {
    #[serde(rename = "trae")]
    Trae,
    #[serde(rename = "traecn")]
    Traecn,
}

impl Edition {
    pub fn tool(self) -> &'static str {
        match self {
            Self::Trae => "trae",
            Self::Traecn => "traecn",
        }
    }

    fn api(self) -> &'static str {
        match self {
            Self::Trae => "https://growsg-normal.trae.ai",
            Self::Traecn => "https://api.trae.cn",
        }
    }

    fn origin(self) -> &'static str {
        match self {
            Self::Trae => "https://www.trae.ai",
            Self::Traecn => "https://www.trae.cn",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub edition: Edition,
    pub name: String,
    pub plan: Option<String>,
    pub active: bool,
    pub remaining: Option<f64>,
    #[serde(default)]
    pub total: Option<f64>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub base_remaining: Option<f64>,
    #[serde(default)]
    pub base_total: Option<f64>,
    #[serde(default)]
    pub base_reset_at: Option<i64>,
    #[serde(default)]
    pub reward_remaining: Option<f64>,
    #[serde(default)]
    pub reward_total: Option<f64>,
    #[serde(default)]
    pub reward_next_expiry_at: Option<i64>,
    #[serde(default)]
    pub addon_remaining: Option<f64>,
    pub work_remaining: Option<f64>,
    pub daily_claimed_at: Option<i64>,
    pub device_claimed_at: Option<i64>,
    pub reward_enabled: Option<bool>,
    pub subscription_end_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
}

type Store = BTreeMap<String, Saved>;

fn store_path(edition: Edition) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(format!(".echobird/{}-accounts.json", edition.tool())))
}

fn load(edition: Edition) -> Result<Store, String> {
    load_at(&store_path(edition)?)
}

fn load_at(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    super::cursor_auth::read(path)
}

fn save(edition: Edition, accounts: &Store) -> Result<(), String> {
    super::cursor_auth::write(&store_path(edition)?, accounts)
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
        .filter(|encoded| {
            encoded.len() >= 56
                && encoded.len() % 2 == 0
                && encoded.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
        .ok_or("accountError.keychain")?;
    let raw = super::model_manager::decrypt_key_for_use(&saved.credential);
    let credentials: Credentials =
        serde_json::from_str(&raw).map_err(|_| "accountError.keychain")?;
    if summary(saved.account.edition, &credentials, false)?.id != saved.account.id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(credentials)
}

fn native_is_newer(saved: &Value, current: &Value) -> bool {
    let age = |auth: &Value| {
        (
            native::timestamp_ms(&auth["expiredAt"]),
            native::timestamp_ms(&auth["tokenReleaseAt"]),
        )
    };
    age(current) >= age(saved)
}

fn accept_incoming(existing: &Credentials, incoming: &Credentials) -> Result<bool, String> {
    native::validate_identity(&existing.identity, &incoming.identity)?;
    Ok(native_is_newer(&existing.auth, &incoming.auth))
}

fn device_claimed_on(
    current: &Identity,
    saved: Result<Credentials, String>,
    claimed: Option<i64>,
) -> Option<i64> {
    saved
        .ok()
        .filter(|saved| native::validate_identity(current, &saved.identity).is_ok())
        .and(claimed)
}

fn sync_native(
    edition: Edition,
    current: &Credentials,
    accounts: &mut Store,
    allow_insert: bool,
) -> Result<(), String> {
    let id = summary(edition, current, true)?.id;
    let should_sync = accounts
        .get(&id)
        .map(unprotect)
        .transpose()?
        .map(|saved| native_is_newer(&saved.auth, &current.auth))
        .unwrap_or(allow_insert);
    if should_sync {
        capture(edition, current, accounts)?;
    }
    Ok(())
}

fn summary(edition: Edition, credentials: &Credentials, active: bool) -> Result<Account, String> {
    native::access(&credentials.auth)?;
    native::text(&credentials.auth, "refreshToken").ok_or("accountError.invalidAccount")?;
    if native::region(&credentials.auth).is_some_and(|region| region != edition) {
        return Err("accountError.invalidAccount".into());
    }
    let user = native::text(&credentials.auth, "userId").ok_or("accountError.invalidAccount")?;
    let account = &credentials.auth["account"];
    let name = native::text(account, "email")
        .or_else(|| native::text(account, "username"))
        .or_else(|| native::text(account, "nonPlainTextMobile"))
        .unwrap_or(user);
    Ok(Account {
        id: format!(
            "{:x}",
            Sha256::digest(format!("{}\0{user}", edition.tool()))
        ),
        edition,
        name: name.into(),
        plan: None,
        active,
        remaining: None,
        total: None,
        expires_at: None,
        currency: None,
        base_remaining: None,
        base_total: None,
        base_reset_at: None,
        reward_remaining: None,
        reward_total: None,
        reward_next_expiry_at: None,
        addon_remaining: None,
        work_remaining: None,
        daily_claimed_at: None,
        device_claimed_at: None,
        reward_enabled: None,
        subscription_end_at: None,
    })
}

fn capture(
    edition: Edition,
    credentials: &Credentials,
    accounts: &mut Store,
) -> Result<Account, String> {
    let mut account = summary(edition, credentials, false)?;
    if let Some(previous) = accounts.get(&account.id) {
        account.plan = previous.account.plan.clone();
        account.remaining = previous.account.remaining;
        account.total = previous.account.total;
        account.expires_at = previous.account.expires_at;
        account.currency = previous.account.currency.clone();
        account.base_remaining = previous.account.base_remaining;
        account.base_total = previous.account.base_total;
        account.base_reset_at = previous.account.base_reset_at;
        account.reward_remaining = previous.account.reward_remaining;
        account.reward_total = previous.account.reward_total;
        account.reward_next_expiry_at = previous.account.reward_next_expiry_at;
        account.addon_remaining = previous.account.addon_remaining;
        account.work_remaining = previous.account.work_remaining;
        account.daily_claimed_at = previous.account.daily_claimed_at;
        account.device_claimed_at = device_claimed_on(
            &credentials.identity,
            unprotect(previous),
            previous.account.device_claimed_at,
        );
        account.reward_enabled = previous.account.reward_enabled;
        account.subscription_end_at = previous.account.subscription_end_at;
    }
    accounts.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            credential: protect(credentials)?,
        },
    );
    Ok(account)
}

fn installed(edition: Edition) -> bool {
    super::tool_manager::get_tool_exe_path(edition.tool()).is_some()
}

pub(crate) async fn list_saved_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    list_saved_accounts_at(edition, &store_path(edition)?)
}

fn list_saved_accounts_at(edition: Edition, path: &Path) -> Result<Vec<Account>, String> {
    Ok(load_at(path)?
        .into_values()
        .filter(|saved| saved.account.edition == edition)
        .map(|mut saved| {
            saved.account.active = false;
            saved.account
        })
        .collect())
}

fn proxy_saved_at(edition: Edition, id: &str, path: &Path) -> Result<Saved, String> {
    load_at(path)?
        .remove(id)
        .filter(|saved| saved.account.edition == edition && saved.account.id == id)
        .ok_or_else(|| "accountError.invalidAccount".into())
}

pub(crate) fn connection_metadata(edition: Edition, id: &str) -> Result<Value, String> {
    let saved = proxy_saved_at(edition, id, &store_path(edition)?)?;
    let metadata = proxy_value(edition, &unprotect(&saved)?)?;
    Ok(serde_json::json!({"region": metadata["region"]}))
}

pub(crate) async fn proxy_credentials(
    edition: Edition,
    id: &str,
    force_refresh: bool,
) -> Result<Value, String> {
    let path = store_path(edition)?;
    let rejected = if force_refresh {
        let _guard = ACCOUNT_LOCK.lock().await;
        let credentials = unprotect(&proxy_saved_at(edition, id, &path)?)?;
        Some(native::access(&credentials.auth)?.to_owned())
    } else {
        None
    };
    let native_dir = native::data_dir(edition).ok();
    proxy_credentials_at(
        edition,
        id,
        &path,
        rejected.as_deref(),
        native_dir.as_deref(),
    )
    .await
}

pub(crate) async fn proxy_credentials_after_rejection(
    edition: Edition,
    id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    let token = native::text(rejected, "accessToken").ok_or("accountError.invalidAccount")?;
    proxy_credentials_at(
        edition,
        id,
        &store_path(edition)?,
        Some(token),
        native::data_dir(edition).ok().as_deref(),
    )
    .await
}

async fn proxy_credentials_at(
    edition: Edition,
    id: &str,
    path: &Path,
    rejected: Option<&str>,
    native_dir: Option<&Path>,
) -> Result<Value, String> {
    let _request = REQUEST_LOCK.lock().await;
    let live = native_dir
        .and_then(|dir| native::read(dir, edition).ok().flatten())
        .filter(|live| summary(edition, live, true).is_ok_and(|account| account.id == id));
    let previous = {
        let _guard = ACCOUNT_LOCK.lock().await;
        if let Some(live) = &live {
            let mut accounts = load_at(path)?;
            sync_native(edition, live, &mut accounts, false)?;
            super::cursor_auth::write(path, &accounts)?;
        }
        unprotect(&proxy_saved_at(edition, id, path)?)?
    };
    let mut incoming = previous.clone();
    let force = rejected_token_is_current(&previous.auth, rejected);
    let refreshed = oauth::ensure_fresh_with_force(edition, &mut incoming, force).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load_at(path)?;
    let saved = accounts
        .get_mut(id)
        .filter(|saved| saved.account.edition == edition && saved.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    let mut current = unprotect(saved)?;
    if refreshed && apply_proxy_refresh(&mut current, &previous, &incoming) {
        saved.credential = protect(&current)?;
        super::cursor_auth::write(path, &accounts)?;
        if let (Some(dir), true) = (native_dir, live.is_some()) {
            native::apply(dir, &current)?;
        }
    }
    proxy_value(edition, &current)
}

fn apply_proxy_refresh(
    current: &mut Credentials,
    previous: &Credentials,
    incoming: &Credentials,
) -> bool {
    if !same_credential_revision(current, previous) {
        return false;
    }
    for field in [
        "token",
        "accessToken",
        "refreshToken",
        "expiredAt",
        "refreshExpiredAt",
        "tokenReleaseAt",
    ] {
        if let Some(value) = incoming.auth.get(field) {
            current.auth[field] = value.clone();
        }
    }
    true
}

fn same_credential_revision(current: &Credentials, previous: &Credentials) -> bool {
    native::validate_identity(&current.identity, &previous.identity).is_ok()
        && current.identity.key_pair.is_some() == previous.identity.key_pair.is_some()
        && native::access(&current.auth).ok() == native::access(&previous.auth).ok()
        && native::text(&current.auth, "refreshToken")
            == native::text(&previous.auth, "refreshToken")
        && native::timestamp_ms(&current.auth["expiredAt"])
            == native::timestamp_ms(&previous.auth["expiredAt"])
        && native::timestamp_ms(&current.auth["refreshExpiredAt"])
            == native::timestamp_ms(&previous.auth["refreshExpiredAt"])
        && native::timestamp_ms(&current.auth["tokenReleaseAt"])
            == native::timestamp_ms(&previous.auth["tokenReleaseAt"])
        && ["host", "loginHost", "userRegion"]
            .into_iter()
            .all(|field| current.auth.get(field) == previous.auth.get(field))
}

fn forget_authentication(edition: Edition, id: &str) {
    super::smart_router::forget_account_authentication(
        match edition {
            Edition::Trae => crate::models::model::AccountProvider::Trae,
            Edition::Traecn => crate::models::model::AccountProvider::Traecn,
        },
        id,
    );
}

fn rejected_token_is_current(auth: &Value, rejected: Option<&str>) -> bool {
    rejected.is_some_and(|token| native::access(auth).ok() == Some(token))
}

fn proxy_value(edition: Edition, credentials: &Credentials) -> Result<Value, String> {
    summary(edition, credentials, false)?;
    let fallback_region = if edition == Edition::Traecn {
        "CN"
    } else {
        let host = oauth::host(edition, &credentials.auth)?;
        if host.ends_with(".traeapi.us") {
            "USTTP"
        } else if host == "https://growva-normal.trae.ai" {
            "US"
        } else {
            "SG"
        }
    };
    let region = native::text(&credentials.auth["userRegion"], "region")
        .or_else(|| credentials.auth["userRegion"].as_str())
        .unwrap_or(fallback_region);
    let mut value = serde_json::json!({
        "accessToken": native::access(&credentials.auth)?,
        "userId": native::text(&credentials.auth, "userId").ok_or("accountError.invalidAccount")?,
        "deviceId": credentials.identity.device_id,
        "machineId": credentials.identity.machine_id,
        "appVersion": credentials.identity.app_version,
        "region": region.to_ascii_uppercase(),
        "edition": edition,
    });
    if edition == Edition::Trae {
        for (source, target) in [("storeRegion", "storeRegion"), ("scope", "accountScope")] {
            if let Some(field) = credentials.auth["account"][source]
                .as_str()
                .filter(|value| !value.trim().is_empty())
            {
                value[target] = if source == "storeRegion" {
                    serde_json::json!(field.to_ascii_uppercase())
                } else {
                    serde_json::json!(field)
                };
            }
        }
    }
    Ok(value)
}

pub async fn list_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = installed(edition)
        .then(|| native::data_dir(edition).ok())
        .flatten();
    list_from_paths(edition, &store_path(edition)?, dir.as_deref())
}

fn list_from_paths(
    edition: Edition,
    store: &Path,
    dir: Option<&Path>,
) -> Result<Vec<Account>, String> {
    let first_load = !store.exists();
    let mut accounts: Store = if first_load {
        Store::new()
    } else {
        super::cursor_auth::read(store)?
    };
    let mut active = None;
    let mut current_identity = None;
    if let Some(dir) = dir {
        current_identity = native::login_identity(dir, edition).ok();
        if let Some(current) = native::read(dir, edition).ok().flatten() {
            if let Ok(summary) = summary(edition, &current, true) {
                active = Some(summary.id.clone());
                // Only the EchoBird snapshot is synchronized during passive loading.
                if (first_load || accounts.contains_key(&summary.id))
                    && sync_native(edition, &current, &mut accounts, first_load).is_ok()
                {
                    let _ = super::cursor_auth::write(store, &accounts);
                }
            }
        }
    }
    Ok(accounts
        .into_values()
        .map(|mut saved| {
            saved.account.active = active.as_deref() == Some(saved.account.id.as_str());
            saved.account.device_claimed_at = current_identity.as_ref().and_then(|current| {
                device_claimed_on(current, unprotect(&saved), saved.account.device_claimed_at)
            });
            saved.account
        })
        .collect())
}

pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
    if !installed(edition) {
        return Err("accountError.initializeClient".into());
    }
    let dir = native::data_dir(edition)?;
    oauth::start(edition, native::login_identity(&dir, edition)?).await
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some((edition, credentials)) = oauth::poll(id)? else {
        return Ok(None);
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    oauth::complete(id, || {
        let mut accounts = load(edition)?;
        let account = capture(edition, &credentials, &mut accounts)?;
        save(edition, &accounts)?;
        forget_authentication(edition, &account.id);
        Ok(Some(account))
    })
}

pub fn cancel_login(id: &str) -> Result<(), String> {
    oauth::cancel(id)
}

pub async fn delete_account(edition: Edition, id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load(edition)?;
    accounts.remove(id);
    save(edition, &accounts)
}

async fn selected(edition: Edition, id: &str) -> Result<Saved, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load(edition)?;
    if !accounts.contains_key(id) {
        return Err("accountError.cancelled".into());
    }
    let mut active = false;
    let mut current_identity = None;
    if installed(edition) {
        let dir = native::data_dir(edition)?;
        current_identity = Some(native::login_identity(&dir, edition)?);
        if let Some(current) = native::read(&dir, edition)? {
            if summary(edition, &current, true)?.id == id {
                active = true;
                sync_native(edition, &current, &mut accounts, false)?;
            }
        }
    }
    let mut saved = accounts
        .get(id)
        .cloned()
        .ok_or("accountError.invalidAccount")?;
    saved.account.active = active;
    let current_identity = current_identity.ok_or("accountError.initializeClient")?;
    let mut credentials = unprotect(&saved)?;
    native::validate_identity(&current_identity, &credentials.identity)?;
    credentials.identity.app_version = current_identity.app_version;
    saved.credential = protect(&credentials)?;
    save(edition, &accounts)?;
    Ok(saved)
}

async fn fresh(
    edition: Edition,
    credentials: &mut Credentials,
    active: bool,
) -> Result<(), String> {
    if active && super::process_manager::desktop_tool_is_running(edition.tool()).await? {
        if native::timestamp(&credentials.auth["expiredAt"])
            .is_none_or(|expiry| expiry <= chrono::Utc::now().timestamp())
        {
            return Err("accountError.expired".into());
        }
        return Ok(());
    }
    oauth::ensure_fresh(edition, credentials).await
}

async fn persist(edition: Edition, mut saved: Saved) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load(edition)?;
    let existing = accounts
        .get(&saved.account.id)
        .ok_or("accountError.cancelled")?;
    let existing_credentials = unprotect(existing)?;
    let incoming_credentials = unprotect(&saved)?;
    let accepted = accept_incoming(&existing_credentials, &incoming_credentials)?;
    let changed =
        accepted && !same_credential_revision(&existing_credentials, &incoming_credentials);
    if !accepted {
        saved.credential = existing.credential.clone();
        saved.account.name = existing.account.name.clone();
    }
    let account = saved.account.clone();
    accounts.insert(account.id.clone(), saved);
    save(edition, &accounts)?;
    if changed {
        forget_authentication(edition, &account.id);
    }
    Ok(account)
}

pub async fn refresh_quota(edition: Edition, id: &str) -> Result<Account, String> {
    let _request = REQUEST_LOCK.lock().await;
    let mut saved = selected(edition, id).await?;
    let mut credentials = unprotect(&saved)?;
    fresh(edition, &mut credentials, saved.account.active).await?;
    saved.credential = protect(&credentials)?;
    persist(edition, saved.clone()).await?;
    saved.account = quota::refresh(
        &oauth::host(edition, &credentials.auth)?,
        edition,
        native::access(&credentials.auth)?,
        &credentials.identity.device_id,
        saved.account,
    )
    .await?;
    persist(edition, saved).await
}

pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
    let _request = REQUEST_LOCK.lock().await;
    if edition != Edition::Traecn {
        return Err("accountError.auth".into());
    }
    let mut saved = selected(edition, id).await?;
    let mut credentials = unprotect(&saved)?;
    if credentials.identity.device_id.is_empty() {
        return Err("accountError.initializeClient".into());
    }
    fresh(edition, &mut credentials, saved.account.active).await?;
    saved.credential = protect(&credentials)?;
    persist(edition, saved.clone()).await?;
    saved.account = quota::claim(
        edition,
        native::access(&credentials.auth)?,
        &credentials.identity.device_id,
        saved.account,
    )
    .await?;
    let claimed = persist(edition, saved.clone()).await?;
    match quota::refresh(
        &oauth::host(edition, &credentials.auth)?,
        edition,
        native::access(&credentials.auth)?,
        &credentials.identity.device_id,
        claimed.clone(),
    )
    .await
    {
        Ok(account) => {
            saved.account = account;
            persist(edition, saved).await
        }
        Err(_) => Ok(claimed),
    }
}

pub async fn activate_account(edition: Edition, id: &str) -> Result<(), String> {
    let _request = REQUEST_LOCK.lock().await;
    if !installed(edition) {
        return Err("accountError.initializeClient".into());
    }
    unprotect(&selected(edition, id).await?)?;
    let _guard = ACCOUNT_LOCK.lock().await;
    super::process_manager::stop_desktop_for_account(edition.tool()).await?;
    let dir = native::data_dir(edition)?;
    let mut accounts = load(edition)?;
    if let Some(current) = native::read(&dir, edition)? {
        sync_native(edition, &current, &mut accounts, false)?;
        save(edition, &accounts)?;
    }
    let mut credentials = unprotect(accounts.get(id).ok_or("accountError.cancelled")?)?;
    let previous = credentials.clone();
    let current_identity = native::login_identity(&dir, edition)?;
    native::validate_identity(&current_identity, &credentials.identity)?;
    credentials.identity.app_version = current_identity.app_version;
    oauth::ensure_fresh(edition, &mut credentials).await?;
    accounts
        .get_mut(id)
        .ok_or("accountError.cancelled")?
        .credential = protect(&credentials)?;
    save(edition, &accounts)?;
    if !same_credential_revision(&previous, &credentials) {
        forget_authentication(edition, id);
    }
    native::apply(&dir, &credentials)
}

pub(super) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}

pub(super) async fn response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let key = if status == reqwest::StatusCode::UNAUTHORIZED
            || status == reqwest::StatusCode::FORBIDDEN
        {
            "accountError.auth"
        } else {
            "accountError.network"
        };
        return Err(format!("{key}|HTTP {}", status.as_u16()));
    }
    let payload: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    response_payload(payload)
}

pub(super) fn response_payload(payload: Value) -> Result<Value, String> {
    if let Some(code) = payload["ResponseMetadata"]["Error"]["Code"].as_str() {
        return Err(business_error(code));
    }
    if payload["code"].as_i64().is_some_and(|code| code != 0) {
        return Err(business_error(&payload["code"].to_string()));
    }
    Ok(payload)
}

fn business_error(code: &str) -> String {
    let key = if ["20324", "20101", "20315", "20125", "20126", "20403"].contains(&code) {
        "accountError.auth"
    } else {
        "accountError.failed"
    };
    format!("{key}|{code}")
}

pub(crate) async fn import_native(edition: Edition) -> Result<Option<bool>, String> {
    let _request = REQUEST_LOCK.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    let Some(c) = native::read(&native::data_dir(edition)?, edition)? else {
        return Ok(None);
    };
    let account = summary(edition, &c, false)?;
    let mut rows = load(edition)?;
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
    capture(edition, &c, &mut rows)?;
    save(edition, &rows)?;
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
    use serde_json::json;

    #[test]
    fn proxy_saved_directory_reads_only_saved_metadata_and_keeps_regions_separate() {
        let dir =
            std::env::temp_dir().join(format!("echobird-trae-proxy-{}", uuid::Uuid::new_v4()));
        let path = dir.join("accounts.json");
        assert!(list_saved_accounts_at(Edition::Trae, &path)
            .unwrap()
            .is_empty());
        assert!(!dir.exists());
        let mut cn = summary(Edition::Traecn, &credentials("CN"), true).unwrap();
        cn.plan = Some("Free".into());
        cn.remaining = Some(150.0);
        let global = summary(Edition::Trae, &credentials("SG"), true).unwrap();
        let accounts = Store::from([
            (
                cn.id.clone(),
                Saved {
                    account: cn.clone(),
                    credential: "not-decrypted".into(),
                },
            ),
            (
                global.id.clone(),
                Saved {
                    account: global.clone(),
                    credential: "not-decrypted".into(),
                },
            ),
        ]);
        super::super::cursor_auth::write(&path, &accounts).unwrap();
        let before = std::fs::read(&path).unwrap();
        let rows = list_saved_accounts_at(Edition::Traecn, &path).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, cn.id);
        assert_eq!(rows[0].plan, cn.plan);
        assert_eq!(rows[0].remaining, cn.remaining);
        assert!(!rows[0].active);
        assert_eq!(
            list_saved_accounts_at(Edition::Trae, &path).unwrap()[0].id,
            global.id
        );
        assert!(proxy_saved_at(Edition::Trae, &cn.id, &path).is_err());
        assert!(proxy_saved_at(Edition::Traecn, &cn.id, &path).is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        super::super::cursor_auth::write(&path, &Store::new()).unwrap();
        assert!(proxy_saved_at(Edition::Traecn, &cn.id, &path).is_err());
        assert!(list_saved_accounts_at(Edition::Traecn, &path)
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn proxy_values_restrict_secrets_and_derive_the_saved_region() {
        let mut current = credentials("CN");
        current.identity.key_pair = Some(native::KeyPair {
            private_key: "fixture-private".into(),
            public_key: "fixture-public".into(),
        });
        let value = proxy_value(Edition::Traecn, &current).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 7);
        assert_eq!(value["accessToken"], "fixture-token");
        assert_eq!(value["edition"], "traecn");
        assert_eq!(value["region"], "CN");
        assert!(!value.to_string().contains("fixture-refresh"));
        assert!(!value.to_string().contains("fixture-private"));
        assert!(!value.to_string().contains("fixture-public"));
        assert!(proxy_value(Edition::Trae, &current).is_err());
        current.auth.as_object_mut().unwrap().remove("userRegion");
        current.auth["host"] = "https://grow-normal.traeapi.us".into();
        assert_eq!(
            proxy_value(Edition::Trae, &current).unwrap()["region"],
            "USTTP"
        );
        current.auth["host"] = "https://growva-normal.trae.ai".into();
        assert_eq!(
            proxy_value(Edition::Trae, &current).unwrap()["region"],
            "US"
        );
        current.auth["userRegion"] = json!({"region":"SG"});
        assert_eq!(
            proxy_value(Edition::Trae, &current).unwrap()["region"],
            "SG"
        );
        assert!(proxy_value(Edition::Traecn, &current).is_err());
    }

    #[test]
    fn proxy_rejection_refresh_skips_an_already_rotated_token() {
        let mut current = credentials("CN");
        assert!(!rejected_token_is_current(&current.auth, None));
        assert!(rejected_token_is_current(
            &current.auth,
            Some("fixture-token")
        ));
        current.auth["token"] = "rotated-token".into();
        assert!(!rejected_token_is_current(
            &current.auth,
            Some("fixture-token")
        ));
        assert!(rejected_token_is_current(
            &current.auth,
            Some("rotated-token")
        ));
    }

    #[test]
    fn proxy_refresh_preserves_newer_tokens_and_current_profile_fields() {
        let mut previous = credentials("CN");
        previous.auth["expiredAt"] = "2030-01-01T00:00:00Z".into();
        let mut incoming = previous.clone();
        incoming.auth["token"] = "rotated-access".into();
        incoming.auth["refreshToken"] = "rotated-refresh".into();
        incoming.auth["expiredAt"] = "2030-02-01T00:00:00Z".into();
        let mut current = previous.clone();
        current.auth["account"]["username"] = "current-name".into();
        current.identity.app_version = "current-version".into();
        assert!(same_credential_revision(&current, &previous));
        assert!(!same_credential_revision(&incoming, &previous));
        assert!(apply_proxy_refresh(&mut current, &previous, &incoming));
        assert_eq!(current.auth["token"], "rotated-access");
        assert_eq!(current.auth["account"]["username"], "current-name");
        assert_eq!(current.identity.app_version, "current-version");
        for (field, value) in [
            ("token", incoming.auth["token"].clone()),
            ("refreshToken", incoming.auth["refreshToken"].clone()),
            ("expiredAt", incoming.auth["expiredAt"].clone()),
            ("host", json!("https://grow-normal.traeapi.us")),
            ("loginHost", json!("https://growva-normal.trae.ai")),
            ("userRegion", json!({"region":"US"})),
        ] {
            let mut newer = previous.clone();
            newer.auth[field] = value;
            let before = serde_json::to_value(&newer).unwrap();
            assert!(!same_credential_revision(&newer, &previous));
            assert!(!apply_proxy_refresh(&mut newer, &previous, &incoming));
            assert_eq!(serde_json::to_value(newer).unwrap(), before);
        }
        let mut other_device = previous.clone();
        other_device.identity.device_id = "new-device".into();
        assert!(!apply_proxy_refresh(
            &mut other_device,
            &previous,
            &incoming
        ));
    }

    fn credentials(region: &str) -> Credentials {
        Credentials {
            auth: json!({"token":"fixture-token", "refreshToken":"fixture-refresh", "userId":"user", "userRegion":{"region":region}, "account":{"username":"name"}}),
            identity: Identity {
                device_id: "device".into(),
                machine_id: "machine".into(),
                app_version: "3".into(),
                client_id: "client".into(),
                key_pair: None,
            },
        }
    }

    #[test]
    fn account_identity_is_region_bound_and_unknown_quota_stays_unknown() {
        let cn = summary(Edition::Traecn, &credentials("CN"), false).unwrap();
        let global = summary(Edition::Trae, &credentials("SG"), false).unwrap();
        assert_ne!(cn.id, global.id);
        assert_eq!(cn.remaining, None);
        assert_eq!(cn.work_remaining, None);
        assert_eq!(cn.plan, None);
        assert_eq!(cn.total, None);
        assert_eq!(cn.base_remaining, None);
        assert_eq!(cn.reward_remaining, None);
        assert!(summary(Edition::Trae, &credentials("CN"), false).is_err());
    }

    #[test]
    fn older_saved_account_accepts_missing_grouped_quota_fields() {
        let account: Account = serde_json::from_value(json!({
            "id":"fixture","edition":"traecn","name":"fixture","active":false,
            "plan":"Free","remaining":4600,"workRemaining":null,
            "dailyClaimedAt":null,"deviceClaimedAt":null,"rewardEnabled":true,
            "subscriptionEndAt":null
        }))
        .unwrap();
        assert_eq!(account.remaining, Some(4600.0));
        assert_eq!(account.total, None);
        assert_eq!(account.currency, None);
        assert_eq!(account.expires_at, None);
        assert_eq!(account.base_remaining, None);
        assert_eq!(account.base_total, None);
        assert_eq!(account.base_reset_at, None);
        assert_eq!(account.reward_remaining, None);
        assert_eq!(account.reward_total, None);
        assert_eq!(account.reward_next_expiry_at, None);
        assert_eq!(account.addon_remaining, None);
    }

    #[test]
    fn rotated_snapshot_is_not_replaced_by_older_native_session_within_same_second() {
        let saved = json!({"token":"new-access","refreshToken":"new-refresh","expiredAt":"2030-01-01T00:00:00.900Z","tokenReleaseAt":"2029-12-18T00:00:00.900Z"});
        let old_native = json!({"token":"old-access","refreshToken":"old-refresh","expiredAt":"2030-01-01T00:00:00.100Z","tokenReleaseAt":"2029-12-18T00:00:00.100Z"});
        assert!(!native_is_newer(&saved, &old_native));
        assert!(native_is_newer(&old_native, &saved));
        let same_expiry_old_native = json!({"expiredAt":"2030-01-01T00:00:00.900Z","tokenReleaseAt":"2029-12-18T00:00:00.100Z"});
        assert!(!native_is_newer(&saved, &same_expiry_old_native));
    }

    #[test]
    fn damaged_ciphertext_is_rejected_before_shared_decoder() {
        let account = summary(Edition::Traecn, &credentials("CN"), false).unwrap();
        for credential in [
            "enc:v1:a",
            "enc:v1:éééééééééééééééééééééééééééééé",
            "enc:v1:zz",
            "plaintext",
        ] {
            let saved = Saved {
                account: account.clone(),
                credential: credential.into(),
            };
            assert_eq!(
                unprotect(&saved).err().as_deref(),
                Some("accountError.keychain")
            );
        }
    }

    #[test]
    fn native_sync_never_restores_deleted_accounts() {
        for edition in [Edition::Traecn, Edition::Trae] {
            let region = if edition == Edition::Traecn {
                "CN"
            } else {
                "SG"
            };
            let current = credentials(region);
            let current_id = summary(edition, &current, true).unwrap().id;
            let mut accounts = Store::new();
            sync_native(edition, &current, &mut accounts, false).unwrap();
            assert!(accounts.is_empty());

            let mut other = current.clone();
            other.auth["userId"] = "other-user".into();
            let other_account = summary(edition, &other, false).unwrap();
            accounts.insert(
                other_account.id.clone(),
                Saved {
                    account: other_account,
                    credential: "plaintext".into(),
                },
            );
            let before = serde_json::to_value(&accounts).unwrap();
            sync_native(edition, &current, &mut accounts, false).unwrap();
            assert!(!accounts.contains_key(&current_id));
            assert_eq!(serde_json::to_value(&accounts).unwrap(), before);
        }
    }

    #[test]
    fn native_sync_still_validates_saved_current_accounts() {
        for edition in [Edition::Traecn, Edition::Trae] {
            let region = if edition == Edition::Traecn {
                "CN"
            } else {
                "SG"
            };
            let current = credentials(region);
            let account = summary(edition, &current, true).unwrap();
            for credential in ["plaintext", "enc:v1:broken"] {
                let mut accounts = Store::from([(
                    account.id.clone(),
                    Saved {
                        account: account.clone(),
                        credential: credential.into(),
                    },
                )]);
                let before = serde_json::to_value(&accounts).unwrap();
                assert_eq!(
                    sync_native(edition, &current, &mut accounts, false)
                        .err()
                        .as_deref(),
                    Some("accountError.keychain")
                );
                assert_eq!(serde_json::to_value(&accounts).unwrap(), before);
            }
        }
    }

    #[test]
    fn passive_native_failure_preserves_saved_rows_and_deleted_rows_stay_deleted() {
        let dir = std::env::temp_dir().join(format!("echobird-trae-{}", uuid::Uuid::new_v4()));
        let store = dir.join("accounts.json");
        let native_dir = dir.join("native");
        let mut account = summary(Edition::Traecn, &credentials("CN"), false).unwrap();
        account.remaining = Some(150.0);
        account.total = Some(200.0);
        account.reward_remaining = Some(150.0);
        account.reward_total = Some(200.0);
        account.reward_next_expiry_at = Some(1793462399);
        account.device_claimed_at = Some(1791774000);
        let accounts = Store::from([(
            account.id.clone(),
            Saved {
                account: account.clone(),
                credential: "enc:v1:broken".into(),
            },
        )]);
        super::super::cursor_auth::write(&store, &accounts).unwrap();
        let before = std::fs::read(&store).unwrap();
        let native_path = native::storage_path(&native_dir);
        std::fs::create_dir_all(native_path.parent().unwrap()).unwrap();
        std::fs::write(&native_path, b"malformed native storage").unwrap();
        let rows = list_from_paths(Edition::Traecn, &store, Some(&native_dir)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].remaining, Some(150.0));
        assert_eq!(rows[0].total, Some(200.0));
        assert_eq!(rows[0].reward_remaining, Some(150.0));
        assert_eq!(rows[0].reward_total, Some(200.0));
        assert_eq!(rows[0].reward_next_expiry_at, Some(1793462399));
        assert!(!rows[0].active);
        assert_eq!(rows[0].device_claimed_at, None);
        let doc = json!({"iCubeAuthInfo://icube.cloudide":credentials("CN").auth,"telemetry.machineId":"fixture-machine"});
        super::super::cursor_auth::write(&native_path, &doc).unwrap();
        assert!(list_from_paths(Edition::Traecn, &store, Some(&native_dir)).unwrap()[0].active);
        assert!(!list_from_paths(Edition::Traecn, &store, None).unwrap()[0].active);
        assert_eq!(std::fs::read(&store).unwrap(), before);
        super::super::cursor_auth::write(&store, &Store::new()).unwrap();
        assert!(list_from_paths(Edition::Traecn, &store, Some(&native_dir))
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn non_auth_business_refusals_do_not_report_invalid_login() {
        for code in [9004, 9095] {
            assert!(response_payload(json!({"code":code}))
                .unwrap_err()
                .starts_with("accountError.failed|"));
        }
        assert!(
            response_payload(json!({"ResponseMetadata":{"Error":{"Code":"20324"}}}))
                .unwrap_err()
                .starts_with("accountError.auth|")
        );
    }

    #[test]
    fn pending_quota_preserves_newer_credentials_and_accepts_fresh_offline_rotation() {
        let mut old = credentials("CN");
        old.auth["expiredAt"] = "2030-01-01T00:00:00.100Z".into();
        old.auth["refreshToken"] = "old-refresh".into();
        let mut latest = old.clone();
        latest.auth["expiredAt"] = "2030-01-01T00:00:00.900Z".into();
        latest.auth["refreshToken"] = "rotated-refresh".into();
        assert!(!accept_incoming(&latest, &old).unwrap());
        assert!(accept_incoming(&old, &latest).unwrap());
        let mut other_device = old.clone();
        other_device.identity.device_id = "another-device".into();
        assert!(accept_incoming(&latest, &other_device).is_err());
    }

    #[test]
    fn device_claim_cache_is_retained_only_for_current_native_identity() {
        let saved = credentials("CN");
        let current = saved.identity.clone();
        assert_eq!(
            device_claimed_on(&current, Ok(saved.clone()), Some(1791774000)),
            Some(1791774000)
        );
        let mut other = current.clone();
        other.device_id = "new-device".into();
        assert_eq!(device_claimed_on(&other, Ok(saved), Some(1791774000)), None);
        assert_eq!(
            device_claimed_on(
                &current,
                Err("accountError.keychain".into()),
                Some(1791774000)
            ),
            None
        );
    }
}
