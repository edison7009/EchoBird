// Tool manager �?loads tool definitions, detects installed tools, manages configs
// Mirrors tools/loader.ts architecture:
// Each tool = directory with paths.json (detection) + config.json (config mapping)

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub mod system_applications;

use crate::models::tool::{
    ConfigMapping, DetectedTool, InstallHints, PathsConfig, ToolCategory, ToolDefinition,
};
use crate::utils::platform;

// ─── Path expansion (mirrors tools/utils.ts expandPath) ───

/// Normalise a path string for UI display.
/// - Strip Windows UNC prefix `\\?\` that Rust's canonicalize / Tauri resource_dir adds
///   (without this, paths in the UI look like `\\?\C:\Users\...`).
/// - On Windows, replace any forward slashes with native backslashes so the UI shows
///   the same separator users see in Explorer / cmd / file dialogs. Tool JSON configs
///   use `/` for cross-platform portability and `expand_path` doesn't normalise, which
///   would otherwise leak mixed-separator paths like `C:\Users\eben/.codex/config.toml`
///   into the App Desktop cards.
///
/// On non-Windows the function is a no-op (forward slash is already native).
fn normalize_for_display(s: String) -> String {
    #[cfg(target_os = "windows")]
    let s = {
        let stripped = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
        stripped.replace('/', "\\")
    };
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

#[cfg(any(debug_assertions, test))]
fn source_tools_dir(manifest_dir: &Path) -> Option<PathBuf> {
    manifest_dir
        .parent()
        .map(|root| root.join("tools"))
        .filter(|tools| tools.is_dir())
}

/// Find source tools in debug builds, otherwise use bundled app resources.
pub fn find_tools_dir() -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if let Some(source_tools) = source_tools_dir(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        return Some(source_tools);
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
// Fill in the optional parts of a models.json that an author left out.
// Lets users ship a 5-line `write`-only file and still get the full mapping
// at runtime; full-schema files written before this loosening are no-ops
// here because the relevant fields are already populated.
fn backfill_config_mapping(
    cm: &mut crate::models::tool::ConfigMapping,
    paths_config: &crate::models::tool::PathsConfig,
) {
    use crate::models::tool::ConfigReadMapping;

    // config_file: paths.json already has this for every bundled tool;
    // models.json doesn't need to repeat it.
    if cm.config_file.is_empty() && !paths_config.config_file.is_empty() {
        cm.config_file = paths_config.config_file.clone();
    }

    // read: invert the write map. For each (config_path, model_field) write
    // entry, "model_field" tells us which ModelInfo field flows into that
    // config_path. Reading is just the reverse — ModelInfo.<field> can come
    // from any of the config_paths that wrote it. Multiple writes per field
    // (e.g. anthropic.baseUrl + openai.baseUrl both holding baseUrl) become
    // priority-ordered read paths.
    if cm.read.is_none() {
        if let Some(write_map) = &cm.write {
            let mut model: Vec<String> = Vec::new();
            let mut base_url: Vec<String> = Vec::new();
            let mut api_key: Vec<String> = Vec::new();
            // Sort write entries by config_path for stable derived order
            // across runs (HashMap iteration order is random).
            let mut entries: Vec<(&String, &String)> = write_map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            for (config_path, model_field) in entries {
                match model_field.as_str() {
                    "model" => model.push(config_path.clone()),
                    "baseUrl" => base_url.push(config_path.clone()),
                    "apiKey" => api_key.push(config_path.clone()),
                    _ => { /* ignore unknown ModelInfo fields */ }
                }
            }
            cm.read = Some(ConfigReadMapping {
                model: if model.is_empty() { None } else { Some(model) },
                base_url: if base_url.is_empty() {
                    None
                } else {
                    Some(base_url)
                },
                api_key: if api_key.is_empty() {
                    None
                } else {
                    Some(api_key)
                },
            });
        }
    }
}

// ─── User path overrides (~/.echobird/tool-paths.json) ───
//
// Lets users add install paths for tools whose bundled paths.json missed the
// install (non-default directory, portable layout, etc.) WITHOUT editing the
// bundled files — those are part of the app resources and get overwritten on
// every update. This file lives in the user data dir, so edits survive
// updates; deleting it restores pure defaults. Fully fault-tolerant: a missing
// file, invalid JSON, or an unexpected shape all degrade to "no overrides" and
// never break the scan.
//
// Shape: { "<toolId>": ["full/path/to/binary", ...], ... }
//   • A bare string is accepted as a one-element list.
//   • Keys starting with '_' are skipped, so a user may add their own "_note"
//     field without it being read as a tool id. (The seed itself writes no
//     "_"-prefixed keys — it's pure "<id>": [paths] data.)
fn load_user_path_overrides() -> HashMap<String, Vec<String>> {
    let Some(home) = dirs::home_dir() else {
        return HashMap::new();
    };
    let file = home.join(".echobird").join("tool-paths.json");

    let content = match fs::read_to_string(&file) {
        Ok(c) => c,
        Err(_) => return HashMap::new(), // no file → no overrides (the norm)
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log::warn!(
                "[ToolManager] ~/.echobird/tool-paths.json is not valid JSON — ignoring overrides: {}",
                e
            );
            return HashMap::new();
        }
    };
    let Some(obj) = parsed.as_object() else {
        return HashMap::new();
    };

    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for (tool_id, val) in obj {
        if tool_id.starts_with('_') {
            continue; // doc/template field, not a tool id
        }
        let mut paths: Vec<String> = Vec::new();
        match val {
            serde_json::Value::String(s) if !s.trim().is_empty() => {
                paths.push(s.clone());
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        if !s.trim().is_empty() {
                            paths.push(s.to_string());
                        }
                    }
                }
            }
            _ => {} // tolerate anything else by ignoring this key
        }
        if !paths.is_empty() {
            out.insert(tool_id.clone(), paths);
        }
    }

    if !out.is_empty() {
        log::info!(
            "[ToolManager] Applied user path overrides for {} tool(s)",
            out.len()
        );
    }
    out
}

/// Prepend the current-OS user override paths to a tool's candidate list, so
/// detection (and the launcher, which reads the same field) also try the
/// user's custom location. User paths go FIRST: someone who bothered to set
/// this almost certainly means that exact binary. Dedupes against existing
/// entries so re-runs don't grow the list.
fn apply_user_path_overrides(paths_config: &mut PathsConfig, extra: &[String]) {
    if extra.is_empty() {
        return;
    }

    #[cfg(target_os = "windows")]
    let slot = &mut paths_config.paths.win32;
    #[cfg(target_os = "macos")]
    let slot = &mut paths_config.paths.darwin;
    #[cfg(target_os = "linux")]
    let slot = &mut paths_config.paths.linux;
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let slot = &mut paths_config.paths.win32;

    let existing = slot.get_or_insert_with(Vec::new);
    let mut merged: Vec<String> = Vec::with_capacity(existing.len() + extra.len());
    for p in extra {
        // Older EchoBird versions seeded CLI paths for the DSH desktop card.
        // Keep custom desktop locations, but never launch an old npm shim.
        if paths_config.name == "DeepSeek Harness"
            && matches!(
                p.rsplit(['/', '\\']).next(),
                Some(name) if name.eq_ignore_ascii_case("dsh") || name.eq_ignore_ascii_case("dsh.cmd")
            )
        {
            continue;
        }
        if !merged.contains(p) {
            merged.push(p.clone());
        }
    }
    for p in existing.iter() {
        if !merged.contains(p) {
            merged.push(p.clone());
        }
    }
    *existing = merged;
}

fn load_tool_definitions() -> Vec<ToolDefinition> {
    load_tool_definitions_with_overrides(true)
}

/// Parse one catalog directory, shared by built-in and plugin applications.
fn load_tool_directory(
    tool_id: String,
    path: &Path,
    user_overrides: &HashMap<String, Vec<String>>,
) -> Option<ToolDefinition> {
    let paths_file = path.join("paths.json");
    // Prefer models.json (canonical name for the model-field mapping file).
    // Fall back to config.json so legacy bundled or user-installed tools
    // that haven't migrated still load.
    let models_file = path.join("models.json");
    let config_file = if models_file.exists() {
        models_file
    } else {
        path.join("config.json")
    };

    if !paths_file.exists() || !config_file.exists() {
        return None;
    }

    // Parse paths.json
    let mut paths_config: PathsConfig = match fs::read_to_string(&paths_file) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(pc) => pc,
            Err(e) => {
                log::warn!(
                    "[ToolManager] Failed to parse {}/paths.json: {}",
                    tool_id,
                    e
                );
                return None;
            }
        },
        Err(e) => {
            log::warn!("[ToolManager] Failed to read {}/paths.json: {}", tool_id, e);
            return None;
        }
    };

    // Merge any user-supplied custom paths for this tool (current OS) on
    // top of the bundled defaults, so an install at a non-default location
    // is still detected. See load_user_path_overrides.
    if let Some(extra) = user_overrides.get(&tool_id) {
        apply_user_path_overrides(&mut paths_config, extra);
    }

    // Parse models.json (or config.json fallback — `config_file` already
    // points at whichever one we found above).
    let mut config_mapping: ConfigMapping = match fs::read_to_string(&config_file) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(cm) => cm,
            Err(e) => {
                let fname = config_file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("models.json");
                log::warn!("[ToolManager] Failed to parse {}/{}: {}", tool_id, fname, e);
                return None;
            }
        },
        Err(e) => {
            let fname = config_file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("models.json");
            log::warn!("[ToolManager] Failed to read {}/{}: {}", tool_id, fname, e);
            return None;
        }
    };

    // Auto-fill the parts of models.json a Vibe-Coding user shouldn't have
    // to type. The loader lets them author just the `write` block (5
    // lines for the canonical OpenAI / Anthropic shape); we infer
    // everything else from paths.json + the write map itself.
    backfill_config_mapping(&mut config_mapping, &paths_config);

    log::info!(
        "[ToolManager] Loaded tool: {} ({}) | start_command={:?}, command={}",
        tool_id,
        paths_config.name,
        paths_config.start_command,
        paths_config.command
    );

    Some(ToolDefinition {
        id: tool_id,
        paths_config,
        config_mapping,
        tool_dir: path.to_string_lossy().to_string(),
    })
}

/// Core loader. `apply_overrides` gates the user `~/.echobird/tool-paths.json`
/// merge: scanning and launching want it ON (via the cached `get_definitions`),
/// while `default_override_seed` wants it OFF — the seed must reflect PURE
/// bundled defaults, or re-creating the file (e.g. after the user deleted it to
/// reset) would bake the user's own already-cached overrides back in as
/// "defaults".
fn load_tool_definitions_with_overrides(apply_overrides: bool) -> Vec<ToolDefinition> {
    let tools_dir = match find_tools_dir() {
        Some(d) => d,
        None => return Vec::new(),
    };

    log::info!("[ToolManager] Scanning tools directory: {:?}", tools_dir);
    let user_overrides = if apply_overrides {
        load_user_path_overrides()
    } else {
        HashMap::new()
    };
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
        let Some(tool_id) = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        definitions.extend(load_tool_directory(tool_id, &path, &user_overrides));
    }

    if apply_overrides {
        for (tool_id, path) in super::plugin_accounts::tool_directories() {
            if !definitions
                .iter()
                .any(|definition| definition.id == tool_id)
            {
                definitions.extend(load_tool_directory(tool_id, &path, &user_overrides));
            }
        }
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

/// True when a registry `DisplayName` (already lowercased) is a hit: an EXACT
/// match against any `names_lower`, OR a PREFIX match against any
/// `prefixes_lower` (equal, or starts with `<prefix> ` so a trailing version
/// like "workbuddy 4.24.2" still matches the prefix "workbuddy"). Pure string
/// logic so it stays unit-testable on every platform (the registry walk that
/// uses it is Windows-only).
#[allow(dead_code)]
fn registry_display_name_matches(
    dn_lower: &str,
    names_lower: &[String],
    prefixes_lower: &[String],
) -> bool {
    if names_lower.iter().any(|n| n == dn_lower) {
        return true;
    }
    prefixes_lower.iter().any(|p| {
        if matches!(
            p.as_str(),
            "qoder" | "qoder cn" | "codearts" | "qwenwork" | "千问办公"
        ) {
            return dn_lower == p
                || dn_lower
                    .strip_prefix(&format!("{p} "))
                    .is_some_and(|version| version.starts_with(|c: char| c.is_ascii_digit()));
        }
        if p == "workbuddy" && (dn_lower == "workbuddy ai" || dn_lower.starts_with("workbuddy ai "))
        {
            return false;
        }
        dn_lower == p || dn_lower.starts_with(&format!("{p} "))
    })
}

/// Returns true when `path` has a Windows executable extension (`.exe`).
/// Registry `DisplayIcon` values can point at standalone `.ico` files (or
/// `.dll` icon resources); passing a non-executable to Start-Process opens
/// it with the default image viewer instead of launching the app, so we
/// gate on this before trusting a registry-discovered path.
#[cfg(windows)]
fn is_windows_exe(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("exe"))
        .unwrap_or(false)
}

/// True when an exe file stem (lowercased, no extension) corresponds to the
/// tool's known display names / prefixes — the rule used to pick the real app
/// exe out of an install directory that also contains uninstallers. Mirrors
/// `registry_display_name_matches` but for file stems: exact name match, or a
/// prefix match with a word boundary (space/dash/dot/underscore after the
/// prefix) so "zcode" / "zcode-beta" match prefix "zcode" but "zcodeextra"
/// does not. Pure string logic — unit-testable on every platform.
//
// Only `#[cfg(windows)]` call sites (find_name_matching_exe / scan_windows_registry)
// reference this, so on non-Windows CI the compiler flags it dead — relax
// dead_code there so the cross-platform unit tests (which do call it) keep
// building everywhere while Windows stays strict. Mirrors the cfg-gate pattern
// on the Windows-only registry scan helpers.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn exe_stem_matches_hints(
    stem_lower: &str,
    names_lower: &[String],
    prefixes_lower: &[String],
) -> bool {
    if names_lower.iter().any(|n| n == stem_lower) {
        return true;
    }
    prefixes_lower.iter().any(|p| {
        stem_lower == *p
            || (stem_lower.starts_with(p)
                && stem_lower[p.len()..]
                    .chars()
                    .next()
                    .map(|c| !c.is_alphanumeric())
                    .unwrap_or(false))
    })
}

/// True for exe stems that are clearly uninstallers / helpers, not the app
/// itself. Used to reject e.g. "Uninstall ZCode.exe" when globbing an install
/// dir whose name also begins with the tool prefix.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_uninstaller_stem(stem_lower: &str) -> bool {
    stem_lower.starts_with("uninstall") || stem_lower.starts_with("unins")
}

/// Extract the executable path from a registry `UninstallString` value.
/// Handles quoted paths with trailing args (`"E:\ZCode\Uninstall ZCode.exe"
/// /currentuser`) and bare paths (`C:\App\uninst.exe`). Returns None for
/// non-exe uninstallers like `winget uninstall --product-code ...` (no .exe
/// token) so callers don't treat a winget CLI string as an install dir.
/// Pure string parsing — no filesystem access.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn parse_uninstall_exe_path(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let token = if let Some(rest) = s.strip_prefix('"') {
        rest.split('"').next().unwrap_or("")
    } else {
        s.split_whitespace().next().unwrap_or("")
    };
    let token = token.trim();
    if token.is_empty() {
        return None;
    }
    let is_exe = Path::new(token)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if is_exe {
        Some(token.to_string())
    } else {
        None
    }
}

/// Glob an install directory for an exe whose name matches the tool's hints,
/// returning its full path. Used as a last-resort fallback when a registry
/// Uninstall entry exposes the install dir (via DisplayIcon's parent or
/// UninstallString) but neither DisplayIcon nor InstallLocation points at the
/// real app exe directly — e.g. electron-builder NSIS installs at a custom
/// path: DisplayIcon is a standalone `.ico`, InstallLocation is empty, and the
/// app exe sits next to `Uninstall <Name>.exe`. Uninstallers are excluded by
/// `is_uninstaller_stem` so we never return the uninstaller itself.
#[cfg(windows)]
fn find_name_matching_exe(
    dir: &Path,
    names_lower: &[String],
    prefixes_lower: &[String],
) -> Option<String> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !is_windows_exe(name) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if is_uninstaller_stem(&stem) {
            continue;
        }
        if exe_stem_matches_hints(&stem, names_lower, prefixes_lower) {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

#[cfg(windows)]
fn scan_windows_registry(hints: &InstallHints) -> Option<String> {
    if hints.windows_display_names.is_empty() && hints.windows_display_name_prefixes.is_empty() {
        return None;
    }

    let names_lower: Vec<String> = hints
        .windows_display_names
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let prefixes_lower: Vec<String> = hints
        .windows_display_name_prefixes
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let publisher_filter = hints.windows_publisher.as_ref().map(|p| p.to_lowercase());
    // The executable name can differ from the product name (Dola ships Cici.exe);
    // the caller then resolves this directory against the catalog's paths.
    let mut install_dir_hit = None;

    let entries = SCAN_DISCOVERY
        .try_with(|snapshot| snapshot.registry.clone())
        .unwrap_or_else(|_| Arc::new(read_registry_entries()));
    for entry in entries.iter() {
        let display_name: String = entry.get("DisplayName").cloned().unwrap_or_default();
        if display_name.is_empty() {
            continue;
        }
        let dn_lower = display_name.to_lowercase();
        // EXACT case-insensitive match by default (windowsDisplayNames) —
        // we don't blanket substring-match because a base product would
        // then match a regional edition and claim the wrong install.
        // Apps whose DisplayName embeds a version ("WorkBuddy 4.24.2") opt
        // into PREFIX matching via windowsDisplayNamePrefixes instead.
        if !registry_display_name_matches(&dn_lower, &names_lower, &prefixes_lower) {
            continue;
        }
        if let Some(ref pub_filter) = publisher_filter {
            let pub_val: String = entry.get("Publisher").cloned().unwrap_or_default();
            if !pub_val.to_lowercase().contains(pub_filter) {
                continue;
            }
        }
        // Collect install-dir candidates for the exe-glob fallback below.
        // Populated when DisplayIcon is a non-exe file (its parent dir is
        // still a strong install-dir signal) and/or UninstallString exposes
        // the install dir — covers installers (electron-builder NSIS at a
        // custom path) that leave InstallLocation empty.
        let mut install_dir_candidates: Vec<PathBuf> = Vec::new();

        // DisplayIcon usually points at the main exe directly. Strip ",N" icon-index
        // suffix if present (Windows convention for selecting an icon from a multi-icon exe).
        if let Some(icon) = entry.get("DisplayIcon") {
            let icon_path = icon.split(',').next().unwrap_or(icon).trim();
            let unquoted = icon_path.trim_matches('"');
            if !unquoted.is_empty() && Path::new(unquoted).exists() {
                // Reject non-executable targets (standalone .ico files,
                // .dll icon resources). Start-Process on one would open
                // it with the default image viewer instead of launching
                // the app — remember its parent dir as an install-dir
                // candidate, then fall through.
                if is_windows_exe(unquoted) {
                    log::info!(
                        "[InstallHints] Registry hit (DisplayIcon): {} → {}",
                        display_name,
                        unquoted
                    );
                    return Some(unquoted.to_string());
                }
                if let Some(parent) = Path::new(unquoted).parent() {
                    install_dir_candidates.push(parent.to_path_buf());
                }
                log::info!(
                        "[InstallHints] DisplayIcon is not an exe, using its dir as install-dir candidate: {} → {}",
                        display_name,
                        unquoted
                    );
            }
        }
        // Fallback: InstallLocation is a directory; we return it as-is.
        // detect_tool's caller knows how to handle both file and directory results.
        if let Some(install_loc) = entry.get("InstallLocation") {
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
        // Fallback 2: installers that leave InstallLocation empty but record
        // an UninstallString like `"E:\ZCode\Uninstall ZCode.exe" /currentuser`.
        // The uninstaller sits next to the real app exe, so its parent dir
        // is the install dir — glob it for a name-matching exe. This is what
        // makes a custom-path ZCode install (E:\ZCode\ZCode.exe) detectable
        // when DisplayIcon is a .ico and InstallLocation is blank.
        if let Some(uninst) = entry.get("UninstallString") {
            if let Some(exe_path) = parse_uninstall_exe_path(uninst) {
                if let Some(parent) = Path::new(&exe_path).parent() {
                    if !install_dir_candidates.iter().any(|c| c == parent) {
                        install_dir_candidates.push(parent.to_path_buf());
                    }
                }
            }
        }
        for dir in &install_dir_candidates {
            if let Some(exe) = find_name_matching_exe(dir, &names_lower, &prefixes_lower) {
                log::info!(
                    "[InstallHints] Registry hit (exe glob in install dir): {} → {}",
                    display_name,
                    exe
                );
                return Some(exe);
            }
        }
        install_dir_hit = install_dir_hit.or_else(|| {
            install_dir_candidates
                .iter()
                .find(|dir| dir.is_dir())
                .map(|dir| dir.to_string_lossy().into_owned())
        });
    }
    install_dir_hit
}

/// Read a GUI installer's DisplayVersion using the same name/publisher
/// matching and scan-scoped registry snapshot as executable discovery.
#[cfg(windows)]
fn scan_windows_registry_version(hints: &InstallHints) -> Option<String> {
    if hints.windows_display_names.is_empty() && hints.windows_display_name_prefixes.is_empty() {
        return None;
    }

    let names_lower: Vec<String> = hints
        .windows_display_names
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let prefixes_lower: Vec<String> = hints
        .windows_display_name_prefixes
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let publisher_filter = hints.windows_publisher.as_ref().map(|p| p.to_lowercase());

    let entries = SCAN_DISCOVERY
        .try_with(|snapshot| snapshot.registry.clone())
        .unwrap_or_else(|_| Arc::new(read_registry_entries()));
    for entry in entries.iter() {
        let display_name: String = entry.get("DisplayName").cloned().unwrap_or_default();
        if display_name.is_empty() {
            continue;
        }
        let dn_lower = display_name.to_lowercase();
        if !registry_display_name_matches(&dn_lower, &names_lower, &prefixes_lower) {
            continue;
        }
        if let Some(ref pub_filter) = publisher_filter {
            let pub_val: String = entry.get("Publisher").cloned().unwrap_or_default();
            if !pub_val.to_lowercase().contains(pub_filter) {
                continue;
            }
        }
        let version: String = entry.get("DisplayVersion").cloned().unwrap_or_default();
        let version = version.trim().to_string();
        if !version.is_empty() {
            log::info!(
                "[InstallHints] Registry version hit: {} → {}",
                display_name,
                version
            );
            return Some(version);
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
    if let Ok(Some(applications)) =
        SCAN_DISCOVERY.try_with(|snapshot| snapshot.applications.clone())
    {
        return applications
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name == normalized.as_str())
                    && path.exists()
            })
            .map(|path| path.to_string_lossy().into_owned());
    }
    // Fallback: mdfind covers non-/Applications installs (e.g. ~/Tools/Foo.app).
    if let Ok(out) = crate::utils::process::command("mdfind")
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

#[cfg(any(target_os = "linux", test))]
fn desktop_exec_program(exec: &str) -> Option<String> {
    let mut result = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in exec.trim_start().chars() {
        if escaped {
            result.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if ch.is_whitespace() && !quoted {
            break;
        } else {
            result.push(ch);
        }
    }
    (!quoted && !escaped && !result.is_empty()).then_some(result)
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

    let entries = SCAN_DISCOVERY
        .try_with(|snapshot| snapshot.desktop.clone())
        .unwrap_or_else(|_| Arc::new(read_desktop_entries()));
    for (path, content) in entries.iter() {
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
        if let Some(program) = desktop_exec_program(&exec) {
            let executable = if Path::new(&program).is_absolute() {
                PathBuf::from(program)
            } else if let Ok(path) = which::which(&program) {
                path
            } else {
                continue;
            };
            if executable.is_file() {
                log::info!(
                    "[InstallHints] .desktop hit: {} → {}",
                    name,
                    executable.display()
                );
                return Some(executable.to_string_lossy().into_owned());
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

/// Whether a tool has a detector stronger than its config directory — i.e.
/// platform paths or install hints (registry / Applications / .desktop scan).
/// When this is true, a lingering config directory must NOT be read as
/// "installed": those detectors are authoritative and have already run in
/// `detect_tool`, so reaching the config-dir step means they all failed — the
/// app is uninstalled and the config is stale (most uninstallers leave
/// ~/.<tool> behind on purpose). Only tools with NO such detector may use the
/// config directory as proof of installation.
fn has_authoritative_detector(pc: &PathsConfig) -> bool {
    !get_platform_paths(&pc.paths).is_empty() || pc.install_hints.is_some()
}

// Candidate traversal is shared by detection and launch. Only vendor-specific
// validation belongs here: MiniMax's two compatible editions identify their
// native config format through app-update.yml.
fn valid_executable(pc: &PathsConfig, path: &Path) -> bool {
    path.is_file()
        && (pc.name != "MiniMax Desktop"
            || super::tool_config_manager::minimaxcode::desktop_region(path).is_some())
        && match pc.name.as_str() {
            "Qoder" => {
                super::qoder_native::desktop_edition(path)
                    == Some(super::qoder_accounts::Edition::Qoder)
            }
            "Qoder CN" => {
                super::qoder_native::desktop_edition(path)
                    == Some(super::qoder_accounts::Edition::QoderCn)
            }
            "QwenWork" | "千问办公" => packaged_desktop_metadata(path).is_some_and(|value| {
                value["name"]
                    == if pc.name == "QwenWork" {
                        "qwen-work"
                    } else {
                        "qwen-work-cn"
                    }
            }),
            "CodeArts" => codearts_desktop_identity(path),
            "DuMate" => {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("DuMate"))
                    && packaged_desktop_metadata(path)
                        .is_some_and(|value| value["name"] == "qianfan-desktop-app")
            }
            "Devin" => devin_desktop_identity(path),
            "Devin CLI" => !devin_desktop_identity(path),
            "Cursor CLI" => fs::canonicalize(path).ok().is_some_and(|resolved| {
                resolved
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("cursor-agent"))
                    && !resolved.components().any(|part| {
                        part.as_os_str()
                            .to_string_lossy()
                            .eq_ignore_ascii_case(".grok")
                    })
            }),
            _ => true,
        }
}

fn codearts_desktop_identity(executable: &Path) -> bool {
    let Some(directory) = executable.parent() else {
        return false;
    };
    let resources = if directory.file_name().is_some_and(|name| name == "MacOS") {
        directory
            .parent()
            .map(|contents| contents.join("Resources"))
    } else {
        Some(directory.join("resources"))
    };
    let Some(resources) = resources else {
        return false;
    };
    fs::read(resources.join("product.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|product| product["nameShort"].as_str() == Some("CodeArts"))
}

fn environment_executable(pc: &PathsConfig) -> Option<String> {
    let key = pc.env_var.as_deref()?;
    if !matches!(key, "COPILOT_CLI_PATH" | "MCODE_PATH") {
        return None;
    }
    let path = expand_path(&std::env::var(key).ok()?);
    valid_executable(pc, &path).then(|| path.to_string_lossy().into_owned())
}

fn find_tool_executable(pc: &PathsConfig, include_hints: bool) -> Option<String> {
    if let Some(path) = environment_executable(pc) {
        return Some(path);
    }
    let paths = get_platform_paths(&pc.paths);
    let resolve = |value: &str| {
        let path = expand_path(value);
        if matches!(pc.name.as_str(), "QwenWork" | "千问办公") && path.is_dir() {
            if let Some(binary) = qwenwork_versioned_executable(pc, &path) {
                return Some(binary.to_string_lossy().into_owned());
            }
        }
        #[cfg(windows)]
        if pc.name == "Kilo Desktop" {
            let root = if path.is_dir() {
                Some(path.as_path())
            } else {
                path.parent()
            };
            if let Some(binary) = root.and_then(|root| kilo_versioned_executable(root, false)) {
                return Some(binary.to_string_lossy().into_owned());
            }
        }
        let candidate = if path.is_dir() {
            #[cfg(windows)]
            if matches!(pc.name.as_str(), "Kilo CLI" | "Kilo Desktop") {
                if let Some(binary) = kilo_versioned_executable(&path, pc.name == "Kilo CLI") {
                    return Some(binary.to_string_lossy().into_owned());
                }
            }
            PathBuf::from(resolve_install_directory(&path, &paths)?)
        } else {
            path
        };
        valid_executable(pc, &candidate).then(|| candidate.to_string_lossy().into_owned())
    };
    paths
        .iter()
        .find_map(|p| resolve(p))
        .or_else(|| {
            include_hints
                .then(|| system_applications::executable(pc))
                .flatten()
        })
        .or_else(|| {
            include_hints
                .then(|| scan_install_hints(pc))
                .flatten()
                .and_then(|hit| {
                    // An install root can hold a launcher above the declared runtime.
                    #[cfg(windows)]
                    if Path::new(&hit).is_file() {
                        if let Some(found) = Path::new(&hit)
                            .parent()
                            .and_then(Path::to_str)
                            .and_then(resolve)
                        {
                            return Some(found);
                        }
                    }
                    resolve(&hit)
                })
        })
        .or_else(|| {
            if !include_hints || !searches_path(&pc.name) {
                return None;
            }
            filenames_of(&paths).iter().find_map(|name| {
                let path = which::which(name).ok()?;
                #[cfg(windows)]
                if pc.name == "Kilo Desktop" {
                    if let Some(binary) = path
                        .parent()
                        .and_then(|root| kilo_versioned_executable(root, false))
                    {
                        return Some(binary.to_string_lossy().into_owned());
                    }
                }
                valid_executable(pc, &path).then(|| path.to_string_lossy().into_owned())
            })
        })
        .or_else(|| {
            if pc.name != "Devin CLI" {
                return None;
            }
            let defs = get_definitions();
            let desktop = defs.iter().find(|d| d.id == "devin")?;
            let executable = find_tool_executable(&desktop.paths_config, true)?;
            devin_bundled_cli(Path::new(&executable), std::env::consts::OS)
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().into_owned())
        })
}

/// PATH holds command-line tools. Hermes' and Claude's CLIs have the same leaf
/// name as their desktop apps, so a PATH hit would be the wrong program.
fn searches_path(tool_name: &str) -> bool {
    !matches!(tool_name, "Hermes Desktop" | "Claude Desktop")
}

fn devin_resources(executable: &Path) -> Option<PathBuf> {
    let dir = executable.parent()?;
    if dir.file_name().is_some_and(|s| s == "MacOS") {
        Some(dir.parent()?.join("Resources"))
    } else {
        Some(dir.join("resources"))
    }
}
fn devin_desktop_identity(executable: &Path) -> bool {
    devin_resources(executable)
        .and_then(|p| fs::read(p.join("app/product.json")).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .is_some_and(|p| p["nameShort"] == "Devin" && p["applicationName"] == "devin-desktop")
}
fn devin_bundled_cli(executable: &Path, platform: &str) -> Option<PathBuf> {
    let file = if platform == "windows" {
        "devin.exe"
    } else {
        "devin"
    };
    Some(
        devin_resources(executable)?
            .join("app/extensions/windsurf/devin/bin")
            .join(file),
    )
}

#[cfg(any(windows, test))]
fn kilo_versioned_executable(root: &Path, cli: bool) -> Option<PathBuf> {
    let suffixes: &[&str] = if cli {
        &[
            "resources/kilo/windows-x64-baseline/bin/kilo.exe",
            "resources/kilo/windows-arm64/bin/kilo.exe",
        ]
    } else {
        &["kilo-desktop.exe"]
    };
    let mut versions: Vec<(Vec<u32>, PathBuf)> = fs::read_dir(root)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name = name.to_str()?.strip_prefix("app-")?;
            let version = name
                .split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            entry.path().is_dir().then(|| (version, entry.path()))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    let mut directories: Vec<PathBuf> = versions.into_iter().map(|(_, path)| path).collect();
    if cli {
        directories.insert(0, root.to_path_buf());
    } else {
        // Squirrel's root executable forwards to a versioned native app.
        directories.push(root.to_path_buf());
    }
    directories
        .into_iter()
        .flat_map(|dir| suffixes.iter().map(move |suffix| dir.join(suffix)))
        .find(|path| path.is_file())
}

#[cfg(any(windows, test))]
fn kilo_native_binary(path: &Path) -> bool {
    use std::io::Read;
    let mut header = [0; 2];
    fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && header == *b"MZ"
}

#[cfg(any(windows, test))]
fn kilo_package_roots(prefix: &Path) -> Vec<PathBuf> {
    let mut roots = vec![prefix.join("node_modules")];
    if let Ok(versions) = fs::read_dir(prefix.join("global")) {
        roots.extend(
            versions
                .flatten()
                .filter(|entry| entry.path().is_dir())
                .map(|entry| entry.path().join("node_modules")),
        );
    }
    roots
}

#[cfg(any(windows, test))]
fn kilo_package_runtime(roots: &[PathBuf], arch: &str) -> Option<PathBuf> {
    let packages: &[&str] = if arch == "aarch64" {
        &["@kilocode/cli-windows-arm64/bin/kilo.exe"]
    } else {
        &[
            "@kilocode/cli-windows-x64-baseline/bin/kilo.exe",
            "@kilocode/cli-windows-x64/bin/kilo.exe",
        ]
    };
    roots.iter().find_map(|root| {
        let cli = root.join("@kilocode/cli");
        let cached = cli.join("bin/.kilo");
        if kilo_native_binary(&cached) {
            return Some(cached);
        }
        [root.clone(), cli.join("node_modules")]
            .into_iter()
            .find_map(|modules| {
                packages
                    .iter()
                    .map(|suffix| modules.join(suffix))
                    .find(|path| kilo_native_binary(path))
            })
    })
}

/// Kilo model arguments must reach a native executable without a batch decoder.
#[cfg(windows)]
pub(crate) fn get_kilo_runtime_path() -> Option<String> {
    if let Some(detected) = get_tool_exe_path("kilo") {
        let path = Path::new(&detected);
        if kilo_native_binary(path) {
            return Some(detected);
        }
        if let Some(parent) = path.parent() {
            if let Some(native) =
                kilo_package_runtime(&kilo_package_roots(parent), std::env::consts::ARCH)
            {
                return Some(native.to_string_lossy().into_owned());
            }
        }
    }
    if let Some(desktop) = get_tool_exe_path("kilodesktop") {
        if let Some(root) = Path::new(&desktop).parent() {
            if let Some(native) =
                kilo_versioned_executable(root, true).filter(|path| kilo_native_binary(path))
            {
                return Some(native.to_string_lossy().into_owned());
            }
        }
    }
    let mut prefixes = Vec::new();
    if let Some(roaming) = dirs::data_dir() {
        prefixes.push(roaming.join("npm"));
    }
    if let Some(local) = dirs::data_local_dir() {
        prefixes.push(
            std::env::var_os("PNPM_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| local.join("pnpm")),
        );
    }
    if let Some(home) = dirs::home_dir() {
        let bun = std::env::var_os("BUN_INSTALL")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".bun"));
        prefixes.push(bun.join("install/global"));
    }
    let roots = prefixes
        .iter()
        .flat_map(|prefix| kilo_package_roots(prefix))
        .collect::<Vec<_>>();
    kilo_package_runtime(&roots, std::env::consts::ARCH)
        .map(|path| path.to_string_lossy().into_owned())
}

#[test]
fn kilo_native_packages_resolve_npm_cache_and_pnpm_without_launching_batch_shims() {
    let root = std::env::temp_dir().join(format!("echobird-kilo-native-{}", uuid::Uuid::new_v4()));
    let prefix = root.join("npm");
    let modules = prefix.join("node_modules");
    let cached = modules.join("@kilocode/cli/bin/.kilo");
    let baseline = modules.join("@kilocode/cli-windows-x64-baseline/bin/kilo.exe");
    fs::create_dir_all(cached.parent().unwrap()).unwrap();
    fs::create_dir_all(baseline.parent().unwrap()).unwrap();
    fs::write(prefix.join("kilo.cmd"), b"batch fixture").unwrap();
    fs::write(&cached, b"script fixture").unwrap();
    fs::write(&baseline, b"MZnative fixture").unwrap();
    assert!(!kilo_native_binary(&prefix.join("kilo.cmd")));
    assert_eq!(
        kilo_package_runtime(&kilo_package_roots(&prefix), "x86_64"),
        Some(baseline)
    );
    fs::write(&cached, b"MZcache fixture").unwrap();
    assert_eq!(
        kilo_package_runtime(&kilo_package_roots(&prefix), "x86_64"),
        Some(cached)
    );
    let pnpm = root.join("pnpm");
    let arm = pnpm.join("global/5/node_modules/@kilocode/cli-windows-arm64/bin/kilo.exe");
    fs::create_dir_all(arm.parent().unwrap()).unwrap();
    fs::write(&arm, b"MZarm fixture").unwrap();
    assert_eq!(
        kilo_package_runtime(&kilo_package_roots(&pnpm), "aarch64"),
        Some(arm)
    );
    assert!(kilo_package_runtime(&kilo_package_roots(&pnpm), "x86_64").is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn kilo_squirrel_detection_selects_the_latest_installed_binary_without_update_launcher() {
    let root = std::env::temp_dir().join(format!("kilo-path-{}", uuid::Uuid::new_v4()));
    for version in ["0.1.9", "0.1.11"] {
        let dir = root.join(format!("app-{version}"));
        fs::create_dir_all(dir.join("resources/kilo/windows-x64-baseline/bin")).unwrap();
        fs::write(dir.join("kilo-desktop.exe"), b"fixture").unwrap();
        fs::write(
            dir.join("resources/kilo/windows-x64-baseline/bin/kilo.exe"),
            b"fixture",
        )
        .unwrap();
    }
    fs::write(root.join("Update.exe"), b"fixture").unwrap();
    fs::write(root.join("kilo-desktop.exe"), b"Squirrel stub fixture").unwrap();
    assert_eq!(
        kilo_versioned_executable(&root, false).unwrap(),
        root.join("app-0.1.11/kilo-desktop.exe")
    );
    assert_eq!(
        kilo_versioned_executable(&root, true).unwrap(),
        root.join("app-0.1.11/resources/kilo/windows-x64-baseline/bin/kilo.exe")
    );
    fs::remove_file(root.join("app-0.1.11/kilo-desktop.exe")).unwrap();
    assert_eq!(
        kilo_versioned_executable(&root, false).unwrap(),
        root.join("app-0.1.9/kilo-desktop.exe")
    );
    fs::remove_file(root.join("app-0.1.9/kilo-desktop.exe")).unwrap();
    assert_eq!(
        kilo_versioned_executable(&root, false).unwrap(),
        root.join("kilo-desktop.exe")
    );
    fs::remove_file(root.join("kilo-desktop.exe")).unwrap();
    assert!(kilo_versioned_executable(&root, false).is_none());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[tokio::test]
async fn kilo_desktop_stub_file_and_directory_resolve_to_the_same_native_version() {
    let root = std::env::temp_dir().join(format!("kilo-native-{}", uuid::Uuid::new_v4()));
    for version in ["0.1.9", "0.1.11"] {
        let directory = root.join(format!("app-{version}"));
        fs::create_dir_all(directory.join("resources/kilo/windows-x64-baseline/bin")).unwrap();
        fs::write(directory.join("kilo-desktop.exe"), b"MZnative fixture").unwrap();
        fs::write(
            directory.join("resources/kilo/windows-x64-baseline/bin/kilo.exe"),
            b"MZCLI fixture",
        )
        .unwrap();
    }
    fs::write(root.join("kilo-desktop.exe"), b"MZstub fixture").unwrap();
    fs::write(root.join("Update.exe"), b"MZupdater fixture").unwrap();
    let mut definition: PathsConfig =
        serde_json::from_str(include_str!("../../../tools/kilodesktop/paths.json")).unwrap();
    definition.install_hints = None;
    let expected = root.join("app-0.1.11/kilo-desktop.exe");
    for candidate in [
        root.join("kilo-desktop.exe"),
        root.clone(),
        expected.clone(),
    ] {
        definition.paths.win32 = Some(vec![candidate.to_string_lossy().into_owned()]);
        let expected = Some(expected.clone());
        assert_eq!(detect_tool(&definition).await.map(PathBuf::from), expected);
        assert_eq!(
            find_tool_executable(&definition, false).map(PathBuf::from),
            expected
        );
        assert_eq!(
            find_tool_executable(&definition, true).map(PathBuf::from),
            expected
        );
    }
    let embedded = kilo_versioned_executable(expected.parent().unwrap(), true).unwrap();
    assert_eq!(
        embedded,
        root.join("app-0.1.11/resources/kilo/windows-x64-baseline/bin/kilo.exe")
    );
    assert!(kilo_native_binary(&embedded));
    fs::remove_dir_all(root).unwrap();
}

/// Detect if a tool is installed, returns executable path
async fn detect_tool(pc: &PathsConfig) -> Option<String> {
    // 0. Built-in tools (always installed)
    if pc.always_installed {
        return Some("built-in".to_string());
    }

    // Both applications use the devin leaf name; PATH alone cannot identify the GUI.
    if matches!(pc.name.as_str(), "Devin" | "Devin CLI") {
        return find_tool_executable(pc, true);
    }

    // Store registration is authoritative, even before the first client launch.
    #[cfg(windows)]
    if let Some(uri) = pc
        .launch_uri
        .as_deref()
        .and_then(super::msix::resolve_launch_uri)
    {
        return Some(uri);
    }

    // 1. Check custom env var
    if let Some(ref env_var) = pc.env_var {
        if let Ok(custom_path) = std::env::var(env_var) {
            let expanded = expand_path(&custom_path);
            if if matches!(env_var.as_str(), "COPILOT_CLI_PATH" | "MCODE_PATH") {
                valid_executable(pc, &expanded)
            } else {
                expanded.exists()
            } {
                return Some(expanded.to_string_lossy().to_string());
            }
        }
    }

    // 2. Check PATH for command
    if !pc.command.is_empty() {
        if let Some(path) = platform::get_command_path(&pc.command).await {
            if pc.name == "Cursor CLI" {
                return valid_executable(pc, Path::new(&path)).then_some(path);
            }
            if !pc.require_config_file || config_file_exists(pc) {
                return Some(path);
            }
            log::info!(
                "[{}] Command found in PATH but config file missing",
                pc.name
            );
        }
    }

    // 2.5. Check Python module (pip-installed CLI tools)
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

    // Paths and OS install hints use the same resolver as the launcher.
    if !pc.require_config_file || config_file_exists(pc) {
        if let Some(path) = find_tool_executable(pc, true) {
            return Some(path);
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

    // 5. Detect by config directory existence (GUI desktop apps without a fixed
    // install location, e.g. install-anywhere desktop apps).
    //
    // A lingering config directory is a WEAK signal: most uninstallers leave
    // ~/.<tool> behind on purpose (user data is preserved across reinstalls).
    // So config-dir presence may only stand in for "installed" when there is NO
    // stronger detector to contradict it. If the tool defines platform paths OR
    // install hints (registry / Applications / .desktop scan), those are
    // authoritative and have ALREADY been checked above — reaching here means
    // they all failed, i.e. the app is uninstalled and the config dir is stale.
    // Only treat the config dir as proof of installation for tools that have no
    // other detector at all.
    if pc.detect_by_config_dir && !pc.config_dir.is_empty() {
        let has_authoritative = has_authoritative_detector(pc);

        let config_dir = expand_path(&pc.config_dir);
        let found_dir = if config_dir.exists() {
            Some(config_dir)
        } else {
            // Fall back to the alternate config dir (e.g. Windows
            // %LOCALAPPDATA%\hermes vs Unix ~/.hermes).
            pc.config_dir_alt
                .as_ref()
                .map(|alt| expand_path(alt))
                .filter(|dir| dir.exists())
        };

        if let Some(dir) = found_dir {
            if has_authoritative {
                log::info!(
                    "[{}] Config directory {:?} exists but no executable / install-hint match found — app likely uninstalled (stale config), skipping",
                    pc.name,
                    dir
                );
            } else {
                log::info!(
                    "[{}] Config directory found: {:?}, treated as installed",
                    pc.name,
                    dir
                );
                return Some(dir.to_string_lossy().to_string());
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
        let npm_output = crate::utils::process::async_command("npm")
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
        "Cloud Agent" => ToolCategory::CloudAgent,
        "Utility" => ToolCategory::Utility,
        "Science" => ToolCategory::Science,
        "Creative" => ToolCategory::Creative,
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

#[cfg(windows)]
fn read_registry_entries() -> Vec<HashMap<String, String>> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    let mut entries = Vec::new();
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
            entries.push(
                [
                    "DisplayName",
                    "Publisher",
                    "DisplayIcon",
                    "InstallLocation",
                    "UninstallString",
                    "DisplayVersion",
                ]
                .into_iter()
                .filter_map(|name| {
                    entry.get_value::<String, _>(name).ok().map(|value| {
                        let value = if matches!(
                            name,
                            "DisplayIcon" | "InstallLocation" | "UninstallString"
                        ) {
                            expand_path(&value).to_string_lossy().into_owned()
                        } else {
                            value
                        };
                        (name.to_owned(), value)
                    })
                })
                .collect(),
            );
        }
    }
    entries
}

#[cfg(target_os = "linux")]
fn read_desktop_entries() -> Vec<(PathBuf, String)> {
    let mut search_dirs: Vec<PathBuf> = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
        PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        search_dirs.push(home.join(".local/share/applications"));
    }

    let mut collected = Vec::new();
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
            collected.push((path, content));
        }
    }
    collected
}

struct DiscoverySnapshot {
    #[cfg(windows)]
    registry: Arc<Vec<HashMap<String, String>>>,
    #[cfg(windows)]
    msix: Option<super::msix::Snapshot>,
    #[cfg(target_os = "linux")]
    desktop: Arc<Vec<(PathBuf, String)>>,
    #[cfg(target_os = "macos")]
    applications: Option<Arc<Vec<PathBuf>>>,
}

tokio::task_local! { static SCAN_DISCOVERY: Arc<DiscoverySnapshot>; }

impl DiscoverySnapshot {
    fn capture(_definitions: &[ToolDefinition]) -> Self {
        Self {
            #[cfg(windows)]
            registry: Arc::new(read_registry_entries()),
            #[cfg(windows)]
            msix: super::msix::Snapshot::capture(
                &_definitions
                    .iter()
                    .filter_map(|definition| definition.paths_config.launch_uri.clone())
                    .collect::<Vec<_>>(),
            ),
            #[cfg(target_os = "linux")]
            desktop: Arc::new(read_desktop_entries()),
            #[cfg(target_os = "macos")]
            applications: crate::utils::process::command("mdfind")
                .arg("kMDItemContentType == 'com.apple.application-bundle'")
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| {
                    Arc::new(
                        String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .map(PathBuf::from)
                            .collect(),
                    )
                }),
        }
    }
}

#[cfg(windows)]
pub(crate) fn discovery_msix_uri(configured: &str) -> Option<Option<String>> {
    SCAN_DISCOVERY
        .try_with(|snapshot| {
            snapshot
                .msix
                .as_ref()
                .map(|applications| applications.resolve(configured))
        })
        .ok()
        .flatten()
}

// ─── Global tool definitions cache ───

use std::sync::{Arc, Mutex};

static TOOL_DEFINITIONS: Mutex<Option<Arc<Vec<ToolDefinition>>>> = Mutex::new(None);

/// Get or load tool definitions (cached)
fn get_definitions() -> Arc<Vec<ToolDefinition>> {
    let mut cache = TOOL_DEFINITIONS.lock().unwrap();
    if cache.is_none() {
        *cache = Some(Arc::new(load_tool_definitions()));
    }
    cache.as_ref().unwrap().clone()
}

/// Build the pre-filled seed for the user override file: every overridable
/// tool (built-in games excluded) paired with its bundled candidate paths for
/// the CURRENT OS, expanded to concrete locations and normalised to forward
/// slashes so they read cleanly in JSON and stay easy to edit. The user opens
/// this already-populated list and just corrects the one path that's wrong —
/// no format to learn, no line to author from scratch. Sorted by tool id for a
/// stable, scannable file.
///
/// Loads defs fresh with overrides OFF (not the cached `get_definitions`): the
/// seed must be pure bundled defaults, so re-creating the file after a reset
/// can't fold the user's own overrides back in as "defaults".
pub fn default_override_seed() -> Vec<(String, Vec<String>)> {
    let mut defs = load_tool_definitions_with_overrides(false);
    defs.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = Vec::new();
    for def in defs {
        // Built-in tools (Reversi, Translator, ...) are always installed and
        // have no real install path to override — skip them.
        if def.paths_config.always_installed {
            continue;
        }
        let mut paths: Vec<String> = Vec::new();
        for p in get_platform_paths(&def.paths_config.paths) {
            // Forward slashes (not normalize_for_display's backslashes) so the
            // JSON has no escaped "\\" and reads naturally; expand_path/exists
            // accept "/" on Windows too.
            let concrete = expand_path(&p).to_string_lossy().replace('\\', "/");
            if !concrete.is_empty() && !paths.contains(&concrete) {
                paths.push(concrete);
            }
        }
        out.push((def.id, paths));
    }
    out
}

/// Merge the default-override seed into an existing `~/.echobird/tool-paths.json`'s
/// parsed contents, returning the map to write back (or `None` when the file is
/// already complete / unreadable, so the caller leaves it alone).
///
/// `existing` is:
/// - `Some(empty object)` to force a full seed (file missing — every tool is
///   "missing", so all are inserted);
/// - `Some(object)` to self-heal an existing file that predates a tool shipped
///   in a later release (e.g. Kimi Code, added in v5.4.3);
/// - `None` when the existing file couldn't be parsed as a JSON object — the
///   user's malformed-but-edited file is left untouched rather than clobbered.
///
/// Only ADDS tool ids the seed provides that the file is missing. Existing
/// entries, user edits, and "_"-prefixed note keys (ignored by the scanner —
/// see `load_user_path_overrides`) are preserved verbatim, so a user's
/// customizations always survive an EchoBird update that ships new tools.
pub fn merge_override_seed(
    existing: Option<&serde_json::Value>,
    seed: &[(String, Vec<String>)],
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let mut map = match existing? {
        serde_json::Value::Object(m) => m.clone(),
        _ => return None,
    };
    let mut changed = false;
    for (tool_id, paths) in seed {
        if !map.contains_key(tool_id) {
            map.insert(
                tool_id.clone(),
                serde_json::Value::Array(
                    paths
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            changed = true;
        }
    }
    if changed {
        Some(map)
    } else {
        None
    }
}

pub(crate) fn model_config_paths() -> (Vec<PathBuf>, Vec<String>) {
    let mut paths = Vec::new();
    let mut errors = Vec::new();
    for def in get_definitions().iter() {
        match model_config_path(def) {
            Ok(path) if !path.as_os_str().is_empty() => paths.push(path),
            Ok(_) => {}
            Err(error) => errors.push(format!("{} config path: {error}", def.id)),
        }
        paths.push(platform::echobird_dir().join(format!("{}.json", def.id)));
    }
    (paths, errors)
}

fn model_config_path(def: &ToolDefinition) -> Result<PathBuf, String> {
    if def.id == "hermes" {
        return Ok(super::tool_config_manager::hermes::config_path());
    }
    if def.id == "zcode" {
        return super::zcode_paths::resolve()
            .map(|paths| paths.config_dir.join("provider_config.json"));
    }
    if def.id == "vscode" {
        Ok(super::tool_config_manager::vscode::settings_path())
    } else if def.id == "musecode" {
        Ok(super::tool_config_manager::musecode::settings_path())
    } else if matches!(def.id.as_str(), "cline" | "clinedesktop") {
        Ok(super::tool_config_manager::cline::config_path())
    } else {
        Ok(expand_path(&def.config_mapping.config_file))
    }
}

/// Get the config mapping for a specific tool
pub fn get_tool_config_mapping(tool_id: &str) -> Option<(ToolDefinition, PathBuf)> {
    let defs = get_definitions();
    defs.iter().find(|d| d.id == tool_id).and_then(|def| {
        let config_path = model_config_path(def).ok()?;
        Some((def.clone(), config_path))
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
    find_tool_executable(&def.paths_config, true)
}

/// Whether any of the clients is installed, by executable or Store package.
/// Account reads use it so an uninstalled client's leftover files stay unread.
pub(crate) fn any_tool_installed(tool_ids: &[&str]) -> bool {
    tool_ids
        .iter()
        .any(|id| get_tool_exe_path(id).is_some() || store_package_installed(id))
}

/// A configured launch URI alone is not an installation; the Store package must
/// resolve. (`get_tool_launch_uri` falls back to the configured value.)
fn store_package_installed(tool_id: &str) -> bool {
    #[cfg(windows)]
    {
        get_definitions()
            .iter()
            .find(|d| d.id == tool_id)
            .and_then(|d| d.paths_config.launch_uri.as_deref())
            .and_then(super::msix::resolve_launch_uri)
            .is_some()
    }
    #[cfg(not(windows))]
    {
        let _ = tool_id;
        false
    }
}

pub(crate) struct QoderCampaignRuntime {
    pub path: PathBuf,
    pub version: Option<String>,
}

fn qoder_resources(platform: &str, executable: &Path) -> Option<PathBuf> {
    if platform == "macos" {
        Some(executable.parent()?.parent()?.join("Resources"))
    } else {
        Some(executable.parent()?.join("resources"))
    }
}

pub(crate) fn packaged_desktop_metadata(executable: &Path) -> Option<serde_json::Value> {
    let executable = fs::canonicalize(executable).ok()?;
    packaged_metadata(&qoder_resources(std::env::consts::OS, &executable)?)
}

pub(crate) fn doubao_desktop_metadata(tool: &str) -> Option<serde_json::Value> {
    if !matches!(tool, "doubaowork" | "dola") {
        return None;
    }
    let executable = PathBuf::from(get_tool_exe_path(tool)?);
    #[cfg(target_os = "macos")]
    {
        let info =
            plist::Value::from_file(executable.parent()?.parent()?.join("Info.plist")).ok()?;
        let version = info
            .as_dictionary()?
            .get("CFBundleShortVersionString")?
            .as_string()?;
        Some(serde_json::json!({"version":version}))
    }
    #[cfg(not(target_os = "macos"))]
    {
        serde_json::from_slice(&fs::read(executable.with_file_name("manifest.json")).ok()?).ok()
    }
}

fn qwenwork_versioned_executable(pc: &PathsConfig, root: &Path) -> Option<PathBuf> {
    let name = if pc.name == "QwenWork" {
        "QwenWork.exe"
    } else {
        "QwenWorkCN.exe"
    };
    let updater = root.join("updater.cfg");
    if updater.is_file() {
        if fs::metadata(&updater).ok()?.len() > 65_536 {
            return None;
        }
        let config: serde_json::Value = serde_json::from_slice(&fs::read(updater).ok()?).ok()?;
        let version = config["currentVersion"].as_str()?;
        if !version.starts_with(|c: char| c.is_ascii_digit())
            || !version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        {
            return None;
        }
        let executable = root.join(version).join(name);
        return valid_executable(pc, &executable).then_some(executable);
    }
    let mut candidates: Vec<_> = fs::read_dir(root)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path().join(name))
        .filter(|path| valid_executable(pc, path))
        .collect();
    candidates.sort_by(|a, b| {
        let version = |path: &Path| {
            path.parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .unwrap_or("")
                .split(['.', '-'])
                .map(|part| part.parse::<u64>().unwrap_or(0))
                .collect::<Vec<_>>()
        };
        version(b).cmp(&version(a))
    });
    candidates.into_iter().next()
}

fn qoder_package_version(resources: &Path) -> Option<String> {
    let value = packaged_metadata(resources)?;
    let version = value["version"].as_str()?.trim();
    (!version.is_empty() && version.len() <= 96 && version.bytes().all(|b| b.is_ascii_graphic()))
        .then(|| version.to_string())
}

fn packaged_metadata(resources: &Path) -> Option<serde_json::Value> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(resources.join("app.asar")).ok()?;
    let mut prefix = [0u8; 8];
    file.read_exact(&mut prefix).ok()?;
    let header_size = u32::from_le_bytes(prefix[4..8].try_into().ok()?) as usize;
    if !(8..=16_777_216).contains(&header_size) {
        return None;
    }
    let mut header = vec![0u8; header_size];
    file.read_exact(&mut header).ok()?;
    let json_size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
    let tree: serde_json::Value = serde_json::from_slice(header.get(8..8 + json_size)?).ok()?;
    let package = &tree["files"]["package.json"];
    let offset = package["offset"].as_str()?.parse::<u64>().ok()?;
    let size = package["size"].as_u64()?;
    if size > 65_536 || package["unpacked"] == true {
        return None;
    }
    file.seek(SeekFrom::Start(
        (header_size as u64 + 8).checked_add(offset)?,
    ))
    .ok()?;
    let mut bytes = vec![0u8; size as usize];
    file.read_exact(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn get_qoder_campaign_runtime_path(
    tool_id: &str,
) -> Result<QoderCampaignRuntime, String> {
    use super::qoder_accounts::Edition;
    let edition = match tool_id {
        "qoder" => Edition::Qoder,
        "qodercn" => Edition::QoderCn,
        _ => return Err("accountError.invalidAccount".into()),
    };
    let executable = get_tool_exe_path(tool_id).ok_or("accountError.unavailable")?;
    let executable = fs::canonicalize(executable).map_err(|_| "accountError.unavailable")?;
    if super::qoder_native::desktop::identity(&executable)
        != Some((edition, super::qoder_native::desktop::ProductKind::App))
    {
        return Err("accountError.invalidAccount".into());
    }
    let resources =
        qoder_resources(std::env::consts::OS, &executable).ok_or("accountError.unavailable")?;
    let path = resources.join("umid").join(if cfg!(windows) {
        "runtime-info.exe"
    } else {
        "runtime-info"
    });
    if !path.is_file() {
        return Err("accountError.initializeClient|Qoder device identity helper is missing".into());
    }
    Ok(QoderCampaignRuntime {
        path,
        version: qoder_package_version(&resources),
    })
}

/// The official desktop SDK downloads a CLI runtime, but that cache is not a standalone installation.
pub(crate) fn get_copilot_runtime_path() -> Option<String> {
    get_tool_exe_path("copilot").or_else(|| {
        get_tool_exe_path("copilotdesktop")?;
        let root = dirs::cache_dir()?.join("github-copilot-sdk/cli");
        copilot_cached_runtime(&root).map(|path| path.to_string_lossy().into_owned())
    })
}

fn copilot_cached_runtime(root: &Path) -> Option<PathBuf> {
    let mut versions: Vec<(Vec<u32>, PathBuf)> = fs::read_dir(root)
        .ok()?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let version = name
                .to_str()?
                .split(['.', '-'])
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            let dir = entry.path();
            if !dir.join(".copilot-cli.ok").is_file() {
                return None;
            }
            let path = dir.join(if cfg!(windows) {
                "copilot.exe"
            } else {
                "copilot"
            });
            if !path.is_file() {
                return None;
            }
            #[cfg(windows)]
            {
                use std::io::Read;
                let mut signature = [0; 2];
                fs::File::open(&path)
                    .ok()?
                    .read_exact(&mut signature)
                    .ok()?;
                if signature != *b"MZ" {
                    return None;
                }
            }
            Some((version, path))
        })
        .collect();
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, path)| path).next()
}

#[test]
fn copilot_runtime_requires_the_sdk_marker_and_prefers_a_numeric_new_version() {
    let root = std::env::temp_dir().join(format!("copilot-path-{}", uuid::Uuid::new_v4()));
    let file = if cfg!(windows) {
        "copilot.exe"
    } else {
        "copilot"
    };
    for version in ["1.0.9-3", "1.0.94-3", "1.0.100-1"] {
        let dir = root.join(version);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(file), b"MZfixture").unwrap();
        if version != "1.0.100-1" {
            fs::write(dir.join(".copilot-cli.ok"), b"").unwrap();
        }
    }
    assert_eq!(
        copilot_cached_runtime(&root).unwrap(),
        root.join("1.0.94-3").join(file)
    );
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn copilot_environment_path_is_shared_by_detection_and_executable_resolution() {
    struct RestoreEnvironment(Option<std::ffi::OsString>);
    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("COPILOT_CLI_PATH", value);
            } else {
                std::env::remove_var("COPILOT_CLI_PATH");
            }
        }
    }
    let _restore = RestoreEnvironment(std::env::var_os("COPILOT_CLI_PATH"));
    let root = std::env::temp_dir().join(format!("copilot-env-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("bin with spaces")).unwrap();
    let executable = root.join("bin with spaces").join(if cfg!(windows) {
        "copilot.cmd"
    } else {
        "copilot"
    });
    fs::write(&executable, b"fixture").unwrap();
    let mut definition: PathsConfig =
        serde_json::from_str(include_str!("../../../tools/copilot/paths.json")).unwrap();
    definition.command.clear();
    definition.paths.win32 = Some(Vec::new());
    definition.paths.darwin = Some(Vec::new());
    definition.paths.linux = Some(Vec::new());
    std::env::set_var("COPILOT_CLI_PATH", &executable);
    let expected = Some(executable.to_string_lossy().into_owned());
    assert_eq!(detect_tool(&definition).await, expected);
    assert_eq!(find_tool_executable(&definition, true), expected);
    assert_eq!(find_tool_executable(&definition, false), expected);
    std::env::set_var("COPILOT_CLI_PATH", executable.parent().unwrap());
    assert!(detect_tool(&definition).await.is_none());
    assert!(find_tool_executable(&definition, true).is_none());
    fs::remove_file(&executable).unwrap();
    std::env::set_var("COPILOT_CLI_PATH", &executable);
    assert!(detect_tool(&definition).await.is_none());
    assert!(find_tool_executable(&definition, false).is_none());
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn copilot_desktop_linux_catalog_resolves_both_official_appimage_architectures() {
    let mut definition: PathsConfig =
        serde_json::from_str(include_str!("../../../tools/copilotdesktop/paths.json")).unwrap();
    let linux_paths = definition.paths.linux.take().unwrap();
    let names = filenames_of(&linux_paths);
    assert_eq!(
        names,
        [
            "GitHub-Copilot-linux-x64.AppImage",
            "GitHub-Copilot-linux-arm64.AppImage"
        ]
    );
    let root = std::env::temp_dir().join(format!("copilot-appimages-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let paths: Vec<String> = names
        .iter()
        .map(|name| root.join(name).to_string_lossy().into_owned())
        .collect();
    definition.paths.win32 = Some(paths.clone());
    definition.paths.darwin = Some(paths.clone());
    definition.paths.linux = Some(paths);
    definition.install_hints = None;
    for name in names {
        let executable = root.join(name);
        fs::write(&executable, b"AppImage filename fixture").unwrap();
        let expected = Some(executable.to_string_lossy().into_owned());
        assert_eq!(detect_tool(&definition).await, expected);
        assert_eq!(find_tool_executable(&definition, true), expected);
        fs::remove_file(&executable).unwrap();
    }
    assert!(detect_tool(&definition).await.is_none());
    assert!(find_tool_executable(&definition, false).is_none());
    fs::remove_dir(root).unwrap();
}

fn resolve_install_directory(hit_path: &Path, platform_paths: &[String]) -> Option<String> {
    if hit_path.is_dir() {
        let root = if hit_path.extension().is_some_and(|ext| ext == "app") {
            hit_path.join("Contents/MacOS")
        } else {
            hit_path.to_path_buf()
        };
        let mut names = filenames_of(platform_paths);
        if hit_path.extension().is_some_and(|ext| ext == "app") {
            // Electron bundles may use a different executable name than the app.
            if let Some(name) = plist::Value::from_file(hit_path.join("Contents/Info.plist"))
                .ok()
                .and_then(|info| {
                    info.as_dictionary()?
                        .get("CFBundleExecutable")?
                        .as_string()
                        .map(str::to_owned)
                })
            {
                let mut components = Path::new(&name).components();
                if matches!(components.next(), Some(std::path::Component::Normal(_)))
                    && components.next().is_none()
                {
                    names.insert(0, name);
                }
            }
            if let Some(name) = hit_path.file_stem().and_then(|s| s.to_str()) {
                names.push(name.to_owned());
            }
        }
        // Keep each declared `<directory>\<executable>` tail ahead of bare file
        // names, so a runtime in `app\` wins over its launcher in the install root.
        let declared_tails = platform_paths.iter().filter_map(|path| {
            let path = Path::new(path);
            Some(Path::new(path.parent()?.file_name()?).join(path.file_name()?))
        });
        let candidates: Vec<PathBuf> = declared_tails
            .chain(names.iter().map(PathBuf::from))
            .collect();
        for name in candidates {
            let candidate = root.join(&name);
            if candidate.is_file() {
                log::info!(
                    "[InstallHints] resolved executable: {}",
                    candidate.display()
                );
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

#[test]
fn resolves_nonstandard_app_bundles_and_plain_install_directories() {
    let dir = std::env::temp_dir().join(format!("tool-path-{}", uuid::Uuid::new_v4()));
    let bundle = dir.join("User Applications/Grok Bot.app");
    let binary = bundle.join("Contents/MacOS/Grok Bot");
    fs::create_dir_all(binary.parent().unwrap()).unwrap();
    fs::write(&binary, b"fixture").unwrap();
    let declared = vec!["/Applications/Grok Bot.app/Contents/MacOS/Grok Bot".to_string()];
    let resolved = resolve_install_directory(&bundle, &declared).unwrap();
    assert_eq!(
        fs::canonicalize(resolved).unwrap(),
        fs::canonicalize(&binary).unwrap()
    );
    let plain = dir.join("plain");
    fs::create_dir(&plain).unwrap();
    fs::write(plain.join("tool.exe"), b"fixture").unwrap();
    assert_eq!(
        resolve_install_directory(&plain, &["/default/tool.exe".into()]),
        Some(plain.join("tool.exe").to_string_lossy().into())
    );
    // A custom bundle can resolve its own executable even without a matching default filename.
    assert_eq!(
        fs::canonicalize(resolve_install_directory(&bundle, &["/missing".into()]).unwrap())
            .unwrap(),
        fs::canonicalize(&binary).unwrap()
    );
    fs::remove_file(&binary).unwrap();
    assert!(resolve_install_directory(&bundle, &declared).is_none());
    fs::remove_dir_all(dir).unwrap();
}

/// Like [`get_tool_exe_path`] but ONLY checks the explicit platform paths,
/// skipping the registry install-hints fallback. The launcher uses this to
/// decide whether a *directly launchable* exe exists on disk (e.g. a Squirrel
/// install of Claude Desktop at %LOCALAPPDATA%\AnthropicClaude\Claude.exe)
/// that should be preferred over a Store AUMID. A true MSIX/Store install has
/// no exe at the declared paths (it lives under WindowsApps), so this returns
/// None and the caller keeps using the AUMID — no regression for Store
/// installs.
pub fn get_tool_declared_exe_path(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    let def = defs.iter().find(|d| d.id == tool_id)?;
    find_tool_executable(&def.paths_config, false)
}

/// True if the tool is a GUI desktop app whose provider config EchoBird
/// manages — the Desktop category plus OpenScience in Science, excluding
/// `noModelConfig` tools. These
/// load provider config at startup, so EchoBird kills + relaunches them on
/// launch; otherwise switching the model while the app is open silently fails
/// (the running instance keeps the old config). Deliberately excluded:
///   • CLI tools — never `category: "Desktop"`, and they re-read config every
///     invocation anyway.
///   • No-model-config desktop viewers (Gemini Desktop, Coffee CLI) — EchoBird
///     doesn't switch their provider, so a restart would pick up nothing.
pub fn is_managed_desktop_tool(tool_id: &str) -> bool {
    get_definitions()
        .iter()
        .find(|d| d.id == tool_id)
        .map(|d| {
            (d.paths_config.category.eq_ignore_ascii_case("desktop")
                || matches!(d.id.as_str(), "openscience" | "claudescience"))
                && !d.paths_config.no_model_config
        })
        .unwrap_or(false)
}

/// Candidate process image names for a desktop tool on the current OS. Most
/// tools derive these from the filenames of their declared executable paths;
/// `processNames` supplies an explicit override for wrappers such as AppImages
/// whose installed filename differs from the running image.
pub fn get_tool_process_names(tool_id: &str) -> Vec<String> {
    let defs = get_definitions();
    let Some(def) = defs.iter().find(|d| d.id == tool_id) else {
        return Vec::new();
    };
    if let Some(names) = def.paths_config.process_names.as_ref() {
        let configured = get_platform_paths(names);
        if !configured.is_empty() {
            return configured;
        }
    }
    filenames_of(&get_platform_paths(&def.paths_config.paths))
}

/// Pure helper: unique filenames extracted from a list of full paths.
fn filenames_of(paths: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for p in paths {
        if let Some(name) = std::path::Path::new(p).file_name().and_then(|n| n.to_str()) {
            let name = name.to_string();
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
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

/// Prefer the install catalog's homepage over legacy website values, which
/// sometimes point at a repository. A repository is the final fallback.
fn resolve_tool_website(website: Option<&str>, install_ref: Option<&str>) -> Option<String> {
    let install = install_ref.and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());
    let url = [
        install
            .as_ref()
            .and_then(|value| value["homepage"].as_str()),
        website,
        install.as_ref().and_then(|value| value["github"].as_str()),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .find(|url| !url.is_empty())
    .map(str::to_owned);
    url
}

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
        if def.id == "vscode" {
            version = get_tool_exe_path("vscode").and_then(|path| {
                super::tool_config_manager::vscode::installed_version(Path::new(&path))
            });
        } else if version.is_none() && !pc.command.is_empty() && !pc.no_model_config {
            let command = if matches!(def.id.as_str(), "kilo" | "copilot") {
                installed_path.as_deref().unwrap_or(&pc.command)
            } else {
                &pc.command
            };
            version = platform::get_version(command).await;
        }
        // Desktop GUI apps have no CLI `--version` (their `command` is empty),
        // but Windows records a clean DisplayVersion in the registry Uninstall
        // entry. When the tool is registry-detectable (install hints present)
        // and we still have no version, read it — so WorkBuddy and, equally,
        // Claude/Codex/Gemini Desktop surface their version on the card.
        #[cfg(windows)]
        if version.is_none() {
            if let Some(ref hints) = pc.install_hints {
                version = scan_windows_registry_version(hints);
            }
        }
    }

    let active_model = if installed {
        read_active_model(&def).await
    } else {
        None
    };

    let config_path = if installed && !def.config_mapping.config_file.is_empty() {
        model_config_path(&def)
            .ok()
            .map(|cp| normalize_for_display(cp.to_string_lossy().to_string()))
    } else {
        None
    };

    let detected_path = if pc.always_installed {
        // Resolve bundle-relative launch paths, falling back to the definition directory.
        if let Some(ref launch) = pc.launch_file {
            let tools_dir = find_tools_dir().unwrap_or_default();
            let launch_path = tools_dir.join(&def.id).join(launch);
            if launch_path.exists() {
                Some(normalize_for_display(
                    launch_path.to_string_lossy().to_string(),
                ))
            } else {
                Some(normalize_for_display(def.tool_dir.clone()))
            }
        } else {
            Some(normalize_for_display(def.tool_dir.clone()))
        }
    } else {
        // PATH-discovered CLI tools (e.g. `which coffee-cli` under Git Bash)
        // return forward-slash paths. Normalize so the card shows `\` on
        // Windows — same treatment the always_installed branch already gets.
        installed_path.map(normalize_for_display)
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
        website: resolve_tool_website(
            pc.website.as_deref(),
            super::bundled_assets::get_install_ref(&def.id),
        ),
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
        no_cwd_prompt: pc.no_cwd_prompt,
        launch_uri: pc.launch_uri.clone(),
    }
}

/// Get the launch URI (e.g. "shell:AppsFolder\\<AUMID>") for an MSIX/Store app.
pub fn get_tool_launch_uri(tool_id: &str) -> Option<String> {
    let defs = get_definitions();
    let configured = defs
        .iter()
        .find(|d| d.id == tool_id)?
        .paths_config
        .launch_uri
        .clone();
    #[cfg(windows)]
    if let Some(uri) = configured
        .as_deref()
        .and_then(super::msix::resolve_launch_uri)
    {
        return Some(uri);
    }
    #[cfg(windows)]
    if tool_id == "hermes" {
        // Source installs use the GUI path, even when only registry hints find it.
        return None;
    }
    configured
}

/// Scan all installed tools — runs all detections in parallel for fast completion.
pub async fn scan_tools() -> Vec<DetectedTool> {
    if let Err(error) = system_applications::list(true).await {
        log::warn!("[Applications] Keeping previous system inventory: {error}");
    }
    // Clear cache to pick up newly added tool directories
    {
        let mut cache = TOOL_DEFINITIONS.lock().unwrap();
        *cache = None;
    }
    let definitions = get_definitions();

    let snapshot_definitions = definitions.clone();
    let snapshot = Arc::new(
        tokio::task::spawn_blocking(move || DiscoverySnapshot::capture(&snapshot_definitions))
            .await
            .expect("installation snapshot task"),
    );
    // Spawn a task per tool so all detections run concurrently
    let mut handles = Vec::with_capacity(definitions.len());
    for index in 0..definitions.len() {
        let definitions = definitions.clone();
        let snapshot = snapshot.clone();
        handles.push(tokio::spawn(SCAN_DISCOVERY.scope(snapshot, async move {
            scan_single_tool(definitions[index].clone()).await
        })));
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

#[cfg(test)]
mod tests {
    #[test]
    fn source_resources_follow_the_compiled_checkout_not_the_target_directory() {
        let root =
            std::env::temp_dir().join(format!("echobird-performance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.as_path().join("checkout-one");
        let second = root.as_path().join("checkout-two");
        std::fs::create_dir_all(first.join("tools")).unwrap();
        std::fs::create_dir_all(second.join("tools")).unwrap();
        assert_eq!(
            super::source_tools_dir(&first.join("src-tauri")),
            Some(first.join("tools"))
        );
        assert_eq!(
            super::source_tools_dir(&second.join("src-tauri")),
            Some(second.join("tools"))
        );
        assert_eq!(
            super::source_tools_dir(&root.join("unavailable/src-tauri")),
            None
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn repeated_catalog_reads_share_immutable_definitions() {
        let first = super::get_definitions();
        let second = super::get_definitions();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn scan_snapshot_reuses_registry_path_and_version_and_remains_task_scoped() {
        use std::{collections::HashMap, sync::Arc};
        let root =
            std::env::temp_dir().join(format!("echobird-performance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.as_path().join("snapshot-fixture.exe");
        std::fs::write(&exe, "fixture").unwrap();
        let hints: crate::models::tool::InstallHints = serde_json::from_value(
            serde_json::json!({"windowsDisplayNames":["EchoBird snapshot fixture"]}),
        )
        .unwrap();
        let snapshot = Arc::new(super::DiscoverySnapshot {
            registry: Arc::new(vec![HashMap::from([
                ("DisplayName".into(), "EchoBird snapshot fixture".into()),
                ("DisplayIcon".into(), exe.to_string_lossy().into_owned()),
                ("DisplayVersion".into(), "3.2.1".into()),
            ])]),
            msix: None,
        });
        super::SCAN_DISCOVERY
            .scope(snapshot, async {
                assert_eq!(
                    super::scan_windows_registry(&hints),
                    Some(exe.to_string_lossy().into_owned())
                );
                assert_eq!(
                    super::scan_windows_registry_version(&hints).as_deref(),
                    Some("3.2.1")
                );
                std::fs::remove_file(&exe).unwrap();
                // Paths are still validated live: a removed executable is not installed.
                assert_eq!(super::scan_windows_registry(&hints), None);
            })
            .await;
        assert_eq!(super::scan_windows_registry_version(&hints), None);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn custom_directory_installs_resolve_the_declared_runtime() {
        use std::{collections::HashMap, sync::Arc};
        let root = std::env::temp_dir().join(format!("自定义目录-{}", uuid::Uuid::new_v4()));
        let entry = |name: &str, publisher: &str, dir: &std::path::Path, icon: &str| {
            HashMap::from([
                ("DisplayName".to_string(), name.to_string()),
                ("Publisher".to_string(), publisher.to_string()),
                (
                    "DisplayIcon".to_string(),
                    format!("\"{}\"", dir.join(icon).display()),
                ),
                ("InstallLocation".to_string(), String::new()),
                (
                    "UninstallString".to_string(),
                    format!("\"{}\"", dir.join("uninstall.exe").display()),
                ),
            ])
        };
        let doubao = root.join("DoubaoWork");
        let dola = root.join("Cici");
        let trae = root.join("Trae");
        for (dir, files) in [
            (&doubao, vec!["DoubaoWork.exe", "icon.ico", "uninstall.exe"]),
            (&dola, vec!["Cici.exe", "icon.ico", "uninstall.exe"]),
            (&trae, vec!["Trae.exe"]),
        ] {
            std::fs::create_dir_all(dir.join("app")).unwrap();
            for file in files {
                std::fs::write(dir.join(file), "launcher").unwrap();
            }
        }
        std::fs::write(doubao.join("app/DoubaoWork.exe"), "runtime").unwrap();
        std::fs::write(dola.join("app/Cici.exe"), "runtime").unwrap();
        let snapshot = Arc::new(super::DiscoverySnapshot {
            registry: Arc::new(vec![
                entry(
                    "豆包工作",
                    "Beijing Chuntian Zhiyun Technology Co., Ltd.",
                    &doubao,
                    "icon.ico",
                ),
                entry("Dola", "SPRING (SG) PTE. LTD.", &dola, "icon.ico"),
                entry("TRAE (User)", "Trae", &trae, "Trae.exe"),
            ]),
            msix: None,
        });
        // Default locations must miss so an installed client cannot satisfy the test.
        let catalog = |json: &str| {
            let json = json.replace("%LOCALAPPDATA%", "%LOCALAPPDATA%-missing");
            serde_json::from_str::<PathsConfig>(&json).unwrap()
        };
        let doubao_catalog = catalog(include_str!("../../../tools/doubaowork/paths.json"));
        let dola_catalog = catalog(include_str!("../../../tools/dola/paths.json"));
        let trae_catalog = catalog(include_str!("../../../tools/trae/paths.json"));
        let found = super::SCAN_DISCOVERY
            .scope(snapshot, async {
                [
                    super::find_tool_executable(&doubao_catalog, true),
                    super::find_tool_executable(&dola_catalog, true),
                    super::find_tool_executable(&trae_catalog, true),
                ]
            })
            .await;
        let found = found.map(|path| path.map(PathBuf::from));
        assert_eq!(
            found[0].as_deref(),
            Some(doubao.join("app").join("DoubaoWork.exe").as_path())
        );
        assert_eq!(
            found[1].as_deref(),
            Some(dola.join("app").join("Cici.exe").as_path())
        );
        assert_eq!(found[2].as_deref(), Some(trae.join("Trae.exe").as_path()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn qoder_campaign_resources_follow_each_desktop_package_layout() {
        for (platform, executable, resources) in [
            (
                "windows",
                "Program Dir/Qoder/qoder.exe",
                "Program Dir/Qoder/resources",
            ),
            ("linux", "/opt/Qoder/qoder", "/opt/Qoder/resources"),
            (
                "macos",
                "/Applications/Qoder.app/Contents/MacOS/Qoder",
                "/Applications/Qoder.app/Contents/Resources",
            ),
        ] {
            assert_eq!(
                super::qoder_resources(platform, std::path::Path::new(executable)),
                Some(std::path::PathBuf::from(resources))
            );
        }
    }

    #[test]
    fn qoder_campaign_version_reads_bounded_packaged_metadata_without_launching() {
        let root =
            std::env::temp_dir().join(format!("echobird-qoder-version-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let package = br#"{"version":"0.4.4"}"#;
        let tree =
            serde_json::json!({"files":{"package.json":{"offset":"0","size":package.len()}}})
                .to_string();
        let mut header = vec![0u8; 8];
        header[4..8].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        header.extend_from_slice(tree.as_bytes());
        let mut archive = vec![0u8; 8];
        archive[4..8].copy_from_slice(&(header.len() as u32).to_le_bytes());
        archive.extend(header);
        archive.extend(package);
        std::fs::write(root.join("app.asar"), &archive).unwrap();
        assert_eq!(
            super::qoder_package_version(&root).as_deref(),
            Some("0.4.4")
        );
        archive[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(root.join("app.asar"), archive).unwrap();
        assert!(super::qoder_package_version(&root).is_none());
        let resolved = root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(resolved).unwrap();
    }
    use std::fs;
    #[test]
    fn qoder_app_detection_uses_standalone_metadata_and_versioned_registry_names() {
        let root =
            std::env::temp_dir().join(format!("echobird-qoder-app-{}", uuid::Uuid::new_v4()));
        let exe = if cfg!(target_os = "macos") {
            root.join("Contents/MacOS/Qoder CN")
        } else {
            root.join("Qoder CN.exe")
        };
        let resources = if cfg!(target_os = "macos") {
            root.join("Contents/Resources")
        } else {
            root.join("resources")
        };
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::create_dir_all(&resources).unwrap();
        fs::write(&exe, []).unwrap();
        let global: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qoder/paths.json")).unwrap();
        let cn: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qodercn/paths.json")).unwrap();
        for (id, app, data, protocol, expect_global, expect_cn) in [
            (
                "qoder",
                "com.qoder.app",
                "com.qoder.app.stable",
                "qoder",
                true,
                false,
            ),
            (
                "qoder-cn",
                "com.qodercn.app",
                "com.qodercn.app.stable",
                "qoder-cn",
                false,
                true,
            ),
            (
                "qoder-cn",
                "com.qoder.app",
                "com.qodercn.app.stable",
                "qoder-cn",
                false,
                false,
            ),
            (
                "qoderwork-cn",
                "com.qodercn.app",
                "com.qodercn.app.stable",
                "qoder-cn",
                false,
                false,
            ),
        ] {
            fs::write(
                resources.join("product.json"),
                serde_json::json!({
                    "productId":id,"appId":app,"dataDirectoryName":data,"protocolScheme":protocol
                })
                .to_string(),
            )
            .unwrap();
            assert_eq!(super::valid_executable(&global, &exe), expect_global);
            assert_eq!(super::valid_executable(&cn, &exe), expect_cn);
        }
        for (prefix, own, other) in [
            ("qoder", "qoder 0.4.4", "qoder cn 0.4.4"),
            ("qoder cn", "qoder cn 0.4.4", "qoder cn cli"),
        ] {
            let prefixes = [prefix.to_owned()];
            assert!(registry_display_name_matches(own, &[], &prefixes));
            assert!(!registry_display_name_matches(other, &[], &prefixes));
            assert!(!registry_display_name_matches(
                "qoderwork 0.4.4",
                &[],
                &prefixes
            ));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "Requires an explicitly supplied installed Qoder CN desktop executable"]
    async fn installed_qoder_cn_desktop_detection_probe() {
        let expected = std::env::var("ECHOBIRD_TEST_QODER_CN_EXE").unwrap();
        let cn: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qodercn/paths.json")).unwrap();
        let detected = super::detect_tool(&cn).await.unwrap();
        let launch = super::find_tool_executable(&cn, true).unwrap();
        assert_eq!(
            fs::canonicalize(detected).unwrap(),
            fs::canonicalize(&expected).unwrap()
        );
        assert_eq!(
            fs::canonicalize(launch).unwrap(),
            fs::canonicalize(expected).unwrap()
        );
    }

    #[test]
    fn qoder_desktop_detection_validates_product_region_and_ignores_other_products() {
        let root =
            std::env::temp_dir().join(format!("echobird-qoder-products-{}", uuid::Uuid::new_v4()));
        let exe = if cfg!(target_os = "macos") {
            root.join("Qoder.app/Contents/MacOS/Native")
        } else {
            root.join("Qoder.exe")
        };
        let resources = if cfg!(target_os = "macos") {
            root.join("Qoder.app/Contents/Resources/app")
        } else {
            root.join("resources/app")
        };
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::create_dir_all(&resources).unwrap();
        fs::write(&exe, []).unwrap();
        let global: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qoder/paths.json")).unwrap();
        let cn: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qodercn/paths.json")).unwrap();
        for (name, data, protocol, expect_global, expect_cn) in [
            ("Qoder", ".qoder", "qoder", true, false),
            ("QoderCN", ".qoder-cn", "qoder-cn", false, true),
            ("QoderWork", ".qoder", "qoder", false, false),
        ] {
            fs::write(
                resources.join("product.json"),
                serde_json::json!({"nameShort":name,"dataFolderName":data,"urlProtocol":protocol})
                    .to_string(),
            )
            .unwrap();
            assert_eq!(super::valid_executable(&global, &exe), expect_global);
            assert_eq!(super::valid_executable(&cn, &exe), expect_cn);
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn qoder_catalog_has_independent_account_and_api_model_desktop_entries() {
        for (id, name, contents) in [
            (
                "qoder",
                "Qoder",
                include_str!("../../../tools/qoder/paths.json"),
            ),
            (
                "qodercn",
                "Qoder CN",
                include_str!("../../../tools/qodercn/paths.json"),
            ),
        ] {
            let config: PathsConfig = serde_json::from_str(contents).unwrap();
            assert_eq!(config.name, name);
            assert_eq!(config.api_protocol, vec!["openai", "anthropic"]);
            assert!(!config.no_model_config);
            assert!(super::has_authoritative_detector(&config));
            assert!(!config.detect_by_config_dir);
            for paths in [
                &config.paths.win32,
                &config.paths.darwin,
                &config.paths.linux,
            ] {
                assert!(!paths.as_ref().unwrap().is_empty());
            }
            assert!(crate::services::bundled_assets::INSTALLABLE_TOOL_IDS.contains(&id));
            assert_eq!(config.category, "Desktop");
        }
    }
    use super::{
        exe_stem_matches_hints, has_authoritative_detector, is_uninstaller_stem,
        merge_override_seed, parse_uninstall_exe_path, registry_display_name_matches,
    };
    // is_windows_exe is #[cfg(windows)]-gated, so the import must be too —
    // otherwise the Linux CI build fails with an unresolved import even
    // though the only test using it is also #[cfg(windows)].
    #[cfg(windows)]
    use super::is_windows_exe;
    use crate::models::tool::{PathsConfig, ToolCategory};

    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "explicit read-only installed Kilo/Copilot detection verification"]
    async fn installed_kilo_copilot_detection_probe() {
        crate::services::bundled_assets::register(&crate::BUNDLED);
        let definitions = super::get_definitions();
        for id in ["kilo", "kilodesktop", "copilot", "copilotdesktop"] {
            let definition = definitions.iter().find(|def| def.id == id).unwrap().clone();
            let detected = super::scan_single_tool(definition).await;
            eprintln!(
                "tool={id} installed={} version={}",
                detected.installed,
                detected.version.as_deref().unwrap_or("unknown")
            );
            if id != "copilot" {
                assert!(detected.installed, "expected installed client {id}");
            }
        }
        assert!(super::get_copilot_runtime_path().is_some());
    }

    #[tokio::test]
    async fn muse_cli_uses_the_shared_detector_and_never_a_stale_config_directory() {
        use std::fs;
        let mut paths: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/musecode/paths.json")).unwrap();
        assert_eq!(paths.category, "CLI Code");
        assert_eq!(paths.command, "muse");
        assert_eq!(paths.start_command.as_deref(), Some("muse"));
        assert_eq!(paths.api_protocol, vec!["openai"]);
        assert!(!paths.require_config_file);
        assert!(!paths.paths.win32.as_ref().unwrap().is_empty());
        assert!(!paths.paths.darwin.as_ref().unwrap().is_empty());
        assert!(!paths.paths.linux.as_ref().unwrap().is_empty());
        let root =
            std::env::temp_dir().join(format!("echobird-muse-detect-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let fixture_command = format!("muse-fixture-{}", uuid::Uuid::new_v4());
        let binary = root.join(if cfg!(windows) {
            format!("{fixture_command}.cmd")
        } else {
            fixture_command.clone()
        });
        fs::write(&binary, "fixture launcher").unwrap();
        let declared = Some(vec![binary.to_string_lossy().into_owned()]);
        paths.paths = crate::models::tool::PlatformPaths {
            win32: declared.clone(),
            darwin: declared.clone(),
            linux: declared,
        };
        paths.command = fixture_command;
        paths.detect_by_config_dir = true;
        paths.config_dir = root.to_string_lossy().into_owned();
        assert_eq!(
            super::find_tool_executable(&paths, false).as_deref(),
            binary.to_str()
        );
        assert_eq!(super::detect_tool(&paths).await.as_deref(), binary.to_str());
        fs::remove_file(&binary).unwrap();
        assert!(root.exists());
        assert!(super::detect_tool(&paths).await.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn hermes_desktop_requires_a_gui_instead_of_cli_or_leftover_data() {
        let mut definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/hermes/paths.json")).unwrap();
        assert!(definition.command.is_empty());
        assert!(definition.env_var.is_none());
        let root =
            std::env::temp_dir().join(format!("echobird-hermes-detect-{}", uuid::Uuid::new_v4()));
        let data = root.join("data");
        fs::create_dir_all(data.join("bin")).unwrap();
        fs::write(data.join("bin/hermes"), b"CLI fixture").unwrap();
        let executable = root.join(if cfg!(windows) {
            "desktop/Hermes.exe"
        } else {
            "desktop/Hermes"
        });
        let declared = Some(vec![executable.to_string_lossy().into_owned()]);
        definition.paths = crate::models::tool::PlatformPaths {
            win32: declared.clone(),
            darwin: declared.clone(),
            linux: declared,
        };
        definition.launch_uri = None;
        definition.install_hints = None;
        definition.config_dir = data.to_string_lossy().into_owned();
        definition.config_dir_alt = None;
        assert!(super::detect_tool(&definition).await.is_none());
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"GUI fixture").unwrap();
        assert_eq!(
            super::detect_tool(&definition).await.as_deref(),
            executable.to_str()
        );
        fs::remove_file(executable).unwrap();
        assert!(super::detect_tool(&definition).await.is_none());
        let resolved = root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(resolved).unwrap();
    }

    #[test]
    fn claude_desktop_is_never_resolved_to_the_cli_on_path() {
        let definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/claudedesktop/paths.json")).unwrap();
        assert!(!super::searches_path(&definition.name));
        assert!(super::searches_path("Fixture GUI"));
    }

    #[test]
    fn hermes_desktop_path_lookup_does_not_accept_the_cli() {
        const FIXTURE: &str = "ECHOBIRD_HERMES_PATH_FIXTURE";
        if let Some(root) = std::env::var_os(FIXTURE) {
            let root = std::path::PathBuf::from(root);
            let name = if cfg!(windows) {
                "hermes.exe"
            } else {
                "hermes"
            };
            let cli = root.join("bin").join(name);
            assert_eq!(which::which(name).unwrap(), cli);
            let mut definition: PathsConfig =
                serde_json::from_str(include_str!("../../../tools/hermes/paths.json")).unwrap();
            let missing_gui = Some(vec![root
                .join("desktop")
                .join(name)
                .to_string_lossy()
                .into_owned()]);
            definition.paths = crate::models::tool::PlatformPaths {
                win32: missing_gui.clone(),
                darwin: missing_gui.clone(),
                linux: missing_gui,
            };
            definition.install_hints = None;
            assert!(super::find_tool_executable(&definition, true).is_none());
            // The generic resolver still finds PATH executables for other tools.
            definition.name = "Fixture GUI".into();
            assert_eq!(
                super::find_tool_executable(&definition, true).as_deref(),
                cli.to_str()
            );
            println!("Hermes CLI PATH fixture verified");
            return;
        }
        let root =
            std::env::temp_dir().join(format!("echobird-hermes-path-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("bin")).unwrap();
        let cli = root.join("bin").join(if cfg!(windows) {
            "hermes.exe"
        } else {
            "hermes"
        });
        fs::write(&cli, b"fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "services::tool_manager::tests::hermes_desktop_path_lookup_does_not_accept_the_cli",
                "--nocapture",
            ])
            .env(FIXTURE, &root)
            .env("PATH", root.join("bin"))
            .output()
            .unwrap();
        let resolved = root.canonicalize().unwrap();
        assert!(resolved.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(resolved).unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .contains("Hermes CLI PATH fixture verified"),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn codearts_desktop_detection_requires_the_native_product_metadata() {
        let root = std::env::temp_dir().join(format!("echobird-codearts-{}", uuid::Uuid::new_v4()));
        let desktop = root.join("desktop");
        let other = root.join("unrelated");
        fs::create_dir_all(desktop.join("resources")).unwrap();
        fs::create_dir_all(other.join("resources")).unwrap();
        let desktop_exe = desktop.join("CodeArts.exe");
        let other_exe = other.join("codearts.exe");
        fs::write(&desktop_exe, b"fixture").unwrap();
        fs::write(&other_exe, b"fixture").unwrap();
        assert!(!super::codearts_desktop_identity(&desktop_exe));
        fs::write(
            desktop.join("resources/product.json"),
            br#"{"nameShort":"CodeArts"}"#,
        )
        .unwrap();
        assert!(super::codearts_desktop_identity(&desktop_exe));
        assert!(!super::codearts_desktop_identity(&other_exe));
        fs::write(
            other.join("resources/product.json"),
            br#"{"nameShort":"Another Product"}"#,
        )
        .unwrap();
        assert!(!super::codearts_desktop_identity(&other_exe));
        let contents = root.join("CodeArts.app/Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::create_dir_all(contents.join("Resources")).unwrap();
        fs::write(
            contents.join("Resources/product.json"),
            br#"{"nameShort":"CodeArts"}"#,
        )
        .unwrap();
        assert!(super::codearts_desktop_identity(
            &contents.join("MacOS/CodeArts")
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codearts_registry_detection_does_not_confuse_the_ide_or_cli() {
        let desktop: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/codearts/paths.json")).unwrap();
        let hints = desktop.install_hints.unwrap();
        let prefixes: Vec<_> = hints
            .windows_display_name_prefixes
            .iter()
            .map(|name| name.to_lowercase())
            .collect();
        for name in ["codearts", "codearts 0.2.6", "codearts 1.0.0"] {
            assert!(registry_display_name_matches(name, &[], &prefixes));
        }
        for name in [
            "codearts agent",
            "codearts agent 26.9.902",
            "codearts cli",
            "codeartside",
        ] {
            assert!(!registry_display_name_matches(name, &[], &prefixes));
        }
    }

    #[test]
    fn trae_registry_detection_keeps_regional_editions_separate() {
        for (catalog, display_name, other_name) in [
            (
                include_str!("../../../tools/trae/paths.json"),
                "trae",
                "trae cn",
            ),
            (
                include_str!("../../../tools/traecn/paths.json"),
                "trae cn",
                "trae",
            ),
        ] {
            let definition: PathsConfig = serde_json::from_str(catalog).unwrap();
            let hints = definition.install_hints.unwrap();
            let names: Vec<_> = hints
                .windows_display_names
                .iter()
                .map(|name| name.to_lowercase())
                .collect();
            assert!(registry_display_name_matches(display_name, &names, &[]));
            assert!(!registry_display_name_matches(other_name, &names, &[]));
            assert!(!registry_display_name_matches("trae solo", &names, &[]));
            assert!(!registry_display_name_matches("traework", &names, &[]));
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn desktop_registry_aliases_resolve_supported_clients_and_reject_dsh_wrappers() {
        use std::{collections::HashMap, sync::Arc};
        let root = std::env::temp_dir().join(format!("desktop-aliases-{}", uuid::Uuid::new_v4()));
        for (catalog, display_name, executable, supported) in [
            (
                include_str!("../../../tools/traecn/paths.json"),
                "TRAE CN (User)",
                "Trae CN/Trae CN.exe",
                true,
            ),
            (
                include_str!("../../../tools/dsh/paths.json"),
                "DSH Desktop 0.11.1",
                "DSH Desktop/DSH Desktop.exe",
                false,
            ),
            (
                include_str!("../../../tools/dsh/paths.json"),
                "DeepSeek Harness 0.2.1",
                "DeepSeek Harness/DeepSeek Harness.exe",
                true,
            ),
        ] {
            let mut definition: PathsConfig = serde_json::from_str(catalog).unwrap();
            let exe = root.join(executable);
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::write(&exe, b"fixture").unwrap();
            definition.paths.win32 = Some(vec![root
                .join("missing-default.exe")
                .to_string_lossy()
                .into_owned()]);
            let snapshot = Arc::new(super::DiscoverySnapshot {
                registry: Arc::new(vec![HashMap::from([
                    ("DisplayName".into(), display_name.into()),
                    ("DisplayIcon".into(), format!("\"{}\",0", exe.display())),
                    ("InstallLocation".into(), String::new()),
                ])]),
                msix: None,
            });
            super::SCAN_DISCOVERY
                .scope(snapshot, async {
                    let expected = supported.then(|| exe.to_string_lossy().into_owned());
                    assert_eq!(
                        super::detect_tool(&definition).await,
                        expected,
                        "{display_name}"
                    );
                    assert_eq!(super::find_tool_executable(&definition, true), expected);
                    if display_name.starts_with("TRAE") {
                        let mut international: PathsConfig =
                            serde_json::from_str(include_str!("../../../tools/trae/paths.json"))
                                .unwrap();
                        international.paths.win32 = definition.paths.win32.clone();
                        assert!(super::detect_tool(&international).await.is_none());
                    }
                    fs::remove_file(&exe).unwrap();
                    assert!(super::detect_tool(&definition).await.is_none());
                    assert!(super::find_tool_executable(&definition, true).is_none());
                })
                .await;
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn trae_bundle_detection_uses_native_metadata_and_rejects_leftover_data() {
        let root = std::env::temp_dir().join(format!("trae-bundle-{}", uuid::Uuid::new_v4()));
        for catalog in [
            include_str!("../../../tools/trae/paths.json"),
            include_str!("../../../tools/traecn/paths.json"),
        ] {
            let mut definition: PathsConfig = serde_json::from_str(catalog).unwrap();
            assert_eq!(
                super::parse_category(&definition.category),
                ToolCategory::Desktop
            );
            assert!(definition.command.is_empty());
            let bundle = root.join(format!("User Applications/{}.app", definition.name));
            let executable = bundle.join("Contents/MacOS").join("Electron");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, b"fixture").unwrap();
            let mut info = plist::Dictionary::new();
            info.insert("CFBundleExecutable".into(), "Electron".into());
            plist::Value::Dictionary(info)
                .to_file_binary(bundle.join("Contents/Info.plist"))
                .unwrap();
            assert_eq!(
                super::resolve_install_directory(
                    &bundle,
                    definition.paths.darwin.as_ref().unwrap()
                ),
                Some(executable.to_string_lossy().into_owned())
            );
            let paths = vec![bundle.to_string_lossy().into_owned()];
            definition.paths.win32 = Some(paths.clone());
            definition.paths.darwin = Some(paths.clone());
            definition.paths.linux = Some(paths);
            definition.install_hints = None;
            definition.config_dir = root.join("saved-data").to_string_lossy().into_owned();
            std::fs::create_dir_all(&definition.config_dir).unwrap();
            let expected = Some(executable.to_string_lossy().into_owned());
            assert_eq!(super::find_tool_executable(&definition, false), expected);
            assert_eq!(super::detect_tool(&definition).await, expected);
            std::fs::remove_file(executable).unwrap();
            assert!(super::find_tool_executable(&definition, true).is_none());
            assert!(super::detect_tool(&definition).await.is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn workbuddy_bundle_metadata_drives_detection_and_launch() {
        let root = std::env::temp_dir().join(format!("workbuddy-bundle-{}", uuid::Uuid::new_v4()));
        for (catalog, binary_plist) in [
            (include_str!("../../../tools/workbuddy/paths.json"), false),
            (include_str!("../../../tools/workbuddyai/paths.json"), true),
        ] {
            let mut definition: PathsConfig = serde_json::from_str(catalog).unwrap();
            let bundle = root.join(format!("User Applications/{}.app", definition.name));
            let executable = bundle.join("Contents/MacOS").join("Electron");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, b"fixture").unwrap();
            let mut info = plist::Dictionary::new();
            info.insert("CFBundleExecutable".into(), "Electron".into());
            let info = plist::Value::Dictionary(info);
            let metadata = bundle.join("Contents/Info.plist");
            if binary_plist {
                info.to_file_binary(&metadata).unwrap();
            } else {
                info.to_file_xml(&metadata).unwrap();
            }
            let declared = definition.paths.darwin.as_ref().unwrap();
            assert_eq!(
                super::resolve_install_directory(&bundle, declared),
                Some(executable.to_string_lossy().into_owned())
            );
            // The gear accepts a custom bundle path, including user Applications.
            let paths = vec![bundle.to_string_lossy().into_owned()];
            definition.paths.win32 = Some(paths.clone());
            definition.paths.darwin = Some(paths.clone());
            definition.paths.linux = Some(paths);
            definition.install_hints = None;
            definition.config_dir = bundle.to_string_lossy().into_owned();
            let expected = Some(executable.to_string_lossy().into_owned());
            assert_eq!(super::find_tool_executable(&definition, false), expected);
            assert_eq!(super::find_tool_executable(&definition, true), expected);
            assert_eq!(super::detect_tool(&definition).await, expected);
            // A leftover bundle/config directory does not count as installed.
            std::fs::remove_file(executable).unwrap();
            assert!(super::find_tool_executable(&definition, true).is_none());
            assert!(super::detect_tool(&definition).await.is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundle_metadata_preserves_legacy_fallback_and_rejects_path_traversal() {
        let root = std::env::temp_dir().join(format!("bundle-metadata-{}", uuid::Uuid::new_v4()));
        let bundle = root.join("Renamed.app");
        let executable = bundle.join("Contents/MacOS").join("WorkBuddy");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, b"fixture").unwrap();
        let metadata = bundle.join("Contents/Info.plist");
        let declared = vec!["/Applications/WorkBuddy.app/Contents/MacOS/WorkBuddy".into()];
        let electron = bundle.join("Contents/MacOS").join("Electron");
        std::fs::write(&electron, b"fixture").unwrap();
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleExecutable".into(), "Electron".into());
        plist::Value::Dictionary(info)
            .to_file_xml(&metadata)
            .unwrap();
        assert_eq!(
            super::resolve_install_directory(&bundle, &declared),
            Some(electron.to_string_lossy().into_owned())
        );
        std::fs::remove_file(electron).unwrap();
        for name in ["Electron", "../outside", "/outside"] {
            let mut info = plist::Dictionary::new();
            info.insert("CFBundleExecutable".into(), name.into());
            plist::Value::Dictionary(info)
                .to_file_xml(&metadata)
                .unwrap();
            assert_eq!(
                super::resolve_install_directory(&bundle, &declared),
                Some(executable.to_string_lossy().into_owned())
            );
        }
        std::fs::write(&metadata, b"malformed plist").unwrap();
        assert!(super::resolve_install_directory(&bundle, &declared).is_some());
        std::fs::write(bundle.join("Contents/outside"), b"fixture").unwrap();
        let mut info = plist::Dictionary::new();
        info.insert("CFBundleExecutable".into(), "../outside".into());
        plist::Value::Dictionary(info)
            .to_file_binary(&metadata)
            .unwrap();
        std::fs::remove_file(executable).unwrap();
        assert!(super::resolve_install_directory(&bundle, &declared).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn detection_and_launch_share_custom_paths_and_reject_leftover_directories() {
        let root = std::env::temp_dir().join(format!("echobird-paths-{}", uuid::Uuid::new_v4()));
        let custom = root.join("custom install");
        let missing = root.join("removed install");
        std::fs::create_dir_all(&custom).unwrap();
        std::fs::create_dir_all(&missing).unwrap();
        let binary = custom.join(format!("{}.exe", uuid::Uuid::new_v4()));
        std::fs::write(&binary, b"fixture").unwrap();
        let mut definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/chatgptdesktop/paths.json")).unwrap();
        definition.launch_uri = None;
        definition.install_hints = None;
        definition.config_dir = missing.to_string_lossy().into_owned();
        definition.detect_by_config_dir = true;
        let paths = vec![
            missing.to_string_lossy().into_owned(),
            custom.to_string_lossy().into_owned(),
            root.join(binary.file_name().unwrap())
                .to_string_lossy()
                .into_owned(),
        ];
        definition.paths.win32 = Some(paths.clone());
        definition.paths.darwin = Some(paths.clone());
        definition.paths.linux = Some(paths);
        let expected = Some(binary.to_string_lossy().into_owned());
        assert_eq!(super::find_tool_executable(&definition, false), expected);
        assert_eq!(super::find_tool_executable(&definition, true), expected);
        assert_eq!(super::detect_tool(&definition).await, expected);
        std::fs::remove_file(binary).unwrap();
        assert!(super::find_tool_executable(&definition, true).is_none());
        assert!(super::detect_tool(&definition).await.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn tool_website_prefers_homepage_over_legacy_repository() {
        assert_eq!(
            super::resolve_tool_website(
                Some("https://github.com/owner/tool"),
                Some(r#"{"homepage":"https://tool.example/","github":"https://github.com/owner/tool"}"#),
            ).as_deref(),
            Some("https://tool.example/")
        );
    }

    #[test]
    fn tool_website_keeps_configured_site_without_catalog_homepage() {
        assert_eq!(
            super::resolve_tool_website(
                Some("https://tool.example/"),
                Some(r#"{"github":"https://github.com/owner/tool"}"#),
            )
            .as_deref(),
            Some("https://tool.example/")
        );
    }

    #[test]
    fn tool_website_falls_back_to_github_when_homepage_is_missing_or_blank() {
        for install_ref in [
            r#"{"github":"https://github.com/owner/tool"}"#,
            r#"{"homepage":"  ","github":"https://github.com/owner/tool"}"#,
        ] {
            assert_eq!(
                super::resolve_tool_website(Some(" "), Some(install_ref)).as_deref(),
                Some("https://github.com/owner/tool")
            );
        }
    }

    #[test]
    fn tool_website_has_no_link_without_website_or_repository() {
        assert_eq!(super::resolve_tool_website(None, None), None);
        assert_eq!(
            super::resolve_tool_website(None, Some(r#"{"docs":"https://docs.example/"}"#)),
            None
        );
    }

    #[test]
    fn cloud_agent_category_is_preserved() {
        assert_eq!(
            super::parse_category("Cloud Agent"),
            ToolCategory::CloudAgent
        );
    }

    #[test]
    fn openscience_keeps_desktop_model_restart_in_science_category() {
        let config: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/openscience/paths.json")).unwrap();
        assert_eq!(
            super::parse_category(&config.category),
            ToolCategory::Science
        );
        assert!(super::is_managed_desktop_tool("openscience"));
        assert!(super::is_managed_desktop_tool("claudescience"));
    }

    #[test]
    fn muse_is_install_only_and_resolves_its_native_mac_bundle() {
        use std::fs;

        let config: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/muse/paths.json")).unwrap();
        let mapping: super::ConfigMapping =
            serde_json::from_str(include_str!("../../../tools/muse/config.json")).unwrap();
        assert_eq!(
            super::parse_category(&config.category),
            ToolCategory::CloudAgent
        );
        assert!(config.no_model_config && config.api_protocol.is_empty());
        assert!(config.config_dir.is_empty() && mapping.config_file.is_empty());
        assert!(mapping.read.is_none() && mapping.write.is_none());
        assert!(config.paths.win32.as_ref().unwrap().is_empty());
        assert!(config.paths.linux.as_ref().unwrap().is_empty());

        let root = std::env::temp_dir().join(format!("muse-path-{}", uuid::Uuid::new_v4()));
        let bundle = root.join("User Applications/Muse.app");
        let binary = bundle.join("Contents/MacOS/Muse");
        fs::create_dir_all(binary.parent().unwrap()).unwrap();
        fs::write(&binary, b"fixture").unwrap();
        let paths = config.paths.darwin.as_ref().unwrap();
        let resolved = super::resolve_install_directory(&bundle, paths).unwrap();
        assert_eq!(
            fs::canonicalize(resolved).unwrap(),
            fs::canonicalize(&binary).unwrap()
        );
        fs::remove_file(&binary).unwrap();
        assert!(super::resolve_install_directory(&bundle, paths).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn paths_config_from(json: serde_json::Value) -> PathsConfig {
        serde_json::from_value(json).expect("valid PathsConfig json")
    }

    #[test]
    fn filenames_of_strips_dirs_and_dedupes() {
        // Two install locations share the same exe filename → one entry; a name
        // with a space ("OpenCode Beta.exe") survives intact.
        let paths = v(&[
            "C:/Users/x/AppData/Local/AnthropicClaude/Claude.exe",
            "C:/Users/x/AppData/Local/Programs/Claude/Claude.exe",
            "C:/Users/x/AppData/Local/Programs/OpenCode Beta/OpenCode Beta.exe",
        ]);
        assert_eq!(
            super::filenames_of(&paths),
            v(&["Claude.exe", "OpenCode Beta.exe"])
        );
        assert!(super::filenames_of(&[]).is_empty());
    }

    #[test]
    fn paths_config_accepts_explicit_process_names() {
        let config = paths_config_from(serde_json::json!({
            "name": "Wrapped Desktop",
            "category": "Desktop",
            "paths": { "linux": ["~/.local/bin/Wrapped.AppImage"] },
            "processNames": { "linux": ["wrapped-runtime"] }
        }));

        assert_eq!(
            config.process_names.unwrap().linux.unwrap(),
            v(&["wrapped-runtime"])
        );
    }

    // ── config-dir detection: a lingering config dir must not count as
    //    "installed" when a stronger detector exists and already failed.
    //    Regression: WorkBuddy showed installed after uninstall because
    //    ~/.workbuddy survived and was treated as proof of installation. ──

    #[test]
    fn install_hints_are_authoritative_over_config_dir() {
        // WorkBuddy shape: no platform paths (installs anywhere → registry is
        // the real detector), installHints present. A leftover config dir must
        // NOT override the registry's "uninstalled" verdict.
        let pc = paths_config_from(serde_json::json!({
            "name": "WorkBuddy",
            "category": "Agents",
            "detectByConfigDir": true,
            "configDir": "~/.workbuddy",
            "paths": { "win32": [], "darwin": [], "linux": [] },
            "installHints": { "windowsDisplayNamePrefixes": ["WorkBuddy"] }
        }));
        assert!(has_authoritative_detector(&pc));
    }

    #[test]
    fn platform_paths_are_authoritative_over_config_dir() {
        // Tools with real exe paths (e.g. hermes/openclaw) — paths are the
        // detector; a stale config dir alone never means installed.
        let pc = paths_config_from(serde_json::json!({
            "name": "Hermes",
            "category": "Agents",
            "detectByConfigDir": true,
            "configDir": "~/.hermes",
            "paths": {
                "win32": ["%USERPROFILE%/.hermes/bin/hermes.exe"],
                "darwin": ["~/.hermes/bin/hermes"],
                "linux": ["~/.hermes/bin/hermes"]
            }
        }));
        assert!(has_authoritative_detector(&pc));
    }

    #[test]
    fn config_only_tool_has_no_authoritative_detector() {
        // No paths and no installHints → the config dir is the only signal,
        // so it IS allowed to imply installation.
        let pc = paths_config_from(serde_json::json!({
            "name": "ConfigOnly",
            "category": "Agents",
            "detectByConfigDir": true,
            "configDir": "~/.configonly",
            "paths": { "win32": [], "darwin": [], "linux": [] }
        }));
        assert!(!has_authoritative_detector(&pc));
    }

    #[test]
    fn exact_name_matches() {
        let names = v(&["editor cn"]);
        assert!(registry_display_name_matches("editor cn", &names, &[]));
    }

    #[test]
    fn exact_name_does_not_substring_match() {
        // The whole reason exact match exists: a base name must not match a
        // regional edition.
        let names = v(&["editor"]);
        assert!(!registry_display_name_matches("editor cn", &names, &[]));
    }

    #[test]
    fn prefix_matches_versioned_display_name() {
        // WorkBuddy's registry DisplayName is "WorkBuddy 4.24.2".
        let prefixes = v(&["workbuddy"]);
        assert!(registry_display_name_matches(
            "workbuddy 4.24.2",
            &[],
            &prefixes
        ));
    }

    #[test]
    fn workbuddy_editions_do_not_match_each_other() {
        assert!(!registry_display_name_matches(
            "workbuddy ai 5.5.2",
            &[],
            &v(&["workbuddy"])
        ));
        assert!(!registry_display_name_matches(
            "workbuddy ai",
            &[],
            &v(&["workbuddy"])
        ));
        assert!(registry_display_name_matches(
            "workbuddy ai 5.5.2",
            &[],
            &v(&["workbuddy ai"])
        ));
        assert!(!registry_display_name_matches(
            "workbuddy 5.5.2",
            &[],
            &v(&["workbuddy ai"])
        ));
    }

    #[test]
    fn prefix_matches_bare_name_without_version() {
        let prefixes = v(&["workbuddy"]);
        assert!(registry_display_name_matches("workbuddy", &[], &prefixes));
    }

    #[test]
    fn prefix_requires_word_boundary_not_raw_substring() {
        // "workbuddy" prefix must NOT match "workbuddyextra" (no space) — the
        // boundary guard is what keeps prefix matching from over-firing.
        let prefixes = v(&["workbuddy"]);
        assert!(!registry_display_name_matches(
            "workbuddyextra 1.0",
            &[],
            &prefixes
        ));
    }

    #[test]
    fn no_hints_never_matches() {
        assert!(!registry_display_name_matches("anything 1.2.3", &[], &[]));
    }

    // ── exe-glob fallback: a registry Uninstall entry whose DisplayIcon is a
    //    standalone .ico and whose InstallLocation is blank (electron-builder
    //    NSIS at a custom path, e.g. ZCode at E:\ZCode) must still be detected
    //    by deriving the install dir from DisplayIcon's parent / UninstallString
    //    and globbing it for a name-matching exe. These test the pure-string
    //    pieces on every platform; the dir walk itself is Windows-only. ──

    #[test]
    fn parse_uninstall_string_quoted_with_args() {
        // ZCode's real value: `"E:\ZCode\Uninstall ZCode.exe" /currentuser`
        let got = parse_uninstall_exe_path(r#""E:\ZCode\Uninstall ZCode.exe" /currentuser"#);
        assert_eq!(got.as_deref(), Some(r"E:\ZCode\Uninstall ZCode.exe"));
    }

    #[test]
    fn parse_uninstall_string_bare() {
        let got = parse_uninstall_exe_path(r"C:\App\uninst.exe");
        assert_eq!(got.as_deref(), Some(r"C:\App\uninst.exe"));
    }

    #[test]
    fn parse_uninstall_string_rejects_winget_cli() {
        // winget uninstall strings have no .exe token — must not be treated as
        // an install dir (would otherwise resolve to a bogus path).
        let got = parse_uninstall_exe_path(
            "winget uninstall --product-code Anthropic.ClaudeCode_8wekyb3d8bbwe",
        );
        assert!(got.is_none());
    }

    #[test]
    fn parse_uninstall_string_rejects_empty() {
        assert!(parse_uninstall_exe_path("").is_none());
        assert!(parse_uninstall_exe_path("   ").is_none());
    }

    #[test]
    fn exe_stem_matches_exact_prefix() {
        // ZCode.exe stem "zcode" vs prefix "ZCode".
        let prefixes = v(&["zcode"]);
        assert!(exe_stem_matches_hints("zcode", &[], &prefixes));
    }

    #[test]
    fn exe_stem_matches_exact_name() {
        let names = v(&["editor cn"]);
        assert!(exe_stem_matches_hints("editor cn", &names, &[]));
    }

    #[test]
    fn exe_stem_prefix_matches_separator_suffix() {
        // "zcode-beta" matches prefix "zcode" (boundary after prefix).
        let prefixes = v(&["zcode"]);
        assert!(exe_stem_matches_hints("zcode-beta", &[], &prefixes));
        assert!(exe_stem_matches_hints("zcode beta", &[], &prefixes));
        assert!(exe_stem_matches_hints("zcode.beta", &[], &prefixes));
    }

    #[test]
    fn exe_stem_prefix_rejects_no_boundary() {
        // "zcodeextra" must NOT match prefix "zcode" — no word boundary.
        let prefixes = v(&["zcode"]);
        assert!(!exe_stem_matches_hints("zcodeextra", &[], &prefixes));
    }

    #[test]
    fn uninstaller_stem_is_flagged() {
        // "Uninstall ZCode.exe" stem must be excluded from the glob result.
        assert!(is_uninstaller_stem("uninstall zcode"));
        assert!(is_uninstaller_stem("unins000"));
        assert!(!is_uninstaller_stem("zcode"));
    }

    #[cfg(windows)]
    #[test]
    fn find_name_matching_exe_picks_app_not_uninstaller() {
        // Mimic a custom-path ZCode install dir: the real app exe sits next to
        // an `Uninstall ZCode.exe` uninstaller and a standalone .ico. The glob
        // must return the app exe and never the uninstaller.
        let dir = std::env::temp_dir().join(format!("echobird_zcode_test_{}", line!()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ZCode.exe"), b"").unwrap();
        std::fs::write(dir.join("Uninstall ZCode.exe"), b"").unwrap();
        std::fs::write(dir.join("uninstallerIcon.ico"), b"").unwrap();

        let prefixes = v(&["zcode"]);
        let got = super::find_name_matching_exe(&dir, &[], &prefixes);
        assert_eq!(
            got.as_deref().and_then(|p| {
                let p = std::path::Path::new(p);
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            }),
            Some("ZCode.exe".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: only passes where ZCode is installed at a custom path"]
    fn real_registry_finds_custom_path_zcode() {
        // End-to-end: the real ZCode Uninstall entry has DisplayIcon=.ico,
        // InstallLocation blank, UninstallString pointing at E:\ZCode. The scan
        // must derive E:\ZCode and return E:\ZCode\ZCode.exe (or whichever
        // name-matching exe lives there).
        use crate::models::tool::InstallHints;
        let hints = InstallHints {
            windows_display_name_prefixes: v(&["ZCode"]),
            ..Default::default()
        };
        let got = super::scan_windows_registry(&hints);
        let path = got.expect("ZCode registry entry should resolve to an exe");
        assert!(
            super::is_windows_exe(&path),
            "resolved path must be an exe: {path}"
        );
        assert!(
            path.to_lowercase().ends_with(r"\zcode.exe"),
            "expected ...\\ZCode.exe, got {path}"
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires DeepSeek Harness Desktop to be installed"]
    fn real_registry_finds_dsh_desktop() {
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/dsh/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("DeepSeek Harness registry entry should resolve to an executable");
        assert!(super::is_windows_exe(&path));
        assert!(
            path.to_lowercase().ends_with(r"\deepseek harness.exe"),
            "{path}"
        );
        assert!(std::path::Path::new(&path).is_file());
        println!("Detected DeepSeek Harness Desktop: {path}");
    }

    #[test]
    fn dsh_desktop_ignores_legacy_cli_overrides() {
        let mut definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/dsh/paths.json")).unwrap();
        let custom = "E:/Apps/DeepSeek Harness/DeepSeek Harness.exe".to_string();
        let legacy = vec![
            r"%APPDATA%\npm\dsh.cmd".to_string(),
            r"C:\custom\DSH.CMD".to_string(),
            "/usr/local/bin/dsh".to_string(),
            "~/.npm-global/bin/dsh".to_string(),
        ];
        let mut overrides = legacy.clone();
        overrides.push(custom.clone());
        super::apply_user_path_overrides(&mut definition, &overrides);
        let paths = super::get_platform_paths(&definition.paths);
        assert_eq!(paths.first(), Some(&custom));
        assert!(legacy.iter().all(|path| !paths.contains(path)));
        assert!(definition.command.is_empty());
        assert!(definition.start_command.is_none());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires Xiaomi MiMo Desktop to be installed"]
    fn real_registry_finds_mimodesktop() {
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/mimodesktop/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("Xiaomi MiMo registry entry should resolve to an executable");
        assert!(super::is_windows_exe(&path));
        assert!(path.to_lowercase().ends_with(r"\xiaomi mimo.exe"), "{path}");
        println!("Detected Xiaomi MiMo Desktop: {path}");
    }

    #[test]
    fn minimax_desktop_uses_one_localized_catalog_entry() {
        let definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/minimaxdesktop/paths.json")).unwrap();
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/tools/install/minimaxdesktop.json"
        ))
        .unwrap();
        assert_eq!(definition.name, "MiniMax Desktop");
        assert_eq!(definition.names.unwrap()["zh-Hans"], "MiniMax 桌面端");
        assert_eq!(reference["displayName"], "MiniMax Desktop");
        assert_eq!(reference["id"], "minimaxdesktop");
    }

    #[test]
    fn minimax_desktop_honors_path_priority_when_both_editions_exist() {
        let root =
            std::env::temp_dir().join(format!("echobird-minimax-paths-{}", uuid::Uuid::new_v4()));
        let mut definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/minimaxdesktop/paths.json")).unwrap();
        definition.install_hints = None;
        let mut paths = Vec::new();
        for (region, host) in [
            ("cn", "filecdn.minimax.chat"),
            ("en", "file.cdn.minimax.io"),
        ] {
            let dir = root.join(region);
            std::fs::create_dir_all(dir.join("resources")).unwrap();
            let exe = dir.join("MiniMax Code.exe");
            std::fs::write(&exe, []).unwrap();
            std::fs::write(
                dir.join("resources/app-update.yml"),
                format!("url: https://{host}/public/minimax-agent/release"),
            )
            .unwrap();
            paths.push(exe.to_string_lossy().into_owned());
        }
        for _ in 0..2 {
            definition.paths.win32 = Some(paths.clone());
            definition.paths.darwin = Some(paths.clone());
            definition.paths.linux = Some(paths.clone());
            assert_eq!(
                super::find_tool_executable(&definition, true),
                Some(paths[0].clone())
            );
            paths.reverse();
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires both MiniMax desktop editions installed"]
    fn real_minimax_desktop_detects_both_editions_with_path_overrides() {
        let mut definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/minimaxdesktop/paths.json")).unwrap();
        let global = super::expand_path(&definition.paths.win32.as_ref().unwrap()[0]);
        let domestic = std::path::PathBuf::from(r"E:\MiniMax Code\MiniMax Code.exe");
        for (exe, region) in [(global, "en"), (domestic, "cn")] {
            assert_eq!(
                super::super::tool_config_manager::minimaxcode::desktop_region(&exe),
                Some(region)
            );
            let path = exe.to_string_lossy().into_owned();
            super::apply_user_path_overrides(&mut definition, std::slice::from_ref(&path));
            assert_eq!(super::find_tool_executable(&definition, true), Some(path));
        }
    }

    #[test]
    fn minimax_desktop_resolves_custom_macos_bundle_paths() {
        let root =
            std::env::temp_dir().join(format!("echobird-minimax-bundle-{}", uuid::Uuid::new_v4()));
        let bundle = root.join("MiniMax Code.app");
        let exe = bundle.join("Contents/MacOS/MiniMax Code");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        std::fs::write(&exe, []).unwrap();
        std::fs::write(
            bundle.join("Contents/Resources/app-update.yml"),
            "url: https://file.cdn.minimax.io/public/minimax-agent/release",
        )
        .unwrap();
        let mut definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/minimaxdesktop/paths.json")).unwrap();
        definition.install_hints = None;
        let paths = vec![bundle.to_string_lossy().into_owned()];
        definition.paths.win32 = Some(paths.clone());
        definition.paths.darwin = Some(paths.clone());
        definition.paths.linux = Some(paths);
        assert_eq!(
            std::fs::canonicalize(super::find_tool_executable(&definition, true).unwrap()).unwrap(),
            std::fs::canonicalize(exe).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires Kimi Desktop to be installed"]
    fn real_registry_finds_kimidesktop() {
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/kimidesktop/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("Kimi Desktop registry entry should resolve to an executable");
        assert!(super::is_windows_exe(&path));
        assert!(path.to_lowercase().ends_with(r"\kimi code.exe"), "{path}");
        println!("Detected Kimi Desktop: {path}");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires Cline Desktop to be installed"]
    fn real_registry_finds_cline_desktop() {
        let definition: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/clinedesktop/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("Cline Desktop registry entry should resolve to its executable");
        assert!(super::is_windows_exe(&path));
        assert!(path.to_lowercase().ends_with(r"\cline-app.exe"), "{path}");
        println!("Detected Cline Desktop: {path}");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires OpenScience Desktop to be installed"]
    fn real_registry_finds_openscience_desktop() {
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/openscience/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("OpenScience registry entry should resolve to an executable");
        assert!(super::is_windows_exe(&path));
        assert!(path.to_lowercase().ends_with(r"\openscience.exe"), "{path}");
        println!("Detected OpenScience Desktop: {path}");
    }

    #[test]
    fn opendesign_catalog_separates_native_installers_from_linux_source_setup() {
        let definitions = super::load_tool_definitions_with_overrides(false);
        assert!(definitions.iter().any(|tool| tool.id == "codex"));
        let tool = definitions
            .iter()
            .find(|tool| tool.id == "opendesign")
            .expect("Open Design must be included by the bundled tool loader");
        assert!(!tool.paths_config.no_model_config);
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/opendesign/paths.json")).unwrap();
        assert_eq!(
            super::parse_category(&definition.category),
            crate::models::tool::ToolCategory::Creative
        );
        assert!(!definition.no_model_config);
        assert!(definition.paths.win32.unwrap()[0].ends_with("Open Design\\Open Design.exe"));
        assert!(definition.paths.darwin.unwrap()[0]
            .ends_with("Open Design.app/Contents/MacOS/Open Design"));
        assert_eq!(
            definition.paths.linux.unwrap(),
            ["~/.local/bin/open-design"]
        );
        let install: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/tools/install/opendesign.json"
        ))
        .unwrap();
        assert!(install["install"]["linux_download"]
            .as_str()
            .unwrap()
            .contains("no prebuilt"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "machine-specific: requires Open Design to be installed"]
    fn real_registry_finds_opendesign_desktop() {
        let definition: crate::models::tool::PathsConfig =
            serde_json::from_str(include_str!("../../../tools/opendesign/paths.json")).unwrap();
        let path = super::scan_windows_registry(&definition.install_hints.unwrap())
            .expect("Open Design registry entry should resolve to its executable");
        assert!(super::is_windows_exe(&path));
        assert!(path.to_lowercase().ends_with(r"\open design.exe"), "{path}");
    }

    // ── tool-paths.json self-heal: a file seeded before a tool shipped (e.g.
    //    Kimi Code in v5.4.3) must gain that tool's default-path entry on the
    //    next open, while existing user edits + "_" note keys survive. ──

    fn override_seed() -> Vec<(String, Vec<String>)> {
        vec![
            ("claudecode".to_string(), v(&["~/.claude/local/claude"])),
            ("kimicode".to_string(), v(&["~/.kimi-code/bin/kimi"])),
        ]
    }

    fn arr(items: &[&str]) -> serde_json::Value {
        serde_json::Value::Array(
            items
                .iter()
                .map(|s| serde_json::Value::String(s.to_string()))
                .collect(),
        )
    }

    #[test]
    fn missing_file_is_seeded_with_every_tool() {
        // No existing file → empty object → every seed tool inserted.
        let map = merge_override_seed(
            Some(&serde_json::Value::Object(serde_json::Map::new())),
            &override_seed(),
        )
        .expect("should write a full seed");
        assert!(map.contains_key("claudecode"));
        assert!(map.contains_key("kimicode"));
    }

    #[test]
    fn existing_file_gains_only_missing_tools() {
        // File predates kimicode: claudecode present (with a user edit) + a
        // "_" note, no kimi. Kimi must be added; the rest preserved verbatim.
        let mut existing = serde_json::Map::new();
        existing.insert("claudecode".to_string(), arr(&["/my/custom/claude"]));
        existing.insert(
            "_note".to_string(),
            serde_json::Value::String("hand-edited".to_string()),
        );

        let map = merge_override_seed(Some(&serde_json::Value::Object(existing)), &override_seed())
            .expect("should add the missing kimi entry");

        assert!(map.contains_key("kimicode"));
        // User's custom claudecode path NOT overwritten with the default.
        assert_eq!(map.get("claudecode"), Some(&arr(&["/my/custom/claude"])));
        // "_" note preserved (scanner ignores it; we must not drop it).
        assert!(map.contains_key("_note"));
    }

    #[test]
    fn up_to_date_file_is_left_untouched() {
        let mut existing = serde_json::Map::new();
        existing.insert("claudecode".to_string(), arr(&[]));
        existing.insert("kimicode".to_string(), arr(&[]));
        assert!(
            merge_override_seed(Some(&serde_json::Value::Object(existing)), &override_seed())
                .is_none()
        );
    }

    #[test]
    fn unreadable_or_non_object_file_is_left_untouched() {
        // No parseable object → don't risk clobbering the user's file.
        assert!(merge_override_seed(None, &override_seed()).is_none());
        assert!(
            merge_override_seed(Some(&serde_json::Value::Array(vec![])), &override_seed())
                .is_none()
        );
        assert!(merge_override_seed(
            Some(&serde_json::Value::String("oops".into())),
            &override_seed()
        )
        .is_none());
    }

    #[cfg(windows)]
    #[test]
    fn is_windows_exe_recognizes_exe_and_rejects_non_executables() {
        // Exe — case-insensitive on the extension.
        assert!(is_windows_exe("C:\\Program Files\\ZCode\\ZCode.exe"));
        assert!(is_windows_exe("C:\\Program Files\\ZCode\\ZCode.EXE"));
        // Non-executables the registry can serve as DisplayIcon — the exact
        // regression we guard against (Start-Process would open these with
        // the default image viewer instead of launching the app).
        assert!(!is_windows_exe("C:\\Program Files\\ZCode\\zcode.ico"));
        assert!(!is_windows_exe("C:\\Program Files\\ZCode\\resources.dll"));
        // No extension at all.
        assert!(!is_windows_exe("C:\\Program Files\\ZCode\\zcode"));
    }

    #[test]
    fn linux_desktop_exec_preserves_spaces_and_ignores_field_codes() {
        use super::desktop_exec_program;
        assert_eq!(
            desktop_exec_program(r#""/opt/Grok Bot/grok-bot" %U"#).as_deref(),
            Some("/opt/Grok Bot/grok-bot")
        );
        assert_eq!(
            desktop_exec_program("cursor --no-sandbox %F").as_deref(),
            Some("cursor")
        );
        assert_eq!(
            desktop_exec_program(r"/opt/Grok\ Bot/grok-bot %U").as_deref(),
            Some("/opt/Grok Bot/grok-bot")
        );
        assert!(desktop_exec_program("\"unterminated").is_none());
        assert!(desktop_exec_program(" ").is_none());
    }

    #[test]
    fn devin_catalog_identifies_the_gui_and_platform_bundled_cli() {
        let desktop: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/devin/paths.json")).unwrap();
        let cli: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/devincli/paths.json")).unwrap();
        assert_eq!(cli.start_command.as_deref(), Some("devin"));
        let root =
            std::env::temp_dir().join(format!("echobird-devin-detection-{}", uuid::Uuid::new_v4()));
        for (platform, path, file) in [
            ("windows", "windows/Devin.exe", "devin.exe"),
            ("macos", "macos/Devin.app/Contents/MacOS/Electron", "devin"),
            ("linux", "linux/devin", "devin"),
        ] {
            let exe = root.join(path);
            let resources = super::devin_resources(&exe).unwrap();
            fs::create_dir_all(resources.join("app")).unwrap();
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::write(&exe, []).unwrap();
            assert!(!super::valid_executable(&desktop, &exe));
            fs::write(
                resources.join("app/product.json"),
                r#"{"nameShort":"Devin","applicationName":"devin-desktop"}"#,
            )
            .unwrap();
            assert!(super::valid_executable(&desktop, &exe));
            assert!(!super::valid_executable(&cli, &exe));
            let bundled = super::devin_bundled_cli(&exe, platform).unwrap();
            assert_eq!(bundled.file_name().unwrap(), file);
            fs::create_dir_all(bundled.parent().unwrap()).unwrap();
            fs::write(&bundled, []).unwrap();
            assert!(super::valid_executable(&cli, &bundled));
            assert!(!super::valid_executable(&desktop, &bundled));
        }
        let index: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/api/tools/install/index.json"))
                .unwrap();
        for id in ["devin", "devincli"] {
            assert!(index["ids"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!(id)));
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cursor_cli_catalog_and_detection_never_reuse_the_grok_agent_alias() {
        let cli: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/cursorcli/paths.json")).unwrap();
        assert_eq!(cli.command, "cursor-agent");
        assert_eq!(cli.start_command.as_deref(), Some("cursor-agent"));
        assert!(cli.no_model_config);
        let root = std::env::temp_dir().join(format!(
            "echobird-cursor-detection-{}",
            uuid::Uuid::new_v4()
        ));
        for name in [
            "cursor-agent.cmd",
            "cursor-agent",
            "agent.exe",
            "Cursor.exe",
            "grok.exe",
        ] {
            let path = root.join(name);
            fs::create_dir_all(&root).unwrap();
            fs::write(&path, b"fixture").unwrap();
            assert_eq!(
                valid_executable(&cli, &path),
                matches!(name, "cursor-agent.cmd" | "cursor-agent")
            );
        }
        let wrong = root.join(".grok/bin/cursor-agent.cmd");
        assert!(!valid_executable(
            &cli,
            &root.join("missing/cursor-agent.cmd")
        ));
        fs::create_dir_all(wrong.parent().unwrap()).unwrap();
        fs::write(&wrong, b"wrong product").unwrap();
        assert!(!valid_executable(&cli, &wrong));
        let install: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/tools/install/cursorcli.json"
        ))
        .unwrap();
        assert_eq!(install["id"], "cursorcli");
        let index: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/api/tools/install/index.json"))
                .unwrap();
        assert!(index["ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "cursorcli"));
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    use super::{find_tool_executable, qwenwork_versioned_executable};
    use super::{packaged_desktop_metadata, qoder_resources, valid_executable};
    use std::path::{Path, PathBuf};

    fn write_qwenwork_package(resources: &Path, name: &str, version: &str) {
        let package =
            serde_json::to_vec(&serde_json::json!({"name":name,"version":version})).unwrap();
        let tree = serde_json::to_vec(
            &serde_json::json!({"files":{"package.json":{"offset":"0","size":package.len()}}}),
        )
        .unwrap();
        let header_size = (tree.len() + 8 + 3) & !3;
        let mut archive = vec![0u8; 8 + header_size];
        archive[0..4].copy_from_slice(&4u32.to_le_bytes());
        archive[4..8].copy_from_slice(&(header_size as u32).to_le_bytes());
        archive[8..12].copy_from_slice(&((header_size - 4) as u32).to_le_bytes());
        archive[12..16].copy_from_slice(&(tree.len() as u32).to_le_bytes());
        archive[16..16 + tree.len()].copy_from_slice(&tree);
        archive.extend(package);
        fs::create_dir_all(resources).unwrap();
        fs::write(resources.join("app.asar"), archive).unwrap();
    }

    #[test]
    fn qwenwork_catalog_uses_verified_platform_paths_and_excludes_custom_models() {
        for (json, name, package) in [
            (
                include_str!("../../../tools/qwenwork/paths.json"),
                "QwenWork",
                "qwen-work",
            ),
            (
                include_str!("../../../tools/qwenworkcn/paths.json"),
                "千问办公",
                "qwen-work-cn",
            ),
        ] {
            let pc: PathsConfig = serde_json::from_str(json).unwrap();
            assert_eq!(pc.name, name);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(json).unwrap()["noModelConfig"],
                true
            );
            let root =
                std::env::temp_dir().join(format!("qwenwork-detection-{}", uuid::Uuid::new_v4()));
            let executable = if cfg!(target_os = "macos") {
                root.join("Contents/MacOS/Client")
            } else {
                root.join("Client.exe")
            };
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(&executable, b"fixture").unwrap();
            let resources = qoder_resources(std::env::consts::OS, &executable).unwrap();
            write_qwenwork_package(&resources, package, "2.10.0");
            assert!(valid_executable(&pc, &executable));
            write_qwenwork_package(
                &resources,
                if package == "qwen-work" {
                    "qwen-work-cn"
                } else {
                    "qwen-work"
                },
                "2.10.0",
            );
            assert!(!valid_executable(&pc, &executable));
            fs::remove_dir_all(root).unwrap();
        }
        let cn: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/qwenworkcn/paths.json")).unwrap();
        assert_eq!(cn["paths"]["linux"][0], "/opt/apps/QwenWorkCN/qwen-work-cn");
        let global: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/qwenwork/paths.json")).unwrap();
        assert!(global["paths"]["linux"].as_array().unwrap().is_empty());
    }

    #[test]
    fn dumate_detection_requires_the_official_package_and_has_no_linux_install_claim() {
        let pc: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/dumate/paths.json")).unwrap();
        let root = std::env::temp_dir().join(format!("dumate-detection-{}", uuid::Uuid::new_v4()));
        let executable = if cfg!(target_os = "macos") {
            root.join("Contents/MacOS/DuMate")
        } else {
            root.join("DuMate.exe")
        };
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(&executable, b"fixture").unwrap();
        let resources = qoder_resources(std::env::consts::OS, &executable).unwrap();
        write_qwenwork_package(&resources, "other-product", "1.0.86");
        assert!(!valid_executable(&pc, &executable));
        write_qwenwork_package(&resources, "qianfan-desktop-app", "1.0.86");
        assert!(valid_executable(&pc, &executable));
        let uninstaller = executable.with_file_name("Uninstall DuMate.exe");
        fs::write(&uninstaller, b"fixture").unwrap();
        assert!(!valid_executable(&pc, &uninstaller));
        assert_eq!(
            packaged_desktop_metadata(&executable).unwrap()["version"],
            "1.0.86"
        );
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/dumate/paths.json")).unwrap();
        assert_eq!(catalog["noModelConfig"], true);
        assert!(!super::is_managed_desktop_tool("dumate"));
        assert_eq!(catalog["paths"]["linux"], serde_json::json!([]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Read-only detection of the installed official DuMate client"]
    fn installed_dumate_detection_probe() {
        let pc: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/dumate/paths.json")).unwrap();
        let path = super::find_tool_executable(&pc, true).expect("installed DuMate");
        assert!(valid_executable(&pc, Path::new(&path)));
        println!(
            "DuMate {} {}",
            packaged_desktop_metadata(Path::new(&path)).unwrap()["version"],
            path
        );
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Read-only detection of the installed Doubao Work and Dola clients"]
    fn installed_doubao_clients_resolve_the_chromium_runtime() {
        for (definition, executable, product) in [
            (
                include_str!("../../../tools/doubaowork/paths.json"),
                "DoubaoWork.exe",
                "DoubaoWork",
            ),
            (
                include_str!("../../../tools/dola/paths.json"),
                "Cici.exe",
                "Dola",
            ),
        ] {
            let pc: PathsConfig = serde_json::from_str(definition).unwrap();
            let detected = super::find_tool_executable(&pc, true).expect("installed client");
            let path = Path::new(&detected);
            assert_eq!(path.file_name().unwrap(), executable);
            assert_eq!(path.parent().unwrap().file_name().unwrap(), "app");
            assert!(valid_executable(&pc, path));
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(path.with_file_name("manifest.json")).unwrap())
                    .unwrap();
            assert_eq!(manifest["productName"], product);
            println!("{product}: {} at {detected}", manifest["version"]);
        }
    }

    #[cfg(windows)]
    #[test]
    fn qwenwork_versioned_detection_ignores_launchers_and_resolves_the_latest_matching_edition() {
        let root = std::env::temp_dir().join(format!("qwenwork-versions-{}", uuid::Uuid::new_v4()));
        let pc: PathsConfig =
            serde_json::from_str(include_str!("../../../tools/qwenwork/paths.json")).unwrap();
        for (version, package) in [
            ("2.9.0-1", "qwen-work"),
            ("2.10.0-2", "qwen-work"),
            ("3.0.0-1", "qwen-work-cn"),
        ] {
            let dir = root.join(version);
            write_qwenwork_package(&dir.join("resources"), package, version);
            fs::write(dir.join("QwenWork.exe"), b"fixture").unwrap();
        }
        fs::write(root.join("Launcher.exe"), b"launcher").unwrap();
        assert_eq!(
            qwenwork_versioned_executable(&pc, &root).unwrap(),
            root.join("2.10.0-2/QwenWork.exe")
        );
        fs::write(root.join("updater.cfg"), br#"{"currentVersion":"2.9.0-1"}"#).unwrap();
        assert_eq!(
            qwenwork_versioned_executable(&pc, &root).unwrap(),
            root.join("2.9.0-1/QwenWork.exe")
        );
        fs::write(
            root.join("updater.cfg"),
            br#"{"currentVersion":"../outside"}"#,
        )
        .unwrap();
        assert!(qwenwork_versioned_executable(&pc, &root).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Requires explicit paths to the installed QwenWork desktop clients; metadata only"]
    fn installed_qwenwork_detection_probe() {
        for (key, json) in [
            (
                "ECHOBIRD_QWENWORK_EXE",
                include_str!("../../../tools/qwenwork/paths.json"),
            ),
            (
                "ECHOBIRD_QWENWORKCN_EXE",
                include_str!("../../../tools/qwenworkcn/paths.json"),
            ),
        ] {
            let executable = PathBuf::from(std::env::var(key).unwrap());
            let pc: PathsConfig = serde_json::from_str(json).unwrap();
            assert!(valid_executable(&pc, &executable));
            let metadata = packaged_desktop_metadata(&executable).unwrap();
            println!(
                "{} {} {}",
                pc.name,
                metadata["version"].as_str().unwrap(),
                executable.display()
            );
            #[cfg(windows)]
            assert_eq!(
                find_tool_executable(&pc, true).as_deref(),
                Some(executable.to_string_lossy().as_ref())
            );
        }
    }
}
