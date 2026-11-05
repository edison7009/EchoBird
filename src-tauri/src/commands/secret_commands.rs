// Generic secret encryption commands — AES-256-GCM via the `enc:v1:` envelope.
// Reuses model_manager's key derivation. Used by both ModelNexus (API keys) and
// MotherAgent (SSH passwords).

use crate::services::secret_store;

/// Decrypt an `enc:v1:` ciphertext back to plaintext.
#[tauri::command]
pub async fn decrypt_secret(encrypted: String) -> Result<String, String> {
    secret_store::decrypt(&encrypted)
}

/// Encrypt a plaintext secret. No-op if already encrypted or empty.
#[tauri::command]
pub async fn encrypt_secret(plaintext: String) -> Result<String, String> {
    if plaintext.is_empty() || plaintext.starts_with("enc:v1:") {
        return Ok(plaintext);
    }
    secret_store::encrypt(&plaintext)
}
