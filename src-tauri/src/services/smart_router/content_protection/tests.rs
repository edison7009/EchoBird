use super::*;
use axum::body::{Body, Bytes};
use axum::http::header;
use serde_json::json;

const SECRET: &str = "sk-proj-1234567890abcdefghijklmnopqrstuv";

fn engine() -> SharedEngine {
    Arc::new(Mutex::new(Engine::new([17; 32], b"test-router")))
}

fn protect(value: &mut Value, engine: &SharedEngine) -> Protection {
    Protection::with_engine(value, engine.clone()).unwrap()
}

fn token(engine: &SharedEngine) -> String {
    engine.lock().unwrap().secrets[0].token.clone()
}

fn restorer(engine: &SharedEngine) -> SharedRestorer {
    engine.lock().unwrap().restorer.clone()
}

#[test]
fn credentials_are_masked_everywhere_and_plain_personal_data_is_retained() {
    let engine = engine();
    let mut body = json!({"model":SECRET,"messages":[{"role":"user","content":format!("密钥为 {SECRET}，邮箱 user@example.com，电话 13800138000，主机 10.0.0.1")}]});
    protect(&mut body, &engine);
    assert_eq!(body["model"], SECRET);
    let content = body["messages"][0]["content"].as_str().unwrap();
    assert!(!content.contains(SECRET));
    for preserved in ["user@example.com", "13800138000", "10.0.0.1"] {
        assert!(content.contains(preserved));
    }
    assert_eq!(
        engine.lock().unwrap().restore(content, false),
        format!("密钥为 {SECRET}，邮箱 user@example.com，电话 13800138000，主机 10.0.0.1")
    );
}

#[test]
fn all_chosen_credential_types_have_positive_controls() {
    for text in [
        format!("API_KEY={SECRET}"),
        "token=abcdEFGH12345678".into(),
        "密码是 myP@ssw0rd".into(),
        "Bearer abcdefgh12345678".into(),
        "postgres://user:secret_password@host:5432/database".into(),
        "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abcdefgh12345678".into(),
        "-----BEGIN RSA PRIVATE KEY-----\nabc123\n-----END RSA PRIVATE KEY-----".into(),
    ] {
        let engine = engine();
        let mut body = json!({"messages":[{"content":text}]});
        protect(&mut body, &engine);
        let masked = body["messages"][0]["content"].as_str().unwrap();
        assert_ne!(masked, text);
        assert_eq!(engine.lock().unwrap().restore(masked, false), text);
    }
}

#[test]
fn tracked_categories_count_replacements_once_and_keep_known_values_across_requests() {
    let engine = engine();
    let original = json!({"messages":[{"content":format!("{SECRET} {SECRET} token=abcdEFGH12345678 password=fixturepassword -----BEGIN RSA PRIVATE KEY-----\nabc123\n-----END RSA PRIVATE KEY-----")},{"content":[{"type":"tool_use","input":{"secret":"other-credential-fixture"}}]}]});
    let mut body = original.clone();
    let protection = Protection::with_engine_until(
        &mut body,
        engine.clone(),
        Instant::now() + PROCESS_TIMEOUT,
        true,
    )
    .unwrap();
    assert_eq!(protection.categories(), [2, 1, 1, 1, 1]);
    let mut restored = body.clone();
    stream::restore_value(&restorer(&engine), &mut restored).unwrap();
    assert_eq!(restored, original);
    let again = Protection::with_engine_until(
        &mut body,
        engine.clone(),
        Instant::now() + PROCESS_TIMEOUT,
        true,
    )
    .unwrap();
    assert_eq!(
        again.categories(),
        [0; 5],
        "existing placeholders are not counted again"
    );
    let mut later = json!({"messages":[{"content":"reuse abcdEFGH12345678 and fixturepassword without labels"}]});
    let later = Protection::with_engine_until(
        &mut later,
        engine.clone(),
        Instant::now() + PROCESS_TIMEOUT,
        true,
    )
    .unwrap();
    assert_eq!(later.categories(), [0, 1, 1, 0, 0]);
    let mut untracked = original;
    let untracked = Protection::with_engine_until(
        &mut untracked,
        engine,
        Instant::now() + PROCESS_TIMEOUT,
        false,
    )
    .unwrap();
    assert_eq!(untracked.categories(), [0; 5]);
}

#[test]
fn multi_turn_values_and_discovery_after_first_occurrence_are_consistent() {
    let engine = engine();
    let mut first = json!({"messages":[{"content":"the value is abcdEFGH12345678"},{"content":"DB_PASSWORD=abcdEFGH12345678"}]});
    protect(&mut first, &engine);
    let expected = token(&engine);
    assert_eq!(
        first["messages"][0]["content"],
        format!("the value is {expected}")
    );
    let mut second = json!({"messages":[{"content":"It is still abcdEFGH12345678, without its original label."}]});
    protect(&mut second, &engine);
    assert!(second.to_string().contains(&expected));
    assert!(!second.to_string().contains("abcdEFGH12345678"));
    let other = Arc::new(Mutex::new(Engine::new([17; 32], b"another-router")));
    let mut body = json!({"messages":[{"content":"DB_PASSWORD=abcdEFGH12345678"}]});
    protect(&mut body, &other);
    assert_ne!(token(&other), expected);
}

#[test]
fn previously_generated_tokens_are_not_masked_again_by_short_known_values() {
    let engine = engine();
    let mut body =
        json!({"messages":[{"content":[{"type":"tool_use","input":{"password":"SECRET"}}]}]});
    protect(&mut body, &engine);
    let once = body.clone();
    protect(&mut body, &engine);
    assert_eq!(body, once);
    let mut reply = json!({"content":body["messages"][0]["content"][0]["input"]["password"]});
    stream::restore_value(&restorer(&engine), &mut reply).unwrap();
    assert_eq!(reply["content"], "SECRET");
}

#[test]
fn chinese_token_labels_work_without_masking_questions_about_passwords() {
    let engine = engine();
    let mut body = json!({"messages":[{"content":"Token 是 9d8F7e6D5c4B3a21；API Key：abcDef12345xyz；密码是什么，Token 是什么意思？"}],
        "input":[{"type":"tool_use","input":{"密码":"中文密码"}}]});
    protect(&mut body, &engine);
    let text = body["messages"][0]["content"].as_str().unwrap();
    assert!(!text.contains("9d8F7e6D5c4B3a21"));
    assert!(!text.contains("abcDef12345xyz"));
    assert!(text.contains("密码是什么，Token 是什么意思？"));
    assert_ne!(body["input"][0]["input"]["密码"], "中文密码");
}

#[test]
fn additional_key_formats_and_private_key_labels_are_protected() {
    for (text, value) in [
        (
            "PRIVATE_KEY=demoSigningCredential98765",
            "demoSigningCredential98765",
        ),
        (
            "{\"private_key\":\"demoSigningCredential98765\"}",
            "demoSigningCredential98765",
        ),
        (
            "Use gw_sk_demoRelayCredential987654321",
            "gw_sk_demoRelayCredential987654321",
        ),
        (
            "Use sk_demoServiceCredential987654321",
            "sk_demoServiceCredential987654321",
        ),
        ("秘钥为 Abc123Signing987", "Abc123Signing987"),
        ("私钥是 Abc123Signing987", "Abc123Signing987"),
        ("私鑰為 Abc123Signing987", "Abc123Signing987"),
        ("token 为 Abc123Signing987", "Abc123Signing987"),
        ("API Key：Abc123Signing987", "Abc123Signing987"),
        ("密钥为 67382910", "67382910"),
        ("password: DemoPassword987\nuser: sample", "DemoPassword987"),
        ("TOKEN=Abc123Signing987", "Abc123Signing987"),
        ("token: \"Uint8Array\"", "Uint8Array"),
        ("token 是 \"expired\"", "expired"),
        ("密钥为 '4826'", "4826"),
        ("password:\n  \"DemoPassword987\"", "DemoPassword987"),
        ("password=longletterssecret", "longletterssecret"),
        ("token 是 abcdefghijklmnop", "abcdefghijklmnop"),
        ("密码是 4826", "4826"),
    ] {
        // A new engine per case prevents a previous match from hiding a missed rule.
        let engine = engine();
        let mut body = json!({"input":text});
        protect(&mut body, &engine);
        let masked = body["input"].as_str().unwrap();
        assert!(
            !masked.contains(value),
            "credential was left unchanged: {text}"
        );
        assert_eq!(engine.lock().unwrap().restore(masked, false), text);
    }
}

#[test]
fn code_types_calls_and_operational_numbers_are_not_credentials() {
    for text in [
        "interface Auth { token: Uint8Array; }",
        "const token = createSessionToken(options)",
        "const password = hashPassword (input)",
        "submit({ token: state.currentToken, enabled: true })",
        "const keys = { private_key: credentials.signingKey, };",
        "interface Keys { secretKey: Uint8Array; }",
        "token 是 expired",
        "token 是 3600000 ms",
        "RSA 密钥为 2048 位",
        "密钥为 4096000 位",
        "令牌是 900000 毫秒",
        "生日：2026-10-26，deadline 2026-11-01，timestamp: 1792903687",
        "max_tokens=12345678 and total_tokens=87654321",
        "请输入密钥，密码是什么？",
        "密钥为 ${API_KEY}",
    ] {
        let engine = engine();
        let mut body = json!({"input":text});
        let original = body.clone();
        protect(&mut body, &engine);
        assert_eq!(body, original, "non-credential text was changed: {text}");
        let mut signed = json!({"messages":[{"content":[{"type":"thinking","thinking":text,"signature":"sealed"}]}]});
        let original = signed.clone();
        protect(&mut signed, &engine);
        assert_eq!(signed, original);
    }
}

#[test]
fn runtime_handles_remain_literal_in_text_and_structured_tool_payloads() {
    let memory = engine();
    let snapshot = json!({
        "element_token":"s00000001",
        "nodeToken":"n00000003",
        "handle-token":"e17",
        "snapshot_token":"s0000002a",
        "ELEMENT_TOKEN":"SABCDEF01",
        "snapshotToken":"s000000000000002a",
        "element_index":74,
        "frame":{"x":28,"y":28},
        "label":"确认按钮"
    });
    let text = snapshot.to_string();
    for mut body in [
        json!({"messages":[{"role":"tool","content":text}]}),
        json!({"messages":[{"content":[{"type":"tool_result","content":snapshot}]}]}),
        json!({"messages":[{"content":[{"type":"tool_use","input":snapshot}]}]}),
        json!({"messages":[{"tool_calls":[{"function":{"name":"click","arguments":text}}]}]}),
        json!({"input":[{"type":"function_call_output","output":text}]}),
        json!({"input":[{"type":"thinking","thinking":text,"signature":"sealed"}]}),
        json!({"input":format!("Snapshot:\n{}\nClick the button.", serde_json::to_string_pretty(&snapshot).unwrap())}),
        json!({"input":"element_token=s00000001 node_token=n00000003 handle_token=e17 snapshot_token=s0000002a"}),
    ] {
        let original = body.clone();
        protect(&mut body, &memory);
        assert_eq!(body, original, "runtime handle payload was changed");
        assert!(memory.lock().unwrap().secrets.is_empty());
    }
}

#[test]
fn runtime_handle_exceptions_keep_real_credentials_protected() {
    let payload = json!({
        "element_token":"s00000001",
        "snapshot_token":"s0000002a",
        "handle_token":SECRET,
        "access_token":"opaqueAccessCredential98765",
        "api_key":"opaqueApiCredential98765",
        "password":"actualPassword98765"
    });
    for content in [payload.clone(), Value::String(payload.to_string())] {
        let memory = engine();
        let mut body = json!({"messages":[{"content":[{"type":"tool_result","content":content}]}]});
        protect(&mut body, &memory);
        let masked = &body["messages"][0]["content"][0]["content"];
        let masked = masked.as_str().map_or_else(
            || masked.clone(),
            |text| serde_json::from_str(text).unwrap(),
        );
        assert_eq!(masked["element_token"], "s00000001");
        assert_eq!(masked["snapshot_token"], "s0000002a");
        for key in ["handle_token", "access_token", "api_key", "password"] {
            assert_ne!(
                masked[key], payload[key],
                "credential under {key} was exposed"
            );
        }
        let mut reply = json!({"choices":[{"message":{"tool_calls":[{"function":{
            "name":"use_result", "arguments":masked.to_string()
        }}]}}]});
        stream::restore_value(&restorer(&memory), &mut reply).unwrap();
        let restored: Value = serde_json::from_str(
            reply["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(restored, payload);
    }
    for (field, value) in [
        ("access_token", "s00000001"),
        ("session_token", "s00000001"),
        ("node_access_token", "s00000001"),
        ("api_key", "s00000001"),
        ("password", "s00000001"),
        ("my_element_token", "s00000001"),
        ("element_token", "opaqueCredential98765"),
    ] {
        let memory = engine();
        let mut body = json!({"input":format!("{field}={value}")});
        protect(&mut body, &memory);
        assert!(
            !body["input"].as_str().unwrap().contains(value),
            "credential was exposed: {field}"
        );
    }
}

#[test]
fn runtime_handle_values_stay_protected_when_already_known_as_credentials() {
    let memory = engine();
    let mut first = json!({"input":"password=s00000001"});
    protect(&mut first, &memory);
    for content in [
        json!({"element_token":"s00000001"}),
        Value::String(r#"{"element_token":"s00000001"}"#.into()),
    ] {
        let mut body = json!({"messages":[{"content":[{"type":"tool_result","content":content}]}]});
        protect(&mut body, &memory);
        assert!(!body.to_string().contains("s00000001"));
    }
}

#[test]
fn model_and_toolchain_names_remain_literal_beside_credentials() {
    let text = format!(
        "[providers]\ngroqAPIKey = \"{SECRET}\"\ngroqAPIModel = \"llama3-70b-8192\"\n\
         claudeAPIModel = \"claude-3-5-sonnet-20241022\"\n\
         --model mixtral-8x7b-32768\nchannel = \"nightly-2026-09-01\""
    );
    for (mut body, pointer) in [
        (
            json!({"model":"llama3-70b-8192","messages":[{"role":"user","content":text}]}),
            "/messages/0/content",
        ),
        (
            json!({"model":"claude-3-5-sonnet-20241022","messages":[{"role":"user","content":text}],"max_tokens":64}),
            "/messages/0/content",
        ),
        (
            json!({"model":"mixtral-8x7b-32768","input":[{"role":"user","content":[{"type":"input_text","text":text}]}]}),
            "/input/0/content/0/text",
        ),
    ] {
        let engine = engine();
        let original = body.clone();
        protect(&mut body, &engine);
        assert_eq!(engine.lock().unwrap().secrets.len(), 1);
        assert_eq!(
            body.pointer(pointer).unwrap(),
            &text.replace(SECRET, &token(&engine))
        );
        assert_eq!(body["model"], original["model"]);
        stream::restore_value(&restorer(&engine), &mut body).unwrap();
        assert_eq!(body, original);
    }
}

#[test]
fn explicit_credentials_are_not_exempted_by_model_or_toolchain_shaped_values() {
    for value in [
        "llama3-70b-8192",
        "claude-3-5-sonnet-20241022",
        "mixtral-8x7b-32768",
        "nightly-2026-09-01",
        "correct-horse-battery-staple",
        "maple-4-river-77",
    ] {
        for mut body in [
            json!({"input":format!("password = \"{value}\"")}),
            json!({"messages":[{"content":[{"type":"tool_use","id":"call-1","name":"configure","input":{"password":value}}]}]}),
        ] {
            let engine = engine();
            let original = body.clone();
            protect(&mut body, &engine);
            assert!(!body.to_string().contains(value));
            let mut later = json!({"input":format!("Use the previously supplied value {value}")});
            protect(&mut later, &engine);
            assert!(!later.to_string().contains(value));
            stream::restore_value(&restorer(&engine), &mut body).unwrap();
            assert_eq!(body, original);
        }
    }
}

#[test]
fn unquoted_credentials_need_code_context_before_they_can_be_exempted() {
    for (text, value) in [
        ("password=DemoPass987(654)", "DemoPass987(654)"),
        ("password: DemoPass987, user=demo", "DemoPass987"),
        ("password: demo.secret987, user=demo", "demo.secret987"),
        (
            "private_key: credentials.signingKey,",
            "credentials.signingKey",
        ),
        ("secretKey: Uint8Array;", "Uint8Array"),
    ] {
        let memory = engine();
        let mut body = json!({"input":text});
        protect(&mut body, &memory);
        let masked = body["input"].as_str().unwrap();
        assert!(
            !masked.contains(value),
            "ambiguous literal was exempted: {text}"
        );
        assert_eq!(memory.lock().unwrap().restore(masked, false), text);
        let mut sealed =
            json!({"input":[{"type":"thinking","thinking":text,"signature":"sealed"}]});
        assert!(Protection::with_engine(&mut sealed, engine()).is_err());
    }
}

#[test]
fn english_credential_labels_can_follow_chinese_without_a_space() {
    for text in [
        "我的token是Abc123Signing987",
        "这个API Key为Abc123Signing987",
    ] {
        let memory = engine();
        let mut body = json!({"input":text});
        protect(&mut body, &memory);
        let masked = body["input"].as_str().unwrap();
        assert!(!masked.contains("Abc123Signing987"));
        assert_eq!(memory.lock().unwrap().restore(masked, false), text);
        let mut sealed =
            json!({"input":[{"type":"thinking","thinking":text,"signature":"sealed"}]});
        assert!(Protection::with_engine(&mut sealed, engine()).is_err());
    }
}

#[test]
fn structured_tool_arguments_do_not_inherit_protocol_field_exemptions() {
    let engine = engine();
    let args = json!({"password":"q\"uote\\line\n密码", "id":SECRET, "signature":SECRET});
    let mut body = json!({"messages":[{"content":[{"type":"tool_use","id":"call_123","name":"write","input":args}]}],
        "tools":[{"name":SECRET,"input_schema":{"enum":[SECRET]},"description":format!("Use {SECRET}")} ]});
    protect(&mut body, &engine);
    assert_eq!(body["messages"][0]["content"][0]["id"], "call_123");
    assert_eq!(body["tools"][0]["name"], SECRET);
    assert_eq!(body["tools"][0]["input_schema"]["enum"][0], SECRET);
    assert!(!body["messages"].to_string().contains(SECRET));
    assert_ne!(
        body["messages"][0]["content"][0]["input"]["password"],
        args["password"]
    );
    let mut encoded = json!({"messages":[{"tool_calls":[{"id":"call_123","function":{"name":"write","arguments":args.to_string()}}]}]});
    protect(&mut encoded, &engine);
    assert!(!encoded.to_string().contains(SECRET));
    let mut response = json!({"choices":[{"message":{"tool_calls":[{"id":"call_123","function":{"name":"write","arguments":encoded["messages"][0]["tool_calls"][0]["function"]["arguments"]}}]}}]});
    stream::restore_value(&restorer(&engine), &mut response).unwrap();
    let restored: Value = serde_json::from_str(
        response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(restored, args);
}

#[test]
fn signed_content_and_media_are_not_rewritten_and_unsafe_signed_text_is_rejected() {
    let engine = engine();
    let mut body = json!({"messages":[{"content":[{"type":"thinking","thinking":"vendor text", "signature":SECRET},
        {"type":"image","source":{"type":"base64","data":SECRET}}]}]});
    let original = body.clone();
    protect(&mut body, &engine);
    assert_eq!(body, original);
    let mut unsafe_body = json!({"messages":[{"content":[{"type":"thinking","thinking":SECRET,"signature":"sealed"}]}]});
    assert!(Protection::with_engine(&mut unsafe_body, engine.clone()).is_err());
    let mut plain = json!({"input":format!("API_KEY={SECRET}")});
    protect(&mut plain, &engine);
    let placeholder = token(&engine);
    let mut response = json!({"content":[{"type":"thinking","thinking":placeholder,"signature":"sealed"},{"type":"text","text":placeholder},{"type":"reasoning.text","text":placeholder,"signature":"sealed"}]});
    stream::restore_value(&restorer(&engine), &mut response).unwrap();
    assert_eq!(response["content"][0]["thinking"], placeholder);
    assert_eq!(response["content"][1]["text"], SECRET);
    assert_eq!(response["content"][2]["text"], placeholder);
}

#[tokio::test]
async fn settings_are_off_by_default_and_invalid_state_does_not_send_plaintext() {
    let dir = std::env::temp_dir().join(format!("content-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("settings.json");
    assert_eq!(enabled(&path).await, Ok(false));
    for (content, expected) in [
        ("{}", Ok(false)),
        (r#"{"sensitiveInfoProtection":true}"#, Ok(true)),
        (r#"{"sensitiveInfoProtection":false}"#, Ok(false)),
        ("invalid", Err(FAILURE)),
        (r#"{"sensitiveInfoProtection":"false"}"#, Err(FAILURE)),
    ] {
        std::fs::write(&path, content).unwrap();
        assert_eq!(enabled(&path).await, expected);
    }
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

fn chat_delta(choice: u64, text: &str) -> Vec<u8> {
    format!(
        "data: {}\n\n",
        json!({"choices":[{"index":choice,"delta":{"content":text},"finish_reason":null}]})
    )
    .into_bytes()
}

fn joined_content(chunks: Vec<Bytes>, choice: u64) -> String {
    let text: String = chunks
        .into_iter()
        .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
        .collect();
    text.split("\n\n")
        .filter_map(|event| event.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .flat_map(|value| value["choices"].as_array().cloned().unwrap_or_default())
        .filter(|value| value["index"] == choice)
        .filter_map(|value| value["delta"]["content"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn streaming_restores_all_placeholder_splits_and_transport_byte_boundaries() {
    let engine = engine();
    protect(&mut json!({"input":SECRET}), &engine);
    let placeholder = token(&engine);
    for split in 1..placeholder.len() {
        let mut events = stream::Events::new(restorer(&engine));
        let bytes = [
            chat_delta(0, &format!("中文 {}", &placeholder[..split])),
            chat_delta(0, &format!("{} end", &placeholder[split..])),
            b"data: [DONE]\n\n".to_vec(),
        ]
        .concat();
        let mut output = Vec::new();
        for byte in bytes {
            output.extend(events.push(&[byte]).unwrap());
        }
        output.extend(events.finish().unwrap());
        assert_eq!(joined_content(output, 0), format!("中文 {SECRET} end"));
    }
}

#[test]
fn interleaved_choices_keep_separate_carries_and_heartbeats_are_not_held() {
    let engine = engine();
    protect(&mut json!({"input":SECRET}), &engine);
    let placeholder = token(&engine);
    let mut events = stream::Events::new(restorer(&engine));
    assert!(events
        .push(&chat_delta(0, &placeholder[..13]))
        .unwrap()
        .is_empty());
    assert_eq!(
        events.push(b": single-line keepalive\n").unwrap(),
        vec![Bytes::from_static(b": single-line keepalive\n")]
    );
    assert_eq!(events.push(b"\n").unwrap(), vec![Bytes::from_static(b"\n")]);
    assert_eq!(
        events.push(b": keepalive\n\n").unwrap(),
        vec![Bytes::from_static(b": keepalive\n\n")]
    );
    let mut output = events.push(&chat_delta(1, "other choice")).unwrap();
    output.extend(events.push(&chat_delta(0, &placeholder[13..])).unwrap());
    output.extend(events.finish().unwrap());
    assert_eq!(joined_content(output.clone(), 0), SECRET);
    assert_eq!(joined_content(output, 1), "other choice");
}

#[tokio::test]
async fn complete_responses_restore_text_and_arguments_and_remove_stale_lengths() {
    let engine = engine();
    let protection = protect(&mut json!({"input":SECRET}), &engine);
    let placeholder = token(&engine);
    let response = Response::builder().header(header::CONTENT_TYPE,"application/json")
        .header(header::CONTENT_LENGTH,"1").header(header::ETAG,"old")
        .body(Body::from(json!({"output":[{"type":"message","content":[{"type":"output_text","text":placeholder}]}]}).to_string())).unwrap();
    let response = protection.restore(response).await;
    assert!(!response.headers().contains_key(header::CONTENT_LENGTH));
    assert!(!response.headers().contains_key(header::ETAG));
    let body = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    assert!(std::str::from_utf8(&body).unwrap().contains(SECRET));
}

#[test]
fn unmatched_tokens_and_partial_tokens_at_eof_are_preserved() {
    let engine = engine();
    assert_eq!(
        engine.lock().unwrap().restore("{{SECRET_unknown}}", false),
        "{{SECRET_unknown}}"
    );
    let mut events = stream::Events::new(restorer(&engine));
    events.push(&chat_delta(0, "ordinary text {{SEC")).unwrap();
    let output = events.finish().unwrap();
    assert_eq!(joined_content(output, 0), "ordinary text {{SEC");
}

#[test]
fn protocol_settings_are_preserved_while_unsigned_reasoning_and_business_fields_are_protected() {
    let engine = engine();
    let schema = json!({"format":{"type":"json_schema","schema":{"enum":[SECRET]}}});
    let mut body = json!({"text":schema,"input":[{"type":"reasoning","summary":[{"type":"summary_text","text":SECRET}]}],
        "messages":[{"content":[{"type":"tool_result","tool_use_id":"call_original","content":{"id":SECRET,"tools":[{"OPENAI_API_KEY":"custom-gateway-secret-12345"}],"source":{"type":"base64","media_type":"image/png","data":SECRET}}}]}]});
    protect(&mut body, &engine);
    assert_eq!(body["text"], schema);
    assert_ne!(body["input"][0]["summary"][0]["text"], SECRET);
    let result = &body["messages"][0]["content"][0];
    assert_eq!(result["tool_use_id"], "call_original");
    assert_ne!(result["content"]["id"], SECRET);
    assert_ne!(
        result["content"]["tools"][0]["OPENAI_API_KEY"],
        "custom-gateway-secret-12345"
    );
    assert_eq!(result["content"]["source"]["data"], SECRET);
    let mut restored = json!({"output":body["input"]});
    stream::restore_value(&restorer(&engine), &mut restored).unwrap();
    assert_eq!(restored["output"][0]["summary"][0]["text"], SECRET);
    let mut counts = json!({"input":"max_tokens=12345678 and total_tokens=87654321"});
    let before = counts.clone();
    protect(&mut counts, &engine);
    assert_eq!(counts, before);
}

#[test]
fn streamed_tool_arguments_restore_json_escaping_across_all_supported_event_shapes() {
    for protocol in ["chat", "responses", "messages"] {
        let engine = engine();
        let args = json!({"password":"q\"uote\\line\n密码"});
        let mut request = json!({"input":[{"type":"function_call","arguments":args.to_string()}]});
        protect(&mut request, &engine);
        let masked = request["input"][0]["arguments"].as_str().unwrap();
        let split = masked.find(PREFIX).unwrap() + 15;
        let mut events = stream::Events::new(restorer(&engine));
        let mut output = Vec::new();
        for part in [&masked[..split], &masked[split..]] {
            let event = match protocol {
                "chat" => {
                    json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":part}}]}}]})
                }
                "responses" => {
                    json!({"type":"response.function_call_arguments.delta","item_id":"call_1","output_index":0,"delta":part})
                }
                _ => {
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":part}})
                }
            };
            output.extend(
                events
                    .push(format!("data: {event}\n\n").as_bytes())
                    .unwrap(),
            );
        }
        output.extend(events.push(b"data: [DONE]\n\n").unwrap());
        output.extend(events.finish().unwrap());
        let wire: String = output
            .into_iter()
            .map(|bytes| String::from_utf8(bytes.to_vec()).unwrap())
            .collect();
        let restored: String = wire
            .split("\n\n")
            .filter_map(|frame| frame.strip_prefix("data: "))
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .filter_map(|event| match protocol {
                "chat" => event["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
                    .as_str()
                    .map(str::to_owned),
                "responses" => event["delta"].as_str().map(str::to_owned),
                _ => event["delta"]["partial_json"].as_str().map(str::to_owned),
            })
            .collect();
        assert_eq!(serde_json::from_str::<Value>(&restored).unwrap(), args);
    }
}

#[tokio::test]
async fn local_upstream_sees_protected_content_on_all_three_protocols_and_auth_stays_intact() {
    use super::super::{protected_route, AppState, Candidate, ClientProtocol};
    use axum::{extract::Json, http::HeaderMap, routing::post, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let token_pattern = Regex::new(r"\{\{SECRET_[a-f0-9]{32}\}\}").unwrap();
    for enabled in [false, true] {
        for protocol in [
            ClientProtocol::Chat,
            ClientProtocol::Responses,
            ClientProtocol::Messages,
        ] {
            let root =
                std::env::temp_dir().join(format!("router-protection-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let mut state = AppState::for_tests().unwrap();
            state.config_path = root.join("router.json");
            state.protection_settings_path = root.join("settings.json");
            std::fs::write(
                &state.protection_settings_path,
                json!({"sensitiveInfoProtection":enabled}).to_string(),
            )
            .unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let captured = calls.clone();
            let seen = Arc::new(Mutex::new(None::<String>));
            let token_pattern = token_pattern.clone();
            let app = Router::new().route("/v1/{*path}", post(move |headers: HeaderMap, Json(body): Json<Value>| {
                let calls = captured.clone();
                let seen = seen.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(headers[header::AUTHORIZATION], "Bearer upstream-auth-fixture");
                    let encoded = body.to_string();
                    let echo = if enabled {
                        assert!(!encoded.contains(SECRET));
                        token_pattern.find(&encoded).unwrap().as_str().to_owned()
                    } else {
                        assert!(encoded.contains(SECRET)); SECRET.into()
                    };
                    if enabled {
                        let mut first = seen.lock().unwrap();
                        if let Some(expected) = first.as_ref() { assert_eq!(expected,&echo); }
                        else { *first = Some(echo.clone()); }
                    }
                    if body["model"] == "primary-model" {
                        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":{"message":"temporarily unavailable","type":"server_error"}})));
                    }
                    (axum::http::StatusCode::OK, Json(match protocol {
                        ClientProtocol::Responses => json!({"id":"resp_test","object":"response","status":"completed","output":[{"id":"msg_test","type":"message","role":"assistant","content":[{"type":"output_text","text":echo}]}],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}),
                        _ => json!({"id":"chat_test","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":echo},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
                    }))
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let candidate = Candidate {
                internal_id: "fixture".into(),
                model_id: "fixture-model".into(),
                base_url: format!("http://{addr}/v1"),
                anthropic_url: None,
                api_key: "upstream-auth-fixture".into(),
                account_source: None,
                fingerprint: "fixture".into(),
                attempt: None,
            };
            let body = match protocol {
                ClientProtocol::Responses => json!({"model":"auto","input":SECRET,"stream":false}),
                _ => {
                    json!({"model":"auto","messages":[{"role":"user","content":SECRET}],"max_tokens":64,"stream":false})
                }
            };
            let mut candidates = vec![candidate.clone()];
            if enabled {
                let mut primary = candidate;
                primary.internal_id = "primary".into();
                primary.model_id = "primary-model".into();
                primary.fingerprint = "primary".into();
                candidates.insert(0, primary);
            }
            let response =
                protected_route(&state, &HeaderMap::new(), body, candidates, protocol).await;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            let bytes = axum::body::to_bytes(response.into_body(), 8192)
                .await
                .unwrap();
            assert!(std::str::from_utf8(&bytes).unwrap().contains(SECRET));
            assert!(!std::str::from_utf8(&bytes).unwrap().contains(PREFIX));
            assert_eq!(calls.load(Ordering::SeqCst), if enabled { 2 } else { 1 });
            assert_eq!(root.join("content-protection.key").exists(), enabled);
            task.abort();
            std::fs::remove_file(&state.protection_settings_path).unwrap();
            if enabled {
                std::fs::remove_file(root.join("content-protection.key")).unwrap();
            }
            std::fs::remove_dir(root).unwrap();
        }
    }
}

#[tokio::test]
async fn unreadable_protection_fails_before_upstream_and_authentication_precedes_protection() {
    use super::super::{handle_chat, protected_route, AppState, ClientProtocol};
    use axum::{
        extract::State,
        http::{HeaderMap, HeaderValue, StatusCode},
        Json,
    };
    let root =
        std::env::temp_dir().join(format!("router-protection-fail-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let mut state = AppState::for_tests().unwrap();
    state.config_path = root.join("router.json");
    state.protection_settings_path = root.join("settings.json");
    std::fs::write(&state.protection_settings_path, "invalid").unwrap();
    std::fs::write(
        &state.config_path,
        json!({"modelId":"auto","enabled":true,"apiKey":"local-fixture","candidateIds":[]})
            .to_string(),
    )
    .unwrap();
    let body = json!({"model":"auto","messages":[{"role":"user","content":SECRET}]});
    let unauthorized = handle_chat(
        State(state.clone()),
        HeaderMap::new(),
        Ok(Json(body.clone())),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer local-fixture"),
    );
    let unavailable =
        protected_route(&state, &headers, body, Vec::new(), ClientProtocol::Chat).await;
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    let error = axum::body::to_bytes(unavailable.into_body(), 4096)
        .await
        .unwrap();
    assert!(!std::str::from_utf8(&error).unwrap().contains(SECRET));
    assert!(!root.join("content-protection.key").exists());
    for path in [&state.config_path, &state.protection_settings_path] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn terminal_preview_uses_the_actual_replacement_without_mutating_payload() {
    let store = engine();
    let mut body = json!({"messages":[{"role":"user","content":"password = DemoPass!2026"}]});
    let protection = protect(&mut body, &store);
    let before = body.clone();
    let segments = protection.terminal_segments(body["messages"][0]["content"].as_str().unwrap());
    assert_eq!(
        segments.iter().map(|s| s.text.as_str()).collect::<String>(),
        "password = DemoPass!2026"
    );
    let sensitive = segments
        .iter()
        .find(|s| s.protected_text.is_some())
        .unwrap();
    assert_eq!(sensitive.kind, Some(2));
    assert_eq!(
        sensitive.protected_text.as_deref(),
        Some(token(&store).as_str())
    );
    assert_eq!(body, before);
    assert!(!body.to_string().contains("DemoPass!2026"));
}
