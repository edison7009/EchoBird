import { quotaTextClass } from '../../utils/quotaColors';

/** Remaining quota, in the same colors as model cards and the usage island. */
export function QuotaPercent({ percent }: { percent: number | null | undefined }) {
  return (
    <span className={quotaTextClass(percent)}>
      {percent == null ? '—' : `${Math.round(percent)}%`}
    </span>
  );
}
