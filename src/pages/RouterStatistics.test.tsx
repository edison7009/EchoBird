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

const mocks = vi.hoisted(() => ({
  snapshot: vi.fn(),
  catalog: vi.fn(),
  prices: vi.fn(),
  visible: true,
}));
vi.mock('../api/routerStatistics', () => ({
  ROUTER_STATISTICS_CHANGED: 'router-statistics-settings-changed',
  getSmartRouterStatistics: (...args: unknown[]) => mocks.snapshot(...args),
  getSmartRouterStatisticsCatalog: () => mocks.catalog(),
  refreshSmartRouterReferencePrices: () => mocks.prices(),
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
async function activate(tab: 'statistics' | 'pets' = 'statistics') {
  await act(async () =>
    useNavigationStore.setState({
      developerMode: true,
      activePage: tab === 'statistics' ? 'freeModels' : 'apps',
      smartAccessTab: 'statistics',
      appsTab: 'pets',
      developerTab: tab,
    })
  );
}
beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal('window', globalThis);
  vi.stubGlobal('document', new EventTarget());
  vi.stubGlobal('localStorage', { getItem: vi.fn(() => null), setItem: vi.fn() });
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
  await activate('pets');
  expect(mocks.snapshot).not.toHaveBeenCalled();
  await activate();
  expect(mocks.snapshot).toHaveBeenCalledExactlyOnceWith(1, '');
  expect(mocks.catalog).toHaveBeenCalledOnce();
  expect(view.root.findAllByProps({ className: 'rs-summary' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-table' })).toHaveLength(1);
  expect(view.root.findAllByProps({ className: 'rs-detail' })).toHaveLength(1);
  await activate('pets');
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
  await activate('pets');
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
  await activate('pets');
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
  await activate('pets');
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
  await activate('pets');
  await activate();
  await activate('pets');
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
    view.root.findAllByType('button').filter((node) => node.props.className === 'rs-source-details')
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
    view.root.findAllByType('button').filter((node) => node.props.className === 'rs-source-details')
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
  const rows = () => view.root.findAllByProps({ className: 'rs-source-row' });
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
  expect(rows()[0].findByProps({ className: 'rs-outcome-count' }).children[0]).toBe('100.0%');
  expect(view.root.findAllByProps({ className: 'rs-load' })).toHaveLength(0);
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
      .filter((node) => node.props.className === 'rs-source-details');
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
  expect(localStorage.setItem).not.toHaveBeenCalled();
  expect(
    view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
  ).toEqual(['api source']);
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  expect(mocks.catalog).toHaveBeenCalledOnce();
  await act(async () =>
    view.root.findByProps({ 'aria-label': 'statistics.hideAccounts' }).props.onClick()
  );
  expect(rows().map((row) => row.props['aria-label'])).toEqual([
    'statistics.sourceDetails · ******',
    'statistics.sourceDetails · ******',
  ]);
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
  await activate('pets');
  await act(async () => {
    document.dispatchEvent(new Event('router-statistics-settings-changed'));
  });
  expect(mocks.snapshot).toHaveBeenCalledTimes(3);
});

it('shows reported tokens for the selected source, preserves unknown usage, and does not read on selection', async () => {
  const sources = ['reported', 'unknown'].map((id) => ({
    id,
    account: id,
    model: 'model',
    group: 'group',
    name: id,
    modelName: 'Model',
    provider: 'fixture',
    enabled: true,
  }));
  mocks.catalog.mockResolvedValue({ groups: [], sources, capped: false });
  mocks.snapshot.mockResolvedValue({
    ...empty,
    totals: { ...empty.totals, calls: 2, completed: 2 },
    rows: sources.map((source, index) => ({
      ...source,
      lastUsedAt: 1,
      counts: {
        ...emptyStatisticsCounts(),
        attempts: 1,
        completed: 1,
        usage:
          index === 0
            ? { inputTokens: 100, outputTokens: 20, totalTokens: 120, reportedResponses: 1 }
            : undefined,
      },
    })),
  });
  await activate();
  await mount();
  const tokens = () =>
    view.root.findByProps({ className: 'rs-detail-counts' }).findAllByType('b')[1].children;
  expect(tokens()).toEqual(['120']);
  const unknown = view.root.findByProps({ 'aria-label': 'statistics.sourceDetails · unknown' });
  await act(async () => unknown.props.onClick());
  expect(tokens()).toEqual(['—']);
  expect(mocks.snapshot).toHaveBeenCalledOnce();
  expect(mocks.catalog).toHaveBeenCalledOnce();
});

it('never fetches pricing during navigation or metric changes, and refresh is explicit and non-overlapping', async () => {
  await activate();
  await mount();
  expect(mocks.prices).not.toHaveBeenCalled();
  await act(async () =>
    view.root
      .findAllByType('button')
      .find((b) => b.children.includes('statistics.cost'))!
      .props.onClick()
  );
  expect(mocks.prices).not.toHaveBeenCalled();
  let finish!: (value: { fetchedAt: number; models: number }) => void;
  mocks.prices.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  const button = () =>
    view.root.findAllByType('button').find((b) => b.children.includes('statistics.refreshPrices'))!;
  await act(async () => {
    button().props.onClick();
    button().props.onClick();
  });
  expect(mocks.prices).toHaveBeenCalledOnce();
  expect(button().props.disabled).toBe(true);
  await activate('pets');
  await act(async () => finish({ fetchedAt: 1, models: 2 }));
  expect(mocks.snapshot).toHaveBeenCalledOnce();
});

it.each([0, 1, 6])(
  'renders only real account tickers/watchlist rows for %i accounts without changing the snapshot on selection',
  async (count) => {
    const sources = Array.from({ length: count }, (_, index) => ({
      id: `id-${index}`,
      account: `account-${index}`,
      model: 'm',
      group: 'g',
      name: `Account ${index}`,
      modelName: 'Model',
      provider: 'fixture',
      enabled: true,
    }));
    mocks.catalog.mockResolvedValue({ groups: [], sources, capped: false });
    mocks.snapshot.mockResolvedValue({
      ...empty,
      rows: sources.map((source) => ({
        ...source,
        lastUsedAt: 1,
        counts: { ...emptyStatisticsCounts(), attempts: 1, completed: 1 },
      })),
    });
    await activate();
    await mount();
    const tickers = view.root.findAllByProps({ className: 'rm-ticker' });
    expect(tickers).toHaveLength(Math.min(4, count));
    expect(view.root.findAllByProps({ className: 'rm-watch-row' })).toHaveLength(
      count > 1 ? Math.min(4, count) : 0
    );
    if (count > 1) {
      await act(async () =>
        view.root.findAllByProps({ className: 'rm-watch-row' })[1].props.onClick()
      );
      expect(
        view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
      ).toEqual(['Account 1']);
    }
    if (count === 6) {
      const stars = () => view.root.findAllByProps({ className: 'rs-favorite' });
      expect(stars().every((star) => !star.props['aria-pressed'])).toBe(true);
      expect(localStorage.setItem).not.toHaveBeenCalled();
      for (let index = 0; index < 4; index++) {
        await act(async () => stars()[index].props.onClick());
      }
      expect(stars()[4].props.disabled).toBe(true);
      await act(async () => stars()[0].props.onClick());
      expect(view.root.findAllByProps({ className: 'rm-ticker' })).toHaveLength(3);
      expect(stars()[4].props.disabled).toBe(false);
      await act(async () => stars()[4].props.onClick());
      const pinned = view.root.findAllByProps({ className: 'rm-ticker' });
      expect(pinned).toHaveLength(4);
      expect(pinned[3].findByProps({ className: 'rm-ticker-name' }).children).toEqual([
        'Account 4',
      ]);
      expect(localStorage.setItem).toHaveBeenLastCalledWith(
        'router-market-favorites',
        JSON.stringify(['account-1', 'account-2', 'account-3', 'account-4'])
      );
      expect(
        view.root.findByProps({ className: 'rs-detail-identity' }).findByType('strong').children
      ).toEqual(['Account 1']);
      await act(async () => {
        view.unmount();
      });
      vi.mocked(localStorage.getItem).mockReturnValue(JSON.stringify(['account-4']));
      await mount();
      expect(view.root.findAllByProps({ className: 'rm-ticker' })).toHaveLength(1);
      expect(view.root.findAllByProps({ className: 'rm-watch-row' })).toHaveLength(1);
      expect(view.root.findByProps({ className: 'rm-ticker-name' }).children).toEqual([
        'Account 4',
      ]);
      await act(async () =>
        view.root.findAllByProps({ className: 'rs-favorite' })[4].props.onClick()
      );
      expect(view.root.findAllByProps({ className: 'rm-ticker' })).toHaveLength(4);
      expect(
        view.root
          .findAllByProps({ className: 'rs-favorite' })
          .every((star) => !star.props['aria-pressed'])
      ).toBe(true);
    }
    expect(mocks.snapshot).toHaveBeenCalledTimes(count === 6 ? 2 : 1);
    expect(mocks.prices).not.toHaveBeenCalled();
  }
);
