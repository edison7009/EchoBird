//! Official Claude Desktop sessions; the CLI OAuth store is intentionally separate.
//!
//! Claude Desktop cannot be signed in from outside, so accounts are snapshots:
//! the user signs in inside Claude Desktop and EchoBird saves that sign-in.
//! Switching puts a saved sign-in back while Claude Desktop is closed.
pub use super::claude_code_accounts::ClaudeCodeAccount as Account;
use super::cursor_auth::{read, write};
mod browser;
mod oauth;
use super::electron_storage::{cipher, decrypt_bytes, Cipher};
use rusqlite::{types::Value as SqlValue, Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

const AUTH_NAMES: &[&str] = &[
    "sessionKey",
    "sessionKeyV2",
    "sessionKeyV3",
    "sessionKeyLC",
    "sessionKeyV3LC",
    "lastActiveOrg",
    "authLastActiveOrg",
];
const AUTH_CONFIG: &[&str] = &[
    "lastKnownAccountUuid",
    "oauth:tokenCache",
    "oauth:tokenCacheV2",
];
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const WARM_UP_PAUSE: std::time::Duration = std::time::Duration::from_secs(2);
/// A finished snapshot waiting for the next poll of its attempt.
static PENDING: std::sync::Mutex<Option<(String, LoginPoll)>> = std::sync::Mutex::new(None);

#[derive(Clone, Serialize, Deserialize)]
enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}
impl Cell {
    fn sql(&self) -> SqlValue {
        match self {
            Self::Null => SqlValue::Null,
            Self::Integer(v) => SqlValue::Integer(*v),
            Self::Real(v) => SqlValue::Real(*v),
            Self::Text(v) => SqlValue::Text(v.clone()),
            Self::Blob(v) => SqlValue::Blob(v.clone()),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Cookie(BTreeMap<String, Cell>);
impl Cookie {
    fn text(&self, name: &str) -> Option<&str> {
        match self.0.get(name)? {
            Cell::Text(v) => Some(v),
            _ => None,
        }
    }
    fn integer(&self, name: &str) -> Option<i64> {
        match self.0.get(name)? {
            Cell::Integer(v) => Some(*v),
            _ => None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    cookies: Vec<Cookie>,
    config: BTreeMap<String, Value>,
    cookie_version: i64,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    account: Account,
    account_uuid: String,
    organization_uuid: String,
    snapshot: Snapshot,
}
type Store = BTreeMap<String, Saved>;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    login_id: String,
    expires_at: i64,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPoll {
    account: Option<Account>,
    awaiting_client_exit: bool,
    expires_at: Option<i64>,
    /// The sign-in was removed from this device for the next account.
    cleared: bool,
}

/// Claude Desktop's data folder. The Microsoft Store app kept it in its
/// package folder and, since October 2026, keeps it in
/// `%LOCALAPPDATA%\Claude-Data\claude_<publisher>`; the direct download uses
/// `%APPDATA%\Claude`. Old folders can stay behind without a web session, so
/// among folders with a cookie store, the one whose config.json changed last
/// is in use.
pub(crate) fn data_dir() -> Result<PathBuf, String> {
    let default = dirs::config_dir()
        .map(|p| p.join("Claude"))
        .ok_or("accountError.home")?;
    let mut candidates = store_data_dirs();
    candidates.push(default.clone());
    Ok(usable_data_dir(candidates).unwrap_or(default))
}
/// The Microsoft Store app's current and earlier data folders.
#[cfg(windows)]
fn store_data_dirs() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(dir) = super::tool_manager::get_tool_launch_uri("claudedesktop")
        .and_then(|uri| super::msix::roaming_data_dir(&uri, "Claude"))
    {
        candidates.push(dir);
    }
    if let Some(moved) = dirs::data_local_dir().map(|dir| dir.join("Claude-Data")) {
        if let Ok(entries) = std::fs::read_dir(moved) {
            candidates.extend(entries.flatten().map(|entry| entry.path()));
        }
    }
    candidates
}
#[cfg(not(windows))]
fn store_data_dirs() -> Vec<PathBuf> {
    Vec::new()
}
fn usable_data_dir(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    let (signed_in, others): (Vec<_>, Vec<_>) = candidates
        .into_iter()
        .partition(|dir| cookie_path(dir).is_ok());
    newest_config_dir(signed_in).or_else(|| newest_config_dir(others))
}
fn newest_config_dir(candidates: Vec<PathBuf>) -> Option<PathBuf> {
    candidates
        .into_iter()
        .filter_map(|dir| {
            let changed = std::fs::metadata(dir.join("config.json"))
                .and_then(|meta| meta.modified())
                .ok()?;
            Some((changed, dir))
        })
        .max_by_key(|(changed, _)| *changed)
        .map(|(_, dir)| dir)
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/claude-desktop-accounts.json"))
}
fn load_store() -> Result<Store, String> {
    load_store_at(&store_path()?)
}
fn load_store_at(path: &Path) -> Result<Store, String> {
    if path.exists() {
        read(path)
    } else {
        Ok(Store::new())
    }
}
fn cookie_path(dir: &Path) -> Result<PathBuf, String> {
    [
        "Network/Cookies",
        "Cookies",
        "Default/Network/Cookies",
        "Default/Cookies",
    ]
    .iter()
    .map(|p| dir.join(p))
    .find(|p| p.is_file())
    .ok_or_else(|| "accountError.initializeClient".into())
}
fn cookie_db(dir: &Path, writable: bool) -> Result<Connection, String> {
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db = Connection::open_with_flags(cookie_path(dir)?, flags).map_err(|_| {
        if writable {
            "accountError.write"
        } else {
            "accountError.read"
        }
    })?;
    db.busy_timeout(Duration::from_millis(100))
        .map_err(|_| "accountError.read")?;
    Ok(db)
}
fn is_auth_cookie(host: &str, name: &str) -> bool {
    AUTH_NAMES.contains(&name) && matches!(host.trim_start_matches('.'), "claude.ai" | "claude.com")
}
fn cookies(db: &Connection) -> Result<Vec<Cookie>, String> {
    let names = AUTH_NAMES.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut statement = db.prepare(&format!("SELECT * FROM cookies WHERE name IN ({names}) AND host_key IN ('claude.ai','.claude.ai','claude.com','.claude.com')")).map_err(|_| "accountError.read")?;
    let columns = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let result = statement
        .query_map(rusqlite::params_from_iter(AUTH_NAMES), |row| {
            let mut values = BTreeMap::new();
            for (i, name) in columns.iter().enumerate() {
                let cell = match row.get::<_, SqlValue>(i)? {
                    SqlValue::Null => Cell::Null,
                    SqlValue::Integer(v) => Cell::Integer(v),
                    SqlValue::Real(v) => Cell::Real(v),
                    SqlValue::Text(v) => Cell::Text(v),
                    SqlValue::Blob(v) => Cell::Blob(v),
                };
                values.insert(name.clone(), cell);
            }
            Ok(Cookie(values))
        })
        .map_err(|_| "accountError.read")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "accountError.read".into());
    result
}
fn native_config(dir: &Path) -> Result<Value, String> {
    if dir.join("config.json").exists() {
        read(&dir.join("config.json"))
    } else {
        Ok(json!({}))
    }
}
fn snapshot(dir: &Path) -> Result<Snapshot, String> {
    let config = native_config(dir)?;
    let db = cookie_db(dir, false)?;
    let cookie_version = cookie_version(&db);
    Ok(Snapshot {
        cookies: cookies(&db)?,
        config: auth_config(&config),
        cookie_version,
    })
}
fn primary_cookie<'a>(snapshot: &'a Snapshot, name: &str) -> Option<&'a Cookie> {
    let now = (chrono::Utc::now().timestamp() + 11_644_473_600) * 1_000_000;
    snapshot
        .cookies
        .iter()
        .filter(|c| {
            c.text("name") == Some(name)
                && c.text("path") == Some("/")
                && (c.integer("has_expires") == Some(0)
                    || c.integer("expires_utc").is_some_and(|e| e > now))
        })
        .max_by_key(|c| {
            (
                c.text("host_key").is_some_and(|s| s.contains("claude.ai")),
                c.integer("last_update_utc"),
            )
        })
}
/// Claude Desktop reads `sessionKey` and falls back to the newer names, and
/// `lastActiveOrg` before the sign-in page's copy.
fn first_cookie<'a>(snapshot: &'a Snapshot, names: &[&str]) -> Option<&'a Cookie> {
    names.iter().find_map(|name| primary_cookie(snapshot, name))
}
fn session_cookie(snapshot: &Snapshot) -> Option<&Cookie> {
    first_cookie(snapshot, &["sessionKey", "sessionKeyV3", "sessionKeyV2"])
}
fn organization_cookie(snapshot: &Snapshot) -> Option<&Cookie> {
    first_cookie(snapshot, &["lastActiveOrg", "authLastActiveOrg"])
}
fn cookie_value(version: i64, cookie: &Cookie, cipher: &Cipher) -> Result<String, String> {
    if version == browser::COOKIE_VERSION {
        return browser::value(cookie);
    }
    if let Some(value) = cookie.text("value").filter(|v| !v.is_empty()) {
        return Ok(value.into());
    }
    let Some(Cell::Blob(bytes)) = cookie.0.get("encrypted_value") else {
        return Err("accountError.format".into());
    };
    let bytes = decrypt_bytes(cipher, bytes)?;
    let bytes = if version >= 24 {
        bytes
            .strip_prefix(
                Sha256::digest(
                    cookie
                        .text("host_key")
                        .ok_or("accountError.format")?
                        .as_bytes(),
                )
                .as_slice(),
            )
            .ok_or("accountError.format")?
    } else {
        &bytes
    };
    String::from_utf8(bytes.to_vec()).map_err(|_| "accountError.format".into())
}
fn session_values(dir: &Path, snapshot: &Snapshot) -> Result<(String, String), String> {
    let session_cookie = session_cookie(snapshot).ok_or("accountError.loginRequired")?;
    let org_cookie = organization_cookie(snapshot).ok_or("accountError.loginRequired")?;
    let browser = snapshot.cookie_version == browser::COOKIE_VERSION;
    let plain = [session_cookie, org_cookie]
        .iter()
        .all(|cookie| cookie.text("value").is_some_and(|value| !value.is_empty()));
    let key = if browser || plain {
        None
    } else {
        Some(cipher(dir)?)
    };
    let value = |cookie: &Cookie| match &key {
        Some(key) => cookie_value(snapshot.cookie_version, cookie, key),
        None if browser => browser::value(cookie),
        None => cookie
            .text("value")
            .map(str::to_owned)
            .ok_or_else(|| "accountError.format".into()),
    };
    let session = value(session_cookie)?;
    let org = value(org_cookie)?;
    uuid::Uuid::parse_str(&org).map_err(|_| "accountError.authResponse")?;
    if session.is_empty() || session.contains(['\r', '\n', ';']) {
        return Err("accountError.format".into());
    }
    Ok((session, org))
}
fn account_id(uuid: &str, org: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{uuid}\n{org}").as_bytes()))
}
fn validate_saved_identity(saved: &Saved, id: &str, org: &str) -> Result<(), String> {
    if uuid::Uuid::parse_str(&saved.account_uuid).is_err()
        || saved.account.id != id
        || account_id(&saved.account_uuid, org) != id
        || org != saved.organization_uuid
        || saved
            .snapshot
            .config
            .get("lastKnownAccountUuid")
            .and_then(Value::as_str)
            .is_some_and(|uuid| uuid != saved.account_uuid)
        || saved
            .snapshot
            .config
            .keys()
            .any(|name| !AUTH_CONFIG.contains(&name.as_str()))
        || saved.snapshot.cookies.iter().any(|cookie| {
            !is_auth_cookie(
                cookie.text("host_key").unwrap_or_default(),
                cookie.text("name").unwrap_or_default(),
            )
        })
    {
        return Err("accountError.authResponse".into());
    }
    Ok(())
}

fn usage_window(
    body: &Value,
    legacy_field: &str,
    kind: &str,
) -> Option<super::claude_code_accounts::ClaudeCodeQuota> {
    super::claude_code_accounts::quota(&body[legacy_field]).or_else(|| {
        let limit = body["limits"].as_array()?.iter().find(|limit| {
            (limit["kind"].as_str() == Some(kind) || limit["group"].as_str() == Some(kind))
                && limit["scope"]["model"].is_null()
        })?;
        let used = limit["percent"]
            .as_f64()
            .or_else(|| limit["percent"].as_str()?.parse::<f64>().ok())?;
        let mut quota = super::claude_code_accounts::quota(
            &json!({"utilization":used,"resets_at":limit["resets_at"]}),
        )?;
        if let Some(seconds) = limit["resets_at"].as_i64() {
            quota.reset_at = Some(seconds);
        }
        Some(quota)
    })
}
fn set_usage(account: &mut Account, body: &Value) -> Result<(), String> {
    account.five_hour = usage_window(body, "five_hour", "session");
    account.seven_day = usage_window(body, "seven_day", "weekly");
    let free_without_limits = account.plan.as_deref() == Some("Free")
        && body["limits"]
            .as_array()
            .is_some_and(|limits| limits.is_empty());
    if account.five_hour.is_none() && account.seven_day.is_none() && !free_without_limits {
        return Err("accountError.quota".into());
    }
    Ok(())
}
fn fingerprint(snapshot: &Snapshot) -> Option<String> {
    session_cookie(snapshot).map(|c| {
        let fields = [Some(c), organization_cookie(snapshot)]
            .into_iter()
            .flat_map(|c| {
                ["host_key", "name", "path", "value", "encrypted_value"]
                    .into_iter()
                    .map(move |k| c.and_then(|c| c.0.get(k)))
            })
            .collect::<Vec<_>>();
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&fields).unwrap_or_default())
        )
    })
}
pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    let dir = data_dir()?;
    // config.json stays readable while Claude Desktop runs; its cookie store
    // does not, so the cookie fingerprint only refines the match when readable.
    let signed_in = signed_in_account(&native_config(&dir).unwrap_or_default());
    let live = snapshot(&dir).ok().and_then(|s| fingerprint(&s));
    // Without the cookie store the organization is unknown, so the account
    // only counts as active when a single saved snapshot belongs to it.
    let same_account = store
        .values()
        .filter(|s| signed_in.as_deref() == Some(s.account_uuid.as_str()))
        .count();
    let mut accounts = store
        .values_mut()
        .map(|s| {
            s.account.active = match &live {
                Some(live) => Some(live) == fingerprint(&s.snapshot).as_ref(),
                None => same_account == 1 && signed_in.as_deref() == Some(s.account_uuid.as_str()),
            };
            s.account.clone()
        })
        .collect::<Vec<_>>();
    accounts.sort_by(|a, b| a.email.cmp(&b.email).then(a.id.cmp(&b.id)));
    Ok(accounts)
}
/// The account Claude Desktop is signed in to, from its config.json.
fn signed_in_account(config: &Value) -> Option<String> {
    config["oauth:tokenCacheV2"].as_str()?;
    config["lastKnownAccountUuid"]
        .as_str()
        .filter(|uuid| uuid::Uuid::parse_str(uuid).is_ok())
        .map(str::to_owned)
}
/// The sign-in entries of a config.json object.
fn auth_config(config: &Value) -> BTreeMap<String, Value> {
    AUTH_CONFIG
        .iter()
        .filter_map(|key| {
            config
                .get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect()
}
/// The user quits Claude Desktop before snapshots and switches: its cookie
/// store is locked while it runs, and a forced stop would lose the session it
/// has not written yet.
async fn require_closed() -> Result<(), String> {
    if super::process_manager::desktop_tool_is_running("claudedesktop").await? {
        return Err("accountError.closeClient".into());
    }
    Ok(())
}
/// Saves the account Claude Desktop is signed in to and changes nothing else.
/// With `next`, it then removes that sign-in from this device so the next
/// account can sign in. Signing out inside Claude Desktop would instead revoke
/// the session on the server and end the saved snapshot.
pub async fn start_login(next: bool) -> Result<LoginStart, String> {
    let dir = data_dir()?;
    cookie_path(&dir)?;
    let uuid = signed_in_account(&native_config(&dir)?).ok_or("accountError.noAccount")?;
    require_closed().await?;
    let (snapshot, org) = read_signed_in(&dir, &uuid)?;
    let token = oauth::access_token(&dir, &snapshot.config, &uuid, &org, oauth::PROFILE_SCOPE)?;
    let profile = oauth::profile(&token).await?;
    let account = {
        // Switches write the same files; the client may also have been
        // reopened during the profile request.
        let _guard = ACCOUNT_LOCK.lock().await;
        if next {
            require_closed().await?;
        }
        let account = store_snapshot(&store_path()?, &uuid, &org, snapshot, &profile, !next)?;
        if next {
            clear_sign_in(&dir)?;
        }
        account
    };
    let login_id = uuid::Uuid::new_v4().to_string();
    *PENDING.lock().map_err(|_| "accountError.busy")? = Some((
        login_id.clone(),
        LoginPoll {
            account: Some(account),
            cleared: next,
            ..Default::default()
        },
    ));
    Ok(LoginStart {
        login_id,
        expires_at: chrono::Utc::now().timestamp() + 60,
    })
}
/// The cookie store's schema version; 24 added host-bound cookie values.
fn cookie_version(db: &Connection) -> i64 {
    db.query_row("SELECT value FROM meta WHERE key='version'", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .and_then(|s| s.parse::<i64>().ok())
    .unwrap_or(0)
}
/// Removes the closed client's sign-in locally, leaving the session valid.
fn clear_sign_in(dir: &Path) -> Result<(), String> {
    let mut db = cookie_db(dir, true)?;
    let cookie_version = cookie_version(&db);
    let nothing = Snapshot {
        cookies: Vec::new(),
        config: BTreeMap::new(),
        cookie_version,
    };
    restore(&mut db, &dir.join("config.json"), &nothing)
}
/// Reads the closed client's sign-in as `uuid` and the organization it uses.
fn read_signed_in(dir: &Path, uuid: &str) -> Result<(Snapshot, String), String> {
    if signed_in_account(&native_config(dir)?).as_deref() != Some(uuid) {
        return Err("accountError.noAccount".into());
    }
    let mut snapshot = snapshot(dir)?;
    let (_, org) = session_values(dir, &snapshot).map_err(|error| {
        if error == "accountError.loginRequired" {
            "accountError.noAccount".to_string()
        } else {
            error
        }
    })?;
    snapshot
        .config
        .insert("lastKnownAccountUuid".into(), json!(uuid));
    Ok((snapshot, org))
}
/// Saves a sign-in described by `profile`; `active` while the device keeps it.
fn store_snapshot(
    store_path: &Path,
    uuid: &str,
    org: &str,
    snapshot: Snapshot,
    profile: &Value,
    active: bool,
) -> Result<Account, String> {
    let mut account = oauth::account_from_profile(profile, uuid, org)?;
    let mut store = load_store_at(store_path)?;
    if let Some(previous) = store.get(&account.id) {
        account.five_hour = previous.account.five_hour.clone();
        account.seven_day = previous.account.seven_day.clone();
    }
    account.active = active;
    for saved in store.values_mut() {
        saved.account.active = false;
    }
    store.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            account_uuid: uuid.to_owned(),
            organization_uuid: org.to_owned(),
            snapshot,
        },
    );
    write(store_path, &store)?;
    Ok(account)
}
pub async fn poll_login(id: &str) -> Result<LoginPoll, String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    match pending.take() {
        Some((login, result)) if login == id => Ok(result),
        other => {
            *pending = other;
            Err("accountError.cancelled".into())
        }
    }
}
pub async fn cancel_login(id: &str) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    if pending.as_ref().is_some_and(|(login, _)| login == id) {
        *pending = None;
    }
    Ok(())
}
fn restore(db: &mut Connection, config_path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let version = cookie_version(db);
    if (version >= 24) != (snapshot.cookie_version >= 24) {
        return Err("accountError.format".into());
    }
    let mut config: Value = read(config_path)?;
    let original = config.clone();
    let object = config.as_object_mut().ok_or("accountError.format")?;
    for name in AUTH_CONFIG {
        object.remove(*name);
    }
    for (name, value) in &snapshot.config {
        if !AUTH_CONFIG.contains(&name.as_str()) {
            return Err("accountError.format".into());
        }
        object.insert(name.clone(), value.clone());
    }
    let columns = db
        .prepare("SELECT * FROM cookies LIMIT 0")
        .map_err(|_| "accountError.read")?
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let tx = db.transaction().map_err(|_| "accountError.write")?;
    let names = AUTH_NAMES.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    tx.execute(&format!("DELETE FROM cookies WHERE name IN ({names}) AND host_key IN ('claude.ai','.claude.ai','claude.com','.claude.com')"), rusqlite::params_from_iter(AUTH_NAMES)).map_err(|_| "accountError.write")?;
    for cookie in &snapshot.cookies {
        if !is_auth_cookie(
            cookie.text("host_key").ok_or("accountError.format")?,
            cookie.text("name").ok_or("accountError.format")?,
        ) {
            return Err("accountError.format".into());
        }
        let fields = columns
            .iter()
            .filter(|k| cookie.0.contains_key(*k))
            .collect::<Vec<_>>();
        let quoted = fields
            .iter()
            .map(|k| format!("\"{}\"", k.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(",");
        let placeholders = fields.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let values = fields
            .iter()
            .map(|k| cookie.0[*k].sql())
            .collect::<Vec<_>>();
        tx.execute(
            &format!("INSERT INTO cookies ({quoted}) VALUES ({placeholders})"),
            rusqlite::params_from_iter(values),
        )
        .map_err(|_| "accountError.write")?;
    }
    write(config_path, &config)?;
    if tx.commit().is_err() {
        write(config_path, &original)?;
        return Err("accountError.write".into());
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct LocalModeBackup {
    #[serde(default)]
    prepared: bool,
    directory: PathBuf,
    snapshot: Option<Snapshot>,
    config: BTreeMap<String, Value>,
}

fn local_mode_backup_path() -> Result<PathBuf, String> {
    Ok(store_path()?.with_file_name("claude-desktop-local-auth.json"))
}

/// Capture native authentication once before the local identity replaces it.
/// Repeated starts preserve the original backup until restoration succeeds.
pub(crate) async fn begin_local_mode() -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    require_closed().await?;
    let path = local_mode_backup_path()?;
    capture_local_mode_at(&data_dir()?, &path)?;
    let mut backup: LocalModeBackup = read(&path)?;
    if !backup.prepared {
        if cookie_path(&backup.directory).is_ok() {
            clear_sign_in(&backup.directory)?;
        } else {
            let mut config = native_config(&backup.directory)?;
            let object = config.as_object_mut().ok_or("accountError.format")?;
            for key in AUTH_CONFIG {
                object.remove(*key);
            }
            write(&backup.directory.join("config.json"), &config)?;
        }
        backup.prepared = true;
        write(&path, &backup)?;
    }
    Ok(())
}

fn capture_local_mode_at(directory: &Path, path: &Path) -> Result<(), String> {
    if path.exists() {
        let _: LocalModeBackup = read(path)?;
        return Ok(());
    }
    let config = auth_config(&native_config(directory)?);
    let snapshot = if cookie_path(directory).is_ok() {
        Some(snapshot(directory)?)
    } else {
        None
    };
    write(
        path,
        &LocalModeBackup {
            prepared: false,
            directory: directory.into(),
            snapshot,
            config,
        },
    )
}

/// Restore only authentication. The model-config owner restores deployment mode.
pub(crate) async fn end_local_mode() -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    restore_local_mode().await
}

async fn restore_local_mode() -> Result<(), String> {
    let path = local_mode_backup_path()?;
    if !path.exists() {
        return Ok(());
    }
    require_closed().await?;
    restore_local_mode_at(&path)
}

fn restore_local_mode_at(path: &Path) -> Result<(), String> {
    let backup: LocalModeBackup = read(path)?;
    let config_path = backup.directory.join("config.json");
    if cookie_path(&backup.directory).is_ok() {
        let mut db = cookie_db(&backup.directory, true)?;
        let snapshot = backup.snapshot.unwrap_or_else(|| Snapshot {
            cookies: Vec::new(),
            config: backup.config,
            cookie_version: cookie_version(&db),
        });
        if !config_path.exists() {
            write(&config_path, &json!({}))?;
        }
        restore(&mut db, &config_path, &snapshot)?;
    } else {
        // No cookie store existed at entry and none has been created since.
        if backup.snapshot.is_some() {
            return Err("accountError.initializeClient".into());
        }
        let mut config = native_config(&backup.directory)?;
        let object = config.as_object_mut().ok_or("accountError.format")?;
        for key in AUTH_CONFIG {
            object.remove(*key);
        }
        object.extend(backup.config);
        write(&config_path, &config)?;
    }
    std::fs::remove_file(path).map_err(|_| "accountError.write".into())
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    restore_local_mode().await?;
    switch_at(
        id,
        &data_dir()?,
        &store_path()?,
        require_closed(),
        |target| super::claude_desktop_history::prepare(Some(target)),
    )
    .await
}
async fn switch_at<S, H, HF>(
    id: &str,
    dir: &Path,
    store_path: &Path,
    stop: S,
    prepare_history: H,
) -> Result<Account, String>
where
    S: std::future::Future<Output = Result<(), String>>,
    H: FnOnce(PathBuf) -> HF,
    HF: std::future::Future<Output = Result<(), String>>,
{
    let mut store = load_store_at(store_path)?;
    let saved = store.get(id).ok_or("accountError.invalidAccount")?;
    let (_session, org) = session_values(dir, &saved.snapshot)?;
    validate_saved_identity(saved, id, &org)?;
    stop.await?;
    // Claude Desktop renews its tokens while it runs. Keep the signed-in
    // account's current sign-in, read before the target replaces it.
    let live = snapshot(dir).ok().and_then(|live| {
        let uuid = signed_in_account(&native_config(dir).ok()?)?;
        let (_, org) = session_values(dir, &live).ok()?;
        Some((uuid, org, live))
    });
    prepare_history(
        dir.join("claude-code-sessions")
            .join(&saved.account_uuid)
            .join(&saved.organization_uuid),
    )
    .await?;
    let mut db = cookie_db(dir, true)?;
    let snapshot = if saved.snapshot.cookie_version == browser::COOKIE_VERSION {
        browser::native_snapshot(&db, &saved.snapshot, &cipher(dir)?)?
    } else {
        saved.snapshot.clone()
    };
    restore(&mut db, &dir.join("config.json"), &snapshot)?;
    store
        .get_mut(id)
        .ok_or("accountError.invalidAccount")?
        .snapshot = snapshot;
    remember_live(&mut store, id, live);
    for (key, saved) in &mut store {
        saved.account.active = key == id;
    }
    let account = store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone();
    write(store_path, &store)?;
    Ok(account)
}
/// Stores the signed-in account's current sign-in in its saved snapshot.
fn remember_live(store: &mut Store, target: &str, live: Option<(String, String, Snapshot)>) {
    let Some((uuid, org, mut live)) = live else {
        return;
    };
    live.config
        .insert("lastKnownAccountUuid".into(), json!(uuid));
    for (id, saved) in store.iter_mut() {
        if id != target && saved.account_uuid == uuid && saved.organization_uuid == org {
            saved.snapshot = live.clone();
        }
    }
}
/// The account's identity and a token from its newest sign-in.
async fn saved_token(id: &str, scope: &str) -> Result<(String, String, String), String> {
    let (dir, config, uuid, org) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        let store = load_store()?;
        let saved = store.get(id).ok_or("accountError.invalidAccount")?;
        let dir = data_dir()?;
        let (_, org) = session_values(&dir, &saved.snapshot)?;
        // The signed-in account's tokens are newest in Claude Desktop itself.
        let live = native_config(&dir).unwrap_or_default();
        let config = if signed_in_account(&live).as_deref() == Some(saved.account_uuid.as_str()) {
            auth_config(&live)
        } else {
            saved.snapshot.config.clone()
        };
        (dir, config, saved.account_uuid.clone(), org)
    };
    // Snapshots saved by earlier versions hold no tokens, and a saved token
    // expires until Claude Desktop renews it while that account is in use.
    let token = oauth::access_token(&dir, &config, &uuid, &org, scope).map_err(|error| {
        if ["accountError.noAccount", "accountError.initializeClient"].contains(&error.as_str()) {
            "accountError.loginRequired".to_string()
        } else {
            error
        }
    })?;
    Ok((uuid, org, token))
}
pub async fn refresh(id: &str) -> Result<Account, String> {
    let (uuid, org, token) = saved_token(id, oauth::PROFILE_SCOPE).await?;
    let mut account = oauth::account_from_profile(&oauth::profile(&token).await?, &uuid, &org)?;
    if account.id != id {
        return Err("accountError.authResponse".into());
    }
    set_usage(&mut account, &oauth::usage(&token).await?)?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    account.active = saved.account.active;
    saved.account = account.clone();
    write(&store_path()?, &store)?;
    Ok(account)
}
/// Saved sign-ins are never renewed here, so one left idle has no live token. The
/// same account saved for Claude Code shares its id and keeps a renewable one.
async fn warm_up_refresh(id: &str) -> Result<Account, String> {
    let error = match refresh(id).await {
        Ok(account) => return Ok(account),
        Err(error) => error,
    };
    let usage = super::claude_code_accounts::refresh(id)
        .await
        .map_err(|_| error)?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    saved.account.five_hour = usage.five_hour;
    saved.account.seven_day = usage.seven_day;
    let account = saved.account.clone();
    write(&store_path()?, &store)?;
    Ok(account)
}
pub async fn warm_up(id: &str, executable: &str) -> Result<Account, String> {
    super::claude_code_accounts::warm_up_with(
        || warm_up_refresh(id),
        || async {
            let credentials = match saved_token(id, oauth::CLIENT_SCOPE).await {
                Ok((_, _, token)) => json!({"accessToken":token,"accountId":id}),
                Err(error) => super::claude_code_accounts::proxy_credentials(id, false)
                    .await
                    .map_err(|_| error)?,
            };
            super::account_proxy::warm_up_claude(&credentials, executable).await
        },
        WARM_UP_PAUSE,
    )
    .await
}
pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    store.remove(id).ok_or("accountError.invalidAccount")?;
    write(&store_path()?, &store)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ApplyFixture {
        dir: PathBuf,
        id: String,
    }
    impl ApplyFixture {
        fn new() -> Self {
            let dir = std::env::temp_dir()
                .join(format!("echobird-claude-apply-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&dir).unwrap();
            let db = database("");
            db.execute("VACUUM INTO ?1", [dir.join("Cookies").to_str().unwrap()])
                .unwrap();
            let account_uuid = "11111111-1111-4111-8111-111111111111";
            let org = "22222222-2222-4222-8222-222222222222";
            let id = account_id(account_uuid, org);
            let mut session = cookies(&db).unwrap().remove(0);
            session
                .0
                .insert("value".into(), Cell::Text("offline-fixture-session".into()));
            let mut organization = session.clone();
            organization
                .0
                .insert("name".into(), Cell::Text("lastActiveOrg".into()));
            organization
                .0
                .insert("value".into(), Cell::Text(org.into()));
            write(
                &dir.join("accounts.json"),
                &Store::from([(
                    id.clone(),
                    Saved {
                        account: Account {
                            id: id.clone(),
                            email: "fixture@example.test".into(),
                            plan: Some("Pro".into()),
                            five_hour: None,
                            seven_day: None,
                            active: false,
                        },
                        account_uuid: account_uuid.into(),
                        organization_uuid: org.into(),
                        snapshot: Snapshot {
                            cookies: vec![session, organization],
                            config: BTreeMap::from([(
                                "lastKnownAccountUuid".into(),
                                json!(account_uuid),
                            )]),
                            cookie_version: 24,
                        },
                    },
                )]),
            )
            .unwrap();
            write(
                &dir.join("config.json"),
                &json!({"locale":"ja","lastKnownAccountUuid":"old"}),
            )
            .unwrap();
            Self { dir, id }
        }
        fn state(&self) -> Vec<Vec<u8>> {
            ["accounts.json", "config.json", "Cookies"]
                .iter()
                .map(|name| std::fs::read(self.dir.join(name)).unwrap())
                .collect()
        }
    }
    impl Drop for ApplyFixture {
        fn drop(&mut self) {
            for name in ["accounts.json", "config.json", "Cookies"] {
                std::fs::remove_file(self.dir.join(name)).unwrap();
            }
            std::fs::remove_dir(&self.dir).unwrap();
        }
    }

    #[tokio::test]
    async fn cached_apply_works_offline_and_stops_before_history_and_native_writes() {
        let fixture = ApplyFixture::new();
        let path = fixture.dir.join("accounts.json");
        let store = load_store_at(&path).unwrap();
        let saved = &store[&fixture.id];
        // Claude requests need a browser window, which applying never opens.
        let (session, org) = session_values(&fixture.dir, &saved.snapshot).unwrap();
        assert!(!session.is_empty());
        let stage = std::cell::Cell::new(0);
        let before = fixture.state();
        let account = switch_at(
            &fixture.id,
            &fixture.dir,
            &path,
            async {
                assert_eq!(stage.replace(1), 0);
                assert_eq!(fixture.state(), before);
                Ok(())
            },
            |target| {
                assert_eq!(stage.replace(2), 1);
                assert_eq!(fixture.state(), before);
                assert_eq!(
                    target,
                    fixture
                        .dir
                        .join("claude-code-sessions")
                        .join(&saved.account_uuid)
                        .join(&saved.organization_uuid)
                );
                async { Ok(()) }
            },
        )
        .await
        .unwrap();
        assert_eq!(stage.get(), 2);
        assert_eq!(account.id, fixture.id);
        assert!(account.active);
        assert!(load_store_at(&path).unwrap()[&fixture.id].account.active);
        let native = snapshot(&fixture.dir).unwrap();
        assert_eq!(
            session_values(&fixture.dir, &native).unwrap(),
            (session, org)
        );
        assert_eq!(
            native.config["lastKnownAccountUuid"],
            json!(saved.account_uuid)
        );
        assert_eq!(native_config(&fixture.dir).unwrap()["locale"], json!("ja"));
        let db = cookie_db(&fixture.dir, false).unwrap();
        let unrelated: Vec<String> = db.prepare("SELECT value FROM cookies WHERE name='theme' OR host_key='example.test' ORDER BY value")
            .unwrap().query_map([], |row| row.get(0)).unwrap()
            .collect::<Result<_, _>>().unwrap();
        assert_eq!(unrelated, ["dark", "other-app"]);
    }

    #[tokio::test]
    async fn invalid_cached_identity_does_not_stop_or_write() {
        for field in [
            "account_id",
            "account_uuid",
            "organization_uuid",
            "config_uuid",
        ] {
            let fixture = ApplyFixture::new();
            let path = fixture.dir.join("accounts.json");
            let mut store = load_store_at(&path).unwrap();
            let saved = store.get_mut(&fixture.id).unwrap();
            match field {
                "account_id" => saved.account.id = "wrong".into(),
                "account_uuid" => {
                    saved.account_uuid = "33333333-3333-4333-8333-333333333333".into()
                }
                "organization_uuid" => {
                    saved.organization_uuid = "33333333-3333-4333-8333-333333333333".into()
                }
                "config_uuid" => {
                    saved
                        .snapshot
                        .config
                        .insert("lastKnownAccountUuid".into(), json!("other-account"));
                }
                _ => unreachable!(),
            }
            write(&path, &store).unwrap();
            let before = fixture.state();
            let result = switch_at(
                &fixture.id,
                &fixture.dir,
                &path,
                async { panic!("invalid identity must not stop the client") },
                |_| async { panic!("invalid identity must not prepare history") },
            )
            .await;
            assert!(matches!(result, Err(error) if error == "accountError.authResponse"));
            assert_eq!(fixture.state(), before);
        }
    }

    #[tokio::test]
    async fn failed_stop_does_not_prepare_history_or_write_credentials() {
        let fixture = ApplyFixture::new();
        let before = fixture.state();
        let result = switch_at(
            &fixture.id,
            &fixture.dir,
            &fixture.dir.join("accounts.json"),
            async { Err("accountError.stopClient".into()) },
            |_| async { panic!("failed stop must not prepare history") },
        )
        .await;
        assert!(matches!(result, Err(error) if error == "accountError.stopClient"));
        assert_eq!(fixture.state(), before);
    }

    fn database(extra: &str) -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(&format!("CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,value TEXT,encrypted_value BLOB,has_expires INTEGER,expires_utc INTEGER,last_update_utc INTEGER{extra});
            CREATE TABLE meta(key TEXT,value TEXT); INSERT INTO meta VALUES('version','24');
            INSERT INTO cookies(host_key,name,path,value,encrypted_value,has_expires,expires_utc,last_update_utc) VALUES('.claude.ai','sessionKey','/','fixture-old',X'',0,0,1),('.claude.ai','theme','/','dark',X'',0,0,1),('example.test','sessionKey','/','other-app',X'',0,0,1);" )).unwrap();
        db
    }
    #[test]
    fn restore_preserves_unrelated_settings_and_cookies_in_current_and_legacy_schemas() {
        for extra in ["", ",source_type INTEGER DEFAULT 0"] {
            let mut db = database(extra);
            let mut captured = cookies(&db).unwrap();
            assert_eq!(captured.len(), 1);
            captured[0]
                .0
                .insert("value".into(), Cell::Text("fixture-new".into()));
            let dir = std::env::temp_dir().join(format!(
                "echobird-claude-desktop-test-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let config_path = dir.join("config.json");
            write(&config_path, &json!({"locale":"ja","lastKnownAccountUuid":"old","oauth:tokenCache":"old-cache","oauth:tokenCacheV2":"old-v2","coworkTrustedDeviceToken:other":"keep"})).unwrap();
            let saved = Snapshot {
                cookies: captured,
                config: BTreeMap::from([
                    ("lastKnownAccountUuid".into(), json!("new")),
                    ("oauth:tokenCache".into(), json!("new-cache")),
                ]),
                cookie_version: 24,
            };
            restore(&mut db, &config_path, &saved).unwrap();
            let rows = db
                .prepare("SELECT value FROM cookies ORDER BY value")
                .unwrap()
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(rows, ["dark", "fixture-new", "other-app"]);
            let config: Value = read(&config_path).unwrap();
            assert_eq!(
                config,
                json!({"locale":"ja","lastKnownAccountUuid":"new","oauth:tokenCache":"new-cache","coworkTrustedDeviceToken:other":"keep"})
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
    #[test]
    fn invalid_snapshot_rolls_back_cookie_changes_without_touching_config() {
        let mut db = database("");
        let dir = std::env::temp_dir().join(format!(
            "echobird-claude-desktop-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        let config = json!({"locale":"en"});
        write(&path, &config).unwrap();
        let mut rows = cookies(&db).unwrap();
        rows[0]
            .0
            .insert("host_key".into(), Cell::Text("unrelated.test".into()));
        let mut snapshot = Snapshot {
            cookies: rows,
            config: BTreeMap::new(),
            cookie_version: 24,
        };
        assert_eq!(
            restore(&mut db, &path, &snapshot).unwrap_err(),
            "accountError.format"
        );
        assert_eq!(cookies(&db).unwrap()[0].text("value"), Some("fixture-old"));
        assert_eq!(read::<Value>(&path).unwrap(), config);
        snapshot.cookies[0]
            .0
            .insert("host_key".into(), Cell::Text(".claude.ai".into()));
        snapshot.cookie_version = 23;
        assert_eq!(
            restore(&mut db, &path, &snapshot).unwrap_err(),
            "accountError.format"
        );
        assert_eq!(cookies(&db).unwrap()[0].text("value"), Some("fixture-old"));
        assert_eq!(read::<Value>(&path).unwrap(), config);
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn cookie_decryption_verifies_host_binding_and_expiry() {
        let db = database("");
        let mut cookie = cookies(&db).unwrap().remove(0);
        let mut bytes = Sha256::digest(b".claude.ai").to_vec();
        bytes.extend_from_slice(b"fixture-session");
        cookie.0.insert("value".into(), Cell::Text(String::new()));
        for key in super::super::electron_storage::test_ciphers() {
            cookie.0.insert(
                "encrypted_value".into(),
                Cell::Blob(super::super::electron_storage::encrypt_bytes(&key, &bytes).unwrap()),
            );
            assert_eq!(cookie_value(24, &cookie, &key).unwrap(), "fixture-session");
            let mut wrong_host = cookie.clone();
            wrong_host
                .0
                .insert("host_key".into(), Cell::Text(".claude.com".into()));
            assert_eq!(
                cookie_value(24, &wrong_host, &key).unwrap_err(),
                "accountError.format"
            );
            let mut legacy = cookie.clone();
            legacy.0.insert(
                "encrypted_value".into(),
                Cell::Blob(
                    super::super::electron_storage::encrypt_bytes(&key, b"legacy-session").unwrap(),
                ),
            );
            assert_eq!(cookie_value(23, &legacy, &key).unwrap(), "legacy-session");
        }
        let mut snapshot = Snapshot {
            cookies: vec![cookie.clone()],
            config: BTreeMap::new(),
            cookie_version: 24,
        };
        assert!(primary_cookie(&snapshot, "sessionKey").is_some());
        snapshot.cookies[0]
            .0
            .insert("has_expires".into(), Cell::Integer(1));
        snapshot.cookies[0]
            .0
            .insert("expires_utc".into(), Cell::Integer(1));
        assert!(primary_cookie(&snapshot, "sessionKey").is_none());
    }
    #[test]
    fn free_empty_usage_stays_unknown_and_malformed_or_paid_empty_usage_fails() {
        let mut account = Account {
            id: "fixture".into(),
            email: "fixture@example.test".into(),
            plan: Some("Free".into()),
            five_hour: None,
            seven_day: None,
            active: false,
        };
        set_usage(&mut account, &json!({"limits":[]})).unwrap();
        assert!(account.five_hour.is_none());
        assert!(account.seven_day.is_none());
        assert!(set_usage(&mut account, &json!({})).is_err());
        set_usage(
            &mut account,
            &json!({"limits":[{"kind":"session","percent":100}]}),
        )
        .unwrap();
        assert_eq!(account.five_hour.as_ref().unwrap().remaining_percent, 0);
        account.plan = Some("Pro".into());
        assert!(set_usage(&mut account, &json!({"limits":[]})).is_err());
    }
    #[test]
    fn usage_accepts_both_schemas_without_treating_model_limits_as_account_limits() {
        let legacy = json!({"five_hour":{"utilization":20,"resets_at":"2026-10-09T12:00:00Z"}});
        let expected = usage_window(&legacy, "five_hour", "session").unwrap();
        let current = json!({"limits":[
            {"kind":"weekly","percent":90,"scope":{"model":{"display_name":"Claude Sonnet"}}},
            {"kind":"session","percent":20,"resets_at":"2026-10-09T12:00:00Z"},
            {"group":"weekly","percent":"0","resets_at":1791547200}
        ]});
        assert_eq!(
            usage_window(&current, "five_hour", "session").unwrap(),
            expected
        );
        let weekly = usage_window(&current, "seven_day", "weekly").unwrap();
        assert_eq!(weekly.remaining_percent, 100);
        assert_eq!(weekly.reset_at, Some(1791547200));
        assert!(usage_window(
            &json!({"limits":[{"kind":"session"}]}),
            "five_hour",
            "session"
        )
        .is_none());
        assert!(usage_window(
            &json!({"limits":[{"kind":"weekly","percent":10,"scope":{"model":{}}}]}),
            "seven_day",
            "weekly"
        )
        .is_none());
    }
    #[test]
    fn access_time_updates_do_not_change_account_fingerprint() {
        let db = database("");
        let mut snapshot = Snapshot {
            cookies: cookies(&db).unwrap(),
            config: BTreeMap::new(),
            cookie_version: 24,
        };
        let original = fingerprint(&snapshot);
        snapshot.cookies[0]
            .0
            .insert("last_update_utc".into(), Cell::Integer(999));
        assert_eq!(fingerprint(&snapshot), original);
        snapshot.cookies[0]
            .0
            .insert("value".into(), Cell::Text("changed-session".into()));
        assert_ne!(fingerprint(&snapshot), original);
    }

    fn snapshot_dir(config: Value, org: Option<&str>) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("echobird-claude-snapshot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let db = database("");
        db.execute("UPDATE cookies SET value='fixture-live' WHERE host_key='.claude.ai' AND name='sessionKey'", [])
            .unwrap();
        if let Some(org) = org {
            db.execute(
                "INSERT INTO cookies(host_key,name,path,value,encrypted_value,has_expires,expires_utc,last_update_utc) VALUES('.claude.ai','lastActiveOrg','/',?1,X'',0,0,1)",
                [org],
            )
            .unwrap();
        }
        db.execute("VACUUM INTO ?1", [dir.join("Cookies").to_str().unwrap()])
            .unwrap();
        write(&dir.join("config.json"), &config).unwrap();
        dir
    }

    #[test]
    fn snapshots_save_the_signed_in_account_without_touching_the_client() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        let org = "22222222-2222-4222-8222-222222222222";
        let config = json!({
            "locale": "ja",
            "lastKnownAccountUuid": uuid,
            "oauth:tokenCacheV2": "sealed-fixture",
        });
        let dir = snapshot_dir(config.clone(), Some(org));
        let store_path = dir.join("accounts.json");
        let profile = json!({
            "account": {"uuid": uuid, "email": "fixture@example.test"},
            "organization": {"uuid": org, "organization_type": "claude_pro"}
        });
        let before = std::fs::read(dir.join("Cookies")).unwrap();
        let (snapshot, read_org) = read_signed_in(&dir, uuid).unwrap();
        assert_eq!(read_org, org);
        let account = store_snapshot(&store_path, uuid, org, snapshot, &profile, true).unwrap();
        assert_eq!(account.id, account_id(uuid, org));
        assert_eq!(account.plan.as_deref(), Some("Pro"));
        assert!(account.active);
        let store = load_store_at(&store_path).unwrap();
        let saved = &store[&account.id];
        assert_eq!(saved.organization_uuid, org);
        assert_eq!(
            saved.snapshot.config["oauth:tokenCacheV2"],
            json!("sealed-fixture")
        );
        assert_eq!(saved.snapshot.config["lastKnownAccountUuid"], json!(uuid));
        assert!(!saved.snapshot.config.contains_key("locale"));
        assert_eq!(
            session_values(&dir, &saved.snapshot).unwrap(),
            ("fixture-live".to_string(), org.to_string())
        );
        validate_saved_identity(saved, &account.id, org).unwrap();
        assert_eq!(std::fs::read(dir.join("Cookies")).unwrap(), before);
        assert_eq!(read::<Value>(&dir.join("config.json")).unwrap(), config);
        // Another account signed in meanwhile, or no web session: nothing saved.
        assert_eq!(
            read_signed_in(&dir, "33333333-3333-4333-8333-333333333333").err(),
            Some("accountError.noAccount".to_string())
        );
        std::fs::remove_dir_all(&dir).unwrap();
        let dir = snapshot_dir(config, None);
        assert_eq!(
            read_signed_in(&dir, uuid).err(),
            Some("accountError.noAccount".to_string())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_a_complete_sign_in_counts_as_signed_in() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        assert_eq!(
            signed_in_account(&json!({"lastKnownAccountUuid": uuid, "oauth:tokenCacheV2": "x"}))
                .as_deref(),
            Some(uuid)
        );
        assert!(signed_in_account(&json!({"lastKnownAccountUuid": uuid})).is_none());
        assert!(signed_in_account(
            &json!({"lastKnownAccountUuid": "old", "oauth:tokenCacheV2": "x"})
        )
        .is_none());
        assert!(signed_in_account(&json!({})).is_none());
    }

    #[test]
    fn the_newest_config_marks_the_data_folder_in_use() {
        let root =
            std::env::temp_dir().join(format!("echobird-claude-dirs-{}", uuid::Uuid::new_v4()));
        let [old, new, empty] = ["old", "new", "empty"].map(|name| root.join(name));
        for dir in [&old, &new, &empty] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let base = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        for (dir, offset) in [(&old, 0), (&new, 60)] {
            std::fs::File::create(dir.join("config.json"))
                .unwrap()
                .set_modified(base + Duration::from_secs(offset))
                .unwrap();
        }
        assert_eq!(
            newest_config_dir(vec![old.clone(), empty.clone(), new.clone()]),
            Some(new.clone())
        );
        assert_eq!(newest_config_dir(vec![empty.clone()]), None);
        // A newer config.json without a web session is a folder left behind.
        std::fs::create_dir(old.join("Network")).unwrap();
        std::fs::write(old.join("Network/Cookies"), "").unwrap();
        assert_eq!(
            usable_data_dir(vec![new.clone(), old.clone()]),
            Some(old.clone())
        );
        assert_eq!(usable_data_dir(vec![new.clone()]), Some(new.clone()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn switching_keeps_the_signed_in_accounts_newest_sign_in_only() {
        let snapshot = |token: &str| Snapshot {
            cookies: Vec::new(),
            config: BTreeMap::from([("oauth:tokenCacheV2".into(), json!(token))]),
            cookie_version: 24,
        };
        let saved = |uuid: &str, org: &str| Saved {
            account: Account {
                id: account_id(uuid, org),
                email: "fixture@example.test".into(),
                plan: None,
                five_hour: None,
                seven_day: None,
                active: false,
            },
            account_uuid: uuid.into(),
            organization_uuid: org.into(),
            snapshot: snapshot("old"),
        };
        let mut store = Store::from([
            ("live".to_string(), saved("a", "org")),
            ("other-org".to_string(), saved("a", "other")),
            ("target".to_string(), saved("b", "org")),
        ]);
        remember_live(
            &mut store,
            "target",
            Some(("a".into(), "org".into(), snapshot("new"))),
        );
        let token =
            |store: &Store, id: &str| store[id].snapshot.config["oauth:tokenCacheV2"].clone();
        assert_eq!(token(&store, "live"), json!("new"));
        assert_eq!(
            store["live"].snapshot.config["lastKnownAccountUuid"],
            json!("a")
        );
        assert_eq!(token(&store, "other-org"), json!("old"));
        assert_eq!(token(&store, "target"), json!("old"));
        remember_live(
            &mut store,
            "live",
            Some(("a".into(), "org".into(), snapshot("newer"))),
        );
        assert_eq!(token(&store, "live"), json!("new"));
        remember_live(&mut store, "target", None);
        assert_eq!(token(&store, "live"), json!("new"));
    }

    #[test]
    fn newer_session_cookie_names_count_as_signed_in() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        let org = "22222222-2222-4222-8222-222222222222";
        let config = json!({"lastKnownAccountUuid": uuid, "oauth:tokenCacheV2": "sealed"});
        for (session, organization) in [
            ("sessionKeyV3", "lastActiveOrg"),
            ("sessionKeyV2", "lastActiveOrg"),
            ("sessionKey", "authLastActiveOrg"),
        ] {
            let dir = snapshot_dir(config.clone(), Some(org));
            let db = cookie_db(&dir, true).unwrap();
            db.execute(
                "UPDATE cookies SET name=?1 WHERE host_key='.claude.ai' AND name='sessionKey'",
                [session],
            )
            .unwrap();
            db.execute(
                "UPDATE cookies SET name=?1 WHERE name='lastActiveOrg'",
                [organization],
            )
            .unwrap();
            drop(db);
            let (snapshot, read_org) = read_signed_in(&dir, uuid).unwrap();
            assert_eq!(read_org, org);
            assert!(fingerprint(&snapshot).is_some());
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }

    #[test]
    fn clearing_a_sign_in_keeps_everything_else_on_the_device() {
        let uuid = "11111111-1111-4111-8111-111111111111";
        let org = "22222222-2222-4222-8222-222222222222";
        let dir = snapshot_dir(
            json!({"locale": "ja", "lastKnownAccountUuid": uuid, "oauth:tokenCache": "a", "oauth:tokenCacheV2": "b"}),
            Some(org),
        );
        clear_sign_in(&dir).unwrap();
        let config = native_config(&dir).unwrap();
        assert_eq!(config, json!({"locale": "ja"}));
        assert!(signed_in_account(&config).is_none());
        let db = cookie_db(&dir, false).unwrap();
        let left: Vec<String> = db
            .prepare("SELECT host_key||'/'||name FROM cookies ORDER BY 1")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(left, [".claude.ai/theme", "example.test/sessionKey"]);
        drop(db);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod local_mode_tests {
    use super::*;

    #[test]
    fn repeated_local_starts_restore_original_auth_and_keep_other_settings() {
        let directory =
            std::env::temp_dir().join(format!("echobird-local-auth-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let backup = directory.join("saved-auth.json");
        let config = directory.join("config.json");
        write(&config, &json!({"lastKnownAccountUuid":"original", "oauth:tokenCacheV2":"original-token", "theme":"light"})).unwrap();
        capture_local_mode_at(&directory, &backup).unwrap();
        write(&config, &json!({"lastKnownAccountUuid":"virtual", "oauth:tokenCacheV2":"local-token", "theme":"dark"})).unwrap();
        capture_local_mode_at(&directory, &backup).unwrap();
        restore_local_mode_at(&backup).unwrap();
        let restored: Value = read(&config).unwrap();
        assert_eq!(restored["lastKnownAccountUuid"], "original");
        assert_eq!(restored["oauth:tokenCacheV2"], "original-token");
        assert_eq!(restored["theme"], "dark");
        assert!(!backup.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
