import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useNavigationStore as nav } from './navigationStore';
import { usePetsStore as pets } from './petsStore';
import {
  DOUBLE_DURATION,
  GROWTH_REWARD,
  HASTE_DURATION,
  REWARD_INTERVAL,
  rewardDay,
} from '../pages/Pets/petRewardRules';

let storage: Map<string, string>;
const setItem = vi.fn((key: string, value: string) => storage.set(key, value));
beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 29, 12, 0, 0));
  storage = new Map();
  setItem.mockClear();
  vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key) ?? null, setItem });
  pets.setState(pets.getInitialState(), true);
  nav.setState(
    {
      ...nav.getInitialState(),
      developerMode: true,
      activePage: 'developer',
      developerTab: 'pets',
    },
    true
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
const roll = (value: number) => vi.spyOn(Math, 'random').mockReturnValue(value);
const restore = () => {
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
};
const claim = () => {
  vi.setSystemTime(pets.getState().rewards.nextClaimAt);
  expect(pets.getState().claimReward()).toBe(true);
};
const applyCard = () => {
  pets.getState().revealReward();
  return pets.getState().useReward();
};
const tomorrow = () => {
  const date = new Date();
  date.setDate(date.getDate() + 1);
  vi.setSystemTime(date);
};

it('waits five minutes initially and starts the next clock only after the saved card is used', () => {
  const random = roll(0.1);
  pets.getState().load();
  random.mockClear();
  const balance = pets.getState().balance;
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
  expect(pets.getState().claimReward()).toBe(false);
  claim();
  const pending = pets.getState().rewards.pending;
  expect(pending).toMatchObject({ kind: 'growth', amount: GROWTH_REWARD, opened: false });
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  expect(pets.getState().useReward()).toBe(false);
  vi.advanceTimersByTime(20 * 60_000);
  restore();
  expect(pets.getState().rewards.pending).toEqual(pending);
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  random.mockReturnValue(0.99);
  pets.getState().claimReward();
  expect(pets.getState().rewards.pending).toEqual(pending);
  expect(random).toHaveBeenCalledOnce();
  expect(pets.getState().balance).toBe(balance);
  pets.getState().revealReward();
  restore();
  expect(pets.getState().rewards.pending?.opened).toBe(true);
  expect(applyCard()).toBe(true);
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + 2 * REWARD_INTERVAL);
  expect(pets.getState().rewards.usedToday).toBe(1);
  expect(applyCard()).toBe(false);
  expect(pets.getState().balance).toBe(balance + GROWTH_REWARD);
  restore();
  expect(pets.getState().balance).toBe(balance + GROWTH_REWARD);
  expect(pets.getState().claimReward()).toBe(false);
});

it('increases waits by five minutes to thirty, caps there and preserves each deadline on reload', () => {
  roll(0.1);
  pets.getState().load();
  for (const minutes of [5, 10, 15, 20, 25, 30, 30, 30]) {
    const due = pets.getState().rewards.nextClaimAt;
    expect(due).toBe(Date.now() + minutes * 60_000);
    restore();
    expect(pets.getState().rewards.nextClaimAt).toBe(due);
    vi.setSystemTime(due - 1);
    expect(pets.getState().claimReward()).toBe(false);
    claim();
    vi.advanceTimersByTime(2 * 60_000);
    expect(pets.getState().rewards.nextClaimAt).toBe(0);
    expect(applyCard()).toBe(true);
  }
  expect(pets.getState().rewards.usedToday).toBe(8);
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + 30 * 60_000);
});

it('keeps one opportunity ready for hours without generating or accumulating cards', () => {
  const random = roll(0.1);
  pets.getState().load();
  random.mockClear();
  vi.advanceTimersByTime(6 * 60 * 60_000);
  restore();
  expect(pets.getState().rewards.pending).toBeNull();
  expect(random).not.toHaveBeenCalled();
  expect(pets.getState().claimReward()).toBe(true);
  pets.getState().claimReward();
  expect(random).toHaveBeenCalledOnce();
  expect(pets.getState().rewards.usedToday).toBe(0);
});

it('saves the first local clock once, then navigation and reload preserve it without new writes or requests', () => {
  const fetch = vi.fn();
  vi.stubGlobal('fetch', fetch);
  pets.getState().load();
  expect(setItem).toHaveBeenCalledOnce();
  const due = pets.getState().rewards.nextClaimAt;
  setItem.mockClear();
  vi.advanceTimersByTime(60_000);
  pets.getState().load();
  restore();
  expect(pets.getState().rewards.nextClaimAt).toBe(due);
  expect(setItem).not.toHaveBeenCalled();
  expect(fetch).not.toHaveBeenCalled();
});

it('resets to five minutes on the next local day without changing pet levels or balance', () => {
  roll(0.1);
  pets.getState().load();
  for (let i = 0; i < 4; i++) {
    claim();
    applyCard();
  }
  const { balance, items } = pets.getState();
  tomorrow();
  restore();
  expect(pets.getState().rewards.usedToday).toBe(0);
  expect(pets.getState().rewards.day).toBe(rewardDay(Date.now()));
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
  expect(pets.getState().balance).toBe(balance);
  expect(pets.getState().items).toEqual(items);
  expect(pets.getState().claimReward()).toBe(false);
});

it('preserves an unused card across days, pets and reloads, then starts the new day at five minutes', () => {
  roll(0.1);
  pets.getState().load();
  pets.setState({ balance: 200_000_000 });
  pets.getState().acquire();
  expect(pets.getState().selected).toBe(2);
  claim();
  const pending = pets.getState().rewards.pending;
  pets.getState().select(1);
  expect(pets.getState().selected).toBe(1);
  pets.getState().acquire();
  expect(pets.getState().selected).toBe(3);
  tomorrow();
  restore();
  expect(pets.getState().rewards.pending).toEqual(pending);
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  expect(pets.getState().rewards.usedToday).toBe(0);
  applyCard();
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
  expect(pets.getState().rewards.usedToday).toBe(0);
});

it('checks the local day again before a claim, even when the prior day was already ready', () => {
  pets.getState().load();
  vi.advanceTimersByTime(REWARD_INTERVAL);
  tomorrow();
  expect(pets.getState().claimReward()).toBe(false);
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
  expect(pets.getState().rewards.pending).toBeNull();
});

it('freezes doubled card amounts at draw time and still waits until use before restarting the clock', () => {
  const random = roll(0.7);
  pets.getState().load();
  claim();
  applyCard();
  expect(pets.getState().rewards.doubleUntil).toBe(Date.now() + DOUBLE_DURATION);
  random.mockReturnValue(0.2);
  claim();
  expect(pets.getState().rewards.pending?.amount).toBe(GROWTH_REWARD * 2);
  vi.advanceTimersByTime(DOUBLE_DURATION);
  const balance = pets.getState().balance;
  applyCard();
  expect(pets.getState().balance).toBe(balance + GROWTH_REWARD * 2);
  expect(pets.getState().claimReward()).toBe(false);
  claim();
  expect(pets.getState().rewards.pending?.amount).toBe(GROWTH_REWARD);
});

it('extends double buffs and applies haste once to the next progressive wait', () => {
  const random = roll(0.7);
  pets.getState().load();
  claim();
  applyCard();
  const expiry = pets.getState().rewards.doubleUntil;
  claim();
  applyCard();
  expect(pets.getState().rewards.doubleUntil).toBe(expiry + DOUBLE_DURATION);
  random.mockReturnValue(0.95);
  claim();
  applyCard();
  const due = Date.now() + 20 * 60_000 - HASTE_DURATION;
  expect(pets.getState().rewards.nextClaimAt).toBe(due);
  expect(applyCard()).toBe(false);
  expect(pets.getState().rewards.nextClaimAt).toBe(due);
});

it('does not claim, reveal or use while upgrading or outside the active pet page', () => {
  pets.getState().load();
  vi.advanceTimersByTime(REWARD_INTERVAL);
  pets.setState({ busy: true });
  expect(pets.getState().claimReward()).toBe(false);
  pets.setState({ busy: false });
  pets.getState().claimReward();
  setItem.mockClear();
  nav.setState({ developerTab: 'statistics' });
  expect(pets.getState().claimReward()).toBe(false);
  expect(pets.getState().revealReward()).toBe(false);
  expect(pets.getState().useReward()).toBe(false);
  pets.getState().refreshRewards();
  expect(setItem).not.toHaveBeenCalled();
});

it('keeps valid pets and balances when reward metadata is malformed', () => {
  storage.set(
    'echobird-pet-preview-v2',
    JSON.stringify({
      items: [{ id: 1, kind: 'mecha', level: 13, color: 0 }],
      selected: 1,
      balance: 123,
      rewards: { nextClaimAt: -1, doubleUntil: 0, pending: { kind: 'unknown' } },
    })
  );
  restore();
  expect(pets.getState().balance).toBe(123);
  expect(pets.getState().items[0].level).toBe(13);
  expect(pets.getState().rewards.pending).toBeNull();
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
});

it('does not overwrite an unreadable pet save when initializing or rolling over its local clock', () => {
  storage.set('echobird-pet-preview-v2', '{broken');
  pets.getState().load();
  tomorrow();
  pets.getState().refreshRewards();
  expect(setItem).not.toHaveBeenCalled();
  expect(storage.get('echobird-pet-preview-v2')).toBe('{broken');
});

it('does not re-award already-applied legacy cards and preserves an active buff', () => {
  const doubleUntil = Date.now() + DOUBLE_DURATION;
  storage.set(
    'echobird-pet-preview-v2',
    JSON.stringify({
      items: [{ id: 1, kind: 'mecha', level: 13, color: 0 }],
      selected: 1,
      balance: 123,
      rewards: {
        nextClaimAt: Date.now() + REWARD_INTERVAL,
        doubleUntil,
        pending: { kind: 'growth', amount: GROWTH_REWARD, revealed: true },
      },
    })
  );
  restore();
  expect(pets.getState().balance).toBe(123);
  expect(pets.getState().rewards.pending).toBeNull();
  expect(pets.getState().rewards.doubleUntil).toBe(doubleUntil);
  expect(applyCard()).toBe(false);
});

it('migrates an old unused card without discarding it or inheriting its already-running timer', () => {
  const pending = { kind: 'growth', amount: GROWTH_REWARD, revealed: false, opened: true };
  storage.set(
    'echobird-pet-preview-v2',
    JSON.stringify({
      items: [{ id: 1, kind: 'mecha', level: 13, color: 0 }],
      selected: 1,
      balance: 123,
      rewards: { nextClaimAt: Date.now() - REWARD_INTERVAL, doubleUntil: 0, pending },
    })
  );
  restore();
  expect(pets.getState().rewards.pending).toEqual(pending);
  expect(pets.getState().rewards.nextClaimAt).toBe(0);
  expect(applyCard()).toBe(true);
  expect(pets.getState().rewards.nextClaimAt).toBe(Date.now() + REWARD_INTERVAL);
  expect(pets.getState().balance).toBe(123 + GROWTH_REWARD);
});
