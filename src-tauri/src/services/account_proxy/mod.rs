//! Account credentials stay in the backend. These adapters never switch native clients.

pub(crate) mod antigravity_voice;
pub(crate) mod audio_format;
mod cline;
mod codearts;
pub(crate) mod codearts_voice;
pub(crate) mod codex_voice;
pub(crate) mod cursor_voice;
mod devin;
pub(crate) mod devin_voice;
pub(crate) mod dumate_voice;
mod grok;
pub(crate) mod grokbot_voice;
pub(crate) mod kilo_voice;
mod kiro;
mod minimax;
pub(crate) mod minimax_voice;
mod protocol;
mod providers;
mod qoder;
pub(crate) mod qoder_voice;
pub(crate) mod qwenwork_voice;
mod responses;
mod signatures;
mod tools;
pub(crate) mod voice;
mod zcode;

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::{json, Value};

use crate::models::model::AccountProvider;

pub(crate) use codearts::signed_headers as codearts_signed_headers;
pub(crate) use zcode::bind_session as bind_zcode_session;

pub(crate) fn connection_base(
    provider: AccountProvider,
    metadata: &Value,
) -> Result<String, ProxyError> {
    require_text_provider(provider)?;
    use AccountProvider::*;
    match provider {
        ClaudeCode | WorkBuddy | WorkBuddyAi | DoubaoWork | Dola | Trae | Traecn | Plugin(_) => {
            Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Account capability is supplied by a plugin",
            ))
        }
        CodeArts => Ok(codearts::api(metadata).into()),
        Devin => Ok(metadata["baseUrl"]
            .as_str()
            .unwrap_or(crate::services::devin_accounts::SERVER)
            .into()),
        Grok => grok::base(metadata),
        Kiro => kiro::base(metadata, false),
        Minimax => minimax::base(metadata),
        Qoder => Ok(crate::services::qoder_accounts::Edition::Qoder
            .gateway()
            .into()),
        QoderCn => Ok(crate::services::qoder_accounts::Edition::QoderCn
            .gateway()
            .into()),
        Zcode => Ok(zcode::origin(metadata).into()),
        _ => Ok(providers::base(provider, metadata).into()),
    }
}

pub(crate) use protocol::SseParser;
#[cfg(test)]
pub(crate) fn remember_free_model_ids(
    provider: AccountProvider,
    credentials: &Value,
    ids: Vec<String>,
) {
    providers::remember_free_model_ids(provider, credentials, ids);
}
pub use providers::responses_response;
pub(crate) use providers::{context_limit_error, stream_error};
pub use responses::{chat_to_responses, chat_to_responses_with_tools, responses_to_chat};
pub(crate) use responses::{normalize_responses_tool_rounds, GoogleThought};

pub(crate) fn declares_hosted_tools(body: &Value) -> bool {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|tool| tools::is_hosted(tool["type"].as_str().unwrap_or_default()))
}

pub(crate) fn chat_request_to_responses(body: &Value, model: &str) -> Value {
    protocol::chat_to_responses(body, model)
}

pub(crate) async fn convert_public_model_stream(
    response: reqwest::Response,
    model: &str,
    streaming: bool,
    responses: bool,
) -> Result<Response, ProxyError> {
    let protocol = if responses {
        protocol::WireProtocol::Responses
    } else {
        protocol::WireProtocol::Chat
    };
    providers::converted(response, protocol, model, streaming, true).await
}

pub(crate) fn forget_preparation_metadata(provider: AccountProvider, account_id: &str) {
    providers::forget_metadata(provider, account_id);
}

pub(crate) fn cached_qoder_model_labels(
    provider: AccountProvider,
    account_id: &str,
) -> std::collections::BTreeMap<String, String> {
    qoder::cached_model_labels(provider, account_id)
}

pub(crate) fn cached_kiro_model_labels(
    account_id: &str,
) -> std::collections::BTreeMap<String, String> {
    kiro::cached_model_labels(account_id)
}

pub(crate) fn cached_free_model_ids(provider: AccountProvider, account_id: &str) -> Vec<String> {
    providers::cached_free_model_ids(provider, account_id)
}

pub(crate) fn cached_qoder_free_model_ids(
    provider: AccountProvider,
    account_id: &str,
) -> Vec<String> {
    qoder::cached_free_model_ids(provider, account_id)
}

/// Internal Responses state belongs only on a Responses-capable account hop.
pub fn strip_response_metadata(body: &mut Value) {
    if let Some(object) = body.as_object_mut() {
        object.remove(tools::RESPONSES_REQUEST);
    }
    for message in body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if let Some(object) = message.as_object_mut() {
            object.remove("_echobird_response_items");
        }
        for call in message
            .get_mut("tool_calls")
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(object) = call.as_object_mut() {
                object.remove("_echobird_response_item_id");
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProxyError {
    pub status: StatusCode,
    pub message: String,
    pub error_code: Option<String>,
    pub error_type: Option<String>,
    pub retry_after: Option<HeaderValue>,
    pub model_scope: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProxyFailureMetadata {
    pub model_scope: bool,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProxyError {}

pub(crate) fn require_text_provider(provider: AccountProvider) -> Result<(), ProxyError> {
    if matches!(
        provider,
        AccountProvider::QwenWork
            | AccountProvider::QwenWorkCn
            | AccountProvider::Cursor
            | AccountProvider::GrokBot
    ) {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "This account source only supports speech transcription",
        ));
    }
    Ok(())
}

impl ProxyError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            error_code: None,
            error_type: None,
            retry_after: None,
            model_scope: false,
        }
    }

    pub fn into_response(self) -> Response {
        let mut error = json!({"message":self.message,"type":self.error_type.as_deref().unwrap_or("upstream_error")});
        if let Some(code) = self.error_code {
            error["code"] = json!(code);
        }
        if let Some(retry_after) = self
            .retry_after
            .as_ref()
            .and_then(|value| value.to_str().ok())
        {
            error["retry_after"] = json!(retry_after);
        }
        let mut response = json_response(&json!({"error":error}));
        *response.status_mut() = self.status;
        if let Some(retry_after) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, retry_after);
        }
        response.extensions_mut().insert(ProxyFailureMetadata {
            model_scope: self.model_scope,
        });
        response
    }

    pub(crate) fn upstream(error: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            format!("Account upstream failed: {error}"),
        )
    }
}

pub async fn route(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    require_text_provider(provider)?;
    if let Some(application) = super::plugin_accounts::application(provider) {
        return super::plugin_accounts::proxy(
            application,
            credentials["accountId"].as_str().unwrap_or_default(),
            model,
            body,
        )
        .await;
    }
    if provider == AccountProvider::Devin {
        devin::route(credentials, model, body, client).await
    } else if provider == AccountProvider::CodeArts {
        codearts::route(credentials, model, body, client).await
    } else if provider == AccountProvider::Grok {
        grok::route(credentials, model, body, client).await
    } else if provider == AccountProvider::Kiro {
        kiro::route(credentials, model, body, client).await
    } else if provider == AccountProvider::Minimax {
        minimax::route(credentials, model, body, client).await
    } else if matches!(provider, AccountProvider::Qoder | AccountProvider::QoderCn) {
        qoder::route(provider, credentials, model, body, client).await
    } else if provider == AccountProvider::Zcode {
        zcode::route(credentials, model, body, client).await
    } else {
        providers::route(provider, credentials, model, body, client).await
    }
}

pub async fn route_responses(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    body: Value,
    client: &reqwest::Client,
) -> Result<Option<Response>, ProxyError> {
    require_text_provider(provider)?;
    if provider == AccountProvider::Grok {
        return grok::route_responses(credentials, model, body, client)
            .await
            .map(Some);
    }
    providers::route_responses(provider, credentials, model, body, client).await
}

pub async fn list_models(
    provider: AccountProvider,
    credentials: Value,
    client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    require_text_provider(provider)?;
    if let Some(application) = super::plugin_accounts::application(provider) {
        let value = super::plugin_accounts::call(
            application,
            "models",
            json!({"accountId":credentials["accountId"]}),
        )
        .await
        .map_err(super::account_model_sources::credential_error)?;
        return serde_json::from_value(value).map_err(ProxyError::upstream);
    }
    if provider == AccountProvider::Devin {
        devin::list_models(credentials, client).await
    } else if provider == AccountProvider::CodeArts {
        codearts::list_models(credentials, client).await
    } else if provider == AccountProvider::Grok {
        grok::list_models(credentials, client).await
    } else if provider == AccountProvider::Kiro {
        kiro::list_models(credentials, client).await
    } else if provider == AccountProvider::Cline {
        cline::list_models(credentials, client).await
    } else if provider == AccountProvider::Minimax {
        minimax::list_models(credentials, client).await
    } else if matches!(provider, AccountProvider::Qoder | AccountProvider::QoderCn) {
        qoder::list_models(provider, credentials, client).await
    } else if provider == AccountProvider::Zcode {
        zcode::list_models(credentials, client).await
    } else {
        providers::list_models(provider, credentials, client).await
    }
}

pub(crate) fn json_response(value: &Value) -> Response {
    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .expect("fixed response headers")
}

pub(crate) fn stream_response(body: Body) -> Response {
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .expect("fixed response headers")
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[tokio::test]
    async fn speech_only_sources_never_enter_text_gateways() {
        let client = reqwest::Client::new();
        for provider in [
            AccountProvider::QwenWork,
            AccountProvider::QwenWorkCn,
            AccountProvider::Cursor,
            AccountProvider::GrokBot,
        ] {
            assert_eq!(
                connection_base(provider, &Value::Null).unwrap_err().status,
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                list_models(provider, Value::Null, &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                route(provider, Value::Null, "model", Value::Null, &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::BAD_REQUEST
            );
            assert_eq!(
                route_responses(provider, Value::Null, "model", Value::Null, &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::BAD_REQUEST
            );
            assert!(crate::services::account_model_sources::list_models(
                &crate::models::model::AccountSource {
                    provider,
                    account_id: "not-saved".into(),
                }
            )
            .await
            .unwrap_err()
            .contains("only supports speech"));
        }
    }

    #[test]
    fn connection_targets_cover_all_providers_and_saved_regions_without_authentication() {
        use AccountProvider::*;
        for (provider, metadata, expected) in [
            (
                CodeArts,
                Value::Null,
                "https://snap-access.cn-north-4.myhuaweicloud.com",
            ),
            (Codex, Value::Null, "https://chatgpt.com/backend-api/codex"),
            (
                Antigravity,
                Value::Null,
                "https://daily-cloudcode-pa.googleapis.com",
            ),
            (
                Copilot,
                json!({"baseUrl":"https://api.business.githubcopilot.com"}),
                "https://api.business.githubcopilot.com",
            ),
            (Kilo, Value::Null, "https://api.kilo.ai/api/openrouter"),
            (
                Kimi,
                json!({"baseUrl":"https://api.kimi.ai/coding/v1"}),
                "https://api.kimi.ai/coding/v1",
            ),
            (
                Kimi,
                json!({"baseUrl":"https://api.kimi.com/coding/v1"}),
                "https://api.kimi.com/coding/v1",
            ),
            (
                OpenDesign,
                Value::Null,
                crate::services::opendesign_accounts::LINK,
            ),
            (Grok, Value::Null, "https://cli-chat-proxy.grok.com/v1"),
            (Cline, Value::Null, "https://api.cline.bot/api/v1"),
            (
                Kiro,
                json!({"region":"eu-west-1"}),
                "https://runtime.eu-west-1.kiro.dev",
            ),
            (Minimax, json!({"region":"cn"}), "https://agent.minimax.cn"),
            (
                Minimax,
                json!({"region":"global"}),
                "https://agent.minimax.io",
            ),
            (Qoder, Value::Null, "https://api3.qoder.sh"),
            (QoderCn, Value::Null, "https://gateway.qoder.com.cn"),
            (Zcode, Value::Null, "https://zcode.z.ai"),
        ] {
            assert_eq!(connection_base(provider, &metadata).unwrap(), expected);
        }
        assert!(connection_base(Kiro, &json!({"region":"invalid"})).is_err());
        assert!(connection_base(Minimax, &json!({"region":"invalid"})).is_err());
    }
}
