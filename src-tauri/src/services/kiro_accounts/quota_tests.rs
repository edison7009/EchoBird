use super::*;
use serde_json::json;

fn account() -> Account {
    Account {
        id: "fixture".into(),
        edition: super::super::Edition::Cli,
        name: "Fixture".into(),
        plan: None,
        active: false,
        quota: vec![],
    }
}

#[test]
fn quota_keeps_plan_trial_and_bonus_counters_and_their_actual_dates() {
    let mut account = account();
    update(
        &mut account,
        &json!({"subscriptionInfo":{"subscriptionTitle":"KIRO PRO+"},"nextDateReset":1793491200.0,
        "usageBreakdownList":[{"resourceType":"TOKEN","usageLimit":99,"currentUsage":0},
        {"resourceType":"CREDIT","usageLimitWithPrecision":1000.25,"currentUsageWithPrecision":1.5,
        "freeTrialInfo":{"freeTrialStatus":"ACTIVE","usageLimit":500,"currentUsage":200,"freeTrialExpiry":1792406534},
        "bonuses":[{"status":"ACTIVE","usageLimit":125,"currentUsage":5,"expiresAt":1797000000},{"status":"EXPIRED","usageLimit":100,"currentUsage":0}]}]}),
    );
    assert_eq!(account.plan.as_deref(), Some("Pro+"));
    assert_eq!(account.quota.len(), 3);
    assert_eq!(account.quota[0].remaining, Some(998.75));
    assert_eq!(account.quota[0].reset_at, Some(1793491200));
    assert_eq!(account.quota[1].remaining, Some(300.));
    assert_eq!(account.quota[1].reset_at, Some(1792406534));
    assert_eq!(account.quota[2].remaining, Some(120.));
    assert_eq!(account.quota[2].reset_at, Some(1797000000));
}

#[test]
fn missing_and_invalid_counters_are_unknown_and_known_zero_is_retained() {
    let mut account = account();
    update(
        &mut account,
        &json!({"subscriptionInfo":{"subscriptionTitle":"KIRO FREE"},"usageBreakdownList":[
            {"resourceType":"CREDIT","usageLimit":0,"currentUsage":0},
            {"resourceType":"CREDIT","usageLimit":100,"currentUsage":null},
            {"resourceType":"CREDIT","usageLimit":-1,"currentUsage":0},
            {"resourceType":"CREDIT","usageLimit":10,"currentUsage":11,
                "freeTrialInfo":{"freeTrialStatus":"EXPIRED","usageLimit":500,"currentUsage":0}}
        ]}),
    );
    assert_eq!(account.plan.as_deref(), Some("Free"));
    assert_eq!(account.quota.len(), 4);
    assert_eq!(account.quota[0].remaining, Some(0.));
    assert_eq!(account.quota[0].total, Some(0.));
    assert_eq!(account.quota[1].remaining, None);
    assert_eq!(account.quota[2].total, None);
    assert_eq!(account.quota[3].remaining, Some(0.));
    assert!(account.quota.iter().all(|quota| quota.reset_at.is_none()));
    update(&mut account, &json!({}));
    assert!(account.quota.is_empty());
    assert!(account.plan.is_none());
}

#[test]
fn a_single_bucket_without_a_resource_type_is_the_credit_pool() {
    let mut account = account();
    update(
        &mut account,
        &json!({"usageBreakdownList":[{"usageLimitWithPrecision":10.0,"currentUsageWithPrecision":4.0}]}),
    );
    assert_eq!(account.quota.len(), 1);
    assert_eq!(account.quota[0].remaining, Some(6.));
    update(
        &mut account,
        &json!({"usageBreakdownList":[
            {"usageLimit":10,"currentUsage":4},
            {"resourceType":"TOKEN","usageLimit":99,"currentUsage":0}
        ]}),
    );
    assert!(account.quota.is_empty());
}
