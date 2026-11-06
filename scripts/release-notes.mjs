// Shared by full installer syncs and metadata-only corrections.
export function notesForRelease(release, canonical) {
  const version = release.tag_name.replace(/^v/, '');
  const clean = Object.fromEntries(
    Object.entries(canonical?.releaseNotesI18n ?? {}).filter(
      ([, value]) => typeof value === 'string' && value.trim()
    )
  );
  if (canonical?.version === version && Object.keys(clean).length) {
    return {
      releaseDate: canonical.releaseDate,
      releaseNotes: canonical.releaseNotes ?? clean['zh-cn'] ?? clean.en,
      releaseNotesI18n: clean,
    };
  }
  const codes = { 中文: 'zh-cn', English: 'en', 繁體中文: 'zh-tw', 日本語: 'ja' };
  const languages = {};
  for (const match of (release.body ?? '').matchAll(
    /<details\b[^>]*>\s*<summary\b[^>]*>([^<]+)<\/summary>([\s\S]*?)<\/details>/gi
  )) {
    const code = codes[match[1].trim()];
    if (code) languages[code] = match[2].trim();
  }
  return {
    releaseDate: release.published_at?.slice(0, 10),
    releaseNotes: languages['zh-cn'] ?? languages.en ?? release.body ?? '',
    ...(Object.keys(languages).length ? { releaseNotesI18n: languages } : {}),
  };
}
