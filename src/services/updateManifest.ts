export const MANIFEST_SOURCES = [
  'https://cnb.cool/echobird/echobird-releases/-/git/raw/main/manifest.json',
  'https://api.gitcode.com/api/v5/repos/edison7009/echobird-releases/raw/manifest.json?ref=main',
  'https://echobird.ai/api/version/index.json',
] as const;

export interface UpdateManifest {
  version: string;
  releaseDate?: string;
  releaseNotes?: string;
  releaseNotesI18n?: Record<string, string>;
}

const SEMVER = /^\d+\.\d+\.\d+$/;

export function isNewerVersion(remote: string, local: string): boolean {
  const r = remote.replace(/^v/, '').split('.').map(Number);
  const l = local.replace(/^v/, '').split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    if ((r[i] ?? 0) !== (l[i] ?? 0)) return (r[i] ?? 0) > (l[i] ?? 0);
  }
  return false;
}

function parseManifest(value: unknown): UpdateManifest | null {
  if (!value || typeof value !== 'object') return null;
  const item = value as Record<string, unknown>;
  if (typeof item.version !== 'string' || !SEMVER.test(item.version.replace(/^v/, ''))) return null;
  return {
    version: item.version.replace(/^v/, ''),
    releaseDate:
      typeof item.releaseDate === 'string'
        ? item.releaseDate
        : typeof item.publishedAt === 'string'
          ? item.publishedAt
          : undefined,
    releaseNotes: typeof item.releaseNotes === 'string' ? item.releaseNotes : undefined,
    releaseNotesI18n:
      item.releaseNotesI18n && typeof item.releaseNotesI18n === 'object'
        ? (item.releaseNotesI18n as Record<string, string>)
        : undefined,
  };
}

async function fetchOne(
  url: string,
  signal?: AbortSignal,
  timeoutMs = 8000
): Promise<UpdateManifest | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  const abort = () => controller.abort();
  signal?.addEventListener('abort', abort, { once: true });
  try {
    const response = await fetch(url, { signal: controller.signal, cache: 'no-store' });
    return response.ok ? parseManifest(await response.json()) : null;
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
    signal?.removeEventListener('abort', abort);
  }
}

export async function fetchLatestManifest(
  signal?: AbortSignal,
  timeoutMs = 8000
): Promise<UpdateManifest | null> {
  const results = await Promise.all(
    MANIFEST_SOURCES.map((url) => fetchOne(url, signal, timeoutMs))
  );
  if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
  return results
    .filter((m): m is UpdateManifest => !!m)
    .reduce<UpdateManifest | null>(
      (best, item) => (!best || isNewerVersion(item.version, best.version) ? item : best),
      null
    );
}
