import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { ModelConfig } from '../../api/types';
import {
  ACCOUNT_NAME_MASK,
  useAccountPrivacyStore as privacy,
} from '../../stores/accountPrivacyStore';
import { useNavigationStore as navigation } from '../../stores/navigationStore';
import { useRewardClaimStore } from '../../stores/rewardClaimStore';
import {
  USAGE_ISLAND_LIMIT,
  useUsageIslandStore as island,
  usageIslandVisible,
  type UsageIslandStyle,
} from '../../stores/usageIslandStore';
import type { AccountCardData, AccountProvider } from '../AccountCenter/accountCenterData';
import { UsageIslandMain, UsageIslandPanel } from './UsageIsland';
import { UsageIslandView } from './UsageIslandView';
import {
  accountSources,
  accountSourceKey,
  modelSourceKey,
  modelSources,
  type UsageIslandItem,
} from './usageIslandData';

const refreshAccount = vi.hoisted(() => vi.fn(async () => {}));
const refreshModel = vi.hoisted(() => vi.fn(async () => {}));
const showToast = vi.hoisted(() => vi.fn());
const testProviders = vi.hoisted(() => ({ current: [] as unknown[] }));
const systemDark = vi.hoisted(() => ({ current: true }));

vi.mock('react-dom', () => ({ createPortal: (children: unknown) => children }));
vi.mock('@dnd-kit/core', async (original) => ({
  ...(await original<typeof import('@dnd-kit/core')>()),
  DndContext: ({ children }: { children: unknown }) => children,
  DragOverlay: ({ children }: { children: unknown }) => children,
}));
vi.mock('@dnd-kit/sortable', async (original) => ({
  ...(await original<typeof import('@dnd-kit/sortable')>()),
  SortableContext: ({ children }: { children: unknown }) => children,
  useSortable: () => ({
    attributes: { tabIndex: 0 },
    listeners: {},
    setNodeRef: vi.fn(),
    setActivatorNodeRef: vi.fn(),
    transform: null,
    transition: undefined,
    isDragging: false,
  }),
}));
vi.mock('../../hooks/useI18n', () => ({
  useI18n: () => ({ t: (key: string) => key, locale: 'en' }),
}));
vi.mock('../../hooks/useSystemDark', () => ({ useSystemDark: () => systemDark.current }));
vi.mock('../../stores/themeStore', () => ({
  useThemeStore: (select: (state: { resolved: string }) => unknown) => select({ resolved: 'dark' }),
}));
vi.mock('../../components/cards', () => ({ getModelIcon: () => '/icons/models/test.svg' }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast }) }));
vi.mock('../../components/MiniSelect', () => ({ MiniSelect: () => null }));
vi.mock('../AppManager/context', () => ({ useAppManager: () => ({}) }));
vi.mock('../AccountCenter/accountCenterData', () => ({
  accountCenterProviders: () => testProviders.current,
}));
vi.mock('../ModelNexus/context', () => ({
  useModelNexus: () => ({
    userModels: [
      { internalId: 'model-1', name: 'Model One', modelId: 'one' },
      { internalId: 'local-server', name: 'Local', modelId: 'local' },
    ],
    modelUsageData: {},
    refreshingUsageIds: new Set(),
    refreshSingleUsage: refreshModel,
  }),
}));

const account = (overrides: Partial<AccountCardData> = {}): AccountCardData => ({
  id: 'a',
  identity: 'me@example.com',
  metrics: [{ label: 'Quota', value: '42%', percent: 42 }],
  refreshing: false,
  authorizationFailed: false,
  refresh: refreshAccount,
  remove: () => {},
  ...overrides,
});
const provider = (overrides: Partial<AccountProvider> = {}): AccountProvider => ({
  id: 'claude',
  name: 'Claude',
  icon: '/icons/tools/claudecode.svg',
  installed: true,
  busy: false,
  loading: false,
  loaded: true,
  remainingSeconds: 0,
  add: () => {},
  cancelLogin: () => {},
  accounts: [account()],
  ...overrides,
});

let view: ReactTestRenderer | undefined;
const render = async () => {
  await act(async () => {
    view = create(
      <>
        <UsageIslandMain />
        <UsageIslandPanel />
      </>
    );
  });
  return view!;
};
const renderView = async (style: UsageIslandStyle, items: UsageIslandItem[]) => {
  const { theme } = island.getState();
  await act(async () => {
    view = create(
      <UsageIslandView items={items} settings={{ style, theme, expandMode: 'always' }} />
    );
  });
};
const accountItems = () => accountSources(testProviders.current as AccountProvider[], {});
const button = (label: string) =>
  view!.root.findAll((node) => node.type === 'button' && node.props['aria-label'] === label)[0];
const buttonWithText = (text: string) =>
  view!.root.findAll(
    (node) => node.type === 'button' && [node.props.children].flat().includes(text)
  )[0];

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: vi.fn() });
  vi.stubGlobal('document', { addEventListener: vi.fn(), removeEventListener: vi.fn() });
  vi.clearAllMocks();
  testProviders.current = [provider()];
  systemDark.current = true;
  navigation.setState(navigation.getInitialState(), true);
  useRewardClaimStore.setState({ running: false });
  privacy.setState({ hidden: false });
  island.setState({
    enabled: false,
    pick: null,
    style: 'row',
    theme: 'dark',
    expandMode: 'hover',
    refreshMinutes: 0,
    sources: [],
  });
  navigation.setState({
    developerMode: true,
    activePage: 'developer',
    developerTab: 'usageIsland',
  });
});

afterEach(() => {
  act(() => view?.unmount());
  view = undefined;
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it('previews demo data on a fixed stage whatever sources are selected', async () => {
  island.setState({ sources: [accountSourceKey('claude', 'a')] });
  await render();
  const shown = () =>
    view!.root
      .findAll(
        (node) => typeof node.type === 'string' && node.props['data-usage-source'] !== undefined
      )
      .map((node) => node.props['data-usage-source']);
  expect(shown()).toEqual(['demo:chatgpt', 'demo:claude', 'demo:deepseek']);
  await act(async () => island.getState().update({ style: 'column' }));
  expect(shown()).toEqual(['demo:chatgpt', 'demo:claude']);
  await act(async () => island.getState().update({ style: 'dial' }));
  expect(shown()).toHaveLength(6);
  await act(async () => island.getState().update({ style: 'line' }));
  expect(shown()).toEqual(['demo:chatgpt', 'demo:claude']);
});

it('fills six numbered slots in order and removes a source from its card', async () => {
  island.setState({
    sources: [accountSourceKey('claude', 'a'), modelSourceKey('model-1')],
  });
  await render();
  const slots = view!.root.findByProps({ className: 'usage-island-slots' });
  expect(
    slots.findAll((node) => node.type === 'div' && node.props.role === 'listitem')
  ).toHaveLength(USAGE_ISLAND_LIMIT);
  const remove = () =>
    slots.findAll(
      (node) => node.type === 'button' && node.props['aria-label'] === 'usageIsland.removeSource'
    );
  expect(remove()).toHaveLength(2);
  await act(async () => remove()[0].props.onClick());
  expect(island.getState().sources).toEqual([modelSourceKey('model-1')]);
  expect(refreshAccount).not.toHaveBeenCalled();
  expect(refreshModel).not.toHaveBeenCalled();
});

it('does not query quota on entry, tab switches or when a source is added', async () => {
  await render();
  await act(async () => button('Claude — me@example.com').props.onClick());
  await act(async () => navigation.getState().setDeveloperTab('statistics'));
  await act(async () => navigation.getState().setDeveloperTab('usageIsland'));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(24 * 60 * 60_000);
  });
  expect(island.getState().sources).toEqual([accountSourceKey('claude', 'a')]);
  expect(refreshAccount).not.toHaveBeenCalled();
  expect(refreshModel).not.toHaveBeenCalled();
});

it('caps the island at six sources and lets a missing source be removed', async () => {
  const keys = Array.from({ length: USAGE_ISLAND_LIMIT }, (_, index) => `model:gone-${index}`);
  island.setState({ sources: keys });
  await render();
  act(() => island.getState().toggleSource(accountSourceKey('claude', 'a')));
  expect(island.getState().sources).toEqual(keys);
  expect(button('Claude — me@example.com').props.disabled).toBe(true);
  await act(async () => buttonWithText('usageIsland.models').props.onClick());
  await act(async () => button('voice.missingSource — ').props.onClick());
  expect(island.getState().sources).toHaveLength(USAGE_ISLAND_LIMIT - 1);
});

it('refreshes selected sources once on request and reports failures', async () => {
  refreshAccount.mockRejectedValueOnce(new Error('offline'));
  island.setState({
    sources: [accountSourceKey('claude', 'a'), modelSourceKey('model-1')],
  });
  await render();
  await act(async () => buttonWithText('accountCenter.refreshAll').props.onClick());
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  expect(refreshModel).toHaveBeenCalledExactlyOnceWith('model-1', { silent: false });
  expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'accountCenter.batchRefreshFailed');
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('refreshes on the chosen interval only while the page is open and stays quiet on failure', async () => {
  refreshAccount.mockRejectedValue(new Error('offline'));
  island.setState({ sources: [accountSourceKey('claude', 'a')], refreshMinutes: 5 });
  await render();
  expect(refreshAccount).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5 * 60_000);
  });
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  expect(showToast).not.toHaveBeenCalled();
  await act(async () => navigation.getState().setDeveloperTab('pets'));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(60 * 60_000);
  });
  expect(refreshAccount).toHaveBeenCalledTimes(1);
});

it('keeps unknown quota unknown instead of reporting zero', () => {
  const [source] = accountSources(
    [provider({ accounts: [account({ metrics: [{ label: 'Quota', value: '—' }] })] })],
    {}
  );
  expect(source).toMatchObject({ percent: null, value: '—' });
  const [model] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    {},
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(model).toMatchObject({ percent: null, value: '—' });
});

it('shows remaining allowance like the account page and leaves balances alone', async () => {
  testProviders.current = [
    provider({
      accounts: [account({ metrics: [{ label: 'Five hour', value: '100%', percent: 100 }] })],
    }),
  ];
  await renderView('line', accountItems());
  const quota = view!.root.findByProps({ className: 'usage-island-line' });
  expect(quota.props['data-tone']).toBe('healthy');
  expect(quota.findByProps({ className: 'usage-island-line-value' }).props.children).toBe('100%');
  expect(quota.findByProps({ className: 'usage-island-line-track' }).children[0]).toMatchObject({
    props: { style: { width: '100%' } },
  });
  const [model] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    { m: { quotas: [{ percentage: 0, resetAt: 0, balance: 12.5 }] } },
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(model).toMatchObject({ percent: null, value: '12.50' });
});

it('keeps two quota lines with reset times for accounts and models', () => {
  const [source] = accountSources(
    [
      provider({
        accounts: [
          account({
            metrics: [
              { label: 'Five hour', value: '83%', percent: 83, resetAt: 1_000 },
              { label: 'Weekly', value: '38%', percent: 38, resetAt: 2_000 },
              { label: 'Monthly', value: '10%', percent: 10 },
            ],
          }),
        ],
      }),
    ],
    {}
  );
  expect(source.lines).toEqual([
    { label: 'Five hour', percent: 83, value: '83%', resetAt: 1_000 },
    { label: 'Weekly', percent: 38, value: '38%', resetAt: 2_000 },
  ]);
  const [model] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    {
      m: {
        quotas: [
          { percentage: 40, resetAt: 9_000_000, period: 'weekly' },
          { percentage: 20, resetAt: 3_000_000, period: 'fiveHour' },
        ],
      },
    },
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(model.lines).toEqual([
    { label: 'model.quota.fiveHour', percent: 80, value: '80%', resetAt: 3_000 },
    { label: 'model.quota.weekly', percent: 60, value: '60%', resetAt: 9_000 },
  ]);
  expect(model).toMatchObject({ percent: 80, value: '80%' });
});

it('renders one bar per quota line in the line style and none for a balance', async () => {
  testProviders.current = [
    provider({
      accounts: [
        account({
          metrics: [
            { label: 'Five hour', value: '83%', percent: 83 },
            { label: 'Weekly', value: '38%', percent: 38 },
          ],
        }),
      ],
    }),
    provider({
      id: 'kimi',
      name: 'Kimi',
      accounts: [account({ id: 'b', metrics: [{ label: 'Balance', value: '$12.40' }] })],
    }),
  ];
  await renderView('line', accountItems());
  const bars = (key: string) =>
    view!.root
      .findByProps({ 'data-usage-source': key })
      .findAll((node) => node.props.className === 'usage-island-line-track');
  expect(bars(accountSourceKey('claude', 'a'))).toHaveLength(2);
  expect(bars(accountSourceKey('kimi', 'b'))).toHaveLength(0);
});

it('names each source above its quota in the line style', async () => {
  await renderView('line', [
    ...accountItems(),
    ...modelSources(
      [{ internalId: 'model-1', name: 'Model One', modelId: 'one' } as ModelConfig],
      {},
      new Set(),
      refreshModel,
      (key) => key
    ),
  ]);
  const name = (key: string) =>
    view!.root
      .findByProps({ 'data-usage-source': key })
      .findByProps({ className: 'usage-island-line-name' }).props.children;
  expect(name(accountSourceKey('claude', 'a'))).toBe('me@example.com');
  expect(name(modelSourceKey('model-1'))).toBe('Model One');
  await act(async () => privacy.setState({ hidden: true }));
  expect(name(accountSourceKey('claude', 'a'))).toBe(ACCOUNT_NAME_MASK);
  expect(name(modelSourceKey('model-1'))).toBe('Model One');
});

it('follows the system appearance independently of the app theme', async () => {
  island.setState({ theme: 'system' });
  systemDark.current = false;
  await render();
  const surface = () =>
    view!.root.findAll(
      (node) => node.type === 'div' && String(node.props.className).startsWith('usage-island ')
    )[0].props.className;
  expect(surface()).toContain('usage-island-theme-light');
  expect(surface()).not.toContain('usage-island-theme-auto');
  await act(async () => island.getState().update({ theme: 'auto' }));
  expect(surface()).toContain('usage-island-theme-dark');
  expect(surface()).toContain('usage-island-theme-auto');
});

it('turns the logo into the picked ring in the horizontal and vertical styles only', async () => {
  const onPick = vi.fn();
  const items = accountItems();
  const key = accountSourceKey('claude', 'a');
  const mount = async (style: UsageIslandStyle, pick: string | null) => {
    await act(async () => {
      view = create(
        <UsageIslandView
          items={items}
          settings={{ style, theme: 'dark', expandMode: 'always' }}
          pick={pick}
          onPick={onPick}
        />
      );
    });
  };
  const logo = () => view!.root.findByProps({ className: 'usage-island-logo' });
  await mount('row', null);
  expect(logo().findAllByType('img')).toHaveLength(1);
  await act(async () => view!.root.findByProps({ 'data-usage-source': key }).props.onClick());
  expect(onPick).toHaveBeenCalledExactlyOnceWith(key);
  await act(async () => view!.unmount());
  await mount('column', key);
  expect(logo().props['data-tone']).toBe('healthy');
  expect(logo().findAll((node) => node.props.className === 'usage-island-gauge')).toHaveLength(1);
  await act(async () => view!.root.findByProps({ 'data-usage-source': key }).props.onClick());
  expect(onPick).toHaveBeenLastCalledWith(null);
  await act(async () => view!.unmount());
  for (const style of ['line', 'dial'] as const) {
    await mount(style, key);
    expect(logo().findAllByType('img')).toHaveLength(1);
    await act(async () => view!.unmount());
  }
  view = undefined;
});

it('needs developer mode, the switch and at least one source to show the window', async () => {
  const visible = () => usageIslandVisible();
  expect(visible()).toBe(false);
  island.setState({ sources: [modelSourceKey('model-1')] });
  expect(visible()).toBe(false);
  island.setState({ enabled: true });
  expect(visible()).toBe(true);
  navigation.setState({ developerMode: false });
  expect(visible()).toBe(false);
  navigation.setState({ developerMode: true });
  island.setState({ sources: [] });
  expect(visible()).toBe(false);
});

it('hands the timer to the floating window while it is open', async () => {
  island.setState({
    enabled: true,
    sources: [accountSourceKey('claude', 'a')],
    refreshMinutes: 5,
  });
  await render();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10 * 60_000);
  });
  expect(refreshAccount).not.toHaveBeenCalled();
});

it('uses cached rows only until the saved accounts have been read', () => {
  const cached = {
    claude: [
      {
        id: 'cached',
        identity: 'old@example.com',
        metrics: [{ label: 'Quota', value: '10%', percent: 10 }],
        authorizationFailed: false,
        claimable: false,
      },
    ],
  };
  const loading = accountSources(
    [provider({ loading: true, loaded: false, accounts: [] })],
    cached
  );
  expect(loading.map(({ detail, refresh }) => [detail, refresh])).toEqual([
    ['old@example.com', undefined],
  ]);
  expect(accountSources([provider({ readFailed: true, accounts: [] })], cached)).toHaveLength(1);
  expect(accountSources([provider({ accounts: [] })], cached)).toEqual([]);
});

it('marks busy or auth-failed accounts as not refreshable or unknown', () => {
  const [busy] = accountSources([provider({ busy: true })], {});
  expect(busy.refresh).toBeUndefined();
  const [failed] = accountSources(
    [provider({ accounts: [account({ authorizationFailed: true })] })],
    {}
  );
  expect(failed).toMatchObject({ percent: null, value: '—' });
});
