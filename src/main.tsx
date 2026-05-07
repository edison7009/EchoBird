import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { I18nProvider } from './hooks/useI18n';
import { useThemeStore } from './stores/themeStore';
import { detectLocale, loadLocale, resolveLocale } from './i18n';
import * as api from './api/tauri';

// ── Boot pipeline ────────────────────────────────────────────────────────────
// One linear sequence runs before the Tauri window becomes visible:
//
//   theme → (i18n ‖ fonts) → <html lang> → React render → 1 paint → appReady()
//
// Each step exists because skipping it produced a visible artifact in cold
// start. The window stays hidden (Tauri visible:false) the entire time, so
// the user sees the *first* paint, not an intermediate state.

// 1. Theme — must run before any CSS reads --bg-base-rgb. Sync.
useThemeStore.getState().init();

// 2. i18n — resolve locale (saved Rust setting > navigator.language) and
// load its language pack before render. Without this the first paint is
// English then swaps to the user's locale.
async function bootI18n(): Promise<string> {
    let locale = detectLocale();
    try {
        const s = await api.getSettings();
        if (s.locale) locale = resolveLocale(s.locale);
    } catch { /* settings unreadable — keep detected */ }
    if (locale !== 'en') await loadLocale(locale);
    return locale;
}

// 3. Fonts — explicitly request every weight that appears on the first
// painted screen, then await fonts.ready. document.fonts.ready ONLY waits
// for fonts that have already been requested; @font-face declarations alone
// don't trigger a download. Before React mounts there's no text in the DOM,
// so without these explicit fonts.load() calls ready resolves immediately
// and we paint with system fallbacks → Inter (font-display: block) ghost-
// renders empty cards, Noto Sans/Lora swap mid-paint and shift baselines.
async function bootFonts(): Promise<void> {
    if (typeof document === 'undefined' || !(document as any).fonts) return;
    const fs = (document as any).fonts;
    const specs = [
        '400 15px "Inter"',         // body text
        '500 15px "Inter"',         // medium UI
        '600 15px "Inter"',         // strong UI
        '700 15px "Inter"',         // bold UI
        '700 22px "Noto Sans"',     // .cjk-title (page headers)
        '700 italic 22px "Lora"',   // .brand-mark (sidebar wordmark)
    ];
    for (const spec of specs) {
        try { fs.load(spec); } catch { /* ignore individual failures */ }
    }
    await Promise.race([
        fs.ready as Promise<unknown>,
        new Promise<void>(resolve => setTimeout(resolve, 1500)),
    ]);
}

// 4. Tell Tauri the React tree has actually painted — only then show the
// window. Two RAFs because the first fires *before* the browser paints; the
// second fires after the paint that draws our first frame. This is what
// makes the window appear with the real UI already on screen.
function showWindowAfterFirstPaint(): void {
    requestAnimationFrame(() => {
        requestAnimationFrame(() => {
            api.appReady();
        });
    });
}

(async () => {
    const [locale] = await Promise.all([bootI18n(), bootFonts()]);

    // Sync <html lang> before render so :lang(zh) CJK overrides apply on the
    // first paint instead of triggering a reflow when I18nProvider's effect
    // runs post-mount.
    document.documentElement.lang = locale;

    ReactDOM.createRoot(document.getElementById('root')!).render(
        <React.StrictMode>
            <I18nProvider initialLocale={locale}>
                <App />
            </I18nProvider>
        </React.StrictMode>
    );

    showWindowAfterFirstPaint();
})();
