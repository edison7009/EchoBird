import { create } from 'zustand';
import { PET_KINDS, PET_TINTS, petStage, type PreviewPet } from '../pages/Pets/petConstants';
import {
  EGG_COST,
  MAX_PET_LEVEL,
  TEST_GROWTH_BALANCE,
  petUpgradeCost,
} from '../pages/Pets/petProgression';
import { useNavigationStore } from './navigationStore';
import type { RouterTokenCredits } from '../api/routerStatistics';
import {
  DOUBLE_DURATION,
  HASTE_DURATION,
  REWARD_INTERVAL,
  rewardDay,
  drawReward,
  initialRewards,
  readRewards,
  type PetRewards,
} from '../pages/Pets/petRewardRules';

const STORAGE_KEY = 'echobird-pet-preview-v2';
const active = () => {
  const nav = useNavigationStore.getState();
  return nav.developerMode && nav.activePage === 'apps' && nav.appsTab === 'pets';
};
const initialPets = (): PreviewPet[] => [{ id: 1, kind: 'mecha', level: 0, color: 0 }];
function validCredits(value: RouterTokenCredits | null | undefined): value is RouterTokenCredits {
  return (
    !!value &&
    typeof value.ledgerId === 'string' &&
    value.ledgerId.length > 0 &&
    value.ledgerId.length <= 128 &&
    Number.isSafeInteger(value.total) &&
    value.total >= 0
  );
}

function validPet(item: PreviewPet): boolean {
  return (
    !!item &&
    Number.isSafeInteger(item.id) &&
    item.id > 0 &&
    PET_KINDS.includes(item.kind) &&
    Number.isInteger(item.level) &&
    item.level >= 0 &&
    item.level <= MAX_PET_LEVEL &&
    (item.experience === undefined ||
      (Number.isSafeInteger(item.experience) &&
        item.experience >= 0 &&
        item.experience < (petUpgradeCost(item.level) || 1))) &&
    Number.isInteger(item.color) &&
    item.color >= 0 &&
    item.color < PET_TINTS.length
  );
}

interface PetsState {
  loaded: boolean;
  preserveInvalidStorage: boolean;
  items: PreviewPet[];
  selected: number;
  balance: number;
  routerCredits: RouterTokenCredits | null;
  busy: boolean;
  thumbnails: Record<string, string>;
  rewards: PetRewards;
  load(): void;
  refreshRewards(): void;
  creditRouter(credits: RouterTokenCredits): boolean;
  select(id: number): void;
  acquire(): void;
  startUpgrade(): PreviewPet | null;
  completeUpgrade(id: number, fromLevel: number): boolean;
  claimReward(): boolean;
  revealReward(): boolean;
  useReward(): boolean;
}

export const petThumbnailKey = (pet: PreviewPet) =>
  `${pet.kind}:${petStage(pet.level)}:${pet.color}`;

function save(items: PreviewPet[], selected: number) {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        items,
        selected,
        balance: usePetsStore.getState().balance,
        rewards: usePetsStore.getState().rewards,
        routerCredits: usePetsStore.getState().routerCredits,
      })
    );
    if (usePetsStore.getState().preserveInvalidStorage)
      usePetsStore.setState({ preserveInvalidStorage: false });
  } catch {
    // A preview remains usable if local browser storage is unavailable.
  }
}

export const usePetsStore = create<PetsState>((set, get) => ({
  loaded: false,
  preserveInvalidStorage: false,
  items: initialPets(),
  selected: 1,
  balance: 0,
  routerCredits: null,
  busy: false,
  thumbnails: {},
  rewards: initialRewards(),
  load() {
    if (!active()) return;
    if (get().loaded) {
      get().refreshRewards();
      return;
    }
    try {
      let data = JSON.parse(localStorage.getItem(STORAGE_KEY) || 'null');
      if (!data) {
        const old = JSON.parse(localStorage.getItem('echobird-pet-preview-v1') || 'null');
        const levels = [0, 10, 20, 30, 50];
        if (old && Array.isArray(old.items))
          data = {
            ...old,
            balance: TEST_GROWTH_BALANCE,
            items: old.items.map(
              (pet: { id: number; kind: PreviewPet['kind']; color: number; stage: number }) => ({
                id: pet.id,
                kind: pet.kind,
                color: pet.color,
                level: levels[pet.stage],
              })
            ),
          };
      }
      if (
        data &&
        Number.isSafeInteger(data.balance) &&
        data.balance >= 0 &&
        (data.routerCredits == null || validCredits(data.routerCredits)) &&
        Array.isArray(data.items) &&
        data.items.length &&
        data.items.every(validPet) &&
        new Set(data.items.map((pet: PreviewPet) => pet.id)).size === data.items.length
      ) {
        set({
          items: data.items,
          balance: data.balance,
          routerCredits: validCredits(data.routerCredits) ? data.routerCredits : null,
          rewards: readRewards(data.rewards),
          selected: data.items.some((pet: PreviewPet) => pet.id === data.selected)
            ? data.selected
            : data.items[0].id,
        });
      } else if (data) set({ preserveInvalidStorage: true });
      else
        set({
          items: [
            {
              id: 1,
              kind: PET_KINDS[Math.floor(Math.random() * PET_KINDS.length)],
              color: Math.floor(Math.random() * PET_TINTS.length),
              level: 0,
            },
          ],
          selected: 1,
          balance: 0,
        });
    } catch {
      set({ preserveInvalidStorage: true });
      // Keep invalid saved data intact until an explicit successful save.
    }
    set({ loaded: true });
    get().refreshRewards();
  },
  creditRouter(credits) {
    if (
      !active() ||
      !get().loaded ||
      get().busy ||
      get().preserveInvalidStorage ||
      !validCredits(credits)
    )
      return false;
    const state = get(),
      previous = state.routerCredits;
    if (previous?.ledgerId === credits.ledgerId && credits.total <= previous.total) return false;
    const delta = credits.total - (previous?.ledgerId === credits.ledgerId ? previous.total : 0);
    if (delta === 0) return false;
    const balance = state.balance + delta;
    if (!Number.isSafeInteger(balance)) return false;
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          items: state.items,
          selected: state.selected,
          balance,
          rewards: state.rewards,
          routerCredits: credits,
        })
      );
    } catch {
      return false;
    }
    set({ balance, routerCredits: { ...credits } });
    return true;
  },
  refreshRewards() {
    if (!active() || !get().loaded) return;
    const now = Date.now(),
      day = rewardDay(now),
      { rewards } = get();
    if (rewards.day === day) return;
    set({
      rewards: {
        ...rewards,
        day,
        usedToday: 0,
        nextClaimAt: rewards.pending ? 0 : now + REWARD_INTERVAL,
      },
    });
    if (!get().preserveInvalidStorage) save(get().items, get().selected);
  },
  select(id) {
    if (!active() || !get().items.some((pet) => pet.id === id) || get().selected === id) return;
    set({ selected: id, busy: false });
    save(get().items, id);
  },
  acquire() {
    if (!active() || get().balance < EGG_COST) return;
    const id = Math.max(...get().items.map((pet) => pet.id)) + 1;
    const pet = {
      id,
      kind: PET_KINDS[(id - 1) % PET_KINDS.length],
      color: (id - 1) % PET_TINTS.length,
      level: 0,
    };
    const items = [...get().items, pet];
    set({ items, selected: id, balance: get().balance - EGG_COST, busy: false });
    save(items, id);
  },
  startUpgrade() {
    const state = get(),
      pet = state.items.find((pet) => pet.id === state.selected)!;
    if (!active() || state.busy || pet.level >= MAX_PET_LEVEL || state.balance <= 0) return null;
    set({ busy: true });
    return { ...pet };
  },
  completeUpgrade(id, fromLevel) {
    if (!active() || !get().busy || get().selected !== id) return false;
    const pet = get().items.find((pet) => pet.id === id);
    if (!pet || pet.level !== fromLevel || fromLevel >= MAX_PET_LEVEL) return false;
    const cost = petUpgradeCost(fromLevel);
    const experience = pet.experience ?? 0;
    const amount = Math.min(get().balance, cost - experience);
    if (amount <= 0) {
      set({ busy: false });
      return false;
    }
    const leveledUp = experience + amount >= cost;
    const items = get().items.map((pet) =>
      pet.id === id
        ? {
            ...pet,
            level: fromLevel + (leveledUp ? 1 : 0),
            experience: leveledUp ? 0 : experience + amount,
          }
        : pet
    );
    set({ items, balance: get().balance - amount, busy: false });
    save(items, id);
    return true;
  },
  claimReward() {
    if (!active() || !get().loaded || get().busy) return false;
    get().refreshRewards();
    const { rewards } = get();
    if (rewards.pending) return true;
    const now = Date.now();
    if (now < rewards.nextClaimAt) return false;
    set({
      rewards: {
        ...rewards,
        nextClaimAt: 0,
        pending: drawReward(rewards.doubleUntil, now),
      },
    });
    save(get().items, get().selected);
    return true;
  },
  revealReward() {
    if (!active() || !get().loaded || get().busy) return false;
    get().refreshRewards();
    const { rewards } = get();
    if (!rewards.pending || rewards.pending.revealed || rewards.pending.opened !== false)
      return false;
    set({ rewards: { ...rewards, pending: { ...rewards.pending, opened: true } } });
    save(get().items, get().selected);
    return true;
  },
  useReward() {
    if (!active() || !get().loaded || get().busy) return false;
    get().refreshRewards();
    const { rewards, balance } = get();
    const reward = rewards.pending;
    if (!reward || reward.revealed || reward.opened === false) return false;
    const now = Date.now();
    const usedToday = (rewards.usedToday ?? 0) + (reward.day === rewards.day ? 1 : 0);
    const wait = Math.min(usedToday + 1, 6) * REWARD_INTERVAL;
    set({
      balance: balance + (reward.kind === 'growth' ? reward.amount : 0),
      rewards: {
        ...rewards,
        pending: null,
        usedToday,
        doubleUntil:
          reward.kind === 'double'
            ? Math.max(now, rewards.doubleUntil) + DOUBLE_DURATION
            : rewards.doubleUntil,
        nextClaimAt: now + wait - (reward.kind === 'haste' ? HASTE_DURATION : 0),
      },
    });
    save(get().items, get().selected);
    return true;
  },
}));
