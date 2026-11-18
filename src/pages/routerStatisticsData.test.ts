import { expect, it } from 'vitest';
import type { RouterStatisticsCatalog, RouterStatisticsSnapshot } from '../api/routerStatistics';
import { emptyStatisticsCounts, statisticsRows } from './routerStatisticsData';

const source = (id: string, group: string, model: string) => ({
  id,
  group,
  account: 'stable-account',
  model,
  name: 'Main account',
  modelName: model,
  provider: 'codex',
  enabled: true,
});
const catalog: RouterStatisticsCatalog = {
  groups: [],
  capped: false,
  sources: [
    source('one', 'g1', 'm1'),
    source('two', 'g1', 'm2'),
    source('three', 'g2', 'm1'),
    { ...source('unused', 'g1', 'm3'), account: 'unused-account' },
  ],
};
const snapshot: RouterStatisticsSnapshot = {
  recording: true,
  since: 1,
  savedAt: 1,
  storageError: false,
  capped: false,
  today: 739916,
  totals: { calls: 2, completed: 2, failed: 0, cancelled: 0, rescued: 0, durationMs: 10 },
  trend: [],
  rows: catalog.sources.slice(0, 3).map((item, index) => ({
    ...item,
    lastUsedAt: index + 10,
    counts: {
      ...emptyStatisticsCounts(),
      attempts: index + 1,
      completed: 1,
      protections: [index + 1, 0, 0, 0, 0],
    },
  })),
};

it('merges stable accounts across models and groups while retaining individual model and protection counts', () => {
  const rows = statisticsRows(snapshot, catalog, '');
  expect(rows).toHaveLength(1);
  expect(rows[0]).toMatchObject({
    account: 'stable-account',
    lastUsedAt: 12,
    groups: ['g1', 'g2'],
    counts: { attempts: 6, completed: 3, protections: [6, 0, 0, 0, 0] },
  });
  expect(rows[0].models).toEqual([
    { name: 'm1', attempts: 4 },
    { name: 'm2', attempts: 2 },
  ]);
});
it('filters by connection before merging sources and keeps model usage in the details', () => {
  expect(statisticsRows(snapshot, catalog, 'g1')[0].counts.attempts).toBe(3);
  const rows = statisticsRows(snapshot, catalog, '');
  expect(rows).toHaveLength(1);
  expect(rows[0]).toMatchObject({ counts: { attempts: 6 } });
  expect(rows[0].models).toHaveLength(2);
});

it('shows account and API sources together without duplicating account models or combining different sources', () => {
  const api = {
    ...source('api', 'g1', 'm1'),
    account: 'api-source',
    name: 'API model',
    provider: 'api.example',
  };
  const rows = statisticsRows(
    { ...snapshot, rows: [...snapshot.rows, { ...snapshot.rows[0], ...api }] },
    { ...catalog, sources: [...catalog.sources, api] },
    ''
  );
  expect(rows).toHaveLength(2);
  expect(rows.map((row) => row.name)).toEqual(['Main account', 'API model']);
  expect(rows.map((row) => row.counts.attempts)).toEqual([6, 1]);
  expect(rows.map((row) => row.counts.protections[0])).toEqual([6, 1]);
  expect(rows[0].models).toEqual([
    { name: 'm1', attempts: 4 },
    { name: 'm2', attempts: 2 },
  ]);
});

it('does not turn configured sources into usage rows when no statistics exist', () => {
  expect(statisticsRows(null, catalog, '')).toEqual([]);
  expect(statisticsRows({ ...snapshot, since: 0, rows: [] }, catalog, '')).toEqual([]);
  expect(statisticsRows({ ...snapshot, rows: [] }, catalog, 'g1')).toEqual([]);
});
it('keeps historical removed sources without inventing names and preserves the latest use outside the selected window', () => {
  const rows = statisticsRows(
    { ...snapshot, rows: [{ ...snapshot.rows[0], counts: emptyStatisticsCounts() }] },
    null,
    ''
  );
  expect(rows[0]).toMatchObject({
    name: '',
    archived: true,
    enabled: false,
    lastUsedAt: 10,
    counts: { attempts: 0 },
  });
});

it('uses the current account name when a removed model is encountered before an active one', () => {
  const rows = statisticsRows(snapshot, { ...catalog, sources: catalog.sources.slice(1) }, '');
  expect(rows).toHaveLength(1);
  expect(rows[0]).toMatchObject({
    name: 'Main account',
    provider: 'codex',
    archived: false,
    enabled: true,
    counts: { attempts: 6, protections: [6, 0, 0, 0, 0] },
  });
});

it('merges only reported token usage across models and applies group filtering before aggregation', () => {
  const rows = snapshot.rows.map((row, index) => ({
    ...row,
    counts: {
      ...row.counts,
      usage: {
        inputTokens: 100 * (index + 1),
        outputTokens: 20 * (index + 1),
        totalTokens: 120 * (index + 1),
        reportedResponses: index === 1 ? 0 : 1,
      },
    },
  }));
  const data = { ...snapshot, rows };
  expect(statisticsRows(data, catalog, '')[0].counts.usage).toEqual({
    inputTokens: 400,
    outputTokens: 80,
    totalTokens: 480,
    reportedResponses: 2,
  });
  expect(statisticsRows(data, catalog, 'g1')[0].counts.usage?.totalTokens).toBe(120);
  expect(statisticsRows(snapshot, catalog, '')[0].counts.usage).toBeUndefined();
  expect(rows[0].counts.usage.totalTokens).toBe(120);
});
