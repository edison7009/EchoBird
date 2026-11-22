use super::{dsh_mcp, opencode_resources, App, Entry, Kind};
use crate::services::{
    native_account_files::{self, DirectoryLock},
    tool_config_manager::jsonc,
};
use jsonc_parser::{cst::CstRootNode, ParseOptions};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table};

enum Format {
    Json,
    Jsonc,
    Toml,
}

enum Config {
    Json(Value),
    Jsonc { tree: CstRootNode, bom: bool },
    Toml(DocumentMut),
}

impl Config {
    fn read(path: &Path, format: Format) -> Result<Self, &'static str> {
        let raw = native_account_files::read_optional(path)
            .map_err(|_| "readFailed")?
            .unwrap_or_default();
        let text = std::str::from_utf8(&raw)
            .map_err(|_| "invalidConfig")?
            .trim_start_matches('\u{feff}');
        match format {
            Format::Toml => return text.parse().map(Self::Toml).map_err(|_| "invalidConfig"),
            Format::Jsonc => {
                let options = ParseOptions {
                    allow_comments: true,
                    allow_trailing_commas: true,
                    ..Default::default()
                };
                let tree = CstRootNode::parse(text, &options).map_err(|_| "invalidConfig")?;
                tree.object_value_or_create().ok_or("invalidConfig")?;
                return Ok(Self::Jsonc {
                    tree,
                    bom: raw.starts_with(b"\xef\xbb\xbf"),
                });
            }
            Format::Json => {}
        }
        let value: Value = if raw.is_empty() {
            json!({})
        } else {
            serde_json::from_str(text).map_err(|_| "invalidConfig")?
        };
        if !value.is_object() {
            return Err("invalidConfig");
        }
        Ok(Self::Json(value))
    }

    fn entries(&self, key: &str) -> Result<Map<String, Value>, &'static str> {
        let value = match self {
            Config::Json(document) => document.get(key).cloned(),
            Config::Jsonc { tree, .. } => tree
                .object_value()
                .and_then(|root| root.get(key))
                .map(|value| value.to_serde_value().ok_or("invalidConfig"))
                .transpose()?,
            Config::Toml(document) => {
                let Some(item) = document.get(key) else {
                    return Ok(Map::new());
                };
                let mut table = DocumentMut::new();
                table.as_table_mut().insert("entries", item.clone());
                let mut data: Value =
                    toml_edit::de::from_document(table).map_err(|_| "invalidConfig")?;
                data.as_object_mut().and_then(|data| data.remove("entries"))
            }
        };
        match value {
            None => Ok(Map::new()),
            Some(Value::Object(entries)) => Ok(entries),
            _ => Err("invalidConfig"),
        }
    }

    fn set(&mut self, key: &str, id: &str, value: Option<Value>) -> Result<(), &'static str> {
        match self {
            Config::Json(document) => {
                let entries = document
                    .as_object_mut()
                    .ok_or("invalidConfig")?
                    .entry(key)
                    .or_insert_with(|| json!({}))
                    .as_object_mut()
                    .ok_or("invalidConfig")?;
                if let Some(value) = value {
                    entries.insert(id.into(), value);
                } else {
                    entries.remove(id);
                }
            }
            Config::Jsonc { tree, .. } => {
                let root = tree.object_value().ok_or("invalidConfig")?;
                if let Some(value) = value {
                    let entries = root.object_value_or_create(key).ok_or("invalidConfig")?;
                    jsonc::set(&entries, id, &value);
                } else if let Some(property) =
                    root.object_value(key).and_then(|group| group.get(id))
                {
                    property.remove();
                }
            }
            Config::Toml(document) => {
                let group = document
                    .as_table_mut()
                    .entry(key)
                    .or_insert_with(|| Item::Table(Table::new()));
                let inline = group.is_inline_table();
                let entries = group.as_table_like_mut().ok_or("invalidConfig")?;
                if let Some(value) = value {
                    let mut entry = toml_edit::ser::to_document(&json!({"entry": value}))
                        .map_err(|_| "invalidConfig")?;
                    let value = entry
                        .as_table_mut()
                        .remove("entry")
                        .ok_or("invalidConfig")?;
                    // TOML inline tables cannot contain a standard table item.
                    let value = if inline {
                        Item::Value(value.into_value().map_err(|_| "invalidConfig")?)
                    } else {
                        value
                    };
                    entries.insert(id, value);
                } else {
                    entries.remove(id);
                }
            }
        }
        Ok(())
    }

    fn flag(&mut self, key: &str, id: &str, enabled: bool) -> Result<(), &'static str> {
        match self {
            Config::Json(_) | Config::Jsonc { .. } => self.set(key, id, Some(json!(enabled))),
            Config::Toml(document) => {
                let entry = document
                    .get_mut(key)
                    .and_then(Item::as_table_like_mut)
                    .and_then(|table| table.get_mut(id))
                    .and_then(Item::as_table_like_mut)
                    .ok_or("missing")?;
                entry.insert("enabled", toml_edit::value(enabled));
                Ok(())
            }
        }
    }

    fn save(&self, path: &Path) -> Result<(), &'static str> {
        let bytes = match self {
            Config::Json(document) => {
                native_account_files::json(document).map_err(|_| "writeFailed")?
            }
            Config::Jsonc { tree, bom } => {
                let mut text = tree.to_string();
                if *bom {
                    text.insert(0, '\u{feff}');
                }
                text.into_bytes()
            }
            Config::Toml(document) => document.to_string().into_bytes(),
        };
        native_account_files::write_private_file(path, &bytes).map_err(|_| "writeFailed")
    }
}

fn edit(
    path: &Path,
    format: Format,
    apply: impl FnOnce(&mut Config) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    fs::create_dir_all(path.parent().ok_or("writeFailed")?).map_err(|_| "writeFailed")?;
    let _lock = DirectoryLock::acquire_existing(path).map_err(|_| "writeFailed")?;
    let mut document = Config::read(path, format)?;
    apply(&mut document)?;
    document.save(path)
}

pub(super) fn claude_user_memory_dir(directory: &Path, home: &Path) -> Option<PathBuf> {
    let Config::Json(settings) =
        Config::read(&directory.join("settings.json"), Format::Json).ok()?
    else {
        return None;
    };
    let value = settings.get("autoMemoryDirectory")?.as_str()?;
    let path = if value.starts_with("~/") {
        super::expand_user_path(Path::new(value), home)
    } else {
        PathBuf::from(value)
    };
    path.is_absolute().then_some(path)
}

const CLAUDE_SKILL_OVERRIDES: &str = "skillOverrides";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodexSkillRule {
    path: Option<PathBuf>,
    name: Option<String>,
    enabled: bool,
}

fn codex_skill_rules(document: &Config) -> Result<Vec<CodexSkillRule>, &'static str> {
    let Some(config) = document.entries("skills")?.remove("config") else {
        return Ok(Vec::new());
    };
    let mut rules: Vec<CodexSkillRule> =
        serde_json::from_value(config).map_err(|_| "invalidConfig")?;
    for rule in &mut rules {
        if let Some(path) = &mut rule.path {
            *path = path.canonicalize().unwrap_or_else(|_| path.clone());
        }
    }
    Ok(rules)
}

fn skill_name(entry: &Entry) -> &str {
    entry.declared_skill_name.as_deref().unwrap_or(&entry.name)
}

fn skill_path(entry: &Entry) -> Option<PathBuf> {
    entry.path.as_ref()?.join("SKILL.md").canonicalize().ok()
}

pub(super) fn apply_skill_states(app: &App, entries: &mut [Entry]) {
    if entries.is_empty() {
        return;
    }
    match app.id {
        "claudecode" => {
            let Ok(overrides) = Config::read(&app.directory.join("settings.json"), Format::Json)
                .and_then(|document| document.entries(CLAUDE_SKILL_OVERRIDES))
            else {
                return;
            };
            for entry in entries {
                let name = skill_name(entry);
                let mode = overrides.get(name);
                let can_toggle = name != "synced"
                    && !entry.name.eq_ignore_ascii_case("synced")
                    && mode.is_none_or(|mode| mode == "on" || mode == "off");
                entry.enabled = Some(mode.is_none_or(|mode| mode != "off"));
                entry.can_toggle = can_toggle;
            }
        }
        "codex" => {
            let Ok(rules) = Config::read(&app.mcp_file, Format::Toml)
                .and_then(|document| codex_skill_rules(&document))
            else {
                return;
            };
            for entry in entries {
                let Some(path) = skill_path(entry) else {
                    continue;
                };
                let name = skill_name(entry);
                let mode = rules
                    .iter()
                    .rev()
                    .find(|rule| match (&rule.path, &rule.name) {
                        (Some(selector), None) => *selector == path,
                        (None, Some(selector)) => selector == name,
                        _ => false,
                    });
                entry.enabled = Some(mode.is_none_or(|rule| rule.enabled));
                entry.can_toggle = path.to_str().is_some();
            }
        }
        _ => {}
    }
}

pub(super) fn toggle_skill(app: &App, entry: &Entry, enabled: bool) -> Result<(), String> {
    let result = match app.id {
        "claudecode" => {
            let name = skill_name(entry);
            edit(
                &app.directory.join("settings.json"),
                Format::Json,
                |document| {
                    let overrides = document.entries(CLAUDE_SKILL_OVERRIDES)?;
                    if overrides
                        .get(name)
                        .is_some_and(|mode| mode != "on" && mode != "off")
                    {
                        return Err("unsupported");
                    }
                    let mode = if enabled { "on" } else { "off" };
                    document.set(CLAUDE_SKILL_OVERRIDES, name, Some(json!(mode)))
                },
            )
        }
        "codex" => {
            let path = skill_path(entry).ok_or("missing")?;
            edit(&app.mcp_file, Format::Toml, |document| {
                set_codex_skill_rule(document, &path, enabled)
            })
        }
        _ => Err("unsupported"),
    };
    result.map_err(str::to_owned)
}

fn set_codex_skill_rule(
    document: &mut Config,
    path: &Path,
    enabled: bool,
) -> Result<(), &'static str> {
    codex_skill_rules(document)?;
    let Config::Toml(document) = document else {
        return Err("invalidConfig");
    };
    let path_value = path.to_str().ok_or("invalidConfig")?;
    let matches_path = |value: Option<&str>| {
        value.is_some_and(|value| {
            let candidate = Path::new(value);
            candidate == path
                || candidate
                    .canonicalize()
                    .is_ok_and(|candidate| candidate == path)
        })
    };
    let skills = document
        .entry("skills")
        .or_insert_with(|| Item::Table(Table::new()));
    let inline = skills.is_inline_table();
    let rules = skills
        .as_table_like_mut()
        .ok_or("invalidConfig")?
        .entry("config")
        .or_insert_with(|| {
            if inline {
                toml_edit::value(toml_edit::Array::new())
            } else {
                Item::ArrayOfTables(ArrayOfTables::new())
            }
        });
    // Move the exact path rule after any name rule that also matches this skill.
    if let Some(tables) = rules.as_array_of_tables_mut() {
        tables.retain(|rule| !matches_path(rule.get("path").and_then(Item::as_str)));
        let mut rule = Table::new();
        rule.insert("path", toml_edit::value(path_value));
        rule.insert("enabled", toml_edit::value(enabled));
        tables.push(rule);
    } else {
        let rules = rules.as_array_mut().ok_or("invalidConfig")?;
        rules.retain(|rule| {
            let path = rule
                .as_inline_table()
                .and_then(|rule| rule.get("path"))
                .and_then(toml_edit::Value::as_str);
            !matches_path(path)
        });
        let mut rule = InlineTable::new();
        rule.insert("path", path_value.into());
        rule.insert("enabled", enabled.into());
        rules.push(rule);
    }
    Ok(())
}

fn mcp_table(app: &App) -> &'static str {
    if app.id == "codex" {
        "mcp_servers"
    } else {
        "mcpServers"
    }
}

fn mcp_format(app: &App) -> Format {
    match app.id {
        "codex" => Format::Toml,
        "qoder" | "qodercn" => Format::Jsonc,
        _ => Format::Json,
    }
}

fn raw_mcp(app: &App) -> Result<Map<String, Value>, &'static str> {
    if app.id == "dsh" {
        return Ok(dsh_mcp::read(&app.mcp_file)?.entries);
    }
    if app.id == "opencode" {
        return Ok(opencode_resources::servers(&app.directory)?
            .into_iter()
            .map(|(name, server)| (name, server.value))
            .collect());
    }
    Config::read(&app.mcp_file, mcp_format(app))?.entries(mcp_table(app))
}

fn uses_disabled_mcp_flag(app: &App) -> bool {
    matches!(
        app.id,
        "workbuddy"
            | "workbuddyai"
            | "antigravitydesktop"
            | "qoder"
            | "qodercn"
            | "cline"
            | "devincli"
            | "opencode"
    )
}

fn workbuddy_connector(app: &App, server: &Value) -> bool {
    matches!(app.id, "workbuddy" | "workbuddyai") && server["managedBy"] == "connector"
}

pub(super) fn mcp_entries(app: &App) -> Result<Vec<Entry>, String> {
    let (servers, removable) = if app.id == "dsh" {
        let native = dsh_mcp::read(&app.mcp_file)?;
        (native.entries, native.removable)
    } else if app.id == "opencode" {
        let mut removable = std::collections::HashSet::new();
        let servers = opencode_resources::servers(&app.directory)?
            .into_iter()
            .map(|(name, server)| {
                if !server.duplicate {
                    removable.insert(name.clone());
                }
                (name, server.value)
            })
            .collect();
        (servers, removable)
    } else {
        (raw_mcp(app)?, Default::default())
    };
    Ok(servers
        .into_iter()
        .filter(|(_, server)| server.is_object())
        .map(|(name, server)| {
            let mut entry = Entry::new(name, app.mcp_file.parent().map(Path::to_path_buf));
            entry.description = server["url"]
                .as_str()
                .or_else(|| server["serverUrl"].as_str())
                .or_else(|| server["transport"]["url"].as_str())
                .and_then(|value| reqwest::Url::parse(value).ok())
                .and_then(|url| url.host_str().map(str::to_owned));
            let connector = workbuddy_connector(app, &server);
            entry.enabled = if uses_disabled_mcp_flag(app) {
                Some(server["disabled"] != true)
            } else {
                server["enabled"]
                    .as_bool()
                    .or(matches!(app.id, "codex" | "kimicode" | "pi").then_some(true))
            };
            entry.can_copy = !connector
                && entry.enabled != Some(false)
                && normalize_transport(app, &server)
                    .and_then(|server| portable(&server, app.id == "codex", false))
                    .is_ok();
            entry.can_remove = !connector
                && (!matches!(app.id, "dsh" | "opencode") || removable.contains(&entry.id));
            entry.can_toggle = !connector
                && (app.id != "opencode" || removable.contains(&entry.id))
                && (uses_disabled_mcp_flag(app) || matches!(app.id, "codex" | "kimicode" | "pi"));
            entry
        })
        .collect())
}

fn normalize_transport(app: &App, server: &Value) -> Result<Value, &'static str> {
    let mut data = server.as_object().cloned().ok_or("invalidConfig")?;
    if uses_disabled_mcp_flag(app)
        && data
            .remove("disabled")
            .is_some_and(|disabled| disabled == true)
    {
        return Err("unsupported");
    }
    match app.id {
        "opencode" => return opencode_resources::portable(data),
        "workbuddy" | "workbuddyai" | "qoder" | "qodercn" => {
            if server["managedBy"] == "connector" {
                return Err("unsupported");
            }
        }
        "antigravitydesktop" => {
            if let Some(url) = data.remove("serverUrl") {
                if data.get("url").is_some_and(|existing| existing != &url) {
                    return Err("invalidConfig");
                }
                data.insert("url".into(), url);
            }
        }
        "cline" => {
            if let Some(transport) = data.remove("transport") {
                let Value::Object(fields) = transport else {
                    return Err("invalidConfig");
                };
                for (key, value) in fields {
                    if data.insert(key, value).is_some() {
                        return Err("invalidConfig");
                    }
                }
            }
            if data
                .get("type")
                .is_some_and(|kind| kind == "streamableHttp")
            {
                data.insert("type".into(), json!("http"));
            } else if !data.contains_key("type") && data.contains_key("url") {
                data.insert("type".into(), json!("sse"));
            }
        }
        "devincli" => {
            if let Some(transport) = data.remove("transport") {
                data.insert("type".into(), transport);
            }
        }
        "pi" => {
            if data
                .remove("enabled")
                .is_some_and(|enabled| enabled != true)
                || pi_mcp_has_runtime_values(server)
            {
                return Err("unsupported");
            }
            if server["type"] == "streamable-http" {
                data.insert("type".into(), json!("http"));
            }
        }
        "kimicode" => {
            if server["enabled"] == false {
                return Err("unsupported");
            }
            data.remove("enabled");
            if let Some(transport) = data.remove("transport") {
                data.insert(
                    "type".into(),
                    if transport == "streamable-http" {
                        json!("http")
                    } else {
                        transport
                    },
                );
            }
        }
        _ => {}
    }
    Ok(Value::Object(data))
}

// Pi interprets these values at runtime, including whole-value shell commands.
fn pi_mcp_has_runtime_values(server: &Value) -> bool {
    ["env", "headers"].iter().any(|key| {
        server[*key].as_object().is_some_and(|fields| {
            fields
                .values()
                .filter_map(Value::as_str)
                .any(|value| value.starts_with('!') || value.contains("${"))
        })
    })
}

// Translate the common transport fields. Refuse settings with no equivalent,
// such as secondary auth and per-client tool/permission policies.
fn portable(server: &Value, source_toml: bool, target_toml: bool) -> Result<Value, &'static str> {
    let mut data = server.as_object().cloned().ok_or("invalidConfig")?;
    let explicit = data.remove("type");
    let active = data.remove("enabled");
    if active.is_some() && !source_toml {
        return Err("unsupported");
    }
    let remote = data.get("url").is_some();
    let transport = explicit
        .as_ref()
        .map(|value| value.as_str().ok_or("invalidConfig"))
        .transpose()?
        .unwrap_or(if remote { "http" } else { "stdio" });
    let allowed: &[&str] = if remote {
        if !matches!(transport, "http" | "sse")
            || (target_toml && transport == "sse")
            || data.contains_key("command")
        {
            return Err("unsupported");
        }
        let url = data["url"]
            .as_str()
            .and_then(|value| reqwest::Url::parse(value).ok())
            .ok_or("invalidConfig")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.has_host()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("invalidConfig");
        }
        if source_toml {
            &["url", "http_headers"]
        } else {
            &["url", "headers"]
        }
    } else {
        if transport != "stdio"
            || data
                .get("command")
                .and_then(Value::as_str)
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err("invalidConfig");
        }
        &["command", "args", "env", "cwd"]
    };
    if data.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("unsupported");
    }
    for (key, value) in &data {
        let valid = match key.as_str() {
            "args" => value
                .as_array()
                .is_some_and(|values| values.iter().all(Value::is_string)),
            "env" | "headers" | "http_headers" => value
                .as_object()
                .is_some_and(|values| values.values().all(Value::is_string)),
            _ => value.is_string(),
        };
        if !valid {
            return Err("invalidConfig");
        }
    }
    let headers_key = if source_toml {
        "http_headers"
    } else {
        "headers"
    };
    if let Some(headers) = data.remove(headers_key) {
        data.insert(
            if target_toml {
                "http_headers"
            } else {
                "headers"
            }
            .into(),
            headers,
        );
    }
    if target_toml {
        if let Some(active) = active {
            data.insert("enabled".into(), active);
        }
    } else {
        data.insert("type".into(), json!(transport));
    }
    Ok(Value::Object(data))
}

pub(super) fn copy_mcp(app: &App, target: &App, id: &str) -> Result<(), &'static str> {
    let servers = raw_mcp(app)?;
    let server = normalize_transport(app, servers.get(id).ok_or("missing")?)?;
    if server["enabled"] == false && target.id != "codex" {
        return Err("unsupported");
    }
    insert_mcp(
        target,
        id,
        portable(&server, app.id == "codex", target.id == "codex")?,
    )
}

pub(super) fn add_mcp(app: &App, id: &str, definition: Value) -> Result<(), &'static str> {
    insert_mcp(app, id, portable(&definition, false, app.id == "codex")?)
}

fn insert_mcp(app: &App, id: &str, mut definition: Value) -> Result<(), &'static str> {
    if app.id == "dsh" {
        return dsh_mcp::add(&app.mcp_file, id, definition);
    }
    if app.id == "opencode" {
        return opencode_resources::change(
            &app.directory,
            id,
            opencode_resources::Change::Add(definition),
        );
    }
    if app.id == "antigravitydesktop" {
        let fields = definition.as_object_mut().ok_or("invalidConfig")?;
        if fields.remove("type").is_some_and(|kind| kind == "sse") {
            return Err("unsupported");
        }
        if let Some(url) = fields.remove("url") {
            fields.insert("serverUrl".into(), url);
        }
        fields.insert("disabled".into(), json!(false));
    }
    if app.id == "claudedesktop" {
        let fields = definition.as_object_mut().ok_or("invalidConfig")?;
        if fields.remove("type").is_none_or(|kind| kind != "stdio") {
            return Err("mcpLocalOnly");
        }
        if fields.contains_key("cwd") {
            return Err("unsupported");
        }
    }
    if app.id == "kimicode" {
        let fields = definition.as_object_mut().ok_or("invalidConfig")?;
        if fields.remove("type").is_some_and(|kind| kind == "sse") {
            fields.insert("transport".into(), json!("sse"));
        }
    }
    if app.id == "pi" {
        if !super::valid_mcp_name(id) {
            return Err("invalidName");
        }
        if definition["type"] == "sse" || pi_mcp_has_runtime_values(&definition) {
            return Err("unsupported");
        }
    }
    if app.id == "cline" {
        if definition["type"] == "http" {
            definition["type"] = json!("streamableHttp");
        }
        definition = json!({"transport": definition});
    } else if app.id == "devincli" {
        let fields = definition.as_object_mut().ok_or("invalidConfig")?;
        let transport = fields.remove("type").ok_or("invalidConfig")?;
        fields.insert("transport".into(), transport);
    }
    let key = mcp_table(app);
    edit(&app.mcp_file, mcp_format(app), |document| {
        let entries = document.entries(key)?;
        let exists = if app.id == "pi" {
            let namespace = id.replace('-', "_");
            entries
                .keys()
                .any(|name| name.replace('-', "_") == namespace)
        } else {
            entries.contains_key(id)
        };
        if exists {
            return Err("exists");
        }
        document.set(key, id, Some(definition))
    })
}

pub(super) fn remove_mcp(app: &App, id: &str) -> Result<(), String> {
    if app.id == "dsh" {
        return dsh_mcp::remove(&app.mcp_file, id).map_err(str::to_owned);
    }
    if app.id == "opencode" {
        return opencode_resources::change(&app.directory, id, opencode_resources::Change::Remove)
            .map_err(str::to_owned);
    }
    let key = mcp_table(app);
    edit(&app.mcp_file, mcp_format(app), |document| {
        let entries = document.entries(key)?;
        let item = entries.get(id).ok_or("missing")?;
        if workbuddy_connector(app, item) {
            return Err("unsupported");
        }
        document.set(key, id, None)
    })
    .map_err(str::to_owned)
}

pub(super) fn plugin_entries(app: &App) -> Result<Vec<Entry>, String> {
    if app.id == "kimicode" {
        let path = app.directory.join("plugins/installed.json");
        let Some(bytes) = native_account_files::read_optional(&path).map_err(|_| "readFailed")?
        else {
            return Ok(Vec::new());
        };
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalidConfig")?;
        let plugins = value["plugins"].as_array().ok_or("invalidConfig")?;
        return Ok(plugins
            .iter()
            .filter_map(|plugin| {
                let id = plugin["id"].as_str()?;
                let root = plugin["root"]
                    .as_str()
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute() && path.is_dir());
                let mut entry = Entry::new(id.into(), root);
                entry.enabled = plugin["enabled"].as_bool();
                Some(entry)
            })
            .collect());
    }
    if app.id == "codex" {
        return codex_plugins(app);
    }
    if app.id != "claudecode" {
        return Ok(Vec::new());
    }
    let root = app.directory.join("plugins");
    let installed =
        Config::read(&root.join("installed_plugins.json"), Format::Json)?.entries("plugins")?;
    let flags = Config::read(&app.directory.join("settings.json"), Format::Json)?
        .entries("enabledPlugins")?;
    let mut plugins = Vec::new();
    for (name, installations) in installed {
        let Some(user) = installations.as_array().and_then(|list| {
            list.iter().find(|item| {
                item["scope"] == "user" || item["scope"].as_str().is_none_or(str::is_empty)
            })
        }) else {
            continue;
        };
        let Some(folder) = user["installPath"]
            .as_str()
            .map(Path::new)
            .filter(|path| path.is_absolute() && path.is_dir())
        else {
            continue;
        };
        let mut entry = Entry::new(name.clone(), Some(folder.to_path_buf()));
        entry.enabled = flags.get(&name).and_then(Value::as_bool).or(Some(true));
        entry.description = user["version"].as_str().map(str::to_owned);
        entry.can_toggle = true;
        plugins.push(entry);
    }
    Ok(plugins)
}

fn codex_plugins(app: &App) -> Result<Vec<Entry>, String> {
    let configured = Config::read(&app.mcp_file, Format::Toml)?.entries("plugins")?;
    let cache = app.directory.join("plugins").join("cache");
    let mut entries = Vec::new();
    for market in directories(&cache)? {
        let Some(market_name) = market.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        for plugin in directories(&market)? {
            let Some(plugin_name) = plugin.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let newest = directories(&plugin)?
                .into_iter()
                .filter_map(|version| {
                    let files = [
                        "plugin.json",
                        ".codex-plugin/plugin.json",
                        ".claude-plugin/plugin.json",
                        ".cursor-plugin/plugin.json",
                    ];
                    let manifest = files.iter().find_map(|file| {
                        let bytes = fs::read(version.join(file)).ok()?;
                        let value: Value = serde_json::from_slice(&bytes).ok()?;
                        value.is_object().then_some(value)
                    })?;
                    let time = fs::metadata(&version)
                        .and_then(|metadata| metadata.modified())
                        .ok();
                    Some((time, version, manifest))
                })
                .max_by(
                    |(first_time, first_path, _), (second_time, second_path, _)| {
                        let first_numbered =
                            first_path.file_name().is_some_and(|name| name != "latest");
                        let second_numbered =
                            second_path.file_name().is_some_and(|name| name != "latest");
                        (first_numbered, first_time, first_path).cmp(&(
                            second_numbered,
                            second_time,
                            second_path,
                        ))
                    },
                );
            let Some((_, path, manifest)) = newest else {
                continue;
            };
            let id = format!("{plugin_name}@{market_name}");
            let name = manifest["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .unwrap_or(&id);
            let mut entry = Entry::new(name.into(), Some(path));
            entry.id = id;
            entry.description = manifest["version"].as_str().map(str::to_owned);
            entry.enabled = configured
                .get(&entry.id)
                .and_then(|spec| spec["enabled"].as_bool());
            entry.can_toggle = configured.get(&entry.id).is_some_and(Value::is_object);
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn directories(root: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    Ok(super::children(root)?
        .into_iter()
        .filter(|path| {
            !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                && fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir())
        })
        .collect())
}

pub(super) fn toggle(app: &App, kind: Kind, id: &str, enabled: bool) -> Result<(), String> {
    if app.id == "opencode" && kind == Kind::Mcp {
        return opencode_resources::change(
            &app.directory,
            id,
            opencode_resources::Change::Toggle(enabled),
        )
        .map_err(str::to_owned);
    }
    if kind == Kind::Mcp && (uses_disabled_mcp_flag(app) || matches!(app.id, "kimicode" | "pi")) {
        let key = mcp_table(app);
        return edit(&app.mcp_file, mcp_format(app), |document| {
            if let Config::Jsonc { tree, .. } = document {
                let entry = tree
                    .object_value()
                    .and_then(|root| root.object_value(key))
                    .and_then(|group| group.object_value(id))
                    .ok_or("missing")?;
                jsonc::set(&entry, "disabled", &json!(!enabled));
                return Ok(());
            }
            let mut server = document.entries(key)?.remove(id).ok_or("missing")?;
            if workbuddy_connector(app, &server) {
                return Err("unsupported");
            }
            let fields = server.as_object_mut().ok_or("invalidConfig")?;
            if matches!(app.id, "kimicode" | "pi") {
                fields.insert("enabled".into(), json!(enabled));
            } else if app.id == "devincli" && enabled {
                fields.remove("disabled");
            } else {
                fields.insert("disabled".into(), json!(!enabled));
            }
            document.set(key, id, Some(server))
        })
        .map_err(str::to_owned);
    }
    let (path, key, format) = match (app.id, kind) {
        ("claudecode", Kind::Plugins) => (
            app.directory.join("settings.json"),
            "enabledPlugins",
            Format::Json,
        ),
        ("codex", Kind::Plugins) => (app.mcp_file.clone(), "plugins", Format::Toml),
        ("codex", Kind::Mcp) => (app.mcp_file.clone(), "mcp_servers", Format::Toml),
        _ => return Err("unsupported".into()),
    };
    edit(&path, format, |document| document.flag(key, id, enabled)).map_err(str::to_owned)
}
