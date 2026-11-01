//! Usage query providers
//!
//! Each provider is implemented in its own module for easy maintenance and extension.

use serde::{Deserialize, Serialize};

pub mod deepseek;
pub mod kimi;
pub mod minimax;
pub mod moonshot;
pub mod new_api;
pub mod novita;
pub mod opencode;
pub mod openrouter;
pub mod relay_subscription;
pub mod siliconflow;
pub mod stepfun;
pub mod volcengine;
pub mod zenmux;
pub mod zhipu;
pub mod zhipu_team;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaPeriod {
    FiveHour,
    Daily,
    Weekly,
    Monthly,
}

/// Single usage quota data (progress bar)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageQuota {
    pub percentage: f64, // Used percentage, 0-100
    pub reset_at: i64,   // Unix timestamp (ms); 0 when the reset time is unknown
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period: Option<QuotaPeriod>,
    // Balance display (for providers like DeepSeek that show remaining balance)
    pub balance: Option<f64>,         // Remaining balance (e.g., 10.50 USD)
    pub balance_unit: Option<String>, // Currency unit (e.g., "USD", "CNY", "Credits")
    // The amount is what was spent, for keys that have no balance to report.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spent: bool,
}

impl UsageQuota {
    fn balance(amount: f64, unit: &str) -> Option<Self> {
        (amount.is_finite() && !unit.trim().is_empty()).then(|| Self {
            percentage: 0.0,
            reset_at: 0,
            period: None,
            balance: Some(amount),
            balance_unit: Some(unit.to_string()),
            spent: false,
        })
    }

    fn window(period: QuotaPeriod, used: f64, reset_at: Option<i64>) -> Option<Self> {
        used.is_finite().then(|| Self {
            percentage: used.clamp(0.0, 100.0),
            reset_at: reset_at.filter(|value| *value > 0).unwrap_or(0),
            period: Some(period),
            balance: None,
            balance_unit: None,
            spent: false,
        })
    }
}

/// Model usage data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageData {
    pub quotas: Vec<UsageQuota>,
    pub last_updated: Option<i64>, // Unix timestamp (ms)
}

/// Usage query result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageResult {
    pub success: bool,
    pub data: Option<ModelUsageData>,
    pub error: Option<String>,
}

impl UsageResult {
    fn from_quotas(quotas: Vec<UsageQuota>) -> Self {
        if quotas.is_empty() {
            return Self::failure("No usage data available");
        }
        Self {
            success: true,
            data: Some(ModelUsageData {
                quotas,
                last_updated: Some(now_millis()),
            }),
            error: None,
        }
    }

    /// Nothing to show and nothing to report: the UI says "暂无用量数据" rather
    /// than raising an error, for sites that simply do not offer this query.
    fn no_data() -> Self {
        Self {
            success: false,
            data: None,
            error: None,
        }
    }

    fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
        }
    }
}

fn parse_reset_time(value: &serde_json::Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        if let Ok(time) = chrono::DateTime::parse_from_rfc3339(text) {
            return (time.timestamp_millis() > 0).then_some(time.timestamp_millis());
        }
    }
    let value = value.as_i64().or_else(|| value.as_str()?.parse().ok())?;
    if value <= 0 {
        return None;
    }
    Some(if value < 1_000_000_000_000 {
        value * 1000
    } else {
        value
    })
}

fn api_url(base_url: &str, hosts: &[&str]) -> Option<url::Url> {
    let url = url::Url::parse(base_url).ok()?;
    (url.scheme() == "https"
        && hosts.contains(&url.host_str()?)
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(url)
}

async fn fetch_usage(url: &str, authorization: &str) -> Result<serde_json::Value, String> {
    send_usage_request(
        reqwest::Client::new()
            .get(url)
            .header("Authorization", authorization),
    )
    .await
}

fn usage_status_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 => "Usage authentication failed (HTTP 401)".to_string(),
        403 => "Usage access denied; check the key and subscription (HTTP 403)".to_string(),
        _ => format!("Usage API error (HTTP {status})"),
    }
}

async fn send_usage_request(request: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let response = request
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| "Unable to reach the usage API".to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(usage_status_error(status));
    }
    response
        .json()
        .await
        .map_err(|_| "Invalid usage API response".to_string())
}

/// Provider trait - each provider implements this
// The attribute macro adds `must_use` to a future that already has it.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait UsageProvider {
    /// Query usage from provider API
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String>;

    /// Check if this provider can handle the given base_url
    fn can_handle(&self, base_url: &str) -> bool;

    /// Provider name for logging
    fn name(&self) -> &'static str;
}

/// Provider enum - concrete type wrapper
pub enum Provider {
    DeepSeek(deepseek::DeepSeekProvider),
    Kimi(kimi::KimiProvider),
    Moonshot(moonshot::MoonshotProvider),
    MiniMax(minimax::MiniMaxProvider),
    Novita(novita::NovitaProvider),
    OpenCode(opencode::OpenCodeProvider),
    OpenRouter(openrouter::OpenRouterProvider),
    SiliconFlow(siliconflow::SiliconFlowProvider),
    StepFun(stepfun::StepFunProvider),
    ZenMux(zenmux::ZenMuxProvider),
    Zhipu(zhipu::ZhipuProvider),
    RelaySubscription(relay_subscription::RelaySubscriptionProvider),
    Volcengine(volcengine::VolcengineProvider),
}

impl Provider {
    pub async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        match self {
            Provider::DeepSeek(p) => p.query_usage(api_key, base_url).await,
            Provider::Kimi(p) => p.query_usage(api_key, base_url).await,
            Provider::Moonshot(p) => p.query_usage(api_key, base_url).await,
            Provider::MiniMax(p) => p.query_usage(api_key, base_url).await,
            Provider::Novita(p) => p.query_usage(api_key, base_url).await,
            Provider::OpenCode(p) => p.query_usage(api_key, base_url).await,
            Provider::OpenRouter(p) => p.query_usage(api_key, base_url).await,
            Provider::SiliconFlow(p) => p.query_usage(api_key, base_url).await,
            Provider::StepFun(p) => p.query_usage(api_key, base_url).await,
            Provider::ZenMux(p) => p.query_usage(api_key, base_url).await,
            Provider::Zhipu(p) => p.query_usage(api_key, base_url).await,
            Provider::RelaySubscription(p) => p.query_usage(api_key, base_url).await,
            Provider::Volcengine(p) => p.query_usage(api_key, base_url).await,
        }
    }
}

/// Detect provider from base_url and return appropriate implementation
pub fn detect_provider(base_url: &str) -> Option<Provider> {
    let url = base_url.to_lowercase();

    if deepseek::DeepSeekProvider.can_handle(&url) {
        return Some(Provider::DeepSeek(deepseek::DeepSeekProvider));
    }
    if kimi::KimiProvider.can_handle(&url) {
        return Some(Provider::Kimi(kimi::KimiProvider));
    }
    if moonshot::MoonshotProvider.can_handle(&url) {
        return Some(Provider::Moonshot(moonshot::MoonshotProvider));
    }
    if minimax::MiniMaxProvider.can_handle(&url) {
        return Some(Provider::MiniMax(minimax::MiniMaxProvider));
    }
    if novita::NovitaProvider.can_handle(&url) {
        return Some(Provider::Novita(novita::NovitaProvider));
    }
    if opencode::OpenCodeProvider.can_handle(&url) {
        return Some(Provider::OpenCode(opencode::OpenCodeProvider));
    }
    if openrouter::OpenRouterProvider.can_handle(&url) {
        return Some(Provider::OpenRouter(openrouter::OpenRouterProvider));
    }
    if siliconflow::SiliconFlowProvider.can_handle(&url) {
        return Some(Provider::SiliconFlow(siliconflow::SiliconFlowProvider));
    }
    if stepfun::StepFunProvider.can_handle(&url) {
        return Some(Provider::StepFun(stepfun::StepFunProvider));
    }
    if zenmux::ZenMuxProvider.can_handle(&url) {
        return Some(Provider::ZenMux(zenmux::ZenMuxProvider));
    }
    if zhipu::ZhipuProvider.can_handle(&url) {
        return Some(Provider::Zhipu(zhipu::ZhipuProvider));
    }
    if volcengine::VolcengineProvider.can_handle(&url) {
        return Some(Provider::Volcengine(volcengine::VolcengineProvider));
    }
    if relay_subscription::RelaySubscriptionProvider.can_handle(&url) {
        return Some(Provider::RelaySubscription(
            relay_subscription::RelaySubscriptionProvider,
        ));
    }

    None
}

/// Main entry point - query usage for a model
pub async fn query_model_usage(
    base_url: &str,
    api_key: &str,
    internal_id: &str,
) -> Result<UsageResult, String> {
    let provider = match detect_provider(base_url) {
        Some(p) => p,
        None => {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("Provider does not support usage query".to_string()),
            });
        }
    };

    // Volcengine usage uses per-model AK/SK (keyed by internal_id), not the
    // inference api_key - route it through the per-model entrypoint so the
    // empty-api-key check below doesn't block it.
    if let Provider::Volcengine(p) = &provider {
        return p.query_usage_for_model(internal_id, base_url).await;
    }

    if api_key.trim().is_empty() {
        return Ok(UsageResult {
            success: false,
            data: None,
            error: Some("API key is empty".to_string()),
        });
    }

    if matches!(&provider, Provider::Zhipu(_)) && api_url(base_url, &["open.bigmodel.cn"]).is_some()
    {
        match zhipu_team::read_access(internal_id) {
            Ok(Some(access)) => return zhipu_team::query_usage(api_key, &access).await,
            Ok(None) => {}
            Err(error) => return Ok(UsageResult::failure(error)),
        }
    }
    provider.query_usage(api_key, base_url).await
}

/// Helper function to get current timestamp in milliseconds
pub(crate) fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Helper function to parse f64 from JSON value (supports both number and string)
pub(crate) fn parse_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
        .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn balance_payloads_keep_real_amounts_without_a_percentage_or_reset() {
        for amount in [0.0, -1.25, 5384.70840195] {
            let quota = UsageQuota::balance(amount, "USD").unwrap();
            assert_eq!(quota.balance, Some(amount));
            assert_eq!(quota.balance_unit.as_deref(), Some("USD"));
            assert_eq!(quota.period, None);
            assert_eq!(quota.percentage, 0.0);
            assert_eq!(quota.reset_at, 0);
        }
        for amount in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(UsageQuota::balance(amount, "USD").is_none());
        }
        assert!(UsageQuota::balance(1.0, " ").is_none());
        for value in [json!("NaN"), json!("inf"), json!("1e999"), json!(null)] {
            assert!(parse_f64(&value).is_none());
        }
        assert_eq!(parse_f64(&json!("0")), Some(0.0));
    }

    #[test]
    fn official_balance_hosts_do_not_match_relay_names_paths_or_plain_http() {
        for host in [
            "api.deepseek.com",
            "api.siliconflow.cn",
            "api.siliconflow.com",
            "api.stepfun.com",
            "api.stepfun.ai",
            "api.novita.ai",
            "openrouter.ai",
            "zenmux.ai",
        ] {
            assert!(!matches!(
                detect_provider(&format!("https://{host}/v1")),
                Some(Provider::RelaySubscription(_))
            ));
            for base in [
                format!("https://{host}.relay.example/v1"),
                format!("https://relay.example/{host}/v1"),
                format!("https://relay.example/v1?host={host}"),
                format!("http://{host}/v1"),
                format!("https://user@{host}/v1"),
            ] {
                assert!(matches!(
                    detect_provider(&base),
                    Some(Provider::RelaySubscription(_))
                ));
            }
        }
    }

    #[test]
    fn reset_times_accept_iso_seconds_and_milliseconds_but_not_missing_values() {
        for value in [
            json!(1800000000),
            json!(1800000000000_i64),
            json!("1800000000"),
        ] {
            assert_eq!(parse_reset_time(&value), Some(1800000000000));
        }
        assert_eq!(
            parse_reset_time(&json!("2027-01-01T12:00:00Z")),
            Some(1798804800000)
        );
        for value in [json!(null), json!(0), json!(-1), json!("invalid")] {
            assert_eq!(parse_reset_time(&value), None);
        }
    }

    #[test]
    fn old_payloads_remain_compatible_and_go_is_routed_before_the_generic_provider() {
        let quota: UsageQuota =
            serde_json::from_value(json!({"percentage":20,"resetAt":0})).unwrap();
        assert_eq!(quota.period, None);
        let payload =
            serde_json::to_value(UsageQuota::window(QuotaPeriod::Weekly, 20.0, None).unwrap())
                .unwrap();
        assert_eq!(payload["period"], "weekly");
        assert!(matches!(
            detect_provider("https://opencode.ai/zen/go/v1"),
            Some(Provider::OpenCode(_))
        ));
        assert!(!matches!(
            detect_provider("https://opencode.ai/zen/v1"),
            Some(Provider::OpenCode(_))
        ));
        assert!(!UsageResult::from_quotas(vec![]).success);
    }

    #[tokio::test]
    async fn request_errors_never_become_quota_data_or_echo_credentials() {
        use std::io::{Read, Write};
        for (status, body) in [
            (401, "test-api-key"),
            (403, "forbidden"),
            (429, "limited"),
            (200, "invalid JSON"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/usage", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                let length = stream.read(&mut request).unwrap();
                let request = String::from_utf8_lossy(&request[..length]);
                assert!(request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-api-key"));
                write!(stream, "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let error = fetch_usage(&endpoint, "Bearer test-api-key")
                .await
                .unwrap_err();
            assert!(!error.contains("test-api-key"));
            server.join().unwrap();
        }
    }

    #[tokio::test]
    async fn request_success_positive_control_sends_required_json_headers() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/balance", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let length = stream.read(&mut chunk).unwrap();
                assert!(length > 0);
                request.extend_from_slice(&chunk[..length]);
            }
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer fixture-key"));
            assert!(request.contains("accept: application/json"));
            assert!(request.contains("content-type: application/json"));
            let body = r#"{"availableBalance":"1000000"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let body = fetch_usage(&endpoint, "Bearer fixture-key").await.unwrap();
        assert_eq!(body["availableBalance"], "1000000");
        server.join().unwrap();
    }
}
