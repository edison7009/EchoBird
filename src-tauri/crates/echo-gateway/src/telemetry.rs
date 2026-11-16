//! What happened, as it happens: every attempt, every answer, every plan change. The engine and the planner
//! emit events to a `Sink`; the app decides where they go (the bar's live tab, a database for the scorer). An
//! event carries numbers and names only, never what anyone said.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// A round began: which route and zone took the sentence, and the app it came from.
    Round {
        id: u64,
        route: String,
        zone: String,
        app: String,
        internal: bool,
    },
    /// One try at a channel, and how it ended before the answer started.
    Attempt {
        round: u64,
        n: u32,
        channel: String,
        model: String,
        ok: bool,
        status: u16,
        /// Milliseconds to the first byte, or to the failure.
        ms: u64,
        why: String,
    },
    /// A new plan is in use.
    Plan {
        version: u64,
        by: String,
        changes: Vec<String>,
    },
}

/// Where events go. Must not block: it is called on the request path.
pub trait Sink: Send + Sync {
    fn emit(&self, event: Event);
}

/// Drops everything (tests, or before the app has wired a sink).
pub struct Discard;

impl Sink for Discard {
    fn emit(&self, _: Event) {}
}

/// Keeps the last `capacity` events in memory, for the live tab to read on open and the scorer to sample.
pub struct Ring {
    capacity: usize,
    events: Mutex<VecDeque<Event>>,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            events: Mutex::new(VecDeque::with_capacity(capacity)),
        }
    }

    pub fn recent(&self) -> Vec<Event> {
        self.events
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}

impl Sink for Ring {
    fn emit(&self, event: Event) {
        let mut events = self.events.lock().unwrap_or_else(|p| p.into_inner());
        if events.len() == self.capacity {
            events.pop_front();
        }
        events.push_back(event);
    }
}

/// Sends every event to several sinks.
pub struct Fanout(pub Vec<std::sync::Arc<dyn Sink>>);

impl Sink for Fanout {
    fn emit(&self, event: Event) {
        for sink in &self.0 {
            sink.emit(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ring_keeps_only_the_newest() {
        let ring = Ring::new(2);
        for version in 1..=3 {
            ring.emit(Event::Plan {
                version,
                by: "scorer".into(),
                changes: vec![],
            });
        }
        let versions: Vec<u64> = ring
            .recent()
            .into_iter()
            .map(|e| {
                if let Event::Plan { version, .. } = e {
                    version
                } else {
                    0
                }
            })
            .collect();
        assert_eq!(versions, vec![2, 3]);
    }

    #[test]
    fn events_serialize_with_their_kind() {
        let text = serde_json::to_string(&Event::Round {
            id: 1,
            route: "auto".into(),
            zone: "code".into(),
            app: "claude-code".into(),
            internal: false,
        })
        .unwrap();
        assert!(text.starts_with(r#"{"kind":"round""#));
    }
}
