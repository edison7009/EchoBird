use super::{auth_headers, ProxyError, ENDPOINT};
use axum::http::StatusCode;
use futures_util::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite::{client::IntoClientRequest, Error, Message},
    MaybeTlsStream, WebSocketStream,
};

pub(crate) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(crate) struct Event {
    pub text: String,
    pub final_result: bool,
}

pub(crate) fn event(message: Message) -> Result<Option<Event>, ProxyError> {
    let bytes = match message {
        Message::Text(text) => text.as_bytes().to_vec(),
        Message::Binary(bytes) => bytes.to_vec(),
        Message::Close(_) => return Err(failed()),
        _ => return Ok(None),
    };
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| failed())?;
    if value.get("code").is_some_and(|code| code != 0) {
        return Err(failed());
    }
    Ok(Some(Event {
        text: value["text"].as_str().unwrap_or_default().to_owned(),
        final_result: value["final"].as_bool().unwrap_or(false),
    }))
}

pub(crate) async fn connect_with<F, Fut>(mut credentials: F) -> Result<(Socket, Event), ProxyError>
where
    F: FnMut(Option<Value>) -> Fut,
    Fut: std::future::Future<Output = Result<Value, String>>,
{
    let mut current = credentials(None)
        .await
        .map_err(crate::services::account_model_sources::credential_error)?;
    for attempt in 0..2 {
        match connect(&current).await {
            Err(error) if error.status == StatusCode::UNAUTHORIZED && attempt == 0 => {
                current = credentials(Some(current))
                    .await
                    .map_err(crate::services::account_model_sources::credential_error)?;
            }
            result => return result,
        }
    }
    unreachable!()
}

async fn connect(credentials: &Value) -> Result<(Socket, Event), ProxyError> {
    let endpoint = ENDPOINT;
    #[cfg(test)]
    let endpoint = credentials["_voice_test_endpoint"]
        .as_str()
        .unwrap_or(endpoint);
    let mut url = url::Url::parse(endpoint).map_err(|_| failed())?;
    let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
    url.set_scheme(scheme).map_err(|_| failed())?;
    url.set_path("/clientcap/v2/asr/stream");
    url.query_pairs_mut()
        .append_pair("source", "desktop")
        .append_pair("interim_results", "1");
    let mut request = url.as_str().into_client_request().map_err(|_| failed())?;
    request.headers_mut().extend(auth_headers(credentials)?);
    tokio::time::timeout(Duration::from_secs(10), async {
        let (mut socket, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|error| {
                if let Error::Http(response) = error {
                    ProxyError::new(
                        response.status(),
                        "WorkBuddy speech stream connection failed",
                    )
                } else {
                    failed()
                }
            })?;
        while let Some(message) = socket.next().await {
            if let Some(event) = event(message.map_err(|_| failed())?)? {
                return Ok((socket, event));
            }
        }
        Err(failed())
    })
    .await
    .map_err(|_| {
        ProxyError::new(
            StatusCode::GATEWAY_TIMEOUT,
            "WorkBuddy speech stream handshake timed out",
        )
    })?
}

fn failed() -> ProxyError {
    ProxyError::new(StatusCode::BAD_GATEWAY, "WorkBuddy speech stream failed")
}
