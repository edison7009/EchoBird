use super::*;
use serde_json::{json, Value};

fn write_log(path: &Path, records: &[Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut text = records
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    std::fs::write(path, text).unwrap();
}

fn antigravity_log(root: &Path, id: &str) -> PathBuf {
    root.join("brain")
        .join(id)
        .join(".system_generated/logs/transcript_full.jsonl")
}

#[test]
fn native_families_without_history_return_empty_success() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let count_cache = home.join("count.json");

    for directory in [
        ".gemini/antigravity/brain",
        ".gemini/antigravity-cli/brain",
        ".workbuddy/projects",
        ".workbuddy-ai/projects",
    ] {
        std::fs::create_dir_all(home.join(directory)).unwrap();
    }

    for family in [Family::Antigravity, Family::WorkBuddy] {
        let family = Family::from_id(family.as_id()).unwrap();
        assert!(family_history_at_checked(home, family, 0, 20)
            .unwrap()
            .is_empty());
        assert!(message_heatmap_at_checked(home, Some(family), &count_cache)
            .unwrap()
            .is_empty());
        assert_eq!(
            estimate_token_bytes_at_checked(home, Some(family)).unwrap(),
            0
        );
        let daily = daily_tokens::daily_tokens_at(home, Some(family), SystemTime::now());
        assert!(daily.entries.is_empty());
        assert!(daily.unavailable_families.is_empty());
    }
}

#[test]
fn antigravity_family_merges_indexes_with_one_row_per_conversation() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let root = Family::Antigravity.root(home);
    let desktop = root.join("antigravity");
    let cli = root.join("antigravity-cli");
    let now = chrono::Utc::now();
    let recent = now - chrono::Duration::hours(1);
    let earlier = now - chrono::Duration::hours(3);
    let workspace = home.join("Sample Workspace");
    let uris = json!([url::Url::from_file_path(&workspace).unwrap().as_str()]).to_string();

    for path in [&desktop, &cli] {
        std::fs::create_dir_all(path).unwrap();
    }
    let conn = rusqlite::Connection::open(desktop.join("conversation_summaries.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
            conversation_id TEXT, title TEXT, preview TEXT, last_modified_time TEXT,
            workspace_uris TEXT, step_count INTEGER DEFAULT 2, parent_conversation_id TEXT,
            nesting_depth INTEGER DEFAULT 0, app_data_dir TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries(conversation_id,title,last_modified_time,workspace_uris)
         VALUES ('desktop-only','Desktop conversation',?1,?2), ('copied','Older copy',?1,?2)",
        rusqlite::params![earlier.to_rfc3339(), uris],
    ).unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries(conversation_id,title,last_modified_time,parent_conversation_id)
         VALUES ('worker','Background worker',?1,'desktop-only')",
        [recent.to_rfc3339()],
    ).unwrap();
    conn.execute(
        "UPDATE conversation_summaries SET app_data_dir = 'antigravity-cli' WHERE conversation_id = 'copied'",
        [],
    ).unwrap();
    drop(conn);
    let conn = rusqlite::Connection::open(cli.join("conversation_summaries.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries (
        conversation_id TEXT, title TEXT, preview TEXT, last_modified_time TEXT, workspace_uris TEXT
    );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES ('copied','','<USER_REQUEST>CLI conversation</USER_REQUEST>metadata',?1,?2)",
        rusqlite::params![recent.format("%Y-%m-%d %H:%M:%S%.f%:z").to_string(), uris],
    ).unwrap();
    drop(conn);
    let transcript = antigravity_log(&cli, "copied");
    let timestamp = recent.to_rfc3339();
    write_log(
        &transcript,
        &[
            json!({"type":"USER_INPUT","content":"Prompt","created_at":timestamp}),
            json!({"type":"PLANNER_RESPONSE","content":"Response","status":"DONE","step_index":1,"input_tokens":21,"output_tokens":5,"created_at":timestamp}),
            json!({"type":"GENERIC","content":"Tool log","created_at":timestamp}),
            json!({"type":"PLANNER_RESPONSE","status":"DONE","step_index":3,"input_tokens":10,"output_tokens":2,"created_at":timestamp}),
        ],
    );

    let first = family_history_at_checked(home, Family::Antigravity, 0, 1).unwrap();
    assert_eq!(first[0].name, "CLI conversation");
    assert_eq!(first[0].tool, "antigravity");
    assert_eq!(first[0].cwd, workspace.to_string_lossy());
    assert_eq!(first[0].message_count, Some(2));
    assert_eq!(
        (first[0].input_tokens, first[0].output_tokens),
        (Some(31), Some(7))
    );
    let second = family_history_at_checked(home, Family::Antigravity, 1, 10).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].name, "Desktop conversation");
    assert_eq!(second[0].tool, "antigravitydesktop");
    assert_eq!(second[0].message_count, None);
    let heatmap =
        message_heatmap_at_checked(home, Some(Family::Antigravity), &home.join("count.json"))
            .unwrap();
    assert_eq!(
        heatmap.iter().map(|row| row.count).collect::<Vec<_>>(),
        vec![2, 0]
    );
    assert_eq!(
        estimate_token_bytes_at_checked(home, Some(Family::Antigravity)).unwrap(),
        transcript.metadata().unwrap().len()
    );
    let daily = daily_tokens::daily_tokens_at(home, Some(Family::Antigravity), now.into());
    assert_eq!(daily.entries.len(), 2);
    assert_eq!(daily.unavailable_families, ["antigravity"]);
}

#[test]
fn antigravity_family_reads_legacy_exports_without_a_summary_index() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let root = Family::Antigravity.root(home).join("antigravity");
    write_log(
        &antigravity_log(&root, "older-thread"),
        &[
            json!({"type":"USER_INPUT","content":"<USER_REQUEST>Legacy prompt</USER_REQUEST>","created_at":"2026-11-20T10:20:30Z"}),
        ],
    );
    write_log(
        &antigravity_log(&root, "background"),
        &[json!({"type":"GENERIC"})],
    );
    let rows = family_history_at_checked(home, Family::Antigravity, 0, 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session_token.as_deref(), Some("older-thread"));
    assert_eq!(rows[0].name, "Legacy prompt");
    assert_eq!(rows[0].tool, "antigravitydesktop");
}

#[test]
fn workbuddy_family_reads_both_editions_and_counts_each_request_once() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let now = chrono::Utc::now();
    let timestamp = now.timestamp_millis();
    let domestic = home.join(".workbuddy");
    let project = domestic.join("projects/local");
    let international = home.join(".workbuddy-ai/projects/international");
    let prompt = json!({
        "type":"message","role":"user","cwd":"C:\\Projects\\Example",
        "timestamp":timestamp,"content":[{"type":"input_text","text":"Context <user_query>Actual prompt</user_query>"}]
    });
    let domestic_log = project.join("local-1.jsonl");
    let international_log = international.join("international-1.jsonl");
    write_log(
        &domestic_log,
        &[
            prompt.clone(),
            json!({"type":"message","role":"assistant","timestamp":timestamp,"providerData":{
                "messageId":"same-call","usage":{"inputTokens":3,"outputTokens":2}
            }}),
            json!({"type":"function_call","timestamp":timestamp,"providerData":{
                "messageId":"same-call","model":"Example Model","rawUsage":{"prompt_tokens":17,"completion_tokens":4}
            }}),
            json!({"type":"ai-title","aiTitle":"Domestic title","timestamp":timestamp}),
        ],
    );
    write_log(
        &international_log,
        &[
            prompt.clone(),
            json!({"type":"message","role":"assistant","timestamp":timestamp,"providerData":{
                "messageId":"same-call","usage":{"inputTokens":11,"outputTokens":3}
            }}),
        ],
    );
    write_log(
        &project.join("local-1/subagents/worker.jsonl"),
        std::slice::from_ref(&prompt),
    );
    write_log(&project.join("deleted.jsonl"), &[prompt]);
    let conn = rusqlite::Connection::open(domestic.join("workbuddy.db")).unwrap();
    conn.execute_batch(
        "CREATE TABLE sessions(id TEXT, deleted_at INTEGER);
        INSERT INTO sessions VALUES ('local-1',NULL), ('deleted',1)",
    )
    .unwrap();
    drop(conn);

    let rows = family_history_at_checked(home, Family::WorkBuddy, 0, 10).unwrap();
    assert_eq!(rows.len(), 2);
    let domestic = rows.iter().find(|row| row.tool == "workbuddy").unwrap();
    let international = rows.iter().find(|row| row.tool == "workbuddyai").unwrap();
    assert_eq!(domestic.name, "Domestic title");
    assert_eq!(domestic.model_id.as_deref(), Some("Example Model"));
    assert_eq!(domestic.input_tokens, Some(17));
    assert_eq!(domestic.message_count, Some(2));
    assert_eq!(international.name, "Actual prompt");
    assert_eq!(international.cwd, "C:\\Projects\\Example");
    let heatmap =
        message_heatmap_at_checked(home, Some(Family::WorkBuddy), &home.join("count.json"))
            .unwrap();
    assert_eq!(
        heatmap.iter().map(|row| row.count).collect::<Vec<_>>(),
        vec![2, 2]
    );
    assert_eq!(
        estimate_token_bytes_at_checked(home, Some(Family::WorkBuddy)).unwrap(),
        domestic_log.metadata().unwrap().len() + international_log.metadata().unwrap().len()
    );
    let daily = daily_tokens::daily_tokens_at(home, Some(Family::WorkBuddy), now.into());
    assert_eq!(daily.entries.len(), 2);
    assert_eq!(
        daily
            .entries
            .iter()
            .map(|entry| entry.input_tokens)
            .sum::<u64>(),
        28
    );
    assert!(daily.unavailable_families.is_empty());
}
