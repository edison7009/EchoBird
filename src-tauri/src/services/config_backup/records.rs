use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

#[derive(Clone, Copy)]
pub(super) enum Layout {
    Map,
    Wrapper,
    Vector,
    Directory,
    YamlDirectory,
}
pub(super) struct Store {
    pub provider: &'static str,
    pub layout: Layout,
}
pub(super) const STORES: &[Store] = &[
    Store {
        provider: "manus",
        layout: Layout::Map,
    },
    Store {
        provider: "cue",
        layout: Layout::Map,
    },
    Store {
        provider: "codex",
        layout: Layout::Directory,
    },
    Store {
        provider: "claudecode",
        layout: Layout::Directory,
    },
    Store {
        provider: "workbuddy",
        layout: Layout::Directory,
    },
    Store {
        provider: "copilot",
        layout: Layout::Directory,
    },
    Store {
        provider: "grok",
        layout: Layout::Directory,
    },
    Store {
        provider: "dsh",
        layout: Layout::YamlDirectory,
    },
    Store {
        provider: "commandcode",
        layout: Layout::Vector,
    },
    Store {
        provider: "devin",
        layout: Layout::Wrapper,
    },
    Store {
        provider: "claudedesktop",
        layout: Layout::Map,
    },
    Store {
        provider: "antigravity",
        layout: Layout::Map,
    },
    Store {
        provider: "cursor",
        layout: Layout::Map,
    },
    Store {
        provider: "grokbot",
        layout: Layout::Map,
    },
    Store {
        provider: "trae",
        layout: Layout::Map,
    },
    Store {
        provider: "traecn",
        layout: Layout::Map,
    },
    Store {
        provider: "qoder",
        layout: Layout::Map,
    },
    Store {
        provider: "qodercn",
        layout: Layout::Map,
    },
    Store {
        provider: "qwenwork",
        layout: Layout::Map,
    },
    Store {
        provider: "qwenworkcn",
        layout: Layout::Map,
    },
    Store {
        provider: "doubaowork",
        layout: Layout::Map,
    },
    Store {
        provider: "dola",
        layout: Layout::Map,
    },
    Store {
        provider: "dumate",
        layout: Layout::Map,
    },
    Store {
        provider: "minimax",
        layout: Layout::Map,
    },
    Store {
        provider: "cline",
        layout: Layout::Map,
    },
    Store {
        provider: "kiro",
        layout: Layout::Map,
    },
    Store {
        provider: "kilo",
        layout: Layout::Map,
    },
    Store {
        provider: "kimi",
        layout: Layout::Map,
    },
    Store {
        provider: "zcode",
        layout: Layout::Map,
    },
    Store {
        provider: "opendesign",
        layout: Layout::Map,
    },
    Store {
        provider: "codearts",
        layout: Layout::Map,
    },
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Record {
    pub kind: String,
    pub id: String,
    pub data: Value,
    pub device_bound: bool,
}

pub(super) fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("backupError.read".into()),
    };
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > super::format::LIMIT
    {
        return Err("backupError.size".into());
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| "backupError.read")?
        .take(super::format::LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "backupError.read")?;
    if bytes.len() as u64 > super::format::LIMIT {
        return Err("backupError.size".into());
    }
    Ok(Some(bytes))
}
pub(super) fn json_at(path: &Path) -> Result<Option<Value>, String> {
    read(path)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(|_| "backupError.format".into()))
        .transpose()
}
pub(super) fn directory_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(super) fn map_secrets(
    value: &mut Value,
    export: bool,
    crypt: &impl Fn(&str) -> Result<String, String>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("backupError.format".into());
    }
    match value {
        Value::String(s) if export && s.starts_with("enc:v1:") => {
            *value = json!({"$echobirdSecret":crypt(s)?})
        }
        Value::Object(m) if !export && m.len() == 1 && m.contains_key("$echobirdSecret") => {
            *value = Value::String(crypt(
                m["$echobirdSecret"].as_str().ok_or("backupError.format")?,
            )?)
        }
        Value::Object(m) => {
            for v in m.values_mut() {
                map_secrets(v, export, crypt, depth + 1)?;
            }
        }
        Value::Array(a) => {
            for v in a {
                map_secrets(v, export, crypt, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(super) fn validate_row(provider: &str, value: &Value) -> Result<(), String> {
    use super::super::*;
    match provider {
        "antigravity" => antigravity_accounts::validate_backup_record(value),
        "claudedesktop" => claude_desktop_accounts::validate_backup_record(value),
        "copilot" => copilot_accounts::validate_backup_record(value),
        "cursor" => cursor_accounts::validate_backup_record(value),
        "grokbot" => grok_bot_accounts::validate_backup_record(value),
        "commandcode" => commandcode_accounts::validate_backup_record(value),
        "devin" => devin_accounts::validate_backup_record(value),
        "trae" | "traecn" => trae_accounts::validate_backup_record(value),
        "qoder" | "qodercn" => qoder_accounts::validate_backup_record(value),
        "qwenwork" | "qwenworkcn" => qwenwork_accounts::validate_backup_record(value),
        "doubaowork" | "dola" => doubao_accounts::validate_backup_record(value),
        "dumate" => dumate_accounts::validate_backup_record(value),
        "minimax" => minimax_accounts::validate_backup_record(value),
        "cline" => cline_accounts::validate_backup_record(value),
        "kiro" => kiro_accounts::validate_backup_record(value),
        "kilo" => kilo_accounts::validate_backup_record(value),
        "kimi" => kimi_accounts::validate_backup_record(value),
        "zcode" => zcode_accounts::validate_backup_record(value),
        "opendesign" => opendesign_accounts::validate_backup_record(value),
        "manus" => manus_accounts::validate_backup_record(value),
        "cue" => cue_accounts::validate_backup_record(value),
        "dsh" => deepseek_accounts::validate_backup_record(value),
        "codex" => codex_accounts::validate_backup_record(value),
        "claudecode" => claude_code_accounts::validate_backup_record(value),
        "workbuddy" => workbuddy_accounts::validate_backup_record(value),
        "codearts" => codearts_accounts::validate_backup_record(value),
        "grok" => grok_accounts::validate_backup_record(value),
        _ => Err("backupError.format".into()),
    }
}
