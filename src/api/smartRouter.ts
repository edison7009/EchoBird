import { invoke } from '@tauri-apps/api/core';
import type { ModelConfig } from './types';

export interface SmartRouterConfig {
  id: string;
  name: string;
  icon?: string;
  masterEnabled: boolean;
  groupEnabled: boolean;
  enabled: boolean;
  candidateIds: string[];
  disabledCandidateIds?: string[];
  usableCandidateCount: number;
  baseUrl: string;
  modelId: string;
  port: number;
  running: boolean;
}

export interface SmartRouterActivity {
  candidateId: string | null;
  active: boolean;
  activeCandidateIds?: string[];
  sequence: number;
  updatedAtMs: number;
}

export interface SmartRouterProbeResult {
  latencyMs: number | null;
  error: string | null;
}

export async function testSmartRouterCandidate(
  internalId: string,
  modelId: string,
  requestId: string,
  batchId?: string,
  routerId?: string
): Promise<SmartRouterProbeResult> {
  return invoke('test_smart_router_candidate', {
    internalId,
    modelId,
    requestId,
    batchId,
    routerId,
  });
}

export async function cancelSmartRouterProbe(requestId: string): Promise<void> {
  return invoke('cancel_smart_router_probe', { requestId });
}

export async function getSmartRouterConfig(routerId?: string): Promise<SmartRouterConfig> {
  return invoke('get_smart_router_config', routerId ? { routerId } : undefined);
}

export async function setSmartRouterEnabled(enabled: boolean): Promise<SmartRouterConfig> {
  const result = await invoke<SmartRouterConfig>('set_smart_router_enabled', { enabled });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}

export async function setSmartRouterCandidateEnabled(
  internalId: string,
  enabled: boolean,
  routerId?: string
): Promise<SmartRouterConfig> {
  return invoke('set_smart_router_candidate_enabled', { internalId, enabled, routerId });
}

export async function getSmartRouterActivity(routerId?: string): Promise<SmartRouterActivity> {
  return invoke('get_smart_router_activity', routerId ? { routerId } : undefined);
}

export async function setSmartRouterCandidates(
  candidateIds: string[],
  routerId?: string
): Promise<SmartRouterConfig> {
  const result = await invoke<SmartRouterConfig>('set_smart_router_candidates', {
    candidateIds,
    routerId,
  });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}

export async function getSmartRouterCandidates(routerId?: string): Promise<ModelConfig[]> {
  return invoke('get_smart_router_candidates', routerId ? { routerId } : undefined);
}

export async function removeSmartRouterCandidate(
  internalId: string,
  routerId?: string
): Promise<SmartRouterConfig> {
  const result = await invoke<SmartRouterConfig>('remove_smart_router_candidate', {
    internalId,
    routerId,
  });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}

export interface SmartRouterSettings {
  id?: string;
  name: string;
  icon?: string;
  modelId: string;
  port: number;
  apiKey: string;
}

export function listSmartRouters(): Promise<SmartRouterConfig[]> {
  return invoke('list_smart_routers');
}

export function getSmartRouterSettings(routerId: string): Promise<SmartRouterSettings> {
  return invoke('get_smart_router_settings', { routerId });
}

export function getSmartRouterAvailablePort(excludePort?: number): Promise<number> {
  return invoke('get_smart_router_available_port', { excludePort });
}

export async function saveSmartRouter(settings: SmartRouterSettings): Promise<SmartRouterConfig> {
  const result = await invoke<SmartRouterConfig>('save_smart_router', { settings });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}

export async function deleteSmartRouter(routerId: string): Promise<void> {
  await invoke('delete_smart_router', { routerId });
  window.dispatchEvent(new Event('models-changed'));
}

export async function setSmartRouterGroupEnabled(
  routerId: string,
  enabled: boolean
): Promise<SmartRouterConfig> {
  const result = await invoke<SmartRouterConfig>('set_smart_router_group_enabled', {
    routerId,
    enabled,
  });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}
