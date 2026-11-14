import { getSystemApplicationIcon } from '../api/tauri';
import { getDecodedImage, preloadToolImage, releaseToolImage } from './toolIcons';

const cacheLifetime = 7 * 24 * 60 * 60 * 1000;
const maxCachedIcons = 128;
const maxCachedBytes = 16 * 1024 * 1024;
const icons = new Map<string, { src: string | null; bytes: number }>();
let cachedBytes = 0;
let backgroundQueue: Promise<void> = Promise.resolve();
const requests = new Map<string, Promise<string | null>>();
let generation = 0;
let database: Promise<IDBDatabase | null> | undefined;
let reset: Promise<void> = Promise.resolve();

interface CachedIcon {
  id: string;
  src: string;
  updatedAt: number;
}

function openCache(): Promise<IDBDatabase | null> {
  if (!database) {
    database = new Promise((resolve) => {
      if (typeof indexedDB === 'undefined') return resolve(null);
      try {
        const request = indexedDB.open('echobird-system-application-icons', 1);
        let blocked = false;
        request.onupgradeneeded = () =>
          request.result.createObjectStore('icons', { keyPath: 'id' });
        request.onsuccess = () => {
          const db = request.result;
          if (blocked) {
            db.close();
            return;
          }
          db.onversionchange = () => {
            db.close();
            database = undefined;
          };
          resolve(db);
        };
        request.onerror = () => resolve(null);
        request.onblocked = () => {
          blocked = true;
          resolve(null);
        };
      } catch {
        resolve(null);
      }
    });
  }
  return database;
}

async function readCache(id: string): Promise<string | null> {
  const db = await openCache();
  if (!db) return null;
  try {
    return await new Promise((resolve) => {
      const request = db.transaction('icons').objectStore('icons').get(id);
      request.onsuccess = () => {
        const entry = request.result as CachedIcon | undefined;
        resolve(
          entry &&
            Date.now() - entry.updatedAt < cacheLifetime &&
            typeof entry.src === 'string' &&
            (entry.src.startsWith('data:image/png;base64,') ||
              entry.src.startsWith('data:image/svg+xml;base64,'))
            ? entry.src
            : null
        );
      };
      request.onerror = () => resolve(null);
    });
  } catch {
    return null;
  }
}

async function writeCache(entry: CachedIcon): Promise<void> {
  const db = await openCache();
  if (!db) return;
  try {
    const transaction = db.transaction('icons', 'readwrite');
    transaction.objectStore('icons').put(entry);
    transaction.onerror = () => {
      /* Memory caching remains available if storage is full. */
    };
  } catch {
    /* Storage can be unavailable in restricted webviews. */
  }
}

export const cachedSystemApplicationIcon = (id: string) => icons.get(id)?.src;

function releaseUnusedImage(src: string | null | undefined): void {
  if (src && !Array.from(icons.values()).some((entry) => entry.src === src)) {
    releaseToolImage(src);
  }
}

function rememberIcon(id: string, src: string | null): void {
  const image = src ? getDecodedImage(src) : undefined;
  // Budget retained source text and decoded RGBA pixels conservatively.
  const bytes = src
    ? src.length * 2 + (image?.naturalWidth || 64) * (image?.naturalHeight || 64) * 4
    : 0;
  const previous = icons.get(id);
  cachedBytes -= previous?.bytes ?? 0;
  icons.delete(id);
  icons.set(id, { src, bytes });
  cachedBytes += bytes;
  releaseUnusedImage(previous?.src);
  while (icons.size > maxCachedIcons || cachedBytes > maxCachedBytes) {
    const oldest = icons.entries().next().value!;
    icons.delete(oldest[0]);
    cachedBytes -= oldest[1].bytes;
    releaseUnusedImage(oldest[1].src);
  }
}

async function decodeIcon(src: string | null): Promise<string | null> {
  if (!src || typeof Image === 'undefined') return src;
  return (await preloadToolImage(src)) ? src : null;
}

export function loadSystemApplicationIcon(id: string): Promise<string | null> {
  const cached = icons.get(id);
  if (cached) {
    icons.delete(id);
    icons.set(id, cached);
    return Promise.resolve(cached.src);
  }
  const existing = requests.get(id);
  if (existing) return existing;
  const revision = generation;
  const request = (async () => {
    await reset;
    const cached = await decodeIcon(await readCache(id));
    const src = cached || (await decodeIcon(await getSystemApplicationIcon(id)));
    if (revision === generation) {
      rememberIcon(id, src);
      if (src && !cached) void writeCache({ id, src, updatedAt: Date.now() });
    } else {
      releaseUnusedImage(src);
    }
    return src;
  })()
    .catch(() => null)
    .finally(() => {
      if (requests.get(id) === request) requests.delete(id);
    });
  requests.set(id, request);
  return request;
}

export async function invalidateSystemApplicationIcons(): Promise<void> {
  generation++;
  const sources = Array.from(icons.values(), (entry) => entry.src);
  icons.clear();
  cachedBytes = 0;
  sources.forEach(releaseUnusedImage);
  requests.clear();
  reset = (async () => {
    const db = await openCache();
    if (!db) return;
    await new Promise<void>((resolve) => {
      try {
        const transaction = db.transaction('icons', 'readwrite');
        transaction.objectStore('icons').clear();
        transaction.oncomplete = transaction.onerror = transaction.onabort = () => resolve();
      } catch {
        resolve();
      }
    });
  })();
  await reset;
}

/** Warm a bounded nearby set while its page is active; visible tiles load independently. */
export function preloadSystemApplicationIcons(ids: string[], signal?: AbortSignal): Promise<void> {
  const revision = generation;
  const nearby = ids.slice(0, maxCachedIcons);
  const work = backgroundQueue.then(async () => {
    let next = 0;
    const worker = async () => {
      while (!signal?.aborted && revision === generation && next < nearby.length) {
        const id = nearby[next++];
        await loadSystemApplicationIcon(id);
      }
    };
    await Promise.all(Array.from({ length: Math.min(4, nearby.length) }, worker));
  });
  // A new page/query waits for at most the old queue's four in-flight reads.
  backgroundQueue = work.catch(() => {});
  return work;
}
