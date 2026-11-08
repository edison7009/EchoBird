import { create } from 'zustand';

export type BackgroundMode = 'none' | 'characters' | 'carbon' | 'basalt' | 'monogram';
const STORAGE_KEY = 'echobird_background';

function readMode(): BackgroundMode {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (
      saved === 'none' ||
      saved === 'characters' ||
      saved === 'carbon' ||
      saved === 'basalt' ||
      saved === 'monogram'
    )
      return saved;
  } catch {
    // Unavailable storage leaves the current default intact.
  }
  return 'basalt';
}

export const useBackgroundStore = create<{
  mode: BackgroundMode;
  setMode: (mode: BackgroundMode) => void;
}>((set) => ({
  mode: readMode(),
  setMode: (mode) => {
    set({ mode });
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      // Keep the selection usable even if persistence is unavailable.
    }
  },
}));
