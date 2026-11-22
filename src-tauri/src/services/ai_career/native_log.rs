use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

use super::{make_title, mtime_millis, session_model_id, SavedSession, TokenUsage};

#[derive(Default)]
pub(super) struct Log {
    id: Option<String>,
    cwd: String,
    title: Option<String>,
    prompt: Option<String>,
    pub(super) modified_ms: Option<i64>,
    turns: u32,
    messages: u32,
    model: Option<String>,
    pub(super) samples: Vec<(Option<i64>, TokenUsage)>,
    pub(super) incomplete: bool,
}

impl Log {
    pub(super) fn session(&self, path: &Path, tool: &str) -> Option<SavedSession> {
        if self.turns == 0 {
            return None;
        }
        let id = self.id.as_deref().or_else(|| path.file_stem()?.to_str())?;
        let mut usage = TokenUsage::default();
        for (_, sample) in &self.samples {
            usage.add(*sample);
        }
        Some(SavedSession {
            id: format!("{tool}:{id}"),
            name: self
                .title
                .as_deref()
                .or(self.prompt.as_deref())
                .unwrap_or(id)
                .into(),
            tool: tool.into(),
            cwd: self.cwd.clone(),
            session_token: Some(id.to_string()),
            saved_at: self
                .modified_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| mtime_millis(path)),
            file_path: Some(path.to_string_lossy().into_owned()),
            turn_count: Some(self.turns),
            model_id: self.model.clone(),
            message_count: Some(self.messages),
            input_tokens: usage.input,
            output_tokens: usage.output,
        })
    }
}

fn text(value: &Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub(super) fn timestamp(value: &Value) -> Option<i64> {
    value.as_i64().filter(|ms| *ms > 0).or_else(|| {
        let raw = value.as_str()?;
        let time = chrono::DateTime::parse_from_rfc3339(raw)
            .or_else(|_| chrono::DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f%:z"))
            .map(|date| date.timestamp_millis())
            .or_else(|_| {
                chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f")
                    .map(|date| date.and_utc().timestamp_millis())
            })
            .ok()?;
        (time > 0).then_some(time)
    })
}

pub(super) fn user_title(raw: &str, opening: &str, closing: &str) -> String {
    let prompt = raw.rsplit_once(opening).map_or(raw, |(_, tail)| {
        tail.split_once(closing).map_or(tail, |(prompt, _)| prompt)
    });
    make_title(prompt.trim())
}

fn read(path: &Path, mut visit: impl FnMut(&Value)) -> io::Result<bool> {
    let file = File::open(path)?;
    let mut incomplete = false;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str(&line) {
            Ok(value) => visit(&value),
            Err(_) => incomplete = true,
        }
    }
    Ok(incomplete)
}

pub(super) fn antigravity(path: &Path) -> io::Result<Log> {
    let mut log = Log {
        id: path
            .ancestors()
            .nth(3)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_string),
        ..Log::default()
    };
    let mut steps = HashMap::new();
    let malformed = read(path, |value| {
        let ts = timestamp(&value["created_at"]);
        log.modified_ms = log.modified_ms.max(ts);
        match value["type"].as_str() {
            Some("USER_INPUT") => {
                log.turns += 1;
                log.messages += 1;
                if log.prompt.is_none() {
                    log.prompt = text(&value["content"])
                        .map(|raw| user_title(raw, "<USER_REQUEST>", "</USER_REQUEST>"))
                        .filter(|title| !title.is_empty());
                }
            }
            Some("PLANNER_RESPONSE") => {
                if text(&value["content"]).is_some() {
                    log.messages += 1;
                }
                if value["status"]
                    .as_str()
                    .is_some_and(|status| status != "DONE")
                {
                    return;
                }
                log.model = session_model_id(value.get("model")).or(log.model.take());
                let usage = TokenUsage::read(value, "input_tokens", "output_tokens");
                if usage.input.is_none() || usage.output.is_none() {
                    log.incomplete = true;
                }
                if usage.input.is_some() || usage.output.is_some() {
                    let sample = (ts, usage);
                    if let Some(step) = value["step_index"].as_u64() {
                        steps.insert(step, sample);
                    } else {
                        log.samples.push(sample);
                    }
                }
            }
            _ => {}
        }
    })?;
    log.incomplete |= malformed || (log.turns > 0 && steps.is_empty() && log.samples.is_empty());
    log.samples.extend(steps.into_values());
    Ok(log)
}

pub(super) fn workbuddy(path: &Path) -> io::Result<Log> {
    let mut log = Log::default();
    // Tool calls may carry the final usage for the same provider message as an earlier text event.
    let mut requests = HashMap::new();
    let mut unmeasured = false;
    let malformed = read(path, |value| {
        let ts = timestamp(&value["timestamp"]);
        log.modified_ms = log.modified_ms.max(ts);
        if log.id.is_none() {
            log.id = text(&value["sessionId"]).map(str::to_string);
        }
        if log.cwd.is_empty() {
            log.cwd = text(&value["cwd"]).unwrap_or_default().into();
        }
        let kind = value["type"].as_str();
        let role = value["role"].as_str();
        let assistant = kind == Some("message") && role == Some("assistant");
        if kind == Some("ai-title") {
            if let Some(title) = text(&value["aiTitle"]) {
                log.title = Some(make_title(title));
            }
        } else if kind == Some("message") && role == Some("user") {
            log.messages += 1;
            log.turns += 1;
            if log.prompt.is_none() {
                let raw = value["content"].as_array().map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| text(&part["text"]))
                        .collect::<Vec<_>>()
                        .join("\n")
                });
                log.prompt = text(&value["content"])
                    .or(raw.as_deref())
                    .map(|prompt| user_title(prompt, "<user_query>", "</user_query>"))
                    .filter(|prompt| !prompt.is_empty());
            }
        } else if assistant {
            log.messages += 1;
        }
        if role == Some("user") {
            return;
        }
        let data = &value["providerData"];
        if let Some(model) = session_model_id(data.get("model"))
            .or_else(|| session_model_id(data.get("requestModelId")))
        {
            log.model = Some(model);
        }
        let raw = TokenUsage::read(&data["rawUsage"], "prompt_tokens", "completion_tokens");
        let has_raw = raw.input.is_some() || raw.output.is_some();
        let usage = if assistant && !has_raw {
            TokenUsage::read(&data["usage"], "inputTokens", "outputTokens")
        } else {
            raw
        };
        if let Some(id) = text(&data["messageId"]) {
            if assistant || has_raw {
                let sample = requests
                    .entry(id.to_string())
                    .or_insert((false, (ts, usage)));
                if has_raw || (!sample.0 && (usage.input.is_some() || usage.output.is_some())) {
                    *sample = (has_raw, (ts, usage));
                }
            }
        } else if usage.input.is_some() || usage.output.is_some() {
            log.samples.push((ts, usage));
        } else {
            unmeasured |= assistant;
        }
    })?;
    log.samples
        .extend(requests.into_values().map(|(_, sample)| sample));
    log.incomplete |= malformed || unmeasured || (log.turns > 0 && log.samples.is_empty());
    Ok(log)
}

pub(super) fn workbuddy_deleted(db: &Path) -> rusqlite::Result<std::collections::HashSet<String>> {
    use rusqlite::{Connection, OpenFlags};

    if !db.is_file() {
        return Ok(Default::default());
    }
    let conn = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(std::time::Duration::from_millis(500))?;
    if !super::sqlite_has_column(&conn, "sessions", "deleted_at")? {
        return Ok(Default::default());
    }
    let mut stmt = conn.prepare("SELECT id FROM sessions WHERE deleted_at IS NOT NULL")?;
    let deleted = stmt.query_map([], |row| row.get(0))?;
    deleted.collect()
}
