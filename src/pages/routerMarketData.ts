import type {
  RouterMarketMetrics,
  RouterMarketPoint,
  RouterStatisticsCatalog,
  RouterStatisticsSnapshot,
} from '../api/routerStatistics';

export type MarketMetric = 'speed' | 'cost';
export const emptyMarketMetrics = (): RouterMarketMetrics => ({
  attempts: 0,
  completed: 0,
  failed: 0,
  cancelled: 0,
  speedSamples: 0,
  outputTokens: 0,
  generationMs: 0,
  firstOutputMs: 0,
  firstOutputSamples: 0,
  usageReports: 0,
  totalTokens: 0,
  pricedReports: 0,
  pricedTokens: 0,
  costMicros: 0,
  completedMicros: 0,
  failedMicros: 0,
  completedPricedReports: 0,
  failedPricedReports: 0,
});
export function addMarketMetrics(target: RouterMarketMetrics, value: RouterMarketMetrics) {
  for (const key of Object.keys(target) as (keyof RouterMarketMetrics)[]) target[key] += value[key];
}
export function marketValue(value: RouterMarketMetrics, metric: MarketMetric): number | null {
  if (metric === 'speed')
    return value.speedSamples > 0 && value.generationMs > 0
      ? (value.outputTokens * 1000) / value.generationMs
      : null;
  // USD / million reported, priced tokens. Unknown samples never count as free.
  return value.pricedReports > 0 && value.pricedTokens > 0
    ? value.costMicros / value.pricedTokens
    : null;
}
export function marketChange(
  current: number | null,
  baseline: number | null,
  metric: MarketMetric
) {
  if (current === null || baseline === null || baseline <= 0) return null;
  return (metric === 'speed' ? current / baseline - 1 : 1 - current / baseline) * 100;
}
export function marketModels(
  snapshot: RouterStatisticsSnapshot | null,
  catalog: RouterStatisticsCatalog | null,
  account: string | undefined
) {
  if (!snapshot || !account) return [];
  const saved = snapshot.rows.filter((r) => r.account === account && r.counts.attempts > 0);
  const models = new Map<
    string,
    {
      id: string;
      name: string;
      attempts: number;
      rowIds: Set<string>;
      metrics: RouterMarketMetrics;
      baseline: RouterMarketMetrics;
      trend: RouterMarketPoint[];
    }
  >();
  for (const row of saved) {
    let model = models.get(row.model);
    if (!model) {
      model = {
        id: row.model,
        name: '',
        attempts: 0,
        rowIds: new Set(),
        metrics: emptyMarketMetrics(),
        baseline: emptyMarketMetrics(),
        trend: [],
      };
      models.set(row.model, model);
    }
    model.attempts += row.counts.attempts;
    model.rowIds.add(row.id);
    model.name ||= catalog?.sources.find((s) => s.id === row.id)?.modelName ?? '';
    const market = snapshot.market?.rows.find((m) => m.row === row.id);
    if (market) {
      addMarketMetrics(model.metrics, market.metrics);
      addMarketMetrics(model.baseline, market.baseline);
    }
  }
  for (const model of models.values()) {
    const points = new Map<string, RouterMarketPoint>();
    for (const point of snapshot.market?.trend ?? []) {
      if (!model.rowIds.has(point.row)) continue;
      const key = `${point.day}/${point.hour}`;
      let target = points.get(key);
      if (!target) {
        target = { ...point, metrics: emptyMarketMetrics() };
        points.set(key, target);
      }
      addMarketMetrics(target.metrics, point.metrics);
    }
    model.trend = [...points.values()].sort((a, b) => a.day - b.day || a.hour - b.hour);
  }
  return [...models.values()].sort((a, b) => b.attempts - a.attempts || a.id.localeCompare(b.id));
}
export function baselineValue(
  snapshot: RouterStatisticsSnapshot | null,
  model: ReturnType<typeof marketModels>[number] | undefined,
  metric: MarketMetric
) {
  if (!snapshot?.market?.baselineRetained || !model) return null;
  const samples = metric === 'speed' ? model.baseline.speedSamples : model.baseline.pricedReports;
  return samples >= 3 ? marketValue(model.baseline, metric) : null;
}

export function completionRateChange(
  snapshot: RouterStatisticsSnapshot | null,
  account: string,
  counts: { attempts: number; completed: number; failed: number }
): number | null {
  if (!snapshot?.market?.baselineRetained || !(counts.completed + counts.failed)) return null;
  const ids = new Set(snapshot.rows.filter((row) => row.account === account).map((row) => row.id));
  let attempts = 0;
  let completed = 0;
  for (const row of snapshot.market.rows) {
    if (!ids.has(row.row)) continue;
    attempts += row.baseline.completed + row.baseline.failed;
    completed += row.baseline.completed;
  }
  return attempts >= 3
    ? (counts.completed / (counts.completed + counts.failed) - completed / attempts) * 100
    : null;
}
