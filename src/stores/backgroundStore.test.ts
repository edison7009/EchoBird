import { afterEach, beforeEach, expect, it, vi } from 'vitest';

let saved: Map<string, string>;
let write: ReturnType<typeof vi.fn>;

beforeEach(() => {
  vi.resetModules();
  saved = new Map();
  write = vi.fn((key: string, value: string) => saved.set(key, value));
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => saved.get(key) ?? null,
    setItem: write,
  });
});
afterEach(() => vi.unstubAllGlobals());

it.each([undefined, 'invalid', 'false'])(
  'keeps character flow as the default for %s',
  async (value) => {
    if (value) saved.set('echobird_background', value);
    saved.set('echobird_easter_egg', 'false');
    const { useBackgroundStore } = await import('./backgroundStore');
    expect(useBackgroundStore.getState().mode).toBe('characters');
    expect(write).not.toHaveBeenCalled();
  }
);

it.each(['none', 'characters', 'carbon', 'basalt', 'monogram'] as const)(
  'restores %s after restart independently of the Easter Egg preference',
  async (mode) => {
    saved.set('echobird_easter_egg', 'false');
    const { useBackgroundStore } = await import('./backgroundStore');
    useBackgroundStore.getState().setMode(mode);
    expect(write).toHaveBeenCalledExactlyOnceWith('echobird_background', mode);
    expect(saved.get('echobird_easter_egg')).toBe('false');
    write.mockClear();
    vi.resetModules();
    const restarted = (await import('./backgroundStore')).useBackgroundStore;
    expect(restarted.getState().mode).toBe(mode);
    expect(write).not.toHaveBeenCalled();
  }
);

it('keeps explicit choices usable if storage cannot be read or written', async () => {
  vi.stubGlobal('localStorage', {
    getItem: () => {
      throw new Error('storage unavailable');
    },
    setItem: () => {
      throw new Error('storage unavailable');
    },
  });
  const { useBackgroundStore } = await import('./backgroundStore');
  expect(useBackgroundStore.getState().mode).toBe('characters');
  expect(() => useBackgroundStore.getState().setMode('none')).not.toThrow();
  expect(useBackgroundStore.getState().mode).toBe('none');
});
