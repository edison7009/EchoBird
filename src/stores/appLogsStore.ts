// App logs store — shared log entries for system diagnostics
// Used by: App.tsx (produces logs), MotherAgentProvider (reads + sends to AI)

import { create } from 'zustand';
import type { AppLogEntry } from '../api/types';

interface AppLogsState {
    appLogs: AppLogEntry[];
    addLog: (entry: AppLogEntry) => void;
    clearLogs: () => void;
}

export const useAppLogsStore = create<AppLogsState>((set) => ({
    appLogs: [],
    addLog: (entry) => set(state => ({ appLogs: [...state.appLogs, entry] })),
    clearLogs: () => set({ appLogs: [] }),
}));
