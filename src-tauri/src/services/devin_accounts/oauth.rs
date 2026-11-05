use super::{Account, Credentials, SERVER};
use axum::{
    extract::{Query, State},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    future::IntoFuture,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::oneshot;

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}
type LoginResult = Result<(Credentials, Account), String>;
struct Pending {
    id: String,
    expires: i64,
    result: oneshot::Receiver<LoginResult>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.task.abort();
    }
}
struct Flow {
    state: String,
    verifier: String,
    redirect: String,
    busy: AtomicBool,
    sender: Mutex<Option<oneshot::Sender<LoginResult>>>,
}
async fn callback(
    State(flow): State<Arc<Flow>>,
    Query(q): Query<HashMap<String, String>>,
) -> (axum::http::StatusCode, &'static str) {
    use axum::http::StatusCode;
    if q.get("state") != Some(&flow.state) || q.get("code").is_none_or(|s| s.is_empty()) {
        return (StatusCode::BAD_REQUEST, "Invalid sign-in callback");
    }
    if flow.busy.swap(true, Ordering::SeqCst) {
        return (StatusCode::CONFLICT, "Sign-in already received");
    }
    let result = exchange(&flow, q.get("code").expect("checked code")).await;
    let ok = result.is_ok();
    if let Ok(mut tx) = flow.sender.lock() {
        if let Some(tx) = tx.take() {
            let _ = tx.send(result);
        }
    }
    (
        if ok {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        },
        if ok {
            "Signed in to EchoBird. You can close this page."
        } else {
            "Sign-in failed. Return to EchoBird to retry."
        },
    )
}
async fn exchange(flow: &Flow, code: &str) -> LoginResult {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network")?;
    let r = client
        .post(format!(
            "{SERVER}/exa.seat_management_pb.SeatManagementService/ExchangeDevinCLIPKCECode"
        ))
        .json(&json!({"code":code,"code_verifier":flow.verifier,"redirect_uri":flow.redirect}))
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    if !r.status().is_success() {
        return Err(format!("accountError.auth|HTTP {}", r.status().as_u16()));
    }
    let v: Value = r.json().await.map_err(|_| "accountError.authResponse")?;
    let token = v["sessionToken"]
        .as_str()
        .or_else(|| v["session_token"].as_str())
        .ok_or("accountError.authResponse")?;
    let user = super::user_status(token, SERVER).await?;
    super::from_user(token, SERVER, &user, true)
}
pub(super) async fn start() -> Result<LoginStart, String> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "accountError.auth")?;
    let redirect = format!(
        "http://127.0.0.1:{}/callback",
        listener
            .local_addr()
            .map_err(|_| "accountError.auth")?
            .port()
    );
    let mut random = [0; 48];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let verifier = URL_SAFE_NO_PAD.encode(random);
    let id = uuid::Uuid::new_v4().to_string();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let (tx, result) = oneshot::channel();
    let flow = Arc::new(Flow {
        state: id.clone(),
        verifier,
        redirect: redirect.clone(),
        busy: AtomicBool::new(false),
        sender: Mutex::new(Some(tx)),
    });
    let app = Router::new()
        .route("/callback", get(callback))
        .with_state(flow);
    let task = tokio::spawn(async move {
        let _ = tokio::time::timeout(
            Duration::from_secs(60),
            axum::serve(listener, app).into_future(),
        )
        .await;
    });
    let expires = chrono::Utc::now().timestamp() + 60;
    let mut url = url::Url::parse("https://app.devin.ai/auth/cli/continue")
        .map_err(|_| "accountError.auth")?;
    url.query_pairs_mut().extend_pairs([
        ("redirect_uri", redirect.as_str()),
        ("state", id.as_str()),
        ("prompt", "select_account"),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
        ("cli_pkce_marker", "1"),
    ]);
    *PENDING.lock().map_err(|_| "accountError.busy")? = Some(Pending {
        id: id.clone(),
        expires,
        result,
        task,
    });
    Ok(LoginStart {
        login_id: id,
        verification_uri: url.into(),
        expires_at: expires,
    })
}
pub(super) fn cancel(id: &str) -> Result<(), String> {
    let mut p = PENDING.lock().map_err(|_| "accountError.busy")?;
    if p.as_ref().is_some_and(|p| p.id == id) {
        p.take();
    }
    Ok(())
}
pub(super) fn complete(
    id: &str,
    save: impl FnOnce(Credentials, Account) -> Result<Account, String>,
) -> Result<Option<Account>, String> {
    let mut slot = PENDING.lock().map_err(|_| "accountError.busy")?;
    let p = slot
        .as_mut()
        .filter(|p| p.id == id)
        .ok_or("accountError.cancelled")?;
    if chrono::Utc::now().timestamp() >= p.expires {
        slot.take();
        return Err("accountError.expired".into());
    }
    match p.result.try_recv() {
        Ok(result) => {
            slot.take();
            let (c, a) = result?;
            let a = save(c, a)?;
            Ok(Some(a))
        }
        Err(oneshot::error::TryRecvError::Empty) => Ok(None),
        Err(_) => {
            slot.take();
            Err("accountError.auth".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pkce_state_cancel_replacement_expiry_and_terminal_failure() {
        let first = start().await.unwrap();
        let url = url::Url::parse(&first.verification_uri).unwrap();
        let q: HashMap<_, _> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"].len(), 43);
        let callback = format!("{}?state=wrong&code=fixture", q["redirect_uri"]);
        assert_eq!(reqwest::get(callback).await.unwrap().status(), 400);
        assert!(complete(&first.login_id, |_, _| panic!("must not save"))
            .unwrap()
            .is_none());
        let second = start().await.unwrap();
        cancel(&first.login_id).unwrap();
        assert!(complete(&first.login_id, |_, _| panic!("must not save")).is_err());
        assert!(complete(&second.login_id, |_, _| panic!("must not save"))
            .unwrap()
            .is_none());
        PENDING.lock().unwrap().as_mut().unwrap().expires = 0;
        assert_eq!(
            complete(&second.login_id, |_, _| panic!("must not save")).unwrap_err(),
            "accountError.expired"
        );
        assert!(PENDING.lock().unwrap().is_none());
        let failed = start().await.unwrap();
        let (tx, rx) = oneshot::channel();
        PENDING.lock().unwrap().as_mut().unwrap().result = rx;
        tx.send(Err("accountError.network".into())).ok().unwrap();
        assert_eq!(
            complete(&failed.login_id, |_, _| panic!("must not save")).unwrap_err(),
            "accountError.network"
        );
        assert!(PENDING.lock().unwrap().is_none());
        let cancelled = start().await.unwrap();
        cancel(&cancelled.login_id).unwrap();
        assert!(PENDING.lock().unwrap().is_none());
    }
}
