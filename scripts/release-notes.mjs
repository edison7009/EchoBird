const NOTES_LABELS = {
  'zh-cn': '中文',
  en: 'English',
  'zh-tw': '繁體中文',
  ja: '日本語',
  ko: '한국어',
  es: 'Español',
  fr: 'Français',
  de: 'Deutsch',
  pt: 'Português',
  ru: 'Русский',
  vi: 'Tiếng Việt',
};

const NOTES_CODES = new Map(Object.entries(NOTES_LABELS).map(([code, label]) => [label, code]));

export function renderReleaseBody(notes) {
  return Object.entries(NOTES_LABELS)
    .filter(([code]) => typeof notes[code] === 'string' && notes[code].trim())
    .map(([code, label]) => {
      const open = code === 'zh-cn' ? ' open' : '';
      return `<details${open}>
<summary>${label}</summary>

${notes[code].trim()}

</details>`;
    })
    .join('\n\n');
}

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
  const languages = {};
  for (const match of (release.body ?? '').matchAll(
    /<details\b[^>]*>\s*<summary\b[^>]*>([^<]+)<\/summary>([\s\S]*?)<\/details>/gi
  )) {
    const code = NOTES_CODES.get(match[1].trim());
    if (code) languages[code] = match[2].trim();
  }
  return {
    releaseDate: release.published_at?.slice(0, 10),
    releaseNotes: languages['zh-cn'] ?? languages.en ?? release.body ?? '',
    ...(Object.keys(languages).length ? { releaseNotesI18n: languages } : {}),
  };
}
