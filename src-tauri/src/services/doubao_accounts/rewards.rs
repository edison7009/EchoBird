use super::{api, Credentials, DailyReward};
use serde_json::{json, Value};

const V3_GROUP: &str = "pc_incentive_task_mvp_v3";

async fn detail(c: &Credentials) -> Result<Option<(String, Value)>, String> {
    let launch = api::post(c, "/alice/user/launch", &json!({})).await?;
    let Some(group) = launch["data"]["ug_campaign_info"]["campaign_group_code"]
        .as_str()
        .filter(|group| !group.trim().is_empty())
    else {
        return Ok(None);
    };
    let path = if group == V3_GROUP {
        "/alice/ug/campaign/v3/detail/"
    } else {
        "/alice/ug/campaign/list/"
    };
    let value = api::post(c, path, &json!({"campaign_group_code":group})).await?;
    let data = value["data"].as_object().ok_or("accountError.format")?;
    if group == V3_GROUP
        && data
            .get("campaign_code")
            .and_then(Value::as_str)
            .is_some_and(|code| !code.is_empty() && code != group)
    {
        return Err("accountError.format".into());
    }
    Ok(Some((group.to_owned(), value)))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value[key]
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}

#[derive(PartialEq)]
struct DailyTask {
    group: String,
    item: String,
    status: &'static str,
    claim_body: Option<Value>,
}

fn daily_task(value: &Value, group: &str) -> Option<DailyTask> {
    fn collect(value: &Value, group: &str, tasks: &mut Vec<DailyTask>) {
        let group = text(value, "campaign_group_code")
            .or_else(|| text(value, "campaign_code"))
            .unwrap_or(group);
        for key in ["tasks", "campaigns"] {
            for row in value[key].as_array().into_iter().flatten() {
                // The server's typed daily item distinguishes sign-in from all
                // other completed tasks; names and the landing bubble do not.
                let Some(checked) = row["daily_checkin"]["checked_in_today"].as_bool() else {
                    continue;
                };
                let Some(item) = text(row, "campaign_code") else {
                    continue;
                };
                let group = text(row, "campaign_group_code").unwrap_or(group);
                let state = number(&row["task_status"]);
                let direct = number(&row["reward_mode"]) == Some(2);
                let claim_body = if direct {
                    (!group.is_empty())
                        .then(|| json!({"task_id":"","campaign_group_code":group,"campaign_item_code":item}))
                } else {
                    text(row, "task_id").map(|id| json!({"task_id":id}))
                };
                let status = if checked || state == Some(3) {
                    "claimed"
                } else if matches!(state, Some(1 | 2 | 4))
                    && (state == Some(2) || direct)
                    && claim_body.is_some()
                {
                    "available"
                } else if matches!(state, Some(1 | 4)) && !direct {
                    "unavailable"
                } else {
                    "unknown"
                };
                let task = DailyTask {
                    group: group.into(),
                    item: item.into(),
                    status,
                    claim_body: if status == "available" {
                        claim_body
                    } else {
                        None
                    },
                };
                if !tasks.contains(&task) {
                    tasks.push(task);
                }
            }
        }
        if let Some(child) = value.get("campaign_group") {
            collect(child, group, tasks);
        }
        for child in value["campaign_groups"].as_array().into_iter().flatten() {
            collect(child, group, tasks);
        }
    }
    let mut tasks = vec![];
    collect(value.get("data").unwrap_or(value), group, &mut tasks);
    (tasks.len() == 1).then(|| tasks.remove(0))
}

pub(super) async fn status(c: &Credentials) -> Result<DailyReward, String> {
    let Some((group, value)) = detail(c).await? else {
        return Ok(DailyReward {
            status: "unavailable".into(),
            ..DailyReward::default()
        });
    };
    Ok(DailyReward {
        status: daily_task(&value, &group)
            .map_or("unavailable", |task| task.status)
            .into(),
        scene: Some(group),
        next_claim_at: None,
    })
}

fn receipt(value: &Value) -> Result<bool, String> {
    let data = value["data"].as_object().ok_or("accountError.claim")?;
    if data.get("success") == Some(&Value::Bool(false)) {
        return Err("accountError.claim".into());
    }
    if let Some(error) = data.get("biz_error").filter(|error| !error.is_null()) {
        // These are accepted acknowledgements in the desktop. They do not by
        // themselves prove that today's reward was claimed.
        if !matches!(number(&error["code"]), Some(0 | 4 | 6)) {
            return Err("accountError.claim".into());
        }
    }
    Ok(number(&value["data"]["task_status"]) == Some(3)
        || value["data"]["checked_in_today"] == true
        || value["data"]["daily_checkin"]["checked_in_today"] == true)
}

fn after_claim(group: &str, confirmed: bool, fresh: Result<DailyReward, String>) -> DailyReward {
    if confirmed {
        return DailyReward {
            status: "claimed".into(),
            scene: Some(group.into()),
            next_claim_at: None,
        };
    }
    fresh.unwrap_or_else(|_| DailyReward {
        status: "unknown".into(),
        scene: Some(group.into()),
        next_claim_at: None,
    })
}

pub(super) async fn claim(c: &Credentials) -> Result<DailyReward, String> {
    let mut reward = DailyReward {
        status: "unavailable".into(),
        ..DailyReward::default()
    };
    let Some((group, value)) = detail(c).await? else {
        return Ok(reward);
    };
    reward.scene = Some(group.clone());
    let Some(task) = daily_task(&value, &group) else {
        return Ok(reward);
    };
    reward.status = task.status.into();
    if task.status != "available" {
        return Ok(reward);
    }
    let body = task.claim_body.ok_or("accountError.claimUnavailable")?;
    let confirmed = receipt(&api::post(c, "/alice/ug/campaign/item/reward/claim/", &body).await?)?;
    Ok(after_claim(&group, confirmed, status(c).await))
}

#[cfg(test)]
mod tests;
