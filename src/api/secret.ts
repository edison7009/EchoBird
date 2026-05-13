// Generic secret encryption — AES-256-GCM via the `enc:v1:` envelope.
// Used by ModelNexus (API keys) and MotherAgent (SSH passwords).
import { invoke } from '@tauri-apps/api/core';

export async function decryptSecret(encrypted: string): Promise<string> {
  return invoke('decrypt_secret', { encrypted });
}

export async function encryptSecret(plaintext: string): Promise<string> {
  return invoke('encrypt_secret', { plaintext });
}
