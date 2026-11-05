import type { AccountDirectoryContextType } from '../../features/accounts/context';
import { createContext, useContext } from 'react';
import type { ModelConfig, LocalTool } from '../../api/types';

// ===== Context =====

export interface AppManagerContextType extends AccountDirectoryContextType {
  smartRouterEnabled: boolean;
  // Internalized state
  selectedTool: string | null;
  setSelectedTool: (id: string | null) => void;
  launchAfterApply: boolean;
  setLaunchAfterApply: (v: boolean) => void;
  isLaunching: boolean;
  agreedConfigPolicy: boolean;
  setAgreedConfigPolicy: (v: boolean) => void;
  toolModelConfig: Record<string, string | null>;
  handleSelectModel: (toolId: string, modelId: string) => void;
  /** Restore the tool's config back to its official vendor endpoint */
  handleRestoreModel: (toolId: string) => Promise<void>;
  selectedToolData: LocalTool | undefined;
  applyError: string | null;
  setApplyError: (v: string | null) => void;
  // Shared props (from App.tsx)
  detectedTools: LocalTool[];
  setDetectedTools: React.Dispatch<React.SetStateAction<LocalTool[]>>;
  isScanning: boolean;
  scanTools: () => Promise<void>;
  userModels: ModelConfig[];
  codexWebSearch: boolean;
  setCodexWebSearch: (v: boolean) => void;
  claudeDesktopWebSearch: boolean;
  claudeCodeWebSearch: boolean;
  setClaudeWebSearch: (tool: 'claudedesktop' | 'claudecode', value: boolean) => void;
  zcodeWebSearch: boolean;
  zcodeImageInput: boolean;
  setZCodeCapability: (capability: 'webSearch' | 'imageInput', value: boolean) => void;
  workBuddyImageInput: boolean;
  setWorkBuddyImageInput: (value: boolean) => void;
  workBuddy1mMode: boolean;
  setWorkBuddy1mMode: (value: boolean) => void;
  mimoImageInput: boolean;
  mimo1mMode: boolean;
  setMimoCapability: (capability: 'imageInput' | 'oneMContext', value: boolean) => void;
  minimaxImageInput: boolean;
  minimax1mMode: boolean;
  setMinimaxCapability: (capability: 'imageInput' | 'oneMContext', value: boolean) => void;
  qoderImageInput: boolean;
  qoder1mMode: boolean;
  setQoderCapability: (capability: 'imageInput' | 'oneMContext', value: boolean) => void;
  dshImageInput: boolean;
  dsh1mMode: boolean;
  setDshCapability: (capability: 'imageInput' | 'oneMContext', value: boolean) => void;
  /** Desktop profile prefer1m. */
  claudeDesktop1mMode: boolean;
  setClaudeDesktop1mMode: (v: boolean) => void;
  /** Claude Code 1M-context toggle. When on,
   *  apply_claudecode appends `[1m]` to the model id (MODEL / OPUS / SONNET / FABLE
   *  env vars only — HAIKU + SUBAGENT stay bare) so Claude Code budgets the
   *  1M window. CC strips the suffix before sending upstream, so the provider
   *  still sees the bare id. */
  claude1mMode: boolean;
  setClaude1mMode: (v: boolean) => void;
  // Launch handler
  handleLaunch: () => Promise<void>;
  // Navigation — internal handler: (toolId, toolName) => fetch install info → call prop
  onGoToMother: (toolId: string, toolName: string) => void;
  // AI-installable tool IDs (from bundled tools/install/index.json)
  aiInstallableIds: string[];
  viewMode: 'desktop' | 'install';
  setViewMode: (mode: 'desktop' | 'install') => void;
}

export const AppManagerContext = createContext<AppManagerContextType | null>(null);

export const useAppManager = () => {
  const ctx = useContext(AppManagerContext);
  if (!ctx) throw new Error('useAppManager must be used within AppManagerProvider');
  return ctx;
};
