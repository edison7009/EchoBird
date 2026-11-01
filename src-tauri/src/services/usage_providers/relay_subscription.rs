//! Relay subscription usage provider
//!
//! Compatible relay/proxy endpoints expose subscription or wallet usage.
//! GET {base}/v1/usage with Bearer api-key returns quota mode:
//! { planName, unit, subscription: { daily_limit_usd, ... }, usage: { total: {...} } }
//! or wallet mode:
//! { planName, unit, mode: "unrestricted", balance, remaining, usage: {...} }
//!
//! This is a fallback provider (can_handle always true) - it sits last in
//! detect_provider so official providers (DeepSeek/Kimi/...) match first.
//! Any unmatched base_url (relay/proxy) is tried against /v1/usage; if the
//! response matches the usage schema, we render bars, otherwise (404, bad
//! key, non-JSON, wrong shape) the standard new-api queries get their turn,
//! and a site answering neither silently shows "暂无用量数据".

use super::{
    api_url, new_api, now_millis, parse_f64, parse_reset_time, ModelUsageData, QuotaPeriod,
    UsageProvider, UsageQuota, UsageResult,
};
use chrono::TimeZone;
use reqwest;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;
use tokio::sync::OnceCell;

pub struct RelaySubscriptionProvider;

/// Build the /v1/usage endpoint from a base_url.
/// Both https://relay.example/v1 and https://relay.example use /v1/usage.
fn build_usage_url(base_url: &str) -> String {
    if let Ok(mut url) = url::Url::parse(base_url) {
        url.set_path("/v1/usage");
        url.set_query(None);
        url.set_fragment(None);
        return url.to_string();
    }
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        format!("{}/usage", trimmed)
    } else {
        format!("{}/v1/usage", trimmed)
    }
}

/// Extract the first number from a plan name (e.g. "日卡 每日200刀" -> 200).
fn parse_plan_limit(plan_name: &str) -> f64 {
    let mut num = String::new();
    let mut started = false;
    for c in plan_name.chars() {
        if c.is_ascii_digit() || (c == '.' && started) {
            num.push(c);
            started = true;
        } else if started {
            break;
        }
    }
    num.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
}

/// The confirmed deployment's daily quota resets at midnight in CST (+08:00).
/// Computed from the current time rather than the response's `daily_usage`
/// date, which can be stale and yield a past reset (-> "0m" countdown).
fn daily_reset_ms() -> i64 {
    let cst = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let now_cst = chrono::Utc::now().with_timezone(&cst);
    let tomorrow = now_cst.date_naive() + chrono::Duration::days(1);
    // FixedOffset has no DST, so from_local_datetime is always Single.
    cst.from_local_datetime(&tomorrow.and_hms_opt(0, 0, 0).unwrap())
        .unwrap()
        .timestamp_millis()
}

/// Build one quota bar per enforced limit found in the `subscription` object
/// (daily/weekly/monthly). The response contains only limits a plan enforces,
/// so a 天卡 yields 1 bar (daily) and a 月卡 yields 3 (daily+weekly+monthly).
/// Missing usage remains unknown. Only the confirmed deployment uses
/// today's API-key spend to work around its stale daily subscription aggregate.
fn parse_subscription_quotas(body: &serde_json::Value, base_url: &str) -> Option<Vec<UsageQuota>> {
    let sub = body.get("subscription")?;
    let mut quotas: Vec<UsageQuota> = Vec::new();
    let week_ms = 7 * 24 * 60 * 60 * 1000;
    let use_today_cost = api_url(base_url, &["cc-vibe.com"])
        .is_some_and(|url| url.port_or_known_default() == Some(443));
    let aggregate_daily = sub.get("daily_usage_usd").and_then(parse_f64);
    let daily_usage = if use_today_cost {
        match body.pointer("/usage/today/actual_cost") {
            Some(today) => parse_f64(today),
            None => aggregate_daily,
        }
    } else {
        aggregate_daily
    };
    let weekly_reset = sub
        .get("weekly_window_start")
        .and_then(parse_reset_time)
        .and_then(|start| start.checked_add(week_ms));

    for (period, limit_field, usage, reset) in [
        (
            QuotaPeriod::Daily,
            "daily_limit_usd",
            daily_usage,
            use_today_cost.then(daily_reset_ms),
        ),
        (
            QuotaPeriod::Weekly,
            "weekly_limit_usd",
            sub.get("weekly_usage_usd").and_then(parse_f64),
            weekly_reset,
        ),
        // Subscription expiry is not the monthly quota-window reset. The
        // upstream response exposes no monthly_window_start or reset time.
        (
            QuotaPeriod::Monthly,
            "monthly_limit_usd",
            sub.get("monthly_usage_usd").and_then(parse_f64),
            None,
        ),
    ] {
        if let (Some(limit), Some(usage)) = (
            sub.get(limit_field)
                .and_then(parse_f64)
                .filter(|limit| *limit > 0.0),
            usage,
        ) {
            if let Some(quota) = UsageQuota::window(period, usage / limit * 100.0, reset) {
                quotas.push(quota);
            }
        }
    }

    if quotas.is_empty() {
        None
    } else {
        Some(quotas)
    }
}

fn parse_balance_quota(body: &serde_json::Value) -> Option<UsageQuota> {
    if body
        .get("subscription")
        .is_some_and(|value| !value.is_null())
        || body.get("mode").and_then(|value| value.as_str()) == Some("quota_limited")
    {
        return None;
    }
    let balance = body.get("balance").and_then(parse_f64).or_else(|| {
        body.get("remaining")
            .and_then(parse_f64)
            .filter(|value| *value >= 0.0)
    })?;
    let unit = body
        .get("unit")
        .or_else(|| body.get("currency"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("USD");

    UsageQuota::balance(balance, unit)
}

/// The relay could not be reached at all, so no other query is worth waiting for.
struct Unreachable;

type SharedRead = Arc<OnceCell<UsageResult>>;

/// Models on one key refresh together, and relays limit these queries per IP,
/// so requests running at the same time share one read. Nothing outlives it:
/// the next refresh always asks again.
async fn share_read(
    origin: &str,
    api_key: &str,
    read: impl Future<Output = UsageResult>,
) -> UsageResult {
    static READS: OnceLock<Mutex<HashMap<String, SharedRead>>> = OnceLock::new();
    let reads = READS.get_or_init(Mutex::default);
    let key = format!("{origin}\n{:x}", Sha256::digest(api_key.as_bytes()));
    let shared = reads
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(key.clone())
        .or_default()
        .clone();
    let usage = shared.get_or_init(|| read).await.clone();
    let mut reads = reads.lock().unwrap_or_else(PoisonError::into_inner);
    if reads
        .get(&key)
        .is_some_and(|current| Arc::ptr_eq(current, &shared))
    {
        reads.remove(&key);
    }
    usage
}

impl RelaySubscriptionProvider {
    async fn query_compatible_usage(
        &self,
        api_key: &str,
        base_url: &str,
    ) -> Result<UsageResult, Unreachable> {
        let usage_url = build_usage_url(base_url);
        let client = reqwest::Client::new();
        let resp = match client
            .get(&usage_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Accept", "application/json")
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(r) => r,
            // DNS / network / timeout: the site is down, whatever it runs.
            Err(_) => return Err(Unreachable),
        };

        // Any non-2xx (404, 401/403, 5xx, ...) -> no data. As a guess we can't
        // tell a bad key from an incompatible endpoint, so this query surfaces no
        // specific error; the UI shows "暂无用量数据" unless another query answers.
        if !resp.status().is_success() {
            return Ok(UsageResult::no_data());
        }

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            // 200 but not JSON / wrong shape -> no data.
            Err(_) => return Ok(UsageResult::no_data()),
        };

        // Prefer the structured `subscription` object: it carries explicit
        // per-window limits (daily/weekly/monthly), so we render one bar per
        // enforced limit (天卡=daily only, 月卡=daily+weekly+monthly).
        if let Some(quotas) = parse_subscription_quotas(&body, base_url) {
            return Ok(UsageResult {
                success: true,
                data: Some(ModelUsageData {
                    quotas,
                    last_updated: Some(now_millis()),
                }),
                error: None,
            });
        }

        // An unlimited/malformed subscription is not a wallet; remaining=-1
        // means no enforced quota, not a negative monetary balance.
        if body
            .get("subscription")
            .is_some_and(|value| !value.is_null())
            || body.get("mode").and_then(|value| value.as_str()) == Some("quota_limited")
        {
            return Ok(UsageResult::no_data());
        }

        // Wallet / API-call mode exposes a remaining balance instead of enforced
        // daily/weekly/monthly limits. Return a balance quota so the existing UI
        // takes the same display path as DeepSeek, without changing quota bars.
        if let Some(quota) = parse_balance_quota(&body) {
            return Ok(UsageResult {
                success: true,
                data: Some(ModelUsageData {
                    quotas: vec![quota],
                    last_updated: Some(now_millis()),
                }),
                error: None,
            });
        }

        // Legacy fallback without `subscription`: single bar from
        // total cost vs the limit parsed out of the plan name. Only emit a bar
        // when we actually parsed a limit; otherwise the 200 isn't usage
        // data -> no data (avoids a bogus 0% bar on incompatible endpoints).
        let plan_name = body
            .get("planName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let plan_limit = parse_plan_limit(&plan_name);
        if plan_limit <= 0.0 {
            return Ok(UsageResult::no_data());
        }
        let Some(actual_cost) = body
            .get("usage")
            .and_then(|u| u.get("total"))
            .and_then(|u| u.get("actual_cost"))
            .and_then(parse_f64)
        else {
            return Ok(UsageResult::no_data());
        };
        let percentage = actual_cost / plan_limit * 100.0;
        if !percentage.is_finite() {
            return Ok(UsageResult::no_data());
        }

        Ok(UsageResult {
            success: true,
            data: Some(ModelUsageData {
                quotas: vec![UsageQuota {
                    period: None,
                    percentage: percentage.clamp(0.0, 100.0),
                    reset_at: 0,
                    balance: None,
                    balance_unit: None,
                }],
                last_updated: Some(now_millis()),
            }),
            error: None,
        })
    }

    async fn read_usage(&self, api_key: &str, base_url: &str) -> UsageResult {
        match self.query_compatible_usage(api_key, base_url).await {
            Ok(usage) if usage.success => usage,
            Ok(_) => new_api::query_usage(api_key, base_url).await,
            Err(Unreachable) => UsageResult::no_data(),
        }
    }
}

#[async_trait::async_trait]
impl UsageProvider for RelaySubscriptionProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        let read = self.read_usage(api_key, base_url);
        // Both queries read from the site root, so the site identifies the read.
        Ok(match new_api::site_origin(base_url) {
            Some(origin) => share_read(&origin, api_key, read).await,
            None => read.await,
        })
    }

    fn can_handle(&self, _base_url: &str) -> bool {
        // Fallback: try any base_url not matched by official providers above.
        true
    }

    fn name(&self) -> &'static str {
        "Relay Subscription"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn monthly_card_yields_three_bars() {
        let body = json!({
            "planName": "月卡 每日200刀",
            "unit": "USD",
            "subscription": {
                "daily_limit_usd": 200, "daily_usage_usd": 96.12,
                "weekly_limit_usd": 1400, "weekly_usage_usd": 96.12,
                "monthly_limit_usd": 6000, "monthly_usage_usd": 96.12,
                "weekly_window_start": "2026-07-14T00:00:00+08:00",
                "expires_at": "2026-07-17T17:27:45+08:00"
            },
            "daily_usage": [{ "date": "2026-07-14" }],
            "usage": { "total": { "actual_cost": 96.12 } }
        });
        let quotas = parse_subscription_quotas(&body, "https://relay.example/v1")
            .expect("subscription present");
        assert_eq!(quotas.len(), 3);
        assert!((quotas[0].percentage - 48.06).abs() < 0.1);
        assert!((quotas[1].percentage - 6.87).abs() < 0.1);
        assert!((quotas[2].percentage - 1.60).abs() < 0.1);
    }

    #[test]
    fn day_card_yields_one_bar_when_only_daily_limit() {
        let body = json!({
            "planName": "天卡 每日200刀",
            "subscription": {
                "daily_limit_usd": 200, "daily_usage_usd": 50.0
            },
            "daily_usage": [{ "date": "2026-07-14" }]
        });
        let quotas = parse_subscription_quotas(&body, "https://relay.example/v1")
            .expect("subscription present");
        assert_eq!(quotas.len(), 1);
        assert!((quotas[0].percentage - 25.0).abs() < 0.01);
    }

    #[test]
    fn zero_or_absent_limits_are_skipped() {
        let body = json!({
            "subscription": {
                "daily_limit_usd": 200, "daily_usage_usd": 10.0,
                "weekly_limit_usd": 0, "monthly_limit_usd": 0
            },
            "daily_usage": [{ "date": "2026-07-14" }]
        });
        let quotas = parse_subscription_quotas(&body, "https://relay.example/v1")
            .expect("subscription present");
        assert_eq!(quotas.len(), 1);
    }

    #[test]
    fn wallet_mode_yields_balance_display() {
        let body = json!({
            "balance": 5384.70840195,
            "remaining": 5384.70840195,
            "isValid": true,
            "mode": "unrestricted",
            "planName": "Wallet balance",
            "unit": "USD",
            "usage": {
                "today": { "actual_cost": 203.944362 },
                "total": { "actual_cost": 286.406855 }
            }
        });
        let quota = parse_balance_quota(&body).expect("balance present");
        assert_eq!(quota.balance_unit.as_deref(), Some("USD"));
        assert!((quota.balance.unwrap_or_default() - 5384.70840195).abs() < 0.000001);
    }

    #[test]
    fn wallet_mode_can_use_balance_when_remaining_absent() {
        let body = json!({
            "balance": "12.50",
            "planName": "Wallet balance",
            "unit": "CNY"
        });
        let quota = parse_balance_quota(&body).expect("balance present");
        assert_eq!(quota.balance_unit.as_deref(), Some("CNY"));
        assert!((quota.balance.unwrap_or_default() - 12.5).abs() < 0.000001);
    }

    #[test]
    fn no_subscription_falls_back_to_none() {
        let body = json!({
            "planName": "X",
            "usage": { "total": { "actual_cost": 10.0 } }
        });
        assert!(parse_subscription_quotas(&body, "https://relay.example/v1").is_none());
    }

    #[test]
    fn fresh_day_with_zero_today_is_0_pct_not_100() {
        // The confirmed deployment on a new day with no usage yet: the upstream
        // leaves `subscription.daily_usage_usd` holding YESTERDAY's spend
        // (200.708 against a 200 limit) while `usage.today.actual_cost` is the
        // authoritative 0. We must read today's figure, else the bar wrongly
        // shows 100%.
        let body = json!({
            "planName": "月卡 每日200刀",
            "subscription": {
                "daily_limit_usd": 200, "daily_usage_usd": 200.7082975,
                "weekly_limit_usd": 1400, "weekly_usage_usd": 296.8321385,
                "monthly_limit_usd": 6000, "monthly_usage_usd": 296.8321385,
                "weekly_window_start": "2026-07-14T00:00:00+08:00",
                "expires_at": "2026-07-17T17:27:45+08:00"
            },
            "daily_usage": [
                { "date": "2026-07-14", "actual_cost": 96.123841 },
                { "date": "2026-07-15", "actual_cost": 200.7082975 }
            ],
            "usage": {
                "today": { "actual_cost": 0 },
                "total": { "actual_cost": 296.8321385 }
            }
        });
        let quotas = parse_subscription_quotas(&body, "https://cc-vibe.com/v1")
            .expect("subscription present");
        assert_eq!(quotas.len(), 3);
        // Daily bar must reflect today's 0 usage, not the stale 200.708.
        assert!(
            (quotas[0].percentage - 0.0).abs() < 0.01,
            "daily should be 0%, got {}",
            quotas[0].percentage
        );
        // Weekly/monthly are cumulative-over-window and keep including past days.
        assert!((quotas[1].percentage - 21.20).abs() < 0.1);
        assert!((quotas[2].percentage - 4.95).abs() < 0.1);
    }

    #[test]
    fn daily_falls_back_to_daily_usage_usd_when_today_absent() {
        // Older compatible endpoints expose no `usage.today`; keep using
        // `subscription.daily_usage_usd` so we don't regress those sites.
        let body = json!({
            "planName": "天卡 每日200刀",
            "subscription": {
                "daily_limit_usd": 200, "daily_usage_usd": 50.0
            },
            "daily_usage": [{ "date": "2026-07-14" }]
        });
        let quotas = parse_subscription_quotas(&body, "https://cc-vibe.com/v1")
            .expect("subscription present");
        assert_eq!(quotas.len(), 1);
        assert!((quotas[0].percentage - 25.0).abs() < 0.01);
    }

    #[test]
    fn usage_endpoint_preserves_ports_ipv6_and_removes_query_and_fragment() {
        for (base, endpoint) in [
            (
                "https://relay.example:8443/v1",
                "https://relay.example:8443/v1/usage",
            ),
            (
                "http://127.0.0.1:8765/v1?x=1#usage",
                "http://127.0.0.1:8765/v1/usage",
            ),
            ("http://[::1]:8765/v1", "http://[::1]:8765/v1/usage"),
        ] {
            assert_eq!(build_usage_url(base), endpoint);
        }
    }

    #[test]
    fn generic_subscription_daily_usage_is_aggregate_not_one_api_keys_today_cost() {
        let body = json!({
            "subscription":{"daily_limit_usd":200,"daily_usage_usd":150},
            "usage":{"today":{"actual_cost":10}}
        });
        for base in [
            "https://relay.example/v1",
            "https://cc-vibe.com.relay.example/v1",
            "https://relay.example/cc-vibe.com/v1",
            "https://cc-vibe.com:8443/v1",
            "http://cc-vibe.com/v1",
        ] {
            let quotas = parse_subscription_quotas(&body, base).unwrap();
            assert_eq!(quotas[0].percentage, 75.0);
            assert_eq!(quotas[0].reset_at, 0);
        }
        let quotas = parse_subscription_quotas(&body, "https://cc-vibe.com/v1").unwrap();
        assert_eq!(quotas[0].percentage, 5.0);
        assert!(quotas[0].reset_at > now_millis());
    }

    #[test]
    fn monthly_expiry_is_not_a_reset_and_missing_weekly_anchor_is_unknown() {
        let body = json!({"subscription":{
            "weekly_limit_usd":100,"weekly_usage_usd":25,
            "monthly_limit_usd":400,"monthly_usage_usd":50,
            "expires_at":"2027-10-08T00:00:00Z"
        }});
        let quotas = parse_subscription_quotas(&body, "https://relay.example/v1").unwrap();
        assert_eq!(quotas.len(), 2);
        assert_eq!(quotas[0].reset_at, 0);
        assert_eq!(quotas[1].reset_at, 0);
        assert_eq!(quotas[1].percentage, 12.5);
    }

    #[test]
    fn invalid_or_absent_subscription_usage_is_not_fabricated_as_zero() {
        for body in [
            json!({"subscription":{"daily_limit_usd":200}}),
            json!({"subscription":{"weekly_limit_usd":200,"weekly_usage_usd":"NaN"}}),
            json!({"subscription":{"monthly_limit_usd":"inf","monthly_usage_usd":1}}),
            json!({"subscription":{"daily_limit_usd":1e-300,"daily_usage_usd":1e300}}),
        ] {
            assert!(parse_subscription_quotas(&body, "https://relay.example/v1").is_none());
        }
        let zero = json!({"subscription":{"daily_limit_usd":200,"daily_usage_usd":0}});
        assert_eq!(
            parse_subscription_quotas(&zero, "https://relay.example/v1").unwrap()[0].percentage,
            0.0
        );
        let invalid_today = json!({
            "subscription":{"daily_limit_usd":200,"daily_usage_usd":50},
            "usage":{"today":{"actual_cost":"NaN"}}
        });
        assert!(parse_subscription_quotas(&invalid_today, "https://cc-vibe.com/v1").is_none());
    }

    #[test]
    fn unlimited_subscription_sentinel_and_api_key_budget_are_not_wallet_balances() {
        for body in [
            json!({"mode":"subscription","remaining":-1,"unit":"USD","subscription":{}}),
            json!({"mode":"quota_limited","remaining":100,"balance":100,"unit":"USD"}),
            json!({"remaining":-1,"unit":"USD"}),
            json!({"balance":"NaN","unit":"USD"}),
        ] {
            assert!(parse_balance_quota(&body).is_none());
        }
        let quota = parse_balance_quota(
            &json!({"mode":"unrestricted","balance":0,"remaining":0,"unit":"USD"}),
        )
        .unwrap();
        assert_eq!(quota.balance, Some(0.0));
        assert_eq!(quota.reset_at, 0);
    }
}
