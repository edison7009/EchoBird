// Bundled-asset APIs — read install references and Mother Agent prompts/hints
// from the compile-time embedded copy so smart-install works offline.
import { invoke } from '@tauri-apps/api/core';

export async function getMotherSystemPrompt(): Promise<string> {
    return invoke('get_mother_system_prompt');
}

export async function getMotherHints(): Promise<string> {
    return invoke('get_mother_hints');
}

export async function getInstallIndex(): Promise<string> {
    return invoke('get_install_index');
}

export async function getInstallRef(toolId: string): Promise<string | null> {
    return invoke('get_install_ref', { toolId });
}

export async function getToolScript(name: string): Promise<string | null> {
    return invoke('get_tool_script', { name });
}
