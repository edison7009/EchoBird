use super::{children, Entry};
use crate::services::{
    native_account_files::{self, DirectoryLock},
    tool_config_manager::jsonc,
};
use jsonc_parser::{
    cst::{CstObject, CstRootNode},
    ParseOptions,
};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct Document {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
    root: CstObject,
    tree: CstRootNode,
}

fn documents(directory: &Path) -> Result<[Document; 2], &'static str> {
    let load = |name: &str| {
        let path = directory.join(name);
        let bytes = native_account_files::read_optional(&path).map_err(|_| "readFailed")?;
        let text = std::str::from_utf8(bytes.as_deref().unwrap_or(b"{}"))
            .map_err(|_| "invalidConfig")?
            .trim_start_matches('\u{feff}');
        let options = ParseOptions {
            allow_comments: true,
            allow_trailing_commas: true,
            ..Default::default()
        };
        let tree = CstRootNode::parse(text, &options).map_err(|_| "invalidConfig")?;
        let root = tree.object_value_or_create().ok_or("invalidConfig")?;
        Ok::<_, &'static str>(Document {
            path,
            bytes,
            root,
            tree,
        })
    };
    Ok([load("opencode.json")?, load("opencode.jsonc")?])
}

pub(super) struct Server {
    document: usize,
    legacy: bool,
    pub(super) value: Value,
    pub(super) duplicate: bool,
}

fn is_server(value: &Value) -> bool {
    matches!(value["type"].as_str(), Some("local" | "remote"))
}

fn configured_servers(documents: &[Document]) -> Result<BTreeMap<String, Server>, &'static str> {
    let mut servers = BTreeMap::new();
    for (document, file) in documents.iter().enumerate() {
        let Some(mcp) = file.root.get("mcp") else {
            continue;
        };
        let value = mcp.to_serde_value().ok_or("invalidConfig")?;
        let fields = value.as_object().ok_or("invalidConfig")?;
        let mut insert = |name: &String, value: &Value, legacy| {
            let duplicate = servers.contains_key(name);
            let mut value = value.clone();
            if legacy {
                let enabled = value
                    .as_object_mut()
                    .and_then(|fields| fields.remove("enabled"));
                if enabled == Some(json!(false)) {
                    value["disabled"] = json!(true);
                }
            }
            servers.insert(
                name.clone(),
                Server {
                    document,
                    legacy,
                    value,
                    duplicate,
                },
            );
        };
        for (name, value) in fields {
            if is_server(value) {
                insert(name, value, true);
            }
        }
        if let Some(group) = fields.get("servers").filter(|value| !is_server(value)) {
            for (name, value) in group.as_object().ok_or("invalidConfig")? {
                if is_server(value) {
                    insert(name, value, false);
                }
            }
        }
    }
    Ok(servers)
}

pub(super) fn servers(directory: &Path) -> Result<BTreeMap<String, Server>, &'static str> {
    configured_servers(&documents(directory)?)
}

pub(super) fn portable(mut fields: Map<String, Value>) -> Result<Value, &'static str> {
    match fields.remove("type").as_ref().and_then(Value::as_str) {
        Some("local") => {
            let command = fields.remove("command").ok_or("invalidConfig")?;
            let [program, args @ ..] = command.as_array().ok_or("invalidConfig")?.as_slice() else {
                return Err("invalidConfig");
            };
            fields.insert("type".into(), json!("stdio"));
            fields.insert("command".into(), program.clone());
            fields.insert("args".into(), json!(args));
            if let Some(environment) = fields.remove("environment") {
                fields.insert("env".into(), environment);
            }
        }
        Some("remote") => {
            fields.insert("type".into(), json!("http"));
        }
        _ => return Err("unsupported"),
    }
    Ok(Value::Object(fields))
}

pub(super) enum Change {
    Add(Value),
    Toggle(bool),
    Remove,
}

pub(super) fn change(directory: &Path, name: &str, change: Change) -> Result<(), &'static str> {
    std::fs::create_dir_all(directory).map_err(|_| "writeFailed")?;
    let _lock = DirectoryLock::acquire_existing(&directory.join("opencode.jsonc"))
        .map_err(|_| "writeFailed")?;
    let files = documents(directory)?;
    let servers = configured_servers(&files)?;
    let target = match change {
        Change::Add(mut definition) => {
            if servers.contains_key(name) {
                return Err("exists");
            }
            let fields = definition.as_object_mut().ok_or("invalidConfig")?;
            let kind = fields.remove("type").ok_or("invalidConfig")?;
            if kind == "stdio" {
                let program = fields.remove("command").ok_or("invalidConfig")?;
                let mut command = vec![program];
                if let Some(Value::Array(args)) = fields.remove("args") {
                    command.extend(args);
                }
                fields.insert("command".into(), json!(command));
                if let Some(env) = fields.remove("env") {
                    fields.insert("environment".into(), env);
                }
                fields.insert("type".into(), json!("local"));
            } else if kind == "http" {
                fields.insert("type".into(), json!("remote"));
            } else {
                return Err("unsupported");
            }
            let file = if files[1].bytes.is_some() || files[0].bytes.is_none() {
                &files[1]
            } else {
                &files[0]
            };
            let legacy = file
                .root
                .get("mcp")
                .and_then(|property| property.to_serde_value())
                .and_then(|value| {
                    value.as_object().map(|fields| {
                        fields.get("servers").is_none_or(is_server)
                            && fields.values().any(is_server)
                    })
                })
                .unwrap_or(false);
            let mcp = file
                .root
                .object_value_or_create("mcp")
                .ok_or("invalidConfig")?;
            let group = if legacy {
                mcp
            } else {
                mcp.object_value_or_create("servers")
                    .ok_or("invalidConfig")?
            };
            if group.get(name).is_some() {
                return Err("exists");
            }
            jsonc::set(&group, name, &definition);
            file
        }
        Change::Toggle(_) | Change::Remove => {
            let server = servers.get(name).ok_or("missing")?;
            if server.duplicate {
                return Err("unsupported");
            }
            let file = &files[server.document];
            let mcp = file.root.object_value("mcp").ok_or("invalidConfig")?;
            let group = if server.legacy {
                mcp
            } else {
                mcp.object_value("servers").ok_or("invalidConfig")?
            };
            let property = group.get(name).ok_or("missing")?;
            if let Change::Toggle(enabled) = change {
                let entry = property.object_value().ok_or("invalidConfig")?;
                let (key, value) = if server.legacy {
                    ("enabled", enabled)
                } else {
                    ("disabled", !enabled)
                };
                jsonc::set(&entry, key, &json!(value));
            } else {
                property.remove();
            }
            file
        }
    };
    for file in &files {
        if native_account_files::read_optional(&file.path).map_err(|_| "writeFailed")? != file.bytes
        {
            return Err("writeFailed");
        }
    }
    let mut output = target.tree.to_string();
    if target
        .bytes
        .as_ref()
        .is_some_and(|bytes| bytes.starts_with(b"\xef\xbb\xbf"))
    {
        output.insert(0, '\u{feff}');
    }
    native_account_files::write_private_file(&target.path, output.as_bytes())
        .map_err(|_| "writeFailed")
}

pub(super) fn plugins(directory: &Path) -> Result<Vec<Entry>, String> {
    let mut entries = BTreeMap::new();
    for file in documents(directory)? {
        for key in ["plugin", "plugins"] {
            let value = file
                .root
                .get(key)
                .and_then(|property| property.to_serde_value());
            for plugin in value
                .as_ref()
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(name) = plugin
                    .as_str()
                    .or_else(|| plugin["package"].as_str())
                    .or_else(|| plugin.get(0).and_then(Value::as_str))
                {
                    let mut entry = Entry::new(name.to_owned(), Some(file.path.clone()));
                    entry.id = format!("package/{name}");
                    entries.insert(entry.id.clone(), entry);
                }
            }
        }
    }
    for root in ["plugin", "plugins"] {
        for path in children(&directory.join(root))? {
            if path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "ts" || ext == "js")
            {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let id = format!("file/{}", path.display());
                let mut entry = Entry::new(name, Some(path));
                entry.id = id;
                entries.insert(entry.id.clone(), entry);
            }
        }
    }
    Ok(entries.into_values().collect())
}
