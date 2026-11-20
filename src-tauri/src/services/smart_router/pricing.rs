//! Shared public reference prices. Route traffic only reads the cached catalog.
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::usage::UsageReport;
use crate::models::model::{AccountProvider, AccountSource};

const MAX_BYTES: usize = 32 * 1024 * 1024;
const MAX_MODELS: usize = 30_000;
const ENDPOINT: &str = "https://models.dev/api.json";

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub cache_write_1h: Option<f64>,
    pub tiers: Vec<Tier>,
    pub provider: String,
    pub fetched_at: u64,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tier {
    pub above: u64,
    pub input: f64,
    pub output: f64,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Cache {
    fetched_at: u64,
    prices: BTreeMap<String, Price>,
    hosts: BTreeMap<String, String>,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub fetched_at: u64,
    pub models: usize,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

#[cfg(test)]
pub(super) fn set_test_price(provider: &str, model: &str, price: Price) {
    cache()
        .lock()
        .unwrap()
        .prices
        .insert(format!("{provider}/{model}"), price);
}

fn path() -> std::path::PathBuf {
    crate::utils::platform::echobird_dir().join("config/smart-router-prices.json")
}
fn sane(n: f64) -> bool {
    n.is_finite() && (0.0..=1_000_000.0).contains(&n)
}
pub(super) fn valid(price: &Price) -> bool {
    sane(price.input)
        && sane(price.output)
        && [price.cache_read, price.cache_write, price.cache_write_1h]
            .into_iter()
            .flatten()
            .all(sane)
        && price.provider.len() <= 100
        && price.tiers.len() <= 32
        && price.tiers.iter().all(|t| {
            sane(t.input)
                && sane(t.output)
                && [t.cache_read, t.cache_write]
                    .into_iter()
                    .flatten()
                    .all(sane)
        })
}
pub fn status() -> Status {
    // Loading our own catalog cache is allowed on navigation; no native accounts or network.
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        if let Ok(file) = std::fs::File::open(path()) {
            let mut bytes = Vec::new();
            if file
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .is_ok()
                && bytes.len() <= MAX_BYTES
            {
                if let Ok(saved) = serde_json::from_slice::<Cache>(&bytes) {
                    if saved.prices.len() <= MAX_MODELS && saved.prices.values().all(valid) {
                        let mut current = cache().lock().unwrap_or_else(|e| e.into_inner());
                        if saved.fetched_at > current.fetched_at {
                            *current = saved;
                        }
                    }
                }
            }
        }
    });
    let c = cache().lock().unwrap_or_else(|e| e.into_inner());
    Status {
        fetched_at: c.fetched_at,
        models: c.prices.len(),
    }
}
fn rate(value: &Value) -> Option<f64> {
    value.as_f64().filter(|n| sane(*n))
}
fn parse(bytes: &[u8], now: u64) -> Result<Cache, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Reference price catalog is too large".into());
    }
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| "Invalid reference price catalog")?;
    let providers = root.as_object().ok_or("Invalid reference price catalog")?;
    let mut result = Cache {
        fetched_at: now,
        ..Cache::default()
    };
    for (provider, data) in providers {
        if provider.len() > 100 {
            continue;
        }
        if let Some(host) = data["api"]
            .as_str()
            .and_then(|s| url::Url::parse(s).ok())
            .and_then(|u| u.host_str().map(str::to_owned))
        {
            // Ambiguous hosts must not silently select one seller's prices.
            result
                .hosts
                .entry(host)
                .and_modify(|p| p.clear())
                .or_insert_with(|| provider.clone());
        }
        let Some(models) = data["models"].as_object() else {
            continue;
        };
        for (model, data) in models {
            if model.len() > 256 {
                continue;
            }
            let cost = &data["cost"];
            let (Some(input), Some(output)) = (rate(&cost["input"]), rate(&cost["output"])) else {
                continue;
            };
            // Separate reasoning/audio prices need category counts we do not yet have.
            if cost["reasoning"].as_f64().is_some_and(|n| n != output)
                || ["input_audio", "output_audio", "input_video", "output_image"]
                    .iter()
                    .any(|k| cost.get(*k).is_some())
            {
                continue;
            }
            let mut price = Price {
                input,
                output,
                cache_read: rate(&cost["cache_read"]),
                cache_write: rate(&cost["cache_write"]),
                cache_write_1h: rate(&cost["cache_write_1h"]),
                provider: provider.clone(),
                fetched_at: now,
                tiers: Vec::new(),
            };
            let mut supported = true;
            if let Some(tiers) = cost["tiers"].as_array() {
                if tiers.len() > 32 {
                    continue;
                }
                for t in tiers {
                    let (Some(above), Some(ti), Some(to)) = (
                        t["tier"]["size"].as_u64(),
                        rate(&t["input"]),
                        rate(&t["output"]),
                    ) else {
                        supported = false;
                        break;
                    };
                    if t["tier"]["type"].as_str() != Some("context") {
                        supported = false;
                        break;
                    }
                    price.tiers.push(Tier {
                        above,
                        input: ti,
                        output: to,
                        cache_read: rate(&t["cache_read"]),
                        cache_write: rate(&t["cache_write"]),
                    });
                }
            }
            price.tiers.sort_by_key(|t| t.above);
            if supported {
                result.prices.insert(format!("{provider}/{model}"), price);
            }
            if result.prices.len() > MAX_MODELS {
                return Err("Too many reference prices".into());
            }
        }
    }
    if result.prices.is_empty() {
        return Err("Empty reference price catalog".into());
    }
    Ok(result)
}
pub async fn refresh() -> Result<Status, String> {
    static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = REFRESH.lock().await;
    let mut response = crate::services::http_transport::catalog()
        .get(ENDPOINT)
        .header(reqwest::header::USER_AGENT, "EchoBird/reference-prices")
        .send()
        .await
        .map_err(|_| "Unable to fetch reference prices")?
        .error_for_status()
        .map_err(|_| "Unable to fetch reference prices")?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err("Reference price catalog is too large".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Unable to fetch reference prices")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err("Reference price catalog is too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let saved = parse(&bytes, chrono::Utc::now().timestamp_millis().max(0) as u64)?;
    let status = Status {
        fetched_at: saved.fetched_at,
        models: saved.prices.len(),
    };
    let body = serde_json::to_vec(&saved).map_err(|_| "Unable to save reference prices")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::services::native_account_files::write_private_file(&path(), &body)
            .map_err(|_| "Unable to save reference prices")?;
        *cache().lock().unwrap_or_else(|e| e.into_inner()) = saved;
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "Unable to save reference prices")??;
    Ok(status)
}

pub(crate) async fn warm_for_assistant() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_CHECK: AtomicU64 = AtomicU64::new(0);
    let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let next = NEXT_CHECK.load(Ordering::Relaxed);
    if now < next
        || NEXT_CHECK
            .compare_exchange(
                next,
                now.saturating_add(300_000),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_err()
    {
        return;
    }
    let Ok(current) = tauri::async_runtime::spawn_blocking(status).await else {
        return;
    };
    if current.models == 0 || now.saturating_sub(current.fetched_at) >= 86_400_000 {
        let _ = refresh().await;
    }
}
pub fn lookup(base_url: &str, source: Option<&AccountSource>, model: &str) -> Option<Price> {
    let host = url::Url::parse(base_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned));
    let c = cache().lock().unwrap_or_else(|e| e.into_inner());
    let provider = match source.map(|s| s.provider) {
        Some(AccountProvider::ClaudeCode) => "anthropic",
        Some(AccountProvider::Codex) => "openai",
        _ => match host.as_deref()? {
            "api.anthropic.com" => "anthropic",
            "api.openai.com" => "openai",
            "generativelanguage.googleapis.com" => "google",
            "api.deepseek.com" => "deepseek",
            host => c.hosts.get(host)?.as_str(),
        },
    };
    if provider.is_empty() {
        return None;
    }
    c.prices.get(&format!("{provider}/{model}")).cloned()
}
impl Price {
    pub fn estimate(&self, report: &UsageReport) -> Option<u64> {
        if !report.billable || !valid(self) {
            return None;
        }
        let u = &report.usage;
        let read = report.cache_read;
        let write = report.cache_write;
        let plain = u.input_tokens.checked_sub(read.checked_add(write)?)?;
        let mut input = self.input;
        let mut output = self.output;
        let mut cr = self.cache_read;
        let mut cw = self.cache_write;
        for tier in &self.tiers {
            if u.input_tokens > tier.above {
                // The retained context tier has no one-hour write rate. A base
                // rate is not evidence for this tier; preserve unknown billing.
                if report.cache_write_1h > 0 {
                    return None;
                }
                input = tier.input;
                output = tier.output;
                cr = tier.cache_read;
                cw = tier.cache_write;
            }
        }
        let short_write = write.checked_sub(report.cache_write_1h)?;
        let amount = plain as f64 * input
            + u.output_tokens as f64 * output
            + if read > 0 { read as f64 * cr? } else { 0.0 }
            + if short_write > 0 {
                short_write as f64 * cw?
            } else {
                0.0
            }
            + if report.cache_write_1h > 0 {
                report.cache_write_1h as f64 * self.cache_write_1h?
            } else {
                0.0
            };
        (amount.is_finite() && (0.0..u64::MAX as f64).contains(&amount))
            .then(|| amount.round() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prices_exact_models_cache_categories_and_context_tiers_without_inventing_missing_rates() {
        let bytes=br#"{"vendor":{"models":{"exact":{"cost":{"input":2,"output":10,"cache_read":0.2,"cache_write":2.5,"tiers":[{"tier":{"type":"context","size":100},"input":4,"output":20,"cache_read":0.4,"cache_write":5}]}},"missing":{"cost":{"input":2}},"bad":{"cost":{"input":-1,"output":3}}}}}"#;
        let c = parse(bytes, 123).unwrap();
        assert_eq!(c.prices.len(), 1);
        let p = &c.prices["vendor/exact"];
        let mut r = UsageReport {
            billable: true,
            cache_read: 10,
            cache_write: 5,
            usage: super::super::usage::TokenUsage {
                input_tokens: 100,
                output_tokens: 10,
                total_tokens: 110,
                reported_responses: 1,
            },
            ..UsageReport::default()
        };
        assert_eq!(p.estimate(&r), Some(285));
        r.usage.input_tokens = 200;
        assert_eq!(p.estimate(&r), Some(969));
        r.cache_write_1h = 1;
        assert_eq!(p.estimate(&r), None);
        let mut priced_hour = p.clone();
        priced_hour.cache_write_1h = Some(4.0);
        assert_eq!(priced_hour.estimate(&r), None);
        r.usage.input_tokens = 100;
        assert_eq!(priced_hour.estimate(&r), Some(286));
        r.billable = false;
        assert_eq!(priced_hour.estimate(&r), None);
    }
}
