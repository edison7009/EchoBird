//! Where the plan in use lives. The gateway reads it on every request; the scorer and the helper publish new
//! ones beside it. Reading is one lock-free-in-practice `Arc` clone; publishing checks the plan first, keeps
//! the user's pins, limits how often a route may change, and keeps the last few plans for rolling back.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::plan::{Author, Known, Plan, Problem};

/// How many earlier plans are kept for rolling back.
const HISTORY: usize = 8;

/// Why a plan was not published.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    Invalid(Vec<Problem>),
    /// Something other than the user changed plans too recently; the plan in use stays.
    TooSoon {
        wait: Duration,
    },
}

pub struct PlanStore {
    current: RwLock<Arc<Plan>>,
    inner: Mutex<Inner>,
    /// The least time between two plans from the scorer or the helper. The user is never held back.
    min_gap: Duration,
}

struct Inner {
    history: VecDeque<Arc<Plan>>,
    last_publish: Option<Instant>,
}

impl PlanStore {
    /// Starts from a plan (normally the built-in default). It is not checked: it is what there is.
    pub fn new(mut first: Plan, min_gap: Duration) -> Self {
        first.version = first.version.max(1);
        first.tunables.clamp();
        Self {
            current: RwLock::new(Arc::new(first)),
            inner: Mutex::new(Inner {
                history: VecDeque::new(),
                last_publish: None,
            }),
            min_gap,
        }
    }

    /// The plan in use. Cheap: callers may take it on every request.
    pub fn current(&self) -> Arc<Plan> {
        self.current
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// Checks `next`, keeps the user's pinned tunables, clamps the rest, and makes it the plan in use. Returns
    /// the version it was given.
    pub fn publish(&self, mut next: Plan, known: &Known, now: Instant) -> Result<u64, Refused> {
        next.validate(known).map_err(Refused::Invalid)?;
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if next.by != Author::User {
            if let Some(last) = inner.last_publish {
                let since = now.saturating_duration_since(last);
                if since < self.min_gap {
                    return Err(Refused::TooSoon {
                        wait: self.min_gap - since,
                    });
                }
            }
        }
        let previous = self.current();
        next.tunables = previous.tunables.merged_keeping_pins(next.tunables);
        next.tunables.clamp();
        next.version = previous.version + 1;
        let version = next.version;
        inner.history.push_back(previous);
        while inner.history.len() > HISTORY {
            inner.history.pop_front();
        }
        inner.last_publish = Some(now);
        *self
            .current
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = Arc::new(next);
        Ok(version)
    }

    /// Goes back to the plan before the current one (when the new one made things worse). The plan put back
    /// gets a new version number, so everyone can tell it changed. Returns that version, or `None` when there
    /// is nothing to go back to.
    pub fn roll_back(&self) -> Option<u64> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let back = inner.history.pop_back()?;
        let mut plan = (*back).clone();
        plan.version = self.current().version + 1;
        let version = plan.version;
        *self
            .current
            .write()
            .unwrap_or_else(|poison| poison.into_inner()) = Arc::new(plan);
        Some(version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Candidate, Route, Share, Tunables, Zone};

    fn known() -> Known {
        Known {
            channels: ["claude", "glm"].map(String::from).into(),
            models: ["claude/opus", "glm/flash"].map(String::from).into(),
            ..Known::default()
        }
    }

    fn plan(channel: &str, model: &str, by: Author) -> Plan {
        Plan {
            version: 0,
            routes: vec![Route {
                id: "auto".into(),
                zones: vec![Zone {
                    id: "code".into(),
                    when: vec![],
                    share: Share::InOrder,
                    candidates: vec![Candidate {
                        model: model.into(),
                        channels: vec![channel.into()],
                    }],
                }],
            }],
            tunables: Tunables::default(),
            changes: vec![],
            by,
        }
    }

    fn first(store: &PlanStore) -> String {
        store.current().routes[0].zones[0].candidates[0].channels[0].clone()
    }

    #[test]
    fn a_valid_plan_is_published_with_the_next_version() {
        let store = PlanStore::new(plan("claude", "opus", Author::Default), Duration::ZERO);
        let now = Instant::now();
        assert_eq!(
            store.publish(plan("glm", "flash", Author::Scorer), &known(), now),
            Ok(2)
        );
        assert_eq!(first(&store), "glm");
    }

    #[test]
    fn an_invalid_plan_leaves_the_one_in_use() {
        let store = PlanStore::new(plan("claude", "opus", Author::Default), Duration::ZERO);
        let refused = store.publish(
            plan("nowhere", "opus", Author::Helper),
            &known(),
            Instant::now(),
        );
        assert!(matches!(refused, Err(Refused::Invalid(_))));
        assert_eq!(
            (store.current().version, first(&store).as_str()),
            (1, "claude")
        );
    }

    #[test]
    fn the_helper_cannot_flip_plans_faster_than_the_gap_but_the_user_can() {
        let store = PlanStore::new(
            plan("claude", "opus", Author::Default),
            Duration::from_secs(600),
        );
        let now = Instant::now();
        assert_eq!(
            store.publish(plan("glm", "flash", Author::Helper), &known(), now),
            Ok(2)
        );
        let soon = now + Duration::from_secs(60);
        assert_eq!(
            store.publish(plan("claude", "opus", Author::Helper), &known(), soon),
            Err(Refused::TooSoon {
                wait: Duration::from_secs(540)
            })
        );
        assert_eq!(
            store.publish(plan("claude", "opus", Author::User), &known(), soon),
            Ok(3)
        );
    }

    #[test]
    fn pins_survive_and_values_are_clamped_on_publish() {
        let mut start = plan("claude", "opus", Author::Default);
        start.tunables.first_token_ms = 20_000;
        start.tunables.pinned.insert("first_token_ms".into());
        let store = PlanStore::new(start, Duration::ZERO);
        let mut next = plan("glm", "flash", Author::Helper);
        next.tunables.first_token_ms = 120_000;
        next.tunables.judge_timeout_ms = 1;
        store.publish(next, &known(), Instant::now()).unwrap();
        let tunables = &store.current().tunables;
        assert_eq!(
            (tunables.first_token_ms, tunables.judge_timeout_ms),
            (20_000, crate::plan::JUDGE_TIMEOUT_MS.0)
        );
    }

    #[test]
    fn rolling_back_restores_the_plan_before_under_a_new_version() {
        let store = PlanStore::new(plan("claude", "opus", Author::Default), Duration::ZERO);
        store
            .publish(
                plan("glm", "flash", Author::Helper),
                &known(),
                Instant::now(),
            )
            .unwrap();
        assert_eq!(store.roll_back(), Some(3));
        assert_eq!(first(&store), "claude");
        assert_eq!(store.roll_back(), None);
    }
}
