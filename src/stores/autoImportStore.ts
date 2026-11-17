import { create } from 'zustand';
import { Channel, invoke } from '@tauri-apps/api/core';
import { modelRepository } from './modelRepository';

export type ImportIssueReason =
  | 'readFailed'
  | 'saveFailed'
  | 'loginRequired'
  | 'unsupported'
  | 'invalidConfiguration'
  | 'localProxy';

export interface ImportProgress {
  completed: number;
  total: number;
  source: string;
  models: number;
  accounts: number;
  duplicates: number;
  issues: { source: string; reason: ImportIssueReason; count: number }[];
}

interface ImportState {
  running: boolean;
  progress: ImportProgress | null;
  error: boolean;
  revision: number;
  start: () => Promise<void>;
}

export const useAutoImportStore = create<ImportState>((set, get) => ({
  running: false,
  progress: null,
  error: false,
  revision: 0,
  start: async () => {
    if (get().running) return;
    set({ running: true, progress: null, error: false });
    const channel = new Channel<ImportProgress>();
    let active = true;
    channel.onmessage = (progress) => {
      if (active) set({ progress });
    };
    try {
      const progress = await invoke<ImportProgress>('auto_import', { onProgress: channel });
      set({ progress });
    } catch {
      set({ error: true });
    } finally {
      active = false;
      set((state) => ({ running: false, revision: state.revision + 1 }));
      modelRepository.invalidate();
    }
  },
}));
