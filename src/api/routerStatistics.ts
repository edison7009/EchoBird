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
