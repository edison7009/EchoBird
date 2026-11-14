export const siteLanguages = {
  en: { label: 'English', tag: 'en', og: 'en_US' },
  zh: { label: '简体中文', tag: 'zh-CN', og: 'zh_CN' },
  'zh-Hant': { label: '繁體中文', tag: 'zh-Hant', og: 'zh_TW' },
  ja: { label: '日本語', tag: 'ja', og: 'ja_JP' },
  ko: { label: '한국어', tag: 'ko', og: 'ko_KR' },
  es: { label: 'Español', tag: 'es', og: 'es_ES' },
  'pt-BR': { label: 'Português (Brasil)', tag: 'pt-BR', og: 'pt_BR' },
  de: { label: 'Deutsch', tag: 'de', og: 'de_DE' },
  fr: { label: 'Français', tag: 'fr', og: 'fr_FR' },
  ru: { label: 'Русский', tag: 'ru', og: 'ru_RU' },
  id: { label: 'Bahasa Indonesia', tag: 'id', og: 'id_ID' },
  vi: { label: 'Tiếng Việt', tag: 'vi', og: 'vi_VN' },
  th: { label: 'ไทย', tag: 'th', og: 'th_TH' },
  tr: { label: 'Türkçe', tag: 'tr', og: 'tr_TR' },
  ar: { label: 'العربية', tag: 'ar', og: 'ar_SA', dir: 'rtl' },
  hi: { label: 'हिन्दी', tag: 'hi', og: 'hi_IN' },
};
export const siteLocales = Object.keys(siteLanguages);
export const docsLocales = ['zh', 'en'];
export const docsLocale = (locale) => (locale === 'zh' || locale === 'zh-Hant' ? 'zh' : 'en');
export function resolveSiteLanguage(value) {
  const tag = String(value || '')
    .replaceAll('_', '-')
    .toLowerCase();
  if (/^zh(?:-|$)/.test(tag)) {
    const subtags = tag.split('-');
    if (subtags.includes('hans')) return 'zh';
    return subtags.includes('hant') || subtags.some((part) => ['tw', 'hk', 'mo'].includes(part))
      ? 'zh-Hant'
      : 'zh';
  }
  if (/^pt(?:-|$)/.test(tag)) return 'pt-BR';
  return (
    siteLocales.find((locale) => locale.toLowerCase() === tag) ||
    siteLocales.find((locale) => locale === tag.split('-')[0]) ||
    null
  );
}
