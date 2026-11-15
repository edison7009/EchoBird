use crate::{
    models::model::AccountProvider,
    services::{
        account_model_sources,
        doubao_accounts::{self, Account, Edition, LoginStart},
    },
};

#[tauri::command]
pub async fn list_doubao_accounts(edition: Edition) -> Result<Vec<Account>, String> {
    doubao_accounts::list(edition).await
}
#[tauri::command]
pub async fn start_doubao_login(
    app: tauri::AppHandle,
    edition: Edition,
) -> Result<LoginStart, String> {
    doubao_accounts::start_login(app, edition).await
}
#[tauri::command]
pub async fn poll_doubao_login(
    app: tauri::AppHandle,
    login_id: String,
) -> Result<Option<Account>, String> {
    doubao_accounts::poll_login(&app, &login_id).await
}
#[tauri::command]
pub async fn cancel_doubao_login(app: tauri::AppHandle, login_id: String) -> Result<(), String> {
    doubao_accounts::cancel_login(&app, &login_id)
}
#[tauri::command]
pub async fn refresh_doubao_account_quota(
    edition: Edition,
    account_id: String,
) -> Result<Account, String> {
    doubao_accounts::refresh(edition, &account_id).await
}

#[tauri::command]
pub async fn claim_doubao_daily_credits(
    edition: Edition,
    account_id: String,
) -> Result<Account, String> {
    doubao_accounts::claim_daily(edition, &account_id).await
}
#[tauri::command]
pub async fn delete_doubao_account(edition: Edition, account_id: String) -> Result<(), String> {
    account_model_sources::finish_account_deletion(
        match edition {
            Edition::DoubaoWork => AccountProvider::DoubaoWork,
            Edition::Dola => AccountProvider::Dola,
        },
        &account_id,
        doubao_accounts::delete(edition, &account_id).await,
    )
}
#[tauri::command]
pub async fn switch_doubao_account(
    edition: Edition,
    account_id: String,
) -> Result<Account, String> {
    doubao_accounts::apply(edition, &account_id).await
}
