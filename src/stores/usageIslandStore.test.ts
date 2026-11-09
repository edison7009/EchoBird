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
  ['iridescent', 'dark'],
  ['translucent', 'dark'],
  ['light', 'light'],
  ['unknown', 'dark'],
])('restores the saved %s theme as %s', async (theme, expected) => {
  saved.set('echobird_usage_island', JSON.stringify({ theme }));
  const { useUsageIslandStore } = await import('./usageIslandStore');
  expect(useUsageIslandStore.getState().theme).toBe(expected);
});

it('restores the list style and its independent opening direction', async () => {
  saved.set('echobird_usage_island', JSON.stringify({ style: 'list', reversed: ['list', 'line'] }));
  const { useUsageIslandStore } = await import('./usageIslandStore');
  expect(useUsageIslandStore.getState()).toMatchObject({
    style: 'list',
    reversed: ['list', 'line'],
  });
  useUsageIslandStore.getState().toggleReversed('list');
  expect(JSON.parse(saved.get('echobird_usage_island')!)).toMatchObject({
    style: 'list',
    reversed: ['line'],
  });
});

it('restores and persists opacity and Smart Access visibility', async () => {
  saved.set(
    'echobird_usage_island',
    JSON.stringify({ opacity: 60, showSmartAccess: true, theme: 'translucent' })
  );
  const { useUsageIslandStore } = await import('./usageIslandStore');
  expect(useUsageIslandStore.getState()).toMatchObject({
    opacity: 60,
    showSmartAccess: true,
    theme: 'dark',
  });
  useUsageIslandStore.getState().update({ opacity: 80, showSmartAccess: false });
  expect(JSON.parse(saved.get('echobird_usage_island')!)).toMatchObject({
    opacity: 80,
    showSmartAccess: false,
  });
});
