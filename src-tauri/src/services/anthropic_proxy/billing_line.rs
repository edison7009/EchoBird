// Claude Code opens its system prompt with a billing line whose `cch` field
// changes every turn. Anthropic ignores it, but any other vendor reads it as
// the first words of the prompt, so its prefix cache misses from byte zero.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

fn billing_line() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^x-anthropic-billing-header:(\s*[A-Za-z_]+=[^;\s]*;)*\s*")
            .expect("static billing line pattern")
    })
}

fn without_billing_line(text: &str) -> Option<&str> {
    billing_line().find(text).map(|line| &text[line.end()..])
}

/// A Claude model keeps the line; the router decides for its own candidates.
pub(super) fn keeps_billing_line(model: &str, router_upstream: bool) -> bool {
    router_upstream || model.to_ascii_lowercase().contains("claude")
}

pub(super) fn strip_from_anthropic(body: &mut Value) {
    let Some(request) = body.as_object_mut() else {
        return;
    };
    let empty = match request.get_mut("system") {
        Some(Value::String(system)) => {
            let Some(rest) = without_billing_line(system).map(str::to_owned) else {
                return;
            };
            *system = rest;
            system.is_empty()
        }
        Some(Value::Array(blocks)) => {
            for block in blocks.iter_mut() {
                if let Some(text) = block
                    .as_object_mut()
                    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                    .and_then(|block| block.get_mut("text"))
                {
                    let rest = text
                        .as_str()
                        .and_then(without_billing_line)
                        .map(str::to_owned);
                    if let Some(rest) = rest {
                        *text = Value::String(rest);
                    }
                }
            }
            blocks.retain(|block| block["type"] != "text" || block["text"] != "");
            blocks.is_empty()
        }
        _ => return,
    };
    if empty {
        request.remove("system");
    }
}

pub(crate) fn strip_from_chat(body: &mut Value) {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let Some(first) = messages
        .first_mut()
        .filter(|message| message["role"] == "system")
    else {
        return;
    };
    let Some(rest) = first["content"]
        .as_str()
        .and_then(without_billing_line)
        .map(str::to_owned)
    else {
        return;
    };
    if rest.is_empty() {
        messages.remove(0);
    } else {
        first["content"] = Value::String(rest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const LINE: &str =
        "x-anthropic-billing-header: cc_version=2.1.114; cc_entrypoint=cli; cch=abc12;";

    #[test]
    fn anthropic_body_loses_only_the_billing_line() {
        let mut blocks = json!({"system":[
            {"type":"text","text":LINE},
            {"type":"text","text":"You are helpful.","cache_control":{"type":"ephemeral"}}
        ]});
        strip_from_anthropic(&mut blocks);
        assert_eq!(
            blocks["system"],
            json!([{"type":"text","text":"You are helpful.","cache_control":{"type":"ephemeral"}}])
        );

        let mut prefixed =
            json!({"system":[{"type":"text","text":format!("{LINE}\nYou are helpful.")}]});
        strip_from_anthropic(&mut prefixed);
        assert_eq!(prefixed["system"][0]["text"], "You are helpful.");

        let mut text = json!({"system":format!("{LINE} You are helpful.")});
        strip_from_anthropic(&mut text);
        assert_eq!(text["system"], "You are helpful.");

        let mut only_line = json!({"system":[{"type":"text","text":LINE}]});
        strip_from_anthropic(&mut only_line);
        assert!(only_line.get("system").is_none());
    }

    #[test]
    fn chat_body_loses_only_the_billing_line() {
        let mut joined = json!({"messages":[
            {"role":"system","content":format!("{LINE}You are helpful.")},
            {"role":"user","content":"hello"}
        ]});
        strip_from_chat(&mut joined);
        assert_eq!(joined["messages"][0]["content"], "You are helpful.");

        let mut only_line = json!({"messages":[
            {"role":"system","content":LINE},
            {"role":"user","content":"hello"}
        ]});
        strip_from_chat(&mut only_line);
        assert_eq!(only_line["messages"].as_array().unwrap().len(), 1);
        assert_eq!(only_line["messages"][0]["role"], "user");
    }

    #[test]
    fn a_prompt_that_only_mentions_the_header_is_left_alone() {
        let original = json!({
            "system":"Explain x-anthropic-billing-header: cch=1;",
            "messages":[{"role":"system","content":"Explain x-anthropic-billing-header: cch=1;"}]
        });
        let mut body = original.clone();
        strip_from_anthropic(&mut body);
        strip_from_chat(&mut body);
        assert_eq!(body, original);
    }

    #[test]
    fn claude_models_and_router_upstreams_keep_the_line() {
        assert!(keeps_billing_line("claude-sonnet-5-5", false));
        assert!(keeps_billing_line("Anthropic/Claude-Opus-5-5", false));
        assert!(keeps_billing_line("deepseek-v4", true));
        assert!(!keeps_billing_line("deepseek-v4", false));
    }
}
