import { getSystemApplicationIcon } from '../api/tauri';
import { preloadToolImage } from './toolIcons';

const cacheLifetime = 7 * 24 * 60 * 60 * 1000;
const icons = new Map<string, string | null>();
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

export const cachedSystemApplicationIcon = (id: string) => icons.get(id);

async function decodeIcon(src: string | null): Promise<string | null> {
  if (!src || typeof Image === 'undefined') return src;
  return (await preloadToolImage(src)) ? src : null;
}

export function loadSystemApplicationIcon(id: string): Promise<string | null> {
  if (icons.has(id)) return Promise.resolve(icons.get(id)!);
  const existing = requests.get(id);
  if (existing) return existing;
  const revision = generation;
  const request = (async () => {
    await reset;
    const cached = await decodeIcon(await readCache(id));
    const src = cached || (await decodeIcon(await getSystemApplicationIcon(id)));
    if (revision === generation) {
      icons.set(id, src);
      if (src && !cached) void writeCache({ id, src, updatedAt: Date.now() });
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
  icons.clear();
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

/** Keep a small background queue so visible tiles can request icons immediately. */
export async function preloadSystemApplicationIcons(ids: string[]): Promise<void> {
  const revision = generation;
  let next = 0;
  const worker = async () => {
    while (revision === generation && next < ids.length) {
      const id = ids[next++];
      await loadSystemApplicationIcon(id);
    }
  };
  await Promise.all(Array.from({ length: Math.min(4, ids.length) }, worker));
}
