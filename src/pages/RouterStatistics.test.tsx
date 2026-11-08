import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  RouterStatisticsMain,
  RouterStatisticsPanel,
  RouterStatisticsProvider,
} from './RouterStatistics';
import { useNavigationStore } from '../stores/navigationStore';
import type { RouterStatisticsSnapshot } from '../api/routerStatistics';
import { emptyStatisticsCounts } from './routerStatisticsData';

const mocks = vi.hoisted(() => ({ snapshot: vi.fn(), catalog: vi.fn(), visible: true }));
vi.mock('../api/routerStatistics', () => ({
  ROUTER_STATISTICS_CHANGED: 'router-statistics-settings-changed',
  getSmartRouterStatistics: (...args: unknown[]) => mocks.snapshot(...args),
  getSmartRouterStatisticsCatalog: () => mocks.catalog(),
}));
vi.mock('../hooks/useDocumentVisible', () => ({ useDocumentVisible: () => mocks.visible }));
vi.mock('../hooks/useI18n', () => ({ useI18n: () => ({ t: (key: string) => key, locale: 'en' }) }));
vi.mock('../components/cards/ModelCard', () => ({ getModelIcon: () => null }));
let view: ReactTestRenderer;
const empty: RouterStatisticsSnapshot = {
  recording: false,
  since: 0,
  savedAt: 0,
  storageError: false,
  capped: false,
  today: 739916,
  totals: { calls: 0, completed: 0, failed: 0, cancelled: 0, rescued: 0, durationMs: 0 },
  rows: [],
  trend: [],
};
const tree = () => (
  <RouterStatisticsProvider>
    <RouterStatisticsMain />
    <RouterStatisticsPanel />
  </RouterStatisticsProvider>
);
async function mount() {
  await act(async () => {
    view = create(tree());
  });
}
async function activate(tab: 'statistics' | 'plugins' = 'statistics') {
  await act(async () =>
    useNavigationStore.setState({
      developerMode: true,
      activePage: tab === 'statistics' ? 'freeModels' : 'apps',
      smartAccessTab: 'statistics',
      appsTab: 'plugins',
      developerTab: tab,
    })
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal('window', globalThis);
  vi.stubGlobal('document', new EventTarget());
  mocks.visible = true;
  mocks.snapshot.mockResolvedValue(empty);
  mocks.catalog.mockResolvedValue({ groups: [], sources: [], capped: false });
  useNavigationStore.setState({ developerMode: false, activePage: 'mother', developerTab: 'pets' });
});
afterEach(() => {
  act(() => view?.unmount());
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it('reads nothing outside the active statistics tab, and shares one snapshot between main and detail', async () => {
  await mount();
  expect(mocks.snapshot).not.toHaveBeenCalled();
  expect(mocks.catalog).not.toHaveBeenCalled();
  await activate('plugins');
  expect(mocks.snapshot).not.toHaveBeenCalled();
  await activate();
  expect(mocks.snapshot).toHaveBeenCalledExactlyOnceWith(1, '');
  expect(mocks.catalog).toHaveBeenCalledOnce();
  expect(view.root.findAllByProps({ className: 'rs-summary' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-table' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-detail' })).toHaveLength(1);
  await activate('plugins');
  expect(mocks.snapshot).toHaveBeenCalledOnce();
});
it('does not poll while paused, hidden or outside the tab, and prevents overlapping reads', async () => {
  vi.useFakeTimers();
  await activate();
  await mount();
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  mocks.snapshot.mockResolvedValue({ ...empty, recording: true, since: 1 });
  await activate('plugins');
  await activate();
  const calls = mocks.snapshot.mock.calls.length;
  let finish!: (value: RouterStatisticsSnapshot) => void;
  mocks.snapshot.mockImplementationOnce(
    () =>
      new Promise<RouterStatisticsSnapshot>((resolve) => {
        finish = resolve;
      })
  );
  await act(async () => {
    vi.advanceTimersByTime(10_000);
  });
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(calls + 1);
  await act(async () => finish({ ...empty, recording: true, since: 1 }));
  mocks.visible = false;
  await act(async () => view.update(tree()));
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(calls + 1);
  mocks.visible = true;
  await act(async () => view.update(tree()));
  await activate('plugins');
  const left = mocks.snapshot.mock.calls.length;
  await act(async () => {
    vi.advanceTimersByTime(30_000);
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(left);
});
it('ignores responses after leaving and preserves cached history on passive read failures', async () => {
  await activate();
  mocks.snapshot.mockResolvedValue({ ...empty, since: 1, totals: { ...empty.totals, calls: 42 } });
  await mount();
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('42'))).toBe(true);
  await activate('plugins');
  mocks.snapshot.mockRejectedValueOnce(new Error('offline'));
  await activate();
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('42'))).toBe(true);
  expect(view.root.findAllByProps({ role: 'alert' })).toHaveLength(1);
  let finish!: (value: RouterStatisticsSnapshot) => void;
  mocks.snapshot.mockImplementationOnce(
    () =>
      new Promise<RouterStatisticsSnapshot>((resolve) => {
        finish = resolve;
      })
  );
  await activate('plugins');
  await activate();
  await activate('plugins');
  await act(async () => finish({ ...empty, since: 1, totals: { ...empty.totals, calls: 999 } }));
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('999'))).toBe(
    false
  );
});

it('stops reads with the developer master switch and ignores the previous generation after re-enabling', async () => {
  let finish!: (value: RouterStatisticsSnapshot) => void;
  mocks.snapshot.mockImplementationOnce(
    () =>
      new Promise<RouterStatisticsSnapshot>((resolve) => {
        finish = resolve;
      })
  );
  await activate();
  await mount();
  await act(async () => useNavigationStore.setState({ developerMode: false }));
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  mocks.snapshot.mockResolvedValue({
    ...empty,
    since: 1,
    recording: true,
    totals: { ...empty.totals, calls: 7 },
  });
  await act(async () => useNavigationStore.setState({ developerMode: true }));
  await act(async () => finish(empty));
  expect(mocks.snapshot).toHaveBeenCalledTimes(2);
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('7'))).toBe(true);
});

it('ignores old reads when disabling and re-enabling are batched before a render', async () => {
  let finish!: (value: RouterStatisticsSnapshot) => void;
  mocks.snapshot.mockImplementationOnce(
    () =>
      new Promise<RouterStatisticsSnapshot>((resolve) => {
        finish = resolve;
      })
  );
  await activate();
  await mount();
  mocks.snapshot.mockResolvedValue({
    ...empty,
    since: 1,
    totals: { ...empty.totals, calls: 7 },
  });
  await act(async () => {
    useNavigationStore.getState().setDeveloperMode(false);
    useNavigationStore.getState().setDeveloperMode(true);
    useNavigationStore.setState({
      activePage: 'freeModels',
      smartAccessTab: 'statistics',
      developerTab: 'statistics',
    });
  });
  await act(async () => finish({ ...empty, since: 1, totals: { ...empty.totals, calls: 999 } }));
  expect(mocks.snapshot).toHaveBeenCalledTimes(2);
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('7'))).toBe(true);
  expect(view.root.findAllByType('strong').some((node) => node.children.includes('999'))).toBe(
    false
  );
});

it('leaves usage and details empty until real rows arrive, then shows only recorded protection categories', async () => {
  const source = {
    id: 'source',
    account: 'account',
    model: 'model',
    group: 'group',
    name: 'Configured source',
    modelName: 'Actual model',
    provider: 'fixture',
    enabled: true,
  };
  mocks.catalog.mockResolvedValue({
    groups: [{ id: 'group', name: 'Connection' }],
    sources: [source],
    capped: false,
  });
  await activate();
  await mount();
  expect(
    view.root
      .findAllByType('button')
      .filter((node) => String(node.props.className).startsWith('rs-row '))
  ).toHaveLength(0);
  expect(
    view.root
      .findByProps({ className: 'rs-summary' })
      .findAllByType('strong')
      .map((node) => node.children.join(''))
  ).toEqual(['—', '—', '—', '—']);
  const detail = view.root.findByProps({ className: 'rs-detail' });
  expect(
    detail.findAllByType('div').some((node) => node.children.includes('statistics.selectSource'))
  ).toBe(true);
  expect(detail.findAllByType('strong')).toHaveLength(0);
  expect(detail.findAllByProps({ className: 'rs-detail-title' })).toHaveLength(0);
  expect(detail.findAllByProps({ className: 'rs-detail-models' })).toHaveLength(0);
  expect(detail.findAllByProps({ className: 'rs-detail-errors' })).toHaveLength(0);
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  const placeholders = view.root.findAllByProps({ className: 'rs-row rs-placeholder' });
  expect(placeholders).toHaveLength(3);
  expect(
    placeholders.every(
      (node) =>
        node.type === 'div' &&
        node.props['aria-hidden'] === 'true' &&
        !node.props.onClick &&
        !node.props.tabIndex
    )
  ).toBe(true);
  expect(view.root.findAllByProps({ className: 'rs-detail-counts' })).toHaveLength(0);
  expect(
    view.root.findAllByType('span').some((node) => node.children.includes('Configured source'))
  ).toBe(false);
  expect(
    view.root.findAllByType('button').some((node) => node.children.includes('statistics.unused'))
  ).toBe(false);

  mocks.snapshot.mockResolvedValue({
    ...empty,
    since: 1,
    totals: { ...empty.totals, calls: 1, completed: 1 },
    rows: [
      {
        id: source.id,
        account: source.account,
        model: source.model,
        group: source.group,
        lastUsedAt: Date.now(),
        counts: {
          attempts: 1,
          completed: 1,
          failed: 0,
          cancelled: 0,
          protections: [2, 0, 3, 0, 0],
          errors: [0, 0, 0, 0, 0, 0],
        },
      },
    ],
  });
  await act(async () =>
    view.root
      .findAllByType('button')
      .find((node) => node.props['aria-label'] === 'btn.refresh')!
      .props.onClick()
  );
  expect(
    view.root
      .findAllByType('button')
      .filter((node) => String(node.props.className).startsWith('rs-row '))
  ).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-row rs-placeholder' })).toHaveLength(0);
  expect(view.root.findAllByProps({ className: 'rs-tag rs-kind-0' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-tag rs-kind-2' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-tag rs-kind-1' })).toHaveLength(0);
  expect(
    view.root.findByProps({ className: 'rs-detail-rate' }).findByType('strong').children[0]
  ).toBe('100.0');
  expect(
    view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
  ).toEqual(['Configured source']);
});

it('shows only sources used in the selected period and removes all placeholders once real data arrives', async () => {
  const source = {
    id: 'used',
    account: 'used-account',
    model: 'model',
    group: 'group',
    name: 'DeepSeek',
    modelName: 'deepseek-flash',
    provider: 'api.deepseek.com',
    enabled: true,
  };
  const old = Array.from({ length: 3 }, (_, index) => ({
    id: `old-${index}`,
    account: `old-account-${index}`,
    model: 'old-model',
    group: 'group',
    lastUsedAt: Date.now() - 86400000,
    counts: emptyStatisticsCounts(),
  }));
  mocks.catalog.mockResolvedValue({
    groups: [{ id: 'group', name: 'Connection' }],
    sources: [source],
    capped: false,
  });
  mocks.snapshot.mockResolvedValue({ ...empty, rows: old });
  await activate();
  await mount();
  const rows = () =>
    view.root
      .findAllByType('button')
      .filter((node) => String(node.props.className).startsWith('rs-row '));
  const refresh = () =>
    act(async () => view.root.findByProps({ 'aria-label': 'btn.refresh' }).props.onClick());
  expect(rows()).toHaveLength(0);
  expect(view.root.findAllByProps({ className: 'rs-row rs-placeholder' })).toHaveLength(3);

  const used = {
    ...source,
    lastUsedAt: Date.now(),
    counts: { ...emptyStatisticsCounts(), attempts: 82, completed: 82 },
  };
  mocks.snapshot.mockResolvedValue({
    ...empty,
    rows: [old[0], used, ...old.slice(1)],
    totals: { ...empty.totals, calls: 82, completed: 82 },
  });
  await refresh();
  expect(rows()).toHaveLength(1);
  expect(rows()[0].findByProps({ className: 'rs-identity' }).findByType('b').children).toEqual([
    'DeepSeek',
  ]);
  expect(rows()[0].findByProps({ className: 'rs-load' }).findByType('b').children).toEqual(['82']);
  expect(view.root.findAllByProps({ className: 'rs-row rs-placeholder' })).toHaveLength(0);
  expect(JSON.stringify(view.toJSON())).not.toContain('statistics.archived');

  // A removed source with real counts in a longer period remains valid history.
  mocks.snapshot.mockImplementation((days) =>
    Promise.resolve({
      ...empty,
      rows: [
        used,
        ...old.map((row, index) => ({
          ...row,
          counts: { ...row.counts, attempts: days === 7 && index === 0 ? 2 : 0 },
        })),
      ],
    })
  );
  await act(async () =>
    view.root
      .findAllByType('button')
      .find((node) => node.children.includes('statistics.week'))!
      .props.onClick()
  );
  expect(rows()).toHaveLength(2);
  expect(rows()[1].findByProps({ className: 'rs-identity' }).findByType('b').children).toEqual([
    'statistics.archived',
  ]);
  expect(view.root.findAllByProps({ className: 'rs-row rs-placeholder' })).toHaveLength(0);
});

it('selects account and API sources from the same list without extra reads and hides both names together', async () => {
  const sources = ['account', 'api'].map((account) => ({
    id: account,
    account,
    model: 'same-model',
    group: 'connection',
    name: `${account} source`,
    modelName: 'Upstream model',
    provider: account,
    enabled: true,
  }));
  mocks.catalog.mockResolvedValue({ groups: [], sources, capped: false });
  mocks.snapshot.mockResolvedValue({
    ...empty,
    rows: sources.map((source) => ({
      ...source,
      lastUsedAt: 1,
      counts: {
        attempts: 1,
        completed: 1,
        failed: 0,
        cancelled: 0,
        protections: [1, 0, 0, 0, 0],
        errors: [0, 0, 0, 0, 0, 0],
      },
    })),
  });
  await activate();
  await mount();
  const rows = () =>
    view.root
      .findAllByType('button')
      .filter((node) => String(node.props.className).startsWith('rs-row '));
  expect(rows()).toHaveLength(2);
  expect(view.root.findAllByProps({ className: 'rs-detail-title' })).toHaveLength(0);
  expect(
    view.root
      .findAllByType('button')
      .some(
        (node) =>
          node.children.includes('statistics.accounts') ||
          node.children.includes('statistics.models')
      )
  ).toBe(false);
  await act(async () => rows()[1].props.onClick());
  expect(rows().map((row) => row.props['aria-pressed'])).toEqual([false, true]);
  expect(
    view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
  ).toEqual(['api source']);
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  expect(mocks.catalog).toHaveBeenCalledOnce();
  await act(async () =>
    view.root.findByProps({ 'aria-label': 'statistics.hideAccounts' }).props.onClick()
  );
  expect(
    rows().every((row) =>
      row.findByProps({ className: 'rs-identity' }).findByType('b').children.includes('******')
    )
  ).toBe(true);
  expect(
    view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
  ).toEqual(['******']);
});

it('refreshes the visible snapshot after an explicit recording change and stays quiet on another tab', async () => {
  vi.useFakeTimers();
  await activate();
  await mount();
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  mocks.snapshot.mockResolvedValue({ ...empty, recording: true });
  await act(async () => {
    document.dispatchEvent(new Event('router-statistics-settings-changed'));
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(2);
  await act(async () => {
    vi.advanceTimersByTime(10_000);
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(3);
  await activate('plugins');
  await act(async () => {
    document.dispatchEvent(new Event('router-statistics-settings-changed'));
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(3);
});
