//! The scorer: from what has been measured (success, speed, allowance left, health check), re-orders every zone
//! of every route, the way each route means it (日常 balanced, 省钱 free and cheap first, 赶时间 fastest first),
//! and reports each change of who answers first as a `Shift` with its cause. Deterministic and cheap: it runs
//! every minute, and most minutes nothing changes. The helper reviews a draft and phrases the causes for the
//! user; the crate stays out of wording.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::plan::{Author, Candidate, Plan};
use crate::upstream::Billing;

/// What is known of one channel serving one model.
#[derive(Clone, Debug)]
pub struct Measure {
    pub channel: String,
    pub model: String,
    pub billing: Billing,
    /// Share of recent answers that succeeded, 0–1.
    pub success: f64,
    /// Seconds to the first token, averaged; none until it has answered.
    pub first_token: Option<f64>,
    /// Share of the allowance left, 0–1; none for pay-as-you-go keys.
    pub quota_left: Option<f64>,
    /// Failed its health check.
    pub suspect: bool,
}

impl Measure {
    fn usable(&self) -> bool {
        !self.suspect && self.quota_left != Some(0.0)
    }
}

/// How a route orders its zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strategy {
    /// Keep the zone's order of models (best first); within a model, the channel most likely to answer.
    Balanced,
    /// Free first, then subscriptions, then pay-as-you-go; reliability breaks ties.
    Cheap,
    /// Fastest first token first; reliability breaks ties.
    Fast,
    /// Leave the route as it is.
    Keep,
}

impl Strategy {
    /// The strategy behind a route id.
    pub fn of(route: &str) -> Self {
        match route {
            "auto" => Strategy::Balanced,
            "save" | "free" => Strategy::Cheap,
            "fast" => Strategy::Fast,
            _ => Strategy::Keep,
        }
    }
}

/// Why a zone's first choice changed.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// The old first is out (failed health check, or no allowance left).
    Unusable { suspect: bool },
    /// The old first's allowance is running low.
    LowQuota { left: f64 },
    /// The old first keeps failing.
    Failing { success: f64 },
    /// The new first answers faster.
    Faster { seconds: f64 },
    /// The new first costs less.
    Cheaper,
    /// Anything else (a channel came back, more is known).
    Reordered,
}

/// One change of who answers first in a zone.
#[derive(Clone, Debug, PartialEq)]
pub struct Shift {
    pub route: String,
    pub zone: String,
    pub from: (String, String),
    pub to: (String, String),
    pub cause: Cause,
}

fn billing_rank(billing: Billing) -> u8 {
    match billing {
        Billing::Free => 0,
        Billing::Subscription => 1,
        Billing::PerUse => 2,
    }
}

/// How likely a channel is to answer well now: reliability, softened by how little allowance is left.
fn reliability(m: &Measure) -> f64 {
    let quota = m.quota_left.map_or(1.0, |left| (left * 4.0).min(1.0));
    m.success * quota
}

fn compare(strategy: Strategy, a: &Measure, b: &Measure) -> Ordering {
    let usable = b.usable().cmp(&a.usable());
    let reliable = || {
        reliability(b)
            .partial_cmp(&reliability(a))
            .unwrap_or(Ordering::Equal)
    };
    match strategy {
        Strategy::Keep => Ordering::Equal,
        Strategy::Balanced => usable.then_with(reliable),
        Strategy::Cheap => usable
            .then(billing_rank(a.billing).cmp(&billing_rank(b.billing)))
            .then_with(reliable),
        Strategy::Fast => usable
            .then_with(|| {
                a.first_token
                    .unwrap_or(f64::MAX)
                    .partial_cmp(&b.first_token.unwrap_or(f64::MAX))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(reliable),
    }
}

fn cause(strategy: Strategy, old: &Measure, new: &Measure) -> Cause {
    if !old.usable() {
        Cause::Unusable {
            suspect: old.suspect,
        }
    } else if old.quota_left.is_some_and(|left| left < 0.25) {
        Cause::LowQuota {
            left: old.quota_left.unwrap_or(0.0),
        }
    } else if old.success < 0.8 {
        Cause::Failing {
            success: old.success,
        }
    } else if strategy == Strategy::Fast && new.first_token.is_some() {
        Cause::Faster {
            seconds: new.first_token.unwrap_or(0.0),
        }
    } else if billing_rank(new.billing) < billing_rank(old.billing) {
        Cause::Cheaper
    } else {
        Cause::Reordered
    }
}

/// A new plan from the current one and what is known, and what changed. Channels with no measure yet keep their
/// place (they count as reliable, unknown speed). Suspect channels are left out entirely.
pub fn draft(current: &Plan, measures: &[Measure]) -> (Plan, Vec<Shift>) {
    let by_pair: HashMap<(&str, &str), &Measure> = measures
        .iter()
        .map(|m| ((m.channel.as_str(), m.model.as_str()), m))
        .collect();
    let unknown = |channel: &str, model: &str| Measure {
        channel: channel.into(),
        model: model.into(),
        billing: Billing::PerUse,
        success: 1.0,
        first_token: None,
        quota_left: None,
        suspect: false,
    };
    let measure = |channel: &str, model: &str| {
        by_pair
            .get(&(channel, model))
            .map(|m| (*m).clone())
            .unwrap_or_else(|| unknown(channel, model))
    };

    let mut plan = current.clone();
    plan.by = Author::Scorer;
    plan.changes.clear();
    let mut shifts = Vec::new();
    for route in &mut plan.routes {
        let strategy = Strategy::of(&route.id);
        if strategy == Strategy::Keep {
            continue;
        }
        for zone in &mut route.zones {
            let first_before = zone
                .candidates
                .iter()
                .find_map(|c| c.channels.first().map(|ch| (ch.clone(), c.model.clone())));
            // Within a model: its channels, best first, suspect ones dropped.
            for candidate in &mut zone.candidates {
                candidate
                    .channels
                    .retain(|ch| !measure(ch, &candidate.model).suspect);
                candidate.channels.sort_by(|a, b| {
                    compare(
                        strategy,
                        &measure(a, &candidate.model),
                        &measure(b, &candidate.model),
                    )
                });
            }
            zone.candidates.retain(|c| !c.channels.is_empty());
            // Across models: by each model's best channel (the balanced route keeps the zone's model order, only
            // moving models that cannot answer to the back).
            let best = |c: &Candidate| measure(&c.channels[0], &c.model);
            if strategy == Strategy::Balanced {
                zone.candidates.sort_by_key(|c| !best(c).usable());
            } else {
                zone.candidates
                    .sort_by(|a, b| compare(strategy, &best(a), &best(b)));
            }
            let first_after = zone
                .candidates
                .first()
                .map(|c| (c.channels[0].clone(), c.model.clone()));
            if let (Some(from), Some(to)) = (first_before, first_after) {
                if from != to {
                    let cause = cause(strategy, &measure(&from.0, &from.1), &measure(&to.0, &to.1));
                    shifts.push(Shift {
                        route: route.id.clone(),
                        zone: zone.id.clone(),
                        from,
                        to,
                        cause,
                    });
                }
            }
        }
    }
    (plan, shifts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Route, Share, Tunables, Zone};

    fn plan(route: &str, candidates: &[(&str, &[&str])]) -> Plan {
        Plan {
            version: 3,
            routes: vec![Route {
                id: route.into(),
                zones: vec![Zone {
                    id: "code".into(),
                    when: vec![],
                    share: Share::InOrder,
                    candidates: candidates
                        .iter()
                        .map(|(model, channels)| Candidate {
                            model: (*model).into(),
                            channels: channels.iter().map(|c| (*c).to_string()).collect(),
                        })
                        .collect(),
                }],
            }],
            tunables: Tunables::default(),
            changes: vec![],
            by: Author::Helper,
        }
    }

    fn m(
        channel: &str,
        model: &str,
        billing: Billing,
        success: f64,
        first_token: Option<f64>,
        quota_left: Option<f64>,
        suspect: bool,
    ) -> Measure {
        Measure {
            channel: channel.into(),
            model: model.into(),
            billing,
            success,
            first_token,
            quota_left,
            suspect,
        }
    }

    fn order(plan: &Plan) -> Vec<String> {
        plan.routes[0].zones[0]
            .candidates
            .iter()
            .flat_map(|c| c.channels.iter().map(move |ch| format!("{ch}/{}", c.model)))
            .collect()
    }

    #[test]
    fn nothing_known_changes_nothing() {
        let current = plan(
            "auto",
            &[("opus", &["claude", "openrouter"]), ("sonnet", &["claude"])],
        );
        let (next, shifts) = draft(&current, &[]);
        assert_eq!(order(&next), order(&current));
        assert!(shifts.is_empty());
        assert_eq!(next.by, Author::Scorer);
    }

    #[test]
    fn balanced_keeps_the_model_order_but_moves_off_a_channel_running_dry() {
        let current = plan(
            "auto",
            &[("opus", &["claude", "openrouter"]), ("sonnet", &["claude"])],
        );
        let measures = [m(
            "claude",
            "opus",
            Billing::Subscription,
            1.0,
            Some(1.0),
            Some(0.1),
            false,
        )];
        let (next, shifts) = draft(&current, &measures);
        assert_eq!(
            order(&next),
            vec!["openrouter/opus", "claude/opus", "claude/sonnet"]
        );
        assert_eq!(shifts[0].cause, Cause::LowQuota { left: 0.1 });
    }

    #[test]
    fn a_suspect_channel_is_dropped_and_said_so() {
        let current = plan("auto", &[("opus", &["relay", "claude"])]);
        let (next, shifts) = draft(
            &current,
            &[m("relay", "opus", Billing::PerUse, 1.0, None, None, true)],
        );
        assert_eq!(order(&next), vec!["claude/opus"]);
        assert_eq!(shifts[0].cause, Cause::Unusable { suspect: true });
    }

    #[test]
    fn cheap_puts_free_first_and_fast_puts_the_quickest_first() {
        let pairs: &[(&str, &[&str])] = &[
            ("sonnet", &["claude"]),
            ("flash", &["glm"]),
            ("v4", &["deepseek"]),
        ];
        let measures = [
            m(
                "claude",
                "sonnet",
                Billing::Subscription,
                1.0,
                Some(2.0),
                Some(0.9),
                false,
            ),
            m("glm", "flash", Billing::Free, 1.0, Some(0.6), None, false),
            m(
                "deepseek",
                "v4",
                Billing::PerUse,
                1.0,
                Some(0.4),
                None,
                false,
            ),
        ];
        let (save, shifts) = draft(&plan("save", pairs), &measures);
        assert_eq!(
            order(&save),
            vec!["glm/flash", "claude/sonnet", "deepseek/v4"]
        );
        assert_eq!(shifts[0].cause, Cause::Cheaper);
        let (fast, shifts) = draft(&plan("fast", pairs), &measures);
        assert_eq!(
            order(&fast),
            vec!["deepseek/v4", "glm/flash", "claude/sonnet"]
        );
        assert_eq!(shifts[0].cause, Cause::Faster { seconds: 0.4 });
    }

    #[test]
    fn a_route_without_a_strategy_is_left_alone() {
        let current = plan("custom", &[("a", &["x"]), ("b", &["y"])]);
        let (next, shifts) = draft(
            &current,
            &[m("x", "a", Billing::PerUse, 0.0, None, Some(0.0), true)],
        );
        assert_eq!(order(&next), order(&current));
        assert!(shifts.is_empty());
    }
}
