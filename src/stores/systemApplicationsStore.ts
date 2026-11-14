import { create } from 'zustand';
import * as api from '../api/tauri';
import type { LocalTool } from '../api/types';
import { invalidateSystemApplicationIcons } from '../utils/systemApplicationIcons';

interface SystemApplicationsState {
  applications: LocalTool[];
  loaded: boolean;
  loading: boolean;
  error: string | null;
  load: (forceRefresh?: boolean) => Promise<void>;
}

export const useSystemApplicationsStore = create<SystemApplicationsState>((set, get) => ({
  applications: [],
  loaded: false,
  loading: false,
  error: null,
  load: async (forceRefresh = false) => {
    if (get().loading || (get().loaded && !forceRefresh)) return;
    set({ loading: true, error: null });
    try {
      const applications = await api.listSystemApplications(forceRefresh);
      if (forceRefresh) await invalidateSystemApplicationIcons();
      const tools: LocalTool[] = applications.map((app) => ({
        id: app.id,
        systemAppId: app.id,
        name: app.name,
        category: 'Desktop',
        installed: true,
        detectedPath: app.path,
        createdAt: app.createdAt,
        noModelConfig: true,
        noCwdPrompt: true,
      }));
      set({ applications: tools, loaded: true });
    } catch (error) {
      set({ error: String(error) });
    } finally {
      set({ loading: false });
    }
  },
}));
