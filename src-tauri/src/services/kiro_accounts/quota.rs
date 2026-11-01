use super::{client, response, text, Account, Credentials, Quota};
use serde_json::{json, Value};
#[cfg(test)]
#[path = "quota_tests.rs"]
mod tests;

pub(super) async fn fetch(credentials: &mut Credentials) -> Result<Value, String> {
    if credentials.profile().is_none() && !credentials.provider().eq_ignore_ascii_case("BuilderId")
    {
        let mut failure = "accountError.authResponse".to_string();
        for region in ["us-east-1", "eu-central-1"] {
            let result = response(
                client()?
                    .post(format!(
                        "https://management.{region}.kiro.dev/List-Available-Profiles"
                    ))
                    .bearer_auth(credentials.access()?)
                    .json(&json!({}))
                    .send()
                    .await
                    .map_err(|_| "accountError.network")?,
            )
            .await;
            match result {
                Ok(data) => {
                    if let Some(profile) = data["profiles"]
                        .as_array()
                        .and_then(|rows| rows.iter().find_map(|row| text(row, "arn")))
                    {
                        credentials.token["profile_arn"] = json!(profile);
                        break;
                    }
                }
                Err(error) => failure = error,
            }
        }
        if credentials.profile().is_none() {
            return Err(failure);
        }
    }
    let mut request = client()?
        .get(format!(
            "https://management.{}.kiro.dev/Get-Usage-Limits",
            credentials.region()?
        ))
        .bearer_auth(credentials.access()?)
        .query(&[
            ("origin", "KIRO_CLI"),
            ("resourceType", "CREDIT"),
            ("isEmailRequired", "true"),
        ]);
    if let Some(profile) = credentials.profile() {
        request = request.query(&[("profileArn", profile)]);
    }
    if credentials.provider().eq_ignore_ascii_case("ExternalIdp") {
        request = request.header("tokentype", "EXTERNAL_IDP");
    }
    let usage = response(request.send().await.map_err(|_| "accountError.network")?).await?;
    if credentials.subject.is_none() {
        credentials.subject = text(&usage["userInfo"], "userId").map(str::to_owned);
    }
    credentials.name = text(&usage["userInfo"], "email")
        .map(str::to_owned)
        .or(credentials.name.take());
    if credentials.identity().is_none() {
        credentials.subject = credentials.name.clone();
    }
    credentials.usage = Some(usage.clone());
    Ok(usage)
}

fn number(value: &Value, precise: &str, fallback: &str) -> Option<f64> {
    value[precise]
        .as_f64()
        .or_else(|| value[fallback].as_f64())
        .filter(|value| value.is_finite() && *value >= 0.)
}
fn timestamp(value: &Value) -> Option<i64> {
    value.as_i64().filter(|value| *value > 0).or_else(|| {
        value
            .as_f64()
            .filter(|v| v.is_finite() && *v > 0. && *v < i64::MAX as f64)
            .map(|v| v as i64)
    })
}
fn bucket(value: &Value, label: String, date: Option<i64>) -> Quota {
    let total = number(value, "usageLimitWithPrecision", "usageLimit");
    let used = number(value, "currentUsageWithPrecision", "currentUsage");
    Quota {
        label,
        total,
        remaining: total.zip(used).map(|(total, used)| (total - used).max(0.)),
        reset_at: date,
    }
}

pub(super) fn update(account: &mut Account, value: &Value) {
    account.plan = text(&value["subscriptionInfo"], "subscriptionTitle")
        .or_else(|| text(&value["subscriptionInfo"], "type"))
        .map(|value| {
            let value = value.trim().strip_prefix("KIRO ").unwrap_or(value.trim());
            match value.to_ascii_uppercase().as_str() {
                "FREE" => "Free",
                "PRO" => "Pro",
                "PRO+" | "PRO_PLUS" => "Pro+",
                "PRO MAX" | "PRO_MAX" => "Pro Max",
                "POWER" => "Power",
                _ => value,
            }
            .to_owned()
        });
    let Some(rows) = value["usageBreakdownList"].as_array() else {
        account.quota.clear();
        return;
    };
    // Some plans list their one bucket without a resource type.
    let unlabeled_only = rows.len() == 1 && text(&rows[0], "resourceType").is_none();
    account.quota = rows
        .iter()
        .filter(|row| {
            unlabeled_only
                || text(row, "resourceType").is_some_and(|kind| kind.eq_ignore_ascii_case("CREDIT"))
        })
        .flat_map(|row| {
            let mut quotas = vec![bucket(
                row,
                "credits".into(),
                timestamp(&row["nextDateReset"]).or_else(|| timestamp(&value["nextDateReset"])),
            )];
            let trial = &row["freeTrialInfo"];
            if text(trial, "freeTrialStatus")
                .is_some_and(|status| status.eq_ignore_ascii_case("ACTIVE"))
            {
                quotas.push(bucket(
                    trial,
                    "trial".into(),
                    timestamp(&trial["freeTrialExpiry"]),
                ));
            }
            if let Some(bonuses) = row["bonuses"].as_array() {
                for bonus in bonuses.iter().filter(|bonus| {
                    text(bonus, "status")
                        .is_some_and(|status| status.eq_ignore_ascii_case("ACTIVE"))
                }) {
                    quotas.push(bucket(
                        bonus,
                        "bonus".into(),
                        timestamp(&bonus["expiresAt"]).or_else(|| timestamp(&bonus["expiryDate"])),
                    ));
                }
            }
            quotas
        })
        .collect();
}
