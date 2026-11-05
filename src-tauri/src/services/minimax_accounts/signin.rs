use super::{Credentials, DailyReward, RewardStatus};
use serde_json::{json, Value};

pub(super) struct Eligibility {
    pub reward: DailyReward,
    pub day_no: Option<u8>,
}

fn points(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|points| points.is_finite() && points >= 0.0)
}

fn panel(value: &Value, now: i64) -> Result<Eligibility, String> {
    if value["scene"].as_u64().is_none_or(|scene| scene > 4) {
        return Err("accountError.format".into());
    }
    let days = value["days"]
        .as_array()
        .filter(|days| days.len() == 7)
        .ok_or("accountError.format")?;
    let mut seen = [false; 7];
    let mut claimable = None;
    let mut today = false;
    let mut claimed_today = false;
    for day in days {
        let number = day["day_no"]
            .as_u64()
            .filter(|number| (1..=7).contains(number))
            .ok_or("accountError.format")?;
        let status = day["status"]
            .as_u64()
            .filter(|status| (1..=4).contains(status))
            .ok_or("accountError.format")?;
        let is_today = day["is_today"].as_bool().ok_or("accountError.format")?;
        if seen[number as usize - 1]
            || !points(&day["points"])
            || day.get("bonus_points").is_some_and(|bonus| !points(bonus))
            || status == 2 && claimable.is_some()
            || is_today && today
        {
            return Err("accountError.format".into());
        }
        seen[number as usize - 1] = true;
        if status == 2 {
            claimable = Some(number as u8);
        }
        today |= is_today;
        claimed_today |= is_today && status == 3;
    }
    Ok(Eligibility {
        reward: DailyReward {
            status: if claimed_today {
                RewardStatus::Claimed
            } else if claimable.is_some() {
                RewardStatus::Claimable
            } else {
                RewardStatus::Unavailable
            },
            checked_at: now,
        },
        day_no: (!claimed_today).then_some(claimable).flatten(),
    })
}

async fn response(request: reqwest::RequestBuilder) -> Result<Value, String> {
    let response = request.send().await.map_err(|_| "accountError.network")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("accountError.loginRequired".into());
    }
    if !response.status().is_success() {
        return Err("accountError.network".into());
    }
    let value: Value = response.json().await.map_err(|_| "accountError.format")?;
    if value["base_resp"]["status_code"] == 1_000_048 || value["statusInfo"]["code"] == 1_000_048 {
        return Err("accountError.loginRequired".into());
    }
    match value["base_resp"]["status_code"].as_i64() {
        Some(0) => value
            .get("data")
            .filter(|data| data.is_object())
            .cloned()
            .ok_or_else(|| "accountError.format".into()),
        Some(_) => Err("accountError.failed".into()),
        None => Err("accountError.format".into()),
    }
}

pub(super) async fn status(
    client: &reqwest::Client,
    host: &str,
    credential: &Credentials,
    version: &str,
) -> Result<Eligibility, String> {
    let value = response(super::quota::signed_desktop(
        client,
        host,
        "/minimax-cloud/api/v1/signin/status",
        credential,
        None,
        version,
    )?)
    .await?;
    panel(&value, chrono::Utc::now().timestamp())
}

fn receipt(value: &Value, day_no: u8, now: i64) -> Result<DailyReward, String> {
    if value["claim_id"].as_str().is_none_or(|id| id.is_empty())
        || !matches!(value["claim_result"].as_u64(), Some(1 | 2))
        || value["day_no"].as_u64() != Some(u64::from(day_no))
        || !(1..=7).contains(&day_no)
        || !points(&value["points"])
        || !value["expire_at_ms"]
            .as_f64()
            .is_some_and(|expiry| expiry.is_finite())
    {
        return Err("accountError.format".into());
    }
    panel(&value["panel"], now)?;
    Ok(DailyReward {
        status: RewardStatus::Claimed,
        checked_at: now,
    })
}

pub(super) async fn claim(
    client: &reqwest::Client,
    host: &str,
    credential: &Credentials,
    version: &str,
    day_no: u8,
) -> Result<DailyReward, String> {
    if !(1..=7).contains(&day_no) {
        return Err("accountError.format".into());
    }
    let value = response(super::quota::signed_desktop(
        client,
        host,
        "/minimax-cloud/api/v1/signin/claim",
        credential,
        Some(json!({})),
        version,
    )?)
    .await?;
    receipt(&value, day_no, chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::minimax_accounts::{
        test_support::{json as http_json, MockServer},
        Region,
    };
    use std::sync::{Arc, Mutex};

    fn signin_panel(day: u8, status: u8) -> Value {
        json!({"scene":2,"days":(1..=7).map(|number| json!({
            "day_no":number,
            "points":if number==4 { 1_234 } else if number==7 { 2_345 } else { 321 },
            "bonus_points":if number==4 { 234 } else { 0 },
            "status":if number==day { status } else { 1 },
            "is_today":number==day,
        })).collect::<Vec<_>>()})
    }

    fn claim_data(result: u8) -> Value {
        json!({"claim_id":"server-receipt","claim_result":result,"day_no":4,
            "points":1_234,"expire_at_ms":1_900_000_000_000i64,"panel":signin_panel(4,3)})
    }

    fn credential(region: Region) -> Credentials {
        Credentials {
            region,
            access_token: "fixture-access".into(),
            refresh_token: "fixture-refresh".into(),
            expires_at: 1_900_000_000,
            user_id: "fixture-user".into(),
            email: "fixture@example.invalid".into(),
            name: None,
        }
    }

    #[test]
    fn eligibility_uses_server_days_without_assuming_rewards_or_reset_times() {
        for day in [4, 7] {
            let result = panel(&signin_panel(day, 2), 100).unwrap();
            assert!(result.reward.status == RewardStatus::Claimable);
            assert_eq!(result.day_no, Some(day));
            assert_eq!(result.reward.checked_at, 100);
        }
        let mut claimed = signin_panel(4, 3);
        claimed["days"][4]["status"] = json!(2);
        let result = panel(&claimed, 100).unwrap();
        assert!(result.reward.status == RewardStatus::Claimed);
        assert_eq!(result.day_no, None);
        let result = panel(&signin_panel(0, 1), 100).unwrap();
        assert!(result.reward.status == RewardStatus::Unavailable);
        assert_eq!(result.day_no, None);
        let mut available = signin_panel(4, 2);
        available["days"][3]["is_today"] = json!(false);
        assert_eq!(panel(&available, 100).unwrap().day_no, Some(4));
    }

    #[test]
    fn ambiguous_or_malformed_panels_do_not_create_eligibility() {
        for (path, value) in [
            ("/scene", json!(5)),
            ("/days/0/day_no", json!(4)),
            ("/days/0/status", json!(99)),
            ("/days/0/status", json!(2)),
            ("/days/0/is_today", json!(true)),
            ("/days/0/is_today", json!("true")),
            ("/days/0/points", json!(-1)),
            ("/days/0/points", json!("400")),
            ("/days/0/bonus_points", json!(null)),
        ] {
            let mut value_panel = signin_panel(4, 2);
            *value_panel.pointer_mut(path).unwrap() = value;
            assert_eq!(
                panel(&value_panel, 100).err().unwrap(),
                "accountError.format"
            );
        }
        let mut short = signin_panel(4, 2);
        short["days"].as_array_mut().unwrap().pop();
        assert!(panel(&short, 100).is_err());
        assert!(panel(&json!({}), 100).is_err());
    }

    #[test]
    fn only_a_complete_matching_claim_or_idempotent_receipt_confirms_success() {
        for result in [1, 2] {
            let reward = receipt(&claim_data(result), 4, 100).unwrap();
            assert!(reward.status == RewardStatus::Claimed);
            assert_eq!(reward.checked_at, 100);
        }
        for (path, value) in [
            ("/claim_id", json!("")),
            ("/claim_result", json!(0)),
            ("/claim_result", json!("1")),
            ("/day_no", json!(7)),
            ("/points", json!(-1)),
            ("/expire_at_ms", json!(null)),
            ("/panel", json!({})),
        ] {
            let mut data = claim_data(1);
            *data.pointer_mut(path).unwrap() = value;
            assert_eq!(receipt(&data, 4, 100).unwrap_err(), "accountError.format");
        }
        assert!(receipt(&claim_data(1), 0, 100).is_err());
        assert!(receipt(&json!({"claim_result":1}), 4, 100).is_err());
        let mut data = claim_data(1);
        data["panel"] = signin_panel(0, 1);
        assert!(receipt(&data, 4, 100).unwrap().status == RewardStatus::Claimed);
    }

    #[tokio::test]
    async fn region_device_attribution_and_signed_empty_post_have_positive_controls() {
        for region in [Region::Cn, Region::Global] {
            for result in [1, 2] {
                let requests = Arc::new(Mutex::new(Vec::new()));
                let recorded = requests.clone();
                let server = MockServer::start(move |request| {
                    let recorded = recorded.clone();
                    async move {
                        assert_eq!(request.headers["authorization"], "Bearer fixture-access");
                        assert_eq!(request.headers["content-type"], "application/json");
                        for name in ["yy", "x-signature"] {
                            assert_eq!(request.headers[name].to_str().unwrap().len(), 32);
                        }
                        let query: std::collections::HashMap<_, _> =
                            url::form_urlencoded::parse(request.query.as_bytes())
                                .into_owned()
                                .collect();
                        assert_eq!(query["device_platform"], "web");
                        assert_eq!(query["is_desktop"], "1");
                        assert_eq!(query["desktop_version"], "3.1.1");
                        assert_eq!(query["client"], "mcode");
                        assert_eq!(query["app_id"], "3001");
                        assert_eq!(query["user_id"], "fixture-user");
                        assert_eq!(
                            query["lang"],
                            if region == Region::Cn { "zh" } else { "en" }
                        );
                        recorded.lock().unwrap().push(request.path.clone());
                        let data = if request.path.ends_with("/status") {
                            assert!(request.body.is_empty());
                            signin_panel(4, 2)
                        } else {
                            assert!(request.path.ends_with("/claim"));
                            assert_eq!(request.body, "{}");
                            claim_data(result)
                        };
                        http_json(200, json!({"base_resp":{"status_code":0},"data":data}))
                    }
                })
                .await;
                let client = reqwest::Client::new();
                let eligible = status(&client, &server.url, &credential(region), "3.1.1")
                    .await
                    .ok()
                    .unwrap();
                assert_eq!(eligible.day_no, Some(4));
                let reward = claim(&client, &server.url, &credential(region), "3.1.1", 4)
                    .await
                    .unwrap();
                assert!(reward.status == RewardStatus::Claimed);
                assert_eq!(
                    requests.lock().unwrap().as_slice(),
                    &[
                        "/minimax-cloud/api/v1/signin/status",
                        "/minimax-cloud/api/v1/signin/claim",
                    ]
                );
            }
        }
    }

    #[tokio::test]
    async fn rejected_claims_never_retry_or_accept_an_unconfirmed_success_envelope() {
        for (http_status, body, expected) in [
            (401, json!({}), "accountError.loginRequired"),
            (403, json!({}), "accountError.loginRequired"),
            (503, json!({}), "accountError.network"),
            (
                200,
                json!({"base_resp":{"status_code":1_000_048}}),
                "accountError.loginRequired",
            ),
            (
                200,
                json!({"statusInfo":{"code":1_000_048}}),
                "accountError.loginRequired",
            ),
            (
                200,
                json!({"base_resp":{"status_code":7}}),
                "accountError.failed",
            ),
            (200, json!({}), "accountError.format"),
            (
                200,
                json!({"base_resp":{"status_code":0},"data":{}}),
                "accountError.format",
            ),
        ] {
            let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let recorded = count.clone();
            let server = MockServer::start(move |_| {
                let recorded = recorded.clone();
                let body = body.clone();
                async move {
                    recorded.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    http_json(http_status, body)
                }
            })
            .await;
            let client = reqwest::Client::new();
            assert_eq!(
                status(&client, &server.url, &credential(Region::Cn), "3.1.1")
                    .await
                    .err()
                    .unwrap(),
                expected
            );
            assert_eq!(
                claim(&client, &server.url, &credential(Region::Cn), "3.1.1", 4)
                    .await
                    .unwrap_err(),
                expected
            );
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
            assert!(
                claim(&client, &server.url, &credential(Region::Cn), "3.1.1", 8)
                    .await
                    .is_err()
            );
            assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 2);
        }
    }
}
