//! Isolated consumer sessions for Doubao Work and Dola.
mod api;
mod import;
pub(crate) use import::scan as import_native;
mod login;
mod native;
mod quota;
mod rewards;
pub(crate) mod speech;
pub use login::LoginStart;
pub use quota::{DailyReward, Quota};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, path::PathBuf};

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Edition {
    #[serde(rename = "doubaowork")]
    DoubaoWork,
    #[serde(rename = "dola")]
    Dola,
}
impl Edition {
    pub(crate) fn tool(self) -> &'static str {
        match self {
            Self::DoubaoWork => "doubaowork",
            Self::Dola => "dola",
        }
    }
    pub(crate) fn base(self) -> &'static str {
        match self {
            Self::DoubaoWork => "https://www.doubao.com",
            Self::Dola => "https://www.dola.com",
        }
    }
    fn domain(self) -> &'static str {
        match self {
            Self::DoubaoWork => "doubao.com",
            Self::Dola => "dola.com",
        }
    }
    fn title(self) -> &'static str {
        match self {
            Self::DoubaoWork => "豆包工作",
            Self::Dola => "Dola",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub edition: Edition,
    pub name: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub expires_at: Option<i64>,
    pub remaining: Option<f64>,
    pub total: Option<f64>,
    #[serde(default)]
    pub quota: Option<Quota>,
    #[serde(default)]
    pub daily_reward: Option<DailyReward>,
    pub active: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Cookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    expires: Option<i64>,
}
impl Cookie {
    fn valid(&self, edition: Edition, now: i64) -> bool {
        let domain = self.domain.trim_start_matches('.');
        (domain == edition.domain() || domain.ends_with(&format!(".{}", edition.domain())))
            && !self.name.is_empty()
            && !self
                .name
                .bytes()
                .any(|b| b.is_ascii_control() || b"=;, ".contains(&b))
            && !self
                .value
                .bytes()
                .any(|b| b.is_ascii_control() || b == b';')
            && self.path.starts_with('/')
            && self.expires.is_none_or(|expires| expires > now)
    }
    fn sent_to_base(&self, edition: Edition) -> bool {
        let host = format!("www.{}", edition.domain());
        let domain = self.domain.trim_start_matches('.');
        (domain == host || (self.domain.starts_with('.') && host.ends_with(&format!(".{domain}"))))
            && self.path == "/"
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Credentials {
    edition: Edition,
    user_id: String,
    enterprise_type: String,
    cookies: Vec<Cookie>,
}
impl Credentials {
    fn id(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}",
                self.edition.tool(),
                self.user_id,
                self.enterprise_type
            ))
        )
    }
    fn cookie_header(&self) -> Result<String, String> {
        let now = chrono::Utc::now().timestamp();
        let cookies: Vec<_> = self
            .cookies
            .iter()
            .filter(|cookie| cookie.valid(self.edition, now) && cookie.sent_to_base(self.edition))
            .collect();
        if !cookies.iter().any(|cookie| {
            matches!(cookie.name.as_str(), "sessionid" | "sessionid_ss") && !cookie.value.is_empty()
        }) {
            return Err("accountError.loginRequired".into());
        }
        Ok(cookies
            .into_iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; "))
    }
    fn validate(&self) -> Result<(), String> {
        if self.user_id.trim().is_empty()
            || self.cookies.is_empty()
            || self
                .cookies
                .iter()
                .any(|cookie| !cookie.valid(self.edition, i64::MIN))
        {
            return Err("accountError.invalidAccount".into());
        }
        self.cookie_header().map(|_| ())
    }
    fn metadata(&self, name: &str) -> Option<&str> {
        let now = chrono::Utc::now().timestamp();
        self.cookies
            .iter()
            .find(|cookie| {
                cookie.name == name
                    && cookie.valid(self.edition, now)
                    && cookie.sent_to_base(self.edition)
            })
            .map(|cookie| cookie.value.as_str())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    account: Account,
    credential: String,
}
type Store = BTreeMap<String, Saved>;
fn path(edition: Edition) -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(format!(".echobird/{}-accounts.json", edition.tool())))
}
fn load(path: &Path) -> Result<Store, String> {
    if !path.exists() {
        return Ok(Store::new());
    }
    super::cursor_auth::read(path)
}
fn credential(store: &Store, edition: Edition, id: &str) -> Result<Credentials, String> {
    let saved = store
        .get(id)
        .filter(|saved| saved.account.edition == edition && saved.account.id == id)
        .ok_or("accountError.invalidAccount")?;
    let c: Credentials =
        serde_json::from_str(&super::account_login_browser::unseal(&saved.credential)?)
            .map_err(|_| "accountError.keychain")?;
    c.validate()?;
    if c.edition != edition || c.id() != id {
        return Err("accountError.invalidAccount".into());
    }
    Ok(c)
}
fn protect(c: &Credentials) -> Result<String, String> {
    c.validate()?;
    super::account_login_browser::seal(
        &serde_json::to_string(c).map_err(|_| "accountError.format")?,
    )
}
pub async fn list(edition: Edition) -> Result<Vec<Account>, String> {
    let _guard = STORE_LOCK.lock().await;
    Ok(load(&path(edition)?)?
        .into_values()
        .filter(|saved| saved.account.edition == edition)
        .map(|saved| saved.account)
        .collect())
}
pub async fn start_login(app: tauri::AppHandle, edition: Edition) -> Result<LoginStart, String> {
    login::start(app, edition).await
}
pub fn cancel_login(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    login::cancel(app, id)
}
pub async fn poll_login(app: &tauri::AppHandle, id: &str) -> Result<Option<Account>, String> {
    let Some((c, mut account, login)) = login::poll(app, id).await? else {
        return Ok(None);
    };
    let _guard = STORE_LOCK.lock().await;
    login.complete(app, || {
        login::with_current(id, || {
            let path = path(c.edition)?;
            let mut store = load(&path)?;
            if let Some(previous) = store.get(&account.id) {
                account.active = previous.account.active;
                account.plan = previous.account.plan.clone();
                account.expires_at = previous.account.expires_at;
                account.remaining = previous.account.remaining;
                account.total = previous.account.total;
                account.quota = previous.account.quota.clone();
                account.daily_reward = previous.account.daily_reward.clone();
            }
            store.insert(
                account.id.clone(),
                Saved {
                    account: account.clone(),
                    credential: protect(&c)?,
                },
            );
            super::cursor_auth::write(&path, &store)?;
            Ok(Some(account))
        })
    })
}
pub async fn delete(edition: Edition, id: &str) -> Result<(), String> {
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    if !store
        .get(id)
        .is_some_and(|saved| saved.account.edition == edition)
    {
        return Err("accountError.invalidAccount".into());
    }
    store.remove(id);
    super::cursor_auth::write(&path, &store)
}
pub async fn refresh(edition: Edition, id: &str) -> Result<Account, String> {
    let _refresh = REFRESH_LOCK.lock().await;
    refresh_account(edition, id, false).await
}

pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
    let _refresh = REFRESH_LOCK.lock().await;
    refresh_account(edition, id, true).await
}

async fn refresh_account(edition: Edition, id: &str, claim: bool) -> Result<Account, String> {
    let (c, stamp, previous) = {
        let _guard = STORE_LOCK.lock().await;
        let store = load(&path(edition)?)?;
        let c = credential(&store, edition, id)?;
        let saved = store.get(id).ok_or("accountError.invalidAccount")?;
        (c, saved.credential.clone(), saved.account.clone())
    };
    let mut account = api::profile(&c).await?;
    if account.id != id {
        return Err("accountError.invalidAccount".into());
    }
    if claim {
        account.daily_reward = Some(rewards::claim(&c).await?);
        // Like DuMate and QwenWork, keep the claim receipt when a later balance
        // read fails; neither invent a new balance nor repeat a claimed task.
        if let Err(error) = api::refresh_quota(&c, &mut account).await {
            log::warn!("[DoubaoAccount] post-claim quota refresh failed: {error}");
            account.plan = previous.plan;
            account.quota = previous.quota;
        }
    } else {
        api::refresh_quota(&c, &mut account).await?;
        account.daily_reward = match rewards::status(&c).await {
            Ok(reward) => Some(reward),
            Err(error) => {
                log::warn!("[DoubaoAccount] daily status refresh failed: {error}");
                previous.daily_reward
            }
        };
    }
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    credential(&store, edition, id)?;
    if store.get(id).map(|saved| saved.credential.as_str()) != Some(stamp.as_str()) {
        return Err("accountError.invalidAccount".into());
    }
    let saved = store.get_mut(id).ok_or("accountError.invalidAccount")?;
    saved.account.name = account.name;
    saved.account.email = account.email;
    saved.account.plan = account.plan;
    saved.account.quota = account.quota;
    saved.account.daily_reward = account.daily_reward;
    let account = saved.account.clone();
    super::cursor_auth::write(&path, &store)?;
    Ok(account)
}
pub async fn apply(edition: Edition, id: &str) -> Result<Account, String> {
    static APPLY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _apply = APPLY_LOCK.lock().await;
    let (c, before) = {
        let _guard = STORE_LOCK.lock().await;
        let store = load(&path(edition)?)?;
        let c = credential(&store, edition, id)?;
        (
            c,
            store
                .get(id)
                .ok_or("accountError.invalidAccount")?
                .credential
                .clone(),
        )
    };
    native::apply(&c).await?;
    let _guard = STORE_LOCK.lock().await;
    let path = path(edition)?;
    let mut store = load(&path)?;
    credential(&store, edition, id)?;
    if store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .credential
        != before
    {
        return Err("accountError.invalidAccount".into());
    }
    for saved in store
        .values_mut()
        .filter(|saved| saved.account.edition == edition)
    {
        saved.account.active = saved.account.id == id;
    }
    let account = store
        .get(id)
        .ok_or("accountError.invalidAccount")?
        .account
        .clone();
    super::cursor_auth::write(&path, &store)?;
    Ok(account)
}
pub(crate) async fn proxy_credentials(edition: Edition, id: &str) -> Result<Value, String> {
    let _guard = STORE_LOCK.lock().await;
    let c = credential(&load(&path(edition)?)?, edition, id)?;
    credential_metadata(&c)
}

fn credential_metadata(c: &Credentials) -> Result<Value, String> {
    let metadata =
        super::tool_manager::doubao_desktop_metadata(c.edition.tool()).unwrap_or(Value::Null);
    Ok(
        json!({"accountId":c.id(),"baseUrl":c.edition.base(),"cookieHeader":c.cookie_header()?,"deviceId":c.metadata("device_id"),"webId":c.metadata("web_id"),"fingerprint":c.metadata("s_v_web_id").or_else(||c.metadata("fp")),"clientVersion":metadata["version"],"channel":metadata["channel"]}),
    )
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    serde_json::from_value::<Saved>(value.clone())
        .map(|_| ())
        .map_err(|_| "backupError.format".into())
}

#[cfg(test)]
mod tests;
