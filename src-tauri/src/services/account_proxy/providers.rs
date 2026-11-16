use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use futures_util::StreamExt;
use serde_json::{json, Value};
#[cfg(test)]
#[path = "providers/dumate_tests.rs"]
mod dumate_tests;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::models::model::AccountProvider;

#[cfg(test)]
use super::json_response;
use super::protocol::{self, ChatDecoder, SseParser, WireProtocol};
use super::{stream_response, ProxyError};

const CODEX_BASE: &str = "https://chatgpt.com/backend-api/codex";
const CODE_ASSIST_BASE: &str = "https://daily-cloudcode-pa.googleapis.com";
const METADATA_CACHE_LIMIT: usize = 64;
const PROJECT_LIFETIME: Duration = Duration::from_secs(30);
const COPILOT_CATALOG_LIFETIME: Duration = Duration::from_secs(300);

#[cfg(test)]
mod kimi_tests;
#[cfg(test)]
mod opendesign_tests;

#[derive(Clone)]
enum PreparedMetadata {
    Project((String, Option<String>)),
    Catalog(Value),
}

#[derive(Default)]
struct MetadataCell {
    generation: AtomicU64,
    value: tokio::sync::Mutex<Option<(Instant, u64, PreparedMetadata)>>,
}

struct MetadataEntry {
    provider: AccountProvider,
    account_id: Option<String>,
    created: Instant,
    cell: Arc<MetadataCell>,
}

fn metadata_cells() -> &'static Mutex<BTreeMap<String, MetadataEntry>> {
    static CACHE: OnceLock<Mutex<BTreeMap<String, MetadataEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn project_load_base(credentials: &Value) -> &str {
    credentials["loadBaseUrl"]
        .as_str()
        .unwrap_or_else(|| {
            if credentials["baseUrl"].is_string() {
                base(AccountProvider::Antigravity, credentials)
            } else {
                "https://cloudcode-pa.googleapis.com"
            }
        })
        .trim_end_matches('/')
}

fn metadata_key(provider: AccountProvider, credentials: &Value) -> String {
    let context = json!([
        provider,
        base(provider, credentials),
        if provider == AccountProvider::Antigravity {
            project_load_base(credentials)
        } else {
            ""
        },
        credentials
    ]);
    format!("{:x}", Sha256::digest(context.to_string()))
}

fn metadata_cell(provider: AccountProvider, credentials: &Value) -> Arc<MetadataCell> {
    let mut cells = metadata_cells()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    metadata_cell_at(&mut cells, provider, credentials)
}

fn metadata_cell_at(
    cells: &mut BTreeMap<String, MetadataEntry>,
    provider: AccountProvider,
    credentials: &Value,
) -> Arc<MetadataCell> {
    let key = metadata_key(provider, credentials);
    if let Some(entry) = cells.get(&key) {
        return entry.cell.clone();
    }
    if cells.len() >= METADATA_CACHE_LIMIT {
        let oldest = cells
            .iter()
            .filter(|(_, entry)| Arc::strong_count(&entry.cell) == 1)
            .min_by_key(|(_, entry)| entry.created)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            cells.remove(&oldest);
        } else {
            return Arc::new(MetadataCell::default());
        }
    }
    let cell = Arc::new(MetadataCell::default());
    cells.insert(
        key,
        MetadataEntry {
            provider,
            account_id: credentials["_echobird_saved_account_id"]
                .as_str()
                .map(str::to_owned),
            created: Instant::now(),
            cell: cell.clone(),
        },
    );
    cell
}

pub(super) fn forget_metadata(provider: AccountProvider, account_id: &str) {
    let mut cells = metadata_cells()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    cells.retain(|_, entry| {
        if entry.provider == provider && entry.account_id.as_deref() == Some(account_id) {
            entry.cell.generation.fetch_add(1, Ordering::SeqCst);
            false
        } else {
            true
        }
    });
}

fn forget_metadata_credentials(provider: AccountProvider, credentials: &Value) {
    if let Some(entry) = metadata_cells()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .remove(&metadata_key(provider, credentials))
    {
        entry.cell.generation.fetch_add(1, Ordering::SeqCst);
    }
}

fn forget_failed_metadata(provider: AccountProvider, credentials: &Value, error: &ProxyError) {
    if (matches!(
        provider,
        AccountProvider::Antigravity | AccountProvider::Copilot
    ) && matches!(
        error.status,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    )) || (provider == AccountProvider::Copilot
        && matches!(
            error.error_code.as_deref(),
            Some("model_not_found" | "model_not_supported")
        ))
    {
        forget_metadata_credentials(provider, credentials);
    }
}

async fn prepare_metadata<F, Fut>(
    provider: AccountProvider,
    credentials: &Value,
    refresh: bool,
    fetch: F,
) -> Result<PreparedMetadata, ProxyError>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<PreparedMetadata, ProxyError>>,
{
    access_token(credentials)?;
    let cell = metadata_cell(provider, credentials);
    if refresh {
        cell.generation.fetch_add(1, Ordering::SeqCst);
    }
    let mut cached = cell.value.lock().await;
    let generation = cell.generation.load(Ordering::SeqCst);
    let lifetime = if provider == AccountProvider::Antigravity {
        PROJECT_LIFETIME
    } else {
        COPILOT_CATALOG_LIFETIME
    };
    if !refresh {
        if let Some((_, _, value)) = cached.as_ref().filter(|(at, saved_generation, _)| {
            at.elapsed() < lifetime && *saved_generation == generation
        }) {
            return Ok(value.clone());
        }
    }
    *cached = None;
    let value = fetch().await?;
    if cell.generation.load(Ordering::SeqCst) == generation {
        *cached = Some((Instant::now(), generation, value.clone()));
    }
    Ok(value)
}

type AccountFreeModels = BTreeMap<String, Vec<String>>;

fn free_models() -> &'static Mutex<AccountFreeModels> {
    static CACHE: OnceLock<Mutex<AccountFreeModels>> = OnceLock::new();
    CACHE.get_or_init(|| {
        #[cfg(not(test))]
        let saved = crate::services::cursor_auth::read(&free_models_path()).unwrap_or_default();
        #[cfg(test)]
        let saved = AccountFreeModels::default();
        Mutex::new(saved)
    })
}

#[cfg(not(test))]
fn free_models_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird/cache/account-free-models.json")
}

fn free_model_cache_key(provider: AccountProvider, account_id: &str) -> String {
    json!([provider, account_id]).to_string()
}

pub(super) fn cached_free_model_ids(provider: AccountProvider, account_id: &str) -> Vec<String> {
    free_models()
        .lock()
        .ok()
        .and_then(|cache| {
            cache
                .get(&free_model_cache_key(provider, account_id))
                .cloned()
        })
        .unwrap_or_default()
}

fn workbuddy_free_model_ids(value: &Value, available: &[String]) -> Vec<String> {
    value["data"]["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            if !available.iter().any(|model| model == id) {
                return None;
            }
            let credits = row["credits"].as_f64().or_else(|| {
                let text = row["credits"].as_str()?.trim();
                text.strip_prefix('x')
                    .or_else(|| text.strip_prefix('X'))
                    .unwrap_or(text)
                    .parse::<f64>()
                    .ok()
            })?;
            (credits.is_finite() && credits >= 0.0 && credits == 0.0).then(|| id.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn remember_free_model_ids(
    provider: AccountProvider,
    credentials: &Value,
    free_ids: Vec<String>,
) {
    let Some(account_id) = credentials["_echobird_saved_account_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| {
            if matches!(
                provider,
                AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi
            ) {
                credentials["accountId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
            } else {
                None
            }
        })
    else {
        return;
    };
    let Ok(mut cache) = free_models().lock() else {
        return;
    };
    cache.insert(free_model_cache_key(provider, account_id), free_ids);
    #[cfg(not(test))]
    if crate::services::cursor_auth::write(&free_models_path(), &*cache).is_err() {
        log::warn!("[AccountProxy] Could not persist free model metadata");
    }
}

fn access_token(credentials: &Value) -> Result<&str, ProxyError> {
    credentials["accessToken"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ProxyError::new(StatusCode::UNAUTHORIZED, "Account has no access token"))
}

pub(super) fn base(provider: AccountProvider, credentials: &Value) -> &str {
    if provider == AccountProvider::Cline {
        #[cfg(test)]
        if let Some(base) = credentials["baseUrl"].as_str() {
            return base.trim_end_matches('/');
        }
        return "https://api.cline.bot/api/v1";
    }
    credentials["baseUrl"]
        .as_str()
        .unwrap_or(match provider {
            AccountProvider::CommandCode => super::super::commandcode_accounts::BASE,
            AccountProvider::Dumate => "https://dumate-svc.baidu.com/gateway/apis/v1",
            AccountProvider::Codex => CODEX_BASE,
            AccountProvider::Kimi => "https://api.kimi.com/coding/v1",
            AccountProvider::WorkBuddy => "https://copilot.tencent.com",
            AccountProvider::WorkBuddyAi => "https://www.workbuddy.ai",
            AccountProvider::Antigravity => CODE_ASSIST_BASE,
            AccountProvider::Copilot => "https://api.githubcopilot.com",
            AccountProvider::Kilo => "https://api.kilo.ai/api/openrouter",
            AccountProvider::Cursor | AccountProvider::GrokBot => "https://api2.cursor.sh",
            AccountProvider::QwenWork => "https://gateway.qwenwork.ai",
            AccountProvider::QwenWorkCn => "https://gateway.qwenwork.cn",
            AccountProvider::OpenDesign => super::super::opendesign_accounts::LINK,
            AccountProvider::ClaudeCode => unreachable!("Claude uses its CLI"),
            AccountProvider::Trae | AccountProvider::Traecn => {
                unreachable!("Trae uses its own gateway")
            }
            AccountProvider::Grok
            | AccountProvider::DoubaoWork
            | AccountProvider::Dola
            | AccountProvider::Cline
            | AccountProvider::Kiro
            | AccountProvider::Minimax
            | AccountProvider::Qoder
            | AccountProvider::QoderCn
            | AccountProvider::Zcode
            | AccountProvider::CodeArts
            | AccountProvider::Devin => {
                unreachable!("Provider uses its account gateway")
            }
        })
        .trim_end_matches('/')
}

fn signed(
    request: reqwest::RequestBuilder,
    provider: AccountProvider,
    credentials: &Value,
) -> Result<reqwest::RequestBuilder, ProxyError> {
    let mut request = request.bearer_auth(access_token(credentials)?);
    match provider {
        AccountProvider::CommandCode => {
            request = request.header("x-api-key", access_token(credentials)?);
        }
        AccountProvider::QwenWork
        | AccountProvider::QwenWorkCn
        | AccountProvider::Cursor
        | AccountProvider::GrokBot => {
            super::require_text_provider(provider)?;
        }
        AccountProvider::Dumate => {
            request = request
                .header("X-Dumate-Client-Type", "web")
                .header("X-Dumate-Account-Id", "global");
        }
        AccountProvider::Codex => {
            request = request
                .header("originator", "codex_cli_rs")
                .header("version", "0.159.0")
                .header("user-agent", "codex_cli_rs/0.159.0 (EchoBird)")
                .header("OpenAI-Beta", "responses=experimental");
            if let Some(account) = credentials["accountId"].as_str() {
                request = request.header("chatgpt-account-id", account);
            }
        }
        AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi => {
            let uid = credentials["uid"].as_str().ok_or_else(|| {
                ProxyError::new(StatusCode::UNAUTHORIZED, "WorkBuddy account has no user id")
            })?;
            request = request
                .header("X-User-Id", uid)
                .header("X-Product", "SaaS")
                .header("X-IDE-Type", "WorkBuddy")
                .header("user-agent", "WorkBuddy/5.5.6")
                .header("X-Requested-With", "XMLHttpRequest");
            if let Some(domain) = credentials["domain"].as_str() {
                request = request.header("X-Domain", domain);
            }
            if let Some(enterprise) = credentials["enterpriseId"].as_str() {
                request = request.header("X-Enterprise-Id", enterprise);
            }
            if provider == AccountProvider::WorkBuddyAi {
                request = request
                    .header("X-Requested-With", "XMLHttpRequest")
                    .header("X-Agent-Intent", "craft")
                    .header("X-Agent-Type", "main")
                    .header("X-IDE-Name", "WorkBuddy")
                    .header("X-IDE-Version", "5.5.6");
                let conversation = uuid::Uuid::new_v4().to_string();
                request = request
                    .header("X-Conversation-ID", &conversation)
                    .header("X-Conversation-Request-ID", &conversation);
                let message = uuid::Uuid::new_v4().to_string();
                request = request
                    .header("X-Conversation-Message-ID", &message)
                    .header("X-Request-ID", &message);
            }
        }
        AccountProvider::Antigravity => {
            request = request
                .header(
                    "user-agent",
                    format!(
                        "antigravity/hub/3.1.4 {} /{}",
                        std::env::consts::OS,
                        std::env::consts::ARCH
                    ),
                )
                .header("X-Goog-Api-Client", "gl-node/22.21.1");
        }
        AccountProvider::Copilot => {
            request = request
                .header("Copilot-Integration-Id", "copilot-developer-cli")
                .header("X-GitHub-Api-Version", "2026-07-01")
                .header("Editor-Version", "copilot/0.0.1")
                .header("X-Initiator", "user")
                .header("X-Request-Id", uuid::Uuid::new_v4().to_string());
        }
        AccountProvider::Kilo => {
            request = request
                .header("HTTP-Referer", "https://kilo.ai")
                .header("X-Title", "EchoBird");
        }
        AccountProvider::OpenDesign | AccountProvider::Kimi => {}
        AccountProvider::Cline => {
            request = request
                .header("HTTP-Referer", "https://cline.bot")
                .header("X-Title", "EchoBird")
                .header(
                    "user-agent",
                    concat!("EchoBird/", env!("CARGO_PKG_VERSION")),
                );
        }
        AccountProvider::ClaudeCode => unreachable!("Claude uses its CLI"),
        AccountProvider::Trae | AccountProvider::Traecn => {
            unreachable!("Trae uses its own gateway")
        }
        AccountProvider::Grok
        | AccountProvider::DoubaoWork
        | AccountProvider::Dola
        | AccountProvider::Kiro
        | AccountProvider::Minimax
        | AccountProvider::Qoder
        | AccountProvider::QoderCn
        | AccountProvider::Zcode
        | AccountProvider::CodeArts
        | AccountProvider::Devin => {
            unreachable!("Provider uses its account gateway")
        }
    }
    if let Some(headers) = credentials["headers"].as_object() {
        let mut overrides = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                let name =
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                        ProxyError::new(StatusCode::BAD_REQUEST, "Invalid account request header")
                    })?;
                let value = reqwest::header::HeaderValue::from_str(value).map_err(|_| {
                    ProxyError::new(StatusCode::BAD_REQUEST, "Invalid account request header")
                })?;
                overrides.insert(name, value);
            }
        }
        request = request.headers(overrides);
    }
    Ok(request)
}

async fn json_upstream(request: reqwest::RequestBuilder) -> Result<Value, ProxyError> {
    let response = request.send().await.map_err(ProxyError::upstream)?;
    let status = response.status();
    let headers = response.headers().clone();
    let value = response.json::<Value>().await;
    if !status.is_success() {
        return Err(upstream_error(
            status,
            &headers,
            &value.unwrap_or(Value::Null),
        ));
    }
    value.map_err(ProxyError::upstream)
}

// Upstream error classification lives in the shared `echo-gateway` crate.
use echo_gateway::failure::safe_error;
pub(super) use echo_gateway::failure::upstream_error;
pub(crate) use echo_gateway::failure::{context_limit_error, stream_error};

#[cfg(test)]
mod context_limit_regressions {
    use super::*;

    #[test]
    fn numeric_quota_codes_and_nested_messages_survive_proxy_and_stream_conversion() {
        let raw = json!({"code":6004,"msg":"于 2026-10-22 09:00:00 UTC+8 可用"});
        for value in [raw.clone(), json!({"error":raw})] {
            let error = upstream_error(StatusCode::BAD_REQUEST, &HeaderMap::new(), &value);
            assert_eq!(error.error_code.as_deref(), Some("6004"));
            assert!(error.message.contains("UTC+8"));
            let stream = stream_error("error", &value.to_string(), &HeaderMap::new()).unwrap();
            assert_eq!(stream.error_code.as_deref(), Some("6004"));
            assert!(stream.message.contains("UTC+8"));
        }
        for code in ["request_incompatible", "unsupported_parameter"] {
            let error = stream_error(
                "error",
                &json!({"error":{"code":code,"message":"Candidate cannot serve this request"}})
                    .to_string(),
                &HeaderMap::new(),
            )
            .unwrap();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.error_code.as_deref(), Some(code));
        }
    }

    #[test]
    fn context_limit_stream_failures_are_client_errors_even_with_a_rate_status() {
        for code in [
            "context_length_exceeded",
            "context_window_exceeded",
            "prompt_too_long",
        ] {
            let detail =
                json!({"code":code,"message":"Request does not fit the model","status":429});
            for event in ["", "response.failed", "response.completed"] {
                let value = if event.is_empty() {
                    json!({"error":detail})
                } else {
                    json!({"type":event,"response":{"error":detail}})
                };
                let error = stream_error(event, &value.to_string(), &HeaderMap::new()).unwrap();
                assert_eq!(error.status, StatusCode::BAD_REQUEST);
                assert_eq!(error.error_code.as_deref(), Some(code));
            }
        }
        let error = stream_error("error", &json!({"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 201000 tokens > 200000 maximum"}}).to_string(), &HeaderMap::new()).unwrap();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.error_type.as_deref(), Some("invalid_request_error"));
    }

    #[test]
    fn context_limit_detection_preserves_quota_and_unknown_failures() {
        for (code, expected) in [
            ("rate_limit_exceeded", StatusCode::TOO_MANY_REQUESTS),
            ("insufficient_quota", StatusCode::PAYMENT_REQUIRED),
            ("unknown_upstream_error", StatusCode::BAD_GATEWAY),
        ] {
            let error = stream_error(
                "error",
                &json!({"error":{"code":code,"message":"Too many requests in the quota window"}})
                    .to_string(),
                &HeaderMap::new(),
            )
            .unwrap();
            assert_eq!(error.status, expected);
            assert!(!context_limit_error(Some(code), &error.message));
        }
    }
}

async fn project(
    credentials: &Value,
    client: &reqwest::Client,
) -> Result<(String, Option<String>), ProxyError> {
    prepared_project(credentials, client, false).await
}

async fn prepared_project(
    credentials: &Value,
    client: &reqwest::Client,
    refresh: bool,
) -> Result<(String, Option<String>), ProxyError> {
    let value = prepare_metadata(
        AccountProvider::Antigravity,
        credentials,
        refresh,
        || async {
            fetch_project(credentials, client, true)
                .await
                .map(PreparedMetadata::Project)
        },
    )
    .await?;
    let PreparedMetadata::Project(value) = value else {
        unreachable!("project cache uses project values")
    };
    Ok(value)
}

pub(super) async fn fetch_project(
    credentials: &Value,
    client: &reqwest::Client,
    require_project: bool,
) -> Result<(String, Option<String>), ProxyError> {
    let load_base = project_load_base(credentials);
    let request = client
        .post(format!(
            "{}/v1internal:loadCodeAssist",
            load_base.trim_end_matches('/')
        ))
        .json(&json!({"metadata":{"ideType":"ANTIGRAVITY"}}));
    let value = json_upstream(signed(request, AccountProvider::Antigravity, credentials)?).await?;
    if !value.is_object() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Account API returned an invalid project response",
        ));
    }
    let id = credentials["projectId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .or_else(|| value["cloudaicompanionProject"].as_str())
        .or_else(|| value["cloudaicompanionProject"]["id"].as_str());
    let id = id.filter(|id| !id.trim().is_empty()).unwrap_or_default();
    if id.is_empty() && require_project {
        let reason = value["ineligibleTiers"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|v| {
                v["reasonMessage"]
                    .as_str()
                    .or_else(|| v["validationErrorMessage"].as_str())
            })
            .unwrap_or("Antigravity has not set up this account; sign in to its app first");
        return Err(ProxyError::new(StatusCode::FORBIDDEN, reason));
    }
    let tier = value["paidTier"]["id"]
        .as_str()
        .filter(|tier| !matches!(*tier, "" | "free-tier" | "legacy-tier"))
        .or_else(|| value["currentTier"]["id"].as_str())
        .filter(|tier| !matches!(*tier, "" | "free-tier" | "legacy-tier"))
        .map(str::to_string);
    Ok((id.into(), tier))
}

async fn kimi_catalog(
    credentials: &Value,
    client: &reqwest::Client,
    refresh: bool,
) -> Result<Value, ProxyError> {
    let value = prepare_metadata(AccountProvider::Kimi, credentials, refresh, || async {
        let value = json_upstream(signed(
            client.get(format!(
                "{}/models",
                base(AccountProvider::Kimi, credentials)
            )),
            AccountProvider::Kimi,
            credentials,
        )?)
        .await?;
        if !value["data"].is_array() {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Kimi returned an invalid model directory",
            ));
        }
        Ok(PreparedMetadata::Catalog(value))
    })
    .await?;
    let PreparedMetadata::Catalog(value) = value else {
        unreachable!()
    };
    Ok(value)
}

async fn copilot_catalog(
    credentials: &Value,
    client: &reqwest::Client,
) -> Result<Value, ProxyError> {
    prepared_copilot_catalog(credentials, client, false).await
}

async fn prepared_copilot_catalog(
    credentials: &Value,
    client: &reqwest::Client,
    refresh: bool,
) -> Result<Value, ProxyError> {
    let value = prepare_metadata(AccountProvider::Copilot, credentials, refresh, || async {
        let value = json_upstream(signed(
            client.get(format!(
                "{}/models",
                base(AccountProvider::Copilot, credentials)
            )),
            AccountProvider::Copilot,
            credentials,
        )?)
        .await?;
        if !value["data"].as_array().is_some_and(|rows| {
            rows.is_empty()
                || rows
                    .iter()
                    .any(|row| row["id"].as_str().is_some_and(|id| !id.trim().is_empty()))
        }) {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Account API returned an invalid model directory",
            ));
        }
        Ok(PreparedMetadata::Catalog(value))
    })
    .await?;
    let PreparedMetadata::Catalog(value) = value else {
        unreachable!("catalog cache uses catalog values")
    };
    Ok(value)
}

pub(super) async fn list_models(
    provider: AccountProvider,
    credentials: Value,
    client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let result = fetch_models(provider, &credentials, client).await;
    if result.is_err()
        && matches!(
            provider,
            AccountProvider::Antigravity | AccountProvider::Copilot
        )
    {
        forget_metadata_credentials(provider, &credentials);
    }
    result
}

async fn fetch_models(
    provider: AccountProvider,
    credentials: &Value,
    client: &reqwest::Client,
) -> Result<Vec<String>, ProxyError> {
    let api = base(provider, credentials);
    let value = match provider {
        AccountProvider::QwenWork
        | AccountProvider::QwenWorkCn
        | AccountProvider::Cursor
        | AccountProvider::GrokBot => {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "This account source only supports speech transcription",
            ));
        }
        // The gateway has no public /models endpoint. This is its verified
        // native text route, not an alias for a claimed underlying model.
        AccountProvider::Dumate => return Ok(vec!["model-text".into()]),
        AccountProvider::Antigravity => {
            let (id, tier) = prepared_project(credentials, client, true).await?;
            let mut body = json!({"project":id});
            if let Some(tier) = tier {
                body["entitlement"] = json!({"userTier":tier});
            }
            json_upstream(signed(
                client
                    .post(format!("{api}/v1internal:fetchAvailableModels"))
                    .json(&body),
                provider,
                credentials,
            )?)
            .await?
        }
        AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi => {
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::USER_AGENT,
                reqwest::header::HeaderValue::from_static("CLI/2.0.0 WorkBuddy/5.5.6"),
            );
            headers.insert(
                "x-requested-with",
                reqwest::header::HeaderValue::from_static("XMLHttpRequest"),
            );
            headers.insert(
                reqwest::header::ACCEPT,
                reqwest::header::HeaderValue::from_static("application/json"),
            );
            let value = json_upstream(
                signed(
                    client.get(format!("{api}/v3/config")),
                    provider,
                    credentials,
                )?
                .headers(headers),
            )
            .await?;
            if value["code"].as_i64().is_some_and(|c| c != 0) {
                return Err(ProxyError::new(StatusCode::BAD_GATEWAY, safe_error(&value)));
            }
            let models: Vec<String> = value["data"]["agents"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| a["name"] == "cli")
                .flat_map(|a| a["models"].as_array().into_iter().flatten())
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect();
            let models = nonempty(models)?;
            remember_free_model_ids(
                provider,
                credentials,
                workbuddy_free_model_ids(&value, &models),
            );
            return Ok(models);
        }
        AccountProvider::Copilot => prepared_copilot_catalog(credentials, client, true).await?,
        AccountProvider::Kimi => kimi_catalog(credentials, client, true).await?,
        AccountProvider::Codex => {
            json_upstream(signed(
                client.get(format!("{api}/models?client_version=0.159.0")),
                provider,
                credentials,
            )?)
            .await?
        }
        AccountProvider::CommandCode | AccountProvider::Kilo | AccountProvider::OpenDesign => {
            json_upstream(signed(
                client.get(format!("{api}/models")),
                provider,
                credentials,
            )?)
            .await?
        }
        AccountProvider::ClaudeCode => unreachable!("Claude uses its CLI"),
        AccountProvider::Trae | AccountProvider::Traecn => {
            unreachable!("Trae uses its own gateway")
        }
        AccountProvider::Grok
        | AccountProvider::DoubaoWork
        | AccountProvider::Dola
        | AccountProvider::Cline
        | AccountProvider::Kiro
        | AccountProvider::Minimax
        | AccountProvider::Qoder
        | AccountProvider::QoderCn
        | AccountProvider::Zcode
        | AccountProvider::CodeArts
        | AccountProvider::Devin => {
            unreachable!("Provider uses its account gateway")
        }
    };
    let ids = if provider == AccountProvider::Antigravity {
        value["models"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(id, m)| {
                !m["disabled"].as_bool().unwrap_or(false)
                    && !id.starts_with("chat_")
                    && !id.starts_with("tab_")
                    && !id.contains("image")
            })
            .map(|(id, _)| id.clone())
            .collect()
    } else {
        let rows = value["data"]
            .as_array()
            .or_else(|| value["models"].as_array());
        rows.into_iter()
            .flatten()
            .filter(|row| {
                row["policy"]["state"] != "disabled"
                    && (provider == AccountProvider::Copilot
                        || row["model_picker_enabled"] != false)
                    && row["visibility"] != "hide"
                    && row["supported_in_api"] != false
                    && (provider != AccountProvider::Copilot || copilot_endpoint(row).is_some())
            })
            .filter_map(|row| row["id"].as_str().or_else(|| row["slug"].as_str()))
            .map(str::to_string)
            .collect()
    };
    let ids = nonempty(ids)?;
    if provider == AccountProvider::Kilo {
        remember_free_model_ids(provider, credentials, kilo_free_model_ids(&value, &ids));
    }
    Ok(ids)
}

fn kilo_free_model_ids(value: &Value, available: &[String]) -> Vec<String> {
    value["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let id = row["id"].as_str()?;
            if !available.iter().any(|model| model == id) {
                return None;
            }
            let free = match row.get("isFree") {
                Some(value) => value.as_bool() == Some(true),
                None => {
                    kilo_zero_price(&row["pricing"]["prompt"])
                        && kilo_zero_price(&row["pricing"]["completion"])
                        && row["pricing"]
                            .as_object()
                            .is_some_and(|pricing| pricing.values().all(kilo_zero_price))
                }
            };
            free.then(|| id.to_string())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn kilo_zero_price(value: &Value) -> bool {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .is_some_and(|price| price.is_finite() && price == 0.0)
}

fn nonempty(ids: Vec<String>) -> Result<Vec<String>, ProxyError> {
    let ids: Vec<_> = ids
        .into_iter()
        .filter(|id| !id.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "The account API returned no available chat models",
        ))
    } else {
        Ok(ids)
    }
}

fn copilot_endpoint(row: &Value) -> Option<(WireProtocol, &'static str)> {
    if row.get("supported_endpoints").is_none() && row["capabilities"]["type"] == "chat" {
        return Some((WireProtocol::Chat, "/chat/completions"));
    }
    let endpoints = row["supported_endpoints"].as_array()?;
    for (path, protocol) in [
        ("/chat/completions", WireProtocol::Chat),
        ("/responses", WireProtocol::Responses),
        ("/v1/messages", WireProtocol::Anthropic),
        ("/messages", WireProtocol::Anthropic),
    ] {
        if endpoints.iter().any(|endpoint| endpoint == path) {
            return Some((protocol, path));
        }
    }
    None
}

async fn wire(
    provider: AccountProvider,
    credentials: &Value,
    model: &str,
    client: &reqwest::Client,
) -> Result<(WireProtocol, &'static str), ProxyError> {
    if provider == AccountProvider::Kimi {
        let catalog = kimi_catalog(credentials, client, false).await?;
        let row = catalog["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["id"] == model)
            .ok_or_else(|| {
                ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Kimi model is not available for this account",
                )
            })?;
        return match row["protocol"].as_str() {
            Some("anthropic") => Ok((WireProtocol::Anthropic, "/messages?beta=true")),
            None | Some("openai") => Ok((WireProtocol::Chat, "/chat/completions")),
            _ => Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Kimi returned an unsupported model protocol",
            )),
        };
    }
    if provider == AccountProvider::OpenDesign {
        return Ok(match model {
            "claude-haiku-5.5" | "deepseek-v4-pro" => (WireProtocol::Responses, "/responses"),
            "glm-5.3-flash" => (WireProtocol::Anthropic, "/messages"),
            _ => (WireProtocol::Chat, "/chat/completions"),
        });
    }
    if provider == AccountProvider::Codex {
        return Ok((WireProtocol::Responses, "/responses"));
    }
    if provider == AccountProvider::Antigravity {
        return Ok((
            WireProtocol::CodeAssist,
            "/v1internal:streamGenerateContent?alt=sse",
        ));
    }
    if provider == AccountProvider::WorkBuddy && model.to_ascii_lowercase().starts_with("claude-") {
        return Ok((WireProtocol::Anthropic, "/v1/messages"));
    }
    if provider != AccountProvider::Copilot {
        return Ok((
            WireProtocol::Chat,
            if matches!(
                provider,
                AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi
            ) {
                "/v2/chat/completions"
            } else {
                "/chat/completions"
            },
        ));
    }
    let configured = credentials["modelProtocols"][model]
        .as_str()
        .or_else(|| credentials["protocol"].as_str());
    if let Some(protocol) = configured {
        return Ok(match protocol {
            "responses" => (WireProtocol::Responses, "/responses"),
            "messages" => (WireProtocol::Anthropic, "/v1/messages"),
            _ => (WireProtocol::Chat, "/chat/completions"),
        });
    }
    let catalog = copilot_catalog(credentials, client).await?;
    let row = catalog["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|v| v["id"] == model)
        .filter(|row| {
            row["policy"]["state"] != "disabled"
                && row["visibility"] != "hide"
                && row["supported_in_api"] != false
        })
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::NOT_FOUND,
                "Copilot does not offer this model to the account",
            )
        })?;
    copilot_endpoint(row).ok_or_else(|| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Copilot model has no supported inference endpoint",
        )
    })
}

pub(super) fn anthropic_request(body: &Value, model: &str) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    let mut system = Vec::new();
    for message in super::responses::repaired_chat_messages(body) {
        let role = message["role"].as_str().unwrap_or("user");
        if matches!(role, "system" | "developer") {
            system.push(protocol::text(&message["content"]));
            continue;
        }
        let content = if role == "tool" {
            let mut result = json!({"type":"tool_result","tool_use_id":message["tool_call_id"],"content":message["content"]});
            if message["is_error"] == true {
                result["is_error"] = json!(true);
            }
            if let Some(previous) = messages.last_mut().filter(|previous| {
                previous["role"] == "user"
                    && previous["content"].as_array().is_some_and(|blocks| {
                        !blocks.is_empty()
                            && blocks.iter().all(|block| block["type"] == "tool_result")
                    })
            }) {
                previous["content"].as_array_mut().unwrap().push(result);
                continue;
            }
            json!([result])
        } else {
            let value = crate::services::local_llm::proxy::openai_to_anthropic(
                &json!({"choices":[{"message":message}]}),
            );
            let mut blocks = value["content"].as_array().cloned().unwrap_or_default();
            if role != "assistant" && message["content"].is_array() {
                blocks=message["content"].as_array().into_iter().flatten().filter_map(|part| {
                    if part["type"]=="text" { Some(json!({"type":"text","text":part["text"]})) }
                    else if part["type"]=="image_url" { let url=part["image_url"]["url"].as_str()?;
                        Some(if let Some(data)=url.strip_prefix("data:") { let (mime,bytes)=data.split_once(";base64,")?;
                            json!({"type":"image","source":{"type":"base64","media_type":mime,"data":bytes}})
                        } else { json!({"type":"image","source":{"type":"url","url":url}}) })
                    } else { None }
                }).collect();
            }
            json!(blocks)
        };
        messages.push(
            json!({"role":if role=="assistant" { "assistant" } else { "user" },"content":content}),
        );
    }
    let tools:Vec<_>=body["tools"].as_array().into_iter().flatten().map(|tool| {
        let function=&tool["function"];
        let mut tool=json!({"name":function["name"],"input_schema":function.get("parameters").cloned().unwrap_or(json!({"type":"object","properties":{}}))});
        if let Some(description)=function.get("description").filter(|value| value.is_string()) { tool["description"]=description.clone(); }
        tool
    }).collect();
    let mut request = json!({"model":model,"system":system.join("\n\n"),"messages":messages,"tools":tools,
        "max_tokens":body["max_tokens"].as_u64().unwrap_or(8192),"stream":true});
    let mut choice = body.get("tool_choice").map(|choice| match choice.as_str() {
        Some("required") => json!({"type":"any"}),
        Some("none") => json!({"type":"none"}),
        _ if choice.is_object() => json!({"type":"tool","name":choice["function"]["name"]}),
        _ => json!({"type":"auto"}),
    });
    if body["parallel_tool_calls"] == false {
        let choice = choice.get_or_insert(json!({"type":"auto"}));
        if choice["type"] != "none" {
            choice["disable_parallel_tool_use"] = json!(true);
        }
    }
    if let Some(choice) = choice {
        request["tool_choice"] = choice;
    }
    request
}

fn workbuddy_tool_choice(body: &mut Value) -> Result<(), ProxyError> {
    let Some(choice) = body.get("tool_choice").filter(|choice| choice.is_object()) else {
        return Ok(());
    };
    let name = choice["function"]["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .filter(|_| choice["type"] == "function")
        .ok_or_else(|| ProxyError::new(StatusCode::BAD_REQUEST, "Invalid named tool choice"))?;
    let tools: Vec<_> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|tool| tool["type"] == "function" && tool["function"]["name"] == name)
        .cloned()
        .collect();
    if tools.is_empty() {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Named tool is not declared",
        ));
    }
    // This gateway decodes tool_choice as a string. A name string acts like auto;
    // required with only that declaration preserves the named-choice contract.
    body["tools"] = json!(tools);
    body["tool_choice"] = json!("required");
    Ok(())
}

pub(super) async fn route(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    mut body: Value,
    client: &reqwest::Client,
) -> Result<Response, ProxyError> {
    let streaming = body["stream"].as_bool().unwrap_or(false);
    if matches!(
        provider,
        AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi
    ) {
        workbuddy_tool_choice(&mut body)?;
        if let Some(tools) = body.get_mut("tools").and_then(Value::as_array_mut) {
            for tool in tools {
                if let Some(parameters) = tool
                    .get_mut("function")
                    .and_then(|function| function.get_mut("parameters"))
                {
                    super::tools::type_object_unions(parameters);
                }
            }
        }
    }
    let (protocol, path) = wire(provider, &credentials, model, client).await?;
    if matches!(protocol, WireProtocol::CodeAssist | WireProtocol::Anthropic) {
        protocol::reject_structured_output(&body)?;
    }
    let api = base(provider, &credentials);
    let mut request = match protocol {
        WireProtocol::Trae => unreachable!("Trae uses its own gateway"),
        WireProtocol::Responses => protocol::chat_to_responses(&body, model),
        WireProtocol::CodeAssist => {
            let (project, _) = project(&credentials, client).await?;
            protocol::chat_to_code_assist(&body, model, &project)
        }
        WireProtocol::Anthropic => {
            let mut request = anthropic_request(&body, model);
            if matches!(provider, AccountProvider::Kimi | AccountProvider::WorkBuddy) {
                if let Some(effort) = body["reasoning_effort"].as_str() {
                    if effort == "none" {
                        request["thinking"] = json!({"type":"disabled"});
                    } else {
                        request["thinking"] = json!({"type":"adaptive"});
                        request["output_config"] = json!({"effort":effort});
                    }
                }
            }
            if provider == AccountProvider::WorkBuddy {
                if let Some(cap) = body
                    .get("max_completion_tokens")
                    .filter(|cap| !cap.is_null())
                {
                    request["max_tokens"] = cap.clone();
                }
                for key in ["temperature", "top_p"] {
                    if let Some(value) = body.get(key) {
                        request[key] = value.clone();
                    }
                }
                if let Some(stop) = body.get("stop").filter(|value| !value.is_null()) {
                    request["stop_sequences"] = if stop.is_string() {
                        json!([stop])
                    } else {
                        stop.clone()
                    };
                }
            }
            request
        }
        WireProtocol::Chat => {
            super::strip_response_metadata(&mut body);
            body["model"] = json!(model);
            body["stream"] = json!(true);
            if matches!(
                provider,
                AccountProvider::WorkBuddy | AccountProvider::WorkBuddyAi
            ) {
                if let Some(messages) = body["messages"].as_array_mut() {
                    if messages
                        .first()
                        .is_some_and(|message| message["role"] != "system")
                    {
                        messages.insert(
                            0,
                            json!({"role":"system","content":"You are a helpful assistant."}),
                        );
                    }
                }
            }
            body
        }
    };
    if provider == AccountProvider::Codex {
        if let Some(request) = request.as_object_mut() {
            request.remove("prompt_cache_options");
        }
    }
    let request = signed(
        client
            .post(format!("{api}{path}"))
            .json(&request)
            .header("accept", "text/event-stream"),
        provider,
        &credentials,
    )?;
    let request = if protocol == WireProtocol::Anthropic {
        request.header("anthropic-version", "2023-06-01")
    } else {
        request
    };
    let request = if provider == AccountProvider::OpenDesign && protocol == WireProtocol::Anthropic
    {
        request.header("x-api-key", access_token(&credentials)?)
    } else {
        request
    };
    let response = request.send().await.map_err(ProxyError::upstream)?;
    let status = response.status();
    if !status.is_success() {
        let headers = response.headers().clone();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        if provider == AccountProvider::Dumate {
            return Err(dumate_error(status, &headers, &value));
        }
        let error = upstream_error(status, &headers, &value);
        forget_failed_metadata(provider, &credentials, &error);
        return Err(error);
    }
    if provider == AccountProvider::Dumate
        && !response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"))
    {
        let headers = response.headers().clone();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        return Err(dumate_error(status, &headers, &value));
    }
    converted(response, protocol, model, streaming, true).await
}

fn dumate_error(status: StatusCode, headers: &HeaderMap, value: &Value) -> ProxyError {
    let mut error = if status.is_success() {
        stream_error("error", &value.to_string(), headers)
            .unwrap_or_else(|| upstream_error(StatusCode::BAD_GATEWAY, headers, value))
    } else {
        upstream_error(status, headers, value)
    };
    error.message = match error.status {
        StatusCode::UNAUTHORIZED => "DuMate sign-in has expired",
        StatusCode::FORBIDDEN => "DuMate account cannot use this model",
        StatusCode::PAYMENT_REQUIRED => "DuMate credits are exhausted",
        StatusCode::TOO_MANY_REQUESTS => "DuMate usage limit reached",
        _ => "DuMate upstream request failed",
    }
    .into();
    error
}

pub(super) async fn route_responses(
    provider: AccountProvider,
    credentials: Value,
    model: &str,
    mut body: Value,
    client: &reqwest::Client,
) -> Result<Option<Response>, ProxyError> {
    if provider == AccountProvider::Copilot {
        let configured = credentials["modelProtocols"][model]
            .as_str()
            .or_else(|| credentials["protocol"].as_str());
        let supported = if configured == Some("responses") {
            true
        } else {
            let catalog = copilot_catalog(&credentials, client).await?;
            catalog["data"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|row| row["id"] == model)
                .is_some_and(|row| {
                    row["policy"]["state"] != "disabled"
                        && row["visibility"] != "hide"
                        && row["supported_in_api"] != false
                        && row["supported_endpoints"]
                            .as_array()
                            .is_some_and(|endpoints| {
                                endpoints.iter().any(|path| path == "/responses")
                            })
                })
        };
        if !supported {
            return Ok(None);
        }
    } else if provider == AccountProvider::OpenDesign {
        if wire(provider, &credentials, model, client).await?.0 != WireProtocol::Responses {
            return Ok(None);
        }
    } else if provider != AccountProvider::Codex {
        return Ok(None);
    }
    let streaming = body["stream"].as_bool().unwrap_or(false);
    let request = body.as_object_mut().ok_or_else(|| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Responses request must be an object",
        )
    })?;
    request.insert("model".into(), json!(model));
    if provider == AccountProvider::Codex {
        // This account backend requires streaming and does not store responses.
        request.insert("stream".into(), json!(true));
        request.insert("store".into(), json!(false));
        if !request.get("instructions").is_some_and(Value::is_string) {
            request.insert("instructions".into(), json!(""));
        }
        request
            .entry("include")
            .or_insert_with(|| json!(["reasoning.encrypted_content"]));
        request.remove("prompt_cache_options");
    }
    super::responses::repair_responses_tool_history(&mut body);
    let response = signed(
        client
            .post(format!("{}/responses", base(provider, &credentials)))
            .json(&body)
            .header(
                "accept",
                if streaming || provider == AccountProvider::Codex {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            ),
        provider,
        &credentials,
    )?
    .send()
    .await
    .map_err(ProxyError::upstream)?;
    let status = response.status();
    let mut headers = response.headers().clone();
    if !status.is_success() {
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        let error = upstream_error(status, &headers, &value);
        forget_failed_metadata(provider, &credentials, &error);
        return Err(error);
    }
    if provider == AccountProvider::Codex && !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !content_type.is_some_and(|value| {
        value.contains("text/event-stream") || value.contains("application/json")
    }) {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Account API did not return a Responses payload",
        ));
    }
    let mut result = Response::new(Body::from_stream(response.bytes_stream()));
    *result.status_mut() = status;
    *result.headers_mut() = headers;
    responses_response(result, streaming).map(Some)
}

pub fn responses_response(mut response: Response, streaming: bool) -> Result<Response, ProxyError> {
    let connection_headers: Vec<_> = response
        .headers()
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_string())
        .collect();
    for name in connection_headers {
        response.headers_mut().remove(name.as_str());
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "authorization",
        "set-cookie",
        "cookie",
        "content-length",
        "content-encoding",
    ] {
        response.headers_mut().remove(name);
    }
    if !response.status().is_success() {
        return Ok(response);
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    let is_sse = content_type.is_some_and(|value| value.contains("text/event-stream"));
    if streaming {
        if !is_sse {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Responses API did not return the requested event stream",
            ));
        }
        return Ok(response);
    }
    if content_type.is_some_and(|value| value.contains("application/json")) {
        return Ok(response);
    }
    if !is_sse {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Responses API did not return an event stream",
        ));
    }
    let (mut parts, body) = response.into_parts();
    let headers = parts.headers.clone();
    let mut upstream = body.into_data_stream();
    let (tx, rx) = mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(1);
    tokio::spawn(async move {
        let mut parser = SseParser::default();
        loop {
            let next = tokio::select! { _=tx.closed()=>return, next=upstream.next()=>next };
            let eof = next.is_none();
            let frames = match next {
                Some(Ok(bytes)) => parser.push(&bytes),
                Some(Err(error)) => {
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
                None => parser.finish(),
            };
            for (event, data) in frames {
                if let Some(error) = stream_error(&event, &data, &headers) {
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
                let value = serde_json::from_str::<Value>(&data);
                let value = match value {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = tx.send(Err(std::io::Error::other(error))).await;
                        return;
                    }
                };
                let kind = value["type"].as_str().unwrap_or(&event);
                if matches!(kind, "response.completed" | "response.incomplete") {
                    let terminal = &value["response"];
                    if !terminal["error"].is_null() {
                        let error = stream_error("response.failed", &data, &headers)
                            .expect("A non-null terminal error is an explicit failure");
                        let _ = tx.send(Err(std::io::Error::other(error))).await;
                        return;
                    }
                    let expected_status = if kind == "response.completed" {
                        "completed"
                    } else {
                        "incomplete"
                    };
                    if terminal["object"] == "response"
                        && terminal["status"] == expected_status
                        && terminal["id"].as_str().is_some_and(|id| !id.is_empty())
                        && (event.is_empty() || event == kind)
                    {
                        let _ = tx.send(Ok(bytes::Bytes::from(terminal.to_string()))).await;
                    } else {
                        let _ = tx
                            .send(Err(std::io::Error::other(
                                "Responses terminal event has an invalid response object",
                            )))
                            .await;
                    }
                    return;
                }
            }
            if eof {
                let _ = tx
                    .send(Err(std::io::Error::other(
                        "Responses stream ended before completion",
                    )))
                    .await;
                return;
            }
        }
    });
    parts.headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    parts.headers.remove(header::CONTENT_LENGTH);
    Ok(Response::from_parts(
        parts,
        Body::from_stream(ReceiverStream::new(rx)),
    ))
}

pub(super) async fn converted(
    response: reqwest::Response,
    protocol: WireProtocol,
    model: &str,
    streaming: bool,
    tools_allowed: bool,
) -> Result<Response, ProxyError> {
    let response_headers = response.headers().clone();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok());
    // Codex's authenticated backend can omit Content-Type on a valid event stream.
    // The decoder still requires valid SSE events and a terminal response event.
    let is_sse = content_type.is_some_and(|v| v.contains("text/event-stream"));
    if !(is_sse || protocol == WireProtocol::Responses && content_type.is_none()) {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Account API did not return the requested event stream",
        ));
    }
    let mut upstream = response.bytes_stream();
    let mut parser = SseParser::default();
    let mut decoder = ChatDecoder::new(protocol, model);
    // The bounded queue applies downstream backpressure. Closing the response cancels
    // the upstream request, including while it is waiting for its next frame.
    let (tx, rx) = mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        #[cfg(test)]
        let diagnostics =
            protocol == WireProtocol::Trae && super::trae::stream_diagnostics_enabled();
        #[cfg(test)]
        let started = std::time::Instant::now();
        #[cfg(test)]
        let mut event_count = 0usize;
        loop {
            let next = tokio::select! {
                _=tx.closed()=>{
                    #[cfg(test)]
                    if diagnostics { eprintln!("Trae stream cancelled elapsed_ms={}", started.elapsed().as_millis()); }
                    return;
                },
                next=upstream.next()=>next
            };
            let eof = next.is_none();
            let frames = match next {
                Some(Ok(bytes)) => parser.push(&bytes),
                Some(Err(error)) => {
                    #[cfg(test)]
                    if diagnostics {
                        eprintln!(
                            "Trae stream transport_error timeout={} connect={} elapsed_ms={}",
                            error.is_timeout(),
                            error.is_connect(),
                            started.elapsed().as_millis()
                        );
                    }
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
                None => parser.finish(),
            };
            for (event, data) in frames {
                #[cfg(test)]
                if diagnostics && event_count < 24 {
                    event_count += 1;
                    eprintln!(
                        "Trae stream event ordinal={event_count} elapsed_ms={} shape={}",
                        started.elapsed().as_millis(),
                        super::trae::stream_event_shape(&event, &data)
                    );
                }
                if protocol == WireProtocol::Trae
                    && !tools_allowed
                    && serde_json::from_str::<Value>(&data)
                        .ok()
                        .is_some_and(|value| {
                            value["tool_calls"]
                                .as_array()
                                .is_some_and(|calls| !calls.is_empty())
                        })
                {
                    let mut error = ProxyError::new(
                        StatusCode::BAD_REQUEST,
                        "Trae returned a tool call while tool_choice=none",
                    );
                    error.error_code = Some("unsupported_parameter".into());
                    error.model_scope = true;
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
                match decoder.decode(&event, &data) {
                    Ok(chunks) => {
                        if streaming {
                            for chunk in chunks {
                                if tx.send(Ok(chunk)).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    Err(error) => {
                        #[cfg(test)]
                        if diagnostics {
                            eprintln!(
                                "Trae stream decoder_error elapsed_ms={} shape={}",
                                started.elapsed().as_millis(),
                                super::trae::stream_error_shape(&event, &data, &error)
                            );
                        }
                        let error = if protocol == WireProtocol::Trae {
                            super::trae::stream_error(&event, &data, &response_headers)
                        } else {
                            stream_error(&event, &data, &response_headers)
                        }
                        .map(std::io::Error::other)
                        .unwrap_or_else(|| std::io::Error::other(error));
                        let _ = tx.send(Err(error)).await;
                        return;
                    }
                }
            }
            if decoder.finished {
                if !streaming {
                    let _ = tx
                        .send(Ok(bytes::Bytes::from(decoder.collected().to_string())))
                        .await;
                }
                return;
            }
            if eof {
                #[cfg(test)]
                if diagnostics {
                    eprintln!(
                        "Trae stream eof completed=false elapsed_ms={}",
                        started.elapsed().as_millis()
                    );
                }
                let _ = tx
                    .send(Err(std::io::Error::other(
                        "Account stream ended before completion",
                    )))
                    .await;
                return;
            }
        }
    });
    let body = Body::from_stream(ReceiverStream::new(rx));
    Ok(if streaming {
        stream_response(body)
    } else {
        Response::builder()
            .header(header::CONTENT_TYPE, "application/json")
            .body(body)
            .expect("fixed response headers")
    })
}

#[cfg(test)]
mod metadata_performance_tests {
    use super::*;
    use axum::{extract::State, routing::any, Json, Router};
    use std::sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc,
    };
    use std::time::Duration;

    #[derive(Clone)]
    struct Fixture {
        calls: Arc<tokio::sync::Mutex<Vec<String>>>,
        catalog: Arc<tokio::sync::Mutex<Value>>,
        project: Arc<tokio::sync::Mutex<Value>>,
        status: Arc<AtomicU16>,
        error_code: Arc<tokio::sync::Mutex<Option<String>>>,
        delay: Arc<AtomicBool>,
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    async fn upstream(State(state): State<Fixture>, uri: axum::http::Uri) -> Response {
        let path = uri.path().to_owned();
        let catalog = state.catalog.lock().await.clone();
        let project = state.project.lock().await.clone();
        state.calls.lock().await.push(path.clone());
        if state.delay.load(Ordering::SeqCst) {
            let release = state.release.notified();
            state.entered.notify_one();
            release.await;
        } else {
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        let status = StatusCode::from_u16(state.status.load(Ordering::SeqCst)).unwrap();
        if !status.is_success() {
            let mut response = json_response(
                &json!({"error":{"message":"fixture failure","code":state.error_code.lock().await.clone()}}),
            );
            *response.status_mut() = status;
            return response;
        }
        match path.as_str() {
            "/models" => Json(catalog).into_response(),
            "/v1internal:loadCodeAssist" => Json(project).into_response(),
            "/v1internal:fetchAvailableModels" => json_response(&json!({"models":{"gemini-test":{}}})),
            "/v1internal:streamGenerateContent" => stream_response(Body::from("data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}}\n\n")),
            "/chat/completions" => stream_response(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")),
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }

    use axum::response::IntoResponse;

    async fn fixture() -> (Value, Fixture, tokio::task::JoinHandle<()>) {
        let state = Fixture {
            calls: Arc::default(),
            catalog: Arc::new(tokio::sync::Mutex::new(
                json!({"data":[{"id":"chat-test","supported_endpoints":["/chat/completions"]}]}),
            )),
            project: Arc::new(tokio::sync::Mutex::new(
                json!({"cloudaicompanionProject":"validated-project","currentTier":{"id":"standard-tier"}}),
            )),
            status: Arc::new(AtomicU16::new(200)),
            error_code: Arc::default(),
            delay: Arc::new(AtomicBool::new(false)),
            entered: Arc::default(),
            release: Arc::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .fallback(any(upstream))
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (
            json!({"accessToken":"fixture-token","baseUrl":base,"_echobird_saved_account_id":format!("fixture-{}", uuid::Uuid::new_v4()),"login":"fixture"}),
            state,
            server,
        )
    }

    async fn count(state: &Fixture, path: &str) -> usize {
        state
            .calls
            .lock()
            .await
            .iter()
            .filter(|call| *call == path)
            .count()
    }

    #[tokio::test]
    async fn perf_antigravity_warm_generation_reuses_validated_project() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        for _ in 0..2 {
            let response = route(
                AccountProvider::Antigravity,
                credentials.clone(),
                "gemini-test",
                json!({"messages":[{"role":"user","content":"hello"}]}),
                &client,
            )
            .await
            .unwrap();
            let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert!(String::from_utf8(body.to_vec()).unwrap().contains("ok"));
        }
        assert_eq!(count(&state, "/v1internal:loadCodeAssist").await, 1);
        assert_eq!(count(&state, "/v1internal:streamGenerateContent").await, 2);
        server.abort();
    }

    #[tokio::test]
    async fn perf_copilot_responses_fallback_and_warm_route_query_catalog_once() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        assert!(route_responses(
            AccountProvider::Copilot,
            credentials.clone(),
            "chat-test",
            json!({"input":"hello"}),
            &client
        )
        .await
        .unwrap()
        .is_none());
        for _ in 0..2 {
            let response = route(
                AccountProvider::Copilot,
                credentials.clone(),
                "chat-test",
                json!({"messages":[{"role":"user","content":"hello"}]}),
                &client,
            )
            .await
            .unwrap();
            let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert!(String::from_utf8(body.to_vec()).unwrap().contains("ok"));
        }
        assert_eq!(count(&state, "/models").await, 1);
        assert_eq!(count(&state, "/chat/completions").await, 2);
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_simultaneous_cold_requests_coalesce() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        let results =
            futures_util::future::join_all((0..8).map(|_| project(&credentials, &client))).await;
        assert!(results
            .iter()
            .all(|result| result.as_ref().unwrap().0 == "validated-project"));
        assert_eq!(count(&state, "/v1internal:loadCodeAssist").await, 1);
        let results = futures_util::future::join_all(
            (0..8).map(|_| wire(AccountProvider::Copilot, &credentials, "chat-test", &client)),
        )
        .await;
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(count(&state, "/models").await, 1);
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_identity_and_endpoint_changes_do_not_share_permissions() {
        let (credentials, state, server) = fixture().await;
        let (second, other_state, other_server) = fixture().await;
        let client = reqwest::Client::new();
        for provider in [AccountProvider::Antigravity, AccountProvider::Copilot] {
            let mut variants = vec![credentials.clone()];
            for (field, value) in [
                ("accessToken", json!("next-token")),
                ("_echobird_saved_account_id", json!("other-account")),
                ("login", json!("other-login")),
                ("credentialSource", json!("other-native-source")),
                ("headers", json!({"X-Enterprise-Id":"other-enterprise"})),
            ] {
                let mut changed = credentials.clone();
                changed[field] = value;
                variants.push(changed);
            }
            let mut changed_endpoint = credentials.clone();
            changed_endpoint["baseUrl"] = second["baseUrl"].clone();
            variants.push(changed_endpoint);
            for auth in &variants {
                if provider == AccountProvider::Antigravity {
                    project(auth, &client).await.unwrap();
                } else {
                    wire(provider, auth, "chat-test", &client).await.unwrap();
                }
            }
            if provider == AccountProvider::Antigravity {
                project(&credentials, &client).await.unwrap();
            } else {
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
            }
        }
        for path in ["/v1internal:loadCodeAssist", "/models"] {
            assert_eq!(count(&state, path).await, 6);
            assert_eq!(count(&other_state, path).await, 1);
        }
        server.abort();
        other_server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_failure_and_cancelled_load_can_retry_without_stale_warmth() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        state.status.store(403, Ordering::SeqCst);
        assert_eq!(
            project(&credentials, &client).await.unwrap_err().status,
            StatusCode::FORBIDDEN
        );
        state.status.store(200, Ordering::SeqCst);
        project(&credentials, &client).await.unwrap();
        project(&credentials, &client).await.unwrap();
        assert_eq!(count(&state, "/v1internal:loadCodeAssist").await, 2);
        state.delay.store(true, Ordering::SeqCst);
        let auth = credentials.clone();
        let http = client.clone();
        let pending =
            tokio::spawn(
                async move { wire(AccountProvider::Copilot, &auth, "chat-test", &http).await },
            );
        tokio::time::timeout(Duration::from_secs(2), state.entered.notified())
            .await
            .unwrap();
        pending.abort();
        assert!(pending.await.unwrap_err().is_cancelled());
        state.delay.store(false, Ordering::SeqCst);
        state.release.notify_waiters();
        wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
            .await
            .unwrap();
        wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
            .await
            .unwrap();
        assert_eq!(count(&state, "/models").await, 2);
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_explicit_model_refresh_updates_both_preparation_paths() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
            .await
            .unwrap();
        *state.catalog.lock().await =
            json!({"data":[{"id":"new-test","supported_endpoints":["/responses"]}]});
        assert_eq!(
            list_models(AccountProvider::Copilot, credentials.clone(), &client)
                .await
                .unwrap(),
            ["new-test"]
        );
        assert_eq!(
            wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
                .await
                .unwrap_err()
                .status,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            wire(AccountProvider::Copilot, &credentials, "new-test", &client)
                .await
                .unwrap()
                .1,
            "/responses"
        );
        assert_eq!(count(&state, "/models").await, 2);
        assert_eq!(
            project(&credentials, &client).await.unwrap().0,
            "validated-project"
        );
        *state.project.lock().await =
            json!({"cloudaicompanionProject":"refreshed-project","currentTier":{"id":"free-tier"}});
        list_models(AccountProvider::Antigravity, credentials.clone(), &client)
            .await
            .unwrap();
        assert_eq!(
            project(&credentials, &client).await.unwrap(),
            ("refreshed-project".into(), None)
        );
        assert_eq!(count(&state, "/v1internal:loadCodeAssist").await, 2);
        state.status.store(403, Ordering::SeqCst);
        assert!(
            list_models(AccountProvider::Copilot, credentials.clone(), &client)
                .await
                .is_err()
        );
        assert_eq!(
            wire(AccountProvider::Copilot, &credentials, "new-test", &client)
                .await
                .unwrap_err()
                .status,
            StatusCode::FORBIDDEN
        );
        assert!(
            list_models(AccountProvider::Antigravity, credentials.clone(), &client)
                .await
                .is_err()
        );
        assert_eq!(
            project(&credentials, &client).await.unwrap_err().status,
            StatusCode::FORBIDDEN
        );
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_expiry_and_bad_payloads_require_new_validated_loads() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        for (provider, lifetime, path) in [
            (
                AccountProvider::Antigravity,
                PROJECT_LIFETIME,
                "/v1internal:loadCodeAssist",
            ),
            (
                AccountProvider::Copilot,
                COPILOT_CATALOG_LIFETIME,
                "/models",
            ),
        ] {
            if provider == AccountProvider::Antigravity {
                project(&credentials, &client).await.unwrap();
            } else {
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
            }
            let cell = metadata_cell(provider, &credentials);
            cell.value.lock().await.as_mut().unwrap().0 =
                Instant::now() - lifetime - Duration::from_secs(1);
            if provider == AccountProvider::Antigravity {
                project(&credentials, &client).await.unwrap();
            } else {
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
            }
            assert_eq!(count(&state, path).await, 2);
        }
        *state.project.lock().await = Value::Null;
        *state.catalog.lock().await = json!({"data":[{"unexpected":true}]});
        for provider in [AccountProvider::Antigravity, AccountProvider::Copilot] {
            assert!(list_models(provider, credentials.clone(), &client)
                .await
                .is_err());
            if provider == AccountProvider::Antigravity {
                assert!(project(&credentials, &client).await.is_err());
            } else {
                assert!(wire(provider, &credentials, "chat-test", &client)
                    .await
                    .is_err());
            }
        }
        *state.project.lock().await =
            json!({"ineligibleTiers":[{"reasonMessage":"Account is not eligible"}]});
        assert_eq!(
            project(&credentials, &client).await.unwrap_err().status,
            StatusCode::FORBIDDEN
        );
        *state.project.lock().await = json!({"cloudaicompanionProject":"recovered-project"});
        *state.catalog.lock().await =
            json!({"data":[{"id":"recovered","supported_endpoints":["/chat/completions"]}]});
        assert_eq!(
            project(&credentials, &client).await.unwrap().0,
            "recovered-project"
        );
        wire(AccountProvider::Copilot, &credentials, "recovered", &client)
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_catalogs_distinguish_authoritative_denials_from_malformed_rows() {
        let (credentials, state, server) = fixture().await;
        let client = reqwest::Client::new();
        *state.catalog.lock().await = json!({"data":[]});
        for _ in 0..2 {
            assert_eq!(
                wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(count(&state, "/models").await, 1);
        assert_eq!(
            list_models(AccountProvider::Copilot, credentials.clone(), &client)
                .await
                .unwrap_err()
                .status,
            StatusCode::BAD_GATEWAY
        );
        for invalid in [
            json!({"data":[{"unexpected":true}]}),
            json!({"data":[{"id":" "}]}),
            json!({"data":[{"id":23}]}),
            json!({"data":{}}),
        ] {
            *state.catalog.lock().await = invalid;
            let before = count(&state, "/models").await;
            for _ in 0..2 {
                assert_eq!(
                    wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
                        .await
                        .unwrap_err()
                        .status,
                    StatusCode::BAD_GATEWAY
                );
            }
            assert_eq!(count(&state, "/models").await, before + 2);
        }
        *state.catalog.lock().await = json!({"data":[{"id":"chat-test","supported_endpoints":["/chat/completions"],"policy":{"state":"disabled"}}]});
        let before = count(&state, "/models").await;
        for _ in 0..2 {
            assert_eq!(
                wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(count(&state, "/models").await, before + 1);
        *state.catalog.lock().await = json!({"data":[{"unexpected":true},{"id":"chat-test","supported_endpoints":["/chat/completions"]}]});
        assert_eq!(
            list_models(AccountProvider::Copilot, credentials.clone(), &client)
                .await
                .unwrap(),
            ["chat-test"]
        );
        wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn perf_metadata_inference_rejections_invalidate_permissions_without_replaying() {
        for provider in [AccountProvider::Antigravity, AccountProvider::Copilot] {
            let (credentials, state, server) = fixture().await;
            let client = reqwest::Client::new();
            let path = if provider == AccountProvider::Antigravity {
                "/v1internal:loadCodeAssist"
            } else {
                "/models"
            };
            if provider == AccountProvider::Antigravity {
                project(&credentials, &client).await.unwrap();
            } else {
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
            }
            state.status.store(403, Ordering::SeqCst);
            assert_eq!(
                route(
                    provider,
                    credentials.clone(),
                    "chat-test",
                    json!({"messages":[]}),
                    &client
                )
                .await
                .unwrap_err()
                .status,
                StatusCode::FORBIDDEN
            );
            assert_eq!(count(&state, path).await, 1);
            state.status.store(200, Ordering::SeqCst);
            if provider == AccountProvider::Antigravity {
                project(&credentials, &client).await.unwrap();
            } else {
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
            }
            assert_eq!(count(&state, path).await, 2);
            if provider == AccountProvider::Copilot {
                state.status.store(404, Ordering::SeqCst);
                *state.error_code.lock().await = Some("model_not_found".into());
                assert_eq!(
                    route(
                        provider,
                        credentials.clone(),
                        "chat-test",
                        json!({"messages":[]}),
                        &client
                    )
                    .await
                    .unwrap_err()
                    .error_code
                    .as_deref(),
                    Some("model_not_found")
                );
                state.status.store(200, Ordering::SeqCst);
                wire(provider, &credentials, "chat-test", &client)
                    .await
                    .unwrap();
                assert_eq!(count(&state, path).await, 3);
                *state.catalog.lock().await = json!({"data":[{"id":"chat-test","supported_endpoints":["/responses", "/chat/completions"]}]});
                list_models(provider, credentials.clone(), &client)
                    .await
                    .unwrap();
                assert_eq!(
                    route_responses(
                        provider,
                        credentials.clone(),
                        "chat-test",
                        json!({"input":"hello"}),
                        &client
                    )
                    .await
                    .unwrap_err()
                    .status,
                    StatusCode::NOT_FOUND
                );
                let response = route(
                    provider,
                    credentials.clone(),
                    "chat-test",
                    json!({"messages":[]}),
                    &client,
                )
                .await
                .unwrap();
                axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                assert_eq!(count(&state, path).await, 4);
            }
            server.abort();
        }
    }

    #[tokio::test]
    async fn perf_metadata_invalidation_and_refresh_never_warm_from_late_old_loads() {
        for force_refresh in [false, true] {
            let (credentials, state, server) = fixture().await;
            let client = reqwest::Client::new();
            state.delay.store(true, Ordering::SeqCst);
            let auth = credentials.clone();
            let http = client.clone();
            let pending = tokio::spawn(async move {
                wire(AccountProvider::Copilot, &auth, "chat-test", &http).await
            });
            tokio::time::timeout(Duration::from_secs(2), state.entered.notified())
                .await
                .unwrap();
            *state.catalog.lock().await =
                json!({"data":[{"id":"new-test","supported_endpoints":["/responses"]}]});
            let refresh = if force_refresh {
                let cell = metadata_cell(AccountProvider::Copilot, &credentials);
                let auth = credentials.clone();
                let http = client.clone();
                let refresh = tokio::spawn(async move {
                    list_models(AccountProvider::Copilot, auth, &http).await
                });
                tokio::time::timeout(Duration::from_secs(2), async {
                    while cell.generation.load(Ordering::SeqCst) == 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                Some(refresh)
            } else {
                forget_metadata(
                    AccountProvider::Copilot,
                    credentials["_echobird_saved_account_id"].as_str().unwrap(),
                );
                None
            };
            state.delay.store(false, Ordering::SeqCst);
            state.release.notify_waiters();
            assert_eq!(pending.await.unwrap().unwrap().1, "/chat/completions");
            if let Some(refresh) = refresh {
                assert_eq!(refresh.await.unwrap().unwrap(), ["new-test"]);
            }
            assert_eq!(
                wire(AccountProvider::Copilot, &credentials, "chat-test", &client)
                    .await
                    .unwrap_err()
                    .status,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                wire(AccountProvider::Copilot, &credentials, "new-test", &client)
                    .await
                    .unwrap()
                    .1,
                "/responses"
            );
            assert_eq!(count(&state, "/models").await, 2);
            server.abort();
        }
    }

    #[test]
    fn perf_metadata_map_is_bounded_and_does_not_evict_inflight_cells() {
        let mut cells = BTreeMap::new();
        let mut held = Vec::new();
        for id in 0..METADATA_CACHE_LIMIT {
            let credentials = json!({"accessToken":"fixture-secret","_echobird_saved_account_id":format!("account-{id}")});
            held.push(metadata_cell_at(
                &mut cells,
                AccountProvider::Copilot,
                &credentials,
            ));
        }
        assert_eq!(cells.len(), METADATA_CACHE_LIMIT);
        assert!(cells
            .keys()
            .all(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())));
        let overflow =
            json!({"accessToken":"other-secret","_echobird_saved_account_id":"overflow"});
        let first = metadata_cell_at(&mut cells, AccountProvider::Copilot, &overflow);
        let second = metadata_cell_at(&mut cells, AccountProvider::Copilot, &overflow);
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(cells.len(), METADATA_CACHE_LIMIT);
        drop(held);
        let cached = metadata_cell_at(&mut cells, AccountProvider::Copilot, &overflow);
        assert!(Arc::ptr_eq(
            &cached,
            &metadata_cell_at(&mut cells, AccountProvider::Copilot, &overflow)
        ));
        assert_eq!(cells.len(), METADATA_CACHE_LIMIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::{Request, State},
        Router,
    };
    use std::sync::Arc;
    use tokio::sync::Mutex;

    type Requests = Arc<Mutex<Vec<(String, reqwest::header::HeaderMap, Value)>>>;
    async fn upstream(State(requests): State<Requests>, request: Request) -> Response {
        let path = request.uri().to_string();
        let headers = request.headers().clone();
        let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
        requests
            .lock()
            .await
            .push((path.clone(), headers, body.clone()));
        if path == "/v1internal:loadCodeAssist" {
            return json_response(
                &json!({"cloudaicompanionProject":"test-project","currentTier":{"id":"standard-tier"}}),
            );
        }
        if path == "/free/v1internal:loadCodeAssist" {
            return json_response(
                &json!({"cloudaicompanionProject":"test-project","currentTier":{"id":"free-tier"},"paidTier":{"id":"free-tier"}}),
            );
        }
        if path == "/v1internal:fetchAvailableModels" {
            if body["entitlement"]["userTier"] == "free-tier" {
                let mut response = json_response(
                    &json!({"error":{"status":"FAILED_PRECONDITION","message":"Precondition check failed."}}),
                );
                *response.status_mut() = StatusCode::BAD_REQUEST;
                return response;
            }
            return json_response(
                &json!({"models":{"gemini-test":{},"image-gen":{},"disabled":{"disabled":true}}}),
            );
        }
        if path == "/v3/config" {
            return json_response(
                &json!({"code":0,"data":{"agents":[{"name":"cli","models":["workbuddy-test"]}]}}),
            );
        }
        if path == "/responses" {
            if body["model"] == "quota-stream" {
                return stream_response(Body::from("event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"error\":{\"type\":\"usage_limit_reached\",\"code\":\"rate_limit_exceeded\",\"message\":\"Account rate limit\",\"scope\":\"model\",\"resets_in_seconds\":90}}}\n\n"));
            }
            if body["model"] == "headerless-invalid" {
                return Response::new(Body::from("{\"not\":\"an event stream\"}"));
            }
            if body["model"] == "done-only" {
                return stream_response(Body::from("event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\ndata: [DONE]\n\n"));
            }
            let incomplete =
                body["model"] == "terminal-incomplete" || body["model"] == "terminal-auth";
            let kind = if incomplete {
                "response.incomplete"
            } else {
                "response.completed"
            };
            let mut terminal = json!({"type":kind,"response":{"object":"response","id":"resp_fixture","status":if incomplete { "incomplete" } else { "completed" },"error":null,"usage":{"input_tokens":1,"output_tokens":1}}});
            if body["model"] == "fake-completed" {
                terminal["response"]["status"] = json!("in_progress");
            } else if body["model"] == "terminal-quota" {
                terminal["response"]["status"] = json!("failed");
                terminal["response"]["error"] = json!({"code":"rate_limit_exceeded","type":"usage_limit_reached","message":"Account limit","scope":"model","resets_in_seconds":90});
            } else if body["model"] == "terminal-auth" {
                terminal["response"]["status"] = json!("failed");
                terminal["response"]["error"] = json!({"code":"authentication_error","message":"Account authorization expired"});
            }
            let payload = Body::from(format!("event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"ok\"}}\n\nevent: {kind}\ndata: {terminal}\n\n"));
            if body["model"] == "headerless" {
                return Response::new(payload);
            }
            return stream_response(payload);
        }
        if path.starts_with("/v1internal:streamGenerateContent") {
            return stream_response(Body::from("data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}]}}\n\n"));
        }
        if path == "/v2/chat/completions" {
            return stream_response(Body::from("data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"));
        }
        if path == "/v1/messages" {
            return stream_response(Body::from("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_fixture\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":1,\"output_tokens\":0}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"ok\"}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"));
        }
        let mut response = json_response(
            &json!({"error":{"message":"account quota","code":"rate_limit_exceeded","type":"rate_limit_error","scope":"model"}}),
        );
        *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("120"));
        response
    }

    async fn fixture() -> (String, Requests, tokio::task::JoinHandle<()>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .fallback(upstream)
            .with_state(requests.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (base, requests, server)
    }

    #[test]
    fn tool_history_messages_pairs_parallel_calls_before_followup() {
        let input = json!({"messages":[
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"a","type":"function","function":{"name":"read","arguments":"{}"}},
                {"id":"b","type":"function","function":{"name":"write","arguments":"{}"}}
            ]},
            {"role":"tool","tool_call_id":"a","content":"first"},
            {"role":"tool","tool_call_id":"a","content":"duplicate"},
            {"role":"user","content":"Continue"}
        ]});
        let request = anthropic_request(&input, "claude-test");
        let messages = request["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        let results = messages[1]["content"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["content"], "first");
        assert_eq!(results[1]["tool_use_id"], "b");
        assert_eq!(results[1]["is_error"], true);
        assert_eq!(messages[2]["content"][0]["text"], "Continue");
    }

    #[tokio::test]
    async fn tool_history_account_wire_repairs_without_extra_inference_requests() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        for provider in [AccountProvider::Codex, AccountProvider::Antigravity] {
            let body = json!({"stream":true,"messages":[
                {"role":"assistant","content":null,"tool_calls":[{"id":"a","type":"function","function":{"name":"read","arguments":"{}"}}]},
                {"role":"user","content":"Continue after interruption"}
            ]});
            let response = route(
                provider,
                json!({"accessToken":"fixture","baseUrl":base}),
                "tool-history-fixture",
                body,
                &client,
            )
            .await
            .unwrap();
            axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
        }
        let sent = requests.lock().await;
        assert_eq!(
            sent.iter()
                .filter(|(path, _, _)| path == "/responses")
                .count(),
            1
        );
        assert_eq!(
            sent.iter()
                .filter(|(path, _, _)| path.starts_with("/v1internal:streamGenerateContent"))
                .count(),
            1
        );
        let codex = &sent
            .iter()
            .find(|(path, _, _)| path == "/responses")
            .unwrap()
            .2;
        assert_eq!(codex["input"][1]["type"], "function_call_output");
        assert_eq!(codex["input"][1]["call_id"], "a");
        let google = &sent
            .iter()
            .find(|(path, _, _)| path.starts_with("/v1internal:streamGenerateContent"))
            .unwrap()
            .2;
        assert_eq!(
            google["request"]["contents"][1]["parts"][0]["functionResponse"]["id"],
            "a"
        );
        server.abort();
    }

    #[tokio::test]
    async fn tool_history_codex_native_filters_unsupported_cache_option_only_on_its_backend() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let body = json!({"stream":true,"input":[
            {"type":"custom_tool_call","id":"ctc_fixture","call_id":"a","namespace":"fixture","name":"write","input":"patch"},
            {"role":"user","content":"Continue"}
        ],"tools":[{"type":"custom","name":"write","format":{"type":"text"}}],
            "prompt_cache_options":{"extended_retention":true},"prompt_cache_key":"keep-this-key","metadata":{"keep":true}});
        for provider in [AccountProvider::Codex, AccountProvider::Copilot] {
            let response = route_responses(
                provider,
                json!({"accessToken":"fixture","baseUrl":base,"protocol":"responses"}),
                "native",
                body.clone(),
                &client,
            )
            .await
            .unwrap()
            .unwrap();
            axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
        }
        let sent = requests.lock().await;
        assert_eq!(sent.len(), 2);
        assert!(sent[0].2.get("prompt_cache_options").is_none());
        assert_eq!(
            sent[1].2["prompt_cache_options"],
            body["prompt_cache_options"]
        );
        for (_, _, request) in sent.iter() {
            assert_eq!(request["prompt_cache_key"], body["prompt_cache_key"]);
            assert_eq!(request["metadata"], body["metadata"]);
            assert_eq!(request["input"][0], body["input"][0]);
            assert_eq!(request["input"][1]["type"], "custom_tool_call_output");
            assert_eq!(request["input"][1]["call_id"], "a");
        }
        server.abort();
    }

    #[tokio::test]
    async fn chat_wire_preserves_structured_format_and_other_protocols_reject_before_inference() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let input = json!({"model":"auto","input":"Return an answer","text":{"format":{"type":"json_schema","name":"answer","schema":{"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false},"strict":true}}});
        let chat = super::super::responses_to_chat(&input).unwrap();
        let response = route(
            AccountProvider::WorkBuddy,
            json!({"accessToken":"fixture","uid":"fixture-user","baseUrl":base}),
            "workbuddy-test",
            chat.clone(),
            &client,
        )
        .await
        .unwrap();
        axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        {
            let captured = requests.lock().await;
            assert_eq!(captured.len(), 1);
            assert_eq!(captured[0].0, "/v2/chat/completions");
            assert_eq!(captured[0].2["response_format"], chat["response_format"]);
            assert!(captured[0].2.get("text").is_none());
            assert!(captured[0]
                .2
                .get(super::super::tools::RESPONSES_REQUEST)
                .is_none());
        }
        for provider in [AccountProvider::Antigravity, AccountProvider::Copilot] {
            for format in [
                json!({"type":"json_object"}),
                chat["response_format"].clone(),
            ] {
                let mut request = chat.clone();
                request["response_format"] = format;
                let error = route(
                    provider,
                    json!({"accessToken":"fixture","baseUrl":base,"protocol":"messages"}),
                    "model-fixture",
                    request,
                    &client,
                )
                .await
                .unwrap_err();
                assert_eq!(error.status, StatusCode::BAD_REQUEST);
                assert_eq!(error.error_code.as_deref(), Some("unsupported_parameter"));
                assert!(error.model_scope);
            }
        }
        assert_eq!(requests.lock().await.len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn native_responses_preserve_file_inputs_and_structured_format() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        for provider in [AccountProvider::Codex, AccountProvider::Copilot] {
            for stream in [false, true] {
                let input = json!({"model":"auto","stream":stream,"input":[{"role":"user","content":[{"type":"input_text","text":"Summarize the file"},{"type":"input_file","file_id":"file_fixture"}]}],"text":{"format":{"type":"json_schema","name":"summary","schema":{"type":"object"},"strict":true}}});
                let response = route_responses(
                    provider,
                    json!({"accessToken":"fixture","baseUrl":base,"protocol":"responses"}),
                    "native-fixture",
                    input.clone(),
                    &client,
                )
                .await
                .unwrap()
                .unwrap();
                axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                let captured = requests.lock().await;
                let request = captured.last().unwrap();
                assert_eq!(request.0, "/responses");
                assert_eq!(request.2["input"], input["input"]);
                assert_eq!(request.2["text"], input["text"]);
                assert!(request.2.get("response_format").is_none());
            }
        }
        assert_eq!(requests.lock().await.len(), 4);
        server.abort();
    }

    #[tokio::test]
    async fn codex_and_workbuddy_send_identity_and_their_actual_endpoints() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let body = json!({"stream":false,"messages":[{"role":"user","content":"hello"}]});
        let response = route(
            AccountProvider::Codex,
            json!({"accessToken":"fixture","accountId":"own-account","_echobird_saved_account_id":"saved-echo-account","baseUrl":base}),
            "gpt-test",
            body.clone(),
            &client,
        )
        .await
        .unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["choices"][0]["message"]["content"], "ok");
        route(AccountProvider::WorkBuddy,json!({"accessToken":"fixture","uid":"own-uid","domain":"fixture-domain","baseUrl":base}),"workbuddy-test",body,&client).await.unwrap();
        let recorded = requests.lock().await;
        assert_eq!(recorded[0].0, "/responses");
        assert_eq!(recorded[0].1["chatgpt-account-id"], "own-account");
        assert!(!recorded[0].1.contains_key("_echobird_saved_account_id"));
        assert_eq!(recorded[0].2["store"], false);
        assert_eq!(recorded[1].0, "/v2/chat/completions");
        assert_eq!(recorded[1].1["x-user-id"], "own-uid");
        assert_eq!(recorded[1].1["x-domain"], "fixture-domain");
        assert_eq!(recorded[1].2["messages"][0]["role"], "system");
        assert_eq!(recorded[1].2["messages"][1]["content"], "hello");
        server.abort();
    }

    #[tokio::test]
    async fn protocol_regression_workbuddy_claude_uses_messages_and_effort() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = json!({"accessToken":"fixture","uid":"own-uid","baseUrl":base});
        for (provider, model, effort) in [
            (AccountProvider::WorkBuddy, "claude-sonnet-test", "max"),
            (AccountProvider::WorkBuddy, "claude-sonnet-test", "none"),
            (AccountProvider::WorkBuddyAi, "claude-sonnet-test", "max"),
            (AccountProvider::WorkBuddy, "kimi-test", "max"),
        ] {
            let response = route(
                provider,
                credentials.clone(),
                model,
                json!({"messages":[{"role":"user","content":"hello"}],"reasoning_effort":effort}),
                &client,
            )
            .await
            .unwrap();
            let output: Value = serde_json::from_slice(
                &axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(output["choices"][0]["message"]["content"], "ok");
        }
        let calls = requests.lock().await;
        assert_eq!(calls[0].0, "/v1/messages");
        assert_eq!(calls[0].2["thinking"]["type"], "adaptive");
        assert_eq!(calls[0].2["output_config"]["effort"], "max");
        assert_eq!(calls[0].1["x-user-id"], "own-uid");
        assert_eq!(calls[0].1["anthropic-version"], "2023-06-01");
        assert_eq!(calls[1].2["thinking"]["type"], "disabled");
        for call in &calls[2..] {
            assert_eq!(call.0, "/v2/chat/completions");
        }
        server.abort();
    }

    #[tokio::test]
    async fn protocol_regression_workbuddy_object_unions() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let schema = json!({"type":"object","properties":{"choice":{"oneOf":[{"type":"object"},{"type":"object"}]}},"default":{"oneOf":[{"type":"object"}]}});
        for provider in [AccountProvider::WorkBuddy, AccountProvider::WorkBuddyAi] {
            route(provider, json!({"accessToken":"fixture","uid":"fixture","baseUrl":base}), "kimi-test", json!({"messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"read","parameters":schema}}]}), &client).await.unwrap();
        }
        for call in requests.lock().await.iter() {
            let actual = &call.2["tools"][0]["function"]["parameters"];
            assert_eq!(actual["properties"]["choice"]["type"], "object");
            assert_eq!(actual["default"], schema["default"]);
        }
        server.abort();
    }

    #[tokio::test]
    async fn protocol_regression_workbuddy_messages_preserve_tools_history_and_controls() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let schema = json!({"type":"object","properties":{"choice":{"oneOf":[{"type":"object"},{"type":"object"}]}}});
        let body = json!({"messages":[{"role":"system","content":"Original instructions"},{"role":"assistant","content":null,"tool_calls":[{"id":"previous","type":"function","function":{"name":"read","arguments":"{}"}}]},{"role":"tool","tool_call_id":"previous","content":"previous result"}],"tools":[{"type":"function","function":{"name":"read","parameters":schema}}],"tool_choice":{"type":"function","function":{"name":"read"}},"max_completion_tokens":4096,"temperature":0.2,"top_p":0.8,"stop":["STOP"],"parallel_tool_calls":false});
        route(
            AccountProvider::WorkBuddy,
            json!({"accessToken":"fixture","uid":"fixture","baseUrl":base}),
            "claude-sonnet-test",
            body.clone(),
            &client,
        )
        .await
        .unwrap();
        let calls = requests.lock().await;
        assert_eq!(calls.len(), 1);
        let actual = &calls[0].2;
        assert_eq!(actual["system"], "Original instructions");
        assert_eq!(actual["messages"][0]["content"][0]["id"], "previous");
        assert_eq!(
            actual["messages"][1]["content"][0]["tool_use_id"],
            "previous"
        );
        assert_eq!(
            actual["messages"][1]["content"][0]["content"],
            "previous result"
        );
        assert_eq!(
            actual["tool_choice"],
            json!({"type":"any","disable_parallel_tool_use":true})
        );
        assert_eq!(
            actual["tools"][0]["input_schema"]["properties"]["choice"]["type"],
            "object"
        );
        for key in ["temperature", "top_p"] {
            assert_eq!(actual[key], body[key]);
        }
        assert_eq!(actual["max_tokens"], 4096);
        assert_eq!(actual["stop_sequences"], body["stop"]);
        assert_eq!(body["tools"][0]["function"]["parameters"], schema);
        server.abort();
    }

    #[tokio::test]
    async fn workbuddy_named_tool_choice_keeps_the_forced_tool_contract() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let selected = json!({"type":"function","function":{"name":"echo_probe","parameters":{"type":"object"},"strict":true}});
        let original = json!({"messages":[{"role":"user","content":"Call another tool."}],"tools":[selected, {"type":"function","function":{"name":"another_probe"}}],"tool_choice":{"type":"function","function":{"name":"echo_probe"}}});
        for provider in [AccountProvider::WorkBuddy, AccountProvider::WorkBuddyAi] {
            route(
                provider,
                json!({"accessToken":"fixture","uid":"fixture","baseUrl":base}),
                "test",
                original.clone(),
                &client,
            )
            .await
            .unwrap();
        }
        assert!(route(
            AccountProvider::Kilo,
            json!({"accessToken":"fixture","baseUrl":base}),
            "test",
            original.clone(),
            &client
        )
        .await
        .is_err());
        let recorded = requests.lock().await;
        for request in &recorded[..2] {
            assert_eq!(request.2["tool_choice"], "required");
            assert_eq!(request.2["tools"], json!([selected]));
            assert_eq!(request.2["messages"][1], original["messages"][0]);
        }
        assert_eq!(recorded[2].2["tool_choice"], original["tool_choice"]);
        assert_eq!(recorded[2].2["tools"], original["tools"]);
        server.abort();
    }

    #[tokio::test]
    async fn workbuddy_string_choices_keep_tools_and_continuation_history() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let history = json!([
            {"role":"system","content":"Original system"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"call_a","type":"function","function":{"name":"echo_probe","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call_a","content":"probe-ok"}
        ]);
        let tools = json!([{"type":"function","function":{"name":"echo_probe"}},{"type":"function","function":{"name":"another_probe"}}]);
        for choice in ["auto", "none", "required"] {
            route(
                AccountProvider::WorkBuddy,
                json!({"accessToken":"fixture","uid":"fixture","baseUrl":base}),
                "test",
                json!({"messages":history,"tools":tools,"tool_choice":choice}),
                &client,
            )
            .await
            .unwrap();
        }
        for (request, choice) in requests
            .lock()
            .await
            .iter()
            .zip(["auto", "none", "required"])
        {
            assert_eq!(request.2["tool_choice"], choice);
            assert_eq!(request.2["tools"], tools);
            assert_eq!(request.2["messages"], history);
        }
        server.abort();
    }

    #[tokio::test]
    async fn workbuddy_invalid_named_choices_do_not_contact_upstream() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        for choice in [
            json!({"type":"function","function":{"name":"missing"}}),
            json!({"type":"function"}),
            json!({"type":"custom","name":"echo_probe"}),
        ] {
            let error = route(AccountProvider::WorkBuddy, json!({"accessToken":"fixture","uid":"fixture","baseUrl":base}), "test", json!({"messages":[],"tools":[{"type":"function","function":{"name":"echo_probe"}}],"tool_choice":choice}), &client).await.unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
        }
        assert!(requests.lock().await.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn workbuddy_ai_catalog_and_chat_keep_required_headers_and_original_prompt() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = json!({"accessToken":"fixture","uid":"own-uid","baseUrl":base});
        list_models(AccountProvider::WorkBuddyAi, credentials.clone(), &client)
            .await
            .unwrap();
        route(AccountProvider::WorkBuddyAi,credentials,"test",json!({"stream":false,"messages":[{"role":"system","content":"Original client identity"},{"role":"user","content":"hello"}],"_echobird_responses":{"input":"hello"}}),&client).await.unwrap();
        let recorded = requests.lock().await;
        assert_eq!(recorded[0].0, "/v3/config");
        assert_eq!(recorded[0].1["user-agent"], "CLI/2.0.0 WorkBuddy/5.5.6");
        assert_eq!(recorded[0].1["x-requested-with"], "XMLHttpRequest");
        assert_eq!(recorded[0].1["accept"], "application/json");
        assert_eq!(recorded[1].1["x-requested-with"], "XMLHttpRequest");
        assert_eq!(recorded[1].1["x-agent-intent"], "craft");
        assert_eq!(
            recorded[1].2["messages"][0]["content"],
            "Original client identity"
        );
        assert!(recorded[1]
            .2
            .get(super::super::tools::RESPONSES_REQUEST)
            .is_none());
        server.abort();
    }

    #[test]
    fn workbuddy_free_metadata_requires_an_explicit_zero_price_in_the_cli_directory() {
        let free_prices = [json!(0), json!("0"), json!("x0.00"), json!("X0.00")];
        let unknown_or_paid = [
            Value::Null,
            json!(false),
            json!(""),
            json!("unknown"),
            json!("NaN"),
            json!("inf"),
            json!("Infinity"),
            json!("xNaN"),
            json!(-1),
            json!("-0.1"),
            json!(0.79),
            json!("x1.62"),
        ];
        for credits in free_prices {
            let value = json!({"data":{"models":[
                {"id":"allowed","credits":credits},
                {"id":"unavailable","credits":0},
                {"id":"allowed","credits":credits},
                {"id":"missing"}
            ]}});
            assert_eq!(
                workbuddy_free_model_ids(&value, &["allowed".into(), "missing".into()]),
                ["allowed"]
            );
        }
        for credits in unknown_or_paid {
            assert!(workbuddy_free_model_ids(
                &json!({"data":{"models":[{"id":"allowed","credits":credits}]}}),
                &["allowed".into()]
            )
            .is_empty());
        }
        assert!(workbuddy_free_model_ids(&json!({"data":{}}), &["allowed".into()]).is_empty());
    }

    #[test]
    fn kilo_free_metadata_obeys_the_flag_and_requires_explicit_zero_for_every_fee() {
        for (row, expected) in [
            (
                json!({"isFree":true,"pricing":{"prompt":"1","completion":"1"}}),
                true,
            ),
            (
                json!({"isFree":false,"pricing":{"prompt":0,"completion":"0"}}),
                false,
            ),
            (
                json!({"isFree":null,"pricing":{"prompt":0,"completion":0}}),
                false,
            ),
            (json!({"isFree":"true"}), false),
            (json!({"pricing":{"prompt":0,"completion":"0"}}), true),
            (
                json!({"pricing":{"prompt":"0.00","completion":0,"request":"0"}}),
                true,
            ),
            (
                json!({"pricing":{"prompt":0,"completion":0,"request":"0.1"}}),
                false,
            ),
            (
                json!({"pricing":{"prompt":0,"completion":0,"image":null}}),
                false,
            ),
            (
                json!({"pricing":{"prompt":0,"completion":0,"request":"unknown"}}),
                false,
            ),
            (json!({"pricing":{"prompt":0}}), false),
            (json!({"pricing":{"prompt":null,"completion":0}}), false),
            (json!({"pricing":{"prompt":"NaN","completion":0}}), false),
            (json!({"pricing":{"prompt":"inf","completion":0}}), false),
            (json!({"pricing":{"prompt":-1,"completion":0}}), false),
            (json!({"pricing":{"prompt":0,"completion":"x0"}}), false),
            (json!({}), false),
        ] {
            let mut row = row;
            row["id"] = json!("allowed");
            let value = json!({"data":[row,{"id":"unavailable","isFree":true}]});
            let free = kilo_free_model_ids(&value, &["allowed".into()]);
            assert_eq!(free == ["allowed"], expected, "{value}");
        }
    }

    #[test]
    fn free_metadata_cache_binds_to_the_saved_identity_and_isolates_provider_and_account() {
        let account_id = uuid::Uuid::new_v4().to_string();
        let second_id = uuid::Uuid::new_v4().to_string();
        let native_id = uuid::Uuid::new_v4().to_string();
        let credentials = json!({"_echobird_saved_account_id":account_id,"accountId":native_id});
        for (provider, model) in [
            (AccountProvider::WorkBuddy, "workbuddy-free"),
            (AccountProvider::Kilo, "kilo-free"),
            (AccountProvider::Cline, "cline-free"),
        ] {
            remember_free_model_ids(provider, &credentials, vec![model.into()]);
            assert_eq!(cached_free_model_ids(provider, &account_id), [model]);
            assert!(cached_free_model_ids(provider, &native_id).is_empty());
            assert!(cached_free_model_ids(provider, &second_id).is_empty());
        }
        assert_eq!(
            cached_free_model_ids(AccountProvider::WorkBuddy, &account_id),
            ["workbuddy-free"]
        );
        remember_free_model_ids(
            AccountProvider::Codex,
            &json!({"accountId":native_id}),
            vec!["untrusted".into()],
        );
        assert!(cached_free_model_ids(AccountProvider::Codex, &native_id).is_empty());
        let mut cache = free_models().lock().unwrap();
        for provider in [
            AccountProvider::WorkBuddy,
            AccountProvider::Kilo,
            AccountProvider::Cline,
        ] {
            cache.remove(&free_model_cache_key(provider, &account_id));
        }
    }

    #[tokio::test]
    async fn workbuddy_free_metadata_refreshes_only_after_a_successful_account_directory() {
        type CatalogState = Arc<Mutex<(StatusCode, Value)>>;
        async fn catalog(State(state): State<CatalogState>) -> Response {
            let (status, value) = state.lock().await.clone();
            let mut response = json_response(&value);
            *response.status_mut() = status;
            response
        }
        let free = json!({"code":0,"data":{
            "agents":[{"name":"cli","models":["free-model","paid-model"]}],
            "models":[{"id":"free-model","credits":"x0.00"},{"id":"paid-model","credits":"x0.79"}],
            "accessToken":"must-not-be-cached"
        }});
        let state = Arc::new(Mutex::new((StatusCode::OK, free.clone())));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/v3/config", axum::routing::get(catalog))
            .route("/models", axum::routing::get(catalog))
            .with_state(state.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let account_id = uuid::Uuid::new_v4().to_string();
        let second_id = uuid::Uuid::new_v4().to_string();
        let credentials = json!({"accountId":account_id,"accessToken":"fixture-secret","uid":"fixture-user","baseUrl":base});
        let client = reqwest::Client::new();
        list_models(AccountProvider::WorkBuddy, credentials.clone(), &client)
            .await
            .unwrap();
        assert_eq!(
            cached_free_model_ids(AccountProvider::WorkBuddy, &account_id),
            ["free-model"]
        );
        assert!(cached_free_model_ids(AccountProvider::WorkBuddyAi, &account_id).is_empty());
        assert!(cached_free_model_ids(AccountProvider::WorkBuddy, &second_id).is_empty());
        for (status, value) in [
            (StatusCode::FORBIDDEN, json!({"error":"rejected"})),
            (StatusCode::OK, json!({"code":1,"message":"unavailable"})),
            (
                StatusCode::OK,
                json!({"code":0,"data":{"agents":[{"name":"cli","models":[]}]}}),
            ),
        ] {
            *state.lock().await = (status, value);
            assert!(
                list_models(AccountProvider::WorkBuddy, credentials.clone(), &client)
                    .await
                    .is_err()
            );
            assert_eq!(
                cached_free_model_ids(AccountProvider::WorkBuddy, &account_id),
                ["free-model"]
            );
        }
        let mut paid = free.clone();
        paid["data"]["models"][0]["credits"] = json!("x0.79");
        *state.lock().await = (StatusCode::OK, paid.clone());
        list_models(AccountProvider::WorkBuddyAi, credentials.clone(), &client)
            .await
            .unwrap();
        let mut second_credentials = credentials.clone();
        second_credentials["accountId"] = json!(second_id);
        list_models(AccountProvider::WorkBuddy, second_credentials, &client)
            .await
            .unwrap();
        assert_eq!(
            cached_free_model_ids(AccountProvider::WorkBuddy, &account_id),
            ["free-model"]
        );
        list_models(AccountProvider::WorkBuddy, credentials.clone(), &client)
            .await
            .unwrap();
        assert!(cached_free_model_ids(AccountProvider::WorkBuddy, &account_id).is_empty());
        *state.lock().await = (StatusCode::OK, free);
        list_models(AccountProvider::WorkBuddy, credentials, &client)
            .await
            .unwrap();

        let kilo_credentials = json!({"_echobird_saved_account_id":account_id,"accessToken":"fixture-secret","baseUrl":base});
        *state.lock().await = (
            StatusCode::OK,
            json!({"data":[
                {"id":"kilo-free","isFree":true},
                {"id":"disabled-free","isFree":true,"policy":{"state":"disabled"}}
            ]}),
        );
        assert_eq!(
            list_models(AccountProvider::Kilo, kilo_credentials.clone(), &client)
                .await
                .unwrap(),
            ["kilo-free"]
        );
        assert_eq!(
            cached_free_model_ids(AccountProvider::Kilo, &account_id),
            ["kilo-free"]
        );
        assert_eq!(
            cached_free_model_ids(AccountProvider::WorkBuddy, &account_id),
            ["free-model"]
        );
        *state.lock().await = (StatusCode::FORBIDDEN, json!({"error":"unavailable"}));
        assert!(
            list_models(AccountProvider::Kilo, kilo_credentials.clone(), &client)
                .await
                .is_err()
        );
        assert_eq!(
            cached_free_model_ids(AccountProvider::Kilo, &account_id),
            ["kilo-free"]
        );
        *state.lock().await = (
            StatusCode::OK,
            json!({"data":[{"id":"kilo-free","isFree":false}]}),
        );
        list_models(AccountProvider::Kilo, kilo_credentials, &client)
            .await
            .unwrap();
        assert!(cached_free_model_ids(AccountProvider::Kilo, &account_id).is_empty());

        let mut cache = free_models().lock().unwrap();
        let serialized = serde_json::to_string(&*cache).unwrap();
        for secret in [
            "fixture-secret",
            "fixture-user",
            "must-not-be-cached",
            "accessToken",
            "baseUrl",
            "credits",
        ] {
            assert!(!serialized.contains(secret));
        }
        for (provider, id) in [
            (AccountProvider::WorkBuddy, &account_id),
            (AccountProvider::WorkBuddyAi, &account_id),
            (AccountProvider::WorkBuddy, &second_id),
            (AccountProvider::Kilo, &account_id),
        ] {
            cache.remove(&free_model_cache_key(provider, id));
        }
        server.abort();
    }

    #[tokio::test]
    async fn codex_forwards_custom_schema_and_history_without_router_metadata() {
        let (base, requests, server) = fixture().await;
        let original = json!({"model":"auto","input":[{"type":"custom_tool_call","call_id":"call_a","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"},{"type":"custom_tool_call_output","call_id":"call_a","output":"ok"}],"tools":[{"type":"custom","name":"apply_patch","format":{"type":"grammar","syntax":"lark","definition":"start: SOURCE"}}]});
        let chat = super::super::responses_to_chat(&original).unwrap();
        route(
            AccountProvider::Codex,
            json!({"accessToken":"fixture","baseUrl":base}),
            "gpt-test",
            chat,
            &reqwest::Client::new(),
        )
        .await
        .unwrap();
        let recorded = requests.lock().await;
        assert_eq!(recorded[0].0, "/responses");
        assert_eq!(recorded[0].2["input"], original["input"]);
        assert_eq!(recorded[0].2["tools"], original["tools"]);
        assert!(recorded[0]
            .2
            .get(super::super::tools::RESPONSES_REQUEST)
            .is_none());
        server.abort();
    }

    #[tokio::test]
    async fn responses_without_content_type_still_require_real_events_and_completion() {
        let (base, _, server) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = json!({"accessToken":"fixture","baseUrl":base});
        for streaming in [false, true] {
            let response = route(
                AccountProvider::Codex,
                credentials.clone(),
                "headerless",
                json!({"stream":streaming,"messages":[{"role":"user","content":"hello"}]}),
                &client,
            )
            .await
            .unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            if streaming {
                let text = String::from_utf8(bytes.to_vec()).unwrap();
                assert!(text.contains("data: [DONE]"));
                assert!(text.contains("\"content\":\"ok\""));
            } else {
                let response: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(response["choices"][0]["message"]["content"], "ok");
            }
        }
        let response = route(
            AccountProvider::Codex,
            credentials,
            "headerless-invalid",
            json!({"messages":[{"role":"user","content":"hello"}]}),
            &client,
        )
        .await
        .unwrap();
        assert!(axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .is_err());
        server.abort();
    }

    #[tokio::test]
    async fn antigravity_resolves_project_and_catalog_without_quota_or_onboarding() {
        let (base, requests, server) = fixture().await;
        let client = reqwest::Client::new();
        let credentials = json!({"accessToken":"fixture","baseUrl":base});
        assert_eq!(
            list_models(AccountProvider::Antigravity, credentials.clone(), &client)
                .await
                .unwrap(),
            vec!["gemini-test"]
        );
        route(
            AccountProvider::Antigravity,
            credentials,
            "gemini-test",
            json!({"stream":false,"messages":[{"role":"user","content":"hello"}]}),
            &client,
        )
        .await
        .unwrap();
        let recorded = requests.lock().await;
        assert_eq!(
            recorded.iter().map(|v| v.0.as_str()).collect::<Vec<_>>(),
            vec![
                "/v1internal:loadCodeAssist",
                "/v1internal:fetchAvailableModels",
                "/v1internal:streamGenerateContent?alt=sse"
            ]
        );
        assert_eq!(recorded[2].2["project"], "test-project");
        server.abort();
    }

    #[tokio::test]
    async fn antigravity_free_account_omits_the_inapplicable_entitlement() {
        let (base, requests, server) = fixture().await;
        let credentials =
            json!({"accessToken":"fixture","baseUrl":base,"loadBaseUrl":format!("{base}/free")});
        let models = list_models(
            AccountProvider::Antigravity,
            credentials,
            &reqwest::Client::new(),
        )
        .await
        .unwrap();
        assert_eq!(models, vec!["gemini-test"]);
        let recorded = requests.lock().await;
        assert_eq!(
            recorded
                .iter()
                .map(|request| request.0.as_str())
                .collect::<Vec<_>>(),
            vec![
                "/free/v1internal:loadCodeAssist",
                "/v1internal:fetchAvailableModels"
            ]
        );
        assert!(recorded[1].2.get("entitlement").is_none());
        assert_eq!(recorded[1].2["project"], "test-project");
        server.abort();
    }

    #[tokio::test]
    async fn quota_status_is_preserved_for_router_fallback() {
        let (base, _, server) = fixture().await;
        let error = route(
            AccountProvider::Kilo,
            json!({"accessToken":"fixture","baseUrl":base}),
            "test",
            json!({"messages":[]}),
            &reqwest::Client::new(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(error.message, "account quota");
        assert_eq!(error.error_code.as_deref(), Some("rate_limit_exceeded"));
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()[header::RETRY_AFTER], "120");
        assert!(
            response
                .extensions()
                .get::<super::super::ProxyFailureMetadata>()
                .unwrap()
                .model_scope
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], "rate_limit_exceeded");
        assert_eq!(value["error"]["type"], "rate_limit_error");
        server.abort();
    }

    #[test]
    fn quota_reset_hint_is_preserved_without_inventing_an_error_code() {
        let value = json!({"error":{"type":"usage_limit_reached","message":"Wait for reset","resets_in_seconds":90}});
        let error = upstream_error(StatusCode::TOO_MANY_REQUESTS, &HeaderMap::new(), &value);
        assert_eq!(error.retry_after.unwrap(), "90");
        assert!(error.error_code.is_none());
        assert_eq!(error.error_type.as_deref(), Some("usage_limit_reached"));
        assert!(!error.model_scope);
        let error = upstream_error(
            StatusCode::TOO_MANY_REQUESTS,
            &HeaderMap::new(),
            &json!({"error":{"status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"3.25s"}]}}),
        );
        assert_eq!(error.retry_after.unwrap(), "4");
        assert_eq!(error.error_code.as_deref(), Some("RESOURCE_EXHAUSTED"));
    }

    #[tokio::test]
    async fn structured_stream_failure_remains_in_the_body_error_chain() {
        use std::error::Error;
        let (base, _, server) = fixture().await;
        for streaming in [false, true] {
            let response = route(
                AccountProvider::Codex,
                json!({"accessToken":"fixture","baseUrl":base}),
                "quota-stream",
                json!({"stream":streaming,"messages":[]}),
                &reqwest::Client::new(),
            )
            .await
            .unwrap();
            let error = response
                .into_body()
                .into_data_stream()
                .next()
                .await
                .unwrap()
                .unwrap_err();
            let mut cause: &(dyn Error + 'static) = &error;
            let metadata = loop {
                if let Some(metadata) = cause.downcast_ref::<ProxyError>() {
                    break metadata;
                }
                cause = cause
                    .downcast_ref::<std::io::Error>()
                    .and_then(|error| error.get_ref())
                    .map(|error| error as &(dyn Error + 'static))
                    .or_else(|| cause.source())
                    .expect("structured error remains attached to the body");
            };
            assert_eq!(metadata.status, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(metadata.retry_after.as_ref().unwrap(), "90");
            assert_eq!(metadata.error_code.as_deref(), Some("rate_limit_exceeded"));
            assert!(metadata.model_scope);
        }
        server.abort();
    }

    #[tokio::test]
    async fn chat_entry_rejects_responses_sentinel_and_invalid_terminal() {
        let (base, requests, server) = fixture().await;
        for provider in [AccountProvider::Codex, AccountProvider::Copilot] {
            for streaming in [false, true] {
                for model in ["done-only", "fake-completed"] {
                    let response = route(
                        provider,
                        json!({"accessToken":"fixture","baseUrl":base,"protocol":"responses"}),
                        model,
                        json!({"stream":streaming,"messages":[]}),
                        &reqwest::Client::new(),
                    )
                    .await
                    .unwrap();
                    let mut body = response.into_body().into_data_stream();
                    let mut returned = Vec::new();
                    let mut failure = None;
                    while let Some(chunk) = body.next().await {
                        match chunk {
                            Ok(bytes) => returned.extend_from_slice(&bytes),
                            Err(error) => {
                                failure = Some(error);
                                break;
                            }
                        }
                    }
                    assert!(
                        failure.is_some(),
                        "{provider:?} {model} incorrectly completed"
                    );
                    let text = String::from_utf8(returned).unwrap();
                    assert!(!text.contains("[DONE]"));
                    if streaming {
                        assert!(
                            text.contains(if model == "done-only" {
                                "partial"
                            } else {
                                "ok"
                            }),
                            "positive control: upstream text must have reached the Chat stream"
                        );
                    }
                }
            }
        }
        assert!(requests
            .lock()
            .await
            .iter()
            .all(|request| request.0 == "/responses"));
        server.abort();
    }

    #[tokio::test]
    async fn chat_entry_responses_completed_and_incomplete_keep_success_and_length() {
        let (base, requests, server) = fixture().await;
        for provider in [AccountProvider::Codex, AccountProvider::Copilot] {
            for streaming in [false, true] {
                for (model, reason) in [
                    ("terminal-completed", "stop"),
                    ("terminal-incomplete", "length"),
                ] {
                    let response = route(
                        provider,
                        json!({"accessToken":"fixture","baseUrl":base,"protocol":"responses"}),
                        model,
                        json!({"stream":streaming,"messages":[]}),
                        &reqwest::Client::new(),
                    )
                    .await
                    .unwrap();
                    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                        .await
                        .unwrap();
                    if streaming {
                        let mut parser = SseParser::default();
                        let frames = parser.push(&bytes);
                        assert!(frames.iter().any(|(_, data)| data == "[DONE]"));
                        assert!(frames
                            .iter()
                            .filter_map(|(_, data)| serde_json::from_str::<Value>(data).ok())
                            .any(|value| value["choices"][0]["finish_reason"] == reason));
                    } else {
                        let value: Value = serde_json::from_slice(&bytes).unwrap();
                        assert_eq!(value["choices"][0]["message"]["content"], "ok");
                        assert_eq!(value["choices"][0]["finish_reason"], reason);
                    }
                }
            }
        }
        assert!(requests
            .lock()
            .await
            .iter()
            .all(|request| request.0 == "/responses"));
        server.abort();
    }

    #[tokio::test]
    async fn chat_entry_responses_nested_terminal_errors_keep_typed_account_failures() {
        use std::error::Error;
        let (base, _, server) = fixture().await;
        for provider in [AccountProvider::Codex, AccountProvider::Copilot] {
            for streaming in [false, true] {
                for (model, status, code, retry_after, model_scope) in [
                    (
                        "terminal-quota",
                        StatusCode::TOO_MANY_REQUESTS,
                        "rate_limit_exceeded",
                        Some("90"),
                        true,
                    ),
                    (
                        "terminal-auth",
                        StatusCode::UNAUTHORIZED,
                        "authentication_error",
                        None,
                        false,
                    ),
                ] {
                    let response = route(
                        provider,
                        json!({"accessToken":"fixture","baseUrl":base,"protocol":"responses"}),
                        model,
                        json!({"stream":streaming,"messages":[]}),
                        &reqwest::Client::new(),
                    )
                    .await
                    .unwrap();
                    let mut body = response.into_body().into_data_stream();
                    let mut returned = Vec::new();
                    let error = loop {
                        match body
                            .next()
                            .await
                            .expect("the invalid terminal must fail the body")
                        {
                            Ok(bytes) => returned.extend_from_slice(&bytes),
                            Err(error) => break error,
                        }
                    };
                    assert!(!String::from_utf8(returned).unwrap().contains("[DONE]"));
                    let mut cause: &(dyn Error + 'static) = &error;
                    let metadata = loop {
                        if let Some(metadata) = cause.downcast_ref::<ProxyError>() {
                            break metadata;
                        }
                        cause = cause
                            .downcast_ref::<std::io::Error>()
                            .and_then(|error| error.get_ref())
                            .map(|error| error as &(dyn Error + 'static))
                            .or_else(|| cause.source())
                            .expect("terminal error retains its typed metadata");
                    };
                    assert_eq!(metadata.status, status);
                    assert_eq!(metadata.error_code.as_deref(), Some(code));
                    assert_eq!(
                        metadata
                            .retry_after
                            .as_ref()
                            .and_then(|value| value.to_str().ok()),
                        retry_after
                    );
                    assert_eq!(metadata.model_scope, model_scope);
                }
            }
        }
        server.abort();
    }

    #[tokio::test]
    async fn nonstream_headers_return_before_generation_and_dropping_body_cancels_upstream() {
        use std::time::Duration;
        let canceled = Arc::new(tokio::sync::Notify::new());
        let observed = canceled.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new().fallback(move || {
            let observed = observed.clone();
            async move {
                let (tx, rx) = mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(1);
                tokio::spawn(async move {
                    tx.closed().await;
                    observed.notify_one();
                });
                stream_response(Body::from_stream(ReceiverStream::new(rx)))
            }
        });
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let response = tokio::time::timeout(
            Duration::from_secs(5),
            route(
                AccountProvider::Codex,
                json!({"accessToken":"fixture","baseUrl":base}),
                "test",
                json!({"stream":false,"messages":[]}),
                &reqwest::Client::new(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
        drop(response);
        tokio::time::timeout(Duration::from_secs(5), canceled.notified())
            .await
            .unwrap();
        server.abort();
    }

    mod native_responses {
        use super::*;

        fn final_response(status: &str) -> Value {
            json!({"id":"resp_upstream","object":"response","created_at":42,"status":status,
                "output":[{"id":"reasoning_upstream","type":"reasoning","encrypted_content":"opaque"},
                    {"id":"message_upstream","type":"message","role":"assistant","content":[{"type":"output_text","text":"原始结果","annotations":[{"type":"url_citation","url":"https://example.invalid/","title":"source"}]}]}],
                "usage":{"input_tokens":3,"output_tokens":7,"output_tokens_details":{"reasoning_tokens":2}},
                "incomplete_details":if status == "incomplete" {json!({"reason":"content_filter"})} else {Value::Null},
                "future_metadata":{"kept":true}})
        }

        fn events(status: &str) -> String {
            format!(": upstream comment\r\nid: upstream-sequence\r\nevent: response.created\r\ndata: {{\"type\":\"response.created\",\"sequence_number\":0,\"response\":{{\"id\":\"resp_upstream\"}}}}\r\n\r\nevent: future.event\r\ndata: {{\"type\":\"future.event\",\"extension\":true}}\r\n\r\nevent: response.{status}\r\ndata: {{\"type\":\"response.{status}\",\"sequence_number\":9,\"response\":{}}}\r\n\r\n", final_response(status))
        }

        async fn upstream(State(requests): State<Requests>, request: Request) -> Response {
            let path = request.uri().to_string();
            let headers = request.headers().clone();
            let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            requests
                .lock()
                .await
                .push((path.clone(), headers, body.clone()));
            if path == "/models" {
                return json_response(&json!({"data":[
                    {"id":"both","supported_endpoints":["/chat/completions","/responses"]},
                    {"id":"chat-only","supported_endpoints":["/chat/completions"]},
                    {"id":"blocked","supported_endpoints":["/responses"],"policy":{"state":"disabled"}}
                ]}));
            }
            assert_eq!(path, "/responses");
            let mut response = match body["model"].as_str().unwrap_or_default() {
                "bad-content" => Response::builder().header(header::CONTENT_TYPE, "text/plain").body(Body::from("not a Responses body")).unwrap(),
                "headerless" => Response::new(Body::from(events("completed"))),
                "incomplete" => stream_response(Body::from(events("incomplete"))),
                "truncated" => stream_response(Body::from("event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"unfinished\"}\n\n")),
                "failed" => stream_response(Body::from("event: response.failed\ndata: {\"response\":{\"error\":{\"code\":\"rate_limit_exceeded\",\"type\":\"usage_limit_reached\",\"message\":\"wait\",\"scope\":\"model\",\"resets_in_seconds\":90}}}\n\n")),
                "json-unexpected" => json_response(&final_response("completed")),
                "both" if body["stream"] == false => Response::builder().header(header::CONTENT_TYPE, "application/json").body(Body::from(format!("  {}\n", final_response("completed")))).unwrap(),
                _ => stream_response(Body::from(events("completed"))),
            };
            response
                .headers_mut()
                .insert("x-request-id", HeaderValue::from_static("req_upstream"));
            response
        }

        async fn fixture() -> (String, Requests, tokio::task::JoinHandle<()>) {
            let requests = Arc::new(Mutex::new(Vec::new()));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .fallback(upstream)
                .with_state(requests.clone());
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (base, requests, server)
        }

        #[tokio::test]
        async fn codex_keeps_native_payload_events_and_terminal_response() {
            let (base, requests, server) = fixture().await;
            let client = reqwest::Client::new();
            for streaming in [true, false] {
                let original = json!({"model":"auto","stream":streaming,"store":true,"input":[{"type":"reasoning","encrypted_content":"earlier"}],
                    "tools":[{"type":"web_search"}],"previous_response_id":"resp_previous","include":["custom.include"],"metadata":{"original":true}});
                let response = route_responses(
                    AccountProvider::Codex,
                    json!({"accessToken":"fixture","accountId":"own-account","baseUrl":base}),
                    "native",
                    original.clone(),
                    &client,
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(response.headers()["x-request-id"], "req_upstream");
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                if streaming {
                    assert_eq!(bytes.as_ref(), events("completed").as_bytes());
                } else {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes).unwrap(),
                        final_response("completed")
                    );
                }
                let recorded = requests.lock().await;
                let sent = recorded.last().unwrap();
                assert_eq!(sent.0, "/responses");
                assert_eq!(sent.1["chatgpt-account-id"], "own-account");
                let mut expected = original;
                expected["model"] = json!("native");
                expected["stream"] = json!(true);
                expected["store"] = json!(false);
                expected["instructions"] = json!("");
                assert_eq!(sent.2, expected);
            }
            server.abort();
        }

        #[tokio::test]
        async fn copilot_prefers_catalog_responses_and_preserves_json_and_request_fields() {
            let (base, requests, server) = fixture().await;
            let client = reqwest::Client::new();
            let credentials =
                json!({"accessToken":"fixture","baseUrl":base,"modelProtocols":{"both":"chat"}});
            let original = json!({"model":"auto","stream":false,"store":true,"previous_response_id":"resp_previous","input":"continue","tools":[{"type":"web_search"}],"metadata":{"kept":true}});
            let response = route_responses(
                AccountProvider::Copilot,
                credentials.clone(),
                "both",
                original.clone(),
                &client,
            )
            .await
            .unwrap()
            .unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert_eq!(
                bytes.as_ref(),
                format!("  {}\n", final_response("completed")).as_bytes()
            );
            for model in ["chat-only", "blocked"] {
                assert!(route_responses(
                    AccountProvider::Copilot,
                    credentials.clone(),
                    model,
                    original.clone(),
                    &client
                )
                .await
                .unwrap()
                .is_none());
            }
            let recorded = requests.lock().await;
            assert_eq!(
                recorded
                    .iter()
                    .filter(|call| call.0 == "/responses")
                    .count(),
                1
            );
            let mut expected = original;
            expected["model"] = json!("both");
            assert_eq!(recorded[1].2, expected);
            server.abort();
        }

        #[tokio::test]
        async fn streaming_json_is_rejected_and_response_credentials_are_removed() {
            let (base, _, server) = fixture().await;
            let error = route_responses(
                AccountProvider::Codex,
                json!({"accessToken":"fixture","baseUrl":base}),
                "json-unexpected",
                json!({"stream":true,"input":"hello"}),
                &reqwest::Client::new(),
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_GATEWAY);
            assert_eq!(
                responses_response(json_response(&final_response("completed")), true)
                    .unwrap_err()
                    .status,
                StatusCode::BAD_GATEWAY
            );
            for streaming in [false, true] {
                let mut response = stream_response(Body::from(events("completed")));
                response.headers_mut().append(
                    header::SET_COOKIE,
                    HeaderValue::from_static("session=fixture"),
                );
                response.headers_mut().append(
                    header::SET_COOKIE,
                    HeaderValue::from_static("other=fixture"),
                );
                for name in [
                    "authorization",
                    "proxy-authorization",
                    "cookie",
                    "keep-alive",
                    "upgrade",
                    "x-hop-secret",
                ] {
                    response
                        .headers_mut()
                        .insert(name, HeaderValue::from_static("fixture"));
                }
                response.headers_mut().insert(
                    header::CONNECTION,
                    HeaderValue::from_static("keep-alive, x-hop-secret"),
                );
                response
                    .headers_mut()
                    .insert("x-request-id", HeaderValue::from_static("req_original"));
                response
                    .headers_mut()
                    .insert("x-ratelimit-remaining", HeaderValue::from_static("7"));
                let response = responses_response(response, streaming).unwrap();
                for name in [
                    "set-cookie",
                    "authorization",
                    "proxy-authorization",
                    "cookie",
                    "connection",
                    "keep-alive",
                    "upgrade",
                    "x-hop-secret",
                ] {
                    assert!(
                        !response.headers().contains_key(name),
                        "{name} must not be relayed"
                    );
                }
                assert_eq!(response.headers()["x-request-id"], "req_original");
                assert_eq!(response.headers()["x-ratelimit-remaining"], "7");
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                if streaming {
                    assert_eq!(bytes.as_ref(), events("completed").as_bytes());
                } else {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes).unwrap(),
                        final_response("completed")
                    );
                }
            }
            server.abort();
        }

        #[tokio::test]
        async fn failed_http_response_keeps_status_body_and_retry_metadata() {
            for streaming in [false, true] {
                let original = json!({"error":{"code":"rate_limit_exceeded","message":"Wait"}});
                let mut response = json_response(&original);
                *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, HeaderValue::from_static("120"));
                response.headers_mut().append(
                    header::SET_COOKIE,
                    HeaderValue::from_static("session=fixture"),
                );
                let response = responses_response(response, streaming).unwrap();
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(response.headers()[header::RETRY_AFTER], "120");
                assert!(!response.headers().contains_key(header::SET_COOKIE));
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), original);
            }
        }

        #[tokio::test]
        async fn unsupported_account_protocol_does_not_contact_upstream() {
            let (base, requests, server) = fixture().await;
            for provider in [
                AccountProvider::ClaudeCode,
                AccountProvider::WorkBuddy,
                AccountProvider::WorkBuddyAi,
                AccountProvider::Antigravity,
                AccountProvider::Kilo,
            ] {
                assert!(route_responses(
                    provider,
                    json!({"accessToken":"fixture","baseUrl":base}),
                    "responses-named-model",
                    json!({"input":"hello"}),
                    &reqwest::Client::new()
                )
                .await
                .unwrap()
                .is_none());
            }
            assert!(requests.lock().await.is_empty());
            server.abort();
        }

        #[tokio::test]
        async fn native_stream_keeps_failure_events_while_nonstream_retains_typed_failure() {
            use std::error::Error;
            let (base, _, server) = fixture().await;
            let credentials = json!({"accessToken":"fixture","baseUrl":base});
            let response = route_responses(
                AccountProvider::Codex,
                credentials.clone(),
                "failed",
                json!({"stream":true,"input":"hello"}),
                &reqwest::Client::new(),
            )
            .await
            .unwrap()
            .unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert!(String::from_utf8(bytes.to_vec())
                .unwrap()
                .starts_with("event: response.failed\n"));
            let response = route_responses(
                AccountProvider::Codex,
                credentials,
                "failed",
                json!({"stream":false,"input":"hello"}),
                &reqwest::Client::new(),
            )
            .await
            .unwrap()
            .unwrap();
            let error = response
                .into_body()
                .into_data_stream()
                .next()
                .await
                .unwrap()
                .unwrap_err();
            let mut cause: &(dyn Error + 'static) = &error;
            let metadata = loop {
                if let Some(metadata) = cause.downcast_ref::<ProxyError>() {
                    break metadata;
                }
                cause = cause
                    .downcast_ref::<std::io::Error>()
                    .and_then(|error| error.get_ref())
                    .map(|error| error as &(dyn Error + 'static))
                    .or_else(|| cause.source())
                    .unwrap();
            };
            assert_eq!(metadata.status, StatusCode::TOO_MANY_REQUESTS);
            assert_eq!(metadata.retry_after.as_ref().unwrap(), "90");
            assert!(metadata.model_scope);
            server.abort();
        }

        #[tokio::test]
        async fn nonstream_incomplete_is_preserved_and_truncation_is_not_completion() {
            let (base, _, server) = fixture().await;
            for model in ["incomplete", "headerless", "truncated"] {
                let response = route_responses(
                    AccountProvider::Codex,
                    json!({"accessToken":"fixture","baseUrl":base}),
                    model,
                    json!({"input":"hello"}),
                    &reqwest::Client::new(),
                )
                .await
                .unwrap()
                .unwrap();
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024).await;
                if model == "truncated" {
                    assert!(bytes.is_err());
                } else {
                    let status = if model == "incomplete" {
                        "incomplete"
                    } else {
                        "completed"
                    };
                    assert_eq!(
                        serde_json::from_slice::<Value>(&bytes.unwrap()).unwrap(),
                        final_response(status)
                    );
                }
            }
            assert_eq!(
                route_responses(
                    AccountProvider::Codex,
                    json!({"accessToken":"fixture","baseUrl":base}),
                    "bad-content",
                    json!({"input":"hello"}),
                    &reqwest::Client::new()
                )
                .await
                .unwrap_err()
                .status,
                StatusCode::BAD_GATEWAY
            );
            server.abort();
        }

        #[tokio::test]
        async fn nonstream_terminal_response_must_match_its_event_and_keep_valid_incomplete() {
            for status in ["completed", "incomplete"] {
                let original = final_response(status);
                let kind = format!("response.{status}");
                let value = json!({"type":kind,"response":original});
                let response = responses_response(
                    stream_response(Body::from(format!("event: {kind}\ndata: {value}\n\n"))),
                    false,
                )
                .unwrap();
                let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), original);
            }
            for terminal in [
                Value::Null,
                json!({}),
                json!({"id":"resp_fixture","object":"chat.completion","status":"completed"}),
                json!({"id":"resp_fixture","object":"response","status":"in_progress"}),
                json!({"id":"resp_fixture","object":"response","status":"failed"}),
                json!({"id":"resp_fixture","object":"response","status":"incomplete"}),
                json!({"id":"","object":"response","status":"completed"}),
                json!({"object":"response","status":"completed"}),
                json!({"id":"resp_fixture","object":"response","status":"completed","error":{"code":"rate_limit_exceeded","type":"usage_limit_reached","message":"wait","resets_in_seconds":90}}),
            ] {
                let value = json!({"type":"response.completed","response":terminal});
                let response = responses_response(
                    stream_response(Body::from(format!(
                        "event: response.completed\ndata: {value}\n\n"
                    ))),
                    false,
                )
                .unwrap();
                assert!(axum::body::to_bytes(response.into_body(), 1024 * 1024)
                    .await
                    .is_err());
            }
            let value = json!({"type":"response.completed","response":final_response("completed")});
            let response = responses_response(
                stream_response(Body::from(format!(
                    "event: response.incomplete\ndata: {value}\n\n"
                ))),
                false,
            )
            .unwrap();
            assert!(axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .is_err());
        }

        #[tokio::test]
        async fn nonstream_collector_handles_split_utf8_and_cancels_pending_source() {
            use std::time::Duration;
            let wire = events("completed");
            let response = stream_response(Body::from_stream(futures_util::stream::iter(
                wire.into_bytes()
                    .into_iter()
                    .map(|byte| Ok::<_, std::io::Error>(bytes::Bytes::copy_from_slice(&[byte]))),
            )));
            let response = responses_response(response, false).unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&bytes).unwrap(),
                final_response("completed")
            );

            let (tx, rx) = mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(1);
            let response = responses_response(
                stream_response(Body::from_stream(ReceiverStream::new(rx))),
                false,
            )
            .unwrap();
            assert!(tokio::time::timeout(Duration::from_millis(20), tx.closed())
                .await
                .is_err());
            drop(response);
            tokio::time::timeout(Duration::from_secs(5), tx.closed())
                .await
                .unwrap();
        }
    }

    mod copilot {
        use super::*;
        use axum::http::Method;

        type Calls = Arc<Mutex<Vec<(Method, String, reqwest::header::HeaderMap, Value)>>>;

        fn events(path: &str, tool: bool, continued: bool) -> String {
            let text = if continued { "probe-ok" } else { "ok" };
            let call = json!({"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"echo_probe","arguments":"{\"value\":\"probe-ok\"}"});
            let frames = match path {
                "/chat/completions" if tool => vec![
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_fixture","type":"function","function":{"name":"echo_probe","arguments":"{\"value\":"}}]},"finish_reason":null}]}),
                    json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"probe-ok\"}"}}]},"finish_reason":null}]}),
                    json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
                ],
                "/chat/completions" => vec![
                    json!({"choices":[{"delta":{"content":text},"finish_reason":null}]}),
                    json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
                ],
                "/responses" if tool => vec![
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_fixture","call_id":"call_fixture","name":"echo_probe","arguments":""}}),
                    json!({"type":"response.function_call_arguments.delta","item_id":"fc_fixture","delta":"{\"value\":"}),
                    json!({"type":"response.function_call_arguments.delta","item_id":"fc_fixture","delta":"\"probe-ok\"}"}),
                    json!({"type":"response.output_item.done","output_index":0,"item":call}),
                    json!({"type":"response.completed","response":{"object":"response","id":"resp_tool_fixture","status":"completed","error":null,"output":[call],"usage":{"input_tokens":1,"output_tokens":2}}}),
                ],
                "/responses" => vec![
                    json!({"type":"response.output_text.delta","delta":text}),
                    json!({"type":"response.completed","response":{"object":"response","id":"resp_fixture","status":"completed","error":null,"usage":{"input_tokens":1,"output_tokens":2}}}),
                ],
                _ if tool => vec![
                    json!({"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_fixture","name":"echo_probe","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"value\":"}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"probe-ok\"}"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":2}}),
                    json!({"type":"message_stop"}),
                ],
                _ => vec![
                    json!({"type":"message_start","message":{"usage":{"input_tokens":1,"output_tokens":0}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
                    json!({"type":"message_stop"}),
                ],
            };
            let mut output = String::new();
            for frame in frames {
                if let Some(kind) = frame["type"].as_str() {
                    output.push_str(&format!("event: {kind}\n"));
                }
                output.push_str(&format!("data: {frame}\n\n"));
            }
            if path == "/chat/completions" {
                output.push_str("data: [DONE]\n\n");
            }
            output
        }

        async fn upstream(State(calls): State<Calls>, request: Request) -> Response {
            let method = request.method().clone();
            let path = request.uri().to_string();
            let headers = request.headers().clone();
            let bytes = axum::body::to_bytes(request.into_body(), 1024 * 1024)
                .await
                .unwrap();
            let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
            calls
                .lock()
                .await
                .push((method.clone(), path.clone(), headers, body.clone()));
            if method == Method::GET && path == "/models" {
                return json_response(&json!({"data":[
                    {"id":"chat-test","supported_endpoints":["/chat/completions","/responses","/v1/messages"]},
                    {"id":"responses-test","supported_endpoints":["/responses"]},
                    {"id":"messages-test","supported_endpoints":["/v1/messages"]},
                    {"id":"messages-alias-test","supported_endpoints":["/messages"]},
                    {"id":"disabled","supported_endpoints":["/chat/completions"],"policy":{"state":"disabled"}},
                    {"id":"hidden","supported_endpoints":["/responses"],"visibility":"hide"},
                    {"id":"unpicked","supported_endpoints":["/responses"],"model_picker_enabled":false,"policy":{"state":"enabled"}},
                    {"id":"legacy-chat","capabilities":{"type":"chat"},"model_picker_enabled":false,"policy":{"state":"enabled"}},
                    {"id":"legacy-embedding","capabilities":{"type":"embeddings"}},
                    {"id":"legacy-unknown"},
                    {"id":"legacy-disabled","capabilities":{"type":"chat"},"policy":{"state":"disabled"}},
                    {"id":"empty-endpoints","capabilities":{"type":"chat"},"supported_endpoints":[]},
                    {"id":"api-disabled","supported_endpoints":["/responses"],"supported_in_api":false},
                    {"id":"unsupported","supported_endpoints":["/embeddings"]}
                ]}));
            }
            let expected = match body["model"].as_str() {
                Some("chat-test" | "legacy-chat") => "/chat/completions",
                Some("responses-test" | "unpicked") => "/responses",
                Some("messages-test") => "/v1/messages",
                Some("messages-alias-test") => "/messages",
                _ => "",
            };
            if method != Method::POST || path != expected || expected.is_empty() {
                let mut response =
                    json_response(&json!({"error":{"message":"Unexpected fixture endpoint"}}));
                *response.status_mut() = StatusCode::BAD_REQUEST;
                return response;
            }
            let continued = body["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|message| {
                    message["role"] == "tool"
                        || message["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|part| part["type"] == "tool_result")
                })
                || body["input"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|item| item["type"] == "function_call_output");
            let tool = !continued
                && body["tools"]
                    .as_array()
                    .is_some_and(|tools| !tools.is_empty());
            stream_response(Body::from(events(&path, tool, continued)))
        }

        async fn fixture() -> (Value, Calls, tokio::task::JoinHandle<()>) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new().fallback(upstream).with_state(calls.clone());
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (
                json!({"accessToken":"fixture-copilot","baseUrl":base,"headers":{"Editor-Version":"copilot/1.0.95","Copilot-Integration-Id":"copilot-developer-cli","X-GitHub-Api-Version":"2026-07-01"}}),
                calls,
                server,
            )
        }

        async fn result(response: Response, streaming: bool) -> Value {
            assert_eq!(response.status(), StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
                .await
                .unwrap();
            if !streaming {
                return serde_json::from_slice::<Value>(&bytes).unwrap();
            }
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(text.contains("data: [DONE]"));
            let mut message = json!({"role":"assistant","content":""});
            let mut content = String::new();
            let mut call = json!({"id":"","type":"function","function":{"name":"","arguments":""}});
            let mut arguments = String::new();
            let mut reason = Value::Null;
            for line in text
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter(|line| *line != "[DONE]")
            {
                let chunk: Value = serde_json::from_str(line).unwrap();
                let choice = &chunk["choices"][0];
                if let Some(value) = choice["delta"]["content"].as_str() {
                    content.push_str(value);
                }
                for delta in choice["delta"]["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    assert_eq!(delta["index"], 0);
                    if let Some(id) = delta.get("id") {
                        call["id"] = id.clone();
                    }
                    if let Some(name) = delta["function"].get("name") {
                        call["function"]["name"] = name.clone();
                    }
                    if let Some(value) = delta["function"]["arguments"].as_str() {
                        arguments.push_str(value);
                    }
                }
                if !choice["finish_reason"].is_null() {
                    reason = choice["finish_reason"].clone();
                }
            }
            message["content"] = json!(content);
            if !call["id"].as_str().unwrap().is_empty() {
                call["function"]["arguments"] = json!(arguments);
                message["tool_calls"] = json!([call]);
            }
            json!({"choices":[{"message":message,"finish_reason":reason}]})
        }

        #[tokio::test]
        async fn catalog_uses_signed_get_and_filters_account_disabled_models() {
            let (credentials, calls, server) = fixture().await;
            assert_eq!(
                list_models(
                    AccountProvider::Copilot,
                    credentials,
                    &reqwest::Client::new()
                )
                .await
                .unwrap(),
                vec![
                    "chat-test",
                    "legacy-chat",
                    "messages-alias-test",
                    "messages-test",
                    "responses-test",
                    "unpicked",
                ]
            );
            let calls = calls.lock().await;
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].0, Method::GET);
            assert_eq!(calls[0].1, "/models");
            assert!(calls[0].3.is_null());
            assert_eq!(calls[0].2["authorization"], "Bearer fixture-copilot");
            assert_eq!(
                calls[0].2["copilot-integration-id"],
                "copilot-developer-cli"
            );
            assert_eq!(calls[0].2["editor-version"], "copilot/1.0.95");
            assert_eq!(calls[0].2["x-github-api-version"], "2026-07-01");
            assert_eq!(calls[0].2.get_all("editor-version").iter().count(), 1);
            server.abort();
        }

        #[tokio::test]
        async fn picker_visibility_does_not_deny_copilot_api_models() {
            let (credentials, calls, server) = fixture().await;
            let client = reqwest::Client::new();
            let response = route(
                AccountProvider::Copilot,
                credentials.clone(),
                "unpicked",
                json!({"messages":[{"role":"user","content":"hello"}]}),
                &client,
            )
            .await
            .unwrap();
            assert_eq!(
                result(response, false).await["choices"][0]["message"]["content"],
                "ok"
            );
            let recorded = calls.lock().await;
            assert_eq!(recorded.len(), 2);
            assert_eq!(recorded[0].0, Method::GET);
            assert_eq!(recorded[0].1, "/models");
            assert_eq!(recorded[1].0, Method::POST);
            assert_eq!(recorded[1].1, "/responses");
            assert_eq!(recorded[1].3["model"], "unpicked");
            drop(recorded);
            let other_provider_models = list_models(AccountProvider::Kilo, credentials, &client)
                .await
                .unwrap();
            assert!(!other_provider_models.iter().any(|id| id == "unpicked"));
            server.abort();
        }

        #[tokio::test]
        async fn catalog_selected_endpoints_stream_tools_and_continue_without_quota() {
            let (credentials, calls, server) = fixture().await;
            let client = reqwest::Client::new();
            for (model, path) in [
                ("chat-test", "/chat/completions"),
                ("legacy-chat", "/chat/completions"),
                ("responses-test", "/responses"),
                ("messages-test", "/v1/messages"),
                ("messages-alias-test", "/messages"),
            ] {
                for streaming in [false, true] {
                    let history = json!([{"role":"system","content":"Original system"},{"role":"user","content":"Call echo_probe"}]);
                    let mut body = json!({"stream":streaming,"messages":history,"max_tokens":128});
                    let text = result(
                        route(
                            AccountProvider::Copilot,
                            credentials.clone(),
                            model,
                            body.clone(),
                            &client,
                        )
                        .await
                        .unwrap(),
                        streaming,
                    )
                    .await;
                    assert_eq!(text["choices"][0]["message"]["content"], "ok");
                    body["tools"] = json!([{"type":"function","function":{"name":"echo_probe","parameters":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}}}]);
                    body["tool_choice"] =
                        json!({"type":"function","function":{"name":"echo_probe"}});
                    body["parallel_tool_calls"] = json!(false);
                    let tool = result(
                        route(
                            AccountProvider::Copilot,
                            credentials.clone(),
                            model,
                            body.clone(),
                            &client,
                        )
                        .await
                        .unwrap(),
                        streaming,
                    )
                    .await;
                    assert_eq!(tool["choices"][0]["finish_reason"], "tool_calls");
                    let message = &tool["choices"][0]["message"];
                    assert_eq!(message["tool_calls"][0]["id"], "call_fixture");
                    assert_eq!(message["tool_calls"][0]["function"]["name"], "echo_probe");
                    assert_eq!(
                        serde_json::from_str::<Value>(
                            message["tool_calls"][0]["function"]["arguments"]
                                .as_str()
                                .unwrap()
                        )
                        .unwrap(),
                        json!({"value":"probe-ok"})
                    );
                    body["messages"].as_array_mut().unwrap().extend([
                        message.clone(),
                        json!({"role":"tool","tool_call_id":"call_fixture","content":"probe-ok"}),
                    ]);
                    body["tool_choice"] = json!("auto");
                    let continuation = result(
                        route(
                            AccountProvider::Copilot,
                            credentials.clone(),
                            model,
                            body,
                            &client,
                        )
                        .await
                        .unwrap(),
                        streaming,
                    )
                    .await;
                    assert_eq!(continuation["choices"][0]["message"]["content"], "probe-ok");
                    assert_eq!(continuation["choices"][0]["finish_reason"], "stop");
                    let recorded = calls.lock().await;
                    let stages = &recorded[recorded.len() - 3..];
                    assert_eq!(
                        stages
                            .iter()
                            .map(|call| (call.0.clone(), call.1.as_str()))
                            .collect::<Vec<_>>(),
                        vec![
                            (Method::POST, path),
                            (Method::POST, path),
                            (Method::POST, path)
                        ]
                    );
                    for call in stages {
                        assert_eq!(call.2["authorization"], "Bearer fixture-copilot");
                    }
                    let request = &stages[1].3;
                    assert_eq!(request["model"], model);
                    assert_eq!(request["stream"], true);
                    match path {
                        "/chat/completions" => {
                            assert_eq!(
                                request["tool_choice"],
                                json!({"type":"function","function":{"name":"echo_probe"}})
                            );
                            assert_eq!(stages[2].3["messages"][3]["tool_call_id"], "call_fixture");
                            assert_eq!(stages[2].3["messages"][3]["content"], "probe-ok");
                        }
                        "/responses" => {
                            assert_eq!(request["tools"][0]["type"], "function");
                            assert_eq!(
                                request["tool_choice"],
                                json!({"type":"function","name":"echo_probe"})
                            );
                            let input = stages[2].3["input"].as_array().unwrap();
                            assert!(input.iter().any(|item| item["type"] == "function_call"
                                && item["call_id"] == "call_fixture"
                                && item["arguments"] == "{\"value\":\"probe-ok\"}"));
                            assert!(input
                                .iter()
                                .any(|item| item["type"] == "function_call_output"
                                    && item["call_id"] == "call_fixture"
                                    && item["output"] == "probe-ok"));
                        }
                        _ => {
                            assert_eq!(stages[1].2["anthropic-version"], "2023-06-01");
                            assert_eq!(request["system"], "Original system");
                            assert_eq!(request["max_tokens"], 128);
                            assert_eq!(
                                request["tool_choice"],
                                json!({"type":"tool","name":"echo_probe","disable_parallel_tool_use":true})
                            );
                            assert!(request["tools"][0].get("description").is_none());
                            assert_eq!(
                                stages[2].3["messages"][1]["content"][0]["type"],
                                "tool_use"
                            );
                            assert_eq!(
                                stages[2].3["messages"][1]["content"][0]["id"],
                                "call_fixture"
                            );
                            assert_eq!(
                                stages[2].3["messages"][1]["content"][0]["input"],
                                json!({"value":"probe-ok"})
                            );
                            assert_eq!(
                                stages[2].3["messages"][2]["content"][0],
                                json!({"type":"tool_result","tool_use_id":"call_fixture","content":"probe-ok"})
                            );
                        }
                    }
                }
            }
            assert_eq!(calls.lock().await.len(), 31);
            server.abort();
        }

        #[tokio::test]
        async fn missing_disabled_and_unsupported_models_never_start_inference() {
            let (credentials, calls, server) = fixture().await;
            let client = reqwest::Client::new();
            for (model, status) in [
                ("missing", StatusCode::NOT_FOUND),
                ("disabled", StatusCode::NOT_FOUND),
                ("hidden", StatusCode::NOT_FOUND),
                ("api-disabled", StatusCode::NOT_FOUND),
                ("unsupported", StatusCode::BAD_REQUEST),
                ("legacy-embedding", StatusCode::BAD_REQUEST),
                ("legacy-unknown", StatusCode::BAD_REQUEST),
                ("legacy-disabled", StatusCode::NOT_FOUND),
                ("empty-endpoints", StatusCode::BAD_REQUEST),
            ] {
                let error = route(
                    AccountProvider::Copilot,
                    credentials.clone(),
                    model,
                    json!({"messages":[]}),
                    &client,
                )
                .await
                .unwrap_err();
                assert_eq!(error.status, status);
            }
            let calls = calls.lock().await;
            assert_eq!(calls.len(), 1);
            assert!(calls
                .iter()
                .all(|call| call.0 == Method::GET && call.1 == "/models"));
            server.abort();
        }

        #[test]
        fn anthropic_tool_choices_preserve_forcing_and_parallel_contract() {
            for (choice, expected) in [
                (
                    json!("auto"),
                    json!({"type":"auto","disable_parallel_tool_use":true}),
                ),
                (
                    json!("required"),
                    json!({"type":"any","disable_parallel_tool_use":true}),
                ),
                (json!("none"), json!({"type":"none"})),
            ] {
                assert_eq!(
                    anthropic_request(
                        &json!({"messages":[],"tool_choice":choice,"parallel_tool_calls":false}),
                        "messages-test"
                    )["tool_choice"],
                    expected
                );
            }
        }
    }
}

#[cfg(test)]
mod commandcode_tests {
    use super::*;
    #[tokio::test]
    async fn commandcode_uses_its_api_key_and_openai_chat_endpoint() {
        let provider = AccountProvider::CommandCode;
        let credentials = json!({"accessToken":"fixture-commandcode-key"});
        let client = reqwest::Client::new();
        assert_eq!(
            base(provider, &credentials),
            "https://api.commandcode.ai/provider/v1"
        );
        let request = signed(
            client.post(format!("{}/chat/completions", base(provider, &credentials))),
            provider,
            &credentials,
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(
            request.headers()["authorization"],
            "Bearer fixture-commandcode-key"
        );
        assert_eq!(request.headers()["x-api-key"], "fixture-commandcode-key");
        let (protocol, endpoint) = wire(provider, &credentials, "fixture-model", &client)
            .await
            .unwrap();
        assert_eq!(protocol, WireProtocol::Chat);
        assert_eq!(endpoint, "/chat/completions");
    }
}
