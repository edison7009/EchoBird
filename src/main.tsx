import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { I18nProvider } from './hooks/useI18n';
import { useThemeStore } from './stores/themeStore';
import { detectLocale, loadLocale, resolveLocale } from './i18n';
import * as api from './api/tauri';

// Resolve and apply the theme before first paint to avoid a flash.
useThemeStore.getState().init();

// Pre-resolve locale (incl. saved Rust settings) and locale pack BEFORE first
// React render — otherwise the cold-start sequence is en → flash → real locale.
async function bootI18n(): Promise<string> {
    let locale = detectLocale();
    try {
        const s = await api.getSettings();
        if (s.locale) locale = resolveLocale(s.locale);
    } catch { /* settings unreadable — keep detected */ }
    if (locale !== 'en') await loadLocale(locale);
    return locale;
}

// Wait for @font-face files to finish loading so the first paint already uses
// the real font (no FOUT swap). Capped so a stalled font can't block forever.
async function bootFonts(): Promise<void> {
    if (typeof document === 'undefined' || !(document as any).fonts) return;
    await Promise.race([
        (document as any).fonts.ready as Promise<unknown>,
        new Promise<void>(resolve => setTimeout(resolve, 1500)),
    ]);
}

(async () => {
    const [locale] = await Promise.all([bootI18n(), bootFonts()]);
    ReactDOM.createRoot(document.getElementById('root')!).render(
        <React.StrictMode>
            <I18nProvider initialLocale={locale}>
                <App />
            </I18nProvider>
        </React.StrictMode>
    );
})();
