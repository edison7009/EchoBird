use super::{client, Credentials, Edition, LoginStart};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

static PENDING: Mutex<Option<Arc<Attempt>>> = Mutex::new(None);
struct Attempt {
    id: String,
    edition: Edition,
    verifier: String,
    nonce: String,
    machine: String,
    expires: i64,
    cancel: CancellationToken,
    outcome: Mutex<Option<Result<Credentials, String>>>,
}
fn url(edition: Edition, verifier: &str, nonce: &str, machine: &str) -> Result<String, String> {
    let mut url = url::Url::parse(&format!("{}/device/selectAccounts", edition.web()))
        .map_err(|_| "accountError.format")?;
    url.query_pairs_mut()
        .append_pair(
            "challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier)),
        )
        .append_pair("challenge_method", "S256")
        .append_pair("nonce", nonce)
        .append_pair("machine_id", machine)
        .append_pair("client_id", edition.client_id());
    if edition == Edition::Qoder {
        url.query_pairs_mut()
            .append_pair("redirect_uri", "qoder-app://");
    }
    Ok(url.into())
}
pub(super) async fn start(edition: Edition) -> Result<LoginStart, String> {
    let mut random = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut random);
    let attempt = Arc::new(Attempt {
        id: uuid::Uuid::new_v4().to_string(),
        edition,
        verifier: URL_SAFE_NO_PAD.encode(random),
        nonce: uuid::Uuid::new_v4().to_string(),
        machine: uuid::Uuid::new_v4().to_string(),
        expires: chrono::Utc::now().timestamp() + 60,
        cancel: CancellationToken::new(),
        outcome: Mutex::new(None),
    });
    let result = LoginStart {
        login_id: attempt.id.clone(),
        verification_uri: url(edition, &attempt.verifier, &attempt.nonce, &attempt.machine)?,
        expires_at: attempt.expires,
    };
    {
        let mut pending = PENDING.lock().map_err(|_| "accountError.read")?;
        if let Some(previous) = pending.replace(attempt.clone()) {
            previous.cancel.cancel();
        }
    }
    tokio::spawn(async move {
        let remaining = (attempt.expires - chrono::Utc::now().timestamp()).max(0) as u64;
        let result = tokio::select! {
            _ = attempt.cancel.cancelled() => Err("accountError.cancelled".into()),
            _ = tokio::time::sleep(Duration::from_secs(remaining)) => Err("accountError.expired".into()),
            result = authorize(&attempt) => result,
        };
        if let Ok(mut outcome) = attempt.outcome.lock() {
            *outcome = Some(result);
        }
    });
    Ok(result)
}
pub(super) fn poll(id: &str) -> Result<Option<Credentials>, String> {
    let pending = PENDING.lock().map_err(|_| "accountError.read")?;
    let attempt = pending
        .as_ref()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    check(attempt)?;
    let result = attempt
        .outcome
        .lock()
        .map_err(|_| "accountError.read")?
        .clone();
    result.transpose()
}
fn check(attempt: &Attempt) -> Result<(), String> {
    if attempt.cancel.is_cancelled() {
        return Err("accountError.cancelled".into());
    }
    if chrono::Utc::now().timestamp() >= attempt.expires {
        return Err("accountError.expired".into());
    }
    Ok(())
}
pub(super) fn complete<T>(id: &str, save: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.read")?;
    let attempt = pending
        .as_ref()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    check(attempt)?;
    let result = save()?;
    pending.take();
    Ok(result)
}
pub(super) fn cancel(id: &str) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.read")?;
    if pending.as_ref().is_some_and(|p| p.id == id) {
        if let Some(attempt) = pending.take() {
            attempt.cancel.cancel();
        }
    }
    Ok(())
}
async fn authorize(attempt: &Attempt) -> Result<Credentials, String> {
    let client = client()?;
    let device = loop {
        let response = client
            .get(format!(
                "{}/api/v1/deviceToken/poll",
                attempt.edition.openapi()
            ))
            .query(&[
                ("nonce", attempt.nonce.as_str()),
                ("verifier", attempt.verifier.as_str()),
                ("challenge_method", "S256"),
            ])
            .send()
            .await;
        if let Ok(response) = response {
            if response.status().is_success() {
                if let Ok(value) = response.json::<Value>().await {
                    if field(&value, "token").is_some() {
                        break value;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    };
    exchange(
        &client,
        attempt.edition,
        attempt.edition.openapi(),
        &device,
        &attempt.machine,
    )
    .await
}
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}
fn token_expiry(value: &Value, device: bool, now: i64) -> i64 {
    if device {
        if let Some(time) = super::quota::timestamp(&value["expires_at"]) {
            return time.saturating_mul(1000);
        }
    }
    let lifetime = value["expires_in"]
        .as_i64()
        .filter(|n| *n > 0)
        .map(|n| if device { n.saturating_mul(1000) } else { n })
        .unwrap_or(86_400_000);
    now.saturating_add(lifetime)
}
async fn exchange(
    client: &reqwest::Client,
    edition: Edition,
    base: &str,
    device: &Value,
    machine: &str,
) -> Result<Credentials, String> {
    let access = field(device, "token").ok_or("accountError.invalidAccount")?;
    let refresh = field(device, "refresh_token").ok_or("accountError.invalidAccount")?;
    let user_id = field(device, "user_id").ok_or("accountError.invalidAccount")?;
    let response = client
        .post(format!("{base}/api/v1/me/jobToken"))
        .bearer_auth(access)
        .json(&json!({"clientId":edition.client_id()}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let status = response.status();
    let device_chat = edition == Edition::QoderCn && status.is_client_error();
    let now = chrono::Utc::now().timestamp_millis();
    let (chat_access, chat_refresh, expires) = if status.is_success() {
        let value = response
            .json::<Value>()
            .await
            .map_err(|_| "accountError.format")?;
        (
            field(&value, "token")
                .ok_or("accountError.invalidAccount")?
                .to_string(),
            field(&value, "refresh_token")
                .ok_or("accountError.invalidAccount")?
                .to_string(),
            token_expiry(&value, false, now),
        )
    } else if device_chat {
        (
            access.into(),
            refresh.into(),
            token_expiry(device, true, now),
        )
    } else {
        return Err(status_error(status));
    };
    let mut name = field(device, "user_name").unwrap_or("").to_string();
    let mut email = String::new();
    if let Ok(response) = client
        .get(format!("{base}/api/v1/userinfo"))
        .bearer_auth(access)
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(value) = response.json::<Value>().await {
                email = field(&value, "email").unwrap_or("").into();
                if let Some(value) = field(&value, "name") {
                    name = value.into();
                }
            }
        }
    }
    Ok(Credentials {
        edition,
        access_token: chat_access,
        refresh_token: chat_refresh,
        expires_at: expires,
        user_id: user_id.into(),
        name,
        email,
        machine_id: machine.into(),
        device_token: access.into(),
        device_refresh: refresh.into(),
        device_expires_at: token_expiry(device, true, now),
        device_chat,
        native_device_only: false,
    })
}
fn status_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        401 | 403 => "accountError.loginRequired".into(),
        429 => "accountError.rateLimited".into(),
        _ => "accountError.network".into(),
    }
}
pub(super) async fn refresh_at(
    client: &reqwest::Client,
    c: &Credentials,
    base: &str,
    device: bool,
) -> Result<Credentials, String> {
    let refresh = if device {
        &c.device_refresh
    } else {
        &c.refresh_token
    };
    let kind = if device { "deviceToken" } else { "jobToken" };
    let response = client
        .post(format!("{base}/api/v1/{kind}/refresh"))
        .json(&json!({"refresh_token":refresh}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !response.status().is_success() {
        if device && !c.device_chat && matches!(response.status().as_u16(), 401 | 403) {
            return Err("accountError.network|Qoder account-page authorization expired".into());
        }
        return Err(status_error(response.status()));
    }
    let value = response
        .json::<Value>()
        .await
        .map_err(|_| "accountError.format")?;
    let token = field(&value, "token")
        .or_else(|| field(&value, "device_token"))
        .ok_or("accountError.invalidAccount")?;
    let refresh = field(&value, "refresh_token").ok_or("accountError.invalidAccount")?;
    let expiry = token_expiry(&value, device, chrono::Utc::now().timestamp_millis());
    let mut next = c.clone();
    if device {
        next.device_token = token.into();
        next.device_refresh = refresh.into();
        next.device_expires_at = expiry;
    }
    if !device || c.device_chat {
        next.access_token = token.into();
        next.refresh_token = refresh.into();
        next.expires_at = expiry;
    }
    Ok(next)
}

pub(super) async fn exchange_native(
    client: &reqwest::Client,
    c: &Credentials,
    base: &str,
) -> Result<Credentials, String> {
    // A native device session is not a job token. Derive the latter only on explicit use.
    exchange(client,c.edition,base,&json!({"token":c.device_token,"refresh_token":c.device_refresh,"user_id":c.user_id,"expires_at":c.device_expires_at / 1000}),&c.machine_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_authorization_uses_region_and_pkce_without_cross_region_redirects() {
        for edition in [Edition::Qoder, Edition::QoderCn] {
            let uri =
                url::Url::parse(&url(edition, "verifier", "nonce", "machine").unwrap()).unwrap();
            assert_eq!(
                uri.host_str(),
                Some(if edition == Edition::Qoder {
                    "qoder.com"
                } else {
                    "qoder.cn"
                })
            );
            let pairs = uri
                .query_pairs()
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(
                pairs["challenge"],
                URL_SAFE_NO_PAD.encode(Sha256::digest("verifier"))
            );
            assert_eq!(
                pairs.contains_key("redirect_uri"),
                edition == Edition::Qoder
            );
            assert_eq!(pairs["client_id"], edition.client_id());
        }
    }
    #[test]
    fn job_and_device_expiry_units_are_distinct() {
        assert_eq!(token_expiry(&json!({"expires_in":60}), false, 1000), 1060);
        assert_eq!(token_expiry(&json!({"expires_in":60}), true, 1000), 61000);
        assert_eq!(
            token_expiry(&json!({"expires_at":"2030-01-01T00:00:00Z"}), true, 1000),
            1_893_456_000_000
        );
    }
    #[tokio::test]
    async fn cn_device_fallback_and_refresh_keep_job_and_quota_pairs_separate() {
        use axum::{
            routing::{get, post},
            Json, Router,
        };
        let app = Router::new()
            .route(
                "/api/v1/me/jobToken",
                post(|| async { reqwest::StatusCode::FORBIDDEN }),
            )
            .route(
                "/api/v1/userinfo",
                get(|| async { Json(json!({"name":"account","email":"mail"})) }),
            )
            .route(
                "/api/v1/deviceToken/refresh",
                post(|| async {
                    Json(
                        json!({"token":"new-device","refresh_token":"new-refresh","expires_in":60}),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = client().unwrap();
        let device = json!({"token":"device","refresh_token":"device-refresh","user_id":"id","expires_in":60});
        assert!(exchange(&client, Edition::Qoder, &base, &device, "machine")
            .await
            .is_err());
        let mut c = exchange(&client, Edition::QoderCn, &base, &device, "machine")
            .await
            .unwrap();
        assert!(c.device_chat);
        let next = refresh_at(&client, &c, &base, true).await.unwrap();
        assert_eq!(next.access_token, "new-device");
        assert_eq!(next.device_token, "new-device");
        c.device_chat = false;
        c.access_token = "job".into();
        c.refresh_token = "job-refresh".into();
        let next = refresh_at(&client, &c, &base, true).await.unwrap();
        assert_eq!(next.access_token, "job");
        assert_eq!(next.refresh_token, "job-refresh");
        assert_eq!(next.device_token, "new-device");
        server.abort();
    }
    #[tokio::test]
    async fn cancellation_replacement_and_expiry_cannot_save_late_credentials() {
        let attempt = Arc::new(Attempt {
            id: "expired".into(),
            edition: Edition::Qoder,
            verifier: String::new(),
            nonce: String::new(),
            machine: String::new(),
            expires: chrono::Utc::now().timestamp() + 60,
            cancel: CancellationToken::new(),
            outcome: Mutex::new(None),
        });
        PENDING.lock().unwrap().replace(attempt.clone());
        cancel("expired").unwrap();
        assert!(matches!(poll("expired"),Err(e) if e=="accountError.cancelled"));
        assert_eq!(
            complete("expired", || Ok(())).unwrap_err(),
            "accountError.cancelled"
        );
        assert!(attempt.cancel.is_cancelled());
        let ready = Arc::new(Attempt {
            id: "ready".into(),
            edition: Edition::Qoder,
            verifier: String::new(),
            nonce: String::new(),
            machine: String::new(),
            expires: chrono::Utc::now().timestamp() + 60,
            cancel: CancellationToken::new(),
            outcome: Mutex::new(None),
        });
        PENDING.lock().unwrap().replace(ready);
        assert_eq!(
            complete("ready", || Err::<(), _>("accountError.write".into())).unwrap_err(),
            "accountError.write"
        );
        assert!(PENDING
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|attempt| attempt.id == "ready"));
        complete("ready", || Ok(())).unwrap();
        assert!(PENDING.lock().unwrap().is_none());
        let expired = Attempt {
            expires: chrono::Utc::now().timestamp() - 1,
            ..Attempt {
                id: "old".into(),
                edition: Edition::Qoder,
                verifier: String::new(),
                nonce: String::new(),
                machine: String::new(),
                expires: 0,
                cancel: CancellationToken::new(),
                outcome: Mutex::new(None),
            }
        };
        assert_eq!(check(&expired).unwrap_err(), "accountError.expired");
    }
}
