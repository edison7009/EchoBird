//! The router uses Chat Completions internally; account APIs keep their own wire format.

use std::collections::BTreeMap;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use bytes::Bytes;
use serde_json::{json, Value};

use super::tools::{self, RESPONSES_REQUEST};
use super::ProxyError;

pub(super) fn reject_structured_output(body: &Value) -> Result<(), ProxyError> {
    if body
        .get("response_format")
        .is_some_and(|format| !format.is_null() && format["type"] != "text")
    {
        let mut error = ProxyError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "This account protocol cannot enforce the requested structured output format",
        );
        error.error_code = Some("unsupported_parameter".into());
        error.model_scope = true;
        return Err(error);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WireProtocol {
    Chat,
    Responses,
    CodeAssist,
    Anthropic,
}

pub(crate) fn text(value: &Value) -> String {
    if let Some(value) = value.as_str() {
        return value.to_string();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn chat_to_responses(body: &Value, model: &str) -> Value {
    if let Some(original) = body.get(RESPONSES_REQUEST) {
        let mut request = original.clone();
        request["model"] = json!(model);
        request["stream"] = json!(true);
        request["store"] = json!(false);
        if !request["instructions"].is_string() {
            request["instructions"] = json!("");
        }
        if request.get("include").is_none() {
            request["include"] = json!(["reasoning.encrypted_content"]);
        }
        super::responses::repair_responses_tool_history(&mut request);
        return request;
    }
    let mut input = Vec::new();
    let mut instructions = Vec::new();
    for message in super::responses::repaired_chat_messages(body) {
        let role = message["role"].as_str().unwrap_or("user");
        if matches!(role, "system" | "developer") {
            instructions.push(text(&message["content"]));
            continue;
        }
        if role == "tool" {
            input.push(
                json!({"type":"function_call_output", "call_id":message["tool_call_id"],
                "output":message["content"]}),
            );
            continue;
        }
        input.extend(
            message["_echobird_response_items"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
        let content = responses_content(&message["content"], role == "assistant");
        if !content.is_empty() {
            input.push(json!({"role":role,"content":content}));
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            let mut item = json!({"type":"function_call","call_id":call["id"],
                "name":call["function"]["name"],"arguments":call["function"]["arguments"]});
            if let Some(id) = call.get("_echobird_response_item_id") {
                item["id"] = id.clone();
            }
            input.push(item);
        }
    }
    let tools: Vec<Value> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let mut function = tool["function"].as_object()?.clone();
            function.insert("type".into(), json!("function"));
            Some(Value::Object(function))
        })
        .collect();
    let mut output = json!({"model":model,"input":input,"instructions":instructions.join("\n\n"),
        "tools":tools,"stream":true,"store":false,"include":["reasoning.encrypted_content"]});
    for key in ["parallel_tool_calls", "reasoning", "text"] {
        if let Some(value) = body.get(key) {
            output[key] = value.clone();
        }
    }
    if let Some(format) = body
        .get("response_format")
        .filter(|format| !format.is_null())
    {
        let format = if let Some(mut schema) = format
            .get("json_schema")
            .and_then(Value::as_object)
            .cloned()
            .filter(|_| format["type"] == "json_schema")
        {
            schema.insert("type".into(), json!("json_schema"));
            Value::Object(schema)
        } else {
            format.clone()
        };
        let mut text = output["text"].as_object().cloned().unwrap_or_default();
        text.insert("format".into(), format);
        output["text"] = Value::Object(text);
    }
    if let Some(effort) = body.get("reasoning_effort") {
        output["reasoning"] = json!({"effort":effort});
    }
    if let Some(choice) = body.get("tool_choice") {
        output["tool_choice"] = if choice.is_object() {
            json!({"type":"function","name":choice["function"]["name"]})
        } else {
            choice.clone()
        };
    }
    super::responses::normalize_responses_tool_rounds(&mut output);
    output
}

fn responses_content(content: &Value, assistant: bool) -> Vec<Value> {
    let kind = if assistant {
        "output_text"
    } else {
        "input_text"
    };
    if let Some(value) = content.as_str() {
        return (!value.is_empty())
            .then(|| json!({"type":kind,"text":value}))
            .into_iter()
            .collect();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| match part["type"].as_str()? {
            "text" | "input_text" | "output_text" => Some(json!({"type":kind,"text":part["text"]})),
            "image_url" => {
                let mut image = json!({"type":"input_image","image_url":part["image_url"]["url"]});
                if let Some(detail) = part["image_url"].get("detail") {
                    image["detail"] = detail.clone();
                }
                Some(image)
            }
            "input_image" | "input_file" => Some(part.clone()),
            _ => None,
        })
        .collect()
}

fn signed_call_id(id: &str, signature: &str) -> Result<String, String> {
    super::signatures::encode(id, signature)
}

fn split_call_id(id: &str) -> (String, Option<String>) {
    if let Some((original, signature)) = super::signatures::decode(id) {
        return (original, Some(signature));
    }
    let Some((bare, suffix)) = id.rsplit_once("__ebsig") else {
        return (id.to_string(), None);
    };
    let Some((length, encoded)) = suffix.split_once("__") else {
        return (id.to_string(), None);
    };
    if length.parse::<usize>().ok() != Some(encoded.len()) {
        return (bare.to_string(), None);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(encoded)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok());
    (bare.to_string(), signature)
}

pub(crate) fn chat_to_code_assist(body: &Value, model: &str, project: &str) -> Value {
    let mut contents: Vec<Value> = Vec::new();
    let mut system = Vec::new();
    let mut calls = BTreeMap::new();
    let messages = super::responses::repaired_chat_messages(body);
    for message in &messages {
        let role = message["role"].as_str().unwrap_or("user");
        if matches!(role, "system" | "developer") {
            system.push(json!({"text":text(&message["content"])}));
            continue;
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            let id = call["id"].as_str().unwrap_or_default();
            calls.insert(id.to_string(), call["function"]["name"].clone());
        }
        let mut parts = if role == "tool" {
            let id = message["tool_call_id"].as_str().unwrap_or_default();
            let (bare, _) = split_call_id(id);
            vec![
                json!({"functionResponse":{"id":bare,"name":calls.get(id).cloned().unwrap_or(json!("tool")),
                "response":{"output":message["content"]}}}),
            ]
        } else {
            gemini_content(&message["content"])
        };
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            let (id, signature) = split_call_id(call["id"].as_str().unwrap_or_default());
            let arguments = call["function"]["arguments"]
                .as_str()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
                .unwrap_or(json!({}));
            parts.push(json!({"functionCall":{"id":id,"name":call["function"]["name"],"args":arguments},
                "thoughtSignature":signature.unwrap_or_else(|| "skip_thought_signature_validator".into())}));
        }
        if parts.is_empty() {
            continue;
        }
        let wire_role = if role == "assistant" { "model" } else { "user" };
        if role == "tool" {
            if let Some(previous) = contents.last_mut().filter(|v: &&mut Value| {
                v["role"] == "user" && v["parts"][0].get("functionResponse").is_some()
            }) {
                previous["parts"].as_array_mut().unwrap().extend(parts);
                continue;
            }
        }
        contents.push(json!({"role":wire_role,"parts":parts}));
    }
    let mut request = json!({"contents":contents});
    if !system.is_empty() {
        request["systemInstruction"] = json!({"parts":system});
    }
    let declarations: Vec<Value> = body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| {
            let function = tool["function"].as_object()?;
            Some(json!({"name":function.get("name"),"description":function.get("description"),
                "parameters":plain_schema(function.get("parameters").unwrap_or(&json!({"type":"object","properties":{}})))}))
        })
        .collect();
    if !declarations.is_empty() {
        request["tools"] = json!([{"functionDeclarations":declarations}]);
    }
    let mut generation = json!({});
    for (from, to) in [
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("max_tokens", "maxOutputTokens"),
    ] {
        if let Some(value) = body.get(from) {
            generation[to] = value.clone();
        }
    }
    if let Some(stop) = body.get("stop") {
        generation["stopSequences"] = if stop.is_string() {
            json!([stop])
        } else {
            stop.clone()
        };
    }
    if generation.as_object().is_some_and(|v| !v.is_empty()) {
        request["generationConfig"] = generation;
    }
    if let Some(choice) = body.get("tool_choice") {
        let mode = match choice.as_str() {
            Some("none") => "NONE",
            Some("required") => "ANY",
            _ if choice.is_object() => "ANY",
            _ => "AUTO",
        };
        let mut config = json!({"mode":mode});
        if choice.is_object() {
            config["allowedFunctionNames"] = json!([choice["function"]["name"]]);
        }
        request["toolConfig"] = json!({"functionCallingConfig":config});
    }
    json!({"model":model,"project":project,"userAgent":"antigravity","requestType":"agent",
        "requestId":format!("agent-{}",uuid::Uuid::new_v4()),"request":request})
}

fn gemini_content(content: &Value) -> Vec<Value> {
    if let Some(value) = content.as_str() {
        return (!value.is_empty())
            .then(|| json!({"text":value}))
            .into_iter()
            .collect();
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| match part["type"].as_str()? {
            "text" => Some(json!({"text":part["text"]})),
            "image_url" => {
                let url = part["image_url"]["url"].as_str()?;
                if let Some(data) = url.strip_prefix("data:") {
                    let (mime, bytes) = data.split_once(";base64,")?;
                    Some(json!({"inlineData":{"mimeType":mime,"data":bytes}}))
                } else {
                    Some(json!({"fileData":{"mimeType":"image/png","fileUri":url}}))
                }
            }
            _ => None,
        })
        .collect()
}

fn plain_schema(value: &Value) -> Value {
    clean_schema(value, value, 0)
}

fn clean_schema(value: &Value, root: &Value, depth: usize) -> Value {
    if depth > 20 {
        return json!({"type":"object"});
    }
    if let Some(reference) = value["$ref"].as_str().and_then(|v| v.strip_prefix('#')) {
        if let Some(target) = root.pointer(reference) {
            return clean_schema(target, root, depth + 1);
        }
    }
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "$schema"
                            | "$id"
                            | "$defs"
                            | "$ref"
                            | "additionalProperties"
                            | "default"
                            | "examples"
                    )
                })
                .map(|(key, value)| (key.clone(), clean_schema(value, root, depth + 1)))
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|v| clean_schema(v, root, depth + 1))
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[derive(Default)]
pub(crate) struct SseParser {
    pending: Vec<u8>,
}

impl SseParser {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<(String, String)> {
        self.pending.extend_from_slice(bytes);
        let mut frames = Vec::new();
        loop {
            let separator = [
                self.pending
                    .windows(2)
                    .position(|v| v == b"\n\n")
                    .map(|at| (at, 2)),
                self.pending
                    .windows(4)
                    .position(|v| v == b"\r\n\r\n")
                    .map(|at| (at, 4)),
            ]
            .into_iter()
            .flatten()
            .min_by_key(|(at, _)| *at);
            let Some((at, length)) = separator else { break };
            let raw = self.pending.drain(..at + length).collect::<Vec<_>>();
            let block = String::from_utf8_lossy(&raw[..at]);
            let mut event = String::new();
            let mut data = Vec::new();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    event = value.trim_start().to_string();
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if !data.is_empty() {
                frames.push((event, data.join("\n")));
            }
        }
        frames
    }

    pub(crate) fn finish(&mut self) -> Vec<(String, String)> {
        if self.pending.is_empty() {
            Vec::new()
        } else {
            self.push(b"\n\n")
        }
    }
}

pub(crate) fn chat_sse(value: Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}

#[derive(Default)]
struct ToolState {
    index: usize,
    id: String,
    name: String,
    arguments: String,
    response_item_id: Option<String>,
}

pub(crate) struct ChatDecoder {
    protocol: WireProtocol,
    id: String,
    model: String,
    started: bool,
    pub(crate) finished: bool,
    tools: BTreeMap<String, ToolState>,
    custom_inputs: BTreeMap<String, String>,
    response_items: Vec<Value>,
    text: String,
    reasoning: String,
    usage: Value,
    finish_reason: String,
}

impl ChatDecoder {
    pub(crate) fn new(protocol: WireProtocol, model: &str) -> Self {
        Self {
            protocol,
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
            model: model.into(),
            started: false,
            finished: false,
            tools: BTreeMap::new(),
            custom_inputs: BTreeMap::new(),
            response_items: Vec::new(),
            text: String::new(),
            reasoning: String::new(),
            usage: json!({}),
            finish_reason: "stop".into(),
        }
    }

    fn chunk(&self, delta: Value, reason: Value) -> Bytes {
        chat_sse(
            json!({"id":self.id,"object":"chat.completion.chunk","created":chrono::Utc::now().timestamp(),
            "model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":reason}]}),
        )
    }

    pub(crate) fn emit_text(&mut self, value: &str, reasoning: bool) -> Bytes {
        if reasoning {
            self.reasoning.push_str(value);
        } else {
            self.text.push_str(value);
        }
        self.chunk(
            if reasoning {
                json!({"reasoning_content":value})
            } else {
                json!({"content":value})
            },
            Value::Null,
        )
    }

    pub(crate) fn emit_tool(
        &mut self,
        key: &str,
        id: &str,
        name: &str,
        arguments: &str,
        snapshot: bool,
    ) -> Vec<Bytes> {
        let next_index = self.tools.len();
        let is_new = !self.tools.contains_key(key);
        let state = self.tools.entry(key.to_string()).or_default();
        if is_new {
            state.index = next_index;
            state.id = id.into();
            state.name = name.into();
            if self.protocol == WireProtocol::Responses {
                state.response_item_id = Some(key.into());
            }
        }
        let metadata_changed = !is_new
            && !name.is_empty()
            && (state.name.is_empty() || state.id != id && !id.is_empty());
        if state.name.is_empty() && !name.is_empty() {
            state.name = name.into();
        }
        if !id.is_empty() && metadata_changed {
            state.id = id.into();
        }
        let index = state.index;
        let appended = if snapshot {
            arguments.strip_prefix(&state.arguments).unwrap_or("")
        } else {
            arguments
        };
        let appended = appended.to_string();
        state.arguments.push_str(&appended);
        let mut function = json!({"arguments":appended});
        let mut call = json!({"index":index,"function":function});
        if is_new || metadata_changed {
            function["name"] = json!(state.name);
            call["function"] = function;
            call["id"] = json!(state.id);
            call["type"] = json!("function");
            if let Some(item_id) = &state.response_item_id {
                call["_echobird_response_item_id"] = json!(item_id);
            }
        }
        if !is_new && !metadata_changed && appended.is_empty() {
            return Vec::new();
        }
        vec![self.chunk(json!({"tool_calls":[call]}), Value::Null)]
    }

    pub(crate) fn complete(&mut self, reason: &str) -> Vec<Bytes> {
        if self.finished {
            return Vec::new();
        }
        self.finished = true;
        self.finish_reason = reason.into();
        let mut result = vec![self.chunk(json!({}), json!(reason))];
        if self.usage.is_object() && !self.usage.as_object().unwrap().is_empty() {
            result.push(chat_sse(
                json!({"id":self.id,"object":"chat.completion.chunk","model":self.model,
                "choices":[],"usage":self.usage}),
            ));
        }
        result.push(Bytes::from_static(b"data: [DONE]\n\n"));
        result
    }

    pub(crate) fn decode(&mut self, event: &str, data: &str) -> Result<Vec<Bytes>, String> {
        if self.finished {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            return if self.protocol == WireProtocol::Chat {
                Ok(self.complete(&self.finish_reason.clone()))
            } else {
                Err("Account stream ended before its protocol completion event".into())
            };
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|error| format!("Invalid account stream: {error}"))?;
        if event == "error"
            || value.get("error").is_some_and(|error| !error.is_null())
            || (value["error_code"].is_string() && value["error_msg"].is_string())
            || matches!(value["type"].as_str(), Some("error" | "response.failed"))
        {
            let error = value
                .get("error")
                .filter(|error| !error.is_null())
                .or_else(|| value["response"].get("error"))
                .unwrap_or(&value);
            return Err(error["message"]
                .as_str()
                .or_else(|| error["error_msg"].as_str())
                .unwrap_or("Account inference failed")
                .to_string());
        }
        if self.protocol == WireProtocol::Responses
            && matches!(
                value["type"].as_str(),
                Some("response.completed" | "response.incomplete")
            )
        {
            let response = &value["response"];
            if !response["error"].is_null() {
                return Err(response["error"]["message"]
                    .as_str()
                    .unwrap_or("Account inference failed")
                    .to_string());
            }
            let kind = value["type"].as_str().expect("Responses terminal type");
            if (!event.is_empty() && event != kind)
                || response["object"] != "response"
                || response["status"] != kind.trim_start_matches("response.")
                || response["id"].as_str().is_none_or(|id| id.is_empty())
            {
                return Err("Responses terminal event has no matching terminal response".into());
            }
        }
        let mut output = Vec::new();
        if !self.started {
            self.started = true;
            output.push(self.chunk(json!({"role":"assistant"}), Value::Null));
        }
        match self.protocol {
            WireProtocol::Responses => self.decode_responses(&value, &mut output),
            WireProtocol::CodeAssist => self.decode_code_assist(&value, &mut output)?,
            WireProtocol::Anthropic => self.decode_anthropic(&value, &mut output),
            WireProtocol::Chat => {
                if let Some(usage) = value.get("usage") {
                    self.usage = usage.clone();
                }
                for choice in value["choices"].as_array().into_iter().flatten().take(1) {
                    let delta = choice
                        .get("delta")
                        .or_else(|| choice.get("message"))
                        .unwrap_or(&Value::Null);
                    if let Some(content) = delta["content"].as_str() {
                        output.push(self.emit_text(content, false));
                    }
                    if let Some(content) = delta["reasoning_content"].as_str() {
                        output.push(self.emit_text(content, true));
                    }
                    for call in delta["tool_calls"].as_array().into_iter().flatten() {
                        let key = call["index"].as_u64().unwrap_or(0).to_string();
                        output.extend(self.emit_tool(
                            &key,
                            call["id"].as_str().unwrap_or(&key),
                            call["function"]["name"].as_str().unwrap_or_default(),
                            call["function"]["arguments"].as_str().unwrap_or_default(),
                            false,
                        ));
                    }
                    if let Some(reason) = choice["finish_reason"].as_str() {
                        self.finish_reason = reason.into();
                    }
                }
            }
        }
        Ok(output)
    }

    fn decode_responses(&mut self, value: &Value, output: &mut Vec<Bytes>) {
        match value["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                output.push(self.emit_text(value["delta"].as_str().unwrap_or_default(), false))
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                output.push(self.emit_text(value["delta"].as_str().unwrap_or_default(), true))
            }
            "response.output_item.added" | "response.output_item.done"
                if value["item"]["type"] == "reasoning" =>
            {
                output.push(self.emit_response_item(
                    &value["item"],
                    value["type"] == "response.output_item.done",
                ));
            }
            "response.output_item.added" | "response.output_item.done"
                if value["item"]["type"] == "function_call" =>
            {
                let item = &value["item"];
                let key = item["id"].as_str().unwrap_or_default();
                output.extend(self.emit_tool(
                    key,
                    item["call_id"].as_str().unwrap_or(key),
                    &tools::alias(
                        item["namespace"].as_str().unwrap_or_default(),
                        item["name"].as_str().unwrap_or_default(),
                    ),
                    item["arguments"].as_str().unwrap_or_default(),
                    true,
                ));
            }
            "response.output_item.added" | "response.output_item.done"
                if value["item"]["type"] == "custom_tool_call" =>
            {
                let item = &value["item"];
                let key = item["id"].as_str().unwrap_or_default();
                let done = value["type"] == "response.output_item.done";
                let input = item["input"].as_str().unwrap_or_default();
                self.custom_inputs.insert(key.into(), input.into());
                let arguments = if done {
                    json!({"input":input}).to_string()
                } else {
                    String::new()
                };
                output.extend(self.emit_tool(
                    key,
                    item["call_id"].as_str().unwrap_or(key),
                    &tools::alias(
                        item["namespace"].as_str().unwrap_or_default(),
                        item["name"].as_str().unwrap_or_default(),
                    ),
                    &arguments,
                    true,
                ));
            }
            "response.custom_tool_call_input.delta" => {
                let key = value["item_id"].as_str().unwrap_or_default();
                self.custom_inputs
                    .entry(key.into())
                    .or_default()
                    .push_str(value["delta"].as_str().unwrap_or_default());
            }
            "response.custom_tool_call_input.done" => {
                let key = value["item_id"].as_str().unwrap_or_default();
                let input = value["input"]
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.custom_inputs.get(key).cloned().unwrap_or_default());
                self.custom_inputs.insert(key.into(), input.clone());
                output.extend(self.emit_tool(
                    key,
                    key,
                    "",
                    &json!({"input":input}).to_string(),
                    true,
                ));
            }
            "response.function_call_arguments.delta" => {
                let key = value["item_id"].as_str().unwrap_or_default();
                output.extend(self.emit_tool(
                    key,
                    key,
                    "",
                    value["delta"].as_str().unwrap_or_default(),
                    false,
                ));
            }
            "response.function_call_arguments.done" => {
                let key = value["item_id"].as_str().unwrap_or_default();
                output.extend(self.emit_tool(
                    key,
                    key,
                    "",
                    value["arguments"].as_str().unwrap_or_default(),
                    true,
                ));
            }
            "response.completed" | "response.incomplete" => {
                let response = &value["response"];
                let mut full_text = String::new();
                for item in response["output"].as_array().into_iter().flatten() {
                    if item["type"] == "reasoning" {
                        if !self
                            .response_items
                            .iter()
                            .any(|existing| existing["id"] == item["id"])
                        {
                            output.push(self.emit_response_item(item, true));
                        }
                    } else if item["type"] == "function_call" {
                        let key = item["id"].as_str().unwrap_or_default();
                        output.extend(self.emit_tool(
                            key,
                            item["call_id"].as_str().unwrap_or(key),
                            &tools::alias(
                                item["namespace"].as_str().unwrap_or_default(),
                                item["name"].as_str().unwrap_or_default(),
                            ),
                            item["arguments"].as_str().unwrap_or_default(),
                            true,
                        ));
                    } else if item["type"] == "custom_tool_call" {
                        let key = item["id"].as_str().unwrap_or_default();
                        let input =
                            item["input"]
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| {
                                    self.custom_inputs.get(key).cloned().unwrap_or_default()
                                });
                        output.extend(self.emit_tool(
                            key,
                            item["call_id"].as_str().unwrap_or(key),
                            &tools::alias(
                                item["namespace"].as_str().unwrap_or_default(),
                                item["name"].as_str().unwrap_or_default(),
                            ),
                            &json!({"input":input}).to_string(),
                            true,
                        ));
                    }
                    for part in item["content"].as_array().into_iter().flatten() {
                        if let Some(text) = part["text"].as_str() {
                            full_text.push_str(text);
                        }
                    }
                }
                let suffix = full_text
                    .strip_prefix(&self.text)
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned);
                if let Some(suffix) = suffix {
                    output.push(self.emit_text(&suffix, false));
                }
                if let Some(usage) = response.get("usage") {
                    self.set_usage(usage, false);
                }
                let reason = if value["type"] == "response.incomplete" {
                    "length"
                } else if self.tools.is_empty() {
                    "stop"
                } else {
                    "tool_calls"
                };
                output.extend(self.complete(reason));
            }
            _ => {}
        }
    }

    fn emit_response_item(&mut self, item: &Value, complete: bool) -> Bytes {
        if complete {
            if let Some(existing) = self
                .response_items
                .iter_mut()
                .find(|existing| existing["id"] == item["id"])
            {
                *existing = item.clone();
            } else {
                self.response_items.push(item.clone());
            }
        }
        self.chunk(json!({"_echobird_response_item":item}), Value::Null)
    }

    fn decode_code_assist(&mut self, value: &Value, output: &mut Vec<Bytes>) -> Result<(), String> {
        let response = value.get("response").unwrap_or(value);
        if let Some(usage) = response.get("usageMetadata") {
            self.set_usage(usage, true);
        }
        for candidate in response["candidates"]
            .as_array()
            .into_iter()
            .flatten()
            .take(1)
        {
            for (index, part) in candidate["content"]["parts"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(text) = part["text"].as_str() {
                    output.push(self.emit_text(text, part["thought"].as_bool().unwrap_or(false)));
                }
                if let Some(call) = part.get("functionCall") {
                    let key = call["id"].as_str().map(str::to_string).unwrap_or_else(|| {
                        format!("call_{}_{index}", uuid::Uuid::new_v4().simple())
                    });
                    let id = signed_call_id(
                        &key,
                        part["thoughtSignature"].as_str().unwrap_or_default(),
                    )?;
                    let arguments = call.get("args").cloned().unwrap_or(json!({})).to_string();
                    output.extend(self.emit_tool(
                        &key,
                        &id,
                        call["name"].as_str().unwrap_or_default(),
                        &arguments,
                        true,
                    ));
                }
            }
            if let Some(reason) = candidate["finishReason"].as_str() {
                let reason = if reason == "MAX_TOKENS" {
                    "length"
                } else if matches!(
                    reason,
                    "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT"
                ) {
                    "content_filter"
                } else if !self.tools.is_empty() {
                    "tool_calls"
                } else {
                    "stop"
                };
                output.extend(self.complete(reason));
            }
        }
        Ok(())
    }

    fn decode_anthropic(&mut self, value: &Value, output: &mut Vec<Bytes>) {
        match value["type"].as_str().unwrap_or_default() {
            "message_start" => {
                if let Some(usage) = value["message"].get("usage") {
                    self.set_usage(usage, false);
                }
            }
            "content_block_start" if value["content_block"]["type"] == "tool_use" => {
                let block = &value["content_block"];
                let key = value["index"].as_u64().unwrap_or(0).to_string();
                let arguments = block
                    .get("input")
                    .filter(|v| v.as_object().is_some_and(|o| !o.is_empty()))
                    .map(Value::to_string)
                    .unwrap_or_default();
                output.extend(self.emit_tool(
                    &key,
                    block["id"].as_str().unwrap_or(&key),
                    block["name"].as_str().unwrap_or_default(),
                    &arguments,
                    true,
                ));
            }
            "content_block_delta" => {
                let delta = &value["delta"];
                match delta["type"].as_str().unwrap_or_default() {
                    "text_delta" => output
                        .push(self.emit_text(delta["text"].as_str().unwrap_or_default(), false)),
                    "thinking_delta" => output
                        .push(self.emit_text(delta["thinking"].as_str().unwrap_or_default(), true)),
                    "input_json_delta" => {
                        let key = value["index"].as_u64().unwrap_or(0).to_string();
                        output.extend(self.emit_tool(
                            &key,
                            &key,
                            "",
                            delta["partial_json"].as_str().unwrap_or_default(),
                            false,
                        ));
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(tokens) = value["usage"]["output_tokens"].as_u64() {
                    self.usage["completion_tokens"] = json!(tokens);
                    self.usage["total_tokens"] =
                        json!(self.usage["prompt_tokens"].as_u64().unwrap_or(0) + tokens);
                }
                self.finish_reason = match value["delta"]["stop_reason"].as_str() {
                    Some("tool_use") => "tool_calls",
                    Some("max_tokens") => "length",
                    _ => "stop",
                }
                .into();
            }
            "message_stop" => output.extend(self.complete(&self.finish_reason.clone())),
            _ => {}
        }
    }

    pub(crate) fn set_usage(&mut self, usage: &Value, gemini: bool) {
        let mut prompt = usage[if gemini {
            "promptTokenCount"
        } else {
            "input_tokens"
        }]
        .as_u64()
        .unwrap_or(0);
        let reasoning = usage[if gemini {
            "thoughtsTokenCount"
        } else {
            "output_tokens_details"
        }]
        .as_u64()
        .or_else(|| usage["output_tokens_details"]["reasoning_tokens"].as_u64())
        .unwrap_or(0);
        if !gemini {
            prompt += usage["cache_creation_input_tokens"].as_u64().unwrap_or(0)
                + usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
        }
        let output = usage[if gemini {
            "candidatesTokenCount"
        } else {
            "output_tokens"
        }]
        .as_u64()
        .unwrap_or(0)
            + if gemini { reasoning } else { 0 };
        let total = usage[if gemini {
            "totalTokenCount"
        } else {
            "total_tokens"
        }]
        .as_u64()
        .unwrap_or(prompt + output);
        self.usage =
            json!({"prompt_tokens":prompt,"completion_tokens":output,"total_tokens":total});
        if reasoning > 0 {
            self.usage["completion_tokens_details"] = json!({"reasoning_tokens":reasoning});
        }
        let cached = if gemini {
            usage["cachedContentTokenCount"].as_u64()
        } else {
            usage["input_tokens_details"]["cached_tokens"]
                .as_u64()
                .or_else(|| usage["cache_read_input_tokens"].as_u64())
        };
        if let Some(cached) = cached {
            self.usage["prompt_tokens_details"] = json!({"cached_tokens":cached});
        }
    }

    pub(crate) fn collected(&self) -> Value {
        let mut tools: Vec<_> = self.tools.values().collect();
        tools.sort_by_key(|tool| tool.index);
        let calls: Vec<Value> = tools
            .into_iter()
            .map(|tool| {
                let mut call = json!({"id":tool.id,"type":"function",
                    "function":{"name":tool.name,"arguments":tool.arguments}});
                if let Some(id) = &tool.response_item_id {
                    call["_echobird_response_item_id"] = json!(id);
                }
                call
            })
            .collect();
        let mut message = json!({"role":"assistant","content":self.text});
        if !calls.is_empty() {
            message["tool_calls"] = json!(calls);
        }
        if !self.reasoning.is_empty() {
            message["reasoning_content"] = json!(self.reasoning);
        }
        if !self.response_items.is_empty() {
            message["_echobird_response_items"] = json!(self.response_items);
        }
        json!({"id":self.id,"object":"chat.completion","created":chrono::Utc::now().timestamp(),"model":self.model,
            "choices":[{"index":0,"message":message,"finish_reason":self.finish_reason}],"usage":self.usage})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interrupted_tool_history() -> Value {
        json!({"messages":[
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"a","type":"function","function":{"name":"read","arguments":"{}"}},
                {"id":"b","type":"function","function":{"name":"write","arguments":"{}"}}
            ]},
            {"role":"tool","tool_call_id":"a","content":"first answer"},
            {"role":"tool","tool_call_id":"a","content":"duplicate answer"},
            {"role":"user","content":"Continue after interruption"},
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"a","type":"function","function":{"name":"inspect","arguments":"{}"}}
            ]},
            {"role":"tool","tool_call_id":"a","content":"later answer"},
            {"role":"tool","tool_call_id":"a","content":"later duplicate"}
        ]})
    }

    #[test]
    fn tool_history_responses_repairs_interruption_and_duplicate_answers_per_round() {
        let body = interrupted_tool_history();
        let before = body.clone();
        let request = chat_to_responses(&body, "native");
        assert_eq!(body, before);
        let input = request["input"].as_array().unwrap();
        let answers: Vec<_> = input
            .iter()
            .filter(|item| item["type"] == "function_call_output")
            .collect();
        assert_eq!(answers.len(), 3);
        assert_eq!(answers[0]["output"], "first answer");
        assert_eq!(answers[1]["call_id"], "b");
        assert!(answers[1]["output"]
            .as_str()
            .unwrap()
            .contains("interrupted"));
        assert_eq!(answers[2]["output"], "later answer");
        let followup = input
            .iter()
            .position(|item| item["role"] == "user")
            .unwrap();
        assert!(input[..followup]
            .iter()
            .any(|item| item["type"] == "function_call_output" && item["call_id"] == "b"));
    }

    #[test]
    fn tool_history_code_assist_has_one_answer_per_call_and_keeps_reused_ids() {
        let request = chat_to_code_assist(&interrupted_tool_history(), "gemini-test", "project");
        let contents = request["request"]["contents"].as_array().unwrap();
        assert_eq!(contents[1]["parts"].as_array().unwrap().len(), 2);
        assert_eq!(
            contents[1]["parts"][0]["functionResponse"]["response"]["output"],
            "first answer"
        );
        assert_eq!(contents[1]["parts"][0]["functionResponse"]["name"], "read");
        assert_eq!(contents[1]["parts"][1]["functionResponse"]["id"], "b");
        assert!(
            contents[1]["parts"][1]["functionResponse"]["response"]["output"]
                .as_str()
                .unwrap()
                .contains("interrupted")
        );
        assert_eq!(
            contents[2]["parts"][0]["text"],
            "Continue after interruption"
        );
        assert_eq!(contents[4]["parts"].as_array().unwrap().len(), 1);
        assert_eq!(
            contents[4]["parts"][0]["functionResponse"]["response"]["output"],
            "later answer"
        );
        assert_eq!(
            contents[4]["parts"][0]["functionResponse"]["name"],
            "inspect"
        );
    }

    #[test]
    fn tool_history_messages_to_responses_repairs_a_stopped_tool_turn() {
        let input = json!({"messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"a","name":"read","input":{"path":"fixture"}}]},
            {"role":"user","content":"Continue after interruption"}
        ]});
        let chat = crate::services::local_llm::proxy::anthropic_to_openai(&input);
        let request = chat_to_responses(&chat, "native");
        assert_eq!(request["input"][0]["type"], "function_call");
        assert_eq!(request["input"][1]["type"], "function_call_output");
        assert_eq!(request["input"][1]["call_id"], "a");
        assert_eq!(request["input"][2]["role"], "user");
    }

    #[test]
    fn tool_history_native_responses_preserves_custom_items_and_continuations() {
        let original = json!({"model":"auto","input":[
            {"type":"reasoning","id":"r1","encrypted_content":"opaque","summary":[]},
            {"type":"custom_tool_call","id":"ctc1","call_id":"a","namespace":"fixture","name":"write","input":"raw patch","future_field":true},
            {"type":"custom_tool_call_output","call_id":"a","output":[{"type":"input_text","text":"first answer"}]},
            {"type":"custom_tool_call_output","call_id":"a","output":"duplicate"},
            {"role":"user","content":"Continue"},
            {"type":"function_call","call_id":"a","name":"read","arguments":"{}"}
        ],"tools":[{"type":"namespace","name":"fixture","tools":[{"type":"custom","name":"write","format":{"type":"text"}}]}],"metadata":{"keep":true}});
        let request = chat_to_responses(&json!({RESPONSES_REQUEST:original}), "native");
        let items = request["input"].as_array().unwrap();
        assert_eq!(items.len(), 6);
        assert_eq!(items[0], original["input"][0]);
        assert_eq!(items[1], original["input"][1]);
        assert_eq!(items[2], original["input"][2]);
        assert_eq!(items[5]["type"], "function_call_output");
        assert!(items[5]["output"].as_str().unwrap().contains("interrupted"));
        assert_eq!(request["tools"], original["tools"]);
        assert_eq!(request["metadata"], original["metadata"]);

        let incremental = json!({"previous_response_id":"unknown-upstream-id","input":[
            {"type":"function_call_output","call_id":"old-call","output":"already known upstream"},
            {"type":"function_call","call_id":"new-call","name":"read","arguments":"{}"}
        ]});
        let request = chat_to_responses(&json!({RESPONSES_REQUEST:incremental}), "native");
        assert_eq!(request["input"], incremental["input"]);
        assert_eq!(
            request["previous_response_id"],
            incremental["previous_response_id"]
        );
    }

    #[test]
    fn chat_structured_format_maps_to_native_responses_text_format() {
        let schema = json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false});
        let request = chat_to_responses(
            &json!({"messages":[{"role":"user","content":"Answer"}],"response_format":{"type":"json_schema","json_schema":{"name":"answer","description":"An answer","schema":schema,"strict":true}}}),
            "native",
        );
        assert_eq!(
            request["text"]["format"],
            json!({"type":"json_schema","name":"answer","description":"An answer","schema":schema,"strict":true})
        );
        for kind in ["text", "json_object"] {
            let request = chat_to_responses(
                &json!({"messages":[],"response_format":{"type":kind}}),
                "native",
            );
            assert_eq!(request["text"]["format"], json!({"type":kind}));
        }
        let invalid = json!({"type":"json_schema","json_schema":"invalid"});
        assert_eq!(
            chat_to_responses(&json!({"messages":[],"response_format":invalid}), "native")["text"]
                ["format"],
            invalid
        );
        let request = chat_to_responses(
            &json!({"messages":[],"text":"invalid","response_format":{"type":"json_object"}}),
            "native",
        );
        assert_eq!(request["text"]["format"], json!({"type":"json_object"}));
    }

    #[test]
    fn mixed_newlines_and_parallel_tool_indexes_remain_stable() {
        let mut parser = SseParser::default();
        assert_eq!(
            parser.push(b"data: first\r\n\r\ndata: second\n\n"),
            vec![
                (String::new(), "first".into()),
                (String::new(), "second".into())
            ]
        );
        let mut decoder = ChatDecoder::new(WireProtocol::Responses, "test");
        decoder.emit_tool("z", "call-z", "write", "{", false);
        decoder.emit_tool("a", "call-a", "read", "{}", false);
        let chunks = decoder.emit_tool("z", "z", "", "}", false);
        let frames = parser.push(&chunks[0]);
        let value: Value = serde_json::from_str(&frames[0].1).unwrap();
        assert_eq!(value["choices"][0]["delta"]["tool_calls"][0]["index"], 0);
        assert_eq!(
            decoder.collected()["choices"][0]["message"]["tool_calls"][0]["id"],
            "call-z"
        );
    }

    #[test]
    fn gemini_calls_without_upstream_ids_remain_distinct_between_frames() {
        let mut decoder = ChatDecoder::new(WireProtocol::CodeAssist, "test");
        for path in ["first", "second"] {
            decoder.decode("",&json!({"response":{"candidates":[{"content":{"parts":[{"functionCall":{"name":"read","args":{"path":path}}}]}}]}}).to_string()).unwrap();
        }
        decoder.decode("",&json!({"response":{"candidates":[{"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"thoughtsTokenCount":4,"totalTokenCount":9}}}).to_string()).unwrap();
        let result = decoder.collected();
        let calls = result["choices"][0]["message"]["tool_calls"]
            .as_array()
            .unwrap();
        assert_eq!(calls.len(), 2);
        assert_ne!(calls[0]["id"], calls[1]["id"]);
        assert_eq!(
            result["usage"]["completion_tokens_details"]["reasoning_tokens"],
            4
        );
        assert_eq!(result["usage"]["total_tokens"], 9);
    }

    #[test]
    fn gemini_schema_resolves_local_refs_before_removing_defs() {
        let schema = plain_schema(
            &json!({"type":"object","$defs":{"Address":{"type":"object","properties":{"street":{"type":"string"}}}},
            "properties":{"address":{"$ref":"#/$defs/Address"}},"additionalProperties":false}),
        );
        assert_eq!(
            schema["properties"]["address"]["properties"]["street"]["type"],
            "string"
        );
        assert!(schema.get("$defs").is_none());
    }

    #[test]
    fn chat_usage_after_finish_is_retained_and_length_is_not_lost() {
        let mut decoder = ChatDecoder::new(WireProtocol::Chat, "test");
        for value in [
            json!({"choices":[{"delta":{"content":"ok"},"finish_reason":"length"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}),
        ] {
            decoder.decode("", &value.to_string()).unwrap();
        }
        decoder.decode("", "[DONE]").unwrap();
        assert_eq!(decoder.collected()["choices"][0]["finish_reason"], "length");
        assert_eq!(decoder.collected()["usage"]["total_tokens"], 3);
    }

    #[test]
    fn responses_preserves_tools_results_and_images() {
        let body = json!({"messages":[{"role":"system","content":"system"},{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,eA=="}}]},
            {"role":"assistant","tool_calls":[{"id":"a","function":{"name":"read","arguments":"{\"p\":1}"}}]},
            {"role":"tool","tool_call_id":"a","content":"done"}],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]});
        let request = chat_to_responses(&body, "gpt-test");
        assert_eq!(request["instructions"], "system");
        assert_eq!(request["input"][0]["content"][0]["type"], "input_image");
        assert_eq!(request["input"][1]["type"], "function_call");
        assert_eq!(request["input"][2]["call_id"], "a");
        assert_eq!(request["tools"][0]["name"], "read");
        assert_eq!(request["store"], false);
    }

    #[test]
    fn parser_keeps_split_utf8_and_crlf_frames() {
        let mut parser = SseParser::default();
        let source = "event: delta\r\ndata: {\"text\":\"模型\"}\r\n\r\n".as_bytes();
        let mut frames = Vec::new();
        for byte in source {
            frames.extend(parser.push(&[*byte]));
        }
        assert_eq!(
            frames,
            vec![("delta".to_string(), "{\"text\":\"模型\"}".to_string())]
        );
    }

    #[test]
    fn responses_tool_deltas_do_not_duplicate_final_arguments() {
        let mut decoder = ChatDecoder::new(WireProtocol::Responses, "gpt-test");
        for value in [
            json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc1","call_id":"call1","name":"read","arguments":""}}),
            json!({"type":"response.function_call_arguments.delta","item_id":"fc1","delta":"{\"p\":"}),
            json!({"type":"response.function_call_arguments.delta","item_id":"fc1","delta":"1}"}),
            json!({"type":"response.output_item.done","item":{"type":"function_call","id":"fc1","call_id":"call1","name":"read","arguments":"{\"p\":1}"}}),
            json!({"type":"response.completed","response":{"object":"response","id":"resp_tools","status":"completed","error":null,"usage":{"input_tokens":3,"output_tokens":2}}}),
        ] {
            decoder.decode("", &value.to_string()).unwrap();
        }
        let result = decoder.collected();
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"][0]["id"],
            "call1"
        );
        assert_eq!(
            result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"p\":1}"
        );
        assert_eq!(result["usage"]["total_tokens"], 5);
        assert!(decoder.finished);
    }

    #[test]
    fn code_assist_signature_returns_on_the_original_call() {
        let mut decoder = ChatDecoder::new(WireProtocol::CodeAssist, "gemini-test");
        decoder.decode("",&json!({"response":{"candidates":[{"content":{"parts":[{"functionCall":{"id":"call1","name":"read","args":{"p":1}},"thoughtSignature":"signature"}]},"finishReason":"STOP"}]}}).to_string()).unwrap();
        let message = decoder.collected()["choices"][0]["message"].clone();
        let call_id = message["tool_calls"][0]["id"].clone();
        let body =
            json!({"messages":[message,{"role":"tool","tool_call_id":call_id,"content":"ok"}]});
        let request = chat_to_code_assist(&body, "gemini-test", "project");
        assert_eq!(
            request["request"]["contents"][0]["parts"][0]["thoughtSignature"],
            "signature"
        );
        assert_eq!(
            request["request"]["contents"][0]["parts"][0]["functionCall"]["id"],
            "call1"
        );
        assert_eq!(
            request["request"]["contents"][1]["parts"][0]["functionResponse"]["name"],
            "read"
        );
    }

    #[test]
    fn maintenance_long_code_assist_signature_has_a_short_round_trip_id() {
        let signature = "signed-thought".repeat(512);
        let id = signed_call_id("call-original", &signature).unwrap();
        assert!(id.len() <= 64, "tool call id is {} bytes", id.len());
        assert_eq!(
            split_call_id(&id),
            ("call-original".into(), Some(signature))
        );
    }

    #[test]
    fn truncated_signature_is_never_sent_as_a_partial_signature() {
        let id = signed_call_id("call", "signature").unwrap();
        let (_, signature) = split_call_id(&id[..id.len() - 1]);
        assert!(signature.is_none());
        let encoded = URL_SAFE_NO_PAD.encode(b"legacy-signature");
        let legacy = format!("original__ebsig{}__{encoded}", encoded.len());
        assert_eq!(
            split_call_id(&legacy),
            ("original".into(), Some("legacy-signature".into()))
        );
        assert!(split_call_id(&legacy[..legacy.len() - 1]).1.is_none());
    }

    #[test]
    fn null_error_preserves_valid_chat_output_and_nonempty_errors_still_fail() {
        let mut decoder = ChatDecoder::new(WireProtocol::Chat, "test");
        let chunks = decoder
            .decode(
                "",
                &json!({"error":null,"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}).to_string(),
            )
            .unwrap();
        assert!(chunks
            .iter()
            .any(|chunk| String::from_utf8_lossy(chunk).contains("\"content\":\"ok\"")));
        decoder.decode("", "[DONE]").unwrap();
        assert!(decoder.finished);
        assert_eq!(
            decoder.collected()["choices"][0]["message"]["content"],
            "ok"
        );

        for (event, frame, message) in [
            (
                "",
                json!({"error":{"message":"rate limit"},"choices":[{"delta":{"content":"must not emit"}}]}),
                "rate limit",
            ),
            (
                "error",
                json!({"error":null,"message":"event failure"}),
                "event failure",
            ),
            (
                "",
                json!({"type":"response.failed","error":null,"response":{"error":{"message":"nested failure"}}}),
                "nested failure",
            ),
        ] {
            let mut decoder = ChatDecoder::new(WireProtocol::Responses, "test");
            assert_eq!(
                decoder.decode(event, &frame.to_string()).unwrap_err(),
                message
            );
            assert!(!decoder.started);
            assert!(!decoder.finished);
        }
    }

    #[test]
    fn only_chat_sentinel_completes_without_a_protocol_terminal() {
        for protocol in [
            WireProtocol::Responses,
            WireProtocol::Anthropic,
            WireProtocol::CodeAssist,
        ] {
            let mut decoder = ChatDecoder::new(protocol, "test");
            assert!(decoder.decode("", "[DONE]").is_err());
            assert!(!decoder.finished);
        }
        let mut chat = ChatDecoder::new(WireProtocol::Chat, "test");
        assert!(!chat.decode("", "[DONE]").unwrap().is_empty());
        assert!(chat.finished);
    }

    #[test]
    fn responses_terminal_requires_matching_identity_status_and_event() {
        for (event, terminal) in [
            (
                "response.completed",
                json!({"object":"response","status":"completed","error":null}),
            ),
            (
                "response.completed",
                json!({"object":"response","id":"resp_test","status":"in_progress","error":null}),
            ),
            (
                "response.incomplete",
                json!({"object":"response","id":"resp_test","status":"completed","error":null}),
            ),
            (
                "response.completed",
                json!({"object":"message","id":"resp_test","status":"completed","error":null}),
            ),
        ] {
            let mut decoder = ChatDecoder::new(WireProtocol::Responses, "test");
            let frame = json!({"type":"response.completed","response":terminal});
            assert!(decoder.decode(event, &frame.to_string()).is_err());
            assert!(!decoder.finished);
        }
    }

    #[test]
    fn upstream_failure_is_not_a_successful_completion() {
        let mut decoder = ChatDecoder::new(WireProtocol::Responses, "test");
        assert_eq!(
            decoder
                .decode(
                    "",
                    &json!({"type":"response.failed","response":{"error":{"message":"quota"}}})
                        .to_string()
                )
                .unwrap_err(),
            "quota"
        );
        assert!(!decoder.finished);
    }
}
