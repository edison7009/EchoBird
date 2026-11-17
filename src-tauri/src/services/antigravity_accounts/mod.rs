mod native;
mod oauth;
mod quota;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use quota::ModelQuota;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub active: bool,
    pub plan: Option<String>,
    pub quotas: Vec<ModelQuota>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    email: String,
    credential: String,
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    quotas: Vec<ModelQuota>,
}

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/antigravity-accounts.json"))
}

fn load() -> Result<BTreeMap<String, Saved>, String> {
    load_at(&store_path()?)
}

fn load_at(path: &Path) -> Result<BTreeMap<String, Saved>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    super::cursor_auth::read(path)
}

fn save(accounts: &BTreeMap<String, Saved>) -> Result<(), String> {
    save_at(&store_path()?, accounts)
}

fn save_at(path: &Path, accounts: &BTreeMap<String, Saved>) -> Result<(), String> {
    super::cursor_auth::write(path, accounts)
}

fn identity(raw: &str) -> Result<(String, String), String> {
    let credential: Value = serde_json::from_str(raw).map_err(|_| "accountError.format")?;
    if credential["token"]["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .is_none()
    {
        return Err("accountError.invalidAccount".into());
    }
    let token = credential["id_token"]
        .as_str()
        .ok_or("accountError.invalidAccount")?;
    let segment = token
        .split('.')
        .nth(1)
        .ok_or("accountError.invalidAccount")?;
    let decoded = URL_SAFE_NO_PAD
        .decode(segment.trim_end_matches('='))
        .map_err(|_| "accountError.invalidAccount")?;
    let claims: Value =
        serde_json::from_slice(&decoded).map_err(|_| "accountError.invalidAccount")?;
    let subject = claims["sub"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.invalidAccount")?;
    let email = claims["email"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.invalidAccount")?;
    Ok((format!("{:x}", Sha256::digest(subject)), email.to_owned()))
}

fn protect(value: &str) -> Result<String, String> {
    let encrypted = super::model_manager::encrypt_key_for_storage(value);
    if !encrypted.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(encrypted)
}

fn unprotect(value: &str) -> Result<String, String> {
    if !value.starts_with("enc:v1:") {
        return Err("accountError.invalidAccount".into());
    }
    let plain = super::model_manager::decrypt_key_for_use(value);
    if plain.is_empty() {
        return Err("accountError.keychain".into());
    }
    Ok(plain)
}

fn summary(id: &str, saved: &Saved, active: Option<&str>) -> Account {
    Account {
        id: id.to_owned(),
        email: saved.email.clone(),
        active: active == Some(id),
        plan: saved.plan.clone(),
        quotas: saved.quotas.clone(),
    }
}

fn capture(
    raw: &str,
    accounts: &mut BTreeMap<String, Saved>,
    active: bool,
) -> Result<Account, String> {
    let (id, email) = identity(raw)?;
    let previous = accounts.get(&id);
    let saved = Saved {
        email,
        credential: protect(raw)?,
        plan: previous.and_then(|s| s.plan.clone()),
        quotas: previous.map(|s| s.quotas.clone()).unwrap_or_default(),
    };
    let account = summary(&id, &saved, active.then_some(id.as_str()));
    accounts.insert(id, saved);
    save(accounts)?;
    Ok(account)
}

pub(crate) async fn import_snapshot(raw: &str) -> Result<bool, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let (id, email) = identity(raw)?;
    let mut accounts = load()?;
    if accounts.contains_key(&id)
        || accounts
            .values()
            .any(|saved| saved.email.eq_ignore_ascii_case(&email))
    {
        return Ok(false);
    }
    capture(raw, &mut accounts, false)?;
    Ok(true)
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let accounts = load()?;
    let current = if super::tool_manager::any_tool_installed(&["antigravitydesktop", "antigravity"])
    {
        native::read()?
    } else {
        None
    };
    let active = current.as_deref().map(identity).transpose()?.map(|v| v.0);
    let rows = accounts
        .iter()
        .map(|(id, saved)| summary(id, saved, active.as_deref()))
        .collect::<Vec<_>>();
    Ok(rows)
}

pub(crate) async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    list_saved_accounts_at(&store_path()?)
}

fn list_saved_accounts_at(path: &Path) -> Result<Vec<Account>, String> {
    Ok(load_at(path)?
        .iter()
        .map(|(id, saved)| summary(id, saved, None))
        .collect())
}

pub(crate) async fn proxy_credentials(id: &str, force_refresh: bool) -> Result<Value, String> {
    proxy_credentials_at(
        &store_path()?,
        id,
        force_refresh,
        quota::TOKEN_URL,
        unprotect,
        protect,
    )
    .await
}

pub(crate) async fn proxy_credentials_after_rejection(
    id: &str,
    rejected_credentials: &Value,
) -> Result<Value, String> {
    let rejected_token = rejected_credentials["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .ok_or("accountError.loginRequired")?;
    proxy_credentials_with_rejection_at(
        &store_path()?,
        id,
        true,
        quota::TOKEN_URL,
        unprotect,
        protect,
        Some(rejected_token),
    )
    .await
}

async fn proxy_credentials_at(
    path: &Path,
    id: &str,
    force_refresh: bool,
    token_url: &str,
    decode: fn(&str) -> Result<String, String>,
    encode: fn(&str) -> Result<String, String>,
) -> Result<Value, String> {
    proxy_credentials_with_rejection_at(path, id, force_refresh, token_url, decode, encode, None)
        .await
}

async fn proxy_credentials_with_rejection_at(
    path: &Path,
    id: &str,
    force_refresh: bool,
    token_url: &str,
    decode: fn(&str) -> Result<String, String>,
    encode: fn(&str) -> Result<String, String>,
    rejected_token: Option<&str>,
) -> Result<Value, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let accounts = load_at(path)?;
    let saved = accounts.get(id).ok_or("accountError.invalidAccount")?;
    let raw = decode(&saved.credential)?;
    if identity(&raw)?.0 != id {
        return Err("accountError.invalidAccount".into());
    }
    let mut secret: Value = serde_json::from_str(&raw).map_err(|_| "accountError.format")?;
    if force_refresh
        && (rejected_token.is_none() || rejected_token == secret["token"]["access_token"].as_str())
    {
        secret["token"]["expiry"] = Value::Null;
    }
    quota::refresh_access_at(&mut secret, token_url).await?;
    let refreshed = serde_json::to_string(&secret).map_err(|_| "accountError.format")?;
    if refreshed != raw {
        let mut latest_accounts = load_at(path)?;
        let latest = latest_accounts
            .get_mut(id)
            .ok_or("accountError.invalidAccount")?;
        if latest.credential != saved.credential {
            let latest_raw = decode(&latest.credential)?;
            if identity(&latest_raw)?.0 != id {
                return Err("accountError.invalidAccount".into());
            }
            secret = serde_json::from_str(&latest_raw).map_err(|_| "accountError.format")?;
        } else {
            latest.credential = encode(&refreshed)?;
            save_at(path, &latest_accounts)?;
        }
    }
    Ok(serde_json::json!({
        "accessToken":secret["token"]["access_token"].as_str().filter(|s|!s.is_empty()).ok_or("accountError.loginRequired")?,
        "projectId":secret["project_id"].as_str().or_else(||secret["cloudaicompanionProject"].as_str()),
    }))
}

pub async fn start_login() -> Result<LoginStart, String> {
    oauth::start().await
}

pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let Some(raw) = oauth::poll(login_id)? else {
        return Ok(None);
    };
    let mut accounts = load()?;
    let result = capture(&raw, &mut accounts, false)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Antigravity,
        &result.id,
    );
    cancel_login(login_id)?;
    Ok(Some(result))
}

pub fn cancel_login(login_id: &str) -> Result<(), String> {
    oauth::cancel(login_id)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    accounts.remove(id);
    save(&accounts)?;
    super::account_proxy::forget_preparation_metadata(
        crate::models::model::AccountProvider::Antigravity,
        id,
    );
    Ok(())
}

async fn ensure_cli_closed() -> Result<(), String> {
    #[cfg(windows)]
    {
        let result = crate::utils::process::async_command("tasklist")
            .args(["/FI", "IMAGENAME eq agy.exe", "/FO", "CSV", "/NH"])
            .output()
            .await
            .map_err(|_| "accountError.closeClient")?;
        if !result.status.success() {
            return Err("accountError.closeClient".into());
        }
        if String::from_utf8_lossy(&result.stdout)
            .to_ascii_lowercase()
            .contains("\"agy.exe\"")
        {
            return Err("accountError.closeClient".into());
        }
    }
    #[cfg(unix)]
    {
        if crate::utils::process::async_command("pgrep")
            .args(["-x", "agy"])
            .status()
            .await
            .is_ok_and(|status| status.success())
        {
            return Err("accountError.closeClient".into());
        }
    }
    Ok(())
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    let saved = accounts.get(id).ok_or("accountError.invalidAccount")?;
    let mut raw = unprotect(&saved.credential)?;
    if identity(&raw)?.0 != id {
        return Err("accountError.invalidAccount".into());
    }
    let mut document: Value = serde_json::from_str(&raw).map_err(|_| "accountError.format")?;
    quota::refresh_access(&mut document).await?;
    raw = serde_json::to_string(&document).map_err(|_| "accountError.format")?;
    if identity(&raw)?.0 != id {
        return Err("accountError.invalidAccount".into());
    }
    let protected = protect(&raw)?;
    let previous = native::read()?;
    if let Some(current) = previous.as_deref() {
        let current_id = identity(current)?.0;
        if !accounts.contains_key(&current_id) {
            capture(current, &mut accounts, true)?;
        }
    }
    ensure_cli_closed().await?;
    super::process_manager::stop_desktop_for_account("antigravitydesktop").await?;
    native::write(&raw)?;
    let verified = native::read()
        .and_then(|value| value.as_deref().map(identity).transpose())
        .map(|value| value.map(|v| v.0));
    if !matches!(verified, Ok(Some(ref value)) if value == id) {
        restore_native(previous.as_deref())?;
        return Err("accountError.write".into());
    }
    let account = accounts.get_mut(id).ok_or("accountError.invalidAccount")?;
    account.credential = protected;
    let result = summary(id, account, Some(id));
    if let Err(error) = save(&accounts) {
        restore_native(previous.as_deref())?;
        return Err(error);
    }
    Ok(result)
}

fn restore_native(previous: Option<&str>) -> Result<(), String> {
    match previous {
        Some(value) => native::write(value),
        None => native::delete(),
    }
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut accounts = load()?;
    let saved = accounts.get_mut(id).ok_or("accountError.invalidAccount")?;
    let mut document: Value =
        serde_json::from_str(&unprotect(&saved.credential)?).map_err(|_| "accountError.format")?;
    if identity(&document.to_string())?.0 != id {
        return Err("accountError.invalidAccount".into());
    }
    quota::refresh_access(&mut document).await?;
    if identity(&document.to_string())?.0 != id {
        return Err("accountError.invalidAccount".into());
    }
    let (plan, quotas) = quota::fetch(&document).await?;
    saved.credential = protect(&document.to_string())?;
    saved.plan = plan;
    saved.quotas = quotas;
    let active = native::read()?
        .as_deref()
        .map(identity)
        .transpose()?
        .map(|v| v.0);
    let result = summary(id, saved, active.as_deref());
    save(&accounts)?;
    super::account_proxy::forget_preparation_metadata(
        crate::models::model::AccountProvider::Antigravity,
        id,
    );
    Ok(result)
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let Some(raw) = native::read()? else {
        return Ok(None);
    };
    import_snapshot(&raw).await.map(Some)
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    fn fixture_secret(value: &str) -> Result<String, String> {
        Ok(value.to_owned())
    }

    #[tokio::test]
    async fn proxy_reads_only_saved_credentials_and_serializes_own_store_rotation() {
        use axum::{
            http::{HeaderMap, StatusCode},
            routing::any,
            Json, Router,
        };
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        };

        let root = std::env::temp_dir().join(format!(
            "echobird-antigravity-proxy-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("saved.json");
        let native = root.join("native.json");
        fs::write(&native, b"native-fixture-untouched").unwrap();
        let native_hash = Sha256::digest(fs::read(&native).unwrap());
        let claims =
            URL_SAFE_NO_PAD.encode(br#"{"sub":"fixture-id","email":"fixture@example.test"}"#);
        let mut secret = json!({"id_token":format!("x.{claims}.y"),"project_id":"fixture-project", "token":{"access_token":"saved-token","refresh_token":"saved-refresh","expiry":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339()}});
        let raw = secret.to_string();
        let (id, email) = identity(&raw).unwrap();
        let mut accounts = BTreeMap::from([(
            id.clone(),
            Saved {
                email,
                credential: raw,
                plan: None,
                quotas: vec![],
            },
        )]);
        save_at(&path, &accounts).unwrap();
        let own_before = fs::read(&path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let failed_refresh = Arc::new(AtomicBool::new(false));
        let failed = failed_refresh.clone();
        let replaced_during_refresh = Arc::new(AtomicBool::new(false));
        let replace = replaced_during_refresh.clone();
        let mut replacement_secret = secret.clone();
        replacement_secret["token"]["access_token"] = json!("replacement-token");
        replacement_secret["token"]["refresh_token"] = json!("replacement-refresh");
        let mut replacement_accounts = accounts.clone();
        replacement_accounts.get_mut(&id).unwrap().credential = replacement_secret.to_string();
        let replacement_path = path.clone();
        let rejected_requests = Arc::new(AtomicUsize::new(0));
        let rejected_count = rejected_requests.clone();
        let rejected_barrier = Arc::new(tokio::sync::Barrier::new(2));
        let app = Router::new().route("/inference", any(move |headers: HeaderMap| {
            let barrier = rejected_barrier.clone();
            let rejected_count = rejected_count.clone();
            let rejected = headers.get("authorization").and_then(|value| value.to_str().ok()) == Some("Bearer saved-token");
            async move {
                if rejected {
                    rejected_count.fetch_add(1, Ordering::SeqCst);
                    barrier.wait().await;
                    StatusCode::UNAUTHORIZED
                } else { StatusCode::OK }
            }
        })).fallback(any(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            if replace.load(Ordering::SeqCst) {
                save_at(&replacement_path, &replacement_accounts).unwrap();
            }
            let status = if failed.load(Ordering::SeqCst) { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
            async move { (status, Json(json!({"access_token":"rotated-token","refresh_token":"next-refresh","expires_in":3600}))) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let rows = list_saved_accounts_at(&path).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].active);
        assert!(rows[0].quotas.is_empty());
        assert!(!serde_json::to_string(&rows)
            .unwrap()
            .contains("saved-token"));
        let value = proxy_credentials_at(&path, &id, false, &url, fixture_secret, fixture_secret)
            .await
            .unwrap();
        assert_eq!(value["accessToken"], "saved-token");
        assert_eq!(value["projectId"], "fixture-project");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(fs::read(&path).unwrap(), own_before);
        assert!(proxy_credentials_at(
            &path,
            "missing",
            false,
            &url,
            fixture_secret,
            fixture_secret
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        secret["token"]["expiry"] = Value::Null;
        accounts.get_mut(&id).unwrap().credential = secret.to_string();
        save_at(&path, &accounts).unwrap();
        let (first, second) = tokio::join!(
            proxy_credentials_at(&path, &id, false, &url, fixture_secret, fixture_secret),
            proxy_credentials_at(&path, &id, false, &url, fixture_secret, fixture_secret),
        );
        assert_eq!(first.unwrap()["accessToken"], "rotated-token");
        assert_eq!(second.unwrap()["accessToken"], "rotated-token");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let saved = load_at(&path).unwrap();
        let refreshed: Value = serde_json::from_str(&saved[&id].credential).unwrap();
        assert_eq!(refreshed["token"]["refresh_token"], "next-refresh");
        secret["token"]["expiry"] =
            json!((chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339());
        accounts.get_mut(&id).unwrap().credential = secret.to_string();
        save_at(&path, &accounts).unwrap();
        let client = reqwest::Client::new();
        let inference = || async {
            let rejected =
                proxy_credentials_at(&path, &id, false, &url, fixture_secret, fixture_secret)
                    .await
                    .unwrap();
            let rejected_token = rejected["accessToken"].as_str().unwrap();
            assert_eq!(
                client
                    .get(format!("{url}/inference"))
                    .bearer_auth(rejected_token)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            let latest = proxy_credentials_with_rejection_at(
                &path,
                &id,
                true,
                &url,
                fixture_secret,
                fixture_secret,
                Some(rejected_token),
            )
            .await
            .unwrap();
            assert_eq!(
                client
                    .get(format!("{url}/inference"))
                    .bearer_auth(latest["accessToken"].as_str().unwrap())
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
            latest
        };
        let (first, second) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(inference(), inference())
        })
        .await
        .unwrap();
        assert_eq!(first["accessToken"], "rotated-token");
        assert_eq!(second["accessToken"], "rotated-token");
        assert_eq!(rejected_requests.load(Ordering::SeqCst), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        save_at(&path, &accounts).unwrap();
        let before_failure = fs::read(&path).unwrap();
        failed_refresh.store(true, Ordering::SeqCst);
        assert!(proxy_credentials_with_rejection_at(
            &path,
            &id,
            true,
            &url,
            fixture_secret,
            fixture_secret,
            Some("saved-token")
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(fs::read(&path).unwrap(), before_failure);
        failed_refresh.store(false, Ordering::SeqCst);
        replaced_during_refresh.store(true, Ordering::SeqCst);
        let latest = proxy_credentials_with_rejection_at(
            &path,
            &id,
            true,
            &url,
            fixture_secret,
            fixture_secret,
            Some("saved-token"),
        )
        .await
        .unwrap();
        assert_eq!(latest["accessToken"], "replacement-token");
        let saved = load_at(&path).unwrap();
        let replacement: Value = serde_json::from_str(&saved[&id].credential).unwrap();
        assert_eq!(replacement["token"]["refresh_token"], "replacement-refresh");
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(Sha256::digest(fs::read(&native).unwrap()), native_hash);
        server.abort();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_native_identity_without_using_email_as_key() {
        let claims = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            br#"{"sub":"stable-id","email":"a@example.com"}"#,
        );
        let raw = serde_json::json!({"id_token":format!("x.{claims}.y"),"token":{"refresh_token":"refresh"}}).to_string();
        let (id, email) = identity(&raw).unwrap();
        assert_eq!(email, "a@example.com");
        assert_ne!(id, email);
    }
}
