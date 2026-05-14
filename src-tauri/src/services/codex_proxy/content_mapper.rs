// Content translation (Responses parts ↔ Chat parts) — port of
// tools/codex/lib/content-mapper.cjs.
//
// Responses API content is either a string OR an array of typed parts:
//   { type: "input_text",  text: "..." }                  — user input text
//   { type: "text",        text: "..." }                  — generic text
//   { type: "output_text", text: "..." }                  — assistant history replay
//   { type: "input_image", image_url: "data:..." }        — image as URL/data URI
//   { type: "image_url",   image_url: "..." | {url:".."} } — already chat shape
//
// Chat Completions accepts content as string OR an array of:
//   { type: "text",      text: "..." }
//   { type: "image_url", image_url: { url: "..." } }
//
// We collapse all-text parts to a plain string (less verbose, more
// providers accept it), otherwise emit the multimodal array shape.

use serde_json::{json, Value};

/// Translate one Responses-shape content part into Chat-shape.
pub fn map_content_part(part: &Value) -> Value {
    let kind = part.get("type").and_then(|v| v.as_str());
    match kind {
        Some("input_text") | Some("text") | Some("output_text") => {
            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "type": "text", "text": text })
        }
        Some("input_image") => {
            // Responses API: image_url is a plain string (often a data: URL).
            // Chat Completions wants it wrapped: { image_url: { url: "..." } }.
            let url = part.get("image_url").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "type": "image_url", "image_url": { "url": url } })
        }
        Some("image_url") => {
            // Either already chat-shape ({url:...} object) or flat string.
            let raw = part.get("image_url");
            let inner = match raw {
                Some(o) if o.is_object() => o.clone(),
                Some(Value::String(s)) => json!({ "url": s }),
                _ => json!({ "url": "" }),
            };
            json!({ "type": "image_url", "image_url": inner })
        }
        _ => {
            // Unknown / future part type: pass through verbatim so
            // providers that accept it can use it, and we don't crash
            // on schemas the launcher hasn't been updated to know about.
            part.clone()
        }
    }
}

/// Translate a Responses-shape content value (string, array of parts,
/// or anything else) into a Chat-shape content value. Returns:
///   • Value::String("") for None / Null (caller injects the assistant
///     `tool_calls` null contract separately when needed)
///   • Value::String for plain strings
///   • Value::String when the array is all-text (joined)
///   • Value::Array of mapped parts when the array contains an image
///   • Stringified JSON for any other shape (defensive fallback)
pub fn value_to_chat_content(content: Option<&Value>) -> Value {
    match content {
        None | Some(Value::Null) => Value::String(String::new()),
        Some(Value::String(s)) => Value::String(s.clone()),
        Some(Value::Array(parts)) => {
            // Pure text array → collapse to a single string (lower-friction
            // shape for providers that don't fully support multimodal
            // content arrays). output_text is treated like text because
            // that's what Codex replays for assistant history items.
            let has_non_text = parts.iter().any(|p| {
                let k = p.get("type").and_then(|v| v.as_str());
                match k {
                    Some(k) => k != "input_text" && k != "text" && k != "output_text",
                    None => false,
                }
            });
            if !has_non_text {
                let joined: String = parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
                    .collect();
                Value::String(joined)
            } else {
                Value::Array(parts.iter().map(map_content_part).collect())
            }
        }
        Some(other) => Value::String(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- map_content_part ----

    #[test]
    fn map_part_input_text_becomes_text() {
        let part = json!({ "type": "input_text", "text": "hello" });
        assert_eq!(
            map_content_part(&part),
            json!({ "type": "text", "text": "hello" })
        );
    }

    #[test]
    fn map_part_output_text_becomes_text() {
        let part = json!({ "type": "output_text", "text": "world" });
        assert_eq!(
            map_content_part(&part),
            json!({ "type": "text", "text": "world" })
        );
    }

    #[test]
    fn map_part_text_passes_text_through() {
        let part = json!({ "type": "text", "text": "x" });
        assert_eq!(
            map_content_part(&part),
            json!({ "type": "text", "text": "x" })
        );
    }

    #[test]
    fn map_part_text_missing_text_field_becomes_empty() {
        let part = json!({ "type": "input_text" });
        assert_eq!(
            map_content_part(&part),
            json!({ "type": "text", "text": "" })
        );
    }

    #[test]
    fn map_part_input_image_wraps_url_string() {
        let part = json!({ "type": "input_image", "image_url": "data:image/png;base64,xxx" });
        assert_eq!(
            map_content_part(&part),
            json!({
                "type": "image_url",
                "image_url": { "url": "data:image/png;base64,xxx" }
            })
        );
    }

    #[test]
    fn map_part_input_image_missing_url_becomes_empty() {
        let part = json!({ "type": "input_image" });
        assert_eq!(
            map_content_part(&part),
            json!({ "type": "image_url", "image_url": { "url": "" } })
        );
    }

    #[test]
    fn map_part_image_url_string_wraps_to_object() {
        let part = json!({ "type": "image_url", "image_url": "https://example.com/cat.png" });
        assert_eq!(
            map_content_part(&part),
            json!({
                "type": "image_url",
                "image_url": { "url": "https://example.com/cat.png" }
            })
        );
    }

    #[test]
    fn map_part_image_url_object_passes_object_through() {
        let part = json!({
            "type": "image_url",
            "image_url": { "url": "https://example.com/dog.png", "detail": "high" }
        });
        assert_eq!(
            map_content_part(&part),
            json!({
                "type": "image_url",
                "image_url": { "url": "https://example.com/dog.png", "detail": "high" }
            })
        );
    }

    #[test]
    fn map_part_unknown_type_passes_through_verbatim() {
        let part = json!({ "type": "future_widget", "payload": 42 });
        // Pass-through preserves the original shape so providers that
        // accept the new type can still use it.
        assert_eq!(map_content_part(&part), part);
    }

    // ---- value_to_chat_content ----

    #[test]
    fn content_none_returns_empty_string() {
        assert_eq!(value_to_chat_content(None), Value::String(String::new()));
    }

    #[test]
    fn content_null_returns_empty_string() {
        assert_eq!(
            value_to_chat_content(Some(&Value::Null)),
            Value::String(String::new())
        );
    }

    #[test]
    fn content_plain_string_passes_through() {
        let v = json!("hello world");
        assert_eq!(value_to_chat_content(Some(&v)), json!("hello world"));
    }

    #[test]
    fn content_all_text_array_collapses_to_string() {
        let v = json!([
            { "type": "input_text", "text": "hello " },
            { "type": "text", "text": "world" },
        ]);
        assert_eq!(
            value_to_chat_content(Some(&v)),
            Value::String("hello world".into())
        );
    }

    #[test]
    fn content_array_with_image_keeps_array_shape() {
        let v = json!([
            { "type": "input_text", "text": "look at this:" },
            { "type": "input_image", "image_url": "data:image/png;base64,xxx" },
        ]);
        let expected = json!([
            { "type": "text", "text": "look at this:" },
            { "type": "image_url", "image_url": { "url": "data:image/png;base64,xxx" } },
        ]);
        assert_eq!(value_to_chat_content(Some(&v)), expected);
    }

    #[test]
    fn content_array_with_unknown_type_keeps_array_shape() {
        // Unknown type counts as "non-text" — we keep the array form and
        // pass the unknown part through verbatim.
        let v = json!([
            { "type": "input_text", "text": "x" },
            { "type": "future_widget", "data": "y" },
        ]);
        let expected = json!([
            { "type": "text", "text": "x" },
            { "type": "future_widget", "data": "y" },
        ]);
        assert_eq!(value_to_chat_content(Some(&v)), expected);
    }

    #[test]
    fn content_empty_array_collapses_to_empty_string() {
        let v = json!([]);
        assert_eq!(
            value_to_chat_content(Some(&v)),
            Value::String(String::new())
        );
    }

    #[test]
    fn content_array_text_part_without_text_field_treated_as_empty() {
        let v = json!([
            { "type": "text" },
            { "type": "text", "text": "abc" },
        ]);
        assert_eq!(value_to_chat_content(Some(&v)), Value::String("abc".into()));
    }

    #[test]
    fn content_non_string_non_array_stringifies() {
        let v = json!({ "weird": "object" });
        // Defensive fallback — stringify the JSON so we don't drop info.
        let out = value_to_chat_content(Some(&v));
        let s = out.as_str().unwrap();
        assert!(s.contains("weird"), "got: {s}");
        assert!(s.contains("object"), "got: {s}");
    }
}
