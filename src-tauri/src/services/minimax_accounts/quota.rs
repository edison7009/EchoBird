use super::{text, Account, Credentials, Quota, QuotaWindow};
use md5::{Digest, Md5};
use serde_json::{json, Value};

const SIGNING_KEY: &str = "I*7Cf%WZ#S&%1RlZJ&C2";

fn md5(value: &str) -> String {
    format!("{:x}", Md5::digest(value.as_bytes()))
}

fn component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub(super) fn signed(
    client: &reqwest::Client,
    host: &str,
    path: &str,
    credential: &Credentials,
    body: Option<Value>,
) -> Result<reqwest::RequestBuilder, String> {
    signed_request(client, host, path, credential, body, None)
}

pub(super) fn signed_desktop(
    client: &reqwest::Client,
    host: &str,
    path: &str,
    credential: &Credentials,
    body: Option<Value>,
    version: &str,
) -> Result<reqwest::RequestBuilder, String> {
    signed_request(client, host, path, credential, body, Some(version))
}

fn signed_request(
    client: &reqwest::Client,
    host: &str,
    path: &str,
    credential: &Credentials,
    body: Option<Value>,
    desktop_version: Option<&str>,
) -> Result<reqwest::RequestBuilder, String> {
    let now = chrono::Utc::now().timestamp_millis();
    let mut url = url::Url::parse(&format!("{host}{path}")).map_err(|_| "accountError.auth")?;
    let language = if credential.region == super::Region::Cn {
        "zh"
    } else {
        "en"
    };
    let timezone = chrono::Local::now().offset().local_minus_utc().to_string();
    let time = now.to_string();
    url.query_pairs_mut().extend_pairs([
        (
            "device_platform",
            if desktop_version.is_some() {
                "web"
            } else {
                "mcode"
            },
        ),
        ("biz_id", "3"),
        ("app_id", "3001"),
        ("version_code", "22201"),
        ("unix", &time),
        ("timezone_offset", &timezone),
        ("sys_language", language),
        ("lang", language),
        ("device_id", "0"),
        ("os_name", std::env::consts::OS),
        ("browser_name", "mcode"),
        ("user_id", &credential.user_id),
        ("client", "mcode"),
    ]);
    if let Some(version) = desktop_version {
        url.query_pairs_mut()
            .extend_pairs([("is_desktop", "1"), ("desktop_version", version)]);
    }
    let content = body.as_ref().map(Value::to_string);
    let json = content.as_deref().unwrap_or("{}");
    let timestamp = (now / 1000).to_string();
    let path = format!("{}?{}", url.path(), url.query().unwrap_or_default());
    let yy = md5(&format!("{}_{}{}ooui", component(&path), json, md5(&time)));
    let signature = md5(&format!(
        "{timestamp}{SIGNING_KEY}{}",
        content.as_deref().unwrap_or_default()
    ));
    let request = if content.is_some() {
        client.post(url)
    } else {
        client.get(url)
    };
    let request = request
        .bearer_auth(&credential.access_token)
        .header("content-type", "application/json")
        .header("accept", "application/json")
        .header("user-agent", "MiniMaxCode")
        .header("yy", yy)
        .header("x-timestamp", timestamp)
        .header("x-signature", signature);
    Ok(if let Some(content) = content {
        request.body(content)
    } else {
        request
    })
}

async fn response(response: reqwest::Response) -> Result<Value, String> {
    if matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    if value["statusInfo"]["code"] == 1000048 {
        return Err("accountError.loginRequired".into());
    }
    if value["statusInfo"]["code"]
        .as_i64()
        .is_some_and(|code| code != 0)
        || value["base_resp"]["status_code"]
            .as_i64()
            .is_some_and(|code| code != 0)
    {
        return Err("accountError.failed".into());
    }
    Ok(value)
}

async fn call(
    client: &reqwest::Client,
    host: &str,
    path: &str,
    credential: &Credentials,
    body: Option<Value>,
) -> Result<Value, String> {
    response(
        signed(client, host, path, credential, body)?
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await
}

pub(super) async fn identity_at(
    client: &reqwest::Client,
    host: &str,
    credential: &mut Credentials,
) -> Result<(), String> {
    let mut identity_request = credential.clone();
    identity_request.user_id = "0".into();
    let payload = call(client, host, "/v1/api/user/info", &identity_request, None).await?;
    let user = payload["data"]
        .get("userInfo")
        .or_else(|| payload["data"].get("user_info"))
        .or_else(|| payload.get("userInfo"))
        .or_else(|| payload.get("user_info"))
        .ok_or("accountError.authResponse")?;
    credential.user_id = text(user, "realUserID")
        .or_else(|| text(user, "real_user_id"))
        .ok_or("accountError.authResponse")?
        .into();
    credential.name = text(user, "name")
        .or_else(|| text(user, "userName"))
        .or_else(|| text(user, "user_name"))
        .map(str::to_string);
    credential.email = text(user, "userEmail")
        .or_else(|| text(user, "email"))
        .or_else(|| text(user, "userMail"))
        .or_else(|| text(user, "user_email"))
        .map(str::to_string)
        .or_else(|| credential.name.clone())
        .unwrap_or_else(|| credential.user_id.clone());
    Ok(())
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn seconds(value: &Value) -> Option<i64> {
    let value = number(value)?;
    (value > 0.0).then_some(if value >= 1e12 {
        (value / 1000.0) as i64
    } else {
        value as i64
    })
}

fn credit_amount(value: &Value) -> Option<f64> {
    credit_number(value).filter(|amount| *amount >= 0.0)
}

fn credit_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| {
            let value = value.as_str()?.replace(',', "");
            let value = value.trim();
            value
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                .then(|| value.parse::<f64>().ok())
                .flatten()
        })
        .filter(|amount| amount.is_finite())
}

fn membership(value: &Value, account: &mut Account) -> Option<String> {
    let data = value.get("data").unwrap_or(value);
    let field = |name: &str| value.get(name).or_else(|| data.get(name));
    if let Some(has) = field("has_token_plan").and_then(Value::as_bool) {
        if !has {
            account.subscription_end_at = None;
            if let Some(quota) = account.quota.as_mut() {
                quota.windows.clear();
            }
        }
        account.plan = Some(if has {
            field("token_plan_tier")
                .and_then(Value::as_str)
                .filter(|tier| !tier.is_empty() && !tier.bytes().all(|byte| byte.is_ascii_digit()))
                .map(|tier| format!("M Plan {tier}"))
                .unwrap_or_else(|| "M Plan".into())
        } else {
            "Free".into()
        });
    }
    if let Some(end) = field("token_plan_expires_at")
        .filter(|_| field("has_token_plan").and_then(Value::as_bool) != Some(false))
        .and_then(seconds)
    {
        account.subscription_end_at = Some(end);
    }
    let summary = field("op_credit_summary");
    let credits = summary
        .and_then(|summary| credit_amount(&summary["total_remaining_amount"]))
        .or_else(|| field("opcredit_balance").and_then(credit_amount));
    if let Some(credits) = credits {
        account.quota.get_or_insert_with(Quota::default).credits = Some(credits);
    }
    if let Some(summary) = summary.filter(|value| value.is_object()) {
        let quota = account.quota.get_or_insert_with(Quota::default);
        quota.purchased_credits = credit_amount(&summary["purchased_remaining_amount"])
            .or_else(|| credit_amount(&summary["purchased_credit"]));
        quota.granted_credits = credit_amount(&summary["free_remaining_amount"])
            .or_else(|| credit_amount(&summary["free_credit"]));
    }
    field("op_group_id")
        .and_then(Value::as_str)
        .filter(|group| !group.is_empty())
        .map(str::to_string)
}

pub(super) fn windows(value: &Value) -> Vec<QuotaWindow> {
    let mut windows = Vec::new();
    for model in value["model_remains"].as_array().into_iter().flatten() {
        let Some(name) = text(model, "model_name") else {
            continue;
        };
        for (prefix, label, start, end) in [
            ("current_interval", "5 hours", "start_time", "end_time"),
            (
                "current_weekly",
                "7 days",
                "weekly_start_time",
                "weekly_end_time",
            ),
        ] {
            let status = number(&model[format!("{prefix}_status")]);
            if status == Some(3.0) {
                continue;
            }
            let used = if status == Some(2.0) {
                Some(100.0)
            } else {
                number(&model[format!("{prefix}_remaining_percent")])
                    .map(|remaining| (100.0 - remaining).clamp(0.0, 100.0))
            };
            let Some(used_percent) = used else {
                continue;
            };
            let resets_at = seconds(&model[end]);
            let span_seconds = seconds(&model[start])
                .zip(resets_at)
                .and_then(|(start, end)| (end > start).then_some(end - start));
            windows.push(QuotaWindow {
                name: if name.eq_ignore_ascii_case("general") {
                    label.into()
                } else {
                    format!("{name} · {label}")
                },
                used_percent,
                resets_at,
                span_seconds,
            });
        }
    }
    windows
}

pub(super) async fn refresh_at(
    client: &reqwest::Client,
    agent: &str,
    platform: &str,
    details_host: &str,
    credential: &Credentials,
    account: &mut Account,
) -> Result<(), String> {
    if let Some(quota) = account.quota.as_mut() {
        quota.purchased_credits = None;
        quota.granted_credits = None;
        quota.purchased_total = None;
        quota.granted_total = None;
        quota.purchased_expires_at = None;
        quota.granted_expires_at = None;
    }
    let mut group = None;
    let extra = call(
        client,
        agent,
        "/matrix/api/v1/user/get_user_extra_info",
        credential,
        Some(json!({})),
    )
    .await;
    let workspace = match extra {
        Ok(extra) => extra
            .get("workspaces")
            .or_else(|| extra["data"].get("workspaces"))
            .and_then(Value::as_array)
            .and_then(|workspaces| {
                workspaces
                    .iter()
                    .find(|workspace| number(&workspace["workspace_type"]) == Some(0.0))
            })
            .cloned(),
        Err(error) if error == "accountError.loginRequired" => return Err(error),
        Err(_) => None,
    };
    if let Some(workspace) = &workspace {
        group = membership(workspace, account);
    }
    let body = workspace
        .as_ref()
        .and_then(|workspace| workspace.get("workspace_id"))
        .map(|id| json!({"workspace_id":id}))
        .unwrap_or(json!({}));
    match call(
        client,
        agent,
        "/matrix/api/v1/commerce/get_membership_info",
        credential,
        Some(body),
    )
    .await
    {
        Ok(info) => {
            group = membership(&info, account).or(group);
        }
        Err(error) if workspace.is_none() || error == "accountError.loginRequired" => {
            return Err(error)
        }
        Err(_) => {}
    }
    if account
        .plan
        .as_deref()
        .is_some_and(|plan| plan.starts_with("M Plan"))
    {
        if let Some(group) = group {
            let payload = response(
                client
                    .get(format!(
                        "{platform}/v1/api/openplatform/coding_plan/remains"
                    ))
                    .bearer_auth(&credential.access_token)
                    .header("x-group-id", group)
                    .header("accept", "application/json")
                    .send()
                    .await
                    .map_err(|_| "accountError.network")?,
            )
            .await?;
            account.quota.get_or_insert_with(Quota::default).windows = windows(&payload);
        }
    }
    if let Some(quota) = account.quota.as_mut() {
        if quota.purchased_credits.is_some() || quota.granted_credits.is_some() {
            if let Ok(details) = credit_details_at(client, details_host, credential).await {
                quota.purchased_total = category_total(&details.items, 1, quota.purchased_credits);
                quota.granted_total = category_total(&details.items, 2, quota.granted_credits);
                quota.purchased_expires_at =
                    category_expiry(&details.items, 1, quota.purchased_credits);
                quota.granted_expires_at =
                    category_expiry(&details.items, 2, quota.granted_credits);
            }
        }
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub(super) struct CreditDetail {
    pub credit_type: u8,
    pub granted: f64,
    pub remaining: f64,
    pub consumed: f64,
    pub granted_at: Option<i64>,
    pub expires_at: Option<i64>,
}

#[derive(serde::Serialize)]
pub(super) struct CreditDetails {
    pub items: Vec<CreditDetail>,
    pub pages: usize,
    pub total_count: usize,
}

fn millis(value: &Value) -> Option<i64> {
    use chrono::Datelike;
    let millis = number(value)?;
    if millis <= 0.0 || millis >= i64::MAX as f64 {
        return None;
    }
    chrono::DateTime::from_timestamp_millis(millis as i64)
        .filter(|date| date.year() < 9999)
        .map(|date| date.timestamp())
}

fn detail(value: &Value) -> Result<CreditDetail, String> {
    let credit_type = value["credit_type"]
        .as_u64()
        .filter(|kind| matches!(kind, 1 | 2))
        .ok_or("accountError.format")? as u8;
    Ok(CreditDetail {
        credit_type,
        granted: credit_amount(&value["granted_amount"]).ok_or("accountError.format")?,
        remaining: credit_number(&value["remaining_amount"])
            .map(|value| value.max(0.0))
            .ok_or("accountError.format")?,
        consumed: credit_amount(&value["consumed_amount"]).ok_or("accountError.format")?,
        granted_at: millis(&value["granted_at_ms"]),
        expires_at: millis(&value["expire_at_ms"]),
    })
}

pub(super) async fn credit_details_at(
    client: &reqwest::Client,
    host: &str,
    credential: &Credentials,
) -> Result<CreditDetails, String> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut items = Vec::new();
        let mut total = None;
        let mut seen = std::collections::HashSet::new();
        for page in 1..=64 {
            let path = format!("/minimax-cloud/api/v1/credit/details?page_number={page}&page_size=20&include_expired=false");
            let value = response(signed_desktop(
                client, host, &path, credential, None, env!("CARGO_PKG_VERSION"),
            )?.send().await.map_err(|_| "accountError.network")?).await?;
            let count = value["total_count"].as_u64()
                .and_then(|count| usize::try_from(count).ok())
                .ok_or("accountError.format")?;
            if total.is_some_and(|total| total != count) {
                return Err("accountError.format".into());
            }
            total = Some(count);
            let rows: &[Value] = match value.get("details") {
                Some(Value::Array(rows)) => rows,
                None | Some(Value::Null) if count == 0 => &[],
                _ => return Err("accountError.format".into()),
            };
            if count > 64 * 20 || rows.len() > 20 || items.len() + rows.len() > count
                || !rows.is_empty() && !seen.insert(serde_json::to_string(rows).map_err(|_| "accountError.format")?) {
                return Err("accountError.format".into());
            }
            for row in rows {
                items.push(detail(row)?);
            }
            if items.len() == count {
                return Ok(CreditDetails { items, pages: page, total_count: count });
            }
            if rows.is_empty() {
                return Err("accountError.format".into());
            }
        }
        Err("accountError.format".into())
    }).await.map_err(|_| "accountError.network")?
}

fn category_expiry(details: &[CreditDetail], kind: u8, balance: Option<f64>) -> Option<i64> {
    let balance = balance.filter(|balance| *balance > 0.0)?;
    let active: Vec<_> = details
        .iter()
        .filter(|detail| detail.credit_type == kind && detail.remaining > 0.0)
        .collect();
    let expiry = active.first()?.expires_at?;
    let now = chrono::Utc::now().timestamp();
    if expiry <= now
        || active.iter().any(|detail| {
            detail.expires_at != Some(expiry) || detail.granted_at.is_none_or(|at| at > expiry)
        })
    {
        return None;
    }
    let sum: f64 = active.iter().map(|detail| detail.remaining).sum();
    let rounding = f64::EPSILON * balance.max(1.0) * active.len() as f64;
    (sum.is_finite() && (sum - balance).abs() <= rounding).then_some(expiry)
}

fn category_total(details: &[CreditDetail], kind: u8, balance: Option<f64>) -> Option<f64> {
    let balance = balance?;
    let now = chrono::Utc::now().timestamp();
    let mut total = 0.0;
    let mut remaining = 0.0;
    let mut count = 0;
    for detail in details.iter().filter(|detail| detail.credit_type == kind) {
        let expiry = detail.expires_at?;
        if expiry <= now {
            continue;
        }
        if detail.granted_at.is_none_or(|at| at > expiry)
            || detail.remaining > detail.granted + f64::EPSILON * detail.granted.max(1.0)
        {
            return None;
        }
        total += detail.granted;
        remaining += detail.remaining;
        count += 1;
    }
    let rounding = f64::EPSILON * balance.max(1.0) * f64::from(count.max(1));
    (total.is_finite() && remaining.is_finite() && (remaining - balance).abs() <= rounding)
        .then_some(total)
}

#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;
