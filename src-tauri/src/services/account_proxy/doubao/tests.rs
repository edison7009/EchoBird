use super::*;
use axum::{
    extract::{Request, State},
    Router,
};
use http_body_util::BodyExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

fn menu() -> Value {
    json!({"code":0,"data":{"entry_list":[{"active_switch_conf":{"use_menu_conf_v2":true,"menu_conf_v2":{"mode_list":{"item_list":[{"mode_id":"3","agent_mode":1}]},"model_list":{"item_list":[{"model_item_key":"4","item_id":4,"name":"Turbo","model_extra_params":{"total_window_size":"256000"}},{"model_item_key":"5","item_id":5,"name":"Pro","enabled":false,"model_extra_params":"{\"total_window_size\":\"256000\"}"}]}}}}]}})
}

fn event(name: &str, value: Value) -> String {
    format!("event: {name}\ndata: {value}\n\n")
}

fn answer(text: &str) -> String {
    event(
        "STREAM_MSG_NOTIFY",
        json!({"content":{"content_block":[{"block_id":"answer","block_type":10000,"content":{"text_block":{"text":text}}}]}}),
    ) + &event("SSE_REPLY_END", json!({}))
}

type Requests = Arc<Mutex<Vec<(String, HeaderMap, Value)>>>;

#[derive(Clone, Default)]
struct Fixture {
    requests: Requests,
    response: Arc<Mutex<Option<String>>>,
    hang: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    base: Arc<Mutex<String>>,
    uploaded: Arc<Mutex<Vec<u8>>>,
    fail_commit: Arc<AtomicBool>,
    tool_reply: Arc<AtomicBool>,
    parallel_tool_reply: Arc<AtomicBool>,
    menu: Arc<Mutex<Option<Value>>>,
}

struct DropMarker(Arc<AtomicBool>);
impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

async fn upstream(State(state): State<Fixture>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    let headers = request.headers().clone();
    let bytes = axum::body::to_bytes(request.into_body(), MAX_IMAGE + 8192)
        .await
        .unwrap();
    let body = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    state
        .requests
        .lock()
        .unwrap()
        .push((path.clone(), headers, body.clone()));
    match path.as_str() {
        "/alice/ug/campaign/code200/" | "/alice/user/code200/" => {
            json_response(&json!({"code":200,"data":{}}))
        }
        "/alice/ug/campaign/error/" => json_response(&json!({"code":401,"data":{}})),
        "/alice/user/launch" => {
            json_response(&json!({"code":0,"data":{"assistant_bot_id":"fixture-bot"}}))
        }
        "/alice/slot/action_bar_v3/brief_list" => {
            json_response(&state.menu.lock().unwrap().clone().unwrap_or_else(menu))
        }
        "/alice/resource/prepare_upload" => json_response(
            &json!({"code":0,"data":{"service_id":"fixture-service","upload_host":state.base.lock().unwrap().clone(),"upload_auth_token":{"access_key":"AK","secret_key":"SK","session_token":"TOKEN"}}}),
        ),
        "/" if query.contains("Action=ApplyImageUpload") => json_response(
            &json!({"Result":{"UploadAddress":{"StoreInfos":[{"StoreUri":"fixture-image","Auth":"upload-auth"}],"UploadHosts":[state.base.lock().unwrap().clone()],"SessionKey":"fixture-session"}}}),
        ),
        "/upload/v1/fixture-image" => {
            *state.uploaded.lock().unwrap() = bytes.to_vec();
            json_response(&json!({"code":2000}))
        }
        "/" if query.contains("Action=CommitImageUpload") => json_response(
            &json!({"Result":{"Results":[{"Uri":"committed-image","UriStatus":if state.fail_commit.load(Ordering::SeqCst) { 4000 } else { 2000 }}]}}),
        ),
        "/chat/completion" => {
            if state.hang.load(Ordering::SeqCst) {
                let marker = DropMarker(state.dropped.clone());
                let bytes = Bytes::from(event(
                    "STREAM_MSG_NOTIFY",
                    json!({"content":{"content_block":[{"block_id":"partial","block_type":10000,"content":{"text_block":{"text":"partial"}}}]}}),
                ));
                let stream = futures_util::stream::unfold(
                    (Some(bytes), marker),
                    |(first, marker)| async move {
                        if let Some(bytes) = first {
                            Some((Ok::<_, std::io::Error>(bytes), (None, marker)))
                        } else {
                            std::future::pending().await
                        }
                    },
                );
                return stream_response(Body::from_stream(stream));
            }
            if state.tool_reply.load(Ordering::SeqCst) {
                let prompt = body["messages"][0]["content_block"][0]["content"]["text_block"]
                    ["text"]
                    .as_str()
                    .unwrap();
                let start = prompt.find("echobird_tools_").unwrap();
                let marker = prompt[start..].split('"').next().unwrap();
                let calls = if state.parallel_tool_reply.load(Ordering::SeqCst) {
                    json!([{"name":"read","arguments":{"path":"fixture.txt"}},{"name":"write","arguments":{"path":"other.txt","text":"result"}}])
                } else {
                    json!([{"name":"read","arguments":{"path":"fixture.txt"}}])
                };
                return stream_response(Body::from(answer(&json!({marker:calls}).to_string())));
            }
            let text = state.response.lock().unwrap().clone().unwrap_or_else(|| {
                answer(
                    body["option"]["model_config"]["model_item_key"]
                        .as_str()
                        .unwrap_or("missing"),
                )
            });
            stream_response(Body::from(text))
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

use axum::response::IntoResponse;

async fn fixture() -> (Fixture, Value, tokio::task::JoinHandle<()>) {
    let state = Fixture::default();
    let app = Router::new().fallback(upstream).with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    *state.base.lock().unwrap() = format!("http://{address}");
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (
        state,
        json!({"accountId":format!("fixture-{address}"),"baseUrl":format!("http://{address}"),"cookieHeader":"sessionid=synthetic","deviceId":"fixture-device"}),
        server,
    )
}

fn plain(stream: bool) -> Value {
    json!({"messages":[{"role":"user","content":"hello"}],"stream":stream})
}

#[tokio::test]
async fn only_ug_account_metadata_accepts_code_200() {
    let (_, credentials, server) = fixture().await;
    let provider = AccountProvider::DoubaoWork;
    let body = json!({});
    assert!(
        account_json(provider, &credentials, "/alice/ug/campaign/code200/", &body)
            .await
            .is_ok()
    );
    assert!(
        account_json(provider, &credentials, "/alice/user/launch", &body)
            .await
            .is_ok()
    );
    assert!(
        account_json(provider, &credentials, "/alice/user/code200/", &body)
            .await
            .is_err()
    );
    assert!(api_json(
        request(provider, &credentials, "/alice/ug/campaign/code200/")
            .unwrap()
            .json(&body)
    )
    .await
    .is_err());
    assert_eq!(
        account_json(provider, &credentials, "/alice/ug/campaign/error/", &body)
            .await
            .unwrap_err()
            .status,
        StatusCode::UNAUTHORIZED
    );
    server.abort();
}

#[test]
fn requests_use_each_desktop_product_context_without_guessing_dola_region() {
    for (provider, aid, runtime, language) in [
        (AccountProvider::DoubaoWork, "1044603", "3.42.2", "zh"),
        (AccountProvider::Dola, "582465", "3.42.1", "en"),
    ] {
        let request = request(provider, &json!({"cookieHeader":"sessionid=fixture","deviceId":"fixture-device","fingerprint":"unrelated-web-fingerprint","clientVersion":"2.33.5","chromiumVersion":"147.0.7727.149","channel":"1001"}), "/alice/slot/action_bar_v3/brief_list").unwrap().build().unwrap();
        let query: BTreeMap<_, _> = request.url().query_pairs().into_owned().collect();
        for (key, value) in [
            ("aid", aid),
            ("real_aid", aid),
            ("device_platform", "web"),
            ("web_platform", "desktop"),
            ("doubao_device_platform", "desktop"),
            ("client_platform", "pc_client"),
            ("pkg_type", "release_version"),
            ("runtime", "web"),
            ("runtime_version", runtime),
            ("pc_version", "2.33.5"),
            ("doubao_pc_version", "2.33.5"),
            ("chromium_version", "147.0.7727.149"),
            ("channel", "1001"),
            ("language", language),
            ("fp", "verify_fixture-device"),
        ] {
            assert_eq!(query.get(key).map(String::as_str), Some(value));
        }
        assert_eq!(request.headers()[header::COOKIE], "sessionid=fixture");
        assert!(!query.contains_key("cookieHeader"));
        if provider == AccountProvider::Dola {
            assert!(!query.contains_key("region") && !query.contains_key("sys_region"));
        } else {
            assert_eq!(query["region"], "CN");
            assert_eq!(query["sys_region"], "CN");
        }
    }
}

async fn response_json(response: Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[test]
fn directory_keeps_real_ids_and_leaves_account_permissions_to_upstream() {
    let models = directory(&menu());
    assert_eq!(models.len(), 2);
    assert_eq!(models["4"].name, "Turbo");
    assert_eq!(models["5"].extra["total_window_size"], "256000");
    assert_eq!(models["5"].agent_mode, 1);
    assert_eq!(aliases(&models)["Pro"], "5");
    let legacy = json!({"data":{"entry_list":[{"active_switch_conf":{"menu_conf":{"item_list":[{"item_id":0,"name":"Fast"},{"model_item_key":"3","name":"Pro","agent_mode":0}]}}}]}});
    assert_eq!(directory(&legacy)["0"].deep_think, 0);
    assert!(directory(&json!({"data":{"models":[{"id":"fake"}]}})).is_empty());
}

fn mixed_menu() -> Value {
    json!({"code":0,"data":{"entry_list":[{"active_switch_conf":{"use_menu_conf_v2":true,"menu_conf_v2":{
        "default_select_mode_map":{"new_chat":{"mode_id":"chat"}},
        "mode_list":{"item_list":[
            {"mode_id":"work","name":"Work","agent_mode":1,"model_list":{"item_list":[{"model_item_key":"paid","item_id":5,"name":"Paid","enabled":false},{"model_item_key":"shared","item_id":8,"name":"Shared","model_extra_params":{"total_window_size":"256000"}}]}},
            {"mode_id":"chat","name":"Chat","agent_mode":2}
        ]},
        "model_list":{"item_list":[
            {"model_item_key":"free","item_id":0,"name":"Free","conversation_mode_list":["chat"]},
            {"model_item_key":"shared","item_id":8,"name":"Shared"},
            {"model_item_key":"default","item_id":1,"name":"Default"},
            {"model_item_key":"explicit","item_id":5,"name":"Explicit Work","default_mode":"work"}
        ]}
    }}}]}})
}

#[test]
fn directory_preserves_mode_local_models_and_default_chat_contexts() {
    let mut value = mixed_menu();
    let models = directory(&value);
    assert_eq!(models["free"].agent_mode, 2);
    assert_eq!(models["free"].mode_id.as_deref(), Some("chat"));
    assert_eq!(models["paid"].agent_mode, 1);
    assert_eq!(models["paid"].mode_id.as_deref(), Some("work"));
    assert_eq!(models["explicit"].mode_id.as_deref(), Some("work"));
    assert_eq!(models["default"].mode_id.as_deref(), Some("chat"));
    assert_eq!(models["chat::shared"].id, "shared");
    assert_eq!(models["work::shared"].id, "shared");
    assert_eq!(models["chat::shared"].agent_mode, 2);
    assert_eq!(models["work::shared"].agent_mode, 1);
    assert_eq!(models["work::shared"].extra["total_window_size"], "256000");
    assert_eq!(aliases(&models)["Shared · Chat"], "chat::shared");
    assert_eq!(aliases(&models)["Shared · Work"], "work::shared");
    assert!(!models.contains_key("shared"));
    value["data"]["entry_list"][0]["active_switch_conf"]["menu_conf_v2"]
        ["default_select_mode_map"]["new_chat"]["mode_id"] = json!("work");
    let models = directory(&value);
    assert_eq!(models["default"].mode_id.as_deref(), Some("work"));
    assert_eq!(models["free"].mode_id.as_deref(), Some("chat"));
    value["data"]["entry_list"][0]["active_switch_conf"]["menu_conf_v2"]
        ["default_select_mode_map"] = json!({});
    assert_eq!(
        directory(&value)["default"].mode_id.as_deref(),
        Some("chat")
    );
}

#[test]
fn editions_never_accept_another_provider_or_untrusted_base() {
    assert_eq!(
        base(AccountProvider::Dola, &json!({})).unwrap(),
        "https://www.dola.com"
    );
    assert_eq!(
        base(AccountProvider::DoubaoWork, &json!({})).unwrap(),
        "https://www.doubao.com"
    );
    assert!(base(
        AccountProvider::Dola,
        &json!({"baseUrl":"https://www.doubao.com"})
    )
    .is_err());
    assert!(base(
        AccountProvider::Dola,
        &json!({"baseUrl":"https://example.com"})
    )
    .is_err());
    assert!(base(AccountProvider::Grok, &json!({})).is_err());
}

#[test]
fn tools_preserve_history_and_enforce_named_choice_and_parallel_policy() {
    let body = json!({"messages":[{"role":"system","content":"be careful"},{"role":"assistant","content":null,"tool_calls":[{"id":"old-call","type":"function","function":{"name":"read","arguments":"{\"path\":\"x\"}"}}]},{"role":"tool","tool_call_id":"old-call","content":"file contents"},{"role":"user","content":"continue"}],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}}],"tool_choice":{"type":"function","function":{"name":"read"}},"parallel_tool_calls":false});
    let bridge = ToolBridge::new(&body).unwrap();
    let (prompt, images) = content(&body, &bridge).unwrap();
    assert!(images.is_empty());
    for required in [
        "old-call",
        "file contents",
        "be careful",
        "properties",
        "path",
    ] {
        assert!(prompt.contains(required));
    }
    let message = bridge
        .message(&json!({&bridge.marker:[{"name":"read","arguments":{"path":"x"}}]}).to_string())
        .unwrap();
    assert_eq!(message["tool_calls"][0]["function"]["name"], "read");
    assert_eq!(
        message["tool_calls"][0]["function"]["arguments"],
        "{\"path\":\"x\"}"
    );
    assert!(bridge.message("I cannot use that tool").is_err());
    assert!(bridge
        .message(&json!({&bridge.marker:[{"name":"other","arguments":{}}]}).to_string())
        .is_err());
    assert!(bridge.message(&json!({&bridge.marker:[{"name":"read","arguments":{}},{"name":"read","arguments":{}}]}).to_string()).is_err());
}

#[test]
fn images_and_unknown_content_are_not_silently_removed() {
    let body = json!({"messages":[{"role":"user","content":[{"type":"text","text":"see picture"},{"type":"image_url","image_url":{"url":"data:image/png;base64,eA=="}}]}]});
    let bridge = ToolBridge::new(&body).unwrap();
    let (prompt, images) = content(&body, &bridge).unwrap();
    assert_eq!(images, ["data:image/png;base64,eA=="]);
    assert!(prompt.contains("[Attached image 1]"));
    assert!(!prompt.contains("eA=="));
    let remote = json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.com/image.png"}}]}]});
    assert_eq!(
        content(&remote, &bridge).unwrap_err().error_code.as_deref(),
        Some("unsupported_parameter")
    );
    let audio = json!({"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"data":"x"}}]}]});
    assert!(content(&audio, &bridge).is_err());
}

#[test]
fn decoder_keeps_deltas_unique_and_does_not_accept_unfinished_or_failed_answers() {
    let mut decoder = Decoder::default();
    let headers = HeaderMap::new();
    let initial = json!({"content":{"content_block":[{"block_id":"x","block_type":10000,"content":{"text_block":{"text":"你"}}}]}});
    assert_eq!(
        decoder
            .frame("STREAM_MSG_NOTIFY", &initial.to_string(), &headers)
            .unwrap(),
        "你"
    );
    let delta = json!({"patch_op":[{"patch_object":1,"patch_type":1,"patch_value":{"content_block":[{"block_id":"x","block_type":10000,"content":{"text_block":{"text":"好"}}}]}}]});
    assert_eq!(
        decoder
            .frame("STREAM_CHUNK", &delta.to_string(), &headers)
            .unwrap(),
        "好"
    );
    let full = json!({"message":{"user_type":2,"content_block":[{"block_id":"x","block_type":10000,"content":{"text_block":{"text":"你好"}}}]}});
    assert_eq!(
        decoder
            .frame("FULL_MSG_NOTIFY", &full.to_string(), &headers)
            .unwrap(),
        ""
    );
    assert_eq!(decoder.text, "你好");
    assert!(!decoder.done);
    let error = decoder
        .frame(
            "SSE_ERROR",
            &json!({"code":429,"message":"model quota exhausted"}).to_string(),
            &headers,
        )
        .unwrap_err();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(error.model_scope);
    assert!(!decoder.done);
}

#[test]
fn fast_deltas_and_pro_reasoning_use_separate_output_channels() {
    let mut decoder = Decoder::default();
    let headers = HeaderMap::new();
    for text in ["Hello", " world"] {
        decoder.frame("STREAM_MSG_NOTIFY", &json!({"content":{"content_block":[{"block_id":"fast","block_type":10000,"content":{"text_block":{"text":text}}}]}}).to_string(),&headers).unwrap();
    }
    assert_eq!(decoder.text, "Hello world");
    let mut pro = Decoder::default();
    let blocks = json!([{"block_type":10040,"is_finish":false},{"block_type":10000,"content":{"text_block":{"text":"private reasoning","icon_url":"Deep_Think.png"}}},{"block_type":10040,"is_finish":true},{"block_id":"answer","block_type":10000,"content":{"text_block":{"text":"answer"}}}]);
    assert_eq!(pro.blocks(&blocks, true).unwrap(), "answer");
    assert_eq!(pro.text, "answer");
    assert_eq!(pro.reasoning, "private reasoning");
    let mut header_only = Decoder::default();
    let blocks = json!([{"block_type":10040},{"block_id":"answer","block_type":10000,"content":{"text_block":{"text":"visible answer"}}}]);
    assert_eq!(header_only.blocks(&blocks, true).unwrap(), "visible answer");
    assert!(header_only.reasoning.is_empty());
    let reasoning = json!([{"block_id":"thought","block_type":10000,"content":{"text_block":{"text":"first thought","icon_url":"Deep_Think.png"}}}]);
    header_only.blocks(&reasoning, true).unwrap();
    let more = json!([{"block_id":"thought","block_type":10000,"content":{"text_block":{"text":" and more"}}},{"block_id":"answer","block_type":10000,"content":{"text_block":{"text":" final answer"}}}]);
    assert_eq!(header_only.blocks(&more, true).unwrap(), " final answer");
    assert_eq!(header_only.reasoning, "first thought and more");
}

#[test]
fn image_signing_crc_and_host_checks_are_deterministic() {
    assert_eq!(crc32(b"123456789"), "cbf43926");
    assert_eq!(encode("a b/+*"), "a%20b%2F%2B%2A");
    let auth = json!({"access_key":"AK","secret_key":"SK","session_token":"TOKEN"});
    let query = BTreeMap::from([
        ("Action", "ApplyImageUpload".into()),
        ("FileExtension", ".png".into()),
    ]);
    let (query, signature) = image_sign(
        &auth,
        "GET",
        "imagex-us-east-1.bytevcloudapi.com",
        &query,
        "",
        "us-east-1",
        "20261022T120000Z",
    )
    .unwrap();
    assert_eq!(query, "Action=ApplyImageUpload&FileExtension=.png");
    assert!(signature.contains("Credential=AK/20261022/us-east-1/imagex/aws4_request"));
    assert!(signature
        .ends_with("Signature=393c83fcaf86350456a0b275e7b9d0bc177b101df451cace27e8674413f82c20"));
    assert!(upload_base("imagex-us-east-1.bytevcloudapi.com").is_ok());
    for host in [
        "127.0.0.1",
        "localhost",
        "example.com",
        "user@imagex.byteimg.com",
        "imagex.byteimg.com/evil",
    ] {
        assert!(upload_base(host).is_err());
    }
}

#[tokio::test]
async fn real_directory_routes_exact_model_and_concurrent_calls_have_distinct_conversations() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    assert_eq!(
        list_models(AccountProvider::Dola, credentials.clone(), &client)
            .await
            .unwrap(),
        ["Pro", "Turbo"]
    );
    let (one, two) = tokio::join!(
        route(
            AccountProvider::Dola,
            credentials.clone(),
            "Turbo",
            plain(false),
            &client
        ),
        route(
            AccountProvider::Dola,
            credentials.clone(),
            "Pro",
            plain(false),
            &client
        )
    );
    assert_eq!(
        response_json(one.unwrap()).await["choices"][0]["message"]["content"],
        "4"
    );
    assert_eq!(
        response_json(two.unwrap()).await["choices"][0]["message"]["content"],
        "5"
    );
    let requests = state.requests.lock().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|(path, _, _)| path == "/alice/user/launch")
            .count(),
        1
    );
    let chats: Vec<_> = requests
        .iter()
        .filter(|(path, _, _)| path == "/chat/completion")
        .collect();
    assert_eq!(chats.len(), 2);
    assert_ne!(
        chats[0].2["client_meta"]["local_conversation_id"],
        chats[1].2["client_meta"]["local_conversation_id"]
    );
    assert_ne!(
        chats[0].2["messages"][0]["local_message_id"],
        chats[1].2["messages"][0]["local_message_id"]
    );
    for (_, headers, body) in chats {
        assert_eq!(headers[header::COOKIE], "sessionid=synthetic");
        assert_eq!(body["client_meta"]["conversation_id"], "");
        assert_eq!(body["option"]["need_create_conversation"], true);
    }
    server.abort();
}

#[tokio::test]
async fn truncated_and_failed_streams_never_emit_done() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    *state.response.lock().unwrap() = Some(event(
        "STREAM_MSG_NOTIFY",
        json!({"content":{"content_block":[{"block_id":"x","block_type":10000,"content":{"text_block":{"text":"partial"}}}]}}),
    ));
    let response = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        plain(true),
        &client,
    )
    .await
    .unwrap();
    let mut body = response.into_body();
    let first = body.frame().await.unwrap().unwrap().into_data().unwrap();
    assert!(String::from_utf8_lossy(&first).contains("partial"));
    assert!(body.frame().await.unwrap().is_err());
    *state.response.lock().unwrap() = Some(event(
        "SSE_ERROR",
        json!({"code":429,"message":"quota exhausted"}),
    ));
    let error = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        plain(false),
        &client,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(error.model_scope);
    let first_error = event("SSE_ACK", json!({}))
        + &event("SSE_ERROR", json!({"code":429,"message":"quota exhausted"}));
    *state.response.lock().unwrap() = Some(first_error);
    let error = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        plain(true),
        &client,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error.error_code.as_deref(), Some("429"));
    assert!(error.model_scope);
    *state.response.lock().unwrap() = Some(event(
        "SSE_ERROR",
        json!({"code":401,"message":"session expired"}),
    ));
    let error = route(
        AccountProvider::Dola,
        credentials,
        "Turbo",
        plain(true),
        &client,
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::UNAUTHORIZED);
    assert!(!error.model_scope);
    server.abort();
}

#[tokio::test]
async fn dropping_stream_body_cancels_only_its_own_upstream() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    state.hang.store(true, Ordering::SeqCst);
    let response = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        plain(true),
        &client,
    )
    .await
    .unwrap();
    assert!(!state.dropped.load(Ordering::SeqCst));
    drop(response);
    tokio::time::timeout(Duration::from_secs(3), async {
        while !state.dropped.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    state.hang.store(false, Ordering::SeqCst);
    let response = route(
        AccountProvider::Dola,
        credentials,
        "Pro",
        plain(false),
        &client,
    )
    .await
    .unwrap();
    assert_eq!(
        response_json(response).await["choices"][0]["message"]["content"],
        "5"
    );
    server.abort();
}

#[tokio::test]
async fn image_bytes_are_uploaded_committed_and_attached_without_sending_account_cookies_to_storage(
) {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    let mut image = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(1, 1)
        .write_to(&mut image, image::ImageFormat::Png)
        .unwrap();
    let bytes = image.into_inner();
    let body = json!({"messages":[{"role":"user","content":[{"type":"text","text":"read image"},{"type":"image_url","image_url":{"url":format!("data:image/png;base64,{}",STANDARD.encode(&bytes))}}]}]});
    let response = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        body.clone(),
        &client,
    )
    .await
    .unwrap();
    assert_eq!(
        response_json(response).await["choices"][0]["message"]["content"],
        "4"
    );
    assert_eq!(*state.uploaded.lock().unwrap(), bytes);
    {
        let requests = state.requests.lock().unwrap();
        let chat = &requests
            .iter()
            .find(|(path, _, _)| path == "/chat/completion")
            .unwrap()
            .2;
        let attachment = &chat["messages"][0]["content_block"][0]["content"]["attachment_block"]
            ["attachments"][0];
        assert_eq!(attachment["image"]["uri"], "committed-image");
        assert_eq!(attachment["image"]["image_ori"]["width"], 1);
        for (path, headers, _) in requests
            .iter()
            .filter(|(path, _, _)| path == "/" || path.starts_with("/upload/"))
        {
            assert!(!headers.contains_key(header::COOKIE));
            if path == "/" {
                assert!(headers[header::AUTHORIZATION]
                    .to_str()
                    .unwrap()
                    .starts_with("AWS4-HMAC-SHA256"));
            }
        }
    }
    state.requests.lock().unwrap().clear();
    state.fail_commit.store(true, Ordering::SeqCst);
    assert!(
        route(AccountProvider::Dola, credentials, "Turbo", body, &client)
            .await
            .is_err()
    );
    assert!(!state
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(path, _, _)| path == "/chat/completion"));
    server.abort();
}

#[tokio::test]
async fn tool_stream_produces_standard_calls_and_next_round_preserves_results() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    state.tool_reply.store(true, Ordering::SeqCst);
    let tools = json!([{"type":"function","function":{"name":"read","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}]);
    let body = json!({"messages":[{"role":"user","content":"read fixture.txt"}],"tools":tools,"tool_choice":"required","stream":true});
    let response = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        body,
        &client,
    )
    .await
    .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let frames = SseParser::default().push(&bytes);
    let call = serde_json::from_str::<Value>(&frames[0].1).unwrap()["choices"][0]["delta"]
        ["tool_calls"][0]
        .clone();
    assert_eq!(call["function"]["name"], "read");
    assert_eq!(call["function"]["arguments"], "{\"path\":\"fixture.txt\"}");
    assert_eq!(
        serde_json::from_str::<Value>(&frames[1].1).unwrap()["choices"][0]["finish_reason"],
        "tool_calls"
    );
    assert_eq!(frames[2].1, "[DONE]");
    state.tool_reply.store(false, Ordering::SeqCst);
    let next = json!({"messages":[{"role":"user","content":"read fixture.txt"},{"role":"assistant","content":null,"tool_calls":[call.clone()]},{"role":"tool","tool_call_id":call["id"],"content":"fixture file result"}],"tools":tools});
    let response = route(AccountProvider::Dola, credentials, "Turbo", next, &client)
        .await
        .unwrap();
    assert_eq!(
        response_json(response).await["choices"][0]["message"]["content"],
        "4"
    );
    let requests = state.requests.lock().unwrap();
    let prompt = requests
        .iter()
        .rev()
        .find(|(path, _, _)| path == "/chat/completion")
        .unwrap()
        .2["messages"][0]["content_block"][0]["content"]["text_block"]["text"]
        .as_str()
        .unwrap();
    assert!(prompt.contains(call["id"].as_str().unwrap()));
    assert!(prompt.contains("fixture file result"));
    server.abort();
}

#[tokio::test]
async fn parallel_tool_stream_keeps_distinct_ids_names_and_arguments_through_shared_decoder() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    state.tool_reply.store(true, Ordering::SeqCst);
    state.parallel_tool_reply.store(true, Ordering::SeqCst);
    let tools = json!([{"type":"function","function":{"name":"read","parameters":{"type":"object"}}},{"type":"function","function":{"name":"write","parameters":{"type":"object"}}}]);
    let body = json!({"messages":[{"role":"user","content":"read and write"}],"tools":tools,"tool_choice":"required","stream":true});
    let response = route(
        AccountProvider::Dola,
        credentials.clone(),
        "Turbo",
        body.clone(),
        &client,
    )
    .await
    .unwrap();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let frames = SseParser::default().push(&bytes);
    let emitted = serde_json::from_str::<Value>(&frames[0].1).unwrap();
    let calls = emitted["choices"][0]["delta"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(calls[0]["index"], 0);
    assert_eq!(calls[1]["index"], 1);
    let mut decoder = protocol::ChatDecoder::new(protocol::WireProtocol::Chat, "Turbo");
    for (event, data) in frames {
        decoder.decode(&event, &data).unwrap();
    }
    assert!(decoder.finished);
    let collected = decoder.collected();
    let normalized = collected["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(normalized.len(), 2);
    assert_ne!(normalized[0]["id"], normalized[1]["id"]);
    for (actual, expected) in normalized.iter().zip(calls) {
        assert_eq!(actual["id"], expected["id"]);
        assert_eq!(actual["function"], expected["function"]);
    }
    assert_eq!(normalized[0]["function"]["name"], "read");
    assert_eq!(
        normalized[0]["function"]["arguments"],
        "{\"path\":\"fixture.txt\"}"
    );
    assert_eq!(normalized[1]["function"]["name"], "write");
    assert_eq!(
        serde_json::from_str::<Value>(normalized[1]["function"]["arguments"].as_str().unwrap())
            .unwrap(),
        json!({"path":"other.txt","text":"result"})
    );
    let mut nonstream = body;
    nonstream["stream"] = json!(false);
    let response = route(
        AccountProvider::Dola,
        credentials,
        "Turbo",
        nonstream,
        &client,
    )
    .await
    .unwrap();
    let value = response_json(response).await;
    assert!(value["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .all(|call| call.get("index").is_none()));
    server.abort();
}

#[tokio::test]
async fn mixed_directory_routes_chat_and_agent_models_without_changing_their_context() {
    let (state, credentials, server) = fixture().await;
    let client = reqwest::Client::new();
    *state.menu.lock().unwrap() = Some(mixed_menu());
    let names = list_models(AccountProvider::Dola, credentials.clone(), &client)
        .await
        .unwrap();
    assert!(names.contains(&"Paid".to_string()));
    assert!(names.contains(&"Shared · Chat".to_string()));
    assert!(names.contains(&"Shared · Work".to_string()));
    for model in ["Free", "Paid", "chat::shared", "work::shared"] {
        let response = route(
            AccountProvider::Dola,
            credentials.clone(),
            model,
            plain(false),
            &client,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let requests = state.requests.lock().unwrap();
    let chats: Vec<_> = requests
        .iter()
        .filter(|(path, _, _)| path == "/chat/completion")
        .map(|(_, _, body)| body)
        .collect();
    for index in [0, 2] {
        assert_eq!(chats[index]["option"]["agent_mode"], 2);
        assert_eq!(
            chats[index]["option"]["aggregate_params"]["mode_id"],
            "chat"
        );
        assert!(chats[index]["option"]["general_task_param"].is_object());
    }
    for index in [1, 3] {
        assert_eq!(chats[index]["option"]["agent_mode"], 1);
        assert_eq!(
            chats[index]["option"]["aggregate_params"]["mode_id"],
            "work"
        );
        assert!(chats[index]["option"]["general_task_param"].is_object());
    }
    assert_eq!(chats[0]["option"]["model_config"]["model_item_key"], "free");
    assert_eq!(chats[1]["option"]["model_config"]["model_item_key"], "paid");
    assert_eq!(
        chats[2]["option"]["model_config"]["model_item_key"],
        "shared"
    );
    assert_eq!(
        chats[3]["option"]["model_config"]["model_item_key"],
        "shared"
    );
    server.abort();
}
