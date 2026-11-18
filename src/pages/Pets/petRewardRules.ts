import { GROWTH_TOKEN_UNIT } from './petProgression';

export const REWARD_INTERVAL = 5 * 60_000;
export const DOUBLE_DURATION = 15 * 60_000;
export const HASTE_DURATION = REWARD_INTERVAL / 2;
export const GROWTH_REWARD = GROWTH_TOKEN_UNIT / 2;
export const COMBO_GROWTH_REWARD = GROWTH_REWARD / 2;
export const COMBO_HASTE_DURATION = HASTE_DURATION / 2;
export const SUPER_GROWTH_REWARD = GROWTH_REWARD * 2;
export const REWARD_KINDS = ['growth', 'double', 'haste', 'combo', 'super'] as const;
export type RewardKind = (typeof REWARD_KINDS)[number];
const REWARD_AMOUNTS: Record<RewardKind, number> = {
  growth: GROWTH_REWARD,
  double: 0,
  haste: 0,
  combo: COMBO_GROWTH_REWARD,
  super: SUPER_GROWTH_REWARD,
};
export interface PetReward {
  kind: RewardKind;
  amount: number;
  revealed: boolean;
  opened?: boolean;
  day?: string;
}
export interface PetRewards {
  nextClaimAt: number;
  doubleUntil: number;
  pending: PetReward | null;
  day?: string;
  usedToday?: number;
}
export const initialRewards = (): PetRewards => ({ nextClaimAt: 0, doubleUntil: 0, pending: null });
export function rewardDay(now: number): string {
  const date = new Date(now);
  return `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

export function drawReward(doubleUntil: number, now: number): PetReward {
  // Five equally likely rewards; choosing a back never redraws this saved result.
  const kind = REWARD_KINDS[Math.floor(Math.random() * REWARD_KINDS.length)];
  return {
    kind,
    amount: REWARD_AMOUNTS[kind] * (doubleUntil > now ? 2 : 1),
    revealed: false,
    opened: false,
    day: rewardDay(now),
  };
}

export function readRewards(value: PetRewards | undefined): PetRewards {
  const validTime = (time: number) => Number.isSafeInteger(time) && time >= 0;
  const pending = value?.pending;
  // Older v2 cards marked revealed have already granted their reward.
  return value &&
    validTime(value.nextClaimAt) &&
    validTime(value.doubleUntil) &&
    (value.day === undefined || typeof value.day === 'string') &&
    (value.usedToday === undefined ||
      (Number.isSafeInteger(value.usedToday) && value.usedToday >= 0)) &&
    (pending === null ||
      (pending &&
        REWARD_KINDS.includes(pending.kind) &&
        typeof pending.revealed === 'boolean' &&
        (pending.opened === undefined || typeof pending.opened === 'boolean') &&
        (pending.day === undefined || typeof pending.day === 'string') &&
        (pending.amount === REWARD_AMOUNTS[pending.kind] ||
          pending.amount === REWARD_AMOUNTS[pending.kind] * 2)))
    ? { ...value, pending: pending?.revealed ? null : pending }
    : initialRewards();
}

export function rewardCountdown(until: number, now: number): string {
  const seconds = Math.max(0, Math.ceil((until - now) / 1000));
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}
