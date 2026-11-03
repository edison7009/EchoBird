import { invoke } from '@tauri-apps/api/core';
import type { ModelConfig } from './types';

export type VoiceApiProtocol =
  | 'openai'
  | 'chatAudio'
  | 'mimo'
  | 'openrouter'
  | 'gemini'
  | 'doubao'
  | 'minimax';
export const voiceProtocols: Record<VoiceApiProtocol, string> = {
  openai: 'OpenAI / Whisper',
  chatAudio: 'Qwen ASR',
  mimo: 'MiMo ASR',
  openrouter: 'OpenRouter',
  gemini: 'Google Gemini',
  doubao: 'Doubao ASR',
  minimax: 'MiniMax ASR',
};
export interface VoiceApiOptions {
  protocol: VoiceApiProtocol;
}
export interface VoiceDirectoryEntry {
  name: string;
  url: string;
  baseUrl: string;
  modelId: string;
  modelIds: string[];
  protocol: VoiceApiProtocol;
  region: string;
}

export interface VoiceChannel {
  kind: 'workbuddy' | 'account' | 'model';
  id: string;
}
export interface VoiceBinding {
  code: string;
  modifiers: string[];
}
export type VoicePalette = 'white' | 'aurora' | 'ocean' | 'sunset' | 'iris';

export interface VoiceConfig {
  channels: VoiceChannel[];
  apiOptions?: Record<string, VoiceApiOptions>;
  microphone: string | null;
  shortcutEnabled: boolean;
  hotkeyEnabled: boolean;
  hotkeyMode?: 'hold' | 'toggle';
  shortcut: VoiceBinding;
  islandTheme: 'auto' | 'dark' | 'light' | 'system' | 'iridescent';
  islandPalette: VoicePalette;
  islandScale?: number;
  selectionModel?: string | null;
  selectionTranslationEnabled?: boolean;
  selectionTranslationLanguage?: (typeof selectionLanguages)[number];
}
export interface VoiceUsage {
  characters: number;
  durationMs: number;
}
export interface VoiceSnapshot {
  config: VoiceConfig;
  days: Record<string, number>;
  usage: VoiceUsage;
  phase:
    | 'idle'
    | 'starting'
    | 'recording'
    | 'testing'
    | 'transcribing'
    | 'inserting'
    | 'done'
    | 'error';
  activeChannel: number | null;
  level: number;
  elapsedMs: number;
  text: string;
  resultAudioId: number | null;
  error: string | null;
  revision: number;
  shortcutActive: boolean;
  shortcutDescription: string | null;
  appIcon: string | null;
  appIconLoading: boolean;
  selectionOpen?: boolean;
  origin: 'page' | 'test' | 'shortcut' | 'island';
}
export const getVoiceState = () => invoke<VoiceSnapshot>('voice_input_state');
export const voiceIslandScales = [70, 80, 90, 100, 110, 120, 130, 140, 150] as const;
export const selectionLanguages = [
  'zh-Hans',
  'zh-Hant',
  'en',
  'ja',
  'ko',
  'fr',
  'de',
  'es',
] as const;
export interface SelectionTranslation {
  mode?: 'closed' | 'toolbar' | 'card' | 'manual';
  source: string;
  text: string;
  language: string;
  error: string | null;
  busy: boolean;
  revision: number;
}
export const openSelectionTranslation = () => invoke<void>('voice_selection_open');
export const askSelection = () => invoke<void>('voice_selection_ask');
export const getSelectionTranslation = () => invoke<SelectionTranslation>('voice_selection_state');
export const readySelectionTranslation = () =>
  invoke<SelectionTranslation>('voice_selection_ready');
export const translateSelection = (source: string, language: string) =>
  invoke<void>('voice_selection_translate', { source, language });
export const closeSelectionTranslation = () => invoke<void>('voice_selection_close');
export const getVoiceModels = () => invoke<ModelConfig[]>('voice_input_models');
export const getVoiceDevices = () => invoke<string[]>('voice_input_devices');
export const saveVoiceConfig = (config: VoiceConfig) =>
  invoke<VoiceSnapshot>('voice_input_save', { config });
export const startVoice = () => invoke<VoiceSnapshot>('voice_input_start');
export const recordVoice = () => invoke<VoiceSnapshot>('voice_input_record');
export const stopVoice = () => invoke<VoiceSnapshot>('voice_input_stop');
export const cancelVoice = () => invoke<VoiceSnapshot>('voice_input_cancel');
export const suspendVoiceShortcut = (suspended: boolean) =>
  invoke<void>('voice_input_suspend', { suspended });
export const voiceIslandReady = () => invoke<VoiceSnapshot>('voice_input_island_ready');
export const getVoiceReplay = async (audioId: number): Promise<ArrayBuffer> => {
  const data = await invoke<ArrayBuffer | number[]>('voice_input_replay', { audioId });
  return data instanceof ArrayBuffer ? data : new Uint8Array(data).buffer;
};
export const showMainWindow = () => invoke<void>('voice_input_show_main');
