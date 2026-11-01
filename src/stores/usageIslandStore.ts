import { create } from 'zustand';
import { useNavigationStore } from './navigationStore';

const STORAGE_KEY = 'echobird_usage_island';

export const USAGE_ISLAND_LIMIT = 6;
export const usageIslandStyles = ['row', 'column', 'dial', 'line'] as const;
export const usageIslandThemes = ['auto', 'dark', 'light', 'system'] as const;
export const usageIslandExpandModes = ['hover', 'click', 'always'] as const;
export const usageIslandRefreshMinutes = [0, 5, 15, 30, 60] as const;

export type UsageIslandStyle = (typeof usageIslandStyles)[number];
export type UsageIslandTheme = (typeof usageIslandThemes)[number];
export type UsageIslandExpandMode = (typeof usageIslandExpandModes)[number];

export interface UsageIslandConfig {
  enabled: boolean;
  // The source shown in place of the logo; the horizontal and vertical styles use it.
  pick: string | null;
  style: UsageIslandStyle;
  // Styles opening the other way: left, up or counter-clockwise.
  reversed: UsageIslandStyle[];
  theme: UsageIslandTheme;
  expandMode: UsageIslandExpandMode;
  // Zero leaves quota refresh to the explicit button.
  refreshMinutes: number;
  sources: string[];
}

interface UsageIslandState extends UsageIslandConfig {
  update: (patch: Partial<Omit<UsageIslandConfig, 'sources'>>) => void;
  toggleSource: (key: string) => void;
  toggleReversed: (style: UsageIslandStyle) => void;
  setSources: (sources: string[]) => void;
}

const defaults: UsageIslandConfig = {
  enabled: false,
  pick: null,
  style: 'row',
  reversed: [],
  theme: 'dark',
  expandMode: 'hover',
  refreshMinutes: 0,
  sources: [],
};

const oneOf = <T>(options: readonly T[], value: unknown, fallback: T) =>
  options.includes(value as T) ? (value as T) : fallback;

function readConfig(): UsageIslandConfig {
  try {
    const saved = JSON.parse(
      localStorage.getItem(STORAGE_KEY) ?? '{}'
    ) as Partial<UsageIslandConfig> | null;
    return {
      enabled: saved?.enabled === true,
      pick: typeof saved?.pick === 'string' ? saved.pick : null,
      style: oneOf(usageIslandStyles, saved?.style, defaults.style),
      reversed: usageIslandStyles.filter((style) => saved?.reversed?.includes(style)),
      theme: oneOf(usageIslandThemes, saved?.theme, defaults.theme),
      expandMode: oneOf(usageIslandExpandModes, saved?.expandMode, defaults.expandMode),
      refreshMinutes: oneOf(usageIslandRefreshMinutes, saved?.refreshMinutes, 0),
      sources: Array.isArray(saved?.sources)
        ? [...new Set(saved.sources.filter((key): key is string => typeof key === 'string'))].slice(
            0,
            USAGE_ISLAND_LIMIT
          )
        : [],
    };
  } catch {
    return defaults;
  }
}

function writeConfig(state: UsageIslandConfig) {
  const { enabled, pick, style, reversed, theme, expandMode, refreshMinutes, sources } = state;
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ enabled, pick, style, reversed, theme, expandMode, refreshMinutes, sources })
    );
  } catch {
    // Keep this session's island if local storage is unavailable.
  }
}

export const useUsageIslandStore = create<UsageIslandState>((set, get) => ({
  ...readConfig(),
  update: (patch) => {
    set(patch);
    writeConfig(get());
  },
  toggleSource: (key) => {
    const { sources } = get();
    if (sources.includes(key)) set({ sources: sources.filter((source) => source !== key) });
    else if (sources.length < USAGE_ISLAND_LIMIT) set({ sources: [...sources, key] });
    else return;
    writeConfig(get());
  },
  toggleReversed: (style) => {
    const { reversed } = get();
    set({
      reversed: reversed.includes(style)
        ? reversed.filter((entry) => entry !== style)
        : [...reversed, style],
    });
    writeConfig(get());
  },
  setSources: (sources) => {
    set({ sources });
    writeConfig(get());
  },
}));

const shouldShow = (
  island: Pick<UsageIslandConfig, 'enabled' | 'sources'>,
  developerMode: boolean
) => developerMode && island.enabled && island.sources.length > 0;

export const usageIslandVisible = () =>
  shouldShow(useUsageIslandStore.getState(), useNavigationStore.getState().developerMode);

export function useUsageIslandVisible() {
  const developerMode = useNavigationStore((state) => state.developerMode);
  return useUsageIslandStore((state) => shouldShow(state, developerMode));
}
