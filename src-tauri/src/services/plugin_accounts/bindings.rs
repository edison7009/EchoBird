//! Compatibility DTOs and dispatch for the existing account controls.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

async fn invoke<T: serde::de::DeserializeOwned>(
    application: &str,
    operation: &str,
    input: Value,
) -> Result<T, String> {
    let value = super::call(application, operation, input.clone()).await?;
    if operation == "login.start" {
        if let Some(id) = value["loginId"].as_str() {
            super::logins()
                .lock()
                .unwrap()
                .insert(id.into(), application.into());
        }
    } else if operation == "login.cancel" {
        if let Some(id) = input["loginId"].as_str() {
            super::logins().lock().unwrap().remove(id);
        }
    }
    serde_json::from_value(value).map_err(|_| "Invalid account plugin result".into())
}

/// Warm-ups run through the installed Claude Code, which a Claude Desktop plugin
/// cannot locate by itself.
async fn warm_up_claude<T: serde::de::DeserializeOwned>(
    application: &str,
    id: &str,
) -> Result<T, String> {
    let executable = crate::services::tool_manager::get_tool_exe_path("claudecode")
        .ok_or("accountError.claudeCodeRequired")?;
    invoke(
        application,
        "account.warmup",
        json!({"accountId":id,"executable":executable}),
    )
    .await
}

pub mod claude_code_accounts {
    use super::*;
    #[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "camelCase")]
    pub struct ClaudeCodeQuota {
        pub remaining_percent: i32,
        pub reset_at: Option<i64>,
    }
    #[derive(Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct ClaudeCodeAccount {
        pub id: String,
        pub email: String,
        pub plan: Option<String>,
        pub five_hour: Option<ClaudeCodeQuota>,
        pub seven_day: Option<ClaudeCodeQuota>,
        pub active: bool,
    }
    pub async fn list() -> Result<Vec<ClaudeCodeAccount>, String> {
        super::super::list("claudecode", false).await
    }
    pub async fn list_saved_accounts() -> Result<Vec<ClaudeCodeAccount>, String> {
        super::super::list("claudecode", true).await
    }
    pub async fn switch(id: &str) -> Result<ClaudeCodeAccount, String> {
        invoke("claudecode", "account.apply", json!({"accountId":id})).await
    }
    pub async fn refresh(id: &str) -> Result<ClaudeCodeAccount, String> {
        invoke("claudecode", "account.refresh", json!({"accountId":id})).await
    }
    pub async fn warm_up(id: &str) -> Result<ClaudeCodeAccount, String> {
        warm_up_claude("claudecode", id).await
    }
    pub async fn delete(id: &str) -> Result<(), String> {
        invoke("claudecode", "account.remove", json!({"accountId":id})).await
    }
}

pub mod claude_code_oauth {
    use super::claude_code_accounts::ClaudeCodeAccount;
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginStart {
        login_id: String,
        authorization_url: String,
        expires_at: i64,
    }
    pub async fn start() -> Result<LoginStart, String> {
        invoke("claudecode", "login.start", json!({})).await
    }
    pub async fn complete(login_id: &str, code: &str) -> Result<ClaudeCodeAccount, String> {
        invoke(
            "claudecode",
            "login.complete",
            json!({"loginId":login_id,"code":code}),
        )
        .await
    }
    pub async fn cancel(login_id: &str) -> Result<(), String> {
        invoke("claudecode", "login.cancel", json!({"loginId":login_id})).await
    }
}

pub mod claude_desktop_accounts {
    pub use super::claude_code_accounts::ClaudeCodeAccount as Account;
    use super::*;
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginStart {
        login_id: String,
        expires_at: i64,
    }
    #[derive(Default, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginPoll {
        account: Option<Account>,
        awaiting_client_exit: bool,
        expires_at: Option<i64>,
        /// The plugin removed the sign-in from this device for the next account.
        #[serde(default)]
        cleared: bool,
    }
    pub async fn list() -> Result<Vec<Account>, String> {
        super::super::list("claudedesktop", false).await
    }
    /// Saves the signed-in account; with `next`, also clears it from this device.
    pub async fn start_login(_app: tauri::AppHandle, next: bool) -> Result<LoginStart, String> {
        invoke("claudedesktop", "login.start", json!({"next":next})).await
    }
    pub async fn poll_login(_app: &tauri::AppHandle, id: &str) -> Result<LoginPoll, String> {
        invoke("claudedesktop", "login.poll", json!({"loginId":id})).await
    }
    pub async fn cancel_login(_app: &tauri::AppHandle, id: &str) -> Result<(), String> {
        invoke("claudedesktop", "login.cancel", json!({"loginId":id})).await
    }
    pub async fn switch(id: &str) -> Result<Account, String> {
        invoke("claudedesktop", "account.apply", json!({"accountId":id})).await
    }
    pub async fn refresh(id: &str) -> Result<Account, String> {
        invoke("claudedesktop", "account.refresh", json!({"accountId":id})).await
    }
    pub async fn warm_up(id: &str) -> Result<Account, String> {
        warm_up_claude("claudedesktop", id).await
    }
    pub async fn delete(id: &str) -> Result<(), String> {
        invoke("claudedesktop", "account.remove", json!({"accountId":id})).await
    }
}

pub mod workbuddy_accounts {
    use super::*;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Edition {
        #[serde(rename = "workbuddy")]
        Cn,
        #[serde(rename = "workbuddyai")]
        Ai,
    }
    impl Edition {
        pub fn application(self) -> &'static str {
            match self {
                Self::Cn => "workbuddy",
                Self::Ai => "workbuddyai",
            }
        }
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Account {
        pub id: String,
        pub name: String,
        pub edition: Edition,
        pub plan: Option<String>,
        pub remaining: Option<f64>,
        pub total: Option<f64>,
        pub base_remaining: Option<f64>,
        pub base_total: Option<f64>,
        pub base_reset_at: Option<i64>,
        pub reward_remaining: Option<f64>,
        pub reward_total: Option<f64>,
        pub reward_next_expiry_at: Option<i64>,
        pub addon_remaining: Option<f64>,
        pub daily_claimed_at: Option<i64>,
        pub expires_at: Option<i64>,
        pub active: bool,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginStart {
        login_id: String,
        verification_uri: String,
        expires_at: i64,
    }
    pub async fn list(edition: Edition) -> Result<Vec<Account>, String> {
        super::super::list(edition.application(), false).await
    }
    pub async fn list_saved_accounts(edition: Edition) -> Result<Vec<Account>, String> {
        super::super::list(edition.application(), true).await
    }
    pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
        invoke(edition.application(), "login.start", json!({})).await
    }
    pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
        invoke(
            &super::super::login_application(id)?,
            "login.poll",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn cancel_login(id: &str) -> Result<(), String> {
        invoke(
            &super::super::login_application(id)?,
            "login.cancel",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn switch(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.apply",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn refresh(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.refresh",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.claim",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn delete(edition: Edition, id: &str) -> Result<(), String> {
        invoke(
            edition.application(),
            "account.remove",
            json!({"accountId":id}),
        )
        .await
    }
}

pub mod doubao_accounts {
    use super::*;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Edition {
        #[serde(rename = "doubaowork")]
        DoubaoWork,
        #[serde(rename = "dola")]
        Dola,
    }
    impl Edition {
        pub fn application(self) -> &'static str {
            match self {
                Self::DoubaoWork => "doubaowork",
                Self::Dola => "dola",
            }
        }
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Account {
        pub id: String,
        pub name: String,
        pub email: Option<String>,
        pub edition: Edition,
        pub plan: Option<String>,
        pub expires_at: Option<i64>,
        pub remaining: Option<f64>,
        pub total: Option<f64>,
        #[serde(default)]
        pub quota: Option<Value>,
        #[serde(default)]
        pub daily_reward: Option<Value>,
        pub active: bool,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginStart {
        pub login_id: String,
        pub expires_at: i64,
    }
    pub async fn list(edition: Edition) -> Result<Vec<Account>, String> {
        super::super::list(edition.application(), false).await
    }
    pub async fn start_login(
        _app: tauri::AppHandle,
        edition: Edition,
    ) -> Result<LoginStart, String> {
        invoke(edition.application(), "login.start", json!({})).await
    }
    pub async fn poll_login(_app: &tauri::AppHandle, id: &str) -> Result<Option<Account>, String> {
        invoke(
            &super::super::login_application(id)?,
            "login.poll",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn cancel_login(_app: &tauri::AppHandle, id: &str) -> Result<(), String> {
        invoke(
            &super::super::login_application(id)?,
            "login.cancel",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn apply(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.apply",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn refresh(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.refresh",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(
            edition.application(),
            "account.claim",
            json!({"accountId":id}),
        )
        .await
    }
    pub async fn delete(edition: Edition, id: &str) -> Result<(), String> {
        invoke(
            edition.application(),
            "account.remove",
            json!({"accountId":id}),
        )
        .await
    }
}

pub mod trae_accounts {
    use super::*;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Edition {
        #[serde(rename = "trae")]
        Trae,
        #[serde(rename = "traecn")]
        Traecn,
    }
    impl Edition {
        pub fn tool(self) -> &'static str {
            match self {
                Self::Trae => "trae",
                Self::Traecn => "traecn",
            }
        }
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Account {
        pub id: String,
        pub edition: Edition,
        pub name: String,
        pub plan: Option<String>,
        pub active: bool,
        pub remaining: Option<f64>,
        #[serde(default)]
        pub total: Option<f64>,
        #[serde(default)]
        pub expires_at: Option<i64>,
        #[serde(default)]
        pub currency: Option<String>,
        #[serde(default)]
        pub base_remaining: Option<f64>,
        #[serde(default)]
        pub base_total: Option<f64>,
        #[serde(default)]
        pub base_reset_at: Option<i64>,
        #[serde(default)]
        pub reward_remaining: Option<f64>,
        #[serde(default)]
        pub reward_total: Option<f64>,
        #[serde(default)]
        pub reward_next_expiry_at: Option<i64>,
        #[serde(default)]
        pub addon_remaining: Option<f64>,
        pub work_remaining: Option<f64>,
        pub daily_claimed_at: Option<i64>,
        pub device_claimed_at: Option<i64>,
        pub reward_enabled: Option<bool>,
        pub subscription_end_at: Option<i64>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct LoginStart {
        pub login_id: String,
        pub verification_uri: String,
        pub expires_at: i64,
    }
    pub async fn list_accounts(edition: Edition) -> Result<Vec<Account>, String> {
        super::super::list(edition.tool(), false).await
    }
    pub async fn start_login(edition: Edition) -> Result<LoginStart, String> {
        invoke(edition.tool(), "login.start", json!({})).await
    }
    pub async fn poll_login(id: &str) -> Result<Option<Account>, String> {
        invoke(
            &super::super::login_application(id)?,
            "login.poll",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn cancel_login(id: &str) -> Result<(), String> {
        invoke(
            &super::super::login_application(id)?,
            "login.cancel",
            json!({"loginId":id}),
        )
        .await
    }
    pub async fn activate_account(edition: Edition, id: &str) -> Result<(), String> {
        invoke(edition.tool(), "account.apply", json!({"accountId":id})).await
    }
    pub async fn refresh_quota(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(edition.tool(), "account.refresh", json!({"accountId":id})).await
    }
    pub async fn claim_daily(edition: Edition, id: &str) -> Result<Account, String> {
        invoke(edition.tool(), "account.claim", json!({"accountId":id})).await
    }
    pub async fn delete_account(edition: Edition, id: &str) -> Result<(), String> {
        invoke(edition.tool(), "account.remove", json!({"accountId":id})).await
    }
}
