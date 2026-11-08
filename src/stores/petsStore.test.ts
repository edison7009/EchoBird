import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { useNavigationStore as nav } from './navigationStore';
import { usePetsStore as pets } from './petsStore';
import { GROWTH_TOKEN_UNIT as YI, TEST_GROWTH_BALANCE } from '../pages/Pets/petProgression';

let storage: Map<string, string>;
const getItem = vi.fn((key: string) => storage.get(key) ?? null);
const setItem = vi.fn((key: string, value: string) => storage.set(key, value));
beforeEach(() => {
  storage = new Map();
  storage.set(
    'echobird-pet-preview-v2',
    JSON.stringify({
      items: [
        { id: 1, kind: 'mecha', level: 0, color: 0 },
        { id: 2, kind: 'unicorn', level: 0, color: 1 },
        { id: 3, kind: 'kaiju', level: 10, color: 2 },
      ],
      selected: 1,
      balance: TEST_GROWTH_BALANCE,
    })
  );
  getItem.mockClear();
  setItem.mockClear();
  vi.stubGlobal('localStorage', { getItem, setItem });
  pets.setState(pets.getInitialState(), true);
  nav.setState(nav.getInitialState(), true);
});
afterEach(() => vi.unstubAllGlobals());
const enter = () =>
  nav.setState({ developerMode: true, activePage: 'apps', appsTab: 'pets', developerTab: 'pets' });

it('only loads on an active visit and persists the first daily clock without enabling statistics', () => {
  const fetch = vi.fn();
  vi.stubGlobal('fetch', fetch);
  pets.getState().load();
  pets.getState().acquire();
  pets.getState().completeUpgrade(1, 2);
  expect(getItem).not.toHaveBeenCalled();
  expect(setItem).not.toHaveBeenCalled();
  enter();
  pets.getState().load();
  pets.getState().load();
  expect(getItem).toHaveBeenCalledTimes(1);
  expect(setItem).toHaveBeenCalledOnce();
  expect(fetch).not.toHaveBeenCalled();
});

it('keeps one selection and independently saves many eggs, their colors and completed stages', () => {
  enter();
  pets.getState().load();
  for (let i = 0; i < 8; i++) pets.getState().acquire();
  expect(pets.getState().items).toHaveLength(11);
  expect(pets.getState().selected).toBe(11);
  expect(new Set(pets.getState().items.map((pet) => pet.id)).size).toBe(11);
  pets.getState().startUpgrade();
  pets.getState().completeUpgrade(11, 0);
  pets.getState().select(1);
  pets.getState().startUpgrade();
  pets.getState().completeUpgrade(1, 0);
  pets.getState().select(11);
  const saved = { items: pets.getState().items, selected: pets.getState().selected };
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect({ items: pets.getState().items, selected: pets.getState().selected }).toEqual(saved);
  expect(pets.getState().items.find((pet) => pet.id === 11)).toMatchObject({ level: 1, color: 4 });
  expect(pets.getState().items[0].level).toBe(1);
  expect(pets.getState().balance).toBe(990 * YI);
});

it('rejects cancelled, stale, wrong-pet and paused upgrade callbacks', () => {
  enter();
  pets.getState().load();
  pets.getState().startUpgrade();
  pets.getState().select(2);
  expect(pets.getState().completeUpgrade(1, 0)).toBe(false);
  expect(pets.getState().items[0].level).toBe(0);
  pets.getState().startUpgrade();
  expect(pets.getState().completeUpgrade(2, 0)).toBe(true);
  expect(pets.getState().completeUpgrade(2, 0)).toBe(false);
  expect(pets.getState().items[1].level).toBe(1);
  nav.getState().setDeveloperMode(false);
  setItem.mockClear();
  pets.getState().completeUpgrade(2, 1);
  pets.getState().acquire();
  pets.getState().select(1);
  expect(pets.getState().items[1].level).toBe(1);
  expect(setItem).not.toHaveBeenCalled();
});

it('uses safe initial specimens for corrupted local data without overwriting it on navigation', () => {
  storage.set('echobird-pet-preview-v2', '{broken');
  enter();
  pets.getState().load();
  expect(pets.getState().items).toHaveLength(1);
  expect(setItem).not.toHaveBeenCalled();
});

it('charges once after each successful level and stops at 60 without resetting the wallet on reload', () => {
  enter();
  pets.getState().load();
  for (let level = 0; level < 60; level++) {
    const balance = pets.getState().balance;
    expect(pets.getState().startUpgrade()?.level).toBe(level);
    expect(pets.getState().startUpgrade()).toBeNull();
    expect(pets.getState().balance).toBe(balance);
    expect(pets.getState().completeUpgrade(1, level)).toBe(true);
    expect(pets.getState().completeUpgrade(1, level)).toBe(false);
  }
  expect(pets.getState().items[0].level).toBe(60);
  expect(pets.getState().balance).toBe(865 * YI);
  expect(pets.getState().startUpgrade()).toBeNull();
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().balance).toBe(865 * YI);
  expect(pets.getState().items[0].level).toBe(60);
});

it('refuses spending when funds are insufficient and preserves valid zero balance', () => {
  enter();
  pets.getState().load();
  setItem.mockClear();
  pets.setState({ balance: 0 });
  expect(pets.getState().startUpgrade()).toBeNull();
  pets.getState().acquire();
  expect(pets.getState().items).toHaveLength(3);
  expect(setItem).not.toHaveBeenCalled();
  pets.getState().select(2);
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().balance).toBe(0);
});

it('spends a small balance on only the selected pet and retains its partial experience on reload', () => {
  enter();
  pets.getState().load();
  pets.setState({ balance: 0.4 * YI });
  expect(pets.getState().startUpgrade()).toMatchObject({ id: 1, level: 0 });
  expect(pets.getState().completeUpgrade(1, 0)).toBe(true);
  expect(pets.getState().items[0]).toMatchObject({ level: 0, experience: 0.4 * YI });
  expect(pets.getState().balance).toBe(0);
  expect(pets.getState().startUpgrade()).toBeNull();
  pets.getState().select(2);
  expect(pets.getState().items[1].experience ?? 0).toBe(0);
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().selected).toBe(2);
  expect(pets.getState().items[0]).toMatchObject({ level: 0, experience: 0.4 * YI });
  expect(pets.getState().balance).toBe(0);
});

it('stops immediately after one level, spending only its missing experience from the shared balance', () => {
  enter();
  pets.setState({
    items: [{ id: 1, kind: 'mecha', level: 10, color: 0, experience: 0.4 * YI }],
    balance: 10 * YI,
  });
  pets.getState().startUpgrade();
  expect(pets.getState().completeUpgrade(1, 10)).toBe(true);
  expect(pets.getState().items[0]).toMatchObject({ level: 11, experience: 0 });
  expect(pets.getState().balance).toBe(8.9 * YI);
  expect(pets.getState().busy).toBe(false);
  expect(pets.getState().completeUpgrade(1, 10)).toBe(false);
  expect(pets.getState().items[0].level).toBe(11);
  expect(pets.getState().balance).toBe(8.9 * YI);
});

it('levels up with exactly the remaining experience and cannot charge a cancelled partial contribution', () => {
  enter();
  pets.getState().load();
  pets.setState({ balance: 0.3 * YI });
  pets.getState().startUpgrade();
  pets.getState().select(2);
  expect(pets.getState().completeUpgrade(1, 0)).toBe(false);
  expect(pets.getState().balance).toBe(0.3 * YI);
  pets.getState().select(1);
  pets.setState({
    items: [{ id: 1, kind: 'mecha', level: 59, color: 0, experience: 3.2 * YI }],
  });
  pets.getState().startUpgrade();
  expect(pets.getState().completeUpgrade(1, 59)).toBe(true);
  expect(pets.getState().items[0]).toMatchObject({ level: 60, experience: 0 });
  expect(pets.getState().balance).toBe(0);
  expect(pets.getState().startUpgrade()).toBeNull();
});

it('migrates old forms and persists their daily clock without deleting the old specimens', () => {
  storage.delete('echobird-pet-preview-v2');
  storage.set(
    'echobird-pet-preview-v1',
    JSON.stringify({
      selected: 2,
      items: [
        { id: 1, kind: 'mecha', stage: 2, color: 0 },
        { id: 2, kind: 'fox', stage: 4, color: 1 },
      ],
    })
  );
  enter();
  pets.getState().load();
  expect(pets.getState().items.map((pet) => pet.level)).toEqual([20, 50]);
  expect(pets.getState().balance).toBe(TEST_GROWTH_BALANCE);
  expect(pets.getState().selected).toBe(2);
  expect(setItem).toHaveBeenCalledOnce();
  expect(storage.has('echobird-pet-preview-v1')).toBe(true);
});

it('gives a new local user exactly one random level-zero egg with no test balance and never gifts twice', () => {
  storage.clear();
  const random = vi.spyOn(Math, 'random').mockReturnValueOnce(0.99).mockReturnValueOnce(0.34);
  enter();
  pets.getState().load();
  expect(pets.getState().items).toEqual([{ id: 1, kind: 'kangaroo', color: 2, level: 0 }]);
  expect(pets.getState().balance).toBe(0);
  expect(random).toHaveBeenCalledTimes(2);
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().items).toEqual([{ id: 1, kind: 'kangaroo', color: 2, level: 0 }]);
  expect(pets.getState().balance).toBe(0);
  expect(random).toHaveBeenCalledTimes(2);
  random.mockRestore();
});
