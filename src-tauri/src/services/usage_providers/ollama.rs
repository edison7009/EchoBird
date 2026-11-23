//! Ollama Cloud included allowance and purchased credits.

use super::{
    api_url, fetch_official_usage, parse_f64, parse_reset_time, share_read, QuotaPeriod,
    UsageProvider, UsageQuota, UsageResult,
};
use serde_json::Value;

pub struct OllamaProvider;

fn parse_quotas(body: &Value) -> Vec<UsageQuota> {
    let included = &body["included"];
    let mut quotas = Vec::new();
    if let (Some(allowance), Some(balance)) = (
        parse_f64(&included["allowance_usd"]),
        parse_f64(&included["balance_usd"]),
    ) {
        quotas.extend(UsageQuota::allowance(
            QuotaPeriod::Monthly,
            allowance,
            (allowance - balance).max(0.0),
            balance,
            "USD",
            parse_reset_time(&included["period"]["until"]),
        ));
    } else {
        for (field, period) in [
            ("session", QuotaPeriod::Session),
            ("weekly", QuotaPeriod::Weekly),
        ] {
            let limit = &included[field];
            let Some(remaining) = parse_f64(&limit["remaining_percent"])
                .filter(|remaining| (0.0..=100.0).contains(remaining))
            else {
                continue;
            };
            quotas.extend(UsageQuota::window(
                period,
                100.0 - remaining,
                parse_reset_time(&limit["resets_at"]),
            ));
        }
    }
    let purchased = parse_f64(&body["purchased"]["balance_usd"]).filter(|balance| *balance >= 0.0);
    quotas.extend(purchased.and_then(|balance| UsageQuota::balance(balance, "USD")));
    quotas
}

#[async_trait::async_trait]
impl UsageProvider for OllamaProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        if !self.can_handle(base_url) {
            return Ok(UsageResult::failure("Unsupported Ollama Cloud API host"));
        }
        let url = "https://ollama.com/api/balance";
        let api_key = api_key.trim();
        Ok(share_read(url, api_key, async {
            let body = match fetch_official_usage(url, api_key).await {
                Ok(body) => body,
                Err(error) => return UsageResult::failure(error),
            };
            UsageResult::from_quotas(parse_quotas(&body))
        })
        .await)
    }

    fn can_handle(&self, base_url: &str) -> bool {
        api_url(base_url, &["ollama.com"]).is_some_and(|url| url.port().is_none())
    }

    fn name(&self) -> &'static str {
        "Ollama Cloud"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_plan_preserves_included_and_purchased_usd_separately() {
        let quotas = parse_quotas(&json!({
            "included": {
                "balance_usd": 72.5, "allowance_usd": 100,
                "period": {"from": "2026-09-15T09:30:00Z", "until": "2026-10-15T09:30:00Z"}
            },
            "purchased": {"balance_usd": 25}
        }));
        assert_eq!(quotas.len(), 2);
        assert!((quotas[0].percentage - 27.5).abs() < 1e-9);
        assert_eq!(quotas[0].period, Some(QuotaPeriod::Monthly));
        assert!(quotas[0].reset_at > 0);
        assert_eq!(quotas[0].amounts.as_ref().unwrap().remaining, 72.5);
        assert_eq!(quotas[1].balance, Some(25.0));
        assert_eq!(quotas[1].balance_unit.as_deref(), Some("USD"));
    }

    #[test]
    fn legacy_plan_has_session_and_weekly_limits_alongside_zero_purchased_balance() {
        let quotas = parse_quotas(&json!({
            "included": {
                "session": {"remaining_percent": 75, "resets_at": "2026-10-01T07:00:00Z"},
                "weekly": {"remaining_percent": 0, "resets_at": "2026-10-05T00:00:00Z"}
            },
            "purchased": {"balance_usd": 0}
        }));
        assert_eq!(quotas.len(), 3);
        assert_eq!(quotas[0].period, Some(QuotaPeriod::Session));
        assert_eq!(quotas[0].percentage, 25.0);
        assert_eq!(quotas[1].period, Some(QuotaPeriod::Weekly));
        assert_eq!(quotas[1].percentage, 100.0);
        assert_eq!(quotas[2].balance, Some(0.0));
        assert!(parse_quotas(&json!({"included":{"weekly":{"remaining_percent":110}}})).is_empty());
    }

    #[test]
    fn local_instances_and_relays_do_not_use_the_cloud_balance_endpoint() {
        assert!(OllamaProvider.can_handle("https://ollama.com/v1"));
        for base in [
            "http://localhost:11434/v1",
            "https://ollama.com.example/v1",
            "https://relay.example/ollama.com",
            "https://ollama.com:8443",
        ] {
            assert!(!OllamaProvider.can_handle(base));
        }
    }
}
