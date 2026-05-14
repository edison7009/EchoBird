// Tool manager �?loads tool definitions, detects installed tools, manages configs
// Mirrors tools/loader.ts architecture:
// Each tool = directory with paths.json (detection) + config.json (config mapping)

use std::fs;
use std::path::{Path, PathBuf};

use crate::models::tool::{
    ConfigMapping, DetectedTool, InstallHints, PathsConfig, ToolCategory, ToolDefinition,
};
use crate::utils::platform;

// ─── Path expansion (mirrors tools/utils.ts expandPath) ───

/// Strip Windows UNC prefix `\\?\` that Rust's canonicalize / Tauri resource_dir adds.
/// Without this, paths displayed in the UI look like `\\?\C:\Users\...`.
#[allow(unused)]
fn strip_unc(s: String) -> String {
    #[cfg(target_os = "windows")]
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        return stripped.to_string();
    }
    s
}

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
            result = format!(
                "{}{}{}",
                &result[..start],
                replacement,
                &result[start + 2 + end..]
            );
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
pub fn find_tools_dir() -> Option<PathBuf> {
    // DEV-MODE PRIORITY: if the exe path contains /target/debug/ or
    // /target/release/, we're running from a Cargo build dir (not an
    // installed binary). Tauri only mirrors ../tools/ into _up_/tools/
    // at the START of `tauri dev`, so subdirs added during a dev session
    // won't appear in the mirror until the user restarts. Prefer the
    // SOURCE tree's tools/ directly — it's always the freshest copy.
    if let Ok(exe) = std::env::current_exe() {
        let exe_str = exe.to_string_lossy().replace('\\', "/");
        let is_cargo_target =
            exe_str.contains("/target/debug/") || exe_str.contains("/target/release/");
        if is_cargo_target {
            // exe at <repo>/src-tauri/target/{debug,release}/echobird.exe
            // Walk up 4 parents: target/debug → target → src-tauri → <repo>
            if let Some(repo_root) = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                let src_tools = repo_root.join("tools");
                if src_tools.is_dir() {
                    log::info!(
                        "[ToolManager] dev mode — using source tree tools dir: {:?}",
                        src_tools
                    );
                    return Some(src_tools);
                }
            }
        }
    }

    // 0. Tauri-native resource_dir (most reliable — set at startup via init_resource_dir)
    //
    // Tauri v2 bundles resources relative to src-tauri/ using "_up_" for "../" paths:
    //   config: "../tools/"  →  resource_dir/_up_/tools/  (confirmed on Linux deb)
    //   config: "tools"      →  resource_dir/tools/
    //
    // Reality (confirmed via dpkg -L echobird on Ubuntu):
    //   binary: /usr/bin/echobird
    //   tools:  /usr/lib/Echobird/_up_/tools/
    //   resource_dir = /usr/lib/Echobird/
    if let Ok(guard) = RESOURCE_DIR.lock() {
        if let Some(ref res_dir) = *guard {
            // Case 1: standard subdirectory
            let tools_dir = res_dir.join("tools");
            if tools_dir.is_dir() {
                log::info!(
                    "[ToolManager] Found tools dir (subdirectory): {:?}",
                    tools_dir
                );
                return Some(tools_dir);
            }
            // Case 2: Tauri encodes "../tools/" as _up_/tools/ inside resource_dir
            let up_tools = res_dir.join("_up_").join("tools");
            if up_tools.is_dir() {
                log::info!("[ToolManager] Found tools dir (_up_/tools): {:?}", up_tools);
                return Some(up_tools);
            }
            // Case 3: tool contents placed directly in resource_dir (rare)
            if res_dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(res_dir) {
                    let has_tool = entries
                        .filter_map(|e| e.ok())
                        .any(|e| e.path().is_dir() && e.path().join("paths.json").exists());
                    if has_tool {
                        log::info!(
                            "[ToolManager] Found tools dir (resource_dir itself): {:?}",
                            res_dir
                        );
                        return Some(res_dir.clone());
                    }
                }
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
        let project_root = PathBuf::from(manifest_dir)
            .parent()
            .map(|p| p.to_path_buf());
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
        let paths_config: PathsConfig = match fs::read_to_string(&paths_file) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(pc) => pc,
                Err(e) => {
                    log::warn!(
                        "[ToolManager] Failed to parse {}/paths.json: {}",
                        tool_id,
                        e
                    );
                    continue;
                }
            },
            Err(e) => {
                log::warn!("[ToolManager] Failed to read {}/paths.json: {}", tool_id, e);
                continue;
            }
        };

        // Parse config.json
        let config_mapping: ConfigMapping = match fs::read_to_string(&config_file) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(cm) => cm,
                Err(e) => {
                    log::warn!(
                        "[ToolManager] Failed to parse {}/config.json: {}",
                        tool_id,
                        e
                    );
                    continue;
                }
            },
            Err(e) => {
                log::warn!(
                    "[ToolManager] Failed to read {}/config.json: {}",
                    tool_id,
                    e
                );
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
    {
        paths.win32.clone().unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        paths.darwin.clone().unwrap_or_default()
    }
    #[cfg(target_os = "linux")]
    {
        paths.linux.clone().unwrap_or_default()
    }
    #[cfg(target_os = "android")]
    {
        let _ = paths;
        Vec::new()
    }
}

// ─── Install-hints scan (per platform) ───
//
// Fallback when paths.json's hardcoded locations miss the install — user
// chose a non-default directory, OS uses a different package layout, etc.
// Authoritative source per platform:
//   Windows: HKLM/HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*
//   macOS:   /Applications, ~/Applications, then mdfind fallback
//   Linux:   /usr/share/applications/*.desktop, ~/.local/share/applications, flatpak exports

#[cfg(windows)]
fn scan_windows_registry(hints: &InstallHints) -> Option<String> {
    if hints.windows_display_names.is_empty() {
        return None;
    }
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    let names_lower: Vec<String> = hints
        .windows_display_names
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let publisher_filter = hints.windows_publisher.as_ref().map(|p| p.to_lowercase());

    // Standard Uninstall hives. WOW6432Node catches 32-bit installers on 64-bit Windows.
    let hives: &[(_, &str)] = &[
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ),
    ];

    for (hive, path) in hives {
        let key = RegKey::predef(*hive);
        let uninstall = match key.open_subkey_with_flags(path, KEY_READ) {
            Ok(k) => k,
            Err(_) => continue,
        };
        for subkey_name in uninstall.enum_keys().filter_map(|x| x.ok()) {
            let entry = match uninstall.open_subkey(&subkey_name) {
                Ok(e) => e,
                Err(_) => continue,
            };
            let display_name: String = entry.get_value("DisplayName").unwrap_or_default();
            if display_name.is_empty() {
                continue;
            }
            let dn_lower = display_name.to_lowercase();
            // EXACT case-insensitive match. We don't substring-match because
            // "Trae" would then match "Trae CN" and the wrong card would
            // claim a non-default-install. If the registry uses suffixes
            // ("Microsoft Visual Studio Code" vs "(User)"), list every
            // variant explicitly in windowsDisplayNames.
            if !names_lower.contains(&dn_lower) {
                continue;
            }
            if let Some(ref pub_filter) = publisher_filter {
                let pub_val: String = entry.get_value("Publisher").unwrap_or_default();
                if !pub_val.to_lowercase().contains(pub_filter) {
                    continue;
                }
            }
            // DisplayIcon usually points at the main exe directly. Strip ",N" icon-index
            // suffix if present (Windows convention for selecting an icon from a multi-icon exe).
            if let Ok(icon) = entry.get_value::<String, _>("DisplayIcon") {
                let icon_path = icon.split(',').next().unwrap_or(&icon).trim();
                let unquoted = icon_path.trim_matches('"');
                if !unquoted.is_empty() && Path::new(unquoted).exists() {
                    log::info!(
                        "[InstallHints] Registry hit (DisplayIcon): {} → {}",
                        display_name,
                        unquoted
                    );
                    return Some(unquoted.to_string());
                }
            }
            // Fallback: InstallLocation is a directory; we return it as-is.
            // detect_tool's caller knows how to handle both file and directory results.
            if let Ok(install_loc) = entry.get_value::<String, _>("InstallLocation") {
                let trimmed = install_loc.trim().trim_matches('"');
                if !trimmed.is_empty() && Path::new(trimmed).exists() {
                    log::info!(
                        "[InstallHints] Registry hit (InstallLocation): {} → {}",
                        display_name,
                        trimmed
                    );
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn scan_macos_applications(hints: &InstallHints) -> Option<String> {
    let app_name = hints.macos_app_name.as_ref()?;
    let normalized = if app_name.ends_with(".app") {
        app_name.clone()
    } else {
        format!("{}.app", app_name)
    };
    // Standard install roots first — fast filesystem stat.
    for root in ["/Applications", "/Applications/Utilities"] {
        let candidate = PathBuf::from(root).join(&normalized);
        if candidate.exists() {
            log::info!("[InstallHints] macOS hit: {}", candidate.display());
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    if let Some(home) = dirs::home_dir() {
        let candidate = home.join("Applications").join(&normalized);
        if candidate.exists() {
            log::info!("[InstallHints] macOS hit (user): {}", candidate.display());
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    // Fallback: mdfind covers non-/Applications installs (e.g. ~/Tools/Foo.app).
    if let Ok(out) = std::process::Command::new("mdfind")
        .args(["-name", &normalized])
        .output()
    {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let p = line.trim();
            if !p.is_empty() && p.ends_with(&normalized) && Path::new(p).exists() {
                log::info!("[InstallHints] mdfind hit: {}", p);
                return Some(p.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn scan_linux_desktop(hints: &InstallHints) -> Option<String> {
    if hints.linux_desktop_names.is_empty() {
        return None;
    }
    let names_lower: Vec<String> = hints
        .linux_desktop_names
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    let mut search_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join(".local/share/applications"));
    }

    for dir in &search_dirs {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            // Parse the [Desktop Entry] Name= and Exec= keys (first occurrence wins).
            let mut name = String::new();
            let mut exec = String::new();
            for line in content.lines() {
                if name.is_empty() {
                    if let Some(v) = line.strip_prefix("Name=") {
                        name = v.trim().to_string();
                    }
                }
                if exec.is_empty() {
                    if let Some(v) = line.strip_prefix("Exec=") {
                        exec = v.trim().to_string();
                    }
                }
                if !name.is_empty() && !exec.is_empty() {
                    break;
                }
            }
            let name_lower = name.to_lowercase();
            let filename_lower = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            // Match against either visible Name= or the .desktop filename stem.
            let hit = names_lower
                .iter()
                .any(|n| name_lower == *n || filename_lower == *n);
            if !hit {
                continue;
            }
            // Exec= often contains %U/%F field codes — keep only the command itself.
            let exec_clean = exec
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches('"');
            if !exec_clean.is_empty() {
                log::info!("[InstallHints] .desktop hit: {} → {}", name, exec_clean);
                return Some(exec_clean.to_string());
            }
        }
    }
    None
}

/// Cross-platform dispatcher. Returns Some(path) if the install hints
/// found the tool on disk; None otherwise (caller falls through to next
/// detection step).
fn scan_install_hints(pc: &PathsConfig) -> Option<String> {
    let hints = pc.install_hints.as_ref()?;
    #[cfg(windows)]
    {
        scan_windows_registry(hints)
    }
    #[cfg(target_os = "macos")]
    {
        scan_macos_applications(hints)
    }
    #[cfg(target_os = "linux")]
    {
        scan_linux_desktop(hints)
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = hints;
        None
    }
}

/// Detect if a tool is installed, returns executable path
async fn detect_tool(pc: &PathsConfig) -> Option<String> {
    // 0. Built-in tools (always installed)
    if pc.always_installed {
        return Some("built-in".to_string());
    }

    // 0.5. MSIX / Store apps (Windows): check user-data dir under %LOCALAPPDATA%\Packages\<PFN>.
    // PackageFamilyName is the AUMID prefix before '!'. Standard Windows install marker.
    #[cfg(windows)]
    if let Some(ref aumid) = pc.launch_uri {
        // Accept either raw AUMID or "shell:AppsFolder\<AUMID>"
        let aumid_clean = aumid
            .strip_prefix("shell:AppsFolder\\")
            .or_else(|| aumid.strip_prefix("shell:AppsFolder/"))
            .unwrap_or(aumid);
        if let Some(pfn) = aumid_clean.split('!').next() {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                let user_data = std::path::PathBuf::from(local).join("Packages").join(pfn);
                if user_data.exists() {
                    return Some(strip_unc(user_data.to_string_lossy().to_string()));
                }
            }
        }
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

    // 2.5. Check Python module (pip-installed tools like nanobot)
    if let Some(ref py_module) = pc.python_module {
        let found = platform::python_module_exists(py_module).await;
        if found {
            log::info!(
                "[{}] Python module '{}' detected (python -m {})",
                pc.name,
                py_module,
                py_module
            );
            return Some(format!("python -m {}", py_module));
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

    // 3.5. Install-hints fallback — catch installs at non-default paths.
    // Windows scans the registry Uninstall hive; macOS checks /Applications +
    // mdfind; Linux scans .desktop files. Only triggers when the hardcoded
    // paths above missed, so default installs don't pay the lookup cost.
    if let Some(hit) = scan_install_hints(pc) {
        if pc.require_config_file {
            if config_file_exists(pc) {
                return Some(hit);
            }
        } else {
            return Some(hit);
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

    // 5. Detect by config directory existence (GUI desktop apps without fixed install location)
    // Guard: if platform paths are defined but NONE exist (step 3 already checked),
    // the app was uninstalled — config dir alone is not a reliable signal.
    if pc.detect_by_config_dir && !pc.config_dir.is_empty() {
        let config_dir = expand_path(&pc.config_dir);
        if config_dir.exists() {
            let platform_paths = get_platform_paths(&pc.paths);
            if !platform_paths.is_empty() {
                // Platform paths are defined but none were found in step 3 → app uninstalled
                log::info!(
                    "[{}] Config directory exists but no executable found — app likely uninstalled, skipping",
                    pc.name
                );
            } else {
                // No platform paths defined — config dir is the only detection signal (e.g. purely config-based tools)
                log::info!(
                    "[{}] Config directory found: {:?}, treated as installed",
                    pc.name,
                    config_dir
                );
                return Some(config_dir.to_string_lossy().to_string());
            }
        } else if let Some(ref alt) = pc.config_dir_alt {
            // Try alternate config dir (e.g. Windows-specific %LOCALAPPDATA%\hermes vs Unix ~/.hermes)
            let alt_config_dir = expand_path(alt);
            if alt_config_dir.exists() {
                let platform_paths = get_platform_paths(&pc.paths);
                if platform_paths.is_empty() {
                    log::info!(
                        "[{}] Alternate config directory found: {:?}, treated as installed",
                        pc.name,
                        alt_config_dir
                    );
                    return Some(alt_config_dir.to_string_lossy().to_string());
                }
            }
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
        {
            sp.win32.clone().unwrap_or_default()
        }
        #[cfg(target_os = "macos")]
        {
            sp.darwin.clone().unwrap_or_default()
        }
        #[cfg(target_os = "linux")]
        {
            sp.linux.clone().unwrap_or_default()
        }
        #[cfg(target_os = "android")]
        {
            Vec::<String>::new()
        }
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

/// Read the current active model from a tool's config.
/// Delegates to tool_config_manager which has proper readers for every tool type
/// (generic JSON, echobird relay, YAML, TOML, custom formats, etc.)
async fn read_active_model(def: &ToolDefinition) -> Option<String> {
    use crate::services::tool_config_manager;

    let info = tool_config_manager::get_tool_model_info(&def.id).await?;

    // Prefer model ID; fall back to display name
    info.model.or(info.name)
}

/// Parse category string to ToolCategory enum
fn parse_category(s: &str) -> ToolCategory {
    match s {
        "Agents" | "CLI Agent" | "AgentOS" => ToolCategory::Agents,
        "IDE" => ToolCategory::IDE,
        "CLI Code" | "CLI" => ToolCategory::CLI,
        "AutoTrading" => ToolCategory::AutoTrading,
        "Game" => ToolCategory::Game,
        "Desktop" => ToolCategory::Desktop,
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
    defs.iter().find(|d| d.id == tool_id).and_then(|def| {
        let cmd = &def.paths_config.command;
        if cmd.is_empty() {
            None
        } else {
            Some(cmd.clone())
        }
    })
}

/// Get the explicit start command for launching a tool (from paths.json "startCommand" field).
/// Only returns a value if startCommand is explicitly defined �?does NOT fall back to "command"
/// (which is used for detection only). Matches old Electron getStartCommand() behavior.
pub fn get_tool_start_command(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    defs.iter().find(|d| d.id == tool_id).and_then(|def| {
        def.paths_config
            .start_command
            .as_ref()
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

/// Scan a single tool definition — runs detection, skills, version, and model reads.
/// Extracted so scan_tools() can run all tools concurrently via tokio::spawn.
async fn scan_single_tool(def: ToolDefinition) -> DetectedTool {
    let pc = &def.paths_config;
    let installed_path = detect_tool(pc).await;
    let installed = installed_path.is_some();

    let mut skills_path_str = None;
    let mut skills_count = 0u32;
    let mut version = pc.version.clone();

    if installed {
        if let Some(sp) = find_skills_path(pc).await {
            skills_count = count_skills(&sp);
            skills_path_str = Some(sp);
        }
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
        // Skip --version probe for desktop GUI apps: their binary doesn't
        // implement a CLI fast-path for --version, so invoking it just opens
        // the main window. noModelConfig is the right gate — it's already the
        // marker for "this is a launch-only desktop app, not a CLI we query".
        if version.is_none() && !pc.command.is_empty() && !pc.no_model_config {
            version = platform::get_version(&pc.command).await;
        }
    }

    let active_model = if installed {
        read_active_model(&def).await
    } else {
        None
    };

    let config_path = if installed && !def.config_mapping.config_file.is_empty() {
        let cp = expand_path(&def.config_mapping.config_file);
        Some(strip_unc(cp.to_string_lossy().to_string()))
    } else {
        None
    };

    let detected_path = if pc.always_installed {
        if let Some(ref launch) = pc.launch_file {
            let tools_dir = find_tools_dir().unwrap_or_default();
            let launch_path = tools_dir.join(&def.id).join(launch);
            if launch_path.exists() {
                Some(strip_unc(launch_path.to_string_lossy().to_string()))
            } else {
                installed_path
            }
        } else {
            installed_path
        }
    } else {
        installed_path
    };

    DetectedTool {
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
        command: if pc.command.is_empty() {
            None
        } else {
            Some(pc.command.clone())
        },
        no_model_config: pc.no_model_config,
        launch_uri: pc.launch_uri.clone(),
    }
}

/// Get the launch URI (e.g. "shell:AppsFolder\\<AUMID>") for an MSIX/Store app.
pub fn get_tool_launch_uri(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    defs.iter()
        .find(|d| d.id == tool_id)?
        .paths_config
        .launch_uri
        .clone()
}

/// Scan all installed tools — runs all detections in parallel for fast completion.
pub async fn scan_tools() -> Vec<DetectedTool> {
    // Clear cache to pick up newly added tool directories
    {
        let mut cache = TOOL_DEFINITIONS.lock().unwrap();
        *cache = None;
    }
    let definitions = get_definitions();

    // Spawn a task per tool so all detections run concurrently
    let mut handles = Vec::with_capacity(definitions.len());
    for def in definitions {
        handles.push(tokio::spawn(async move { scan_single_tool(def).await }));
    }

    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        match handle.await {
            Ok(tool) => results.push(tool),
            Err(e) => log::warn!("[ToolManager] Tool scan task panicked: {}", e),
        }
    }

    log::info!("[ToolManager] Scan complete: {} tools found", results.len());
    results
}
