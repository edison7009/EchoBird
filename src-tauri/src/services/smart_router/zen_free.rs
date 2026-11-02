use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use axum::body::Body;
use axum::http::{header, HeaderMap};
use axum::response::Response;
use rand::rngs::OsRng;
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{responses_chat_request, responses_from_chat, AppState, Candidate, RouteProtocol};
use crate::services::{account_proxy, free_model_directory};

const BASE_URL: &str = "https://opencode.ai/inference/v1";
const CORE_TOOLS: [(&str, &str); 5] = [
    ("bash", "command"),
    ("edit", "filePath"),
    ("glob", "pattern"),
    ("grep", "pattern"),
    ("read", "filePath"),
];

pub(super) const REFUSED_MESSAGE: &str = "All Auto Router models failed. OpenCode Zen refused its free model: free access works only in the OpenCode app, or today's free allowance is used up. Choose another model or use a personal API key.";

static COUNTER: AtomicU64 = AtomicU64::new(0);
static SESSIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

pub(super) fn is_candidate(candidate: &Candidate) -> bool {
    candidate.api_key.trim().is_empty()
        && free_model_directory::keyless_auth(&candidate.base_url, &candidate.model_id)
            == Some(free_model_directory::KeylessAuth::Public)
}

/// Zen names its free-tier refusals with these error types.
pub(super) fn is_refusal(body: &[u8]) -> bool {
    serde_json::from_slice::<Value>(body).is_ok_and(|value| {
        matches!(
            value.pointer("/error/type").and_then(Value::as_str),
            Some("FreeUsageLimitError" | "FreeTierError")
        )
    })
}

pub(super) async fn send(
    state: &AppState,
    headers: &HeaderMap,
    candidate: &Candidate,
    body: &Value,
    protocol: RouteProtocol,
    final_candidate: bool,
) -> Result<Response, String> {
    let streaming = body["stream"].as_bool().unwrap_or(false);
    let mut request_body = if protocol == RouteProtocol::Responses {
        match responses_chat_request(body, final_candidate) {
            Ok(chat) => chat,
            Err(error) => return Ok(error.into_response()),
        }
    } else {
        body.clone()
    };
    account_proxy::strip_response_metadata(&mut request_body);
    request_body["model"] = json!(candidate.model_id);
    request_body["stream"] = json!(true);
    inject_core_tools(&mut request_body);

    let session = session_id(headers, body);
    let upstream = state
        .http_client
        .post(format!("{BASE_URL}/chat/completions"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "*/*")
        .header(
            header::USER_AGENT,
            "opencode/1.19.0 ai-sdk/provider-utils/4.0.23 runtime/bun/1.3.14",
        )
        .header("x-opencode-project", "global")
        .header("x-opencode-session", &session)
        .header("x-opencode-session-id", &session)
        .header("x-opencode-request", opencode_id("msg", false))
        .header("x-opencode-client", "cli")
        .bearer_auth("public")
        .json(&request_body)
        .send()
        .await
        .map_err(|error| error.to_string())?;

    if !upstream.status().is_success() {
        let status = upstream.status();
        let response_headers = upstream.headers().clone();
        let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
        *response.status_mut() = status;
        *response.headers_mut() = response_headers;
        return Ok(response);
    }
    let converted = account_proxy::convert_public_model_stream(
        upstream,
        &candidate.model_id,
        protocol == RouteProtocol::Responses || streaming,
    )
    .await
    .map_err(|error| error.to_string())?;
    if protocol == RouteProtocol::Responses {
        let chat = responses_chat_request(body, false).map_err(|error| error.to_string())?;
        responses_from_chat(converted, &chat)
            .await
            .map_err(|error| error.to_string())
    } else {
        Ok(converted)
    }
}

fn inject_core_tools(body: &mut Value) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let tools = object.entry("tools").or_insert_with(|| json!([]));
    let Some(tools) = tools.as_array_mut() else {
        return;
    };
    for (name, parameter) in CORE_TOOLS {
        if tools.iter().any(|tool| tool["function"]["name"] == name) {
            continue;
        }
        let mut parameters = json!({
            "type":"object",
            "properties":{},
            "additionalProperties":false
        });
        parameters["properties"][parameter] = json!({"type":"string"});
        tools.push(json!({"type":"function", "function":{
            "name":name,
            "description":"Not available. Never call this tool.",
            "parameters":parameters
        }}));
    }
}

fn session_id(headers: &HeaderMap, body: &Value) -> String {
    let signal = [
        "x-opencode-session",
        "x-opencode-session-id",
        "x-session-affinity",
        "x-session-id",
        "conversation-id",
    ]
    .iter()
    .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()))
    .map(str::to_owned)
    .or_else(|| body["metadata"]["session_id"].as_str().map(str::to_owned))
    .or_else(|| {
        body["messages"]
            .as_array()
            .and_then(|messages| messages.iter().find(|message| message["role"] == "user"))
            .map(|message| message["content"].to_string())
    });
    let Some(signal) = signal else {
        return opencode_id("ses", true);
    };
    let key = format!("{:x}", Sha256::digest(signal.as_bytes()));
    let sessions = SESSIONS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut sessions = sessions.lock().unwrap();
    if let Some(session) = sessions.get(&key) {
        return session.clone();
    }
    if sessions.len() >= 1000 {
        sessions.clear();
    }
    let session = opencode_id("ses", true);
    sessions.insert(key, session.clone());
    session
}

fn opencode_id(prefix: &str, descending: bool) -> String {
    let tick = super::now_ms()
        .saturating_mul(4096)
        .saturating_add(COUNTER.fetch_add(1, Ordering::Relaxed) % 16 + 1);
    let stamp = if descending { !tick } else { tick } & 0xffff_ffff_ffff;
    let mut random = [0u8; 14];
    OsRng.fill_bytes(&mut random);
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let tail: String = random
        .iter()
        .map(|byte| ALPHABET[(*byte as usize) % ALPHABET.len()] as char)
        .collect();
    format!("{prefix}_{stamp:012x}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    #[test]
    fn public_request_has_core_tools_with_parameters_and_stable_session() {
        let mut chat = json!({"model":"big-pickle","messages":[{"role":"user","content":"hi"}]});
        inject_core_tools(&mut chat);
        assert_eq!(chat["tools"].as_array().unwrap().len(), 5);
        assert_eq!(
            chat["tools"][0]["function"]["parameters"]["properties"]["command"]["type"],
            "string"
        );
        let session = session_id(&HeaderMap::new(), &chat);
        assert_eq!(session_id(&HeaderMap::new(), &chat), session);
        assert_eq!(session.len(), 30);
        assert!(session.starts_with("ses_"));
    }

    #[tokio::test]
    #[ignore = "Calls public upstream models; run explicitly for live verification"]
    async fn live_public_models_return_chat_completions() {
        let state = AppState::for_tests().unwrap();
        for model in ["big-pickle", "glm-5.3-flash-free"] {
            let candidate = Candidate {
                internal_id: format!("live-{model}"),
                model_id: model.into(),
                base_url: BASE_URL.into(),
                api_key: String::new(),
                account_source: None,
                fingerprint: String::new(),
                attempt: None,
            };
            assert!(is_candidate(&candidate));
            let response = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                send(
                    &state,
                    &HeaderMap::new(),
                    &candidate,
                    &json!({"model":model,"stream":false,"max_tokens":128,
                        "messages":[{"role":"user","content":"Reply with the word OK."}]}),
                    RouteProtocol::Chat,
                    true,
                ),
            )
            .await
            .expect("public request timed out")
            .unwrap();
            let status = response.status();
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert!(status.is_success(), "{model}: {status} {body}");
            assert!(
                body["choices"][0]["message"]["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty()),
                "{model}: {body}"
            );
        }
    }

    #[tokio::test]
    #[ignore = "Calls public upstream models; run explicitly for live verification"]
    async fn live_public_streams_complete() {
        let state = AppState::for_tests().unwrap();
        for model in ["big-pickle", "glm-5.3-flash-free"] {
            let candidate = Candidate {
                internal_id: format!("live-stream-{model}"),
                model_id: model.into(),
                base_url: BASE_URL.into(),
                api_key: String::new(),
                account_source: None,
                fingerprint: String::new(),
                attempt: None,
            };
            let response = send(
                &state,
                &HeaderMap::new(),
                &candidate,
                &json!({"model":model,"stream":true,"max_tokens":128,
                    "messages":[{"role":"user","content":"Reply with the word OK."}]}),
                RouteProtocol::Chat,
                true,
            )
            .await
            .unwrap();
            let status = response.status();
            let bytes = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                response.into_body().collect(),
            )
            .await
            .expect("stream timed out")
            .unwrap()
            .to_bytes();
            let text = String::from_utf8_lossy(&bytes);
            assert!(status.is_success(), "{model}: {status} {text}");
            assert!(text.contains("[DONE]"), "{model}: {text}");
        }
    }
}
