import { create } from 'zustand';
import * as api from '../api/tauri';
import type { LocalTool } from '../api/types';
import { invalidateSystemApplicationIcons } from '../utils/systemApplicationIcons';

const MY_APPLICATIONS_KEY = 'echobird_my_system_applications';
const HIDDEN_APPLICATIONS_KEY = 'echobird_hidden_system_applications';

function readApplicationIds(key: string): string[] {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(key) ?? '[]');
    return Array.isArray(saved)
      ? [...new Set(saved.filter((id): id is string => typeof id === 'string' && id.length > 0))]
      : [];
  } catch {
    return [];
  }
}

interface SystemApplicationsState {
  applications: LocalTool[];
  myApplicationIds: string[];
  hiddenApplicationIds: string[];
  setInMyApplications: (id: string, visible: boolean) => void;
  loaded: boolean;
  loading: boolean;
  error: string | null;
  load: (forceRefresh?: boolean) => Promise<void>;
}

export const useSystemApplicationsStore = create<SystemApplicationsState>((set, get) => ({
  applications: [],
  myApplicationIds: readApplicationIds(MY_APPLICATIONS_KEY),
  hiddenApplicationIds: readApplicationIds(HIDDEN_APPLICATIONS_KEY),
  setInMyApplications: (id, visible) => {
    if (!id) return;
    const previous = get().myApplicationIds;
    const hidden = get().hiddenApplicationIds;
    if (
      visible
        ? previous.includes(id) && !hidden.includes(id)
        : !previous.includes(id) && hidden.includes(id)
    )
      return;
    const ids = visible ? [...new Set([...previous, id])] : previous.filter((item) => item !== id);
    const hiddenIds = visible
      ? hidden.filter((item) => item !== id)
      : [...new Set([...hidden, id])];
    set({ myApplicationIds: ids, hiddenApplicationIds: hiddenIds });
    try {
      localStorage.setItem(MY_APPLICATIONS_KEY, JSON.stringify(ids));
      if (hidden.includes(id) === visible)
        localStorage.setItem(HIDDEN_APPLICATIONS_KEY, JSON.stringify(hiddenIds));
    } catch {
      // Keep the current session usable when local preferences cannot be saved.
    }
  },
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
