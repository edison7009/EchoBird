//! The hot path: given the plan in use, a request's facts and how every channel is doing, which zone takes the
//! request and in what order its `channel/model` pairs are tried. Pure and in memory: no I/O, no model calls,
//! no locks held. The judging model's answer, when a zone needs one, is passed in as `intent`.

use std::time::Instant;

use crate::health::Board;
use crate::plan::{Condition, Plan, RateLimited, Share, Stick, Zone};

/// What is known of a request before routing it.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    pub has_image: bool,
    pub tokens: u32,
    /// The reasoning effort the app asked for, if any.
    pub effort: Option<String>,
    /// The app's key, e.g. `claude-code`.
    pub app: String,
    /// What the judging model said this sentence is, if it was asked and answered in time.
    pub intent: Option<String>,
    /// The pair that answered this conversation last, while its cache may still be worth keeping.
    pub last: Option<(String, String)>,
    /// A number that changes with every sentence the user says (for `Rotate`).
    pub turn: u64,
}

/// One place to send the request: a channel and the model asked of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub channel: String,
    pub model: String,
}

const EFFORTS: [&str; 4] = ["low", "medium", "high", "max"];
fn effort_rank(effort: &str) -> usize {
    EFFORTS.iter().position(|e| *e == effort).unwrap_or(0)
}

fn holds(condition: &Condition, facts: &Facts) -> bool {
    match condition {
        Condition::HasImage => facts.has_image,
        Condition::TokensAtLeast(least) => facts.tokens >= *least,
        Condition::EffortAtLeast(least) => facts
            .effort
            .as_deref()
            .is_some_and(|effort| effort_rank(effort) >= effort_rank(least)),
        Condition::App(app) => facts.app == *app,
        Condition::Intent(intent) => facts
            .intent
            .as_deref()
            .is_some_and(|said| said.eq_ignore_ascii_case(intent)),
    }
}

/// The zone that takes the request: the first whose conditions hold (any one is enough), or the first with no
/// conditions, which takes everything else.
pub fn zone<'a>(plan: &'a Plan, route: &str, facts: &Facts) -> Option<&'a Zone> {
    let zones = &plan.route(route).or_else(|| plan.routes.first())?.zones;
    zones
        .iter()
        .find(|zone| !zone.when.is_empty() && zone.when.iter().any(|c| holds(c, facts)))
        .or_else(|| zones.iter().find(|zone| zone.when.is_empty()))
}

/// Whether any zone of the route could be decided by intent: only then is the judging model asked.
pub fn needs_judge(plan: &Plan, route: &str) -> bool {
    plan.route(route).is_some_and(|route| {
        route
            .zones
            .iter()
            .any(|zone| zone.when.iter().any(Condition::needs_judge))
    })
}

/// The order to try a zone's pairs in. Unusable pairs are left out; rate-limited ones go to the back when the
/// plan says so; the pair that answered this conversation last goes first when sticking.
pub fn order(
    plan: &Plan,
    zone: &Zone,
    facts: &Facts,
    board: &Board,
    free: &dyn Fn(&str, &str) -> bool,
    now: Instant,
) -> Vec<Pick> {
    let mut picks: Vec<Pick> = zone
        .candidates
        .iter()
        .flat_map(|candidate| {
            candidate.channels.iter().map(|channel| Pick {
                channel: channel.clone(),
                model: candidate.model.clone(),
            })
        })
        .filter(|pick| board.get(&pick.channel, &pick.model).usable(now))
        .collect();

    match zone.share {
        Share::InOrder => {}
        Share::FreeFirst => picks.sort_by_key(|pick| !free(&pick.channel, &pick.model)),
        Share::Rotate => {
            if !picks.is_empty() {
                let by = (facts.turn % picks.len() as u64) as usize;
                picks.rotate_left(by);
            }
        }
        // Healthier and faster first; ties keep the plan's order (the sort is stable).
        Share::Smart => picks.sort_by(|a, b| {
            let (a, b) = (
                board.get(&a.channel, &a.model),
                board.get(&b.channel, &b.model),
            );
            b.success
                .partial_cmp(&a.success)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
    }

    if plan.tunables.rate_limited == RateLimited::ToTheBack {
        picks.sort_by_key(|pick| board.get(&pick.channel, &pick.model).rate_limited(now));
    }

    if plan.tunables.stick != Stick::Off {
        if let Some((channel, model)) = &facts.last {
            if let Some(at) = picks
                .iter()
                .position(|pick| pick.channel == *channel && pick.model == *model)
            {
                let kept = picks.remove(at);
                picks.insert(0, kept);
            }
        }
    }
    picks.truncate(plan.tunables.max_switches as usize + 1);
    picks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ProxyError;
    use crate::plan::{Author, Candidate, Route, Tunables};
    use axum::http::{HeaderValue, StatusCode};
    use std::time::Duration;

    fn zone(id: &str, when: Vec<Condition>, share: Share, pairs: &[(&str, &str)]) -> Zone {
        Zone {
            id: id.into(),
            when,
            share,
            candidates: pairs
                .iter()
                .map(|(channel, model)| Candidate {
                    model: (*model).into(),
                    channels: vec![(*channel).into()],
                })
                .collect(),
        }
    }

    fn plan(zones: Vec<Zone>) -> Plan {
        Plan {
            version: 1,
            routes: vec![Route {
                id: "auto".into(),
                zones,
            }],
            tunables: Tunables::default(),
            changes: vec![],
            by: Author::Default,
        }
    }

    fn channels(picks: &[Pick]) -> Vec<&str> {
        picks.iter().map(|p| p.channel.as_str()).collect()
    }

    const NOT_FREE: &dyn Fn(&str, &str) -> bool = &|_, _| false;

    #[test]
    fn facts_pick_the_zone_and_the_open_zone_takes_the_rest() {
        let p = plan(vec![
            zone(
                "image",
                vec![Condition::HasImage],
                Share::InOrder,
                &[("chatgpt", "sol")],
            ),
            zone(
                "hard",
                vec![
                    Condition::EffortAtLeast("high".into()),
                    Condition::Intent("hard".into()),
                ],
                Share::InOrder,
                &[("claude", "opus")],
            ),
            zone("code", vec![], Share::InOrder, &[("claude", "sonnet")]),
        ]);
        let pick = |facts: Facts| super::zone(&p, "auto", &facts).unwrap().id.clone();
        assert_eq!(
            pick(Facts {
                has_image: true,
                ..Facts::default()
            }),
            "image"
        );
        assert_eq!(
            pick(Facts {
                effort: Some("max".into()),
                ..Facts::default()
            }),
            "hard"
        );
        assert_eq!(
            pick(Facts {
                intent: Some("HARD".into()),
                ..Facts::default()
            }),
            "hard"
        );
        assert_eq!(pick(Facts::default()), "code");
        assert!(needs_judge(&p, "auto"));
    }

    #[test]
    fn unusable_pairs_are_left_out_and_rate_limited_ones_can_go_to_the_back() {
        let now = Instant::now();
        let z = zone(
            "code",
            vec![],
            Share::InOrder,
            &[
                ("claude", "opus"),
                ("relay", "opus"),
                ("openrouter", "opus"),
            ],
        );
        let mut board = Board::default();
        board.update("relay", "opus", now, |_| {});
        board.set_disabled("relay", Some("watered down"));
        let mut limited = ProxyError::new(StatusCode::TOO_MANY_REQUESTS, "slow down");
        limited.retry_after = Some(HeaderValue::from_static("30"));
        board.update("claude", "opus", now, |h| h.on_failure(&limited, now));

        let mut p = plan(vec![z.clone()]);
        assert_eq!(
            channels(&order(&p, &z, &Facts::default(), &board, NOT_FREE, now)),
            vec!["openrouter"]
        );
        p.tunables.rate_limited = RateLimited::ToTheBack;
        let later = now + Duration::from_secs(31);
        assert_eq!(
            channels(&order(&p, &z, &Facts::default(), &board, NOT_FREE, later)),
            vec!["claude", "openrouter"]
        );
    }

    #[test]
    fn the_conversation_sticks_to_the_pair_that_answered_it() {
        let now = Instant::now();
        let z = zone(
            "code",
            vec![],
            Share::InOrder,
            &[("claude", "opus"), ("openrouter", "opus")],
        );
        let facts = Facts {
            last: Some(("openrouter".into(), "opus".into())),
            ..Facts::default()
        };
        let mut p = plan(vec![z.clone()]);
        assert_eq!(
            channels(&order(&p, &z, &facts, &Board::default(), NOT_FREE, now)),
            vec!["openrouter", "claude"]
        );
        p.tunables.stick = Stick::Off;
        assert_eq!(
            channels(&order(&p, &z, &facts, &Board::default(), NOT_FREE, now)),
            vec!["claude", "openrouter"]
        );
    }

    #[test]
    fn free_first_and_rotate_reorder_the_zone() {
        let now = Instant::now();
        let free_first = zone(
            "chat",
            vec![],
            Share::FreeFirst,
            &[("deepseek", "flash"), ("glm", "flash")],
        );
        let free: &dyn Fn(&str, &str) -> bool = &|channel, _| channel == "glm";
        let p = plan(vec![free_first.clone()]);
        assert_eq!(
            channels(&order(
                &p,
                &free_first,
                &Facts::default(),
                &Board::default(),
                free,
                now
            )),
            vec!["glm", "deepseek"]
        );

        let rotate = zone(
            "chat",
            vec![],
            Share::Rotate,
            &[("a", "m"), ("b", "m"), ("c", "m")],
        );
        let turn = |turn| {
            channels(&order(
                &p,
                &rotate,
                &Facts {
                    turn,
                    ..Facts::default()
                },
                &Board::default(),
                NOT_FREE,
                now,
            ))
            .join("")
        };
        assert_eq!(
            (turn(0), turn(1), turn(2)),
            ("abc".to_string(), "bca".to_string(), "cab".to_string())
        );
    }
}
