//! EchoBird-owned Grok Build OAuth account snapshots.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

pub(crate) mod proxy;
mod storage;

static PENDING: OnceLock<Mutex<Option<Pending>>> = OnceLock::new();
static ACCOUNT_LOCK: Mutex<()> = Mutex::new(());
const LOGIN_TIMEOUT: i64 = 60;
const SETTINGS_URL: &str = "https://cli-chat-proxy.grok.com/v1/settings";
const TOKEN_URL: &str = "https://auth.x.ai/oauth2/token";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub active: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    summary: Account,
    key: String,
    #[serde(with = "storage::credential")]
    auth: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_pair: Option<String>,
}
struct Pending {
    id: String,
    dir: PathBuf,
    expires: i64,
    child: Option<std::process::Child>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or("accountError.home".into())
}
fn auth_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("GROK_AUTH_PATH").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    Ok(native_home()?.join("auth.json"))
}
pub(crate) fn native_home() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("GROK_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    Ok(home()?.join(".grok"))
}
fn store() -> Result<PathBuf, String> {
    Ok(home()?.join(".echobird/grok-accounts"))
}
fn saved_path(id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(store()?.join(format!("{id}.json")))
}
fn summary(key: &str, auth: &Value, active: bool) -> Result<Account, String> {
    let row = auth.get(key).ok_or("accountError.invalidAccount")?;
    let email = row
        .get("email")
        .and_then(Value::as_str)
        .unwrap_or("Grok account")
        .to_string();
    let user = row["user_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| row["email"].as_str().filter(|s| !s.is_empty()))
        .ok_or("accountError.invalidAccount")?;
    let id = format!("{:x}", Sha256::digest(format!("{key}\0{user}")));
    Ok(Account {
        id,
        email,
        plan: None,
        active,
    })
}
fn save(key: &str, auth: &Value) -> Result<Account, String> {
    let row = &auth[key];
    if row["oidc_issuer"] != "https://auth.x.ai"
        || row["user_id"]
            .as_str()
            .is_none_or(|id| id.trim().is_empty())
        || row["key"]
            .as_str()
            .is_none_or(|token| token.trim().is_empty())
    {
        return Err("accountError.invalidAccount".into());
    }
    let _guard = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    save_at(&store()?, key, auth)
}
fn save_at(dir: &std::path::Path, key: &str, auth: &Value) -> Result<Account, String> {
    let mut a = summary(key, auth, false)?;
    // Keep existing account IDs stable while separating users on the same issuer.
    let legacy_id = format!("{:x}", Sha256::digest(key));
    let legacy_path = dir.join(format!("{legacy_id}.json"));
    if legacy_path.exists() {
        let legacy: Saved = super::cursor_auth::read(&legacy_path)?;
        let legacy_auth = serde_json::json!({legacy.key.clone(): legacy.auth});
        if summary(&legacy.key, &legacy_auth, false)?.id == a.id {
            a.id = legacy_id;
        }
    }
    let path = dir.join(format!("{}.json", a.id));
    if path.exists() {
        let previous: Saved = super::cursor_auth::read(&path)?;
        a.plan = previous.summary.plan;
    }
    let saved = Saved {
        summary: a.clone(),
        key: key.into(),
        auth: auth[key].clone(),
        native_pair: None,
    };
    super::native_account_files::private_dir(dir)?;
    super::cursor_auth::write(&path, &saved)?;
    proxy::forget_pending(&path);
    Ok(a)
}

#[cfg(test)]
#[path = "grok_account_grade_tests.rs"]
mod grade_tests;

pub async fn start_login() -> Result<(String, i64), String> {
    let mut guard = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    drop(guard.take());
    let exe =
        super::tool_manager::get_tool_exe_path("grok").ok_or("accountError.initializeClient")?;
    let id = uuid::Uuid::new_v4().to_string();
    let expires = chrono::Utc::now().timestamp() + LOGIN_TIMEOUT;
    let dir = home()?.join(".echobird/grok-logins").join(&id);
    super::native_account_files::private_dir(&dir)?;
    let mut pending = Pending {
        id: id.clone(),
        dir,
        expires,
        child: None,
    };
    // Adding an account must not replace the CLI's current login.
    let mut command = crate::utils::process::command(exe);
    command
        .args(["login", "--oauth"])
        .env("GROK_HOME", &pending.dir)
        .env_remove("GROK_AUTH_PATH");

    pending.child = Some(command.spawn().map_err(|_| "accountError.auth")?);
    *guard = Some(pending);
    Ok((id, expires))
}
pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
    let mut guard = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    let p = guard
        .as_mut()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    if chrono::Utc::now().timestamp() >= p.expires {
        *guard = None;
        return Err("accountError.expired".into());
    }
    let now = match fs::read(p.dir.join("auth.json")) {
        Ok(bytes) => match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => value,
            Err(_) => return Ok(None),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("accountError.read".into()),
    };
    let obj = now.as_object().ok_or("accountError.format")?;
    if let Some(key) = obj.keys().next() {
        let a = save(key, &now)?;
        *guard = None;
        return Ok(Some(a));
    }
    Ok(None)
}
pub fn cancel_login(id: &str) -> Result<(), String> {
    let mut guard = PENDING
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| "accountError.busy")?;
    if guard.as_ref().is_some_and(|p| p.id == id) {
        *guard = None;
    }
    Ok(())
}
pub fn list() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    list_at(&store()?, Path::new(""))
}
pub(crate) fn list_saved_accounts() -> Result<Vec<Account>, String> {
    list()
}
fn list_at(dir: &Path, _native_path: &Path) -> Result<Vec<Account>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    #[derive(Deserialize)]
    struct Metadata {
        summary: Account,
    }
    for e in fs::read_dir(dir).map_err(|_| "accountError.read")? {
        let p = e.map_err(|_| "accountError.read")?.path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let saved: Metadata =
            serde_json::from_slice(&fs::read(&p).map_err(|_| "accountError.read")?)
                .map_err(|_| "accountError.format")?;
        out.push(saved.summary)
    }
    out.sort_by(|a, b| a.email.cmp(&b.email));
    Ok(out)
}
pub async fn switch(id: &str) -> Result<Account, String> {
    let path = saved_path(id)?;
    let _action = proxy::account_lock(&path)?.lock_owned().await;
    let native = auth_path()?;
    let _native_lock = storage::NativeLock::acquire(&native, true).await?;
    _native_lock
        .as_ref()
        .ok_or("accountError.busy")?
        .ensure_current()?;
    let _guard = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    let saved = proxy::current(&path)?;
    proxy::valid(&saved, id)?;
    storage::apply_at(
        &store()?,
        &native,
        &native_home()?.join("settings_cache.json"),
        &saved,
    )
}
pub async fn delete(id: &str) -> Result<(), String> {
    let path = saved_path(id)?;
    let _action = proxy::account_lock(&path)?.lock_owned().await;
    let _guard = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    fs::remove_file(&path).map_err(|_| "accountError.write")?;
    proxy::forget_pending(&path);
    drop(_guard);
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::Grok,
        id,
    );
    Ok(())
}

pub async fn refresh(id: &str) -> Result<Account, String> {
    let id = id.to_owned();
    proxy::complete(async move {
        let native = super::tool_manager::get_tool_exe_path("grok").and_then(|_| auth_path().ok());
        refresh_with_native(
            &saved_path(&id)?,
            native.as_deref(),
            SETTINGS_URL,
            TOKEN_URL,
        )
        .await
    })
    .await
}

async fn request_settings(
    client: &reqwest::Client,
    url: &str,
    auth: &Value,
) -> Result<reqwest::Response, String> {
    // Match GrokBuild's official GET /settings user-token authentication.
    let mut request = client
        .get(url)
        .bearer_auth(auth["key"].as_str().ok_or("accountError.auth")?)
        .header("X-XAI-Token-Auth", "xai-grok-cli")
        .header(
            "x-userid",
            auth["user_id"].as_str().ok_or("accountError.auth")?,
        )
        .header("x-grok-client-identifier", "echobird")
        .header("x-grok-client-mode", "headless");
    if let Some(email) = auth["email"].as_str() {
        request = request.header("x-email", email);
    }
    request
        .send()
        .await
        .map_err(|_| "accountError.network".into())
}

async fn renew_auth(client: &reqwest::Client, url: &str, auth: &Value) -> Result<Value, String> {
    // This integration captures the official xAI OAuth login, not custom issuers.
    if auth["oidc_issuer"].as_str() != Some("https://auth.x.ai") {
        return Err("accountError.auth".into());
    }
    let mut form = vec![
        ("grant_type", "refresh_token"),
        (
            "refresh_token",
            auth["refresh_token"].as_str().ok_or("accountError.auth")?,
        ),
        (
            "client_id",
            auth["oidc_client_id"].as_str().ok_or("accountError.auth")?,
        ),
    ];
    for field in ["principal_type", "principal_id"] {
        if let Some(value) = auth[field].as_str() {
            form.push((field, value));
        }
    }
    let response = client
        .post(url)
        .form(&form)
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let status = response.status();
    let body = response.json::<Value>().await;
    if !status.is_success() {
        return Err(if matches!(status.as_u16(), 401 | 403)
            || (status.as_u16() == 400
                && body.as_ref().is_ok_and(|value| {
                    matches!(
                        value["error"].as_str(),
                        Some("invalid_grant" | "invalid_client")
                    )
                })) {
            "accountError.auth"
        } else {
            "accountError.network"
        }
        .into());
    }
    let body = body.map_err(|_| "accountError.format")?;
    let mut renewed = auth.clone();
    renewed["key"] = body["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.format")?
        .into();
    if let Some(token) = body["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        renewed["refresh_token"] = token.into();
    }
    let now = chrono::Utc::now();
    renewed["create_time"] = now.to_rfc3339().into();
    renewed["expires_at"] = if let Some(seconds) = body["expires_in"].as_i64() {
        now.checked_add_signed(chrono::Duration::try_seconds(seconds).ok_or("accountError.format")?)
            .ok_or("accountError.format")?
            .to_rfc3339()
            .into()
    } else {
        Value::Null
    };
    Ok(renewed)
}

#[cfg(test)]
async fn refresh_at(
    path: &Path,
    native_path: &Path,
    settings_url: &str,
    token_url: &str,
) -> Result<Account, String> {
    refresh_with_native(path, Some(native_path), settings_url, token_url).await
}
async fn refresh_with_native(
    path: &Path,
    native_path: Option<&Path>,
    settings_url: &str,
    token_url: &str,
) -> Result<Account, String> {
    let mut session = proxy::Session::open(path, native_path).await?;
    let client = proxy::client()?;
    let mut response = request_settings(&client, settings_url, &session.saved.auth).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        let rejected = session.saved.auth["key"]
            .as_str()
            .ok_or("accountError.auth")?
            .to_owned();
        session.renew(Some(&rejected), token_url).await?;
        response = request_settings(&client, settings_url, &session.saved.auth).await?;
    }
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.auth".into());
    }
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    let settings: Value = response.json().await.map_err(|_| "accountError.format")?;
    let plan = settings["subscription_tier_display"]
        .as_str()
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .ok_or("accountError.format")?;
    let _store = ACCOUNT_LOCK.lock().map_err(|_| "accountError.busy")?;
    let mut latest = proxy::current(path)?;
    if latest.auth != session.saved.auth {
        return Err("accountError.cancelled".into());
    }
    latest.summary.plan = Some(plan.into());
    super::cursor_auth::write(path, &latest)?;
    proxy::forget_pending(path);
    Ok(latest.summary)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn switching_accounts_keeps_original_local_sessions_and_missing_history() {
        let dir = std::env::temp_dir().join(format!("grok-history-{}", uuid::Uuid::new_v4()));
        let store = dir.join("accounts");
        let native = dir.join("auth.json");
        fs::create_dir_all(&dir).unwrap();
        let login = |user: &str| {
            let key = "https://auth.x.ai";
            let auth = json!({key:{"user_id":user,"email":format!("{user}@example.test"),"key":format!("token-{user}"),"oidc_issuer":key}});
            let account = save_at(&store, key, &auth).unwrap();
            super::super::cursor_auth::read::<Saved>(&store.join(format!("{}.json", account.id)))
                .unwrap()
        };
        let a = login("a");
        let b = login("b");
        storage::apply_at(&store, &native, &dir.join("settings_cache.json"), &a).unwrap();
        assert!(!dir.join("sessions").exists());
        let mut originals = Vec::new();
        for id in ["a-session", "b-session"] {
            let session = dir.join("sessions/workspace").join(id);
            fs::create_dir_all(&session).unwrap();
            for (name, value) in [
                (
                    "summary.json",
                    json!({"info":{"id":id,"cwd":"/workspace"},"session_summary":id}),
                ),
                (
                    "chat_history.jsonl",
                    json!({"role":"assistant","content":"original tool result"}),
                ),
            ] {
                let path = session.join(name);
                fs::write(&path, value.to_string()).unwrap();
                originals.push((path.clone(), fs::read(path).unwrap()));
            }
        }
        for saved in [&b, &b, &a] {
            fs::write(dir.join("settings_cache.json"), "old-plan").unwrap();
            storage::apply_at(&store, &native, &dir.join("settings_cache.json"), saved).unwrap();
            let auth: Value =
                serde_json::from_slice(&fs::read(dir.join("auth.json")).unwrap()).unwrap();
            assert_eq!(
                summary(&saved.key, &auth, true).unwrap().id,
                saved.summary.id
            );
            assert!(!dir.join("settings_cache.json").exists());
            for (path, bytes) in &originals {
                assert_eq!(&fs::read(path).unwrap(), bytes);
            }
        }
        // A reported write failure must leave both accounts' original history intact.
        fs::create_dir(dir.join("settings_cache.json")).unwrap();
        let native_before = fs::read(&native).unwrap();
        assert!(storage::apply_at(&store, &native, &dir.join("settings_cache.json"), &b).is_err());
        assert_eq!(fs::read(&native).unwrap(), native_before);
        for (path, bytes) in &originals {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn users_on_the_same_issuer_have_independent_snapshots_and_legacy_ids_survive() {
        let dir = std::env::temp_dir().join(format!("echobird-grok-{}", uuid::Uuid::new_v4()));
        let issuer = "https://auth.example.test";
        let a = json!({issuer:{"user_id":"a", "email":"a@example.test", "key":"token-a"}});
        let b = json!({issuer:{"user_id":"b", "email":"b@example.test", "key":"token-b"}});
        let first = save_at(&dir, issuer, &a).unwrap();
        let second = save_at(&dir, issuer, &b).unwrap();
        assert_ne!(first.id, second.id);
        assert!(dir.join(format!("{}.json", first.id)).exists());
        assert!(dir.join(format!("{}.json", second.id)).exists());
        let legacy_id = format!("{:x}", Sha256::digest(issuer));
        fs::rename(
            dir.join(format!("{}.json", first.id)),
            dir.join(format!("{legacy_id}.json")),
        )
        .unwrap();
        let mut rotated = a.clone();
        rotated[issuer]["key"] = json!("rotated-token");
        assert_eq!(save_at(&dir, issuer, &rotated).unwrap().id, legacy_id);
        assert_eq!(save_at(&dir, issuer, &b).unwrap().id, second.id);
        let legacy: Saved =
            super::super::cursor_auth::read(&dir.join(format!("{legacy_id}.json"))).unwrap();
        assert_eq!(legacy.auth["key"], "rotated-token");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pending_login_cleanup_only_removes_its_isolated_directory() {
        let root = std::env::temp_dir().join(format!("echobird-grok-{}", uuid::Uuid::new_v4()));
        let dir = root.join("login");
        fs::create_dir_all(&dir).unwrap();
        fs::write(root.join("auth.json"), "existing native login").unwrap();
        fs::write(dir.join("auth.json"), "temporary login").unwrap();
        drop(Pending {
            id: "test".into(),
            dir: dir.clone(),
            expires: 0,
            child: None,
        });
        assert!(!dir.exists());
        assert_eq!(
            fs::read_to_string(root.join("auth.json")).unwrap(),
            "existing native login"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
