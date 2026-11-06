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
  const notes = typeof item.releaseNotes === 'string' ? item.releaseNotes : undefined;
  const languages: Record<string, string> = {};
  const labels: Record<string, string> = {
    中文: 'zh-cn',
    English: 'en',
    繁體中文: 'zh-tw',
    日本語: 'ja',
  };
  for (const match of (notes ?? '').matchAll(
    /<details\b[^>]*>\s*<summary\b[^>]*>([^<]+)<\/summary>([\s\S]*?)<\/details>/gi
  )) {
    const code = labels[match[1].trim()];
    if (code) languages[code] = match[2].trim();
  }
  if (item.releaseNotesI18n && typeof item.releaseNotesI18n === 'object') {
    for (const [code, value] of Object.entries(item.releaseNotesI18n)) {
      if (typeof value === 'string' && value.trim()) languages[code] = value.trim();
    }
  }
  const date =
    typeof item.releaseDate === 'string'
      ? item.releaseDate
      : typeof item.publishedAt === 'string'
        ? item.publishedAt
        : undefined;
  return {
    version: item.version.replace(/^v/, ''),
    releaseDate: date?.match(/^\d{4}-\d{2}-\d{2}/)?.[0] ?? date,
    releaseNotes: languages['zh-cn'] ?? languages.en ?? notes,
    releaseNotesI18n: Object.keys(languages).length ? languages : undefined,
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
    .reduce<UpdateManifest | null>((best, item) => {
      if (!best || isNewerVersion(item.version, best.version)) return item;
      if (item.version !== best.version) return best;
      return {
        ...best,
        releaseDate: item.releaseDate ?? best.releaseDate,
        releaseNotes: item.releaseNotes ?? best.releaseNotes,
        releaseNotesI18n: { ...best.releaseNotesI18n, ...item.releaseNotesI18n },
      };
    }, null);
}
