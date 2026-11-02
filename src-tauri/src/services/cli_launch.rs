//! Per-tool launch choices for CLI clients: command preset, working directory
//! and the terminal that hosts the client. Saved here and read at launch, so
//! the settings dialog itself never applies a model or starts anything.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const AUTO_TERMINAL: &str = "auto";
pub const COFFEE_TERMINAL: &str = "coffee-cli";
const COFFEE_TOOL_ID: &str = "coffeecli";
const FIXED_DIRECTORY: &str = "fixed";
const ASK_DIRECTORY: &str = "ask";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliLaunchSettings {
    pub preset: String,
    pub directory_mode: String,
    pub directory: String,
    pub terminal: String,
}

impl Default for CliLaunchSettings {
    fn default() -> Self {
        Self {
            preset: String::new(),
            directory_mode: ASK_DIRECTORY.into(),
            directory: String::new(),
            terminal: AUTO_TERMINAL.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CommandPreset {
    pub id: &'static str,
    pub command: &'static str,
}

const fn preset(id: &'static str, command: &'static str) -> CommandPreset {
    CommandPreset { id, command }
}

const CLAUDE_PRESETS: &[CommandPreset] = &[
    preset("plan", "claude --permission-mode plan"),
    preset("acceptEdits", "claude --permission-mode acceptEdits"),
    preset("bypass", "claude --dangerously-skip-permissions"),
];
const CODEX_PRESETS: &[CommandPreset] = &[
    preset(
        "readOnly",
        "codex -c sandbox_mode=read-only -c approval_policy=on-request",
    ),
    preset(
        "workspace",
        "codex -c sandbox_mode=workspace-write -c approval_policy=on-request",
    ),
    preset(
        "fullAccess",
        "codex -c sandbox_mode=danger-full-access -c approval_policy=never",
    ),
];
const OPENCODE_PRESETS: &[CommandPreset] = &[preset("bypass", "opencode --auto")];

fn presets_for(tool_id: &str) -> &'static [CommandPreset] {
    match tool_id {
        "claudecode" => CLAUDE_PRESETS,
        "codex" => CODEX_PRESETS,
        "opencode" => OPENCODE_PRESETS,
        _ => &[],
    }
}

/// EchoBird tool ids that Coffee CLI can open and that keep their selected model
/// in native files. Tools needing launch-time flags or environment stay out:
/// an already running Coffee CLI receives neither.
const COFFEE_TOOLS: &[(&str, &str)] = &[
    ("antigravity", "antigravity"),
    ("claudecode", "claude"),
    ("cline", "cline"),
    ("codex", "codex"),
    ("copilot", "copilot"),
    ("cursorcli", "cursor"),
    ("grok", "grok"),
    ("kimicode", "kimicode"),
    ("kirocli", "kiro"),
    ("minimaxcode", "minimax"),
    ("omp", "omp"),
    ("openclaw", "openclaw"),
    ("opencode", "opencode"),
    ("pi", "pi"),
    ("qwencode", "qwen"),
];

pub fn coffee_tool_id(tool_id: &str) -> Option<&'static str> {
    COFFEE_TOOLS
        .iter()
        .find(|(echobird, _)| *echobird == tool_id)
        .map(|(_, coffee)| *coffee)
}

fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".echobird")
        .join("config")
        .join("cli-launch.json")
}

fn load_from(path: &Path) -> HashMap<String, CliLaunchSettings> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn load() -> HashMap<String, CliLaunchSettings> {
    load_from(&settings_path())
}

fn validate(
    settings: &HashMap<String, CliLaunchSettings>,
    terminals: &[&str],
) -> Result<(), String> {
    for (tool_id, entry) in settings {
        if !entry.preset.is_empty()
            && !presets_for(tool_id)
                .iter()
                .any(|preset| preset.id == entry.preset)
        {
            return Err(format!("Unknown launch command for {tool_id}"));
        }
        match entry.directory_mode.as_str() {
            ASK_DIRECTORY => {}
            FIXED_DIRECTORY if !entry.directory.trim().is_empty() => {}
            _ => return Err(format!("Invalid launch directory for {tool_id}")),
        }
        let coffee = entry.terminal == COFFEE_TERMINAL && coffee_tool_id(tool_id).is_some();
        if !coffee && !terminals.contains(&entry.terminal.as_str()) {
            return Err(format!("Unknown terminal for {tool_id}"));
        }
    }
    Ok(())
}

fn save_to(path: &Path, settings: &HashMap<String, CliLaunchSettings>) -> Result<(), String> {
    validate(settings, SUPPORTED_TERMINALS)?;
    let parent = path.parent().ok_or("Invalid CLI launch settings path")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("Failed to save settings: {e}"))?;
    let body = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    let staging = path.with_extension("json.tmp");
    std::fs::write(&staging, body).map_err(|e| format!("Failed to save settings: {e}"))?;
    std::fs::rename(&staging, path).map_err(|e| format!("Failed to save settings: {e}"))
}

pub fn save(settings: &HashMap<String, CliLaunchSettings>) -> Result<(), String> {
    save_to(&settings_path(), settings)
}

/// What a launch actually uses once the saved choices are applied.
#[derive(Debug, PartialEq, Eq)]
pub struct ResolvedLaunch {
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub terminal: String,
}

fn resolve_entry(
    tool_id: &str,
    entry: &CliLaunchSettings,
    default_command: Option<&str>,
    start_command: Option<&str>,
    cwd: Option<&str>,
) -> Result<ResolvedLaunch, String> {
    // Coffee CLI opens a tool by id and applies its own launch settings.
    let preset = (entry.terminal != COFFEE_TERMINAL)
        .then(|| {
            presets_for(tool_id)
                .iter()
                .find(|preset| preset.id == entry.preset)
        })
        .flatten();
    // Only the catalog command is replaceable, never a caller's specific one.
    let replaceable = start_command.is_none() || start_command == default_command;
    let command = match preset {
        Some(preset) if replaceable => Some(preset.command.to_string()),
        _ => start_command.map(str::to_string),
    };
    let cwd = match (cwd, entry.directory_mode.as_str()) {
        (Some(cwd), _) => Some(cwd.to_string()),
        (None, FIXED_DIRECTORY) => {
            // Never fall back to home here: a saved folder scopes what a
            // bypass-permissions session can touch.
            if !Path::new(&entry.directory).is_dir() {
                return Err(format!("Launch directory not found: {}", entry.directory));
            }
            Some(entry.directory.clone())
        }
        (None, _) => None,
    };
    Ok(ResolvedLaunch {
        command,
        cwd,
        terminal: entry.terminal.clone(),
    })
}

pub fn resolve(
    tool_id: &str,
    start_command: Option<&str>,
    cwd: Option<&str>,
) -> Result<ResolvedLaunch, String> {
    let entry = load().remove(tool_id).unwrap_or_default();
    let default_command = crate::services::tool_manager::get_tool_start_command(tool_id);
    resolve_entry(
        tool_id,
        &entry,
        default_command.as_deref(),
        start_command,
        cwd,
    )
}

#[cfg(windows)]
const SUPPORTED_TERMINALS: &[&str] = &[AUTO_TERMINAL, "pwsh", "powershell", "git-bash", "cmd"];
#[cfg(target_os = "macos")]
const SUPPORTED_TERMINALS: &[&str] = &[
    AUTO_TERMINAL,
    "iterm2",
    "ghostty",
    "kitty",
    "wezterm",
    "alacritty",
];
#[cfg(target_os = "linux")]
const SUPPORTED_TERMINALS: &[&str] = &[
    AUTO_TERMINAL,
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "ghostty",
    "kitty",
    "wezterm",
    "alacritty",
];
#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
const SUPPORTED_TERMINALS: &[&str] = &[AUTO_TERMINAL];

/// Choices the dialog offers: only what this machine can actually start.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliLaunchOptions {
    pub terminals: Vec<&'static str>,
    pub presets: HashMap<&'static str, &'static [CommandPreset]>,
    pub coffee_tools: Vec<&'static str>,
}

pub fn options() -> CliLaunchOptions {
    let coffee_installed =
        crate::services::tool_manager::get_tool_exe_path(COFFEE_TOOL_ID).is_some();
    CliLaunchOptions {
        terminals: SUPPORTED_TERMINALS
            .iter()
            .copied()
            .filter(|id| *id == AUTO_TERMINAL || shell_available(id))
            .collect(),
        presets: ["claudecode", "codex", "opencode"]
            .into_iter()
            .map(|tool_id| (tool_id, presets_for(tool_id)))
            .collect(),
        coffee_tools: if coffee_installed {
            COFFEE_TOOLS.iter().map(|(tool_id, _)| *tool_id).collect()
        } else {
            Vec::new()
        },
    }
}

/// Open `tool_id` in Coffee CLI. A running instance receives the request through
/// its single-instance handler, so this process normally exits right away.
pub fn start_in_coffee(tool_id: &str, cwd: Option<&str>) -> Result<u32, String> {
    let coffee_id =
        coffee_tool_id(tool_id).ok_or_else(|| format!("Coffee CLI cannot open {tool_id}"))?;
    let executable = crate::services::tool_manager::get_tool_exe_path(COFFEE_TOOL_ID)
        .ok_or("Coffee CLI is not installed")?;
    let mut command = crate::utils::process::command(executable);
    command.args(coffee_launch_arguments(coffee_id, cwd));
    let child = command
        .spawn()
        .map_err(|e| format!("Failed to open Coffee CLI: {e}"))?;
    Ok(child.id())
}

fn coffee_launch_arguments(coffee_id: &str, cwd: Option<&str>) -> Vec<String> {
    let mut arguments = vec!["launch".to_string(), "--tool".into(), coffee_id.into()];
    if let Some(cwd) = cwd {
        arguments.extend(["--cwd".to_string(), cwd.to_string()]);
    }
    arguments
}

#[cfg(windows)]
fn is_launchable_file(path: &Path) -> bool {
    // App Execution Aliases report zero bytes, so existence is the only test;
    // symlink metadata keeps the reparse point itself from being followed.
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(windows)]
fn find_pwsh() -> Option<PathBuf> {
    let env_path = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut candidates = Vec::new();
    if let Some(root) = env_path("ProgramFiles") {
        candidates.push(root.join("PowerShell/7/pwsh.exe"));
        candidates.push(root.join("PowerShell/7-preview/pwsh.exe"));
    }
    if let Some(root) = env_path("LOCALAPPDATA") {
        candidates.push(root.join("Programs/PowerShell/7/pwsh.exe"));
        candidates.push(root.join("Microsoft/WindowsApps/pwsh.exe"));
    }
    if let Some(root) = env_path("USERPROFILE") {
        candidates.push(root.join(".dotnet/tools/pwsh.exe"));
    }
    candidates
        .into_iter()
        .find(|path| is_launchable_file(path))
        .or_else(|| which::which("pwsh").ok())
}

#[cfg(windows)]
fn find_git_bash() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for name in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(root) = std::env::var_os(name) {
            candidates.push(PathBuf::from(root).join("Git/bin/bash.exe"));
        }
    }
    if let Some(root) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(root).join("Programs/Git/bin/bash.exe"));
    }
    // Derive from git.exe rather than `which bash`, which finds WSL's launcher.
    if let Some(install) = which::which("git")
        .ok()
        .and_then(|git| Some(git.parent()?.parent()?.to_path_buf()))
    {
        candidates.push(install.join("bin/bash.exe"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

#[cfg(windows)]
pub enum WindowsShell {
    PowerShell(PathBuf),
    Cmd(PathBuf),
    GitBash(PathBuf),
}

#[cfg(windows)]
impl WindowsShell {
    pub fn program(&self) -> &Path {
        match self {
            Self::PowerShell(path) | Self::Cmd(path) | Self::GitBash(path) => path,
        }
    }

    /// Command line for the shell. The tool command and folder travel in the
    /// child's environment, so they need no second round of quoting here.
    pub fn arguments(&self, command: &str) -> String {
        match self {
            Self::PowerShell(_) => "-NoLogo -NoExit -ExecutionPolicy RemoteSigned -Command \
                \"Set-Location -LiteralPath $env:ECHOBIRD_CLI_CWD; \
                Invoke-Expression $env:ECHOBIRD_CLI_COMMAND\""
                .to_string(),
            Self::Cmd(_) => format!("/D /S /K \"{command}\""),
            Self::GitBash(_) => "--login -i -c \"cd \\\"$ECHOBIRD_CLI_CWD\\\"; \
                eval \\\"$ECHOBIRD_CLI_COMMAND\\\"; exec bash --login -i\""
                .to_string(),
        }
    }

    pub fn quote(&self, value: &str) -> String {
        match self {
            Self::PowerShell(_) => format!("'{}'", value.replace('\'', "''")),
            Self::Cmd(_) => format!("\"{value}\""),
            Self::GitBash(_) => format!("'{}'", value.replace('\\', "/").replace('\'', "'\\''")),
        }
    }

    /// `quote`, plus the call operator PowerShell needs for a quoted program.
    pub fn program_line(&self, program: &str) -> String {
        match self {
            Self::PowerShell(_) => format!("& {}", self.quote(program)),
            _ => self.quote(program),
        }
    }
}

#[cfg(windows)]
pub fn windows_shell(terminal: &str) -> Result<Option<WindowsShell>, String> {
    let missing = || format!("Terminal not found: {terminal}");
    let system_root = std::env::var_os("SystemRoot").map(PathBuf::from);
    match terminal {
        AUTO_TERMINAL => Ok(None),
        "pwsh" => find_pwsh()
            .map(|path| Some(WindowsShell::PowerShell(path)))
            .ok_or_else(missing),
        "powershell" => Ok(Some(WindowsShell::PowerShell(
            system_root
                .as_ref()
                .map(|root| root.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
                .unwrap_or_else(|| "powershell.exe".into()),
        ))),
        "cmd" => Ok(Some(WindowsShell::Cmd(
            system_root
                .as_ref()
                .map(|root| root.join("System32/cmd.exe"))
                .unwrap_or_else(|| "cmd.exe".into()),
        ))),
        "git-bash" => find_git_bash()
            .map(|path| Some(WindowsShell::GitBash(path)))
            .ok_or_else(missing),
        _ => Err(missing()),
    }
}

#[cfg(windows)]
fn shell_available(id: &str) -> bool {
    matches!(windows_shell(id), Ok(Some(_)))
}

#[cfg(target_os = "linux")]
fn shell_available(id: &str) -> bool {
    which::which(id).is_ok()
}

#[cfg(all(unix, not(target_os = "macos"), not(target_os = "linux")))]
fn shell_available(_id: &str) -> bool {
    false
}

/// macOS application name and the flags that precede the launch command.
#[cfg(target_os = "macos")]
pub fn macos_terminal_app(id: &str) -> Option<(&'static str, &'static [&'static str])> {
    match id {
        "iterm2" => Some(("iTerm", &[])),
        "ghostty" => Some(("Ghostty", &["-e"])),
        "kitty" => Some(("kitty", &[])),
        "wezterm" => Some(("WezTerm", &["-e"])),
        "alacritty" => Some(("Alacritty", &["-e"])),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn shell_available(id: &str) -> bool {
    let Some((name, _)) = macos_terminal_app(id) else {
        return false;
    };
    let bundle = format!("{name}.app");
    let user_apps = dirs::home_dir().unwrap_or_default().join("Applications");
    Path::new("/Applications").join(&bundle).is_dir() || user_apps.join(&bundle).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(preset: &str, mode: &str, directory: &str, terminal: &str) -> CliLaunchSettings {
        CliLaunchSettings {
            preset: preset.into(),
            directory_mode: mode.into(),
            directory: directory.into(),
            terminal: terminal.into(),
        }
    }

    #[test]
    fn presets_replace_only_the_catalog_command() {
        let settings = entry("bypass", ASK_DIRECTORY, "", AUTO_TERMINAL);
        let resolved = |start| {
            resolve_entry("claudecode", &settings, Some("claude"), start, None)
                .unwrap()
                .command
        };
        let bypass = Some("claude --dangerously-skip-permissions".to_string());
        assert_eq!(resolved(None), bypass);
        assert_eq!(resolved(Some("claude")), bypass);
        assert_eq!(
            resolved(Some("claude --resume abc")).as_deref(),
            Some("claude --resume abc")
        );
    }

    #[test]
    fn unconfigured_tools_keep_their_launch_unchanged() {
        let resolved = resolve_entry(
            "aider",
            &CliLaunchSettings::default(),
            Some("aider"),
            Some("aider"),
            Some("D:/picked"),
        )
        .unwrap();
        assert_eq!(
            resolved,
            ResolvedLaunch {
                command: Some("aider".into()),
                cwd: Some("D:/picked".into()),
                terminal: AUTO_TERMINAL.into(),
            }
        );
    }

    #[test]
    fn coffee_keeps_its_own_command_settings() {
        let settings = entry("bypass", ASK_DIRECTORY, "", COFFEE_TERMINAL);
        let resolved = resolve_entry(
            "claudecode",
            &settings,
            Some("claude"),
            Some("claude"),
            None,
        )
        .unwrap();
        assert_eq!(resolved.command.as_deref(), Some("claude"));
        assert_eq!(resolved.terminal, COFFEE_TERMINAL);
    }

    #[test]
    fn fixed_directory_is_used_and_a_missing_one_is_an_error() {
        let dir = std::env::temp_dir();
        let dir = dir.to_string_lossy();
        let fixed = entry("", FIXED_DIRECTORY, &dir, AUTO_TERMINAL);
        let launch = |cwd| resolve_entry("codex", &fixed, Some("codex"), None, cwd);
        assert_eq!(launch(None).unwrap().cwd.as_deref(), Some(dir.as_ref()));
        assert_eq!(
            launch(Some("D:/picked")).unwrap().cwd.as_deref(),
            Some("D:/picked")
        );

        let missing = entry("", FIXED_DIRECTORY, "Z:/echobird-missing", AUTO_TERMINAL);
        let error = resolve_entry("codex", &missing, Some("codex"), None, None).unwrap_err();
        assert!(error.contains("Z:/echobird-missing"));
    }

    #[test]
    fn saved_settings_round_trip_and_invalid_ones_leave_the_file_alone() {
        let dir = std::env::temp_dir().join(format!("echobird-cli-{}", uuid::Uuid::new_v4()));
        let path = dir.join("config/cli-launch.json");
        let settings = HashMap::from([(
            "claudecode".to_string(),
            entry("plan", FIXED_DIRECTORY, "D:/work", COFFEE_TERMINAL),
        )]);
        save_to(&path, &settings).unwrap();
        assert_eq!(load_from(&path), settings);

        for (tool_id, invalid) in [
            (
                "claudecode",
                entry("nope", ASK_DIRECTORY, "", AUTO_TERMINAL),
            ),
            ("aider", entry("plan", ASK_DIRECTORY, "", AUTO_TERMINAL)),
            ("codex", entry("", FIXED_DIRECTORY, " ", AUTO_TERMINAL)),
            ("codex", entry("", "sometimes", "", AUTO_TERMINAL)),
            ("codex", entry("", ASK_DIRECTORY, "", "nope")),
            ("aider", entry("", ASK_DIRECTORY, "", COFFEE_TERMINAL)),
        ] {
            let attempt = HashMap::from([(tool_id.to_string(), invalid)]);
            assert!(save_to(&path, &attempt).is_err());
            assert_eq!(load_from(&path), settings);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_or_corrupt_settings_mean_defaults() {
        let dir = std::env::temp_dir().join(format!("echobird-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cli-launch.json");
        assert!(load_from(&path).is_empty());
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_from(&path).is_empty());
        std::fs::write(&path, r#"{"codex":{"preset":"workspace"}}"#).unwrap();
        let loaded = load_from(&path).remove("codex").unwrap();
        assert_eq!(loaded.directory_mode, ASK_DIRECTORY);
        assert_eq!(loaded.terminal, AUTO_TERMINAL);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn coffee_launch_uses_its_own_tool_ids() {
        assert_eq!(
            coffee_launch_arguments(coffee_tool_id("claudecode").unwrap(), Some("D:/a b")),
            ["launch", "--tool", "claude", "--cwd", "D:/a b"]
        );
        assert_eq!(
            coffee_launch_arguments(coffee_tool_id("qwencode").unwrap(), None),
            ["launch", "--tool", "qwen"]
        );
        for excluded in ["aider", "kilo", "mimocode", "musecode", "devincli"] {
            assert_eq!(coffee_tool_id(excluded), None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_shells_quote_a_literal_program_for_their_own_syntax() {
        let path = r"C:\Program Files\it's\tool.exe";
        assert_eq!(
            WindowsShell::PowerShell("pwsh".into()).program_line(path),
            r"& 'C:\Program Files\it''s\tool.exe'"
        );
        assert_eq!(
            WindowsShell::Cmd("cmd".into()).program_line(path),
            r#""C:\Program Files\it's\tool.exe""#
        );
        assert_eq!(
            WindowsShell::GitBash("bash".into()).program_line(path),
            r"'C:/Program Files/it'\''s/tool.exe'"
        );
    }

    #[cfg(windows)]
    #[test]
    fn inbox_windows_shells_are_always_offered_with_auto_first() {
        let terminals = options().terminals;
        assert_eq!(terminals.first(), Some(&AUTO_TERMINAL));
        assert!(terminals.contains(&"powershell"));
        assert!(terminals.contains(&"cmd"));
    }

    #[cfg(not(windows))]
    #[test]
    fn unix_platforms_never_offer_windows_shells() {
        let terminals = options().terminals;
        assert_eq!(terminals.first(), Some(&AUTO_TERMINAL));
        assert!(!terminals.contains(&"cmd"));
        assert!(!terminals.contains(&"powershell"));
    }
}
