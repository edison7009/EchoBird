//! Claude subscription requests run the installed official CLI. Only the caller's
//! function tools are exposed through a per-run, authenticated loopback MCP server.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock, Weak,
    },
    time::Duration,
};

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
    Json, Router,
};
use bytes::Bytes;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot, Mutex, Notify},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use super::protocol::{ChatDecoder, WireProtocol};
use super::{json_response, stream_response, ProxyError};

const WARM_UP_MODEL: &str = "haiku";
const WARM_UP_PROMPT: &str = "hi";
const WARM_UP_TIMEOUT: Duration = Duration::from_secs(60);

type Registry = BTreeMap<String, Weak<Run>>;
static RUNS: OnceLock<Mutex<Registry>> = OnceLock::new();
fn runs() -> &'static Mutex<Registry> {
    RUNS.get_or_init(Mutex::default)
}

enum Output {
    Chunks(Vec<Bytes>),
    Done(Value),
    Error(ProxyError),
}
struct Turn {
    decoder: ChatDecoder,
    output: Option<mpsc::Sender<Output>>,
    partial: bool,
    tool_stop: bool,
    policy: ToolPolicy,
    tool_ids: BTreeSet<String>,
}
impl Turn {
    fn observe_tool(&mut self, block: &Value) -> Result<(), ProxyError> {
        let id = block["id"].as_str().unwrap_or_default();
        if !self
            .policy
            .permits(block["name"].as_str().unwrap_or_default())
            || (!self.policy.parallel && !self.tool_ids.is_empty() && !self.tool_ids.contains(id))
        {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Installed Claude Code did not honor the client tool choice",
            ));
        }
        self.tool_ids.insert(id.into());
        Ok(())
    }
}
struct Run {
    owner: String,
    model: String,
    tools: Vec<Value>,
    secret: String,
    turn: Mutex<Turn>,
    pending: Mutex<BTreeMap<String, oneshot::Sender<Value>>>,
    pending_changed: Notify,
    input: Mutex<Option<Box<dyn AsyncWrite + Send + Unpin>>>,
    controls: Mutex<BTreeMap<String, oneshot::Sender<bool>>>,
    waiting: AtomicBool,
    handoff_delivered: AtomicBool,
    cancel: CancellationToken,
}

#[derive(Clone)]
struct ToolPolicy {
    wire: Value,
    allowed: Vec<usize>,
    forced: bool,
    parallel: bool,
}

impl ToolPolicy {
    fn from_request(body: &Value) -> Result<Self, ProxyError> {
        let tools = body["tools"].as_array().cloned().unwrap_or_default();
        let mut allowed: Vec<_> = (0..tools.len()).collect();
        let choice = body.get("tool_choice").filter(|value| !value.is_null());
        let mut wire = match choice.and_then(Value::as_str) {
            None if choice.is_none() => json!({"type":"auto"}),
            Some("auto") => json!({"type":"auto"}),
            Some("none") => {
                allowed.clear();
                json!({"type":"none"})
            }
            Some("required") if !tools.is_empty() => json!({"type":"any"}),
            None if choice.is_some_and(Value::is_object) => {
                let choice = choice.expect("object choice");
                let name = choice["function"]["name"].as_str().unwrap_or_default();
                allowed.retain(|index| tools[*index]["function"]["name"] == name);
                if choice["type"] != "function" || name.is_empty() || allowed.len() != 1 {
                    return Err(ProxyError::new(
                        StatusCode::BAD_REQUEST,
                        "Claude named tool choice must select one declared function",
                    ));
                }
                json!({"type":"tool","name":format!("mcp__echobird__t{}",allowed[0])})
            }
            _ => {
                return Err(ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Invalid Claude client tool choice",
                ));
            }
        };
        if tools.is_empty() {
            wire = json!({"type":"none"});
        }
        if body["parallel_tool_calls"] == false && wire["type"] != "none" {
            wire["disable_parallel_tool_use"] = json!(true);
        }
        Ok(Self {
            forced: wire["type"] == "any" || wire["type"] == "tool",
            wire,
            allowed,
            parallel: body["parallel_tool_calls"] != false,
        })
    }

    fn extra_body(&self) -> String {
        json!({"tool_choice":self.wire}).to_string()
    }

    fn permits(&self, name: &str) -> bool {
        name.strip_prefix("mcp__echobird__t")
            .and_then(|index| index.parse::<usize>().ok())
            .is_some_and(|index| self.allowed.contains(&index))
    }
}

async fn write_input(run: &Run, value: &Value) -> Result<(), ProxyError> {
    let write = async {
        let mut input = run.input.lock().await;
        let input = input.as_mut().ok_or_else(|| {
            ProxyError::new(StatusCode::BAD_GATEWAY, "Claude session input is closed")
        })?;
        input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .map_err(ProxyError::upstream)
    };
    let result = tokio::select! {
        _=run.cancel.cancelled()=>Err(ProxyError::new(StatusCode::BAD_GATEWAY,"Claude session was canceled")),
        result=tokio::time::timeout(Duration::from_secs(10),write)=>result.unwrap_or_else(|_|Err(ProxyError::new(StatusCode::GATEWAY_TIMEOUT,"Claude session input timed out"))),
    };
    if result.is_err() {
        run.cancel.cancel();
    }
    result
}

async fn configure_turn(
    run: &Run,
    policy: &ToolPolicy,
    users: &[&Value],
) -> Result<(), ProxyError> {
    // The official headless input loop enqueues each SDK user before handling the
    // following control request. Its success ACK follows the synchronous flag/env
    // update. Waiting for that ACK before returning MCP results is the barrier:
    // the next agent request sees both the queued user and this turn's tool choice.
    for user in users {
        let mut input = initial_prompt(&json!({"messages":[user]}));
        // Explicit SDK next priority keeps an in-flight MCP call in the foreground.
        input["priority"] = json!("next");
        write_input(run, &input).await?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let (tx, rx) = oneshot::channel();
    run.controls.lock().await.insert(id.clone(), tx);
    let request = json!({"type":"control_request","request_id":id,"request":{"subtype":"apply_flag_settings","settings":{"env":{"CLAUDE_CODE_EXTRA_BODY":policy.extra_body()}}}});
    let result = async {
        write_input(run, &request).await?;
        let acknowledged = tokio::select! {
            _=run.cancel.cancelled()=>false,
            result=tokio::time::timeout(Duration::from_secs(10),rx)=>matches!(result,Ok(Ok(true))),
        };
        if !acknowledged {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Installed Claude Code did not acknowledge the client tool choice",
            ));
        }
        Ok(())
    }
    .await;
    run.controls.lock().await.remove(&id);
    if result.is_err() {
        // A partially applied queue or policy must never continue under old state.
        run.cancel.cancel();
    }
    result
}

struct ConfigurationGuard<'a> {
    run: &'a Run,
    resumed: bool,
}
impl Drop for ConfigurationGuard<'_> {
    fn drop(&mut self) {
        if !self.resumed {
            self.run.cancel.cancel();
            if let Ok(mut controls) = self.run.controls.try_lock() {
                controls.clear();
            }
        }
    }
}

fn configure_client_tools(
    command: &mut tokio::process::Command,
    tools: &[Value],
    policy: &ToolPolicy,
) {
    command.env("CLAUDE_CODE_EXTRA_BODY", policy.extra_body());
    if !tools.is_empty() {
        command.arg("--allowedTools").arg(
            (0..tools.len())
                .map(|index| format!("mcp__echobird__t{index}"))
                .collect::<Vec<_>>()
                .join(","),
        );
    }
}

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Result<Self, ProxyError> {
        let path = std::env::temp_dir().join(format!("echobird-claude-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(ProxyError::upstream)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(ProxyError::upstream)?;
        }
        Ok(Self(path))
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OwnedChild(
    tokio::process::Child,
    #[cfg(windows)] Option<std::os::windows::io::OwnedHandle>,
);
impl OwnedChild {
    fn new(child: tokio::process::Child) -> Result<Self, String> {
        #[cfg(windows)]
        let tree = crate::utils::process::owned_process_tree(&child)?;
        Ok(Self(
            child,
            #[cfg(windows)]
            Some(tree),
        ))
    }
}
impl std::ops::Deref for OwnedChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl OwnedChild {
    async fn stop(&mut self) {
        #[cfg(windows)]
        drop(self.1.take());
        let _ = self.0.kill().await;
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.1.take());
    }
}

fn owner(credentials: &Value) -> Result<String, ProxyError> {
    let token = credentials["accessToken"]
        .as_str()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::UNAUTHORIZED,
                "Claude account has no OAuth token",
            )
        })?;
    // Only a fingerprint survives after spawning; the token is not a session-store key.
    let identity = credentials["accountId"]
        .as_str()
        .or_else(|| credentials["identity"]["emailAddress"].as_str())
        .unwrap_or(token);
    Ok(hex::encode(Sha256::digest(identity.as_bytes())))
}

fn command(credentials: &Value, dir: &TempDir) -> Result<tokio::process::Command, ProxyError> {
    let executable =
        crate::services::tool_manager::get_tool_exe_path("claudecode").ok_or_else(|| {
            ProxyError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "Install Claude Code before using its account proxy",
            )
        })?;
    Ok(client_command(&executable, credentials, dir))
}

fn client_command(executable: &str, credentials: &Value, dir: &TempDir) -> tokio::process::Command {
    let mut command = crate::utils::process::async_command(executable);
    command
        .current_dir(&dir.0)
        .env("CLAUDE_CONFIG_DIR", &dir.0)
        .env(
            "CLAUDE_CODE_OAUTH_TOKEN",
            credentials["accessToken"].as_str().unwrap_or_default(),
        )
        .env("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1")
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for variable in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "CLAUDECODE",
        "CLAUDE_CODE_ENTRYPOINT",
        "CLAUDE_CODE_SSE_PORT",
        "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_EXTRA_BODY",
    ] {
        command.env_remove(variable);
    }
    command.args([
        "-p",
        "--output-format",
        "stream-json",
        "--input-format",
        "stream-json",
        "--include-partial-messages",
        "--verbose",
        "--permission-mode",
        "default",
        "--tools",
        "",
        "--strict-mcp-config",
        "--setting-sources",
        "",
        "--no-session-persistence",
    ]);
    command
}

/// Whether a finished warm-up turn counts, as an account error key on failure.
fn warm_up_result(value: &Value) -> Result<(), String> {
    if !value["is_error"].as_bool().unwrap_or(false) {
        return Ok(());
    }
    Err(match value["api_error_status"].as_u64() {
        Some(401) => "accountError.loginRequired|HTTP 401".into(),
        Some(403) => "accountError.denied|HTTP 403".into(),
        Some(429) => "accountError.rateLimited|HTTP 429".into(),
        Some(status @ 500..=599) => format!("accountError.unavailable|HTTP {status}"),
        _ => "accountError.failed".into(),
    })
}

/// Sends one short message through the official client so the subscription
/// starts its usage window. The temporary profile leaves the signed-in client alone.
pub(super) async fn warm_up(credentials: &Value, executable: &str) -> Result<(), String> {
    fn failed(_: impl std::fmt::Debug) -> String {
        "accountError.failed".into()
    }
    owner(credentials).map_err(|_| "accountError.loginRequired".to_string())?;
    let dir = TempDir::new().map_err(failed)?;
    let mut command = client_command(executable, credentials, &dir);
    command.args([
        "--model",
        WARM_UP_MODEL,
        "--max-turns",
        "1",
        "--mcp-config",
        "{\"mcpServers\":{}}",
    ]);
    let mut child = OwnedChild::new(command.spawn().map_err(failed)?).map_err(failed)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let prompt = json!({"type":"user","message":{"role":"user","content":WARM_UP_PROMPT},"parent_tool_use_id":null});
    let sent = stdin.write_all(format!("{prompt}\n").as_bytes()).await;
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let read = async {
        sent.map_err(failed)?;
        while let Some(line) = stdout.next_line().await.map_err(failed)? {
            let value: Value = serde_json::from_str(&line).map_err(failed)?;
            if value["type"] == "result" {
                return warm_up_result(&value);
            }
        }
        Err("accountError.failed".to_string())
    };
    let result = tokio::time::timeout(WARM_UP_TIMEOUT, read)
        .await
        .unwrap_or_else(|_| Err("accountError.network".into()));
    child.stop().await;
    result
}

pub(super) async fn list_models(credentials: Value) -> Result<Vec<String>, ProxyError> {
    owner(&credentials)?;
    let dir = TempDir::new()?;
    let mut command = command(&credentials, &dir)?;
    command.args(["--mcp-config", "{\"mcpServers\":{}}"]);
    let mut child = OwnedChild::new(command.spawn().map_err(ProxyError::upstream)?)
        .map_err(ProxyError::upstream)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin.write_all(b"{\"type\":\"control_request\",\"request_id\":\"catalog\",\"request\":{\"subtype\":\"initialize\"}}\n").await.map_err(ProxyError::upstream)?;
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let read = async {
        while let Some(line) = stdout.next_line().await.map_err(ProxyError::upstream)? {
            let value: Value = serde_json::from_str(&line).map_err(ProxyError::upstream)?;
            if value["type"] != "control_response" {
                continue;
            }
            let response = &value["response"];
            if response["subtype"] == "error" {
                return Err(ProxyError::new(
                    StatusCode::UNAUTHORIZED,
                    "Claude Code refused account initialization",
                ));
            }
            let models = response["response"]["models"]
                .as_array()
                .or_else(|| response["models"].as_array());
            let ids: Vec<_> = models
                .into_iter()
                .flatten()
                .filter_map(|model| {
                    model["value"]
                        .as_str()
                        .or_else(|| model["id"].as_str())
                        .or_else(|| model.as_str())
                })
                .filter(|id| !id.is_empty())
                .map(str::to_string)
                .collect();
            if ids.is_empty() {
                return Err(ProxyError::new(
                    StatusCode::BAD_GATEWAY,
                    "Installed Claude Code did not expose its available-model catalog",
                ));
            }
            return Ok(ids);
        }
        Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Claude Code ended before returning its model catalog",
        ))
    };
    let result = tokio::time::timeout(Duration::from_secs(30), read)
        .await
        .unwrap_or_else(|_| {
            Err(ProxyError::new(
                StatusCode::GATEWAY_TIMEOUT,
                "Claude model catalog timed out",
            ))
        });
    child.stop().await;
    result
}

async fn mcp(
    State(run): State<Arc<Run>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.get("authorization").and_then(|v| v.to_str().ok())
        != Some(&format!("Bearer {}", run.secret))
    {
        let mut response = json_response(&json!({"error":"Unauthorized"}));
        *response.status_mut() = StatusCode::UNAUTHORIZED;
        return response;
    }
    if request.get("id").is_none() {
        return Response::builder()
            .status(StatusCode::ACCEPTED)
            .body(Body::empty())
            .expect("fixed response");
    }
    let id = request["id"].clone();
    let result = match request["method"].as_str().unwrap_or_default() {
        "initialize" => {
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"EchoBird","version":"1"}})
        }
        "ping" => json!({}),
        "tools/list" => {
            json!({"tools":run.tools.iter().enumerate().map(|(index,tool)| json!({"name":format!("t{index}"),
            "description":format!("{}: {}",tool["function"]["name"].as_str().unwrap_or("Client tool"),tool["function"]["description"].as_str().unwrap_or_default()),
            "inputSchema":tool["function"].get("parameters").filter(|schema|schema.is_object()).cloned().unwrap_or(json!({"type":"object"}))})).collect::<Vec<_>>()})
        }
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or_default();
            let tool = name
                .strip_prefix('t')
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|index| run.tools.get(index));
            let call_id = request["params"]["_meta"]["claudecode/toolUseId"]
                .as_str()
                .unwrap_or_default();
            let permitted = run
                .turn
                .lock()
                .await
                .policy
                .permits(&format!("mcp__echobird__{name}"));
            if tool.is_none() || call_id.is_empty() || !permitted {
                send(
                    &run,
                    Output::Error(ProxyError::new(
                        StatusCode::BAD_GATEWAY,
                        "Installed Claude Code requested an unavailable client tool callback",
                    )),
                )
                .await;
                run.cancel.cancel();
                return rpc_error(
                    id,
                    -32602,
                    "Claude MCP callback has no matching client tool or call id",
                );
            }
            let (tx, rx) = oneshot::channel();
            run.pending.lock().await.insert(call_id.into(), tx);
            runs()
                .lock()
                .await
                .insert(call_id.into(), Arc::downgrade(&run));
            run.pending_changed.notify_one();
            let value = tokio::select! { _=run.cancel.cancelled()=>return rpc_error(id,-32000,"Proxy session canceled"),value=rx=>value };
            match value {
                Ok(value) => {
                    json!({"content":mcp_content(&value),"isError":value["is_error"].as_bool().unwrap_or(false)})
                }
                Err(_) => return rpc_error(id, -32000, "Client tool result was not returned"),
            }
        }
        _ => return rpc_error(id, -32601, "Method not found"),
    };
    json_response(&json!({"jsonrpc":"2.0","id":id,"result":result}))
}

fn mcp_content(value: &Value) -> Vec<Value> {
    let content = value.get("content").unwrap_or(value);
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .map(|part| {
                if part["type"] == "text" {
                    part.clone()
                } else {
                    json!({"type":"text","text":part.to_string()})
                }
            })
            .collect();
    }
    vec![
        json!({"type":"text","text":content.as_str().map(str::to_string).unwrap_or_else(|| content.to_string())}),
    ]
}
fn rpc_error(id: Value, code: i64, message: &str) -> Response {
    json_response(&json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}))
}

fn initial_prompt(body: &Value) -> Value {
    let history = body["messages"].as_array().cloned().unwrap_or_default();
    let content = if history.len() == 1 && history[0]["role"] == "user" {
        history[0]["content"].clone()
    } else {
        json!(format!("Continue the following conversation. Treat the role-tagged messages as conversation context:\n{}",json!(history)))
    };
    let content = if let Some(parts) = content.as_array() {
        json!(parts.iter().filter_map(|part| {
        if part["type"]=="text" { Some(json!({"type":"text","text":part["text"]})) }
        else if part["type"]=="image_url" { let url=part["image_url"]["url"].as_str()?;
            let data=url.strip_prefix("data:")?;let (mime,bytes)=data.split_once(";base64,")?;
            Some(json!({"type":"image","source":{"type":"base64","media_type":mime,"data":bytes}}))
        } else { None }
    }).collect::<Vec<_>>())
    } else {
        content
    };
    json!({"type":"user","message":{"role":"user","content":content},"parent_tool_use_id":null})
}

fn remap_event(value: &mut Value, tools: &[Value]) {
    let block = &mut value["content_block"];
    if block["type"] == "tool_use" {
        if let Some(index) = block["name"]
            .as_str()
            .and_then(|name| name.strip_prefix("mcp__echobird__t"))
            .and_then(|index| index.parse::<usize>().ok())
        {
            if let Some(tool) = tools.get(index) {
                block["name"] = tool["function"]["name"].clone();
            }
        }
    }
}

async fn send(run: &Run, output: Output) -> bool {
    let sender = run.turn.lock().await.output.clone();
    if let Some(sender) = sender {
        let sent = sender.send(output).await.is_ok();
        if !sent && !run.handoff_delivered.load(Ordering::Acquire) {
            run.cancel.cancel();
        }
        sent
    } else {
        false
    }
}

async fn await_callbacks(run: &Run, collected: &Value) -> Result<(), ProxyError> {
    let ids: Vec<_> = collected["choices"][0]["message"]["tool_calls"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|call| call["id"].as_str())
        .collect();
    let ready = async {
        loop {
            let changed = run.pending_changed.notified();
            let pending = run.pending.lock().await;
            if ids.iter().all(|id| pending.contains_key(*id)) {
                return;
            }
            drop(pending);
            tokio::select! { _=run.cancel.cancelled()=>return,_=changed=>{} }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), ready)
        .await
        .map_err(|_| {
            ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Claude Code did not hand off its requested client tools",
            )
        })?;
    if run.cancel.is_cancelled() {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Claude MCP session was canceled",
        ));
    }
    Ok(())
}

struct RequestGuard(Arc<Run>);
impl Drop for RequestGuard {
    fn drop(&mut self) {
        if !self.0.handoff_delivered.load(Ordering::Acquire) {
            self.0.cancel.cancel();
        }
    }
}

async fn read_event(run: &Run, value: Value) -> Result<bool, ProxyError> {
    if value["type"] == "control_response" {
        let response = &value["response"];
        if let Some(id) = response["request_id"].as_str() {
            if let Some(sender) = run.controls.lock().await.remove(id) {
                let _ = sender.send(response["subtype"] == "success");
            }
        }
        return Ok(false);
    }
    if value["type"] == "stream_event" {
        let mut event = value["event"].clone();
        let mut turn = run.turn.lock().await;
        if event["content_block"]["type"] == "tool_use" {
            turn.observe_tool(&event["content_block"])?;
        }
        remap_event(&mut event, &run.tools);
        turn.partial = true;
        if event["type"] == "message_delta" && event["delta"]["stop_reason"] == "tool_use" {
            turn.tool_stop = true;
        }
        if event["type"] == "message_stop" && !turn.tool_stop {
            return Ok(false);
        }
        let ended = event["type"] == "message_stop";
        if ended {
            run.waiting.store(true, Ordering::Release);
        }
        let chunks = turn
            .decoder
            .decode("", &event.to_string())
            .map_err(ProxyError::upstream)?;
        let collected = ended.then(|| turn.decoder.collected());
        drop(turn);
        if let Some(value) = &collected {
            await_callbacks(run, value).await?;
        }
        if !chunks.is_empty() {
            send(run, Output::Chunks(chunks)).await;
        }
        if let Some(value) = collected {
            send(run, Output::Done(value)).await;
            run.turn.lock().await.output = None;
        }
        return Ok(false);
    }
    if value["type"] == "assistant" {
        let mut turn = run.turn.lock().await;
        if turn.partial {
            return Ok(false);
        }
        let mut chunks = Vec::new();
        for block in value["message"]["content"].as_array().into_iter().flatten() {
            if let Some(text) = block["text"].as_str() {
                chunks.push(turn.decoder.emit_text(text, false));
            }
            if block["type"] == "tool_use" {
                turn.observe_tool(block)?;
                let mut event = json!({"content_block":block});
                remap_event(&mut event, &run.tools);
                let block = &event["content_block"];
                chunks.extend(turn.decoder.emit_tool(
                    block["id"].as_str().unwrap_or_default(),
                    block["id"].as_str().unwrap_or_default(),
                    block["name"].as_str().unwrap_or_default(),
                    &block["input"].to_string(),
                    true,
                ));
                turn.tool_stop = true;
            }
        }
        let done = if turn.tool_stop {
            run.waiting.store(true, Ordering::Release);
            chunks.extend(turn.decoder.complete("tool_calls"));
            Some(turn.decoder.collected())
        } else {
            None
        };
        drop(turn);
        if let Some(value) = &done {
            await_callbacks(run, value).await?;
        }
        send(run, Output::Chunks(chunks)).await;
        if let Some(value) = done {
            send(run, Output::Done(value)).await;
            run.turn.lock().await.output = None;
        }
    }
    if value["type"] == "result" {
        if value["is_error"].as_bool().unwrap_or(false) {
            let upstream_status = value["api_error_status"]
                .as_u64()
                .filter(|code| (400..600).contains(code))
                .and_then(|code| StatusCode::from_u16(code as u16).ok());
            if upstream_status.is_none()
                && value["result"]
                    .as_str()
                    .is_some_and(|message| super::context_limit_error(None, message))
            {
                let mut error = ProxyError::new(
                    StatusCode::BAD_REQUEST,
                    "Claude Code rejected a prompt that exceeds its available context window",
                );
                error.error_code = Some("context_length_exceeded".into());
                return Err(error);
            }
            return Err(ProxyError::new(
                upstream_status.unwrap_or(StatusCode::BAD_GATEWAY),
                "Claude Code account inference failed",
            ));
        }
        let mut turn = run.turn.lock().await;
        if turn.policy.forced && !turn.tool_stop {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "Installed Claude Code completed without the required client tool",
            ));
        }
        if turn.decoder.finished {
            return Ok(true);
        }
        if let Some(usage) = value.get("usage") {
            turn.decoder.set_usage(usage, false);
        }
        let chunks = turn.decoder.complete("stop");
        let collected = turn.decoder.collected();
        drop(turn);
        send(run, Output::Chunks(chunks)).await;
        send(run, Output::Done(collected)).await;
        return Ok(true);
    }
    Ok(false)
}

async fn start(
    credentials: &Value,
    model: &str,
    body: &Value,
    tx: mpsc::Sender<Output>,
    policy: ToolPolicy,
) -> Result<Arc<Run>, ProxyError> {
    #[cfg(test)]
    if model == "claude-test" {
        return Err(ProxyError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "A Claude fixture must not start the installed client",
        ));
    }
    let dir = TempDir::new()?;
    let run = Arc::new(Run {
        owner: owner(credentials)?,
        model: model.into(),
        tools: body["tools"].as_array().cloned().unwrap_or_default(),
        secret: uuid::Uuid::new_v4().to_string(),
        turn: Mutex::new(Turn {
            decoder: ChatDecoder::new(WireProtocol::Anthropic, model),
            output: Some(tx),
            partial: false,
            tool_stop: false,
            policy: policy.clone(),
            tool_ids: BTreeSet::new(),
        }),
        pending: Mutex::new(BTreeMap::new()),
        pending_changed: Notify::new(),
        input: Mutex::new(None),
        controls: Mutex::new(BTreeMap::new()),
        waiting: AtomicBool::new(false),
        handoff_delivered: AtomicBool::new(false),
        cancel: CancellationToken::new(),
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(ProxyError::upstream)?;
    let port = listener.local_addr().map_err(ProxyError::upstream)?.port();
    let config = json!({"mcpServers":{"echobird":{"type":"http","url":format!("http://127.0.0.1:{port}/mcp"),"headers":{"Authorization":format!("Bearer {}",run.secret)}}}});
    let config_path = dir.0.join("mcp.json");
    std::fs::write(&config_path, config.to_string()).map_err(ProxyError::upstream)?;
    let mut command = command(credentials, &dir)?;
    command
        .args(["--model", model, "--mcp-config"])
        .arg(&config_path);
    configure_client_tools(&mut command, &run.tools, &policy);
    let systems: Vec<_> = body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "system" || m["role"] == "developer")
        .map(|m| super::protocol::text(&m["content"]))
        .collect();
    if !systems.is_empty() {
        let system_path = dir.0.join("system.txt");
        std::fs::write(&system_path, systems.join("\n\n")).map_err(ProxyError::upstream)?;
        command.arg("--system-prompt-file").arg(system_path);
    }
    let mut child = OwnedChild::new(command.spawn().map_err(ProxyError::upstream)?)
        .map_err(ProxyError::upstream)?;
    *run.input.lock().await = Some(Box::new(child.stdin.take().expect("piped stdin")));
    write_input(&run, &initial_prompt(body)).await?;
    let mut stdout = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let app = Router::new()
        .route("/mcp", post(mcp))
        .with_state(run.clone());
    let canceled = run.cancel.clone();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(canceled.cancelled_owned())
            .await;
    });
    let task_run = run.clone();
    tokio::spawn(async move {
        let _dir = dir;
        // An abandoned tool handoff cannot retain an authenticated process indefinitely.
        let deadline = tokio::time::sleep(Duration::from_secs(600));
        tokio::pin!(deadline);
        loop {
            let next = tokio::select! { _=task_run.cancel.cancelled()=>break,_=&mut deadline=>break,next=stdout.next_line()=>next };
            match next {
                Ok(Some(line)) => match serde_json::from_str(&line).map_err(ProxyError::upstream) {
                    Ok(value) => match read_event(&task_run, value).await {
                        Ok(true) => break,
                        Ok(false) => {}
                        Err(error) => {
                            send(&task_run, Output::Error(error)).await;
                            break;
                        }
                    },
                    Err(error) => {
                        send(&task_run, Output::Error(error)).await;
                        break;
                    }
                },
                Ok(None) => {
                    send(
                        &task_run,
                        Output::Error(ProxyError::new(
                            StatusCode::BAD_GATEWAY,
                            "Claude Code ended before completion",
                        )),
                    )
                    .await;
                    break;
                }
                Err(error) => {
                    send(&task_run, Output::Error(ProxyError::upstream(error))).await;
                    break;
                }
            }
        }
        task_run.cancel.cancel();
        task_run.input.lock().await.take();
        task_run.controls.lock().await.clear();
        child.stop().await;
        server.abort();
        runs().lock().await.retain(|_, value| {
            value
                .upgrade()
                .is_some_and(|run| !Arc::ptr_eq(&run, &task_run))
        });
    });
    Ok(run)
}

pub(super) async fn route(
    credentials: Value,
    model: &str,
    body: Value,
) -> Result<Response, ProxyError> {
    super::protocol::reject_structured_output(&body)?;
    let owner = owner(&credentials)?;
    let policy = ToolPolicy::from_request(&body)?;
    let (tx, mut rx) = mpsc::channel(16);
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    // Earlier completed tool results are history. Only the last assistant's
    // subsequent tool results resume its pending callbacks; user messages beside
    // those results are real SDK input, never text inside an MCP tool result.
    let last_assistant = messages
        .iter()
        .rposition(|message| message["role"] == "assistant");
    let tail = &messages[last_assistant.map_or(0, |index| index + 1)..];
    let results: Vec<_> = tail.iter().filter(|m| m["role"] == "tool").collect();
    let users: Vec<_> = tail.iter().filter(|m| m["role"] == "user").collect();
    if results.is_empty()
        && last_assistant.is_some_and(|index| {
            messages[index]["tool_calls"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty())
        })
    {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Return every pending Claude tool result together",
        ));
    }
    let run = if let Some(first) = results.first() {
        let id = first["tool_call_id"].as_str().unwrap_or_default();
        let run = runs()
            .lock()
            .await
            .get(id)
            .and_then(Weak::upgrade)
            .ok_or_else(|| {
                ProxyError::new(
                    StatusCode::CONFLICT,
                    "Claude tool session expired; restart the conversation",
                )
            })?;
        if run.owner != owner
            || run.model != model
            || run.tools != body["tools"].as_array().cloned().unwrap_or_default()
        {
            return Err(ProxyError::new(
                StatusCode::CONFLICT,
                "Claude tool results belong to a different account, model, or tool set",
            ));
        }
        let mut pending = run.pending.lock().await;
        let ids: BTreeSet<_> = results
            .iter()
            .map(|message| message["tool_call_id"].as_str().unwrap_or_default())
            .collect();
        if ids.len() != results.len()
            || results.len() != pending.len()
            || results
                .iter()
                .any(|m| !pending.contains_key(m["tool_call_id"].as_str().unwrap_or_default()))
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Return every pending Claude tool result together",
            ));
        }
        if tail
            .iter()
            .any(|message| message["role"] != "tool" && message["role"] != "user")
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Claude tool continuation only accepts tool results and user messages",
            ));
        }
        // Only this validated continuation owns the pending callbacks. A second
        // request canceled while waiting for the pending lock must not stop it.
        let mut guard = ConfigurationGuard {
            run: &run,
            resumed: false,
        };
        configure_turn(&run, &policy, &users).await?;
        {
            let mut turn = run.turn.lock().await;
            *turn = Turn {
                decoder: ChatDecoder::new(WireProtocol::Anthropic, model),
                output: Some(tx),
                partial: false,
                tool_stop: false,
                policy,
                tool_ids: BTreeSet::new(),
            };
        }
        run.waiting.store(false, Ordering::Release);
        run.handoff_delivered.store(false, Ordering::Release);
        for result in results {
            let id = result["tool_call_id"].as_str().unwrap_or_default();
            if let Some(sender) = pending.remove(id) {
                let _ = sender.send(result.clone());
            }
            runs().lock().await.remove(id);
        }
        drop(pending);
        guard.resumed = true;
        drop(guard);
        run
    } else {
        start(&credentials, model, &body, tx, policy).await?
    };
    if !body["stream"].as_bool().unwrap_or(false) {
        let _guard = RequestGuard(run.clone());
        while let Some(output) = rx.recv().await {
            match output {
                Output::Done(value) => {
                    if run.waiting.load(Ordering::Acquire) {
                        run.handoff_delivered.store(true, Ordering::Release);
                    }
                    return Ok(json_response(&value));
                }
                Output::Error(error) => {
                    run.cancel.cancel();
                    return Err(error);
                }
                Output::Chunks(_) => {}
            }
        }
        run.cancel.cancel();
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "Claude stream ended before completion",
        ));
    }
    let (tx, output) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        loop {
            let item = tokio::select! { _=tx.closed()=>{ if !run.handoff_delivered.load(Ordering::Acquire) { run.cancel.cancel(); }return; },item=rx.recv()=>item };
            match item {
                Some(Output::Chunks(chunks)) => {
                    for chunk in chunks {
                        let is_done = chunk.as_ref() == b"data: [DONE]\n\n";
                        if tx.send(Ok(chunk)).await.is_err() {
                            if !run.handoff_delivered.load(Ordering::Acquire) {
                                run.cancel.cancel();
                            }
                            return;
                        }
                        if is_done && run.waiting.load(Ordering::Acquire) {
                            run.handoff_delivered.store(true, Ordering::Release);
                        }
                    }
                }
                Some(Output::Done(_)) => return,
                Some(Output::Error(error)) => {
                    let _ = tx.send(Err(std::io::Error::other(error))).await;
                    return;
                }
                None => return,
            }
        }
    });
    Ok(stream_response(Body::from_stream(ReceiverStream::new(
        output,
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unsupported_structured_output_is_rejected_without_starting_cli() {
        let credentials = json!({"accessToken":"fixture","accountId":"fixture-owner"});
        for format in [
            json!({"type":"json_object"}),
            json!({"type":"json_schema","json_schema":{"name":"answer","schema":{"type":"object"},"strict":true}}),
        ] {
            let error = route(
                credentials.clone(),
                "claude-test",
                json!({"messages":[{"role":"user","content":"Answer"}],"response_format":format}),
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.error_code.as_deref(), Some("unsupported_parameter"));
            assert!(error.model_scope);
        }
        let error = route(credentials, "claude-test", json!({"messages":[{"role":"user","content":"Answer"}],"response_format":{"type":"text"}})).await.unwrap_err();
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_cmd_cli_launch_uses_the_shared_background_command() {
        let dir = TempDir::new().unwrap();
        let script = dir.0.join("cli.cmd");
        std::fs::write(
            &script,
            "@echo off\r\necho fixture-background-cli\r\nexit /b 7\r\n",
        )
        .unwrap();
        let output = crate::utils::process::async_command(&script)
            .output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "fixture-background-cli"
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn canceled_windows_cmd_kills_its_owned_heartbeat_child() {
        let dir = TempDir::new().unwrap();
        let script = dir.0.join("parent.cmd");
        let heartbeat_script = dir.0.join("heartbeat.cmd");
        let heartbeat = dir.0.join("heartbeat.txt");
        std::fs::write(
            &heartbeat_script,
            "@echo off\r\nsetlocal\r\nset /a heartbeat=0 > nul\r\n:heartbeat\r\nset /a heartbeat+=1 > nul\r\n> \"%~dp0heartbeat.txt\" echo %heartbeat%\r\ngoto heartbeat\r\n",
        )
        .unwrap();
        std::fs::write(
            &script,
            "@echo off\r\nstart \"\" /b \"%ComSpec%\" /d /c \"\"%~dp0heartbeat.cmd\"\"\r\nping -t 127.0.0.1 > nul\r\n",
        )
        .unwrap();
        let mut child = OwnedChild::new(
            crate::utils::process::async_command(&script)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .unwrap(),
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut first = None;
            loop {
                if let Some(value) = std::fs::read_to_string(&heartbeat)
                    .ok()
                    .and_then(|text| text.trim().parse::<u64>().ok())
                {
                    match first {
                        Some(first) if value > first => break,
                        None => first = Some(value),
                        _ => {}
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("positive control: child must write and advance an integer heartbeat");
        child.stop().await;
        let stopped = std::fs::read_to_string(&heartbeat).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            std::fs::read_to_string(&heartbeat).unwrap(),
            stopped,
            "owned descendant survived cancellation"
        );
    }

    #[tokio::test]
    #[ignore = "requires an installed official Claude Code; uses an invalid fixture token and no inference"]
    async fn installed_cli_initialize_catalog_probe() {
        let models =
            list_models(json!({"accessToken":"fixture-invalid-oauth","accountId":"fixture"})).await;
        match models {
            Ok(models) => assert!(!models.is_empty()),
            Err(error) => panic!(
                "Claude capability probe: {} {}",
                error.status, error.message
            ),
        }
    }

    #[test]
    fn remaps_only_the_bridge_tool_name() {
        let tools = vec![json!({"function":{"name":"read_file"}})];
        let mut event =
            json!({"content_block":{"type":"tool_use","id":"call","name":"mcp__echobird__t0"}});
        remap_event(&mut event, &tools);
        assert_eq!(event["content_block"]["name"], "read_file");
        assert_eq!(event["content_block"]["id"], "call");
    }

    fn fixture_run(
        tools: Vec<Value>,
        choice: Value,
    ) -> (Arc<Run>, mpsc::Receiver<Output>, tokio::io::DuplexStream) {
        let credentials = json!({"accessToken":"fixture","accountId":"fixture-owner"});
        let (tx, rx) = mpsc::channel(16);
        let (input, reader) = tokio::io::duplex(16384);
        let policy =
            ToolPolicy::from_request(&json!({"tools":tools,"tool_choice":choice})).unwrap();
        let run = Arc::new(Run {
            owner: owner(&credentials).unwrap(),
            model: "claude-test".into(),
            tools,
            secret: "fixture-secret".into(),
            turn: Mutex::new(Turn {
                decoder: ChatDecoder::new(WireProtocol::Anthropic, "claude-test"),
                output: Some(tx),
                partial: false,
                tool_stop: false,
                policy,
                tool_ids: BTreeSet::new(),
            }),
            pending: Mutex::new(BTreeMap::new()),
            pending_changed: Notify::new(),
            input: Mutex::new(Some(Box::new(input))),
            controls: Mutex::new(BTreeMap::new()),
            waiting: AtomicBool::new(false),
            handoff_delivered: AtomicBool::new(false),
            cancel: CancellationToken::new(),
        });
        (run, rx, reader)
    }

    async fn next_frame(input: &mut tokio::io::Lines<BufReader<tokio::io::DuplexStream>>) -> Value {
        let line = tokio::time::timeout(Duration::from_secs(5), input.next_line())
            .await
            .expect("the production continuation did not write to the same session")
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }

    async fn resume_tool_fixture(
        initial_choice: Value,
        stream: bool,
        with_users: bool,
        error_status: Option<u16>,
    ) {
        let credentials = json!({"accessToken":"fixture","accountId":"fixture-owner"});
        let tools = vec![
            json!({"type":"function","function":{"name":"another_function","parameters":{"type":"object"}}}),
            json!({"type":"function","function":{"name":"read_file","description":"Read a file","parameters":{"type":"object"}}}),
        ];
        let (run, mut rx, input) = fixture_run(tools.clone(), initial_choice);
        let mut input = BufReader::new(input).lines();
        let call_id = format!("fixture-{}", uuid::Uuid::new_v4());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/mcp", post(mcp))
            .with_state(run.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = reqwest::Client::new();
        let denied = client
            .post(&url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
        let listed = client
            .post(&url)
            .bearer_auth("fixture-secret")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert_eq!(listed["result"]["tools"][1]["name"], "t1");
        read_event(&run,json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":call_id,"name":"mcp__echobird__t1","input":{}}}})).await.unwrap();
        read_event(&run,json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"test.txt\"}"}}})).await.unwrap();
        read_event(&run,json!({"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"}}})).await.unwrap();
        let callback_id = call_id.clone();
        let callback = tokio::spawn(async move {
            client.post(url).bearer_auth("fixture-secret")
            .json(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"t1","arguments":{"path":"test.txt"},"_meta":{"claudecode/toolUseId":callback_id}}}))
            .send().await.unwrap().json::<Value>().await.unwrap()
        });
        read_event(
            &run,
            json!({"type":"stream_event","event":{"type":"message_stop"}}),
        )
        .await
        .unwrap();
        let collected = loop {
            if let Some(Output::Done(value)) = rx.recv().await {
                break value;
            }
        };
        assert_eq!(
            collected["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "read_file"
        );
        assert_eq!(
            collected["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"path\":\"test.txt\"}"
        );
        let result_marker = format!("result-{}", uuid::Uuid::new_v4());
        let mut history = vec![
            json!({"role":"user","content":"Original task"}),
            json!({"role":"assistant","tool_calls":[{"id":"old-completed-call","type":"function","function":{"name":"another_function","arguments":"{}"}}]}),
            json!({"role":"tool","tool_call_id":"old-completed-call","content":"earlier result"}),
            json!({"role":"assistant","tool_calls":collected["choices"][0]["message"]["tool_calls"]}),
            json!({"role":"tool","tool_call_id":call_id,"content":result_marker}),
        ];
        let users = vec![
            json!({"role":"user","content":[{"type":"text","text":"Use the returned marker"},{"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]}),
            json!({"role":"user","content":"Return it exactly, without a tool call."}),
        ];
        if with_users {
            history.extend(users.clone());
        }
        let next_choice = if with_users { "none" } else { "auto" };
        let request =
            json!({"stream":stream,"tools":tools,"tool_choice":next_choice,"messages":history});
        let response = tokio::spawn(route(credentials.clone(), "claude-test", request.clone()));
        if with_users {
            for user in &users {
                let frame = next_frame(&mut input).await;
                let mut expected = initial_prompt(&json!({"messages":[user]}));
                expected["priority"] = json!("next");
                assert_eq!(frame, expected);
                assert_eq!(frame["priority"], "next");
                assert_eq!(frame["message"]["role"], "user");
                if user["content"].is_array() {
                    assert_eq!(
                        frame["message"]["content"],
                        json!([
                            {"type":"text","text":"Use the returned marker"},
                            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AA=="}}
                        ])
                    );
                } else {
                    assert_eq!(
                        frame["message"]["content"],
                        "Return it exactly, without a tool call."
                    );
                }
                assert!(!frame.to_string().contains(&result_marker));
            }
        }
        let control = next_frame(&mut input).await;
        assert_eq!(control["request"]["subtype"], "apply_flag_settings");
        let extra: Value = serde_json::from_str(
            control["request"]["settings"]["env"]["CLAUDE_CODE_EXTRA_BODY"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(extra["tool_choice"]["type"], next_choice);
        assert!(
            !callback.is_finished(),
            "MCP result escaped before the control ACK"
        );
        assert!(run.pending.try_lock().is_err());
        // Dropping another request waiting for this owner's lock cannot cancel it.
        let competing = tokio::spawn(route(credentials, "claude-test", request));
        tokio::task::yield_now().await;
        competing.abort();
        assert!(competing.await.unwrap_err().is_cancelled());
        assert!(!run.cancel.is_cancelled());
        read_event(&run, json!({"type":"control_response","response":{"subtype":"success","request_id":control["request_id"]}})).await.unwrap();
        let result = callback.await.unwrap();
        assert_eq!(result["result"]["content"][0]["text"], result_marker);
        assert!(run.pending.lock().await.is_empty());
        assert!(!run.waiting.load(Ordering::Acquire));
        read_event(&run,json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"continued"}}})).await.unwrap();
        if let Some(status) = error_status {
            use futures_util::StreamExt;
            assert!(stream);
            let error = read_event(
                &run,
                json!({"type":"result","is_error":true,"api_error_status":status}),
            )
            .await
            .unwrap_err();
            send(&run, Output::Error(error)).await;
            let response = response.await.unwrap().unwrap();
            let mut body = response.into_body().into_data_stream();
            let mut delivered = Vec::new();
            let error = loop {
                match body.next().await {
                    Some(Ok(bytes)) => delivered.extend_from_slice(&bytes),
                    Some(Err(error)) => break error,
                    None => panic!("CLI failure became a successful stream EOF"),
                }
            };
            let mut cause: &(dyn std::error::Error + 'static) = &error;
            let error = loop {
                if let Some(error) = cause.downcast_ref::<ProxyError>() {
                    break error;
                }
                cause = cause
                    .downcast_ref::<std::io::Error>()
                    .and_then(|error| {
                        error
                            .get_ref()
                            .map(|inner| inner as &(dyn std::error::Error + 'static))
                    })
                    .or_else(|| cause.source())
                    .expect("CLI failure lost its typed upstream error");
            };
            assert_eq!(error.status.as_u16(), status);
            let delivered = String::from_utf8(delivered).unwrap();
            assert!(delivered.contains("continued"));
            assert!(!delivered.contains("[DONE]"));
            assert!(run.controls.lock().await.is_empty());
            server.abort();
            return;
        }
        read_event(&run, json!({"type":"result","is_error":false}))
            .await
            .unwrap();
        let response = response.await.unwrap().unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .unwrap();
        if stream {
            let text = String::from_utf8(bytes.to_vec()).unwrap();
            assert!(text.contains("continued"));
            assert!(text.ends_with("data: [DONE]\n\n"));
            assert!(!text.contains("tool_calls"));
        } else {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["choices"][0]["message"]["content"], "continued");
            assert!(value["choices"][0]["message"]["tool_calls"].is_null());
        }
        assert!(run.controls.lock().await.is_empty());
        server.abort();
    }

    #[tokio::test]
    async fn mcp_tool_result_resumes_the_same_run_without_executing_a_local_tool() {
        resume_tool_fixture(json!("auto"), false, false, None).await;
    }

    #[tokio::test]
    async fn named_tool_results_and_tail_users_resume_same_run_after_ack() {
        resume_tool_fixture(
            json!({"type":"function","function":{"name":"read_file"}}),
            true,
            true,
            None,
        )
        .await;
    }

    #[tokio::test]
    async fn required_tool_results_reset_choice_to_auto_on_same_run() {
        resume_tool_fixture(json!("required"), false, false, None).await;
    }

    #[tokio::test]
    async fn cli_stream_failures_preserve_authentication_and_rate_limit_status() {
        for status in [401, 429] {
            resume_tool_fixture(json!("auto"), true, false, Some(status)).await;
        }
    }

    #[test]
    fn client_tool_choices_are_strict_and_permissions_only_name_declared_mcp_tools() {
        let tools = vec![
            json!({"type":"function","function":{"name":"first"}}),
            json!({"type":"function","function":{"name":"selected"}}),
        ];
        for (choice, expected, permitted) in [
            (
                json!("auto"),
                json!({"type":"auto","disable_parallel_tool_use":true}),
                vec![0, 1],
            ),
            (json!("none"), json!({"type":"none"}), vec![]),
            (
                json!("required"),
                json!({"type":"any","disable_parallel_tool_use":true}),
                vec![0, 1],
            ),
            (
                json!({"type":"function","function":{"name":"selected"}}),
                json!({"type":"tool","name":"mcp__echobird__t1","disable_parallel_tool_use":true}),
                vec![1],
            ),
        ] {
            let policy = ToolPolicy::from_request(
                &json!({"tools":tools,"tool_choice":choice,"parallel_tool_calls":false}),
            )
            .unwrap();
            assert_eq!(policy.wire, expected);
            assert_eq!(policy.allowed, permitted);
            assert!(!policy.permits("Bash"));
            assert!(!policy.permits("mcp__unrelated__t0"));
            let mut command = tokio::process::Command::new("fixture-never-executed");
            configure_client_tools(&mut command, &tools, &policy);
            let args: Vec<_> = command
                .as_std()
                .get_args()
                .map(|arg| arg.to_str().unwrap())
                .collect();
            assert_eq!(
                args,
                ["--allowedTools", "mcp__echobird__t0,mcp__echobird__t1"]
            );
            let extra = command
                .as_std()
                .get_envs()
                .find(|(key, _)| *key == "CLAUDE_CODE_EXTRA_BODY")
                .unwrap()
                .1
                .unwrap();
            let extra: Value = serde_json::from_str(extra.to_str().unwrap()).unwrap();
            assert_eq!(extra, json!({"tool_choice":expected}));
        }
        for choice in [
            json!("unexpected"),
            json!({"type":"function","function":{"name":"missing"}}),
            json!({"type":"function"}),
        ] {
            assert_eq!(
                ToolPolicy::from_request(&json!({"tools":tools,"tool_choice":choice}))
                    .err()
                    .unwrap()
                    .status,
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            ToolPolicy::from_request(&json!({"tools":[],"tool_choice":"required"}))
                .err()
                .unwrap()
                .status,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn local_context_rejection_is_a_client_error() {
        let (run, _, _) = fixture_run(vec![], json!("none"));
        let error = read_event(
            &run,
            json!({"type":"result","is_error":true,"result":"Prompt is too long"}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.error_code.as_deref(), Some("context_length_exceeded"));
        let other = read_event(
            &run,
            json!({"type":"result","is_error":true,"api_error_status":429,"result":"Prompt is too long"}),
        )
        .await
        .unwrap_err();
        assert_eq!(other.status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn forced_choices_cannot_succeed_as_text_and_none_rejects_callbacks() {
        let tools = vec![json!({"type":"function","function":{"name":"selected"}})];
        for choice in [
            json!("required"),
            json!({"type":"function","function":{"name":"selected"}}),
        ] {
            let (run, mut output, _) = fixture_run(tools.clone(), choice);
            read_event(&run,json!({"type":"assistant","message":{"content":[{"type":"text","text":"unexpected automatic answer"}]}})).await.unwrap();
            let error = read_event(&run, json!({"type":"result","is_error":false}))
                .await
                .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_GATEWAY);
            assert!(
                matches!(output.try_recv(), Ok(Output::Chunks(_))),
                "positive control: the assistant output reached the bridge"
            );
            assert!(
                output.try_recv().is_err(),
                "forced choice emitted a successful completion"
            );
            let error = read_event(
                &run,
                json!({"type":"result","is_error":true,"api_error_status":400}),
            )
            .await
            .unwrap_err();
            assert_eq!(
                error.status,
                StatusCode::BAD_REQUEST,
                "vendor refusal must retain its status"
            );
        }
        let (run, _output, _input) = fixture_run(tools, json!("none"));
        let error = read_event(&run,json!({"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"tool_use","id":"unexpected","name":"mcp__echobird__t0"}}})).await.unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_GATEWAY);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer fixture-secret".parse().unwrap());
        let response = mcp(State(run.clone()),headers,Json(json!({"id":1,"method":"tools/call","params":{"name":"t0","_meta":{"claudecode/toolUseId":"none-forbidden"}}}))).await;
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], -32602);
        assert!(run.pending.lock().await.is_empty());
        assert!(run.cancel.is_cancelled());
    }

    #[tokio::test]
    async fn named_and_serial_choices_reject_wrong_or_additional_tools() {
        let tools = vec![
            json!({"function":{"name":"first"}}),
            json!({"function":{"name":"selected"}}),
        ];
        let (run, _output, _input) = fixture_run(
            tools.clone(),
            json!({"type":"function","function":{"name":"selected"}}),
        );
        let wrong = json!({"type":"stream_event","event":{"type":"content_block_start","content_block":{"type":"tool_use","id":"wrong","name":"mcp__echobird__t0"}}});
        assert_eq!(
            read_event(&run, wrong).await.unwrap_err().status,
            StatusCode::BAD_GATEWAY
        );
        let (run, _output, _input) = fixture_run(tools.clone(), json!("auto"));
        run.turn.lock().await.policy =
            ToolPolicy::from_request(&json!({"tools":tools,"parallel_tool_calls":false})).unwrap();
        let first = json!({"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"first","name":"mcp__echobird__t0"}}});
        read_event(&run, first).await.unwrap();
        let second = json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"second","name":"mcp__echobird__t1","input":{}}]}});
        // Exercise the snapshot path separately from the partial stream path.
        run.turn.lock().await.partial = false;
        assert_eq!(
            read_event(&run, second).await.unwrap_err().status,
            StatusCode::BAD_GATEWAY
        );
    }

    #[tokio::test]
    async fn incomplete_or_duplicate_results_do_not_write_or_release_callbacks() {
        let tools = vec![json!({"function":{"name":"selected"}})];
        let (run, _, input) = fixture_run(tools.clone(), json!("auto"));
        let mut input = BufReader::new(input).lines();
        let first = uuid::Uuid::new_v4().to_string();
        let second = uuid::Uuid::new_v4().to_string();
        let (first_tx, mut first_rx) = oneshot::channel();
        let (second_tx, mut second_rx) = oneshot::channel();
        run.pending.lock().await.insert(first.clone(), first_tx);
        run.pending.lock().await.insert(second.clone(), second_tx);
        runs()
            .lock()
            .await
            .insert(first.clone(), Arc::downgrade(&run));
        for results in [
            json!([{"role":"assistant","tool_calls":[{"id":first,"type":"function","function":{"name":"selected","arguments":"{}"}},{"id":second,"type":"function","function":{"name":"selected","arguments":"{}"}}]},{"role":"user","content":"Both results are missing"}]),
            json!([{"role":"tool","tool_call_id":first,"content":"one"}]),
            json!([{"role":"tool","tool_call_id":first,"content":"one"},{"role":"tool","tool_call_id":first,"content":"duplicated"}]),
        ] {
            let error = route(
                json!({"accessToken":"fixture","accountId":"fixture-owner"}),
                "claude-test",
                json!({"tools":tools,"messages":results}),
            )
            .await
            .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(run.pending.lock().await.len(), 2);
            assert!(matches!(
                first_rx.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                second_rx.try_recv(),
                Err(oneshot::error::TryRecvError::Empty)
            ));
            assert!(!run.cancel.is_cancelled());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(20), input.next_line())
                .await
                .is_err(),
            "invalid results wrote into the CLI session"
        );
        runs().lock().await.remove(&first);
    }

    async fn failed_control_fixture(cancel_request: bool) {
        let tools = vec![json!({"type":"function","function":{"name":"selected"}})];
        let (run, _output, input) = fixture_run(tools.clone(), json!("auto"));
        let mut input = BufReader::new(input).lines();
        let id = uuid::Uuid::new_v4().to_string();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer fixture-secret".parse().unwrap());
        let callback = tokio::spawn(mcp(
            State(run.clone()),
            headers,
            Json(
                json!({"id":1,"method":"tools/call","params":{"name":"t0","_meta":{"claudecode/toolUseId":id}}}),
            ),
        ));
        await_callbacks(
            &run,
            &json!({"choices":[{"message":{"tool_calls":[{"id":id}]}}]}),
        )
        .await
        .unwrap();
        run.waiting.store(true, Ordering::Release);
        run.handoff_delivered.store(true, Ordering::Release);
        let response = tokio::spawn(route(
            json!({"accessToken":"fixture","accountId":"fixture-owner"}),
            "claude-test",
            json!({"tools":tools,"tool_choice":"none","messages":[{"role":"tool","tool_call_id":id,"content":"result-must-not-escape"},{"role":"user","content":"Follow-up user"}]}),
        ));
        let user = next_frame(&mut input).await;
        assert_eq!(user["message"]["content"], "Follow-up user");
        assert_eq!(user["priority"], "next");
        let control = next_frame(&mut input).await;
        assert!(!callback.is_finished());
        assert!(!run.cancel.is_cancelled());
        if cancel_request {
            response.abort();
            assert!(response.await.unwrap_err().is_cancelled());
        } else {
            read_event(&run,json!({"type":"control_response","response":{"subtype":"success","request_id":"unrelated-control"}})).await.unwrap();
            assert_eq!(run.controls.lock().await.len(), 1);
            assert!(!callback.is_finished());
            read_event(&run,json!({"type":"control_response","response":{"subtype":"error","request_id":control["request_id"]}})).await.unwrap();
            assert_eq!(
                response.await.unwrap().unwrap_err().status,
                StatusCode::BAD_GATEWAY
            );
            assert!(run.controls.lock().await.is_empty());
        }
        assert!(
            run.cancel.is_cancelled(),
            "abandoned continuation retained the old delivered handoff"
        );
        assert!(run.controls.lock().await.is_empty());
        let response = callback.await.unwrap();
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error"]["code"], -32000);
        assert!(value.get("result").is_none());
        assert!(run.pending.lock().await.contains_key(&id));
        runs().lock().await.remove(&id);
    }

    #[tokio::test]
    async fn rejected_control_cancels_session_without_releasing_tool_result() {
        failed_control_fixture(false).await;
    }

    #[tokio::test]
    async fn dropped_continuation_cancels_delivered_handoff_while_waiting_for_ack() {
        failed_control_fixture(true).await;
    }

    #[test]
    fn warm_up_reads_the_outcome_from_the_is_error_field() {
        let success = json!({"type":"result","subtype":"success","is_error":false});
        assert!(warm_up_result(&success).is_ok());
        // A failed turn still reports the success subtype.
        let cases = [
            (401, "accountError.loginRequired|HTTP 401"),
            (403, "accountError.denied|HTTP 403"),
            (429, "accountError.rateLimited|HTTP 429"),
            (529, "accountError.unavailable|HTTP 529"),
        ];
        for (status, expected) in cases {
            let failed = json!({"subtype":"success","is_error":true,"api_error_status":status});
            assert_eq!(warm_up_result(&failed).unwrap_err(), expected);
        }
        assert_eq!(
            warm_up_result(&json!({"is_error":true})).unwrap_err(),
            "accountError.failed"
        );
    }

    #[tokio::test]
    async fn warm_up_without_a_token_never_starts_the_client() {
        let error = warm_up(&json!({"accountId":"fixture"}), "fixture-never-executed")
            .await
            .unwrap_err();
        assert_eq!(error, "accountError.loginRequired");
    }

    #[tokio::test]
    async fn fixture_fallback_cannot_spawn_the_installed_claude_client() {
        let error = route(
            json!({"accessToken":"fixture","accountId":"fixture-owner"}),
            "claude-test",
            json!({"messages":[{"role":"user","content":"Fixture fallback"}]}),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            error.message,
            "A Claude fixture must not start the installed client"
        );
    }
}
