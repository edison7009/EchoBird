// Tauri IPC API layer — replaces window.electron.* calls
// All frontend↔backend communication goes through this module.

import { invoke } from '@tauri-apps/api/core';
import type {
    DetectedTool, ModelConfig, ModelTestResult, PingResult,
    ToggleEncryptionResult, SSNodeConfig, ProxyRule,
    LocalServerInfo, GgufFile, HfModelEntry, ModelSettings,
    ToolModelInfo, ApplyModelInput, AppLogEntry,
    ChannelConfig, AppSettings, StoreModel,
    AgentRequest, AgentEvent,
} from './types';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

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


// ─── Model APIs ───

export async function getModels(): Promise<ModelConfig[]> {
    return invoke('get_models');
}

export async function addModel(input: {
    name: string;
    baseUrl: string;
    apiKey: string;
    anthropicUrl?: string;
    modelId?: string;
    proxyUrl?: string;
    ssNode?: SSNodeConfig;
}): Promise<ModelConfig> {
    const result = await invoke<ModelConfig>('add_model', { input });
    window.dispatchEvent(new Event('models-changed'));
    return result;
}

export async function deleteModel(internalId: string): Promise<boolean> {
    const result = await invoke<boolean>('delete_model', { internalId });
    window.dispatchEvent(new Event('models-changed'));
    return result;
}

export async function updateModel(internalId: string, updates: {
    name?: string;
    baseUrl?: string;
    apiKey?: string;
    anthropicUrl?: string;
    modelId?: string;
    proxyUrl?: string;
    ssNode?: SSNodeConfig;
}): Promise<ModelConfig | null> {
    const result = await invoke<ModelConfig | null>('update_model', { internalId, updates });
    window.dispatchEvent(new Event('models-changed'));
    return result;
}

export async function testModel(internalId: string, prompt: string, protocol: string = 'openai'): Promise<ModelTestResult> {
    return invoke('test_model', { internalId, prompt, protocol });
}

export async function pingModel(internalId: string): Promise<PingResult> {
    return invoke('ping_model', { internalId });
}

export async function toggleKeyEncryption(internalId: string): Promise<ToggleEncryptionResult> {
    return invoke('toggle_key_encryption', { internalId });
}

export async function isKeyDestroyed(internalId: string): Promise<boolean> {
    return invoke('is_key_destroyed', { internalId });
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

// ─── Local LLM APIs ───

export async function startLlmServer(modelPath: string, port: number, gpuLayers?: number, contextSize?: number, runtime?: string): Promise<void> {
    return invoke('start_llm_server', { modelPath, port, gpuLayers: gpuLayers ?? null, contextSize: contextSize ?? null, runtime: runtime ?? null });
}

export async function stopLlmServer(): Promise<void> {
    return invoke('stop_llm_server');
}

export async function getLlmServerInfo(): Promise<LocalServerInfo> {
    return invoke('get_llm_server_info');
}

export async function getLlmServerLogs(): Promise<string[]> {
    return invoke('get_llm_server_logs');
}

export async function findLlamaServer(): Promise<string | null> {
    return invoke('find_llama_server');
}

export async function downloadLlamaServer(): Promise<string> {
    return invoke('download_llama_server');
}

export async function getModelsDirs(): Promise<string[]> {
    return invoke('get_models_dirs');
}

export async function getDownloadDir(): Promise<string> {
    return invoke('get_download_dir');
}

export async function loadModelSettings(): Promise<ModelSettings> {
    return invoke('load_model_settings');
}

export async function saveModelSettings(settings: ModelSettings): Promise<void> {
    return invoke('save_model_settings', { settings });
}

export async function scanGgufFiles(dir: string): Promise<GgufFile[]> {
    return invoke('scan_gguf_files', { dir });
}

export async function scanHfModels(dir: string): Promise<HfModelEntry[]> {
    return invoke('scan_hf_models', { dir });
}

export async function addModelsDir(): Promise<string[]> {
    return invoke('add_models_dir');
}

export async function removeModelsDir(dir: string): Promise<string[]> {
    return invoke('remove_models_dir', { dir });
}

export async function detectGpu(): Promise<{ gpuName: string; gpuVramGb: number } | null> {
    return invoke('detect_gpu');
}

export async function getGpuInfo(): Promise<{ gpuName: string; gpuVramGb: number } | null> {
    return invoke('get_gpu_info');
}

export interface SystemInfo {
    os: string;         // "windows" | "macos" | "linux"
    arch: string;       // "x86_64" | "aarch64"
    hasNvidiaGpu: boolean;
    hasAmdGpu?: boolean;
    gpuName: string | null;
    gpuVramGb: number | null;
}

export async function getSystemInfo(): Promise<SystemInfo> {
    return invoke('get_system_info');
}

export async function setDownloadDir(): Promise<string> {
    return invoke('set_download_dir');
}

export async function getStoreModels(): Promise<StoreModel[]> {
    return invoke('get_store_models');
}

export async function downloadModel(repo: string, fileName: string): Promise<string> {
    return invoke('download_model', { repo, fileName });
}

export interface LocalEngineEntry {
    name: string;
    installed: boolean;
    version: string;
}

export interface LocalEngineStatus {
    engines: LocalEngineEntry[];
}

export async function getLocalEngineStatus(): Promise<LocalEngineStatus> {
    return invoke('get_local_engine_status');
}

export async function installLocalEngine(runtime: string): Promise<void> {
    return invoke('install_local_engine', { runtime });
}

export async function pauseDownload(): Promise<void> {
    return invoke('pause_download');
}

export async function cancelDownload(fileName?: string): Promise<void> {
    return invoke('cancel_download', { fileName: fileName || null });
}

// Download progress event listener

export interface DownloadProgressEvent {
    fileName: string;
    progress: number;
    downloaded: number;
    total: number;
    status: 'downloading' | 'completed' | 'error' | 'cancelled' | 'paused' | 'speed_test' | 'installing';
}

export function onDownloadProgress(callback: (data: DownloadProgressEvent) => void): Promise<UnlistenFn> {
    return listen<DownloadProgressEvent>('download-progress', (event) => {
        callback(event.payload);
    });
}


// ─── SSH APIs ───

export interface SSHConnectResult {
    success: boolean;
    message: string;
}

export interface SSHExecResult {
    stdout: string;
    stderr: string;
    exitCode: number;
    success: boolean;
}

export async function sshConnect(id: string, host: string, port: number, username: string, password: string): Promise<SSHConnectResult> {
    return invoke('ssh_connect', { id, host, port, username, password });
}

export async function sshExecute(id: string, command: string): Promise<SSHExecResult> {
    return invoke('ssh_execute', { id, command });
}

export async function sshDisconnect(id: string): Promise<boolean> {
    return invoke('ssh_disconnect', { id });
}

export async function sshTestConnection(host: string, port: number, username: string, password: string): Promise<SSHConnectResult> {
    return invoke('ssh_test_connection', { host, port, username, password });
}

// SSH server persistence (encrypted storage)
export interface SSHServer {
    id: string;
    host: string;
    port: number;
    username: string;
    password: string; // encrypted (enc:v1:...)
    alias?: string;   // user-defined display name
}

export async function loadSSHServers(): Promise<SSHServer[]> {
    return invoke('load_ssh_servers');
}

export async function saveSSHServer(id: string, host: string, port: number, username: string, password: string, alias?: string): Promise<SSHServer> {
    return invoke('save_ssh_server', { id, host, port, username, password, alias: alias ?? null });
}

export async function removeSSHServerFromDisk(id: string): Promise<boolean> {
    return invoke('remove_ssh_server', { id });
}

export async function updateSSHAlias(id: string, alias: string): Promise<boolean> {
    return invoke('update_ssh_alias', { id, alias });
}

export async function decryptSSHPassword(encrypted: string): Promise<string> {
    return invoke('decrypt_ssh_password', { encrypted });
}

export async function encryptSSHPassword(plaintext: string): Promise<string> {
    return invoke('encrypt_ssh_password', { plaintext });
}

export async function sshUploadFile(id: string, localPath: string, remotePath: string): Promise<SSHExecResult> {
    return invoke('ssh_upload_file', { id, localPath, remotePath });
}

// ─── Plugin APIs ───

export interface PluginConfig {
    id: string;
    name: string;
    protocol: string;
    bridge?: { linux?: string; darwin?: string; win32?: string };
    cli?: {
        command: string;
        detectCommand?: string;
        args: string[];
        resumeArgs?: string[];
        sessionArg?: string;
        sessionMode?: string;
        modelArg?: string;
        systemPromptArg?: string;
        systemPromptWhen?: string;
    };
}

export async function scanPlugins(): Promise<PluginConfig[]> {
    return invoke('scan_plugins');
}

export async function getBridgePath(pluginId: string): Promise<string> {
    return invoke('get_bridge_path', { pluginId });
}

export async function bridgeStart(pluginId?: string): Promise<{ status: string; error?: string; agentName?: string }> {
    return invoke('bridge_start', { pluginId });
}

export async function bridgeStop(): Promise<void> {
    return invoke('bridge_stop');
}

export async function bridgeStatus(): Promise<{ status: string; agentName?: string }> {
    return invoke('bridge_status');
}

export async function bridgeChatLocal(message: string, sessionId?: string, systemPrompt?: string): Promise<{ text: string; session_id?: string; model?: string; tokens?: number; duration_ms?: number }> {
    return invoke('bridge_chat_local', { message, sessionId: sessionId ?? null, systemPrompt: systemPrompt ?? null });
}

export async function bridgeChatRemote(serverId: string, message: string, sessionId?: string, pluginId?: string): Promise<{ text: string; session_id?: string; model?: string; tokens?: number; duration_ms?: number }> {
    return invoke('bridge_chat_remote', { serverId, message, sessionId: sessionId ?? null, pluginId: pluginId ?? null });
}

// ─── Remote Bridge CLI Commands ───

export interface RemoteAgentInfo {
    id: string;
    name: string;
    installed: boolean;
    running: boolean;
    path?: string;
}

export async function bridgeDetectAgentsRemote(serverId: string): Promise<RemoteAgentInfo[]> {
    return invoke('bridge_detect_agents_remote', { serverId });
}

export async function bridgeSetRoleLocal(agentId: string, roleId: string, url: string): Promise<unknown> {
    return invoke('bridge_set_role_local', { agentId, roleId, url });
}

export async function bridgeSetRoleRemote(serverId: string, agentId: string, roleId: string, url: string): Promise<unknown> {
    return invoke('bridge_set_role_remote', { serverId, agentId, roleId, url });
}

export async function bridgeClearRoleRemote(serverId: string, agentId: string, roleId: string): Promise<unknown> {
    return invoke('bridge_clear_role_remote', { serverId, agentId, roleId });
}

export async function bridgeStartAgentRemote(serverId: string, agentId: string): Promise<unknown> {
    return invoke('bridge_start_agent_remote', { serverId, agentId });
}

// ─── Role APIs ───

export interface RoleCategory {
    id: string;
    name: string;
    label?: string; // alias for backward compat
    order?: number;
}

export interface RoleEntry {
    id: string;
    name: string;
    description: string;
    category: string;
    filePath: string;
    img?: string;
    fallbackImg?: string;
}

export interface RoleScanResult {
    categories: RoleCategory[];
    roles: RoleEntry[];
    locale: string;
    allLabel: string;
}

const ROLES_CDN_BASE = 'https://echobird.ai/roles';
const ROLES_CACHE_KEY = 'eb_roles_cache';

function resolveLocaleFileName(locale: string): string {
    if (locale.startsWith('zh')) return 'roles-zh-Hans.json';
    // Future: add more languages here (e.g. ja, ko, fr, de)
    return 'roles-en.json';
}

async function fetchRolesFromCDN(fileName: string): Promise<{ categories: RoleCategory[]; roles: RoleEntry[] } | null> {
    try {
        const resp = await fetch(`${ROLES_CDN_BASE}/${fileName}`, { cache: 'no-cache' });
        if (!resp.ok) return null;
        return await resp.json();
    } catch {
        return null;
    }
}

export async function scanRoles(locale: string): Promise<RoleScanResult> {
    const localeFile = resolveLocaleFileName(locale);
    const enFile = 'roles-en.json';
    const cacheKey = `${ROLES_CACHE_KEY}_${localeFile}`;
    const isZh = locale.startsWith('zh');

    // Try user's language first
    let data = await fetchRolesFromCDN(localeFile);

    // Fallback to English if user's language failed and it's not already English
    if (!data && localeFile !== enFile) {
        data = await fetchRolesFromCDN(enFile);
    }

    // On success: cache to localStorage
    if (data) {
        try { localStorage.setItem(cacheKey, JSON.stringify(data)); } catch { /* quota exceeded */ }
    } else {
        // All CDN failed: try localStorage cache
        try {
            const cached = localStorage.getItem(cacheKey);
            if (cached) data = JSON.parse(cached);
        } catch { /* corrupted */ }
    }

    if (!data) {
        // Final fallback: empty result
        return { categories: [], roles: [], locale: isZh ? 'zh-Hans' : 'en', allLabel: isZh ? '\u5168\u90e8' : 'All' };
    }

    // Map category.name → label for backward compat
    const categories: RoleCategory[] = (data.categories || []).map((c: RoleCategory, i: number) => ({
        ...c,
        label: c.label || c.name,
        order: c.order ?? i,
    }));

    return {
        categories,
        roles: data.roles || [],
        locale: isZh ? 'zh-Hans' : 'en',
        allLabel: isZh ? '\u5168\u90e8' : 'All',
    };
}

export interface AgentStatus {
    id: string;
    name: string;
    installed: boolean;
    path?: string;
}

export async function detectLocalAgents(): Promise<AgentStatus[]> {
    return invoke('detect_local_agents');
}

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

// ─── Channel APIs ───

export async function getChannels(): Promise<ChannelConfig[]> {
    return invoke('get_channels');
}

export async function saveChannels(channels: ChannelConfig[]): Promise<void> {
    return invoke('save_channels', { channels });
}

// ─── Channel Chat History APIs ───

export interface ChannelHistoryMessage {
    role: string;    // "user" | "assistant" | "system"
    content: string;
}

export interface ChannelHistoryResponse {
    messages: ChannelHistoryMessage[];
    total: number;
}

/** Load a paginated slice. offset=0 → newest batch, offset=30 → next older. */
export async function channelHistoryLoad(channelKey: string, offset: number, limit: number): Promise<ChannelHistoryResponse> {
    return invoke('channel_history_load', { channelKey, offset, limit });
}

/** Save full message list (replaces file). Call debounced after each new message. */
export async function channelHistorySave(channelKey: string, messages: ChannelHistoryMessage[]): Promise<void> {
    return invoke('channel_history_save', { channelKey, messages });
}

/** Delete the channel history file. */
export async function channelHistoryClear(channelKey: string): Promise<void> {
    return invoke('channel_history_clear', { channelKey });
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

// ─── Agent APIs ───

export async function sendAgentMessage(request: AgentRequest): Promise<string> {
    return invoke('agent_send_message', { request });
}

export async function abortAgent(serverKey: string): Promise<boolean> {
    return invoke('agent_abort', { serverKey });
}

export async function resetAgent(serverKey: string): Promise<string> {
    return invoke('agent_reset', { serverKey });
}

export function listenAgentEvents(handler: (event: AgentEvent) => void): Promise<UnlistenFn> {
    return listen<AgentEvent>('agent_event', (e) => handler(e.payload));
}

// ─── Channel Config ───

export async function getChannelConfig(): Promise<ChannelConfig[]> {
    return invoke('get_channels');
}

export async function saveChannelConfig(channels: ChannelConfig[]): Promise<void> {
    return invoke('save_channels', { channels });
}

// ─── Window APIs (Tauri built-in) ───

export { getCurrentWindow } from '@tauri-apps/api/window';
