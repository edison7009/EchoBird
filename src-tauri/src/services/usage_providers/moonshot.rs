//! Kimi open platform (Moonshot) balance for pay-as-you-go API keys.
//!
//! API: GET {origin}/v1/users/me/balance with the inference key.
//! The China site bills in CNY and the international site in USD.
use super::{api_url, fetch_usage, parse_f64, UsageProvider, UsageQuota, UsageResult};
use serde_json::Value;

pub struct MoonshotProvider;

fn regional_balance_api(base_url: &str) -> Option<(&'static str, &'static str)> {
    match api_url(base_url, &["api.moonshot.cn", "api.moonshot.ai"])?.host_str()? {
        "api.moonshot.cn" => Some(("https://api.moonshot.cn/v1/users/me/balance", "CNY")),
        "api.moonshot.ai" => Some(("https://api.moonshot.ai/v1/users/me/balance", "USD")),
        _ => None,
    }
}

fn parse_balance(body: &Value, currency: &str) -> UsageResult {
    // A reply may leave out either marker; only one that says so is a refusal.
    let refused_code = body
        .get("code")
        .is_some_and(|code| code.as_i64() != Some(0));
    if refused_code || body["status"] == false {
        return UsageResult::failure("Kimi rejected the balance query; check the key");
    }
    match parse_f64(&body["data"]["available_balance"])
        .and_then(|amount| UsageQuota::balance(amount, currency))
    {
        Some(quota) => UsageResult::from_quotas(vec![quota]),
        None => UsageResult::failure("No balance data available"),
    }
}

#[async_trait::async_trait]
impl UsageProvider for MoonshotProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        let Some((endpoint, currency)) = regional_balance_api(base_url) else {
            return Ok(UsageResult::failure("Unsupported Kimi API host"));
        };
        Ok(
            match fetch_usage(endpoint, &format!("Bearer {api_key}")).await {
                Ok(body) => parse_balance(&body, currency),
                Err(error) => UsageResult::failure(error),
            },
        )
    }

    fn can_handle(&self, base_url: &str) -> bool {
        regional_balance_api(base_url).is_some()
    }

    fn name(&self) -> &'static str {
        "Moonshot"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn each_site_reads_its_own_balance_in_its_currency() {
        assert_eq!(
            regional_balance_api("https://api.moonshot.cn/v1"),
            Some(("https://api.moonshot.cn/v1/users/me/balance", "CNY"))
        );
        assert_eq!(
            regional_balance_api("https://api.moonshot.ai/anthropic"),
            Some(("https://api.moonshot.ai/v1/users/me/balance", "USD"))
        );
        for base in [
            "http://api.moonshot.cn/v1",
            "https://api.moonshot.cn.relay.example/v1",
            "https://relay.example/api.moonshot.cn/v1",
            "https://api.kimi.com/coding/v1",
        ] {
            assert_eq!(regional_balance_api(base), None, "{base}");
        }
    }

    #[test]
    fn the_available_balance_includes_vouchers_and_may_be_zero() {
        let body = json!({"code":0,"scode":"0x0","status":true,"data":{
            "available_balance":49.58894,"voucher_balance":46.58893,"cash_balance":3.00001
        }});
        let result = parse_balance(&body, "CNY");
        let quota = &result.data.unwrap().quotas[0];
        assert_eq!(quota.balance, Some(49.58894));
        assert_eq!(quota.balance_unit.as_deref(), Some("CNY"));
        let zero = parse_balance(
            &json!({"code":0,"status":true,"data":{"available_balance":0}}),
            "USD",
        );
        assert_eq!(zero.data.unwrap().quotas[0].balance, Some(0.0));
        let unmarked = parse_balance(&json!({"data":{"available_balance":12}}), "USD");
        assert_eq!(unmarked.data.unwrap().quotas[0].balance, Some(12.0));
    }

    #[test]
    fn rejected_or_unreadable_replies_have_no_balance() {
        for body in [
            json!({"code":1,"status":false,"data":{"available_balance":5}}),
            json!({"code":0,"status":false,"data":{"available_balance":5}}),
            json!({"code":0,"status":true,"data":{}}),
            json!({"code":0,"status":true,"data":{"available_balance":"NaN"}}),
            json!({"error":{"message":"invalid key"}}),
        ] {
            let result = parse_balance(&body, "CNY");
            assert!(!result.success);
            assert!(result.data.is_none());
        }
    }
}
