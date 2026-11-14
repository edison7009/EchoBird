import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import * as api from '../../api/aiCareer';
import { useAiCareerStore } from '../../stores/aiCareerStore';
import { useNavigationStore } from '../../stores/navigationStore';
import type { DayBuckets } from './heatmapData';
import { AiCareerMain } from './AiCareerMain';
import { AiCareerFamilyPanel } from './AiCareerFamilyPanel';

const i18n = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('../../hooks/useI18n', () => {
  return { useI18n: () => ({ t: i18n.t, locale: 'en' }) };
});
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../../components/Toast', () => ({ useToast: () => ({ showToast: vi.fn() }) }));
vi.mock('../../api/aiCareer', () => ({
  aiCareerHeatmap: vi.fn(),
  aiCareerTokenBytes: vi.fn(),
  aiCareerDailyTokens: vi.fn(),
  getAvatar: vi.fn(),
  setAvatar: vi.fn(),
}));
vi.mock('./ContributionHeatmap', () => ({
  ContributionHeatmap: ({ buckets }: { buckets: DayBuckets }) => (
    <div
      data-heatmap={true}
      data-messages={[...buckets.messages.values()].reduce((a, b) => a + b, 0)}
    />
  ),
}));
vi.mock('./DailyActivityChart', () => ({
  DailyActivityChart: ({
    buckets,
    metric,
    onMetricChange,
    tokenDays,
    tokenStatus,
    onTokenRetry,
  }: {
    buckets: DayBuckets;
    metric: 'messages' | 'tokens';
    onMetricChange: (metric: 'messages' | 'tokens') => void;
    tokenDays: Map<string, { input: number; output: number }>;
    tokenStatus: string;
    onTokenRetry: () => void;
  }) => (
    <div
      data-chart={true}
      data-messages={[...buckets.messages.values()].reduce((a, b) => a + b, 0)}
      data-metric={metric}
      data-token-days={tokenDays}
      data-token-status={tokenStatus}
    >
      <button data-token-tab={true} onClick={() => onMetricChange('tokens')} />
      <button data-token-retry={true} onClick={onTokenRetry} />
    </div>
  ),
}));
vi.mock('./AiCareerPanel', () => ({
  AiCareerPanel: ({ family }: { family: string }) => <div data-session-family={family} />,
}));

let renderer: ReactTestRenderer;
const writeSnapshot = vi.fn();

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

async function mount() {
  await act(async () => {
    renderer = create(
      <>
        <AiCareerMain />
        <AiCareerFamilyPanel />
      </>
    );
  });
}

async function selectFamily(name: string) {
  const button = renderer.root
    .findAllByType('button')
    .find((node) => node.findAllByType('img').some((image) => image.props.alt === name));
  if (!button) throw new Error('Missing family button');
  await act(async () => {
    button.props.onClick();
  });
}

const chartCount = () => renderer.root.findByProps({ 'data-chart': true }).props['data-messages'];
const heatmapCount = () =>
  renderer.root.findByProps({ 'data-heatmap': true }).props['data-messages'];
const chart = () => renderer.root.findByProps({ 'data-chart': true });
const tokenDays = (): Map<string, { input: number; output: number }> =>
  chart().props['data-token-days'];
const tokenData = (input: number): api.DailyTokenResponse => ({
  entries: [{ ts: 1_791_676_800, input_tokens: input, output_tokens: 2 }],
  unavailable_families: [],
});
async function selectTokens() {
  await act(async () => renderer.root.findByProps({ 'data-token-tab': true }).props.onClick());
}
const entries = (count: number) => [{ ts: 1_791_676_800, count }];

beforeEach(() => {
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.clearAllMocks();
  vi.stubGlobal('localStorage', { getItem: () => null, setItem: writeSnapshot });
  i18n.t = (key: string) => key;
  useAiCareerStore.setState({
    selectedFamily: null,
    chartMetric: 'messages',
    refreshKey: 0,
    refreshing: false,
    cachedScopes: {},
  });
  useNavigationStore.setState({ activePage: 'aiCareer' });
  vi.mocked(api.aiCareerHeatmap).mockResolvedValue(entries(12));
  vi.mocked(api.aiCareerTokenBytes).mockResolvedValue(100);
  vi.mocked(api.aiCareerDailyTokens).mockResolvedValue(tokenData(10));
  vi.mocked(api.getAvatar).mockResolvedValue(null);
});

afterEach(async () => {
  if (renderer) await act(async () => renderer.unmount());
  vi.unstubAllGlobals();
});

it('defaults to overview Tokens and keeps dashboard scans dormant while the page is hidden', async () => {
  const initialState = useAiCareerStore.getInitialState();
  expect(initialState.chartMetric).toBe('tokens');
  useAiCareerStore.setState(initialState);
  useNavigationStore.setState({ activePage: 'apps' });
  await mount();
  expect(api.aiCareerHeatmap).not.toHaveBeenCalled();
  expect(api.aiCareerTokenBytes).not.toHaveBeenCalled();
  expect(api.aiCareerDailyTokens).not.toHaveBeenCalled();
  expect(renderer.root.findAllByProps({ 'data-session-family': 'claude' })).toHaveLength(0);
  await act(async () => useNavigationStore.getState().setActivePage('aiCareer'));
  expect(api.aiCareerHeatmap).toHaveBeenCalledExactlyOnceWith(null);
  expect(api.aiCareerTokenBytes).toHaveBeenCalledExactlyOnceWith(null);
  expect(chartCount()).toBe(12);
  expect(heatmapCount()).toBe(12);
  expect(chart().props['data-metric']).toBe('tokens');
  expect(chart().props['data-token-status']).toBe('ready');
  expect(openDialog).not.toHaveBeenCalled();
  expect(api.setAvatar).not.toHaveBeenCalled();
  expect(api.aiCareerDailyTokens).toHaveBeenCalledExactlyOnceWith(null);
});

it('filters both summary requests and the session list to the selected family', async () => {
  await mount();
  vi.mocked(api.aiCareerHeatmap).mockResolvedValueOnce(entries(7));
  vi.mocked(api.aiCareerTokenBytes).mockResolvedValueOnce(50);
  await selectFamily('Claude');
  expect(api.aiCareerHeatmap).toHaveBeenLastCalledWith('claude');
  expect(api.aiCareerTokenBytes).toHaveBeenLastCalledWith('claude');
  expect(renderer.root.findByProps({ 'data-session-family': 'claude' })).toBeDefined();
  expect(chartCount()).toBe(7);
  expect(renderer.root.findAllByProps({ 'data-heatmap': true })).toHaveLength(0);
  expect(
    renderer.root.findAllByType('img').some((image) => image.props.src === '/default-avatar.webp')
  ).toBe(false);
  expect(openDialog).not.toHaveBeenCalled();
  expect(api.setAvatar).not.toHaveBeenCalled();
  expect(api.aiCareerDailyTokens).not.toHaveBeenCalled();
});

it('does not mix a previous family response into a newer dashboard', async () => {
  await mount();
  const claude = deferred<api.HeatmapEntry[]>();
  vi.mocked(api.aiCareerHeatmap).mockImplementation((family) =>
    family === 'claude' ? claude.promise : Promise.resolve(entries(3))
  );
  await selectFamily('Claude');
  await selectFamily('Codex');
  expect(chartCount()).toBe(3);
  await act(async () => claude.resolve(entries(999)));
  expect(chartCount()).toBe(3);
  expect(renderer.root.findAllByProps({ 'data-heatmap': true })).toHaveLength(0);
  expect(renderer.root.findAllByProps({ 'data-session-family': 'claude' })).toHaveLength(0);
  expect(renderer.root.findByProps({ 'data-session-family': 'codex' })).toBeDefined();
});

it('preserves cached statistics on refresh failure and allows an explicit retry', async () => {
  await mount();
  vi.mocked(api.aiCareerHeatmap).mockRejectedValueOnce(new Error('Fixture read failed'));
  await act(async () => useAiCareerStore.getState().refresh());
  expect(chartCount()).toBe(12);
  expect(heatmapCount()).toBe(12);
  const alert = renderer.root.findByProps({ role: 'alert' });
  expect(useAiCareerStore.getState().refreshing).toBe(false);
  vi.mocked(api.aiCareerHeatmap).mockResolvedValueOnce(entries(18));
  await act(async () => alert.findByType('button').props.onClick());
  expect(chartCount()).toBe(18);
  expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
});

it('ignores pending statistics after leaving and unmounts the session list', async () => {
  await mount();
  const pending = deferred<api.HeatmapEntry[]>();
  vi.mocked(api.aiCareerHeatmap).mockReturnValueOnce(pending.promise);
  await selectFamily('Claude');
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => pending.resolve(entries(200)));
  expect(renderer.root.findAllByProps({ 'data-chart': true })).toHaveLength(0);
  expect(renderer.root.findAllByProps({ 'data-session-family': 'claude' })).toHaveLength(0);
  vi.mocked(api.aiCareerHeatmap).mockResolvedValueOnce(entries(9));
  await act(async () => useNavigationStore.getState().setActivePage('aiCareer'));
  expect(chartCount()).toBe(9);
  expect(api.aiCareerHeatmap).toHaveBeenLastCalledWith('claude');
});

it('loads Token usage on selection and revalidates cached scopes while retaining the mode', async () => {
  await mount();
  await selectTokens();
  expect(api.aiCareerDailyTokens).toHaveBeenCalledExactlyOnceWith(null);
  expect(chart().props['data-token-status']).toBe('ready');
  vi.mocked(api.aiCareerDailyTokens).mockResolvedValueOnce(tokenData(42));
  await selectFamily('Claude');
  expect(chart().props['data-metric']).toBe('tokens');
  expect(api.aiCareerDailyTokens).toHaveBeenLastCalledWith('claude');
  expect([...tokenDays().values()][0].input).toBe(42);
  await act(async () => useAiCareerStore.getState().setSelectedFamily(null));
  expect(api.aiCareerDailyTokens).toHaveBeenCalledTimes(3);
  expect([...tokenDays().values()][0].input).toBe(10);
  await act(async () => useAiCareerStore.getState().setChartMetric('messages'));
  await selectTokens();
  expect(api.aiCareerDailyTokens).toHaveBeenCalledTimes(3);
});

it('aggregates native timestamps into local days and preserves recorded zero and absent days', async () => {
  vi.mocked(api.aiCareerDailyTokens).mockResolvedValue({
    entries: [
      { ts: new Date(2026, 9, 10, 23, 59).getTime() / 1000, input_tokens: 8, output_tokens: 2 },
      { ts: new Date(2026, 9, 11, 0, 0).getTime() / 1000, input_tokens: 20, output_tokens: 5 },
      { ts: new Date(2026, 9, 11, 0, 1).getTime() / 1000, input_tokens: 3, output_tokens: 7 },
      { ts: new Date(2026, 9, 12).getTime() / 1000, input_tokens: 0, output_tokens: 0 },
    ],
    unavailable_families: ['hermes'],
  });
  await mount();
  await selectTokens();
  expect(tokenDays().get('2026-10-10')).toEqual({ input: 8, output: 2 });
  expect(tokenDays().get('2026-10-11')).toEqual({ input: 23, output: 12 });
  expect(tokenDays().get('2026-10-12')).toEqual({ input: 0, output: 0 });
  expect(tokenDays().has('2026-10-09')).toBe(false);
});

it('ignores a previous family Token response after selection changes', async () => {
  await mount();
  await selectTokens();
  const claude = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerDailyTokens).mockImplementation((family) =>
    family === 'claude' ? claude.promise : Promise.resolve(tokenData(3))
  );
  await selectFamily('Claude');
  expect(chart().props['data-token-status']).toBe('loading');
  expect(tokenDays().size).toBe(0);
  await selectFamily('Codex');
  await act(async () => claude.resolve(tokenData(999)));
  expect([...tokenDays().values()][0].input).toBe(3);
  expect(chart().props['data-token-status']).toBe('ready');
});

it('keeps Token scans dormant while hidden and discards an in-flight response on exit', async () => {
  useAiCareerStore.setState({ chartMetric: 'tokens' });
  useNavigationStore.setState({ activePage: 'apps' });
  await mount();
  expect(api.aiCareerDailyTokens).not.toHaveBeenCalled();
  const pending = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerDailyTokens).mockReturnValueOnce(pending.promise);
  await act(async () => useNavigationStore.getState().setActivePage('aiCareer'));
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => pending.resolve(tokenData(999)));
  expect(renderer.root.findAllByProps({ 'data-chart': true })).toHaveLength(0);
  await act(async () => useNavigationStore.getState().setActivePage('aiCareer'));
  expect(api.aiCareerDailyTokens).toHaveBeenCalledTimes(2);
  expect([...tokenDays().values()][0].input).toBe(10);
});

it('preserves cached Token data on failure and retries only the explicit Token read', async () => {
  await mount();
  await selectTokens();
  vi.mocked(api.aiCareerDailyTokens).mockRejectedValueOnce(new Error('Read failed'));
  await act(async () => useAiCareerStore.getState().refresh());
  expect(chart().props['data-token-status']).toBe('error');
  expect([...tokenDays().values()][0].input).toBe(10);
  expect(useAiCareerStore.getState().refreshing).toBe(false);
  const statReads = vi.mocked(api.aiCareerHeatmap).mock.calls.length;
  vi.mocked(api.aiCareerDailyTokens).mockResolvedValueOnce(tokenData(25));
  await act(async () => renderer.root.findByProps({ 'data-token-retry': true }).props.onClick());
  expect(chart().props['data-token-status']).toBe('ready');
  expect([...tokenDays().values()][0].input).toBe(25);
  expect(api.aiCareerHeatmap).toHaveBeenCalledTimes(statReads);
  expect(openDialog).not.toHaveBeenCalled();
  expect(api.setAvatar).not.toHaveBeenCalled();
});

it('does not refetch on translation changes and finishes refresh after both current reads', async () => {
  await mount();
  await selectTokens();
  i18n.t = (key: string) => 'translated ' + key;
  await act(async () => renderer.update(<AiCareerMain />));
  expect(api.aiCareerDailyTokens).toHaveBeenCalledTimes(1);
  const pending = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerDailyTokens).mockReturnValueOnce(pending.promise);
  await act(async () => useAiCareerStore.getState().refresh());
  expect(useAiCareerStore.getState().refreshing).toBe(true);
  expect(chart().props['data-token-status']).toBe('ready');
  expect([...tokenDays().values()][0].input).toBe(10);
  await act(async () => pending.resolve(tokenData(50)));
  expect(useAiCareerStore.getState().refreshing).toBe(false);
  expect([...tokenDays().values()][0].input).toBe(50);
});

it('shows successful cached statistics and Tokens before a fresh response replaces them', async () => {
  useAiCareerStore.getState().cacheDashboard(null, entries(88), 200);
  useAiCareerStore.getState().cacheDailyTokens(null, tokenData(24));
  useAiCareerStore.setState({ chartMetric: 'tokens' });
  const pendingStats = deferred<api.HeatmapEntry[]>();
  const pendingTokens = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerHeatmap).mockReturnValueOnce(pendingStats.promise);
  vi.mocked(api.aiCareerDailyTokens).mockReturnValueOnce(pendingTokens.promise);
  writeSnapshot.mockClear();
  await mount();
  expect(chartCount()).toBe(88);
  expect(heatmapCount()).toBe(88);
  expect([...tokenDays().values()][0].input).toBe(24);
  expect(chart().props['data-token-status']).toBe('ready');
  expect(api.aiCareerHeatmap).toHaveBeenCalledExactlyOnceWith(null);
  expect(api.aiCareerDailyTokens).toHaveBeenCalledExactlyOnceWith(null);
  expect(writeSnapshot).not.toHaveBeenCalled();
  await act(async () => {
    pendingStats.resolve(entries(18));
    pendingTokens.resolve(tokenData(40));
  });
  expect(chartCount()).toBe(18);
  expect([...tokenDays().values()][0].input).toBe(40);
  expect(writeSnapshot).toHaveBeenCalledTimes(2);
});

it('treats cached empty responses as ready and preserves them after a background failure', async () => {
  useAiCareerStore.getState().cacheDashboard(null, [], 0);
  useAiCareerStore.getState().cacheDailyTokens(null, { entries: [], unavailable_families: [] });
  useAiCareerStore.setState({ chartMetric: 'tokens' });
  vi.mocked(api.aiCareerHeatmap).mockRejectedValueOnce(new Error('background failure'));
  vi.mocked(api.aiCareerDailyTokens).mockRejectedValueOnce(new Error('background failure'));
  writeSnapshot.mockClear();
  await mount();
  expect(chartCount()).toBe(0);
  expect(chart().props['data-token-status']).toBe('ready');
  expect(renderer.root.findAllByProps({ role: 'alert' })).toHaveLength(0);
  expect(useAiCareerStore.getState().cachedScopes.overview).toEqual({
    dashboard: { heatmapEntries: [], tokenBytes: 0 },
    dailyTokens: { entries: [], unavailable_families: [] },
  });
  expect(writeSnapshot).not.toHaveBeenCalled();
});

it('revalidates a reentered page while preserving cached values and rejects its late result after exit', async () => {
  useAiCareerStore.setState({ chartMetric: 'tokens' });
  await mount();
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  const pendingStats = deferred<api.HeatmapEntry[]>();
  const pendingTokens = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerHeatmap).mockReturnValueOnce(pendingStats.promise);
  vi.mocked(api.aiCareerDailyTokens).mockReturnValueOnce(pendingTokens.promise);
  writeSnapshot.mockClear();
  await act(async () => useNavigationStore.getState().setActivePage('aiCareer'));
  expect(chartCount()).toBe(12);
  expect([...tokenDays().values()][0].input).toBe(10);
  expect(chart().props['data-token-status']).toBe('ready');
  expect(api.aiCareerHeatmap).toHaveBeenCalledTimes(2);
  expect(api.aiCareerDailyTokens).toHaveBeenCalledTimes(2);
  await act(async () => useNavigationStore.getState().setActivePage('apps'));
  await act(async () => {
    pendingStats.resolve(entries(999));
    pendingTokens.resolve(tokenData(999));
  });
  expect(useAiCareerStore.getState().cachedScopes.overview?.dashboard?.heatmapEntries).toEqual(
    entries(12)
  );
  expect(useAiCareerStore.getState().cachedScopes.overview?.dailyTokens).toEqual(tokenData(10));
  expect(writeSnapshot).not.toHaveBeenCalled();
});

it('does not publish or persist a late family refresh over the current generation', async () => {
  useAiCareerStore.setState({ selectedFamily: 'claude', chartMetric: 'tokens' });
  const oldStats = deferred<api.HeatmapEntry[]>();
  const oldTokens = deferred<api.DailyTokenResponse>();
  vi.mocked(api.aiCareerHeatmap).mockReturnValueOnce(oldStats.promise);
  vi.mocked(api.aiCareerDailyTokens).mockReturnValueOnce(oldTokens.promise);
  await mount();
  await act(async () => useAiCareerStore.getState().refresh());
  expect(chartCount()).toBe(12);
  expect([...tokenDays().values()][0].input).toBe(10);
  const writes = writeSnapshot.mock.calls.length;
  await act(async () => {
    oldStats.resolve(entries(999));
    oldTokens.resolve(tokenData(999));
  });
  expect(chartCount()).toBe(12);
  expect([...tokenDays().values()][0].input).toBe(10);
  expect(writeSnapshot).toHaveBeenCalledTimes(writes);
  expect(useAiCareerStore.getState().cachedScopes.claude?.dailyTokens).toEqual(tokenData(10));
});
