use super::*;
use serde_json::json;

fn overview() -> Value {
    json!({"code":0,"data":{
        "member_info":{"hasActiveSubscription":true,"hasEnterpriseSubscription":false},
        "current_subscription":{"display":{"short_name":"体验","product_name":"官方体验套餐"}},
        "window_limit_section":{"window_limit_groups":[{"feature_group_name":"专业任务","window_limits":[
            {"window_type":1,"total_amount":"735","used_amount":"0","used_percent":0,"end_time":0},
            {"window_type":2,"used_amount":"2","used_percent":0,"end_time":1793179688385_i64}
        ]}]},
        "incentive_quota_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":3,"total_amount":"200","used_amount":"0","used_percent":0,"end_time":1795189624888_i64}
        ]}]}
    }})
}

#[test]
fn real_overview_keeps_overlapping_windows_and_bonus_separate() {
    let quota = parse_overview(&overview());
    assert_eq!(quota.windows.len(), 2);
    let current = &quota.windows[0];
    assert_eq!(current.kind, "5h");
    assert_eq!(current.label.as_deref(), Some("专业任务"));
    assert_eq!(current.remaining, Some(735.0));
    assert_eq!(current.total, Some(735.0));
    assert_eq!(current.used_percent, Some(0.0));
    assert_eq!(current.reset_at, None);
    let week = &quota.windows[1];
    assert_eq!(week.kind, "7d");
    assert_eq!(week.remaining, None);
    assert_eq!(week.total, None);
    assert_eq!(week.used_percent, Some(0.0));
    assert_eq!(week.reset_at, Some(1793179688));
    assert_eq!(
        quota.bonus,
        Some(QuotaBucket {
            remaining: Some(200.0),
            total: Some(200.0),
            used_percent: Some(0.0),
            expires_at: Some(1795189624),
        })
    );
}

#[test]
fn missing_or_invalid_amounts_never_infer_totals_from_percentages() {
    let quota = parse_overview(&json!({
        "member_info":{"hasActiveSubscription":true},
        "window_limit_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":1,"used_amount":"2","used_percent":50},
            {"window_type":2,"used_amount":"NaN","total_amount":"100","used_percent":"Infinity"},
            {"window_type":4,"used_amount":"3","total_amount":"-1","used_percent":-1},
            {"window_type":99,"used_amount":"0","total_amount":"100"}
        ]}]}
    }));
    assert_eq!(quota.windows.len(), 3);
    assert!(quota
        .windows
        .iter()
        .all(|window| window.remaining.is_none()));
    assert_eq!(quota.windows[0].total, None);
    assert_eq!(quota.windows[0].used_percent, Some(50.0));
    assert_eq!(quota.windows[1].total, Some(100.0));
    assert_eq!(quota.windows[1].used_percent, None);
    assert_eq!(quota.windows[2].total, None);
    assert_eq!(quota.windows[2].used_percent, None);
}

#[test]
fn real_zero_amounts_and_overuse_are_distinct_from_unknown() {
    let quota = parse_overview(&json!({
        "quota_package_section":{"window_limit_groups":[{"feature_group_name":"额外额度","window_limits":[
            {"window_type":3,"used_amount":0,"total_amount":0,"used_percent":0},
            {"window_type":3,"used_amount":"101.25","total_amount":"100","used_percent":101.25}
        ]}]}
    }));
    assert_eq!(quota.windows.len(), 2);
    assert!(quota
        .windows
        .iter()
        .all(|window| window.kind == "subscription"));
    assert_eq!(quota.windows[0].remaining, Some(0.0));
    assert_eq!(quota.windows[0].total, Some(0.0));
    assert_eq!(quota.windows[1].remaining, Some(0.0));
    assert_eq!(quota.windows[1].total, Some(100.0));
    assert_eq!(quota.windows[1].used_percent, Some(100.0));
}

#[test]
fn exemption_only_changes_the_current_window_and_uses_its_deadline() {
    let mut value = overview();
    value["data"]["window_limit_section"]["window_limit_groups"][0]["window_limits"][0]
        ["exemption"] = json!({"active":true,"end_time":"1793179688385"});
    let quota = parse_overview(&value);
    assert!(quota.windows[0].unlimited);
    assert_eq!(quota.windows[0].reset_at, Some(1793179688));
    assert!(!quota.windows[1].unlimited);
    assert_eq!(quota.windows[1].total, None);
}

#[test]
fn enterprise_windows_do_not_mix_with_personal_or_reward_pools() {
    let mut value = overview();
    value["data"]["member_info"]["hasEnterpriseSubscription"] = json!(true);
    value["data"]["enterprise_window_limit_section"] = json!({"window_limit_groups":[
        {"feature_group_name":"团队专业","window_limits":[{"window_type":1,"total_amount":"500","used_amount":"50"}]},
        {"feature_group_name":"团队其他","window_limits":[{"window_type":1,"total_amount":"300","used_amount":"25"}]}
    ]});
    let quota = parse_overview(&value);
    assert_eq!(quota.windows.len(), 2);
    assert_eq!(quota.windows[0].label.as_deref(), Some("团队专业"));
    assert_eq!(quota.windows[0].remaining, Some(450.0));
    assert_eq!(quota.windows[1].label.as_deref(), Some("团队其他"));
    assert_eq!(quota.windows[1].remaining, Some(275.0));
    assert!(quota.bonus.is_none());
    value["data"]["enterprise_window_limit_section"]["usage_exhausted"] = json!(true);
    assert_eq!(parse_overview(&value), parse_overview(&overview()));
}

#[test]
fn unknown_payload_and_legacy_serialization_keep_missing_state_unknown() {
    assert_eq!(parse_overview(&Value::Null), Quota::default());
    assert_eq!(parse_overview(&json!({})), Quota::default());
    assert_eq!(
        serde_json::from_value::<Quota>(json!({})).unwrap(),
        Quota::default()
    );
    assert_eq!(
        serde_json::from_value::<DailyReward>(json!({})).unwrap(),
        DailyReward::default()
    );
    let serialized = serde_json::to_value(parse_overview(&overview())).unwrap();
    assert_eq!(serialized["windows"][0]["usedPercent"], 0.0);
    assert!(serialized["windows"][0]["resetAt"].is_null());
    assert_eq!(serialized["bonus"]["expiresAt"], 1795189624);
    assert_eq!(
        serde_json::to_value(DailyReward::default()).unwrap()["status"],
        "unknown"
    );
}

#[test]
fn plan_uses_official_display_names_and_never_infers_a_free_tier() {
    assert_eq!(parse_plan(&overview()).as_deref(), Some("体验"));
    assert_eq!(
        parse_plan(&json!({"data":{"membership_display_name":"Plus"}})).as_deref(),
        Some("Plus")
    );
    assert_eq!(
        parse_plan(&json!({"current_subscription":{"display":{"product_name":"标准"}}})).as_deref(),
        Some("标准")
    );
    assert_eq!(
        parse_plan(&json!({"member_info":{"hasActiveSubscription":false}})),
        None
    );
    assert_eq!(parse_plan(&json!({"membership_display_name":"  "})), None);
    assert_eq!(
        parse_plan(&json!({"incentive_quota_entitlements":[{"display":{"title":"签到奖励"}}]})),
        None
    );
}
