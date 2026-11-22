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

const RESPONSES_TOOLS = new Set([
  'codex',
  'chatgptdesktop',
  'cline',
  'clinedesktop',
  'vscode',
  'openclaw',
  'hermes',
  'dsh',
  'minimaxcode',
  'minimaxdesktop',
]);

export function directModelProtocol(
  model: ModelConfig,
  toolId: string | null,
  toolProtocols: readonly string[]
): 'openai' | 'openai-responses' | 'anthropic' | undefined {
  if (!isDirectModelAllowedForTool(model, toolId)) return undefined;
  if (model.internalId === 'local-server' && (toolId === 'codex' || toolId === 'chatgptdesktop')) {
    return undefined;
  }
  for (const protocol of toolProtocols) {
    if (protocol === 'anthropic' && model.anthropicUrl) return 'anthropic';
    if (protocol === 'openai' && model.baseUrl) {
      if (!model.openaiResponses) return 'openai';
      if (toolId && RESPONSES_TOOLS.has(toolId)) return 'openai-responses';
    }
  }
  return undefined;
}
