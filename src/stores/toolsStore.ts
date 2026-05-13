// Tools store — shared state for detected tools and scanning
// Used by: App.tsx (init), AppManagerProvider, MotherAgentProvider

import { create } from 'zustand';
import * as api from '../api/tauri';
import type { LocalTool } from '../api/types';

interface ToolsState {
  detectedTools: LocalTool[];
  isScanning: boolean;
  modelProtocolSelection: Record<string, 'openai' | 'anthropic'>;

  setDetectedTools: (tools: LocalTool[] | ((prev: LocalTool[]) => LocalTool[])) => void;
  setModelProtocolSelection: (
    sel:
      | Record<string, 'openai' | 'anthropic'>
      | ((prev: Record<string, 'openai' | 'anthropic'>) => Record<string, 'openai' | 'anthropic'>)
  ) => void;
  scanTools: () => Promise<void>;
}

export const useToolsStore = create<ToolsState>((set, _get) => ({
  detectedTools: [],
  isScanning: false,
  modelProtocolSelection: {},

  setDetectedTools: (tools) =>
    set((state) => ({
      detectedTools: typeof tools === 'function' ? tools(state.detectedTools) : tools,
    })),

  setModelProtocolSelection: (sel) =>
    set((state) => ({
      modelProtocolSelection: typeof sel === 'function' ? sel(state.modelProtocolSelection) : sel,
    })),

  scanTools: async () => {
    set({ isScanning: true });
    try {
      const tools = await api.scanTools();
      set({ detectedTools: tools });
    } catch {
      /* ignore */
    }
    set({ isScanning: false });
  },
}));
