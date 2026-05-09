// Model APIs — CRUD, test, ping, encryption
import { invoke } from '@tauri-apps/api/core';
import type {
    ModelConfig, ModelTestResult, PingResult,
} from './types';


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

export async function updateModel(internalId: string, updates: {
    name?: string;
    baseUrl?: string;
    apiKey?: string;
    anthropicUrl?: string;
    modelId?: string;
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

export async function isKeyDestroyed(internalId: string): Promise<boolean> {
    return invoke('is_key_destroyed', { internalId });
}
