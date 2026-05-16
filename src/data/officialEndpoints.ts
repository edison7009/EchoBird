// Per-tool "official" endpoint registry. The Restore button in App Manager
// uses this to revert a tool from a third-party / proxy URL back to the
// vendor's canonical address. Inspired by cc-switch.
//
// Entries should match the shape of an ApplyModelInput so the existing
// applyModelToTool command can write them straight to the tool's config.

export interface OfficialEndpoint {
  /** Display name shown in UI / toasts */
  name: string;
  /** OpenAI-protocol base URL (or "" if N/A) */
  baseUrl: string;
  /** Anthropic-protocol base URL (optional) */
  anthropicUrl?: string;
  /** Which protocol to write into the tool's config */
  protocol: 'openai' | 'anthropic';
  /**
   * Fallback model id used only when the tool has no model configured yet.
   * Restore preserves the user's existing model where possible — this is
   * the seed for fresh installs, not a forced override.
   */
  modelId?: string;
}

/** Sentinel internalId used to mark "official endpoint" as the pending selection. */
export const officialModelSentinel = (toolId: string) => `__official__${toolId}`;
export const isOfficialModelSentinel = (id: string | null | undefined) =>
  typeof id === 'string' && id.startsWith('__official__');

export const OFFICIAL_ENDPOINTS: Record<string, OfficialEndpoint> = {
  claudecode: {
    name: 'Anthropic Official',
    baseUrl: 'https://api.anthropic.com',
    anthropicUrl: 'https://api.anthropic.com',
    protocol: 'anthropic',
    modelId: 'claude-sonnet-4-5',
  },
  claudedesktop: {
    // Restore for Claude Desktop flips deploymentMode back to '1p' and
    // deletes the 3P profile / relay; the URL fields here are display-only
    // (Desktop's official mode uses Anthropic OAuth, not an API key).
    name: 'Anthropic Official',
    baseUrl: 'https://api.anthropic.com',
    anthropicUrl: 'https://api.anthropic.com',
    protocol: 'anthropic',
    modelId: 'claude-sonnet-4-5',
  },
  codex: {
    name: 'OpenAI Official',
    baseUrl: 'https://api.openai.com/v1',
    protocol: 'openai',
    modelId: 'gpt-4o',
  },
  codexdesktop: {
    name: 'OpenAI Official',
    baseUrl: 'https://api.openai.com/v1',
    protocol: 'openai',
    modelId: 'gpt-4o',
  },
  // OpenCode is a third-party tool, not OpenAI official
  // Community open-source tools (openclaw, zeroclaw, hermes, nanobot,
  // picoclaw, openfang, opencode) have no canonical vendor URL — restore is hidden.
};

export function getOfficialEndpoint(toolId: string): OfficialEndpoint | undefined {
  return OFFICIAL_ENDPOINTS[toolId];
}
