use crate::services::voice_input::{self, config::Config, Snapshot};
use tauri::{AppHandle, WebviewWindow};

fn allowed(window: &WebviewWindow, main_only: bool) -> Result<(), String> {
    match window.label() {
        "main" => Ok(()),
        "voice-island" if !main_only => Ok(()),
        _ => Err("voice.error.config".into()),
    }
}

#[tauri::command]
pub fn voice_input_models(
    window: WebviewWindow,
) -> Result<Vec<crate::models::model::ModelConfig>, String> {
    allowed(&window, true)?;
    Ok(crate::services::model_manager::get_user_models()
        .into_iter()
        .filter(|model| model.scope == crate::models::model::ModelScope::VoiceInput)
        .collect())
}

#[tauri::command]
pub fn voice_input_state(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, false)?;
    Ok(voice_input::snapshot(&app))
}
#[tauri::command]
pub fn voice_input_devices(window: WebviewWindow) -> Result<Vec<String>, String> {
    allowed(&window, true)?;
    voice_input::devices()
}
#[tauri::command]
pub async fn voice_input_save(
    app: AppHandle,
    window: WebviewWindow,
    config: Config,
) -> Result<Snapshot, String> {
    allowed(&window, true)?;
    voice_input::save(&app, config).await
}
#[tauri::command]
pub async fn voice_input_start(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, true)?;
    voice_input::begin(&app, "test").await
}

#[tauri::command]
pub async fn voice_input_record(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, false)?;
    voice_input::begin(&app, "island").await
}
#[tauri::command]
pub async fn voice_input_stop(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, false)?;
    voice_input::stop(&app).await
}
#[tauri::command]
pub async fn voice_input_cancel(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, false)?;
    Ok(voice_input::cancel(&app).await)
}
#[tauri::command]
pub fn voice_input_suspend(
    app: AppHandle,
    window: WebviewWindow,
    suspended: bool,
) -> Result<(), String> {
    allowed(&window, true)?;
    voice_input::suspend(&app, suspended);
    Ok(())
}
#[tauri::command]
pub fn voice_input_island_ready(app: AppHandle, window: WebviewWindow) -> Result<Snapshot, String> {
    allowed(&window, false)?;
    Ok(voice_input::island_ready(&app))
}
#[tauri::command]
pub fn voice_input_replay(
    app: AppHandle,
    window: WebviewWindow,
    audio_id: u64,
) -> Result<tauri::ipc::Response, String> {
    allowed(&window, true)?;
    voice_input::replay_audio(&app, audio_id)
}

#[tauri::command]
pub fn voice_input_show_main(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    allowed(&window, false)?;
    crate::show_main_window(&app);
    Ok(())
}

#[tauri::command]
pub async fn voice_selection_open(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if !matches!(window.label(), "main" | "voice-translation") {
        return Err("voice.error.config".into());
    }
    voice_input::selection::open(&app, window.label() == "main").await
}

#[tauri::command]
pub async fn voice_selection_ask(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if window.label() != "voice-translation" {
        return Err("voice.error.config".into());
    }
    voice_input::selection::ask(&app).await
}

#[tauri::command]
pub fn voice_selection_state(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<voice_input::selection::Translation, String> {
    if window.label() != "voice-translation" {
        return Err("voice.error.config".into());
    }
    Ok(voice_input::selection::snapshot(&app))
}

#[tauri::command]
pub fn voice_selection_ready(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<voice_input::selection::Translation, String> {
    if window.label() != "voice-translation" {
        return Err("voice.error.config".into());
    }
    Ok(voice_input::selection::ready(&app))
}

#[tauri::command]
pub async fn voice_selection_translate(
    app: AppHandle,
    window: WebviewWindow,
    source: String,
    language: String,
) -> Result<(), String> {
    if window.label() != "voice-translation" {
        return Err("voice.error.config".into());
    }
    voice_input::selection::translate(&app, source, language).await
}

#[tauri::command]
pub fn voice_selection_close(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    if !matches!(
        window.label(),
        "voice-island" | "voice-translation" | "main"
    ) {
        return Err("voice.error.config".into());
    }
    voice_input::selection::close(&app);
    Ok(())
}
