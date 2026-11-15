use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{sync::Mutex, time::Duration};

use super::claude_code_accounts::{self, ClaudeCodeAccount};

const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
const SCOPES: &str =
    "user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
const LOGIN_TIMEOUT_SECONDS: i64 = 60;
static PENDING: Mutex<Option<PendingLogin>> = Mutex::new(None);

#[derive(Clone)]
struct PendingLogin {
    id: String,
    state: String,
    verifier: String,
    expires_at: i64,
    busy: bool,
    snapshots: Option<(Value, Value)>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    login_id: String,
    authorization_url: String,
    expires_at: i64,
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn start() -> Result<LoginStart, String> {
    let pending = PendingLogin {
        id: random_token(),
        state: random_token(),
        verifier: random_token(),
        expires_at: chrono::Utc::now().timestamp() + LOGIN_TIMEOUT_SECONDS,
        busy: false,
        snapshots: None,
    };
    let mut url = reqwest::Url::parse("https://claude.com/cai/oauth/authorize")
        .map_err(|e| format!("accountError.auth|{e}"))?;
    url.query_pairs_mut()
        .append_pair("code", "true")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", REDIRECT)
        .append_pair("scope", SCOPES)
        .append_pair(
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.as_bytes())),
        )
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &pending.state);
    let result = LoginStart {
        login_id: pending.id.clone(),
        authorization_url: url.to_string(),
        expires_at: pending.expires_at,
    };
    *PENDING.lock().map_err(|_| "accountError.auth")? = Some(pending);
    Ok(result)
}

pub fn cancel(id: &str) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.cancelled")?;
    if pending.as_ref().is_some_and(|p| p.id == id) {
        *pending = None;
    }
    Ok(())
}

fn parse_code<'a>(input: &'a str, expected_state: &str) -> Result<&'a str, String> {
    let input = input.trim();
    let (code, state) = input
        .split_once('#')
        .map_or((input, None), |(code, state)| (code, Some(state)));
    if code.is_empty() || code.contains(char::is_whitespace) || code.contains("://") {
        return Err("accountError.code".to_string());
    }
    if state.is_some_and(|state| state != expected_state) {
        return Err("accountError.state".to_string());
    }
    Ok(code)
}

fn profile_fields(fallback: &Value, profile: &Value) -> Value {
    let mut fields = fallback.as_object().cloned().unwrap_or_default();
    if let Some(profile) = profile.as_object() {
        for (key, value) in profile {
            if !value.is_null() && !value.as_str().is_some_and(|s| s.trim().is_empty()) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(fields)
}

fn login_snapshots(tokens: &Value, profile: &Value) -> Result<(Value, Value), String> {
    let token = tokens["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("accountError.authResponse")?;
    let account = profile_fields(&tokens["account"], &profile["account"]);
    let organization = profile_fields(&tokens["organization"], &profile["organization"]);
    let email = account["email"]
        .as_str()
        .or_else(|| account["email_address"].as_str())
        .ok_or("accountError.authResponse")?;
    let plan = organization["organization_type"]
        .as_str()
        .and_then(|s| s.strip_prefix("claude_"));
    let credentials = json!({"claudeAiOauth": {
        "accessToken": token,
        "refreshToken": tokens["refresh_token"],
        "expiresAt": chrono::Utc::now().timestamp_millis() + tokens["expires_in"].as_i64().unwrap_or(3600) * 1000,
        "scopes": tokens["scope"].as_str().unwrap_or(SCOPES).split_whitespace().collect::<Vec<_>>(),
        "subscriptionType": plan,
        "rateLimitTier": organization["rate_limit_tier"],
    }});
    let config = json!({"oauthAccount": {
        "accountUuid": account["uuid"], "emailAddress": email,
        "organizationUuid": organization["uuid"], "organizationName": organization["name"],
        "displayName": account["display_name"], "subscriptionType": plan,
    }});
    Ok((credentials, config))
}

async fn exchange(pending: &PendingLogin, code: &str) -> Result<(Value, Value), String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("accountError.auth|{e}"))?;
    let response = client
        .post("https://platform.claude.com/v1/oauth/token")
        .json(&json!({
            "grant_type": "authorization_code", "client_id": CLIENT_ID, "code": code,
            "redirect_uri": REDIRECT, "code_verifier": pending.verifier, "state": pending.state,
        }))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        return Err(format!(
            "accountError.auth|HTTP {}",
            response.status().as_u16()
        ));
    }
    let tokens: Value = response
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    let access_token = tokens["access_token"]
        .as_str()
        .ok_or("accountError.authResponse")?;
    let profile = match client
        .get("https://api.anthropic.com/api/oauth/profile")
        .bearer_auth(access_token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => {
            response.json().await.unwrap_or(Value::Null)
        }
        _ => Value::Null,
    };
    login_snapshots(&tokens, &profile)
}

pub async fn complete(id: &str, input: &str) -> Result<ClaudeCodeAccount, String> {
    let (pending, code) = {
        let mut guard = PENDING.lock().map_err(|_| "accountError.auth")?;
        let pending = guard
            .as_mut()
            .filter(|p| p.id == id)
            .ok_or("accountError.cancelled")?;
        if pending.expires_at <= chrono::Utc::now().timestamp() {
            return Err("accountError.expired".to_string());
        }
        if pending.busy {
            return Err("accountError.busy".to_string());
        }
        let code = parse_code(input, &pending.state)?.to_string();
        pending.busy = true;
        (pending.clone(), code)
    };
    let result = match pending.snapshots {
        Some(ref snapshots) => Ok(snapshots.clone()),
        None => exchange(&pending, &code).await,
    };
    let _account_guard = claude_code_accounts::ACCOUNT_LOCK.lock().await;
    let mut guard = PENDING.lock().map_err(|_| "accountError.auth")?;
    let current = guard
        .as_mut()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    current.busy = false;
    if current.expires_at <= chrono::Utc::now().timestamp() {
        return Err("accountError.expired".to_string());
    }
    let (credentials, config) = result?;
    // An authorization code is single-use. Retain the exchanged login if saving fails.
    current.snapshots = Some((credentials.clone(), config.clone()));
    let account = claude_code_accounts::save_login(&credentials, &config)?;
    *guard = None;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_code_and_code_with_state_but_rejects_wrong_session_and_urls() {
        assert_eq!(parse_code(" code#state ", "state").unwrap(), "code");
        assert_eq!(parse_code("code", "state").unwrap(), "code");
        assert!(parse_code("code#other", "state").is_err());
        assert!(parse_code("https://claude.com/cai/oauth/authorize", "state").is_err());
        assert!(parse_code(" ", "state").is_err());
    }

    #[test]
    fn converts_tokens_and_profile_to_native_claude_code_credentials() {
        let tokens = json!({"access_token":"test", "refresh_token":"refresh", "expires_in":3600, "account":{"email_address":"test@example.com", "uuid":"account"}, "organization":{"uuid":"org"}});
        let (credentials, config) = login_snapshots(&tokens, &Value::Null).unwrap();
        assert_eq!(credentials["claudeAiOauth"]["refreshToken"], "refresh");
        assert_eq!(config["oauthAccount"]["emailAddress"], "test@example.com");
        assert_eq!(config["oauthAccount"]["organizationUuid"], "org");
        assert!(login_snapshots(&json!({}), &Value::Null).is_err());
        let profile = json!({"account":{"uuid":null,"display_name":"Demo"},"organization":{"organization_type":"claude_max", "rate_limit_tier":"default_claude_max_20x"}});
        let (credentials, config) = login_snapshots(&tokens, &profile).unwrap();
        assert_eq!(config["oauthAccount"]["accountUuid"], "account");
        assert_eq!(config["oauthAccount"]["organizationUuid"], "org");
        assert_eq!(config["oauthAccount"]["emailAddress"], "test@example.com");
        assert_eq!(credentials["claudeAiOauth"]["subscriptionType"], "max");
    }

    #[tokio::test]
    async fn cancelled_session_cannot_exchange_or_save_an_account() {
        let login = start().unwrap();
        let url = reqwest::Url::parse(&login.authorization_url).unwrap();
        assert!(url
            .query_pairs()
            .any(|(k, v)| k == "code_challenge_method" && v == "S256"));
        assert!(!login.authorization_url.contains("code_verifier"));
        cancel(&login.login_id).unwrap();
        assert!(complete(&login.login_id, "test").await.is_err());

        // A retry after a local save failure must use the exchanged snapshot,
        // not submit the already-consumed authorization code again.
        let retry = start().unwrap();
        PENDING.lock().unwrap().as_mut().unwrap().snapshots = Some((json!({}), json!({})));
        let error = complete(&retry.login_id, "test").await.err().unwrap();
        assert!(error.contains("accountError.loginRequired"));
        let guard = PENDING.lock().unwrap();
        assert!(guard.as_ref().unwrap().snapshots.is_some());
        assert!(!guard.as_ref().unwrap().busy);
        drop(guard);
        cancel(&retry.login_id).unwrap();
    }
}
