import { afterEach, beforeEach, expect, it, vi } from 'vitest';

let saved: Map<string, string>;

beforeEach(() => {
  vi.resetModules();
  saved = new Map();
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: (key: string, value: string) => saved.set(key, value),
  });
});
afterEach(() => vi.unstubAllGlobals());

it.each([
  ['iridescent', 'translucent'],
  ['translucent', 'translucent'],
  ['light', 'light'],
  ['unknown', 'dark'],
])('restores the saved %s theme as %s', async (theme, expected) => {
  saved.set('echobird_usage_island', JSON.stringify({ theme }));
  const { useUsageIslandStore } = await import('./usageIslandStore');
  expect(useUsageIslandStore.getState().theme).toBe(expected);
});
