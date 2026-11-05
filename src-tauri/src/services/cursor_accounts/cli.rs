//! Cursor CLI shares account identities, not the editor's native login storage.
use super::*;
use crate::services::native_account_files::{self as files, Transaction};
use serde_json::json;

pub(super) fn config_file() -> Result<PathBuf, String> {
    let root = std::env::var_os("CURSOR_CONFIG_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|p| PathBuf::from(p).join("cursor")))
        .or_else(|| dirs::home_dir().map(|p| p.join(".cursor")))
        .ok_or("accountError.home")?;
    Ok(root.join("cli-config.json"))
}

#[cfg(not(target_os = "macos"))]
fn auth_file() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let root = dirs::config_dir().map(|p| p.join("Cursor"));
    #[cfg(not(windows))]
    let root = dirs::config_dir().map(|p| p.join("cursor"));
    Ok(root.ok_or("accountError.home")?.join("auth.json"))
}

fn native_credentials(config: &Path) -> Result<Option<Credentials>, String> {
    #[cfg(not(target_os = "macos"))]
    let auth = files::object(&auth_file()?)?;
    #[cfg(target_os = "macos")]
    let auth = json!({
        "accessToken": keychain_read("cursor-access-token")?,
        "refreshToken": keychain_read("cursor-refresh-token")?,
    });
    native_from_values(&auth, &files::object(config)?)
}

fn native_from_values(auth: &Value, config: &Value) -> Result<Option<Credentials>, String> {
    if auth.get("accessToken").is_none_or(Value::is_null) {
        return Ok(None);
    }
    let mut login = login_credentials(auth)?;
    login.email = config["authInfo"]["email"].as_str().map(String::from);
    login.team_id = team_id(&config["authInfo"]["activeTeamId"]);
    Ok(Some(login))
}

pub(super) fn desktop_key() -> Result<Option<Cipher>, String> {
    let dir = data_dir()?;
    if !dir.exists() {
        return Ok(None);
    }
    // CLI-only installations must not initialize an editor or create its database.
    #[cfg(windows)]
    if !dir.join("Local State").exists() {
        return Ok(None);
    }
    cipher(&dir).map(Some)
}

pub(super) fn snapshot(login: &Credentials, key: Option<&Cipher>) -> Result<Saved, String> {
    if let Some(key) = key {
        return saved(login, key, false);
    }
    let raw = serde_json::to_string(login).map_err(|_| "accountError.format")?;
    let session = super::super::model_manager::encrypt_key_for_storage(&raw);
    // The shared encryption helper can fall back to plaintext; account snapshots cannot.
    if !session.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(Saved {
        session,
        applied: false,
        cli_applied: false,
        email: Some(summary(login, None)?.email),
        usage: None,
    })
}

pub(super) fn restored(
    id: &str,
    entry: &Saved,
    key: Option<&Cipher>,
) -> Result<Credentials, String> {
    if entry.session.starts_with("enc:v1:") {
        let raw = super::super::model_manager::decrypt_key_for_use(&entry.session);
        let login: Credentials = serde_json::from_str(&raw).map_err(|_| "accountError.format")?;
        if summary(&login, None)?.id != id {
            return Err("accountError.invalidAccount".into());
        }
        Ok(login)
    } else {
        credentials(id, entry, key.ok_or("accountError.keychain")?)
    }
}

/// Keeps the CLI's newer login and says whether the CLI holds account `id`.
pub(super) fn adopt(store: &mut Store, id: &str, key: Option<&Cipher>) -> Result<bool, String> {
    let current = native_credentials(&config_file()?)?;
    let active = sync_current(store, current.as_ref(), key, false)?;
    Ok(active.as_deref() == Some(id) && store.get(id).is_some_and(|entry| entry.cli_applied))
}

/// Gives the CLI the token EchoBird rotated, so its own copy stays valid.
pub(super) fn store_rotation(login: &Credentials) -> Result<(), String> {
    let config = config_file()?;
    let _guard = files::DirectoryLock::acquire(&config)?;
    #[cfg(not(target_os = "macos"))]
    {
        let path = auth_file()?;
        let mut transaction = Transaction::new();
        transaction.replace_private_file(
            &path,
            &files::json(&authenticated(files::object(&path)?, login)?)?,
        )?;
        transaction.commit();
    }
    #[cfg(target_os = "macos")]
    keychain_apply(login)?;
    Ok(())
}

fn store() -> Result<Store, String> {
    let path = store_path()?;
    if path.exists() {
        read(&path)
    } else {
        Ok(Store::new())
    }
}

fn sync_current(
    store: &mut Store,
    current: Option<&Credentials>,
    key: Option<&Cipher>,
    initialize: bool,
) -> Result<Option<String>, String> {
    let active = current.map(|c| identity(&c.access_token)).transpose()?;
    if let (Some(login), Some(id)) = (current, &active) {
        if let Some(entry) = store.get_mut(id) {
            if entry.cli_applied {
                if restored(id, entry, key)?.team_id != login.team_id {
                    entry.usage = None;
                }
                let replacement = snapshot(login, key)?;
                entry.session = replacement.session;
                entry.email = replacement.email;
                entry.applied = false;
            }
        } else if initialize {
            let mut entry = snapshot(login, key)?;
            entry.cli_applied = true;
            store.insert(id.clone(), entry);
        }
    }
    Ok(active)
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let current = if super::super::tool_manager::any_tool_installed(&["cursorcli"]) {
        native_credentials(&config_file()?)?
    } else {
        None
    };
    let key = desktop_key()?;
    let initialize = !store_path()?.exists();
    let mut store = store()?;
    let active = sync_current(&mut store, current.as_ref(), key.as_ref(), initialize)?;
    let accounts = store
        .iter()
        .map(|(id, entry)| {
            let mut account = summary(&restored(id, entry, key.as_ref())?, active.as_deref())?;
            account.usage = entry.usage.clone();
            Ok(account)
        })
        .collect::<Result<Vec<_>, String>>()?;
    write(&store_path()?, &store)?;
    Ok(accounts)
}

pub fn start_login() -> Result<LoginStart, String> {
    LOGIN.start(Some("cli"))
}

pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let Some(value) = LOGIN.poll(id).await? else {
        return Ok(None);
    };
    let _guard = ACCOUNT_LOCK.lock().await;
    LOGIN.complete(id, || {
        let login = login_credentials(&value)?;
        let account = summary(&login, None)?;
        let key = desktop_key()?;
        let mut store = store()?;
        store.insert(account.id.clone(), snapshot(&login, key.as_ref())?);
        write(&store_path()?, &store)?;
        Ok(Some(account))
    })
}

pub async fn refresh(id: &str) -> Result<Usage, String> {
    let login = {
        let _guard = ACCOUNT_LOCK.lock().await;
        let key = desktop_key()?;
        let mut store = store()?;
        let current = native_credentials(&config_file()?)?;
        sync_current(&mut store, current.as_ref(), key.as_ref(), false)?;
        write(&store_path()?, &store)?;
        restored(
            id,
            store.get(id).ok_or("accountError.invalidAccount")?,
            key.as_ref(),
        )?
    };
    let usage = cursor_usage::fetch(&login.access_token, login.team_id, false).await?;
    let _guard = ACCOUNT_LOCK.lock().await;
    let key = desktop_key()?;
    let mut store = store()?;
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    if restored(id, entry, key.as_ref())?.team_id != login.team_id {
        return Err("accountError.cancelled".into());
    }
    entry.usage = Some(usage.clone());
    write(&store_path()?, &store)?;
    Ok(usage)
}

fn configured(mut config: Value, login: &Credentials) -> Result<Value, String> {
    let config = config.as_object_mut().ok_or("accountError.format")?;
    for key in [
        "privacyCache",
        "autoReviewAvailabilityCache",
        "serverConfigCache",
    ] {
        config.remove(key);
    }
    config.insert(
        "authInfo".into(),
        json!({"email": summary(login, None)?.email, "activeTeamId": login.team_id}),
    );
    Ok(Value::Object(config.clone()))
}

#[cfg(not(target_os = "macos"))]
fn authenticated(mut auth: Value, login: &Credentials) -> Result<Value, String> {
    let object = auth.as_object_mut().ok_or("accountError.format")?;
    object.insert("accessToken".into(), json!(login.access_token));
    object.insert("refreshToken".into(), json!(login.refresh_token));
    object.remove("apiKey");
    Ok(auth)
}

#[cfg(not(target_os = "macos"))]
fn apply_files(
    transaction: &mut Transaction,
    config: &Path,
    auth: &Path,
    login: &Credentials,
) -> Result<(), String> {
    transaction.replace_private_file(
        config,
        &files::json(&configured(files::object(config)?, login)?)?,
    )?;
    transaction.replace_private_file(
        auth,
        &files::json(&authenticated(files::object(auth)?, login)?)?,
    )
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let key = desktop_key()?;
    let mut store = store()?;
    let current = native_credentials(&config_file()?)?;
    sync_current(&mut store, current.as_ref(), key.as_ref(), false)?;
    let login = restored(
        id,
        store.get(id).ok_or("accountError.invalidAccount")?,
        key.as_ref(),
    )?;
    let config = config_file()?;
    let _file_guard = files::DirectoryLock::acquire(&config)?;
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    entry.cli_applied = true;
    entry.applied = false;
    let mut transaction = Transaction::new();
    transaction.replace(
        &store_path()?,
        &files::json(&serde_json::to_value(&store).map_err(|_| "accountError.format")?)?,
    )?;
    #[cfg(not(target_os = "macos"))]
    {
        let path = auth_file()?;
        apply_files(&mut transaction, &config, &path, &login)?;
    }
    #[cfg(target_os = "macos")]
    {
        transaction.replace_private_file(
            &config,
            &files::json(&configured(files::object(&config)?, &login)?)?,
        )?;
        keychain_apply(&login)?;
    }
    transaction.commit();
    summary(&login, Some(id))
}

#[cfg(target_os = "macos")]
fn keychain_read(service: &str) -> Result<Option<String>, String> {
    let output = crate::utils::process::command("/usr/bin/security")
        .args([
            "find-generic-password",
            "-a",
            "cursor-user",
            "-s",
            service,
            "-w",
        ])
        .output()
        .map_err(|_| "accountError.keychain")?;
    if output.status.code() == Some(44) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err("accountError.keychain".into());
    }
    String::from_utf8(output.stdout)
        .map(|s| Some(s.trim_end_matches(['\r', '\n']).into()))
        .map_err(|_| "accountError.keychain".into())
}

#[cfg(target_os = "macos")]
fn keychain_write(service: &str, value: Option<&str>) -> Result<(), String> {
    let mut command = crate::utils::process::command("/usr/bin/security");
    if let Some(value) = value {
        command.args([
            "add-generic-password",
            "-U",
            "-a",
            "cursor-user",
            "-s",
            service,
            "-w",
            value,
        ]);
    } else {
        command.args([
            "delete-generic-password",
            "-a",
            "cursor-user",
            "-s",
            service,
        ]);
    }
    let output = command.output().map_err(|_| "accountError.keychain")?;
    if output.status.success() || (value.is_none() && output.status.code() == Some(44)) {
        Ok(())
    } else {
        Err("accountError.keychain".into())
    }
}

#[cfg(target_os = "macos")]
fn keychain_apply(login: &Credentials) -> Result<(), String> {
    let services = [
        "cursor-access-token",
        "cursor-refresh-token",
        "cursor-api-key",
    ];
    let old = services
        .iter()
        .map(|s| keychain_read(s))
        .collect::<Result<Vec<_>, _>>()?;
    for (service, value) in services.iter().zip([
        Some(login.access_token.as_str()),
        Some(login.refresh_token.as_str()),
        None,
    ]) {
        if let Err(error) = keychain_write(service, value) {
            for (service, previous) in services.iter().zip(&old) {
                if keychain_write(service, previous.as_deref()).is_err() {
                    log::error!("Failed to restore Cursor CLI keychain entry");
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    #[test]
    fn shared_account_sync_does_not_restore_the_other_clients_older_tokens() {
        for key in crate::services::electron_storage::test_ciphers() {
            let mut desktop = login();
            desktop.refresh_token = "desktop-older".into();
            let mut cli = login();
            cli.refresh_token = "cli-newer".into();
            let id = identity(&cli.access_token).unwrap();
            let mut entry = snapshot(&cli, Some(&key)).unwrap();
            // Earlier previews could leave both native-sync markers set.
            entry.applied = true;
            entry.cli_applied = true;
            let mut store = Store::from([(id.clone(), entry)]);
            sync_current(&mut store, Some(&cli), Some(&key), false).unwrap();
            super::super::sync_native(&mut store, Some(&desktop), &key).unwrap();
            assert!(!store[&id].applied);
            assert!(store[&id].cli_applied);
            assert!(
                restored(&id, &store[&id], Some(&key))
                    .unwrap()
                    .refresh_token
                    == "cli-newer"
            );

            let entry = store.get_mut(&id).unwrap();
            entry.applied = true;
            desktop.refresh_token = "desktop-newer".into();
            super::super::sync_native(&mut store, Some(&desktop), &key).unwrap();
            sync_current(&mut store, Some(&cli), Some(&key), false).unwrap();
            assert!(store[&id].applied);
            assert!(!store[&id].cli_applied);
            assert!(
                restored(&id, &store[&id], Some(&key))
                    .unwrap()
                    .refresh_token
                    == "desktop-newer"
            );
        }
    }
    #[test]
    fn native_rotations_preserve_pending_logins_and_deleted_accounts() {
        for key in crate::services::electron_storage::test_ciphers() {
            let mut current = login();
            let id = identity(&current.access_token).unwrap();
            let mut store = Store::new();
            sync_current(&mut store, Some(&current), Some(&key), true).unwrap();
            assert!(store[&id].cli_applied);
            current.refresh_token = "rotated-fixture".into();
            sync_current(&mut store, Some(&current), Some(&key), false).unwrap();
            assert!(
                restored(&id, &store[&id], Some(&key))
                    .unwrap()
                    .refresh_token
                    == current.refresh_token
            );
            let mut desktop_entry = snapshot(&current, Some(&key)).unwrap();
            desktop_entry.applied = true;
            store.insert(id.clone(), desktop_entry);
            current.refresh_token = "rotated-shared-account".into();
            sync_current(&mut store, Some(&current), Some(&key), false).unwrap();
            assert!(!store[&id].cli_applied);
            assert!(
                restored(&id, &store[&id], Some(&key))
                    .unwrap()
                    .refresh_token
                    == "rotated-fixture"
            );
            let mut pending = login();
            pending.refresh_token = "pending-new-login".into();
            store.insert(id.clone(), snapshot(&pending, Some(&key)).unwrap());
            sync_current(&mut store, Some(&current), Some(&key), false).unwrap();
            assert!(
                restored(&id, &store[&id], Some(&key))
                    .unwrap()
                    .refresh_token
                    == pending.refresh_token
            );
            store.remove(&id);
            sync_current(&mut store, Some(&current), Some(&key), false).unwrap();
            assert!(store.is_empty());
        }
    }
    fn login() -> Credentials {
        let jwt = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"sub":"fixture-user","email":"fixture@example.test"}"#);
        Credentials {
            access_token: format!("e30.{jwt}.signature"),
            refresh_token: "fixture-refresh".into(),
            email: Some("fixture@example.test".into()),
            team_id: Some(42),
        }
    }
    #[test]
    fn cli_config_changes_only_account_and_account_bound_caches() {
        let login = login();
        let original = json!({"version":1,"model":{"modelId":"keep"},"permissions":{"allow":["Read"]},"authInfo":{"email":"old","activeTeamId":1},"privacyCache":{"old":true},"serverConfigCache":{"old":true},"autoReviewAvailabilityCache":{},"future":{"setting":true}});
        let updated = configured(original.clone(), &login).unwrap();
        for key in ["model", "permissions", "future", "version"] {
            assert_eq!(original[key], updated[key]);
        }
        assert_eq!(updated["authInfo"]["activeTeamId"], 42);
        for key in [
            "privacyCache",
            "serverConfigCache",
            "autoReviewAvailabilityCache",
        ] {
            assert!(updated.get(key).is_none());
        }
        assert!(native_from_values(&json!({}), &updated).unwrap().is_none());
        assert!(native_from_values(
            &json!({"accessToken":"invalid","refreshToken":"x"}),
            &updated
        )
        .is_err());
        assert!(configured(json!([]), &login).is_err());
    }
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn cli_switch_preserves_other_credentials_and_rolls_back_both_native_files() {
        let root =
            std::env::temp_dir().join(format!("echobird-cursor-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("cli-config.json");
        let auth = root.join("auth.json");
        let desktop = root.join("state.vscdb");
        let original_config =
            br#"{"version":1,"model":{"id":"keep"},"permissions":{"deny":["Write"]}}"#;
        let original_auth = br#"{"accessToken":"old","refreshToken":"old-refresh","apiKey":"old-key","bedrockCredentials":{"accessKey":"keep"},"unknown":true}"#;
        std::fs::write(&config, original_config).unwrap();
        std::fs::write(&auth, original_auth).unwrap();
        std::fs::write(&desktop, b"untouched editor").unwrap();
        let login = login();
        {
            let mut tx = Transaction::new();
            apply_files(&mut tx, &config, &auth, &login).unwrap();
            let value = files::object(&auth).unwrap();
            assert!(value["accessToken"].as_str() == Some(login.access_token.as_str()));
            assert!(value.get("apiKey").is_none());
            assert_eq!(value["bedrockCredentials"]["accessKey"], "keep");
            assert_eq!(value["unknown"], true);
            let imported = native_from_values(&value, &files::object(&config).unwrap())
                .unwrap()
                .unwrap();
            assert!(imported.refresh_token == login.refresh_token);
            assert_eq!(imported.team_id, Some(42));
        }
        assert!(std::fs::read(&auth).unwrap() == original_auth);
        assert!(std::fs::read(&config).unwrap() == original_config);
        {
            let mut tx = Transaction::new();
            assert!(apply_files(&mut tx, &config, &auth.join("invalid"), &login).is_err());
        }
        assert!(std::fs::read(&config).unwrap() == original_config);
        let mut tx = Transaction::new();
        apply_files(&mut tx, &config, &auth, &login).unwrap();
        tx.commit();
        assert!(native_from_values(
            &files::object(&auth).unwrap(),
            &files::object(&config).unwrap()
        )
        .unwrap()
        .is_some());
        assert!(std::fs::read(&desktop).unwrap() == b"untouched editor");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    #[ignore = "Read-only installed Cursor CLI credential and encrypted snapshot verification"]
    async fn installed_cli_credentials_read_only() {
        let _guard = ACCOUNT_LOCK.lock().await;
        let login = native_credentials(&config_file().unwrap())
            .unwrap()
            .expect("CLI must be signed in");
        let key = desktop_key().unwrap();
        let saved = snapshot(&login, key.as_ref()).unwrap();
        let id = identity(&login.access_token).unwrap();
        let recovered = restored(&id, &saved, key.as_ref()).unwrap();
        assert!(recovered.access_token == login.access_token);
        assert!(recovered.refresh_token == login.refresh_token);
        assert!(!saved.session.contains(&login.refresh_token));
        assert!(!serde_json::to_string(&summary(&login, None).unwrap())
            .unwrap()
            .contains("accessToken"));
        // Verify the CLI-only storage path without initializing an editor or
        // creating a key in the user's profile during this read-only probe.
        let key_file = crate::utils::platform::echobird_dir().join("config/.encryption_key");
        let before = std::fs::read(&key_file).expect("Existing EchoBird key required");
        assert_eq!(before.len(), 32);
        let standalone = snapshot(&login, None).unwrap();
        assert!(standalone.session.starts_with("enc:v1:"));
        let recovered = restored(&id, &standalone, None).unwrap();
        assert!(recovered.access_token == login.access_token);
        assert!(recovered.refresh_token == login.refresh_token);
        assert!(std::fs::read(key_file).unwrap() == before);
    }
}
