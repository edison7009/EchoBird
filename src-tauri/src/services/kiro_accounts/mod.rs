mod native;
mod oauth;
mod proxy;
mod quota;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static REQUEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const LEGACY_BUILDER_PROFILE: &str =
    "arn:aws:codewhisperer:us-east-1:638616132270:profile/AAAACCCCXXXX";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edition {
    #[serde(rename = "kirocrew")]
    Crew,
    #[serde(rename = "kirocli")]
    Cli,
}

impl Edition {
    pub fn tool(self) -> &'static str {
        match self {
            Self::Crew => "kirocrew",
            Self::Cli => "kirocli",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub edition: Edition,
    pub name: String,
    pub plan: Option<String>,
    pub active: bool,
    #[serde(default)]
    pub quota: Vec<Quota>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub label: String,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Credentials {
    token: Value,
    #[serde(default)]
    registration: Option<Value>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(skip)]
    usage: Option<Value>,
    #[serde(skip)]
    source: Option<Edition>,
    #[serde(skip)]
    source_path: Option<PathBuf>,
}

impl Credentials {
    fn access(&self) -> Result<&str, String> {
        text(&self.token, "access_token").ok_or_else(|| "accountError.invalidAccount".into())
    }

    fn provider(&self) -> &str {
        text(&self.token, "provider").unwrap_or("BuilderId")
    }

    fn is_social(&self) -> bool {
        matches!(
            self.provider().to_ascii_lowercase().as_str(),
            "google" | "github"
        )
    }

    fn profile(&self) -> Option<&str> {
        text(&self.token, "profile_arn").filter(|profile| *profile != LEGACY_BUILDER_PROFILE)
    }

    fn region(&self) -> Result<String, String> {
        let region = self
            .profile()
            .and_then(|v| v.split(':').nth(3))
            .or_else(|| text(&self.token, "region"))
            .unwrap_or("us-east-1");
        if !valid_region(region) {
            return Err("accountError.format".into());
        }
        Ok(region.into())
    }

    fn identity(&self) -> Option<String> {
        let key = if self.is_social() {
            self.profile()
        } else {
            self.subject.as_deref()
        }?;
        Some(format!("{:x}", Sha256::digest(key)))
    }

    fn auth_region(&self) -> Result<String, String> {
        let region = text(&self.token, "region").unwrap_or("us-east-1");
        if !valid_region(region) {
            return Err("accountError.format".into());
        }
        Ok(region.into())
    }

    fn expiry(&self) -> Option<i64> {
        text(&self.token, "expires_at")
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.timestamp())
    }
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|v| !v.trim().is_empty())
}

fn valid_region(region: &str) -> bool {
    region.len() <= 32
        && region.split('-').count() >= 3
        && region
            .bytes()
            .all(|v| v.is_ascii_lowercase() || v.is_ascii_digit() || v == b'-')
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("Kiro-CLI")
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let error = if matches!(status.as_u16(), 401 | 403) {
            "accountError.loginRequired"
        } else {
            "accountError.network"
        };
        return Err(format!("{error}|HTTP {}", status.as_u16()));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    native_pairs: BTreeMap<String, String>,
}
type Store = BTreeMap<String, Saved>;

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/kiro-accounts.json"))
}
fn load() -> Result<Store, String> {
    load_at(&store_path()?)
}
fn load_at(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    super::cursor_auth::read(path)
}
fn save(store: &Store) -> Result<(), String> {
    save_at(&store_path()?, store)
}
fn save_at(path: &Path, store: &Store) -> Result<(), String> {
    #[cfg(test)]
    if proxy::tests::failed_writes().lock().unwrap().contains(path) {
        return Err("accountError.write".into());
    }
    super::cursor_auth::write(path, store)
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
        .filter(|value| {
            value.len() >= 56
                && value.len() % 2 == 0
                && value.bytes().all(|v| v.is_ascii_hexdigit())
        })
        .ok_or("accountError.keychain")?;
    let raw = super::model_manager::decrypt_key_for_use(&saved.credential);
    let credentials: Credentials =
        serde_json::from_str(&raw).map_err(|_| "accountError.keychain")?;
    if credentials
        .identity()
        .is_some_and(|id| id != saved.account.id)
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(credentials)
}

fn current_id(credentials: &Credentials, store: &Store) -> Option<String> {
    credentials.identity().or_else(|| {
        store.iter().find_map(|(id, saved)| {
            let previous = unprotect(saved).ok()?;
            let same = previous.access().ok() == credentials.access().ok()
                || text(&previous.token, "refresh_token")
                    .is_some_and(|token| Some(token) == text(&credentials.token, "refresh_token"));
            same.then(|| id.clone())
        })
    })
}

fn capture(
    edition: Edition,
    credentials: &Credentials,
    store: &mut Store,
) -> Result<Account, String> {
    let id = current_id(credentials, store).ok_or("accountError.invalidAccount")?;
    let mut credentials = credentials.clone();
    if let Some(previous) = store.get(&id).and_then(|saved| unprotect(saved).ok()) {
        if credentials.subject.is_none() {
            credentials.subject = previous.subject;
        }
        if credentials.name.is_none() {
            credentials.name = previous.name;
        }
    }
    let mut account = store
        .get(&id)
        .map(|saved| saved.account.clone())
        .unwrap_or_else(|| Account {
            id: id.clone(),
            edition,
            name: credentials
                .name
                .clone()
                .unwrap_or_else(|| credentials.provider().into()),
            plan: None,
            active: false,
            quota: Vec::new(),
        });
    if let Some(usage) = &credentials.usage {
        quota::update(&mut account, usage);
    }
    if let Some(name) = &credentials.name {
        account.name = name.clone();
    }
    let mut native_pairs = store
        .get(&id)
        .filter(|saved| {
            unprotect(saved)
                .is_ok_and(|previous| proxy::pair(&previous) == proxy::pair(&credentials))
        })
        .map(|saved| saved.native_pairs.clone())
        .unwrap_or_default();
    if let (Some(source), Some(path)) = (credentials.source, &credentials.source_path) {
        native_pairs.insert(native::binding_key(source, path), proxy::pair(&credentials));
    }
    store.insert(
        id,
        Saved {
            account: account.clone(),
            credential: protect(&credentials)?,
            native_pairs,
        },
    );
    Ok(account)
}

fn installed(edition: Edition) -> bool {
    super::tool_manager::get_tool_exe_path(edition.tool()).is_some()
}

fn native_is_newer(current: &Credentials, saved: &Saved) -> bool {
    unprotect(saved).is_ok_and(|previous| credentials_newer(current, &previous))
}

fn credentials_newer(current: &Credentials, previous: &Credentials) -> bool {
    current
        .expiry()
        .zip(previous.expiry())
        .is_some_and(|(current, previous)| current >= previous)
}

pub async fn list_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    let _request = REQUEST_LOCK.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    let first_load = !store_path()?.exists();
    let mut store = load()?;
    if first_load {
        for source in [Edition::Crew, Edition::Cli] {
            if installed(source) {
                if let Ok(Some(credentials)) = native::read(source) {
                    if current_id(&credentials, &store).is_some_and(|id| {
                        store
                            .get(&id)
                            .is_none_or(|saved| native_is_newer(&credentials, saved))
                    }) {
                        let _ = capture(source, &credentials, &mut store);
                    }
                }
            }
        }
        if !store.is_empty() {
            let _ = save(&store);
        }
    }
    let current = if installed(edition) {
        native::read(edition).ok().flatten()
    } else {
        None
    };
    let active = current
        .as_ref()
        .and_then(|current| current_id(current, &store));
    if let (Some(current), Some(id)) = (&current, &active) {
        if store
            .get(id)
            .is_some_and(|saved| native_is_newer(current, saved))
            && proxy::may_adopt_native(current, &store[id], edition)
            && capture(edition, current, &mut store).is_ok()
        {
            let _ = save(&store);
        }
    }
    Ok(store
        .into_values()
        .map(|mut saved| {
            saved.account.edition = edition;
            saved.account.active = active.as_deref() == Some(saved.account.id.as_str());
            saved.account
        })
        .collect())
}

pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
    if !installed(edition) {
        return Err("accountError.initializeClient".into());
    }
    oauth::start(edition).await
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some((edition, credentials)) = oauth::poll(id)? else {
        return Ok(None);
    };
    let _request = REQUEST_LOCK.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    oauth::complete(id, || {
        let mut store = load()?;
        let account = capture(edition, &credentials, &mut store)?;
        save(&store)?;
        proxy::forget_pending(&store_path()?, &account.id);
        Ok(Some(account))
    })
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    oauth::cancel(id)
}
pub async fn delete_account(_edition: Edition, id: &str) -> Result<(), String> {
    let _request = REQUEST_LOCK.lock().await;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load()?;
    store.remove(id);
    save(&store)?;
    proxy::forget_pending(&store_path()?, id);
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Kiro,
        id,
    );
    Ok(())
}

pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    proxy::list_at(&store_path()?)
}

pub(crate) async fn proxy_credentials(id: &str, force: bool) -> Result<Value, String> {
    proxy::credentials(id, force, None).await
}

pub(crate) fn connection_metadata(id: &str) -> Result<Value, String> {
    let accounts = load()?;
    let saved = accounts
        .get(id)
        .filter(|saved| saved.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    Ok(serde_json::json!({"region": unprotect(saved)?.region()?}))
}

pub(crate) async fn proxy_credentials_after_rejection(
    id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    proxy::credentials(id, true, Some(rejected)).await
}

pub async fn refresh_quota(edition: Edition, id: &str) -> Result<Account, String> {
    let id = id.to_owned();
    proxy::complete(async move {
        let _request = REQUEST_LOCK.lock().await;
        if !installed(edition) {
            return Err("accountError.initializeClient".into());
        }
        let path = store_path()?;
        let targets = native::targets()?;
        let running = proxy::running(&path, &id, &targets).await?;
        let mut credentials =
            proxy::obtain_locked(&path, &id, false, None, &targets, running, None).await?;
        let usage = quota::fetch(&mut credentials).await?;
        let _guard = ACCOUNT_LOCK.lock().await;
        let mut store = load_at(&path)?;
        let saved = store.get_mut(&id).ok_or("accountError.cancelled")?;
        if proxy::pair(&unprotect(saved)?) != proxy::pair(&credentials) {
            return Err("accountError.cancelled".into());
        }
        saved.account.name = credentials
            .name
            .clone()
            .unwrap_or_else(|| saved.account.name.clone());
        saved.account.edition = edition;
        quota::update(&mut saved.account, &usage);
        saved.credential = protect(&credentials)?;
        let account = saved.account.clone();
        save_at(&path, &store)?;
        Ok(account)
    })
    .await
}

pub async fn activate_account(edition: Edition, id: &str) -> Result<(), String> {
    let id = id.to_owned();
    proxy::complete(async move {
        let _request = REQUEST_LOCK.lock().await;
        if !installed(edition) {
            return Err("accountError.initializeClient".into());
        }
        if edition == Edition::Cli
            && super::process_manager::desktop_tool_is_running(edition.tool()).await?
        {
            return Err("accountError.closeClient".into());
        }
        let path = store_path()?;
        {
            let _guard = ACCOUNT_LOCK.lock().await;
            proxy::current(&path, &id)?;
        }
        if edition == Edition::Crew {
            super::process_manager::stop_desktop_for_account(edition.tool()).await?;
        }
        let targets = native::targets()?;
        let running = proxy::running(&path, &id, &targets).await?;
        let credentials =
            proxy::obtain_locked(&path, &id, false, None, &targets, running, None).await?;
        let target = native::target_for_apply(edition)?;
        let mut session = native::Session::open_for_apply(edition, &target)?;
        proxy::apply_at(&path, &id, &credentials, &mut session).await
    })
    .await
}
