use std::collections::{BTreeMap, HashSet};

use bytes::Bytes;
use serde_json::{json, Value};

pub(super) type ToolNamespaces = BTreeMap<String, Option<String>>;

fn functions(request: &Value) -> Result<Vec<(Value, Option<String>)>, String> {
    let mut result = Vec::new();
    let mut names = HashSet::new();
    for tool in request["tools"].as_array().into_iter().flatten() {
        let (children, namespace) = if tool["type"] == "namespace" {
            (
                tool["tools"]
                    .as_array()
                    .ok_or("Invalid tool namespace")?
                    .clone(),
                Some(
                    tool["name"]
                        .as_str()
                        .ok_or("Missing namespace name")?
                        .to_string(),
                ),
            )
        } else {
            (vec![tool.clone()], None)
        };
        for child in children {
            if child["type"] != "function" {
                return Err("This endpoint supports function tools only".into());
            }
            let name = child["name"].as_str().ok_or("Missing function name")?;
            if !names.insert(name.to_string()) {
                return Err(format!("Duplicate function name: {name}"));
            }
            result.push((child, namespace.clone()));
        }
    }
    Ok(result)
}

pub(super) fn tool_namespaces(request: &Value) -> Result<ToolNamespaces, String> {
    Ok(functions(request)?
        .into_iter()
        .map(|(tool, namespace)| (tool["name"].as_str().unwrap().to_string(), namespace))
        .collect())
}

fn message_content(content: &Value) -> Result<Value, String> {
    if content.is_string() {
        return Ok(content.clone());
    }
    let mut parts = Vec::new();
    for part in content.as_array().ok_or("Invalid message content")? {
        match part["type"].as_str() {
            Some("input_text" | "output_text" | "text") => {
                parts.push(json!({"type":"text", "text":part["text"]}));
            }
            Some("input_image") => {
                let image = part["image_url"]
                    .as_str()
                    .ok_or("Only image URLs are supported")?;
                let mut image_url = json!({"url":image});
                if let Some(detail) = part.get("detail") {
                    image_url["detail"] = detail.clone();
                }
                parts.push(json!({"type":"image_url", "image_url":image_url}));
            }
            _ => return Err("Unsupported message content type".into()),
        }
    }
    Ok(Value::Array(parts))
}

pub(super) fn responses_to_chat(request: &Value) -> Result<Value, String> {
    if !request["previous_response_id"].is_null() {
        return Err("This endpoint requires the complete conversation in input".into());
    }
    let namespaces = tool_namespaces(request)?;
    let unqualified = |name: &str| {
        namespaces
            .iter()
            .find_map(|(function, namespace)| {
                (name == format!("{}.{function}", namespace.as_deref()?))
                    .then_some(function.as_str())
            })
            .unwrap_or(name)
            .to_string()
    };
    let mut messages = Vec::new();
    if let Some(instructions) = request["instructions"].as_str().filter(|s| !s.is_empty()) {
        messages.push(json!({"role":"system", "content":instructions}));
    }
    if let Some(text) = request["input"].as_str() {
        messages.push(json!({"role":"user", "content":text}));
    } else {
        for item in request["input"]
            .as_array()
            .ok_or("Missing conversation input")?
        {
            match item["type"].as_str().unwrap_or("message") {
                "message" => {
                    let role = item["role"].as_str().ok_or("Missing message role")?;
                    messages.push(json!({"role":if role == "developer" {"system"} else {role}, "content":message_content(&item["content"])?}));
                }
                "function_call" => {
                    let name = item["name"]
                        .as_str()
                        .ok_or("Missing historical function name")?;
                    let call = json!({"id":item["call_id"],"type":"function","function":{"name":unqualified(name),"arguments":item["arguments"]}});
                    if messages.last().is_some_and(|m| m["role"] == "assistant") {
                        let last = messages.last_mut().unwrap();
                        if last.get("tool_calls").is_none() {
                            last["tool_calls"] = json!([]);
                        }
                        last["tool_calls"].as_array_mut().unwrap().push(call);
                    } else {
                        messages
                            .push(json!({"role":"assistant", "content":null,"tool_calls":[call]}));
                    }
                }
                "function_call_output" => {
                    let output = &item["output"];
                    let content = if output.is_string() {
                        output.clone()
                    } else {
                        json!(output.to_string())
                    };
                    messages.push(
                        json!({"role":"tool", "tool_call_id":item["call_id"], "content":content}),
                    );
                }
                "reasoning" => {}
                _ => return Err("Unsupported conversation item type".into()),
            }
        }
    }
    let mut result = json!({"model":request["model"], "messages":messages,"stream":request["stream"].as_bool().unwrap_or(false)});
    let tools: Vec<Value> = functions(request)?
        .into_iter()
        .map(|(tool, _)| {
            let mut function = json!({"name":tool["name"],"parameters":tool.get("parameters").cloned().unwrap_or(json!({"type":"object","properties":{}}))});
            for field in ["description", "strict"] {
                if let Some(value) = tool.get(field) {
                    function[field] = value.clone();
                }
            }
            json!({"type":"function","function":function})
        })
        .collect();
    if !tools.is_empty() {
        result["tools"] = json!(tools);
    }
    if let Some(choice) = request.get("tool_choice") {
        result["tool_choice"] = if choice.is_object() && choice["type"] == "function" {
            json!({"type":"function","function":{"name":unqualified(choice["name"].as_str().ok_or("Missing selected function name")?)}})
        } else {
            choice.clone()
        };
    }
    for field in ["temperature", "top_p", "parallel_tool_calls"] {
        if let Some(value) = request.get(field).filter(|v| !v.is_null()) {
            result[field] = value.clone();
        }
    }
    if let Some(limit) = request.get("max_output_tokens").filter(|v| !v.is_null()) {
        result["max_tokens"] = limit.clone();
    }
    if result["stream"] == true {
        result["stream_options"] = json!({"include_usage":true});
    }
    if let Some(format) = request.pointer("/text/format") {
        if format["type"] == "json_schema" {
            result["response_format"] = json!({"type":"json_schema","json_schema":{"name":format["name"],"schema":format["schema"],"strict":format["strict"].as_bool().unwrap_or(false)}});
        } else if format["type"] == "json_object" {
            result["response_format"] = json!({"type":"json_object"});
        }
    }
    Ok(result)
}

fn response(id: &str, model: &str, status: &str, output: &[Value], usage: &Value) -> Value {
    let input = usage["prompt_tokens"].as_u64().unwrap_or(0);
    let completion = usage["completion_tokens"].as_u64().unwrap_or(0);
    json!({
        "id":id,"object":"response","created_at":chrono::Utc::now().timestamp(),
        "model":model,"status":status,"output":output,"error":null,
        "incomplete_details":if status == "incomplete" {json!({"reason":"max_output_tokens"})} else {Value::Null},
        "usage":{"input_tokens":input,"output_tokens":completion,"total_tokens":input+completion,
        "input_tokens_details":{"cached_tokens":usage.pointer("/prompt_tokens_details/cached_tokens").and_then(Value::as_u64).unwrap_or(0)},
        "output_tokens_details":{"reasoning_tokens":usage.pointer("/completion_tokens_details/reasoning_tokens").and_then(Value::as_u64).unwrap_or(0)}}
    })
}

fn id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

fn function_item(call: &Value, namespaces: &ToolNamespaces) -> Value {
    let mut item = json!({"type":"function_call","id":id("fc"),"status":"completed",
        "call_id":call["id"],"name":call["function"]["name"],"arguments":call["function"]["arguments"]});
    if let Some(namespace) = call["function"]["name"]
        .as_str()
        .and_then(|n| namespaces.get(n))
        .and_then(Option::as_deref)
    {
        item["namespace"] = json!(namespace);
    }
    item
}

fn validate_output(output: &[Value], finish: &str) -> Result<(), String> {
    if !matches!(finish, "stop" | "tool_calls" | "length") {
        return Err(format!("Unsupported upstream finish reason: {finish}"));
    }
    for item in output.iter().filter(|item| item["type"] == "function_call") {
        if finish == "length" {
            return Err("Upstream truncated a tool call".into());
        }
        for field in ["call_id", "name"] {
            if item[field].as_str().is_none_or(str::is_empty) {
                return Err(format!("Missing upstream tool {field}"));
            }
        }
        let arguments: Value = serde_json::from_str(
            item["arguments"]
                .as_str()
                .ok_or("Missing upstream tool arguments")?,
        )
        .map_err(|_| "Invalid upstream tool arguments")?;
        if !arguments.is_object() {
            return Err("Upstream tool arguments must be a JSON object".into());
        }
    }
    Ok(())
}

pub(super) fn chat_to_response(
    data: &Value,
    model: &str,
    namespaces: &ToolNamespaces,
) -> Result<Value, String> {
    let choice = data["choices"]
        .as_array()
        .and_then(|c| c.first())
        .ok_or("Missing upstream choice")?;
    let finish = choice["finish_reason"]
        .as_str()
        .ok_or("Missing upstream finish reason")?;
    if finish == "content_filter" {
        return Err("The upstream filtered this response".into());
    }
    let message = &choice["message"];
    let mut output = Vec::new();
    if let Some(content) = message["content"].as_str().filter(|s| !s.is_empty()) {
        output.push(
            json!({"id":id("msg"),"type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":content,"annotations":[]}]}),
        );
    }
    for call in message["tool_calls"].as_array().into_iter().flatten() {
        output.push(function_item(call, namespaces));
    }
    validate_output(&output, finish)?;
    Ok(response(
        &id("resp"),
        model,
        if finish == "length" {
            "incomplete"
        } else {
            "completed"
        },
        &output,
        &data["usage"],
    ))
}

pub(super) struct ChatSseAdapter {
    id: String,
    model: String,
    namespaces: ToolNamespaces,
    buffer: Vec<u8>,
    data_lines: Vec<String>,
    output: Vec<Value>,
    text_index: Option<usize>,
    tool_indices: BTreeMap<u64, usize>,
    pending_tools: BTreeMap<u64, Value>,
    usage: Value,
    finish_reason: Option<String>,
    sequence: u64,
    started: bool,
    done: bool,
}

impl ChatSseAdapter {
    pub(super) fn new(model: &str, namespaces: ToolNamespaces) -> Self {
        Self {
            id: id("resp"),
            model: model.into(),
            namespaces,
            buffer: Vec::new(),
            data_lines: Vec::new(),
            output: Vec::new(),
            text_index: None,
            tool_indices: BTreeMap::new(),
            pending_tools: BTreeMap::new(),
            usage: Value::Null,
            finish_reason: None,
            sequence: 0,
            started: false,
            done: false,
        }
    }

    fn event(&mut self, kind: &str, mut data: Value) -> Bytes {
        data["type"] = json!(kind);
        data["sequence_number"] = json!(self.sequence);
        self.sequence += 1;
        Bytes::from(format!("event: {kind}\ndata: {data}\n\n"))
    }

    fn start(&mut self, events: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        let root = response(&self.id, &self.model, "in_progress", &[], &Value::Null);
        events.push(self.event("response.created", json!({"response":root})));
    }

    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Bytes>, String> {
        if self.done {
            return Ok(Vec::new());
        }
        self.buffer.extend_from_slice(chunk);
        if self.buffer.len() > 16 * 1024 * 1024 {
            return Err("Upstream SSE event is too large".into());
        }
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=end).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8(line).map_err(|_| "Invalid upstream SSE encoding")?;
            if line.is_empty() {
                if !self.data_lines.is_empty() {
                    let data = std::mem::take(&mut self.data_lines).join("\n");
                    self.consume(&data, &mut events)?;
                }
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data_lines
                    .push(data.strip_prefix(' ').unwrap_or(data).to_string());
            }
        }
        Ok(events)
    }

    fn consume(&mut self, data: &str, events: &mut Vec<Bytes>) -> Result<(), String> {
        if self.done {
            return Ok(());
        }
        self.start(events);
        if data == "[DONE]" {
            if self.finish_reason.is_none() {
                return Err("Upstream stream ended without a finish reason".into());
            }
            return self.complete(events);
        }
        let value: Value =
            serde_json::from_str(data).map_err(|e| format!("Invalid upstream SSE JSON: {e}"))?;
        if value.get("error").is_some() {
            return Err("Upstream returned a stream error".into());
        }
        if !value["usage"].is_null() {
            self.usage = value["usage"].clone();
        }
        for choice in value["choices"].as_array().into_iter().flatten() {
            if choice["index"].as_u64().unwrap_or(0) != 0 {
                continue;
            }
            if let Some(finish) = choice["finish_reason"].as_str() {
                self.finish_reason = Some(finish.into());
            }
            let delta = &choice["delta"];
            if let Some(text) = delta["content"].as_str().filter(|s| !s.is_empty()) {
                let index = if let Some(index) = self.text_index {
                    index
                } else {
                    let index = self.output.len();
                    let item = json!({"id":id("msg"),"type":"message","role":"assistant","status":"in_progress","content":[]});
                    events.push(self.event(
                        "response.output_item.added",
                        json!({"output_index":index,"item":item}),
                    ));
                    self.output.push(item);
                    self.output[index]["content"] =
                        json!([{"type":"output_text","text":"","annotations":[]}]);
                    let item_id = self.output[index]["id"].clone();
                    events.push(self.event("response.content_part.added", json!({"output_index":index,"item_id":item_id,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}})));
                    self.text_index = Some(index);
                    index
                };
                let item_id = self.output[index]["id"].clone();
                let previous = self.output[index]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_string();
                self.output[index]["content"][0]["text"] = json!(format!("{previous}{text}"));
                events.push(self.event(
                    "response.output_text.delta",
                    json!({"output_index":index,"item_id":item_id,"content_index":0,"delta":text}),
                ));
            }
            for tool in delta["tool_calls"].as_array().into_iter().flatten() {
                self.tool_delta(tool, events)?;
            }
        }
        Ok(())
    }

    fn tool_delta(&mut self, delta: &Value, events: &mut Vec<Bytes>) -> Result<(), String> {
        let upstream_index = delta["index"].as_u64().ok_or("Missing tool-call index")?;
        let index = if let Some(index) = self.tool_indices.get(&upstream_index) {
            *index
        } else {
            let pending = self
                .pending_tools
                .entry(upstream_index)
                .or_insert_with(|| json!({"id":"","function":{"name":"","arguments":""}}));
            if let Some(call_id) = delta["id"].as_str() {
                pending["id"] = json!(call_id);
            }
            for field in ["name", "arguments"] {
                if let Some(fragment) = delta["function"][field].as_str() {
                    let previous = pending["function"][field].as_str().unwrap();
                    pending["function"][field] = json!(format!("{previous}{fragment}"));
                }
            }
            if pending["id"].as_str().unwrap().is_empty()
                || pending["function"]["name"].as_str().unwrap().is_empty()
            {
                return Ok(());
            }
            let name = pending["function"]["name"].as_str().unwrap();
            if self.namespaces.is_empty()
                || !self.namespaces.contains_key(name)
                || self
                    .namespaces
                    .keys()
                    .any(|other| other != name && other.starts_with(name))
            {
                return Ok(());
            }
            self.publish_pending_tool(upstream_index, events);
            return Ok(());
        };
        if delta["function"]["name"]
            .as_str()
            .is_some_and(|name| !name.is_empty())
        {
            return Err("Upstream changed a tool name after starting its arguments".into());
        }
        if delta["id"]
            .as_str()
            .is_some_and(|call_id| Some(call_id) != self.output[index]["call_id"].as_str())
        {
            return Err("Upstream changed a tool-call id".into());
        }
        if let Some(arguments) = delta["function"]["arguments"].as_str() {
            self.append_arguments(index, arguments, events);
        }
        Ok(())
    }

    fn publish_pending_tool(&mut self, upstream_index: u64, events: &mut Vec<Bytes>) {
        let pending = self.pending_tools.remove(&upstream_index).unwrap();
        let arguments = pending["function"]["arguments"]
            .as_str()
            .unwrap()
            .to_string();
        let mut item = function_item(&pending, &self.namespaces);
        item["status"] = json!("in_progress");
        item["arguments"] = json!("");
        let index = self.output.len();
        events.push(self.event(
            "response.output_item.added",
            json!({"output_index":index,"item":item}),
        ));
        self.output.push(item);
        self.tool_indices.insert(upstream_index, index);
        if !arguments.is_empty() {
            self.append_arguments(index, &arguments, events);
        }
    }

    fn append_arguments(&mut self, index: usize, fragment: &str, events: &mut Vec<Bytes>) {
        let previous = self.output[index]["arguments"].as_str().unwrap();
        self.output[index]["arguments"] = json!(format!("{previous}{fragment}"));
        let item_id = self.output[index]["id"].clone();
        events.push(self.event(
            "response.function_call_arguments.delta",
            json!({"item_id":item_id,"output_index":index,"delta":fragment}),
        ));
    }

    fn complete(&mut self, events: &mut Vec<Bytes>) -> Result<(), String> {
        for upstream_index in self.pending_tools.keys().copied().collect::<Vec<_>>() {
            let pending = &self.pending_tools[&upstream_index];
            let name = pending["function"]["name"].as_str().unwrap();
            if pending["id"].as_str().unwrap().is_empty()
                || name.is_empty()
                || (!self.namespaces.is_empty() && !self.namespaces.contains_key(name))
            {
                return Err("Incomplete upstream tool call".into());
            }
            self.publish_pending_tool(upstream_index, events);
        }
        if self.finish_reason.as_deref() == Some("content_filter") {
            return Err("The upstream filtered this response".into());
        }
        validate_output(&self.output, self.finish_reason.as_deref().unwrap_or(""))?;
        for index in 0..self.output.len() {
            self.output[index]["status"] = json!("completed");
            let item = self.output[index].clone();
            if item["type"] == "message" {
                events.push(self.event("response.output_text.done", json!({"item_id":item["id"],"output_index":index,"content_index":0,"text":item["content"][0]["text"]})));
                events.push(self.event("response.content_part.done", json!({"item_id":item["id"],"output_index":index,"content_index":0,"part":item["content"][0]})));
            } else {
                events.push(self.event("response.function_call_arguments.done", json!({"item_id":item["id"],"output_index":index,"arguments":item["arguments"]})));
            }
            events.push(self.event(
                "response.output_item.done",
                json!({"output_index":index,"item":item}),
            ));
        }
        let status = if self.finish_reason.as_deref() == Some("length") {
            "incomplete"
        } else {
            "completed"
        };
        let root = response(&self.id, &self.model, status, &self.output, &self.usage);
        events.push(self.event(
            if status == "incomplete" {
                "response.incomplete"
            } else {
                "response.completed"
            },
            json!({"response":root}),
        ));
        self.done = true;
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<Vec<Bytes>, String> {
        if self.done {
            Ok(Vec::new())
        } else {
            Err("Upstream stream ended before [DONE]".into())
        }
    }

    pub(super) fn is_finished(&self) -> bool {
        self.done
    }

    pub(super) fn failed(&mut self, message: &str) -> Bytes {
        self.done = true;
        let mut root = response(&self.id, &self.model, "failed", &self.output, &self.usage);
        root["error"] = json!({"code":"upstream_error","message":message});
        self.event("response.failed", json!({"response":root}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(events: &[Bytes]) -> Vec<Value> {
        events
            .iter()
            .map(|e| {
                let text = std::str::from_utf8(e).unwrap();
                serde_json::from_str(text.lines().find_map(|s| s.strip_prefix("data: ")).unwrap())
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn namespace_tools_and_parallel_results_round_trip() {
        let request = json!({"model":"fixture","instructions":"system","stream":true,
            "tools":[{"type":"namespace","name":"files","tools":[{"type":"function","name":"read","parameters":{"type":"object"}}]}],
            "input":[{"role":"user","content":[{"type":"input_text","text":"read"}]},
                {"type":"function_call","call_id":"c1","name":"files.read","arguments":"{}"},
                {"type":"function_call","call_id":"c2","name":"read","namespace":"files","arguments":"{}"},
                {"type":"function_call_output","call_id":"c1","output":"one"},
                {"type":"function_call_output","call_id":"c2","output":"two"}]});
        let chat = responses_to_chat(&request).unwrap();
        assert_eq!(
            chat["messages"][2]["tool_calls"].as_array().unwrap().len(),
            2
        );
        assert_eq!(chat["messages"][3]["tool_call_id"], "c1");
        assert_eq!(
            chat["messages"][2]["tool_calls"][0]["function"]["name"],
            "read"
        );
        assert_eq!(chat["tools"][0]["function"]["name"], "read");
        let output = chat_to_response(&json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":"c3","function":{"name":"read","arguments":"{}"}}]}}]}), "fixture", &tool_namespaces(&request).unwrap()).unwrap();
        assert_eq!(output["output"][0]["namespace"], "files");
    }

    #[test]
    fn duplicate_names_and_stateful_requests_are_rejected() {
        let request = json!({"input":"x","tools":[{"type":"namespace","name":"a","tools":[{"type":"function","name":"same"}]},{"type":"function","name":"same"}]});
        assert!(responses_to_chat(&request)
            .unwrap_err()
            .contains("Duplicate"));
        assert!(responses_to_chat(&json!({"input":"x","previous_response_id":"old"})).is_err());
    }

    #[test]
    fn fragmented_unicode_sse_preserves_text_tools_usage_and_sequence() {
        let mut adapter = ChatSseAdapter::new(
            "fixture",
            BTreeMap::from([("read".into(), Some("files".into()))]),
        );
        let chunks = [
            json!({"choices":[{"index":0,"delta":{"content":"你好"}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call1","function":{"name":"read","arguments":"{\"path\":"}},{"index":1,"id":"call2","function":{"name":"read","arguments":"{}"}}]}}]}),
            json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"one\"}"}}]},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}),
        ];
        let wire = chunks
            .iter()
            .map(|c| format!("data: {c}\r\n\r\n"))
            .collect::<String>()
            + "data: [DONE]\n\n";
        let mut events = Vec::new();
        for byte in wire.as_bytes() {
            events.extend(adapter.push(&[*byte]).unwrap());
        }
        assert!(adapter.finish().unwrap().is_empty());
        let values = decoded(&events);
        for (index, event) in values.iter().enumerate() {
            assert_eq!(event["sequence_number"], index);
        }
        let root = &values.last().unwrap()["response"];
        assert_eq!(root["output"][0]["content"][0]["text"], "你好");
        assert_eq!(root["output"][1]["arguments"], "{\"path\":\"one\"}");
        assert_eq!(root["output"][1]["namespace"], "files");
        assert_eq!(root["output"][2]["call_id"], "call2");
        assert_eq!(root["usage"]["total_tokens"], 15);
    }

    #[test]
    fn truncated_and_error_streams_never_complete() {
        let mut adapter = ChatSseAdapter::new("fixture", BTreeMap::new());
        adapter
            .push(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .unwrap();
        assert!(adapter.finish().is_err());
        assert_eq!(
            decoded(&[adapter.failed("closed")])[0]["type"],
            "response.failed"
        );
        let mut adapter = ChatSseAdapter::new("fixture", BTreeMap::new());
        assert!(adapter.push(b"data: [DONE]\n\n").is_err());
    }

    #[test]
    fn invalid_and_truncated_tool_arguments_never_emit_done() {
        for finish in ["tool_calls", "length"] {
            let chunk = json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read","arguments":"{\"path\":"}}]},"finish_reason":finish}]});
            let mut adapter = ChatSseAdapter::new("fixture", BTreeMap::new());
            let events = adapter
                .push(format!("data: {chunk}\n\n").as_bytes())
                .unwrap();
            assert!(!decoded(&events)
                .iter()
                .any(|event| event["type"] == "response.function_call_arguments.done"));
            assert!(adapter.push(b"data: [DONE]\n\n").is_err());
            assert!(chat_to_response(&json!({"choices":[{"finish_reason":finish,"message":{"tool_calls":[{"id":"call","function":{"name":"read","arguments":"{\"path\":"}}]}}]}),"fixture",&BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn fragmented_namespace_function_name_is_resolved_before_emitting() {
        let mut adapter = ChatSseAdapter::new(
            "fixture",
            BTreeMap::from([("read_file".into(), Some("muse".into()))]),
        );
        let first = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"read_","arguments":""}}]}}]});
        let second = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"file","arguments":"{}"}}]},"finish_reason":"tool_calls"}]});
        let events = adapter
            .push(format!("data: {first}\n\n").as_bytes())
            .unwrap();
        assert!(!decoded(&events)
            .iter()
            .any(|event| event["type"] == "response.output_item.added"));
        adapter
            .push(format!("data: {second}\n\n").as_bytes())
            .unwrap();
        let events = decoded(&adapter.push(b"data: [DONE]\n\n").unwrap());
        assert_eq!(
            events.last().unwrap()["response"]["output"][0]["name"],
            "read_file"
        );
        assert_eq!(
            events.last().unwrap()["response"]["output"][0]["namespace"],
            "muse"
        );
    }

    #[test]
    fn tool_name_prefixes_wait_for_disambiguation_or_final_completion() {
        for namespaced in [true, false] {
            for longer_name in [true, false] {
                let tools = json!([
                    {"type":"function","name":"powershell"},
                    {"type":"function","name":"powershell_input"}
                ]);
                let request = json!({"tools":if namespaced {
                    json!([{"type":"namespace","name":"muse","tools":tools}])
                } else {
                    tools
                }});
                let mut adapter =
                    ChatSseAdapter::new("fixture", tool_namespaces(&request).unwrap());
                let first = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"powershell","arguments":"{\"input\":"}}]}}]});
                let events = adapter
                    .push(format!("data: {first}\n\n").as_bytes())
                    .unwrap();
                assert!(!decoded(&events)
                    .iter()
                    .any(|event| event["type"] == "response.output_item.added"));
                let second = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":if longer_name {"_input"} else {""},"arguments":"\"fixture\"}"}}]},"finish_reason":"tool_calls"}]});
                adapter
                    .push(format!("data: {second}\n\n").as_bytes())
                    .unwrap();
                let events = decoded(&adapter.push(b"data: [DONE]\n\n").unwrap());
                let root = &events.last().unwrap()["response"];
                assert_eq!(root["status"], "completed");
                assert_eq!(
                    root["output"][0]["name"],
                    if longer_name {
                        "powershell_input"
                    } else {
                        "powershell"
                    }
                );
                assert_eq!(root["output"][0]["arguments"], "{\"input\":\"fixture\"}");
                if namespaced {
                    assert_eq!(root["output"][0]["namespace"], "muse");
                } else {
                    assert!(root["output"][0].get("namespace").is_none());
                }
            }
        }
    }

    #[test]
    fn images_and_token_limits_are_forwarded_and_length_is_incomplete() {
        let chat = responses_to_chat(&json!({"model":"fixture","max_output_tokens":100,"input":[{"role":"user","content":[{"type":"input_image","image_url":"data:image/png;base64,AA=="}]}]})).unwrap();
        assert_eq!(chat["max_tokens"], 100);
        assert_eq!(
            chat["messages"][0]["content"][0]["image_url"]["url"],
            "data:image/png;base64,AA=="
        );
        let response = chat_to_response(
            &json!({"choices":[{"finish_reason":"length","message":{"content":"partial"}}]}),
            "fixture",
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(response["status"], "incomplete");
        assert!(chat_to_response(&json!({"choices":[]}), "fixture", &BTreeMap::new()).is_err());
    }
}
