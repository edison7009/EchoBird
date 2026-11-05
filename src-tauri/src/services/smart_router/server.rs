use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(test)]
use tokio_stream::wrappers::ReceiverStream;

use axum::body::{Body, Bytes};
use axum::extract::{rejection::JsonRejection, DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use futures_util::{stream, StreamExt};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{PublicActivity, SMART_ROUTER_INTERNAL_ID, SMART_ROUTER_MODEL_ID};
use crate::models::model::AccountProvider;
use crate::services::model_manager;

#[path = "zen_free.rs"]
mod zen_free;

#[path = "standby.rs"]
mod standby;
#[path = "standby_probe.rs"]
mod standby_probe;
#[cfg(test)]
#[path = "standby_route_tests.rs"]
mod standby_route_tests;
#[cfg(test)]
#[path = "statistics_integration_tests.rs"]
mod statistics_integration_tests;
#[cfg(test)]
#[path = "switch_tests.rs"]
mod switch_tests;

#[path = "probe.rs"]
mod probe;
pub(crate) use probe::{cancel_probe, probe_candidate, ProbeResult};

#[path = "voice.rs"]
mod voice;

#[path = "content_protection.rs"]
mod content_protection;

const MAX_REQUEST_BODY_BYTES: usize = 64 * 1024 * 1024;
const MAX_STREAM_PREFLIGHT_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BINDINGS: usize = 512;
const RESPONSE_BINDING_LIFETIME_MS: u64 = 24 * 60 * 60 * 1_000;
const RESPONSES_CAPABILITY_LIFETIME_MS: u64 = 5 * 60 * 1_000;
const MAX_ROUTE_ATTEMPTS: usize = 20;
const FALLBACK_TIME_BUDGET: Duration = Duration::from_secs(45);
const UPSTREAM_HEADER_TIMEOUT: Duration = Duration::from_secs(30);
const UPSTREAM_FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(120);
const UPSTREAM_FIRST_CONTENT_TIMEOUT: Duration = Duration::from_secs(20);
const UPSTREAM_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const UPSTREAM_FINISH_ERROR: &str = "upstream_finish_error";
const TRANSIENT_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_TRANSIENT_COOLDOWN: Duration = Duration::from_secs(5 * 60);
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(60);
const AUTH_OR_BILLING_COOLDOWN: Duration = Duration::from_secs(60 * 60);
const MODEL_UNAVAILABLE_COOLDOWN: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

static ROUTE_MEMORY: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<RouteMemory>>>>> = OnceLock::new();
static ROUTE_MEMORY_WRITERS: OnceLock<Mutex<HashMap<PathBuf, u64>>> = OnceLock::new();
static ROUTE_ACTIVITY: OnceLock<Arc<Mutex<RouteActivity>>> = OnceLock::new();
static GROUP_ACTIVITY: OnceLock<Mutex<HashMap<String, Arc<Mutex<RouteActivity>>>>> =
    OnceLock::new();
static GROUP_MEMORY: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<ResponseMemory>>>>> =
    OnceLock::new();

fn shared_activity() -> Arc<Mutex<RouteActivity>> {
    let id = super::profiles::current_id();
    if id == SMART_ROUTER_INTERNAL_ID {
        return ROUTE_ACTIVITY
            .get_or_init(|| Arc::new(Mutex::new(RouteActivity::default())))
            .clone();
    }
    GROUP_ACTIVITY
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .entry(id)
        .or_insert_with(|| Arc::new(Mutex::new(RouteActivity::default())))
        .clone()
}

#[derive(Default)]
struct RouteActivity {
    next_attempt_id: u64,
    active_attempts: BTreeMap<u64, String>,
    last_candidate_id: Option<String>,
    sequence: u64,
    updated_at_ms: u64,
}

impl RouteActivity {
    fn begin(&mut self, candidate_id: &str, updated_at_ms: u64) -> u64 {
        self.next_attempt_id = self.next_attempt_id.saturating_add(1);
        let attempt_id = self.next_attempt_id;
        self.active_attempts
            .insert(attempt_id, candidate_id.to_string());
        self.last_candidate_id = Some(candidate_id.to_string());
        self.sequence = self.sequence.saturating_add(1);
        self.updated_at_ms = updated_at_ms;
        attempt_id
    }

    fn end(&mut self, attempt_id: u64, updated_at_ms: u64) {
        if self.active_attempts.remove(&attempt_id).is_none() {
            return;
        }
        if let Some((_, candidate_id)) = self.active_attempts.last_key_value() {
            self.last_candidate_id = Some(candidate_id.clone());
        }
        self.sequence = self.sequence.saturating_add(1);
        self.updated_at_ms = updated_at_ms;
    }

    fn snapshot(&self) -> PublicActivity {
        PublicActivity {
            candidate_id: self.last_candidate_id.clone(),
            active: !self.active_attempts.is_empty(),
            active_candidate_ids: self
                .active_attempts
                .values()
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            sequence: self.sequence,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

struct RouteActivityGuard {
    attempt_id: Option<u64>,
    activity: Arc<Mutex<RouteActivity>>,
}

impl RouteActivityGuard {
    #[cfg(test)]
    fn begin(candidate_id: &str) -> Self {
        Self::begin_for(candidate_id, shared_activity())
    }
    fn begin_for(candidate_id: &str, activity: Arc<Mutex<RouteActivity>>) -> Self {
        let attempt_id = activity
            .lock()
            .ok()
            .map(|mut activity| activity.begin(candidate_id, now_ms()));
        Self {
            attempt_id,
            activity,
        }
    }
}

impl Drop for RouteActivityGuard {
    fn drop(&mut self) {
        let Some(attempt_id) = self.attempt_id else {
            return;
        };
        if let Ok(mut activity) = self.activity.lock() {
            activity.end(attempt_id, now_ms());
        }
    }
}

pub(crate) fn public_activity() -> PublicActivity {
    shared_activity()
        .lock()
        .map(|activity| activity.snapshot())
        .unwrap_or(PublicActivity {
            candidate_id: None,
            active: false,
            active_candidate_ids: Vec::new(),
            sequence: 0,
            updated_at_ms: 0,
        })
}

#[derive(Clone)]
struct AppState {
    request_statistics: Option<Arc<super::statistics::Request>>,
    statistics_collector: Option<Arc<super::statistics::Collector>>,
    config_path: PathBuf,
    protection_settings_path: PathBuf,
    model_id: String,
    activity: Arc<Mutex<RouteActivity>>,
    http_client: reqwest::Client,
    route_memory: Arc<Mutex<RouteMemory>>,
    route_memory_path: Option<PathBuf>,
    response_memory: Arc<Mutex<ResponseMemory>>,
    response_memory_path: Option<PathBuf>,
    standby: Arc<standby::Standby>,
    probe_pool: Arc<standby::ProbePool>,
    shutdown: tokio_util::sync::CancellationToken,
    request_epoch: Option<u64>,
    #[cfg(test)]
    fixture_responses: Option<Arc<Mutex<HashMap<String, Response>>>>,
    #[cfg(test)]
    fixture_authentication: Option<Arc<Mutex<AuthenticationFixture>>>,
}

#[cfg(test)]
struct AuthenticationFixture {
    result: Result<Value, String>,
    delay: Duration,
    rejections: Vec<(crate::models::model::AccountSource, Value)>,
    retry_responses: std::collections::VecDeque<Response>,
    sent_credentials: Vec<Value>,
}

impl AppState {
    fn supports_model(&self, model: &str) -> bool {
        model == self.model_id
            || (self.model_id == SMART_ROUTER_MODEL_ID && is_supported_router_model(model))
    }
    fn new() -> Result<Self, String> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        let config_path = super::config_path();
        let standby = Arc::new(standby::Standby::new());
        standby::register(&config_path, &standby);
        Ok(Self {
            request_statistics: None,
            statistics_collector: None,
            config_path,
            protection_settings_path: crate::utils::platform::echobird_dir().join("settings.json"),
            model_id: SMART_ROUTER_MODEL_ID.into(),
            activity: shared_activity(),
            http_client,
            route_memory: shared_route_memory(),
            route_memory_path: Some(route_memory_path()),
            response_memory: shared_response_memory(),
            response_memory_path: Some(response_memory_path()),
            standby,
            probe_pool: standby::shared_probes(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            request_epoch: None,
            #[cfg(test)]
            fixture_responses: None,
            #[cfg(test)]
            fixture_authentication: None,
        })
    }

    #[cfg(test)]
    fn for_tests() -> Result<Self, String> {
        let http_client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("reqwest client build failed: {e}"))?;
        Ok(Self {
            request_statistics: None,
            statistics_collector: None,
            config_path: super::config_path(),
            protection_settings_path: std::env::temp_dir()
                .join(format!("router-settings-{}.json", uuid::Uuid::new_v4())),
            model_id: SMART_ROUTER_MODEL_ID.into(),
            activity: shared_activity(),
            http_client,
            route_memory: Arc::new(Mutex::new(RouteMemory::default())),
            route_memory_path: None,
            response_memory: Arc::new(Mutex::new(ResponseMemory::default())),
            response_memory_path: None,
            standby: Arc::new(standby::Standby::disabled()),
            probe_pool: Arc::new(standby::ProbePool::default()),
            shutdown: tokio_util::sync::CancellationToken::new(),
            request_epoch: None,
            fixture_responses: None,
            fixture_authentication: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RouteMemory {
    #[serde(default = "route_memory_version")]
    version: u32,
    #[serde(default)]
    candidates: HashMap<String, CandidateHealth>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    accounts: HashMap<String, CandidateHealth>,
    #[serde(default)]
    revision: u64,
    #[serde(skip)]
    credential_versions: HashMap<String, (String, u64)>,
    #[serde(skip)]
    account_epochs: HashMap<String, u64>,
    #[serde(skip)]
    candidate_epochs: HashMap<String, u64>,
    #[serde(skip)]
    recovered_revisions: HashMap<String, u64>,
    #[serde(skip)]
    recovering: HashMap<String, Weak<AttemptHealth>>,
    #[serde(skip)]
    credential_refreshes: HashMap<String, Arc<CredentialRefresh>>,
    #[serde(skip)]
    unsupported_responses: HashMap<String, u64>,
    #[serde(skip)]
    zen_refused_until: HashMap<String, u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseMemory {
    #[serde(default)]
    response_bindings: HashMap<String, ResponseBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseBinding {
    candidate_id: String,
    fingerprint: String,
    expires_at_ms: u64,
    #[serde(default)]
    ambiguous: bool,
}

impl Default for RouteMemory {
    fn default() -> Self {
        Self {
            version: route_memory_version(),
            candidates: HashMap::new(),
            accounts: HashMap::new(),
            revision: 0,
            credential_versions: HashMap::new(),
            account_epochs: HashMap::new(),
            candidate_epochs: HashMap::new(),
            recovered_revisions: HashMap::new(),
            recovering: HashMap::new(),
            credential_refreshes: HashMap::new(),
            unsupported_responses: HashMap::new(),
            zen_refused_until: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidateHealth {
    #[serde(default)]
    consecutive_failures: u32,
    #[serde(default)]
    cooldown_until_ms: Option<u64>,
    #[serde(default)]
    fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource_key: Option<String>,
    #[serde(default)]
    failure: Option<FailureClass>,
    #[serde(default)]
    revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    account_key: Option<String>,
}

#[derive(Debug)]
struct AttemptHealth {
    revision: u64,
    candidate_id: String,
    account_key: Option<String>,
    account_epoch: u64,
    candidate_epoch: u64,
    credential_version: Mutex<Option<String>>,
}

#[derive(Debug)]
struct CredentialRefresh {
    created: Instant,
    result: tokio::sync::Mutex<Option<Result<Value, String>>>,
}

#[derive(Debug, Clone)]
struct Candidate {
    internal_id: String,
    model_id: String,
    base_url: String,
    api_key: String,
    account_source: Option<crate::models::model::AccountSource>,
    fingerprint: String,
    attempt: Option<Arc<AttemptHealth>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FailureClass {
    Authentication,
    Billing,
    ModelUnavailable,
    RateLimit,
    Transient,
    RequestIncompatible,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AttemptRecord {
    candidate_id: String,
    model_id: String,
    failure: FailureClass,
    status: Option<u16>,
}

#[derive(Clone, Copy)]
struct RoutePolicy {
    max_attempts: usize,
    time_budget: Duration,
    header_timeout: Duration,
    first_byte_timeout: Duration,
    first_content_timeout: Duration,
    stream_idle_timeout: Duration,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RouteProtocol {
    #[default]
    Chat,
    Responses,
}

impl Default for RoutePolicy {
    fn default() -> Self {
        Self {
            max_attempts: MAX_ROUTE_ATTEMPTS,
            time_budget: FALLBACK_TIME_BUDGET,
            header_timeout: UPSTREAM_HEADER_TIMEOUT,
            first_byte_timeout: UPSTREAM_FIRST_BYTE_TIMEOUT,
            first_content_timeout: UPSTREAM_FIRST_CONTENT_TIMEOUT,
            stream_idle_timeout: UPSTREAM_STREAM_IDLE_TIMEOUT,
        }
    }
}

#[path = "server/memory_store.rs"]
mod memory_store;
#[cfg(test)]
use memory_store::{load_route_memory, write_memory_snapshot};
use memory_store::{
    persist_memory_at, response_memory_path, route_memory_path, route_memory_version,
    shared_response_memory, shared_route_memory,
};

pub(crate) fn retain_candidate_memory(candidate_ids: &[String]) {
    let mut valid: HashSet<String> = candidate_ids.iter().cloned().collect();
    let current = super::profiles::current_id();
    for id in super::profiles::ids().iter().filter(|id| **id != current) {
        valid.extend(
            super::profiles::read_stored(id)
                .into_iter()
                .flat_map(|config| config.candidate_ids),
        );
    }
    let memory = shared_route_memory();
    let Ok(mut memory) = memory.lock() else {
        return;
    };
    let previous_len = memory.candidates.len();
    let removed: HashSet<_> = memory
        .candidates
        .keys()
        .chain(memory.candidate_epochs.keys())
        .filter(|id| !valid.contains(*id))
        .cloned()
        .collect();
    for id in removed {
        clear_candidate_health(&mut memory, &id);
    }
    if memory.candidates.len() != previous_len {
        persist_memory_at(&route_memory_path(), memory);
    }
}

pub(crate) fn forget_candidate_memory(candidate_id: &str) {
    standby::invalidate_all();
    let memory = shared_route_memory();
    let Ok(mut memory) = memory.lock() else {
        return;
    };
    if clear_candidate_health(&mut memory, candidate_id) {
        persist_memory_at(&route_memory_path(), memory);
    }
}

pub(super) fn invalidate_standby(path: &Path) {
    standby::invalidate(path);
}

fn clear_candidate_health(memory: &mut RouteMemory, candidate_id: &str) -> bool {
    let epoch = memory
        .candidate_epochs
        .entry(candidate_id.into())
        .or_default();
    *epoch = epoch.saturating_add(1);
    memory.revision = memory.revision.saturating_add(1);
    memory.candidates.remove(candidate_id).is_some()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn candidate_fingerprint(model_id: &str, base_url: &str, api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(model_id.as_bytes());
    hasher.update([0]);
    hasher.update(base_url.as_bytes());
    hasher.update([0]);
    hasher.update(api_key.as_bytes());
    hex::encode(hasher.finalize())
}

pub(super) fn start(
    listener: std::net::TcpListener,
) -> Result<super::runtime::ServerControl, String> {
    let state = AppState::new()?;
    let shutdown = state.shutdown.clone();
    let app = Router::new()
        .route("/health", get(handle_health))
        .route("/v1/models", get(handle_models))
        .route("/models", get(handle_models))
        .route("/v1/chat/completions", post(handle_chat))
        .route("/chat/completions", post(handle_chat))
        .route("/v1/responses", post(handle_responses))
        .route("/responses", post(handle_responses))
        .route("/v1/messages", post(handle_messages))
        .route("/messages", post(handle_messages))
        .route("/v1/audio/transcriptions", post(voice::handle))
        .route("/audio/transcriptions", post(voice::handle))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state);
    let listener = tokio::net::TcpListener::from_std(listener).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    log::info!("[SmartRouter] listening on 127.0.0.1:{port}");
    Ok(super::runtime::ServerControl::start(
        listener, app, shutdown,
    ))
}

fn config_for_request(state: &AppState) -> Result<super::RequestConfig, String> {
    super::request_config_from_loader(|| {
        let bytes = fs::read(&state.config_path).map_err(|error| error.to_string())?;
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    })
}

async fn handle_health(State(state): State<AppState>) -> Response {
    let config = match config_for_request(&state) {
        Ok(config) => config,
        Err(error) => return openai_error(StatusCode::INTERNAL_SERVER_ERROR, &error),
    };
    Json(json!({
        "status": "ok",
        "model": config.model_id,
        "candidates": config.candidate_ids.len(),
        "usableCandidates": super::usable_candidate_count(&config.candidate_ids),
    }))
    .into_response()
}

async fn handle_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let config = match config_for_request(&state) {
        Ok(config) => config,
        Err(e) => return openai_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };
    if !is_authorized(&headers, &config.api_key) {
        return openai_error(StatusCode::UNAUTHORIZED, "Invalid EchoBird router API key");
    }

    let candidates = resolve_candidates(config.candidate_ids);
    let mut data = vec![json!({"id":config.model_id,"object":"model","created":0,
        "owned_by":"echobird","display_name":"Auto","type":"model",
        "created_at":"1970-01-01T00:00:00Z"})];
    let stored_models = model_manager::get_user_models();
    data.extend(candidates.iter().map(|candidate| {
        let name = stored_models
            .iter()
            .find(|m| m.internal_id == candidate.internal_id)
            .map(|m| m.name.as_str())
            .unwrap_or(&candidate.model_id);
        let name = super::catalog::display_name(&candidate.model_id, name);
        json!({"id":super::catalog::selector(&candidate.internal_id),"object":"model",
            "created":0,"owned_by":"echobird","display_name":name,"name":name,
            "type":"model","created_at":"1970-01-01T00:00:00Z"})
    }));
    Json(json!({
        "object": "list",
        "data": data,
        "has_more": false,
        "first_id": data.first().map(|model| &model["id"]),
        "last_id": data.last().map(|model| &model["id"])
    }))
    .into_response()
}

async fn handle_chat(
    State(mut state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return openai_error(error.status(), &error.body_text())
        }
        Err(error) => return error.into_response(),
    };
    state.request_epoch = Some(state.standby.epoch());
    let request_config = match config_for_request(&state) {
        Ok(config) => config,
        Err(e) => return openai_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };
    if !is_authorized(&headers, &request_config.api_key) {
        return openai_error(StatusCode::UNAUTHORIZED, "Invalid EchoBird router API key");
    }

    let requested_model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    state.model_id = request_config.model_id;
    let candidates = match select_candidates(
        &mut state,
        requested_model,
        resolve_candidates(request_config.candidate_ids),
        false,
    ) {
        Ok(candidates) => candidates,
        Err(error) => {
            return openai_error(StatusCode::NOT_FOUND, &error);
        }
    };
    protected_route(&state, &headers, body, candidates, ClientProtocol::Chat).await
}

#[derive(Clone, Copy)]
enum ClientProtocol {
    Chat,
    Responses,
    Messages,
}

async fn protected_route(
    state: &AppState,
    headers: &HeaderMap,
    mut body: Value,
    candidates: Vec<Candidate>,
    protocol: ClientProtocol,
) -> Response {
    let group = state
        .config_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(SMART_ROUTER_INTERNAL_ID);
    let statistics = state.statistics_collector.as_ref().map_or_else(
        || super::statistics::begin(group),
        |collector| collector.begin(group),
    );
    let mut completion = statistics
        .as_ref()
        .map(|request| super::statistics::Completion {
            request: request.clone(),
            response_ok: false,
            response_ready: false,
            done: false,
        });
    let terminal_session = super::terminal::ticket(group);
    let mut state = state.clone();
    state.request_statistics = statistics.clone();
    let protection = match prepare_protection(&state, &mut body).await {
        Ok(protection) => protection,
        Err(error) => {
            if let Some(completion) = &mut completion {
                completion.request.finish(false, false);
                completion.done = true;
            }
            return match protocol {
                ClientProtocol::Messages => anthropic_error(StatusCode::SERVICE_UNAVAILABLE, error),
                _ => openai_error(StatusCode::SERVICE_UNAVAILABLE, error),
            };
        }
    };
    if let (Some(statistics), Some(protection)) = (&statistics, &protection) {
        statistics.protections(protection.categories());
    }
    if let Some(session) = terminal_session {
        super::terminal::capture(&session, group, &body, |text| {
            protection.as_ref().map_or_else(
                || {
                    vec![super::terminal::Segment {
                        text: text.into(),
                        protected_text: None,
                        kind: None,
                    }]
                },
                |p| p.terminal_segments(text),
            )
        });
    }
    let response = match protocol {
        ClientProtocol::Chat => route_chat_with_candidates(&state, headers, body, candidates).await,
        ClientProtocol::Responses => {
            route_responses_with_candidates(&state, headers, body, candidates).await
        }
        ClientProtocol::Messages => {
            route_messages_with_candidates(&state, headers, body, candidates).await
        }
    };
    let response = restore_protection(protection, response).await;
    let Some(mut completion) = completion else {
        return response;
    };
    completion.response_ok = response.status().is_success();
    completion.response_ready = true;
    completion.request.response_ready();
    if !completion.response_ok {
        completion.error();
    }
    let (parts, body) = response.into_parts();
    let body = body.map_err(move |error| {
        completion.error();
        error
    });
    Response::from_parts(parts, Body::new(body))
}

async fn prepare_protection(
    state: &AppState,
    body: &mut Value,
) -> Result<Option<content_protection::Protection>, &'static str> {
    if content_protection::enabled(&state.protection_settings_path).await? {
        content_protection::Protection::prepare_tracked(
            body,
            &state.config_path,
            &state.protection_settings_path,
            state.request_statistics.is_some(),
        )
        .await
        .map(Some)
    } else {
        Ok(None)
    }
}

async fn restore_protection(
    protection: Option<content_protection::Protection>,
    response: Response,
) -> Response {
    match protection {
        Some(protection) => protection.restore(response).await,
        None => response,
    }
}

fn is_supported_router_model(model_id: &str) -> bool {
    matches!(
        model_id,
        SMART_ROUTER_MODEL_ID | "echobird/auto" | "echobird-auto" | SMART_ROUTER_INTERNAL_ID
    )
}

fn select_candidates(
    state: &mut AppState,
    requested: &str,
    candidates: Vec<Candidate>,
    anthropic_alias: bool,
) -> Result<Vec<Candidate>, String> {
    let requested = requested
        .strip_suffix("[1m]")
        .or_else(|| requested.strip_suffix("[1M]"))
        .unwrap_or(requested);
    if requested.starts_with(super::catalog::SELECTOR_PREFIX) {
        let selected: Vec<_> = candidates
            .into_iter()
            .filter(|candidate| super::catalog::selector(&candidate.internal_id) == requested)
            .collect();
        if selected.is_empty() {
            return Err(
                "Selected router model is unavailable. Refresh the client model list.".into(),
            );
        }
        state.model_id = requested.into();
        Ok(selected)
    } else if state.supports_model(requested) || anthropic_alias {
        Ok(candidates)
    } else {
        Err(format!("Unknown router model: {requested}"))
    }
}

async fn handle_responses(
    State(mut state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return openai_error(error.status(), &error.body_text())
        }
        Err(error) => return error.into_response(),
    };
    state.request_epoch = Some(state.standby.epoch());
    let request_config = match config_for_request(&state) {
        Ok(config) => config,
        Err(error) => return openai_error(StatusCode::INTERNAL_SERVER_ERROR, &error),
    };
    if !is_authorized(&headers, &request_config.api_key) {
        return openai_error(StatusCode::UNAUTHORIZED, "Invalid EchoBird router API key");
    }
    state.model_id = request_config.model_id;
    let candidates = match select_candidates(
        &mut state,
        body["model"].as_str().unwrap_or_default(),
        resolve_candidates(request_config.candidate_ids),
        false,
    ) {
        Ok(candidates) => candidates,
        Err(error) => return openai_error(StatusCode::NOT_FOUND, &error),
    };
    protected_route(
        &state,
        &headers,
        body,
        candidates,
        ClientProtocol::Responses,
    )
    .await
}

async fn handle_messages(
    State(mut state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Response {
    let body = match body {
        Ok(Json(body)) => body,
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            return anthropic_error(error.status(), &error.body_text())
        }
        Err(error) => return error.into_response(),
    };
    state.request_epoch = Some(state.standby.epoch());
    let request_config = match config_for_request(&state) {
        Ok(config) => config,
        Err(e) => return anthropic_error(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };
    if !is_authorized(&headers, &request_config.api_key) {
        return anthropic_error(StatusCode::UNAUTHORIZED, "Invalid EchoBird router API key");
    }

    state.model_id = request_config.model_id;
    let candidates = match select_candidates(
        &mut state,
        body["model"].as_str().unwrap_or_default(),
        resolve_candidates(request_config.candidate_ids),
        true,
    ) {
        Ok(candidates) => candidates,
        Err(error) => return anthropic_error(StatusCode::NOT_FOUND, &error),
    };
    protected_route(&state, &headers, body, candidates, ClientProtocol::Messages).await
}

async fn route_messages_with_candidates(
    state: &AppState,
    headers: &HeaderMap,
    body: Value,
    candidates: Vec<Candidate>,
) -> Response {
    let stream_requested = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut openai_body = crate::services::local_llm::proxy::anthropic_to_openai(&body);
    openai_body["model"] = Value::String(state.model_id.clone());
    let response = route_chat_with_candidates(state, headers, openai_body, candidates).await;

    if !response.status().is_success() {
        return openai_error_to_anthropic(response).await;
    }
    if stream_requested {
        openai_stream_to_anthropic_for_model(response, state.model_id.clone())
    } else {
        openai_response_to_anthropic(response).await
    }
}

fn resolve_candidates(ids: Vec<String>) -> Vec<Candidate> {
    resolve_candidates_from(
        ids,
        &model_manager::get_user_models(),
        &crate::services::local_llm::get_server_info_sync(),
    )
}

fn resolve_candidates_from(
    ids: Vec<String>,
    user_models: &[crate::models::model::ModelConfig],
    local_server: &crate::services::local_llm::LocalServerInfo,
) -> Vec<Candidate> {
    ids.into_iter()
        .filter_map(|id| {
            if id == "local-server" && local_server.running {
                let model_id = local_server.model_name.clone();
                let base_url = format!("http://127.0.0.1:{}/v1", local_server.port);
                let api_key = local_server.api_key.clone();
                return Some(Candidate {
                    attempt: None,
                    internal_id: id,
                    fingerprint: candidate_fingerprint(&model_id, &base_url, &api_key),
                    model_id,
                    base_url,
                    api_key,
                    account_source: None,
                });
            }

            let model = user_models.iter().find(|model| model.internal_id == id)?;
            let model_id = model.model_id.as_deref()?.trim();
            if model_id.is_empty() || model.base_url.trim().is_empty() {
                return None;
            }
            let api_key = model_manager::decrypt_key_for_use(&model.api_key);

            Some(Candidate {
                attempt: None,
                internal_id: model.internal_id.clone(),
                fingerprint: candidate_fingerprint(
                    model_id,
                    &model.base_url,
                    &model
                        .account_source
                        .as_ref()
                        .map(|source| serde_json::to_string(source).unwrap_or_default())
                        .unwrap_or_else(|| model.api_key.clone()),
                ),
                model_id: model_id.to_string(),
                base_url: model.base_url.clone(),
                api_key,
                account_source: model.account_source.clone(),
            })
        })
        .filter(|candidate| !super::is_router_url(&candidate.base_url))
        .collect()
}

async fn route_chat_with_candidates(
    state: &AppState,
    inbound_headers: &HeaderMap,
    body: Value,
    candidates: Vec<Candidate>,
) -> Response {
    route_chat_with_policy(
        state,
        inbound_headers,
        body,
        candidates,
        RoutePolicy::default(),
    )
    .await
}

async fn route_responses_with_candidates(
    state: &AppState,
    inbound_headers: &HeaderMap,
    body: Value,
    candidates: Vec<Candidate>,
) -> Response {
    route_with_policy(
        state,
        inbound_headers,
        body,
        candidates,
        RoutePolicy::default(),
        RouteProtocol::Responses,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn route_live_account_probe(
    source: crate::models::model::AccountSource,
    model_id: String,
    body: Value,
) -> Result<Response, String> {
    let state = AppState::for_tests()?;
    let mut policy = RoutePolicy::default();
    if let Some(seconds) = std::env::var("ECHOBIRD_LIVE_PROXY_BUDGET_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| (45..=180).contains(seconds))
    {
        policy.time_budget = Duration::from_secs(seconds);
    }
    let source_json = serde_json::to_string(&source).map_err(|_| "Invalid account source")?;
    let candidate = Candidate {
        attempt: None,
        internal_id: "live-account-probe".into(),
        fingerprint: candidate_fingerprint(&model_id, "account://echobird", &source_json),
        model_id,
        base_url: "account://echobird".into(),
        api_key: String::new(),
        account_source: Some(source),
    };
    // Use production routing without changing the user's candidates or health file.
    Ok(route_chat_with_policy(&state, &HeaderMap::new(), body, vec![candidate], policy).await)
}

/// Only the Claude subscription reads Claude Code's billing line.
fn drops_billing_line(candidate: &Candidate, protocol: RouteProtocol) -> bool {
    protocol == RouteProtocol::Chat
        && !candidate
            .account_source
            .as_ref()
            .is_some_and(|source| source.provider == AccountProvider::ClaudeCode)
}

async fn send_candidate(
    state: &AppState,
    headers: &HeaderMap,
    candidate: &Candidate,
    mut body: Value,
    protocol: RouteProtocol,
    refreshed_credentials: Option<Value>,
    final_candidate: bool,
) -> Result<Response, String> {
    if drops_billing_line(candidate, protocol) {
        crate::services::anthropic_proxy::billing_line::strip_from_chat(&mut body);
    }
    #[cfg(test)]
    if let Some(responses) = &state.fixture_responses {
        if let Some(credentials) = refreshed_credentials {
            let mut fixture = state
                .fixture_authentication
                .as_ref()
                .expect("Refresh fixture is missing")
                .lock()
                .unwrap();
            fixture.sent_credentials.push(credentials.clone());
            bind_attempt_credentials(state, candidate, &credentials);
            return fixture
                .retry_responses
                .pop_front()
                .ok_or_else(|| "Fixture retry response is missing".into());
        }
        let response = responses
            .lock()
            .map_err(|_| "Fixture response lock poisoned".to_string())?
            .remove(&candidate.internal_id)
            .ok_or_else(|| "Fixture response is missing".to_string())?;
        if let Some(credentials) = response.extensions().get::<RejectedAccountCredentials>() {
            bind_attempt_credentials(state, candidate, &credentials.0);
        }
        return Ok(response);
    }
    if let Some(source) = &candidate.account_source {
        if source.provider == crate::models::model::AccountProvider::Zcode {
            crate::services::account_proxy::bind_zcode_session(&mut body, headers);
        }
        let credentials = match refreshed_credentials {
            Some(credentials) => credentials,
            None => {
                match crate::services::account_model_sources::credentials(source, false).await {
                    Ok(value) => value,
                    Err(error) => {
                        return Ok(
                            crate::services::account_model_sources::credential_error(error)
                                .into_response(),
                        )
                    }
                }
            }
        };
        bind_attempt_credentials(state, candidate, &credentials);
        let rejected_credentials = credentials.clone();
        let result = send_account_request(
            state,
            source,
            credentials,
            &candidate.model_id,
            body.clone(),
            protocol,
            final_candidate,
        )
        .await;
        let mut response = match result {
            Ok(response) => response,
            Err(error) => error.into_response(),
        };
        response
            .extensions_mut()
            .insert(RejectedAccountCredentials(rejected_credentials));
        return Ok(response);
    }
    if zen_free::is_candidate(candidate) {
        return zen_free::send(state, headers, candidate, &body, protocol, final_candidate).await;
    }
    if protocol == RouteProtocol::Responses {
        let continuing = has_previous_response(&body);
        let unsupported = state.route_memory.lock().ok().is_some_and(|memory| {
            memory
                .unsupported_responses
                .get(&candidate.fingerprint)
                .is_some_and(|until| *until > now_ms())
        });
        if !unsupported || continuing {
            let response = send_api_request(state, headers, candidate, &body, protocol).await?;
            if matches!(response.status().as_u16(), 400 | 404 | 405 | 501) {
                let status = response.status();
                let (parts, response_body) = response.into_parts();
                let bytes = axum::body::to_bytes(response_body, 64 * 1024)
                    .await
                    .map_err(|error| error.to_string())?;
                if !responses_endpoint_unsupported(status, &bytes) || continuing {
                    return Ok(Response::from_parts(parts, Body::from(bytes)));
                }
                remember_unsupported_responses(state, candidate);
            } else {
                return Ok(response);
            }
        }
        body = match responses_chat_request(&body, final_candidate) {
            Ok(chat) => chat,
            Err(error) => return Ok(error.into_response()),
        };
        let response =
            send_api_request(state, headers, candidate, &body, RouteProtocol::Chat).await?;
        return responses_from_chat(response, &body)
            .await
            .map_err(|error| error.to_string());
    }
    crate::services::account_proxy::strip_response_metadata(&mut body);
    send_api_request(state, headers, candidate, &body, protocol).await
}

#[derive(Clone)]
struct RejectedAccountCredentials(Value);

async fn refresh_rejected_account(
    state: &AppState,
    candidate: &Candidate,
    rejected: &RejectedAccountCredentials,
    started: Instant,
    policy: RoutePolicy,
) -> Result<Value, crate::services::account_proxy::ProxyError> {
    let refresh = async {
        let source = candidate
            .account_source
            .as_ref()
            .expect("Only account requests bind credentials");
        let slot = {
            let mut memory = state
                .route_memory
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let account = source_health_key(source);
            let epoch = memory.account_epochs.get(&account).copied().unwrap_or(0);
            let key = format!(
                "{account}:{epoch}:{:x}",
                Sha256::digest(rejected.0.to_string())
            );
            memory.credential_refreshes.retain(|_, slot| {
                slot.created.elapsed() < Duration::from_secs(30) || Arc::strong_count(slot) > 1
            });
            if let Some(slot) = memory.credential_refreshes.get(&key) {
                slot.clone()
            } else {
                let slot = Arc::new(CredentialRefresh {
                    created: Instant::now(),
                    result: tokio::sync::Mutex::new(None),
                });
                if memory.credential_refreshes.len() < 128 {
                    memory.credential_refreshes.insert(key, slot.clone());
                }
                slot
            }
        };
        let mut result = slot.result.lock().await;
        if let Some(value) = result.as_ref() {
            return value.clone();
        }
        crate::services::account_proxy::forget_preparation_metadata(
            source.provider,
            &source.account_id,
        );
        let value = async {
            #[cfg(test)]
            if let Some(fixture) = &state.fixture_authentication {
                let (delay, result) = {
                    let mut fixture = fixture.lock().unwrap();
                    fixture.rejections.push((
                        candidate.account_source.clone().unwrap(),
                        rejected.0.clone(),
                    ));
                    (fixture.delay, fixture.result.clone())
                };
                tokio::time::sleep(delay).await;
                return result;
            }
            crate::services::account_model_sources::credentials_after_rejection(
                candidate
                    .account_source
                    .as_ref()
                    .expect("Only account requests bind credentials"),
                &rejected.0,
            )
            .await
        }
        .await;
        *result = Some(value.clone());
        value
    };
    let timeout = remaining_timeout(started, policy.time_budget, policy.header_timeout)
        .ok_or_else(|| {
            crate::services::account_proxy::ProxyError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "Account refresh exceeded the routing budget",
            )
        })?;
    tokio::time::timeout(timeout, refresh)
        .await
        .map_err(|_| {
            crate::services::account_proxy::ProxyError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "Account refresh exceeded the routing budget",
            )
        })?
        .map_err(crate::services::account_model_sources::credential_error)
}

#[derive(Clone, Copy)]
struct NativeResponses;

async fn send_account_request(
    state: &AppState,
    source: &crate::models::model::AccountSource,
    credentials: Value,
    model: &str,
    body: Value,
    protocol: RouteProtocol,
    final_candidate: bool,
) -> Result<Response, crate::services::account_proxy::ProxyError> {
    if protocol == RouteProtocol::Responses {
        if let Some(mut response) = crate::services::account_proxy::route_responses(
            source.provider,
            credentials.clone(),
            model,
            body.clone(),
            &state.http_client,
        )
        .await?
        {
            response.extensions_mut().insert(NativeResponses);
            return Ok(response);
        }
        let chat = responses_chat_request(&body, final_candidate)?;
        let response = crate::services::account_proxy::route(
            source.provider,
            credentials,
            model,
            chat.clone(),
            &state.http_client,
        )
        .await?;
        return responses_from_chat(response, &chat).await;
    }
    crate::services::account_proxy::route(
        source.provider,
        credentials,
        model,
        body,
        &state.http_client,
    )
    .await
}

fn responses_chat_request(
    body: &Value,
    final_candidate: bool,
) -> Result<Value, crate::services::account_proxy::ProxyError> {
    let conversion_error = |mut error: crate::services::account_proxy::ProxyError| {
        error.error_code = Some("responses_conversion_unsupported".into());
        error
    };
    // A later native candidate can still run hosted tools; only the last goes without.
    if !final_candidate && crate::services::account_proxy::declares_hosted_tools(body) {
        return Err(conversion_error(
            crate::services::account_proxy::ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Hosted tools require a native Responses model",
            ),
        ));
    }
    let mut chat =
        crate::services::account_proxy::responses_to_chat(body).map_err(conversion_error)?;
    // Collect after headers, within the router's existing completion budget.
    chat["stream"] = json!(true);
    Ok(chat)
}

async fn responses_from_chat(
    response: Response,
    chat: &Value,
) -> Result<Response, crate::services::account_proxy::ProxyError> {
    let original = &chat["_echobird_responses"];
    let response =
        crate::services::account_proxy::chat_to_responses_with_tools(response, true, original)
            .await;
    crate::services::account_proxy::responses_response(
        response,
        original["stream"].as_bool().unwrap_or(false),
    )
}

async fn send_api_request(
    state: &AppState,
    headers: &HeaderMap,
    candidate: &Candidate,
    body: &Value,
    protocol: RouteProtocol,
) -> Result<Response, String> {
    let mut body = body.clone();
    if protocol == RouteProtocol::Chat {
        crate::services::account_proxy::strip_response_metadata(&mut body);
    } else {
        crate::services::account_proxy::normalize_responses_tool_rounds(&mut body);
    }
    if protocol == RouteProtocol::Chat
        && url::Url::parse(&candidate.base_url)
            .ok()
            .is_some_and(|url| url.host_str() == Some("api.deepseek.com"))
        && body.get("max_tokens").is_none()
    {
        if let Some(value) = body
            .as_object_mut()
            .and_then(|body| body.remove("max_completion_tokens"))
        {
            body["max_tokens"] = value;
        }
    }
    let mut request = state
        .http_client
        .post(if protocol == RouteProtocol::Responses {
            responses_url(&candidate.base_url)
        } else {
            chat_completions_url(&candidate.base_url)
        })
        .header(header::CONTENT_TYPE, "application/json")
        .header("HTTP-Referer", "https://echobird.ai")
        .header("X-OpenRouter-Title", "EchoBird")
        .header("X-OpenRouter-Categories", "programming-app,personal-agent")
        .json(&body);
    if let Some(api_key) = upstream_bearer_token(candidate) {
        request = request.bearer_auth(api_key);
    }
    if let Some(user_agent) = headers.get(header::USER_AGENT) {
        request = request.header(header::USER_AGENT, user_agent.clone());
    }
    let upstream = request.send().await.map_err(|e| e.to_string())?;
    let status = upstream.status();
    let response_headers = upstream.headers().clone();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = response_headers;
    if protocol == RouteProtocol::Responses {
        response.extensions_mut().insert(NativeResponses);
        crate::services::account_proxy::responses_response(
            response,
            body["stream"].as_bool().unwrap_or(false),
        )
        .map_err(|error| error.to_string())
    } else {
        Ok(response)
    }
}

fn upstream_bearer_token(candidate: &Candidate) -> Option<&str> {
    (!candidate.api_key.trim().is_empty()).then_some(candidate.api_key.as_str())
}

async fn route_chat_with_policy(
    state: &AppState,
    inbound_headers: &HeaderMap,
    body: Value,
    candidates: Vec<Candidate>,
    policy: RoutePolicy,
) -> Response {
    route_with_policy(
        state,
        inbound_headers,
        body,
        candidates,
        policy,
        RouteProtocol::Chat,
    )
    .await
}

async fn route_with_policy(
    state: &AppState,
    inbound_headers: &HeaderMap,
    body: Value,
    mut candidates: Vec<Candidate>,
    policy: RoutePolicy,
    protocol: RouteProtocol,
) -> Response {
    if protocol == RouteProtocol::Responses && body["background"] == true {
        return openai_error(StatusCode::BAD_REQUEST, "Background Responses require a separate task lifecycle and are not supported by Auto Router");
    }
    if protocol == RouteProtocol::Responses
        && body
            .get("conversation")
            .is_some_and(|value| !value.is_null())
    {
        return openai_error(StatusCode::BAD_REQUEST, "Conversation objects are not supported by Auto Router; use complete input or a bound previous_response_id");
    }
    let bound_continuation = protocol == RouteProtocol::Responses && has_previous_response(&body);
    let bound_candidates = match response_candidates(state, &body, candidates.clone(), protocol) {
        Ok(candidates) => candidates,
        Err(error) => return openai_error(StatusCode::BAD_REQUEST, error),
    };
    if candidates.is_empty() {
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "No usable models are configured for Auto Router",
        );
    }

    let configured_candidates = candidates.clone();
    let standby_epoch = state
        .standby
        .order_for_request(&mut candidates, state.request_epoch)
        .filter(|_| !bound_continuation);
    let mut available = prioritized_candidates(state, candidates);
    if bound_continuation {
        available.retain(|candidate| {
            bound_candidates
                .iter()
                .any(|bound| bound.internal_id == candidate.internal_id)
        });
    }
    if available.is_empty() {
        let now = now_ms();
        let refused = state.route_memory.lock().ok().is_some_and(|memory| {
            configured_candidates.iter().any(|candidate| {
                memory
                    .zen_refused_until
                    .get(&candidate.internal_id)
                    .is_some_and(|until| *until > now)
            })
        });
        return openai_error(
            StatusCode::SERVICE_UNAVAILABLE,
            if refused {
                zen_free::REFUSED_MESSAGE
            } else {
                "All Auto Router models are cooling down"
            },
        );
    }

    let stream_requested = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let mut attempts = Vec::new();
    let mut zen_refused = false;
    let mut incompatible_response = None;
    let routing_started = Instant::now();

    let mut available = available.into_iter();
    let mut authentication_retry: Option<(Candidate, Option<Value>)> = None;
    while let Some((mut candidate, refreshed_credentials)) = authentication_retry
        .take()
        .or_else(|| available.next().map(|candidate| (candidate, None)))
    {
        let authentication_retried = refreshed_credentials.is_some();
        if (authentication_retried && candidate_is_cooling_down(state, &candidate))
            || (!authentication_retried && !try_begin_attempt(state, &mut candidate))
        {
            continue;
        }
        if attempts.len() >= policy.max_attempts {
            break;
        }
        let Some(header_timeout) = remaining_timeout(
            routing_started,
            policy.time_budget,
            if !stream_requested
                && candidate.account_source.as_ref().is_some_and(|source| {
                    source.provider == crate::models::model::AccountProvider::ClaudeCode
                })
            {
                policy.time_budget
            } else {
                policy.header_timeout
            },
        ) else {
            break;
        };
        let mut upstream_body = body.clone();
        if !authentication_retried {
            if let Some(statistics) = &state.request_statistics {
                let group = state
                    .config_path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or(SMART_ROUTER_INTERNAL_ID);
                statistics.attempt(
                    group,
                    &candidate.internal_id,
                    &candidate.model_id,
                    candidate.account_source.as_ref(),
                );
            }
        }
        upstream_body["model"] = Value::String(candidate.model_id.clone());
        let activity_guard =
            RouteActivityGuard::begin_for(&candidate.internal_id, state.activity.clone());
        // A slow start is worth cutting only while another candidate can answer.
        let final_candidate = available.len() == 0 || attempts.len() + 1 >= policy.max_attempts;
        let first_content_limit = if final_candidate {
            policy.first_byte_timeout
        } else {
            shortened_first_content_timeout(policy.first_content_timeout, routing_started.elapsed())
        };
        let first_content_deadline = tokio::time::Instant::now() + first_content_limit;
        let sent = tokio::time::timeout(
            if stream_requested {
                header_timeout.min(first_content_limit)
            } else {
                header_timeout
            },
            send_candidate(
                state,
                inbound_headers,
                &candidate,
                upstream_body,
                protocol,
                refreshed_credentials,
                final_candidate,
            ),
        )
        .await;
        let mut response = match sent {
            Ok(Ok(response)) => response,
            Ok(Err(error)) => {
                mark_failure(state, &candidate, FailureClass::Transient, None);
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure: FailureClass::Transient,
                    status: None,
                });
                log::warn!(
                    "[SmartRouter] {} transport failure: {}",
                    candidate.internal_id,
                    error
                );
                continue;
            }
            Err(_) => {
                mark_failure(state, &candidate, FailureClass::Transient, None);
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure: FailureClass::Transient,
                    status: None,
                });
                log::warn!(
                    "[SmartRouter] {} timed out before response headers",
                    candidate.internal_id
                );
                continue;
            }
        };

        response = match &state.statistics_collector {
            Some(collector) => collector.observe(
                response,
                state.request_statistics.as_ref(),
                &candidate.internal_id,
            ),
            None => super::statistics::observe(
                response,
                state.request_statistics.as_ref(),
                &candidate.internal_id,
            ),
        };
        let rejected_credentials = (!authentication_retried)
            .then(|| {
                response
                    .extensions()
                    .get::<RejectedAccountCredentials>()
                    .cloned()
            })
            .flatten();
        if response.status() == StatusCode::UNAUTHORIZED {
            if let Some(rejected) = &rejected_credentials {
                match refresh_rejected_account(state, &candidate, rejected, routing_started, policy)
                    .await
                {
                    Ok(credentials) => {
                        authentication_retry = Some((candidate, Some(credentials)));
                        continue;
                    }
                    Err(error) => response = error.into_response(),
                }
            }
        }
        let status = response.status();
        let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
        let cache_control = response.headers().get(header::CACHE_CONTROL).cloned();
        let response_headers = response.headers().clone();
        let native_responses = response.extensions().get::<NativeResponses>().is_some();
        let model_scope = response
            .extensions()
            .get::<crate::services::account_proxy::ProxyFailureMetadata>()
            .is_some_and(|metadata| metadata.model_scope);
        if !status.is_success() {
            let retry_after = retry_after(response.headers());
            let Some(error_body_timeout) =
                remaining_timeout(routing_started, policy.time_budget, policy.header_timeout)
            else {
                mark_failure(state, &candidate, FailureClass::Transient, None);
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure: FailureClass::Transient,
                    status: Some(status.as_u16()),
                });
                break;
            };
            let response_body = match tokio::time::timeout(
                error_body_timeout,
                response.into_body().collect(),
            )
            .await
            {
                Ok(Ok(body)) => body.to_bytes(),
                Ok(Err(_)) | Err(_) => {
                    mark_failure(state, &candidate, FailureClass::Transient, None);
                    attempts.push(AttemptRecord {
                        candidate_id: candidate.internal_id.clone(),
                        model_id: candidate.model_id.clone(),
                        failure: FailureClass::Transient,
                        status: Some(status.as_u16()),
                    });
                    break;
                }
            };
            if status == StatusCode::PAYLOAD_TOO_LARGE {
                return openai_error(
                    status,
                    "Upstream rejected the request because its body is too large",
                );
            }
            if bound_continuation && is_continuation_failure(status, &response_body) {
                let mut response = upstream_response(
                    status,
                    content_type,
                    cache_control,
                    response_body,
                    &candidate,
                );
                copy_response_headers(&mut response, &response_headers);
                return response;
            }
            if is_context_limit_body(&response_body) {
                let mut response = upstream_response(
                    StatusCode::BAD_REQUEST,
                    content_type,
                    cache_control,
                    response_body,
                    &candidate,
                );
                copy_response_headers(&mut response, &response_headers);
                return response;
            }
            if let Some(failure) = mark_upstream_failure(
                state,
                &candidate,
                status,
                &response_body,
                retry_after,
                model_scope,
            ) {
                if zen_free::is_refusal(&response_body) {
                    zen_refused = true;
                    if let Ok(mut memory) = state.route_memory.lock() {
                        let wait = retry_after.unwrap_or_default().as_millis() as u64;
                        memory
                            .zen_refused_until
                            .insert(candidate.internal_id.clone(), now_ms().saturating_add(wait));
                    }
                }
                if failure == FailureClass::RequestIncompatible {
                    incompatible_response = Some(upstream_response(
                        status,
                        content_type.clone(),
                        cache_control.clone(),
                        response_body.clone(),
                        &candidate,
                    ));
                }
                if protocol == RouteProtocol::Responses && has_previous_response(&body) {
                    let mut response = upstream_response(
                        status,
                        content_type,
                        cache_control,
                        response_body,
                        &candidate,
                    );
                    copy_response_headers(&mut response, &response_headers);
                    return response;
                }
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure,
                    status: Some(status.as_u16()),
                });
                log::warn!(
                    "[SmartRouter] {} returned {}, trying next candidate",
                    candidate.internal_id,
                    status.as_u16()
                );
                continue;
            }

            let mut response = upstream_response(
                status,
                content_type,
                cache_control,
                response_body,
                &candidate,
            );
            if protocol == RouteProtocol::Responses {
                copy_response_headers(&mut response, &response_headers);
            }
            return response;
        }

        if stream_requested {
            let mut completion = StreamCompletion {
                headers: response.headers().clone(),
                protocol,
                ..StreamCompletion::default()
            };
            let mut upstream_stream = response.into_body().into_data_stream();
            let Some(first_byte_timeout) = remaining_timeout(
                routing_started,
                policy.time_budget,
                policy.first_byte_timeout,
            ) else {
                mark_failure(state, &candidate, FailureClass::Transient, None);
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure: FailureClass::Transient,
                    status: Some(status.as_u16()),
                });
                break;
            };
            let first_byte_deadline =
                first_content_deadline.min(tokio::time::Instant::now() + first_byte_timeout);
            let first = match tokio::time::timeout_at(first_byte_deadline, async {
                while let Some(chunk) = upstream_stream.next().await {
                    let chunk = chunk?;
                    if !chunk.is_empty() {
                        return Ok(chunk);
                    }
                }
                Err(axum::Error::new(empty_response_error()))
            })
            .await
            {
                Ok(Ok(chunk)) => tokio::time::timeout_at(
                    first_content_deadline,
                    preflight_stream(
                        chunk,
                        &mut upstream_stream,
                        &mut completion,
                        bound_continuation && native_responses,
                    ),
                )
                .await
                .unwrap_or_else(|_| {
                    Err(axum::Error::new(stream_timeout_error(
                        "first_content_timeout",
                        "Upstream timed out before any output",
                    )))
                }),
                Ok(Err(error)) => Err(error),
                Err(_) => {
                    mark_failure(state, &candidate, FailureClass::Transient, None);
                    attempts.push(AttemptRecord {
                        candidate_id: candidate.internal_id.clone(),
                        model_id: candidate.model_id.clone(),
                        failure: FailureClass::Transient,
                        status: Some(status.as_u16()),
                    });
                    continue;
                }
            };
            match first {
                Ok(buffered) => {
                    if let Some(standby_epoch) = standby_epoch.filter(|_| completion.meaningful) {
                        state.standby.used(
                            state,
                            &candidate,
                            &configured_candidates,
                            protocol,
                            standby_epoch,
                        );
                    }
                    completion
                        .last_progress
                        .get_or_insert_with(tokio::time::Instant::now);
                    let stream_state = state.clone();
                    let stream_candidate = candidate.clone();
                    let body_stream = stream::unfold(
                        Some((buffered, upstream_stream, completion, activity_guard)),
                        move |state| {
                            let stream_state = stream_state.clone();
                            let stream_candidate = stream_candidate.clone();
                            async move {
                                let (mut buffered, mut upstream, mut completion, guard) = state?;
                                let (item, observed) = if let Some(bytes) = buffered.pop_front() {
                                    (Some(Ok(bytes)), true)
                                } else {
                                    let deadline = completion.last_progress.unwrap()
                                        + policy.stream_idle_timeout;
                                    let item = tokio::time::timeout_at(deadline, upstream.next())
                                        .await
                                        .unwrap_or_else(|_| {
                                            Some(Err(axum::Error::new(stream_timeout_error(
                                                "stream_idle_timeout",
                                                "Upstream stream timed out waiting for output",
                                            ))))
                                        });
                                    (item, false)
                                };
                                let (result, finished) = match item {
                                    Some(Ok(bytes)) => match if observed {
                                        Ok(())
                                    } else {
                                        completion.push(&bytes).and_then(|()| {
                                            if !completion.done && completion.last_progress
                                                .is_some_and(|last| last.elapsed() >= policy.stream_idle_timeout)
                                            {
                                                Err(std::io::Error::new(
                                                    std::io::ErrorKind::TimedOut,
                                                    stream_timeout_error(
                                                        "stream_idle_timeout",
                                                        "Upstream stream timed out waiting for output",
                                                    ),
                                                ))
                                            } else {
                                                Ok(())
                                            }
                                        })
                                    } {
                                        Ok(()) => {
                                            if completion.done && buffered.is_empty() {
                                                if completion.failure.is_none() {
                                                    mark_success(&stream_state, &stream_candidate);
                                                    if native_responses {
                                                        remember_response(
                                                            &stream_state,
                                                            &stream_candidate,
                                                            completion.response_id.as_deref(),
                                                        );
                                                    }
                                                } else if let Some(error) = &completion.failure {
                                                    let _ = mark_response_failure(
                                                        &stream_state,
                                                        &stream_candidate,
                                                        error,
                                                        bound_continuation,
                                                    );
                                                }
                                            }
                                            (Ok(bytes), completion.done && buffered.is_empty())
                                        }
                                        Err(error) => {
                                            let _ = mark_response_failure(
                                                &stream_state,
                                                &stream_candidate,
                                                &error,
                                                bound_continuation,
                                            );
                                            (Err(error), true)
                                        }
                                    },
                                    Some(Err(error)) => {
                                        let _ = mark_response_failure(
                                            &stream_state,
                                            &stream_candidate,
                                            &error,
                                            bound_continuation,
                                        );
                                        (Err(std::io::Error::other(error)), true)
                                    }
                                    None => match completion.finish() {
                                        Ok(()) => {
                                            if let Some(error) = &completion.failure {
                                                let _ = mark_response_failure(
                                                    &stream_state,
                                                    &stream_candidate,
                                                    error,
                                                    bound_continuation,
                                                );
                                            } else {
                                                mark_success(&stream_state, &stream_candidate);
                                                if native_responses {
                                                    remember_response(
                                                        &stream_state,
                                                        &stream_candidate,
                                                        completion.response_id.as_deref(),
                                                    );
                                                }
                                            }
                                            return None;
                                        }
                                        Err(error) => {
                                            let _ = mark_response_failure(
                                                &stream_state,
                                                &stream_candidate,
                                                &error,
                                                bound_continuation,
                                            );
                                            (Err(error), true)
                                        }
                                    },
                                };
                                let result =
                                    result.or_else(|error| {
                                        match proxy_error_from_chain(&error).filter(|error| {
                                            matches!(
                                                error.error_code.as_deref(),
                                                Some("stream_idle_timeout" | UPSTREAM_FINISH_ERROR)
                                            )
                                        }) {
                                            Some(error) => {
                                                Ok(stream_failure_event(protocol, error))
                                            }
                                            None => Err(error),
                                        }
                                    });
                                Some((
                                    result,
                                    (!finished).then_some((buffered, upstream, completion, guard)),
                                ))
                            }
                        },
                    );
                    let mut result = streaming_response(
                        content_type,
                        cache_control,
                        Body::from_stream(body_stream),
                        &candidate,
                    );
                    if protocol == RouteProtocol::Responses {
                        copy_response_headers(&mut result, &response_headers);
                    }
                    return result;
                }
                Err(mut error) => {
                    if proxy_error_from_chain(&error)
                        .is_some_and(|error| error.status == StatusCode::UNAUTHORIZED)
                    {
                        if let Some(rejected) = &rejected_credentials {
                            match refresh_rejected_account(
                                state,
                                &candidate,
                                rejected,
                                routing_started,
                                policy,
                            )
                            .await
                            {
                                Ok(credentials) => {
                                    authentication_retry = Some((candidate, Some(credentials)));
                                    continue;
                                }
                                Err(refresh_error) => error = axum::Error::new(refresh_error),
                            }
                        }
                    }
                    let Some(failure) =
                        mark_response_failure(state, &candidate, &error, bound_continuation)
                    else {
                        return proxy_error_from_chain(&error)
                            .expect("Unclassified body failure has a typed upstream error")
                            .clone()
                            .into_response();
                    };
                    if bound_continuation {
                        return proxy_error_from_chain(&error)
                            .map(|error| error.clone().into_response())
                            .unwrap_or_else(|| {
                                openai_error(
                                    StatusCode::BAD_GATEWAY,
                                    "The bound Responses stream failed",
                                )
                            });
                    }
                    attempts.push(AttemptRecord {
                        candidate_id: candidate.internal_id.clone(),
                        model_id: candidate.model_id.clone(),
                        failure,
                        status: Some(status.as_u16()),
                    });
                    continue;
                }
            }
        }

        let Some(body_timeout) =
            remaining_timeout(routing_started, policy.time_budget, policy.time_budget)
        else {
            mark_failure(state, &candidate, FailureClass::Transient, None);
            attempts.push(AttemptRecord {
                candidate_id: candidate.internal_id.clone(),
                model_id: candidate.model_id.clone(),
                failure: FailureClass::Transient,
                status: Some(status.as_u16()),
            });
            break;
        };
        match tokio::time::timeout(body_timeout, response.into_body().collect()).await {
            Ok(Ok(response_body)) => {
                let bytes = response_body.to_bytes();
                if protocol == RouteProtocol::Responses {
                    let value = match serde_json::from_slice::<Value>(&bytes) {
                        Ok(value) => value,
                        Err(error) => {
                            mark_stream_failure(state, &candidate, &error);
                            attempts.push(AttemptRecord {
                                candidate_id: candidate.internal_id.clone(),
                                model_id: candidate.model_id.clone(),
                                failure: FailureClass::Transient,
                                status: Some(status.as_u16()),
                            });
                            continue;
                        }
                    };
                    if let Some(error) = crate::services::account_proxy::stream_error(
                        if value["status"] == "failed" {
                            "response.failed"
                        } else {
                            ""
                        },
                        &value.to_string(),
                        &response_headers,
                    ) {
                        mark_response_failure(state, &candidate, &error, bound_continuation);
                        if crate::services::account_proxy::context_limit_error(
                            error.error_code.as_deref(),
                            &error.message,
                        ) {
                            return error.into_response();
                        }
                    } else if matches!(value["status"].as_str(), Some("completed" | "incomplete"))
                        && value["object"] == "response"
                        && value["id"].as_str().is_some_and(|id| !id.is_empty())
                    {
                        if empty_completed_response(&value) {
                            let error = empty_response_error();
                            mark_response_failure(state, &candidate, &error, bound_continuation);
                            if bound_continuation {
                                return error.into_response();
                            }
                            attempts.push(AttemptRecord {
                                candidate_id: candidate.internal_id.clone(),
                                model_id: candidate.model_id.clone(),
                                failure: FailureClass::Transient,
                                status: Some(status.as_u16()),
                            });
                            continue;
                        }
                        mark_success(state, &candidate);
                        if let Some(standby_epoch) =
                            standby_epoch.filter(|_| standby_probe::response_has_output(&value))
                        {
                            state.standby.used(
                                state,
                                &candidate,
                                &configured_candidates,
                                protocol,
                                standby_epoch,
                            );
                        }
                        if native_responses {
                            remember_response(state, &candidate, value["id"].as_str());
                        }
                    } else {
                        mark_failure(state, &candidate, FailureClass::Transient, None);
                        attempts.push(AttemptRecord {
                            candidate_id: candidate.internal_id.clone(),
                            model_id: candidate.model_id.clone(),
                            failure: FailureClass::Transient,
                            status: Some(status.as_u16()),
                        });
                        continue;
                    }
                    let mut response =
                        upstream_response(status, content_type, cache_control, bytes, &candidate);
                    copy_response_headers(&mut response, &response_headers);
                    return response;
                }
                let response_value = serde_json::from_slice::<Value>(&bytes).ok();
                if response_value.as_ref().is_some_and(|value| {
                    empty_chat_response(value) || chat_finished_with_error(value)
                }) {
                    mark_failure(state, &candidate, FailureClass::Transient, None);
                    attempts.push(AttemptRecord {
                        candidate_id: candidate.internal_id.clone(),
                        model_id: candidate.model_id.clone(),
                        failure: FailureClass::Transient,
                        status: Some(status.as_u16()),
                    });
                    continue;
                }
                mark_success(state, &candidate);
                if let Some(standby_epoch) = standby_epoch.filter(|_| {
                    response_value.as_ref().is_some_and(|value| {
                        value["choices"].as_array().is_some_and(|choices| {
                            choices
                                .iter()
                                .any(|choice| chat_message_has_content(&choice["message"]))
                        })
                    })
                }) {
                    state.standby.used(
                        state,
                        &candidate,
                        &configured_candidates,
                        protocol,
                        standby_epoch,
                    );
                }
                return upstream_response(status, content_type, cache_control, bytes, &candidate);
            }
            Ok(Err(mut error)) => {
                if proxy_error_from_chain(&error)
                    .is_some_and(|error| error.status == StatusCode::UNAUTHORIZED)
                {
                    if let Some(rejected) = &rejected_credentials {
                        match refresh_rejected_account(
                            state,
                            &candidate,
                            rejected,
                            routing_started,
                            policy,
                        )
                        .await
                        {
                            Ok(credentials) => {
                                authentication_retry = Some((candidate, Some(credentials)));
                                continue;
                            }
                            Err(refresh_error) => error = axum::Error::new(refresh_error),
                        }
                    }
                }
                let Some(failure) =
                    mark_response_failure(state, &candidate, &error, bound_continuation)
                else {
                    return proxy_error_from_chain(&error)
                        .expect("Unclassified body failure has a typed upstream error")
                        .clone()
                        .into_response();
                };
                if protocol == RouteProtocol::Responses && has_previous_response(&body) {
                    return proxy_error_from_chain(&error)
                        .map(|error| error.clone().into_response())
                        .unwrap_or_else(|| {
                            openai_error(
                                StatusCode::BAD_GATEWAY,
                                "The bound Responses stream failed",
                            )
                        });
                }
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure,
                    status: Some(status.as_u16()),
                });
                log::warn!(
                    "[SmartRouter] {} response body failure: {}",
                    candidate.internal_id,
                    error
                );
            }
            Err(_) => {
                mark_failure(state, &candidate, FailureClass::Transient, None);
                attempts.push(AttemptRecord {
                    candidate_id: candidate.internal_id.clone(),
                    model_id: candidate.model_id.clone(),
                    failure: FailureClass::Transient,
                    status: Some(status.as_u16()),
                });
                log::warn!(
                    "[SmartRouter] {} response body exceeded the routing budget",
                    candidate.internal_id
                );
                break;
            }
        }
    }

    let detail = serde_json::to_value(&attempts).unwrap_or_else(|_| json!([]));
    if !attempts.is_empty()
        && attempts
            .iter()
            .all(|attempt| attempt.failure == FailureClass::RequestIncompatible)
    {
        if let Some(response) = incompatible_response {
            return response;
        }
    }
    log::error!("[SmartRouter] all candidates failed: {detail}");
    json_error(
        StatusCode::SERVICE_UNAVAILABLE,
        if zen_refused {
            zen_free::REFUSED_MESSAGE
        } else {
            "All Auto Router models failed"
        },
        Some(detail),
    )
}

#[path = "server/anthropic.rs"]
mod anthropic;
use anthropic::{
    anthropic_error, openai_error_to_anthropic, openai_response_to_anthropic,
    openai_stream_to_anthropic_for_model,
};
#[cfg(test)]
use anthropic::{openai_stream_to_anthropic, AnthropicSseAdapter};

#[path = "server/health_selection.rs"]
mod health_selection;
#[cfg(test)]
use health_selection::{account_health_key, is_cooling_down};
use health_selection::{
    candidate_is_cooling_down, prioritized_candidates, resource_key, source_health_key,
};

#[derive(Default)]
struct StreamCompletion {
    parser: crate::services::account_proxy::SseParser,
    headers: HeaderMap,
    protocol: RouteProtocol,
    response_id: Option<String>,
    failure: Option<crate::services::account_proxy::ProxyError>,
    meaningful: bool,
    last_progress: Option<tokio::time::Instant>,
    accounted: bool,
    legitimate_empty: bool,
    finish_reason: bool,
    done: bool,
}

impl StreamCompletion {
    fn push(&mut self, bytes: &[u8]) -> Result<(), std::io::Error> {
        let frames = self.parser.push(bytes);
        self.observe(frames)
    }

    fn observe(&mut self, frames: Vec<(String, String)>) -> Result<(), std::io::Error> {
        for (event, data) in frames {
            if self.done || data.trim().is_empty() {
                continue;
            }
            if data.trim() == "[DONE]" {
                if self.protocol == RouteProtocol::Chat {
                    self.done = true;
                    self.require_output()?;
                }
                continue;
            }
            let value: Value = serde_json::from_str(&data)
                .map_err(|_| std::io::Error::other("Invalid upstream event stream"))?;
            if let Some(error) =
                crate::services::account_proxy::stream_error(&event, &data, &self.headers)
            {
                if self.protocol == RouteProtocol::Responses {
                    self.failure = Some(error);
                    self.done = true;
                    continue;
                }
                return Err(std::io::Error::other(error));
            }
            if self.protocol == RouteProtocol::Responses {
                let kind = value["type"].as_str().unwrap_or(&event);
                let meaningful = matches!(
                    kind,
                    "response.output_text.delta"
                        | "response.reasoning_summary_text.delta"
                        | "response.reasoning_text.delta"
                        | "response.refusal.delta"
                        | "response.audio.delta"
                        | "response.output_audio.delta"
                        | "response.audio_transcript.delta"
                        | "response.output_audio_transcript.delta"
                        | "response.function_call_arguments.delta"
                        | "response.custom_tool_call_input.delta"
                ) && nonempty_text(&value["delta"]);
                let meaningful = meaningful
                    || output_item_has_content(&value["item"])
                    || content_part_has_content(&value["part"]);
                if meaningful {
                    self.meaningful = true;
                    self.last_progress = Some(tokio::time::Instant::now());
                }
                self.accounted |= positive_token_usage(&value["response"]["usage"]);
                if matches!(kind, "response.completed" | "response.incomplete") {
                    if !value["response"]["error"].is_null() {
                        self.failure = crate::services::account_proxy::stream_error(
                            "response.failed",
                            &data,
                            &self.headers,
                        );
                        self.done = true;
                        continue;
                    }
                    let expected_status = kind.trim_start_matches("response.");
                    if (!event.is_empty() && event != kind)
                        || value["response"]["object"] != "response"
                        || value["response"]["status"] != expected_status
                        || value["response"]["id"]
                            .as_str()
                            .is_none_or(|id| id.is_empty())
                    {
                        return Err(std::io::Error::other(
                            "Responses terminal event has no matching terminal response",
                        ));
                    }
                    self.done = true;
                    self.response_id = value["response"]["id"].as_str().map(str::to_owned);
                    self.meaningful |= response_has_content(&value["response"]);
                    self.legitimate_empty |= legitimate_empty_response(&value["response"]);
                    self.require_output()?;
                }
                continue;
            }
            self.accounted |= positive_token_usage(&value["usage"]);
            if chat_finished_with_error(&value) {
                return Err(std::io::Error::other(upstream_finish_error()));
            }
            if let Some(choices) = value["choices"].as_array() {
                for choice in choices {
                    if chat_message_has_content(&choice["delta"]) {
                        self.meaningful = true;
                        self.last_progress = Some(tokio::time::Instant::now());
                    }
                    self.legitimate_empty |= matches!(
                        choice["finish_reason"].as_str(),
                        Some("length" | "content_filter")
                    );
                }
            }
            self.finish_reason |=
                value
                    .get("choices")
                    .and_then(Value::as_array)
                    .is_some_and(|choices| {
                        choices.iter().any(|choice| {
                            choice
                                .get("finish_reason")
                                .and_then(Value::as_str)
                                .is_some_and(|reason| !reason.is_empty())
                        })
                    });
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), std::io::Error> {
        let frames = self.parser.finish();
        self.observe(frames)?;
        if self.done || self.finish_reason {
            if self.failure.is_some() {
                Ok(())
            } else {
                self.require_output()
            }
        } else {
            Err(std::io::Error::other(
                "Upstream stream ended before completion",
            ))
        }
    }

    fn require_output(&self) -> Result<(), std::io::Error> {
        if self.meaningful || self.accounted || self.legitimate_empty {
            Ok(())
        } else {
            Err(std::io::Error::other(empty_response_error()))
        }
    }
}

async fn preflight_stream<S>(
    first_chunk: Bytes,
    upstream: &mut S,
    completion: &mut StreamCompletion,
    preserve_native_failure: bool,
) -> Result<VecDeque<Bytes>, axum::Error>
where
    S: futures_util::Stream<Item = Result<Bytes, axum::Error>> + Unpin,
{
    let mut buffered = VecDeque::new();
    let mut buffered_bytes = 0usize;
    let mut first = Some(first_chunk);
    loop {
        let chunk = if let Some(chunk) = first.take() {
            chunk
        } else {
            match upstream.next().await {
                Some(chunk) => chunk?,
                None => break,
            }
        };
        if chunk.is_empty() {
            continue;
        }
        completion.push(&chunk).map_err(axum::Error::new)?;
        if let Some(error) = &completion.failure {
            if !preserve_native_failure
                || crate::services::account_proxy::context_limit_error(
                    error.error_code.as_deref(),
                    &error.message,
                )
            {
                return Err(axum::Error::new(error.clone()));
            }
        }
        buffered_bytes = buffered_bytes.saturating_add(chunk.len());
        buffered.push_back(chunk);
        if completion.meaningful
            || completion.done
            || (completion.finish_reason && (completion.accounted || completion.legitimate_empty))
        {
            return Ok(buffered);
        }
        if buffered_bytes > MAX_STREAM_PREFLIGHT_BYTES {
            return Err(axum::Error::new(std::io::Error::other(
                "Upstream stream sent too much metadata before any output",
            )));
        }
    }
    completion.finish().map_err(axum::Error::new)?;
    if let Some(error) = &completion.failure {
        if !preserve_native_failure
            || crate::services::account_proxy::context_limit_error(
                error.error_code.as_deref(),
                &error.message,
            )
        {
            return Err(axum::Error::new(error.clone()));
        }
    }
    Ok(buffered)
}

#[path = "server/stream_content.rs"]
mod stream_content;
use stream_content::{
    chat_finished_with_error, chat_message_has_content, content_part_has_content,
    empty_chat_response, empty_completed_response, empty_response_error, legitimate_empty_response,
    nonempty_text, output_item_has_content, positive_token_usage, response_has_content,
    stream_failure_event, stream_timeout_error, upstream_finish_error,
};

fn proxy_error_from_chain<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> Option<&'a crate::services::account_proxy::ProxyError> {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(proxy) = error.downcast_ref::<crate::services::account_proxy::ProxyError>() {
            return Some(proxy);
        }
        cause = error
            .downcast_ref::<std::io::Error>()
            .and_then(|error| {
                error
                    .get_ref()
                    .map(|inner| inner as &(dyn std::error::Error + 'static))
            })
            .or_else(|| error.source());
    }
    None
}

#[path = "server/health_transitions.rs"]
mod health_transitions;
pub(crate) use health_transitions::forget_account_authentication;
#[cfg(test)]
use health_transitions::{begin_attempt, clear_account_authentication, mark_failure_scoped};
use health_transitions::{
    bind_attempt_credentials, mark_failure, mark_stream_failure, mark_success,
    mark_upstream_failure, try_begin_attempt,
};

#[path = "server/failure_policy.rs"]
mod failure_policy;
#[cfg(test)]
use failure_policy::{classify_failure, cooldown_for_failure};
use failure_policy::{
    is_context_limit_body, remaining_timeout, retry_after, shortened_first_content_timeout,
};

fn chat_completions_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    if base_url.ends_with("/chat/completions") {
        base_url.to_string()
    } else {
        format!("{base_url}/chat/completions")
    }
}

fn responses_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/responses") {
        return base.to_string();
    }
    let base = base.strip_suffix("/chat/completions").unwrap_or(base);
    format!("{base}/responses")
}

#[path = "server/response_bindings.rs"]
mod response_bindings;
use response_bindings::{
    has_previous_response, is_continuation_failure, mark_response_failure, remember_response,
    remember_unsupported_responses, response_candidates, responses_endpoint_unsupported,
};

fn copy_response_headers(response: &mut Response, headers: &HeaderMap) {
    for (name, value) in headers {
        if name != header::CONTENT_LENGTH
            && name != header::TRANSFER_ENCODING
            && name != header::CONNECTION
            && name != header::CONTENT_ENCODING
            && name != "x-echobird-routed-via"
        {
            response.headers_mut().insert(name.clone(), value.clone());
        }
    }
}

fn is_authorized(headers: &HeaderMap, expected_key: &str) -> bool {
    if expected_key.trim().is_empty() {
        return false;
    }
    let bearer_matches = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|key| key == expected_key);
    bearer_matches
        || headers
            .get("x-api-key")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|key| key == expected_key)
}

fn routed_via(candidate: &Candidate) -> HeaderValue {
    let value = format!("{}/{}", candidate.internal_id, candidate.model_id);
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("echobird/unknown"))
}

fn upstream_response(
    status: StatusCode,
    content_type: Option<HeaderValue>,
    cache_control: Option<HeaderValue>,
    body: Bytes,
    candidate: &Candidate,
) -> Response {
    response_with_headers(
        status,
        content_type,
        cache_control,
        Body::from(body),
        candidate,
    )
}

fn streaming_response(
    content_type: Option<HeaderValue>,
    cache_control: Option<HeaderValue>,
    body: Body,
    candidate: &Candidate,
) -> Response {
    response_with_headers(StatusCode::OK, content_type, cache_control, body, candidate)
}

fn response_with_headers(
    status: StatusCode,
    content_type: Option<HeaderValue>,
    cache_control: Option<HeaderValue>,
    body: Body,
    candidate: &Candidate,
) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header("X-EchoBird-Routed-Via", routed_via(candidate));
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(cache_control) = cache_control {
        response = response.header(header::CACHE_CONTROL, cache_control);
    }
    response.body(body).unwrap_or_else(|_| {
        openai_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build router response",
        )
    })
}

fn openai_error(status: StatusCode, message: &str) -> Response {
    json_error(status, message, None)
}

fn json_error(status: StatusCode, message: &str, attempts: Option<Value>) -> Response {
    let mut error = json!({
        "message": message,
        "type": if status == StatusCode::PAYLOAD_TOO_LARGE { "request_too_large" } else { "echobird_router_error" },
        "code": status.as_u16(),
    });
    if let Some(attempts) = attempts {
        error["attempts"] = attempts;
    }
    (status, Json(json!({ "error": error }))).into_response()
}

#[cfg(test)]
#[path = "responses_tests.rs"]
mod responses_tests;

#[cfg(test)]
#[path = "audit_tests.rs"]
mod audit_tests;

#[cfg(test)]
#[path = "concurrency_tests.rs"]
mod concurrency_tests;

#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;
