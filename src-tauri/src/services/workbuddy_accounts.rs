//! WorkBuddy account authentication. Protocol reference: changexbc/workbuddy-switch (MIT).
//! See tools/workbuddy/workbuddy-switch-LICENSE.txt for the upstream notice.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

#[path = "workbuddy_credits.rs"]
mod credits;
#[path = "workbuddy_history.rs"]
mod history;
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGINS: OnceLock<Mutex<HashMap<String, Login>>> = OnceLock::new();
const LOGIN_TIMEOUT_SECONDS: i64 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edition {
    #[serde(rename = "workbuddy")]
    Cn,
    #[serde(rename = "workbuddyai")]
    Ai,
}
impl Edition {
    fn api(self) -> &'static str {
        match self {
            Self::Cn => "https://www.codebuddy.cn",
            Self::Ai => "https://www.workbuddy.ai",
        }
    }
    fn platform(self) -> &'static str {
        match self {
            Self::Cn => "workbuddy",
            Self::Ai => "workbuddy-ai",
        }
    }
    fn tool(self) -> &'static str {
        match self {
            Self::Cn => "workbuddy",
            Self::Ai => "workbuddyai",
        }
    }
    pub(crate) fn auth_path(self) -> Result<PathBuf, String> {
        let home = dirs::home_dir().ok_or("accountError.home")?;
        #[cfg(windows)]
        let root = dirs::data_local_dir().ok_or("accountError.home")?;
        #[cfg(target_os = "macos")]
        let root = home.join("Library/Application Support");
        #[cfg(not(any(windows, target_os = "macos")))]
        let root = home.join(".local/share");
        let _ = home;
        Ok(root
            .join("CodeBuddyExtension/Data/Public/auth")
            .join(match self {
                Self::Cn => "workbuddy-desktop.info",
                Self::Ai => "workbuddy-desktop-ai.info",
            }))
    }
    fn accepts_domain(self, domain: &str) -> bool {
        let domain = domain.to_ascii_lowercase();
        domain.is_empty()
            || match self {
                Self::Cn => [
                    "codebuddy.cn",
                    "www.codebuddy.cn",
                    "workbuddy.cn",
                    "www.workbuddy.cn",
                ]
                .contains(&domain.as_str()),
                Self::Ai => ["workbuddy.ai", "www.workbuddy.ai"].contains(&domain.as_str()),
            }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseUsage {
    pub used: f64,
    pub unlimited: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub edition: Edition,
    #[serde(default)]
    pub plan: Option<String>,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_usage: Option<EnterpriseUsage>,
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
    #[serde(default)]
    pub daily_claimed_at: Option<i64>,
    pub expires_at: Option<i64>,
    pub active: bool,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    summary: Account,
    session: Value,
}
#[derive(Clone)]
struct Login {
    edition: Edition,
    state: String,
    expires: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    login_id: String,
    verification_uri: String,
    expires_at: i64,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}
fn store() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/workbuddy-accounts"))
}
fn account_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(dir.join(format!("{id}.json")))
}
fn read(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|_| "accountError.read")?)
        .map_err(|_| "accountError.format".into())
}
fn write(path: &Path, value: &impl Serialize) -> Result<(), String> {
    use std::io::Write;
    fs::create_dir_all(path.parent().ok_or("accountError.write")?)
        .map_err(|_| "accountError.write")?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| "accountError.write")?;
        file.write_all(&serde_json::to_vec_pretty(value).map_err(|_| "accountError.format")?)
            .map_err(|_| "accountError.write")?;
        file.sync_all().map_err(|_| "accountError.write")?;
        drop(file);
        fs::rename(&temporary, path).map_err(|_| "accountError.write".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
fn snapshot(edition: Edition, session: Value) -> Result<Saved, String> {
    let profile = &session["account"];
    let uid = text(profile, "uid")
        .or_else(|| text(profile, "id"))
        .ok_or("accountError.invalidAccount")?;
    let auth = &session["auth"];
    let token = &auth["accessToken"];
    if !(token.as_str().is_some_and(|s| !s.is_empty()) || token.get("$wbEncrypted").is_some())
        || !edition.accepts_domain(text(auth, "domain").unwrap_or_default())
    {
        return Err("accountError.invalidAccount".into());
    }
    let id = format!(
        "{:x}",
        Sha256::digest(format!(
            "{}:{uid}:{}",
            edition.tool(),
            text(profile, "enterpriseId").unwrap_or_default()
        ))
    );
    let name = text(profile, "email")
        .or_else(|| text(profile, "nickname"))
        .unwrap_or(uid)
        .to_string();
    Ok(Saved {
        summary: Account {
            id,
            name,
            edition,
            plan: None,
            remaining: None,
            total: None,
            enterprise_usage: None,
            base_remaining: None,
            base_total: None,
            base_reset_at: None,
            reward_remaining: None,
            reward_total: None,
            reward_next_expiry_at: None,
            addon_remaining: None,
            daily_claimed_at: None,
            expires_at: None,
            active: false,
        },
        session,
    })
}
fn load(id: &str, edition: Edition) -> Result<Saved, String> {
    load_at(&store()?, id, edition)
}
fn load_at(dir: &Path, id: &str, edition: Edition) -> Result<Saved, String> {
    let saved: Saved = serde_json::from_value(read(&account_path(dir, id)?)?)
        .map_err(|_| "accountError.invalidAccount")?;
    if saved.summary.edition != edition
        || saved.summary.id != id
        || snapshot(edition, saved.session.clone())?.summary.id != id
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(saved)
}
fn save(saved: &Saved) -> Result<(), String> {
    save_at(&store()?, saved)
}
fn save_at(dir: &Path, saved: &Saved) -> Result<(), String> {
    write(&account_path(dir, &saved.summary.id)?, saved)
}

pub async fn list(edition: Edition) -> Result<Vec<Account>, String> {
    list_from_paths(edition, &store()?, &edition.auth_path()?).await
}

pub(crate) async fn list_saved_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    list_saved_accounts_at(edition, &store()?)
}

fn list_saved_accounts_at(edition: Edition, dir: &Path) -> Result<Vec<Account>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut accounts = Vec::new();
    for entry in fs::read_dir(dir).map_err(|_| "accountError.read")? {
        let path = entry.map_err(|_| "accountError.read")?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let saved: Saved =
            serde_json::from_value(read(&path)?).map_err(|_| "accountError.format")?;
        if saved.summary.edition == edition {
            load_at(dir, &saved.summary.id, edition)?;
            let mut row = saved.summary;
            row.active = false;
            accounts.push(row);
        }
    }
    accounts.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(accounts)
}

pub(crate) async fn proxy_credentials(
    edition: Edition,
    id: &str,
    force_refresh: bool,
) -> Result<Value, String> {
    let native = edition.auth_path()?;
    proxy_credentials_with_rejection_at(
        edition,
        id,
        force_refresh,
        &store()?,
        edition.api(),
        None,
        Some(&native),
    )
    .await
}

pub(crate) async fn proxy_credentials_after_rejection(
    edition: Edition,
    id: &str,
    rejected_credentials: &Value,
) -> Result<Value, String> {
    let rejected_token =
        text(rejected_credentials, "accessToken").ok_or("accountError.loginRequired")?;
    let native = edition.auth_path()?;
    proxy_credentials_with_rejection_at(
        edition,
        id,
        true,
        &store()?,
        edition.api(),
        Some(rejected_token),
        Some(&native),
    )
    .await
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    let saved: Saved = serde_json::from_value(value.clone()).map_err(|_| "backupError.format")?;
    let parsed = snapshot(saved.summary.edition, saved.session)?;
    if parsed.summary.id != saved.summary.id {
        return Err("backupError.format".into());
    }
    Ok(())
}

#[cfg(test)]
async fn proxy_credentials_at(
    edition: Edition,
    id: &str,
    force_refresh: bool,
    dir: &Path,
    token_api: &str,
) -> Result<Value, String> {
    proxy_credentials_with_rejection_at(edition, id, force_refresh, dir, token_api, None, None)
        .await
}

async fn proxy_credentials_with_rejection_at(
    edition: Edition,
    id: &str,
    force_refresh: bool,
    dir: &Path,
    token_api: &str,
    rejected_token: Option<&str>,
    native: Option<&Path>,
) -> Result<Value, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut saved = load_at(dir, id, edition)?;
    if let Some(native) = native {
        adopt_native(&mut saved, native, dir)?;
    }
    if force_refresh
        && (rejected_token.is_none()
            || rejected_token == text(&saved.session["auth"], "accessToken"))
        || credits::timestamp(saved.session["auth"].get("expiresAt"))
            .is_some_and(|t| t <= chrono::Utc::now().timestamp() + 60)
    {
        refresh_token_at(&mut saved, dir, token_api).await?;
        if let Some(native) = native {
            write_native(&saved, native)?;
        }
    }
    let auth = &saved.session["auth"];
    let profile = &saved.session["account"];
    Ok(json!({
        "accountId":saved.summary.id,
        "accessToken":text(auth,"accessToken").ok_or("accountError.loginRequired")?,
        "uid":text(profile,"uid").or_else(||text(profile,"id")).ok_or("accountError.invalidAccount")?,
        "domain":text(auth,"domain").unwrap_or_default(),
        "enterpriseId":text(profile,"enterpriseId"),
        "baseUrl":match edition {
            Edition::Cn => "https://copilot.tencent.com",
            Edition::Ai => "https://www.workbuddy.ai",
        },
    }))
}

async fn list_from_paths(
    edition: Edition,
    dir: &Path,
    auth_path: &Path,
) -> Result<Vec<Account>, String> {
    // Saved accounts are replaced atomically, so reads need not wait for network refreshes.
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let active = read(auth_path)
        .ok()
        .and_then(|v| snapshot(edition, v).ok())
        .map(|s| s.summary.id);
    let mut accounts = Vec::new();
    for entry in fs::read_dir(dir).map_err(|_| "accountError.read")? {
        let path = entry.map_err(|_| "accountError.read")?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let saved: Saved =
            serde_json::from_value(read(&path)?).map_err(|_| "accountError.format")?;
        if saved.summary.edition == edition {
            let mut summary = saved.summary;
            summary.active = active.as_deref() == Some(&summary.id);
            accounts.push(summary);
        }
    }
    accounts.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(accounts)
}
pub async fn delete(edition: Edition, id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    load(id, edition)?;
    fs::remove_file(account_path(&store()?, id)?).map_err(|_| "accountError.write".into())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "accountError.network".into())
}
async fn response(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|_| "accountError.network")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "{}|HTTP {}",
            match status.as_u16() {
                401 => "accountError.loginRequired",
                403 => "accountError.denied",
                429 => "accountError.rateLimited",
                _ => "accountError.network",
            },
            status.as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.format".into())
}
fn success(value: &Value) -> bool {
    matches!(value["code"].as_i64(), Some(0 | 200))
}
fn normalize_auth(mut auth: Value) -> Value {
    for (camel, snake) in [
        ("accessToken", "access_token"),
        ("refreshToken", "refresh_token"),
        ("tokenType", "token_type"),
    ] {
        if auth.get(camel).is_none() {
            if let Some(v) = auth.get(snake).cloned() {
                auth[camel] = v;
            }
        }
    }
    for (absolute, relative) in [
        ("expiresAt", "expiresIn"),
        ("refreshExpiresAt", "refreshExpiresIn"),
    ] {
        if let Some(ts) = credits::timestamp(auth.get(absolute)) {
            auth[absolute] = json!(ts * 1000);
        } else if let Some(seconds) = auth[relative].as_i64() {
            auth[absolute] = json!(chrono::Utc::now().timestamp_millis() + seconds * 1000);
        }
    }
    auth
}
pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
    let body = response(
        client()?
            .post(format!("{}/v2/plugin/auth/state", edition.api()))
            .query(&[("platform", edition.platform())])
            .json(&json!({})),
    )
    .await?;
    if !success(&body) {
        return Err("accountError.auth".into());
    }
    let data = &body["data"];
    let state = text(data, "state")
        .ok_or("accountError.authResponse")?
        .to_string();
    let uri = text(data, "authUrl")
        .or_else(|| text(data, "auth_url"))
        .or_else(|| text(data, "url"))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{}/login?state={state}", edition.api()));
    let url = url::Url::parse(&uri).map_err(|_| "accountError.authResponse")?;
    if url.scheme() != "https"
        || !edition.accepts_domain(url.host_str().unwrap_or_default())
        || url.host_str().is_none()
    {
        return Err("accountError.authResponse".into());
    }
    let expires = chrono::Utc::now().timestamp() + LOGIN_TIMEOUT_SECONDS;
    let login_id = uuid::Uuid::new_v4().to_string();
    let mut pending = LOGINS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "accountError.busy")?;
    pending.retain(|_, login| login.expires > chrono::Utc::now().timestamp());
    pending.insert(
        login_id.clone(),
        Login {
            edition,
            state,
            expires,
        },
    );
    Ok(LoginStart {
        login_id,
        verification_uri: uri,
        expires_at: expires,
    })
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    LOGINS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "accountError.busy")?
        .remove(id);
    Ok(())
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let login = LOGINS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "accountError.busy")?
        .get(id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    if login.expires <= chrono::Utc::now().timestamp() {
        cancel_login(id)?;
        return Err("accountError.expired".into());
    }
    let client = client()?;
    let token = response(
        client
            .get(format!("{}/v2/plugin/auth/token", login.edition.api()))
            .query(&[("state", &login.state)]),
    )
    .await?;
    if !success(&token)
        || text(&token["data"], "accessToken")
            .or_else(|| text(&token["data"], "access_token"))
            .is_none()
    {
        return Ok(None);
    }
    let auth = normalize_auth(token["data"].clone());
    if !login
        .edition
        .accepts_domain(text(&auth, "domain").unwrap_or_default())
    {
        return Err("accountError.authResponse".into());
    }
    let mut request = client
        .get(format!("{}/v2/plugin/login/account", login.edition.api()))
        .query(&[("state", &login.state)])
        .bearer_auth(text(&auth, "accessToken").ok_or("accountError.authResponse")?);
    if let Some(domain) = text(&auth, "domain") {
        request = request.header("X-Domain", domain);
    }
    let profile = response(request).await?;
    if !success(&profile) {
        return Err("accountError.authResponse".into());
    }
    if !login
        .edition
        .accepts_domain(text(&profile["data"], "domain").unwrap_or_default())
    {
        return Err("accountError.authResponse".into());
    }
    let saved = snapshot(
        login.edition,
        json!({"account": profile["data"], "auth": auth}),
    )?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut pending = LOGINS
        .get_or_init(Mutex::default)
        .lock()
        .map_err(|_| "accountError.busy")?;
    if pending
        .get(id)
        .is_none_or(|s| s.expires <= chrono::Utc::now().timestamp())
    {
        return Err("accountError.cancelled".into());
    }
    save(&saved)?;
    super::smart_router::forget_account_authentication(
        match saved.summary.edition {
            Edition::Cn => crate::models::model::AccountProvider::WorkBuddy,
            Edition::Ai => crate::models::model::AccountProvider::WorkBuddyAi,
        },
        &saved.summary.id,
    );
    pending.remove(id);
    Ok(Some(saved.summary))
}
fn authorized(
    client: &reqwest::Client,
    saved: &Saved,
    url: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let auth = &saved.session["auth"];
    let mut request = client
        .post(url)
        .bearer_auth(text(auth, "accessToken").ok_or("accountError.loginRequired")?)
        .header("Accept", "application/json");
    for (header, value) in [
        ("X-Domain", text(auth, "domain")),
        ("X-User-Id", text(&saved.session["account"], "uid")),
        (
            "X-Enterprise-Id",
            text(&saved.session["account"], "enterpriseId"),
        ),
        (
            "X-Tenant-Id",
            text(&saved.session["account"], "enterpriseId"),
        ),
    ] {
        if let Some(value) = value {
            request = request.header(header, value);
        }
    }
    Ok(request)
}
async fn refresh_token(saved: &mut Saved) -> Result<(), String> {
    let token_api = saved.summary.edition.api();
    refresh_token_at(saved, &store()?, token_api).await
}

async fn refresh_token_at(saved: &mut Saved, dir: &Path, token_api: &str) -> Result<(), String> {
    let original_auth = saved.session["auth"].clone();
    let auth = &saved.session["auth"];
    let refresh = text(auth, "refreshToken").ok_or("accountError.loginRequired")?;
    let body = response(
        authorized(
            &client()?,
            saved,
            &format!("{token_api}/v2/plugin/auth/token/refresh"),
        )?
        .header("X-Refresh-Token", refresh)
        .header("X-Auth-Refresh-Source", "plugin")
        .json(&json!({})),
    )
    .await?;
    if !success(&body) {
        return Err("accountError.loginRequired".into());
    }
    let update = normalize_auth(body["data"].clone());
    if text(&update, "accessToken").is_none() {
        return Err("accountError.authResponse".into());
    }
    if !saved
        .summary
        .edition
        .accepts_domain(text(&update, "domain").unwrap_or_default())
    {
        return Err("accountError.authResponse".into());
    }
    let latest = load_at(dir, &saved.summary.id, saved.summary.edition)?;
    if latest.session["auth"] != original_auth {
        *saved = latest;
        return Ok(());
    }
    let target = saved.session["auth"]
        .as_object_mut()
        .ok_or("accountError.invalidAccount")?;
    target.extend(
        update
            .as_object()
            .ok_or("accountError.authResponse")?
            .clone(),
    );
    save_at(dir, saved)
}
fn sync_current(saved: &mut Saved) -> Result<(), String> {
    adopt_native(saved, &saved.summary.edition.auth_path()?, &store()?)
}

fn adopt_native(saved: &mut Saved, path: &Path, dir: &Path) -> Result<(), String> {
    let edition = saved.summary.edition;
    let Ok(session) = read(path) else {
        return Ok(());
    };
    let Ok(current) = snapshot(edition, session) else {
        return Ok(());
    };
    if current.summary.id == saved.summary.id
        && text(&current.session["auth"], "accessToken").is_some()
        && credits::timestamp(current.session["auth"].get("expiresAt"))
            > credits::timestamp(saved.session["auth"].get("expiresAt"))
    {
        saved.session = current.session;
        save_at(dir, saved)?;
    }
    Ok(())
}

// The client keeps working only if it receives the token EchoBird rotated.
fn write_native(saved: &Saved, path: &Path) -> Result<(), String> {
    let Ok(session) = read(path) else {
        return Ok(());
    };
    let signed_in = snapshot(saved.summary.edition, session.clone())
        .is_ok_and(|current| current.summary.id == saved.summary.id);
    if signed_in {
        write(path, &merge_session(session, saved)?)?;
    }
    Ok(())
}

async fn ensure_fresh(saved: &mut Saved) -> Result<(), String> {
    sync_current(saved)?;
    if credits::timestamp(saved.session["auth"].get("expiresAt"))
        .is_some_and(|t| t <= chrono::Utc::now().timestamp() + 60)
    {
        refresh_token(saved).await?;
        write_native(saved, &saved.summary.edition.auth_path()?)?;
    }
    Ok(())
}
pub async fn refresh(edition: Edition, id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut saved = load(id, edition)?;
    refresh_saved(&mut saved, true).await
}

async fn refresh_saved(saved: &mut Saved, sync_checkin_status: bool) -> Result<Account, String> {
    ensure_fresh(saved).await?;
    let result = credits::fetch(saved).await;
    let quota = match result {
        Err(e) if e.starts_with("accountError.loginRequired") => {
            refresh_token(saved).await?;
            credits::fetch(saved).await?
        }
        other => other?,
    };
    let unlimited = quota
        .enterprise_usage
        .as_ref()
        .is_some_and(|usage| usage.unlimited);
    saved.summary.remaining = (!unlimited).then_some(quota.remaining);
    saved.summary.total = (!unlimited).then_some(quota.total);
    saved.summary.enterprise_usage = quota.enterprise_usage;
    saved.summary.base_remaining = quota.base_remaining;
    saved.summary.base_total = quota.base_total;
    saved.summary.base_reset_at = quota.base_reset_at;
    saved.summary.reward_remaining = quota.reward_remaining;
    saved.summary.reward_total = quota.reward_total;
    saved.summary.reward_next_expiry_at = quota.reward_next_expiry_at;
    saved.summary.addon_remaining = quota.addon_remaining;
    saved.summary.expires_at = quota.expires_at;
    saved.summary.plan = quota.plan;
    if sync_checkin_status && saved.summary.edition == Edition::Cn {
        if let Ok(checked_in) = credits::checkin_status(saved).await {
            saved.summary.daily_claimed_at = checked_in.then(|| chrono::Utc::now().timestamp());
        }
    }
    saved.summary.active = read(&saved.summary.edition.auth_path()?)
        .ok()
        .and_then(|v| snapshot(saved.summary.edition, v).ok())
        .is_some_and(|v| v.summary.id == saved.summary.id);
    save(saved)?;
    Ok(saved.summary.clone())
}

pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
    if edition != Edition::Cn {
        return Err("accountError.claimUnavailable".into());
    }
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut saved = load(id, edition)?;
    ensure_fresh(&mut saved).await?;
    if credits::checkin_status(&saved).await != Ok(true) {
        match credits::claim_daily(&saved).await {
            Err(error) if error.starts_with("accountError.loginRequired") => {
                refresh_token(&mut saved).await?;
                credits::claim_daily(&saved).await?;
            }
            result => result?,
        }
    }
    saved.summary.daily_claimed_at = Some(chrono::Utc::now().timestamp());
    save(&saved)?;
    Ok(refresh_saved(&mut saved, false)
        .await
        .unwrap_or_else(|_| saved.summary.clone()))
}
fn merge_session(mut current: Value, saved: &Saved) -> Result<Value, String> {
    if !current.is_object() {
        return Err("accountError.format".into());
    }
    let profile = saved.session["account"].clone();
    let uid = text(&profile, "uid").or_else(|| text(&profile, "id"));
    let mut all = current
        .get("allAccounts")
        .or_else(|| current.get("accounts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    all.retain(|p| text(p, "uid").or_else(|| text(p, "id")) != uid);
    all.push(profile.clone());
    current["account"] = profile;
    current["auth"] = saved.session["auth"].clone();
    let now = chrono::Utc::now().timestamp();
    for (absolute, relative) in [
        ("expiresAt", "expiresIn"),
        ("refreshExpiresAt", "refreshExpiresIn"),
    ] {
        if let Some(expires) = credits::timestamp(current["auth"].get(absolute)) {
            current["auth"][relative] = json!((expires - now).max(0));
        }
    }
    current["accounts"] = json!(all);
    current["allAccounts"] = current["accounts"].clone();
    Ok(current)
}
pub async fn switch(edition: Edition, id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut saved = load(id, edition)?;
    ensure_fresh(&mut saved).await?;
    log::info!("[WorkBuddy] Switching {}: closing client", edition.tool());
    close_app(edition).await?;
    log::info!("[WorkBuddy] Switching {}: client stopped", edition.tool());
    let path = edition.auth_path()?;
    let original = if path.exists() {
        read(&path)?
    } else {
        json!({})
    };
    // Preserve the last credentials written by the official client before replacing them.
    if let Ok(mut current) = snapshot(edition, original.clone()) {
        if let Ok(old) = load(&current.summary.id, edition) {
            current.summary = old.summary;
            // A newer plaintext credential from EchoBird must not be replaced by an older snapshot.
            let live_time = credits::timestamp(current.session["auth"].get("expiresAt"));
            let stored_time = credits::timestamp(old.session["auth"].get("expiresAt"));
            if live_time > stored_time {
                save(&current)?;
                if current.summary.id == id {
                    saved = current;
                }
            }
        }
    }
    let next = merge_session(original.clone(), &saved)?;
    if path.exists() {
        write(
            &store()?.join(format!("{}-auth.backup", edition.tool())),
            &original,
        )?;
    }
    let data_root = dirs::home_dir()
        .ok_or("accountError.home")?
        .join(format!(".{}", edition.platform()));
    history::apply(&path, &data_root, &original, &next)?;
    saved.summary.active = true;
    Ok(saved.summary)
}
#[cfg(windows)]
fn windows_close_script(name: &str) -> String {
    // Electron children can exit between any property read and close/kill call.
    // The final process check is authoritative; a stale process object isn't a failure.
    format!(
        r#"
$ErrorActionPreference='Stop'
$items=@(Get-Process -Name '{name}' -ErrorAction SilentlyContinue)
foreach($item in $items) {{ try {{ if($item.MainWindowHandle -ne 0) {{ [void]$item.CloseMainWindow() }} }} catch {{ }} }}
$deadline=(Get-Date).AddSeconds(8)
while((Get-Process -Name '{name}' -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) {{ Start-Sleep -Milliseconds 200 }}
foreach($item in $items) {{ try {{ if(-not $item.HasExited) {{ $item.Kill() }} }} catch {{ }} }}
$deadline=(Get-Date).AddSeconds(10)
while(Get-Process -Name '{name}' -ErrorAction SilentlyContinue) {{
    if((Get-Date) -ge $deadline) {{ exit 1 }}
    Start-Sleep -Milliseconds 200
}}
exit 0
"#
    )
}

async fn close_app(edition: Edition) -> Result<(), String> {
    #[cfg(windows)]
    {
        let name = match edition {
            Edition::Cn => "WorkBuddy",
            Edition::Ai => "WorkBuddyAI",
        };
        let script = windows_close_script(name);
        let status = crate::utils::process::async_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .map_err(|_| "accountError.failed")?;
        if !status.success() {
            log::warn!("[WorkBuddy] Client shutdown failed: {name}, status={status}");
            return Err("accountError.stopClient".into());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let tool = match edition {
            Edition::Cn => "workbuddy",
            Edition::Ai => "workbuddyai",
        };
        super::process_manager::stop_desktop_for_account(tool).await
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = edition;
        Err("accountError.unavailable".into())
    }
}

pub(crate) async fn import_native(edition: Edition) -> Result<Option<bool>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    import_native_at(edition, &edition.auth_path()?, &store()?)
}
fn import_native_at(edition: Edition, path: &Path, dir: &Path) -> Result<Option<bool>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let saved = snapshot(edition, read(path)?)?;
    if list_saved_accounts_at(edition, dir)?.iter().any(|a| {
        super::auto_import::native::same_account(
            &saved.summary.id,
            &saved.summary.name,
            &a.id,
            &a.name,
        )
    }) {
        return Ok(Some(false));
    }
    save_at(dir, &saved)?;
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_receives_rotations_only_for_its_own_account() {
        let dir = std::env::temp_dir().join(format!("echobird-wb-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let native = dir.join("auth.info");
        write(&native, &session()).unwrap();
        let mut saved = snapshot(Edition::Cn, session()).unwrap();
        saved.session["auth"]["accessToken"] = json!("rotated-access");
        saved.session["auth"]["refreshToken"] = json!("rotated-refresh");
        write_native(&saved, &native).unwrap();
        let written = read(&native).unwrap();
        assert_eq!(written["auth"]["refreshToken"], "rotated-refresh");

        let mut other = session();
        other["account"]["uid"] = json!("someone-else");
        other["account"]["id"] = json!("someone-else");
        write(&native, &other).unwrap();
        write_native(&saved, &native).unwrap();
        assert_ne!(
            read(&native).unwrap()["auth"]["refreshToken"],
            "rotated-refresh"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn explicit_native_scan_preserves_source_and_deduplicates_id_and_email() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("native.json");
        let dir = root.path().join("saved");
        assert_eq!(import_native_at(Edition::Cn, &path, &dir).unwrap(), None);
        assert!(!dir.exists());
        let mut value = session();
        value["account"]["email"] = json!("Fixture@example.test");
        write(&path, &value).unwrap();
        let source = fs::read(&path).unwrap();
        assert_eq!(
            import_native_at(Edition::Cn, &path, &dir).unwrap(),
            Some(true)
        );
        let rows = list_saved_accounts_at(Edition::Cn, &dir).unwrap();
        let own = account_path(&dir, &rows[0].id).unwrap();
        let saved = fs::read(&own).unwrap();
        value["auth"]["accessToken"] = json!("newer-must-not-overwrite");
        write(&path, &value).unwrap();
        assert_eq!(
            import_native_at(Edition::Cn, &path, &dir).unwrap(),
            Some(false)
        );
        assert_eq!(fs::read(&own).unwrap(), saved);
        value["account"]["uid"] = json!("different-id");
        value["account"]["email"] = json!("fixture@EXAMPLE.test");
        write(&path, &value).unwrap();
        assert_eq!(
            import_native_at(Edition::Cn, &path, &dir).unwrap(),
            Some(false)
        );
        assert_eq!(fs::read(&own).unwrap(), saved);
        fs::write(&path, &source).unwrap();
        import_native_at(Edition::Cn, &path, &dir).unwrap();
        assert_eq!(fs::read(&path).unwrap(), source);
        assert!(import_native_at(Edition::Ai, &path, &dir).is_err());
        assert!(list_saved_accounts_at(Edition::Ai, &dir)
            .unwrap()
            .is_empty());
    }

    fn session() -> Value {
        json!({"account":{"uid":"u1","nickname":"Name"},"auth":{"accessToken":"token","domain":"www.codebuddy.cn"}})
    }

    #[tokio::test]
    async fn proxy_reads_only_saved_credentials_and_serializes_own_store_rotation() {
        use axum::{
            http::{HeaderMap, StatusCode},
            routing::any,
            Json, Router,
        };
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        };

        let root =
            std::env::temp_dir().join(format!("echobird-workbuddy-proxy-{}", uuid::Uuid::new_v4()));
        let dir = root.join("saved");
        let native = root.join("native.info");
        fs::create_dir_all(&root).unwrap();
        fs::write(&native, b"native-fixture-untouched").unwrap();
        let native_hash = Sha256::digest(fs::read(&native).unwrap());
        let mut value = session();
        value["auth"]["refreshToken"] = json!("saved-refresh");
        value["auth"]["expiresAt"] = json!(chrono::Utc::now().timestamp() + 3600);
        let mut saved = snapshot(Edition::Cn, value).unwrap();
        save_at(&dir, &saved).unwrap();
        let own_path = account_path(&dir, &saved.summary.id).unwrap();
        let own_before = fs::read(&own_path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let failed_refresh = Arc::new(AtomicBool::new(false));
        let failed = failed_refresh.clone();
        let replaced_during_refresh = Arc::new(AtomicBool::new(false));
        let replace = replaced_during_refresh.clone();
        let mut replacement = serde_json::to_value(&saved).unwrap();
        replacement["session"]["auth"]["accessToken"] = json!("replacement-token");
        replacement["session"]["auth"]["refreshToken"] = json!("replacement-refresh");
        let replacement_raw = serde_json::to_vec(&replacement).unwrap();
        let replacement_path = own_path.clone();
        let rejected_requests = Arc::new(AtomicUsize::new(0));
        let rejected_count = rejected_requests.clone();
        let rejected_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let app = Router::new().route("/inference", any(move |headers: HeaderMap| {
            let barrier = rejected_barrier.clone();
            let rejected_count = rejected_count.clone();
            let rejected = headers.get("authorization").and_then(|value| value.to_str().ok()) == Some("Bearer token");
            async move {
                if rejected {
                    rejected_count.fetch_add(1, Ordering::SeqCst);
                    barrier.wait().await;
                    StatusCode::UNAUTHORIZED
                } else { StatusCode::OK }
            }
        })).fallback(any(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            if replace.load(Ordering::SeqCst) {
                fs::write(&replacement_path, &replacement_raw).unwrap();
            }
            let status = if failed.load(Ordering::SeqCst) { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
            async move { (status, Json(json!({"code":0,"data":{"accessToken":"rotated-token","refreshToken":"next-refresh","domain":"www.codebuddy.cn","expiresAt":chrono::Utc::now().timestamp()+3600}}))) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let rows = list_saved_accounts_at(Edition::Cn, &dir).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].active);
        assert!(rows[0].remaining.is_none());
        assert!(list_saved_accounts_at(Edition::Ai, &dir)
            .unwrap()
            .is_empty());
        assert!(!serde_json::to_string(&rows)
            .unwrap()
            .contains("saved-refresh"));
        let secret = proxy_credentials_at(Edition::Cn, &saved.summary.id, false, &dir, &url)
            .await
            .unwrap();
        assert_eq!(secret["accessToken"], "token");
        assert_eq!(secret["accountId"], saved.summary.id);
        assert_eq!(secret["baseUrl"], "https://copilot.tencent.com");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fs::read(&own_path).unwrap(), own_before);
        assert!(
            proxy_credentials_at(Edition::Cn, &"f".repeat(64), false, &dir, &url)
                .await
                .is_err()
        );
        assert!(
            proxy_credentials_at(Edition::Ai, &saved.summary.id, false, &dir, &url)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        saved.session["auth"]["expiresAt"] = json!(0);
        save_at(&dir, &saved).unwrap();
        let (first, second) = tokio::join!(
            proxy_credentials_at(Edition::Cn, &saved.summary.id, false, &dir, &url),
            proxy_credentials_at(Edition::Cn, &saved.summary.id, false, &dir, &url),
        );
        assert_eq!(first.unwrap()["accessToken"], "rotated-token");
        assert_eq!(second.unwrap()["accessToken"], "rotated-token");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            load_at(&dir, &saved.summary.id, Edition::Cn)
                .unwrap()
                .session["auth"]["refreshToken"],
            "next-refresh"
        );
        saved.session["auth"]["expiresAt"] = json!(chrono::Utc::now().timestamp() + 3600);
        save_at(&dir, &saved).unwrap();
        let client = reqwest::Client::new();
        let inference = || async {
            let rejected = proxy_credentials_at(Edition::Cn, &saved.summary.id, false, &dir, &url)
                .await
                .unwrap();
            let rejected_token = rejected["accessToken"].as_str().unwrap();
            assert_eq!(
                client
                    .get(format!("{url}/inference"))
                    .bearer_auth(rejected_token)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            let latest = proxy_credentials_with_rejection_at(
                Edition::Cn,
                &saved.summary.id,
                true,
                &dir,
                &url,
                Some(rejected_token),
                None,
            )
            .await
            .unwrap();
            assert_eq!(
                client
                    .get(format!("{url}/inference"))
                    .bearer_auth(latest["accessToken"].as_str().unwrap())
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
            latest
        };
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(inference(), inference())
        })
        .await
        .unwrap();
        assert_eq!(first["accessToken"], "rotated-token");
        assert_eq!(second["accessToken"], "rotated-token");
        assert_eq!(rejected_requests.load(Ordering::SeqCst), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        save_at(&dir, &saved).unwrap();
        let before_failure = fs::read(&own_path).unwrap();
        failed_refresh.store(true, Ordering::SeqCst);
        assert!(proxy_credentials_with_rejection_at(
            Edition::Cn,
            &saved.summary.id,
            true,
            &dir,
            &url,
            Some("token"),
            None
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(fs::read(&own_path).unwrap(), before_failure);
        failed_refresh.store(false, Ordering::SeqCst);
        replaced_during_refresh.store(true, Ordering::SeqCst);
        let latest = proxy_credentials_with_rejection_at(
            Edition::Cn,
            &saved.summary.id,
            true,
            &dir,
            &url,
            Some("token"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(latest["accessToken"], "replacement-token");
        assert_eq!(
            load_at(&dir, &saved.summary.id, Edition::Cn)
                .unwrap()
                .session["auth"]["refreshToken"],
            "replacement-refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(Sha256::digest(fs::read(&native).unwrap()), native_hash);
        server.abort();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saved_accounts_without_plan_remain_readable() {
        let mut value = serde_json::to_value(snapshot(Edition::Cn, session()).unwrap()).unwrap();
        value["summary"].as_object_mut().unwrap().remove("plan");
        value["summary"]
            .as_object_mut()
            .unwrap()
            .remove("baseRemaining");
        for key in [
            "baseTotal",
            "baseResetAt",
            "rewardRemaining",
            "rewardTotal",
            "rewardNextExpiryAt",
            "addonRemaining",
        ] {
            value["summary"].as_object_mut().unwrap().remove(key);
        }
        value["summary"]
            .as_object_mut()
            .unwrap()
            .remove("dailyClaimedAt");
        let saved: Saved = serde_json::from_value(value).unwrap();
        assert_eq!(saved.summary.plan, None);
        assert_eq!(saved.summary.base_remaining, None);
        assert_eq!(saved.summary.reward_remaining, None);
        assert_eq!(saved.summary.reward_next_expiry_at, None);
        assert_eq!(saved.summary.addon_remaining, None);
        assert_eq!(saved.summary.daily_claimed_at, None);
    }
    #[tokio::test]
    async fn international_daily_claim_is_rejected_before_loading_credentials() {
        assert_eq!(
            claim_daily(Edition::Ai, "invalid-id").await.unwrap_err(),
            "accountError.claimUnavailable"
        );
    }
    #[tokio::test]
    async fn account_list_does_not_wait_for_quota_refresh_lock() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let saved = snapshot(Edition::Cn, session()).unwrap();
        write(&account_path(&dir, &saved.summary.id).unwrap(), &saved).unwrap();
        let auth_path = dir.join("auth.info");
        write(&auth_path, &session()).unwrap();
        let _refresh_guard = ACCOUNT_LOCK.lock().await;
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            list_from_paths(Edition::Cn, &dir, &auth_path),
        )
        .await;
        fs::remove_dir_all(dir).unwrap();
        let accounts = result
            .expect("local account listing must not wait for network operations")
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, saved.summary.id);
        assert!(accounts[0].active);
    }
    #[test]
    fn editions_and_enterprises_have_distinct_identity() {
        let cn = snapshot(Edition::Cn, session()).unwrap();
        let mut ai = session();
        ai["auth"]["domain"] = json!("www.workbuddy.ai");
        assert_ne!(cn.summary.id, snapshot(Edition::Ai, ai).unwrap().summary.id);
        assert!(snapshot(Edition::Ai, session()).is_err());
        let mut enterprise = session();
        enterprise["account"]["enterpriseId"] = json!("org");
        assert_ne!(
            cn.summary.id,
            snapshot(Edition::Cn, enterprise).unwrap().summary.id
        );
        assert!(!Edition::Ai.accepts_domain("www.workbuddy.ai.evil.test"));
    }
    #[test]
    fn merge_preserves_encrypted_credentials_and_unrelated_fields() {
        let mut source = session();
        source["auth"]["accessToken"] = json!({"$wbEncrypted":true,"envelope":"cipher"});
        let saved = snapshot(Edition::Cn, source.clone()).unwrap();
        let merged = merge_session(
            json!({"extra":42,"allAccounts":[{"uid":"other"},{"uid":"u1","nickname":"old"}]}),
            &saved,
        )
        .unwrap();
        assert_eq!(merged["extra"], 42);
        assert_eq!(merged["auth"], source["auth"]);
        assert_eq!(merged["allAccounts"].as_array().unwrap().len(), 2);
        assert!(!serde_json::to_string(&saved.summary)
            .unwrap()
            .contains("cipher"));
    }
    #[test]
    fn rejects_empty_credentials_and_path_traversal() {
        let mut value = session();
        value["auth"]["accessToken"] = json!("");
        assert!(snapshot(Edition::Cn, value).is_err());
        assert!(account_path(Path::new("store"), "../auth").is_err());
    }
    #[tokio::test]
    async fn expired_and_cancelled_logins_cannot_be_polled() {
        let id = uuid::Uuid::new_v4().to_string();
        LOGINS.get_or_init(Mutex::default).lock().unwrap().insert(
            id.clone(),
            Login {
                edition: Edition::Ai,
                state: "expired-state".into(),
                expires: 1,
            },
        );
        assert_eq!(poll_login(&id).await.unwrap_err(), "accountError.expired");
        assert_eq!(poll_login(&id).await.unwrap_err(), "accountError.cancelled");
        cancel_login(&id).unwrap();
    }
    #[test]
    fn writing_an_account_recalculates_remaining_token_lifetime() {
        let mut value = session();
        value["auth"]["expiresAt"] = json!(1000);
        value["auth"]["expiresIn"] = json!(3600);
        let saved = snapshot(Edition::Cn, value).unwrap();
        let merged = merge_session(json!({}), &saved).unwrap();
        assert_eq!(merged["auth"]["expiresIn"], 0);
        assert_eq!(merged["auth"]["accessToken"], "token");
    }
    #[cfg(windows)]
    fn run_close_fixture(phase: &str, exits: bool) -> bool {
        // Fake process objects reproduce OS exit races without touching real applications.
        let fixture = format!(
            r#"
$global:fixture = [pscustomobject]@{{ MainWindowHandle = {window}; HasExited = $false }}
$global:fixture | Add-Member ScriptMethod CloseMainWindow {{ $this.HasExited = $true; throw 'Process already exited' }}
$global:fixture | Add-Member ScriptMethod Kill {{ $this.HasExited = ${exits}; throw 'Process already exited or access denied' }}
function Get-Process {{ if (-not $global:fixture.HasExited) {{ $global:fixture }} }}
"#,
            window = if phase == "close" { 1 } else { 0 },
            exits = if exits { "true" } else { "false" }
        );
        let script = windows_close_script("EchoBirdTestOnly")
            .replace("AddSeconds(8)", "AddSeconds(0)")
            .replace("AddSeconds(10)", "AddSeconds(0)");
        crate::utils::process::command("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!("{fixture}\n{script}"),
            ])
            .output()
            .unwrap()
            .status
            .success()
    }
    #[cfg(windows)]
    #[test]
    fn closing_a_process_that_exits_during_close_is_successful() {
        assert!(run_close_fixture("close", true));
    }
    #[cfg(windows)]
    #[test]
    fn killing_a_process_that_exits_concurrently_is_successful() {
        assert!(run_close_fixture("kill", true));
    }
    #[cfg(windows)]
    #[test]
    fn a_process_that_remains_running_still_blocks_switching() {
        assert!(!run_close_fixture("kill", false));
    }
    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let path = dir.join("auth.info");
        write(&path, &session()).unwrap();
        write(&path, &json!({"updated":true})).unwrap();
        assert_eq!(read(&path).unwrap(), json!({"updated":true}));
        fs::remove_dir_all(dir).unwrap();
    }
}
