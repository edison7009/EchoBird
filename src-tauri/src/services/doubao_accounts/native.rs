//! Use the client's account isolation service; never rewrite its Chromium files.
use super::{api, Credentials, Edition};
use crate::services::process_manager;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeSet, time::Duration};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

const CALL_TIMEOUT: Duration = Duration::from_secs(6);
const SWITCH_TIMEOUT: Duration = Duration::from_secs(60);

fn stage(stage: &str) {
    log::info!("[doubao-native] {stage}");
    #[cfg(test)]
    eprintln!("[doubao-native] {stage}");
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    web_socket_debugger_url: Option<String>,
}

fn is_surface(target: &Target, edition: Edition, surface: &str) -> bool {
    if !matches!(target.kind.as_str(), "page" | "webview") {
        return false;
    }
    let Ok(url) = url::Url::parse(&target.url) else {
        return false;
    };
    let native = (url.scheme() == edition.tool() || url.scheme() == "chrome")
        && url.host_str() == Some(format!("{}-{surface}", edition.tool()).as_str())
        && url.username().is_empty()
        && url.password().is_none();
    let web_chat = surface == "chat"
        && url.scheme() == "https"
        && url.host_str() == Some(format!("www.{}", edition.domain()).as_str())
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && (url.path() == "/chat" || url.path().starts_with("/chat/"));
    native || web_chat
}
fn is_login(target: &Target, edition: Edition) -> bool {
    if is_surface(target, edition, "login") {
        return true;
    }
    if !matches!(target.kind.as_str(), "page" | "webview") {
        return false;
    }
    let Ok(url) = url::Url::parse(&target.url) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some(format!("www.{}", edition.domain()).as_str())
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.path(), "/client-login" | "/client-login-modal")
}

fn is_main(target: &Target, edition: Edition) -> bool {
    is_surface(target, edition, "chat") || is_surface(target, edition, "launcher")
}
fn socket_url(target: &Target, port: u16) -> Result<url::Url, String> {
    let url = url::Url::parse(
        target
            .web_socket_debugger_url
            .as_deref()
            .ok_or("accountError.format")?,
    )
    .map_err(|_| "accountError.format")?;
    if url.scheme() != "ws"
        || url.host_str() != Some("127.0.0.1")
        || url.port() != Some(port)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != format!("/devtools/page/{}", target.id)
    {
        return Err("accountError.format".into());
    }
    Ok(url)
}

struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: u64,
}
impl Cdp {
    async fn connect(target: &Target, port: u16) -> Result<Self, String> {
        let url = socket_url(target, port)?;
        let (socket, _) =
            tokio::time::timeout(CALL_TIMEOUT, tokio_tungstenite::connect_async(url.as_str()))
                .await
                .map_err(|_| "accountError.expired")?
                .map_err(|_| "accountError.network")?;
        Ok(Self { socket, next_id: 0 })
    }
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.next_id += 1;
        let id = self.next_id;
        tokio::time::timeout(CALL_TIMEOUT, async {
            self.socket
                .send(Message::Text(
                    json!({"id":id,"method":method,"params":params})
                        .to_string()
                        .into(),
                ))
                .await
                .map_err(|_| "accountError.network")?;
            while let Some(message) = self.socket.next().await {
                let Message::Text(text) = message.map_err(|_| "accountError.network")? else {
                    continue;
                };
                let value: Value =
                    serde_json::from_str(text.as_str()).map_err(|_| "accountError.format")?;
                if value["id"].as_u64() != Some(id) {
                    continue;
                }
                if !value["error"].is_null() {
                    return Err("accountError.authResponse".into());
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "accountError.authResponse".into());
            }
            Err("accountError.network".into())
        })
        .await
        .map_err(|_| "accountError.expired")?
    }
    async fn evaluate(&mut self, expression: String) -> Result<Value, String> {
        let value = self
            .request(
                "Runtime.evaluate",
                json!({"expression":expression,"awaitPromise":true,"returnByValue":true}),
            )
            .await?;
        if !value["exceptionDetails"].is_null() {
            return Err("accountError.authResponse".into());
        }
        if value["result"]["type"] == "undefined" {
            return Ok(Value::Null);
        }
        value["result"]
            .get("value")
            .cloned()
            .ok_or_else(|| "accountError.authResponse".into())
    }
    async fn identity(&mut self) -> Result<Value, String> {
        self.evaluate("(async()=>{const a=globalThis.neotix?.account;if(!a?.getLoginUser)return null;const u=await a.getLoginUser();if(typeof u?.isLogin!=='boolean')return null;return {isLogin:u.isLogin,userId:String(u.userId??'')};})()".into()).await
    }
    async fn profile(&mut self, edition: Edition) -> Result<Value, String> {
        let aid = match edition {
            Edition::DoubaoWork => "582478",
            Edition::Dola => "582465",
        };
        let url = format!("{}/passport/account/info/v2/?account_sdk_source=web&sdk_version=2.2.5&aid={aid}&real_aid={aid}&device_platform=web&samantha_web=1&use-olympus-account=1", edition.base());
        self.evaluate(format!("(async()=>{{const r=await fetch({},{{credentials:'include',headers:{{'Agw-Js-Conv':'str'}}}});if(!r.ok)return null;const v=await r.json();const d=v.data??v;return {{http_status:r.status,message:v.message,error_code:d.error_code,data:{{user_id_str:d.user_id_str,sec_user_id:d.sec_user_id,name:d.name,screen_name:d.screen_name,avatar_url:d.avatar_url,passport_enterprise_user_type:d.passport_enterprise_user_type}}}};}})()", json!(url))).await
    }
}

struct Bridge {
    http: reqwest::Client,
    port: u16,
}
impl Bridge {
    fn new(port: u16) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|_| "accountError.network")?;
        Ok(Self { http, port })
    }
    async fn targets(&self) -> Result<Vec<Target>, String> {
        let response = self
            .http
            .get(format!("http://127.0.0.1:{}/json/list", self.port))
            .send()
            .await
            .map_err(|_| "accountError.network")?;
        if !response.status().is_success() {
            return Err("accountError.network".into());
        }
        let values: Vec<Target> = response.json().await.map_err(|_| "accountError.format")?;
        if values.len() > 256 {
            return Err("accountError.format".into());
        }
        Ok(values)
    }
    async fn initial(&self, c: &Credentials) -> Result<(Target, BTreeSet<String>, bool), String> {
        loop {
            if let Ok(targets) = self.targets().await {
                let ids = targets.iter().map(|target| target.id.clone()).collect();
                for target in targets.iter().filter(|target| is_main(target, c.edition)) {
                    if let Ok(mut socket) = Cdp::connect(target, self.port).await {
                        if let Ok(identity) = socket.identity().await {
                            if identity.is_object() {
                                stage("initial.main_ready");
                                return Ok((target.clone(), ids, false));
                            }
                        }
                    }
                }
                // A first launch can consist only of an unbound login surface.
                if !targets
                    .iter()
                    .any(|target| is_surface(target, c.edition, "chat"))
                {
                    for target in targets.iter().filter(|target| is_login(target, c.edition)) {
                        if let Ok(mut socket) = Cdp::connect(target, self.port).await {
                            if let Ok(identity) = socket.identity().await {
                                if unbound(&identity) {
                                    stage("initial.unbound_login_ready");
                                    return Ok((target.clone(), ids, true));
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    async fn sandbox(
        &self,
        c: &Credentials,
        previous: &BTreeSet<String>,
    ) -> Result<Target, String> {
        loop {
            if let Ok(targets) = self.targets().await {
                for target in targets
                    .into_iter()
                    .filter(|target| !previous.contains(&target.id) && is_login(target, c.edition))
                {
                    if let Ok(mut socket) = Cdp::connect(&target, self.port).await {
                        match socket.identity().await {
                            Ok(identity) if unbound(&identity) => {
                                stage("sandbox.unbound_ready");
                                return Ok(target);
                            }
                            Ok(identity) if identity.is_object() => {
                                stage("sandbox.already_bound");
                                return Err("accountError.invalidAccount".into());
                            }
                            _ => {}
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    async fn verified(&self, c: &Credentials) -> Result<(), String> {
        loop {
            if let Ok(targets) = self.targets().await {
                for target in targets.iter().filter(|target| is_main(target, c.edition)) {
                    if let Ok(mut socket) = Cdp::connect(target, self.port).await {
                        if let Ok(identity) = socket.identity().await {
                            if identity["isLogin"] == true && identity["userId"] == c.user_id {
                                if let Ok(profile) = socket.profile(c.edition).await {
                                    if verified_profile(c, &profile).is_ok() {
                                        stage("verify.main_identity_and_profile_match");
                                        return Ok(());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

fn unbound(identity: &Value) -> bool {
    // Logged-out native profiles can retain a nonempty anonymous user ID.
    identity["isLogin"] == false
}
fn anonymous_profile(profile: &Value) -> bool {
    // Passport's no-session response, verified against a request without cookies.
    profile["http_status"] == 200
        && profile["message"] == "error"
        && profile["error_code"] == 13
        && profile["data"]["user_id_str"].is_null()
}
fn action_acknowledged(value: &Value) -> bool {
    value.is_null() || value.as_bool() == Some(true) || value["success"] == true
}
fn verified_profile(c: &Credentials, profile: &Value) -> Result<Value, String> {
    let (account, _, _) = api::parse_profile(c.edition, profile)?;
    if account.id != c.id() {
        stage("profile.identity_mismatch");
        return Err("accountError.invalidAccount".into());
    }
    let data = profile.get("data").unwrap_or(profile);
    let sec_id = data["sec_user_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("accountError.authResponse")?;
    let name = data["name"].as_str().unwrap_or("");
    let screen = data["screen_name"].as_str().unwrap_or("");
    Ok(
        json!({"userId":c.user_id,"secId":sec_id,"userName":if screen.is_empty(){name}else{screen},"nickName":if name.is_empty(){screen}else{name},"avatarUrl":data["avatar_url"].as_str().unwrap_or("")}),
    )
}
fn cookie_params(c: &Credentials) -> Vec<Value> {
    let now = chrono::Utc::now().timestamp();
    c.cookies.iter().filter(|cookie| cookie.valid(c.edition, now)).map(|cookie| {
        let mut value = json!({"name":cookie.name,"value":cookie.value,"domain":cookie.domain,"path":cookie.path,"secure":cookie.secure,"httpOnly":cookie.http_only});
        if let Some(expires) = cookie.expires { value["expires"] = json!(expires); }
        value
    }).collect()
}
async fn switch(bridge: &Bridge, c: &Credentials) -> Result<(), String> {
    stage("initial.wait");
    let (initial, previous, reuse_unbound) = bridge.initial(c).await?;
    let sandbox = if reuse_unbound {
        initial
    } else {
        let mut socket = Cdp::connect(&initial, bridge.port).await?;
        let identity = socket.identity().await?;
        if identity["isLogin"] == true && identity["userId"] == c.user_id {
            if let Ok(profile) = socket.profile(c.edition).await {
                if verified_profile(c, &profile).is_ok() {
                    stage("initial.already_applied");
                    return Ok(());
                }
            }
        }
        let request = json!({"entry":"other","feature":"other","afterLoginToUrl":"","loginUrl":format!("{}://{}-login/login",c.edition.tool(),c.edition.tool()),"closeWhenClickMask":true});
        stage("start_login.call");
        let result = socket
            .evaluate(format!("neotix.account.startLogin({request})"))
            .await?;
        if !action_acknowledged(&result) {
            stage("start_login.rejected");
            return Err("accountError.authResponse".into());
        }
        stage("start_login.acknowledged");
        bridge.sandbox(c, &previous).await?
    };
    let mut socket = Cdp::connect(&sandbox, bridge.port).await?;
    if !unbound(&socket.identity().await?) {
        stage("sandbox.preinject_bound");
        return Err("accountError.invalidAccount".into());
    }
    let profile = socket.profile(c.edition).await?;
    #[cfg(test)]
    eprintln!("[doubao-native] preinject http_status={:?} error_code={:?} has_profile_user={} anonymous={}",profile["http_status"].as_u64(),profile["error_code"].as_u64(),profile["data"]["user_id_str"].as_str().is_some_and(|user|!user.is_empty()),anonymous_profile(&profile));
    if !anonymous_profile(&profile) {
        stage("sandbox.preinject_profile_not_anonymous");
        return Err("accountError.invalidAccount".into());
    }
    stage("sandbox.set_cookies");
    socket
        .request("Network.setCookies", json!({"cookies":cookie_params(c)}))
        .await?;
    stage("sandbox.profile_read");
    let metadata = verified_profile(c, &socket.profile(c.edition).await?)?;
    stage("finish_login.call");
    match socket
        .evaluate(format!("neotix.account.finishLogin({metadata})"))
        .await
    {
        Ok(result) if action_acknowledged(&result) => stage("finish_login.acknowledged"),
        Ok(_) => {
            stage("finish_login.rejected");
            return Err("accountError.authResponse".into());
        }
        // Finish can destroy its sandbox or restart the client before CDP replies.
        Err(error)
            if matches!(
                error.as_str(),
                "accountError.network" | "accountError.expired"
            ) =>
        {
            stage("finish_login.socket_closed")
        }
        Err(error) => return Err(error),
    }
    stage("verify.wait");
    bridge.verified(c).await
}

pub(super) async fn apply(c: &Credentials) -> Result<(), String> {
    api::installed(c.edition)?;
    c.validate()?;
    stage("apply.validated");
    process_manager::stop_desktop_for_account(c.edition.tool()).await?;
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|_| "accountError.network")?;
    let port = listener
        .local_addr()
        .map_err(|_| "accountError.network")?
        .port();
    drop(listener);
    let mut owned = process_manager::start_doubao_account_bridge(c.edition.tool(), port)?;
    stage("apply.owned_client_started");
    let result = match Bridge::new(port) {
        Ok(bridge) => tokio::time::timeout(SWITCH_TIMEOUT, switch(&bridge, c))
            .await
            .unwrap_or_else(|_| Err("accountError.expired".into())),
        Err(error) => Err(error),
    };
    tokio::task::spawn_blocking(move || owned.stop())
        .await
        .map_err(|_| "accountError.closeClient")??;
    stage("apply.owned_client_stopped");
    result
}

#[cfg(test)]
mod tests;
