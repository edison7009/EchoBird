use crate::services::commandcode_accounts::{self, Account, LoginStart};
#[tauri::command]
pub async fn list_commandcode_accounts() -> Result<Vec<Account>, String> {
    commandcode_accounts::list().await
}
#[tauri::command]
pub async fn start_commandcode_login() -> Result<LoginStart, String> {
    commandcode_accounts::start().await
}
#[tauri::command]
pub async fn poll_commandcode_login(login_id: String) -> Result<Option<Account>, String> {
    commandcode_accounts::poll(&login_id).await
}
#[tauri::command]
pub async fn cancel_commandcode_login(login_id: String) -> Result<(), String> {
    commandcode_accounts::cancel(&login_id).await
}
#[tauri::command]
pub async fn refresh_commandcode_account(account_id: String) -> Result<Account, String> {
    commandcode_accounts::refresh(&account_id).await
}
#[tauri::command]
pub async fn switch_commandcode_account(account_id: String) -> Result<Account, String> {
    commandcode_accounts::apply(&account_id).await
}
#[tauri::command]
pub async fn delete_commandcode_account(account_id: String) -> Result<(), String> {
    commandcode_accounts::remove(&account_id).await
}
