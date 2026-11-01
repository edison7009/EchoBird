//! Zhipu and Z.ai subscription quota windows.
use super::{
    api_url, fetch_usage, now_millis, parse_f64, parse_reset_time, QuotaPeriod, UsageProvider,
    UsageQuota, UsageResult,
};
use serde_json::Value;

pub struct ZhipuProvider;

const BALANCE_URL: &str = "https://open.bigmodel.cn/api/biz/account/query-customer-account-report";

/// Window length from the limit's unit code and count, as the console reads
/// them: 1 = day, 3 = hour, 5 = minute, 6 = week.
fn window_period(item: &Value) -> Option<QuotaPeriod> {
    let unit = item["unit"].as_i64()?;
    let minutes = match (unit, item["number"].as_i64()) {
        // Older replies carry the unit alone; these were the 5-hour and weekly plans.
        (3, None) => 300,
        (6, None) => 10080,
        (1, Some(count)) => count * 1440,
        (3, Some(count)) => count * 60,
        (5, Some(count)) => count,
        (6, Some(count)) => count * 10080,
        _ => return None,
    };
    match (minutes, unit) {
        (300, _) => Some(QuotaPeriod::FiveHour),
        (1440, _) => Some(QuotaPeriod::Daily),
        (10080, _) => Some(QuotaPeriod::Weekly),
        (43200, _) => Some(QuotaPeriod::Monthly),
        // An hour or week limit of another length keeps the window it had.
        (_, 3) => Some(QuotaPeriod::FiveHour),
        (_, 6) => Some(QuotaPeriod::Weekly),
        _ => None,
    }
}

/// A 5-hour window cannot reset hours beyond five hours; such a reply is off by
/// a time zone, and no correction is guessed. Half an hour of slack absorbs
/// clock drift between the server and this computer.
fn plausible_reset(period: Option<QuotaPeriod>, reset_at: i64) -> i64 {
    let latest = now_millis() + (5 * 3600 + 1800) * 1000;
    if period == Some(QuotaPeriod::FiveHour) && reset_at > latest {
        0
    } else {
        reset_at
    }
}

/// A domestic account without a Coding Plan is told so instead of given limits.
fn has_no_coding_plan(body: &Value) -> bool {
    body["success"] == false
        && body["code"].as_i64() == Some(500)
        && body["msg"]
            .as_str()
            .is_some_and(|msg| msg.to_ascii_lowercase().contains("coding plan"))
}

fn parse_balance(body: &Value) -> UsageResult {
    if body["success"] != true {
        return UsageResult::failure("Zhipu rejected the balance query; check the key");
    }
    let data = &body["data"];
    match parse_f64(&data["availableBalance"])
        .or_else(|| parse_f64(&data["balance"]))
        .and_then(|amount| UsageQuota::balance(amount, "CNY"))
    {
        Some(quota) => UsageResult::from_quotas(vec![quota]),
        None => UsageResult::failure("No balance data available"),
    }
}

fn usage_url(base_url: &str) -> Option<String> {
    let url = api_url(base_url, &["open.bigmodel.cn", "api.z.ai"])?;
    Some(format!(
        "https://{}/api/monitor/usage/quota/limit",
        url.host_str()?
    ))
}

pub(crate) fn parse_response(body: &Value) -> UsageResult {
    if body["success"] == false {
        return UsageResult::failure(
            "Zhipu rejected the quota query; check the key and subscription",
        );
    }
    let Some(limits) = body["data"]["limits"].as_array() else {
        return UsageResult::failure("No usage data available");
    };
    let mut quotas = Vec::new();
    for item in limits {
        let kind = item["type"].as_str().unwrap_or("");
        if !kind.eq_ignore_ascii_case("TOKENS_LIMIT") && !kind.eq_ignore_ascii_case("CREDIT_LIMIT")
        {
            continue;
        }
        let Some(used) = parse_f64(&item["percentage"]).filter(|value| value.is_finite()) else {
            continue;
        };
        let period = window_period(item);
        if period.is_some()
            && quotas
                .iter()
                .any(|quota: &UsageQuota| quota.period == period)
        {
            continue;
        }
        quotas.push(UsageQuota {
            percentage: used.clamp(0.0, 100.0),
            reset_at: plausible_reset(
                period,
                parse_reset_time(&item["nextResetTime"]).unwrap_or(0),
            ),
            period,
            balance: None,
            balance_unit: None,
            spent: false,
        });
    }
    // A window the card cannot name is dropped beside named ones; alone it is
    // the plan's only limit, which older replies sent without a unit.
    if quotas.iter().any(|quota| quota.period.is_some()) {
        quotas.retain(|quota| quota.period.is_some());
    }
    if quotas.len() == 1 && quotas[0].period.is_none() {
        quotas[0].period = Some(QuotaPeriod::FiveHour);
    }
    quotas.sort_by_key(|quota| match quota.period {
        Some(QuotaPeriod::FiveHour) => 0,
        Some(QuotaPeriod::Daily) => 1,
        Some(QuotaPeriod::Weekly) => 2,
        Some(QuotaPeriod::Monthly) => 3,
        None => 4,
    });
    UsageResult::from_quotas(quotas)
}

#[async_trait::async_trait]
impl UsageProvider for ZhipuProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        let endpoint = usage_url(base_url).ok_or("Unsupported Zhipu endpoint")?;
        // The monitoring endpoint accepts the raw key, without a Bearer prefix.
        let body = match fetch_usage(&endpoint, api_key).await {
            Ok(body) => body,
            Err(error) => return Ok(UsageResult::failure(error)),
        };
        if !has_no_coding_plan(&body) || api_url(base_url, &["open.bigmodel.cn"]).is_none() {
            return Ok(parse_response(&body));
        }
        // Pay-as-you-go accounts still have a balance, asked with the same raw key.
        Ok(match fetch_usage(BALANCE_URL, api_key).await {
            Ok(body) => parse_balance(&body),
            Err(error) => UsageResult::failure(error),
        })
    }
    fn can_handle(&self, base_url: &str) -> bool {
        usage_url(base_url).is_some()
    }
    fn name(&self) -> &'static str {
        "Zhipu"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn window_identity_does_not_depend_on_reset_order() {
        let result = parse_response(&json!({"success":true,"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":20,"nextResetTime":1800000000000_i64},
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":90,"nextResetTime":1800010000000_i64},
            {"type":"TIME_LIMIT","percentage":70}
        ]}}));
        let quotas = result.data.unwrap().quotas;
        assert_eq!(quotas.len(), 2);
        assert_eq!(quotas[0].period, Some(QuotaPeriod::FiveHour));
        assert_eq!(quotas[0].percentage, 90.0);
        assert_eq!(quotas[1].period, Some(QuotaPeriod::Weekly));
        assert_eq!(quotas[1].percentage, 20.0);
    }
    #[test]
    fn windows_follow_the_unit_and_count_and_unknown_ones_are_dropped() {
        let result = parse_response(&json!({"success":true,"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":4,"number":1,"percentage":99},
            {"type":"TOKENS_LIMIT","unit":1,"number":1,"percentage":30},
            {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":42},
            {"type":"TOKENS_LIMIT","unit":1,"number":30,"percentage":10},
            {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":15}
        ]}}));
        let quotas = result.data.unwrap().quotas;
        let periods: Vec<_> = quotas.iter().map(|quota| quota.period).collect();
        assert_eq!(
            periods,
            vec![
                Some(QuotaPeriod::FiveHour),
                Some(QuotaPeriod::Daily),
                Some(QuotaPeriod::Weekly),
                Some(QuotaPeriod::Monthly)
            ]
        );
        assert_eq!(quotas[0].percentage, 42.0);
        assert_eq!(quotas[1].percentage, 30.0);
    }

    #[test]
    fn a_five_hour_reset_beyond_five_hours_is_unknown() {
        let soon = now_millis() + 3600 * 1000;
        let far = now_millis() + 10 * 3600 * 1000;
        for (reset, expected) in [(soon, soon), (far, 0)] {
            let result = parse_response(&json!({"success":true,"data":{"limits":[
                {"type":"TOKENS_LIMIT","unit":3,"number":5,"percentage":42,"nextResetTime":reset}
            ]}}));
            assert_eq!(result.data.unwrap().quotas[0].reset_at, expected);
        }
        let weekly = parse_response(&json!({"success":true,"data":{"limits":[
            {"type":"TOKENS_LIMIT","unit":6,"number":1,"percentage":42,"nextResetTime":far}
        ]}}));
        assert_eq!(weekly.data.unwrap().quotas[0].reset_at, far);
    }

    #[test]
    fn an_account_without_a_coding_plan_shows_its_balance() {
        let no_plan = json!({"code":500,"msg":"当前用户不存在coding plan","success":false});
        assert!(has_no_coding_plan(&no_plan));
        assert!(!has_no_coding_plan(
            &json!({"code":401,"msg":"bad key","success":false})
        ));
        let result = parse_balance(&json!({"code":200,"success":true,"data":{
            "balance":14.68,"availableBalance":14.5,"frozenBalance":0.18
        }}));
        let quota = &result.data.unwrap().quotas[0];
        assert_eq!(quota.balance, Some(14.5));
        assert_eq!(quota.balance_unit.as_deref(), Some("CNY"));
        let fallback = parse_balance(&json!({"success":true,"data":{"balance":"3.5"}}));
        assert_eq!(fallback.data.unwrap().quotas[0].balance, Some(3.5));
        for body in [
            json!({"success":false,"msg":"x"}),
            json!({"success":true,"data":{}}),
            json!({"success":true,"data":{"availableBalance":"inf"}}),
        ] {
            assert!(!parse_balance(&body).success);
        }
    }

    #[test]
    fn rejects_api_errors_and_missing_percentages() {
        assert!(!parse_response(&json!({"success":false,"data":{"limits":[]}})).success);
        assert!(
            !parse_response(&json!({"data":{"limits":[{"type":"TOKENS_LIMIT","unit":3}]}})).success
        );
        let result =
            parse_response(&json!({"data":{"limits":[{"type":"CREDIT_LIMIT","percentage":0}]}}));
        let quota = &result.data.unwrap().quotas[0];
        assert_eq!(quota.percentage, 0.0);
        assert_eq!(quota.reset_at, 0);
    }
}
