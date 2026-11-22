//! User-scoped resources shown under the selected application.
mod claude_desktop;
mod config;
mod dsh_mcp;
mod opencode_resources;
mod pi_resources;

use crate::services::{claude_code_accounts, codex_accounts};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Kind {
    Skills,
    Mcp,
    Plugins,
    Memory,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Entry {
    id: String,
    name: String,
    description: Option<String>,
    #[serde(skip)]
    declared_skill_name: Option<String>,
    group: Option<String>,
    path: Option<PathBuf>,
    enabled: Option<bool>,
    can_copy: bool,
    can_remove: bool,
    can_toggle: bool,
}

impl Entry {
    fn new(name: String, path: Option<PathBuf>) -> Self {
        Self {
            id: name.clone(),
            name,
            description: None,
            declared_skill_name: None,
            group: None,
            path,
            enabled: None,
            can_copy: false,
            can_remove: false,
            can_toggle: false,
        }
    }
}

struct App {
    id: &'static str,
    home: PathBuf,
    directory: PathBuf,
    mcp_file: PathBuf,
}

const TOOL_IDS: [&str; 19] = [
    "antigravity",
    "antigravitydesktop",
    "dsh",
    "claudecode",
    "claudedesktop",
    "codex",
    "chatgptdesktop",
    "cursor",
    "cline",
    "devincli",
    "qoder",
    "qodercn",
    "workbuddy",
    "workbuddyai",
    "kimicode",
    "kimidesktop",
    "pi",
    "opencode",
    "opencodedesktop",
];

impl App {
    fn open(id: &str, kind: Kind) -> Result<Self, String> {
        Self::open_in_home(id, kind, dirs::home_dir().ok_or("unsupported")?)
    }

    fn open_in_home(id: &str, kind: Kind, home: PathBuf) -> Result<Self, String> {
        let (id, directory, mcp_file) = match (id, kind) {
            ("claudecode", _) | ("claudedesktop", Kind::Memory) => (
                "claudecode",
                claude_code_accounts::config_dir()?,
                claude_code_accounts::config_path()?,
            ),
            ("claudedesktop", _) => {
                let directory = claude_desktop::data_directory()?;
                (
                    "claudedesktop",
                    directory.clone(),
                    directory.join("claude_desktop_config.json"),
                )
            }
            ("codex" | "chatgptdesktop", _) => {
                let directory = codex_accounts::codex_home()?;
                ("codex", directory.clone(), directory.join("config.toml"))
            }
            ("antigravity" | "antigravitydesktop", _) => {
                let directory = home.join(".gemini/config");
                let owner = if id == "antigravity" && kind == Kind::Skills {
                    "antigravity"
                } else {
                    "antigravitydesktop"
                };
                (owner, directory.clone(), directory.join("mcp_config.json"))
            }
            ("dsh", _) => {
                let directory = super::tool_config_manager::dsh::dsh_config_dir();
                let profile = directory.join("profiles/desktop/cordis.patch.yml");
                ("dsh", directory, profile)
            }
            ("cursor", _) => {
                let directory = home.join(".cursor");
                ("cursor", directory.clone(), directory.join("mcp.json"))
            }
            ("cline", _) => {
                let directory = configured_home("CLINE_DIR", home.join(".cline"));
                let data = configured_home("CLINE_DATA_DIR", directory.join("data"));
                let mcp = configured_home(
                    "CLINE_MCP_SETTINGS_PATH",
                    data.join("settings/cline_mcp_settings.json"),
                );
                ("cline", directory, mcp)
            }
            ("devincli", _) => {
                let config = if cfg!(windows) {
                    dirs::config_dir().ok_or("unsupported")?
                } else {
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|path| path.is_absolute())
                        .unwrap_or_else(|| home.join(".config"))
                };
                let directory = config.join("devin");
                (
                    "devincli",
                    directory.clone(),
                    directory.join("mcp_config.json"),
                )
            }
            ("pi", _) => {
                let directory = std::env::var_os("PI_CODING_AGENT_DIR")
                    .filter(|value| !value.is_empty())
                    .map(|value| expand_user_path(Path::new(&value), &home))
                    .unwrap_or_else(|| home.join(".pi/agent"));
                ("pi", directory.clone(), directory.join("mcp.json"))
            }
            ("qoder", _) => {
                let directory = home.join(".qoder");
                ("qoder", directory.clone(), directory.join("settings.json"))
            }
            ("qodercn", _) => {
                let directory = home.join(".qoder-cn");
                (
                    "qodercn",
                    directory.clone(),
                    directory.join("settings.json"),
                )
            }
            ("workbuddy", _) => {
                let directory = configured_home("WORKBUDDY_CONFIG_DIR", home.join(".workbuddy"));
                ("workbuddy", directory.clone(), directory.join("mcp.json"))
            }
            ("workbuddyai", _) => {
                let directory = home.join(".workbuddy-ai");
                ("workbuddyai", directory.clone(), directory.join("mcp.json"))
            }
            ("kimicode" | "kimidesktop", _) => {
                let directory = configured_home("KIMI_CODE_HOME", home.join(".kimi-code"));
                ("kimicode", directory.clone(), directory.join("mcp.json"))
            }
            ("opencode" | "opencodedesktop", _) => {
                let directory = super::tool_config_manager::opencode::opencode_config_dir();
                (
                    "opencode",
                    directory.clone(),
                    directory.join("opencode.jsonc"),
                )
            }
            _ => return Err("unsupported".into()),
        };
        if !directory.is_absolute() || !mcp_file.is_absolute() {
            return Err("unsupported".into());
        }
        Ok(Self {
            id,
            home,
            directory,
            mcp_file,
        })
    }

    fn primary_skills(&self) -> PathBuf {
        self.directory.join("skills")
    }

    fn supports(&self, kind: Kind) -> bool {
        match kind {
            Kind::Skills | Kind::Mcp => true,
            Kind::Plugins => matches!(
                self.id,
                "claudecode" | "codex" | "pi" | "kimicode" | "opencode"
            ),
            Kind::Memory => matches!(
                self.id,
                "claudecode"
                    | "codex"
                    | "antigravitydesktop"
                    | "dsh"
                    | "pi"
                    | "cline"
                    | "devincli"
                    | "kimicode"
                    | "opencode"
            ),
        }
    }

    fn skill_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.primary_skills()];
        match self.id {
            "codex" | "pi" | "kimicode" | "opencode" => {
                let shared = self.home.join(".agents/skills");
                if shared != roots[0] {
                    roots.push(shared);
                }
                if self.id == "opencode" {
                    roots.push(self.home.join(".claude/skills"));
                }
            }
            "devincli" => roots.extend([
                self.directory.with_file_name("cognition").join("skills"),
                self.home.join(".agents/skills"),
            ]),
            "antigravity" => roots.extend([
                self.home.join(".gemini/antigravity-cli/skills"),
                self.home.join(".gemini/skills"),
            ]),
            "dsh" => {
                let root = configured_home("DSH_AGENTS_HOME", self.home.join(".agents"));
                if root.is_absolute() && root.join("skills") != roots[0] {
                    roots.push(root.join("skills"));
                }
            }
            _ => {}
        }
        roots
    }

    fn instructions(&self) -> Option<PathBuf> {
        match self.id {
            "claudecode" => Some(self.directory.join("CLAUDE.md")),
            "dsh" => Some(self.directory.join("AGENTS.md")),
            "cline" => Some(self.directory.join("rules/AGENTS.md")),
            "devincli" | "kimicode" | "opencode" => Some(self.directory.join("AGENTS.md")),
            "pi" => Some(pi_resources::instructions(&self.directory)),
            "antigravitydesktop" => {
                let gemini = self.directory.join("GEMINI.md");
                Some(if gemini.exists() {
                    gemini
                } else {
                    self.directory.join("AGENTS.md")
                })
            }
            "codex" => {
                let override_file = self.directory.join("AGENTS.override.md");
                if override_file.is_file() {
                    Some(override_file)
                } else {
                    Some(self.directory.join("AGENTS.md"))
                }
            }
            _ => None,
        }
    }

    fn assets(&self, kind: Kind) -> Result<Vec<Entry>, String> {
        if !self.supports(kind) {
            return Err("unsupported".into());
        }
        let mut entries = match kind {
            Kind::Skills => self.skills()?,
            Kind::Mcp => config::mcp_entries(self)?,
            Kind::Plugins if self.id == "pi" => pi_resources::extensions(&self.directory)?,
            Kind::Plugins if self.id == "opencode" => opencode_resources::plugins(&self.directory)?,
            Kind::Plugins => config::plugin_entries(self)?,
            Kind::Memory => self.memory()?,
        };
        entries.sort_by_cached_key(|entry| (entry.group.clone(), entry.name.to_lowercase()));
        Ok(entries)
    }

    fn skills(&self) -> Result<Vec<Entry>, String> {
        if self.id == "claudedesktop" {
            return claude_desktop::skills(&self.directory);
        }
        let mut skills = Vec::new();
        for (root_index, root) in self.skill_roots().iter().enumerate() {
            if self.id == "pi" {
                skills.extend(pi_resources::skills(root, root_index)?);
                continue;
            }
            let writable_root = real_directory(root);
            for file in children(root)? {
                let Some(name) = file
                    .file_name()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.starts_with('.'))
                else {
                    continue;
                };
                let instructions = file.join("SKILL.md");
                if !instructions.is_file() {
                    continue;
                }
                let writable = writable_root
                    && real_directory(&file)
                    && fs::symlink_metadata(&instructions).is_ok_and(|info| info.is_file());
                let mut entry = Entry::new(name.into(), Some(file.clone()));
                entry.id = format!("{root_index}/{name}");
                if let Some(metadata) = markdown_metadata(&instructions) {
                    entry.description = metadata.description;
                    entry.declared_skill_name = metadata.name;
                }
                entry.can_copy = writable;
                // Secondary roots may be shared with other clients.
                entry.can_remove = writable && root_index == 0;
                skills.push(entry);
            }
        }
        config::apply_skill_states(self, &mut skills);
        Ok(skills)
    }

    fn memory(&self) -> Result<Vec<Entry>, String> {
        if self.id == "antigravitydesktop" {
            let mut entries = Vec::new();
            for name in ["AGENTS.md", "GEMINI.md"] {
                add_memory_file(&mut entries, self.directory.join(name), "");
            }
            add_memory_files(&mut entries, &self.directory.join("rules"), "")?;
            return Ok(entries);
        }
        let Some(instructions) = self.instructions() else {
            return Ok(vec![]);
        };
        let mut entries = Vec::new();
        if instructions.is_file() {
            let name = instructions
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let mut entry = Entry::new(name, Some(instructions));
            entry.id = "instructions".into();
            entry.group = Some(String::new());
            entry.can_copy = true;
            entries.push(entry);
        }
        if self.id == "claudecode" {
            add_claude_rules(&mut entries, &self.directory.join("rules"))?;
            if let Some(directory) = config::claude_user_memory_dir(&self.directory, &self.home) {
                add_memory_files(&mut entries, &directory, "")?;
            }
            for agent in children(&self.directory.join("agent-memory"))? {
                if fs::symlink_metadata(&agent).is_ok_and(|metadata| metadata.is_dir()) {
                    let group = format!(
                        "agent-memory/{}",
                        agent.file_name().unwrap_or_default().to_string_lossy()
                    );
                    add_memory_files(&mut entries, &agent, &group)?;
                }
            }
            for project in children(&self.directory.join("projects"))? {
                let Some(project_name) = project.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let directory = project.join("memory");
                if !fs::symlink_metadata(&project).is_ok_and(|info| info.is_dir())
                    || !fs::symlink_metadata(&directory).is_ok_and(|info| info.is_dir())
                {
                    continue;
                }
                add_memory_files(
                    &mut entries,
                    &directory,
                    project_name.trim_start_matches('-'),
                )?;
            }
        } else if self.id == "codex" {
            add_memory_files(&mut entries, &self.directory.join("memories"), "Codex")?;
        } else if self.id == "cline" {
            add_memory_file(&mut entries, self.home.join(".agents/AGENTS.md"), "");
            for root in [
                self.directory.join("rules"),
                self.home.join("Documents/Cline/Rules"),
                self.home.join("Cline/Rules"),
            ] {
                add_memory_files(&mut entries, &root, "")?;
            }
        } else if self.id == "devincli" {
            add_memory_files(&mut entries, &self.directory.join("rules"), "")?;
            let legacy = self.home.join(".devin");
            add_memory_file(&mut entries, legacy.join("global_rules.md"), "");
            add_memory_files(&mut entries, &legacy.join("rules"), "")?;
        }
        let mut listed = std::collections::HashSet::new();
        entries.retain(|entry| listed.insert(entry.path.clone()));
        Ok(entries)
    }
}

fn expand_user_path(path: &Path, home: &Path) -> PathBuf {
    path.strip_prefix("~")
        .map(|suffix| home.join(suffix))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn configured_home(env: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(env)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}

fn real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            metadata.is_dir() && metadata.file_attributes() & 0x400 == 0
        }
        #[cfg(not(windows))]
        metadata.is_dir()
    })
}

fn children(path: &Path) -> Result<Vec<PathBuf>, String> {
    match fs::read_dir(path) {
        Ok(entries) => entries
            .map(|entry| {
                entry
                    .map(|entry| entry.path())
                    .map_err(|_| "readFailed".into())
            })
            .collect(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(_) => Err("readFailed".into()),
    }
}

#[derive(Default)]
struct MarkdownMetadata {
    name: Option<String>,
    description: Option<String>,
}

fn markdown_metadata(path: &Path) -> Option<MarkdownMetadata> {
    let mut text = String::new();
    let mut input = fs::File::open(path).ok()?.take(64 * 1024);
    input.read_to_string(&mut text).ok()?;
    let mut frontmatter = text.lines().skip_while(|line| line.trim() != "---");
    frontmatter.next()?;
    let yaml = frontmatter
        .take_while(|line| line.trim() != "---")
        .collect::<Vec<_>>()
        .join("\n");
    let data = yaml_rust2::YamlLoader::load_from_str(&yaml).ok()?;
    let fields = data.first()?.as_hash()?;
    let field = |key: &str| {
        fields
            .get(&yaml_rust2::Yaml::String(key.into()))?
            .as_str()
            .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|value| !value.is_empty())
    };
    Some(MarkdownMetadata {
        name: field("name"),
        description: field("description"),
    })
}

fn add_claude_rules(entries: &mut Vec<Entry>, root: &Path) -> Result<(), String> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        if !fs::symlink_metadata(&directory).is_ok_and(|metadata| metadata.is_dir()) {
            continue;
        }
        for path in children(&directory)? {
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                directories.push(path);
            } else if path.extension().is_some_and(|extension| extension == "md") {
                add_memory_file(entries, path, "");
            }
        }
    }
    Ok(())
}

fn add_memory_files(entries: &mut Vec<Entry>, root: &Path, group: &str) -> Result<(), String> {
    for path in children(root)? {
        if path.extension().is_some_and(|extension| extension == "md") {
            add_memory_file(entries, path, group);
        }
    }
    Ok(())
}

fn add_memory_file(entries: &mut Vec<Entry>, path: PathBuf, group: &str) {
    if !fs::symlink_metadata(&path).is_ok_and(|info| info.is_file()) {
        return;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let metadata = markdown_metadata(&path).unwrap_or_default();
    let mut entry = Entry::new(
        metadata.name.unwrap_or_else(|| name.into_owned()),
        Some(path.clone()),
    );
    entry.id = path.to_string_lossy().into_owned();
    entry.description = metadata.description;
    entry.group = Some(group.into());
    entries.push(entry);
}

pub(crate) fn list(tool: &str, kind: Kind) -> Result<Vec<Entry>, String> {
    App::open(tool, kind)?.assets(kind)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CopyResult {
    tool_id: String,
    status: &'static str,
}

pub(crate) fn copy(
    tool: &str,
    kind: Kind,
    id: &str,
    targets: &[String],
) -> Result<Vec<CopyResult>, String> {
    let app = App::open(tool, kind)?;
    let item = app
        .assets(kind)?
        .into_iter()
        .find(|item| item.id == id && item.can_copy)
        .ok_or("missing")?;
    let results = TOOL_IDS
        .into_iter()
        .filter(|target| *target != tool && targets.iter().any(|id| id == target))
        .map(|target| {
            let result =
                App::open(target, kind)
                    .map_err(|_| "unsupported")
                    .and_then(|destination| {
                        if destination.id == app.id
                            || !destination.supports(kind)
                            || !super::tool_manager::any_tool_installed(&[target])
                        {
                            return Err("unsupported");
                        }
                        copy_item(&app, &destination, kind, &item)
                    });
            CopyResult {
                tool_id: target.into(),
                status: result.err().unwrap_or("copied"),
            }
        })
        .collect();
    Ok(results)
}

fn copy_item(app: &App, target: &App, kind: Kind, item: &Entry) -> Result<(), &'static str> {
    match kind {
        Kind::Skills => {
            let source = item.path.as_deref().ok_or("missing")?;
            if target.id == "claudedesktop" {
                return claude_desktop::copy_skill(&target.directory, source, &item.name);
            }
            let name = source
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("invalidName")?;
            let declared_name = if target.id == "pi" {
                markdown_metadata(&source.join("SKILL.md")).and_then(|metadata| metadata.name)
            } else {
                None
            };
            if target
                .skills()
                .map_err(|_| "readFailed")?
                .iter()
                .any(|entry| {
                    entry.name == name || declared_name.as_deref() == Some(entry.name.as_str())
                })
            {
                return Err("exists");
            }
            copy_skill(source, &target.primary_skills(), name)
        }
        Kind::Mcp => config::copy_mcp(app, target, &item.id),
        Kind::Memory => {
            let source = item.path.as_deref().ok_or("missing")?;
            let target = target.instructions().ok_or("unsupported")?;
            if target.try_exists().map_err(|_| "readFailed")? {
                return Err("exists");
            }
            let bytes = fs::read(source).map_err(|_| "readFailed")?;
            let parent = target.parent().ok_or("writeFailed")?;
            fs::create_dir_all(parent).map_err(|_| "writeFailed")?;
            let _lock = super::native_account_files::DirectoryLock::acquire_existing(&target)
                .map_err(|_| "writeFailed")?;
            if fs::symlink_metadata(&target).is_ok() {
                return Err("exists");
            }
            super::native_account_files::write_private_file(&target, &bytes)
                .map_err(|_| "writeFailed")
        }
        Kind::Plugins => Err("unsupported"),
    }
}

fn copy_skill(source: &Path, root: &Path, name: &str) -> Result<(), &'static str> {
    if !matches!(
        (
            Path::new(name).components().next(),
            Path::new(name).components().count()
        ),
        (Some(Component::Normal(_)), 1)
    ) {
        return Err("unsupported");
    }
    if fs::symlink_metadata(root).is_ok() && !real_directory(root) {
        return Err("unsupported");
    }
    fs::create_dir_all(root).map_err(|_| "writeFailed")?;
    let destination = root.join(name);
    fs::create_dir(&destination).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => "exists",
        _ => "writeFailed",
    })?;
    let result = copy_contents(source, &destination, 0);
    if result.is_err() {
        let _ = fs::remove_dir_all(&destination);
    }
    result
}

fn copy_contents(source: &Path, destination: &Path, depth: usize) -> Result<(), &'static str> {
    if depth >= 32 || !real_directory(source) {
        return Err("unsupported");
    }
    let mut paths = children(source).map_err(|_| "readFailed")?;
    // Publish the entry point after its resources have been copied.
    paths.sort_by_key(|path| path.file_name().is_some_and(|name| name == "SKILL.md"));
    for path in paths {
        let target = destination.join(path.file_name().ok_or("unsupported")?);
        let metadata = fs::symlink_metadata(&path).map_err(|_| "readFailed")?;
        if metadata.is_file() {
            fs::copy(&path, &target).map_err(|_| "writeFailed")?;
        } else if metadata.is_dir() {
            fs::create_dir(&target).map_err(|_| "writeFailed")?;
            copy_contents(&path, &target, depth + 1)?;
        } else {
            return Err("unsupported");
        }
    }
    Ok(())
}

pub(crate) fn remove(tool: &str, kind: Kind, id: &str) -> Result<(), String> {
    let app = App::open(tool, kind)?;
    let item = app
        .assets(kind)?
        .into_iter()
        .find(|item| item.id == id && item.can_remove)
        .ok_or("missing")?;
    match kind {
        Kind::Skills => archive_skill(&app.directory, item.path.as_deref().ok_or("missing")?)
            .map_err(str::to_owned),
        Kind::Mcp => config::remove_mcp(&app, id),
        _ => Err("unsupported".into()),
    }
}

fn archive_skill(directory: &Path, source: &Path) -> Result<(), &'static str> {
    let mut name = source.file_name().ok_or("missing")?.to_os_string();
    name.push(format!("-{}", uuid::Uuid::new_v4()));
    let archive = directory.join("echobird-skill-backups");
    if !real_directory(source)
        || (fs::symlink_metadata(&archive).is_ok() && !real_directory(&archive))
    {
        return Err("writeFailed");
    }
    fs::create_dir_all(&archive).map_err(|_| "writeFailed")?;
    fs::rename(source, archive.join(name)).map_err(|_| "writeFailed")
}

pub(crate) fn toggle(tool: &str, kind: Kind, id: &str, enabled: bool) -> Result<(), String> {
    let app = App::open(tool, kind)?;
    let item = app
        .assets(kind)?
        .into_iter()
        .find(|entry| entry.id == id && entry.can_toggle)
        .ok_or("missing")?;
    if kind == Kind::Skills {
        return config::toggle_skill(&app, &item, enabled);
    }
    config::toggle(&app, kind, id, enabled)
}

fn valid_mcp_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn add_mcp(tool: &str, name: &str, definition: &str) -> Result<(), String> {
    let name = name.trim();
    if !valid_mcp_name(name) {
        return Err("invalidName".into());
    }
    if definition.len() > 128 * 1024 {
        return Err("invalidConfig".into());
    }
    let app = App::open(tool, Kind::Mcp)?;
    if !app.supports(Kind::Mcp) {
        return Err("unsupported".into());
    }
    let data = serde_json::from_str(definition).map_err(|_| "invalidConfig")?;
    config::add_mcp(&app, name, data).map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fixture(id: &'static str, root: &Path) -> App {
        let directory = root.join(id);
        App {
            id,
            home: root.to_path_buf(),
            mcp_file: directory.join(match id {
                "codex" => "config.toml",
                "claudedesktop" => "claude_desktop_config.json",
                "qoder" | "qodercn" => "settings.json",
                "cline" => "data/settings/cline_mcp_settings.json",
                "devincli" => "mcp_config.json",
                "opencode" => "opencode.jsonc",
                _ => "mcp.json",
            }),
            directory,
        }
    }

    #[test]
    fn additional_editions_open_and_edit_their_own_user_resources() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path();

        for (id, native_directory, mcp_name, other_directory) in [
            ("qodercn", ".qoder-cn", "settings.json", ".qoder"),
            ("workbuddyai", ".workbuddy-ai", "mcp.json", ".workbuddy"),
        ] {
            let other = home.join(other_directory);
            fs::create_dir_all(&other).unwrap();
            let other_config = other.join(mcp_name);
            let other_bytes = br#"{"mcpServers":{"other-edition":{"command":"example"}}}"#;
            fs::write(&other_config, other_bytes).unwrap();

            let app = App::open_in_home(id, Kind::Mcp, home.to_path_buf()).unwrap();
            assert_eq!(app.id, id);
            assert_eq!(app.mcp_file, home.join(native_directory).join(mcp_name));
            assert!(app.assets(Kind::Mcp).unwrap().is_empty());
            assert!(app.assets(Kind::Skills).unwrap().is_empty());

            let source = fixture("codex", home);
            let skill = source.primary_skills().join("test-skill");
            fs::create_dir_all(&skill).unwrap();
            fs::write(skill.join("SKILL.md"), "# Test skill").unwrap();
            let source_entry = source.assets(Kind::Skills).unwrap().remove(0);
            copy_item(&source, &app, Kind::Skills, &source_entry).unwrap();
            let entry = app.assets(Kind::Skills).unwrap().remove(0);
            assert_eq!(entry.name, "test-skill");
            assert_eq!(
                entry.path.unwrap(),
                home.join(native_directory).join("skills/test-skill")
            );
            assert!(!other.join("skills").exists());

            let unrelated = json!({"settings":{"keep":42}});
            let mut original = unrelated.clone();
            if id == "workbuddyai" {
                original["mcpServers"] = json!({
                    "connector": {"managedBy":"connector","url":"https://example.test"}
                });
            }
            fs::write(&app.mcp_file, original.to_string()).unwrap();
            config::add_mcp(&app, "local", json!({"command":"example"})).unwrap();
            config::toggle(&app, Kind::Mcp, "local", false).unwrap();
            let entries = app.assets(Kind::Mcp).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .find(|entry| entry.id == "local")
                    .unwrap()
                    .enabled,
                Some(false)
            );
            config::toggle(&app, Kind::Mcp, "local", true).unwrap();
            config::remove_mcp(&app, "local").unwrap();
            if id == "workbuddyai" {
                let connector = app.assets(Kind::Mcp).unwrap().remove(0);
                assert!(!connector.can_copy && !connector.can_remove && !connector.can_toggle);
                assert!(config::remove_mcp(&app, "connector").is_err());
                assert!(config::toggle(&app, Kind::Mcp, "connector", false).is_err());
            }
            let updated: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
            assert_eq!(updated["settings"], unrelated["settings"]);
            assert_eq!(
                updated["mcpServers"].get("connector"),
                original["mcpServers"].get("connector")
            );
            assert_eq!(fs::read(&other_config).unwrap(), other_bytes);
        }
    }

    #[test]
    fn opencode_adds_to_the_existing_legacy_mcp_table() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("opencode", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        fs::write(
            &app.mcp_file,
            r#"{"mcp":{"old":{"type":"local","command":["node"]}}}"#,
        )
        .unwrap();

        config::add_mcp(&app, "sample", json!({"command":"runner"})).unwrap();
        config::toggle(&app, Kind::Mcp, "sample", false).unwrap();
        let document: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
        assert_eq!(document["mcp"]["sample"]["command"], json!(["runner"]));
        assert_eq!(document["mcp"]["sample"]["enabled"], false);
        assert!(document["mcp"].get("servers").is_none());
    }

    #[test]
    fn opencode_combines_user_configs_and_preserves_jsonc_when_editing() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("opencode", root.path());
        let kimi = fixture("kimicode", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        let json = app.directory.join("opencode.json");
        fs::write(&json, r#"{"mcp":{"legacy":{"type":"local","command":["node","probe.js"],"enabled":false},"duplicate":{"type":"remote","url":"https://one.test"}}}"#).unwrap();
        fs::write(
            &app.mcp_file,
            r#"{
  // preserve provider settings
  "model": "fixture/model",
  "mcp": {
    "servers": {
      "duplicate": {"type":"remote","url":"https://two.test"},
    },
  },
}"#,
        )
        .unwrap();
        let entries = app.assets(Kind::Mcp).unwrap();
        assert_eq!(entries[0].description.as_deref(), Some("two.test"));
        assert!(!entries[0].can_remove);
        assert_eq!(entries[1].enabled, Some(false));
        config::toggle(&app, Kind::Mcp, "legacy", true).unwrap();
        config::copy_mcp(&app, &kimi, "legacy").unwrap();
        let copied: Value = serde_json::from_slice(&fs::read(&kimi.mcp_file).unwrap()).unwrap();
        assert_eq!(copied["mcpServers"]["legacy"]["command"], "node");
        assert_eq!(copied["mcpServers"]["legacy"]["args"], json!(["probe.js"]));

        config::add_mcp(&app, "sample", json!({"command":"node", "env":{"A":"B"}})).unwrap();
        config::toggle(&app, Kind::Mcp, "sample", false).unwrap();
        let text = fs::read_to_string(&app.mcp_file).unwrap();
        assert!(text.contains("// preserve provider settings"));
        let value =
            jsonc_parser::parse_to_serde_value::<Value>(&text, &Default::default()).unwrap();
        assert_eq!(value["model"], "fixture/model");
        let server = &value["mcp"]["servers"]["sample"];
        assert_eq!(server["command"], json!(["node"]));
        assert_eq!(server["environment"], json!({"A":"B"}));
        assert_eq!(server["disabled"], true);
        config::remove_mcp(&app, "sample").unwrap();
        assert_eq!(app.assets(Kind::Mcp).unwrap().len(), 2);
    }

    #[test]
    fn opencode_plugins_with_the_same_name_have_distinct_resource_ids() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("opencode", root.path());
        for name in ["plugin", "plugins"] {
            let directory = app.directory.join(name);
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("shared.ts"), "").unwrap();
        }
        fs::write(&app.mcp_file, r#"{"plugin":["shared.ts"]}"#).unwrap();
        let entries = app.assets(Kind::Plugins).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.iter().all(|entry| entry.name == "shared.ts"));
        assert_eq!(
            entries
                .iter()
                .map(|entry| &entry.id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn kimi_and_pi_list_native_user_package_declarations() {
        let root = tempfile::tempdir().unwrap();
        let kimi = fixture("kimicode", root.path());
        fs::create_dir_all(kimi.directory.join("plugins")).unwrap();
        fs::write(kimi.directory.join("AGENTS.md"), "# User instructions").unwrap();
        fs::write(kimi.directory.join("plugins/installed.json"), json!({
            "version": 1, "plugins": [{"id": "sample", "enabled": false, "root": kimi.directory}]
        }).to_string()).unwrap();
        assert_eq!(kimi.assets(Kind::Memory).unwrap()[0].name, "AGENTS.md");
        let plugins = kimi.assets(Kind::Plugins).unwrap();
        assert_eq!(plugins[0].id, "sample");
        assert_eq!(plugins[0].enabled, Some(false));

        let pi = fixture("pi", root.path());
        fs::create_dir_all(&pi.directory).unwrap();
        fs::write(pi.directory.join("settings.json"), r#"{"packages":["npm:example",{"source":"npm:example","skills":[]},{"source":"./local"}]}"#).unwrap();
        let packages = pi.assets(Kind::Plugins).unwrap();
        assert_eq!(
            packages
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["./local", "npm:example"]
        );
    }

    #[test]
    fn cline_and_devin_mcp_use_their_native_transports_and_preserve_config() {
        let root = tempfile::tempdir().unwrap();
        let cline = fixture("cline", root.path());
        let devin = fixture("devincli", root.path());
        fs::create_dir_all(cline.mcp_file.parent().unwrap()).unwrap();
        let original = json!({
            "keep": ["settings"],
            "mcpServers": {
                "legacy": {"url": "https://example.test/events", "disabled": false},
                "native": {"transport": {"type": "stdio", "command": "node"}, "timeout": 60}
            }
        });
        fs::write(&cline.mcp_file, original.to_string()).unwrap();

        config::add_mcp(&cline, "echo", json!({"url": "https://example.test/mcp"})).unwrap();
        assert_eq!(
            cline.assets(Kind::Mcp).unwrap()[0].description.as_deref(),
            Some("example.test")
        );
        let cline_config: Value =
            serde_json::from_slice(&fs::read(&cline.mcp_file).unwrap()).unwrap();
        assert_eq!(
            cline_config["mcpServers"]["echo"]["transport"],
            json!({"type":"streamableHttp","url":"https://example.test/mcp"})
        );
        config::copy_mcp(&cline, &devin, "echo").unwrap();
        config::copy_mcp(&cline, &devin, "legacy").unwrap();
        config::toggle(&devin, Kind::Mcp, "echo", false).unwrap();
        assert!(!devin.assets(Kind::Mcp).unwrap()[0].enabled.unwrap());
        config::toggle(&devin, Kind::Mcp, "echo", true).unwrap();
        let devin_config: Value =
            serde_json::from_slice(&fs::read(&devin.mcp_file).unwrap()).unwrap();
        assert_eq!(
            devin_config["mcpServers"]["echo"],
            json!({"transport":"http","url":"https://example.test/mcp"})
        );
        assert_eq!(devin_config["mcpServers"]["legacy"]["transport"], "sse");

        config::toggle(&cline, Kind::Mcp, "native", false).unwrap();
        let cline_config: Value =
            serde_json::from_slice(&fs::read(&cline.mcp_file).unwrap()).unwrap();
        assert_eq!(cline_config["mcpServers"]["native"]["disabled"], true);
        assert_eq!(cline_config["mcpServers"]["native"]["timeout"], 60);
        config::toggle(&cline, Kind::Mcp, "native", true).unwrap();
        config::remove_mcp(&cline, "echo").unwrap();
        config::copy_mcp(&devin, &cline, "echo").unwrap();
        config::remove_mcp(&cline, "echo").unwrap();
        let cline_config: Value =
            serde_json::from_slice(&fs::read(&cline.mcp_file).unwrap()).unwrap();
        assert_eq!(cline_config["keep"], original["keep"]);
        assert_eq!(
            cline_config["mcpServers"]["legacy"],
            original["mcpServers"]["legacy"]
        );
        assert_eq!(cline_config["mcpServers"]["native"]["timeout"], 60);
        assert_eq!(cline_config["mcpServers"]["native"]["disabled"], false);
        assert!(cline_config["mcpServers"].get("echo").is_none());
    }

    #[test]
    fn cline_and_devin_list_user_skills_and_rules_from_their_native_roots() {
        let root = tempfile::tempdir().unwrap();
        let cline = fixture("cline", root.path());
        let devin = fixture("devincli", root.path());
        for app in [&cline, &devin] {
            let skill = app.primary_skills().join("echo");
            fs::create_dir_all(&skill).unwrap();
            fs::write(skill.join("SKILL.md"), "# Fixture").unwrap();
            let rule = app.directory.join("rules/global.md");
            fs::create_dir_all(rule.parent().unwrap()).unwrap();
            fs::write(&rule, "# Global directions").unwrap();
            assert_eq!(
                app.assets(Kind::Skills).unwrap()[0].path.as_ref(),
                Some(&skill)
            );
            assert_eq!(
                app.assets(Kind::Memory).unwrap()[0].path.as_ref(),
                Some(&rule)
            );
        }
        let shared = root.path().join(".agents/skills/shared");
        fs::create_dir_all(&shared).unwrap();
        fs::write(shared.join("SKILL.md"), "# Shared fixture").unwrap();
        let skills = devin.assets(Kind::Skills).unwrap();
        assert_eq!(skills.len(), 2);
        assert!(skills[0].can_remove);
        assert!(!skills[1].can_remove);
    }

    #[test]
    fn chatgpt_desktop_reuses_the_codex_library() {
        let cli = App::open("codex", Kind::Skills).unwrap();
        let desktop = App::open("chatgptdesktop", Kind::Skills).unwrap();
        assert_eq!(desktop.id, cli.id);
        assert_eq!(desktop.skill_roots(), cli.skill_roots());
        assert_eq!(desktop.mcp_file, cli.mcp_file);
        assert_eq!(desktop.instructions(), cli.instructions());
        assert!(desktop.supports(Kind::Plugins));
    }

    #[test]
    fn claude_desktop_reuses_claude_code_only_for_memory() {
        let cli = App::open("claudecode", Kind::Memory).unwrap();
        let desktop = App::open("claudedesktop", Kind::Memory).unwrap();
        assert_eq!(desktop.id, cli.id);
        assert_eq!(desktop.directory, cli.directory);
        assert_eq!(desktop.instructions(), cli.instructions());
        assert!(desktop.supports(Kind::Memory));

        let desktop = App::open("claudedesktop", Kind::Mcp).unwrap();
        assert_eq!(desktop.id, "claudedesktop");
        assert_ne!(desktop.mcp_file, cli.mcp_file);
    }

    #[test]
    fn claude_desktop_copies_skills_through_the_active_cowork_manifest() {
        let root = tempfile::tempdir().unwrap();
        let code = fixture("claudecode", root.path());
        let desktop = fixture("claudedesktop", root.path());
        let codex = fixture("codex", root.path());
        let account = "6a356259-eaab-4e7f-98f6-19fe7e9091cb";
        let profiles = desktop
            .directory
            .join("local-agent-mode-sessions/skills-plugin");
        let active = profiles.join("active-org").join(account);
        let inactive = profiles.join("other-org/0c0cff56-4057-4b63-9c44-4b5553ad7bfe");
        let skill = code.primary_skills().join("fixture");
        fs::create_dir_all(&skill).unwrap();
        fs::create_dir_all(&active).unwrap();
        fs::create_dir_all(&inactive).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: Fixture Skill\ndescription: Test skill\n---\nContent",
        )
        .unwrap();
        fs::write(
            desktop.directory.join("config.json"),
            json!({"lastKnownAccountUuid":account}).to_string(),
        )
        .unwrap();
        let initial =
            json!({"lastUpdated":0,"skills":[{"skillId":"builtin","enabled":false}],"keep":true});
        fs::write(inactive.join("manifest.json"), initial.to_string()).unwrap();

        let item = code.assets(Kind::Skills).unwrap().pop().unwrap();
        assert_eq!(
            copy_item(&code, &desktop, Kind::Skills, &item),
            Err("coworkProfileMissing")
        );
        fs::write(active.join("manifest.json"), initial.to_string()).unwrap();
        copy_item(&code, &desktop, Kind::Skills, &item).unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(active.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["keep"], true);
        assert_eq!(manifest["skills"][0], initial["skills"][0]);
        assert_eq!(manifest["skills"][1]["skillId"], "fixture");
        assert_eq!(manifest["skills"][1]["enabled"], true);
        assert!(manifest["skills"][1]["updatedAt"]
            .as_str()
            .unwrap()
            .ends_with('Z'));
        assert_eq!(
            fs::read_to_string(inactive.join("manifest.json")).unwrap(),
            initial.to_string()
        );
        assert_eq!(
            copy_item(&code, &desktop, Kind::Skills, &item),
            Err("exists")
        );

        let exported = desktop.assets(Kind::Skills).unwrap().pop().unwrap();
        assert_eq!(exported.name, "Fixture Skill");
        assert!(!exported.can_remove);
        copy_item(&desktop, &codex, Kind::Skills, &exported).unwrap();
        assert!(codex.primary_skills().join("fixture/SKILL.md").is_file());

        let ambiguous = profiles.join("another-org").join(account);
        fs::create_dir_all(&ambiguous).unwrap();
        fs::write(ambiguous.join("manifest.json"), initial.to_string()).unwrap();
        assert_eq!(
            copy_item(&code, &desktop, Kind::Skills, &item),
            Err("coworkProfileAmbiguous")
        );
        assert!(!ambiguous.join("skills").exists());
    }

    #[test]
    fn claude_desktop_mcp_uses_the_native_stdio_format() {
        let root = tempfile::tempdir().unwrap();
        let desktop = fixture("claudedesktop", root.path());
        fs::create_dir_all(&desktop.directory).unwrap();
        let initial = json!({"deploymentMode":"1p","mcpServers":{"existing":{"command":"other"}}});
        fs::write(&desktop.mcp_file, initial.to_string()).unwrap();
        let remote = json!({"url":"https://example.test/mcp"});
        assert_eq!(
            config::add_mcp(&desktop, "remote", remote),
            Err("mcpLocalOnly")
        );
        let local = json!({"command":"node","args":["server.js"]});
        config::add_mcp(&desktop, "local", local.clone()).unwrap();
        let current: Value = serde_json::from_slice(&fs::read(&desktop.mcp_file).unwrap()).unwrap();
        assert_eq!(current["mcpServers"]["local"], local);
        assert_eq!(
            current["mcpServers"]["existing"],
            initial["mcpServers"]["existing"]
        );
        assert_eq!(current["deploymentMode"], initial["deploymentMode"]);
        assert_eq!(desktop.assets(Kind::Mcp).unwrap().len(), 2);
        assert!(!desktop.supports(Kind::Plugins));
        assert!(!desktop.supports(Kind::Memory));
    }

    #[test]
    fn claude_code_skill_switch_preserves_other_visibility_modes() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("claudecode", root.path());
        for (folder, name) in [("first", "one"), ("second", "two")] {
            let path = app.primary_skills().join(folder);
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join("SKILL.md"),
                format!("---\nname: {name}\n---\nSkill"),
            )
            .unwrap();
        }
        let settings = app.directory.join("settings.json");
        fs::write(
            &settings,
            r#"{"skillOverrides":{"two":"user-invocable-only"},"theme":"dark"}"#,
        )
        .unwrap();
        let entries = app.assets(Kind::Skills).unwrap();
        assert_eq!(entries[0].enabled, Some(true));
        assert!(entries[0].can_toggle);
        assert!(!entries[1].can_toggle);
        config::toggle_skill(&app, &entries[0], false).unwrap();
        assert_eq!(app.assets(Kind::Skills).unwrap()[0].enabled, Some(false));
        config::toggle_skill(&app, &entries[0], true).unwrap();
        assert_eq!(app.assets(Kind::Skills).unwrap()[0].enabled, Some(true));
        let settings: Value = serde_json::from_slice(&fs::read(settings).unwrap()).unwrap();
        assert_eq!(
            settings["skillOverrides"],
            json!({"one":"on","two":"user-invocable-only"})
        );
        assert_eq!(settings["theme"], "dark");
    }

    #[test]
    fn codex_skill_switch_honors_rule_order_across_toml_styles() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("codex", root.path());
        let path = app.primary_skills().join("fixture");
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("SKILL.md"), "---\nname: fixture\n---\nSkill").unwrap();
        for styles in [
            "[[skills.config]]\nname = \"fixture\"\nenabled = false\n",
            "[skills]\nconfig = [{name = \"fixture\", enabled = false}]\n",
            "skills = {config = [{name = \"fixture\", enabled = false}]}\n",
        ] {
            let original = format!("# preserve comment\nmodel = \"test\"\n{styles}");
            fs::write(&app.mcp_file, original).unwrap();
            let skill = app.assets(Kind::Skills).unwrap().pop().unwrap();
            assert!(skill.can_toggle);
            assert_eq!(skill.enabled, Some(false));

            config::toggle_skill(&app, &skill, true).unwrap();
            assert_eq!(app.assets(Kind::Skills).unwrap()[0].enabled, Some(true));
            config::toggle_skill(&app, &skill, false).unwrap();
            assert_eq!(app.assets(Kind::Skills).unwrap()[0].enabled, Some(false));

            let written = fs::read_to_string(&app.mcp_file).unwrap();
            assert!(written.contains("# preserve comment"));
            assert!(written.contains("model = \"test\""));
            let document: toml_edit::DocumentMut = written.parse().unwrap();
            let value: Value = toml_edit::de::from_document(document).unwrap();
            assert_eq!(value["skills"]["config"].as_array().unwrap().len(), 2);
            assert_eq!(value["skills"]["config"][0]["name"], "fixture");
            assert_eq!(value["skills"]["config"][1]["enabled"], false);
        }
    }

    #[test]
    fn skill_copy_uses_native_roots_and_never_replaces_an_existing_item() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture("claudecode", root.path());
        let destination = fixture("qoder", root.path());
        let skill = source.primary_skills().join("assistant");
        fs::create_dir_all(skill.join("references")).unwrap();
        let instructions =
            "---\nname: assistant\ndescription: >-\n  Does review\n  with references\n---\n";
        fs::write(skill.join("SKILL.md"), instructions).unwrap();
        fs::write(skill.join("references/example.md"), "fixture reference").unwrap();
        let mut entries = source.assets(Kind::Skills).unwrap();
        let item = entries.pop().unwrap();
        assert_eq!(
            item.description.as_deref(),
            Some("Does review with references")
        );
        assert!(!destination.primary_skills().exists());
        copy_item(&source, &destination, Kind::Skills, &item).unwrap();
        let copied = destination.primary_skills().join("assistant");
        assert_eq!(
            fs::read_to_string(copied.join("SKILL.md")).unwrap(),
            instructions
        );
        fs::write(copied.join("SKILL.md"), "owned by user").unwrap();
        assert_eq!(
            copy_item(&source, &destination, Kind::Skills, &item),
            Err("exists")
        );
        assert_eq!(
            fs::read_to_string(copied.join("SKILL.md")).unwrap(),
            "owned by user"
        );
    }

    #[test]
    fn mcp_copy_converts_headers_without_exposing_secrets_or_overwriting_other_settings() {
        let root = tempfile::tempdir().unwrap();
        let source = fixture("claudecode", root.path());
        let dest = fixture("codex", root.path());
        fs::create_dir_all(&source.directory).unwrap();
        fs::create_dir_all(&dest.directory).unwrap();
        fs::write(
            &source.mcp_file,
            json!({"mcpServers":{"remote":{
            "type":"http","url":"https://tools.example.test/mcp",
            "headers":{"Authorization":"Bearer fixture-key"}
        }}, "keep":true})
            .to_string(),
        )
        .unwrap();
        fs::write(
            &dest.mcp_file,
            "# keep comment\nmodel = \"fixture\"\n[mcp_servers.other]\ncommand = \"existing\"\n",
        )
        .unwrap();
        let item = source.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert_eq!(item.description.as_deref(), Some("tools.example.test"));
        assert!(!serde_json::to_string(&item)
            .unwrap()
            .contains("fixture-key"));
        copy_item(&source, &dest, Kind::Mcp, &item).unwrap();
        let written = fs::read_to_string(&dest.mcp_file).unwrap();
        assert!(written.contains("# keep comment"));
        assert!(written.contains("command = \"existing\""));
        assert!(written.contains("Authorization = \"Bearer fixture-key\""));
        assert!(dest
            .assets(Kind::Mcp)
            .unwrap()
            .iter()
            .any(|item| item.name == "remote"));
        assert_eq!(copy_item(&source, &dest, Kind::Mcp, &item), Err("exists"));
        assert_eq!(fs::read_to_string(&dest.mcp_file).unwrap(), written);
        config::toggle(&dest, Kind::Mcp, "remote", false).unwrap();
        assert_eq!(
            dest.assets(Kind::Mcp)
                .unwrap()
                .into_iter()
                .find(|entry| entry.id == "remote")
                .unwrap()
                .enabled,
            Some(false)
        );
        config::remove_mcp(&dest, "remote").unwrap();
        assert_eq!(dest.assets(Kind::Mcp).unwrap().len(), 1);
        assert!(fs::read_to_string(&dest.mcp_file)
            .unwrap()
            .contains("# keep comment"));
    }

    #[test]
    fn antigravity_shares_mcp_and_instructions_but_reads_extra_cli_skills() {
        let cli = App::open("antigravity", Kind::Skills).unwrap();
        let desktop = App::open("antigravitydesktop", Kind::Skills).unwrap();
        assert_eq!(cli.primary_skills(), desktop.primary_skills());
        assert!(cli.skill_roots().len() > desktop.skill_roots().len());

        for kind in [Kind::Mcp, Kind::Memory] {
            let cli = App::open("antigravity", kind).unwrap();
            let desktop = App::open("antigravitydesktop", kind).unwrap();
            assert_eq!(cli.id, desktop.id);
            assert_eq!(cli.mcp_file, desktop.mcp_file);
        }

        let root = tempfile::tempdir().unwrap();
        let app = fixture("antigravitydesktop", root.path());
        let target = fixture("codex", root.path());
        config::add_mcp(&app, "fixture", json!({"url":"https://example.test/mcp"})).unwrap();
        let entry = app.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert!(entry.can_toggle);
        assert_eq!(entry.enabled, Some(true));
        assert!(entry.can_copy);
        let written: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
        assert_eq!(
            written["mcpServers"]["fixture"],
            json!({
                "serverUrl":"https://example.test/mcp", "disabled":false
            })
        );
        config::copy_mcp(&app, &target, "fixture").unwrap();
        config::toggle(&app, Kind::Mcp, "fixture", false).unwrap();
        let entry = app.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert_eq!(entry.enabled, Some(false));
        assert!(!entry.can_copy);
    }

    #[test]
    fn claude_loads_user_rules_and_shared_memory_without_following_project_settings() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("claudecode", root.path());
        let shared = root.path().join("shared-memory");
        let rule = app.directory.join("rules/general/naming.md");
        let agent = app.directory.join("agent-memory/reviewer/MEMORY.md");
        let project = app.directory.join("projects/fixture/memory/topic.md");
        let instructions = app.directory.join("CLAUDE.md");
        for path in [
            shared.join("MEMORY.md"),
            rule.clone(),
            agent.clone(),
            project.clone(),
            instructions.clone(),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "Fixture").unwrap();
        }
        let settings = app.directory.join("settings.json");
        fs::write(&settings, r#"{"autoMemoryDirectory":"~/shared-memory"}"#).unwrap();
        let listed = app.assets(Kind::Memory).unwrap();
        assert_eq!(listed.len(), 5);
        assert!(listed
            .iter()
            .any(|entry| entry.path.as_ref() == Some(&rule) && !entry.can_copy));
        assert!(listed
            .iter()
            .any(|entry| entry.path.as_ref() == Some(&agent)));
        assert!(listed
            .iter()
            .any(|entry| entry.path.as_ref() == Some(&instructions) && entry.can_copy));
        assert!(listed
            .iter()
            .any(|entry| entry.path.as_ref() == Some(&shared.join("MEMORY.md"))));
        fs::write(settings, r#"{"autoMemoryDirectory":"shared-memory"}"#).unwrap();
        assert_eq!(app.assets(Kind::Memory).unwrap().len(), 4);
    }

    #[test]
    fn pi_lists_local_and_shared_skills_extensions_and_instruction_precedence() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("pi", root.path());
        for (path, name) in [
            (app.primary_skills().join("nested/skill/SKILL.md"), "nested"),
            (app.primary_skills().join("flat.md"), "flat"),
            (root.path().join(".agents/skills/shared/SKILL.md"), "shared"),
        ] {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(
                path,
                format!("---\nname: {name}\ndescription: A fixture\n---\n"),
            )
            .unwrap();
        }
        let skills = app.assets(Kind::Skills).unwrap();
        assert_eq!(skills.len(), 3);
        assert!(skills
            .iter()
            .any(|item| item.name == "nested" && item.can_copy && item.can_remove));
        assert!(skills
            .iter()
            .any(|item| item.name == "shared" && item.can_copy && !item.can_remove));
        assert!(skills
            .iter()
            .any(|item| item.name == "flat" && !item.can_remove));

        let extensions = app.directory.join("extensions");
        fs::create_dir_all(extensions.join("bundle")).unwrap();
        fs::write(extensions.join("bundle/index.ts"), "").unwrap();
        fs::write(extensions.join("local.js"), "").unwrap();
        fs::write(extensions.join("README.md"), "").unwrap();
        let plugins = app.assets(Kind::Plugins).unwrap();
        assert_eq!(plugins.len(), 2);
        assert!(plugins.iter().all(|entry| !entry.can_toggle));
        fs::write(app.directory.join("AGENTS.md"), "Default").unwrap();
        fs::write(app.directory.join("AGENTS.override.md"), "Override").unwrap();
        let memory = app.assets(Kind::Memory).unwrap();
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[0].name, "AGENTS.override.md");
    }

    #[test]
    fn skill_archive_uses_the_source_folder_instead_of_the_display_name() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("pi", root.path());
        let skill = app.primary_skills().join("skill-folder");
        fs::create_dir_all(&skill).unwrap();
        let document = "---\nname: nested/display-name\ndescription: A fixture\n---\n";
        fs::write(skill.join("SKILL.md"), document).unwrap();
        let item = app.assets(Kind::Skills).unwrap().pop().unwrap();
        assert_eq!(item.name, "nested/display-name");
        assert!(item.can_remove);
        archive_skill(&app.directory, item.path.as_deref().unwrap()).unwrap();
        let backups = children(&app.directory.join("echobird-skill-backups")).unwrap();
        assert_eq!(backups.len(), 1);
        assert!(backups[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("skill-folder-"));
        assert_eq!(
            fs::read_to_string(backups[0].join("SKILL.md")).unwrap(),
            document
        );
        assert!(!skill.exists());
    }

    #[test]
    fn pi_mcp_matches_native_namespace_transport_and_activation() {
        let root = tempfile::tempdir().unwrap();
        let pi = fixture("pi", root.path());
        let codex = fixture("codex", root.path());
        config::add_mcp(
            &pi,
            "fixture-http",
            json!({"url":"https://example.test/mcp"}),
        )
        .unwrap();
        assert_eq!(
            config::add_mcp(&pi, "fixture_http", json!({"command":"runner"})),
            Err("exists")
        );
        assert_eq!(
            config::add_mcp(
                &pi,
                "sse",
                json!({"type":"sse", "url":"https://example.test/sse"})
            ),
            Err("unsupported")
        );
        let cursor = fixture("cursor", root.path());
        fs::create_dir_all(&cursor.directory).unwrap();
        fs::write(
            &cursor.mcp_file,
            r#"{"mcpServers":{"legacy name":{"command":"runner"}}}"#,
        )
        .unwrap();
        assert_eq!(
            config::copy_mcp(&cursor, &pi, "legacy name"),
            Err("invalidName")
        );
        config::copy_mcp(&pi, &codex, "fixture-http").unwrap();
        config::toggle(&pi, Kind::Mcp, "fixture-http", false).unwrap();
        let entry = pi.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert_eq!(entry.enabled, Some(false));
        assert!(entry.can_toggle);
        assert!(!entry.can_copy);
        let native: Value = serde_json::from_slice(&fs::read(&pi.mcp_file).unwrap()).unwrap();
        assert_eq!(native["mcpServers"]["fixture-http"]["enabled"], false);
        assert_eq!(
            native["mcpServers"]["fixture-http"]["url"],
            "https://example.test/mcp"
        );
        fs::write(&pi.mcp_file, json!({"mcpServers": {"native": {"type":"streamable-http", "url":"https://example.test/mcp", "headers":{"Authorization":"!fixture"}}}}).to_string()).unwrap();
        assert!(!pi.assets(Kind::Mcp).unwrap()[0].can_copy);
        assert_eq!(config::copy_mcp(&pi, &codex, "native"), Err("unsupported"));
    }

    #[test]
    fn qoder_mcp_preserves_shared_settings_and_writes_native_disabled_flag() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("qoder", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        fs::write(&app.mcp_file, r#"{"keep":{"value":42}}"#).unwrap();
        config::add_mcp(&app, "fixture", json!({"url":"https://example.test/mcp"})).unwrap();
        let listed = app.assets(Kind::Mcp).unwrap();
        assert!(listed[0].can_toggle && listed[0].can_copy);
        config::toggle(&app, Kind::Mcp, "fixture", false).unwrap();
        assert_eq!(app.assets(Kind::Mcp).unwrap()[0].enabled, Some(false));
        let native: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
        assert_eq!(native["mcpServers"]["fixture"]["disabled"], true);
        assert_eq!(native["keep"]["value"], 42);
    }

    #[test]
    fn qoder_mcp_preserves_comments_in_both_editions() {
        let root = tempfile::tempdir().unwrap();
        for id in ["qoder", "qodercn"] {
            let app = fixture(id, root.path());
            fs::create_dir_all(&app.directory).unwrap();
            let original = concat!(
                "\u{feff}",
                r#"{
                    // existing model settings
                    "keep": 42,
                    "mcpServers": {
                        "original": {
                            // existing server command
                            "command": "example",
                        },
                    },
                }"#
            );
            fs::write(&app.mcp_file, original).unwrap();
            assert_eq!(app.assets(Kind::Mcp).unwrap()[0].id, "original");
            config::add_mcp(&app, "new", json!({"command":"example"})).unwrap();
            config::toggle(&app, Kind::Mcp, "original", false).unwrap();
            config::remove_mcp(&app, "new").unwrap();
            let entries = app.assets(Kind::Mcp).unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].enabled, Some(false));
            let updated = fs::read_to_string(&app.mcp_file).unwrap();
            assert!(updated.starts_with('\u{feff}'));
            assert!(updated.contains("// existing model settings"));
            assert!(updated.contains("// existing server command"));
        }
    }

    #[test]
    fn dsh_reads_user_instructions() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("dsh", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        let instructions = app.directory.join("AGENTS.md");
        fs::write(&instructions, "Global instructions").unwrap();
        let memory = app.assets(Kind::Memory).unwrap();
        assert_eq!(memory.len(), 1);
        assert_eq!(memory[0].path.as_ref(), Some(&instructions));
        assert!(memory[0].can_copy);
    }

    #[test]
    fn dsh_mcp_preserves_native_profile_content() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("dsh", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        let original = r#"# Native profile
- id: example
  config:
    expression: !!js "1 + 1"
- insert:
  - id: native-mcp
    name: '@deepseek-ai/dsh-mcp-client'
    config:
      serverName: native
      transport: stdio
      command: fixture-native
"#;
        fs::write(&app.mcp_file, original).unwrap();
        let listed = app.assets(Kind::Mcp).unwrap();
        assert_eq!(listed[0].name, "native");
        assert!(listed[0].can_copy);
        assert!(!listed[0].can_remove);
        assert_eq!(
            config::add_mcp(&app, "native", json!({"command":"other"})),
            Err("exists")
        );
        config::add_mcp(&app, "echo", json!({"url":"https://example.test/mcp"})).unwrap();
        let written = fs::read_to_string(&app.mcp_file).unwrap();
        assert!(written.starts_with(original));
        let listed = app.assets(Kind::Mcp).unwrap();
        assert_eq!(listed[0].name, "echo");
        assert!(listed[0].can_remove);
        assert!(listed[0].can_copy);
        assert!(config::remove_mcp(&app, "native").is_err());
        config::remove_mcp(&app, "echo").unwrap();
        assert_eq!(fs::read_to_string(&app.mcp_file).unwrap(), original);

        config::add_mcp(&app, "echo", json!({"command":"runner"})).unwrap();
        let profile = super::super::tool_config_manager::dsh::parse_yaml(
            &fs::read_to_string(&app.mcp_file).unwrap(),
        )
        .unwrap();
        super::super::tool_config_manager::dsh::write_private_yaml(&app.mcp_file, &profile)
            .unwrap();
        assert!(app.assets(Kind::Mcp).unwrap()[0].can_remove);
        config::remove_mcp(&app, "echo").unwrap();
        assert_eq!(app.assets(Kind::Mcp).unwrap()[0].name, "native");
    }

    #[test]
    fn dsh_adds_the_first_mcp_to_an_empty_commented_native_profile() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("dsh", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        for eol in ["\n", "\r\n"] {
            let comments = format!("# Profile patch{eol}# Add additional operations below.{eol}");
            let original = format!("{comments}[]{eol}");
            fs::write(&app.mcp_file, &original).unwrap();

            config::add_mcp(&app, "echo", json!({"command":"runner"})).unwrap();
            assert_eq!(app.assets(Kind::Mcp).unwrap()[0].name, "echo");
            assert!(fs::read_to_string(&app.mcp_file)
                .unwrap()
                .starts_with(&comments));

            config::remove_mcp(&app, "echo").unwrap();
            assert_eq!(fs::read_to_string(&app.mcp_file).unwrap(), original);
        }

        let original = "\u{feff}[]\r\n";
        fs::write(&app.mcp_file, original).unwrap();
        config::add_mcp(&app, "echo", json!({"command":"runner"})).unwrap();
        assert!(fs::read_to_string(&app.mcp_file)
            .unwrap()
            .starts_with('\u{feff}'));
        config::remove_mcp(&app, "echo").unwrap();
        assert_eq!(fs::read_to_string(&app.mcp_file).unwrap(), original);
    }

    #[test]
    fn kimi_mcp_switch_preserves_the_server_definition() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("kimicode", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        let server = json!({"command":"runner","args":["fixture"],"cwd":"/example"});
        fs::write(
            &app.mcp_file,
            json!({"mcpServers":{"example":server},"keep":true}).to_string(),
        )
        .unwrap();

        let entry = app.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert_eq!(entry.enabled, Some(true));
        assert!(entry.can_toggle);
        config::toggle(&app, Kind::Mcp, "example", false).unwrap();
        let entry = app.assets(Kind::Mcp).unwrap().pop().unwrap();
        assert_eq!(entry.enabled, Some(false));
        assert!(!entry.can_copy);
        config::toggle(&app, Kind::Mcp, "example", true).unwrap();
        assert_eq!(app.assets(Kind::Mcp).unwrap()[0].enabled, Some(true));
        let updated: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
        assert_eq!(
            updated["mcpServers"]["example"]["command"],
            server["command"]
        );
        assert_eq!(updated["mcpServers"]["example"]["args"], server["args"]);
        assert_eq!(updated["mcpServers"]["example"]["cwd"], server["cwd"]);
        assert_eq!(updated["keep"], true);
    }

    #[test]
    fn connector_owned_and_invalid_mcp_config_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let app = fixture("workbuddy", root.path());
        fs::create_dir_all(&app.directory).unwrap();
        let config = json!({"keep":true,"mcpServers":{
            "connector":{"managedBy":"connector","url":"https://example.test"},
            "regular":{"command":"fixture","disabled":false}
        }});
        fs::write(&app.mcp_file, config.to_string()).unwrap();
        assert!(config::remove_mcp(&app, "connector").is_err());
        assert!(config::toggle(&app, Kind::Mcp, "connector", false).is_err());
        config::toggle(&app, Kind::Mcp, "regular", false).unwrap();
        let updated: Value = serde_json::from_slice(&fs::read(&app.mcp_file).unwrap()).unwrap();
        assert_eq!(
            updated["mcpServers"]["connector"],
            config["mcpServers"]["connector"]
        );
        assert_eq!(updated["mcpServers"]["regular"]["disabled"], true);
        fs::write(&app.mcp_file, "{malformed").unwrap();
        assert_eq!(
            config::add_mcp(&app, "new", json!({"command":"fixture"})),
            Err("invalidConfig")
        );
        assert_eq!(fs::read_to_string(&app.mcp_file).unwrap(), "{malformed");
    }

    #[test]
    fn plugin_cards_describe_native_installations_and_keep_unrelated_options() {
        let root = tempfile::tempdir().unwrap();
        let codex = fixture("codex", root.path());
        let plugin = codex.directory.join("plugins/cache/test/demo/1.2.3");
        fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
        fs::write(
            plugin.join(".codex-plugin/plugin.json"),
            r#"{"name":"Demo","version":"1.2.3"}"#,
        )
        .unwrap();
        let config = "# keep\n[plugins.\"demo@test\"]\nenabled = true\nsetting = \"preserve\"\n[plugins.\"missing@test\"]\nenabled = true\n";
        fs::write(&codex.mcp_file, config).unwrap();
        let entries = codex.assets(Kind::Plugins).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Demo");
        assert_eq!(entries[0].path.as_deref(), Some(plugin.as_path()));
        config::toggle(&codex, Kind::Plugins, &entries[0].id, false).unwrap();
        assert_eq!(codex.assets(Kind::Plugins).unwrap()[0].enabled, Some(false));
        assert!(fs::read_to_string(&codex.mcp_file)
            .unwrap()
            .contains("setting = \"preserve\""));
    }
}
