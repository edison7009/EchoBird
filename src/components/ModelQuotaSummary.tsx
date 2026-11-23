import type { ModelUsageData } from '../api/models';
import type { TKey } from '../i18n';
import { useUsageClock } from '../hooks/useUsageClock';
import {
  balanceLabelKey,
  compactModelUsage,
  formatQuotaPercent,
  quotaPeriodKeys,
} from '../utils/modelUsage';

export function ModelQuotaSummary({
  usage,
  t,
  inheritColor = false,
}: {
  usage: ModelUsageData;
  t: (key: TKey) => string;
  inheritColor?: boolean;
}) {
  const now = useUsageClock(
    usage.quotas.some((quota) => quota.balance == null && !!quota.period && quota.resetAt > 0)
  );
  const balances = usage.quotas.filter((quota) => quota.balance != null);
  const windows = usage.quotas.filter((quota) => quota.balance == null);
  const balanceSummaries = balances.map((quota) =>
    [t(balanceLabelKey(quota)), quota.balance!.toFixed(2), quota.balanceUnit]
      .filter(Boolean)
      .join(' ')
  );
  const summary = [compactModelUsage(usage, now), ...balanceSummaries].filter(Boolean).join(' · ');
  const label = [
    ...windows.map(
      (quota) =>
        `${quota.period ? `${t(quotaPeriodKeys[quota.period])} ${t('model.quota.remaining')} ` : ''}${formatQuotaPercent(quota)}`
    ),
    ...balanceSummaries,
  ].join(', ');
  return summary ? (
    <span
      aria-label={label}
      className={`min-w-0 max-w-[60%] truncate text-[10px] tabular-nums ${inheritColor ? 'text-inherit' : 'text-cyber-text-secondary'}`}
    >
      {summary}
    </span>
  ) : null;
}
