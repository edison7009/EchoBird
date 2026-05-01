// Tauri IPC API layer — replaces window.electron.* calls
// All frontend↔backend communication goes through this module.
//
// Domain modules have been split out for better organisation.
// This file keeps common/misc functions and re-exports all domain modules
// so consumers can continue to use  `import * as api from '../api/tauri'`.

import { invoke } from '@tauri-apps/api/core';
import type {
    DetectedTool, ToolModelInfo, ApplyModelInput,
    ProxyRule, SSNodeConfig, AppLogEntry,
    AppSettings,
} from './types';

// ─── Re-export domain modules ───

export * from './models';
export * from './localServer';
export * from './agent';
export * from './ssh';
export * from './roles';
export * from './bundled';

// ─── Tool APIs ───

export async function scanTools(): Promise<DetectedTool[]> {
    return invoke('scan_tools');
}

export async function getToolModelInfo(toolId: string): Promise<ToolModelInfo | null> {
    return invoke('get_tool_model_info', { toolId });
}

export async function applyModelToTool(toolId: string, modelInfo: ApplyModelInput): Promise<{ success: boolean; message: string }> {
    return invoke('apply_model_to_tool', { toolId, modelInfo });
}

export async function restoreToolToOfficial(toolId: string): Promise<{ success: boolean; message: string }> {
    return invoke('restore_tool_to_official', { toolId });
}

// ─── Proxy APIs ───

export async function startProxy(config?: SSNodeConfig): Promise<number> {
    return invoke('start_proxy', { config: config || null });
}

export async function stopProxy(): Promise<void> {
    return invoke('stop_proxy');
}

export async function getProxyPort(): Promise<number> {
    return invoke('get_proxy_port');
}

export async function getProxyRules(): Promise<ProxyRule[]> {
    return invoke('get_proxy_rules');
}

export async function saveProxyRules(rules: ProxyRule[]): Promise<void> {
    return invoke('save_proxy_rules', { rules });
}

export async function addProxyHostRule(hostname: string, ssNode: SSNodeConfig): Promise<void> {
    return invoke('add_proxy_host_rule', { hostname, ssNode });
}

export async function clearProxyHostRules(): Promise<void> {
    return invoke('clear_proxy_host_rules');
}

export async function parseSsUrl(url: string): Promise<SSNodeConfig | null> {
    return invoke('parse_ss_url', { url });
}

// ─── Process APIs ───

export async function startTool(toolId: string, startCommand?: string): Promise<void> {
    return invoke('start_tool', { toolId, startCommand: startCommand || null });
}

export async function stopTool(toolId: string): Promise<void> {
    return invoke('stop_tool', { toolId });
}

export async function getRunningTools(): Promise<string[]> {
    return invoke('get_running_tools');
}

export async function isToolRunning(toolId: string): Promise<boolean> {
    return invoke('is_tool_running', { toolId });
}

// ─── Shell APIs (uses Tauri shell plugin) ───

export async function openExternal(url: string): Promise<void> {
    const { open } = await import('@tauri-apps/plugin-shell');
    await open(url);
}

export async function openFolder(path: string): Promise<void> {
    await invoke('open_folder', { path });
}

// ─── App Log APIs ───

export async function getAppLogs(): Promise<AppLogEntry[]> {
    return invoke('get_app_logs');
}

export async function clearAppLogs(): Promise<void> {
    return invoke('clear_app_logs');
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

export async function setLocale(locale: string): Promise<void> {
    return invoke('set_locale', { locale });
}

export async function quitApp(): Promise<void> {
    return invoke('quit_app');
}

// ─── Misc APIs ───

export async function updateToolConfig(toolId: string, config: Record<string, unknown>): Promise<boolean> {
    return invoke('update_tool_config', { toolId, config });
}

export async function launchGame(toolId: string, launchFile: string, modelConfig?: {
    baseUrl?: string;
    anthropicUrl?: string;
    apiKey?: string;
    model?: string;
    name?: string;
    protocol?: string;
}): Promise<{ success: boolean; message?: string }> {
    return invoke('launch_game', { toolId, launchFile, modelConfig: modelConfig || null });
}

// ─── SS Proxy APIs ───

export async function addSSProxyRoute(modelId: string, targetUrl: string, ssNode: SSNodeConfig): Promise<{ success: boolean; proxyUrl?: string }> {
    return invoke('add_ss_proxy_route', { modelId, targetUrl, ssNode });
}

// ─── Window APIs (Tauri built-in) ───

export { getCurrentWindow } from '@tauri-apps/api/window';
