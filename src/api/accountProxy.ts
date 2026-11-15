import { invoke } from '@tauri-apps/api/core';
import type { AccountProvider, AccountSource } from './types';

export const accountProxyProviders: {
  id: AccountProvider;
  name: string;
  icon: string;
  speechPrefix?: string;
}[] = [
  { id: 'commandcode', name: 'CommandCode', icon: 'commandcode.svg' },
  { id: 'claudeCode', name: 'Claude Code', icon: 'claudecode.svg', speechPrefix: 'claudecode' },
  { id: 'codex', name: 'ChatGPT / Codex', icon: 'chatgptdesktop.svg', speechPrefix: 'codex' },
  { id: 'workBuddy', name: 'WorkBuddy', icon: 'workbuddy.png', speechPrefix: 'workbuddy' },
  { id: 'workBuddyAi', name: 'WorkBuddy AI', icon: 'workbuddyai.png', speechPrefix: 'workbuddyai' },
  { id: 'dumate', name: 'DuMate', icon: 'dumate.svg', speechPrefix: 'dumate' },
  { id: 'doubaowork', name: '豆包工作', icon: 'doubaowork.png', speechPrefix: 'doubaowork' },
  { id: 'dola', name: 'Dola', icon: 'dola.png' },
  { id: 'antigravity', name: 'Antigravity', icon: 'antigravity.png', speechPrefix: 'antigravity' },
  { id: 'kiro', name: 'Kiro', icon: 'kirocrew.svg' },
  { id: 'trae', name: 'Trae', icon: 'trae.svg', speechPrefix: 'trae' },
  { id: 'traecn', name: 'Trae CN', icon: 'traecn.svg', speechPrefix: 'traecn' },
  { id: 'minimax', name: 'MiniMax', icon: 'minimaxdesktop.svg', speechPrefix: 'minimax' },
  { id: 'qoder', name: 'Qoder', icon: 'qoder.svg', speechPrefix: 'qoder' },
  { id: 'qoderCn', name: 'Qoder CN', icon: 'qoder.svg', speechPrefix: 'qodercn' },
  { id: 'grok', name: 'Grok Build', icon: 'grok.svg' },
  { id: 'cline', name: 'Cline', icon: 'clinedesktop.png' },
  { id: 'kilo', name: 'Kilo', icon: 'kilodesktop.png', speechPrefix: 'kilo' },
  { id: 'copilot', name: 'GitHub Copilot', icon: 'copilotdesktop.png' },
  { id: 'zcode', name: 'ZCode', icon: 'zcode.png' },
  { id: 'kimi', name: 'Kimi', icon: 'kimidesktop.png' },
  { id: 'openDesign', name: 'Open Design', icon: 'opendesign.png' },
  { id: 'codeArts', name: 'CodeArts', icon: 'codearts.png', speechPrefix: 'codearts' },
  { id: 'devin', name: 'Devin', icon: 'devin.png', speechPrefix: 'devin' },
];

export const accountSpeechProviders: typeof accountProxyProviders = [
  ...accountProxyProviders.filter((provider) => provider.speechPrefix),
  { id: 'cursor', name: 'Cursor', icon: 'cursor.svg', speechPrefix: 'cursor' },
  { id: 'grokbot', name: 'Grok Bot', icon: 'grokbot.png', speechPrefix: 'grokbot' },
  { id: 'qwenworkcn', name: '千问办公', icon: 'qwenworkcn.svg', speechPrefix: 'qwenworkcn' },
  { id: 'qwenwork', name: 'QwenWork', icon: 'qwenwork.svg', speechPrefix: 'qwenwork' },
];

export const accountProviderIconSrc = (icon: string) =>
  icon.includes(':') ? icon : `/icons/tools/${icon}`;

export function speechSourceFromSelector(selector: string): AccountSource | undefined {
  if (selector.length > 512) return undefined;
  const [prefix, accountId, ...model] = selector.split(':');
  const provider = accountSpeechProviders.find((entry) => entry.speechPrefix === prefix);
  if (!provider || !/^[a-fA-F0-9]{64}$/.test(accountId ?? '')) return undefined;
  if (provider.id === 'kilo' ? !model.join(':').trim() : model.length !== 0) return undefined;
  return {
    provider: provider.id,
    accountId: provider.id === 'codeArts' ? `codearts:${accountId}` : accountId,
  };
}

export function fixedAccountSpeechModels(source: AccountSource): string[] | undefined {
  const provider = accountSpeechProviders.find((entry) => entry.id === source.provider);
  if (!provider) return [];
  if (provider.id === 'kilo') return undefined;
  const accountId =
    provider.id === 'codeArts'
      ? source.accountId.startsWith('codearts:')
        ? source.accountId.slice('codearts:'.length)
        : undefined
      : source.accountId;
  return accountId ? [`${provider.speechPrefix}:${accountId}`] : [];
}

export function getAccountSpeechModels(source: AccountSource): Promise<string[]> {
  return invoke('get_account_speech_models', { source });
}

export interface ProxyAccount {
  source: AccountSource;
  label: string;
  plan?: string;
  region?: 'cn' | 'global';
  modelLabels?: Record<string, string>;
  freeModelIds?: string[];
}

export function accountModelPickerOptions(
  models: string[],
  currentId: string,
  labels?: Record<string, string>
): string[] {
  const alias = labels?.[currentId];
  return alias && alias !== currentId
    ? models.map((id) => (id === alias ? currentId : id))
    : models;
}

export function listProxyAccounts(provider: AccountProvider): Promise<ProxyAccount[]> {
  return invoke('list_proxy_accounts', { provider });
}

export function getAccountProxyModels(source: AccountSource): Promise<string[]> {
  return invoke('get_account_proxy_models', { source });
}

function modelCacheKey(source: AccountSource) {
  return `echobird-account-models:${JSON.stringify([source.provider, source.accountId])}`;
}

export function getCachedAccountProxyModels(source: AccountSource): string[] | undefined {
  try {
    const models = JSON.parse(localStorage.getItem(modelCacheKey(source)) ?? 'null');
    if (Array.isArray(models) && models.every((id) => typeof id === 'string')) return models;
  } catch {
    // An unavailable or damaged cache falls back to reading the account directory.
  }
  return undefined;
}

export function cacheAccountProxyModels(source: AccountSource, models: string[]): void {
  try {
    localStorage.setItem(modelCacheKey(source), JSON.stringify(models));
  } catch {
    // Keep the current directory usable if persistence is unavailable.
  }
}
