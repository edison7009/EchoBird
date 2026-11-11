import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import './index.css';
import { I18nProvider } from './hooks/useI18n';
import { useThemeStore } from './stores/themeStore';
import { initializeUiScale } from './stores/uiScaleStore';
import { detectLocale, loadLocale, resolveLocale } from './i18n';
import * as api from './api/tauri';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { IS_LINUX, IS_MACOS } from './utils/platform';
import { ErrorBoundary } from './components/ErrorBoundary';
import { ERROR_MESSAGES, pickErrorLang } from './components/errorMessages';
import { initializeWindowVisibility } from './utils/windowVisibility';
import { VoiceIsland } from './pages/VoiceInput/VoiceIsland';
import { SelectionTranslation } from './pages/VoiceInput/SelectionTranslation';
import { handleDesktopContextMenu } from './utils/desktopInteraction';
import { preloadDesktopToolIcons } from './utils/toolIcons';
import { useToolsStore } from './stores/toolsStore';

document.addEventListener('contextmenu', handleDesktopContextMenu);
const isVoiceIsland = getCurrentWindow().label === 'voice-island';
const isVoiceTranslation = getCurrentWindow().label === 'voice-translation';
const isUsageIsland = getCurrentWindow().label === 'usage-island';
const isUsageIslandTooltip = getCurrentWindow().label === 'usage-island-tooltip';
// Start cached desktop icons alongside fonts/theme, without delaying window
// visibility or loading desktop artwork in auxiliary windows.
if (!isVoiceIsland && !isVoiceTranslation && !isUsageIsland && !isUsageIslandTooltip) {
  void preloadDesktopToolIcons(
    useToolsStore.getState().detectedTools.filter((tool) => tool.installed)
  );
}

// ── Object.hasOwn polyfill ───────────────────────────────────────────────────
// ES2022 runtime API; native in Safari 15.4+ (macOS 12.3+), absent in Safari
// ≤15.3 (macOS ≤12.2). react-markdown@10 calls it unconditionally on every
// render (deprecated-option check at lib/index.js:323), so without this Intel
// Macs pinned to Monterey ≤12.2 hard-crash ("Object.hasOwn is not a function")
// on any screen that renders markdown. Runs at module-eval time, before the
// boot pipeline and React mount; no-op where the native impl exists. tsconfig
// lib is ES2020 (Object.hasOwn lands in ES2022), hence the cast rather than a
// `declare global` augmentation.
type ObjectWithHasOwn = typeof Object & {
  hasOwn?: (o: object, v: PropertyKey) => boolean;
};
const Obj = Object as unknown as ObjectWithHasOwn;
if (typeof Obj.hasOwn !== 'function') {
  Obj.hasOwn = (obj, prop) => Object.prototype.hasOwnProperty.call(obj, prop);
}

// ── Boot pipeline ────────────────────────────────────────────────────────────
// One linear sequence runs before the Tauri window becomes visible:
//
//   theme → (i18n ‖ fonts ‖ UI scale) → <html lang> → React render → 1 paint → appReady()
//
// Each step exists because skipping it produced a visible artifact in cold
// start. The window stays hidden (Tauri visible:false) the entire time, so
// the user sees the *first* paint, not an intermediate state.

// 0. Platform tag — set before the first paint so the macOS branch (drop the
// borderless rounded shell; native decorations own the window corners) applies
// without a flash. Windows/Linux keep the custom frameless chrome unchanged.
if (IS_MACOS) document.documentElement.classList.add('is-macos');
if (!IS_MACOS && !IS_LINUX) document.documentElement.classList.add('is-windows');

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
  } catch {
    /* settings unreadable — keep detected */
  }
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
  if (typeof document === 'undefined' || !document.fonts) return;
  const fs = document.fonts;
  const specs = [
    '400 15px "Inter"', // body text
    '500 15px "Inter"', // medium UI
    '600 15px "Inter"', // strong UI
    '700 15px "Inter"', // bold UI
    '700 22px "Noto Sans"', // .cjk-title (page headers)
    '700 italic 22px "Lora"', // .brand-mark (sidebar wordmark)
    '600 italic 11px "Lora"', // .page-kicker (editorial page kicker)
  ];
  for (const spec of specs) {
    try {
      fs.load(spec);
    } catch {
      /* ignore individual failures */
    }
  }
  await Promise.race([
    fs.ready as Promise<unknown>,
    new Promise<void>((resolve) => setTimeout(resolve, 1500)),
  ]);
}

// 4. Sidebar bird logo — small but visible top-left. The PNG is preloaded via
// <link rel="preload"> in index.html so the network fetch starts at HTML-parse
// time, but we still await img.decode() here to guarantee the bitmap is
// decoded BEFORE the first paint. Without this, on a cold cache the bird pops
// in visibly later than text + icons because img decode is async.
async function bootBrandImage(): Promise<void> {
  try {
    const img = new Image();
    img.src = '/brand/bird.png';
    // decode() resolves once the image is fully ready to paint without further work.
    // Race against a 1s ceiling so we never block window-show on a flaky network.
    await Promise.race([img.decode(), new Promise<void>((resolve) => setTimeout(resolve, 1000))]);
  } catch {
    /* decode rejects on broken image — fall through, browser will retry on render */
  }
}

// 5. Tell Tauri the React tree has actually painted — only then show the
// window. Two RAFs because the first fires *before* the browser paints; the
// second fires after the paint that draws our first frame. This is what
// makes the window appear with the real UI already on screen.
function showWindowAfterFirstPaint(): void {
  if (isVoiceIsland || isVoiceTranslation || isUsageIsland) return;
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      api.appReady();
    });
  });
}

// Pre-React crash floor. If the boot pipeline throws BEFORE React mounts (the
// ErrorBoundary only catches errors during render), inject a minimal,
// self-contained error screen straight into #root — built with DOM APIs (no
// inline handlers, CSP-safe; no theme/i18n deps) so the user sees the failure
// instead of the blank window the 1s safety-show fallback would otherwise reveal.
function renderBootError(err: unknown): void {
  const root = document.getElementById('root');
  if (!root) return;
  const t = ERROR_MESSAGES[pickErrorLang()];
  const msg =
    err instanceof Error ? `${err.name}: ${err.message}\n\n${err.stack ?? ''}` : String(err);
  root.textContent = '';
  const wrap = document.createElement('div');
  wrap.setAttribute(
    'style',
    'position:fixed;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:16px;padding:24px;background:#1a1a1a;color:#e8e8e8;font-family:system-ui,-apple-system,sans-serif;text-align:center;'
  );
  const title = document.createElement('div');
  title.setAttribute('style', 'font-size:18px;font-weight:600;');
  title.textContent = t.bootTitle;
  const pre = document.createElement('pre');
  pre.setAttribute(
    'style',
    'max-width:640px;max-height:220px;overflow:auto;text-align:left;font-size:12px;line-height:1.5;padding:12px;border-radius:8px;background:#0f0f0f;color:#ff9b9b;white-space:pre-wrap;word-break:break-word;margin:0;'
  );
  pre.textContent = msg;
  const reload = document.createElement('button');
  reload.setAttribute(
    'style',
    'padding:8px 16px;border-radius:8px;border:1px solid #444;background:#2a2a2a;color:#e8e8e8;cursor:default;font-size:13px;'
  );
  reload.textContent = t.reload;
  reload.addEventListener('click', () => window.location.reload());
  wrap.append(title, pre, reload);
  root.append(wrap);
}

async function bootAndRender(): Promise<void> {
  if (isUsageIslandTooltip) {
    document.documentElement.classList.add('usage-island-tooltip-window');
    const [, { UsageIslandTooltipWindow }] = await Promise.all([
      bootFonts(),
      import('./pages/UsageIsland/UsageIslandTooltipWindow'),
    ]);
    ReactDOM.createRoot(document.getElementById('root')!).render(<UsageIslandTooltipWindow />);
    return;
  }
  if (isUsageIsland) {
    document.documentElement.classList.add('usage-island-window');
    const [locale, , { UsageIslandWindow }] = await Promise.all([
      bootI18n(),
      bootFonts(),
      import('./pages/UsageIsland/UsageIslandWindow'),
    ]);
    document.documentElement.lang = locale;
    ReactDOM.createRoot(document.getElementById('root')!).render(
      <I18nProvider initialLocale={locale}>
        <UsageIslandWindow />
      </I18nProvider>
    );
    return;
  }
  if (isVoiceIsland || isVoiceTranslation) {
    document.documentElement.classList.add(
      isVoiceTranslation ? 'voice-translation-window' : 'voice-island-window'
    );
    // The island's zoom is its own setting, applied natively by the backend.
    const [locale, , disposeUiScale] = await Promise.all([
      bootI18n(),
      bootFonts(),
      isVoiceTranslation ? initializeUiScale() : undefined,
    ]);
    if (disposeUiScale) import.meta.hot?.dispose(disposeUiScale);
    document.documentElement.lang = locale;
    ReactDOM.createRoot(document.getElementById('root')!).render(
      <I18nProvider initialLocale={locale}>
        {isVoiceTranslation ? <SelectionTranslation /> : <VoiceIsland />}
      </I18nProvider>
    );
    return;
  }
  const disposeWindowVisibility = initializeWindowVisibility();
  import.meta.hot?.dispose(disposeWindowVisibility);
  const [locale, , , disposeUiScale] = await Promise.all([
    bootI18n(),
    bootFonts(),
    bootBrandImage(),
    initializeUiScale(),
  ]);
  import.meta.hot?.dispose(disposeUiScale);

  // Sync <html lang> before render so :lang(zh) CJK overrides apply on the
  // first paint instead of triggering a reflow when I18nProvider's effect
  // runs post-mount.
  document.documentElement.lang = locale;

  ReactDOM.createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <ErrorBoundary>
        <I18nProvider initialLocale={locale}>
          <App />
        </I18nProvider>
      </ErrorBoundary>
    </React.StrictMode>
  );
}

(async () => {
  try {
    await bootAndRender();
  } catch (err) {
    console.error('[main.tsx] boot failed before render:', err);
    renderBootError(err);
  } finally {
    // Always reveal the window — on success it shows the app; on failure, the
    // error screen above, instead of the blank 1s safety-show fallback window.
    showWindowAfterFirstPaint();
  }
})();
