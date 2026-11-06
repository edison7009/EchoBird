// Theme store — light / dark / system, persisted to AppSettings.themeMode.
// Missing settings file defaults to dark; an unset saved themeMode means follow system.
import { create } from 'zustand';
import { emit } from '@tauri-apps/api/event';
import * as api from '../api/tauri';
import type { PluginTheme } from '../api/plugins';
import {
  getColorTheme,
  isColorThemeId,
  type ColorThemeId,
  type ColorThemeVariant,
} from '../data/colorThemes';

export type ThemeMode = 'system' | 'light' | 'dark';
export const THEME_CHANGED_EVENT = 'echobird-theme-changed';
export const THEME_REQUEST_EVENT = 'echobird-theme-request';
export interface ThemeSelection {
  mode: ThemeMode;
  colorTheme: ColorThemeId;
  pluginTheme?: PluginTheme | null;
}
let pluginTheme: PluginTheme | null = null;
const pluginColorKeys: Record<string, string> = {
  background: '--bg-base-rgb',
  surface: '--bg-surface-rgb',
  elevated: '--bg-elevated-rgb',
  input: '--bg-input-rgb',
  border: '--border-rgb',
  text: '--text-primary-rgb',
  textSecondary: '--text-secondary-rgb',
  textMuted: '--text-muted-rgb',
  accent: '--accent-rgb',
  accentSecondary: '--accent-secondary-rgb',
};
const appliedPluginColors = new Set<string>();
type Resolved = 'light' | 'dark';

const mql = window.matchMedia('(prefers-color-scheme: dark)');

const resolve = (mode: ThemeMode): Resolved => {
  if (mode === 'system') return mql.matches ? 'dark' : 'light';
  return mode;
};

const hexToRgbChannels = (hex: string): string => {
  const value = Number.parseInt(hex.slice(1), 16);
  return `${(value >> 16) & 255} ${(value >> 8) & 255} ${value & 255}`;
};

const applyColorTheme = (id: ColorThemeId, resolved: Resolved) => {
  const theme = getColorTheme(id);
  const variant: ColorThemeVariant = resolved === 'dark' ? theme.dark : theme.light;
  const root = document.documentElement;
  for (const key of appliedPluginColors) root.style.removeProperty(key);
  appliedPluginColors.clear();
  root.style.setProperty('--bg-base-rgb', hexToRgbChannels(variant.canvas));
  root.style.setProperty('--bg-terminal-rgb', hexToRgbChannels(variant.shell));
  root.style.setProperty('--bg-surface-rgb', hexToRgbChannels(variant.secondary));
  root.style.setProperty('--bg-elevated-rgb', hexToRgbChannels(variant.tertiary));
  root.style.setProperty('--bg-deep-rgb', hexToRgbChannels(variant.shell));
  root.style.setProperty('--bg-input-rgb', hexToRgbChannels(variant.input));
  // Tips always use this palette's dark surface, even in light mode.
  const pluginTooltip = pluginTheme?.dark.surface ?? pluginTheme?.dark.background;
  root.style.setProperty(
    '--tooltip-bg-rgb',
    hexToRgbChannels(
      pluginTooltip && /^#[0-9a-f]{6}$/i.test(pluginTooltip) ? pluginTooltip : theme.dark.secondary
    )
  );
  const tabColor = hexToRgbChannels(variant.tertiary);
  const channels = tabColor.split(' ').map(Number);
  // Match the character background's neutral-palette fallback without changing primary buttons.
  const neutral = Math.max(...channels) - Math.min(...channels) < 18;
  root.style.setProperty('--view-mode-accent-rgb', neutral ? 'var(--accent-rgb)' : tabColor);
  // Max-effort text needs a vivid foreground, not a pastel surface mixed with white.
  const low = Math.min(...channels);
  const range = Math.max(...channels) - low;
  const emphasis = channels
    .map((channel) => Math.round(90 + (127 * (channel - low)) / (range || 1)))
    .join(' ');
  const useAccent = neutral || pluginTheme !== null;
  root.style.setProperty('--reasoning-accent-rgb', useAccent ? 'var(--accent-rgb)' : emphasis);
  root.style.setProperty(
    '--reasoning-accent-on-light',
    useAccent
      ? 'rgb(var(--accent-rgb))'
      : 'color-mix(in srgb, rgb(var(--reasoning-accent-rgb)) 45%, rgb(var(--text-primary-rgb)))'
  );
  // Light chromatic fills are pastel; keep their labels on the theme's dark text.
  root.style.setProperty(
    '--view-mode-label-rgb',
    neutral ? '255 255 255' : 'var(--text-primary-rgb)'
  );
  root.style.setProperty('--view-mode-wave-rgb', 'var(--view-mode-accent-rgb)');
  if (pluginTheme) {
    for (const [key, value] of Object.entries(pluginTheme[resolved])) {
      const property = pluginColorKeys[key];
      if (property && /^#[0-9a-f]{6}$/i.test(value)) {
        root.style.setProperty(property, hexToRgbChannels(value));
        appliedPluginColors.add(property);
      }
    }
  }
  try {
    localStorage.setItem('color-theme', id);
  } catch {
    /* private mode */
  }
};

// Apply the resolved theme to <html data-theme> and persist a sync-readable
// copy to localStorage so index.html's inline pre-script can pick it up on
// next launch (avoids a wrong-theme flash before getSettings() resolves).
//
// `instant` swaps the theme without animating any color/background/border
// transitions — every element changes in the same frame, so theme switching
// looks like a clean swap rather than a janky stagger of `transition-colors`
// elements interpolating at different speeds.
const apply = (resolved: Resolved, colorTheme: ColorThemeId, instant: boolean) => {
  if (instant) {
    const kill = document.createElement('style');
    // Preserve the shared flowing backgrounds while theme colors swap instantly.
    kill.textContent =
      '*:not(.voice-island.voice-theme-auto, .usage-island.usage-island-theme-auto), *:not(.view-mode-tabs[data-sliding="true"])::before, *::after { transition: none !important; animation: none !important; }';
    document.head.appendChild(kill);
    document.documentElement.dataset.theme = resolved;
    applyColorTheme(colorTheme, resolved);
    // Force a reflow so the no-transition rule is in effect for this frame.
    void document.body?.offsetHeight;
    // Re-enable transitions on the next frame so future hover/state
    // changes still animate normally.
    requestAnimationFrame(() => requestAnimationFrame(() => kill.remove()));
  } else {
    document.documentElement.dataset.theme = resolved;
    applyColorTheme(colorTheme, resolved);
  }
  try {
    localStorage.setItem('theme', resolved);
  } catch {
    /* private mode */
  }
};

interface ThemeStore {
  mode: ThemeMode;
  resolved: Resolved;
  colorTheme: ColorThemeId;
  setMode: (mode: ThemeMode) => void;
  setColorTheme: (colorTheme: ColorThemeId) => void;
  sync: (selection: ThemeSelection) => void;
  init: () => Promise<void>;
  setPluginTheme: (theme: PluginTheme | null) => void;
}

export const useThemeStore = create<ThemeStore>((set, get) => ({
  mode: 'dark',
  resolved: 'dark',
  colorTheme: 'oatgray',
  setPluginTheme: (theme) => {
    if (JSON.stringify(pluginTheme) === JSON.stringify(theme)) return;
    pluginTheme = theme;
    apply(get().resolved, get().colorTheme, false);
    void emit(THEME_CHANGED_EVENT, {
      mode: get().mode,
      colorTheme: get().colorTheme,
      pluginTheme,
    }).catch(() => {});
  },
  setMode: (mode) => {
    const resolved = resolve(mode);
    apply(resolved, get().colorTheme, true);
    set({ mode, resolved });
    void emit(THEME_CHANGED_EVENT, {
      mode,
      colorTheme: get().colorTheme,
      ...(pluginTheme ? { pluginTheme } : {}),
    }).catch(() => {});
    void api
      .saveSettings({
        themeMode: get().mode === 'system' ? null : (get().mode as 'light' | 'dark'),
        colorTheme: get().colorTheme,
      })
      .catch(() => {});
  },
  setColorTheme: (colorTheme) => {
    const hadPluginTheme = pluginTheme !== null;
    pluginTheme = null;
    apply(get().resolved, colorTheme, true);
    set({ colorTheme });
    void emit(THEME_CHANGED_EVENT, {
      mode: get().mode,
      colorTheme,
      ...(hadPluginTheme ? { pluginTheme: null } : {}),
    }).catch(() => {});
    void api
      .saveSettings({
        themeMode: get().mode === 'system' ? null : (get().mode as 'light' | 'dark'),
        colorTheme: get().colorTheme,
      })
      .catch(() => {});
  },
  sync: ({ mode, colorTheme, pluginTheme: incoming }) => {
    const paletteChanged =
      incoming !== undefined && JSON.stringify(pluginTheme) !== JSON.stringify(incoming);
    if (incoming !== undefined) pluginTheme = incoming;
    if (get().mode === mode && get().colorTheme === colorTheme && !paletteChanged) return;
    const resolved = resolve(mode);
    apply(resolved, colorTheme, true);
    set({ mode, resolved, colorTheme });
  },
  init: async () => {
    let mode: ThemeMode = 'system';
    let colorTheme: ColorThemeId = 'oatgray';
    // Restore the cached palette before the first await. index.html already
    // resolves the light/dark half synchronously, so this prevents the first
    // painted frame from briefly using EchoBird's default background colors
    // while the Rust settings file is still loading.
    try {
      const cachedColorTheme = localStorage.getItem('color-theme');
      if (isColorThemeId(cachedColorTheme)) colorTheme = cachedColorTheme;
    } catch {
      /* private mode */
    }
    const cachedResolved: Resolved =
      document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
    applyColorTheme(colorTheme, cachedResolved);
    try {
      const s = await api.getSettings();
      if (s.themeMode === 'light' || s.themeMode === 'dark') mode = s.themeMode;
      if (isColorThemeId(s.colorTheme)) colorTheme = s.colorTheme;
    } catch {
      /* default to system */
    }
    const resolved = resolve(mode);
    // No `instant: true` — initial paint has nothing to transition from.
    apply(resolved, colorTheme, false);
    set({ mode, resolved, colorTheme });
    mql.addEventListener('change', () => {
      if (get().mode !== 'system') return;
      const r: Resolved = mql.matches ? 'dark' : 'light';
      apply(r, get().colorTheme, true);
      set({ resolved: r });
    });
  },
}));
