use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use super::{make_title, native_log, path_metadata, read_directory, Family, SavedSession};

pub(super) const EDITION_DIRS: [&str; 2] = ["antigravity", "antigravity-cli"];

pub(super) struct Session {
    id: String,
    tool: &'static str,
    pub(super) modified_ms: i64,
    title: String,
    cwd: String,
    pub(super) transcript: Option<PathBuf>,
    pub(super) file: Option<PathBuf>,
}

impl Session {
    pub(super) fn save(&self, log: Option<SavedSession>) -> SavedSession {
        let mut session = log.unwrap_or_else(|| SavedSession {
            id: String::new(),
            name: String::new(),
            tool: String::new(),
            cwd: String::new(),
            session_token: None,
            saved_at: String::new(),
            file_path: None,
            turn_count: None,
            model_id: None,
            message_count: None,
            input_tokens: None,
            output_tokens: None,
        });
        session.id = format!("{}:{}", Family::Antigravity.as_id(), self.id);
        session.tool = self.tool.into();
        if !self.title.is_empty() {
            session.name.clone_from(&self.title);
        } else if session.name.is_empty() {
            session.name.clone_from(&self.id);
        }
        session.cwd.clone_from(&self.cwd);
        session.saved_at = self.modified_ms.to_string();
        session.session_token = Some(self.id.clone());
        session.file_path = self
            .file
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        session
    }
}

fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

fn transcript(root: &Path, id: &str) -> PathBuf {
    root.join("brain")
        .join(id)
        .join(".system_generated/logs/transcript_full.jsonl")
}

fn workspace(uris: &str) -> String {
    let uris: Vec<String> = serde_json::from_str(uris).unwrap_or_default();
    uris.iter()
        .find_map(|uri| url::Url::parse(uri).ok()?.to_file_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn edition_tool(edition: &str) -> &'static str {
    if edition == "antigravity-cli" {
        "antigravity"
    } else {
        "antigravitydesktop"
    }
}

fn edition_index(data_root: &Path, edition: &str) -> Result<Vec<Session>, String> {
    let root = data_root.join(edition);
    let db = root.join("conversation_summaries.db");
    if !path_metadata(&db)
        .map_err(|error| error.to_string())?
        .is_some_and(|meta| meta.is_file())
    {
        return exported_sessions(&root, edition_tool(edition));
    }

    let read = || -> rusqlite::Result<Option<Vec<Session>>> {
        let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(Duration::from_millis(500))?;
        let mut schema = conn.prepare("PRAGMA table_info(conversation_summaries)")?;
        let columns = schema
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
        if columns.is_empty() {
            return Ok(None);
        }
        let optional = |column: &'static str, otherwise| {
            if columns.contains(column) {
                column
            } else {
                otherwise
            }
        };
        let sql = format!(
            "SELECT conversation_id, title, preview, last_modified_time, workspace_uris, {} \
             FROM conversation_summaries WHERE COALESCE({}, 1) > 0 AND COALESCE({}, 0) = 0 \
             AND COALESCE({}, '') = '' AND COALESCE({}, '') = ''",
            optional("app_data_dir", "NULL"),
            optional("step_count", "NULL"),
            optional("nesting_depth", "NULL"),
            optional("agent_name", "NULL"),
            optional("parent_conversation_id", "NULL"),
        );
        let mut query = conn.prepare(&sql)?;
        let mut rows = query.query([])?;
        let mut sessions = Vec::new();
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let owner: Option<String> = row.get(5)?;
            if !safe_id(&id) {
                continue;
            }
            let title: Option<String> = row.get(1)?;
            let preview: Option<String> = row.get(2)?;
            let modified: Option<String> = row.get(3)?;
            let uris: Option<String> = row.get(4)?;
            let transcript = transcript(&root, &id);
            let transcript = transcript.is_file().then_some(transcript);
            let file = transcript.clone().or_else(|| {
                let directory = root.join("conversations");
                ["db", "pb"]
                    .into_iter()
                    .map(|extension| directory.join(format!("{id}.{extension}")))
                    .find(|file| file.is_file())
            });
            let source = owner
                .as_deref()
                .filter(|owner| file.is_none() && EDITION_DIRS.contains(owner))
                .unwrap_or(edition);
            let Some(modified_ms) = modified
                .and_then(|date| native_log::timestamp(&date.into()))
                .or_else(|| {
                    file.as_deref()
                        .and_then(|path| super::mtime_millis(path).parse().ok())
                })
            else {
                continue;
            };
            let title = title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(make_title)
                .unwrap_or_else(|| {
                    native_log::user_title(
                        preview.as_deref().unwrap_or_default(),
                        "<USER_REQUEST>",
                        "</USER_REQUEST>",
                    )
                });
            if title.is_empty() && file.is_none() {
                continue;
            }
            sessions.push(Session {
                id,
                tool: edition_tool(source),
                modified_ms,
                title,
                cwd: workspace(uris.as_deref().unwrap_or_default()),
                transcript,
                file,
            });
        }
        Ok(Some(sessions))
    };
    match read().map_err(|error| error.to_string())? {
        Some(sessions) => Ok(sessions),
        None => exported_sessions(&root, edition_tool(edition)),
    }
}

fn exported_sessions(root: &Path, tool: &'static str) -> Result<Vec<Session>, String> {
    let Some(entries) = read_directory(&root.join("brain")).map_err(|error| error.to_string())?
    else {
        return Ok(Vec::new());
    };
    let mut sessions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let id = entry.file_name().to_string_lossy().into_owned();
        if !safe_id(&id) {
            continue;
        }
        let path = transcript(root, &id);
        let Some(log) =
            super::parse_session_cached_in(super::session_cache(), &path, Family::Antigravity)?
        else {
            continue;
        };
        let Ok(modified_ms) = log.saved_at.parse() else {
            continue;
        };
        sessions.push(Session {
            id,
            tool,
            modified_ms,
            title: log.name,
            cwd: String::new(),
            transcript: Some(path.clone()),
            file: Some(path),
        });
    }
    Ok(sessions)
}

pub(super) fn index(root: &Path) -> Result<Vec<Session>, String> {
    let mut sessions: HashMap<String, Session> = HashMap::new();
    for edition in EDITION_DIRS {
        for session in edition_index(root, edition)? {
            // An imported conversation with the same ID contributes once to the family.
            let replace = sessions.get(&session.id).is_none_or(|existing| {
                (session.transcript.is_some(), session.modified_ms)
                    > (existing.transcript.is_some(), existing.modified_ms)
            });
            if replace {
                sessions.insert(session.id.clone(), session);
            }
        }
    }
    let mut sessions: Vec<_> = sessions.into_values().collect();
    sessions.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(sessions)
}
