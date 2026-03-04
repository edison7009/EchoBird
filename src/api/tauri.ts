// Tauri IPC API layer — replaces window.electron.* calls
// All frontend↔backend communication goes through this module.

import { invoke } from '@tauri-apps/api/core';
import type {
    DetectedTool, ModelConfig, ModelTestResult, PingResult,
    ToggleEncryptionResult, SSNodeConfig, ProxyRule,
    LocalServerInfo, GgufFile, HfModelEntry, ModelSettings,
    ToolModelInfo, ApplyModelInput, AppLogEntry,
    ChannelConfig, AppSettings, StoreModel,
    AgentRequest, AgentEvent, AgentStatusResponse,
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

export async function getToolInstalledSkills(toolId: string): Promise<Array<{ id: string; name: string; path: string }>> {
    return invoke('get_tool_installed_skills', { toolId });
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
    return invoke('add_model', { input });
}

export async function deleteModel(internalId: string): Promise<boolean> {
    return invoke('delete_model', { internalId });
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
    return invoke('update_model', { internalId, updates });
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

export async function setDownloadDir(): Promise<string> {
    return invoke('set_download_dir');
}

export async function getStoreModels(): Promise<StoreModel[]> {
    return invoke('get_store_models');
}

export async function downloadModel(repo: string, fileName: string): Promise<string> {
    return invoke('download_model', { repo, fileName });
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
    status: 'downloading' | 'completed' | 'error' | 'cancelled' | 'paused' | 'speed_test';
}

export function onDownloadProgress(callback: (data: DownloadProgressEvent) => void): Promise<UnlistenFn> {
    return listen<DownloadProgressEvent>('download-progress', (event) => {
        callback(event.payload);
    });
}

// ─── Skill Registry APIs ───

/** Single skill entry from claude-skill-registry search-index.json */
export interface RegistrySkill {
    n: string;   // name
    d: string;   // description
    c: string;   // category code
    g: string[]; // tags
    r: number;   // GitHub stars
    i: string;   // GitHub path (e.g. "owner/repo/.claude/skills/name/SKILL.md")
    b: string;   // branch
}

/** Local cache format for skills data */
export interface SkillsData {
    skills: RegistrySkill[];
    userCategories?: string[];
    sources?: string[];
    lastUpdated?: string;
}

/** User favorites stored separately in skills_favorites.json */
export interface SkillsFavorites {
    favorites: string[];
}

export async function loadSkillsData(): Promise<SkillsData> {
    return invoke('load_skills_data');
}

export async function saveSkillsData(data: SkillsData): Promise<void> {
    return invoke('save_skills_data', { data });
}

export async function loadSkillsFavorites(): Promise<SkillsFavorites> {
    return invoke('load_skills_favorites');
}

export async function saveSkillsFavorites(data: SkillsFavorites): Promise<void> {
    return invoke('save_skills_favorites', { data });
}

export async function fetchSkillSource(url: string): Promise<string> {
    return invoke('fetch_skill_source', { url });
}

export interface LlmQuickConfig {
    provider: string;
    base_url: string;
    api_key: string;
    model: string;
    proxy_url?: string;
}

export async function llmQuickChat(config: LlmQuickConfig, prompt: string): Promise<string> {
    return invoke('llm_quick_chat', { config, prompt });
}

export interface SkillI18nEntry {
    n?: string;
    d?: string;
    expanded_d?: string;
    content?: string;
    locale: string;
}

export type SkillsI18nMap = Record<string, SkillI18nEntry>;

export async function loadSkillsI18n(): Promise<SkillsI18nMap> {
    return invoke('load_skills_i18n');
}

export async function saveSkillsI18n(data: SkillsI18nMap): Promise<void> {
    return invoke('save_skills_i18n', { data });
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

// ─── Tool Config APIs ───

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

export async function getAgentStatus(serverKey: string): Promise<AgentStatusResponse> {
    return invoke('agent_status', { serverKey });
}

export function listenAgentEvents(handler: (event: AgentEvent) => void): Promise<UnlistenFn> {
    return listen<AgentEvent>('agent_event', (e) => handler(e.payload));
}

export async function loadAgentHistory(serverKey: string): Promise<Array<{ role: string; text: string }>> {
    return invoke('load_agent_history', { serverKey });
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
