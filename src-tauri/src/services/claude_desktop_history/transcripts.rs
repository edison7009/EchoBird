use super::SourceSession;
use serde_json::Value;
use std::io::BufRead;
use std::path::{Path, PathBuf};

pub(super) struct Transcript {
    pub(super) source: SourceSession,
    pub(super) desktop: bool,
}

// Match Desktop's JavaScript /[^a-zA-Z0-9]/g, including UTF-16 surrogate pairs.
pub(super) fn project_slug(cwd: &str) -> Option<String> {
    let slug: String = cwd
        .encode_utf16()
        .map(|c| match c {
            48..=57 | 65..=90 | 97..=122 => char::from_u32(c as u32).unwrap(),
            _ => '-',
        })
        .collect();
    (!slug.is_empty() && slug.len() <= 200).then_some(slug)
}

pub(super) fn path(home: &Path, cwd: &str, cli_id: &str) -> Option<PathBuf> {
    Some(
        home.join("projects")
            .join(project_slug(cwd)?)
            .join(format!("{cli_id}.jsonl")),
    )
}

pub(super) fn scan(home: &Path) -> Result<Vec<Transcript>, String> {
    let mut transcripts = Vec::new();
    for file in files(home)? {
        match read(&file) {
            Ok(Some(transcript)) => transcripts.push(transcript),
            Ok(None) => {}
            Err(_) => log::warn!("[ClaudeHistory] skipped an unreadable or changing transcript"),
        }
    }
    Ok(transcripts)
}

pub(super) fn files(home: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for project in super::entries(&home.join("projects"))? {
        if !super::regular_directory(&project) {
            continue;
        }
        for file in super::entries(&project)? {
            if file.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            if super::regular_file(&file) {
                files.push(file);
            }
        }
    }
    Ok(files)
}

pub(super) fn read(file: &Path) -> Result<Option<Transcript>, String> {
    let Some(cli_id) = file.file_stem().and_then(|s| s.to_str()) else {
        return Ok(None);
    };
    if uuid::Uuid::parse_str(cli_id).is_err() || !super::regular_file(file) {
        return Ok(None);
    }
    let before = std::fs::metadata(file).map_err(|_| "Cannot read Claude history.")?;
    let mut cwd = String::new();
    let mut title = String::new();
    let mut custom_title = None;
    let mut created = i64::MAX;
    let mut activity = i64::MIN;
    let mut desktop = false;
    let mut user = false;
    let mut matching_id = false;
    let reader = std::io::BufReader::new(
        std::fs::File::open(file).map_err(|_| "Cannot read Claude history.")?,
    );
    for line in reader.lines() {
        let line = line.map_err(|_| "Cannot read Claude history.")?;
        if line.trim().is_empty() {
            continue;
        }
        // Incomplete tails and corrupt rows must not turn into a partial import.
        let value: Value =
            serde_json::from_str(&line).map_err(|_| "Claude history is incomplete.")?;
        if value["isSidechain"] == true {
            return Ok(None);
        }
        matching_id |= value["sessionId"].as_str() == Some(cli_id);
        desktop |= matches!(
            value["entrypoint"].as_str(),
            Some("claude-desktop" | "claude-desktop-3p")
        );
        if let Some(path) = value["cwd"]
            .as_str()
            .filter(|s| cwd.is_empty() && !s.is_empty())
        {
            cwd = path.to_owned();
        }
        if value["type"] == "custom-title" {
            custom_title = value["customTitle"].as_str().map(str::to_owned);
        }
        let role = value["message"]["role"].as_str();
        if !matches!(role, Some("user" | "assistant")) {
            continue;
        }
        if let Some(timestamp) = timestamp(&value["timestamp"]) {
            created = created.min(timestamp);
            activity = activity.max(timestamp);
        }
        if role == Some("user") && value["isMeta"] != true {
            let content = &value["message"]["content"];
            let text = text(content);
            let attachment = content.as_array().is_some_and(|blocks| {
                blocks
                    .iter()
                    .any(|b| matches!(b["type"].as_str(), Some("image" | "document")))
            });
            if !text.trim().is_empty() || attachment {
                user = true;
                if title.is_empty() {
                    title = text
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(100)
                        .collect();
                }
            }
        }
    }
    let after = std::fs::metadata(file).map_err(|_| "Cannot read Claude history.")?;
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return Err("Claude history changed while being read.".into());
    }
    if !matching_id || !user || !Path::new(&cwd).is_absolute() || created == i64::MAX {
        return Ok(None);
    }
    Ok(Some(Transcript {
        source: SourceSession {
            cli_id: cli_id.into(),
            cwd,
            title: custom_title
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    if title.is_empty() {
                        "Claude Code".into()
                    } else {
                        title
                    }
                }),
            created_at: created,
            last_activity_at: activity,
            is_archived: false,
        },
        desktop,
    }))
}

fn timestamp(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        chrono::DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()
            .map(|t| t.timestamp_millis())
    })
}

fn text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.into();
    }
    value
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    (block["type"] == "text")
                        .then(|| block["text"].as_str())
                        .flatten()
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_matches_desktop_utf16_and_rejects_native_unsupported_length() {
        assert_eq!(project_slug(r"C:\项目\demo😀").unwrap(), "C-----demo--");
        assert!(project_slug(&"a".repeat(201)).is_none());
        assert_eq!(project_slug("/tmp/demo").unwrap(), "-tmp-demo");
    }
}
