use super::{Model, ProxyError};
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const CONTENT_LIMIT: usize = 16 * 1024 * 1024;

pub(super) struct Built {
    pub(super) body: Value,
    pub(super) names: BTreeMap<String, String>,
    pub(super) allowed: BTreeSet<String>,
    pub(super) thinking: bool,
}

pub(super) fn unsupported(message: impl Into<String>) -> ProxyError {
    let mut error = ProxyError::new(StatusCode::BAD_REQUEST, message);
    error.error_code = Some("unsupported_parameter".into());
    error.model_scope = true;
    error
}

pub(super) fn validate(body: &Value) -> Result<(), ProxyError> {
    super::super::protocol::reject_structured_output(body)?;
    let object = body
        .as_object()
        .ok_or_else(|| unsupported("The Chat request must be an object"))?;
    for (key, value) in object {
        if value.is_null() {
            continue;
        }
        match key.as_str() {
            "model" | "messages" | "stream" | "tools" => (),
            "reasoning_effort" if value.is_string() => (),
            "reasoning"
                if value.as_object().is_some_and(|options| {
                    options.iter().all(|(key, value)| {
                        value.is_null()
                            || match key.as_str() {
                                "effort" => value.is_string(),
                                "summary" => value == "auto",
                                _ => false,
                            }
                    })
                }) => {}
            // Native Kiro controls its output budget. These are client hints, not hard caps.
            "max_tokens" | "max_completion_tokens"
                if value.as_u64().is_some_and(|value| value > 0) => {}
            "temperature" | "top_p" if value.as_f64() == Some(1.0) => (),
            "presence_penalty" | "frequency_penalty" if value.as_f64() == Some(0.0) => (),
            "n" if value == 1 => (),
            "logprobs" if value == false => (),
            "parallel_tool_calls" if value == true => (),
            "tool_choice" if matches!(value.as_str(), Some("auto" | "none")) => (),
            "response_format" if value["type"] == "text" => (),
            "stream_options"
                if value.as_object().is_some_and(|options| {
                    options.keys().all(|key| key == "include_usage")
                        && options.get("include_usage").is_none_or(Value::is_boolean)
                }) => {}
            "stop" if value.as_array().is_some_and(Vec::is_empty) => (),
            key if key.starts_with("_echobird_") => (),
            _ => {
                return Err(unsupported(format!(
                    "Kiro cannot preserve Chat parameter {key}"
                )))
            }
        }
    }
    if body["messages"]
        .as_array()
        .is_none_or(|messages| messages.is_empty())
    {
        return Err(unsupported("Chat messages must be a nonempty array"));
    }
    if body
        .get("stream")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(unsupported("Invalid Chat stream option"));
    }
    if body
        .get("tools")
        .is_some_and(|value| !value.is_null() && !value.is_array())
    {
        return Err(unsupported("Chat tools must be an array"));
    }
    if body["max_tokens"]
        .as_u64()
        .zip(body["max_completion_tokens"].as_u64())
        .is_some_and(|(a, b)| a != b)
    {
        return Err(unsupported("Conflicting Chat output budget hints"));
    }
    Ok(())
}

fn tool_name(name: &str) -> String {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return name.into();
    }
    let hash = format!("{:x}", Sha256::digest(name));
    let prefix: String = name
        .chars()
        .take(51)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}_{}", &hash[..12])
}

fn tool_id(id: &str) -> String {
    if !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return id.into();
    }
    format!("t_{:x}", Sha256::digest(id))[..34].to_string()
}

fn register_name(names: &mut BTreeMap<String, String>, name: &str) -> Result<String, ProxyError> {
    if name.trim().is_empty() {
        return Err(unsupported("Function tools must have a name"));
    }
    let native = tool_name(name);
    if names.get(&native).is_some_and(|previous| previous != name) {
        return Err(unsupported("Kiro tool names collide after normalization"));
    }
    names.insert(native.clone(), name.into());
    Ok(native)
}

#[derive(Default)]
struct Content {
    text: Vec<String>,
    images: Vec<Value>,
}

fn content(value: &Value, images: bool, tool: bool) -> Result<Content, ProxyError> {
    let mut output = Content::default();
    if value.is_null() {
        return Ok(output);
    }
    if let Some(text) = value.as_str() {
        output.text.push(text.into());
        return Ok(output);
    }
    let Some(parts) = value.as_array() else {
        if tool && value.is_object() {
            output.text.push(value.to_string());
            return Ok(output);
        }
        return Err(unsupported("Kiro messages require text or content blocks"));
    };
    for part in parts {
        match part["type"].as_str() {
            Some("text" | "input_text" | "output_text") => output.text.push(
                part["text"]
                    .as_str()
                    .ok_or_else(|| unsupported("Invalid text content block"))?
                    .into(),
            ),
            Some("image_url" | "input_image" | "image") => {
                if !images {
                    return Err(unsupported(
                        "This Kiro model or message does not accept images",
                    ));
                }
                let (media, data) = if part["type"] == "image" {
                    if part["source"]["type"] != "base64" {
                        return Err(unsupported("Kiro images require inline base64 data"));
                    }
                    (
                        part["source"]["media_type"].as_str().unwrap_or_default(),
                        part["source"]["data"].as_str().unwrap_or_default(),
                    )
                } else {
                    let image = if part["type"] == "input_image" {
                        &part["image_url"]
                    } else {
                        &part["image_url"]["url"]
                    };
                    let detail = if part["type"] == "input_image" {
                        &part["detail"]
                    } else {
                        &part["image_url"]["detail"]
                    };
                    if !detail.is_null() && detail != "auto" {
                        return Err(unsupported("Kiro cannot select an image detail level"));
                    }
                    let (media, data) = image
                        .as_str()
                        .and_then(|image| image.strip_prefix("data:"))
                        .and_then(|image| image.split_once(";base64,"))
                        .ok_or_else(|| unsupported("Kiro images require inline base64 data"))?;
                    (media, data)
                };
                let format = match media.to_ascii_lowercase().as_str() {
                    "image/png" => "png",
                    "image/jpeg" | "image/jpg" => "jpeg",
                    "image/gif" => "gif",
                    "image/webp" => "webp",
                    _ => return Err(unsupported("Unsupported Kiro image format")),
                };
                if data.is_empty()
                    || data.len() > CONTENT_LIMIT
                    || STANDARD.decode(data).map_or(true, |bytes| bytes.is_empty())
                {
                    return Err(unsupported("Invalid Kiro image data"));
                }
                output
                    .images
                    .push(json!({"format":format,"source":{"bytes":data}}));
            }
            _ => return Err(unsupported("Kiro cannot preserve this content block")),
        }
    }
    Ok(output)
}

fn append_text(target: &mut String, text: &str) {
    if !target.is_empty() && !text.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(text);
}

#[derive(Default)]
struct Turn {
    assistant: bool,
    text: String,
    reasoning: String,
    images: Vec<Value>,
    calls: Vec<Value>,
    results: Vec<Value>,
}

fn thinking(body: &Value, model: &str) -> Result<Option<u32>, ProxyError> {
    if body["reasoning_effort"]
        .as_str()
        .zip(body["reasoning"]["effort"].as_str())
        .is_some_and(|(a, b)| a != b)
    {
        return Err(unsupported("Conflicting Kiro reasoning hints"));
    }
    let effort = body["reasoning_effort"]
        .as_str()
        .or_else(|| body["reasoning"]["effort"].as_str())
        .unwrap_or_default();
    if effort.is_empty() || effort == "none" {
        return Ok(None);
    }
    if model != "auto" && !model.to_ascii_lowercase().contains("claude") {
        return Err(unsupported(
            "This Kiro model cannot use the requested reasoning hint",
        ));
    }
    Ok(Some(match effort {
        "low" => 10000,
        "medium" => 20000,
        "high" => 30000,
        "xhigh" | "max" => 50000,
        _ => return Err(unsupported("Unsupported Kiro reasoning effort")),
    }))
}

pub(super) fn build(
    body: &Value,
    model: &Model,
    profile: Option<&str>,
) -> Result<Built, ProxyError> {
    validate(body)?;
    let mut names = BTreeMap::new();
    let mut definitions = BTreeMap::new();
    let mut allowed = BTreeSet::new();
    let none = body["tool_choice"] == "none";
    for tool in body["tools"].as_array().into_iter().flatten() {
        if none {
            continue;
        }
        if tool["type"] != "function" || !tool["function"].is_object() {
            return Err(unsupported("Kiro supports function tools only"));
        }
        let function = &tool["function"];
        if function["strict"] == true {
            return Err(unsupported(
                "Kiro cannot guarantee strict function arguments",
            ));
        }
        let original = function["name"]
            .as_str()
            .ok_or_else(|| unsupported("Function tools must have a name"))?;
        let name = register_name(&mut names, original)?;
        let schema = function
            .get("parameters")
            .filter(|value| !value.is_null())
            .cloned()
            .unwrap_or_else(|| json!({"type":"object","properties":{}}));
        if !schema.is_object() {
            return Err(unsupported("Invalid function tool schema"));
        }
        if definitions.insert(name.clone(), json!({"toolSpecification":{"name":name,"description":function["description"].as_str().filter(|value| !value.is_empty()).unwrap_or(original),"inputSchema":{"json":schema}}})).is_some() { return Err(unsupported("Duplicate function tool name")); }
        allowed.insert(name);
    }
    let mut system = String::new();
    let mut turns: Vec<Turn> = Vec::new();
    let mut seen_calls = BTreeSet::new();
    for message in body["messages"].as_array().expect("validated messages") {
        let role = message["role"]
            .as_str()
            .ok_or_else(|| unsupported("Chat messages require a role"))?;
        if !message["name"].is_null()
            || !message["audio"].is_null()
            || !message["function_call"].is_null()
            || !message["refusal"].is_null()
        {
            return Err(unsupported("Kiro cannot preserve this Chat message"));
        }
        let contents = content(
            &message["content"],
            model.images && matches!(role, "user" | "tool"),
            role == "tool",
        )?;
        let text = contents.text.join("\n\n");
        if matches!(role, "system" | "developer") {
            append_text(&mut system, &text);
            continue;
        }
        if !matches!(role, "user" | "assistant" | "tool") {
            return Err(unsupported("Unsupported Chat message role"));
        }
        let assistant = role == "assistant";
        if turns.last().is_none_or(|turn| turn.assistant != assistant) {
            turns.push(Turn {
                assistant,
                ..Turn::default()
            });
        }
        let turn = turns.last_mut().expect("created turn");
        if role == "tool" {
            let original = message["tool_call_id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| unsupported("Tool results require a call ID"))?;
            turn.results.push(json!({"toolUseId":tool_id(original),"status":if message["is_error"] == true { "error" } else { "success" },"content":[{"text":if text.is_empty(){"(no output)"}else{&text}}]}));
        } else {
            append_text(&mut turn.text, &text);
        }
        turn.images.extend(contents.images);
        if let Some(reasoning) = message["reasoning_content"].as_str() {
            append_text(&mut turn.reasoning, reasoning);
        } else if !message["reasoning_content"].is_null() {
            return Err(unsupported("Invalid Chat reasoning content"));
        }
        if message
            .get("tool_calls")
            .is_some_and(|value| !value.is_null() && (!assistant || !value.is_array()))
        {
            return Err(unsupported("Invalid assistant tool calls"));
        }
        for call in message["tool_calls"].as_array().into_iter().flatten() {
            if call["type"] != "function" {
                return Err(unsupported("Kiro supports function calls only"));
            }
            let id = call["id"]
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| unsupported("Function calls require an ID"))?;
            let id = tool_id(id);
            if !seen_calls.insert(id.clone()) {
                return Err(unsupported("Duplicate function call ID"));
            }
            let name = register_name(
                &mut names,
                call["function"]["name"]
                    .as_str()
                    .ok_or_else(|| unsupported("Function calls require a name"))?,
            )?;
            let arguments = call["function"]["arguments"]
                .as_str()
                .ok_or_else(|| unsupported("Function arguments must be JSON text"))?;
            let input: Value = serde_json::from_str(arguments)
                .map_err(|_| unsupported("Invalid function arguments JSON"))?;
            if !input.is_object() {
                return Err(unsupported("Function arguments must be a JSON object"));
            }
            definitions.entry(name.clone()).or_insert_with(|| json!({"toolSpecification":{"name":name,"description":"Tool","inputSchema":{"json":{"type":"object","properties":{}}}}}));
            turn.calls
                .push(json!({"toolUseId":id,"name":name,"input":input}));
        }
    }
    if turns.is_empty() || turns[0].assistant {
        return Err(unsupported(
            "Kiro conversations must begin with a user message",
        ));
    }
    if turns.last().is_some_and(|turn| turn.assistant) {
        turns.push(Turn::default());
    }
    for index in 0..turns.len() {
        if turns[index].assistant {
            continue;
        }
        let previous: Vec<_> = if index > 0 {
            turns[index - 1]
                .calls
                .iter()
                .map(|call| {
                    call["toolUseId"]
                        .as_str()
                        .expect("constructed call")
                        .to_owned()
                })
                .collect()
        } else {
            vec![]
        };
        let mut answered = BTreeSet::new();
        for result in &turns[index].results {
            let id = result["toolUseId"].as_str().expect("constructed result");
            if !previous.iter().any(|call| call == id) || !answered.insert(id.to_string()) {
                return Err(unsupported(
                    "Tool results must answer the preceding assistant calls exactly once",
                ));
            }
        }
        for id in previous.into_iter().filter(|id| !answered.contains(id)) {
            turns[index].results.push(json!({"toolUseId":id,"status":"error","content":[{"text":"Tool use was interrupted and did not produce a result."}]}));
        }
        if turns[index].text.is_empty() && turns[index].results.is_empty() {
            turns[index].text = "Please proceed with the task.".into();
        }
    }
    let thinking = thinking(body, &model.id)?;
    if let Some(budget) = thinking {
        system = format!("<thinking_mode>enabled</thinking_mode><max_thinking_length>{budget}</max_thinking_length>\n{system}");
    }
    if !system.is_empty() {
        turns[0].text = format!("{system}\n\n{}", turns[0].text);
    }
    let last = turns.len() - 1;
    let mut history = Vec::new();
    let historical_tools: Vec<Vec<String>> = turns
        .iter()
        .enumerate()
        .map(|(index, turn)| {
            if !turn.assistant && index + 1 < turns.len() {
                turns[index + 1]
                    .calls
                    .iter()
                    .filter_map(|call| call["name"].as_str().map(str::to_owned))
                    .collect()
            } else {
                vec![]
            }
        })
        .collect();
    for (index, turn) in turns.into_iter().enumerate() {
        if turn.assistant {
            let mut assistant = json!({"content":turn.text});
            if !turn.calls.is_empty() {
                assistant["toolUses"] = json!(turn.calls);
            }
            if !turn.reasoning.is_empty() {
                assistant["reasoningContent"] = json!({"reasoningText":{"text":turn.reasoning}});
            }
            history.push(json!({"assistantResponseMessage":assistant}));
        } else {
            let mut user = json!({"content":turn.text,"modelId":model.id,"origin":"KIRO_CLI"});
            if !turn.images.is_empty() {
                user["images"] = json!(turn.images);
            }
            let mut context = json!({});
            if !turn.results.is_empty() {
                context["toolResults"] = json!(turn.results);
            }
            if index == last && !allowed.is_empty() {
                context["tools"] = json!(allowed
                    .iter()
                    .filter_map(|name| definitions.get(name))
                    .collect::<Vec<_>>());
            } else if !historical_tools[index].is_empty() {
                let names: BTreeSet<_> = historical_tools[index].iter().collect();
                context["tools"] = json!(names
                    .into_iter()
                    .filter_map(|name| definitions.get(name))
                    .collect::<Vec<_>>());
            }
            if context.as_object().is_some_and(|object| !object.is_empty()) {
                user["userInputMessageContext"] = context;
            }
            history.push(json!({"userInputMessage":user}));
        }
    }
    let current = history.pop().expect("validated user turn");
    let mut state = json!({"chatTriggerType":"MANUAL","agentTaskType":"vibe","conversationId":uuid::Uuid::new_v4().to_string(),"currentMessage":current});
    if !history.is_empty() {
        state["history"] = json!(history);
    }
    let mut output = json!({"conversationState":state,"agentMode":"vibe"});
    if let Some(profile) = profile.filter(|profile| !profile.is_empty()) {
        output["profileArn"] = json!(profile);
    }
    if output.to_string().len() > CONTENT_LIMIT {
        return Err(unsupported("Kiro conversation is too large"));
    }
    Ok(Built {
        body: output,
        names,
        allowed,
        thinking: thinking.is_some(),
    })
}
