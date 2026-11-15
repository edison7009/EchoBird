use super::{authorized, client, response, success, text, Edition, Saved};
use chrono::{Local, TimeZone};
use serde_json::{json, Value};

pub(super) struct Quota {
    pub remaining: f64,
    pub total: f64,
    pub expires_at: Option<i64>,
    pub base_remaining: Option<f64>,
    pub base_total: Option<f64>,
    pub base_reset_at: Option<i64>,
    pub reward_remaining: Option<f64>,
    pub reward_total: Option<f64>,
    pub reward_next_expiry_at: Option<i64>,
    pub addon_remaining: Option<f64>,
    pub plan: Option<String>,
}

pub(super) fn timestamp(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    if let Some(number) = number(value) {
        return Some(if number.abs() > 10_000_000_000.0 {
            (number / 1000.0) as i64
        } else {
            number as i64
        });
    }
    let raw = value.as_str()?;
    if let Ok(date) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Some(date.timestamp());
    }
    chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f")
        .ok()
        .or_else(|| {
            chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d")
                .ok()?
                .and_hms_opt(23, 59, 59)
        })
        .and_then(|date| Local.from_local_datetime(&date).single())
        .map(|date| date.timestamp())
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse::<f64>().ok())
        .filter(|v| v.is_finite())
}
fn amount(v: &Value, suffix: &str) -> Option<f64> {
    let alias = match suffix {
        "Size" => "CycleTotalCapacity",
        "Remain" => "CycleRemainCapacity",
        _ => "CycleUsedCapacity",
    };
    for prefix in ["CycleCapacity", "Capacity", "SlicePeriodCapacity"] {
        for ending in [format!("{suffix}Precise"), suffix.to_string()] {
            if let Some(n) = v.get(format!("{prefix}{ending}")).and_then(number) {
                return Some(n.max(0.0));
            }
        }
        if prefix == "CycleCapacity" {
            if let Some(n) = v.get(alias).and_then(number) {
                return Some(n.max(0.0));
            }
        }
    }
    None
}
fn resources<'a>(body: &'a Value, kind: &str) -> Option<&'a Vec<Value>> {
    for prefix in [
        "/data",
        "/data/data",
        "/data/Response/Data",
        "/data/data/Response/Data",
    ] {
        for key in [kind.to_string(), kind.to_lowercase()] {
            if let Some(items) = body
                .pointer(&format!("{prefix}/{key}"))
                .and_then(Value::as_array)
            {
                return Some(items);
            }
        }
    }
    None
}
fn resource_amount(item: &Value, suffix: &str) -> Option<f64> {
    amount(item, suffix).or_else(|| {
        item.get("SlicePeriodUsageDetails")
            .or_else(|| item.get("slicePeriodUsageDetails"))?
            .as_array()?
            .first()
            .and_then(|slice| amount(slice, suffix))
    })
}
fn package_expiry(v: &Value) -> Option<i64> {
    [
        "PackageEndTime",
        "packageEndTime",
        "DeductionEndTime",
        "deductionEndTime",
        "ExpiredTime",
        "expiredTime",
    ]
    .iter()
    .find_map(|k| timestamp(v.get(k)))
}
fn expiry(v: &Value, now: i64) -> Option<i64> {
    let deduction = package_expiry(v);
    let cycle = timestamp(v.get("CycleEndTime").or_else(|| v.get("cycleEndTime")));
    let result = match (deduction, cycle) {
        (Some(a), Some(b)) if a.saturating_sub(b) > 365 * 86400 => Some(b),
        (Some(a), _) => Some(a),
        (_, b) => b,
    };
    result.filter(|v| v.saturating_sub(now) <= 730 * 86400)
}
fn summarize(items: &[Value], now: i64) -> (f64, f64, Option<i64>) {
    let mut remaining = 0.0;
    let mut total = 0.0;
    let mut expires = None;
    for item in items {
        let end = expiry(item, now);
        if end.is_some_and(|v| v <= now) {
            continue;
        }
        let get = |key| resource_amount(item, key);
        let capacity = get("Size");
        let left = get("Remain");
        let used = get("Used");
        let capacity = capacity
            .or_else(|| left.zip(used).map(|(a, b)| a + b))
            .or(left)
            .or(used)
            .unwrap_or(0.0);
        let left = left.unwrap_or_else(|| (capacity - used.unwrap_or(0.0)).max(0.0));
        total += capacity;
        remaining += left;
        if left > 0.0 {
            if let Some(end) = end {
                expires = Some(expires.map_or(end, |v: i64| v.min(end)));
            }
        }
    }
    (remaining, total, expires)
}

fn summary_field<'a>(body: &'a Value, key: &str) -> Option<&'a Value> {
    [
        "/data",
        "/data/data",
        "/data/Response/Data",
        "/data/data/Response/Data",
    ]
    .iter()
    .find_map(|prefix| body.pointer(&format!("{prefix}/{key}")))
}

fn domestic_tier(name: &str) -> Option<(u8, &'static str)> {
    if name.contains("企业") || name.contains("團隊") || name.contains("团队") {
        return None;
    }
    if name.contains("旗舰") || name.contains("旗艦") {
        Some((4, "旗舰版"))
    } else if name.contains("高级") || name.contains("高級") {
        Some((3, "高级版"))
    } else if name.contains("标准") || name.contains("標準") || name.contains("专业") {
        Some((2, "标准版"))
    } else if name.contains("体验") || name.contains("體驗") || name.contains("免费") {
        Some((0, "体验版"))
    } else {
        None
    }
}

fn domestic_code_tier(code: &str) -> Option<(u8, &'static str)> {
    match code {
        "TCACA_code_001_PqouKr6QWV" | "TCACA_code_006_DbXS0lrypC" => Some((0, "体验版")),
        "TCACA_code_003_FAnt7lcmRT" => Some((2, "标准版")),
        _ => None,
    }
}

fn subscription_plan(
    items: &[Value],
    summary: Option<&Value>,
    now: i64,
    edition: Edition,
) -> Option<String> {
    let subscription = summary
        .and_then(|v| summary_field(v, "SubscriptionPackageCode"))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty());
    let paid = summary
        .and_then(|v| summary_field(v, "IsPaidUser"))
        .and_then(Value::as_bool);
    if edition == Edition::Cn {
        if let Some(tier) = summary
            .and_then(|v| summary_field(v, "SubscriptionPackageName"))
            .and_then(Value::as_str)
            .and_then(domestic_tier)
            .or_else(|| subscription.and_then(domestic_code_tier))
        {
            return Some(tier.1.to_string());
        }
    }
    let plan = items
        .iter()
        .filter_map(|item| {
            if expiry(item, now).is_some_and(|end| end <= now)
                || timestamp(
                    item.get("DeductionStartTime")
                        .or_else(|| item.get("deductionStartTime")),
                )
                .is_some_and(|start| start > now)
                || item
                    .get("Status")
                    .or_else(|| item.get("status"))
                    .and_then(Value::as_i64)
                    .is_some_and(|status| !matches!(status, 0 | 3))
            {
                return None;
            }
            let code = text(item, "PackageCode").or_else(|| text(item, "packageCode"));
            if subscription.is_some() && code != subscription {
                return None;
            }
            if edition == Edition::Cn {
                if let Some(tier) = code.and_then(domestic_code_tier) {
                    return Some(tier);
                }
            }
            let name = text(item, "PackageName")
                .or_else(|| text(item, "packageName"))?
                .to_lowercase();
            // Credit top-ups and promotional gifts do not establish a subscription tier.
            if [
                "top-up", "top up", "addon", "add-on", "bonus", "赠", "加量", "补充", "活动",
                "运营",
            ]
            .iter()
            .any(|word| name.contains(word))
            {
                return None;
            }
            if edition == Edition::Cn {
                if let Some(tier) = domestic_tier(&name) {
                    return Some(tier);
                }
            }
            let words: Vec<_> = name.split(|c: char| !c.is_ascii_alphanumeric()).collect();
            let is_plan =
                name.contains("subscription") || words.contains(&"plan") || words.len() == 1;
            if (is_plan && words.contains(&"team")) || name.contains("团队版") {
                Some((5, "Team"))
            } else if (is_plan && words.contains(&"pro")) || name.contains("专业版") {
                Some((
                    2,
                    if edition == Edition::Cn {
                        "标准版"
                    } else {
                        "Pro"
                    },
                ))
            } else if paid != Some(true)
                && ((is_plan && words.contains(&"free"))
                    || name.contains("免费版")
                    || name.contains("个人体验版"))
            {
                Some((
                    0,
                    if edition == Edition::Cn {
                        "体验版"
                    } else {
                        "Free"
                    },
                ))
            } else {
                None
            }
        })
        .max_by_key(|(rank, _)| *rank)
        .map(|(_, name)| name.to_string());
    plan.or_else(|| {
        (subscription.is_none() && paid == Some(false)).then(|| {
            if edition == Edition::Cn {
                "体验版"
            } else {
                "Free"
            }
            .to_string()
        })
    })
}

fn quota(items: &[Value], summary: Option<&Value>, now: i64, edition: Edition) -> Quota {
    let (remaining, total, expires_at) = summarize(items, now);
    let base: Vec<_> = items
        .iter()
        .filter(|item| category(item) == Some("base"))
        .cloned()
        .collect();
    let rewards: Vec<_> = items
        .iter()
        .filter(|item| category(item) == Some("reward"))
        .cloned()
        .collect();
    let addons: Vec<_> = items
        .iter()
        .filter(|item| category(item) == Some("addon"))
        .cloned()
        .collect();
    let known = |group: &[Value]| {
        !group.is_empty()
            && group
                .iter()
                .filter(|item| expiry(item, now).is_none_or(|end| end > now))
                .all(|item| {
                    resource_amount(item, "Remain").is_some()
                        || (resource_amount(item, "Size").is_some()
                            && resource_amount(item, "Used").is_some())
                })
    };
    let base_values = known(&base).then(|| summarize(&base, now));
    let reward_values = known(&rewards).then(|| summarize(&rewards, now));
    let reward_next_expiry_at = reward_values.and_then(|_| {
        rewards
            .iter()
            .filter_map(|item| {
                let end = package_expiry(item)?;
                (end > now
                    && end.saturating_sub(now) <= 730 * 86400
                    && summarize(std::slice::from_ref(item), now).0 > 0.0)
                    .then_some(end)
            })
            .min()
    });
    let addon_values = known(&addons).then(|| summarize(&addons, now));
    let base_reset_at = base
        .iter()
        .filter_map(|item| {
            let end = timestamp(
                item.get("CycleEndTime")
                    .or_else(|| item.get("cycleEndTime")),
            )
            .or_else(|| expiry(item, now))?;
            (end > now).then_some(end)
        })
        .min();
    Quota {
        remaining,
        total,
        expires_at,
        base_remaining: base_values.map(|value| value.0),
        base_total: base_values.map(|value| value.1),
        base_reset_at,
        reward_remaining: reward_values.map(|value| value.0),
        reward_total: reward_values.map(|value| value.1),
        reward_next_expiry_at,
        addon_remaining: addon_values.map(|value| value.0),
        plan: subscription_plan(items, summary, now, edition),
    }
}
fn category(item: &Value) -> Option<&'static str> {
    let code = text(item, "PackageCode").or_else(|| text(item, "packageCode"));
    let name = text(item, "PackageName")
        .or_else(|| text(item, "packageName"))
        .unwrap_or_default()
        .to_lowercase();
    let numbered = code
        .and_then(|code| code.strip_prefix("TCACA_code_"))
        .and_then(|code| code.split('_').next());
    if matches!(numbered, Some("009" | "036" | "038"))
        || ["加量", "top-up", "top up", "addon", "add-on"]
            .iter()
            .any(|word| name.contains(word))
    {
        Some("addon")
    } else if matches!(
        numbered,
        Some(
            "001"
                | "002"
                | "003"
                | "005"
                | "006"
                | "008"
                | "023"
                | "026"
                | "027"
                | "035"
                | "039"
                | "040"
        )
    ) || code == Some("TCACA_code_enterprise")
    {
        Some("base")
    } else if matches!(numbered, Some("007" | "028" | "029" | "030" | "037"))
        || ["奖励", "赠", "bonus", "reward", "activity"]
            .iter()
            .any(|word| name.contains(word))
    {
        Some("reward")
    } else if ["基础", "体验", "free", "pro", "subscription", "trial"]
        .iter()
        .any(|word| name.contains(word))
    {
        Some("base")
    } else {
        None
    }
}

fn claim_result(body: &Value) -> Result<(), String> {
    match body["code"].as_i64() {
        Some(401) => return Err("accountError.loginRequired".into()),
        Some(403 | 10085) => return Err("accountError.denied".into()),
        _ => {}
    }
    let message = text(body, "message")
        .or_else(|| text(body, "msg"))
        .or_else(|| text(&body["data"], "message"))
        .unwrap_or_default()
        .to_lowercase();
    if [
        "未开启",
        "未开始",
        "未开放",
        "活动已过期",
        "活动已结束",
        "inactive",
        "not available",
        "not enabled",
    ]
    .iter()
    .any(|word| message.contains(word))
    {
        return Err("accountError.claimUnavailable".into());
    }
    let already = body["code"].as_i64() == Some(10001)
        && [
            "已签到",
            "已领取",
            "已经签到",
            "已经领取",
            "重复签到",
            "already checked in",
            "already checked-in",
            "already claimed",
        ]
        .iter()
        .any(|word| message.contains(word));
    if already || (success(body) && body["data"]["success"].as_bool() != Some(false)) {
        Ok(())
    } else {
        Err("accountError.claim".into())
    }
}

fn status_result(body: &Value) -> Option<bool> {
    if !success(body) {
        return None;
    }
    body["data"]["today_checked_in"]
        .as_bool()
        .or_else(|| body["data"]["todayCheckedIn"].as_bool())
}

fn claim_http_result(status: u16, body: Option<&Value>) -> Result<Option<()>, String> {
    if status == 404 || body.and_then(|v| v["code"].as_i64()) == Some(404) {
        return Ok(None);
    }
    let error = match status {
        401 => "accountError.loginRequired",
        403 => "accountError.denied",
        429 => "accountError.rateLimited",
        _ => "accountError.claim",
    };
    let body = body.ok_or_else(|| format!("{error}|HTTP {status}"))?;
    let result = claim_result(body);
    if !((200..300).contains(&status) || body["code"].as_i64() == Some(10001) && result.is_ok()) {
        return Err(format!(
            "{}|HTTP {status}",
            result.err().unwrap_or(error.into())
        ));
    }
    result.map(Some)
}

pub(super) async fn checkin_status(saved: &Saved) -> Result<bool, String> {
    let base = base(saved);
    for path in ["checkin-activity-status", "checkin-status"] {
        let body = response(
            authorized(
                &client()?,
                saved,
                &format!("{base}/v2/billing/meter/{path}"),
            )?
            .header("Origin", base)
            .header("Referer", format!("{base}/profile/plans-usage"))
            .json(&json!({})),
        )
        .await;
        if let Ok(Some(checked_in)) = body.as_ref().map(status_result) {
            return Ok(checked_in);
        }
    }
    Err("accountError.claim".into())
}

pub(super) async fn claim_daily(saved: &Saved) -> Result<(), String> {
    let base = base(saved);
    for path in [
        "/v2/billing/meter/daily-checkin",
        "/billing/meter/daily-checkin",
    ] {
        let response = authorized(&client()?, saved, &format!("{base}{path}"))?
            .header("Origin", base)
            .header("Referer", format!("{base}/profile/plans-usage"))
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| "accountError.network")?;
        let status = response.status();
        let body = response.json::<Value>().await.ok();
        if claim_http_result(status.as_u16(), body.as_ref())?.is_some() {
            return Ok(());
        }
    }
    Err("accountError.network|HTTP 404".into())
}
fn base(saved: &Saved) -> &'static str {
    if saved.summary.edition == Edition::Ai {
        return Edition::Ai.api();
    }
    match text(&saved.session["auth"], "domain") {
        Some("www.workbuddy.cn" | "workbuddy.cn") => "https://www.workbuddy.cn",
        _ => Edition::Cn.api(),
    }
}
async fn post(saved: &Saved, path: &str, body: Value) -> Result<Value, String> {
    let base = base(saved);
    let paths = if saved.summary.edition == Edition::Ai {
        vec![path.to_string(), format!("/v2{path}")]
    } else {
        vec![path.to_string()]
    };
    for (index, path) in paths.iter().enumerate() {
        let result = response(
            authorized(&client()?, saved, &format!("{base}{path}"))?
                .header("Origin", base)
                .header("Referer", format!("{base}/profile/plans-usage"))
                .json(&body),
        )
        .await;
        let value = match result {
            Err(e) if e.ends_with("HTTP 404") && index + 1 < paths.len() => continue,
            other => other?,
        };
        if matches!(value["code"].as_i64(), Some(401 | 10085)) {
            return Err("accountError.loginRequired".into());
        }
        if value["code"].as_i64() == Some(404) && index + 1 < paths.len() {
            continue;
        }
        if !success(&value) {
            return Err("accountError.quota".into());
        }
        return Ok(value);
    }
    Err("accountError.quota".into())
}

pub(super) async fn fetch(saved: &Saved) -> Result<Quota, String> {
    let now = Local::now();
    if saved.summary.edition == Edition::Cn {
        // Fetch package details alongside the billing summary for accurate balances.
        let summary_request = post(saved, "/billing/meter/get-user-resource-summary", json!({}));
        let paid_request = post(
            saved,
            "/billing/meter/get-user-resource-paid-packages",
            json!({"PageNumber":1,"PageSize":200,"Status":[0,3],"NeedRenewInfo":true,"PackageCodes":["TCACA_code_002_AkiJS3ZHF5","TCACA_code_023_4xbGhMrE6q","TCACA_code_026_BaESVICNoi","TCACA_code_027_0FCGVA6vSa","TCACA_code_009_0XmEQc2xOf","TCACA_code_038_OhvqZtiPKr","TCACA_code_003_FAnt7lcmRT","TCACA_code_036_lupO5WgNdG"]}),
        );
        let free_request = post(
            saved,
            "/billing/meter/get-user-resource-free-packages",
            json!({"PageNumber":1,"PageSize":200,"Status":[0,3],"SlicePeriodStartTime":now.format("%Y-%m-%d 00:00:00").to_string(),"SlicePeriodEndTime":now.format("%Y-%m-%d 23:59:59").to_string(),"PackageCodes":["TCACA_code_008_cfWoLwvjU4","TCACA_code_007_nzdH5h4Nl0","TCACA_code_028_NtpWi0jzXs","TCACA_code_029_6wCGEWquYy","TCACA_code_030_BjSt89qTvr","TCACA_code_001_PqouKr6QWV","TCACA_code_006_DbXS0lrypC","TCACA_code_035_ArVxJcGDsm","TCACA_code_037_WxOD3MpI2o","TCACA_code_039_KRcQj7wUat","TCACA_code_040_mi9rCYg46x"]}),
        );
        let (summary, paid, free) = tokio::join!(summary_request, paid_request, free_request);
        for result in [&summary, &paid, &free] {
            if let Err(e) = result {
                if e.starts_with("accountError.loginRequired") {
                    return Err(e.clone());
                }
            }
        }
        // Require complete detail responses so a partial network failure cannot show a false balance.
        if let (Ok(summary), Ok(paid), Ok(free)) = (summary, paid, free) {
            if let (Some(packages), Some(paid), Some(free)) = (
                resources(&summary, "Packages"),
                resources(&paid, "Accounts"),
                resources(&free, "Accounts"),
            ) {
                let mut items = paid.clone();
                items.extend(free.clone());
                for package in packages {
                    let code = package
                        .get("PackageCode")
                        .or_else(|| package.get("packageCode"));
                    if !items.iter().any(|v| {
                        code.is_some()
                            && v.get("PackageCode").or_else(|| v.get("packageCode")) == code
                    }) {
                        items.push(package.clone());
                    }
                }
                return Ok(quota(&items, Some(&summary), now.timestamp(), Edition::Cn));
            }
        }
    }
    let path = if saved.summary.edition == Edition::Ai {
        "/billing/meter/get-user-resource"
    } else {
        "/v2/billing/meter/get-user-resource"
    };
    let body = post(saved,path,json!({"PageNumber":1,"PageSize":100,"ProductCode":"p_tcaca","Status":[0,3],"PackageEndTimeRangeBegin":now.format("%Y-%m-%d %H:%M:%S").to_string(),"PackageEndTimeRangeEnd":(now+chrono::Duration::days(365*101)).format("%Y-%m-%d %H:%M:%S").to_string()})).await?;
    Ok(quota(
        resources(&body, "Accounts").ok_or("accountError.quota")?,
        None,
        now.timestamp(),
        saved.summary.edition,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base_and_reward_balances_keep_their_own_times() {
        let result = quota(
            &[
                json!({"PackageCode":"TCACA_code_008_cfWoLwvjU4", "PackageName":"Free Plan Subscription", "CycleCapacitySize":500, "CycleCapacityRemain":400, "CycleEndTime":1700300000, "DeductionEndTime":1800000000}),
                json!({"PackageCode":"TCACA_code_007_nzdH5h4Nl0", "CycleCapacitySize":100, "CycleCapacityRemain":75, "DeductionEndTime":1700100000}),
                json!({"PackageCode":"TCACA_code_028_NtpWi0jzXs", "CycleCapacitySize":20, "CycleCapacityRemain":10, "DeductionEndTime":1700200000}),
                json!({"PackageCode":"TCACA_code_009_0XmEQc2xOf", "CycleCapacitySize":50, "CycleCapacityRemain":25}),
            ],
            None,
            1700000000,
            Edition::Cn,
        );
        assert_eq!(result.remaining, 510.0);
        assert_eq!(result.base_remaining, Some(400.0));
        assert_eq!(result.base_total, Some(500.0));
        assert_eq!(result.base_reset_at, Some(1700300000));
        assert_eq!(result.reward_remaining, Some(85.0));
        assert_eq!(result.reward_total, Some(120.0));
        assert_eq!(result.reward_next_expiry_at, Some(1700100000));
        assert_eq!(result.addon_remaining, Some(25.0));
        assert_eq!(result.plan.as_deref(), Some("体验版"));
    }
    #[test]
    fn reward_deadline_uses_the_next_unspent_package_not_the_billing_cycle() {
        let now = 1700000000;
        let reward = |remaining, end| json!({"PackageCode":"TCACA_code_007_nzdH5h4Nl0", "CycleCapacitySize":100,"CycleCapacityRemain":remaining,"PackageEndTime":end,"CycleEndTime":now+300,"DeductionEndTime":now+86400});
        let result = quota(
            &[
                reward(18.49, now + 3600),
                reward(30.0, now + 7200),
                reward(0.0, now + 60),
                reward(100.0, now - 1),
                json!({"PackageCode":"TCACA_code_008_cfWoLwvjU4","CycleCapacityRemain":500,"CycleEndTime":now+600}),
            ],
            None,
            now,
            Edition::Cn,
        );
        assert!((result.reward_remaining.unwrap() - 48.49).abs() < 1e-9);
        assert_eq!(result.reward_next_expiry_at, Some(now + 3600));
        assert_eq!(result.base_reset_at, Some(now + 600));
        let unknown = quota(
            &[
                json!({"PackageCode":"TCACA_code_007_nzdH5h4Nl0","CycleCapacityRemain":20,"CycleEndTime":now+300}),
            ],
            None,
            now,
            Edition::Cn,
        );
        assert_eq!(unknown.reward_remaining, Some(20.0));
        assert_eq!(unknown.reward_next_expiry_at, None);
        let exhausted = quota(&[reward(0.0, now + 3600)], None, now, Edition::Cn);
        assert_eq!(exhausted.reward_next_expiry_at, None);
    }
    #[test]
    fn unknown_group_amount_is_not_fabricated() {
        assert_eq!(
            quota(&[], None, 1700000000, Edition::Cn).reward_remaining,
            None
        );
        assert_eq!(
            quota(&[], None, 1700000000, Edition::Cn).addon_remaining,
            None
        );
        assert_eq!(
            quota(
                &[json!({"PackageCode":"TCACA_code_009_0XmEQc2xOf","CapacitySize":100})],
                None,
                1700000000,
                Edition::Cn,
            )
            .addon_remaining,
            None
        );
        assert_eq!(
            quota(
                &[json!({"PackageName":"活动赠送包"})],
                None,
                1700000000,
                Edition::Cn
            )
            .reward_remaining,
            None
        );
        assert_eq!(
            quota(
                &[json!({"PackageName":"活动赠送包", "CapacitySize":100})],
                None,
                1700000000,
                Edition::Cn,
            )
            .reward_remaining,
            None
        );
        assert_eq!(
            quota(
                &[json!({"PackageName":"活动赠送包", "CapacitySize":100, "CapacityUsed":30})],
                None,
                1700000000,
                Edition::Cn,
            )
            .reward_remaining,
            Some(70.0)
        );
        assert_eq!(
            quota(
                &[json!({"PackageName":"活动赠送包", "CapacityRemain":0})],
                None,
                1700000000,
                Edition::Cn,
            )
            .reward_remaining,
            Some(0.0)
        );
    }
    #[test]
    fn exhausted_base_package_keeps_its_refresh_time() {
        let result = quota(
            &[
                json!({"PackageCode":"TCACA_code_008_cfWoLwvjU4", "CycleCapacitySize":500, "CycleCapacityRemain":0, "CycleEndTime":1700100000}),
            ],
            None,
            1700000000,
            Edition::Cn,
        );
        assert_eq!(result.base_remaining, Some(0.0));
        assert_eq!(result.base_total, Some(500.0));
        assert_eq!(result.base_reset_at, Some(1700100000));
    }
    #[test]
    fn daily_claim_requires_confirmed_success_or_an_explicit_already_claimed_result() {
        for body in [
            json!({"code":0,"data":{"success":true,"credit":20}}),
            json!({"code":0}),
            json!({"code":200,"data":{}}),
            json!({"code":10001,"message":"今日已签到"}),
            json!({"code":10001,"msg":"Already claimed"}),
        ] {
            assert_eq!(claim_result(&body), Ok(()));
        }
        for body in [
            json!({}),
            json!({"code":0,"data":{"success":false}}),
            json!({"code":10001,"message":"请明天签到"}),
            json!({"code":500,"message":"already claimed"}),
        ] {
            assert_eq!(claim_result(&body), Err("accountError.claim".into()));
        }
    }
    #[test]
    fn checkin_status_and_http_receipts_are_classified_without_guessing() {
        assert_eq!(
            status_result(&json!({"code":0,"data":{"today_checked_in":true}})),
            Some(true)
        );
        assert_eq!(
            status_result(&json!({"code":0,"data":{"todayCheckedIn":false}})),
            Some(false)
        );
        assert_eq!(status_result(&json!({"code":0,"data":{}})), None);
        assert_eq!(
            claim_http_result(200, Some(&json!({"code":0}))),
            Ok(Some(()))
        );
        assert_eq!(
            claim_http_result(400, Some(&json!({"code":10001,"message":"今日已签到"}))),
            Ok(Some(()))
        );
        assert_eq!(claim_http_result(404, None), Ok(None));
        assert_eq!(claim_http_result(200, Some(&json!({"code":404}))), Ok(None));
        assert_eq!(
            claim_http_result(401, Some(&json!({"code":0}))),
            Err("accountError.loginRequired|HTTP 401".into())
        );
        assert_eq!(
            claim_http_result(500, Some(&json!({"code":0}))),
            Err("accountError.claim|HTTP 500".into())
        );
    }
    #[test]
    fn daily_claim_preserves_auth_denial_and_inactive_failures() {
        for (body, error) in [
            (json!({"code":401}), "accountError.loginRequired"),
            (
                json!({"code":401,"message":"登录已过期"}),
                "accountError.loginRequired",
            ),
            (json!({"code":10085}), "accountError.denied"),
            (json!({"code":403}), "accountError.denied"),
            (
                json!({"code":10001,"message":"签到活动未开启"}),
                "accountError.claimUnavailable",
            ),
            (
                json!({"code":0,"data":{"success":false,"message":"Activity not available"}}),
                "accountError.claimUnavailable",
            ),
        ] {
            assert_eq!(claim_result(&body), Err(error.into()));
        }
    }
    #[test]
    fn cycle_aliases_take_priority_over_lifetime_capacity() {
        let result = summarize(
            &[
                json!({"CycleTotalCapacity":100,"CycleRemainCapacity":25,"CycleUsedCapacity":75,
                "CapacitySize":1200,"CapacityRemain":900,"CapacityUsed":300}),
            ],
            1700000000,
        );
        assert_eq!(result, (25.0, 100.0, None));
    }
    #[test]
    fn detects_subscription_tiers_from_domestic_and_international_packages() {
        for (edition, name, expected) in [
            (Edition::Cn, "CodeBuddy个人体验版", "体验版"),
            (Edition::Cn, "CodeBuddy个人标准版", "标准版"),
            (Edition::Cn, "CodeBuddy个人高级版", "高级版"),
            (Edition::Cn, "CodeBuddy个人旗舰版", "旗舰版"),
            (Edition::Cn, "CodeBuddy个人专业版", "标准版"),
            (Edition::Cn, "CodeBuddy团队版", "Team"),
            (Edition::Ai, "Free Plan Subscription", "Free"),
            (Edition::Ai, "Pro Plan Subscription", "Pro"),
            (Edition::Ai, "Team Plan Subscription", "Team"),
        ] {
            let items = [
                json!({"PackageName":name,"CapacityRemain":0,"Status":0,"CycleEndTime":1700001000}),
            ];
            assert_eq!(
                subscription_plan(&items, None, 1700000000, edition).as_deref(),
                Some(expected)
            );
        }
    }
    #[test]
    fn domestic_official_subscription_name_precedes_generic_resource_names() {
        let items =
            [json!({"PackageCode":"TCACA_code_008_cfWoLwvjU4","PackageName":"版本基础用量"})];
        for (name, expected) in [
            ("体验版", "体验版"),
            ("个人标准版", "标准版"),
            ("个人高级版", "高级版"),
            ("个人旗舰版", "旗舰版"),
        ] {
            let summary =
                json!({"data":{"SubscriptionPackageName":name,"IsPaidUser":name != "体验版"}});
            assert_eq!(
                subscription_plan(&items, Some(&summary), 1700000000, Edition::Cn).as_deref(),
                Some(expected)
            );
        }
        let standard = json!({"data":{"SubscriptionPackageCode":"TCACA_code_003_FAnt7lcmRT","IsPaidUser":true}});
        assert_eq!(
            subscription_plan(&items, Some(&standard), 1700000000, Edition::Cn).as_deref(),
            Some("标准版")
        );
        assert_eq!(
            subscription_plan(&items, Some(&standard), 1700000000, Edition::Ai),
            None
        );
    }
    #[test]
    fn respects_current_subscription_and_does_not_infer_plan_from_credit_topups() {
        let items = [
            json!({"PackageCode":"free","PackageName":"Free Plan Subscription"}),
            json!({"PackageCode":"pro","PackageName":"Pro Plan Subscription"}),
            json!({"PackageCode":"topup","PackageName":"Team Plan Top-up"}),
        ];
        assert_eq!(
            subscription_plan(&items, None, 1700000000, Edition::Ai).as_deref(),
            Some("Pro")
        );
        let summary = json!({"data":{"Response":{"Data":{"SubscriptionPackageCode":"free","IsPaidUser":false}}}});
        assert_eq!(
            subscription_plan(&items, Some(&summary), 1700000000, Edition::Ai).as_deref(),
            Some("Free")
        );
        let unknown = json!({"data":{"SubscriptionPackageCode":"unknown","IsPaidUser":true}});
        assert_eq!(
            subscription_plan(&items, Some(&unknown), 1700000000, Edition::Ai),
            None
        );
        assert_eq!(
            subscription_plan(&items[2..], None, 1700000000, Edition::Ai),
            None
        );
    }
    #[test]
    fn ignores_expired_future_and_inactive_plans_and_preserves_unknown_state() {
        let items = [
            json!({"PackageName":"Pro Plan Subscription","DeductionEndTime":1699999999}),
            json!({"PackageName":"Team Plan Subscription","DeductionStartTime":1700001000}),
            json!({"PackageName":"Pro Plan Subscription","Status":1}),
            json!({"packageName":"Free Plan Subscription","status":0}),
        ];
        assert_eq!(
            subscription_plan(&items, None, 1700000000, Edition::Ai).as_deref(),
            Some("Free")
        );
        assert_eq!(subscription_plan(&[], None, 1700000000, Edition::Ai), None);
        let free = json!({"data":{"SubscriptionPackageCode":"","IsPaidUser":false}});
        assert_eq!(
            subscription_plan(&[], Some(&free), 1700000000, Edition::Ai).as_deref(),
            Some("Free")
        );
        let paid = json!({"data":{"IsPaidUser":true}});
        assert_eq!(
            subscription_plan(&items, Some(&paid), 1700000000, Edition::Ai),
            None
        );
    }
    #[test]
    fn quota_uses_precise_values_and_nearest_nonempty_expiry() {
        let result = summarize(
            &[
                json!({"CycleCapacitySizePrecise":"100.5","CycleCapacityRemainPrecise":"75.25","ExpiredTime":1700001000}),
                json!({"CapacitySize":50,"CapacityRemain":0,"ExpiredTime":1700000100}),
                json!({"CapacityRemain":500,"ExpiredTime":1699999999}),
            ],
            1700000000,
        );
        assert_eq!(result, (75.25, 150.5, Some(1700001000)));
    }
    #[test]
    fn long_term_placeholder_uses_cycle_and_timestamps_are_seconds() {
        assert_eq!(timestamp(Some(&json!(1700000000000_i64))), Some(1700000000));
        assert_eq!(
            timestamp(Some(&json!("2023-11-14T22:13:20Z"))),
            Some(1700000000)
        );
        assert_eq!(
            expiry(
                &json!({"DeductionEndTime":2500000000_i64,"CycleEndTime":1700001000}),
                1700000000
            ),
            Some(1700001000)
        );
        assert_eq!(
            expiry(&json!({"DeductionEndTime":2500000000_i64}), 1700000000),
            None
        );
    }
    #[test]
    fn empty_and_missing_resource_lists_are_distinct() {
        assert!(resources(&json!({"data":{}}), "Accounts").is_none());
        assert_eq!(
            resources(
                &json!({"data":{"Response":{"Data":{"Accounts":[]}}}}),
                "Accounts"
            ),
            Some(&vec![])
        );
        assert_eq!(summarize(&[], 1700000000), (0.0, 0.0, None));
    }
}
