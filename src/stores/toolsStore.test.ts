import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import * as api from '../api/tauri';
import type { LocalTool } from '../api/types';

vi.mock('../api/tauri', () => ({ scanTools: vi.fn() }));

class PendingImage {
  static instances: PendingImage[] = [];
  src = '';
  resolve!: () => void;
  reject!: () => void;
  decode = vi.fn(
    () =>
      new Promise<void>((resolve, reject) => {
        this.resolve = resolve;
        this.reject = () => reject(new Error('Broken image'));
      })
  );
  constructor() {
    PendingImage.instances.push(this);
  }
}

const tool = (id: string, installed = true): LocalTool => ({
  id,
  name: id,
  category: 'Desktop',
  installed,
});
const cacheKey = 'echobird-tools-cache';
const storage = new Map<string, string>();
const setItem = vi.fn((key: string, value: string) => storage.set(key, value));
let store: typeof import('./toolsStore').useToolsStore;

beforeEach(async () => {
  vi.resetModules();
  vi.clearAllMocks();
  storage.clear();
  setItem.mockImplementation((key, value) => storage.set(key, value));
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem,
  });
  PendingImage.instances = [];
  vi.stubGlobal('Image', PendingImage);
  store = (await import('./toolsStore')).useToolsStore;
});
afterEach(() => vi.unstubAllGlobals());

async function image(src: string) {
  await vi.waitFor(() => expect(PendingImage.instances.some((img) => img.src === src)).toBe(true));
  return PendingImage.instances.find((img) => img.src === src)!;
}

it('publishes both tabs together only after the last icon has decoded', async () => {
  vi.mocked(api.scanTools).mockResolvedValue([tool('one'), tool('two', false)]);
  const scan = store.getState().scanTools();
  (await image('./icons/tools/one.svg')).resolve();
  await Promise.resolve();
  expect(store.getState().detectedTools).toEqual([]);
  expect(store.getState().isScanning).toBe(true);

  (await image('./icons/tools/two.svg')).resolve();
  await scan;
  expect(store.getState().detectedTools.map((t) => t.icon)).toEqual([
    './icons/tools/one.svg',
    './icons/tools/two.svg',
  ]);
  expect(store.getState().isScanning).toBe(false);
});

it('resolves PNG, native and generic fallbacks before publishing, with dsh preferring PNG', async () => {
  const native = 'data:image/png;base64,native';
  vi.mocked(api.scanTools).mockResolvedValue([
    tool('png'),
    { ...tool('native'), iconBase64: native },
    { ...tool('broken'), iconBase64: 'data:image/png;base64,broken' },
    tool('dsh'),
  ]);
  const scan = store.getState().scanTools();
  for (const id of ['png', 'native', 'broken']) {
    (await image(`./icons/tools/${id}.svg`)).reject();
  }
  (await image('./icons/tools/png.png')).resolve();
  (await image('./icons/tools/native.png')).reject();
  (await image('./icons/tools/broken.png')).reject();
  (await image(native)).resolve();
  (await image('data:image/png;base64,broken')).reject();
  expect(store.getState().detectedTools).toEqual([]);
  (await image('./icons/tools/dsh.png')).resolve();
  await scan;
  expect(store.getState().detectedTools.map((t) => t.icon)).toEqual([
    './icons/tools/png.png',
    native,
    '',
    './icons/tools/dsh.png',
  ]);
  expect(PendingImage.instances.some((img) => img.src.endsWith('dsh.svg'))).toBe(false);
});

it('keeps the existing desktop during refresh and reuses decoded icons', async () => {
  vi.mocked(api.scanTools).mockResolvedValue([tool('one')]);
  const first = store.getState().scanTools();
  (await image('./icons/tools/one.svg')).resolve();
  await first;
  const previous = store.getState().detectedTools;

  vi.mocked(api.scanTools).mockResolvedValue([tool('one'), tool('new')]);
  const refresh = store.getState().scanTools();
  await image('./icons/tools/new.svg');
  expect(store.getState().detectedTools).toBe(previous);
  expect(PendingImage.instances.filter((img) => img.src.endsWith('one.svg'))).toHaveLength(1);
  (await image('./icons/tools/new.svg')).resolve();
  await refresh;
  expect(store.getState().detectedTools).toHaveLength(2);
});

it('does not start a second scan while icons are pending', async () => {
  vi.mocked(api.scanTools).mockResolvedValue([tool('one')]);
  const first = store.getState().scanTools();
  const pending = await image('./icons/tools/one.svg');
  const second = store.getState().scanTools();
  expect(api.scanTools).toHaveBeenCalledTimes(1);
  pending.resolve();
  await Promise.all([first, second]);
});

it('preserves the last desktop when detection fails', async () => {
  const previous = [tool('cached')];
  store.getState().setDetectedTools(previous);
  vi.mocked(api.scanTools).mockRejectedValue(new Error('Scan failed'));
  await store.getState().scanTools();
  expect(store.getState().detectedTools).toBe(previous);
  expect(store.getState().isScanning).toBe(false);
});

async function restartWithCache(tools: LocalTool[]) {
  storage.set(cacheKey, JSON.stringify({ version: 1, platform: navigator.platform, tools }));
  vi.resetModules();
  store = (await import('./toolsStore')).useToolsStore;
}

it('restores metadata synchronously and can warm cached icons without scanning or writing', async () => {
  const tools = [{ ...tool('cached'), icon: './icons/tools/cached.svg' }];
  await restartWithCache(tools);
  expect(store.getState().detectedTools).toEqual(tools);
  expect(store.getState().isScanning).toBe(false);
  expect(store.getState().hasScanned).toBe(false);
  expect(store.getState().usingCachedTools).toBe(true);
  expect(api.scanTools).not.toHaveBeenCalled();
  expect(PendingImage.instances).toHaveLength(0);
  const { preloadDesktopToolIcons } = await import('../utils/toolIcons');
  const warming = preloadDesktopToolIcons(store.getState().detectedTools);
  expect(PendingImage.instances.map((image) => image.src)).toEqual(['./icons/tools/cached.svg']);
  PendingImage.instances[0].resolve();
  await warming;
  expect(setItem).not.toHaveBeenCalled();
});

it('keeps the cached desktop until refreshed icons are ready, then persists additions and removals', async () => {
  await restartWithCache([tool('keep'), tool('removed')]);
  const cached = store.getState().detectedTools;
  vi.mocked(api.scanTools).mockResolvedValue([tool('keep'), tool('added')]);
  const refresh = store.getState().scanTools();
  expect(store.getState().detectedTools).toBe(cached);
  expect(store.getState().isScanning).toBe(true);
  (await image('./icons/tools/keep.svg')).resolve();
  expect(store.getState().detectedTools).toBe(cached);
  (await image('./icons/tools/added.svg')).resolve();
  await refresh;
  expect(store.getState().detectedTools.map((entry) => entry.id)).toEqual(['keep', 'added']);
  expect(store.getState().usingCachedTools).toBe(false);
  expect(JSON.parse(storage.get(cacheKey)!).tools).toEqual(store.getState().detectedTools);
  vi.resetModules();
  store = (await import('./toolsStore')).useToolsStore;
  expect(store.getState().detectedTools.map((entry) => entry.id)).toEqual(['keep', 'added']);
});

it('preserves the startup snapshot in memory and storage when the refresh fails', async () => {
  await restartWithCache([tool('cached')]);
  const cached = store.getState().detectedTools;
  const saved = storage.get(cacheKey);
  vi.mocked(api.scanTools).mockRejectedValue(new Error('Scan failed'));
  await store.getState().scanTools();
  expect(store.getState().detectedTools).toBe(cached);
  expect(store.getState().isScanning).toBe(false);
  expect(storage.get(cacheKey)).toBe(saved);
  expect(store.getState().usingCachedTools).toBe(true);
  expect(setItem).not.toHaveBeenCalled();
});

it('persists an empty completed scan rather than resurrecting removed apps on restart', async () => {
  await restartWithCache([tool('removed')]);
  vi.mocked(api.scanTools).mockResolvedValue([]);
  await store.getState().scanTools();
  vi.resetModules();
  store = (await import('./toolsStore')).useToolsStore;
  expect(store.getState().detectedTools).toEqual([]);
  expect(JSON.parse(storage.get(cacheKey)!).tools).toEqual([]);
});

it.each([
  '{broken',
  JSON.stringify({ version: 2, platform: navigator.platform, tools: [tool('old')] }),
  JSON.stringify({ version: 1, platform: 'another-platform', tools: [tool('other')] }),
  JSON.stringify({ version: 1, platform: navigator.platform, tools: [{ id: 'incomplete' }] }),
  JSON.stringify({
    version: 1,
    platform: navigator.platform,
    tools: [{ ...tool('bad'), icon: {} }],
  }),
])('ignores an incompatible or malformed snapshot: %s', async (saved) => {
  storage.set(cacheKey, saved);
  vi.resetModules();
  store = (await import('./toolsStore')).useToolsStore;
  expect(store.getState().detectedTools).toEqual([]);
  expect(api.scanTools).not.toHaveBeenCalled();
});

it('persists successful tool metadata updates through the existing setter', async () => {
  store.getState().setDetectedTools([tool('one')]);
  store
    .getState()
    .setDetectedTools((tools) => tools.map((entry) => ({ ...entry, activeModel: 'new' })));
  expect(JSON.parse(storage.get(cacheKey)!).tools[0].activeModel).toBe('new');
  expect(api.scanTools).not.toHaveBeenCalled();
});

it('does not mistake a cached model-label edit for fresh installation detection', async () => {
  await restartWithCache([tool('cached')]);
  store
    .getState()
    .setDetectedTools((tools) => tools.map((entry) => ({ ...entry, activeModel: 'new' })));
  expect(store.getState().usingCachedTools).toBe(true);
  expect(api.scanTools).not.toHaveBeenCalled();
});

it('remains usable when storage cannot be read or written', async () => {
  vi.stubGlobal('localStorage', {
    getItem: () => {
      throw new Error('Storage unavailable');
    },
    setItem: () => {
      throw new Error('Storage full');
    },
  });
  vi.resetModules();
  store = (await import('./toolsStore')).useToolsStore;
  expect(store.getState().detectedTools).toEqual([]);
  store.getState().setDetectedTools([tool('one')]);
  expect(store.getState().detectedTools.map((entry) => entry.id)).toEqual(['one']);
  vi.mocked(api.scanTools).mockResolvedValue([]);
  await store.getState().scanTools();
  expect(store.getState().isScanning).toBe(false);
  expect(store.getState().hasScanned).toBe(true);
});
