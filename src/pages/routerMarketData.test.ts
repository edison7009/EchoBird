import { expect, it } from 'vitest';
import {
  baselineValue,
  completionRateChange,
  emptyMarketMetrics,
  marketChange,
  marketModels,
  marketValue,
} from './routerMarketData';
import type { RouterStatisticsSnapshot, RouterStatisticsCatalog } from '../api/routerStatistics';
const metrics = {
  ...emptyMarketMetrics(),
  speedSamples: 3,
  outputTokens: 300,
  generationMs: 10_000,
  pricedReports: 2,
  pricedTokens: 100_000,
  costMicros: 200_000,
};
it('keeps unmeasured samples unknown, accepts free prices and inverts cost improvement without division by zero', () => {
  expect(marketValue(emptyMarketMetrics(), 'speed')).toBeNull();
  expect(marketValue(emptyMarketMetrics(), 'cost')).toBeNull();
  expect(marketValue(metrics, 'speed')).toBe(30);
  expect(marketValue(metrics, 'cost')).toBe(2);
  expect(marketValue({ ...metrics, costMicros: 0 }, 'cost')).toBe(0);
  expect(marketChange(42, 35, 'speed')).toBeCloseTo(20);
  expect(marketChange(8, 10, 'cost')).toBeCloseTo(20);
  expect(marketChange(1, 0, 'cost')).toBeNull();
});
it('separates models and accounts while merging the same model across groups and hours', () => {
  const rows = ['a', 'b', 'c', 'other'].map((id, i) => ({
    id,
    account: i === 3 ? 'other' : 'one',
    model: i === 1 ? 'm2' : 'm1',
    group: id,
    lastUsedAt: 1,
    counts: { attempts: 10, completed: 10, failed: 0, cancelled: 0, protections: [], errors: [] },
  }));
  const snapshot: RouterStatisticsSnapshot = {
    recording: true,
    since: 1,
    savedAt: 1,
    storageError: false,
    capped: false,
    today: 10,
    totals: { calls: 40, completed: 40, failed: 0, cancelled: 0, rescued: 0, durationMs: 1000 },
    trend: [],
    rows,
    market: {
      rows: rows.map((row) => ({ row: row.id, metrics: { ...metrics }, baseline: { ...metrics } })),
      trend: rows.map((row) => ({ row: row.id, day: 10, hour: 2, metrics: { ...metrics } })),
      baselineRetained: true,
      baselineStart: 3,
      baselineEnd: 9,
      capped: false,
      events: [],
      pricing: { fetchedAt: 1, models: 1 },
    },
  };
  const catalog: RouterStatisticsCatalog = {
    groups: [],
    capped: false,
    sources: rows.map((row) => ({
      ...row,
      modelName: row.model,
      name: row.account,
      provider: 'fixture',
      enabled: true,
    })),
  };
  expect(
    completionRateChange(snapshot, 'one', { attempts: 110, completed: 99, failed: 1 })
  ).toBeNull();
  for (const row of snapshot.market!.rows) {
    row.baseline.completed = 96;
    row.baseline.failed = 4;
  }
  expect(
    completionRateChange(snapshot, 'one', { attempts: 110, completed: 99, failed: 1 })
  ).toBeCloseTo(3);
  expect(
    completionRateChange(snapshot, 'absent', { attempts: 1, completed: 1, failed: 0 })
  ).toBeNull();
  const models = marketModels(snapshot, catalog, 'one');
  expect(models).toHaveLength(2);
  expect(models[0].rowIds).toEqual(new Set(['a', 'c']));
  expect(models[0].metrics.outputTokens).toBe(600);
  expect(models[0].trend).toHaveLength(1);
  expect(models[0].trend[0].metrics.speedSamples).toBe(6);
  expect(baselineValue(snapshot, models[0], 'speed')).toBe(30);
  expect(
    baselineValue(
      { ...snapshot, market: { ...snapshot.market!, baselineRetained: false } },
      models[0],
      'speed'
    )
  ).toBeNull();
  expect(marketModels(snapshot, catalog, 'missing')).toHaveLength(0);
  expect(marketModels(null, catalog, 'one')).toHaveLength(0);
});
