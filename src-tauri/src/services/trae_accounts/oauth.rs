use super::{client, native, response, Credentials, Edition, Identity, LoginStart};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use rand::RngCore;
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

const EXCHANGE: &str = "/trae/api/v3/oauth/ExchangeToken";
static PENDING: Mutex<Option<Arc<Attempt>>> = Mutex::new(None);
static START_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Attempt {
    id: String,
    edition: Edition,
    verifier: String,
    identity: Identity,
    expires: i64,
    cancel: CancellationToken,
    outcome: Mutex<Outcome>,
}

enum Outcome {
    Waiting,
    Exchanging,
    Done(Result<Credentials, String>),
}

fn authorization(
    edition: Edition,
    callback: &str,
    id: &str,
    verifier: &str,
    identity: &Identity,
) -> Result<String, String> {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse(&format!("{}/authorization", edition.origin()))
        .map_err(|_| "accountError.auth")?;
    url.query_pairs_mut().extend_pairs([
        ("login_version", "1"),
        ("auth_from", "trae"),
        ("login_channel", "native_ide"),
        ("plugin_version", "local"),
        ("auth_type", "local"),
        ("client_id", identity.client_id.as_str()),
        ("redirect", "0"),
        ("login_trace_id", id),
        ("auth_callback_url", callback),
        ("machine_id", identity.machine_id.as_str()),
        ("device_id", identity.device_id.as_str()),
        ("x_device_id", identity.device_id.as_str()),
        ("x_machine_id", identity.machine_id.as_str()),
        (
            "x_device_type",
            match std::env::consts::OS {
                "macos" => "mac",
                value => value,
            },
        ),
        ("x_app_version", identity.app_version.as_str()),
        ("x_app_type", "stable"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    Ok(url.to_string())
}

pub(super) async fn start(edition: Edition, identity: Identity) -> Result<LoginStart, String> {
    let _guard = START_LOCK.lock().await;
    if let Some(previous) = PENDING.lock().map_err(|_| "accountError.busy")?.take() {
        previous.cancel.cancel();
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "accountError.auth")?;
    let port = listener
        .local_addr()
        .map_err(|_| "accountError.auth")?
        .port();
    let id = uuid::Uuid::new_v4().to_string();
    let path = "/authorize";
    let callback = format!("http://127.0.0.1:{port}{path}");
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let verifier = URL_SAFE_NO_PAD.encode(random);
    let verification_uri = authorization(edition, &callback, &id, &verifier, &identity)?;
    let attempt = Arc::new(Attempt {
        id: id.clone(),
        edition,
        verifier,
        identity,
        expires: chrono::Utc::now().timestamp() + 60,
        cancel: CancellationToken::new(),
        outcome: Mutex::new(Outcome::Waiting),
    });
    let result = LoginStart {
        login_id: id,
        verification_uri,
        expires_at: attempt.expires,
    };
    *PENDING.lock().map_err(|_| "accountError.busy")? = Some(attempt.clone());
    let router = Router::new()
        .route(path, get(callback_request))
        .with_state(attempt.clone());
    let stop = attempt.cancel.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(stop.cancelled_owned())
            .await;
    });
    tokio::spawn(async move {
        tokio::select! {
            _ = attempt.cancel.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(60)) => attempt.cancel.cancel(),
        }
    });
    Ok(result)
}

fn callback_code(pairs: &[(String, String)], id: &str) -> Result<String, String> {
    let values = |key: &str| {
        pairs
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value)
            .collect::<Vec<_>>()
    };
    for key in ["login_trace_id", "loginTraceID"] {
        let traces = values(key);
        if traces.len() > 1 || traces.first().is_some_and(|value| value.as_str() != id) {
            return Err("accountError.state".into());
        }
    }
    if values("isRedirect")
        .iter()
        .any(|value| value.as_str() == "false")
    {
        return Err("accountError.auth".into());
    }
    let mut codes = values("code").into_iter().cloned().collect::<Vec<_>>();
    codes.extend(values("authCode").into_iter().cloned());
    for encoded in values("authCodeInfo") {
        let value: Value =
            serde_json::from_str(encoded).map_err(|_| "accountError.authResponse")?;
        codes.push(
            native::text(&value, "AuthCode")
                .ok_or("accountError.authResponse")?
                .into(),
        );
    }
    if codes.len() != 1 || codes[0].is_empty() || codes[0].len() > 4096 {
        return Err("accountError.authResponse".into());
    }
    Ok(codes.remove(0))
}

async fn callback_request(
    State(attempt): State<Arc<Attempt>>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> Response {
    let code = match callback_code(&pairs, &attempt.id) {
        Ok(code) => code,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    {
        let Ok(mut outcome) = attempt.outcome.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if validate(&attempt).is_err() || !matches!(*outcome, Outcome::Waiting) {
            return StatusCode::GONE.into_response();
        }
        *outcome = Outcome::Exchanging;
    }
    let result = tokio::select! {
        _ = attempt.cancel.cancelled() => Err("accountError.cancelled".into()),
        result = exchange(&attempt, &code, &pairs) => result,
    };
    if validate(&attempt).is_err() {
        return StatusCode::GONE.into_response();
    }
    let success = result.is_ok();
    if let Ok(mut outcome) = attempt.outcome.lock() {
        *outcome = Outcome::Done(result);
    }
    if success {
        (
            StatusCode::OK,
            "Authorization complete. Return to EchoBird.",
        )
            .into_response()
    } else {
        StatusCode::BAD_REQUEST.into_response()
    }
}

fn device_info(identity: &Identity) -> Result<Value, String> {
    let pair = identity
        .key_pair
        .as_ref()
        .ok_or("accountError.initializeClient")?;
    Ok(json!({
        "DeviceID":identity.device_id,
        "MachineID":identity.machine_id,
        "PlatformCode":"IDE_PC",
        "DeviceType":"PC",
        "ClientVersion":identity.app_version,
        "DevicePublicKey":pair.public_key,
        "OSInfo":match std::env::consts::OS {"macos" => "mac", value => value},
    }))
}

async fn exchange(
    attempt: &Attempt,
    code: &str,
    pairs: &[(String, String)],
) -> Result<Credentials, String> {
    let client = client()?;
    let user_tag = pairs
        .iter()
        .find(|(key, _)| key == "userTag")
        .map(|(_, value)| value.as_str());
    let profile_hint: Value = pairs
        .iter()
        .find(|(key, _)| key == "userInfo")
        .and_then(|(_, value)| serde_json::from_str(value).ok())
        .unwrap_or(Value::Null);
    let base = match attempt.edition {
        Edition::Traecn => attempt.edition.api(),
        Edition::Trae if user_tag.is_some_and(|tag| tag.eq_ignore_ascii_case("usttp")) => {
            "https://grow-normal.traeapi.us"
        }
        Edition::Trae
            if native::text(&profile_hint, "AIRegion")
                .is_some_and(|region| ["SG", "US"].contains(&region)) =>
        {
            attempt.edition.api()
        }
        Edition::Trae => "https://grow-normal.trae.ai",
    };
    let payload = response(client.post(format!("{base}{EXCHANGE}"))
        .header("x-cloudide-token", "")
        .json(&json!({"ClientID":attempt.identity.client_id,"AuthCode":code,"CodeVerifier":attempt.verifier,"DeviceInfo":device_info(&attempt.identity)?,"IDEVersion":attempt.identity.app_version}))
        .send().await.map_err(|_| "accountError.network")?).await?;
    let token = native::text(&payload["Result"], "Token").ok_or("accountError.authResponse")?;
    let profile = response(
        client
            .post(format!("{base}/cloudide/api/v3/trae/GetUserInfo"))
            .header("x-cloudide-token", token)
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| "accountError.network")?,
    )
    .await?;
    let mut credentials = login_credentials(
        attempt.edition,
        &payload["Result"],
        &profile["Result"],
        attempt.identity.clone(),
    )?;
    credentials.auth["host"] = base.into();
    if let Some(tag) = user_tag {
        credentials.auth["account"]["userTag"] = tag.to_ascii_lowercase().into();
    }
    Ok(credentials)
}

fn login_credentials(
    edition: Edition,
    token: &Value,
    profile: &Value,
    identity: Identity,
) -> Result<Credentials, String> {
    let region = native::text(profile, "AIRegion").unwrap_or(match edition {
        Edition::Trae => "SG",
        Edition::Traecn => "CN",
    });
    let user = native::text(profile, "UserID").ok_or("accountError.authResponse")?;
    let now = chrono::Utc::now().to_rfc3339();
    let auth = json!({
        "token":native::text(token,"Token").ok_or("accountError.authResponse")?,
        "refreshToken":native::text(token,"RefreshToken").ok_or("accountError.authResponse")?,
        "expiredAt":token["TokenExpireAt"],
        "refreshExpiredAt":token["RefreshExpireAt"],
        "tokenReleaseAt":now,
        "userId":user,
        "host":edition.api(),
        "userRegion":{"region":region,"_aiRegion":region},
        "account":{
            "username":profile["ScreenName"],
            "email":profile["NonPlainTextEmail"],
            "avatar_url":profile["AvatarUrl"],
            "description":profile["Description"],
            "nonPlainTextMobile":profile["NonPlainTextMobile"],
            "scope":"marscode",
            "loginScope":"trae",
            "storeCountryCode":profile["StoreCountry"],
            "storeCountrySrc":profile["StoreCountrySrc"],
            "storeRegion":region,
            "userTag":match edition {Edition::Traecn => "cn",Edition::Trae => "row"},
        },
    });
    if native::region(&auth) != Some(edition) || native::timestamp(&auth["expiredAt"]).is_none() {
        return Err("accountError.authResponse".into());
    }
    Ok(Credentials { auth, identity })
}

fn validate(attempt: &Attempt) -> Result<(), String> {
    if chrono::Utc::now().timestamp() >= attempt.expires {
        return Err("accountError.expired".into());
    }
    if attempt.cancel.is_cancelled() {
        return Err("accountError.cancelled".into());
    }
    Ok(())
}

pub(super) fn poll(id: &str) -> Result<Option<(Edition, Credentials)>, String> {
    let pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    let attempt = pending
        .as_ref()
        .filter(|attempt| attempt.id == id)
        .ok_or("accountError.cancelled")?;
    validate(attempt)?;
    let outcome = attempt.outcome.lock().map_err(|_| "accountError.busy")?;
    match &*outcome {
        Outcome::Waiting | Outcome::Exchanging => Ok(None),
        Outcome::Done(result) => result
            .clone()
            .map(|credential| Some((attempt.edition, credential))),
    }
}

pub(super) fn complete<T>(id: &str, save: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    let attempt = pending
        .as_ref()
        .filter(|attempt| attempt.id == id)
        .ok_or("accountError.cancelled")?;
    validate(attempt)?;
    let result = save()?;
    attempt.cancel.cancel();
    *pending = None;
    Ok(result)
}

pub(super) fn cancel(id: &str) -> Result<(), String> {
    let mut pending = PENDING.lock().map_err(|_| "accountError.busy")?;
    if pending.as_ref().is_some_and(|attempt| attempt.id == id) {
        if let Some(attempt) = pending.take() {
            attempt.cancel.cancel();
        }
    }
    Ok(())
}

pub(super) fn host(edition: Edition, auth: &Value) -> Result<String, String> {
    let raw = native::text(auth, "host").unwrap_or(edition.api());
    let url = url::Url::parse(raw).map_err(|_| "accountError.invalidAccount")?;
    let hostname = url.host_str().ok_or("accountError.invalidAccount")?;
    let allowed = match edition {
        Edition::Traecn => hostname == "api.trae.cn" || hostname == "api.trae.com.cn",
        Edition::Trae => [
            "grow-normal.trae.ai",
            "growsg-normal.trae.ai",
            "growva-normal.trae.ai",
            "grow-normal.traeapi.us",
        ]
        .contains(&hostname),
    };
    if url.scheme() != "https" || !allowed {
        return Err("accountError.invalidAccount".into());
    }
    Ok(format!("https://{hostname}"))
}

fn proof(identity: &Identity, refresh: &str, timestamp: i64, nonce: &str) -> Result<Value, String> {
    let pair = identity
        .key_pair
        .as_ref()
        .ok_or("accountError.initializeClient")?;
    let pem = pair
        .private_key
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    let der = STANDARD
        .decode(pem)
        .map_err(|_| "accountError.invalidAccount")?;
    let random = SystemRandom::new();
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &der, &random)
        .map_err(|_| "accountError.invalidAccount")?;
    let message = format!(
        "POST\n{EXCHANGE}\n{}\n{refresh}\n{timestamp}\n{nonce}",
        identity.client_id
    );
    let signature = key
        .sign(&random, message.as_bytes())
        .map_err(|_| "accountError.auth")?;
    Ok(json!({"Signature":STANDARD.encode(signature.as_ref()),"Timestamp":timestamp,"Nonce":nonce}))
}

pub(super) async fn ensure_fresh(
    edition: Edition,
    credentials: &mut Credentials,
) -> Result<(), String> {
    ensure_fresh_with_force(edition, credentials, false)
        .await
        .map(|_| ())
}

fn refresh_required(auth: &Value, force: bool, now: i64) -> bool {
    force || native::timestamp(&auth["expiredAt"]).is_none_or(|expiry| expiry <= now + 60)
}

pub(super) async fn ensure_fresh_with_force(
    edition: Edition,
    credentials: &mut Credentials,
    force: bool,
) -> Result<bool, String> {
    let now = chrono::Utc::now().timestamp();
    if !refresh_required(&credentials.auth, force, now) {
        return Ok(false);
    }
    let refresh =
        native::text(&credentials.auth, "refreshToken").ok_or("accountError.invalidAccount")?;
    if native::timestamp(&credentials.auth["refreshExpiredAt"]).is_some_and(|expiry| expiry <= now)
    {
        return Err("accountError.expired".into());
    }
    let client = client()?;
    let base = host(edition, &credentials.auth)?;
    let payload = if credentials.identity.key_pair.is_some() {
        let mut nonce = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let body = json!({"ClientID":credentials.identity.client_id,"ClientSecret":"","RefreshToken":refresh,"DeviceInfo":device_info(&credentials.identity)?,"DeviceProof":proof(&credentials.identity,refresh,now,&hex::encode(nonce))?,"IDEVersion":credentials.identity.app_version});
        response(
            client
                .post(format!("{base}{EXCHANGE}"))
                .header("x-cloudide-token", native::access(&credentials.auth)?)
                .json(&body)
                .send()
                .await
                .map_err(|_| "accountError.network")?,
        )
        .await?
    } else {
        response(client.post(format!("{base}/cloudide/api/v3/trae/oauth/ExchangeToken")).json(&json!({"ClientID":credentials.identity.client_id,"ClientSecret":"-","RefreshToken":refresh,"UserID":credentials.auth["userId"]})).send().await.map_err(|_| "accountError.network")?).await?
    };
    let token = &payload["Result"];
    let access = native::text(token, "Token").ok_or("accountError.authResponse")?;
    if native::timestamp(&token["TokenExpireAt"]).is_none() {
        return Err("accountError.authResponse".into());
    }
    credentials.auth["token"] = access.into();
    if credentials.auth.get("accessToken").is_some() {
        credentials.auth["accessToken"] = access.into();
    }
    credentials.auth["expiredAt"] = token["TokenExpireAt"].clone();
    if let Some(refresh) = native::text(token, "RefreshToken") {
        credentials.auth["refreshToken"] = refresh.into();
    }
    if native::timestamp(&token["RefreshExpireAt"]).is_some() {
        credentials.auth["refreshExpiredAt"] = token["RefreshExpireAt"].clone();
    }
    credentials.auth["tokenReleaseAt"] = chrono::Utc::now().to_rfc3339().into();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_refresh_respects_expiry_and_forced_rejection_without_network() {
        let now = 1791774000;
        let valid = json!({"expiredAt": now + 3600});
        assert!(!refresh_required(&valid, false, now));
        assert!(refresh_required(&valid, true, now));
        assert!(refresh_required(
            &json!({"expiredAt": now + 60}),
            false,
            now
        ));
        assert!(refresh_required(&json!({"expiredAt": now - 1}), false, now));
        assert!(refresh_required(&json!({}), false, now));
    }

    fn identity() -> Identity {
        Identity {
            device_id: "device".into(),
            machine_id: "machine".into(),
            app_version: "3".into(),
            client_id: "client".into(),
            key_pair: None,
        }
    }

    #[test]
    fn pkce_authorization_and_callback_use_attempt_identity() {
        let callback_pattern = regex::Regex::new(r"^http://127\.0\.0\.1:(\d+)/authorize$").unwrap();
        for edition in [Edition::Traecn, Edition::Trae] {
            let url = authorization(
                edition,
                "http://127.0.0.1:1234/authorize",
                "attempt",
                "verifier",
                &identity(),
            )
            .unwrap();
            let url = url::Url::parse(&url).unwrap();
            let pairs = url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(url.origin().ascii_serialization(), edition.origin());
            assert_eq!(pairs["code_challenge_method"], "S256");
            assert_eq!(pairs["device_id"], "device");
            assert_eq!(pairs["login_trace_id"], "attempt");
            assert_eq!(
                pairs["auth_callback_url"],
                "http://127.0.0.1:1234/authorize"
            );
            assert!(callback_pattern.is_match(&pairs["auth_callback_url"]));
            assert!(!callback_pattern.is_match("http://127.0.0.1:1234/authorize/attempt"));
            assert!(!callback_pattern.is_match("http://127.0.0.1:1234/authorize?state=attempt"));
            assert_eq!(
                pairs["x_device_type"],
                match std::env::consts::OS {
                    "macos" => "mac",
                    value => value,
                }
            );
            assert!(!url.as_str().contains("verifier"));
        }
        assert_eq!(
            callback_code(
                &[("authCodeInfo".into(), "{\"AuthCode\":\"code\"}".into())],
                "attempt"
            )
            .unwrap(),
            "code"
        );
        assert!(callback_code(
            &[
                ("code".into(), "code".into()),
                ("login_trace_id".into(), "other".into())
            ],
            "attempt"
        )
        .is_err());
        assert!(callback_code(
            &[("code".into(), "one".into()), ("code".into(), "two".into())],
            "attempt"
        )
        .is_err());
    }

    #[tokio::test]
    async fn replacing_and_cancelling_login_rejects_late_completion() {
        let first = start(Edition::Traecn, identity()).await.unwrap();
        let second = start(Edition::Trae, identity()).await.unwrap();
        let callback_pattern = regex::Regex::new(r"^http://127\.0\.0\.1:(\d+)/authorize$").unwrap();
        let mut second_callback = String::new();
        for login in [&first, &second] {
            let url = url::Url::parse(&login.verification_uri).unwrap();
            let pairs = url
                .query_pairs()
                .collect::<std::collections::HashMap<_, _>>();
            assert_eq!(pairs["login_trace_id"], login.login_id);
            assert!(callback_pattern.is_match(&pairs["auth_callback_url"]));
            let callback = url::Url::parse(&pairs["auth_callback_url"]).unwrap();
            assert_eq!(callback.path(), "/authorize");
            assert!(callback.port().is_some_and(|port| port != 0));
            second_callback = callback.to_string();
        }
        assert_eq!(
            reqwest::get(&second_callback).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        assert!(poll(&first.login_id).is_err());
        cancel(&first.login_id).unwrap();
        assert!(poll(&second.login_id).unwrap().is_none());
        cancel(&second.login_id).unwrap();
        let wrote = std::cell::Cell::new(false);
        assert!(complete(&second.login_id, || {
            wrote.set(true);
            Ok(())
        })
        .is_err());
        assert!(!wrote.get());
    }

    #[test]
    fn successful_login_uses_native_shape_and_rejects_other_region() {
        let token = json!({"Token":"fixture-token","RefreshToken":"fixture-refresh","TokenExpireAt":"2030-01-01T00:00:00Z"});
        let profile = json!({"UserID":"user","AIRegion":"CN","NonPlainTextEmail":"test@example.test","ScreenName":"name"});
        let auth = login_credentials(Edition::Traecn, &token, &profile, identity())
            .unwrap()
            .auth;
        assert_eq!(auth["account"]["scope"], "marscode");
        assert_eq!(auth["account"]["email"], "test@example.test");
        assert!(login_credentials(Edition::Trae, &token, &profile, identity()).is_err());
        assert!(host(Edition::Traecn, &json!({"host":"https://other.example"})).is_err());
    }

    #[test]
    fn device_proof_is_der_p256_over_newline_contract() {
        use ring::signature::{KeyPair, UnparsedPublicKey, ECDSA_P256_SHA256_ASN1};
        let random = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &random).unwrap();
        let pair =
            EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &random)
                .unwrap();
        let mut identity = identity();
        identity.key_pair = Some(native::KeyPair {
            private_key: format!(
                "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----",
                STANDARD.encode(pkcs8.as_ref())
            ),
            public_key: "fixture-public-key".into(),
        });
        let proof = proof(&identity, "refresh", 1791774000, "nonce").unwrap();
        let signature = STANDARD
            .decode(proof["Signature"].as_str().unwrap())
            .unwrap();
        let verifier = UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, pair.public_key().as_ref());
        verifier
            .verify(
                b"POST\n/trae/api/v3/oauth/ExchangeToken\nclient\nrefresh\n1791774000\nnonce",
                &signature,
            )
            .unwrap();
        assert!(verifier
            .verify(
                b"POST /trae/api/v3/oauth/ExchangeToken client refresh 1791774000 nonce",
                &signature
            )
            .is_err());
    }
}
