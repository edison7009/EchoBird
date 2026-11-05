use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

pub(super) const CLIENT_ID: &str = match option_env!("ANTIGRAVITY_OAUTH_CLIENT_ID") {
    Some(value) => value,
    None => "",
};
pub(super) const CLIENT_SECRET: &str = match option_env!("ANTIGRAVITY_OAUTH_CLIENT_SECRET") {
    Some(value) => value,
    None => "",
};
const BASE: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal:";
pub(super) const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelQuota {
    pub name: String,
    pub remaining_percent: f64,
    pub reset_at: Option<i64>,
}

fn model_quota(name: &str, info: &Value) -> Option<ModelQuota> {
    let fraction = info["quotaInfo"]["remainingFraction"].as_f64()?;
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return None;
    }
    let reset_at = info["quotaInfo"]["resetTime"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp());
    Some(ModelQuota {
        name: name.to_owned(),
        remaining_percent: fraction * 100.0,
        reset_at,
    })
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "accountError.network".into())
}

async fn post(token: &str, path: &str, body: Value) -> Result<Value, String> {
    let response = client()?
        .post(format!("{BASE}{path}"))
        .bearer_auth(token)
        .header(
            "User-Agent",
            format!(
                "antigravity/{} {}/{}",
                "2.19.1",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        )
        .json(&body)
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err(format!(
            "accountError.auth|HTTP {}",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

pub(super) async fn refresh_access(secret: &mut Value) -> Result<(), String> {
    refresh_access_at(secret, TOKEN_URL).await
}

pub(super) async fn refresh_access_at(secret: &mut Value, token_url: &str) -> Result<(), String> {
    let expires = secret["token"]["expiry"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);
    if expires > chrono::Utc::now().timestamp() + 120 {
        return Ok(());
    }
    let refresh_token = secret["token"]["refresh_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.invalidAccount")?;
    let response = client()?
        .post(token_url)
        .form(&[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("refresh_token", refresh_token),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(super::super::account_model_sources::oauth_refresh_error(
            status, &body,
        ));
    }
    let result: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    let access = result["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    secret["token"]["access_token"] = json!(access);
    if let Some(token) = result["id_token"].as_str() {
        secret["id_token"] = json!(token);
    }
    if let Some(token) = result["refresh_token"].as_str().filter(|s| !s.is_empty()) {
        secret["token"]["refresh_token"] = json!(token);
    }
    let expires_in = result["expires_in"].as_i64().unwrap_or(3600);
    secret["token"]["expiry"] =
        json!((chrono::Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339());
    Ok(())
}

pub(super) async fn fetch(secret: &Value) -> Result<(Option<String>, Vec<ModelQuota>), String> {
    let token = secret["token"]["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.invalidAccount")?;
    let version = "2.19.1";
    let os = match std::env::consts::OS {
        "macos" => "DARWIN",
        "linux" => "LINUX",
        _ => "WINDOWS",
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "ARM64",
        _ => "AMD64",
    };
    let metadata = json!({
        "ideName": "antigravity", "ideType": "ANTIGRAVITY",
        "ideVersion": version, "platform": format!("{os}_{arch}"),
        "updateChannel": "stable", "pluginType": "GEMINI"
    });
    let tiers = post(
        token,
        "loadCodeAssist",
        json!({"metadata":metadata,"mode":"FULL_ELIGIBILITY_CHECK"}),
    )
    .await?;
    let tier_id = tiers["paidTier"]["id"]
        .as_str()
        .or_else(|| tiers["currentTier"]["id"].as_str());
    let plan = tier_id.and_then(|id| {
        let id = id.to_ascii_lowercase();
        if id.contains("ultra") {
            Some("Ultra".to_string())
        } else if id.contains("pro") {
            Some("Pro".to_string())
        } else if id.contains("plus") {
            Some("Plus".to_string())
        } else if id.contains("free") {
            Some("Free".to_string())
        } else {
            None
        }
    });
    let project = tiers["cloudaicompanionProject"]
        .as_str()
        .or_else(|| tiers["cloudaicompanionProject"]["id"].as_str());
    let body = project.map_or_else(|| json!({}), |p| json!({"project":p}));
    let models = post(token, "fetchAvailableModels", body.clone()).await?;
    let quotas: Vec<ModelQuota> = models["models"]
        .as_object()
        .ok_or("accountError.authResponse")?
        .iter()
        .filter_map(|(name, info)| model_quota(name, info))
        .collect();
    if !looks_untouched(&quotas) {
        return Ok((plan, quotas));
    }
    // The check is best effort: when it cannot answer, the list stands.
    let quotas = match post(token, "retrieveUserQuota", body).await {
        Ok(buckets) => verified_quotas(quotas, &buckets),
        Err(_) => quotas,
    };
    Ok((plan, quotas))
}

/// The model list can report every model at 100% when it only proves the
/// models are available, so such a reply is checked against the measured quota.
fn looks_untouched(quotas: &[ModelQuota]) -> bool {
    !quotas.is_empty() && quotas.iter().all(|quota| quota.remaining_percent >= 99.9)
}

/// Each model takes its measured fraction (the lowest bucket of its id), and
/// measured models missing from the list are added. When nothing in the list is
/// measured, the list is kept as it came.
fn verified_quotas(models: Vec<ModelQuota>, response: &Value) -> Vec<ModelQuota> {
    let buckets = response["buckets"]
        .as_array()
        .or_else(|| response["response"]["buckets"].as_array());
    let mut measured: std::collections::BTreeMap<String, ModelQuota> =
        std::collections::BTreeMap::new();
    for bucket in buckets.into_iter().flatten() {
        let Some(id) = bucket["modelId"]
            .as_str()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let Some(quota) = model_quota(id, &json!({"quotaInfo": bucket})) else {
            continue;
        };
        let key = id.to_lowercase();
        if measured
            .get(&key)
            .is_none_or(|kept| quota.remaining_percent < kept.remaining_percent)
        {
            measured.insert(key, quota);
        }
    }
    if !models
        .iter()
        .any(|model| measured.contains_key(&model.name.to_lowercase()))
    {
        return models;
    }
    let mut merged: Vec<ModelQuota> = models
        .into_iter()
        .map(|model| match measured.remove(&model.name.to_lowercase()) {
            Some(quota) => ModelQuota {
                reset_at: quota.reset_at.or(model.reset_at),
                remaining_percent: quota.remaining_percent,
                name: model.name,
            },
            None => model,
        })
        .collect();
    merged.extend(measured.into_values());
    merged
}

#[cfg(test)]
mod tests {
    use super::{looks_untouched, model_quota, verified_quotas, ModelQuota};

    fn full(name: &str) -> ModelQuota {
        ModelQuota {
            name: name.into(),
            remaining_percent: 100.0,
            reset_at: Some(10),
        }
    }

    #[test]
    fn an_all_full_model_list_is_replaced_by_the_measured_quota() {
        let models = vec![full("gemini-3-pro"), full("Claude-Sonnet"), full("gpt-oss")];
        assert!(looks_untouched(&models));
        let measured = verified_quotas(
            models,
            &serde_json::json!({"buckets":[
                {"modelId":"gemini-3-pro","remainingFraction":0.4,"resetTime":"2027-01-01T00:00:00Z"},
                {"modelId":"gemini-3-pro","remainingFraction":0.7},
                {"modelId":"claude-sonnet","remainingFraction":1.0},
                {"modelId":"gemini-3-flash","remainingFraction":0.9},
                {"modelId":"no-fraction"}
            ]}),
        );
        let rows: Vec<_> = measured
            .iter()
            .map(|quota| (quota.name.as_str(), quota.remaining_percent, quota.reset_at))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("gemini-3-pro", 40.0, Some(1798761600)),
                ("Claude-Sonnet", 100.0, Some(10)),
                ("gpt-oss", 100.0, Some(10)),
                ("gemini-3-flash", 90.0, None),
            ]
        );
        for unmatched in [
            serde_json::json!({"buckets":[]}),
            serde_json::json!({"buckets":[{"modelId":"other","remainingFraction":0.2}]}),
            serde_json::json!({}),
        ] {
            let kept = verified_quotas(vec![full("a")], &unmatched);
            assert_eq!(kept.len(), 1);
            assert_eq!(
                (kept[0].name.as_str(), kept[0].remaining_percent),
                ("a", 100.0)
            );
        }
        assert!(verified_quotas(
            vec![full("a")],
            &serde_json::json!({"response":{"buckets":[{"modelId":"a","remainingFraction":0.5}]}})
        )[0]
        .remaining_percent
        .eq(&50.0));
    }

    #[test]
    fn a_model_list_with_real_usage_is_trusted() {
        let mut used = full("a");
        used.remaining_percent = 80.0;
        assert!(!looks_untouched(&[full("b"), used]));
        assert!(!looks_untouched(&[]));
    }

    #[tokio::test]
    async fn refresh_rejection_retains_credential_failure_without_replacing_tokens() {
        use axum::{http::StatusCode, routing::post, Router};
        for (status, error, expected) in [
            (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error":"invalid_grant"}).to_string(),
                "accountError.loginRequired|HTTP 400",
            ),
            (
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error":"invalid_request"}).to_string(),
                "accountError.auth|HTTP 400",
            ),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "<html>Service unavailable</html>".into(),
                "accountError.auth|HTTP 503",
            ),
            (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limited".into(),
                "accountError.auth|HTTP 429",
            ),
        ] {
            let app = Router::new().route("/token", post(move || async move { (status, error) }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/token", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let mut secret = serde_json::json!({
                "token":{"access_token":"expired-fixture","refresh_token":"fixture-refresh","expiry":null}
            });
            let original = secret.clone();
            let error = super::refresh_access_at(&mut secret, &url)
                .await
                .unwrap_err();
            assert_eq!(error, expected);
            assert_eq!(
                super::super::super::account_model_sources::credential_error(error).status,
                if expected.starts_with("accountError.loginRequired") {
                    StatusCode::UNAUTHORIZED
                } else {
                    status
                }
            );
            assert_eq!(secret, original);
            server.abort();
        }
    }

    #[test]
    fn parses_real_fraction_and_skips_missing_or_invalid_quota() {
        let response = serde_json::json!({"quotaInfo":{"remainingFraction":0.25,"resetTime":"2026-10-01T12:00:00Z"}});
        let parsed = model_quota("gemini-3", &response).unwrap();
        assert_eq!(parsed.remaining_percent, 25.0);
        assert_eq!(parsed.reset_at, Some(1790856000));
        assert!(model_quota("claude", &serde_json::json!({"quotaInfo":{}})).is_none());
        assert!(model_quota(
            "claude",
            &serde_json::json!({"quotaInfo":{"remainingFraction":1.5}})
        )
        .is_none());
    }
}
