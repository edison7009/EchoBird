// Bridge APIs — Plugin scanning, bridge CLI, remote agent management
import { invoke } from '@tauri-apps/api/core';


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

export async function bridgeChatLocal(message: string, sessionId?: string, systemPrompt?: string, roleName?: string): Promise<{ text: string; session_id?: string; model?: string; tokens?: number; duration_ms?: number }> {
    return invoke('bridge_chat_local', { message, sessionId: sessionId ?? null, systemPrompt: systemPrompt ?? null, roleName: roleName ?? null });
}

export async function bridgeChatRemote(serverId: string, message: string, sessionId?: string, pluginId?: string, roleId?: string): Promise<{ text: string; session_id?: string; model?: string; tokens?: number; duration_ms?: number }> {
    return invoke('bridge_chat_remote', { serverId, message, sessionId: sessionId ?? null, pluginId: pluginId ?? null, roleId: roleId ?? null });
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

export async function bridgeEnsureRemote(serverId: string): Promise<string> {
    return invoke('bridge_ensure_remote', { serverId });
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

export async function bridgeStopAgentRemote(serverId: string, agentId: string): Promise<unknown> {
    return invoke('bridge_stop_agent_remote', { serverId, agentId });
}

// Remote model read/write

export interface RemoteModelResult {
    modelId: string;
    modelName: string;
}

export async function bridgeGetRemoteModel(serverId: string, agentId: string): Promise<RemoteModelResult | null> {
    return invoke('bridge_get_remote_model', { serverId, agentId });
}

export async function bridgeSetRemoteModel(
    serverId: string, agentId: string,
    modelId: string, modelName: string,
    apiKey: string, baseUrl: string, apiType: string,
): Promise<{ success: boolean; message: string }> {
    return invoke('bridge_set_remote_model', { serverId, agentId, modelId, modelName, apiKey, baseUrl, apiType });
}

// Local model read/write (same pattern as remote but no SSH)

export async function bridgeGetLocalModel(agentId: string): Promise<RemoteModelResult | null> {
    return invoke('bridge_get_local_model', { agentId });
}

export async function bridgeSetLocalModel(
    agentId: string,
    modelId: string, modelName: string,
    apiKey: string, baseUrl: string, apiType: string,
): Promise<{ success: boolean; message: string }> {
    return invoke('bridge_set_local_model', { agentId, modelId, modelName, apiKey, baseUrl, apiType });
}

