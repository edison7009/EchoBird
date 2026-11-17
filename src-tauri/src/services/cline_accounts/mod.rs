mod oauth;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const API: &str = "https://api.cline.bot";
static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
type Locks = Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>;
static ACTIONS: OnceLock<Locks> = OnceLock::new();
static ROTATIONS: OnceLock<Mutex<BTreeMap<String, (Credentials, Credentials)>>> = OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub name: Option<String>,
    pub active: bool,
    pub plan: Option<String>,
    pub balance_micros: Option<i64>,
    pub subscription_end_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub user_code: String,
    pub expires_at: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Credentials {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: i64,
    pub(crate) user_id: String,
    pub(crate) email: String,
    pub(crate) name: Option<String>,
    pub(crate) user_info: Value,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_pair: Option<String>,
}
type Store = BTreeMap<String, Saved>;

pub(crate) struct NativeAccountSession {
    pub(crate) credentials: Credentials,
    pub(crate) native: Option<super::cline_native::RefreshSession>,
    _action: tokio::sync::OwnedMutexGuard<()>,
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}
pub(crate) fn raw_token(token: &str) -> &str {
    let token = token.trim();
    if token
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("workos:"))
    {
        &token[7..]
    } else {
        token
    }
}
pub(crate) fn bearer(credentials: &Credentials) -> String {
    format!("workos:{}", raw_token(&credentials.access_token))
}
pub(crate) fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/cline-accounts.json"))
}
fn load(path: &Path) -> Result<Store, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| "accountError.format".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::new()),
        Err(_) => Err("accountError.read".into()),
    }
}
fn write(path: &Path, store: &Store) -> Result<(), String> {
    super::native_account_files::write(
        path,
        &serde_json::to_vec_pretty(store).map_err(|_| "accountError.format")?,
    )
}
fn account_id(c: &Credentials) -> Result<String, String> {
    if c.user_id.trim().is_empty()
        || raw_token(&c.access_token).is_empty()
        || c.refresh_token.trim().is_empty()
        || c.expires_at <= 0
        || text(&c.user_info, "clineUserId").is_some_and(|id| id != c.user_id)
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("cline\0{}", c.user_id))
    ))
}
pub(crate) fn pair(c: &Credentials) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "{}\0{}",
            raw_token(&c.access_token),
            c.refresh_token
        ))
    )
}
fn summary(c: &Credentials) -> Result<Account, String> {
    Ok(Account {
        id: account_id(c)?,
        email: c.email.clone(),
        name: c.name.clone(),
        active: false,
        plan: None,
        balance_micros: None,
        subscription_end_at: None,
    })
}
fn protect(c: &Credentials) -> Result<String, String> {
    let raw = serde_json::to_string(c).map_err(|_| "accountError.format")?;
    let encrypted = encrypt(&raw);
    if !encrypted.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(encrypted)
}
fn selected(store: &Store, id: &str) -> Result<Credentials, String> {
    let saved = store
        .get(id)
        .filter(|saved| saved.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    let encoded = saved
        .credential
        .strip_prefix("enc:v1:")
        .ok_or("accountError.keychain")?;
    if encoded.len() < 56
        || encoded.len() % 2 != 0
        || !encoded.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("accountError.keychain".into());
    }
    let c: Credentials =
        serde_json::from_str(&decrypt(&saved.credential)).map_err(|_| "accountError.keychain")?;
    if account_id(&c)? != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(c)
}
#[cfg(not(test))]
use super::model_manager::{decrypt_key_for_use as decrypt, encrypt_key_for_storage as encrypt};
pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
use tests::{decrypt, encrypt};

fn key(path: &Path, id: &str) -> String {
    format!("{}\0{id}", path.display())
}
fn action(path: &Path, id: &str) -> Result<Arc<tokio::sync::Mutex<()>>, String> {
    Ok(ACTIONS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| "accountError.busy")?
        .entry(key(path, id))
        .or_default()
        .clone())
}
fn rotations() -> &'static Mutex<BTreeMap<String, (Credentials, Credentials)>> {
    ROTATIONS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn capture(path: &Path, c: &Credentials) -> Result<Account, String> {
    let mut store = load(path)?;
    let mut account = summary(c)?;
    let mut native_pair = None;
    if let Some(saved) = store.get(&account.id) {
        native_pair = saved
            .native_pair
            .clone()
            .filter(|baseline| baseline == &pair(c));
        account.active = saved.account.active && native_pair.is_some();
    }
    store.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            credential: protect(c)?,
            native_pair,
        },
    );
    write(path, &store)?;
    rotations()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&key(path, &account.id));
    Ok(account)
}
pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _store = STORE_LOCK.lock().await;
    list_at(&store_path()?)
}
fn list_at(path: &Path) -> Result<Vec<Account>, String> {
    Ok(load(path)?
        .into_iter()
        .filter(|(id, s)| *id == s.account.id)
        .map(|(_, s)| s.account)
        .collect())
}
pub async fn start_login() -> Result<LoginStart, String> {
    oauth::start().await
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some(c) = oauth::poll(id)? else {
        return Ok(None);
    };
    let path = store_path()?;
    let _action = action(&path, &account_id(&c)?)?.lock_owned().await;
    let _store = STORE_LOCK.lock().await;
    oauth::complete(id, || capture(&path, &c)).map(Some)
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    oauth::cancel(id)
}
pub async fn delete_account(id: &str) -> Result<(), String> {
    let path = store_path()?;
    let _action = action(&path, id)?.lock_owned().await;
    let _store = STORE_LOCK.lock().await;
    let mut store = load(&path)?;
    store.remove(id).ok_or("accountError.invalidAccount")?;
    write(&path, &store)?;
    rotations()
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(&key(&path, id));
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Cline,
        id,
    );
    Ok(())
}

async fn rotate_locked(
    path: &Path,
    id: &str,
    force: bool,
    rejected: Option<&str>,
    base: &str,
    native: &mut Option<super::cline_native::RefreshSession>,
) -> Result<Credentials, String> {
    let (previous, binding) = {
        let _store = STORE_LOCK.lock().await;
        let store = load(path)?;
        (selected(&store, id)?, store[id].native_pair.clone())
    };
    let pending = rotations()
        .lock()
        .map_err(|_| "accountError.busy")?
        .get(&key(path, id))
        .cloned();
    let mut current = pending
        .as_ref()
        .filter(|(old, _)| old == &previous)
        .map(|(_, new)| new.clone())
        .unwrap_or_else(|| previous.clone());
    let bound = match native.as_mut() {
        Some(session) => match session.credentials(&current, binding.as_deref())? {
            Some(incoming) => {
                current = incoming;
                true
            }
            None => false,
        },
        None => false,
    };
    if !bound {
        native.take();
    }
    let rejected_current = rejected.is_some_and(|token| {
        token.strip_prefix("pair:").map_or_else(
            || raw_token(token) == raw_token(&current.access_token),
            |generation| generation == pair(&current),
        )
    });
    if (force && (rejected.is_none() || rejected_current))
        || (!force && current.expires_at <= chrono::Utc::now().timestamp_millis() + 30_000)
    {
        current = oauth::refresh(&current, base).await?;
    }
    if current != previous {
        rotations()
            .lock()
            .map_err(|_| "accountError.busy")?
            .insert(key(path, id), (previous.clone(), current.clone()));
        let _store = STORE_LOCK.lock().await;
        let mut store = load(path)?;
        if selected(&store, id)? != previous {
            return Err("accountError.invalidAccount".into());
        }
        store
            .get_mut(id)
            .ok_or("accountError.invalidAccount")?
            .credential = protect(&current)?;
        write(path, &store)?;
        rotations()
            .lock()
            .map_err(|_| "accountError.busy")?
            .remove(&key(path, id));
    }
    // Persist the new pair before touching the native copy. A native write failure
    // leaves the previous binding available to heal it on the next explicit use.
    if let Some(session) = native.as_mut() {
        if session.synchronize(&current)? {
            let _store = STORE_LOCK.lock().await;
            let mut store = load(path)?;
            if selected(&store, id)? != current {
                return Err("accountError.invalidAccount".into());
            }
            let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
            let next_pair = pair(&current);
            if saved.native_pair.as_deref() != Some(&next_pair) {
                saved.native_pair = Some(next_pair);
                write(path, &store)?;
            }
        }
    }
    Ok(current)
}
async fn session_at(
    path: &Path,
    id: &str,
    force: bool,
    rejected: Option<&str>,
    base: &str,
    native_path: Option<&Path>,
) -> Result<NativeAccountSession, String> {
    let path = path.to_path_buf();
    let id = id.to_owned();
    let rejected = rejected.map(str::to_owned);
    let base = base.to_owned();
    let native_path = native_path.map(Path::to_path_buf);
    // A caller timeout must not discard a single-use refresh response. The task
    // keeps both locks until its received credentials are durably saved.
    tokio::spawn(async move {
        session_locked(
            &path,
            &id,
            force,
            rejected.as_deref(),
            &base,
            native_path.as_deref(),
        )
        .await
    })
    .await
    .map_err(|_| "accountError.auth".to_string())?
}
async fn session_locked(
    path: &Path,
    id: &str,
    force: bool,
    rejected: Option<&str>,
    base: &str,
    native_path: Option<&Path>,
) -> Result<NativeAccountSession, String> {
    let action = action(path, id)?.lock_owned().await;
    {
        let _store = STORE_LOCK.lock().await;
        selected(&load(path)?, id)?;
    }
    let mut native = match native_path {
        Some(path) => Some(super::cline_native::RefreshSession::open_at(path).await?),
        None => None,
    };
    let credentials = rotate_locked(path, id, force, rejected, base, &mut native).await?;
    Ok(NativeAccountSession {
        credentials,
        native,
        _action: action,
    })
}
async fn session(
    id: &str,
    force: bool,
    rejected: Option<&str>,
    applying: bool,
) -> Result<NativeAccountSession, String> {
    let path = store_path()?;
    let bound_active = {
        let _store = STORE_LOCK.lock().await;
        let store = load(&path)?;
        selected(&store, id)?;
        store[id].account.active && store[id].native_pair.is_some()
    };
    let native = if applying || bound_active {
        super::cline_native::existing_installed_config_path()?
    } else {
        None
    };
    session_at(&path, id, force, rejected, API, native.as_deref()).await
}
pub(crate) async fn credentials_for_native(id: &str) -> Result<NativeAccountSession, String> {
    session(id, false, None, true).await
}
pub(crate) async fn saved_credentials(id: &str) -> Result<Credentials, String> {
    let _store = STORE_LOCK.lock().await;
    selected(&load(&store_path()?)?, id)
}
pub(crate) async fn mark_active(id: &str, c: &Credentials) -> Result<Account, String> {
    let _store = STORE_LOCK.lock().await;
    mark_active_at(&store_path()?, id, c)
}
pub(crate) async fn capture_native_at(
    path: &Path,
    native: &mut super::cline_native::RefreshSession,
) -> Result<(), String> {
    let _store = STORE_LOCK.lock().await;
    let mut store = load(path)?;
    let ids: Vec<_> = store
        .iter()
        .filter(|(_, saved)| saved.native_pair.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    let mut pending = rotations().lock().map_err(|_| "accountError.busy")?;
    let mut updates = Vec::new();
    for id in ids {
        let previous = selected(&store, &id)?;
        let current = pending
            .get(&key(path, &id))
            .filter(|(old, _)| old == &previous)
            .map(|(_, new)| new.clone())
            .unwrap_or_else(|| previous.clone());
        let incoming = native
            .credentials(&current, store[&id].native_pair.as_deref())?
            .unwrap_or(current);
        if account_id(&incoming)? != id {
            return Err("accountError.invalidAccount".into());
        }
        if incoming != previous {
            store.get_mut(&id).unwrap().credential = protect(&incoming)?;
            updates.push((id, previous, incoming));
        }
    }
    if updates.is_empty() {
        return Ok(());
    }
    for (id, previous, incoming) in &updates {
        pending.insert(key(path, id), (previous.clone(), incoming.clone()));
    }
    // Keep the native baseline until the new selection commits. It identifies
    // an older native pair when a newer own pair could not be synchronized.
    write(path, &store)?;
    for (id, _, _) in updates {
        pending.remove(&key(path, &id));
    }
    Ok(())
}
fn mark_active_at(path: &Path, id: &str, c: &Credentials) -> Result<Account, String> {
    let mut store = load(path)?;
    if selected(&store, id)? != *c {
        return Err("accountError.invalidAccount".into());
    }
    for saved in store.values_mut() {
        saved.account.active = saved.account.id == id;
        saved.native_pair = saved.account.active.then(|| pair(c));
    }
    let account = store[id].account.clone();
    write(path, &store)?;
    Ok(account)
}
fn proxy_value(c: &Credentials) -> Value {
    json!({"accessToken":bearer(c),"userId":c.user_id,"credentialGeneration":pair(c)})
}
pub(crate) async fn proxy_credentials(id: &str, force: bool) -> Result<Value, String> {
    let rejected = if force {
        let _store = STORE_LOCK.lock().await;
        Some(format!(
            "pair:{}",
            pair(&selected(&load(&store_path()?)?, id)?)
        ))
    } else {
        None
    };
    Ok(proxy_value(
        &session(id, force, rejected.as_deref(), false)
            .await?
            .credentials,
    ))
}
pub(crate) async fn proxy_credentials_after_rejection(
    id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    text(rejected, "accessToken").ok_or("accountError.invalidAccount")?;
    let generation = text(rejected, "credentialGeneration")
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or("accountError.invalidAccount")?;
    let rejected = format!("pair:{generation}");
    Ok(proxy_value(
        &session(id, true, Some(&rejected), false).await?.credentials,
    ))
}

pub async fn refresh_account(id: &str) -> Result<Account, String> {
    let id = id.to_owned();
    tokio::spawn(async move {
        let mut session = session(&id, false, None, false).await?;
        refresh_at(&store_path()?, &id, &mut session, API).await
    })
    .await
    .map_err(|_| "accountError.auth".to_string())?
}
async fn refresh_at(
    path: &Path,
    id: &str,
    session: &mut NativeAccountSession,
    base: &str,
) -> Result<Account, String> {
    let mut result = fetch_summary(base, &session.credentials).await;
    if result
        .as_ref()
        .err()
        .is_some_and(|e| e == "accountError.loginRequired")
    {
        let rejected = format!("pair:{}", pair(&session.credentials));
        session.credentials =
            rotate_locked(path, id, true, Some(&rejected), base, &mut session.native).await?;
        result = fetch_summary(base, &session.credentials).await;
    }
    let (profile, balance, plan) = result?;
    let _store = STORE_LOCK.lock().await;
    let mut store = load(path)?;
    if selected(&store, id)? != session.credentials {
        return Err("accountError.invalidAccount".into());
    }
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    saved.account.email = text(&profile, "email")
        .unwrap_or(&session.credentials.email)
        .to_owned();
    saved.account.name = text(&profile, "displayName")
        .map(str::to_owned)
        .or_else(|| session.credentials.name.clone());
    saved.account.balance_micros = balance["balance"].as_i64();
    saved.account.plan = text(&plan["plan"], "displayName")
        .or_else(|| text(&plan["plan"], "name"))
        .map(str::to_owned);
    saved.account.subscription_end_at = text(&plan, "currentPeriodEnd")
        .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
        .map(|date| date.timestamp())
        .filter(|time| *time > 0);
    let account = saved.account.clone();
    write(path, &store)?;
    Ok(account)
}
async fn fetch_summary(base: &str, c: &Credentials) -> Result<(Value, Value, Value), String> {
    let client = oauth::client()?;
    let profile = oauth::get(&client, base, "/api/v1/users/me", c).await?;
    if profile["id"] != c.user_id {
        return Err("accountError.invalidAccount".into());
    }
    let user =
        url::Url::parse(&format!("{base}/api/v1/users/")).map_err(|_| "accountError.format")?;
    let mut balance_url = user;
    balance_url
        .path_segments_mut()
        .map_err(|_| "accountError.format")?
        .pop_if_empty()
        .push(&c.user_id)
        .push("balance");
    let (balance, plan) = tokio::try_join!(
        oauth::get(&client, base, balance_url.path(), c),
        oauth::get(&client, base, "/api/v1/users/me/plan", c)
    )?;
    if balance.get("userId").is_some_and(|user| user != &c.user_id)
        || !balance.is_object()
        || !(plan.is_null() || plan.is_object())
    {
        return Err("accountError.authResponse".into());
    }
    Ok((profile, balance, plan))
}

pub(crate) async fn import_native() -> Result<super::auto_import::native::Counts, String> {
    let candidates = super::cline_native::discover()?;
    let _guard = STORE_LOCK.lock().await;
    let path = store_path()?;
    let mut rows = load(&path)?;
    let mut counts = super::auto_import::native::Counts::default();
    for c in candidates {
        counts.record((|| {
            let account = summary(&c)?;
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
            let id = account.id.clone();
            rows.insert(
                id.clone(),
                Saved {
                    account,
                    credential: protect(&c)?,
                    native_pair: None,
                },
            );
            if let Err(error) = write(&path, &rows) {
                rows.remove(&id);
                return Err(error);
            }
            Ok(Some(true))
        })());
    }
    Ok(counts)
}

#[cfg(test)]
mod tests;
