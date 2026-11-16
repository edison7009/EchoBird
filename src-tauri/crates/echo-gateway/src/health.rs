//! How each channel is doing, per model: whether it may be asked now, and how fast it has been. Updated when a
//! request ends, read when the next one is routed. Also the measurements the helper's scorer plans from.
//!
//! States (after EchoBird's health transitions):
//!
//! ```text
//! ok ──429──▶ rate limited (until Retry-After) ─────────────────────────▶ ok
//!  │ ──quota gone──▶ exhausted (until the reset) ─────────────────────────▶ ok
//!  │ ──5xx / timeout──▶ backing off (1 min, doubling, at most 15) ──▶ trial ──ok──▶ ok
//!  └ ──failed health check──▶ disabled (until checked again or restored by the user)
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axum::http::StatusCode;

use crate::error::ProxyError;

const BACKOFF_FIRST: Duration = Duration::from_secs(60);
const BACKOFF_MOST: Duration = Duration::from_secs(15 * 60);
/// Without a Retry-After, a rate limit is assumed to last this long.
const RATE_LIMIT_DEFAULT: Duration = Duration::from_secs(60);
/// How much a new measurement moves the running averages.
const EWMA: f64 = 0.2;

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Ok,
    RateLimited {
        until: Instant,
    },
    Exhausted {
        until: Instant,
    },
    BackingOff {
        until: Instant,
        strikes: u32,
    },
    /// The back-off is over: the next request is a trial; one success and it is back to normal.
    Trial {
        strikes: u32,
    },
    Disabled {
        reason: String,
    },
}

/// What is known of one channel serving one model.
#[derive(Clone, Debug)]
pub struct Health {
    pub state: State,
    /// Seconds to the first token, averaged.
    pub first_token: Option<f64>,
    /// Characters a second, averaged.
    pub speed: Option<f64>,
    /// Of the last answers, the share that succeeded (0–1), averaged.
    pub success: f64,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            state: State::Ok,
            first_token: None,
            speed: None,
            success: 1.0,
        }
    }
}

fn average(old: Option<f64>, new: f64) -> f64 {
    old.map_or(new, |old| old + EWMA * (new - old))
}

impl Health {
    /// Whether a request may go here now. Expired waits count as over.
    pub fn usable(&self, now: Instant) -> bool {
        match &self.state {
            State::Ok | State::Trial { .. } => true,
            State::RateLimited { until }
            | State::Exhausted { until }
            | State::BackingOff { until, .. } => now >= *until,
            State::Disabled { .. } => false,
        }
    }

    /// Rate limited but otherwise fine: the plan may choose to keep it in place or send it to the back.
    pub fn rate_limited(&self, now: Instant) -> bool {
        matches!(self.state, State::RateLimited { until } if now < until)
    }

    pub fn on_success(&mut self, first_token: Duration, speed: f64) {
        self.state = State::Ok;
        self.first_token = Some(average(self.first_token, first_token.as_secs_f64()));
        if speed.is_finite() && speed > 0.0 {
            self.speed = Some(average(self.speed, speed));
        }
        self.success = average(Some(self.success), 1.0);
    }

    /// Records a failure before the first token. Errors that are the request's fault (a bad request, a
    /// context too long) say nothing about the channel and leave it as it is.
    pub fn on_failure(&mut self, error: &ProxyError, now: Instant) {
        let retry_after = error
            .retry_after
            .as_ref()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let strikes = match self.state {
            State::BackingOff { strikes, .. } | State::Trial { strikes } => strikes,
            _ => 0,
        };
        self.state = match error.status {
            StatusCode::TOO_MANY_REQUESTS => State::RateLimited {
                until: now + retry_after.unwrap_or(RATE_LIMIT_DEFAULT),
            },
            StatusCode::PAYMENT_REQUIRED => State::Exhausted {
                until: now + retry_after.unwrap_or(BACKOFF_MOST),
            },
            status if status.is_server_error() || status == StatusCode::REQUEST_TIMEOUT => {
                let wait = BACKOFF_FIRST
                    .saturating_mul(1 << strikes.min(4))
                    .min(BACKOFF_MOST);
                State::BackingOff {
                    until: now + wait,
                    strikes: strikes + 1,
                }
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => State::Disabled {
                reason: error.message.clone(),
            },
            _ => return,
        };
        self.success = average(Some(self.success), 0.0);
    }

    /// Moves an expired back-off to a trial; called when the channel is about to be considered.
    pub fn tick(&mut self, now: Instant) {
        match self.state {
            State::BackingOff { until, strikes } if now >= until => {
                self.state = State::Trial { strikes }
            }
            State::RateLimited { until } | State::Exhausted { until } if now >= until => {
                self.state = State::Ok
            }
            _ => {}
        }
    }
}

/// Health of every `channel/model` pair seen so far, and the channels taken out of service as a whole.
#[derive(Default)]
pub struct Board {
    pairs: HashMap<String, Health>,
    /// Channel → why it is out of service (a failed health check), on every model, seen or not.
    disabled: std::collections::BTreeMap<String, String>,
}

impl Board {
    pub fn get(&self, channel: &str, model: &str) -> Health {
        let mut health = self
            .pairs
            .get(&key(channel, model))
            .cloned()
            .unwrap_or_default();
        if let Some(reason) = self.disabled.get(channel) {
            health.state = State::Disabled {
                reason: reason.clone(),
            };
        }
        health
    }

    /// The channels out of service.
    pub fn disabled(&self) -> std::collections::BTreeSet<String> {
        self.disabled.keys().cloned().collect()
    }

    pub fn update(
        &mut self,
        channel: &str,
        model: &str,
        now: Instant,
        change: impl FnOnce(&mut Health),
    ) {
        let health = self.pairs.entry(key(channel, model)).or_default();
        health.tick(now);
        change(health);
    }

    /// Takes a channel out of service on every model (a failed health check), or puts it back.
    pub fn set_disabled(&mut self, channel: &str, reason: Option<&str>) {
        match reason {
            Some(reason) => {
                self.disabled.insert(channel.into(), reason.into());
            }
            None => {
                self.disabled.remove(channel);
            }
        }
    }
}

fn key(channel: &str, model: &str) -> String {
    format!("{channel}/{model}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn error(status: StatusCode, retry_after: Option<&str>) -> ProxyError {
        let mut error = ProxyError::new(status, "upstream said no");
        error.retry_after = retry_after.map(|value| HeaderValue::from_str(value).unwrap());
        error
    }

    #[test]
    fn a_rate_limit_waits_for_retry_after_then_is_usable_again() {
        let now = Instant::now();
        let mut health = Health::default();
        health.on_failure(&error(StatusCode::TOO_MANY_REQUESTS, Some("30")), now);
        assert!(!health.usable(now + Duration::from_secs(29)));
        assert!(health.rate_limited(now));
        assert!(health.usable(now + Duration::from_secs(30)));
    }

    #[test]
    fn server_errors_back_off_doubling_up_to_fifteen_minutes() {
        let mut now = Instant::now();
        let mut health = Health::default();
        let mut waits = Vec::new();
        for _ in 0..6 {
            health.on_failure(&error(StatusCode::BAD_GATEWAY, None), now);
            let State::BackingOff { until, .. } = health.state else {
                panic!("{:?}", health.state)
            };
            waits.push((until - now).as_secs());
            now = until;
            health.tick(now);
            assert_eq!(
                health.state,
                State::Trial {
                    strikes: waits.len() as u32
                }
            );
        }
        assert_eq!(waits, vec![60, 120, 240, 480, 900, 900]);
        health.on_success(Duration::from_millis(800), 60.0);
        assert_eq!(health.state, State::Ok);
    }

    #[test]
    fn the_requests_own_mistakes_do_not_mark_the_channel() {
        let mut health = Health::default();
        health.on_failure(&error(StatusCode::BAD_REQUEST, None), Instant::now());
        assert_eq!((health.state.clone(), health.success), (State::Ok, 1.0));
    }

    #[test]
    fn a_rejected_key_is_disabled_and_quota_waits_for_the_reset() {
        let now = Instant::now();
        let mut health = Health::default();
        health.on_failure(&error(StatusCode::UNAUTHORIZED, None), now);
        assert!(!health.usable(now + BACKOFF_MOST));
        health = Health::default();
        health.on_failure(&error(StatusCode::PAYMENT_REQUIRED, Some("7200")), now);
        assert!(!health.usable(now + Duration::from_secs(7199)));
        assert!(health.usable(now + Duration::from_secs(7200)));
    }

    #[test]
    fn speeds_are_averaged_not_replaced() {
        let mut health = Health::default();
        health.on_success(Duration::from_secs(1), 100.0);
        health.on_success(Duration::from_secs(3), 50.0);
        assert!((health.first_token.unwrap() - 1.4).abs() < 1e-9);
        assert!((health.speed.unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn a_failed_health_check_takes_the_channel_off_every_model() {
        let now = Instant::now();
        let mut board = Board::default();
        board.update("relay", "opus", now, |h| {
            h.on_success(Duration::from_secs(1), 10.0)
        });
        board.update("relay", "sonnet", now, |h| {
            h.on_success(Duration::from_secs(1), 10.0)
        });
        board.set_disabled("relay", Some("looks watered down"));
        assert!(!board.get("relay", "opus").usable(now));
        assert!(!board.get("relay", "sonnet").usable(now));
        board.set_disabled("relay", None);
        assert!(board.get("relay", "opus").usable(now));
    }
}
