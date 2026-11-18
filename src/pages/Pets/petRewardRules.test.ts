import { afterEach, expect, it, vi } from 'vitest';
import {
  COMBO_GROWTH_REWARD,
  GROWTH_REWARD,
  REWARD_KINDS,
  SUPER_GROWTH_REWARD,
  drawReward,
  initialRewards,
  readRewards,
} from './petRewardRules';
afterEach(() => vi.restoreAllMocks());
it('gives each of the five reward kinds an equal fifth of the draw interval', () => {
  const random = vi.spyOn(Math, 'random');
  const counts = Object.fromEntries(REWARD_KINDS.map((kind) => [kind, 0]));
  for (let i = 0; i < 100; i++) {
    random.mockReturnValue((i + 0.5) / 100);
    counts[drawReward(0, 1000).kind]++;
  }
  expect(counts).toEqual({ growth: 20, double: 20, haste: 20, combo: 20, super: 20 });
});
it.each([
  [0, 'growth'],
  [0.2, 'double'],
  [0.4, 'haste'],
  [0.6, 'combo'],
  [0.8, 'super'],
  [0.999999, 'super'],
] as const)('draw boundary %s produces %s', (roll, kind) => {
  vi.spyOn(Math, 'random').mockReturnValue(roll);
  expect(drawReward(0, 1000)).toMatchObject({ kind, opened: false, revealed: false });
});
it.each([
  [0.1, GROWTH_REWARD],
  [0.7, COMBO_GROWTH_REWARD],
  [0.9, SUPER_GROWTH_REWARD],
] as const)(
  'freezes the growth payout when drawn at %s, including an active boost',
  (roll, amount) => {
    vi.spyOn(Math, 'random').mockReturnValue(roll);
    expect(drawReward(0, 1000).amount).toBe(amount);
    expect(drawReward(2000, 1000).amount).toBe(amount * 2);
    expect(drawReward(1000, 1000).amount).toBe(amount);
  }
);
it.each(['growth', 'double', 'haste', 'combo', 'super'] as const)(
  'restores valid %s cards and rejects invented payouts',
  (kind) => {
    const amount = {
      growth: GROWTH_REWARD,
      double: 0,
      haste: 0,
      combo: COMBO_GROWTH_REWARD,
      super: SUPER_GROWTH_REWARD,
    }[kind];
    const value = { ...initialRewards(), pending: { kind, amount, opened: true, revealed: false } };
    expect(readRewards(value)).toEqual(value);
    expect(readRewards({ ...value, pending: { ...value.pending, amount: amount + 1 } })).toEqual(
      initialRewards()
    );
  }
);
it('preserves an older unopened growth card without changing its kind or payout', () => {
  const old = {
    ...initialRewards(),
    pending: { kind: 'growth' as const, amount: GROWTH_REWARD * 2, opened: false, revealed: false },
  };
  expect(readRewards(old)).toEqual(old);
});
