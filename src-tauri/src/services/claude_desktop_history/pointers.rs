use serde_json::{json, Value};
use std::collections::HashSet;

fn valid_desktop_id(value: &str) -> bool {
    value
        .strip_prefix("local_")
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
}

pub(super) fn native_pointer(
    source: &super::SourceSession,
    desktop_id: &str,
    cli_id: &str,
) -> Value {
    assert!(valid_desktop_id(desktop_id));
    assert!(uuid::Uuid::parse_str(cli_id).is_ok());
    // Native CLI adoption persists a pointer to an existing host transcript.
    // importedFrom would instead require Desktop's separate import confirmation.
    json!({
        "sessionId": desktop_id,
        "cliSessionId": cli_id,
        "cwd": source.cwd,
        "originCwd": source.cwd,
        "title": source.title,
        "createdAt": source.created_at,
        "lastActivityAt": source.last_activity_at,
        "indexedAt": chrono::Utc::now().timestamp_millis(),
        "isArchived": source.is_archived,
        "adoptedFromOtherSurface": true,
    })
}

pub(super) fn portable_pointer(pointer: &Value) -> Option<Value> {
    let record = pointer.as_object()?;
    if ["sshConfig", "wslConfig"]
        .iter()
        .any(|field| record.get(*field).is_some_and(|value| !value.is_null()))
        || record.get("violinBow") == Some(&Value::Bool(true))
    {
        return None;
    }
    let desktop_id = record.get("sessionId")?.as_str()?;
    let cli_id = match record.get("cliSessionId") {
        None | Some(Value::Null) => record.get("unarchivedCliSessionId")?.as_str()?,
        Some(Value::String(value)) if value.is_empty() => {
            record.get("unarchivedCliSessionId")?.as_str()?
        }
        Some(Value::String(value)) => value,
        Some(_) => return None,
    };
    if !valid_desktop_id(desktop_id) || uuid::Uuid::parse_str(cli_id).is_err() {
        return None;
    }
    let cwd = record.get("cwd")?.as_str()?;
    if cwd.trim().is_empty() {
        return None;
    }
    let origin_cwd = match record.get("originCwd") {
        None | Some(Value::Null) => cwd,
        Some(Value::String(value)) if value.is_empty() => cwd,
        Some(Value::String(value)) => value,
        Some(_) => return None,
    };
    let created_at = record.get("createdAt")?.as_i64()?;
    let last_activity_at = record.get("lastActivityAt")?.as_i64()?;
    let archived = match record.get("isArchived") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return None,
    };
    // Do not carry another account's connectors, permissions, model, tasks,
    // cloud bindings, or pending runtime work into a new tenancy.
    let mut portable = json!({
        "sessionId": desktop_id,
        "cliSessionId": cli_id,
        "cwd": cwd,
        "originCwd": origin_cwd,
        "createdAt": created_at,
        "lastActivityAt": last_activity_at,
        "indexedAt": chrono::Utc::now().timestamp_millis(),
        "isArchived": archived,
        "adoptedFromOtherSurface": true,
    });
    if let Some(title) = record.get("title").and_then(Value::as_str) {
        portable["title"] = title.into();
    }
    // Desktop persists lineage in priorCliSessionIds. Its unarchived and
    // pre-clear handles are runtime fields, so retain their claims there too.
    let mut lineage = Vec::new();
    let mut seen = HashSet::new();
    for id in record
        .get("priorCliSessionIds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(record.get("preClearCliSessionId"))
        .chain(record.get("unarchivedCliSessionId"))
        .filter_map(Value::as_str)
    {
        if id != cli_id && uuid::Uuid::parse_str(id).is_ok() && seen.insert(id) {
            lineage.push(id);
        }
    }
    if !lineage.is_empty() {
        let start = lineage.len().saturating_sub(200);
        portable["priorCliSessionIds"] = json!(&lineage[start..]);
    }
    Some(portable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLI_ID: &str = "11111111-1111-4111-8111-111111111111";
    const DESKTOP_ID: &str = "local_11111111-1111-4111-8111-111111111111";

    fn source() -> super::super::SourceSession {
        super::super::SourceSession {
            cli_id: CLI_ID.into(),
            cwd: r"D:\项目\demo".into(),
            title: "继续已有工作".into(),
            created_at: 1_700_000_000_000,
            last_activity_at: 1_700_000_001_000,
            is_archived: false,
        }
    }

    #[test]
    fn native_adoption_preserves_identity_and_times_without_pending_execution() {
        let source = source();
        let before = chrono::Utc::now().timestamp_millis();
        let pointer = native_pointer(&source, DESKTOP_ID, CLI_ID);
        assert_eq!(pointer["sessionId"], DESKTOP_ID);
        assert_eq!(pointer["cliSessionId"], CLI_ID);
        assert_eq!(pointer["cwd"], source.cwd);
        assert_eq!(pointer["originCwd"], source.cwd);
        assert_eq!(pointer["title"], source.title);
        assert_eq!(pointer["createdAt"], source.created_at);
        assert_eq!(pointer["lastActivityAt"], source.last_activity_at);
        assert!(pointer["indexedAt"].as_i64().unwrap() >= before);
        assert_eq!(pointer["isArchived"], false);
        assert_eq!(pointer["adoptedFromOtherSurface"], true);
        assert_eq!(pointer.as_object().unwrap().len(), 10);
        for field in [
            "importedFrom",
            "resumeConfirmed",
            "messageBuffer",
            "pendingFirstStart",
            "query",
            "isRunning",
        ] {
            assert!(pointer.get(field).is_none());
        }
    }

    #[test]
    fn portable_adoption_keeps_archive_and_title_but_drops_other_tenancy_state() {
        let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
        pointer["isArchived"] = true.into();
        pointer["originCwd"] = r"D:\项目\original".into();
        pointer["indexedAt"] = 10.into();
        for field in [
            "emailAddress",
            "envScopeId",
            "homeTenancy",
            "remoteMcpServersConfig",
            "sessionPermissionUpdates",
            "alwaysAllowedReasons",
            "permissionMode",
            "model",
            "effort",
            "pendingFirstStart",
            "armedWorkAtQuit",
            "interruptedByQuitAt",
            "contextRecovery",
            "cloudSessionId",
            "bridgeSessionIds",
            "scheduledTaskId",
            "spawnedFrom",
            "promptAppendSnapshot",
            "toolSurfaceSnapshot",
            "stagedTranscriptPath",
            "importedFrom",
            "resumeConfirmed",
        ] {
            pointer[field] = json!({"foreign": true});
        }
        let portable = portable_pointer(&pointer).unwrap();
        assert_eq!(portable["isArchived"], true);
        assert_eq!(portable["originCwd"], r"D:\项目\original");
        assert_eq!(portable["title"], pointer["title"]);
        assert_eq!(portable["createdAt"], pointer["createdAt"]);
        assert_eq!(portable["lastActivityAt"], pointer["lastActivityAt"]);
        assert_ne!(portable["indexedAt"], pointer["indexedAt"]);
        assert_eq!(portable["adoptedFromOtherSurface"], true);
        assert_eq!(portable.as_object().unwrap().len(), 10);
        for field in pointer.as_object().unwrap().keys() {
            if field != "indexedAt" && portable.get(field).is_some() {
                assert_eq!(portable[field], pointer[field]);
            }
        }
    }

    #[test]
    fn importing_archived_source_keeps_it_archived() {
        let mut source = source();
        source.is_archived = true;
        assert_eq!(
            native_pointer(&source, DESKTOP_ID, CLI_ID)["isArchived"],
            true
        );
    }

    #[test]
    fn remote_records_are_not_reinterpreted_as_host_sessions() {
        for (field, value) in [
            ("sshConfig", json!({"sshHost": "fixture"})),
            ("wslConfig", json!({"distro": "fixture"})),
            ("violinBow", Value::Bool(true)),
        ] {
            let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
            pointer[field] = value;
            assert!(portable_pointer(&pointer).is_none());
        }
    }

    #[test]
    fn invalid_identity_missing_time_or_invalid_archive_state_preserves_source() {
        for (field, value) in [
            ("sessionId", json!(CLI_ID)),
            ("sessionId", json!("local_invalid")),
            ("cliSessionId", json!("../foreign")),
            ("cwd", json!(" ")),
            ("createdAt", Value::Null),
            ("lastActivityAt", json!("1700000001000")),
            ("isArchived", json!("true")),
        ] {
            let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
            pointer[field] = value;
            let original = pointer.clone();
            assert!(portable_pointer(&pointer).is_none(), "{field}");
            assert_eq!(pointer, original);
        }
    }

    #[test]
    fn absent_native_optional_fields_use_safe_local_defaults() {
        let pointer = json!({
            "sessionId": DESKTOP_ID,
            "cliSessionId": CLI_ID,
            "cwd": "/fixture/project",
            "createdAt": 1_700_000_000_000i64,
            "lastActivityAt": 1_700_000_001_000i64,
            "sshConfig": null,
            "wslConfig": null,
            "violinBow": false,
        });
        let portable = portable_pointer(&pointer).unwrap();
        assert_eq!(portable["originCwd"], "/fixture/project");
        assert_eq!(portable["isArchived"], false);
        assert!(portable.get("title").is_none());
    }

    #[test]
    fn portable_lineage_keeps_local_transcript_claims_without_making_them_active() {
        let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
        let prior = "22222222-2222-4222-8222-222222222222";
        let cleared = "33333333-3333-4333-8333-333333333333";
        let unarchived = "44444444-4444-4444-8444-444444444444";
        pointer["priorCliSessionIds"] = json!([prior, "invalid", prior, CLI_ID]);
        pointer["preClearCliSessionId"] = cleared.into();
        pointer["unarchivedCliSessionId"] = unarchived.into();
        let portable = portable_pointer(&pointer).unwrap();
        assert_eq!(portable["cliSessionId"], CLI_ID);
        assert_eq!(
            portable["priorCliSessionIds"],
            json!([prior, cleared, unarchived])
        );
        assert!(portable.get("preClearCliSessionId").is_none());
        assert!(portable.get("unarchivedCliSessionId").is_none());
    }

    #[test]
    fn archived_runtime_fallback_resumes_the_unarchived_handle_but_not_cleared_history() {
        let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
        pointer["cliSessionId"] = Value::Null;
        pointer["isArchived"] = true.into();
        pointer["preClearCliSessionId"] = CLI_ID.into();
        assert!(portable_pointer(&pointer).is_none());
        let unarchived = "22222222-2222-4222-8222-222222222222";
        pointer["unarchivedCliSessionId"] = unarchived.into();
        let portable = portable_pointer(&pointer).unwrap();
        assert_eq!(portable["cliSessionId"], unarchived);
        assert_eq!(portable["priorCliSessionIds"], json!([CLI_ID]));
        assert_eq!(portable["isArchived"], true);
    }

    #[test]
    fn portable_lineage_matches_native_retention_of_the_last_two_hundred_ids() {
        let mut pointer = native_pointer(&source(), DESKTOP_ID, CLI_ID);
        let ids: Vec<_> = (1..=205)
            .map(|id| uuid::Uuid::from_u128(id).to_string())
            .collect();
        pointer["priorCliSessionIds"] = json!(ids);
        let portable = portable_pointer(&pointer).unwrap();
        assert_eq!(portable["priorCliSessionIds"], json!(&ids[5..]));
    }
}
