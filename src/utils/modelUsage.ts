import type { ModelUsageData, UsageQuota } from '../api/models';
import type { TKey } from '../i18n';

export const quotaPeriodKeys: Record<NonNullable<UsageQuota['period']>, TKey> = {
  fiveHour: 'model.quota.fiveHour',
  daily: 'model.quota.daily',
  weekly: 'model.quota.weekly',
  monthly: 'model.quota.monthly',
};

export function quotaPercent(quota: UsageQuota): number | null {
  if (!Number.isFinite(quota.percentage)) return null;
  const used = Math.min(100, Math.max(0, quota.percentage));
  return quota.period ? 100 - used : used;
}

export function formatQuotaPercent(quota: UsageQuota): string {
  const percent = quotaPercent(quota);
  return percent === null ? '—' : `${Number(percent.toFixed(1))}%`;
}

export function compactCountdown(resetAt: number, now: number): string {
  if (!Number.isFinite(resetAt) || resetAt <= 0) return '';
  const minutes = Math.max(0, Math.ceil((resetAt - now) / 60_000));
  if (minutes >= 1440) return `${Math.floor(minutes / 1440)}d${Math.floor((minutes % 1440) / 60)}h`;
  if (minutes >= 60) return `${Math.floor(minutes / 60)}h${minutes % 60}m`;
  return `${minutes}m`;
}

export function primaryQuota(usage: ModelUsageData): UsageQuota | undefined {
  const quotas = usage.quotas.filter((quota) => quota.balance == null);
  return (
    quotas.find((quota) => quota.period === 'fiveHour') ??
    quotas.find((quota) => quota.period === 'daily') ??
    quotas[0]
  );
}

export function compactModelUsage(usage: ModelUsageData, now: number): string {
  const primary = primaryQuota(usage);
  if (!primary) return '';
  const weekly = usage.quotas.find((quota) => quota.balance == null && quota.period === 'weekly');
  // Legacy quotas may carry placeholder reset times without a known period.
  const countdown = primary.period ? compactCountdown(primary.resetAt, now) : '';
  const prefix = primary.period === 'weekly' ? '7d ' : primary.period === 'monthly' ? '1mo ' : '';
  return `${prefix}${formatQuotaPercent(primary)}${countdown ? ` ${countdown}` : ''}${
    weekly && weekly !== primary ? `(${formatQuotaPercent(weekly)})` : ''
  }`;
}
