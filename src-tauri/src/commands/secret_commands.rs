// Generic secret encryption commands — AES-256-GCM via the `enc:v1:` envelope.
// Reuses model_manager's key derivation. Used by both ModelNexus (API keys) and
// MotherAgent (SSH passwords).

use crate::services::model_manager;

/// Decrypt an `enc:v1:` ciphertext back to plaintext.
#[tauri::command]
pub async fn decrypt_secret(encrypted: String) -> Result<String, String> {
    Ok(model_manager::decrypt_key_for_use(&encrypted))
}

/// Encrypt a plaintext secret. No-op if already encrypted or empty.
#[tauri::command]
pub async fn encrypt_secret(plaintext: String) -> Result<String, String> {
    if plaintext.is_empty() || plaintext.starts_with("enc:v1:") {
        return Ok(plaintext);
    }
    Ok(model_manager::encrypt_key_for_storage(&plaintext))
}
