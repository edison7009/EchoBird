import { useUsageClock } from '../../hooks/useUsageClock';
import type { TraeAccount } from '../../api/tauri';

export const traeClaimedToday = (at?: number | null, now = Date.now()) =>
  at != null && Math.floor((at + 8 * 3600) / 86400) === Math.floor((now / 1000 + 8 * 3600) / 86400);

export const traeClaimUnavailable = (account: TraeAccount, now = Date.now()) =>
  account.edition !== 'traecn' ||
  account.rewardEnabled === false ||
  traeClaimedToday(account.dailyClaimedAt, now) ||
  traeClaimedToday(account.deviceClaimedAt, now);

export function useTraeClaimedToday(enabled: boolean) {
  const now = useUsageClock(enabled);
  return (at?: number | null) => traeClaimedToday(at, now);
}
