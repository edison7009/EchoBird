//! Command Code's provider and CLI share the account's credit pool.

use super::{api_url, fetch_official_usage, share_read, UsageProvider, UsageQuota, UsageResult};
use crate::services::commandcode_accounts::{remaining_credits, CREDITS_URL};

pub struct CommandCodeProvider;

#[async_trait::async_trait]
impl UsageProvider for CommandCodeProvider {
    async fn query_usage(&self, api_key: &str, base_url: &str) -> Result<UsageResult, String> {
        if !self.can_handle(base_url) {
            return Ok(UsageResult::failure("Unsupported Command Code API host"));
        }
        let api_key = api_key.trim();
        Ok(share_read(CREDITS_URL, api_key, async {
            let body = match fetch_official_usage(CREDITS_URL, api_key).await {
                Ok(body) => body,
                Err(error) => return UsageResult::failure(error),
            };
            let quota =
                remaining_credits(&body).and_then(|amount| UsageQuota::balance(amount, "Credits"));
            UsageResult::from_quotas(quota.into_iter().collect())
        })
        .await)
    }

    fn can_handle(&self, base_url: &str) -> bool {
        api_url(base_url, &["api.commandcode.ai"]).is_some_and(|url| url.port().is_none())
    }

    fn name(&self) -> &'static str {
        "Command Code"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_official_endpoint_and_excludes_relay_lookalikes() {
        assert!(CommandCodeProvider.can_handle("https://api.commandcode.ai/provider/v1"));
        for base in [
            "https://api.commandcode.ai.relay.example",
            "https://relay.example/api.commandcode.ai",
            "https://api.commandcode.ai:8443/provider/v1",
        ] {
            assert!(!CommandCodeProvider.can_handle(base));
        }
    }
}
