import { createContext, useContext } from 'react';
import type { ModelConfig, LocalTool } from '../../api/types';

// ===== Context =====

export interface AppManagerContextType {
  // Internalized state
  selectedTool: string | null;
  setSelectedTool: (id: string | null) => void;
  activeToolCategory: string;
  setActiveToolCategory: (cat: string) => void;
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
  modelProtocolSelection: Record<string, 'openai' | 'anthropic'>;
  setModelProtocolSelection: React.Dispatch<
    React.SetStateAction<Record<string, 'openai' | 'anthropic'>>
  >;
  /** Codex routing toggle (shared between Codex CLI and Codex Desktop —
   *  they share ~/.codex/config.toml so a single flag is correct). */
  codexRelayMode: boolean;
  setCodexRelayMode: (v: boolean) => void;
  /** Claude Desktop routing toggle. Kept separate from Codex because the
   *  two apps target different protocols / different relay-station compat. */
  claudeDesktopRelayMode: boolean;
  setClaudeDesktopRelayMode: (v: boolean) => void;
  // Launch handler
  handleLaunch: () => Promise<void>;
  // Navigation — internal handler: (toolId, toolName) => fetch install info → call prop
  onGoToMother: (toolId: string, toolName: string) => void;
  // AI-installable tool IDs (from bundled tools/install/index.json)
  aiInstallableIds: string[];
}

export const AppManagerContext = createContext<AppManagerContextType | null>(null);

export const useAppManager = () => {
  const ctx = useContext(AppManagerContext);
  if (!ctx) throw new Error('useAppManager must be used within AppManagerProvider');
  return ctx;
};

// Tool categories
export const toolCategories = [
  'ALL',
  'Desktop',
  'Agents',
  'IDE',
  'CLI Code',
  'AutoTrading',
  'Game',
  'Utility',
] as const;
