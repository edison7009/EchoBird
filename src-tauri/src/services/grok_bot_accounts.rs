//! Grok Bot 0.61 native account catalog (separate from Grok Build).
pub(crate) mod speech;
use super::cursor_auth::{
    cipher, claims, decrypt, encrypt, identity, read, write, Cipher, LoginFlow,
};
pub use super::cursor_auth::{Account, LoginStart};
use super::cursor_usage::{self, Usage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const ACCESS: &str = "cursor-access-token";
const REFRESH: &str = "cursor-refresh-token";
const PROFILE: &str = "cursor-account-profile";
const TEAM: &str = "cursor-selected-team-id";
const CATALOG: &str = "cursor-accounts";
type Row = BTreeMap<String, String>;
static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: LoginFlow = LoginFlow::new();

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    row: Row,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    email: Option<String>,
    // An unapplied browser login must not be replaced by the client's older session.
    applied: bool,
    #[serde(default)]
    usage: Option<Usage>,
}
type Store = BTreeMap<String, Saved>;

#[derive(Default, Serialize, Deserialize)]
struct Catalog {
    active: Option<String>,
    accounts: BTreeMap<String, Row>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn data_dir() -> Result<PathBuf, String> {
    Ok(dirs::config_dir()
        .ok_or("accountError.home")?
        .join("Grok Bot"))
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/grokbot-accounts.json"))
}
fn summary(id: &str, row: &Row, active: Option<&str>, cipher: &Cipher) -> Result<Account, String> {
    let token = decrypt(
        cipher,
        row.get(ACCESS).ok_or("accountError.invalidAccount")?,
    )?;
    if identity(&token)? != id
        || decrypt(
            cipher,
            row.get(REFRESH).ok_or("accountError.invalidAccount")?,
        )?
        .is_empty()
    {
        return Err("accountError.invalidAccount".into());
    }
    let profile: Value = row
        .get(PROFILE)
        .map(|p| {
            decrypt(cipher, p)
                .and_then(|s| serde_json::from_str(&s).map_err(|_| "accountError.format".into()))
        })
        .transpose()?
        .unwrap_or_default();
    let payload = claims(&token)?;
    let email = profile["email"]
        .as_str()
        .or_else(|| profile["name"].as_str())
        .or_else(|| payload["email"].as_str())
        .or_else(|| payload["sub"].as_str())
        .unwrap_or("Grok Bot account");
    Ok(Account {
        id: id.into(),
        email: email.into(),
        active: active == Some(id),
        usage: None,
    })
}
fn catalog(document: &Value) -> Result<Catalog, String> {
    if !document.is_object() {
        return Err("accountError.format".into());
    }
    match document.get(CATALOG) {
        Some(Value::String(value)) => {
            serde_json::from_str(value).map_err(|_| "accountError.format".into())
        }
        None if document.get(ACCESS).is_none() && document.get(REFRESH).is_none() => {
            Ok(Catalog::default())
        }
        _ => Err("accountError.format".into()),
    }
}
fn native(dir: &Path) -> Result<Value, String> {
    let path = dir.join("sand-secrets.json");
    if !path.exists() {
        return Ok(json!({}));
    }
    read(&path)
}
fn load_store(path: &Path, current: &Catalog) -> Result<Store, String> {
    if path.exists() {
        return read(path);
    }
    Ok(current
        .accounts
        .iter()
        .filter(|(_, row)| row.contains_key(ACCESS) && row.contains_key(REFRESH))
        .map(|(id, row)| {
            (
                id.clone(),
                Saved {
                    row: row.clone(),
                    email: None,
                    applied: true,
                    usage: None,
                },
            )
        })
        .collect())
}
fn selected_team(row: &Row, key: &Cipher) -> Result<Option<u64>, String> {
    row.get(TEAM)
        .map(|s| {
            decrypt(key, s).and_then(|s| s.parse::<u64>().map_err(|_| "accountError.format".into()))
        })
        .transpose()
}
fn sync_native(store: &mut Store, current: &Catalog, key: &Cipher) -> Result<(), String> {
    for (id, saved) in store.iter_mut().filter(|(_, saved)| saved.applied) {
        if let Some(row) = current
            .accounts
            .get(id)
            .filter(|row| row.contains_key(ACCESS) && row.contains_key(REFRESH))
        {
            if selected_team(&saved.row, key)? != selected_team(row, key)? {
                saved.usage = None;
            }
            saved.row = row.clone();
        }
    }
    Ok(())
}
fn apply(document: &mut Value, id: &str, saved: &Saved) -> Result<(), String> {
    let mut current = catalog(document)?;
    current.accounts.insert(id.into(), saved.row.clone());
    current.active = Some(id.into());
    document[CATALOG] =
        Value::String(serde_json::to_string(&current).map_err(|_| "accountError.format")?);
    // Legacy fallback credentials must never leak a previous account's team into this one.
    for field in [ACCESS, REFRESH, TEAM] {
        document
            .as_object_mut()
            .ok_or("accountError.format")?
            .remove(field);
    }
    Ok(())
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let current = catalog(&native(&dir)?)?;
    let path = store_path()?;
    let mut saved = load_store(&path, &current)?;
    if saved.is_empty() {
        return Ok(vec![]);
    }
    let key = cipher(&dir)?;
    sync_native(&mut saved, &current, &key)?;
    let result = saved
        .iter_mut()
        .map(|(id, s)| {
            let mut account = summary(id, &s.row, current.active.as_deref(), &key)?;
            s.email = Some(account.email.clone());
            account.usage = s.usage.clone();
            Ok(account)
        })
        .collect::<Result<Vec<_>, String>>()?;
    write(&path, &saved)?;
    Ok(result)
}

pub fn start_login() -> Result<LoginStart, String> {
    cipher(&data_dir()?)?;
    LOGIN.start(Some("sand"))
}
pub async fn refresh(id: &str) -> Result<Usage, String> {
    let (token, team) = {
        let _guard = ACCOUNT_LOCK.lock().await;
        let dir = data_dir()?;
        let key = cipher(&dir)?;
        let current = catalog(&native(&dir)?)?;
        let path = store_path()?;
        let mut store = load_store(&path, &current)?;
        sync_native(&mut store, &current, &key)?;
        let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
        entry.email = Some(summary(id, &entry.row, None, &key)?.email);
        let token = decrypt(
            &key,
            entry.row.get(ACCESS).ok_or("accountError.invalidAccount")?,
        )?;
        let team = selected_team(&entry.row, &key)?;
        write(&path, &store)?;
        (token, team)
    };
    let usage = cursor_usage::fetch(&token, team, true).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path()?;
    let mut store: Store = read(&path)?;
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    if selected_team(&entry.row, &cipher(&data_dir()?)?)? != team {
        return Err("accountError.cancelled".into());
    }
    entry.usage = Some(usage.clone());
    write(&path, &store)?;
    Ok(usage)
}
fn login_row(value: &Value, cipher: &Cipher) -> Result<(String, Row), String> {
    let access = value["accessToken"]
        .as_str()
        .ok_or("accountError.authResponse")?;
    let refresh = value["refreshToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    let id = identity(access)?;
    let payload = claims(access)?;
    let mut row = Row::from([
        (ACCESS.into(), encrypt(cipher, access)?),
        (REFRESH.into(), encrypt(cipher, refresh)?),
        (
            PROFILE.into(),
            encrypt(
                cipher,
                &json!({"email": value["email"].as_str().or_else(|| payload["email"].as_str())})
                    .to_string(),
            )?,
        ),
    ]);
    if let Some(team) = value["selectedTeamId"]
        .as_u64()
        .filter(|v| *v > 0 && *v <= 9_007_199_254_740_991)
    {
        row.insert(TEAM.into(), encrypt(cipher, &team.to_string())?);
    }
    Ok((id, row))
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some(value) = LOGIN.poll(id).await? else {
        return Ok(None);
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    LOGIN.complete(id, || {
        let dir = data_dir()?;
        let key = cipher(&dir)?;
        let (id, row) = login_row(&value, &key)?;
        let current = catalog(&native(&dir)?)?;
        let mut saved = load_store(&store_path()?, &current)?;
        let account = summary(&id, &row, None, &key)?;
        saved.insert(
            id,
            Saved {
                row,
                email: Some(account.email.clone()),
                applied: false,
                usage: None,
            },
        );
        write(&store_path()?, &saved)?;
        Ok(Some(account))
    })
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    LOGIN.cancel(id)
}
pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path()?;
    let mut saved: Store = read(&path)?;
    saved.remove(id);
    write(&path, &saved)
}
pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let key = cipher(&dir)?;
    let path = store_path()?;
    let mut saved: Store = read(&path)?;
    let selected = saved.get(id).ok_or("accountError.invalidAccount")?;
    summary(id, &selected.row, None, &key)?;
    super::process_manager::stop_desktop_for_account("grokbot").await?;
    // Re-read after exit, including any last token rotation made by the client.
    let mut document = native(&dir)?;
    let current = catalog(&document)?;
    sync_native(&mut saved, &current, &key)?;
    let selected = saved.get_mut(id).ok_or("accountError.invalidAccount")?;
    let account = summary(id, &selected.row, Some(id), &key)?;
    selected.email = Some(account.email.clone());
    apply(&mut document, id, selected)?;
    // Keep the unapplied snapshot recoverable if writing the native file fails.
    write(&path, &saved)?;
    write(&dir.join("sand-secrets.json"), &document)?;
    saved
        .get_mut(id)
        .ok_or("accountError.invalidAccount")?
        .applied = true;
    write(&path, &saved)?;
    Ok(account)
}
pub(crate) async fn import_native() -> Result<super::auto_import::native::Counts, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let current = catalog(&native(&dir)?)?;
    if current.accounts.is_empty() {
        return Ok(Default::default());
    }
    let key = cipher(&dir)?;
    let path = store_path()?;
    let mut rows: Store = if path.exists() {
        read(&path)?
    } else {
        Store::new()
    };
    let mut counts = super::auto_import::native::Counts::default();
    for (id, row) in current.accounts {
        counts.record((|| {
            let account = summary(&id, &row, None, &key)?;
            if rows.iter().any(|(existing_id, s)| {
                super::auto_import::native::same_account(
                    &id,
                    &account.email,
                    existing_id,
                    s.email.as_deref().unwrap_or_default(),
                )
            }) {
                return Ok(Some(false));
            }
            let mut next = rows.clone();
            next.insert(
                id,
                Saved {
                    row,
                    email: Some(account.email),
                    applied: false,
                    usage: None,
                },
            );
            write(&path, &next)?;
            rows = next;
            Ok(Some(true))
        })());
    }
    Ok(counts)
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes_gcm::KeyInit;
    use base64::{
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
        Engine,
    };
    use std::fs;

    #[test]
    fn native_catalog_switch_preserves_accounts_with_each_os_cipher() {
        for key in super::super::electron_storage::test_ciphers() {
            let mut document = json!({"unrelated":"preserved"});
            for subject in ["a", "b", "a"] {
                let (id, row) = login_row(&json!({"accessToken":token(subject),"refreshToken":"refresh","selectedTeamId":42}), &key).unwrap();
                let saved = Saved {
                    row,
                    email: None,
                    applied: true,
                    usage: None,
                };
                apply(&mut document, &id, &saved).unwrap();
                let current = catalog(&document).unwrap();
                assert!(
                    summary(&id, &current.accounts[&id], current.active.as_deref(), &key)
                        .unwrap()
                        .active
                );
                assert_eq!(
                    selected_team(&current.accounts[&id], &key).unwrap(),
                    Some(42)
                );
            }
            assert_eq!(catalog(&document).unwrap().accounts.len(), 2);
            assert_eq!(document["unrelated"], "preserved");
        }
    }

    fn key() -> Cipher {
        Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[7u8; 32]).unwrap(),
        ))
    }
    fn token(subject: &str) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(
                json!({"sub":subject,"email":format!("{subject}@example.test")}).to_string()
            )
        )
    }
    fn account(subject: &str, team: Option<u64>) -> (String, Saved) {
        let mut value =
            json!({"accessToken":token(subject),"refreshToken":format!("refresh-{subject}")});
        if let Some(team) = team {
            value["selectedTeamId"] = json!(team);
        }
        let (id, row) = login_row(&value, &key()).unwrap();
        (
            id,
            Saved {
                row,
                email: None,
                applied: false,
                usage: None,
            },
        )
    }

    #[test]
    fn native_encryption_and_summary_never_expose_credentials() {
        let (id, saved) = account("one", Some(12));
        let encrypted = &saved.row[ACCESS];
        assert!(STANDARD.decode(encrypted).unwrap().starts_with(b"v10"));
        assert_eq!(decrypt(&key(), encrypted).unwrap(), token("one"));
        assert_ne!(encrypt(&key(), &token("one")).unwrap(), *encrypted);
        let row = summary(&id, &saved.row, Some(&id), &key()).unwrap();
        assert!(row.active);
        assert_eq!(row.email, "one@example.test");
        let encoded = serde_json::to_string(&row).unwrap();
        assert!(!encoded.contains("refresh-one"));
        assert!(!encoded.contains(&token("one")));
        assert!(!serde_json::to_string(&saved)
            .unwrap()
            .contains("refresh-one"));
        assert!(summary("wrong-account", &saved.row, None, &key()).is_err());
        assert!(decrypt(&key(), "djEw").is_err());
        assert!(login_row(
            &json!({"accessToken":token("one"),"refreshToken":""}),
            &key()
        )
        .is_err());
        for team in [Value::Null, json!(0)] {
            let (_, row) = login_row(&json!({"accessToken":token("one"),"refreshToken":"refresh-one","selectedTeamId":team}), &key()).unwrap();
            assert!(!row.contains_key(TEAM));
        }
    }

    #[test]
    fn browser_profile_email_is_saved_when_jwt_omits_it() {
        let access = format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(json!({"sub":"profile-only"}).to_string())
        );
        let (id, row) = login_row(
            &json!({"accessToken":access,"refreshToken":"refresh","email":"profile@example.test"}),
            &key(),
        )
        .unwrap();
        assert_eq!(
            summary(&id, &row, None, &key()).unwrap().email,
            "profile@example.test"
        );
    }

    #[test]
    fn switch_a_b_a_preserves_other_secrets_and_isolates_team_selection() {
        let (a, saved_a) = account("one", Some(42));
        let (b, saved_b) = account("two", None);
        let mut document = json!({"cursor-machine-id":"machine-cipher", "local-exec-file-key":"file-cipher", "future-field":"keep"});
        apply(&mut document, &a, &saved_a).unwrap();
        document[TEAM] = json!("legacy-team");
        apply(&mut document, &b, &saved_b).unwrap();
        let current = catalog(&document).unwrap();
        assert_eq!(current.active.as_deref(), Some(b.as_str()));
        assert!(!current.accounts[&b].contains_key(TEAM));
        assert!(!document.as_object().unwrap().contains_key(TEAM));
        assert_eq!(current.accounts[&a], saved_a.row);
        apply(&mut document, &a, &saved_a).unwrap();
        let current = catalog(&document).unwrap();
        assert_eq!(current.active.as_deref(), Some(a.as_str()));
        assert_eq!(decrypt(&key(), &current.accounts[&a][TEAM]).unwrap(), "42");
        assert_eq!(current.accounts[&b], saved_b.row);
        assert_eq!(document["cursor-machine-id"], "machine-cipher");
        assert_eq!(document["local-exec-file-key"], "file-cipher");
        assert_eq!(document["future-field"], "keep");
    }

    #[test]
    fn token_rotation_sync_does_not_overwrite_an_unapplied_login() {
        let (id, saved) = account("one", None);
        let mut current = Catalog::default();
        let mut rotated = saved.row.clone();
        rotated.insert(REFRESH.into(), encrypt(&key(), "rotated-refresh").unwrap());
        current.accounts.insert(id.clone(), rotated.clone());
        let mut store = Store::from([(id.clone(), saved.clone())]);
        sync_native(&mut store, &current, &key()).unwrap();
        assert_eq!(store[&id].row, saved.row);
        store.get_mut(&id).unwrap().applied = true;
        store.get_mut(&id).unwrap().usage = Some(Usage {
            plan: Some("Pro".into()),
            ..Default::default()
        });
        sync_native(&mut store, &current, &key()).unwrap();
        assert_eq!(store[&id].row, rotated);
        assert_eq!(
            store[&id].usage.as_ref().unwrap().plan.as_deref(),
            Some("Pro")
        );
        current
            .accounts
            .get_mut(&id)
            .unwrap()
            .insert(TEAM.into(), encrypt(&key(), "7").unwrap());
        sync_native(&mut store, &current, &key()).unwrap();
        assert!(store[&id].usage.is_none());
        store.get_mut(&id).unwrap().usage = Some(Usage {
            plan: Some("Pro".into()),
            ..Default::default()
        });
        current
            .accounts
            .get_mut(&id)
            .unwrap()
            .insert(TEAM.into(), encrypt(&key(), "7").unwrap());
        sync_native(&mut store, &current, &key()).unwrap();
        assert!(store[&id].usage.is_some());
        let mut legacy = serde_json::to_value(&store[&id]).unwrap();
        legacy.as_object_mut().unwrap().remove("usage");
        assert!(serde_json::from_value::<Saved>(legacy)
            .unwrap()
            .usage
            .is_none());
    }

    #[test]
    fn corrupt_catalog_is_not_replaced_and_deleted_accounts_stay_deleted() {
        assert!(catalog(&json!({CATALOG:"not json"})).is_err());
        assert!(catalog(&json!({ACCESS:"old-client-format"})).is_err());
        let dir =
            std::env::temp_dir().join(format!("echobird-grokbot-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("store.json");
        let (id, saved) = account("one", None);
        let current = Catalog {
            active: Some(id.clone()),
            accounts: BTreeMap::from([(id.clone(), saved.row)]),
            ..Default::default()
        };
        assert!(load_store(&path, &current).unwrap().contains_key(&id));
        write(&path, &load_store(&path, &current).unwrap()).unwrap();
        write(&path, &Store::new()).unwrap();
        assert!(load_store(&path, &current).unwrap().is_empty());
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn registration_has_no_model_mapping_and_matches_install_index() {
        let paths: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/grokbot/paths.json")).unwrap();
        assert!(paths.no_model_config);
        assert!(paths.command.is_empty());
        assert!(paths.api_protocol.is_empty());
        assert!(!paths.paths.win32.as_ref().unwrap().is_empty());
        assert!(!paths.paths.darwin.as_ref().unwrap().is_empty());
        assert!(paths
            .paths
            .linux
            .as_ref()
            .unwrap()
            .iter()
            .any(|p| p == "/opt/Grok Bot/grok-bot"));
        let install: Value =
            serde_json::from_str(include_str!("../../../docs/api/tools/install/grokbot.json"))
                .unwrap();
        let index: Value =
            serde_json::from_str(include_str!("../../../docs/api/tools/install/index.json"))
                .unwrap();
        assert_eq!(install["id"], "grokbot");
        assert!(index["ids"].as_array().unwrap().contains(&json!("grokbot")));
    }

    #[test]
    #[ignore = "Read-only verification against the user's installed Grok Bot; never writes credentials"]
    fn installed_client_credentials_round_trip_read_only() {
        let dir = data_dir().unwrap();
        let key = cipher(&dir).unwrap();
        let current = catalog(&native(&dir).unwrap()).unwrap();
        assert!(!current.accounts.is_empty());
        for (id, row) in &current.accounts {
            let account = summary(id, row, current.active.as_deref(), &key).unwrap();
            assert!(!account.email.is_empty());
            let access = decrypt(&key, &row[ACCESS]).unwrap();
            assert!(decrypt(&key, &encrypt(&key, &access).unwrap()).unwrap() == access);
        }
        let paths: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/grokbot/paths.json")).unwrap();
        #[cfg(windows)]
        assert!(paths
            .paths
            .win32
            .unwrap()
            .iter()
            .any(|p| super::super::tool_manager::expand_path(p).is_file()));
        #[cfg(not(windows))]
        assert!(paths.no_model_config);
    }

    #[tokio::test]
    #[ignore = "Queries the installed Grok Bot account's own usage API; no local or remote writes"]
    async fn installed_grok_bot_usage_read_only() {
        let dir = data_dir().unwrap();
        let key = cipher(&dir).unwrap();
        let current = catalog(&native(&dir).unwrap()).unwrap();
        let row = &current.accounts[current.active.as_ref().unwrap()];
        let token = decrypt(&key, &row[ACCESS]).unwrap();
        let team = row
            .get(TEAM)
            .map(|s| decrypt(&key, s).unwrap().parse().unwrap());
        let usage = cursor_usage::fetch(&token, team, true).await.unwrap();
        assert!(usage.plan.is_some());
    }
}
