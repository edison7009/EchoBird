import type { ModelConfig } from '../api/types';
import { isSmartRouterId } from './smartRouter';

// This is the direct-picker policy; routed models keep their protocol adapters.
export function isDirectModelAllowedForTool(model: ModelConfig, toolId: string | null): boolean {
  if (isSmartRouterId(model.internalId) || model.internalId === 'local-server') return true;
  const id = model.modelId?.trim().toLowerCase() ?? '';
  if (toolId === 'claudecode' || toolId === 'claudedesktop') return !id.startsWith('gpt-');
  if (toolId === 'codex' || toolId === 'chatgptdesktop') return !id.startsWith('claude-');
  return true;
}
