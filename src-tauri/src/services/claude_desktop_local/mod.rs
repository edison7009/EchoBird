//! Process-scoped compatibility service for Claude Desktop's local Code identity.
//! The selected inference provider remains owned by `anthropic_proxy`.
use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    Json,
};
#[cfg(not(windows))]
use base64::{engine::general_purpose::STANDARD, Engine};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::rt::TokioIo;
#[cfg(not(windows))]
use rcgen::PublicKeyData;
use serde_json::{json, Value};
#[cfg(not(windows))]
use sha2::{Digest, Sha256};
use std::{
    convert::Infallible,
    path::PathBuf,
    sync::{Arc, OnceLock},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::Mutex,
};
use tokio_rustls::{rustls, TlsAcceptor};
use tokio_util::sync::CancellationToken;

mod assets;
#[cfg(windows)]
mod certificates;
mod localization;

#[cfg(windows)]
pub(crate) async fn ensure_certificate_trusted(path: &std::path::Path) -> Result<(), String> {
    certificates::ensure_trusted(path).await
}

const ACCOUNT: &str = "ec40b17d-31cf-4c81-94a9-000000000001";
const ORGANIZATION: &str = "ec40b17d-31cf-4c81-94a9-000000000002";

#[derive(Clone)]
pub struct LaunchSettings {
    pub port: u16,
    pub ca_path: PathBuf,
    #[cfg(not(windows))]
    pub spki: String,
    pub token: String,
}
struct Running {
    settings: LaunchSettings,
    cancelled: CancellationToken,
    upstream: String,
    assets: Option<Arc<assets::Archive>>,
    locale: String,
}
fn running() -> &'static Mutex<Option<Running>> {
    static RUNNING: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(None))
}
#[derive(Clone)]
struct Context {
    upstream: String,
    token: String,
    client: reqwest::Client,
    tls: TlsAcceptor,
    cancelled: CancellationToken,
    selection: Arc<Mutex<Value>>,
    selection_path: PathBuf,
    assets: Option<Arc<assets::Archive>>,
    locale: String,
}

pub async fn ensure_started(upstream: String) -> Result<LaunchSettings, String> {
    let directory = dirs::home_dir()
        .ok_or("Home directory unavailable")?
        .join(".echobird/claude-desktop-local");
    let (assets, locale) = tokio::task::spawn_blocking(|| {
        Ok::<_, String>((assets::installed()?, localization::native_locale()))
    })
    .await
    .map_err(|error| error.to_string())??;
    ensure_started_at(upstream, directory, assets, locale).await
}

fn local_token(directory: &std::path::Path) -> Result<String, String> {
    use super::native_account_files::{read_optional, write, DirectoryLock};
    let path = directory.join("access-token");
    let _lock = DirectoryLock::acquire(&path)?;
    if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err("Local Code token must be a regular file".into());
    }
    if let Some(bytes) = read_optional(&path)? {
        let token = String::from_utf8(bytes).map_err(|_| "Invalid local Code token")?;
        let suffix = token
            .strip_prefix("echobird-local-")
            .ok_or("Invalid local Code token")?;
        uuid::Uuid::parse_str(suffix).map_err(|_| "Invalid local Code token")?;
        return Ok(token);
    }
    let token = format!("echobird-local-{}", uuid::Uuid::new_v4());
    write(&path, token.as_bytes())?;
    Ok(token)
}

async fn ensure_started_at(
    upstream: String,
    directory: PathBuf,
    assets: Option<Arc<assets::Archive>>,
    locale: String,
) -> Result<LaunchSettings, String> {
    let url = url::Url::parse(&upstream).map_err(|e| e.to_string())?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
        return Err("Claude local inference relay must be loopback HTTP".into());
    }
    let mut guard = running().lock().await;
    if let Some(current) = guard.as_ref() {
        if current.upstream == upstream
            && current.locale == locale
            && current.settings.ca_path.parent() == Some(directory.as_path())
            && current.assets.as_ref().map(|archive| &archive.path)
                == assets.as_ref().map(|archive| &archive.path)
            && !current.cancelled.is_cancelled()
        {
            return Ok(current.settings.clone());
        }
        current.cancelled.cancel();
    }
    super::native_account_files::private_dir(&directory)?;
    #[cfg(windows)]
    let (identity, ca_path) = certificates::load(&directory)?;
    #[cfg(windows)]
    let (certificate, private_key) = (identity.certificate, identity.key);
    #[cfg(not(windows))]
    let certified = rcgen::generate_simple_self_signed(vec![
        "claude.ai".into(),
        "www.claude.ai".into(),
        "claude.com".into(),
        "api.anthropic.com".into(),
        "api.claude.ai".into(),
    ])
    .map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let spki = STANDARD.encode(Sha256::digest(
        certified.signing_key.subject_public_key_info(),
    ));
    #[cfg(not(windows))]
    let ca_path = directory.join("process-ca.pem");
    #[cfg(not(windows))]
    std::fs::write(&ca_path, certified.cert.pem()).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let (certificate, private_key) = (
        certified.cert.der().to_vec(),
        certified.signing_key.serialize_der(),
    );
    let crypto = Arc::new(rustls::crypto::ring::default_provider());
    let tls = rustls::ServerConfig::builder_with_provider(crypto)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.into()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(private_key).into(),
        )
        .map_err(|e| e.to_string())?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let token = local_token(&directory)?;
    let cancelled = CancellationToken::new();
    let context = Context {
        upstream: upstream.clone(),
        token: token.clone(),
        client: reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|e| e.to_string())?,
        tls: TlsAcceptor::from(Arc::new(tls)),
        cancelled: cancelled.clone(),
        selection: Arc::new(Mutex::new(read_selection(&directory.join("selector.json")))),
        selection_path: directory.join("selector.json"),
        assets: assets.clone(),
        locale: locale.clone(),
    };
    tokio::spawn(async move {
        loop {
            let accepted = tokio::select! { biased; _ = context.cancelled.cancelled() => break, accepted = listener.accept() => accepted };
            let (socket, _) = match accepted {
                Ok(connection) => connection,
                Err(error) => {
                    log::warn!("[ClaudeLocal] Listener failed: {error}");
                    context.cancelled.cancel();
                    break;
                }
            };
            let ctx = context.clone();
            tokio::spawn(async move {
                let cancellation = ctx.cancelled.clone();
                let connection = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(socket),
                        service_fn(move |request| proxy(request, ctx.clone())),
                    )
                    .with_upgrades();
                tokio::select! { _ = cancellation.cancelled() => {}, _ = connection => {} }
            });
        }
    });
    let settings = LaunchSettings {
        port,
        ca_path,
        #[cfg(not(windows))]
        spki,
        token,
    };
    *guard = Some(Running {
        settings: settings.clone(),
        cancelled,
        upstream,
        assets,
        locale,
    });
    Ok(settings)
}

pub async fn is_running() -> bool {
    running()
        .lock()
        .await
        .as_ref()
        .is_some_and(|instance| !instance.cancelled.is_cancelled())
}

pub async fn stop() {
    if let Some(current) = running().lock().await.take() {
        current.cancelled.cancel();
    }
}
fn intercepted(host: &str) -> bool {
    matches!(
        host,
        "claude.ai" | "www.claude.ai" | "claude.com" | "api.anthropic.com" | "api.claude.ai"
    )
}
async fn proxy(
    mut request: Request<Incoming>,
    context: Context,
) -> Result<Response<Body>, Infallible> {
    if request.method() != "CONNECT" {
        return Ok(route(request, context, None).await);
    }
    let authority = request
        .uri()
        .authority()
        .map(|v| v.as_str())
        .unwrap_or_default()
        .to_owned();
    let Ok(target) = url::Url::parse(&format!("https://{authority}")) else {
        return Ok(StatusCode::BAD_REQUEST.into_response());
    };
    if target.port_or_known_default() != Some(443) {
        return Ok(StatusCode::FORBIDDEN.into_response());
    }
    let host = target.host_str().unwrap_or_default().to_owned();
    let upgrade = hyper::upgrade::on(&mut request);
    tokio::spawn(async move {
        let Ok(upgraded) = upgrade.await else { return };
        let cancellation = context.cancelled.clone();
        let work = async {
            let mut socket = TokioIo::new(upgraded);
            if intercepted(&host) {
                if let Ok(tls) = context.tls.accept(socket).await {
                    let service = service_fn(move |request| {
                        let ctx = context.clone();
                        let host = host.clone();
                        async move { Ok::<_, Infallible>(route(request, ctx, Some(host)).await) }
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(tls), service)
                        .await;
                }
            } else if let Ok(mut upstream) = TcpStream::connect(&authority).await {
                let _ = tokio::io::copy_bidirectional(&mut socket, &mut upstream).await;
            }
        };
        tokio::select! { _ = cancellation.cancelled() => {}, _ = work => {} }
    });
    Ok(StatusCode::OK.into_response())
}
fn relay_settings() -> Value {
    dirs::home_dir()
        .and_then(|home| std::fs::read(home.join(".echobird/claudedesktop.json")).ok())
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null)
}
fn model_entry(id: &str, name: &str, one_m: bool) -> Value {
    json!({"id":id,"type":"model","name":name,"display_name":name,"description":"Custom API",
        "section":"main","supports_1m":one_m,"supports_1m_context":one_m})
}
fn local_models() -> Vec<Value> {
    models_for_relay(&relay_settings())
}
fn models_for_relay(relay: &Value) -> Vec<Value> {
    let one_m = relay["oneMContext"].as_bool().unwrap_or(false);
    let mut models = vec![model_entry(
        "claude-opus-5-5",
        relay["modelName"].as_str().unwrap_or("EchoBird model"),
        one_m,
    )];
    if let Some(base) = relay["baseUrl"].as_str() {
        if let Ok(routes) = super::smart_router::catalog::models_for_url(base) {
            for route in routes {
                if route.id != "auto" && !models.iter().any(|model| model["id"] == route.id) {
                    models.push(model_entry(&route.id, &route.name, one_m));
                }
            }
        }
    }
    models
}
fn user_access() -> Value {
    json!({"features":[{"feature":"claude_code","status":"available"},
        {"feature":"claude_code_desktop","status":"available"},
        {"feature":"cowork","status":"blocked_by_platform"}]})
}
fn selectable(models: &[Value], selected: &Value) -> bool {
    let Some(id) = selected.as_str() else {
        return false;
    };
    models.iter().any(|model| {
        model["id"] == id
            || id
                .strip_suffix("[1m]")
                .is_some_and(|base| model["id"] == base && model["supports_1m_context"] == true)
    })
}
fn read_selection(path: &std::path::Path) -> Value {
    let saved = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    saved
        .filter(|state| selectable(&local_models(), &state["model"]))
        .unwrap_or_else(|| selector_state()[0].clone())
}
fn patch_selection(
    current: &Value,
    patch: &Value,
    models: &[Value],
) -> Result<Value, &'static str> {
    let fields = patch.as_object().ok_or("Invalid model selection patch")?;
    if fields
        .keys()
        .any(|field| !matches!(field.as_str(), "model" | "thinking"))
    {
        return Err("Unknown model selection field");
    }
    let mut next = current.clone();
    if let Some(model) = fields.get("model") {
        if model.is_null() {
            next["model"] = "claude-opus-5-5".into();
        } else if selectable(models, model) {
            next["model"] = model.clone();
        } else {
            return Err("Model is unavailable");
        }
        next["selection_source"] = "user_setting".into();
    }
    if let Some(thinking) = fields.get("thinking") {
        if thinking.is_null() {
            next.as_object_mut()
                .expect("selection object")
                .remove("thinking");
        } else {
            validate_thinking(thinking)?;
            next["thinking"] = thinking.clone();
        }
    }
    Ok(next)
}

fn validate_thinking(thinking: &Value) -> Result<(), &'static str> {
    let object = thinking
        .as_object()
        .ok_or("Invalid thinking configuration")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("Invalid thinking type")?;
    if !matches!(kind, "none" | "effort" | "mode" | "effort_and_mode")
        || object
            .keys()
            .any(|key| !matches!(key.as_str(), "type" | "effort" | "mode"))
        || matches!(kind, "effort" | "effort_and_mode") != thinking["effort"].is_string()
        || matches!(kind, "mode" | "effort_and_mode") != thinking["mode"].is_string()
    {
        return Err("Invalid thinking configuration");
    }
    Ok(())
}

async fn current_selection(context: &Context) -> Value {
    let selected = context.selection.lock().await;
    if selectable(&local_models(), &selected["model"]) {
        selected.clone()
    } else {
        selector_state()[0].clone()
    }
}
fn selector_config() -> Value {
    use sha2::Digest;
    let models = local_models();
    let revision = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&models).expect("model catalog JSON"))
    );
    json!([{"id":"code","revision":revision,"models":models}])
}
fn selector_state() -> Value {
    json!([{"id":"code","model":"claude-opus-5-5","selection_source":"global_default"}])
}
async fn selections(context: &Context) -> Value {
    json!([current_selection(context).await])
}
fn identity(locale: &str) -> Value {
    let created = "2026-01-01T00:00:00Z";
    let org = json!({"uuid":ORGANIZATION,"id":0,"name":"EchoBird.ai","organization_type":"personal",
        "capabilities":["chat","claude_pro","claude_code"],"billing_type":"stripe_subscription","plan_display_name":"Custom API",
        "claude_ai_bootstrap_models_config":local_models(),"settings":{}});
    let memberships = json!([{"organization":org,"role":"admin","seat_tier":"full_member","created_at":created,"updated_at":created}]);
    let account = json!({"uuid":ACCOUNT,"tagged_id":format!("acct_{ACCOUNT}"),"email_address":"hi@echobird.ai",
        "email":"hi@echobird.ai","full_name":"EchoBird.ai","display_name":"EchoBird.ai",
        "created_at":created,"updated_at":created,"is_verified":true,"age_is_verified":true,
        "accepted_clickwrap_versions":{},"locale":locale,"settings":{"locale":locale},"memberships":memberships,"workspace_memberships":[],"invites":[]});
    let mut features = json!({});
    if locale == "zh-Hans" {
        features["witty_scone_main"] = json!({"defaultValue":{"released":["zh-Hans"]}});
    }
    json!({"account":account,"user":account,"organization":org,"activeOrganization":org,"organizations":[org],
        "memberships":memberships,"resolved_org_uuid":ORGANIZATION,"featureFlagsOrgUuid":ORGANIZATION,
        "current_user_access":user_access(),"locale":locale,"install_id":ACCOUNT,
        "model_selector_config":selector_config(),"model_selector_state":selector_state(),
        "statsig":{"user":{"userID":ACCOUNT},"values":{},"values_hash":"echobird-local"},
        "feature_flags":{"user":{"userID":ACCOUNT},"values":{},"values_hash":"echobird-local"},"growthbook":{"features":features},
        "bootstrap_push_revision":0,"intercom_account_hash":null,"system_prompts":{}})
}
fn oauth_profile(locale: &str) -> Value {
    let bootstrap = identity(locale);
    let mut organization = bootstrap["organization"].clone();
    // The native OAuth parser translates this protocol value to its local Code tier.
    organization["organization_type"] = "claude_pro".into();
    json!({"account":bootstrap["account"],"organization":organization})
}

async fn route(
    request: Request<Incoming>,
    context: Context,
    tunnel_host: Option<String>,
) -> Response<Body> {
    let path = request.uri().path().to_owned();
    if path.contains("bootstrap") || path.contains("model_selector") {
        log::info!("[ClaudeLocal] {} {path}", request.method());
    }
    let host = tunnel_host.or_else(|| request.uri().host().map(str::to_owned));
    if let Some(host) = host.as_deref() {
        if !intercepted(host) {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    if path.starts_with("/api/organizations/")
        && (path
            .split('/')
            .nth(4)
            .is_some_and(|part| matches!(part, "chat_conversations" | "chat_conversations_v2"))
            || path.ends_with("/model_selector/chat")
            || path.ends_with("/model_selector_state/chat"))
    {
        return (
            StatusCode::NOT_IMPLEMENTED,
            Json(json!({"error":{"type":"api_error","message":"API mode supports Claude Code only."}})),
        )
            .into_response();
    }
    if path == "/v1/messages" {
        let authenticated = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == format!("Bearer {}", context.token))
            || request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                == Some(context.token.as_str());
        if !authenticated {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        let Ok(body) = axum::body::to_bytes(Body::new(request.into_body()), 32 * 1024 * 1024).await
        else {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        };
        let result = context
            .client
            .post(format!(
                "{}/v1/messages",
                context.upstream.trim_end_matches('/')
            ))
            .header("authorization", format!("Bearer {}", context.token))
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await;
        return match result {
            Ok(response) => {
                let mut output = Response::builder().status(response.status());
                for header in ["content-type", "retry-after", "cache-control"] {
                    if let Some(value) = response.headers().get(header) {
                        output = output.header(header, value);
                    }
                }
                output
                    .body(Body::from_stream(response.bytes_stream()))
                    .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
            }
            Err(_) => StatusCode::BAD_GATEWAY.into_response(),
        };
    }
    if path == "/v1/oauth/token" {
        return Json(json!({"access_token":context.token,"refresh_token":context.token,"token_type":"Bearer","expires_in":28800,"scope":"user:inference user:profile user:sessions:claude_code"})).into_response();
    }
    if path.contains("/authorize") {
        if request.method() == "GET" {
            return Json(json!({"session_stale_for_elevated_grant":false})).into_response();
        }
        let Ok(body) = axum::body::to_bytes(Body::new(request.into_body()), 65536).await else {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        };
        let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let Ok(mut redirect) = url::Url::parse(
            input["redirect_uri"]
                .as_str()
                .unwrap_or("claude://oauth/callback"),
        ) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let native_https_callback = redirect.scheme() == "https"
            && redirect.username().is_empty()
            && redirect.password().is_none()
            && redirect.port().is_none()
            && matches!(
                (redirect.host_str(), redirect.path()),
                (Some("console.anthropic.com"), "/oauth/code/callback")
                    | (Some("claude.ai"), "/desktop/callback")
            );
        if redirect.scheme() != "claude" && !native_https_callback {
            return StatusCode::BAD_REQUEST.into_response();
        }
        redirect
            .query_pairs_mut()
            .append_pair("code", "echobird-local");
        if let Some(state) = input["state"].as_str() {
            redirect.query_pairs_mut().append_pair("state", state);
        }
        return Json(json!({"code":"echobird-local","redirect_uri":redirect.as_str(),"state":input["state"]})).into_response();
    }
    if path == "/v1/models" {
        return Json(json!({"data":local_models(),"has_more":false})).into_response();
    }
    if path.starts_with("/api/organizations/") && path.contains("/model_selector_state/") {
        if !path.ends_with("/code") {
            return StatusCode::NOT_FOUND.into_response();
        }
        if request.method() != "PATCH" {
            return StatusCode::METHOD_NOT_ALLOWED.into_response();
        }
        let Ok(body) = axum::body::to_bytes(Body::new(request.into_body()), 65536).await else {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        };
        let Ok(patch) = serde_json::from_slice::<Value>(&body) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let mut current = context.selection.lock().await;
        let next =
            match patch_selection(&current, &patch, &local_models()) {
                Ok(next) => next,
                Err(message) => return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":{"type":"model_selector_state_error","message":message}})),
                )
                    .into_response(),
            };
        if super::cursor_auth::write(&context.selection_path, &next).is_err() {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        *current = next.clone();
        return Json(next).into_response();
    }
    if path.starts_with("/api/organizations/") && path.ends_with("/model_selector/code") {
        return Json(json!({"model_selector_config":selector_config(),"model_selector_state":selections(&context).await})).into_response();
    }
    if path.ends_with("/current_user_access") {
        return Json(user_access()).into_response();
    }
    if path.ends_with("/system_prompts") {
        return Json(json!({})).into_response();
    }
    if path.ends_with("/model_selector_config") {
        return Json(selector_config()).into_response();
    }
    if path.ends_with("/model_selector_state") {
        return Json(selections(&context).await).into_response();
    }
    if path == "/api/account" || path == "/api/account_profile" {
        return Json(identity(&context.locale)["account"].clone()).into_response();
    }
    if path == "/api/bootstrap"
        || path == "/edge-api/bootstrap"
        || ((path.starts_with("/api/bootstrap/") || path.starts_with("/edge-api/bootstrap/"))
            && path.ends_with("/app_start"))
    {
        let mut bootstrap = identity(&context.locale);
        if let Some(assets) = &context.assets {
            bootstrap["gated_imports_build_id"] = assets.build_id.clone().into();
        }
        bootstrap["model_selector_state"] = selections(&context).await;
        let mut response = Json(bootstrap).into_response();
        for cookie in [
            format!(
                "sessionKey={}; Path=/; HttpOnly; Secure; SameSite=None",
                context.token
            ),
            format!(
                "lastActiveOrg={ORGANIZATION}; Domain=.claude.ai; Path=/; Secure; SameSite=None"
            ),
        ] {
            if let Ok(value) = cookie.parse() {
                response.headers_mut().append("set-cookie", value);
            }
        }
        return response;
    }
    if path == "/api/oauth/profile" {
        return Json(oauth_profile(&context.locale)).into_response();
    }
    if path.starts_with("/api/claude_code/settings") {
        let mut response=Json(json!({"uuid":ORGANIZATION,"settings":{},"content":{"entries":{}},"checksum":"echobird-local-v1"})).into_response();
        response.headers_mut().insert(
            "etag",
            "\"echobird-local-v1\"".parse().expect("constant header"),
        );
        return response;
    }
    if path.starts_with("/api/claude_code/policy_limits") {
        return Json(json!({"restrictions":[],"compliance_taints":[],"defaults":{}}))
            .into_response();
    }
    if path.starts_with("/api/oauth/organizations/") && path.ends_with("/oauth_tokens") {
        return Json(json!([])).into_response();
    }
    if path == "/api/organizations" {
        return Json(identity(&context.locale)["organizations"].clone()).into_response();
    }
    if path == format!("/api/organizations/{ORGANIZATION}/projects") {
        return if request.method() == "GET" {
            Json(json!([])).into_response()
        } else {
            (StatusCode::NOT_IMPLEMENTED,Json(json!({"error":{"type":"api_error","message":"Hosted projects are unavailable in API mode."}}))).into_response()
        };
    }
    if path.starts_with("/api/organizations/") && path.split('/').count() == 4 {
        return Json(identity(&context.locale)["organization"].clone()).into_response();
    }
    if path.starts_with("/api/") || path.starts_with("/edge-api/") {
        return Json(json!({})).into_response();
    }
    if path.starts_with("/v1/") {
        return (StatusCode::NOT_FOUND, Json(json!({"type":"error","error":{"type":"not_found_error","message":"This local Code endpoint is unavailable"}}))).into_response();
    }
    if matches!(host.as_deref(), Some("claude.ai" | "www.claude.ai"))
        && matches!(request.method().as_str(), "GET" | "HEAD")
    {
        let member = if std::path::Path::new(&path).extension().is_none() {
            "/index.html".to_owned()
        } else {
            path.clone()
        };
        let content_type = assets::content_type(&member);
        let locale = context.locale.clone();
        let Some(assets) = context.assets.clone() else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let resource = tokio::task::spawn_blocking(move || {
            if let Some(catalog) = localization::catalog(&member)? {
                return Ok(Some(catalog));
            }
            let bytes = assets.read(&member)?;
            if content_type.starts_with("text/html") {
                bytes
                    .map(|bytes| localization::html(bytes, &locale))
                    .transpose()
            } else {
                Ok(bytes)
            }
        })
        .await;
        return match resource {
            Ok(Ok(Some(bytes))) => {
                let length = bytes.len();
                let body = if request.method() == "HEAD" {
                    Body::empty()
                } else {
                    Body::from(bytes)
                };
                Response::builder()
                    .header("content-type", content_type)
                    .header("content-length", length)
                    .header("cache-control", "no-cache")
                    .body(body)
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
            }
            Ok(Ok(None)) => StatusCode::NOT_FOUND.into_response(),
            _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }
    let Some(host) = host else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let uri = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_owned();
    match context
        .client
        .get(format!("https://{host}{uri}"))
        .send()
        .await
    {
        Ok(response) => {
            let mut output = Response::builder().status(response.status());
            if let Some(value) = response.headers().get("content-type") {
                output = output.header("content-type", value);
            }
            output
                .body(Body::from_stream(response.bytes_stream()))
                .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

/// Native Code history uses the same local identity as bootstrap and OAuth.
pub(crate) fn history_target() -> Result<PathBuf, String> {
    Ok(super::claude_desktop_accounts::data_dir()?
        .join("claude-code-sessions")
        .join(ACCOUNT)
        .join(ORGANIZATION))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn virtual_identity_exposes_code_model() {
        let bootstrap = identity("en-US");
        let chinese = identity("zh-Hans");
        assert_eq!(chinese["locale"], "zh-Hans");
        assert_eq!(chinese["account"]["settings"]["locale"], "zh-Hans");
        assert_eq!(
            chinese["growthbook"]["features"]["witty_scone_main"]["defaultValue"]["released"],
            json!(["zh-Hans"])
        );
        assert_eq!(bootstrap["account"]["full_name"], "EchoBird.ai");
        assert_eq!(bootstrap["account"]["display_name"], "EchoBird.ai");
        assert_eq!(bootstrap["account"]["email_address"], "hi@echobird.ai");
        assert_eq!(bootstrap["account"]["email"], "hi@echobird.ai");
        assert_eq!(bootstrap["organization"]["name"], "EchoBird.ai");
        assert_eq!(bootstrap["organization"]["plan_display_name"], "Custom API");
        assert_eq!(
            bootstrap["organization"]["billing_type"],
            "stripe_subscription"
        );
        assert_eq!(
            oauth_profile("en-US")["organization"]["organization_type"],
            "claude_pro"
        );
        assert_eq!(
            oauth_profile("en-US")["organization"]["plan_display_name"],
            "Custom API"
        );
        assert_eq!(
            bootstrap["organization"]["claude_ai_bootstrap_models_config"][0]["id"],
            "claude-opus-5-5"
        );
    }

    #[test]
    fn model_patch_preserves_supported_context_and_thinking() {
        let models = vec![model_entry("claude-opus-5-5", "Fixture", true)];
        let current = selector_state()[0].clone();
        let updated = patch_selection(
            &current,
            &json!({"model":"claude-opus-5-5[1m]","thinking":{"type":"effort","effort":"high"}}),
            &models,
        )
        .unwrap();
        assert_eq!(updated["model"], "claude-opus-5-5[1m]");
        assert_eq!(updated["thinking"]["effort"], "high");
        assert!(patch_selection(&current, &json!({"model":"unknown"}), &models).is_err());
        assert!(
            patch_selection(&current, &json!({"thinking":{"type":"effort"}}), &models).is_err()
        );
    }

    #[test]
    fn exposes_router_selectors_and_context_policy() {
        super::super::smart_router::catalog::with_test_models(|| {
            let models = models_for_relay(
                &json!({"baseUrl":"http://127.0.0.1:53683/v1","modelName":"Selected model","oneMContext":true}),
            );
            assert_eq!(models.len(), 3);
            assert_eq!(models[0]["name"], "Selected model");
            assert_eq!(models[1]["name"], "First model");
            assert_eq!(models[1]["supports_1m_context"], true);
            assert!(models[1]["id"]
                .as_str()
                .unwrap()
                .starts_with("claude-fable-echobird-"));
        });
    }

    #[tokio::test]
    async fn local_identity_exchange_and_authenticated_model_relay() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = format!("http://{}", listener.local_addr().unwrap());
        let service = axum::Router::new().route("/v1/messages", post(move |headers: axum::http::HeaderMap, Json(body): Json<Value>| {
            let observed = observed.clone();
            async move {
                assert!(headers["authorization"].to_str().unwrap().starts_with("Bearer echobird-local-"));
                assert!(headers.get("cookie").is_none());
                assert_eq!(body["model"], "fixture-model");
                observed.fetch_add(1, Ordering::SeqCst);
                Json(json!({"type":"message","content":[{"type":"text","text":"fixture reply"}]}))
            }
        }));
        let server = tokio::spawn(async move {
            axum::serve(listener, service).await.unwrap();
        });
        let directory =
            std::env::temp_dir().join(format!("echobird-local-test-{}", uuid::Uuid::new_v4()));
        let settings = ensure_started_at(upstream.clone(), directory.clone(), None, "en-US".into())
            .await
            .unwrap();
        assert_eq!(
            ensure_started_at(upstream.clone(), directory.clone(), None, "en-US".into())
                .await
                .unwrap()
                .port,
            settings.port
        );
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let base = format!("http://127.0.0.1:{}", settings.port);
        let bootstrap = client
            .get(format!("{base}/edge-api/bootstrap"))
            .send()
            .await
            .unwrap();
        let cookies = bootstrap
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(cookies.len(), 2);
        assert!(cookies[0].starts_with(&format!("sessionKey={};", settings.token)));
        assert!(cookies[1].starts_with(&format!("lastActiveOrg={ORGANIZATION};")));
        let bootstrap: Value = bootstrap.json().await.unwrap();
        assert_eq!(bootstrap["account"]["full_name"], "EchoBird.ai");
        assert_eq!(bootstrap["account"]["email_address"], "hi@echobird.ai");
        assert_eq!(bootstrap["organization"]["name"], "EchoBird.ai");
        assert_eq!(
            bootstrap["model_selector_config"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            bootstrap["model_selector_state"].as_array().unwrap().len(),
            1
        );
        let access: Value = client
            .get(format!("{base}/api/bootstrap/device/current_user_access"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(access["features"][0]["feature"], "claude_code");
        assert_eq!(access["features"].as_array().unwrap().len(), 3);
        assert_eq!(
            access["features"][1],
            json!({"feature":"claude_code_desktop","status":"available"})
        );
        assert_eq!(
            access["features"][2],
            json!({"feature":"cowork","status":"blocked_by_platform"})
        );
        assert!(!access["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["feature"] == "chat"));
        assert!(access.get("account").is_none());
        let account: Value = client
            .get(format!("{base}/api/account"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(account["memberships"].is_array());
        assert!(account.get("account").is_none());
        let app_start: Value = client
            .get(format!("{base}/api/bootstrap/device/app_start"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(app_start["model_selector_config"][0]["id"], "code");
        let organization = format!("{base}/api/organizations/{ORGANIZATION}");
        let old_chat = directory.join("chat/old-conversation.json");
        super::super::native_account_files::write(&old_chat, b"saved-chat").unwrap();
        let old_selector = directory.join("chat-selector.json");
        super::super::native_account_files::write(&old_selector, b"saved-selector").unwrap();
        for endpoint in [
            "chat_conversations",
            "chat_conversations_v2",
            "chat_conversations/fixture/completion",
            "model_selector/chat",
            "model_selector_state/chat",
        ] {
            for method in [
                reqwest::Method::GET,
                reqwest::Method::POST,
                reqwest::Method::PATCH,
            ] {
                let rejected = client
                    .request(method, format!("{organization}/{endpoint}"))
                    .json(&json!({"model":"claude-opus-5-5","prompt":"hello"}))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(rejected.status(), StatusCode::NOT_IMPLEMENTED);
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!directory.join("selector.json").exists());
        assert_eq!(std::fs::read(&old_chat).unwrap(), b"saved-chat");
        assert_eq!(std::fs::read(&old_selector).unwrap(), b"saved-selector");
        let code_selection: Value = client
            .patch(format!("{organization}/model_selector_state/code"))
            .json(&json!({"thinking":{"type":"effort","effort":"high"}}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        let code_selector: Value = client
            .get(format!("{organization}/model_selector/code"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            code_selector["model_selector_state"],
            json!([code_selection])
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let profile: Value = client
            .get(format!("{base}/api/oauth/profile"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(profile["organization"]["organization_type"], "claude_pro");
        assert_eq!(profile["organization"]["uuid"], ORGANIZATION);
        for callback in [
            "claude://oauth/callback",
            "https://console.anthropic.com/oauth/code/callback",
            "https://claude.ai/desktop/callback",
        ] {
            let authorization: Value = client
                .post(format!("{base}/v1/oauth/{ORGANIZATION}/authorize"))
                .json(&json!({"redirect_uri":callback,"state":"fixture-state"}))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            let redirect =
                url::Url::parse(authorization["redirect_uri"].as_str().unwrap()).unwrap();
            assert!(redirect
                .query_pairs()
                .any(|(key, value)| key == "code" && value == "echobird-local"));
            assert!(redirect
                .query_pairs()
                .any(|(key, value)| key == "state" && value == "fixture-state"));
        }
        for callback in [
            "https://example.com/oauth/code/callback",
            "https://console.anthropic.com/unexpected",
            "invalid callback",
        ] {
            let rejected = client
                .post(format!("{base}/v1/oauth/{ORGANIZATION}/authorize"))
                .json(&json!({"redirect_uri":callback}))
                .send()
                .await
                .unwrap();
            assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        }
        let token: Value = client
            .post(format!("{base}/v1/oauth/token"))
            .json(&json!({"code":"echobird-local"}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(token["access_token"], settings.token);
        assert!(token["scope"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .any(|scope| scope == "user:sessions:claude_code"));
        let rejected = client
            .post(format!("{base}/v1/messages"))
            .bearer_auth("official-fixture-token")
            .json(&json!({"model":"fixture-model"}))
            .send()
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let certificate =
            reqwest::Certificate::from_pem(&std::fs::read(&settings.ca_path).unwrap()).unwrap();
        let tls_client = reqwest::Client::builder()
            .no_proxy()
            .proxy(reqwest::Proxy::https(&base).unwrap())
            .add_root_certificate(certificate)
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let missing_shell = tls_client
            .get("https://claude.ai/epitaxy")
            .send()
            .await
            .unwrap();
        assert_eq!(missing_shell.status(), StatusCode::NOT_FOUND);
        assert!(!directory.join("web-shell.html").exists());
        assert!(!directory.join("web-assets").exists());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let result: Value = tls_client
            .post("https://api.anthropic.com/v1/messages")
            .bearer_auth(token["access_token"].as_str().unwrap())
            .header("cookie", "official-secret-fixture")
            .json(&json!({"model":"fixture-model","messages":[{"role":"user","content":"hello"}]}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "fixture reply");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        stop().await;
        let restarted = ensure_started_at(upstream, directory.clone(), None, "en-US".into())
            .await
            .unwrap();
        assert_eq!(restarted.token, settings.token);
        let restored: Value = client
            .get(format!(
                "http://127.0.0.1:{}/api/organizations/{ORGANIZATION}/model_selector/code",
                restarted.port
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(restored["model_selector_state"], json!([code_selection]));
        assert_eq!(std::fs::read(old_chat).unwrap(), b"saved-chat");
        assert_eq!(std::fs::read(old_selector).unwrap(), b"saved-selector");
        assert!(directory.join("access-token").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(directory.join("access-token"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        stop().await;
        std::fs::remove_dir_all(directory).unwrap();
        server.abort();
    }
}
