// Lightweight i18n entry — translation loader
// Tool names, API terms, and brand names stay in English

import { en } from './en';
import type { TKey, Translations } from './types';

export type { TKey, Translations } from './types';
export { en };

const loadedLocales: Record<string, Partial<Translations>> = { en };

const localeLoaders: Record<string, () => Promise<{ default: Partial<Translations> }>> = {
  'zh-Hans': () => import('./zh-Hans'),
  'zh-Hant': () => import('./zh-Hant'),
  ja: () => import('./ja'),
};

export function resolveLocale(tag: string): string {
  if (!tag) return 'en';
  // Traditional Chinese: TW / HK / MO / explicit Hant tag
  if (/^zh[-_](TW|HK|MO|Hant)/i.test(tag)) return 'zh-Hant';
  // Anything else starting with zh → Simplified
  if (/^zh/i.test(tag)) return 'zh-Hans';
  if (/^ja/i.test(tag)) return 'ja';
  return 'en';
}

export function detectLocale(): string {
  try {
    const saved = localStorage.getItem('echobird-locale');
    if (saved) return resolveLocale(saved);
  } catch {
    /* ignore */
  }
  const lang = typeof navigator !== 'undefined' ? navigator.language || 'en' : 'en';
  return resolveLocale(lang);
}

export async function loadLocale(locale: string): Promise<void> {
  if (loadedLocales[locale]) return;
  const loader = localeLoaders[locale];
  if (!loader) return;
  try {
    const mod = await loader();
    if (mod?.default) loadedLocales[locale] = mod.default;
  } catch {
    // Silently fall back to English
  }
}

export function translate(key: TKey, locale: string): string {
  return loadedLocales[locale]?.[key] || en[key] || key;
}
