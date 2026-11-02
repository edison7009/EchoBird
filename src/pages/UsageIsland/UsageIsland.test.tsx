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
import { USAGE_ISLAND_TICK_EVENT } from '../../api/usageIsland';
import { UsageIslandMain, UsageIslandPanel } from './UsageIsland';
import UsageIslandBridge from './UsageIslandBridge';
import { UsageIslandView, type IslandLayout } from './UsageIslandView';
import { UsageIslandWindow } from './UsageIslandWindow';
import {
  accountSources,
  accountSourceKey,
  compactValue,
  modelSourceKey,
  modelSources,
  type UsageIslandItem,
} from './usageIslandData';

const refreshAccount = vi.hoisted(() => vi.fn(async () => {}));
const refreshModel = vi.hoisted(() => vi.fn(async () => {}));
const showToast = vi.hoisted(() => vi.fn());
const testProviders = vi.hoisted(() => ({ current: [] as unknown[] }));
const systemDark = vi.hoisted(() => ({ current: true }));
const events = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  emitTo: vi.fn(async (..._args: unknown[]) => {}),
}));
const setZoom = vi.hoisted(() => vi.fn(async () => {}));

vi.mock('@tauri-apps/api/webview', () => ({ getCurrentWebview: () => ({ setZoom }) }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ show: vi.fn(async () => {}) }),
}));
vi.mock('@tauri-apps/api/event', () => ({
  emit: vi.fn(async () => {}),
  emitTo: events.emitTo,
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    events.listeners.set(name, handler);
    return () => events.listeners.delete(name);
  }),
}));

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
  THEME_CHANGED_EVENT: 'theme-changed',
  THEME_REQUEST_EVENT: 'theme-request',
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
      <UsageIslandView
        items={items}
        settings={{ style, reversed: [], theme, expandMode: 'always' }}
      />
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
    reversed: [],
    theme: 'dark',
    expandMode: 'hover',
    refreshSeconds: 0,
    scale: 100,
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
  expect(refreshModel).toHaveBeenCalledExactlyOnceWith(
    'model-1',
    expect.objectContaining({ silent: false })
  );
  expect(showToast).toHaveBeenCalledExactlyOnceWith('error', 'accountCenter.batchRefreshFailed');
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('refreshes on the chosen interval only while the page is open and stays quiet on failure', async () => {
  refreshAccount.mockRejectedValue(new Error('offline'));
  island.setState({ sources: [accountSourceKey('claude', 'a')], refreshSeconds: 300 });
  await render();
  expect(refreshAccount).not.toHaveBeenCalled();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300_000);
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
  expect(model.lines[0].label).toBe('model.balance');
  const [spent] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    { m: { quotas: [{ percentage: 0, resetAt: 0, balance: 3, spent: true }] } },
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(spent.lines[0].label).toBe('model.quota.used');
  expect(spent).toMatchObject({ percent: null, value: '-3.00' });
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

it('turns the logo into the picked ring in every style', async () => {
  const onPick = vi.fn();
  const items = accountItems();
  const key = accountSourceKey('claude', 'a');
  const mount = async (style: UsageIslandStyle, pick: string | null) => {
    await act(async () => {
      view = create(
        <UsageIslandView
          items={items}
          settings={{ style, reversed: [], theme: 'dark', expandMode: 'always' }}
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
  await mount('dial', key);
  expect(logo().findAll((node) => node.props.className === 'usage-island-gauge')).toHaveLength(1);
  await act(async () => view!.root.findByProps({ 'data-usage-source': key }).props.onClick());
  expect(onPick).toHaveBeenLastCalledWith(null);
  await act(async () => view!.unmount());
  await mount('line', key);
  expect(logo().findAll((node) => node.props.className === 'usage-island-gauge')).toHaveLength(1);
  await act(async () => view!.root.findByProps({ 'data-usage-source': key }).props.onClick());
  expect(onPick).toHaveBeenLastCalledWith(null);
  await act(async () => view!.unmount());
  view = undefined;
});

it('reverses one style at a time without changing the selected style', async () => {
  await render();
  const direction = (name: string, label: string) =>
    view!.root
      .findByProps({ 'aria-label': `usageIsland.style.${name}`, role: 'group' })
      .findByProps({ 'aria-label': `usageIsland.direction.${label}` });
  await act(async () => direction('column', 'up').props.onClick());
  expect(island.getState()).toMatchObject({ style: 'row', reversed: ['column'] });
  expect(direction('column', 'up').props['aria-pressed']).toBe(true);
  expect(direction('column', 'down').props['aria-pressed']).toBe(false);
  expect(direction('row', 'right').props['aria-pressed']).toBe(true);
  await act(async () => direction('column', 'up').props.onClick());
  expect(island.getState().reversed).toEqual(['column']);
  const flags = () =>
    view!.root.findByProps({ className: 'usage-island-fit' }).props['data-reversed'];
  expect(flags()).toBe(false);
  await act(async () => island.getState().update({ style: 'column' }));
  expect(flags()).toBe(true);
  await act(async () => direction('column', 'down').props.onClick());
  expect(island.getState().reversed).toEqual([]);
});

it('keeps the logo in place while the window resizes in either direction', async () => {
  const anchors: [number, number][] = [];
  const onLayout = (layout: IslandLayout) => anchors.push([layout.anchorX, layout.anchorY]);
  const cases: [UsageIslandStyle, boolean, [number, number]][] = [
    ['row', false, [0, 0]],
    ['row', true, [1, 0]],
    ['line', true, [1, 0]],
    ['column', false, [0, 0]],
    ['column', true, [0, 1]],
    ['dial', false, [0.5, 0.5]],
    ['dial', true, [0.5, 0.5]],
  ];
  for (const [style, reversed, expected] of cases) {
    await act(async () => {
      view = create(
        <UsageIslandView
          items={accountItems()}
          settings={{
            style,
            reversed: reversed ? [style] : [],
            theme: 'dark',
            expandMode: 'always',
          }}
          onLayout={onLayout}
        />
      );
    });
    expect(anchors[anchors.length - 1]).toEqual(expected);
    await act(async () => view!.unmount());
  }
  view = undefined;
});

it('keeps the demo open whatever the floating island expand mode is', async () => {
  island.setState({ expandMode: 'hover' });
  await render();
  const shell = () =>
    view!.root.findAll(
      (node) => node.type === 'div' && String(node.props.className).startsWith('usage-island ')
    )[0];
  expect(shell().props['data-expanded']).toBe(true);
  await act(async () => island.getState().update({ expandMode: 'click', theme: 'light' }));
  expect(shell().props['data-expanded']).toBe(true);
  expect(shell().props.className).toContain('usage-island-theme-light');
});

it('moves up a unit when rounding would print 1000K or 1000M', () => {
  expect(['999,949', '999,990', '999,990,000'].map(compactValue)).toEqual(['999.9K', '1M', '1B']);
});

it('keeps every wallet on the island when an account holds two', () => {
  const [model] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    {
      m: {
        quotas: [
          { percentage: 0, resetAt: 0, balance: 5, balanceUnit: 'CNY' },
          { percentage: 0, resetAt: 0, balance: 2, balanceUnit: 'USD' },
        ],
      },
    },
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(model.lines.map((line) => line.value)).toEqual(['5.00', '2.00']);
});

it('drops a click pin once the island follows hover', async () => {
  const shell = () =>
    view!.root.findAll(
      (node) => node.type === 'div' && String(node.props.className).startsWith('usage-island ')
    )[0];
  const mount = (expandMode: 'click' | 'hover') => (
    <UsageIslandView
      items={accountItems()}
      settings={{ style: 'row', reversed: [], theme: 'dark', expandMode }}
      hover={false}
    />
  );
  await act(async () => {
    view = create(mount('click'));
  });
  await act(async () => button('usageIsland.expand').props.onClick());
  expect(shell().props['data-expanded']).toBe(true);
  await act(async () => view!.update(mount('hover')));
  expect(shell().props['data-expanded']).toBe(false);
});

it('keeps the scale of token balances', () => {
  const [model] = modelSources(
    [{ internalId: 'm', name: 'M', modelId: 'm' } as ModelConfig],
    { m: { quotas: [{ percentage: 0, resetAt: 0, balance: 1.5, balanceUnit: 'M Tokens' }] } },
    new Set(),
    refreshModel,
    (key) => key
  );
  expect(model.value).toBe('1.50M');
  expect(compactValue(model.value)).toBe('1.50M');
});

it('rests a failing source between timed refreshes but not on request', async () => {
  refreshAccount.mockImplementation(async () => {});
  refreshAccount.mockRejectedValueOnce(new Error('rate limited'));
  island.setState({ sources: [accountSourceKey('claude', 'a')], refreshSeconds: 300 });
  await render();
  const advance = async (ms: number) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  };
  await advance(300_000);
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  await advance(300_000);
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  await act(async () => buttonWithText('accountCenter.refreshAll').props.onClick());
  expect(refreshAccount).toHaveBeenCalledTimes(2);
  await advance(300_000);
  expect(refreshAccount).toHaveBeenCalledTimes(3);
});

it('lets timed refreshes give way to the reward lock without holding it', async () => {
  island.setState({ sources: [accountSourceKey('claude', 'a')], refreshSeconds: 300 });
  let locked = false;
  refreshAccount.mockImplementationOnce(async () => {
    locked = useRewardClaimStore.getState().running;
  });
  await render();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300_000);
  });
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  expect(locked).toBe(false);
  useRewardClaimStore.setState({ running: true });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(300_000);
  });
  expect(refreshAccount).toHaveBeenCalledTimes(1);
});

it('leaves account names to a host that already masked them', async () => {
  privacy.setState({ hidden: true });
  await act(async () => {
    view = create(
      <UsageIslandView
        items={accountItems()}
        settings={{ style: 'line', reversed: [], theme: 'dark', expandMode: 'always' }}
        masked
      />
    );
  });
  expect(view!.root.findByProps({ className: 'usage-island-line-name' }).props.children).toBe(
    'me@example.com'
  );
});

it('shortens long balances for the logo and leaves short or unknown values alone', () => {
  expect(
    [
      '12.40',
      '$12.40',
      '999.6',
      '2,000',
      '500',
      '1,250',
      '$12,400',
      '3,400,000',
      '2.5B',
      '—',
      'N/A',
    ].map(compactValue)
  ).toEqual(['12.40', '$12.40', '1000', '2K', '500', '1.3K', '$12.4K', '3.4M', '2.5B', '—', 'N/A']);
});

it('shows the icon and value instead of an empty ring for a picked balance', async () => {
  testProviders.current = [
    provider({ accounts: [account({ metrics: [{ label: 'Balance', value: '2,000' }] })] }),
  ];
  const [item] = accountItems();
  await act(async () => {
    view = create(
      <UsageIslandView
        items={[item]}
        settings={{ style: 'row', reversed: [], theme: 'dark', expandMode: 'always' }}
        pick={item.key}
      />
    );
  });
  const logo = view!.root.findByProps({ className: 'usage-island-logo' });
  expect(logo.findAll((node) => node.props.className === 'usage-island-gauge')).toEqual([]);
  expect(logo.findAll((node) => node.type === 'span' && node.children[0] === '2K')).toHaveLength(1);
});

it('never lets an island image start a native drag', async () => {
  for (const style of ['row', 'column', 'dial', 'line'] as const) {
    await renderView(style, accountItems());
    const images = view!.root.findAllByType('img');
    expect(images.length).toBeGreaterThan(1);
    expect(images.every((image) => image.props.draggable === false)).toBe(true);
    await act(async () => view!.unmount());
  }
  view = undefined;
});

it('lets a host that tracks the cursor decide hover', async () => {
  const mount = async (hover: boolean | null) => {
    await act(async () => {
      view = create(
        <UsageIslandView
          items={accountItems()}
          settings={{ style: 'row', reversed: [], theme: 'dark', expandMode: 'hover' }}
          hover={hover}
        />
      );
    });
    const state = view!.root.findAll(
      (node) => node.type === 'div' && String(node.props.className).startsWith('usage-island ')
    )[0].props['data-expanded'];
    await act(async () => view!.unmount());
    return state;
  };
  expect(await mount(true)).toBe(true);
  expect(await mount(false)).toBe(false);
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
    refreshSeconds: 300,
  });
  await render();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10 * 60_000);
  });
  expect(refreshAccount).not.toHaveBeenCalled();
});

it('refreshes on ticks from the floating window instead of its own timer', async () => {
  island.setState({
    enabled: true,
    sources: [accountSourceKey('claude', 'a')],
    refreshSeconds: 300,
  });
  navigation.setState({ activePage: 'apps' });
  await act(async () => {
    view = create(<UsageIslandBridge />);
  });
  const advance = async (ms: number) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  };
  await advance(5_000);
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  await advance(20 * 60_000);
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  const states = events.emitTo.mock.calls.filter((call) => call[1] === 'usage-island-state');
  expect(JSON.parse(states[states.length - 1][2] as string)).toMatchObject({
    refreshSeconds: 300,
  });
  await act(async () => events.listeners.get(USAGE_ISLAND_TICK_EVENT)!({ payload: {} }));
  expect(refreshAccount).toHaveBeenCalledTimes(2);
});

it('sizes the preview and saves the choice from an unlabelled slider', async () => {
  await render();
  const slider = view!.root.findByProps({ type: 'range' });
  expect(slider.props).toMatchObject({ min: 70, max: 150, step: 10, value: 100 });
  expect(slider.parent!.children).toHaveLength(1);
  await act(async () => slider.props.onChange({ target: { value: '130' } }));
  expect(island.getState().scale).toBe(130);
  expect(view!.root.findByProps({ className: 'usage-island-preview' }).props.style).toEqual({
    '--usage-island-scale': 1.3,
  });
  expect(JSON.parse(vi.mocked(localStorage.setItem).mock.lastCall![1])).toMatchObject({
    scale: 130,
  });
  expect(refreshAccount).not.toHaveBeenCalled();
});

it('sends the island scale to the floating window, which zooms by it alone', async () => {
  island.setState({ enabled: true, sources: [accountSourceKey('claude', 'a')], scale: 120 });
  navigation.setState({ activePage: 'apps' });
  await act(async () => {
    view = create(<UsageIslandBridge />);
  });
  const states = events.emitTo.mock.calls.filter((call) => call[1] === 'usage-island-state');
  const payload = states[states.length - 1][2] as string;
  expect(JSON.parse(payload)).toMatchObject({ scale: 120 });
  act(() => view!.unmount());
  vi.stubGlobal('window', { screen: { availWidth: 1920 } });
  await act(async () => {
    view = create(<UsageIslandWindow />);
  });
  expect(setZoom).not.toHaveBeenCalled();
  await act(async () => events.listeners.get('usage-island-state')!({ payload }));
  expect(setZoom).toHaveBeenCalledExactlyOnceWith(1.2);
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
