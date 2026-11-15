import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  cacheAccountProxyModels,
  getAccountProxyModels,
  getCachedAccountProxyModels,
  listProxyAccounts,
  getAccountSpeechModels,
  speechSourceFromSelector,
  accountProxyProviders,
  accountSpeechProviders,
  fixedAccountSpeechModels,
} from './accountProxy';
import type { AccountSource } from './types';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

let saved: Map<string, string>;
beforeEach(() => {
  saved = new Map();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
  });
});

it('resolves only supported speech selectors with a valid saved account identity', () => {
  const accountId = 'a'.repeat(64);
  for (const [prefix, provider] of [
    ['workbuddy', 'workBuddy'],
    ['workbuddyai', 'workBuddyAi'],
    ['claudecode', 'claudeCode'],
    ['codex', 'codex'],
    ['minimax', 'minimax'],
    ['antigravity', 'antigravity'],
    ['cursor', 'cursor'],
    ['grokbot', 'grokbot'],
    ['dumate', 'dumate'],
    ['doubaowork', 'doubaowork'],
    ['devin', 'devin'],
    ['trae', 'trae'],
    ['codearts', 'codeArts'],
    ['traecn', 'traecn'],
    ['qoder', 'qoder'],
    ['qodercn', 'qoderCn'],
    ['qwenworkcn', 'qwenworkcn'],
    ['qwenwork', 'qwenwork'],
    ['kilo', 'kilo'],
  ] as const) {
    expect(
      speechSourceFromSelector(`${prefix}:${accountId}${prefix === 'kilo' ? ':vendor:speech' : ''}`)
    ).toEqual({
      provider,
      accountId: provider === 'codeArts' ? `codearts:${accountId}` : accountId,
    });
  }
  for (const selector of [
    `copilot:${accountId}`,
    `workbuddy:../${accountId}`,
    `workbuddy:${accountId}:chat-model`,
    `kilo:${accountId}`,
    `kilo:${accountId}: `,
    `kilo:${accountId}:${'x'.repeat(512)}`,
  ])
    expect(speechSourceFromSelector(selector)).toBeUndefined();
  expect(invoke).not.toHaveBeenCalled();
});

it('keeps speech-only providers out of text access and preserves CodeArts saved identity prefixes', () => {
  for (const provider of ['cursor', 'grokbot', 'qwenwork', 'qwenworkcn']) {
    expect(accountSpeechProviders.map((entry) => entry.id)).toContain(provider);
    expect(accountProxyProviders.map((entry) => entry.id)).not.toContain(provider);
  }
  expect(accountProxyProviders).toHaveLength(24);
  expect(accountProxyProviders.map((entry) => entry.id)).toContain('commandcode');
  const hash = 'a'.repeat(64);
  expect(fixedAccountSpeechModels({ provider: 'codeArts', accountId: `codearts:${hash}` })).toEqual(
    [`codearts:${hash}`]
  );
  expect(fixedAccountSpeechModels({ provider: 'codeArts', accountId: hash })).toEqual([]);
  expect(speechSourceFromSelector(`codearts:codearts:${hash}`)).toBeUndefined();
  expect(invoke).not.toHaveBeenCalled();
});

it('reads Kilo speech models without changing cached text models', async () => {
  const source: AccountSource = { provider: 'kilo', accountId: 'a'.repeat(64) };
  const selector = `kilo:${source.accountId}:speech-model`;
  cacheAccountProxyModels(source, ['text-model']);
  vi.mocked(invoke).mockResolvedValue([selector]);
  expect(await getAccountSpeechModels(source)).toEqual([selector]);
  expect(invoke).toHaveBeenCalledExactlyOnceWith('get_account_speech_models', { source });
  expect(getCachedAccountProxyModels(source)).toEqual(['text-model']);
});
afterEach(() => {
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});

it.each(['trae', 'traecn', 'kiro', 'devin', 'dumate'] as const)(
  'reads saved %s accounts and model choices using the exact saved source',
  async (provider) => {
    const source: AccountSource = { provider, accountId: 'shared-id' };
    const account = { source, label: 'Saved identity', plan: 'Pro' };
    vi.mocked(invoke).mockResolvedValueOnce([account]).mockResolvedValueOnce(['model-one']);
    expect(await listProxyAccounts(provider)).toEqual([account]);
    expect(invoke).toHaveBeenNthCalledWith(1, 'list_proxy_accounts', { provider });
    expect(await getAccountProxyModels(source)).toEqual(['model-one']);
    expect(invoke).toHaveBeenNthCalledWith(2, 'get_account_proxy_models', { source });
    expect(invoke).toHaveBeenCalledTimes(2);
  }
);

it('isolates both regional directories for the same account ID, including a cached empty list', () => {
  const global: AccountSource = { provider: 'trae', accountId: 'shared-id' };
  const cn: AccountSource = { provider: 'traecn', accountId: 'shared-id' };
  expect(getCachedAccountProxyModels(global)).toBeUndefined();
  expect(getCachedAccountProxyModels(cn)).toBeUndefined();
  cacheAccountProxyModels(global, ['global-model']);
  cacheAccountProxyModels(cn, ['cn-model']);
  expect(getCachedAccountProxyModels(global)).toEqual(['global-model']);
  expect(getCachedAccountProxyModels(cn)).toEqual(['cn-model']);
  cacheAccountProxyModels(cn, []);
  expect(getCachedAccountProxyModels(cn)).toEqual([]);
  expect(getCachedAccountProxyModels(global)).toEqual(['global-model']);
  expect([...saved.values()].sort()).toEqual(['["global-model"]', '[]'].sort());
  expect(invoke).not.toHaveBeenCalled();
});
