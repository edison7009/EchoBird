// Tool & Model data structures �?mirrors tools/types.ts + loader.ts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── paths.json data structure (tool detection & metadata) ───

/// Platform-specific candidate paths for tool detection
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformPaths {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub win32: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub darwin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<String>>,
}

/// Skills path configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillsPathConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub win32: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub darwin: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linux: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub npm_module: Option<String>,
}

/// paths.json �?tool metadata + detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<String, String>>,
    pub category: String,
    #[serde(default)]
    pub api_protocol: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(default)]
    pub docs: String,
    #[serde(default)]
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env_var: Option<String>,
    #[serde(default)]
    pub config_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_dir_alt: Option<String>,
    #[serde(default)]
    pub config_file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_file_alt: Option<String>,
    #[serde(default)]
    pub require_config_file: bool,
    #[serde(default)]
    pub detect_by_config_dir: bool,
    #[serde(default)]
    pub paths: PlatformPaths,
    /// Optional platform-specific process image names used for desktop
    /// kill-and-restart when the installed filename differs from the running
    /// image (for example, an AppImage launching an internal executable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_names: Option<PlatformPaths>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_path: Option<SkillsPathConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_skills_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_paths: Option<PlatformPaths>,
    #[serde(default)]
    pub always_installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_app_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub launchable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_file: Option<String>,
    #[serde(default)]
    pub no_model_config: bool,
    /// Skip the working-directory folder picker on launch and run the CLI
    /// directly (spawns in the user home, like the pre-picker behavior).
    /// Set for CLI tools that don't need a project cwd to be useful (e.g.
    /// openclaw, a self-contained agent). Most "CLI Code" tools still prompt,
    /// because spawning in ~/ made them useless for development — keep this
    /// false unless the tool genuinely doesn't care about its working dir.
    #[serde(default)]
    pub no_cwd_prompt: bool,
    /// Optional shell URI (e.g. "shell:AppsFolder\\Claude_pzs8sxrjxfjjc!Claude")
    /// used to launch MSIX/Store apps that have no plain .exe path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension_id: Option<String>,
    /// Python module name for pip-installed tools (detected via `python -m <module>`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub python_module: Option<String>,
    /// Platform-specific hints for detecting installs at non-standard paths.
    /// Used by detect_install_path AFTER the hardcoded paths.json fall through —
    /// Windows scans the registry Uninstall keys, macOS uses mdfind /
    /// /Applications, Linux scans .desktop files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_hints: Option<InstallHints>,
}

/// Platform-specific install detection hints. Each field is checked only on
/// its target OS; unrelated fields on other OSes are ignored.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InstallHints {
    /// Windows: match against `DisplayName` in
    /// HKLM/HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*.
    /// Multiple entries are OR-matched (e.g. "Editor CN", "Editor-CN").
    /// EXACT (case-insensitive) — see `windows_display_name_prefixes` for
    /// apps whose DisplayName embeds a version.
    #[serde(default)]
    pub windows_display_names: Vec<String>,
    /// Windows: like `windows_display_names` but PREFIX-matched, for apps
    /// whose registry DisplayName embeds a version (e.g. "WorkBuddy 4.24.2").
    /// A DisplayName matches when it equals the prefix OR starts with
    /// `<prefix> ` (prefix + a space), so the trailing version is tolerated
    /// without enumerating every release. Opt-in per tool — exact
    /// `windows_display_names` stays the safe default to avoid a base product
    /// matching a regional edition. Pair with `windows_publisher` for
    /// extra disambiguation.
    #[serde(default)]
    pub windows_display_name_prefixes: Vec<String>,
    /// Optional Windows `Publisher` filter (e.g. "Bytedance") for
    /// disambiguation when display names alone could match unrelated apps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows_publisher: Option<String>,
    /// macOS: `.app` bundle name to search for. Looked up under
    /// /Applications and ~/Applications, then via `mdfind kMDItemKind == Application`.
    /// Either "Editor CN" or "Editor CN.app" works — we normalize.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos_app_name: Option<String>,
    /// Linux: names to match against `Name=` in `.desktop` files
    /// under /usr/share/applications, ~/.local/share/applications,
    /// and /var/lib/flatpak/exports/share/applications.
    #[serde(default)]
    pub linux_desktop_names: Vec<String>,
}

// ─── models.json data structure (model field read/write mapping) ───
// Also accepts the legacy file name `config.json` — tool_manager's loader
// transparently falls back when models.json is absent.

/// Read mapping: ModelInfo field → config file path(s) (priority order)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReadMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<Vec<String>>,
}

/// models.json — model field read/write mapping (legacy: config.json).
///
/// Vibe-Coding users only need to author the `write` block (5 lines for the
/// canonical OpenAI / Anthropic shape). The loader fills in the rest:
///   - `config_file`: falls back to paths.json's `configFile` if empty
///   - `read`:        auto-derived by inverting `write` if absent
///   - `format`:      defaults to "json"
///   - `custom`:      defaults to false
///   - `docs`:        defaults to ""
///
/// Full-schema files written before this loosening keep working unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigMapping {
    #[serde(default)]
    pub docs: String,
    #[serde(default)]
    pub config_file: String,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default)]
    pub custom: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<ConfigReadMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<HashMap<String, String>>,
}

fn default_format() -> String {
    "json".to_string()
}

/// Loaded tool definition (paths.json + models.json combined; config.json accepted as legacy)
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub id: String,
    pub paths_config: PathsConfig,
    pub config_mapping: ConfigMapping,
    pub tool_dir: String,
}

// ─── DetectedTool (sent to frontend) ───

/// Tool category
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCategory {
    #[serde(rename = "CLI Code", alias = "CLI")]
    CLI,
    #[serde(rename = "Agents", alias = "CLI Agent", alias = "AgentOS")]
    Agents,
    IDE,
    AutoTrading,
    Game,
    Desktop,
    #[serde(rename = "Cloud Agent")]
    CloudAgent,
    Utility,
    Science,
    Creative,
    Custom,
}

/// Detected tool with runtime info (sent to frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub category: ToolCategory,
    pub official: bool,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skills_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_skills_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_protocol: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub names: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_model_config: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_cwd_prompt: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_uri: Option<String>,
}
