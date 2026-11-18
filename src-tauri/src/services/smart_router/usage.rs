//! Observe reported usage without changing or retaining the forwarded response.
use axum::{body::Body, http::header, response::Response};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Instant;

const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    pub reported_responses: u64,
}
impl TokenUsage {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn add(&mut self, other: &Self) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.total_tokens = self.total_tokens.saturating_add(other.total_tokens);
        self.reported_responses = self
            .reported_responses
            .saturating_add(other.reported_responses);
    }
}

#[derive(Clone, Default, Debug)]
pub struct UsageReport {
    pub usage: TokenUsage,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cache_write_1h: u64,
    pub billable: bool,
    pub first_output_ms: Option<u64>,
    pub generation_ms: Option<u64>,
    pub elapsed_ms: u64,
}

struct Parser {
    sse: bool,
    bytes: Vec<u8>,
    overflow: bool,
    blank_line: bool,
    usage: TokenUsage,
    started: Instant,
    first_output_ms: Option<u64>,
    last_output_ms: Option<u64>,
    cache_read: u64,
    cache_write: u64,
    cache_write_1h: u64,
    input_seen: bool,
    output_seen: bool,
    finished: bool,
}
impl Parser {
    fn new(sse: bool) -> Self {
        Self {
            sse,
            bytes: Vec::new(),
            overflow: false,
            blank_line: true,
            usage: TokenUsage::default(),
            started: Instant::now(),
            first_output_ms: None,
            last_output_ms: None,
            cache_read: 0,
            cache_write: 0,
            cache_write_1h: 0,
            input_seen: false,
            output_seen: false,
            finished: false,
        }
    }
    fn value(&mut self, value: &Value) {
        let nonempty = |v: &Value| v.as_str().is_some_and(|s| !s.is_empty());
        let output = value["choices"].as_array().is_some_and(|choices| {
            choices.iter().any(|c| {
                nonempty(&c["delta"]["content"])
                    || nonempty(&c["delta"]["reasoning_content"])
                    || c["delta"]["tool_calls"].as_array().is_some_and(|calls| {
                        calls.iter().any(|c| nonempty(&c["function"]["arguments"]))
                    })
            })
        }) || matches!(
            value["type"].as_str(),
            Some(
                "response.output_text.delta"
                    | "response.function_call_arguments.delta"
                    | "response.reasoning_summary_text.delta"
            )
        ) && nonempty(&value["delta"])
            || nonempty(&value["delta"]["text"])
            || nonempty(&value["delta"]["thinking"])
            || nonempty(&value["delta"]["partial_json"])
            || value["candidates"].as_array().is_some_and(|cs| {
                cs.iter().any(|c| {
                    c["content"]["parts"].as_array().is_some_and(|ps| {
                        ps.iter()
                            .any(|p| nonempty(&p["text"]) || p.get("functionCall").is_some())
                    })
                })
            });
        if self.sse && output {
            let elapsed = self.started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            self.first_output_ms.get_or_insert(elapsed);
            self.last_output_ms = Some(elapsed);
        }
        let usage = value
            .get("usage")
            .or_else(|| value.get("response").and_then(|value| value.get("usage")))
            .or_else(|| value.get("message").and_then(|value| value.get("usage")))
            .or_else(|| value.get("usageMetadata"));
        let Some(usage) = usage.filter(|value| value.is_object()) else {
            return;
        };
        let input = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .or_else(|| usage.get("promptTokenCount"))
            .and_then(Value::as_u64);
        let output = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .or_else(|| usage.get("candidatesTokenCount"))
            .and_then(Value::as_u64)
            .map(|n| n.saturating_add(usage["thoughtsTokenCount"].as_u64().unwrap_or(0)));
        let total = usage
            .get("total_tokens")
            .or_else(|| usage.get("totalTokenCount"))
            .and_then(Value::as_u64);
        self.input_seen |= input.is_some();
        self.output_seen |= output.is_some();
        self.cache_read = self.cache_read.max(
            usage["cache_read_input_tokens"]
                .as_u64()
                .or_else(|| usage["prompt_tokens_details"]["cached_tokens"].as_u64())
                .or_else(|| usage["input_tokens_details"]["cached_tokens"].as_u64())
                .or_else(|| usage["cachedContentTokenCount"].as_u64())
                .unwrap_or(0),
        );
        self.cache_write = self
            .cache_write
            .max(usage["cache_creation_input_tokens"].as_u64().unwrap_or(0));
        self.cache_write_1h = self.cache_write_1h.max(
            usage["cache_creation"]["ephemeral_1h_input_tokens"]
                .as_u64()
                .unwrap_or(0),
        );
        if input.is_none() && output.is_none() && total.is_none() {
            return;
        }
        let cache = if usage.get("input_tokens").is_some()
            && usage.get("prompt_tokens").is_none()
            && usage.get("input_tokens_details").is_none()
        {
            usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .saturating_add(
                    usage
                        .get("cache_creation_input_tokens")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                )
        } else {
            0
        };
        self.usage.input_tokens = self
            .usage
            .input_tokens
            .max(input.unwrap_or(0).saturating_add(cache));
        self.usage.output_tokens = self.usage.output_tokens.max(output.unwrap_or(0));
        self.usage.total_tokens = self.usage.total_tokens.max(total.unwrap_or(0)).max(
            self.usage
                .input_tokens
                .saturating_add(self.usage.output_tokens),
        );
        self.usage.reported_responses = 1;
    }
    fn event(&mut self) {
        if !self.overflow {
            if let Ok(text) = std::str::from_utf8(&self.bytes) {
                let data = text
                    .lines()
                    .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Ok(value) = serde_json::from_str(&data) {
                    self.value(&value);
                }
            }
        }
        self.bytes.clear();
        self.overflow = false;
    }
    fn push(&mut self, bytes: &[u8]) {
        if !self.sse {
            if !self.overflow && self.bytes.len().saturating_add(bytes.len()) <= MAX_JSON_BYTES {
                self.bytes.extend_from_slice(bytes);
            } else {
                self.bytes.clear();
                self.overflow = true;
            }
            return;
        }
        for &byte in bytes {
            if !self.overflow {
                if self.bytes.len() < MAX_EVENT_BYTES {
                    self.bytes.push(byte);
                } else {
                    self.bytes.clear();
                    self.overflow = true;
                }
            }
            if byte == b'\n' {
                if self.blank_line {
                    self.event();
                }
                self.blank_line = true;
            } else if byte != b'\r' {
                self.blank_line = false;
            }
        }
    }
    fn finish(&mut self) -> Option<TokenUsage> {
        if self.finished {
            return (self.usage.reported_responses > 0).then(|| self.usage.clone());
        }
        self.finished = true;
        if self.sse {
            self.event();
        } else if !self.overflow {
            if let Ok(value) = serde_json::from_slice(&self.bytes) {
                self.value(&value);
            }
        }
        (self.usage.reported_responses > 0).then(|| self.usage.clone())
    }
}

struct Meter<F: FnOnce(UsageReport)> {
    parser: Parser,
    record: Option<F>,
}
impl<F: FnOnce(UsageReport)> Drop for Meter<F> {
    fn drop(&mut self) {
        self.parser.finish();
        if let Some(record) = self.record.take() {
            let first = self.parser.first_output_ms;
            let generation = self
                .parser
                .last_output_ms
                .zip(first)
                .map(|(last, first)| last.saturating_sub(first))
                .filter(|ms| *ms >= 20 && self.parser.usage.output_tokens > 1);
            record(UsageReport {
                usage: self.parser.usage.clone(),
                cache_read: self.parser.cache_read,
                cache_write: self.parser.cache_write,
                cache_write_1h: self.parser.cache_write_1h,
                billable: self.parser.input_seen && self.parser.output_seen,
                first_output_ms: first,
                generation_ms: generation,
                elapsed_ms: self
                    .parser
                    .started
                    .elapsed()
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64,
            });
        }
    }
}

#[cfg(test)]
pub fn observe(response: Response, record: impl FnOnce(TokenUsage) + Send + 'static) -> Response {
    observe_report(response, Instant::now(), move |report| {
        if report.usage.reported_responses > 0 {
            record(report.usage);
        }
    })
}

pub fn observe_report(
    response: Response,
    started: Instant,
    record: impl FnOnce(UsageReport) + Send + 'static,
) -> Response {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let sse = content_type.starts_with("text/event-stream");
    if !sse && !content_type.starts_with("application/json") {
        record(UsageReport {
            elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
            ..UsageReport::default()
        });
        return response;
    }
    let mut parser = Parser::new(sse);
    parser.started = started;
    let mut meter = Meter {
        parser,
        record: Some(record),
    };
    let (parts, body) = response.into_parts();
    let body = body.map_frame(move |frame| {
        if let Some(bytes) = frame.data_ref() {
            meter.parser.push(bytes);
        }
        frame
    });
    Response::from_parts(parts, Body::new(body))
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
