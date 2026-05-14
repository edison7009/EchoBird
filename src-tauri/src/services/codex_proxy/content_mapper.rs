// Phase 5 stub — full port of content-mapper.cjs (multimodal `input_image`
// / `image_url` / `input_file` handling) lands in Phase 5. Today this
// covers the text-only branch, which is 99% of real Codex traffic.
//
// Behavior parity with the JS version's text path:
//   • A plain string passes through verbatim.
//   • An array of content parts gets joined: text-bearing parts
//     (input_text / output_text / text) are concatenated; non-text
//     parts are dropped silently for now (Phase 5 builds a proper
//     multimodal array shape).
//   • Anything else stringifies to its JSON form (best-effort fallback).

use serde_json::Value;

pub fn value_to_chat_content(content: Option<&Value>) -> Value {
    match content {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(parts)) => {
            let texts: Vec<&str> = parts
                .iter()
                .filter_map(|p| {
                    let t = p.get("type").and_then(|v| v.as_str())?;
                    if matches!(t, "input_text" | "output_text" | "text") {
                        p.get("text").and_then(|v| v.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            Value::String(texts.join(""))
        }
        Some(other) => Value::String(other.to_string()),
    }
}
