import { afterEach, expect, it, vi } from 'vitest';
import { ERROR_MESSAGES } from '../components/errorMessages';
import { en, loadLocale, translate, type TKey } from './index';
import { detectLocale, LOCALE_OPTIONS, resolveLocale } from './locales';

afterEach(() => vi.unstubAllGlobals());

it.each([
  ['zh-CN', 'zh-Hans'],
  ['zh_HK', 'zh-Hant'],
  ['zh-Hant-TW', 'zh-Hant'],
  ['zh-Hans-HK', 'zh-Hans'],
  ['en-GB', 'en'],
  ['ja-JP', 'ja'],
  ['ko-KR', 'ko'],
  ['es-419', 'es'],
  ['fr-CA', 'fr'],
  ['de-AT', 'de'],
  ['pt-BR', 'pt'],
  ['ru-RU', 'ru'],
  ['vi-VN', 'vi'],
  ['ar-SA', 'en'],
  ['esperanto', 'en'],
])('resolves %s to %s', (tag, locale) => {
  expect(resolveLocale(tag)).toBe(locale);
});

it('honors an explicit choice and can use the next supported system preference', () => {
  vi.stubGlobal('navigator', { language: 'it-IT', languages: ['it-IT', 'pt-BR'] });
  const getItem = vi.fn().mockReturnValue(null);
  vi.stubGlobal('localStorage', { getItem });
  expect(detectLocale()).toBe('pt');
  getItem.mockReturnValue('ko-KR');
  expect(detectLocale()).toBe('ko');
});

const newLocales = ['ko', 'es', 'fr', 'de', 'pt', 'ru', 'vi'];

it('provides a settings option and standalone error messages for every supported language', () => {
  expect(LOCALE_OPTIONS.map(({ id }) => id)).toEqual([
    'en',
    'zh-Hans',
    'zh-Hant',
    'ja',
    ...newLocales,
  ]);
  expect(Object.keys(ERROR_MESSAGES).sort()).toEqual(LOCALE_OPTIONS.map(({ id }) => id).sort());
});

it.each(newLocales)('loads %s and preserves interpolation variables', async (locale) => {
  await loadLocale(locale);
  expect(translate('settings.interfaceLanguage', locale)).not.toBe(
    en['settings.interfaceLanguage']
  );
  for (const key of Object.keys(en) as TKey[]) {
    const placeholders = (text: string) => (text.match(/\{[\w.:-]+\}/g) ?? []).sort();
    expect(placeholders(translate(key, locale)), key).toEqual(placeholders(en[key]));
  }
});
