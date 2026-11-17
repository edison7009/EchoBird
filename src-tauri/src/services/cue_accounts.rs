//! Cue shares Manus authentication; membership and quota remain Cue-specific.
use super::account_login_browser::{self, Login};
use super::cursor_auth::{read, write};
use super::electron_storage::cipher;
use super::manus_accounts::{
    cookie_db, native_session, profile, profile_identity, saved_session, set_cookie,
    Account as ManusAccount, Credits,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static PENDING: std::sync::Mutex<Option<Login>> = std::sync::Mutex::new(None);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyQuota {
    pub remaining_percent: Option<f64>,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    #[serde(flatten)]
    base: ManusAccount,
    weekly: Option<WeeklyQuota>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginPoll {
    account: Option<Account>,
    awaiting_client_exit: bool,
    expires_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Owner {
    token_hash: String,
    user_id: String,
}
impl Owner {
    fn verified(token: &str, user_id: &str, account_id: &str) -> Result<Self, String> {
        let owner = Self {
            user_id: user_id.to_owned(),
            token_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
        };
        if !owner.matches(token, account_id) {
            return Err("accountError.authResponse".into());
        }
        Ok(owner)
    }
    fn matches(&self, token: &str, id: &str) -> bool {
        !token.is_empty()
            && !self.user_id.is_empty()
            && self.token_hash == format!("{:x}", Sha256::digest(token.as_bytes()))
            && id == format!("{:x}", Sha256::digest(self.user_id.as_bytes()))
    }
}
#[derive(Serialize, Deserialize)]
struct Saved {
    account: Account,
    session: String,
    owner: Owner,
}
type Store = BTreeMap<String, Saved>;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    login_id: String,
    expires_at: i64,
}

fn data_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    if super::tool_manager::get_tool_exe_path("cue").is_none() {
        if let Some(uri) = super::tool_manager::get_tool_launch_uri("cue") {
            if let Some(dir) = super::msix::roaming_data_dir(&uri, "Cue") {
                return Ok(dir);
            }
        }
    }
    dirs::config_dir()
        .map(|p| p.join("Cue"))
        .ok_or_else(|| "accountError.home".into())
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/cue-accounts.json"))
}
fn load_store() -> Result<Store, String> {
    let path = store_path()?;
    if path.exists() {
        read(&path)
    } else {
        Ok(Store::new())
    }
}
fn device_id(dir: &Path) -> Result<Option<String>, String> {
    let path = dir.join("device-id");
    if !path.exists() {
        return Ok(None);
    }
    let device = std::fs::read_to_string(path).map_err(|_| "accountError.read")?;
    if device.trim().is_empty() {
        return Err("accountError.initializeClient".into());
    }
    Ok(Some(device))
}

fn owner(dir: &Path) -> Result<Option<Owner>, String> {
    let path = dir.join("session-owner.json");
    if path.exists() {
        read(&path).map(Some)
    } else {
        Ok(None)
    }
}
fn save_owner(dir: &Path, value: Option<&Owner>) -> Result<(), String> {
    let path = dir.join("session-owner.json");
    if let Some(value) = value {
        write(&path, value)
    } else if path.exists() {
        std::fs::remove_file(path).map_err(|_| "accountError.write".into())
    } else {
        Ok(())
    }
}
fn write_native(dir: &Path, token: Option<&str>, next_owner: Option<&Owner>) -> Result<(), String> {
    let previous_owner = owner(dir)?;
    let mut db = cookie_db(dir, true)?;
    let tx = db.transaction().map_err(|_| "accountError.write")?;
    set_cookie(&tx, dir, token, cipher)?;
    save_owner(dir, next_owner)?;
    if tx.commit().is_err() {
        save_owner(dir, previous_owner.as_ref())?;
        return Err("accountError.write".into());
    }
    Ok(())
}

pub async fn list() -> Result<Vec<Account>, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    Ok(load_store()?
        .into_values()
        .map(|saved| saved.account)
        .collect())
}
pub async fn start_login(app: tauri::AppHandle) -> Result<LoginStart, String> {
    if !cfg!(any(windows, target_os = "macos")) {
        return Err("accountError.unavailable".into());
    }
    let login = Login::start_desktop(
        &app,
        "Cue",
        "cue",
        &[
            "manus.im",
            "google.com",
            "googleusercontent.com",
            "apple.com",
            "facebook.com",
        ],
    )?;
    let old = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .replace(login.clone());
    if let Some(old) = old {
        old.cancel(&app);
    }
    Ok(LoginStart {
        login_id: login.id,
        expires_at: login.expires,
    })
}
pub async fn cancel_login(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let login = {
        let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
        if pending.as_ref().is_some_and(|login| login.id == id) {
            pending.take()
        } else {
            None
        }
    };
    if let Some(login) = login {
        login.cancel(app);
    }
    Ok(())
}
pub async fn poll_login(app: &tauri::AppHandle, id: &str) -> Result<LoginPoll, String> {
    let login = PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .as_ref()
        .filter(|login| login.id == id)
        .cloned()
        .ok_or("accountError.cancelled")?;
    let Some(token) = login.desktop_token()? else {
        return Ok(LoginPoll::default());
    };
    let (mut account, user_id) = profile_identity(&token).await?;
    let owner = Owner::verified(&token, &user_id, &account.id)?;
    let session = account_login_browser::seal(&token)?;
    let _lock = ACCOUNT_LOCK.lock().await;
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    pending
        .as_ref()
        .filter(|current| current.id == id)
        .ok_or("accountError.cancelled")?;
    let result = login.complete(app, || {
        let mut store = load_store()?;
        let previous = store.get(&account.id);
        // UserInfo's plan belongs to Manus; only Cue membership responses populate these fields.
        account.plan = previous.and_then(|saved| saved.account.base.plan.clone());
        account.credits = previous.and_then(|saved| saved.account.base.credits.clone());
        account.subscription_end_at =
            previous.and_then(|saved| saved.account.base.subscription_end_at);
        account.active = previous.is_some_and(|saved| saved.account.base.active);
        let weekly = previous.and_then(|saved| saved.account.weekly.clone());
        let account = Account {
            base: account,
            weekly,
        };
        store.insert(
            account.base.id.clone(),
            Saved {
                account: account.clone(),
                session,
                owner,
            },
        );
        write(&store_path()?, &store)?;
        Ok(LoginPoll {
            account: Some(account),
            ..Default::default()
        })
    })?;
    *pending = None;
    Ok(result)
}

pub async fn switch(id: &str) -> Result<Account, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    if PENDING
        .lock()
        .map_err(|_| "accountError.busy")?
        .as_ref()
        .is_some_and(|p| p.check().is_ok())
    {
        return Err("accountError.busy".into());
    }
    let dir = data_dir()?;
    let mut store = load_store()?;
    let saved = store.get(id).ok_or("accountError.invalidAccount")?;
    let token = saved_session(&dir, &saved.session)?;
    if !saved.owner.matches(&token, id) || profile(&token).await?.id != id {
        return Err("accountError.invalidAccount".into());
    }
    super::process_manager::stop_desktop_for_account("cue").await?;
    let previous = native_session(&dir)?;
    let previous_owner = owner(&dir)?;
    write_native(&dir, Some(&token), Some(&saved.owner))?;
    for (key, saved) in &mut store {
        saved.account.base.active = key == id;
    }
    if let Err(error) = write(&store_path()?, &store) {
        write_native(&dir, previous.as_deref(), previous_owner.as_ref())
            .map_err(|rollback| format!("accountError.rollback|{error} {rollback}"))?;
        return Err(error);
    }
    Ok(store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone())
}

fn integer(value: &Value, key: &str) -> Result<Option<i64>, String> {
    value
        .get(key)
        .map(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
                .filter(|v| *v >= 0)
                .ok_or_else(|| "accountError.quota".into())
        })
        .transpose()
}

type Membership = (
    Option<String>,
    Option<Credits>,
    Option<WeeklyQuota>,
    Option<i64>,
);

fn membership(value: &Value, now_ms: i64) -> Result<Membership, String> {
    let membership = value
        .get("membership")
        .filter(|v| v.is_object())
        .ok_or("accountError.format")?;
    let plan = ["membershipVersion", "planKey"].iter().find_map(|key| {
        membership[*key]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .map(|plan| {
                match plan {
                    "agents_20_monthly" | "agents_20_yearly" => "plus",
                    "agents_100_monthly" | "agents_100_yearly" => "pro",
                    "agents_200_monthly" | "agents_200_yearly" => "max",
                    plan => plan,
                }
                .to_owned()
            })
    });
    let total = integer(value, "remainingCredits")?;
    let subscription_end_at = if matches!(plan.as_deref(), Some("plus" | "pro" | "max")) {
        integer(membership, "validUntilMs")?
            .filter(|time| *time > 0)
            .map(|time| time / 1000)
    } else {
        None
    };
    let limit = integer(value, "limitCredits")?;
    let start = integer(value, "windowStartMs")?;
    let end = integer(value, "windowEndMs")?;
    // Connect's protobuf JSON omits zero used/reserved credits. Purchased credits
    // are a separate bucket and must not increase the weekly percentage.
    let used = integer(value, "usedCredits")?.unwrap_or(0);
    let reserved = integer(value, "reservedCredits")?.unwrap_or(0);
    let weekly = limit.zip(start).zip(end).map(|((limit, start), end)| {
        let active = limit > 0 && start <= now_ms && now_ms < end;
        WeeklyQuota {
            remaining_percent: active.then(|| {
                (100.0 - (used as f64 + reserved as f64) / limit as f64 * 100.0).clamp(0.0, 100.0)
            }),
            reset_at: active.then_some(end / 1000),
        }
    });
    Ok((
        plan,
        total.map(|total| Credits {
            total,
            free: None,
            refresh: None,
            next_refresh_at: None,
        }),
        weekly,
        subscription_end_at,
    ))
}
pub async fn refresh(id: &str) -> Result<Account, String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    let dir = data_dir()?;
    let mut store = load_store()?;
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    let token = saved_session(&dir, &saved.session)?;
    let device = device_id(&dir)?.ok_or("accountError.initializeClient")?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "accountError.network")?
        .post("https://api.manus.im/user.v1.SubscriptionService/GetAgentsMembership")
        .bearer_auth(token)
        .header("Connect-Protocol-Version", "1")
        .header("x-client-id", device)
        .header("x-client-type", "desktop")
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err(format!("accountError.quota|HTTP {}", response.status()));
    }
    let (plan, credits, weekly, subscription_end_at) = membership(
        &response.json().await.map_err(|_| "accountError.format")?,
        chrono::Utc::now().timestamp_millis(),
    )?;
    saved.account.base.plan = plan;
    saved.account.base.credits = credits;
    saved.account.weekly = weekly;
    saved.account.base.subscription_end_at = subscription_end_at;
    let account = saved.account.clone();
    write(&store_path()?, &store)?;
    Ok(account)
}
pub async fn delete(id: &str) -> Result<(), String> {
    let _lock = ACCOUNT_LOCK.lock().await;
    let mut store = load_store()?;
    store.remove(id).ok_or("accountError.invalidAccount")?;
    write(&store_path()?, &store)
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let dir = data_dir()?;
    let Some(token) = native_session(&dir)? else {
        return Ok(None);
    };
    let (mut account, user) = profile_identity(&token).await?;
    account.active = false;
    account.credits = None;
    account.plan = None;
    account.subscription_end_at = None;
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut rows = load_store()?;
    if rows.values().any(|s| {
        super::auto_import::native::same_account(
            &account.id,
            &account.email,
            &s.account.base.id,
            &s.account.base.email,
        )
    }) {
        return Ok(Some(false));
    }
    if native_session(&dir)?.as_deref() != Some(&token) {
        return Err("accountError.invalidAccount".into());
    }
    rows.insert(
        account.id.clone(),
        Saved {
            owner: Owner::verified(&token, &user, &account.id)?,
            account: Account {
                base: account,
                weekly: None,
            },
            session: account_login_browser::seal(&token)?,
        },
    );
    write(&store_path()?, &rows)?;
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
    #[test]
    fn cue_plan_and_balance_use_cue_membership_not_manus_credits() {
        let (plan, credits, _, _) = membership(&serde_json::json!({
            "membership": {"membershipVersion":"pro"}, "remainingCredits":"1300", "usedCredits":"700"
        }), 0).unwrap();
        assert_eq!(plan.as_deref(), Some("pro"));
        assert_eq!(credits.unwrap().total, 1300);
        assert_eq!(
            membership(
                &serde_json::json!({"membership":{"planKey":"free"},"remainingCredits":0}),
                0
            )
            .unwrap()
            .1
            .unwrap()
            .total,
            0
        );
        assert!(membership(&serde_json::json!({"membership":{}}), 0)
            .unwrap()
            .1
            .is_none());
        for value in [
            serde_json::json!({}),
            serde_json::json!({"membership":{},"remainingCredits":-1}),
            serde_json::json!({"membership":{},"remainingCredits":"bad"}),
        ] {
            assert!(membership(&value, 0).is_err());
        }
    }
    #[test]
    fn cue_keeps_free_and_invitation_plus_distinct_from_paid_plans() {
        for (member, expected) in [
            (
                serde_json::json!({"membershipVersion":"free","source":"none"}),
                "free",
            ),
            (
                serde_json::json!({"membershipVersion":"plus","planKey":"agents_20_monthly","source":"invitation"}),
                "plus",
            ),
            (
                serde_json::json!({"membershipVersion":"pro","source":"paid"}),
                "pro",
            ),
            (
                serde_json::json!({"membershipVersion":"max","source":"paid"}),
                "max",
            ),
            (serde_json::json!({"planKey":"agents_100_yearly"}), "pro"),
            (serde_json::json!({"planKey":"agents_200_monthly"}), "max"),
        ] {
            assert_eq!(
                membership(&serde_json::json!({"membership":member}), 0)
                    .unwrap()
                    .0
                    .as_deref(),
                Some(expected)
            );
        }
        assert!(membership(&serde_json::json!({"membership":{}}), 0)
            .unwrap()
            .0
            .is_none());
    }
    #[test]
    fn cue_subscription_expiry_is_independent_of_weekly_reset_and_optional_for_invitation_plus() {
        for plan in ["plus", "pro", "max"] {
            let value = serde_json::json!({
                "membership":{"membershipVersion":plan,"validUntilMs":"1794132901689"},
                "windowStartMs":"1791540901689", "windowEndMs":"1792145701689",
                "limitCredits":"5000"
            });
            let (_, _, weekly, subscription) = membership(&value, 1791541000000).unwrap();
            assert_eq!(subscription, Some(1794132901));
            assert_eq!(weekly.unwrap().reset_at, Some(1792145701));
        }
        for member in [
            serde_json::json!({"membershipVersion":"free","validUntilMs":"1794132901689"}),
            serde_json::json!({"membershipVersion":"plus","source":"invitation"}),
            serde_json::json!({"membershipVersion":"plus","source":"invitation","validUntilMs":"0"}),
        ] {
            assert!(membership(&serde_json::json!({"membership":member}), 0)
                .unwrap()
                .3
                .is_none());
        }
        assert!(membership(
            &serde_json::json!({"membership":{"membershipVersion":"pro","validUntilMs":"invalid"}}),
            0
        )
        .is_err());
    }
    #[test]
    fn weekly_usage_counts_reserved_credits_but_excludes_purchased_balance() {
        let (_, _, weekly, _) = membership(&serde_json::json!({
            "membership":{"membershipVersion":"plus","source":"invitation"},
            "windowStartMs":"1791540901689", "windowEndMs":"1792145701689",
            "limitCredits":"5000", "usedCredits":"900", "reservedCredits":"100",
            "weeklyRemainingCredits":"4000", "purchasedRemainingCredits":"20000", "remainingCredits":"24000"
        }), 1791541000000).unwrap();
        let weekly = weekly.unwrap();
        assert_eq!(weekly.remaining_percent, Some(80.0));
        assert_eq!(weekly.reset_at, Some(1792145701));
    }
    #[test]
    fn weekly_usage_distinguishes_protobuf_zero_defaults_from_missing_or_expired_windows() {
        let mut value = serde_json::json!({"membership":{}, "windowStartMs":1000,"windowEndMs":604801000,"limitCredits":5000});
        assert_eq!(
            membership(&value, 1000)
                .unwrap()
                .2
                .unwrap()
                .remaining_percent,
            Some(100.0)
        );
        value["usedCredits"] = 5000.into();
        assert_eq!(
            membership(&value, 1000)
                .unwrap()
                .2
                .unwrap()
                .remaining_percent,
            Some(0.0)
        );
        value["reservedCredits"] = 100.into();
        assert_eq!(
            membership(&value, 1000)
                .unwrap()
                .2
                .unwrap()
                .remaining_percent,
            Some(0.0)
        );
        for now in [999, 604801000] {
            let weekly = membership(&value, now).unwrap().2.unwrap();
            assert!(weekly.remaining_percent.is_none());
            assert!(weekly.reset_at.is_none());
        }
        value["limitCredits"] = 0.into();
        assert!(membership(&value, 1000)
            .unwrap()
            .2
            .unwrap()
            .remaining_percent
            .is_none());
        for key in [
            "limitCredits",
            "usedCredits",
            "reservedCredits",
            "windowStartMs",
            "windowEndMs",
        ] {
            let mut malformed = value.clone();
            malformed[key] = "invalid".into();
            assert!(membership(&malformed, 1000).is_err());
        }
        assert!(membership(
            &serde_json::json!({"membership":{},"remainingCredits":123}),
            0
        )
        .unwrap()
        .2
        .is_none());
    }
    #[test]
    fn old_saved_cue_accounts_load_and_new_weekly_summaries_round_trip() {
        let value = serde_json::json!({
            "account":{"id":"cue","email":"cue@example.test","plan":"plus","active":true,"credits":null},
            "session":"encrypted-session", "owner":{"tokenHash":"hash","userId":"user"}
        });
        let mut saved: Saved = serde_json::from_value(value.clone()).unwrap();
        assert!(saved.account.weekly.is_none());
        assert!(saved.account.base.subscription_end_at.is_none());
        saved.account.weekly = Some(WeeklyQuota {
            remaining_percent: Some(80.0),
            reset_at: Some(1792145701),
        });
        saved.account.base.subscription_end_at = Some(1794132901);
        let encoded = serde_json::to_value(&saved).unwrap();
        assert_eq!(encoded["account"]["id"], "cue");
        assert_eq!(encoded["account"]["subscriptionEndAt"], 1794132901_i64);
        assert_eq!(
            serde_json::to_string(&saved)
                .unwrap()
                .matches("\"subscriptionEndAt\"")
                .count(),
            1
        );
        assert_eq!(encoded["session"], value["session"]);
        assert_eq!(encoded["owner"], value["owner"]);
        let decoded: Saved = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.account.base.subscription_end_at, Some(1794132901));
        assert_eq!(
            decoded.account.weekly.unwrap().remaining_percent,
            Some(80.0)
        );
    }
    #[test]
    fn owner_must_match_both_session_and_authenticated_account() {
        let id = format!("{:x}", Sha256::digest(b"user-one"));
        let owner = Owner {
            user_id: "user-one".into(),
            token_hash: format!("{:x}", Sha256::digest(b"session-one")),
        };
        assert!(owner.matches("session-one", &id));
        assert!(!owner.matches("session-two", &id));
        assert!(!owner.matches("session-one", "other-account"));
        assert!(Owner::verified("session-one", "user-one", &id)
            .unwrap()
            .matches("session-one", &id));
        assert!(Owner::verified("session-one", "other-user", &id).is_err());
        assert!(Owner::verified("", "user-one", &id).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn native_switch_changes_only_cue_session_and_owner_and_can_roll_back() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let dir = root.join("Cue");
        std::fs::create_dir_all(dir.join("Network")).unwrap();
        let manus = root.join("Manus");
        std::fs::create_dir(&manus).unwrap();
        std::fs::write(manus.join("session"), b"other-client-session").unwrap();
        let device_path = dir.join("device-id");
        std::fs::write(&device_path, "device-one").unwrap();
        std::fs::write(dir.join("settings.json"), b"unrelated-settings").unwrap();
        let old_owner = Owner {
            user_id: "old-user".into(),
            token_hash: format!("{:x}", Sha256::digest(b"old-token")),
        };
        save_owner(&dir, Some(&old_owner)).unwrap();
        let db = rusqlite::Connection::open(dir.join("Network/Cookies")).unwrap();
        db.execute_batch("CREATE TABLE cookies(host_key TEXT,name TEXT,path TEXT,value TEXT,encrypted_value BLOB); INSERT INTO cookies VALUES ('api.manus.im','session_id','/','old-token',x''); INSERT INTO cookies VALUES ('unrelated.example','preference','/','keep',x'');").unwrap();
        let new_owner = Owner {
            user_id: "new-user".into(),
            token_hash: format!("{:x}", Sha256::digest(b"new-token")),
        };
        write_native(&dir, Some("new-token"), Some(&new_owner)).unwrap();
        assert_eq!(native_session(&dir).unwrap().as_deref(), Some("new-token"));
        assert_eq!(owner(&dir).unwrap().unwrap().user_id, "new-user");
        write_native(&dir, Some("old-token"), Some(&old_owner)).unwrap();
        assert_eq!(native_session(&dir).unwrap().as_deref(), Some("old-token"));
        assert_eq!(owner(&dir).unwrap().unwrap().user_id, "old-user");
        assert_eq!(
            db.query_row(
                "SELECT value FROM cookies WHERE host_key='unrelated.example'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "keep"
        );
        assert_eq!(std::fs::read_to_string(device_path).unwrap(), "device-one");
        assert_eq!(
            std::fs::read(dir.join("settings.json")).unwrap(),
            b"unrelated-settings"
        );
        assert_eq!(
            std::fs::read(manus.join("session")).unwrap(),
            b"other-client-session"
        );
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }
}
