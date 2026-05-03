// Lightweight i18n React Hook + Context (lazy-loaded language packs)
import React, { createContext, useContext, useState, useCallback, useEffect } from 'react';
import { translate, loadLocale, detectLocale, TKey } from '../i18n';
import * as api from '../api/tauri';

interface I18nContextValue {
    locale: string;
    setLocale: (locale: string) => void;
    t: (key: TKey) => string;
}

const I18nContext = createContext<I18nContextValue>({
    locale: 'en',
    setLocale: () => { },
    t: (key) => key,
});

export const I18nProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [locale, setLocaleState] = useState(() => detectLocale());
    const [ready, setReady] = useState(locale === 'en');

    // Load saved locale from Rust backend on mount
    useEffect(() => {
        api.getSettings().then(settings => {
            if (settings.locale) {
                setLocaleState(settings.locale);
            }
        }).catch(() => { });
    }, []);

    // Load language pack when locale changes
    useEffect(() => {
        if (locale === 'en') { setReady(true); return; }
        setReady(false);
        loadLocale(locale).then(() => setReady(true));
    }, [locale]);

    const setLocale = useCallback((newLocale: string) => {
        setLocaleState(newLocale);
        // Persist to Rust backend
        api.getSettings().then(settings => {
            api.saveSettings({ ...settings, locale: newLocale }).catch(() => { });
        }).catch(() => {
            api.saveSettings({ locale: newLocale }).catch(() => { });
        });
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
