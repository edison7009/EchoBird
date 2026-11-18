use super::*;
use axum::body::Bytes;
use std::sync::{Arc, Mutex};

fn parse(text: &str, sse: bool) -> Option<TokenUsage> {
    let mut parser = Parser::new(sse);
    for byte in text.as_bytes() {
        parser.push(&[*byte]);
    }
    parser.finish()
}

#[test]
fn parses_reported_json_without_counting_cached_or_reasoning_details_twice() {
    let usage = parse(r#"{"usage":{"prompt_tokens":12,"completion_tokens":8,"total_tokens":20,"prompt_tokens_details":{"cached_tokens":10},"completion_tokens_details":{"reasoning_tokens":5}}}"#, false).unwrap();
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.total_tokens,
            usage.reported_responses
        ),
        (12, 8, 20, 1)
    );
    assert!(parse(r#"{"content":"usage: 999999 tokens"}"#, false).is_none());
    assert!(parse(
        r#"{"usage":{"input_tokens":-1,"output_tokens":"12"}}"#,
        false
    )
    .is_none());
}

#[test]
fn merges_split_anthropic_usage_and_cache_inputs_without_adding_cumulative_events() {
    let text = "event: message_start\r\ndata: {\"message\":{\"usage\":{\"input_tokens\":2,\"cache_read_input_tokens\":10,\"cache_creation_input_tokens\":3,\"output_tokens\":1}}}\r\n\r\ndata: {\"usage\":{\"output_tokens\":4}}\n\ndata: {\"usage\":{\"output_tokens\":9}}\n\ndata: {\"usage\":{\"output_tokens\":9}}\n\n";
    let usage = parse(text, true).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.total_tokens),
        (15, 9, 24)
    );
    assert_eq!(usage.reported_responses, 1);
}

#[test]
fn responses_terminal_repeats_are_one_usage_report_and_no_terminal_separator_is_required() {
    let frame = "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":4,\"output_tokens\":6,\"total_tokens\":10}}}";
    let usage = parse(&format!("{frame}\n\n{frame}"), true).unwrap();
    assert_eq!((usage.total_tokens, usage.reported_responses), (10, 1));
}

#[test]
fn bounds_buffers_and_recovers_after_an_oversized_stream_event() {
    let mut parser = Parser::new(true);
    parser.push(&vec![b'x'; MAX_EVENT_BYTES + 100]);
    assert!(parser.bytes.len() <= MAX_EVENT_BYTES);
    parser.push(b"\n\ndata: {\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":2}}\n\n");
    assert_eq!(parser.finish().unwrap().total_tokens, 3);
    let mut json = Parser::new(false);
    json.push(&vec![b'x'; MAX_JSON_BYTES + 1]);
    assert!(json.bytes.is_empty());
    assert!(json.finish().is_none());
}

#[tokio::test]
async fn preserves_body_headers_extensions_and_errors_and_records_once() {
    let result = Arc::new(Mutex::new(Vec::new()));
    let output = result.clone();
    let mut response = Response::builder()
        .header("content-type", "text/event-stream")
        .header("x-fixture", "kept")
        .body(Body::from_stream(futures_util::stream::iter(vec![
            Ok::<_, std::io::Error>(Bytes::from_static(
                b"data: {\"usage\":{\"total_tokens\":7}}\n\n",
            )),
            Err(std::io::Error::other("fixture interruption")),
        ])))
        .unwrap();
    response.extensions_mut().insert(123u32);
    let response = observe(response, move |usage| output.lock().unwrap().push(usage));
    assert_eq!(response.headers()["x-fixture"], "kept");
    assert_eq!(response.extensions().get::<u32>(), Some(&123));
    assert!(response.into_body().collect().await.is_err());
    assert_eq!(result.lock().unwrap().len(), 1);
    assert_eq!(result.lock().unwrap()[0].total_tokens, 7);
}

#[test]
fn telemetry_ignores_role_heartbeats_and_keeps_cache_categories_and_tiers_billable() {
    let mut parser = Parser::new(true);
    parser.started = Instant::now() - std::time::Duration::from_millis(100);
    parser.push(b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n");
    assert!(parser.first_output_ms.is_none());
    parser.push(b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n");
    assert!(parser.first_output_ms.unwrap() >= 100);
    parser.push(b"data: {\"usage\":{\"prompt_tokens\":20,\"completion_tokens\":10,\"prompt_tokens_details\":{\"cached_tokens\":12}}}\n\n");
    parser.finish();
    assert_eq!(parser.cache_read, 12);
    assert_eq!(parser.usage.input_tokens, 20);
    assert!(parser.input_seen && parser.output_seen);
    let mut anthropic = Parser::new(false);
    anthropic.push(br#"{"usage":{"input_tokens":5,"cache_read_input_tokens":10,"cache_creation_input_tokens":3,"cache_creation":{"ephemeral_1h_input_tokens":2},"output_tokens":4}}"#);
    anthropic.finish();
    assert_eq!(anthropic.usage.input_tokens, 18);
    assert_eq!(anthropic.cache_write_1h, 2);
    assert!(anthropic.first_output_ms.is_none());
}

#[tokio::test]
async fn telemetry_observes_failed_reported_usage_and_preserves_status_and_bytes() {
    let out = Arc::new(Mutex::new(None));
    let sink = out.clone();
    let text = r#"{"usage":{"input_tokens":20,"output_tokens":5}}"#;
    let response = Response::builder()
        .status(500)
        .header("content-type", "application/json")
        .body(Body::from(text))
        .unwrap();
    let response = observe_report(response, Instant::now(), move |r| {
        *sink.lock().unwrap() = Some(r)
    });
    assert_eq!(response.status(), 500);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        text
    );
    let report = out.lock().unwrap().take().unwrap();
    assert!(report.billable);
    assert_eq!(report.usage.total_tokens, 25);
    assert!(report.generation_ms.is_none());
}
