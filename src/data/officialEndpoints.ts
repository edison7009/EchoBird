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
    /** Default model id to write (empty = let the tool fall back to its own default) */
    modelId?: string;
}

export const OFFICIAL_ENDPOINTS: Record<string, OfficialEndpoint> = {
    claudecode: {
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
    opencode: {
        name: 'OpenAI Official',
        baseUrl: 'https://api.openai.com/v1',
        protocol: 'openai',
    },
    // Community open-source tools (openclaw, zeroclaw, hermes, nanobot,
    // picoclaw, openfang) have no canonical vendor URL — restore is hidden.
};

export function getOfficialEndpoint(toolId: string): OfficialEndpoint | undefined {
    return OFFICIAL_ENDPOINTS[toolId];
}
