// Lazy-load language packs outside the bundled English fallback.
import { en } from './en';
import { resolveLocale, type Locale } from './locales';
import type { TKey, Translations } from './types';

export { detectLocale, resolveLocale } from './locales';
export type { TKey, Translations } from './types';
export { en };

const loadedLocales = new Map<string, Partial<Translations>>([['en', en]]);

const localeLoaders: Record<
  Exclude<Locale, 'en'>,
  () => Promise<{ default: Partial<Translations> }>
> = {
  'zh-Hans': () => import('./zh-Hans'),
  'zh-Hant': () => import('./zh-Hant'),
  ja: () => import('./ja'),
  ko: () => import('./ko'),
  es: () => import('./es'),
  fr: () => import('./fr'),
  de: () => import('./de'),
  pt: () => import('./pt'),
  ru: () => import('./ru'),
  vi: () => import('./vi'),
};

export async function loadLocale(tag: string): Promise<void> {
  const locale = resolveLocale(tag);
  if (locale === 'en' || loadedLocales.has(locale)) return;
  try {
    loadedLocales.set(locale, (await localeLoaders[locale]()).default);
  } catch {
    // Keep the English fallback if a language pack cannot be loaded.
  }
}

export function translate(key: TKey, locale: string): string {
  return loadedLocales.get(locale)?.[key] || en[key] || key;
}
