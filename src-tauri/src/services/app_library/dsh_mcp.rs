use crate::services::{
    native_account_files::{self, DirectoryLock},
    tool_config_manager::dsh::{parse_yaml, CONFIG_LOCK},
};
use serde_json::{json, Map, Value};
use serde_yaml_ng::Value as Yaml;
use std::{collections::HashSet, fs, ops::Range, path::Path};
use yaml_rust2::scanner::{Scanner, Token, TokenType};

const MCP_CLIENT: &str = "@deepseek-ai/dsh-mcp-client";
const MANAGED_ID_PREFIX: &str = "echobird-mcp-";

#[derive(Default)]
pub(super) struct Servers {
    pub(super) entries: Map<String, Value>,
    pub(super) removable: HashSet<String>,
}

fn read_profile(path: &Path) -> Result<String, &'static str> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(_) => Err("readFailed"),
    }
}

fn plugins(profile: &Yaml) -> impl Iterator<Item = &Yaml> {
    profile
        .as_sequence()
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            std::iter::once(entry).chain(
                entry
                    .get("insert")
                    .and_then(Yaml::as_sequence)
                    .into_iter()
                    .flatten(),
            )
        })
}

fn parse_profile(text: &str) -> Result<Yaml, &'static str> {
    let value = parse_yaml(text.trim_start_matches('\u{feff}')).map_err(|_| "invalidConfig")?;
    if !value.is_null() && !value.is_sequence() {
        return Err("invalidConfig");
    }
    Ok(value)
}

fn inspect(profile: &Yaml) -> Result<Servers, &'static str> {
    let mut result = Servers::default();
    for plugin in plugins(profile) {
        if plugin["name"].as_str() != Some(MCP_CLIENT) {
            continue;
        }
        let native = &plugin["config"];
        let Some(name) = native["serverName"].as_str() else {
            continue;
        };
        let mut config = serde_json::to_value(native)
            .ok()
            .and_then(|value| match value {
                Value::Object(fields) => Some(fields),
                _ => None,
            })
            .unwrap_or_default();
        config.remove("serverName");
        match config.remove("transport").as_ref().and_then(Value::as_str) {
            Some("streamable-http") => {
                config.insert("type".into(), json!("http"));
            }
            Some("stdio") => {
                config.insert("type".into(), json!("stdio"));
            }
            Some(transport) => {
                config.insert("type".into(), json!(transport));
            }
            None => {}
        }
        if result
            .entries
            .insert(name.into(), Value::Object(config))
            .is_some()
        {
            return Err("invalidConfig");
        }
    }
    result.removable.extend(
        profile
            .as_sequence()
            .into_iter()
            .flatten()
            .filter_map(managed_server)
            .map(str::to_owned),
    );
    Ok(result)
}

fn managed_server(row: &Yaml) -> Option<&str> {
    if row.as_mapping()?.len() != 1 {
        return None;
    }
    let [entry] = row.get("insert")?.as_sequence()?.as_slice() else {
        return None;
    };
    let name = entry["config"]["serverName"].as_str()?;
    (entry["name"].as_str()? == MCP_CLIENT
        && entry["id"].as_str()?.strip_prefix(MANAGED_ID_PREFIX)? == name)
        .then_some(name)
}

// The profile can be reformatted by the native settings writer. Locate the
// top-level sequence entry by its scanner position and preserve neighboring rows.
fn row_range(text: &str, index: usize, expected: &Yaml) -> Result<Range<usize>, &'static str> {
    let offsets: Vec<_> = text.char_indices().map(|(index, _)| index).collect();
    let mut scanner = Scanner::new(text.chars());
    let mut starts = Vec::new();
    let mut end = text.len();
    while let Some(Token(mark, token)) = scanner.next_token().map_err(|_| "invalidConfig")? {
        if matches!(token, TokenType::BlockEntry) {
            let after_dash = offsets.get(mark.index()).copied().unwrap_or(text.len());
            let line_start = text[..after_dash]
                .rfind('\n')
                .map_or(0, |newline| newline + 1);
            if text[line_start..].starts_with('-') && starts.last() != Some(&line_start) {
                starts.push(line_start);
            }
        }
        if matches!(token, TokenType::DocumentEnd) {
            end = offsets[mark.index()];
        }
    }
    let start = *starts.get(index).ok_or("unsupported")?;
    let end = starts.get(index + 1).copied().unwrap_or(end);
    let row = &text[start..end];
    if parse_profile(row)?.as_sequence().map(Vec::as_slice) != Some(std::slice::from_ref(expected))
    {
        return Err("unsupported");
    }
    let len = row
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            *offset += line.len();
            Some((*offset, line.trim()))
        })
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .map(|(offset, _)| offset)
        .last()
        .ok_or("unsupported")?;
    Ok(start..start + len)
}

pub(super) fn read(path: &Path) -> Result<Servers, &'static str> {
    inspect(&parse_profile(&read_profile(path)?)?)
}

fn edit(
    path: &Path,
    change: impl FnOnce(&str, &Yaml, &Servers) -> Result<String, &'static str>,
) -> Result<(), &'static str> {
    let _guard = CONFIG_LOCK.lock().map_err(|_| "writeFailed")?;
    fs::create_dir_all(path.parent().ok_or("writeFailed")?).map_err(|_| "writeFailed")?;
    let _lock = DirectoryLock::acquire_existing(path).map_err(|_| "writeFailed")?;
    let original = read_profile(path)?;
    let body = original.strip_prefix('\u{feff}').unwrap_or(&original);
    let profile = parse_profile(body)?;
    let current = inspect(&profile)?;
    let mut updated = change(body, &profile, &current)?;
    inspect(&parse_profile(&updated)?)?;
    if read_profile(path)? != original {
        return Err("writeFailed");
    }
    if original.starts_with('\u{feff}') {
        updated.insert(0, '\u{feff}');
    }
    native_account_files::write_private_file(path, updated.as_bytes()).map_err(|_| "writeFailed")
}

pub(super) fn add(path: &Path, name: &str, definition: Value) -> Result<(), &'static str> {
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err("invalidName");
    }
    let mut spec = definition.as_object().cloned().ok_or("invalidConfig")?;
    let transport = match spec.remove("type").as_ref().and_then(Value::as_str) {
        Some("stdio") => "stdio",
        Some("http") => "streamable-http",
        _ => return Err("unsupported"),
    };
    spec.insert("serverName".into(), json!(name));
    spec.insert("transport".into(), json!(transport));
    let id = format!("{MANAGED_ID_PREFIX}{name}");
    let insert = json!({"insert": [{"id": id, "name": MCP_CLIENT, "config": spec}]});
    edit(path, |original, profile, current| {
        if current.entries.contains_key(name)
            || plugins(profile).any(|entry| entry["id"].as_str() == Some(id.as_str()))
        {
            return Err("exists");
        }
        let row = serde_json::to_string(&insert).map_err(|_| "invalidConfig")?;
        let eol = if original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut text = if profile.as_sequence().is_some_and(Vec::is_empty) {
            // The native empty template puts a standalone `[]` after its comments.
            original
                .split_inclusive('\n')
                .filter(|line| line.trim() != "[]")
                .collect::<String>()
        } else {
            original.to_owned()
        };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push_str(eol);
        }
        text.push_str(&format!("- {row}{eol}"));
        Ok(text)
    })
}

pub(super) fn remove(path: &Path, name: &str) -> Result<(), &'static str> {
    edit(path, |original, profile, current| {
        if !current.entries.contains_key(name) {
            return Err("missing");
        }
        let (index, row) = profile
            .as_sequence()
            .into_iter()
            .flatten()
            .enumerate()
            .find(|(_, row)| managed_server(row) == Some(name))
            .ok_or("unsupported")?;
        let range = row_range(original, index, row)?;
        let mut updated = original.to_owned();
        let remaining = profile.as_sequence().map(Vec::len).unwrap_or_default() - 1;
        let replacement = if remaining != 0 {
            ""
        } else if original.contains("\r\n") {
            "[]\r\n"
        } else {
            "[]\n"
        };
        updated.replace_range(range, replacement);
        if parse_profile(&updated)?
            .as_sequence()
            .map(Vec::len)
            .unwrap_or_default()
            != remaining
        {
            return Err("invalidConfig");
        }
        Ok(updated)
    })
}
