//! The engine: runs one request through the plan in use. Picks the zone, orders the channels, and tries them
//! one by one until one starts answering: a channel that errors, or stays silent past the first-token wait, is
//! marked and the next is tried, so the app never sees it. Once an answer has started it is passed through and
//! never replayed elsewhere. Every step goes to the telemetry sink.
//!
//! Shared by every way in (OpenAI Chat, Responses, Anthropic Messages are converted to Chat before they get
//! here) and by the helper's own calls (`internal`), which use the same channels on their own route.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::error::ProxyError;
use crate::health::{Board, Health};
use crate::plan::{Known, Plan, Stick};
use crate::select::{self, Facts};
use crate::store::PlanStore;
use crate::telemetry::{Event, Sink};
use crate::upstream::{Billing, Upstreams};

/// How long a conversation stays on the channel that answered it when sticking is automatic: about as long as
/// vendors keep a prompt cache warm.
const STICK_FOR: Duration = Duration::from_secs(5 * 60);

/// One request: its facts, the conversation it belongs to (for sticking), whether it is the app's own (the
/// helper's), and the route to take when not the one the user picked.
#[derive(Clone, Debug, Default)]
pub struct Ask {
    pub facts: Facts,
    pub session: Option<String>,
    pub internal: bool,
    pub route: Option<String>,
}

pub struct Engine {
    plans: PlanStore,
    board: Mutex<Board>,
    upstreams: RwLock<Upstreams>,
    /// Session → the pair that answered it last, and when.
    sticky: Mutex<HashMap<String, (String, String, Instant)>>,
    route: RwLock<String>,
    rounds: AtomicU64,
    sink: Arc<dyn Sink>,
    client: reqwest::Client,
}

impl Engine {
    pub fn new(plans: PlanStore, sink: Arc<dyn Sink>) -> Self {
        Self {
            plans,
            board: Mutex::new(Board::default()),
            upstreams: RwLock::new(Upstreams::default()),
            sticky: Mutex::new(HashMap::new()),
            route: RwLock::new("auto".into()),
            rounds: AtomicU64::new(0),
            sink,
            client: reqwest::Client::new(),
        }
    }

    pub fn plans(&self) -> &PlanStore {
        &self.plans
    }

    pub fn sink(&self) -> &Arc<dyn Sink> {
        &self.sink
    }

    pub fn set_upstreams(&self, upstreams: Upstreams) {
        *self.upstreams.write().unwrap_or_else(|p| p.into_inner()) = upstreams;
    }

    pub fn upstreams(&self) -> Upstreams {
        self.upstreams
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    /// What a plan may refer to now: the upstreams, minus the channels taken out of service.
    pub fn known(&self) -> Known {
        let disabled = self
            .board
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .disabled();
        self.upstreams().known(&disabled)
    }

    /// The route the user picked (日常 is `auto`).
    pub fn set_route(&self, route: &str) {
        *self.route.write().unwrap_or_else(|p| p.into_inner()) = route.into();
    }

    pub fn route(&self) -> String {
        self.route.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// How a channel serving a model is doing (for the scorer and the UI).
    pub fn health(&self, channel: &str, model: &str) -> Health {
        self.board
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(channel, model)
    }

    /// Takes a channel out of service on every model (a failed health check), or puts it back.
    pub fn set_disabled(&self, channel: &str, reason: Option<&str>) {
        self.board
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .set_disabled(channel, reason);
    }

    fn last_for(
        &self,
        session: Option<&str>,
        stick: Stick,
        now: Instant,
    ) -> Option<(String, String)> {
        if stick == Stick::Off {
            return None;
        }
        let sticky = self.sticky.lock().unwrap_or_else(|p| p.into_inner());
        let (channel, model, at) = sticky.get(session?)?;
        (stick != Stick::Auto || now.duration_since(*at) < STICK_FOR)
            .then(|| (channel.clone(), model.clone()))
    }

    /// Runs one OpenAI Chat request through the plan.
    pub async fn chat(&self, ask: Ask, mut chat: Value) -> Result<Response, ProxyError> {
        let plan: Arc<Plan> = self.plans.current();
        let now = Instant::now();
        let route = ask.route.clone().unwrap_or_else(|| self.route());
        let mut facts = ask.facts.clone();
        facts.last = self.last_for(ask.session.as_deref(), plan.tunables.stick, now);
        let zone = select::zone(&plan, &route, &facts).ok_or_else(|| {
            ProxyError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "No channel is connected yet",
            )
        })?;
        let upstreams = self.upstreams();
        let free = |channel: &str, _: &str| {
            upstreams
                .get(channel)
                .is_some_and(|u| u.billing() == Billing::Free)
        };
        let picks = {
            let board = self.board.lock().unwrap_or_else(|p| p.into_inner());
            select::order(&plan, zone, &facts, &board, &free, now)
        };
        let round = self.rounds.fetch_add(1, Ordering::Relaxed) + 1;
        self.sink.emit(Event::Round {
            id: round,
            route: route.clone(),
            zone: zone.id.clone(),
            app: facts.app.clone(),
            internal: ask.internal,
        });

        let streaming = chat["stream"].as_bool().unwrap_or(false);
        let deadline = now + Duration::from_millis(plan.tunables.round_budget_ms);
        let first_token = Duration::from_millis(plan.tunables.first_token_ms);
        let mut last = ProxyError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Every channel for this request is resting; try again shortly",
        );
        for (n, pick) in picks.iter().enumerate() {
            let started = Instant::now();
            if started >= deadline {
                break;
            }
            let Some(upstream) = upstreams.get(&pick.channel) else {
                continue;
            };
            chat["model"] = json!(pick.model);
            let wait = first_token.min(deadline - started);
            let outcome = match tokio::time::timeout(wait, upstream.send(&chat, &self.client)).await
            {
                Err(_) => Err(silent()),
                Ok(Err(error)) => Err(error),
                Ok(Ok(answer)) if streaming => {
                    first_bytes(answer, wait.saturating_sub(started.elapsed())).await
                }
                Ok(Ok(answer)) => Ok(answer),
            };
            let ms = started.elapsed().as_millis() as u64;
            let attempt = |ok: bool, status: u16, why: String| Event::Attempt {
                round,
                n: n as u32 + 1,
                channel: pick.channel.clone(),
                model: pick.model.clone(),
                ok,
                status,
                ms,
                why,
            };
            match outcome {
                Ok(answer) => {
                    let done = Instant::now();
                    self.board.lock().unwrap_or_else(|p| p.into_inner()).update(
                        &pick.channel,
                        &pick.model,
                        done,
                        |h| h.on_success(started.elapsed(), 0.0),
                    );
                    if let Some(session) = &ask.session {
                        self.sticky
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .insert(
                                session.clone(),
                                (pick.channel.clone(), pick.model.clone(), done),
                            );
                    }
                    self.sink
                        .emit(attempt(true, answer.status().as_u16(), String::new()));
                    return Ok(answer);
                }
                Err(error) => {
                    let failed = Instant::now();
                    self.board.lock().unwrap_or_else(|p| p.into_inner()).update(
                        &pick.channel,
                        &pick.model,
                        failed,
                        |h| h.on_failure(&error, failed),
                    );
                    self.sink
                        .emit(attempt(false, error.status.as_u16(), error.message.clone()));
                    if request_fault(&error) {
                        return Err(error);
                    }
                    last = error;
                }
            }
        }
        Err(last)
    }
}

fn silent() -> ProxyError {
    ProxyError::new(
        StatusCode::GATEWAY_TIMEOUT,
        "The channel did not start answering in time",
    )
}

/// An error that is the request's own fault: another channel would refuse it too.
fn request_fault(error: &ProxyError) -> bool {
    error.status == StatusCode::BAD_REQUEST && !error.model_scope
}

/// Waits for a streamed answer's first bytes: a channel that accepts the request and then says nothing counts
/// as failed, and the next one is tried. The bytes read are put back in front of the rest.
async fn first_bytes(answer: Response, wait: Duration) -> Result<Response, ProxyError> {
    let (parts, body) = answer.into_parts();
    let mut stream = body.into_data_stream();
    match tokio::time::timeout(wait, stream.next()).await {
        Err(_) => Err(silent()),
        Ok(None) => Err(ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "The channel answered with nothing",
        )),
        Ok(Some(Err(error))) => Err(ProxyError::upstream(error)),
        Ok(Some(Ok(first))) => {
            let rest = futures_util::stream::once(async move { Ok::<_, axum::Error>(first) })
                .chain(stream);
            Ok(Response::from_parts(parts, Body::from_stream(rest)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{Author, Candidate, Route, Share, Tunables, Zone};
    use crate::telemetry::Ring;
    use crate::upstream::{Pending, Upstream};
    use futures_util::future::BoxFuture;
    use std::sync::atomic::AtomicUsize;

    /// A stand-in channel: fails with `status` (when set), or answers "ok" after `delay` — before the headers,
    /// or, when `silent_body`, after them.
    struct Fake {
        id: String,
        status: Option<StatusCode>,
        delay: Duration,
        silent_body: bool,
        calls: AtomicUsize,
        models: Vec<String>,
    }

    fn fake(id: &str, status: Option<StatusCode>, delay_ms: u64, silent_body: bool) -> Arc<Fake> {
        Arc::new(Fake {
            id: id.into(),
            status,
            delay: Duration::from_millis(delay_ms),
            silent_body,
            calls: AtomicUsize::new(0),
            models: vec!["m".into()],
        })
    }

    impl Upstream for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn models(&self) -> &[String] {
            &self.models
        }
        fn billing(&self) -> Billing {
            Billing::PerUse
        }
        fn send<'a>(
            &'a self,
            _: &'a Value,
            _: &'a reqwest::Client,
        ) -> BoxFuture<'a, Result<Response, ProxyError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if !self.silent_body {
                    tokio::time::sleep(self.delay).await;
                }
                if let Some(status) = self.status {
                    let mut error = ProxyError::new(status, "no");
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        error.retry_after = Some(axum::http::HeaderValue::from_static("60"));
                    }
                    return Err(error);
                }
                let delay = self.delay;
                let body = futures_util::stream::once(async move {
                    tokio::time::sleep(delay).await;
                    Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"data: ok\n\n"))
                });
                Ok(Response::new(Body::from_stream(body)))
            })
        }
    }

    fn engine(channels: &[&Arc<Fake>], first_token_ms: u64) -> (Engine, Arc<Ring>) {
        let candidates = channels
            .iter()
            .map(|f| Candidate {
                model: "m".into(),
                channels: vec![f.id.clone()],
            })
            .collect();
        let plan = Plan {
            version: 1,
            routes: vec![Route {
                id: "auto".into(),
                zones: vec![Zone {
                    id: "all".into(),
                    when: vec![],
                    share: Share::InOrder,
                    candidates,
                }],
            }],
            tunables: Tunables {
                first_token_ms,
                ..Tunables::default()
            },
            changes: vec![],
            by: Author::Default,
        };
        let ring = Arc::new(Ring::new(100));
        let engine = Engine::new(PlanStore::new(plan, Duration::ZERO), ring.clone());
        engine.set_upstreams(Upstreams::new(
            channels
                .iter()
                .map(|f| (*f).clone() as Arc<dyn Upstream>)
                .collect(),
        ));
        (engine, ring)
    }

    fn streamed() -> Value {
        json!({"stream":true,"messages":[]})
    }

    async fn text(response: Response) -> String {
        String::from_utf8(
            axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_failing_channel_is_skipped_and_marked() {
        let busy = fake("busy", Some(StatusCode::TOO_MANY_REQUESTS), 0, false);
        let spare = fake("spare", None, 0, false);
        let (engine, ring) = engine(&[&busy, &spare], 5_000);
        let answer = engine.chat(Ask::default(), streamed()).await.unwrap();
        assert_eq!(text(answer).await, "data: ok\n\n");
        assert!(!engine.health("busy", "m").usable(Instant::now()));
        let attempts: Vec<bool> = ring
            .recent()
            .into_iter()
            .filter_map(|e| {
                if let Event::Attempt { ok, .. } = e {
                    Some(ok)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(attempts, vec![false, true]);
        // Resting now: the next request goes straight to the spare.
        engine.chat(Ask::default(), streamed()).await.unwrap();
        assert_eq!(
            (
                busy.calls.load(Ordering::SeqCst),
                spare.calls.load(Ordering::SeqCst)
            ),
            (1, 2)
        );
    }

    // Tokio's clock is paused here: the minute of silence passes at once.
    #[tokio::test(start_paused = true)]
    async fn a_channel_that_accepts_and_then_says_nothing_is_switched_away_from() {
        let mute = fake("mute", None, 60_000, true);
        let spare = fake("spare", None, 0, false);
        let (engine, _) = engine(&[&mute, &spare], 5_000);
        let answer = engine.chat(Ask::default(), streamed()).await.unwrap();
        assert_eq!(text(answer).await, "data: ok\n\n");
        assert_eq!(spare.calls.load(Ordering::SeqCst), 1);
        assert!(!engine.health("mute", "m").usable(Instant::now()));
    }

    #[tokio::test]
    async fn the_requests_own_fault_is_returned_without_trying_others() {
        let strict = fake("strict", Some(StatusCode::BAD_REQUEST), 0, false);
        let spare = fake("spare", None, 0, false);
        let (engine, _) = engine(&[&strict, &spare], 5_000);
        let error = engine.chat(Ask::default(), streamed()).await.unwrap_err();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(spare.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_conversation_sticks_to_the_channel_that_answered_it() {
        let first = fake("first", None, 0, false);
        let second = fake("second", None, 0, false);
        let (engine, _) = engine(&[&first, &second], 5_000);
        // The first channel rests for a moment; the conversation lands on the second and stays there.
        engine
            .board
            .lock()
            .unwrap()
            .update("first", "m", Instant::now(), |h| {
                h.on_failure(
                    &ProxyError::new(StatusCode::BAD_GATEWAY, "x"),
                    Instant::now(),
                )
            });
        let ask = Ask {
            session: Some("s1".into()),
            ..Ask::default()
        };
        engine.chat(ask.clone(), streamed()).await.unwrap();
        engine
            .board
            .lock()
            .unwrap()
            .update("first", "m", Instant::now(), |h| {
                h.on_success(Duration::from_millis(1), 1.0)
            });
        engine.chat(ask, streamed()).await.unwrap();
        assert_eq!(
            (
                first.calls.load(Ordering::SeqCst),
                second.calls.load(Ordering::SeqCst)
            ),
            (0, 2)
        );
        // Another conversation starts on the first again.
        engine
            .chat(
                Ask {
                    session: Some("s2".into()),
                    ..Ask::default()
                },
                streamed(),
            )
            .await
            .unwrap();
        assert_eq!(first.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_channel_not_wired_yet_is_listed_but_passed_over() {
        let spare = fake("spare", None, 0, false);
        let plan = Plan {
            version: 1,
            routes: vec![Route {
                id: "auto".into(),
                zones: vec![Zone {
                    id: "all".into(),
                    when: vec![],
                    share: Share::InOrder,
                    candidates: vec![Candidate {
                        model: "m".into(),
                        channels: vec!["account".into(), "spare".into()],
                    }],
                }],
            }],
            tunables: Tunables::default(),
            changes: vec![],
            by: Author::Default,
        };
        let engine = Engine::new(
            PlanStore::new(plan, Duration::ZERO),
            Arc::new(Ring::new(10)),
        );
        let pending: Arc<dyn Upstream> = Arc::new(Pending {
            id: "account".into(),
            models: vec!["m".into()],
            billing: Billing::Subscription,
            why: "not wired yet".into(),
        });
        engine.set_upstreams(Upstreams::new(vec![pending, spare.clone()]));
        assert!(engine.known().channels.contains("account"));
        assert_eq!(
            text(engine.chat(Ask::default(), streamed()).await.unwrap()).await,
            "data: ok\n\n"
        );
    }
}
