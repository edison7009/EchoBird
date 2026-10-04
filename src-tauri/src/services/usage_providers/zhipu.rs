//! Zhipu and Z.ai subscription quota windows.
use super::{
    api_url, fetch_usage, parse_f64, parse_reset_time, QuotaPeriod, UsageProvider, UsageQuota,
    UsageResult,
};
use serde_json::Value;

pub struct ZhipuProvider;

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
        let period = match item["unit"].as_i64() {
            Some(3) => Some(QuotaPeriod::FiveHour),
            Some(6) => Some(QuotaPeriod::Weekly),
            _ => None,
        };
        if period.is_some()
            && quotas
                .iter()
                .any(|quota: &UsageQuota| quota.period == period)
        {
            continue;
        }
        quotas.push(UsageQuota {
            percentage: used.clamp(0.0, 100.0),
            reset_at: parse_reset_time(&item["nextResetTime"]).unwrap_or(0),
            period,
            balance: None,
            balance_unit: None,
        });
    }
    // Old plans exposed a single token limit without a window unit.
    if quotas.len() == 1 && quotas[0].period.is_none() {
        quotas[0].period = Some(QuotaPeriod::FiveHour);
    }
    quotas.sort_by_key(|quota| match quota.period {
        Some(QuotaPeriod::FiveHour) => 0,
        Some(QuotaPeriod::Weekly) => 1,
        _ => 2,
    });
    UsageResult::from_quotas(quotas)
}

#[async_trait::async_trait]
impl UsageProvider for ZhipuProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        let endpoint = usage_url(base_url).ok_or("Unsupported Zhipu endpoint")?;
        // The monitoring endpoint accepts the raw key, without a Bearer prefix.
        Ok(match fetch_usage(&endpoint, api_key).await {
            Ok(body) => parse_response(&body),
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
