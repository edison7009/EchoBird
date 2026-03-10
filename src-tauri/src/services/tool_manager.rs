// Tool manager �?loads tool definitions, detects installed tools, manages configs
// Mirrors tools/loader.ts architecture:
// Each tool = directory with paths.json (detection) + config.json (config mapping)

use std::fs;
use std::path::{Path, PathBuf};

use crate::models::tool::{
    ConfigMapping, DetectedTool, PathsConfig, ToolCategory, ToolDefinition,
};
use crate::utils::platform;

// ─── Path expansion (mirrors tools/utils.ts expandPath) ───

/// Expand ~ and %ENV_VAR% in path strings
pub fn expand_path(p: &str) -> PathBuf {
    let mut result = p.to_string();

    // Expand ~ to home directory
    if result.starts_with("~/") || result.starts_with("~\\") {
        if let Some(home) = dirs::home_dir() {
            result = format!("{}{}", home.display(), &result[1..]);
        }
    }

    // Expand %ENV_VAR% (Windows style)
    while let Some(start) = result.find('%') {
        if let Some(end) = result[start + 1..].find('%') {
            let var_name = &result[start + 1..start + 1 + end];
            let replacement = std::env::var(var_name).unwrap_or_default();
            result = format!("{}{}{}", &result[..start], replacement, &result[start + 2 + end..]);
        } else {
            break;
        }
    }

    PathBuf::from(result)
}

// ─── Tauri resource_dir (set at startup via init_resource_dir) ───

use std::sync::Mutex as ResDirMutex;
static RESOURCE_DIR: ResDirMutex<Option<PathBuf>> = ResDirMutex::new(None);

/// Called once at app startup to store Tauri's resource_dir for correct platform paths.
/// resource_dir() returns the right location on every OS:
///   macOS  → <app>.app/Contents/Resources
///   Windows → install dir
///   Linux   → /usr/lib/com.echobird.ai  (deb)  or  $APPDIR/usr/lib/com.echobird.ai  (AppImage)
pub fn init_resource_dir(path: PathBuf) {
    if let Ok(mut guard) = RESOURCE_DIR.lock() {
        log::info!("[ToolManager] resource_dir = {:?}", path);
        *guard = Some(path);
    }
}

// ─── Tool directory resolution ───

/// Find the tools directory (tools/)
/// In dev: relative to project root
/// In production: bundled with the app binary
fn find_tools_dir() -> Option<PathBuf> {
    // 0. Tauri-native resource_dir (most reliable — set at startup via init_resource_dir)
    if let Ok(guard) = RESOURCE_DIR.lock() {
        if let Some(ref res_dir) = *guard {
            let tools_dir = res_dir.join("tools");
            if tools_dir.exists() {
                return Some(tools_dir);
            }
        }
    }

    // 1. Try relative to current exe (production fallback)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let tools_dir = exe_dir.join("tools");
            if tools_dir.exists() {
                return Some(tools_dir);
            }
            let up_tools = exe_dir.join("_up_").join("tools");
            if up_tools.exists() {
                return Some(up_tools);
            }
            if let Some(parent) = exe_dir.parent() {
                let tools_dir = parent.join("tools");
                if tools_dir.exists() {
                    return Some(tools_dir);
                }
            }
        }
    }

    // 2. Linux: Tauri bundles resources under /usr/lib/{identifier}/
    //    - deb/rpm: /usr/lib/com.echobird.ai/tools/
    //    - AppImage: $APPDIR/usr/lib/com.echobird.ai/tools/
    #[cfg(target_os = "linux")]
    {
        // AppImage mounts at $APPDIR
        if let Ok(appdir) = std::env::var("APPDIR") {
            let tools_dir = PathBuf::from(&appdir).join("usr/lib/com.echobird.ai/tools");
            if tools_dir.exists() {
                return Some(tools_dir);
            }
        }
        // deb/rpm install path
        for prefix in &["/usr/lib/com.echobird.ai", "/usr/lib/echobird"] {
            let tools_dir = PathBuf::from(prefix).join("tools");
            if tools_dir.exists() {
                return Some(tools_dir);
            }
        }
    }

    // 3. Try relative to CARGO_MANIFEST_DIR (dev mode)
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let project_root = PathBuf::from(manifest_dir).parent().map(|p| p.to_path_buf());
        if let Some(root) = project_root {
            let tools_dir = root.join("tools");
            if tools_dir.exists() {
                return Some(tools_dir);
            }
        }
    }

    // 4. Try cwd-based lookup (fallback)
    let cwd_tools = PathBuf::from("tools");
    if cwd_tools.exists() {
        return Some(cwd_tools);
    }

    log::warn!("[ToolManager] Cannot find tools directory");
    None
}

/// Public wrapper to get the tools directory path
pub fn get_tools_dir() -> PathBuf {
    find_tools_dir().unwrap_or_else(|| PathBuf::from("tools"))
}

// ─── Load tool definitions from directory ───

/// Load all tool definitions by scanning tools/*/paths.json
fn load_tool_definitions() -> Vec<ToolDefinition> {
    let tools_dir = match find_tools_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };

    log::info!("[ToolManager] Scanning tools directory: {:?}", tools_dir);
    let mut definitions = Vec::new();

    let entries = match fs::read_dir(&tools_dir) {
        Ok(e) => e,
        Err(e) => {
            log::error!("[ToolManager] Failed to read tools directory: {}", e);
            return Vec::new();
        }
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let tool_id = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let paths_file = path.join("paths.json");
        let config_file = path.join("config.json");

        if !paths_file.exists() || !config_file.exists() {
            continue;
        }

        // Parse paths.json
        let paths_config: PathsConfig = match fs::read_to_string(&paths_file)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
        {
            Some(pc) => pc,
            None => {
                log::warn!("[ToolManager] Failed to parse {}/paths.json", tool_id);
                continue;
            }
        };

        // Parse config.json
        let config_mapping: ConfigMapping = match fs::read_to_string(&config_file)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
        {
            Some(cm) => cm,
            None => {
                log::warn!("[ToolManager] Failed to parse {}/config.json", tool_id);
                continue;
            }
        };

        log::info!(
            "[ToolManager] Loaded tool: {} ({}) | start_command={:?}, command={}",
            tool_id,
            paths_config.name,
            paths_config.start_command,
            paths_config.command
        );

        definitions.push(ToolDefinition {
            id: tool_id,
            paths_config,
            config_mapping,
            tool_dir: path.to_string_lossy().to_string(),
        });
    }

    log::info!(
        "[ToolManager] Loaded {} tool definitions",
        definitions.len()
    );
    definitions
}

// ─── Tool detection (mirrors loader.ts detect()) ───

/// Get platform-specific paths from PlatformPaths
fn get_platform_paths(paths: &crate::models::tool::PlatformPaths) -> Vec<String> {
    #[cfg(target_os = "windows")]
    { paths.win32.clone().unwrap_or_default() }
    #[cfg(target_os = "macos")]
    { paths.darwin.clone().unwrap_or_default() }
    #[cfg(target_os = "linux")]
    { paths.linux.clone().unwrap_or_default() }
}

/// Detect if a tool is installed, returns executable path
async fn detect_tool(pc: &PathsConfig) -> Option<String> {
    // 0. Built-in tools (always installed)
    if pc.always_installed {
        return Some("built-in".to_string());
    }

    // 1. Check custom env var
    if let Some(ref env_var) = pc.env_var {
        if let Ok(custom_path) = std::env::var(env_var) {
            let expanded = expand_path(&custom_path);
            if expanded.exists() {
                return Some(expanded.to_string_lossy().to_string());
            }
        }
    }

    // 2. Check PATH for command
    if !pc.command.is_empty() {
        let found_in_path = platform::command_exists(&pc.command).await;
        if found_in_path {
            if pc.require_config_file {
                if config_file_exists(pc) {
                    let path = platform::get_command_path(&pc.command).await;
                    return path.or(Some(pc.command.clone()));
                }
                log::info!(
                    "[{}] Command found in PATH but config file missing",
                    pc.name
                );
            } else {
                let path = platform::get_command_path(&pc.command).await;
                return path.or(Some(pc.command.clone()));
            }
        }
    }

    // 3. Check platform-specific paths
    let platform_paths = get_platform_paths(&pc.paths);
    for p in &platform_paths {
        let expanded = expand_path(p);
        if expanded.exists() {
            if pc.require_config_file {
                if config_file_exists(pc) {
                    return Some(expanded.to_string_lossy().to_string());
                }
            } else {
                return Some(expanded.to_string_lossy().to_string());
            }
        }
    }

    // 4. Check VS Code extension paths (glob matching)
    if let Some(ref ext_paths) = pc.extension_paths {
        let ext_platform = get_platform_paths(ext_paths);
        for pattern in &ext_platform {
            let expanded = expand_path(pattern);
            let base_dir = expanded.parent();
            let glob_part = expanded.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if let Some(base) = base_dir {
                if base.exists() {
                    if let Ok(entries) = fs::read_dir(base) {
                        let prefix = glob_part.replace('*', "");
                        for entry in entries.filter_map(|e| e.ok()) {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.starts_with(&prefix) {
                                let full_path = base.join(&name);
                                log::info!("[{}] Extension found: {:?}", pc.name, full_path);
                                return Some(full_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // 5. Detect by config directory existence (GUI desktop apps)
    if pc.detect_by_config_dir && !pc.config_dir.is_empty() {
        let config_dir = expand_path(&pc.config_dir);
        if config_dir.exists() {
            log::info!(
                "[{}] Config directory found: {:?}, treated as installed",
                pc.name,
                config_dir
            );
            return Some(config_dir.to_string_lossy().to_string());
        }
    }

    None
}

/// Check if the tool's config file exists
fn config_file_exists(pc: &PathsConfig) -> bool {
    if !pc.config_file.is_empty() {
        let main_config = expand_path(&pc.config_file);
        if main_config.exists() {
            return true;
        }
    }
    if let Some(ref alt) = pc.config_file_alt {
        let alt_config = expand_path(alt);
        if alt_config.exists() {
            return true;
        }
    }
    false
}

/// Get the skills path for a tool
async fn find_skills_path(pc: &PathsConfig) -> Option<String> {
    let sp = pc.skills_path.as_ref()?;

    // 1. Environment variable
    if let Some(ref env_var) = sp.env_var {
        if let Ok(path) = std::env::var(env_var) {
            return Some(path);
        }
    }

    // 2. Platform-specific paths
    let platform_paths = {
        #[cfg(target_os = "windows")]
        { sp.win32.clone().unwrap_or_default() }
        #[cfg(target_os = "macos")]
        { sp.darwin.clone().unwrap_or_default() }
        #[cfg(target_os = "linux")]
        { sp.linux.clone().unwrap_or_default() }
    };
    for p in &platform_paths {
        let expanded = expand_path(p);
        if expanded.exists() {
            return Some(expanded.to_string_lossy().to_string());
        }
    }

    // 3. npm global module
    if let Some(ref npm_module) = sp.npm_module {
        #[cfg(windows)]
        let npm_output = {
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            tokio::process::Command::new("npm")
                .args(["root", "-g"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .await
        };
        #[cfg(not(windows))]
        let npm_output = tokio::process::Command::new("npm")
            .args(["root", "-g"])
            .output()
            .await;
        if let Ok(output) = npm_output {
            let global_root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !global_root.is_empty() {
                let module_path: PathBuf = npm_module
                    .split('/')
                    .fold(PathBuf::from(&global_root), |acc, part| acc.join(part));
                if module_path.exists() {
                    return Some(module_path.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Count installed skills in a directory
fn count_skills(skills_path: &str) -> u32 {
    let path = Path::new(skills_path);
    if !path.exists() {
        return 0;
    }
    fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                        && !e.file_name().to_string_lossy().starts_with('.')
                })
                .count() as u32
        })
        .unwrap_or(0)
}

/// Read the current active model from a tool's config
async fn read_active_model(def: &ToolDefinition) -> Option<String> {
    let cm = &def.config_mapping;

    // Only handle JSON format for generic read
    if cm.format != "json" || cm.custom {
        return None;
    }

    let read_map = cm.read.as_ref()?;
    let model_paths = read_map.model.as_ref()?;

    let config_path = expand_path(&cm.config_file);
    let content = fs::read_to_string(&config_path).ok()?;
    let config: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Try each path in priority order
    for json_path in model_paths {
        if let Some(val) = get_nested_value(&config, json_path) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Parse category string to ToolCategory enum
fn parse_category(s: &str) -> ToolCategory {
    match s {
        "AgentOS" => ToolCategory::AgentOS,
        "IDE" => ToolCategory::IDE,
        "CLI" => ToolCategory::CLI,
        "AutoTrading" => ToolCategory::AutoTrading,
        "Game" => ToolCategory::Game,
        "Utility" => ToolCategory::Utility,
        _ => ToolCategory::Custom,
    }
}

// ─── JSON nested value helpers (mirrors tools/utils.ts) ───

/// Get a nested value from a JSON object by dot-separated path (e.g. "env.OPENAI_API_KEY")
pub fn get_nested_value(obj: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = obj;
    for part in &parts {
        current = current.get(*part)?;
    }
    Some(current.clone())
}

/// Set a nested value in a JSON object by dot-separated path
pub fn set_nested_value(obj: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = obj;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current[*part] = value;
            return;
        }
        if !current.get(*part).map(|v| v.is_object()).unwrap_or(false) {
            current[*part] = serde_json::json!({});
        }
        current = current.get_mut(*part).unwrap();
    }
}

/// Delete a nested value from a JSON object by dot-separated path
pub fn delete_nested_value(obj: &mut serde_json::Value, path: &str) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }
    let mut current = obj;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(map) = current.as_object_mut() {
                map.remove(*part);
            }
            return;
        }
        match current.get_mut(*part) {
            Some(next) if next.is_object() => current = next,
            _ => return,
        }
    }
}

// ─── Global tool definitions cache ───

use std::sync::Mutex;

static TOOL_DEFINITIONS: Mutex<Option<Vec<ToolDefinition>>> = Mutex::new(None);

/// Get or load tool definitions (cached)
fn get_definitions() -> Vec<ToolDefinition> {
    let mut cache = TOOL_DEFINITIONS.lock().unwrap();
    if cache.is_none() {
        *cache = Some(load_tool_definitions());
    }
    cache.as_ref().unwrap().clone()
}

/// Get the config mapping for a specific tool
pub fn get_tool_config_mapping(tool_id: &str) -> Option<(ToolDefinition, PathBuf)> {
    let defs = get_definitions();
    defs.into_iter().find(|d| d.id == tool_id).map(|def| {
        let config_path = expand_path(&def.config_mapping.config_file);
        (def, config_path)
    })
}

/// Get the CLI command for a tool (from paths.json "command" field)
pub fn get_tool_command(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    defs.iter()
        .find(|d| d.id == tool_id)
        .and_then(|def| {
            let cmd = &def.paths_config.command;
            if cmd.is_empty() { None } else { Some(cmd.clone()) }
        })
}

/// Get the explicit start command for launching a tool (from paths.json "startCommand" field).
/// Only returns a value if startCommand is explicitly defined �?does NOT fall back to "command"
/// (which is used for detection only). Matches old Electron getStartCommand() behavior.
pub fn get_tool_start_command(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    defs.iter()
        .find(|d| d.id == tool_id)
        .and_then(|def| {
            def.paths_config.start_command.as_ref()
                .filter(|sc| !sc.is_empty())
                .cloned()
        })
}

/// Get the executable path for a GUI tool (checks platform-specific paths from paths.json)
pub fn get_tool_exe_path(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    let def = defs.iter().find(|d| d.id == tool_id)?;
    let platform_paths = get_platform_paths(&def.paths_config.paths);
    for p in &platform_paths {
        let expanded = expand_path(p);
        if expanded.exists() {
            return Some(expanded.to_string_lossy().to_string());
        }
    }
    None
}

/// Check if a tool is a VS Code extension (detected via extensionPaths)
pub fn is_vscode_extension(tool_id: &str) -> bool {
    let defs = get_definitions();
    defs.iter()
        .find(|d| d.id == tool_id)
        .map(|def| def.paths_config.extension_paths.is_some())
        .unwrap_or(false)
}

// ─── Main entry point ───

/// Scan all installed tools — main entry point
pub async fn scan_tools() -> Vec<DetectedTool> {
    // Clear cache to pick up newly added tool directories
    {
        let mut cache = TOOL_DEFINITIONS.lock().unwrap();
        *cache = None;
    }
    let definitions = get_definitions();
    let mut results: Vec<DetectedTool> = Vec::new();

    for def in &definitions {
        let pc = &def.paths_config;
        let installed_path = detect_tool(pc).await;
        let installed = installed_path.is_some();

        let mut skills_path_str = None;
        let mut skills_count = 0;
        let mut version = pc.version.clone();

        if installed {
            // Find skills path (from user directory via skillsPath config)
            if let Some(sp) = find_skills_path(pc).await {
                skills_count = count_skills(&sp);
                skills_path_str = Some(sp);
            }
            // Fallback: if no user skills path found, use defaultSkillsPath (relative to tool dir)
            if skills_path_str.is_none() {
                if let Some(ref rel_path) = pc.default_skills_path {
                    let default_path = PathBuf::from(&def.tool_dir).join(rel_path);
                    if default_path.exists() {
                        let p = default_path.to_string_lossy().to_string();
                        skills_count = count_skills(&p);
                        skills_path_str = Some(p);
                    }
                }
            }

            // Get version from command
            if version.is_none() && !pc.command.is_empty() {
                version = platform::get_version(&pc.command).await;
            }
        }

        // Read active model
        let active_model = if installed {
            read_active_model(def).await
        } else {
            None
        };

        // Config file path �?show the expected path even if file doesn't exist yet
        let config_path = if installed && !def.config_mapping.config_file.is_empty() {
            let cp = expand_path(&def.config_mapping.config_file);
            Some(cp.to_string_lossy().to_string())
        } else {
            None
        };

        // For built-in tools: resolve launchFile to actual path under tools directory
        let detected_path = if pc.always_installed {
            if let Some(ref launch) = pc.launch_file {
                let tools_dir = find_tools_dir().unwrap_or_default();
                let launch_path = tools_dir.join(&def.id).join(launch);
                if launch_path.exists() {
                    Some(launch_path.to_string_lossy().to_string())
                } else {
                    installed_path  // fallback to "built-in"
                }
            } else {
                installed_path
            }
        } else {
            installed_path
        };

        results.push(DetectedTool {
            id: def.id.clone(),
            name: pc.name.clone(),
            category: parse_category(&pc.category),
            official: true,
            installed,
            detected_path,
            config_path,
            skills_path: skills_path_str,
            version,
            installed_skills_count: Some(skills_count),
            active_model,
            website: pc.website.clone().or(Some(pc.docs.clone())),
            api_protocol: if pc.api_protocol.is_empty() {
                None
            } else {
                Some(pc.api_protocol.clone())
            },
            launch_file: pc.launch_file.clone(),
            names: pc.names.clone(),
            start_command: pc.start_command.clone(),
        });
    }

    log::info!("[ToolManager] Scan complete: {} tools found", results.len());
    results
}
