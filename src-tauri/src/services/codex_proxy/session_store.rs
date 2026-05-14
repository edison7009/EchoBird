// In-memory session store — port of tools/codex/lib/session-store.cjs.
//
// Three independent maps, all process-local:
//
//   response_history : response_id            → Vec<ChatMessage>
//       Codex uses `previous_response_id` to continue a conversation;
//       we replay the stored messages so each upstream Chat Completions
//       call is self-contained even when Codex omits the redundant
//       input items.
//
//   reasoning        : call_id                → reasoning_content
//       For thinking models (DeepSeek-V4-*, Kimi-K2.6, etc.) the
//       upstream returns reasoning_content alongside tool_calls. We
//       save it keyed by the call_id so when Codex replays the same
//       function_call in a subsequent request, we can re-attach the
//       saved reasoning_content to the assistant message — third-party
//       providers require this round-trip or the next turn degrades.
//
//   turn_reasoning   : fnv1a64(content)       → reasoning_content
//       For pure-text assistant turns (no tool_calls) there's no
//       call_id to key on. We hash the assistant content and use that
//       as the lookup key, so Codex replaying the assistant turn as a
//       message item still recovers the reasoning_content.
//
// All three maps use the same bounded-FIFO eviction. The Node version
// was unbounded because the launcher lived only for one Codex session;
// the Rust proxy lives as long as the Tauri app (potentially days), so
// we cap each map at 512 entries and evict the oldest insertion. That
// works out to roughly 5-10 MB of resident memory on a long session —
// not LRU but cheaper to implement and fine for the access pattern
// (Codex almost always asks for the most recent response).

use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// Maximum entries kept in each of the three internal maps. When the
/// cap is reached, the oldest insertion is evicted. See module-level
/// comment for the rationale.
const MAP_CAPACITY: usize = 512;

// FNV-1a-64 over a string. Used to fingerprint assistant message
// content for the turn_reasoning index. Collisions just mean a missed
// reasoning lookup, not incorrect data — 64 bits is far overkill for
// the working-set size we're dealing with (a few hundred turns).
fn fnv1a_64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// Flatten Responses-shape content into a string for hashing. Handles:
//   • plain string → returned as-is
//   • array of typed parts → text fields concatenated, non-text
//     parts dropped (matches content_mapper text-only behavior)
//   • anything else → empty string (lookup returns None)
fn content_to_string(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|v| v.as_str()))
            .collect::<String>();
    }
    String::new()
}

#[derive(Default)]
struct Inner {
    response_history: HashMap<String, Vec<Value>>,
    response_history_order: VecDeque<String>,
    reasoning: HashMap<String, String>,
    reasoning_order: VecDeque<String>,
    turn_reasoning: HashMap<u64, String>,
    turn_reasoning_order: VecDeque<u64>,
}

#[derive(Clone, Default)]
pub struct SessionStore {
    inner: Arc<Mutex<Inner>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// History stashed under previous_response_id by past streaming /
    /// non-stream completion calls. Returns an empty vec if the id is
    /// unknown (which is the steady-state for the first turn of any
    /// conversation, and also for proxy restarts).
    pub fn get_history(&self, response_id: &str) -> Vec<Value> {
        if response_id.is_empty() {
            return Vec::new();
        }
        let inner = self.inner.lock().expect("session store mutex poisoned");
        inner
            .response_history
            .get(response_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Save the final assembled history under a response_id so a later
    /// request with `previous_response_id: <that_id>` replays it. Empty
    /// id or empty message list is a no-op (matches Node behavior — JS
    /// rejected non-arrays similarly).
    pub fn save_history(&self, response_id: &str, messages: Vec<Value>) {
        if response_id.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().expect("session store mutex poisoned");
        let key = response_id.to_string();
        // If the key already exists, drop its old position from the
        // eviction queue so we don't double-count.
        if inner.response_history.contains_key(&key) {
            inner.response_history_order.retain(|k| k != &key);
        }
        inner.response_history.insert(key.clone(), messages);
        inner.response_history_order.push_back(key);
        while inner.response_history_order.len() > MAP_CAPACITY {
            if let Some(evict) = inner.response_history_order.pop_front() {
                inner.response_history.remove(&evict);
            }
        }
    }

    /// Reasoning content indexed by tool-call id. Returns None if the
    /// call_id wasn't seen on a prior turn (first turn of a conversation,
    /// or proxy restarted between turns).
    pub fn get_reasoning(&self, call_id: &str) -> Option<String> {
        if call_id.is_empty() {
            return None;
        }
        let inner = self.inner.lock().expect("session store mutex poisoned");
        inner.reasoning.get(call_id).cloned()
    }

    /// Store reasoning content under a tool-call id. Empty id or empty
    /// content is a no-op (matches Node behavior, prevents the maps
    /// from filling with junk if upstream emits empty reasoning).
    pub fn store_reasoning(&self, call_id: &str, content: &str) {
        if call_id.is_empty() || content.is_empty() {
            return;
        }
        let mut inner = self.inner.lock().expect("session store mutex poisoned");
        let key = call_id.to_string();
        if inner.reasoning.contains_key(&key) {
            inner.reasoning_order.retain(|k| k != &key);
        }
        inner.reasoning.insert(key.clone(), content.to_string());
        inner.reasoning_order.push_back(key);
        while inner.reasoning_order.len() > MAP_CAPACITY {
            if let Some(evict) = inner.reasoning_order.pop_front() {
                inner.reasoning.remove(&evict);
            }
        }
    }

    /// Reasoning content indexed by assistant message content (turn
    /// fingerprint). Used when Codex replays a prior turn as a
    /// `message` item rather than as a `reasoning` item.
    pub fn get_turn_reasoning(&self, content: &Value) -> Option<String> {
        let key_str = content_to_string(content);
        if key_str.is_empty() {
            return None;
        }
        let key = fnv1a_64(&key_str);
        let inner = self.inner.lock().expect("session store mutex poisoned");
        inner.turn_reasoning.get(&key).cloned()
    }

    /// Store reasoning under a turn-content fingerprint. Empty content
    /// or empty reasoning is a no-op.
    pub fn store_turn_reasoning(&self, content: &Value, reasoning: &str) {
        let key_str = content_to_string(content);
        if key_str.is_empty() || reasoning.is_empty() {
            return;
        }
        let key = fnv1a_64(&key_str);
        let mut inner = self.inner.lock().expect("session store mutex poisoned");
        if inner.turn_reasoning.contains_key(&key) {
            inner.turn_reasoning_order.retain(|k| *k != key);
        }
        inner.turn_reasoning.insert(key, reasoning.to_string());
        inner.turn_reasoning_order.push_back(key);
        while inner.turn_reasoning_order.len() > MAP_CAPACITY {
            if let Some(evict) = inner.turn_reasoning_order.pop_front() {
                inner.turn_reasoning.remove(&evict);
            }
        }
    }

    /// Generate a new `resp_xxxxx` id for an outgoing Responses-API stream.
    /// Format: `resp_` + 16 lowercase alphanumerics (matches the regex
    /// `/^resp_[a-z0-9]+$/` that Codex client and our test suite expect).
    pub fn new_response_id(&self) -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let suffix: String = (0..16)
            .map(|_| {
                let n: u8 = rng.gen_range(0..36);
                if n < 10 {
                    (b'0' + n) as char
                } else {
                    (b'a' + (n - 10)) as char
                }
            })
            .collect();
        format!("resp_{suffix}")
    }
}

// ---------------------------------------------------------------------------
// Tests — mirror tools/codex/lib/__tests__/session-store.test.js 1:1
// where the public API maps directly. Plus eviction-cap tests that the
// JS version doesn't need.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- reasoning storage ----

    #[test]
    fn stores_and_retrieves_reasoning_by_call_id() {
        let s = SessionStore::new();
        s.store_reasoning("call_123", "Let me think about this...");
        assert_eq!(
            s.get_reasoning("call_123"),
            Some("Let me think about this...".to_string())
        );
    }

    #[test]
    fn returns_none_for_non_existent_call_id() {
        let s = SessionStore::new();
        assert_eq!(s.get_reasoning("call_nonexistent"), None);
    }

    #[test]
    fn does_not_store_reasoning_with_empty_call_id() {
        let s = SessionStore::new();
        s.store_reasoning("", "Some reasoning");
        assert_eq!(s.get_reasoning(""), None);
    }

    #[test]
    fn does_not_store_reasoning_with_empty_text() {
        let s = SessionStore::new();
        s.store_reasoning("call_123", "");
        assert_eq!(s.get_reasoning("call_123"), None);
    }

    #[test]
    fn overwrites_existing_reasoning_for_same_call_id() {
        let s = SessionStore::new();
        s.store_reasoning("call_123", "First reasoning");
        s.store_reasoning("call_123", "Second reasoning");
        assert_eq!(
            s.get_reasoning("call_123"),
            Some("Second reasoning".to_string())
        );
    }

    // ---- turn reasoning storage ----

    #[test]
    fn stores_and_retrieves_turn_reasoning_by_content_fingerprint() {
        let s = SessionStore::new();
        let content = json!("The answer is 42.");
        s.store_turn_reasoning(&content, "Let me calculate...");
        assert_eq!(
            s.get_turn_reasoning(&content),
            Some("Let me calculate...".to_string())
        );
    }

    #[test]
    fn handles_array_content_with_text_parts() {
        let s = SessionStore::new();
        let content = json!([
            { "type": "text", "text": "Hello " },
            { "type": "text", "text": "world" },
        ]);
        s.store_turn_reasoning(&content, "Greeting reasoning");
        assert_eq!(
            s.get_turn_reasoning(&content),
            Some("Greeting reasoning".to_string())
        );
    }

    #[test]
    fn turn_reasoning_returns_none_for_empty_content() {
        let s = SessionStore::new();
        assert_eq!(s.get_turn_reasoning(&json!("")), None);
        assert_eq!(s.get_turn_reasoning(&Value::Null), None);
    }

    #[test]
    fn turn_reasoning_returns_none_for_non_existent_content() {
        let s = SessionStore::new();
        assert_eq!(s.get_turn_reasoning(&json!("Never stored this")), None);
    }

    #[test]
    fn turn_reasoning_uses_content_fingerprint_for_lookup() {
        // Two separately constructed values with identical content
        // must hash to the same key.
        let s = SessionStore::new();
        let content1 = json!("Same text");
        let content2 = json!("Same text");
        s.store_turn_reasoning(&content1, "Reasoning for this text");
        assert_eq!(
            s.get_turn_reasoning(&content2),
            Some("Reasoning for this text".to_string())
        );
    }

    #[test]
    fn different_content_produces_different_fingerprints() {
        let s = SessionStore::new();
        s.store_turn_reasoning(&json!("Content A"), "Reasoning A");
        s.store_turn_reasoning(&json!("Content B"), "Reasoning B");
        assert_eq!(
            s.get_turn_reasoning(&json!("Content A")),
            Some("Reasoning A".to_string())
        );
        assert_eq!(
            s.get_turn_reasoning(&json!("Content B")),
            Some("Reasoning B".to_string())
        );
    }

    #[test]
    fn turn_reasoning_does_not_store_empty_reasoning() {
        let s = SessionStore::new();
        s.store_turn_reasoning(&json!("Some content"), "");
        assert_eq!(s.get_turn_reasoning(&json!("Some content")), None);
    }

    // ---- history storage ----

    #[test]
    fn stores_and_retrieves_message_history() {
        let s = SessionStore::new();
        let messages = vec![
            json!({ "role": "user", "content": "Hello" }),
            json!({ "role": "assistant", "content": "Hi there!" }),
        ];
        s.save_history("resp_abc", messages.clone());
        assert_eq!(s.get_history("resp_abc"), messages);
    }

    #[test]
    fn returns_empty_vec_for_non_existent_response_id() {
        let s = SessionStore::new();
        assert!(s.get_history("resp_nonexistent").is_empty());
    }

    #[test]
    fn does_not_store_history_with_empty_response_id() {
        let s = SessionStore::new();
        s.save_history("", vec![json!({ "role": "user", "content": "Hello" })]);
        assert!(s.get_history("").is_empty());
    }

    #[test]
    fn overwrites_existing_history_for_same_response_id() {
        let s = SessionStore::new();
        s.save_history(
            "resp_abc",
            vec![json!({ "role": "user", "content": "First" })],
        );
        s.save_history(
            "resp_abc",
            vec![json!({ "role": "user", "content": "Second" })],
        );
        let result = s.get_history("resp_abc");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["content"], "Second");
    }

    #[test]
    fn stores_complex_message_structures() {
        let s = SessionStore::new();
        let messages = vec![
            json!({ "role": "user", "content": "What's the weather?" }),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": { "name": "get_weather", "arguments": "{\"location\":\"Tokyo\"}" },
                }],
                "reasoning_content": "I need to check the weather API",
            }),
            json!({
                "role": "tool",
                "tool_call_id": "call_123",
                "content": "{\"temperature\":20,\"condition\":\"sunny\"}",
            }),
            json!({ "role": "assistant", "content": "It's 20°C and sunny in Tokyo." }),
        ];
        s.save_history("resp_complex", messages.clone());
        assert_eq!(s.get_history("resp_complex"), messages);
    }

    // ---- response ID generation ----

    #[test]
    fn generates_response_ids_with_correct_prefix() {
        let s = SessionStore::new();
        let id = s.new_response_id();
        assert!(id.starts_with("resp_"), "got: {id}");
        // All suffix chars must be lowercase alphanumeric.
        let suffix = &id[5..];
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()),
            "got: {suffix}"
        );
    }

    #[test]
    fn generates_unique_response_ids() {
        let s = SessionStore::new();
        let id1 = s.new_response_id();
        let id2 = s.new_response_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn generates_response_ids_of_reasonable_length() {
        let s = SessionStore::new();
        let id = s.new_response_id();
        assert!(id.len() > 10 && id.len() < 25, "got: {id} len={}", id.len());
    }

    // ---- integration scenarios ----

    #[test]
    fn handles_multi_turn_conversation_with_reasoning() {
        let s = SessionStore::new();
        let turn1 = vec![
            json!({ "role": "user", "content": "What's 2+2?" }),
            json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{ "id": "call_calc", "type": "function",
                                 "function": { "name": "calculate", "arguments": "{\"expr\":\"2+2\"}" } }],
                "reasoning_content": "I'll use the calculator",
            }),
        ];
        s.store_reasoning("call_calc", "I'll use the calculator");
        s.save_history("resp_turn1", turn1.clone());

        let mut turn2 = turn1.clone();
        turn2.push(json!({ "role": "tool", "tool_call_id": "call_calc", "content": "4" }));
        turn2.push(json!({ "role": "assistant", "content": "The answer is 4." }));
        s.store_turn_reasoning(&json!("The answer is 4."), "Simple arithmetic");
        s.save_history("resp_turn2", turn2.clone());

        assert_eq!(
            s.get_reasoning("call_calc"),
            Some("I'll use the calculator".to_string())
        );
        assert_eq!(
            s.get_turn_reasoning(&json!("The answer is 4.")),
            Some("Simple arithmetic".to_string())
        );
        assert_eq!(s.get_history("resp_turn1").len(), 2);
        assert_eq!(s.get_history("resp_turn2").len(), 4);
    }

    #[test]
    fn handles_conversation_continuation_via_previous_response_id() {
        let s = SessionStore::new();
        let initial = vec![
            json!({ "role": "user", "content": "Hello" }),
            json!({ "role": "assistant", "content": "Hi! How can I help?" }),
        ];
        s.save_history("resp_001", initial.clone());

        // Continuation pattern — Codex sends previous_response_id, our
        // proxy fetches the stored history and extends it.
        let previous = s.get_history("resp_001");
        let mut continued = previous;
        continued.push(json!({ "role": "user", "content": "Tell me a joke" }));
        continued.push(json!({
            "role": "assistant",
            "content": "Why did the chicken cross the road?",
        }));
        s.save_history("resp_002", continued);

        let result = s.get_history("resp_002");
        assert_eq!(result.len(), 4);
        assert_eq!(result[0]["content"], "Hello");
        assert_eq!(result[3]["content"], "Why did the chicken cross the road?");
    }

    // ---- bounded-FIFO eviction (Rust-only, no JS analog) ----

    #[test]
    fn history_evicts_oldest_when_over_cap() {
        let s = SessionStore::new();
        // Fill cap + a few extra. Oldest entries must vanish.
        for i in 0..(MAP_CAPACITY + 5) {
            s.save_history(&format!("resp_{i:04}"), vec![json!({ "i": i })]);
        }
        // First 5 should be gone.
        for i in 0..5 {
            assert!(
                s.get_history(&format!("resp_{i:04}")).is_empty(),
                "resp_{i:04} should have been evicted"
            );
        }
        // Last entry must still be there.
        assert_eq!(
            s.get_history(&format!("resp_{:04}", MAP_CAPACITY + 4))
                .len(),
            1
        );
    }

    #[test]
    fn history_overwrite_does_not_double_count_against_cap() {
        let s = SessionStore::new();
        // Overwrite the same key many times — should never evict anything.
        for i in 0..(MAP_CAPACITY * 2) {
            s.save_history("resp_x", vec![json!({ "i": i })]);
        }
        let result = s.get_history("resp_x");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["i"], (MAP_CAPACITY * 2 - 1) as i64);
    }

    #[test]
    fn reasoning_evicts_oldest_when_over_cap() {
        let s = SessionStore::new();
        for i in 0..(MAP_CAPACITY + 3) {
            s.store_reasoning(&format!("call_{i:04}"), &format!("r{i}"));
        }
        // Oldest 3 should be evicted.
        for i in 0..3 {
            assert_eq!(s.get_reasoning(&format!("call_{i:04}")), None);
        }
        // Newest one survives.
        assert_eq!(
            s.get_reasoning(&format!("call_{:04}", MAP_CAPACITY + 2)),
            Some(format!("r{}", MAP_CAPACITY + 2))
        );
    }

    #[test]
    fn turn_reasoning_evicts_oldest_when_over_cap() {
        let s = SessionStore::new();
        for i in 0..(MAP_CAPACITY + 3) {
            s.store_turn_reasoning(&json!(format!("content {i}")), &format!("r{i}"));
        }
        for i in 0..3 {
            assert_eq!(s.get_turn_reasoning(&json!(format!("content {i}"))), None);
        }
        assert_eq!(
            s.get_turn_reasoning(&json!(format!("content {}", MAP_CAPACITY + 2))),
            Some(format!("r{}", MAP_CAPACITY + 2))
        );
    }

    #[test]
    fn fnv1a_is_deterministic_and_distinct() {
        // Sanity-check the hash directly. Same input → same output;
        // different inputs almost certainly differ.
        assert_eq!(fnv1a_64("hello"), fnv1a_64("hello"));
        assert_ne!(fnv1a_64("hello"), fnv1a_64("world"));
        // Known FNV-1a-64 of "hello" (regression vector).
        assert_eq!(fnv1a_64("hello"), 0xa430d84680aabd0b);
    }

    #[test]
    fn content_to_string_handles_strings_and_arrays() {
        assert_eq!(content_to_string(&json!("plain")), "plain");
        assert_eq!(
            content_to_string(&json!([
                { "type": "text", "text": "a" },
                { "type": "image", "image_url": "ignored" },
                { "type": "text", "text": "b" },
            ])),
            "ab"
        );
        assert_eq!(content_to_string(&Value::Null), "");
        assert_eq!(content_to_string(&json!(42)), "");
    }
}
