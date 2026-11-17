//! Cursor 3.22 account state in the default user-data directory.
//! Only authentication keys and team selection are changed, in one SQLite transaction.
pub(crate) mod cli;
pub(crate) mod speech;
use super::cursor_auth::{
    cipher, claims, decrypt, encrypt, identity, read, write, Cipher, LoginFlow,
};
pub use super::cursor_auth::{Account, LoginStart};
use super::cursor_usage::{self, Usage};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

const ACCESS: &str = "cursorAuth/accessToken";
const REFRESH: &str = "cursorAuth/refreshToken";
const EMAIL: &str = "cursorAuth/cachedEmail";
const REACTIVE: &str = "src.vs.platform.reactivestorage.browser.reactiveStorageServiceImpl.persistentStorage.applicationUser";
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: LoginFlow = LoginFlow::new();

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credentials {
    access_token: String,
    refresh_token: String,
    email: Option<String>,
    team_id: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    // EchoBird never persists Cursor's plaintext SQLite tokens in its own account store.
    session: String,
    applied: bool,
    #[serde(default)]
    cli_applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}
type Store = BTreeMap<String, Saved>;

fn data_dir() -> Result<PathBuf, String> {
    Ok(dirs::config_dir()
        .ok_or("accountError.home")?
        .join("Cursor"))
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/cursor-accounts.json"))
}
fn database(dir: &Path, writable: bool) -> Result<Connection, String> {
    let path = dir.join("User/globalStorage/state.vscdb");
    if !path.is_file() {
        return Err("accountError.initializeClient".into());
    }
    let flags = if writable {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let db = Connection::open_with_flags(path, flags).map_err(|_| "accountError.read")?;
    db.busy_timeout(Duration::from_secs(5))
        .map_err(|_| "accountError.read")?;
    Ok(db)
}
fn get(db: &Connection, key: &str) -> Result<Option<String>, String> {
    db.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
    .map_err(|_| "accountError.read".into())
}
fn reactive(db: &Connection) -> Result<Value, String> {
    let value = match get(db, REACTIVE)? {
        Some(raw) => serde_json::from_str(&raw).map_err(|_| "accountError.format")?,
        None => json!({}),
    };
    if !value.is_object() || value.get("aiSettings").is_some_and(|v| !v.is_object()) {
        return Err("accountError.format".into());
    }
    Ok(value)
}
fn native(db: &Connection) -> Result<Option<Credentials>, String> {
    let Some(access) = get(db, ACCESS)?.filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let refresh = get(db, REFRESH)?
        .filter(|s| !s.is_empty())
        .ok_or("accountError.invalidAccount")?;
    identity(&access)?;
    let state = reactive(db)?;
    Ok(Some(Credentials {
        access_token: access,
        refresh_token: refresh,
        email: get(db, EMAIL)?.filter(|s| !s.is_empty()),
        team_id: team_id(&state["aiSettings"]["teamId"]),
    }))
}
fn team_id(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
}
fn login_credentials(value: &Value) -> Result<Credentials, String> {
    let access = value["accessToken"]
        .as_str()
        .ok_or("accountError.authResponse")?;
    let refresh = value["refreshToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    identity(access)?;
    Ok(Credentials {
        access_token: access.into(),
        refresh_token: refresh.into(),
        email: value["email"]
            .as_str()
            .map(String::from)
            .or_else(|| claims(access).ok()?["email"].as_str().map(String::from)),
        team_id: team_id(&value["selectedTeamId"]),
    })
}
fn summary(credentials: &Credentials, active: Option<&str>) -> Result<Account, String> {
    let id = identity(&credentials.access_token)?;
    if credentials.refresh_token.is_empty() {
        return Err("accountError.invalidAccount".into());
    }
    let payload = claims(&credentials.access_token)?;
    let email = credentials
        .email
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| payload["email"].as_str())
        .or_else(|| payload["sub"].as_str())
        .ok_or("accountError.invalidAccount")?;
    Ok(Account {
        active: active == Some(id.as_str()),
        id,
        email: email.into(),
        usage: None,
    })
}
fn saved(credentials: &Credentials, key: &Cipher, applied: bool) -> Result<Saved, String> {
    Ok(Saved {
        session: encrypt(
            key,
            &serde_json::to_string(credentials).map_err(|_| "accountError.format")?,
        )?,
        applied,
        cli_applied: false,
        email: Some(summary(credentials, None)?.email),
        usage: None,
    })
}
fn credentials(id: &str, saved: &Saved, key: &Cipher) -> Result<Credentials, String> {
    let raw = if saved.session.starts_with("enc:v1:") {
        super::model_manager::decrypt_key_for_use(&saved.session)
    } else {
        decrypt(key, &saved.session)?
    };
    let value: Credentials = serde_json::from_str(&raw).map_err(|_| "accountError.format")?;
    if summary(&value, None)?.id != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(value)
}
fn load_store(path: &Path, current: Option<&Credentials>, key: &Cipher) -> Result<Store, String> {
    if path.exists() {
        return read(path);
    }
    let mut store = Store::new();
    if let Some(current) = current {
        store.insert(identity(&current.access_token)?, saved(current, key, true)?);
    }
    Ok(store)
}
fn sync_native(
    store: &mut Store,
    current: Option<&Credentials>,
    key: &Cipher,
) -> Result<(), String> {
    if let Some(current) = current {
        if let Some(entry) = store
            .get_mut(&identity(&current.access_token)?)
            .filter(|s| s.applied)
        {
            if credentials(&identity(&current.access_token)?, entry, key)?.team_id
                != current.team_id
            {
                entry.usage = None;
            }
            let replacement = saved(current, key, true)?;
            entry.session = replacement.session;
            entry.email = replacement.email;
            entry.cli_applied = false;
        }
    }
    Ok(())
}
fn put(db: &Connection, key: &str, value: &str) -> Result<(), String> {
    db.execute(
        "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
        [key, value],
    )
    .map(|_| ())
    .map_err(|_| "accountError.write".into())
}
fn apply(db: &mut Connection, credentials: &Credentials) -> Result<(), String> {
    summary(credentials, None)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "accountError.write")?;
    let mut state = reactive(&tx)?;
    // Cached membership/profile belongs to the previous login. Cursor fetches it on startup.
    for field in [
        "membershipType",
        "subscriptionStatus",
        "isEnterprise",
        "hasBedrockIamRole",
    ] {
        state
            .as_object_mut()
            .ok_or("accountError.format")?
            .remove(field);
    }
    if state.get("aiSettings").is_none() {
        state["aiSettings"] = json!({});
    }
    let settings = state["aiSettings"]
        .as_object_mut()
        .ok_or("accountError.format")?;
    settings.remove("teamIds");
    settings.remove("teamId");
    if let Some(team) = credentials.team_id {
        settings.insert("teamId".into(), json!(team));
    }
    put(&tx, REACTIVE, &state.to_string())?;
    for key in [
        EMAIL,
        "cursorAuth/cachedSignUpType",
        "cursorAuth/cachedScopedProfile",
        "cursorAuth/cachedTeam",
        "cursorAuth/stripeCustomerId",
        "cursorAuth/stripeMembershipType",
        "cursorAuth/stripeMembershipAuthId",
        "cursorAuth/stripeSubscriptionStatus",
        "cursorAuth/teamId",
        "autorun.cachedAdminSettings",
    ] {
        tx.execute("DELETE FROM ItemTable WHERE key = ?1", [key])
            .map_err(|_| "accountError.write")?;
    }
    put(&tx, ACCESS, &credentials.access_token)?;
    put(&tx, REFRESH, &credentials.refresh_token)?;
    if let Some(email) = &credentials.email {
        put(&tx, EMAIL, email)?;
    }
    tx.commit().map_err(|_| "accountError.write".into())
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let path = store_path()?;
    if !dir.join("User/globalStorage/state.vscdb").exists() && !path.exists() {
        return Ok(vec![]);
    }
    let db = database(&dir, false)?;
    let current = native(&db)?;
    let key = cipher(&dir)?;
    let mut store = load_store(&path, current.as_ref(), &key)?;
    sync_native(&mut store, current.as_ref(), &key)?;
    let active = current
        .as_ref()
        .map(|c| identity(&c.access_token))
        .transpose()?;
    let result = store
        .iter_mut()
        .map(|(id, saved)| {
            let mut account = summary(&credentials(id, saved, &key)?, active.as_deref())?;
            saved.email = Some(account.email.clone());
            account.usage = saved.usage.clone();
            Ok(account)
        })
        .collect::<Result<Vec<_>, String>>()?;
    write(&path, &store)?;
    Ok(result)
}
pub fn start_login() -> Result<LoginStart, String> {
    let dir = data_dir()?;
    database(&dir, false)?;
    cipher(&dir)?;
    LOGIN.start(None)
}
pub async fn refresh(id: &str) -> Result<Usage, String> {
    // Only snapshots in EchoBird are updated; the editor's selected account stays intact.
    let (token, team) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        let dir = data_dir()?;
        let key = cipher(&dir)?;
        let current = native(&database(&dir, false)?)?;
        let path = store_path()?;
        let mut store = load_store(&path, current.as_ref(), &key)?;
        sync_native(&mut store, current.as_ref(), &key)?;
        let login = credentials(
            id,
            store.get(id).ok_or("accountError.invalidAccount")?,
            &key,
        )?;
        store
            .get_mut(id)
            .ok_or("accountError.invalidAccount")?
            .email = Some(summary(&login, None)?.email);
        write(&path, &store)?;
        (login.access_token, login.team_id)
    };
    let usage = cursor_usage::fetch(&token, team, false).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path()?;
    let mut store: Store = read(&path)?;
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    if credentials(id, entry, &cipher(&data_dir()?)?)?.team_id != team {
        return Err("accountError.cancelled".into());
    }
    entry.usage = Some(usage.clone());
    write(&path, &store)?;
    Ok(usage)
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some(value) = LOGIN.poll(id).await? else {
        return Ok(None);
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    LOGIN.complete(id, || {
        let dir = data_dir()?;
        let key = cipher(&dir)?;
        let current = native(&database(&dir, false)?)?;
        let path = store_path()?;
        let mut store = load_store(&path, current.as_ref(), &key)?;
        let login = login_credentials(&value)?;
        let account = summary(&login, None)?;
        store.insert(account.id.clone(), saved(&login, &key, false)?);
        write(&path, &store)?;
        Ok(Some(account))
    })
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    LOGIN.cancel(id)
}
pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path()?;
    let mut store: Store = read(&path)?;
    store.remove(id);
    write(&path, &store)
}
pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let key = cipher(&dir)?;
    let path = store_path()?;
    let mut store: Store = read(&path)?;
    credentials(
        id,
        store.get(id).ok_or("accountError.invalidAccount")?,
        &key,
    )?;
    super::process_manager::stop_desktop_for_account("cursor").await?;
    let mut db = database(&dir, true)?;
    let current = native(&db)?;
    sync_native(&mut store, current.as_ref(), &key)?;
    let selected = credentials(
        id,
        store.get(id).ok_or("accountError.invalidAccount")?,
        &key,
    )?;
    store
        .get_mut(id)
        .ok_or("accountError.invalidAccount")?
        .email = Some(summary(&selected, None)?.email);
    write(&path, &store)?;
    apply(&mut db, &selected)?;
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    entry.applied = true;
    entry.cli_applied = false;
    write(&path, &store)?;
    summary(&selected, Some(id))
}
pub(crate) async fn import_native(cli_only: bool) -> Result<Option<bool>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let current = if cli_only {
        cli::read_native()?
    } else {
        let dir = data_dir()?;
        if !dir.join("User/globalStorage/state.vscdb").is_file() {
            return Ok(None);
        }
        native(&database(&dir, false)?)?
    };
    let Some(current) = current else {
        return Ok(None);
    };
    let account = summary(&current, None)?;
    let path = store_path()?;
    let mut rows: Store = if path.exists() {
        read(&path)?
    } else {
        Store::new()
    };
    if rows.iter().any(|(id, s)| {
        super::auto_import::native::same_account(
            &account.id,
            &account.email,
            id,
            s.email.as_deref().unwrap_or_default(),
        )
    }) {
        return Ok(Some(false));
    }
    let encrypted = super::secret_store::encrypt(
        &serde_json::to_string(&current).map_err(|_| "accountError.format")?,
    )?;
    rows.insert(
        account.id,
        Saved {
            session: encrypted,
            applied: false,
            cli_applied: false,
            email: Some(account.email),
            usage: None,
        },
    );
    write(&path, &rows)?;
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
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    fn login(sub: &str, team: Option<u64>) -> Credentials {
        login_credentials(&json!({"accessToken":format!("header.{}.signature", URL_SAFE_NO_PAD.encode(json!({"sub":sub,"email":format!("{sub}@example.test")}).to_string())), "refreshToken":format!("refresh-{sub}"), "selectedTeamId":team})).unwrap()
    }
    fn db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB)",
        )
        .unwrap();
        db
    }
    #[test]
    fn login_uses_profile_email_when_token_has_only_a_subject() {
        let token = format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(json!({"sub":"user-without-email"}).to_string())
        );
        let login = login_credentials(
            &json!({"accessToken":token,"refreshToken":"refresh","email":"profile@example.test"}),
        )
        .unwrap();
        assert_eq!(summary(&login, None).unwrap().email, "profile@example.test");
    }

    #[tokio::test]
    #[ignore = "Queries the installed Cursor account's own usage API; no local or remote writes"]
    async fn installed_cursor_usage_read_only() {
        let login = native(&database(&data_dir().unwrap(), false).unwrap())
            .unwrap()
            .unwrap();
        let usage = cursor_usage::fetch(&login.access_token, login.team_id, false)
            .await
            .unwrap();
        assert!(usage.plan.is_some());
    }

    #[tokio::test]
    #[ignore = "Reads installed Cursor and its own profile API; mutations occur only in an in-memory database"]
    async fn installed_cursor_credentials_round_trip_read_only() {
        let dir = data_dir().unwrap();
        let native_db = database(&dir, false).unwrap();
        native_db.execute_batch("BEGIN").unwrap();
        let current = native(&native_db)
            .unwrap()
            .expect("Cursor must be signed in");
        let key = cipher(&dir).unwrap();
        let id = identity(&current.access_token).unwrap();
        let encrypted = saved(&current, &key, true).unwrap();
        let restored = credentials(&id, &encrypted, &key).unwrap();
        // Do not use assertions that print credentials or identity when they fail.
        assert!(restored.access_token == current.access_token);
        assert!(restored.refresh_token == current.refresh_token);
        let rows: Vec<(String, rusqlite::types::Value)> = native_db
            .prepare("SELECT key,value FROM ItemTable")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        drop(native_db);
        let profile_email = super::super::cursor_auth::account_email(&current.access_token)
            .await
            .unwrap();
        assert!(current
            .email
            .as_deref()
            .is_some_and(|email| email.eq_ignore_ascii_case(&profile_email)));

        let mut copy = db();
        for (key, value) in &rows {
            copy.execute(
                "INSERT INTO ItemTable (key,value) VALUES (?1,?2)",
                rusqlite::params![key, value],
            )
            .unwrap();
        }
        let original_settings = reactive(&copy).unwrap();
        apply(&mut copy, &login("synthetic-other-account", Some(789))).unwrap();
        apply(&mut copy, &current).unwrap();
        let round_trip = native(&copy).unwrap().unwrap();
        assert!(round_trip.access_token == current.access_token);
        assert!(round_trip.refresh_token == current.refresh_token);
        assert!(round_trip.team_id == current.team_id);
        let settings = reactive(&copy).unwrap();
        assert!(
            settings["aiSettings"]["modelConfig"] == original_settings["aiSettings"]["modelConfig"]
        );
        for (key, value) in &rows {
            if key.starts_with("cursorAuth/")
                || key == REACTIVE
                || key == "autorun.cachedAdminSettings"
            {
                continue;
            }
            let after: rusqlite::types::Value = copy
                .query_row("SELECT value FROM ItemTable WHERE key=?1", [key], |r| {
                    r.get(0)
                })
                .unwrap();
            assert!(
                &after == value,
                "Unrelated native state changed in the in-memory copy"
            );
        }
    }
    #[test]
    fn switches_a_b_a_without_changing_projects_models_or_machine_identity() {
        let mut db = db();
        let a = login("a", Some(42));
        let b = login("b", None);
        put(&db, "telemetry.machineId", "original-machine").unwrap();
        put(&db, "history.recentlyOpenedPathsList", "projects").unwrap();
        put(&db, "cursorAuth/openAIKey", "byok").unwrap();
        put(&db, REACTIVE, &json!({"aiSettings":{"modelConfig":{"composer":"selected-model"},"teamIds":[42]},"otherSetting":true,"membershipType":"pro"}).to_string()).unwrap();
        apply(&mut db, &a).unwrap();
        put(&db, "cursorAuth/cachedTeam", "old-team").unwrap();
        put(&db, "cursorAuth/stripeMembershipType", "pro").unwrap();
        apply(&mut db, &b).unwrap();
        assert_eq!(
            native(&db).unwrap().unwrap().email.as_deref(),
            Some("b@example.test")
        );
        assert!(native(&db).unwrap().unwrap().team_id.is_none());
        assert!(get(&db, "cursorAuth/cachedTeam").unwrap().is_none());
        assert!(get(&db, "cursorAuth/stripeMembershipType")
            .unwrap()
            .is_none());
        apply(&mut db, &a).unwrap();
        assert_eq!(native(&db).unwrap().unwrap().team_id, Some(42));
        assert_eq!(
            get(&db, "telemetry.machineId").unwrap().as_deref(),
            Some("original-machine")
        );
        assert_eq!(
            get(&db, "history.recentlyOpenedPathsList")
                .unwrap()
                .as_deref(),
            Some("projects")
        );
        assert_eq!(
            get(&db, "cursorAuth/openAIKey").unwrap().as_deref(),
            Some("byok")
        );
        let state = reactive(&db).unwrap();
        assert_eq!(
            state["aiSettings"]["modelConfig"]["composer"],
            "selected-model"
        );
        assert_eq!(state["otherSetting"], true);
        assert!(state["aiSettings"].get("teamIds").is_none());
    }
    #[test]
    fn account_switch_keeps_local_composer_index_messages_and_checkpoints() {
        let mut db = db();
        db.execute_batch("CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value BLOB)")
            .unwrap();
        let index = json!({"allComposers":[{"composerId":"a-chat","workspaceIdentifier":{"id":"workspace"}},{"composerId":"b-chat","workspaceIdentifier":{"id":"workspace"}}]}).to_string();
        put(&db, "composer.composerHeaders", &index).unwrap();
        for (key, value) in [
            ("composerData:a-chat", br#"{"composerId":"a-chat","fullConversationHeadersOnly":[{"bubbleId":"message"}]}"#.as_slice()),
            ("composerData:b-chat", br#"{"composerId":"b-chat","conversation":[{"text":"original answer"}]}"#.as_slice()),
            ("bubbleId:a-chat:message", br#"{"text":"original tool result","type":2}"#.as_slice()),
            ("checkpointId:a-chat:checkpoint", b"\x00\x01original-checkpoint".as_slice()),
        ] {
            db.execute("INSERT INTO cursorDiskKV VALUES (?1,?2)", rusqlite::params![key,value]).unwrap();
        }
        let records = |db: &Connection| {
            // All values remain BLOBs, including native binary checkpoint data.
            db.prepare("SELECT key,value FROM cursorDiskKV ORDER BY key")
                .unwrap()
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        let before = records(&db);
        for user in ["a", "b", "b", "a"] {
            apply(&mut db, &login(user, None)).unwrap();
            assert_eq!(
                native(&db).unwrap().unwrap().email,
                Some(format!("{user}@example.test"))
            );
            assert_eq!(
                get(&db, "composer.composerHeaders").unwrap().as_deref(),
                Some(index.as_str())
            );
            assert_eq!(records(&db), before);
        }
        db.execute_batch("CREATE TRIGGER fail_login BEFORE INSERT ON ItemTable WHEN NEW.key='cursorAuth/refreshToken' BEGIN SELECT RAISE(ABORT,'fixture failure'); END;").unwrap();
        assert!(apply(&mut db, &login("b", None)).is_err());
        assert_eq!(
            native(&db).unwrap().unwrap().email.as_deref(),
            Some("a@example.test")
        );
        assert_eq!(
            get(&db, "composer.composerHeaders").unwrap().as_deref(),
            Some(index.as_str())
        );
        assert_eq!(records(&db), before);
    }
    #[test]
    fn transaction_rolls_back_every_change_on_write_failure_or_invalid_state() {
        let mut db = db();
        let a = login("a", Some(12));
        apply(&mut db, &a).unwrap();
        let before = get(&db, REACTIVE).unwrap();
        db.execute_batch("CREATE TRIGGER fail_refresh BEFORE INSERT ON ItemTable WHEN NEW.key = 'cursorAuth/refreshToken' BEGIN SELECT RAISE(ABORT, 'test failure'); END;").unwrap();
        assert!(apply(&mut db, &login("b", None)).is_err());
        assert_eq!(get(&db, REACTIVE).unwrap(), before);
        assert_eq!(get(&db, ACCESS).unwrap().unwrap(), a.access_token);
        put(&db, REACTIVE, "invalid-json").unwrap();
        assert!(apply(&mut db, &login("b", None)).is_err());
        assert_eq!(get(&db, ACCESS).unwrap().unwrap(), a.access_token);
    }
    #[test]
    fn encrypted_store_sync_preserves_fresh_logins_and_deletions() {
        for key in super::super::electron_storage::test_ciphers() {
            verify_store_sync(key);
        }
    }
    fn verify_store_sync(key: Cipher) {
        let mut login = login("a", None);
        let id = identity(&login.access_token).unwrap();
        let mut store = Store::from([(id.clone(), saved(&login, &key, false).unwrap())]);
        let encoded = serde_json::to_string(&store).unwrap();
        assert!(!encoded.contains("refresh-a"));
        assert!(!encoded.contains(&login.access_token));
        let summary = serde_json::to_string(&summary(&login, Some(&id)).unwrap()).unwrap();
        assert!(!summary.contains("refreshToken"));
        login.refresh_token = "rotated".into();
        sync_native(&mut store, Some(&login), &key).unwrap();
        assert_eq!(
            credentials(&id, &store[&id], &key).unwrap().refresh_token,
            "refresh-a"
        );
        store.get_mut(&id).unwrap().applied = true;
        store.get_mut(&id).unwrap().usage = Some(Usage {
            plan: Some("Pro".into()),
            ..Default::default()
        });
        sync_native(&mut store, Some(&login), &key).unwrap();
        assert_eq!(
            store[&id].usage.as_ref().unwrap().plan.as_deref(),
            Some("Pro")
        );
        assert_eq!(
            credentials(&id, &store[&id], &key).unwrap().refresh_token,
            "rotated"
        );
        assert!(credentials("wrong", &store[&id], &key).is_err());
        login.team_id = Some(7);
        sync_native(&mut store, Some(&login), &key).unwrap();
        assert!(store[&id].usage.is_none());
        let mut legacy = serde_json::to_value(&store[&id]).unwrap();
        legacy.as_object_mut().unwrap().remove("usage");
        assert!(serde_json::from_value::<Saved>(legacy)
            .unwrap()
            .usage
            .is_none());
        let path =
            std::env::temp_dir().join(format!("echobird-cursor-{}.json", uuid::Uuid::new_v4()));
        assert_eq!(load_store(&path, Some(&login), &key).unwrap().len(), 1);
        write(&path, &Store::new()).unwrap();
        assert!(load_store(&path, Some(&login), &key).unwrap().is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
