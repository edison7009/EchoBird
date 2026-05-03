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

const apply = (resolved: Resolved) => {
    document.documentElement.dataset.theme = resolved;
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
        apply(resolved);
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
        apply(resolved);
        set({ mode, resolved });
        mql.addEventListener('change', () => {
            if (get().mode !== 'system') return;
            const r: Resolved = mql.matches ? 'dark' : 'light';
            apply(r);
            set({ resolved: r });
        });
    },
}));
