// Tauri IPC commands that expose the compile-time bundled install/script
// assets to the frontend. Lets the AppManager and Mother Agent UIs work
// offline — no fetch to echobird.ai.

use crate::services::bundled_assets;

#[tauri::command]
pub fn get_mother_system_prompt() -> String {
    bundled_assets::MOTHER_SYSTEM_PROMPT.to_string()
}

#[tauri::command]
pub fn get_mother_hints() -> String {
    bundled_assets::MOTHER_HINTS_JSON.to_string()
}

#[tauri::command]
pub fn get_install_index() -> String {
    bundled_assets::INSTALL_INDEX_JSON.to_string()
}

#[tauri::command]
pub fn get_install_ref(tool_id: String) -> Option<String> {
    bundled_assets::get_install_ref(&tool_id).map(|s| s.to_string())
}

#[tauri::command]
pub fn get_tool_script(name: String) -> Option<String> {
    bundled_assets::get_tool_script(&name).map(|s| s.to_string())
}
