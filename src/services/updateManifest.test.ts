import { afterEach, expect, it, vi } from 'vitest';
import { fetchLatestManifest, MANIFEST_SOURCES } from './updateManifest';
afterEach(() => vi.unstubAllGlobals());
const body =
  '<details><summary>中文</summary>\n\n## 中文更新\n\n内容\n</details>\n<details><summary>English</summary>\n\n## English update\n\nNotes\n</details>';
it('normalizes legacy GitHub details from mirrors and merges same-version localized file metadata', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) =>
      Response.json(
        url === MANIFEST_SOURCES[2]
          ? {
              version: '6.2.1',
              releaseDate: '2026-11-06',
              releaseNotes: 'Canonical Chinese',
              releaseNotesI18n: {
                'zh-cn': 'Canonical Chinese',
                en: 'Canonical English',
                'zh-tw': '繁體更新',
                ja: '日本語更新',
              },
            }
          : { version: '6.2.1', releaseDate: '2026-11-06T00:08:40Z', releaseNotes: body }
      )
    )
  );
  const manifest = await fetchLatestManifest();
  expect(manifest?.releaseDate).toBe('2026-11-06');
  expect(manifest?.releaseNotesI18n).toEqual({
    'zh-cn': 'Canonical Chinese',
    en: 'Canonical English',
    'zh-tw': '繁體更新',
    ja: '日本語更新',
  });
  expect(JSON.stringify(manifest)).not.toContain('<details>');
});
it('extracts languages when only the legacy mirror body is available and rejects invalid localized values', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () =>
      Response.json({
        version: '6.2.1',
        releaseNotes: body,
        publishedAt: '2026-11-06T00:08:40Z',
        releaseNotesI18n: { en: {}, ja: 42 },
      })
    )
  );
  const manifest = await fetchLatestManifest();
  expect(manifest?.releaseNotesI18n).toEqual({
    'zh-cn': '## 中文更新\n\n内容',
    en: '## English update\n\nNotes',
  });
  expect(manifest?.releaseDate).toBe('2026-11-06');
});
it('never merges release notes from different versions', async () => {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string) =>
      Response.json(
        url === MANIFEST_SOURCES[0]
          ? { version: '6.2.2', releaseNotesI18n: { en: 'New notes' } }
          : { version: '6.2.1', releaseNotesI18n: { en: 'Old notes', ja: 'Old Japanese' } }
      )
    )
  );
  expect((await fetchLatestManifest())?.releaseNotesI18n).toEqual({ en: 'New notes' });
});
