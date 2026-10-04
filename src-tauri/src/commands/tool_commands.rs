// Tauri Commands for tool operations — exposed to frontend via invoke()

use crate::models::tool::DetectedTool;
use crate::services::antigravity_accounts::{
    self, Account as AntigravityAccount, LoginStart as AntigravityLogin,
};
use crate::services::codex_accounts::{self, CodexAccountSummary};
use crate::services::tool_config_manager::{self, ApplyResult, ModelInfo};
use crate::services::tool_manager;

#[tauri::command]
pub async fn list_antigravity_accounts() -> Result<Vec<AntigravityAccount>, String> {
    antigravity_accounts::list().await
}

#[tauri::command]
pub async fn start_antigravity_login() -> Result<AntigravityLogin, String> {
    antigravity_accounts::start_login().await
}

#[tauri::command]
pub async fn poll_antigravity_login(
    login_id: String,
) -> Result<Option<AntigravityAccount>, String> {
    antigravity_accounts::poll_login(&login_id).await
}

#[tauri::command]
pub fn cancel_antigravity_login(login_id: String) -> Result<(), String> {
    antigravity_accounts::cancel_login(&login_id)
}

#[tauri::command]
pub async fn switch_antigravity_account(account_id: String) -> Result<AntigravityAccount, String> {
    antigravity_accounts::switch(&account_id).await
}

#[tauri::command]
pub async fn delete_antigravity_account(account_id: String) -> Result<(), String> {
    antigravity_accounts::delete(&account_id).await
}

#[tauri::command]
pub async fn refresh_antigravity_account(account_id: String) -> Result<AntigravityAccount, String> {
    antigravity_accounts::refresh(&account_id).await
}

use crate::services::grok_bot_accounts::{
    self, Account as GrokBotAccount, LoginStart as GrokBotLogin,
};

#[tauri::command]
pub async fn list_grok_bot_accounts() -> Result<Vec<GrokBotAccount>, String> {
    grok_bot_accounts::list().await
}
#[tauri::command]
pub fn start_grok_bot_login() -> Result<GrokBotLogin, String> {
    grok_bot_accounts::start_login()
}
#[tauri::command]
pub async fn poll_grok_bot_login(login_id: String) -> Result<Option<GrokBotAccount>, String> {
    grok_bot_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_grok_bot_login(login_id: String) -> Result<(), String> {
    grok_bot_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_grok_bot_account(account_id: String) -> Result<GrokBotAccount, String> {
    grok_bot_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn delete_grok_bot_account(account_id: String) -> Result<(), String> {
    grok_bot_accounts::delete(&account_id).await
}
#[tauri::command]
pub async fn refresh_grok_bot_account(
    account_id: String,
) -> Result<crate::services::cursor_usage::Usage, String> {
    grok_bot_accounts::refresh(&account_id).await
}

use crate::services::cursor_accounts::{self, Account as CursorAccount, LoginStart as CursorLogin};

#[tauri::command]
pub async fn list_cursor_accounts() -> Result<Vec<CursorAccount>, String> {
    cursor_accounts::list().await
}
#[tauri::command]
pub fn start_cursor_login() -> Result<CursorLogin, String> {
    cursor_accounts::start_login()
}
#[tauri::command]
pub async fn poll_cursor_login(login_id: String) -> Result<Option<CursorAccount>, String> {
    cursor_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_cursor_login(login_id: String) -> Result<(), String> {
    cursor_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_cursor_account(account_id: String) -> Result<CursorAccount, String> {
    cursor_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn delete_cursor_account(account_id: String) -> Result<(), String> {
    cursor_accounts::delete(&account_id).await
}
#[tauri::command]
pub async fn refresh_cursor_account(
    account_id: String,
) -> Result<crate::services::cursor_usage::Usage, String> {
    cursor_accounts::refresh(&account_id).await
}

/// Copy a built-in tool's bundle to the user's ~/.echobird/<id>/ directory so
/// the "我的AI项目" page can present real, navigable, editable reference
/// files (Reversi / Translator). This is a one-way dead copy — editing or
/// deleting any file here has **no effect** on EchoBird's actual launch of
/// that tool, which always uses the original bundle inside the app. Idempotent:
/// files the user already has (kept previous copy, hand-edited) are NOT
/// overwritten on subsequent seeds.
///
/// Returns the destination directory's absolute path so the frontend can
/// build the per-file paths it stores in the project record.
#[tauri::command]
pub async fn seed_builtin_to_user_dir(tool_id: String) -> Result<String, String> {
    use std::fs;

    // Source: <resource>/tools/<id>/  — game.html + <id>.svg are shipped in
    // tools/<id>/ alongside paths.json + models.json (duplicated from
    // public/ at build time so the user reference copy and the in-app
    // WebView launcher share the same byte-for-byte content).
    let tools_dir = tool_manager::find_tools_dir()
        .ok_or_else(|| "Could not resolve bundled tools directory".to_string())?;
    let src_dir = tools_dir.join(&tool_id);
    if !src_dir.exists() {
        return Err(format!(
            "Bundled tool '{}' not found at {:?}",
            tool_id, src_dir
        ));
    }

    // Destination: ~/.echobird/<id>/
    let home = dirs::home_dir().ok_or_else(|| "Could not resolve home dir".to_string())?;
    let dst_dir = home.join(".echobird").join(&tool_id);
    fs::create_dir_all(&dst_dir).map_err(|e| format!("Failed to create {:?}: {}", dst_dir, e))?;

    // Copy the runtime files that users can study. The seeded directory
    // mirrors exactly what an end-user authoring their own AI project needs
    // to prepare — launcher + icon + models.json (generated below). We do
    // NOT copy:
    //   • bundle's models.json (internal write-map schema; we write a
    //     clean 4-field template instead — see below)
    //   • bundle's paths.json (EchoBird-internal metadata: launchType,
    //     configDir, apiProtocol, etc. — none of which user-authored
    //     projects need; user projects register via the "我的AI项目"
    //     dialog and metadata lives in localStorage, not a file)
    let icon_name = format!("{}.svg", tool_id);
    let files: [&str; 2] = ["game.html", &icon_name];

    for name in files {
        let src = src_dir.join(name);
        let dst = dst_dir.join(name);
        if dst.exists() {
            continue; // user may have edited / kept the previous copy
        }
        if !src.exists() {
            log::warn!("[Seed] Source missing: {:?}", src);
            continue;
        }
        fs::copy(&src, &dst).map_err(|e| format!("Copy {:?} -> {:?} failed: {}", src, dst, e))?;
    }

    // models.json — flat 4-field template the user can copy into their own
    // AI project. After registering that project under "我的AI项目",
    // clicking 仅修改 / 启动 writes the chosen model's 4 fields into
    // their models.json at these exact keys. Idempotent: don't clobber an
    // existing copy.
    let models_template = dst_dir.join("models.json");
    if !models_template.exists() {
        let content = "{\n  \"modelId\": \"\",\n  \"baseUrl\": \"\",\n  \"anthropicUrl\": \"\",\n  \"apiKey\": \"\"\n}\n";
        fs::write(&models_template, content)
            .map_err(|e| format!("Failed to write models.json template: {}", e))?;
    }

    // README.txt — generated, English only (intentionally — keeping i18n debt
    // off a static file dropped into the user's home).
    let readme = dst_dir.join("README.txt");
    if !readme.exists() {
        let content = format!(
            "EchoBird built-in tool: {tool_id}\n\n\
This folder is a personal reference copy. Edit, delete, or copy it freely —\n\
none of these changes affect EchoBird's actual launch of {tool_id}, which\n\
always uses the original bundle inside the EchoBird application itself.\n\n\
The 3 files map 1:1 to the 3 file fields you fill in when registering\n\
your own AI project under 'My AI Projects' in EchoBird:\n\
  - game.html        Launcher — your app/game entry point. This file shows\n\
                     how to call an LLM, read the model config EchoBird\n\
                     injects via window.__MODEL_CONFIG__, and render the\n\
                     experience. Replace with your own .html / .exe / .bat\n\
                     / whatever you launch.\n\
  - {tool_id}.svg    Icon — any .ico / .svg / .png works.\n\
  - models.json      4-field template (modelId / baseUrl / anthropicUrl /\n\
                     apiKey). Copy this into your own AI project: when you\n\
                     register the project under 'My AI Projects', clicking\n\
                     Apply writes the chosen model's 4 fields into your\n\
                     models.json at these exact keys.\n\n\
Use this folder as a starting point: copy the 3 files into your own\n\
project, edit game.html + models.json to taste, then register the new\n\
paths under 'My AI Projects' in EchoBird.\n",
            tool_id = tool_id
        );
        fs::write(&readme, content).map_err(|e| format!("Failed to write README: {}", e))?;
    }

    Ok(dst_dir.to_string_lossy().to_string())
}

/// Scan all installed tools and return detection results
#[tauri::command]
pub async fn scan_tools() -> Result<Vec<DetectedTool>, String> {
    Ok(tool_manager::scan_tools().await)
}

/// Apply a model configuration to a tool
#[tauri::command]
pub async fn apply_model_to_tool(
    tool_id: String,
    model_info: ModelInfo,
) -> Result<ApplyResult, String> {
    // Decrypt API key if it's encrypted (frontend stores encrypted keys)
    let mut info = model_info;
    if let Some(ref encrypted_key) = info.api_key {
        let decrypted = crate::services::model_manager::decrypt_key_for_use(encrypted_key);
        if !decrypted.is_empty() {
            info.api_key = Some(decrypted);
        }
    }
    Ok(tool_config_manager::apply_model_to_tool(&tool_id, info).await)
}

/// Restore a tool to its official defaults by deleting its config file. The
/// tool will regenerate a default config (pointing at its vendor endpoint) on
/// next launch.
#[tauri::command]
pub async fn restore_tool_to_official(tool_id: String) -> Result<ApplyResult, String> {
    Ok(tool_config_manager::restore_tool_to_official(&tool_id).await)
}

#[tauri::command]
pub fn list_codex_accounts() -> Result<Vec<CodexAccountSummary>, String> {
    codex_accounts::list_accounts()
}

#[tauri::command]
pub fn capture_current_codex_account() -> Result<CodexAccountSummary, String> {
    codex_accounts::capture_current_account()
}

#[tauri::command]
pub fn start_codex_login() -> Result<String, String> {
    codex_accounts::start_login()
}

#[tauri::command]
pub fn cancel_codex_login(login_id: String) -> Result<(), String> {
    codex_accounts::cancel_login(&login_id)
}

#[tauri::command]
pub async fn add_codex_account_via_oauth(
    app_handle: tauri::AppHandle,
    login_id: String,
    callback_messages: codex_accounts::OAuthCallbackMessages,
) -> Result<CodexAccountSummary, String> {
    codex_accounts::add_account_via_oauth(app_handle, login_id, callback_messages).await
}

#[tauri::command]
pub fn switch_codex_account(account_id: String) -> Result<CodexAccountSummary, String> {
    codex_accounts::switch_account(&account_id)
}

#[tauri::command]
pub async fn refresh_codex_account_quota(
    account_id: String,
) -> Result<CodexAccountSummary, String> {
    codex_accounts::refresh_account_quota(&account_id).await
}

#[tauri::command]
pub fn delete_codex_account(account_id: String) -> Result<(), String> {
    codex_accounts::delete_account(&account_id)
}

/// Apply a model config to a user-authored project's models.json.
/// Writes the 4 canonical ModelInfo fields — `modelId`, `baseUrl`,
/// `anthropicUrl`, `apiKey` — at the top level of the file. Existing
/// top-level keys are preserved (merge, not overwrite). Silent on every
/// failure: these are user-authored projects, broken paths / write
/// permission errors are the user's problem to debug. Caller ignores Err.
#[tauri::command]
pub async fn apply_user_project_model(
    models_json_path: String,
    model_info: ModelInfo,
) -> Result<(), String> {
    use serde_json::Value;
    use std::fs;
    use std::path::Path;

    let models_path = Path::new(&models_json_path);

    let mut info = model_info;
    if let Some(ref encrypted_key) = info.api_key {
        let decrypted = crate::services::model_manager::decrypt_key_for_use(encrypted_key);
        if !decrypted.is_empty() {
            info.api_key = Some(decrypted);
        }
    }

    let mut config_root: Value = match fs::read_to_string(models_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or(Value::Object(Default::default())),
        Err(_) => Value::Object(Default::default()),
    };
    if !config_root.is_object() {
        config_root = Value::Object(Default::default());
    }
    let obj = config_root.as_object_mut().unwrap();

    // Always write all 4 keys — mirror the chosen model exactly, including
    // empty values. None / undefined becomes "". No filtering, no
    // synthesizing. Other top-level keys in the user's file are left
    // untouched (we own these 4 keys; everything else is the user's).
    obj.insert(
        "modelId".to_string(),
        Value::String(info.model.clone().unwrap_or_default()),
    );
    obj.insert(
        "baseUrl".to_string(),
        Value::String(info.base_url.clone().unwrap_or_default()),
    );
    obj.insert(
        "anthropicUrl".to_string(),
        Value::String(info.anthropic_url.clone().unwrap_or_default()),
    );
    obj.insert(
        "apiKey".to_string(),
        Value::String(info.api_key.clone().unwrap_or_default()),
    );

    if let Some(parent) = models_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let serialized = serde_json::to_string_pretty(&config_root)
        .map_err(|e| format!("Failed to serialise config: {e}"))?;
    fs::write(models_path, serialized)
        .map_err(|e| format!("Failed to write {models_json_path}: {e}"))?;

    Ok(())
}

/// Launch a user-authored project's launcher via the OS default handler.
/// HTML → system browser, .exe / .bat → executed, CLI scripts → whatever
/// the user's OS associates. We don't second-guess: hand the path to the
/// shell and let the OS dispatch. Silent on failure.
#[tauri::command]
pub async fn launch_user_project(launcher_path: String) -> Result<(), String> {
    log::info!("[LaunchUserProject] Opening: {}", launcher_path);

    // Match open_folder's pattern: spawn the OS-native dispatcher per
    // platform. `explorer <path>` on Windows triggers the file's
    // default association the same way double-clicking would (folders
    // open in a new window; .exe / .bat / .html / .py / etc. go to
    // whatever the user has associated). `open` and `xdg-open` are the
    // macOS / Linux equivalents. Avoids tauri-plugin-shell's deprecated
    // .open(), and stays consistent with how we open folders.
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&launcher_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open {launcher_path}: {e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&launcher_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open {launcher_path}: {e}"))
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&launcher_path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Failed to open {launcher_path}: {e}"))
    }
    #[cfg(target_os = "android")]
    {
        let _ = launcher_path;
        Err("Not supported on Android".to_string())
    }
}

/// Launch a built-in tool (game/utility) in a new WebView window
#[tauri::command]
pub async fn launch_game(
    app_handle: tauri::AppHandle,
    tool_id: String,
    _launch_file: String,
    model_config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    #[cfg(not(target_os = "android"))]
    {
        use tauri::Manager;

        let window_label = format!("tool-{}", tool_id);

        // If window already exists, just focus it
        if let Some(existing) = app_handle.get_webview_window(&window_label) {
            let _ = existing.show();
            let _ = existing.set_focus();
            return Ok(serde_json::json!({ "success": true }));
        }

        // Determine window size based on tool
        let (width, height, title) = match tool_id.as_str() {
            "reversi" => (860.0, 680.0, "Reversi"),
            "translator" => (800.0, 560.0, "AI Translate"),
            _ => (800.0, 600.0, "Tool"),
        };

        let app_path = format!("tools/{}.html", tool_id);

        let init_script = {
            // Read current app locale from settings (falls back to empty = browser default)
            let locale = crate::commands::settings_commands::get_settings()
                .locale
                .unwrap_or_default();

            let mut script = format!("window.__APP_LOCALE__ = {:?};", locale);

            if let Some(mut config) = model_config {
                // Decrypt API key if it's encrypted (frontend stores encrypted keys as enc:v1:...)
                // Without this, the tool window receives an encrypted key and gets 401 from all APIs.
                if let Some(api_key_val) = config.get("apiKey").and_then(|v| v.as_str()) {
                    let decrypted =
                        crate::services::model_manager::decrypt_key_for_use(api_key_val);
                    if !decrypted.is_empty() {
                        config["apiKey"] = serde_json::Value::String(decrypted);
                    }
                }
                script.push_str(&format!("\nwindow.__MODEL_CONFIG__ = {};", config));
            }
            script
        };

        let mut builder = tauri::WebviewWindowBuilder::new(
            &app_handle,
            &window_label,
            tauri::WebviewUrl::App(app_path.into()),
        )
        .title(title)
        .inner_size(width, height)
        .resizable(true)
        .decorations(false)
        .center();

        if !init_script.is_empty() {
            builder = builder.initialization_script(&init_script);
        }

        let _window = builder
            .build()
            .map_err(|e| format!("Failed to create window: {}", e))?;

        log::info!("[ToolCommands] Launched {} in new window", tool_id);
        Ok(serde_json::json!({ "success": true }))
    }
    #[cfg(target_os = "android")]
    {
        let _ = (app_handle, tool_id, _launch_file, model_config);
        Err("Not available on mobile".to_string())
    }
}

/// Open a folder in the system file manager
#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    // Canonicalize to get proper OS path separators (backslashes on Windows)
    let canonical = p
        .canonicalize()
        .map_err(|e| format!("Failed to resolve path: {}", e))?;
    let resolved = canonical.to_string_lossy().to_string();
    // Strip Windows UNC prefix \\?\ that canonicalize adds
    #[cfg(target_os = "windows")]
    let resolved = resolved
        .strip_prefix(r"\\?\")
        .unwrap_or(&resolved)
        .to_string();

    log::info!("[OpenFolder] Opening: {}", resolved);

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "android")]
    {
        let _ = path;
        return Err("Not available on mobile".to_string());
    }

    #[cfg(not(target_os = "android"))]
    Ok(())
}

/// Open the user's tool-path overrides file (`~/.echobird/tool-paths.json`),
/// seeding it from a template on first use AND self-healing it on later
/// opens — any tool shipped in a later release (e.g. Kimi Code in v5.4.3)
/// whose entry the file predates is added with its default paths. Lets a user
/// add the install path for a tool EchoBird's bundled defaults missed
/// (installed in a non-default directory) WITHOUT editing the bundled
/// `tools/<id>/paths.json` — those are app resources that every update
/// overwrites. This file lives in the user data dir, so edits survive
/// updates; the scanner merges it on top of the built-in candidate paths
/// (see tool_manager::load_user_path_overrides), and deleting the file
/// restores pure defaults. Returns the file's absolute path.
#[tauri::command]
pub async fn open_tool_paths_config() -> Result<String, String> {
    use std::fs;

    let home = dirs::home_dir().ok_or_else(|| "Could not resolve home dir".to_string())?;
    let dir = home.join(".echobird");
    fs::create_dir_all(&dir).map_err(|e| format!("Failed to create {:?}: {}", dir, e))?;
    let file = dir.join("tool-paths.json");

    // (Re)write the override file when it's missing OR when an existing file
    // predates a tool shipped in a later release (e.g. Kimi Code in v5.4.3).
    // The file is pure data — "<tool id>": ["path", ...], current OS only
    // (see tool_manager::default_override_seed) — so the user just edits the
    // one that's wrong instead of authoring a line from scratch, and zero
    // prose means nothing to mistranslate or misread. We only ever ADD
    // missing tool ids: existing entries, user edits, and "_"-prefixed note
    // keys (ignored by the scanner — see load_user_path_overrides) are
    // preserved verbatim, so a user's customizations survive an update that
    // ships new tools. If the existing file isn't a readable JSON object we
    // leave it untouched rather than risk clobbering the user's edits.
    let created_fresh = !file.exists();

    // Empty object when the file is missing → forces a full seed (every tool
    // is "missing", so all are inserted). For an existing file, parse it and
    // keep it only if it's a JSON object; anything else falls through to
    // `None` → no rewrite.
    let parsed = if created_fresh {
        Some(serde_json::Value::Object(serde_json::Map::new()))
    } else {
        fs::read_to_string(&file)
            .ok()
            .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .filter(|v| matches!(v, serde_json::Value::Object(_)))
    };

    let seed = tool_manager::default_override_seed();

    if let Some(map) = tool_manager::merge_override_seed(parsed.as_ref(), &seed) {
        let content = serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .map_err(|e| format!("Failed to build override file: {}", e))?;
        fs::write(&file, format!("{content}\n"))
            .map_err(|e| format!("Failed to write override file: {}", e))?;
        if created_fresh {
            log::info!("[ToolPaths] Seeded pre-filled override file at {:?}", file);
        } else {
            log::info!("[ToolPaths] Added missing tool entries to {:?}", file);
        }
    }

    let resolved = file.to_string_lossy().to_string();
    log::info!("[ToolPaths] Opening override file: {}", resolved);

    // Open the FILE with the OS default handler (editor / Notepad). Same
    // pattern as launch_user_project: `explorer <file>` dispatches the file's
    // default association exactly like a double-click.
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| format!("Failed to open file: {}", e))?;
    }
    #[cfg(target_os = "android")]
    {
        return Err("Not available on mobile".to_string());
    }

    #[cfg(not(target_os = "android"))]
    Ok(resolved)
}

#[tauri::command]
pub async fn list_claude_code_accounts(
) -> Result<Vec<crate::services::claude_code_accounts::ClaudeCodeAccount>, String> {
    crate::services::claude_code_accounts::list().await
}

#[tauri::command]
pub fn start_claude_code_login() -> Result<crate::services::claude_code_oauth::LoginStart, String> {
    crate::services::claude_code_oauth::start()
}

#[tauri::command]
pub async fn complete_claude_code_login(
    login_id: String,
    code: String,
) -> Result<crate::services::claude_code_accounts::ClaudeCodeAccount, String> {
    crate::services::claude_code_oauth::complete(&login_id, &code).await
}

#[tauri::command]
pub fn cancel_claude_code_login(login_id: String) -> Result<(), String> {
    crate::services::claude_code_oauth::cancel(&login_id)
}

#[tauri::command]
pub async fn switch_claude_code_account(
    account_id: String,
) -> Result<crate::services::claude_code_accounts::ClaudeCodeAccount, String> {
    crate::services::claude_code_accounts::switch(&account_id).await
}

#[tauri::command]
pub async fn refresh_claude_code_account_quota(
    account_id: String,
) -> Result<crate::services::claude_code_accounts::ClaudeCodeAccount, String> {
    crate::services::claude_code_accounts::refresh(&account_id).await
}

#[tauri::command]
pub async fn delete_claude_code_account(account_id: String) -> Result<(), String> {
    crate::services::claude_code_accounts::delete(&account_id).await
}

use crate::services::workbuddy_accounts::{
    self, Account as WorkBuddyAccount, Edition as WorkBuddyEdition, LoginStart as WorkBuddyLogin,
};

use crate::services::zcode_accounts::{self, Account as ZCodeAccount, LoginStart as ZCodeLogin};

#[tauri::command]
pub async fn list_zcode_accounts() -> Result<Vec<ZCodeAccount>, String> {
    zcode_accounts::list().await
}
#[tauri::command]
pub async fn start_zcode_login(provider: String) -> Result<ZCodeLogin, String> {
    zcode_accounts::start_login(&provider).await
}
#[tauri::command]
pub async fn poll_zcode_login(login_id: String) -> Result<Option<ZCodeAccount>, String> {
    zcode_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_zcode_login(login_id: String) -> Result<(), String> {
    zcode_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_zcode_account(account_id: String) -> Result<ZCodeAccount, String> {
    zcode_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn refresh_zcode_account_quota(account_id: String) -> Result<ZCodeAccount, String> {
    zcode_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn delete_zcode_account(account_id: String) -> Result<(), String> {
    zcode_accounts::delete(&account_id).await
}

#[tauri::command]
pub async fn list_workbuddy_accounts(
    edition: WorkBuddyEdition,
) -> Result<Vec<WorkBuddyAccount>, String> {
    workbuddy_accounts::list(edition).await
}

#[tauri::command]
pub async fn start_workbuddy_login(edition: WorkBuddyEdition) -> Result<WorkBuddyLogin, String> {
    workbuddy_accounts::start_login(edition).await
}

#[tauri::command]
pub async fn poll_workbuddy_login(login_id: String) -> Result<Option<WorkBuddyAccount>, String> {
    workbuddy_accounts::poll_login(&login_id).await
}

#[tauri::command]
pub async fn cancel_workbuddy_login(login_id: String) -> Result<(), String> {
    workbuddy_accounts::cancel_login(&login_id)
}

#[tauri::command]
pub async fn switch_workbuddy_account(
    edition: WorkBuddyEdition,
    account_id: String,
) -> Result<WorkBuddyAccount, String> {
    workbuddy_accounts::switch(edition, &account_id).await
}

#[tauri::command]
pub async fn refresh_workbuddy_account_quota(
    edition: WorkBuddyEdition,
    account_id: String,
) -> Result<WorkBuddyAccount, String> {
    workbuddy_accounts::refresh(edition, &account_id).await
}

#[tauri::command]
pub async fn claim_workbuddy_daily_credits(
    edition: WorkBuddyEdition,
    account_id: String,
) -> Result<WorkBuddyAccount, String> {
    workbuddy_accounts::claim_daily(edition, &account_id).await
}

#[tauri::command]
pub async fn delete_workbuddy_account(
    edition: WorkBuddyEdition,
    account_id: String,
) -> Result<(), String> {
    workbuddy_accounts::delete(edition, &account_id).await
}

use crate::services::{deepseek_accounts, deepseek_oauth};
#[tauri::command]
pub async fn list_deepseek_accounts() -> Result<Vec<deepseek_accounts::Account>, String> {
    deepseek_accounts::list()
}
#[tauri::command]
pub async fn start_deepseek_login(locale: String) -> Result<deepseek_oauth::LoginStart, String> {
    deepseek_oauth::start(&locale).await
}
#[tauri::command]
pub async fn poll_deepseek_login(
    login_id: String,
) -> Result<Option<deepseek_accounts::Account>, String> {
    deepseek_oauth::poll(&login_id).await
}
#[tauri::command]
pub async fn cancel_deepseek_login(login_id: String) -> Result<(), String> {
    deepseek_oauth::cancel(&login_id)
}
#[tauri::command]
pub async fn switch_deepseek_account(
    account_id: String,
    locale: String,
) -> Result<deepseek_accounts::Account, String> {
    deepseek_accounts::switch(&account_id, &locale).await
}
#[tauri::command]
pub async fn refresh_deepseek_account_quota(
    account_id: String,
    locale: String,
) -> Result<deepseek_accounts::Account, String> {
    deepseek_accounts::refresh(&account_id, &locale).await
}
#[tauri::command]
pub async fn delete_deepseek_account(account_id: String) -> Result<(), String> {
    deepseek_accounts::delete(&account_id).await
}

#[tauri::command]
pub async fn start_grok_login() -> Result<serde_json::Value, String> {
    let (id, expires) = crate::services::grok_accounts::start_login().await?;
    Ok(serde_json::json!({"loginId":id,"expiresAt":expires}))
}
#[tauri::command]
pub async fn poll_grok_login(
    login_id: String,
) -> Result<Option<crate::services::grok_accounts::Account>, String> {
    crate::services::grok_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_grok_login(login_id: String) -> Result<(), String> {
    crate::services::grok_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub fn list_grok_accounts() -> Result<Vec<crate::services::grok_accounts::Account>, String> {
    crate::services::grok_accounts::list()
}
#[tauri::command]
pub async fn switch_grok_account(
    account_id: String,
) -> Result<crate::services::grok_accounts::Account, String> {
    crate::services::grok_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn delete_grok_account(account_id: String) -> Result<(), String> {
    crate::services::grok_accounts::delete(&account_id).await
}
#[tauri::command]
pub async fn refresh_grok_account(
    account_id: String,
) -> Result<crate::services::grok_accounts::Account, String> {
    crate::services::grok_accounts::refresh(&account_id).await
}

#[tauri::command]
pub async fn list_manus_accounts() -> Result<Vec<crate::services::manus_accounts::Account>, String>
{
    crate::services::manus_accounts::list().await
}
#[tauri::command]
pub async fn start_manus_login() -> Result<crate::services::manus_accounts::LoginStart, String> {
    crate::services::manus_accounts::start_login().await
}
#[tauri::command]
pub async fn poll_manus_login(
    login_id: String,
) -> Result<Option<crate::services::manus_accounts::Account>, String> {
    crate::services::manus_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub async fn cancel_manus_login(login_id: String) -> Result<(), String> {
    crate::services::manus_accounts::cancel_login(&login_id).await
}
#[tauri::command]
pub async fn switch_manus_account(
    account_id: String,
) -> Result<crate::services::manus_accounts::Account, String> {
    crate::services::manus_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn refresh_manus_account(
    account_id: String,
) -> Result<crate::services::manus_accounts::Account, String> {
    crate::services::manus_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn delete_manus_account(account_id: String) -> Result<(), String> {
    crate::services::manus_accounts::delete(&account_id).await
}
