//! OpenRouter usage provider
//!
//! The account balance (`/credits`) answers only a Management API key, so an
//! inference key reads its own spending limit from `GET /api/v1/key`.
use super::{api_url, fetch_usage, parse_f64, QuotaPeriod, UsageProvider, UsageQuota, UsageResult};
use chrono::{Datelike, Duration, TimeZone, Utc};
use serde_json::Value;

pub struct OpenRouterProvider;

const KEY_URL: &str = "https://openrouter.ai/api/v1/key";

/// The limit windows are UTC days, Monday-to-Sunday weeks and calendar months.
fn window(reset: Option<&str>) -> Option<(QuotaPeriod, i64)> {
    let today = Utc::now().date_naive();
    let (period, next) = match reset? {
        "daily" => (QuotaPeriod::Daily, today + Duration::days(1)),
        "weekly" => (
            QuotaPeriod::Weekly,
            today + Duration::days(7 - i64::from(today.weekday().num_days_from_monday())),
        ),
        "monthly" => {
            let (year, month) = if today.month() == 12 {
                (today.year() + 1, 1)
            } else {
                (today.year(), today.month() + 1)
            };
            (
                QuotaPeriod::Monthly,
                today.with_day(1)?.with_year(year)?.with_month(month)?,
            )
        }
        _ => return None,
    };
    let reset_at = Utc.from_utc_datetime(&next.and_hms_opt(0, 0, 0)?);
    Some((period, reset_at.timestamp_millis()))
}

/// A key without a spending limit has nothing to measure against.
fn parse_key_limit(body: &Value) -> Option<UsageQuota> {
    let data = body.get("data")?;
    let limit = parse_f64(&data["limit"]).filter(|limit| *limit > 0.0)?;
    let reset = data["limit_reset"].as_str();
    let used = match parse_f64(&data["limit_remaining"]) {
        Some(remaining) => limit - remaining.clamp(0.0, limit),
        None => {
            let field = match reset {
                Some("daily") => "usage_daily",
                Some("weekly") => "usage_weekly",
                Some("monthly") => "usage_monthly",
                _ => "usage",
            };
            parse_f64(&data[field])?
        }
    };
    let percentage = used / limit * 100.0;
    match window(reset) {
        Some((period, reset_at)) => UsageQuota::window(period, percentage, Some(reset_at)),
        None => percentage.is_finite().then(|| UsageQuota {
            percentage: percentage.clamp(0.0, 100.0),
            reset_at: 0,
            period: None,
            balance: None,
            balance_unit: None,
            spent: false,
        }),
    }
}

#[async_trait::async_trait]
impl UsageProvider for OpenRouterProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        if !self.can_handle(base_url) {
            return Ok(UsageResult::failure("Unsupported OpenRouter API host"));
        }
        Ok(
            match fetch_usage(KEY_URL, &format!("Bearer {api_key}")).await {
                Ok(body) => match parse_key_limit(&body) {
                    Some(quota) => UsageResult::from_quotas(vec![quota]),
                    // Without a spending limit there is nothing to show; the
                    // account balance needs a Management API key.
                    None => UsageResult::no_data(),
                },
                Err(error) => UsageResult::failure(error),
            },
        )
    }

    fn can_handle(&self, base_url: &str) -> bool {
        api_url(base_url, &["openrouter.ai"]).is_some()
    }

    fn name(&self) -> &'static str {
        "OpenRouter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_capped_key_shows_its_limit_window() {
        let quota = parse_key_limit(&json!({"data":{
            "limit":50.0,"limit_remaining":24.5,"limit_reset":"monthly","usage":25.5
        }}))
        .unwrap();
        assert_eq!(quota.period, Some(QuotaPeriod::Monthly));
        assert!((quota.percentage - 51.0).abs() < 1e-9);
        let next = Utc.timestamp_millis_opt(quota.reset_at).unwrap();
        assert_eq!(next.day(), 1);
        assert!(quota.reset_at > Utc::now().timestamp_millis());

        let daily = parse_key_limit(&json!({"data":{
            "limit":10,"limit_reset":"daily","usage":9,"usage_daily":2
        }}))
        .unwrap();
        assert_eq!(daily.period, Some(QuotaPeriod::Daily));
        assert!((daily.percentage - 20.0).abs() < 1e-9);

        let weekly = parse_key_limit(&json!({"data":{
            "limit":10,"limit_remaining":10,"limit_reset":"weekly"
        }}))
        .unwrap();
        let monday = Utc.timestamp_millis_opt(weekly.reset_at).unwrap();
        assert_eq!(monday.weekday(), chrono::Weekday::Mon);
        assert_eq!(weekly.percentage, 0.0);

        let lifetime = parse_key_limit(&json!({"data":{
            "limit":20,"limit_remaining":30,"limit_reset":null
        }}))
        .unwrap();
        assert_eq!(lifetime.period, None);
        assert_eq!(lifetime.percentage, 0.0);
    }

    #[test]
    fn an_uncapped_or_unreadable_key_has_no_limit() {
        for body in [
            json!({"data":{"limit":null,"usage":12}}),
            json!({"data":{"limit":0,"usage":12}}),
            json!({"data":{"limit":10,"limit_reset":"daily"}}),
            json!({"error":{"message":"x"}}),
        ] {
            assert!(parse_key_limit(&body).is_none());
        }
    }

    #[tokio::test]
    async fn the_key_is_only_sent_to_the_official_host() {
        let result = OpenRouterProvider
            .query_usage("fixture-key", "https://relay.example/openrouter.ai/api/v1")
            .await
            .unwrap();
        assert!(!result.success);
        assert!(!result.error.unwrap().contains("fixture-key"));
    }
}
