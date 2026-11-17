//! Read summaries after an explicit import without re-entering native sync/login paths.
use super::super::*;
use serde_json::Value;

pub(crate) async fn list(scope: &str) -> Result<Value, String> {
    macro_rules! rows {
        ($value:expr) => {
            serde_json::to_value($value?).map_err(|_| "accountError.format".to_string())
        };
    }
    match scope {
        "codex" => rows!(codex_accounts::list_saved_accounts()),
        "claudecode" => rows!(claude_code_accounts::list_saved_accounts().await),
        "claudedesktop" => rows!(claude_desktop_accounts::list_saved_accounts().await),
        "antigravity" => rows!(antigravity_accounts::list_saved_accounts().await),
        "workbuddy" => {
            rows!(workbuddy_accounts::list_saved_accounts(workbuddy_accounts::Edition::Cn).await)
        }
        "workbuddyai" => {
            rows!(workbuddy_accounts::list_saved_accounts(workbuddy_accounts::Edition::Ai).await)
        }
        "trae" => rows!(trae_accounts::list_saved_accounts(trae_accounts::Edition::Trae).await),
        "traecn" => rows!(trae_accounts::list_saved_accounts(trae_accounts::Edition::Traecn).await),
        "dumate" => rows!(dumate_accounts::list().await),
        "qwenwork" => rows!(qwenwork_accounts::list(qwenwork_accounts::Edition::QwenWork).await),
        "qwenworkcn" => {
            rows!(qwenwork_accounts::list(qwenwork_accounts::Edition::QwenWorkCn).await)
        }
        "doubaowork" => rows!(doubao_accounts::list(doubao_accounts::Edition::DoubaoWork).await),
        "dola" => rows!(doubao_accounts::list(doubao_accounts::Edition::Dola).await),
        "qoder" => rows!(qoder_accounts::list_saved_accounts(qoder_accounts::Edition::Qoder).await),
        "qodercn" => {
            rows!(qoder_accounts::list_saved_accounts(qoder_accounts::Edition::QoderCn).await)
        }
        "codearts" => rows!(codearts_accounts::list_saved_accounts()),
        "zcode" => rows!(zcode_accounts::list_saved_accounts()),
        "opendesign" => rows!(opendesign_accounts::list_saved_accounts().await),
        "kiro" => rows!(kiro_accounts::list_saved_accounts().await),
        "grok" => rows!(grok_accounts::list_saved_accounts()),
        "grokbot" => rows!(grok_bot_accounts::speech::list_saved_accounts().await),
        "dsh" => rows!(deepseek_accounts::list_saved_accounts()),
        "cline" => rows!(cline_accounts::list_saved_accounts().await),
        "copilot" | "copilotdesktop" => rows!(copilot_accounts::list_saved_accounts().await),
        "devin" | "devincli" => rows!(devin_accounts::list_saved_accounts().await),
        "cursor" | "cursorcli" => rows!(cursor_accounts::speech::list_saved_accounts().await),
        "commandcode" => rows!(commandcode_accounts::list_saved_accounts()),
        "kilo" => rows!(kilo_accounts::list_saved_accounts().await),
        "manus" => rows!(manus_accounts::list().await),
        "cue" => rows!(cue_accounts::list().await),
        "kimi" | "kimicode" | "kimidesktop" => rows!(kimi_accounts::list_saved_accounts()),
        "minimax" => rows!(minimax_accounts::list_saved_accounts().await),
        _ => Err("accountError.invalidAccount".into()),
    }
}
