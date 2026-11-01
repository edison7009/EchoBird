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
}: {
  usage: ModelUsageData;
  t: (key: TKey) => string;
}) {
  const now = useUsageClock(
    usage.quotas.some((quota) => quota.balance == null && !!quota.period && quota.resetAt > 0)
  );
  const balance = usage.quotas.find((quota) => quota.balance != null);
  const summary = balance
    ? `${t(balanceLabelKey(balance))}${balance.balance!.toFixed(2)}`
    : compactModelUsage(usage, now);
  const label = balance
    ? summary
    : usage.quotas
        .map(
          (quota) =>
            `${quota.period ? `${t(quotaPeriodKeys[quota.period])} ${t('model.quota.remaining')} ` : ''}${formatQuotaPercent(quota)}`
        )
        .join(', ');
  return summary ? (
    <span
      aria-label={label}
      className="text-[10px] text-cyber-text-secondary shrink-0 whitespace-nowrap tabular-nums"
    >
      {summary}
    </span>
  ) : null;
}
