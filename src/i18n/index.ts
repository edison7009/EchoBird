// Lightweight i18n entry — translation loader
// Tool names, API terms, and brand names stay in English

import { en } from './en';
import type { Translations } from './types';

const loadedLocales: Record<string, Partial<Translations>> = { en };

export async function loadLocale(locale: string): Promise<void> {
    if (loadedLocales[locale]) return;
    const resolved = resolveLocale(locale);
    if (!resolved || resolved === 'en') return;
    try {
        if (resolved === 'zh-Hans') {
            const mod = await import('./zh-Hans');
            if (mod?.default) loadedLocales['zh-Hans'] = mod.default;
        }
    } catch {
        // Silently fall back to English
    }
}

export function resolveLocale(tag: string): string {
    if (!tag) return 'en';
    // Any zh-* variant -> zh-Hans for now
    if (/^zh/i.test(tag)) return 'zh-Hans';
    return 'en';
}

// Synchronous translate (requires loadLocale called first)
export function t(locale: string, key: keyof Translations): string {
    const dict = loadedLocales[locale] || en;
    return (dict[key] || en[key] || key) as string;
}

export { en };
export type { Translations };