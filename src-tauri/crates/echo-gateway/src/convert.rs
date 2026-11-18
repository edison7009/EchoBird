//! Request and response conversion between the Anthropic Messages and OpenAI Chat Completions shapes.
//! Pure functions on JSON values: no I/O, no app state.

/// Convert Anthropic Messages request → OpenAI Chat Completions request
fn anthropic_user_content_part(block: &serde_json::Value) -> Option<serde_json::Value> {
    match block.get("type").and_then(|value| value.as_str()) {
        Some("text") => block
            .get("text")
            .and_then(|value| value.as_str())
            .map(|text| serde_json::json!({"type": "text", "text": text})),
        Some("image") => {
            let source = block.get("source")?;
            let url = match source.get("type").and_then(|value| value.as_str()) {
                Some("base64") => {
                    let media_type = source.get("media_type")?.as_str()?;
                    let data = source.get("data")?.as_str()?;
                    format!("data:{media_type};base64,{data}")
                }
                Some("url") => source.get("url")?.as_str()?.to_string(),
                _ => return None,
            };
            Some(serde_json::json!({
                "type": "image_url",
                "image_url": {"url": url}
            }))
        }
        Some("document") => {
            let source = block.get("source")?;
            match source.get("type").and_then(|value| value.as_str()) {
                Some("base64") => {
                    let data = source.get("data")?.as_str()?;
                    let filename = block
                        .get("title")
                        .and_then(|value| value.as_str())
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or("document.pdf");
                    Some(serde_json::json!({
                        "type": "file",
                        "file": {"file_data": data, "filename": filename}
                    }))
                }
                Some("text") => source
                    .get("data")
                    .and_then(|value| value.as_str())
                    .map(|text| serde_json::json!({"type": "text", "text": text})),
                Some("url") => source
                    .get("url")
                    .and_then(|value| value.as_str())
                    .map(|url| {
                        serde_json::json!({
                            "type": "text",
                            "text": format!("Document URL: {url}")
                        })
                    }),
                Some("file") => {
                    source
                        .get("file_id")
                        .and_then(|value| value.as_str())
                        .map(|file_id| {
                            serde_json::json!({
                                "type": "text",
                                "text": format!("Anthropic file reference: {file_id}")
                            })
                        })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn openai_user_content(parts: Vec<serde_json::Value>) -> serde_json::Value {
    let text_parts: Option<Vec<&str>> = parts
        .iter()
        .map(|part| {
            if part.get("type").and_then(|value| value.as_str()) == Some("text") {
                part.get("text").and_then(|value| value.as_str())
            } else {
                None
            }
        })
        .collect();

    match text_parts {
        Some(text_parts) => serde_json::Value::String(text_parts.concat()),
        None => serde_json::Value::Array(parts),
    }
}

fn anthropic_tool_result_content(
    tool_result: &serde_json::Value,
) -> (String, Vec<serde_json::Value>) {
    let Some(content) = tool_result.get("content") else {
        return (String::new(), Vec::new());
    };
    if let Some(text) = content.as_str() {
        return (text.to_string(), Vec::new());
    }

    let mut text = String::new();
    let mut attachments = Vec::new();
    if let Some(blocks) = content.as_array() {
        for block in blocks {
            if block.get("type").and_then(|value| value.as_str()) == Some("text") {
                if let Some(part) = block.get("text").and_then(|value| value.as_str()) {
                    text.push_str(part);
                }
            } else if block.get("type").and_then(|value| value.as_str()) == Some("tool_reference") {
                if let Some(name) = block
                    .get("tool_name")
                    .and_then(|value| value.as_str())
                    .filter(|name| !name.is_empty())
                {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(&format!("Tool {name} is loaded and can be called now."));
                }
            } else if let Some(part) = anthropic_user_content_part(block) {
                attachments.push(part);
            }
        }
    }
    (text, attachments)
}

pub fn anthropic_to_openai(body: &serde_json::Value) -> serde_json::Value {
    anthropic_to_chat(body, false)
}

/// DeepSeek and MiMo Chat require prior reasoning to continue thinking with tools.
pub fn anthropic_to_reasoning_chat(body: &serde_json::Value) -> serde_json::Value {
    let mut request = anthropic_to_chat(body, true);
    let has_tools = request.get("tools").is_some();
    let forced_tool = request["tool_choice"] == "required" || request["tool_choice"].is_object();
    let messages = request["messages"]
        .as_array_mut()
        .expect("converted messages");
    // Tool history can come from a different upstream or an interrupted turn.
    // Preserve that history and use non-thinking mode if reasoning is unavailable.
    let missing_reasoning = has_tools
        && messages.iter().any(|message| {
            message["role"] == "assistant" && message.get("reasoning_content").is_none()
        });
    if body["thinking"]["type"] == "disabled" || forced_tool || missing_reasoning {
        messages.retain_mut(|message| {
            let Some(fields) = message.as_object_mut() else {
                return true;
            };
            fields.remove("reasoning_content");
            !fields.get("role").is_some_and(|role| role == "assistant")
                || fields.contains_key("tool_calls")
                || fields.get("content").and_then(serde_json::Value::as_str) != Some("")
        });
        request["thinking"] = serde_json::json!({"type": "disabled"});
    } else if matches!(
        body["thinking"]["type"].as_str(),
        Some("enabled" | "adaptive")
    ) {
        request["thinking"] = serde_json::json!({"type": "enabled"});
    }
    request
}

fn anthropic_to_chat(body: &serde_json::Value, replay_reasoning: bool) -> serde_json::Value {
    let mut messages = Vec::new();

    // System message
    if let Some(system) = body.get("system") {
        let system_text = if let Some(s) = system.as_str() {
            s.to_string()
        } else if let Some(arr) = system.as_array() {
            arr.iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };
        if !system_text.is_empty() {
            messages.push(serde_json::json!({"role": "system", "content": system_text}));
        }
    }

    // Conversation messages — handle text, tool_use, and tool_result blocks
    if let Some(msgs) = body.get("messages").and_then(|m| m.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("user");

            match msg.get("content") {
                // Simple string content
                Some(c) if c.is_string() => {
                    if role == "assistant" && c.as_str().is_some_and(str::is_empty) {
                        continue;
                    }
                    messages.push(
                        serde_json::json!({"role": role, "content": c.as_str().unwrap_or("")}),
                    );
                }
                // Array of content blocks
                Some(c) if c.is_array() => {
                    let blocks = c.as_array().unwrap();

                    let text_parts: Vec<&str> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect();

                    let tool_use_blocks: Vec<&serde_json::Value> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
                        .collect();

                    let tool_result_blocks: Vec<&serde_json::Value> = blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
                        .collect();

                    let has_sealed_thinking = replay_reasoning
                        && blocks.iter().any(|block| {
                            block["type"] == "redacted_thinking"
                                || block["type"] == "thinking"
                                    && block["signature"]
                                        .as_str()
                                        .is_some_and(|signature| !signature.is_empty())
                        });
                    let reasoning =
                        if replay_reasoning && role == "assistant" && !has_sealed_thinking {
                            blocks
                                .iter()
                                .filter(|block| block["type"] == "thinking")
                                .filter_map(|block| block["thinking"].as_str())
                                .collect::<String>()
                        } else {
                            String::new()
                        };

                    if !tool_use_blocks.is_empty() {
                        let tool_calls: Vec<serde_json::Value> = tool_use_blocks.iter().map(|tu| {
                            let id = tu.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
                            let name = tu.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let input = tu.get("input").cloned().unwrap_or(serde_json::json!({}));
                            serde_json::json!({
                                "id": id, "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string())
                                }
                            })
                        }).collect();
                        let text_content = text_parts.join("");
                        let mut assistant_msg = serde_json::json!({
                            "role": "assistant",
                            "tool_calls": tool_calls
                        });
                        if replay_reasoning || !text_content.is_empty() {
                            assistant_msg["content"] = serde_json::json!(text_content);
                        }
                        if !reasoning.trim().is_empty() {
                            assistant_msg["reasoning_content"] = serde_json::json!(reasoning);
                        }
                        messages.push(assistant_msg);
                    } else if !tool_result_blocks.is_empty() {
                        let mut follow_up_content = Vec::new();
                        for tr in &tool_result_blocks {
                            let tool_call_id =
                                tr.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let (mut result_content, attachments) =
                                anthropic_tool_result_content(tr);
                            if result_content.is_empty() && !attachments.is_empty() {
                                result_content =
                                    "Attachments are included in the following user message."
                                        .to_string();
                            }
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": result_content
                            }));
                            if !attachments.is_empty() {
                                follow_up_content.push(serde_json::json!({
                                    "type": "text",
                                    "text": format!("Attachments from tool result {tool_call_id}:")
                                }));
                                follow_up_content.extend(attachments);
                            }
                        }
                        follow_up_content.extend(
                            blocks
                                .iter()
                                .filter(|block| {
                                    !matches!(
                                        block.get("type").and_then(|value| value.as_str()),
                                        Some("tool_result" | "tool_use")
                                    )
                                })
                                .filter_map(anthropic_user_content_part),
                        );
                        if !follow_up_content.is_empty() {
                            messages.push(serde_json::json!({
                                "role": role,
                                "content": openai_user_content(follow_up_content)
                            }));
                        }
                    } else {
                        let content: Vec<serde_json::Value> = blocks
                            .iter()
                            .filter_map(anthropic_user_content_part)
                            .collect();
                        // Strict Chat upstreams refuse an interrupted turn's empty reply.
                        if role == "assistant"
                            && reasoning.trim().is_empty()
                            && content.iter().all(|part| part["text"] == "")
                        {
                            continue;
                        }
                        let mut message = serde_json::json!({
                            "role": role,
                            "content": openai_user_content(content)
                        });
                        if !reasoning.trim().is_empty() {
                            message["reasoning_content"] = serde_json::json!(reasoning);
                        }
                        messages.push(message);
                    }
                }
                _ if role == "assistant" => {}
                _ => {
                    messages.push(serde_json::json!({"role": role, "content": ""}));
                }
            }
        }
    }

    let mut req = serde_json::json!({
        "model": body.get("model").and_then(|m| m.as_str()).unwrap_or("local-model"),
        "messages": messages,
        "max_tokens": body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        "stream": body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false),
    });

    if let Some(temperature) = body.get("temperature") {
        req["temperature"] = temperature.clone();
    }

    if let Some(tools) = body.get("tools").and_then(|t| t.as_array()) {
        if !tools.is_empty() {
            let openai_tools: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "description": t.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                        "parameters": t.get("input_schema").cloned()
                            .unwrap_or(serde_json::json!({"type": "object", "properties": {}}))
                    }
                })
            }).collect();
            req["tools"] = serde_json::json!(openai_tools);
            if let Some(tc) = body.get("tool_choice") {
                let choice_type = tc.get("type").and_then(|v| v.as_str()).unwrap_or("auto");
                req["tool_choice"] = match choice_type {
                    "any" => serde_json::json!("required"),
                    "none" => serde_json::json!("none"),
                    "tool" => serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": tc.get("name").and_then(|v| v.as_str()).unwrap_or("")
                        }
                    }),
                    _ => serde_json::json!("auto"),
                };
                if tc
                    .get("disable_parallel_tool_use")
                    .and_then(|v| v.as_bool())
                    == Some(true)
                {
                    req["parallel_tool_calls"] = serde_json::json!(false);
                }
            }
        }
    }

    if let Some(stop) = body.get("stop_sequences") {
        req["stop"] = stop.clone();
    }
    if let Some(top_p) = body.get("top_p") {
        req["top_p"] = top_p.clone();
    }

    req
}

/// Convert OpenAI Chat Completions non-streaming response → Anthropic Messages response
pub fn openai_to_anthropic(data: &serde_json::Value) -> serde_json::Value {
    let model = data
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("local-model");
    let choice = data
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let message = choice.and_then(|c| c.get("message"));
    let finish_reason = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("end_turn");

    let mut content_blocks: Vec<serde_json::Value> = Vec::new();

    if let Some(thinking) = message
        .and_then(|message| message["reasoning_content"].as_str())
        .filter(|thinking| !thinking.trim().is_empty())
    {
        content_blocks.push(serde_json::json!({"type": "thinking", "thinking": thinking}));
    }

    if let Some(text) = message.and_then(|m| {
        m.get("content")
            .and_then(|c| c.as_str())
            .filter(|text| !text.is_empty())
            .or_else(|| m.get("refusal").and_then(|value| value.as_str()))
    }) {
        if !text.is_empty() {
            content_blocks.push(serde_json::json!({"type": "text", "text": text}));
        }
    }

    if let Some(tool_calls) = message
        .filter(|_| !matches!(finish_reason, "length" | "content_filter"))
        .and_then(|m| m.get("tool_calls"))
        .and_then(|t| t.as_array())
    {
        for tc in tool_calls {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("call_0");
            let name = tc
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args_str = tc
                .get("function")
                .and_then(|f| f.get("arguments"))
                .and_then(|v| v.as_str())
                .unwrap_or("{}");
            let input: serde_json::Value =
                serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
            content_blocks.push(serde_json::json!({
                "type": "tool_use", "id": id, "name": name, "input": input
            }));
        }
    }

    if content_blocks.is_empty() {
        content_blocks.push(serde_json::json!({"type": "text", "text": ""}));
    }

    let stop_reason = anthropic_stop_reason(
        finish_reason,
        content_blocks
            .iter()
            .any(|block| block["type"] == "tool_use"),
    );

    serde_json::json!({
        "id": format!("msg_{}", chrono::Utc::now().timestamp_millis()),
        "type": "message",
        "role": "assistant",
        "content": content_blocks,
        "model": model,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": data.get("usage").and_then(|u| u.get("prompt_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
            "output_tokens": data.get("usage").and_then(|u| u.get("completion_tokens")).and_then(|v| v.as_u64()).unwrap_or(0),
        }
    })
}

pub fn anthropic_stop_reason(finish_reason: &str, has_tool_calls: bool) -> &'static str {
    match finish_reason {
        "length" => "max_tokens",
        "content_filter" => "refusal",
        _ if has_tool_calls => "tool_use",
        _ => "end_turn",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_reasoning_chat_tool_round_trip_without_fabricating_thinking() {
        let first = openai_to_anthropic(&serde_json::json!({
            "choices":[{
                "message":{"reasoning_content":"Check the first result.", "content":null,
                    "tool_calls":[{"id":"call_first","function":{"name":"read","arguments":"{}"}}]},
                "finish_reason":"tool_calls"
            }]
        }));
        let mut follow_up = serde_json::json!({
            "thinking":{"type":"adaptive"},
            "messages":[
                {"role":"user","content":"Check the file."},
                {"role":"assistant","content":first["content"]},
                {"role":"user","content":[
                    {"type":"tool_result","tool_use_id":"call_first","content":"first result"},
                    {"type":"text","text":"Compare it to the second."}
                ]}
            ],
            "tools":[{"name":"read","input_schema":{"type":"object"}}]
        });
        let converted = anthropic_to_reasoning_chat(&follow_up);
        assert_eq!(converted["thinking"]["type"], "enabled");
        assert_eq!(
            converted["messages"][1]["reasoning_content"],
            "Check the first result."
        );
        assert_eq!(converted["messages"][1]["content"], "");
        assert_eq!(
            converted["messages"][1]["tool_calls"][0]["id"],
            "call_first"
        );
        assert_eq!(converted["messages"][2]["tool_call_id"], "call_first");
        assert_eq!(
            converted["messages"][3]["content"],
            "Compare it to the second."
        );

        let generic = anthropic_to_openai(&follow_up);
        assert!(generic.get("thinking").is_none());
        assert!(generic["messages"][1].get("reasoning_content").is_none());
        assert_eq!(generic["messages"][2]["content"], "first result");

        follow_up["messages"][1]["content"][0]["signature"] = serde_json::json!("native-signature");
        let converted = anthropic_to_reasoning_chat(&follow_up);
        assert_eq!(converted["thinking"]["type"], "disabled");
        assert!(converted["messages"][1].get("reasoning_content").is_none());
        assert_eq!(converted["messages"][2]["tool_call_id"], "call_first");

        follow_up["messages"].as_array_mut().unwrap().insert(
            1,
            serde_json::json!({"role":"assistant","content":[
                {"type":"thinking","thinking":"Interrupted"}
            ]}),
        );
        follow_up["messages"][2]["content"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"type":"thinking","thinking":"Partial replay"}));
        let converted = anthropic_to_reasoning_chat(&follow_up);
        assert_eq!(converted["thinking"]["type"], "disabled");
        assert_eq!(converted["messages"].as_array().unwrap().len(), 4);
        assert!(converted["messages"][1].get("reasoning_content").is_none());
        assert_eq!(converted["messages"][2]["tool_call_id"], "call_first");
    }

    #[test]
    fn interrupted_chat_responses_do_not_publish_partial_tool_calls() {
        for (finish, stop) in [("length", "max_tokens"), ("content_filter", "refusal")] {
            let converted = openai_to_anthropic(&serde_json::json!({
                "choices":[{"finish_reason":finish,"message":{"content":"partial",
                    "tool_calls":[{"id":"incomplete","function":{"name":"write","arguments":"{"}}]}}]
            }));
            assert_eq!(converted["stop_reason"], stop);
            assert_eq!(
                converted["content"],
                serde_json::json!([{"type":"text","text":"partial"}])
            );
        }
    }

    #[test]
    fn an_interrupted_empty_reply_is_not_sent_to_chat() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [
                {"role": "user", "content": "First"},
                {"role": "assistant", "content": ""},
                {"role": "assistant", "content": [{"type": "thinking", "thinking": "partial"}]},
                {"role": "assistant", "content": [{"type": "text", "text": ""}]},
                {"role": "assistant"},
                {"role": "user", "content": "Second"},
                {"role": "assistant", "content": [{"type": "text", "text": "Kept"}]}
            ]
        }));
        let roles: Vec<_> = converted["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "user", "assistant"]);
    }

    #[test]
    fn converts_anthropic_image_and_document_inputs() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "model": "claude-test",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Inspect these"},
                    {"type": "image", "source": {
                        "type": "base64", "media_type": "image/png", "data": "aW1hZ2U="
                    }},
                    {"type": "image", "source": {
                        "type": "url", "url": "https://example.com/image.png"
                    }},
                    {"type": "document", "title": "report.pdf", "source": {
                        "type": "base64", "media_type": "application/pdf", "data": "cGRm"
                    }}
                ]
            }]
        }));

        let content = converted["messages"][0]["content"].as_array().unwrap();
        assert_eq!(
            content[0],
            serde_json::json!({"type": "text", "text": "Inspect these"})
        );
        assert_eq!(
            content[1]["image_url"]["url"],
            "data:image/png;base64,aW1hZ2U="
        );
        assert_eq!(
            content[2]["image_url"]["url"],
            "https://example.com/image.png"
        );
        assert_eq!(content[3]["file"]["file_data"], "cGRm");
        assert_eq!(content[3]["file"]["filename"], "report.pdf");
    }

    #[test]
    fn collapses_anthropic_text_blocks_for_openai_compatibility() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "Hello, "},
                    {"type": "text", "text": "world"}
                ]
            }]
        }));

        assert_eq!(converted["messages"][0]["content"], "Hello, world");
        assert!(converted.get("temperature").is_none());
    }

    #[test]
    fn preserves_tool_result_attachments_and_follow_up_text() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "tool_result", "tool_use_id": "call_1", "content": [
                        {"type": "text", "text": "Screenshot captured."},
                        {"type": "image", "source": {
                            "type": "base64", "media_type": "image/jpeg", "data": "anBlZw=="
                        }}
                    ]},
                    {"type": "text", "text": "What is wrong?"}
                ]
            }]
        }));

        let messages = converted["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "tool");
        assert_eq!(messages[0]["tool_call_id"], "call_1");
        assert_eq!(messages[0]["content"], "Screenshot captured.");
        assert_eq!(messages[1]["role"], "user");
        let follow_up = messages[1]["content"].as_array().unwrap();
        assert_eq!(
            follow_up[1]["image_url"]["url"],
            "data:image/jpeg;base64,anBlZw=="
        );
        assert_eq!(follow_up[2]["text"], "What is wrong?");
    }

    #[test]
    fn tool_reference_results_keep_loaded_tool_names_and_pairing() {
        for content in [
            serde_json::json!([
                {"type":"tool_reference","tool_name":"mcp__fixture__read"},
                {"type":"tool_reference","tool_name":"mcp__fixture__write"}
            ]),
            serde_json::json!([
                {"type":"text","text":"Loaded tools:"},
                {"type":"tool_reference","tool_name":"mcp__fixture__read"},
                {"type":"tool_reference","tool_name":"mcp__fixture__write"}
            ]),
        ] {
            let converted = anthropic_to_openai(&serde_json::json!({"messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"search_call","name":"ToolSearch","input":{"query":"fixture"}}]},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"search_call","content":content}]}
            ]}));
            let messages = converted["messages"].as_array().unwrap();
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0]["tool_calls"][0]["id"], "search_call");
            assert_eq!(messages[1]["role"], "tool");
            assert_eq!(messages[1]["tool_call_id"], "search_call");
            let text = messages[1]["content"].as_str().unwrap();
            assert!(text.contains("mcp__fixture__read"));
            assert!(text.contains("mcp__fixture__write"));
            assert!(!text.is_empty());
            if content[0]["type"] == "text" {
                assert!(text.starts_with("Loaded tools:"));
            }
        }
    }

    #[test]
    fn json_refusals_remain_visible_when_answer_content_is_missing() {
        for message in [
            serde_json::json!({"role":"assistant","refusal":"I cannot help with that."}),
            serde_json::json!({"role":"assistant","content":null,"refusal":"I cannot help with that."}),
            serde_json::json!({"role":"assistant","content":"","refusal":"I cannot help with that."}),
        ] {
            let converted = openai_to_anthropic(
                &serde_json::json!({"choices":[{"message":message,"finish_reason":"stop"}]}),
            );
            assert_eq!(
                converted["content"],
                serde_json::json!([{"type":"text","text":"I cannot help with that."}])
            );
        }
        let converted = openai_to_anthropic(
            &serde_json::json!({"choices":[{"message":{"content":"Answer already present","refusal":"Other explanation"},"finish_reason":"stop"}]}),
        );
        assert_eq!(converted["content"][0]["text"], "Answer already present");
    }

    #[test]
    fn historical_tool_calls_do_not_include_stream_indexes() {
        let converted = anthropic_to_openai(&serde_json::json!({
            "messages": [{
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": "call_1",
                    "name": "read_file",
                    "input": {"path": "a.txt"}
                }]
            }]
        }));

        let tool_call = &converted["messages"][0]["tool_calls"][0];
        assert_eq!(tool_call["id"], "call_1");
        assert!(tool_call.get("index").is_none());
    }
}
