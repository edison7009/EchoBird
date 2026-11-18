export const ROUTER_STATISTICS_CHANGED = 'router-statistics-settings-changed';
import { invoke } from '@tauri-apps/api/core';

export interface RouterTokenUsage {
  inputTokens: number;
  outputTokens: number;
  totalTokens: number;
  reportedResponses: number;
}
export interface RouterTokenCredits {
  ledgerId: string;
  total: number;
}
export function getSmartRouterTokenCredits(): Promise<RouterTokenCredits> {
  return invoke('get_smart_router_token_credits');
}

export interface RouterStatisticsCounts {
  usage?: RouterTokenUsage;
  attempts: number;
  completed: number;
  failed: number;
  cancelled: number;
  protections: number[];
  errors: number[];
}
export interface RouterStatisticsTotals {
  usage?: RouterTokenUsage;
  calls: number;
  completed: number;
  failed: number;
  cancelled: number;
  rescued: number;
  durationMs: number;
}
export interface RouterStatisticsRow {
  id: string;
  group: string;
  account: string;
  model: string;
  lastUsedAt: number;
  counts: RouterStatisticsCounts;
}
export interface RouterStatisticsSnapshot {
  recording: boolean;
  since: number;
  savedAt: number;
  storageError: boolean;
  capped: boolean;
  today: number;
  totals: RouterStatisticsTotals;
  rows: RouterStatisticsRow[];
  trend: { day: number; hour: number; totals: RouterStatisticsTotals }[];
  market?: RouterMarketSnapshot;
}
export interface RouterStatisticsSource {
  id: string;
  group: string;
  account: string;
  model: string;
  name: string;
  modelName: string;
  provider: string;
  enabled: boolean;
}
export interface RouterStatisticsCatalog {
  groups: { id: string; name: string }[];
  sources: RouterStatisticsSource[];
  capped: boolean;
}
export function getSmartRouterStatistics(
  days: number,
  group?: string
): Promise<RouterStatisticsSnapshot> {
  return invoke('get_smart_router_statistics', { days, group: group || null });
}
export function getSmartRouterStatisticsCatalog(): Promise<RouterStatisticsCatalog> {
  return invoke('get_smart_router_statistics_catalog');
}

export interface RouterMarketMetrics {
  attempts: number;
  completed: number;
  failed: number;
  cancelled: number;
  speedSamples: number;
  outputTokens: number;
  generationMs: number;
  firstOutputMs: number;
  firstOutputSamples: number;
  usageReports: number;
  totalTokens: number;
  pricedReports: number;
  pricedTokens: number;
  costMicros: number;
  completedMicros: number;
  failedMicros: number;
  completedPricedReports: number;
  failedPricedReports: number;
}
export interface RouterReferencePrice {
  input: number;
  output: number;
  cacheRead?: number | null;
  cacheWrite?: number | null;
  provider: string;
  fetchedAt: number;
}
export interface RouterMarketPoint {
  row: string;
  day: number;
  hour: number;
  metrics: RouterMarketMetrics;
}
export interface RouterMarketEvent {
  row: string;
  at: number;
  day: number;
  hour: number;
  outcome: 'completed' | 'failed' | 'cancelled';
  elapsedMs: number;
  firstOutputMs: number | null;
  generationMs: number | null;
  outputTokens: number | null;
  totalTokens: number | null;
  costMicros: number | null;
  price: RouterReferencePrice | null;
}
export interface RouterMarketSnapshot {
  rows: { row: string; metrics: RouterMarketMetrics; baseline: RouterMarketMetrics }[];
  trend: RouterMarketPoint[];
  events: RouterMarketEvent[];
  capped: boolean;
  baselineStart: number;
  baselineEnd: number;
  baselineRetained: boolean;
  pricing: { fetchedAt: number; models: number };
}
export function refreshSmartRouterReferencePrices(): Promise<{
  fetchedAt: number;
  models: number;
}> {
  return invoke('refresh_smart_router_reference_prices');
}
