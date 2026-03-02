// Lightweight i18n entry — translation loader
// Tool names, API terms, and brand names stay in English (no translation)

// Types re-exported from types.ts
export type { TKey, Translations } from './types';
import type { Translations } from './types';
import type { TKey } from './types';
import { en } from './en';

// Lazy-load locale packs on demand
const localeModules: Record<string, () => Promise<{ default: Partial<Translations> }>> = {
    'zh-Hans': () => import('./zh-Hans'),
    'zh-Hant': () => import('./zh-Hant'),
    ja: () => import('./ja'),
    ko: () => import('./ko'),
    de: () => import('./de'),
    fr: () => import('./fr'),
    es: () => import('./es'),
    pt: () => import('./pt'),
    it: () => import('./it'),
    nl: () => import('./nl'),
    ru: () => import('./ru'),
    ar: () => import('./ar'),
    hi: () => import('./hi'),
    bn: () => import('./bn'),
    th: () => import('./th'),
    vi: () => import('./vi'),
    id: () => import('./id'),
    ms: () => import('./ms'),
    tr: () => import('./tr'),
    pl: () => import('./pl'),
    cs: () => import('./cs'),
    hu: () => import('./hu'),
    sv: () => import('./sv'),
    fi: () => import('./fi'),
    el: () => import('./el'),
    he: () => import('./he'),
    fa: () => import('./fa'),
};

// Loaded locale cache
const loadedLocales: Record<string, Translations> = { en };

// Supported locale codes (besides 'en')
export const supportedLocales = ['en', ...Object.keys(localeModules)];

// Detect best matching locale from browser/system language
export function detectLocale(): string {
    try {
        const langs = navigator.languages || [navigator.language];
        for (const lang of langs) {
            const tag = lang.trim();
            // Exact match (e.g. 'ja', 'ko', 'de')
            if (supportedLocales.includes(tag)) return tag;
            // Map BCP47 to our keys
            if (/^zh\b.*(?:Hans|CN|SG)/i.test(tag)) return 'zh-Hans';
            if (/^zh\b/i.test(tag)) return 'zh-Hant';
            // Base language match (e.g. 'pt-BR' -> 'pt')
            const base = tag.split('-')[0].toLowerCase();
            if (supportedLocales.includes(base)) return base;
        }
    } catch { /* SSR / test env */ }
    return 'en';
}

// Preload locale pack
export async function loadLocale(locale: string): Promise<void> {
    if (locale === 'en' || loadedLocales[locale]) return;
    const loader = localeModules[locale];
    if (!loader) return;
    try {
        const mod = await loader();
        loadedLocales[locale] = { ...en, ...mod.default };
    } catch {
        console.warn(`[i18n] Failed to load locale: ${locale}`);
    }
}

// Synchronous translate (requires loadLocale called first)
export function translate(key: TKey, locale: string): string {
    const dict = loadedLocales[locale] || en;
    return dict[key] || en[key] || key;
}
