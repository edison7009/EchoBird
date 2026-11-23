//! Xiaomi MiMo wallet and personal Token Plan usage.

use super::{
    api_url, fetch_official_usage, parse_f64, parse_reset_time, share_read, QuotaPeriod,
    UsageProvider, UsageQuota, UsageResult,
};
use serde_json::Value;

const WALLET_HOST: &str = "api.xiaomimimo.com";
const PLAN_HOSTS: &[&str] = &[
    "token-plan-cn.xiaomimimo.com",
    "token-plan-sgp.xiaomimimo.com",
    "token-plan-ams.xiaomimimo.com",
];

pub struct MiMoProvider;

#[derive(Clone, Copy)]
enum Endpoint {
    Wallet,
    Plan,
}

fn official_endpoint(base_url: &str) -> Option<(url::Url, Endpoint)> {
    let (mut url, kind, path) = if let Some(url) = api_url(base_url, &[WALLET_HOST]) {
        (url, Endpoint::Wallet, "/v1/platform/balance")
    } else {
        (
            api_url(base_url, PLAN_HOSTS)?,
            Endpoint::Plan,
            "/v1/platform/token_plan",
        )
    };
    if url.port().is_some() {
        return None;
    }
    url.set_path(path);
    url.set_query(None);
    url.set_fragment(None);
    Some((url, kind))
}

fn wallet_quotas(body: &Value) -> Vec<UsageQuota> {
    if body["object"].as_str() != Some("balance") {
        return Vec::new();
    }
    body["balance_infos"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|wallet| {
            let total = parse_f64(&wallet["total_available"])?;
            let frozen = parse_f64(&wallet["frozen_balance"]).filter(|amount| *amount >= 0.0)?;
            UsageQuota::balance(total - frozen, wallet["currency"].as_str()?)
        })
        .collect()
}

fn plan_quota(body: &Value) -> Option<UsageQuota> {
    if body["object"].as_str() != Some("token_plan") {
        return None;
    }
    let quota = &body["quota"];
    let end = &body["period"]["end_at"];
    let reset_at = end
        .as_str()
        .and_then(|value| chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S").ok())
        .map(|date| date.and_utc().timestamp_millis())
        .or_else(|| parse_reset_time(end));
    UsageQuota::allowance(
        QuotaPeriod::BillingCycle,
        parse_f64(&quota["total_credits"])?,
        parse_f64(&quota["used_credits"])?,
        parse_f64(&quota["remaining_credits"])?,
        "Credits",
        reset_at,
    )
}

#[async_trait::async_trait]
impl UsageProvider for MiMoProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        let Some((url, endpoint)) = official_endpoint(base_url) else {
            return Ok(UsageResult::failure("Unsupported Xiaomi MiMo API host"));
        };
        let (prefix, invalid_key) = match endpoint {
            Endpoint::Wallet => ("sk-", "Xiaomi MiMo balance requires a standard sk- API key"),
            Endpoint::Plan => ("tp-", "Xiaomi MiMo Token Plan requires a tp- API key"),
        };
        let api_key = api_key.trim();
        if api_key.strip_prefix(prefix).is_none_or(str::is_empty) {
            return Ok(UsageResult::failure(invalid_key));
        }
        let url = url.as_str();
        Ok(share_read(url, api_key, async {
            let body = match fetch_official_usage(url, api_key).await {
                Ok(body) => body,
                Err(error) => return UsageResult::failure(error),
            };
            let quotas = match endpoint {
                Endpoint::Wallet => wallet_quotas(&body),
                Endpoint::Plan => plan_quota(&body).into_iter().collect(),
            };
            UsageResult::from_quotas(quotas)
        })
        .await)
    }

    fn can_handle(&self, base_url: &str) -> bool {
        official_endpoint(base_url).is_some()
    }

    fn name(&self) -> &'static str {
        "Xiaomi MiMo"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wallet_subtracts_frozen_funds_and_keeps_zero_and_debt() {
        for (frozen, expected) in [("25.50", 475.0), ("500.50", 0.0), ("501.50", -1.0)] {
            let body = json!({
                "object": "balance",
                "balance_infos": [{
                    "currency": "CNY", "total_available": "500.50", "frozen_balance": frozen
                }]
            });
            let quotas = wallet_quotas(&body);
            assert_eq!(quotas.len(), 1);
            assert_eq!(quotas[0].balance, Some(expected));
            assert_eq!(quotas[0].balance_unit.as_deref(), Some("CNY"));
        }
        assert!(wallet_quotas(&json!({"object":"balance", "balance_infos":[{"currency":"CNY","total_available":"12.00"}]})).is_empty());
    }

    #[test]
    fn token_plan_reports_actual_credits_and_utc_period_end() {
        let body = json!({
            "object": "token_plan",
            "period": {"start_at": "2026-09-21 23:59:59", "end_at": "2027-09-21 23:59:59"},
            "quota": {"total_credits": 10000000, "used_credits": 1500000, "remaining_credits": 8500000}
        });
        let quota = plan_quota(&body).unwrap();
        assert_eq!(quota.period, Some(QuotaPeriod::BillingCycle));
        assert!((quota.percentage - 15.0).abs() < 1e-9);
        assert_eq!(quota.reset_at, 1_821_571_199_000);
        let amounts = quota.amounts.unwrap();
        assert_eq!(amounts.remaining, 8_500_000.0);
        assert_eq!(amounts.used, 1_500_000.0);
        assert_eq!(amounts.unit, "Credits");
    }

    #[test]
    fn inference_paths_share_canonical_endpoints_and_plan_keeps_its_region() {
        for path in ["/v1", "/anthropic?ignore=1#fragment"] {
            let (wallet, kind) =
                official_endpoint(&format!("https://{WALLET_HOST}{path}")).unwrap();
            assert!(matches!(kind, Endpoint::Wallet));
            assert_eq!(
                wallet.as_str(),
                "https://api.xiaomimimo.com/v1/platform/balance"
            );
            for host in PLAN_HOSTS {
                let (plan, kind) = official_endpoint(&format!("https://{host}{path}")).unwrap();
                assert!(matches!(kind, Endpoint::Plan));
                assert_eq!(
                    plan.as_str(),
                    format!("https://{host}/v1/platform/token_plan")
                );
            }
        }
        for url in [
            "https://api.xiaomimimo.com.example/v1",
            "https://relay.example/api.xiaomimimo.com/v1",
            "http://api.xiaomimimo.com/v1",
            "https://api.xiaomimimo.com:8443/v1",
            "https://user@api.xiaomimimo.com/v1",
        ] {
            assert!(official_endpoint(url).is_none());
        }
    }

    #[tokio::test]
    async fn wrong_key_kind_is_rejected_before_requesting_either_endpoint() {
        for (key, base, expected) in [
            ("tp-fixture", "https://api.xiaomimimo.com/v1", "sk-"),
            (
                "sk-fixture",
                "https://token-plan-cn.xiaomimimo.com/anthropic",
                "tp-",
            ),
        ] {
            let result = MiMoProvider.query_usage(key, base).await.unwrap();
            assert!(result.data.is_none());
            assert!(result.error.unwrap().contains(expected));
        }
    }
}
