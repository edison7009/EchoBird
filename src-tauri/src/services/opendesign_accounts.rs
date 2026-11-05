//! Open Design Cloud device authorization and production-profile accounts.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, path::PathBuf, sync::Mutex, time::Duration};

pub(crate) const CONTROL: &str = "https://amr-api.open-design.ai";
pub(crate) const LINK: &str = "https://amr-link.open-design.ai/v1";
static ACCOUNTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: Mutex<Option<Pending>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub label: String,
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub active: bool,
    pub plan: Option<String>,
    #[serde(default)]
    pub quota_windows: Vec<QuotaWindow>,
    pub balance_usd: Option<f64>,
    pub subscription_end_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
    pub poll_interval_seconds: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    control_key: String,
    runtime_key: String,
    user: Value,
}
type Store = BTreeMap<String, Saved>;

#[derive(Clone)]
struct Pending {
    id: String,
    device_id: String,
    secret: String,
    expires: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or("accountError.home".into())
}

fn native_path() -> Result<PathBuf, String> {
    let dir = match std::env::var("AMR_HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        Some(value) if value.trim() == "~" => home()?,
        Some(value) => super::tool_manager::expand_path(value.trim()),
        None => home()?.join(".amr"),
    };
    Ok(dir.join("config.json"))
}

fn store_path() -> Result<PathBuf, String> {
    Ok(home()?.join(".echobird/opendesign-accounts.json"))
}

fn read_config(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let value: Value = super::cursor_auth::read(path)?;
    value
        .is_object()
        .then_some(value)
        .ok_or("accountError.format".into())
}

fn account_from_profile(profile: &Value) -> Result<Saved, String> {
    let user = &profile["user"];
    let uid = user["id"]
        .as_str()
        .or_else(|| user["appUserId"].as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let email = user["email"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let runtime = profile["runtimeKey"]
        .as_str()
        .filter(|s| s.starts_with("sk-amr-"))
        .ok_or("accountError.authResponse")?;
    let control = profile["controlKey"]
        .as_str()
        .filter(|s| s.starts_with("vela_ctrl_"))
        .ok_or("accountError.authResponse")?;
    Ok(Saved {
        account: Account {
            id: format!("{:x}", Sha256::digest(uid)),
            email: email.into(),
            active: false,
            plan: user["plan"].as_str().map(str::to_owned),
            quota_windows: Vec::new(),
            balance_usd: None,
            subscription_end_at: None,
        },
        control_key: control.into(),
        runtime_key: runtime.into(),
        user: user.clone(),
    })
}

fn native_at(path: &Path) -> Result<Option<Saved>, String> {
    let config = read_config(path)?;
    let profile = &config["profiles"]["prod"];
    if profile["runtimeKey"].as_str().is_none_or(str::is_empty) {
        return Ok(None);
    }
    for (key, expected) in [("apiUrl", CONTROL), ("linkUrl", LINK)] {
        if profile[key]
            .as_str()
            .is_some_and(|url| url.trim_end_matches('/') != expected)
        {
            return Err("accountError.invalidAccount".into());
        }
    }
    account_from_profile(profile).map(Some)
}

fn protect(secret: &str) -> Result<String, String> {
    let encrypted = super::model_manager::encrypt_key_for_storage(secret);
    encrypted
        .starts_with("enc:v1:")
        .then_some(encrypted)
        .ok_or("accountError.keychain".into())
}

fn load_at(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    let mut saved: Store = super::cursor_auth::read(path)?;
    for (id, row) in &mut saved {
        for key in [&mut row.runtime_key, &mut row.control_key] {
            if !key.starts_with("enc:v1:") {
                return Err("accountError.invalidAccount".into());
            }
            *key = super::model_manager::decrypt_key_for_use(key);
        }
        let parsed = account_from_profile(
            &json!({"user":row.user,"runtimeKey":row.runtime_key,"controlKey":row.control_key}),
        )?;
        if id != &parsed.account.id || id != &row.account.id {
            return Err("accountError.invalidAccount".into());
        }
    }
    Ok(saved)
}

fn save_at(path: &Path, rows: &Store) -> Result<(), String> {
    let mut encrypted = rows.clone();
    for saved in encrypted.values_mut() {
        saved.control_key = protect(&saved.control_key)?;
        saved.runtime_key = protect(&saved.runtime_key)?;
    }
    super::cursor_auth::write(path, &encrypted)
}

fn remember(rows: &mut Store, mut saved: Saved) -> Account {
    if let Some(previous) = rows.get(&saved.account.id) {
        saved.account.plan = previous.account.plan.clone().or(saved.account.plan);
        saved.account.quota_windows = previous.account.quota_windows.clone();
        saved.account.balance_usd = previous.account.balance_usd;
        saved.account.subscription_end_at = previous.account.subscription_end_at;
    }
    let result = saved.account.clone();
    rows.insert(result.id.clone(), saved);
    result
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNTS.lock().await;
    let path = store_path()?;
    let mut rows = load_at(&path)?;
    let active = if super::tool_manager::any_tool_installed(&["opendesign"]) {
        native_at(&native_path()?)?
    } else {
        None
    };
    let active_id = active.as_ref().map(|a| a.account.id.clone());
    if let Some(saved) = active {
        remember(&mut rows, saved);
        save_at(&path, &rows)?;
    }
    Ok(rows
        .values()
        .map(|saved| {
            let mut account = saved.account.clone();
            account.active = active_id.as_deref() == Some(&account.id);
            account
        })
        .collect())
}

pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNTS.lock().await;
    Ok(load_at(&store_path()?)?
        .into_values()
        .map(|row| row.account)
        .collect())
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(if matches!(status.as_u16(), 401 | 403) {
            format!("accountError.loginRequired|HTTP {status}")
        } else {
            format!("accountError.auth|HTTP {status}")
        });
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

fn authorization_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("open-design.ai")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
            && url.path().starts_with("/cloud/cli/activate")
    })
}

async fn start_device(client: &reqwest::Client, control: &str) -> Result<Value, String> {
    response_json(
        client
            .post(format!("{control}/api/v1/cli/device-authorizations"))
            .json(&json!({"cliVersion":"0.1.4","host":"EchoBird","profile":"prod"}))
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await
}

pub async fn start_login() -> Result<LoginStart, String> {
    let id = uuid::Uuid::new_v4().to_string();
    *LOGIN.lock().map_err(|_| "accountError.busy")? = Some(Pending {
        id: id.clone(),
        device_id: String::new(),
        secret: String::new(),
        expires: now() + 60,
    });
    let result = async {
        let data = start_device(&client()?, CONTROL).await?;
        let url = data["activationUrl"]
            .as_str()
            .filter(|url| authorization_url(url))
            .ok_or("accountError.authResponse")?;
        let device_id = data["deviceId"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.authResponse")?;
        let secret = data["deviceSecret"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("accountError.authResponse")?;
        let mut login = LOGIN.lock().map_err(|_| "accountError.busy")?;
        let pending = login
            .as_mut()
            .filter(|p| p.id == id && p.expires > now())
            .ok_or("accountError.cancelled")?;
        pending.expires = timestamp(&data["expiresAt"])
            .unwrap_or(pending.expires)
            .min(pending.expires);
        pending.device_id = device_id.into();
        pending.secret = secret.into();
        Ok(LoginStart {
            login_id: id.clone(),
            verification_uri: url.into(),
            expires_at: pending.expires,
            poll_interval_seconds: data["pollIntervalSeconds"]
                .as_u64()
                .unwrap_or(2)
                .clamp(1, 10),
        })
    }
    .await;
    if result.is_err() {
        cancel_login(&id)?;
    }
    result
}

pub fn cancel_login(id: &str) -> Result<(), String> {
    let mut pending = LOGIN.lock().map_err(|_| "accountError.busy")?;
    if pending.as_ref().is_some_and(|p| p.id == id) {
        *pending = None;
    }
    Ok(())
}

fn current_login(pending: &Option<Pending>, id: &str, time: i64) -> Result<Pending, String> {
    let login = pending
        .as_ref()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    if login.expires <= time {
        return Err("accountError.expired".into());
    }
    Ok(login.clone())
}

async fn poll_device(
    client: &reqwest::Client,
    control: &str,
    pending: &Pending,
) -> Result<Option<Saved>, String> {
    let device =
        url::form_urlencoded::byte_serialize(pending.device_id.as_bytes()).collect::<String>();
    let response = client
        .post(format!(
            "{control}/api/v1/cli/device-authorizations/{device}/token"
        ))
        .json(&json!({"deviceSecret":pending.secret}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let status = response.status();
    if status.as_u16() == 404 {
        return Err("accountError.expired".into());
    }
    let data = response_json(response).await?;
    if matches!(
        data["status"].as_str(),
        Some("expired" | "canceled" | "cancelled" | "denied" | "rejected")
    ) {
        return Err("accountError.cancelled".into());
    }
    if data["status"] == "pending" {
        return Ok(None);
    }
    account_from_profile(&data).map(Some)
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let pending = current_login(&*LOGIN.lock().map_err(|_| "accountError.busy")?, id, now())?;
    let Some(saved) = poll_device(&client()?, CONTROL, &pending).await? else {
        return Ok(None);
    };
    let _guard = ACCOUNTS.lock().await;
    let mut login = LOGIN.lock().map_err(|_| "accountError.busy")?;
    current_login(&login, id, now())?;
    let path = store_path()?;
    let mut rows = load_at(&path)?;
    let result = remember(&mut rows, saved);
    save_at(&path, &rows)?;
    *login = None;
    Ok(Some(result))
}

fn native_bytes(path: &Path, saved: &Saved) -> Result<Vec<u8>, String> {
    let mut config = read_config(path)?;
    let profiles = config
        .as_object_mut()
        .ok_or("accountError.format")?
        .entry("profiles")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("accountError.format")?;
    let profile = profiles
        .entry("prod")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("accountError.format")?;
    for (key, value) in [
        ("controlKey", json!(saved.control_key)),
        ("runtimeKey", json!(saved.runtime_key)),
        ("user", saved.user.clone()),
        ("apiUrl", json!(CONTROL)),
        ("linkUrl", json!(LINK)),
    ] {
        profile.insert(key.into(), value);
    }
    super::native_account_files::json(&config)
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNTS.lock().await;
    let path = store_path()?;
    let mut rows = load_at(&path)?;
    if !rows.contains_key(id) {
        return Err("accountError.invalidAccount".into());
    }
    super::process_manager::stop_desktop_for_account("opendesign").await?;
    let native = native_path()?;
    if let Some(current) = native_at(&native)? {
        remember(&mut rows, current);
        save_at(&path, &rows)?;
    }
    let saved = rows.get(id).ok_or("accountError.invalidAccount")?;
    let mut transaction = super::native_account_files::Transaction::new();
    transaction.replace(&native, &native_bytes(&native, saved)?)?;
    super::tool_config_manager::restore_opendesign_account_mode()?;
    transaction.commit();
    let mut account = saved.account.clone();
    account.active = true;
    Ok(account)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNTS.lock().await;
    let path = store_path()?;
    let mut rows = load_at(&path)?;
    rows.remove(id).ok_or("accountError.invalidAccount")?;
    save_at(&path, &rows)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}

fn timestamp(value: &Value) -> Option<i64> {
    if let Some(value) = number(value).filter(|n| *n > 0.0) {
        return Some(if value > 1e11 {
            (value / 1000.0) as i64
        } else {
            value as i64
        });
    }
    chrono::DateTime::parse_from_rfc3339(value.as_str()?)
        .ok()
        .map(|date| date.timestamp())
}

fn apply_quota(account: &mut Account, preflight: &Value, summary: &Value) -> Result<(), String> {
    if !summary.is_object() {
        return Err("accountError.authResponse".into());
    }
    account.balance_usd =
        number(&preflight["balanceUsd"]).or_else(|| number(&summary["balanceUsd"]));
    account.plan = Some(
        summary["membershipTier"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("free")
            .to_owned(),
    );
    account.subscription_end_at = timestamp(&summary["subscriptionCurrentPeriodEnd"]);
    let mut windows: Vec<_> = preflight["codingPlan"]["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let limit = number(&window["limitCredits"])?;
            let used = number(&window["usedCredits"])?;
            let duration = window["durationSeconds"].as_u64()?;
            if limit <= 0.0 || used < 0.0 || duration == 0 {
                return None;
            }
            let label = if duration % 86400 == 0 {
                format!("{}d", duration / 86400)
            } else if duration % 3600 == 0 {
                format!("{}h", duration / 3600)
            } else {
                format!("{}m", duration / 60)
            };
            Some((
                duration,
                QuotaWindow {
                    label,
                    remaining_percent: ((limit - used) / limit * 100.0).clamp(0.0, 100.0),
                    reset_at: timestamp(&window["resetsAt"]),
                },
            ))
        })
        .collect();
    windows.sort_by_key(|(duration, _)| *duration);
    account.quota_windows = windows.into_iter().map(|(_, row)| row).collect();
    Ok(())
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let initial = {
        let _guard = ACCOUNTS.lock().await;
        load_at(&store_path()?)?
            .remove(id)
            .ok_or("accountError.invalidAccount")?
    };
    let client = client()?;
    let workspace = response_json(
        client
            .get(format!("{CONTROL}/api/v1/workspaces/current"))
            .bearer_auth(&initial.control_key)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let wid = workspace["activeWorkspaceId"]
        .as_str()
        .or_else(|| workspace["workspaceId"].as_str())
        .ok_or("accountError.authResponse")?;
    let wid = url::form_urlencoded::byte_serialize(wid.as_bytes()).collect::<String>();
    let preflight = response_json(
        client
            .get(format!(
                "{CONTROL}/api/v1/workspaces/{wid}/billing/preflight"
            ))
            .bearer_auth(&initial.control_key)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let summary = response_json(
        client
            .get(format!("{CONTROL}/api/v1/billing/summary"))
            .bearer_auth(&initial.control_key)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let _guard = ACCOUNTS.lock().await;
    let path = store_path()?;
    let mut rows = load_at(&path)?;
    let saved = rows
        .get_mut(id)
        .filter(|row| row.control_key == initial.control_key)
        .ok_or("accountError.cancelled")?;
    apply_quota(&mut saved.account, &preflight, &summary)?;
    let result = saved.account.clone();
    save_at(&path, &rows)?;
    Ok(result)
}

pub(crate) async fn proxy_credentials(id: &str) -> Result<Value, String> {
    let _guard = ACCOUNTS.lock().await;
    let saved = load_at(&store_path()?)?
        .remove(id)
        .ok_or("accountError.invalidAccount")?;
    Ok(json!({"accessToken":saved.runtime_key}))
}

#[cfg(test)]
mod tests;
