//! Where a request can be sent. Every channel — an API key, a subscription account, a local model — is an
//! `Upstream`: it names the models it serves and turns an OpenAI Chat request into an answer, or a classified
//! `ProxyError`. The engine never knows which kind it is talking to. New kinds of channel are added here, one
//! adapter at a time; nothing else changes.

use std::collections::BTreeSet;

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use futures_util::future::BoxFuture;
use serde_json::Value;

use crate::error::ProxyError;
use crate::failure;

/// What kind of channel it is, for the user and for the scorer (subscriptions are used before pay-as-you-go).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Billing {
    /// Costs nothing (a free tier, a local model).
    Free,
    /// A subscription's allowance: already paid for, it resets.
    Subscription,
    /// Charged per use.
    PerUse,
}

/// A channel the engine can send to.
pub trait Upstream: Send + Sync {
    /// Stable id, used by plans (`channel/model`).
    fn id(&self) -> &str;
    /// The models it serves, as it names them.
    fn models(&self) -> &[String];
    fn billing(&self) -> Billing;
    /// Sends one OpenAI Chat request (its `model` already set) and returns the answer as it arrives. An error
    /// is returned only before the answer starts; it must be classified (`failure`), so routing can tell a rate
    /// limit from an exhausted quota or a bad request.
    fn send<'a>(
        &'a self,
        chat: &'a Value,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>>;
}

/// An OpenAI-compatible API: a base URL and a key.
pub struct OpenAiChat {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub billing: Billing,
}

impl Upstream for OpenAiChat {
    fn id(&self) -> &str {
        &self.id
    }

    fn models(&self) -> &[String] {
        &self.models
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn send<'a>(
        &'a self,
        chat: &'a Value,
        client: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>> {
        Box::pin(async move {
            let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
            let upstream = client
                .post(url)
                .bearer_auth(&self.api_key)
                .json(chat)
                .send()
                .await
                .map_err(ProxyError::upstream)?;
            let status = upstream.status();
            if !status.is_success() {
                let headers = upstream.headers().clone();
                let body = upstream.json::<Value>().await.unwrap_or(Value::Null);
                return Err(failure::upstream_error(status, &headers, &body));
            }
            Ok(passthrough(upstream))
        })
    }
}

/// An upstream's successful answer, passed on as it arrives.
pub fn passthrough(upstream: reqwest::Response) -> Response {
    let status = upstream.status();
    let kind = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
    *response.status_mut() = status;
    if let Some(kind) = kind {
        response.headers_mut().insert(header::CONTENT_TYPE, kind);
    }
    response
}

/// Asks an OpenAI-compatible API which models it serves (its `/models`), at most `limit`.
pub async fn list_models(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
    limit: usize,
) -> Result<Vec<String>, ProxyError> {
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(ProxyError::upstream)?;
    if !response.status().is_success() {
        return Err(ProxyError::new(
            response.status(),
            "The channel would not list its models",
        ));
    }
    let list: Value = response.json().await.map_err(ProxyError::upstream)?;
    Ok(list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .take(limit)
        .collect())
}

/// Every upstream the engine may use, by id.
#[derive(Default, Clone)]
pub struct Upstreams {
    list: Vec<std::sync::Arc<dyn Upstream>>,
}

impl Upstreams {
    pub fn new(list: Vec<std::sync::Arc<dyn Upstream>>) -> Self {
        Self { list }
    }

    pub fn get(&self, id: &str) -> Option<&std::sync::Arc<dyn Upstream>> {
        self.list.iter().find(|u| u.id() == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &std::sync::Arc<dyn Upstream>> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// What a plan may refer to: these upstreams, their models, which cost nothing.
    pub fn known(&self, disabled: &BTreeSet<String>) -> crate::plan::Known {
        let mut known = crate::plan::Known {
            disabled: disabled.clone(),
            ..Default::default()
        };
        for upstream in &self.list {
            known.channels.insert(upstream.id().to_string());
            for model in upstream.models() {
                let pair = format!("{}/{model}", upstream.id());
                if upstream.billing() == Billing::Free {
                    known.free.insert(pair.clone());
                }
                known.models.insert(pair);
            }
        }
        known
    }
}

/// An upstream that is not wired yet (a kind of account still being moved over): it is listed, so plans and
/// the user see it, but refuses with a clear 501 until its adapter lands.
pub struct Pending {
    pub id: String,
    pub models: Vec<String>,
    pub billing: Billing,
    pub why: String,
}

impl Upstream for Pending {
    fn id(&self) -> &str {
        &self.id
    }

    fn models(&self) -> &[String] {
        &self.models
    }

    fn billing(&self) -> Billing {
        self.billing
    }

    fn send<'a>(
        &'a self,
        _: &'a Value,
        _: &'a reqwest::Client,
    ) -> BoxFuture<'a, Result<Response, ProxyError>> {
        Box::pin(async move {
            Err(ProxyError::new(
                StatusCode::NOT_IMPLEMENTED,
                self.why.clone(),
            ))
        })
    }
}
