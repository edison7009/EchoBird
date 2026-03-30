// Lightweight i18n entry — translation loader
// Tool names, API terms, and brand names stay in English

import { en } from './en';
import type { Translations } from './types';

export type TKey = keyof Translations;
export type { Translations };
export { en };

const loadedLocales: Record<string, Partial<Translations>> = { en };

const localeLoaders: Record<string, () => Promise<{ default: Partial<Translations> }>> = {
    'zh-Hans': () => import('./zh-Hans'),
};

export function resolveLocale(tag: string): string {
    if (!tag) return 'en';
    if (/^zh/i.test(tag)) return 'zh-Hans';
    return 'en';
}

export function detectLocale(): string {
    try {
        const saved = localStorage.getItem('echobird-locale');
        if (saved) return resolveLocale(saved);
    } catch { /* ignore */ }
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
    const dict = loadedLocales[locale] || en;
    return ((dict as Record<string, string>)[key as string] || (en as Record<string, string>)[key as string] || key as string);
}