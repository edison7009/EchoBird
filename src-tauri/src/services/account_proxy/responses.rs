use std::collections::{BTreeMap, BTreeSet};

use axum::{
    body::{to_bytes, Body},
    http::StatusCode,
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::protocol::SseParser;
use super::tools::{self, RESPONSES_REQUEST};
use super::{json_response, stream_response, ProxyError};

const INTERRUPTED_TOOL_RESULT: &str =
    "[The result of this tool call is unavailable: the turn was interrupted.]";

#[derive(Default)]
struct ToolRound {
    pending: Vec<(String, bool)>,
    answered: BTreeSet<String>,
    has_results: bool,
}

impl ToolRound {
    fn call(&mut self, id: &str, custom: bool) {
        if !id.is_empty() && !self.pending.iter().any(|(pending, _)| pending == id) {
            self.answered.remove(id);
            self.pending.push((id.to_string(), custom));
        }
    }

    fn answer(&mut self, id: &str) -> bool {
        self.has_results = true;
        if let Some(index) = self.pending.iter().position(|(pending, _)| pending == id) {
            self.pending.remove(index);
            self.answered.insert(id.to_string());
            true
        } else {
            // An unknown result may answer a call held by the upstream.
            !self.answered.contains(id)
        }
    }
}

fn finish_tool_round(round: &mut ToolRound, output: &mut Vec<Value>, responses: bool) {
    for (id, custom) in std::mem::take(&mut round.pending) {
        round.answered.insert(id.clone());
        output.push(if responses {
            json!({"type":if custom { "custom_tool_call_output" } else { "function_call_output" },
                "call_id":id,"output":INTERRUPTED_TOOL_RESULT})
        } else {
            json!({"role":"tool","tool_call_id":id,"content":INTERRUPTED_TOOL_RESULT,"is_error":true})
        });
    }
    round.has_results = false;
}

pub(super) fn repaired_chat_messages(body: &Value) -> Vec<Value> {
    let mut output = Vec::new();
    let mut round = ToolRound::default();
    for message in body["messages"].as_array().into_iter().flatten() {
        if message["role"] == "tool" {
            if !round.answer(message["tool_call_id"].as_str().unwrap_or_default()) {
                continue;
            }
        } else {
            let calls = message["tool_calls"].as_array();
            if message["role"] != "assistant"
                || calls.map_or(true, Vec::is_empty)
                || round.has_results
            {
                finish_tool_round(&mut round, &mut output, false);
            }
            for call in calls.into_iter().flatten() {
                round.call(call["id"].as_str().unwrap_or_default(), false);
            }
        }
        output.push(message.clone());
    }
    finish_tool_round(&mut round, &mut output, false);
    output
}

pub(super) fn repair_responses_tool_history(body: &mut Value) {
    if body
        .get("previous_response_id")
        .is_some_and(|id| !id.is_null())
    {
        return;
    }
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    let mut output = Vec::with_capacity(input.len());
    let mut round = ToolRound::default();
    for item in std::mem::take(input) {
        match item["type"].as_str().unwrap_or("message") {
            "function_call" | "custom_tool_call" => {
                if round.has_results {
                    finish_tool_round(&mut round, &mut output, true);
                }
                round.call(
                    item["call_id"].as_str().unwrap_or_default(),
                    item["type"] == "custom_tool_call",
                );
            }
            "function_call_output" | "custom_tool_call_output"
                if !round.answer(item["call_id"].as_str().unwrap_or_default()) =>
            {
                continue;
            }
            "message" if item["role"] == "user" || round.has_results => {
                finish_tool_round(&mut round, &mut output, true);
            }
            _ => {}
        }
        output.push(item);
    }
    finish_tool_round(&mut round, &mut output, true);
    *input = output;
}

/// Keep a batch's text before its calls and repair only known tool exchanges.
pub(crate) fn normalize_responses_tool_rounds(body: &mut Value) {
    let Some(input) = body.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    let mut first_call = None;
    for index in 0..input.len() {
        let item = &input[index];
        match item["type"].as_str().unwrap_or("message") {
            "function_call" | "custom_tool_call" => {
                first_call.get_or_insert(index);
            }
            "reasoning" if item.get("call_id").is_none() => {}
            "message"
                if item["role"] == "assistant"
                    && item.get("call_id").is_none()
                    && (item["content"].is_string()
                        || item["content"].as_array().is_some_and(|parts| {
                            !parts.is_empty()
                                && parts.iter().all(|part| {
                                    matches!(
                                        part["type"].as_str(),
                                        Some("text" | "input_text" | "output_text")
                                    ) && part["text"].is_string()
                                })
                        })) =>
            {
                if let Some(first) = first_call {
                    let mut from = index;
                    while from > first
                        && input[from - 1]["type"] == "reasoning"
                        && input[from - 1].get("call_id").is_none()
                    {
                        from -= 1;
                    }
                    let moved: Vec<_> = input.drain(from..=index).collect();
                    first_call = Some(first + moved.len());
                    input.splice(first..first, moved);
                }
            }
            _ => first_call = None,
        }
    }
    repair_responses_tool_history(body);
}

/// Google can mark only the chunk opening a thought, leaving its close unmarked.
#[derive(Default)]
pub(crate) struct GoogleThought {
    pending: String,
    inside: bool,
    awaiting_open: bool,
}

impl GoogleThought {
    pub(crate) fn push(&mut self, text: &str, marked: bool) -> Vec<(bool, String)> {
        const OPEN: &str = "<thought>";
        const CLOSE: &str = "</thought>";
        let pending_thought = !self.pending.is_empty();
        let continuing = pending_thought || self.awaiting_open;
        let mut value = std::mem::take(&mut self.pending);
        value.push_str(text);
        if !self.inside {
            if !marked && !continuing {
                return (!value.is_empty())
                    .then_some((false, value))
                    .into_iter()
                    .collect();
            }
            let lead = value.trim_start();
            if let Some(thought) = lead.strip_prefix(OPEN) {
                value = thought.to_string();
                self.inside = true;
                self.awaiting_open = false;
            } else if lead.is_empty() {
                self.awaiting_open = true;
                return (!value.is_empty())
                    .then_some((true, value))
                    .into_iter()
                    .collect();
            } else if !lead.is_empty() && OPEN.starts_with(lead) {
                self.pending = value;
                return Vec::new();
            } else {
                self.awaiting_open = false;
                return (!value.is_empty())
                    .then_some((marked || pending_thought, value))
                    .into_iter()
                    .collect();
            }
        }
        if let Some(close) = value.find(CLOSE) {
            self.inside = false;
            let mut parts = Vec::new();
            if close > 0 {
                parts.push((true, value[..close].to_string()));
            }
            let answer = &value[close + CLOSE.len()..];
            if !answer.is_empty() {
                parts.push((false, answer.to_string()));
            }
            return parts;
        }
        let held = (1..CLOSE.len())
            .rev()
            .find(|length| value.ends_with(&CLOSE[..*length]))
            .unwrap_or(0);
        let split = value.len() - held;
        self.pending = value[split..].to_string();
        (!value[..split].is_empty())
            .then(|| (true, value[..split].to_string()))
            .into_iter()
            .collect()
    }

    pub(crate) fn finish(&mut self) -> Option<(bool, String)> {
        self.inside = false;
        self.awaiting_open = false;
        (!self.pending.is_empty()).then(|| (true, std::mem::take(&mut self.pending)))
    }
}

fn empty_chat_content(content: &Value) -> bool {
    content.as_str().is_some_and(str::is_empty)
        || content
            .as_array()
            .is_some_and(|parts| parts.iter().all(|part| part["text"] == ""))
}

/// Responses clients enter the same Chat-based router, including tool-result turns.
pub fn responses_to_chat(input: &Value) -> Result<Value, ProxyError> {
    if input
        .get("previous_response_id")
        .is_some_and(|value| !value.is_null())
    {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "The router requires explicit conversation input instead of previous_response_id",
        ));
    }
    let catalog = tools::catalog(input)?;
    let mut messages = Vec::new();
    if let Some(instructions) = input.get("instructions").filter(|v| !v.is_null()) {
        messages.push(json!({"role":"system","content":instructions}));
    }
    if let Some(text) = input["input"].as_str() {
        messages.push(json!({"role":"user","content":text}));
    } else if let Some(items) = input["input"].as_array() {
        for item in items {
            match item["type"].as_str().unwrap_or("message") {
                "message" => {
                    let content = if item["content"].is_string() {
                        item["content"].clone()
                    } else {
                        let mut parts = Vec::new();
                        for part in item["content"].as_array().into_iter().flatten() {
                            parts.push(match part["type"].as_str().unwrap_or_default() {
                                "input_text"|"output_text"|"text"=>json!({"type":"text","text":part["text"]}),
                                "input_image"=>json!({"type":"image_url","image_url":{"url":part["image_url"],"detail":part.get("detail").cloned().unwrap_or(json!("auto"))}}),
                                "input_file"=>return Err(ProxyError::new(StatusCode::BAD_REQUEST,"File inputs require a native Responses model")),
                                _=>return Err(ProxyError::new(StatusCode::BAD_REQUEST,"Unsupported Responses content type")),
                            });
                        }
                        json!(parts)
                    };
                    // Strict Chat upstreams refuse an interrupted turn's empty reply.
                    if item["role"] == "assistant" && empty_chat_content(&content) {
                        continue;
                    }
                    if let Some(previous) = messages.last_mut().filter(|previous| {
                        item["role"] == "assistant"
                            && previous["role"] == "assistant"
                            && previous["tool_calls"].is_array()
                    }) {
                        let mut parts = if let Some(text) = previous["content"].as_str() {
                            vec![json!({"type":"text","text":text})]
                        } else {
                            previous["content"].as_array().cloned().unwrap_or_default()
                        };
                        if let Some(text) = content.as_str() {
                            parts.push(json!({"type":"text","text":text}));
                        } else if let Some(content) = content.as_array() {
                            parts.extend(content.iter().cloned());
                        }
                        previous["content"] = json!(parts);
                    } else {
                        messages.push(json!({"role":item["role"],"content":content}));
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let name = tools::alias(
                        item["namespace"].as_str().unwrap_or_default(),
                        item["name"].as_str().unwrap_or_default(),
                    );
                    let arguments = if item["type"] == "custom_tool_call" {
                        json!({"input":item["input"]}).to_string()
                    } else {
                        item["arguments"].as_str().unwrap_or_default().to_string()
                    };
                    let call = json!({"id":item["call_id"],"type":"function","function":{"name":name,"arguments":arguments}});
                    if let Some(message) = messages
                        .last_mut()
                        .filter(|message| message["role"] == "assistant")
                    {
                        let calls = message
                            .as_object_mut()
                            .expect("constructed message")
                            .entry("tool_calls")
                            .or_insert_with(|| json!([]));
                        calls
                            .as_array_mut()
                            .expect("constructed tool calls")
                            .push(call);
                    } else {
                        messages
                            .push(json!({"role":"assistant","content":null,"tool_calls":[call]}));
                    }
                }
                "function_call_output" | "custom_tool_call_output" => {
                    if item["output"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|part| part["type"] == "input_file")
                    {
                        return Err(ProxyError::new(
                            StatusCode::BAD_REQUEST,
                            "File inputs require a native Responses model",
                        ));
                    }
                    messages.push(json!({"role":"tool","tool_call_id":item["call_id"],"content":item["output"]}));
                }
                "reasoning" | "additional_tools" | "tool_search_output" => {}
                // Records of work the previous model's host already finished.
                // A native compaction stays refused: dropping it loses the turns it replaced.
                "web_search_call"
                | "file_search_call"
                | "code_interpreter_call"
                | "image_generation_call"
                | "mcp_call"
                | "mcp_list_tools"
                | "mcp_approval_request"
                | "mcp_approval_response"
                | "tool_search_call" => {}
                _ => {
                    return Err(ProxyError::new(
                        StatusCode::BAD_REQUEST,
                        "Unsupported Responses input item",
                    ))
                }
            }
        }
    } else {
        return Err(ProxyError::new(
            StatusCode::BAD_REQUEST,
            "Responses input must be text or an array",
        ));
    }
    let tools = tools::chat_tools(&catalog);
    let mut output = json!({"model":input["model"],"messages":messages,"tools":tools,"stream":input["stream"].as_bool().unwrap_or(false)});
    output[RESPONSES_REQUEST] = input.clone();
    for key in ["temperature", "top_p", "parallel_tool_calls", "reasoning"] {
        if let Some(value) = input.get(key) {
            output[key] = value.clone();
        }
    }
    if let Some(text) = input.get("text").filter(|text| !text.is_null()) {
        let text = text.as_object().ok_or_else(|| {
            ProxyError::new(
                StatusCode::BAD_REQUEST,
                "Responses text options must be an object",
            )
        })?;
        if text
            .iter()
            .any(|(key, value)| key != "format" && !value.is_null())
        {
            return Err(ProxyError::new(
                StatusCode::BAD_REQUEST,
                "These Responses text options require a native Responses model",
            ));
        }
        if let Some(format) = text.get("format").filter(|format| !format.is_null()) {
            output["response_format"] = match format["type"].as_str() {
                Some("text" | "json_object") => format.clone(),
                Some("json_schema") => {
                    let mut schema = format.as_object().expect("format type field").clone();
                    schema.remove("type");
                    json!({"type":"json_schema","json_schema":schema})
                }
                _ => {
                    return Err(ProxyError::new(
                        StatusCode::BAD_REQUEST,
                        "Unsupported Responses text format",
                    ))
                }
            };
        }
    }
    if let Some(value) = input.get("max_output_tokens") {
        output["max_tokens"] = value.clone();
    }
    if let Some(choice) = input
        .get("tool_choice")
        .filter(|choice| !tools::is_hosted(choice["type"].as_str().unwrap_or_default()))
    {
        output["tool_choice"] = if choice.is_object() {
            json!({"type":"function","function":{"name":tools::alias(choice["namespace"].as_str().unwrap_or_default(),choice["name"].as_str().unwrap_or_default())}})
        } else {
            choice.clone()
        };
    }
    Ok(output)
}

#[derive(Default)]
struct Encoder {
    id: String,
    model: String,
    sequence: u64,
    output: Vec<Value>,
    text_index: Option<usize>,
    reasoning_index: Option<usize>,
    tool_indexes: BTreeMap<u64, usize>,
    tools: BTreeMap<String, Value>,
    custom_arguments: BTreeMap<usize, String>,
    pending_tools: BTreeMap<u64, Vec<Value>>,
    response_item_indexes: BTreeMap<String, usize>,
    usage: Value,
    finish_reason: String,
    finished: bool,
    google_thought: GoogleThought,
}

impl Encoder {
    fn new() -> Self {
        Self {
            id: format!("resp_{}", uuid::Uuid::new_v4().simple()),
            ..Self::default()
        }
    }

    fn with_tools(request: &Value) -> Self {
        Self {
            tools: tools::catalog(request).unwrap_or_default(),
            ..Self::new()
        }
    }

    fn event(&mut self, kind: &str, mut value: Value) -> Bytes {
        value["type"] = json!(kind);
        value["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        Bytes::from(format!("event: {kind}\ndata: {value}\n\n"))
    }

    fn response(&self, status: &str) -> Value {
        let mut output = self.output.clone();
        for item in &mut output {
            item["status"] = json!(if status == "in_progress" {
                "in_progress"
            } else {
                "completed"
            });
        }
        let mut response = json!({"id":self.id,"object":"response","created_at":chrono::Utc::now().timestamp(),"model":self.model,
            "status":status,"output":output,"error":null,"incomplete_details":null,"usage":self.usage});
        if status == "incomplete" {
            response["incomplete_details"] = json!({"reason":if self.finish_reason == "content_filter" { "content_filter" } else { "max_output_tokens" }});
        }
        response
    }

    fn created(&mut self) -> Bytes {
        let response = self.response("in_progress");
        self.event("response.created", json!({"response":response}))
    }

    fn response_item(&mut self, item: &Value, chunks: &mut Vec<Bytes>) {
        let id = item["id"].as_str().unwrap_or_default();
        if let Some(index) = self.response_item_indexes.get(id) {
            self.output[*index] = item.clone();
            return;
        }
        let index = self.output.len();
        self.response_item_indexes.insert(id.into(), index);
        self.reasoning_index = Some(index);
        self.output.push(item.clone());
        chunks.push(self.event(
            "response.output_item.added",
            json!({"output_index":index,"item":item}),
        ));
    }

    fn decode(&mut self, data: &str) -> Result<Vec<Bytes>, String> {
        if self.finished {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            if !self.pending_tools.is_empty() {
                return Err("Router completed a tool call without its name".into());
            }
            return Ok(self.complete());
        }
        let value: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(value["error"]["message"]
                .as_str()
                .unwrap_or("Inference failed")
                .into());
        }
        if let Some(model) = value["model"].as_str() {
            self.model = model.into();
        }
        if let Some(usage) = value.get("usage") {
            self.usage = json!({"input_tokens":usage["prompt_tokens"],"output_tokens":usage["completion_tokens"],"total_tokens":usage["total_tokens"],
                "input_tokens_details":{"cached_tokens":usage["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0)},
                "output_tokens_details":{"reasoning_tokens":usage["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0)}});
        }
        let mut chunks = Vec::new();
        for choice in value["choices"].as_array().into_iter().flatten().take(1) {
            if let Some(reason) = choice["finish_reason"].as_str() {
                if reason == "error" {
                    return Err("Upstream stream finished with an error".into());
                }
                self.finish_reason = reason.into();
            }
            let delta = choice
                .get("delta")
                .or_else(|| choice.get("message"))
                .unwrap_or(&Value::Null);
            let mut delta = delta.clone();
            if let Some(text) = delta["content"].as_str() {
                let parts = self
                    .google_thought
                    .push(text, delta["extra_content"]["google"]["thought"] == true);
                let thought: String = parts
                    .iter()
                    .filter(|(reasoning, _)| *reasoning)
                    .map(|(_, text)| text.as_str())
                    .collect();
                let answer: String = parts
                    .iter()
                    .filter(|(reasoning, _)| !*reasoning)
                    .map(|(_, text)| text.as_str())
                    .collect();
                delta["content"] = json!(answer);
                if !thought.is_empty() {
                    let previous = delta["reasoning_content"].as_str().unwrap_or_default();
                    delta["reasoning_content"] = json!(format!("{previous}{thought}"));
                }
            }
            if let Some(item) = delta.get("_echobird_response_item") {
                self.response_item(item, &mut chunks);
            }
            let native_items = delta["_echobird_response_items"].as_array();
            for item in native_items.into_iter().flatten() {
                self.response_item(item, &mut chunks);
            }
            if let Some(text) = delta["reasoning_content"]
                .as_str()
                .filter(|text| !text.is_empty() && native_items.is_none())
            {
                let index = if let Some(index) = self.reasoning_index {
                    index
                } else {
                    let index = self.output.len();
                    self.reasoning_index = Some(index);
                    let item = json!({"type":"reasoning","id":format!("rs_{}",uuid::Uuid::new_v4().simple()),"status":"in_progress","summary":[]});
                    self.output.push(item.clone());
                    chunks.push(self.event(
                        "response.output_item.added",
                        json!({"output_index":index,"item":item}),
                    ));
                    let part = json!({"type":"summary_text","text":""});
                    self.output[index]["summary"] = json!([part]);
                    chunks.push(self.event("response.reasoning_summary_part.added",json!({"output_index":index,"item_id":self.output[index]["id"],"summary_index":0,"part":part})));
                    index
                };
                if self.output[index]["summary"]
                    .as_array()
                    .map_or(true, |summary| summary.is_empty())
                {
                    let part = json!({"type":"summary_text","text":""});
                    self.output[index]["summary"] = json!([part]);
                    chunks.push(self.event("response.reasoning_summary_part.added",json!({"output_index":index,"item_id":self.output[index]["id"],"summary_index":0,"part":part})));
                }
                let previous = self.output[index]["summary"][0]["text"]
                    .as_str()
                    .unwrap_or_default();
                self.output[index]["summary"][0]["text"] = json!(format!("{previous}{text}"));
                chunks.push(self.event("response.reasoning_summary_text.delta",json!({"output_index":index,"item_id":self.output[index]["id"],"summary_index":0,"delta":text})));
            }
            if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
                let index = if let Some(index) = self.text_index {
                    index
                } else {
                    let index = self.output.len();
                    self.text_index = Some(index);
                    let item = json!({"type":"message","id":format!("msg_{}",uuid::Uuid::new_v4().simple()),"role":"assistant","status":"in_progress","content":[]});
                    self.output.push(item.clone());
                    chunks.push(self.event(
                        "response.output_item.added",
                        json!({"output_index":index,"item":item}),
                    ));
                    let part = json!({"type":"output_text","text":"","annotations":[]});
                    self.output[index]["content"] = json!([part]);
                    chunks.push(self.event("response.content_part.added",json!({"output_index":index,"item_id":self.output[index]["id"],"content_index":0,"part":part})));
                    index
                };
                let previous = self.output[index]["content"][0]["text"]
                    .as_str()
                    .unwrap_or_default();
                self.output[index]["content"][0]["text"] = json!(format!("{previous}{text}"));
                chunks.push(self.event("response.output_text.delta",json!({"output_index":index,"item_id":self.output[index]["id"],"content_index":0,"delta":text})));
            }
            for (position, call) in delta["tool_calls"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let key = call["index"].as_u64().unwrap_or(position as u64);
                let mut call = call.clone();
                if !self.tool_indexes.contains_key(&key)
                    && call["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .is_empty()
                {
                    self.pending_tools.entry(key).or_default().push(call);
                    continue;
                }
                if let Some(previous) = self.pending_tools.remove(&key) {
                    let arguments = previous
                        .iter()
                        .filter_map(|call| call["function"]["arguments"].as_str())
                        .collect::<String>();
                    if call["id"].is_null() {
                        if let Some(id) = previous.iter().find_map(|call| call.get("id")) {
                            call["id"] = id.clone();
                        }
                    }
                    call["function"]["arguments"] = json!(format!(
                        "{arguments}{}",
                        call["function"]["arguments"].as_str().unwrap_or_default()
                    ));
                }
                let index = if let Some(index) = self.tool_indexes.get(&key) {
                    *index
                } else {
                    let index = self.output.len();
                    self.tool_indexes.insert(key, index);
                    let alias = call["function"]["name"].as_str().unwrap_or_default();
                    let declaration = self.tools.get(alias);
                    let custom = declaration.is_some_and(|tool| tool["type"] == "custom");
                    let name = declaration
                        .map(|tool| tool["name"].clone())
                        .unwrap_or_else(|| json!(alias));
                    let mut item = json!({"type":if custom {"custom_tool_call"} else {"function_call"},"id":format!("{}_{}",if custom {"ctc"} else {"fc"},uuid::Uuid::new_v4().simple()),"call_id":call["id"],
                        "name":name,"status":"in_progress"});
                    if let Some(id) = call.get("_echobird_response_item_id") {
                        item["id"] = id.clone();
                    }
                    item[if custom { "input" } else { "arguments" }] = json!("");
                    if let Some(namespace) = declaration.and_then(|tool| tool.get("namespace")) {
                        item["namespace"] = namespace.clone();
                    }
                    self.output.push(item.clone());
                    chunks.push(self.event(
                        "response.output_item.added",
                        json!({"output_index":index,"item":item}),
                    ));
                    index
                };
                if let Some(id) = call.get("id") {
                    self.output[index]["call_id"] = id.clone();
                }
                if let Some(name) = call["function"].get("name") {
                    self.output[index]["name"] = self
                        .tools
                        .get(name.as_str().unwrap_or_default())
                        .map(|tool| tool["name"].clone())
                        .unwrap_or_else(|| name.clone());
                }
                if let Some(arguments) = call["function"]["arguments"]
                    .as_str()
                    .filter(|a| !a.is_empty())
                {
                    if self.output[index]["type"] == "custom_tool_call" {
                        self.custom_arguments
                            .entry(index)
                            .or_default()
                            .push_str(arguments);
                    } else {
                        let old = self.output[index]["arguments"].as_str().unwrap_or_default();
                        self.output[index]["arguments"] = json!(format!("{old}{arguments}"));
                        chunks.push(self.event("response.function_call_arguments.delta",json!({"output_index":index,"item_id":self.output[index]["id"],"delta":arguments})));
                    }
                }
            }
        }
        Ok(chunks)
    }

    fn complete(&mut self) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        let mut chunks = if let Some((_, text)) = self.google_thought.finish() {
            self.decode(&json!({"choices":[{"delta":{"reasoning_content":text}}]}).to_string())
                .expect("constructed Chat reasoning event")
        } else {
            Vec::new()
        };
        self.finished = true;
        for index in 0..self.output.len() {
            if self.output[index]["type"] == "custom_tool_call" {
                let input = tools::custom_input(
                    self.custom_arguments
                        .get(&index)
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
                self.output[index]["input"] = json!(input);
                chunks.push(self.event(
                    "response.custom_tool_call_input.delta",
                    json!({"output_index":index,"item_id":self.output[index]["id"],"delta":input}),
                ));
                chunks.push(self.event(
                    "response.custom_tool_call_input.done",
                    json!({"output_index":index,"item_id":self.output[index]["id"],"input":input}),
                ));
            }
            self.output[index]["status"] = json!("completed");
            let item = self.output[index].clone();
            if item["type"] == "function_call" {
                chunks.push(self.event("response.function_call_arguments.done",json!({"output_index":index,"item_id":item["id"],"arguments":item["arguments"]})));
            } else if item["type"] == "reasoning" {
                for (summary_index, part) in
                    item["summary"].as_array().into_iter().flatten().enumerate()
                {
                    chunks.push(self.event("response.reasoning_summary_text.done",json!({"output_index":index,"item_id":item["id"],"summary_index":summary_index,"text":part["text"]})));
                    chunks.push(self.event("response.reasoning_summary_part.done",json!({"output_index":index,"item_id":item["id"],"summary_index":summary_index,"part":part})));
                }
            } else if item["type"] == "message" {
                chunks.push(self.event("response.output_text.done",json!({"output_index":index,"item_id":item["id"],"content_index":0,"text":item["content"][0]["text"]})));
                chunks.push(self.event("response.content_part.done",json!({"output_index":index,"item_id":item["id"],"content_index":0,"part":item["content"][0]})));
            }
            chunks.push(self.event(
                "response.output_item.done",
                json!({"output_index":index,"item":item}),
            ));
        }
        let status = if matches!(self.finish_reason.as_str(), "length" | "content_filter") {
            "incomplete"
        } else {
            "completed"
        };
        let response = self.response(status);
        chunks.push(self.event(
            if status == "incomplete" {
                "response.incomplete"
            } else {
                "response.completed"
            },
            json!({"response":response}),
        ));
        chunks
    }

    fn failed(&mut self, error: &ProxyError) -> Bytes {
        self.finished = true;
        let mut response = self.response("failed");
        let mut detail = json!({"code":error.error_code.as_deref().unwrap_or("upstream_error"),
            "message":error.message,"status":error.status.as_u16()});
        if let Some(kind) = &error.error_type {
            detail["type"] = json!(kind);
        }
        if let Some(retry_after) = error
            .retry_after
            .as_ref()
            .and_then(|value| value.to_str().ok())
        {
            detail["retry_after"] = json!(retry_after);
        }
        if error.model_scope {
            detail["scope"] = json!("model");
        }
        response["error"] = detail;
        self.event("response.failed", json!({"response":response}))
    }
}

fn stream_failure(error: &(dyn std::error::Error + 'static)) -> ProxyError {
    let mut cause = Some(error);
    while let Some(error) = cause {
        if let Some(error) = error.downcast_ref::<ProxyError>() {
            return error.clone();
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
    ProxyError::new(StatusCode::BAD_GATEWAY, error.to_string())
}

fn failure_chunk(
    encoder: &mut Encoder,
    started: bool,
    error: ProxyError,
) -> Result<Bytes, std::io::Error> {
    if started {
        Ok(encoder.failed(&error))
    } else {
        Err(std::io::Error::other(error))
    }
}

/// Convert router output for native Responses clients without losing function calls.
pub async fn chat_to_responses(response: Response, stream: bool) -> Response {
    chat_to_responses_with_tools(response, stream, &Value::Null).await
}

/// The original declarations restore custom calls and namespaces after any account hop.
pub async fn chat_to_responses_with_tools(
    response: Response,
    stream: bool,
    request: &Value,
) -> Response {
    if !response.status().is_success() {
        return response;
    }
    let (parts, body) = response.into_parts();
    let mut encoder = Encoder::with_tools(request);
    if !stream {
        let bytes = match to_bytes(body, 32 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return routed_headers(stream_failure(&error).into_response(), &parts.headers)
            }
        };
        let data = match serde_json::from_slice::<Value>(&bytes) {
            Ok(value) => value,
            Err(error) => {
                return routed_headers(conversion_error(&error.to_string()), &parts.headers)
            }
        };
        if let Err(error) = encoder.decode(&data.to_string()) {
            return routed_headers(conversion_error(&error), &parts.headers);
        }
        if !encoder.pending_tools.is_empty() {
            return routed_headers(
                conversion_error("Router completed a tool call without its name"),
                &parts.headers,
            );
        }
        encoder.complete();
        return routed_headers(
            json_response(&encoder.response(
                if matches!(encoder.finish_reason.as_str(), "length" | "content_filter") {
                    "incomplete"
                } else {
                    "completed"
                },
            )),
            &parts.headers,
        );
    }
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(16);
    let response_headers = parts.headers.clone();
    tokio::spawn(async move {
        let mut upstream = body.into_data_stream();
        let mut parser = SseParser::default();
        let mut created = Some(encoder.created());
        loop {
            let next = tokio::select! { _=tx.closed()=>return,next=upstream.next()=>next };
            let eof = next.is_none();
            let frames = match next {
                Some(Ok(bytes)) => parser.push(&bytes),
                Some(Err(error)) => {
                    let _ = tx
                        .send(failure_chunk(
                            &mut encoder,
                            created.is_none(),
                            stream_failure(&error),
                        ))
                        .await;
                    return;
                }
                None => parser.finish(),
            };
            for (event, data) in frames {
                if let Some(error) = super::stream_error(&event, &data, &response_headers) {
                    let _ = tx
                        .send(failure_chunk(&mut encoder, created.is_none(), error))
                        .await;
                    return;
                }
                match encoder.decode(&data) {
                    Ok(chunks) => {
                        if !chunks.is_empty() {
                            if let Some(created) = created.take() {
                                if tx.send(Ok(created)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        for chunk in chunks {
                            if tx.send(Ok(chunk)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let error = ProxyError::new(StatusCode::BAD_GATEWAY, error);
                        let _ = tx
                            .send(failure_chunk(&mut encoder, created.is_none(), error))
                            .await;
                        return;
                    }
                }
            }
            if encoder.finished {
                return;
            }
            if eof {
                let _ = tx
                    .send(failure_chunk(
                        &mut encoder,
                        created.is_none(),
                        ProxyError::new(
                            StatusCode::BAD_GATEWAY,
                            "Router stream ended before completion",
                        ),
                    ))
                    .await;
                return;
            }
        }
    });
    routed_headers(
        stream_response(Body::from_stream(ReceiverStream::new(rx))),
        &parts.headers,
    )
}

fn routed_headers(mut response: Response, original: &axum::http::HeaderMap) -> Response {
    for (name, value) in original {
        if !matches!(
            name.as_str(),
            "content-type" | "content-length" | "content-encoding" | "transfer-encoding"
        ) {
            response.headers_mut().insert(name.clone(), value.clone());
        }
    }
    response
}

fn conversion_error(message: &str) -> Response {
    let mut response = json_response(&json!({"error":{"message":message,"type":"upstream_error"}}));
    *response.status_mut() = StatusCode::BAD_GATEWAY;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom_request() -> Value {
        json!({"model":"auto","stream":true,"tools":[{"type":"custom","name":"apply_patch","description":"Patch files","format":{"type":"grammar","syntax":"lark","definition":"start: patch\npatch: PATCH\nPATCH: /[\\s\\S]+/"}},
            {"type":"namespace","name":"mcp__pad","tools":[{"type":"custom","name":"write","format":{"type":"text"}}]}],
            "input":[{"type":"custom_tool_call","call_id":"old","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"},{"type":"custom_tool_call_output","call_id":"old","output":"ok"}]})
    }

    #[test]
    fn parallel_function_and_custom_history_share_one_assistant_tool_round() {
        let input = json!({"model":"auto","tools":[{"type":"function","name":"read","parameters":{"type":"object"}},{"type":"custom","name":"write","format":{"type":"text"}}],
            "input":[{"role":"assistant","content":"Checking both files"},
                {"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
                {"type":"reasoning","summary":[]},
                {"type":"custom_tool_call","call_id":"b","name":"write","input":"patch"},
                {"type":"function_call_output","call_id":"a","output":"read-result"},
                {"type":"custom_tool_call_output","call_id":"b","output":"write-result"},
                {"role":"user","content":"Continue"},
                {"type":"function_call","call_id":"c","name":"read","arguments":"{}"},
                {"type":"function_call_output","call_id":"c","output":"last-result"}]});
        let chat = responses_to_chat(&input).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[0]["content"], "Checking both files");
        let calls = messages[0]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["id"], "a");
        assert_eq!(calls[1]["id"], "b");
        assert_eq!(calls[1]["function"]["arguments"], "{\"input\":\"patch\"}");
        assert_eq!(messages[1]["tool_call_id"], "a");
        assert_eq!(messages[2]["tool_call_id"], "b");
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(messages[4]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(messages[4]["tool_calls"][0]["id"], "c");
        assert_eq!(messages[5]["tool_call_id"], "c");
        assert_eq!(
            super::super::protocol::chat_to_responses(&chat, "native")["input"],
            input["input"]
        );
    }

    #[test]
    fn interleaved_batch_text_stays_with_one_assistant_tool_round() {
        let input = json!({"input":[
            {"role":"assistant","content":"Reading first"},
            {"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
            {"type":"reasoning","summary":[]},
            {"role":"assistant","content":[{"type":"output_text","text":"Reading second"}]},
            {"type":"function_call","call_id":"b","name":"read","arguments":"{}"},
            {"type":"function_call_output","call_id":"a","output":"first"},
            {"type":"function_call_output","call_id":"b","output":"second"},
            {"role":"user","content":"Continue"}
        ]});
        let chat = responses_to_chat(&input).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(
            messages[0]["content"],
            json!([
                {"type":"text","text":"Reading first"},
                {"type":"text","text":"Reading second"}
            ])
        );
        assert_eq!(messages[0]["tool_calls"][0]["id"], "a");
        assert_eq!(messages[0]["tool_calls"][1]["id"], "b");
        assert_eq!(messages[1]["tool_call_id"], "a");
        assert_eq!(messages[2]["tool_call_id"], "b");
        assert_eq!(messages[3]["role"], "user");
        assert_eq!(
            super::super::protocol::chat_to_responses(&chat, "native")["input"],
            input["input"]
        );
    }

    #[test]
    fn native_tool_round_normalization_preserves_items_and_complete_exchanges() {
        let input = json!([
            {"type":"message","id":"msg_first","role":"assistant","content":[{"type":"output_text","text":"First"}]},
            {"type":"function_call","id":"fc_first","call_id":"a","namespace":"fixture","name":"read","arguments":"{\"path\":\"a\"}"},
            {"type":"reasoning","id":"rs_second","encrypted_content":"opaque-fixture","summary":[]},
            {"type":"message","id":"msg_second","role":"assistant","content":"Second"},
            {"type":"custom_tool_call","id":"ctc_second","call_id":"b","name":"write","input":"raw fixture"},
            {"type":"function_call_output","call_id":"a","output":"first result"},
            {"type":"custom_tool_call_output","call_id":"b","output":[{"type":"input_text","text":"second result"}]},
            {"role":"assistant","content":"Next exchange"},
            {"type":"function_call","call_id":"c","name":"read","arguments":"{}"},
            {"type":"function_call_output","call_id":"c","output":"third result"}
        ]);
        let mut body = json!({"input":input,"instructions":"Keep all metadata","store":false,"tools":[{"type":"custom","name":"write","format":{"type":"text"}}]});
        let mut expected = body.clone();
        let source = body["input"].as_array().unwrap();
        expected["input"] = json!([
            source[0], source[2], source[3], source[1], source[4], source[5], source[6], source[7],
            source[8], source[9]
        ]);
        normalize_responses_tool_rounds(&mut body);
        assert_eq!(body, expected);
        normalize_responses_tool_rounds(&mut body);
        assert_eq!(body, expected);
    }

    #[test]
    fn native_tool_round_normalization_respects_unknown_and_user_barriers() {
        for barrier in [
            json!({"type":"future_vendor_event","id":"opaque-item","payload":{"keep":true}}),
            json!({"role":"user","content":"New instruction"}),
            json!({"type":"function_call_output","call_id":"a","output":"completed"}),
            json!({"role":"assistant","call_id":"opaque-call","content":"Bound message"}),
            json!({"type":"message","role":"assistant","content":[{"type":"future_content","payload":"keep"}]}),
        ] {
            let mut body = json!({"input":[
                {"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
                barrier,
                {"role":"assistant","content":"Keep me after the barrier"},
                {"type":"function_call","call_id":"b","name":"read","arguments":"{}"},
                {"type":"function_call_output","call_id":"b","output":"finished"}
            ]});
            let expected = body.clone();
            normalize_responses_tool_rounds(&mut body);
            let repaired = body["input"].as_array().unwrap();
            let original_items: Vec<_> = repaired
                .iter()
                .filter(|item| item["output"] != INTERRUPTED_TOOL_RESULT)
                .cloned()
                .collect();
            assert_eq!(json!(original_items), expected["input"]);
            let missing: Vec<_> = repaired
                .iter()
                .filter(|item| item["output"] == INTERRUPTED_TOOL_RESULT)
                .collect();
            if expected["input"][1]["type"] == "function_call_output" {
                assert!(missing.is_empty());
            } else {
                assert_eq!(missing.len(), 1);
                assert_eq!(missing[0]["call_id"], "a");
            }
        }
    }

    #[test]
    fn tool_history_native_rounds_drop_duplicate_attachments_and_preserve_unknown_results() {
        let first = json!({"type":"function_call_output","call_id":"a","output":[{"type":"input_image","image_url":"data:image/png;base64,Zmlyc3Q="}]});
        let reused = json!({"type":"function_call_output","call_id":"a","output":"later answer"});
        let unknown = json!({"type":"function_call_output","call_id":"upstream-call","output":"keep unknown result"});
        let reasoning = json!({"type":"reasoning","encrypted_content":"opaque"});
        let mut body = json!({"input":[
            {"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
            first,
            {"type":"function_call_output","call_id":"a","output":[{"type":"input_image","image_url":"data:image/png;base64,ZHVwbGljYXRl"}]},
            reasoning,
            {"type":"function_call","call_id":"a","name":"write","arguments":"{}"},
            reused,
            {"role":"user","content":"Next"},
            unknown
        ],"metadata":{"keep":true}});
        normalize_responses_tool_rounds(&mut body);
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 7);
        assert_eq!(input[1], first);
        assert_eq!(input[2], reasoning);
        assert_eq!(input[4], reused);
        assert_eq!(input[6], unknown);
        let expected = body.clone();
        normalize_responses_tool_rounds(&mut body);
        assert_eq!(body, expected);
    }

    #[test]
    fn tool_history_answers_belong_to_the_latest_call_with_that_id() {
        let messages = json!({"messages":[
            {"role":"assistant","tool_calls":[{"id":"a","function":{"name":"read","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"a","content":"first"},
            {"role":"assistant","content":"The first task is done"},
            {"role":"tool","tool_call_id":"a","content":"late duplicate"},
            {"role":"user","content":"Next task"},
            {"role":"assistant","tool_calls":[{"id":"a","function":{"name":"write","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"a","content":"new answer"}
        ]});
        let repaired = repaired_chat_messages(&messages);
        assert_eq!(repaired.len(), 6);
        assert_eq!(repaired[1]["content"], "first");
        assert_eq!(repaired[5]["content"], "new answer");
        assert!(!repaired
            .iter()
            .any(|message| message["content"] == "late duplicate"));
    }

    #[test]
    fn text_between_complete_tool_exchanges_stays_in_its_turn() {
        let chat = responses_to_chat(&json!({"input":[
            {"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
            {"type":"function_call_output","call_id":"a","output":"first"},
            {"role":"assistant","content":"Next task"},
            {"type":"function_call","call_id":"b","name":"read","arguments":"{}"},
            {"type":"function_call_output","call_id":"b","output":"second"}
        ]}))
        .unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["tool_calls"].as_array().unwrap().len(), 1);
        assert_eq!(messages[1]["tool_call_id"], "a");
        assert_eq!(messages[2]["content"], "Next task");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "b");
        assert_eq!(messages[3]["tool_call_id"], "b");
    }

    #[test]
    fn marked_google_thought_tags_stay_out_of_responses_answers() {
        for parts in [
            vec![(true, "<thought>计划"), (false, "中</thought>答案")],
            vec![
                (true, "<th"),
                (false, "ought>计划中</tho"),
                (false, "ught>答案"),
            ],
            vec![(true, "<thought>计划中</thought>答案")],
            vec![(true, "计划中"), (false, "答案")],
        ] {
            let mut encoder = Encoder::new();
            let mut wire = String::new();
            for (index, (marked, text)) in parts.into_iter().enumerate() {
                wire.push_str(&format!("data: {}\n\n", json!({"choices":[{"delta":{"content":text,"reasoning_content":if index == 0 { "Existing " } else { "" },"extra_content":{"google":{"thought":marked}}}}]})));
            }
            wire.push_str(&format!("data: {}\n\n", json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_probe","function":{"name":"probe","arguments":"{\"number\":42}"}}]},"finish_reason":"tool_calls"}]})));
            wire.push_str("data: [DONE]\n\n");
            let mut parser = SseParser::default();
            let mut events = Vec::new();
            for byte in wire.as_bytes() {
                for (_, data) in parser.push(std::slice::from_ref(byte)) {
                    events.extend(encoder.decode(&data).unwrap());
                }
            }
            let response = encoder.response("completed");
            let output = response["output"].as_array().unwrap();
            let reasoning = output
                .iter()
                .find(|item| item["type"] == "reasoning")
                .unwrap();
            let answer = output
                .iter()
                .find(|item| item["type"] == "message")
                .unwrap();
            assert_eq!(reasoning["summary"][0]["text"], "Existing 计划中");
            assert_eq!(answer["content"][0]["text"], "答案");
            let call = output
                .iter()
                .find(|item| item["type"] == "function_call")
                .unwrap();
            assert_eq!(call["call_id"], "call_probe");
            assert_eq!(call["arguments"], "{\"number\":42}");
            let deltas: Vec<_> = events
                .iter()
                .flat_map(|event| {
                    let mut parser = SseParser::default();
                    parser.push(event)
                })
                .filter_map(|(_, data)| serde_json::from_str::<Value>(&data).ok())
                .collect();
            assert_eq!(
                deltas
                    .iter()
                    .filter(|event| event["type"] == "response.output_text.delta")
                    .filter_map(|event| event["delta"].as_str())
                    .collect::<String>(),
                "答案"
            );
            assert!(deltas
                .iter()
                .any(|event| event["type"] == "response.completed"));
        }
    }

    #[test]
    fn unmarked_thought_quotes_and_explicit_reasoning_remain_intact() {
        for quote in [
            "Quote <thought>literally</thought>",
            "<thought>literally</thought>",
        ] {
            let mut encoder = Encoder::new();
            encoder.decode(&json!({"choices":[{"delta":{"reasoning_content":"Explicit reasoning","content":quote}}]}).to_string()).unwrap();
            encoder.complete();
            let response = encoder.response("completed");
            assert_eq!(
                response["output"][0]["summary"][0]["text"],
                "Explicit reasoning"
            );
            assert_eq!(response["output"][1]["content"][0]["text"], quote);
        }
    }

    #[test]
    fn marked_partial_thought_tags_flush_only_to_reasoning() {
        for marked in [
            "<th",
            "<thought>unfinished</th",
            "Thought quotes <thought>literally",
        ] {
            let mut encoder = Encoder::new();
            encoder.decode(&json!({"choices":[{"delta":{"content":marked,"extra_content":{"google":{"thought":true}}}}]}).to_string()).unwrap();
            encoder.complete();
            let response = encoder.response("completed");
            let output = response["output"].as_array().unwrap();
            assert!(output.iter().all(|item| item["type"] == "reasoning"));
            assert_eq!(
                output[0]["summary"][0]["text"],
                marked.strip_prefix("<thought>").unwrap_or(marked)
            );
        }
    }

    #[test]
    fn closed_google_thought_keeps_literal_tags_in_the_answer() {
        let mut encoder = Encoder::new();
        encoder.decode(&json!({"choices":[{"delta":{"content":"<thought>Planning","extra_content":{"google":{"thought":true}}}}]}).to_string()).unwrap();
        encoder.decode(&json!({"choices":[{"delta":{"content":"</thought>Quote <thought>literally</thought>"}}]}).to_string()).unwrap();
        encoder.complete();
        let response = encoder.response("completed");
        assert_eq!(response["output"][0]["summary"][0]["text"], "Planning");
        assert_eq!(
            response["output"][1]["content"][0]["text"],
            "Quote <thought>literally</thought>"
        );
    }

    #[test]
    fn marked_whitespace_and_empty_prefixes_still_arm_google_thought_tags() {
        for prefix in [" \n", ""] {
            let mut encoder = Encoder::new();
            encoder.decode(&json!({"choices":[{"delta":{"content":prefix,"extra_content":{"google":{"thought":true}}}}]}).to_string()).unwrap();
            encoder
                .decode(
                    &json!({"choices":[{"delta":{"content":"<thought>private</thought>answer"}}]})
                        .to_string(),
                )
                .unwrap();
            encoder.complete();
            let response = encoder.response("completed");
            let output = response["output"].as_array().unwrap();
            let answer = output
                .iter()
                .find(|item| item["type"] == "message")
                .unwrap();
            assert_eq!(answer["content"][0]["text"], "answer");
            let reasoning = output
                .iter()
                .find(|item| item["type"] == "reasoning")
                .unwrap();
            assert_eq!(reasoning["summary"][0]["text"], format!("{prefix}private"));
        }
    }

    #[test]
    fn unmarked_whitespace_and_marked_prefix_without_tags_preserve_answers() {
        for (marked, prefix, answer) in [
            (false, " \n", "<thought>literal</thought>"),
            (true, " \n", "Plain answer"),
            (true, "", "Plain answer"),
        ] {
            let mut thought = GoogleThought::default();
            let mut parts = thought.push(prefix, marked);
            parts.extend(thought.push(answer, false));
            let visible: String = parts
                .iter()
                .filter(|(reasoning, _)| !*reasoning)
                .map(|(_, text)| text.as_str())
                .collect();
            assert_eq!(
                visible,
                if marked {
                    answer.to_string()
                } else {
                    format!("{prefix}{answer}")
                }
            );
        }
    }

    #[tokio::test]
    async fn content_filtered_chat_replies_are_incomplete_in_stream_and_json() {
        for stream in [false, true] {
            let value = json!({"model":"filtered-fixture","choices":[{"message":{"role":"assistant","content":null},"delta":{},"finish_reason":"content_filter"}],"usage":{"prompt_tokens":0,"completion_tokens":0,"total_tokens":0}});
            let wire = if stream {
                format!("data: {value}\n\ndata: [DONE]\n\n")
            } else {
                value.to_string()
            };
            let upstream = Response::builder()
                .header(
                    "content-type",
                    if stream {
                        "text/event-stream"
                    } else {
                        "application/json"
                    },
                )
                .body(Body::from(wire))
                .unwrap();
            let response = chat_to_responses(upstream, stream).await;
            assert!(response.status().is_success());
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let result = if stream {
                let mut parser = SseParser::default();
                let events: Vec<Value> = parser
                    .push(&bytes)
                    .into_iter()
                    .map(|(_, data)| serde_json::from_str(&data).unwrap())
                    .collect();
                assert!(!events
                    .iter()
                    .any(|event| event["type"] == "response.completed"));
                events
                    .into_iter()
                    .find(|event| event["type"] == "response.incomplete")
                    .unwrap()["response"]
                    .clone()
            } else {
                serde_json::from_slice::<Value>(&bytes).unwrap()
            };
            assert_eq!(result["status"], "incomplete");
            assert_eq!(result["incomplete_details"]["reason"], "content_filter");
            assert_eq!(result["output"], json!([]));
            assert_eq!(result["usage"]["output_tokens"], 0);
        }
    }

    #[test]
    fn length_limited_chat_reply_keeps_max_output_reason() {
        let mut encoder = Encoder::new();
        encoder
            .decode(
                &json!({"choices":[{"delta":{"content":"partial"},"finish_reason":"length"}]})
                    .to_string(),
            )
            .unwrap();
        encoder.complete();
        assert_eq!(
            encoder.response("incomplete")["incomplete_details"]["reason"],
            "max_output_tokens"
        );
    }

    #[test]
    fn structured_text_formats_map_to_chat_without_losing_schema_constraints() {
        for (format, expected) in [
            (json!({"type":"text"}), json!({"type":"text"})),
            (json!({"type":"json_object"}), json!({"type":"json_object"})),
            (
                json!({"type":"json_schema","name":"answer","description":"Answer schema","schema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false},"strict":true}),
                json!({"type":"json_schema","json_schema":{"name":"answer","description":"Answer schema","schema":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false},"strict":true}}),
            ),
        ] {
            let input = json!({"model":"auto","input":"Answer","text":{"format":format}});
            let chat = responses_to_chat(&input).unwrap();
            assert_eq!(chat["response_format"], expected);
            assert!(chat.get("text").is_none());
            assert_eq!(
                super::super::protocol::chat_to_responses(&chat, "native")["text"],
                input["text"]
            );
        }
        for text in [
            json!({"format":{"type":"unsupported"}}),
            json!({"format":{"type":"text"},"verbosity":"low"}),
            json!("invalid"),
        ] {
            let error = responses_to_chat(&json!({"input":"Answer","text":text})).unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
        }
        let plain = responses_to_chat(&json!({"input":"Answer"})).unwrap();
        assert!(plain.get("response_format").is_none());
    }

    #[test]
    fn compatibility_conversion_rejects_file_inputs_and_file_tool_outputs() {
        for part in [
            json!({"type":"input_file","file_id":"file_fixture"}),
            json!({"type":"input_file","file_url":"https://example.com/file.pdf"}),
            json!({"type":"input_file","filename":"file.pdf","file_data":"data:application/pdf;base64,fixture"}),
        ] {
            for item in [
                json!({"role":"user","content":[{"type":"input_text","text":"Summarize"},part]}),
                json!({"type":"function_call_output","call_id":"call_fixture","output":[part]}),
                json!({"type":"custom_tool_call_output","call_id":"call_fixture","output":[part]}),
            ] {
                let error = responses_to_chat(&json!({"input":[item]})).unwrap_err();
                assert_eq!(error.status, StatusCode::BAD_REQUEST);
                assert_eq!(
                    error.message,
                    "File inputs require a native Responses model"
                );
            }
        }
        let image = responses_to_chat(&json!({"input":[{"role":"user","content":[{"type":"input_text","text":"Describe"},{"type":"input_image","image_url":"data:image/png;base64,fixture"}]}]})).unwrap();
        assert_eq!(image["messages"][0]["content"][1]["type"], "image_url");
    }

    #[test]
    fn custom_request_keeps_raw_history_and_native_grammar() {
        let request = custom_request();
        let chat = responses_to_chat(&request).unwrap();
        assert_eq!(
            tools::custom_input(
                chat["messages"][0]["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .unwrap()
            ),
            request["input"][0]["input"]
        );
        assert_eq!(chat["messages"][1]["tool_call_id"], "old");
        assert_eq!(chat["messages"][1]["content"], "ok");
        let native = super::super::protocol::chat_to_responses(&chat, "gpt-test");
        assert_eq!(native["tools"], request["tools"]);
        assert_eq!(native["input"], request["input"]);
        assert_eq!(native["model"], "gpt-test");
        let gemini = super::super::protocol::chat_to_code_assist(&chat, "test", "project");
        assert_eq!(
            gemini["request"]["contents"][0]["parts"][0]["functionCall"]["args"]["input"],
            request["input"][0]["input"]
        );
    }

    #[test]
    fn custom_stream_restores_raw_input_and_namespace_without_function_events() {
        let request = custom_request();
        let raw = "*** Begin Patch\n*** Add File: 模型.txt\n+\"hello\"\n*** End Patch";
        let arguments = json!({"input":raw}).to_string();
        let split = arguments.find("Add File").unwrap();
        let mut encoder = Encoder::with_tools(&request);
        let mut chunks=encoder.decode(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_a","function":{"arguments":&arguments[..split]}}]}}]}).to_string()).unwrap();
        chunks.extend(encoder.decode(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"apply_patch","arguments":&arguments[split..]}},{"index":1,"id":"call_b","function":{"name":"mcp__pad__write","arguments":"{\"input\":\"raw\"}"}}]}}]}).to_string()).unwrap());
        chunks.extend(encoder.decode("[DONE]").unwrap());
        let text = chunks
            .iter()
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .collect::<String>();
        assert!(!text.contains("response.function_call_arguments"));
        assert!(text.contains("response.custom_tool_call_input.delta"));
        assert!(text.contains("response.custom_tool_call_input.done"));
        assert_eq!(encoder.output[0]["type"], "custom_tool_call");
        assert_eq!(encoder.output[0]["call_id"], "call_a");
        assert_eq!(encoder.output[0]["input"], raw);
        assert!(encoder.output[0].get("arguments").is_none());
        assert_eq!(encoder.output[1]["name"], "write");
        assert_eq!(encoder.output[1]["namespace"], "mcp__pad");
    }

    #[tokio::test]
    async fn native_custom_sse_returns_original_type_in_stream_and_json() {
        use super::super::protocol::{ChatDecoder, WireProtocol};
        let request = custom_request();
        let raw = "*** Begin Patch\n*** End Patch";
        let mut decoder = ChatDecoder::new(WireProtocol::Responses, "gpt-test");
        let mut frames = Vec::new();
        for frame in [
            json!({"type":"response.output_item.added","item":{"type":"custom_tool_call","id":"ctc_a","call_id":"call_a","name":"apply_patch","input":""}}),
            json!({"type":"response.custom_tool_call_input.delta","item_id":"ctc_a","delta":"*** Begin Patch\n"}),
            json!({"type":"response.custom_tool_call_input.delta","item_id":"ctc_a","delta":"*** End Patch"}),
            json!({"type":"response.custom_tool_call_input.done","item_id":"ctc_a","input":raw}),
            json!({"type":"response.output_item.done","item":{"type":"custom_tool_call","id":"ctc_a","call_id":"call_a","name":"apply_patch","input":raw}}),
            json!({"type":"response.completed","response":{"object":"response","id":"resp_custom","status":"completed","error":null,"output":[{"type":"custom_tool_call","id":"ctc_a","call_id":"call_a","name":"apply_patch","input":raw}]}}),
        ] {
            frames.extend(decoder.decode("", &frame.to_string()).unwrap());
        }
        assert_eq!(
            tools::custom_input(
                decoder.collected()["choices"][0]["message"]["tool_calls"][0]["function"]
                    ["arguments"]
                    .as_str()
                    .unwrap()
            ),
            raw
        );
        for stream in [false, true] {
            let body = if stream {
                Body::from(
                    frames
                        .iter()
                        .flat_map(|frame| frame.to_vec())
                        .collect::<Vec<_>>(),
                )
            } else {
                Body::from(decoder.collected().to_string())
            };
            let response =
                chat_to_responses_with_tools(Response::new(body), stream, &request).await;
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let response = if stream {
                let mut parser = SseParser::default();
                let frames = parser.push(&bytes);
                frames
                    .iter()
                    .find_map(|(_, data)| {
                        let value: Value = serde_json::from_str(data).ok()?;
                        (value["type"] == "response.completed").then(|| value["response"].clone())
                    })
                    .unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            assert_eq!(response["output"][0]["type"], "custom_tool_call");
            assert_eq!(response["output"][0]["input"], raw);
            assert_eq!(response["output"][0]["call_id"], "call_a");
        }
    }

    #[tokio::test]
    async fn stateless_tool_followup_keeps_completed_reasoning_and_native_item_ids() {
        use super::super::protocol::{ChatDecoder, WireProtocol};
        let request = custom_request();
        let reasoning = json!({"type":"reasoning","id":"rs_original","status":"completed","summary":[{"type":"summary_text","text":"thinking"}],"encrypted_content":"opaque-fixture-content"});
        let tool = json!({"type":"custom_tool_call","id":"ctc_original","call_id":"call_a","name":"apply_patch","input":"*** Begin Patch\n*** End Patch","status":"completed"});
        let mut decoder = ChatDecoder::new(WireProtocol::Responses, "gpt-test");
        let mut frames = Vec::new();
        for frame in [
            json!({"type":"response.output_item.added","item":{"type":"reasoning","id":"rs_original","summary":[],"status":"in_progress"}}),
            json!({"type":"response.reasoning_summary_text.delta","delta":"thinking"}),
            json!({"type":"response.output_item.done","item":reasoning}),
            json!({"type":"response.output_item.added","item":{"type":"custom_tool_call","id":"ctc_original","call_id":"call_a","name":"apply_patch","input":""}}),
            json!({"type":"response.output_item.done","item":tool}),
            json!({"type":"response.completed","response":{"object":"response","id":"resp_followup","status":"completed","error":null,"output":[reasoning,tool]}}),
        ] {
            frames.extend(decoder.decode("", &frame.to_string()).unwrap());
        }
        for stream in [false, true] {
            let body = if stream {
                Body::from(
                    frames
                        .iter()
                        .flat_map(|frame| frame.to_vec())
                        .collect::<Vec<_>>(),
                )
            } else {
                Body::from(decoder.collected().to_string())
            };
            let response =
                chat_to_responses_with_tools(Response::new(body), stream, &request).await;
            let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
            let result = if stream {
                let mut parser = SseParser::default();
                parser
                    .push(&bytes)
                    .iter()
                    .find_map(|(_, data)| {
                        let value: Value = serde_json::from_str(data).ok()?;
                        (value["type"] == "response.completed").then(|| value["response"].clone())
                    })
                    .unwrap()
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            assert_eq!(result["output"][0], reasoning);
            assert_eq!(result["output"][1]["id"], "ctc_original");
            let mut followup = request.clone();
            let mut input = result["output"].as_array().unwrap().clone();
            input.push(json!({"type":"custom_tool_call_output","call_id":"call_a","output":"ok"}));
            followup["input"] = json!(input);
            let chat = responses_to_chat(&followup).unwrap();
            let native = super::super::protocol::chat_to_responses(&chat, "gpt-test");
            assert_eq!(native["input"], followup["input"]);
            assert_eq!(
                native["input"][0]["encrypted_content"],
                "opaque-fixture-content"
            );
        }
    }

    #[test]
    fn code_mode_additional_tools_are_declared_and_hosted_search_is_explicitly_rejected() {
        let request = json!({"model":"auto","input":[{"type":"additional_tools","tools":[{"type":"namespace","name":"functions","tools":[{"type":"custom","name":"exec","format":{"type":"text"}},{"type":"function","name":"wait","parameters":{"type":"object"}}]}]},
            {"type":"custom_tool_call","call_id":"a","name":"exec","namespace":"functions","input":"text('ok')"},{"type":"custom_tool_call_output","call_id":"a","output":"ok"}]});
        let chat = responses_to_chat(&request).unwrap();
        assert_eq!(chat["tools"].as_array().unwrap().len(), 2);
        assert_eq!(
            chat["messages"][0]["tool_calls"][0]["function"]["name"],
            "exec"
        );
        assert_eq!(
            super::super::protocol::chat_to_responses(&chat, "native")["input"],
            request["input"]
        );
        let unsupported = json!({"input":"hello","tools":[{"type":"computer_use_preview"}]});
        assert_eq!(
            responses_to_chat(&unsupported).unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn hosted_tools_and_their_records_do_not_block_a_chat_model() {
        let request = json!({"tool_choice":{"type":"web_search"},
        "tools":[{"type":"web_search"},{"type":"function","name":"exec","parameters":{}}],
        "input":[
            {"type":"message","role":"user","content":"Find it"},
            {"type":"web_search_call","id":"ws_1","status":"completed","action":{"type":"search","query":"fixture"}},
            {"type":"tool_search_call","id":"ts_1","call_id":"ts_1","arguments":{}},
            {"type":"tool_search_output","call_id":"ts_1","tools":[{"type":"function","name":"deferred","parameters":{}}]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Found it"}]}
        ]});
        let chat = responses_to_chat(&request).unwrap();
        let roles: Vec<_> = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "assistant"]);
        let names: Vec<_> = chat["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["deferred", "exec"]);
        assert!(chat.get("tool_choice").is_none());
    }

    #[test]
    fn a_chat_error_finish_does_not_complete_the_response() {
        let mut encoder = Encoder::new();
        let frame = json!({"choices":[{"delta":{"content":"partial"},"finish_reason":"error"}]});
        assert!(encoder.decode(&frame.to_string()).is_err());
    }

    #[test]
    fn a_native_compaction_is_still_refused_rather_than_lost() {
        let request = json!({"input":[{"type":"compaction","encrypted_content":"sealed"}]});
        assert_eq!(
            responses_to_chat(&request).unwrap_err().status,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn an_interrupted_empty_reply_is_not_sent_to_chat() {
        let request = json!({"input":[
            {"type":"message","role":"user","content":"First"},
            {"type":"reasoning","id":"rs_1","summary":[]},
            {"type":"message","role":"assistant","content":""},
            {"type":"message","role":"assistant","content":[]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":""}]},
            {"type":"message","role":"user","content":"Second"}
        ]});
        let chat = responses_to_chat(&request).unwrap();
        let roles: Vec<_> = chat["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "user"]);
    }

    #[tokio::test]
    async fn conversion_preserves_routing_trace_and_replaces_body_headers() {
        for stream in [false, true] {
            let body = if stream {
                Body::from("data: {\"model\":\"actual\",\"error\":null,\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n")
            } else {
                Body::from(json!({"model":"actual","error":null,"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}).to_string())
            };
            let response = Response::builder()
                .header("X-EchoBird-Routed-Via", "own-model")
                .header("content-length", "1")
                .header("content-type", "application/x-original")
                .body(body)
                .unwrap();
            let converted = chat_to_responses(response, stream).await;
            assert_eq!(converted.headers()["X-EchoBird-Routed-Via"], "own-model");
            assert!(!converted.headers().contains_key("content-length"));
            let bytes = to_bytes(converted.into_body(), 1024 * 1024).await.unwrap();
            let content = String::from_utf8(bytes.to_vec()).unwrap();
            if stream {
                assert!(content.contains("response.completed"));
                assert!(content.contains("response.output_text.delta"));
                assert!(!content.contains("response.failed"));
            } else {
                assert_eq!(
                    serde_json::from_str::<Value>(&content).unwrap()["status"],
                    "completed"
                );
                assert_eq!(
                    serde_json::from_str::<Value>(&content).unwrap()["output"][0]["content"][0]
                        ["text"],
                    "ok"
                );
            }
        }
    }

    #[tokio::test]
    async fn conversion_rejects_upstream_errors_in_stream_and_json() {
        let failure = json!({"error":{"code":"rate_limit_exceeded","message":"Quota exhausted"}});
        for stream in [false, true] {
            let body = if stream {
                Body::from(format!("data: {failure}\n\ndata: [DONE]\n\n"))
            } else {
                Body::from(failure.to_string())
            };
            let response = Response::builder()
                .header("X-EchoBird-Routed-Via", "own-model")
                .body(body)
                .unwrap();
            let converted = chat_to_responses(response, stream).await;
            assert_eq!(converted.headers()["X-EchoBird-Routed-Via"], "own-model");
            assert_eq!(
                converted.status(),
                if stream {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_GATEWAY
                }
            );
            if stream {
                let mut upstream = converted.into_body().into_data_stream();
                let error = stream_failure(&upstream.next().await.unwrap().unwrap_err());
                assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(error.error_code.as_deref(), Some("rate_limit_exceeded"));
                assert_eq!(error.message, "Quota exhausted");
                assert!(upstream.next().await.is_none());
            } else {
                let bytes = to_bytes(converted.into_body(), 1024 * 1024).await.unwrap();
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(value["error"]["message"], "Quota exhausted");
            }
        }
    }

    #[tokio::test]
    async fn early_body_failures_preserve_typed_status_scope_and_retry_without_creation() {
        for stream in [false, true] {
            for status in [
                StatusCode::UNAUTHORIZED,
                StatusCode::PAYMENT_REQUIRED,
                StatusCode::TOO_MANY_REQUESTS,
            ] {
                let mut error = ProxyError::new(status, "fixture refusal");
                error.error_code = Some("fixture_code".into());
                error.error_type = Some("fixture_type".into());
                error.model_scope = status == StatusCode::PAYMENT_REQUIRED;
                error.retry_after = Some(axum::http::HeaderValue::from_static("17"));
                let body = Body::from_stream(futures_util::stream::iter([
                    Ok(Bytes::from_static(
                        b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":1}}\n\n",
                    )),
                    Err(std::io::Error::other(error.clone())),
                ]));
                let response = Response::builder()
                    .header("X-EchoBird-Routed-Via", "fixture-account")
                    .body(body)
                    .unwrap();
                let converted = chat_to_responses(response, stream).await;
                assert_eq!(
                    converted.headers()["X-EchoBird-Routed-Via"],
                    "fixture-account"
                );
                if stream {
                    let mut output = converted.into_body().into_data_stream();
                    let recovered = stream_failure(&output.next().await.unwrap().unwrap_err());
                    assert_eq!(recovered.status, status);
                    assert_eq!(recovered.error_code, error.error_code);
                    assert_eq!(recovered.error_type, error.error_type);
                    assert_eq!(recovered.model_scope, error.model_scope);
                    assert_eq!(recovered.retry_after, error.retry_after);
                    assert!(output.next().await.is_none());
                } else {
                    assert_eq!(converted.status(), status);
                    assert_eq!(converted.headers()["retry-after"], "17");
                    assert_eq!(
                        converted
                            .extensions()
                            .get::<super::super::ProxyFailureMetadata>()
                            .unwrap()
                            .model_scope,
                        error.model_scope
                    );
                    let bytes = to_bytes(converted.into_body(), 1024 * 1024).await.unwrap();
                    let value: Value = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(value["error"]["code"], "fixture_code");
                    assert_eq!(value["error"]["type"], "fixture_type");
                }
            }
        }
    }

    #[tokio::test]
    async fn body_failure_after_output_emits_failed_with_typed_details_and_ordered_events() {
        let mut error = ProxyError::new(StatusCode::PAYMENT_REQUIRED, "fixture plan refusal");
        error.error_code = Some("fixture_plan".into());
        error.error_type = Some("plan_required".into());
        error.model_scope = true;
        error.retry_after = Some(axum::http::HeaderValue::from_static("19"));
        let body = Body::from_stream(futures_util::stream::iter([
            Ok(Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            )),
            Err(std::io::Error::other(error)),
        ]));
        let converted = chat_to_responses(Response::new(body), true).await;
        let bytes = to_bytes(converted.into_body(), 1024 * 1024).await.unwrap();
        let frames = SseParser::default().push(&bytes);
        assert_eq!(frames.first().unwrap().0, "response.created");
        assert!(frames
            .iter()
            .any(|(event, _)| event == "response.output_text.delta"));
        assert!(!frames
            .iter()
            .any(|(event, _)| event == "response.completed"));
        for (sequence, (_, data)) in frames.iter().enumerate() {
            let value: Value = serde_json::from_str(data).unwrap();
            assert_eq!(value["sequence_number"], sequence as u64);
        }
        let failed: Value = serde_json::from_str(&frames.last().unwrap().1).unwrap();
        assert_eq!(failed["type"], "response.failed");
        assert_eq!(
            failed["response"]["error"],
            json!({"code":"fixture_plan","type":"plan_required","message":"fixture plan refusal","status":402,"scope":"model","retry_after":"19"})
        );
    }

    #[tokio::test]
    async fn conversion_does_not_complete_a_truncated_stream_with_null_error() {
        let response = Response::new(Body::from(
            "data: {\"error\":null,\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ));
        let converted = chat_to_responses(response, true).await;
        let bytes = to_bytes(converted.into_body(), 1024 * 1024).await.unwrap();
        let mut parser = SseParser::default();
        let events = parser.push(&bytes);
        assert!(events
            .iter()
            .any(|(event, _)| event == "response.output_text.delta"));
        assert!(!events
            .iter()
            .any(|(event, _)| event == "response.completed"));
        let failed = events
            .iter()
            .find(|(event, _)| event == "response.failed")
            .unwrap();
        let value: Value = serde_json::from_str(&failed.1).unwrap();
        assert_eq!(
            value["response"]["error"]["message"],
            "Router stream ended before completion"
        );
    }

    #[test]
    fn responses_round_trip_preserves_parallel_calls_and_results() {
        let input = json!({"model":"auto","input":[{"type":"function_call","call_id":"a","name":"read","arguments":"{}"},
            {"type":"function_call_output","call_id":"a","output":"ok"}],"tools":[{"type":"function","name":"read","parameters":{"type":"object"}}]});
        let chat = responses_to_chat(&input).unwrap();
        assert_eq!(chat["messages"][0]["tool_calls"][0]["id"], "a");
        assert_eq!(chat["messages"][1]["tool_call_id"], "a");
        let mut encoder = Encoder::new();
        encoder.decode(&json!({"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"write","arguments":"{"}},
            {"index":0,"id":"a","function":{"name":"read","arguments":"{}"}}]}}]}).to_string()).unwrap();
        encoder.decode(&json!({"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"p\":1}"}}]},"finish_reason":"tool_calls"}]}).to_string()).unwrap();
        let chunks = encoder.decode("[DONE]").unwrap();
        assert!(String::from_utf8(chunks.last().unwrap().to_vec())
            .unwrap()
            .contains("response.completed"));
        assert_eq!(encoder.output[0]["arguments"], "{\"p\":1}");
        assert_eq!(encoder.output[1]["call_id"], "a");
    }
}
