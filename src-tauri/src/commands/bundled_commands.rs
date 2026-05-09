// Tauri IPC commands that expose the compile-time bundled install/script
// assets to the frontend. Lets the AppManager and Mother Agent UIs work
// offline — no fetch to echobird.ai.

use crate::services::bundled_assets;

#[tauri::command]
pub fn get_mother_hints() -> String {
    bundled_assets::MOTHER_HINTS_JSON.to_string()
}

#[tauri::command]
pub fn get_install_index() -> String {
    bundled_assets::INSTALL_INDEX_JSON.to_string()
}
