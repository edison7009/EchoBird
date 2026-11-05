//! Saved Cursor accounts only. No native catalog sync, usage lookup or account switch.
use super::{
    claims, cli, data_dir, database, identity, native, put, read, store_path, sync_native, write,
    Account, Store, ACCESS, ACCOUNT_LOCK, REFRESH,
};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub(crate) async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    list_saved_accounts_at(&store_path()?).await
}

async fn list_saved_accounts_at(path: &Path) -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    if !path.exists() {
        return Ok(vec![]);
    }
    let store: Store = read(path)?;
    Ok(store
        .iter()
        .map(|(id, saved)| Account {
            id: id.clone(),
            email: saved
                .email
                .as_deref()
                .filter(|email| !email.trim().is_empty())
                .unwrap_or("—")
                .to_owned(),
            active: false,
            usage: saved.usage.clone(),
        })
        .collect())
}

pub(crate) async fn speech_credentials(
    id: &str,
    rejected: Option<&Value>,
) -> Result<Value, String> {
    selected_credentials(id, rejected, false).await
}

pub(crate) async fn proxy_credentials(id: &str, force: bool) -> Result<Value, String> {
    selected_credentials(id, None, force).await
}

async fn selected_credentials(
    id: &str,
    rejected: Option<&Value>,
    force: bool,
) -> Result<Value, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = store_path()?;
    let mut store: Store = read(&path)?;
    let key = cli::desktop_key()?;
    // The signed-in editor owns the newest token of its account.
    let editor = key
        .as_ref()
        .and_then(|_| data_dir().ok())
        .and_then(|dir| database(&dir, false).ok())
        .and_then(|db| native(&db).ok().flatten());
    if let (Some(key), Some(login)) = (&key, &editor) {
        if identity(&login.access_token)? == id {
            sync_native(&mut store, Some(login), key)?;
        }
    }
    let cli_holds_token = cli::adopt(&mut store, id, key.as_ref()).unwrap_or(false);
    if cli_holds_token {
        write(&path, &store)?;
    }
    let entry = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    let client_holds_token = entry.applied
        && editor
            .as_ref()
            .is_some_and(|login| identity(&login.access_token).is_ok_and(|active| active == id));
    let mut current = cli::restored(id, entry, key.as_ref())?;
    if should_rotate(&current.access_token, rejected, force)? {
        let rotated = refresh_pair(&current.refresh_token).await?;
        if identity(&rotated.access_token)? != id {
            return Err("accountError.invalidAccount".into());
        }
        current.access_token = rotated.access_token;
        current.refresh_token = rotated.refresh_token;
        let replacement = cli::snapshot(&current, key.as_ref())?;
        entry.session = replacement.session;
        entry.email = replacement.email;
        write(&path, &store)?;
        if client_holds_token {
            let db = database(&data_dir()?, true)?;
            put(&db, ACCESS, &current.access_token)?;
            put(&db, REFRESH, &current.refresh_token)?;
        }
        if cli_holds_token {
            cli::store_rotation(&current)?;
        }
    }
    Ok(json!({"accessToken":current.access_token,"teamId":current.team_id}))
}

pub(crate) fn should_rotate(
    token: &str,
    rejected: Option<&Value>,
    force: bool,
) -> Result<bool, String> {
    let expiry = claims(token)?["exp"].as_i64();
    Ok(force
        || expiry.is_none_or(|expiry| expiry <= chrono::Utc::now().timestamp() + 300)
        || rejected.is_some_and(|value| value["accessToken"].as_str() == Some(token)))
}

pub(crate) struct RotatedPair {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
}

pub(crate) async fn refresh_pair(refresh: &str) -> Result<RotatedPair, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network")?;
    refresh_pair_at(&client, "https://api2.cursor.sh/oauth/token", refresh).await
}

async fn refresh_pair_at(
    client: &reqwest::Client,
    endpoint: &str,
    refresh: &str,
) -> Result<RotatedPair, String> {
    if refresh.trim().is_empty() {
        return Err("accountError.invalidAccount".into());
    }
    let mut response = client
        .post(endpoint)
        .json(&json!({
            "client_id":"KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB",
            "grant_type":"refresh_token","refresh_token":refresh,
        }))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err("accountError.auth".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "accountError.network")? {
        if bytes.len() + chunk.len() > 65_536 {
            return Err("accountError.authResponse".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    pair_from(
        &serde_json::from_slice::<Value>(&bytes).map_err(|_| "accountError.authResponse")?,
        refresh,
    )
}

fn pair_from(value: &Value, old_refresh: &str) -> Result<RotatedPair, String> {
    if value["shouldLogout"] == true {
        return Err("accountError.auth".into());
    }
    let access = value["access_token"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or("accountError.authResponse")?;
    identity(access)?;
    let refresh = match value.get("refresh_token") {
        None | Some(Value::Null) => old_refresh,
        Some(value) => value
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or("accountError.authResponse")?,
    };
    Ok(RotatedPair {
        access_token: access.into(),
        refresh_token: refresh.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{credentials, saved, Cipher};
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use cbc::cipher::KeyInit;

    fn token(subject: &str, expiry: i64) -> String {
        format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(json!({"sub":subject,"exp":expiry}).to_string())
        )
    }
    fn key() -> Cipher {
        Cipher::Gcm(Box::new(
            aes_gcm::Aes256Gcm::new_from_slice(&[8; 32]).unwrap(),
        ))
    }

    #[tokio::test]
    async fn saved_directory_reads_its_file_without_native_cipher_or_passive_writes() {
        let root =
            std::env::temp_dir().join(format!("cursor-speech-saved-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("cursor-accounts.json");
        assert!(!root.join("Cursor/Local State").exists());
        assert!(list_saved_accounts_at(&path).await.unwrap().is_empty());
        assert!(!path.exists());
        let stored = json!({
            "cached-id":{"session":"not-decryptable","applied":true,"email":"saved@example.test","usage":{"plan":"cached-plan","remainingPercent":42.0,"resetAt":null}},
            "legacy-id":{"session":"not-decryptable","applied":false},
            "blank-id":{"session":"not-decryptable","applied":false,"email":" "}
        });
        std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
        let before = std::fs::read(&path).unwrap();
        let legacy: Store = read(&path).unwrap();
        assert!(serde_json::to_value(&legacy).unwrap()["legacy-id"]
            .get("email")
            .is_none());
        let rows = list_saved_accounts_at(&path).await.unwrap();
        assert_eq!(rows.len(), 3);
        let cached = rows.iter().find(|row| row.id == "cached-id").unwrap();
        assert_eq!(cached.email, "saved@example.test");
        assert_eq!(
            cached.usage.as_ref().unwrap().plan.as_deref(),
            Some("cached-plan")
        );
        assert_eq!(cached.usage.as_ref().unwrap().remaining_percent, Some(42.0));
        assert_eq!(
            rows.iter().find(|row| row.id == "legacy-id").unwrap().email,
            "—"
        );
        assert_eq!(
            rows.iter().find(|row| row.id == "blank-id").unwrap().email,
            "—"
        );
        assert!(rows.iter().all(|row| !row.active));
        assert_eq!(before, std::fs::read(&path).unwrap());
        assert!(!serde_json::to_string(&rows)
            .unwrap()
            .contains("not-decryptable"));
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::write(&path, b"bad-json").unwrap();
        assert!(list_saved_accounts_at(&path).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"bad-json");
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&root).unwrap();
    }

    #[test]
    fn explicit_credentials_keep_identity_checks_and_only_cache_public_email() {
        let key = key();
        let login = super::super::Credentials {
            access_token: token("selected", i64::MAX),
            refresh_token: "refresh".into(),
            email: Some("saved@example.test".into()),
            team_id: Some(12),
        };
        let id = identity(&login.access_token).unwrap();
        let entry = saved(&login, &key, false).unwrap();
        assert_eq!(entry.email.as_deref(), Some("saved@example.test"));
        assert!(credentials(&id, &entry, &key).is_ok());
        assert!(credentials("wrong-provider-id", &entry, &key).is_err());
        let encoded = serde_json::to_string(&entry).unwrap();
        assert!(!encoded.contains(&login.access_token));
        assert!(!encoded.contains("\"refresh\""));
    }

    #[test]
    fn rejected_token_comparison_prevents_a_second_rotation() {
        let old = token("selected", i64::MAX);
        let new = format!("{old}rotated");
        assert!(should_rotate(&old, Some(&json!({"accessToken":old})), false).unwrap());
        assert!(!should_rotate(&new, Some(&json!({"accessToken":old})), false).unwrap());
        assert!(should_rotate(&new, None, true).unwrap());
        assert!(should_rotate(&token("selected", 1), None, false).unwrap());
        let pair = pair_from(
            &json!({"access_token":new,"refresh_token":"rotation"}),
            "old",
        )
        .unwrap();
        assert_eq!(pair.refresh_token, "rotation");
        assert_eq!(
            pair_from(&json!({"access_token":old}), "keep")
                .unwrap()
                .refresh_token,
            "keep"
        );
        assert!(pair_from(&json!({"access_token":old,"shouldLogout":true}), "keep").is_err());
    }

    #[tokio::test]
    async fn refresh_uses_the_native_body_and_never_returns_upstream_error_text() {
        use axum::{routing::post, Json, Router};
        let access = token("selected", i64::MAX);
        let app=Router::new().route("/oauth/token",post(move |Json(body):Json<Value>|{let access=access.clone();async move {
            assert_eq!(body,json!({"client_id":"KbZUR41cY7W6zRSdpSUJ7I7mLYBKOCmB","grant_type":"refresh_token","refresh_token":"fixture-refresh"}));
            Json(json!({"access_token":access,"refresh_token":"fixture-rotated"}))
        }})).route("/reject",post(||async {(axum::http::StatusCode::UNAUTHORIZED,"private-upstream-secret")}));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/oauth/token", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = refresh_pair_at(&reqwest::Client::new(), &endpoint, "fixture-refresh").await;
        let rejected = refresh_pair_at(
            &reqwest::Client::new(),
            &endpoint.replace("/oauth/token", "/reject"),
            "fixture-refresh",
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap().refresh_token, "fixture-rotated");
        assert_eq!(rejected.err().unwrap(), "accountError.auth");
    }
}
