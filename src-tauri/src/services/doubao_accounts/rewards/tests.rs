use super::*;

fn task(checked: bool, state: u64, mode: u64) -> Value {
    json!({"campaign_code":"fixture_daily","campaign_group_code":V3_GROUP,"task_kind":"fixture_daily_kind","daily_checkin":{"checked_in_today":checked,"continuous_days":1,"total_days":1,"reward":{"reward_amount":200}},"reward_mode":mode,"task_status":state,"title":"A translated title"})
}

fn v3(row: Value) -> Value {
    json!({"code":0,"data":{"campaign_code":V3_GROUP,"tasks":[row]}})
}

#[test]
fn real_daily_shape_allows_only_the_daily_direct_reward() {
    let mut value = v3(task(false, 1, 2));
    value["data"]["tasks"].as_array_mut().unwrap().push(json!({"campaign_code":"fixture_other","task_status":2,"reward_mode":2,"task_id":"other-id","title":"每日签到"}));
    let daily = daily_task(&value, V3_GROUP).unwrap();
    assert_eq!(daily.status, "available");
    assert_eq!(
        daily.claim_body,
        Some(
            json!({"task_id":"","campaign_group_code":V3_GROUP,"campaign_item_code":"fixture_daily"})
        )
    );
}

#[test]
fn fresh_checked_or_rewarded_daily_does_not_produce_a_claim() {
    for row in [task(true, 1, 2), task(false, 3, 2)] {
        let daily = daily_task(&v3(row), V3_GROUP).unwrap();
        assert_eq!(daily.status, "claimed");
        assert!(daily.claim_body.is_none());
    }
}

#[test]
fn unknown_states_and_tasks_requiring_completion_are_not_claimed() {
    for (state, mode, expected) in [
        (0, 2, "unknown"),
        (99, 2, "unknown"),
        (1, 1, "unavailable"),
        (4, 1, "unavailable"),
    ] {
        let daily = daily_task(&v3(task(false, state, mode)), V3_GROUP).unwrap();
        assert_eq!(daily.status, expected);
        assert!(daily.claim_body.is_none());
    }
    let mut missing_id = task(false, 2, 1);
    assert!(daily_task(&v3(missing_id.clone()), V3_GROUP)
        .unwrap()
        .claim_body
        .is_none());
    missing_id["task_id"] = json!("fixture-current-task-id");
    let daily = daily_task(&v3(missing_id), V3_GROUP).unwrap();
    assert_eq!(daily.status, "available");
    assert_eq!(
        daily.claim_body,
        Some(json!({"task_id":"fixture-current-task-id"}))
    );
}

#[test]
fn exact_daily_metadata_and_single_unambiguous_item_are_required() {
    assert!(daily_task(&json!({"data":{"tasks":[{"title":"每日签到","task_status":2,"reward_mode":2,"campaign_code":"other"}]}}),V3_GROUP).is_none());
    let mut malformed = task(false, 1, 2);
    malformed["daily_checkin"]["checked_in_today"] = json!("false");
    assert!(daily_task(&v3(malformed), V3_GROUP).is_none());
    let mut other = task(false, 1, 2);
    other["campaign_code"] = json!("different-daily-item");
    assert!(daily_task(&json!({"data":{"tasks":[task(false,1,2),other]}}), V3_GROUP).is_none());
    assert!(daily_task(&json!({"data":{"tasks":[]}}), V3_GROUP).is_none());
}

#[test]
fn legacy_groups_preserve_the_actual_daily_claim_group_and_deduplicate_mirrors() {
    let mut row = task(false, 1, 2);
    row.as_object_mut().unwrap().remove("campaign_group_code");
    let value = json!({"data":{"campaign_group_code":"legacy-root","campaign_group":{"campaign_code":"daily-child","campaigns":[row.clone()],"campaign_groups":[{"campaign_code":"daily-child","campaigns":[row]}]}}});
    let daily = daily_task(&value, "legacy-root").unwrap();
    assert_eq!(
        daily.claim_body,
        Some(
            json!({"task_id":"","campaign_group_code":"daily-child","campaign_item_code":"fixture_daily"})
        )
    );
}

#[test]
fn business_failure_receipts_do_not_look_like_success() {
    assert!(receipt(&json!({"code":0,"data":{"task_status":3}})).is_ok());
    for code in [0, 4, 6] {
        assert!(receipt(&json!({"data":{"biz_error":{"code":code}}})).is_ok());
    }
    for value in [
        json!({"code":0}),
        json!({"data":{"success":false}}),
        json!({"data":{"biz_error":{"code":99,"message":"upstream private text"}}}),
        json!({"data":{"biz_error":{}}}),
        json!({"data":{"biz_error":"malformed error"}}),
    ] {
        assert_eq!(receipt(&value), Err("accountError.claim".into()));
    }
}

#[test]
fn accepted_business_ack_is_not_proof_of_a_claimed_reward() {
    for code in [0, 4, 6] {
        assert_eq!(
            receipt(&json!({"data":{"biz_error":{"code":code}}})),
            Ok(false)
        );
    }
    for value in [
        json!({"data":{"task_status":3}}),
        json!({"data":{"checked_in_today":true}}),
        json!({"data":{"daily_checkin":{"checked_in_today":true}}}),
    ] {
        assert_eq!(receipt(&value), Ok(true));
    }
    assert_eq!(
        receipt(&json!({"data":{"task_status":2,"checked_in_today":false}})),
        Ok(false)
    );
}

#[test]
fn post_claim_refresh_failure_preserves_only_confirmed_receipts() {
    assert_eq!(
        after_claim(V3_GROUP, false, Err("accountError.network".into())).status,
        "unknown"
    );
    assert_eq!(
        after_claim(V3_GROUP, true, Err("accountError.network".into())).status,
        "claimed"
    );
    let fresh = DailyReward {
        status: "unavailable".into(),
        scene: Some("fresh-scene".into()),
        next_claim_at: None,
    };
    assert_eq!(after_claim(V3_GROUP, false, Ok(fresh.clone())), fresh);
    let fresh = DailyReward {
        status: "claimed".into(),
        scene: Some(V3_GROUP.into()),
        next_claim_at: None,
    };
    assert_eq!(after_claim(V3_GROUP, false, Ok(fresh.clone())), fresh);
}

#[test]
fn confirmed_receipt_is_not_regressed_by_a_stale_available_snapshot() {
    let stale = DailyReward {
        status: "available".into(),
        scene: Some(V3_GROUP.into()),
        next_claim_at: None,
    };
    let result = after_claim(V3_GROUP, true, Ok(stale));
    assert_eq!(result.status, "claimed");
    assert_eq!(result.scene.as_deref(), Some(V3_GROUP));
}

#[tokio::test]
#[ignore = "Explicit read-only daily eligibility check; never claims, chats, or changes saved accounts"]
async fn saved_daily_status_read_only_probe() {
    assert_eq!(
        std::env::var("ECHOBIRD_DOUBAO_READ_ONLY_PROBE").as_deref(),
        Ok("1")
    );
    let key = dirs::home_dir()
        .unwrap()
        .join(".echobird/config/.encryption_key");
    assert_eq!(std::fs::metadata(key).unwrap().len(), 32);
    let edition = super::super::Edition::DoubaoWork;
    let store = super::super::load(&super::super::path(edition).unwrap()).unwrap();
    let (id, _) = store.iter().next().unwrap();
    let c = super::super::credential(&store, edition, id).unwrap();
    let reward = status(&c).await.expect("read-only daily status failed");
    println!(
        "daily_status={} scene_present={} exact_next_claim_time_present={}",
        reward.status,
        reward.scene.is_some(),
        reward.next_claim_at.is_some()
    );
}
