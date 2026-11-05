import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { DailyTokenResponse, SavedSession } from '../api/aiCareer';

const STORAGE_KEY = 'echobird-ai-career-cache';
let saved: string | null;
let getItem: ReturnType<typeof vi.fn>;
let setItem: ReturnType<typeof vi.fn>;

async function restart() {
  vi.resetModules();
  return (await import('./aiCareerStore')).useAiCareerStore;
}

function session(index: number): SavedSession {
  return {
    id: `codex_${index}`,
    name: `Session ${index}`,
    tool: 'codex',
    cwd: 'C:\\项目',
    session_token: `native-${index}`,
    saved_at: '2026-10-11T10:00:00Z',
    file_path: `C:\\sessions\\${index}.jsonl`,
    turn_count: null,
    input_tokens: 0,
    output_tokens: null,
  };
}

beforeEach(() => {
  saved = null;
  getItem = vi.fn(() => saved);
  setItem = vi.fn((_key: string, value: string) => {
    saved = value;
  });
  vi.stubGlobal('localStorage', { getItem, setItem });
});

afterEach(() => vi.unstubAllGlobals());

it('restores successful scope data synchronously across a restart without persisting selection', async () => {
  const first = await restart();
  first.getState().cacheDashboard(null, [{ ts: 1_791_676_800, count: 12 }], 100);
  first.getState().cacheDashboard('claude', [], 0);
  first.getState().cacheDailyTokens(null, { entries: [], unavailable_families: [] });
  first.getState().cacheSessions(
    'codex',
    Array.from({ length: 25 }, (_, i) => session(i))
  );
  first.getState().setSelectedFamily('codex');
  first.getState().setChartMetric('messages');
  first.getState().refresh();
  const second = await restart();
  expect(second.getState().cachedScopes.overview?.dashboard?.heatmapEntries).toEqual([
    { ts: 1_791_676_800, count: 12 },
  ]);
  expect(second.getState().cachedScopes.claude?.dashboard).toEqual({
    heatmapEntries: [],
    tokenBytes: 0,
  });
  expect(second.getState().cachedScopes.overview?.dailyTokens).toEqual({
    entries: [],
    unavailable_families: [],
  });
  expect(second.getState().cachedScopes.codex?.sessions).toHaveLength(20);
  expect(second.getState().cachedScopes.codex?.sessions?.[0]).toEqual(session(0));
  expect(second.getState().selectedFamily).toBeNull();
  expect(second.getState().chartMetric).toBe('tokens');
  expect(second.getState().refreshKey).toBe(0);
  expect(second.getState().refreshing).toBe(false);
  expect(Object.keys(JSON.parse(saved!))).toEqual(['version', 'data']);
  expect(getItem).toHaveBeenLastCalledWith(STORAGE_KEY);
});

it('does not publish or write an unchanged successful response', async () => {
  const store = await restart();
  const listener = vi.fn();
  store.subscribe(listener);
  store.getState().cacheSessions('codex', [session(1)]);
  const previous = store.getState().cachedScopes;
  store.getState().cacheSessions('codex', [session(1)]);
  expect(store.getState().cachedScopes).toBe(previous);
  expect(listener).toHaveBeenCalledTimes(1);
  expect(setItem).toHaveBeenCalledTimes(1);
});

it('restores native model metadata alongside sessions cached before the fields existed', async () => {
  const legacy = session(1);
  const known = { ...session(2), model_id: 'provider/model-v1', message_count: 0 };
  const unknown = { ...session(3), model_id: null, message_count: null };
  const first = await restart();
  first.getState().cacheSessions('codex', [legacy, known, unknown]);
  const second = await restart();
  expect(second.getState().cachedScopes.codex?.sessions).toEqual([legacy, known, unknown]);
  expect(setItem).toHaveBeenCalledTimes(1);
});

it.each([{ model_id: 123 }, { message_count: '5' }, { message_count: -1 }])(
  'rejects invalid cached session metadata without discarding other sections: %o',
  async (metadata) => {
    saved = JSON.stringify({
      version: 1,
      data: {
        codex: {
          dashboard: { heatmapEntries: [], tokenBytes: 0 },
          sessions: [{ ...session(1), ...metadata }],
        },
      },
    });
    const store = await restart();
    expect(store.getState().cachedScopes.codex).toEqual({
      dashboard: { heatmapEntries: [], tokenBytes: 0 },
    });
    expect(setItem).not.toHaveBeenCalled();
  }
);

it.each(['broken json', JSON.stringify({ version: 999, data: {} })])(
  'ignores a damaged or unsupported snapshot: %s',
  async (value) => {
    saved = value;
    const store = await restart();
    expect(store.getState().cachedScopes).toEqual({});
    expect(setItem).not.toHaveBeenCalled();
  }
);

it('rejects malformed sections while preserving valid empty scope data', async () => {
  saved = JSON.stringify({
    version: 1,
    data: {
      unknown: { sessions: [session(1)] },
      overview: { dashboard: { heatmapEntries: [{ ts: 'bad', count: 1 }], tokenBytes: 0 } },
      codex: { dashboard: { heatmapEntries: [], tokenBytes: 0 }, sessions: [{}] },
      claude: { sessions: [] },
    },
  });
  const store = await restart();
  expect(store.getState().cachedScopes).toEqual({
    codex: { dashboard: { heatmapEntries: [], tokenBytes: 0 } },
    claude: { sessions: [] },
  });
});

it('keeps data in memory when storage cannot be read or written', async () => {
  getItem.mockImplementation(() => {
    throw new Error('read blocked');
  });
  setItem.mockImplementation(() => {
    throw new Error('quota exceeded');
  });
  const store = await restart();
  expect(store.getState().cachedScopes).toEqual({});
  store.getState().cacheDashboard(null, [], 0);
  expect(store.getState().cachedScopes.overview?.dashboard).toEqual({
    heatmapEntries: [],
    tokenBytes: 0,
  });
  expect(setItem).toHaveBeenCalledTimes(1);
});

it('persists one hundred thousand daily samples compactly with exact totals and recorded zero', async () => {
  setItem.mockImplementation((_key: string, value: string) => {
    if (value.length > 100_000) throw new Error('fixture quota exceeded');
    saved = value;
  });
  const first = await restart();
  const start = new Date(2026, 9, 1, 12).getTime() / 1000;
  const data: DailyTokenResponse = {
    entries: Array.from({ length: 100_000 }, (_, index) => ({
      ts: start + (index % 20) * 86_400,
      input_tokens: 3,
      output_tokens: 2,
    })),
    unavailable_families: ['hermes'],
  };
  data.entries.push({
    ts: new Date(2026, 9, 22, 12).getTime() / 1000,
    input_tokens: 0,
    output_tokens: 0,
  });
  expect(JSON.stringify({ overview: data, codex: data }).length).toBeGreaterThan(10_000_000);
  first.getState().cacheDashboard(
    null,
    [
      { ts: start, count: 2 },
      { ts: start, count: 3 },
    ],
    100
  );
  first.getState().cacheDailyTokens(null, data);
  first.getState().cacheDailyTokens('codex', data);
  expect(first.getState().cachedScopes.overview?.dailyTokens?.entries).toHaveLength(100_001);
  expect(saved!.length).toBeLessThan(10_000);
  const second = await restart();
  const restored = second.getState().cachedScopes.overview?.dailyTokens;
  expect(restored?.entries).toHaveLength(21);
  expect(restored?.entries.reduce((sum, entry) => sum + entry.input_tokens, 0)).toBe(300_000);
  expect(restored?.entries.reduce((sum, entry) => sum + entry.output_tokens, 0)).toBe(200_000);
  expect(restored?.entries[20]).toEqual(data.entries[100_000]);
  expect(restored?.unavailable_families).toEqual(['hermes']);
  expect(second.getState().cachedScopes.codex?.dailyTokens).toEqual(restored);
  expect(second.getState().cachedScopes.overview?.dashboard?.heatmapEntries).toHaveLength(2);
  setItem.mockClear();
  second.getState().cacheDailyTokens(null, data);
  second.getState().cacheDailyTokens('codex', data);
  expect(second.getState().cachedScopes.overview?.dailyTokens?.entries).toHaveLength(100_001);
  expect(setItem).not.toHaveBeenCalled();
  second.getState().cacheDailyTokens(null, {
    ...data,
    entries: [{ ...data.entries[0], input_tokens: 4 }, ...data.entries.slice(1)],
  });
  expect(setItem).toHaveBeenCalledTimes(1);
});

it('drops only daily snapshots when their recorded timezone differs', async () => {
  const first = await restart();
  first.getState().cacheDashboard('codex', [], 0);
  first.getState().cacheSessions('codex', [session(1)]);
  first.getState().cacheDailyTokens('codex', { entries: [], unavailable_families: [] });
  const persisted = JSON.parse(saved!);
  persisted.data.codex.dailyTokens.timeZone = 'different-timezone';
  saved = JSON.stringify(persisted);
  const second = await restart();
  expect(second.getState().cachedScopes.codex?.dailyTokens).toBeUndefined();
  expect(second.getState().cachedScopes.codex?.dashboard).toEqual({
    heatmapEntries: [],
    tokenBytes: 0,
  });
  expect(second.getState().cachedScopes.codex?.sessions).toEqual([session(1)]);
});

it('does not aggregate unchanged token samples for unrelated scope writes', async () => {
  const store = await restart();
  const aggregate = vi.spyOn(Date.prototype, 'toDateString');
  const data: DailyTokenResponse = {
    entries: Array.from({ length: 100_000 }, (_, index) => ({
      ts: 1_791_676_800 + index,
      input_tokens: 3,
      output_tokens: 2,
    })),
    unavailable_families: [],
  };
  store.getState().cacheDailyTokens(null, data);
  expect(aggregate).toHaveBeenCalledTimes(100_000);
  aggregate.mockClear();
  setItem.mockClear();
  store.getState().cacheDailyTokens(null, data);
  store
    .getState()
    .cacheDailyTokens(null, { ...data, entries: data.entries.map((entry) => ({ ...entry })) });
  expect(setItem).not.toHaveBeenCalled();
  store.getState().cacheDashboard('codex', [{ ts: 1_791_676_800, count: 1 }], 3);
  expect(aggregate).not.toHaveBeenCalled();
  expect(setItem).toHaveBeenCalledOnce();
  aggregate.mockRestore();
});

it('releases memory on disable and ignores late cache results without erasing the saved archive', async () => {
  const store = await restart();
  const { useNavigationStore } = await import('./navigationStore');
  store.getState().cacheDashboard(null, [{ ts: 1_791_676_800, count: 12 }], 100);
  expect(store.getState().cachedScopes.overview?.dashboard).toBeDefined();
  const persisted = saved;
  setItem.mockClear();
  useNavigationStore.getState().setOptionalPages({ aiCareer: false });
  expect(store.getState().cachedScopes).toEqual({});
  store.getState().cacheDashboard(null, [{ ts: 1_791_676_800, count: 99 }], 999);
  expect(store.getState().cachedScopes).toEqual({});
  expect(setItem).not.toHaveBeenCalled();
  expect(saved).toBe(persisted);
  useNavigationStore.getState().setOptionalPages({ aiCareer: true });
  store.getState().cacheDashboard(null, [{ ts: 1_791_676_800, count: 13 }], 101);
  expect(store.getState().cachedScopes.overview?.dashboard?.tokenBytes).toBe(101);
});
