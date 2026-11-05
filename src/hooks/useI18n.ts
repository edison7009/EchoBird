// Lightweight i18n React Hook + Context (lazy-loaded language packs)
import React, { useContext, useState, useCallback, useEffect } from 'react';
import { translate, loadLocale, detectLocale, TKey } from '../i18n';
import * as api from '../api/tauri';
import { I18nContext } from './i18nContext';

interface I18nProviderProps {
  children: React.ReactNode;
  /** Pre-resolved locale from main.tsx boot. When provided, skip the
   *  detect → fetch-settings → load-pack flicker chain entirely. */
  initialLocale?: string;
}

export const I18nProvider: React.FC<I18nProviderProps> = ({ children, initialLocale }) => {
  const [locale, setLocaleState] = useState(() => initialLocale ?? detectLocale());
  // If main.tsx pre-loaded the pack we're already ready; otherwise English
  // is always bundled so it's instantly ready, and any non-en locale will
  // flip ready=false during its load below.
  const [ready, setReady] = useState(true);

  // Load language pack when locale changes (e.g. user switches language at runtime).
  useEffect(() => {
    if (locale === 'en') {
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setReady(true);
      return;
    }
    setReady(false);
    loadLocale(locale).then(() => setReady(true));
  }, [locale]);

  const setLocale = useCallback((newLocale: string) => {
    setLocaleState(newLocale);
    // Persist to Rust backend
    void api.saveSettings({ locale: newLocale }).catch(() => {});
  }, []);

  const t = useCallback((key: TKey) => translate(key, ready ? locale : 'en'), [locale, ready]);

  // Sync document lang attribute
  useEffect(() => {
    document.documentElement.lang = locale;
  }, [locale]);

  return React.createElement(I18nContext.Provider, { value: { locale, setLocale, t } }, children);
};

export function useI18n() {
  return useContext(I18nContext);
}
