//! Balance of relays running a standard new-api panel.
//!
//! The public `GET /api/status` identifies the panel and tells how it displays
//! quota, so the key only goes to `GET /api/usage/token/` once the site is known
//! to be one. Panels moved or modified away from these paths are not recognised
//! and keep showing no usage data.

use super::{now_millis, parse_f64, usage_status_error, UsageQuota, UsageResult};
use reqwest::{RequestBuilder, StatusCode};
use serde_json::Value;
use std::time::Duration;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const USER_AGENT: &str = concat!("EchoBird/", env!("CARGO_PKG_VERSION"));
const MAX_UNIT_CHARS: usize = 8;
const TOKENS_UNIT: &str = "Tokens";

pub(super) fn site_origin(base_url: &str) -> Option<String> {
    let url = url::Url::parse(base_url).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.origin().ascii_serialization())
}

/// How the panel turns stored quota into the amount its own pages show.
struct DisplayUnit {
    quota_per_amount: f64,
    unit: String,
}

fn positive(data: &Value, field: &str) -> Option<f64> {
    data.get(field)
        .and_then(parse_f64)
        .filter(|value| *value > 0.0)
}

fn parse_display_unit(status: &Value) -> Option<DisplayUnit> {
    if status.get("success")?.as_bool() != Some(true) {
        return None;
    }
    let data = status.get("data")?;
    let quota_per_unit = positive(data, "quota_per_unit")?;
    let shows_currency = data.get("display_in_currency").and_then(Value::as_bool);
    let display_type = data
        .get("quota_display_type")
        .and_then(Value::as_str)
        .unwrap_or(if shows_currency == Some(false) {
            "TOKENS"
        } else {
            "USD"
        });
    let (quota_per_amount, unit) = match display_type.to_ascii_uppercase().as_str() {
        "USD" => (quota_per_unit, "USD".to_string()),
        "CNY" => (
            quota_per_unit / positive(data, "usd_exchange_rate")?,
            "CNY".to_string(),
        ),
        "CUSTOM" => (
            quota_per_unit / positive(data, "custom_currency_exchange_rate").unwrap_or(1.0),
            data.get("custom_currency_symbol")
                .and_then(Value::as_str)
                .filter(|symbol| {
                    !symbol.trim().is_empty() && symbol.chars().count() <= MAX_UNIT_CHARS
                })
                .unwrap_or("¤")
                .to_string(),
        ),
        "TOKENS" => (1.0, TOKENS_UNIT.to_string()),
        _ => return None,
    };
    Some(DisplayUnit {
        quota_per_amount,
        unit,
    })
}

/// What the key can still spend: its own quota capped by its owner's, when the
/// panel tells it. An unlimited key without its owner's quota has no known balance.
fn parse_remaining_quota(usage: &Value) -> Option<f64> {
    let data = usage.get("data")?;
    let unlimited = data.get("unlimited_quota").and_then(Value::as_bool) == Some(true);
    let key = if unlimited {
        None
    } else {
        Some(data.get("total_available").and_then(parse_f64)?)
    };
    let owner = data
        .pointer("/user_quota/total_available")
        .and_then(parse_f64);
    match (key, owner) {
        (Some(key), Some(owner)) => Some(key.min(owner)),
        (key, owner) => key.or(owner),
    }
}

/// An unlimited key on a panel that keeps its owner's quota private has no
/// balance to tell; what it has spent is the one figure left to show.
fn spent_quota(usage: &Value, display: &DisplayUnit) -> Option<UsageQuota> {
    let data = usage.get("data")?;
    if data.get("unlimited_quota").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let used = data.get("total_used").and_then(parse_f64)?;
    let quota = balance_quota(used, display)?;
    Some(UsageQuota {
        spent: true,
        ..quota
    })
}

/// Token counts run to many digits, which the balance line has no room for.
fn balance_quota(quota: f64, display: &DisplayUnit) -> Option<UsageQuota> {
    let amount = quota / display.quota_per_amount;
    if display.unit != TOKENS_UNIT {
        return UsageQuota::balance(amount, &display.unit);
    }
    match amount.abs() {
        size if size >= 1e9 => UsageQuota::balance(amount / 1e9, "B Tokens"),
        size if size >= 1e6 => UsageQuota::balance(amount / 1e6, "M Tokens"),
        size if size >= 1e3 => UsageQuota::balance(amount / 1e3, "K Tokens"),
        _ => UsageQuota::balance(amount, TOKENS_UNIT),
    }
}

/// The usage query keeps answering for an expired key, which can spend nothing.
fn key_has_expired(usage: &Value) -> bool {
    usage
        .pointer("/data/expires_at")
        .and_then(Value::as_i64)
        .is_some_and(|expires_at| expires_at > 0 && expires_at * 1000 <= now_millis())
}

fn is_refusal(usage: &Value) -> bool {
    let refused = |field| usage.get(field).and_then(Value::as_bool) == Some(false);
    refused("code") || refused("success")
}

fn get(client: &reqwest::Client, url: String) -> RequestBuilder {
    client
        .get(url)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
}

async fn read_usage(api_key: &str, origin: &str) -> UsageResult {
    let client = reqwest::Client::new();
    let Ok(response) = get(&client, format!("{origin}/api/status")).send().await else {
        return UsageResult::no_data();
    };
    if !response.status().is_success() {
        return UsageResult::no_data();
    }
    let Ok(status) = response.json::<Value>().await else {
        return UsageResult::no_data();
    };
    let Some(display) = parse_display_unit(&status) else {
        return UsageResult::no_data();
    };

    let Ok(response) = get(&client, format!("{origin}/api/usage/token/"))
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await
    else {
        return UsageResult::failure("Unable to reach the usage API");
    };
    match response.status() {
        StatusCode::NOT_FOUND => return UsageResult::no_data(),
        status if !status.is_success() => return UsageResult::failure(usage_status_error(status)),
        _ => {}
    }
    // Anything but the panel's own two reply shapes is a modified panel.
    let Ok(usage) = response.json::<Value>().await else {
        return UsageResult::no_data();
    };
    if is_refusal(&usage) {
        return UsageResult::failure("Usage API rejected the request");
    }
    if usage.get("code").and_then(Value::as_bool) != Some(true) {
        return UsageResult::no_data();
    }
    if key_has_expired(&usage) {
        return UsageResult::failure("The API key has expired");
    }
    let quota = parse_remaining_quota(&usage)
        .and_then(|quota| balance_quota(quota, &display))
        .or_else(|| spent_quota(&usage, &display));
    match quota {
        Some(quota) => UsageResult::from_quotas(vec![quota]),
        None => UsageResult::no_data(),
    }
}

pub(super) async fn query_usage(api_key: &str, base_url: &str) -> UsageResult {
    match site_origin(base_url) {
        Some(origin) => read_usage(api_key, &origin).await,
        None => UsageResult::no_data(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::relay_subscription::RelaySubscriptionProvider;
    use super::super::{UsageProvider, UsageResult};
    use serde_json::{json, Value};
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    type Requests = Arc<Mutex<Vec<(String, Option<String>)>>>;

    struct Relay {
        origin: String,
        requests: Requests,
    }

    impl Relay {
        fn start(route: impl Fn(&str) -> (u16, String) + Send + 'static) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            let requests = Requests::default();
            let recorded = requests.clone();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = stream.unwrap();
                    let mut request = Vec::new();
                    let mut chunk = [0; 4096];
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        match stream.read(&mut chunk) {
                            Ok(length) if length > 0 => request.extend_from_slice(&chunk[..length]),
                            _ => break,
                        }
                    }
                    let request = String::from_utf8_lossy(&request).to_string();
                    let path = request
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_string();
                    let authorization = request
                        .lines()
                        .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
                        .map(|line| line[14..].trim().to_string());
                    recorded.lock().unwrap().push((path.clone(), authorization));
                    let (status, body) = route(&path);
                    if status == 0 {
                        continue;
                    }
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} Response\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
            Self { origin, requests }
        }

        fn paths(&self) -> Vec<String> {
            let requests = self.requests.lock().unwrap();
            requests.iter().map(|(path, _)| path.clone()).collect()
        }

        fn count(&self, path: &str) -> usize {
            self.paths().iter().filter(|seen| *seen == path).count()
        }
    }

    fn unique_key() -> String {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        format!("sk-fixture-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn status_body(site: Value) -> String {
        json!({"success": true, "data": site}).to_string()
    }

    fn token_body(data: Value) -> String {
        json!({"code": true, "message": "ok", "data": data}).to_string()
    }

    fn new_api(site: Value, data: Value) -> Relay {
        let status = status_body(site);
        let usage = token_body(data);
        Relay::start(move |path| match path {
            "/api/status" => (200, status.clone()),
            "/api/usage/token/" => (200, usage.clone()),
            _ => (404, r#"{"error":{"message":"not found"}}"#.to_string()),
        })
    }

    async fn query(relay: &Relay, key: &str) -> UsageResult {
        RelaySubscriptionProvider
            .query_usage(key, &format!("{}/v1", relay.origin))
            .await
            .unwrap()
    }

    fn balance(result: &UsageResult) -> (f64, String) {
        let quotas = &result.data.as_ref().expect("usage data").quotas;
        assert_eq!(quotas.len(), 1);
        assert_eq!(quotas[0].reset_at, 0);
        assert_eq!(quotas[0].period, None);
        (
            quotas[0].balance.expect("balance"),
            quotas[0].balance_unit.clone().expect("unit"),
        )
    }

    fn assert_no_data(result: &UsageResult) {
        assert!(!result.success);
        assert!(result.data.is_none());
        assert!(result.error.is_none(), "unexpected {:?}", result.error);
    }

    #[tokio::test]
    async fn balances_follow_the_display_settings_of_the_site() {
        for (site, expected, unit) in [
            (
                json!({"quota_per_unit":500000,"quota_display_type":"USD"}),
                3.0,
                "USD",
            ),
            (
                json!({"quota_per_unit":1000000,"quota_display_type":"USD"}),
                1.5,
                "USD",
            ),
            (
                json!({"quota_per_unit":500000,"quota_display_type":"CNY","usd_exchange_rate":1}),
                3.0,
                "CNY",
            ),
            (
                json!({"quota_per_unit":500000,"quota_display_type":"CNY","usd_exchange_rate":6.71}),
                20.13,
                "CNY",
            ),
            (
                json!({"quota_per_unit":500000,"quota_display_type":"CUSTOM",
                    "custom_currency_symbol":"€","custom_currency_exchange_rate":0.9}),
                2.7,
                "€",
            ),
            (
                json!({"quota_per_unit":500000,"quota_display_type":"CUSTOM",
                    "custom_currency_symbol":"","custom_currency_exchange_rate":0}),
                3.0,
                "¤",
            ),
            (
                json!({"quota_per_unit":500000,"quota_display_type":"TOKENS"}),
                1.5,
                "M Tokens",
            ),
            // Panels from before quota_display_type only tell whether they show currency.
            (
                json!({"quota_per_unit":500000,"display_in_currency":true}),
                3.0,
                "USD",
            ),
            (
                json!({"quota_per_unit":500000,"display_in_currency":false}),
                1.5,
                "M Tokens",
            ),
        ] {
            let relay = new_api(
                site,
                json!({"total_available":1500000,"unlimited_quota":false}),
            );
            let key = unique_key();
            let result = query(&relay, &key).await;
            let (amount, shown_unit) = balance(&result);
            assert!((amount - expected).abs() < 1e-9, "{amount} != {expected}");
            assert_eq!(shown_unit, unit);

            let requests = relay.requests.lock().unwrap();
            let status = requests.iter().find(|(path, _)| path == "/api/status");
            assert_eq!(status.unwrap().1, None, "the status query stays anonymous");
            let usage = requests
                .iter()
                .find(|(path, _)| path == "/api/usage/token/");
            assert_eq!(usage.unwrap().1, Some(format!("Bearer {key}")));
        }
    }

    #[tokio::test]
    async fn the_balance_is_what_the_key_can_still_spend() {
        for (data, expected) in [
            (
                json!({"total_available":1000000,"unlimited_quota":false}),
                2.0,
            ),
            (
                json!({"total_available":500000,"unlimited_quota":false,
                    "user_quota":{"total_available":2500000}}),
                1.0,
            ),
            (
                json!({"total_available":2500000,"unlimited_quota":false,
                    "user_quota":{"total_available":500000}}),
                1.0,
            ),
            (
                json!({"total_available":0,"unlimited_quota":true,
                    "user_quota":{"total_available":1500000}}),
                3.0,
            ),
            (
                json!({"total_available":0,"unlimited_quota":false,
                    "user_quota":{"total_available":1500000}}),
                0.0,
            ),
        ] {
            let site = json!({"quota_per_unit":500000,"quota_display_type":"USD"});
            let relay = new_api(site, data);
            let result = query(&relay, &unique_key()).await;
            let (amount, unit) = balance(&result);
            assert!((amount - expected).abs() < 1e-9, "{amount} != {expected}");
            assert_eq!(unit, "USD");
        }
    }

    #[tokio::test]
    async fn an_unknown_balance_is_not_shown_as_zero() {
        let site = || json!({"quota_per_unit":500000,"quota_display_type":"USD"});
        for data in [
            json!({"total_available":0,"unlimited_quota":true}),
            json!({"total_available":0,"unlimited_quota":true,"user_quota":null}),
            json!({"unlimited_quota":false}),
            json!({"total_available":"NaN","unlimited_quota":false}),
        ] {
            let relay = new_api(site(), data);
            assert_no_data(&query(&relay, &unique_key()).await);
        }
        for bad_site in [
            json!({"quota_per_unit":500000,"quota_display_type":"CNY"}),
            json!({"quota_per_unit":500000,"quota_display_type":"CNY","usd_exchange_rate":0}),
            json!({"quota_per_unit":500000,"quota_display_type":"GOLD"}),
        ] {
            let data = json!({"total_available":1500000,"unlimited_quota":false});
            let relay = new_api(bad_site, data);
            assert_no_data(&query(&relay, &unique_key()).await);
        }
    }

    #[tokio::test]
    async fn an_unlimited_key_without_its_owners_quota_shows_what_it_spent() {
        let site = json!({"quota_per_unit":500000,"quota_display_type":"CNY",
            "usd_exchange_rate":6.71});
        for (data, expected) in [
            (
                json!({"total_available":0,"total_used":0,"unlimited_quota":true}),
                0.0,
            ),
            (
                json!({"total_available":0,"total_used":1500000,"unlimited_quota":true}),
                20.13,
            ),
        ] {
            let result = query(&new_api(site.clone(), data), &unique_key()).await;
            let (amount, unit) = balance(&result);
            assert!((amount - expected).abs() < 1e-9, "{amount} != {expected}");
            assert_eq!(unit, "CNY");
            assert!(result.data.unwrap().quotas[0].spent);
        }

        // The balance stays the answer whenever the panel gives one.
        let data = json!({"total_available":0,"total_used":1500000,"unlimited_quota":true,
            "user_quota":{"total_available":500000}});
        let result = query(&new_api(site.clone(), data), &unique_key()).await;
        assert!(!result.data.unwrap().quotas[0].spent);

        // A limited key whose remainder is unreadable has not spent its way into a balance.
        let data = json!({"total_used":1500000,"unlimited_quota":false});
        assert_no_data(&query(&new_api(site, data), &unique_key()).await);
    }

    #[tokio::test]
    async fn other_sites_never_receive_the_key_on_the_usage_path() {
        for (status, body) in [
            (404, "404 page not found".to_string()),
            (200, "<html>panel</html>".to_string()),
            (200, json!({"success":true,"data":{}}).to_string()),
            (
                200,
                json!({"success":false,"data":{"quota_per_unit":500000}}).to_string(),
            ),
            (200, status_body(json!({"quota_per_unit":0}))),
            (500, status_body(json!({"quota_per_unit":500000}))),
        ] {
            let relay = Relay::start(move |path| match path {
                "/api/status" => (status, body.clone()),
                _ => (404, "not found".to_string()),
            });
            assert_no_data(&query(&relay, &unique_key()).await);
            assert_eq!(relay.count("/api/status"), 1);
            assert_eq!(relay.count("/api/usage/token/"), 0);
        }
    }

    #[tokio::test]
    async fn a_relay_answering_the_existing_usage_query_is_not_probed() {
        let relay = Relay::start(|path| match path {
            "/v1/usage" => (
                200,
                json!({"mode":"unrestricted","balance":12.5,"unit":"USD"}).to_string(),
            ),
            _ => (200, status_body(json!({"quota_per_unit":500000}))),
        });
        let result = query(&relay, &unique_key()).await;
        assert_eq!(balance(&result), (12.5, "USD".to_string()));
        assert_eq!(relay.paths(), vec!["/v1/usage".to_string()]);
    }

    #[tokio::test]
    async fn an_identified_panel_reports_real_failures_without_echoing_the_key() {
        for (status, body, expected) in [
            (401, r#"{"success":false,"message":"sk-x"}"#, "HTTP 401"),
            (403, r#"{"success":false,"message":"sk-x"}"#, "HTTP 403"),
            (429, "", "HTTP 429"),
            (500, "", "HTTP 500"),
            (200, r#"{"success":false,"message":"sk-x"}"#, "rejected"),
            (200, r#"{"code":false,"message":"sk-x"}"#, "rejected"),
        ] {
            let site = status_body(json!({"quota_per_unit":500000,"quota_display_type":"USD"}));
            let relay = Relay::start(move |path| match path {
                "/api/status" => (200, site.clone()),
                "/api/usage/token/" => (status, body.to_string()),
                _ => (404, "not found".to_string()),
            });
            let key = unique_key();
            let result = query(&relay, &key).await;
            assert!(!result.success);
            assert!(result.data.is_none());
            let error = result.error.expect("visible failure");
            assert!(error.contains(expected), "{error}");
            assert!(!error.contains("sk-"), "{error}");
        }
    }

    #[tokio::test]
    async fn a_panel_without_the_key_query_has_no_data_instead_of_an_error() {
        let site = status_body(json!({"quota_per_unit":500000}));
        let relay = Relay::start(move |path| match path {
            "/api/status" => (200, site.clone()),
            _ => (404, "not found".to_string()),
        });
        assert_no_data(&query(&relay, &unique_key()).await);
        assert_eq!(relay.count("/api/usage/token/"), 1);
    }

    #[tokio::test]
    async fn long_token_counts_and_units_stay_short_enough_to_display() {
        let tokens = json!({"quota_per_unit":500000,"quota_display_type":"TOKENS"});
        for (quota, expected, unit) in [
            (2_500_000_000_u64, 2.5, "B Tokens"),
            (125_000_000, 125.0, "M Tokens"),
            (12_000, 12.0, "K Tokens"),
            (900, 900.0, "Tokens"),
        ] {
            let data = json!({"total_available":quota,"unlimited_quota":false});
            let relay = new_api(tokens.clone(), data);
            let result = query(&relay, &unique_key()).await;
            assert_eq!(balance(&result), (expected, unit.to_string()));
        }
        let long_symbol = json!({"quota_per_unit":500000,"quota_display_type":"CUSTOM",
            "custom_currency_symbol":"a very long symbol","custom_currency_exchange_rate":1});
        let data = json!({"total_available":1500000,"unlimited_quota":false});
        let relay = new_api(long_symbol, data);
        let result = query(&relay, &unique_key()).await;
        assert_eq!(balance(&result), (3.0, "¤".to_string()));
    }

    #[tokio::test]
    async fn an_expired_key_reports_that_instead_of_a_balance() {
        let site = || json!({"quota_per_unit":500000,"quota_display_type":"USD"});
        let data = json!({"total_available":1500000,"unlimited_quota":false,"expires_at":1});
        let result = query(&new_api(site(), data), &unique_key()).await;
        assert!(result.data.is_none());
        assert!(result.error.expect("visible failure").contains("expired"));

        for expires_at in [0, 4_102_444_800_i64] {
            let data = json!({"total_available":1500000,"unlimited_quota":false,
                "expires_at":expires_at});
            let result = query(&new_api(site(), data), &unique_key()).await;
            assert_eq!(balance(&result), (3.0, "USD".to_string()));
        }
    }

    #[tokio::test]
    async fn a_modified_usage_reply_keeps_showing_no_data() {
        for body in [
            "<html>panel</html>".to_string(),
            json!({"success":true,"data":{"total_available":1500000}}).to_string(),
            json!({"total_available":1500000}).to_string(),
        ] {
            let site = status_body(json!({"quota_per_unit":500000}));
            let relay = Relay::start(move |path| match path {
                "/api/status" => (200, site.clone()),
                "/api/usage/token/" => (200, body.clone()),
                _ => (404, "not found".to_string()),
            });
            assert_no_data(&query(&relay, &unique_key()).await);
        }
    }

    #[tokio::test]
    async fn an_unreachable_relay_is_not_asked_again() {
        let relay = Relay::start(|_| (0, String::new()));
        assert_no_data(&query(&relay, &unique_key()).await);
        assert_eq!(relay.paths(), vec!["/v1/usage".to_string()]);
    }

    #[tokio::test]
    async fn a_later_refresh_asks_again_after_a_failure_or_a_top_up() {
        let site = status_body(json!({"quota_per_unit":500000,"quota_display_type":"USD"}));
        let reads = AtomicUsize::new(0);
        let relay = Relay::start(move |path| match path {
            "/api/status" => (200, site.clone()),
            "/api/usage/token/" => match reads.fetch_add(1, Ordering::Relaxed) {
                0 => (429, String::new()),
                read => (
                    200,
                    token_body(json!({"total_available":read * 500000,"unlimited_quota":false})),
                ),
            },
            _ => (404, "not found".to_string()),
        });
        let key = unique_key();
        assert!(query(&relay, &key).await.error.unwrap().contains("429"));
        assert_eq!(balance(&query(&relay, &key).await).0, 1.0);
        assert_eq!(balance(&query(&relay, &key).await).0, 2.0);
    }

    #[tokio::test]
    async fn every_model_on_one_key_shares_a_single_rate_limited_read() {
        let relay = new_api(
            json!({"quota_per_unit":500000,"quota_display_type":"USD"}),
            json!({"total_available":1500000,"unlimited_quota":false}),
        );
        let key = unique_key();
        let results = futures_util::future::join_all((0..8).map(|_| query(&relay, &key))).await;
        for result in &results {
            assert_eq!(balance(result), (3.0, "USD".to_string()));
        }
        assert_eq!(relay.count("/v1/usage"), 1);
        assert_eq!(relay.count("/api/usage/token/"), 1);

        let other = unique_key();
        assert!(query(&relay, &other).await.success);
        assert_eq!(relay.count("/api/usage/token/"), 2);
    }

    #[tokio::test]
    async fn standard_endpoint_forms_read_from_the_site_root() {
        let relay = new_api(
            json!({"quota_per_unit":500000,"quota_display_type":"USD"}),
            json!({"total_available":1500000,"unlimited_quota":false}),
        );
        for suffix in ["", "/", "/v1", "/v1/", "/v1?x=1#fragment"] {
            let base = format!("{}{suffix}", relay.origin);
            let result = RelaySubscriptionProvider
                .query_usage(&unique_key(), &base)
                .await
                .unwrap();
            assert!(result.success, "{base}");
        }
        assert_eq!(relay.count("/api/usage/token/"), 5);
    }

    #[tokio::test]
    async fn urls_with_embedded_credentials_are_not_followed() {
        let relay = new_api(
            json!({"quota_per_unit":500000}),
            json!({"total_available":1,"unlimited_quota":false}),
        );
        let base = relay.origin.replace("http://", "http://user:secret@");
        let result = RelaySubscriptionProvider
            .query_usage(&unique_key(), &format!("{base}/v1"))
            .await
            .unwrap();
        assert_no_data(&result);
        assert_eq!(relay.count("/api/status"), 0);
    }
}
