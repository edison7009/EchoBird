use super::{now, unix_seconds, QuotaWindow, Saved};
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde_json::{json, Value};

#[cfg(test)]
mod tests;

fn business_origin(provider: &str) -> Result<&'static str, String> {
    match provider {
        "bigmodel" => Ok("https://open.bigmodel.cn"),
        "zai" => Ok("https://api.z.ai"),
        _ => Err("accountError.invalidAccount".into()),
    }
}

async fn get_data(
    client: &reqwest::Client,
    url: &str,
    authorization: &str,
) -> Result<Value, String> {
    let response = client
        .get(url)
        .header("Authorization", authorization)
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err(format!("accountError.auth|HTTP {}", response.status()));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    if body["success"] == false
        || body
            .get("code")
            .is_some_and(|code| !code.is_null() && !matches!(code.as_i64(), Some(0 | 200)))
    {
        return Err("accountError.authResponse".into());
    }
    Ok(body["data"].clone())
}

fn date_seconds(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(date) = DateTime::parse_from_rfc3339(value) {
        return Some(date.timestamp());
    }
    if let Ok(date) =
        NaiveDateTime::parse_from_str(&value.replace('T', " "), "%Y-%m-%d %H:%M:%S%.f")
    {
        return Local
            .from_local_datetime(&date)
            .single()
            .map(|date| date.timestamp());
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp());
    }
    unix_seconds(&Value::String(value.into()))
}

fn subscription_end(subscription: &Value) -> Option<i64> {
    if subscription["autoRenew"] != true && subscription["autoRenew"] != 1 {
        if let Some(end) = subscription["nextRenewTime"]
            .as_str()
            .and_then(date_seconds)
        {
            return Some(end);
        }
    }
    let valid = subscription["valid"].as_str()?;
    valid.char_indices().rev().find_map(|(start, _)| {
        let end = &valid[start..];
        let date = end.get(..10)?;
        NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        date_seconds(end)
            .or_else(|| end.get(..19).and_then(date_seconds))
            .or_else(|| date_seconds(date))
    })
}

fn current_subscription(data: &Value) -> Result<Option<&Value>, String> {
    let list = data.as_array().ok_or("accountError.authResponse")?;
    let mut malformed = false;
    for item in list {
        let coding = ["productId", "productName"].iter().any(|key| {
            item[key]
                .as_str()
                .is_some_and(|s| s.to_lowercase().contains("coding"))
        });
        if !coding {
            continue;
        }
        if !item["status"].is_string() || !item["inCurrentPeriod"].is_boolean() {
            malformed = true;
            continue;
        }
        if item["status"] == "VALID"
            && item["inCurrentPeriod"] == true
            && subscription_end(item).map_or(true, |end| end > now())
        {
            return Ok(Some(item));
        }
    }
    if malformed {
        Err("accountError.authResponse".into())
    } else {
        Ok(None)
    }
}

pub(super) fn plan_label(name: &str) -> String {
    let name = name.trim();
    let lower = name.to_lowercase();
    if name.contains("团队")
        || name.contains("團隊")
        || lower
            .split(|ch: char| !ch.is_ascii_alphabetic())
            .any(|part| part == "team")
    {
        return "Team".into();
    }
    for part in name.split(|ch: char| !ch.is_ascii_alphabetic()) {
        match part.to_ascii_lowercase().as_str() {
            "lite" => return "Lite".into(),
            "pro" => return "Pro".into(),
            "max" => return "Max".into(),
            _ => {}
        }
    }
    if lower.contains("start plan")
        || lower.contains("trial")
        || lower.contains("trust build")
        || name.contains("体验")
        || name.contains("體驗")
    {
        "Trial".into()
    } else {
        name.into()
    }
}

fn personal_project(customer: &Value) -> Option<(&str, &str)> {
    let organizations = customer["organizations"].as_array()?;
    let candidates: Vec<_> = organizations
        .iter()
        .filter_map(|org| {
            let organization_id = org["organizationId"].as_str()?;
            let projects: Vec<_> = org["projects"]
                .as_array()?
                .iter()
                .filter(|p| {
                    p["projectType"] != 2 && p["projectType"] != "2" && p["projectId"].is_string()
                })
                .collect();
            let project = projects
                .iter()
                .find(|p| {
                    p["projectName"]
                        .as_str()
                        .is_some_and(|s| s.contains("默认项目"))
                })
                .or_else(|| projects.first())?;
            Some((org, organization_id, project["projectId"].as_str()?))
        })
        .collect();
    candidates
        .iter()
        .find(|(org, _, _)| {
            org["organizationName"]
                .as_str()
                .is_some_and(|s| s.contains("默认机构"))
        })
        .or_else(|| candidates.first())
        .map(|(_, org, project)| (*org, *project))
}

async fn existing_usage_key(
    client: &reqwest::Client,
    origin: &str,
    authorization: &str,
    provider: &str,
) -> Result<Option<String>, String> {
    let customer = get_data(
        client,
        &format!("{origin}/api/biz/customer/getCustomerInfo"),
        authorization,
    )
    .await?;
    let Some((organization, project)) = personal_project(&customer) else {
        return Ok(None);
    };
    let mut url = url::Url::parse(origin).map_err(|_| "accountError.authResponse")?;
    url.path_segments_mut()
        .map_err(|_| "accountError.authResponse")?
        .extend([
            "api",
            "biz",
            "v1",
            "organization",
            organization,
            "projects",
            project,
            "api_keys",
        ]);
    let keys = get_data(client, url.as_str(), authorization).await?;
    let keys = keys.as_array().ok_or("accountError.authResponse")?;
    let key = keys
        .iter()
        .find(|key| key["name"] == "zcode-api-key")
        .and_then(|key| key["apiKey"].as_str())
        .filter(|key| !key.trim().is_empty());
    let Some(key) = key else {
        return Ok(None);
    };
    url.path_segments_mut()
        .map_err(|_| "accountError.authResponse")?
        .extend(["copy", key]);
    let secret = get_data(client, url.as_str(), authorization).await?;
    match secret["secretKey"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(secret) => Ok(Some(format!("{key}.{secret}"))),
        None if provider == "bigmodel" => Ok(Some(key.into())),
        None => Ok(None),
    }
}

fn apply_paid(saved: &mut Saved, subscription: &Value, quota: Option<Value>) -> Result<(), String> {
    let name = subscription["productName"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| subscription["productId"].as_str())
        .ok_or("accountError.authResponse")?;
    saved.plan = Some(plan_label(name));
    saved.subscription_end_at = subscription_end(subscription);
    saved.quota_windows = match quota {
        Some(data) => {
            if !data["limits"].is_array() {
                return Err("accountError.authResponse".into());
            }
            super::super::usage_providers::zhipu::parse_response(&json!({"data": data}))
                .data
                .map(|data| {
                    data.quotas
                        .into_iter()
                        .map(|quota| QuotaWindow {
                            remaining_percent: 100.0 - quota.percentage,
                            reset_at: (quota.reset_at > 0).then_some(quota.reset_at / 1000),
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        None => Vec::new(),
    };
    saved.remaining_percent = saved.quota_windows.first().map(|w| w.remaining_percent);
    saved.reset_at = saved.quota_windows.first().and_then(|w| w.reset_at);
    Ok(())
}

pub(super) async fn refresh(saved: &mut Saved) -> Result<(), String> {
    let client = super::http()?;
    let origin = business_origin(&saved.provider)?;
    let authorization = if saved.provider == "zai" {
        format!("Bearer {}", saved.access_token)
    } else {
        saved.access_token.clone()
    };
    let subscriptions = get_data(
        &client,
        &format!("{origin}/api/biz/subscription/list"),
        &authorization,
    )
    .await?;
    if let Some(subscription) = current_subscription(&subscriptions)? {
        let quota =
            match existing_usage_key(&client, origin, &authorization, &saved.provider).await? {
                Some(key) => Some(
                    get_data(
                        &client,
                        &format!("{origin}/api/monitor/usage/quota/limit"),
                        &key,
                    )
                    .await?,
                ),
                None => None,
            };
        return apply_paid(saved, subscription, quota);
    }
    let data = super::envelope(
        client
            .get(super::BALANCE_URL)
            .query(&[("app_version", "3.14.3")])
            .bearer_auth(&saved.jwt_token)
            .header("x-device-mid", super::device_mid()?)
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    super::apply_balance(saved, &data);
    Ok(())
}
