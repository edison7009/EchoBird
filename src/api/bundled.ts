// Bundled-asset APIs — read Mother Agent hints and install index from the
// compile-time embedded copy so smart-install works offline.
import { invoke } from '@tauri-apps/api/core';

export async function getMotherHints(): Promise<string> {
  return invoke('get_mother_hints');
}

export async function getInstallIndex(): Promise<string> {
  return invoke('get_install_index');
}
