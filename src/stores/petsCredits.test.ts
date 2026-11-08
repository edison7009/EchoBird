import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { usePetsStore as pets } from './petsStore';
import { useNavigationStore as nav } from './navigationStore';

let storage: Map<string, string>;
const setItem = vi.fn((key: string, value: string) => storage.set(key, value));
beforeEach(() => {
  storage = new Map();
  setItem.mockClear();
  vi.stubGlobal('localStorage', { getItem: (key: string) => storage.get(key) ?? null, setItem });
  pets.setState(pets.getInitialState(), true);
  nav.setState(
    {
      ...nav.getInitialState(),
      developerMode: true,
      activePage: 'apps',
      appsTab: 'pets',
      developerTab: 'pets',
    },
    true
  );
  pets.getState().load();
});
afterEach(() => vi.unstubAllGlobals());

it('credits cumulative reported usage once across duplicate, out-of-order, reload and a new ledger', () => {
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 100 })).toBe(true);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 100 })).toBe(false);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 90 })).toBe(false);
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().balance).toBe(100);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 140 })).toBe(true);
  expect(pets.getState().balance).toBe(140);
  expect(pets.getState().creditRouter({ ledgerId: 'b', total: 7 })).toBe(true);
  expect(pets.getState().balance).toBe(147);
});

it('does not advance a credit checkpoint if saving fails, then retries exactly once', () => {
  setItem.mockImplementationOnce(() => {
    throw new Error('fixture disk failure');
  });
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 50 })).toBe(false);
  expect(pets.getState().balance).toBe(0);
  expect(pets.getState().routerCredits).toBeNull();
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 50 })).toBe(true);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 50 })).toBe(false);
  expect(pets.getState().balance).toBe(50);
});

it('defers a new income during an upgrade so partial feeding never consumes the arriving balance', () => {
  pets.setState({ balance: 40_000_000 });
  pets.getState().startUpgrade();
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 100_000_000 })).toBe(false);
  pets.getState().completeUpgrade(1, 0);
  expect(pets.getState().items[0]).toMatchObject({ level: 0, experience: 40_000_000 });
  expect(pets.getState().balance).toBe(0);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 100_000_000 })).toBe(true);
  expect(pets.getState().balance).toBe(100_000_000);
});

it('preserves existing preview balances and rejects invalid or background credit applications', () => {
  pets.setState({ balance: 100_000_000_000 });
  pets.getState().creditRouter({ ledgerId: 'a', total: 10 });
  expect(pets.getState().balance).toBe(100_000_000_010);
  setItem.mockClear();
  for (const total of [-1, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1])
    expect(pets.getState().creditRouter({ ledgerId: 'a', total })).toBe(false);
  nav.getState().setAppsTab('desktop');
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 20 })).toBe(false);
  expect(setItem).not.toHaveBeenCalled();
});

it('preserves a damaged credit checkpoint without treating its lifetime usage as new income', () => {
  const saved = JSON.parse(storage.get('echobird-pet-preview-v2')!);
  saved.routerCredits = { ledgerId: 'a', total: -1 };
  const raw = JSON.stringify(saved);
  storage.set('echobird-pet-preview-v2', raw);
  setItem.mockClear();
  pets.setState(pets.getInitialState(), true);
  pets.getState().load();
  expect(pets.getState().preserveInvalidStorage).toBe(true);
  expect(pets.getState().creditRouter({ ledgerId: 'a', total: 100 })).toBe(false);
  expect(storage.get('echobird-pet-preview-v2')).toBe(raw);
  expect(setItem).not.toHaveBeenCalled();
});
