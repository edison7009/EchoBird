//! CodeArts desktop authorization, native credentials and explicit credit operations.
use base64::{
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    Engine,
};
use chrono::Datelike;
use ring::signature::KeyPair;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub(crate) mod native;
pub(crate) async fn import_native() -> Result<Option<bool>, String> {
    let _guard = ACCOUNTS.lock().await;
    let Some(native) = native::read()? else {
        return Ok(None);
    };
    let account = account(&native)?;
    let mut rows = load()?;
    if rows.values().any(|s| {
        super::auto_import::native::same_account(
            &account.id,
            &account.email,
            &s.account.id,
            &s.account.email,
        )
    }) {
        return Ok(Some(false));
    }
    rows.insert(account.id.clone(), Saved { account, native });
    save(&rows)?;
    Ok(Some(true))
}

pub(crate) fn validate_backup_record(value: &serde_json::Value) -> Result<(), String> {
    let saved: Stored = serde_json::from_value(value.clone()).map_err(|_| "backupError.format")?;
    let native: serde_json::Value =
        serde_json::from_str(&saved.session).map_err(|_| "backupError.format")?;
    if account(&native)?.id != saved.account.id {
        return Err("backupError.format".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;

const STS: &str = "https://sts.cn-north-4.myhuaweicloud.com";
const PORTAL: &str = "https://codearts.huaweicloud.com/portal";
const POLICY: &str = "https://snap-access.cn-north-4.myhuaweicloud.com";
static ACCOUNTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CLAIM: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LOGIN: Mutex<Option<Pending>> = Mutex::new(None);
const TOOL_ID: &str = "codearts";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub tool_id: String,
    pub plan: Option<String>,
    pub balance: Option<f64>,
    #[serde(default)]
    pub total: Option<f64>,
    #[serde(default)]
    pub subscription_end_at: Option<i64>,
    pub active: bool,
    #[serde(default)]
    pub daily_claimed_at: Option<i64>,
}
#[derive(Clone)]
struct Saved {
    account: Account,
    native: Value,
}
#[derive(Serialize, Deserialize)]
struct Stored {
    account: Account,
    session: String,
}
#[derive(Clone)]
struct Context {
    verifier: String,
    private: Value,
    public: Value,
    redirect_uri: String,
}
#[derive(Clone)]
struct Pending {
    id: String,
    context: Context,
    expires: i64,
    cancel: CancellationToken,
    result: Arc<Mutex<Option<Result<Value, String>>>>,
    callback_used: Arc<AtomicBool>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginStart {
    pub login_id: String,
    pub verification_uri: String,
    pub expires_at: i64,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
fn store_path() -> Result<PathBuf, String> {
    Ok(dirs::home_dir()
        .ok_or("accountError.home")?
        .join(".echobird/codearts-accounts.json"))
}
fn desktop_record(id: &str, value: &Value) -> bool {
    id.starts_with("codearts:") && value["account"]["toolId"] == TOOL_ID
}
fn require_desktop_id(id: &str) -> Result<(), String> {
    if id
        .strip_prefix("codearts:")
        .is_some_and(|subject| !subject.is_empty())
    {
        Ok(())
    } else {
        Err("accountError.invalidAccount".into())
    }
}
fn read_store() -> Result<BTreeMap<String, Value>, String> {
    let path = store_path()?;
    if path.exists() {
        super::cursor_auth::read(&path)
    } else {
        Ok(BTreeMap::new())
    }
}
fn load() -> Result<BTreeMap<String, Saved>, String> {
    read_store()?
        .into_iter()
        .filter(|(id, value)| desktop_record(id, value))
        .map(|(id, value)| {
            let mut stored: Stored =
                serde_json::from_value(value).map_err(|_| "accountError.format")?;
            stored.account.plan = stored.account.plan.as_deref().map(account_plan_name);
            if id != stored.account.id || !stored.session.starts_with("enc:v1:") {
                return Err("accountError.invalidAccount".into());
            }
            let raw = super::model_manager::decrypt_key_for_use(&stored.session);
            if raw.is_empty() {
                return Err("accountError.keychain".into());
            }
            let native = serde_json::from_str(&raw).map_err(|_| "accountError.format")?;
            if account(&native)?.id != id {
                return Err("accountError.invalidAccount".into());
            }
            Ok((
                id,
                Saved {
                    account: stored.account,
                    native,
                },
            ))
        })
        .collect()
}
fn merge_store(
    mut existing: BTreeMap<String, Value>,
    updates: BTreeMap<String, Value>,
) -> Result<BTreeMap<String, Value>, String> {
    existing.retain(|id, row| !desktop_record(id, row) || updates.contains_key(id));
    for (id, row) in updates {
        if !desktop_record(&id, &row) {
            return Err("accountError.invalidAccount".into());
        }
        let previous = existing.entry(id).or_insert_with(|| json!({"account":{}}));
        let previous = previous.as_object_mut().ok_or("accountError.format")?;
        let account = previous
            .entry("account")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("accountError.format")?;
        account.extend(
            row["account"]
                .as_object()
                .ok_or("accountError.format")?
                .clone(),
        );
        previous.insert("session".into(), row["session"].clone());
    }
    Ok(existing)
}
fn save(rows: &BTreeMap<String, Saved>) -> Result<(), String> {
    let updates = rows
        .iter()
        .map(|(id, saved)| {
            require_desktop_id(id)?;
            if saved.account.tool_id != TOOL_ID {
                return Err("accountError.invalidAccount".into());
            }
            let raw = serde_json::to_string(&saved.native).map_err(|_| "accountError.format")?;
            let session = super::model_manager::encrypt_key_for_storage(&raw);
            if !session.starts_with("enc:v1:") {
                return Err("accountError.keychain".into());
            }
            let value = serde_json::to_value(Stored {
                account: saved.account.clone(),
                session,
            })
            .map_err(|_| "accountError.format")?;
            Ok((id.clone(), value))
        })
        .collect::<Result<_, String>>()?;
    super::cursor_auth::write(&store_path()?, &merge_store(read_store()?, updates)?)
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "accountError.authResponse".into())
}
fn account(native: &Value) -> Result<Account, String> {
    let session = &native["session"];
    if session.get("clientId").is_some_and(|id| id != TOOL_ID) {
        return Err("accountError.invalidAccount".into());
    }
    let user = string(session, "userId")?;
    let domain = string(session, "domainId")?;
    Ok(Account {
        id: format!("codearts:{:x}", Sha256::digest(format!("{domain}:{user}"))),
        email: session["userName"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or(user)
            .into(),
        tool_id: TOOL_ID.into(),
        plan: None,
        balance: None,
        total: None,
        subscription_end_at: None,
        active: false,
        daily_claimed_at: None,
    })
}

pub fn list_saved_accounts() -> Result<Vec<Account>, String> {
    Ok(load()?.into_values().map(|saved| saved.account).collect())
}

pub async fn list_accounts() -> Result<Vec<Account>, String> {
    let _guard = ACCOUNTS.lock().await;
    let rows = load()?;
    let native = if rows.is_empty() || super::tool_manager::get_tool_exe_path(TOOL_ID).is_none() {
        Ok(None)
    } else {
        native::read()
    };
    Ok(account_list(rows, native))
}

fn account_list(
    rows: BTreeMap<String, Saved>,
    native: Result<Option<Value>, String>,
) -> Vec<Account> {
    let active = native
        .ok()
        .flatten()
        .and_then(|native| account(&native).ok());
    rows.into_values()
        .map(|mut s| {
            s.account.active = active.as_ref().is_some_and(|a| a.id == s.account.id);
            s.account
        })
        .collect()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "accountError.network".into())
}
fn key_pair() -> Result<(Value, Value), String> {
    let rng = ring::rand::SystemRandom::new();
    let algorithm = &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING;
    let pkcs8 = ring::signature::EcdsaKeyPair::generate_pkcs8(algorithm, &rng)
        .map_err(|_| "accountError.auth")?;
    let key = ring::signature::EcdsaKeyPair::from_pkcs8(algorithm, pkcs8.as_ref(), &rng)
        .map_err(|_| "accountError.auth")?;
    let public = key.public_key().as_ref();
    let marker = [2, 1, 1, 4, 32];
    let offset = pkcs8
        .as_ref()
        .windows(marker.len())
        .position(|b| b == marker)
        .ok_or("accountError.auth")?
        + marker.len();
    let scalar = pkcs8
        .as_ref()
        .get(offset..offset + 32)
        .ok_or("accountError.auth")?;
    ring::signature::EcdsaKeyPair::from_private_key_and_public_key(algorithm, scalar, public, &rng)
        .map_err(|_| "accountError.auth")?;
    let public = json!({"kty":"EC","crv":"P-256","x":URL_SAFE_NO_PAD.encode(&public[1..33]),"y":URL_SAFE_NO_PAD.encode(&public[33..65])});
    let mut private = public.clone();
    private["d"] = json!(URL_SAFE_NO_PAD.encode(scalar));
    Ok((private, public))
}
fn dpop(context: &Context, url: &str) -> Result<String, String> {
    let decode = |key: &str| {
        URL_SAFE_NO_PAD
            .decode(string(&context.private, key)?)
            .map_err(|_| "accountError.auth".to_string())
    };
    let mut public = vec![4];
    public.extend(decode("x")?);
    public.extend(decode("y")?);
    let rng = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::from_private_key_and_public_key(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &decode("d")?,
        &public,
        &rng,
    )
    .map_err(|_| "accountError.auth")?;
    let header = URL_SAFE_NO_PAD
        .encode(json!({"alg":"ES256","typ":"dpop+jwt","jwk":context.public}).to_string());
    let payload = URL_SAFE_NO_PAD.encode(
        json!({"htm":"POST","htu":url,"iat":now(),"jti":uuid::Uuid::new_v4().simple().to_string()})
            .to_string(),
    );
    let input = format!("{header}.{payload}");
    let signature = key
        .sign(&rng, input.as_bytes())
        .map_err(|_| "accountError.auth")?;
    Ok(format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

pub async fn start_login() -> Result<LoginStart, String> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "accountError.auth")?;
    let port = listener
        .local_addr()
        .map_err(|_| "accountError.auth")?
        .port();
    let verifier = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let (private, public) = key_pair()?;
    let context = Context {
        verifier: verifier.clone(),
        private,
        public,
        redirect_uri: format!("http://127.0.0.1:{port}/oauth/callback"),
    };
    let pending = Pending {
        id: uuid::Uuid::new_v4().to_string(),
        context: context.clone(),
        expires: now() + 60,
        cancel: CancellationToken::new(),
        result: Arc::new(Mutex::new(None)),
        callback_used: Arc::new(AtomicBool::new(false)),
    };
    {
        let mut slot = LOGIN.lock().map_err(|_| "accountError.busy")?;
        if let Some(old) = slot.replace(pending.clone()) {
            old.cancel.cancel();
        }
    }
    let mut url =
        url::Url::parse(&format!("{PORTAL}/authorize")).map_err(|_| "accountError.auth")?;
    url.query_pairs_mut().extend_pairs([
        ("theme", "2"),
        ("locale", "zh-cn"),
        ("uri_scheme", TOOL_ID),
        ("client_id", TOOL_ID),
        ("port", &port.to_string()),
        (
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        ),
        ("code_challenge_method", "SHA-256"),
        ("ticket_id", &pending.id),
    ]);
    let state = pending.clone();
    let router=axum::Router::new().route("/oauth/callback",axum::routing::get(move |axum::extract::Query(params):axum::extract::Query<BTreeMap<String,String>>| {
        let state=state.clone();async move {
            use axum::http::StatusCode;
            if !["code","secret","error"].iter().any(|key|params.get(*key).is_some_and(|v|!v.is_empty())) {return (StatusCode::BAD_REQUEST,"Missing authorization response.");}
            if state.callback_used.swap(true,Ordering::AcqRel) || state.cancel.is_cancelled() || now()>=state.expires {return (StatusCode::BAD_REQUEST,"Login expired. Return to EchoBird.");}
            let result=tokio::select! {
                _=state.cancel.cancelled()=>return (StatusCode::BAD_REQUEST,"Login cancelled. Return to EchoBird."),
                _=tokio::time::sleep(Duration::from_secs((state.expires-now()).max(0) as u64))=>return (StatusCode::BAD_REQUEST,"Login expired. Return to EchoBird."),
                result=callback(&state,&params)=>result,
            };
            let success=result.is_ok();
            if !state.cancel.is_cancelled() && now()<state.expires { if let Ok(mut slot)=state.result.lock() {if slot.is_none() {*slot=Some(result);}} }
            if success {(StatusCode::OK,"Login complete. You can close this window.")} else {(StatusCode::BAD_REQUEST,"Login failed. Return to EchoBird.")}
        }
    }));
    let cancel = pending.cancel.clone();
    tokio::spawn(async move {
        let _=axum::serve(listener,router).with_graceful_shutdown(async move {tokio::select! {_=cancel.cancelled()=>{},_=tokio::time::sleep(Duration::from_secs(60))=>{}}}).await;
    });
    Ok(LoginStart {
        login_id: pending.id,
        verification_uri: url.into(),
        expires_at: pending.expires,
    })
}

async fn callback(pending: &Pending, params: &BTreeMap<String, String>) -> Result<Value, String> {
    if let Some(code) = params.get("code").filter(|s| !s.is_empty()) {
        return exchange(&pending.context, code).await;
    }
    if let Some(secret) = params.get("secret").filter(|s| !s.is_empty()) {
        let url = "https://snap-access.cn-north-4.myhuaweicloud.com/snap-manager/v1/login/ticket";
        for _ in 0..10 {
            let response = client()?
                .get(url)
                .query(&[("ticket_id", pending.id.as_str()), ("secret", secret)])
                .header("plugin-name", "snap_AIIDE")
                .header("plugin-version", "5.1.0")
                .send()
                .await
                .map_err(|_| "accountError.network")?;
            if response.status().is_success() {
                let ticket: Value = response
                    .json()
                    .await
                    .map_err(|_| "accountError.authResponse")?;
                if ticket["user_id"].as_str().is_some() {
                    let response = json!({"credentials":{"access_key_id":ticket["credential"]["access"],"secret_access_key":ticket["credential"]["secret"],"security_token":ticket["credential"]["securitytoken"],"expiration":ticket["credential"]["expires_at"]},"refresh_token":""});
                    let user = json!({"principal_id":ticket["user_id"],"account_id":ticket["domain_id"],"principal_urn":ticket["user_name"]});
                    return native_from_login(&pending.context, &response, &user);
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
    Err("accountError.authResponse".into())
}

async fn exchange(context: &Context, code: &str) -> Result<Value, String> {
    let url = format!("{STS}/v1/oauth2/tokens");
    let response = client()?
        .post(&url)
        .header("DPoP", dpop(context, &url)?)
        .form(&[
            ("client_id", TOOL_ID),
            ("code", code),
            ("code_verifier", &context.verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &context.redirect_uri),
        ])
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let value = response_json(response).await?;
    let user = signed_json(
        "GET",
        &format!("{STS}/v5/caller-identity"),
        &credentials_from_response(&value)?,
        "",
    )
    .await?;
    native_from_login(context, &value, &user)
}
async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!(
            "accountError.auth|HTTP {}",
            response.status().as_u16()
        ));
    }
    response
        .json()
        .await
        .map_err(|_| "accountError.authResponse".into())
}
fn credentials_from_response(response: &Value) -> Result<Value, String> {
    let c = &response["credentials"];
    Ok(
        json!({"accessKey":string(c,"access_key_id")?,"secretKey":string(c,"secret_access_key")?,"securityToken":string(c,"security_token")?,"expiresAt":string(c,"expiration")?}),
    )
}
fn native_from_login(context: &Context, response: &Value, user: &Value) -> Result<Value, String> {
    credentials_from_response(response)?;
    let credentials = &response["credentials"];
    let user_id = string(user, "principal_id").or_else(|_| string(user, "user_id"))?;
    let domain = string(user, "account_id").or_else(|_| string(user, "domain_id"))?;
    let name = user["principal_urn"]
        .as_str()
        .and_then(|s| s.rsplit(':').next())
        .and_then(|s| s.split('/').next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or(user_id);
    let expires = chrono::DateTime::parse_from_rfc3339(string(credentials, "expiration")?)
        .map_err(|_| "accountError.authResponse")?
        .timestamp_millis();
    let login_context = json!({"pkcePair":{"codeVerifier":context.verifier,"codeChallenge":URL_SAFE_NO_PAD.encode(Sha256::digest(context.verifier.as_bytes())),"codeChallengeMethod":"SHA-256"},"dpopKeyPair":{"privateKey":context.private.to_string(),"publicKey":context.public.to_string()}});
    let refresh = response["refresh_token"].as_str().unwrap_or("");
    Ok(
        json!({"session":{"id":uuid::Uuid::new_v4().to_string(),"userId":user_id,"userName":name,"domainId":domain,"accessKey":credentials["access_key_id"],"secretKey":credentials["secret_access_key"],"securityToken":credentials["security_token"],"refreshToken":refresh,"expiresAt":expires,"clientId":TOOL_ID,"stationType":user["station_type"].as_str().unwrap_or("HC"),"createdAt":chrono::Utc::now().to_rfc3339(),"updatedAt":chrono::Utc::now().to_rfc3339()},"loginContext":login_context}),
    )
}

pub async fn poll_login(login_id: &str) -> Result<Option<Account>, String> {
    let pending = {
        let guard = LOGIN.lock().map_err(|_| "accountError.busy")?;
        guard
            .as_ref()
            .filter(|p| p.id == login_id && !p.cancel.is_cancelled() && p.expires > now())
            .cloned()
            .ok_or("accountError.loginExpired")?
    };
    let native = pending
        .result
        .lock()
        .map_err(|_| "accountError.busy")?
        .take();
    let Some(native) = native else {
        return Ok(None);
    };
    let native = native?;
    let account = account(&native)?;
    let _guard = ACCOUNTS.lock().await;
    let slot = LOGIN.lock().map_err(|_| "accountError.busy")?;
    if !slot
        .as_ref()
        .is_some_and(|p| p.id == login_id && !p.cancel.is_cancelled() && p.expires > now())
    {
        return Err("accountError.loginExpired".into());
    }
    let mut rows = load()?;
    let mut account = account;
    if let Some(old) = rows.get(&account.id) {
        account.balance = old.account.balance;
        account.total = old.account.total;
        account.subscription_end_at = old.account.subscription_end_at;
        account.plan = old.account.plan.clone();
        account.daily_claimed_at = old.account.daily_claimed_at;
    }
    rows.insert(
        account.id.clone(),
        Saved {
            account: account.clone(),
            native,
        },
    );
    save(&rows)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::CodeArts,
        &account.id,
    );
    pending.cancel.cancel();
    Ok(Some(account))
}
pub fn cancel_login(login_id: &str) -> Result<(), String> {
    let mut slot = LOGIN.lock().map_err(|_| "accountError.busy")?;
    if slot.as_ref().is_some_and(|pending| pending.id == login_id) {
        if let Some(pending) = slot.take() {
            pending.cancel.cancel();
        }
    }
    Ok(())
}
fn credentials(saved: &Saved) -> Result<Value, String> {
    require_desktop_id(&saved.account.id)?;
    if saved.account.tool_id != TOOL_ID {
        return Err("accountError.invalidAccount".into());
    }
    let s = &saved.native["session"];
    if s.get("clientId").is_some_and(|id| id != TOOL_ID) {
        return Err("accountError.invalidAccount".into());
    }
    Ok(
        json!({"accessKey":string(s,"accessKey")?,"secretKey":string(s,"secretKey")?,"securityToken":string(s,"securityToken")?,"expiresAt":s["expiresAt"],"toolId":TOOL_ID}),
    )
}
fn update_native_session(saved: &mut Saved, native: &Value) -> Result<bool, String> {
    if account(native)?.id != saved.account.id {
        return Ok(false);
    }
    let incoming = credentials(&Saved {
        account: saved.account.clone(),
        native: native.clone(),
    })?;
    if incoming["expiresAt"].as_i64() > credentials(saved)?["expiresAt"].as_i64() {
        saved.native = native.clone();
        return Ok(true);
    }
    Ok(false)
}
fn refresh_context(saved: &Saved) -> Result<(Context, String), String> {
    credentials(saved)?;
    let c = &saved.native["loginContext"];
    let private = serde_json::from_str(string(&c["dpopKeyPair"], "privateKey")?)
        .map_err(|_| "accountError.format")?;
    let public = serde_json::from_str(string(&c["dpopKeyPair"], "publicKey")?)
        .map_err(|_| "accountError.format")?;
    Ok((
        Context {
            private,
            public,
            verifier: string(&c["pkcePair"], "codeVerifier")?.into(),
            redirect_uri: String::new(),
        },
        string(&saved.native["session"], "refreshToken")?.into(),
    ))
}
async fn renew(saved: &mut Saved) -> Result<(), String> {
    if super::tool_manager::get_tool_exe_path(TOOL_ID).is_some() {
        if let Ok(Some(native)) = native::read() {
            if update_native_session(saved, &native).unwrap_or(false)
                && credentials(saved)?["expiresAt"].as_i64().unwrap_or(0)
                    > chrono::Utc::now().timestamp_millis() + 60000
            {
                return Ok(());
            }
        }
    }
    let (context, refresh) = refresh_context(saved)?;
    let url = format!("{STS}/v1/oauth2/tokens");
    let response = client()?
        .post(&url)
        .header("DPoP", dpop(&context, &url)?)
        .form(&[
            ("client_id", TOOL_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("code_verifier", &context.verifier),
        ])
        .send()
        .await
        .map_err(|_| "accountError.network")?;
    let value = response_json(response).await?;
    credentials_from_response(&value)?;
    let c = &value["credentials"];
    let expiration = chrono::DateTime::parse_from_rfc3339(string(c, "expiration")?)
        .map_err(|_| "accountError.authResponse")?
        .timestamp_millis();
    let s = &mut saved.native["session"];
    s["accessKey"] = c["access_key_id"].clone();
    s["secretKey"] = c["secret_access_key"].clone();
    s["securityToken"] = c["security_token"].clone();
    s["expiresAt"] = json!(expiration);
    s["refreshToken"] = json!(value["refresh_token"].as_str().unwrap_or(&refresh));
    Ok(())
}
pub async fn proxy_credentials(account_id: &str, force_refresh: bool) -> Result<Value, String> {
    require_desktop_id(account_id)?;
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
    let saved = rows.get_mut(account_id).ok_or("accountError.notFound")?;
    let current = credentials(saved)?;
    if force_refresh
        || current["expiresAt"].as_i64().unwrap_or(0)
            <= chrono::Utc::now().timestamp_millis() + 60000
    {
        renew(saved).await?;
        // The renewal spent the client's refresh token too when it holds this account.
        if let Ok(Some(live)) = native::read() {
            if account(&live)?.id == account_id {
                native::apply(saved)?.commit();
            }
        }
        save(&rows)?;
    }
    credentials(rows.get(account_id).ok_or("accountError.notFound")?)
}
pub async fn proxy_credentials_after_rejection(
    account_id: &str,
    rejected: &Value,
) -> Result<Value, String> {
    require_desktop_id(account_id)?;
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
    let saved = rows.get_mut(account_id).ok_or("accountError.notFound")?;
    let current = credentials(saved)?;
    if (current["accessKey"] != rejected["accessKey"]
        || current["securityToken"] != rejected["securityToken"])
        && current["expiresAt"].as_i64().unwrap_or(0)
            > chrono::Utc::now().timestamp_millis() + 60000
    {
        return Ok(current);
    }
    renew(saved).await?;
    let refreshed = credentials(saved)?;
    save(&rows)?;
    Ok(refreshed)
}
async fn signed_json(
    method: &str,
    url: &str,
    credentials: &Value,
    body: &str,
) -> Result<Value, String> {
    let headers = BTreeMap::from([
        ("Content-Type".into(), "application/json".into()),
        ("Agent-Type".into(), "PromptCenter".into()),
        ("X-Language".into(), "zh-cn".into()),
    ]);
    let signed =
        super::account_proxy::codearts_signed_headers(method, url, credentials, body, headers)?;
    let mut request = client()?.request(
        reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| "accountError.auth")?,
        url,
    );
    for (k, v) in signed {
        request = request.header(k, v);
    }
    if !body.is_empty() {
        request = request.body(body.to_string());
    }
    response_json(request.send().await.map_err(|_| "accountError.network")?).await
}
fn account_plan_name(package_name: &str) -> String {
    // Trial is a free credit package, not a separate account tier.
    if package_name.eq_ignore_ascii_case("trial") {
        "Free".into()
    } else {
        package_name.into()
    }
}

fn package_summary(value: &Value) -> Result<(Option<String>, Option<f64>), String> {
    if !value.is_object()
        || value.get("error").is_some_and(|v| !v.is_null())
        || value.get("error_code").is_some_and(|v| v != "0000")
    {
        return Err("accountError.quota".into());
    }
    let package = &value["package"];
    let plan = ["package_name_en", "package_name_cn"]
        .iter()
        .find_map(|key| package[key].as_str().filter(|s| !s.trim().is_empty()))
        .map(account_plan_name);
    let balance = value["metrics"]
        .as_array()
        .and_then(|metrics| {
            metrics
                .iter()
                .find(|m| m["name"] == "usageTotalPackageCredit")
        })
        .and_then(|metric| {
            let value = &metric["package_credit_remain"];
            value
                .as_f64()
                .or_else(|| value.as_str()?.trim().parse().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
        });
    Ok((plan, balance))
}

fn package_total(value: &Value) -> Option<f64> {
    value["metrics"]
        .as_array()?
        .iter()
        .find(|metric| metric["name"] == "usageTotalPackageCredit")?
        .get("package_credit_amount")
        .and_then(|value| {
            value
                .as_f64()
                .or_else(|| value.as_str()?.trim().parse().ok())
        })
        .filter(|total| total.is_finite() && *total >= 0.0)
}
fn package_subscription_end(value: &Value, plan: Option<&str>) -> Option<i64> {
    let plan = plan?;
    if plan.eq_ignore_ascii_case("free") {
        return None;
    }
    let end = value["end_date"].as_str()?.trim();
    let date = chrono::DateTime::parse_from_rfc3339(end).ok().or_else(|| {
        chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?
            .and_local_timezone(chrono::FixedOffset::east_opt(8 * 3600)?)
            .single()
    })?;
    (date.timestamp() > 0 && date.year() < 9999).then_some(date.timestamp())
}

pub async fn refresh_account(account_id: &str) -> Result<Account, String> {
    let credentials = proxy_credentials(account_id, false).await?;
    let result = signed_json(
        "GET",
        "https://snap-access.cn-north-4.myhuaweicloud.com/snap-manager/v1/statistics/plugin",
        &credentials,
        "",
    )
    .await?;
    let (plan, balance) = package_summary(&result)?;
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
    let row = rows.get_mut(account_id).ok_or("accountError.notFound")?;
    row.account.balance = balance;
    row.account.total = package_total(&result);
    row.account.subscription_end_at = package_subscription_end(&result, plan.as_deref());
    row.account.plan = plan;
    let account = row.account.clone();
    save(&rows)?;
    Ok(account)
}
pub async fn claim_credits(account_id: &str) -> Result<Account, String> {
    require_desktop_id(account_id)?;
    let _claim = CLAIM.lock().await;
    let day = |timestamp: i64| (timestamp + 28800).div_euclid(86400);
    let rows = load()?;
    let current = rows.get(account_id).ok_or("accountError.notFound")?;
    if current
        .account
        .daily_claimed_at
        .is_some_and(|timestamp| day(timestamp) == day(now()))
    {
        return Ok(current.account.clone());
    }
    let credentials = proxy_credentials(account_id, false).await?;
    claim_welfare(&credentials, POLICY).await?;
    {
        let _guard = ACCOUNTS.lock().await;
        let mut rows = load()?;
        rows.get_mut(account_id)
            .ok_or("accountError.notFound")?
            .account
            .daily_claimed_at = Some(now());
        save(&rows)?;
    }
    match refresh_account(account_id).await {
        Ok(account) => Ok(account),
        Err(_) => load()?
            .remove(account_id)
            .map(|saved| saved.account)
            .ok_or("accountError.notFound".into()),
    }
}

fn welfare_data(value: &Value) -> Result<&Value, String> {
    let success = value.get("code").map_or_else(
        || value.get("data").is_some(),
        |code| matches!(code.as_i64(), Some(0 | 200)),
    );
    if !success {
        return Err("accountError.quota".into());
    }
    Ok(&value["data"])
}
fn identifier(value: &Value) -> Option<Value> {
    if value.as_str().is_some_and(|s| !s.is_empty()) || value.as_u64().is_some() {
        Some(value.clone())
    } else {
        None
    }
}
fn daily_campaign(value: &Value) -> Result<Option<Value>, String> {
    let items = welfare_data(value)?["items"]
        .as_array()
        .ok_or("accountError.authResponse")?;
    let daily = |a: &Value| {
        a["type"].as_str().is_some_and(|t| {
            t.eq_ignore_ascii_case("daily_claim") || t.eq_ignore_ascii_case("user_login")
        })
    };
    let claimed = |a: &Value| {
        a["claimed"] == true
            || a["status"].as_str().is_some_and(|s| {
                ["CLAIMED", "CONFIRMED", "CONSUMED"]
                    .iter()
                    .any(|status| s.eq_ignore_ascii_case(status))
            })
    };
    let activity = items.iter().find(|a| {
        daily(a)
            && a["claimable"] != false
            && a["claimed"] != true
            && !a["status"].as_str().is_some_and(|s| {
                ["CLAIMED", "CONFIRMED", "CONSUMED"]
                    .iter()
                    .any(|status| s.eq_ignore_ascii_case(status))
            })
    });
    let Some(activity) = activity else {
        return if items.iter().any(|a| daily(a) && claimed(a)) {
            Ok(None)
        } else {
            Err("accountError.unavailable".into())
        };
    };
    identifier(&activity["campaignId"])
        .or_else(|| identifier(&activity["id"]))
        .map(Some)
        .ok_or("accountError.authResponse".into())
}
async fn claim_welfare(credentials: &Value, base: &str) -> Result<(), String> {
    let channel = "DESKTOP";
    let delivery = signed_json(
        "GET",
        &format!("{base}/v1/ops/delivery?channel={channel}"),
        credentials,
        "",
    )
    .await?;
    let Some(campaign) = daily_campaign(&delivery)? else {
        return Ok(());
    };
    let label = campaign
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| campaign.to_string());
    let body=json!({"campaignId":campaign,"idempotentKey":format!("claim_{label}_{}",chrono::Utc::now().timestamp_millis()),"channel":channel}).to_string();
    let claim = signed_json("POST", &format!("{base}/v1/ops/claim"), credentials, &body).await?;
    let claim = welfare_data(&claim)?;
    let Some(benefit) = identifier(&claim["id"]).or_else(|| identifier(&claim["userBenefitId"]))
    else {
        return Ok(());
    };
    let body = json!({"userBenefitId":benefit}).to_string();
    let confirm = signed_json(
        "POST",
        &format!("{base}/v1/ops/confirm"),
        credentials,
        &body,
    )
    .await?;
    welfare_data(&confirm)?;
    Ok(())
}
pub async fn switch_account(account_id: &str) -> Result<Account, String> {
    require_desktop_id(account_id)?;
    if super::tool_manager::get_tool_exe_path(TOOL_ID).is_none() {
        return Err("accountError.unavailable".into());
    }
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
    let saved = rows.get_mut(account_id).ok_or("accountError.notFound")?;
    if saved.account.tool_id != TOOL_ID {
        return Err("accountError.invalidAccount".into());
    }
    if credentials(saved)?["expiresAt"].as_i64().unwrap_or(0)
        <= chrono::Utc::now().timestamp_millis() + 60000
    {
        renew(saved).await?;
        save(&rows)?;
    }
    let saved = rows.get(account_id).ok_or("accountError.notFound")?;
    super::process_manager::stop_desktop_for_account(TOOL_ID).await?;
    let tx = native::apply(saved)?;
    tx.commit();
    let mut account = saved.account.clone();
    account.active = true;
    Ok(account)
}
pub async fn delete_account(account_id: &str) -> Result<(), String> {
    require_desktop_id(account_id)?;
    let _guard = ACCOUNTS.lock().await;
    let mut rows = load()?;
    rows.remove(account_id).ok_or("accountError.notFound")?;
    save(&rows)?;
    super::smart_router::forget_account_authentication(
        crate::models::model::AccountProvider::CodeArts,
        account_id,
    );
    Ok(())
}
