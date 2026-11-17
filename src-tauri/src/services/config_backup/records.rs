use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

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
    pub file: &'static str,
    pub layout: Layout,
}
pub(super) const STORES: &[Store] = &[
    Store {
        provider: "manus",
        file: "manus-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "cue",
        file: "cue-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "codex",
        file: "codex-accounts",
        layout: Layout::Directory,
    },
    Store {
        provider: "claudecode",
        file: "claude-code-accounts",
        layout: Layout::Directory,
    },
    Store {
        provider: "workbuddy",
        file: "workbuddy-accounts",
        layout: Layout::Directory,
    },
    Store {
        provider: "copilot",
        file: "copilot-accounts",
        layout: Layout::Directory,
    },
    Store {
        provider: "grok",
        file: "grok-accounts",
        layout: Layout::Directory,
    },
    Store {
        provider: "dsh",
        file: "deepseek-accounts",
        layout: Layout::YamlDirectory,
    },
    Store {
        provider: "commandcode",
        file: "commandcode-accounts.json",
        layout: Layout::Vector,
    },
    Store {
        provider: "devin",
        file: "devin-accounts.json",
        layout: Layout::Wrapper,
    },
    Store {
        provider: "claudedesktop",
        file: "claude-desktop-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "antigravity",
        file: "antigravity-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "cursor",
        file: "cursor-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "grokbot",
        file: "grokbot-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "trae",
        file: "trae-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "traecn",
        file: "traecn-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "qoder",
        file: "qoder-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "qodercn",
        file: "qodercn-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "qwenwork",
        file: "qwenwork-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "qwenworkcn",
        file: "qwenworkcn-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "doubaowork",
        file: "doubaowork-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "dola",
        file: "dola-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "dumate",
        file: "dumate-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "minimax",
        file: "minimax-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "cline",
        file: "cline-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "kiro",
        file: "kiro-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "kilo",
        file: "kilo-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "kimi",
        file: "kimi-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "zcode",
        file: "zcode-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "opendesign",
        file: "opendesign-accounts.json",
        layout: Layout::Map,
    },
    Store {
        provider: "codearts",
        file: "codearts-accounts.json",
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
pub(super) fn rows(root: &Path, store: &Store) -> Result<BTreeMap<String, Value>, String> {
    let path = root.join(store.file);
    match store.layout {
        Layout::Directory | Layout::YamlDirectory => {
            let mut rows = BTreeMap::new();
            let mut total_bytes = 0u64;
            if !path.exists() {
                return Ok(rows);
            }
            if fs::symlink_metadata(&path)
                .map_err(|_| "backupError.read")?
                .file_type()
                .is_symlink()
            {
                return Err("backupError.format".into());
            }
            for entry in fs::read_dir(&path).map_err(|_| "backupError.read")? {
                let path = entry.map_err(|_| "backupError.read")?.path();
                let extension = if matches!(store.layout, Layout::YamlDirectory) {
                    "yaml"
                } else {
                    "json"
                };
                if path.extension().and_then(|s| s.to_str()) != Some(extension) {
                    continue;
                }
                let id = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| directory_id(s))
                    .ok_or("backupError.format")?;
                let bytes = read(&path)?.ok_or("backupError.read")?;
                total_bytes += bytes.len() as u64;
                if total_bytes > super::format::LIMIT / 2 {
                    return Err("backupError.size".into());
                }
                let value = if matches!(store.layout, Layout::YamlDirectory) {
                    serde_yaml_ng::from_slice(&bytes).map_err(|_| "backupError.format")?
                } else {
                    serde_json::from_slice(&bytes).map_err(|_| "backupError.format")?
                };
                rows.insert(id.into(), value);
                if rows.len() > 10_000 {
                    return Err("backupError.size".into());
                }
            }
            Ok(rows)
        }
        layout => {
            let Some(value) = json_at(&path)? else {
                return Ok(BTreeMap::new());
            };
            let value = match layout {
                Layout::Wrapper | Layout::Vector => {
                    value.get("accounts").cloned().ok_or("backupError.format")?
                }
                _ => value,
            };
            if matches!(layout, Layout::Vector) {
                value
                    .as_array()
                    .ok_or("backupError.format")?
                    .iter()
                    .map(|row| {
                        Ok((
                            row["id"].as_str().ok_or("backupError.format")?.into(),
                            row.clone(),
                        ))
                    })
                    .collect()
            } else {
                serde_json::from_value(value).map_err(|_| "backupError.format".into())
            }
        }
    }
}
pub(super) fn identity<'a>(id: &'a str, row: &'a Value) -> (&'a str, &'a str) {
    let account = row
        .get("account")
        .or_else(|| row.get("summary"))
        .unwrap_or(row);
    let email = account["email"]
        .as_str()
        .or_else(|| account["name"].as_str())
        .or_else(|| row["email"].as_str())
        .unwrap_or_default();
    (id, email)
}
fn contains_native(value: &Value) -> bool {
    match value {
        Value::Object(m) => m.contains_key("$wbEncrypted") || m.values().any(contains_native),
        Value::Array(a) => a.iter().any(contains_native),
        _ => false,
    }
}
pub(super) fn device_bound(provider: &str, row: &Value) -> bool {
    match provider {
        "claudedesktop" => row["snapshot"]["cookie_version"] != -1,
        "grokbot" => true,
        "cursor" => row["session"]
            .as_str()
            .is_some_and(|s| !s.starts_with("enc:v1:")),
        _ => contains_native(row),
    }
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
pub(super) fn clear_activation(row: &mut Value) {
    for key in ["active", "applied", "cli_applied"] {
        if row.get(key).is_some() {
            row[key] = json!(false);
        }
    }
    for key in ["account", "summary"] {
        if row[key].is_object() && row[key].get("active").is_some() {
            row[key]["active"] = json!(false);
        }
    }
    if row.get("activeTools").is_some() {
        row["activeTools"] = json!([]);
    }
    for key in ["native_pair", "nativePair"] {
        if row.get(key).is_some() {
            row[key] = Value::Null;
        }
    }
    if row.get("native_pairs").is_some() {
        row["native_pairs"] = json!({});
    }
}
pub(super) fn store_files(
    root: &Path,
    store: &Store,
    rows: &BTreeMap<String, Value>,
) -> Result<Vec<(PathBuf, Vec<u8>)>, String> {
    let encode = |value: &Value| {
        serde_json::to_vec_pretty(value).map_err(|_| String::from("backupError.format"))
    };
    match store.layout {
        Layout::Directory | Layout::YamlDirectory => rows
            .iter()
            .map(|(id, row)| {
                if !directory_id(id) {
                    return Err("backupError.format".into());
                }
                let yaml = matches!(store.layout, Layout::YamlDirectory);
                let bytes = if yaml {
                    serde_yaml_ng::to_string(row)
                        .map_err(|_| "backupError.format")?
                        .into_bytes()
                } else {
                    encode(row)?
                };
                Ok((
                    root.join(store.file)
                        .join(format!("{id}.{}", if yaml { "yaml" } else { "json" })),
                    bytes,
                ))
            })
            .collect(),
        Layout::Map => Ok(vec![(
            root.join(store.file),
            encode(&serde_json::to_value(rows).map_err(|_| "backupError.format")?)?,
        )]),
        Layout::Wrapper | Layout::Vector => {
            let path = root.join(store.file);
            let mut value = json_at(&path)?.unwrap_or(json!({"accounts":{}}));
            value["accounts"] = if matches!(store.layout, Layout::Vector) {
                Value::Array(rows.values().cloned().collect())
            } else {
                serde_json::to_value(rows).map_err(|_| "backupError.format")?
            };
            Ok(vec![(path, encode(&value)?)])
        }
    }
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
