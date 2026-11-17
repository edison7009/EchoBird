//! Devin accounts shared by the desktop and terminal applications.
mod native;
mod oauth;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::Duration,
};

pub(crate) const SERVER: &str = "https://server.codeium.com";
pub(crate) const VERSION: &str = "3000.11.92";
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
pub use oauth::LoginStart;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub label: String,
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
    pub active: bool,
    #[serde(default)]
    pub quota_windows: Vec<QuotaWindow>,
    pub balance_usd: Option<f64>,
    pub subscription_end_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Credentials {
    pub token: String,
    pub server: String,
    pub user_id: String,
    pub email: String,
    pub user: Value,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    account: Account,
    secret: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Store {
    accounts: BTreeMap<String, Saved>,
    #[serde(default)]
    ignored: BTreeSet<String>,
}

fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/devin-accounts.json"))
}
fn load() -> Result<Store, String> {
    let path = store_path()?;
    if !path.exists() {
        return Ok(Store::default());
    }
    super::cursor_auth::read(&path)
}
fn write(store: &Store) -> Result<(), String> {
    super::native_account_files::write(
        &store_path()?,
        &super::native_account_files::json(
            &serde_json::to_value(store).map_err(|_| "accountError.format")?,
        )?,
    )
}
fn id(c: &Credentials) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{}\0{}", c.server, c.user_id))
    )
}
fn encode(c: &Credentials) -> Result<String, String> {
    let raw = serde_json::to_string(c).map_err(|_| "accountError.format")?;
    let encrypted = super::model_manager::encrypt_key_for_storage(&raw);
    if !encrypted.starts_with("enc:v1:") {
        return Err("accountError.keychain".into());
    }
    Ok(encrypted)
}
fn decode(saved: &Saved) -> Result<Credentials, String> {
    if !saved.secret.starts_with("enc:v1:") {
        return Err("accountError.format".into());
    }
    let raw = super::model_manager::decrypt_key_for_use(&saved.secret);
    let c: Credentials = serde_json::from_str(&raw).map_err(|_| "accountError.keychain")?;
    if id(&c) != saved.account.id {
        return Err("accountError.invalidAccount".into());
    }
    validate(&c.token, &c.server)?;
    Ok(c)
}
fn validate(token: &str, server: &str) -> Result<(), String> {
    if !token.starts_with("devin-session-token$") || token.len() <= 20 {
        return Err("accountError.invalidAccount".into());
    }
    let url = url::Url::parse(server).map_err(|_| "accountError.format")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("accountError.format".into());
    }
    Ok(())
}
fn scalar_number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite())
}
fn unix(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|v| *v > 0)
}
pub(crate) fn plan_name(value: &Value) -> Option<String> {
    if let Some(n) = value.as_u64() {
        return Some(match n {
            1 | 7 | 14 | 15 => "Teams".into(),
            2 | 8 | 16 => "Pro".into(),
            3 | 4 | 5 | 10 | 11 | 12 => "Enterprise".into(),
            17 | 18 => "Max".into(),
            19 => "Free".into(),
            9 | 20 => "Trial".into(),
            0 => return None,
            _ => format!("Tier {n}"),
        });
    }
    let raw = value.as_str()?.trim();
    if raw.is_empty() {
        return None;
    }
    let name = raw
        .trim_start_matches("TEAMS_TIER_")
        .trim_start_matches("DEVIN_")
        .trim_end_matches("_V2");
    match name.to_ascii_lowercase().as_str() {
        "free" => Some("Free".into()),
        "pro" | "pro_ultimate" => Some("Pro".into()),
        "max" => Some("Max".into()),
        "teams" | "team" | "teams_ultimate" => Some("Teams".into()),
        "trial" => Some("Trial".into()),
        "unspecified" => None,
        s if s.starts_with("enterprise") || s == "hybrid" => Some("Enterprise".into()),
        _ => Some(raw.to_owned()),
    }
}
fn quota(user: &Value) -> (Vec<QuotaWindow>, Option<f64>, Option<i64>) {
    let status = &user["planStatus"];
    let info = &status["planInfo"];
    let quota = info["billingStrategy"] == "BILLING_STRATEGY_QUOTA" || info["billingStrategy"] == 2;
    let mut windows = Vec::new();
    for (label, left, reset, hide) in [
        (
            "1 day",
            "dailyQuotaRemainingPercent",
            "dailyQuotaResetAtUnix",
            "hideDailyQuota",
        ),
        (
            "7 days",
            "weeklyQuotaRemainingPercent",
            "weeklyQuotaResetAtUnix",
            "hideWeeklyQuota",
        ),
    ] {
        if info[hide] == true {
            continue;
        }
        let reset_at = unix(&status[reset]);
        if let Some(remaining) =
            scalar_number(&status[left]).or_else(|| (quota && reset_at.is_some()).then_some(0.0))
        {
            windows.push(QuotaWindow {
                label: label.into(),
                remaining_percent: remaining.clamp(0.0, 100.0),
                reset_at,
            });
        }
    }
    let balance = scalar_number(&status["overageBalanceMicros"])
        .filter(|v| *v >= 0.0)
        .map(|n| n / 1_000_000.0);
    let end = unix(&status["planEnd"]).or_else(|| {
        status["planEnd"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp())
    });
    (windows, balance, end)
}
pub(super) fn from_user(
    token: &str,
    server: &str,
    user: &Value,
    with_quota: bool,
) -> Result<(Credentials, Account), String> {
    validate(token, server)?;
    let user_id = user["userId"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("accountError.authResponse")?;
    let email = user["email"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| user["name"].as_str())
        .unwrap_or(user_id);
    let c = Credentials {
        token: token.into(),
        server: server.trim_end_matches('/').into(),
        user_id: user_id.into(),
        email: email.into(),
        user: user.clone(),
    };
    let (quota_windows, balance_usd, subscription_end_at) = if with_quota {
        quota(user)
    } else {
        (Vec::new(), None, None)
    };
    let plan = plan_name(&user["teamsTier"])
        .or_else(|| plan_name(&user["planStatus"]["planInfo"]["teamsTier"]))
        .or_else(|| plan_name(&user["planStatus"]["planInfo"]["planName"]));
    Ok((
        c.clone(),
        Account {
            id: id(&c),
            email: email.into(),
            plan,
            active: false,
            quota_windows,
            balance_usd,
            subscription_end_at,
        },
    ))
}
pub(crate) fn metadata(token: &str) -> Value {
    json!({"ideName":"devin-cli","ideVersion":VERSION,"extensionName":"devin-cli","extensionVersion":VERSION,"apiKey":token,"locale":"en","os":std::env::consts::OS})
}
pub(crate) async fn user_status(token: &str, server: &str) -> Result<Value, String> {
    validate(token, server)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network")?;
    let r = client
        .post(format!(
            "{}/exa.seat_management_pb.SeatManagementService/GetUserStatus",
            server.trim_end_matches('/')
        ))
        .header("Connect-Protocol-Version", "1")
        .json(&json!({"metadata":metadata(token)}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !r.status().is_success() {
        return Err(format!("accountError.auth|HTTP {}", r.status().as_u16()));
    }
    let value: Value = r.json().await.map_err(|_| "accountError.authResponse")?;
    value
        .get("userStatus")
        .cloned()
        .ok_or("accountError.authResponse".into())
}
fn save(store: &mut Store, c: Credentials, account: Account) -> Result<Account, String> {
    store.accounts.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            secret: encode(&c)?,
        },
    );
    Ok(account)
}
fn import_snapshot(store: &mut Store, c: Credentials, account: Account) -> Result<bool, String> {
    // A passive import must not replace credentials saved by an explicit sign-in.
    if store.accounts.contains_key(&account.id) || store.ignored.contains(&account.id) {
        return Ok(false);
    }
    save(store, c, account)?;
    Ok(true)
}
pub async fn list(tool_id: &str) -> Result<Vec<Account>, String> {
    // Saved accounts list without the client; its own files are read only when installed.
    check_tool_id(tool_id)?;
    let _guard = LOCK.lock().await;
    let mut store = load()?;
    let mut changed = false;
    let mut active = None;
    if super::tool_manager::get_tool_exe_path("devin").is_some() {
        // An unreadable native profile must not hide EchoBird's saved accounts.
        if let Ok(Some((c, a))) = native::read_desktop() {
            if tool_id == "devin" {
                active = Some(a.id.clone());
            }
            changed = import_snapshot(&mut store, c, a)?;
        }
    }
    if tool_id == "devincli" && super::tool_manager::any_tool_installed(&["devincli"]) {
        if let Ok(Some((token, server))) = native::cli_token() {
            active = store.accounts.iter().find_map(|(id, s)| {
                let c = decode(s).ok()?;
                (c.token == token && c.server == server.trim_end_matches('/')).then(|| id.clone())
            });
        }
    }
    if changed {
        write(&store)?;
    }
    Ok(store
        .accounts
        .into_values()
        .map(|s| {
            let mut a = s.account;
            a.active = active.as_deref() == Some(&a.id);
            a
        })
        .collect())
}
fn check_tool_id(tool: &str) -> Result<(), String> {
    if !matches!(tool, "devin" | "devincli") {
        return Err("accountError.invalidAccount".into());
    }
    Ok(())
}
fn check_tool(tool: &str) -> Result<(), String> {
    check_tool_id(tool)?;
    if super::tool_manager::get_tool_exe_path(tool).is_none() {
        return Err("accountError.initializeClient".into());
    }
    Ok(())
}
pub async fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let _guard = LOCK.lock().await;
    Ok(load()?.accounts.into_values().map(|s| s.account).collect())
}
pub async fn refresh(account_id: &str) -> Result<Account, String> {
    let c = {
        let _guard = LOCK.lock().await;
        let store = load()?;
        decode(
            store
                .accounts
                .get(account_id)
                .ok_or("accountError.invalidAccount")?,
        )?
    };
    let user = user_status(&c.token, &c.server).await?;
    let (updated, a) = from_user(&c.token, &c.server, &user, true)?;
    if a.id != account_id {
        return Err("accountError.invalidAccount".into());
    }
    let _guard = LOCK.lock().await;
    let mut store = load()?;
    let current = decode(
        store
            .accounts
            .get(account_id)
            .ok_or("accountError.cancelled")?,
    )?;
    if current.token != c.token {
        return Err("accountError.cancelled".into());
    }
    let a = save(&mut store, updated, a)?;
    write(&store)?;
    Ok(a)
}
pub async fn delete(account_id: &str) -> Result<(), String> {
    let _guard = LOCK.lock().await;
    let mut store = load()?;
    store.accounts.remove(account_id);
    store.ignored.insert(account_id.into());
    write(&store)
}
pub async fn switch(account_id: &str, tool_id: &str) -> Result<Account, String> {
    check_tool(tool_id)?;
    let _guard = LOCK.lock().await;
    let store = load()?;
    let saved = store
        .accounts
        .get(account_id)
        .ok_or("accountError.invalidAccount")?;
    let c = decode(saved)?;
    if tool_id == "devin" {
        super::process_manager::stop_desktop_for_account("devin").await?;
        native::apply_desktop(&c)?;
    } else {
        native::apply_cli(&c)?;
    }
    let mut account = saved.account.clone();
    account.active = true;
    Ok(account)
}
pub async fn proxy_credentials(account_id: &str) -> Result<Value, String> {
    let _guard = LOCK.lock().await;
    let store = load()?;
    let c = decode(
        store
            .accounts
            .get(account_id)
            .ok_or("accountError.invalidAccount")?,
    )?;
    Ok(json!({"accessToken":c.token,"baseUrl":c.server,"userId":c.user_id}))
}
pub async fn start_login(tool_id: &str) -> Result<LoginStart, String> {
    check_tool(tool_id)?;
    oauth::start().await
}
pub fn cancel_login(login_id: &str) -> Result<(), String> {
    oauth::cancel(login_id)
}
pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let _guard = LOCK.lock().await;
    oauth::complete(login_id, |c, a| {
        let mut store = load()?;
        store.ignored.remove(&a.id);
        let a = save(&mut store, c, a)?;
        write(&store)?;
        Ok(a)
    })
}

pub(crate) async fn import_native(cli_only: bool) -> Result<Option<bool>, String> {
    let candidate = if cli_only {
        let Some((token, server)) = native::cli_token()? else {
            return Ok(None);
        };
        // Resolve the opaque CLI key with the existing identity/status endpoint;
        // never query separate quota or refresh the source token.
        let user = user_status(&token, &server).await?;
        Some(from_user(&token, &server, &user, false)?)
    } else {
        native::read_desktop()?
    };
    let Some((c, account)) = candidate else {
        return Ok(None);
    };
    let _guard = LOCK.lock().await;
    let mut rows = load()?;
    if rows.accounts.values().any(|s| {
        super::auto_import::native::same_account(
            &account.id,
            &account.email,
            &s.account.id,
            &s.account.email,
        )
    }) {
        return Ok(Some(false));
    }
    let added = import_snapshot(&mut rows, c, account)?;
    if added {
        write(&rows)?;
    }
    Ok(Some(added))
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
    fn passive_native_snapshot_cannot_overwrite_reauthorization_or_restore_deleted_accounts() {
        let user = json!({"userId":"fixture-user","email":"fixture@example.test","teamsTier":19});
        let (native, account) =
            from_user("devin-session-token$old-native", SERVER, &user, false).unwrap();
        let mut store = Store::default();
        let mut refreshed = account.clone();
        refreshed.plan = Some("Max".into());
        refreshed.quota_windows.push(QuotaWindow {
            label: "1 day".into(),
            remaining_percent: 75.0,
            reset_at: None,
        });
        store.accounts.insert(
            account.id.clone(),
            Saved {
                account: refreshed,
                secret: "fixture-new-browser-credentials".into(),
            },
        );
        assert!(!import_snapshot(&mut store, native.clone(), account.clone()).unwrap());
        let saved = &store.accounts[&account.id];
        assert_eq!(saved.secret, "fixture-new-browser-credentials");
        assert_eq!(saved.account.plan.as_deref(), Some("Max"));
        assert_eq!(saved.account.quota_windows[0].remaining_percent, 75.0);
        store.accounts.clear();
        store.ignored.insert(account.id.clone());
        assert!(!import_snapshot(&mut store, native, account).unwrap());
        assert!(store.accounts.is_empty());
        assert_eq!(store.ignored.len(), 1);
    }
    #[test]
    fn five_current_plans_and_unknown_values() {
        for (n, s, name) in [
            (19, "TEAMS_TIER_DEVIN_FREE", "Free"),
            (16, "TEAMS_TIER_DEVIN_PRO", "Pro"),
            (17, "TEAMS_TIER_DEVIN_MAX", "Max"),
            (15, "TEAMS_TIER_DEVIN_TEAMS_V2", "Teams"),
            (12, "TEAMS_TIER_DEVIN_ENTERPRISE", "Enterprise"),
        ] {
            assert_eq!(plan_name(&json!(n)).as_deref(), Some(name));
            assert_eq!(plan_name(&json!(s)).as_deref(), Some(name));
        }
        assert_eq!(
            plan_name(&json!("Future Plan")).as_deref(),
            Some("Future Plan")
        );
        assert_eq!(plan_name(&json!(0)), None);
    }
    #[test]
    fn missing_quota_is_unknown_but_omitted_proto_zero_is_exhausted() {
        assert!(quota(&json!({})).0.is_empty());
        let (w, _, _) = quota(
            &json!({"planStatus":{"planInfo":{"billingStrategy":"BILLING_STRATEGY_QUOTA"},"dailyQuotaResetAtUnix":"123","weeklyQuotaRemainingPercent":42}}),
        );
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].remaining_percent, 0.0);
        assert_eq!(w[1].remaining_percent, 42.0);
    }
    #[test]
    fn stable_identity_does_not_depend_on_token_or_plan() {
        let u = json!({"userId":"fixture-user","email":"fixture@example.test","teamsTier":19});
        let (_, a) = from_user("devin-session-token$fixture-one", SERVER, &u, false).unwrap();
        let (_, b) = from_user("devin-session-token$fixture-two", SERVER, &u, false).unwrap();
        assert_eq!(a.id, b.id);
        assert!(from_user("invalid", SERVER, &u, false).is_err());
    }
}
