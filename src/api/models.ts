import { modelRepository } from '../stores/modelRepository';
// Model APIs — CRUD, test, ping, encryption
import { invoke } from '@tauri-apps/api/core';
import type { AccountSource, ModelConfig, ModelTestResult, PingResult } from './types';
import type { VoiceDirectoryEntry } from './voiceInput';

// Scoped consumers such as Voice Input retain their own query projections.
function notifyScopedModels() {
  if (typeof window !== 'undefined') {
    window.dispatchEvent(
      new CustomEvent('models-changed', { detail: { repositoryUpdated: true } })
    );
  }
}

export async function getModels(): Promise<ModelConfig[]> {
  return invoke('get_models');
}

export interface UpstreamModelCatalogInput {
  baseUrl: string;
  anthropicUrl: string;
  apiKey: string;
}

export interface UpstreamModelInfo {
  id: string;
  free?: boolean;
  multiplier?: number;
}

export interface UpstreamModelCatalog {
  models: UpstreamModelInfo[];
  requiresApiKey: boolean;
}

export function fetchUpstreamModelCatalog(
  input: UpstreamModelCatalogInput
): Promise<UpstreamModelCatalog> {
  return invoke('fetch_upstream_model_catalog', { input });
}

export function getCachedUpstreamModelCatalog(
  input: UpstreamModelCatalogInput
): Promise<UpstreamModelCatalog | null> {
  return invoke('get_cached_upstream_model_catalog', { input });
}

export async function addModel(input: {
  name: string;
  baseUrl: string;
  apiKey: string;
  accountSource?: AccountSource;
  anthropicUrl?: string;
  modelId?: string;
  scope?: 'modelCenter' | 'smartRouter' | 'voiceInput';
}): Promise<ModelConfig> {
  const result = await invoke<ModelConfig>('add_model', { input });
  modelRepository.ingest([result]);
  modelRepository.invalidate();
  notifyScopedModels();
  return result;
}

export async function deleteModel(internalId: string): Promise<boolean> {
  const result = await invoke<boolean>('delete_model', { internalId });
  if (result) modelRepository.remove(internalId);
  modelRepository.invalidate();
  notifyScopedModels();
  return result;
}

export async function updateModel(
  internalId: string,
  updates: {
    name?: string;
    baseUrl?: string;
    apiKey?: string;
    accountSource?: AccountSource;
    anthropicUrl?: string;
    modelId?: string;
  }
): Promise<ModelConfig | null> {
  const result = await invoke<ModelConfig | null>('update_model', { internalId, updates });
  if (result) modelRepository.ingest([result]);
  modelRepository.invalidate();
  notifyScopedModels();
  return result;
}

export async function testModel(
  internalId: string,
  prompt: string,
  protocol: string = 'openai'
): Promise<ModelTestResult> {
  return invoke('test_model', { internalId, prompt, protocol });
}

/** Persist a user-defined model display order (full visible list of
 *  internal_ids in the new order). */
export async function reorderModels(orderedIds: string[]): Promise<boolean> {
  return modelRepository.reorder(orderedIds, () => invoke('reorder_models', { orderedIds }));
}

export async function pingModel(internalId: string, batchId?: string): Promise<PingResult> {
  return invoke('ping_model', { internalId, batchId });
}

export async function finishConnectionTestBatch(batchId: string): Promise<void> {
  return invoke('finish_connection_test_batch', { batchId });
}

export async function isKeyDestroyed(internalId: string): Promise<boolean> {
  return invoke('is_key_destroyed', { internalId });
}

export interface UsageQuota {
  // Used percentage. Period-based quotas are displayed as remaining allowance.
  percentage: number;
  resetAt: number;
  period?: 'fiveHour' | 'daily' | 'weekly' | 'monthly';
  balance?: number;
  balanceUnit?: string;
  // The amount is what was spent, for keys that have no balance to report.
  spent?: boolean;
}

export interface ModelUsageData {
  quotas: UsageQuota[];
  lastUpdated?: number;
}

export interface UsageResult {
  success: boolean;
  data?: ModelUsageData;
  error?: string;
}

export async function queryModelUsage(internalId: string): Promise<UsageResult> {
  return invoke('query_model_usage', { internalId });
}

export interface ZhipuTeamAccess {
  organizationId: string;
  projectId: string;
}

export function getZhipuTeamAccess(internalId: string): Promise<ZhipuTeamAccess | null> {
  return invoke('get_zhipu_team_access', { internalId });
}

export function saveZhipuTeamAccess(
  internalId: string,
  access: ZhipuTeamAccess | null
): Promise<void> {
  return invoke('save_zhipu_team_access', { internalId, access });
}

/** Save Volcengine IAM AK/SK (encrypted on the backend) for a specific model. */
export function saveVolcAksk(
  internalId: string,
  accessKey: string,
  secretKey: string
): Promise<void> {
  return invoke('save_volc_aksk', { internalId, accessKey, secretKey });
}

/** Whether Volcengine AK/SK are stored for a specific model. */
export function hasVolcAksk(internalId: string): Promise<boolean> {
  return invoke('has_volc_aksk', { internalId });
}

/** Read stored AK/SK (plaintext) for a model, to pre-fill the config modal. */
export function getVolcAksk(
  internalId: string
): Promise<{ access_key: string; secret_key: string } | null> {
  return invoke('get_volc_aksk', { internalId });
}

/** Shape of one entry in the Model Center right-panel Providers/Relays list. */
export interface ModelDirectoryEntry {
  name: string;
  url: string;
  baseUrl: string;
  anthropicUrl: string;
  // Presets are needed only when an endpoint does not offer model discovery.
  modelId?: string;
  modelIds?: string[];
  region: 'cn' | 'global';
  // Editions of one company share a group and appear as one list item.
  group?: string;
  // Short tab label for an edition; defaults to the entry name.
  edition?: string;
}

/** Full directory payload — what `getModelDirectory` resolves to on success. */
export interface ModelDirectory {
  speechProviders?: VoiceDirectoryEntry[];
  providers: ModelDirectoryEntry[];
  relays: ModelDirectoryEntry[];
}

/**
 * Fetch the Model Center directory (Providers + Relays lists) from the
 * remote-first backend pipeline. Returns null when both the remote and
 * the on-disk cache are unavailable — caller is expected to fall back
 * to the bundled `src/data/modelDirectory.json` shipped with the app.
 *
 * Mirrors `getStoreModels` in spirit: remote drives content updates
 * without a release, while bundled JSON keeps the UI populated when
 * the user is offline / behind a strict firewall.
 */
export async function getModelDirectory(): Promise<ModelDirectory | null> {
  const result = await invoke<ModelDirectory | null>('get_model_directory');
  if (
    result &&
    Array.isArray((result as ModelDirectory).providers) &&
    Array.isArray((result as ModelDirectory).relays)
  ) {
    return result;
  }
  return null;
}
