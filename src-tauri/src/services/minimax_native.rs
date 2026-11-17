mod desktop;

use super::{
    minimax_accounts::{self, Account, Credentials, Region},
    native_account_files::{self as files, DirectoryLock, Transaction},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

fn native_region(region: Region) -> &'static str {
    match region {
        Region::Cn => "cn",
        Region::Global => "en",
    }
}

pub(crate) async fn apply(account_id: &str, tool_id: &str) -> Result<Account, String> {
    if !matches!(tool_id, "minimaxcode" | "minimaxdesktop") {
        return Err("accountError.invalidAccount".into());
    }
    let executable =
        super::tool_manager::get_tool_exe_path(tool_id).ok_or("accountError.unavailable")?;
    let session = minimax_accounts::credentials_for_native(account_id).await?;
    let credentials = &session.credentials;
    let home = if tool_id == "minimaxdesktop" {
        if super::tool_config_manager::minimaxcode::desktop_region(Path::new(&executable))
            != Some(native_region(credentials.region))
        {
            return Err("accountError.invalidAccount".into());
        }
        desktop::data_dir(Path::new(&executable), credentials.region)?
    } else {
        let dir = files::absolute_path(&super::tool_config_manager::minimaxcode::data_dir())?;
        if !["MINIMAX_DATA_DIR", "MAVIS_DATA_DIR"]
            .iter()
            .any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()))
        {
            desktop::ensure_default_root(&dirs::home_dir().ok_or("accountError.home")?, &dir)?;
        }
        dir
    };
    if tool_id == "minimaxdesktop" {
        super::process_manager::stop_desktop_for_account(tool_id).await?;
    }
    let _lock = DirectoryLock::acquire(&home.join("config.yaml"))?;
    let auth_home = home
        .join("auth/prod")
        .join(native_region(credentials.region))
        .join("mcode-public");
    let _auth_lock = DirectoryLock::acquire(&auth_home.join("auth.lock"))?;
    let transaction = write_cli(&home, credentials)?;
    let account = minimax_accounts::mark_active(account_id).await?;
    transaction.commit();
    Ok(account)
}

struct Record {
    namespace: PathBuf,
    credential: PathBuf,
    state: PathBuf,
    key: String,
}

fn record(home: &Path, region: Region) -> Record {
    let region = native_region(region);
    let auth_home = home.join("auth");
    let namespace = auth_home.join("prod").join(region).join("mcode-public");
    let account_key = URL_SAFE_NO_PAD.encode(Sha256::digest(
        format!("{}\0mcode-public", auth_home.display()).as_bytes(),
    ));
    Record {
        credential: namespace.join("auth.json"),
        state: namespace.join("auth-state.json"),
        key: format!("com.minimax.mcode.oauth.prod.{region}\0{account_key}"),
        namespace,
    }
}

fn client_homes(region: Region) -> Vec<PathBuf> {
    let mut homes = vec![];
    if let Ok(home) = files::absolute_path(&super::tool_config_manager::minimaxcode::data_dir()) {
        homes.push(home);
    }
    if let Some(executable) = super::tool_manager::get_tool_exe_path("minimaxdesktop") {
        if let Ok(home) = desktop::data_dir(Path::new(&executable), region) {
            if !homes.contains(&home) {
                homes.push(home);
            }
        }
    }
    homes
}

fn subject(token: &str) -> Option<String> {
    super::cursor_auth::claims(token).ok()?["sub"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The newest login a client holds for this account: access, refresh, expiry.
pub(crate) fn client_tokens(c: &Credentials) -> Option<(String, String, i64)> {
    client_tokens_in(c, &client_homes(c.region))
}

fn client_tokens_in(c: &Credentials, homes: &[PathBuf]) -> Option<(String, String, i64)> {
    let account = subject(&c.access_token)?;
    homes
        .iter()
        .filter_map(|home| {
            let location = record(home, c.region);
            let value = files::object(&location.credential).ok()?;
            let entry = &value["records"][&location.key];
            let access = entry["accessToken"].as_str()?;
            (subject(access)? == account).then_some((
                access.to_owned(),
                entry["refreshToken"].as_str()?.to_owned(),
                entry["expiresAtMs"].as_i64()? / 1000,
            ))
        })
        .max_by_key(|(_, _, expires)| *expires)
}

/// Gives every client signed in to this account the token EchoBird rotated.
pub(crate) fn store_rotation(next: &Credentials) -> Result<(), String> {
    store_rotation_in(next, &client_homes(next.region))
}

fn store_rotation_in(next: &Credentials, homes: &[PathBuf]) -> Result<(), String> {
    let Some(account) = subject(&next.access_token) else {
        return Ok(());
    };
    for home in homes {
        let location = record(home, next.region);
        let Ok(mut credential) = files::object(&location.credential) else {
            continue;
        };
        let held = credential["records"][&location.key]["accessToken"]
            .as_str()
            .and_then(subject);
        if held.as_deref() != Some(account.as_str()) {
            continue;
        }
        let _lock = DirectoryLock::acquire(&location.namespace.join("auth.lock"))?;
        let mut state = files::object(&location.state)?;
        let entry = &mut credential["records"][&location.key];
        let generation = entry["generation"]
            .as_u64()
            .unwrap_or(0)
            .max(state["generation"].as_u64().unwrap_or(0))
            + 1;
        let expires = next.expires_at.saturating_mul(1000);
        entry["accessToken"] = json!(next.access_token);
        entry["refreshToken"] = json!(next.refresh_token);
        entry["expiresAtMs"] = json!(expires);
        entry["generation"] = json!(generation);
        if state.is_object() {
            state["expiresAtMs"] = json!(expires);
            state["generation"] = json!(generation);
        }
        let mut transaction = Transaction::new();
        transaction.replace_private_file(&location.credential, &files::json(&credential)?)?;
        if location.state.exists() {
            transaction.replace_private_file(&location.state, &files::json(&state)?)?;
        }
        transaction.commit();
    }
    Ok(())
}

fn write_cli(home: &Path, credentials: &Credentials) -> Result<Transaction, String> {
    let region = native_region(credentials.region);
    let location = record(home, credentials.region);
    let credential_path = location.credential;
    let state_path = location.state;
    let config_path = home.join("config.yaml");
    let preference_path = home.join("preferences/mcode-region.json");
    let mut credential = files::object(&credential_path)?;
    let previous_state = files::object(&state_path)?;
    if credential.get("records").is_some_and(|v| !v.is_object()) {
        return Err("accountError.format".into());
    }
    let key = location.key;
    let generation = credential["records"][&key]["generation"]
        .as_u64()
        .unwrap_or(0)
        .max(previous_state["generation"].as_u64().unwrap_or(0))
        .checked_add(1)
        .filter(|g| *g < 9_007_199_254_740_992)
        .ok_or("accountError.format")?;
    let expires = credentials
        .expires_at
        .checked_mul(1000)
        .ok_or("accountError.format")?;
    let mut record = json!({"schemaVersion":1,"accessToken":credentials.access_token,
        "refreshToken":credentials.refresh_token,"tokenType":"Bearer","clientId":"mcode-public",
        "scopes":["agent.default"],"audience":"agent-backend","expiresAtMs":expires,
        "generation":generation,"loginEpoch":uuid::Uuid::new_v4().to_string()});
    if let Ok(claims) = super::cursor_auth::claims(&credentials.access_token) {
        for (source, target) in [("sub", "subject"), ("account_id", "accountId")] {
            if let Some(value) = claims[source].as_str().filter(|s| !s.is_empty()) {
                record[target] = json!(value);
            }
        }
    }
    credential["schemaVersion"] = json!(1);
    if credential.get("records").is_none() {
        credential["records"] = json!({});
    }
    credential["records"][key] = record;
    let state = json!({"schemaVersion":2,"status":"authenticated","storeKind":"file",
        "clientId":"mcode-public","scopes":["agent.default"],"audience":"agent-backend",
        "buildEnv":"prod","region":region,"expiresAtMs":expires,"generation":generation});
    let mut preference = files::object(&preference_path)?;
    if preference.get("regions").is_some_and(|v| !v.is_object()) {
        return Err("accountError.format".into());
    }
    preference["version"] = json!(1);
    if preference.get("regions").is_none() {
        preference["regions"] = json!({});
    }
    preference["regions"]["prod"] = json!(region);
    preference["updatedAtMs"] = json!(chrono::Utc::now().timestamp_millis());
    let mut config: Value = match files::read_optional(&config_path)? {
        Some(bytes) => serde_yaml_ng::from_slice(&bytes).map_err(|_| "accountError.format")?,
        None => json!({}),
    };
    if config.is_null() {
        config = json!({});
    }
    if !config.is_object()
        || config
            .get("custom_provider")
            .is_some_and(|v| !v.is_object())
    {
        return Err("accountError.format".into());
    }
    super::tool_config_manager::minimaxcode::select_official(&mut config);
    if let Some(providers) = config["custom_provider"].as_object_mut() {
        providers.remove("echobird");
    }
    let mut transaction = Transaction::new();
    for (path, bytes) in [
        (credential_path, files::json(&credential)?),
        (state_path, files::json(&state)?),
        (preference_path, files::json(&preference)?),
        (
            config_path,
            serde_yaml_ng::to_string(&config)
                .map_err(|_| "accountError.format")?
                .into_bytes(),
        ),
    ] {
        transaction.replace(&path, &bytes)?;
    }
    Ok(transaction)
}

pub(crate) fn discover(region: Region) -> Vec<Credentials> {
    client_homes(region)
        .iter()
        .filter_map(|home| {
            let location = record(home, region);
            let value = files::object(&location.credential).ok()?;
            let entry = &value["records"][&location.key];
            let token = entry["accessToken"].as_str()?;
            let claims = super::cursor_auth::claims(token).ok()?;
            let user = claims["account_id"]
                .as_str()
                .or_else(|| claims["sub"].as_str())?;
            Some(Credentials {
                region,
                access_token: token.into(),
                refresh_token: entry["refreshToken"].as_str()?.into(),
                expires_at: entry["expiresAtMs"].as_i64()? / 1000,
                user_id: user.into(),
                email: claims["email"].as_str().unwrap_or(user).into(),
                name: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn rotations_reach_only_clients_signed_in_to_the_same_account() {
        let token = |sub: &str| {
            format!(
                "h.{}.s",
                URL_SAFE_NO_PAD.encode(json!({ "sub": sub }).to_string())
            )
        };
        let credentials = |sub: &str, refresh: &str, expires: i64| Credentials {
            region: Region::Global,
            access_token: token(sub),
            refresh_token: refresh.into(),
            expires_at: expires,
            user_id: sub.into(),
            email: format!("{sub}@example.test"),
            name: None,
        };
        let root =
            std::env::temp_dir().join(format!("echobird-minimax-share-{}", uuid::Uuid::new_v4()));
        let (mine, other) = (root.join("mine"), root.join("other"));
        for (home, sub) in [(&mine, "user-a"), (&other, "user-b")] {
            write_cli(home, &credentials(sub, &format!("{sub}-old"), 1_000))
                .unwrap()
                .commit();
        }
        let homes = vec![mine.clone(), other.clone()];
        let (_, refresh, expires) =
            client_tokens_in(&credentials("user-a", "x", 0), &homes).unwrap();
        assert_eq!((refresh.as_str(), expires), ("user-a-old", 1_000));

        store_rotation_in(&credentials("user-a", "user-a-new", 2_000), &homes).unwrap();
        let read = |home: &Path| {
            let location = record(home, Region::Global);
            files::object(&location.credential).unwrap()["records"][&location.key]["refreshToken"]
                .clone()
        };
        assert_eq!(read(&mine), "user-a-new");
        assert_eq!(read(&other), "user-b-old");
        let (_, refresh, expires) =
            client_tokens_in(&credentials("user-a", "x", 0), &homes).unwrap();
        assert_eq!((refresh.as_str(), expires), ("user-a-new", 2_000));
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;
    use std::path::PathBuf;
    fn fixture(region: Region) -> Credentials {
        Credentials {
            region,
            access_token: "synthetic-access".into(),
            refresh_token: "synthetic-refresh".into(),
            expires_at: 2_000_000_000,
            user_id: "fixture-user".into(),
            email: "fixture@example.test".into(),
            name: None,
        }
    }
    fn home() -> PathBuf {
        std::env::temp_dir().join(format!("echobird-minimax-native-{}", uuid::Uuid::new_v4()))
    }
    #[test]
    fn cli_regions_select_native_namespace_and_preserve_custom_configuration() {
        let home = home();
        files::write(&home.join("config.yaml"),b"theme: dark\ncustom_provider:\n  user: {enabled: true}\n  echobird: {}\ndefaultModel: custom_provider:echobird/test\n").unwrap();
        files::write(
            &home.join("preferences/mcode-region.json"),
            b"{\"version\":1,\"regions\":{\"test\":\"cn\"}}",
        )
        .unwrap();
        for region in [Region::Cn, Region::Global] {
            write_cli(&home, &fixture(region)).unwrap().commit();
        }
        for region in ["cn", "en"] {
            let directory = home.join("auth/prod").join(region).join("mcode-public");
            let state = files::object(&directory.join("auth-state.json")).unwrap();
            assert_eq!(state["status"], "authenticated");
            assert_eq!(state["region"], region);
            assert_eq!(state["schemaVersion"], 2);
            assert_eq!(state["expiresAtMs"], 2_000_000_000_000i64);
            assert!(state.get("accessToken").is_none());
            let record = files::object(&directory.join("auth.json")).unwrap();
            assert_eq!(record["records"].as_object().unwrap().len(), 1);
            assert_eq!(
                record["records"]
                    .as_object()
                    .unwrap()
                    .values()
                    .next()
                    .unwrap()["accessToken"],
                "synthetic-access"
            );
        }
        let preference = files::object(&home.join("preferences/mcode-region.json")).unwrap();
        assert_eq!(preference["regions"], json!({"test":"cn","prod":"en"}));
        let config: Value =
            serde_yaml_ng::from_slice(&std::fs::read(home.join("config.yaml")).unwrap()).unwrap();
        assert_eq!(config["theme"], "dark");
        assert_eq!(config["custom_provider"]["user"]["enabled"], true);
        assert!(config["custom_provider"].get("echobird").is_none());
        assert!(config.get("defaultModel").is_none());
        assert_eq!(config["minimaxModelSource"], "token_plan");
        std::fs::remove_dir_all(home).unwrap();
    }
    #[test]
    fn malformed_native_configuration_is_preserved_and_auth_not_written() {
        let home = home();
        files::write(&home.join("config.yaml"), b"invalid: [").unwrap();
        assert!(write_cli(&home, &fixture(Region::Cn)).is_err());
        assert!(!home.join("auth/prod/cn/mcode-public/auth.json").exists());
        assert_eq!(
            std::fs::read(home.join("config.yaml")).unwrap(),
            b"invalid: ["
        );
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn native_credential_key_uses_the_resolved_lexical_data_root() {
        let root = home();
        let normalized = files::absolute_path(&root.join("data/../.minimax")).unwrap();
        write_cli(&normalized, &fixture(Region::Cn))
            .unwrap()
            .commit();
        let auth_home = root.join(".minimax").join("auth");
        let key = format!(
            "com.minimax.mcode.oauth.prod.cn\0{}",
            URL_SAFE_NO_PAD.encode(Sha256::digest(
                format!("{}\0mcode-public", auth_home.display()).as_bytes()
            ))
        );
        let saved = files::object(&auth_home.join("prod/cn/mcode-public/auth.json")).unwrap();
        assert_eq!(saved["records"][key]["accessToken"], "synthetic-access");
        std::fs::remove_dir_all(root).unwrap();
    }
}
