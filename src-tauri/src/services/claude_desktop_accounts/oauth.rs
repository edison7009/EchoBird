//! Claude Desktop keeps its OAuth tokens encrypted in config.json
//! (`oauth:tokenCacheV2`). A saved sign-in's token identifies the account and
//! reads its usage from api.anthropic.com. EchoBird only reads these tokens; it
//! never refreshes them, so the copy Claude Desktop holds stays valid.
use super::*;
use base64::Engine;

#[cfg(not(test))]
const API: &str = "https://api.anthropic.com";
const TOKEN_CACHE: &str = "oauth:tokenCacheV2";
/// Enough to read the account's profile and usage.
pub(super) const PROFILE_SCOPE: &str = "user:profile";
/// What the official client needs to send a message; narrower tokens get a 403.
pub(super) const CLIENT_SCOPE: &str = "user:sessions:claude_code";

/// The newest unexpired token of `account` holding `scope`, in `org` when it
/// is known. Cache keys look like
/// `acct:<account>|<client>:<organization>:<audience>:<scopes>`.
fn select_token(
    plain: &[u8],
    account: &str,
    org: &str,
    scope: &str,
    now_ms: i64,
) -> Result<String, String> {
    let cache: BTreeMap<String, Value> =
        serde_json::from_slice(plain).map_err(|_| "accountError.format")?;
    let prefix = format!("acct:{account}|");
    let organization = format!(":{org}:");
    cache
        .iter()
        .filter(|(key, _)| {
            key.starts_with(&prefix)
                && key.contains(scope)
                && (org.is_empty() || key.contains(&organization))
        })
        .filter_map(|(_, value)| {
            let expires = value["expiresAt"].as_i64()?;
            let token = value["token"].as_str().filter(|token| !token.is_empty())?;
            (expires > now_ms).then_some((expires, token))
        })
        .max_by_key(|(expires, _)| *expires)
        .map(|(_, token)| token.to_owned())
        // Opening Claude Desktop renews the tokens of the signed-in account.
        .ok_or_else(|| "accountError.initializeClient".into())
}

/// Reads a token from a snapshot's or the live client's config entries.
pub(super) fn access_token(
    dir: &Path,
    config: &BTreeMap<String, Value>,
    account: &str,
    org: &str,
    scope: &str,
) -> Result<String, String> {
    let encoded = config
        .get(TOKEN_CACHE)
        .and_then(Value::as_str)
        .ok_or("accountError.noAccount")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| "accountError.format")?;
    let plain = decrypt_bytes(&cipher(dir)?, &bytes)?;
    select_token(
        &plain,
        account,
        org,
        scope,
        chrono::Utc::now().timestamp_millis(),
    )
}

fn http_error(status: u16) -> String {
    let key = match status {
        401 => "accountError.loginRequired",
        403 => "accountError.denied",
        429 => "accountError.rateLimited",
        500..=599 => "accountError.unavailable",
        _ => "accountError.quota",
    };
    format!("{key}|HTTP {status}")
}

// Unit tests must not contact the public service.
#[cfg(test)]
async fn get(_token: &str, _path: &str) -> Result<Value, String> {
    Err("accountError.network".into())
}
#[cfg(not(test))]
async fn get(token: &str, path: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .get(format!("{API}{path}"))
        .bearer_auth(token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_error(status.as_u16()));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}

pub(super) async fn profile(token: &str) -> Result<Value, String> {
    get(token, "/api/oauth/profile").await
}

pub(super) async fn usage(token: &str) -> Result<Value, String> {
    get(token, "/api/oauth/usage").await
}

/// The account a profile describes, if it is `account` in `org`.
pub(super) fn account_from_profile(
    body: &Value,
    account: &str,
    org: &str,
) -> Result<Account, String> {
    let identity = &body["account"];
    let organization = &body["organization"];
    if identity["uuid"].as_str() != Some(account) || organization["uuid"].as_str() != Some(org) {
        return Err("accountError.authResponse".into());
    }
    let email = identity["email"]
        .as_str()
        .or_else(|| identity["email_address"].as_str())
        .filter(|email| !email.is_empty())
        .ok_or("accountError.authResponse")?;
    let kind = organization["organization_type"]
        .as_str()
        .unwrap_or_default()
        .trim_start_matches("claude_");
    let tier = organization["rate_limit_tier"].as_str().unwrap_or_default();
    let plan = match kind {
        "max" if tier.contains("20x") => Some("Ultra"),
        "max" => Some("Max"),
        "pro" => Some("Pro"),
        "team" => Some("Team"),
        "enterprise" => Some("Enterprise"),
        "free" => Some("Free"),
        _ if identity["has_claude_max"].as_bool() == Some(true) => Some("Max"),
        _ if identity["has_claude_pro"].as_bool() == Some(true) => Some("Pro"),
        _ => None,
    };
    Ok(Account {
        id: account_id(account, org),
        email: email.into(),
        plan: plan.map(str::to_owned),
        five_hour: None,
        seven_day: None,
        active: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACCOUNT: &str = "12345678-1234-1234-1234-123456789def";
    const ORG: &str = "12345678-1234-1234-1234-123456789abc";
    const OTHER_ORG: &str = "87654321-1234-1234-1234-123456789abc";

    fn cache(entries: &[(&str, &str, i64)]) -> Vec<u8> {
        let map: BTreeMap<String, Value> = entries
            .iter()
            .map(|(key, token, expires)| {
                (
                    (*key).to_owned(),
                    json!({"token": token, "expiresAt": expires}),
                )
            })
            .collect();
        serde_json::to_vec(&map).unwrap()
    }

    #[test]
    fn a_message_uses_the_token_the_official_client_accepts() {
        let key = |scopes: &str| {
            format!("acct:{ACCOUNT}|client:{ORG}:https://api.anthropic.com:{scopes}")
        };
        // Tied expiry, as Claude Desktop issues them; the longest-lived token lacks the scope.
        let plain = cache(&[
            (
                &key("user:inference user:file_upload user:profile"),
                "api",
                90_000,
            ),
            (
                &key("user:inference user:file_upload user:profile user:sessions:claude_code"),
                "client",
                8_000,
            ),
            (&key("user:profile"), "profile", 8_000),
        ]);
        assert_eq!(
            select_token(&plain, ACCOUNT, ORG, CLIENT_SCOPE, 100).unwrap(),
            "client"
        );
        let plain = cache(&[(&key("user:profile"), "profile", 8_000)]);
        assert_eq!(
            select_token(&plain, ACCOUNT, ORG, CLIENT_SCOPE, 100).unwrap_err(),
            "accountError.initializeClient"
        );
    }

    #[test]
    fn tokens_belong_to_the_account_can_read_its_profile_and_are_unexpired() {
        let key = |account: &str, org: &str, scopes: &str| {
            format!("acct:{account}|client:{org}:https://api.anthropic.com:{scopes}")
        };
        let profile = "user:profile user:inference";
        let plain = cache(&[
            (&key("other", ORG, profile), "wrong-account", 9_000),
            (
                &key(ACCOUNT, ORG, "user:inference"),
                "no-profile-scope",
                9_000,
            ),
            (&key(ACCOUNT, ORG, profile), "expired", 50),
            (&key(ACCOUNT, OTHER_ORG, profile), "other-org", 9_500),
            (&key(ACCOUNT, ORG, profile), "same-org", 8_000),
        ]);
        assert_eq!(
            select_token(&plain, ACCOUNT, ORG, PROFILE_SCOPE, 100).unwrap(),
            "same-org"
        );
        assert_eq!(
            select_token(&plain, ACCOUNT, OTHER_ORG, PROFILE_SCOPE, 100).unwrap(),
            "other-org"
        );
        // Before the organization is known, any organization's newest token.
        assert_eq!(
            select_token(&plain, ACCOUNT, "", PROFILE_SCOPE, 100).unwrap(),
            "other-org"
        );
        assert_eq!(
            select_token(&plain, ACCOUNT, "unknown-org", PROFILE_SCOPE, 100).unwrap_err(),
            "accountError.initializeClient"
        );
        assert_eq!(
            select_token(&plain, "missing", ORG, PROFILE_SCOPE, 100).unwrap_err(),
            "accountError.initializeClient"
        );
        assert_eq!(
            select_token(b"not json", ACCOUNT, ORG, PROFILE_SCOPE, 100).unwrap_err(),
            "accountError.format"
        );
    }

    #[test]
    fn profiles_must_match_the_snapshot_and_name_the_plan() {
        let body = |kind: &str, tier: &str| {
            json!({
                "account": {"uuid": ACCOUNT, "email": "fixture@example.test"},
                "organization": {"uuid": ORG, "organization_type": kind, "rate_limit_tier": tier}
            })
        };
        for (kind, tier, plan) in [
            ("claude_max", "default_claude_max_20x", Some("Ultra")),
            ("claude_max", "default_claude_max_5x", Some("Max")),
            ("claude_pro", "", Some("Pro")),
            ("claude_team", "", Some("Team")),
            ("claude_enterprise", "", Some("Enterprise")),
            ("unknown", "", None),
        ] {
            let account = account_from_profile(&body(kind, tier), ACCOUNT, ORG).unwrap();
            assert_eq!(account.plan.as_deref(), plan, "{kind}");
            assert_eq!(account.email, "fixture@example.test");
            assert_eq!(account.id, account_id(ACCOUNT, ORG));
        }
        assert_eq!(
            account_from_profile(&body("claude_pro", ""), ACCOUNT, OTHER_ORG).err(),
            Some("accountError.authResponse".to_string())
        );
        assert_eq!(
            account_from_profile(&body("claude_pro", ""), "other", ORG).err(),
            Some("accountError.authResponse".to_string())
        );
    }

    #[test]
    fn failed_requests_keep_the_shared_account_errors() {
        assert_eq!(http_error(401), "accountError.loginRequired|HTTP 401");
        assert_eq!(http_error(403), "accountError.denied|HTTP 403");
        assert_eq!(http_error(429), "accountError.rateLimited|HTTP 429");
        assert_eq!(http_error(503), "accountError.unavailable|HTTP 503");
        assert_eq!(http_error(404), "accountError.quota|HTTP 404");
    }
}
