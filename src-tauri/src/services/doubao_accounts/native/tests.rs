use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn c() -> Credentials {
    Credentials {
        edition: Edition::DoubaoWork,
        user_id: "fixture-user".into(),
        enterprise_type: "0".into(),
        cookies: vec![super::super::Cookie {
            name: "sessionid".into(),
            value: "fixture-session".into(),
            domain: ".doubao.com".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            expires: None,
        }],
    }
}
fn target(id: &str, surface: &str, port: u16) -> Target {
    Target {
        id: id.into(),
        kind: "page".into(),
        url: format!("doubaowork://doubaowork-{surface}/{surface}/"),
        web_socket_debugger_url: Some(format!("ws://127.0.0.1:{port}/devtools/page/{id}")),
    }
}
#[test]
fn cdp_targets_require_the_owned_loopback_endpoint_and_tool_surface() {
    let mut value = target("main", "chat", 12345);
    assert!(socket_url(&value, 12345).is_ok());
    assert!(is_surface(&value, Edition::DoubaoWork, "chat"));
    assert!(!is_surface(&value, Edition::Dola, "chat"));
    value.url = "chrome://doubaowork-chat/chat".into();
    assert!(is_surface(&value, Edition::DoubaoWork, "chat"));
    assert!(!is_surface(&value, Edition::Dola, "chat"));
    value.url = "doubaowork://doubaowork-launcher/".into();
    assert!(is_main(&value, Edition::DoubaoWork));
    assert!(!is_main(&value, Edition::Dola));
    for address in [
        "ws://example.com:12345/devtools/page/main",
        "ws://127.0.0.1:12346/devtools/page/main",
        "ws://user@127.0.0.1:12345/devtools/page/main",
        "ws://127.0.0.1:12345/devtools/page/other",
        "ws://127.0.0.1:12345/devtools/page/main?token=x",
    ] {
        value.web_socket_debugger_url = Some(address.into());
        assert!(socket_url(&value, 12345).is_err());
    }
    value.url = "https://www.doubao.com.evil/client-login-modal".into();
    assert!(!is_login(&value, Edition::DoubaoWork));
    value.url = "https://www.doubao.com/client-login-modal".into();
    assert!(is_login(&value, Edition::DoubaoWork));
    value.url = "https://www.doubao.com/chat/fixture".into();
    assert!(is_surface(&value, Edition::DoubaoWork, "chat"));
    value.url = "https://www.doubao.com/chatty".into();
    assert!(!is_surface(&value, Edition::DoubaoWork, "chat"));
    assert!(action_acknowledged(&Value::Null));
    assert!(action_acknowledged(&json!({"success":true})));
    assert!(!action_acknowledged(&Value::Bool(false)));
    assert!(!action_acknowledged(&json!({"success":false})));
}
#[test]
fn native_profile_verification_requires_real_identity_and_enterprise_match() {
    let c = c();
    let good = json!({"message":"success","data":{"user_id_str":c.user_id,"sec_user_id":"fixture-sec","name":"Fixture","screen_name":"fixture-screen","passport_enterprise_user_type":0}});
    let mapped = verified_profile(&c, &good).unwrap();
    assert_eq!(mapped["userId"], c.user_id);
    assert_eq!(mapped["secId"], "fixture-sec");
    assert_eq!(mapped["userName"], "fixture-screen");
    assert_eq!(mapped["nickName"], "Fixture");
    let mut wrong = good.clone();
    wrong["data"]["passport_enterprise_user_type"] = json!(1);
    assert!(verified_profile(&c, &wrong).is_err());
    wrong = good.clone();
    wrong["data"]["user_id_str"] = json!("another-user");
    assert!(verified_profile(&c, &wrong).is_err());
    wrong = good;
    wrong["data"]["sec_user_id"] = Value::Null;
    assert!(verified_profile(&c, &wrong).is_err());
}

#[derive(Default)]
struct State {
    stage: u8,
    cookie_targets: Vec<String>,
    calls: Vec<String>,
    wrong_profile: bool,
    bound_sandbox: bool,
    anonymous_user_id: bool,
    authenticated_sandbox: bool,
    failed_sandbox_profile: bool,
    void_ack: bool,
    https_main: bool,
    login_only: bool,
    start_login_calls: usize,
    finish_login_calls: usize,
}
struct Fake {
    port: u16,
    state: Arc<Mutex<State>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.server.abort();
    }
}
async fn fake(wrong_profile: bool, bound_sandbox: bool) -> Fake {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let state = Arc::new(Mutex::new(State {
        wrong_profile,
        bound_sandbox,
        ..Default::default()
    }));
    let outer = state.clone();
    let server = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let state = outer.clone();
            tokio::spawn(async move {
                let mut header = [0u8; 2048];
                let n = loop {
                    let n = stream.peek(&mut header).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    if header[..n].windows(4).any(|part| part == b"\r\n\r\n") {
                        break n;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                };
                let request = String::from_utf8_lossy(&header[..n]);
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                if path == "/json/list" {
                    let (stage, https_main, login_only) = {
                        let state = state.lock().unwrap();
                        (state.stage, state.https_main, state.login_only)
                    };
                    let mut targets = vec![target(
                        if stage == 2 {
                            "main-after"
                        } else {
                            "main-before"
                        },
                        "chat",
                        port,
                    )];
                    if https_main {
                        targets[0].url = "https://www.doubao.com/chat/fixture".into();
                    }
                    if login_only && stage < 2 {
                        targets = vec![target("sandbox-new", "login", port)];
                    } else if stage == 1 {
                        targets.push(target("sandbox-new", "login", port));
                    }
                    let value:Vec<_>=targets.iter().map(|t|json!({"id":t.id,"type":t.kind,"url":t.url,"webSocketDebuggerUrl":t.web_socket_debugger_url})).collect();
                    let body = serde_json::to_string(&value).unwrap();
                    let response=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);
                    let _ = stream.read(&mut header).await;
                    stream.write_all(response.as_bytes()).await.unwrap();
                    return;
                }
                let id = path.trim_start_matches("/devtools/page/").to_string();
                let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(Message::Text(text))) = ws.next().await {
                    let v: Value = serde_json::from_str(text.as_str()).unwrap();
                    let method = v["method"].as_str().unwrap();
                    let expression = v["params"]["expression"].as_str().unwrap_or("");
                    let mut close = false;
                    let result = {
                        let mut state = state.lock().unwrap();
                        state.calls.push(format!("{id}:{method}"));
                        if method == "Network.setCookies" {
                            assert_eq!(id, "sandbox-new");
                            assert_eq!(v["params"]["cookies"][0]["value"], "fixture-session");
                            state.cookie_targets.push(id.clone());
                            json!({})
                        } else {
                            let value = if expression.contains("getLoginUser") {
                                if id == "sandbox-new" {
                                    json!({"isLogin":state.bound_sandbox,"userId":if state.bound_sandbox{"other-user"}else if state.anonymous_user_id{"anonymous-user"}else{""}})
                                } else {
                                    json!({"isLogin":true,"userId":if state.stage==2{"fixture-user"}else{"previous-user"}})
                                }
                            } else if expression.contains("startLogin") {
                                state.start_login_calls += 1;
                                state.stage = 1;
                                json!({"success":true})
                            } else if expression.contains("finishLogin") {
                                assert_eq!(id, "sandbox-new");
                                assert!(expression.contains("fixture-sec"));
                                state.finish_login_calls += 1;
                                state.stage = 2;
                                close = !state.void_ack;
                                json!({"success":true})
                            } else if expression.contains("fetch(") {
                                if id == "sandbox-new" && state.cookie_targets.is_empty() {
                                    if state.failed_sandbox_profile {
                                        Value::Null
                                    } else if state.authenticated_sandbox {
                                        json!({"http_status":200,"message":"success","data":{"user_id_str":"previous-user"}})
                                    } else {
                                        json!({"http_status":200,"message":"error","error_code":13,"data":{}})
                                    }
                                } else {
                                    json!({"http_status":200,"message":"success","data":{"user_id_str":if state.wrong_profile{"wrong-user"}else{"fixture-user"},"sec_user_id":"fixture-sec","name":"Fixture","passport_enterprise_user_type":0}})
                                }
                            } else {
                                panic!("unexpected CDP method")
                            };
                            if state.void_ack
                                && (expression.contains("startLogin")
                                    || expression.contains("finishLogin"))
                            {
                                json!({"result":{"type":"undefined"}})
                            } else {
                                json!({"result":{"type":"object","value":value}})
                            }
                        }
                    };
                    if close {
                        let _ = ws.close(None).await;
                        break;
                    }
                    ws.send(Message::Text(
                        json!({"id":v["id"],"result":result}).to_string().into(),
                    ))
                    .await
                    .unwrap();
                }
            });
        }
    });
    Fake {
        port,
        state,
        server,
    }
}
#[tokio::test]
async fn native_login_only_sets_cookies_in_new_sandbox_and_accepts_finish_socket_close() {
    let fixture = fake(false, false).await;
    let bridge = Bridge::new(fixture.port).unwrap();
    tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
        .await
        .unwrap()
        .unwrap();
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.stage, 2);
    assert_eq!(state.cookie_targets, vec!["sandbox-new"]);
    assert!(state
        .calls
        .iter()
        .any(|call| call == "main-after:Runtime.evaluate"));
}
#[tokio::test]
async fn first_launch_reuses_only_unbound_login_surface_and_verifies_finished_main() {
    let fixture = fake(false, false).await;
    fixture.state.lock().unwrap().login_only = true;
    let bridge = Bridge::new(fixture.port).unwrap();
    tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
        .await
        .unwrap()
        .unwrap();
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.stage, 2);
    assert_eq!(state.start_login_calls, 0);
    assert_eq!(state.finish_login_calls, 1);
    assert_eq!(state.cookie_targets, vec!["sandbox-new"]);
    assert!(!state
        .calls
        .iter()
        .any(|call| call.starts_with("main-before:")));
    assert!(state
        .calls
        .iter()
        .any(|call| call == "main-after:Runtime.evaluate"));
}
#[tokio::test]
async fn void_native_acknowledgements_still_require_new_sandbox_and_verified_https_main() {
    let fixture = fake(false, false).await;
    {
        let mut state = fixture.state.lock().unwrap();
        state.void_ack = true;
        state.https_main = true;
    }
    let bridge = Bridge::new(fixture.port).unwrap();
    tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
        .await
        .unwrap()
        .unwrap();
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.stage, 2);
    assert_eq!(state.cookie_targets, vec!["sandbox-new"]);
    assert!(state
        .calls
        .iter()
        .any(|call| call == "main-after:Runtime.evaluate"));
}
#[tokio::test]
async fn wrong_sandbox_identity_never_finishes_native_switch() {
    let fixture = fake(true, false).await;
    let bridge = Bridge::new(fixture.port).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
        .await
        .unwrap();
    assert_eq!(result.unwrap_err(), "accountError.invalidAccount");
    assert_eq!(fixture.state.lock().unwrap().stage, 1);
}
#[tokio::test]
async fn previously_bound_login_surface_never_receives_other_account_cookies() {
    let fixture = fake(false, true).await;
    let bridge = Bridge::new(fixture.port).unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
            .await
            .unwrap()
            .unwrap_err(),
        "accountError.invalidAccount"
    );
    assert!(fixture.state.lock().unwrap().cookie_targets.is_empty());
}

#[tokio::test]
async fn logged_out_sandbox_with_anonymous_user_id_can_receive_target_login() {
    let fixture = fake(false, false).await;
    fixture.state.lock().unwrap().anonymous_user_id = true;
    let bridge = Bridge::new(fixture.port).unwrap();
    tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
        .await
        .unwrap()
        .unwrap();
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.stage, 2);
    assert_eq!(state.cookie_targets, vec!["sandbox-new"]);
    assert_eq!(state.finish_login_calls, 1);
}

#[tokio::test]
async fn logged_out_sdk_cannot_overwrite_authenticated_or_failed_passport_context() {
    for authenticated in [true, false] {
        let fixture = fake(false, false).await;
        {
            let mut state = fixture.state.lock().unwrap();
            state.anonymous_user_id = true;
            state.authenticated_sandbox = authenticated;
            state.failed_sandbox_profile = !authenticated;
        }
        let bridge = Bridge::new(fixture.port).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), switch(&bridge, &c()))
                .await
                .unwrap()
                .unwrap_err(),
            "accountError.invalidAccount"
        );
        let state = fixture.state.lock().unwrap();
        assert!(state.cookie_targets.is_empty());
        assert_eq!(state.finish_login_calls, 0);
    }
}

#[test]
fn only_explicit_passport_no_session_response_is_anonymous() {
    let anonymous = json!({"http_status":200,"message":"error","error_code":13,"data":{}});
    assert!(anonymous_profile(&anonymous));
    for (field, value) in [
        ("http_status", json!(500)),
        ("message", json!("success")),
        ("error_code", json!(999)),
        ("data", json!({"user_id_str":"old-user"})),
    ] {
        let mut invalid = anonymous.clone();
        invalid[field] = value;
        assert!(!anonymous_profile(&invalid));
    }
    assert!(!anonymous_profile(&Value::Null));
}

#[tokio::test]
#[ignore = "Explicit native account switch; launches only an owned client and leaves EchoBird account JSON unchanged"]
async fn saved_doubao_native_apply_probe() {
    assert_eq!(
        std::env::var("ECHOBIRD_DOUBAO_NATIVE_PROBE").as_deref(),
        Ok("1")
    );
    for tool in ["doubaowork", "dola"] {
        assert!(!process_manager::desktop_tool_is_running(tool)
            .await
            .unwrap());
    }
    let edition = Edition::DoubaoWork;
    let path = super::super::path(edition).unwrap();
    let before = std::fs::read(&path).unwrap();
    let key = dirs::home_dir()
        .unwrap()
        .join(".echobird/config/.encryption_key");
    assert_eq!(std::fs::metadata(key).unwrap().len(), 32);
    let store = super::super::load(&path).unwrap();
    let index: usize = std::env::var("ECHOBIRD_DOUBAO_NATIVE_PROBE_INDEX")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    let id = store.keys().nth(index).expect("Requires saved account");
    let credentials = super::super::credential(&store, edition, id).unwrap();
    let result = apply(&credentials).await;
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!process_manager::desktop_tool_is_running(edition.tool())
        .await
        .unwrap());
    println!(
        "native_apply_ok={} error_key={:?}",
        result.is_ok(),
        result.as_ref().err()
    );
    result.unwrap();
}

#[tokio::test]
#[ignore = "Explicit native account switch round trip; requires two saved accounts and restores the active account"]
async fn saved_doubao_native_switch_round_trip_probe() {
    async fn verify_restart(c: &Credentials) -> Result<(), String> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut owned = process_manager::start_doubao_account_restart_probe(port)?;
        let bridge = Bridge::new(port)?;
        let result = tokio::time::timeout(Duration::from_secs(15), bridge.verified(c))
            .await
            .unwrap_or_else(|_| Err("accountError.invalidAccount".into()));
        tokio::task::spawn_blocking(move || owned.stop())
            .await
            .unwrap()?;
        result
    }
    assert_eq!(
        std::env::var("ECHOBIRD_DOUBAO_NATIVE_PROBE").as_deref(),
        Ok("1")
    );
    let key = dirs::home_dir()
        .unwrap()
        .join(".echobird/config/.encryption_key");
    assert_eq!(std::fs::metadata(key).unwrap().len(), 32);
    let edition = Edition::DoubaoWork;
    assert!(!process_manager::desktop_tool_is_running(edition.tool())
        .await
        .unwrap());
    let path = super::super::path(edition).unwrap();
    let before = std::fs::read(&path).unwrap();
    let store = super::super::load(&path).unwrap();
    let active = store
        .iter()
        .find(|(_, saved)| saved.account.active)
        .expect("Requires a saved active account")
        .0;
    let other = store
        .keys()
        .find(|id| *id != active)
        .expect("Requires a second saved account");
    let original = super::super::credential(&store, edition, active).unwrap();
    let other = super::super::credential(&store, edition, other).unwrap();
    let switched = match apply(&other).await {
        Ok(()) => verify_restart(&other).await,
        error => error,
    };
    // Restore even if the first switch fails after the native profile changed.
    let restored = match apply(&original).await {
        Ok(()) => verify_restart(&original).await,
        error => error,
    };
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(!process_manager::desktop_tool_is_running(edition.tool())
        .await
        .unwrap());
    println!(
        "native_round_trip_switch_ok={} restore_ok={}",
        switched.is_ok(),
        restored.is_ok()
    );
    switched.unwrap();
    restored.unwrap();
}
