// 3-layer JSON repair for LLM tool-call arguments.
//
// Layer 1: direct parse (fast path, no work for valid JSON).
// Layer 2: structural repair — markdown fences, trailing commas, single quotes,
//          unquoted keys, missing closing braces, invalid `\.` `\w` `\d` escapes,
//          missing commas between fields.
// Layer 3: tool-specific extractor for `file_edit` (source code in old/new_string
//          often contains unescaped quotes/newlines that no generic repair handles).
// Layer 4 (last-resort): key-value scrape — find every "key": value pair by string
//          matching, ignoring outer structure.
//
// If every layer fails the original string is returned unchanged so the tool's
// own parse error reaches the model (a misleading "repaired" stub is worse than
// the real error).

/// Normalize tool-call arguments into valid JSON before execution.
///
/// `tool_name` selects a specialized extractor when available (`file_edit`).
pub fn repair_tool_args(tool_name: &str, args: &str) -> String {
    if serde_json::from_str::<serde_json::Value>(args).is_ok() {
        return args.to_string();
    }
    let repaired = repair_json(args);
    if serde_json::from_str::<serde_json::Value>(&repaired).is_ok() {
        return repaired;
    }
    if tool_name == "file_edit" {
        if let Some(v) = extract_file_edit_args(args) {
            if let Ok(s) = serde_json::to_string(&v) {
                return s;
            }
        }
    }
    let extracted = extract_json_fields(args);
    if let Some(obj) = extracted.as_object() {
        if !obj.is_empty() {
            if let Ok(s) = serde_json::to_string(&extracted) {
                return s;
            }
        }
    }
    args.to_string()
}

/// Repair common LLM-JSON issues. Order matters: escape-fix before fence-strip
/// (fences may sit inside an outer string), unquoted-key insertion before
/// trailing-comma cleanup (a freshly inserted key shouldn't end up trailing).
pub fn repair_json(s: &str) -> String {
    let mut result = s.to_string();

    // Fix invalid backslash escapes. JSON only allows: \\ \" \/ \n \r \t \b \f \uXXXX.
    // Models often write regex like `app\.rs` (\. is invalid) — double the
    // backslash so the parser sees a literal backslash + char.
    let valid_escapes = ['\\', '"', '/', 'n', 'r', 't', 'b', 'f', 'u'];
    let chars: Vec<char> = result.chars().collect();
    let mut fixed = String::with_capacity(result.len() + 16);
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if valid_escapes.contains(&next) {
                fixed.push('\\');
                fixed.push(next);
            } else {
                fixed.push('\\');
                fixed.push('\\');
                fixed.push(next);
            }
            i += 2;
        } else {
            fixed.push(chars[i]);
            i += 1;
        }
    }
    result = fixed;

    // Strip markdown fences.
    result = result.trim().to_string();
    if let Some(rest) = result.strip_prefix("```json") {
        result = rest.to_string();
    } else if let Some(rest) = result.strip_prefix("```") {
        result = rest.to_string();
    }
    if let Some(rest) = result.strip_suffix("```") {
        result = rest.to_string();
    }
    result = result.trim().to_string();

    // Single-quote → double-quote, only when no double quotes exist (otherwise
    // we risk corrupting string contents that contain apostrophes).
    if !result.contains('"') && result.contains('\'') {
        result = result.replace('\'', "\"");
    }

    // Insert missing commas between adjacent string-keyed fields:
    //   {"a": "x" "b": 1}  →  {"a": "x", "b": 1}
    let chars: Vec<char> = result.chars().collect();
    let mut insertions: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '"' {
            let after_close = i + 1;
            let mut k = after_close;
            while k < chars.len() && chars[k].is_whitespace() {
                k += 1;
            }
            if k < chars.len() && chars[k] == '"' && k > after_close {
                let mut q = k + 1;
                while q < chars.len() && chars[q] != '"' {
                    q += 1;
                }
                if q + 1 < chars.len() {
                    let mut r = q + 1;
                    while r < chars.len() && chars[r].is_whitespace() {
                        r += 1;
                    }
                    if r < chars.len() && chars[r] == ':' {
                        insertions.push(after_close);
                    }
                }
            }
        }
        i += 1;
    }
    let mut chars = chars;
    for pos in insertions.into_iter().rev() {
        chars.insert(pos, ',');
    }
    result = chars.into_iter().collect();

    // Quote bare keys: {path: "src"} → {"path": "src"}
    let rchars: Vec<char> = result.chars().collect();
    let mut fixed = String::with_capacity(result.len() + 16);
    let mut ri = 0;
    while ri < rchars.len() {
        if rchars[ri] == '{' || rchars[ri] == ',' {
            fixed.push(rchars[ri]);
            ri += 1;
            while ri < rchars.len() && rchars[ri].is_whitespace() {
                fixed.push(rchars[ri]);
                ri += 1;
            }
            if ri < rchars.len() && rchars[ri].is_alphabetic() {
                let key_start = ri;
                while ri < rchars.len() && (rchars[ri].is_alphanumeric() || rchars[ri] == '_') {
                    ri += 1;
                }
                let mut ki = ri;
                while ki < rchars.len() && rchars[ki].is_whitespace() {
                    ki += 1;
                }
                if ki < rchars.len() && rchars[ki] == ':' {
                    fixed.push('"');
                    for c in &rchars[key_start..ri] {
                        fixed.push(*c);
                    }
                    fixed.push('"');
                } else {
                    for c in &rchars[key_start..ri] {
                        fixed.push(*c);
                    }
                }
            }
        } else {
            fixed.push(rchars[ri]);
            ri += 1;
        }
    }
    result = fixed;

    // Drop trailing commas: {"a":1,} → {"a":1}, [1,] → [1].
    loop {
        let before = result.clone();
        result = result.replace(",}", "}").replace(",]", "]");
        if result == before {
            break;
        }
    }

    // Wrap if neither a JSON object nor array.
    if !result.starts_with('{') && !result.starts_with('[') {
        result = format!("{{{}}}", result);
    }

    // Pad missing closing braces. (Mismatched `[` is rarer; we don't pad those.)
    let open = result.chars().filter(|c| *c == '{').count();
    let close = result.chars().filter(|c| *c == '}').count();
    for _ in 0..open.saturating_sub(close) {
        result.push('}');
    }

    result
}

/// Last-resort: scrape every `"key": value` pair by string matching, ignoring
/// outer JSON structure. Returns an empty object if nothing was recovered.
pub fn extract_json_fields(s: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let key = if chars[i] == '"' {
            let start = i + 1;
            i = start;
            while i < len && chars[i] != '"' {
                i += 1;
            }
            if i >= len {
                break;
            }
            let k: String = chars[start..i].iter().collect();
            i += 1;
            k
        } else if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            chars[start..i].iter().collect()
        } else {
            i += 1;
            continue;
        };

        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len || chars[i] != ':' {
            continue;
        }
        i += 1;
        while i < len && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= len {
            break;
        }

        if chars[i] == '"' {
            let start = i + 1;
            i = start;
            while i < len && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            let raw: String = chars[start..i.min(len)].iter().collect();
            let val = raw
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
            map.insert(key, serde_json::json!(val));
            if i < len {
                i += 1;
            }
        } else if chars[i] == 't' || chars[i] == 'f' {
            let start = i;
            while i < len && chars[i].is_alphabetic() {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.as_str() {
                "true" => {
                    map.insert(key, serde_json::json!(true));
                }
                "false" => {
                    map.insert(key, serde_json::json!(false));
                }
                _ => {
                    map.insert(key, serde_json::json!(word));
                }
            }
        } else if chars[i].is_ascii_digit() || chars[i] == '-' {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '-') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if let Ok(n) = num_str.parse::<i64>() {
                map.insert(key, serde_json::json!(n));
            } else if let Ok(f) = num_str.parse::<f64>() {
                map.insert(key, serde_json::json!(f));
            }
        } else {
            let start = i;
            while i < len && !matches!(chars[i], ',' | '}' | ']' | '\n') {
                i += 1;
            }
            let val: String = chars[start..i]
                .iter()
                .collect::<String>()
                .trim()
                .to_string();
            if !val.is_empty() {
                map.insert(key, serde_json::json!(val));
            }
        }
    }

    serde_json::Value::Object(map)
}

/// Specialized extractor for `file_edit { file_path, old_string, new_string,
/// replace_all? }`. Models routinely embed unescaped quotes / newlines in
/// old_string and new_string, which no generic repair handles. We use the known
/// field order and slice by marker positions.
pub fn extract_file_edit_args(raw: &str) -> Option<serde_json::Value> {
    let fp_marker = raw.find("\"file_path\"")?;
    let old_marker = raw.find("\"old_string\"")?;
    let new_marker = raw.find("\"new_string\"")?;
    if old_marker <= fp_marker || new_marker <= old_marker {
        return None;
    }

    let fp_region = &raw[fp_marker + "\"file_path\"".len()..old_marker];
    let fp_colon = fp_region.find(':')?;
    let fp_val = fp_region[fp_colon + 1..]
        .trim()
        .trim_matches(|c| c == '"' || c == ',')
        .trim();
    if fp_val.is_empty() {
        return None;
    }
    let file_path = fp_val.to_string();

    let old_colon = raw[old_marker..].find(':')?;
    let old_start = old_marker + old_colon + 1;
    let old_raw = &raw[old_start..new_marker];
    let old_string = unescape_inner(old_raw, true);

    let new_colon = raw[new_marker..].find(':')?;
    let new_start = new_marker + new_colon + 1;
    let new_raw = &raw[new_start..];
    let new_string = unescape_inner_tail(new_raw);

    if old_string.is_empty() && new_string.is_empty() {
        return None;
    }

    // replace_all: only true if the literal `true` appears AFTER `"replace_all"`.
    let replace_all = raw.contains("\"replace_all\"")
        && raw
            .rfind("\"replace_all\"")
            .and_then(|r| raw[r..].find("true"))
            .is_some();

    Some(serde_json::json!({
        "file_path": file_path,
        "old_string": old_string,
        "new_string": new_string,
        "replace_all": replace_all,
    }))
}

fn unescape_inner(raw: &str, trim_trailing_comma: bool) -> String {
    let mut t = raw.trim();
    if trim_trailing_comma {
        t = t.trim_end_matches(',').trim();
    }
    let inner = t.strip_prefix('"').unwrap_or(t);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    inner
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

fn unescape_inner_tail(raw: &str) -> String {
    let t = raw.trim();
    let inner = t.strip_prefix('"').unwrap_or(t);
    let end = inner
        .rfind("\", \"replace_all\"")
        .or_else(|| inner.rfind("\"}"))
        .or_else(|| inner.rfind("\"\n}"))
        .unwrap_or(inner.len());
    let content = &inner[..end];
    content
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("expected valid JSON")
    }

    #[test]
    fn passes_valid_json_through_unchanged() {
        let input = r#"{"file_path":"/tmp/a.rs","content":"x"}"#;
        assert_eq!(repair_tool_args("file_write", input), input);
    }

    #[test]
    fn fixes_trailing_comma() {
        let v = parse(&repair_json(r#"{"key": "value",}"#));
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn fixes_single_quotes() {
        let v = parse(&repair_json("{'key': 'value'}"));
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn fixes_missing_closing_brace() {
        let v = parse(&repair_json(r#"{"key": "value""#));
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn fixes_unquoted_keys() {
        let v = parse(&repair_json(r#"{path: "src/main.rs"}"#));
        assert_eq!(v["path"], "src/main.rs");
    }

    #[test]
    fn fixes_invalid_backslash_dot_in_regex() {
        // `\.` isn't a valid JSON escape; doubling lets it parse as literal `\` + `.`.
        let v = parse(&repair_json(r#"{"pattern": "app\.rs"}"#));
        assert!(v["pattern"].as_str().unwrap().contains('.'));
    }

    #[test]
    fn fixes_markdown_json_fence() {
        let v = parse(&repair_json("```json\n{\"key\": \"value\"}\n```"));
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn fixes_markdown_bare_fence() {
        let v = parse(&repair_json("```\n{\"key\": \"value\"}\n```"));
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn fixes_missing_comma_between_fields() {
        let repaired = repair_json(r#"{"path": "src" "depth": 2}"#);
        let v = parse(&repaired);
        assert_eq!(v["path"], "src");
        assert_eq!(v["depth"], 2);
    }

    #[test]
    fn extract_fields_basic() {
        let v = extract_json_fields(r#"{"file_path": "/src/main.rs", "pattern": "hello"}"#);
        assert_eq!(v["file_path"], "/src/main.rs");
        assert_eq!(v["pattern"], "hello");
    }

    #[test]
    fn extract_fields_booleans_and_numbers() {
        let v = extract_json_fields(r#"{"recursive": true, "depth": 3}"#);
        assert_eq!(v["recursive"], true);
        assert_eq!(v["depth"], 3);
    }

    #[test]
    fn extract_file_edit_handles_escaped_newlines() {
        let raw = r#"{"file_path": "/src/lib.rs", "old_string": "fn old(){\n}", "new_string": "fn new(){\n}"}"#;
        let v = extract_file_edit_args(raw).expect("should parse");
        assert_eq!(v["file_path"], "/src/lib.rs");
        assert!(v["old_string"].as_str().unwrap().contains('\n'));
        assert!(v["new_string"].as_str().unwrap().contains('\n'));
    }

    #[test]
    fn extract_file_edit_returns_none_on_missing_markers() {
        assert!(extract_file_edit_args(r#"{"file_path": "/src/lib.rs"}"#).is_none());
    }

    #[test]
    fn extract_file_edit_replace_all_true() {
        let raw = r#"{"file_path": "/src/lib.rs", "old_string": "foo", "new_string": "bar", "replace_all": true}"#;
        let v = extract_file_edit_args(raw).expect("should parse");
        assert_eq!(v["replace_all"], true);
    }

    #[test]
    fn keeps_empty_object_untouched() {
        // `{}` is valid JSON; we must not invent fields. Caller surfaces it as
        // a real "missing required argument" error.
        assert_eq!(repair_tool_args("file_write", "{}"), "{}");
    }

    #[test]
    fn returns_original_when_unsalvageable() {
        // Pure garbage with no extractable key=value pairs → return as-is so
        // the tool emits the real parse error, not a misleading repaired stub.
        assert_eq!(repair_tool_args("file_write", "!!!"), "!!!");
    }

    #[test]
    fn full_chain_recovers_fence_wrapped_json() {
        let input = "```json\n{\"file_path\":\"/tmp/a.rs\",\"content\":\"x\"}\n```";
        let v = parse(&repair_tool_args("file_write", input));
        assert_eq!(v["file_path"], "/tmp/a.rs");
    }

    #[test]
    fn full_chain_recovers_unquoted_keys_via_repair() {
        let input = r#"{file_path: "/tmp/a.rs", content: "x"}"#;
        let v = parse(&repair_tool_args("file_write", input));
        assert_eq!(v["file_path"], "/tmp/a.rs");
    }
}
