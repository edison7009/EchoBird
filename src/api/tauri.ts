// Tauri IPC API layer — replaces window.electron.* calls
// All frontend↔backend communication goes through this module.
//
// Domain modules have been split out for better organisation.
// This file keeps common/misc functions and re-exports all domain modules
// so consumers can continue to use  `import * as api from '../api/tauri'`.

import { invoke } from '@tauri-apps/api/core';
import type { DetectedTool, ApplyModelInput, AppSettings } from './types';

// ─── Re-export domain modules ───

export * from './models';
export * from './localServer';
export * from './agent';
export * from './ssh';
export * from './secret';
export * from './bundled';

// ─── Tool APIs ───

export async function scanTools(): Promise<DetectedTool[]> {
  return invoke('scan_tools');
}

export async function applyModelToTool(
  toolId: string,
  modelInfo: ApplyModelInput
): Promise<{ success: boolean; message: string }> {
  return invoke('apply_model_to_tool', { toolId, modelInfo });
}

export async function restoreToolToOfficial(
  toolId: string
): Promise<{ success: boolean; message: string }> {
  return invoke('restore_tool_to_official', { toolId });
}

// ─── Process APIs ───

export async function startTool(toolId: string, startCommand?: string): Promise<void> {
  return invoke('start_tool', { toolId, startCommand: startCommand || null });
}

// ─── Shell APIs (uses Tauri shell plugin) ───

export async function openExternal(url: string): Promise<void> {
  const { open } = await import('@tauri-apps/plugin-shell');
  await open(url);
}

export async function openFolder(path: string): Promise<void> {
  await invoke('open_folder', { path });
}

// ─── App Settings APIs ───

export async function getSettings(): Promise<AppSettings> {
  return invoke('get_settings');
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invoke('save_settings', { settings });
}

// ─── App Lifecycle APIs ───

export async function appReady(): Promise<void> {
  return invoke('app_ready');
}

/// Pop the main webview's devtools panel — used by the "问题反馈 /
/// Feedback" page so users can copy the failing log tail and attach it
/// to a GitHub issue.
export async function openDevtools(): Promise<void> {
  return invoke('open_devtools');
}

// ─── Misc APIs ───

export async function launchGame(
  toolId: string,
  launchFile: string,
  modelConfig?: {
    baseUrl?: string;
    anthropicUrl?: string;
    apiKey?: string;
    model?: string;
    name?: string;
    protocol?: string;
  }
): Promise<{ success: boolean; message?: string }> {
  return invoke('launch_game', { toolId, launchFile, modelConfig: modelConfig || null });
}

// ─── Window APIs (Tauri built-in) ───

export { getCurrentWindow } from '@tauri-apps/api/window';
