//! The routing plan: what the gateway does, written as data.
//!
//! Nobody configures routing by hand. A plan is produced by the default rules, then re-made by the scorer and
//! the helper (an AI) as channels slow down, run out or fail their health check. Because it is data, a plan can
//! be checked before it is used, compared with the one before, explained, and rolled back. The gateway only
//! ever reads a compiled snapshot of it (see `store`).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Who made a plan, or a change to one. Shown to the user next to the reason.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    /// The built-in rules, before anything was learned.
    Default,
    /// The deterministic scorer, from measured speed, allowance, cache and health.
    Scorer,
    /// The helper's model, reviewing the scorer's draft.
    Helper,
    /// The user pinned it; nobody else changes it.
    User,
}

/// What puts a sentence in a zone. Cheap facts first; `Intent` needs the judging model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Condition {
    HasImage,
    /// The prompt is at least this many tokens.
    TokensAtLeast(u32),
    /// The app asked for at least this reasoning effort (`low` < `medium` < `high` < `max`).
    EffortAtLeast(String),
    /// The request comes from this app (its key, e.g. `claude-code`).
    App(String),
    /// The judging model says the sentence is this kind of work.
    Intent(String),
}

impl Condition {
    /// Conditions that need the judging model (and so a little time and, at most, one cheap call).
    pub fn needs_judge(&self) -> bool {
        matches!(self, Condition::Intent(_))
    }
}

/// How a zone shares its work among its candidates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Share {
    /// The first that can answer, then the next.
    #[default]
    InOrder,
    /// Plenty left first; what resets soonest is used before it is wasted.
    Smart,
    /// A different candidate for each sentence the user says.
    Rotate,
    /// Free first; paid only when no free one can answer.
    FreeFirst,
}

/// One model in a zone and the channels that serve it, in the order they are tried. One model, however many
/// channels: the plan says "Opus 5.5", the channels say where it can be had.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    pub model: String,
    pub channels: Vec<String>,
}

/// A kind of work and who does it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    pub id: String,
    /// Any one is enough. Empty: everything no earlier zone took.
    #[serde(default)]
    pub when: Vec<Condition>,
    #[serde(default)]
    pub share: Share,
    pub candidates: Vec<Candidate>,
}

/// One way of routing everything (日常 / 省钱 / 赶时间): its zones, checked in order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub zones: Vec<Zone>,
}

/// Whether a conversation stays on the account that answered it, so the vendor's prompt cache keeps paying.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stick {
    /// While the cache is still worth keeping; always within a turn.
    #[default]
    Auto,
    Session,
    Turn,
    Off,
}

/// A rate-limited channel that still has allowance: wait in place, or go to the back of the line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimited {
    #[default]
    StayInPlace,
    ToTheBack,
}

/// What other tools leave as settings. Here they have defaults that work, and the helper tunes them within
/// bounds; the user may pin one, after which it is left alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tunables {
    pub stick: Stick,
    pub rate_limited: RateLimited,
    /// How long a channel may stay silent before the first token, before the round is switched.
    pub first_token_ms: u64,
    /// How long to wait when every candidate is busy before giving up.
    pub all_busy_ms: u64,
    /// The whole round's budget, switches included.
    pub round_budget_ms: u64,
    pub max_switches: u32,
    /// How long the judging model may take; past it, the sentence falls to the default zone.
    pub judge_timeout_ms: u64,
    /// The judging model, as `channel/model`. Empty: no intent conditions are evaluated.
    pub judge: String,
    /// Names of tunables the user pinned.
    pub pinned: BTreeSet<String>,
}

impl Default for Tunables {
    fn default() -> Self {
        Self {
            stick: Stick::Auto,
            rate_limited: RateLimited::StayInPlace,
            first_token_ms: 60_000,
            all_busy_ms: 60_000,
            // EchoBird's routing budget and attempt limit.
            round_budget_ms: 45_000,
            max_switches: 20,
            judge_timeout_ms: 800,
            judge: String::new(),
            pinned: BTreeSet::new(),
        }
    }
}

/// The bounds a tunable is kept in, whoever sets it.
pub const FIRST_TOKEN_MS: (u64, u64) = (5_000, 300_000);
pub const ALL_BUSY_MS: (u64, u64) = (0, 300_000);
pub const ROUND_BUDGET_MS: (u64, u64) = (10_000, 600_000);
pub const MAX_SWITCHES: (u32, u32) = (1, 50);
pub const JUDGE_TIMEOUT_MS: (u64, u64) = (200, 5_000);

impl Tunables {
    /// Brings every value into its bounds. A plan from the helper is clamped, never rejected, for this.
    pub fn clamp(&mut self) {
        self.first_token_ms = self
            .first_token_ms
            .clamp(FIRST_TOKEN_MS.0, FIRST_TOKEN_MS.1);
        self.all_busy_ms = self.all_busy_ms.clamp(ALL_BUSY_MS.0, ALL_BUSY_MS.1);
        self.round_budget_ms = self
            .round_budget_ms
            .clamp(ROUND_BUDGET_MS.0, ROUND_BUDGET_MS.1);
        self.max_switches = self.max_switches.clamp(MAX_SWITCHES.0, MAX_SWITCHES.1);
        self.judge_timeout_ms = self
            .judge_timeout_ms
            .clamp(JUDGE_TIMEOUT_MS.0, JUDGE_TIMEOUT_MS.1);
    }

    /// Takes `next` but keeps every value the user pinned in `self`.
    pub fn merged_keeping_pins(&self, mut next: Tunables) -> Tunables {
        let pinned = &self.pinned;
        if pinned.contains("stick") {
            next.stick = self.stick;
        }
        if pinned.contains("rate_limited") {
            next.rate_limited = self.rate_limited;
        }
        if pinned.contains("first_token_ms") {
            next.first_token_ms = self.first_token_ms;
        }
        if pinned.contains("all_busy_ms") {
            next.all_busy_ms = self.all_busy_ms;
        }
        if pinned.contains("round_budget_ms") {
            next.round_budget_ms = self.round_budget_ms;
        }
        if pinned.contains("max_switches") {
            next.max_switches = self.max_switches;
        }
        if pinned.contains("judge_timeout_ms") {
            next.judge_timeout_ms = self.judge_timeout_ms;
        }
        if pinned.contains("judge") {
            next.judge.clone_from(&self.judge);
        }
        next.pinned.clone_from(pinned);
        next
    }
}

/// One change to a plan, in the helper's words: what it noticed, then what it did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub route: String,
    pub saw: String,
    pub did: String,
    pub by: Author,
}

/// Everything the gateway needs to route. `version` only goes up; the store assigns it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub version: u64,
    pub routes: Vec<Route>,
    #[serde(default)]
    pub tunables: Tunables,
    /// What changed since the plan before, and why.
    #[serde(default)]
    pub changes: Vec<Change>,
    pub by: Author,
}

/// What the plan may refer to: the channels and models that exist, the ones out of service, and which are free.
#[derive(Clone, Debug, Default)]
pub struct Known {
    pub channels: BTreeSet<String>,
    /// `channel/model` pairs the channel serves.
    pub models: BTreeSet<String>,
    /// Channels that failed their health check: no plan may send work to them.
    pub disabled: BTreeSet<String>,
    /// `channel/model` pairs that cost nothing.
    pub free: BTreeSet<String>,
}

/// Why a plan cannot be used. Each names where the problem is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NoRoutes,
    EmptyZone {
        route: String,
        zone: String,
    },
    UnknownChannel {
        route: String,
        zone: String,
        channel: String,
    },
    UnknownModel {
        route: String,
        zone: String,
        model: String,
        channel: String,
    },
    DisabledChannel {
        route: String,
        zone: String,
        channel: String,
    },
    /// The `free` route sends work to something that costs.
    PaidInFree {
        zone: String,
        model: String,
        channel: String,
    },
    DuplicateRoute(String),
    DuplicateZone {
        route: String,
        zone: String,
    },
}

/// The route that may only use what costs nothing.
pub const FREE_ROUTE: &str = "free";

impl Plan {
    /// Checks a plan against what exists. A plan with any problem is not published: the last good one stays.
    pub fn validate(&self, known: &Known) -> Result<(), Vec<Problem>> {
        let mut problems = Vec::new();
        if self.routes.is_empty() {
            problems.push(Problem::NoRoutes);
        }
        let mut routes = BTreeSet::new();
        for route in &self.routes {
            if !routes.insert(&route.id) {
                problems.push(Problem::DuplicateRoute(route.id.clone()));
            }
            let mut zones = BTreeSet::new();
            for zone in &route.zones {
                let at =
                    |f: &dyn Fn(String, String) -> Problem| f(route.id.clone(), zone.id.clone());
                if !zones.insert(&zone.id) {
                    problems.push(at(&|route, zone| Problem::DuplicateZone { route, zone }));
                }
                if zone.candidates.iter().all(|c| c.channels.is_empty()) {
                    problems.push(at(&|route, zone| Problem::EmptyZone { route, zone }));
                }
                for candidate in &zone.candidates {
                    for channel in &candidate.channels {
                        let pair = format!("{channel}/{}", candidate.model);
                        if !known.channels.contains(channel) {
                            problems.push(Problem::UnknownChannel {
                                route: route.id.clone(),
                                zone: zone.id.clone(),
                                channel: channel.clone(),
                            });
                        } else if known.disabled.contains(channel) {
                            problems.push(Problem::DisabledChannel {
                                route: route.id.clone(),
                                zone: zone.id.clone(),
                                channel: channel.clone(),
                            });
                        } else if !known.models.contains(&pair) {
                            problems.push(Problem::UnknownModel {
                                route: route.id.clone(),
                                zone: zone.id.clone(),
                                model: candidate.model.clone(),
                                channel: channel.clone(),
                            });
                        }
                        if route.id == FREE_ROUTE && !known.free.contains(&pair) {
                            problems.push(Problem::PaidInFree {
                                zone: zone.id.clone(),
                                model: candidate.model.clone(),
                                channel: channel.clone(),
                            });
                        }
                    }
                }
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems)
        }
    }

    /// The route with this id.
    pub fn route(&self, id: &str) -> Option<&Route> {
        self.routes.iter().find(|route| route.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known() -> Known {
        Known {
            channels: ["claude", "glm", "relay"].map(String::from).into(),
            models: ["claude/opus", "glm/flash", "relay/opus"]
                .map(String::from)
                .into(),
            disabled: ["relay".to_string()].into(),
            free: ["glm/flash".to_string()].into(),
        }
    }

    fn plan(route: &str, candidates: Vec<Candidate>) -> Plan {
        Plan {
            version: 0,
            routes: vec![Route {
                id: route.into(),
                zones: vec![Zone {
                    id: "code".into(),
                    when: vec![],
                    share: Share::InOrder,
                    candidates,
                }],
            }],
            tunables: Tunables::default(),
            changes: vec![],
            by: Author::Scorer,
        }
    }

    fn one(model: &str, channel: &str) -> Candidate {
        Candidate {
            model: model.into(),
            channels: vec![channel.into()],
        }
    }

    #[test]
    fn a_plan_on_known_channels_passes() {
        assert_eq!(
            plan("auto", vec![one("opus", "claude"), one("flash", "glm")]).validate(&known()),
            Ok(())
        );
    }

    #[test]
    fn a_plan_cannot_send_work_to_a_disabled_or_unknown_channel_or_model() {
        let problems = plan(
            "auto",
            vec![
                one("opus", "relay"),
                one("opus", "nowhere"),
                one("haiku", "claude"),
            ],
        )
        .validate(&known())
        .unwrap_err();
        assert!(problems.contains(&Problem::DisabledChannel {
            route: "auto".into(),
            zone: "code".into(),
            channel: "relay".into()
        }));
        assert!(problems.contains(&Problem::UnknownChannel {
            route: "auto".into(),
            zone: "code".into(),
            channel: "nowhere".into()
        }));
        assert!(problems
            .iter()
            .any(|p| matches!(p, Problem::UnknownModel { model, .. } if model == "haiku")));
    }

    #[test]
    fn the_free_route_only_takes_what_costs_nothing() {
        assert_eq!(
            plan(FREE_ROUTE, vec![one("flash", "glm")]).validate(&known()),
            Ok(())
        );
        let problems = plan(FREE_ROUTE, vec![one("opus", "claude")])
            .validate(&known())
            .unwrap_err();
        assert_eq!(
            problems,
            vec![Problem::PaidInFree {
                zone: "code".into(),
                model: "opus".into(),
                channel: "claude".into()
            }]
        );
    }

    #[test]
    fn an_empty_plan_or_zone_is_refused() {
        let empty = Plan {
            routes: vec![],
            ..plan("auto", vec![])
        };
        assert_eq!(empty.validate(&known()), Err(vec![Problem::NoRoutes]));
        assert!(plan("auto", vec![])
            .validate(&known())
            .unwrap_err()
            .contains(&Problem::EmptyZone {
                route: "auto".into(),
                zone: "code".into()
            }));
    }

    #[test]
    fn tunables_are_clamped_and_pins_survive_a_new_plan() {
        let mut wild = Tunables {
            first_token_ms: 1,
            judge_timeout_ms: 99_999,
            max_switches: 0,
            ..Tunables::default()
        };
        wild.clamp();
        assert_eq!(
            (
                wild.first_token_ms,
                wild.judge_timeout_ms,
                wild.max_switches
            ),
            (FIRST_TOKEN_MS.0, JUDGE_TIMEOUT_MS.1, MAX_SWITCHES.0)
        );

        let mine = Tunables {
            first_token_ms: 20_000,
            pinned: ["first_token_ms".to_string()].into(),
            ..Tunables::default()
        };
        let helpers = Tunables {
            first_token_ms: 90_000,
            all_busy_ms: 5_000,
            ..Tunables::default()
        };
        let merged = mine.merged_keeping_pins(helpers);
        assert_eq!((merged.first_token_ms, merged.all_busy_ms), (20_000, 5_000));
        assert!(merged.pinned.contains("first_token_ms"));
    }

    #[test]
    fn a_plan_round_trips_through_json_the_helper_writes() {
        let text = r#"{"routes":[{"id":"auto","zones":[{"id":"image","when":[{"kind":"has_image"}],"share":"smart",
            "candidates":[{"model":"opus","channels":["claude"]}]}]}],"tunables":{"stick":"session"},"by":"helper"}"#;
        let parsed: Plan = serde_json::from_str(text).unwrap();
        assert_eq!(parsed.routes[0].zones[0].when, vec![Condition::HasImage]);
        assert_eq!(parsed.tunables.stick, Stick::Session);
        assert_eq!(
            parsed.tunables.first_token_ms,
            Tunables::default().first_token_ms
        );
        assert_eq!(parsed.by, Author::Helper);
    }
}
