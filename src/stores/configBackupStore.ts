import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { modelRepository } from './modelRepository';
import { useAutoImportStore } from './autoImportStore';

export type BackupMode = 'backup' | 'restore';
export interface BackupOutcome {
  records: number;
  restartRequired: boolean;
  duplicates: number;
  deviceBound: number;
  skipped: number;
  fileName: string;
  uiPreferences: Record<string, string>;
}
const uiKeys = [
  'echobird_background',
  'echobird_easter_egg',
  'echobird_auto_claim_rewards',
  'echobird-ui-scale',
];
interface BackupState {
  phase: 'idle' | 'choosing' | 'password' | 'working';
  challenge: { mode: BackupMode; path: string } | null;
  mode: BackupMode | null;
  outcome: BackupOutcome | null;
  error: string | null;
  revision: number;
  choose: (mode: BackupMode, title: string) => Promise<void>;
  cancel: () => void;
  run: (password: string) => Promise<void>;
}
export const useConfigBackupStore = create<BackupState>((set, get) => ({
  phase: 'idle',
  challenge: null,
  mode: null,
  outcome: null,
  error: null,
  revision: 0,
  choose: async (mode, title) => {
    if (get().phase !== 'idle' || useAutoImportStore.getState().running) return;
    const revision = get().revision + 1;
    set({ phase: 'choosing', mode, error: null, outcome: null, revision });
    try {
      const path = await open({
        title,
        directory: mode === 'backup',
        multiple: false,
        ...(mode === 'restore'
          ? { filters: [{ name: 'EchoBird', extensions: ['ebbackup'] }] }
          : {}),
      });
      if (get().revision !== revision) return;
      if (typeof path !== 'string') {
        set({ phase: 'idle' });
        return;
      }
      set({ challenge: { mode, path }, phase: 'password' });
      if (mode === 'restore') await get().run('');
    } catch {
      if (get().revision === revision) set({ phase: 'idle', error: 'backupError.read' });
    }
  },
  cancel: () => {
    if (get().phase === 'working') return;
    set((state) => ({ phase: 'idle', challenge: null, error: null, revision: state.revision + 1 }));
  },
  run: async (password) => {
    const { challenge, phase } = get();
    if (!challenge || phase !== 'password' || useAutoImportStore.getState().running) return;
    set({ phase: 'working', error: null, outcome: null });
    try {
      const uiPreferences: Record<string, string> = {};
      if (challenge.mode === 'backup') {
        for (const key of uiKeys) {
          const value = localStorage.getItem(key);
          if (value !== null) uiPreferences[key] = value;
        }
      }
      const result =
        challenge.mode === 'backup'
          ? await invoke<BackupOutcome>('create_config_backup', {
              directory: challenge.path,
              password,
              uiPreferences,
            })
          : await invoke<BackupOutcome>('restore_config_backup', {
              filePath: challenge.path,
              password,
            });
      const outcome = { ...result };
      if (challenge.mode === 'restore') {
        let uiFailed = false;
        let uiChanged = false;
        for (const key of uiKeys) {
          if (outcome.uiPreferences[key] !== undefined) {
            try {
              if (localStorage.getItem(key) !== outcome.uiPreferences[key]) {
                localStorage.setItem(key, outcome.uiPreferences[key]);
                uiChanged = true;
              }
            } catch {
              uiFailed = true;
            }
          }
        }
        if (Object.keys(outcome.uiPreferences).some((key) => uiKeys.includes(key))) {
          if (uiFailed) outcome.skipped += 1;
          else if (uiChanged) outcome.records += 1;
          else outcome.duplicates += 1;
        }
        outcome.restartRequired ||= uiChanged;
        modelRepository.invalidate();
        useAutoImportStore.setState((state) => ({ progress: null, revision: state.revision + 1 }));
      }
      set({ phase: 'idle', challenge: null, outcome, error: null });
    } catch (error) {
      // Keep the selected file for a password retry; never persist the password.
      set({
        phase: 'password',
        error: String(error).includes('backupError.passwordRequired') ? null : String(error),
      });
    }
  },
}));
