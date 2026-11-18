import { afterEach, expect, it, vi } from 'vitest';
// Use a public-asset module import fixture so simultaneous callers exercise the
// real initialization promise without fetching WASM in the unit-test runtime.
const fixture = vi.hoisted(() => ({ init: vi.fn(), loads: 0 }));
vi.mock('http://localhost/pet-renderer/renderer.js', () => ({ default: fixture.init }));
afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
  fixture.init.mockReset();
});
it('shares concurrent initialization between the pet and card entry points', async () => {
  vi.stubGlobal('document', { baseURI: 'http://localhost/' });
  let finish!: () => void;
  fixture.init.mockReturnValue(
    new Promise<void>((resolve) => {
      finish = resolve;
    })
  );
  const { loadPetRenderer } = await import('./petRenderer.js');
  const pet = loadPetRenderer();
  const supply = loadPetRenderer();
  expect(pet).toBe(supply);
  finish();
  expect(await pet).toBe(await supply);
  expect(fixture.init).toHaveBeenCalledOnce();
});
it('permits a new load after initialization fails', async () => {
  vi.stubGlobal('document', { baseURI: 'http://localhost/' });
  fixture.init.mockRejectedValueOnce(new Error('unavailable')).mockResolvedValueOnce({});
  const { loadPetRenderer } = await import('./petRenderer.js');
  await expect(loadPetRenderer()).rejects.toThrow('unavailable');
  await expect(loadPetRenderer()).resolves.toBeDefined();
  expect(fixture.init).toHaveBeenCalledTimes(2);
});
