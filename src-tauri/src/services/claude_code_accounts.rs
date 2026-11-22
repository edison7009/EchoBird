use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const LOGIN_REQUIRED: &str = "accountError.loginRequired";
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const LOCK_STALE: Duration = Duration::from_secs(60);
const LOCK_WAIT: Duration = Duration::from_secs(10);
const WARM_UP_CONFIRMATIONS: usize = 3;
const WARM_UP_PAUSE: Duration = Duration::from_secs(2);
pub(super) static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The directory locks Claude Code holds while it rotates tokens. Refresh tokens
/// are single use, so the client and EchoBird must never rotate at the same time.
struct RefreshLocks(Vec<PathBuf>);

impl RefreshLocks {
    async fn acquire() -> Result<Self, String> {
        Self::acquire_in(&config_dir()?).await
    }

    async fn acquire_in(dir: &Path) -> Result<Self, String> {
        let mut locks = Self(vec![]);
        if !dir.is_dir() {
            return Ok(locks);
        }
        let mut shared = fs::canonicalize(dir)
            .unwrap_or_else(|_| dir.to_path_buf())
            .into_os_string();
        shared.push(".lock");
        for path in [dir.join(".oauth_refresh.lock"), PathBuf::from(shared)] {
            acquire_directory(&path).await?;
            locks.0.push(path);
        }
        Ok(locks)
    }
}

impl Drop for RefreshLocks {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = fs::remove_dir(path);
        }
    }
}

async fn acquire_directory(path: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        match fs::create_dir(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                return Err(format!("accountError.read|{error}"));
            }
            Err(_) => {}
        }
        let abandoned = fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > LOCK_STALE);
        if abandoned {
            let _ = fs::remove_dir(path);
        } else if started.elapsed() >= LOCK_WAIT {
            return Err("accountError.unavailable".into());
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeQuota {
    pub remaining_percent: i32,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCodeAccount {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub five_hour: Option<ClaudeCodeQuota>,
    pub seven_day: Option<ClaudeCodeQuota>,
    pub active: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedAccount {
    summary: ClaudeCodeAccount,
    oauth: Value,
    identity: Value,
}

pub(crate) fn config_dir() -> Result<PathBuf, String> {
    if let Ok(value) = std::env::var("CLAUDE_CONFIG_DIR") {
        if !value.trim().is_empty() {
            return Ok(PathBuf::from(value.trim().trim_matches('"')));
        }
    }
    Ok(home()?.join(".claude"))
}

fn home() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "accountError.home".to_string())
}

pub(crate) fn config_path() -> Result<PathBuf, String> {
    let dir = config_dir()?;
    if dir.join(".config.json").exists() {
        return Ok(dir.join(".config.json"));
    }
    Ok(if dir == home()?.join(".claude") {
        home()?.join(".claude.json")
    } else {
        dir.join(".claude.json")
    })
}

fn store() -> Result<PathBuf, String> {
    Ok(home()?.join(".echobird/claude-code-accounts"))
}

fn account_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("accountError.invalidAccount".to_string());
    }
    Ok(dir.join(format!("{id}.json")))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read(path).map_err(|_| format!("accountError.read|{}", path.display()))?;
    serde_json::from_slice(&raw).map_err(|_| format!("accountError.format|{}", path.display()))
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    use std::io::Write;
    fs::create_dir_all(path.parent().ok_or("accountError.write")?)
        .map_err(|e| format!("accountError.write|{e}"))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| format!("accountError.format|{e}"))?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|e| format!("accountError.write|{e}"))?;
        file.write_all(&bytes)
            .map_err(|e| format!("accountError.write|{e}"))?;
        file.sync_all()
            .map_err(|e| format!("accountError.write|{e}"))?;
        drop(file);
        fs::rename(&temporary, path).map_err(|e| format!("accountError.write|{e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(target_os = "macos")]
fn keychain_service() -> Result<String, String> {
    let suffix = if std::env::var("CLAUDE_CONFIG_DIR").is_ok_and(|s| !s.trim().is_empty()) {
        let hash = format!(
            "{:x}",
            Sha256::digest(config_dir()?.to_string_lossy().as_bytes())
        );
        format!("-{}", &hash[..8])
    } else {
        String::new()
    };
    Ok(format!("Claude Code-credentials{suffix}"))
}

fn read_credentials() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let output = crate::utils::process::command("security")
            .args(["find-generic-password", "-s", &keychain_service()?, "-w"])
            .output()
            .map_err(|e| format!("accountError.keychain|{e}"))?;
        if output.status.success() {
            return serde_json::from_slice(&output.stdout).map_err(|_| LOGIN_REQUIRED.to_string());
        }
    }
    let path = config_dir()?.join(".credentials.json");
    if !path.exists() {
        return Err(LOGIN_REQUIRED.to_string());
    }
    read_json(&path)
}

fn write_credentials(value: &Value) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = crate::utils::process::command("security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                &keychain_service()?,
                "-a",
                &std::env::var("USER").map_err(|e| format!("accountError.keychain|{e}"))?,
                "-w",
                &value.to_string(),
            ])
            .output()
            .map_err(|e| format!("accountError.keychain|{e}"))?;
        if !output.status.success() {
            return Err("accountError.keychain".to_string());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    write_json(&config_dir()?.join(".credentials.json"), value)
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}

fn snapshot(credentials: &Value, config: &Value) -> Result<SavedAccount, String> {
    let oauth = credentials
        .get("claudeAiOauth")
        .filter(|v| text(v, "accessToken").is_some())
        .ok_or(LOGIN_REQUIRED)?;
    let identity = config.get("oauthAccount").ok_or(LOGIN_REQUIRED)?;
    let email = text(identity, "emailAddress").ok_or(LOGIN_REQUIRED)?;
    let account = text(identity, "accountUuid").unwrap_or(email);
    let org = text(identity, "organizationUuid").unwrap_or_default();
    let id = format!(
        "{:x}",
        Sha256::digest(format!("{account}\n{org}").as_bytes())
    );
    let plan = text(oauth, "subscriptionType").or_else(|| text(identity, "subscriptionType"));
    let tier = text(oauth, "rateLimitTier")
        .or_else(|| text(identity, "rateLimitTier"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let normalized_plan = plan.map(str::to_ascii_lowercase);
    let plan = match normalized_plan.as_deref() {
        Some("ultra") => Some("Ultra".to_string()),
        Some("max") if tier.contains("20x") || tier.contains("ultra") => Some("Ultra".to_string()),
        Some(value) => Some(
            value
                .split_whitespace()
                .map(|word| {
                    let mut chars = word.chars();
                    chars
                        .next()
                        .map(|c| c.to_uppercase().to_string() + chars.as_str())
                        .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        None => None,
    };
    Ok(SavedAccount {
        summary: ClaudeCodeAccount {
            id,
            email: email.to_string(),
            plan,
            five_hour: None,
            seven_day: None,
            active: true,
        },
        oauth: oauth.clone(),
        identity: identity.clone(),
    })
}

fn current() -> Result<SavedAccount, String> {
    let config = config_path()?;
    if !config.exists() {
        return Err(LOGIN_REQUIRED.to_string());
    }
    snapshot(&read_credentials()?, &read_json(&config)?)
}

fn load(dir: &Path, id: &str) -> Result<SavedAccount, String> {
    let account: SavedAccount = serde_json::from_value(read_json(&account_path(dir, id)?)?)
        .map_err(|_| "accountError.invalidAccount".to_string())?;
    let identity = snapshot(
        &json!({"claudeAiOauth":account.oauth}),
        &json!({"oauthAccount":account.identity}),
    )?;
    if account.summary.id != id || identity.summary.id != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(account)
}

fn save(dir: &Path, account: &SavedAccount) -> Result<(), String> {
    write_json(&account_path(dir, &account.summary.id)?, account)
}

// Keep native token rotations before switching away or refreshing a saved account.
fn sync_current(dir: &Path) -> Result<Option<SavedAccount>, String> {
    let mut account = match current() {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let path = account_path(dir, &account.summary.id)?;
    if path.exists() {
        let saved = load(dir, &account.summary.id)?;
        // Adding the same account through OAuth must not be overwritten by an older CLI login.
        if text(&account.oauth, "accessToken") != text(&saved.oauth, "accessToken")
            && account.oauth["expiresAt"].as_i64().unwrap_or(0)
                <= saved.oauth["expiresAt"].as_i64().unwrap_or(0)
        {
            return Ok(Some(saved));
        }
        account.summary.five_hour = saved.summary.five_hour;
        account.summary.seven_day = saved.summary.seven_day;
    }
    if path.exists() {
        save(dir, &account)?;
    }
    Ok(Some(account))
}

pub async fn list() -> Result<Vec<ClaudeCodeAccount>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = store()?;
    let active = sync_current(&dir)?.map(|a| a.summary.id);
    list_saved_at(&dir, active.as_deref())
}

fn list_saved_at(dir: &Path, active: Option<&str>) -> Result<Vec<ClaudeCodeAccount>, String> {
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut accounts = vec![];
    for entry in fs::read_dir(dir).map_err(|e| format!("accountError.read|{e}"))? {
        let path = entry.map_err(|e| format!("accountError.read|{e}"))?.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        let mut account = load(dir, id)?.summary;
        account.active = active == Some(&account.id);
        accounts.push(account);
    }
    accounts.sort_by(|a, b| a.email.cmp(&b.email).then(a.id.cmp(&b.id)));
    Ok(accounts)
}

pub(crate) async fn list_saved_accounts() -> Result<Vec<ClaudeCodeAccount>, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    list_saved_at(&store()?, None)
}

pub(crate) async fn proxy_credentials(id: &str, force_refresh: bool) -> Result<Value, String> {
    proxy_credentials_with_rejection_at(&store()?, id, force_refresh, TOKEN_URL, None, true).await
}

pub(crate) async fn proxy_credentials_after_rejection(
    id: &str,
    rejected_credentials: &Value,
) -> Result<Value, String> {
    let rejected_token = text(rejected_credentials, "accessToken").ok_or(LOGIN_REQUIRED)?;
    proxy_credentials_with_rejection_at(&store()?, id, true, TOKEN_URL, Some(rejected_token), true)
        .await
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    let saved: SavedAccount =
        serde_json::from_value(value.clone()).map_err(|_| "backupError.format")?;
    let parsed = snapshot(
        &json!({"claudeAiOauth":saved.oauth}),
        &json!({"oauthAccount":saved.identity}),
    )?;
    if parsed.summary.id != saved.summary.id {
        return Err("backupError.format".into());
    }
    Ok(())
}

#[cfg(test)]
async fn proxy_credentials_at(
    dir: &Path,
    id: &str,
    force_refresh: bool,
    token_url: &str,
) -> Result<Value, String> {
    proxy_credentials_with_rejection_at(dir, id, force_refresh, token_url, None, false).await
}

async fn proxy_credentials_with_rejection_at(
    dir: &Path,
    id: &str,
    force_refresh: bool,
    token_url: &str,
    rejected_token: Option<&str>,
    follow_native: bool,
) -> Result<Value, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    // The signed-in account's newest token lives with the client.
    let active = follow_native && sync_current(dir)?.is_some_and(|a| a.summary.id == id);
    let mut account = load(dir, id)?;
    if force_refresh
        && (rejected_token.is_none() || rejected_token == text(&account.oauth, "accessToken"))
        || account.oauth["expiresAt"]
            .as_i64()
            .is_some_and(|t| t <= chrono::Utc::now().timestamp_millis() + 60_000)
    {
        renew_at(dir, &mut account, active, token_url).await?;
    }
    Ok(
        json!({"accessToken": text(&account.oauth, "accessToken").ok_or(LOGIN_REQUIRED)?, "accountId":id, "oauth": account.oauth, "identity": account.identity}),
    )
}

pub(super) fn save_login(credentials: &Value, config: &Value) -> Result<ClaudeCodeAccount, String> {
    let mut account = snapshot(credentials, config)?;
    account.summary.active = current().is_ok_and(|live| live.summary.id == account.summary.id);
    save(&store()?, &account)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::ClaudeCode,
        &account.summary.id,
    );
    Ok(account.summary)
}

pub(crate) async fn import_snapshot(credentials: &Value, config: &Value) -> Result<bool, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    import_snapshot_at(credentials, config, &store()?)
}

fn import_snapshot_at(credentials: &Value, config: &Value, dir: &Path) -> Result<bool, String> {
    let account = snapshot(credentials, config)?;
    if list_saved_at(dir, None)?.iter().any(|saved| {
        saved.id == account.summary.id || saved.email.eq_ignore_ascii_case(&account.summary.email)
    }) {
        return Ok(false);
    }
    let temporary = dir.join(format!(".import-{}.json", uuid::Uuid::new_v4()));
    write_json(&temporary, &account)?;
    let result = fs::hard_link(&temporary, account_path(dir, &account.summary.id)?);
    let _ = fs::remove_file(&temporary);
    match result {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(_) => return Err("accountError.write".into()),
    }
    Ok(true)
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn duplicate_id_or_email_never_replaces_saved_credentials() {
        let root = tempfile::tempdir().unwrap();
        let credentials = json!({"claudeAiOauth":{"accessToken":"fixture-first"}});
        let profile = |id: &str, email: &str| json!({"oauthAccount":{"accountUuid":id,"emailAddress":email,"organizationUuid":"fixture-org"}});
        assert!(import_snapshot_at(
            &credentials,
            &profile("first", "a@example.test"),
            root.path()
        )
        .unwrap());
        let id = snapshot(&credentials, &profile("first", "a@example.test"))
            .unwrap()
            .summary
            .id;
        let path = account_path(root.path(), &id).unwrap();
        let before = fs::read(&path).unwrap();
        let changed = json!({"claudeAiOauth":{"accessToken":"fixture-changed"}});
        assert!(!import_snapshot_at(
            &changed,
            &profile("first", "different@example.test"),
            root.path()
        )
        .unwrap());
        assert!(
            !import_snapshot_at(&changed, &profile("other", "A@example.test"), root.path())
                .unwrap()
        );
        assert_eq!(fs::read(path).unwrap(), before);
        assert!(import_snapshot_at(
            &credentials,
            &profile("second", "b@example.test"),
            root.path()
        )
        .unwrap());
    }
}

fn merge_oauth(target: &mut Value, key: &str, value: &Value) -> Result<(), String> {
    target
        .as_object_mut()
        .ok_or("accountError.format")?
        .insert(key.to_string(), value.clone());
    Ok(())
}

pub async fn switch(id: &str) -> Result<ClaudeCodeAccount, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let _locks = RefreshLocks::acquire().await?;
    let dir = store()?;
    sync_current(&dir)?;
    let mut account = load(&dir, id)?;
    let config_file = config_path()?;
    let mut config = if config_file.exists() {
        read_json(&config_file)?
    } else {
        json!({})
    };
    let original_config = config.clone();
    let mut credentials = match read_credentials() {
        Ok(value) => value,
        Err(_) if !config_dir()?.join(".credentials.json").exists() => json!({}),
        Err(error) => return Err(error),
    };
    merge_oauth(&mut config, "oauthAccount", &account.identity)?;
    config["hasCompletedOnboarding"] = json!(true);
    merge_oauth(&mut credentials, "claudeAiOauth", &account.oauth)?;
    write_json(&config_file, &config)?;
    if let Err(error) = write_credentials(&credentials) {
        write_json(&config_file, &original_config)
            .map_err(|rollback| format!("accountError.rollback|{error} {rollback}"))?;
        return Err(error);
    }
    account.summary.active = true;
    Ok(account.summary)
}

pub(super) fn quota(window: &Value) -> Option<ClaudeCodeQuota> {
    let used = window.get("utilization")?.as_f64()?;
    let reset_at = text(window, "resets_at")
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp());
    Some(ClaudeCodeQuota {
        remaining_percent: (100.0 - used).clamp(0.0, 100.0).round() as i32,
        reset_at,
    })
}

async fn request_quota(token: &str) -> Result<reqwest::Response, String> {
    reqwest::Client::new()
        .get("https://api.anthropic.com/api/oauth/usage")
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| "accountError.network".to_string())
}

fn quota_http_error(status: reqwest::StatusCode) -> String {
    let key = match status.as_u16() {
        401 => LOGIN_REQUIRED,
        403 => "accountError.denied",
        429 => "accountError.rateLimited",
        500..=599 => "accountError.unavailable",
        _ => "accountError.quota",
    };
    format!("{key}|HTTP {}", status.as_u16())
}

async fn renewal_response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let body = response.json::<Value>().await;
    if !status.is_success() {
        if status == reqwest::StatusCode::BAD_REQUEST
            && body
                .as_ref()
                .is_ok_and(|body| body["error"].as_str() == Some("invalid_grant"))
        {
            return Err(format!(
                "{LOGIN_REQUIRED}|HTTP {}|invalid_grant",
                status.as_u16()
            ));
        }
        return Err(quota_http_error(status));
    }
    body.map_err(|_| "accountError.format".to_string())
}

async fn renew(dir: &Path, account: &mut SavedAccount, update_native: bool) -> Result<(), String> {
    renew_at(dir, account, update_native, TOKEN_URL).await
}

async fn renew_at(
    dir: &Path,
    account: &mut SavedAccount,
    update_native: bool,
    token_url: &str,
) -> Result<(), String> {
    let _locks = if update_native {
        Some(RefreshLocks::acquire().await?)
    } else {
        None
    };
    if update_native {
        // The client may have rotated while waiting; spending the old token would revoke it.
        if let Some(live) = sync_current(dir)?.filter(|live| live.summary.id == account.summary.id)
        {
            if text(&live.oauth, "accessToken") != text(&account.oauth, "accessToken") {
                account.oauth = live.oauth;
                return Ok(());
            }
        }
    }
    let original_oauth = account.oauth.clone();
    let refresh_token = text(&account.oauth, "refreshToken").ok_or(LOGIN_REQUIRED)?;
    let old_token = text(&account.oauth, "accessToken")
        .unwrap_or_default()
        .to_string();
    let response = reqwest::Client::new()
        .post(token_url)
        .timeout(Duration::from_secs(15))
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        }))
        .send()
        .await
        .map_err(|_| "accountError.network".to_string())?;
    let body = renewal_response(response).await?;
    let token = text(&body, "access_token").ok_or("accountError.format")?;
    account.oauth["accessToken"] = json!(token);
    if let Some(token) = text(&body, "refresh_token") {
        account.oauth["refreshToken"] = json!(token);
    }
    if let Some(seconds) = body.get("expires_in").and_then(Value::as_i64) {
        account.oauth["expiresAt"] = json!(chrono::Utc::now().timestamp_millis() + seconds * 1000);
    }
    let latest = load(dir, &account.summary.id)?;
    if latest.oauth != original_oauth {
        *account = latest;
        return Ok(());
    }
    save(dir, account)?;
    // Do not overwrite a different login or a token rotated by the CLI while awaiting HTTP.
    if update_native
        && current().is_ok_and(|live| {
            live.summary.id == account.summary.id
                && text(&live.oauth, "accessToken") == Some(old_token.as_str())
        })
    {
        let mut credentials = read_credentials()?;
        merge_oauth(&mut credentials, "claudeAiOauth", &account.oauth)?;
        write_credentials(&credentials)?;
    }
    Ok(())
}

pub async fn refresh(id: &str) -> Result<ClaudeCodeAccount, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let dir = store()?;
    let active = sync_current(&dir)?.is_some_and(|a| a.summary.id == id);
    let mut account = load(&dir, id)?;
    let mut response =
        request_quota(text(&account.oauth, "accessToken").ok_or(LOGIN_REQUIRED)?).await?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        renew(&dir, &mut account, active).await?;
        response =
            request_quota(text(&account.oauth, "accessToken").ok_or(LOGIN_REQUIRED)?).await?;
    }
    let status = response.status();
    if !status.is_success() {
        return Err(quota_http_error(status));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "accountError.format".to_string())?;
    let five_hour = quota(&body["five_hour"]);
    let seven_day = quota(&body["seven_day"]);
    if five_hour.is_none() && seven_day.is_none() {
        return Err("accountError.quota".to_string());
    }
    account.summary.five_hour = five_hour;
    account.summary.seven_day = seven_day;
    account.summary.active = active;
    save(&dir, &account)?;
    Ok(account.summary)
}

fn window_running(account: &ClaudeCodeAccount) -> bool {
    account
        .five_hour
        .as_ref()
        .and_then(|window| window.reset_at)
        .is_some_and(|reset_at| reset_at > chrono::Utc::now().timestamp())
}

/// Shared by Claude Code and Claude Desktop accounts: both draw on one subscription.
pub(super) async fn warm_up_with<R, RF, S, SF>(
    refresh: R,
    send: S,
    pause: Duration,
) -> Result<ClaudeCodeAccount, String>
where
    R: Fn() -> RF,
    RF: std::future::Future<Output = Result<ClaudeCodeAccount, String>>,
    S: FnOnce() -> SF,
    SF: std::future::Future<Output = Result<(), String>>,
{
    // A running window keeps its end time, so a message would only spend quota.
    let mut latest = refresh().await;
    if latest.as_ref().is_ok_and(window_running) {
        return latest;
    }
    send().await?;
    // Usage can lag behind the message that started the window.
    for _ in 0..WARM_UP_CONFIRMATIONS {
        tokio::time::sleep(pause).await;
        latest = refresh().await.or(latest);
        if latest.as_ref().is_ok_and(window_running) {
            break;
        }
    }
    latest
}

pub async fn warm_up(id: &str, executable: &str) -> Result<ClaudeCodeAccount, String> {
    warm_up_with(
        || refresh(id),
        || async {
            let credentials = proxy_credentials(id, false).await?;
            super::account_proxy::warm_up_claude(&credentials, executable).await
        },
        WARM_UP_PAUSE,
    )
    .await
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let path = account_path(&store()?, id)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| format!("accountError.write|{e}"))?;
    }
    Ok(())
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    if !config_path()?.exists() {
        return Ok(None);
    }
    let account = current()?;
    import_snapshot(
        &json!({"claudeAiOauth":account.oauth}),
        &json!({"oauthAccount":account.identity}),
    )
    .await
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warm_up_fixture(reset_at: Option<i64>) -> ClaudeCodeAccount {
        ClaudeCodeAccount {
            id: "fixture".into(),
            email: "fixture@example.test".into(),
            plan: Some("Max".into()),
            five_hour: Some(ClaudeCodeQuota {
                remaining_percent: 100,
                reset_at,
            }),
            seven_day: None,
            active: false,
        }
    }

    /// Runs a warm-up against scripted usage reads; returns the outcome, the
    /// number of reads and whether a message was sent.
    async fn warm_up_run(
        reads: Vec<Result<ClaudeCodeAccount, String>>,
        send: Result<(), String>,
    ) -> (Result<ClaudeCodeAccount, String>, usize, bool) {
        let reads = std::sync::Mutex::new(reads);
        let count = std::sync::atomic::AtomicUsize::new(0);
        let sent = std::sync::atomic::AtomicBool::new(false);
        let result = warm_up_with(
            || {
                count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let next = reads.lock().unwrap().remove(0);
                async move { next }
            },
            || {
                sent.store(true, std::sync::atomic::Ordering::SeqCst);
                async move { send }
            },
            Duration::ZERO,
        )
        .await;
        (result, count.into_inner(), sent.into_inner())
    }

    #[tokio::test]
    async fn warming_a_running_window_sends_nothing() {
        let running = chrono::Utc::now().timestamp() + 3600;
        let (result, reads, sent) =
            warm_up_run(vec![Ok(warm_up_fixture(Some(running)))], Ok(())).await;
        assert_eq!(result.unwrap().five_hour.unwrap().reset_at, Some(running));
        assert_eq!(reads, 1);
        assert!(!sent);
    }

    #[tokio::test]
    async fn warming_an_idle_account_waits_for_the_window_to_appear() {
        let started = chrono::Utc::now().timestamp() + 18_000;
        let (result, reads, sent) = warm_up_run(
            vec![
                Ok(warm_up_fixture(None)),
                Ok(warm_up_fixture(None)),
                Ok(warm_up_fixture(Some(started))),
            ],
            Ok(()),
        )
        .await;
        assert_eq!(result.unwrap().five_hour.unwrap().reset_at, Some(started));
        assert_eq!(reads, 3);
        assert!(sent);
    }

    #[tokio::test]
    async fn a_finished_window_counts_as_idle() {
        let ended = chrono::Utc::now().timestamp() - 60;
        let (_, _, sent) = warm_up_run(
            vec![
                Ok(warm_up_fixture(Some(ended))),
                Ok(warm_up_fixture(None)),
                Ok(warm_up_fixture(None)),
                Ok(warm_up_fixture(None)),
            ],
            Ok(()),
        )
        .await;
        assert!(sent);
    }

    #[tokio::test]
    async fn unreadable_usage_still_warms_and_a_failed_message_is_reported() {
        let error = || Err::<ClaudeCodeAccount, _>("accountError.rateLimited|HTTP 429".to_string());
        let started = chrono::Utc::now().timestamp() + 18_000;
        let (result, _, sent) =
            warm_up_run(vec![error(), Ok(warm_up_fixture(Some(started)))], Ok(())).await;
        assert!(sent);
        assert!(result.is_ok());
        let (result, reads, _) =
            warm_up_run(vec![error()], Err("accountError.failed".into())).await;
        assert_eq!(result.err().unwrap(), "accountError.failed");
        assert_eq!(reads, 1);
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

        let root =
            std::env::temp_dir().join(format!("echobird-claude-proxy-{}", uuid::Uuid::new_v4()));
        let dir = root.join("saved");
        let native = root.join("native.json");
        fs::create_dir_all(&root).unwrap();
        fs::write(&native, b"native-fixture-untouched").unwrap();
        let native_hash = Sha256::digest(fs::read(&native).unwrap());
        let mut account = snapshot(
            &json!({"claudeAiOauth":{"accessToken":"saved-token","refreshToken":"saved-refresh", "expiresAt":chrono::Utc::now().timestamp_millis()+3_600_000}}),
            &json!({"oauthAccount":{"emailAddress":"fixture@example.test"}}),
        ).unwrap();
        save(&dir, &account).unwrap();
        let own_before = fs::read(account_path(&dir, &account.summary.id).unwrap()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let failed_refresh = Arc::new(AtomicBool::new(false));
        let failed = failed_refresh.clone();
        let replaced_during_refresh = Arc::new(AtomicBool::new(false));
        let replace = replaced_during_refresh.clone();
        let mut replacement = serde_json::to_value(&account).unwrap();
        replacement["oauth"]["accessToken"] = json!("replacement-token");
        replacement["oauth"]["refreshToken"] = json!("replacement-refresh");
        let replacement_raw = serde_json::to_vec(&replacement).unwrap();
        let replacement_path = account_path(&dir, &account.summary.id).unwrap();
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
                fs::write(&replacement_path, &replacement_raw).unwrap();
            }
            let status = if failed.load(Ordering::SeqCst) { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::OK };
            async move { (status, Json(json!({"access_token":"rotated-token","refresh_token":"rotated-refresh","expires_in":3600}))) }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let rows = list_saved_at(&dir, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(!rows[0].active);
        assert!(rows[0].five_hour.is_none());
        assert!(!serde_json::to_string(&rows)
            .unwrap()
            .contains("saved-token"));
        let secret = proxy_credentials_at(&dir, &account.summary.id, false, &url)
            .await
            .unwrap();
        assert_eq!(secret["accessToken"], "saved-token");
        assert_eq!(secret["accountId"], account.summary.id);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            fs::read(account_path(&dir, &account.summary.id).unwrap()).unwrap(),
            own_before
        );
        assert!(proxy_credentials_at(&dir, &"f".repeat(64), false, &url)
            .await
            .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        account.identity["emailAddress"] = json!("other@example.test");
        save(&dir, &account).unwrap();
        assert!(proxy_credentials_at(&dir, &account.summary.id, false, &url)
            .await
            .is_err());
        account.identity["emailAddress"] = json!("fixture@example.test");
        save(&dir, &account).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        account.oauth["expiresAt"] = json!(0);
        save(&dir, &account).unwrap();
        let (first, second) = tokio::join!(
            proxy_credentials_at(&dir, &account.summary.id, false, &url),
            proxy_credentials_at(&dir, &account.summary.id, false, &url),
        );
        assert_eq!(first.unwrap()["accessToken"], "rotated-token");
        assert_eq!(second.unwrap()["accessToken"], "rotated-token");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            load(&dir, &account.summary.id).unwrap().oauth["refreshToken"],
            "rotated-refresh"
        );
        account.oauth["expiresAt"] = json!(chrono::Utc::now().timestamp_millis() + 3_600_000);
        save(&dir, &account).unwrap();
        let client = reqwest::Client::new();
        let inference = || async {
            let rejected = proxy_credentials_at(&dir, &account.summary.id, false, &url)
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
                &dir,
                &account.summary.id,
                true,
                &url,
                Some(rejected_token),
                false,
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

        save(&dir, &account).unwrap();
        let before_failure = fs::read(account_path(&dir, &account.summary.id).unwrap()).unwrap();
        failed_refresh.store(true, Ordering::SeqCst);
        assert!(proxy_credentials_with_rejection_at(
            &dir,
            &account.summary.id,
            true,
            &url,
            Some("saved-token"),
            false,
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            fs::read(account_path(&dir, &account.summary.id).unwrap()).unwrap(),
            before_failure
        );
        failed_refresh.store(false, Ordering::SeqCst);
        replaced_during_refresh.store(true, Ordering::SeqCst);
        let latest = proxy_credentials_with_rejection_at(
            &dir,
            &account.summary.id,
            true,
            &url,
            Some("saved-token"),
            false,
        )
        .await
        .unwrap();
        assert_eq!(latest["accessToken"], "replacement-token");
        assert_eq!(
            load(&dir, &account.summary.id).unwrap().oauth["refreshToken"],
            "replacement-refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(Sha256::digest(fs::read(&native).unwrap()), native_hash);
        server.abort();
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn renewal_preserves_http_failures_and_only_invalidates_explicit_credentials() {
        use axum::{routing::any, Router};

        for (status, body, expected) in [
            (200, r#"{"access_token":"fixture-access"}"#, None),
            (200, "not json", Some("accountError.format")),
            (
                400,
                r#"{"error":"invalid_grant"}"#,
                Some("accountError.loginRequired|HTTP 400|invalid_grant"),
            ),
            (
                400,
                r#"{"error":"invalid_request"}"#,
                Some("accountError.quota|HTTP 400"),
            ),
            (
                400,
                r#"{"message":"invalid_grant"}"#,
                Some("accountError.quota|HTTP 400"),
            ),
            (400, "invalid_grant", Some("accountError.quota|HTTP 400")),
            (401, "not json", Some("accountError.loginRequired|HTTP 401")),
            (
                403,
                r#"{"error":"invalid_grant"}"#,
                Some("accountError.denied|HTTP 403"),
            ),
            (429, "not json", Some("accountError.rateLimited|HTTP 429")),
            (503, "not json", Some("accountError.unavailable|HTTP 503")),
        ] {
            let status = reqwest::StatusCode::from_u16(status).unwrap();
            let app = Router::new().fallback(any(move || async move { (status, body) }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let response = reqwest::Client::new().get(url).send().await.unwrap();
            let result = renewal_response(response).await;
            server.abort();
            if let Some(expected) = expected {
                assert_eq!(result.unwrap_err(), expected);
            } else {
                assert_eq!(result.unwrap()["access_token"], "fixture-access");
            }
        }
    }

    #[tokio::test]
    async fn refresh_locks_wait_for_the_client_and_take_over_only_when_abandoned() {
        let dir = std::env::temp_dir().join(format!("echobird-lock-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let held = dir.join(".oauth_refresh.lock");
        fs::create_dir(&held).unwrap();
        let release = held.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            fs::remove_dir(release).unwrap();
        });
        let started = std::time::Instant::now();
        let locks = RefreshLocks::acquire_in(&dir).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(200));
        assert!(held.is_dir());
        drop(locks);
        assert!(!held.exists());

        fs::create_dir(&held).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(120);
        #[cfg(windows)]
        let directory = {
            use std::os::windows::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .custom_flags(0x0200_0000)
                .open(&held)
                .unwrap()
        };
        #[cfg(not(windows))]
        let directory = fs::File::open(&held).unwrap();
        directory.set_modified(old).unwrap();
        drop(directory);
        drop(RefreshLocks::acquire_in(&dir).await.unwrap());
        assert!(!held.exists());

        let missing = dir.join("missing");
        assert!(RefreshLocks::acquire_in(&missing)
            .await
            .unwrap()
            .0
            .is_empty());
    }

    // macOS keeps these credentials in the login keychain, which a test must not touch.
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn signed_in_account_shares_one_token_with_the_client() {
        use axum::{http::StatusCode, routing::any, Json, Router};
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let root = std::env::temp_dir().join(format!("echobird-claude-{}", uuid::Uuid::new_v4()));
        let config = root.join("config");
        let dir = root.join("saved");
        fs::create_dir_all(&config).unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", &config);
        let identity = json!({"emailAddress":"active@example.test"});
        fs::write(
            config.join(".claude.json"),
            json!({"oauthAccount":identity}).to_string(),
        )
        .unwrap();
        let now = chrono::Utc::now().timestamp_millis();
        let credentials = |access: &str, refresh: &str, expires: i64| {
            fs::write(
                config.join(".credentials.json"),
                json!({"claudeAiOauth":{"accessToken":access,"refreshToken":refresh,"expiresAt":expires}})
                    .to_string(),
            )
            .unwrap();
        };
        credentials("old-access", "old-refresh", now + 3_600_000);
        let saved = current().unwrap();
        save(&dir, &saved).unwrap();
        let id = saved.summary.id.clone();

        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let app = Router::new().fallback(any(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            async {
                (
                    StatusCode::OK,
                    Json(json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600})),
                )
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        // A token the client already rotated is used as is, never spent again.
        credentials("cli-access", "cli-refresh", now + 7_200_000);
        let secret = proxy_credentials_with_rejection_at(&dir, &id, false, &url, None, true)
            .await
            .unwrap();
        assert_eq!(secret["accessToken"], "cli-access");
        assert_eq!(
            load(&dir, &id).unwrap().oauth["refreshToken"],
            "cli-refresh"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        // EchoBird's own rotation reaches the client, so its copy stays valid.
        let secret = proxy_credentials_with_rejection_at(&dir, &id, true, &url, None, true)
            .await
            .unwrap();
        assert_eq!(secret["accessToken"], "new-access");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let native: Value =
            serde_json::from_slice(&fs::read(config.join(".credentials.json")).unwrap()).unwrap();
        assert_eq!(native["claudeAiOauth"]["refreshToken"], "new-refresh");
        assert!(!config.join(".oauth_refresh.lock").exists());

        server.abort();
        std::env::remove_var("CLAUDE_CONFIG_DIR");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn usage_denial_and_transient_failures_keep_their_status() {
        for (status, expected) in [
            (401, "accountError.loginRequired|HTTP 401"),
            (403, "accountError.denied|HTTP 403"),
            (429, "accountError.rateLimited|HTTP 429"),
            (500, "accountError.unavailable|HTTP 500"),
            (400, "accountError.quota|HTTP 400"),
        ] {
            assert_eq!(
                quota_http_error(reqwest::StatusCode::from_u16(status).unwrap()),
                expected
            );
        }
    }

    #[test]
    fn recognizes_max_tiers_without_guessing_an_unknown_tier() {
        for (plan, tier, expected) in [
            ("max", "default_claude_max_5x", "Max"),
            ("Max", "default_claude_max_20x", "Ultra"),
            ("max", "default_claude_ultra", "Ultra"),
            ("ultra", "", "Ultra"),
            ("MAX", "", "Max"),
            ("pro", "", "Pro"),
            ("free", "", "Free"),
        ] {
            let account = snapshot(
                &json!({"claudeAiOauth":{"accessToken":"test","subscriptionType":plan,"rateLimitTier":tier}}),
                &json!({"oauthAccount":{"emailAddress":"test@example.com"}}),
            ).unwrap();
            assert_eq!(account.summary.plan.as_deref(), Some(expected));
        }
    }

    #[test]
    fn saves_separate_accounts_and_replaces_snapshot_without_exposing_tokens() {
        let dir =
            std::env::temp_dir().join(format!("echobird-claude-test-{}", uuid::Uuid::new_v4()));
        let mut first = snapshot(
            &json!({"claudeAiOauth":{"accessToken":"first-token", "refreshToken":"first-refresh"}}),
            &json!({"oauthAccount":{"emailAddress":"first@example.com"}}),
        )
        .unwrap();
        let second = snapshot(
            &json!({"claudeAiOauth":{"accessToken":"second-token"}}),
            &json!({"oauthAccount":{"emailAddress":"second@example.com"}}),
        )
        .unwrap();
        save(&dir, &first).unwrap();
        save(&dir, &second).unwrap();
        first.oauth["accessToken"] = json!("rotated-token");
        save(&dir, &first).unwrap();
        assert_eq!(
            load(&dir, &first.summary.id).unwrap().oauth["accessToken"],
            "rotated-token"
        );
        assert_eq!(
            load(&dir, &second.summary.id).unwrap().oauth["accessToken"],
            "second-token"
        );
        let summary = serde_json::to_string(&first.summary).unwrap();
        assert!(!summary.contains("token"));
        assert!(!summary.contains("refresh"));
        fs::remove_file(account_path(&dir, &first.summary.id).unwrap()).unwrap();
        assert!(load(&dir, &second.summary.id).is_ok());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_api_key_only_and_incomplete_login() {
        assert!(snapshot(&json!({"apiKey":"test"}), &json!({})).is_err());
        assert!(snapshot(&json!({"claudeAiOauth":{"accessToken":"test"}}), &json!({})).is_err());
    }

    #[test]
    fn account_id_survives_token_rotation_and_separates_organizations() {
        let mut config = json!({"oauthAccount":{"emailAddress":"test@example.com", "accountUuid":"a", "organizationUuid":"one"}});
        let first = snapshot(&json!({"claudeAiOauth":{"accessToken":"first"}}), &config).unwrap();
        let next = snapshot(&json!({"claudeAiOauth":{"accessToken":"next"}}), &config).unwrap();
        assert_eq!(first.summary.id, next.summary.id);
        config["oauthAccount"]["organizationUuid"] = json!("two");
        assert_ne!(
            first.summary.id,
            snapshot(&json!({"claudeAiOauth":{"accessToken":"next"}}), &config)
                .unwrap()
                .summary
                .id
        );
    }

    #[test]
    fn switch_merges_identity_without_replacing_user_settings() {
        let mut config =
            json!({"projects":{"demo":{}},"theme":"dark","oauthAccount":{"emailAddress":"old"}});
        merge_oauth(&mut config, "oauthAccount", &json!({"emailAddress":"new"})).unwrap();
        assert_eq!(config["theme"], "dark");
        assert!(config["projects"].get("demo").is_some());
        assert_eq!(config["oauthAccount"]["emailAddress"], "new");
        assert!(merge_oauth(&mut json!([]), "oauthAccount", &json!({})).is_err());
        assert!(account_path(Path::new("test"), "../credentials").is_err());
    }

    #[test]
    fn quota_keeps_both_windows_and_their_own_resets() {
        let body = json!({"five_hour":{"utilization":20,"resets_at":"2026-09-19T12:00:00Z"},"seven_day":{"utilization":85,"resets_at":"2026-09-22T12:00:00Z"}});
        let five = quota(&body["five_hour"]).unwrap();
        let seven = quota(&body["seven_day"]).unwrap();
        assert_eq!(five.remaining_percent, 80);
        assert_eq!(seven.remaining_percent, 15);
        assert_eq!(seven.reset_at.unwrap() - five.reset_at.unwrap(), 3 * 86400);
        assert!(quota(&Value::Null).is_none());
        assert_eq!(
            quota(&json!({"utilization":0})).unwrap().remaining_percent,
            100
        );
        assert_eq!(
            quota(&json!({"utilization":100}))
                .unwrap()
                .remaining_percent,
            0
        );
    }
}
