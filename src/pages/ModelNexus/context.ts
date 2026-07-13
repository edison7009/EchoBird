// context.ts — ModelNexus shared context + types
import React, { createContext, useContext } from 'react';
import type { ModelConfig } from '../../api/types';
import type { ModelUsageData } from '../../api/tauri';

// ===== Types =====

export interface NewModelForm {
  name: string;
  baseUrl: string;
  anthropicUrl: string;
  apiKey: string;
  modelId: string;
  // Quick-pick model id options, carried over from a right-panel directory
  // entry click. ≥2 options → the Add-Model modal shows a dropdown above the
  // model id input. Absent/empty → no dropdown (manual add / edit). Purely a
  // convenience picker; the model id input is always free-editable.
  modelIdOptions?: string[];
}

export interface ModelNexusCtx {
  // Models
  userModels: ModelConfig[];
  setUserModels: React.Dispatch<React.SetStateAction<ModelConfig[]>>;
  isLoadingModels: boolean;
  selectedModel: string | null;
  setSelectedModel: (id: string | null) => void;
  selectedModelData: ModelConfig | undefined;
  // View mode
  viewMode: 'config' | 'usage';
  setViewMode: (mode: 'config' | 'usage') => void;
  // Usage data
  modelUsageData: Record<string, ModelUsageData>;
  setModelUsageData: React.Dispatch<React.SetStateAction<Record<string, ModelUsageData>>>;
  isRefreshingUsage: boolean;
  // Volcengine SSO
  volcSsoValid: boolean;
  setVolcSsoValid: React.Dispatch<React.SetStateAction<boolean>>;
  volcSsoLogin: () => Promise<void>;
  // Test
  testInput: string;
  setTestInput: (v: string) => void;
  testOutput: string[];
  setTestOutput: React.Dispatch<React.SetStateAction<string[]>>;
  isTesting: boolean;
  arrowIndex: number;
  testProtocol: 'openai' | 'anthropic';
  setTestProtocol: (v: 'openai' | 'anthropic') => void;
  modelLatencies: Record<string, number>;
  pingingModelIds: Set<string>;
  modelTerminals: Record<string, { input: string; output: string[] }>;
  setModelTerminals: React.Dispatch<
    React.SetStateAction<Record<string, { input: string; output: string[] }>>
  >;
  testInputRef: React.RefObject<HTMLInputElement>;
  inputFocused: boolean;
  setInputFocused: (v: boolean) => void;
  cursorPos: number;
  setCursorPos: (v: number) => void;
  // Modal
  showAddModelModal: boolean;
  setShowAddModelModal: (v: boolean) => void;
  modelModalAnimatingOut: boolean;
  editingModelId: string | null;
  setEditingModelId: (v: string | null) => void;
  newModelForm: NewModelForm;
  setNewModelForm: React.Dispatch<React.SetStateAction<NewModelForm>>;
  showApiKey: boolean;
  setShowApiKey: (v: boolean) => void;
  keyDestroyed: boolean;
  setKeyDestroyed: (v: boolean) => void;
  closeModelModal: () => void;
  // Actions
  pingAllModels: () => Promise<void>;
  refreshAllUsage: () => Promise<void>;
  refreshSingleUsage: (modelId: string) => Promise<void>; // Single model refresh
  handleTestModel: () => Promise<void>;
}

// ===== Context =====

export const ModelNexusContext = createContext<ModelNexusCtx | null>(null);

export const useModelNexus = () => {
  const ctx = useContext(ModelNexusContext);
  if (!ctx) throw new Error('useModelNexus must be used within ModelNexusProvider');
  return ctx;
};
