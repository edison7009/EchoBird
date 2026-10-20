//! Anonymous Search MCP for the built-in agent. Conversation identity is supplied
//! by AgentSession, independently of the MCP connection and its transport session.

use std::time::Duration;

use reqwest::header::{HeaderValue, USER_AGENT};
use rmcp::model::{CallToolRequestParams, ClientCapabilities, ClientConfig, Implementation};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use super::agent_tools::ToolResult;
use super::llm_client::ToolDef;

const ENDPOINT: &str = "https://search.parallel.ai/mcp";
const USER_AGENT_VALUE: &str = concat!(
    "EchoBird/",
    env!("CARGO_PKG_VERSION"),
    " (+https://echobird.ai)"
);
const MAX_OUTPUT_BYTES: usize = 25_000;

#[derive(Clone)]
pub struct SearchContext {
    pub enabled: bool,
    pub session_id: String,
    pub model_name: String,
    pub cancel: CancellationToken,
}

pub fn tool_definitions(enabled: bool) -> Vec<ToolDef> {
    if !enabled {
        return Vec::new();
    }
    vec![
        ToolDef {
            name: "web_search".into(),
            description: "Search the public web with Parallel for current installation guides, documentation and troubleshooting. Returns URLs and relevant excerpts. Prefer official sources; use the excerpts directly when sufficient. Search queries are sent to Parallel. Do not include credentials or private machine details.".into(),
            parameters: json!({"type":"object","properties":{
                "objective":{"type":"string","minLength":1,"maxLength":1000},
                "search_queries":{"type":"array","minItems":1,"maxItems":5,"items":{"type":"string","minLength":1,"maxLength":500}}
            },"required":["objective","search_queries"],"additionalProperties":false}),
        },
        ToolDef {
            name: "parallel_fetch".into(),
            description: "Extract relevant markdown excerpts from public HTTP/HTTPS pages with Parallel Search MCP. Use when web_search excerpts are insufficient or a specific page is needed. URLs and the optional objective are sent to Parallel. For direct fetching, including internal pages, use the existing web_fetch tool.".into(),
            parameters: json!({"type":"object","properties":{
                "urls":{"type":"array","minItems":1,"maxItems":20,"items":{"type":"string"}},
                "objective":{"type":"string","maxLength":200},
                "search_queries":{"type":"array","maxItems":5,"items":{"type":"string","minLength":1,"maxLength":500}}
            },"required":["urls"],"additionalProperties":false}),
        },
    ]
}

pub struct SearchClient {
    endpoint: String,
    timeout: Duration,
}

impl Default for SearchClient {
    fn default() -> Self {
        Self {
            endpoint: ENDPOINT.into(),
            timeout: Duration::from_secs(45),
        }
    }
}

impl SearchClient {
    pub async fn execute(&self, name: &str, args: &str, context: &SearchContext) -> ToolResult {
        if !context.enabled {
            return failure("Web search is disabled in EchoBird settings.");
        }
        let (remote_name, arguments) = match arguments(name, args, context) {
            Ok(value) => value,
            Err(error) => return failure(&error),
        };
        // Each invocation owns its MCP connection. Dropping this bounded future
        // cancels the SDK service/transport, including on an aborted parallel batch.
        // A retry opens a connection but keeps the conversation's session_id.
        tokio::select! {
            biased;
            _ = context.cancel.cancelled() => failure("Cancelled by user"),
            result = tokio::time::timeout(self.timeout, self.call(remote_name, arguments)) => {
                match result {
                    Ok(Ok(result)) => result,
                    Ok(Err(error)) => failure(&format!("Parallel Search MCP: {error}")),
                    Err(_) => failure("Parallel Search MCP timed out after 45 seconds."),
                }
            }
        }
    }

    async fn call(
        &self,
        name: &'static str,
        arguments: Map<String, Value>,
    ) -> Result<ToolResult, String> {
        let mut config = StreamableHttpClientTransportConfig::with_uri(self.endpoint.clone());
        config.allow_stateless = true;
        config
            .custom_headers
            .insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
        let transport = StreamableHttpClientTransport::from_config(config);
        let mut service = ClientConfig::new(
            ClientCapabilities::default(),
            Implementation::new("EchoBird", env!("CARGO_PKG_VERSION")),
        )
        .serve(transport)
        .await
        .map_err(|error| error.to_string())?;
        let tools = service
            .list_tools(Default::default())
            .await
            .map_err(|error| error.to_string())?;
        if !tools.tools.iter().any(|tool| tool.name == name) {
            return Err(format!("server does not advertise {name}"));
        }
        let result = service
            .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
            .await
            .map_err(|error| error.to_string())?;
        let mut output = if let Some(value) = &result.structured_content {
            serde_json::to_string(value).map_err(|error| error.to_string())?
        } else {
            result
                .content
                .iter()
                .filter_map(|content| content.as_text().map(|text| text.text.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        if output.is_empty() {
            return Err("server returned no text or structured content".into());
        }
        let mut success = !result.is_error.unwrap_or(false);
        if let Ok(value) = serde_json::from_str::<Value>(&output) {
            if value["results"].as_array().is_some_and(Vec::is_empty)
                && value["errors"]
                    .as_array()
                    .is_some_and(|errors| !errors.is_empty())
            {
                success = false;
            }
        }
        if output.len() > MAX_OUTPUT_BYTES {
            let mut end = MAX_OUTPUT_BYTES;
            while !output.is_char_boundary(end) {
                end -= 1;
            }
            output.truncate(end);
            output.push_str("\n[Parallel result truncated]");
        }
        // Closing is best-effort; it must not turn a useful result into an error.
        let _ = service.close_with_timeout(Duration::from_secs(1)).await;
        Ok(ToolResult { success, output })
    }
}

fn failure(message: &str) -> ToolResult {
    ToolResult {
        success: false,
        output: message.into(),
    }
}

fn arguments(
    name: &str,
    raw: &str,
    context: &SearchContext,
) -> Result<(&'static str, Map<String, Value>), String> {
    let value: Value =
        serde_json::from_str(raw).map_err(|error| format!("Invalid tool arguments: {error}"))?;
    let mut out = Map::new();
    let remote_name = match name {
        "web_search" => {
            let objective = value["objective"]
                .as_str()
                .filter(|s| !s.trim().is_empty() && s.chars().count() <= 1000)
                .ok_or("objective must be nonempty and at most 1000 characters")?;
            out.insert("objective".into(), json!(objective));
            out.insert(
                "search_queries".into(),
                string_list(&value["search_queries"], 1, 5, 500)?,
            );
            "web_search"
        }
        "parallel_fetch" => {
            let urls = string_list(&value["urls"], 1, 20, 4096)?;
            for raw in urls.as_array().unwrap() {
                let url =
                    url::Url::parse(raw.as_str().unwrap()).map_err(|_| "Invalid fetch URL")?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                {
                    return Err("Fetch URLs must be HTTP/HTTPS without embedded credentials".into());
                }
            }
            out.insert("urls".into(), urls);
            if let Some(objective) = value.get("objective") {
                let text = objective
                    .as_str()
                    .filter(|s| s.chars().count() <= 200)
                    .ok_or("objective must be a string of at most 200 characters")?;
                out.insert("objective".into(), json!(text));
            }
            if let Some(queries) = value.get("search_queries") {
                out.insert("search_queries".into(), string_list(queries, 0, 5, 500)?);
            }
            "web_fetch"
        }
        _ => return Err("Unknown Parallel tool".into()),
    };
    // Identity and model metadata never come from model-generated arguments.
    out.insert("session_id".into(), json!(context.session_id));
    if !context.model_name.is_empty() && context.model_name.chars().count() <= 100 {
        out.insert("model_name".into(), json!(context.model_name));
    }
    Ok((remote_name, out))
}

fn string_list(value: &Value, min: usize, max: usize, length: usize) -> Result<Value, String> {
    let items = value
        .as_array()
        .filter(|items| (min..=max).contains(&items.len()))
        .ok_or_else(|| format!("Expected an array with {min} to {max} entries"))?;
    if items.iter().any(|item| {
        item.as_str().map_or(true, |text| {
            text.trim().is_empty() || text.chars().count() > length
        })
    }) {
        return Err("Array entries must be nonempty strings within the tool's length limit".into());
    }
    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use super::super::{agent_loop::AgentSession, agent_tools, llm_client::*};
    use super::*;
    use axum::{
        extract::State,
        http::{HeaderMap, StatusCode, Uri},
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use std::sync::{Arc, Mutex};

    const QUERY: &str = r#"{"objective":"Find official Node.js installation instructions","search_queries":["Node.js official installation instructions"],"session_id":"model-spoof","model_name":"model-spoof"}"#;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<Value>>>);

    async fn mcp(
        State(capture): State<Capture>,
        uri: Uri,
        headers: HeaderMap,
        Json(request): Json<Value>,
    ) -> Response {
        assert_eq!(uri.path(), "/mcp");
        assert_eq!(headers["user-agent"], USER_AGENT_VALUE);
        assert!(!headers.contains_key("authorization"));
        capture.0.lock().unwrap().push(request.clone());
        let method = request["method"].as_str().unwrap();
        let args = &request["params"]["arguments"];
        if args["objective"] == "wait" {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if args["objective"] == "http-error" {
            return StatusCode::BAD_GATEWAY.into_response();
        }
        if args["objective"] == "invalid-json" {
            return ([("content-type", "application/json")], "broken").into_response();
        }
        let result = match method {
            "initialize" => {
                json!({"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}})
            }
            "notifications/initialized" | "notifications/cancelled" => {
                return StatusCode::ACCEPTED.into_response()
            }
            "tools/list" => json!({"tools":[
                {"name":"web_search","inputSchema":{"type":"object","properties":{}}},
                {"name":"web_fetch","inputSchema":{"type":"object","properties":{}}}
            ]}),
            "tools/call" if args["objective"] == "quota" => {
                json!({"isError":true,"content":[{"type":"text","text":"Free anonymous quota exceeded; retry later"}]})
            }
            "tools/call" => {
                json!({"content":[{"type":"text","text":"source excerpt"}],"structuredContent":{"results":[{"url":"https://nodejs.org/en/download","title":"Download Node.js","excerpts":["Install the current LTS release."]}]}})
            }
            _ => panic!("unexpected MCP request: {method}"),
        };
        let response = json!({"jsonrpc":"2.0","id":request["id"],"result":result});
        if args["objective"] == "sse" {
            (
                [("content-type", "text/event-stream")],
                format!("event: message\ndata: {response}\n\n"),
            )
                .into_response()
        } else {
            Json(response).into_response()
        }
    }

    async fn model(State(capture): State<Capture>, Json(request): Json<Value>) -> Response {
        capture.0.lock().unwrap().push(request);
        ([("content-type", "text/event-stream")], concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Use the official Node.js download instructions.\"}}]}\n\n",
            "data: [DONE]\n\n"
        )).into_response()
    }

    async fn fixture() -> (SearchClient, Capture, tokio::task::JoinHandle<()>) {
        let capture = Capture::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/mcp", post(mcp))
            .route("/v1/chat/completions", post(model))
            .with_state(capture.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (
            SearchClient {
                endpoint,
                timeout: Duration::from_secs(5),
            },
            capture,
            server,
        )
    }

    async fn dispatch(
        client: &SearchClient,
        context: &SearchContext,
        name: &str,
        args: &str,
    ) -> ToolResult {
        agent_tools::execute_tool_with_search(
            name,
            args,
            &Default::default(),
            &[],
            "fixture-conversation",
            (client, context),
        )
        .await
    }

    fn calls(capture: &Capture) -> Vec<Value> {
        capture
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .cloned()
            .collect()
    }

    #[tokio::test]
    async fn parallel_search_dispatch_and_scripted_model_consumption() {
        let (client, capture, server) = fixture().await;
        assert_eq!(
            SearchClient::default().endpoint,
            "https://search.parallel.ai/mcp"
        );
        let tools = agent_tools::get_tool_definitions_with_search(true);
        for name in ["web_search", "parallel_fetch", "web_fetch"] {
            assert!(tools.iter().any(|tool| tool.name == name));
        }
        let mut session = AgentSession::new();
        let context = session.search_context(true, "fixture-model");
        let result = dispatch(&client, &context, "web_search", QUERY).await;
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("https://nodejs.org/en/download"));
        session.messages.push(Message {
            role: "assistant".into(),
            content: MessageContent::Blocks(vec![ContentBlock::ToolUse {
                id: "search-1".into(),
                name: "web_search".into(),
                input: serde_json::from_str(QUERY).unwrap(),
            }]),
        });
        session.push_tool_result("search-1", result.output, LlmProvider::OpenAI);
        let model = LlmClient::new(LlmConfig {
            provider: LlmProvider::OpenAI,
            base_url: client.endpoint.replace("/mcp", "/v1"),
            api_key: "fixture-only".into(),
            model: "fixture-model".into(),
            reasoning_effort: None,
        })
        .unwrap();
        let mut events = model
            .chat_stream(
                &session.messages,
                &tools,
                "Scripted model fixture, not live inference",
            )
            .await
            .unwrap();
        let mut answer = String::new();
        while let Some(event) = events.recv().await {
            if let LlmEvent::TextDelta(text) = event {
                answer.push_str(&text);
            }
        }
        assert!(answer.contains("official Node.js"));
        let requests = capture.0.lock().unwrap();
        let consumed = requests
            .iter()
            .find(|r| r.get("messages").is_some())
            .unwrap();
        assert!(consumed["messages"]
            .to_string()
            .contains("https://nodejs.org/en/download"));
        assert!(consumed["messages"].to_string().contains("search-1"));
        let outgoing = calls_unlocked(&requests);
        assert_eq!(
            outgoing[0]["params"]["arguments"]["session_id"],
            context.session_id
        );
        assert_eq!(
            outgoing[0]["params"]["arguments"]["model_name"],
            "fixture-model"
        );
        server.abort();
    }

    fn calls_unlocked(requests: &[Value]) -> Vec<&Value> {
        requests
            .iter()
            .filter(|r| r["method"] == "tools/call")
            .collect()
    }

    #[tokio::test]
    async fn parallel_search_conversations_followups_restore_reset_and_quota() {
        let (client, capture, server) = fixture().await;
        let mut first = AgentSession::new();
        let second = AgentSession::new();
        let a = first.search_context(true, "model-a");
        let b = second.search_context(true, "model-b");
        assert_ne!(a.session_id, b.session_id);
        let (ra, rb) = tokio::join!(
            dispatch(&client, &a, "web_search", QUERY),
            dispatch(&client, &b, "web_search", QUERY)
        );
        assert!(ra.success && rb.success);
        assert!(
            dispatch(
                &client,
                &a,
                "parallel_fetch",
                r#"{"urls":["https://nodejs.org/en/download"],"objective":"Read instructions"}"#
            )
            .await
            .success
        );
        first.prepare_run().unwrap();
        first.running = false;
        let restored = AgentSession::from_history(
            serde_json::from_value(serde_json::to_value(first.snapshot()).unwrap()).unwrap(),
        );
        assert_eq!(restored.id, a.session_id);
        let again = restored.search_context(true, "model-a");
        assert!(
            dispatch(
                &client,
                &again,
                "web_search",
                &QUERY.replace("Find official Node.js installation instructions", "sse")
            )
            .await
            .success
        );
        let quota = QUERY.replace("Find official Node.js installation instructions", "quota");
        let result = dispatch(&client, &again, "web_search", &quota).await;
        assert!(!result.success && result.output.contains("quota exceeded"));
        assert!(dispatch(&client, &again, "web_search", QUERY).await.success);
        first.reset().unwrap();
        let reset = first.search_context(true, "model-a");
        assert_ne!(reset.session_id, a.session_id);
        assert!(dispatch(&client, &reset, "web_search", QUERY).await.success);
        let observed = calls(&capture);
        assert_eq!(observed.len(), 7);
        let first_ids: Vec<_> = observed[..2]
            .iter()
            .map(|r| r["params"]["arguments"]["session_id"].as_str().unwrap())
            .collect();
        assert!(
            first_ids.contains(&a.session_id.as_str())
                && first_ids.contains(&b.session_id.as_str())
        );
        for request in &observed[2..6] {
            assert_eq!(request["params"]["arguments"]["session_id"], a.session_id);
        }
        assert_eq!(observed[2]["params"]["name"], "web_fetch");
        assert_eq!(
            observed[6]["params"]["arguments"]["session_id"],
            reset.session_id
        );
        server.abort();
    }

    #[tokio::test]
    async fn parallel_search_disabled_invalid_errors_timeout_and_cancellation() {
        let (mut client, capture, server) = fixture().await;
        let session = AgentSession::new();
        let settings: crate::commands::settings_commands::AppSettings =
            serde_json::from_value(json!({"agentWebSearch":false})).unwrap();
        assert_eq!(
            serde_json::to_value(&settings).unwrap()["agentWebSearch"],
            false
        );
        let disabled = session.search_context(settings.agent_search_enabled(), "model");
        let names = agent_tools::get_tool_definitions_with_search(disabled.enabled);
        assert!(names
            .iter()
            .all(|t| t.name != "web_search" && t.name != "parallel_fetch"));
        assert!(names.iter().any(|t| t.name == "web_fetch"));
        assert!(
            !dispatch(&client, &disabled, "web_search", QUERY)
                .await
                .success
        );
        assert!(capture.0.lock().unwrap().is_empty());
        let context = session.search_context(true, "model");
        for raw in [
            "{",
            "{}",
            r#"{"urls":["file:///etc/passwd"]}"#,
            r#"{"urls":["https://user:pass@example.com"]}"#,
        ] {
            assert!(
                !dispatch(&client, &context, "parallel_fetch", raw)
                    .await
                    .success
            );
        }
        assert!(capture.0.lock().unwrap().is_empty());
        for objective in ["http-error", "invalid-json"] {
            let result = dispatch(
                &client,
                &context,
                "web_search",
                &QUERY.replace("Find official Node.js installation instructions", objective),
            )
            .await;
            assert!(!result.success, "{}", result.output);
            assert!(result.output.contains("Parallel Search MCP"));
        }
        client.timeout = Duration::from_millis(500);
        let wait = QUERY.replace("Find official Node.js installation instructions", "wait");
        assert!(dispatch(&client, &context, "web_search", &wait)
            .await
            .output
            .contains("timed out"));
        let before = calls(&capture).len();
        let cancel = context.cancel.clone();
        let pending = dispatch(&client, &context, "web_search", &wait);
        let cancelling = async {
            tokio::time::timeout(Duration::from_secs(1), async {
                while calls(&capture).len() == before {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
            cancel.cancel();
        };
        let (result, _) = tokio::join!(pending, cancelling);
        assert!(!result.success && result.output == "Cancelled by user");
        assert_eq!(
            calls(&capture).last().unwrap()["params"]["arguments"]["session_id"],
            context.session_id
        );
        server.abort();
    }

    #[tokio::test]
    #[ignore = "calls the free anonymous Parallel service"]
    async fn parallel_search_live_anonymous_dispatch() {
        let client = SearchClient::default();
        let session = AgentSession::new();
        let context = session.search_context(true, "");
        let result = dispatch(&client, &context, "web_search", QUERY).await;
        eprintln!("LIVE_SEARCH {} {}", result.success, result.output);
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("https://"));
        let result = dispatch(&client, &context, "parallel_fetch", r#"{"urls":["https://nodejs.org/en/download"],"objective":"Find official Node.js installation options"}"#).await;
        eprintln!("LIVE_FETCH {} {}", result.success, result.output);
        assert!(result.success, "{}", result.output);
        assert!(result.output.contains("nodejs.org"));
    }
}
