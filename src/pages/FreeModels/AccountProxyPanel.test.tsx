import { enableAccountPlugins } from '../../test/accountPlugins';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { invoke } from '@tauri-apps/api/core';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AccountProxyPanel } from './AccountProxyPanel';
import * as api from '../../api/accountProxy';
import type { AccountProvider } from '../../api/types';
import { useNavigationStore } from '../../stores/navigationStore';
import { loadLocale, translate, type TKey } from '../../i18n';

const { context, confirm } = vi.hoisted(() => ({
  confirm: vi.fn(),
  context: {
    setNewModelForm: vi.fn(),
    setEditingModelId: vi.fn(),
    setKeyDestroyed: vi.fn(),
    setShowApiKey: vi.fn(),
    setModelModalDestination: vi.fn(),
    setShowAddModelModal: vi.fn(),
  },
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: TKey) => translate(key, locale) }),
}));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../ModelNexus/context', () => ({ useModelNexus: () => context }));
vi.mock('../../api/accountProxy', async (original) => ({
  ...(await original<typeof import('../../api/accountProxy')>()),
  listProxyAccounts: vi.fn(),
  getAccountProxyModels: vi.fn(),
}));

let renderer: ReactTestRenderer;
let locale = 'zh-Hans';
function accounts(provider: AccountProvider, count: number): api.ProxyAccount[] {
  return Array.from({ length: count }, (_, index) => ({
    source: { provider, accountId: `saved-${index}` },
    label: `Saved ${index}`,
  }));
}
async function open() {
  await act(async () => {
    renderer = create(<AccountProxyPanel />);
  });
}
async function navigate(page: 'accounts' | 'freeModels') {
  await act(async () => useNavigationStore.getState().setActivePage(page));
}
function row(provider: AccountProvider) {
  const name = api.accountProxyProviders.find((entry) => entry.id === provider)!.name;
  return renderer.root
    .findAllByType('button')
    .find((button) => button.props['aria-label']?.endsWith(`: ${name}`))!;
}
function count(provider: AccountProvider) {
  return row(provider)
    .findAllByType('span')
    .find((span) => String(span.props.className).includes('tabular-nums'))!
    .children.join('');
}
beforeEach(async () => {
  enableAccountPlugins();
  locale = 'zh-Hans';
  await loadLocale(locale);
  useNavigationStore.setState({ activePage: 'freeModels' });
  vi.mocked(api.listProxyAccounts)
    .mockReset()
    .mockImplementation(async (provider) =>
      accounts(provider, provider === 'claudeCode' ? 0 : provider === 'codex' ? 2 : 1)
    );
});

it('updates zero, singular, plural and unknown account counts across all languages without new account or catalog reads', async () => {
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) => {
    if (provider === 'copilot') throw new Error('Unreadable store');
    return accounts(provider, provider === 'claudeCode' ? 0 : provider === 'codex' ? 2 : 1);
  });
  await open();
  const reads = vi.mocked(api.listProxyAccounts).mock.calls.length;
  for (const [language, expected] of [
    ['zh-Hans', ['0 个账号', '1 个账号', '2 个账号']],
    ['zh-Hant', ['0 個帳號', '1 個帳號', '2 個帳號']],
    ['en', ['0 accounts', '1 account', '2 accounts']],
    ['ja', ['0 アカウント', '1 アカウント', '2 アカウント']],
  ] as const) {
    locale = language;
    await loadLocale(locale);
    await act(async () => renderer.update(<AccountProxyPanel />));
    expect([count('claudeCode'), count('kilo'), count('codex'), count('copilot')]).toEqual([
      ...expected,
      '—',
    ]);
  }
  expect(api.listProxyAccounts).toHaveBeenCalledTimes(reads);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.clearAllMocks();
});

it('counts saved accounts independently, including zero, without reading model catalogs or changing selections', async () => {
  await open();
  expect(vi.mocked(api.listProxyAccounts).mock.calls.map(([provider]) => provider)).toEqual(
    api.accountProxyProviders.map((provider) => provider.id)
  );
  expect(count('claudeCode')).toBe('0 个账号');
  expect(count('codex')).toBe('2 个账号');
  expect(count('kilo')).toBe('1 个账号');
  expect(count('workBuddy')).toBe('1 个账号');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
  act(() => row('kilo').props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ accountSource: { provider: 'kilo', accountId: '' } })
  );
  expect(context.setModelModalDestination).toHaveBeenCalledWith('freeRouter');
  expect(context.setShowAddModelModal).toHaveBeenCalledWith(true);
  expect(api.listProxyAccounts).toHaveBeenCalledTimes(api.accountProxyProviders.length);
});

it('offers DuMate saved accounts as a router source without starting login or reading models on entry', async () => {
  await open();
  expect(count('dumate')).toBe('1 个账号');
  expect(api.accountProxyProviders.find((p) => p.id === 'dumate')?.icon).toBe('dumate.svg');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  act(() => row('dumate').props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({
      accountSource: { provider: 'dumate', accountId: '' },
      apiKey: '',
      baseUrl: '',
    })
  );
  expect(context.setModelModalDestination).toHaveBeenCalledWith('freeRouter');
  expect(invoke).not.toHaveBeenCalled();
});

it.each(['doubaowork', 'dola'] as const)(
  'offers %s through the existing account form without loading models or starting login',
  async (provider) => {
    await open();
    expect(api.accountProxyProviders.find((entry) => entry.id === provider)?.icon).toBe(
      `${provider}.png`
    );
    expect(api.getAccountProxyModels).not.toHaveBeenCalled();
    act(() => row(provider).props.onClick());
    expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({
        accountSource: { provider, accountId: '' },
        apiKey: '',
        baseUrl: '',
      })
    );
    expect(context.setModelModalDestination).toHaveBeenCalledWith('freeRouter');
    expect(invoke).not.toHaveBeenCalled();
  }
);

it('puts larger saved-account pools first and preserves catalog order for ties without mutating it', async () => {
  const originalOrder = api.accountProxyProviders.map((provider) => provider.id);
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) => {
    if (provider === 'claudeCode') throw new Error('Unreadable store');
    return accounts(
      provider,
      provider === 'workBuddyAi' ? 3 : provider === 'codex' || provider === 'kilo' ? 2 : 0
    );
  });
  await open();
  const names = renderer.root
    .findAllByType('button')
    .filter((button) => button.props['aria-label']?.includes(': '))
    .map((button) => button.findAllByType('span')[0].children.join(''));
  expect(names).toEqual([
    'WorkBuddy AI',
    'ChatGPT / Codex',
    'Kilo',
    ...api.accountProxyProviders
      .filter((provider) => !['workBuddyAi', 'codex', 'kilo'].includes(provider.id))
      .map((provider) => provider.name),
  ]);
  expect(count('claudeCode')).toBe('—');
  expect(api.accountProxyProviders.map((provider) => provider.id)).toEqual(originalOrder);
  expect(api.listProxyAccounts).toHaveBeenCalledTimes(originalOrder.length);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  act(() => row('workBuddyAi').props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ accountSource: { provider: 'workBuddyAi', accountId: '' } })
  );
});

it('keeps TRAE regional counts and add sources separate while passive pool navigation performs only saved-list reads', async () => {
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) =>
    accounts(provider, provider === 'trae' ? 2 : provider === 'traecn' ? 1 : 0)
  );
  await open();
  expect(count('trae')).toBe('2 个账号');
  expect(count('traecn')).toBe('1 个账号');
  await navigate('accounts');
  await navigate('freeModels');
  expect(count('trae')).toBe('2 个账号');
  expect(count('traecn')).toBe('1 个账号');
  expect(vi.mocked(api.listProxyAccounts).mock.calls.filter(([id]) => id === 'trae')).toHaveLength(
    2
  );
  expect(
    vi.mocked(api.listProxyAccounts).mock.calls.filter(([id]) => id === 'traecn')
  ).toHaveLength(2);
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
  for (const provider of ['trae', 'traecn'] as const) {
    context.setNewModelForm.mockClear();
    act(() => row(provider).props.onClick());
    expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ accountSource: { provider, accountId: '' }, apiKey: '' })
    );
  }
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
});

it('counts the shared Kiro account pool once and opens its existing account form', async () => {
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) =>
    accounts(provider, provider === 'kiro' ? 2 : 0)
  );
  await open();
  expect(count('kiro')).toBe('2 个账号');
  expect(vi.mocked(api.listProxyAccounts).mock.calls.filter(([id]) => id === 'kiro')).toHaveLength(
    1
  );
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
  act(() => row('kiro').props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ accountSource: { provider: 'kiro', accountId: '' }, apiKey: '' })
  );
  expect(context.setModelModalDestination).toHaveBeenCalledWith('freeRouter');
  await navigate('accounts');
  vi.mocked(api.listProxyAccounts).mockResolvedValue([]);
  await navigate('freeModels');
  expect(count('kiro')).toBe('0 个账号');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
});

it('reloads counts when returning from account management, preserves known counts on failure and updates after deletion', async () => {
  await open();
  await navigate('accounts');
  vi.mocked(api.listProxyAccounts)
    .mockClear()
    .mockImplementation(async (provider) => {
      if (provider === 'codex') throw new Error('Saved store temporarily unavailable');
      return accounts(provider, 0);
    });
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
  await navigate('freeModels');
  expect(count('codex')).toBe('2 个账号');
  expect(count('kilo')).toBe('0 个账号');
  await navigate('accounts');
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) =>
    accounts(provider, provider === 'codex' ? 3 : 0)
  );
  await navigate('freeModels');
  expect(count('codex')).toBe('3 个账号');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
});

it('shows unknown rather than a false zero for an unreadable saved account list', async () => {
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) => {
    if (provider === 'codex') throw new Error('Unreadable store');
    return [];
  });
  await open();
  expect(count('codex')).toBe('—');
  expect(count('kilo')).toBe('0 个账号');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
});

it('ignores an earlier directory after leaving the page and does not load while hidden', async () => {
  useNavigationStore.setState({ activePage: 'accounts' });
  await open();
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
  let resolve!: (rows: api.ProxyAccount[]) => void;
  vi.mocked(api.listProxyAccounts).mockImplementation((provider) =>
    provider === 'codex'
      ? new Promise((done) => {
          resolve = done;
        })
      : Promise.resolve([])
  );
  await navigate('freeModels');
  expect(count('codex')).toBe('—');
  await navigate('accounts');
  vi.mocked(api.listProxyAccounts).mockImplementation(async (provider) =>
    accounts(provider, provider === 'codex' ? 1 : 0)
  );
  await navigate('freeModels');
  expect(count('codex')).toBe('1 个账号');
  await act(async () => resolve(accounts('codex', 9)));
  expect(count('codex')).toBe('1 个账号');
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
});

it('reuses the speech-capable provider rows and supplied counts without a second directory read', async () => {
  useNavigationStore.setState({ activePage: 'voice' });
  const saved = [
    ...accounts('workBuddy', 1),
    ...accounts('claudeCode', 2),
    ...accounts('minimax', 3),
  ];
  await act(async () => {
    renderer = create(<AccountProxyPanel voice accounts={saved} />);
  });
  const names = renderer.root
    .findAllByType('button')
    .filter((button) => button.props['aria-label']?.includes(': '))
    .map((button) => button.findAllByType('span')[0].children.join(''));
  expect(names).toEqual([
    'MiniMax',
    'Claude Code',
    'WorkBuddy',
    'ChatGPT / Codex',
    'WorkBuddy AI',
    'DuMate',
    '豆包工作',
    'Antigravity',
    'Trae',
    'Trae CN',
    'Qoder',
    'Qoder CN',
    'Kilo',
    'CodeArts',
    'Devin',
    'Cursor',
    'Grok Bot',
    '千问办公',
    'QwenWork',
  ]);
  expect(count('workBuddy')).toBe('1 个账号');
  expect(count('workBuddyAi')).toBe('0 个账号');
  expect(count('claudeCode')).toBe('2 个账号');
  expect(count('minimax')).toBe('3 个账号');
  expect(renderer.root.findAllByType('div')[0].props.className).toBe('space-y-2');
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  act(() => row('minimax').props.onClick());
  expect(context.setNewModelForm).toHaveBeenCalledExactlyOnceWith(
    expect.objectContaining({ accountSource: { provider: 'minimax', accountId: '' } })
  );
  expect(context.setModelModalDestination).toHaveBeenCalledWith('voiceInput');
});

it('keeps speech source actions disabled while pending and enables them afterward', async () => {
  useNavigationStore.setState({ activePage: 'voice' });
  await act(async () => {
    renderer = create(<AccountProxyPanel voice disabled accounts={[]} />);
  });
  expect(row('workBuddyAi').props.disabled).toBe(true);
  act(() => row('workBuddyAi').props.onClick());
  expect(context.setShowAddModelModal).not.toHaveBeenCalled();
  await act(async () => {
    renderer.update(<AccountProxyPanel voice accounts={[]} />);
  });
  expect(row('workBuddyAi').props.disabled).toBe(false);
  act(() => row('workBuddyAi').props.onClick());
  expect(context.setShowAddModelModal).toHaveBeenCalledOnce();
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
});

it('shows unknown speech counts until supplied results establish zero or a saved count', async () => {
  useNavigationStore.setState({ activePage: 'voice' });
  const pending = {};
  const saved = accounts('workBuddy', 1);
  await act(async () => {
    renderer = create(<AccountProxyPanel voice accounts={saved} accountCounts={pending} />);
  });
  expect(count('workBuddy')).toBe('—');
  expect(count('claudeCode')).toBe('—');
  expect(count('codex')).toBe('—');
  await act(async () => {
    renderer.update(
      <AccountProxyPanel voice accounts={saved} accountCounts={{ workBuddy: 1, codex: 0 }} />
    );
  });
  expect(count('workBuddy')).toBe('1 个账号');
  expect(count('codex')).toBe('0 个账号');
  expect(count('claudeCode')).toBe('—');
  expect(pending).toEqual({});
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(context.setNewModelForm).not.toHaveBeenCalled();
});

it('only infers known speech counts when account rows were explicitly supplied', async () => {
  useNavigationStore.setState({ activePage: 'voice' });
  await act(async () => {
    renderer = create(<AccountProxyPanel voice />);
  });
  expect(count('workBuddy')).toBe('—');
  await act(async () => renderer.update(<AccountProxyPanel voice accounts={[]} />));
  expect(count('workBuddy')).toBe('0 个账号');
  expect(api.listProxyAccounts).not.toHaveBeenCalled();
  expect(api.getAccountProxyModels).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
});
