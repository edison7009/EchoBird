//! Volcengine usage provider (arkcli CLI)
//!
//! Volcengine usage requires SSO login (not api-key). EchoBird calls
//! `arkcli usage balance --type plan --format json` as a subprocess.
//! If SSO is expired, arkcli returns an error containing "SSO STS" or
//! "sso_expired"; we surface that as `error: "SSO_EXPIRED"` so the
//! frontend can show the [二次验证] button.
//!
//! BytePlus (overseas) is NOT matched here — arkcli doesn't support it.
//! BytePlus falls through to Sub2Api fallback (shows "no usage data").

use super::{ModelUsageData, UsageProvider, UsageQuota, UsageResult};

pub struct VolcengineProvider;

/// Parse RFC3339 (e.g. "2026-07-20T00:00:00+08:00") to Unix ms epoch.
fn parse_reset_at(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

#[async_trait::async_trait]
impl UsageProvider for VolcengineProvider {
    async fn query_usage(&self, _api_key: &str, _base_url: &str) -> Result<UsageResult, String> {
        // Run arkcli usage balance --type plan --format json
        let mut cmd = super::arkcli_command();
        cmd.args(["usage", "balance", "--type", "plan", "--format", "json"]);
        let output = cmd.output();

        let output = match output {
            Ok(o) => o,
            Err(_) => {
                return Ok(UsageResult {
                    success: false,
                    data: None,
                    error: Some("ARKCLI_NOT_FOUND".to_string()),
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Check for SSO expiry in stderr/stdout
        let combined = format!("{} {}", stdout, stderr);
        if combined.contains("SSO STS")
            || combined.contains("sso_expired")
            || combined.contains("NotLogin")
            || combined.contains("requires Volc SSO")
        {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("SSO_EXPIRED".to_string()),
            });
        }

        if !output.status.success() {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some(format!("arkcli error: {}", stderr.trim())),
            });
        }

        // Parse JSON response
        let body: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(e) => {
                return Ok(UsageResult {
                    success: false,
                    data: None,
                    error: Some(format!("Failed to parse arkcli response: {}", e)),
                });
            }
        };

        // Response shape: { items: [{ periods: [{ label, percent, reset_at }] }] }
        let items = body.get("items").and_then(|v| v.as_array());
        let Some(items) = items else {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("No usage items".to_string()),
            });
        };

        let mut quotas = Vec::new();
        for item in items {
            // Skip items that are not subscribed
            if item.get("error").is_some() {
                continue;
            }
            let Some(periods) = item.get("periods").and_then(|v| v.as_array()) else {
                continue;
            };
            for period in periods {
                let percent = period
                    .get("percent")
                    .and_then(super::parse_f64)
                    .unwrap_or(0.0);
                let label = period
                    .get("label")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let reset_at = period
                    .get("reset_at")
                    .and_then(|v| v.as_str())
                    .and_then(parse_reset_at)
                    .unwrap_or_else(super::now_millis);

                quotas.push(UsageQuota {
                    percentage: percent,
                    reset_at,
                    label,
                    balance: None,
                    balance_unit: None,
                });
            }
        }

        if quotas.is_empty() {
            return Ok(UsageResult {
                success: false,
                data: None,
                error: Some("No usage data available".to_string()),
            });
        }

        Ok(UsageResult {
            success: true,
            data: Some(ModelUsageData {
                quotas,
                last_updated: Some(super::now_millis()),
            }),
            error: None,
        })
    }

    fn can_handle(&self, base_url: &str) -> bool {
        let url = base_url.to_lowercase();
        url.contains("ark.cn-beijing")
            || url.contains("volcengine")
            || url.contains("volces.com")
            // BytePlus is NOT matched here (falls to Sub2Api)
    }

    fn name(&self) -> &'static str {
        "Volcengine"
    }
}
