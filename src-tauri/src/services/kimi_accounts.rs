//! Kimi account authorization and saved credentials shared by CLI and Desktop.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex, time::Duration};
use tokio_util::sync::CancellationToken;

mod native;
pub(crate) async fn import_native(region: Region) -> Result<Option<bool>, String> {
    let dir = super::tool_config_manager::kimicode::kimicode_dir();
    let Some(token) = native::discover(&dir, region) else {
        return Ok(None);
    };
    let account = profile(&get(region, "/me", &token).await?, region)?;
    let claims = super::cursor_auth::claims(&token.access_token)?;
    let user = claims["user_id"]
        .as_str()
        .or_else(|| claims["sub"].as_str())
        .ok_or("accountError.invalidAccount")?;
    if account.id != account_id(region, user) {
        return Err("accountError.invalidAccount".into());
    }
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
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
    rows.insert(account.id.clone(), Saved { account, token });
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

const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
static ACCOUNTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: Mutex<Option<Pending>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Region {
    Cn,
    Global,
}

impl Region {
    fn host(self) -> &'static str {
        match self {
            Self::Cn => "https://auth.kimi.com",
            Self::Global => "https://auth.kimi.ai",
        }
    }
    pub(crate) fn base(self) -> &'static str {
        match self {
            Self::Cn => "https://api.kimi.com/coding/v1",
            Self::Global => "https://api.kimi.ai/coding/v1",
        }
    }
    fn key(self) -> String {
        if self == Self::Cn {
            return "oauth/kimi-code".into();
        }
        let scope = format!(
            r#"{{"oauthHost":"{}","baseUrl":"{}"}}"#,
            self.host(),
            self.base()
        );
        format!(
            "oauth/kimi-code-env-{}",
            &format!("{:x}", Sha256::digest(scope))[..16]
        )
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub name: String,
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Wallet {
    pub remaining: f64,
    pub total: f64,
    pub currency: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub region: Region,
    pub plan: Option<String>,
    #[serde(default)]
    pub quota_windows: Vec<QuotaWindow>,
    pub wallet: Option<Wallet>,
    pub active: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Token {
    access_token: String,
    refresh_token: String,
    expires_at: f64,
    expires_in: f64,
    scope: String,
    token_type: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    token: Token,
}

#[derive(Clone)]
struct Pending {
    id: String,
    region: Region,
    device_code: String,
    expires_at: i64,
    next_poll_at: i64,
    interval: i64,
    cancel: CancellationToken,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
    pub poll_interval_seconds: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn store() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/kimi-accounts.json"))
}

fn protect(token: &mut Token, encrypt: bool) -> Result<(), String> {
    for value in [&mut token.access_token, &mut token.refresh_token] {
        if value.is_empty() {
            continue;
        }
        if encrypt {
            *value = super::model_manager::encrypt_key_for_storage(value);
            if !value.starts_with("enc:v1:") {
                return Err("accountError.keychain".into());
            }
        } else {
            if !value.starts_with("enc:v1:") {
                return Err("accountError.invalidAccount".into());
            }
            *value = super::model_manager::decrypt_key_for_use(value);
            if value.is_empty() {
                return Err("accountError.keychain".into());
            }
        }
    }
    Ok(())
}

fn load() -> Result<BTreeMap<String, Saved>, String> {
    let path = store()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let mut saved: BTreeMap<String, Saved> = super::cursor_auth::read(&path)?;
    for (id, row) in &mut saved {
        if id != &row.account.id {
            return Err("accountError.invalidAccount".into());
        }
        protect(&mut row.token, false)?;
    }
    Ok(saved)
}

fn save(saved: &BTreeMap<String, Saved>) -> Result<(), String> {
    let mut encrypted = saved.clone();
    for row in encrypted.values_mut() {
        protect(&mut row.token, true)?;
    }
    super::cursor_auth::write(&store()?, &encrypted)
}

fn account_id(region: Region, user: &str) -> String {
    format!(
        "{}:{:x}",
        if region == Region::Cn { "cn" } else { "global" },
        Sha256::digest(user)
    )
}

fn profile(value: &Value, region: Region) -> Result<Account, String> {
    let user = value["user_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    let label = ["email", "nickname", "username"]
        .into_iter()
        .find_map(|key| value[key].as_str().filter(|s| !s.trim().is_empty()))
        .unwrap_or(user);
    let plan = value["user_level_name"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            ["Free", "Plus", "Pro", "Max", "Ultra"]
                .into_iter()
                .find(|p| p.eq_ignore_ascii_case(s.trim()))
                .unwrap_or(s)
                .to_string()
        });
    Ok(Account {
        id: account_id(region, user),
        email: label.into(),
        region,
        plan,
        quota_windows: vec![],
        wallet: None,
        active: false,
    })
}

fn quota_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|v| v.is_finite())
}

fn quota(account: &mut Account, value: &Value) -> Result<(), String> {
    if !value.is_object() || !value["error"].is_null() {
        return Err("accountError.quota".into());
    }
    let usages = match &value["usages"] {
        Value::Null => None,
        Value::Object(usages) => Some(usages),
        _ => return Err("accountError.quota".into()),
    };
    account.quota_windows = [
        "limit_5h",
        "limit_7d",
        "limit_month_total",
        "limit_month_code",
    ]
    .into_iter()
    .filter_map(|name| {
        let entry = usages?.get(name)?;
        let used = quota_number(&entry["used_ratio"])?;
        let reset_at = entry["reset_time"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp());
        Some(QuotaWindow {
            name: name.into(),
            remaining_percent: ((1.0 - used) * 100.0).clamp(0.0, 100.0),
            reset_at,
        })
    })
    .collect();
    let wallet = &value["boosterWallet"];
    account.wallet = if wallet["balance"]["type"] == "BOOSTER" {
        quota_number(&wallet["balance"]["amount"])
            .filter(|v| *v > 0.0)
            .zip(quota_number(&wallet["balance"]["amountLeft"]))
            .map(|(total, remaining)| Wallet {
                total: total / 100_000_000.0,
                remaining: remaining / 100_000_000.0,
                currency: wallet["monthlyChargeLimit"]["currency"]
                    .as_str()
                    .or_else(|| wallet["monthlyUsed"]["currency"].as_str())
                    .map(str::to_string),
            })
    } else {
        None
    };
    Ok(())
}

fn token(value: &Value, previous: Option<&Token>) -> Result<Token, String> {
    let access_token = value["access_token"]
        .as_str()
        .filter(|v| !v.is_empty())
        .ok_or("accountError.authResponse")?;
    let expires_in = value["expires_in"]
        .as_f64()
        .filter(|v| *v > 0.0)
        .ok_or("accountError.authResponse")?;
    Ok(Token {
        access_token: access_token.into(),
        refresh_token: value["refresh_token"]
            .as_str()
            .filter(|v| !v.is_empty())
            .or_else(|| previous.map(|p| p.refresh_token.as_str()))
            .unwrap_or_default()
            .into(),
        expires_at: now() as f64 + expires_in,
        expires_in,
        scope: value["scope"].as_str().unwrap_or_default().into(),
        token_type: value["token_type"].as_str().unwrap_or("Bearer").into(),
    })
}

fn http() -> Result<reqwest::Client, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "X-Msh-Platform",
        reqwest::header::HeaderValue::from_static("echobird"),
    );
    headers.insert(
        "X-Msh-Version",
        reqwest::header::HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("EchoBird/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let error = if matches!(status.as_u16(), 400 | 401 | 403) {
            "accountError.loginRequired"
        } else {
            "accountError.quota"
        };
        return Err(format!("{error}|HTTP {}", status.as_u16()));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

async fn get(region: Region, path: &str, token: &Token) -> Result<Value, String> {
    response(
        http()?
            .get(format!("{}{path}", region.base()))
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await
}

pub(crate) fn list_saved_accounts() -> Result<Vec<Account>, String> {
    Ok(load()?.into_values().map(|s| s.account).collect())
}

pub async fn list() -> Result<Vec<Account>, String> {
    let mut rows = list_saved_accounts()?;
    if !rows.is_empty() && super::tool_manager::any_tool_installed(&["kimicode", "kimidesktop"]) {
        let active = native::active_id(&super::tool_config_manager::kimicode::kimicode_dir());
        for row in &mut rows {
            row.active = active.as_deref() == Some(&row.id);
        }
    }
    Ok(rows)
}

pub fn cancel_login(id: &str) -> Result<(), String> {
    let mut slot = LOGIN.lock().map_err(|_| "accountError.auth")?;
    if slot.as_ref().is_some_and(|p| p.id == id) {
        if let Some(pending) = slot.take() {
            pending.cancel.cancel();
        }
    }
    Ok(())
}

pub async fn start_login(region: Region) -> Result<LoginStart, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let expires_at = now() + 60;
    let cancel = CancellationToken::new();
    let previous = LOGIN
        .lock()
        .map_err(|_| "accountError.auth")?
        .replace(Pending {
            id: id.clone(),
            region,
            device_code: String::new(),
            expires_at,
            next_poll_at: 0,
            interval: 5,
            cancel: cancel.clone(),
        });
    if let Some(previous) = previous {
        previous.cancel.cancel();
    }
    let request = async {
        let value = response(
            http()?
                .post(format!("{}/api/oauth/device_authorization", region.host()))
                .form(&[("client_id", CLIENT_ID)])
                .send()
                .await
                .map_err(|_| "accountError.network")?,
        )
        .await?;
        let verification_uri = value["verification_uri_complete"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.authResponse")?
            .to_string();
        let url = url::Url::parse(&verification_uri).map_err(|_| "accountError.authResponse")?;
        let domain = if region == Region::Cn {
            "kimi.com"
        } else {
            "kimi.ai"
        };
        if url.scheme() != "https"
            || !url
                .host_str()
                .is_some_and(|s| s == domain || s.ends_with(&format!(".{domain}")))
        {
            return Err("accountError.authResponse".into());
        }
        let device_code = value["device_code"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.authResponse")?
            .to_string();
        let mut slot = LOGIN.lock().map_err(|_| "accountError.auth")?;
        let pending = slot
            .as_mut()
            .filter(|p| p.id == id)
            .ok_or("accountError.expired")?;
        pending.device_code = device_code;
        pending.expires_at =
            expires_at.min(now() + value["expires_in"].as_i64().unwrap_or(60).max(1));
        pending.interval = value["interval"].as_i64().unwrap_or(5).clamp(1, 60);
        Ok(LoginStart {
            login_id: id.clone(),
            verification_uri,
            expires_at: pending.expires_at,
            poll_interval_seconds: pending.interval,
        })
    };
    let result = tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("accountError.expired".into()),
        result = request => result,
    };
    if result.is_err() {
        cancel_login(&id)?;
    }
    result
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let pending = {
        let mut slot = LOGIN.lock().map_err(|_| "accountError.auth")?;
        let pending = slot
            .as_mut()
            .filter(|p| p.id == id && p.expires_at > now())
            .ok_or("accountError.expired")?;
        if pending.next_poll_at > now() {
            return Ok(None);
        }
        pending.next_poll_at = now() + pending.interval;
        pending.clone()
    };
    let cancel = pending.cancel.clone();
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err("accountError.expired".into()),
        result = poll_pending(pending) => result,
    }
}

async fn poll_pending(pending: Pending) -> Result<Option<Account>, String> {
    let id = pending.id.as_str();
    let result = http()?
        .post(format!("{}/api/oauth/token", pending.region.host()))
        .form(&[
            ("client_id", CLIENT_ID),
            ("device_code", pending.device_code.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let status = result.status();
    let value: Value = result
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    if !status.is_success() {
        return match value["error"].as_str() {
            Some("authorization_pending") => Ok(None),
            Some("slow_down") => {
                if let Some(p) = LOGIN
                    .lock()
                    .map_err(|_| "accountError.auth")?
                    .as_mut()
                    .filter(|p| p.id == id)
                {
                    p.interval = (p.interval + 5).min(60);
                    p.next_poll_at = now() + p.interval;
                }
                Ok(None)
            }
            Some("expired_token") => Err("accountError.expired".into()),
            _ => Err("accountError.auth".into()),
        };
    }
    let token = token(&value, None)?;
    let account = profile(&get(pending.region, "/me", &token).await?, pending.region)?;
    let _guard = ACCOUNTS.lock().await;
    let mut saved = load()?;
    let mut slot = LOGIN.lock().map_err(|_| "accountError.auth")?;
    if !slot
        .as_ref()
        .is_some_and(|p| p.id == id && p.expires_at > now())
    {
        return Ok(None);
    }
    let mut account = account;
    if let Some(previous) = saved.get(&account.id) {
        account
            .quota_windows
            .clone_from(&previous.account.quota_windows);
        account.wallet.clone_from(&previous.account.wallet);
    }
    saved.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            token,
        },
    );
    save(&saved)?;
    *slot = None;
    Ok(Some(account))
}

async fn refresh_token(row: &mut Saved) -> Result<(), String> {
    if row.token.refresh_token.is_empty() {
        return Err("accountError.loginRequired".into());
    }
    let result = response(
        http()?
            .post(format!("{}/api/oauth/token", row.account.region.host()))
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", row.token.refresh_token.as_str()),
            ])
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    row.token = token(&result, Some(&row.token))?;
    Ok(())
}

pub(crate) async fn proxy_credentials(id: &str, force: bool) -> Result<Value, String> {
    let _guard = ACCOUNTS.lock().await;
    let mut saved = load()?;
    let row = saved.get_mut(id).ok_or("accountError.invalidAccount")?;
    let dir = super::tool_config_manager::kimicode::kimicode_dir();
    let native = native::token_for(&dir, &row.account);
    let client_holds_token = native.is_some();
    let client_rotated = native.filter(|native| {
        native.access_token != row.token.access_token && native.expires_at > now() as f64 + 60.0
    });
    if force || row.token.expires_at <= now() as f64 + 60.0 {
        if let Some(native) = client_rotated {
            row.token = native;
        } else {
            refresh_token(row).await?;
            if client_holds_token {
                native::store_token(&dir, row.account.region, &row.token)?;
            }
        }
        save(&saved)?;
    } else if let Some(native) = client_rotated.filter(|n| n.expires_at > row.token.expires_at) {
        row.token = native;
        save(&saved)?;
    }
    let row = saved.get(id).ok_or("accountError.invalidAccount")?;
    Ok(
        json!({"accessToken":row.token.access_token,"baseUrl":row.account.region.base(),"accountId":id,"headers":{"User-Agent":concat!("EchoBird/",env!("CARGO_PKG_VERSION"))}}),
    )
}

pub(crate) async fn credentials_after_rejection(
    id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    let current = proxy_credentials(id, false).await?;
    if current["accessToken"] != rejected["accessToken"] {
        return Ok(current);
    }
    proxy_credentials(id, true).await
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let secret = proxy_credentials(id, false).await?;
    let row = load()?.remove(id).ok_or("accountError.invalidAccount")?;
    let token = row.token;
    let region = row.account.region;
    let fetch: Result<Account, String> = async {
        let user = get(region, "/me", &token).await?;
        let mut account = profile(&user, region)?;
        if account.id != id {
            return Err("accountError.invalidAccount".into());
        }
        quota(&mut account, &get(region, "/usages", &token).await?)?;
        Ok(account)
    }
    .await;
    let account = match fetch {
        Err(error) if error.starts_with("accountError.loginRequired") => {
            credentials_after_rejection(id, &secret).await?;
            let saved = load()?.remove(id).ok_or("accountError.invalidAccount")?;
            let mut account = profile(&get(region, "/me", &saved.token).await?, region)?;
            if account.id != id {
                return Err("accountError.invalidAccount".into());
            }
            quota(&mut account, &get(region, "/usages", &saved.token).await?)?;
            account
        }
        result => result?,
    };
    let _guard = ACCOUNTS.lock().await;
    let mut saved = load()?;
    saved
        .get_mut(id)
        .ok_or("accountError.invalidAccount")?
        .account = account.clone();
    save(&saved)?;
    Ok(account)
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let secret = proxy_credentials(id, false).await?;
    let models = response(
        http()?
            .get(format!(
                "{}/models",
                secret["baseUrl"]
                    .as_str()
                    .ok_or("accountError.invalidAccount")?
            ))
            .bearer_auth(
                secret["accessToken"]
                    .as_str()
                    .ok_or("accountError.invalidAccount")?,
            )
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let _guard = ACCOUNTS.lock().await;
    let saved = load()?.remove(id).ok_or("accountError.invalidAccount")?;
    native::apply_at(
        &super::tool_config_manager::kimicode::kimicode_dir(),
        &saved,
        &models,
    )?;
    Ok(Account {
        active: true,
        ..saved.account
    })
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNTS.lock().await;
    let mut saved = load()?;
    saved.remove(id);
    save(&saved)?;
    super::account_proxy::forget_preparation_metadata(
        crate::models::model::AccountProvider::Kimi,
        id,
    );
    Ok(())
}
