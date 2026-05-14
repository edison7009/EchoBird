// Phase 4 stub — protocol_converter and stream_handler depend on a
// SessionStore for response_id history replay + reasoning_content
// round-trip. The full implementation (in-memory maps + per-response
// dedup + content-fingerprint reasoning index) lands in Phase 4 along
// with the test coverage of session-store.test.js (22 tests). For now
// every method returns the "nothing cached" branch — protocol_converter
// degrades gracefully to "use the input items array verbatim" and
// reasoning_content just doesn't get round-tripped for the first turn
// after a proxy restart (acceptable since the Node launcher is still
// serving live traffic in v4.6.8).

use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub struct SessionStore {
    // Real fields land in Phase 4.
    _inner: Arc<Mutex<()>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// History stashed under previous_response_id by past `chatStreamToResponsesStream`
    /// / `chatToResponsesNonStream` calls. Phase 4 will return real
    /// `Vec<serde_json::Value>` clones; today returns empty.
    pub fn get_history(&self, _response_id: &str) -> Vec<Value> {
        Vec::new()
    }

    /// Reasoning content indexed by tool-call id. Used to recover
    /// reasoning_content on assistant.tool_calls turns for thinking models.
    pub fn get_reasoning(&self, _call_id: &str) -> Option<String> {
        None
    }

    /// Store reasoning content under a tool-call id. Phase 4 will persist
    /// to an LRU map; today no-op.
    pub fn store_reasoning(&self, _call_id: &str, _content: &str) {}

    /// Reasoning content indexed by assistant message content (turn
    /// fingerprint). Used when Codex replays a prior turn as a
    /// `message` item rather than as a `reasoning` item.
    pub fn get_turn_reasoning(&self, _content: &Value) -> Option<String> {
        None
    }

    /// Store reasoning under a turn-content fingerprint.
    pub fn store_turn_reasoning(&self, _content: &Value, _reasoning: &str) {}

    /// Generate a new `resp_xxxxx` id for an outgoing Responses-API stream.
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
        format!("resp_{}", suffix)
    }

    /// Save the final assembled history under a response_id so a later
    /// request with `previous_response_id: <that_id>` can replay it.
    pub fn save_history(&self, _response_id: &str, _messages: Vec<Value>) {}
}
