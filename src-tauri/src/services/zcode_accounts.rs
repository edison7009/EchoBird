//! ZCode desktop accounts. Protocol and native storage follow zai-org/ZCode v3.14.3.
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex, time::Duration};
mod quota;

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: Mutex<Option<Pending>> = Mutex::new(None);
const OAUTH_URL: &str = "https://zcode.z.ai/api/v1/oauth/cli";
const BALANCE_URL: &str = "https://zcode.z.ai/api/v1/zcode-plan/billing/balance";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub provider: String,
    pub active: bool,
    pub plan: Option<String>,
    pub subscription_end_at: Option<i64>,
    pub quota_windows: Vec<QuotaWindow>,
    pub remaining_percent: Option<f64>,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
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
    provider: String,
    email: String,
    user_id: String,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    jwt_token: String,
    user_info: Value,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    subscription_end_at: Option<i64>,
    #[serde(default)]
    quota_windows: Vec<QuotaWindow>,
    #[serde(default)]
    remaining_percent: Option<f64>,
    #[serde(default)]
    reset_at: Option<i64>,
}

#[derive(Clone)]
struct Pending {
    id: String,
    provider: String,
    flow_id: String,
    poll_token: String,
    expires_at: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "accountError.home".into())
}

fn store_path() -> Result<PathBuf, String> {
    Ok(home()?.join(".echobird/zcode-accounts.json"))
}

pub(super) fn native_dir() -> Result<PathBuf, String> {
    let home = home()?;
    let settings = read_map(&home.join(".zcode/v2/setting.json"))?;
    let base = settings
        .get("dataBaseDir")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("ZCODE_DATA_BASE_DIR")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or(home);
    Ok(base.join(".zcode/v2"))
}

fn device_mid() -> Result<String, String> {
    let telemetry = native_dir()?.join("telemetry-state.json");
    if let Ok(raw) = fs::read_to_string(telemetry) {
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            if let Some(mid) = value["deviceMid"]
                .as_str()
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
            {
                return Ok(mid.to_string());
            }
        }
    }
    let fallback = home()?.join(".echobird/zcode-device-id");
    if let Ok(raw) = fs::read_to_string(&fallback) {
        if let Ok(mid) = uuid::Uuid::parse_str(raw.trim()) {
            return Ok(mid.to_string());
        }
    }
    let mid = uuid::Uuid::new_v4().to_string();
    fs::create_dir_all(fallback.parent().ok_or("accountError.write")?)
        .map_err(|_| "accountError.write")?;
    fs::write(&fallback, &mid).map_err(|_| "accountError.write")?;
    Ok(mid)
}

fn read_map(path: &std::path::Path) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let raw = fs::read_to_string(path).map_err(|_| "accountError.read")?;
    serde_json::from_str::<Value>(&raw)
        .map_err(|_| "accountError.format".to_string())?
        .as_object()
        .cloned()
        .ok_or_else(|| "accountError.format".into())
}

fn protect(value: &str) -> Result<String, String> {
    let encrypted = super::model_manager::encrypt_key_for_storage(value);
    encrypted
        .starts_with("enc:v1:")
        .then_some(encrypted)
        .ok_or_else(|| "accountError.keychain".into())
}

fn unprotect(value: &str) -> Result<String, String> {
    if !value.starts_with("enc:v1:") {
        return Err("accountError.invalidAccount".into());
    }
    let decrypted = super::model_manager::decrypt_key_for_use(value);
    (!decrypted.is_empty())
        .then_some(decrypted)
        .ok_or_else(|| "accountError.keychain".into())
}

fn load() -> Result<BTreeMap<String, Saved>, String> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let mut accounts: BTreeMap<String, Saved> = super::cursor_auth::read(&path)?;
    for (key, saved) in &mut accounts {
        if key != &id(saved) || !matches!(saved.provider.as_str(), "bigmodel" | "zai") {
            return Err("accountError.invalidAccount".into());
        }
        saved.access_token = unprotect(&saved.access_token)?;
        saved.jwt_token = unprotect(&saved.jwt_token)?;
        saved.refresh_token = saved.refresh_token.as_deref().map(unprotect).transpose()?;
    }
    Ok(accounts)
}

fn save(accounts: &BTreeMap<String, Saved>) -> Result<(), String> {
    let mut encrypted = accounts.clone();
    for saved in encrypted.values_mut() {
        saved.access_token = protect(&saved.access_token)?;
        saved.jwt_token = protect(&saved.jwt_token)?;
        saved.refresh_token = saved.refresh_token.as_deref().map(protect).transpose()?;
    }
    super::cursor_auth::write(&store_path()?, &encrypted)
}

#[cfg(windows)]
fn native_username() -> Option<String> {
    #[link(name = "advapi32")]
    extern "system" {
        fn GetUserNameW(buffer: *mut u16, size: *mut u32) -> i32;
    }
    let mut buffer = [0_u16; 257];
    let mut size = buffer.len() as u32;
    // Node's os.userInfo() uses the OS account, not the mutable USERNAME variable.
    // SAFETY: the buffer is writable for size UTF-16 units; the API bounds its writes.
    if unsafe { GetUserNameW(buffer.as_mut_ptr(), &mut size) } == 0 || size == 0 {
        return None;
    }
    String::from_utf16(&buffer[..size as usize - 1]).ok()
}

#[cfg(unix)]
fn native_username() -> Option<String> {
    let mut buffer = vec![0_u8; 16384];
    let mut result = std::ptr::null_mut();
    // SAFETY: getpwuid_r initializes the supplied record and writes only into the
    // owned buffer. The name is copied before that buffer is dropped.
    unsafe {
        let mut passwd: libc::passwd = std::mem::zeroed();
        if libc::getpwuid_r(
            libc::geteuid(),
            &mut passwd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        ) != 0
            || result.is_null()
            || passwd.pw_name.is_null()
        {
            return None;
        }
        Some(
            std::ffi::CStr::from_ptr(passwd.pw_name)
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn cipher_key() -> Result<[u8; 32], String> {
    let secret = std::env::var("ZCODE_CREDENTIAL_SECRET")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let platform = if cfg!(target_os = "macos") {
                "darwin"
            } else if cfg!(windows) {
                "win32"
            } else {
                "linux"
            };
            let username = native_username().unwrap_or_else(|| "unknown".into());
            format!(
                "zcode-credential-fallback:{platform}:{}:{username}",
                home().unwrap_or_default().display()
            )
        });
    Ok(Sha256::digest(secret).into())
}

fn native_encrypt(value: &str) -> Result<String, String> {
    let key = cipher_key()?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| "accountError.keychain")?;
    let mut nonce = [0_u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    let mut output = cipher
        .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
        .map_err(|_| "accountError.keychain")?;
    let tag = output.split_off(output.len() - 16);
    Ok(format!(
        "enc:v1:{}.{}.{}",
        URL_SAFE_NO_PAD.encode(nonce),
        URL_SAFE_NO_PAD.encode(tag),
        URL_SAFE_NO_PAD.encode(output)
    ))
}

fn native_decrypt(value: &str) -> Result<String, String> {
    decrypt_with_key(value, &cipher_key()?)
}

fn decrypt_with_key(value: &str, key: &[u8; 32]) -> Result<String, String> {
    let Some(raw) = value.strip_prefix("enc:v1:") else {
        return Ok(value.to_owned());
    };
    let parts = raw.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("accountError.format".into());
    }
    let nonce = URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|_| "accountError.format")?;
    let tag = URL_SAFE_NO_PAD
        .decode(parts[1])
        .map_err(|_| "accountError.format")?;
    let mut bytes = URL_SAFE_NO_PAD
        .decode(parts[2])
        .map_err(|_| "accountError.format")?;
    if nonce.len() != 12 || tag.len() != 16 {
        return Err("accountError.format".into());
    }
    bytes.extend(tag);
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "accountError.keychain")?;
    String::from_utf8(
        cipher
            .decrypt(Nonce::from_slice(&nonce), bytes.as_ref())
            .map_err(|_| "accountError.keychain")?,
    )
    .map_err(|_| "accountError.format".into())
}

fn native_value(map: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(native_decrypt)
        .transpose()
}

fn native_account() -> Result<Option<Saved>, String> {
    native_account_at(&native_dir()?)
}

fn native_account_at(dir: &std::path::Path) -> Result<Option<Saved>, String> {
    let map = read_map(&dir.join("credentials.json"))?;
    let Some(provider) = native_value(&map, "oauth:active_provider")? else {
        return Ok(None);
    };
    if provider != "zai" && provider != "bigmodel" {
        return Ok(None);
    }
    let Some(access_token) = native_value(&map, &format!("oauth:{provider}:access_token"))? else {
        return Ok(None);
    };
    let Some(jwt_token) = native_value(&map, "zcodejwttoken")? else {
        return Ok(None);
    };
    let info: Value = native_value(&map, &format!("oauth:{provider}:user_info"))?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null);
    let raw = info.get("rawProfile").unwrap_or(&info);
    let Some(user_id) = raw["user_id"]
        .as_str()
        .or_else(|| info["id"].as_str())
        .filter(|s| !s.is_empty())
    else {
        return Ok(None);
    };
    let email = raw["email"]
        .as_str()
        .or_else(|| raw["name"].as_str())
        .or_else(|| info["username"].as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(user_id);
    Ok(Some(Saved {
        provider: provider.clone(),
        email: email.to_owned(),
        user_id: user_id.to_owned(),
        access_token,
        refresh_token: native_value(&map, &format!("oauth:{provider}:refresh_token"))?,
        jwt_token,
        user_info: info,
        plan: None,
        subscription_end_at: None,
        quota_windows: Vec::new(),
        remaining_percent: None,
        reset_at: None,
    }))
}

fn id(saved: &Saved) -> String {
    format!("{}:{}", saved.provider, saved.user_id)
}

fn row(saved: &Saved, active: Option<&str>) -> Account {
    let account_id = id(saved);
    Account {
        active: active == Some(account_id.as_str()),
        id: account_id,
        email: saved.email.clone(),
        provider: saved.provider.clone(),
        plan: saved.plan.as_deref().map(quota::plan_label),
        subscription_end_at: saved.subscription_end_at,
        quota_windows: saved.quota_windows.clone(),
        remaining_percent: saved.remaining_percent,
        reset_at: saved.reset_at,
    }
}

pub async fn list() -> Result<Vec<Account>, String> {
    // Atomic saved files let passive reads proceed without waiting for a quota request.
    let accounts = load()?;
    if accounts.is_empty() {
        return Ok(Vec::new());
    }
    let active = native_account().ok().flatten().as_ref().map(id);
    Ok(accounts
        .values()
        .map(|saved| row(saved, active.as_deref()))
        .collect())
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn envelope(response: reqwest::Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!("accountError.auth|HTTP {}", response.status()));
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    if value["code"].as_i64() != Some(0) {
        return Err(value["msg"]
            .as_str()
            .unwrap_or("accountError.authResponse")
            .into());
    }
    if !value["data"].is_object() {
        return Err("accountError.authResponse".into());
    }
    Ok(value["data"].clone())
}

pub async fn start_login(provider: &str) -> Result<LoginStart, String> {
    if provider != "zai" && provider != "bigmodel" {
        return Err("accountError.invalidAccount".into());
    }
    let mut token = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut token);
    let poll_token = hex::encode(token);
    let login_id = uuid::Uuid::new_v4().to_string();
    // A late initialization must not replace a newer login attempt.
    *LOGIN.lock().map_err(|_| "accountError.auth")? = Some(Pending {
        id: login_id.clone(),
        provider: provider.into(),
        flow_id: String::new(),
        poll_token: poll_token.clone(),
        expires_at: now() + 60,
    });
    let data = envelope(
        http()?
            .post(format!("{OAUTH_URL}/init"))
            .bearer_auth(&poll_token)
            .json(&json!({"provider": provider}))
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let flow_id = data["flow_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    let verification_uri = data["authorize_url"]
        .as_str()
        .filter(|s| valid_authorization_url(provider, s))
        .ok_or("accountError.authResponse")?;
    let expires_at = data["expires_at"]
        .as_i64()
        .ok_or("accountError.authResponse")?
        .min(now() + 60);
    if expires_at <= now() {
        return Err("accountError.expired".into());
    }
    let mut login = LOGIN.lock().map_err(|_| "accountError.auth")?;
    if !login
        .as_ref()
        .is_some_and(|p| p.id == login_id && p.expires_at > now())
    {
        return Err("accountError.expired".into());
    }
    *login = Some(Pending {
        id: login_id.clone(),
        provider: provider.into(),
        flow_id: flow_id.into(),
        poll_token,
        expires_at,
    });
    Ok(LoginStart {
        login_id,
        verification_uri: verification_uri.into(),
        expires_at,
        poll_interval_seconds: data["poll_interval_sec"].as_u64().unwrap_or(1).clamp(1, 60),
    })
}

fn valid_authorization_url(provider: &str, value: &str) -> bool {
    let host = match provider {
        "zai" => "chat.z.ai",
        "bigmodel" => "bigmodel.cn",
        _ => return false,
    };
    url::Url::parse(value).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some(host)
            && url.username().is_empty()
            && url.password().is_none()
            && url.port_or_known_default() == Some(443)
    })
}

pub fn cancel_login(login_id: &str) -> Result<(), String> {
    let mut pending = LOGIN.lock().map_err(|_| "accountError.auth")?;
    if pending.as_ref().is_some_and(|p| p.id == login_id) {
        *pending = None;
    }
    Ok(())
}

pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let pending = LOGIN
        .lock()
        .map_err(|_| "accountError.auth")?
        .as_ref()
        .filter(|p| p.id == login_id)
        .cloned()
        .ok_or("accountError.expired")?;
    if pending.expires_at <= now() {
        cancel_login(login_id)?;
        return Err("accountError.expired".into());
    }
    let mut poll_url = url::Url::parse(OAUTH_URL).map_err(|_| "accountError.authResponse")?;
    poll_url
        .path_segments_mut()
        .map_err(|_| "accountError.authResponse")?
        .extend(["poll", &pending.flow_id]);
    let data = envelope(
        http()?
            .get(poll_url)
            .bearer_auth(&pending.poll_token)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    match data["status"].as_str() {
        Some("pending") => return Ok(None),
        Some("ready") => {}
        _ => return Err("accountError.authResponse".into()),
    }
    if !LOGIN
        .lock()
        .map_err(|_| "accountError.auth")?
        .as_ref()
        .is_some_and(|p| p.id == login_id && p.expires_at > now())
    {
        return Ok(None);
    }
    let user = &data["user"];
    let user_id = user["user_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    let email = user["email"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| user["name"].as_str().filter(|s| !s.is_empty()))
        .unwrap_or(user_id);
    let provider_data = &data[&pending.provider];
    let access_token = provider_data["access_token"]
        .as_str()
        .or_else(|| provider_data["accessToken"].as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let jwt_token = data["token"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let access_token = if pending.provider == "zai" {
        let response = http()?
            .post("https://api.z.ai/api/auth/z/login")
            .json(&json!({"token": access_token}))
            .send()
            .await
            .map_err(|_| "accountError.network")?;
        if !response.status().is_success() {
            return Err(format!("accountError.auth|HTTP {}", response.status()));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|_| "accountError.authResponse")?;
        let code_ok = match &value["code"] {
            Value::Null => true,
            Value::Number(code) => matches!(code.as_i64(), Some(0 | 200)),
            Value::String(code) => matches!(code.as_str(), "0" | "200"),
            _ => false,
        };
        if !code_ok || value["success"] == false {
            return Err(value["msg"]
                .as_str()
                .unwrap_or("accountError.authResponse")
                .into());
        }
        let business = &value["data"];
        business["access_token"]
            .as_str()
            .or_else(|| business["accessToken"].as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or("accountError.authResponse")?
            .to_owned()
    } else {
        access_token.to_owned()
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut login = LOGIN.lock().map_err(|_| "accountError.auth")?;
    if !login
        .as_ref()
        .is_some_and(|p| p.id == login_id && p.expires_at > now())
    {
        return Ok(None);
    }
    let mut accounts = load()?;
    let saved = Saved {
        provider: pending.provider,
        email: email.into(),
        user_id: user_id.into(),
        access_token,
        refresh_token: provider_data["refresh_token"]
            .as_str()
            .or_else(|| provider_data["refreshToken"].as_str())
            .map(str::to_owned),
        jwt_token: jwt_token.to_owned(),
        user_info: user.clone(),
        plan: None,
        subscription_end_at: None,
        quota_windows: Vec::new(),
        remaining_percent: None,
        reset_at: None,
    };
    let account_id = id(&saved);
    let active = native_account().ok().flatten().as_ref().map(id);
    let result = row(&saved, active.as_deref());
    accounts.insert(account_id, saved);
    save(&accounts)?;
    *login = None;
    Ok(Some(result))
}

fn write_native(saved: &Saved, dir: &std::path::Path) -> Result<(), String> {
    let credentials = dir.join("credentials.json");
    let settings = dir.join("setting.json");
    let mut values = read_map(&credentials)?;
    values.remove("oauth:login_attribution");
    for provider in ["zai", "bigmodel"] {
        for suffix in ["access_token", "refresh_token", "user_info"] {
            values.remove(&format!("oauth:{provider}:{suffix}"));
        }
    }
    let mut put = |key: String, value: &str| -> Result<(), String> {
        values.insert(key, Value::String(native_encrypt(value)?));
        Ok(())
    };
    put("oauth:active_provider".into(), &saved.provider)?;
    put(
        format!("oauth:{}:access_token", saved.provider),
        &saved.access_token,
    )?;
    if let Some(refresh) = &saved.refresh_token {
        put(format!("oauth:{}:refresh_token", saved.provider), refresh)?;
    }
    put(
        format!("oauth:{}:user_info", saved.provider),
        &if saved.provider == "bigmodel" {
            json!({
                "id": saved.user_id,
                "username": saved.email,
                "displayName": saved.email,
                "rawProfile": saved.user_info.get("rawProfile").unwrap_or(&saved.user_info),
            })
            .to_string()
        } else {
            saved
                .user_info
                .get("rawProfile")
                .unwrap_or(&saved.user_info)
                .to_string()
        },
    )?;
    put("zcodejwttoken".into(), &saved.jwt_token)?;
    let mut settings_value = read_map(&settings)?;
    let same_account = native_account_at(dir)?
        .as_ref()
        .is_some_and(|current| id(current) == id(saved));
    if !same_account {
        for key in [
            "providerFamilyConnectionSelections",
            "modelProviderFamilySelectedKeys",
        ] {
            if let Some(selections) = settings_value.get_mut(key).and_then(Value::as_object_mut) {
                selections.remove(&saved.provider);
            }
        }
    }
    settings_value.insert(
        "providerFamilyDomain".into(),
        Value::String(saved.provider.clone()),
    );
    settings_value.insert(
        "providerFamilyDomainUpdatedAt".into(),
        Value::Number(chrono::Utc::now().timestamp_millis().into()),
    );
    settings_value.insert("providerFamilyDomainMigrated".into(), Value::Bool(true));
    let modes = settings_value
        .entry("modelProviderFamilyModes")
        .or_insert_with(|| json!({}));
    modes
        .as_object_mut()
        .ok_or("accountError.format")?
        .insert(saved.provider.clone(), Value::String("oauth".into()));
    super::cursor_auth::write(&credentials, &values)?;
    super::cursor_auth::write(&settings, &settings_value)?;
    Ok(())
}

pub async fn switch(account_id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    if !accounts.contains_key(account_id) {
        return Err("accountError.invalidAccount".into());
    }
    super::process_manager::stop_desktop_for_config("zcode").await;
    let dir = native_dir()?;
    if let Some(mut current) = native_account_at(&dir)? {
        if let Some(previous) = accounts.get(&id(&current)) {
            current.plan = previous.plan.clone();
            current.subscription_end_at = previous.subscription_end_at;
            current.quota_windows = previous.quota_windows.clone();
            current.remaining_percent = previous.remaining_percent;
            current.reset_at = previous.reset_at;
        }
        accounts.insert(id(&current), current);
        save(&accounts)?;
    }
    let saved = accounts
        .get(account_id)
        .ok_or("accountError.invalidAccount")?;
    switch_native(saved, &dir)?;
    Ok(row(saved, Some(account_id)))
}

fn switch_native(saved: &Saved, dir: &std::path::Path) -> Result<(), String> {
    let mut backups = Vec::new();
    for name in [
        "credentials.json",
        "setting.json",
        "provider_config.json",
        "config.json",
    ] {
        let path = dir.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err("accountError.read".into()),
        };
        backups.push((path, bytes));
    }
    let result = (|| {
        let restored = super::tool_config_manager::restore_zcode_account_config(dir);
        if !restored.success {
            return Err(restored.message);
        }
        write_native(saved, dir)?;
        if native_account_at(dir)?.as_ref().map(id) != Some(id(saved)) {
            return Err("accountError.write".into());
        }
        Ok(())
    })();
    if result.is_err() {
        for (path, bytes) in backups {
            match bytes {
                Some(bytes) => fs::write(path, bytes).map_err(|_| "accountError.write")?,
                None if path.exists() => fs::remove_file(path).map_err(|_| "accountError.write")?,
                None => {}
            }
        }
    }
    result
}

pub async fn delete(account_id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    accounts
        .remove(account_id)
        .ok_or("accountError.invalidAccount")?;
    save(&accounts)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .filter(|number: &f64| number.is_finite())
}

fn unix_seconds(value: &Value) -> Option<i64> {
    let timestamp = number(value)?;
    (timestamp > 0.0).then_some(if timestamp > 1_000_000_000_000.0 {
        (timestamp / 1000.0) as i64
    } else {
        timestamp as i64
    })
}

fn apply_balance(saved: &mut Saved, data: &Value) {
    let plans = data["plans"].as_array();
    let active_plan = plans.and_then(|plans| {
        plans.iter().find(|p| {
            p["status"] == "active" && unix_seconds(&p["ends_at"]).map_or(true, |end| end > now())
        })
    });
    saved.subscription_end_at = active_plan.and_then(|p| unix_seconds(&p["ends_at"]));
    saved.quota_windows.clear();
    saved.plan = active_plan
        .and_then(|p| p["name"].as_str())
        .map(quota::plan_label);
    let balances: Vec<&Value> = data["balances"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| {
            unix_seconds(&b["expires_at"]).map_or(true, |end| end > now())
                && !plans.into_iter().flatten().any(|p| {
                    let owner = if b["user_plan_id"].is_string() {
                        p["user_plan_id"] == b["user_plan_id"]
                    } else {
                        b["plan_id"].is_string() && p["plan_id"] == b["plan_id"]
                    };
                    owner
                        && (p["status"] == "expired"
                            || unix_seconds(&p["ends_at"]).is_some_and(|end| end <= now()))
                })
        })
        .collect();
    let amounts: Option<Vec<(f64, f64)>> = balances
        .iter()
        .map(|b| Some((number(&b["total_units"])?, number(&b["remaining_units"])?)))
        .collect();
    saved.remaining_percent = amounts.and_then(|values| {
        let total: f64 = values.iter().map(|v| v.0).sum();
        let remaining: f64 = values.iter().map(|v| v.1).sum();
        (total > 0.0).then_some((remaining / total * 100.0).clamp(0.0, 100.0))
    });
    saved.reset_at = balances
        .iter()
        .filter_map(|b| unix_seconds(&b["expires_at"]))
        .min();
}

pub async fn refresh(account_id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    let saved = accounts
        .get_mut(account_id)
        .ok_or("accountError.invalidAccount")?;
    // The running client may have renewed the currently selected token.
    let native = native_account().ok().flatten();
    if let Some(current) = native.as_ref().filter(|current| id(current) == account_id) {
        saved.access_token.clone_from(&current.access_token);
        saved.refresh_token.clone_from(&current.refresh_token);
        saved.jwt_token.clone_from(&current.jwt_token);
    }
    quota::refresh(saved).await?;
    let active = native.as_ref().map(id);
    let result = row(saved, active.as_deref());
    save(&accounts)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_balance, decrypt_with_key, native_decrypt, native_encrypt, read_map, write_native,
        Saved,
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};

    #[test]
    fn zcode_native_cipher_round_trip() {
        let value = native_encrypt("zai-token").unwrap();
        assert!(value.starts_with("enc:v1:"));
        assert_eq!(native_decrypt(&value).unwrap(), "zai-token");
    }

    #[test]
    fn decrypts_native_ciphertext_fixture() {
        let key: [u8; 32] = Sha256::digest(b"fixture-secret").into();
        assert_eq!(
            decrypt_with_key(
                "enc:v1:AAECAwQFBgcICQoL.FiU1Vp2Lqcrw8HsqTSII4A.yWyyERKTBZU",
                &key
            )
            .unwrap(),
            "bigmodel"
        );
    }

    #[test]
    fn quota_parser_keeps_unknown_and_reads_numeric_strings() {
        let mut saved = Saved {
            provider: "zai".into(),
            email: "one@example.test".into(),
            user_id: "one".into(),
            access_token: String::new(),
            refresh_token: None,
            jwt_token: String::new(),
            user_info: json!({}),
            plan: None,
            subscription_end_at: None,
            quota_windows: Vec::new(),
            remaining_percent: None,
            reset_at: None,
        };
        apply_balance(&mut saved, &json!({"plans": [], "balances": []}));
        assert_eq!(saved.plan, None);
        assert_eq!(saved.remaining_percent, None);
        apply_balance(
            &mut saved,
            &json!({
                "plans": [{"status": "active", "name": "Pro"}],
                "balances": [{"total_units": "100", "remaining_units": "40", "expires_at": 1900000000}]
            }),
        );
        assert_eq!(saved.plan.as_deref(), Some("Pro"));
        assert_eq!(saved.remaining_percent, Some(40.0));
        assert_eq!(saved.reset_at, Some(1_900_000_000));
    }

    #[test]
    fn native_switch_writes_both_provider_and_mode_without_dropping_other_keys() {
        let dir = std::env::temp_dir().join(format!("echobird-zcode-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        super::super::cursor_auth::write(&dir.join("credentials.json"), &json!({"other": "keep"}))
            .unwrap();
        let saved = Saved {
            provider: "zai".into(),
            email: "one@example.test".into(),
            user_id: "one".into(),
            access_token: "access".into(),
            refresh_token: None,
            jwt_token: "jwt".into(),
            user_info: json!({"user_id":"one","email":"one@example.test"}),
            plan: None,
            subscription_end_at: None,
            quota_windows: Vec::new(),
            remaining_percent: None,
            reset_at: None,
        };
        write_native(&saved, &dir).unwrap();
        let credentials = read_map(&dir.join("credentials.json")).unwrap();
        assert_eq!(credentials["other"], "keep");
        assert_eq!(
            native_decrypt(credentials["oauth:active_provider"].as_str().unwrap()).unwrap(),
            "zai"
        );
        assert_eq!(
            native_decrypt(credentials["oauth:zai:access_token"].as_str().unwrap()).unwrap(),
            "access"
        );
        let settings = read_map(&dir.join("setting.json")).unwrap();
        assert_eq!(settings["providerFamilyDomain"], "zai");
        assert_eq!(settings["modelProviderFamilyModes"]["zai"], "oauth");
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn account(provider: &str, user_id: &str) -> Saved {
        Saved {
            provider: provider.into(),
            user_id: user_id.into(),
            email: format!("{user_id}@example.test"),
            access_token: format!("access-{provider}-{user_id}"),
            refresh_token: Some("refresh-fixture".into()),
            jwt_token: format!("jwt-{provider}-{user_id}"),
            user_info: json!({"user_id":user_id,"email":format!("{user_id}@example.test")}),
            plan: None,
            subscription_end_at: None,
            quota_windows: Vec::new(),
            remaining_percent: None,
            reset_at: None,
        }
    }
    fn fixture_dir() -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("echobird-zcode-fixture-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    #[test]
    fn authorization_is_bound_to_the_selected_region() {
        assert!(super::valid_authorization_url(
            "bigmodel",
            "https://bigmodel.cn/login?state=fixture"
        ));
        assert!(super::valid_authorization_url(
            "zai",
            "https://chat.z.ai/oauth/authorize"
        ));
        for value in [
            "https://chat.z.ai/oauth/authorize",
            "http://bigmodel.cn/login",
            "https://bigmodel.cn.evil.test/login",
            "https://user@bigmodel.cn/login",
            "https://bigmodel.cn:444/login",
        ] {
            assert!(!super::valid_authorization_url("bigmodel", value));
        }
    }
    #[tokio::test]
    async fn cancelled_and_expired_logins_stop_before_network_or_storage() {
        *super::LOGIN.lock().unwrap() = Some(super::Pending {
            id: "current".into(),
            provider: "bigmodel".into(),
            flow_id: "unused".into(),
            poll_token: "unused".into(),
            expires_at: super::now() - 1,
        });
        super::cancel_login("older").unwrap();
        assert!(super::LOGIN.lock().unwrap().is_some());
        assert!(
            matches!(super::poll_login("current").await, Err(error) if error == "accountError.expired")
        );
        assert!(super::LOGIN.lock().unwrap().is_none());
        assert!(
            matches!(super::poll_login("current").await, Err(error) if error == "accountError.expired")
        );
    }
    #[test]
    fn native_region_switch_preserves_unrelated_settings_and_removes_stale_tokens() {
        let dir = fixture_dir();
        let cn = account("bigmodel", "same-id");
        let international = account("zai", "same-id");
        assert_ne!(super::id(&cn), super::id(&international));
        super::super::cursor_auth::write(&dir.join("setting.json"), &json!({
            "theme":"dark", "providerFamilyConnectionSelections":{"zai":{"kind":"team-coding-plan","projectId":"old"}},
            "modelProviderFamilyModes":{"unrelated":"keep"}
        })).unwrap();
        super::switch_native(&cn, &dir).unwrap();
        assert_eq!(
            super::native_account_at(&dir)
                .unwrap()
                .unwrap()
                .access_token,
            cn.access_token
        );
        super::switch_native(&international, &dir).unwrap();
        let current = super::native_account_at(&dir).unwrap().unwrap();
        assert_eq!(super::id(&current), "zai:same-id");
        assert_eq!(current.access_token, international.access_token);
        assert_eq!(current.jwt_token, international.jwt_token);
        let credentials = read_map(&dir.join("credentials.json")).unwrap();
        assert!(!credentials.contains_key("oauth:bigmodel:access_token"));
        let settings = read_map(&dir.join("setting.json")).unwrap();
        assert_eq!(settings["theme"], "dark");
        assert_eq!(settings["providerFamilyDomain"], "zai");
        assert_eq!(settings["modelProviderFamilyModes"]["unrelated"], "keep");
        assert!(settings["providerFamilyConnectionSelections"]
            .get("zai")
            .is_none());
        let before = std::fs::read(dir.join("credentials.json")).unwrap();
        let _ = super::native_account_at(&dir).unwrap();
        assert_eq!(before, std::fs::read(dir.join("credentials.json")).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn account_apply_restores_models_and_rolls_back_every_file_on_failure() {
        let dir = fixture_dir();
        let config = json!({"schemaVersion":1,"config":{
            "defaultModelSelection":{"providerId":"echobird","modelId":"fixture"},
            "providerOrder":["echobird","other"],"providerConfigRules":{"providerRules":[{"providerId":"echobird"},{"providerId":"other"}]}
        }});
        super::super::cursor_auth::write(&dir.join("provider_config.json"), &config).unwrap();
        std::fs::write(dir.join("setting.json"), "broken").unwrap();
        let before = std::fs::read(dir.join("provider_config.json")).unwrap();
        assert!(super::switch_native(&account("bigmodel", "one"), &dir).is_err());
        assert_eq!(
            std::fs::read(dir.join("provider_config.json")).unwrap(),
            before
        );
        assert!(!dir.join("credentials.json").exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("setting.json")).unwrap(),
            "broken"
        );
        std::fs::write(dir.join("setting.json"), "{}").unwrap();
        super::switch_native(&account("bigmodel", "one"), &dir).unwrap();
        let config = read_map(&dir.join("provider_config.json")).unwrap();
        assert!(config["config"].get("defaultModelSelection").is_none());
        assert_eq!(config["config"]["providerOrder"], json!(["other"]));
        assert_eq!(
            super::native_account_at(&dir).unwrap().unwrap().provider,
            "bigmodel"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn quota_refresh_does_not_invent_zero_or_use_reserved_available_units() {
        let mut saved = account("bigmodel", "one");
        saved.plan = Some("Old".into());
        saved.remaining_percent = Some(99.0);
        apply_balance(&mut saved, &json!({"plans":[],"balances":[]}));
        assert_eq!(saved.plan, None);
        assert_eq!(saved.remaining_percent, None);
        apply_balance(
            &mut saved,
            &json!({"balances":[{"total_units":100,"available_units":0}]}),
        );
        assert_eq!(saved.remaining_percent, None);
        apply_balance(
            &mut saved,
            &json!({"balances":[{"total_units":100,"remaining_units":0,"expires_at":1900000000,"period_end":1950000000}]}),
        );
        assert_eq!(saved.remaining_percent, Some(0.0));
        assert_eq!(saved.reset_at, Some(1900000000));
        apply_balance(
            &mut saved,
            &json!({"balances":[{"total_units":100,"remaining_units":40},{"total_units":100}]}),
        );
        assert_eq!(saved.remaining_percent, None);
        apply_balance(
            &mut saved,
            &json!({"plans":[{"plan_id":"old","status":"expired","name":"Old"}],
            "balances":[{"plan_id":"old","total_units":100,"remaining_units":40}]}),
        );
        assert_eq!(saved.remaining_percent, None);
        assert_eq!(saved.plan, None);
    }
    #[test]
    fn public_account_never_exposes_tokens() {
        let value = serde_json::to_string(&super::row(&account("zai", "one"), None)).unwrap();
        assert!(!value.contains("access-"));
        assert!(!value.contains("jwt-"));
        assert!(!value.contains("refresh-fixture"));
    }

    #[tokio::test]
    async fn successful_envelopes_require_data_before_updating_cached_quota() {
        for body in [
            json!({"code":0}),
            json!({"code":0,"data":null}),
            json!({"code":0,"data":[]}),
            json!({"code":0,"data":"invalid"}),
        ] {
            let response = reqwest::Response::from(axum::http::Response::new(
                serde_json::to_vec(&body).unwrap(),
            ));
            assert_eq!(
                super::envelope(response).await.unwrap_err(),
                "accountError.authResponse"
            );
        }
        let data = json!({"plans":[],"balances":[]});
        let response = reqwest::Response::from(axum::http::Response::new(
            serde_json::to_vec(&json!({"code":0,"data":data})).unwrap(),
        ));
        assert_eq!(super::envelope(response).await.unwrap(), data);
    }
}
