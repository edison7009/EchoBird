// Theme store — light / dark / system, persisted to AppSettings.themeMode.
// undefined themeMode === follow system.
import { create } from 'zustand';
import * as api from '../api/tauri';

export type ThemeMode = 'system' | 'light' | 'dark';
type Resolved = 'light' | 'dark';

const mql = window.matchMedia('(prefers-color-scheme: dark)');

const resolve = (mode: ThemeMode): Resolved => {
    if (mode === 'system') return mql.matches ? 'dark' : 'light';
    return mode;
};

// Apply the resolved theme to <html data-theme> and persist a sync-readable
// copy to localStorage so index.html's inline pre-script can pick it up on
// next launch (avoids a wrong-theme flash before getSettings() resolves).
//
// `instant` swaps the theme without animating any color/background/border
// transitions — every element changes in the same frame, so theme switching
// looks like a clean swap rather than a janky stagger of `transition-colors`
// elements interpolating at different speeds.
const apply = (resolved: Resolved, instant: boolean) => {
    if (instant) {
        const kill = document.createElement('style');
        kill.textContent = '*, *::before, *::after { transition: none !important; animation: none !important; }';
        document.head.appendChild(kill);
        document.documentElement.dataset.theme = resolved;
        // Force a reflow so the no-transition rule is in effect for this frame.
        void document.body?.offsetHeight;
        // Re-enable transitions on the next frame so future hover/state
        // changes still animate normally.
        requestAnimationFrame(() => requestAnimationFrame(() => kill.remove()));
    } else {
        document.documentElement.dataset.theme = resolved;
    }
    try { localStorage.setItem('theme', resolved); } catch { /* private mode */ }
};

interface ThemeStore {
    mode: ThemeMode;
    resolved: Resolved;
    setMode: (mode: ThemeMode) => void;
    init: () => Promise<void>;
}

export const useThemeStore = create<ThemeStore>((set, get) => ({
    mode: 'system',
    resolved: resolve('system'),
    setMode: (mode) => {
        const resolved = resolve(mode);
        apply(resolved, true);
        set({ mode, resolved });
        api.getSettings()
            .then(s => api.saveSettings({ ...s, themeMode: mode === 'system' ? undefined : mode }))
            .catch(() => { });
    },
    init: async () => {
        let mode: ThemeMode = 'system';
        try {
            const s = await api.getSettings();
            if (s.themeMode === 'light' || s.themeMode === 'dark') mode = s.themeMode;
        } catch { /* default to system */ }
        const resolved = resolve(mode);
        // No `instant: true` — initial paint has nothing to transition from.
        apply(resolved, false);
        set({ mode, resolved });
        mql.addEventListener('change', () => {
            if (get().mode !== 'system') return;
            const r: Resolved = mql.matches ? 'dark' : 'light';
            apply(r, true);
            set({ resolved: r });
        });
    },
}));
