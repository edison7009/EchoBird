export const LOCALE_OPTIONS = [
  { id: 'en', glyph: 'A', label: 'English' },
  { id: 'zh-Hans', glyph: '简', label: '简体中文' },
  { id: 'zh-Hant', glyph: '繁', label: '繁體中文' },
  { id: 'ja', glyph: 'あ', label: '日本語' },
  { id: 'ko', glyph: '가', label: '한국어' },
  { id: 'es', glyph: 'Ñ', label: 'Español' },
  { id: 'fr', glyph: 'Fr', label: 'Français' },
  { id: 'de', glyph: 'De', label: 'Deutsch' },
  { id: 'pt', glyph: 'Pt', label: 'Português' },
  { id: 'ru', glyph: 'Я', label: 'Русский' },
  { id: 'vi', glyph: 'Vi', label: 'Tiếng Việt' },
] as const;

export type Locale = (typeof LOCALE_OPTIONS)[number]['id'];

export const LOCALE_STORAGE_KEY = 'echobird-locale';

function matchLocale(tag: string): Locale | undefined {
  const [language, ...subtags] = tag.trim().toLowerCase().split(/[-_]/);
  if (language === 'zh') {
    if (subtags.includes('hans')) return 'zh-Hans';
    if (subtags.includes('hant') || subtags.some((tag) => ['tw', 'hk', 'mo'].includes(tag))) {
      return 'zh-Hant';
    }
    return 'zh-Hans';
  }
  return LOCALE_OPTIONS.find(({ id }) => id === language)?.id;
}

export function resolveLocale(tag: string): Locale {
  return matchLocale(tag) ?? 'en';
}

export function detectLocale(): Locale {
  try {
    const saved = localStorage.getItem(LOCALE_STORAGE_KEY);
    if (saved) return resolveLocale(saved);
  } catch {
    // Storage can be unavailable before the app has started.
  }
  if (typeof navigator !== 'undefined') {
    for (const tag of new Set([...(navigator.languages ?? []), navigator.language])) {
      const match = tag && matchLocale(tag);
      if (match) return match;
    }
  }
  return 'en';
}
