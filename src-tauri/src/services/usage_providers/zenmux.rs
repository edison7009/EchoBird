//! ZenMux wallet and subscription queries use a separate Management API key.

use super::{
    api_url, fetch_official_usage, parse_f64, parse_reset_time, share_read, QuotaPeriod,
    UsageProvider, UsageQuota, UsageResult,
};
use crate::services::secret_store;
use serde_json::Value;
use std::{collections::HashMap, path::Path, path::PathBuf, sync::Mutex};

pub struct ZenMuxProvider;

const BALANCE_URL: &str = "https://zenmux.ai/api/v1/management/payg/balance";
const PLAN_URL: &str = "https://zenmux.ai/api/v1/management/subscription/detail";
static ACCESS_LOCK: Mutex<()> = Mutex::new(());

fn access_path() -> PathBuf {
    crate::utils::platform::echobird_dir().join("zenmux_usage_access.json")
}

fn read_access(path: &Path) -> Result<HashMap<String, String>, String> {
    match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "Invalid usage access configuration".into())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(_) => Err("Unable to read usage access configuration".into()),
    }
}

fn save_at(path: &Path, internal_id: &str, encrypted_key: Option<String>) -> Result<(), String> {
    let _lock = ACCESS_LOCK
        .lock()
        .map_err(|_| "Usage access configuration is busy")?;
    let mut access = read_access(path)?;
    if let Some(key) = encrypted_key {
        access.insert(internal_id.to_owned(), key);
    } else if access.remove(internal_id).is_none() {
        return Ok(());
    }
    let parent = path.parent().ok_or("Invalid usage access path")?;
    std::fs::create_dir_all(parent).map_err(|_| "Unable to create usage access directory")?;
    let data =
        serde_json::to_vec(&access).map_err(|_| "Unable to encode usage access configuration")?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, data).map_err(|_| "Unable to write usage access configuration")?;
    std::fs::rename(temporary, path).map_err(|_| "Unable to save usage access configuration".into())
}

pub fn save_access(internal_id: &str, key: Option<&str>) -> Result<(), String> {
    let encrypted_key = key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(secret_store::encrypt)
        .transpose()?;
    save_at(&access_path(), internal_id, encrypted_key)
}

pub fn has_access(internal_id: &str) -> Result<bool, String> {
    Ok(read_access(&access_path())?.contains_key(internal_id))
}

pub fn supports(base_url: &str) -> bool {
    api_url(base_url, &["zenmux.ai"]).is_some_and(|url| url.port().is_none())
}

fn data(body: &Value) -> Option<&Value> {
    (body["success"].as_bool() == Some(true)).then(|| &body["data"])
}

fn wallet(body: &Value) -> Option<UsageQuota> {
    let data = data(body)?;
    if data["currency"].as_str() != Some("usd") {
        return None;
    }
    UsageQuota::balance(parse_f64(&data["total_credits"])?, "USD")
}

fn plan(body: &Value) -> Vec<UsageQuota> {
    let Some(data) = data(body) else {
        return Vec::new();
    };
    [
        ("quota_5_hour", QuotaPeriod::FiveHour),
        ("quota_7_day", QuotaPeriod::Weekly),
    ]
    .into_iter()
    .filter_map(|(field, period)| {
        let window = &data[field];
        let total = parse_f64(&window["max_flows"]).filter(|amount| *amount > 0.0)?;
        UsageQuota::allowance(
            period,
            total,
            parse_f64(&window["used_flows"])?,
            parse_f64(&window["remaining_flows"])?,
            "Flows",
            parse_reset_time(&window["resets_at"]),
        )
    })
    .collect()
}

impl ZenMuxProvider {
    pub(super) async fn query_for_model(&self, internal_id: &str) -> Result<UsageResult, String> {
        let Some(stored) = read_access(&access_path())?.remove(internal_id) else {
            return Ok(UsageResult::no_data());
        };
        let key = secret_store::decrypt(&stored)?;
        Ok(share_read(PLAN_URL, &key, async {
            let (plan_response, wallet_response) = tokio::join!(
                fetch_official_usage(PLAN_URL, &key),
                fetch_official_usage(BALANCE_URL, &key),
            );
            let mut quotas = plan_response.as_ref().map(plan).unwrap_or_default();
            quotas.extend(wallet_response.as_ref().ok().and_then(wallet));
            if quotas.is_empty() {
                if let Err(error) = wallet_response {
                    return UsageResult::failure(error);
                }
                if let Err(error) = plan_response {
                    return UsageResult::failure(error);
                }
            }
            UsageResult::from_quotas(quotas)
        })
        .await)
    }
}

#[async_trait::async_trait]
impl UsageProvider for ZenMuxProvider {
    async fn query_usage(&self, _api_key: &str, _base_url: &str) -> Result<UsageResult, String> {
        Ok(UsageResult::failure(
            "ZenMux balance and subscription querying requires a separate Management API key",
        ))
    }

    fn can_handle(&self, base_url: &str) -> bool {
        supports(base_url)
    }

    fn name(&self) -> &'static str {
        "ZenMux"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wallet_and_subscription_keep_separate_units_and_monthly_cap_has_no_meter() {
        let wallet = wallet(&json!({
            "success": true, "data": {"currency": "usd", "total_credits": 0}
        }))
        .unwrap();
        assert_eq!(wallet.balance, Some(0.0));
        let quotas = plan(&json!({
            "success": true,
            "data": {
                "quota_5_hour": {"max_flows": 200, "used_flows": 12, "remaining_flows": 188},
                "quota_7_day": {"max_flows": 500, "used_flows": 500, "remaining_flows": 0},
                "quota_monthly": {"max_flows": 1000}
            }
        }));
        assert_eq!(quotas.len(), 2);
        assert_eq!(quotas[0].amounts.as_ref().unwrap().unit, "Flows");
        assert_eq!(quotas[1].percentage, 100.0);
        assert!(plan(&json!({"success": false, "data": {}})).is_empty());
    }

    #[test]
    fn saving_and_removing_one_models_access_keeps_other_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("access.json");
        save_at(&path, "one", Some("encrypted-one".into())).unwrap();
        save_at(&path, "two", Some("encrypted-two".into())).unwrap();
        save_at(&path, "one", None).unwrap();
        assert_eq!(
            read_access(&path).unwrap(),
            HashMap::from([("two".into(), "encrypted-two".into())])
        );
    }

    #[tokio::test]
    async fn inference_key_is_not_sent_to_a_management_api() {
        let result = ZenMuxProvider
            .query_usage("fixture-key", "https://zenmux.ai/api/v1")
            .await
            .unwrap();
        assert!(result.data.is_none());
        let error = result.error.unwrap();
        assert!(error.contains("separate Management API key"));
        assert!(!error.contains("fixture-key"));
    }
}
