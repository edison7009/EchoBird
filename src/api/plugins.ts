import { invoke } from '@tauri-apps/api/core';
import type { ModelConfig } from './types';

export type PluginCategory = 'application' | 'account' | 'appearance' | 'feature' | 'other';

export interface PluginProviderContribution {
  id: string;
  title: string;
  applications: string[];
  pageId?: string;
  searchTerms?: string[];
  operations?: string[];
}

export interface PluginManifest {
  name: string;
  publisher: string;
  version: string;
  displayName: string;
  description: string;
  license: string;
  repository: string;
  icon?: string;
  category?: PluginCategory;
  categories?: PluginCategory[];
  tools?: { id: string; directory: string }[];
  echobird: {
    manifestVersion: number;
    apiVersion: string;
    platforms: string[];
    permissions: string[];
    contributes: {
      pages: { id: string; title: string; entry: string }[];
      providers?: PluginProviderContribution[];
      themes?: PluginTheme[];
      configuration?: PluginConfiguration[];
      commands?: { id: string; title: string; pageId?: string }[];
      textProcessors?: { id: string; title: string; stage: 'voice.transcribed' | 'voice.output' }[];
    };
    background?: string;
    worker?: { entry: string; runtime?: 'node' | 'native' };
  };
}

export interface InstalledPlugin {
  id: string;
  manifest: PluginManifest;
  enabled: boolean;
  directory: string;
  source: string;
  revision: string | null;
  icon: string | null;
}

export interface PluginTheme {
  id: string;
  title: string;
  light: Record<string, string>;
  dark: Record<string, string>;
}
export interface PluginConfiguration {
  key: string;
  title: string;
  type: 'boolean' | 'number' | 'string';
  default: boolean | number | string;
  options?: (boolean | number | string)[];
}
export interface PluginSelection {
  pluginId: string;
  id: string;
}
export interface PluginProductState {
  theme: PluginSelection | null;
  palette: PluginTheme | null;
  processors: PluginSelection[];
  configuration: Record<string, Record<string, boolean | number | string>>;
  errors: Record<string, string>;
}
export const getPluginProductState = () => invoke<PluginProductState>('plugin_sdk_state');
export const managePluginProduct = (
  id: string | null,
  action: string,
  key: string,
  value: unknown = null
) => invoke<void>('plugin_sdk_manage', { id, action, key, value });
export const executePluginCommand = (id: string, command: string, input: unknown = null) =>
  invoke<unknown>('plugin_sdk_execute', { id, command, input });

export interface PluginRegistry {
  schemaVersion: number;
  name: string;
  plugins: {
    id: string;
    repository: string;
    clientIcon?: string;
    descriptions?: Record<PluginIndexRegion, string>;
    icons?: Partial<Record<PluginIndexRegion, string>>;
    releases: {
      manifest: PluginManifest;
      commit: string;
      sha256: string;
      downloads: Record<string, string>;
    }[];
  }[];
}

export interface PluginCandidate {
  token: string;
  manifest: PluginManifest;
  source: string;
  revision: string | null;
}

export type PluginRegion = 'international' | 'china' | 'cnb';
/** CNB mirrors GitCode, so it reads the China entries of the index. */
export type PluginIndexRegion = Exclude<PluginRegion, 'cnb'>;
export type PluginSource = 'directory' | 'repository' | 'catalog';

export const listPlugins = () => invoke<InstalledPlugin[]>('plugin_list');
export const getPluginRegistry = (region: PluginRegion, refresh: boolean) =>
  invoke<PluginRegistry>('plugin_registry', { region, refresh });
export const previewPlugin = (kind: PluginSource, value: string, region: PluginRegion) =>
  invoke<PluginCandidate>('plugin_preview', { kind, value, region });
export const cancelPlugin = (token: string) => invoke<void>('plugin_cancel', { token });
export const installPlugin = (token: string) =>
  invoke<InstalledPlugin[]>('plugin_install', { token });
export const setPluginEnabled = (id: string, enabled: boolean) =>
  invoke<InstalledPlugin[]>('plugin_set_enabled', { id, enabled });
export const uninstallPlugin = (id: string) =>
  invoke<InstalledPlugin[]>('plugin_uninstall', { id });
export const launchPlugin = (id: string, pageId: string, model?: ModelConfig) =>
  invoke<void>('plugin_launch', { id, pageId, model: model ?? null });

export const accountPluginOperation = (
  id: string,
  providerId: string,
  operation: string,
  params: unknown = {}
) => invoke<unknown>('plugin_account_operation', { id, providerId, operation, params });
