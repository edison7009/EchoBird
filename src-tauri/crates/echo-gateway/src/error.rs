//! The error every gateway path returns, and how it becomes an HTTP response.

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ProxyError {
    pub status: StatusCode,
    pub message: String,
    pub error_code: Option<String>,
    pub error_type: Option<String>,
    pub retry_after: Option<HeaderValue>,
    pub model_scope: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProxyFailureMetadata {
    pub model_scope: bool,
}

impl std::fmt::Display for ProxyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProxyError {}

impl ProxyError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            error_code: None,
            error_type: None,
            retry_after: None,
            model_scope: false,
        }
    }

    pub fn into_response(self) -> Response {
        let mut error = json!({"message":self.message,"type":self.error_type.as_deref().unwrap_or("upstream_error")});
        if let Some(code) = self.error_code {
            error["code"] = json!(code);
        }
        let mut response = json_response(&json!({"error":error}));
        *response.status_mut() = self.status;
        if let Some(retry_after) = self.retry_after {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, retry_after);
        }
        response.extensions_mut().insert(ProxyFailureMetadata {
            model_scope: self.model_scope,
        });
        response
    }

    pub fn upstream(error: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::BAD_GATEWAY,
            format!("Account upstream failed: {error}"),
        )
    }
}

pub fn json_response(value: &Value) -> Response {
    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(value.to_string()))
        .expect("fixed response headers")
}

pub fn stream_response(body: Body) -> Response {
    Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(body)
        .expect("fixed response headers")
}
