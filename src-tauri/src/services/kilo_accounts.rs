//! Kilo's official device login. Secrets stay in the encrypted EchoBird store.

use super::{cursor_auth, model_manager};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const API: &str = "https://api.kilo.ai";
const LOGIN_SECONDS: i64 = 60;
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGINS: Mutex<BTreeMap<String, Arc<Login>>> = Mutex::new(BTreeMap::new());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub active: bool,
    pub plan: Option<String>,
    pub balance: Option<f64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    email: String,
    credential: String,
    expires_at: i64,
    organization_id: Option<String>,
    organization_name: Option<String>,
    default_model: String,
    balance: Option<f64>,
}

struct Login {
    code: String,
    expires: i64,
    cancel: CancellationToken,
    poll_lock: tokio::sync::Mutex<()>,
    next_poll: Mutex<std::time::Instant>,
}

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/kilo-accounts.json"))
}

fn load() -> Result<BTreeMap<String, Saved>, String> {
    let path = store_path()?;
    if path.exists() {
        cursor_auth::read(&path)
    } else {
        Ok(BTreeMap::new())
    }
}

fn save(accounts: &BTreeMap<String, Saved>) -> Result<(), String> {
    cursor_auth::write(&store_path()?, accounts)
}

fn protect(token: &str) -> Result<String, String> {
    let encrypted = model_manager::encrypt_key_for_storage(token);
    if encrypted.starts_with("enc:v1:") {
        Ok(encrypted)
    } else {
        Err("accountError.keychain".into())
    }
}

fn unprotect(saved: &Saved) -> Result<String, String> {
    if !saved.credential.starts_with("enc:v1:") {
        return Err("accountError.invalidAccount".into());
    }
    let token = model_manager::decrypt_key_for_use(&saved.credential);
    if valid_token(&token) {
        Ok(token)
    } else {
        Err("accountError.keychain".into())
    }
}

fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn account_id(email: &str, org: Option<&str>) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!(
            "kilo:{}:{}",
            email.trim().to_lowercase(),
            org.unwrap_or("")
        ))
    )
}

fn summary(id: &str, saved: &Saved, active: bool) -> Account {
    Account {
        id: id.into(),
        email: saved.email.clone(),
        active,
        plan: saved.organization_name.clone(),
        balance: saved.balance,
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn response(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let result = request.send().await.map_err(|_| "accountError.network")?;
    if matches!(result.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !result.status().is_success() {
        return Err(format!(
            "accountError.network|HTTP {}",
            result.status().as_u16()
        ));
    }
    result
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

async fn profile(token: &str) -> Result<Value, String> {
    response(
        client()?
            .get(format!("{API}/api/profile"))
            .bearer_auth(token),
    )
    .await
}

fn profile_email(profile: &Value) -> Option<&str> {
    profile["user"]["email"]
        .as_str()
        .or_else(|| profile["email"].as_str())
        .filter(|v| !v.trim().is_empty())
}

fn profile_organization(profile: &Value) -> (Option<String>, Option<String>) {
    let orgs = profile["organizations"].as_array();
    let selected = profile["selectedOrganizationId"].as_str();
    let org = orgs
        .and_then(|orgs| orgs.iter().find(|org| org["id"].as_str() == selected))
        .or_else(|| {
            (profile["hasPersonalAccount"] == false)
                .then(|| orgs.and_then(|v| v.first()))
                .flatten()
        });
    (
        org.and_then(|v| v["id"].as_str()).map(str::to_owned),
        org.and_then(|v| v["name"].as_str()).map(str::to_owned),
    )
}

async fn default_model(token: &str, org: Option<&str>) -> Result<String, String> {
    let mut url = reqwest::Url::parse(API).map_err(|_| "accountError.authResponse")?;
    if let Some(org) = org {
        url.path_segments_mut()
            .map_err(|_| "accountError.authResponse")?
            .extend(["api", "organizations", org, "defaults"]);
    } else {
        url.set_path("/api/defaults");
    }
    // Match Kilo's SDK: defaults are optional and must not invalidate a successful login.
    Ok(default_model_from(
        response(client()?.get(url).bearer_auth(token)).await.ok(),
    ))
}

fn default_model_from(value: Option<Value>) -> String {
    value
        .as_ref()
        .and_then(|value| value["defaultModel"].as_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("kilo-auto/free")
        .to_owned()
}

fn approved_saved(value: &Value, profile: &Value, model: String) -> Result<Saved, String> {
    let token = value["token"]
        .as_str()
        .filter(|v| valid_token(v))
        .ok_or("accountError.authResponse")?;
    let email = profile_email(profile)
        .or_else(|| value["userEmail"].as_str())
        .filter(|v| !v.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let (organization_id, organization_name) = profile_organization(profile);
    Ok(Saved {
        email: email.into(),
        credential: protect(token)?,
        expires_at: chrono::Utc::now().timestamp_millis() + 365 * 24 * 60 * 60 * 1000,
        organization_id,
        organization_name,
        default_model: model,
        balance: None,
    })
}

pub async fn start_login() -> Result<LoginStart, String> {
    let started = chrono::Utc::now().timestamp();
    let value = response(
        client()?
            .post(format!("{API}/api/device-auth/codes"))
            .json(&json!({})),
    )
    .await?;
    let code = value["code"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("accountError.authResponse")?;
    let uri = value["verificationUrl"]
        .as_str()
        .ok_or("accountError.authResponse")?;
    let url = reqwest::Url::parse(uri).map_err(|_| "accountError.authResponse")?;
    let host = url.host_str().unwrap_or_default();
    if url.scheme() != "https"
        || !(host == "kilo.ai" || host.ends_with(".kilo.ai"))
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("accountError.authResponse".into());
    }
    let ttl = value["expiresIn"]
        .as_i64()
        .filter(|v| *v > 0)
        .ok_or("accountError.authResponse")?
        .min(LOGIN_SECONDS);
    let expires = started + ttl;
    if expires <= chrono::Utc::now().timestamp() {
        return Err("accountError.expired".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let login = Arc::new(Login {
        code: code.into(),
        expires,
        cancel: CancellationToken::new(),
        poll_lock: tokio::sync::Mutex::new(()),
        next_poll: Mutex::new(std::time::Instant::now()),
    });
    {
        let mut logins = LOGINS.lock().map_err(|_| "accountError.busy")?;
        for old in logins.values() {
            old.cancel.cancel();
        }
        logins.clear();
        logins.insert(id.clone(), login.clone());
    }
    let timer = login.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = timer.cancel.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs((expires - chrono::Utc::now().timestamp()).max(0) as u64)) => timer.cancel.cancel(),
        }
    });
    Ok(LoginStart {
        login_id: id,
        verification_uri: uri.into(),
        expires_at: expires,
    })
}

pub fn cancel_login(id: &str) -> Result<(), String> {
    if let Some(login) = LOGINS.lock().map_err(|_| "accountError.busy")?.remove(id) {
        login.cancel.cancel();
    }
    Ok(())
}

async fn poll_approved(login: &Login) -> Result<Option<Saved>, String> {
    let mut url = reqwest::Url::parse(API).map_err(|_| "accountError.authResponse")?;
    url.path_segments_mut()
        .map_err(|_| "accountError.authResponse")?
        .extend(["api", "device-auth", "codes", &login.code]);
    let result = client()?
        .get(url)
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    match result.status().as_u16() {
        202 => return Ok(None),
        403 => return Err("accountError.auth".into()),
        410 => return Err("accountError.expired".into()),
        _ if !result.status().is_success() => {
            return Err(format!(
                "accountError.network|HTTP {}",
                result.status().as_u16()
            ))
        }
        _ => {}
    }
    let value: Value = result
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    match value["status"].as_str() {
        Some("pending") => return Ok(None),
        Some("denied") => return Err("accountError.auth".into()),
        Some("expired") => return Err("accountError.expired".into()),
        Some("approved") => {}
        _ => return Err("accountError.authResponse".into()),
    }
    let token = value["token"]
        .as_str()
        .filter(|v| valid_token(v))
        .ok_or("accountError.authResponse")?;
    let profile = profile(token).await?;
    let (org, _) = profile_organization(&profile);
    let model = default_model(token, org.as_deref()).await?;
    approved_saved(&value, &profile, model).map(Some)
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let login = LOGINS
        .lock()
        .map_err(|_| "accountError.busy")?
        .get(id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    let _poll = login.poll_lock.lock().await;
    if login.expires <= chrono::Utc::now().timestamp() {
        cancel_login(id)?;
        return Err("accountError.expired".into());
    }
    if login.cancel.is_cancelled() {
        return Err("accountError.cancelled".into());
    }
    {
        let mut next = login.next_poll.lock().map_err(|_| "accountError.busy")?;
        if *next > std::time::Instant::now() {
            return Ok(None);
        }
        *next = std::time::Instant::now() + Duration::from_secs(3);
    }
    let saved = tokio::select! {
        _ = login.cancel.cancelled() => return Err("accountError.cancelled".into()),
        value = poll_approved(&login) => value?,
    };
    let Some(mut saved) = saved else {
        return Ok(None);
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    // Keep cancellation and completion mutually exclusive across the store write.
    let mut logins = LOGINS.lock().map_err(|_| "accountError.busy")?;
    if login.cancel.is_cancelled()
        || login.expires <= chrono::Utc::now().timestamp()
        || !logins
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, &login))
    {
        return Err("accountError.cancelled".into());
    }
    let id = account_id(&saved.email, saved.organization_id.as_deref());
    let mut accounts = load()?;
    saved.balance = accounts.get(&id).and_then(|v| v.balance);
    let account = summary(&id, &saved, false);
    accounts.insert(id, saved);
    save(&accounts)?;
    logins.retain(|_, current| !Arc::ptr_eq(current, &login));
    login.cancel.cancel();
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Kilo,
        &account.id,
    );
    Ok(Some(account))
}

fn native_auth_path() -> Result<PathBuf, String> {
    Ok(std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(
            dirs::home_dir()
                .ok_or("accountError.home")?
                .join(".local/share"),
        )
        .join("kilo/auth.json"))
}

fn native_auth(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let value: Value = cursor_auth::read(path)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err("accountError.format".into())
    }
}

pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    Ok(load()?
        .iter()
        .map(|(id, saved)| summary(id, saved, false))
        .collect())
}

pub async fn list_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let accounts = load()?;
    let native = if super::tool_manager::any_tool_installed(&["kilo", "kilodesktop"]) {
        native_auth(&native_auth_path()?)?
    } else {
        Value::Null
    };
    let active_token = native["kilo"]["access"]
        .as_str()
        .or_else(|| native["kilo"]["key"].as_str());
    let active_org = native["kilo"]["accountId"].as_str();
    Ok(accounts
        .iter()
        .map(|(id, saved)| {
            let active = active_token
                .is_some_and(|token| unprotect(saved).is_ok_and(|v| v == token))
                && active_org == saved.organization_id.as_deref();
            summary(id, saved, active)
        })
        .collect())
}

async fn verified_token(saved: &Saved) -> Result<String, String> {
    if saved.expires_at <= chrono::Utc::now().timestamp_millis() {
        return Err("accountError.loginRequired".into());
    }
    let token = unprotect(saved)?;
    let value = profile(&token).await?;
    if !profile_email(&value).is_some_and(|email| email.eq_ignore_ascii_case(&saved.email)) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(token)
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    let saved = accounts.get_mut(id).ok_or("accountError.invalidAccount")?;
    let token = verified_token(saved).await?;
    let mut request = client()?
        .get(format!("{API}/api/profile/balance"))
        .bearer_auth(token);
    if let Some(org) = &saved.organization_id {
        request = request.header("x-kilocode-organizationid", org);
    }
    let value = response(request).await?;
    let balance = value["balance"]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or("accountError.authResponse")?;
    saved.balance = Some(balance);
    let account = summary(id, saved, false);
    save(&accounts)?;
    Ok(account)
}

fn apply_auth_at(path: &Path, saved: &Saved, token: &str) -> Result<(), String> {
    let mut native = native_auth(path)?;
    native["kilo"] =
        json!({"type":"oauth", "access":token, "refresh":token, "expires":saved.expires_at});
    if let Some(org) = &saved.organization_id {
        native["kilo"]["accountId"] = json!(org);
    }
    cursor_auth::write(path, &native)
}

pub async fn switch_account(id: &str, tool_id: &str) -> Result<Account, String> {
    if !matches!(tool_id, "kilo" | "kilodesktop") {
        return Err("accountError.unavailable".into());
    }
    let _guard = ACCOUNT_LOCK.lock().await;
    let accounts = load()?;
    let saved = accounts.get(id).ok_or("accountError.invalidAccount")?;
    let token = verified_token(saved).await?;
    let auth_path = native_auth_path()?;
    native_auth(&auth_path)?; // Refuse malformed auth before changing model selection.
    if super::tool_manager::get_tool_exe_path("kilodesktop").is_some() {
        super::process_manager::stop_desktop_for_account("kilodesktop").await?;
    }
    super::tool_config_manager::kilo_apply_account_selection(
        tool_id == "kilodesktop",
        &saved.default_model,
        || apply_auth_at(&auth_path, saved, &token),
    )?;
    Ok(summary(id, saved, true))
}

pub async fn capture_current_account() -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let native = native_auth(&native_auth_path()?)?;
    let auth = &native["kilo"];
    let token = auth["access"]
        .as_str()
        .or_else(|| auth["key"].as_str())
        .filter(|v| valid_token(v))
        .ok_or("accountError.loginRequired")?;
    let profile = profile(token).await?;
    let email = profile_email(&profile)
        .ok_or("accountError.authResponse")?
        .to_string();
    let org = auth["accountId"].as_str().map(str::to_owned);
    let org_name = profile["organizations"]
        .as_array()
        .and_then(|orgs| orgs.iter().find(|v| v["id"].as_str() == org.as_deref()))
        .and_then(|v| v["name"].as_str())
        .map(str::to_owned);
    let saved = Saved {
        email,
        credential: protect(token)?,
        expires_at: auth["expires"]
            .as_i64()
            .unwrap_or(chrono::Utc::now().timestamp_millis() + 365 * 24 * 60 * 60 * 1000),
        default_model: default_model(token, org.as_deref()).await?,
        organization_id: org,
        organization_name: org_name,
        balance: None,
    };
    let id = account_id(&saved.email, saved.organization_id.as_deref());
    let result = summary(&id, &saved, true);
    let mut accounts = load()?;
    accounts.insert(id, saved);
    save(&accounts)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Kilo,
        &result.id,
    );
    Ok(result)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    accounts.remove(id);
    save(&accounts)
}

/// Internal gateway access; deliberately absent from Tauri command registration.
pub async fn proxy_credentials(account_id: &str, force_refresh: bool) -> Result<Value, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let accounts = load()?;
    let saved = accounts
        .get(account_id)
        .ok_or("accountError.invalidAccount")?;
    let token = if force_refresh {
        verified_token(saved).await?
    } else {
        if saved.expires_at <= chrono::Utc::now().timestamp_millis() {
            return Err("accountError.loginRequired".into());
        }
        unprotect(saved)?
    };
    let mut base = reqwest::Url::parse(API).map_err(|_| "accountError.authResponse")?;
    if let Some(org) = &saved.organization_id {
        base.path_segments_mut()
            .map_err(|_| "accountError.authResponse")?
            .extend(["api", "organizations", org]);
    } else {
        base.set_path("/api/openrouter");
    }
    Ok(
        json!({"accessToken":token, "organizationId":saved.organization_id, "expiresAt":saved.expires_at, "baseUrl":base.as_str()}),
    )
}

fn imported_expiry(auth: &Value) -> Result<i64, String> {
    if let Some(expires) = auth["expires"].as_i64().filter(|time| *time > 0) {
        return Ok(expires);
    }
    // Native API-key records have no expiry. OAuth records must retain a real deadline.
    if auth["access"].as_str().is_none() && auth["key"].as_str().is_some_and(valid_token) {
        return Ok(i64::MAX);
    }
    auth["access"]
        .as_str()
        .and_then(|token| super::cursor_auth::claims(token).ok())
        .and_then(|claims| claims["exp"].as_i64())
        .and_then(|seconds| seconds.checked_mul(1000))
        .filter(|time| *time > 0)
        .ok_or("accountError.invalidAccount".into())
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let native = native_auth(&native_auth_path()?)?;
    let auth = &native["kilo"];
    let Some(token) = auth["access"]
        .as_str()
        .or_else(|| auth["key"].as_str())
        .filter(|s| valid_token(s))
    else {
        return Ok(None);
    };
    let expires_at = imported_expiry(auth)?;
    let profile = profile(token).await?;
    let email = profile_email(&profile)
        .ok_or("accountError.authResponse")?
        .to_owned();
    let org = auth["accountId"].as_str().map(str::to_owned);
    let id = account_id(&email, org.as_deref());
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut rows = load()?;
    if rows.iter().any(|(existing_id, s)| {
        super::auto_import::native::same_account(&id, &email, existing_id, &s.email)
    }) {
        return Ok(Some(false));
    }
    let model = default_model(token, org.as_deref()).await?;
    rows.insert(
        id,
        Saved {
            email,
            credential: protect(token)?,
            expires_at,
            default_model: model,
            organization_id: org,
            organization_name: None,
            balance: None,
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
mod tests {
    use super::*;
    #[test]
    fn imported_keys_and_oauth_keep_their_real_expiry_semantics() {
        assert_eq!(
            imported_expiry(&json!({"key":"native-api-key"})).unwrap(),
            i64::MAX
        );
        assert_eq!(
            imported_expiry(&json!({"access":"oauth","expires":1234})).unwrap(),
            1234
        );
        assert!(imported_expiry(&json!({"access":"opaque-oauth-without-expiry"})).is_err());
        assert!(imported_expiry(&json!({"key":""})).is_err());
    }
    use std::fs;

    #[tokio::test]
    #[ignore = "explicit unapproved Kilo device lifecycle; ECHOBIRD_LIVE_KILO_LOGIN=1 required; no browser or account save"]
    async fn live_device_login_pending_cancel_probe() -> Result<(), String> {
        if std::env::var("ECHOBIRD_LIVE_KILO_LOGIN").as_deref() != Ok("1") {
            return Err("explicit Kilo login probe flag is required".into());
        }
        fn hash(path: &Path) -> Result<Option<Vec<u8>>, String> {
            match fs::read(path) {
                Ok(bytes) => Ok(Some(Sha256::digest(bytes).to_vec())),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err("login probe snapshot failed".into()),
            }
        }
        struct Cancel(String);
        impl Drop for Cancel {
            fn drop(&mut self) {
                let _ = cancel_login(&self.0);
            }
        }
        let paths = [native_auth_path()?, store_path()?];
        let before = paths
            .iter()
            .map(|path| hash(path))
            .collect::<Result<Vec<_>, _>>()?;
        let started = start_login()
            .await
            .map_err(|_| "device login start failed")?;
        let guard = Cancel(started.login_id.clone());
        eprintln!("provider=Kilo stage=login_start status=ok");
        let login = LOGINS
            .lock()
            .map_err(|_| "login probe busy")?
            .get(&guard.0)
            .cloned()
            .ok_or("login probe missing pending session")?;
        // This HTTP helper never persists an account, even if an unexpected approval arrives.
        let pending = poll_approved(&login)
            .await
            .map_err(|_| "device pending poll failed")?;
        let result = if pending.is_none() {
            eprintln!("provider=Kilo stage=login_pending status=ok");
            Ok(())
        } else {
            Err("unexpected device approval; no account was saved".to_owned())
        };
        cancel_login(&guard.0).map_err(|_| "device cancel failed")?;
        if !matches!(poll_login(&guard.0).await, Err(error) if error == "accountError.cancelled") {
            return Err("late device poll was not cancelled".into());
        }
        eprintln!("provider=Kilo stage=login_cancel_late_poll status=ok");
        let after = paths
            .iter()
            .map(|path| hash(path))
            .collect::<Result<Vec<_>, _>>()?;
        if before != after {
            return Err("device login probe changed native auth or saved accounts".into());
        }
        eprintln!("provider=Kilo stage=login_preservation status=ok nativeAuthHashUnchanged=true ownStoreHashUnchanged=true");
        result
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "explicit reversible native API/same-account apply; ECHOBIRD_LIVE_KILO_NATIVE_APPLY=1 and closed clients required"]
    async fn live_native_api_and_same_account_apply_probe() -> Result<(), String> {
        use super::super::{process_manager, tool_config_manager};

        if std::env::var("ECHOBIRD_LIVE_KILO_NATIVE_APPLY").as_deref() != Ok("1") {
            return Err("explicit Kilo native apply flag is required".into());
        }
        async fn require_closed() -> Result<(), String> {
            let status = crate::utils::process::async_command("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command",
                    "$ErrorActionPreference='Stop'; try { if(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object { $_.Name -in @('kilo.exe','kilo-desktop.exe') }){exit 1}else{exit 0} } catch {exit 2}"])
                .status().await.map_err(|_| "process check failed")?;
            if status.code() != Some(0) {
                return Err("Kilo clients must be closed before this probe".into());
            }
            Ok(())
        }
        fn object(path: &Path) -> Result<Value, String> {
            if !path.exists() {
                return Ok(json!({}));
            }
            let text = fs::read_to_string(path).map_err(|_| "native config read failed")?;
            jsonc_parser::parse_to_serde_value(&text, &Default::default())
                .ok()
                .filter(Value::is_object)
                .ok_or_else(|| "native config parse failed".into())
        }
        fn unrelated_config(mut config: Value, desktop: bool) -> Value {
            if let Some(providers) = config.get_mut("provider").and_then(Value::as_object_mut) {
                providers.remove("echobird");
                if providers.is_empty() {
                    config.as_object_mut().unwrap().remove("provider");
                }
            }
            if !desktop {
                for key in ["model", "small_model"] {
                    config.as_object_mut().unwrap().remove(key);
                }
            }
            config
        }
        type SnapshotFile = (PathBuf, Option<(Vec<u8>, fs::Permissions)>);
        struct Snapshot {
            files: Vec<SnapshotFile>,
            missing_dirs: Vec<PathBuf>,
        }
        impl Snapshot {
            fn take(paths: Vec<PathBuf>) -> Result<Self, String> {
                let mut snapshot = Self {
                    files: Vec::new(),
                    missing_dirs: Vec::new(),
                };
                for path in paths {
                    let value = if path.exists() {
                        Some((
                            fs::read(&path).map_err(|_| "snapshot read failed")?,
                            fs::metadata(&path)
                                .map_err(|_| "snapshot metadata failed")?
                                .permissions(),
                        ))
                    } else {
                        let mut parent = path.parent();
                        while let Some(dir) = parent.filter(|dir| !dir.exists()) {
                            if !snapshot.missing_dirs.contains(&dir.to_path_buf()) {
                                snapshot.missing_dirs.push(dir.to_path_buf());
                            }
                            parent = dir.parent();
                        }
                        None
                    };
                    snapshot.files.push((path, value));
                }
                snapshot
                    .missing_dirs
                    .sort_by_key(|path| std::cmp::Reverse(path.components().count()));
                Ok(snapshot)
            }
            fn restore(&self) -> Result<(), String> {
                let mut failed = false;
                for (path, original) in &self.files {
                    let restored = match original {
                        Some((bytes, permissions)) => fs::write(path, bytes)
                            .and_then(|_| fs::set_permissions(path, permissions.clone())),
                        None if path.exists() => fs::remove_file(path),
                        None => Ok(()),
                    };
                    failed |= restored.is_err();
                }
                for path in &self.missing_dirs {
                    if path.exists() {
                        failed |= fs::remove_dir(path).is_err();
                    }
                }
                for (path, original) in &self.files {
                    failed |= match original {
                        Some((bytes, _)) => !fs::read(path).is_ok_and(|value| value == *bytes),
                        None => path.exists(),
                    };
                }
                if failed {
                    Err("native snapshot restoration failed".into())
                } else {
                    Ok(())
                }
            }
        }
        impl Drop for Snapshot {
            fn drop(&mut self) {
                if self.restore().is_err() {
                    eprintln!("provider=Kilo stage=restore status=failed");
                }
            }
        }

        require_closed().await?;
        for tool in ["kilo", "kilodesktop"] {
            if super::super::tool_manager::get_tool_exe_path(tool).is_none()
                || process_manager::desktop_tool_is_running(tool)
                    .await
                    .map_err(|_| "process check failed")?
            {
                return Err("both Kilo clients must be installed and closed".into());
            }
        }
        let auth_path = native_auth_path()?;
        let original_auth = native_auth(&auth_path).map_err(|_| "native auth read failed")?;
        let native_token = original_auth["kilo"]["access"]
            .as_str()
            .or_else(|| original_auth["kilo"]["key"].as_str())
            .ok_or("native sign-in is missing")?;
        let (id, saved) = {
            let _guard = ACCOUNT_LOCK.lock().await;
            load()?
                .into_iter()
                .find(|(id, saved)| {
                    *id == account_id(&saved.email, saved.organization_id.as_deref())
                        && original_auth["kilo"]["accountId"].as_str()
                            == saved.organization_id.as_deref()
                        && unprotect(saved).is_ok_and(|token| token == native_token)
                })
                .ok_or("no saved account matches the current native sign-in")?
        };
        let token = verified_token(&saved)
            .await
            .map_err(|_| "saved/native identity validation failed")?;
        if token != native_token {
            return Err("saved/native credentials changed before probing".into());
        }
        if native_auth(&auth_path).map_err(|_| "native auth recheck failed")? != original_auth {
            return Err("native sign-in changed before probing".into());
        }
        let home = dirs::home_dir().ok_or("home is unavailable")?;
        let cli_dir = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("kilo");
        let desktop_data = dirs::config_dir()
            .ok_or("desktop data is unavailable")?
            .join("Kilo Desktop");
        let desktop_dir = desktop_data.join("plugins/kilo-server/runtime/config/kilo");
        let preferences = desktop_data.join("plugins/kilo-ui/model-preferences.json");
        let mut paths = vec![
            auth_path.clone(),
            preferences.clone(),
            home.join(".echobird/kilo.json"),
            store_path()?,
        ];
        for dir in [&cli_dir, &desktop_dir] {
            paths.extend(["config.json", "kilo.json", "kilo.jsonc"].map(|name| dir.join(name)));
        }
        let snapshot = Snapshot::take(paths)?;
        let original_store_bytes =
            fs::read(store_path()?).map_err(|_| "own store snapshot failed")?;
        let mut other_auth = original_auth.clone();
        other_auth.as_object_mut().unwrap().remove("kilo");
        let result = async {
            for (tool, dir, desktop) in [
                ("kilo", &cli_dir, false),
                ("kilodesktop", &desktop_dir, true),
            ] {
                require_closed().await?;
                let write_path = if dir.join("kilo.jsonc").exists() {
                    dir.join("kilo.jsonc")
                } else {
                    dir.join("kilo.json")
                };
                let before = object(&write_path)?;
                let prefs_before = object(&preferences)?;
                let auth_before_api =
                    fs::read(&auth_path).map_err(|_| "native auth snapshot failed")?;
                let info = serde_json::from_value(
                    json!({"model":"echo-probe/model", "name":"EchoBird probe",
                    "baseUrl":"http://127.0.0.1:9/v1", "apiKey":"echo-probe-unused"}),
                )
                .map_err(|_| "probe fixture format failed")?;
                if !tool_config_manager::apply_model_to_tool(tool, info)
                    .await
                    .success
                {
                    return Err("native API apply failed".into());
                }
                let readback = tool_config_manager::get_tool_model_info(tool)
                    .await
                    .ok_or("native API readback failed")?;
                let after = object(&write_path)?;
                if readback.model.as_deref() != Some("echo-probe/model")
                    || readback.base_url.as_deref() != Some("http://127.0.0.1:9/v1")
                    || readback.api_key.as_deref() != Some("echo-probe-unused")
                    || unrelated_config(before, desktop) != unrelated_config(after, desktop)
                    || fs::read(&auth_path).map_err(|_| "native auth readback failed")?
                        != auth_before_api
                {
                    return Err("native API selection/preservation failed".into());
                }
                eprintln!("provider=Kilo tool={tool} stage=api_apply_readback status=ok");
                require_closed().await?;
                switch_account(&id, tool)
                    .await
                    .map_err(|_| "same-account native apply failed")?;
                let current = native_auth(&auth_path).map_err(|_| "native auth readback failed")?;
                let mut other_current = current.clone();
                other_current.as_object_mut().unwrap().remove("kilo");
                if other_current != other_auth
                    || current["kilo"]["access"] != token
                    || current["kilo"]["refresh"] != token
                    || current["kilo"]["expires"] != saved.expires_at
                    || current["kilo"]["accountId"].as_str() != saved.organization_id.as_deref()
                {
                    return Err("same-account auth/unrelated credential preservation failed".into());
                }
                let config = object(&write_path)?;
                if config.pointer("/provider/echobird").is_some() {
                    return Err("account apply kept the API provider".into());
                }
                if desktop {
                    let prefs = object(&preferences)?;
                    if prefs["preferred"]["providerID"] != "kilo"
                        || prefs["preferred"]["modelID"] != saved.default_model
                        || prefs["variantSelections"] != prefs_before["variantSelections"]
                        || prefs["autoFreeModelsEnabled"] != prefs_before["autoFreeModelsEnabled"]
                    {
                        return Err("desktop official model/prefs readback failed".into());
                    }
                } else if config["model"] != format!("kilo/{}", saved.default_model)
                    || tool_config_manager::kilo_echobird_model().is_some()
                {
                    return Err("CLI official model readback failed".into());
                }
                if fs::read(store_path()?).map_err(|_| "own store readback failed")?
                    != original_store_bytes
                {
                    return Err("native apply unexpectedly changed the saved account".into());
                }
                require_closed().await?;
                eprintln!("provider=Kilo tool={tool} stage=same_account_apply_readback status=ok");
            }
            Ok(())
        }
        .await;
        snapshot.restore()?;
        eprintln!("provider=Kilo stage=restore status=ok nativeBytesUnchanged=true");
        result
    }

    #[test]
    fn optional_defaults_follow_the_official_sdk_fallback() {
        assert_eq!(default_model_from(None), "kilo-auto/free");
        assert_eq!(default_model_from(Some(json!({}))), "kilo-auto/free");
        assert_eq!(
            default_model_from(Some(json!({"defaultModel":"vendor/model"}))),
            "vendor/model"
        );
    }

    #[test]
    fn summaries_hide_credentials_and_account_identity_is_stable() {
        let saved = Saved {
            email: "user@example.test".into(),
            credential: "fixture-secret".into(),
            expires_at: 42,
            organization_id: None,
            organization_name: None,
            default_model: "vendor/model".into(),
            balance: None,
        };
        let id = account_id(&saved.email, None);
        assert_eq!(id, account_id(" USER@EXAMPLE.TEST ", None));
        assert_ne!(id, account_id(&saved.email, Some("organization")));
        let value = serde_json::to_value(summary(&id, &saved, false)).unwrap();
        assert!(value["balance"].is_null());
        assert!(value.get("credential").is_none());
        assert!(!value.to_string().contains("fixture-secret"));
    }

    #[test]
    fn account_apply_preserves_other_provider_credentials() {
        let dir = std::env::temp_dir().join(format!("echobird-kilo-auth-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("auth.json");
        let other = json!({"type":"api", "key":"other-fixture"});
        cursor_auth::write(
            &path,
            &json!({"custom":other,"kilo":{"type":"api","key":"old"}}),
        )
        .unwrap();
        let saved = Saved {
            email: "user@example.test".into(),
            credential: String::new(),
            expires_at: 42,
            organization_id: Some("org".into()),
            organization_name: None,
            default_model: "test".into(),
            balance: None,
        };
        apply_auth_at(&path, &saved, "fixture-token").unwrap();
        let value = native_auth(&path).unwrap();
        assert_eq!(value["custom"], other);
        assert_eq!(value["kilo"]["type"], "oauth");
        assert_eq!(value["kilo"]["accountId"], "org");
        fs::write(&path, "broken").unwrap();
        assert!(apply_auth_at(&path, &saved, "fixture-token").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn organization_default_is_validated_against_membership() {
        let profile = json!({"selectedOrganizationId":"missing", "hasPersonalAccount":true,
            "organizations":[{"id":"org", "name":"Team"}]});
        assert_eq!(profile_organization(&profile), (None, None));
        let profile = json!({"selectedOrganizationId":"missing", "hasPersonalAccount":false,
            "organizations":[{"id":"org", "name":"Team"}]});
        assert_eq!(
            profile_organization(&profile),
            (Some("org".into()), Some("Team".into()))
        );
    }
}
