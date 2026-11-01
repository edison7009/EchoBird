use super::*;

fn account() -> Saved {
    serde_json::from_value(json!({
        "provider":"bigmodel", "user_id":"fixture", "email":"fixture@example.test",
        "access_token":"fixture-access", "jwt_token":"fixture-jwt", "user_info":{}
    }))
    .unwrap()
}

#[tokio::test]
async fn http_quota_failures_preserve_status_without_expiring_credentials() {
    use axum::{routing::any, Router};

    for (status, expected) in [
        (200, None),
        (401, Some("accountError.auth|HTTP 401")),
        (403, Some("accountError.auth|HTTP 403")),
        (429, Some("accountError.quota|HTTP 429")),
        (500, Some("accountError.quota|HTTP 500")),
        (503, Some("accountError.quota|HTTP 503")),
    ] {
        let status = reqwest::StatusCode::from_u16(status).unwrap();
        let body = if status.is_success() {
            r#"{"code":0,"data":{"balance":20}}"#
        } else {
            "not json"
        };
        let app = Router::new().fallback(any(move || async move { (status, body) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = super::super::http().unwrap();
        let response = client.get(&url).send().await.unwrap();
        let envelope = super::super::envelope(response).await;
        let quota = get_data(&client, &url, "Bearer fixture-access").await;
        server.abort();
        for result in [envelope, quota] {
            if let Some(expected) = expected {
                assert_eq!(result.unwrap_err(), expected);
            } else {
                assert_eq!(result.unwrap(), json!({"balance":20}));
            }
        }
    }
}

#[test]
fn compact_tiers_also_apply_to_cached_accounts_without_refresh_or_writes() {
    for (name, expected) in [
        ("ZCode Trust Build", "Trial"),
        ("ZCode Start Plan", "Trial"),
        ("体验套餐", "Trial"),
        ("體驗套餐", "Trial"),
        ("GLM Coding Lite", "Lite"),
        ("GLM Coding Pro", "Pro"),
        ("GLM Coding Max", "Max"),
        ("GLM Coding Team Pro", "Team"),
        ("团队套餐", "Team"),
        ("團隊套餐", "Team"),
        ("Unknown Plan", "Unknown Plan"),
    ] {
        for provider in ["bigmodel", "zai"] {
            let mut saved = account();
            saved.provider = provider.into();
            saved.plan = Some(name.into());
            let row = super::super::row(&saved, None);
            assert_eq!(row.plan.as_deref(), Some(expected));
            assert_eq!(saved.plan.as_deref(), Some(name));
            assert_eq!(row.remaining_percent, None);
        }
    }
    assert_eq!(super::super::row(&account(), None).plan, None);
}

#[test]
fn confirmed_empty_plan_is_distinct_from_unknown_and_active_trial() {
    for provider in ["bigmodel", "zai"] {
        let mut saved = account();
        saved.provider = provider.into();
        assert_eq!(super::super::row(&saved, None).plan, None);
        for data in [json!({}), json!({"plans":null}), json!({"plans":{}})] {
            super::super::apply_balance(&mut saved, &data);
            assert_eq!(saved.plan, None);
        }
        super::super::apply_balance(&mut saved, &json!({"plans":[],"balances":[]}));
        assert_eq!(saved.plan.as_deref(), Some("No Plan"));
        assert_eq!(
            super::super::row(&saved, None).plan.as_deref(),
            Some("No Plan")
        );
        assert_eq!(saved.remaining_percent, None);
        assert_eq!(saved.subscription_end_at, None);
        super::super::apply_balance(
            &mut saved,
            &json!({"plans":[{"status":"active","name":"ZCode V3 Start Plan"}],"balances":[]}),
        );
        assert_eq!(saved.plan.as_deref(), Some("Trial"));
        assert_eq!(saved.remaining_percent, None);
    }
}

#[test]
fn paid_tiers_and_billing_cycles_keep_subscription_and_quota_deadlines_separate() {
    // A 5-hour window resets within five hours of the reply.
    let session_reset = chrono::Utc::now().timestamp() + 3600;
    for provider in ["bigmodel", "zai"] {
        for tier in ["Lite", "Pro", "Max"] {
            for (cycle, end) in [
                ("month", "2030-02-01T08:00:00+08:00"),
                ("quarter", "2030-04-01T00:00:00Z"),
                ("year", "2031-01-01T00:00:00Z"),
            ] {
                let mut saved = account();
                saved.provider = provider.into();
                let list = json!([{"productId":format!("coding-{tier}"), "productName":format!("GLM Coding {tier}"), "status":"VALID", "inCurrentPeriod":true, "billingCycle":cycle, "autoRenew":false, "nextRenewTime":end}]);
                let subscription = current_subscription(&list).unwrap().unwrap();
                apply_paid(&mut saved, subscription, Some(json!({"limits":[
                    {"type":"CREDIT_LIMIT","unit":6,"percentage":75,"nextResetTime":1894060800000_i64},
                    {"type":"CREDIT_LIMIT","unit":3,"percentage":"25.5","nextResetTime":session_reset * 1000},
                    {"type":"TIME_LIMIT","percentage":90}
                ]}))).unwrap();
                assert_eq!(saved.plan.as_deref(), Some(tier));
                assert_eq!(saved.subscription_end_at, date_seconds(end));
                assert_eq!(saved.quota_windows.len(), 2);
                assert_eq!(saved.quota_windows[0].remaining_percent, 74.5);
                assert_eq!(saved.quota_windows[1].remaining_percent, 25.0);
                assert_eq!(saved.reset_at, Some(session_reset));
                assert_ne!(saved.reset_at, saved.subscription_end_at);
            }
        }
    }
    assert_eq!(
        business_origin("bigmodel").unwrap(),
        "https://open.bigmodel.cn"
    );
    assert_eq!(business_origin("zai").unwrap(), "https://api.z.ai");
    assert!(business_origin("unknown").is_err());
}

#[test]
fn auto_renewal_is_not_a_final_expiry_and_local_dates_follow_native_timezone() {
    let local = Local
        .with_ymd_and_hms(2030, 12, 31, 23, 59, 59)
        .single()
        .unwrap()
        .timestamp();
    for auto_renew in [json!(true), json!(1)] {
        assert_eq!(
            subscription_end(
                &json!({"autoRenew":auto_renew,"nextRenewTime":"2030-02-01T00:00:00Z","valid":"2030-01-01 00:00:00 至 2030-12-31 23:59:59"})
            ),
            Some(local)
        );
        assert_eq!(
            subscription_end(
                &json!({"autoRenew":auto_renew,"nextRenewTime":"2030-02-01T00:00:00Z"})
            ),
            None
        );
    }
    assert_eq!(date_seconds("2030-12-31 23:59:59.000"), Some(local));
    assert_eq!(
        subscription_end(&json!({"valid":"2030-01-01 ~ 2030-12-31"})),
        date_seconds("2030-12-31T00:00:00Z")
    );
    assert_eq!(date_seconds("invalid"), None);
}

#[test]
fn missing_malformed_and_expired_subscriptions_do_not_invent_entitlements() {
    let active = json!({"productId":"coding-pro","status":"VALID","inCurrentPeriod":true});
    assert!(current_subscription(
        &json!([{"unrelated":true},{"productId":"coding-broken"},active])
    )
    .unwrap()
    .is_some());
    assert!(current_subscription(&json!([{"productId":"coding-broken"}])).is_err());
    assert!(current_subscription(&json!({})).is_err());
    for list in [
        json!([]),
        json!([{"productId":"other","status":"VALID","inCurrentPeriod":true}]),
        json!([{"productId":"coding-pro","status":"VALID","inCurrentPeriod":false}]),
        json!([{"productId":"coding-pro","status":"EXPIRED","inCurrentPeriod":true}]),
        json!([{"productId":"coding-pro","status":"VALID","inCurrentPeriod":true,"nextRenewTime":"2020-01-01"}]),
    ] {
        assert!(current_subscription(&list).unwrap().is_none());
    }
    let mut saved = account();
    assert!(saved.quota_windows.is_empty());
    assert_eq!(saved.subscription_end_at, None);
    for quota in [
        None,
        Some(json!({"limits":[]})),
        Some(json!({"limits":[{"type":"CREDIT_LIMIT","unit":3}]})),
    ] {
        apply_paid(&mut saved, &active, quota).unwrap();
        assert_eq!(saved.remaining_percent, None);
        assert!(saved.quota_windows.is_empty());
    }
    assert!(apply_paid(&mut saved, &active, Some(json!({}))).is_err());
    apply_paid(
        &mut saved,
        &active,
        Some(json!({"limits":[{"type":"TOKENS_LIMIT","unit":3,"percentage":100}]})),
    )
    .unwrap();
    assert_eq!(saved.remaining_percent, Some(0.0));
}

#[test]
fn trial_expiry_is_distinct_from_balance_reset_and_clears_paid_windows() {
    let mut saved = account();
    saved.quota_windows.push(QuotaWindow {
        remaining_percent: 50.0,
        reset_at: None,
    });
    super::super::apply_balance(
        &mut saved,
        &json!({
            "plans":[{"user_plan_id":"trial","status":"active","name":"Start Plan","ends_at":1893888000}],
            "balances":[{"user_plan_id":"trial","total_units":100,"remaining_units":20,"expires_at":1893474000,"period_end":1894000000}]
        }),
    );
    assert_eq!(saved.plan.as_deref(), Some("Trial"));
    assert_eq!(saved.subscription_end_at, Some(1_893_888_000));
    assert_eq!(saved.remaining_percent, Some(20.0));
    assert_eq!(saved.reset_at, Some(1_893_474_000));
    assert!(saved.quota_windows.is_empty());
    super::super::apply_balance(&mut saved, &json!({"plans":[],"balances":[]}));
    assert_eq!(saved.subscription_end_at, None);
    assert_eq!(saved.remaining_percent, None);
}

#[tokio::test]
async fn existing_personal_key_queries_are_read_only_and_never_use_team_keys() {
    use axum::{extract::Request, routing::any, Json, Router};
    use std::sync::{Arc, Mutex};
    for (provider, keys) in [
        (
            "bigmodel",
            json!([{"name":"zcode-api-key","apiKey":"public"}]),
        ),
        ("zai", json!([{"name":"zcode-api-key","apiKey":"public"}])),
        ("zai", json!([])),
    ] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured = calls.clone();
        let has_key = !keys.as_array().unwrap().is_empty();
        let app = Router::new().fallback(any(move |request: Request| {
            let calls = captured.clone();
            let keys = keys.clone();
            async move {
                calls.lock().unwrap().push((request.method().to_string(), request.uri().path().to_string(), request.headers()["authorization"].to_str().unwrap().to_string()));
                let data = match request.uri().path() {
                    "/api/biz/customer/getCustomerInfo" => json!({"organizations":[
                        {"organizationId":"team","projects":[{"projectId":"team","projectType":2}]},
                        {"organizationId":"personal","organizationName":"默认机构","projects":[{"projectId":"team","projectType":"2"},{"projectId":"personal","projectName":"默认项目"}]}
                    ]}),
                    "/api/biz/v1/organization/personal/projects/personal/api_keys" => keys,
                    "/api/biz/v1/organization/personal/projects/personal/api_keys/copy/public" => json!({"secretKey":"secret"}),
                    _ => panic!("unexpected key request"),
                };
                Json(json!({"code":200,"success":true,"data":data}))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let authorization = if provider == "zai" {
            "Bearer fixture-access"
        } else {
            "fixture-access"
        };
        let result = existing_usage_key(
            &super::super::http().unwrap(),
            &origin,
            authorization,
            provider,
        )
        .await;
        server.abort();
        assert_eq!(result.unwrap(), has_key.then(|| "public.secret".into()));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), if has_key { 3 } else { 2 });
        assert!(calls
            .iter()
            .all(|(method, _, auth)| method == "GET" && auth == authorization));
    }
    assert_eq!(
        personal_project(
            &json!({"organizations":[{"organizationId":"team","projects":[{"projectId":"team","projectType":2}]}]})
        ),
        None
    );
}
