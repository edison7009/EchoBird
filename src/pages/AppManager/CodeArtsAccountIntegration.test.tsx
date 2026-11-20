import { useLayoutEffect } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { CodeArtsAccount } from '../../api/codeArtsAccounts';
import installIndex from '../../../docs/api/tools/install/index.json';
import { useToolsStore } from '../../stores/toolsStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { AppManagerProvider } from './AppManagerProvider';
import { AppManagerPanel, AppManagerBottom } from './AppManagerComponents';
import { useAppManager } from './context';
import { accountCenterProviders } from '../AccountCenter/accountCenterData';
import { AccountSectionButton, AccountSectionRow } from './AccountSectionPrimitives';
import { BrandDeckCard } from '../../components/BrandDeckCard';
import { accountProxyStatus } from './accountProxyStatus';
import { AccountCard, AccountCenterPanel } from '../AccountCenter/AccountCenter';
import { QuotaCountdown } from './QuotaCountdown';

const { invoke, openExternal, confirm, folderPicker } = vi.hoisted(() => ({
  invoke: vi.fn(),
  openExternal: vi.fn().mockResolvedValue(undefined),
  confirm: vi.fn().mockResolvedValue(true),
  folderPicker: vi.fn().mockResolvedValue('/fixture'),
}));
vi.hoisted(() => vi.stubGlobal('__APP_EDITION__', 'full'));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: folderPicker }));
vi.mock('../../api/tauri', async (original) => ({
  ...(await original<typeof import('../../api/tauri')>()),
  openExternal,
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('../../components/ConfirmDialog', () => ({ useConfirm: () => confirm }));
vi.mock('../../components', () => ({ getModelIcon: () => '/fixture.svg' }));
vi.mock('../ModelNexus/context', () => {
  const userModels: never[] = [];
  return { useModelNexus: () => ({ userModels }) };
});
vi.mock('../FreeModels/provider', () => ({ useFreeModels: () => ({ routerEnabled: false }) }));
vi.mock('./ClaudeCodeLoginDialog', () => ({ ClaudeCodeLoginDialog: () => null }));

const row = (): CodeArtsAccount => ({
  id: 'codearts:account',
  email: 'same@example.test',
  toolId: 'codearts',
  active: true,
  plan: '高级版',
  balance: null,
});
let state: ReturnType<typeof useAppManager>;
let renderer: ReactTestRenderer;
let handlers: Map<string, (args: Record<string, unknown>) => unknown>;
function Harness() {
  const context = useAppManager();
  useLayoutEffect(() => {
    state = context;
  });
  return (
    <>
      <AppManagerPanel />
      <AppManagerBottom />
    </>
  );
}
async function tick(ms = 1) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}
const calls = (name: string) => invoke.mock.calls.filter(([command]) => command === name);
const group = () => state.codeArtsAccounts;
const renderedText = () =>
  renderer.root
    .findAllByType('span')
    .flatMap((node) => node.children.filter((child) => typeof child === 'string'))
    .join(' ');
async function mount(installed = true) {
  useToolsStore.getState().setDetectedTools([
    {
      id: 'codearts',
      name: 'CodeArts',
      category: 'Desktop',
      installed,
      noModelConfig: false,
      launchFile: 'fixture',
    },
  ]);
  await act(async () => {
    renderer = create(
      <AppManagerProvider>
        <Harness />
      </AppManagerProvider>
    );
  });
  act(() => state.setSelectedTool('codearts'));
  await tick();
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-10-18T00:00:00Z'));
  vi.clearAllMocks();
  confirm.mockResolvedValue(true);
  folderPicker.mockResolvedValue('/fixture');
  const storage = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => void storage.set(key, value),
  });
  handlers = new Map<string, (args: Record<string, unknown>) => unknown>([
    ['get_models', () => []],
    ['get_tool_install_index', () => '{"ids":[]}'],
    ['list_codearts_accounts', () => [row()]],
    ['refresh_codearts_account', () => ({ ...row(), balance: 42 })],
    [
      'claim_codearts_credits',
      () => ({
        ...row(),
        balance: 50,
        dailyClaimedAt: Date.now() / 1000,
      }),
    ],
    ['switch_codearts_account', () => row()],
    ['start_tool', () => ({ success: true })],
  ]);
  invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) =>
    handlers.get(command)?.(args ?? {})
  );
  useNavigationStore.setState({ activePage: 'apps' });
});
afterEach(() => {
  act(() => renderer?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('only loads saved desktop accounts during entry and selection', async () => {
  await mount();
  expect(calls('list_codearts_accounts')).toEqual([['list_codearts_accounts']]);
  expect(group().selectedId).toBe(row().id);
  act(() => group().select(row().id));
  for (const command of [
    'refresh_codearts_account',
    'claim_codearts_credits',
    'switch_codearts_account',
    'start_codearts_login',
  ])
    expect(calls(command)).toHaveLength(0);
  expect(openExternal).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
  expect(renderer.root.findAllByType(AccountSectionRow)).toHaveLength(1);
  expect(JSON.stringify(renderer.toJSON())).toContain('model.noUsageData');
});

it('does not load native state or start login when the desktop is uninstalled', async () => {
  await mount(false);
  await act(async () => {
    await group().add();
  });
  expect(calls('list_codearts_accounts')).toHaveLength(0);
  expect(calls('start_codearts_login')).toHaveLength(0);
});

it.each([6498.62, 0])(
  'displays real remaining credits %s and uses the shared empty state only when unknown',
  async (balance) => {
    handlers.set('list_codearts_accounts', () => [{ ...row(), balance }]);
    await mount();
    const renderedRow = renderer.root.findByType(AccountSectionRow);
    expect(renderedRow.props.secondary).toBe(
      `agent.availableCredits ${new Intl.NumberFormat('en', { maximumFractionDigits: 2 }).format(balance)}`
    );
    expect(JSON.stringify(renderer.toJSON())).not.toContain('model.noUsageData');
  }
);

it.each([
  ['Free', 6498.62, 6500, 99.98],
  ['Pro', 6498.62, 6500, 99.98],
  ['Free', 0, 6500, 0],
  ['Free', 6500, 6500, 100],
  ['Free', 7000, 6500, 100],
] as const)(
  'shows actual credits with auxiliary progress independently of plan on the desktop and account card (%s, %s/%s)',
  async (plan, balance, total, percent) => {
    handlers.set('list_codearts_accounts', () => [{ ...row(), plan }]);
    handlers.set('refresh_codearts_account', () => ({ ...row(), plan, balance, total }));
    await mount();
    expect(JSON.stringify(renderer.toJSON())).toContain('model.noUsageData');
    await act(async () => {
      await group().refresh(group().accounts[0]);
    });
    expect(renderer.root.findByType(AccountSectionRow).props.plan).toBe(plan);
    expect(
      renderer.root
        .findAll((node) => node.props.role === 'progressbar')
        .map((node) => node.props['aria-valuenow'])
    ).toEqual([percent]);
    const value = new Intl.NumberFormat('en', { maximumFractionDigits: 2 }).format(balance);
    expect(renderedText()).toContain(value);
    expect(renderedText()).not.toContain(`${percent}%`);
    const provider = accountCenterProviders(state, (key) => key, 'en').find(
      (item) => item.id === 'codearts'
    )!;
    expect(provider.accounts[0].plan).toBe(plan);
    expect(provider.accounts[0].metrics[0]).toMatchObject({
      label: 'agent.availableCredits',
      value,
      percent,
    });
    await act(async () => {
      renderer.update(
        <AppManagerProvider>
          <Harness />
          <AccountCard provider={provider} account={provider.accounts[0]} />
        </AppManagerProvider>
      );
    });
    expect(
      renderer.root
        .findAll((node) => node.props.role === 'progressbar')
        .map((node) => node.props['aria-valuenow'])
    ).toEqual([percent, percent]);
    expect(renderedText()).toContain('agent.availableCredits');
    expect(renderedText()).toContain(value);
    expect(renderedText()).not.toContain(`${percent}%`);
    expect(calls('refresh_codearts_account')).toHaveLength(1);
    for (const command of [
      'start_codearts_login',
      'claim_codearts_credits',
      'switch_codearts_account',
    ])
      expect(calls(command)).toHaveLength(0);
  }
);

it('uses the shared subscription countdown in both plan positions and updates it locally', async () => {
  const subscriptionEndAt = Date.now() / 1000 + (2 * 24 + 5) * 3600;
  const subscribed = { ...row(), plan: 'Pro', balance: 6498.62, total: 6500, subscriptionEndAt };
  handlers.set('list_codearts_accounts', () => [subscribed]);
  await mount();
  const accountRow = renderer.root.findByType(AccountSectionRow);
  expect(accountRow.props.planPrefix.type).toBe(QuotaCountdown);
  expect(accountRow.props.planPrefix.props.label).toBe('accountCenter.subscription');
  expect(calls('refresh_codearts_account')).toHaveLength(0);
  expect(JSON.stringify(renderer.toJSON())).toContain('2d5h');
  const refreshed = { ...subscribed, subscriptionEndAt: subscriptionEndAt + 86400 };
  handlers.set('refresh_codearts_account', () => refreshed);
  await act(async () => {
    await group().refresh(group().accounts[0]);
  });
  const provider = accountCenterProviders(state, (key) => key, 'en').find(
    (item) => item.id === 'codearts'
  )!;
  expect(provider.accounts[0].subscriptionEndAt).toBe(refreshed.subscriptionEndAt);
  await act(async () => {
    renderer.update(
      <AppManagerProvider>
        <Harness />
        <AccountCard provider={provider} account={provider.accounts[0]} />
      </AppManagerProvider>
    );
  });
  expect(renderer.root.findAllByType(QuotaCountdown).map((node) => node.props.resetAt)).toEqual([
    refreshed.subscriptionEndAt,
    refreshed.subscriptionEndAt,
  ]);
  await tick(3600_000);
  expect(
    renderer.root.findAllByProps({ 'aria-label': 'accountCenter.subscription 3d4h' })
  ).toHaveLength(2);
  expect(calls('refresh_codearts_account')).toHaveLength(1);
  handlers.set('refresh_codearts_account', () => {
    throw Error('accountError.network');
  });
  await act(async () => {
    await group().refresh(group().accounts[0]);
  });
  expect(group().accounts[0].subscriptionEndAt).toBe(refreshed.subscriptionEndAt);
  vi.setSystemTime(new Date(refreshed.subscriptionEndAt * 1000));
  await tick(60_000);
  expect(
    renderer.root.findAllByProps({ 'aria-label': 'accountCenter.subscription 0m' })
  ).toHaveLength(0);
  expect(renderedText()).toContain('6,498.62');
  expect(renderedText()).not.toContain('99.98%');
  expect(calls('refresh_codearts_account')).toHaveLength(2);
  for (const command of [
    'start_codearts_login',
    'claim_codearts_credits',
    'switch_codearts_account',
  ])
    expect(calls(command)).toHaveLength(0);
});

it.each([
  ['Free', 1794182400],
  ['Pro', null],
  ['Pro', undefined],
] as const)(
  'omits a subscription countdown for %s without a paid end date (%s)',
  async (plan, subscriptionEndAt) => {
    handlers.set('list_codearts_accounts', () => [
      { ...row(), plan, subscriptionEndAt, balance: 6498.62, total: 6500 },
    ]);
    await mount();
    expect(renderer.root.findByType(AccountSectionRow).props.planPrefix).toBeUndefined();
    const provider = accountCenterProviders(state, (key) => key, 'en').find(
      (item) => item.id === 'codearts'
    )!;
    expect(provider.accounts[0].subscriptionEndAt).toBeUndefined();
    await act(async () => {
      renderer.update(
        <AppManagerProvider>
          <Harness />
          <AccountCard provider={provider} account={provider.accounts[0]} />
        </AppManagerProvider>
      );
    });
    expect(renderer.root.findAllByType(QuotaCountdown)).toHaveLength(0);
    expect(renderedText()).toContain('6,498.62');
    expect(renderedText()).not.toContain('99.98%');
    expect(calls('refresh_codearts_account')).toHaveLength(0);
  }
);

it.each([0, null, undefined])(
  'does not invent a percentage without a positive total (%s)',
  async (total) => {
    handlers.set('list_codearts_accounts', () => [{ ...row(), balance: 12.5, total }]);
    await mount();
    expect(renderer.root.findAll((node) => node.props.role === 'progressbar')).toHaveLength(0);
    const provider = accountCenterProviders(state, (key) => key, 'en').find(
      (item) => item.id === 'codearts'
    )!;
    expect(provider.accounts[0].metrics[0].percent).toBeUndefined();
    expect(provider.accounts[0].metrics[0].value).toBe('12.5');
    expect(provider.accounts[0].metrics[0].label).toBe('agent.availableCredits');
    expect(renderedText()).toContain('agent.availableCredits 12.5');
    expect(calls('refresh_codearts_account')).toHaveLength(0);
  }
);

it.each([null, undefined])(
  'keeps unknown credits unknown despite a known total (%s)',
  async (balance) => {
    handlers.set('list_codearts_accounts', () => [{ ...row(), balance, total: 6500 }]);
    await mount();
    expect(renderer.root.findByType(AccountSectionRow).props.secondary).toBeUndefined();
    const provider = accountCenterProviders(state, (key) => key, 'en').find(
      (item) => item.id === 'codearts'
    )!;
    expect(provider.accounts[0].metrics[0]).toMatchObject({
      label: 'agent.availableCredits',
      value: '—',
      percent: undefined,
    });
    await act(async () => {
      renderer.update(
        <AppManagerProvider>
          <Harness />
          <AccountCard provider={provider} account={provider.accounts[0]} />
        </AppManagerProvider>
      );
    });
    expect(renderer.root.findAllByProps({ role: 'progressbar' })).toHaveLength(0);
    expect(renderedText()).toContain('model.noUsageData');
    expect(calls('refresh_codearts_account')).toHaveLength(0);
  }
);

it('does not treat an installed IDE or CLI as an installed CodeArts desktop', async () => {
  await mount(false);
  act(() =>
    useToolsStore.getState().setDetectedTools([
      { id: 'codearts', name: 'CodeArts', category: 'Desktop', installed: false },
      { id: 'codeartsagent', name: 'Old IDE', category: 'Desktop', installed: true },
      { id: 'codeartscli', name: 'Old CLI', category: 'CLI Code', installed: true },
    ])
  );
  act(() => useNavigationStore.setState({ activePage: 'accounts' }));
  await tick();
  // Saved accounts still list; the backend reads desktop files only when it is installed.
  expect(calls('list_codearts_accounts')).toHaveLength(1);
  const provider = accountCenterProviders(state, (key) => key, 'en').find(
    (item) => item.id === 'codearts'
  )!;
  expect(provider.installed).toBe(false);
  // Its login runs in EchoBird's browser flow, so an uninstalled client can still add.
  handlers.set('start_codearts_login', () => ({
    loginId: 'pending',
    verificationUri: 'https://example.test/authorize',
    expiresAt: Date.now() / 1000 + 60,
  }));
  await act(async () => provider.add());
  await tick();
  expect(calls('start_codearts_login')).toHaveLength(1);
  expect(state.applyError).toBeNull();
});

it('refreshes and claims credits only on explicit actions without applying an account', async () => {
  await mount();
  await act(async () => {
    await group().refresh(row());
  });
  expect(calls('refresh_codearts_account')).toEqual([
    ['refresh_codearts_account', { accountId: row().id }],
  ]);
  expect(group().accounts[0].balance).toBe(42);
  await act(async () => {
    await group().claimDaily(group().accounts[0]);
  });
  expect(calls('claim_codearts_credits')).toEqual([
    ['claim_codearts_credits', { accountId: row().id }],
  ]);
  expect(group().accounts[0].balance).toBe(50);
  expect(state.applyError).toBeNull();
  const reward = renderer.root
    .findAllByType('button')
    .find((button) => button.props['aria-label']?.startsWith('agent.dailyCreditsClaimed'));
  expect(reward).toBeDefined();
  expect(reward!.props.disabled).toBe(true);
  expect(group().selectedId).toBe(row().id);
  expect(calls('switch_codearts_account')).toHaveLength(0);
  expect(calls('apply_model_to_tool')).toHaveLength(0);
});

it('keeps account and API choices exclusive without native writes during selection', async () => {
  await mount();
  act(() => state.handleSelectModel('codearts', 'fixture-api'));
  expect(group().selectedId).toBeNull();
  act(() => group().select(row().id));
  expect(state.toolModelConfig.codearts).toBeNull();
  expect(calls('switch_codearts_account')).toHaveLength(0);
  expect(calls('apply_model_to_tool')).toHaveLength(0);
});

it('applies the desktop account without launching when modify only is selected', async () => {
  await mount();
  act(() =>
    useToolsStore.getState().setDetectedTools([
      {
        id: 'codearts',
        name: 'CodeArts',
        category: 'Desktop',
        installed: true,
        noModelConfig: false,
        launchFile: 'fixture',
        activeModel: 'Existing API Model',
      },
    ])
  );
  act(() => state.setLaunchAfterApply(false));
  await act(async () => {
    await state.handleLaunch();
  });
  expect(calls('switch_codearts_account')).toEqual([
    ['switch_codearts_account', { accountId: row().id }],
  ]);
  expect(calls('start_tool')).toHaveLength(0);
  expect(calls('apply_model_to_tool')).toHaveLength(0);
  expect(calls('restore_tool_to_official')).toHaveLength(0);
  expect(state.detectedTools.find((tool) => tool.id === 'codearts')?.activeModel).toBe(
    'Existing API Model'
  );
});

it('does not switch CodeArts when another application is selected', async () => {
  await mount();
  act(() => useNavigationStore.setState({ activePage: 'accounts' }));
  await tick();
  act(() => state.setSelectedTool('unrelated'));
  await tick();
  expect(group().selectedId).toBe(row().id);
  await act(async () => {
    await state.handleLaunch();
  });
  expect(calls('switch_codearts_account')).toHaveLength(0);
});

it.each(['apps', 'accounts'] as const)(
  '%s uses the standard add-account action without an extra import entry or automatic quota/apply',
  async (page) => {
    handlers.set('start_codearts_login', () => ({
      loginId: 'success',
      verificationUri: 'https://example.test/authorize',
      expiresAt: Date.now() / 1000 + 60,
    }));
    handlers.set('poll_codearts_login', () => ({ ...row(), active: false }));
    await mount();
    if (page === 'accounts') {
      act(() => useNavigationStore.setState({ activePage: 'accounts' }));
      await tick();
      await act(async () => {
        renderer.update(
          <AppManagerProvider>
            <Harness />
            <AccountCenterPanel />
          </AppManagerProvider>
        );
      });
    }
    await act(async () => {
      if (page === 'accounts') {
        const directory = renderer.root
          .findAllByType(BrandDeckCard)
          .find((node) => node.props.name === 'CodeArts')!;
        expect(directory.props.extra).toBeUndefined();
        directory.props.onClick();
      } else {
        const button = renderer.root.findByType(AccountSectionButton);
        expect(button.props.secondary).toBeUndefined();
        button.props.onClick();
      }
    });
    expect(calls('start_codearts_login')).toEqual([['start_codearts_login']]);
    expect(openExternal).toHaveBeenCalledExactlyOnceWith('https://example.test/authorize');
    for (const command of [
      'refresh_codearts_account',
      'claim_codearts_credits',
      'switch_codearts_account',
      'start_tool',
    ])
      expect(calls(command)).toHaveLength(0);
  }
);

it('cancels login on leaving and ignores a late result', async () => {
  let resolve!: (value: CodeArtsAccount) => void;
  handlers.set('start_codearts_login', () => ({
    loginId: 'late',
    verificationUri: 'https://example.test/authorize',
    expiresAt: Date.now() / 1000 + 60,
  }));
  handlers.set(
    'poll_codearts_login',
    () =>
      new Promise<CodeArtsAccount>((done) => {
        resolve = done;
      })
  );
  await mount();
  let task!: Promise<void>;
  await act(async () => {
    task = group().add();
  });
  act(() => useNavigationStore.setState({ activePage: 'models' }));
  await tick();
  expect(calls('cancel_codearts_login')).toEqual([['cancel_codearts_login', { loginId: 'late' }]]);
  await act(async () => {
    resolve({ ...row(), id: 'late-result' });
    await task;
  });
  expect(group().accounts.some((account) => account.id === 'late-result')).toBe(false);
});

it('keeps cached rows and opens no blocking dialog on passive read failure', async () => {
  await mount();
  act(() => useNavigationStore.setState({ activePage: 'models' }));
  await tick();
  handlers.set('list_codearts_accounts', () => {
    throw Error('read failed');
  });
  act(() => useNavigationStore.setState({ activePage: 'apps' }));
  await tick();
  expect(group().accounts).toEqual([row()]);
  expect(state.applyError).toBeNull();
  expect(confirm).not.toHaveBeenCalled();
});

it('isolates nested refresh from selection and prevents duplicate pending actions', async () => {
  let resolve!: (value: CodeArtsAccount) => void;
  handlers.set(
    'refresh_codearts_account',
    () =>
      new Promise<CodeArtsAccount>((done) => {
        resolve = done;
      })
  );
  await mount();
  act(() => group().select(null));
  const button = renderer.root
    .findByType(AccountSectionRow)
    .findAllByType('button')
    .find((item) => item.props['aria-label'].startsWith('agent.refreshAccount'))!;
  const stopPropagation = vi.fn();
  await act(async () => {
    button.props.onClick({ stopPropagation });
  });
  expect(stopPropagation).toHaveBeenCalledOnce();
  expect(group().selectedId).toBeNull();
  expect(renderer.root.findByType(AccountSectionRow).props.refreshing).toBe(true);
  await act(async () => {
    await group().refresh(row());
    await group().claimDaily(row());
  });
  expect(calls('refresh_codearts_account')).toHaveLength(1);
  expect(calls('claim_codearts_credits')).toHaveLength(0);
  await act(async () => {
    resolve({ ...row(), balance: 30 });
  });
  expect(group().accounts[0].balance).toBe(30);
  expect(group().selectedId).toBeNull();
});

it('cancels login after 60 seconds and rejects a late success', async () => {
  let resolve!: (value: CodeArtsAccount) => void;
  handlers.set('start_codearts_login', () => ({
    loginId: 'timeout',
    verificationUri: 'https://example.test/authorize',
    expiresAt: Date.now() / 1000 + 300,
  }));
  handlers.set(
    'poll_codearts_login',
    () =>
      new Promise<CodeArtsAccount>((done) => {
        resolve = done;
      })
  );
  await mount();
  let task!: Promise<void>;
  await act(async () => {
    task = group().add();
  });
  await tick(60_000);
  expect(group().busy).toBe(false);
  expect(state.applyError).toBe('accountError.expired');
  expect(calls('cancel_codearts_login')).toEqual([
    ['cancel_codearts_login', { loginId: 'timeout' }],
  ]);
  await act(async () => {
    resolve({ ...row(), id: 'late-result' });
    await task;
  });
  expect(group().accounts.some((account) => account.id === 'late-result')).toBe(false);
});

it('keeps delete confirmation and preserves the account on explicit failure', async () => {
  await mount();
  confirm.mockResolvedValueOnce(false);
  await act(async () => {
    await group().remove(row());
  });
  expect(calls('delete_codearts_account')).toHaveLength(0);
  handlers.set('delete_codearts_account', () => {
    throw Error('accountError.write');
  });
  await act(async () => {
    await group().remove(row());
  });
  expect(confirm).toHaveBeenCalledTimes(2);
  expect(calls('delete_codearts_account')).toEqual([
    ['delete_codearts_account', { accountId: row().id }],
  ]);
  expect(group().accounts).toEqual([row()]);
  expect(state.applyError).toBe('accountError.write');
});

it('shows one CodeArts account system in Account Center', async () => {
  await mount();
  act(() => useNavigationStore.setState({ activePage: 'accounts' }));
  await tick();
  const providers = accountCenterProviders(state, (key) => key, 'en').filter((provider) =>
    provider.id.startsWith('codearts')
  );
  expect(providers).toHaveLength(1);
  expect(providers[0].name).toBe('CodeArts');
  expect(providers[0].accounts[0].metrics[0].value).toBe('—');
  expect(accountProxyStatus(state, 'codeArts').add).toBe(group().add);
});

it('reauthorizes the desktop from Smart Access with no passive remote actions', async () => {
  await mount();
  act(() => useNavigationStore.setState({ activePage: 'freeModels' }));
  await tick();
  expect(calls('start_codearts_login')).toHaveLength(0);
  expect(calls('refresh_codearts_account')).toHaveLength(0);
  handlers.set('start_codearts_login', () => ({
    loginId: 'reauthorize',
    verificationUri: 'https://example.test/authorize',
    expiresAt: Date.now() / 1000 + 60,
  }));
  handlers.set('poll_codearts_login', () => row());
  await act(async () => {
    await accountProxyStatus(state, 'codeArts').add();
  });
  expect(calls('start_codearts_login')).toHaveLength(1);
  expect(openExternal).toHaveBeenCalledOnce();
  expect(calls('switch_codearts_account')).toHaveLength(0);
});

it('excludes the IDE and CLI from the install catalog', () => {
  expect(installIndex.ids).toContain('codearts');
  expect(installIndex.ids).not.toContain('codeartsagent');
  expect(installIndex.ids).not.toContain('codeartscli');
});
