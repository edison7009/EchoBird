// Model APIs — CRUD, test, ping, encryption
import { invoke } from '@tauri-apps/api/core';
import type { ModelConfig, ModelTestResult, PingResult } from './types';

export async function getModels(): Promise<ModelConfig[]> {
  return invoke('get_models');
}

export async function addModel(input: {
  name: string;
  baseUrl: string;
  apiKey: string;
  anthropicUrl?: string;
  modelId?: string;
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

export async function updateModel(
  internalId: string,
  updates: {
    name?: string;
    baseUrl?: string;
    apiKey?: string;
    anthropicUrl?: string;
    modelId?: string;
  }
): Promise<ModelConfig | null> {
  const result = await invoke<ModelConfig | null>('update_model', { internalId, updates });
  window.dispatchEvent(new Event('models-changed'));
  return result;
}

export async function testModel(
  internalId: string,
  prompt: string,
  protocol: string = 'openai'
): Promise<ModelTestResult> {
  return invoke('test_model', { internalId, prompt, protocol });
}

export async function pingModel(internalId: string): Promise<PingResult> {
  return invoke('ping_model', { internalId });
}

export async function isKeyDestroyed(internalId: string): Promise<boolean> {
  return invoke('is_key_destroyed', { internalId });
}

/** Shape of one entry in the Model Center right-panel Providers/Relays list. */
export interface ModelDirectoryEntry {
  name: string;
  url: string;
  baseUrl: string;
  anthropicUrl: string;
  modelId: string;
  region: 'cn' | 'global';
}

/** Full directory payload — what `getModelDirectory` resolves to on success. */
export interface ModelDirectory {
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
