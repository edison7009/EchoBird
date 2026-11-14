import { beforeEach, afterEach, expect, it, vi } from 'vitest';
import { getSystemApplicationIcon } from '../api/tauri';

vi.mock('../api/tauri', () => ({ getSystemApplicationIcon: vi.fn() }));
const png = (id: string) => `data:image/png;base64,${id}`;
const decode = vi.fn().mockResolvedValue(undefined);
beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  decode.mockResolvedValue(undefined);
  vi.stubGlobal(
    'Image',
    class {
      src = '';
      decode = decode;
    }
  );
  vi.stubGlobal('indexedDB', undefined);
  vi.mocked(getSystemApplicationIcon).mockImplementation(async (id) => png(id));
});
afterEach(() => vi.unstubAllGlobals());

function persistentCache(failWrites = false) {
  const entries = new Map<string, { id: string; src: string; updatedAt: number }>();
  let created = false;
  const database = {
    createObjectStore: vi.fn(),
    close: vi.fn(),
    onversionchange: null,
    transaction: () => {
      const transaction = {
        oncomplete: null as (() => void) | null,
        objectStore: () => ({
          get: (id: string) => {
            const request = {
              result: entries.get(id),
              onsuccess: null as (() => void) | null,
              onerror: null,
            };
            queueMicrotask(() => request.onsuccess?.());
            return request;
          },
          put: (entry: { id: string; src: string; updatedAt: number }) => {
            if (failWrites) throw new Error('Quota exceeded');
            entries.set(entry.id, entry);
          },
          clear: () => {
            entries.clear();
            queueMicrotask(() => transaction.oncomplete?.());
          },
        }),
      };
      return transaction;
    },
  };
  vi.stubGlobal('indexedDB', {
    open: () => {
      const request = {
        result: database,
        onupgradeneeded: null as (() => void) | null,
        onsuccess: null as (() => void) | null,
      };
      queueMicrotask(() => {
        if (!created) {
          created = true;
          request.onupgradeneeded?.();
        }
        request.onsuccess?.();
      });
      return request;
    },
  });
  return entries;
}

it('preloads every application with four background workers and decodes before publishing', async () => {
  let running = 0;
  let maximum = 0;
  vi.mocked(getSystemApplicationIcon).mockImplementation(async (id) => {
    running++;
    maximum = Math.max(maximum, running);
    await new Promise((resolve) => setTimeout(resolve, 1));
    running--;
    return png(id);
  });
  const cache = await import('./systemApplicationIcons');
  const ids = Array.from({ length: 12 }, (_, index) => `app-${index}`);
  await cache.preloadSystemApplicationIcons(ids);
  expect(maximum).toBe(4);
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(12);
  expect(decode).toHaveBeenCalledTimes(12);
  ids.forEach((id) => expect(cache.cachedSystemApplicationIcon(id)).toBe(png(id)));
});

it('deduplicates visible and background requests and waits for image decoding', async () => {
  let decoded!: () => void;
  decode.mockImplementation(
    () =>
      new Promise<void>((resolve) => {
        decoded = resolve;
      })
  );
  const cache = await import('./systemApplicationIcons');
  const first = cache.loadSystemApplicationIcon('one');
  expect(cache.loadSystemApplicationIcon('one')).toBe(first);
  await vi.waitFor(() => expect(decode).toHaveBeenCalledOnce());
  expect(cache.cachedSystemApplicationIcon('one')).toBeUndefined();
  decoded();
  await first;
  await cache.loadSystemApplicationIcon('one');
  expect(getSystemApplicationIcon).toHaveBeenCalledOnce();
});

it.each([
  ['PNG', png('one')],
  ['SVG', 'data:image/svg+xml;base64,PHN2Zy8+'],
])('reuses persisted %s icons after a new application session', async (_format, src) => {
  const entries = persistentCache();
  vi.mocked(getSystemApplicationIcon).mockResolvedValueOnce(src);
  const first = await import('./systemApplicationIcons');
  await first.loadSystemApplicationIcon('one');
  await vi.waitFor(() => expect(entries.get('one')?.src).toBe(src));
  vi.resetModules();
  vi.mocked(getSystemApplicationIcon).mockClear();
  const second = await import('./systemApplicationIcons');
  expect(await second.loadSystemApplicationIcon('one')).toBe(src);
  expect(getSystemApplicationIcon).not.toHaveBeenCalled();
  expect(decode).toHaveBeenCalledTimes(2);
});

it.each(['https://example.test/icon.png', 'data:text/html;base64,PGgxLz4='])(
  'ignores unsupported persisted icon sources (%s)',
  async (src) => {
    const entries = persistentCache();
    entries.set('one', { id: 'one', src, updatedAt: Date.now() });
    const cache = await import('./systemApplicationIcons');
    expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
    expect(getSystemApplicationIcon).toHaveBeenCalledExactlyOnceWith('one');
    expect(decode).toHaveBeenCalledOnce();
  }
);

it('rebuilds expired entries and clears persisted icons on explicit refresh', async () => {
  const entries = persistentCache();
  entries.set('one', { id: 'one', src: png('old'), updatedAt: Date.now() - 8 * 86400000 });
  const cache = await import('./systemApplicationIcons');
  expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
  await cache.invalidateSystemApplicationIcons();
  expect(entries.size).toBe(0);
  expect(cache.cachedSystemApplicationIcon('one')).toBeUndefined();
  await cache.loadSystemApplicationIcon('one');
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(2);
});

it.each(['unavailable', 'quota'])(
  'keeps memory caching working when persistence is %s',
  async (failure) => {
    if (failure === 'quota') persistentCache(true);
    else
      vi.stubGlobal('indexedDB', {
        open: () => {
          throw new Error('Unavailable');
        },
      });
    const cache = await import('./systemApplicationIcons');
    expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
    expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
    expect(getSystemApplicationIcon).toHaveBeenCalledOnce();
  }
);

it('ignores old writes and memory results after refresh invalidates an in-flight request', async () => {
  const entries = persistentCache();
  let finish!: (src: string) => void;
  vi.mocked(getSystemApplicationIcon)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    )
    .mockResolvedValueOnce(png('new'));
  const cache = await import('./systemApplicationIcons');
  const old = cache.loadSystemApplicationIcon('one');
  await vi.waitFor(() => expect(getSystemApplicationIcon).toHaveBeenCalledOnce());
  await cache.invalidateSystemApplicationIcons();
  await cache.loadSystemApplicationIcon('one');
  finish(png('old'));
  await old;
  expect(cache.cachedSystemApplicationIcon('one')).toBe(png('new'));
  expect(entries.get('one')?.src).toBe(png('new'));
});

it('retries native failures and never persists undecodable images', async () => {
  const entries = persistentCache();
  vi.mocked(getSystemApplicationIcon).mockRejectedValueOnce(new Error('Unavailable'));
  const cache = await import('./systemApplicationIcons');
  expect(await cache.loadSystemApplicationIcon('one')).toBeNull();
  expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
  decode.mockRejectedValueOnce(new Error('Bad image'));
  expect(await cache.loadSystemApplicationIcon('broken')).toBeNull();
  expect(entries.has('broken')).toBe(false);
});

it('falls back while an upgrade is blocked and closes a late database connection', async () => {
  const close = vi.fn();
  vi.stubGlobal('indexedDB', {
    open: () => {
      const request = {
        result: { close },
        onblocked: null as (() => void) | null,
        onsuccess: null as (() => void) | null,
      };
      queueMicrotask(() => {
        request.onblocked?.();
        request.onsuccess?.();
      });
      return request;
    },
  });
  const cache = await import('./systemApplicationIcons');
  expect(await cache.loadSystemApplicationIcon('one')).toBe(png('one'));
  expect(close).toHaveBeenCalledOnce();
});

it('caps background warming for a large inventory and skips aborted work', async () => {
  const cache = await import('./systemApplicationIcons');
  const controller = new AbortController();
  controller.abort();
  await cache.preloadSystemApplicationIcons(['aborted'], controller.signal);
  expect(getSystemApplicationIcon).not.toHaveBeenCalled();
  await cache.preloadSystemApplicationIcons(
    Array.from({ length: 1000 }, (_, index) => `bounded-${index}`)
  );
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(128);
  expect(cache.cachedSystemApplicationIcon('bounded-128')).toBeUndefined();
});

it('stops the old queue on leaving and serializes a new page behind its in-flight reads', async () => {
  const pending: Array<() => void> = [];
  let running = 0;
  let maximum = 0;
  vi.mocked(getSystemApplicationIcon).mockImplementation((id) => {
    running++;
    maximum = Math.max(maximum, running);
    return new Promise((resolve) => {
      pending.push(() => {
        running--;
        resolve(png(id));
      });
    });
  });
  const cache = await import('./systemApplicationIcons');
  const controller = new AbortController();
  const old = cache.preloadSystemApplicationIcons(
    Array.from({ length: 20 }, (_, index) => `old-${index}`),
    controller.signal
  );
  await vi.waitFor(() => expect(getSystemApplicationIcon).toHaveBeenCalledTimes(4));
  controller.abort();
  const next = cache.preloadSystemApplicationIcons(['new']);
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(4);
  pending.splice(0).forEach((finish) => finish());
  await old;
  await vi.waitFor(() => expect(getSystemApplicationIcon).toHaveBeenCalledTimes(5));
  expect(getSystemApplicationIcon).toHaveBeenLastCalledWith('new');
  pending.splice(0).forEach((finish) => finish());
  await next;
  expect(maximum).toBe(4);
});

it('evicts least-recently-used icons and decoded images while retaining the disk cache', async () => {
  const entries = persistentCache();
  const cache = await import('./systemApplicationIcons');
  const images = await import('./toolIcons');
  await Promise.all(
    Array.from({ length: 128 }, (_, index) => cache.loadSystemApplicationIcon(`lru-${index}`))
  );
  await vi.waitFor(() => expect(entries.size).toBe(128));
  await cache.loadSystemApplicationIcon('lru-0');
  await cache.loadSystemApplicationIcon('lru-extra');
  expect(cache.cachedSystemApplicationIcon('lru-0')).toBe(png('lru-0'));
  expect(cache.cachedSystemApplicationIcon('lru-1')).toBeUndefined();
  expect(images.getDecodedImage(png('lru-1'))).toBeUndefined();
  expect(entries.has('lru-1')).toBe(true);
  expect(await cache.loadSystemApplicationIcon('lru-1')).toBe(png('lru-1'));
  expect(getSystemApplicationIcon).toHaveBeenCalledTimes(129);
});

it('releases oversized decoded images instead of retaining them above the memory budget', async () => {
  vi.stubGlobal(
    'Image',
    class {
      src = '';
      naturalWidth = 4096;
      naturalHeight = 4096;
      decode = decode;
    }
  );
  const cache = await import('./systemApplicationIcons');
  const images = await import('./toolIcons');
  expect(await cache.loadSystemApplicationIcon('large')).toBe(png('large'));
  expect(cache.cachedSystemApplicationIcon('large')).toBeUndefined();
  expect(images.getDecodedImage(png('large'))).toBeUndefined();
});

it('releases decoded images on refresh, including an obsolete request that finishes late', async () => {
  const cache = await import('./systemApplicationIcons');
  const images = await import('./toolIcons');
  await cache.loadSystemApplicationIcon('ready');
  expect(images.getDecodedImage(png('ready'))).toBeDefined();
  let finish!: (src: string) => void;
  vi.mocked(getSystemApplicationIcon).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      })
  );
  const old = cache.loadSystemApplicationIcon('late');
  await vi.waitFor(() => expect(getSystemApplicationIcon).toHaveBeenCalledTimes(2));
  await cache.invalidateSystemApplicationIcons();
  expect(images.getDecodedImage(png('ready'))).toBeUndefined();
  finish(png('late'));
  await old;
  expect(images.getDecodedImage(png('late'))).toBeUndefined();
  expect(cache.cachedSystemApplicationIcon('late')).toBeUndefined();
});
