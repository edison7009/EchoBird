use super::{Account, Credentials, Edition};
use serde_json::Value;
use std::time::Duration;

pub(super) fn installed(edition: Edition) -> Result<(), String> {
    crate::services::tool_manager::get_tool_exe_path(edition.tool())
        .ok_or_else(|| "accountError.unavailable".to_owned())
        .map(|_| ())
}
fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key].as_str().filter(|text| !text.trim().is_empty())
}
pub(super) fn parse_profile(
    edition: Edition,
    value: &Value,
) -> Result<(Account, String, String), String> {
    if value["message"]
        .as_str()
        .is_some_and(|message| message != "success")
    {
        return Err("accountError.loginRequired".into());
    }
    let data = value
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(value);
    let user = text(data, "user_id_str").ok_or("accountError.loginRequired")?;
    let enterprise = data["passport_enterprise_user_type"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            data["passport_enterprise_user_type"]
                .as_i64()
                .map(|n| n.to_string())
        })
        .unwrap_or_else(|| "0".into());
    let c = Credentials {
        edition,
        user_id: user.into(),
        enterprise_type: enterprise.clone(),
        cookies: vec![],
    };
    Ok((
        Account {
            id: c.id(),
            edition,
            name: text(data, "name").unwrap_or(user).into(),
            email: text(data, "email").map(str::to_owned),
            plan: None,
            expires_at: None,
            remaining: None,
            total: None,
            quota: None,
            daily_reward: None,
            active: false,
        },
        user.into(),
        enterprise,
    ))
}
pub(super) async fn identity(c: &Credentials) -> Result<(Account, String, String), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|_| "accountError.network")?;
    let response = client
        .get(format!("{}/passport/account/info/v2/", c.edition.base()))
        .query(&[
            ("account_sdk_source", "web"),
            ("sdk_version", "2.2.5"),
            (
                "aid",
                match c.edition {
                    Edition::DoubaoWork => "582478",
                    Edition::Dola => "582465",
                },
            ),
            (
                "real_aid",
                match c.edition {
                    Edition::DoubaoWork => "582478",
                    Edition::Dola => "582465",
                },
            ),
            ("device_platform", "web"),
            ("samantha_web", "1"),
            ("use-olympus-account", "1"),
        ])
        .header("Cookie", c.cookie_header()?)
        .header("Agw-Js-Conv", "str")
        .header("Referer", format!("{}/", c.edition.base()))
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err(format!("accountError.quota|HTTP {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|_| "accountError.format")?;
    parse_profile(c.edition, &value)
}
pub(super) async fn profile(c: &Credentials) -> Result<Account, String> {
    identity(c).await.map(|value| value.0)
}

pub(super) async fn post(c: &Credentials, path: &str, body: &Value) -> Result<Value, String> {
    let provider = match c.edition {
        Edition::DoubaoWork => crate::models::model::AccountProvider::DoubaoWork,
        Edition::Dola => crate::models::model::AccountProvider::Dola,
    };
    super::super::account_proxy::doubao_account_json(
        provider,
        &super::credential_metadata(c)?,
        path,
        body,
    )
    .await
    .map_err(|error| match error.status.as_u16() {
        401 => "accountError.loginRequired".into(),
        _ => format!("accountError.quota|HTTP {}", error.status.as_u16()),
    })
}

pub(super) async fn refresh_quota(c: &Credentials, account: &mut Account) -> Result<(), String> {
    let overview = post(
        c,
        "/alice/commerce/sale/subscription/overview/",
        &serde_json::json!({"product_line":"membership"}),
    )
    .await?;
    let entry = post(
        c,
        "/alice/commerce/sale/subscription/entry/config/",
        &serde_json::json!({}),
    )
    .await?;
    account.plan = super::quota::parse_plan(&entry).or_else(|| super::quota::parse_plan(&overview));
    account.quota = Some(super::quota::parse_overview(&overview));
    Ok(())
}
