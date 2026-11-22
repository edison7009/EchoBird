// Lightweight i18n React Hook + Context (lazy-loaded language packs)
import React, { useContext, useState, useCallback, useEffect, useRef } from 'react';
import { translate, loadLocale, detectLocale, resolveLocale, type TKey } from '../i18n';
import { LOCALE_STORAGE_KEY } from '../i18n/locales';
import * as api from '../api/tauri';
import { I18nContext } from './i18nContext';

interface I18nProviderProps {
  children: React.ReactNode;
  /** main.tsx loads this language pack before the first paint. */
  initialLocale?: string;
}

export const I18nProvider: React.FC<I18nProviderProps> = ({ children, initialLocale }) => {
  const [locale, setActiveLocale] = useState(() => resolveLocale(initialLocale ?? 'en'));
  const generation = useRef(0);
  const saveQueue = useRef(Promise.resolve());

  const activate = useCallback(async (tag: string, save: boolean) => {
    const request = ++generation.current;
    const next = resolveLocale(tag);
    await loadLocale(next);
    if (request !== generation.current) return;
    setActiveLocale(next);
    if (!save) return;
    try {
      localStorage.setItem(LOCALE_STORAGE_KEY, next);
    } catch {
      // Rust settings are the persistent source of truth.
    }
    saveQueue.current = saveQueue.current
      .then(async () => {
        if (request === generation.current) await api.saveSettings({ locale: next });
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!initialLocale) void activate(detectLocale(), false);
    const onStorage = (event: StorageEvent) => {
      if (event.key === LOCALE_STORAGE_KEY && event.newValue) {
        void activate(event.newValue, false);
      }
    };
    window.addEventListener('storage', onStorage);
    return () => {
      generation.current += 1;
      window.removeEventListener('storage', onStorage);
    };
  }, [activate, initialLocale]);

  const setLocale = useCallback((tag: string) => void activate(tag, true), [activate]);
  const t = useCallback((key: TKey) => translate(key, locale), [locale]);

  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return React.createElement(I18nContext.Provider, { value: { locale, setLocale, t } }, children);
};

export function useI18n() {
  return useContext(I18nContext);
}
