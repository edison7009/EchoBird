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
  const summary = balances.length
    ? balances
        .map(
          (quota) =>
            `${t(balanceLabelKey(quota))}${quota.balance!.toFixed(2)}${
              balances.length > 1 && quota.balanceUnit ? ` ${quota.balanceUnit}` : ''
            }`
        )
        .join(' · ')
    : compactModelUsage(usage, now);
  const label = balances.length
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
      className={`text-[10px] shrink-0 whitespace-nowrap tabular-nums ${inheritColor ? 'text-inherit' : 'text-cyber-text-secondary'}`}
    >
      {summary}
    </span>
  ) : null;
}
