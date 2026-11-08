import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { initializeMergedPageFeatures, migrateMergedPages } from './mergedPagesMigration';
import { useUsageIslandStore as island } from './usageIslandStore';
import { useNavigationStore as navigation } from './navigationStore';
beforeEach(() => {
  navigation.setState(navigation.getInitialState(), true);
  const saved = new Map<string, string>();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
  });
  island.setState({ enabled: true, sources: ['model:fixture'] });
});

it('opens merged pages after a failed settings read without persisting a guessed island preference', () => {
  const write = vi.spyOn(localStorage, 'setItem');
  navigation.getState().setAppsTab('pets');
  const stop = initializeMergedPageFeatures(null);
  expect(navigation.getState()).toMatchObject({
    activePage: 'apps',
    appsTab: 'pets',
    developerMode: true,
    developerModeReady: true,
  });
  expect(island.getState().enabled).toBe(false);
  expect(write).not.toHaveBeenCalled();
  island.getState().update({ enabled: true });
  migrateMergedPages({ developerMode: false });
  expect(island.getState().enabled).toBe(true);
  stop?.();
});

it('preserves an explicit island choice made during a failing settings read', () => {
  const before = island.getState();
  island.getState().update({ enabled: false });
  island.getState().update({ enabled: true });
  initializeMergedPageFeatures(null, before);
  migrateMergedPages({ developerMode: false });
  expect(island.getState().enabled).toBe(true);
});
afterEach(() => vi.unstubAllGlobals());
it('keeps an effectively paused island off once without losing sources or overriding a later explicit enable', () => {
  migrateMergedPages({ developerMode: false });
  expect(island.getState()).toMatchObject({ enabled: false, sources: ['model:fixture'] });
  island.getState().update({ enabled: true });
  migrateMergedPages({ developerMode: false });
  expect(island.getState().enabled).toBe(true);
});
it('preserves an already enabled island when the previous feature gate was enabled', () => {
  migrateMergedPages({ developerMode: true });
  expect(island.getState().enabled).toBe(true);
});

it('does not overwrite an explicit island change made while the settings read was pending', () => {
  const before = island.getState();
  island.getState().update({ enabled: false });
  island.getState().update({ enabled: true });
  migrateMergedPages({ developerMode: false }, before);
  expect(island.getState().enabled).toBe(true);
});
it('keeps a previously paused island off for this session when migration storage cannot be read', () => {
  vi.stubGlobal('localStorage', {
    getItem: () => {
      throw new Error('unavailable');
    },
    setItem: vi.fn(),
  });
  migrateMergedPages({ developerMode: false });
  expect(island.getState().enabled).toBe(false);
  expect(localStorage.setItem).not.toHaveBeenCalled();
});
