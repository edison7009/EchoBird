//! Where a request can be sent. Every channel — an API key, a subscription account, a local model — is an
//! `Upstream`: it names the models it serves and turns an OpenAI Chat request into an answer, or a classified
//! `ProxyError`. The engine never knows which kind it is talking to. New kinds of channel are added here, one
//! adapter at a time; nothing else changes.

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::error::ProxyError;
use crate::failure;
use crate::protocol::{self, ChatDecoder, SseParser, WireProtocol};

/// What kind of channel it is, for the user and for the scorer (subscriptions are used before pay-as-you-go).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Billing {
    /// Costs nothing (a free tier, a local model).
    Free,
    /// A subscription's allowance: already paid for, it resets.
    Subscription,
    /// Charged per use.
    PerUse,
}

/// A channel the engine can send to.
pub trait Upstream: Send + Sync {
    /// Stable id, used by plans (`channel/model`).
    fn id(&self) -> &str;
    /// The models it serves, as it names them.
    fn models(&self) -> &[String];
    fn billing(&self) -> Billing;
    /// Sends one OpenAI Chat request (its `model` already set) and returns the answer as it arrives. An error
    /// is returned only before the answer starts; it must be classified (`failure`), so routing can tell a rate
    /// limit from an exhausted quota or a bad request.
    fn send<'a>(
        &'a self,
        chat: &'a Value,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>>;
}

/// An OpenAI-compatible API: a base URL and a key.
pub struct OpenAiChat {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub billing: Billing,
}

impl Upstream for OpenAiChat {
    fn id(&self) -> &str {
        &self.id
    }

    fn models(&self) -> &[String] {
        &self.models
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn send<'a>(
        &'a self,
        chat: &'a Value,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>> {
        Box::pin(async move {
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let upstream = client
                .post(url)
                .bearer_auth(&self.api_key)
                .json(chat)
                .send()
                .await
                .map_err(ProxyError::upstream)?;
            let status = upstream.status();
            if !status.is_success() {
                let headers = upstream.headers().clone();
                let body = upstream.json::<Value>().await.unwrap_or(Value::Null);
                return Err(failure::upstream_error(status, &headers, &body));
            }
            Ok(passthrough(upstream))
        })
    }
}

/// An Anthropic Messages API: a base URL and a key. Many providers offer this shape next to (or instead of)
/// OpenAI Chat. Requests are converted from Chat and always streamed; the events are decoded back into Chat
/// chunks, or collected into one Chat answer when the caller did not ask for a stream.
pub struct AnthropicMessages {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub billing: Billing,
}

/// The API version header Anthropic-shaped APIs expect.
const ANTHROPIC_VERSION: &str = "2023-06-01";

impl AnthropicMessages {
    /// `…/v1/messages`, whether or not the base URL already ends in `/v1`.
    fn url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }
}

impl Upstream for AnthropicMessages {
    fn id(&self) -> &str {
        &self.id
    }

    fn models(&self) -> &[String] {
        &self.models
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn send<'a>(
        &'a self,
        chat: &'a Value,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>> {
        Box::pin(async move {
            protocol::reject_structured_output(chat)?;
            let model = chat["model"].as_str().unwrap_or_default();
            let mut request = protocol::chat_to_anthropic(chat, model);
            for knob in ["temperature", "top_p"] {
                if chat[knob].is_number() {
                    request[knob] = chat[knob].clone();
                }
            }
            // Providers differ in which header carries the key; sending both is what their own SDKs accept.
            let upstream = client
                .post(self.url())
                .header("x-api-key", &self.api_key)
                .bearer_auth(&self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header(header::ACCEPT, "text/event-stream")
                .json(&request)
                .send()
                .await
                .map_err(ProxyError::upstream)?;
            let status = upstream.status();
            if !status.is_success() {
                let headers = upstream.headers().clone();
                let body = upstream.json::<Value>().await.unwrap_or(Value::Null);
                return Err(failure::upstream_error(status, &headers, &body));
            }
            let streaming = chat["stream"].as_bool().unwrap_or(false);
            decoded(upstream, WireProtocol::Anthropic, model, streaming).await
        })
    }
}

/// An upstream's event stream in another protocol, as OpenAI Chat: chunks as they come when `streaming`,
/// otherwise one Chat answer once the stream completes. An error event before the first chunk is returned as a
/// classified error (so the engine can switch); one later ends the stream.
pub async fn decoded(
    upstream: reqwest::Response,
    wire: WireProtocol,
    model: &str,
    streaming: bool,
) -> Result<Response, ProxyError> {
    use futures_util::StreamExt;

    let is_sse = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/event-stream"));
    if !is_sse {
        return Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "The channel did not answer with an event stream",
        ));
    }
    let headers = upstream.headers().clone();
    let mut events = upstream.bytes_stream();
    let mut parser = SseParser::default();
    let mut decoder = ChatDecoder::new(wire, model);
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::io::Error>>(16);
    tokio::spawn(async move {
        loop {
            let next = tokio::select! {
                _ = tx.closed() => return,
                next = events.next() => next,
            };
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
                match decoder.decode(&event, &data) {
                    Ok(chunks) if streaming => {
                        for chunk in chunks {
                            if tx.send(Ok(chunk)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let error = failure::stream_error(&event, &data, &headers)
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
                let _ = tx
                    .send(Err(std::io::Error::other(
                        "The channel's stream ended before the answer did",
                    )))
                    .await;
                return;
            }
        }
    });
    let mut rx = tokio_stream::wrappers::ReceiverStream::new(rx);
    // Wait for the first piece: an error before any answer is the channel's failure, not the user's stream's.
    let first = match rx.next().await {
        Some(Ok(bytes)) => bytes,
        Some(Err(error)) => {
            return Err(
                match error
                    .into_inner()
                    .map(|inner| inner.downcast::<ProxyError>())
                {
                    Some(Ok(proxy)) => *proxy,
                    Some(Err(other)) => ProxyError::upstream(other),
                    None => ProxyError::new(StatusCode::BAD_GATEWAY, "The channel's stream failed"),
                },
            );
        }
        None => {
            return Err(ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "The channel sent nothing",
            ))
        }
    };
    let body = futures_util::stream::once(async move { Ok::<_, std::io::Error>(first) }).chain(rx);
    let mut response = Response::new(Body::from_stream(body));
    let kind = if streaming {
        "text/event-stream"
    } else {
        "application/json"
    };
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, header::HeaderValue::from_static(kind));
    Ok(response)
}

/// An upstream's successful answer, passed on as it arrives.
pub fn passthrough(upstream: reqwest::Response) -> Response {
    let status = upstream.status();
    let kind = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    if let Some(kind) = kind {
        response.headers_mut().insert(header::CONTENT_TYPE, kind);
    }
    response
}

/// Asks an OpenAI-compatible API which models it serves (its `/models`), at most `limit`.
pub async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    limit: usize,
) -> Result<Vec<String>, ProxyError> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    if !response.status().is_success() {
        return Err(ProxyError::new(
            response.status(),
            "The channel would not list its models",
        ));
    }
    let list: Value = response.json().await.map_err(ProxyError::upstream)?;
    Ok(list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .take(limit)
        .collect())
}

/// Every upstream the engine may use, by id.
#[derive(Default, Clone)]
pub struct Upstreams {
    list: Vec<std::sync::Arc<dyn Upstream>>,
}

impl Upstreams {
    pub fn new(list: Vec<std::sync::Arc<dyn Upstream>>) -> Self {
        Self { list }
    }

    pub fn get(&self, id: &str) -> Option<&std::sync::Arc<dyn Upstream>> {
        self.list.iter().find(|u| u.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &std::sync::Arc<dyn Upstream>> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// What a plan may refer to: these upstreams, their models, which cost nothing.
    pub fn known(&self, disabled: &BTreeSet<String>) -> crate::plan::Known {
        let mut known = crate::plan::Known {
            disabled: disabled.clone(),
            ..Default::default()
        };
        for upstream in &self.list {
            known.channels.insert(upstream.id().to_string());
            for model in upstream.models() {
                let pair = format!("{}/{model}", upstream.id());
                if upstream.billing() == Billing::Free {
                    known.free.insert(pair.clone());
                }
                known.models.insert(pair);
            }
        }
        known
    }
}

/// An upstream that is not wired yet (a kind of account still being moved over): it is listed, so plans and
/// the user see it, but refuses with a clear 501 until its adapter lands.
pub struct Pending {
    pub id: String,
    pub models: Vec<String>,
    pub billing: Billing,
    pub why: String,
}

impl Upstream for Pending {
    fn id(&self) -> &str {
        &self.id
    }

    fn models(&self) -> &[String] {
        &self.models
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn send<'a>(
        &'a self,
        _: &'a Value,
        _: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>> {
        Box::pin(async move {
            Err(ProxyError::new(
                StatusCode::NOT_IMPLEMENTED,
                self.why.clone(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A stand-in Anthropic API on a local port: answers every `/v1/messages` with `status` and `body`,
    /// as an event stream when it succeeds. Returns its base URL.
    async fn anthropic(status: StatusCode, body: String) -> String {
        let app = axum::Router::new().route(
            "/v1/messages",
            axum::routing::post(move |headers: axum::http::HeaderMap| {
                let body = body.clone();
                async move {
                    assert_eq!(headers["x-api-key"], "k");
                    assert_eq!(headers["anthropic-version"], ANTHROPIC_VERSION);
                    let kind = if status.is_success() {
                        "text/event-stream"
                    } else {
                        "application/json"
                    };
                    (status, [(header::CONTENT_TYPE, kind)], body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    fn events(text: &str) -> String {
        [
            json!({"type":"message_start","message":{"id":"m1","model":"glm","usage":{"input_tokens":3,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
            json!({"type":"message_stop"}),
        ]
        .iter()
        .map(|event| format!("event: {}\ndata: {event}\n\n", event["type"].as_str().unwrap()))
        .collect()
    }

    fn channel(base_url: String) -> AnthropicMessages {
        AnthropicMessages {
            id: "a".into(),
            base_url,
            api_key: "k".into(),
            models: vec!["glm".into()],
            billing: Billing::PerUse,
        }
    }

    async fn body(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn an_anthropic_channel_answers_in_chat_shape() {
        let base = anthropic(StatusCode::OK, events("你好")).await;
        let client = reqwest::Client::new();
        let ask = |stream: bool| json!({"model":"glm","stream":stream,"messages":[{"role":"system","content":"s"},{"role":"user","content":"hi"}]});

        let streamed = body(
            channel(base.clone())
                .send(&ask(true), &client)
                .await
                .unwrap(),
        )
        .await;
        assert!(
            streamed.contains("你好") && streamed.contains("data: "),
            "{streamed}"
        );

        let whole: Value = serde_json::from_str(
            &body(
                channel(format!("{base}/v1"))
                    .send(&ask(false), &client)
                    .await
                    .unwrap(),
            )
            .await,
        )
        .unwrap();
        assert_eq!(whole["choices"][0]["message"]["content"], "你好");
    }

    #[tokio::test]
    async fn an_anthropic_channel_refusal_is_classified() {
        let refusal =
            json!({"type":"error","error":{"type":"rate_limit_error","message":"slow down"}})
                .to_string();
        let base = anthropic(StatusCode::TOO_MANY_REQUESTS, refusal).await;
        let error = channel(base)
            .send(
                &json!({"model":"glm","messages":[{"role":"user","content":"hi"}]}),
                &reqwest::Client::new(),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn a_chat_request_becomes_a_messages_request() {
        let chat = json!({"model":"m","max_tokens":100,"messages":[
            {"role":"system","content":"be brief"},
            {"role":"user","content":"hi"},
            {"role":"assistant","content":null,"tool_calls":[{"id":"t1","type":"function","function":{"name":"f","arguments":"{\"x\":1}"}}]},
            {"role":"tool","tool_call_id":"t1","content":"done"}],
            "tools":[{"type":"function","function":{"name":"f","parameters":{"type":"object"}}}]});
        let request = protocol::chat_to_anthropic(&chat, "m");
        assert_eq!(request["system"], "be brief");
        assert_eq!(request["max_tokens"], 100);
        assert_eq!(request["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(request["messages"][2]["content"][0]["tool_use_id"], "t1");
        assert_eq!(request["tools"][0]["name"], "f");
    }
}
