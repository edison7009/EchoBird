// Tauri Commands for tool operations — exposed to frontend via invoke()

use crate::models::tool::DetectedTool;
use crate::services::antigravity_accounts::{
    self, Account as AntigravityAccount, LoginStart as AntigravityLogin,
};
use crate::services::codex_accounts::{self, CodexAccountSummary};
use crate::services::tool_config_manager::{self, ApplyResult, ModelInfo};
use crate::services::tool_manager;
use crate::{
    models::model::AccountProvider, services::account_model_sources::finish_account_deletion,
};

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
    finish_account_deletion(
        AccountProvider::Antigravity,
        &account_id,
        antigravity_accounts::delete(&account_id).await,
    )
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
pub async fn list_cursor_accounts(tool: Option<String>) -> Result<Vec<CursorAccount>, String> {
    match cursor_client(tool.as_deref())? {
        "cursorcli" => cursor_accounts::cli::list().await,
        _ => cursor_accounts::list().await,
    }
}
#[tauri::command]
pub fn start_cursor_login(tool: Option<String>) -> Result<CursorLogin, String> {
    match cursor_client(tool.as_deref())? {
        "cursorcli" => cursor_accounts::cli::start_login(),
        _ => cursor_accounts::start_login(),
    }
}
#[tauri::command]
pub async fn poll_cursor_login(
    login_id: String,
    tool: Option<String>,
) -> Result<Option<CursorAccount>, String> {
    match cursor_client(tool.as_deref())? {
        "cursorcli" => cursor_accounts::cli::poll_login(&login_id).await,
        _ => cursor_accounts::poll_login(&login_id).await,
    }
}
#[tauri::command]
pub fn cancel_cursor_login(login_id: String) -> Result<(), String> {
    cursor_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_cursor_account(
    account_id: String,
    tool: Option<String>,
) -> Result<CursorAccount, String> {
    match cursor_client(tool.as_deref())? {
        "cursorcli" => cursor_accounts::cli::switch(&account_id).await,
        _ => cursor_accounts::switch(&account_id).await,
    }
}
#[tauri::command]
pub async fn delete_cursor_account(account_id: String) -> Result<(), String> {
    cursor_accounts::delete(&account_id).await
}
#[tauri::command]
pub async fn refresh_cursor_account(
    account_id: String,
    tool: Option<String>,
) -> Result<crate::services::cursor_usage::Usage, String> {
    match cursor_client(tool.as_deref())? {
        "cursorcli" => cursor_accounts::cli::refresh(&account_id).await,
        _ => cursor_accounts::refresh(&account_id).await,
    }
}

fn cursor_client(tool: Option<&str>) -> Result<&str, String> {
    match tool.unwrap_or("cursor") {
        "cursor" => Ok("cursor"),
        "cursorcli" => Ok("cursorcli"),
        _ => Err("accountError.invalidAccount".into()),
    }
}

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
    finish_account_deletion(
        AccountProvider::Codex,
        &account_id,
        codex_accounts::delete_account(&account_id),
    )
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
    // dispatcher: `explorer <file>` opens the file's
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
pub async fn start_claude_code_login(
) -> Result<crate::services::claude_code_oauth::LoginStart, String> {
    crate::services::claude_code_oauth::start().await
}

#[tauri::command]
pub async fn complete_claude_code_login(
    login_id: String,
    code: String,
) -> Result<crate::services::claude_code_accounts::ClaudeCodeAccount, String> {
    crate::services::claude_code_oauth::complete(&login_id, &code).await
}

#[tauri::command]
pub async fn cancel_claude_code_login(login_id: String) -> Result<(), String> {
    crate::services::claude_code_oauth::cancel(&login_id).await
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
pub async fn warm_up_claude_code_account(
    account_id: String,
) -> Result<crate::services::claude_code_accounts::ClaudeCodeAccount, String> {
    crate::services::claude_code_accounts::warm_up(&account_id).await
}

#[tauri::command]
pub async fn delete_claude_code_account(account_id: String) -> Result<(), String> {
    finish_account_deletion(
        AccountProvider::ClaudeCode,
        &account_id,
        crate::services::claude_code_accounts::delete(&account_id).await,
    )
}

use crate::services::workbuddy_accounts::{
    self, Account as WorkBuddyAccount, Edition as WorkBuddyEdition, LoginStart as WorkBuddyLogin,
};

use crate::services::zcode_accounts::{self, Account as ZCodeAccount, LoginStart as ZCodeLogin};

use crate::services::opendesign_accounts::{
    self, Account as OpenDesignAccount, LoginStart as OpenDesignLogin,
};

#[tauri::command]
pub async fn list_opendesign_accounts() -> Result<Vec<OpenDesignAccount>, String> {
    opendesign_accounts::list().await
}
#[tauri::command]
pub async fn start_opendesign_login() -> Result<OpenDesignLogin, String> {
    opendesign_accounts::start_login().await
}
#[tauri::command]
pub async fn poll_opendesign_login(login_id: String) -> Result<Option<OpenDesignAccount>, String> {
    opendesign_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_opendesign_login(login_id: String) -> Result<(), String> {
    opendesign_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_opendesign_account(account_id: String) -> Result<OpenDesignAccount, String> {
    opendesign_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn refresh_opendesign_account(account_id: String) -> Result<OpenDesignAccount, String> {
    opendesign_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn delete_opendesign_account(account_id: String) -> Result<(), String> {
    finish_account_deletion(
        AccountProvider::OpenDesign,
        &account_id,
        opendesign_accounts::delete(&account_id).await,
    )
}

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
    finish_account_deletion(
        AccountProvider::Zcode,
        &account_id,
        zcode_accounts::delete(&account_id).await,
    )
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
    workbuddy_accounts::cancel_login(&login_id).await
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
    finish_account_deletion(
        match edition {
            WorkBuddyEdition::Cn => AccountProvider::WorkBuddy,
            WorkBuddyEdition::Ai => AccountProvider::WorkBuddyAi,
        },
        &account_id,
        workbuddy_accounts::delete(edition, &account_id).await,
    )
}

use crate::services::trae_accounts::{self, Account as TraeAccount, Edition as TraeEdition};
use crate::services::{deepseek_accounts, deepseek_oauth};

#[tauri::command]
pub async fn list_trae_accounts(edition: TraeEdition) -> Result<Vec<TraeAccount>, String> {
    trae_accounts::list_accounts(edition).await
}

#[tauri::command]
pub async fn start_trae_login(edition: TraeEdition) -> Result<trae_accounts::LoginStart, String> {
    trae_accounts::start_login(edition).await
}

#[tauri::command]
pub async fn poll_trae_login(login_id: String) -> Result<Option<TraeAccount>, String> {
    trae_accounts::poll_login(&login_id).await
}

#[tauri::command]
pub async fn cancel_trae_login(login_id: String) -> Result<(), String> {
    trae_accounts::cancel_login(&login_id).await
}

#[tauri::command]
pub async fn switch_trae_account(edition: TraeEdition, account_id: String) -> Result<(), String> {
    trae_accounts::activate_account(edition, &account_id).await
}

#[tauri::command]
pub async fn refresh_trae_account_quota(
    edition: TraeEdition,
    account_id: String,
) -> Result<TraeAccount, String> {
    trae_accounts::refresh_quota(edition, &account_id).await
}

#[tauri::command]
pub async fn claim_trae_daily_credits(
    edition: TraeEdition,
    account_id: String,
) -> Result<TraeAccount, String> {
    trae_accounts::claim_daily(edition, &account_id).await
}

#[tauri::command]
pub async fn delete_trae_account(edition: TraeEdition, account_id: String) -> Result<(), String> {
    finish_account_deletion(
        match edition {
            TraeEdition::Trae => AccountProvider::Trae,
            TraeEdition::Traecn => AccountProvider::Traecn,
        },
        &account_id,
        trae_accounts::delete_account(edition, &account_id).await,
    )
}

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
    finish_account_deletion(
        AccountProvider::Grok,
        &account_id,
        crate::services::grok_accounts::delete(&account_id).await,
    )
}
#[tauri::command]
pub async fn refresh_grok_account(
    account_id: String,
) -> Result<crate::services::grok_accounts::Account, String> {
    crate::services::grok_accounts::refresh(&account_id).await
}

#[tauri::command]
pub async fn list_claude_desktop_accounts(
) -> Result<Vec<crate::services::claude_desktop_accounts::Account>, String> {
    crate::services::claude_desktop_accounts::list().await
}
#[tauri::command]
pub async fn start_claude_desktop_login(
    app: tauri::AppHandle,
    next: Option<bool>,
) -> Result<crate::services::claude_desktop_accounts::LoginStart, String> {
    crate::services::claude_desktop_accounts::start_login(app, next.unwrap_or(false)).await
}
#[tauri::command]
pub async fn poll_claude_desktop_login(
    app: tauri::AppHandle,
    login_id: String,
) -> Result<crate::services::claude_desktop_accounts::LoginPoll, String> {
    crate::services::claude_desktop_accounts::poll_login(&app, &login_id).await
}
#[tauri::command]
pub async fn cancel_claude_desktop_login(
    app: tauri::AppHandle,
    login_id: String,
) -> Result<(), String> {
    crate::services::claude_desktop_accounts::cancel_login(&app, &login_id).await
}
#[tauri::command]
pub async fn switch_claude_desktop_account(
    account_id: String,
) -> Result<crate::services::claude_desktop_accounts::Account, String> {
    crate::services::claude_desktop_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn refresh_claude_desktop_account(
    account_id: String,
) -> Result<crate::services::claude_desktop_accounts::Account, String> {
    crate::services::claude_desktop_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn warm_up_claude_desktop_account(
    account_id: String,
) -> Result<crate::services::claude_desktop_accounts::Account, String> {
    crate::services::claude_desktop_accounts::warm_up(&account_id).await
}
#[tauri::command]
pub async fn delete_claude_desktop_account(account_id: String) -> Result<(), String> {
    crate::services::claude_desktop_accounts::delete(&account_id).await
}

#[tauri::command]
pub async fn list_manus_accounts() -> Result<Vec<crate::services::manus_accounts::Account>, String>
{
    crate::services::manus_accounts::list().await
}
#[tauri::command]
pub async fn start_manus_login(
    app: tauri::AppHandle,
) -> Result<crate::services::manus_accounts::LoginStart, String> {
    crate::services::manus_accounts::start_login(app).await
}
#[tauri::command]
pub async fn poll_manus_login(
    app: tauri::AppHandle,
    login_id: String,
) -> Result<crate::services::manus_accounts::LoginPoll, String> {
    crate::services::manus_accounts::poll_login(&app, &login_id).await
}
#[tauri::command]
pub async fn cancel_manus_login(app: tauri::AppHandle, login_id: String) -> Result<(), String> {
    crate::services::manus_accounts::cancel_login(&app, &login_id).await
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

#[tauri::command]
pub async fn list_cue_accounts() -> Result<Vec<crate::services::cue_accounts::Account>, String> {
    crate::services::cue_accounts::list().await
}
#[tauri::command]
pub async fn start_cue_login(
    app: tauri::AppHandle,
) -> Result<crate::services::cue_accounts::LoginStart, String> {
    crate::services::cue_accounts::start_login(app).await
}
#[tauri::command]
pub async fn poll_cue_login(
    app: tauri::AppHandle,
    login_id: String,
) -> Result<crate::services::cue_accounts::LoginPoll, String> {
    crate::services::cue_accounts::poll_login(&app, &login_id).await
}
#[tauri::command]
pub async fn cancel_cue_login(app: tauri::AppHandle, login_id: String) -> Result<(), String> {
    crate::services::cue_accounts::cancel_login(&app, &login_id).await
}
#[tauri::command]
pub async fn switch_cue_account(
    account_id: String,
) -> Result<crate::services::cue_accounts::Account, String> {
    crate::services::cue_accounts::switch(&account_id).await
}
#[tauri::command]
pub async fn refresh_cue_account(
    account_id: String,
) -> Result<crate::services::cue_accounts::Account, String> {
    crate::services::cue_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn delete_cue_account(account_id: String) -> Result<(), String> {
    crate::services::cue_accounts::delete(&account_id).await
}

use crate::services::kiro_accounts::{self, Account as KiroAccount, Edition as KiroEdition};

#[tauri::command]
pub async fn list_kiro_accounts(edition: KiroEdition) -> Result<Vec<KiroAccount>, String> {
    kiro_accounts::list_accounts(edition).await
}
#[tauri::command]
pub async fn start_kiro_login(edition: KiroEdition) -> Result<kiro_accounts::LoginStart, String> {
    kiro_accounts::start_login(edition).await
}
#[tauri::command]
pub async fn poll_kiro_login(login_id: String) -> Result<Option<KiroAccount>, String> {
    kiro_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub async fn cancel_kiro_login(login_id: String) -> Result<(), String> {
    kiro_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_kiro_account(edition: KiroEdition, account_id: String) -> Result<(), String> {
    kiro_accounts::activate_account(edition, &account_id).await
}
#[tauri::command]
pub async fn refresh_kiro_account_quota(
    edition: KiroEdition,
    account_id: String,
) -> Result<KiroAccount, String> {
    kiro_accounts::refresh_quota(edition, &account_id).await
}
#[tauri::command]
pub async fn delete_kiro_account(edition: KiroEdition, account_id: String) -> Result<(), String> {
    finish_account_deletion(
        AccountProvider::Kiro,
        &account_id,
        kiro_accounts::delete_account(edition, &account_id).await,
    )
}

#[tauri::command]
pub async fn list_devin_accounts(
    tool_id: String,
) -> Result<Vec<crate::services::devin_accounts::Account>, String> {
    crate::services::devin_accounts::list(&tool_id).await
}
#[tauri::command]
pub async fn start_devin_login(
    tool_id: String,
) -> Result<crate::services::devin_accounts::LoginStart, String> {
    crate::services::devin_accounts::start_login(&tool_id).await
}
#[tauri::command]
pub async fn poll_devin_login(
    login_id: String,
) -> Result<Option<crate::services::devin_accounts::Account>, String> {
    crate::services::devin_accounts::poll_login(&login_id).await
}
#[tauri::command]
pub fn cancel_devin_login(login_id: String) -> Result<(), String> {
    crate::services::devin_accounts::cancel_login(&login_id)
}
#[tauri::command]
pub async fn switch_devin_account(
    account_id: String,
    tool_id: String,
) -> Result<crate::services::devin_accounts::Account, String> {
    crate::services::devin_accounts::switch(&account_id, &tool_id).await
}
#[tauri::command]
pub async fn refresh_devin_account(
    account_id: String,
) -> Result<crate::services::devin_accounts::Account, String> {
    crate::services::devin_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn delete_devin_account(account_id: String) -> Result<(), String> {
    finish_account_deletion(
        AccountProvider::Devin,
        &account_id,
        crate::services::devin_accounts::delete(&account_id).await,
    )
}
