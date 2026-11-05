use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    antigravity_accounts, codex_accounts, minimax_accounts, qoder_accounts, zcode_accounts,
};
use crate::models::model::{AccountProvider, AccountSource};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccountOption {
    pub source: AccountSource,
    pub label: String,
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<minimax_accounts::Region>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub model_labels: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free_model_ids: Vec<String>,
}

fn option(
    provider: AccountProvider,
    id: String,
    label: String,
    plan: Option<String>,
) -> SavedAccountOption {
    SavedAccountOption {
        source: AccountSource {
            provider,
            account_id: id,
        },
        label,
        plan,
        region: None,
        model_labels: Default::default(),
        free_model_ids: Default::default(),
    }
}

/// This directory reads only EchoBird's saved accounts; it never probes a native client.
pub async fn list_accounts(provider: AccountProvider) -> Result<Vec<SavedAccountOption>, String> {
    if let Some(application) = super::plugin_accounts::application(provider) {
        if !super::plugin_accounts::available(application) {
            return Ok(Vec::new());
        }
        return serde_json::from_value(
            super::plugin_accounts::call(application, "accounts.options", serde_json::json!({}))
                .await?,
        )
        .map_err(|_| "Invalid account plugin directory".into());
    }
    use AccountProvider::*;
    let mut accounts: Vec<SavedAccountOption> = match provider {
        Cursor => super::cursor_accounts::speech::list_saved_accounts()
            .await?
            .into_iter()
            .map(|account| {
                option(
                    provider,
                    account.id,
                    account.email,
                    account.usage.and_then(|usage| usage.plan),
                )
            })
            .collect(),
        GrokBot => super::grok_bot_accounts::speech::list_saved_accounts()
            .await?
            .into_iter()
            .map(|account| {
                option(
                    provider,
                    account.id,
                    account.email,
                    account.usage.and_then(|usage| usage.plan),
                )
            })
            .collect(),
        QwenWork | QwenWorkCn => super::qwenwork_accounts::list(qwen_edition(provider))
            .await?
            .into_iter()
            .map(|account| {
                let label = account
                    .organization
                    .filter(|organization| !organization.trim().is_empty())
                    .map(|organization| format!("{} · {organization}", account.name))
                    .unwrap_or(account.name);
                option(
                    provider,
                    account.id,
                    label,
                    account.plan_name.or(account.plan),
                )
            })
            .collect(),
        DoubaoWork | Dola => unreachable!("Account capability is supplied by a plugin"),
        Dumate => super::dumate_accounts::list()
            .await?
            .into_iter()
            .filter(|account| !account.enterprise)
            .map(|account| option(provider, account.id, account.name, account.plan))
            .collect(),
        CodeArts => super::codearts_accounts::list_saved_accounts()?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        Devin => super::devin_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        OpenDesign => super::opendesign_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        ClaudeCode => unreachable!("Account capability is supplied by a plugin"),
        Codex => codex_accounts::list_saved_accounts()?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        WorkBuddy | WorkBuddyAi => unreachable!("Account capability is supplied by a plugin"),
        Antigravity => antigravity_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        Kilo => super::kilo_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        Copilot => super::copilot_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| option(provider, a.id, a.login, a.plan))
            .collect(),
        Trae | Traecn | Plugin(_) => unreachable!("Account capability is supplied by a plugin"),
        Minimax => minimax_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| {
                let mut saved = option(provider, a.id, a.email, a.plan);
                saved.region = Some(a.region);
                saved
            })
            .collect(),
        Qoder | QoderCn => qoder_accounts::list_saved_accounts(qoder_edition(provider))
            .await?
            .into_iter()
            .map(|a| {
                let labels = super::account_proxy::cached_qoder_model_labels(provider, &a.id);
                let mut saved = option(provider, a.id, a.name, a.plan);
                saved.model_labels = labels;
                saved
            })
            .collect(),
        Grok => super::grok_accounts::list_saved_accounts()?
            .into_iter()
            .map(|a| option(provider, a.id, a.email, a.plan))
            .collect(),
        Cline => super::cline_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| {
                let label = if a.email.trim().is_empty() {
                    a.name
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| a.id.clone())
                } else {
                    a.email
                };
                option(provider, a.id, label, a.plan)
            })
            .collect(),
        Kiro => super::kiro_accounts::list_saved_accounts()
            .await?
            .into_iter()
            .map(|a| {
                let labels = super::account_proxy::cached_kiro_model_labels(&a.id);
                let mut saved = option(provider, a.id, a.name, a.plan);
                saved.model_labels = labels;
                saved
            })
            .collect(),
        Kimi => super::kimi_accounts::list_saved_accounts()?
            .into_iter()
            .map(|a| {
                option(
                    provider,
                    a.id,
                    format!(
                        "{} · {}",
                        if a.region == super::kimi_accounts::Region::Cn {
                            "kimi.com"
                        } else {
                            "kimi.ai"
                        },
                        a.email
                    ),
                    a.plan,
                )
            })
            .collect(),
        Zcode => zcode_accounts::list_saved_accounts()?
            .into_iter()
            .map(|a| {
                let site = if a.provider == "bigmodel" {
                    "BigModel"
                } else {
                    "Z.ai"
                };
                option(provider, a.id, format!("{site} · {}", a.email), None)
            })
            .collect(),
    };
    for account in &mut accounts {
        account.free_model_ids = match provider {
            Qoder | QoderCn => super::account_proxy::cached_qoder_free_model_ids(
                provider,
                &account.source.account_id,
            ),
            _ => super::account_proxy::cached_free_model_ids(provider, &account.source.account_id),
        };
    }
    Ok(accounts)
}

fn qoder_edition(provider: AccountProvider) -> qoder_accounts::Edition {
    if provider == AccountProvider::QoderCn {
        qoder_accounts::Edition::QoderCn
    } else {
        qoder_accounts::Edition::Qoder
    }
}

fn qwen_edition(provider: AccountProvider) -> super::qwenwork_accounts::Edition {
    if provider == AccountProvider::QwenWorkCn {
        super::qwenwork_accounts::Edition::QwenWorkCn
    } else {
        super::qwenwork_accounts::Edition::QwenWork
    }
}

pub async fn validate_source(source: &AccountSource) -> Result<(), String> {
    validate_saved_source(source, &list_accounts(source.provider).await?)
}

pub(crate) fn finish_account_deletion(
    provider: AccountProvider,
    account_id: &str,
    result: Result<(), String>,
) -> Result<(), String> {
    result?;
    super::smart_router::remove_account_candidates(&AccountSource {
        provider,
        account_id: account_id.into(),
    })?;
    super::smart_router::forget_account_authentication(provider, account_id);
    Ok(())
}

fn validate_saved_source(
    source: &AccountSource,
    saved: &[SavedAccountOption],
) -> Result<(), String> {
    if saved.iter().any(|a| a.source == *source) {
        Ok(())
    } else {
        Err("accountError.invalidAccount".into())
    }
}

/// Resolve only the saved endpoint metadata; connection tests never renew tokens.
pub(crate) async fn connection_endpoint(source: &AccountSource) -> Result<String, String> {
    if let Some(application) = super::plugin_accounts::application(source.provider) {
        validate_source(source).await?;
        return serde_json::from_value(
            super::plugin_accounts::call(
                application,
                "connection",
                serde_json::json!({"accountId":source.account_id}),
            )
            .await?,
        )
        .map_err(|_| "Invalid account plugin endpoint".into());
    }
    use AccountProvider::*;
    let metadata = match source.provider {
        Dumate | DoubaoWork | Dola => {
            validate_source(source).await?;
            Value::Null
        }
        Minimax => {
            let account = minimax_accounts::list_saved_accounts()
                .await?
                .into_iter()
                .find(|account| account.id == source.account_id)
                .ok_or("accountError.invalidAccount")?;
            serde_json::json!({"region": account.region})
        }
        Kimi => {
            let account = super::kimi_accounts::list_saved_accounts()?
                .into_iter()
                .find(|account| account.id == source.account_id)
                .ok_or("accountError.invalidAccount")?;
            serde_json::json!({"baseUrl": account.region.base()})
        }
        Kiro => super::kiro_accounts::connection_metadata(&source.account_id)?,
        Trae | Traecn | Plugin(_) => unreachable!("Account capability is supplied by a plugin"),
        Copilot => super::copilot_accounts::proxy_credentials(&source.account_id, false).await?,
        Devin => super::devin_accounts::proxy_credentials(&source.account_id).await?,
        _ => Value::Null,
    };
    super::account_proxy::connection_base(source.provider, &metadata)
        .map_err(|error| error.to_string())
}

/// Secrets are deliberately not exposed by any Tauri command.
pub(crate) async fn credentials(
    source: &AccountSource,
    force_refresh: bool,
) -> Result<Value, String> {
    if let Some(application) = super::plugin_accounts::application(source.provider) {
        validate_source(source).await?;
        return Ok(
            serde_json::json!({"_pluginApplication":application,"accountId":source.account_id}),
        );
    }
    use AccountProvider::*;
    match source.provider {
        Cursor => {
            super::cursor_accounts::speech::proxy_credentials(&source.account_id, force_refresh)
                .await
        }
        GrokBot => {
            super::grok_bot_accounts::speech::proxy_credentials(&source.account_id, force_refresh)
                .await
        }
        QwenWork | QwenWorkCn => {
            super::qwenwork_accounts::proxy_credentials(
                qwen_edition(source.provider),
                &source.account_id,
                force_refresh,
            )
            .await
        }
        DoubaoWork | Dola => unreachable!("Account capability is supplied by a plugin"),
        Dumate => {
            super::dumate_accounts::proxy_credentials(&source.account_id, force_refresh).await
        }
        CodeArts => {
            super::codearts_accounts::proxy_credentials(&source.account_id, force_refresh).await
        }
        Devin => super::devin_accounts::proxy_credentials(&source.account_id).await,
        OpenDesign => super::opendesign_accounts::proxy_credentials(&source.account_id).await,
        ClaudeCode => unreachable!("Account capability is supplied by a plugin"),
        Codex => codex_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        WorkBuddy | WorkBuddyAi => unreachable!("Account capability is supplied by a plugin"),
        Antigravity => {
            antigravity_accounts::proxy_credentials(&source.account_id, force_refresh).await
        }
        Kilo => super::kilo_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        Copilot => {
            super::copilot_accounts::proxy_credentials(&source.account_id, force_refresh).await
        }
        Trae | Traecn | Plugin(_) => unreachable!("Account capability is supplied by a plugin"),
        Minimax => minimax_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        Qoder | QoderCn => {
            qoder_accounts::proxy_credentials(
                qoder_edition(source.provider),
                &source.account_id,
                force_refresh,
            )
            .await
        }
        Grok => super::grok_accounts::proxy::credentials(&source.account_id, force_refresh).await,
        Cline => super::cline_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        Kiro => super::kiro_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        Kimi => super::kimi_accounts::proxy_credentials(&source.account_id, force_refresh).await,
        Zcode => zcode_accounts::proxy_credentials(&source.account_id, force_refresh).await,
    }
    .map(|credentials| preparation_credentials(credentials, source))
}

pub(crate) async fn credentials_after_rejection(
    source: &AccountSource,
    rejected_credentials: &Value,
) -> Result<Value, String> {
    if super::plugin_accounts::application(source.provider).is_some() {
        return Err("accountError.loginRequired".into());
    }
    use AccountProvider::*;
    match source.provider {
        Cursor => {
            super::cursor_accounts::speech::speech_credentials(
                &source.account_id,
                Some(rejected_credentials),
            )
            .await
        }
        GrokBot => {
            super::grok_bot_accounts::speech::speech_credentials(
                &source.account_id,
                Some(rejected_credentials),
            )
            .await
        }
        QwenWork | QwenWorkCn => {
            super::qwenwork_accounts::speech_credentials(
                qwen_edition(source.provider),
                &source.account_id,
                Some(rejected_credentials),
            )
            .await
        }
        DoubaoWork | Dola => unreachable!("Account capability is supplied by a plugin"),
        Dumate => {
            super::dumate_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        CodeArts => {
            super::codearts_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Devin => {
            let current = super::devin_accounts::proxy_credentials(&source.account_id).await?;
            if current["accessToken"] == rejected_credentials["accessToken"] {
                return Err("accountError.loginRequired".into());
            }
            Ok(current)
        }
        OpenDesign => {
            let current = super::opendesign_accounts::proxy_credentials(&source.account_id).await?;
            if current["accessToken"] == rejected_credentials["accessToken"] {
                return Err("accountError.loginRequired".into());
            }
            Ok(current)
        }
        ClaudeCode => unreachable!("Account capability is supplied by a plugin"),
        Codex => {
            codex_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        WorkBuddy | WorkBuddyAi => unreachable!("Account capability is supplied by a plugin"),
        Antigravity => {
            antigravity_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Kilo | Copilot => credentials(source, true).await,
        Kimi => {
            super::kimi_accounts::credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Grok => {
            super::grok_accounts::proxy::after_rejection(&source.account_id, rejected_credentials)
                .await
        }
        Cline => {
            super::cline_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Kiro => {
            super::kiro_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Zcode => {
            zcode_accounts::proxy_credentials_after_rejection(
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Minimax => {
            minimax_accounts::credentials_after_rejection(&source.account_id, rejected_credentials)
                .await
        }
        Qoder | QoderCn => {
            qoder_accounts::proxy_credentials_after_rejection(
                qoder_edition(source.provider),
                &source.account_id,
                rejected_credentials,
            )
            .await
        }
        Trae | Traecn | Plugin(_) => unreachable!("Account capability is supplied by a plugin"),
    }
    .map(|credentials| preparation_credentials(credentials, source))
}

pub(crate) fn credential_error(message: String) -> super::account_proxy::ProxyError {
    use axum::http::StatusCode;
    let key = message.split('|').next().unwrap_or_default();
    let http_status = message
        .split('|')
        .find_map(|part| part.trim().strip_prefix("HTTP "))
        .and_then(|status| status.parse::<u16>().ok())
        .and_then(|status| StatusCode::from_u16(status).ok());
    let status = match key {
        "accountError.invalidAccount"
        | "accountError.noAccount"
        | "accountError.loginRequired"
        | "accountError.expired"
        | "accountError.invalidGrant" => StatusCode::UNAUTHORIZED,
        "accountError.rateLimited" => StatusCode::TOO_MANY_REQUESTS,
        "accountError.network" | "accountError.unavailable" => http_status
            .filter(|status| status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS)
            .unwrap_or(StatusCode::BAD_GATEWAY),
        "accountError.auth" => http_status.unwrap_or(StatusCode::UNAUTHORIZED),
        "accountError.format"
        | "accountError.authResponse"
        | "accountError.home"
        | "accountError.keychain"
        | "accountError.initializeClient" => StatusCode::BAD_REQUEST,
        _ => http_status.unwrap_or(StatusCode::BAD_GATEWAY),
    };
    let code = (key.starts_with("accountError.")
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'))
    .then(|| key.to_string());
    let mut error = super::account_proxy::ProxyError::new(status, message);
    error.error_code = code;
    error
}

pub(crate) fn oauth_refresh_error(status: u16, body: &Value) -> String {
    let code = body["error"]
        .as_str()
        .or_else(|| body["error"]["code"].as_str());
    if status == 400
        && code.is_some_and(|code| {
            matches!(
                code,
                "invalid_grant"
                    | "refresh_token_expired"
                    | "refresh_token_reused"
                    | "refresh_token_invalidated"
            )
        })
    {
        format!("accountError.loginRequired|HTTP {status}")
    } else {
        format!("accountError.auth|HTTP {status}")
    }
}

fn model_directory_credentials(mut credentials: Value, source: &AccountSource) -> Value {
    credentials["_echobird_saved_account_id"] = Value::String(source.account_id.clone());
    credentials
}

fn preparation_credentials(credentials: Value, source: &AccountSource) -> Value {
    if matches!(
        source.provider,
        AccountProvider::Antigravity | AccountProvider::Copilot | AccountProvider::Kimi
    ) {
        model_directory_credentials(credentials, source)
    } else {
        credentials
    }
}

pub async fn list_models(source: &AccountSource) -> Result<Vec<String>, String> {
    super::account_proxy::require_text_provider(source.provider).map_err(|error| error.message)?;
    let secret = model_directory_credentials(credentials(source, false).await?, source);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network")?;
    let result = super::account_proxy::list_models(source.provider, secret.clone(), &client).await;
    match result {
        Err(error)
            if matches!(
                source.provider,
                AccountProvider::Trae
                    | AccountProvider::Devin
                    | AccountProvider::CodeArts
                    | AccountProvider::Traecn
                    | AccountProvider::Minimax
                    | AccountProvider::Qoder
                    | AccountProvider::QoderCn
                    | AccountProvider::Grok
                    | AccountProvider::Cline
                    | AccountProvider::Kiro
                    | AccountProvider::Zcode
                    | AccountProvider::Kimi
            ) && error.status == axum::http::StatusCode::UNAUTHORIZED =>
        {
            let refreshed = model_directory_credentials(
                credentials_after_rejection(source, &secret).await?,
                source,
            );
            super::account_proxy::list_models(source.provider, refreshed, &client).await
        }
        result => result,
    }
    .map_err(|e| e.message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use serde_json::json;

    #[test]
    fn failed_account_deletion_does_not_start_route_cleanup() {
        assert_eq!(
            finish_account_deletion(
                AccountProvider::CodeArts,
                "fixture",
                Err("accountError.write".into())
            ),
            Err("accountError.write".into())
        );
    }

    #[test]
    fn preparation_credentials_bind_only_the_saved_metadata_accounts() {
        let credentials = json!({
            "accessToken":"fixture-token", "accountId":"native-account",
            "projectId":"fixture-project", "baseUrl":"https://api.example.test",
            "headers":{"X-Fixture":"preserved"}, "protocol":"responses",
            "_echobird_saved_account_id":"old-saved-account"
        });
        for provider in [AccountProvider::Antigravity, AccountProvider::Copilot] {
            let source = AccountSource {
                provider,
                account_id: "current-saved-account".into(),
            };
            let mut expected = credentials.clone();
            expected["_echobird_saved_account_id"] = json!(source.account_id);
            let bound = preparation_credentials(credentials.clone(), &source);
            assert_eq!(bound, expected);
            assert_eq!(preparation_credentials(bound.clone(), &source), bound);
        }
        for provider in [
            AccountProvider::ClaudeCode,
            AccountProvider::Codex,
            AccountProvider::WorkBuddy,
            AccountProvider::WorkBuddyAi,
            AccountProvider::Kilo,
            AccountProvider::Trae,
            AccountProvider::Traecn,
            AccountProvider::Grok,
            AccountProvider::Cline,
            AccountProvider::Kiro,
            AccountProvider::Minimax,
            AccountProvider::Qoder,
            AccountProvider::QoderCn,
        ] {
            let source = AccountSource {
                provider,
                account_id: "another-saved-account".into(),
            };
            assert_eq!(
                preparation_credentials(credentials.clone(), &source),
                credentials,
                "{provider:?}"
            );
        }
    }

    #[test]
    fn account_sources_keep_new_provider_regions_and_wire_names_distinct() {
        assert_eq!(serde_json::to_value(AccountProvider::Kiro).unwrap(), "kiro");
        let kiro = option(
            AccountProvider::Kiro,
            "saved-kiro-account".into(),
            "Kiro account".into(),
            Some("Pro+".into()),
        );
        let source = serde_json::to_value(&kiro).unwrap();
        assert_eq!(source["source"]["provider"], "kiro");
        assert_eq!(source["source"]["accountId"], "saved-kiro-account");
        assert_eq!(source["plan"], "Pro+");
        assert!(source.get("accessToken").is_none());
        assert!(source.get("credentialGeneration").is_none());
        assert_eq!(
            qoder_edition(AccountProvider::Qoder),
            qoder_accounts::Edition::Qoder
        );
        assert_eq!(
            qoder_edition(AccountProvider::QoderCn),
            qoder_accounts::Edition::QoderCn
        );
        assert_eq!(
            serde_json::to_value(AccountProvider::QoderCn).unwrap(),
            "qoderCn"
        );
        let mut saved = option(
            AccountProvider::Minimax,
            "global-account".into(),
            "fixture@example.test".into(),
            None,
        );
        saved.region = Some(minimax_accounts::Region::Global);
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value["region"], "global");
        assert_eq!(value["source"]["provider"], "minimax");
        assert!(value.get("accessToken").is_none());
        assert!(serde_json::to_value(option(
            AccountProvider::Qoder,
            "id".into(),
            "Fixture".into(),
            None
        ))
        .unwrap()
        .get("region")
        .is_none());
    }

    #[test]
    fn credential_failures_preserve_known_categories_and_http_statuses() {
        use axum::http::StatusCode;
        for (message, expected) in [
            ("accountError.invalidAccount", StatusCode::UNAUTHORIZED),
            (
                "accountError.loginRequired|HTTP 400|invalid_grant",
                StatusCode::UNAUTHORIZED,
            ),
            (
                "accountError.auth|HTTP 503",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                "accountError.network|HTTP 429",
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (
                "accountError.rateLimited|HTTP 429",
                StatusCode::TOO_MANY_REQUESTS,
            ),
            ("accountError.quota|HTTP 402", StatusCode::PAYMENT_REQUIRED),
            (
                "accountError.network|request timed out",
                StatusCode::BAD_GATEWAY,
            ),
            ("accountError.authResponse", StatusCode::BAD_REQUEST),
            ("accountError.format", StatusCode::BAD_REQUEST),
            ("accountError.expired", StatusCode::UNAUTHORIZED),
            ("unknown credential failure", StatusCode::BAD_GATEWAY),
        ] {
            let error = credential_error(message.into());
            assert_eq!(error.status, expected, "{message}");
            assert_eq!(error.message, message);
            assert!(!error.model_scope);
        }
        assert!(credential_error("unknown credential failure".into())
            .error_code
            .is_none());
        assert_eq!(
            credential_error("accountError.rateLimited".into())
                .error_code
                .as_deref(),
            Some("accountError.rateLimited")
        );
    }

    #[test]
    fn free_model_metadata_is_optional_and_contains_no_credentials() {
        let mut saved = option(
            AccountProvider::WorkBuddy,
            "saved-id".into(),
            "Fixture".into(),
            Some("Free".into()),
        );
        assert!(serde_json::to_value(&saved)
            .unwrap()
            .get("freeModelIds")
            .is_none());
        saved.free_model_ids = vec!["free-model".into()];
        assert_eq!(
            serde_json::to_value(&saved).unwrap(),
            serde_json::json!({
                "source":{"provider":"workBuddy","accountId":"saved-id"},
                "label":"Fixture",
                "plan":"Free",
                "freeModelIds":["free-model"]
            })
        );
    }

    #[test]
    fn model_directory_binding_preserves_codex_identity_for_initial_and_refreshed_credentials() {
        let source = AccountSource {
            provider: AccountProvider::Codex,
            account_id: "saved-echo-account".into(),
        };
        for token in ["initial-token", "refreshed-token"] {
            let credentials = model_directory_credentials(
                json!({"accountId":"native-chatgpt-account","accessToken":token,"_echobird_saved_account_id":"old-id"}),
                &source,
            );
            assert_eq!(credentials["accountId"], "native-chatgpt-account");
            assert_eq!(credentials["_echobird_saved_account_id"], source.account_id);
            assert_eq!(credentials["accessToken"], token);
        }
    }

    #[test]
    fn only_explicit_refresh_credential_failures_require_login_again() {
        use axum::http::StatusCode;
        for code in [
            "invalid_grant",
            "refresh_token_expired",
            "refresh_token_reused",
            "refresh_token_invalidated",
        ] {
            for body in [json!({"error": code}), json!({"error": {"code": code}})] {
                let message = oauth_refresh_error(400, &body);
                assert_eq!(message, "accountError.loginRequired|HTTP 400");
                assert_eq!(credential_error(message).status, StatusCode::UNAUTHORIZED);
            }
        }
        for body in [
            json!({"error":"invalid_request"}),
            json!({"error":{"code":"invalid_client"}}),
            json!({"message":"invalid_grant"}),
            Value::Null,
        ] {
            let message = oauth_refresh_error(400, &body);
            assert_eq!(message, "accountError.auth|HTTP 400");
            assert_eq!(credential_error(message).status, StatusCode::BAD_REQUEST);
        }
        for (status, expected) in [
            (401, StatusCode::UNAUTHORIZED),
            (429, StatusCode::TOO_MANY_REQUESTS),
            (503, StatusCode::SERVICE_UNAVAILABLE),
        ] {
            assert_eq!(
                credential_error(oauth_refresh_error(
                    status,
                    &json!({"error":"invalid_grant"})
                ))
                .status,
                expected
            );
        }
    }

    #[test]
    fn source_validation_is_bound_to_saved_account_and_provider() {
        let saved = vec![option(
            AccountProvider::WorkBuddy,
            "saved-id".into(),
            "Fixture".into(),
            None,
        )];
        let mut source = AccountSource {
            provider: AccountProvider::WorkBuddy,
            account_id: "saved-id".into(),
        };
        assert!(validate_saved_source(&source, &saved).is_ok());
        source.provider = AccountProvider::WorkBuddyAi;
        assert!(validate_saved_source(&source, &saved).is_err());
        source.provider = AccountProvider::WorkBuddy;
        source.account_id = "missing-id".into();
        assert!(validate_saved_source(&source, &saved).is_err());
        assert!(validate_saved_source(&source, &[]).is_err());
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(value[0]["source"]["accountId"], "saved-id");
        assert!(value[0].get("accessToken").is_none());
    }

    #[test]
    fn trae_sources_keep_region_and_saved_identity_separate() {
        let saved = vec![option(
            AccountProvider::Trae,
            "saved-id".into(),
            "Fixture".into(),
            Some("Pro".into()),
        )];
        let source = AccountSource {
            provider: AccountProvider::Trae,
            account_id: "saved-id".into(),
        };
        assert!(validate_saved_source(&source, &saved).is_ok());
        assert!(validate_saved_source(
            &AccountSource {
                provider: AccountProvider::Traecn,
                ..source.clone()
            },
            &saved
        )
        .is_err());
        let value = serde_json::to_value(&saved).unwrap();
        assert_eq!(
            value[0],
            json!({"source":{"provider":"trae","accountId":"saved-id"},"label":"Fixture","plan":"Pro"})
        );
        assert_eq!(
            serde_json::to_value(AccountProvider::Traecn).unwrap(),
            "traecn"
        );
    }

    #[tokio::test]
    #[ignore = "explicit live saved-account catalog verification; select ECHOBIRD_LIVE_PROXY_PROVIDER"]
    async fn live_saved_account_catalog_probe() -> Result<(), String> {
        let name = std::env::var("ECHOBIRD_LIVE_PROXY_PROVIDER").map_err(|_| {
            "ECHOBIRD_LIVE_PROXY_PROVIDER must select an authorized provider".to_owned()
        })?;
        let provider: AccountProvider = serde_json::from_value(Value::String(name))
            .map_err(|_| "invalid ECHOBIRD_LIVE_PROXY_PROVIDER".to_owned())?;
        let accounts = list_accounts(provider)
            .await
            .map_err(|_| format!("provider={provider:?} modelCount=0 status=store_error"))?;
        let source = accounts
            .first()
            .ok_or_else(|| format!("provider={provider:?} modelCount=0 status=no_saved_account"))?
            .source
            .clone();
        let secret = credentials(&source, false).await.map_err(|error| {
            let code = error.split('|').next().unwrap_or_default();
            let code = if code.starts_with("accountError.")
                && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
            {
                code
            } else {
                "credential_error"
            };
            format!("provider={provider:?} modelCount=0 status={code}")
        })?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|_| "catalog client unavailable".to_owned())?;
        let models = super::super::account_proxy::list_models(provider, secret, &client)
            .await
            .map_err(|error| {
                format!(
                    "provider={provider:?} modelCount=0 status=catalog_http_{}",
                    error.status.as_u16()
                )
            })?;
        if models.is_empty() {
            return Err(format!(
                "provider={provider:?} modelCount=0 status=empty_catalog"
            ));
        }
        eprintln!(
            "provider={provider:?} modelCount={} status=ok",
            models.len()
        );
        if std::env::var("ECHOBIRD_LIVE_PROXY_SHOW_MODELS").as_deref() == Ok("1") {
            eprintln!("models={models:?}");
        }
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicitly save an existing authorized native sign-in in EchoBird; ECHOBIRD_LIVE_PROXY_IMPORT_NATIVE=1 required"]
    async fn live_import_installed_account_probe() -> Result<(), String> {
        if std::env::var("ECHOBIRD_LIVE_PROXY_IMPORT_NATIVE").as_deref() != Ok("1") {
            return Err("native account import flag is required".into());
        }
        let provider = serde_json::from_value::<AccountProvider>(json!(std::env::var(
            "ECHOBIRD_LIVE_PROXY_PROVIDER"
        )
        .map_err(|_| "provider is required")?))
        .map_err(|_| "invalid provider")?;
        let result = match provider {
            AccountProvider::Kilo => super::super::kilo_accounts::capture_current_account()
                .await
                .map(|_| ()),
            AccountProvider::Copilot => super::super::copilot_accounts::import_native_accounts()
                .await
                .map(|_| ()),
            _ => return Err("this probe accepts Kilo or Copilot only".into()),
        };
        result.map_err(|_| format!("provider={provider:?} status=native_import_failed"))?;
        let count = list_accounts(provider).await?.len();
        if count == 0 {
            return Err(format!("provider={provider:?} status=no_native_account"));
        }
        eprintln!("provider={provider:?} savedAccountCount={count} status=ok");
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit request to the current local router; ECHOBIRD_LIVE_RUNNING_ROUTER=1 required"]
    async fn live_running_router_inference_probe() -> Result<(), String> {
        if std::env::var("ECHOBIRD_LIVE_RUNNING_ROUTER").as_deref() != Ok("1") {
            return Err("running router verification flag is required".into());
        }
        let port = std::env::var("ECHOBIRD_LIVE_ROUTER_PORT")
            .ok()
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
            .ok_or("verified running router port is required")?;
        let key = super::super::smart_router::api_key_for_use()
            .map_err(|_| "local router credentials unavailable")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(75))
            .build()
            .map_err(|_| "local router client unavailable")?;
        let response = client
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .bearer_auth(key)
            .json(&json!({"model":"auto","stream":false,
                "messages":[{"role":"user","content":"Reply with only OK."}]}))
            .send()
            .await
            .map_err(|_| "local router request failed")?;
        let status = response.status();
        let routed = response.headers().get("X-EchoBird-Routed-Via").is_some();
        let value: Value = response
            .json()
            .await
            .map_err(|_| "local router JSON collection failed")?;
        let safe_label = |value: &Value| {
            value
                .as_str()
                .filter(|label| {
                    label.len() <= 80
                        && label
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                })
                .unwrap_or("other")
                .to_string()
        };
        if !status.is_success() {
            let attempts: Vec<_> = value["error"]["attempts"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|attempt| {
                    json!({"status":attempt["status"].as_u64(),
                    "failure":safe_label(&attempt["failure"])})
                })
                .collect();
            return Err(format!(
                "local router status={} code={} type={} attempts={attempts:?}",
                status.as_u16(),
                safe_label(&value["error"]["code"]),
                safe_label(&value["error"]["type"]),
            ));
        }
        if !routed
            || value["choices"][0]["message"]["content"]
                .as_str()
                .is_none_or(|text| text.trim().is_empty())
        {
            return Err("local router response lacked routing evidence or assistant text".into());
        }
        eprintln!(
            "local router status={} routed=true text=true",
            status.as_u16()
        );
        Ok(())
    }

    #[tokio::test]
    #[ignore = "explicit inference against a saved account; requires provider, model, and ECHOBIRD_LIVE_PROXY_INFERENCE=1"]
    async fn live_saved_account_inference_probe() -> Result<(), String> {
        if std::env::var("ECHOBIRD_LIVE_PROXY_INFERENCE").as_deref() != Ok("1") {
            return Err("live inference flag is required".into());
        }
        let provider = serde_json::from_value::<AccountProvider>(json!(std::env::var(
            "ECHOBIRD_LIVE_PROXY_PROVIDER"
        )
        .map_err(|_| "provider is required")?))
        .map_err(|_| "invalid provider")?;
        let model = std::env::var("ECHOBIRD_LIVE_PROXY_MODEL").map_err(|_| "model is required")?;
        let source = list_accounts(provider)
            .await?
            .first()
            .ok_or("no saved account")?
            .source
            .clone();
        let secret = credentials(&source, false)
            .await
            .map_err(|_| "saved credentials unavailable")?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .map_err(|_| "client unavailable")?;
        let models = super::super::account_proxy::list_models(provider, secret.clone(), &client)
            .await
            .map_err(|e| format!("catalog status={}", e.status.as_u16()))?;
        if !models.contains(&model) {
            return Err("selected model is absent from account catalog".into());
        }
        let mut request = json!({"stream":false,"max_tokens":128,
            "messages":[{"role":"user","content":"Reply with only OK."}]});
        let native_limits = matches!(provider, AccountProvider::Trae | AccountProvider::Traecn);
        if native_limits {
            request.as_object_mut().unwrap().remove("max_tokens");
        }
        // Only the tool result contains this value, so ignoring it cannot pass.
        let result_marker = format!("probe-result-{}", uuid::Uuid::new_v4());
        let mut reply = Value::Null;
        let phases: &[&str] =
            if std::env::var("ECHOBIRD_LIVE_PROXY_TEXT_ONLY").as_deref() == Ok("1") {
                &["text"]
            } else {
                &["text", "stream", "tool", "continuation"]
            };
        for &phase in phases {
            let mut body = request.clone();
            if phase == "stream" {
                body["stream"] = json!(true);
            }
            if matches!(phase, "tool" | "continuation") {
                body["tools"] = json!([{"type":"function","function":{
                    "name":"echo_probe","description":"Return the probe marker.",
                    "parameters":{"type":"object","properties":{"marker":{"type":"string","enum":["probe-ok"]}},
                        "required":["marker"],"additionalProperties":false}}}]);
                if phase == "tool" {
                    body["messages"][0]["content"] = json!("Call echo_probe with marker probe-ok.");
                    body["tool_choice"] = if native_limits {
                        json!("auto")
                    } else {
                        json!({"type":"function","function":{"name":"echo_probe"}})
                    };
                } else {
                    let call = &reply["tool_calls"][0];
                    body["messages"] = json!([
                        {"role":"user","content":"Call echo_probe with marker probe-ok."},
                        reply,
                        {"role":"tool","tool_call_id":call["id"],"content":result_marker},
                        {"role":"user","content":"The tool succeeded. Reply with only its returned value."}
                    ]);
                    body["tool_choice"] = json!("none");
                }
            }
            let response = super::super::smart_router::route_live_account_probe(
                source.clone(),
                model.clone(),
                body,
            )
            .await?;
            if !response.status().is_success() {
                let status = response.status();
                let bytes = to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .map_err(|_| "error response collection failed")?;
                let value = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
                let message = value["error"]["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let hints: Vec<_> = [
                    "max_tokens",
                    "max_completion_tokens",
                    "temperature",
                    "reasoning",
                    "unsupported",
                    "invalid",
                    "model",
                    "quota",
                    "integration",
                    "permission",
                ]
                .into_iter()
                .filter(|hint| message.contains(hint))
                .collect();
                let attempts: Vec<_> = value["error"]["attempts"]
                    .as_array().into_iter().flatten()
                    .map(|attempt| json!({"status":attempt["status"].as_u64(),
                        "failure":attempt["failure"].as_str().filter(|label|
                            label.len() <= 40 && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))}))
                    .collect();
                return Err(format!(
                    "provider={provider:?} phase={phase} status={} hints={hints:?} attempts={attempts:?}",
                    status.as_u16()
                ));
            }
            if response.headers().get("X-EchoBird-Routed-Via").is_none() {
                return Err("response did not pass through the smart router".into());
            }
            let bytes = to_bytes(response.into_body(), 1024 * 1024)
                .await
                .map_err(|_| format!("phase={phase} response collection failed"))?;
            if phase == "stream" {
                let frames = std::str::from_utf8(&bytes).map_err(|_| "invalid stream encoding")?;
                let content = frames
                    .lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                    .any(|value| {
                        value["choices"][0]["delta"]["content"]
                            .as_str()
                            .is_some_and(|text| !text.is_empty())
                    });
                if !content || !frames.contains("data: [DONE]") {
                    return Err("stream did not contain text and completion".into());
                }
            } else {
                let value: Value =
                    serde_json::from_slice(&bytes).map_err(|_| "invalid JSON response")?;
                reply = value["choices"][0]["message"].clone();
                if phase == "tool" {
                    let call = &reply["tool_calls"][0];
                    let arguments = serde_json::from_str::<Value>(
                        call["function"]["arguments"].as_str().unwrap_or_default(),
                    )
                    .ok();
                    if reply["tool_calls"].as_array().map(Vec::len) != Some(1)
                        || call["function"]["name"] != "echo_probe"
                        || !call["id"].is_string()
                        || arguments.as_ref().and_then(|args| args["marker"].as_str())
                            != Some("probe-ok")
                    {
                        let finish = match value["choices"][0]["finish_reason"].as_str() {
                            Some("length") => "length",
                            Some("tool_calls") => "tool_calls",
                            Some("stop") => "stop",
                            _ => "other",
                        };
                        return Err(format!(
                            "incomplete tool response: finish={finish} count={:?} named={} id={} argsBytes={} argsJson={} marker={} text={} completionTokens={:?} reasoningTokens={:?}",
                            reply["tool_calls"].as_array().map(Vec::len),
                            call["function"]["name"] == "echo_probe",
                            call["id"].is_string(),
                            call["function"]["arguments"].as_str().map_or(0, str::len),
                            arguments.is_some(),
                            arguments.as_ref().and_then(|args| args["marker"].as_str()) == Some("probe-ok"),
                            reply["content"].as_str().is_some_and(|text| !text.is_empty()),
                            value["usage"]["completion_tokens"].as_u64(),
                            value["usage"]["completion_tokens_details"]["reasoning_tokens"].as_u64(),
                        ));
                    }
                } else if reply["content"]
                    .as_str()
                    .is_none_or(|text| text.trim().is_empty())
                {
                    return Err(format!("phase={phase} has no assistant text"));
                }
                if phase == "continuation"
                    && (reply["content"].as_str().map(str::trim) != Some(result_marker.as_str())
                        || reply["tool_calls"]
                            .as_array()
                            .is_some_and(|calls| !calls.is_empty()))
                {
                    return Err("continuation ignored the tool result or tool_choice=none".into());
                }
            }
            eprintln!("provider={provider:?} phase={phase} status=ok");
        }
        Ok(())
    }
}
