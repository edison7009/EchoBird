//! EchoBird-owned DeepSeek accounts; only summaries cross the Tauri boundary.
use super::tool_config_manager::dsh;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const ORIGIN: &str = "https://platform.deepseek.com";
pub(super) static ACCOUNT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Balance {
    pub currency: String,
    pub amount: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    pub balances: Option<Vec<Balance>>,
    pub active: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Saved {
    pub summary: Account,
    pub token: String,
    pub device: String,
}

pub(super) fn store() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/deepseek-accounts"))
}

fn account_path(dir: &Path, id: &str) -> Result<PathBuf, String> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(dir.join(format!("{id}.yaml")))
}

pub(super) fn save_at(dir: &Path, saved: &Saved) -> Result<(), String> {
    let document = serde_yaml_ng::to_value(saved).map_err(|_| "accountError.format")?;
    dsh::write_private_yaml(&account_path(dir, &saved.summary.id)?, &document)
}

fn load_at(dir: &Path, id: &str) -> Result<Saved, String> {
    let saved: Saved = serde_yaml_ng::from_slice(
        &fs::read(account_path(dir, id)?).map_err(|_| "accountError.read")?,
    )
    .map_err(|_| "accountError.format")?;
    if saved.summary.id != id
        || !valid_token(&saved.token)
        || uuid::Uuid::parse_str(&saved.device).is_err()
    {
        return Err("accountError.invalidAccount".into());
    }
    Ok(saved)
}

pub(super) fn valid_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

pub(super) fn snapshot(profile: &Value, token: String, device: String) -> Result<Saved, String> {
    let uid = profile["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    if !valid_token(&token) {
        return Err("accountError.authResponse".into());
    }
    let name = [
        &profile["id_profile"]["name"],
        &profile["mobile"],
        &profile["mobile_number"],
        &profile["email"],
    ]
    .into_iter()
    .filter_map(Value::as_str)
    .find(|s| !s.trim().is_empty())
    .unwrap_or(uid);
    Ok(Saved {
        summary: Account {
            id: format!("{:x}", Sha256::digest(format!("{ORIGIN}:{uid}"))),
            name: name.into(),
            balances: None,
            active: false,
        },
        token,
        device,
    })
}

pub(super) fn client(locale: &str) -> Result<reqwest::Client, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    let platform = if cfg!(windows) {
        "desktop-win"
    } else if cfg!(target_os = "macos") {
        "desktop-mac"
    } else {
        "web"
    };
    for (key, value) in [
        ("x-client-platform", platform.to_string()),
        ("x-client-version", env!("CARGO_PKG_VERSION").into()),
        (
            "x-client-locale",
            if locale.starts_with("zh") {
                "zh_CN"
            } else {
                "en_US"
            }
            .into(),
        ),
        (
            "x-client-timezone-offset",
            chrono::Local::now().offset().local_minus_utc().to_string(),
        ),
        ("x-client-bundle-id", String::new()),
    ] {
        headers.insert(
            reqwest::header::HeaderName::from_static(key),
            value.parse().map_err(|_| "accountError.auth")?,
        );
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}

pub(super) async fn response(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let mut result = request.send().await.map_err(|_| "accountError.network")?;
    if result.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("accountError.loginRequired".into());
    }
    if !result.status().is_success() {
        return Err(format!(
            "accountError.network|HTTP {}",
            result.status().as_u16()
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = result.chunk().await.map_err(|_| "accountError.network")? {
        if body.len() + chunk.len() > 65_536 {
            return Err("accountError.authResponse".into());
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|_| "accountError.authResponse")?;
    unpack(value)
}

fn unpack(value: Value) -> Result<Value, String> {
    if value["code"].as_i64() == Some(40003) {
        return Err("accountError.loginRequired".into());
    }
    if value["code"].as_i64() != Some(0) || value["data"]["biz_code"].as_i64() != Some(0) {
        return Err("accountError.authResponse".into());
    }
    value["data"]
        .get("biz_data")
        .cloned()
        .ok_or("accountError.authResponse".into())
}

pub(super) async fn profile(client: &reqwest::Client, token: &str) -> Result<Value, String> {
    response(
        client
            .get(format!("{ORIGIN}/auth-api/v0/users/current"))
            .header("x-dsh-auth-token", token),
    )
    .await
}

fn balances(value: &Value) -> Result<Vec<Balance>, String> {
    let mut result: Vec<Balance> = Vec::new();
    for key in ["normal_wallets", "bonus_wallets"] {
        for wallet in value[key].as_array().ok_or("accountError.authResponse")? {
            let currency = wallet["currency"]
                .as_str()
                .filter(|c| matches!(*c, "CNY" | "USD"))
                .ok_or("accountError.authResponse")?;
            let amount = wallet["balance"]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .ok_or("accountError.authResponse")?;
            if let Some(balance) = result.iter_mut().find(|b| b.currency == currency) {
                balance.amount += amount;
            } else {
                result.push(Balance {
                    currency: currency.into(),
                    amount,
                });
            }
        }
    }
    if result.iter().any(|v| !v.amount.is_finite()) {
        return Err("accountError.authResponse".into());
    }
    result.sort_by(|a, b| a.currency.cmp(&b.currency));
    Ok(result)
}

pub fn list() -> Result<Vec<Account>, String> {
    let dir = store()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let _guard = dsh::CONFIG_LOCK.lock().map_err(|_| "accountError.busy")?;
    let active = super::tool_manager::any_tool_installed(&["dsh"])
        .then(|| dsh::active_account_token(&dsh::dsh_config_dir()))
        .flatten();
    let mut accounts = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|_| "accountError.read")? {
        let path = entry.map_err(|_| "accountError.read")?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let mut saved = load_at(&dir, id)?;
        saved.summary.active = active.as_deref() == Some(&saved.token);
        accounts.push(saved.summary);
    }
    accounts.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(accounts)
}

pub async fn refresh(id: &str, locale: &str) -> Result<Account, String> {
    let mut saved = load_at(&store()?, id)?;
    let response = response(
        client(locale)?
            .get(format!("{ORIGIN}/api/v0/users/get_user_summary"))
            .header("x-dsh-auth-token", &saved.token),
    )
    .await?;
    saved.summary.balances = Some(balances(&response)?);
    let _guard = ACCOUNT_LOCK.lock().await;
    // A deleted/re-authenticated account must not be resurrected by a slow refresh.
    let current = load_at(&store()?, id)?;
    if current.token != saved.token {
        return Ok(current.summary);
    }
    save_at(&store()?, &saved)?;
    let _config = dsh::CONFIG_LOCK.lock().map_err(|_| "accountError.busy")?;
    saved.summary.active =
        dsh::active_account_token(&dsh::dsh_config_dir()).as_deref() == Some(&saved.token);
    Ok(saved.summary)
}

pub async fn delete(id: &str) -> Result<(), String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    // Deleting an EchoBird entry does not revoke the session used by DSH.
    fs::remove_file(account_path(&store()?, id)?).map_err(|_| "accountError.write".into())
}

pub async fn switch(id: &str, locale: &str) -> Result<Account, String> {
    let _guard = ACCOUNT_LOCK.lock().await;
    let mut saved = load_at(&store()?, id)?;
    let verified = snapshot(
        &profile(&client(locale)?, &saved.token).await?,
        saved.token.clone(),
        saved.device.clone(),
    )?;
    if verified.summary.id != id {
        return Err("accountError.invalidAccount".into());
    }
    super::process_manager::stop_desktop_for_account("dsh").await?;
    let _config = dsh::CONFIG_LOCK.lock().map_err(|_| "accountError.busy")?;
    dsh::apply_account_at(&dsh::dsh_config_dir(), &saved.token, &saved.device)?;
    saved.summary.active = true;
    Ok(saved.summary)
}

pub(crate) fn list_saved_accounts() -> Result<Vec<Account>, String> {
    let dir = store()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut rows = vec![];
    for entry in fs::read_dir(&dir).map_err(|_| "accountError.read")? {
        let path = entry.map_err(|_| "accountError.read")?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("accountError.format")?;
        rows.push(load_at(&dir, id)?.summary);
    }
    Ok(rows)
}

pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let home = dsh::dsh_config_dir();
    let creds = {
        let _guard = dsh::CONFIG_LOCK.lock().map_err(|_| "accountError.busy")?;
        dsh::credentials_at(&home)?
    };
    let payload = &creds["records"]["deepseek-account-platform/default"]["payload"];
    if payload["issuer"] != ORIGIN {
        return Ok(None);
    }
    let Some(token) = payload["token"].as_str().filter(|s| valid_token(s)) else {
        return Ok(None);
    };
    let device = creds["records"]["deepseek-account-platform/device"]["payload"]["id"]
        .as_str()
        .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        .ok_or("accountError.invalidAccount")?;
    let profile = profile(&client("en")?, token).await?;
    let saved = snapshot(&profile, token.into(), device.into())?;
    let _guard = ACCOUNT_LOCK.lock().await;
    if list()?.iter().any(|a| {
        super::auto_import::native::same_account(
            &saved.summary.id,
            &saved.summary.name,
            &a.id,
            &a.name,
        )
    }) {
        return Ok(Some(false));
    }
    save_at(&store()?, &saved)?;
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
    use serde_json::json;
    #[test]
    fn sums_wallets_by_currency_without_inventing_quota_percentages() {
        let got = balances(&json!({"normal_wallets":[{"currency":"CNY","balance":"1.25"},{"currency":"USD","balance":"0E-16"}],"bonus_wallets":[{"currency":"CNY","balance":"2.5"}]})).unwrap();
        assert_eq!(
            got,
            vec![
                Balance {
                    currency: "CNY".into(),
                    amount: 3.75
                },
                Balance {
                    currency: "USD".into(),
                    amount: 0.0
                }
            ]
        );
        assert!(balances(
            &json!({"normal_wallets":[],"bonus_wallets":[{"currency":"USD","balance":"NaN"}]})
        )
        .is_err());
        assert!(balances(&json!({})).is_err());
    }
    #[test]
    fn stable_identity_and_secret_free_summary() {
        let profile = json!({"id":"user-1", "email":"a@example.test"});
        let a = snapshot(
            &profile,
            "secret-one".into(),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        let b = snapshot(
            &profile,
            "secret-two".into(),
            uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
        assert_eq!(a.summary.id, b.summary.id);
        assert!(!serde_json::to_string(&a.summary)
            .unwrap()
            .contains("secret"));
        assert!(snapshot(&json!({"email":"masked"}), "t".into(), "d".into()).is_err());
        assert!(account_path(Path::new("."), "../outside").is_err());
        assert_eq!(
            unpack(json!({"code":40003})).unwrap_err(),
            "accountError.loginRequired"
        );
    }
}
