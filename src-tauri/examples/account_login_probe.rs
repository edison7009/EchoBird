//! Temporary live probe; calls the production login flow without launching a native client.
use echobird_lib::services::{claude_desktop_accounts, manus_accounts};
use serde_json::json;
use std::time::Duration;

fn main() {
    let provider = std::env::args().nth(1).unwrap_or_else(|| "claude".into());
    let report = std::env::args().nth(2).expect("report path");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let result: Result<(), String> = async {
                    let start = if provider == "claude" {
                        serde_json::to_value(claude_desktop_accounts::start_login(handle.clone(), false).await?).map_err(|e| e.to_string())?
                    } else {
                        serde_json::to_value(manus_accounts::start_login(handle.clone()).await?).map_err(|e| e.to_string())?
                    };
                    let id = start["loginId"].as_str().ok_or("missing login ID")?;
                    loop {
                        let result = if provider == "claude" {
                            serde_json::to_value(claude_desktop_accounts::poll_login(&handle, id).await?).map_err(|e| e.to_string())?
                        } else {
                            serde_json::to_value(manus_accounts::poll_login(&handle, id).await?).map_err(|e| e.to_string())?
                        };
                        if !result["account"].is_null() { break; }
                        tokio::time::sleep(Duration::from_millis(800)).await;
                    }
                    Ok(())
                }.await;
                let summary = json!({"provider":provider,"accountSaved":result.is_ok(),"error":result.err(),"time":chrono::Utc::now().to_rfc3339()});
                let _ = std::fs::write(report, serde_json::to_vec_pretty(&summary).unwrap());
                tokio::time::sleep(Duration::from_secs(3)).await;
                handle.exit(if summary["accountSaved"] == true { 0 } else { 1 });
            });
            Ok(())
        })
        .build(context)
        .expect("build login probe")
        .run(|_, event| {
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event { api.prevent_exit(); }
        });
}
