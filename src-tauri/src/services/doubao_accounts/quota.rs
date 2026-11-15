use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Quota {
    pub(super) windows: Vec<QuotaWindow>,
    pub(super) bonus: Option<QuotaBucket>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaWindow {
    pub(super) kind: String,
    pub(super) label: Option<String>,
    pub(super) remaining: Option<f64>,
    pub(super) total: Option<f64>,
    pub(super) used_percent: Option<f64>,
    pub(super) reset_at: Option<i64>,
    pub(super) unlimited: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuotaBucket {
    pub(super) remaining: Option<f64>,
    pub(super) total: Option<f64>,
    pub(super) used_percent: Option<f64>,
    pub(super) expires_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DailyReward {
    pub(super) status: String,
    pub(super) scene: Option<String>,
    pub(super) next_claim_at: Option<i64>,
}

impl Default for DailyReward {
    fn default() -> Self {
        Self {
            status: "unknown".into(),
            scene: None,
            next_claim_at: None,
        }
    }
}

fn data(value: &Value) -> &Value {
    value
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(value)
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn timestamp(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
        .filter(|time| *time > 0)
        .map(|time| time / 1000)
        .filter(|time| *time > 0)
}

fn bucket(value: &Value) -> QuotaBucket {
    let total = number(&value["total_amount"]);
    let used = number(&value["used_amount"]);
    QuotaBucket {
        remaining: total.zip(used).map(|(total, used)| (total - used).max(0.0)),
        total,
        used_percent: number(&value["used_percent"]).map(|percent| percent.min(100.0)),
        expires_at: timestamp(&value["end_time"]),
    }
}

fn limits(section: &Value) -> impl Iterator<Item = (&Value, &Value)> {
    section["window_limit_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|group| {
            group["window_limits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |limit| (group, limit))
        })
}

fn usable(section: &Value) -> bool {
    section["usage_exhausted"] != true
        && limits(section)
            .any(|(_, limit)| matches!(limit["window_type"].as_u64(), Some(1 | 2 | 4)))
}

pub(super) fn parse_overview(value: &Value) -> Quota {
    let data = data(value);
    let personal = &data["window_limit_section"];
    let enterprise = &data["enterprise_window_limit_section"];
    let has_personal = data["member_info"]["hasActiveSubscription"] == true;
    let has_enterprise = data["member_info"]["hasEnterpriseSubscription"] == true;
    let enterprise_selected =
        has_enterprise && (usable(enterprise) || !has_personal || !usable(personal));
    let section = if enterprise_selected {
        enterprise
    } else if has_personal {
        personal
    } else {
        &Value::Null
    };
    let mut windows: Vec<_> = limits(section)
        .filter_map(|(group, limit)| {
            let kind = match limit["window_type"].as_u64()? {
                1 => "5h",
                2 => "7d",
                4 => "1m",
                _ => return None,
            };
            Some(window(group, limit, kind))
        })
        .collect();
    let bonus = if enterprise_selected {
        None
    } else {
        windows.extend(
            limits(&data["quota_package_section"])
                .filter(|(_, limit)| limit["window_type"] == 3)
                .map(|(group, limit)| window(group, limit, "subscription")),
        );
        limits(&data["incentive_quota_section"])
            .find(|(_, limit)| limit["window_type"] == 3)
            .map(|(_, limit)| bucket(limit))
    };
    Quota { windows, bonus }
}

fn window(group: &Value, value: &Value, kind: &str) -> QuotaWindow {
    let bucket = bucket(value);
    let exempt = kind == "5h" && value["exemption"]["active"] == true;
    QuotaWindow {
        kind: kind.into(),
        label: text(&group["feature_group_name"]),
        remaining: bucket.remaining,
        total: bucket.total,
        used_percent: bucket.used_percent,
        reset_at: if exempt {
            timestamp(&value["exemption"]["end_time"])
        } else {
            bucket.expires_at
        },
        unlimited: exempt || value["unlimited"] == true,
    }
}

pub(super) fn parse_plan(value: &Value) -> Option<String> {
    let data = data(value);
    text(&data["membership_display_name"])
        .or_else(|| text(&data["current_subscription"]["display"]["short_name"]))
        .or_else(|| text(&data["current_subscription"]["display"]["product_name"]))
        .or_else(|| text(&data["subscription"]["display"]["short_name"]))
}

#[cfg(test)]
mod tests;
