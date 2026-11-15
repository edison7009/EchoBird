use super::{client, native, response, Account, Edition};
use serde_json::{json, Value};

async fn post(
    base: &str,
    edition: Edition,
    path: &str,
    token: &str,
    device: &str,
    body: Value,
) -> Result<Value, String> {
    let mut request = client()?
        .post(format!("{base}{path}"))
        .header("Authorization", format!("Cloud-IDE-JWT {token}"))
        .header("Origin", edition.origin())
        .header("Referer", format!("{}/", edition.origin()))
        .json(&body);
    if !device.is_empty() {
        request = request.header("x-device-id", device);
    }
    let result = request.send().await.map_err(|_| "accountError.network")?;
    if !result.status().is_success() {
        return response(result).await;
    }
    let payload: Value = result
        .json()
        .await
        .map_err(|_| "accountError.authResponse")?;
    if path.ends_with("/claim") && payload["code"].as_i64() == Some(9095) {
        return Ok(payload);
    }
    super::response_payload(payload)
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}

fn pack_quota(pack: &Value) -> &Value {
    let base = &pack["entitlement_base_info"];
    let preferred = if base["product_type"].as_u64() == Some(2) {
        base.pointer("/product_extra/package_extra/quota")
    } else {
        base.pointer("/product_extra/subscription_extra/quota")
    };
    preferred
        .or_else(|| base.get("quota"))
        .or_else(|| base.pointer("/product_extra/package_extra/quota"))
        .unwrap_or(&base["quota"])
}

fn pack_expiry(pack: &Value) -> Option<i64> {
    [
        &pack["expire_time"],
        &pack["entitlement_base_info"]["end_time"],
        &pack["yearly_expire_time"],
    ]
    .into_iter()
    .find_map(native::timestamp)
}

fn visible(pack: &Value, now: i64) -> bool {
    pack["is_hide"] != true
        && pack_expiry(pack).is_none_or(|expiry| expiry > now)
        && native::timestamp(&pack["entitlement_base_info"]["start_time"])
            .is_none_or(|start| start <= now)
}

fn consumed(pack: &Value, key: &str) -> Option<f64> {
    match pack.get("usage") {
        None | Some(Value::Null) => Some(0.0),
        Some(Value::Object(usage)) => match usage.get(key) {
            None | Some(Value::Null) => Some(0.0),
            Some(value) => number(value).filter(|value| *value >= 0.0),
        },
        Some(_) => None,
    }
}

#[derive(Clone, Copy)]
struct CreditTotals {
    remaining: f64,
    total: f64,
    expires_at: Option<i64>,
}

fn credit_totals(
    payload: &Value,
    endpoint: u64,
    base_group: Option<bool>,
    now: i64,
) -> Option<CreditTotals> {
    let packs = payload["user_entitlement_pack_list"].as_array()?;
    let mut total = CreditTotals {
        remaining: 0.0,
        total: 0.0,
        expires_at: None,
    };
    let mut present = false;
    let mut common_expiry = true;
    for pack in packs {
        let base = &pack["entitlement_base_info"];
        if !visible(pack, now)
            || number(&base["currency"]).is_some_and(|currency| ![0.0, 1.0].contains(&currency))
        {
            continue;
        }
        let Some(target) = number(&base["available_endpoint"]) else {
            continue;
        };
        if target != endpoint as f64 {
            continue;
        }
        if let Some(is_base) = base_group {
            let group = pack["group_type"].as_u64();
            if if is_base {
                group != Some(0)
            } else {
                !group.is_some_and(|group| (1..=5).contains(&group))
            } {
                continue;
            }
        }
        let quota = pack_quota(pack);
        if quota["credits_limit"].is_null() && base["product_type"].as_u64() == Some(0) {
            continue;
        }
        let limit = number(&quota["credits_limit"]).filter(|value| *value >= 0.0)?;
        let expiry = pack_expiry(pack);
        if present && total.expires_at != expiry {
            common_expiry = false;
        }
        if !present {
            total.expires_at = expiry;
        }
        present = true;
        total.remaining += (limit - consumed(pack, "credits_amount")?).max(0.0);
        total.total += limit;
    }
    if !common_expiry {
        total.expires_at = None;
    }
    present.then_some(total)
}

fn confirmed_reward_only(
    payload: &Value,
    general: Option<CreditTotals>,
    reward: Option<CreditTotals>,
    now: i64,
) -> bool {
    let Some((general, reward)) = general.zip(reward) else {
        return false;
    };
    let Some(packs) = payload["user_entitlement_pack_list"].as_array() else {
        return false;
    };
    let mut consumed_total = 0.0;
    if payload["is_credits_billing"] != true
        || packs.iter().any(|pack| {
            let base = &pack["entitlement_base_info"];
            let quota = pack_quota(pack);
            if base["product_type"].as_u64() == Some(0)
                && quota["credits_limit"].is_null()
                && quota["no_bonus_quota"] == true
                && pack["group_type"].is_null()
            {
                return false;
            }
            let Some(used) = consumed(pack, "credits_amount") else {
                return true;
            };
            consumed_total += used;
            !visible(pack, now)
                || number(&base["currency"]).is_none_or(|currency| ![0.0, 1.0].contains(&currency))
                || number(&base["available_endpoint"]) != Some(0.0)
                || !pack["group_type"]
                    .as_u64()
                    .is_some_and(|group| (1..=5).contains(&group))
                || number(&quota["credits_limit"]).is_none_or(|limit| limit < 0.0)
        })
    {
        return false;
    }
    let equal = |left: f64, right: f64| (left - right).abs() <= 0.000001;
    equal(general.total, reward.total)
        && equal(general.remaining, reward.remaining)
        && number(&payload["usage_summary"]["total_amount"])
            .filter(|total| *total >= 0.0)
            .is_some_and(|total| equal(total, general.total))
        && number(&payload["usage_summary"]["consumed_amount"])
            .filter(|used| *used >= 0.0)
            .is_some_and(|used| equal(used, consumed_total))
}

fn reward_next_expiry(payload: &Value, now: i64) -> Option<i64> {
    let mut next = None;
    for pack in payload["user_entitlement_pack_list"].as_array()? {
        if !visible(pack, now)
            || number(&pack["entitlement_base_info"]["currency"])
                .is_some_and(|currency| ![0.0, 1.0].contains(&currency))
            || !pack["group_type"]
                .as_u64()
                .is_some_and(|group| (1..=5).contains(&group))
        {
            continue;
        }
        let endpoint = number(&pack["entitlement_base_info"]["available_endpoint"])?;
        if endpoint != 0.0 {
            continue;
        }
        let limit = number(&pack_quota(pack)["credits_limit"]).filter(|value| *value >= 0.0)?;
        if limit > consumed(pack, "credits_amount")? {
            let expiry = pack_expiry(pack)?;
            next = Some(next.map_or(expiry, |current: i64| current.min(expiry)));
        }
    }
    next
}

fn domestic_usage(mut account: Account, payload: &Value, now: i64) -> Account {
    let general = credit_totals(payload, 0, None, now);
    let mut base = credit_totals(payload, 0, Some(true), now);
    let reward = credit_totals(payload, 0, Some(false), now);
    if base.is_none() && confirmed_reward_only(payload, general, reward, now) {
        base = Some(CreditTotals {
            remaining: 0.0,
            total: 0.0,
            expires_at: None,
        });
    }
    account.remaining = general.map(|value| value.remaining);
    account.total = general.map(|value| value.total);
    account.expires_at = general.and_then(|value| value.expires_at);
    account.currency = None;
    account.base_remaining = base.map(|value| value.remaining);
    account.base_total = base.map(|value| value.total);
    account.base_reset_at = base.and_then(|value| value.expires_at);
    account.reward_remaining = reward.map(|value| value.remaining);
    account.reward_total = reward.map(|value| value.total);
    account.reward_next_expiry_at = reward.and_then(|_| reward_next_expiry(payload, now));
    account.addon_remaining = None;
    account.work_remaining = credit_totals(payload, 1, None, now).map(|value| value.remaining);
    account
}

fn international_usage(mut account: Account, payload: &Value, now: i64) -> Account {
    account.remaining = None;
    account.total = None;
    account.expires_at = None;
    account.currency = (payload["is_dollar_usage_billing"] == true).then(|| "USD".into());
    account.base_remaining = None;
    account.base_total = None;
    account.base_reset_at = None;
    account.reward_remaining = None;
    account.reward_total = None;
    account.reward_next_expiry_at = None;
    account.addon_remaining = None;
    account.work_remaining = None;
    let Some(packs) = payload["user_entitlement_pack_list"].as_array() else {
        return account;
    };
    let pack = [100, 6, 4, 1, 8, 0].into_iter().find_map(|product| {
        packs.iter().find(|pack| {
            visible(pack, now)
                && pack["entitlement_base_info"]["product_type"].as_u64() == Some(product)
        })
    });
    let Some(pack) = pack else {
        return account;
    };
    let quota = pack_quota(pack);
    let amounts = || {
        let basic = number(&quota["basic_usage_limit"]).filter(|value| *value >= 0.0)?;
        let mut total = basic;
        let mut remaining = (basic - consumed(pack, "basic_usage_amount")?).max(0.0);
        if quota["no_bonus_quota"] != true && !quota["bonus_usage_limit"].is_null() {
            let bonus = number(&quota["bonus_usage_limit"]).filter(|value| *value >= 0.0)?;
            total += bonus;
            remaining += (bonus - consumed(pack, "bonus_usage_amount")?).max(0.0);
        }
        Some((remaining, total))
    };
    if let Some((remaining, total)) = amounts() {
        account.remaining = Some(remaining);
        account.total = Some(total);
        account.expires_at = native::timestamp(&pack["entitlement_base_info"]["end_time"])
            .map(|end| end.saturating_add(1));
    }
    account
}

fn pay_status(mut account: Account, payload: &Value) -> Account {
    account.plan = native::text(payload, "user_pay_identity_str").map(str::to_owned);
    account.subscription_end_at = native::timestamp(&payload["last_pro_entitlement_expire_time"])
        .or_else(|| {
            native::timestamp(&payload["trial_status"]["trial_end_time"])
                .filter(|_| payload["trial_status"]["is_in_trial"] == true)
        });
    account
}

fn checkin(mut account: Account, payload: &Value, now: i64) -> Result<Account, String> {
    let checked = payload["checked_in"]
        .as_bool()
        .ok_or("accountError.authResponse")?;
    let device = payload["did_checked_in"]
        .as_bool()
        .ok_or("accountError.authResponse")?;
    account.daily_claimed_at = checked.then_some(now);
    account.device_claimed_at = device.then_some(now);
    account.reward_enabled = payload["enable"].as_bool();
    Ok(account)
}

async fn refresh_at(
    base: &str,
    edition: Edition,
    token: &str,
    device: &str,
    account: Account,
) -> Result<Account, String> {
    let path = match edition {
        Edition::Trae => "/trae/api/v1/pay/ide_user_pay_status",
        Edition::Traecn => "/trae/api/v2/pay/ide_user_pay_status",
    };
    let pay = post(
        base,
        edition,
        path,
        token,
        device,
        json!({"trae_client":"IDE","device_id":device}),
    )
    .await?;
    let mut account = pay_status(account, &pay);
    if edition == Edition::Trae {
        let usage = post(
            base,
            edition,
            "/trae/api/v1/pay/ide_user_ent_usage",
            token,
            device,
            json!({"require_usage":true}),
        )
        .await?;
        return Ok(international_usage(
            account,
            &usage,
            chrono::Utc::now().timestamp(),
        ));
    }
    let (credits, status) = tokio::try_join!(
        post(
            base,
            edition,
            "/trae/api/v2/pay/web_user_ent_usage",
            token,
            device,
            json!({"require_usage":true,"full_data":true})
        ),
        post(
            base,
            edition,
            "/trae/api/v2/ug/checkin_credits/status",
            token,
            device,
            json!({})
        ),
    )?;
    let now = chrono::Utc::now().timestamp();
    account = domestic_usage(account, &credits, now);
    checkin(account, &status, now)
}

pub(super) async fn refresh(
    base: &str,
    edition: Edition,
    token: &str,
    device: &str,
    account: Account,
) -> Result<Account, String> {
    refresh_at(base, edition, token, device, account).await
}

async fn claim_at(
    base: &str,
    edition: Edition,
    token: &str,
    device: &str,
    account: Account,
) -> Result<Account, String> {
    if edition != Edition::Traecn {
        return Err("accountError.claimUnavailable".into());
    }
    if device.is_empty() {
        return Err("accountError.initializeClient".into());
    }
    let status = post(
        base,
        edition,
        "/trae/api/v2/ug/checkin_credits/status",
        token,
        device,
        json!({}),
    )
    .await?;
    let mut account = checkin(account, &status, chrono::Utc::now().timestamp())?;
    if account.daily_claimed_at.is_some() {
        return Ok(account);
    }
    if account.reward_enabled != Some(true) || account.device_claimed_at.is_some() {
        return Ok(account);
    }
    let result = post(
        base,
        edition,
        "/trae/api/v2/ug/checkin_credits/claim",
        token,
        device,
        json!({}),
    )
    .await?;
    if result["code"].as_i64() == Some(9095) {
        account.device_claimed_at = Some(chrono::Utc::now().timestamp());
        return Ok(account);
    }
    if result["code"].as_i64() != Some(0) {
        return Err("accountError.authResponse".into());
    }
    let now = chrono::Utc::now().timestamp();
    account.daily_claimed_at = Some(now);
    account.device_claimed_at = Some(now);
    Ok(account)
}

pub(super) async fn claim(
    edition: Edition,
    token: &str,
    device: &str,
    account: Account,
) -> Result<Account, String> {
    claim_at(edition.api(), edition, token, device, account).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

    fn account() -> Account {
        Account {
            id: "fixture".into(),
            edition: Edition::Traecn,
            name: "fixture".into(),
            plan: None,
            active: false,
            remaining: None,
            total: None,
            expires_at: None,
            currency: None,
            base_remaining: None,
            base_total: None,
            base_reset_at: None,
            reward_remaining: None,
            reward_total: None,
            reward_next_expiry_at: None,
            addon_remaining: None,
            work_remaining: None,
            daily_claimed_at: None,
            device_claimed_at: None,
            reward_enabled: None,
            subscription_end_at: None,
        }
    }

    fn remaining(payload: &Value, endpoint: u64) -> Option<f64> {
        credit_totals(payload, endpoint, None, 1791836000).map(|value| value.remaining)
    }

    #[test]
    fn credits_use_bucket_quota_minus_consumption_and_preserve_unknown() {
        let payload = json!({"user_entitlement_pack_list":[
            {"entitlement_base_info":{"available_endpoint":0,"quota":{"credits_limit":150}},"usage":{"credits_amount":20.5}},
            {"entitlement_base_info":{"available_endpoint":1,"product_extra":{"package_extra":{"quota":{"credits_limit":2000}}}},"usage":{"credits_amount":100}},
        ]});
        assert_eq!(remaining(&payload, 0), Some(129.5));
        assert_eq!(remaining(&payload, 1), Some(1900.0));
        assert_eq!(remaining(&json!({}), 0), None);
        assert_eq!(
            remaining(
                &json!({"user_entitlement_pack_list":[{"entitlement_base_info":{"available_endpoint":0}}]}),
                0
            ),
            None
        );
        assert_eq!(
            remaining(&json!({"user_entitlement_pack_list":[]}), 0),
            None
        );
        let only_work = json!({"user_entitlement_pack_list":[{"entitlement_base_info":{"available_endpoint":1,"quota":{"credits_limit":150}},"usage":{"credits_amount":150}}]});
        assert_eq!(remaining(&only_work, 0), None);
        assert_eq!(remaining(&only_work, 1), Some(0.0));
        let plan = pay_status(
            account(),
            &json!({"has_package":false,"user_pay_identity_str":"Pro"}),
        );
        assert_eq!(plan.plan.as_deref(), Some("Pro"));
    }

    #[test]
    fn free_and_unclassified_packs_do_not_hide_known_credit_buckets() {
        let payload = json!({"user_entitlement_pack_list":[
            {"entitlement_base_info":{"currency":0}},
            {"entitlement_base_info":{"currency":2,"available_endpoint":0,"quota":{"credits_limit":5000}},"usage":{"credits_amount":100}},
            {"entitlement_base_info":{"currency":1,"quota":{"credits_limit":1000}},"usage":{"credits_amount":0}},
            {"entitlement_base_info":{"currency":1,"available_endpoint":0.0,"quota":{"credits_limit":150}},"usage":{"credits_amount":10}},
            {"entitlement_base_info":{"currency":1,"available_endpoint":1,"quota":{"credits_limit":2000}},"usage":{"credits_amount":100}},
        ]});
        assert_eq!(remaining(&payload, 0), Some(140.0));
        assert_eq!(remaining(&payload, 1), Some(1900.0));
        assert_eq!(
            remaining(
                &json!({"user_entitlement_pack_list":[
                    {"entitlement_base_info":{"currency":0}},
                    {"entitlement_base_info":{"currency":1,"quota":{"credits_limit":150}}},
                ]}),
                0
            ),
            None
        );
    }

    #[test]
    fn known_new_credit_pack_without_usage_is_unconsumed() {
        let mut pack = json!({"entitlement_base_info":{"currency":1,"available_endpoint":0,"product_extra":{"package_extra":{"quota":{"credits_limit":150}}}}});
        assert_eq!(
            remaining(&json!({"user_entitlement_pack_list":[pack.clone()]}), 0),
            Some(150.0)
        );
        for usage in [Value::Null, json!({}), json!({"credits_amount":null})] {
            pack["usage"] = usage;
            assert_eq!(
                remaining(&json!({"user_entitlement_pack_list":[pack.clone()]}), 0),
                Some(150.0)
            );
        }
    }

    #[test]
    fn malformed_usage_and_missing_credit_quota_stay_unknown() {
        let mut pack = json!({"entitlement_base_info":{"currency":1,"available_endpoint":0,"quota":{"credits_limit":150}}});
        for usage in [
            json!("invalid"),
            json!([]),
            json!({"credits_amount":"invalid"}),
            json!({"credits_amount":false}),
        ] {
            pack["usage"] = usage;
            assert_eq!(
                remaining(&json!({"user_entitlement_pack_list":[pack.clone()]}), 0),
                None
            );
        }
        let payload = json!({"user_entitlement_pack_list":[
            {"entitlement_base_info":{"currency":1,"available_endpoint":0,"quota":{"credits_limit":150}},"usage":{"credits_amount":10}},
            {"entitlement_base_info":{"currency":1,"available_endpoint":0},"usage":{"credits_amount":0}},
        ]});
        assert_eq!(remaining(&payload, 0), None);
    }

    #[test]
    fn domestic_grouped_credits_match_reward_only_snapshot() {
        let pack = |group, limit, end| {
            json!({
                "group_type":group,"is_hide":false,"status":1,"expire_time":end,
                "entitlement_base_info":{
                    "product_type":2,"currency":1,"available_endpoint":0,
                    "start_time":1789728134,"end_time":end,
                    "product_extra":{"package_extra":{"quota":{"credits_limit":limit}}}
                },
                "usage":{}
            })
        };
        let payload = json!({"is_credits_billing":true,"usage_summary":{"total_amount":4600,"consumed_amount":0},"user_entitlement_pack_list":[
            pack(4,2000,1792406534),pack(4,2000,1792406534),
            {"is_hide":false,"entitlement_base_info":{"product_type":0,"currency":0,"available_endpoint":0,"quota":{"no_bonus_quota":true}}},
            pack(3,500,1793462399),pack(1,100,1794513490)
        ]});
        let result = domestic_usage(account(), &payload, 1791836000);
        assert_eq!(result.remaining, Some(4600.0));
        assert_eq!(result.total, Some(4600.0));
        assert_eq!(result.reward_remaining, Some(4600.0));
        assert_eq!(result.reward_total, Some(4600.0));
        assert_eq!(result.base_remaining, Some(0.0));
        assert_eq!(result.base_total, Some(0.0));
        assert_eq!(result.base_reset_at, None);
        assert_eq!(result.expires_at, None);
        assert_eq!(result.work_remaining, None);
        assert_eq!(result.addon_remaining, None);
        assert_eq!(result.reward_next_expiry_at, Some(1792406534));
    }

    #[test]
    fn missing_malformed_unknown_or_partial_snapshot_never_implies_zero_base() {
        let valid = json!({"is_credits_billing":true,
            "usage_summary":{"total_amount":100,"consumed_amount":25},
            "user_entitlement_pack_list":[{"group_type":1,
                "entitlement_base_info":{"product_type":2,"currency":1,"available_endpoint":0,"quota":{"credits_limit":100}},
                "usage":{"credits_amount":25}}]
        });
        let result = domestic_usage(account(), &valid, 1791836000);
        assert_eq!(result.base_remaining, Some(0.0));
        assert_eq!(result.base_total, Some(0.0));
        assert_eq!(result.reward_remaining, Some(75.0));
        for (path, value) in [
            ("/usage_summary", Value::Null),
            ("/usage_summary/total_amount", json!(200)),
            ("/usage_summary/consumed_amount", json!(0)),
            ("/usage_summary/consumed_amount", json!("invalid")),
            ("/is_credits_billing", json!(false)),
            ("/user_entitlement_pack_list", json!([])),
            ("/user_entitlement_pack_list/0/group_type", json!(99)),
            ("/user_entitlement_pack_list/0/group_type", Value::Null),
            (
                "/user_entitlement_pack_list/0/usage/credits_amount",
                json!("invalid"),
            ),
            (
                "/user_entitlement_pack_list/0/entitlement_base_info/currency",
                json!(2),
            ),
            (
                "/user_entitlement_pack_list/0/entitlement_base_info/available_endpoint",
                Value::Null,
            ),
            (
                "/user_entitlement_pack_list/0/entitlement_base_info/quota/credits_limit",
                Value::Null,
            ),
        ] {
            let mut payload = valid.clone();
            *payload.pointer_mut(path).unwrap() = value;
            let result = domestic_usage(account(), &payload, 1791836000);
            assert_eq!(result.base_remaining, None, "{path}");
            assert_eq!(result.base_total, None, "{path}");
        }
        let mut paid = valid;
        paid["user_entitlement_pack_list"][0]["group_type"] = 0.into();
        let result = domestic_usage(account(), &paid, 1791836000);
        assert_eq!(result.base_remaining, Some(75.0));
        assert_eq!(result.base_total, Some(100.0));
        let mut future = paid["user_entitlement_pack_list"][0].clone();
        future["entitlement_base_info"]["start_time"] = 1794513490.into();
        paid["user_entitlement_pack_list"]
            .as_array_mut()
            .unwrap()
            .push(future);
        assert_eq!(
            domestic_usage(account(), &paid, 1791836000).base_remaining,
            Some(75.0)
        );
    }

    #[test]
    fn next_reward_expiry_requires_dates_for_all_remaining_reward_packs() {
        let pack = |end, used| {
            json!({"group_type":1,"expire_time":end,
                "entitlement_base_info":{"available_endpoint":0,"quota":{"credits_limit":100}},
                "usage":{"credits_amount":used}
            })
        };
        let mut packs = vec![
            pack(1792406534, 100),
            pack(1793462399, 10),
            pack(1794513490, 0),
        ];
        let payload = |packs: &[Value]| json!({"user_entitlement_pack_list":packs});
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            Some(1793462399)
        );
        packs[2]["expire_time"] = 1793462399.into();
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            Some(1793462399)
        );
        packs[2]["expire_time"] = Value::Null;
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            None
        );
        packs[2]["expire_time"] = "invalid".into();
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            None
        );
        packs[2]["usage"]["credits_amount"] = 100.into();
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            Some(1793462399)
        );
        packs[1]["expire_time"] = 1791835999.into();
        assert_eq!(
            domestic_usage(account(), &payload(&packs), 1791836000).reward_next_expiry_at,
            None
        );
    }

    #[test]
    fn credit_groups_use_explicit_type_and_only_shared_validity() {
        let pack = |group, endpoint, limit, used, end| {
            json!({"group_type":group,"expire_time":end,
                "entitlement_base_info":{"available_endpoint":endpoint,"quota":{"credits_limit":limit}},
                "usage":{"credits_amount":used}
            })
        };
        let mut packs = vec![
            pack(0, 0, 100, 25, 1793462399),
            pack(0, 0, 50, 20, 1793462399),
            pack(1, 0, 100, 10, 1794513490),
            pack(0, 1, 1000, 100, 1793462399),
            pack(99, 0, 20, 0, 1793462399),
            pack(0, 0, 500, 0, 1791835999),
        ];
        let result = domestic_usage(
            account(),
            &json!({"user_entitlement_pack_list":packs}),
            1791836000,
        );
        assert_eq!(result.remaining, Some(215.0));
        assert_eq!(result.total, Some(270.0));
        assert_eq!(result.base_remaining, Some(105.0));
        assert_eq!(result.base_total, Some(150.0));
        assert_eq!(result.base_reset_at, Some(1793462399));
        assert_eq!(result.reward_remaining, Some(90.0));
        assert_eq!(result.reward_total, Some(100.0));
        assert_eq!(result.work_remaining, Some(900.0));
        packs[1]["expire_time"] = 1794513490.into();
        packs[2]["is_hide"] = true.into();
        let result = domestic_usage(
            account(),
            &json!({"user_entitlement_pack_list":packs}),
            1791836000,
        );
        assert_eq!(result.base_remaining, Some(105.0));
        assert_eq!(result.base_reset_at, None);
        assert_eq!(result.reward_remaining, None);
        assert_eq!(result.reward_total, None);
    }

    #[test]
    fn malformed_and_unlimited_credit_groups_keep_progress_unknown() {
        for limit in [json!(-1), json!("invalid"), Value::Null] {
            let result = domestic_usage(
                account(),
                &json!({"user_entitlement_pack_list":[{"group_type":0,
                    "entitlement_base_info":{"available_endpoint":0,"quota":{"credits_limit":limit}},
                    "usage":{"credits_amount":0}}]}),
                1791836000,
            );
            assert_eq!(result.remaining, None);
            assert_eq!(result.total, None);
            assert_eq!(result.base_remaining, None);
            assert_eq!(result.base_total, None);
        }
        let result = domestic_usage(account(), &json!({}), 1791836000);
        assert_eq!(result.total, None);
        assert_eq!(result.reward_total, None);
    }

    #[test]
    fn international_free_snapshot_uses_dollar_quota_instead_of_zero_credits() {
        let mut international = account();
        international.edition = Edition::Trae;
        let payload = json!({"is_dollar_usage_billing":true,"user_entitlement_pack_list":[{
            "status":1,"is_hide":false,"expire_time":0,"next_billing_time":0,
            "entitlement_base_info":{"product_type":0,"currency":0,"start_time":1790812800,"end_time":1793491199,
                "quota":{"credits_limit":0,"basic_usage_limit":1,"bonus_usage_limit":0,"no_bonus_quota":true}},
            "usage":{"credits_amount":0,"basic_usage_amount":0,"bonus_usage_amount":0}
        }]});
        let result = international_usage(international, &payload, 1791836000);
        assert_eq!(result.currency.as_deref(), Some("USD"));
        assert_eq!(result.remaining, Some(1.0));
        assert_eq!(result.total, Some(1.0));
        assert_eq!(result.expires_at, Some(1793491200));
        assert_eq!(result.reward_remaining, None);
    }

    #[test]
    fn international_current_plan_preserves_bonus_and_malformed_unknown() {
        let mut payload = json!({"is_dollar_usage_billing":true,"user_entitlement_pack_list":[
            {"entitlement_base_info":{"product_type":0,"quota":{"basic_usage_limit":1}},"usage":{}},
            {"expire_time":1824998399,"yearly_expire_time":1824998399,"next_billing_time":1824998400,
                "entitlement_base_info":{"product_type":1,"end_time":1793462399,
                "product_extra":{"subscription_extra":{"quota":{"basic_usage_limit":20,"bonus_usage_limit":5}}}},
                "usage":{"basic_usage_amount":3.5,"bonus_usage_amount":2}},
            {"entitlement_base_info":{"product_type":2,"quota":{"basic_usage_limit":1000}},"usage":{}}
        ]});
        let result = international_usage(account(), &payload, 1791836000);
        assert_eq!(result.remaining, Some(19.5));
        assert_eq!(result.total, Some(25.0));
        assert_eq!(result.expires_at, Some(1793462400));
        payload["user_entitlement_pack_list"][1]["entitlement_base_info"]["end_time"] = Value::Null;
        assert_eq!(
            international_usage(account(), &payload, 1791836000).expires_at,
            None
        );
        payload["user_entitlement_pack_list"][1]["usage"]["basic_usage_amount"] = "invalid".into();
        let result = international_usage(account(), &payload, 1791836000);
        assert_eq!(result.remaining, None);
        assert_eq!(result.total, None);
        assert_eq!(result.expires_at, None);
        let result = international_usage(account(), &json!({}), 1791836000);
        assert_eq!(result.remaining, None);
        assert_eq!(result.total, None);
        assert_eq!(result.currency, None);
    }

    #[test]
    fn daily_status_separates_account_and_device() {
        let now = 1791774000;
        let device = checkin(
            account(),
            &json!({"checked_in":false,"did_checked_in":true,"enable":true}),
            now,
        )
        .unwrap();
        assert_eq!(device.daily_claimed_at, None);
        assert_eq!(device.device_claimed_at, Some(now));
        let both = checkin(
            device,
            &json!({"checked_in":true,"did_checked_in":true,"enable":true}),
            now,
        )
        .unwrap();
        assert_eq!(both.daily_claimed_at, Some(now));
    }

    #[derive(Clone)]
    struct Fixture {
        status: Value,
        claim_result: Value,
        requests: Arc<Mutex<Vec<(String, String)>>>,
    }

    async fn pay(State(fixture): State<Fixture>, Json(body): Json<Value>) -> Json<Value> {
        assert_eq!(body["trae_client"], "IDE");
        fixture
            .requests
            .lock()
            .unwrap()
            .push(("pay".into(), body["device_id"].as_str().unwrap().into()));
        Json(json!({"user_pay_identity_str":"Free"}))
    }

    async fn usage(
        State(fixture): State<Fixture>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(headers["authorization"], "Cloud-IDE-JWT fixture-token");
        assert_eq!(headers["origin"], "https://www.trae.ai");
        assert_eq!(body, json!({"require_usage":true}));
        fixture.requests.lock().unwrap().push((
            "usage".into(),
            headers["x-device-id"].to_str().unwrap().into(),
        ));
        Json(
            json!({"is_dollar_usage_billing":true,"user_entitlement_pack_list":[{
                "entitlement_base_info":{"product_type":0,"quota":{"basic_usage_limit":1,"credits_limit":0,"no_bonus_quota":true}},
                "usage":{"basic_usage_amount":0}
            }]}),
        )
    }

    async fn domestic_credits(
        State(fixture): State<Fixture>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Json<Value> {
        assert_eq!(headers["authorization"], "Cloud-IDE-JWT fixture-token");
        assert_eq!(headers["origin"], "https://www.trae.cn");
        assert_eq!(body, json!({"require_usage":true,"full_data":true}));
        fixture.requests.lock().unwrap().push((
            "credits".into(),
            headers["x-device-id"].to_str().unwrap().into(),
        ));
        Json(json!({"is_credits_billing":true,
            "usage_summary":{"total_amount":100,"consumed_amount":0},
            "user_entitlement_pack_list":[{"group_type":1,"expire_time":"2099-01-01T00:00:00Z",
                "entitlement_base_info":{"product_type":2,"currency":1,"available_endpoint":0,"quota":{"credits_limit":100}},
                "usage":{}
            }]
        }))
    }

    async fn status(State(fixture): State<Fixture>, headers: HeaderMap) -> Json<Value> {
        fixture.requests.lock().unwrap().push((
            "status".into(),
            headers
                .get("x-device-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .into(),
        ));
        Json(fixture.status)
    }

    async fn claim(State(fixture): State<Fixture>, headers: HeaderMap) -> Json<Value> {
        assert_eq!(headers["authorization"], "Cloud-IDE-JWT fixture-token");
        fixture.requests.lock().unwrap().push((
            "claim".into(),
            headers
                .get("x-device-id")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .into(),
        ));
        Json(fixture.claim_result)
    }

    async fn server(status_value: Value) -> (String, Fixture, tokio::task::JoinHandle<()>) {
        server_result(status_value, json!({"code":0})).await
    }

    async fn server_result(
        status_value: Value,
        claim_result: Value,
    ) -> (String, Fixture, tokio::task::JoinHandle<()>) {
        let fixture = Fixture {
            status: status_value,
            claim_result,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let router = Router::new()
            .route("/trae/api/v1/pay/ide_user_pay_status", post(pay))
            .route("/trae/api/v1/pay/ide_user_ent_usage", post(usage))
            .route("/trae/api/v2/pay/ide_user_pay_status", post(pay))
            .route(
                "/trae/api/v2/pay/web_user_ent_usage",
                post(domestic_credits),
            )
            .route("/trae/api/v2/ug/checkin_credits/status", post(status))
            .route("/trae/api/v2/ug/checkin_credits/claim", post(claim))
            .with_state(fixture.clone());
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (base, fixture, task)
    }

    #[tokio::test]
    async fn explicit_international_refresh_queries_usage_and_never_checkin() {
        let (base, fixture, server) = server(json!({})).await;
        let mut international = account();
        international.edition = Edition::Trae;
        let result = refresh_at(
            &base,
            Edition::Trae,
            "fixture-token",
            "native-device",
            international,
        )
        .await
        .unwrap();
        assert_eq!(result.plan.as_deref(), Some("Free"));
        assert_eq!(result.currency.as_deref(), Some("USD"));
        assert_eq!(result.remaining, Some(1.0));
        assert_eq!(result.total, Some(1.0));
        assert_eq!(
            *fixture.requests.lock().unwrap(),
            vec![
                ("pay".into(), "native-device".into()),
                ("usage".into(), "native-device".into())
            ]
        );
        server.abort();
    }

    #[tokio::test]
    async fn explicit_domestic_refresh_requests_full_snapshot_without_claiming() {
        let (base, fixture, server) =
            server(json!({"checked_in":true,"did_checked_in":true,"enable":true})).await;
        let result = refresh_at(
            &base,
            Edition::Traecn,
            "fixture-token",
            "native-device",
            account(),
        )
        .await
        .unwrap();
        assert_eq!(result.base_remaining, Some(0.0));
        assert_eq!(result.base_total, Some(0.0));
        assert_eq!(result.reward_remaining, Some(100.0));
        assert!(result.reward_next_expiry_at.is_some());
        let mut requests = fixture.requests.lock().unwrap().clone();
        requests.sort();
        assert_eq!(
            requests,
            vec![
                ("credits".into(), "native-device".into()),
                ("pay".into(), "native-device".into()),
                ("status".into(), "native-device".into())
            ]
        );
        server.abort();
    }

    #[tokio::test]
    async fn explicit_claim_positive_control_uses_native_device_once() {
        let (base, fixture, server) =
            server(json!({"checked_in":false,"did_checked_in":false,"enable":true})).await;
        let result = claim_at(
            &base,
            Edition::Traecn,
            "fixture-token",
            "native-device",
            account(),
        )
        .await
        .unwrap();
        assert!(result.daily_claimed_at.is_some());
        assert_eq!(
            *fixture.requests.lock().unwrap(),
            vec![
                ("status".into(), "native-device".into()),
                ("claim".into(), "native-device".into())
            ]
        );
        server.abort();
    }

    #[tokio::test]
    async fn device_limit_and_missing_device_never_claim() {
        let (base, fixture, server) =
            server(json!({"checked_in":false,"did_checked_in":true,"enable":true})).await;
        let result = claim_at(
            &base,
            Edition::Traecn,
            "fixture-token",
            "native-device",
            account(),
        )
        .await
        .unwrap();
        assert_eq!(result.daily_claimed_at, None);
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
        assert!(
            claim_at(&base, Edition::Traecn, "fixture-token", "", account())
                .await
                .is_err()
        );
        assert!(claim_at(
            &base,
            Edition::Trae,
            "fixture-token",
            "native-device",
            account()
        )
        .await
        .is_err());
        assert_eq!(fixture.requests.lock().unwrap().len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn concurrent_device_limit_does_not_claim_for_account_or_invalidate_login() {
        let (base, fixture, server) = server_result(
            json!({"checked_in":false,"did_checked_in":false,"enable":true}),
            json!({"code":9095}),
        )
        .await;
        let result = claim_at(
            &base,
            Edition::Traecn,
            "fixture-token",
            "native-device",
            account(),
        )
        .await
        .unwrap();
        assert_eq!(result.daily_claimed_at, None);
        assert!(result.device_claimed_at.is_some());
        assert_eq!(fixture.requests.lock().unwrap().len(), 2);
        server.abort();
    }
}
