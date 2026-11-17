//! Manus desktop sessions. Change only session credentials and login verification;
//! unrelated Chromium storage and Manus settings remain owned by the native client.
use super::account_login_browser::{self, Login};
use super::cursor_auth::{read, write};
use super::electron_storage::{cipher, decrypt, decrypt_bytes, encrypt_bytes, Cipher};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, path::PathBuf, time::Duration};

const HOST: &str = "api.manus.im";
const COOKIE: &str = "session_id";
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static PENDING: std::sync::Mutex<Option<Login>> = std::sync::Mutex::new(None);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credits {
    pub total: i64,
    pub free: Option<i64>,
    pub refresh: Option<i64>,
    pub next_refresh_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub active: bool,
    pub credits: Option<Credits>,
    pub subscription_end_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub expires_at: i64,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPoll {
    pub account: Option<Account>,
    pub awaiting_client_exit: bool,
    pub expires_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    session: String,
}
type Store = BTreeMap<String, Saved>;

fn data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let store = PathBuf::from(local)
            .join("Packages/ManusAI.Manus_vajzd2mq3s8wj/LocalCache/Roaming/Manus");
        if store.is_dir() {
            return Ok(store);
        }
    }
    dirs::config_dir()
        .map(|dir| dir.join("Manus"))
        .ok_or_else(|| "accountError.home".into())
}

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/manus-accounts.json"))
}

fn load_store() -> Result<Store, String> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(Store::new());
    }
    read(&path)
}

pub(super) fn cookie_db(dir: &Path, writable: bool) -> Result<Connection, String> {
    let path = cookie_path(dir);
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db = Connection::open_with_flags(path, flags).map_err(|_| "accountError.closeClient")?;
    db.busy_timeout(Duration::from_secs(2))
        .map_err(|_| "accountError.read")?;
    Ok(db)
}

fn cookie_path(dir: &Path) -> PathBuf {
    ["Network/Cookies", "Cookies"]
        .iter()
        .map(|path| dir.join(path))
        .find(|path| path.is_file())
        .unwrap_or_else(|| dir.join("Network/Cookies"))
}

fn cookie_read_error(error: rusqlite::Error) -> String {
    if matches!(error, rusqlite::Error::SqliteFailure(ref failure, _) if matches!(failure.code, rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked))
    {
        "accountError.closeClient".into()
    } else {
        "accountError.read".into()
    }
}

fn cookie_value(
    db: &Connection,
    dir: &Path,
    load_cipher: impl FnOnce(&Path) -> Result<Cipher, String>,
) -> Result<Option<String>, String> {
    let row: Option<(String, Vec<u8>)> = db
        .query_row(
            "SELECT value, encrypted_value FROM cookies WHERE host_key=?1 AND name=?2 AND path='/'",
            params![HOST, COOKIE],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(cookie_read_error)?;
    let Some((plain, encrypted)) = row else {
        return Ok(None);
    };
    if !plain.is_empty() {
        return Ok(Some(plain));
    }
    if encrypted.is_empty() {
        return Ok(None);
    }
    let bytes = decrypt_bytes(&load_cipher(dir)?, &encrypted)?;
    let version: i64 = db
        .query_row("SELECT value FROM meta WHERE key='version'", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let token = if version >= 24 {
        bytes
            .strip_prefix(Sha256::digest(HOST.as_bytes()).as_slice())
            .ok_or("accountError.format")?
    } else {
        &bytes
    };
    String::from_utf8(token.to_vec())
        .map(Some)
        .map_err(|_| "accountError.format".into())
}

pub(super) fn native_session(dir: &Path) -> Result<Option<String>, String> {
    if !cookie_path(dir).exists() {
        return Ok(None);
    }
    cookie_value(&cookie_db(dir, false)?, dir, cipher)
}

pub(super) fn set_cookie(
    db: &Connection,
    dir: &Path,
    token: Option<&str>,
    load_cipher: impl FnOnce(&Path) -> Result<Cipher, String>,
) -> Result<(), String> {
    let existing: Option<Vec<u8>> = db
        .query_row(
            "SELECT encrypted_value FROM cookies WHERE host_key=?1 AND name=?2 AND path='/'",
            params![HOST, COOKIE],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| "accountError.read")?;
    if token.is_none() {
        db.execute(
            "DELETE FROM cookies WHERE host_key=?1 AND name=?2 AND path='/'",
            params![HOST, COOKIE],
        )
        .map_err(|_| "accountError.write")?;
        return Ok(());
    }
    let token = token.ok_or("accountError.invalidAccount")?;
    let encrypted = existing.as_ref().is_some_and(|v| !v.is_empty()) || cfg!(target_os = "macos");
    let (plain, secret) = if encrypted {
        let version: i64 = db
            .query_row("SELECT value FROM meta WHERE key='version'", [], |row| {
                row.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut bytes = Vec::new();
        if version >= 24 {
            bytes.extend_from_slice(&Sha256::digest(HOST.as_bytes()));
        }
        bytes.extend_from_slice(token.as_bytes());
        (String::new(), encrypt_bytes(&load_cipher(dir)?, &bytes)?)
    } else {
        (token.to_string(), Vec::new())
    };
    if existing.is_some() {
        db.execute(
            "UPDATE cookies SET value=?1, encrypted_value=?2 WHERE host_key=?3 AND name=?4 AND path='/'",
            params![plain, secret, HOST, COOKIE],
        )
        .map_err(|_| "accountError.write")?;
    } else {
        let now = (chrono::Utc::now().timestamp() + 11_644_473_600) * 1_000_000;
        let expires = now + 365 * 24 * 60 * 60 * 1_000_000;
        db.execute(
            "INSERT INTO cookies (creation_utc,host_key,top_frame_site_key,name,value,encrypted_value,path,expires_utc,is_secure,is_httponly,last_access_utc,has_expires,is_persistent,priority,samesite,source_scheme,source_port,last_update_utc,source_type,has_cross_site_ancestor) VALUES (?1,?2,'',?3,?4,?5,'/',?6,1,1,?1,1,1,1,1,2,443,?1,0,0)",
            params![now, HOST, COOKIE, plain, secret, expires],
        )
        .map_err(|_| "accountError.write")?;
    }
    Ok(())
}

fn write_native(dir: &Path, token: Option<&str>) -> Result<(), String> {
    if token.is_none() && !cookie_path(dir).exists() {
        return Ok(());
    }
    if native_session(dir)?.as_deref() != token {
        // The official client invalidates device verification when its token changes.
        invalidate_verification(dir)?;
    }
    let mut db = cookie_db(dir, true)?;
    let tx = db.transaction().map_err(|_| "accountError.write")?;
    set_cookie(&tx, dir, token, cipher)?;
    tx.commit().map_err(|_| "accountError.write".into())
}

fn invalidate_verification(dir: &Path) -> Result<(), String> {
    let path = dir.join("localStorage.json");
    let mut data: Value = read(&path)?;
    let object = data.as_object_mut().ok_or("accountError.format")?;
    if object
        .remove("desktopLoginVerifiedDeviceId:https://api.manus.im/")
        .is_some()
    {
        write(&path, &data)?;
    }
    Ok(())
}

pub(super) async fn profile(token: &str) -> Result<Account, String> {
    Ok(profile_identity(token).await?.0)
}

pub(super) async fn profile_identity(token: &str) -> Result<(Account, String), String> {
    let body = request(token, "UserInfo").await?;
    let account = parse_profile(&body)?;
    let user_id = body["userId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("accountError.authResponse")?;
    Ok((account, user_id.to_owned()))
}

fn parse_profile(response: &Value) -> Result<Account, String> {
    let id = response["userId"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("accountError.authResponse")?;
    let email = response["email"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("accountError.authResponse")?;
    let plan = response["membershipVersion"].as_str().map(str::to_owned);
    let subscription_end_at = if plan
        .as_deref()
        .is_some_and(|plan| !plan.eq_ignore_ascii_case("free"))
    {
        match response.get("currentPeriodEnd") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .or_else(|| value.as_str()?.parse().ok())
                    .ok_or("accountError.format")?,
            )
            .filter(|end| *end > 0),
        }
    } else {
        None
    };
    Ok(Account {
        id: format!("{:x}", Sha256::digest(id.as_bytes())),
        email: email.into(),
        plan,
        active: false,
        credits: None,
        subscription_end_at,
    })
}

async fn request(token: &str, method: &str) -> Result<Value, String> {
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "accountError.network")?
        .post(format!("https://api.manus.im/user.v1.UserService/{method}"))
        .bearer_auth(token)
        .header("Connect-Protocol-Version", "1")
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err(format!("accountError.quota|HTTP {}", response.status()));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.format".into())
}

fn parse_credits(value: &Value) -> Result<Credits, String> {
    let total = value["totalCredits"].as_i64().ok_or("accountError.quota")?;
    if total < 0 {
        return Err("accountError.quota".into());
    }
    Ok(Credits {
        total,
        free: value["freeCredits"].as_i64(),
        refresh: value["refreshCredits"].as_i64(),
        next_refresh_at: value["nextRefreshTime"]
            .as_str()
            .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
            .map(|date| date.timestamp()),
    })
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    Ok(load_store()?
        .into_values()
        .map(|saved| saved.account)
        .collect())
}

pub async fn start_login(app: tauri::AppHandle) -> Result<LoginStart, String> {
    if !cfg!(any(windows, target_os = "macos")) {
        return Err("accountError.unavailable".into());
    }
    let login = Login::start_desktop(
        &app,
        "Manus",
        "manus",
        &[
            "manus.im",
            "google.com",
            "googleusercontent.com",
            "apple.com",
            "facebook.com",
        ],
    )?;
    let old = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .replace(login.clone());
    if let Some(old) = old {
        old.cancel(&app);
    }
    Ok(LoginStart {
        login_id: login.id,
        expires_at: login.expires,
    })
}
pub(super) fn saved_session(dir: &Path, stored: &str) -> Result<String, String> {
    if stored.starts_with("enc:v1:") {
        account_login_browser::unseal(stored)
    } else {
        decrypt(&cipher(dir)?, stored)
    }
}
pub async fn poll_login(app: &tauri::AppHandle, id: &str) -> Result<LoginPoll, String> {
    let login = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .as_ref()
        .filter(|login| login.id == id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    let Some(token) = login.desktop_token()? else {
        return Ok(LoginPoll::default());
    };
    let mut account = profile(&token).await?;
    let session = account_login_browser::seal(&token)?;
    let _lock = ACCOUNT_LOCK.lock().await;
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    pending
        .as_ref()
        .filter(|current| current.id == id)
        .ok_or("accountError.cancelled")?;
    let result = login.complete(app, || {
        let mut store = load_store()?;
        if let Some(previous) = store.get(&account.id) {
            account.credits = previous.account.credits.clone();
            account.active = previous.account.active;
        }
        store.insert(
            account.id.clone(),
            Saved {
                account: account.clone(),
                session,
            },
        );
        write(&store_path()?, &store)?;
        Ok(LoginPoll {
            account: Some(account),
            ..Default::default()
        })
    })?;
    *pending = None;
    Ok(result)
}
pub async fn cancel_login(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let login = {
        let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
        if pending.as_ref().is_some_and(|login| login.id == id) {
            pending.take()
        } else {
            None
        }
    };
    if let Some(login) = login {
        login.cancel(app);
    }
    Ok(())
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let path = store_path()?;
    let mut store = load_store()?;
    let saved = store.get(id).ok_or("accountError.invalidAccount")?;
    let token = saved_session(&dir, &saved.session)?;
    if profile(&token).await?.id != id {
        return Err("accountError.invalidAccount".into());
    }
    super::process_manager::stop_desktop_for_account("manus").await?;
    let previous = native_session(&dir)?;
    if let Err(error) = write_native(&dir, Some(&token)) {
        let _ = super::process_manager::start_tool("manus", None, None).await;
        return Err(error);
    }
    for (key, saved) in &mut store {
        saved.account.active = key == id;
    }
    if let Err(error) = write(&path, &store) {
        write_native(&dir, previous.as_deref())
            .map_err(|rollback| format!("accountError.rollback|{error} {rollback}"))?;
        let _ = super::process_manager::start_tool("manus", None, None).await;
        return Err(error);
    }
    Ok(store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone())
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let mut store = load_store()?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    let token = saved_session(&dir, &saved.session)?;
    let profile = profile(&token).await?;
    if profile.id != id {
        return Err("accountError.invalidAccount".into());
    }
    saved.account.credits = Some(parse_credits(
        &request(&token, "GetAvailableCredits").await?,
    )?);
    saved.account.plan = profile.plan;
    saved.account.subscription_end_at = profile.subscription_end_at;
    let account = saved.account.clone();
    write(&store_path()?, &store)?;
    Ok(account)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    store.remove(id).ok_or("accountError.invalidAccount")?;
    write(&store_path()?, &store)
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let dir = data_dir()?;
    let Some(token) = native_session(&dir)? else {
        return Ok(None);
    };
    let (mut account, _user) = (profile(&token).await?, String::new());
    account.active = false;
    account.credits = None;

    let _guard = ACCOUNT_LOCK.lock().await;
    let mut rows = load_store()?;
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
    if native_session(&dir)?.as_deref() != Some(&token) {
        return Err("accountError.invalidAccount".into());
    }
    rows.insert(
        account.id.clone(),
        Saved {
            account,
            session: account_login_browser::seal(&token)?,
        },
    );
    write(&store_path()?, &rows)?;
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
    fn subscription_end_uses_official_seconds_and_not_credit_refresh_time() {
        for end in [
            serde_json::json!(1794132901_i64),
            serde_json::json!("1794132901"),
        ] {
            let account = parse_profile(&serde_json::json!({
                "userId":"fixture-user", "email":"manus@example.test", "membershipVersion":"pro",
                "currentPeriodEnd":end, "nextRefreshTime":"2026-10-10T00:00:00Z"
            }))
            .unwrap();
            assert_eq!(account.plan.as_deref(), Some("pro"));
            assert_eq!(account.subscription_end_at, Some(1794132901));
            assert!(account.credits.is_none());
        }
    }

    #[test]
    fn free_missing_or_zero_subscription_end_stays_unknown_without_inferred_duration() {
        for value in [
            serde_json::json!({"membershipVersion":"free","currentPeriodEnd":"1794132901"}),
            serde_json::json!({"membershipVersion":"Free","currentPeriodEnd":"1794132901"}),
            serde_json::json!({"membershipVersion":"pro"}),
            serde_json::json!({"membershipVersion":"pro","currentPeriodEnd":"0"}),
            serde_json::json!({"membershipVersion":"pro","currentPeriodEnd":null}),
            serde_json::json!({"membershipVersion":"pro","currentPeriodEnd":-1}),
            serde_json::json!({"currentPeriodEnd":"1794132901"}),
        ] {
            let mut profile = value;
            profile["userId"] = "fixture-user".into();
            profile["email"] = "manus@example.test".into();
            assert!(parse_profile(&profile)
                .unwrap()
                .subscription_end_at
                .is_none());
        }
        assert!(parse_profile(&serde_json::json!({
            "userId":"fixture-user", "email":"manus@example.test", "membershipVersion":"pro",
            "currentPeriodEnd":"invalid"
        }))
        .is_err());
        assert!(parse_profile(&serde_json::json!({"membershipVersion":"pro"})).is_err());
    }

    #[test]
    fn existing_manus_cache_loads_and_subscription_summary_round_trips() {
        let value = serde_json::json!({
            "account":{"id":"manus","email":"manus@example.test","plan":"pro","active":true,
                "credits":{"total":1300,"free":1000,"refresh":300,"nextRefreshAt":1791561600}},
            "session":"encrypted-session"
        });
        let mut saved: Saved = serde_json::from_value(value.clone()).unwrap();
        assert!(saved.account.subscription_end_at.is_none());
        saved.account.subscription_end_at = Some(1794132901);
        let encoded = serde_json::to_value(&saved).unwrap();
        assert_eq!(encoded["session"], value["session"]);
        assert_eq!(encoded["account"]["credits"], value["account"]["credits"]);
        let decoded: Saved = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.account.subscription_end_at, Some(1794132901));
        assert!(decoded.account.active);
    }

    #[test]
    fn electron_cookie_layout_supports_root_and_network_without_creating_a_database() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&dir).unwrap();
        assert!(native_session(&dir).unwrap().is_none());
        assert!(!dir.join("Network/Cookies").exists());
        let db = Connection::open(dir.join("Cookies")).unwrap();
        db.execute_batch("CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,value TEXT,encrypted_value BLOB); INSERT INTO cookies VALUES ('api.manus.im','session_id','/','fixture-session',x'');").unwrap();
        assert_eq!(
            native_session(&dir).unwrap().as_deref(),
            Some("fixture-session")
        );
        assert_eq!(cookie_path(&dir), dir.join("Cookies"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn login_fixture(token: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(dir.join("Network")).unwrap();
        write(
            &dir.join("localStorage.json"),
            &serde_json::json!({
                "deviceId": "fixture-device", "manus-theme": "dark",
                "desktopLoginBoundDeviceId:https://api.manus.im/": "fixture-device",
                "desktopLoginVerifiedDeviceId:https://api.manus.im/": "fixture-device"
            }),
        )
        .unwrap();
        let db = Connection::open(dir.join("Network/Cookies")).unwrap();
        db.execute_batch("CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,value TEXT,encrypted_value BLOB,last_update_utc INTEGER);").unwrap();
        if let Some(token) = token {
            db.execute(
                "INSERT INTO cookies VALUES (?1,?2,'/',?3,x'',1)",
                params![HOST, COOKIE, token],
            )
            .unwrap();
        }
        dir
    }

    #[test]
    fn reading_device_identity_preserves_settings_and_cookie_state() {
        let dir = login_fixture(None);
        let before = std::fs::read(dir.join("localStorage.json")).unwrap();
        assert_eq!(
            std::fs::read(dir.join("localStorage.json")).unwrap(),
            before
        );
        assert!(native_session(&dir).unwrap().is_none());
        // Explicit native apply still clears only the old device-verification marker.
        invalidate_verification(&dir).unwrap();
        let state: Value = read(&dir.join("localStorage.json")).unwrap();
        assert_eq!(state["deviceId"], "fixture-device");
        assert_eq!(state["manus-theme"], "dark");
        assert_eq!(
            state["desktopLoginBoundDeviceId:https://api.manus.im/"],
            "fixture-device"
        );
        assert!(state
            .get("desktopLoginVerifiedDeviceId:https://api.manus.im/")
            .is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn credits_distinguish_unknown_from_zero_and_use_the_actual_balance() {
        assert!(parse_credits(&serde_json::json!({})).is_err());
        let credits = parse_credits(&serde_json::json!({
            "totalCredits": 0,
            "freeCredits": 0,
            "refreshCredits": 0,
            "nextRefreshTime": "2026-09-29T16:00:00Z"
        }))
        .unwrap();
        assert_eq!(credits.total, 0);
        assert_eq!(credits.next_refresh_at, Some(1790697600));
    }

    #[test]
    fn cookie_update_preserves_unrelated_browser_state() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY,value TEXT); INSERT INTO meta VALUES ('version','24'); CREATE TABLE cookies(creation_utc INTEGER NOT NULL,host_key TEXT NOT NULL,top_frame_site_key TEXT NOT NULL,name TEXT NOT NULL,value TEXT NOT NULL,encrypted_value BLOB NOT NULL,path TEXT NOT NULL,expires_utc INTEGER NOT NULL,is_secure INTEGER NOT NULL,is_httponly INTEGER NOT NULL,last_access_utc INTEGER NOT NULL,has_expires INTEGER NOT NULL,is_persistent INTEGER NOT NULL,priority INTEGER NOT NULL,samesite INTEGER NOT NULL,source_scheme INTEGER NOT NULL,source_port INTEGER NOT NULL,last_update_utc INTEGER NOT NULL,source_type INTEGER NOT NULL,has_cross_site_ancestor INTEGER NOT NULL); INSERT INTO cookies VALUES (1,'api.manus.im','','session_id','old',x'','/',2,1,1,1,1,1,1,1,2,443,1,0,0); INSERT INTO cookies VALUES (1,'other.example','','other','untouched',x'','/',2,1,1,1,1,1,1,1,2,443,1,0,0);").unwrap();
        // A database fixture must not query the runner's real Manus Keychain entry.
        let fixture_cipher =
            |_: &Path| Ok(super::super::electron_storage::test_ciphers().remove(1));
        set_cookie(&db, Path::new("unused"), Some("new"), fixture_cipher).unwrap();
        assert_eq!(
            cookie_value(&db, Path::new("unused"), fixture_cipher)
                .unwrap()
                .as_deref(),
            Some("new")
        );
        // Exercise all native encryption formats and Chromium's v24 host binding.
        for index in 0..3 {
            db.execute(
                "UPDATE cookies SET value='', encrypted_value=x'01' WHERE host_key=?1",
                [HOST],
            )
            .unwrap();
            let fixture_cipher =
                |_: &Path| Ok(super::super::electron_storage::test_ciphers().remove(index));
            set_cookie(
                &db,
                Path::new("unused"),
                Some("encrypted-token"),
                fixture_cipher,
            )
            .unwrap();
            assert_eq!(
                cookie_value(&db, Path::new("unused"), fixture_cipher)
                    .unwrap()
                    .as_deref(),
                Some("encrypted-token")
            );
            let (plain, encrypted): (String, Vec<u8>) = db
                .query_row(
                    "SELECT value,encrypted_value FROM cookies WHERE host_key=?1",
                    [HOST],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert!(plain.is_empty());
            let key = fixture_cipher(Path::new("unused")).unwrap();
            let bytes = decrypt_bytes(&key, &encrypted).unwrap();
            assert_eq!(&bytes[..32], Sha256::digest(HOST.as_bytes()).as_slice());
            assert_eq!(&bytes[32..], b"encrypted-token");
            assert!(
                set_cookie(&db, Path::new("unused"), Some("replacement"), |_| Err(
                    "accountError.keychain".into()
                ))
                .is_err()
            );
            assert_eq!(
                cookie_value(&db, Path::new("unused"), fixture_cipher)
                    .unwrap()
                    .as_deref(),
                Some("encrypted-token")
            );
        }
        let other: String = db
            .query_row(
                "SELECT value FROM cookies WHERE host_key='other.example'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(other, "untouched");
    }

    #[test]
    fn missing_native_cookie_database_means_not_signed_in() {
        let dir = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        assert!(native_session(&dir).unwrap().is_none());
        write_native(&dir, None).unwrap();
    }
}
