use super::*;
use http_body_util::BodyExt;

fn fixture() -> (PathBuf, Arc<Collector>) {
    let root = std::env::temp_dir().join(format!("router-statistics-{}", uuid::Uuid::new_v4()));
    let collector = Collector::open(root.join("counts.json")).unwrap();
    (root, collector)
}

fn successful(collector: &Arc<Collector>) {
    let request = collector.begin("group").unwrap();
    request.attempt("group", "candidate", "model", None);
    request.outcome("candidate", None);
    request.finish(true, false);
}

#[tokio::test]
async fn local_pet_earnings_share_the_persisted_router_ledger() {
    let (root, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    collector
        .observe(
            crate::services::account_proxy::json_response(
                &serde_json::json!({"usage":{"total_tokens":40}}),
            ),
            Some(&request),
            "",
        )
        .into_body()
        .collect()
        .await
        .unwrap();
    collector.flush().unwrap();
    let router_wallet = collector.token_credits().unwrap();
    assert_eq!(router_wallet.total, 40);

    let epoch = collector.epoch.load(Ordering::SeqCst);
    let merge = |store: &mut Store, _, _, now| {
        let now = (now / 1000) as i64;
        let since = store.pet_history.since(now, store.credits.total, false);
        store.pet_history.merge_native(
            since,
            now,
            BTreeMap::from([(
                pet_history::day(now),
                BTreeMap::from([("codex".into(), 65)]),
            )]),
        )
    };
    collector.update_if(epoch, merge);
    assert_eq!(collector.token_credits().unwrap().total, 40);
    collector.flush().unwrap();
    let writes = collector.writes.load(Ordering::Relaxed);
    collector.update_if(epoch, merge);
    collector.flush().unwrap();
    assert_eq!(collector.writes.load(Ordering::Relaxed), writes);
    let reopened = Collector::open(root.join("counts.json")).unwrap();
    let state = reopened.state.lock().unwrap();
    assert_eq!(state.durable_credits.ledger_id, router_wallet.ledger_id);
    assert_eq!(state.durable_credits.total, 65);
    drop(state);
    collector.configure(false);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn paused_or_old_generation_responses_cannot_credit_a_new_session() {
    let (root, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    let response = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"total_tokens":7}}),
        ),
        Some(&request),
        "",
    );
    collector.configure(false);
    collector.configure(true);
    response.into_body().collect().await.unwrap();
    collector.flush().unwrap();
    assert_eq!(collector.token_credits().unwrap().total, 0);
    let request = collector.begin("group").unwrap();
    let response = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"total_tokens":7}}),
        ),
        Some(&request),
        "",
    );
    response.into_body().collect().await.unwrap();
    collector.flush().unwrap();
    assert_eq!(collector.token_credits().unwrap().total, 7);
    collector.configure(false);
    assert!(collector.token_credits().is_err());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn delayed_headers_keep_the_request_generation_even_when_detail_capacity_is_full() {
    for capped in [false, true] {
        let (root, collector) = fixture();
        collector.configure(true);
        if capped {
            for index in 0..MAX_GROUPS {
                collector.begin(&format!("group-{index}")).unwrap();
            }
        }
        let request = collector.begin("group").unwrap();
        assert_eq!(request.admitted, !capped);
        request.attempt("group", "candidate", "model", None);
        collector.configure(false);
        collector.configure(true);
        let response = collector.observe(
            crate::services::account_proxy::json_response(
                &serde_json::json!({"usage":{"total_tokens":7}}),
            ),
            Some(&request),
            "candidate",
        );
        response.into_body().collect().await.unwrap();
        collector.flush().unwrap();
        assert_eq!(collector.token_credits().unwrap().total, 0);

        let current = collector.begin("group").unwrap();
        current.attempt("group", "candidate", "model", None);
        let response = collector.observe(
            crate::services::account_proxy::json_response(
                &serde_json::json!({"usage":{"total_tokens":7}}),
            ),
            Some(&current),
            "candidate",
        );
        response.into_body().collect().await.unwrap();
        current.outcome("candidate", None);
        current.finish(true, false);
        collector.flush().unwrap();
        assert_eq!(
            collector.token_credits().unwrap().total,
            7,
            "positive control"
        );
        if capped {
            let snapshot = collector.snapshot(1, None).unwrap();
            assert_eq!(snapshot.totals.calls, MAX_GROUPS as u64);
            assert_eq!(snapshot.totals.completed, 0);
            assert!(snapshot.rows.is_empty());
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn requests_started_while_paused_cannot_earn_credits_after_enabling() {
    let (root, collector) = fixture();
    let request = collector.begin("group");
    assert!(request.is_none());
    collector.configure(true);
    let response = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"total_tokens":7}}),
        ),
        request.as_ref(),
        "candidate",
    );
    response.into_body().collect().await.unwrap();
    assert_eq!(collector.token_credits().unwrap().total, 0);
    assert_eq!(collector.writes.load(Ordering::Relaxed), 0);
    assert!(!root.exists());
}

#[tokio::test]
async fn wallet_polling_reads_only_durable_credits_without_forcing_writes() {
    let (root, collector) = fixture();
    collector.configure(true);
    for _ in 0..6 {
        let request = collector.begin("group").unwrap();
        request.attempt("group", "candidate", "model", None);
        let response = collector.observe(
            crate::services::account_proxy::json_response(
                &serde_json::json!({"usage":{"total_tokens":7}}),
            ),
            Some(&request),
            "candidate",
        );
        response.into_body().collect().await.unwrap();
        request.outcome("candidate", None);
        request.finish(true, false);
        assert_eq!(collector.token_credits().unwrap().total, 0);
    }
    assert_eq!(collector.writes.load(Ordering::Relaxed), 0);
    collector.flush().unwrap();
    for _ in 0..6 {
        assert_eq!(collector.token_credits().unwrap().total, 42);
    }
    assert_eq!(collector.writes.load(Ordering::Relaxed), 1);
    println!("six dirty wallet polls: zero forced writes, one merged flush");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn lifetime_credits_survive_retention_and_group_capacity_without_persisting_contents() {
    let (root, collector) = fixture();
    collector.configure(true);
    for index in 0..MAX_GROUPS {
        assert!(collector.begin(&format!("group-{index}")).is_some());
    }
    let request = collector.begin("over-capacity").unwrap();
    assert!(!request.admitted);
    let response = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"input_tokens":3,"output_tokens":4}}),
        ),
        Some(&request),
        "",
    );
    response.into_body().collect().await.unwrap();
    collector
        .state
        .lock()
        .unwrap()
        .data
        .trim(clock().0 + RETENTION_DAYS + 1);
    collector.flush().unwrap();
    assert_eq!(collector.token_credits().unwrap().total, 7);
    assert!(collector.state.lock().unwrap().data.days.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn a_failed_disk_write_does_not_publish_uncommitted_credits_and_can_recover() {
    let (root, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    std::fs::write(&root, "fixture blocker").unwrap();
    let response = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"total_tokens":9}}),
        ),
        Some(&request),
        "",
    );
    response.into_body().collect().await.unwrap();
    assert!(collector.flush().is_err());
    assert!(collector.token_credits().is_err());
    assert_eq!(collector.state.lock().unwrap().durable_credits.total, 0);
    assert_eq!(std::fs::read_to_string(&root).unwrap(), "fixture blocker");
    std::fs::remove_file(&root).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    collector.flush().unwrap();
    assert_eq!(collector.token_credits().unwrap().total, 9);
    assert_eq!(collector.token_credits().unwrap().total, 9);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_off_has_no_counters_or_files_and_passive_snapshots_do_not_write() {
    let (root, collector) = fixture();
    assert!(collector.begin("group").is_none());
    assert_eq!(collector.snapshot(14, None).unwrap().totals.calls, 0);
    collector.flush().unwrap();
    assert!(!root.exists());
    assert_eq!(collector.writes.load(Ordering::Relaxed), 0);
}

#[test]
fn catalog_navigation_does_not_initialize_or_migrate_router_configuration() {
    let (root, _) = fixture();
    std::fs::create_dir_all(root.join("config")).unwrap();
    super::super::profiles::with_fixture_root(root.clone(), || {
        assert!(catalog().unwrap().groups.is_empty());
        let path = super::super::profiles::path_for(super::super::SMART_ROUTER_INTERNAL_ID);
        assert!(!path.exists());
        let original = serde_json::to_vec(&super::super::StoredConfig::default()).unwrap();
        std::fs::write(&path, &original).unwrap();
        assert_eq!(catalog().unwrap().groups.len(), 1);
        assert_eq!(
            std::fs::read(&path).unwrap(),
            original,
            "no new keys, defaults or migration writes"
        );
        assert_eq!(std::fs::read_dir(root.join("config")).unwrap().count(), 1);
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn fallbacks_authentication_retry_and_duplicate_outcomes_count_once() {
    let (root, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    request.protections([2, 1, 0, 0, 0]);
    request.attempt("group", "first", "model", None);
    request.outcome("first", Some(0));
    request.outcome("first", Some(0));
    request.attempt("group", "second", "model", None);
    request.outcome("second", None);
    request.outcome("second", None);
    request.finish(true, false);
    request.finish(false, true);
    let snapshot = collector.snapshot(1, None).unwrap();
    assert_eq!(
        (
            snapshot.totals.calls,
            snapshot.totals.completed,
            snapshot.totals.rescued
        ),
        (1, 1, 1)
    );
    assert_eq!(
        snapshot
            .rows
            .iter()
            .map(|row| row.counts.attempts)
            .sum::<u64>(),
        2
    );
    assert_eq!(
        snapshot
            .rows
            .iter()
            .map(|row| row.counts.failed)
            .sum::<u64>(),
        1
    );
    assert!(snapshot
        .rows
        .iter()
        .all(|row| row.counts.protections == [2, 1, 0, 0, 0]));
    collector.flush().unwrap();
    let bytes = std::fs::read(root.join("counts.json")).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    for private in [
        "candidate",
        "first",
        "second",
        "model",
        "http://",
        "apiKey",
        "prompt",
        "tokens",
    ] {
        assert!(
            !text.contains(&format!(":\"{private}\"")),
            "unexpected stored value: {private}"
        );
    }
    let loaded = Collector::open(root.join("counts.json")).unwrap();
    assert_eq!(loaded.snapshot(14, None).unwrap().totals.completed, 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancellation_and_late_results_after_disable_are_not_failures() {
    let (_, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    request.attempt("group", "candidate", "model", None);
    drop(Completion {
        request: request.clone(),
        response_ok: true,
        response_ready: true,
        done: false,
    });
    request.outcome("candidate", None);
    let snapshot = collector.snapshot(1, None).unwrap();
    assert_eq!(
        (
            snapshot.totals.completed,
            snapshot.totals.failed,
            snapshot.totals.cancelled
        ),
        (0, 0, 1)
    );
    assert_eq!(
        (
            snapshot.rows[0].counts.failed,
            snapshot.rows[0].counts.cancelled
        ),
        (0, 1)
    );
    let late = collector.begin("group").unwrap();
    late.attempt("group", "candidate", "model", None);
    collector.configure(false);
    collector.configure(true);
    late.outcome("candidate", None);
    late.finish(true, false);
    assert_eq!(collector.snapshot(1, None).unwrap().totals.completed, 0);
}

#[test]
fn stable_account_identity_ignores_model_and_credentials() {
    let source = AccountSource {
        provider: crate::models::model::AccountProvider::Codex,
        account_id: "private-account".into(),
    };
    let (_, first) = row_identity("group", "first", "model-a", Some(&source));
    let (_, second) = row_identity("group", "second", "model-b", Some(&source));
    assert_eq!(first.account, second.account);
    assert_ne!(first.model, second.model);
    assert!(!first.account.contains("private-account"));
}

#[test]
fn retention_and_identity_caps_bound_storage_without_affecting_requests() {
    let (_, collector) = fixture();
    collector.configure(true);
    for i in 0..(MAX_IDENTITIES + 20) {
        let request = collector.begin("group").unwrap();
        request.attempt("group", &format!("candidate-{i}"), "model", None);
        request.outcome(&format!("candidate-{i}"), None);
        request.finish(true, false);
    }
    let snapshot = collector.snapshot(14, None).unwrap();
    assert!(snapshot.capped);
    assert_eq!(snapshot.rows.len(), MAX_IDENTITIES);
    let today = clock().0;
    let mut state = collector.state.lock().unwrap();
    state.data.days.insert(today - 20, Day::default());
    state.data.trim(today);
    assert!(!state.data.days.contains_key(&(today - 20)));
    let bytes = serde_json::to_vec(&state.data).unwrap();
    assert!(bytes.len() < MAX_FILE_BYTES as usize);
    println!(
        "statistics: {} bounded identities, {} compact bytes",
        MAX_IDENTITIES,
        bytes.len()
    );
}

#[test]
fn malformed_oversized_and_unwritable_storage_isolated_from_collection() {
    let (root, collector) = fixture();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("counts.json"), b"not-json").unwrap();
    assert!(Collector::open(root.join("counts.json")).is_err());
    std::fs::File::create(root.join("counts.json"))
        .unwrap()
        .set_len(MAX_FILE_BYTES + 1)
        .unwrap();
    assert!(Collector::open(root.join("counts.json")).is_err());
    std::fs::remove_file(root.join("counts.json")).unwrap();
    std::fs::create_dir(root.join("counts.json")).unwrap();
    collector.configure(true);
    successful(&collector);
    assert!(collector.flush().is_err());
    let snapshot = collector.snapshot(1, None).unwrap();
    assert!(snapshot.storage_error);
    assert_eq!(snapshot.totals.completed, 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn writer_coalesces_events_and_does_not_write_again_while_idle() {
    let (root, collector) = fixture();
    collector.configure(true);
    let writer = tokio::spawn(writer_loop(collector.clone(), Duration::from_millis(40)));
    for _ in 0..1000 {
        successful(&collector);
    }
    assert_eq!(collector.writes.load(Ordering::Relaxed), 0);
    tokio::time::timeout(Duration::from_secs(5), async {
        while collector.writes.load(Ordering::Relaxed) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(collector.writes.load(Ordering::Relaxed), 1);
    for _ in 0..10 {
        collector.snapshot(14, None).unwrap();
    }
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert_eq!(collector.writes.load(Ordering::Relaxed), 1);
    writer.abort();
    println!("statistics: 1000 requests, 1 merged write, 0 additional idle writes");
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn writer_retries_failed_flush_without_new_requests_and_respects_pause() {
    for paused in [false, true] {
        let (root, collector) = fixture();
        std::fs::create_dir_all(root.join("counts.json")).unwrap();
        collector.configure(true);
        successful(&collector);
        let writer = tokio::spawn(writer_loop(collector.clone(), Duration::from_millis(25)));
        tokio::time::timeout(Duration::from_secs(2), async {
            while !collector.snapshot(1, None).unwrap().storage_error {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        if paused {
            collector.configure(false);
        }
        std::fs::remove_dir(root.join("counts.json")).unwrap();
        if paused {
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(collector.writes.load(Ordering::Relaxed), 0);
            collector.configure(true);
        }
        let saved = tokio::time::timeout(Duration::from_secs(2), async {
            while collector.writes.load(Ordering::Relaxed) == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await;
        writer.abort();
        let persisted = root.join("counts.json").is_file();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            saved.is_ok() && persisted,
            "dirty statistics must recover without a new request"
        );
        assert!(!collector.snapshot(1, None).unwrap().storage_error);
        assert_eq!(collector.snapshot(1, None).unwrap().totals.completed, 1);
    }
}

#[test]
fn maximum_fourteen_day_store_fits_the_file_cap_and_expired_groups_can_be_replaced() {
    let (_, collector) = fixture();
    collector.configure(true);
    for group in 0..MAX_GROUPS {
        let request = collector.begin(&format!("group-{group}")).unwrap();
        for model in 0..(MAX_IDENTITIES / MAX_GROUPS) {
            let candidate = format!("candidate-{model}");
            request.attempt(&format!("group-{group}"), &candidate, "model", None);
            request.outcome(&candidate, None);
        }
        request.finish(true, false);
    }
    assert!(!collector.begin("over-cap").unwrap().admitted);
    let today = clock().0;
    let mut state = collector.state.lock().unwrap();
    let template = state.data.days[&today].clone();
    for day in (today - 13)..=today {
        let mut bucket = template.clone();
        let hours = bucket.hours.values().next().unwrap().clone();
        bucket.hours = (0..24).map(|hour| (hour, hours.clone())).collect();
        bucket.rows.values_mut().for_each(|counts| {
            counts.usage = super::super::usage::TokenUsage {
                input_tokens: u64::MAX,
                output_tokens: u64::MAX,
                total_tokens: u64::MAX,
                reported_responses: u64::MAX,
            };
            counts.attempts = u64::MAX;
            counts.completed = u64::MAX;
            counts.protections = [u64::MAX; 5];
            counts.errors = [u64::MAX; 6];
        });
        for totals in bucket
            .hours
            .values_mut()
            .flat_map(|hours| hours.values_mut())
        {
            totals.usage = super::super::usage::TokenUsage {
                input_tokens: u64::MAX,
                output_tokens: u64::MAX,
                total_tokens: u64::MAX,
                reported_responses: u64::MAX,
            };
            totals.calls = u64::MAX;
            totals.completed = u64::MAX;
            totals.failed = u64::MAX;
            totals.cancelled = u64::MAX;
            totals.rescued = u64::MAX;
            totals.duration_ms = u64::MAX;
        }
        state.data.days.insert(day, bucket);
    }
    assert!(state.data.valid());
    let bytes = serde_json::to_vec(&state.data).unwrap();
    println!(
        "statistics: full 14-day store with 256 identities and 32 groups: {} bytes",
        bytes.len()
    );
    assert!(bytes.len() < MAX_FILE_BYTES as usize);
    state.data.days.clear();
    drop(state);
    assert!(collector.begin("replacement-group").is_some());
    assert_eq!(
        collector.state.lock().unwrap().data.groups.len(),
        MAX_GROUPS
    );
}

#[test]
fn delayed_response_consumption_does_not_inflate_upstream_duration() {
    let (_, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    request.attempt("group", "candidate", "model", None);
    request.outcome("candidate", None);
    request.response_ready();
    let elapsed = request.state.lock().unwrap().duration_ms;
    std::thread::sleep(Duration::from_millis(20));
    request.finish(true, false);
    assert_eq!(
        collector.snapshot(1, None).unwrap().totals.duration_ms,
        elapsed
    );
}

#[test]
fn final_body_error_replaces_success_once_and_cannot_change_a_finished_request() {
    let (_, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    request.attempt("group", "candidate", "model", None);
    request.outcome("candidate", None);
    let mut completion = Completion {
        request: request.clone(),
        response_ok: true,
        response_ready: true,
        done: false,
    };
    completion.error();
    completion.error();
    request.outcome("candidate", None);
    drop(completion);
    let snapshot = collector.snapshot(1, None).unwrap();
    assert_eq!((snapshot.totals.completed, snapshot.totals.failed), (0, 1));
    let counts = &snapshot.rows[0].counts;
    assert_eq!(
        (counts.attempts, counts.completed, counts.failed),
        (1, 0, 1)
    );
    assert_eq!(counts.errors, [0, 0, 0, 0, 0, 1]);
}

#[tokio::test]
async fn market_records_measured_stream_and_attributes_restoration_failure_without_double_counting()
{
    for restoration_failed in [false, true] {
        let (root, collector) = fixture();
        collector.configure(true);
        let request = collector.begin("group").unwrap();
        request.attempt("group", "candidate", "model", None);
        {
            let state = request.state.lock().unwrap();
            state.current.as_ref().unwrap().market.lock().unwrap().price =
                Some(super::super::pricing::Price {
                    input: 2.0,
                    output: 10.0,
                    provider: "fixture".into(),
                    fetched_at: 1,
                    ..Default::default()
                });
        }
        let frames = futures_util::stream::unfold(0, |index| async move {
            match index {
                0=>Some((Ok::<_,std::io::Error>(axum::body::Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}],\"usage\":{\"prompt_tokens\":10}}\n\n")),1)),
                1=>{tokio::time::sleep(Duration::from_millis(35)).await;Some((Ok(axum::body::Bytes::from_static(b"data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}],\"usage\":{\"completion_tokens\":20}}\n\n")),2))},
                _=>None,
            }
        });
        let response = axum::response::Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(frames))
            .unwrap();
        collector
            .observe(response, Some(&request), "candidate")
            .into_body()
            .collect()
            .await
            .unwrap();
        request.outcome("candidate", None);
        request.finish(!restoration_failed, false);
        request.finish(true, false);
        let snapshot = collector.snapshot(1, None).unwrap();
        assert_eq!(snapshot.market.events.len(), 1);
        let event = &snapshot.market.events[0];
        assert_eq!(event.cost_micros, Some(220));
        assert_eq!(
            event.outcome,
            if restoration_failed {
                market::Outcome::Failed
            } else {
                market::Outcome::Completed
            }
        );
        assert_eq!(
            snapshot.market.rows[0].metrics.speed_samples,
            u64::from(!restoration_failed)
        );
        if !restoration_failed {
            assert!(event.generation_ms.unwrap() >= 20);
        }
        collector.flush().unwrap();
        let reopened = Collector::open(root.join("counts.json")).unwrap();
        assert_eq!(reopened.snapshot(1, None).unwrap().market.events.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn market_keeps_authenticated_retry_in_one_event_and_never_assigns_stale_usage_to_new_response(
) {
    let (root, collector) = fixture();
    collector.configure(true);
    let request = collector.begin("group").unwrap();
    request.attempt("group", "candidate", "model", None);
    let first = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"input_tokens":999,"output_tokens":999}}),
        ),
        Some(&request),
        "candidate",
    );
    let second = collector.observe(
        crate::services::account_proxy::json_response(
            &serde_json::json!({"usage":{"input_tokens":10,"output_tokens":20}}),
        ),
        Some(&request),
        "candidate",
    );
    second.into_body().collect().await.unwrap();
    request.outcome("candidate", None);
    request.finish(true, false);
    assert!(collector
        .snapshot(1, None)
        .unwrap()
        .market
        .events
        .is_empty());
    first.into_body().collect().await.unwrap();
    let snapshot = collector.snapshot(1, None).unwrap();
    assert_eq!(snapshot.market.events.len(), 1);
    assert_eq!(snapshot.market.events[0].output_tokens, Some(20));
    assert_eq!(snapshot.market.events[0].cost_micros, None);
    collector.configure(false);
    let _ = std::fs::remove_dir_all(root);
}
