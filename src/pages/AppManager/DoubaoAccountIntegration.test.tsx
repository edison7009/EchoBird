import { enableAccountPlugins } from '../../test/accountPlugins';
import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DoubaoAccount, DoubaoEdition } from '../../api/doubao';
import workDefinition from '../../../tools/doubaowork/paths.json';
import dolaDefinition from '../../../tools/dola/paths.json';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { ACCOUNT_NAME_MASK, useAccountPrivacyStore } from '../../stores/accountPrivacyStore';
import { AppManagerProvider } from './AppManagerProvider';
import { useAppManager } from './context';
import { DoubaoAccountSection } from './DoubaoAccountSection';
import { AccountSectionRow } from './AccountSectionPrimitives';
import { accountProxyStatus } from './accountProxyStatus';
import { AccountCenterTitleActions } from '../AccountCenter/AccountCenter';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';

const { invoke, openExternal } = vi.hoisted(() => ({
  invoke: vi.fn(),
  openExternal: vi.fn(),
}));
vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/plugin-shell', () => ({ open: openExternal }));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({
    t: (key: string) =>
      (
        ({
          'accountCenter.batchAuthFailed': 'Authorization failed: {n}',
          'accountCenter.batchClaimFailed': 'Claim failed: {n}',
          'accountCenter.batchRefreshFailed': 'Refresh failed: {n}',
        }) as Record<string, string>
      )[key] ?? key,
    locale: 'en',
  }),
}));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => async () => true }));
vi.mock('../ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => ({ routerEnabled: false }) }));
vi.mock('./ClaudeCodeLoginDialog', () => ({ ClaudeCodeLoginDialog: () => null }));

const editions = ['doubaowork', 'dola'] as const;
it.each([
  [workDefinition, 'DoubaoWork'],
  [dolaDefinition, 'Cici'],
] as const)(
  'prefers the native Chromium executable over its outer launcher ($1)',
  (definition, executable) => {
    expect(definition.paths.win32).toEqual([
      `%LOCALAPPDATA%\\${executable}\\Application\\app\\${executable}.exe`,
      `%LOCALAPPDATA%\\${executable}\\Application\\${executable}.exe`,
    ]);
  }
);
const account = (edition: DoubaoEdition): DoubaoAccount => ({
  id: `${edition}-saved`,
  edition,
  name: `${edition} Fixture`,
  email: null,
  active: true,
  plan: null,
  expiresAt: null,
  remaining: null,
  total: null,
});
let state: ReturnType<typeof useAppManager>;
let renderer: ReactTestRenderer;
let handlers: Map<string, (args?: Record<string, string>) => unknown>;
let errorEvents: (string | null)[];
function Harness() {
  const context = useAppManager();
  const page = useNavigationStore((state) => state.activePage);
  useLayoutEffect(() => {
    state = context;
  });
  useLayoutEffect(() => {
    errorEvents.push(context.applyError);
  }, [context.applyError]);
  return (
    <>
      {editions.includes(context.selectedTool as DoubaoEdition) ? <DoubaoAccountSection /> : null}
      {page === 'accounts' && <AccountCenterTitleActions />}
    </>
  );
}
async function tick() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1);
  });
}
async function mount(edition: DoubaoEdition, installed = true) {
  useToolsStore
    .getState()
    .setDetectedTools([
      { id: edition, name: edition, category: 'Desktop', installed, noModelConfig: true },
    ]);
  act(() => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  act(() => state.setSelectedTool(edition));
  await tick();
}
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);

it('the Account Center claim-all action rechecks a cached claimed daily reward with no reset time', async () => {
  const row: DoubaoAccount = {
    ...account('doubaowork'),
    dailyReward: { status: 'claimed', scene: 'fixture', nextClaimAt: null },
  };
  handlers.set('list_doubao_accounts', () => [row]);
  handlers.set('claim_doubao_daily_credits', () => row);
  await mount('doubaowork');
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  expect(calls('claim_doubao_daily_credits')).toHaveLength(0);
  const all = renderer.root.findByProps({ 'aria-label': 'accountCenter.claimAll' });
  expect(all.props.disabled).toBe(false);
  await act(async () => {
    await all.props.onClick();
  });
  expect(calls('claim_doubao_daily_credits')).toEqual([
    ['claim_doubao_daily_credits', { edition: 'doubaowork', accountId: 'doubaowork-saved' }],
  ]);
});

it.each(editions)(
  '%s claims only from an explicit gift action without changing selection',
  async (edition) => {
    const row: DoubaoAccount = {
      ...account(edition),
      dailyReward: { status: 'available', scene: 'fixture-campaign', nextClaimAt: null },
    };
    handlers.set('list_doubao_accounts', () => [row]);
    handlers.set('claim_doubao_daily_credits', () => ({
      ...row,
      dailyReward: { ...row.dailyReward, status: 'claimed' },
    }));
    await mount(edition);
    act(() => state.doubaoAccounts.select(null));
    expect(calls('claim_doubao_daily_credits')).toHaveLength(0);
    const button = renderer.root.findByProps({
      'aria-label': `agent.claimDailyCredits ${edition} Fixture`,
    });
    const stopPropagation = vi.fn();
    await act(async () => {
      button.props.onClick({ stopPropagation });
    });
    expect(stopPropagation).toHaveBeenCalledOnce();
    expect(calls('claim_doubao_daily_credits')).toEqual([
      ['claim_doubao_daily_credits', { edition, accountId: `${edition}-saved` }],
    ]);
    expect(state.doubaoAccounts.selectedId).toBeNull();
    expect(calls('switch_doubao_account')).toHaveLength(0);
    expect(calls('start_tool')).toHaveLength(0);
    expect(
      renderer.root.findByProps({ 'aria-label': `agent.dailyCreditsClaimed ${edition} Fixture` })
        .props.disabled
    ).toBe(true);
  }
);
beforeEach(() => {
  enableAccountPlugins();
  vi.useFakeTimers();
  vi.clearAllMocks();
  const storage = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  useNavigationStore.getState().setActivePage('apps');
  useAccountPrivacyStore.getState().setHidden(false);
  errorEvents = [];
  handlers = new Map<string, (args?: Record<string, string>) => unknown>([
    ['get_models', () => []],
    ['get_tool_install_index', () => '{"ids":[]}'],
    ['list_doubao_accounts', (args) => [account(args!.edition as DoubaoEdition)]],
    ['switch_doubao_account', (args) => account(args!.edition as DoubaoEdition)],
    ['refresh_doubao_account_quota', (args) => account(args!.edition as DoubaoEdition)],
    ['start_tool', () => ({ success: true })],
    ['cancel_doubao_login', () => undefined],
    ['delete_doubao_account', () => undefined],
  ]);
  invoke.mockImplementation(async (name: string, args?: Record<string, string>) => {
    const handler = handlers.get(name);
    if (!handler) throw new Error(`Unexpected IPC ${name}`);
    return handler(args);
  });
});
afterEach(() => {
  act(() => renderer?.unmount());
  useToolsStore.getState().setDetectedTools([]);
  useAccountPrivacyStore.getState().setHidden(false);
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it.each([
  ['claim', 'visible'],
  ['claim', 'hidden'],
  ['refresh', 'visible'],
  ['claim', 'leave'],
] as const)(
  '%s across Doubao, DuMate and QwenWork collects errors once after the last response (%s)',
  async (action, mode) => {
    const doubao = {
      ...account('doubaowork'),
      dailyReward: { status: 'available' as const, scene: 'fixture', nextClaimAt: null },
    };
    const dumate = {
      id: 'dumate',
      name: 'DuMate user',
      enterprise: false,
      active: false,
      plan: 'free',
      expiresAt: null,
      remaining: 100,
      total: 100,
      creditBuckets: [],
      dailyReward: null,
    };
    const qwen = {
      id: 'qwen',
      edition: 'qwenworkcn',
      name: 'Qwen user',
      email: null,
      organization: null,
      active: false,
      plan: 'free',
      planName: 'Free',
      expiresAt: null,
      remaining: 100,
      total: 100,
      creditBuckets: [],
      dailyReward: null,
    };
    // Uninstalled editions now list too; give each fixture to one installed edition.
    handlers.set('list_doubao_accounts', (args?: Record<string, string>) =>
      args?.edition === 'doubaowork' ? [doubao] : []
    );
    handlers.set('list_dumate_accounts', () => [dumate]);
    handlers.set('list_qwenwork_accounts', (args?: Record<string, string>) =>
      args?.edition === 'qwenworkcn' ? [qwen] : []
    );
    const commands =
      action === 'claim'
        ? ['claim_doubao_daily_credits', 'claim_dumate_daily_credits']
        : [
            'refresh_doubao_account_quota',
            'refresh_dumate_account_quota',
            'refresh_qwenwork_account_quota',
          ];
    handlers.set(commands[0], () => {
      throw new Error('accountError.claim');
    });
    if (action === 'refresh')
      handlers.set(commands[1], () => {
        throw new Error('accountError.loginRequired');
      });
    let failLast!: (error: Error) => void;
    handlers.set(
      commands[commands.length - 1],
      () =>
        new Promise((_, reject) => {
          failLast = reject;
        })
    );
    await mount('doubaowork');
    act(() => {
      useToolsStore.getState().setDetectedTools(
        ['doubaowork', 'dumate', 'qwenworkcn'].map((id) => ({
          id,
          name: id,
          category: 'Desktop',
          installed: true,
        }))
      );
      useNavigationStore.getState().setActivePage('accounts');
    });
    await tick();
    for (const command of commands) expect(calls(command)).toHaveLength(0);
    let task!: Promise<void>;
    await act(async () => {
      task = renderer.root
        .findByProps({
          'aria-label': `accountCenter.${action === 'claim' ? 'claimAll' : 'refreshAll'}`,
        })
        .props.onClick();
    });
    for (const command of commands) expect(calls(command)).toHaveLength(1);
    expect(state.applyError).toBeNull();
    expect(errorEvents).toEqual([null]);
    act(() => {
      if (mode === 'hidden') useAccountPrivacyStore.getState().setHidden(true);
      if (mode === 'leave') useNavigationStore.getState().setActivePage('models');
    });
    await act(async () => {
      failLast(
        new Error(action === 'claim' ? 'accountError.loginRequired' : 'accountError.network')
      );
      await task;
    });
    if (mode === 'leave') {
      expect(state.applyError).toBeNull();
      expect(errorEvents).toEqual([null]);
    } else {
      const label = (name: string) => (mode === 'hidden' ? ACCOUNT_NAME_MASK : name);
      const summary = `Authorization failed: 1 (DuMate · ${label('DuMate user')}) ${action === 'claim' ? `Claim failed: 1 (豆包工作 · ${label('doubaowork Fixture')})` : `Refresh failed: 2 (豆包工作 · ${label('doubaowork Fixture')}, 千问办公 · ${label('Qwen user')})`}`;
      expect(state.applyError).toBe(summary);
      expect(errorEvents).toEqual([null, summary]);
      expect(state.dumateAccounts.authorizationFailedIds.has('dumate')).toBe(true);
      expect(state.doubaoAccountGroups.doubaowork.accounts).toEqual([doubao]);
      expect(state.qwenWorkAccountGroups.qwenworkcn.accounts).toEqual([qwen]);
    }
    expect(calls('claim_qwenwork_daily_credits')).toHaveLength(0);
  }
);

it.each(editions)(
  '%s navigation reads saved accounts and explicit apply switches before launch',
  async (edition) => {
    await mount(edition);
    const row = renderer.root.findByType(AccountSectionRow);
    expect(row.props.plan).toBeUndefined();
    expect(row.findAllByType('span').some((node) => node.children.includes('—'))).toBe(true);
    expect(calls('list_doubao_accounts')).toEqual([['list_doubao_accounts', { edition }]]);
    expect(calls('refresh_doubao_account_quota')).toHaveLength(0);
    expect(calls('start_doubao_login')).toHaveLength(0);
    expect(calls('switch_doubao_account')).toHaveLength(0);
    expect(calls('start_tool')).toHaveLength(0);
    await act(async () => state.handleLaunch());
    expect(calls('switch_doubao_account')).toEqual([
      ['switch_doubao_account', { edition, accountId: `${edition}-saved` }],
    ]);
    expect(calls('start_tool')).toEqual([
      ['start_tool', { toolId: edition, startCommand: null, cwd: null }],
    ]);
    const commands = invoke.mock.calls.map(([command]) => command);
    expect(commands.indexOf('switch_doubao_account')).toBeLessThan(commands.indexOf('start_tool'));
    expect(openExternal).not.toHaveBeenCalled();
  }
);

it.each(editions)('uninstalled %s never reads accounts or starts login', async (edition) => {
  await mount(edition, false);
  await act(async () => state.doubaoAccounts.add());
  expect(calls('list_doubao_accounts')).toHaveLength(0);
  act(() => useNavigationStore.getState().setActivePage('accounts'));
  await tick();
  // Native account configuration stays unread until the client is installed.
  expect(calls('list_doubao_accounts')).toHaveLength(0);
  const provider = accountCenterProviders(state, (key) => key, 'en').find(
    (item) => item.id === edition
  )!;
  act(() => provider.add());
  await tick();
  expect(state.applyError).toBe('accountCenter.installClientFirst');
  expect(calls('start_doubao_login')).toHaveLength(0);
});

it.each(editions)(
  '%s actions preserve selection and API-model selection is mutually exclusive',
  async (edition) => {
    await mount(edition);
    act(() => state.doubaoAccounts.select(null));
    const row = renderer.root.findByType(AccountSectionRow);
    const refresh = row.findByProps({ 'aria-label': `agent.refreshAccount ${edition} Fixture` });
    const stopPropagation = vi.fn();
    await act(async () => refresh.props.onClick({ stopPropagation }));
    expect(stopPropagation).toHaveBeenCalled();
    expect(state.doubaoAccounts.selectedId).toBeNull();
    expect(calls('refresh_doubao_account_quota')).toEqual([
      ['refresh_doubao_account_quota', { edition, accountId: `${edition}-saved` }],
    ]);
    act(() => state.doubaoAccounts.select(`${edition}-saved`));
    act(() => state.handleSelectModel(edition, 'fixture-model'));
    expect(state.doubaoAccounts.selectedId).toBeNull();
    expect(state.toolModelConfig[edition]).toBe('fixture-model');
    act(() => state.doubaoAccounts.select(`${edition}-saved`));
    expect(state.toolModelConfig[edition]).toBeNull();
    const remove = renderer.root.findByProps({ 'aria-label': `btn.delete ${edition} Fixture` });
    await act(async () => remove.props.onClick({ stopPropagation }));
    expect(calls('delete_doubao_account')).toEqual([
      ['delete_doubao_account', { edition, accountId: `${edition}-saved` }],
    ]);
    expect(calls('switch_doubao_account')).toHaveLength(0);
  }
);

it.each(editions)(
  '%s router reauthorization uses the embedded login and a ten-minute deadline',
  async (edition) => {
    await mount(edition);
    act(() => useNavigationStore.getState().setActivePage('freeModels'));
    await tick();
    expect(accountProxyStatus(state, edition).accounts[0].id).toBe(`${edition}-saved`);
    expect(calls('start_doubao_login')).toHaveLength(0);
    expect(calls('refresh_doubao_account_quota')).toHaveLength(0);
    let finish!: (value: DoubaoAccount | null) => void;
    handlers.set('start_doubao_login', () => ({
      loginId: 'login',
      expiresAt: Date.now() / 1000 + 600,
    }));
    handlers.set(
      'poll_doubao_login',
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.doubaoAccountGroups[edition].add();
    });
    expect(state.doubaoAccountGroups[edition].remainingSeconds).toBe(600);
    expect(openExternal).not.toHaveBeenCalled();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(61_000);
    });
    expect(state.doubaoAccountGroups[edition].busy).toBe(true);
    expect(state.doubaoAccountGroups[edition].remainingSeconds).toBe(539);
    act(() => useNavigationStore.getState().setActivePage('apps'));
    await tick();
    expect(calls('cancel_doubao_login')).toEqual([['cancel_doubao_login', { loginId: 'login' }]]);
    await act(async () => {
      finish({ ...account(edition), id: 'late' });
      await operation;
    });
    expect(state.doubaoAccountGroups[edition].accounts.some((a) => a.id === 'late')).toBe(false);
    expect(calls('switch_doubao_account')).toHaveLength(0);
    expect(calls('start_tool')).toHaveLength(0);
  }
);

it.each(editions)(
  '%s passive failures keep cached rows without a dialog, while explicit failures are visible',
  async (edition) => {
    await mount(edition);
    handlers.set('list_doubao_accounts', () => {
      throw new Error('accountError.read');
    });
    act(() => useNavigationStore.getState().setActivePage('accounts'));
    await tick();
    expect(state.doubaoAccountGroups[edition].accounts).toEqual([account(edition)]);
    expect(state.applyError).toBeNull();
    await act(async () => {
      await state.doubaoAccountGroups[edition].refresh(account(edition));
    });
    expect(calls('refresh_doubao_account_quota')).toHaveLength(1);
    handlers.set('start_doubao_login', () => {
      throw new Error('accountError.network');
    });
    await act(async () => state.doubaoAccountGroups[edition].add());
    expect(state.applyError).toBe('accountError.network');
  }
);

it.each(editions)(
  '%s ignores a pending switch result after a different account is selected',
  async (edition) => {
    const second = { ...account(edition), id: 'second', name: 'Second', active: false };
    handlers.set('list_doubao_accounts', () => [account(edition), second]);
    await mount(edition);
    let finish!: (value: DoubaoAccount) => void;
    handlers.set(
      'switch_doubao_account',
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.handleLaunch();
    });
    act(() => state.doubaoAccounts.select('second'));
    await act(async () => {
      finish(account(edition));
      await operation;
    });
    expect(state.doubaoAccounts.selectedId).toBe('second');
    expect(calls('start_tool')).toHaveLength(0);
    expect(calls('list_doubao_accounts')).toHaveLength(1);
  }
);

it.each(editions)(
  '%s switching failure or a late result never launches another tool',
  async (edition) => {
    await mount(edition);
    handlers.set('switch_doubao_account', () => {
      throw new Error('accountError.write');
    });
    await act(async () => state.handleLaunch());
    expect(state.applyError).toBe('accountError.write');
    expect(calls('start_tool')).toHaveLength(0);
    let finish!: (value: DoubaoAccount) => void;
    handlers.set(
      'switch_doubao_account',
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    let operation!: Promise<void>;
    await act(async () => {
      operation = state.handleLaunch();
    });
    act(() => state.setSelectedTool(null));
    await act(async () => {
      finish(account(edition));
      await operation;
    });
    expect(calls('start_tool')).toHaveLength(0);
    expect(state.isLaunching).toBe(false);
  }
);
