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
  usageIslandScales,
  usageIslandStyles,
  useUsageIslandStore as island,
  usageIslandVisible,
  type UsageIslandStyle,
} from '../../stores/usageIslandStore';
import type { AccountCardData, AccountProvider } from '../AccountCenter/accountCenterData';
import {
  USAGE_ISLAND_TICK_EVENT,
  resizeUsageIsland,
  setUsageIslandHitArea,
} from '../../api/usageIsland';
import { UsageIslandMain, UsageIslandPanel } from './UsageIsland';
import UsageIslandBridge from './UsageIslandBridge';
import { useUsageIslandBatchStore } from './useUsageIslandData';
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

vi.mock('../../api/usageIsland', async (original) => ({
  ...(await original<typeof import('../../api/usageIsland')>()),
  resizeUsageIsland: vi.fn(async () => {}),
  setUsageIslandHitArea: vi.fn(async () => {}),
}));

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
vi.mock('../../features/accounts/context', () => ({ useAccounts: () => ({}) }));
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
  useUsageIslandBatchStore.setState({ running: false });
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
    activePage: 'apps',
    appsTab: 'usageIsland',
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
  await act(async () => button('usageIsland.style.list').props.onClick());
  expect(shown()).toEqual(['demo:chatgpt', 'demo:claude']);
  expect(refreshAccount).not.toHaveBeenCalled();
  expect(refreshModel).not.toHaveBeenCalled();
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

it.each(['line', 'list'] as const)(
  'renders quota bars and bare balances in %s style',
  async (style) => {
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
    await renderView(style, accountItems());
    const bars = (key: string) =>
      view!.root
        .findByProps({ 'data-usage-source': key })
        .findAll((node) => node.props.className === 'usage-island-line-track');
    expect(bars(accountSourceKey('claude', 'a'))).toHaveLength(2);
    expect(bars(accountSourceKey('kimi', 'b'))).toHaveLength(0);
  }
);

it.each(['line', 'list'] as const)(
  'names and masks sources above their quotas in %s style',
  async (style) => {
    await renderView(style, [
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
  }
);

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
  await act(async () => island.getState().update({ theme: 'dark' }));
  expect(surface()).toContain('usage-island-theme-dark');
  expect(surface()).not.toContain('usage-island-theme-translucent');
  expect(surface()).not.toContain('voice-theme-iridescent');
  expect(surface()).not.toContain('usage-island-theme-auto');
});

it('shows each countdown beside its percent in the horizontal style', async () => {
  const resetAt = Date.now() / 1000 + 2 * 3600;
  const item = (lines: UsageIslandItem['lines']): UsageIslandItem => ({
    kind: 'account',
    key: 'a',
    name: 'Claude',
    detail: 'me@example.com',
    icon: null,
    percent: 82,
    value: '82%',
    lines,
  });
  await renderView('row', [
    item([
      { label: '5h', percent: 82, value: '82%', resetAt },
      { label: '7d', percent: 64, value: '64%', resetAt: null },
    ]),
  ]);
  const shown = JSON.stringify(view!.toJSON());
  expect(shown).toContain('2h0m');
  expect(shown).not.toContain('5h');
  expect(shown).toContain('7d');
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
    ['list', false, [0, 0]],
    ['list', true, [0, 1]],
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

it('shortens the Chinese US dollar mark in the slot', () => {
  const zhDollars = new Intl.NumberFormat('zh-CN', { style: 'currency', currency: 'USD' });
  expect(compactValue(zhDollars.format(0))).toBe('$0.00');
  expect(compactValue(zhDollars.format(12400))).toBe('$12.4K');
  expect(compactValue('¥8.50')).toBe('¥8.50');
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
  for (const style of usageIslandStyles) {
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

it('keeps the editor preview fixed while saving the floating scale from each slider step', async () => {
  await render();
  const slider = view!.root
    .findAllByProps({ type: 'range' })
    .find((node) => node.props.max === 150)!;
  expect(slider.props).toMatchObject({ min: 70, max: 150, step: 10, value: 100 });
  const ticks = () =>
    slider.parent!.findByProps({ className: 'island-scale-ticks' }).findAllByType('span');
  expect(ticks().map((tick) => tick.children.join(''))).toEqual(
    usageIslandScales.map((scale) => `${scale}%`)
  );
  await act(async () => slider.props.onChange({ target: { value: '130' } }));
  expect(island.getState().scale).toBe(130);
  expect(
    ticks()
      .filter((tick) => tick.props['data-active'])
      .map((tick) => tick.children.join(''))
  ).toEqual(['130%']);
  expect(view!.root.findByProps({ className: 'usage-island-preview' }).props.style).toBeUndefined();
  expect(view!.root.findByProps({ className: 'usage-island-stage' }).props.style).toBeUndefined();
  for (const scale of [70, 150]) {
    await act(async () => slider.props.onChange({ target: { value: String(scale) } }));
    expect(island.getState().scale).toBe(scale);
    expect(
      view!.root.findByProps({ className: 'usage-island-preview' }).props.style
    ).toBeUndefined();
    expect(view!.root.findByProps({ className: 'usage-island-stage' }).props.style).toBeUndefined();
  }
  expect(JSON.parse(vi.mocked(localStorage.setItem).mock.lastCall![1])).toMatchObject({
    scale: 150,
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

it('sizes a six-source list and preserves click selection, collapse and empty behavior', async () => {
  const items = Array.from({ length: 6 }, (_, index) => ({
    ...accountItems()[0],
    key: `test:${index}`,
  }));
  const onLayout = vi.fn();
  const onPick = vi.fn();
  const mount = (entries: UsageIslandItem[], pick: string | null = null) => (
    <UsageIslandView
      items={entries}
      settings={{ style: 'list', reversed: [], theme: 'dark', expandMode: 'click' }}
      pick={pick}
      onPick={onPick}
      onLayout={onLayout}
      available={1000}
    />
  );
  await act(async () => {
    view = create(mount(items));
  });
  expect(onLayout).toHaveBeenLastCalledWith({
    expanded: false,
    width: 280,
    height: 410,
    radius: 24,
    anchorX: 0,
    anchorY: 0,
  });
  await act(async () => button('usageIsland.expand').props.onClick());
  expect(onLayout).toHaveBeenLastCalledWith({
    expanded: true,
    width: 280,
    height: 410,
    radius: 24,
    anchorX: 0,
    anchorY: 0,
  });
  const source = () => view!.root.findByProps({ 'data-usage-source': 'test:0' });
  await act(async () => source().props.onClick());
  expect(onPick).toHaveBeenLastCalledWith('test:0');
  await act(async () => view!.update(mount(items, 'test:0')));
  expect(source().props['aria-pressed']).toBe(true);
  await act(async () => source().props.onClick());
  expect(onPick).toHaveBeenLastCalledWith(null);
  await act(async () => view!.update(mount(items)));
  await act(async () => button('usageIsland.collapse').props.onClick());
  expect(onLayout.mock.lastCall![0].expanded).toBe(false);
  await act(async () => view!.update(mount([])));
  expect(button('usageIsland.expand').props.disabled).toBe(true);
  expect(onLayout.mock.lastCall![0].expanded).toBe(false);
  expect(refreshAccount).not.toHaveBeenCalled();
  expect(refreshModel).not.toHaveBeenCalled();
});

const clickListAction = async (label: string) => {
  const stopPropagation = vi.fn();
  await act(async () => button(label).props.onClick({ stopPropagation }));
  expect(stopPropagation).toHaveBeenCalledOnce();
};

it('runs list actions only on selected sources with the matching capability', async () => {
  const warmA = vi.fn(async () => {});
  const warmB = vi.fn(async () => {});
  const claimA = vi.fn(async () => {});
  const claimB = vi.fn(async () => {});
  const refreshA = vi.fn(async () => {});
  const refreshB = vi.fn(async () => {});
  testProviders.current = [
    provider({
      accounts: [
        account({ id: 'a', warmUp: warmA, claim: claimA, refresh: refreshA }),
        account({ id: 'b', warmUp: warmB, claim: claimB, refresh: refreshB }),
      ],
    }),
  ];
  island.setState({
    style: 'list',
    sources: [accountSourceKey('claude', 'a'), modelSourceKey('model-1')],
  });
  await render();
  expect(warmA).not.toHaveBeenCalled();
  expect(claimA).not.toHaveBeenCalled();
  expect(refreshA).not.toHaveBeenCalled();
  await clickListAction('agent.warmUpAccount');
  expect(warmA).toHaveBeenCalledOnce();
  expect(warmB).not.toHaveBeenCalled();
  await clickListAction('accountCenter.claimAll');
  expect(claimA).toHaveBeenCalledOnce();
  expect(claimB).not.toHaveBeenCalled();
  await clickListAction('btn.refresh');
  expect(refreshA).toHaveBeenCalledOnce();
  expect(refreshB).not.toHaveBeenCalled();
  expect(refreshModel).toHaveBeenCalledExactlyOnceWith(
    'model-1',
    expect.objectContaining({ silent: false })
  );
  expect(island.getState()).toMatchObject({
    pick: null,
    sources: [accountSourceKey('claude', 'a'), modelSourceKey('model-1')],
  });
});

it('stops device claims only for that provider while continuing other selected providers', async () => {
  const deviceClaim = vi.fn(async () => true);
  const skippedClaim = vi.fn(async () => {});
  const otherClaim = vi.fn(async () => {});
  testProviders.current = [
    provider({
      accounts: [
        account({ id: 'a', claim: deviceClaim }),
        account({ id: 'b', claim: skippedClaim }),
      ],
    }),
    provider({ id: 'workbuddy', accounts: [account({ id: 'c', claim: otherClaim })] }),
  ];
  island.setState({
    style: 'list',
    sources: [
      accountSourceKey('claude', 'a'),
      accountSourceKey('claude', 'b'),
      accountSourceKey('workbuddy', 'c'),
    ],
  });
  await render();
  await clickListAction('accountCenter.claimAll');
  expect(deviceClaim).toHaveBeenCalledOnce();
  expect(skippedClaim).not.toHaveBeenCalled();
  expect(otherClaim).toHaveBeenCalledOnce();
});

it('locks list actions during a request and excludes sources changed during that request', async () => {
  let finish!: () => void;
  const first = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      })
  );
  const removed = vi.fn(async () => {});
  const added = vi.fn(async () => {});
  testProviders.current = [
    provider({
      accounts: [
        account({ id: 'a', warmUp: first }),
        account({ id: 'b', warmUp: removed }),
        account({ id: 'c', warmUp: added }),
      ],
    }),
  ];
  island.setState({
    style: 'list',
    sources: [accountSourceKey('claude', 'a'), accountSourceKey('claude', 'b')],
  });
  await render();
  await clickListAction('agent.warmUpAccount');
  expect(button('agent.warmUpAccount').props.disabled).toBe(true);
  expect(button('btn.refresh').props.disabled).toBe(true);
  expect(useRewardClaimStore.getState().running).toBe(true);
  await clickListAction('agent.warmUpAccount');
  expect(first).toHaveBeenCalledOnce();
  await act(async () =>
    island.getState().setSources([accountSourceKey('claude', 'a'), accountSourceKey('claude', 'c')])
  );
  await act(async () => finish());
  expect(removed).not.toHaveBeenCalled();
  expect(added).not.toHaveBeenCalled();
  expect(useRewardClaimStore.getState().running).toBe(false);
  expect(button('agent.warmUpAccount').props.disabled).toBe(false);
});

it('summarizes list failures while continuing the selected batch', async () => {
  const failed = vi.fn(async () => {
    throw new Error('failed');
  });
  const next = vi.fn(async () => {});
  testProviders.current = [
    provider({
      accounts: [account({ id: 'a', warmUp: failed }), account({ id: 'b', warmUp: next })],
    }),
  ];
  island.setState({
    style: 'list',
    sources: [accountSourceKey('claude', 'a'), accountSourceKey('claude', 'b')],
  });
  await render();
  await clickListAction('agent.warmUpAccount');
  expect(next).toHaveBeenCalledOnce();
  expect(showToast).toHaveBeenCalledExactlyOnceWith('warning', 'accountCenter.batchWarmUpSummary');
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('stops remaining list operations and ignores late failures after leaving the island', async () => {
  let reject!: (error: Error) => void;
  const first = vi.fn(
    () =>
      new Promise<void>((_resolve, fail) => {
        reject = fail;
      })
  );
  const next = vi.fn(async () => {});
  testProviders.current = [
    provider({ accounts: [account({ id: 'a', claim: first }), account({ id: 'b', claim: next })] }),
  ];
  island.setState({
    style: 'list',
    sources: [accountSourceKey('claude', 'a'), accountSourceKey('claude', 'b')],
  });
  await render();
  await clickListAction('accountCenter.claimAll');
  await act(async () => navigation.setState({ activePage: 'accounts' }));
  await act(async () => navigation.setState({ activePage: 'apps' }));
  await act(async () => reject(new Error('late failure')));
  expect(next).not.toHaveBeenCalled();
  expect(showToast).not.toHaveBeenCalled();
  expect(useRewardClaimStore.getState().running).toBe(false);
});

it('uses shared busy and capability state for list buttons without passive work', async () => {
  const claim = vi.fn(async () => {});
  const warmUp = vi.fn(async () => {});
  testProviders.current = [
    provider({ accounts: [account({ claim, warmUp, claimDisabled: true, refreshing: true })] }),
  ];
  island.setState({ style: 'list', sources: [accountSourceKey('claude', 'a')] });
  await render();
  expect(button('accountCenter.claimAll')).toBeUndefined();
  expect(button('agent.warmUpAccount').props.disabled).toBe(true);
  expect(button('btn.refresh').props.disabled).toBe(true);
  await clickListAction('agent.warmUpAccount');
  expect(warmUp).not.toHaveBeenCalled();
  expect(claim).not.toHaveBeenCalled();
  expect(refreshAccount).not.toHaveBeenCalled();
});

it('bridges explicit list actions and serializes capabilities without account callbacks', async () => {
  const warmUp = vi.fn(async () => {});
  testProviders.current = [provider({ accounts: [account({ warmUp })] })];
  island.setState({
    enabled: true,
    style: 'list',
    expandMode: 'always',
    sources: [accountSourceKey('claude', 'a')],
    scale: 120,
  });
  await act(async () => {
    view = create(<UsageIslandBridge />);
  });
  const states = () => events.emitTo.mock.calls.filter((call) => call[1] === 'usage-island-state');
  const calls = states();
  const payload = calls[calls.length - 1][2] as string;
  expect(JSON.parse(payload)).toMatchObject({
    actions: { warmUp: true, claim: false, refresh: true, blocked: false },
  });
  expect(JSON.parse(payload).items[0]).not.toHaveProperty('warmUp'); // callbacks stay in main
  const dispatch = events.listeners.get('usage-island-action')!;
  await act(async () => dispatch({ payload: 'unexpected' }));
  expect(warmUp).not.toHaveBeenCalled();
  await act(async () => dispatch({ payload: 'warmUp' }));
  expect(warmUp).toHaveBeenCalledOnce();
  await act(async () => view!.unmount());
  vi.stubGlobal('window', { screen: { availWidth: 1920 } });
  await act(async () => {
    view = create(<UsageIslandWindow />);
  });
  await act(async () => events.listeners.get('usage-island-state')!({ payload }));
  await clickListAction('agent.warmUpAccount');
  expect(events.emitTo).toHaveBeenCalledWith('main', 'usage-island-action', 'warmUp');
  expect(setZoom).toHaveBeenCalledWith(1.2);
});

it('keeps one source icon outside the list identity and quota text', async () => {
  await renderView('list', accountItems());
  const row = view!.root.findByProps({ 'data-usage-source': accountSourceKey('claude', 'a') });
  const icons = row.findAllByType('img');
  expect(icons).toHaveLength(1);
  expect(row.findByProps({ className: 'usage-island-lines' }).findAllByType('img')).toHaveLength(0);
  const title = row.findByProps({ className: 'usage-island-line-title' });
  expect(title.findAllByType('img')).toHaveLength(0);
  expect(title.findByProps({ className: 'usage-island-line-name' }).children).toEqual([
    'me@example.com',
  ]);
  expect(row.findByProps({ className: 'usage-island-line-value' }).children).toEqual(['42%']);
});

it('respects another page batch lock before dispatching a list account operation', async () => {
  const warmUp = vi.fn(async () => {});
  testProviders.current = [provider({ accounts: [account({ warmUp })] })];
  island.setState({ style: 'list', sources: [accountSourceKey('claude', 'a')] });
  useRewardClaimStore.setState({ running: true });
  await render();
  expect(button('agent.warmUpAccount').props.disabled).toBe(true);
  await clickListAction('agent.warmUpAccount');
  expect(warmUp).not.toHaveBeenCalled();
  await act(async () => useRewardClaimStore.getState().finish());
  await clickListAction('agent.warmUpAccount');
  expect(warmUp).toHaveBeenCalledOnce();
});

it.each(['hover', 'click'] as const)(
  'keeps list geometry stable through %s expansion and payload updates',
  async (expandMode) => {
    const onLayout = vi.fn();
    const mount = (hover: boolean, value = '42%') => (
      <UsageIslandView
        items={accountItems().map((item) => ({ ...item, value }))}
        settings={{ style: 'list', reversed: [], theme: 'dark', expandMode }}
        available={1200}
        hover={hover}
        onLayout={onLayout}
      />
    );
    await act(async () => {
      view = create(mount(false));
    });
    expect(onLayout.mock.lastCall![0]).toMatchObject({ expanded: false, width: 280, radius: 24 });
    if (expandMode === 'click') await act(async () => button('usageIsland.expand').props.onClick());
    else await act(async () => view!.update(mount(true)));
    const stable = { expanded: true, width: 280, height: 120, radius: 24, anchorX: 0, anchorY: 0 };
    expect(onLayout).toHaveBeenLastCalledWith(stable);
    const calls = onLayout.mock.calls.length;
    // Both payload and DOM hover events can change while native hit testing owns hover.
    await act(async () => view!.update(mount(expandMode === 'hover', '41%')));
    const shell = view!.root.findAll(
      (node) => node.type === 'div' && String(node.props.className).startsWith('usage-island ')
    )[0];
    await act(async () => shell.props.onPointerLeave({ pointerType: 'mouse' }));
    expect(onLayout.mock.calls).toHaveLength(calls);
    expect(shell.props['data-expanded']).toBe(true);
    if (expandMode === 'click')
      await act(async () => button('usageIsland.collapse').props.onClick());
    else await act(async () => view!.update(mount(false)));
    expect(onLayout.mock.lastCall![0].expanded).toBe(false);
  }
);

it.each([70, 100, 150])(
  'sends the actual list radius at %s percent without resizing on each open or close',
  async (scale) => {
    vi.stubGlobal('window', { screen: { availWidth: 1920 } });
    const payload = JSON.stringify({
      settings: { style: 'list', reversed: [], theme: 'dark', expandMode: 'hover' },
      pick: null,
      scale,
      refreshSeconds: 0,
      items: accountItems(),
    });
    await act(async () => {
      view = create(<UsageIslandWindow />);
    });
    await act(async () => events.listeners.get('usage-island-state')!({ payload }));
    const factor = scale / 100;
    expect(setUsageIslandHitArea).toHaveBeenLastCalledWith(
      24 * factor,
      24 * factor,
      48 * factor,
      48 * factor,
      24 * factor
    );
    await act(async () => events.listeners.get('usage-island-hover')!({ payload: true }));
    expect(setUsageIslandHitArea).toHaveBeenLastCalledWith(
      24 * factor,
      24 * factor,
      280 * factor,
      120 * factor,
      24 * factor
    );
    expect(resizeUsageIsland).toHaveBeenCalledTimes(1);
    const size = vi.mocked(resizeUsageIsland).mock.lastCall;
    expect(size).toEqual([328 * factor, 168 * factor, 0, 0]);
    await act(async () => events.listeners.get('usage-island-hover')!({ payload: false }));
    expect(setUsageIslandHitArea).toHaveBeenLastCalledWith(
      24 * factor,
      24 * factor,
      48 * factor,
      48 * factor,
      24 * factor
    );
    // Native window's target size remains unchanged as only its mouse hit area changes.
    expect(vi.mocked(resizeUsageIsland).mock.lastCall).toEqual(size);
    expect(resizeUsageIsland).toHaveBeenCalledTimes(1);
  }
);

it('applies only the latest hit region when native resize finishes after hover changes', async () => {
  let finish!: () => void;
  vi.mocked(resizeUsageIsland).mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      })
  );
  vi.stubGlobal('window', { screen: { availWidth: 1920 } });
  const payload = JSON.stringify({
    settings: { style: 'list', reversed: [], theme: 'dark', expandMode: 'hover' },
    pick: null,
    scale: 100,
    refreshSeconds: 0,
    items: accountItems(),
  });
  await act(async () => {
    view = create(<UsageIslandWindow />);
  });
  await act(async () => events.listeners.get('usage-island-state')!({ payload }));
  expect(resizeUsageIsland).toHaveBeenCalledTimes(1);
  expect(setUsageIslandHitArea).not.toHaveBeenCalled();
  await act(async () => events.listeners.get('usage-island-hover')!({ payload: true }));
  await act(async () => finish());
  expect(resizeUsageIsland).toHaveBeenCalledTimes(1);
  expect(setUsageIslandHitArea).toHaveBeenCalledExactlyOnceWith(24, 24, 280, 120, 24);
});

it('marks active Smart Access sources and applies the saved surface opacity', async () => {
  const [item] = accountItems();
  await act(async () => {
    view = create(
      <UsageIslandView
        items={[{ ...item, smartActive: true }]}
        settings={{
          style: 'list',
          reversed: [],
          theme: 'dark',
          expandMode: 'always',
          opacity: 60,
          showSmartAccess: true,
        }}
      />
    );
  });
  const row = view!.root
    .findAllByProps({ 'data-smart-active': true })
    .find((node) => node.props['data-usage-source'] === item.key)!;
  expect(row.props['data-usage-source']).toBe(item.key);
  expect(view!.root.findAllByProps({ 'data-smart-active': true })).toHaveLength(1);
  expect(
    view!.root.findByProps({ className: 'usage-island-logo' }).props['data-smart-active']
  ).toBeUndefined();
  const track = view!.root.findByProps({ className: 'usage-island-track' });
  expect(track.props.style['--usage-island-surface-opacity']).toBe('60%');
});

it.each([
  ['auto', 'carbon', 'carbon'],
  ['auto', 'basalt', 'basalt'],
  ['auto', 'monogram', 'monogram'],
  ['auto', 'characters', null],
  ['auto', 'none', null],
  ['dark', 'carbon', null],
] as const)(
  'theme %s with app background %s shows texture %s',
  async (theme, background, expected) => {
    const [item] = accountItems();
    await act(async () => {
      view = create(
        <UsageIslandView
          items={[item]}
          settings={{ style: 'row', reversed: [], theme, expandMode: 'always', background }}
        />
      );
    });
    const textures = view!.root
      .findAll((node) => typeof node.type === 'string' && node.props['data-island-texture'])
      .map((node) => node.props['data-island-texture']);
    expect(textures).toEqual(expected ? [expected] : []);
  }
);

it.each(['click', 'always'] as const)(
  'clicking the picked source in the logo slot restores the logo in %s mode',
  async (expandMode) => {
    const [item] = accountItems();
    const onPick = vi.fn();
    await act(async () => {
      view = create(
        <UsageIslandView
          items={[item]}
          pick={item.key}
          onPick={onPick}
          settings={{ style: 'list', reversed: [], theme: 'dark', expandMode }}
        />
      );
    });
    const toggle = view!.root.findByProps({ className: 'usage-island-toggle' });
    expect(toggle.props.disabled).toBe(false);
    expect(toggle.props['aria-label']).toBe(item.name);
    await act(async () => toggle.props.onClick());
    expect(onPick).toHaveBeenCalledExactlyOnceWith(null);
  }
);

it('keeps the page refresh and the island timed refresh from overlapping', async () => {
  island.setState({
    enabled: true,
    sources: [accountSourceKey('claude', 'a')],
    refreshSeconds: 300,
  });
  let finish = () => {};
  refreshAccount.mockImplementationOnce(() => new Promise<void>((resolve) => (finish = resolve)));
  let bridge: ReactTestRenderer | undefined;
  await act(async () => {
    bridge = create(<UsageIslandBridge />);
  });
  await render();
  await act(async () => events.listeners.get('usage-island-tick')!({ payload: undefined }));
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  const pageRefresh = () => buttonWithText('accountCenter.refreshAll');
  expect(pageRefresh().props.disabled).toBe(true);
  await act(async () => pageRefresh().props.onClick());
  expect(refreshAccount).toHaveBeenCalledTimes(1);
  await act(async () => finish());
  expect(pageRefresh().props.disabled).toBe(false);
  await act(async () => pageRefresh().props.onClick());
  expect(refreshAccount).toHaveBeenCalledTimes(2);
  // A manual page refresh in flight makes the island's tick skip, not stack.
  refreshAccount.mockImplementationOnce(() => new Promise<void>((resolve) => (finish = resolve)));
  await act(async () => pageRefresh().props.onClick());
  expect(refreshAccount).toHaveBeenCalledTimes(3);
  await act(async () => events.listeners.get('usage-island-tick')!({ payload: undefined }));
  expect(refreshAccount).toHaveBeenCalledTimes(3);
  await act(async () => finish());
  await act(async () => bridge!.unmount());
});
